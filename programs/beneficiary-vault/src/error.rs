use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    #[msg("deposit amount must be positive")]
    ZeroAmount,
    #[msg("annual release rate must be between 1 and 500 basis points")]
    InvalidAnnualReleaseRate,
    #[msg("cliff must be at least 730 days")]
    CliffTooShort,
    #[msg("policy hash must not be all zeroes")]
    ZeroPolicyHash,
    #[msg("integer arithmetic overflow")]
    ArithmeticOverflow,
    #[msg("monthly cap rounds to zero; increase the deposit or rate")]
    ZeroMonthlyCap,
    #[msg("beneficiary cliff has not ended")]
    CliffActive,
    #[msg("release exceeds this period's non-carrying cap")]
    PeriodCapExceeded,
    #[msg("release exceeds the original deposit")]
    DepositExceeded,
    #[msg("clock moved before the frozen cliff timestamp")]
    InvalidClock,
}
