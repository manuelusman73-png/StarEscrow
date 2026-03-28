#![cfg(test)]

use escrow::{EscrowContract, EscrowContractClient, EscrowError, EscrowStatus, YieldRecipient};
use soroban_sdk::{
    testutils::Ledger as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String, Vec,
};

fn create_token<'a>(env: &Env, admin: &Address) -> (TokenClient<'a>, StellarAssetClient<'a>) {
    let token_addr = env.register_stellar_asset_contract_v2(admin.clone());
    (
        TokenClient::new(env, &token_addr.address()),
        StellarAssetClient::new(env, &token_addr.address()),
    )
}

struct Setup<'a> {
    env: Env,
    payer: Address,
    freelancer: Address,
    token: TokenClient<'a>,
    token_addr: Address,
    contract: EscrowContractClient<'a>,
    admin: Address,
}

impl<'a> Setup<'a> {
    fn new() -> Self {
        Self::with_fee(0)
    }

    fn with_fee(fee_bps: u32) -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let payer = Address::generate(&env);
        let freelancer = Address::generate(&env);
        let admin = Address::generate(&env);
        let fee_collector = Address::generate(&env);

        let (token, token_admin) = create_token(&env, &admin);
        let token_addr = token.address.clone();

        token_admin.mint(&payer, &1000);

        let contract_addr = env.register_contract(None, EscrowContract);
        let contract = EscrowContractClient::new(&env, &contract_addr);

        contract.init(&admin, &fee_bps, &fee_collector);

        Setup { env, payer, freelancer, token, token_addr, contract, admin }
    }

    /// Helper: create a simple single-payer escrow (no multisig, no yield).
    fn simple_create(&self, amount: i128, milestone: &str) {
        let m = String::from_str(&self.env, milestone);
        let approvers: Vec<Address> = Vec::new(&self.env);
        self.contract.create(
            &self.payer,
            &self.freelancer,
            &self.token_addr,
            &amount,
            &m,
            &None,
            &None,
            &YieldRecipient::Payer,
            &approvers,
            &1u32,
        );
    }

    /// Helper: approve as payer (single-payer mode).
    fn payer_approve(&self) {
        self.contract.approve(&self.payer);
    }
}

// ── Happy path ────────────────────────────────────────────────────────────────

#[test]
fn test_full_happy_path() {
    let s = Setup::new();
    s.simple_create(500, "Deliver MVP");

    assert_eq!(s.token.balance(&s.payer), 500);
    assert_eq!(s.token.balance(&s.contract.address), 500);

    s.contract.submit_work();
    s.payer_approve();

    assert_eq!(s.token.balance(&s.freelancer), 500);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn test_cancel_refunds_payer() {
    let s = Setup::new();
    s.simple_create(300, "Design mockups");
    assert_eq!(s.token.balance(&s.payer), 700);

    s.contract.cancel();

    assert_eq!(s.token.balance(&s.payer), 1000);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

// ── Error cases ───────────────────────────────────────────────────────────────

#[test]
fn test_approve_before_submit_fails() {
    let s = Setup::new();
    s.simple_create(100, "Approve before submit");

    let err = s.contract.try_approve(&s.payer).unwrap_err().unwrap();
    assert_eq!(err, EscrowError::WorkNotSubmitted);
}

#[test]
fn test_cancel_after_submit_fails() {
    let s = Setup::new();
    s.simple_create(200, "Write tests");
    s.contract.submit_work();

    let err = s.contract.try_cancel().unwrap_err().unwrap();
    assert_eq!(err, EscrowError::NotActive);
}

#[test]
fn test_double_create_fails() {
    let s = Setup::new();
    s.simple_create(100, "First");

    let m = String::from_str(&s.env, "Second");
    let approvers: Vec<Address> = Vec::new(&s.env);
    let err = s.contract
        .try_create(
            &s.payer,
            &s.freelancer,
            &s.token_addr,
            &100,
            &m,
            &None,
            &None,
            &YieldRecipient::Payer,
            &approvers,
            &1u32,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, EscrowError::AlreadyExists);
}

#[test]
fn test_invalid_amount_fails() {
    let s = Setup::new();
    let m = String::from_str(&s.env, "Bad amount");
    let approvers: Vec<Address> = Vec::new(&s.env);
    let err = s.contract
        .try_create(
            &s.payer,
            &s.freelancer,
            &s.token_addr,
            &0,
            &m,
            &None,
            &None,
            &YieldRecipient::Payer,
            &approvers,
            &1u32,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, EscrowError::InvalidAmount);
}

#[test]
fn test_expire_before_deadline_fails() {
    let s = Setup::new();
    s.env.ledger().with_mut(|l| l.timestamp = 100);
    let m = String::from_str(&s.env, "Expire test");
    let approvers: Vec<Address> = Vec::new(&s.env);
    s.contract.create(
        &s.payer,
        &s.freelancer,
        &s.token_addr,
        &100,
        &m,
        &Some(500u64),
        &None,
        &YieldRecipient::Payer,
        &approvers,
        &1u32,
    );

    let err = s.contract.try_expire().unwrap_err().unwrap();
    assert_eq!(err, EscrowError::DeadlineNotPassed);
}

// ── Status lifecycle ──────────────────────────────────────────────────────────

#[test]
fn test_get_status_lifecycle() {
    let s = Setup::new();
    s.simple_create(100, "Status test");
    assert_eq!(s.contract.get_status(), EscrowStatus::Active);

    s.contract.submit_work();
    assert_eq!(s.contract.get_status(), EscrowStatus::WorkSubmitted);

    s.payer_approve();
    assert_eq!(s.contract.get_status(), EscrowStatus::Completed);
}

#[test]
fn test_get_status_expired() {
    let s = Setup::new();
    s.env.ledger().with_mut(|l| l.timestamp = 100);
    let m = String::from_str(&s.env, "Expired status");
    let approvers: Vec<Address> = Vec::new(&s.env);
    s.contract.create(
        &s.payer,
        &s.freelancer,
        &s.token_addr,
        &100,
        &m,
        &Some(500u64),
        &None,
        &YieldRecipient::Payer,
        &approvers,
        &1u32,
    );
    s.env.ledger().with_mut(|l| l.timestamp = 1000);
    s.contract.expire();
    assert_eq!(s.contract.get_status(), EscrowStatus::Expired);
}

// ── transfer_freelancer ───────────────────────────────────────────────────────

#[test]
fn test_transfer_freelancer_and_submit_work() {
    let s = Setup::new();
    let new_freelancer = Address::generate(&s.env);
    s.simple_create(400, "Subcontract work");

    s.contract.transfer_freelancer(&new_freelancer);
    s.contract.submit_work();
    s.payer_approve();

    assert_eq!(s.token.balance(&new_freelancer), 400);
    assert_eq!(s.token.balance(&s.freelancer), 0);
}

// ── pause / unpause ───────────────────────────────────────────────────────────

#[test]
fn test_pause_blocks_submit_work() {
    let s = Setup::new();
    s.simple_create(100, "Paused submit");
    s.contract.pause();

    let err = s.contract.try_submit_work().unwrap_err().unwrap();
    assert_eq!(err, EscrowError::Paused);
}

#[test]
fn test_unpause_restores_operations() {
    let s = Setup::new();
    s.contract.pause();
    s.contract.unpause();

    s.simple_create(100, "Unpause test");
    s.contract.submit_work();
    s.payer_approve();

    assert_eq!(s.token.balance(&s.freelancer), 100);
}

// ── fee mechanism ─────────────────────────────────────────────────────────────

#[test]
fn test_fee_deducted_on_approve() {
    let s = Setup::with_fee(100); // 1%
    s.simple_create(500, "Fee test");
    s.contract.submit_work();
    s.payer_approve();

    assert_eq!(s.token.balance(&s.freelancer), 495);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn test_zero_fee_full_payment() {
    let s = Setup::new();
    s.simple_create(500, "Zero fee");
    s.contract.submit_work();
    s.payer_approve();

    assert_eq!(s.token.balance(&s.freelancer), 500);
}

// ── Multisig: 2-of-3 ─────────────────────────────────────────────────────────

#[test]
fn test_multisig_2_of_3_releases_on_second_approval() {
    let s = Setup::new();
    let a1 = Address::generate(&s.env);
    let a2 = Address::generate(&s.env);
    let a3 = Address::generate(&s.env);

    let mut approvers = Vec::new(&s.env);
    approvers.push_back(a1.clone());
    approvers.push_back(a2.clone());
    approvers.push_back(a3.clone());

    let m = String::from_str(&s.env, "2-of-3 milestone");
    s.contract.create(
        &s.payer,
        &s.freelancer,
        &s.token_addr,
        &600,
        &m,
        &None,
        &None,
        &YieldRecipient::Payer,
        &approvers,
        &2u32,
    );

    s.contract.submit_work();

    // First approval — not yet released
    s.contract.approve(&a1);
    assert_eq!(s.contract.get_status(), EscrowStatus::WorkSubmitted);
    assert_eq!(s.token.balance(&s.freelancer), 0);

    // Second approval — threshold met, funds released
    s.contract.approve(&a2);
    assert_eq!(s.contract.get_status(), EscrowStatus::Completed);
    assert_eq!(s.token.balance(&s.freelancer), 600);
}

#[test]
fn test_multisig_duplicate_approval_rejected() {
    let s = Setup::new();
    let a1 = Address::generate(&s.env);
    let a2 = Address::generate(&s.env);

    let mut approvers = Vec::new(&s.env);
    approvers.push_back(a1.clone());
    approvers.push_back(a2.clone());

    let m = String::from_str(&s.env, "Dup approval");
    s.contract.create(
        &s.payer,
        &s.freelancer,
        &s.token_addr,
        &100,
        &m,
        &None,
        &None,
        &YieldRecipient::Payer,
        &approvers,
        &2u32,
    );
    s.contract.submit_work();
    s.contract.approve(&a1);

    let err = s.contract.try_approve(&a1).unwrap_err().unwrap();
    assert_eq!(err, EscrowError::AlreadyApproved);
}

#[test]
fn test_multisig_non_approver_rejected() {
    let s = Setup::new();
    let a1 = Address::generate(&s.env);
    let outsider = Address::generate(&s.env);

    let mut approvers = Vec::new(&s.env);
    approvers.push_back(a1.clone());

    let m = String::from_str(&s.env, "Unauthorized approver");
    s.contract.create(
        &s.payer,
        &s.freelancer,
        &s.token_addr,
        &100,
        &m,
        &None,
        &None,
        &YieldRecipient::Payer,
        &approvers,
        &1u32,
    );
    s.contract.submit_work();

    let err = s.contract.try_approve(&outsider).unwrap_err().unwrap();
    assert_eq!(err, EscrowError::Unauthorized);
}

// ── Multisig: 3-of-3 ─────────────────────────────────────────────────────────

#[test]
fn test_multisig_3_of_3_requires_all_approvals() {
    let s = Setup::new();
    let a1 = Address::generate(&s.env);
    let a2 = Address::generate(&s.env);
    let a3 = Address::generate(&s.env);

    let mut approvers = Vec::new(&s.env);
    approvers.push_back(a1.clone());
    approvers.push_back(a2.clone());
    approvers.push_back(a3.clone());

    let m = String::from_str(&s.env, "3-of-3 milestone");
    s.contract.create(
        &s.payer,
        &s.freelancer,
        &s.token_addr,
        &900,
        &m,
        &None,
        &None,
        &YieldRecipient::Payer,
        &approvers,
        &3u32,
    );
    s.contract.submit_work();

    s.contract.approve(&a1);
    assert_eq!(s.contract.get_status(), EscrowStatus::WorkSubmitted);

    s.contract.approve(&a2);
    assert_eq!(s.contract.get_status(), EscrowStatus::WorkSubmitted);

    s.contract.approve(&a3);
    assert_eq!(s.contract.get_status(), EscrowStatus::Completed);
    assert_eq!(s.token.balance(&s.freelancer), 900);
}

#[test]
fn test_invalid_threshold_rejected() {
    let s = Setup::new();
    let a1 = Address::generate(&s.env);

    let mut approvers = Vec::new(&s.env);
    approvers.push_back(a1.clone());

    let m = String::from_str(&s.env, "Bad threshold");
    // threshold > M
    let err = s.contract
        .try_create(
            &s.payer,
            &s.freelancer,
            &s.token_addr,
            &100,
            &m,
            &None,
            &None,
            &YieldRecipient::Payer,
            &approvers,
            &5u32, // 5 > 1
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, EscrowError::InvalidThreshold);
}
