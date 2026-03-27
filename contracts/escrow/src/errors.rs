use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum EscrowError {
    AlreadyExists = 1,
    NotActive = 2,
    WorkNotSubmitted = 3,
    InvalidAmount = 4,
    DeadlineNotPassed = 5,
    NotExpired = 6,
    ConfigNotSet = 8,
    RateLimitExceeded = 7,
    /// Yield protocol address is invalid or not a contract.
    YieldProtocolInvalid = 9,
    /// Failed to withdraw from yield protocol.
    YieldWithdrawFailed = 10,
    /// Yield protocol not enabled for this escrow.
    YieldNotEnabled = 11,
    /// Token not on admin allowlist.
    TokenNotAllowed = 12,
    /// Amount outside configured min/max range.
    AmountOutOfRange = 13,
}
