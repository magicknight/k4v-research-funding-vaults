#!/usr/bin/env python3
"""Render verified, frozen author-run evidence; never a wallet or live-node demo."""
from __future__ import annotations

import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = ("SHA256SUMS", "E11B_SHA256SUMS", "E11C_SHA256SUMS")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_manifests(root):
    """The user-selected checkout is the trust anchor, not this mutable report."""
    checked = 0
    for name in MANIFESTS:
        manifest = root / name
        require(not manifest.is_symlink(), "MANIFEST_SYMLINK")
        for line in manifest.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            digest, relative = line.split(maxsplit=1)
            relative = relative.removeprefix("*")
            require(len(digest) == 64 and all(c in "0123456789abcdef" for c in digest), "MANIFEST_DIGEST")
            target = root / relative
            require(not Path(relative).is_absolute() and ".." not in Path(relative).parts, "MANIFEST_PATH")
            require(target.resolve().is_relative_to(root.resolve()) and not target.is_symlink(), "MANIFEST_ESCAPE")
            with target.open("rb") as stream:
                hasher = hashlib.sha256()
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    hasher.update(chunk)
                actual = hasher.hexdigest()
            require(actual == digest, "CHECKSUM_MISMATCH:" + relative)
            checked += 1
    require(checked > 0, "EMPTY_MANIFESTS")
    return checked


def run_verifier(root, name):
    # -E keeps PYTHONOPTIMIZE/PYTHONPATH from silently changing frozen verifiers.
    result = subprocess.run([sys.executable, "-E", "-B", str(root / "tools" / name)],
                            cwd=root, capture_output=True, text=True, timeout=120, check=True)
    return json.loads(result.stdout)


def compose_report(initialization, continuation, files_checked):
    require(initialization.get("valid") is True, "INITIALIZATION_INVALID")
    require(initialization.get("scope") == "RECHECK_OF_ARCHIVED_AUTHOR_LOCAL_EVIDENCE", "INITIALIZATION_SCOPE")
    for name, expected in (("financial_checkpoints", 11), ("agave_account_checkpoints", 4)):
        require(type(initialization.get(name)) is int and initialization[name] == expected, name)
    require(initialization.get("preparation") is True, "PREPARATION")
    for name in ("independent_human_review", "live_rpc_observed_by_this_command"):
        require(initialization.get(name) is False, "INITIALIZATION_SCOPE:" + name)
    require(continuation.get("valid") is True, "CONTINUATION_INVALID")
    require(continuation.get("mode") == "OFFLINE_ARCHIVED_BYTES_REPLAY", "CONTINUATION_MODE")
    for name, expected in (("finalized_transactions_in_archive", 11), ("raw_checkpoints_replayed", 8)):
        require(type(continuation.get(name)) is int and continuation[name] == expected, name)
    require(continuation.get("application_state_preloaded") is True, "PRELOAD_DISCLOSURE")
    for name in ("natural_90_180_day_soak", "production_ready", "independent_human_review"):
        require(continuation.get(name) is False, "CONTINUATION_SCOPE:" + name)
    phases = continuation.get("phases", {})
    require(set(phases) == {"recovery", "expiry"}, "PHASE_SET")
    for name, phase in phases.items():
        require(phase.get("phase") == name, "PHASE_NAME")
        for key in ("valid", "authority_action_preserved_counters_and_budgets", "supply_conserved",
                    "bounded_withdrawals_after_authority_action", "application_state_preloaded", "fixture_clock_controlled"):
            require(phase.get(key) is True, "PHASE_EVIDENCE:" + key)
        require(type(phase.get("raw_checkpoints")) is int and phase["raw_checkpoints"] == 4, "PHASE_CHECKPOINTS")
        require(phase.get("year_two_checked") is (name == "expiry"), "YEAR_TWO")
        for key in ("natural_90_180_day_soak", "production_ready", "independent_human_review"):
            require(phase.get(key) is False, "PHASE_SCOPE:" + key)
    require(type(files_checked) is int and files_checked > 0, "CHECKSUM_COUNT")
    return {
        "schema": "K4V-EVIDENCE-DEMO-v1",
        "valid": True,
        "mode": "OFFLINE_REPLAY_OF_ARCHIVED_AUTHOR_EVIDENCE",
        "files_checksum_verified": files_checked,
        "initialization": initialization,
        "continuation": continuation,
        "demonstrations": [
            {"name": "Six-role initialization", "result": "Preparation and four actual-Agave account checkpoints redecoded.",
             "boundary": "Archived single-operator test keys, not independent people."},
            {"name": "Financial lifecycle", "result": "Eleven E11B native-loader financial checkpoints verified.",
             "boundary": "Test fixtures; not eleven new live-node transactions."},
            {"name": "Withdrawal-key recovery", "result": "Authority changes preserve counters and budgets; bounded withdrawals continue.",
             "boundary": "E11C mature application state was preloaded."},
            {"name": "Expiry and year two", "result": "Expiry preserves the current key; subsequent annual accounting is checked.",
             "boundary": "A separate preloaded scenario, not continuous history from initialization."},
            {"name": "Rejected operations", "result": "E11C raw application-account bytes remain unchanged after recorded refusals.",
             "boundary": "Verified by the frozen continuation verifier, not a fresh transaction submission."},
        ],
        "boundaries": {"new_transactions": 0, "live_rpc": False, "wallet_required": False,
                       "natural_90_180_day_soak": False, "independent_human_review": False,
                       "production_ready": False, "official_mint": None},
    }


def build_report(root=ROOT):
    count = verify_manifests(root)
    initialization = run_verifier(root, "verify_e11b_archive.py")
    continuation = run_verifier(root, "verify_e11c_archive.py")
    report = compose_report(initialization, continuation, count)
    report["provenance"] = {
        "trust_anchor": "The exact source checkout selected and verified by the reader; not an external attestation.",
        "manifests_sha256": {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in MANIFESTS},
        "program_sha256": json.loads((root / "evidence/E11C_ACCEPTED_2026-09-10.json").read_text())["canonical_v7_program_sha256"],
    }
    return report


def render_html(report):
    escape = lambda value: html.escape(str(value), quote=True)
    rows = "".join("<tr>" + "".join("<td>" + escape(row[key]) + "</td>" for key in ("name", "result", "boundary")) + "</tr>"
                   for row in report["demonstrations"])
    raw = escape(json.dumps(report, indent=2, ensure_ascii=False))
    return """<!doctype html>
<html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Purpose-Bound Vaults — Verified Evidence Demo</title>
<style>body{font:17px/1.6 system-ui,sans-serif;max-width:1050px;margin:3rem auto;padding:0 1.2rem;color:#172936;background:#f7f9fa}h1{line-height:1.2}aside{border-left:5px solid #b56b0b;padding:1rem;background:#fff3d9}table{border-collapse:collapse;width:100%;margin:2rem 0}th,td{text-align:left;vertical-align:top;padding:.8rem;border-bottom:1px solid #bdcbd1}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px}small{color:#415c6b}</style>
<main><small>OPEN-SOURCE DEVELOPER TOOLING · NO WALLET · NO NETWORK REQUESTS</small>
<h1>Purpose-Bound Vaults<br>Verified evidence walkthrough</h1>
<p>Replay recorded initialization, key recovery, constrained withdrawals and refusal behavior from raw account bytes.</p>
<aside><strong>OFFLINE ARCHIVED EVIDENCE — NOT A LIVE DAPP.</strong><br>
No transactions are sent. Mature E11C states were preloaded. This is not natural 90/180-day history,
independent human review, production approval, or an official token launch.</aside>
<table><thead><tr><th>Demonstration</th><th>Verified result</th><th>Limit of the evidence</th></tr></thead><tbody>""" + rows + """</tbody></table>
<p>All results are author-run historical evidence rechecked on your machine. A static report can be copied or edited;
rerun <code>python3 tools/evidence_demo.py</code> from your independently selected exact checkout.</p>
<details><summary>Machine-readable verification and provenance</summary><pre>""" + raw + """</pre></details></main></html>
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=("json", "html"), default="json")
    parser.add_argument("--output", type=Path, help="Create a new file; existing files are never overwritten.")
    args = parser.parse_args()
    try:
        report = build_report()
        content = render_html(report) if args.format == "html" else json.dumps(report, indent=2) + "\n"
        if args.output:
            with args.output.open("x", encoding="utf-8") as stream:
                stream.write(content)
        else:
            print(content, end="")
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print("EVIDENCE_DEMO_FAILED: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
