// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

interface IERC20Like {
    function transfer(address to, uint256 amount) external returns (bool);
    function approve(address spender, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
}

library SafeCallHelper {
    function callWithSafeReturnData(bytes memory payload, address target)
        internal
        returns (bool success, bytes memory data)
    {
        (success, data) = target.call(payload);
        if (success && data.length > 0) {
            success = abi.decode(data, (bool));
        }
    }
}

contract UnsafeERC20Selector {
    // Good: only reads the selector, nothing is called
    bytes4 public constant TRANSFER_SELECTOR = IERC20Like.transfer.selector;

    // Good: payload goes to a helper that checks the return data (issue #548)
    function transferViaHelper(address token, address to, uint256 amount) external returns (bool success) {
        (success,) = SafeCallHelper.callWithSafeReturnData(
            abi.encodeWithSelector(IERC20Like.transfer.selector, to, amount), token
        );
    }

    // Good: same thing with abi.encodeCall
    function approveViaHelper(address token, address spender, uint256 amount) external returns (bool success) {
        (success,) = SafeCallHelper.callWithSafeReturnData(abi.encodeCall(IERC20Like.approve, (spender, amount)), token);
    }

    // Good: comparing selectors
    function isTransferFrom(bytes4 sig) external pure returns (bool) {
        return sig == IERC20Like.transferFrom.selector;
    }

    // Bad: selector used directly in a raw call
    function rawTransfer(address token, address to, uint256 amount) external {
        (bool ok,) = token.call(abi.encodeWithSelector(IERC20Like.transfer.selector, to, amount));
        require(ok);
    }

    // Bad: abi.encodeCall used directly in a raw call
    function rawApprove(address token, address spender, uint256 amount) external {
        (bool ok,) = token.call(abi.encodeCall(IERC20Like.approve, (spender, amount)));
        require(ok);
    }

    // Bad: raw call with call options
    function rawTransferWithGas(address token, address to, uint256 amount) external {
        (bool ok,) = token.call{gas: 50_000}(abi.encodeWithSelector(IERC20Like.transfer.selector, to, amount));
        require(ok);
    }

    // Bad: direct call
    function directTransfer(IERC20Like token, address to, uint256 amount) external {
        token.transfer(to, amount);
    }
}
