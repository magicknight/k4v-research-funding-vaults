//! E-06 authority semantics enforced on canonical accounts in a new namespace.
use crate::{contexts::*, state::*};
use anchor_lang::prelude::*;

pub fn prepare_key(ctx: Context<PrepareWithdrawalKey>, subject: Pubkey) -> Result<()> {
    require!(subject != Pubkey::default(), LaunchError::InvalidConfig);
    require!(
        ctx.accounts.policy.state != CANCELLED,
        LaunchError::InvalidState
    );
    ctx.accounts.record.set_inner(WithdrawalKeyV6 {
        policy: ctx.accounts.policy.key(),
        subject,
        history_mask: 0,
        pending_mask: 0,
        recipient: false,
        bump: ctx.bumps.record,
    });
    Ok(())
}

fn quorum(p: &LaunchPolicyV6, role: u8, a: Pubkey, b: Pubkey) -> Result<bool> {
    let keys = p.withdrawal_keys(role)?;
    Ok(a != b && keys.contains(&a) && keys.contains(&b))
}

pub fn eligible_recipient(p: &LaunchPolicyV6, r: &WithdrawalKeyV6) -> Result<()> {
    require!(
        r.subject != p.founder
            && r.subject != p.treasury
            && !p.config.founder_recovery_keys.contains(&r.subject)
            && !p.config.treasury_recovery_keys.contains(&r.subject)
            && r.history_mask == 0
            && r.pending_mask == 0,
        LaunchError::KnownSelfPayment
    );
    Ok(())
}

fn eligible_successor(p: &LaunchPolicyV6, role: u8, r: &WithdrawalKeyV6) -> Result<()> {
    require!(
        r.subject != Pubkey::default()
            && r.subject != p.owner_for(role)?
            && r.history_mask & (1 << role) == 0
            && !r.recipient
            && !p.withdrawal_keys(role)?.contains(&r.subject),
        LaunchError::IneligibleSuccessor
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Fixed signed ABI fields; no off-chain defaults.
pub fn propose(
    ctx: Context<ProposeWithdrawal>,
    role: u8,
    recovery: bool,
    nonce: u64,
    epoch: u64,
    valid_from: i64,
    valid_until: i64,
    predecessor: Pubkey,
) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    let now = p.tick()?;
    require!(p.state != CANCELLED, LaunchError::InvalidState);
    validate_admission(valid_from, valid_until, now)?;
    let r = *p.withdrawal_for(role)?;
    require!(
        r.pending == 0
            && nonce == r.sequence.checked_add(1).ok_or(LaunchError::Overflow)?
            && epoch == r.epoch
            && predecessor == r.current,
        LaunchError::InvalidChange
    );
    r.epoch.checked_add(1).ok_or(LaunchError::Overflow)?;
    let a = ctx.accounts.initiator.key();
    let b = ctx.accounts.cosigner.key();
    if recovery {
        require!(quorum(p, role, a, b)?, LaunchError::RecoveryQuorum);
    } else {
        require!(
            a == r.current && b == r.current,
            LaunchError::WithdrawalAuthority
        );
    }
    let record = &mut ctx.accounts.successor_record;
    eligible_successor(p, role, record)?;
    let execute_after = now
        .checked_add(CHANGE_NOTICE)
        .ok_or(LaunchError::Overflow)?;
    let expires_at = execute_after
        .checked_add(WITHDRAWAL_EXECUTION_WINDOW)
        .ok_or(LaunchError::Overflow)?;
    ctx.accounts.proposal.set_inner(WithdrawalProposalV6 {
        policy: p.key(),
        role,
        nonce,
        epoch,
        predecessor: r.current,
        successor: ctx.accounts.successor.key(),
        recovery,
        valid_from,
        valid_until,
        created_at: now,
        execute_after,
        expires_at,
        status: PROPOSAL_PENDING,
        finished_at: 0,
        bump: ctx.bumps.proposal,
    });
    record.pending_mask |= 1 << role;
    p.withdrawal[role as usize].pending = nonce;
    p.withdrawal[role as usize].sequence = nonce;
    Ok(())
}

/// The whole signed window must be safe, including a final-second admission.
pub fn validate_admission(valid_from: i64, valid_until: i64, now: i64) -> Result<()> {
    require!(
        valid_from >= 0 && valid_until >= valid_from,
        LaunchError::SubmissionWindow
    );
    let width = valid_until
        .checked_sub(valid_from)
        .ok_or(LaunchError::Overflow)?;
    require!(
        width <= MAX_SUBMISSION_WINDOW && valid_from <= now && now <= valid_until,
        LaunchError::SubmissionWindow
    );
    valid_until
        .checked_add(CHANGE_NOTICE)
        .and_then(|t| t.checked_add(WITHDRAWAL_EXECUTION_WINDOW))
        .ok_or(LaunchError::Overflow)?;
    Ok(())
}

fn pending(p: &LaunchPolicyV6, q: &WithdrawalProposalV6, k: &WithdrawalKeyV6) -> Result<()> {
    let r = p.withdrawal_for(q.role)?;
    require!(
        q.status == PROPOSAL_PENDING
            && q.nonce == r.sequence
            && q.nonce == r.pending
            && q.epoch == r.epoch
            && q.predecessor == r.current
            && q.finished_at == 0
            && k.pending_mask & (1 << q.role) != 0,
        LaunchError::InvalidChange
    );
    Ok(())
}

fn finish(
    p: &mut LaunchPolicyV6,
    q: &mut WithdrawalProposalV6,
    k: &mut WithdrawalKeyV6,
    status: u8,
    now: i64,
) {
    p.withdrawal[q.role as usize].pending = 0;
    k.pending_mask &= !(1 << q.role);
    q.status = status;
    q.finished_at = now;
}

pub fn execute(ctx: Context<ExecuteWithdrawal>) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    let now = p.tick()?;
    require!(p.state != CANCELLED, LaunchError::InvalidState);
    let q = &mut ctx.accounts.proposal;
    let k = &mut ctx.accounts.successor_record;
    pending(p, q, k)?;
    require!(
        now >= q.execute_after && now < q.expires_at,
        LaunchError::WithdrawalWindow
    );
    eligible_successor(p, q.role, k)?;
    let r = &mut p.withdrawal[q.role as usize];
    r.current = q.successor;
    r.epoch = r.epoch.checked_add(1).ok_or(LaunchError::Overflow)?;
    k.history_mask |= 1 << q.role;
    finish(p, q, k, PROPOSAL_EXECUTED, now);
    Ok(())
}

pub fn cancel(ctx: Context<CancelWithdrawal>) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    let now = p.tick()?;
    let q = &mut ctx.accounts.proposal;
    let k = &mut ctx.accounts.successor_record;
    pending(p, q, k)?;
    let a = ctx.accounts.initiator.key();
    let b = ctx.accounts.cosigner.key();
    require!(
        quorum(p, q.role, a, b)?
            || (a == b
                && (a == q.successor || (!q.recovery && a == p.withdrawal_for(q.role)?.current))),
        LaunchError::WithdrawalAuthority
    );
    finish(p, q, k, PROPOSAL_CANCELLED, now);
    Ok(())
}

pub fn expire(ctx: Context<ExecuteWithdrawal>) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    let now = p.tick()?;
    let q = &mut ctx.accounts.proposal;
    let k = &mut ctx.accounts.successor_record;
    pending(p, q, k)?;
    require!(now >= q.expires_at, LaunchError::WithdrawalWindow);
    finish(p, q, k, PROPOSAL_EXPIRED, now);
    Ok(())
}
