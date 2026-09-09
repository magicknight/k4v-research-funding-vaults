//! TEST_ONLY fixed 2-of-3 recovery and 90-day accepted key changes.
use crate::{contexts::*, state::*};
use anchor_lang::prelude::*;

pub fn validate_recovery_keys(keys: &[Pubkey; 3]) -> Result<()> {
    require!(
        keys.iter().all(|k| *k != Pubkey::default())
            && keys[0] != keys[1]
            && keys[0] != keys[2]
            && keys[1] != keys[2],
        LaunchError::InvalidConfig
    );
    Ok(())
}

fn quorum(p: &LaunchPolicyV5, first: Pubkey, second: Pubkey) -> bool {
    first != second
        && p.config.recovery_keys.contains(&first)
        && p.config.recovery_keys.contains(&second)
}

pub fn propose(ctx: Context<ProposeChange>, kind: u8, recovery: bool, nonce: u64) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    let now = p.tick()?;
    require!(p.state != CANCELLED, LaunchError::InvalidState);
    require!(
        p.pending_change == 0
            && nonce
                == p.change_sequence
                    .checked_add(1)
                    .ok_or(LaunchError::Overflow)?,
        LaunchError::InvalidChange
    );
    let initiator = ctx.accounts.initiator.key();
    let cosigner = ctx.accounts.cosigner.key();
    if recovery {
        require!(quorum(p, initiator, cosigner), LaunchError::RecoveryQuorum);
    } else {
        require!(
            initiator == p.controller && cosigner == p.controller,
            LaunchError::Unauthorized
        );
    }
    let successor = ctx.accounts.successor.key();
    let current = match kind {
        CHANGE_ORACLE => p.oracle,
        CHANGE_CONTROLLER => p.controller,
        _ => return err!(LaunchError::InvalidChange),
    };
    require!(
        successor != Pubkey::default() && successor != current,
        LaunchError::InvalidChange
    );
    let execute_after = now
        .checked_add(CHANGE_NOTICE)
        .ok_or(LaunchError::Overflow)?;
    ctx.accounts.proposal.set_inner(ChangeProposalV5 {
        policy: p.key(),
        nonce,
        kind,
        recovery,
        successor,
        controller_epoch: p.controller_epoch,
        oracle_epoch: p.oracle_epoch,
        created_at: now,
        execute_after,
        status: PROPOSAL_PENDING,
        bump: ctx.bumps.proposal,
    });
    p.change_sequence = nonce;
    p.pending_change = nonce;
    Ok(())
}

fn pending(p: &LaunchPolicyV5, q: &ChangeProposalV5) -> Result<()> {
    require!(p.state != CANCELLED, LaunchError::InvalidState);
    require!(
        q.status == PROPOSAL_PENDING
            && p.pending_change == q.nonce
            && p.change_sequence == q.nonce
            && p.controller_epoch == q.controller_epoch
            && p.oracle_epoch == q.oracle_epoch,
        LaunchError::InvalidChange
    );
    Ok(())
}

pub fn cancel(ctx: Context<CancelChange>) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    p.tick()?;
    let q = &mut ctx.accounts.proposal;
    pending(p, q)?;
    let a = ctx.accounts.initiator.key();
    let b = ctx.accounts.cosigner.key();
    // Recovery quorum can evict a malicious normal proposal. The old
    // controller alone cannot veto a recovery proposal.
    require!(
        quorum(p, a, b) || (a == b && (a == q.successor || (!q.recovery && a == p.controller))),
        LaunchError::Unauthorized
    );
    q.status = PROPOSAL_CANCELLED;
    p.pending_change = 0;
    Ok(())
}

pub fn execute(ctx: Context<ExecuteChange>) -> Result<()> {
    let p = &mut ctx.accounts.policy;
    let now = p.tick()?;
    let q = &mut ctx.accounts.proposal;
    pending(p, q)?;
    require!(now >= q.execute_after, LaunchError::ChangeNotice);
    match q.kind {
        CHANGE_ORACLE => {
            p.oracle = q.successor;
            p.oracle_epoch = p.oracle_epoch.checked_add(1).ok_or(LaunchError::Overflow)?;
            p.oracle_activated_at = now;
            // Preserve observed_at and global sequence. A new epoch needs a
            // freshly observed report; old timestamps never become fresh.
            p.report_valid = false;
        }
        CHANGE_CONTROLLER => {
            p.controller = q.successor;
            p.controller_epoch = p
                .controller_epoch
                .checked_add(1)
                .ok_or(LaunchError::Overflow)?;
        }
        _ => return err!(LaunchError::InvalidChange),
    }
    q.status = PROPOSAL_EXECUTED;
    p.pending_change = 0;
    Ok(())
}
