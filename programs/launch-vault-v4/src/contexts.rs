use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

#[derive(Accounts)]
#[instruction(config: LaunchConfig, spec_hash: [u8; 32], identity: [u8; 32])]
pub struct OpenPolicy<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    pub founder: Signer<'info>,
    pub treasury: Signer<'info>,
    /// CHECK: frozen identity only; reports subsequently require this signer.
    pub oracle: UncheckedAccount<'info>,
    pub recovery_one: Signer<'info>,
    pub recovery_two: Signer<'info>,
    pub recovery_three: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(init, payer = creator, space = 8 + LaunchPolicyV4::INIT_SPACE,
        seeds = [b"launch-v4-policy", identity.as_ref()], bump)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(role: u8)]
pub struct Deposit<'info> {
    pub creator: Signer<'info>,
    #[account(mut)]
    pub depositor: Signer<'info>,
    pub authority: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump,
        constraint = policy.controller == creator.key() @ LaunchError::Unauthorized, has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    pub mint: Account<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = depositor)]
    pub source: Account<'info, TokenAccount>,
    #[account(init, payer = depositor, space = 8 + LaunchVaultV4::INIT_SPACE,
        seeds = [b"launch-v4-vault", policy.key().as_ref(), &[role]], bump)]
    pub vault: Account<'info, LaunchVaultV4>,
    #[account(init, payer = depositor, seeds = [b"launch-v4-token", vault.key().as_ref()], bump,
        token::mint = mint, token::authority = vault)]
    pub vault_token: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Control<'info> {
    pub creator: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump,
        constraint = policy.controller == creator.key() @ LaunchError::Unauthorized)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
}

#[derive(Accounts)]
pub struct ConsentControl<'info> {
    pub creator: Signer<'info>,
    pub founder: Signer<'info>,
    pub treasury: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump,
        constraint = policy.controller == creator.key() @ LaunchError::Unauthorized, has_one = founder, has_one = treasury)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
}

#[derive(Accounts)]
pub struct PolicyOnly<'info> {
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
}

#[derive(Accounts)]
pub struct Refund<'info> {
    pub depositor: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump,
        has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    #[account(seeds = [b"launch-v4-vault", policy.key().as_ref(), &[vault.role]], bump = vault.bump,
        has_one = policy, has_one = depositor)]
    pub vault: Account<'info, LaunchVaultV4>,
    pub mint: Account<'info, Mint>,
    #[account(mut, seeds = [b"launch-v4-token", vault.key().as_ref()], bump,
        token::mint = mint, token::authority = vault)]
    pub vault_token: Account<'info, TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = depositor)]
    pub destination: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Report<'info> {
    pub oracle: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump,
        has_one = oracle)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
}

#[derive(Accounts)]
#[instruction(period: u64)]
pub struct ApproveTreasury<'info> {
    #[account(mut)]
    pub treasury: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump,
        has_one = treasury, has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    pub mint: Account<'info, Mint>,
    #[account(token::mint = mint)]
    pub recipient: Account<'info, TokenAccount>,
    #[account(init, payer = treasury, space = 8 + TreasuryApprovalV4::INIT_SPACE,
        seeds = [b"launch-v4-approval", policy.key().as_ref(), &period.to_le_bytes()], bump)]
    pub approval: Account<'info, TreasuryApprovalV4>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Release<'info> {
    pub authority: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump,
        has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    #[account(mut, seeds = [b"launch-v4-vault", policy.key().as_ref(), &[vault.role]], bump = vault.bump,
        has_one = policy, has_one = authority)]
    pub vault: Account<'info, LaunchVaultV4>,
    pub mint: Account<'info, Mint>,
    #[account(mut, seeds = [b"launch-v4-token", vault.key().as_ref()], bump,
        token::mint = mint, token::authority = vault)]
    pub vault_token: Account<'info, TokenAccount>,
    #[account(mut, token::mint = mint)]
    pub destination: Account<'info, TokenAccount>,
    // Treasury path validates this typed account's PDA, policy and period before use.
    #[account(mut)]
    pub approval: Option<Account<'info, TreasuryApprovalV4>>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(kind: u8, recovery: bool, nonce: u64)]
pub struct ProposeChange<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub initiator: Signer<'info>,
    pub cosigner: Signer<'info>,
    pub successor: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    #[account(init, payer = payer, space = 8 + ChangeProposalV4::INIT_SPACE,
        seeds = [b"launch-v4-change", policy.key().as_ref(), &nonce.to_le_bytes()], bump)]
    pub proposal: Account<'info, ChangeProposalV4>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelChange<'info> {
    pub initiator: Signer<'info>,
    pub cosigner: Signer<'info>,
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    #[account(mut, seeds = [b"launch-v4-change", policy.key().as_ref(), &proposal.nonce.to_le_bytes()],
        bump = proposal.bump, has_one = policy)]
    pub proposal: Account<'info, ChangeProposalV4>,
}

#[derive(Accounts)]
pub struct ExecuteChange<'info> {
    #[account(mut, seeds = [b"launch-v4-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV4>>,
    #[account(mut, seeds = [b"launch-v4-change", policy.key().as_ref(), &proposal.nonce.to_le_bytes()],
        bump = proposal.bump, has_one = policy)]
    pub proposal: Account<'info, ChangeProposalV4>,
}
