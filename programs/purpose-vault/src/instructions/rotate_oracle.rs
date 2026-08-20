use crate::constants::{MARKET_SEED, ORACLE_ROTATION_NOTICE_SECONDS, POLICY_SEED};
use crate::error::CovenantError;
use crate::state::{MarketInput, PolicyWindow};
use anchor_lang::prelude::*;

/// Proposing a rotation is the policy authority's only power over the market
/// input. It cannot report a volume, and it cannot make a proposal take effect.
#[derive(Accounts)]
pub struct ProposeOracle<'info> {
    pub policy_authority: Signer<'info>,
    #[account(
        constraint = policy.authority == policy_authority.key() @ CovenantError::WrongPolicyAuthority,
        seeds = [POLICY_SEED, policy.policy_hash.as_ref()],
        bump = policy.bump,
    )]
    pub policy: Account<'info, PolicyWindow>,
    #[account(
        mut,
        constraint = market.policy_hash == policy.policy_hash @ CovenantError::WrongPolicyAuthority,
        seeds = [MARKET_SEED, market.policy_hash.as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketInput>,
}

pub fn propose_oracle_handler(ctx: Context<ProposeOracle>, new_oracle: Pubkey) -> Result<()> {
    require!(new_oracle != Pubkey::default(), CovenantError::ZeroOracle);

    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;
    // A second proposal replaces the first and restarts its clock. That is also
    // how a proposal is withdrawn: propose the oracle already in place, and the
    // rotation that eventually executes changes nothing.
    market.pending_oracle = new_oracle;
    market.pending_since = now;

    emit!(OracleRotationProposed {
        market: market.key(),
        policy: ctx.accounts.policy.key(),
        current_oracle: market.oracle,
        pending_oracle: new_oracle,
        proposed_at: now,
        effective_from: now
            .checked_add(ORACLE_ROTATION_NOTICE_SECONDS)
            .ok_or(CovenantError::ArithmeticOverflow)?,
    });
    Ok(())
}

/// Execution is permissionless on purpose. The authorisation happened when the
/// proposal was made and the notice is what the public was given; requiring the
/// authority again would only add a way for an aged, announced rotation to be
/// silently withheld.
#[derive(Accounts)]
pub struct ExecuteOracleRotation<'info> {
    #[account(
        mut,
        seeds = [MARKET_SEED, market.policy_hash.as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketInput>,
}

pub fn execute_oracle_rotation_handler(ctx: Context<ExecuteOracleRotation>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let market = &mut ctx.accounts.market;
    require!(
        market.pending_oracle != Pubkey::default(),
        CovenantError::NoPendingRotation
    );
    let waited = now
        .checked_sub(market.pending_since)
        .ok_or(CovenantError::InvalidClock)?;
    require!(
        waited >= ORACLE_ROTATION_NOTICE_SECONDS,
        CovenantError::RotationNoticeActive
    );

    let previous = market.oracle;
    market.oracle = market.pending_oracle;
    market.pending_oracle = Pubkey::default();
    market.pending_since = 0;
    // updated_at is deliberately untouched. A rotation restores who may speak,
    // never what was said: the incoming oracle has to report before any release
    // resumes, and a stale input stays stale across the change.

    emit!(OracleRotated {
        market: market.key(),
        previous_oracle: previous,
        new_oracle: market.oracle,
        rotated_at: now,
        eligible_volume_unchanged: market.eligible_volume,
        updated_at_unchanged: market.updated_at,
    });
    Ok(())
}

#[event]
pub struct OracleRotationProposed {
    pub market: Pubkey,
    pub policy: Pubkey,
    pub current_oracle: Pubkey,
    pub pending_oracle: Pubkey,
    pub proposed_at: i64,
    pub effective_from: i64,
}

#[event]
pub struct OracleRotated {
    pub market: Pubkey,
    pub previous_oracle: Pubkey,
    pub new_oracle: Pubkey,
    pub rotated_at: i64,
    pub eligible_volume_unchanged: u64,
    pub updated_at_unchanged: i64,
}
