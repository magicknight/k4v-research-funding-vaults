"""Adversarial RPC transport/discovery tests; fixtures are local E-07 bytes."""
import base64
import copy
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

from beneficiary_vault_verifier import _base58_encode
from e07_verifier import expand_bundle
from launch_v5_rpc_exporter import Client, MAX_RESPONSE, export, main, pda, read_accounts

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from e08_recorded_rpc import GENESIS, RecordedRpc, manifest_for, serve  # noqa: E402


class V5RpcTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.snapshots = expand_bundle(json.loads((ROOT / "examples/e07_rehearsal_bundle.json").read_text()))
        cls.snapshot = cls.snapshots[-1]

    def setUp(self):
        self.rpc = RecordedRpc(copy.deepcopy(self.snapshot))
        self.manifest = manifest_for(self.snapshot)

    def mutate(self, name, offset, data):
        a = self.rpc.bank[self.snapshot["accounts"][name]["address"]]
        raw = bytearray.fromhex(a["data_hex"])
        raw[offset:offset + len(data)] = data
        a["data_hex"] = raw.hex()

    def hooked(self, callback):
        original = self.rpc.call
        def call(method, params):
            result = original(method, params)
            return callback(len(self.rpc.calls), method, params, result)
        self.rpc.call = call

    def test_complete_history_and_unused_future_approvals_in_single_final_response(self):
        self.rpc = RecordedRpc(self.snapshots[1])
        r = export(self.rpc, manifest_for(self.snapshots[1]), 1)
        self.assertTrue(r["verification"]["valid"])
        self.assertEqual(r["verification"]["amount_ceilings_before_transaction_signatures"], ["0", "0"])
        self.assertEqual([m for m, _ in self.rpc.calls], ["getGenesisHash"] + ["getMultipleAccounts"] * 3 + ["getGenesisHash"])
        final = self.rpc.calls[3][1]
        self.assertEqual(set(final[0]), {a["address"] for a in r["snapshot"]["accounts"].values()})
        self.assertTrue({"withdrawal_0_1", "withdrawal_1_1", "approval_6", "approval_9", "approval_13"} <= set(r["snapshot"]["accounts"]))
        self.assertEqual(final[1], {"encoding": "base64", "commitment": "finalized", "minContextSlot": int(self.snapshots[1]["slot"])})
        self.assertFalse(r["provenance"]["discovery_bytes_used_in_snapshot"])
        self.assertFalse(r["provenance"]["chain_proof_verified"])

    def test_actual_loopback_http_preserves_all_final_bytes(self):
        with serve(self.rpc) as endpoint:
            r = export(Client(endpoint), self.manifest)
        for name, value in r["snapshot"]["accounts"].items():
            self.assertEqual(value, {k: self.snapshot["accounts"][name][k] for k in value})
        self.assertEqual(r["verification"]["withdrawal"]["proposals_verified"], "4")
        self.assertFalse(r["verification"]["production_ready"])

    def test_wrong_genesis_stops_before_accounts(self):
        self.manifest["expected"]["genesis_hash"] = self.snapshot["program_id"]
        with self.assertRaisesRegex(ValueError, "RPC_GENESIS_MISMATCH"):
            export(self.rpc, self.manifest)
        self.assertEqual(len(self.rpc.calls), 1)

    def test_genesis_change_after_accounts_rejects(self):
        self.hooked(lambda n, m, p, r: self.snapshot["program_id"] if n == 5 else r)
        with self.assertRaisesRegex(ValueError, "RPC_GENESIS_CHANGED"):
            export(self.rpc, self.manifest)

    def test_explicit_initial_identity_mismatch_rejects(self):
        for field in ("mint", "creator", "founder", "treasury", "initial_oracle", "spec_sha256"):
            self.setUp()
            self.manifest["expected"][field] = "ab" * 32 if field.endswith("sha256") else GENESIS
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "RPC_POLICY_IDENTITY_MISMATCH"):
                export(self.rpc, self.manifest)
            self.assertEqual(len(self.rpc.calls), 2)

    def test_wrong_program_identity_hash_and_policy_reject_before_network(self):
        for field, value in (("program_id", GENESIS), ("policy", GENESIS),
                             ("identity_sha256", "aa" * 32), ("spec_sha256", "XX" * 32)):
            self.setUp()
            self.manifest["expected"][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                export(self.rpc, self.manifest)
            self.assertEqual(self.rpc.calls, [])

    def test_manifest_shape_aliases_types_and_unknown_fields_reject(self):
        variants = []
        for field in ("schema", "expected", "external_accounts", "approval_periods"):
            m = copy.deepcopy(self.manifest)
            del m[field]
            variants.append(m)
        m = copy.deepcopy(self.manifest); m["extra"] = "ignored?"; variants.append(m)
        m = copy.deepcopy(self.manifest); m["external_accounts"]["source"] = m["expected"]["mint"]; variants.append(m)
        m = copy.deepcopy(self.manifest); m["external_accounts"]["founder_destination_9"] = GENESIS; variants.append(m)
        for periods in ([6, True], [6, 6, 13], [13, 9, 6], [6, -1], [2**64], ["6"], None):
            m = copy.deepcopy(self.manifest); m["approval_periods"] = periods; variants.append(m)
        for m in variants:
            with self.subTest(manifest=m), self.assertRaises((ValueError, TypeError)):
                export(self.rpc, m)
        self.assertEqual(self.rpc.calls, [])

    def test_omitted_unused_future_approval_rejects_count(self):
        self.manifest["approval_periods"].remove(13)
        with self.assertRaisesRegex(ValueError, "RPC_COMPLETE_APPROVAL_PERIODS_REQUIRED"):
            export(self.rpc, self.manifest)

    def test_same_count_wrong_period_cannot_replace_canonical_approval(self):
        self.manifest["approval_periods"][-1] = 14
        with self.assertRaisesRegex(ValueError, "RPC_MISSING_ACCOUNT"):
            export(self.rpc, self.manifest)

    def test_missing_each_history_or_required_key_rejects(self):
        names = [n for n in self.snapshot["accounts"] if n.startswith(("withdrawal_", "approval_"))]
        baseline = export(RecordedRpc(self.snapshot), self.manifest)
        names += [n for n in baseline["snapshot"]["accounts"] if n.startswith("key_")]
        for name in names:
            self.setUp()
            del self.rpc.bank[self.snapshot["accounts"][name]["address"]]
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "RPC_MISSING_ACCOUNT"):
                export(self.rpc, self.manifest)

    def test_policy_advances_during_discovery_requires_restart(self):
        def change(n, method, params, result):
            if n == 2:
                self.mutate("policy", 985, struct.pack("<Q", 4))
            return result
        self.hooked(change)
        with self.assertRaisesRegex(ValueError, "RPC_DISCOVERY_CHANGED_RESTART_REQUIRED"):
            export(self.rpc, self.manifest)

    def test_final_bank_can_advance_without_reusing_earlier_clock(self):
        start = int(self.snapshot["slot"])
        def change(n, method, params, result):
            if method == "getMultipleAccounts":
                slot = start + n
                result["context"]["slot"] = slot
                index = params[0].index(self.snapshot["accounts"]["clock"]["address"])
                raw = bytearray(base64.b64decode(result["value"][index]["data"][0]))
                struct.pack_into("<Q", raw, 0, slot)
                result["value"][index]["data"][0] = base64.b64encode(raw).decode()
            return result
        self.hooked(change)
        r = export(self.rpc, self.manifest, start)
        self.assertEqual(r["provenance"]["discovery_slots"], [str(start + 2), str(start + 3)])
        self.assertEqual(r["snapshot"]["slot"], str(start + 4))
        self.assertEqual(self.rpc.calls[3][1][1]["minContextSlot"], start + 3)

    def test_generic_change_tombstone_included_and_missing_history_rejected(self):
        # Synthetic cancelled generic proposal exercises the v5 discovery path;
        # it is not an additional signed-runtime or chain-authenticity receipt.
        from beneficiary_vault_verifier import _pubkey, find_program_address
        from launch_v5_verifier import read_policy
        p = read_policy(self.snapshot)
        key = _pubkey(p["address"])
        address, bump = find_program_address((b"launch-v5-change", key, struct.pack("<Q", 1)), _pubkey(self.snapshot["program_id"]))
        raw = hashlib.sha256(b"account:ChangeProposalV5").digest()[:8] + key
        raw += struct.pack("<QBB", 1, 0, 0) + hashlib.sha256(b"synthetic-generic-successor").digest()
        created = p["config"]["t0"]
        raw += struct.pack("<QQqqBB", 0, 0, created, created + 7_776_000, 2, bump)
        self.mutate("policy", 929, struct.pack("<Q", 1))
        name = _base58_encode(address)
        self.rpc.bank[name] = {"address": name, "owner": self.snapshot["program_id"],
                              "executable": False, "lamports": "1", "data_hex": raw.hex()}
        result = export(self.rpc, self.manifest)
        self.assertEqual(result["snapshot"]["accounts"]["change_1"]["data_hex"], raw.hex())
        del self.rpc.bank[name]
        with self.assertRaisesRegex(ValueError, "RPC_MISSING_ACCOUNT"):
            export(self.rpc, self.manifest)

    def test_policy_and_history_changes_in_final_bank_require_restart(self):
        for name, offset, data in (("policy", 1057, struct.pack("<Q", 4)),
                                  ("withdrawal_0_3", 146, b"\x00"), ("approval_13", 120, struct.pack("<Q", 1))):
            self.setUp()
            def change(n, method, params, result):
                if n == 3:
                    self.mutate(name, offset, data)
                return result
            self.hooked(change)
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "RPC_DISCOVERY_CHANGED_RESTART_REQUIRED"):
                export(self.rpc, self.manifest)

    def test_final_response_bytes_used_even_when_lamports_change(self):
        def change(n, method, params, result):
            if n == 3:
                a = self.rpc.bank[self.snapshot["accounts"]["policy"]["address"]]
                a["lamports"] = str(int(a["lamports"]) + 100)
            return result
        self.hooked(change)
        r = export(self.rpc, self.manifest)
        self.assertEqual(int(r["snapshot"]["accounts"]["policy"]["lamports"]), int(self.snapshot["accounts"]["policy"]["lamports"]) + 100)

    def test_final_program_bytes_upgrade_authority_and_custody_tampering_reject(self):
        for name, offset, data, reason in (("program_data", 45, b"\x00", "PINNED_CODE_OR_PADDING"),
                ("program_data", 12, b"\x01", "TEST_CANDIDATE_MUST_BE_IMMUTABLE"),
                ("founder_token", 64, struct.pack("<Q", 0), "CUSTODY_DEFICIT")):
            self.setUp()
            self.mutate(name, offset, data)
            with self.subTest(name=name, offset=offset), self.assertRaisesRegex(ValueError, reason):
                export(self.rpc, self.manifest)

    def test_consistently_wrong_history_epoch_and_key_masks_still_reject(self):
        key_name = next(n for n in self.snapshot["accounts"] if n.startswith("key_") and bytes.fromhex(self.snapshot["accounts"][n]["data_hex"])[72])
        for name, offset, data, reason in (("withdrawal_0_1", 49, struct.pack("<Q", 7), "WITHDRAWAL_EPOCH_HISTORY"),
                                         (key_name, 72, b"\x00", "KEY_HISTORY_MASK")):
            self.setUp(); self.mutate(name, offset, data)
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, reason):
                export(self.rpc, self.manifest)

    def test_over_limit_history_never_chunked(self):
        self.mutate("policy", 985, struct.pack("<Q", 100))
        with self.assertRaisesRegex(ValueError, "RPC_COMPLETE_GRAPH_EXCEEDS_100"):
            export(self.rpc, self.manifest)
        self.assertEqual(len(self.rpc.calls), 2)

    def test_key_indexes_are_included_in_account_limit(self):
        # Synthetic untrusted discovery supplies 44 successors: core+history fits,
        # but history+keys does not. No final or chunked response is requested.
        self.mutate("policy", 985, struct.pack("<Q", 44))
        policy = self.manifest["expected"]["policy"]
        from beneficiary_vault_verifier import _pubkey
        for nonce in range(1, 45):
            a = copy.deepcopy(self.snapshot["accounts"]["withdrawal_0_1"])
            raw = bytearray.fromhex(a["data_hex"])
            raw[89:121] = hashlib.sha256(str(nonce).encode()).digest()
            address = pda(b"launch-v5-withdraw", _pubkey(policy), b"\x00", struct.pack("<Q", nonce))
            a.update(address=address, data_hex=raw.hex())
            self.rpc.bank[address] = a
        with self.assertRaisesRegex(ValueError, "RPC_COMPLETE_GRAPH_EXCEEDS_100"):
            export(self.rpc, self.manifest)
        self.assertEqual(len(self.rpc.calls), 3)

    def test_read_request_100_boundary_and_101_rejection(self):
        clock = self.snapshot["accounts"]["clock"]
        addresses = {"clock": clock["address"]}
        for i in range(99):
            address = _base58_encode(hashlib.sha256(f"transport-only-{i}".encode()).digest())
            addresses[str(i)] = address
            self.rpc.bank[address] = self.snapshot["accounts"]["source"]
        r = read_accounts(self.rpc, addresses, 0)
        self.assertEqual(len(r["accounts"]), 100)
        addresses["overflow"] = self.snapshot["program_id"]
        with self.assertRaisesRegex(ValueError, "RPC_COMPLETE_GRAPH_EXCEEDS_100"):
            read_accounts(self.rpc, addresses, 0)
        self.assertEqual(len(self.rpc.calls), 1)

    def test_context_minimum_clock_slot_and_owner_mismatch_reject(self):
        for mode in ("bool", "string", "older", "future", "owner", "short"):
            self.setUp()
            def change(n, method, params, result):
                if method == "getMultipleAccounts":
                    slot = result["context"]["slot"]
                    if mode in ("bool", "string", "older", "future"):
                        result["context"]["slot"] = {"bool": True, "string": str(slot), "older": slot - 1, "future": slot + 1}[mode]
                    elif mode == "owner": result["value"][1]["owner"] = GENESIS
                    else: result["value"][1]["data"] = ["AAAA", "base64"]
                return result
            self.hooked(change)
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                export(self.rpc, self.manifest, int(self.snapshot["slot"]))

    def test_missing_count_order_encoding_and_account_envelopes_reject(self):
        variants = [("owner", "bad"), ("executable", 1), ("lamports", True), ("lamports", 0), ("lamports", 2**64),
                    ("data", ["@@", "base64"]), ("data", ["AB==", "base64"]), ("data", ["AAAA", "jsonParsed"])]
        for field, value in variants + [("mode", "null"), ("mode", "count"), ("mode", "reverse")]:
            self.setUp()
            def change(n, method, params, result):
                if method == "getMultipleAccounts":
                    if field != "mode": result["value"][0][field] = value
                    elif value == "null": result["value"][0] = None
                    elif value == "count": result["value"].pop()
                    else: result["value"].reverse()
                return result
            self.hooked(change)
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                export(self.rpc, self.manifest)

    def test_invalid_min_slot_rejects_before_network(self):
        for slot in (True, -1, 2**64, "0"):
            with self.subTest(slot=slot), self.assertRaisesRegex(ValueError, "RPC_MIN_SLOT"):
                export(self.rpc, self.manifest, slot)
        self.assertEqual(self.rpc.calls, [])

    def test_transport_allowlist_url_secrets_and_redirect_refusal(self):
        for url in ("http://example.invalid", "file:///tmp/rpc", "https://user:secret@example.invalid", "https://example.invalid/#secret", "https://example.invalid:bad", "https://example.invalid/\nsecret"):
            with self.subTest(url=url), self.assertRaisesRegex(ValueError, "RPC_ENDPOINT_OR_TRANSPORT"):
                Client(url)
        with serve(self.rpc, redirect=True) as endpoint:
            with self.assertRaisesRegex(ValueError, "RPC_REQUEST_FAILED"):
                Client(endpoint).call("getGenesisHash", [])
        self.assertEqual(self.rpc.calls, [])

    def test_write_and_simulation_methods_never_sent(self):
        client = Client("https://example.invalid/secret")
        with patch.object(client.opener, "open") as opened:
            for method in ("sendTransaction", "requestAirdrop", "simulateTransaction", "getProgramAccounts"):
                with self.subTest(method=method), self.assertRaisesRegex(ValueError, "READ_ONLY_RPC"):
                    client.call(method, [])
            opened.assert_not_called()

    def test_rpc_json_ids_errors_duplicate_keys_nonfinite_and_size_fail_closed(self):
        bodies = [json.dumps(x).encode() for x in (
            {"jsonrpc": "2.0", "id": 2, "result": GENESIS},
            {"jsonrpc": "2.0", "id": True, "result": GENESIS},
            {"jsonrpc": "2.0", "id": 1, "error": {"message": "secret"}},
        )] + [b'{"jsonrpc":"2.0","id":1,"id":1,"result":0}', b'{"id":NaN}', b"\xff", b"a" * (MAX_RESPONSE + 1)]
        for body in bodies:
            client = Client("https://example.invalid/secret")
            with patch.object(client.opener, "open", return_value=io.BytesIO(body)):
                with self.assertRaises(ValueError) as ctx:
                    client.call("getGenesisHash", [])
                self.assertNotIn("secret", str(ctx.exception))

    def test_request_method_params_id_and_timeout(self):
        client = Client("https://example.invalid")
        response = io.BytesIO(json.dumps({"jsonrpc": "2.0", "id": 1, "result": GENESIS}).encode())
        with patch.object(client.opener, "open", return_value=response) as opened:
            self.assertEqual(client.call("getGenesisHash", []), GENESIS)
        request = opened.call_args.args[0]
        self.assertEqual(json.loads(request.data), {"jsonrpc": "2.0", "id": 1, "method": "getGenesisHash", "params": []})
        self.assertEqual(opened.call_args.kwargs["timeout"], 15)
        self.assertEqual(request.get_method(), "POST")

    def test_cli_failure_writes_nothing_and_success_never_overwrites_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps(self.manifest))
            output = root / "observation.json"
            args = ["exporter", "--rpc-url", "https://example.invalid/secret", "--manifest", str(manifest), "--output", str(output)]
            with patch("sys.argv", args), patch("launch_v5_rpc_exporter.Client", return_value=self.rpc), patch("sys.stdout", new_callable=io.StringIO):
                self.assertEqual(main(), 0)
                before = output.read_bytes()
                self.assertEqual(main(), 1)
                self.assertEqual(output.read_bytes(), before)
                output.unlink()
                self.manifest["approval_periods"].pop()
                manifest.write_text(json.dumps(self.manifest))
                self.assertEqual(main(), 1)
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
