use crate::constants::{
    MAX_ANNUAL_RELEASE_BPS, MIN_CLIFF_SECONDS, POLICY_SEED, TOKEN_VAULT_SEED, VAULT_SEED,
};
use crate::error::CovenantError;
use crate::policy;
use crate::state::{CovenantVault, PolicyWindow, VaultKind};
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

#[derive(Accounts)]
#[instruction(kind: VaultKind, amount: u64, annual_release_bps: u16, cliff_seconds: i64)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    /// The policy owner co-signs. A shared capacity window is not open to
    /// strangers: without this, anyone could attach a vault to someone else's
    /// policy and consume their monthly market capacity.
    pub policy_authority: Signer<'info>,
    /// CHECK: beneficiary or approver. Frozen into the vault PDA seeds and
    /// required to sign every release of this vault.
    pub authority: UncheckedAccount<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        mut,
        token::mint = mint,
        token::authority = depositor,
    )]
    pub depositor_token: Account<'info, TokenAccount>,
    #[account(
        mut,
        has_one = mint,
        constraint = policy.authority == policy_authority.key() @ CovenantError::WrongPolicyAuthority,
        seeds = [POLICY_SEED, policy.policy_hash.as_ref()],
        bump = policy.bump,
    )]
    pub policy: Account<'info, PolicyWindow>,
    #[account(
        init,
        payer = depositor,
        space = 8 + CovenantVault::INIT_SPACE,
        seeds = [
            VAULT_SEED,
            policy.policy_hash.as_ref(),
            &[kind.seed_byte()],
            authority.key().as_ref(),
            mint.key().as_ref(),
        ],
        bump,
    )]
    pub vault: Account<'info, CovenantVault>,
    #[account(
        init,
        payer = depositor,
        seeds = [TOKEN_VAULT_SEED, vault.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = vault,
    )]
    pub vault_token: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn deposit_handler(
    ctx: Context<Deposit>,
    kind: VaultKind,
    amount: u64,
    annual_release_bps: u16,
    cliff_seconds: i64,
) -> Result<()> {
    require!(amount > 0, CovenantError::ZeroAmount);
    require!(
        (1..=MAX_ANNUAL_RELEASE_BPS).contains(&annual_release_bps),
        CovenantError::InvalidAnnualReleaseRate
    );
    match kind {
        VaultKind::Beneficiary => require!(
            cliff_seconds >= MIN_CLIFF_SECONDS,
            CovenantError::CliffTooShort
        ),
        // A purpose vault's gate is an approved need, not the passage of time.
        // Allowing a cliff here would invite a vault that looks time-locked
        // while its real constraint is somewhere else entirely.
        VaultKind::Purpose => require!(cliff_seconds == 0, CovenantError::PurposeCliffNotZero),
    }

    let cap = policy::monthly_cap(amount, annual_release_bps)?;
    require!(cap > 0, CovenantError::ZeroMonthlyCap);

    let now = Clock::get()?.unix_timestamp;
    let cliff_end_ts = now
        .checked_add(cliff_seconds)
        .ok_or(CovenantError::ArithmeticOverflow)?;

    token::transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.depositor_token.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.vault_token.to_account_info(),
                authority: ctx.accounts.depositor.to_account_info(),
            },
        ),
        amount,
        ctx.accounts.mint.decimals,
    )?;

    let policy_hash = ctx.accounts.policy.policy_hash;
    let vault = &mut ctx.accounts.vault;
    vault.kind = kind;
    vault.depositor = ctx.accounts.depositor.key();
    vault.authority = ctx.accounts.authority.key();
    vault.mint = ctx.accounts.mint.key();
    vault.policy_hash = policy_hash;
    vault.deposited_amount = amount;
    vault.monthly_cap = cap;
    vault.released_total = 0;
    vault.released_this_period = 0;
    vault.current_period_index = 0;
    vault.genesis_ts = now;
    vault.cliff_end_ts = cliff_end_ts;
    vault.annual_release_bps = annual_release_bps;
    vault.mint_decimals = ctx.accounts.mint.decimals;
    vault.state_bump = ctx.bumps.vault;
    vault.token_vault_bump = ctx.bumps.vault_token;

    let vault_key = vault.key();
    let policy = &mut ctx.accounts.policy;
    policy.vault_count = policy
        .vault_count
        .checked_add(1)
        .ok_or(CovenantError::ArithmeticOverflow)?;

    emit!(DepositEvent {
        vault: vault_key,
        policy: policy.key(),
        kind,
        depositor: ctx.accounts.depositor.key(),
        authority: ctx.accounts.authority.key(),
        mint: ctx.accounts.mint.key(),
        policy_hash,
        amount,
        monthly_cap: cap,
        cliff_end_ts,
        vault_count: policy.vault_count,
    });
    Ok(())
}

#[event]
pub struct DepositEvent {
    pub vault: Pubkey,
    pub policy: Pubkey,
    pub kind: VaultKind,
    pub depositor: Pubkey,
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub amount: u64,
    pub monthly_cap: u64,
    pub cliff_end_ts: i64,
    pub vault_count: u32,
}
