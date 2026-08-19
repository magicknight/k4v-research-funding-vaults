use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod instructions;
pub mod policy;
pub mod state;

use instructions::*;
use state::VaultKind;

declare_id!("2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w");

#[program]
pub mod purpose_vault {
    use super::*;

    /// Create the shared capacity window and its market input. Every vault on
    /// this policy indexes the same 30-day periods and debits one counter.
    pub fn open_policy(
        ctx: Context<OpenPolicy>,
        policy_hash: [u8; 32],
        market_capacity_bps: u16,
        max_age_seconds: i64,
    ) -> Result<()> {
        instructions::open_policy::open_policy_handler(
            ctx,
            policy_hash,
            market_capacity_bps,
            max_age_seconds,
        )
    }

    /// The frozen oracle reports eligible trailing 30-day spot volume. It can
    /// do nothing else: no transfer, no cap change, no destination.
    pub fn report_volume(ctx: Context<ReportVolume>, eligible_volume: u64) -> Result<()> {
        instructions::report_volume::report_volume_handler(ctx, eligible_volume)
    }

    pub fn deposit(
        ctx: Context<Deposit>,
        kind: VaultKind,
        amount: u64,
        annual_release_bps: u16,
        cliff_seconds: i64,
    ) -> Result<()> {
        instructions::deposit::deposit_handler(ctx, kind, amount, annual_release_bps, cliff_seconds)
    }

    /// Record an approved need for a future period. The account's creation
    /// timestamp is what makes the notice period a fact rather than a claim.
    pub fn approve(ctx: Context<Approve>, period_index: u64, approved_need: u64) -> Result<()> {
        instructions::approve::approve_handler(ctx, period_index, approved_need)
    }

    pub fn release_beneficiary(ctx: Context<ReleaseBeneficiary>, amount: u64) -> Result<()> {
        instructions::release_beneficiary::release_beneficiary_handler(ctx, amount)
    }

    pub fn release_purpose(ctx: Context<ReleasePurpose>, amount: u64) -> Result<()> {
        instructions::release_purpose::release_purpose_handler(ctx, amount)
    }
}

#[cfg(test)]
mod surface {
    /// `spec/PURPOSE_VAULT_B2.md` claims B2 has no update, configure, close,
    /// migrate, emergency-release, alternate-destination or administrative
    /// transfer instruction. That claim is only worth as much as something that
    /// fails when a seventh entry point appears, so this asserts it directly.
    #[test]
    fn the_program_exposes_exactly_six_instructions() {
        let source = include_str!("lib.rs");
        let module = source
            .split_once("pub mod purpose_vault {")
            .expect("the program module must be present")
            .1;
        let names: Vec<&str> = module
            .lines()
            .filter_map(|line| line.trim().strip_prefix("pub fn "))
            .filter_map(|rest| rest.split(['(', '<']).next())
            .collect();
        assert_eq!(
            names,
            [
                "open_policy",
                "report_volume",
                "deposit",
                "approve",
                "release_beneficiary",
                "release_purpose",
            ]
        );
    }
}
