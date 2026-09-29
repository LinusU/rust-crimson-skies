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
//! F22-D; the calibration and device adapters are F22-B and the focus, replay
//! and full ownership wiring are F22-C
//! (`docs/findings/2026-09-29-f22-a-command-schema-and-action-map.md`).

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
}
