use soroban_sdk::{contractclient, Env};

#[contractclient(name = "YieldProtocolClient")]
pub trait YieldProtocol {
    /// Deposit amount from caller to this contract.
    fn deposit(env: Env, amount: i128);

    /// Withdraw requested principal, returns (principal_returned, yield_accrued).
    fn withdraw(env: Env, requested: i128) -> (i128, i128);
}
