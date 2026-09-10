use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

#[derive(Accounts)]
#[instruction(config: LaunchConfig, spec_hash: [u8; 32], identity: [u8; 32])]
pub struct PreparePolicy<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    /// CHECK: bound into immutable intent; must sign subsequent confirmation.
    pub founder: UncheckedAccount<'info>,
    /// CHECK: bound into immutable intent; must sign subsequent confirmation.
    pub treasury: UncheckedAccount<'info>,
    /// CHECK: identity only; subsequent reports require the frozen oracle signature.
    pub oracle: UncheckedAccount<'info>,
    pub mint: Account<'info, Mint>,
    #[account(init, payer = creator, space = 8 + LaunchPreparationV7::INIT_SPACE,
        seeds = [b"launch-v7-preparation", identity.as_ref()], bump)]
    pub preparation: Box<Account<'info, LaunchPreparationV7>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OpenPreparedPolicy<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    pub founder: Signer<'info>,
    pub treasury: Signer<'info>,
    /// CHECK: equality to the immutable preparation is verified by the handler.
    pub oracle: UncheckedAccount<'info>,
    pub recovery_one: Signer<'info>,
    pub recovery_two: Signer<'info>,
    pub recovery_three: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(seeds = [b"launch-v7-preparation", preparation.identity.as_ref()], bump = preparation.bump)]
    pub preparation: Box<Account<'info, LaunchPreparationV7>>,
    #[account(init, payer = creator, space = 8 + LaunchPolicyV7::INIT_SPACE,
        seeds = [b"launch-v7-policy", preparation.identity.as_ref()], bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(role: u8)]
pub struct Deposit<'info> {
    pub creator: Signer<'info>,
    #[account(mut)]
    pub depositor: Signer<'info>,
    pub authority: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump,
        constraint = policy.controller == creator.key() @ LaunchError::Unauthorized, has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    pub mint: Account<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = depositor)]
    pub source: Account<'info, TokenAccount>,
    #[account(init, payer = depositor, space = 8 + LaunchVaultV7::INIT_SPACE,
        seeds = [b"launch-v7-vault", policy.key().as_ref(), &[role]], bump)]
    pub vault: Account<'info, LaunchVaultV7>,
    #[account(init, payer = depositor, seeds = [b"launch-v7-token", vault.key().as_ref()], bump,
        token::mint = mint, token::authority = vault)]
    pub vault_token: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Control<'info> {
    pub creator: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump,
        constraint = policy.controller == creator.key() @ LaunchError::Unauthorized)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
}

#[derive(Accounts)]
pub struct ConsentControl<'info> {
    pub creator: Signer<'info>,
    pub founder: Signer<'info>,
    pub treasury: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump,
        constraint = policy.controller == creator.key() @ LaunchError::Unauthorized,
        constraint = policy.withdrawal[0].current == founder.key() @ LaunchError::Unauthorized,
        constraint = policy.withdrawal[1].current == treasury.key() @ LaunchError::Unauthorized)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
}

#[derive(Accounts)]
pub struct PolicyOnly<'info> {
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
}

#[derive(Accounts)]
pub struct Refund<'info> {
    pub depositor: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump,
        has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(seeds = [b"launch-v7-vault", policy.key().as_ref(), &[vault.role]], bump = vault.bump,
        has_one = policy, has_one = depositor)]
    pub vault: Account<'info, LaunchVaultV7>,
    pub mint: Account<'info, Mint>,
    #[account(mut, seeds = [b"launch-v7-token", vault.key().as_ref()], bump,
        token::mint = mint, token::authority = vault)]
    pub vault_token: Account<'info, TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = depositor)]
    pub destination: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Report<'info> {
    pub oracle: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump,
        has_one = oracle)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
}

#[derive(Accounts)]
#[instruction(period: u64)]
pub struct ApproveTreasury<'info> {
    #[account(mut)]
    pub treasury: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump,
        constraint = policy.withdrawal[1].current == treasury.key() @ LaunchError::WithdrawalAuthority, has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    pub mint: Account<'info, Mint>,
    #[account(token::mint = mint)]
    pub recipient: Account<'info, TokenAccount>,
    #[account(init, payer = treasury, space = 8 + TreasuryApprovalV7::INIT_SPACE,
        seeds = [b"launch-v7-approval", policy.key().as_ref(), &period.to_le_bytes()], bump)]
    pub approval: Account<'info, TreasuryApprovalV7>,
    pub system_program: Program<'info, System>,
    #[account(mut, seeds = [b"launch-v7-key", policy.key().as_ref(), recipient.owner.as_ref()],
        bump = recipient_record.bump, has_one = policy,
        constraint = recipient_record.subject == recipient.owner @ LaunchError::Unauthorized)]
    pub recipient_record: Account<'info, WithdrawalKeyV7>,
}

#[derive(Accounts)]
pub struct Release<'info> {
    pub authority: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump,
        has_one = mint)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(mut, seeds = [b"launch-v7-vault", policy.key().as_ref(), &[vault.role]], bump = vault.bump,
        has_one = policy)]
    pub vault: Box<Account<'info, LaunchVaultV7>>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(mut, seeds = [b"launch-v7-token", vault.key().as_ref()], bump,
        token::mint = mint, token::authority = vault)]
    pub vault_token: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = mint)]
    pub destination: Box<Account<'info, TokenAccount>>,
    // Treasury path validates this typed account's PDA, policy and period before use.
    #[account(mut)]
    pub approval: Option<Box<Account<'info, TreasuryApprovalV7>>>,
    pub token_program: Program<'info, Token>,
    // Treasury requires its canonical recipient index; Founder requires None.
    pub recipient_record: Option<Box<Account<'info, WithdrawalKeyV7>>>,
}

#[derive(Accounts)]
#[instruction(subject: Pubkey)]
pub struct PrepareWithdrawalKey<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(init, payer = payer, space = 8 + WithdrawalKeyV7::INIT_SPACE,
        seeds = [b"launch-v7-key", policy.key().as_ref(), subject.as_ref()], bump)]
    pub record: Account<'info, WithdrawalKeyV7>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(role: u8, recovery: bool, nonce: u64)]
pub struct ProposeWithdrawal<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub initiator: Signer<'info>,
    pub cosigner: Signer<'info>,
    pub successor: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(init, payer = payer, space = 8 + WithdrawalProposalV7::INIT_SPACE,
        seeds = [b"launch-v7-withdraw", policy.key().as_ref(), &[role], &nonce.to_le_bytes()], bump)]
    pub proposal: Account<'info, WithdrawalProposalV7>,
    #[account(mut, seeds = [b"launch-v7-key", policy.key().as_ref(), successor.key().as_ref()],
        bump = successor_record.bump, has_one = policy,
        constraint = successor_record.subject == successor.key() @ LaunchError::Unauthorized)]
    pub successor_record: Account<'info, WithdrawalKeyV7>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ExecuteWithdrawal<'info> {
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(mut, seeds = [b"launch-v7-withdraw", policy.key().as_ref(), &[proposal.role], &proposal.nonce.to_le_bytes()],
        bump = proposal.bump, has_one = policy)]
    pub proposal: Account<'info, WithdrawalProposalV7>,
    #[account(mut, seeds = [b"launch-v7-key", policy.key().as_ref(), proposal.successor.as_ref()],
        bump = successor_record.bump, has_one = policy,
        constraint = successor_record.subject == proposal.successor @ LaunchError::Unauthorized)]
    pub successor_record: Account<'info, WithdrawalKeyV7>,
}

#[derive(Accounts)]
pub struct CancelWithdrawal<'info> {
    pub initiator: Signer<'info>,
    pub cosigner: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(mut, seeds = [b"launch-v7-withdraw", policy.key().as_ref(), &[proposal.role], &proposal.nonce.to_le_bytes()],
        bump = proposal.bump, has_one = policy)]
    pub proposal: Account<'info, WithdrawalProposalV7>,
    #[account(mut, seeds = [b"launch-v7-key", policy.key().as_ref(), proposal.successor.as_ref()],
        bump = successor_record.bump, has_one = policy,
        constraint = successor_record.subject == proposal.successor @ LaunchError::Unauthorized)]
    pub successor_record: Account<'info, WithdrawalKeyV7>,
}

#[derive(Accounts)]
#[instruction(kind: u8, recovery: bool, nonce: u64)]
pub struct ProposeChange<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub initiator: Signer<'info>,
    pub cosigner: Signer<'info>,
    pub successor: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(init, payer = payer, space = 8 + ChangeProposalV7::INIT_SPACE,
        seeds = [b"launch-v7-change", policy.key().as_ref(), &nonce.to_le_bytes()], bump)]
    pub proposal: Account<'info, ChangeProposalV7>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelChange<'info> {
    pub initiator: Signer<'info>,
    pub cosigner: Signer<'info>,
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(mut, seeds = [b"launch-v7-change", policy.key().as_ref(), &proposal.nonce.to_le_bytes()],
        bump = proposal.bump, has_one = policy)]
    pub proposal: Account<'info, ChangeProposalV7>,
}

#[derive(Accounts)]
pub struct ExecuteChange<'info> {
    #[account(mut, seeds = [b"launch-v7-policy", policy.identity.as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    #[account(mut, seeds = [b"launch-v7-change", policy.key().as_ref(), &proposal.nonce.to_le_bytes()],
        bump = proposal.bump, has_one = policy)]
    pub proposal: Account<'info, ChangeProposalV7>,
}
