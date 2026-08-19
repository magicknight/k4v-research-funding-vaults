use crate::constants::MARKET_SEED;
use crate::error::CovenantError;
use crate::state::MarketInput;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ReportVolume<'info> {
    pub oracle: Signer<'info>,
    #[account(
        mut,
        has_one = oracle,
        seeds = [MARKET_SEED, market.policy_hash.as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketInput>,
}

pub fn report_volume_handler(ctx: Context<ReportVolume>, eligible_volume: u64) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;
    market.eligible_volume = eligible_volume;
    market.updated_at = now;
    market.report_count = market
        .report_count
        .checked_add(1)
        .ok_or(CovenantError::ArithmeticOverflow)?;

    emit!(VolumeReported {
        market: market.key(),
        oracle: market.oracle,
        eligible_volume,
        updated_at: now,
        report_count: market.report_count,
    });
    Ok(())
}

#[event]
pub struct VolumeReported {
    pub market: Pubkey,
    pub oracle: Pubkey,
    pub eligible_volume: u64,
    pub updated_at: i64,
    pub report_count: u64,
}
