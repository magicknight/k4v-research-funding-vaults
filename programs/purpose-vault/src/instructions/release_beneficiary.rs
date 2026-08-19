use crate::constants::{MARKET_SEED, POLICY_SEED, TOKEN_VAULT_SEED, VAULT_SEED};
use crate::error::CovenantError;
use crate::instructions::common::{apply_release, ReleaseGate};
use crate::policy;
use crate::state::{CovenantVault, MarketInput, PolicyWindow, VaultKind};
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

#[derive(Accounts)]
pub struct ReleaseBeneficiary<'info> {
    pub beneficiary: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        mut,
        has_one = mint,
        seeds = [
            VAULT_SEED,
            vault.policy_hash.as_ref(),
            &[vault.kind.seed_byte()],
            beneficiary.key().as_ref(),
            mint.key().as_ref(),
        ],
        bump = vault.state_bump,
    )]
    pub vault: Account<'info, CovenantVault>,
    #[account(
        mut,
        seeds = [POLICY_SEED, vault.policy_hash.as_ref()],
        bump = policy.bump,
    )]
    pub policy: Account<'info, PolicyWindow>,
    #[account(
        seeds = [MARKET_SEED, vault.policy_hash.as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, MarketInput>,
    #[account(
        mut,
        seeds = [TOKEN_VAULT_SEED, vault.key().as_ref()],
        bump = vault.token_vault_bump,
        token::mint = mint,
        token::authority = vault,
    )]
    pub vault_token: Account<'info, TokenAccount>,
    #[account(
        mut,
        token::mint = mint,
        token::authority = beneficiary,
    )]
    pub destination: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

pub fn release_beneficiary_handler(ctx: Context<ReleaseBeneficiary>, amount: u64) -> Result<()> {
    require!(
        ctx.accounts.vault.kind == VaultKind::Beneficiary,
        CovenantError::WrongVaultKind
    );

    let now = Clock::get()?.unix_timestamp;
    require!(
        now >= ctx.accounts.vault.cliff_end_ts,
        CovenantError::CliffActive
    );
    let period = policy::period_index(now, ctx.accounts.policy.genesis_ts)?;

    let capacity = apply_release(
        ReleaseGate {
            policy: &mut ctx.accounts.policy,
            vault: &mut ctx.accounts.vault,
            market: &ctx.accounts.market,
        },
        now,
        period,
        amount,
    )?;

    let vault = &ctx.accounts.vault;
    let policy_hash = vault.policy_hash;
    let kind_byte = [vault.kind.seed_byte()];
    let authority = vault.authority;
    let mint = vault.mint;
    let bump = [vault.state_bump];
    let signer_seeds: &[&[u8]] = &[
        VAULT_SEED,
        policy_hash.as_ref(),
        &kind_byte,
        authority.as_ref(),
        mint.as_ref(),
        &bump,
    ];

    token::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.vault_token.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                to: ctx.accounts.destination.to_account_info(),
                authority: vault.to_account_info(),
            },
            &[signer_seeds],
        ),
        amount,
        vault.mint_decimals,
    )?;

    emit!(BeneficiaryReleased {
        vault: vault.key(),
        policy: ctx.accounts.policy.key(),
        beneficiary: authority,
        amount,
        period_index: period,
        vault_released_this_period: vault.released_this_period,
        vault_released_total: vault.released_total,
        policy_released_this_period: ctx.accounts.policy.released_this_period,
        market_capacity: capacity,
    });
    Ok(())
}

#[event]
pub struct BeneficiaryReleased {
    pub vault: Pubkey,
    pub policy: Pubkey,
    pub beneficiary: Pubkey,
    pub amount: u64,
    pub period_index: u64,
    pub vault_released_this_period: u64,
    pub vault_released_total: u64,
    pub policy_released_this_period: u64,
    pub market_capacity: u64,
}
