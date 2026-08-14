use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct BeneficiaryVault {
    pub depositor: Pubkey,
    pub beneficiary: Pubkey,
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub deposited_amount: u64,
    pub monthly_cap: u64,
    pub released_total: u64,
    pub released_this_period: u64,
    pub genesis_ts: i64,
    pub cliff_end_ts: i64,
    pub current_period_index: u64,
    pub annual_release_bps: u16,
    pub mint_decimals: u8,
    pub state_bump: u8,
    pub token_vault_bump: u8,
}
