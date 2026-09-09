//! Reserved-capacity and annual-input TEST_ONLY candidate; not a production profile.
use anchor_lang::prelude::*;
use anchor_spl::token::{self, TransferChecked};
pub mod capacity;
pub mod contexts;
pub mod governance;
pub mod state;
pub use contexts::*;
pub use state::*;

declare_id!("5h5iUez8fpHThaQhDdyUSQab9ngRmG2zfMBNx5bGnB9Q");

#[program]
pub mod launch_vault_v4 {
    use super::*;

    pub fn open_policy(
        ctx: Context<OpenPolicy>,
        config: LaunchConfig,
        spec_hash: [u8; 32],
        identity: [u8; 32],
    ) -> Result<()> {
        require!(
            cfg!(feature = "test-profile"),
            LaunchError::ExperimentalProfileDisabled
        );
        let now = Clock::get()?.unix_timestamp;
        require!(config.t0 > now, LaunchError::T0Boundary);
        config.t0.checked_add(CLIFF).ok_or(LaunchError::Overflow)?;
        capacity::validate_rules(&config)?;
        governance::validate_recovery_keys(&config.recovery_keys)?;
        require!(
            config.recovery_keys
                == [
                    ctx.accounts.recovery_one.key(),
                    ctx.accounts.recovery_two.key(),
                    ctx.accounts.recovery_three.key()
                ],
            LaunchError::Unauthorized
        );
        require!(
            config
                .founder_amount
                .checked_add(config.treasury_amount)
                .ok_or(LaunchError::Overflow)?
                <= ctx.accounts.mint.supply,
            LaunchError::InvalidConfig
        );
        require!(
            config.founder_amount > 0
                && config.treasury_amount > 0
                && config.founder_period_cap > 0
                && config.founder_period_cap <= config.founder_amount
                && config.treasury_period_cap > 0
                && config.treasury_period_cap <= config.treasury_amount
                && config.shared_hard_cap > 0
                && (1..=7 * 86_400).contains(&config.max_report_age)
                && spec_hash != [0; 32]
                && ctx.accounts.oracle.key() != Pubkey::default(),
            LaunchError::InvalidConfig
        );
        let bound = state::identity(
            &ctx.accounts.creator.key(),
            &ctx.accounts.mint.key(),
            &ctx.accounts.founder.key(),
            &ctx.accounts.treasury.key(),
            &ctx.accounts.oracle.key(),
            &spec_hash,
            &config,
        );
        require!(bound == identity, LaunchError::IdentityMismatch);
        ctx.accounts.policy.set_inner(LaunchPolicyV4 {
            creator: ctx.accounts.creator.key(),
            mint: ctx.accounts.mint.key(),
            founder: ctx.accounts.founder.key(),
            treasury: ctx.accounts.treasury.key(),
            oracle: ctx.accounts.oracle.key(),
            identity,
            spec_hash,
            config,
            state: PREPARED,
            funded_mask: 0,
            bump: ctx.bumps.policy,
            last_action_at: now,
            period: 0,
            shared_used: 0,
            report_period: 0,
            report_capacity: 0,
            report_at: 0,
            report_sequence: 0,
            founder_period_used: 0,
            treasury_period_used: 0,
            founder_released_total: 0,
            treasury_released_total: 0,
            annual_index: 0,
            founder_annual_used: 0,
            treasury_annual_used: 0,
            initial_oracle: ctx.accounts.oracle.key(),
            controller: ctx.accounts.creator.key(),
            controller_epoch: 0,
            oracle_epoch: 0,
            oracle_activated_at: now,
            report_epoch: 0,
            report_valid: false,
            change_sequence: 0,
            pending_change: 0,
        });
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, role: u8, amount: u64) -> Result<()> {
        let policy = &mut ctx.accounts.policy;
        let now = policy.tick()?;
        require!(policy.state == PREPARED, LaunchError::InvalidState);
        require!(now < policy.config.t0, LaunchError::T0Boundary);
        require_keys_eq!(
            ctx.accounts.authority.key(),
            policy.owner_for(role)?,
            LaunchError::Unauthorized
        );
        require!(
            amount == policy.principal_for(role)?,
            LaunchError::WrongPrincipal
        );
        require!(
            ctx.accounts.mint.mint_authority.is_none()
                && ctx.accounts.mint.freeze_authority.is_none(),
            LaunchError::MintAuthorityLive
        );
        require!(
            policy.funded_mask & (1 << role) == 0,
            LaunchError::PoolsNotFunded
        );
        token::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.source.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.vault_token.to_account_info(),
                    authority: ctx.accounts.depositor.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.mint.decimals,
        )?;
        ctx.accounts.vault.set_inner(LaunchVaultV4 {
            policy: policy.key(),
            depositor: ctx.accounts.depositor.key(),
            authority: ctx.accounts.authority.key(),
            role,
            bump: ctx.bumps.vault,
            principal: amount,
            released_total: 0,
            period: 0,
            period_used: 0,
        });
        policy.funded_mask |= 1 << role;
        Ok(())
    }

    pub fn arm(ctx: Context<Control>) -> Result<()> {
        let p = &mut ctx.accounts.policy;
        let now = p.tick()?;
        require!(p.state == PREPARED, LaunchError::InvalidState);
        require!(now < p.config.t0, LaunchError::T0Boundary);
        // Only exact-principal successful token transfers can set either bit.
        // No withdrawal path exists in PREPARED and the mint cannot freeze or mint.
        require!(p.funded_mask == 3, LaunchError::PoolsNotFunded);
        p.state = ARMED;
        Ok(())
    }

    pub fn activate(ctx: Context<PolicyOnly>) -> Result<()> {
        let p = &mut ctx.accounts.policy;
        let now = p.tick()?;
        require!(p.state == ARMED, LaunchError::InvalidState);
        require!(now >= p.config.t0, LaunchError::T0Boundary);
        p.state = ACTIVE;
        Ok(())
    }

    pub fn cancel(ctx: Context<ConsentControl>) -> Result<()> {
        let p = &mut ctx.accounts.policy;
        let now = p.tick()?;
        require!(
            p.state == PREPARED || p.state == ARMED,
            LaunchError::InvalidState
        );
        require!(now < p.config.t0, LaunchError::T0Boundary);
        p.state = CANCELLED;
        Ok(())
    }

    pub fn expire_unarmed(ctx: Context<PolicyOnly>) -> Result<()> {
        let p = &mut ctx.accounts.policy;
        let now = p.tick()?;
        require!(p.state == PREPARED, LaunchError::InvalidState);
        require!(now >= p.config.t0, LaunchError::T0Boundary);
        p.state = CANCELLED;
        Ok(())
    }

    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        ctx.accounts.policy.tick()?;
        require!(
            ctx.accounts.policy.state == CANCELLED,
            LaunchError::InvalidState
        );
        require!(
            ctx.accounts.vault.released_total == 0,
            LaunchError::InvalidState
        );
        let amount = ctx.accounts.vault_token.amount;
        require!(amount > 0, LaunchError::ZeroAmount);
        let policy_key = ctx.accounts.policy.key();
        let role = [ctx.accounts.vault.role];
        let bump = [ctx.accounts.vault.bump];
        let seeds: &[&[u8]] = &[b"launch-v4-vault", policy_key.as_ref(), &role, &bump];
        token::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.vault_token.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: ctx.accounts.vault.to_account_info(),
                },
            )
            .with_signer(&[seeds]),
            amount,
            ctx.accounts.mint.decimals,
        )?;
        // Retain policy/vault tombstones. Late unsolicited token dust can be
        // returned to the same original depositor without reopening the policy.
        Ok(())
    }

    pub fn report_capacity(
        ctx: Context<Report>,
        capacity: u64,
        observed_at: i64,
        sequence: u64,
        epoch: u64,
    ) -> Result<()> {
        let p = &mut ctx.accounts.policy;
        let now = p.tick()?;
        require!(p.state == ACTIVE, LaunchError::InvalidState);
        let period = period_at(p.config.t0, now)?;
        require!(
            epoch == p.oracle_epoch
                && observed_at >= p.oracle_activated_at
                && sequence > p.report_sequence
                && observed_at >= p.report_at
                && observed_at >= period_start(p.config.t0, period)?
                && observed_at <= now
                && now.checked_sub(observed_at).ok_or(LaunchError::Overflow)?
                    <= p.config.max_report_age,
            LaunchError::InvalidReport
        );
        p.report_period = period;
        p.report_capacity = capacity;
        p.report_at = observed_at;
        p.report_sequence = sequence;
        p.report_epoch = epoch;
        p.report_valid = true;
        Ok(())
    }

    pub fn approve_treasury(ctx: Context<ApproveTreasury>, period: u64, need: u64) -> Result<()> {
        let p = &mut ctx.accounts.policy;
        let now = p.tick()?;
        require!(p.state != CANCELLED, LaunchError::InvalidState);
        require!(
            need > 0 && ctx.accounts.recipient.owner != p.treasury,
            LaunchError::InvalidApproval
        );
        // A pre-T0 notice may target period 0; after T0 only future periods.
        if now >= p.config.t0 {
            require!(
                period > period_at(p.config.t0, now)?,
                LaunchError::InvalidApproval
            );
        }
        let end = period_start(
            p.config.t0,
            period.checked_add(1).ok_or(LaunchError::Overflow)?,
        )?;
        require!(
            now.checked_add(PERIOD).ok_or(LaunchError::Overflow)? < end,
            LaunchError::InvalidApproval
        );
        ctx.accounts.approval.set_inner(TreasuryApprovalV4 {
            policy: p.key(),
            period,
            recipient: ctx.accounts.recipient.key(),
            recipient_owner: ctx.accounts.recipient.owner,
            need,
            consumed: 0,
            created_at: now,
            bump: ctx.bumps.approval,
        });
        Ok(())
    }

    pub fn release(ctx: Context<Release>, amount: u64) -> Result<()> {
        let p = &mut ctx.accounts.policy;
        let now = p.tick()?;
        require!(p.state == ACTIVE, LaunchError::InvalidState);
        require!(now > p.config.t0, LaunchError::T0Boundary);
        require!(amount > 0, LaunchError::ZeroAmount);
        let period = period_at(p.config.t0, now)?;
        require!(
            period >= p.period && period >= ctx.accounts.vault.period,
            LaunchError::ClockRollback
        );
        require!(
            p.report_valid
                && p.report_epoch == p.oracle_epoch
                && p.report_sequence > 0
                && p.report_period == period
                && p.report_at <= now
                && now.checked_sub(p.report_at).ok_or(LaunchError::Overflow)?
                    <= p.config.max_report_age,
            LaunchError::InvalidReport
        );
        let v = &mut ctx.accounts.vault;
        require_keys_eq!(v.authority, p.owner_for(v.role)?, LaunchError::Unauthorized);
        match v.role {
            FOUNDER => {
                require!(
                    now >= p
                        .config
                        .t0
                        .checked_add(CLIFF)
                        .ok_or(LaunchError::Overflow)?,
                    LaunchError::CliffActive
                );
                require_keys_eq!(
                    ctx.accounts.destination.owner,
                    p.founder,
                    LaunchError::Unauthorized
                );
                require!(
                    ctx.accounts.approval.is_none(),
                    LaunchError::InvalidApproval
                );
            }
            TREASURY => {
                let a = ctx
                    .accounts
                    .approval
                    .as_mut()
                    .ok_or(LaunchError::InvalidApproval)?;
                let expected = Pubkey::create_program_address(
                    &[
                        b"launch-v4-approval",
                        p.key().as_ref(),
                        &period.to_le_bytes(),
                        &[a.bump],
                    ],
                    &crate::ID,
                )
                .map_err(|_| error!(LaunchError::InvalidApproval))?;
                require!(
                    a.key() == expected
                        && a.policy == p.key()
                        && a.period == period
                        && a.recipient == ctx.accounts.destination.key()
                        && a.recipient_owner == ctx.accounts.destination.owner
                        && a.recipient_owner != p.treasury
                        && now
                            >= a.created_at
                                .checked_add(PERIOD)
                                .ok_or(LaunchError::Overflow)?,
                    LaunchError::InvalidApproval
                );
                a.consumed = a
                    .consumed
                    .checked_add(amount)
                    .ok_or(LaunchError::Overflow)?;
                require!(a.consumed <= a.need, LaunchError::InvalidApproval);
            }
            _ => return err!(LaunchError::InvalidConfig),
        }
        capacity::consume(p, v, now, amount)?;
        let policy_key = p.key();
        let role = [v.role];
        let bump = [v.bump];
        let seeds: &[&[u8]] = &[b"launch-v4-vault", policy_key.as_ref(), &role, &bump];
        token::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.vault_token.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: v.to_account_info(),
                },
            )
            .with_signer(&[seeds]),
            amount,
            ctx.accounts.mint.decimals,
        )?;
        Ok(())
    }
    pub fn propose_change(
        ctx: Context<ProposeChange>,
        kind: u8,
        recovery: bool,
        nonce: u64,
    ) -> Result<()> {
        governance::propose(ctx, kind, recovery, nonce)
    }

    pub fn cancel_change(ctx: Context<CancelChange>) -> Result<()> {
        governance::cancel(ctx)
    }

    pub fn execute_change(ctx: Context<ExecuteChange>) -> Result<()> {
        governance::execute(ctx)
    }
}
