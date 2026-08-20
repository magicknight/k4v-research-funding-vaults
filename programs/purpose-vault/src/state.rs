use anchor_lang::prelude::*;

#[derive(
    AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, PartialEq, Eq, Debug, Default,
)]
pub enum VaultKind {
    #[default]
    Beneficiary,
    Purpose,
}

impl VaultKind {
    /// One discriminating byte for the vault PDA seed. Two vaults with the same
    /// authority and mint must not collide across kinds.
    pub fn seed_byte(self) -> u8 {
        match self {
            VaultKind::Beneficiary => 0,
            VaultKind::Purpose => 1,
        }
    }
}

/// The shared window. Every vault bound to `policy_hash` debits one counter,
/// which is what makes the covenant's aggregate rule expressible at all.
#[account]
#[derive(InitSpace)]
pub struct PolicyWindow {
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub genesis_ts: i64,
    pub current_period_index: u64,
    pub released_this_period: u64,
    /// An absolute ceiling on the shared window, in mint base units, frozen at
    /// creation. `u64::MAX` leaves it inert. There is no instruction to change
    /// it, so a deployment that wants one must set it before any deposit.
    pub hard_ceiling: u64,
    pub vault_count: u32,
    pub bump: u8,
}

/// Eligible trailing 30-day spot volume is not observable on chain. It arrives
/// through one frozen oracle key and expires; it never falls back.
///
/// `eligible_volume` is denominated in **mint base units**, not in any quote
/// currency. "Spot volume" conventionally means a quote-currency figure, so the
/// unit is stated here and in the covenant rather than left to convention: a
/// USD-denominated report would silently change every ceiling this account
/// feeds. Base units also make the price cancel out of the covenant entirely,
/// and let venues be summed without a per-venue price.
#[account]
#[derive(InitSpace)]
pub struct MarketInput {
    pub oracle: Pubkey,
    pub policy_hash: [u8; 32],
    pub eligible_volume: u64,
    pub updated_at: i64,
    pub max_age_seconds: i64,
    pub report_count: u64,
    pub market_capacity_bps: u16,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct CovenantVault {
    pub kind: VaultKind,
    pub depositor: Pubkey,
    /// Beneficiary for `VaultKind::Beneficiary`; approver for `VaultKind::Purpose`.
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub deposited_amount: u64,
    pub monthly_cap: u64,
    pub released_total: u64,
    pub released_this_period: u64,
    pub current_period_index: u64,
    pub genesis_ts: i64,
    pub cliff_end_ts: i64,
    pub annual_release_bps: u16,
    pub mint_decimals: u8,
    pub state_bump: u8,
    pub token_vault_bump: u8,
}

/// The audit record for one purpose release window. It is a separate account
/// with its own creation timestamp precisely so that the notice period is a
/// fact about chain history rather than a claim.
#[account]
#[derive(InitSpace)]
pub struct Approval {
    pub vault: Pubkey,
    pub approver: Pubkey,
    pub destination: Pubkey,
    pub period_index: u64,
    pub approved_need: u64,
    pub consumed: u64,
    pub created_at: i64,
    pub bump: u8,
}
