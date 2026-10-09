// SPDX-License-Identifier: MIT
pragma solidity ^0.7.0;

library SafeMath {
    function add(uint256 a, uint256 b) internal pure returns (uint256) {
        uint256 c = a + b;
        require(c >= a, "SafeMath: addition overflow");
        return c;
    }

    function sub(uint256 a, uint256 b) internal pure returns (uint256) {
        require(b <= a, "SafeMath: subtraction overflow");
        return a - b;
    }

    function mul(uint256 a, uint256 b) internal pure returns (uint256) {
        if (a == 0) {
            return 0;
        }
        uint256 c = a * b;
        require(c / a == b, "SafeMath: multiplication overflow");
        return c;
    }
}

contract UncheckedArithmetic {
    using SafeMath for uint256;

    uint256 public constant ONE_TOKEN = 10 ** 18;
    uint256 public constant TWO_TOKENS = ONE_TOKEN * 2;

    mapping(address => uint256) public balances;
    uint256 public totalSupply;
    uint8 public smallCounter;
    uint256 public counter;

    function badAdd(uint256 a, uint256 b) external pure returns (uint256) {
        return a + b; // BAD
    }

    function badSub(uint256 a, uint256 b) external pure returns (uint256) {
        return a - b; // BAD
    }

    function badMul(uint256 a, uint256 b) external pure returns (uint256) {
        return a * b; // BAD
    }

    function badExp(uint256 a, uint256 b) external pure returns (uint256) {
        return a ** b; // BAD
    }

    function badSignedSub(int256 a, int256 b) external pure returns (int256) {
        return a - b; // BAD
    }

    function badMint(uint256 amount) external {
        balances[msg.sender] += amount; // BAD
    }

    function badBurn(uint256 amount) external {
        totalSupply -= amount; // BAD
    }

    function badScale(uint256 factor) external {
        totalSupply *= factor; // BAD
    }

    function badSmallIncrement() external {
        smallCounter++; // BAD
    }

    function badDecrement() external {
        counter--; // BAD
    }

    function badCheckAfterTheFact(uint256 a, uint256 b) external pure returns (uint256) {
        uint256 c = a - b; // BAD
        require(a >= b, "checked too late");
        return c;
    }

    // ------------------------------------------------------------------
    // Nothing below this line should be reported
    // ------------------------------------------------------------------

    function goodSafeMath(uint256 a, uint256 b) external pure returns (uint256) {
        return a.add(b).sub(1).mul(2);
    }

    function goodConstants() external pure returns (uint256) {
        return ONE_TOKEN + TWO_TOKENS + 1 + 2 * 3;
    }

    function goodGuardedSub(uint256 a, uint256 b) external pure returns (uint256) {
        require(a >= b, "underflow");
        return a - b;
    }

    function goodGuardedSubIf(uint256 a, uint256 b) external pure returns (uint256) {
        if (b > a) {
            return 0;
        }
        return a - b;
    }

    function goodGuardedWithdraw(uint256 amount) external {
        require(balances[msg.sender] >= amount, "insufficient balance");
        balances[msg.sender] -= amount;
    }

    function goodGuardedDecrement() external {
        require(counter > 0, "nothing to decrement");
        counter--;
    }

    function goodLoop(uint256[] calldata values) external pure returns (uint256 total) {
        for (uint256 i = 0; i < values.length; i++) {
            total = total.add(values[i]);
        }
        for (uint8 j = 0; j < 10; j += 1) {
            total = total.add(j);
        }
    }

    function goodWideIncrement() external {
        counter++;
    }

    function goodDivision(uint256 a, uint256 b) external pure returns (uint256) {
        return (a / b) % 7;
    }

    function goodDeadline() external view returns (uint256 deadline, uint256 later) {
        deadline = block.timestamp + 7 days;
        later = block.number + ONE_TOKEN;
    }

    function goodComparison(uint256 a, uint256 b) external pure returns (bool) {
        return a < b;
    }
}
