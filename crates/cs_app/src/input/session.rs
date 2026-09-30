//! The local input session: focus, UI, control ownership and replay (F22-C).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`, stage
//! `### F22-C`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! F22-A typed the commands, F22-B built the device adapters. Both stopped one
//! step short of a running loop: something outside the crate still had to
//! decide *which* context owns the devices right now, when a focus loss pauses
//! the session or only neutralizes it, where a UI action goes, which authority
//! may own an actor, and what happens to the input that was in flight when any
//! of that changed. This module is that missing step: one [`InputSession`]
//! that owns the producer ([`InputCollector`] and the
//! [`DeviceAdapters`](super::DeviceAdapters)) **and** the consumer
//! ([`ControlBuffer`], [`ControlGate`], [`ThrottleSteps`]) plus the rules that
//! connect them.
//!
//! # The rules this makes structural
//!
//! 1. **One owner of the context.** [`InputSession::set_context`] is the only
//!    way to change the session's [`InputContext`], and it writes the
//!    collector's bindings *and* the control gate together. A menu, a text
//!    field, a pause screen and the simulation therefore cannot disagree about
//!    who owns the devices — the disagreement that would let a text field fire
//!    the guns (non-negotiable behavior 5).
//! 2. **Focus loss pauses where it may and neutralizes where it may not**
//!    (non-negotiable behavior 4). [`InputSession::set_focus`] releases every
//!    hold and neutralizes every axis in *both* directions — the collector's
//!    devices and the simulation's buffer — and switches the context to
//!    [`InputContext::Cinematic`], so the whole path is closed. The session is
//!    then **paused where that is allowed** — a single-player session, whose
//!    pause the local process owns — and only neutralized in a networked
//!    session, which has no local pause authority
//!    ([`PauseDecision::NoLocalAuthority`]). The input path has no network
//!    channel at all, so "do not pause the server" is structural rather than a
//!    promise.
//! 3. **A UI action is a request, never a command**
//!    (`docs/contracts/UI-NETWORK.md`, "UI transition discipline"). Every
//!    [`Action::Ui`] leaves the frame at the producer/consumer boundary and
//!    becomes a [`UiRequest`] for the screen or pause path; it never enters the
//!    control buffer, and the input session never performs it. The session owns
//!    no campaign, ownership or objective field.
//! 4. **Exactly one authority, and a handover is a release**
//!    (`docs/contracts/UI-NETWORK.md`, "Network ownership table"). The gate is
//!    the single owner; local device input reaches the buffer only while it
//!    accepts it, and a frame that carries content the gate refuses is
//!    reported as an [`InputFault::NotAuthoritative`] rather than dropped
//!    quietly. Losing the actor — to the server, to a script, at a pause or at
//!    teardown — runs one internal handover: holds released,
//!    undelivered edges discarded, axes neutralized, so the next owner can
//!    never execute the previous owner's queued press.
//! 5. **Every fault is reported, and no frame is lost with it.** A refused
//!    device event, content this session's policy suppressed, ticks asked for
//!    while paused and a refused stream record are all [`InputFault`]s the
//!    caller drains. The input the same frame collected from the other devices
//!    still reaches the simulation. Content that is reported as suppressed is
//!    also **not** applied: a closed path reports the frame it refused and hands
//!    the simulation nothing, so "suppressed" can never mean "applied and
//!    ignored". The one thing a closed path still applies is an inert frame —
//!    empty, or restating an axis as exactly neutral — because that is what
//!    stops the last deflection sticking.
//! 6. **Replay is the recorded stream, and the stream is quantized** (AC03).
//!    [`InputSession::start_recording`] records, at the input boundary, exactly
//!    what the consumer executed on each tick as a [`CommandStream`].
//!    [`CommandReplay`] feeds that stream back through the *same* pump at any
//!    display rate, driven by a real [`cs_sim::time::SimClock`], and the
//!    ordered command sequence, the throttle trace and the final axis state
//!    must not depend on how the wall time was cut into render frames. A paused
//!    session runs no boundary, so the replay's clock freezes with it and the
//!    window is re-read after the resume rather than consumed and dropped.
//!
//! # Designed, not original
//!
//! Every mode, focus policy, pause reason, context transition and replay rule
//! here is newly authored project design. What the original 2000 PC game does
//! when the window loses focus, whether it pauses at all, whether it shows a
//! pause menu, where it stores its device configuration and whether it recorded
//! or replayed input at all are **unknown** until F22-D or a measured
//! reference; nothing here claims otherwise
//! (`docs/findings/2026-09-30-f22-c-focus-ui-replay-ownership.md`).
//!
//! The module is plain typed state, ECS-free, so a headless test drives it
//! exactly like a render loop and no game state hides in it
//! (`docs/01-ARCHITECTURE.md`). A Bevy system reads `ButtonInput`, `Gamepad`,
//! the mouse accumulation, the window's focus events and `Time`, and calls
//! [`InputSession::pump_frame`] once per render frame; it owns no policy.

use std::fmt;
use std::time::Duration;

use cs_sim::control::{
    ControlAuthority, ControlBuffer, ControlError, ControlGate, LocalSeatId, ThrottleSteps,
};
use cs_sim::time::{ClockPolicy, SimClock, TickRate, TimeError};
use cs_types::Tick;
use cs_types::input::{
    Action, ActionMap, AxisValue, AxisValueError, BindingSource, CommandStream, FlightCommand,
    InputContext, InputFrame, StreamError, UiAction,
};

use super::devices::{AdapterError, DeviceEvent, DeviceLoss, SuppressedHolds};
use super::{InputBindings, InputCollector};

/// Whether the local session is a single-player or a networked one.
///
/// The mode is the *only* thing that decides whether a focus loss may pause the
/// session locally, which is what makes non-negotiable behavior 4 ("in
/// multiplayer it neutralizes local input without pausing the server")
/// structural rather than a convention: a networked session has no local pause
/// authority to exercise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionMode {
    /// One local seat owns the world; the local process may pause it.
    SinglePlayer,
    /// The server owns the world; the local client may not pause it.
    Multiplayer,
}

impl SessionMode {
    /// Every mode, in a stable order.
    pub const ALL: &'static [SessionMode] = &[Self::SinglePlayer, Self::Multiplayer];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SinglePlayer => "single_player",
            Self::Multiplayer => "multiplayer",
        }
    }

    /// Whether the local process may pause the session on a focus loss.
    #[must_use]
    pub const fn may_pause_locally(self) -> bool {
        matches!(self, Self::SinglePlayer)
    }

    /// The clock policy this mode's session runs under.
    #[must_use]
    pub const fn clock_policy(self) -> ClockPolicy {
        match self {
            Self::SinglePlayer => ClockPolicy::single_player_simulation(),
            Self::Multiplayer => ClockPolicy::multiplayer_simulation(),
        }
    }
}

impl fmt::Display for SessionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why the local session is paused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PauseReason {
    /// The window lost focus.
    FocusLost,
    /// The player asked for it, through a UI action the pause path performed.
    PlayerRequest,
    /// A menu or a screen took the session out of flight.
    Menu,
}

impl PauseReason {
    /// Every reason, in a stable order.
    pub const ALL: &'static [PauseReason] = &[Self::FocusLost, Self::PlayerRequest, Self::Menu];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FocusLost => "focus_lost",
            Self::PlayerRequest => "player_request",
            Self::Menu => "menu",
        }
    }
}

impl fmt::Display for PauseReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a pause request did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PauseDecision {
    /// The local session is now paused for the given reason.
    Paused(PauseReason),
    /// It was already paused; the existing reason stands.
    AlreadyPaused(PauseReason),
    /// This mode has no local pause authority, so nothing was paused. The
    /// session's ticks keep running: the server is never asked to pause
    /// (`docs/contracts/UI-NETWORK.md`, "Network ownership table").
    NoLocalAuthority,
    /// Nothing was paused and nothing had to be.
    Unchanged,
}

impl PauseDecision {
    /// Whether the local session is paused after this decision.
    #[must_use]
    pub const fn is_paused(self) -> bool {
        matches!(self, Self::Paused(_) | Self::AlreadyPaused(_))
    }
}

/// What a pause request did, and what entering the pause released.
#[derive(Clone, Debug, PartialEq)]
pub struct PauseOutcome {
    /// The decision itself.
    pub decision: PauseDecision,
    /// What releasing the input path for the pause gave up: the holds that were
    /// released, the edges that were discarded and the axes that were set to
    /// neutral. Empty when the decision did not release anything.
    pub released: ControlHandover,
}

/// Why a handover released the local input path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandoverReason {
    /// The window lost focus.
    FocusLost,
    /// The session was paused, or a pause ended.
    Paused,
    /// Another authority took the actor.
    OwnershipLost,
    /// The session was torn down.
    Teardown,
    /// A torn-down session was re-armed.
    Restarted,
}

impl HandoverReason {
    /// Every reason, in a stable order.
    pub const ALL: &'static [HandoverReason] = &[
        Self::FocusLost,
        Self::Paused,
        Self::OwnershipLost,
        Self::Teardown,
        Self::Restarted,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FocusLost => "focus_lost",
            Self::Paused => "paused",
            Self::OwnershipLost => "ownership_lost",
            Self::Teardown => "teardown",
            Self::Restarted => "restarted",
        }
    }
}

impl fmt::Display for HandoverReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The flight content of one frame: the edges and axes the simulation consumes.
///
/// A UI action is deliberately not here: it left the frame at the
/// producer/consumer boundary as a [`UiRequest`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlightContent {
    /// The one-shot flight edges.
    pub edges: Vec<Action>,
    /// The continuous commands the frame drives.
    pub axes: Vec<FlightCommand>,
}

impl FlightContent {
    /// The flight content of `frame`, which is left untouched.
    #[must_use]
    pub fn of(frame: &InputFrame) -> Self {
        Self {
            edges: frame.edges().to_vec(),
            axes: frame.axes().iter().map(|axis| axis.command()).collect(),
        }
    }

    /// Whether the frame carried no flight edge and no axis.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty() && self.axes.is_empty()
    }
}

/// A UI action the input path received, for the screen or pause path to
/// perform.
///
/// The input session **requests** and never performs: no cash, ownership or
/// objective field is touched here
/// (`docs/contracts/UI-NETWORK.md`, "UI transition discipline").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UiRequest {
    /// The action the source resolved to.
    pub action: UiAction,
    /// The tick whose input boundary the request belongs to.
    pub tick: Tick,
    /// The context the request was produced in.
    pub context: InputContext,
}

/// Why content was suppressed rather than consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SuppressReason {
    /// The window does not have focus: input is suppressed entirely
    /// (non-negotiable behavior 4).
    Unfocused,
    /// The session is paused and this path refuses flight content.
    Paused,
    /// The context does not accept the content.
    Context,
}

impl SuppressReason {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unfocused => "unfocused",
            Self::Paused => "paused",
            Self::Context => "context",
        }
    }
}

impl fmt::Display for SuppressReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One problem the input path reported instead of swallowing.
#[derive(Clone, Debug, PartialEq)]
pub enum InputFault {
    /// A device event was refused. The frame kept the input every other device
    /// delivered (F22-B's "refused whole, not applied half way" rule).
    Device {
        /// The tick the frame was stamped for.
        tick: Tick,
        /// The event's own refusal.
        error: AdapterError,
    },
    /// The frame carried local flight content the control gate does not accept,
    /// so it was not handed to the simulation.
    NotAuthoritative {
        /// The tick the frame was stamped for.
        tick: Tick,
        /// The content that was refused.
        content: FlightContent,
    },
    /// The frame carried content this session's policy suppressed.
    Suppressed {
        /// The tick the frame was stamped for.
        tick: Tick,
        /// The content that was not consumed.
        content: FlightContent,
        /// Why it was suppressed.
        reason: SuppressReason,
    },
    /// Input boundaries were asked for while the session was paused. A paused
    /// simulation's input boundary does not run
    /// (`cs_sim::time::PausePolicy::Freeze`), so the ticks are refused and
    /// reported rather than executed into a frozen world.
    TicksWhilePaused {
        /// The tick the frame was stamped for.
        tick: Tick,
        /// How many boundaries the caller asked for.
        requested: u64,
    },
    /// A recorded tick could not be appended to the command stream.
    Stream {
        /// The stream's own refusal.
        error: StreamError,
    },
    /// The consumer's buffer refused a frame, so it changed nothing. Reported
    /// with the buffer's own refusal; a pump that is refused this way returns
    /// the error to its caller instead.
    Buffer {
        /// The tick the frame was stamped for.
        tick: Tick,
        /// The buffer's refusal.
        error: ControlError,
    },
    /// A held axis could not be turned back into a quantized sample for the
    /// record. Unreachable while the buffer only accepts validated values, and
    /// reported rather than ignored if it ever happens.
    Axis {
        /// The command the axis drives.
        command: FlightCommand,
        /// The sample's own refusal.
        error: AxisValueError,
    },
}

/// What a control handover released.
///
/// A handover is the one place that ends local control, and it always does the
/// same three things so that no path can release only some of them: the device
/// holds are released, the buffer's undelivered edges are discarded, and the
/// buffer's axes are set to exactly neutral. The record of what was released
/// is returned to the caller, so a press that was dropped says so instead of
/// vanishing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlHandover {
    /// Why the handover happened.
    pub reason: Option<HandoverReason>,
    /// The device holds that were released and the axes the devices were
    /// driving, which the next finished frame states neutral.
    pub released: SuppressedHolds,
    /// The queued edges that will now never be delivered. An edge already
    /// delivered to a consumer stays delivered; only the input layer refuses
    /// to rewrite history.
    pub discarded_edges: Vec<Action>,
    /// The commands the simulation's held axes were set to neutral for.
    pub neutralized_axes: Vec<FlightCommand>,
}

impl ControlHandover {
    /// A handover that released nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether the handover released nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.released.is_empty()
            && self.discarded_edges.is_empty()
            && self.neutralized_axes.is_empty()
    }
}

/// What a focus change did to the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusOutcome {
    /// Whether the window has focus after the change.
    pub focused: bool,
    /// The context before the change.
    pub context_before: InputContext,
    /// The context after the change.
    pub context: InputContext,
    /// The tick the change happened at.
    pub at: Tick,
    /// What the input path released while suppressing input: the device holds
    /// that were released, the edges that were discarded and the axes that were
    /// set to neutral. Empty when the call changed nothing.
    pub released: ControlHandover,
    /// What happened to the local pause.
    pub pause: PauseDecision,
    /// Whether this call changed the context or the focus.
    pub changed: bool,
}

/// Why a session operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionError {
    /// The session was torn down. Call
    /// [`restart`](InputSession::restart) to re-arm it.
    Inactive,
    /// The control gate refused the operation.
    Control(ControlError),
    /// The command stream refused a recorded tick.
    Stream(StreamError),
    /// A held axis could not be re-quantized for the record.
    Axis(AxisValueError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inactive => f.write_str(
                "the local input session is torn down; restart it before pumping frames",
            ),
            Self::Control(error) => write!(f, "{error}"),
            Self::Stream(error) => write!(f, "the command stream refused a record: {error}"),
            Self::Axis(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ControlError> for SessionError {
    fn from(error: ControlError) -> Self {
        Self::Control(error)
    }
}

impl From<StreamError> for SessionError {
    fn from(error: StreamError) -> Self {
        Self::Stream(error)
    }
}

impl From<AxisValueError> for SessionError {
    fn from(error: AxisValueError) -> Self {
        Self::Axis(error)
    }
}

/// What one render frame's pump produced.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameOutcome {
    /// The tick the frame was stamped for.
    pub frame_tick: Tick,
    /// The tick after the frame's boundaries: the next frame's stamp.
    pub tick: Tick,
    /// How many input boundaries ran.
    pub ticks_ran: u64,
    /// How many boundaries the caller asked for that were refused because the
    /// session is paused.
    pub ticks_refused: u64,
    /// Every flight action the frame's boundaries delivered, in tick order.
    pub delivered: Vec<Action>,
    /// How many UI actions this frame routed to the screen or pause path.
    pub ui_requests: usize,
    /// The flight content the session did not hand to the simulation.
    pub suppressed: FlightContent,
    /// Why that content was suppressed, when any was.
    pub suppress_reason: Option<SuppressReason>,
    /// How many device reports were dropped because the window is not focused.
    pub dropped_events: usize,
    /// The faults this frame produced.
    pub faults: Vec<InputFault>,
    /// The replay window's bookkeeping, in replay mode.
    pub replay: Option<ReplayWindow>,
}

impl FrameOutcome {
    /// Whether the frame produced any fault.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.faults.is_empty()
    }
}

/// One render frame's input: the live device reports, or a recorded stream.
#[derive(Debug)]
pub enum FrameInput<'a> {
    /// The live device reports of this render frame, in the order the platform
    /// produced them.
    Devices(&'a [DeviceEvent]),
    /// A recorded command stream, replayed from the cursor's position.
    Replay(&'a mut ReplayCursor),
}

/// How one replay window matched the recorded stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayWindow {
    /// The recorded ticks this window read out of the stream into its frame.
    /// The session may still refuse that frame — a path that is closed or does
    /// not own the actor — and says so with the frame's own
    /// [`suppressed`](FrameOutcome::suppressed) content.
    pub matched: usize,
    /// The recorded ticks this window passed over, because the session's tick
    /// had already moved past them. A correct run has none.
    pub orphaned: usize,
}

/// A cursor into a recorded [`CommandStream`].
///
/// The cursor is where a replay keeps its position. A render frame covers the
/// window `[tick, tick + ticks)`, and every recorded tick in that window is
/// merged into one frame stamped at the window's first tick — the same shape a
/// live producer builds. A recorded tick's edges therefore execute at the first
/// boundary at or after their own frame's start, which is how a real input
/// sample behaves: the display rate decides a sample's *latency*, never the
/// command it carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayCursor {
    stream: CommandStream,
    next: usize,
    delivered: usize,
    orphaned: usize,
}

impl ReplayCursor {
    /// A cursor at the start of `stream`.
    #[must_use]
    pub const fn new(stream: CommandStream) -> Self {
        Self {
            stream,
            next: 0,
            delivered: 0,
            orphaned: 0,
        }
    }

    /// The stream being replayed.
    #[must_use]
    pub const fn stream(&self) -> &CommandStream {
        &self.stream
    }

    /// How many recorded ticks have been read out of the stream into a frame.
    ///
    /// This counts the stream being consumed, not the commands being executed:
    /// a session whose path is closed or which does not own the actor reads the
    /// window and refuses it, and reports every command it refused in the
    /// frame's own outcome.
    #[must_use]
    pub const fn delivered(&self) -> usize {
        self.delivered
    }

    /// How many recorded ticks the windows passed over.
    #[must_use]
    pub const fn orphaned(&self) -> usize {
        self.orphaned
    }

    /// How many recorded ticks the cursor has not read yet, whether it matched
    /// them into a frame or passed over them as orphaned.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.stream.len().saturating_sub(self.next)
    }

    /// Whether the whole stream has been delivered.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.remaining() == 0
    }

    /// The frame covering `[start, start + ticks)`.
    ///
    /// The merged frame is stamped at `start` and takes, for every continuous
    /// command, the **last** sample the window states: the freshest reading at
    /// the render boundary, exactly what a live producer's finished frame
    /// states. A window with no recorded tick yields an empty frame, and a
    /// recorded tick the session has already passed is counted as
    /// [`orphaned`](ReplayWindow::orphaned) rather than delivered late.
    pub fn frame(&mut self, start: Tick, ticks: u64) -> (InputFrame, ReplayWindow) {
        let end = Tick(start.0.saturating_add(ticks));
        let mut window = ReplayWindow::default();
        let mut frame = InputFrame::new(start);
        while self.next < self.stream.len() {
            let record = &self.stream.records()[self.next];
            if record.frame_tick() < start {
                self.next += 1;
                window.orphaned += 1;
                self.orphaned += 1;
                continue;
            }
            if record.frame_tick() >= end {
                break;
            }
            self.next += 1;
            window.matched += 1;
            self.delivered += 1;
            for edge in record.edges() {
                frame.push_edge(*edge);
            }
            for axis in record.axes() {
                frame.set_axis(*axis);
            }
        }
        (frame, window)
    }
}

/// What a replay did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayReport {
    /// The render frames the replay pumped.
    pub frames: u64,
    /// The input boundaries the replay's clock committed. A paused session
    /// commits none, and the replay's clock does not move either.
    pub ticks: u64,
    /// The recorded ticks read out of the stream into a frame. Whether the
    /// session executed them is in each [`FrameOutcome`], not here.
    pub delivered: usize,
    /// The recorded ticks the replay passed over.
    pub orphaned: usize,
    /// The recorded ticks still ahead when the replay stopped.
    pub remaining: usize,
}

impl ReplayReport {
    /// Whether the whole stream was consumed and nothing was orphaned.
    ///
    /// This says the *stream* ran out, not that every command was executed: a
    /// session that refused the frames names what it refused in each
    /// [`FrameOutcome`], and a paused session leaves records here until the
    /// replay picks them up again.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.remaining == 0 && self.orphaned == 0
    }
}

/// Why a replay was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplayError {
    /// The clock refused the frame's wall time.
    Time(TimeError),
    /// The session was torn down.
    SessionInactive,
    /// The session refused the frame for a reason that is not a tick
    /// divergence. The refusal is kept whole so nothing is swallowed.
    SessionRefused {
        /// The session's own refusal.
        error: Box<SessionError>,
    },
    /// The session and the replay clock had drifted apart, which would put the
    /// recorded ticks in the wrong window instead of failing loudly.
    TickDivergence {
        /// The tick the session is at.
        session: Tick,
        /// The tick the replay's clock is at.
        clock: Tick,
    },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Time(error) => write!(f, "{error}"),
            Self::SessionInactive => f.write_str("the local input session is torn down"),
            Self::SessionRefused { error } => write!(f, "{error}"),
            Self::TickDivergence { session, clock } => write!(
                f,
                "the session is at tick {} but the replay clock is at tick {}",
                session.0, clock.0
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

/// The local input session of one player seat.
///
/// It owns the whole path from a device report to the simulation's input
/// boundary: the [`InputCollector`] and its adapters (producer), the
/// [`ControlGate`] (ownership), the [`ControlBuffer`] and [`ThrottleSteps`]
/// (consumer), the current [`InputContext`], the window's focus, the local
/// pause, the UI request queue, the fault queue and the recorded
/// [`CommandStream`].
///
/// A render frame is one [`pump_frame`](Self::pump_frame) call. Nothing else
/// may change the context, the authority or the focus, so the simulation and
/// the UI cannot disagree about who owns the devices.
#[derive(Clone, Debug, PartialEq)]
pub struct InputSession {
    collector: InputCollector,
    gate: ControlGate,
    controls: ControlBuffer,
    throttle: ThrottleSteps,
    mode: SessionMode,
    seat: LocalSeatId,
    tick: Tick,
    focused: bool,
    /// The context the session had when it last lost focus, restored when it
    /// gains it again.
    context_before_focus_loss: InputContext,
    pause: Option<PauseReason>,
    ui_requests: Vec<UiRequest>,
    faults: Vec<InputFault>,
    losses: Vec<DeviceLoss>,
    stream: CommandStream,
    recording: bool,
    active: bool,
}

impl InputSession {
    /// A session for `seat` in `mode`, with the session's own action map, at
    /// `tick`.
    ///
    /// The session starts **armed, focused, unpaused and in
    /// [`InputContext::Flight`]**, with its seat holding control: that is the
    /// state a client is in when it joins a session, and the platform's real
    /// focus is reported with [`set_focus`](Self::set_focus). A session that
    /// is created inside a text field must be told so with
    /// [`set_context`](Self::set_context) before its first frame.
    #[must_use]
    pub fn new(map: ActionMap, seat: LocalSeatId, mode: SessionMode, tick: Tick) -> Self {
        let mut gate = ControlGate::new();
        // A session is created for its seat: the seat owns the actor it was
        // created for, and any other authority has to take it over explicitly.
        gate.assign(ControlAuthority::LocalSeat(seat))
            .expect("a fresh gate accepts the seat it was created for");
        gate.set_context(InputContext::Flight);
        Self {
            collector: InputCollector::new(map, tick),
            gate,
            controls: ControlBuffer::new(),
            throttle: ThrottleSteps::designed_default(),
            mode,
            seat,
            tick,
            focused: true,
            context_before_focus_loss: InputContext::Flight,
            pause: None,
            ui_requests: Vec::new(),
            faults: Vec::new(),
            losses: Vec::new(),
            stream: CommandStream::new(),
            recording: false,
            active: true,
        }
    }

    /// A session with the designed default action map.
    #[must_use]
    pub fn designed_default(seat: LocalSeatId, mode: SessionMode, tick: Tick) -> Self {
        Self::new(ActionMap::designed_default(), seat, mode, tick)
    }

    /// The session's mode.
    #[must_use]
    pub const fn mode(&self) -> SessionMode {
        self.mode
    }

    /// The seat this session drives.
    #[must_use]
    pub const fn seat(&self) -> LocalSeatId {
        self.seat
    }

    /// The tick the next frame is stamped for.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Whether the window has focus.
    #[must_use]
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// Why the local session is paused, when it is.
    #[must_use]
    pub const fn pause_reason(&self) -> Option<PauseReason> {
        self.pause
    }

    /// Whether the local session is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.pause.is_some()
    }

    /// Whether the session is armed (not torn down).
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// The session's current input context.
    #[must_use]
    pub const fn context(&self) -> InputContext {
        self.gate.context()
    }

    /// The session's bindings and context.
    #[must_use]
    pub const fn bindings(&self) -> &InputBindings {
        self.collector.bindings()
    }

    /// The session's collector: its device adapters, calibration and losses.
    #[must_use]
    pub const fn collector(&self) -> &InputCollector {
        &self.collector
    }

    /// Mutable access to the collector, for the settings path that connects a
    /// device or adopts an identity.
    pub fn collector_mut(&mut self) -> &mut InputCollector {
        &mut self.collector
    }

    /// The session's control gate: the single owner of the actor.
    #[must_use]
    pub const fn gate(&self) -> &ControlGate {
        &self.gate
    }

    /// The simulation's control buffer.
    #[must_use]
    pub const fn controls(&self) -> &ControlBuffer {
        &self.controls
    }

    /// The throttle this session's tick boundaries moved.
    #[must_use]
    pub const fn throttle(&self) -> &ThrottleSteps {
        &self.throttle
    }

    /// Mutable access to the throttle, for an analog throttle device.
    pub const fn throttle_mut(&mut self) -> &mut ThrottleSteps {
        &mut self.throttle
    }

    /// The recorded command stream.
    #[must_use]
    pub const fn stream(&self) -> &CommandStream {
        &self.stream
    }

    /// Whether the session is recording its command stream.
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.recording
    }

    /// Starts recording the command stream, discarding whatever was recorded
    /// before. The record is made at the input boundary, so it is independent
    /// of the render frame rate by construction.
    pub fn start_recording(&mut self) {
        self.stream = CommandStream::new();
        self.recording = true;
    }

    /// Stops recording and returns the stream, so the caller owns it.
    #[must_use]
    pub fn stop_recording(&mut self) -> CommandStream {
        self.recording = false;
        std::mem::take(&mut self.stream)
    }

    /// The UI requests the screen or pause path has not taken yet.
    #[must_use]
    pub fn ui_requests(&self) -> &[UiRequest] {
        &self.ui_requests
    }

    /// Takes the pending UI requests, so the path handles each exactly once.
    pub fn take_ui_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.ui_requests)
    }

    /// The faults reported since the last [`take_faults`](Self::take_faults).
    #[must_use]
    pub fn faults(&self) -> &[InputFault] {
        &self.faults
    }

    /// Takes the reported faults, so a caller reports each exactly once.
    pub fn take_faults(&mut self) -> Vec<InputFault> {
        std::mem::take(&mut self.faults)
    }

    /// The device losses reported since the last
    /// [`take_losses`](Self::take_losses).
    #[must_use]
    pub fn losses(&self) -> &[DeviceLoss] {
        &self.losses
    }

    /// Takes the reported device losses, so a caller handles each exactly once.
    pub fn take_losses(&mut self) -> Vec<DeviceLoss> {
        std::mem::take(&mut self.losses)
    }

    /// Switches the session's input context.
    ///
    /// This is the **only** way the context changes, and it writes the
    /// collector's bindings and the control gate together: a menu, a text
    /// field, a pause screen and the simulation therefore cannot disagree about
    /// who owns the devices (non-negotiable behavior 5). The previous context
    /// is returned so a caller can restore it.
    pub fn set_context(&mut self, context: InputContext) -> InputContext {
        let previous = self.gate.context();
        self.gate.set_context(context);
        self.collector.set_context(context);
        previous
    }

    /// Reports the window's focus.
    ///
    /// Losing focus (non-negotiable behavior 4) releases every hold, discards
    /// every edge the buffer has not delivered yet, neutralizes every held
    /// axis and switches the context to [`InputContext::Cinematic`], so the
    /// whole path is closed. The session is then **paused where that is
    /// allowed** — a single-player session, whose pause the local process owns
    /// — and only neutralized in a networked session, which has no local pause
    /// authority ([`PauseDecision::NoLocalAuthority`]) and must not ask the
    /// server to pause.
    ///
    /// Regaining focus restores the context the session had before the loss and
    /// **does not** resume: a single-player pause stands until
    /// [`resume`](Self::resume) is called, so a window that comes back from the
    /// background never restarts a mission on its own. The devices stay
    /// connected and the platform's next report re-establishes their state.
    pub fn set_focus(&mut self, focused: bool) -> FocusOutcome {
        let context_before = self.context();
        let at = self.tick;
        if focused == self.focused {
            return FocusOutcome {
                focused,
                context_before,
                context: context_before,
                at,
                released: ControlHandover::empty(),
                pause: match self.pause {
                    Some(reason) => PauseDecision::AlreadyPaused(reason),
                    None => PauseDecision::Unchanged,
                },
                changed: false,
            };
        }
        self.focused = focused;
        if focused {
            // The context the session had *before* the loss is restored, not
            // the one the loss installed.
            self.set_context(self.context_before_focus_loss);
            return FocusOutcome {
                focused,
                context_before,
                context: self.context(),
                at,
                released: ControlHandover::empty(),
                pause: match self.pause {
                    Some(reason) => PauseDecision::AlreadyPaused(reason),
                    None => PauseDecision::Unchanged,
                },
                changed: true,
            };
        }
        self.context_before_focus_loss = context_before;
        let released = self.handover(HandoverReason::FocusLost);
        self.set_context(InputContext::Cinematic);
        // `pause_local` is the one place the mode decides: a networked session
        // has no local pause authority, so the focus loss only neutralized.
        let pause = self.pause_local(PauseReason::FocusLost);
        FocusOutcome {
            focused,
            context_before,
            context: self.context(),
            at,
            released,
            pause,
            changed: true,
        }
    }

    /// Pauses the local session, where the mode allows it.
    ///
    /// Entering a pause releases the input path's holds, discards the edges the
    /// buffer has not delivered and neutralizes the held axes, so a pause
    /// cannot leave the guns firing and a press made before it is not
    /// resurrected by the resume. UI actions keep flowing, because the pause
    /// screen is driven by them. What the release gave up is in the outcome.
    pub fn pause(&mut self, reason: PauseReason) -> PauseOutcome {
        let decision = self.pause_local(reason);
        let released = match decision {
            PauseDecision::Paused(_) => self.handover(HandoverReason::Paused),
            _ => ControlHandover::empty(),
        };
        PauseOutcome { decision, released }
    }

    /// The pause itself, with the mode check the focus path shares.
    fn pause_local(&mut self, reason: PauseReason) -> PauseDecision {
        if !self.mode.may_pause_locally() {
            return PauseDecision::NoLocalAuthority;
        }
        if let Some(existing) = self.pause {
            return PauseDecision::AlreadyPaused(existing);
        }
        self.pause = Some(reason);
        PauseDecision::Paused(reason)
    }

    /// Resumes the local session and reports what the pause left behind.
    ///
    /// Resuming is always the caller's decision: the input session never
    /// resumes itself, so neither a focus gain, a lost device nor a torn-down
    /// session can restart the world. The pause's queued input does not survive
    /// the resume — a press made before the pause is not resurrected by it — so
    /// the handover discards whatever the buffer still held.
    pub fn resume(&mut self) -> ControlHandover {
        if self.pause.take().is_none() {
            return ControlHandover::empty();
        }
        self.handover(HandoverReason::Paused)
    }

    /// Hands control of the actor to `authority`.
    ///
    /// This is the **transfer**, so it is the one place a different authority
    /// can take a held actor: the local seat's hold is released first, one
    /// internal handover reports what that released, and the new
    /// authority is assigned in the same call. Re-assigning the current holder
    /// is a no-op success; assigning to an unowned actor takes it without a
    /// handover, because the local seat never held it.
    ///
    /// So the next owner never inherits the previous owner's held trigger,
    /// queued press or stick deflection.
    ///
    /// # Errors
    ///
    /// [`SessionError::Inactive`] after a teardown, and
    /// [`SessionError::Control`] with [`ControlError::AuthorityAlreadyOwned`]
    /// when a *different* authority already holds the actor, so a transfer can
    /// never steal it. That refusal is **cost-free**: it is decided before
    /// anything is released, so a refused transfer changes nothing at all.
    pub fn assign(&mut self, authority: ControlAuthority) -> Result<ControlHandover, SessionError> {
        self.require_active()?;
        let local = ControlAuthority::LocalSeat(self.seat);
        match self.gate.authority() {
            Some(holder) if holder != local => {
                Err(SessionError::Control(ControlError::AuthorityAlreadyOwned {
                    existing: holder,
                    requested: authority,
                }))
            }
            Some(_) => {
                // The seat holds the actor: this is the release half of the
                // transfer, and the handover is what it reports.
                let handed = self.handover(HandoverReason::OwnershipLost);
                self.gate.release(local)?;
                self.gate.assign(authority)?;
                Ok(handed)
            }
            None => {
                self.gate.assign(authority)?;
                Ok(ControlHandover::empty())
            }
        }
    }

    /// Releases control on behalf of `authority`.
    ///
    /// # Errors
    ///
    /// [`SessionError::Inactive`] after a teardown, and
    /// [`SessionError::Control`] with [`ControlError::NotOwner`] when a
    /// different authority holds the actor. Releasing local control also runs
    /// the handover.
    pub fn release(
        &mut self,
        authority: ControlAuthority,
    ) -> Result<ControlHandover, SessionError> {
        self.require_active()?;
        let local = ControlAuthority::LocalSeat(self.seat);
        let was_local = self.gate.authority() == Some(local);
        self.gate.release(authority)?;
        if was_local {
            return Ok(self.handover(HandoverReason::OwnershipLost));
        }
        Ok(ControlHandover::empty())
    }

    /// Tears the session down: the seat gives up the actor, every hold is
    /// released, every undelivered edge is discarded and every held axis is
    /// set to neutral. A torn-down session accepts no frames, so a stale render
    /// loop cannot keep commanding a session that ended.
    ///
    /// Tearing down twice is safe and reports an empty handover, because
    /// teardown is the path a shutdown, a mission exit and an ownership loss
    /// all take and it must not be able to deadlock on a second call.
    pub fn teardown(&mut self) -> ControlHandover {
        if !self.active {
            return ControlHandover::empty();
        }
        self.active = false;
        let _ = self.gate.release(ControlAuthority::LocalSeat(self.seat));
        self.handover(HandoverReason::Teardown)
    }

    /// Re-arms a torn-down session: the seat takes the actor again, the focus,
    /// the pause and the context are reset and a **new** command stream is
    /// started, so a new session's commands are never recorded into the old
    /// session's stream.
    ///
    /// This is the retry path: a caller that tore a session down and wants to
    /// start again is told exactly what the previous session left behind,
    /// instead of silently reusing a half-finished one.
    ///
    /// # Errors
    ///
    /// [`SessionError::Control`] when another authority holds the actor, so a
    /// retry cannot steal it. Nothing is changed when the retry is refused.
    pub fn restart(&mut self) -> Result<ControlHandover, SessionError> {
        if self.active {
            return Ok(ControlHandover::empty());
        }
        self.gate.assign(ControlAuthority::LocalSeat(self.seat))?;
        let leftover = self.handover(HandoverReason::Restarted);
        self.set_context(InputContext::Flight);
        self.focused = true;
        self.pause = None;
        self.ui_requests.clear();
        self.faults.clear();
        self.losses.clear();
        self.stream = CommandStream::new();
        self.recording = false;
        self.active = true;
        Ok(leftover)
    }

    /// Observes one physical source press through the session's gate.
    ///
    /// This is the manual path a platform system that reports individual key
    /// events uses; the device adapters are the other. A UI action becomes a
    /// [`UiRequest`] instead of an edge, a continuous command is not an edge at
    /// all (its value comes from the adapters), and nothing is accepted while
    /// the window is unfocused, the session is paused or the seat does not own
    /// the actor.
    ///
    /// An accepted edge is queued in the **consumer's** buffer, stamped at the
    /// session's current tick, so the next input boundary delivers it exactly
    /// once — the same shape a device report produces. It is deliberately not
    /// appended to the collector's frame: [`pump_frame`](Self::pump_frame)
    /// opens every live frame with `begin_frame`, which discards whatever was
    /// collected before it, so an edge parked there would be reported to the
    /// caller here and then silently dropped by the next frame.
    ///
    /// The returned action is therefore the action the session accepted, not a
    /// promise about a later delivery: a pause, a handover or a teardown
    /// discards it like any other queued edge, and says so in the handover it
    /// reports.
    pub fn observe_source(&mut self, source: BindingSource) -> Option<Action> {
        if !self.active || !self.focused {
            return None;
        }
        let action = self.gate.resolve(self.collector.bindings().map(), source)?;
        match action {
            Action::Ui(ui) => {
                self.ui_requests.push(UiRequest {
                    action: ui,
                    tick: self.tick,
                    context: self.context(),
                });
                Some(action)
            }
            Action::Flight(command) if command.is_continuous() => None,
            flight => {
                if self.is_paused() {
                    self.suppress(SuppressReason::Paused, flight);
                    return None;
                }
                let mut frame = InputFrame::new(self.tick);
                frame.push_edge(flight);
                // `apply_frame` validates the whole frame before mutating any of
                // it, so a refusal leaves the buffer exactly as it was. It is
                // reported rather than returned as a `Some`, because a caller
                // that only reads the return value would otherwise command an
                // aircraft the session refused.
                if let Err(error) = self.controls.apply_frame(&frame) {
                    self.faults.push(InputFault::Buffer {
                        tick: self.tick,
                        error,
                    });
                    return None;
                }
                Some(flight)
            }
        }
    }

    /// Pumps one render frame.
    ///
    /// The live path ([`FrameInput::Devices`]) applies every device report to
    /// the adapters, closes the frame, routes the UI actions to the request
    /// queue and folds the rest into the control buffer; the replay path
    /// ([`FrameInput::Replay`]) takes the frame from the recorded stream
    /// instead. Both then run `ticks` input boundaries, apply the throttle
    /// steps the delivered edges caused, and — while recording — append one
    /// [`InputFrame`] per boundary to the command stream.
    ///
    /// # Errors
    ///
    /// [`SessionError::Inactive`] when the session was torn down,
    /// [`SessionError::Control`] when the buffer refuses the frame and
    /// [`SessionError::Axis`] when a held axis cannot be re-quantized for the
    /// record. A refused *device event* is not an error of this call: it is
    /// reported as an [`InputFault::Device`] and the frame keeps the input every
    /// other device delivered, because a bad report must not swallow a good
    /// press.
    pub fn pump_frame(
        &mut self,
        input: FrameInput<'_>,
        ticks: u64,
    ) -> Result<FrameOutcome, SessionError> {
        self.require_active()?;
        let frame_tick = self.tick;
        let mut faults = Vec::new();
        let mut dropped_events = 0;

        let (mut frame, replay) = match input {
            FrameInput::Devices(events) => {
                self.collector.begin_frame(frame_tick);
                for event in events {
                    match self.observe_device_event(event) {
                        Ok(()) => {}
                        Err(DeviceOutcome::Dropped) => dropped_events += 1,
                        Err(DeviceOutcome::Refused(error)) => faults.push(InputFault::Device {
                            tick: frame_tick,
                            error,
                        }),
                    }
                }
                (self.collector.take_frame(), None)
            }
            FrameInput::Replay(cursor) => {
                let (frame, window) = cursor.frame(frame_tick, ticks);
                (frame, Some(window))
            }
        };

        // The UI/flight split happens here, at the producer/consumer boundary:
        // a UI action is a request for the screen or pause path and must never
        // enter the control buffer, and the input session never performs it.
        let mut queued_ui = 0;
        for action in frame.take_ui_actions() {
            if !self.focused {
                continue;
            }
            self.ui_requests.push(UiRequest {
                action,
                tick: frame_tick,
                context: self.context(),
            });
            queued_ui += 1;
        }

        let content = FlightContent::of(&frame);
        let mut suppressed = FlightContent::default();
        let mut suppress_reason = None;
        let mut ticks_refused = 0;
        // An inert frame carries no input: it is either empty or it restates an
        // axis as exactly neutral. It is applied even where this session's
        // policy suppresses input, because its only content is the neutral
        // restatement that stops the last deflection sticking — but it is never
        // delivered and never reported as suppressed input. Anything else is
        // **not** applied where the policy suppresses it: reporting a frame as
        // suppressed and then handing it to the simulation anyway would command
        // the aircraft from a path that is closed, which is the one thing
        // non-negotiable behavior 4 and the single-authority rule forbid.
        let inert = frame.is_inert();
        let apply = if !self.focused {
            if !inert {
                suppressed = content.clone();
                suppress_reason = Some(SuppressReason::Unfocused);
                faults.push(InputFault::Suppressed {
                    tick: frame_tick,
                    content,
                    reason: SuppressReason::Unfocused,
                });
            }
            // A live frame cannot reach here with content (a report is not read
            // while the window is unfocused), so this only decides a frame that
            // arrived by another route: a recorded stream replayed into a
            // backgrounded window is reported, and never executed.
            inert
        } else if !self.gate.accepts_local_input() {
            // The gate is the single owner: local device input reaches the
            // simulation only while it accepts it. A live report never gets
            // this far (the session is not reading the devices), so what
            // arrives here is a frame from another route — a replay, or a
            // caller that resolved a source itself — and it is refused by name
            // rather than dropped quietly.
            if !inert {
                faults.push(InputFault::NotAuthoritative {
                    tick: frame_tick,
                    content: content.clone(),
                });
                suppressed = content;
                suppress_reason = Some(SuppressReason::Context);
            }
            inert
        } else {
            true
        };
        if apply {
            // `apply_frame` validates the whole frame before mutating any of it,
            // so a refusal leaves the buffer exactly as it was.
            self.controls.apply_frame(&frame)?;
        }

        let boundaries = if self.is_paused() {
            if ticks > 0 {
                ticks_refused = ticks;
                faults.push(InputFault::TicksWhilePaused {
                    tick: frame_tick,
                    requested: ticks,
                });
            }
            0
        } else {
            ticks
        };

        let mut delivered = Vec::new();
        for _ in 0..boundaries {
            let edges = self.controls.begin_tick(self.tick);
            self.throttle.apply_tick(&edges);
            if self.recording
                && let Err(error) = self.record_tick(&edges, &mut faults)
            {
                faults.push(InputFault::Stream { error });
            }
            delivered.extend(edges);
            self.tick = Tick(self.tick.0 + 1);
        }

        self.losses.extend(self.collector.take_device_losses());
        // The frame's own faults are reported in the outcome *and* queued, so a
        // caller that only drains `take_faults` cannot miss one.
        self.faults.extend(faults.iter().cloned());

        Ok(FrameOutcome {
            frame_tick,
            tick: self.tick,
            ticks_ran: boundaries,
            ticks_refused,
            delivered,
            ui_requests: queued_ui,
            suppressed,
            suppress_reason,
            dropped_events,
            faults,
            replay,
        })
    }

    /// Applies one device report, refuses it, or drops it because this session
    /// is not the one reading the devices.
    ///
    /// A report's *readings* are applied only while this session owns the
    /// devices, which means both that the window has focus and that the seat
    /// still holds the actor. Two things follow, and both are deliberate:
    ///
    /// * The device **set** is maintained either way. A stick unplugged behind
    ///   the window's back, or unplugged while a server owns the actor, is
    ///   still unplugged when this session takes over, and its loss is still
    ///   reported.
    /// * Nothing is *tracked* while this session is not the reader. A hold
    ///   established for an actor this session does not own would silence the
    ///   next legitimate press when control returns, which is the same class of
    ///   bug as a removed device leaving the guns firing.
    fn observe_device_event(&mut self, event: &DeviceEvent) -> Result<(), DeviceOutcome> {
        if !self.reads_devices() {
            return match event {
                DeviceEvent::Connected { .. } | DeviceEvent::Removed { .. } => self
                    .collector
                    .observe_device(event)
                    .map_err(DeviceOutcome::Refused),
                _ => Err(DeviceOutcome::Dropped),
            };
        }
        self.collector
            .observe_device(event)
            .map_err(DeviceOutcome::Refused)
    }

    /// Whether this session reads device *readings* at all: the window has
    /// focus and the seat still holds the actor.
    fn reads_devices(&self) -> bool {
        self.focused && self.gate.accepts_local_input()
    }

    /// Appends one tick's executed commands to the stream.
    ///
    /// Every fault raised on the way is appended to `faults`, so a record that
    /// could not be made exactly is in the frame's own outcome as well as in
    /// the session's queue.
    fn record_tick(
        &mut self,
        edges: &[Action],
        faults: &mut Vec<InputFault>,
    ) -> Result<(), StreamError> {
        let mut record = InputFrame::new(self.tick);
        for edge in edges {
            record.push_edge(*edge);
        }
        for command in FlightCommand::CONTINUOUS {
            let Some(value) = self.controls.axis(*command) else {
                continue;
            };
            // The buffer holds a deflection as a float; re-quantizing it here
            // is exact, so the record is the quantized sample the aircraft was
            // actually driven with. A refusal here would be a buffer invariant
            // break, so it is reported as a fault instead of being hidden in
            // the record.
            match AxisValue::from_unit(*command, value) {
                Ok(sample) => record.set_axis(sample),
                Err(error) => {
                    faults.push(InputFault::Axis {
                        command: *command,
                        error,
                    });
                    let neutral = AxisValue::from_quantized(*command, 0)
                        .expect("a continuous command accepts a quantized sample");
                    record.set_axis(neutral);
                }
            }
        }
        self.stream.record_tick(record)
    }

    /// Releases the local input path: the device holds, the buffer's
    /// undelivered edges and the buffer's held axes.
    fn handover(&mut self, reason: HandoverReason) -> ControlHandover {
        let released = self.collector.devices_mut().suppress();
        let discarded_edges = self.controls.drain_pending();
        let neutralized_axes = self.controls.neutralize_axes();
        ControlHandover {
            reason: Some(reason),
            released,
            discarded_edges,
            neutralized_axes,
        }
    }

    /// Reports one suppressed edge as a fault.
    fn suppress(&mut self, reason: SuppressReason, edge: Action) {
        self.faults.push(InputFault::Suppressed {
            tick: self.tick,
            content: FlightContent {
                edges: vec![edge],
                axes: Vec::new(),
            },
            reason,
        });
    }

    /// Refuses an operation on a torn-down session.
    fn require_active(&self) -> Result<(), SessionError> {
        if self.active {
            Ok(())
        } else {
            Err(SessionError::Inactive)
        }
    }
}

/// What one device report did to the session.
enum DeviceOutcome {
    /// The report was refused whole; the adapters changed nothing.
    Refused(AdapterError),
    /// The report was dropped because the window is not focused.
    Dropped,
}

/// Replays a recorded [`CommandStream`] against a session at a chosen display
/// rate.
///
/// The display rate is real: the replay owns a [`SimClock`] and each
/// [`frame`](Self::frame) converts one render frame's wall time into whole
/// fixed ticks, so 30, 60 and 144 FPS over the same wall time commit different
/// numbers of ticks per frame while the same command stream flows through
/// [`InputSession::pump_frame`]. A replay that stepped the stream per *frame*
/// instead of per tick, or that applied a throttle step once per frame, would
/// disagree with the recording.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandReplay {
    clock: SimClock,
    cursor: ReplayCursor,
    frames: u64,
    ticks: u64,
}

impl CommandReplay {
    /// A replay of `stream` at `rate`, starting at the session's tick and
    /// running under `mode`'s clock policy.
    ///
    /// # Errors
    ///
    /// [`TimeError`] when `rate` is not a usable tick rate.
    pub fn new(
        stream: CommandStream,
        rate: TickRate,
        mode: SessionMode,
        start: Tick,
    ) -> Result<Self, TimeError> {
        Ok(Self {
            clock: SimClock::with_tick(mode.clock_policy(), rate, start),
            cursor: ReplayCursor::new(stream),
            frames: 0,
            ticks: 0,
        })
    }

    /// The replay's clock.
    #[must_use]
    pub const fn clock(&self) -> &SimClock {
        &self.clock
    }

    /// The cursor into the recorded stream.
    #[must_use]
    pub const fn cursor(&self) -> &ReplayCursor {
        &self.cursor
    }

    /// What the replay has done so far.
    #[must_use]
    pub fn report(&self) -> ReplayReport {
        ReplayReport {
            frames: self.frames,
            ticks: self.ticks,
            delivered: self.cursor.delivered(),
            orphaned: self.cursor.orphaned(),
            remaining: self.cursor.remaining(),
        }
    }

    /// Pumps one render frame of `elapsed` wall time into `session`.
    ///
    /// A paused session runs no input boundary, so the replay clock does not
    /// move either: the world's time is frozen while it is paused
    /// (`cs_sim::time::PausePolicy::Freeze`), and a window read now could not
    /// be executed, which would consume the recorded ticks it covers and leave
    /// the replay reporting a complete run that executed nothing. The replay
    /// therefore runs behind the wall clock while paused and picks the window
    /// up again where it stopped once the session resumes.
    ///
    /// # Errors
    ///
    /// [`ReplayError::Time`] when the clock refuses the wall time,
    /// [`ReplayError::SessionInactive`] when the session was torn down, and
    /// [`ReplayError::TickDivergence`] when the session and the replay clock
    /// have drifted apart, which would put the recorded ticks in the wrong
    /// window instead of failing loudly.
    pub fn frame(
        &mut self,
        session: &mut InputSession,
        elapsed: Duration,
    ) -> Result<FrameOutcome, ReplayError> {
        if session.tick() != self.clock.tick() {
            return Err(ReplayError::TickDivergence {
                session: session.tick(),
                clock: self.clock.tick(),
            });
        }
        if !session.is_active() {
            return Err(ReplayError::SessionInactive);
        }
        let ticks = if session.is_paused() {
            0
        } else {
            self.clock.advance(elapsed).map_err(ReplayError::Time)?
        };
        let outcome = session
            .pump_frame(FrameInput::Replay(&mut self.cursor), ticks)
            .map_err(|error| ReplayError::SessionRefused {
                error: Box::new(error),
            })?;
        self.frames += 1;
        self.ticks += ticks;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_sim::control::ThrottleSteps;
    use cs_types::input::{CalibrationError, DeviceClass, DeviceId, Key};

    /// The seat every fixture session drives.
    fn seat() -> LocalSeatId {
        LocalSeatId(0)
    }

    /// The fixture keyboard, with the stable identity a real platform reports.
    fn keyboard() -> DeviceId {
        DeviceId::stable(DeviceClass::Keyboard, "kbd.fixture/0")
            .expect("the fixture identity is valid")
    }

    /// The fixture stick.
    fn stick() -> DeviceId {
        DeviceId::stable(DeviceClass::Joystick, "joy.fixture/0")
            .expect("the fixture identity is valid")
    }

    /// A session with the keyboard and the stick connected, exactly as a
    /// platform's device-enumeration events would leave it.
    fn session(mode: SessionMode) -> InputSession {
        let mut session = InputSession::designed_default(seat(), mode, Tick(0));
        for device in [keyboard(), stick()] {
            session
                .collector_mut()
                .connect_device(device)
                .expect("the fixture device connects");
        }
        session
    }

    /// The keyboard's report for the keys that are down.
    fn keys(down: &[Key]) -> Vec<DeviceEvent> {
        vec![DeviceEvent::KeyboardFrame {
            device: keyboard(),
            keys: down.to_vec(),
        }]
    }

    fn fire() -> Action {
        Action::Flight(FlightCommand::FirePrimary)
    }

    /// Non-negotiable behavior 4: losing focus pauses a single-player session
    /// and only neutralizes a networked one. Both release the held trigger and
    /// neutralize the deflection, and neither leaves a queued press to be
    /// delivered when the window comes back.
    #[test]
    fn accept_f22_c_focus_loss_pauses_single_player_and_only_neutralizes_multiplayer() {
        // Single player: the pilot is holding the trigger and the pitch key.
        let mut single = session(SessionMode::SinglePlayer);
        let held = keys(&[Key::Space, Key::S]);
        let outcome = single
            .pump_frame(FrameInput::Devices(&held), 2)
            .expect("the fixture frame applies");
        assert_eq!(outcome.delivered, vec![fire()], "one press, one command");
        assert!(
            (single.controls().axis(FlightCommand::Pitch).expect("pitch") - 1.0).abs() < 1e-5,
            "the held key is driving pitch"
        );

        let lost = single.set_focus(false);
        assert!(!lost.focused);
        assert_eq!(lost.context_before, InputContext::Flight);
        assert_eq!(lost.context, InputContext::Cinematic);
        assert_eq!(
            lost.released.released.released_edges,
            vec![fire()],
            "the held trigger is released, not left firing"
        );
        assert!(
            lost.released
                .released
                .neutralized_axes
                .contains(&FlightCommand::Pitch),
            "and the deflection the keys were driving is forgotten"
        );
        assert_eq!(lost.pause, PauseDecision::Paused(PauseReason::FocusLost));
        assert_eq!(lost.released.reason, Some(HandoverReason::FocusLost));
        assert!(single.is_paused());
        assert_eq!(
            single.controls().axis(FlightCommand::Pitch),
            Some(0.0),
            "the aircraft's own axis is neutral the moment focus is lost"
        );
        assert_eq!(
            single.gate().context(),
            InputContext::Cinematic,
            "the gate and the collector agree on the context"
        );
        assert_eq!(single.bindings().context(), InputContext::Cinematic);

        // A backgrounded window produces no input, and a frozen clock's zero
        // ticks are what a paused session is asked for.
        let quiet = single
            .pump_frame(FrameInput::Devices(&held), 0)
            .expect("the fixture frame applies");
        assert_eq!(quiet.dropped_events, 1, "the report is dropped, not read");
        assert_eq!(quiet.ticks_ran, 0);
        assert_eq!(quiet.ticks_refused, 0);
        assert!(quiet.delivered.is_empty());
        assert!(
            quiet.faults.is_empty(),
            "a frozen clock and a backgrounded window are not faults: {:?}",
            quiet.faults
        );

        // Ticks asked for anyway are refused and reported, never executed into a
        // frozen world.
        let refused = single
            .pump_frame(FrameInput::Devices(&[]), 4)
            .expect("the fixture frame applies");
        assert_eq!(refused.ticks_ran, 0);
        assert_eq!(refused.ticks_refused, 4);
        assert_eq!(
            refused.faults,
            vec![InputFault::TicksWhilePaused {
                tick: Tick(2),
                requested: 4,
            }]
        );
        assert_eq!(single.tick(), Tick(2), "a paused session does not advance");

        // Focus gain restores the context and does not resume the mission.
        let back = single.set_focus(true);
        assert!(back.focused);
        assert_eq!(back.context, InputContext::Flight);
        assert!(single.is_paused(), "only the caller resumes");
        assert!(single.resume().reason.is_some());
        assert!(!single.is_paused());

        // Control comes back from the platform's next report, not from
        // anything the session remembered: a device report is the device's
        // whole state, so a trigger that is still down reads as pressed. What
        // the focus loss guaranteed is the other half — nothing fired while the
        // path was closed, and the deflection did not survive it.
        let back_in_flight = single
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space, Key::S])), 1)
            .expect("the fixture frame applies");
        assert_eq!(back_in_flight.delivered, vec![fire()]);
        assert!(
            (single.controls().axis(FlightCommand::Pitch).expect("pitch") - 1.0).abs() < 1e-5,
            "and the stick is read afresh rather than resumed where it was"
        );

        // Multiplayer: the same focus loss neutralizes and does not pause.
        let mut net = session(SessionMode::Multiplayer);
        net.pump_frame(FrameInput::Devices(&held), 2)
            .expect("the fixture frame applies");
        let lost = net.set_focus(false);
        assert_eq!(
            lost.pause,
            PauseDecision::NoLocalAuthority,
            "a client has no authority to pause the world"
        );
        assert!(!net.is_paused());
        assert_eq!(
            net.pause(PauseReason::PlayerRequest).decision,
            PauseDecision::NoLocalAuthority,
            "a client cannot pause the world from its input path"
        );

        // The world keeps ticking: nothing in the local input path can pause a
        // server it has no channel to.
        let running = net
            .pump_frame(FrameInput::Devices(&[]), 3)
            .expect("the fixture frame applies");
        assert_eq!(running.ticks_ran, 3);
        assert_eq!(running.ticks_refused, 0);
        assert!(
            running.delivered.is_empty(),
            "and the client sends no input request while unfocused"
        );
        assert_eq!(net.controls().axis(FlightCommand::Pitch), Some(0.0));
        assert_eq!(net.tick(), Tick(5));
        assert!(
            net.observe_source(BindingSource::Key(Key::Space)).is_none(),
            "an unfocused window resolves nothing at all"
        );

        // A device removed while unfocused is still removed and still reported.
        net.pump_frame(
            FrameInput::Devices(&[DeviceEvent::Removed { device: stick() }]),
            0,
        )
        .expect("the fixture removal applies");
        let losses = net.take_losses();
        assert_eq!(
            losses.len(),
            1,
            "the device set is maintained while unfocused"
        );
        assert_eq!(losses[0].device, stick());
    }

    /// A UI action leaves the frame at the producer/consumer boundary as a
    /// request and never reaches the simulation; the input session performs
    /// nothing, so the screen path owns the transaction.
    #[test]
    fn accept_f22_c_ui_actions_are_requests_and_never_reach_the_control_buffer() {
        let mut session = session(SessionMode::SinglePlayer);

        // In flight a menu key resolves to nothing at all.
        assert_eq!(session.observe_source(BindingSource::Key(Key::Enter)), None);
        let flight = session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space])), 1)
            .expect("the fixture frame applies");
        assert_eq!(flight.delivered, vec![fire()]);
        assert_eq!(flight.ui_requests, 0);

        // In a menu, the fire key stops being a flight command and the menu
        // keys become requests.
        session.set_context(InputContext::UiNavigation);
        let menu = session
            .pump_frame(
                FrameInput::Devices(&keys(&[Key::Enter, Key::Space, Key::ArrowDown])),
                1,
            )
            .expect("the fixture frame applies");
        assert_eq!(menu.ui_requests, 2);
        assert!(
            menu.delivered.is_empty(),
            "a menu cannot also fire the guns: {:?}",
            menu.delivered
        );
        assert!(menu.faults.is_empty());
        let requests = session.take_ui_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].action, UiAction::Confirm);
        assert_eq!(requests[1].action, UiAction::NavigateDown);
        assert_eq!(requests[0].tick, Tick(1));
        assert_eq!(requests[0].context, InputContext::UiNavigation);
        assert!(
            session.take_ui_requests().is_empty(),
            "a request is handled exactly once"
        );
        assert_eq!(session.controls().pending_edges(), 0);

        // The input session performed nothing: the pause is the screen path's
        // transaction, not the input path's.
        assert!(!session.is_paused());
        let paused = session.pause(PauseReason::PlayerRequest);
        assert_eq!(
            paused.decision,
            PauseDecision::Paused(PauseReason::PlayerRequest)
        );
        assert_eq!(paused.released.reason, Some(HandoverReason::Paused));
        assert!(session.is_paused());

        // Text entry emits nothing at all — not a flight command, not a UI
        // action, and not a fault either (non-negotiable behavior 5). The
        // session is still paused here, so the frame is pumped with no ticks,
        // which is what a frozen clock asks for.
        session.set_context(InputContext::TextEntry);
        let typing = session
            .pump_frame(
                FrameInput::Devices(&keys(&[Key::Space, Key::Enter, Key::S])),
                0,
            )
            .expect("the fixture frame applies");
        assert!(
            typing.delivered.is_empty(),
            "text entry must not fire weapons"
        );
        assert_eq!(typing.ui_requests, 0, "nor navigate a menu");
        assert!(
            typing.faults.is_empty(),
            "a text field is not a fault: {:?}",
            typing.faults
        );
        assert_eq!(session.ui_requests().len(), 0);
    }

    /// A control handover releases the whole local input path at once, so the
    /// next authority can never execute the previous owner's press.
    #[test]
    fn accept_f22_c_control_handover_releases_holds_discards_edges_and_neutralizes_axes() {
        let mut session = session(SessionMode::SinglePlayer);
        // A press nobody has drained yet, and a held deflection.
        session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space, Key::S])), 0)
            .expect("the fixture frame applies");
        assert_eq!(session.controls().pending_edges(), 1);
        assert_eq!(session.controls().axis(FlightCommand::Pitch), Some(1.0));

        let handed = session
            .assign(ControlAuthority::RemoteAuthority)
            .expect("the authority change is accepted");
        assert_eq!(handed.reason, Some(HandoverReason::OwnershipLost));
        assert_eq!(handed.released.released_edges, vec![fire()]);
        assert_eq!(
            handed.discarded_edges,
            vec![fire()],
            "the queued press is discarded, not left for the next owner"
        );
        assert!(handed.neutralized_axes.contains(&FlightCommand::Pitch));
        assert_eq!(
            session.gate().authority(),
            Some(ControlAuthority::RemoteAuthority)
        );
        assert_eq!(session.controls().pending_edges(), 0);
        assert_eq!(session.controls().axis(FlightCommand::Pitch), Some(0.0));
        assert!(
            session.collector().devices().held_edges().is_empty(),
            "the device holds went with the handover"
        );

        // A server-owned actor never executes local device input: the reports
        // are not even read, because tracking a hold for an actor this session
        // does not own would silence the next legitimate press.
        let refused = session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space, Key::S])), 3)
            .expect("the fixture frame applies");
        assert!(refused.delivered.is_empty());
        assert_eq!(refused.ticks_ran, 3);
        assert_eq!(refused.dropped_events, 1, "the report is not read at all");
        assert!(
            session.collector().devices().held_edges().is_empty(),
            "and nothing was tracked for an actor this session does not own"
        );
        assert!(
            refused.is_clean(),
            "a device report the session never reads is not a fault: {:?}",
            refused.faults
        );

        // A frame that arrives by another route is refused by name instead: a
        // recorded stream replayed into an actor the seat no longer owns must
        // not execute the previous owner's commands.
        let mut recorded = CommandStream::new();
        let mut stale = InputFrame::new(session.tick());
        stale.push_edge(fire());
        stale.set_axis(
            AxisValue::from_unit(FlightCommand::Pitch, 1.0).expect("full scale is a valid sample"),
        );
        recorded
            .record_tick(stale)
            .expect("the fixture record is the first one");
        let mut cursor = ReplayCursor::new(recorded);
        let replayed = session
            .pump_frame(FrameInput::Replay(&mut cursor), 3)
            .expect("a refused replay frame is not a session error");
        assert!(replayed.delivered.is_empty());
        assert_eq!(replayed.ticks_ran, 3);
        assert_eq!(replayed.suppressed.edges, vec![fire()]);
        assert_eq!(replayed.suppress_reason, Some(SuppressReason::Context));
        assert!(
            matches!(
                replayed.faults.as_slice(),
                [InputFault::NotAuthoritative { content, .. }]
                    if content.edges == vec![fire()] && content.axes == vec![FlightCommand::Pitch]
            ),
            "a replay at a server-owned actor is reported by name: {:?}",
            replayed.faults
        );
        assert_eq!(
            cursor.delivered(),
            1,
            "the replay read the record and the session refused to execute it, \
             which is the report the caller needs"
        );

        // A transfer can never steal an actor another authority already holds.
        // The refusal is named and cost-free: it is decided before anything is
        // released, so a refused transfer changes nothing.
        session.take_faults();
        let blocked = session.assign(ControlAuthority::LocalSeat(LocalSeatId(1)));
        assert!(matches!(
            blocked,
            Err(SessionError::Control(ControlError::AuthorityAlreadyOwned {
                existing: ControlAuthority::RemoteAuthority,
                requested: ControlAuthority::LocalSeat(LocalSeatId(1)),
            }))
        ));
        assert_eq!(
            session.gate().authority(),
            Some(ControlAuthority::RemoteAuthority)
        );
        assert!(
            session.faults().is_empty(),
            "a refused transfer is not a fault of the input path: {:?}",
            session.faults()
        );

        // The server hands the actor back: the local seat takes it again and
        // local input works from the next frame.
        session
            .release(ControlAuthority::RemoteAuthority)
            .expect("the owner releases");
        let taken = session
            .assign(ControlAuthority::LocalSeat(seat()))
            .expect("the local seat takes the unowned actor");
        assert!(
            taken.is_empty(),
            "taking an unowned actor releases nothing: {taken:?}"
        );
        let resumed = session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space, Key::S])), 1)
            .expect("the fixture frame applies");
        assert_eq!(
            resumed.delivered,
            vec![fire()],
            "control returned, so the devices are read again from their own state"
        );
        assert!(resumed.is_clean(), "{:?}", resumed.faults);
    }

    /// The manual source path queues its edge in the **consumer's** buffer, so
    /// the next input boundary delivers it exactly once. An implementation that
    /// parked the edge in the collector's frame instead — which
    /// [`pump_frame`](InputSession::pump_frame) opens afresh on every live
    /// frame — would report the action to its caller here and then drop it
    /// silently on the next pump.
    #[test]
    fn accept_f22_c_the_manual_source_path_queues_its_edge_for_the_next_boundary() {
        let mut session = session(SessionMode::SinglePlayer);
        assert_eq!(
            session.observe_source(BindingSource::Key(Key::Space)),
            Some(fire()),
            "a bound press in flight is accepted"
        );
        assert_eq!(
            session.controls().pending_edges(),
            1,
            "it is queued for a boundary, not delivered by the observation"
        );
        let idle = session
            .pump_frame(FrameInput::Devices(&[]), 0)
            .expect("the fixture frame applies");
        assert!(idle.delivered.is_empty(), "no boundary ran yet");
        assert_eq!(
            session.controls().pending_edges(),
            1,
            "and opening the next render frame did not drop it"
        );
        let delivered = session
            .pump_frame(FrameInput::Devices(&[]), 1)
            .expect("the fixture frame applies");
        assert_eq!(
            delivered.delivered,
            vec![fire()],
            "one press is one command, at the boundary"
        );
        assert_eq!(session.controls().pending_edges(), 0);
        assert!(delivered.is_clean(), "{:?}", delivered.faults);

        // A continuous command is never an edge from a press: its value comes
        // from the device adapters.
        assert_eq!(session.observe_source(BindingSource::Key(Key::S)), None);

        // An accepted edge is recorded like any other delivered command, so a
        // replay of this session's stream contains it.
        session.start_recording();
        assert_eq!(
            session.observe_source(BindingSource::Key(Key::Space)),
            Some(fire())
        );
        session
            .pump_frame(FrameInput::Devices(&[]), 1)
            .expect("the fixture frame applies");
        assert_eq!(session.stream().edges(), vec![fire()]);

        // A press accepted before a pause is discarded with the rest of the
        // queued input, exactly like a device report's edge.
        assert_eq!(
            session.observe_source(BindingSource::Key(Key::Space)),
            Some(fire())
        );
        let paused = session.pause(PauseReason::PlayerRequest);
        assert_eq!(
            paused.released.discarded_edges,
            vec![fire()],
            "the manual path's press does not outlive the pause"
        );

        // While paused a source resolves to nothing, and the refusal is
        // reported rather than swallowed.
        assert!(
            session
                .observe_source(BindingSource::Key(Key::Space))
                .is_none()
        );
        assert!(
            matches!(
                session.faults().last(),
                Some(InputFault::Suppressed {
                    reason: SuppressReason::Paused,
                    ..
                })
            ),
            "a press refused by the pause is named: {:?}",
            session.faults()
        );
        session.resume();
        let after = session
            .pump_frame(FrameInput::Devices(&[]), 1)
            .expect("the fixture frame applies");
        assert!(
            after.delivered.is_empty(),
            "and the discarded press is not resurrected: {:?}",
            after.delivered
        );
    }

    /// Teardown stops the loop and a restart arms a genuinely new session, so a
    /// stale render loop cannot keep commanding a session that ended and a new
    /// session never records into the old session's stream.
    #[test]
    fn accept_f22_c_teardown_stops_the_loop_and_a_restart_arms_a_new_session() {
        let mut session = session(SessionMode::SinglePlayer);
        session.start_recording();
        session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space, Key::S])), 2)
            .expect("the fixture frame applies");
        let stream = session.stop_recording();
        assert_eq!(stream.len(), 2, "one record per input boundary");
        assert!(!session.is_recording());
        assert!(session.stream().is_empty(), "the caller took the stream");
        assert_eq!(session.tick(), Tick(2));

        // A press queued after the last boundary dies with the session. The
        // trigger is released first, because a report that still lists a held
        // source re-establishes its hold rather than pressing again, and the
        // pitch key stays down so there is a live deflection to neutralize.
        session
            .pump_frame(FrameInput::Devices(&keys(&[])), 0)
            .expect("the fixture frame applies");
        session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space, Key::S])), 0)
            .expect("the fixture frame applies");
        assert_eq!(
            session.controls().pending_edges(),
            1,
            "a press with no boundary yet is still queued"
        );

        let handed = session.teardown();
        assert_eq!(handed.reason, Some(HandoverReason::Teardown));
        assert_eq!(
            handed.discarded_edges,
            vec![fire()],
            "the press queued with no boundary yet is discarded, not delivered later"
        );
        assert!(handed.neutralized_axes.contains(&FlightCommand::Pitch));
        assert!(!session.is_active());
        assert_eq!(session.gate().authority(), None);
        assert_eq!(session.context(), InputContext::Flight);

        assert_eq!(
            session.pump_frame(FrameInput::Devices(&[]), 1),
            Err(SessionError::Inactive)
        );
        assert!(
            session
                .observe_source(BindingSource::Key(Key::Space))
                .is_none()
        );
        assert!(session.assign(ControlAuthority::LocalSeat(seat())).is_err());
        assert!(
            session.teardown().is_empty(),
            "a second teardown is safe and reports nothing"
        );

        // The retry: a new session, with its own stream and its own authority.
        let leftover = session.restart().expect("the retry is accepted");
        assert!(
            leftover.discarded_edges.is_empty() && leftover.released.is_empty(),
            "the teardown already released and discarded everything: {leftover:?}"
        );
        assert!(session.is_active());
        assert_eq!(
            session.gate().authority(),
            Some(ControlAuthority::LocalSeat(seat()))
        );
        assert!(!session.is_paused());
        assert!(session.stream().is_empty());
        assert_eq!(session.tick(), Tick(2), "ticks are never reused");
        let after = session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space])), 1)
            .expect("the fixture frame applies");
        assert_eq!(after.delivered, vec![fire()]);

        // A retry that would steal the actor from another authority is refused,
        // and it changes nothing.
        let mut blocked = crate::input::session::tests::session(SessionMode::SinglePlayer);
        blocked
            .assign(ControlAuthority::Scripted)
            .expect("a script takes the actor");
        blocked.teardown();
        assert_eq!(blocked.gate().authority(), Some(ControlAuthority::Scripted));
        assert!(matches!(
            blocked.restart(),
            Err(SessionError::Control(ControlError::AuthorityAlreadyOwned {
                existing: ControlAuthority::Scripted,
                ..
            }))
        ));
        assert!(!blocked.is_active(), "a refused retry changes nothing");
        assert_eq!(
            blocked.gate().authority(),
            Some(ControlAuthority::Scripted),
            "and the actor stays with the authority that holds it"
        );
    }

    /// A refused device report is reported by name and never costs the frame
    /// the input its other devices delivered.
    #[test]
    fn accept_f22_c_a_refused_device_report_is_reported_and_keeps_the_other_input() {
        let mut session = session(SessionMode::SinglePlayer);
        let bad = DeviceEvent::JoystickFrame {
            device: stick(),
            buttons: vec![],
            axes: vec![(0, f32::NAN)],
        };
        let outcome = session
            .pump_frame(
                FrameInput::Devices(&[
                    bad,
                    DeviceEvent::KeyboardFrame {
                        device: keyboard(),
                        keys: vec![Key::Space],
                    },
                ]),
                1,
            )
            .expect("a refused report is not a session error");
        assert_eq!(outcome.delivered, vec![fire()], "the good press survives");
        assert!(
            matches!(
                outcome.faults.as_slice(),
                [InputFault::Device {
                    tick: Tick(0),
                    error: AdapterError::ReadingRejected {
                        device,
                        channel: cs_types::input::AxisChannel::Joystick(0),
                        error: CalibrationError::NonFiniteReading { .. },
                    },
                }] if *device == stick()
            ),
            "the refused report is named, device and channel included: {:?}",
            outcome.faults
        );
        let queued = session.take_faults();
        assert_eq!(queued.len(), 1, "a caller that only drains still sees it");
        assert!(
            matches!(
                queued.as_slice(),
                [InputFault::Device { tick: Tick(0), .. }]
            ),
            "{queued:?}"
        );
        assert!(
            session.take_faults().is_empty(),
            "each fault is drained once"
        );
    }

    /// AC03 at the module level: the stream a session records at its input
    /// boundary replays into the same consumer state at any display rate.
    #[test]
    fn accept_f22_c_a_recorded_command_stream_replays_at_any_display_rate() {
        let recorded = record_pilot_stream();
        assert_eq!(
            recorded.len() as u64,
            TICKS_PER_SECOND,
            "the reference recording has one record per committed tick"
        );
        assert_eq!(
            recorded.edges(),
            vec![
                fire(),
                Action::Flight(FlightCommand::ThrottleStepUp),
                Action::Flight(FlightCommand::ThrottleStepUp),
            ],
            "the pilot pressed fire once and the throttle twice"
        );

        let reference = replay_run(&recorded, REFERENCE_FPS);
        assert!(reference.orphaned == 0);
        assert!(reference.remaining == 0);
        assert_eq!(
            reference.tick_of_edge,
            vec![Tick(12), Tick(20), Tick(30)],
            "at the reference rate each recorded tick's edge executes on its own tick"
        );

        for frame_rate in [30_u32, 60] {
            let run = replay_run(&recorded, frame_rate);
            assert_eq!(
                run.delivered, reference.delivered,
                "{frame_rate} FPS must execute the same commands in the same order"
            );
            assert_eq!(
                run.throttle, reference.throttle,
                "{frame_rate} FPS must reach the same throttle"
            );
            assert_eq!(
                run.axes, reference.axes,
                "{frame_rate} FPS must reach the same axis state"
            );
            assert_eq!(run.orphaned, 0, "{frame_rate} FPS orphaned a record");
            assert_eq!(run.remaining, 0, "{frame_rate} FPS left records behind");
            assert_eq!(
                run.frames,
                u64::from(frame_rate) + 1,
                "{frame_rate} FPS is a different frame split"
            );

            // The display rate moves *when* a command is executed, never which
            // command it is. A coarse frame delivers the commands of every tick
            // it covers at its own first boundary, so a command can arrive up to
            // one frame's worth of ticks *early* — the sample latency a lower
            // display rate buys — and never later than the tick it was recorded
            // on.
            let frame_ticks = (TICKS_PER_SECOND / u64::from(frame_rate)) + 1;
            for (index, tick) in run.tick_of_edge.iter().enumerate() {
                let recorded_tick = reference.tick_of_edge[index];
                assert!(
                    *tick <= recorded_tick,
                    "{frame_rate} FPS executed command {index} on tick {}, after its recorded \
                     tick {}",
                    tick.0,
                    recorded_tick.0
                );
                assert!(
                    recorded_tick.0 - tick.0 < frame_ticks,
                    "{frame_rate} FPS executed command {index} {} ticks early, beyond the \
                     {frame_ticks} ticks one frame covers",
                    recorded_tick.0 - tick.0
                );
            }
        }

        // A throttle position that only a per-frame step could produce.
        assert!(
            (reference.throttle - ThrottleSteps::DESIGNED_STEP * 2.0).abs() < 1e-6,
            "two presses are two steps however the frames were cut, got {}",
            reference.throttle
        );
        // And the pitch key is still down at the end, so the axis comparison is
        // over a real deflection rather than two neutrals.
        assert!(
            reference
                .axes
                .iter()
                .any(|(command, value)| *command == FlightCommand::Pitch && *value > 0.9),
            "the held pitch key is still driving the axis, got {:?}",
            reference.axes
        );
    }

    /// The fixture's fixed rate: 64 Hz.
    const TICK_HZ: u32 = 64;
    /// The reference recording's display rate: 144 FPS, the finest sampling the
    /// fixture compares.
    const REFERENCE_FPS: u32 = 144;

    /// The pilot script of the fixture, as the keys that are down at `tick`.
    fn pilot_keys(tick: u64) -> Vec<Key> {
        let mut down = Vec::new();
        if tick >= 4 {
            // The pitch key stays down to the end of the run, so the final axis
            // state is a real deflection and not a neutral that would make the
            // comparison vacuous.
            down.push(Key::S);
        }
        if tick == 12 {
            down.push(Key::Space);
        }
        if tick == 20 || tick == 30 {
            down.push(Key::R);
        }
        down
    }

    /// One second of wall time, split into `frame_rate` render frames. The
    /// frames are `frame_rate` equal spans plus a final remainder frame, so
    /// their sum is exactly one second at any frame rate — the same frame split
    /// the F16-C clock test uses, and the reason every run reaches
    /// [`TICKS_PER_SECOND`] ticks whatever its display rate is.
    fn frame_spans(frame_rate: u32) -> Vec<Duration> {
        let frames = u128::from(frame_rate);
        let per_frame = cs_sim::time::NANOS_PER_SECOND / frames;
        let rest = cs_sim::time::NANOS_PER_SECOND - per_frame * frames;
        let mut spans =
            vec![
                Duration::from_nanos(u64::try_from(per_frame).expect("a frame is under a second"));
                frame_rate as usize
            ];
        spans.push(Duration::from_nanos(
            u64::try_from(rest).expect("the remainder frame is under a second"),
        ));
        spans
    }

    /// Ticks one second of wall time commits at [`TICK_HZ`].
    const TICKS_PER_SECOND: u64 = TICK_HZ as u64;

    /// Records one second of the pilot script at [`REFERENCE_FPS`], which is
    /// the finest sampling of the fixture: every boundary sees the report that
    /// belongs to its own tick.
    fn record_pilot_stream() -> CommandStream {
        let mut clock = SimClock::with_tick(
            SessionMode::SinglePlayer.clock_policy(),
            TickRate::new(TICK_HZ).expect("64 Hz is a valid rate"),
            Tick(0),
        );
        let mut session =
            InputSession::designed_default(seat(), SessionMode::SinglePlayer, Tick(0));
        session
            .collector_mut()
            .connect_device(keyboard())
            .expect("the keyboard connects");
        session.start_recording();
        for span in frame_spans(REFERENCE_FPS) {
            let ticks = clock.advance(span).expect("the clock advances");
            // The report is the one belonging to the tick this frame is stamped
            // for, exactly as a producer that polls once per render frame feeds
            // the frame it is building.
            let events = keys(&pilot_keys(session.tick().0));
            let outcome = session
                .pump_frame(FrameInput::Devices(&events), ticks)
                .expect("the fixture frame applies");
            assert!(outcome.is_clean(), "{:?}", outcome.faults);
        }
        assert_eq!(
            session.tick(),
            Tick(TICKS_PER_SECOND),
            "one second at {TICK_HZ} Hz"
        );
        session.stop_recording()
    }

    /// What one replay produced.
    #[derive(Debug, PartialEq)]
    struct ReplayRun {
        /// The ordered command sequence the consumer executed.
        delivered: Vec<Action>,
        /// The throttle the replay's boundaries moved it to.
        throttle: f32,
        /// The final deflection of every axis the run drove.
        axes: Vec<(FlightCommand, f32)>,
        /// The tick each command executed on.
        tick_of_edge: Vec<Tick>,
        /// Recorded ticks the replay passed over.
        orphaned: usize,
        /// Recorded ticks the replay never reached.
        remaining: usize,
        /// The render frames the replay pumped.
        frames: u64,
    }

    /// Replays `recorded` for one second at `frame_rate` and reports what the
    /// consumer executed.
    ///
    /// The replayed session records its **own** stream while it replays, so the
    /// per-tick attribution below is the consumer trace the production path
    /// produced — not an attribution reconstructed by the test.
    fn replay_run(recorded: &CommandStream, frame_rate: u32) -> ReplayRun {
        let rate = TickRate::new(TICK_HZ).expect("64 Hz is a valid rate");
        let mut session =
            InputSession::designed_default(seat(), SessionMode::SinglePlayer, Tick(0));
        session.start_recording();
        let mut replay =
            CommandReplay::new(recorded.clone(), rate, SessionMode::SinglePlayer, Tick(0))
                .expect("the replay clock is valid");
        for span in frame_spans(frame_rate) {
            replay
                .frame(&mut session, span)
                .expect("the replay frame applies");
        }
        let replayed = session.stop_recording();
        let mut delivered = Vec::new();
        let mut tick_of_edge = Vec::new();
        for record in replayed.records() {
            for edge in record.edges() {
                tick_of_edge.push(record.frame_tick());
                delivered.push(*edge);
            }
        }
        let report = replay.report();
        ReplayRun {
            delivered,
            throttle: session.throttle().position(),
            axes: FlightCommand::CONTINUOUS
                .iter()
                .filter_map(|command| {
                    session
                        .controls()
                        .axis(*command)
                        .map(|value| (*command, value))
                })
                .collect(),
            tick_of_edge,
            orphaned: report.orphaned,
            remaining: report.remaining,
            frames: report.frames,
        }
    }

    /// The replay window merges a frame's ticks into one live-shaped frame, and
    /// an axis in it is the **freshest** sample the window states: a render
    /// boundary reports the newest reading, not the oldest one it happens to
    /// cover.
    #[test]
    fn accept_f22_c_a_replay_window_merges_to_the_freshest_sample() {
        let mut recorded = CommandStream::new();
        for (index, value) in [(-0.75_f32), (0.25), (0.5)].into_iter().enumerate() {
            let mut record = InputFrame::new(Tick(index as u64));
            record.set_axis(AxisValue::from_unit(FlightCommand::Pitch, value).expect("valid"));
            if index == 1 {
                record.push_edge(Action::Flight(FlightCommand::ThrottleStepUp));
            }
            recorded.record_tick(record).expect("the record follows");
        }
        let mut rest = InputFrame::new(Tick(3));
        rest.set_axis(AxisValue::from_unit(FlightCommand::Pitch, -1.0).expect("valid"));
        recorded.record_tick(rest).expect("the last record follows");

        // One render frame covering ticks 0..3 merges the first three records.
        let mut cursor = ReplayCursor::new(recorded.clone());
        let (frame, window) = cursor.frame(Tick(0), 3);
        assert_eq!(
            frame.frame_tick(),
            Tick(0),
            "a frame is stamped at its own start"
        );
        assert_eq!(window.matched, 3);
        assert_eq!(window.orphaned, 0);
        assert_eq!(
            frame.axes(),
            &[AxisValue::from_unit(FlightCommand::Pitch, 0.5).expect("valid")],
            "the axis is the freshest sample the window states"
        );
        assert_eq!(
            frame.edges(),
            &[Action::Flight(FlightCommand::ThrottleStepUp)],
            "the window's edges are merged in tick order"
        );

        // The next window takes the rest, and the cursor is finished.
        let (frame, window) = cursor.frame(Tick(3), 3);
        assert_eq!(window.matched, 1);
        assert_eq!(
            frame.axes(),
            &[AxisValue::from_unit(FlightCommand::Pitch, -1.0).expect("valid")]
        );
        assert!(cursor.is_exhausted());

        // A window that covers no record is an empty frame, and every record the
        // session has already passed is counted, never delivered late.
        let mut behind = ReplayCursor::new(recorded);
        let (frame, window) = behind.frame(Tick(4), 1);
        assert!(frame.is_inert(), "a window with no record asks for nothing");
        assert_eq!(window.matched, 0);
        assert_eq!(
            (window.orphaned, behind.orphaned()),
            (4, 4),
            "every record the session had already passed is counted, not \
             delivered late"
        );
        assert!(behind.is_exhausted());
        let (frame, window) = behind.frame(Tick(5), 1);
        assert!(frame.is_inert());
        assert_eq!((window.matched, window.orphaned), (0, 0));
    }

    /// A pause session refuses the ticks a caller asked for, keeps its UI
    /// actions flowing to the pause screen and drops the input it queued before
    /// the pause, so a resume cannot resurrect it.
    #[test]
    fn accept_f22_c_a_pause_refuses_ticks_but_keeps_the_pause_screen_alive() {
        let mut session = session(SessionMode::SinglePlayer);
        // A press is queued between boundaries: no boundary has run yet, so the
        // edge is still waiting for one.
        session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Space])), 0)
            .expect("the fixture frame applies");
        assert_eq!(session.controls().pending_edges(), 1);

        // The player opens the pause screen: the input path pauses, releases the
        // hold and discards the press that was queued before it.
        let paused = session.pause(PauseReason::PlayerRequest);
        assert_eq!(
            paused.decision,
            PauseDecision::Paused(PauseReason::PlayerRequest)
        );
        assert_eq!(paused.released.reason, Some(HandoverReason::Paused));
        assert_eq!(
            paused.released.released.released_edges,
            vec![fire()],
            "a pause cannot leave the guns firing"
        );
        assert_eq!(
            paused.released.discarded_edges,
            vec![fire()],
            "the press queued before the pause is discarded here, not resurrected \
             by the resume"
        );
        assert!(
            session.collector().devices().held_edges().is_empty(),
            "and the device holds went with it"
        );

        // The pause screen itself is driven by UI actions, which keep flowing.
        session.set_context(InputContext::UiNavigation);
        let menu = session
            .pump_frame(FrameInput::Devices(&keys(&[Key::Enter, Key::Space])), 2)
            .expect("the fixture frame applies");
        assert_eq!(menu.ui_requests, 1, "confirm reaches the pause screen");
        assert_eq!(menu.ticks_ran, 0, "a frozen world runs no input boundary");
        assert_eq!(menu.ticks_refused, 2);
        assert!(menu.delivered.is_empty());
        assert!(matches!(
            menu.faults.first(),
            Some(InputFault::TicksWhilePaused { requested: 2, .. })
        ));

        // The pre-pause press does not survive the resume.
        let handed = session.resume();
        assert_eq!(handed.reason, Some(HandoverReason::Paused));
        assert!(
            handed.discarded_edges.is_empty(),
            "the press was already discarded when the pause was entered: {handed:?}"
        );
        assert!(!session.is_paused());
        session.set_context(InputContext::Flight);
        let after = session
            .pump_frame(FrameInput::Devices(&keys(&[])), 1)
            .expect("the fixture frame applies");
        assert!(
            after.delivered.is_empty(),
            "the discarded press must not fire on resume: {:?}",
            after.delivered
        );
        assert!(
            after.is_clean(),
            "and the resume itself is not a fault: {:?}",
            after.faults
        );

        // An already-paused session keeps the reason it had, and releases
        // nothing a second time.
        assert_eq!(
            session.pause(PauseReason::Menu).decision,
            PauseDecision::Paused(PauseReason::Menu)
        );
        let again = session.pause(PauseReason::FocusLost);
        assert_eq!(
            again.decision,
            PauseDecision::AlreadyPaused(PauseReason::Menu)
        );
        assert!(again.released.is_empty(), "{:?}", again.released);
        assert_eq!(session.pause_reason(), Some(PauseReason::Menu));
        session.resume();
        assert!(
            !session.resume().reason.is_some(),
            "resuming twice is a no-op"
        );
        assert!((ThrottleSteps::IDLE - session.throttle().position()).abs() < 1e-6);
    }
}
