//! Command buffering and control ownership (F22-A).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`, stage
//! `### F22-A`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Network
//! ownership table": the server owns physics truth and a client owns only
//! local input requests; "UI transition discipline": a UI action requests a
//! domain transaction).
//!
//! The typed vocabulary lives in [`cs_types::input`]; this module is the
//! simulation-side consumer and adds the two behaviors the sheet makes
//! structural:
//!
//! * **Continuous axes and edge-triggered commands are buffered
//!   separately.** [`AxisState`] holds the latest analog
//!   [`AxisValue`] per axis and keeps it across ticks; [`ControlBuffer`]
//!   queues one-shot [`Action`] edges stamped with the render frame that
//!   produced them and delivers each exactly once — to the first input
//!   boundary whose tick is at or after the frame's tick. A press that
//!   arrived in one render frame therefore produces **one** action across
//!   every physics substep of that frame (F22-A minimum scenario), instead
//!   of being re-fired each substep or lost.
//! * **Player control is owned by exactly one authority.** [`ControlGate`]
//!   refuses a second, different authority until the current one releases,
//!   and it gates local device input by [`InputContext`]: text entry and
//!   cinematics emit nothing, and a remote or scripted actor never treats a
//!   local device as authoritative (non-negotiable behaviors 4 and 5).
//!
//! Every policy and fixture in this module is newly authored project design,
//! not measured original behavior. Which original commands exist, how they
//! are bound and how the original game distributes control are unknown until
//! F22-D; the calibration and device adapters are F22-B, and the focus, replay
//! and control-ownership wiring that drives this buffer is F22-C
//! (`docs/findings/2026-09-29-f22-a-command-schema-and-action-map.md`,
//! `docs/findings/2026-09-30-f22-c-focus-ui-replay-ownership.md`).

use cs_types::Tick;
use cs_types::input::{
    Action, ActionMap, AxisValue, AxisValueError, BindingSource, FlightCommand, InputContext,
    InputFrame,
};

/// Why a control operation was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlError {
    /// A frame carried an invalid axis value.
    Axis(AxisValueError),
    /// A frame's tick was older than the last applied frame; controls only
    /// ever move forward.
    OutOfOrderFrame {
        /// The last frame tick applied.
        applied: Tick,
        /// The rejected frame's tick.
        received: Tick,
    },
    /// A second, different authority tried to take an already-owned actor.
    AuthorityAlreadyOwned {
        /// The current authority.
        existing: ControlAuthority,
        /// The one that was refused.
        requested: ControlAuthority,
    },
    /// A non-owner authority tried to release the actor.
    NotOwner {
        /// The authority that currently owns the actor.
        existing: ControlAuthority,
        /// The authority that tried to release it.
        requested: ControlAuthority,
    },
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Axis(error) => write!(f, "{error}"),
            Self::OutOfOrderFrame { applied, received } => write!(
                f,
                "input frame {} is older than the last applied frame {}",
                received.0, applied.0
            ),
            Self::AuthorityAlreadyOwned {
                existing,
                requested,
            } => write!(
                f,
                "{requested} cannot take control from {existing} without a release"
            ),
            Self::NotOwner {
                existing,
                requested,
            } => write!(f, "{requested} does not own control; {existing} does"),
        }
    }
}

impl std::error::Error for ControlError {}

impl From<AxisValueError> for ControlError {
    fn from(error: AxisValueError) -> Self {
        Self::Axis(error)
    }
}

/// The latest continuous deflection of every driven flight axis.
///
/// An axis holds its value until a later frame replaces it, so a held stick
/// or a key that produces a digital axis keeps driving the aircraft across
/// every substep while an edge is delivered only once ([`ControlBuffer`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisState {
    values: Vec<(FlightCommand, f32)>,
}

impl AxisState {
    /// An all-neutral axis state.
    #[must_use]
    pub const fn new() -> Self {
        Self { values: Vec::new() }
    }

    /// Sets one axis from a normalized `[-1, 1]` value.
    ///
    /// # Errors
    ///
    /// [`ControlError::Axis`] when the command is not a continuous axis, the
    /// value is non-finite or the value is outside `[-1, 1]`.
    pub fn set(&mut self, command: FlightCommand, value: f32) -> Result<(), ControlError> {
        self.apply(AxisValue::from_unit(command, value)?)
    }

    /// Sets one axis from an already quantized sample.
    ///
    /// # Errors
    ///
    /// [`ControlError::Axis`] when the sample names a non-continuous command.
    pub fn apply(&mut self, value: AxisValue) -> Result<(), ControlError> {
        if !value.command().is_continuous() {
            return Err(ControlError::Axis(AxisValueError::NotContinuous {
                command: value.command(),
            }));
        }
        let unit = value.as_unit();
        if let Some(existing) = self
            .values
            .iter_mut()
            .find(|(command, _)| *command == value.command())
        {
            existing.1 = unit;
        } else {
            self.values.push((value.command(), unit));
        }
        Ok(())
    }

    /// The current deflection of `command`, if it has ever been driven.
    #[must_use]
    pub fn value(&self, command: FlightCommand) -> Option<f32> {
        self.values
            .iter()
            .find(|(driven, _)| *driven == command)
            .map(|(_, value)| *value)
    }

    /// Whether every driven axis is neutral (exactly zero).
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.values.iter().all(|(_, value)| *value == 0.0)
    }
}

/// One queued edge and the render frame that observed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingEdge {
    frame_tick: Tick,
    action: Action,
}

/// Why a [`ThrottleSteps`] value was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThrottleError {
    /// The step was NaN or infinite.
    NonFiniteStep {
        /// The rejected step.
        step: f32,
    },
    /// The step was not a fraction: it must be greater than zero and at most
    /// one, so a step can never jump the whole throttle range.
    StepOutOfRange {
        /// The rejected step.
        step: f32,
    },
    /// A throttle position outside `[IDLE, FULL]` was requested. The
    /// simulation's throttle is the flight model's own `0..=1` quantity
    /// (`flight::FlightInput`), not the signed input axis, so a position is
    /// validated rather than clamped behind the caller's back.
    PositionOutOfRange {
        /// The rejected position.
        position: f32,
    },
}

/// The keyboard throttle position and its step rules (F22-B).
///
/// Non-negotiable behavior 2: "keyboard throttle steps and direct settings do
/// not depend on render FPS". That is a property of *where* a step is applied,
/// not of a comment:
///
/// * a step is applied by [`apply_tick`](Self::apply_tick), which runs at the
///   simulation's input boundary, and it moves the throttle by one step per
///   executed [`FlightCommand::ThrottleStepUp`] /
///   [`FlightCommand::ThrottleStepDown`] edge — never by a per-frame rate. A
///   render frame that covered five ticks and delivered one press moves the
///   throttle exactly as five frames that delivered the same press each do;
/// * a direct setting ([`FlightCommand::ThrottleIdle`] /
///   [`FlightCommand::ThrottleFull`]) is applied after the steps of its own
///   tick, so one tick containing both a step and a direct setting ends at the
///   direct setting whatever order the edges arrived in.
///
/// The position is the flight model's `[0, 1]` throttle, not the signed
/// `FlightCommand::Throttle` axis; [`axis_value`](Self::axis_value) converts it
/// for a caller that reports the canonical input axis.
///
/// **Designed, not original.** The step size is a newly authored project
/// default ([`ThrottleSteps::DESIGNED_STEP`]); the original game's keyboard
/// throttle step, its increment and whether it is a step at all are unknown
/// until F22-D measures them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThrottleSteps {
    position: f32,
    step: f32,
}

impl ThrottleSteps {
    /// The idle end of the throttle range.
    pub const IDLE: f32 = 0.0;
    /// The full end of the throttle range.
    pub const FULL: f32 = 1.0;
    /// The designed step, a twentieth of the range.
    pub const DESIGNED_STEP: f32 = 0.05;

    /// A throttle at [`IDLE`](Self::IDLE) with an explicit step.
    ///
    /// # Errors
    ///
    /// [`ThrottleError::NonFiniteStep`] and
    /// [`ThrottleError::StepOutOfRange`] when the step is not a fraction.
    pub fn new(step: f32) -> Result<Self, ThrottleError> {
        if !step.is_finite() {
            return Err(ThrottleError::NonFiniteStep { step });
        }
        if step <= 0.0 || step > Self::FULL {
            return Err(ThrottleError::StepOutOfRange { step });
        }
        Ok(Self {
            position: Self::IDLE,
            step,
        })
    }

    /// The designed starting throttle: idle with
    /// [`DESIGNED_STEP`](Self::DESIGNED_STEP) per press.
    #[must_use]
    pub fn designed_default() -> Self {
        Self {
            position: Self::IDLE,
            step: Self::DESIGNED_STEP,
        }
    }

    /// The step one press moves the throttle by.
    #[must_use]
    pub const fn step(self) -> f32 {
        self.step
    }

    /// The current throttle position in `[IDLE, FULL]`.
    #[must_use]
    pub const fn position(self) -> f32 {
        self.position
    }

    /// The current throttle as the canonical `FlightCommand::Throttle` axis
    /// value: `(position + 1) / 2` mapped back into `[-1, 1]`.
    #[must_use]
    pub fn axis_value(self) -> f32 {
        self.position * 2.0 - 1.0
    }

    /// Sets the position directly, for a caller that owns an analog throttle.
    ///
    /// # Errors
    ///
    /// [`ThrottleError::PositionOutOfRange`] outside `[IDLE, FULL]`. The
    /// position is never clamped silently.
    pub fn set_position(&mut self, position: f32) -> Result<(), ThrottleError> {
        if !position.is_finite() || !(Self::IDLE..=Self::FULL).contains(&position) {
            return Err(ThrottleError::PositionOutOfRange { position });
        }
        self.position = position;
        Ok(())
    }

    /// Applies the throttle edges one tick executed, and returns the change
    /// each one made.
    ///
    /// The edges are the ones [`ControlBuffer::begin_tick`] returned for that
    /// tick, in the order they executed. Every step moves the throttle by one
    /// [`step`](Self::step) and saturates at the ends; a direct setting in the
    /// same tick is applied after that tick's steps, so it wins over them in
    /// whatever order the edges arrived.
    pub fn apply_tick(&mut self, edges: &[Action]) -> Vec<ThrottleChange> {
        let mut changes = Vec::new();
        let mut direct: Option<(FlightCommand, f32)> = None;
        for edge in edges {
            match edge {
                Action::Flight(FlightCommand::ThrottleStepUp) => {
                    changes.push(self.step_by(self.step, FlightCommand::ThrottleStepUp));
                }
                Action::Flight(FlightCommand::ThrottleStepDown) => {
                    changes.push(self.step_by(-self.step, FlightCommand::ThrottleStepDown));
                }
                Action::Flight(FlightCommand::ThrottleIdle) => {
                    direct = Some((FlightCommand::ThrottleIdle, Self::IDLE));
                }
                Action::Flight(FlightCommand::ThrottleFull) => {
                    direct = Some((FlightCommand::ThrottleFull, Self::FULL));
                }
                _ => {}
            }
        }
        if let Some((cause, position)) = direct {
            let from = self.position;
            self.position = position;
            changes.push(ThrottleChange {
                cause,
                from,
                to: position,
            });
        }
        changes
    }

    /// Moves the throttle by `delta`, saturating at the ends, and reports the
    /// change. A step refused by an end still produces a change record with
    /// `from == to`, so a trace shows the press that did nothing.
    fn step_by(&mut self, delta: f32, cause: FlightCommand) -> ThrottleChange {
        let from = self.position;
        let to = (self.position + delta).clamp(Self::IDLE, Self::FULL);
        self.position = to;
        ThrottleChange { cause, from, to }
    }
}

impl Default for ThrottleSteps {
    fn default() -> Self {
        Self::designed_default()
    }
}

/// One throttle change a tick's edges produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThrottleChange {
    /// The edge that caused it.
    pub cause: FlightCommand,
    /// The position before the change.
    pub from: f32,
    /// The position after the change.
    pub to: f32,
}

/// The simulation's inbound control buffer.
///
/// [`apply_frame`](Self::apply_frame) folds one render frame into the buffer;
/// [`begin_tick`](Self::begin_tick) is the input boundary of one simulation
/// tick and returns the edges that tick must execute. An edge is consumed by
/// the first boundary at or after its frame tick and never returned again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlBuffer {
    pending: Vec<PendingEdge>,
    axes: AxisState,
    last_applied: Option<Tick>,
}

impl ControlBuffer {
    /// An empty buffer with neutral axes.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            axes: AxisState::new(),
            last_applied: None,
        }
    }

    /// Folds a render frame into the buffer.
    ///
    /// Continuous axes are applied immediately (they describe the current
    /// deflection); the frame's edges are queued for the input boundary of
    /// its tick. A frame older than the last applied one is refused so a
    /// late callback cannot rewrite already-committed controls.
    ///
    /// # Errors
    ///
    /// [`ControlError::OutOfOrderFrame`] for a frame older than the last
    /// applied one, and [`ControlError::Axis`] for an invalid axis value.
    /// Nothing is mutated when a frame is refused.
    pub fn apply_frame(&mut self, frame: &InputFrame) -> Result<(), ControlError> {
        if let Some(applied) = self.last_applied
            && frame.frame_tick() < applied
        {
            return Err(ControlError::OutOfOrderFrame {
                applied,
                received: frame.frame_tick(),
            });
        }
        // Validate every axis before mutating any, so a bad frame is atomic.
        for axis in frame.axes() {
            if !axis.command().is_continuous() {
                return Err(ControlError::Axis(AxisValueError::NotContinuous {
                    command: axis.command(),
                }));
            }
        }
        for axis in frame.axes() {
            self.axes.apply(*axis)?;
        }
        let frame_tick = frame.frame_tick();
        for edge in frame.edges() {
            self.pending.push(PendingEdge {
                frame_tick,
                action: *edge,
            });
        }
        self.last_applied = Some(frame_tick);
        Ok(())
    }

    /// The input boundary of `tick`: consumes and returns every queued edge
    /// whose frame tick is at or before `tick`, each exactly once.
    ///
    /// Continuous axes are deliberately untouched, so the same deflection
    /// keeps driving the aircraft on every substep.
    pub fn begin_tick(&mut self, tick: Tick) -> Vec<Action> {
        let mut taken = Vec::new();
        let mut kept = Vec::new();
        for edge in self.pending.drain(..) {
            if edge.frame_tick <= tick {
                taken.push(edge.action);
            } else {
                kept.push(edge);
            }
        }
        self.pending = kept;
        taken
    }

    /// The current continuous axes.
    #[must_use]
    pub const fn axes(&self) -> &AxisState {
        &self.axes
    }

    /// The current deflection of one axis.
    #[must_use]
    pub fn axis(&self, command: FlightCommand) -> Option<f32> {
        self.axes.value(command)
    }

    /// How many edges are still waiting for a boundary.
    #[must_use]
    pub fn pending_edges(&self) -> usize {
        self.pending.len()
    }

    /// Discards the edges that are still waiting for a boundary and returns
    /// them (F22-C).
    ///
    /// This is the teardown half of "an edge is consumed by the first input
    /// boundary at or after its frame tick": an edge discarded here was never
    /// delivered, so control ownership can change without the new owner
    /// executing the previous owner's queued press. A focus loss, a pause, a
    /// control handover and a teardown all drop their queued edges through this
    /// one call, and the returned record says what was dropped instead of
    /// letting a press vanish silently.
    pub fn drain_pending(&mut self) -> Vec<Action> {
        let drained = std::mem::take(&mut self.pending);
        drained.into_iter().map(|edge| edge.action).collect()
    }

    /// Sets every driven axis to exactly neutral and returns the commands it
    /// changed (F22-C).
    ///
    /// [`AxisState`] holds a deflection until a later frame replaces it, so a
    /// session that stops polling — a focus loss, a pause, a control handover,
    /// a menu — must state the neutral explicitly or the aircraft keeps the
    /// stick's last deflection forever. This is the same rule the device
    /// adapters apply at the end of every finished frame, for the paths where
    /// there is no next frame.
    ///
    /// Only the axes that were not already exactly neutral are reported, so a
    /// second handover has nothing to claim and a trace shows the release that
    /// actually changed something.
    pub fn neutralize_axes(&mut self) -> Vec<FlightCommand> {
        let mut neutralized = Vec::new();
        for (command, value) in &mut self.axes.values {
            if *value != 0.0 {
                *value = 0.0;
                neutralized.push(*command);
            }
        }
        neutralized
    }
}

/// A stable identity for one local player seat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalSeatId(pub u32);

impl std::fmt::Display for LocalSeatId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "local-seat:{}", self.0)
    }
}

/// Which authority produces an actor's controls.
///
/// The local seat is the only authority for which local device input is
/// authoritative; a remote (server-owned) actor treats local input as a
/// request, and a scripted actor ignores it entirely
/// (`docs/contracts/UI-NETWORK.md`, "Network ownership table").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlAuthority {
    /// A local seat drives the actor from local devices.
    LocalSeat(LocalSeatId),
    /// The server or another host owns the actor's truth.
    RemoteAuthority,
    /// A script or cinematic drives the actor.
    Scripted,
}

impl ControlAuthority {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalSeat(_) => "local_seat",
            Self::RemoteAuthority => "remote_authority",
            Self::Scripted => "scripted",
        }
    }
}

impl std::fmt::Display for ControlAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalSeat(seat) => write!(f, "local-seat:{}", seat.0),
            Self::RemoteAuthority => f.write_str("remote-authority"),
            Self::Scripted => f.write_str("scripted"),
        }
    }
}

/// The exclusive control ownership and input context of one actor.
///
/// Ownership is a single slot: at most one authority holds an actor at a
/// time, acquiring a different one while it is held is refused, and only the
/// holder may release it. The context is the gate of non-negotiable behaviors
/// 4 and 5.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlGate {
    authority: Option<ControlAuthority>,
    context: InputContext,
}

impl Default for ControlGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlGate {
    /// An unowned gate in [`InputContext::Cinematic`], so nothing is
    /// accepted until ownership and an active context are set explicitly.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            authority: None,
            context: InputContext::Cinematic,
        }
    }

    /// The current owner, if any.
    #[must_use]
    pub const fn authority(&self) -> Option<ControlAuthority> {
        self.authority
    }

    /// The current input context.
    #[must_use]
    pub const fn context(&self) -> InputContext {
        self.context
    }

    /// Sets the input context; see [`InputContext::accepts`].
    pub fn set_context(&mut self, context: InputContext) {
        self.context = context;
    }

    /// Takes ownership for `authority`.
    ///
    /// Re-assigning the current holder is a no-op success; a different
    /// authority is refused until the holder releases.
    ///
    /// # Errors
    ///
    /// [`ControlError::AuthorityAlreadyOwned`].
    pub fn assign(&mut self, authority: ControlAuthority) -> Result<(), ControlError> {
        match self.authority {
            Some(existing) if existing != authority => Err(ControlError::AuthorityAlreadyOwned {
                existing,
                requested: authority,
            }),
            _ => {
                self.authority = Some(authority);
                Ok(())
            }
        }
    }

    /// Releases ownership on behalf of `authority`.
    ///
    /// # Errors
    ///
    /// [`ControlError::NotOwner`] when a different authority holds the actor,
    /// and the same when nobody holds it.
    pub fn release(&mut self, authority: ControlAuthority) -> Result<(), ControlError> {
        match self.authority {
            Some(existing) if existing == authority => {
                self.authority = None;
                Ok(())
            }
            Some(existing) => Err(ControlError::NotOwner {
                existing,
                requested: authority,
            }),
            None => Err(ControlError::NotOwner {
                existing: authority,
                requested: authority,
            }),
        }
    }

    /// Whether a local device may produce an authoritative command right now:
    /// a local seat owns the actor and the context is not text entry or a
    /// cinematic.
    #[must_use]
    pub const fn accepts_local_input(&self) -> bool {
        matches!(self.authority, Some(ControlAuthority::LocalSeat(_)))
            && !matches!(
                self.context,
                InputContext::TextEntry | InputContext::Cinematic
            )
    }

    /// Resolves `source` through `map` only when this gate accepts local
    /// input.
    ///
    /// This is the single place the two gates compose: an actor driven
    /// remotely or by a script, or a local actor in text entry, receives no
    /// command even if the source is bound.
    #[must_use]
    pub fn resolve(&self, map: &ActionMap, source: BindingSource) -> Option<Action> {
        if !self.accepts_local_input() {
            return None;
        }
        map.resolve(self.context, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::input::{Key, MouseButton};

    fn fire_primary() -> Action {
        Action::Flight(FlightCommand::FirePrimary)
    }

    /// AC01, the F22-A minimum scenario: one render frame observes one key
    /// edge, and across several physics substeps that edge produces exactly
    /// one action. An implementation that re-fired the edge per substep
    /// would return three, and one that dropped it would return none.
    #[test]
    fn accept_f22_a_one_frame_key_edge_produces_one_action_across_substeps() {
        let map = ActionMap::designed_default();
        let action = map
            .resolve(InputContext::Flight, BindingSource::Key(Key::Space))
            .expect("space is bound to the primary guns");
        assert_eq!(action, fire_primary());

        // The render frame at tick 10 saw the press exactly once.
        let mut frame = InputFrame::new(Tick(10));
        frame.push_edge(action);

        let mut buffer = ControlBuffer::new();
        buffer.apply_frame(&frame).expect("the frame applies");
        assert_eq!(buffer.pending_edges(), 1);

        // The frame's wall time covers three fixed substeps.
        let first = buffer.begin_tick(Tick(10));
        let second = buffer.begin_tick(Tick(11));
        let third = buffer.begin_tick(Tick(12));

        assert_eq!(first, vec![fire_primary()], "the edge fires at its tick");
        assert!(second.is_empty(), "the second substep must not re-fire");
        assert!(third.is_empty(), "the third substep must not re-fire");
        assert_eq!(
            first.len() + second.len() + third.len(),
            1,
            "a one-frame edge produces exactly one action across substeps"
        );
        assert_eq!(buffer.pending_edges(), 0);
    }

    /// Continuous axes and edges have separate buffering: an axis keeps
    /// driving every substep while an edge is delivered once, and a frame
    /// that only deflects an axis emits no action.
    #[test]
    fn accept_f22_a_continuous_axes_are_buffered_separately_from_edges() {
        let mut frame = InputFrame::new(Tick(4));
        frame.set_axis(
            AxisValue::from_unit(FlightCommand::Pitch, -0.5).expect("a valid deflection"),
        );
        frame
            .set_axis(AxisValue::from_unit(FlightCommand::Roll, 0.25).expect("a valid deflection"));

        let mut buffer = ControlBuffer::new();
        buffer.apply_frame(&frame).expect("the frame applies");
        assert!(
            buffer.begin_tick(Tick(4)).is_empty(),
            "no edge in this frame"
        );
        assert_eq!(buffer.pending_edges(), 0);

        for tick in 4..7 {
            buffer.begin_tick(Tick(tick));
            let pitch = buffer.axis(FlightCommand::Pitch).expect("pitch was driven");
            assert!(
                (pitch + 0.5).abs() < 1e-4,
                "the held pitch survives substep {tick}"
            );
            assert!((buffer.axis(FlightCommand::Roll).expect("roll") - 0.25).abs() < 1e-4);
            assert_eq!(
                buffer.axis(FlightCommand::Throttle),
                None,
                "an undriven axis has no value"
            );
        }

        // A later neutral frame recentres the axis.
        let mut neutral = InputFrame::new(Tick(7));
        neutral.set_axis(AxisValue::from_unit(FlightCommand::Pitch, 0.0).expect("neutral"));
        buffer.apply_frame(&neutral).expect("the frame applies");
        assert_eq!(buffer.axis(FlightCommand::Pitch), Some(0.0));
    }

    /// A late frame is refused and cannot rewrite committed controls; an
    /// edge stamped for a later tick waits for its own boundary.
    #[test]
    fn accept_f22_a_out_of_order_frames_are_refused_and_late_edges_wait() {
        let mut buffer = ControlBuffer::new();
        let mut newer = InputFrame::new(Tick(10));
        newer.push_edge(fire_primary());
        buffer.apply_frame(&newer).expect("the first frame applies");

        let stale = InputFrame::new(Tick(9));
        assert_eq!(
            buffer.apply_frame(&stale),
            Err(ControlError::OutOfOrderFrame {
                applied: Tick(10),
                received: Tick(9),
            })
        );
        assert_eq!(
            buffer.pending_edges(),
            1,
            "the refused frame changed nothing"
        );

        let mut later = InputFrame::new(Tick(12));
        later.push_edge(Action::Flight(FlightCommand::Eject));
        buffer.apply_frame(&later).expect("a later frame applies");

        // At tick 10 only the first edge is due; the tick-12 edge waits.
        assert_eq!(buffer.begin_tick(Tick(10)), vec![fire_primary()]);
        assert_eq!(buffer.pending_edges(), 1);
        assert_eq!(
            buffer.begin_tick(Tick(11)),
            Vec::<Action>::new(),
            "the later edge is not delivered early"
        );
        assert_eq!(
            buffer.begin_tick(Tick(12)),
            vec![Action::Flight(FlightCommand::Eject)]
        );

        // The public axis setters refuse malformed input by name.
        assert_eq!(
            AxisState::new().set(FlightCommand::FirePrimary, 0.0),
            Err(ControlError::Axis(AxisValueError::NotContinuous {
                command: FlightCommand::FirePrimary
            }))
        );
        assert_eq!(
            AxisState::new().set(FlightCommand::Pitch, 2.0),
            Err(ControlError::Axis(AxisValueError::OutOfRange {
                command: FlightCommand::Pitch,
                value: 2.0
            }))
        );

        // A frame with a valid axis still applies after the failures.
        let mut bad = InputFrame::new(Tick(13));
        bad.set_axis(AxisValue::from_quantized(FlightCommand::Pitch, 100).expect("valid"));
        assert!(buffer.apply_frame(&bad).is_ok());
    }

    /// Non-negotiable behaviors 4 and 5: exactly one authority owns the
    /// actor, a different authority is refused until release, and text entry
    /// or a non-local owner suppresses local commands.
    #[test]
    fn accept_f22_a_control_gate_allows_exactly_one_authority() {
        let map = ActionMap::designed_default();
        let seat = ControlAuthority::LocalSeat(LocalSeatId(0));
        let other = ControlAuthority::LocalSeat(LocalSeatId(1));

        let mut gate = ControlGate::new();
        assert_eq!(gate.authority(), None);
        assert!(
            !gate.accepts_local_input(),
            "an unowned gate accepts nothing"
        );

        gate.assign(seat).expect("the first seat takes control");
        assert_eq!(gate.authority(), Some(seat));
        assert_eq!(
            gate.assign(other),
            Err(ControlError::AuthorityAlreadyOwned {
                existing: seat,
                requested: other,
            })
        );
        assert_eq!(
            gate.assign(seat),
            Ok(()),
            "re-assigning the holder is a no-op"
        );

        gate.set_context(InputContext::Flight);
        assert!(gate.accepts_local_input());
        assert_eq!(
            gate.resolve(&map, BindingSource::Key(Key::Space)),
            Some(fire_primary())
        );

        gate.set_context(InputContext::TextEntry);
        assert!(!gate.accepts_local_input());
        assert_eq!(
            gate.resolve(&map, BindingSource::Key(Key::Space)),
            None,
            "text entry cannot also fire weapons"
        );

        gate.set_context(InputContext::Flight);
        assert_eq!(
            gate.release(other),
            Err(ControlError::NotOwner {
                existing: seat,
                requested: other,
            })
        );
        gate.release(seat).expect("the holder releases control");
        assert_eq!(gate.authority(), None);
        gate.assign(other)
            .expect("the actor can be reassigned after release");
        assert_eq!(gate.authority(), Some(other));

        // A remote or scripted actor never treats local input as
        // authoritative.
        for authority in [
            ControlAuthority::RemoteAuthority,
            ControlAuthority::Scripted,
        ] {
            let mut remote = ControlGate::new();
            remote.assign(authority).expect("assigns");
            remote.set_context(InputContext::Flight);
            assert!(!remote.accepts_local_input());
            assert_eq!(remote.resolve(&map, BindingSource::Key(Key::Space)), None);
        }

        // A local seat bound to a mouse button resolves through the same gate.
        let mut mouse = ControlGate::new();
        mouse.assign(seat).expect("assigns");
        mouse.set_context(InputContext::Flight);
        assert_eq!(
            mouse.resolve(&map, BindingSource::MouseButton(MouseButton::Left)),
            Some(fire_primary())
        );
    }

    fn step_up() -> Action {
        Action::Flight(FlightCommand::ThrottleStepUp)
    }

    fn step_down() -> Action {
        Action::Flight(FlightCommand::ThrottleStepDown)
    }

    /// Ticks the frame-rate traces below run for.
    const THROTTLE_TICKS: u64 = 60;

    /// Replays one keyboard-throttle trace over [`THROTTLE_TICKS`] ticks,
    /// `ticks_per_frame` ticks per render frame, with one `ThrottleStepUp` in
    /// every `press_period`-th frame, and returns the final position and the
    /// number of throttle changes.
    ///
    /// The production path is the whole one: an [`InputFrame`] per render
    /// frame, folded into a [`ControlBuffer`], drained once per fixed tick and
    /// handed to [`ThrottleSteps`].
    fn throttle_run(ticks_per_frame: u64, press_period: u64) -> (f32, usize) {
        let mut buffer = ControlBuffer::new();
        let mut steps = ThrottleSteps::designed_default();
        let mut changes = 0;
        let mut tick = Tick(0);
        let mut frame_index = 0_u64;
        while tick.0 < THROTTLE_TICKS {
            let mut frame = InputFrame::new(tick);
            if frame_index.is_multiple_of(press_period) {
                frame.push_edge(step_up());
            }
            buffer.apply_frame(&frame).expect("the frame applies");
            for _ in 0..ticks_per_frame {
                changes += steps.apply_tick(&buffer.begin_tick(tick)).len();
                tick = Tick(tick.0 + 1);
            }
            frame_index += 1;
        }
        (steps.position(), changes)
    }

    /// Non-negotiable behavior 2: a throttle step is applied where a press is
    /// executed, at the input boundary, so the same input trace produces the
    /// same throttle however the render frames are grouped. A per-frame
    /// increment, or a step per frame instead of per press, would give the
    /// coarser and the finer run different results.
    #[test]
    fn accept_f22_b_keyboard_throttle_steps_ignore_render_frame_grouping() {
        // Twelve presses over 60 ticks, delivered as 12 frames of 5 ticks (a
        // 12 FPS render loop at 60 Hz) and as 60 frames of 1 tick with a press
        // in every fifth frame (a 60 FPS render loop at 60 Hz).
        let coarse = throttle_run(5, 1);
        let fine = throttle_run(1, 5);
        assert_eq!(coarse, fine, "the frame grouping must not matter");
        assert_eq!(
            coarse.1, 12,
            "twelve presses are twelve changes, whatever the frames looked like"
        );
        assert!(
            (coarse.0 - ThrottleSteps::DESIGNED_STEP * 12.0).abs() < 1e-6,
            "twelve presses move the throttle twelve steps, got {}",
            coarse.0
        );

        // A run that presses in every frame is a different trace and must give
        // a different result, so the agreement above is not vacuous.
        let every_frame = throttle_run(1, 1);
        assert_eq!(every_frame.1, 60, "sixty presses are sixty changes");
        assert_eq!(
            every_frame.0,
            ThrottleSteps::FULL,
            "the throttle saturates at the full end"
        );

        // The changes are reported, and a step refused by an end is visible as
        // a change that moved nothing.
        let mut steps = ThrottleSteps::designed_default();
        let changes = steps.apply_tick(&[step_up(), step_up()]);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].cause, FlightCommand::ThrottleStepUp);
        assert_eq!(changes[0].from, 0.0);
        assert!((changes[0].to - ThrottleSteps::DESIGNED_STEP).abs() < 1e-6);
        let down = steps.apply_tick(&[step_down(), step_down(), step_down()]);
        assert!(
            (down[0].to - ThrottleSteps::DESIGNED_STEP).abs() < 1e-6,
            "the first step down moves the throttle back one step, got {}",
            down[0].to
        );
        assert_eq!(
            *down.last().expect("the trace is not empty"),
            ThrottleChange {
                cause: FlightCommand::ThrottleStepDown,
                from: 0.0,
                to: 0.0,
            },
            "idle cannot go lower, and the refused step is still reported"
        );

        // A direct setting wins over the steps of its own tick, in any order.
        for edges in [
            vec![Action::Flight(FlightCommand::ThrottleIdle), step_up()],
            vec![step_up(), Action::Flight(FlightCommand::ThrottleIdle)],
        ] {
            let mut steps = ThrottleSteps::designed_default();
            steps
                .set_position(0.5)
                .expect("half throttle is a valid position");
            steps.apply_tick(&edges);
            assert_eq!(
                steps.position(),
                ThrottleSteps::IDLE,
                "a direct setting overrides the same tick's steps"
            );
        }
        let mut full = ThrottleSteps::designed_default();
        full.apply_tick(&[Action::Flight(FlightCommand::ThrottleFull)]);
        assert_eq!(full.position(), ThrottleSteps::FULL);
        assert!(
            (full.axis_value() - 1.0).abs() < 1e-6,
            "the full end maps to +1 on the canonical axis"
        );
        assert!(
            (ThrottleSteps::designed_default().axis_value() + 1.0).abs() < 1e-6,
            "the idle end maps to -1 on the canonical axis"
        );
    }

    /// The throttle's own errors are named, and a position is never clamped
    /// behind the caller's back.
    #[test]
    fn accept_f22_b_keyboard_throttle_refuses_malformed_steps_and_positions() {
        assert!(
            matches!(
                ThrottleSteps::new(f32::NAN),
                Err(ThrottleError::NonFiniteStep { step }) if step.is_nan()
            ),
            "a NaN step is refused by name"
        );
        assert_eq!(
            ThrottleSteps::new(0.0),
            Err(ThrottleError::StepOutOfRange { step: 0.0 })
        );
        assert_eq!(
            ThrottleSteps::new(1.5),
            Err(ThrottleError::StepOutOfRange { step: 1.5 })
        );
        assert!(
            ThrottleSteps::new(1.0).is_ok(),
            "a full-range step is valid"
        );

        let mut steps = ThrottleSteps::designed_default();
        assert_eq!(
            steps.set_position(1.5),
            Err(ThrottleError::PositionOutOfRange { position: 1.5 })
        );
        assert_eq!(
            steps.position(),
            ThrottleSteps::IDLE,
            "a refused position changes nothing"
        );
        assert_eq!(steps.step(), ThrottleSteps::DESIGNED_STEP);
    }

    /// F22-C: the teardown half of the buffer. A handover ends local control,
    /// and it has to be able to (a) drop the edges that no boundary has
    /// delivered, so the next owner cannot execute the previous owner's press,
    /// and (b) state the neutral for every axis the buffer holds, because
    /// `AxisState` only ever moves an axis a frame names and a session that
    /// stops polling has no next frame.
    #[test]
    fn accept_f22_c_draining_and_neutralizing_end_the_local_hold() {
        let mut buffer = ControlBuffer::new();
        let mut frame = InputFrame::new(Tick(5));
        frame.push_edge(fire_primary());
        frame.push_edge(Action::Flight(FlightCommand::Eject));
        frame.set_axis(
            AxisValue::from_unit(FlightCommand::Pitch, -0.5).expect("a valid deflection"),
        );
        frame.set_axis(
            AxisValue::from_unit(FlightCommand::Throttle, 1.0).expect("full scale is valid"),
        );
        buffer.apply_frame(&frame).expect("the frame applies");
        assert_eq!(buffer.pending_edges(), 2);

        // The handover: the queued presses are gone and the axes are neutral.
        let discarded = buffer.drain_pending();
        assert_eq!(
            discarded,
            vec![fire_primary(), Action::Flight(FlightCommand::Eject)],
            "an undelivered edge is discarded in observation order, and reported"
        );
        assert_eq!(buffer.pending_edges(), 0);
        assert_eq!(
            buffer.begin_tick(Tick(5)),
            Vec::<Action>::new(),
            "a discarded edge is never delivered, not even later"
        );

        let mut neutralized = buffer.neutralize_axes();
        neutralized.sort();
        assert_eq!(
            neutralized,
            vec![FlightCommand::Pitch, FlightCommand::Throttle],
            "every held axis is stated neutral and named"
        );
        assert_eq!(buffer.axis(FlightCommand::Pitch), Some(0.0));
        assert_eq!(buffer.axis(FlightCommand::Throttle), Some(0.0));
        assert!(buffer.axes().is_neutral());

        // A second handover has nothing left to do and says so, so a teardown
        // that runs twice cannot deadlock or claim it released something.
        assert!(buffer.drain_pending().is_empty());
        assert!(
            buffer.neutralize_axes().is_empty(),
            "an axis that is already neutral is not reported as neutralized"
        );

        // A frame after the handover still works: the buffer is usable, not
        // poisoned.
        let mut fresh = InputFrame::new(Tick(6));
        fresh.push_edge(fire_primary());
        fresh
            .set_axis(AxisValue::from_unit(FlightCommand::Roll, 0.25).expect("a valid deflection"));
        buffer.apply_frame(&fresh).expect("the frame applies");
        assert_eq!(buffer.begin_tick(Tick(6)), vec![fire_primary()]);
        assert!(
            (buffer.axis(FlightCommand::Roll).expect("roll") - 0.25).abs() < 1e-4,
            "and the new deflection is held again"
        );
        assert_eq!(buffer.axis(FlightCommand::Pitch), Some(0.0));
    }
}
