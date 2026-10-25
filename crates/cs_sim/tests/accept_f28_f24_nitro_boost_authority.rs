//! Task #453: the declared nitro record is the one authority for an
//! airframe's boost terms; the flight model's `BoostParameters` are lowered
//! from it. Task test prefix: `accept_f28_f24_nitro_boost_`.
//!
//! Drives production code only: `NitroParameters::boost_parameters`,
//! `NitroLedger::request` and `FlightModel::compute`. Synthetic data.

use cs_sim::flight::{
    DamageState, EngineState, FlightEnvironment, FlightInput, FlightModel, FlightState,
    LoadoutMass, synthetic_fixed_wing,
};
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{NitroLedger, NitroRefusedRecovery, synthetic_nitro_parameters};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::Quaternion;

const TPS: u32 = 30;

fn session() -> SessionId {
    SessionId::new(5).expect("nonzero")
}

fn rate() -> TickRate {
    TickRate::new(TPS).expect("valid rate")
}

fn flight_tick(model: &FlightModel, available: bool, boost: bool) -> cs_sim::flight::FlightOutput {
    let state = FlightState {
        linear_velocity_mps: [0.0, 0.0, -50.0],
        engine: EngineState::direct(0.5),
        boost_available: available,
        ..FlightState::at_rest(Quaternion::IDENTITY)
    };
    let input = FlightInput::try_new(0.0, 0.0, 0.0, 0.5, boost).expect("valid input");
    model
        .compute(
            &FlightEnvironment::SEA_LEVEL,
            &LoadoutMass {
                fuel_kg: 0.0,
                ordnance_kg: 0.0,
                armor_kg: 0.0,
            },
            &DamageState::PRISTINE,
            &state,
            &input,
            rate().dt_seconds(),
        )
        .expect("valid")
}

fn lowered_model() -> (FlightModel, NitroLedger) {
    // Withheld recovery keeps a refused tick's capacity exactly where it was,
    // so "consumes nothing" is observable as an unchanged capacity.
    let parameters =
        synthetic_nitro_parameters().with_recovery_on_refused(NitroRefusedRecovery::Withheld);
    let mut tuning = synthetic_fixed_wing();
    tuning.boost = parameters.boost_parameters();
    (
        FlightModel::new(tuning),
        NitroLedger::new(session(), Tick(0), rate(), parameters),
    )
}

#[test]
fn accept_f28_f24_nitro_boost_lowering_carries_the_declared_terms() {
    let parameters = synthetic_nitro_parameters();
    let boost = parameters.boost_parameters();
    assert_eq!(boost.thrust_n, parameters.extra_thrust_n());
    assert_eq!(boost.consumption_per_s, parameters.consumption_per_s());
    assert!(boost.validate().is_ok());
}

/// An accepted activation raises thrust and consumes capacity, the ledger and
/// the flight model agree on both numbers, and neither record carries a pose
/// or a wall-clock duration (`step` takes only the fixed tick `dt`).
#[test]
fn accept_f28_f24_nitro_boost_accepted_activation_changes_thrust_and_consumption() {
    let (model, mut ledger) = lowered_model();
    let idle = flight_tick(&model, ledger.boost_available(), false);
    let tick = ledger.request(session(), Tick(1), true).expect("request");
    assert!(tick.active && tick.refused.is_none());
    let boosted = flight_tick(&model, ledger.boost_available(), tick.active);

    assert_eq!(idle.accepted_boost_consumption, 0.0);
    let thrust_gain = boosted.diagnostics.thrust_n - idle.diagnostics.thrust_n;
    assert!((thrust_gain - tick.extra_thrust_n).abs() < 1e-9);
    assert!(tick.extra_thrust_n > 0.0);
    assert!((boosted.accepted_boost_consumption - tick.consumed_units).abs() < 1e-12);
    assert!(tick.consumed_units > 0.0);
    // Only thrust moved: lift, drag and gravity are the same forces.
    assert_eq!(boosted.diagnostics.lift_n, idle.diagnostics.lift_n);
    assert_eq!(boosted.diagnostics.drag_n, idle.diagnostics.drag_n);
    assert_eq!(
        boosted.diagnostics.gravity_force_n,
        idle.diagnostics.gravity_force_n
    );
}

/// A refusal on an empty tank consumes nothing and adds no thrust.
#[test]
fn accept_f28_f24_nitro_boost_refusal_consumes_nothing_and_adds_no_thrust() {
    let (model, mut ledger) = lowered_model();
    let mut n = 0;
    while ledger.boost_available() {
        n += 1;
        ledger.request(session(), Tick(n), true).expect("request");
        assert!(n < 10_000, "the tank must drain");
    }
    let before = ledger.capacity_units();
    let idle = flight_tick(&model, false, false);
    let tick = ledger
        .request(session(), Tick(n + 1), true)
        .expect("request");
    assert!(tick.is_refused() && !tick.is_active());
    assert_eq!(tick.extra_thrust_n, 0.0);
    assert_eq!(tick.consumed_units, 0.0);
    assert_eq!(ledger.capacity_units(), before);
    let refused = flight_tick(&model, ledger.boost_available(), tick.active);
    assert_eq!(refused.accepted_boost_consumption, 0.0);
    assert_eq!(refused.diagnostics.thrust_n, idle.diagnostics.thrust_n);
}
