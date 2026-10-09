use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    str::FromStr,
};

use crate::ast::{
    ASTNode, BinaryOperation, Expression, ExtractBinaryOperations, ExtractPragmaDirectives,
    Mutability, NodeID, NodeType,
};

use crate::{
    capture,
    context::{
        browser::{AppearsBeforeNodeLocation, Peek},
        workspace::WorkspaceContext,
    },
    detect::{
        detector::{IssueDetector, IssueDetectorNamePool, IssueSeverity},
        helpers::pragma_directive_to_semver,
    },
};
use eyre::Result;
use semver::{Version, VersionReq};

#[derive(Default)]
pub struct UncheckedArithmeticDetector {
    // Keys are: [0] source file name, [1] line number, [2] character location of node.
    // Do not add items manually, use `capture!` to add nodes to this BTreeMap.
    found_instances: BTreeMap<(String, usize, String), NodeID>,
}

impl IssueDetector for UncheckedArithmeticDetector {
    fn detect(&mut self, context: &WorkspaceContext) -> Result<bool, Box<dyn Error>> {
        // Cache of source unit ID -> whether its pragma allows solc < 0.8.0
        let mut pre_0_8_source_units: HashMap<NodeID, bool> = HashMap::new();

        for binary_operation in context.binary_operations() {
            if !matches!(binary_operation.operator.as_str(), "+" | "-" | "*" | "**") {
                continue;
            }
            // Compile time constant expressions (literals, constants) are evaluated by the
            // compiler, so there is nothing to overflow at runtime.
            if binary_operation.is_pure {
                continue;
            }
            if integer_bits(binary_operation.type_descriptions.type_string.as_deref()).is_none() {
                continue;
            }
            if binary_operation.operator == "+"
                && is_block_value_plus_constant(
                    context,
                    &binary_operation.left_expression,
                    &binary_operation.right_expression,
                )
            {
                continue;
            }
            let guard = (binary_operation.operator == "-").then(|| {
                vec![
                    binary_operation.left_expression.as_ref(),
                    binary_operation.right_expression.as_ref(),
                ]
            });
            if is_candidate(context, binary_operation.id, guard, &mut pre_0_8_source_units) {
                capture!(self, context, binary_operation);
            }
        }

        for assignment in context.assignments() {
            if !matches!(assignment.operator.as_str(), "+=" | "-=" | "*=") {
                continue;
            }
            if integer_bits(assignment.type_descriptions.type_string.as_deref()).is_none() {
                continue;
            }
            let guard = (assignment.operator == "-=").then(|| {
                vec![assignment.left_hand_side.as_ref(), assignment.right_hand_side.as_ref()]
            });
            if is_candidate(context, assignment.id, guard, &mut pre_0_8_source_units) {
                capture!(self, context, assignment);
            }
        }

        for unary_operation in context.unary_operations() {
            let Some(bits) = integer_bits(unary_operation.type_descriptions.type_string.as_deref())
            else {
                continue;
            };
            match unary_operation.operator.as_str() {
                // Incrementing a 256 bit counter one step at a time will never realistically
                // reach the max value, so only smaller types are reported.
                "++" if bits < 256 => {}
                "--" => {}
                _ => continue,
            }
            let guard = Some(vec![unary_operation.sub_expression.as_ref()]);
            if is_candidate(context, unary_operation.id, guard, &mut pre_0_8_source_units) {
                capture!(self, context, unary_operation);
            }
        }

        Ok(!self.found_instances.is_empty())
    }

    fn severity(&self) -> IssueSeverity {
        IssueSeverity::High
    }

    fn title(&self) -> String {
        String::from("Unchecked Arithmetic in Solidity < 0.8.0")
    }

    fn description(&self) -> String {
        String::from(
            "The pragma allows a compiler version below 0.8.0, where arithmetic operations do not revert on overflow or underflow. Values silently wrap around instead. Use SafeMath (or an equivalent check) for these operations, or upgrade to Solidity 0.8.0 or above where arithmetic is checked by default.",
        )
    }

    fn instances(&self) -> BTreeMap<(String, usize, String), NodeID> {
        self.found_instances.clone()
    }

    fn name(&self) -> String {
        IssueDetectorNamePool::UncheckedArithmetic.to_string()
    }
}

/// Common checks for every kind of arithmetic operation.
///
/// `guard_operands` are the operands that, when compared against each other (or, for a single
/// operand, against anything) earlier in the same function, mean the developer already checked
/// the operation. E.g. `require(a >= b); a - b;`
fn is_candidate(
    context: &WorkspaceContext,
    operation_id: NodeID,
    guard_operands: Option<Vec<&Expression>>,
    pre_0_8_source_units: &mut HashMap<NodeID, bool>,
) -> bool {
    let Some(operation) = context.nodes.get(&operation_id) else {
        return false;
    };

    // 1. The file must be compilable with solc < 0.8.0
    let Some(source_unit) = context.get_closest_ancestor(operation_id, NodeType::SourceUnit) else {
        return false;
    };
    let source_unit_id = source_unit.id().unwrap_or_default();
    let allows_pre_0_8 = *pre_0_8_source_units
        .entry(source_unit_id)
        .or_insert_with(|| source_unit_allows_pre_0_8(source_unit));
    if !allows_pre_0_8 {
        return false;
    }

    // 2. Skip SafeMath style libraries. They do the raw operation and check it afterwards.
    if let Some(ASTNode::ContractDefinition(contract)) =
        context.get_closest_ancestor(operation_id, NodeType::ContractDefinition)
        && contract.name.to_lowercase().contains("safemath")
    {
        return false;
    }

    // 3. Skip the update step of a for loop (`i++`, `i += 1`). It is bounded by the loop
    // condition.
    let ancestors = context.get_ancestral_line(operation_id);
    let ancestor_ids: Vec<NodeID> = ancestors.iter().filter_map(|a| a.id()).collect();
    for ancestor in &ancestors {
        if let ASTNode::ForStatement(for_statement) = ancestor
            && let Some(loop_expression) = &for_statement.loop_expression
            && ancestor_ids.contains(&loop_expression.id)
        {
            return false;
        }
    }

    // 4. Skip operations that were explicitly checked earlier in the same function or modifier.
    if let Some(operands) = guard_operands
        && is_guarded(context, operation, operation_id, &operands)
    {
        return false;
    }

    true
}

fn is_guarded(
    context: &WorkspaceContext,
    operation: &ASTNode,
    operation_id: NodeID,
    operands: &[&Expression],
) -> bool {
    let operand_sources: Vec<String> =
        operands.iter().filter_map(|o| o.peek(context)).map(|s| normalize(&s)).collect();
    if operand_sources.len() != operands.len() || operand_sources.is_empty() {
        return false;
    }

    let enclosing = context
        .get_closest_ancestor(operation_id, NodeType::FunctionDefinition)
        .or_else(|| context.get_closest_ancestor(operation_id, NodeType::ModifierDefinition));
    let Some(enclosing) = enclosing else {
        return false;
    };

    ExtractBinaryOperations::from(enclosing).extracted.iter().any(|comparison| {
        is_comparison(comparison)
            && comparison.appears_before(context, operation) == Some(true)
            && compares_operands(context, comparison, &operand_sources)
    })
}

fn is_comparison(binary_operation: &BinaryOperation) -> bool {
    matches!(binary_operation.operator.as_str(), ">" | ">=" | "<" | "<=")
}

fn compares_operands(
    context: &WorkspaceContext,
    comparison: &BinaryOperation,
    operand_sources: &[String],
) -> bool {
    let (Some(left), Some(right)) =
        (comparison.left_expression.peek(context), comparison.right_expression.peek(context))
    else {
        return false;
    };
    let (left, right) = (normalize(&left), normalize(&right));
    match operand_sources {
        // `x--`: any comparison involving `x`
        [single] => &left == single || &right == single,
        // `a - b`: a comparison between `a` and `b`, either way around
        [a, b] => (&left == a && &right == b) || (&left == b && &right == a),
        _ => false,
    }
}

/// `block.timestamp + 1 days`, `block.number + 10`, `now + DELAY`: a uint256 block value plus a
/// compile time constant can not realistically overflow.
fn is_block_value_plus_constant(
    context: &WorkspaceContext,
    left: &Expression,
    right: &Expression,
) -> bool {
    (is_block_value(left) && is_compile_time_constant(context, right))
        || (is_block_value(right) && is_compile_time_constant(context, left))
}

fn is_block_value(expression: &Expression) -> bool {
    match expression {
        Expression::MemberAccess(member_access) => {
            matches!(member_access.member_name.as_str(), "timestamp" | "number")
                && matches!(
                    member_access.expression.as_ref(),
                    Expression::Identifier(identifier) if identifier.name == "block"
                )
        }
        Expression::Identifier(identifier) => identifier.name == "now",
        _ => false,
    }
}

fn is_compile_time_constant(context: &WorkspaceContext, expression: &Expression) -> bool {
    match expression {
        Expression::Literal(_) => true,
        Expression::BinaryOperation(binary_operation) => binary_operation.is_pure,
        Expression::TupleExpression(tuple_expression) => tuple_expression.is_pure,
        Expression::Identifier(identifier) => identifier
            .referenced_declaration
            .and_then(|id| context.nodes.get(&id))
            .is_some_and(|node| {
                matches!(
                    node,
                    ASTNode::VariableDeclaration(variable)
                        if variable.mutability() == Some(&Mutability::Constant)
                )
            }),
        _ => false,
    }
}

fn normalize(source: &str) -> String {
    source.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Number of bits of an integer type string (`uint8` -> 8, `int` -> 256).
/// Returns `None` for anything that is not a sized integer, including `int_const ...`.
fn integer_bits(type_string: Option<&str>) -> Option<u16> {
    let type_string = type_string?;
    let rest = type_string.strip_prefix("uint").or_else(|| type_string.strip_prefix("int"))?;
    if rest.is_empty() {
        return Some(256);
    }
    rest.parse::<u16>().ok()
}

/// True if at least one solc version below 0.8.0 satisfies every `pragma solidity` in the file.
fn source_unit_allows_pre_0_8(source_unit: &ASTNode) -> bool {
    let mut version_reqs: Vec<VersionReq> = vec![];
    for pragma_directive in ExtractPragmaDirectives::from(source_unit).extracted {
        if pragma_directive.literals.first().map(String::as_str) != Some("solidity") {
            continue;
        }
        match pragma_directive_to_semver(&pragma_directive) {
            Ok(version_req) => version_reqs.push(version_req),
            Err(_) => return false,
        }
    }
    if version_reqs.is_empty() {
        return false;
    }
    // Last patch release of each minor version before 0.8.0
    [(4, 26), (5, 17), (6, 12), (7, 6)].iter().any(|(minor, last_patch)| {
        (0..=*last_patch).any(|patch| {
            let version = Version::from_str(&format!("0.{minor}.{patch}")).unwrap();
            version_reqs.iter().all(|req| req.matches(&version))
        })
    })
}

#[cfg(test)]
mod unchecked_arithmetic_tests {

    use crate::detect::{
        detector::IssueDetector,
        high::unchecked_arithmetic::{UncheckedArithmeticDetector, integer_bits},
    };

    #[test]
    fn test_integer_bits() {
        assert_eq!(integer_bits(Some("uint8")), Some(8));
        assert_eq!(integer_bits(Some("int128")), Some(128));
        assert_eq!(integer_bits(Some("uint")), Some(256));
        assert_eq!(integer_bits(Some("int256")), Some(256));
        assert_eq!(integer_bits(Some("int_const 5")), None);
        assert_eq!(integer_bits(Some("address")), None);
        assert_eq!(integer_bits(None), None);
    }

    #[test]
    fn test_unchecked_arithmetic() {
        let context = crate::detect::test_utils::load_solidity_source_unit(
            "../tests/contract-playground/src/UncheckedArithmetic.sol",
        );

        let mut detector = UncheckedArithmeticDetector::default();
        let found = detector.detect(&context).unwrap();
        assert!(found);

        let mut lines: Vec<usize> = detector.instances().keys().map(|(_, line, _)| *line).collect();
        lines.sort();
        // One instance per line marked `// BAD` in the contract
        assert_eq!(lines, vec![38, 42, 46, 50, 54, 58, 62, 66, 70, 74, 78]);
    }

    #[test]
    fn test_unchecked_arithmetic_not_reported_for_0_8() {
        let context = crate::detect::test_utils::load_solidity_source_unit(
            "../tests/contract-playground/src/UncheckedArithmeticSafe.sol",
        );

        let mut detector = UncheckedArithmeticDetector::default();
        let found = detector.detect(&context).unwrap();
        assert!(!found);
        assert_eq!(detector.instances().len(), 0);
    }
}
