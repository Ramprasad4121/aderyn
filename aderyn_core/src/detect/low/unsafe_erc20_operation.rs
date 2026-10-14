use std::{collections::BTreeMap, error::Error};

use crate::{
    ast::{ASTNode, Expression, FunctionCall, MemberAccess, NodeID},
    capture,
    context::{browser::GetImmediateParent, workspace::WorkspaceContext},
    detect::detector::{IssueDetector, IssueDetectorNamePool, IssueSeverity},
};
use eyre::Result;

#[derive(Default)]
pub struct UnsafeERC20OperationDetector {
    // Keys are: [0] source file name, [1] line number, [2] character location of node.
    // Do not add items manually, use `capture!` to add nodes to this BTreeMap.
    found_instances: BTreeMap<(String, usize, String), NodeID>,
}

impl IssueDetector for UnsafeERC20OperationDetector {
    fn detect(&mut self, context: &WorkspaceContext) -> Result<bool, Box<dyn Error>> {
        for member_access in context.member_accesss() {
            if member_access.expression.as_ref().type_descriptions().is_some_and(|desc| {
                desc.type_string.as_ref().is_some_and(|type_string| type_string.contains("ERC20"))
            }) && member_access.member_name == "transferFrom"
                || member_access.member_name == "approve"
                || member_access.member_name == "transfer"
            {
                if is_reference_outside_raw_call(member_access, context) {
                    continue;
                }
                capture!(self, context, member_access);
            }
        }
        Ok(!self.found_instances.is_empty())
    }

    fn title(&self) -> String {
        String::from("Unsafe ERC20 Operation")
    }

    fn description(&self) -> String {
        String::from(
            "ERC20 functions may not behave as expected. For example: return values are not always meaningful. It is recommended to use OpenZeppelin's SafeERC20 library.",
        )
    }

    fn severity(&self) -> IssueSeverity {
        IssueSeverity::Low
    }

    fn instances(&self) -> BTreeMap<(String, usize, String), NodeID> {
        self.found_instances.clone()
    }

    fn name(&self) -> String {
        format!("{}", IssueDetectorNamePool::UnsafeERC20Operation)
    }
}

/// Returns true when the ERC20 function is only referenced, not called, and the encoded payload
/// is not passed straight into a low level `call`. For example `IERC20.transfer.selector` handed
/// to a helper that checks the return data (see issue #548), or a selector stored in a constant.
fn is_reference_outside_raw_call(member_access: &MemberAccess, context: &WorkspaceContext) -> bool {
    // The node that uses the function as a value. Either `IERC20.transfer.selector` or
    // `IERC20.transfer` passed as an argument, like in `abi.encodeCall(IERC20.transfer, (..))`
    let reference_id = match member_access.parent(context) {
        Some(ASTNode::MemberAccess(parent)) if parent.member_name == "selector" => parent.id,
        Some(ASTNode::FunctionCall(parent))
            if parent.arguments.iter().any(|arg| arg.get_node_id() == Some(member_access.id)) =>
        {
            member_access.id
        }
        _ => return false,
    };

    let Some(reference) = context.nodes.get(&reference_id) else {
        return false;
    };

    // Look for `token.call(abi.encodeWithSelector(IERC20.transfer.selector, ..))`
    if let Some(ASTNode::FunctionCall(encoder)) = reference.parent(context)
        && encoder.arguments.iter().any(|arg| arg.get_node_id() == Some(reference_id))
        && let Some(ASTNode::FunctionCall(outer)) = encoder.parent(context)
        && outer.arguments.iter().any(|arg| arg.get_node_id() == Some(encoder.id))
        && is_low_level_call(outer)
    {
        return false;
    }

    true
}

/// `addr.call(..)` or `addr.call{..}(..)`
fn is_low_level_call(function_call: &FunctionCall) -> bool {
    let callee = match function_call.expression.as_ref() {
        Expression::FunctionCallOptions(options) => options.expression.as_ref(),
        expression => expression,
    };
    let Expression::MemberAccess(member_access) = callee else {
        return false;
    };
    member_access.member_name == "call"
        && member_access.expression.type_descriptions().is_some_and(|desc| {
            desc.type_string.as_ref().is_some_and(|type_string| {
                type_string == "address" || type_string == "address payable"
            })
        })
}

#[cfg(test)]
mod unsafe_erc20_functions_tests {
    use crate::detect::detector::IssueDetector;

    use super::UnsafeERC20OperationDetector;

    #[test]
    fn test_unsafe_erc20_functions_by_loading_contract_directly() {
        let context = crate::detect::test_utils::load_solidity_source_unit(
            "../tests/contract-playground/src/DeprecatedOZFunctions.sol",
        );

        let mut detector = UnsafeERC20OperationDetector::default();
        let found = detector.detect(&context).unwrap();
        assert!(found);
        // failure0, failure1 and failure3
        assert_eq!(detector.instances().len(), 5);
    }

    #[test]
    fn test_unsafe_erc20_selector_only_flagged_in_raw_calls() {
        let context = crate::detect::test_utils::load_solidity_source_unit(
            "../tests/contract-playground/src/UnsafeERC20Selector.sol",
        );

        let mut detector = UnsafeERC20OperationDetector::default();
        let found = detector.detect(&context).unwrap();
        assert!(found);
        // rawTransfer, rawApprove, rawTransferWithGas and directTransfer
        assert_eq!(detector.instances().len(), 4);
    }
}
