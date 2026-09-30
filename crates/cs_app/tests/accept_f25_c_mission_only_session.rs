//! Acceptance scenario F25-C: a mission-only aircraft is launched by a forced
//! assignment, controlled, damaged and unloaded without any shop registration.
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stage `### F25-C`. Task test prefix: `accept_f25_c_`. Everything here goes
//! through `cs_app::airframe_visual::MissionAircraftSession` over the F25-A
//! roster and the F25-B law; the roles and profile are synthetic fixtures, so
//! nothing here is evidence about the original game.

use cs_app::airframe_visual::{MissionAircraftSession, MissionSessionError};
use cs_content::airframe_roles::{
    Availability, ForcedAssignment, LaunchAssignmentError, LaunchSource, OwnedLoadout,
    declared_synthetic_roles,
};
use cs_sim::flight::{
    DamageState, EngineState, FlightEnvironment, FlightInput, FlightState, FlightTelemetry,
    SYNTHETIC_TICK_DT_S, synthetic_exceptional_profile, synthetic_exceptional_tuning,
    synthetic_fixed_wing,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance};
use cs_types::evidence::ClaimId;
use cs_types::space::Quaternion;

fn airframe(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Airframe, key).expect("a valid airframe id")
}

fn owned(generation: u64) -> OwnedLoadout {
    OwnedLoadout {
        airframe: airframe("fixture.synthetic-fixed-wing"),
        session_generation: generation,
    }
}

fn forced(key: &str, generation: u64) -> ForcedAssignment {
    ForcedAssignment {
        airframe: airframe(key),
        session_generation: generation,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(ClaimId::new("f25c.test.forced").expect("claim id")),
    }
}

fn launch(generation: u64) -> Result<MissionAircraftSession, MissionSessionError> {
    MissionAircraftSession::launch(
        &declared_synthetic_roles(),
        &owned(generation),
        Some(&forced("fixture.synthetic-autogyro", generation)),
        synthetic_exceptional_tuning(),
        synthetic_exceptional_profile(),
    )
}

fn cruise() -> FlightState {
    FlightState {
        linear_velocity_mps: [0.0, 0.0, -20.0],
        engine: EngineState::direct(0.8),
        ..FlightState::at_rest(Quaternion::IDENTITY)
    }
}

fn stick() -> FlightInput {
    FlightInput::try_new(0.3, 0.2, 0.0, 0.8, false).expect("legal input")
}

/// The whole lifecycle of a mission-only aircraft, with no shop in the loop.
#[test]
fn accept_f25_c_mission_only_aircraft_is_controlled_damaged_and_unloaded() {
    let roster = declared_synthetic_roles();
    let role = roster
        .role(&airframe("fixture.synthetic-autogyro"))
        .expect("the roster holds it");
    assert_eq!(role.availability, Availability::MissionOnly);
    assert!(!role.launch.hangar_selectable, "the shop cannot select it");

    let mut session = launch(7).expect("a forced mission-only launch is accepted");
    assert_eq!(
        session.launch_record().source,
        LaunchSource::ForcedMissionAssignment
    );
    assert!(!session.launch_record().persists_to_owned_loadout());
    assert_eq!(
        session.launch_record().airframe,
        airframe("fixture.synthetic-autogyro"),
        "the requested actor launches, not the garage plane"
    );

    // Controlled: the rotor spins up from airflow and the stick has authority.
    let env = FlightEnvironment::SEA_LEVEL;
    let state = cruise();
    let mut last_rotor = 0.0;
    let mut pristine_authority = 0.0;
    for tick in 1..=200_u64 {
        let produced = session
            .fly(&env, &state, &stick(), SYNTHETIC_TICK_DT_S, Tick(tick))
            .expect("a legal tick");
        pristine_authority = produced.diagnostics.control_authority;
        last_rotor = session.rotor().physical_speed_radps();
    }
    assert!(last_rotor > 0.0, "the session owns the advancing rotor");
    assert!(pristine_authority > 0.0);

    // Telemetry goes through the shared channel; no ratio is declared, so no
    // visual rate is invented.
    let produced = session
        .fly(&env, &state, &stick(), SYNTHETIC_TICK_DT_S, Tick(201))
        .expect("a legal tick");
    let frame = session
        .telemetry(&produced, &state, Tick(201))
        .expect("telemetry");
    assert!(frame.shared().airspeed_mps.is_finite());
    assert!(session.rotor_mapping().is_none());

    // Damaged: the authority reaches the control law on the next tick.
    let damage = DamageState {
        control_authority: 0.25,
        ..DamageState::PRISTINE
    };
    session.apply_damage(damage).expect("legal damage");
    let damaged = session
        .fly(&env, &state, &stick(), SYNTHETIC_TICK_DT_S, Tick(202))
        .expect("a damaged tick");
    assert!(
        damaged.diagnostics.control_authority < pristine_authority,
        "damage must reduce the control authority the law applies"
    );
    assert!(
        session
            .apply_damage(DamageState {
                lift_scale: 2.0,
                ..DamageState::PRISTINE
            })
            .is_err()
    );
    assert_eq!(session.damage(), &damage, "a refused record is not applied");

    // Unloaded: nothing further works, including a second unload.
    let record = session.unload().expect("first unload");
    assert!(!record.persists_to_owned_loadout());
    assert!(session.is_unloaded());
    assert_eq!(session.rotor().physical_speed_radps(), 0.0);
    assert!(matches!(
        session.fly(&env, &state, &stick(), SYNTHETIC_TICK_DT_S, Tick(203)),
        Err(MissionSessionError::Unloaded { .. })
    ));
    assert!(matches!(
        session.apply_damage(DamageState::PRISTINE),
        Err(MissionSessionError::Unloaded { .. })
    ));
    assert!(matches!(
        session.unload(),
        Err(MissionSessionError::Unloaded { .. })
    ));
}

/// Every refusal is named, never a fallback to the garage plane, and a failed
/// launch can be retried.
#[test]
fn accept_f25_c_launch_failures_are_named_and_retryable() {
    let roster = declared_synthetic_roles();
    let try_launch = |assignment: ForcedAssignment, profile_key: &str| {
        let mut profile = synthetic_exceptional_profile();
        profile.airframe_id = airframe(profile_key);
        MissionAircraftSession::launch(
            &roster,
            &owned(3),
            Some(&assignment),
            synthetic_exceptional_tuning(),
            profile,
        )
    };

    assert!(matches!(
        try_launch(
            forced("fixture.synthetic-autogyro", 2),
            "fixture.synthetic-autogyro"
        ),
        Err(MissionSessionError::Launch(
            LaunchAssignmentError::SessionGenerationMismatch { .. }
        ))
    ));
    assert!(matches!(
        try_launch(forced("fixture.missing", 3), "fixture.synthetic-autogyro"),
        Err(MissionSessionError::Launch(
            LaunchAssignmentError::UnknownAirframe { .. }
        ))
    ));
    // The roster resolves the fixed wing, but the exceptional law cannot fly it.
    assert!(matches!(
        try_launch(
            forced("fixture.synthetic-fixed-wing", 3),
            "fixture.synthetic-fixed-wing"
        ),
        Err(MissionSessionError::NotExceptional { .. })
    ));
    // A profile of another airframe would fly the wrong handling.
    assert!(matches!(
        try_launch(forced("fixture.synthetic-autogyro", 3), "fixture.other"),
        Err(MissionSessionError::ProfileAirframeMismatch { .. })
    ));
    // A fixed-wing tuning is refused by the law.
    assert!(matches!(
        MissionAircraftSession::launch(
            &roster,
            &owned(3),
            Some(&forced("fixture.synthetic-autogyro", 3)),
            synthetic_fixed_wing(),
            synthetic_exceptional_profile(),
        ),
        Err(MissionSessionError::Law(_))
    ));
    // The corrected retry succeeds.
    assert!(
        try_launch(
            forced("fixture.synthetic-autogyro", 3),
            "fixture.synthetic-autogyro"
        )
        .is_ok()
    );
}

/// A refused tick leaves the session's rotor where it was, so the same tick
/// can be retried.
#[test]
fn accept_f25_c_a_refused_tick_does_not_advance_the_session() {
    let mut session = launch(1).expect("launch");
    let env = FlightEnvironment::SEA_LEVEL;
    let state = cruise();
    session
        .fly(&env, &state, &stick(), SYNTHETIC_TICK_DT_S, Tick(5))
        .expect("first tick");
    let before = *session.rotor();
    // A stale tick is refused and changes nothing.
    assert!(
        session
            .fly(&env, &state, &stick(), SYNTHETIC_TICK_DT_S, Tick(5))
            .is_err()
    );
    assert_eq!(*session.rotor(), before);
    session
        .fly(&env, &state, &stick(), SYNTHETIC_TICK_DT_S, Tick(6))
        .expect("the next tick still works");
}
