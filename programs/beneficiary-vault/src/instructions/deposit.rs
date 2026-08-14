use crate::constants::{MAX_ANNUAL_RELEASE_BPS, MIN_CLIFF_SECONDS, STATE_SEED, TOKEN_VAULT_SEED};
use crate::error::VaultError;
use crate::policy;
use crate::state::BeneficiaryVault;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

#[derive(Accounts)]
#[instruction(
    amount: u64,
    annual_release_bps: u16,
    cliff_seconds: i64,
    policy_hash: [u8; 32]
)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    /// CHECK: The beneficiary need not sign the one-time deposit. Its key is
    /// frozen in the state PDA and enforced as signer on every release.
    pub beneficiary: UncheckedAccount<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        mut,
        token::mint = mint,
        token::authority = depositor,
    )]
    pub depositor_token: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = depositor,
        space = 8 + BeneficiaryVault::INIT_SPACE,
        seeds = [STATE_SEED, beneficiary.key().as_ref(), mint.key().as_ref(), policy_hash.as_ref()],
        bump,
    )]
    pub vault_state: Account<'info, BeneficiaryVault>,
    #[account(
        init,
        payer = depositor,
        seeds = [TOKEN_VAULT_SEED, vault_state.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = vault_state,
    )]
    pub vault_token: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn deposit_handler(
    ctx: Context<Deposit>,
    amount: u64,
    annual_release_bps: u16,
    cliff_seconds: i64,
    policy_hash: [u8; 32],
) -> Result<()> {
    require!(amount > 0, VaultError::ZeroAmount);
    require!(
        (1..=MAX_ANNUAL_RELEASE_BPS).contains(&annual_release_bps),
        VaultError::InvalidAnnualReleaseRate
    );
    require!(
        cliff_seconds >= MIN_CLIFF_SECONDS,
        VaultError::CliffTooShort
    );
    require!(policy_hash != [0; 32], VaultError::ZeroPolicyHash);

    let cap = policy::monthly_cap(amount, annual_release_bps)?;
    require!(cap > 0, VaultError::ZeroMonthlyCap);
    let now = Clock::get()?.unix_timestamp;
    let cliff_end_ts = now
        .checked_add(cliff_seconds)
        .ok_or(VaultError::ArithmeticOverflow)?;

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

    let state = &mut ctx.accounts.vault_state;
    state.depositor = ctx.accounts.depositor.key();
    state.beneficiary = ctx.accounts.beneficiary.key();
    state.mint = ctx.accounts.mint.key();
    state.policy_hash = policy_hash;
    state.deposited_amount = amount;
    state.monthly_cap = cap;
    state.released_total = 0;
    state.released_this_period = 0;
    state.genesis_ts = now;
    state.cliff_end_ts = cliff_end_ts;
    state.current_period_index = 0;
    state.annual_release_bps = annual_release_bps;
    state.mint_decimals = ctx.accounts.mint.decimals;
    state.state_bump = ctx.bumps.vault_state;
    state.token_vault_bump = ctx.bumps.vault_token;

    emit!(DepositEvent {
        vault_state: state.key(),
        depositor: state.depositor,
        beneficiary: state.beneficiary,
        mint: state.mint,
        policy_hash,
        amount,
        monthly_cap: cap,
        cliff_end_ts,
    });
    Ok(())
}

#[event]
pub struct DepositEvent {
    pub vault_state: Pubkey,
    pub depositor: Pubkey,
    pub beneficiary: Pubkey,
    pub mint: Pubkey,
    pub policy_hash: [u8; 32],
    pub amount: u64,
    pub monthly_cap: u64,
    pub cliff_end_ts: i64,
}
