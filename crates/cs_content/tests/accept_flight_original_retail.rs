//! Acceptance tests for task #796: the retail import of the original flight
//! law's per-airframe parameters, and the headless probe that flies them
//! through the production step.
//!
//! Test prefix: `accept_flight_original_`. The tests that read
//! `$CS_GAME_DIR` are `#[ignore = "requires CS_GAME_DIR"]`; they fail loudly
//! without it and are run by the implementer and the reviewer with
//! `--include-ignored`. The constants in the first test are the values the
//! task lists for `pbloodhawk` and `pdevastator`, read from the installation
//! read-only.
//!
//! The probe mirrors the original dev tool `0x491c60`: attitude held, full
//! throttle, `dt = 0.01`, run until the speed stops changing.

use std::path::PathBuf;

use cs_content::original_airframe::{
    AIRFRAME_FIELDS, GLOBAL_FIELDS, OriginalAirframeParameters, OriginalDocuments,
    OriginalGlobalParameters, UNUSED_GLOBAL_FIELDS,
};
use cs_sim::flight::{
    DynamicsKind, OriginalAirframe, OriginalFlightModel, OriginalGlobals, OriginalInput,
    OriginalState, nose_direction,
};
use cs_types::space::{Quaternion, Radians, UnitVec3};

/// The environment the retail tests need.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set"))
}

/// The three members, read once for the whole test binary.
fn load() -> &'static OriginalDocuments {
    static DOCUMENTS: std::sync::OnceLock<OriginalDocuments> = std::sync::OnceLock::new();
    DOCUMENTS.get_or_init(|| {
        OriginalDocuments::read(&game_dir()).expect("the installation's zrdr.zbd reads")
    })
}

fn norm(vector: [f64; 3]) -> f64 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

/// A level attitude: the nose is `-Z`, which is `Quaternion::IDENTITY`.
fn level() -> Quaternion {
    Quaternion::IDENTITY
}

/// A vertical climb attitude: pitch `+90` about body `X` points the nose up.
fn nose_up() -> Quaternion {
    let axis = UnitVec3::try_new([1.0, 0.0, 0.0]).expect("the body X axis is a unit vector");
    Quaternion::from_axis_angle(axis, Radians(std::f64::consts::FRAC_PI_2))
        .expect("a quarter turn is a valid rotation")
}

fn globals(documents: &OriginalDocuments) -> OriginalGlobals {
    let parameters: OriginalGlobalParameters = documents.globals().expect("the globals import");
    OriginalGlobals::from_values(&parameters.law_values()).expect("the globals are complete")
}

fn model_for(
    documents: &OriginalDocuments,
    globals: &OriginalGlobals,
    record: &str,
) -> (OriginalAirframeParameters, OriginalFlightModel) {
    let parameters = documents.airframe(record).expect("the record imports");
    let airframe = OriginalAirframe::from_values(&parameters.field_values())
        .expect("the parameters are complete");
    let dynamics = if parameters.is_autogyro {
        DynamicsKind::Fake
    } else {
        DynamicsKind::Full
    };
    let model = OriginalFlightModel {
        airframe,
        globals: globals.clone(),
        dynamics,
    };
    (parameters, model)
}

/// The dev-tool probe: attitude held, full throttle, until the speed stops
/// changing. Returns the settled speed.
fn held_top_speed(model: &OriginalFlightModel, altitude_m: f64, fuel: f64) -> f64 {
    let mut state = OriginalState::at(altitude_m, level(), 1.0, fuel);
    let input = OriginalInput {
        throttle: 1.0,
        is_player: true,
        hold_attitude: true,
        ..OriginalInput::default()
    };
    let mut previous = norm(state.velocity_mps);
    let mut settled = 0;
    for _ in 0..40_000 {
        model
            .step(&mut state, input, 0.01)
            .expect("the production step runs");
        let speed = norm(state.velocity_mps);
        if (speed - previous).abs() < 1.0e-5 {
            settled += 1;
        } else {
            settled = 0;
        }
        previous = speed;
        if settled >= 50 {
            return speed;
        }
    }
    panic!("the held run never settled");
}

/// The dev-tool probe in a vertical climb: returns
/// `(initial vertical acceleration, settled climb speed, altitude gained)`.
fn vertical_climb(
    model: &OriginalFlightModel,
    altitude_m: f64,
    initial_speed_mps: f64,
    fuel: f64,
    settle_epsilon: f64,
) -> (f64, f64, f64) {
    let mut state = OriginalState::at(altitude_m, nose_up(), 1.0, fuel);
    state.velocity_mps = [0.0, initial_speed_mps, 0.0];
    let input = OriginalInput {
        throttle: 1.0,
        is_player: true,
        hold_attitude: true,
        ..OriginalInput::default()
    };
    let first = model
        .step(&mut state, input, 0.01)
        .expect("the production step runs");
    let initial_acceleration = first.linear_acceleration_mps2[1];

    let mut previous = norm(state.velocity_mps);
    let mut settled = 0;
    for _ in 0..40_000 {
        model
            .step(&mut state, input, 0.01)
            .expect("the production step runs");
        let speed = norm(state.velocity_mps);
        if (speed - previous).abs() < settle_epsilon {
            settled += 1;
        } else {
            settled = 0;
        }
        previous = speed;
        if settled >= 50 {
            let gained = state.position_m[1] - altitude_m;
            let climb_speed = state.velocity_mps[1];
            return (initial_acceleration, climb_speed, gained);
        }
    }
    panic!("the climb never settled");
}

/// The two crates declare the same flat vocabulary, so a rename on either
/// side cannot silently drop a value at the boundary. This one needs no
/// installation: it compares the two `const` lists.
#[test]
fn accept_flight_original_field_vocabulary_matches_between_the_two_crates() {
    assert_eq!(
        AIRFRAME_FIELDS,
        cs_sim::flight::original::AIRFRAME_FIELDS,
        "the importer and the law must consume the same airframe fields"
    );
    assert_eq!(
        GLOBAL_FIELDS,
        cs_sim::flight::original::GLOBAL_FIELDS,
        "the importer and the law must consume the same global fields"
    );
    assert!(!UNUSED_GLOBAL_FIELDS.is_empty());
}

/// **The retail table the task lists, read from the installation**
/// (retail): `pbloodhawk` and `pdevastator` import every value, the engine
/// factors come from `engines.zrd`, gravity is `player.zrd`'s `nom_gravity`
/// and the two `player.zrd` keys the law never reads are recorded as unused.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flight_original_retail_bloodhawk_and_devastator_match_the_table() {
    let documents = load();
    let globals_parameters = documents.globals().expect("the globals import");

    // The globals, from the first `player.zrd` directory entry.
    assert_eq!(
        globals_parameters.value("nom_gravity").map(|v| v.value),
        Some(20.0)
    );
    let unused_drag = globals_parameters
        .unused_value("drag_factor")
        .expect("`player.zrd` states drag_factor");
    let unused_fade = globals_parameters
        .unused_value("drag_fade_speed")
        .expect("`player.zrd` states drag_fade_speed");
    assert_eq!(unused_drag.value, 1.5);
    assert_eq!(unused_fade.value, 40.0);
    assert_eq!(unused_drag.key_path, "drag_factor");
    assert_eq!(unused_fade.key_path, "drag_fade_speed");
    assert_eq!(
        unused_drag
            .provenance
            .source
            .as_ref()
            .map(|source| source.member_key()),
        Some(Some("player.zrd")),
        "an unused key still records where it was read from"
    );

    for field in GLOBAL_FIELDS {
        let value = globals_parameters
            .value(field)
            .unwrap_or_else(|| panic!("the globals state {field}"));
        assert!(value.value.is_finite(), "{field} must be finite");
        assert!(
            value.provenance.source.is_some(),
            "{field} must have a span"
        );
    }

    let check = |record: &str,
                 engine_id: u32,
                 engine_name: &str,
                 engine_factor: f64,
                 pitch_torque: f64,
                 roll_torque: f64,
                 rudder_torque: f64,
                 return_rate: f64,
                 damp: f64,
                 inertia: [f64; 3],
                 fd_speed: f64,
                 drag_factor: f64,
                 weight: f64,
                 area: f64| {
        let parameters = documents.airframe(record).expect("the record imports");
        assert_eq!(
            parameters.inheritance_chain,
            ["basic_airplane", "player_airplane", record],
            "{record} inherits player_airplane which inherits basic_airplane"
        );
        assert_eq!(
            parameters.mode.as_deref(),
            Some("jet"),
            "`mode \"jet\"` is basic_airplane's"
        );
        assert!(!parameters.is_autogyro, "{record} is not an autogyro");
        assert_eq!(parameters.engine.id, engine_id, "{record}'s engine id");
        assert_eq!(
            parameters.engine.name, engine_name,
            "{record}'s engine name"
        );
        assert!((parameters.engine.factor - engine_factor).abs() < 1.0e-6);

        let value = |field: &str| {
            parameters
                .value(field)
                .unwrap_or_else(|| panic!("{record} states {field}"))
                .value
        };
        let close = |field: &str, expected: f64| {
            let measured = value(field);
            assert!(
                (measured - expected).abs() <= 1.0e-6 * expected.abs().max(1.0),
                "{record}.{field} = {measured}, expected {expected}"
            );
            let entry = parameters.value(field).expect("the field exists");
            if field == "level_off_rate" {
                // The image's default table, not a data file: no span, but
                // the claim names the address it was read from.
                assert_eq!(
                    entry.provenance.class,
                    cs_types::evidence::ClaimStatus::Documented
                );
                assert!(
                    entry.provenance.claim_id.as_str().ends_with("0x478a00"),
                    "{} names the default table",
                    entry.provenance.claim_id
                );
            } else {
                assert!(
                    entry.provenance.source.is_some(),
                    "{record}.{field} must carry its span"
                );
            }
            assert!(
                !matches!(
                    entry.provenance.class,
                    cs_types::evidence::ClaimStatus::VerifiedOriginal
                ),
                "a static-analysis import is never verified_original"
            );
        };

        close("pitch_torque", pitch_torque);
        close("roll_torque", roll_torque);
        close("rudder_torque", rudder_torque);
        close("return_rate", return_rate);
        close("ang_momentum_damp", damp);
        close("rec_moments_inertia_x", inertia[0]);
        close("rec_moments_inertia_y", inertia[1]);
        close("rec_moments_inertia_z", inertia[2]);
        close("fd_speed", fd_speed);
        close("drag_factor", drag_factor);
        close("veh_weight", weight);
        close("ref_area", area);
        close("engine_factor", engine_factor);
        close("gravity", 20.0);
        close("level_off_rate", 4.0);

        // Every airframe field is present and finite.
        let fields: Vec<&str> = parameters.values.iter().map(|entry| entry.field).collect();
        for field in AIRFRAME_FIELDS {
            assert!(fields.contains(&field), "{record} must state {field}");
        }
        assert_eq!(parameters.values.len(), AIRFRAME_FIELDS.len());

        // The chain's `fuel` is the player's, and it is known.
        let fuel = parameters
            .initial_fuel
            .clone()
            .known()
            .expect("player_airplane states fuel");
        assert!((fuel - 54926.0).abs() < 1.0e-3, "{record} fuel = {fuel}");

        // And it converts into the law's own parameters.
        OriginalAirframe::from_values(&parameters.field_values())
            .expect("the law accepts the imported record");
    };

    check(
        "pbloodhawk",
        11,
        "Bloodhawk Lvl-2",
        0.62,
        3.3,
        7.5,
        2.0,
        3.0,
        5.0,
        [1.18, 1.0, 1.1],
        135.0,
        0.37,
        1900.0,
        330.0,
    );
    check(
        "pdevastator",
        23,
        "Devastator Lvl-2",
        0.65,
        3.3,
        6.8,
        2.0,
        3.0,
        5.0,
        [0.85, 1.0, 0.8],
        113.0,
        0.62,
        2850.0,
        515.0,
    );
}

/// **The headless probe, level** (retail): with the attitude held and full
/// throttle the Bloodhawk settles at 134.3 m/s +/-1 %, and every player
/// fighter settles within 1.5 % of its `fd_speed`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flight_original_retail_level_top_speed_matches_fd_speed() {
    let documents = load();
    let globals = globals(&documents);

    let (bloodhawk, model) = model_for(&documents, &globals, "pbloodhawk");
    let fuel = bloodhawk
        .initial_fuel
        .clone()
        .known()
        .expect("the Bloodhawk has a fuel load");
    let speed = held_top_speed(&model, 500.0, fuel);
    assert!(
        (speed - 134.3).abs() <= 134.3 * 0.01,
        "the Bloodhawk's held top speed is {speed} m/s, expected 134.3 +/-1 %"
    );

    // The same check for every player fighter: the original's `fd_speed` is
    // the real top speed for all of them. The Balmoral bomber is excluded
    // (its held top speed is 56.3 m/s against `fd_speed` 79, a 29 % gap the
    // static analysis does not explain; recorded in the findings doc) and the
    // autogyro is excluded because it flies the fake-dynamics branch, whose
    // top speed is `fd_speed` by construction and is checked separately.
    let fighters = [
        "pbloodhawk",
        "pdevastator",
        "pfirebrand",
        "pbrigand",
        "pfury",
        "pavenger",
        "pkestrel",
        "ppeacemaker",
        "pwarhawk",
    ];
    for record in fighters {
        let (parameters, model) = model_for(&documents, &globals, record);
        let fuel = parameters
            .initial_fuel
            .clone()
            .known()
            .unwrap_or_else(|| panic!("{record} has a fuel load"));
        let fd_speed = parameters
            .value("fd_speed")
            .expect("fd_speed is imported")
            .value;
        let speed = held_top_speed(&model, 500.0, fuel);
        let error = (speed - fd_speed).abs() / fd_speed;
        assert!(
            error <= 0.015,
            "{record}: held top speed {speed} m/s vs fd_speed {fd_speed} ({:.1} %)",
            error * 100.0
        );
    }

    // The autogyro flies step 9's fake dynamics, so its speed converges to
    // exactly `fd_speed * throttle`.
    let (parameters, model) = model_for(&documents, &globals, "pautogyro");
    assert!(parameters.is_autogyro, "pautogyro sets is_autogyro");
    assert_eq!(model.dynamics, DynamicsKind::Fake);
    let fd_speed = parameters
        .value("fd_speed")
        .expect("fd_speed is imported")
        .value;
    let speed = held_top_speed(&model, 500.0, 1.0e9);
    assert!(
        (speed - fd_speed).abs() / fd_speed <= 0.015,
        "pautogyro's fake dynamics settle at {speed} m/s vs fd_speed {fd_speed}"
    );
}

/// **The headless probe, vertical** (retail): from rest at 500 m a held
/// vertical climb gains altitude with a net initial acceleration of
/// +11.2 m/s^2 and settles at 76 m/s; above the 2000 m ceiling the thrust to
/// weight ratio is about 0.23 and the same climb loses speed.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flight_original_retail_vertical_climb_and_ceiling() {
    let documents = load();
    let globals = globals(&documents);
    let (parameters, model) = model_for(&documents, &globals, "pbloodhawk");
    let fuel = parameters
        .initial_fuel
        .clone()
        .known()
        .expect("the Bloodhawk has a fuel load");

    // Below the ceiling: net initial acceleration and the settled climb. The
    // settle epsilon is the per-step change in speed below which the climb
    // has stopped rising: it has to be coarse enough that the run is over
    // while the aircraft is still under the 2000 m ceiling, because above it
    // the hard two-layer atmosphere cuts the thrust and a run that climbed
    // out would oscillate between the layers instead of settling.
    let (initial, settled, gained) = vertical_climb(&model, 500.0, 0.0, fuel, 1.0e-3);
    assert!(
        (initial - 11.2).abs() <= 11.2 * 0.05,
        "the initial vertical acceleration is {initial} m/s^2, expected 11.2 +/-5 %"
    );
    assert!(gained > 0.0, "the climb gained {gained} m of altitude");
    assert!(
        500.0 + gained < 2000.0,
        "the probe run stayed below the ceiling: {} m",
        500.0 + gained
    );
    assert!(
        (settled - 76.0).abs() <= 76.0 * 0.02,
        "the climb settles at {settled} m/s, expected 76 +/-2 %"
    );
    assert!(
        nose_direction(model_step_orientation(&model, fuel))[1] > 0.99,
        "the held attitude keeps the nose up"
    );

    // Above the ceiling: T/W from a level attitude at rest, and a climb that
    // cannot sustain itself.
    let mut state = OriginalState::at(2500.0, level(), 1.0, fuel);
    let input = OriginalInput {
        throttle: 1.0,
        is_player: true,
        hold_attitude: true,
        ..OriginalInput::default()
    };
    let step = model.step(&mut state, input, 0.01).expect("the step runs");
    let thrust_to_weight = step.thrust / parameters.value("veh_weight").expect("weight").value;
    assert!(
        (thrust_to_weight - 0.23).abs() <= 0.012,
        "above the ceiling T/W is {thrust_to_weight}, expected about 0.23"
    );

    let mut climbing = OriginalState::at(2500.0, nose_up(), 1.0, fuel);
    climbing.velocity_mps = [0.0, 40.0, 0.0];
    for _ in 0..100 {
        model
            .step(&mut climbing, input, 0.01)
            .expect("the step runs");
    }
    assert!(
        climbing.velocity_mps[1] < 40.0,
        "above 2000 m the vertical climb loses speed: {} m/s after 1 s",
        climbing.velocity_mps[1]
    );
}

/// The attitude a held probe keeps, for the nose-up assertion above.
fn model_step_orientation(model: &OriginalFlightModel, fuel: f64) -> Quaternion {
    let mut state = OriginalState::at(500.0, nose_up(), 1.0, fuel);
    let input = OriginalInput {
        throttle: 1.0,
        is_player: true,
        hold_attitude: true,
        ..OriginalInput::default()
    };
    model
        .step(&mut state, input, 0.01)
        .expect("the production step runs");
    state.orientation
}
