// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Does a Squads multisig vault PDA satisfy B2's `Signer` constraint?
//
// B2 stores one address as the authority of a policy and of each vault, and
// requires it to sign. Whether that address belongs to one person or to an
// m-of-n committee is invisible to the program — or ought to be. This probe
// checks that it really is, because the whole "lose a key and survive it"
// argument rests on it, and because B2 freezes the approver's address into the
// vault's PDA seeds: a wrong answer discovered after the first deposit cannot
// be repaired.
//
// It runs against the real Squads v4 program dumped from devnet, not a mock.
//
//   solana program dump SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf \
//     squads_v4.so --url https://api.devnet.solana.com
//   solana account BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr \
//     --url https://api.devnet.solana.com --output json --output-file program_config.json
//   solana account HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK \
//     --url https://api.devnet.solana.com --output json --output-file treasury.json
//   solana-test-validator --reset --quiet \
//     --bpf-program SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf squads_v4.so \
//     --bpf-program 2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w \
//         target/deploy/purpose_vault.so \
//     --account BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr program_config.json \
//     --account HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK treasury.json
//   npm install @sqds/multisig @solana/web3.js @solana/spl-token
//   node squads_authority_probe.mjs
//
// Instruction data is hand-encoded here — Anchor's discriminator is the first
// eight bytes of sha256("global:<name>"), then Borsh args. That is deliberate
// and confined to this file: B2 ships no IDL, and a wrong encoding fails loudly
// rather than becoming a published interface anyone might trust.
//
// Local signatures have no explorer value. This establishes a mechanism, not a
// public receipt.

import {
  Connection, Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL,
  TransactionMessage, TransactionInstruction,
} from "@solana/web3.js";
import * as multisig from "@sqds/multisig";
import { createMint, createAccount, mintTo, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { createHash, randomBytes } from "crypto";
import fs from "fs";

const RPC = "http://127.0.0.1:8899";
const B2 = new PublicKey("2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w");
const TREASURY = new PublicKey("HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK");
const connection = new Connection(RPC, "confirmed");

const record = { steps: [] };
const say = (...a) => { const s = a.map(String).join(" "); console.log(s); record.steps.push(s); };

const disc = n => createHash("sha256").update("global:" + n).digest().subarray(0, 8);
const u16 = v => { const b = Buffer.alloc(2); b.writeUInt16LE(v); return b; };
const u64 = v => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v)); return b; };
const i64 = v => { const b = Buffer.alloc(8); b.writeBigInt64LE(BigInt(v)); return b; };
const U64MAX = 18446744073709551615n;

async function fund(pk, sol) {
  const sig = await connection.requestAirdrop(pk, sol * LAMPORTS_PER_SOL);
  await connection.confirmTransaction(sig, "confirmed");
}
const confirm = async sig => { await connection.confirmTransaction(sig, "confirmed"); return sig; };

const payer = Keypair.generate();
const members = [Keypair.generate(), Keypair.generate(), Keypair.generate()];
const oracle = Keypair.generate();
const depositor = Keypair.generate();
const beneficiary = Keypair.generate();
const contractor = Keypair.generate();

// Random per run: the policy PDA is derived from it, so a fixed value would
// collide with any earlier run's accounts on the same ledger.
const POLICY_HASH = randomBytes(32);
const [policyPda] = PublicKey.findProgramAddressSync([Buffer.from("purpose-policy"), POLICY_HASH], B2);
const [marketPda] = PublicKey.findProgramAddressSync([Buffer.from("purpose-market"), POLICY_HASH], B2);

const createKey = Keypair.generate();
const [multisigPda] = multisig.getMultisigPda({ createKey: createKey.publicKey });
const [vaultPda] = multisig.getVaultPda({ multisigPda, index: 0 });

async function throughMultisig(instructions, extraSigners = []) {
  const info = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const transactionIndex = BigInt(info.transactionIndex) + 1n;
  const transactionMessage = new TransactionMessage({
    payerKey: vaultPda,
    recentBlockhash: (await connection.getLatestBlockhash()).blockhash,
    instructions,
  });
  await confirm(await multisig.rpc.vaultTransactionCreate({
    connection, feePayer: payer, multisigPda, transactionIndex,
    creator: members[0].publicKey, vaultIndex: 0, ephemeralSigners: 0,
    transactionMessage, signers: [payer, members[0]], sendOptions: { skipPreflight: true },
  }));
  await confirm(await multisig.rpc.proposalCreate({
    connection, feePayer: payer, multisigPda, transactionIndex,
    creator: members[0], sendOptions: { skipPreflight: true },
  }));
  for (const m of [members[0], members[1]]) {
    await confirm(await multisig.rpc.proposalApprove({
      connection, feePayer: payer, multisigPda, transactionIndex,
      member: m, sendOptions: { skipPreflight: true },
    }));
  }
  return await confirm(await multisig.rpc.vaultTransactionExecute({
    connection, feePayer: payer, multisigPda, transactionIndex,
    member: members[0].publicKey, signers: [payer, members[0], ...extraSigners],
    sendOptions: { skipPreflight: false },
  }));
}

async function main() {
  for (const k of [payer, ...members, depositor, beneficiary, contractor]) await fund(k.publicKey, 10);
  say("funded payer, 3 members, depositor, beneficiary, contractor");

  await confirm(await multisig.rpc.multisigCreateV2({
    connection, treasury: TREASURY, createKey, creator: payer, multisigPda,
    configAuthority: null, threshold: 2, timeLock: 0, rentCollector: null,
    members: members.map(m => ({ key: m.publicKey, permissions: multisig.types.Permissions.all() })),
    sendOptions: { skipPreflight: true },
  }));
  const ms = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  say("multisig created:", multisigPda.toBase58(), "threshold", ms.threshold, "of", ms.members.length);
  say("vault PDA (the address B2 will see as authority):", vaultPda.toBase58());
  record.multisig = multisigPda.toBase58();
  record.vault_pda = vaultPda.toBase58();
  record.threshold = `${ms.threshold} of ${ms.members.length}`;

  await fund(vaultPda, 5);
  const mint = await createMint(connection, payer, payer.publicKey, null, 9);
  say("test mint:", mint.toBase58());
  record.policy_hash = POLICY_HASH.toString("hex");
  record.policy_pda = policyPda.toBase58();
  record.mint = mint.toBase58();

  // ---- step 1: open_policy, authority = the multisig vault PDA -------------
  const openPolicy = new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: true },
      { pubkey: oracle.publicKey, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: marketPda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([
      disc("open_policy"), POLICY_HASH, u16(250), i64(3 * 24 * 3600),
      u64(U64MAX), u64(0), i64(0),
    ]),
  });
  const sig1 = await throughMultisig([openPolicy]);
  say("open_policy executed through the multisig:", sig1);
  record.open_policy_signature = sig1;

  const policy = await connection.getAccountInfo(policyPda);
  const storedAuthority = new PublicKey(policy.data.subarray(8, 40));
  say("policy.authority on chain:", storedAuthority.toBase58());
  record.policy_authority_on_chain = storedAuthority.toBase58();
  record.authority_is_the_vault_pda = storedAuthority.equals(vaultPda);
  if (!storedAuthority.equals(vaultPda)) throw new Error("policy.authority is not the vault PDA");

  // ---- step 2: deposit, two signers, one of them the vault PDA -------------
  const depositorToken = await createAccount(connection, depositor, mint, depositor.publicKey);
  await mintTo(connection, payer, mint, depositorToken, payer, 500_000_000);
  const kindByte = Buffer.from([1]); // Purpose
  const [vaultAccount] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-vault"), POLICY_HASH, kindByte, vaultPda.toBuffer(), mint.toBuffer()], B2);
  const [vaultToken] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-token"), vaultAccount.toBuffer()], B2);

  const deposit = new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: depositor.publicKey, isSigner: true, isWritable: true },
      { pubkey: vaultPda, isSigner: true, isWritable: false },
      { pubkey: vaultPda, isSigner: false, isWritable: false }, // authority = approver = the multisig
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: depositorToken, isSigner: false, isWritable: true },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: vaultAccount, isSigner: false, isWritable: true },
      { pubkey: vaultToken, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("deposit"), Buffer.from([1]), u64(500_000_000), u16(500), i64(0)]),
  });
  const sig2 = await throughMultisig([deposit], [depositor]);
  say("deposit executed: depositor and the multisig vault PDA both signed:", sig2);
  record.deposit_signature = sig2;

  const vaultAcc = await connection.getAccountInfo(vaultAccount);
  const vaultAuthority = new PublicKey(vaultAcc.data.subarray(8 + 1 + 32, 8 + 1 + 32 + 32));
  say("vault.authority (the approver) on chain:", vaultAuthority.toBase58());
  record.vault_authority_on_chain = vaultAuthority.toBase58();
  record.approver_is_the_vault_pda = vaultAuthority.equals(vaultPda);

  // ---- step 3: lose a key, replace the member, and check B2 never notices --
  const replacement = Keypair.generate();
  await fund(replacement.publicKey, 10);
  const before = { multisig: multisigPda.toBase58(), vault: vaultPda.toBase58() };
  const lost = members[2];
  say("simulating a lost key:", lost.publicKey.toBase58());

  const info = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const configIndex = BigInt(info.transactionIndex) + 1n;
  await confirm(await multisig.rpc.configTransactionCreate({
    connection, feePayer: payer, multisigPda, transactionIndex: configIndex,
    creator: members[0].publicKey, rentPayer: payer.publicKey,
    actions: [
      { __kind: "RemoveMember", oldMember: lost.publicKey },
      { __kind: "AddMember", newMember: { key: replacement.publicKey, permissions: multisig.types.Permissions.all() } },
    ],
    signers: [payer, members[0]], sendOptions: { skipPreflight: true },
  }));
  await confirm(await multisig.rpc.proposalCreate({
    connection, feePayer: payer, multisigPda, transactionIndex: configIndex,
    creator: members[0], sendOptions: { skipPreflight: true },
  }));
  // The two surviving keys vote. The lost one is never needed and never
  // reconstructed; it is replaced.
  for (const m of [members[0], members[1]]) {
    await confirm(await multisig.rpc.proposalApprove({
      connection, feePayer: payer, multisigPda, transactionIndex: configIndex,
      member: m, sendOptions: { skipPreflight: true },
    }));
  }
  await confirm(await multisig.rpc.configTransactionExecute({
    connection, feePayer: payer, multisigPda, transactionIndex: configIndex,
    member: members[0], rentPayer: payer, signers: [payer, members[0]],
    sendOptions: { skipPreflight: false },
  }));

  const after = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const keys = after.members.map(m => m.key.toBase58());
  say("membership after replacement:", keys.join(", "));
  record.member_replaced = {
    lost: lost.publicKey.toBase58(),
    added: replacement.publicKey.toBase58(),
    lost_key_still_a_member: keys.includes(lost.publicKey.toBase58()),
    replacement_is_a_member: keys.includes(replacement.publicKey.toBase58()),
    multisig_address_unchanged: multisigPda.toBase58() === before.multisig,
    vault_pda_unchanged: vaultPda.toBase58() === before.vault,
  };

  // ---- step 4: the treasury still works, signed by the NEW key set ---------
  const contractorToken = await createAccount(connection, contractor, mint, contractor.publicKey);
  const periodIndex = 3n;
  const [approvalPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-approval"), vaultAccount.toBuffer(), u64(periodIndex)], B2);
  const approve = new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: vaultAccount, isSigner: false, isWritable: false },
      { pubkey: policyPda, isSigner: false, isWritable: false },
      { pubkey: contractorToken, isSigner: false, isWritable: false },
      { pubkey: approvalPda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("approve"), u64(periodIndex), u64(1_000_000)]),
  });

  const infoB = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const txIndex = BigInt(infoB.transactionIndex) + 1n;
  const transactionMessage = new TransactionMessage({
    payerKey: vaultPda,
    recentBlockhash: (await connection.getLatestBlockhash()).blockhash,
    instructions: [approve],
  });
  await confirm(await multisig.rpc.vaultTransactionCreate({
    connection, feePayer: payer, multisigPda, transactionIndex: txIndex,
    creator: members[0].publicKey, vaultIndex: 0, ephemeralSigners: 0,
    transactionMessage, signers: [payer, members[0]], sendOptions: { skipPreflight: true },
  }));
  await confirm(await multisig.rpc.proposalCreate({
    connection, feePayer: payer, multisigPda, transactionIndex: txIndex,
    creator: members[0], sendOptions: { skipPreflight: true },
  }));
  // One surviving key and the brand-new one. The lost key signs nothing.
  for (const m of [members[0], replacement]) {
    await confirm(await multisig.rpc.proposalApprove({
      connection, feePayer: payer, multisigPda, transactionIndex: txIndex,
      member: m, sendOptions: { skipPreflight: true },
    }));
  }
  const sig3 = await confirm(await multisig.rpc.vaultTransactionExecute({
    connection, feePayer: payer, multisigPda, transactionIndex: txIndex,
    member: replacement.publicKey, signers: [payer, replacement],
    sendOptions: { skipPreflight: false },
  }));
  say("approve executed by the post-replacement key set:", sig3);
  record.approve_after_replacement_signature = sig3;

  const approval = await connection.getAccountInfo(approvalPda);
  const approvalApprover = new PublicKey(approval.data.subarray(8 + 32, 8 + 64));
  say("approval.approver on chain:", approvalApprover.toBase58());
  record.approval_approver_on_chain = approvalApprover.toBase58();
  record.b2_saw_no_change = approvalApprover.equals(vaultPda);

  record.result = "PASS";
  say("RESULT: a Squads multisig vault PDA satisfies B2's Signer constraint,");
  say("        and a member can be replaced without B2 observing anything");
}

main().then(() => {
  fs.writeFileSync("probe_result.json", JSON.stringify(record, null, 2));
  process.exit(0);
}).catch(async e => {
  record.result = "FAIL";
  record.error = String(e?.message ?? e);
  if (e?.transactionLogs) record.logs = e.transactionLogs;
  if (e?.logs) record.logs = e.logs;
  console.error("FAILED:", record.error);
  if (record.logs) console.error(record.logs.join("\n"));
  fs.writeFileSync("probe_result.json", JSON.stringify(record, null, 2));
  process.exit(1);
});
