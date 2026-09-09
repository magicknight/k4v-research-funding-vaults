"""E-09 bounded timing and signed-intent cases; attestations are symbolic."""
import copy
from dataclasses import asdict, replace
import itertools
import unittest

import withdrawal_recovery_model as auth
import submission_window_model as model

START = 1_700_000_000


def state():
    core = auth.register("policy", "controller", START - 10 * auth.PERIOD,
                         b"opaque financial witness; no transfer model",
                         {"founder": "F", "treasury": "T"},
                         {"founder": ("F1", "F2", "F3"), "treasury": ("T1", "T2", "T3")}, {"F", "T"})
    return model.bind(core, "new-test-program", "ab" * 32)


def sign(intent, keys):
    return {k: intent.digest() for k in keys}


class SubmissionWindowTests(unittest.TestCase):
    def setUp(self):
        self.state = state()
        self.intent = model.prepare(self.state, "founder", "newF", "recovery", START, START + 300)
        self.signatures = sign(self.intent, ("F1", "F2", "newF"))

    def admit(self, now=START + 30):
        return model.submit(self.state, self.intent, self.signatures, now)

    def reject_unchanged(self, call, reason, source=None):
        source = source or self.state
        before = copy.deepcopy(source)
        with self.assertRaisesRegex(ValueError, reason):
            call()
        self.assertEqual(source, before)

    def test_delays_0_1_30_300_both_roles_and_modes_start_full_notice_at_arrival(self):
        for role, mode, delay in itertools.product(auth.ROLES, ("normal", "recovery"), (0, 1, 30, 300)):
            with self.subTest(role=role, mode=mode, delay=delay):
                r = self.state.core.roles[role]
                i = model.prepare(self.state, role, "new", mode, START, START + 300)
                keys = {"new", r.current} if mode == "normal" else {"new", *r.committee[:2]}
                s = model.submit(self.state, i, sign(i, keys), START + delay)
                p = s.core.roles[role].pending
                self.assertEqual((p.created_at, p.execute_after, p.expires_at),
                                 (START + delay, START + delay + auth.NOTICE, START + delay + auth.NOTICE + auth.EXECUTION_WINDOW))
                self.assertEqual(s.core.economics, self.state.core.economics)
                other = "treasury" if role == "founder" else "founder"
                self.assertEqual(s.core.roles[other], self.state.core.roles[other])

    def test_before_and_after_inclusive_submission_bounds_reject(self):
        for now in (START - 1, START + 301):
            self.reject_unchanged(lambda: self.admit(now), "SUBMISSION_TIME")

    def test_zero_width_window_is_valid_but_preserves_old_liveness_limit(self):
        i = replace(self.intent, valid_until=START)
        model.submit(self.state, i, sign(i, ("F1", "F2", "newF")), START)
        with self.assertRaisesRegex(ValueError, "SUBMISSION_TIME"):
            model.submit(self.state, i, sign(i, ("F1", "F2", "newF")), START + 1)

    def test_invalid_width_types_and_overflow_are_rejected_before_mutation(self):
        for start, end in ((START, START - 1), (START, START + 301), (True, START),
                           (START, "1700000300"), (-1, 1), (0, 2**63),
                           (auth.I64 - auth.NOTICE - auth.EXECUTION_WINDOW, auth.I64)):
            i = replace(self.intent, valid_from=start, valid_until=end)
            self.reject_unchanged(lambda: model.submit(self.state, i, sign(i, ("F1", "F2", "newF")), START),
                                  "SUBMISSION_WINDOW_WIDTH|INTEGER_RANGE|TIME_OVERFLOW")

    def test_largest_safe_window_and_execution_expiry_do_not_overflow(self):
        end = auth.I64 - auth.NOTICE - auth.EXECUTION_WINDOW
        i = replace(self.intent, valid_from=end - 300, valid_until=end)
        s = model.submit(self.state, i, sign(i, ("F1", "F2", "newF")), end)
        p = s.core.roles["founder"].pending
        self.assertEqual(p.expires_at, auth.I64)
        model.execute(s, "founder", i.digest(), auth.I64 - 1)
        model.expire(s, "founder", i.digest(), auth.I64)

    def test_all_intent_fields_are_bound_to_same_attestations(self):
        changes = {"program": "other-program", "policy": "other-policy", "identity": "cd" * 32,
                   "role": "treasury", "predecessor": "other-key", "successor": "other-successor",
                   "mode": "normal", "nonce": 2, "epoch": 1, "valid_from": START + 1, "valid_until": START + 299}
        for field, value in changes.items():
            i = replace(self.intent, **{field: value})
            self.assertNotEqual(i.digest(), self.intent.digest())
            self.reject_unchanged(lambda: model.submit(self.state, i, self.signatures, START + 30),
                                  "INTENT_DOMAIN|PREDECESSOR|SIGNED_INTENT_CHANGED")

    def test_missing_successor_current_key_and_quorum_reject(self):
        for keys in (("F1", "F2"), ("F1", "newF"), ("controller", "newF"), ("T1", "T2", "newF")):
            self.reject_unchanged(lambda: model.submit(self.state, self.intent, sign(self.intent, keys), START),
                                  "SUCCESSOR_ACCEPTANCE|PROPOSAL_AUTHORITY")
        i = replace(self.intent, mode="normal")
        with self.assertRaisesRegex(ValueError, "PROPOSAL_AUTHORITY"):
            model.submit(self.state, i, sign(i, ("F1", "F2", "newF")), START)

    def test_quorum_sets_do_not_change_with_delayed_admission(self):
        candidates = ("F1", "F2", "F3", "newF", "F", "controller", "T1")
        accepted = 0
        for mask in range(1 << len(candidates)):
            keys = {k for j, k in enumerate(candidates) if mask & (1 << j)}
            valid = "newF" in keys and len(keys & {"F1", "F2", "F3"}) >= 2
            try:
                model.submit(self.state, self.intent, sign(self.intent, keys), START + 300)
            except ValueError:
                self.assertFalse(valid)
            else:
                self.assertTrue(valid); accepted += 1
        self.assertEqual(accepted, 32)

    def test_changed_window_requires_fresh_acceptance_by_every_signer(self):
        i = replace(self.intent, valid_from=START + 301, valid_until=START + 601)
        partial = dict(self.signatures); partial["newF"] = i.digest()
        with self.assertRaisesRegex(ValueError, "SIGNED_INTENT_CHANGED"):
            model.submit(self.state, i, partial, START + 301)
        model.submit(self.state, i, sign(i, self.signatures), START + 301)

    def test_two_racing_intents_cannot_both_use_one_nonce(self):
        other = replace(self.intent, successor="other-newF")
        for first, second in ((self.intent, other), (other, self.intent)):
            s = model.submit(self.state, first, sign(first, ("F1", "F2", first.successor)), START)
            self.reject_unchanged(lambda: model.submit(s, second, sign(second, ("F1", "F2", second.successor)), START),
                                  "PENDING", s)

    def test_cancelled_intent_cannot_replay_even_within_its_signed_window(self):
        s = self.admit(START)
        s = model.cancel(s, "founder", self.intent.digest(), {"newF"}, START + 1)
        self.reject_unchanged(lambda: model.submit(s, self.intent, self.signatures, START + 2), "NONCE", s)
        i = model.prepare(s, "founder", "newF", "recovery", START + 2, START + 302)
        t = model.submit(s, i, sign(i, ("F1", "F2", "newF")), START + 2)
        self.assertEqual(i.nonce, 2)
        self.assertEqual(t.core.roles["founder"].pending.execute_after, START + 2 + auth.NOTICE)

    def test_execution_uses_actual_admission_not_earliest_signed_time(self):
        s = self.admit(START + 300)
        self.reject_unchanged(lambda: model.execute(s, "founder", self.intent.digest(), START + auth.NOTICE), "EXECUTION_WINDOW", s)
        self.reject_unchanged(lambda: model.execute(s, "founder", self.intent.digest(), START + 299 + auth.NOTICE), "EXECUTION_WINDOW", s)
        result = model.execute(s, "founder", self.intent.digest(), START + 300 + auth.NOTICE)
        self.assertEqual(result.core.roles["founder"].current, "newF")
        self.assertEqual(result.core.roles["founder"].epoch, 1)

    def test_expiry_is_exclusive_execution_and_inclusive_cleanup(self):
        s = self.admit()
        end = s.core.roles["founder"].pending.expires_at
        model.execute(s, "founder", self.intent.digest(), end - 1)
        self.reject_unchanged(lambda: model.execute(s, "founder", self.intent.digest(), end), "EXECUTION_WINDOW", s)
        self.reject_unchanged(lambda: model.expire(s, "founder", self.intent.digest(), end - 1), "NOT_EXPIRED", s)
        result = model.expire(s, "founder", self.intent.digest(), end)
        self.assertIsNone(result.core.roles["founder"].pending)

    def test_expired_intent_and_stale_epoch_cannot_be_reused(self):
        s = self.admit()
        end = s.core.roles["founder"].pending.expires_at
        s = model.expire(s, "founder", self.intent.digest(), end)
        with self.assertRaisesRegex(ValueError, "SUBMISSION_TIME"):
            model.submit(s, self.intent, self.signatures, end)
        s = model.execute(self.admit(), "founder", self.intent.digest(), START + 30 + auth.NOTICE)
        now = s.core.last_at
        i = model.prepare(s, "founder", "newerF", "normal", now, now + 300)
        i = replace(i, epoch=0)
        with self.assertRaisesRegex(ValueError, "EPOCH"):
            model.submit(s, i, sign(i, ("newF", "newerF")), now)

    def test_old_key_cannot_veto_recovery_but_normal_cancel_is_preserved(self):
        s = self.admit()
        self.reject_unchanged(lambda: model.cancel(s, "founder", self.intent.digest(), {"F"}, START + 31), "CANCEL_AUTHORITY", s)
        i = replace(self.intent, mode="normal")
        s = model.submit(self.state, i, sign(i, ("F", "newF")), START)
        model.cancel(s, "founder", i.digest(), {"F"}, START + 1)

    def test_pending_role_pause_and_other_role_independence_survive_delay(self):
        s = self.admit()
        with self.assertRaisesRegex(ValueError, "ROLE_PAUSED"):
            auth.authenticate(s.core, "founder", "F", 0, START + 31)
        auth.authenticate(s.core, "treasury", "T", 0, START + 31)
        i = model.prepare(s, "treasury", "newT", "recovery", START + 30, START + 330)
        t = model.submit(s, i, sign(i, ("T1", "T2", "newT")), START + 40)
        self.assertEqual(t.core.roles["founder"], s.core.roles["founder"])

    def test_approvals_and_opaque_financial_witness_are_preserved(self):
        self.state.core = auth.approve(self.state.core, "T", 0, 20, "destination", "recipient", 1, START - 1)
        s = self.admit()
        s = model.execute(s, "founder", self.intent.digest(), START + 30 + auth.NOTICE)
        self.assertEqual(s.core.approvals, self.state.core.approvals)
        self.assertEqual(s.core.economics, self.state.core.economics)

    def test_consistently_resigned_forbidden_successor_is_still_rejected(self):
        for successor in ("F", "F1"):
            i = replace(self.intent, successor=successor)
            with self.assertRaisesRegex(ValueError, "REUSED_WITHDRAWAL_KEY|ACTOR_IS_GUARDIAN"):
                model.submit(self.state, i, sign(i, ("F1", "F2", successor)), START)

    def test_clock_rollback_rejected_after_other_role_activity(self):
        self.state.core.last_at = START + 200
        self.reject_unchanged(lambda: self.admit(START + 199), "CLOCK_BACKWARDS")

    def test_final_pending_reconstruction_rejects_backdating_and_window_tampering(self):
        for field, value in (("created_at", START), ("execute_after", START + auth.NOTICE),
                             ("expires_at", START + auth.NOTICE + auth.EXECUTION_WINDOW)):
            s = self.admit()
            s.core.roles["founder"].pending = replace(s.core.roles["founder"].pending, **{field: value})
            with self.assertRaisesRegex(ValueError, "FULL_NOTICE_FROM_ADMISSION"):
                model.execute(s, "founder", self.intent.digest(), START + 30 + auth.NOTICE)
        s = self.admit(); key = ("founder", 1)
        s.admissions[key] = replace(s.admissions[key], intent=replace(self.intent, valid_until=START + 299))
        with self.assertRaisesRegex(ValueError, "ADMISSION_DIGEST"):
            model.pending(s, "founder", self.intent.digest())

    def test_signing_time_is_not_claimed_observable_and_future_window_waits(self):
        i = replace(self.intent, valid_from=START + 100, valid_until=START + 200)
        signatures = sign(i, ("F1", "F2", "newF"))
        with self.assertRaisesRegex(ValueError, "SUBMISSION_TIME"):
            model.submit(self.state, i, signatures, START)
        s = model.submit(self.state, i, signatures, START + 100)
        self.assertEqual(s.core.roles["founder"].pending.created_at, START + 100)
        self.assertNotIn("signed_at", asdict(i))


if __name__ == "__main__":
    unittest.main()
