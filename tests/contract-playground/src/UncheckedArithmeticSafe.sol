// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

// Same operations as UncheckedArithmetic.sol, but solc >= 0.8.0 checks them by default.
contract UncheckedArithmeticSafe {
    mapping(address => uint256) public balances;
    uint256 public totalSupply;
    uint8 public smallCounter;
    uint256 public counter;

    function add(uint256 a, uint256 b) external pure returns (uint256) {
        return a + b;
    }

    function sub(uint256 a, uint256 b) external pure returns (uint256) {
        return a - b;
    }

    function mul(uint256 a, uint256 b) external pure returns (uint256) {
        return a * b;
    }

    function mint(uint256 amount) external {
        balances[msg.sender] += amount;
        totalSupply += amount;
    }

    function burn(uint256 amount) external {
        balances[msg.sender] -= amount;
        totalSupply -= amount;
    }

    function increment() external {
        smallCounter++;
        counter--;
    }
}
