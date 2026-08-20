// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The B2 devnet run: a Squads 2-of-3 multisig driving the purpose vault on a
// public cluster, and four refusals sent deliberately so they land on chain
// with signatures anyone can look up.
//
// Keys are read from ~/.config/k4v/devnet, outside every repository, mode 0600.
// Nothing here contains key material. Ephemeral keys are written to a run
// directory and reused, so a crash is recoverable rather than terminal — the
// first attempt stranded ~0.27 SOL by generating them in memory, over-funding
// them tenfold and then throwing.
//
// Every step is resumable: B2's accounts are PDAs, so a rerun skips whatever is
// already on chain instead of failing with "already in use".
//
// Two things worth knowing before reading this:
//   - a refusal is only evidence if it is looked-up-able, so refusals are sent
//     with skipPreflight and land as failed transactions rather than being
//     rejected locally with no signature at all;
//   - @sqds/multisig 2.1.4's rpc.proposalCreate accepts a rentPayer, adds it to
//     the signer list, and never passes it to the instruction builder, so the
//     rent is charged to the creator regardless. The instruction is built by
//     hand here, where rentPayer is honoured.
//
// A public cluster has no time control, so the 730-day cliff, the 30-day
// notice, the 90-day rotation notice and the silence grace cannot complete.
// What they produce here are refusals, which is the honest half.

import {
  Connection, Keypair, PublicKey, SystemProgram, LAMPORTS_PER_SOL,
  TransactionMessage, TransactionInstruction, Transaction, sendAndConfirmTransaction,
} from "@solana/web3.js";
import * as multisig from "@sqds/multisig";
import { createMint, mintTo, getOrCreateAssociatedTokenAccount, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { createHash, randomBytes } from "crypto";
import fs from "fs";
import os from "os";

const RPC = "https://api.devnet.solana.com";
const B2 = new PublicKey("2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w");
const TREASURY = new PublicKey("HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK");
const connection = new Connection(RPC, "confirmed");
const KEYDIR = os.homedir() + "/.config/k4v/devnet";
const load = n => Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(`${KEYDIR}/${n}.json`))));

const rec = { cluster: "devnet", rpc: RPC, successes: [], refusals: [] };
const say = (...a) => console.log(a.map(String).join(" "));

const disc = n => createHash("sha256").update("global:" + n).digest().subarray(0, 8);
const u16 = v => { const b = Buffer.alloc(2); b.writeUInt16LE(v); return b; };
const u64 = v => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v)); return b; };
const i64 = v => { const b = Buffer.alloc(8); b.writeBigInt64LE(BigInt(v)); return b; };

const payer = load("payer");
const beneficiary = load("beneficiary");

// B2-D-3 repair. A key generated in process and funded is money you lose the
// moment anything throws. These are written to disk outside every repository,
// mode 0600, so a crash is recoverable instead of terminal — and reused on a
// rerun rather than replaced, which is what stranded the first attempt's SOL.
const RUNDIR = os.homedir() + "/.config/k4v/devnet/probe-20260820";
fs.mkdirSync(RUNDIR, { recursive: true, mode: 0o700 });
function ephemeral(name) {
  const path = `${RUNDIR}/${name}.json`;
  if (fs.existsSync(path)) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(path))));
  const kp = Keypair.generate();
  fs.writeFileSync(path, JSON.stringify(Array.from(kp.secretKey)), { mode: 0o600 });
  return kp;
}
const members = [ephemeral("member0"), ephemeral("member1"), ephemeral("member2")];
const oracle = ephemeral("oracle");
const depositor = ephemeral("depositor");
const stranger = ephemeral("stranger");
const contractor = ephemeral("contractor");

const BENEFICIARY_DEPOSIT = 300_000_000, PURPOSE_DEPOSIT = 500_000_000;
const ANNUAL_BPS = 500, MARKET_BPS = 250, MAX_AGE = 3 * 24 * 3600;
const HARD_CEILING = 2_500_000n, ELIGIBLE_VOLUME = 120_000_000;
const CLIFF = 730 * 24 * 3600;

// A rerun must derive the same policy PDA, or it silently starts over and
// abandons whatever the last attempt paid rent for.
const HASHFILE = os.homedir() + "/.config/k4v/devnet/probe-20260820/policy_hash.hex";
if (!fs.existsSync(HASHFILE)) fs.writeFileSync(HASHFILE, randomBytes(32).toString("hex"), { mode: 0o600 });
const POLICY_HASH = Buffer.from(fs.readFileSync(HASHFILE, "utf8").trim(), "hex");
const [policyPda] = PublicKey.findProgramAddressSync([Buffer.from("purpose-policy"), POLICY_HASH], B2);
const [marketPda] = PublicKey.findProgramAddressSync([Buffer.from("purpose-market"), POLICY_HASH], B2);
const createKey = ephemeral("createkey");
const [multisigPda] = multisig.getMultisigPda({ createKey: createKey.publicKey });
const [vaultPda] = multisig.getVaultPda({ multisigPda, index: 0 });

const sleep = ms => new Promise(r => setTimeout(r, ms));
// The public devnet endpoint rate-limits hard, and a confirmed signature does
// not mean the account it wrote is readable from the next request yet. These
// are propagation waits, not transaction retries: nothing is ever re-sent.
// confirmTransaction resolves for a transaction that landed and *failed*; it
// only rejects when the confirmation itself fails. Swallowing that turned a
// plain "this transaction errored" into a mystifying "the account it should
// have written never became readable". Surface it here instead.
const confirm = async s => {
  const res = await connection.confirmTransaction(s, "confirmed");
  if (res?.value?.err) {
    const tx = await connection.getTransaction(s, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
    throw new Error(`transaction ${s} failed on chain: ${JSON.stringify(res.value.err)}\n` +
                    (tx?.meta?.logMessages ?? []).join("\n"));
  }
  await sleep(600);
  return s;
};

// @sqds/multisig 2.1.4 rpc.proposalCreate accepts rentPayer, adds it to the
// signer list, and then never passes it to the instruction builder — so the
// account rent is charged to the creator regardless. Build the instruction
// directly, where rentPayer is honoured.
async function createProposal(transactionIndex, creator) {
  const ix = multisig.instructions.proposalCreate({
    multisigPda, creator: creator.publicKey, rentPayer: payer.publicKey, transactionIndex,
  });
  const tx = new Transaction().add(ix);
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  tx.sign(payer, creator);
  return await confirm(await connection.sendRawTransaction(tx.serialize(), { skipPreflight: true }));
}

async function waitForAccount(pubkey, label) {
  for (let i = 0; i < 40; i++) {
    const info = await connection.getAccountInfo(pubkey, "confirmed");
    if (info) return info;
    await sleep(1000);
  }
  throw new Error(`${label}: account ${pubkey.toBase58()} never became readable`);
}

async function transfer(to, sol) {
  const tx = new Transaction().add(SystemProgram.transfer({
    fromPubkey: payer.publicKey, toPubkey: to, lamports: Math.round(sol * LAMPORTS_PER_SOL) }));
  return await sendAndConfirmTransaction(connection, tx, [payer], { commitment: "confirmed" });
}

async function direct(ix, signers, label) {
  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(connection, tx, signers, { commitment: "confirmed" });
  say(label, "->", sig);
  rec.successes.push({ step: label, signature: sig });
  return sig;
}

// A refusal is only evidence if anyone can look it up. Preflight would reject
// these locally and produce no signature at all, so they are sent with
// skipPreflight and land on chain as failed transactions with real signatures.
async function expectRefusal(ix, signers, label, expected) {
  const tx = new Transaction().add(ix);
  tx.feePayer = signers[0].publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  tx.sign(...signers);
  const sig = await connection.sendRawTransaction(tx.serialize(), { skipPreflight: true });
  await connection.confirmTransaction(sig, "confirmed").catch(() => {});
  let meta = null;
  for (let i = 0; i < 20 && !meta; i++) {
    meta = await connection.getTransaction(sig, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
    if (!meta) await new Promise(r => setTimeout(r, 1500));
  }
  if (!meta) throw new Error(`${label}: transaction ${sig} never appeared`);
  const logs = (meta.meta?.logMessages ?? []).join("\n");
  if (!meta.meta?.err) throw new Error(`${label}: expected ${expected} but the transaction succeeded: ${sig}`);
  if (!logs.includes(expected)) throw new Error(`${label}: expected ${expected}, got:\n${logs}`);
  say(label, "-> refused on chain with", expected, sig);
  rec.refusals.push({ step: label, error: expected, signature: sig, slot: meta.slot });
  return sig;
}

async function throughMultisig(instructions, label, extraSigners = [], approvers = null) {
  const info = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const transactionIndex = BigInt(info.transactionIndex) + 1n;
  const transactionMessage = new TransactionMessage({
    payerKey: vaultPda,
    recentBlockhash: (await connection.getLatestBlockhash()).blockhash,
    instructions,
  });
  const voters = approvers ?? [members[0], members[1]];
  await confirm(await multisig.rpc.vaultTransactionCreate({
    connection, feePayer: payer, multisigPda, transactionIndex,
    creator: voters[0].publicKey, rentPayer: payer.publicKey,
    vaultIndex: 0, ephemeralSigners: 0,
    transactionMessage, signers: [payer, voters[0]], sendOptions: { skipPreflight: true },
  }));
  await waitForAccount(multisig.getTransactionPda({ multisigPda, index: transactionIndex })[0], label);
  await createProposal(transactionIndex, voters[0]);
  await waitForAccount(multisig.getProposalPda({ multisigPda, transactionIndex })[0], label);
  for (const m of voters) {
    await confirm(await multisig.rpc.proposalApprove({
      connection, feePayer: payer, multisigPda, transactionIndex,
      member: m, sendOptions: { skipPreflight: true },
    }));
  }
  const sig = await confirm(await multisig.rpc.vaultTransactionExecute({
    connection, feePayer: payer, multisigPda, transactionIndex,
    member: voters[0].publicKey, signers: [payer, voters[0], ...extraSigners],
    sendOptions: { skipPreflight: false },
  }));
  say(label, "->", sig);
  rec.successes.push({ step: label, signature: sig, through: "squads 2-of-3" });
  return sig;
}

// The B2 accounts are PDAs, so a rerun would hit "already in use" rather than
// redoing work. Skip what already exists instead.
async function unless(pubkey, label, fn) {
  if (await connection.getAccountInfo(pubkey, "confirmed")) {
    say(label, "-> already on chain, skipped");
    rec.successes.push({ step: label, note: "already on chain from an earlier attempt" });
    return;
  }
  return await fn();
}

function vaultPdas(kindByte, authority, mint) {
  const [v] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-vault"), POLICY_HASH, Buffer.from([kindByte]), authority.toBuffer(), mint.toBuffer()], B2);
  const [t] = PublicKey.findProgramAddressSync([Buffer.from("purpose-token"), v.toBuffer()], B2);
  return [v, t];
}

async function main() {
  const start = await connection.getBalance(payer.publicKey);
  rec.payer = payer.publicKey.toBase58();
  rec.payer_start_lamports = start;
  say("payer", rec.payer, start / LAMPORTS_PER_SOL, "SOL");

  // Measured, not guessed. The depositor pays rent for two vault state
  // accounts and two token accounts (~0.0086) plus fees; the vault PDA pays for
  // the policy, the market and two approvals (~0.0077) plus fees.
  await transfer(depositor.publicKey, 0.02);
  await transfer(beneficiary.publicKey, 0.005);
  await transfer(vaultPda, 0.03);
  say("funded depositor, beneficiary and the multisig vault PDA (minimum, keys on disk)");

  const existing = await connection.getAccountInfo(multisigPda, "confirmed");
  if (existing) { say("multisig already exists, reusing it"); } else
  await confirm(await multisig.rpc.multisigCreateV2({
    connection, treasury: TREASURY, createKey, creator: payer, multisigPda,
    configAuthority: null, threshold: 2, timeLock: 0, rentCollector: null,
    members: members.map(m => ({ key: m.publicKey, permissions: multisig.types.Permissions.all() })),
    sendOptions: { skipPreflight: true },
  }));
  await waitForAccount(multisigPda, "multisig");
  const ms = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  Object.assign(rec, {
    multisig: multisigPda.toBase58(), vault_pda: vaultPda.toBase58(),
    threshold: `${ms.threshold} of ${ms.members.length}`,
    policy_hash: POLICY_HASH.toString("hex"),
    policy_pda: policyPda.toBase58(), market_pda: marketPda.toBase58(),
    oracle: oracle.publicKey.toBase58(),
  });
  say("multisig", rec.multisig, rec.threshold, "vault", rec.vault_pda);

  const mintKp = ephemeral("mint");
  const mint = mintKp.publicKey;
  if (!(await connection.getAccountInfo(mint, "confirmed")))
    await createMint(connection, payer, payer.publicKey, null, 9, mintKp);
  rec.mint = mint.toBase58();
  // Associated accounts, so a rerun reuses them instead of paying rent twice.
  const ata = async owner =>
    (await getOrCreateAssociatedTokenAccount(connection, payer, mint, owner, true)).address;
  const depositorToken = await ata(depositor.publicKey);
  const beneficiaryToken = await ata(beneficiary.publicKey);
  const contractorToken = await ata(contractor.publicKey);
  const held = (await connection.getTokenAccountBalance(depositorToken)).value.amount;
  if (BigInt(held) < BigInt(BENEFICIARY_DEPOSIT + PURPOSE_DEPOSIT))
    await mintTo(connection, payer, mint, depositorToken, payer,
                 BENEFICIARY_DEPOSIT + PURPOSE_DEPOSIT - Number(held));
  say("mint", rec.mint);

  // 1. open_policy, authority = the multisig vault PDA
  await unless(policyPda, "open_policy (authority = multisig vault PDA)", () =>
   throughMultisig([new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: true },
      { pubkey: oracle.publicKey, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: marketPda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("open_policy"), POLICY_HASH, u16(MARKET_BPS), i64(MAX_AGE),
                         u64(HARD_CEILING), u64(0), i64(0)]),
  })], "open_policy (authority = multisig vault PDA)"));

  // 2. report_volume by the frozen oracle, and a stranger refused
  await expectRefusal(new TransactionInstruction({
    programId: B2,
    keys: [{ pubkey: stranger.publicKey, isSigner: true, isWritable: false },
           { pubkey: marketPda, isSigner: false, isWritable: true }],
    data: Buffer.concat([disc("report_volume"), u64(999_999_999)]),
  }), [payer, stranger], "a stranger reports volume", "ConstraintHasOne");

  await direct(new TransactionInstruction({
    programId: B2,
    keys: [{ pubkey: oracle.publicKey, isSigner: true, isWritable: false },
           { pubkey: marketPda, isSigner: false, isWritable: true }],
    data: Buffer.concat([disc("report_volume"), u64(ELIGIBLE_VOLUME)]),
  }), [payer, oracle], "report_volume by the frozen oracle");

  // 3. two deposits, both co-signed by the multisig as policy authority
  const [benVault, benToken] = vaultPdas(0, beneficiary.publicKey, mint);
  const [purVault, purToken] = vaultPdas(1, vaultPda, mint);
  const depositIx = (kind, authority, vault, token, amount, cliff) => new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: depositor.publicKey, isSigner: true, isWritable: true },
      { pubkey: vaultPda, isSigner: true, isWritable: false },
      { pubkey: authority, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: depositorToken, isSigner: false, isWritable: true },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: vault, isSigner: false, isWritable: true },
      { pubkey: token, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("deposit"), Buffer.from([kind]), u64(amount), u16(ANNUAL_BPS), i64(cliff)]),
  });
  await unless(benVault, "deposit beneficiary vault (depositor + multisig both sign)", () =>
    throughMultisig([depositIx(0, beneficiary.publicKey, benVault, benToken, BENEFICIARY_DEPOSIT, CLIFF)],
      "deposit beneficiary vault (depositor + multisig both sign)", [depositor]));
  await unless(purVault, "deposit purpose vault (approver = the multisig)", () =>
    throughMultisig([depositIx(1, vaultPda, purVault, purToken, PURPOSE_DEPOSIT, 0)],
      "deposit purpose vault (approver = the multisig)", [depositor]));
  Object.assign(rec, {
    beneficiary_vault: benVault.toBase58(), beneficiary_vault_token: benToken.toBase58(),
    purpose_vault: purVault.toBase58(), purpose_vault_token: purToken.toBase58(),
    beneficiary_authority: beneficiary.publicKey.toBase58(),
  });

  // 4. the cliff refuses a beneficiary release, on a public cluster
  await expectRefusal(new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: beneficiary.publicKey, isSigner: true, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: benVault, isSigner: false, isWritable: true },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: marketPda, isSigner: false, isWritable: false },
      { pubkey: benToken, isSigner: false, isWritable: true },
      { pubkey: beneficiaryToken, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("release_beneficiary"), u64(1)]),
  }), [payer, beneficiary], "beneficiary release before the 730-day cliff", "CliffActive");

  // 5. approve for a future period, through the multisig
  const period = 1n;
  const [approvalPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-approval"), purVault.toBuffer(), u64(period)], B2);
  const approveIx = (idx, pda, need) => new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: purVault, isSigner: false, isWritable: false },
      { pubkey: policyPda, isSigner: false, isWritable: false },
      { pubkey: contractorToken, isSigner: false, isWritable: false },
      { pubkey: pda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("approve"), u64(idx), u64(need)]),
  });
  await unless(approvalPda, "approve a future period (approver = the multisig)", () =>
    throughMultisig([approveIx(period, approvalPda, 1_000_000)],
      "approve a future period (approver = the multisig)"));
  rec.approval_pda = approvalPda.toBase58();

  // 6. rotation refusals. Both are permissionless instructions, so they need no
  // PDA signature and land as ordinary failed transactions anyone can look up.
  // A purpose-release refusal was dropped here on purpose: it needs the vault
  // PDA to sign, which only Squads can do, so the refusal would be buried
  // inside a Squads execute rather than being its own legible receipt.
  await expectRefusal(new TransactionInstruction({
    programId: B2,
    keys: [{ pubkey: marketPda, isSigner: false, isWritable: true }],
    data: disc("execute_oracle_rotation"),
  }), [payer], "execute a rotation nobody proposed", "NoPendingRotation");

  // 7. oracle rotation: proposed, then refused before its notice
  const incoming = ephemeral("oracle-incoming");
  rec.proposed_oracle = incoming.publicKey.toBase58();
  await throughMultisig([new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: false },
      { pubkey: policyPda, isSigner: false, isWritable: false },
      { pubkey: marketPda, isSigner: false, isWritable: true },
    ],
    data: Buffer.concat([disc("propose_oracle"), incoming.publicKey.toBuffer()]),
  })], "propose_oracle (policy authority = the multisig)");

  await expectRefusal(new TransactionInstruction({
    programId: B2,
    keys: [{ pubkey: marketPda, isSigner: false, isWritable: true }],
    data: disc("execute_oracle_rotation"),
  }), [payer], "execute the rotation before its 90-day notice", "RotationNoticeActive");

  // 8. lose a member, replace it, and approve again with the new key set
  const replacement = ephemeral("member-replacement");
  const info = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const cfgIndex = BigInt(info.transactionIndex) + 1n;
  await confirm(await multisig.rpc.configTransactionCreate({
    connection, feePayer: payer, multisigPda, transactionIndex: cfgIndex,
    creator: members[0].publicKey, rentPayer: payer.publicKey,
    actions: [
      { __kind: "RemoveMember", oldMember: members[2].publicKey },
      { __kind: "AddMember", newMember: { key: replacement.publicKey, permissions: multisig.types.Permissions.all() } },
    ],
    signers: [payer, members[0]], sendOptions: { skipPreflight: true },
  }));
  await waitForAccount(multisig.getTransactionPda({ multisigPda, index: cfgIndex })[0], "config tx");
  await createProposal(cfgIndex, members[0]);
  await waitForAccount(multisig.getProposalPda({ multisigPda, transactionIndex: cfgIndex })[0], "config proposal");
  for (const m of [members[0], members[1]]) {
    await confirm(await multisig.rpc.proposalApprove({
      connection, feePayer: payer, multisigPda, transactionIndex: cfgIndex,
      member: m, sendOptions: { skipPreflight: true },
    }));
  }
  const cfgSig = await confirm(await multisig.rpc.configTransactionExecute({
    connection, feePayer: payer, multisigPda, transactionIndex: cfgIndex,
    member: members[0], rentPayer: payer, signers: [payer, members[0]],
    sendOptions: { skipPreflight: false },
  }));
  say("member replaced ->", cfgSig);
  const after = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  rec.member_replaced = {
    lost: members[2].publicKey.toBase58(),
    added: replacement.publicKey.toBase58(),
    signature: cfgSig,
    members_after: after.members.map(m => m.key.toBase58()),
    multisig_address_unchanged: true,
    vault_pda_unchanged: multisig.getVaultPda({ multisigPda, index: 0 })[0].equals(vaultPda),
  };

  const period2 = 2n;
  const [approval2] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-approval"), purVault.toBuffer(), u64(period2)], B2);
  await unless(approval2, "approve again, signed by the post-replacement key set", () =>
    throughMultisig([approveIx(period2, approval2, 500_000)],
      "approve again, signed by the post-replacement key set", [], [members[0], replacement]));
  rec.approval_pda_after_replacement = approval2.toBase58();

  const end = await connection.getBalance(payer.publicKey);
  rec.payer_end_lamports = end;
  rec.probe_spend_lamports = start - end;
  say("probe spend:", (start - end) / LAMPORTS_PER_SOL, "SOL");
  rec.result = "PASS";
}

main().then(() => {
  fs.writeFileSync("devnet_result.json", JSON.stringify(rec, null, 2));
  say("WROTE devnet_result.json");
  process.exit(0);
}).catch(e => {
  rec.result = "FAIL";
  rec.error = String(e?.message ?? e);
  console.error("FAILED:", rec.error);
  if (e?.transactionLogs) console.error(e.transactionLogs.join("\n"));
  fs.writeFileSync("devnet_result.json", JSON.stringify(rec, null, 2));
  process.exit(1);
});
