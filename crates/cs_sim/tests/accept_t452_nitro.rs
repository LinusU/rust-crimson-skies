//! Acceptance scenario T452 (task #2155): the measured charge-bounded nitro
//! rule, `NitroActivationRule::UntilCharge`.
//!
//! Task test prefix: `accept_t452_nitro_`. The numbers (start at 99% of
//! capacity, cut below 5%) are the ones measured statically from the engine
//! image in `docs/findings/2026-10-25-t452-03-nitro-activation-capacity-and-tradeoffs.md`.
//! The tests drive production code only: `NitroParameters` and `NitroLedger`.
//! Capacity and rates are synthetic round numbers, not original data.

use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    NitroActivationRule, NitroLedger, NitroParameters, NitroRefusal, NitroTradeoffs,
    OrdnanceDefinitionError,
};
use cs_types::Tick;
use cs_types::net::SessionId;

const START: f64 = 0.99;
const STOP: f64 = 0.05;

fn session() -> SessionId {
    SessionId::new(5).expect("a nonzero session generation")
}

fn rule() -> NitroActivationRule {
    NitroActivationRule::UntilCharge {
        start_min_fraction: START,
        stop_below_fraction: STOP,
    }
}

/// Capacity 100, 10 per tick at 10 Hz (100 units/s), no recovery.
fn parameters(recovery_per_s: f64) -> NitroParameters {
    NitroParameters::try_new(
        100.0,
        100.0,
        recovery_per_s,
        1000.0,
        rule(),
        NitroTradeoffs::UNMEASURED,
    )
    .expect("valid parameters")
}

fn ledger(recovery_per_s: f64) -> NitroLedger {
    NitroLedger::new(
        session(),
        Tick(0),
        TickRate::new(10).expect("rate"),
        parameters(recovery_per_s),
    )
}

#[test]
fn accept_t452_nitro_burn_continues_after_release_and_ends_below_five_percent() {
    let mut nitro = ledger(0.0);
    let started = nitro.request(session(), Tick(1), true).unwrap();
    assert!(started.active && started.refused.is_none());
    assert_eq!(started.consumed_units, 10.0);

    // The control is released: the burn keeps running.
    let mut tick = 1;
    let mut last_active = 1;
    while tick < 30 {
        tick += 1;
        let step = nitro.request(session(), Tick(tick), false).unwrap();
        if !step.active {
            break;
        }
        assert!(step.extra_thrust_n > 0.0);
        last_active = tick;
    }
    // 100 -> 90 -> ... -> 10 -> 0 at the tenth tick; the charge before the
    // eleventh is 0 (< 5%), so it is the first tick that does not run.
    assert_eq!(last_active, 10);
    assert!(!nitro.burn_running());
    assert_eq!(nitro.capacity_units(), 0.0);
}

#[test]
fn accept_t452_nitro_cut_is_at_the_first_tick_starting_below_five_percent() {
    // 12.5 per tick: 100, 87.5, ... 12.5, 0. Use 8 per tick at 10 Hz so the
    // charge steps 100, 92, ..., 4: the tick that starts at 12 runs (>= 5),
    // the one starting at 4 (< 5) does not.
    let params =
        NitroParameters::try_new(100.0, 80.0, 0.0, 1000.0, rule(), NitroTradeoffs::UNMEASURED)
            .unwrap();
    let mut nitro = NitroLedger::new(session(), Tick(0), TickRate::new(10).unwrap(), params);
    let mut ran = 0;
    for t in 1..=20 {
        let step = nitro.request(session(), Tick(t), t == 1).unwrap();
        if step.active {
            ran += 1;
        } else {
            break;
        }
    }
    // Starts at 100 - 8k; runs while >= 5: k = 0..=11 (100 - 88 = 12), k = 12 gives 4.
    assert_eq!(ran, 12);
    assert!((nitro.capacity_units() - 4.0).abs() < 1e-9);
}

#[test]
fn accept_t452_nitro_refusal_below_start_is_named_and_consumes_nothing() {
    // Recovery 98 units/s at 1 Hz refills 98 per idle tick, so after the
    // burn is cut the tank is at 98% (cut leaves 0 < 5, recovery adds 98).
    let params = NitroParameters::try_new(
        100.0,
        100.0,
        98.0,
        1000.0,
        rule(),
        NitroTradeoffs::UNMEASURED,
    )
    .unwrap();
    let mut nitro = NitroLedger::new(session(), Tick(0), TickRate::new(1).unwrap(), params);
    assert!(nitro.request(session(), Tick(1), true).unwrap().active);
    // The burn emptied the tank; the next tick cuts it and (not requesting)
    // recovers 98 units.
    let idle = nitro.request(session(), Tick(2), false).unwrap();
    assert!(!idle.active);
    assert!((nitro.capacity_fraction() - 0.98).abs() < 1e-12);

    let before = nitro.capacity_units();
    let refused = nitro.request(session(), Tick(3), true).unwrap();
    assert_eq!(refused.refused, Some(NitroRefusal::BelowStartCharge));
    assert!(!refused.active);
    assert_eq!(refused.consumed_units, 0.0);
    assert_eq!(refused.extra_thrust_n, 0.0);
    // Recovery on a refused tick keeps its own parameter (Recovers by
    // default): the only movement is upward, never a consumption.
    assert!(nitro.capacity_units() >= before);
}

#[test]
fn accept_t452_nitro_ten_ticks_walked_cost_what_ten_ticks_jumped_cost() {
    let mut walked = ledger(0.0);
    let mut cost = 0.0;
    for t in 1..=10 {
        cost += walked
            .request(session(), Tick(t), t == 1)
            .unwrap()
            .consumed_units;
    }
    let mut jumped = ledger(0.0);
    // One accepted start at tick 1, then a jump to tick 10.
    let first = jumped.request(session(), Tick(1), true).unwrap();
    let rest = jumped.request(session(), Tick(10), false).unwrap();
    assert!((first.consumed_units + rest.consumed_units - cost).abs() < 1e-9);
    assert!((walked.capacity_units() - jumped.capacity_units()).abs() < 1e-9);

    // A jump straight across the cut stops at the same charge as walking.
    let mut walk_long = ledger(0.0);
    for t in 1..=40 {
        let _ = walk_long.request(session(), Tick(t), t == 1).unwrap();
    }
    let mut jump_long = ledger(0.0);
    let _ = jump_long.request(session(), Tick(1), true).unwrap();
    let _ = jump_long.request(session(), Tick(40), false).unwrap();
    assert_eq!(walk_long.capacity_units(), jump_long.capacity_units());
    assert!(!jump_long.burn_running());
}

#[test]
fn accept_t452_nitro_rule_validates_its_fractions() {
    for (start, stop) in [
        (0.0, 0.0),
        (1.5, 0.05),
        (0.99, 0.99),
        (0.5, 0.7),
        (0.99, -0.1),
        (f64::NAN, 0.05),
        (0.99, f64::INFINITY),
    ] {
        let err = NitroParameters::try_new(
            10.0,
            1.0,
            0.0,
            1.0,
            NitroActivationRule::UntilCharge {
                start_min_fraction: start,
                stop_below_fraction: stop,
            },
            NitroTradeoffs::UNMEASURED,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            OrdnanceDefinitionError::InvalidNitroChargeFractions { .. }
        ));
    }
    assert!(NitroActivationRule::ALL.contains(&rule()));
    assert!(rule().label().starts_with("until_charge"));
}
