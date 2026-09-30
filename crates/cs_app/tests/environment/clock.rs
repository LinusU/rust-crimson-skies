//! `accept_f19_a_` tests for F19 non-negotiable behavior 4: authored
//! weather changes are deterministic timeline events, and pause and replay
//! respect their time domain.
//!
//! The production code under test is `cs_app::environment::EnvironmentClock`
//! — the wiring from a definition's authored timeline onto
//! `cs_sim::visibility::VisibilityTimeline` — driven at the fixtures' fixed
//! rate. The state each sample reads comes from the clock, never from a
//! copy the test kept.

use std::time::Duration;

use cs_app::environment::{EnvironmentClock, STORM_RATE_HZ, VISIBILITY_TICK, WIND_SHIFT_TICK};
use cs_content::environment::EnvironmentState;
use cs_sim::time::{TimeDomain, TimeError};
use cs_sim::visibility::TimelineError;
use cs_types::content::Resolved;

use crate::common;

/// One observation of the clock: the committed tick, the events installed
/// so far, and the two state values the assertions read.
#[derive(Clone, Debug, PartialEq)]
struct Sample {
    tick: u64,
    applied: Vec<usize>,
    wind_m_s: [f64; 3],
    visibility_range_m: Option<f64>,
}

fn wind_m_s(state: &EnvironmentState) -> [f64; 3] {
    match state.wind() {
        Resolved::Known(known) => known.value.velocity_m_s(),
        Resolved::Unknown { .. } => panic!("the storm fixture authors a wind field"),
    }
}

fn visibility_range_m(state: &EnvironmentState) -> Option<f64> {
    match state.gameplay_visibility() {
        Resolved::Known(known) => Some(known.value.range_m()),
        Resolved::Unknown { .. } => None,
    }
}

fn sample(clock: &EnvironmentClock) -> Sample {
    Sample {
        tick: clock.tick().0,
        applied: clock.applied().to_vec(),
        wind_m_s: wind_m_s(clock.state()),
        visibility_range_m: visibility_range_m(clock.state()),
    }
}

/// The storm clock at the fixtures' fixed rate, built by production code.
fn storm_clock() -> EnvironmentClock {
    EnvironmentClock::new(&common::storm(), common::tick_rate())
        .expect("the storm fixture's schedule is strictly increasing")
}

/// One run of `frames`, sampled after construction and after every frame.
fn run(frames: &[Duration]) -> Vec<Sample> {
    let mut clock = storm_clock();
    let mut samples = vec![sample(&clock)];
    for frame in frames {
        clock
            .advance_frame(*frame)
            .expect("the fixture run does not overflow the tick counter");
        samples.push(sample(&clock));
    }
    samples
}

/// The first tick at which exactly `installed` events had been applied.
fn first_tick_with(samples: &[Sample], installed: usize) -> Option<u64> {
    samples
        .iter()
        .find(|sample| sample.applied.len() == installed)
        .map(|sample| sample.tick)
}

/// Each authored event fires on its own tick, the state it installs
/// replaces the previous one whole, an un-authored value stays unknown
/// until an event authors it, and pausing freezes the whole schedule.
#[test]
fn accept_f19_a_weather_timeline_fires_each_event_at_its_own_tick_and_freezes_while_paused() {
    let mut clock = storm_clock();
    assert_eq!(clock.domain(), TimeDomain::AuthoritativeGameplay);
    assert_eq!(
        clock.id().as_str(),
        common::storm().id().as_str(),
        "the clock runs the environment it was built from"
    );

    // The authored initial state: gust winds, clear sky, and a sight range
    // no event has reached yet.
    assert_eq!(clock.tick().0, 0);
    assert!(clock.applied().is_empty());
    assert_eq!(wind_m_s(clock.state()), [8.0, 0.0, 0.0]);
    assert_eq!(
        visibility_range_m(clock.state()),
        None,
        "an un-authored sight range starts as an explicit unknown, not a default"
    );

    // One frame before the gust tick: still nothing has fired.
    for _ in 0..(WIND_SHIFT_TICK - 1) {
        clock
            .advance_frame(common::FRAME)
            .expect("the clock advances");
    }
    assert_eq!(clock.tick().0, WIND_SHIFT_TICK - 1);
    assert!(clock.applied().is_empty());
    assert_eq!(wind_m_s(clock.state()), [8.0, 0.0, 0.0]);

    // The frame that reaches the gust tick installs exactly that event.
    clock
        .advance_frame(common::FRAME)
        .expect("the clock advances");
    assert_eq!(clock.tick().0, WIND_SHIFT_TICK);
    assert_eq!(clock.applied(), &[0]);
    assert_eq!(wind_m_s(clock.state()), [16.0, 0.0, 2.0]);
    assert_eq!(
        visibility_range_m(clock.state()),
        None,
        "the gust authors a wind and a precipitation, not a sight range"
    );

    // Paused: the tick, the installed events and the state all stand still,
    // however many frames arrive.
    clock.set_paused(true);
    assert!(clock.is_paused());
    for _ in 0..100 {
        clock
            .advance_frame(common::FRAME)
            .expect("a paused clock accepts frames");
    }
    assert_eq!(
        clock.tick().0,
        WIND_SHIFT_TICK,
        "paused time must not advance"
    );
    assert_eq!(clock.applied(), &[0]);
    assert_eq!(wind_m_s(clock.state()), [16.0, 0.0, 2.0]);

    // Resumed, the schedule continues from the tick it stopped at: paused
    // frames were discarded, not banked, so the next event still fires at
    // exactly its own tick.
    clock.set_paused(false);
    for _ in 0..(VISIBILITY_TICK - WIND_SHIFT_TICK - 1) {
        clock
            .advance_frame(common::FRAME)
            .expect("the clock advances");
    }
    assert_eq!(
        clock.applied(),
        &[0],
        "the visibility event has not been reached"
    );
    assert_eq!(visibility_range_m(clock.state()), None);

    clock
        .advance_frame(common::FRAME)
        .expect("the clock advances");
    assert_eq!(clock.tick().0, VISIBILITY_TICK);
    assert_eq!(clock.applied(), &[0, 1]);
    assert_eq!(
        visibility_range_m(clock.state()),
        Some(400.0),
        "the authored event installs the authored sight range"
    );
    assert_eq!(wind_m_s(clock.state()), [16.0, 0.0, 2.0]);
}

/// Replay and frame rate: the same frames reach the same states, and the
/// same total time split into different frames installs each event at the
/// same tick.
#[test]
fn accept_f19_a_replaying_the_same_frames_reaches_the_same_states() {
    let per_tick = vec![common::FRAME; 200];
    let first = run(&per_tick);
    let second = run(&per_tick);
    assert_eq!(
        first, second,
        "replaying identical frames must reach identical states"
    );

    // The same wall time delivered four ticks at a time: a different frame
    // split, the same schedule.
    let per_four_ticks = vec![common::FRAME * 4; 50];
    let coarser = run(&per_four_ticks);
    assert_eq!(
        first.len(),
        201,
        "the two runs sample the same total time in different steps"
    );
    assert_eq!(
        first.last(),
        coarser.last(),
        "the same total time must end in the same state"
    );

    for installed in [1, 2] {
        assert_eq!(
            first_tick_with(&first, installed),
            first_tick_with(&coarser, installed),
            "event {installed} must be installed on the same tick whatever the frame split"
        );
    }
    assert_eq!(first_tick_with(&first, 1), Some(WIND_SHIFT_TICK));
    assert_eq!(first_tick_with(&first, 2), Some(VISIBILITY_TICK));
    assert_eq!(first_tick_with(&coarser, 1), Some(WIND_SHIFT_TICK));
    assert_eq!(first_tick_with(&coarser, 2), Some(VISIBILITY_TICK));
}

/// The domain grants no local speed-up authority, so injected ticks are
/// refused — before the pause check, and without mutating anything.
#[test]
fn accept_f19_a_environment_clock_refuses_local_speed_up_ticks() {
    let mut clock = storm_clock();
    let before = sample(&clock);

    assert!(matches!(
        clock.advance_fixed_ticks(1),
        Err(TimelineError::Clock(TimeError::NoSpeedUpAuthority))
    ));
    assert_eq!(
        sample(&clock),
        before,
        "a refused advance must mutate nothing"
    );

    // Paused, the refusal is still the authority one: a replay may not use
    // pause as a way to inject ticks either.
    clock.set_paused(true);
    assert!(matches!(
        clock.advance_fixed_ticks(1),
        Err(TimelineError::Clock(TimeError::NoSpeedUpAuthority))
    ));
    assert_eq!(sample(&clock), before);

    // The fixtures really do run at the declared rate, so the tick
    // arithmetic above means what it says.
    assert_eq!(STORM_RATE_HZ, 64);
}
