//! F35-A acceptance: the capital-ship subsystem, propulsion, bay, launch and
//! capture contracts (synthetic).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Ordinary build/test only; nothing here is original
//! data.
//!
//! The F35-A minimum scenario is
//! [`accept_f35_a_disabling_engines_reduces_motion_without_destroying_the_hull`]:
//! engines are disabled and the measured motion response changes while the
//! hull is not destroyed automatically. Every test drives production code,
//! so removing a validator, a transition or the thrust sum fails to compile
//! or fails an assertion.

use cs_script::ir::ActorId;
use cs_sim::capital::{
    Bay, BayKind, BayState, CapitalError, CapitalParts, CapitalShip, CaptureRefusal, CaptureStage,
    CaptureTransaction, EngineSpec, ExposureError, ExposureWindow, LaunchLedger, LaunchRefusal,
    Ownership, PropulsionError, Subsystem, SubsystemEffect, SubsystemGraph, SubsystemGraphError,
    SubsystemKey, SubsystemKeyError, SubsystemKind, SubsystemState, synthetic_capital_bays,
    synthetic_capital_docking_anchors, synthetic_capital_engines, synthetic_capital_graph,
    synthetic_capital_ownership, synthetic_capital_ship, synthetic_capital_trajectory,
    synthetic_capital_turrets, synthetic_launch_socket,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn key(name: &str) -> SubsystemKey {
    SubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f35a.test").expect("valid")),
    ))
}

fn unknown_f64(claim: &str) -> Resolved<f64> {
    Resolved::Unknown {
        claim_id: ClaimId::new(claim).expect("valid"),
        reason: "unmeasured".to_owned(),
    }
}

fn fighter() -> ContentId {
    ContentId::from_source(ContentKind::Airframe, "synthetic.fighter").expect("valid")
}

/// Builds a ship with the synthetic graph but caller-supplied parts; the
/// graph's own details are reused so only the field under test differs.
fn ship_with(
    engines: Vec<EngineSpec>,
    bays: Vec<Bay>,
    cargo: Resolved<f64>,
) -> Result<CapitalShip, CapitalError> {
    CapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        ActorId(1),
        CapitalParts {
            graph: synthetic_capital_graph(),
            engines,
            bays,
            turrets: synthetic_capital_turrets(),
            docking_anchors: synthetic_capital_docking_anchors(),
            cargo,
            ownership: synthetic_capital_ownership(),
            trajectory: Some(synthetic_capital_trajectory()),
        },
    )
}

// -------------------------------------------------- AC01 minimum scenario ---

/// The minimum scenario: with both engines intact the ship produces forward
/// acceleration; disabling one halves it; disabling both removes it. At no
/// point is the hull destroyed or its state changed by the engine loss.
#[test]
fn accept_f35_a_disabling_engines_reduces_motion_without_destroying_the_hull() {
    let mut ship = synthetic_capital_ship();
    assert!(!ship.is_destroyed());
    assert_eq!(
        ship.subsystem_state(&key("keel")),
        Some(SubsystemState::Intact)
    );

    // Two 400 kN engines on a 200 t hull: 4 m/s^2 of body-frame acceleration.
    let intact = ship
        .propulsive_acceleration_m_s2(1.0, cs_sim::capital::SYNTHETIC_MASS_KG)
        .expect("intact engines compute");
    assert!((intact[0] - 4.0).abs() < 1e-9, "got {intact:?}");
    assert!((intact[1]).abs() < 1e-12 && (intact[2]).abs() < 1e-12);

    // Disable one engine: the motion response drops, the hull does not.
    let outcome = ship.disable(&key("engine_1")).expect("known engine");
    assert!(outcome.changed);
    assert!(!outcome.actor_destroyed);
    assert_eq!(outcome.effect, Some(SubsystemEffect::Propulsion));
    let one = ship
        .propulsive_acceleration_m_s2(1.0, cs_sim::capital::SYNTHETIC_MASS_KG)
        .expect("one engine computes");
    assert!((one[0] - 2.0).abs() < 1e-9, "got {one:?}");
    assert_ne!(one, intact);
    assert!(!ship.is_destroyed());

    // Destroy the second engine: propulsion is gone, the hull is intact.
    let outcome = ship.disable(&key("engine_2")).expect("known engine");
    assert!(outcome.changed && !outcome.actor_destroyed);
    let none = ship
        .propulsive_acceleration_m_s2(1.0, cs_sim::capital::SYNTHETIC_MASS_KG)
        .expect("no engine computes zero");
    assert_eq!(none, [0.0; 3]);
    assert_eq!(ship.engine_thrust_n().expect("zero thrust"), 0.0);
    assert!(!ship.is_destroyed());
    assert_eq!(
        ship.subsystem_state(&key("keel")),
        Some(SubsystemState::Intact)
    );
    assert_eq!(
        ship.subsystem_state(&key("engine_1")),
        Some(SubsystemState::Disabled)
    );

    // Re-disabling is idempotent and applies nothing twice.
    let repeat = ship.disable(&key("engine_1")).expect("known engine");
    assert!(!repeat.changed && repeat.effect.is_none());

    // An unknown part is refused, never silently ignored.
    assert_eq!(
        ship.disable(&key("ghost")),
        Err(SubsystemGraphError::UnknownSubsystem(key("ghost")))
    );
}

/// A lethal subsystem destroys the actor; a non-lethal one never does.
#[test]
fn accept_f35_a_only_a_lethal_subsystem_destroys_the_ship() {
    let mut ship = synthetic_capital_ship();
    let bay = ship.disable(&key("weapon_bay_1")).expect("known bay");
    assert!(bay.changed && !bay.actor_destroyed);
    assert_eq!(bay.effect, Some(SubsystemEffect::WeaponAccess));
    assert!(!ship.is_destroyed());

    let keel = ship.disable(&key("keel")).expect("known keel");
    assert!(keel.changed && keel.actor_destroyed);
    assert_eq!(keel.effect, Some(SubsystemEffect::MissionCondition));
    assert!(ship.is_destroyed());

    // The gas cell is the other lethal kind.
    let mut other = synthetic_capital_ship();
    let gas = other.disable(&key("gas_cell_1")).expect("known gas cell");
    assert!(gas.actor_destroyed);
    assert_eq!(gas.effect, Some(SubsystemEffect::Vulnerability));
}

// --------------------------------------------------------------- bays ------

/// Before, during and after the exposure window a bay is not always a
/// weakpoint: only the fully open phase counts.
#[test]
fn accept_f35_a_a_bay_is_a_weakpoint_only_while_exposed() {
    // Cycle 40 concealed + 10 opening + 60 exposed + 10 closing = 120.
    let mut ship = synthetic_capital_ship();
    let bay = key("weapon_bay_1");
    assert_eq!(ship.bay_state(&bay, Tick(0)), Some(BayState::Concealed));
    assert_ne!(ship.bay_state(&bay, Tick(39)), Some(BayState::Exposed));

    // Before: concealed. During the opening frames: not yet a weakpoint.
    assert_eq!(ship.bay_state(&bay, Tick(45)), Some(BayState::Opening));
    // During: exposed.
    assert_eq!(ship.bay_state(&bay, Tick(50)), Some(BayState::Exposed));
    assert_eq!(ship.bay_state(&bay, Tick(109)), Some(BayState::Exposed));
    // After: closing, then concealed again.
    assert_eq!(ship.bay_state(&bay, Tick(110)), Some(BayState::Closing));
    assert_eq!(ship.bay_state(&bay, Tick(120)), Some(BayState::Concealed));

    // The window is periodic.
    assert_eq!(ship.bay_state(&bay, Tick(240)), Some(BayState::Concealed));
    assert_eq!(ship.bay_state(&bay, Tick(170)), Some(BayState::Exposed));

    // A destroyed bay is never a weakpoint at any tick.
    ship.disable(&bay).expect("known bay");
    for tick in [0_u64, 45, 50, 110] {
        assert_eq!(ship.bay_state(&bay, Tick(tick)), Some(BayState::Destroyed));
    }
}

/// A degenerate window is refused: a bay that never opens is not a bay.
#[test]
fn accept_f35_a_exposure_window_rejects_degenerate_cycles() {
    assert_eq!(
        ExposureWindow::try_new(0, 0, 0, 0),
        Err(ExposureError::ZeroCycle)
    );
    assert_eq!(
        ExposureWindow::try_new(5, 0, 0, 0),
        Err(ExposureError::NoExposedTicks)
    );
    let window = ExposureWindow::try_new(0, 0, 1, 0).expect("a one-tick window is valid");
    assert_eq!(window.cycle_ticks(), 1);
    assert!(window.is_weakpoint(Tick(0)));
}

// ------------------------------------------------------------- launch ------

/// A launch is released at most once, and destroying its bay cancels the
/// pending launch instead of deferring it — no duplicate aircraft.
#[test]
fn accept_f35_a_launch_ledger_releases_once_and_cancels_a_destroyed_bay() {
    let bay = key("launch_bay_1");
    let open = |_: &SubsystemKey| BayState::Exposed;
    let destroyed = |_: &SubsystemKey| BayState::Destroyed;
    let mut ledger = LaunchLedger::new();

    let first = ledger.schedule(bay.clone(), fighter(), Tick(10));
    // Not ready yet.
    let early = ledger.release_ready(Tick(9), open);
    assert!(early.released.is_empty());
    // Ready and open: released exactly once.
    let ready = ledger.release_ready(Tick(10), open);
    assert_eq!(ready.released.len(), 1);
    assert_eq!(ready.released[0].id, first);
    assert!(ledger.was_released(&first));
    // Ids are stable and not reused.
    let second = ledger.schedule(bay.clone(), fighter(), Tick(12));
    assert_ne!(first, second);

    // A destroyed bay cancels the pending launch; it never spawns later.
    let cancelled = ledger.release_ready(Tick(20), destroyed);
    assert_eq!(cancelled.cancelled.len(), 1);
    assert_eq!(cancelled.cancelled[0].id, second);
    assert!(cancelled.released.is_empty());
    let after = ledger.release_ready(Tick(30), open);
    assert!(after.released.is_empty());
    assert!(after.cancelled.is_empty());

    // The refusal reasons match the state.
    let third = ledger.schedule(bay.clone(), fighter(), Tick(40));
    assert_eq!(
        ledger.check(&third, Tick(39), open),
        Err(LaunchRefusal::NotReady {
            ready_tick: Tick(40)
        })
    );
    assert_eq!(
        ledger.check(&third, Tick(40), |_| BayState::Concealed),
        Err(LaunchRefusal::NotOpen)
    );
    assert_eq!(
        ledger.check(&third, Tick(40), destroyed),
        Err(LaunchRefusal::BayDestroyed)
    );
    assert_eq!(ledger.check(&third, Tick(40), open), Ok(()));
}

/// A released aircraft starts at the socket with the carrier's velocity plus
/// the ejection and gains dynamic authority once.
#[test]
fn accept_f35_a_released_aircraft_inherits_carrier_motion_and_authority() {
    let ship = synthetic_capital_ship();
    let trajectory = ship.trajectory().expect("the fixture has a trajectory");
    let tick = Tick(100); // 20 m/s along +X
    let pose = trajectory.sample(tick);
    let socket = synthetic_launch_socket();
    let released =
        cs_sim::capital::release_aircraft(tick, &pose, &socket, ActorId(77), [0.0, 0.0, 2.0]);
    assert_eq!(released.actor, ActorId(77));
    assert_ne!(released.actor, ship.actor());
    assert_eq!(released.position_m, [200.0, -5.0, 0.0]);
    assert_eq!(released.velocity_m_s, [20.0, 0.0, 2.0]);
    assert!(released.dynamic_authority);
}

// ------------------------------------------------------------ capture ------

/// Capture is a staged transaction: ownership switches exactly at
/// completion, and a terminal transaction cannot advance or abort again.
#[test]
fn accept_f35_a_capture_is_staged_and_switches_ownership_on_completion() {
    let raiders = ContentId::from_source(ContentKind::Faction, "synthetic.raiders").expect("valid");
    let navy = ContentId::from_source(ContentKind::Faction, "synthetic.navy").expect("valid");
    let mut capture =
        CaptureTransaction::begin(7, ActorId(1), ActorId(2), raiders.clone(), navy.clone());
    assert_eq!(capture.stage(), CaptureStage::Approaching);
    assert_eq!(capture.ownership(), None);

    assert_eq!(capture.advance(), Ok(CaptureStage::Eligible));
    assert_eq!(capture.advance(), Ok(CaptureStage::Latching));
    assert_eq!(capture.advance(), Ok(CaptureStage::Transferring));
    assert_eq!(capture.ownership(), None);
    assert_eq!(capture.advance(), Ok(CaptureStage::Completed));

    let ownership: Ownership = capture.ownership().expect("completed captures");
    assert_eq!(ownership.owner, navy);
    assert!(ownership.captured);
    assert_eq!(capture.advance(), Err(CaptureRefusal::AlreadyTerminal));
    assert_eq!(capture.abort(), Err(CaptureRefusal::AlreadyTerminal));
}

/// An aborted capture never transfers ownership and cannot resume.
#[test]
fn accept_f35_a_aborted_capture_never_transfers_ownership() {
    let raiders = ContentId::from_source(ContentKind::Faction, "synthetic.raiders").expect("valid");
    let navy = ContentId::from_source(ContentKind::Faction, "synthetic.navy").expect("valid");
    let mut capture = CaptureTransaction::begin(7, ActorId(1), ActorId(2), raiders, navy);
    assert_eq!(capture.advance(), Ok(CaptureStage::Eligible));
    assert_eq!(capture.abort(), Ok(CaptureStage::Aborted));
    assert_eq!(capture.ownership(), None);
    assert_eq!(capture.advance(), Err(CaptureRefusal::Aborted));
    assert_eq!(capture.abort(), Err(CaptureRefusal::Aborted));
}

// ------------------------------------------------------------ schema -------

/// The graph refuses an empty part set, duplicate keys, disallowed effects
/// and lethal non-structural parts.
#[test]
fn accept_f35_a_graph_refuses_malformed_subsystems() {
    assert_eq!(
        SubsystemGraph::try_new(vec![]),
        Err(SubsystemGraphError::Empty)
    );
    assert_eq!(
        SubsystemGraph::try_new(vec![
            Subsystem::new(key("engine_1"), SubsystemKind::Engine),
            Subsystem::new(key("engine_1"), SubsystemKind::Engine),
        ]),
        Err(SubsystemGraphError::DuplicateKey {
            key: key("engine_1")
        })
    );
    assert_eq!(
        SubsystemGraph::try_new(vec![
            Subsystem::new(key("engine_1"), SubsystemKind::Engine)
                .with_effect(SubsystemEffect::Launching),
        ]),
        Err(SubsystemGraphError::EffectKindMismatch {
            key: key("engine_1"),
            kind: SubsystemKind::Engine,
            effect: SubsystemEffect::Launching,
        })
    );
    assert_eq!(
        SubsystemGraph::try_new(vec![
            Subsystem::new(key("engine_1"), SubsystemKind::Engine).with_lethal(true),
        ]),
        Err(SubsystemGraphError::LethalKind {
            key: key("engine_1"),
            kind: SubsystemKind::Engine,
        })
    );
}

/// Engine records refuse zero/NaN axes and negative thrusts; a known
/// throttle is required; a non-positive mass is refused.
#[test]
fn accept_f35_a_engine_records_refuse_bad_values() {
    assert_eq!(
        EngineSpec::try_new(key("e"), designed(1.0), [0.0, 0.0, 0.0]),
        Err(PropulsionError::ZeroAxis { key: key("e") })
    );
    assert_eq!(
        EngineSpec::try_new(key("e"), designed(1.0), [f64::NAN, 0.0, 0.0]),
        Err(PropulsionError::NonFiniteAxis { key: key("e") })
    );
    assert_eq!(
        EngineSpec::try_new(key("e"), designed(-1.0), [1.0, 0.0, 0.0]),
        Err(PropulsionError::NegativeThrust {
            key: key("e"),
            value: -1.0,
        })
    );
    assert_eq!(
        EngineSpec::try_new(key("e"), designed(f64::INFINITY), [1.0, 0.0, 0.0]),
        Err(PropulsionError::NonFiniteThrust { key: key("e") })
    );

    let engine =
        EngineSpec::try_new(key("e"), designed(100.0), [2.0, 0.0, 0.0]).expect("valid engine");
    // The axis is normalized.
    assert_eq!(engine.axis, [1.0, 0.0, 0.0]);
    assert_eq!(engine.force_n(0.5).expect("finite"), [50.0, 0.0, 0.0]);
    assert_eq!(
        engine.force_n(f64::NAN),
        Err(PropulsionError::NonFiniteThrottle { key: key("e") })
    );

    assert_eq!(
        cs_sim::capital::acceleration_m_s2([1.0, 0.0, 0.0], 0.0),
        Err(PropulsionError::NonPositiveMass { mass_kg: 0.0 })
    );
    assert_eq!(
        cs_sim::capital::acceleration_m_s2([f64::NAN, 0.0, 0.0], 1.0),
        Err(PropulsionError::NonFiniteForce)
    );
}

/// An engine whose thrust is unresolved refuses to sum instead of treating
/// it as zero — the whole total is refused, never approximated.
#[test]
fn accept_f35_a_unknown_thrust_is_refused_not_treated_as_zero() {
    let engines = vec![
        EngineSpec::try_new(key("engine_1"), designed(100.0), [1.0, 0.0, 0.0])
            .expect("valid engine"),
        EngineSpec::try_new(
            key("engine_2"),
            unknown_f64("f35a.test.unknown"),
            [1.0, 0.0, 0.0],
        )
        .expect("an unknown thrust is a valid record"),
    ];
    let ship = ship_with(engines, synthetic_capital_bays(), designed(1000.0)).expect("valid ship");
    assert_eq!(
        ship.engine_thrust_n(),
        Err(PropulsionError::UnknownThrust {
            key: key("engine_2")
        })
    );
    assert!(ship.propulsive_force_n(1.0).is_err());

    // Once the unresolved engine is destroyed, the remainder sums cleanly.
    let mut ship = ship;
    ship.disable(&key("engine_2")).expect("known engine");
    assert_eq!(ship.engine_thrust_n().expect("one known engine"), 100.0);
}

/// Ship construction refuses mistyped, unknown and duplicate parts and a
/// negative cargo capacity.
#[test]
fn accept_f35_a_ship_construction_refuses_mistyped_parts() {
    // An engine key that names a weapon bay.
    assert_eq!(
        ship_with(
            vec![
                EngineSpec::try_new(key("weapon_bay_1"), designed(1.0), [1.0, 0.0, 0.0])
                    .expect("valid engine record"),
            ],
            vec![],
            designed(1.0),
        )
        .err(),
        Some(CapitalError::PartKindMismatch {
            key: key("weapon_bay_1"),
            expected: SubsystemKind::Engine,
            actual: SubsystemKind::WeaponBay,
        })
    );

    // A key that is not a subsystem at all.
    assert_eq!(
        ship_with(
            vec![
                EngineSpec::try_new(key("ghost"), designed(1.0), [1.0, 0.0, 0.0])
                    .expect("valid engine record"),
            ],
            vec![],
            designed(1.0),
        )
        .err(),
        Some(CapitalError::UnknownPart {
            kind: SubsystemKind::Engine,
            key: key("ghost"),
        })
    );

    // Two engines with one key.
    let dup = |name: &str| {
        EngineSpec::try_new(key(name), designed(1.0), [1.0, 0.0, 0.0]).expect("valid engine")
    };
    assert_eq!(
        ship_with(
            vec![dup("engine_1"), dup("engine_1")],
            vec![],
            designed(1.0),
        )
        .err(),
        Some(CapitalError::DuplicatePart {
            kind: SubsystemKind::Engine,
            key: key("engine_1"),
        })
    );

    // A negative cargo capacity.
    assert_eq!(
        ship_with(synthetic_capital_engines(), vec![], designed(-1.0)).err(),
        Some(CapitalError::NegativeCargo { value: -1.0 })
    );

    // A launch bay whose record is built as a weapon bay is refused: the bay
    // record's own kind is checked against the subsystem.
    let launch = Bay::new(
        key("launch_bay_1"),
        BayKind::Weapon,
        ExposureWindow::try_new(1, 0, 1, 0).expect("valid window"),
    );
    assert_eq!(
        ship_with(synthetic_capital_engines(), vec![launch], designed(1.0)).err(),
        Some(CapitalError::PartKindMismatch {
            key: key("launch_bay_1"),
            expected: SubsystemKind::WeaponBay,
            actual: SubsystemKind::LaunchBay,
        })
    );
}

/// The subsystem key grammar is the shared content-id discipline.
#[test]
fn accept_f35_a_subsystem_key_grammar_matches_content_id_discipline() {
    assert_eq!(SubsystemKey::new(""), Err(SubsystemKeyError::Empty));
    assert_eq!(
        SubsystemKey::new("UPPER_CASE"),
        Ok(key("upper_case")),
        "uppercase folds like ContentId"
    );
    assert!(matches!(
        SubsystemKey::new("a/b"),
        Err(SubsystemKeyError::BadCharacter { ch: '/' })
    ));
    assert!(matches!(
        SubsystemKey::new("..."),
        Err(SubsystemKeyError::NoAlphanumeric)
    ));
    assert!(matches!(
        SubsystemKey::new(&"x".repeat(129)),
        Err(SubsystemKeyError::TooLong { len: 129 })
    ));
}
