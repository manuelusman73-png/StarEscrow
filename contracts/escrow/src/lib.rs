#![no_std]

mod errors;
mod events;
mod storage;
mod r#yield;

pub use storage::{EscrowData, EscrowStatus, YieldRecipient};

use crate::errors::EscrowError;
use crate::r#yield::YieldProtocolClient;
use crate::storage::RateLimitConfig;

use soroban_sdk::{contract, contractimpl, token, Address, Env, String, Symbol};

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Create escrow: payer locks `amount` of `token` for `freelancer`.
    /// Optional `deadline` is a ledger timestamp after which payer can reclaim funds.
    /// Optional `yield_protocol` deposits locked funds for yield.
    /// `yield_recipient` receives accrued yield on withdraw.
    pub fn create(
        env: Env,
        payer: Address,
        freelancer: Address,
        token: Address,
        amount: i128,
        milestone: String,
        deadline: Option<u64>,
        yield_protocol: Option<Address>,
        yield_recipient: YieldRecipient,
    ) -> Result<(), EscrowError> {
        let config = storage::read_config(&env).ok_or(EscrowError::ConfigNotSet)?;
        storage::check_and_update_rate_limit(&env, payer.clone(), config.clone())
            .map_err(|_| EscrowError::RateLimitExceeded)?;

        if storage::has_escrow(&env) {
            return Err(EscrowError::AlreadyExists);
        }
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }

        let allowed = storage::read_allowed_tokens(&env);
        if !allowed.is_empty() && !allowed.contains(&token) {
            return Err(EscrowError::TokenNotAllowed);
        }

        payer.require_auth();

        let client = token::Client::new(&env, &token);
        client.transfer(&payer, &env.current_contract_address(), &amount);

        let mut data = EscrowData {
            payer: payer.clone(),
            freelancer: freelancer.clone(),
            token,
            amount,
            milestone: milestone.clone(),
            status: EscrowStatus::Active,
            deadline,
            yield_protocol,
            principal_deposited: 0i128,
            yield_recipient,
        };

        // Deposit to yield protocol if enabled
        if let Some(protocol) = data.yield_protocol {
            let yield_client = YieldProtocolClient::new(&env, &protocol);
            yield_client.deposit(&amount);
            events::yield_deposited(&env, &protocol, amount);
            data.principal_deposited = amount;
        }

        storage::save_escrow(&env, &data);
        events::escrow_created(&env, &payer, &freelancer, amount, &milestone);
        Ok(())
    }

    fn withdraw_funds(
        env: &Env,
        data: &mut EscrowData,
        recipient: Address,
    ) -> Result<(i128, i128), EscrowError> {
        let client = token::Client::new(env, &data.token);

        if let Some(protocol) = data.yield_protocol {
            if data.principal_deposited == 0 {
                return Err(EscrowError::YieldNotEnabled);
            }

            let yield_client = YieldProtocolClient::new(env, &protocol);
            let (prin, yield_amt) = yield_client.withdraw(&data.principal_deposited);
            if prin < data.principal_deposited {
                return Err(EscrowError::YieldWithdrawFailed);
            }

            // Principal to recipient, yield to yield_recipient
            client.transfer(
                &env.current_contract_address(),
                &recipient,
                &data.principal_deposited,
            );
            if yield_amt > 0 {
                let yield_addr = match data.yield_recipient {
                    YieldRecipient::Payer => data.payer.clone(),
                    YieldRecipient::Freelancer => data.freelancer.clone(),
                };
                client.transfer(&env.current_contract_address(), &yield_addr, &yield_amt);
            }

            events::yield_withdrawn(env, data.principal_deposited, yield_amt);

            (data.principal_deposited, yield_amt)
        } else {
            // No yield, transfer amount from contract balance
            client.transfer(&env.current_contract_address(), &recipient, &data.amount);
            (data.amount, 0)
        }
    }

    /// Payer approves milestone — releases funds to freelancer.
    pub fn approve(env: Env) -> Result<(), EscrowError> {
        let mut data = storage::load_escrow(&env);
        if data.status != EscrowStatus::WorkSubmitted {
            return Err(EscrowError::WorkNotSubmitted);
        }
        data.payer.require_auth();

        Self::withdraw_funds(&env, &mut data, data.freelancer.clone())?;

        events::payment_released(&env, &data.freelancer, data.amount);
        data.status = EscrowStatus::Completed;
        storage::save_escrow(&env, &data);
        Ok(())
    }

    /// Payer cancels escrow — refunds locked funds. Only allowed before work is submitted.
    pub fn cancel(env: Env) -> Result<(), EscrowError> {
        let mut data = storage::load_escrow(&env);
        if data.status != EscrowStatus::Active {
            return Err(EscrowError::NotActive);
        }
        data.payer.require_auth();

        Self::withdraw_funds(&env, &mut data, data.payer.clone())?;

        events::escrow_cancelled(&env, &data.payer, data.amount);
        data.status = EscrowStatus::Cancelled;
        storage::save_escrow(&env, &data);
        Ok(())
    }

    /// Payer reclaims funds after the deadline has passed.
    pub fn expire(env: Env) -> Result<(), EscrowError> {
        let mut data = storage::load_escrow(&env);
        if data.status != EscrowStatus::Active {
            return Err(EscrowError::NotActive);
        }

        let deadline = match data.deadline {
            Some(d) => d,
            None => return Err(EscrowError::NotExpired),
        };

        if env.ledger().timestamp() <= deadline {
            return Err(EscrowError::DeadlineNotPassed);
        }

        data.payer.require_auth();

        Self::withdraw_funds(&env, &mut data, data.payer.clone())?;

        events::escrow_expired(&env, &data.payer, data.amount);
        data.status = EscrowStatus::Expired;
        storage::save_escrow(&env, &data);
        Ok(())
    }

    pub fn set_rate_limit_config(
        env: Env,
        min_amount: i128,
        max_amount: i128,
        max_per_window: u32,
        window_duration: u64,
    ) -> Result<(), EscrowError> {
        let caller = env.invoker();
        caller.require_auth();

        storage::read_config(&env).map_or_else(
            || {
                let config = RateLimitConfig {
                    admin: caller.clone(),
                    max_per_window,
                    window_duration,
                };
                storage::write_config(&env, &config);
            },
            |_| (),
        );

        let mut config = storage::read_config(&env).unwrap();
        config.max_per_window = max_per_window;
        config.window_duration = window_duration;
        storage::write_config(&env, &config);
        Ok(())
    }

    /// Returns the current status without the full struct — useful for lightweight UI queries.
    pub fn get_status(env: Env) -> EscrowStatus {
        storage::load_escrow(&env).status
    }

    /// Read full escrow state.
    pub fn get_escrow(env: Env) -> EscrowData {
        storage::load_escrow(&env)
    }

    /// Admin adds token to allowlist.
    pub fn add_token(env: Env, token: Address) -> Result<(), EscrowError> {
        let caller = env.invoker_address();
        caller.require_auth();

        let config = storage::read_config(&env).ok_or(EscrowError::ConfigNotSet)?;
        if caller != config.admin {
            return Err(EscrowError::RateLimitExceeded); // Temp unauthorized
        }

        if storage::add_to_allowlist(&env, token) {
            Ok(())
        } else {
            Err(EscrowError::AlreadyExists)
        }
    }

    /// Admin removes token from allowlist.
    pub fn remove_token(env: Env, token: Address) -> Result<(), EscrowError> {
        let caller = env.invoker_address();
        caller.require_auth();

        let config = storage::read_config(&env).ok_or(EscrowError::ConfigNotSet)?;
        if caller != config.admin {
            return Err(EscrowError::RateLimitExceeded);
        }

        if storage::remove_from_allowlist(&env, token) {
            Ok(())
        } else {
            Err(EscrowError::NotActive)
        }
    }

    /// Returns list of allowed tokens.
    pub fn get_allowed_tokens(env: Env) -> Vec<Address> {
        storage::read_allowed_tokens(&env)
    }
}
