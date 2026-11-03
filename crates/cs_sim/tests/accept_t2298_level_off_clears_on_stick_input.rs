//! Task #2298: the player's Level-Off assist is **cleared by stick input**, as
//! the original's player input routine does (`0x487d3d`..`0x487d96`).
//!
//! Static reading of the owner image (`llvm-objdump -d -M intel`), claim
//! `OWNER-STATIC-2026-10-29`, nothing `verified_original`:
//!
//! * `0x487d3d` `d9 86 00 01 00 00` loads `[esi+0x100]`, `0x487d53` `d9 86 08
//!   01 00 00` loads `+0x108`, `0x487d66` `d9 86 0c 01 00 00` loads `+0x10c`;
//!   each is compared against `0.0` (the image's zero at `0x6032c8`) and each
//!   `test ah, 0x40` / `je` takes the first non-zero word to `0x487d96`
//!   (`0x487d4e`..`0x487d51`, `0x487d61`..`0x487d64`, `0x487d74`..`0x487d77`).
//! * `0x487d96` `c6 86 2c 01 00 00 00` is `mov byte [esi+0x12c], 0`.
//! * Those three words are the raw stick: each is copied through the identity
//!   curve `0x460890` (`d9 44 24 04 c3`, `fld [esp+4]; ret`) into the force
//!   block's inputs `+0x114`/`+0x11c`/`+0x120` (`0x487daa`..`0x487db0`,
//!   `0x487dc7`..`0x487dcc`, `0x487de3`..`0x487de8`), which are exactly what
//!   [`OriginalInput`] carries.
//! * The only writer that **sets** the byte is the player-vehicle `Shift+L`
//!   handler (`0x488640`..`0x48866e`: `mov ecx,[0x71c298]`, `sete al`,
//!   `mov [ecx+0x12c], al`), and the vehicle constructor clears it
//!   (`0x4b0645`). The clear never reaches an AI body, which never has the
//!   flag set in the first place.
//!
//! The assist's own term gate is unchanged: `+0x114 == 0 && +0x11c == 0`
//! (`0x48ce53`, `0x48ce6a`, §11.3 of
//! `docs/findings/2026-11-02-m01-wingman-gains.md`). So a cleared flag means
//! the assist is off after the stick returns to centre until `Shift+L` arms it
//! again — the divergence that finding recorded and this task fixes.
//!
//! These tests drive the production [`OriginalFlightModel::step`]. Removing the
//! clear, clearing on only roll and pitch, or clearing the AI seat each makes
//! one of them fail. Every airframe and globals value here is a synthetic test
//! fixture, never original game data.

use cs_sim::flight::{
    OriginalAirframe, OriginalFlightModel, OriginalGlobals, OriginalInput, OriginalState,
    nose_direction,
};
use cs_types::space::{Quaternion, Radians, UnitVec3};

/// The synthetic fixture the law's own unit tests use, so nothing here claims
/// a retail parameter.
fn airframe() -> OriginalAirframe {
    OriginalAirframe::from_values(&[
        ("roll_torque", 7.5),
        ("pitch_torque", 3.3),
        ("rudder_torque", 2.0),
        ("level_off_rate", 4.0),
        ("return_rate", 3.0),
        ("ang_momentum_damp", 5.0),
        ("rec_moments_inertia_x", 1.18),
        ("rec_moments_inertia_y", 1.0),
        ("rec_moments_inertia_z", 1.1),
        ("fd_speed", 135.0),
        ("engine_factor", 0.62),
        ("drag_factor", 0.37),
        ("veh_weight", 1900.0),
        ("ref_area", 330.0),
        ("gravity", 20.0),
    ])
    .expect("the synthetic airframe fixture is complete")
}

fn globals() -> OriginalGlobals {
    OriginalGlobals::from_values(&[
        ("nom_gravity", 20.0),
        ("lift_aoa_0_deg", 5.0),
        ("lift_aoa_1_deg", 9.0),
        ("max_aoa_deg", 46.0),
        ("high_g_0", 9.0),
        ("high_g_1", 15.0),
        ("low_g_0", -6.0),
        ("low_g_1", -9.0),
        ("lift_accel_rate", 0.75),
        ("stall_mag", 1.25),
        ("turn_fade_in_mph", 10.0),
        ("turn_fade_out_mph", 50.0),
        ("yaw_low_speed", 0.0625),
        ("yaw_high_speed", 0.17),
        ("yaw_fade_in_mph", 10.0),
        ("yaw_max_mph", 50.0),
        ("yaw_fade_out_mph", 400.0),
        ("groundblow_elev_m", 400.0),
        ("groundblow_mag", 10.0),
        ("ai_groundblow", 0.5),
    ])
    .expect("the synthetic globals fixture is complete")
}

/// A 30-degree bank at cruise with the velocity on the nose, so the assist is
/// the only wing-leveling term under test (the bank coupling acts in every
/// compared case alike).
fn banked_state(armed: bool) -> OriginalState {
    let banked = Quaternion::from_axis_angle(UnitVec3::FORWARD, Radians(0.5))
        .expect("a banked attitude is a unit quaternion");
    let mut state = OriginalState::at(0.0, banked, 1.0, 1e9);
    let nose = nose_direction(banked);
    state.velocity_mps = [nose[0] * 134.0, nose[1] * 134.0, nose[2] * 134.0];
    state.level_off = armed;
    state
}

/// Hands off: throttle only, so every stick word is exactly `0.0`.
fn hands_off(is_player: bool) -> OriginalInput {
    OriginalInput {
        throttle: 1.0,
        is_player,
        ..OriginalInput::default()
    }
}

fn deflected(roll: f64, pitch: f64, yaw: f64) -> OriginalInput {
    OriginalInput {
        roll,
        pitch,
        yaw,
        ..hands_off(true)
    }
}

/// The angle between the body up axis and world up, through the shared
/// [`Quaternion::rotate`]: the quantity the assist levels.
fn bank_deg(state: &OriginalState) -> f64 {
    let up = state
        .orientation
        .rotate(UnitVec3::UP)
        .expect("a law state's attitude is a unit quaternion")
        .to_array()[1];
    up.clamp(-1.0, 1.0).acos().to_degrees()
}

/// **The player's flag is cleared in the same step any stick word reaches the
/// law**, for each of roll, pitch and yaw (`0x487d51`/`0x487d64`/`0x487d77` →
/// `0x487d96`), and with it cleared the step is the unarmed step.
#[test]
fn accept_t2298_player_stick_input_clears_the_level_off_assist_in_the_same_step() {
    let model = OriginalFlightModel::full(airframe(), globals());

    for (name, input) in [
        ("roll", deflected(0.5, 0.0, 0.0)),
        ("pitch", deflected(0.0, 0.5, 0.0)),
        ("yaw", deflected(0.0, 0.0, 0.5)),
    ] {
        let mut armed = banked_state(true);
        let mut unarmed = banked_state(false);
        let armed_step = model
            .step(&mut armed, input, 0.01)
            .expect("the declared step is valid");
        let unarmed_step = model
            .step(&mut unarmed, input, 0.01)
            .expect("the declared step is valid");
        assert!(
            !armed.level_off,
            "a non-zero {name} word must clear the player's Level-Off flag in \
             the same step (`0x487d96`)"
        );
        assert_eq!(
            armed_step.world_torque, unarmed_step.world_torque,
            "with {name} deflected the cleared assist must not contribute \
             torque"
        );
        assert_eq!(
            armed, unarmed,
            "and the cleared state must be the unarmed state, not only the \
             same torque"
        );
    }
}

/// **Cleared means off after the stick returns to centre**: the hands-off step
/// that follows a deflection is the unarmed step, while an armed state that
/// never saw a stick input still levels its wings until `Shift+L` arms it
/// again. A hands-off step keeps an armed flag armed.
#[test]
fn accept_t2298_cleared_assist_stays_off_hands_off_until_rearmed() {
    let model = OriginalFlightModel::full(airframe(), globals());

    // The deflection clears the flag.
    let mut cleared = banked_state(true);
    model
        .step(&mut cleared, deflected(0.5, 0.0, 0.0), 0.01)
        .expect("the declared step is valid");
    assert!(!cleared.level_off);

    // The same pose with the flag set is a second `Shift+L` re-arming it.
    let mut rearmed = cleared.clone();
    rearmed.level_off = true;

    let cleared_step = model
        .step(&mut cleared, hands_off(true), 0.01)
        .expect("the declared step is valid");
    assert!(
        !cleared.level_off,
        "a hands-off step must not re-arm the assist: only the player's \
         `Shift+L` handler sets it (`0x488640`..`0x48866e`)"
    );
    let armed_step = model
        .step(&mut rearmed, hands_off(true), 0.01)
        .expect("the declared step is valid");
    assert!(
        rearmed.level_off,
        "a hands-off step keeps an armed assist armed"
    );
    assert_ne!(
        cleared_step.world_torque, armed_step.world_torque,
        "the cleared assist must not add the leveling torque the re-armed \
         one does"
    );

    // Over three simulated seconds the armed assist levels its bank while the
    // cleared one keeps it: the clear turns the assist off, it does not break
    // the term.
    let settle = |armed: bool| -> (f64, bool) {
        let mut state = banked_state(armed);
        for _ in 0..300 {
            model
                .step(&mut state, hands_off(true), 0.01)
                .expect("the declared step is valid");
        }
        (bank_deg(&state), state.level_off)
    };
    let (armed_bank, armed_flag) = settle(true);
    let (off_bank, off_flag) = settle(false);
    println!(
        "T2298-LEVELOFF bank_deg_armed={armed_bank:.4} bank_deg_off={off_bank:.4} \
         start_deg={:.4}",
        bank_deg(&banked_state(true))
    );
    assert!(armed_flag, "the armed case is never cleared hands-off");
    assert!(!off_flag, "the unarmed case stays unarmed");
    assert!(
        armed_bank < 5.0,
        "the armed assist must still roll the 30-degree bank level, got \
         {armed_bank} deg"
    );
    assert!(
        off_bank > 15.0,
        "the cleared assist must leave the bank alone, got {off_bank} deg"
    );
}

/// **The clear is the player's seat only.** The original's clear lives in the
/// player input routine and the flag's only setter is the player-vehicle
/// toggle, so no AI body ever has it set; the law therefore does not clear the
/// AI seat either. This pins the seat gate, so a clear that forgets
/// `is_player` fails here.
#[test]
fn accept_t2298_the_clear_is_the_players_seat_only() {
    let model = OriginalFlightModel::full(airframe(), globals());
    let ai_deflected = OriginalInput {
        is_player: false,
        ..deflected(0.5, 0.5, 0.5)
    };

    let mut armed_ai = banked_state(true);
    model
        .step(&mut armed_ai, ai_deflected, 0.01)
        .expect("the declared step is valid");
    assert!(
        armed_ai.level_off,
        "the AI seat has no clear-on-input writer (`0x487d3d`..`0x487d96` is \
         the player's input routine), so the law leaves its flag as it found it"
    );

    // Hands off, the armed AI keeps its flag too: no AI path sets or clears
    // it in the image (`0x4b0645` clears it once, at construction).
    let mut hands_off_ai = banked_state(true);
    model
        .step(&mut hands_off_ai, hands_off(false), 0.01)
        .expect("the declared step is valid");
    assert!(hands_off_ai.level_off);
}
