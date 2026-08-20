use crate::constants::{
    MARKET_SEED, MAX_INPUT_AGE_CEILING_SECONDS, MAX_MARKET_CAPACITY_BPS, POLICY_SEED,
};
use crate::error::CovenantError;
use crate::state::{MarketInput, PolicyWindow};
use anchor_lang::prelude::*;
use anchor_spl::token::Mint;

#[derive(Accounts)]
#[instruction(policy_hash: [u8; 32])]
pub struct OpenPolicy<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: frozen at creation as the only key that may report volume. It
    /// never signs a transfer and there is no instruction to replace it.
    pub oracle: UncheckedAccount<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        init,
        payer = authority,
        space = 8 + PolicyWindow::INIT_SPACE,
        seeds = [POLICY_SEED, policy_hash.as_ref()],
        bump,
    )]
    pub policy: Account<'info, PolicyWindow>,
    #[account(
        init,
        payer = authority,
        space = 8 + MarketInput::INIT_SPACE,
        seeds = [MARKET_SEED, policy_hash.as_ref()],
        bump,
    )]
    pub market: Account<'info, MarketInput>,
    pub system_program: Program<'info, System>,
}

pub fn open_policy_handler(
    ctx: Context<OpenPolicy>,
    policy_hash: [u8; 32],
    market_capacity_bps: u16,
    max_age_seconds: i64,
    hard_ceiling: u64,
) -> Result<()> {
    require!(policy_hash != [0; 32], CovenantError::ZeroPolicyHash);
    // Zero would freeze the policy the moment it opened, with no instruction to
    // undo it. A deployment that wants no ceiling passes u64::MAX and says so.
    require!(hard_ceiling > 0, CovenantError::ZeroHardCeiling);
    require!(
        (1..=MAX_MARKET_CAPACITY_BPS).contains(&market_capacity_bps),
        CovenantError::InvalidMarketCapacityRate
    );
    require!(
        (1..=MAX_INPUT_AGE_CEILING_SECONDS).contains(&max_age_seconds),
        CovenantError::InvalidInputAge
    );

    let now = Clock::get()?.unix_timestamp;

    let policy = &mut ctx.accounts.policy;
    policy.authority = ctx.accounts.authority.key();
    policy.mint = ctx.accounts.mint.key();
    policy.policy_hash = policy_hash;
    policy.genesis_ts = now;
    policy.current_period_index = 0;
    policy.released_this_period = 0;
    policy.hard_ceiling = hard_ceiling;
    policy.vault_count = 0;
    policy.bump = ctx.bumps.policy;

    let market = &mut ctx.accounts.market;
    market.oracle = ctx.accounts.oracle.key();
    market.policy_hash = policy_hash;
    market.eligible_volume = 0;
    // Zero means "never reported". Until the oracle speaks, every release is
    // rejected as stale rather than treated as unlimited.
    market.updated_at = 0;
    market.max_age_seconds = max_age_seconds;
    market.report_count = 0;
    market.market_capacity_bps = market_capacity_bps;
    market.bump = ctx.bumps.market;

    emit!(PolicyOpened {
        policy: policy.key(),
        market: market.key(),
        authority: policy.authority,
        oracle: market.oracle,
        mint: policy.mint,
        policy_hash,
        genesis_ts: now,
        market_capacity_bps,
        max_age_seconds,
        hard_ceiling,
    });
    Ok(())
}

#[event]
pub struct PolicyOpened {
    pub policy: Pubkey,
    pub market: Pubkey,
    pub authority: Pubkey,
    pub oracle: Pubkey,
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub genesis_ts: i64,
    pub market_capacity_bps: u16,
    pub max_age_seconds: i64,
    pub hard_ceiling: u64,
}
