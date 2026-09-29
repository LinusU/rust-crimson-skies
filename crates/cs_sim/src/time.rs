//! Typed time: integer ticks, fixed rate, explicit pause and speed-up
//! policies (F16-A).
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stage `### F16-A`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Non-negotiable behavior 3 and 4 are structural here, not conventions a
//! caller remembers:
//!
//! * **Simulation time is an integer tick count with a fixed
//!   dt** ([`TickRate`]). [`SimClock::advance`] accumulates whole
//!   nanoseconds with integer arithmetic, so the same wall time advances
//!   exactly the same ticks whether it arrived as 30, 60 or 144 frames —
//!   no floating accumulator can drift across frame boundaries.
//! * **UI wall time, unscaled media time and authoritative gameplay time
//!   are distinct** ([`TimeDomain`]), each with its own [`ClockPolicy`].
//! * **Pause and time acceleration are explicit per subsystem.**
//!   [`PausePolicy::Freeze`] clocks return zero ticks while paused and do
//!   not bank the paused time; [`PausePolicy::KeepRunning`] clocks (UI wall
//!   clock, unscaled media) keep advancing. Single-player speed-up advances
//!   fixed ticks through [`SpeedUpPolicy::AdvanceFixedTicks`]; the
//!   multiplayer policy is [`SpeedUpPolicy::NoLocalAuthority`] and rejects
//!   locally injected ticks with [`TimeError::NoSpeedUpAuthority`].
//!
//! The policies are designed defaults that satisfy the sheet's rules; which
//! pairings the original game uses is F16-D's measurement.
//!
//! # F16-D: gameplay quantities and behavioral probes
//!
//! [`GameplayTimeline`] is the production consumer of those policies for the
//! two gameplay quantities the sheet names in AC04: a weapon cooldown and an
//! objective timer, both [`TickTimer`]s measured in whole ticks. Neither has
//! a wall-time entry point — only ticks a clock already committed — so "pause
//! advances nothing" is a property of the type rather than a rule a caller
//! has to remember.
//!
//! [`BehavioralProbe`] runs a scripted timeline and returns a
//! [`ProbeTrace`]: a measured, ordered trace with a label on every sample.
//! [`ProbeReference`] holds a reference trace together with the
//! [`EvidenceRecord`] that backs it and the tolerance selected *before* the
//! comparison; [`ProbeComparison`] reports the divergences and the claim the
//! evidence supports. A comparison that agrees can only claim
//! [`ClaimStatus::ObservedTool`] unless the reference's own evidence
//! [`verifies_original`](EvidenceRecord::verifies_original) — a passing test
//! cannot award originality.

use std::time::Duration;

use cs_types::Tick;
use cs_types::evidence::{ClaimStatus, EvidenceRecord};

/// Nanoseconds in one second, the denominator of the tick accumulator.
pub const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Why a clock operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimeError {
    /// A tick rate of zero was requested; there would be no fixed dt.
    ZeroTickRate,
    /// Locally injected ticks were refused because this clock's policy
    /// grants no local speed-up authority (multiplayer sessions).
    NoSpeedUpAuthority,
    /// Ticks were injected into a paused clock whose pause policy freezes
    /// it; paused time must produce no advancement at all.
    ClockPaused,
    /// The tick counter would have wrapped; ticks are never reused.
    TickOverflow,
    /// A gameplay timer was built with a zero-tick period. A cooldown or an
    /// objective deadline is a positive duration; a zero-tick one has no
    /// meaning and no seconds equivalent.
    ZeroTimerPeriod,
}

impl std::fmt::Display for TimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroTickRate => write!(f, "tick rate must be greater than zero"),
            Self::NoSpeedUpAuthority => write!(f, "this clock grants no local speed-up authority"),
            Self::ClockPaused => write!(f, "the clock is paused and freezes its ticks"),
            Self::TickOverflow => write!(f, "tick counter would overflow"),
            Self::ZeroTimerPeriod => write!(
                f,
                "a gameplay timer period must be at least one tick, got zero"
            ),
        }
    }
}

impl std::error::Error for TimeError {}

/// Fixed simulation rate: the single dt every tick of a run uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TickRate {
    ticks_per_second: u32,
}

impl TickRate {
    /// Validates the fixed rate.
    ///
    /// # Errors
    ///
    /// [`TimeError::ZeroTickRate`] when `ticks_per_second` is zero.
    pub fn new(ticks_per_second: u32) -> Result<Self, TimeError> {
        if ticks_per_second == 0 {
            return Err(TimeError::ZeroTickRate);
        }
        Ok(Self { ticks_per_second })
    }

    /// Ticks per second.
    #[must_use]
    pub const fn ticks_per_second(self) -> u32 {
        self.ticks_per_second
    }

    /// The fixed dt of one tick, in seconds.
    #[must_use]
    pub fn dt_seconds(self) -> f64 {
        1.0 / f64::from(self.ticks_per_second)
    }
}

/// Which clock a policy describes (`F16` non-negotiable behavior 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeDomain {
    /// Authoritative gameplay ticks of the local session.
    Simulation,
    /// Real time for menus and UI; it does not pause with the game.
    UiWall,
    /// Media time that is never scaled by pause or speed-up (video, music).
    MediaUnscaled,
    /// The gameplay time a session is judged by: objective timers, weapon
    /// cooldowns and networked state.
    AuthoritativeGameplay,
}

impl TimeDomain {
    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Simulation => "simulation",
            Self::UiWall => "ui-wall",
            Self::MediaUnscaled => "media-unscaled",
            Self::AuthoritativeGameplay => "authoritative-gameplay",
        }
    }
}

/// What happens to a subsystem's clock while the local player is paused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PausePolicy {
    /// The clock stops: zero advancement while paused (simulation,
    /// authoritative gameplay — a paused run must not advance weapon
    /// cooldowns or objective timers).
    Freeze,
    /// The clock keeps advancing while gameplay is paused (UI wall time,
    /// unscaled media).
    KeepRunning,
}

/// Whether a subsystem may advance fixed ticks on local authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpeedUpPolicy {
    /// Single-player sessions may advance fixed ticks: speed-up is a number
    /// of whole ticks, never a variable dt.
    AdvanceFixedTicks,
    /// No local speed-up authority (multiplayer sessions and every clock
    /// that is not the simulation).
    NoLocalAuthority,
}

/// The explicit pause and speed-up policy of one time domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockPolicy {
    domain: TimeDomain,
    pause: PausePolicy,
    speed_up: SpeedUpPolicy,
}

impl ClockPolicy {
    /// The single-player simulation clock: freezes on pause and accepts
    /// local speed-up as whole ticks.
    #[must_use]
    pub const fn single_player_simulation() -> Self {
        Self {
            domain: TimeDomain::Simulation,
            pause: PausePolicy::Freeze,
            speed_up: SpeedUpPolicy::AdvanceFixedTicks,
        }
    }

    /// The multiplayer simulation clock: freezes on pause but grants the
    /// local client no speed-up authority (`F16` non-negotiable behavior 4).
    #[must_use]
    pub const fn multiplayer_simulation() -> Self {
        Self {
            domain: TimeDomain::Simulation,
            pause: PausePolicy::Freeze,
            speed_up: SpeedUpPolicy::NoLocalAuthority,
        }
    }

    /// UI wall time: keeps running while the game is paused, no speed-up.
    #[must_use]
    pub const fn ui_wall() -> Self {
        Self {
            domain: TimeDomain::UiWall,
            pause: PausePolicy::KeepRunning,
            speed_up: SpeedUpPolicy::NoLocalAuthority,
        }
    }

    /// Unscaled media time: keeps running while the game is paused, no
    /// speed-up.
    #[must_use]
    pub const fn media_unscaled() -> Self {
        Self {
            domain: TimeDomain::MediaUnscaled,
            pause: PausePolicy::KeepRunning,
            speed_up: SpeedUpPolicy::NoLocalAuthority,
        }
    }

    /// Authoritative gameplay time: freezes on pause so paused play advances
    /// neither weapon cooldowns nor objective timers, and grants no local
    /// speed-up authority.
    #[must_use]
    pub const fn authoritative_gameplay() -> Self {
        Self {
            domain: TimeDomain::AuthoritativeGameplay,
            pause: PausePolicy::Freeze,
            speed_up: SpeedUpPolicy::NoLocalAuthority,
        }
    }

    /// The domain this policy describes.
    #[must_use]
    pub const fn domain(self) -> TimeDomain {
        self.domain
    }

    /// What happens to this clock while the local player is paused.
    #[must_use]
    pub const fn pause(self) -> PausePolicy {
        self.pause
    }

    /// Whether fixed ticks may be injected locally.
    #[must_use]
    pub const fn speed_up(self) -> SpeedUpPolicy {
        self.speed_up
    }
}

/// The fixed-tick accumulator of one subsystem.
///
/// Time enters as a wall-clock [`Duration`] and leaves as integer ticks;
/// paused wall time of a [`PausePolicy::Freeze`] clock is discarded rather
/// than banked, so resuming never dumps a burst of ticks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimClock {
    policy: ClockPolicy,
    rate: TickRate,
    tick: Tick,
    /// Nanoseconds worth of ticks (× `ticks_per_second`) carried into the
    /// next advance, always below [`NANOS_PER_SECOND`].
    carry: u128,
    paused: bool,
}

impl SimClock {
    /// A clock starting at [`Tick`] 0.
    #[must_use]
    pub const fn new(policy: ClockPolicy, rate: TickRate) -> Self {
        Self::with_tick(policy, rate, Tick(0))
    }

    /// A clock starting at an explicit tick (session restore, probes).
    #[must_use]
    pub const fn with_tick(policy: ClockPolicy, rate: TickRate, tick: Tick) -> Self {
        Self {
            policy,
            rate,
            tick,
            carry: 0,
            paused: false,
        }
    }

    /// The policy this clock was configured with.
    #[must_use]
    pub const fn policy(&self) -> ClockPolicy {
        self.policy
    }

    /// The fixed rate of this clock.
    #[must_use]
    pub const fn rate(&self) -> TickRate {
        self.rate
    }

    /// The last committed tick.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Whether the local player is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pauses or resumes this clock. Whether paused time freezes or keeps
    /// running is [`ClockPolicy::pause`], not the caller's assumption.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Whether this clock stops while paused.
    #[must_use]
    pub const fn freezes_while_paused(&self) -> bool {
        matches!(self.policy.pause(), PausePolicy::Freeze)
    }

    /// Advances the clock by `elapsed` wall time and returns the ticks
    /// gained.
    ///
    /// The accumulation is integer-only: `carry + elapsed_ns · hz` divided
    /// by 10⁹, with the remainder carried over, so any frame split of the
    /// same total wall time yields the same tick count. A paused
    /// [`PausePolicy::Freeze`] clock returns zero and drops the elapsed
    /// time instead of banking it.
    ///
    /// # Errors
    ///
    /// [`TimeError::TickOverflow`] if the tick counter would wrap; the
    /// clock is left unchanged when it does.
    pub fn advance(&mut self, elapsed: Duration) -> Result<u64, TimeError> {
        if self.paused && self.freezes_while_paused() {
            return Ok(0);
        }
        let numerator = self.carry + elapsed.as_nanos() * u128::from(self.rate.ticks_per_second());
        let ticks = numerator / NANOS_PER_SECOND;
        let carry = numerator % NANOS_PER_SECOND;
        let ticks = u64::try_from(ticks).map_err(|_| TimeError::TickOverflow)?;
        let next = self
            .tick
            .0
            .checked_add(ticks)
            .ok_or(TimeError::TickOverflow)?;
        self.tick = Tick(next);
        self.carry = carry;
        Ok(ticks)
    }

    /// Advances the clock by whole ticks on local authority — the only way
    /// single-player speed-up may run.
    ///
    /// # Errors
    ///
    /// [`TimeError::NoSpeedUpAuthority`] when the policy grants no local
    /// speed-up authority, [`TimeError::ClockPaused`] when a freezing clock
    /// is paused, and [`TimeError::TickOverflow`] if the counter would
    /// wrap. Nothing is mutated on error.
    pub fn advance_fixed_ticks(&mut self, ticks: u64) -> Result<(), TimeError> {
        if self.policy.speed_up() == SpeedUpPolicy::NoLocalAuthority {
            return Err(TimeError::NoSpeedUpAuthority);
        }
        if self.paused && self.freezes_while_paused() {
            return Err(TimeError::ClockPaused);
        }
        let next = self
            .tick
            .0
            .checked_add(ticks)
            .ok_or(TimeError::TickOverflow)?;
        self.tick = Tick(next);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// F16-D: gameplay tick timers, behavioral probes and reference comparison
// ---------------------------------------------------------------------------

/// Which gameplay quantity a [`TickTimer`] measures.
///
/// The sheet names the two in AC04 — "Pause produces zero weapon cooldown and
/// objective timer advancement" — so they are the two kinds here. The kind is
/// data rather than a comment: a probe record and a divergence report say
/// *which* quantity was measured, and a consumer can never read a cooldown
/// trace as an objective trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimerKind {
    /// Time left before a weapon may fire again.
    WeaponCooldown,
    /// Time left before an objective's deadline.
    ObjectiveTimer,
}

impl TimerKind {
    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WeaponCooldown => "weapon-cooldown",
            Self::ObjectiveTimer => "objective-timer",
        }
    }
}

/// A gameplay duration measured in whole fixed ticks: a weapon cooldown or an
/// objective timer.
///
/// The timer has **no wall-time entry point**. [`commit`](Self::commit) is
/// private to this module, so the only way a timer can move is whole ticks
/// that some clock already committed — which is the structural form of AC04:
/// a paused frame commits zero ticks, so a paused frame cannot shorten a
/// cooldown or move an objective deadline. A caller cannot express "this
/// cooldown ticks down by 0.25 s" or "this timer runs while paused", because
/// no method takes a [`Duration`].
///
/// The period is authored in ticks, never in seconds, so a session's fixed
/// rate is the single place a duration is defined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickTimer {
    kind: TimerKind,
    period: u64,
    remaining: u64,
    expirations: u64,
}

impl TickTimer {
    /// Builds a timer counting down `period` ticks from now.
    ///
    /// # Errors
    ///
    /// [`TimeError::ZeroTimerPeriod`] when `period` is zero.
    pub fn new(kind: TimerKind, period: u64) -> Result<Self, TimeError> {
        if period == 0 {
            return Err(TimeError::ZeroTimerPeriod);
        }
        Ok(Self {
            kind,
            period,
            remaining: period,
            expirations: 0,
        })
    }

    /// Which gameplay quantity this timer measures.
    #[must_use]
    pub const fn kind(&self) -> TimerKind {
        self.kind
    }

    /// The authored period, in ticks.
    #[must_use]
    pub const fn period_ticks(&self) -> u64 {
        self.period
    }

    /// Ticks left before the timer expires. Zero once it has.
    #[must_use]
    pub const fn remaining_ticks(&self) -> u64 {
        self.remaining
    }

    /// Ticks consumed since the timer was last armed.
    #[must_use]
    pub const fn elapsed_ticks(&self) -> u64 {
        self.period - self.remaining
    }

    /// Whether the timer has run out.
    #[must_use]
    pub const fn is_expired(&self) -> bool {
        self.remaining == 0
    }

    /// How many times this timer has run out since it was built.
    #[must_use]
    pub const fn expirations(&self) -> u64 {
        self.expirations
    }

    /// The period as seconds at `rate`, for reporting.
    ///
    /// # Errors
    ///
    /// [`TimeError::TickOverflow`] when the conversion is not finite.
    pub fn period_seconds(&self, rate: TickRate) -> Result<f64, TimeError> {
        let seconds = self.period as f64 * rate.dt_seconds();
        if seconds.is_finite() {
            Ok(seconds)
        } else {
            Err(TimeError::TickOverflow)
        }
    }

    /// Re-arms the timer to its full period, as firing a weapon does.
    pub fn restart(&mut self) {
        self.remaining = self.period;
    }

    /// Consumes `ticks` whole committed ticks. Saturates at zero and counts
    /// the single moment the timer runs out, so a long frame cannot bank a
    /// second expiration for one expiration.
    fn commit(&mut self, ticks: u64) {
        if self.remaining == 0 {
            return;
        }
        self.remaining = self.remaining.saturating_sub(ticks);
        if self.remaining == 0 {
            self.expirations += 1;
        }
    }
}

/// One session's authoritative gameplay clock together with the gameplay
/// quantities that share it.
///
/// This is the production consumer AC04 names. The timeline owns a
/// [`ClockPolicy::authoritative_gameplay`] clock and advances every timer from
/// the **whole ticks that clock committed**, so:
/// * a paused frame commits zero ticks, so no cooldown and no objective timer
///   advances (the minimum scenario);
/// * a frame's wall delta never becomes a variable dt, because the clock
///   hands out whole ticks (AC03);
/// * single-player speed-up, which is a number of whole ticks on the
///   simulation clock, reaches these timers as ticks and never as wall time.
///
/// The timeline deliberately exposes no speed-up of its own: the authoritative
/// gameplay policy grants no local speed-up authority, so
/// [`advance_fixed_ticks`](GameplayTimeline::advance_fixed_ticks) refuses and
/// the session driver (F16-C) is the thing that runs extra fixed ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct GameplayTimeline {
    clock: SimClock,
    cooldown: TickTimer,
    objective: TickTimer,
}

impl GameplayTimeline {
    /// Builds a timeline at `rate` with the two gameplay quantities AC04
    /// names.
    ///
    /// # Errors
    ///
    /// [`TimeError::ZeroTimerPeriod`] when either period is zero.
    pub fn new(
        rate: TickRate,
        cooldown_ticks: u64,
        objective_ticks: u64,
    ) -> Result<Self, TimeError> {
        Ok(Self {
            clock: SimClock::new(ClockPolicy::authoritative_gameplay(), rate),
            cooldown: TickTimer::new(TimerKind::WeaponCooldown, cooldown_ticks)?,
            objective: TickTimer::new(TimerKind::ObjectiveTimer, objective_ticks)?,
        })
    }

    /// The authoritative gameplay clock.
    #[must_use]
    pub const fn clock(&self) -> &SimClock {
        &self.clock
    }

    /// The weapon cooldown.
    #[must_use]
    pub const fn cooldown(&self) -> &TickTimer {
        &self.cooldown
    }

    /// The objective timer.
    #[must_use]
    pub const fn objective_timer(&self) -> &TickTimer {
        &self.objective
    }

    /// The last committed tick.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.clock.tick()
    }

    /// Whether the local player is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    /// Pauses or resumes the session clock. While paused this timeline's
    /// timers cannot advance: the clock commits no ticks and the timers are
    /// fed only committed ticks.
    pub fn set_paused(&mut self, paused: bool) {
        self.clock.set_paused(paused);
    }

    /// Advances one render frame of `elapsed` wall time, returning the whole
    /// ticks it committed and applying them to every gameplay quantity.
    ///
    /// # Errors
    ///
    /// [`TimeError::TickOverflow`] if the tick counter would wrap; nothing is
    /// mutated when it does.
    pub fn advance_frame(&mut self, elapsed: Duration) -> Result<u64, TimeError> {
        let ticks = self.clock.advance(elapsed)?;
        self.cooldown.commit(ticks);
        self.objective.commit(ticks);
        Ok(ticks)
    }

    /// Injects whole ticks on local authority, as single-player speed-up is
    /// defined to be.
    ///
    /// The authoritative gameplay policy grants no local speed-up authority
    /// (`F16` non-negotiable behavior 4), so this always refuses; it exists so
    /// that the refusal is a named, tested boundary rather than a missing
    /// method.
    ///
    /// # Errors
    ///
    /// [`TimeError::NoSpeedUpAuthority`], and [`TimeError::ClockPaused`] if
    /// the clock is paused.
    pub fn advance_fixed_ticks(&mut self, ticks: u64) -> Result<u64, TimeError> {
        self.clock.advance_fixed_ticks(ticks)?;
        self.cooldown.commit(ticks);
        self.objective.commit(ticks);
        Ok(ticks)
    }

    /// Fires the weapon: the cooldown re-arms to its full period. The
    /// objective timer is untouched — firing is not a pause.
    pub fn fire(&mut self) {
        self.cooldown.restart();
    }
}

// ---------------------------------------------------------------------------
// Behavioral probes: a measured trace, a reference trace, and their comparison
// ---------------------------------------------------------------------------

/// One scripted step of a behavioral probe.
///
/// A probe is a *script*, not a test: the same script runs at every render
/// frame rate and produces a trace, and the traces are what get compared. That
/// is what makes "does pause freeze the game" a measurement (the trace shows
/// zero advancement) rather than an assertion someone wrote twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeStep {
    /// Advance `nanos` of wall time, delivered as `frames` render frames of
    /// the same length plus a final remainder frame. Zero frames means "one
    /// frame carrying all of it", so a probe can express a single long frame.
    Advance {
        /// Wall time to deliver, in nanoseconds.
        nanos: u64,
        /// How many render frames to deliver it in.
        frames: u32,
    },
    /// Pause or resume the session clock.
    Pause(bool),
    /// Fire the weapon, re-arming the cooldown.
    Fire,
    /// Record a labelled sample of the current state.
    Observe(&'static str),
}

/// One labelled sample inside a [`ProbeTrace`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeSample {
    label: &'static str,
    tick: Tick,
    paused: bool,
    cooldown_remaining_ticks: u64,
    cooldown_period_ticks: u64,
    cooldown_expirations: u64,
    objective_remaining_ticks: u64,
    objective_period_ticks: u64,
}

impl ProbeSample {
    /// The label the step asked for.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        self.label
    }

    /// The tick this sample was taken on.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Whether the session was paused.
    #[must_use]
    pub const fn paused(&self) -> bool {
        self.paused
    }

    /// Ticks left on the weapon cooldown.
    #[must_use]
    pub const fn cooldown_remaining_ticks(&self) -> u64 {
        self.cooldown_remaining_ticks
    }

    /// The authored cooldown period.
    #[must_use]
    pub const fn cooldown_period_ticks(&self) -> u64 {
        self.cooldown_period_ticks
    }

    /// How many times the cooldown has expired.
    #[must_use]
    pub const fn cooldown_expirations(&self) -> u64 {
        self.cooldown_expirations
    }

    /// Ticks left on the objective timer.
    #[must_use]
    pub const fn objective_remaining_ticks(&self) -> u64 {
        self.objective_remaining_ticks
    }

    /// The authored objective period.
    #[must_use]
    pub const fn objective_period_ticks(&self) -> u64 {
        self.objective_period_ticks
    }
}

/// What a probe measured: the ordered samples, the render frame rate the
/// script was delivered at, and the total ticks it committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeTrace {
    name: &'static str,
    render_fps: u32,
    committed_ticks: u64,
    samples: Vec<ProbeSample>,
}

impl ProbeTrace {
    /// The script's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The render frame rate this trace was delivered at.
    #[must_use]
    pub const fn render_fps(&self) -> u32 {
        self.render_fps
    }

    /// Ticks committed by the whole script.
    #[must_use]
    pub const fn committed_ticks(&self) -> u64 {
        self.committed_ticks
    }

    /// The samples, in order.
    #[must_use]
    pub fn samples(&self) -> &[ProbeSample] {
        &self.samples
    }

    /// The sample labelled `label`, if the script observed it.
    #[must_use]
    pub fn sample(&self, label: &str) -> Option<&ProbeSample> {
        self.samples.iter().find(|s| s.label == label)
    }

    /// A one-line summary for an evidence record: the name, the frame rate,
    /// the tick total and every label with its two gameplay values.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = format!(
            "{}@{}fps ticks={}",
            self.name, self.render_fps, self.committed_ticks
        );
        for sample in &self.samples {
            out.push_str(&format!(
                " | {}@{} cd={}/{} obj={}/{}",
                sample.label,
                sample.tick.0,
                sample.cooldown_remaining_ticks,
                sample.cooldown_period_ticks,
                sample.objective_remaining_ticks,
                sample.objective_period_ticks,
            ));
        }
        out
    }
}

/// Runs a scripted [`ProbeStep`] list against a real [`GameplayTimeline`] and
/// returns what it measured.
///
/// `render_fps` is a *delivery* parameter, not part of the script: the same
/// steps run at 30, 60 and 144 render FPS, and the traces must agree. A probe
/// that only agrees with itself at one frame rate has measured the render
/// loop, not the game.
#[derive(Clone, Copy, Debug)]
pub struct BehavioralProbe {
    steps: &'static [ProbeStep],
}

impl BehavioralProbe {
    /// A probe that runs `steps` verbatim.
    #[must_use]
    pub const fn new(steps: &'static [ProbeStep]) -> Self {
        Self { steps }
    }

    /// The steps this probe runs.
    #[must_use]
    pub const fn steps(&self) -> &'static [ProbeStep] {
        self.steps
    }

    /// Runs the script at `render_fps` against a fresh timeline built by
    /// `make`.
    ///
    /// `make` is supplied by the caller so the timeline's rate, cooldown and
    /// objective period are the caller's scenario, not a hidden constant.
    /// Ticks the timeline cannot commit (because its counter would wrap) are
    /// surfaced by [`ProbeTrace::committed_ticks`] being lower than the sum of
    /// the requested wall time, and by the samples showing the tick they
    /// stopped at.
    ///
    /// # Errors
    ///
    /// [`TimeError::TickOverflow`] if a step's wall time would wrap the tick
    /// counter; the trace is not produced, so a caller can never compare a
    /// truncated run.
    pub fn run<MakeTimeline>(
        &self,
        name: &'static str,
        render_fps: u32,
        mut make: MakeTimeline,
    ) -> Result<ProbeTrace, TimeError>
    where
        MakeTimeline: FnMut() -> Result<GameplayTimeline, TimeError>,
    {
        let mut timeline = make()?;
        let mut samples = Vec::new();
        let mut committed = 0u64;
        for step in self.steps {
            match *step {
                ProbeStep::Advance { nanos, frames } => {
                    for span in frame_spans(nanos, frames) {
                        committed += timeline.advance_frame(span)?;
                    }
                }
                ProbeStep::Pause(paused) => timeline.set_paused(paused),
                ProbeStep::Fire => timeline.fire(),
                ProbeStep::Observe(label) => samples.push(ProbeSample {
                    label,
                    tick: timeline.tick(),
                    paused: timeline.is_paused(),
                    cooldown_remaining_ticks: timeline.cooldown().remaining_ticks(),
                    cooldown_period_ticks: timeline.cooldown().period_ticks(),
                    cooldown_expirations: timeline.cooldown().expirations(),
                    objective_remaining_ticks: timeline.objective_timer().remaining_ticks(),
                    objective_period_ticks: timeline.objective_timer().period_ticks(),
                }),
            }
        }
        Ok(ProbeTrace {
            name,
            render_fps,
            committed_ticks: committed,
            samples,
        })
    }
}

/// Splits `nanos` of wall time into `frames` equal render frames plus a
/// final remainder frame, so the frames always sum to exactly `nanos` and no
/// nanosecond is dropped or invented at a frame boundary.
///
/// `frames == 0` or `nanos == 0` means "one frame carrying everything": a
/// probe can express a single long frame, and a zero-length delivery is still
/// one (empty) frame, without dividing by zero.
fn frame_spans(nanos: u64, frames: u32) -> Vec<Duration> {
    if frames == 0 || nanos == 0 {
        return vec![Duration::from_nanos(nanos)];
    }
    let count = u64::from(frames);
    let per_frame = nanos / count;
    let mut spans = vec![Duration::from_nanos(per_frame); frames as usize];
    let rest = nanos - per_frame * count;
    if rest > 0 {
        spans.push(Duration::from_nanos(rest));
    }
    spans
}

/// A reference trace a measured trace is compared against, with the evidence
/// that backs it and the tolerance the comparison may use.
///
/// This is where F16-D's "collect actual reference evidence" lives. A
/// reference is *attributed*: a `ProbeReference` built from a synthetic
/// fixture says so in [`ProbeReference::evidence`], and the comparison reports
/// [`ProbeComparison::claim`] accordingly. Nothing here can turn a synthetic
/// trace into a `verified_original` claim, because the evidence decides the
/// claim, not the comparison.
#[derive(Clone, Debug)]
pub struct ProbeReference {
    name: &'static str,
    trace: ProbeTrace,
    evidence: EvidenceRecord,
    tolerance_ticks: u64,
}

impl ProbeReference {
    /// A reference trace backed by `evidence`, compared with a tolerance of
    /// `tolerance_ticks` whole ticks.
    ///
    /// The tolerance is *selected before* the comparison, not fitted to it
    /// (`FLIGHT-PHYSICS`, "Calibration acceptance"): the caller states what it
    /// will accept and the comparison reports what actually happened.
    #[must_use]
    pub const fn new(
        name: &'static str,
        trace: ProbeTrace,
        evidence: EvidenceRecord,
        tolerance_ticks: u64,
    ) -> Self {
        Self {
            name,
            trace,
            evidence,
            tolerance_ticks,
        }
    }

    /// The reference's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The reference trace.
    #[must_use]
    pub const fn trace(&self) -> &ProbeTrace {
        &self.trace
    }

    /// The evidence backing this reference.
    #[must_use]
    pub const fn evidence(&self) -> &EvidenceRecord {
        &self.evidence
    }

    /// The declared tolerance, in whole ticks.
    #[must_use]
    pub const fn tolerance_ticks(&self) -> u64 {
        self.tolerance_ticks
    }
}

/// One place a measured trace and a reference trace disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeDivergence {
    /// The sample label the divergence is in.
    pub label: &'static str,
    /// The measured value.
    pub measured: i128,
    /// The reference value.
    pub expected: i128,
    /// Which quantity diverged, e.g. `cooldown_remaining_ticks`.
    pub field: &'static str,
}

impl std::fmt::Display for ProbeDivergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} measured {}, reference {}",
            self.label, self.field, self.measured, self.expected
        )
    }
}

/// The result of comparing a measured [`ProbeTrace`] against a
/// [`ProbeReference`].
///
/// A comparison reports agreement and the claim its *evidence* supports. It
/// never awards `verified_original` by itself: that requires
/// [`EvidenceRecord::verifies_original`], which requires a fingerprinted
/// original source, a locator and a direct observation method. A green
/// comparison against a synthetic reference is `observed_tool` at best.
#[derive(Clone, Debug)]
pub struct ProbeComparison {
    name: &'static str,
    measured: ProbeTrace,
    reference: &'static str,
    divergences: Vec<ProbeDivergence>,
    claim: ClaimStatus,
    verified_original: bool,
}

impl ProbeComparison {
    /// Compares `measured` against `reference` with the reference's own
    /// tolerance.
    ///
    /// Every sample both traces observed is compared on all four gameplay
    /// values plus the tick. A label present in one trace and missing in the
    /// other is itself a divergence: a shorter trace must not pass as agreement.
    #[must_use]
    pub fn new(measured: &ProbeTrace, reference: &ProbeReference) -> Self {
        let tolerance = i128::from(reference.tolerance_ticks());
        let mut divergences = Vec::new();
        for sample in measured.samples() {
            let Some(expected) = reference.trace().sample(sample.label) else {
                divergences.push(ProbeDivergence {
                    label: sample.label,
                    measured: 0,
                    expected: 0,
                    field: "sample-present-in-reference",
                });
                continue;
            };
            let fields: [(&'static str, i128, i128); 5] = [
                (
                    "tick",
                    i128::from(sample.tick.0),
                    i128::from(expected.tick.0),
                ),
                (
                    "paused",
                    i128::from(sample.paused),
                    i128::from(expected.paused),
                ),
                (
                    "cooldown_remaining_ticks",
                    i128::from(sample.cooldown_remaining_ticks),
                    i128::from(expected.cooldown_remaining_ticks),
                ),
                (
                    "objective_remaining_ticks",
                    i128::from(sample.objective_remaining_ticks),
                    i128::from(expected.objective_remaining_ticks),
                ),
                (
                    "cooldown_expirations",
                    i128::from(sample.cooldown_expirations),
                    i128::from(expected.cooldown_expirations),
                ),
            ];
            for (field, got, want) in fields {
                if (got - want).abs() > tolerance {
                    divergences.push(ProbeDivergence {
                        label: sample.label,
                        measured: got,
                        expected: want,
                        field,
                    });
                }
            }
        }
        for sample in reference.trace().samples() {
            if measured.sample(sample.label).is_none() {
                divergences.push(ProbeDivergence {
                    label: sample.label,
                    measured: 0,
                    expected: 0,
                    field: "sample-present-in-measurement",
                });
            }
        }
        let verified_original = reference.evidence().verifies_original();
        Self {
            name: measured.name(),
            measured: measured.clone(),
            reference: reference.name(),
            claim: if !divergences.is_empty() {
                ClaimStatus::Contradicted
            } else if verified_original {
                ClaimStatus::VerifiedOriginal
            } else {
                ClaimStatus::ObservedTool
            },
            divergences,
            verified_original,
        }
    }

    /// The measured trace's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The measured trace.
    #[must_use]
    pub const fn measured(&self) -> &ProbeTrace {
        &self.measured
    }

    /// The reference trace's name.
    #[must_use]
    pub const fn reference(&self) -> &'static str {
        self.reference
    }

    /// Every disagreement found, empty when the traces agree.
    #[must_use]
    pub fn divergences(&self) -> &[ProbeDivergence] {
        &self.divergences
    }

    /// Whether the measured trace matched the reference within tolerance.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.divergences.is_empty()
    }

    /// The claim this comparison supports.
    ///
    /// `VerifiedOriginal` is reachable **only** when the comparison agrees and
    /// the reference's own evidence verifies the original; it is not a status
    /// a passing test can produce by itself.
    #[must_use]
    pub const fn claim(&self) -> ClaimStatus {
        self.claim
    }

    /// Whether the reference evidence can verify the original, independent of
    /// whether the traces agreed.
    #[must_use]
    pub const fn verified_original(&self) -> bool {
        self.verified_original
    }

    /// A one-line summary suitable for an evidence record.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = format!(
            "{} vs {}: {} (claim {})",
            self.measured.summary(),
            self.reference,
            if self.agrees() { "agree" } else { "diverge" },
            self.claim.label(),
        );
        for divergence in &self.divergences {
            out.push_str(&format!("; {divergence}"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(policy: ClockPolicy) -> SimClock {
        SimClock::new(policy, TickRate::new(60).expect("60 Hz is valid"))
    }

    fn frames_at(frame_rate: u32, total_nanos: u128) -> Vec<Duration> {
        let frames = u128::from(frame_rate);
        let per_frame = total_nanos / frames;
        let mut spans = vec![Duration::from_nanos(per_frame as u64); frames as usize];
        let rest = total_nanos - per_frame * frames;
        spans.push(Duration::from_nanos(rest as u64));
        spans
    }

    /// AC03's clock half: identical wall time produces identical ticks at
    /// 30, 60 and 144 render FPS, because the accumulator is integer
    /// nanoseconds rather than a per-frame float.
    #[test]
    fn accept_f16_a_equal_wall_time_at_30_60_144_fps_advances_identical_ticks() {
        let mut counts = Vec::new();
        for frame_rate in [30, 60, 144] {
            let mut clock = clock(ClockPolicy::single_player_simulation());
            let mut total = 0u64;
            for span in frames_at(frame_rate, 2 * NANOS_PER_SECOND) {
                total += clock.advance(span).expect("no overflow");
            }
            assert_eq!(clock.tick().0, total, "every advanced tick is committed");
            counts.push(total);
        }
        assert_eq!(counts[0], counts[1], "30 fps and 60 fps must agree");
        assert_eq!(counts[1], counts[2], "60 fps and 144 fps must agree");
        assert_eq!(counts[0], 120, "2 s at a fixed 60 Hz is exactly 120 ticks");
    }

    /// AC04's clock half: pause must produce zero advancement for both the
    /// simulation clock and the authoritative gameplay clock (weapon
    /// cooldowns, objective timers), while the UI wall and unscaled media
    /// clocks keep running — pause is explicit per subsystem.
    #[test]
    fn accept_f16_a_pause_freezes_simulation_and_objective_ticks() {
        for policy in [
            ClockPolicy::single_player_simulation(),
            ClockPolicy::multiplayer_simulation(),
            ClockPolicy::authoritative_gameplay(),
        ] {
            let mut clock = clock(policy);
            assert_eq!(
                clock.advance(Duration::from_secs(1)).expect("no overflow"),
                60
            );
            clock.set_paused(true);
            assert_eq!(
                clock.advance(Duration::from_secs(5)).expect("no overflow"),
                0,
                "a {} clock must produce zero ticks while paused",
                policy.domain().label()
            );
            assert_eq!(clock.tick().0, 60, "the tick counter must not move");
        }

        let mut wall = clock(ClockPolicy::ui_wall());
        wall.set_paused(true);
        assert_eq!(
            wall.advance(Duration::from_secs(5)).expect("no overflow"),
            300,
            "UI wall time must keep running while gameplay is paused"
        );
    }

    /// Paused wall time of a freezing clock is dropped, not banked: resume
    /// must not release a burst of ticks that never simulated anything.
    #[test]
    fn accept_f16_a_paused_wall_time_is_never_banked() {
        let mut clock = clock(ClockPolicy::single_player_simulation());
        clock.advance(Duration::from_secs(1)).expect("no overflow");
        clock.set_paused(true);
        assert_eq!(
            clock.advance(Duration::from_secs(10)).expect("no overflow"),
            0
        );
        clock.set_paused(false);
        assert_eq!(
            clock.advance(Duration::from_secs(1)).expect("no overflow"),
            60,
            "one second after resume is one second of ticks, not eleven"
        );
        assert_eq!(clock.tick().0, 120);
    }

    /// Non-negotiable 4: only the single-player simulation policy may inject
    /// ticks; the multiplayer clock refuses local speed-up by name, and a
    /// paused freezing clock refuses injected ticks as well.
    #[test]
    fn accept_f16_a_multiplayer_policy_refuses_local_speed_up() {
        let mut multiplayer = clock(ClockPolicy::multiplayer_simulation());
        assert_eq!(
            multiplayer.advance_fixed_ticks(5),
            Err(TimeError::NoSpeedUpAuthority),
            "multiplayer must not expose local speed-up authority"
        );
        assert_eq!(
            multiplayer.tick().0,
            0,
            "a refused speed-up changes nothing"
        );

        let mut single_player = clock(ClockPolicy::single_player_simulation());
        single_player.advance_fixed_ticks(5).expect("allowed");
        assert_eq!(
            single_player.tick().0,
            5,
            "single-player speed-up advances whole ticks"
        );

        single_player.set_paused(true);
        assert_eq!(
            single_player.advance_fixed_ticks(3),
            Err(TimeError::ClockPaused),
            "a paused freezing clock advances nothing at all"
        );
        assert_eq!(single_player.tick().0, 5);
    }

    /// Non-negotiable 3: the four time domains are distinct, and every one
    /// carries an explicit pause and speed-up policy.
    #[test]
    fn accept_f16_a_clock_policies_are_explicit_per_time_domain() {
        let policies = [
            ClockPolicy::single_player_simulation(),
            ClockPolicy::multiplayer_simulation(),
            ClockPolicy::ui_wall(),
            ClockPolicy::media_unscaled(),
            ClockPolicy::authoritative_gameplay(),
        ];
        let mut labels: Vec<&'static str> = policies
            .iter()
            .map(|policy| policy.domain().label())
            .collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels,
            vec![
                "authoritative-gameplay",
                "media-unscaled",
                "simulation",
                "ui-wall"
            ],
            "all four time domains must exist and be distinct"
        );

        for policy in [
            ClockPolicy::single_player_simulation(),
            ClockPolicy::multiplayer_simulation(),
            ClockPolicy::authoritative_gameplay(),
        ] {
            assert_eq!(
                policy.pause(),
                PausePolicy::Freeze,
                "gameplay clocks freeze on pause"
            );
        }
        for policy in [ClockPolicy::ui_wall(), ClockPolicy::media_unscaled()] {
            assert_eq!(
                policy.pause(),
                PausePolicy::KeepRunning,
                "presentation clocks keep running"
            );
            assert_eq!(policy.speed_up(), SpeedUpPolicy::NoLocalAuthority);
        }

        assert_eq!(
            ClockPolicy::single_player_simulation().speed_up(),
            SpeedUpPolicy::AdvanceFixedTicks
        );
        assert_eq!(
            ClockPolicy::multiplayer_simulation().speed_up(),
            SpeedUpPolicy::NoLocalAuthority,
            "single-player speed-up must not leak into multiplayer"
        );
    }

    /// Failure cases of the fixed-rate clock: no zero rate, and no tick
    /// counter wrap (the clock stays unchanged when refused).
    #[test]
    fn accept_f16_a_zero_tick_rate_and_tick_overflow_are_refused() {
        assert_eq!(TickRate::new(0), Err(TimeError::ZeroTickRate));
        let rate = TickRate::new(60).expect("60 Hz is valid");
        assert!(
            (rate.dt_seconds() - 1.0 / 60.0).abs() < 1e-15,
            "fixed dt is 1/60 s"
        );

        let mut clock = SimClock::with_tick(
            ClockPolicy::single_player_simulation(),
            rate,
            Tick(u64::MAX),
        );
        assert_eq!(
            clock.advance(Duration::from_secs(1)),
            Err(TimeError::TickOverflow),
            "a wrapping tick counter must be refused"
        );
        assert_eq!(
            clock.tick().0,
            u64::MAX,
            "a refused advance changes nothing"
        );
        assert_eq!(
            clock.advance_fixed_ticks(1),
            Err(TimeError::TickOverflow),
            "speed-up must not wrap the counter either"
        );
    }

    /// The AC04 minimum scenario at the unit level: while the session clock
    /// is paused, a weapon cooldown and an objective timer advance by exactly
    /// zero ticks, over 30 s of paused wall time delivered as one frame and as
    /// 300 frames alike.
    #[test]
    fn accept_f16_d_pause_advances_no_cooldown_and_no_objective_timer() {
        let rate = TickRate::new(64).expect("64 Hz is valid");
        let mut timeline = GameplayTimeline::new(rate, 120, 600).expect("periods are positive");
        assert_eq!(
            timeline
                .advance_frame(Duration::from_millis(500))
                .expect("runs"),
            32,
            "500 ms at 64 Hz is 32 ticks"
        );
        let before = (
            timeline.tick(),
            timeline.cooldown().remaining_ticks(),
            timeline.cooldown().elapsed_ticks(),
            timeline.objective_timer().remaining_ticks(),
            timeline.objective_timer().elapsed_ticks(),
        );
        assert_eq!(before.1, 120 - 32, "the cooldown counted down 32 ticks");
        assert_eq!(before.3, 600 - 32, "the objective counted down 32 ticks");

        timeline.set_paused(true);
        // 30 s of paused wall time, delivered as one long frame, as 300 short
        // frames, and as a single frame again. All three must commit nothing.
        let thirty_seconds = [
            vec![Duration::from_secs(30)],
            vec![Duration::from_millis(100); 300],
            vec![Duration::from_secs(30)],
        ];
        for (index, frames) in thirty_seconds.iter().enumerate() {
            for span in frames {
                assert_eq!(
                    timeline.advance_frame(*span).expect("runs"),
                    0,
                    "paused delivery {index} commits no ticks"
                );
            }
            assert_eq!(
                (
                    timeline.tick(),
                    timeline.cooldown().remaining_ticks(),
                    timeline.cooldown().elapsed_ticks(),
                    timeline.objective_timer().remaining_ticks(),
                    timeline.objective_timer().elapsed_ticks(),
                ),
                before,
                "paused wall time must not move a cooldown or an objective timer"
            );
        }

        timeline.set_paused(false);
        assert_eq!(
            timeline
                .advance_frame(Duration::from_millis(500))
                .expect("runs"),
            32,
            "resume banks no paused time"
        );
        assert_eq!(timeline.cooldown().remaining_ticks(), 120 - 32 - 32);
        assert_eq!(timeline.objective_timer().remaining_ticks(), 600 - 32 - 32);
    }

    /// A timer has no wall-time entry point, and a countdown saturates: a long
    /// frame cannot produce two expirations for one expiry, and a zero-tick
    /// period is refused rather than accepted as "already expired".
    #[test]
    fn accept_f16_d_tick_timers_saturate_and_refuse_a_zero_period() {
        assert_eq!(
            TickTimer::new(TimerKind::WeaponCooldown, 0),
            Err(TimeError::ZeroTimerPeriod)
        );
        let rate = TickRate::new(64).expect("64 Hz is valid");
        let timer = TickTimer::new(TimerKind::ObjectiveTimer, 10).expect("positive");
        assert_eq!(timer.kind().label(), "objective-timer");
        assert_eq!(
            timer.period_seconds(rate).expect("finite"),
            10.0 / 64.0,
            "the period is authored in ticks and reported in seconds"
        );
        assert!(!timer.is_expired());

        let mut timeline = GameplayTimeline::new(rate, 10, 10).expect("periods are positive");
        assert_eq!(
            timeline
                .advance_frame(Duration::from_secs(10))
                .expect("runs"),
            640
        );
        assert!(timeline.cooldown().is_expired());
        assert_eq!(timeline.cooldown().expirations(), 1);
        assert!(timeline.objective_timer().is_expired());
        assert_eq!(
            timeline.objective_timer().expirations(),
            1,
            "one expiry per timer, however long the frame was"
        );

        timeline.fire();
        assert_eq!(
            timeline.cooldown().remaining_ticks(),
            10,
            "firing re-arms the cooldown to its full period"
        );
        assert_eq!(timeline.cooldown().elapsed_ticks(), 0);
        assert!(
            timeline.objective_timer().is_expired(),
            "firing is not a pause: the objective timer keeps its own state"
        );
    }

    /// A comparison that agrees can claim only what its reference's evidence
    /// supports. A synthetic-fixture reference is `observed_tool` and never
    /// `verified_original`; a diverging trace is `contradicted` even when the
    /// reference would have verified.
    #[test]
    fn accept_f16_d_probe_comparison_claims_only_what_its_evidence_supports() {
        static STEPS: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: NANOS_PER_SECOND as u64,
                frames: 1,
            },
            ProbeStep::Observe("after-one-second"),
        ];
        let probe = BehavioralProbe::new(STEPS);
        let make = || GameplayTimeline::new(TickRate::new(64).expect("valid"), 120, 600);
        let measured = probe.run("measured", 30, make).expect("the probe runs");

        let reference = ProbeReference::new(
            "reference",
            probe
                .run("reference", 30, make)
                .expect("the reference runs"),
            fixture_evidence(),
            0,
        );
        let comparison = ProbeComparison::new(&measured, &reference);
        assert!(comparison.agrees(), "{}", comparison.summary());
        assert_eq!(
            comparison.claim(),
            ClaimStatus::ObservedTool,
            "agreement against a synthetic fixture is not original verification"
        );
        assert!(!comparison.verified_original());

        // A trace that does not match is contradicted, whatever the evidence.
        let mut diverged = probe
            .run("diverged", 30, || {
                GameplayTimeline::new(TickRate::new(64).expect("valid"), 120, 900)
            })
            .expect("the probe runs");
        diverged.samples[0].objective_remaining_ticks = 0;
        let comparison = ProbeComparison::new(&diverged, &reference);
        assert!(!comparison.agrees());
        assert_eq!(comparison.claim(), ClaimStatus::Contradicted);
        assert!(
            comparison
                .divergences()
                .iter()
                .any(|d| d.field == "objective_remaining_ticks"),
            "{}",
            comparison.summary()
        );

        // A trace that observed *less* than the reference diverges too: a
        // shorter trace must never pass as agreement.
        let short = probe.run("short", 30, make).expect("runs").clone();
        let mut short = short;
        short.samples.clear();
        let comparison = ProbeComparison::new(&short, &reference);
        assert!(!comparison.agrees());
        assert_eq!(comparison.divergences().len(), 1);
        assert_eq!(
            comparison.divergences()[0].field,
            "sample-present-in-measurement"
        );
    }

    /// A refused timeline is reported, never traced: the probe propagates the
    /// clock's own error instead of producing a trace a caller could compare
    /// as a shorter run.
    #[test]
    fn accept_f16_d_probe_propagates_a_refused_timeline_instead_of_tracing_it() {
        static STEPS: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: NANOS_PER_SECOND as u64,
                frames: 1,
            },
            ProbeStep::Observe("never-observed"),
        ];
        let probe = BehavioralProbe::new(STEPS);
        let refused = probe.run("refused", 30, || {
            GameplayTimeline::new(TickRate::new(64).expect("valid"), 0, 600)
        });
        assert_eq!(
            refused,
            Err(TimeError::ZeroTimerPeriod),
            "a refused timeline must not produce a trace"
        );
    }

    /// The frame split loses nothing: `nanos` delivered as `frames` frames
    /// always commits the same number of ticks, including at a frame rate that
    /// does not divide a second evenly, where the remainder frame carries the
    /// difference.
    #[test]
    fn accept_f16_d_frame_split_is_exact_at_every_delivery_rate() {
        static ONE_SECOND: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: NANOS_PER_SECOND as u64,
                frames: 1,
            },
            ProbeStep::Observe("after-one-second"),
        ];
        let probe = BehavioralProbe::new(ONE_SECOND);
        for frames in [0_u32, 1, 7, 30, 60, 144, 1000] {
            let trace = probe
                .run("split", frames.max(1), || {
                    GameplayTimeline::new(TickRate::new(64).expect("valid"), 120, 600)
                })
                .expect("the probe runs");
            assert_eq!(
                trace.committed_ticks(),
                64,
                "one second at 64 Hz is 64 ticks, delivered as {frames} frame(s)"
            );
            assert_eq!(
                trace.sample("after-one-second").expect("observed").tick(),
                Tick(64)
            );
        }
    }

    /// Evidence that can never verify the original, whatever the comparison
    /// says: a synthetic fixture, an artifact fingerprint, a missing locator
    /// and a non-observing method are each individually disqualifying.
    fn fixture_evidence() -> EvidenceRecord {
        EvidenceRecord {
            source: cs_types::evidence::EvidenceSource::SyntheticFixture,
            fingerprint: None,
            locator: None,
            method: cs_types::evidence::ObservationMethod::Authored,
            limitations: vec!["newly authored fixture; no original run was observed".to_string()],
        }
    }
}
