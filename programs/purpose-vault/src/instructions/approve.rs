use crate::constants::{APPROVAL_SEED, POLICY_SEED, VAULT_SEED};
use crate::error::CovenantError;
use crate::policy;
use crate::state::{Approval, CovenantVault, PolicyWindow, VaultKind};
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, TokenAccount};

#[derive(Accounts)]
#[instruction(period_index: u64)]
pub struct Approve<'info> {
    #[account(mut)]
    pub approver: Signer<'info>,
    pub mint: Account<'info, Mint>,
    /// The approver appears in the vault's own seeds, so an unrelated signer
    /// cannot reach this account at all.
    #[account(
        has_one = mint,
        seeds = [
            VAULT_SEED,
            vault.policy_hash.as_ref(),
            &[vault.kind.seed_byte()],
            approver.key().as_ref(),
            mint.key().as_ref(),
        ],
        bump = vault.state_bump,
    )]
    pub vault: Account<'info, CovenantVault>,
    #[account(
        seeds = [POLICY_SEED, vault.policy_hash.as_ref()],
        bump = policy.bump,
    )]
    pub policy: Account<'info, PolicyWindow>,
    #[account(token::mint = mint)]
    pub destination: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = approver,
        space = 8 + Approval::INIT_SPACE,
        seeds = [APPROVAL_SEED, vault.key().as_ref(), &period_index.to_le_bytes()],
        bump,
    )]
    pub approval: Account<'info, Approval>,
    pub system_program: Program<'info, System>,
}

pub fn approve_handler(ctx: Context<Approve>, period_index: u64, approved_need: u64) -> Result<()> {
    require!(
        ctx.accounts.vault.kind == VaultKind::Purpose,
        CovenantError::WrongVaultKind
    );
    require!(approved_need > 0, CovenantError::ZeroAmount);

    // The structural half of the covenant's recusal rule. Whether an approver
    // is independent in substance is not decidable here and is not claimed.
    require!(
        ctx.accounts.destination.owner != ctx.accounts.approver.key(),
        CovenantError::ApproverIsPayee
    );

    let now = Clock::get()?.unix_timestamp;
    let current = policy::period_index(now, ctx.accounts.policy.genesis_ts)?;
    // An approval must name a later period than the one it is written in. With
    // 30-day periods and a 30-day notice, approving for the current period
    // could never be consumed anyway; rejecting it here says so plainly.
    require!(period_index > current, CovenantError::ApprovalPeriodTooSoon);

    let approval = &mut ctx.accounts.approval;
    approval.vault = ctx.accounts.vault.key();
    approval.approver = ctx.accounts.approver.key();
    approval.destination = ctx.accounts.destination.key();
    approval.period_index = period_index;
    approval.approved_need = approved_need;
    approval.consumed = 0;
    approval.created_at = now;
    approval.bump = ctx.bumps.approval;

    emit!(ApprovalRecorded {
        approval: approval.key(),
        vault: approval.vault,
        approver: approval.approver,
        destination: approval.destination,
        period_index,
        approved_need,
        created_at: now,
    });
    Ok(())
}

#[event]
pub struct ApprovalRecorded {
    pub approval: Pubkey,
    pub vault: Pubkey,
    pub approver: Pubkey,
    pub destination: Pubkey,
    pub period_index: u64,
    pub approved_need: u64,
    pub created_at: i64,
}
