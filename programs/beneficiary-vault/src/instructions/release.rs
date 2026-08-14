use crate::constants::{STATE_SEED, TOKEN_VAULT_SEED};
use crate::error::VaultError;
use crate::policy;
use crate::state::BeneficiaryVault;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

#[derive(Accounts)]
pub struct Release<'info> {
    pub beneficiary: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        mut,
        has_one = beneficiary,
        has_one = mint,
        seeds = [
            STATE_SEED,
            beneficiary.key().as_ref(),
            mint.key().as_ref(),
            vault_state.policy_hash.as_ref(),
        ],
        bump = vault_state.state_bump,
    )]
    pub vault_state: Account<'info, BeneficiaryVault>,
    #[account(
        mut,
        seeds = [TOKEN_VAULT_SEED, vault_state.key().as_ref()],
        bump = vault_state.token_vault_bump,
        token::mint = mint,
        token::authority = vault_state,
    )]
    pub vault_token: Account<'info, TokenAccount>,
    #[account(
        mut,
        token::mint = mint,
        token::authority = beneficiary,
    )]
    pub beneficiary_token: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

pub fn release_handler(ctx: Context<Release>, amount: u64) -> Result<()> {
    require!(amount > 0, VaultError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    let period = policy::period_index(now, ctx.accounts.vault_state.cliff_end_ts)?;

    let state = &mut ctx.accounts.vault_state;
    if period > state.current_period_index {
        state.current_period_index = period;
        state.released_this_period = 0;
    }
    let next_period_release = state
        .released_this_period
        .checked_add(amount)
        .ok_or(VaultError::ArithmeticOverflow)?;
    require!(
        next_period_release <= state.monthly_cap,
        VaultError::PeriodCapExceeded
    );
    let next_total = state
        .released_total
        .checked_add(amount)
        .ok_or(VaultError::ArithmeticOverflow)?;
    require!(
        next_total <= state.deposited_amount,
        VaultError::DepositExceeded
    );

    let beneficiary = state.beneficiary;
    let mint = state.mint;
    let policy_hash = state.policy_hash;
    let bump = [state.state_bump];
    let signer_seeds: &[&[u8]] = &[
        STATE_SEED,
        beneficiary.as_ref(),
        mint.as_ref(),
        policy_hash.as_ref(),
        &bump,
    ];

    token::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.vault_token.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.beneficiary_token.to_account_info(),
                authority: state.to_account_info(),
            },
            &[signer_seeds],
        ),
        amount,
        state.mint_decimals,
    )?;

    state.released_this_period = next_period_release;
    state.released_total = next_total;
    emit!(ReleaseEvent {
        vault_state: state.key(),
        beneficiary,
        amount,
        period_index: period,
        released_this_period: next_period_release,
        released_total: next_total,
    });
    Ok(())
}

#[event]
pub struct ReleaseEvent {
    pub vault_state: Pubkey,
    pub beneficiary: Pubkey,
    pub amount: u64,
    pub period_index: u64,
    pub released_this_period: u64,
    pub released_total: u64,
}
