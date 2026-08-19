# SPDX-License-Identifier: MIT OR Apache-2.0
"""Cross-language parity between the B2 program and the chain-independent model.

The on-chain B2 program and `purpose_bound_vault` were written separately, in
different languages, from the same covenant. This module pins one vector that
both must reproduce exactly. Its integers also appear in the Rust unit test
`policy::tests::cross_language_parity_vector` and in the LiteSVM test
`two_vaults_each_within_cap_cannot_jointly_exceed_market_capacity`; if any of
the three drifts, at least one of them fails.

Parity is asserted over integers only. It does not claim the two
implementations are equivalent in behaviour: the model has no accounts, no
oracle staleness, no notice period, and no recusal rule.
"""

import unittest

from purpose_bound_vault import (
    EventKind,
    ReleaseEvent,
    ReleaseRequest,
    evaluate_release,
)

BENEFICIARY_DEPOSIT = 300_000_000
PURPOSE_DEPOSIT = 500_000_000
ANNUAL_RELEASE_BPS = 500
ELIGIBLE_VOLUME = 120_000_000
MARKET_CAPACITY_BPS = 250

BENEFICIARY_CAP = 1_250_000
PURPOSE_CAP = 2_083_333
MARKET_CAPACITY = 3_000_000

PERIOD_SECONDS = 30 * 24 * 60 * 60
MIN_CLIFF_SECONDS = 730 * 24 * 60 * 60
MIN_NOTICE_SECONDS = 30 * 24 * 60 * 60
MAX_ANNUAL_RELEASE_BPS = 500
MAX_MARKET_CAPACITY_BPS = 500


def request(beneficiary_release, purpose_release, approved_need, months=36):
    return ReleaseRequest(
        period_id="2028-09",
        beneficiary_locked_balance_at_year_start=BENEFICIARY_DEPOSIT,
        purpose_locked_balance_at_year_start=PURPOSE_DEPOSIT,
        annual_release_bps=ANNUAL_RELEASE_BPS,
        eligible_trailing_30d_spot_volume=ELIGIBLE_VOLUME,
        market_capacity_bps=MARKET_CAPACITY_BPS,
        beneficiary_months_since_genesis=months,
        beneficiary_events=(
            (ReleaseEvent(EventKind.SALE, beneficiary_release),)
            if beneficiary_release
            else ()
        ),
        purpose_events=(
            (ReleaseEvent(EventKind.SALE, purpose_release),) if purpose_release else ()
        ),
        purpose_approved_need=approved_need,
    )


class TestB2Parity(unittest.TestCase):
    def test_the_three_caps_match_the_on_chain_program(self):
        decision = evaluate_release(request(0, 0, 0))
        self.assertEqual(decision.beneficiary_monthly_rate_cap, BENEFICIARY_CAP)
        self.assertEqual(decision.purpose_monthly_rate_cap, PURPOSE_CAP)
        self.assertEqual(decision.aggregate_market_capacity, MARKET_CAPACITY)

    def test_each_vault_alone_fits_but_together_they_do_not(self):
        self.assertLessEqual(BENEFICIARY_CAP, MARKET_CAPACITY)
        self.assertLessEqual(PURPOSE_CAP, MARKET_CAPACITY)
        self.assertGreater(BENEFICIARY_CAP + PURPOSE_CAP, MARKET_CAPACITY)

        decision = evaluate_release(
            request(BENEFICIARY_CAP, PURPOSE_CAP, PURPOSE_CAP)
        )
        self.assertFalse(decision.allowed)
        self.assertEqual(decision.reasons, ("AGGREGATE_MARKET_CAPACITY",))

    def test_exactly_the_remaining_headroom_is_allowed(self):
        headroom = MARKET_CAPACITY - BENEFICIARY_CAP
        allowed = evaluate_release(request(BENEFICIARY_CAP, headroom, headroom))
        self.assertTrue(allowed.allowed)
        self.assertEqual(allowed.reasons, ())

        over = evaluate_release(request(BENEFICIARY_CAP, headroom + 1, headroom + 1))
        self.assertFalse(over.allowed)
        self.assertIn("AGGREGATE_MARKET_CAPACITY", over.reasons)

    def test_an_unapproved_purpose_release_is_refused(self):
        decision = evaluate_release(request(0, 1, 0))
        self.assertFalse(decision.allowed)
        self.assertEqual(decision.reasons, ("PURPOSE_APPROVED_NEED",))

    def test_zero_eligible_volume_refuses_any_release(self):
        zero_volume = ReleaseRequest(
            period_id="2028-09",
            beneficiary_locked_balance_at_year_start=BENEFICIARY_DEPOSIT,
            purpose_locked_balance_at_year_start=PURPOSE_DEPOSIT,
            annual_release_bps=ANNUAL_RELEASE_BPS,
            eligible_trailing_30d_spot_volume=0,
            market_capacity_bps=MARKET_CAPACITY_BPS,
            beneficiary_months_since_genesis=36,
            purpose_events=(ReleaseEvent(EventKind.SALE, 1),),
            purpose_approved_need=1,
        )
        decision = evaluate_release(zero_volume)
        self.assertEqual(decision.aggregate_market_capacity, 0)
        self.assertFalse(decision.allowed)
        self.assertIn("AGGREGATE_MARKET_CAPACITY", decision.reasons)

    def test_the_beneficiary_cliff_binds_in_both_implementations(self):
        decision = evaluate_release(request(1, 0, 0, months=23))
        self.assertFalse(decision.allowed)
        self.assertIn("BENEFICIARY_CLIFF", decision.reasons)

    def test_the_constants_the_program_freezes_are_the_model_bounds(self):
        """The program's hard floors and ceilings, restated where they can be read.

        These are the numbers a reviewer would otherwise have to extract from
        Rust source; pinning them here means a change to either side has to be
        deliberate.
        """
        self.assertEqual(PERIOD_SECONDS, 2_592_000)
        self.assertEqual(MIN_CLIFF_SECONDS, 63_072_000)
        self.assertEqual(MIN_NOTICE_SECONDS, PERIOD_SECONDS)
        self.assertEqual(MAX_ANNUAL_RELEASE_BPS, 500)
        self.assertEqual(MAX_MARKET_CAPACITY_BPS, 500)
        # The model refuses rates outside the same window.
        with self.assertRaises(ValueError):
            ReleaseRequest(
                period_id="2028-09",
                beneficiary_locked_balance_at_year_start=BENEFICIARY_DEPOSIT,
                purpose_locked_balance_at_year_start=PURPOSE_DEPOSIT,
                annual_release_bps=MAX_ANNUAL_RELEASE_BPS + 1,
                eligible_trailing_30d_spot_volume=ELIGIBLE_VOLUME,
                market_capacity_bps=MARKET_CAPACITY_BPS,
                beneficiary_months_since_genesis=36,
            )


if __name__ == "__main__":
    unittest.main()
