#!/usr/bin/env python3
"""Serve E-07 signed local-runtime bytes over loopback HTTP, never a live chain."""
import argparse
import base64
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sys
import threading

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
from e07_verifier import expand_bundle  # noqa: E402
from launch_v5_verifier import read_policy  # noqa: E402
from launch_v5_rpc_exporter import Client, MANIFEST_SCHEMA, export  # noqa: E402

GENESIS = "11111111111111111111111111111111"  # Placeholder, not a network identity.


def manifest_for(snapshot):
    p = read_policy(snapshot)
    return {"schema": MANIFEST_SCHEMA,
            "expected": {"genesis_hash": GENESIS, "program_id": snapshot["program_id"],
                         "policy": p["address"], "identity_sha256": p["identity"].hex(),
                         "spec_sha256": p["spec_hash"].hex(),
                         **{k: p[k] for k in ("mint", "creator", "founder", "treasury", "initial_oracle")}},
            "external_accounts": {n: a["address"] for n, a in snapshot["accounts"].items()
                                  if n in ("source", "treasury_destination") or n.startswith("founder_destination_")},
            "approval_periods": sorted(int(n.removeprefix("approval_")) for n in snapshot["accounts"] if n.startswith("approval_"))}


class RecordedRpc:
    def __init__(self, snapshot):
        self.snapshot = snapshot
        self.calls = []
        self.bank = {a["address"]: a for a in snapshot["accounts"].values()}

    def call(self, method, params):
        self.calls.append((method, params))
        if method == "getGenesisHash":
            return GENESIS
        if method != "getMultipleAccounts":
            raise ValueError("FIXTURE_READ_ONLY")
        values = []
        for address in params[0]:
            a = self.bank.get(address)
            values.append(None if a is None else {"owner": a["owner"], "executable": a["executable"],
                "lamports": int(a.get("lamports", "1")),
                "data": [base64.b64encode(bytes.fromhex(a["data_hex"])).decode(), "base64"]})
        return {"context": {"slot": int(self.snapshot["slot"])}, "value": values}


@contextmanager
def serve(recorded, redirect=False):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            if redirect:
                self.send_response(307)
                self.send_header("Location", "http://example.invalid/credentials-must-not-follow")
                self.end_headers()
                return
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            result = recorded.call(request["method"], request["params"])
            data = json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


def reproduce(bundle_path):
    snapshots = expand_bundle(json.loads(bundle_path.read_text()))
    results = []
    for snapshot in snapshots:
        recorded = RecordedRpc(snapshot)
        with serve(recorded) as endpoint:
            result = export(Client(endpoint), manifest_for(snapshot), int(snapshot["slot"]))
        exported = result["snapshot"]
        for name, a in exported["accounts"].items():
            assert a == {k: snapshot["accounts"][name][k] for k in a}, name
        assert exported["now"] == snapshot["now"] and exported["slot"] == snapshot["slot"]
        assert result["verification"]["valid"]
        results.append({"label": snapshot["label"], "accounts": len(exported["accounts"]),
                        "requests": len(recorded.calls), "raw_bytes_match_signed_fixture": True,
                        "withdrawal": result["verification"]["withdrawal"],
                        "ceilings": result["verification"]["amount_ceilings_before_transaction_signatures"]})
    return {"schema": "K4V-E08-LOOPBACK-REPLAY-v1", "valid": True,
            "scope": "LOCAL_HTTP_REPLAY_OF_SIGNED_E07_FIXTURES", "checkpoints": len(results),
            "rpc_requests": sum(r["requests"] for r in results), "results": results,
            "public_rpc_requests": 0, "public_chain_transactions": 0,
            "live_public_deployment_verified": False, "independent_human_audit": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, default=ROOT / "examples/e07_rehearsal_bundle.json")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = reproduce(args.bundle)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(f"E-08 loopback HTTP: {result['checkpoints']} checkpoints, {result['rpc_requests']} read-only requests; zero public RPC")
