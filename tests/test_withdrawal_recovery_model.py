"""Adversarial authority traces, not transaction or production acceptance."""
from copy import deepcopy
from dataclasses import asdict, replace
from itertools import combinations
import json
from pathlib import Path
import unittest

import withdrawal_recovery_model as m
from e05_verifier import expand_bundle, verify_graph

ROOT = Path(__file__).resolve().parents[1]


def fixture(shared=False):
    actors = {"founder": "one-key" if shared else "founder-0",
              "treasury": "one-key" if shared else "treasury-0"}
    return m.register("policy-A", "one-key" if shared else "controller", 1_000,
                      b"frozen-pdas-principal-t0-caps-annual-report-counters",
                      actors, {"founder": ("f1", "f2", "f3"),
                               "treasury": ("t1", "t2", "t3")}, set(actors.values()))


def propose(s, role="founder", successor="founder-1", now=1_000, mode="recovery"):
    r = s.roles[role]
    signers = set(r.committee[:2]) if mode == "recovery" else {r.current}
    return m.propose(s, role, successor, mode, r.sequence + 1, r.epoch,
                     signers | {successor}, now)


class WithdrawalRecoveryTests(unittest.TestCase):
    def rejected(self, state, reason, operation, *args, **kwargs):
        before = deepcopy(state)
        with self.assertRaisesRegex(ValueError, reason):
            operation(state, *args, **kwargs)
        self.assertEqual(state, before, "rejected operation mutated its input")

    def test_both_initial_roles_must_consent_even_if_creator_controls_policy(self):
        for signers in ({"controller"}, {"founder-0"}, {"treasury-0"}):
            with self.assertRaisesRegex(ValueError, "REGISTRATION_CONSENT"):
                m.register("p", "controller", 1, b"ledger",
                           {r: r + "-0" for r in m.ROLES},
                           {"founder": ("f1", "f2", "f3"), "treasury": ("t1", "t2", "t3")}, signers)

    def test_invalid_committee_or_operating_key_in_own_backups_rejected(self):
        for members in (("f1", "f1", "f2"), ("f1", "f2"), ("founder-0", "f1", "f2")):
            with self.assertRaises(ValueError):
                m.register("p", "controller", 1, b"ledger", {r: r + "-0" for r in m.ROLES},
                           {"founder": members, "treasury": ("t1", "t2", "t3")}, {"founder-0", "treasury-0"})

    def test_every_signer_subset_has_exact_role_quorum_and_successor_rule(self):
        # Exhaust all 2**7 signer sets: duplicates cannot add voting weight.
        population = ["f1", "f2", "f3", "t1", "controller", "founder-0", "new"]
        s = fixture()
        accepted = 0
        for n in range(len(population) + 1):
            for subset in combinations(population, n):
                expected = "new" in subset and len(set(subset) & {"f1", "f2", "f3"}) >= 2
                if expected:
                    self.assertIsNotNone(m.propose(s, "founder", "new", "recovery", 1, 0, subset, 1_000).roles["founder"].pending)
                    accepted += 1
                else:
                    self.rejected(s, "ACCEPTANCE|AUTHORITY", m.propose,
                                  "founder", "new", "recovery", 1, 0, subset, 1_000)
        self.assertEqual(accepted, 32)
        self.rejected(s, "AUTHORITY", m.propose, "founder", "new", "recovery", 1, 0, ["f1", "f1", "new"], 1_000)

    def test_normal_rotation_needs_current_key_and_successor(self):
        s = fixture()
        for signers in ({"founder-0"}, {"new"}, {"controller", "new"}):
            self.rejected(s, "ACCEPTANCE|AUTHORITY", m.propose, "founder", "new", "normal", 1, 0, signers, 1_000)
        self.assertEqual(propose(s, mode="normal").roles["founder"].pending.mode, "normal")

    def test_wait_minus_one_exact_and_expiry_boundaries(self):
        s = propose(fixture())
        p = s.roles["founder"].pending
        self.rejected(s, "EXECUTION_WINDOW", m.execute, "founder", p.digest(), p.execute_after - 1)
        for now in (p.execute_after, p.expires_at - 1):
            self.assertEqual(m.execute(s, "founder", p.digest(), now).roles["founder"].current, "founder-1")
        self.rejected(s, "EXECUTION_WINDOW", m.execute, "founder", p.digest(), p.expires_at)
        self.rejected(s, "NOT_EXPIRED", m.expire, "founder", p.digest(), p.expires_at - 1)
        self.assertIsNone(m.expire(s, "founder", p.digest(), p.expires_at).roles["founder"].pending)

    def test_pending_freezes_only_affected_role_and_cannot_be_overwritten(self):
        s = propose(fixture())
        self.rejected(s, "ROLE_PAUSED", m.authorize_release, "founder", "founder-0", 0, "ata", "founder-0", 1_000)
        self.rejected(s, "PENDING", m.propose, "founder", "x", "recovery", 2, 0, {"f1", "f2", "x"}, 1_000)
        t = m.approve(s, "treasury-0", 0, 6, "vendor-ata", "vendor", 100, 1_000)
        self.assertEqual(t.roles["founder"], s.roles["founder"])
        t = propose(t, "treasury", "treasury-1")
        self.rejected(t, "ROLE_PAUSED", m.approve, "treasury-0", 0, 7, "v2-ata", "v2", 100, 1_000)

    def test_old_key_and_controller_cannot_veto_recovery(self):
        s = propose(fixture())
        p = s.roles["founder"].pending
        for signers in ({"founder-0"}, {"controller"}, {"f1"}, {"t1", "t2"}):
            self.rejected(s, "CANCEL_AUTHORITY", m.cancel, "founder", p.digest(), signers, 1_000)
        for signers in ({"founder-1"}, {"f1", "f2"}):
            self.assertIsNone(m.cancel(s, "founder", p.digest(), signers, 1_000).roles["founder"].pending)

    def test_current_key_can_cancel_only_normal_rotation(self):
        s = propose(fixture(), mode="normal")
        self.assertIsNone(m.cancel(s, "founder", s.roles["founder"].pending.digest(), {"founder-0"}, 1_000).roles["founder"].pending)

    def test_cancelled_or_executed_digest_cannot_replay_and_nonce_is_spent(self):
        s = propose(fixture())
        p = s.roles["founder"].pending
        for closed in (m.cancel(s, "founder", p.digest(), {"founder-1"}, 1_001),
                       m.execute(s, "founder", p.digest(), p.execute_after)):
            self.rejected(closed, "NO_PENDING", m.execute, "founder", p.digest(), closed.last_at)
            self.rejected(closed, "NONCE", m.propose, "founder", "another", "recovery", 1,
                          closed.roles["founder"].epoch, {"f1", "f2", "another"}, closed.last_at)
            new = propose(closed, successor="another", now=closed.last_at)
            self.assertEqual(new.roles["founder"].pending.execute_after, closed.last_at + m.NOTICE)
            self.rejected(new, "PROPOSAL_BINDING", m.execute, "founder", p.digest(), closed.last_at)

    def test_digest_binds_policy_role_epoch_successor_mode_nonce_and_times(self):
        s = propose(fixture())
        p = s.roles["founder"].pending
        for field, value in {"policy": "other", "role": "treasury", "epoch": 1,
                             "successor": "thief", "predecessor": "thief", "mode": "normal",
                             "nonce": 2, "created_at": 2, "execute_after": 2, "expires_at": 2}.items():
            self.rejected(s, "PROPOSAL_BINDING", m.execute, "founder", replace(p, **{field: value}).digest(), p.execute_after)

    def test_stale_epoch_and_old_key_fail_after_recovery(self):
        s = propose(fixture())
        p = s.roles["founder"].pending
        s = m.execute(s, "founder", p.digest(), p.execute_after)
        for signer, epoch in (("founder-0", 0), ("founder-0", 1), ("founder-1", 0), ("controller", 1)):
            self.rejected(s, "WITHDRAWAL_AUTHORITY", m.authorize_release, "founder", signer, epoch, "ata", signer, s.last_at)
        permit = m.authorize_release(s, "founder", "founder-1", 1, "ata", "founder-1", s.last_at)
        self.assertTrue(permit["financial_kernel_required"])
        self.assertFalse(permit["transfer_executed"])
        self.rejected(s, "FOUNDER_DESTINATION", m.authorize_release, "founder", "founder-1", 1, "ata", "founder-0", s.last_at)

    def test_single_operating_key_lost_across_three_roles_restores_only_consented_scope(self):
        s = fixture(shared=True)
        s = propose(s, successor="f-new")
        s = propose(s, "treasury", "t-new")
        s = m.execute(s, "founder", s.roles["founder"].pending.digest(), 1_000 + m.NOTICE)
        self.assertEqual(s.roles["treasury"].current, "one-key")
        self.assertIsNotNone(s.roles["treasury"].pending)
        s = m.execute(s, "treasury", s.roles["treasury"].pending.digest(), s.last_at)
        self.assertEqual((s.roles["founder"].current, s.roles["treasury"].current, s.controller), ("f-new", "t-new", "one-key"))
        self.assertEqual(s.roles["founder"].initial, "one-key")
        self.assertEqual(s.roles["treasury"].initial, "one-key")

    def test_missing_two_backups_has_no_admin_or_controller_escape(self):
        s = fixture()
        self.rejected(s, "PROPOSAL_AUTHORITY", m.propose, "founder", "new", "recovery", 1, 0,
                      {"new", "controller", "f1", "t1", "t2", "t3"}, 1_000)

    def test_old_withdrawal_keys_and_own_guardian_cannot_be_reused(self):
        s = propose(fixture())
        s = m.execute(s, "founder", s.roles["founder"].pending.digest(), 1_000 + m.NOTICE)
        for successor in ("founder-0", "founder-1", "f1"):
            self.rejected(s, "REUSED_WITHDRAWAL_KEY|ACTOR_IS_GUARDIAN", m.propose, "founder", successor,
                          "recovery", 2, 1, {"f1", "f2", successor}, s.last_at)

    def test_treasury_approval_keeps_recipient_need_consumed_author_and_notice(self):
        s = m.approve(fixture(), "treasury-0", 0, 6, "vendor-ata", "vendor", 100, 1_000)
        # Import an already partially consumed financial-kernel approval witness.
        s.approvals[6] = replace(s.approvals[6], consumed=27)
        original = deepcopy(s.approvals)
        s = propose(s, "treasury", "treasury-1")
        s = m.execute(s, "treasury", s.roles["treasury"].pending.digest(), 1_000 + m.NOTICE)
        self.assertEqual(s.approvals, original)
        now = 1_000 + 6 * m.PERIOD
        permit = m.authorize_release(s, "treasury", "treasury-1", 1, "vendor-ata", "vendor", now, 6)
        self.assertFalse(permit["transfer_executed"])
        self.rejected(s, "APPROVAL_DESTINATION", m.authorize_release, "treasury", "treasury-1", 1, "new-ata", "vendor", now, 6)
        self.rejected(s, "APPROVAL_IMMUTABLE", m.approve, "treasury-1", 1, 6, "new-ata", "new-vendor", 500, s.last_at)

    def test_known_self_payment_is_blocked_before_and_after_role_rotation(self):
        s = fixture()
        for owner in ("founder-0", "treasury-0", "f1", "t1"):
            self.rejected(s, "KNOWN_SELF_PAYMENT", m.approve, "treasury-0", 0, 6, "ata", owner, 100, 1_000)
        s = propose(s)
        self.rejected(s, "KNOWN_SELF_PAYMENT", m.approve, "treasury-0", 0, 6, "ata", "founder-1", 100, 1_000)
        s = m.execute(s, "founder", s.roles["founder"].pending.digest(), 1_000 + m.NOTICE)
        for owner in ("founder-0", "founder-1"):
            self.rejected(s, "KNOWN_SELF_PAYMENT", m.approve, "treasury-0", 0, 6, "ata", owner, 100, s.last_at)

    def test_approved_recipient_cannot_become_successor_to_launder_self_payment(self):
        s = m.approve(fixture(), "treasury-0", 0, 6, "vendor-ata", "vendor", 100, 1_000)
        for role in m.ROLES:
            with self.assertRaisesRegex(ValueError, "SUCCESSOR_IS_APPROVED_RECIPIENT"):
                propose(s, role, "vendor")

    def test_new_unknown_wallet_is_not_proof_of_independent_recipient(self):
        # Explicit residual-risk witness: pubkey inequality cannot identify a human.
        s = m.approve(fixture(), "treasury-0", 0, 6, "alias-ata", "undisclosed-founder-alias", 100, 1_000)
        self.assertEqual(s.approvals[6].owner, "undisclosed-founder-alias")

    def test_treasury_notice_and_current_period_cannot_be_relabelled(self):
        s = m.approve(fixture(), "treasury-0", 0, 1, "ata", "vendor", 100, 1_001)
        boundary = s.t0 + m.PERIOD
        self.rejected(s, "APPROVAL_NOTICE", m.authorize_release, "treasury", "treasury-0", 0, "ata", "vendor", boundary, 1)
        self.assertFalse(m.authorize_release(s, "treasury", "treasury-0", 0, "ata", "vendor", boundary + 1, 1)["transfer_executed"])
        self.rejected(s, "APPROVAL_PERIOD", m.authorize_release, "treasury", "treasury-0", 0, "ata", "vendor", s.t0 + 2 * m.PERIOD, 1)
        self.rejected(s, "APPROVAL_NOTICE|FUTURE_PERIOD", m.approve, "treasury-0", 0, 0, "ata", "vendor", 100, 1_001)

    def test_clock_integer_nonce_epoch_and_timestamp_overflow_rejected(self):
        s = propose(fixture())
        p = s.roles["founder"].pending
        self.rejected(s, "CLOCK_BACKWARDS", m.cancel, "founder", p.digest(), {"founder-1"}, 999)
        for now in (-1, True, 1.5, m.I64 + 1, m.I64):
            with self.assertRaises(ValueError):
                propose(fixture(), now=now)
        for sequence, epoch in ((m.U64, 0), (0, m.U64)):
            s = fixture()
            s.roles["founder"].sequence, s.roles["founder"].epoch = sequence, epoch
            with self.assertRaises(ValueError):
                propose(s)
        self.rejected(fixture(), "EPOCH", m.propose, "founder", "new", "recovery", 1, 1, {"f1", "f2", "new"}, 1_000)

    def test_reproposal_after_expiry_restarts_full_wait_without_resetting_counters(self):
        s = propose(fixture())
        p = s.roles["founder"].pending
        s = m.expire(s, "founder", p.digest(), p.expires_at)
        self.assertEqual(s.roles["founder"].epoch, 0)
        self.assertEqual(s.roles["founder"].sequence, 1)
        new = propose(s, now=s.last_at)
        self.assertEqual(new.roles["founder"].pending.execute_after, p.expires_at + m.NOTICE)
        self.assertEqual(new.economics, s.economics)

    def test_verified_e05_bytes_are_preserved_by_every_model_recovery_path(self):
        bundle = json.loads((ROOT / "examples/e05_rehearsal_bundle.json").read_text())
        graph = expand_bundle(bundle)[-1]
        self.assertTrue(verify_graph(graph)["valid"])
        s = fixture()
        # Entire verified graph includes original PDAs/T0/principal/counters/
        # annual inputs/approvals and loader: authority model cannot modify it.
        s.economics = json.dumps(graph, sort_keys=True, separators=(",", ":")).encode()
        witness = s.economics
        for role in m.ROLES:
            for mode in ("normal", "recovery"):
                opened = propose(s, role, role + "-next", mode=mode)
                p = opened.roles[role].pending
                for result in (opened, m.cancel(opened, role, p.digest(), {p.successor}, 1_000),
                               m.expire(opened, role, p.digest(), p.expires_at),
                               m.execute(opened, role, p.digest(), p.execute_after)):
                    self.assertEqual(result.economics, witness)
                    self.assertEqual(result.roles[role].initial, s.roles[role].initial)
                    self.assertEqual(result.controller, s.controller)
                    self.assertEqual(result.t0, s.t0)
        # This does not establish an E-06 account migration or SBF release.


if __name__ == "__main__":
    unittest.main()
