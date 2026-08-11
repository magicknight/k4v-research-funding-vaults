# SPDX-License-Identifier: MIT OR Apache-2.0
import unittest

from purpose_bound_vault import (
    EventKind,
    ReleaseEvent,
    ReleaseRequest,
    decision_receipt,
    evaluate_release,
    net_release,
    request_from_dict,
)


class PurposeBoundVaultTests(unittest.TestCase):
    def base_request(self, **overrides):
        values = {
            "period_id": "2026-08",
            "beneficiary_locked_balance_at_year_start": 300_000_000,
            "purpose_locked_balance_at_year_start": 500_000_000,
            "annual_release_bps": 300,
            "eligible_trailing_30d_spot_volume": 100_000_000,
            "market_capacity_bps": 500,
            "beneficiary_months_since_genesis": 24,
            "beneficiary_events": (),
            "purpose_events": (),
            "purpose_approved_need": 0,
        }
        values.update(overrides)
        return ReleaseRequest(**values)

    def test_monthly_caps_at_three_percent(self):
        decision = evaluate_release(self.base_request())
        self.assertEqual(decision.beneficiary_monthly_rate_cap, 750_000)
        self.assertEqual(decision.purpose_monthly_rate_cap, 1_250_000)
        self.assertEqual(decision.aggregate_market_capacity, 5_000_000)

    def test_valid_release(self):
        decision = evaluate_release(self.base_request(
            beneficiary_events=(ReleaseEvent(EventKind.SALE, 500_000),),
            purpose_events=(ReleaseEvent(EventKind.SALE, 1_000_000),),
            purpose_approved_need=1_000_000,
        ))
        self.assertTrue(decision.allowed)
        self.assertEqual(decision.reasons, ())

    def test_cliff_rejects_any_beneficiary_release(self):
        decision = evaluate_release(self.base_request(
            beneficiary_months_since_genesis=23,
            beneficiary_events=(ReleaseEvent(EventKind.OTC, 1),),
        ))
        self.assertIn("BENEFICIARY_CLIFF", decision.reasons)

    def test_beneficiary_monthly_cap(self):
        decision = evaluate_release(self.base_request(
            beneficiary_events=(ReleaseEvent(EventKind.SALE, 750_001),),
        ))
        self.assertIn("BENEFICIARY_MONTHLY_RATE_CAP", decision.reasons)

    def test_purpose_vault_requires_approved_need(self):
        decision = evaluate_release(self.base_request(
            purpose_events=(ReleaseEvent(EventKind.SALE, 1),),
        ))
        self.assertIn("PURPOSE_APPROVED_NEED", decision.reasons)

    def test_aggregate_market_capacity_is_joint(self):
        decision = evaluate_release(self.base_request(
            eligible_trailing_30d_spot_volume=20_000_000,
            beneficiary_events=(ReleaseEvent(EventKind.SALE, 500_000),),
            purpose_events=(ReleaseEvent(EventKind.SALE, 750_000),),
            purpose_approved_need=750_000,
        ))
        self.assertEqual(decision.aggregate_market_capacity, 1_000_000)
        self.assertIn("AGGREGATE_MARKET_CAPACITY", decision.reasons)

    def test_zero_eligible_volume_rejects_release(self):
        decision = evaluate_release(self.base_request(
            eligible_trailing_30d_spot_volume=0,
            purpose_events=(ReleaseEvent(EventKind.SALE, 1),),
            purpose_approved_need=1,
        ))
        self.assertIn("AGGREGATE_MARKET_CAPACITY", decision.reasons)

    def test_bypass_events_count_as_release(self):
        events = tuple(
            ReleaseEvent(kind, 10)
            for kind in (
                EventKind.SALE,
                EventKind.OTC,
                EventKind.GRANT,
                EventKind.FREE_WALLET_TRANSFER,
                EventKind.COLLATERAL,
                EventKind.ECONOMIC_RIGHT_TRANSFER,
            )
        ) + (ReleaseEvent(EventKind.EQUAL_OR_STRICTER_LOCK_MIGRATION, 1_000),)
        self.assertEqual(net_release(events), 60)

    def test_rates_above_five_percent_are_rejected(self):
        with self.assertRaises(ValueError):
            self.base_request(annual_release_bps=501)
        with self.assertRaises(ValueError):
            self.base_request(market_capacity_bps=501)

    def test_negative_amount_is_rejected(self):
        with self.assertRaises(ValueError):
            ReleaseEvent(EventKind.SALE, -1)

    def test_receipt_is_deterministic_and_input_sensitive(self):
        first = decision_receipt(self.base_request())
        second = decision_receipt(self.base_request())
        changed = decision_receipt(self.base_request(purpose_approved_need=1))
        self.assertEqual(first, second)
        self.assertNotEqual(first["sha256"], changed["sha256"])
        self.assertEqual(len(first["sha256"]), 64)

    def test_json_round_trip(self):
        request = request_from_dict({
            "period_id": "2026-08",
            "beneficiary_locked_balance_at_year_start": 10,
            "purpose_locked_balance_at_year_start": 20,
            "annual_release_bps": 0,
            "eligible_trailing_30d_spot_volume": 0,
            "market_capacity_bps": 0,
            "beneficiary_months_since_genesis": 24,
            "beneficiary_events": [{"kind": "sale", "amount": 0}],
            "purpose_events": [],
            "purpose_approved_need": 0,
        })
        self.assertTrue(evaluate_release(request).allowed)

    def test_prior_unused_capacity_cannot_be_injected(self):
        data = {
            "period_id": "2026-08",
            "beneficiary_locked_balance_at_year_start": 10,
            "purpose_locked_balance_at_year_start": 20,
            "annual_release_bps": 0,
            "eligible_trailing_30d_spot_volume": 0,
            "market_capacity_bps": 0,
            "beneficiary_months_since_genesis": 24,
            "purpose_approved_need": 0,
            "unused_prior_capacity": 999,
        }
        with self.assertRaises(TypeError):
            request_from_dict(data)

    def test_period_id_is_validated_and_hash_bound(self):
        with self.assertRaises(ValueError):
            self.base_request(period_id="August 2026")
        august = decision_receipt(self.base_request(period_id="2026-08"))
        september = decision_receipt(self.base_request(period_id="2026-09"))
        self.assertNotEqual(august["sha256"], september["sha256"])


if __name__ == "__main__":
    unittest.main()
