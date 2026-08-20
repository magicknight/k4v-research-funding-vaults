use anchor_lang::prelude::*;

#[error_code]
pub enum CovenantError {
    #[msg("amount must be positive")]
    ZeroAmount,
    #[msg("annual release rate must be between 1 and 500 basis points")]
    InvalidAnnualReleaseRate,
    #[msg("market capacity rate must be between 1 and 500 basis points")]
    InvalidMarketCapacityRate,
    #[msg("market input tolerance must be between 1 second and 7 days")]
    InvalidInputAge,
    #[msg("beneficiary cliff must be at least 730 days")]
    CliffTooShort,
    #[msg("a purpose vault carries no cliff; its gate is approval")]
    PurposeCliffNotZero,
    #[msg("policy hash must not be all zeroes")]
    ZeroPolicyHash,
    #[msg("integer arithmetic overflow")]
    ArithmeticOverflow,
    #[msg("monthly cap rounds to zero; increase the deposit or rate")]
    ZeroMonthlyCap,
    #[msg("beneficiary cliff has not ended")]
    CliffActive,
    #[msg("release exceeds this period's non-carrying vault cap")]
    PeriodCapExceeded,
    #[msg("release exceeds the original deposit")]
    DepositExceeded,
    #[msg("release exceeds the market-capacity ceiling shared by this policy")]
    AggregateCapacityExceeded,
    #[msg("market input is older than the declared tolerance, or was never reported")]
    StaleMarketInput,
    #[msg("clock moved before the policy genesis timestamp")]
    InvalidClock,
    #[msg("this instruction does not apply to this vault kind")]
    WrongVaultKind,
    #[msg("approval belongs to a different vault")]
    ApprovalVaultMismatch,
    #[msg("approval was issued for a different period")]
    ApprovalPeriodMismatch,
    #[msg("approval must name a period later than the current one")]
    ApprovalPeriodTooSoon,
    #[msg("release exceeds the approved need")]
    ApprovedNeedExceeded,
    #[msg("approval has not completed its 30-day notice")]
    NoticePeriodActive,
    #[msg("the approver must not own the destination token account")]
    ApproverIsPayee,
    #[msg("destination token account is not owned by the beneficiary")]
    WrongDestinationOwner,
    #[msg("release destination does not match the approved destination")]
    ApprovalDestinationMismatch,
    #[msg("only the policy authority may attach a vault to this capacity window")]
    WrongPolicyAuthority,
    #[msg("hard ceiling must be positive; use u64::MAX to leave it inert")]
    ZeroHardCeiling,
}
