use soroban_sdk::{Address, Env, String, Symbol};

pub fn escrow_created(
    env: &Env,
    payer: &Address,
    freelancer: &Address,
    amount: i128,
    milestone: &String,
) {
    env.events().publish(
        (Symbol::new(env, "escrow_created"),),
        (payer.clone(), freelancer.clone(), amount, milestone.clone()),
    );
}

pub fn work_submitted(env: &Env, freelancer: &Address) {
    env.events()
        .publish((Symbol::new(env, "work_submitted"),), (freelancer.clone(),));
}

pub fn payment_released(env: &Env, freelancer: &Address, amount: i128) {
    env.events().publish(
        (Symbol::new(env, "payment_released"),),
        (freelancer.clone(), amount),
    );
}

pub fn escrow_cancelled(env: &Env, payer: &Address, amount: i128) {
    env.events().publish(
        (Symbol::new(env, "escrow_cancelled"),),
        (payer.clone(), amount),
    );
}

pub fn escrow_expired(env: &Env, payer: &Address, amount: i128) {
    env.events().publish(
        (Symbol::new(env, "escrow_expired"),),
        (payer.clone(), amount),
    );
}

pub fn yield_deposited(env: &Env, protocol: &Address, principal: i128) {
    env.events().publish(
        (Symbol::new(env, "yield_deposited"),),
        (protocol.clone(), principal),
    );
}

pub fn yield_withdrawn(env: &Env, principal: i128, yield_amount: i128) {
    env.events().publish(
        (Symbol::new(env, "yield_withdrawn"),),
        (principal, yield_amount),
    );
}
