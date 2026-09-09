"""RPC wire fixtures are built from signed local-runtime output, not a chain.
Malformed envelopes, network identity and mixed-bank observations fail closed.
"""
import base64
import copy
import io
import json
from pathlib import Path
import struct
import unittest
from unittest.mock import patch

from e05_verifier import expand_bundle
from launch_v4_rpc_exporter import export, Client, MAX_RESPONSE

FIXTURE = Path(__file__).resolve().parents[1] / "examples/e05_rehearsal_bundle.json"
GENESIS = "11111111111111111111111111111111"


class RecordedClient:
    def __init__(self, snapshot):
        self.calls = []
        self.genesis = GENESIS
        clock = bytes.fromhex(snapshot["accounts"]["clock"]["data_hex"])
        self.response = {"context": {"slot": struct.unpack_from("<Q", clock)[0]}, "value": []}
        for a in snapshot["accounts"].values():
            self.response["value"].append({"owner": a["owner"], "executable": a["executable"], "lamports": 1,
                "data": [base64.b64encode(bytes.fromhex(a["data_hex"])).decode(), "base64"]})

    def call(self, method, params):
        self.calls.append((method, params))
        return self.genesis if method == "getGenesisHash" else self.response


class V4RpcTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.snapshot = expand_bundle(json.loads(FIXTURE.read_text()))[-1]
        cls.addresses = {k: v["address"] for k, v in cls.snapshot["accounts"].items()}

    def setUp(self):
        self.client = RecordedClient(self.snapshot)

    def test_all_raw_accounts_and_clock_are_read_in_one_finalized_bank(self):
        r = export(self.client, self.addresses, GENESIS, 1)
        self.assertTrue(r["verification"]["valid"])
        self.assertFalse(r["provenance"]["chain_proof_verified"])
        self.assertEqual([c[0] for c in self.client.calls], ["getGenesisHash", "getMultipleAccounts"])
        self.assertEqual(self.client.calls[1][1][1], {"encoding": "base64", "commitment": "finalized", "minContextSlot": 1})
        self.assertEqual(r["snapshot"]["now"], self.snapshot["now"])

    def test_wrong_chain_rejects_before_reading_accounts(self):
        self.client.genesis = self.snapshot["program_id"]
        with self.assertRaisesRegex(ValueError, "RPC_GENESIS_MISMATCH"): export(self.client, self.addresses, GENESIS)
        self.assertEqual(len(self.client.calls), 1)

    def test_missing_account_length_mismatch_and_alias_reject(self):
        for mode in ("missing", "count", "alias"):
            self.setUp()
            addresses = self.addresses.copy()
            if mode == "missing": self.client.response["value"][0] = None
            elif mode == "count": self.client.response["value"].pop()
            else: addresses["policy"] = addresses["mint"]
            with self.subTest(mode=mode), self.assertRaises(ValueError): export(self.client, addresses, GENESIS)

    def test_context_slot_type_minimum_and_clock_bank_mismatch_reject(self):
        original = self.client.response["context"]["slot"]
        for value in (True, str(original), original - 1, original + 1):
            self.client.response["context"]["slot"] = value
            with self.subTest(value=value), self.assertRaises(ValueError): export(self.client, self.addresses, GENESIS, original)

    def test_malformed_envelopes_and_encoding_reject(self):
        for field, value in (("owner", "bad"), ("executable", 0), ("lamports", True), ("lamports", 0),
                ("data", ["@@", "base64"]), ("data", ["AAAA", "jsonParsed"]), ("data", "AAAA")):
            self.setUp()
            self.client.response["value"][0][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError): export(self.client, self.addresses, GENESIS)

    def test_raw_program_tampering_is_not_rescued_by_valid_rpc_envelope(self):
        index = list(self.addresses).index("target_programdata")
        encoded = self.client.response["value"][index]["data"]
        raw = bytearray(base64.b64decode(encoded[0]))
        raw[45] ^= 1
        encoded[0] = base64.b64encode(raw).decode()
        with self.assertRaisesRegex(ValueError, "UNREVIEWED_PROGRAM_BYTES"): export(self.client, self.addresses, GENESIS)

    def test_http_client_emits_read_only_json_rpc_and_checks_response_id(self):
        client = Client("https://example.invalid/private-token")
        recorded = []
        def respond(request, timeout):
            recorded.append(json.loads(request.data))
            return io.BytesIO(json.dumps({"jsonrpc": "2.0", "id": 1, "result": GENESIS}).encode())
        with patch("launch_v4_rpc_exporter.urlopen", side_effect=respond):
            self.assertEqual(client.call("getGenesisHash", []), GENESIS)
        self.assertEqual(recorded, [{"jsonrpc": "2.0", "id": 1, "method": "getGenesisHash", "params": []}])
        with self.assertRaisesRegex(ValueError, "READ_ONLY_RPC"): client.call("sendTransaction", [])

    def test_rpc_error_wrong_id_bool_id_and_oversized_response_reject_without_endpoint(self):
        bodies = [json.dumps(j).encode() for j in (
            {"jsonrpc": "2.0", "id": 2, "result": GENESIS},
            {"jsonrpc": "2.0", "id": True, "result": GENESIS},
            {"jsonrpc": "2.0", "id": 1, "error": {"message": "secret"}},
        )] + [b"a" * (MAX_RESPONSE + 1), b"\xff"]
        for body in bodies:
            client = Client("https://example.invalid/private-token")
            with patch("launch_v4_rpc_exporter.urlopen", return_value=io.BytesIO(body)):
                with self.assertRaises(ValueError) as ctx: client.call("getGenesisHash", [])
                self.assertNotIn("private-token", str(ctx.exception))
                self.assertNotIn("secret", str(ctx.exception))

    def test_remote_http_is_rejected_but_explicit_loopback_is_supported(self):
        Client("http://127.0.0.1:8899")
        with self.assertRaisesRegex(ValueError, "RPC_TRANSPORT"): Client("http://example.invalid")


if __name__ == "__main__":
    unittest.main()
