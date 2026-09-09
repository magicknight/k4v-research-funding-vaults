//! E-06 authority semantics enforced on canonical accounts in a new namespace.
use crate::{contexts::*, state::*};
use anchor_lang::prelude::*;

pub fn prepare_key(ctx: Context<PrepareWithdrawalKey>, subject: Pubkey) -> Result<()> {
    require!(subject != Pubkey::default(), LaunchError::InvalidConfig);
    require!(
        ctx.accounts.policy.state != CANCELLED,
        LaunchError::InvalidState
    );
    ctx.accounts.record.set_inner(WithdrawalKeyV5 {
        policy: ctx.accounts.policy.key(),
        subject,
        history_mask: 0,
        pending_mask: 0,
        recipient: false,
        bump: ctx.bumps.record,
    });
    Ok(())
}

fn quorum(p: &LaunchPolicyV5, role: u8, a: Pubkey, b: Pubkey) -> Result<bool> {
    let keys = p.withdrawal_keys(role)?;
    Ok(a != b && keys.contains(&a) && keys.contains(&b))
}

pub fn eligible_recipient(p: &LaunchPolicyV5, r: &WithdrawalKeyV5) -> Result<()> {
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

fn eligible_successor(p: &LaunchPolicyV5, role: u8, r: &WithdrawalKeyV5) -> Result<()> {
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

pub fn propose(
    ctx: Context<ProposeWithdrawal>,
    role: u8,
    recovery: bool,
    nonce: u64,
    epoch: u64,
    created_at: i64,
) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    let now = p.tick()?;
    require!(p.state != CANCELLED, LaunchError::InvalidState);
    // Exact-clock submission is deliberate for this local TEST_ONLY profile:
    // all accepting signatures bind the stored notice/expiry, with no backdating.
    require!(created_at == now, LaunchError::ProposalClock);
    let r = *p.withdrawal_for(role)?;
    require!(
        r.pending == 0
            && nonce == r.sequence.checked_add(1).ok_or(LaunchError::Overflow)?
            && epoch == r.epoch,
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
    ctx.accounts.proposal.set_inner(WithdrawalProposalV5 {
        policy: p.key(),
        role,
        nonce,
        epoch,
        predecessor: r.current,
        successor: ctx.accounts.successor.key(),
        recovery,
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

fn pending(p: &LaunchPolicyV5, q: &WithdrawalProposalV5, k: &WithdrawalKeyV5) -> Result<()> {
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
    p: &mut LaunchPolicyV5,
    q: &mut WithdrawalProposalV5,
    k: &mut WithdrawalKeyV5,
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
