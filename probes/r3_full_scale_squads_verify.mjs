// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Read-only verifier for the R3-B2 full-scale Squads receipt. The input JSON is
// treated only as an address/signature index; every verdict is reconstructed
// from standard local RPC account bytes and Squads' account decoder.

import { Connection, PublicKey } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, getAccount, getMint } from "@solana/spl-token";
import * as multisig from "@sqds/multisig";
import { createHash } from "crypto";
import fs from "fs";
import { boundPolicyHash } from "./policy_identity.mjs";

const input = process.env.K4V_R3_SQUADS_RECEIPT_IN;
if (!input) throw new Error("K4V_R3_SQUADS_RECEIPT_IN is required");
const receipt = JSON.parse(fs.readFileSync(input));
const RPC = process.env.K4V_SURFPOOL_RPC ?? receipt.rpc;
if (!RPC?.startsWith("http://127.0.0.1:") && !RPC?.startsWith("http://localhost:")) {
  throw new Error("R3 verifier is restricted to a loopback RPC URL");
}
const connection = new Connection(RPC, "confirmed");
const B2 = new PublicKey("2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w");
const SQUADS = new PublicKey("SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf");
const EXPECTED_SUPPLY = 1_000_000_000_000_000_000n;
const EXPECTED_FOUNDER = 300_000_000_000_000_000n;
const EXPECTED_TREASURY = 500_000_000_000_000_000n;
const EXPECTED_GENESIS = 120_000_000_000_000_000n;
const EXPECTED_LP = 80_000_000_000_000_000n;
const EXPECTED_FOUNDER_CAP = 1_250_000_000_000_000n;
const EXPECTED_TREASURY_CAP = 2_083_333_333_333_333n;
const EXPECTED_VOLUME = 120_000_000_000_000_000n;
const EXPECTED_CAPACITY = 3_000_000_000_000_000n;

const P = value => {
  const key = new PublicKey(value);
  if (key.toBase58() !== value) throw new Error(`non-canonical public key: ${value}`);
  return key;
};
const same = (left, right) => left.equals(right);
const discriminator = name => createHash("sha256").update(`account:${name}`).digest().subarray(0, 8);
const u64 = value => { const bytes = Buffer.alloc(8); bytes.writeBigUInt64LE(value); return bytes; };

async function b2Account(address, type) {
  const key = P(address);
  const account = await connection.getAccountInfo(key, "confirmed");
  if (!account) throw new Error(`${type} account absent: ${address}`);
  if (!same(account.owner, B2)) throw new Error(`${type} owner mismatch`);
  if (account.data.length < 8 || !account.data.subarray(0, 8).equals(discriminator(type))) {
    throw new Error(`${type} discriminator mismatch`);
  }
  return account.data;
}

function reader(data) {
  let offset = 8;
  return {
    pk: () => { const value = new PublicKey(data.subarray(offset, offset + 32)); offset += 32; return value; },
    b32: () => { const value = data.subarray(offset, offset + 32); offset += 32; return value; },
    u8: () => data[offset++],
    u16: () => { const value = data.readUInt16LE(offset); offset += 2; return value; },
    u32: () => { const value = data.readUInt32LE(offset); offset += 4; return value; },
    u64: () => { const value = data.readBigUInt64LE(offset); offset += 8; return value; },
    i64: () => { const value = data.readBigInt64LE(offset); offset += 8; return value; },
    consumed: () => offset,
  };
}

function decodePolicy(data) {
  const r = reader(data);
  const value = {
    authority: r.pk(), mint: r.pk(), policyHash: r.b32(), genesisTs: r.i64(),
    currentPeriod: r.u64(), releasedThisPeriod: r.u64(), hardCeiling: r.u64(),
    silenceFloor: r.u64(), silenceGrace: r.i64(), vaultCount: r.u32(), bump: r.u8(),
  };
  if (r.consumed() !== data.length) throw new Error("PolicyWindow trailing or missing bytes");
  return value;
}

function decodeMarket(data) {
  const r = reader(data);
  const value = {
    oracle: r.pk(), policyHash: r.b32(), eligibleVolume: r.u64(), updatedAt: r.i64(),
    maxAge: r.i64(), reportCount: r.u64(), marketBps: r.u16(), bump: r.u8(),
    pendingOracle: r.pk(), pendingSince: r.i64(),
  };
  if (r.consumed() !== data.length) throw new Error("MarketInput trailing or missing bytes");
  return value;
}

function decodeVault(data) {
  const r = reader(data);
  const value = {
    kind: r.u8(), depositor: r.pk(), authority: r.pk(), mint: r.pk(), policyHash: r.b32(),
    deposited: r.u64(), monthlyCap: r.u64(), releasedTotal: r.u64(),
    releasedThisPeriod: r.u64(), currentPeriod: r.u64(), genesisTs: r.i64(),
    cliffEndTs: r.i64(), annualBps: r.u16(), decimals: r.u8(), stateBump: r.u8(), tokenBump: r.u8(),
  };
  if (r.consumed() !== data.length) throw new Error("CovenantVault trailing or missing bytes");
  return value;
}

function decodeApproval(data) {
  const r = reader(data);
  const value = {
    vault: r.pk(), approver: r.pk(), destination: r.pk(), period: r.u64(),
    approvedNeed: r.u64(), consumed: r.u64(), createdAt: r.i64(), bump: r.u8(),
  };
  if (r.consumed() !== data.length) throw new Error("Approval trailing or missing bytes");
  return value;
}

async function main() {
  const mintKey = P(receipt.mint);
  const vaultPda = P(receipt.vault_pda);
  const multisigPda = P(receipt.multisig);
  const policyKey = P(receipt.policy);
  const marketKey = P(receipt.market);
  const beneficiaryVaultKey = P(receipt.beneficiary_vault);
  const purposeVaultKey = P(receipt.purpose_vault);
  const approvalKey = P(receipt.approval_after_replacement);
  const policyHash = Buffer.from(receipt.policy_hash, "hex");
  if (policyHash.length !== 32) throw new Error("policy hash must be 32 bytes");
  if (receipt.schema === "k4v-r3-b2-full-scale-squads-local/v0.2" && typeof receipt.policy_spec_hash !== "string") throw new Error("bound policy specification hash is required");
  if (receipt.policy_spec_hash !== undefined) {
    const bound = boundPolicyHash(B2.toBuffer(), vaultPda.toBuffer(), mintKey.toBuffer(), Buffer.from(receipt.policy_spec_hash, "hex"));
    if (!policyHash.equals(bound)) throw new Error("policy identity does not bind the declared creator and mint");
  }


  const mint = await getMint(connection, mintKey, "confirmed", TOKEN_PROGRAM_ID);
  const tokenAddresses = {
    founderStaging: receipt.founder_staging,
    treasuryStaging: receipt.treasury_staging,
    genesis: receipt.genesis_account,
    lp: receipt.lp_account,
    founderVault: receipt.beneficiary_vault_token,
    purposeVault: receipt.purpose_vault_token,
    beneficiary: receipt.beneficiary_destination,
    contractor: receipt.contractor_destination,
  };
  const tokenAccounts = {};
  for (const [name, address] of Object.entries(tokenAddresses)) {
    const account = await getAccount(connection, P(address), "confirmed", TOKEN_PROGRAM_ID);
    if (!same(account.mint, mintKey)) throw new Error(`${name} belongs to a different mint`);
    tokenAccounts[name] = account;
  }

  const policy = decodePolicy(await b2Account(receipt.policy, "PolicyWindow"));
  const market = decodeMarket(await b2Account(receipt.market, "MarketInput"));
  const beneficiaryVault = decodeVault(await b2Account(receipt.beneficiary_vault, "CovenantVault"));
  const purposeVault = decodeVault(await b2Account(receipt.purpose_vault, "CovenantVault"));
  const approval = decodeApproval(await b2Account(receipt.approval_after_replacement, "Approval"));

  const derivedPolicy = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-policy"), policyHash], B2,
  )[0];
  const derivedMarket = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-market"), policyHash], B2,
  )[0];
  const derivedPurposeVault = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-vault"), policyHash, Buffer.from([1]), vaultPda.toBuffer(), mintKey.toBuffer()], B2,
  )[0];
  const derivedPurposeToken = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-token"), purposeVaultKey.toBuffer()], B2,
  )[0];
  const derivedApproval = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-approval"), purposeVaultKey.toBuffer(), u64(approval.period)], B2,
  )[0];
  const derivedSquadsVault = multisig.getVaultPda({ multisigPda, index: 0 })[0];
  const multisigAccountInfo = await connection.getAccountInfo(multisigPda, "confirmed");
  if (!multisigAccountInfo || !same(multisigAccountInfo.owner, SQUADS)) {
    throw new Error("multisig account absent or owner mismatch");
  }
  const multisigState = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const memberKeys = multisigState.members.map(member => member.key.toBase58());

  const allAmounts = Object.values(tokenAccounts).map(account => account.amount);
  const conserved = allAmounts.reduce((sum, amount) => sum + amount, 0n);
  const statuses = await connection.getSignatureStatuses(Object.values(receipt.transactions), {
    searchTransactionHistory: true,
  });
  const allSignaturesSuccessful = statuses.value.every(status => status && status.err === null);
  const purposeRelease = EXPECTED_CAPACITY - EXPECTED_FOUNDER_CAP;
  const checks = {
    receipt_declares_local_no_mainnet: receipt.cluster.startsWith("local-") &&
      receipt.no_official_mint === true && receipt.mainnet_authorized === false,
    candidate_remains_open: receipt.candidate_config?.open_parameter_count === 26,
    exact_supply_and_authority_revocation: mint.supply === EXPECTED_SUPPLY &&
      mint.mintAuthority === null && mint.freezeAuthority === null,
    all_token_accounts_share_one_mint: Object.values(tokenAccounts).every(account => same(account.mint, mintKey)),
    four_pool_and_release_conservation: conserved === EXPECTED_SUPPLY,
    staging_accounts_empty: tokenAccounts.founderStaging.amount === 0n && tokenAccounts.treasuryStaging.amount === 0n,
    untouched_allocations_exact: tokenAccounts.genesis.amount === EXPECTED_GENESIS && tokenAccounts.lp.amount === EXPECTED_LP,
    full_vault_deposits_exact: beneficiaryVault.deposited === EXPECTED_FOUNDER && purposeVault.deposited === EXPECTED_TREASURY,
    monthly_caps_exact: beneficiaryVault.monthlyCap === EXPECTED_FOUNDER_CAP && purposeVault.monthlyCap === EXPECTED_TREASURY_CAP,
    policy_and_market_pdas_canonical: same(policyKey, derivedPolicy) && same(marketKey, derivedMarket) &&
      [policy, market, beneficiaryVault, purposeVault].every(account => account.policyHash.equals(policyHash)),
    purpose_vault_and_token_pdas_canonical: same(purposeVaultKey, derivedPurposeVault) &&
      same(P(receipt.purpose_vault_token), derivedPurposeToken),
    approval_pda_canonical: same(approvalKey, derivedApproval),
    policy_and_purpose_authority_are_stable_squads_vault: same(policy.authority, vaultPda) &&
      same(purposeVault.authority, vaultPda) && same(vaultPda, derivedSquadsVault),
    policy_and_vault_mint_match: same(policy.mint, mintKey) && same(beneficiaryVault.mint, mintKey) &&
      same(purposeVault.mint, mintKey),
    market_report_exact: same(market.oracle, P(receipt.oracle)) && market.eligibleVolume === EXPECTED_VOLUME,
    post_replacement_approval_and_release_exact: same(approval.approver, vaultPda) &&
      same(approval.destination, P(receipt.contractor_destination)) && approval.consumed === purposeRelease &&
      tokenAccounts.contractor.amount === purposeRelease,
    shared_window_exhausted_exactly: policy.releasedThisPeriod === EXPECTED_CAPACITY &&
      tokenAccounts.beneficiary.amount === EXPECTED_FOUNDER_CAP,
    member_replacement_visible_in_multisig_state: multisigState.threshold === 2 && memberKeys.length === 3 &&
      !memberKeys.includes(receipt.member_replacement.lost) && memberKeys.includes(receipt.member_replacement.added),
    every_indexed_transaction_succeeded: allSignaturesSuccessful,
  };
  const valid = Object.values(checks).every(Boolean);
  const result = {
    schema: "k4v-r3-b2-full-scale-squads-rpc-verifier/v0.1",
    epistemic_status: "READ_ONLY_LOCAL_RPC_RECONSTRUCTION_NOT_INDEPENDENT_REVIEW",
    valid,
    source_receipt: input,
    cluster: RPC,
    mint: receipt.mint,
    conserved_total: conserved.toString(),
    policy_released_this_period: policy.releasedThisPeriod.toString(),
    members_after: memberKeys,
    checks,
  };
  if (!valid) throw new Error(`R3-B2 read-only checks failed: ${JSON.stringify(checks)}`);
  const output = process.env.K4V_R3_SQUADS_VERIFY_OUT;
  if (output) fs.writeFileSync(output, JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
}

main().catch(error => {
  console.error(error?.stack ?? error);
  process.exitCode = 1;
});
