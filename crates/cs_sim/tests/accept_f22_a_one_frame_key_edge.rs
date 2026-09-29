//! Acceptance scenario F22-A (AC01): a one-frame key edge produces one
//! action across multiple physics substeps.
//!
//! These tests exercise production code only: the `cs_types::input` command
//! schema and action map (`ActionMap`, `InputFrame`, `InputContext`) and the
//! `cs_sim::control` consumer (`ControlBuffer`, `EdgeBuffer` behavior). They
//! drive the buffer the way the fixed-step loop does — one render frame,
//! several fixed substeps — instead of asserting on a test-only
//! implementation.
//!
//! What makes the scenario discriminating:
//!
//! * If `begin_tick` returned the queued edge on every substep (for example
//!   because the edge was stored as a held "pressed" flag instead of a
//!   once-consumed queue entry) the run would fire three times.
//! * If the edge were consumed by the render frame rather than the input
//!   boundary, the substeps would see nothing at all.
//! * If continuous axes shared the edge queue, the held deflection would be
//!   drained once and the later substeps would go neutral.
//!
//! Every binding, command and fixture value here is newly authored
//! development data, not measured original game data.

use cs_sim::control::{AxisState, ControlBuffer, ControlError};
use cs_types::Tick;
use cs_types::input::{
    Action, ActionMap, AxisValue, AxisValueError, BindingSource, FlightCommand, InputContext,
    InputFrame, Key,
};

/// The primary-gun edge the one-frame press resolves to.
fn fire_primary() -> Action {
    Action::Flight(FlightCommand::FirePrimary)
}

/// The minimum acceptance scenario: the render frame at tick 10 observes the
/// space bar pressed once, resolves it through the designed map, and the
/// three fixed substeps that frame covers execute it exactly once.
#[test]
fn accept_f22_a_one_frame_key_edge_produces_one_action_across_substeps() {
    let map = ActionMap::designed_default();
    let action = map
        .resolve(InputContext::Flight, BindingSource::Key(Key::Space))
        .expect("space is bound to the primary guns in flight context");
    assert_eq!(action, fire_primary());

    let mut frame = InputFrame::new(Tick(10));
    frame.push_edge(action);

    let mut controls = ControlBuffer::new();
    controls.apply_frame(&frame).expect("the frame applies");
    assert_eq!(controls.pending_edges(), 1);

    let substeps = [
        controls.begin_tick(Tick(10)),
        controls.begin_tick(Tick(11)),
        controls.begin_tick(Tick(12)),
    ];
    assert_eq!(substeps[0], vec![fire_primary()], "fires at its own tick");
    assert!(
        substeps[1].is_empty(),
        "the second substep must not re-fire"
    );
    assert!(substeps[2].is_empty(), "the third substep must not re-fire");
    assert_eq!(
        substeps.iter().map(Vec::len).sum::<usize>(),
        1,
        "one frame edge is exactly one action across every substep"
    );
    assert_eq!(controls.pending_edges(), 0, "nothing is left queued");
}

/// Continuous axes and edges are buffered separately: a held deflection keeps
/// driving every substep while the one edge fires once.
#[test]
fn accept_f22_a_held_axis_and_edge_are_buffered_separately() {
    let map = ActionMap::designed_default();
    let edge = map
        .resolve(InputContext::Flight, BindingSource::Key(Key::Space))
        .expect("space is bound");

    let mut frame = InputFrame::new(Tick(20));
    frame.set_axis(AxisValue::from_unit(FlightCommand::Pitch, 0.5).expect("valid deflection"));
    frame.push_edge(edge);

    let mut controls = ControlBuffer::new();
    controls.apply_frame(&frame).expect("the frame applies");

    let mut edges = 0;
    for tick in 20..23 {
        edges += controls.begin_tick(Tick(tick)).len();
        let pitch = controls
            .axis(FlightCommand::Pitch)
            .expect("pitch was driven");
        assert!(
            (pitch - 0.5).abs() < 1e-4,
            "the held pitch survives substep {tick}"
        );
    }
    assert_eq!(edges, 1, "only the edge is consumed once");
}

/// The failure cases the scenario must reject: a stale frame is refused
/// without mutating state, and a non-axis command or out-of-range value is
/// refused by name instead of being clamped or ignored.
#[test]
fn accept_f22_a_stale_frames_and_malformed_axes_are_refused() {
    let mut controls = ControlBuffer::new();
    let mut first = InputFrame::new(Tick(30));
    first.push_edge(fire_primary());
    controls.apply_frame(&first).expect("first frame applies");

    let stale = InputFrame::new(Tick(29));
    assert_eq!(
        controls.apply_frame(&stale),
        Err(ControlError::OutOfOrderFrame {
            applied: Tick(30),
            received: Tick(29),
        }),
        "a stale frame must be refused by name"
    );
    assert_eq!(
        controls.pending_edges(),
        1,
        "a refused frame changes nothing"
    );

    assert_eq!(
        AxisState::new().set(FlightCommand::FirePrimary, 0.0),
        Err(ControlError::Axis(AxisValueError::NotContinuous {
            command: FlightCommand::FirePrimary,
        }))
    );
    assert_eq!(
        AxisState::new().set(FlightCommand::Pitch, 2.0),
        Err(ControlError::Axis(AxisValueError::OutOfRange {
            command: FlightCommand::Pitch,
            value: 2.0,
        }))
    );
}
