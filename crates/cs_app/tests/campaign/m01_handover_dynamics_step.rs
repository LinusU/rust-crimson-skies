//! M01-HANDOVER-DYNAMICS-STEP (#1982): the static read of the flight dynamics
//! step the vehicle loop runs after the M01 hand-over
//! (finding `docs/findings/2026-10-23-m01-handover-dynamics-step.md`).
//!
//! * the finding's `bytes` block parses and covers the claims' sites;
//! * with the owner's image named, every row is re-read from the image;
//! * the level-flight equilibrium formula the finding states is checked as a
//!   pure function, then against the production law's own held-attitude run;
//! * the production law is flown from the two hand-over states the image can
//!   produce, and the finding's numbers are pinned.

use std::path::PathBuf;

use cs_app::engine_facts::engine_facts;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::stage_for;
use cs_content::coordinates::{load_engine_image, original_image_digest};
use cs_sim::flight::original::{
    OriginalInput, OriginalState, atmosphere, drag_coefficient, mach_from_speed, nose_direction,
    thrust_coefficient,
};
use cs_types::space::{Quaternion, Radians, UnitVec3};

use crate::campaign::common::label;

const FINDING: &str =
    include_str!("../../../../docs/findings/2026-10-23-m01-handover-dynamics-step.md");

/// Sites the finding's conclusions rest on; each must be in the block.
const REQUIRED_SITES: &[u64] = &[
    0x489abc, 0x489b14, 0x489bb4, 0x489edb, 0x489f48, 0x41c2e6, 0x41afea, 0x41b7df, 0x41bbc3,
    0x4897c6, 0x48e58f, 0x48e5e6, 0x48e652, 0x48e698, 0x6032e0, 0x608040, 0x47f637, 0x47f648,
    0x48fc70, 0x48fce2, 0x48fd76,
];

struct Site {
    va: u64,
    bytes: Vec<u8>,
}

fn sites() -> Vec<Site> {
    let block = FINDING
        .split("```bytes\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("the finding has a `bytes` block");
    block
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (va, rest) = line.split_once(": ").expect("`va: hex  # note`");
            let hex = rest.split('#').next().expect("hex").trim();
            assert!(hex.len() % 2 == 0 && !hex.is_empty(), "hex of {va}");
            Site {
                va: u64::from_str_radix(va, 16).expect("a hex va"),
                bytes: (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("a hex byte"))
                    .collect(),
            }
        })
        .collect()
}

#[test]
fn accept_m01_handover_dynamics_step_block_parses_and_covers_the_claims() {
    engine_facts().expect("the committed table parses");
    let sites = sites();
    for want in REQUIRED_SITES {
        assert!(
            sites.iter().any(|site| site.va == *want),
            "the finding's block lacks {want:#x}"
        );
    }
    let f32_at = |va: u64| {
        let site = sites.iter().find(|s| s.va == va).expect("a cited constant");
        f32::from_le_bytes(site.bytes[..4].try_into().expect("four bytes"))
    };
    // The slew rate, the fuel factor and the placement factor.
    assert_eq!(f32_at(0x6032e0), 0.5);
    assert_eq!(f32_at(0x6036bc), 5.0);
    assert_eq!(f32_at(0x608040), -20.0);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_handover_dynamics_step_instructions_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    for site in sites() {
        let offset = usize::try_from(facts.image_offset(site.va).expect("va inside the image"))
            .expect("an offset");
        assert_eq!(
            &image[offset..offset + site.bytes.len()],
            site.bytes.as_slice(),
            "instruction bytes at {:#x}",
            site.va
        );
    }
}

/// The level, hands-off equilibrium speed of the law (finding section 4):
/// the Mach `M` where thrust equals drag, so `S` cancels,
/// `Tc(max(M, 0.1)) * throttle * engine = 0.5 * rho * v_ft^2 * drag_factor * CD(M)`.
/// `None` when there is no positive root (throttle 0).
fn level_equilibrium_speed(
    throttle: f64,
    engine_factor: f64,
    drag_factor: f64,
    altitude_m: f64,
) -> Option<f64> {
    let air = atmosphere(altitude_m);
    let surplus = |speed_mps: f64| {
        let mach = mach_from_speed(speed_mps, &air);
        let speed_ft = speed_mps * cs_sim::flight::original::METERS_TO_FEET;
        thrust_coefficient(mach.max(0.1), &air) * throttle * engine_factor
            - 0.5 * air.rho * speed_ft * speed_ft * drag_factor * drag_coefficient(mach)
    };
    // Surplus is positive at the floor speed and negative at twice the speed
    // of sound for any positive throttle; bisect between.
    let (mut low, mut high) = (0.1_f64 * 0.3048 * air.sound_speed_ftps * 0.1, 600.0);
    if surplus(low) <= 0.0 || surplus(high) >= 0.0 {
        return None;
    }
    for _ in 0..200 {
        let mid = 0.5 * (low + high);
        if surplus(mid) > 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    Some(0.5 * (low + high))
}

#[test]
fn accept_m01_handover_dynamics_step_level_equilibrium_formula() {
    // Engine 0.65 and drag factor 0.62 are the pdevastator numbers the finding
    // uses; here they are only inputs of the pure formula.
    let at = |throttle| level_equilibrium_speed(throttle, 0.65, 0.62, 150.0);
    assert_eq!(at(0.0), None, "no thrust, no level equilibrium");
    let slow = at(0.4).expect("a root at 0.4");
    let fast = at(1.0).expect("a root at 1.0");
    assert!((slow - 78.77).abs() < 0.1, "0.4 -> {slow}");
    assert!((fast - 112.29).abs() < 0.1, "1.0 -> {fast}");
    assert!(slow < at(0.7).expect("0.7") && at(0.7).expect("0.7") < fast);
    // The root balances thrust and drag.
    let air = atmosphere(150.0);
    let mach = mach_from_speed(slow, &air);
    let thrust = thrust_coefficient(mach.max(0.1), &air) * 0.4 * 0.65;
    let v_ft = slow * cs_sim::flight::original::METERS_TO_FEET;
    let drag = 0.5 * air.rho * v_ft * v_ft * 0.62 * drag_coefficient(mach);
    assert!((thrust - drag).abs() / drag < 1e-9, "T {thrust} D {drag}");
}

fn stage() -> cs_app::mission_session::MissionStage {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    stage_for(&root, &plan).expect("stage")
}

fn speed(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The production law's held-attitude run (the dev probe `0x491c60`) agrees
/// with the pure formula at throttle 0.4 and 1.0.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_handover_dynamics_step_held_level_matches_the_formula() {
    let stage = stage();
    let airframe = &stage.flight.model.airframe;
    for (throttle, expected) in [(0.4, 78.77), (1.0, 112.29)] {
        let mut state = OriginalState {
            position_m: [0.0, 150.0, 0.0],
            velocity_mps: [0.0; 3],
            orientation: Quaternion::IDENTITY,
            angular_momentum_world: [0.0; 3],
            throttle,
            fuel: 1.0e9,
            level_off: false,
        };
        let input = OriginalInput {
            throttle,
            hold_attitude: true,
            ..OriginalInput::default()
        };
        for _ in 0..30_000 {
            stage
                .flight
                .model
                .step(&mut state, input, 0.01)
                .expect("the law steps");
        }
        let settled = speed(state.velocity_mps);
        let formula = level_equilibrium_speed(
            throttle,
            airframe.engine_factor,
            airframe.drag_factor,
            150.0,
        )
        .expect("a root");
        assert!(
            (settled - formula).abs() < 0.05,
            "throttle {throttle}: run {settled}, formula {formula}"
        );
        assert!((settled - expected).abs() < 0.1, "{throttle}: {settled}");
    }
}

/// Flies the production law from the hand-over pose with a constant throttle
/// command (actual = command, as the tail of `0x47f1f0` leaves it), a start
/// speed `forward` along the nose and zero angular momentum. Returns the
/// state every second for `seconds`.
fn fly(throttle: f64, forward: f64, seconds: usize) -> Vec<OriginalState> {
    let stage = stage();
    let orientation =
        Quaternion::from_axis_angle(UnitVec3::UP, Radians(f64::from(stage.start.heading)))
            .expect("a yaw is a unit quaternion");
    let nose = nose_direction(orientation);
    let mut state = OriginalState {
        position_m: stage.start.position.map(f64::from),
        velocity_mps: nose.map(|c| c * forward),
        orientation,
        angular_momentum_world: [0.0; 3],
        throttle,
        fuel: stage.flight.fuel,
        level_off: false,
    };
    let input = OriginalInput {
        throttle,
        ..OriginalInput::default()
    };
    let mut samples = vec![state.clone()];
    for tick in 1..=seconds * 60 {
        stage
            .flight
            .model
            .step(&mut state, input, 1.0 / 60.0)
            .expect("the law steps");
        if tick % 60 == 0 {
            samples.push(state.clone());
        }
    }
    samples
}

/// The cinematic placement state (`0x47f1f0` with `veh+0x91d` set: throttle
/// 0.4, velocity `-20` along the body +Z, i.e. 20 forward) never reaches the
/// 111.8-113.6 m/s of the capture: it settles at 82.8 m/s, sinking 9.35 m/s.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_handover_dynamics_step_placement_state_settles_below_the_capture() {
    let samples = fly(0.4, 20.0, 20);
    let at = |t: usize| speed(samples[t].velocity_mps);
    assert!((at(0) - 20.0).abs() < 1e-6);
    assert!(at(2) > 50.0 && at(2) < 60.0, "t=2: {}", at(2));
    assert!(at(10) > 82.0 && at(10) < 83.5, "t=10: {}", at(10));
    assert!(at(20) > 82.0 && at(20) < 83.5, "t=20: {}", at(20));
    for sample in &samples {
        assert!(speed(sample.velocity_mps) < 90.0);
    }
    let sink = -samples[20].velocity_mps[1];
    assert!((sink - 9.35).abs() < 0.3, "sink {sink}");
}

/// The release state (throttle 0, velocity the zero vector) has no thrust and
/// no lift at rest: it falls and is still diving at 70+ m/s after 10 s.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_handover_dynamics_step_release_state_falls() {
    let samples = fly(0.0, 0.0, 6);
    assert!(samples[6].position_m[1] < 0.0, "below the sea after 6 s");
    assert!(speed(samples[6].velocity_mps) > 60.0);
    assert!(samples[6].velocity_mps[1] < -60.0);
    // And at throttle 0.4 from rest it dives to 95 m/s.
    let rest = fly(0.4, 0.0, 10);
    assert!(speed(rest[10].velocity_mps) > 95.0 && speed(rest[10].velocity_mps) < 96.5);
}
