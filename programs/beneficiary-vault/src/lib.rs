use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod instructions;
pub mod policy;
pub mod state;

use instructions::*;

declare_id!("BzeaJbgEEbJd14yyMad1BbemTUHWepXh6SeZgX5Yt7gM");

#[program]
pub mod beneficiary_vault {
    use super::*;

    pub fn deposit(
        ctx: Context<Deposit>,
        amount: u64,
        annual_release_bps: u16,
        cliff_seconds: i64,
        policy_hash: [u8; 32],
    ) -> Result<()> {
        instructions::deposit::deposit_handler(
            ctx,
            amount,
            annual_release_bps,
            cliff_seconds,
            policy_hash,
        )
    }

    pub fn release(ctx: Context<Release>, amount: u64) -> Result<()> {
        instructions::release::release_handler(ctx, amount)
    }
}
