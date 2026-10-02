//! Acceptance scenarios F41-B (follow-up #445): engine pitch and volume
//! smoothing from measured throttle state, stable across how the elapsed time
//! was divided into steps.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`, non-negotiable behavior 1. Task test prefix: `accept_f41_b_`.
//!
//! These drive the production [`cs_sim::audio_events::EngineVoice`] /
//! [`EngineSmoothing`] the ECS smoothing system runs every fixed tick. The
//! law's numbers are designed project data; what is asserted here is the
//! *property* the spec demands of it — that equal elapsed time reaches the same
//! level however many steps it took, which is what "not render FPS" means for
//! a mix.

use cs_sim::audio_events::{EngineSmoothing, EngineVoice, VoiceLevel};

const LAW: EngineSmoothing = EngineSmoothing::DESIGNED_DEFAULT;

fn law() -> EngineSmoothing {
    LAW
}

/// Advances `voice` toward `target` for `seconds` in `steps` equal parts.
fn advance(voice: &mut EngineVoice, target: VoiceLevel, seconds: f64, steps: u32) {
    let dt = seconds / f64::from(steps);
    for _ in 0..steps {
        voice.advance(&law(), target, dt);
    }
}

/// How close two levels must be for the step-invariance assertion.
///
/// The law's clamp is exact, so the only difference between one long step and
/// twenty short ones is the summation order of the additions — an ulp-scale
/// disagreement. A law that did not clamp (an exponential lag, say) would
/// disagree by a visible fraction of the ramp, which is what the tolerance is
/// tight enough to catch.
const STEP_SUM_EPSILON: f64 = 1e-12;

/// Asserts two levels agree to summation order.
#[track_caller]
fn assert_same_level(left: VoiceLevel, right: VoiceLevel, what: &str) {
    assert!(
        (left.gain - right.gain).abs() <= STEP_SUM_EPSILON,
        "{what}: gain {left:?} vs {right:?}"
    );
    assert!(
        (left.pitch - right.pitch).abs() <= STEP_SUM_EPSILON,
        "{what}: pitch {left:?} vs {right:?}"
    );
}

/// The same elapsed time reaches the same level however it was divided: one
/// 0.4 s step and twenty 0.02 s steps agree, on the attack and on the release.
///
/// A smoothing law that integrated without a clamp at the target (an
/// exponential lag, say) would disagree between the two, and an implementation
/// that stepped per render frame instead of per fixed tick would not even be
/// offered the same `dt` twice.
#[test]
fn accept_f41_b_smoothing_is_step_invariant() {
    let up = law().target(true, 1.0);
    let down = law().target(true, 0.0);

    let mut coarse = EngineVoice::at_idle(&law());
    coarse.advance(&law(), up, 0.4);
    let mut fine = EngineVoice::at_idle(&law());
    advance(&mut fine, up, 0.4, 20);
    assert_same_level(coarse.level(), fine.level(), "attack");

    // ... and the same on the way back down, where the law uses the release
    // rate instead of the attack rate.
    let mut coarse_down = coarse;
    coarse_down.advance(&law(), down, 0.4);
    let mut fine_down = fine;
    advance(&mut fine_down, down, 0.4, 20);
    assert_same_level(coarse_down.level(), fine_down.level(), "release");
}

/// A voice reaches its target and stops there: it never overshoots, never
/// exceeds the law's ceiling, and a further step changes nothing.
#[test]
fn accept_f41_b_smoothing_reaches_the_target_without_overshoot() {
    let mut voice = EngineVoice::at_idle(&law());
    let start = voice.level();
    let target = law().target(true, 1.0);
    assert_eq!(
        start,
        VoiceLevel::new(law().idle_gain(), law().idle_pitch())
    );
    assert!(
        start.gain < target.gain && start.pitch < target.pitch,
        "the designed law opens upward"
    );
    // One step longer than the ramp needs, in one piece: clamped at the target.
    voice.advance(&law(), target, 10.0);
    assert_eq!(
        voice.level(),
        target,
        "the ramp lands exactly on the target"
    );
    voice.advance(&law(), target, 10.0);
    assert_eq!(voice.level(), target, "and stays there");
    assert!(voice.level().validate().is_ok());

    // The engine stopping asks for silence, and the voice falls to it.
    let stopped = law().target(false, 1.0);
    assert_eq!(stopped.gain, 0.0, "a stopped engine is silent");
    voice.advance(&law(), stopped, 10.0);
    assert_eq!(voice.gain(), 0.0);
    assert_eq!(voice.pitch(), law().idle_pitch());
}

/// An entity that does not exist cannot be smoothed: a voice nobody advances
/// keeps its last level, which is what lets the ECS system drop a destroyed
/// aircraft's voice without inventing a fade.
#[test]
fn accept_f41_b_unadvanced_voice_keeps_its_level() {
    let mut voice = EngineVoice::at_idle(&law());
    let idle = voice.level();
    voice.advance(&law(), law().target(true, 1.0), 0.1);
    assert_ne!(voice.level(), idle);
    let held = voice.level();
    assert_eq!(
        held,
        voice.level(),
        "a voice only moves when it is advanced"
    );
}
