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
//!
//! # F16-F: the original clock policy, declared from static code analysis
//!
//! F16-D had no original to compare against. F16-F declares that policy in
//! code: [`OriginalClockPolicy`] is the original's clock, pause and speed-up
//! behaviour as static analysis of the owner-supplied decrypted executable
//! measured it (owner notes on task #391) — one variable `game_dt` per
//! rendered frame capped at 125 ms, the flight world frozen while the frame
//! clock and the accumulators keep running, **no** banking of paused time,
//! one dt source feeding separate accumulators for the weapon cooldown and
//! the objective timer, and a 2× single-player speed-up that is capped and
//! gated off in network games.
//!
//! [`OriginalClockPolicy::compare_project_clocks`] compares that declaration
//! with this module's [`ClockPolicy`]s under a [`PolicyTolerance`] chosen
//! *before* the comparison. Every finding is one of three relations:
//! [`PolicyRelation::Agrees`], [`PolicyRelation::Diverges`] (recorded in
//! `docs/findings/` and filed, never silently "fixed" by changing a policy
//! pairing) and [`PolicyRelation::NotModeled`] (a subsystem these clock
//! policies do not cover). The comparison's claim is the policy's own
//! [`claim_status`](OriginalClockPolicy::claim_status), capped at
//! [`ORIGINAL_CLOCK_POLICY_STATUS`] (`inferred`): static code evidence never
//! yields [`ClaimStatus::VerifiedOriginal`].

use std::time::Duration;

use cs_types::Tick;
use cs_types::evidence::{
    ClaimStatus, ContentHash, EvidenceRecord, EvidenceSource, Fingerprint, FingerprintKind,
    ObservationLocator, ObservationMethod,
};

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
    /// (`F16` non-negotiable behavior 4), so this always refuses: the
    /// authority check precedes the pause check, so a paused clock reports
    /// [`TimeError::NoSpeedUpAuthority`] and never
    /// [`TimeError::ClockPaused`]. It exists so that the refusal is a named,
    /// tested boundary rather than a missing method — extra fixed ticks reach
    /// these timers only as ticks a frame's clock committed.
    ///
    /// # Errors
    ///
    /// [`TimeError::NoSpeedUpAuthority`], always. Nothing is mutated.
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
/// A probe is a *script*, not a test: the same script is run at several render
/// frame rates and produces a trace for each, and the traces are what get
/// compared. That is what makes "does pause freeze the game" a measurement
/// (the trace shows zero advancement at every delivery rate) rather than an
/// assertion someone wrote twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeStep {
    /// Deliver `nanos` of wall time as whole render frames at the probe's
    /// frame rate, plus a final frame carrying the remainder.
    Advance {
        /// Wall time to deliver, in nanoseconds.
        nanos: u64,
    },
    /// Deliver `nanos` of wall time as a **single** render frame — the
    /// pathological stall a real run can hit after a blocking load, and the
    /// sharpest test that a paused frame commits nothing however long it is.
    OneFrame {
        /// Wall time to deliver, in nanoseconds.
        nanos: u64,
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

/// Why a behavioral probe was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// The probe's render frame rate is zero, or so fast that one frame is
    /// shorter than a nanosecond, so no wall time could be delivered in whole
    /// frames.
    UnusableRenderRate {
        /// The refused frame rate.
        render_fps: u32,
    },
    /// A step asks for more render frames than
    /// [`MAX_FRAMES_PER_STEP`], so the run would be unbounded work.
    FrameCountTooLarge {
        /// The wall time the step asked for, in nanoseconds.
        nanos: u64,
        /// How many frames that would take.
        frames: u64,
    },
    /// Two steps observe the same label, which would make a reference trace
    /// ambiguous.
    DuplicateObservationLabel {
        /// The label that appears twice.
        label: &'static str,
    },
    /// The script observes nothing, so it measures nothing.
    NoObservations,
    /// The timeline the probe was handed refused to be built.
    Time(TimeError),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnusableRenderRate { render_fps } => write!(
                f,
                "render rate {render_fps} cannot deliver wall time in whole nanosecond frames"
            ),
            Self::FrameCountTooLarge { nanos, frames } => write!(
                f,
                "{nanos} ns would need {frames} render frames, over the {} frame limit",
                MAX_FRAMES_PER_STEP
            ),
            Self::DuplicateObservationLabel { label } => {
                write!(f, "observation label {label:?} is used twice")
            }
            Self::NoObservations => write!(f, "a probe must observe at least one sample"),
            Self::Time(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for ProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Time(source) => Some(source),
            _ => None,
        }
    }
}

impl From<TimeError> for ProbeError {
    fn from(source: TimeError) -> Self {
        Self::Time(source)
    }
}

/// The most render frames one probe step may ask for.
///
/// A step is wall time, and wall time divided by a frame length is
/// unbounded: `u64::MAX` nanoseconds at 144 frames per second is about 10¹²
/// frames. The bound keeps a probe from turning a typo into an out-of-memory
/// or a hang, and the refusal names both numbers so the caller can see which
/// step was too large.
pub const MAX_FRAMES_PER_STEP: u64 = 1_000_000;

/// Runs a scripted [`ProbeStep`] list against a real [`GameplayTimeline`] and
/// returns what it measured.
///
/// The render frame rate belongs to the probe, not to the call site: the same
/// script is run at several rates, and the traces must agree. A script whose
/// traces only agree with itself at one rate has measured the render loop, not
/// the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehavioralProbe {
    name: &'static str,
    render_fps: u32,
    steps: &'static [ProbeStep],
}

impl BehavioralProbe {
    /// A probe named `name` that delivers its wall time at `render_fps`.
    ///
    /// # Errors
    ///
    /// [`ProbeError::UnusableRenderRate`] for a zero or sub-nanosecond frame
    /// rate, [`ProbeError::NoObservations`] when the script observes nothing
    /// and [`ProbeError::DuplicateObservationLabel`] when two steps share a
    /// label.
    pub fn new(
        name: &'static str,
        render_fps: u32,
        steps: &'static [ProbeStep],
    ) -> Result<Self, ProbeError> {
        if frame_nanos(render_fps)? == 0 {
            return Err(ProbeError::UnusableRenderRate { render_fps });
        }
        let mut labels: Vec<&'static str> = Vec::new();
        for step in steps {
            if let ProbeStep::Observe(label) = step {
                if labels.contains(label) {
                    return Err(ProbeError::DuplicateObservationLabel { label });
                }
                labels.push(label);
            }
        }
        if labels.is_empty() {
            return Err(ProbeError::NoObservations);
        }
        Ok(Self {
            name,
            render_fps,
            steps,
        })
    }

    /// The probe's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The render frame rate this probe delivers its wall time at.
    #[must_use]
    pub const fn render_fps(&self) -> u32 {
        self.render_fps
    }

    /// The steps this probe runs.
    #[must_use]
    pub const fn steps(&self) -> &'static [ProbeStep] {
        self.steps
    }

    /// Runs the script against a fresh timeline built by `make` and returns
    /// the trace.
    ///
    /// `make` is supplied by the caller so the timeline's rate, cooldown and
    /// objective period are the caller's scenario, not a hidden constant of
    /// this module.
    ///
    /// # Errors
    ///
    /// [`ProbeError::Time`] when the timeline refuses to be built or a step
    /// would wrap the tick counter — a refused step produces no trace, so a
    /// truncated run can never be compared as a real one — and
    /// [`ProbeError::FrameCountTooLarge`] when a step would ask for more
    /// frames than [`MAX_FRAMES_PER_STEP`].
    pub fn run<MakeTimeline>(&self, mut make: MakeTimeline) -> Result<ProbeTrace, ProbeError>
    where
        MakeTimeline: FnMut() -> Result<GameplayTimeline, TimeError>,
    {
        let frame = frame_nanos(self.render_fps)?;
        let mut timeline = make()?;
        let mut samples = Vec::new();
        let mut committed = 0u64;
        for step in self.steps {
            match *step {
                ProbeStep::Advance { nanos } => {
                    for span in frame_spans(nanos, frame)? {
                        committed += timeline.advance_frame(span)?;
                    }
                }
                ProbeStep::OneFrame { nanos } => {
                    committed += timeline.advance_frame(Duration::from_nanos(nanos))?;
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
            name: self.name,
            render_fps: self.render_fps,
            committed_ticks: committed,
            samples,
        })
    }
}

/// One render frame at `render_fps`, in whole nanoseconds.
///
/// # Errors
///
/// [`ProbeError::UnusableRenderRate`] when `render_fps` is zero or a frame
/// would be shorter than one nanosecond.
fn frame_nanos(render_fps: u32) -> Result<u64, ProbeError> {
    if render_fps == 0 {
        return Err(ProbeError::UnusableRenderRate { render_fps });
    }
    let rate = u128::from(render_fps);
    let frame = NANOS_PER_SECOND / rate;
    u64::try_from(frame).map_err(|_| ProbeError::UnusableRenderRate { render_fps })
}

/// Splits `nanos` of wall time into whole render frames of `frame` plus a
/// final frame carrying the remainder, so the frames sum to exactly `nanos`
/// and no nanosecond is dropped or invented at a frame boundary.
///
/// # Errors
///
/// [`ProbeError::FrameCountTooLarge`] when the split would exceed
/// [`MAX_FRAMES_PER_STEP`].
fn frame_spans(nanos: u64, frame: u64) -> Result<Vec<Duration>, ProbeError> {
    if frame == 0 {
        return Ok(vec![Duration::from_nanos(nanos)]);
    }
    let whole = nanos / frame;
    let rest = nanos - whole * frame;
    // The frame count is exact, and the bound is applied to it rather than to
    // `whole`: `MAX_FRAMES_PER_STEP` whole frames *plus* a remainder frame is
    // one frame over the limit, and the refusal names the real number even
    // when the wall time is an exact multiple of the frame length and no
    // remainder frame is emitted at all.
    let frames = whole + u64::from(rest > 0);
    if frames > MAX_FRAMES_PER_STEP {
        return Err(ProbeError::FrameCountTooLarge { nanos, frames });
    }
    let mut spans = Vec::with_capacity(frames as usize);
    spans.resize(whole as usize, Duration::from_nanos(frame));
    if rest > 0 {
        spans.push(Duration::from_nanos(rest));
    }
    Ok(spans)
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

// ---------------------------------------------------------------------------
// F16-F: the original clock policy, declared from static code analysis
// ---------------------------------------------------------------------------

/// The findings entry that records this policy: addresses, image sha256 and
/// the four measured answers (`docs/findings/`).
pub const ORIGINAL_POLICY_FINDINGS: &str =
    "docs/findings/2026-10-06-f16-f-original-clock-pause-and-speed-up-policy.md";

/// sha256 of the owner-supplied decrypted image the policy was read from
/// (`$CS_GAME_DIR/crimson.decrypted.exe`, a decryption of `crimson.icd`).
///
/// A hash of decrypted bytes, never the bytes: nothing executable, no
/// decompiled code and no game data is committed anywhere in this tree.
pub const ORIGINAL_IMAGE_SHA256: &str =
    "43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75";

/// How well [`OriginalClockPolicy`] is known.
///
/// `inferred`, never `verified_original`: the policy is derived from static
/// analysis of the owner-supplied decrypted executable (owner notes on task
/// #391, recorded in [`ORIGINAL_POLICY_FINDINGS`]), not from a run of the
/// original engine. A task may not raise this constant itself; only
/// owner-supplied original-run evidence can, and
/// [`OriginalClockPolicy::claim_status`] caps a declared policy there
/// structurally rather than trusting the record it carries.
pub const ORIGINAL_CLOCK_POLICY_STATUS: ClaimStatus = ClaimStatus::Inferred;

/// The tolerance F16-F declared **before** comparing anything
/// (`FLIGHT-PHYSICS`, "Calibration acceptance": select tolerances before
/// fitting, never after).
///
/// Zero slack: the project's fixed dt must land inside the original's
/// measured per-frame window exactly, and a declared fixed debug dt must
/// equal the project's. A caller that wants slack states it up front — the
/// comparison reads the tolerance it is handed and never widens it.
pub const ORIGINAL_POLICY_TOLERANCE: PolicyTolerance = PolicyTolerance { dt_slack_nanos: 0 };

/// Where the original's per-frame `game_dt` comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OriginalDtSource {
    /// One **variable** step per rendered frame: `(GetTickCount() − last) ×
    /// 0.001` s, scaled once and then clamped. The resolution is the OS tick,
    /// not a fixed rate. This is the shipping policy: clamping is on by
    /// default and the command line is not given.
    VariableFrameDelta,
    /// The debug `-freq F` option, which sets both the clamp bounds to `1/F`
    /// and so forces a fixed dt. Declared because it exists, not because the
    /// shipping game runs it.
    FixedDebugFrequency {
        /// The forced frame period, `1/F`.
        period: Duration,
    },
}

impl OriginalDtSource {
    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::VariableFrameDelta => "variable-frame-dt",
            Self::FixedDebugFrequency { .. } => "fixed-debug-frequency",
        }
    }
}

/// One subsystem of the original program, as the owner notes name them.
///
/// Addresses are virtual addresses in the analysed image and appear here only
/// as documentation; no executable byte, disassembly listing or decompiled
/// code is committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OriginalSubsystem {
    /// World/node action-callback update: planes, AI, turrets, effects and
    /// animations.
    WorldSimulation,
    /// The player plane callback's vehicle clock, which the weapon, AI and
    /// turret intervals count against.
    VehicleClock,
    /// The mission update: elapsed time, dormant/nap timers and the countdown
    /// object's expiry and HUD display.
    MissionObjectives,
    /// Camera shake, HUD and input.
    CameraHudInput,
    /// Playing sounds, snapshot-paused when the escape screen is pushed.
    SoundPlayback,
    /// The frame clock and the game/real accumulators.
    FrameClock,
    /// The sound system's per-frame update.
    SoundSystemUpdate,
    /// Network code, which reads the OS tick count directly.
    Network,
    /// Force-feedback and warning effect expiries, which compare against the
    /// total game-time accumulator and therefore stop across a pause.
    ForceFeedbackEffects,
    /// The mission countdown's millisecond copy, a wall-time counter.
    CountdownWallClock,
}

impl OriginalSubsystem {
    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WorldSimulation => "world-simulation",
            Self::VehicleClock => "vehicle-clock",
            Self::MissionObjectives => "mission-objectives",
            Self::CameraHudInput => "camera-hud-input",
            Self::SoundPlayback => "sound-playback",
            Self::FrameClock => "frame-clock",
            Self::SoundSystemUpdate => "sound-system-update",
            Self::Network => "network",
            Self::ForceFeedbackEffects => "force-feedback-effects",
            Self::CountdownWallClock => "countdown-wall-clock",
        }
    }
}

/// The subsystems pause freezes, per the static analysis.
const FROZEN_WHILE_PAUSED: &[OriginalSubsystem] = &[
    OriginalSubsystem::WorldSimulation,
    OriginalSubsystem::VehicleClock,
    OriginalSubsystem::MissionObjectives,
    OriginalSubsystem::CameraHudInput,
    OriginalSubsystem::SoundPlayback,
];

/// The subsystems that keep running while paused, per the static analysis.
const KEEPS_RUNNING_WHILE_PAUSED: &[OriginalSubsystem] = &[
    OriginalSubsystem::FrameClock,
    OriginalSubsystem::SoundSystemUpdate,
    OriginalSubsystem::Network,
    OriginalSubsystem::ForceFeedbackEffects,
    OriginalSubsystem::CountdownWallClock,
];

/// The original's single-player speed-up, as measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OriginalSpeedUp {
    /// The multiplier: while the speed-up flag is set, each frame scales real
    /// dt by this factor.
    factor: f64,
    /// Whether the frame-dt clamp still applies while sped up, so the
    /// speed-up saturates at low frame rates instead of running away.
    capped_by_max_frame_dt: bool,
    /// Whether a non-zero network setting refuses the speed-up entirely.
    network_gated: bool,
    /// Whether the command has a default binding a player can reach.
    default_binding: bool,
}

impl OriginalSpeedUp {
    /// The dt multiplier (2×).
    #[must_use]
    pub const fn factor(self) -> f64 {
        self.factor
    }

    /// Whether the 125 ms frame-dt cap still applies while sped up.
    #[must_use]
    pub const fn capped_by_max_frame_dt(self) -> bool {
        self.capped_by_max_frame_dt
    }

    /// Whether multiplayer refuses the speed-up (the network gate).
    #[must_use]
    pub const fn network_gated(self) -> bool {
        self.network_gated
    }

    /// Whether a player can reach the speed-up without an external binding.
    #[must_use]
    pub const fn default_binding(self) -> bool {
        self.default_binding
    }
}

/// The original's clock, pause and speed-up policy, **declared in code** from
/// static analysis of the owner-supplied decrypted executable (owner notes on
/// task #391, recorded in [`ORIGINAL_POLICY_FINDINGS`]).
///
/// This is a declaration, not a measurement of a run: the values below are
/// what the code does, read from the code. What only a run could give — frame
/// pacing, the distribution of frame deltas, timing uncertainty — stays
/// explicitly unmeasured and is listed in the findings entry.
///
/// The claim it can carry is [`ORIGINAL_CLOCK_POLICY_STATUS`] (`inferred`);
/// see [`claim_status`](Self::claim_status).
#[derive(Clone, Debug)]
pub struct OriginalClockPolicy {
    name: &'static str,
    evidence: EvidenceRecord,
    dt_source: OriginalDtSource,
    max_frame_dt: Duration,
    min_frame_dt: Duration,
    frame_dt_clamped: bool,
    banks_paused_time: bool,
    frozen_subsystems: &'static [OriginalSubsystem],
    keeps_running_subsystems: &'static [OriginalSubsystem],
    gameplay_timers_share_dt_source: bool,
    gameplay_timers_have_separate_accumulators: bool,
    speed_up: OriginalSpeedUp,
}

impl OriginalClockPolicy {
    /// The policy as static analysis of the 2000 PC original measured it:
    /// variable `game_dt` capped at 125 ms, the flight world frozen while the
    /// frame clock keeps ticking, no banking, one dt source with separate
    /// accumulators for cooldown and objective timers, and a capped 2×
    /// single-player speed-up that is gated off in network games and has no
    /// default binding.
    #[must_use]
    pub fn measured_original() -> Self {
        Self {
            name: "crimson-skies-2000-original.static-code-analysis",
            evidence: code_derived_evidence(),
            dt_source: OriginalDtSource::VariableFrameDelta,
            max_frame_dt: Duration::from_millis(125),
            min_frame_dt: Duration::ZERO,
            frame_dt_clamped: true,
            banks_paused_time: false,
            frozen_subsystems: FROZEN_WHILE_PAUSED,
            keeps_running_subsystems: KEEPS_RUNNING_WHILE_PAUSED,
            gameplay_timers_share_dt_source: true,
            gameplay_timers_have_separate_accumulators: true,
            speed_up: OriginalSpeedUp {
                factor: 2.0,
                capped_by_max_frame_dt: true,
                network_gated: true,
                default_binding: false,
            },
        }
    }

    /// The same policy with a different dt source, for comparing the debug
    /// `-freq` variant against a fixed-rate project clock.
    #[must_use]
    pub const fn with_dt_source(mut self, dt_source: OriginalDtSource) -> Self {
        self.dt_source = dt_source;
        self
    }

    /// The same policy carrying different evidence.
    ///
    /// The claim still cannot rise above [`ORIGINAL_CLOCK_POLICY_STATUS`]:
    /// [`claim_status`](Self::claim_status) caps it there, which is exactly
    /// what the acceptance test pins.
    #[must_use]
    pub fn with_evidence(mut self, evidence: EvidenceRecord) -> Self {
        self.evidence = evidence;
        self
    }

    /// The declaration's stable name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The evidence behind the declaration: code-derived, never runtime.
    #[must_use]
    pub const fn evidence(&self) -> &EvidenceRecord {
        &self.evidence
    }

    /// Where the original's per-frame `game_dt` comes from.
    #[must_use]
    pub const fn dt_source(&self) -> OriginalDtSource {
        self.dt_source
    }

    /// The clamp upper bound: one rendered frame may never advance more than
    /// this much game time (125 ms, 1/8 s).
    #[must_use]
    pub const fn max_frame_dt(&self) -> Duration {
        self.max_frame_dt
    }

    /// The clamp lower bound (zero: a frame may be free, never negative).
    #[must_use]
    pub const fn min_frame_dt(&self) -> Duration {
        self.min_frame_dt
    }

    /// Whether the clamp is enabled by default (it is).
    #[must_use]
    pub const fn frame_dt_clamped(&self) -> bool {
        self.frame_dt_clamped
    }

    /// Whether paused time is banked (it is not: the frame clock keeps
    /// ticking during a pause and the first resumed frame carries one normal
    /// frame delta).
    #[must_use]
    pub const fn banks_paused_time(&self) -> bool {
        self.banks_paused_time
    }

    /// The subsystems pause freezes.
    #[must_use]
    pub const fn frozen_subsystems(&self) -> &'static [OriginalSubsystem] {
        self.frozen_subsystems
    }

    /// The subsystems that keep running while paused.
    #[must_use]
    pub const fn keeps_running_subsystems(&self) -> &'static [OriginalSubsystem] {
        self.keeps_running_subsystems
    }

    /// Whether the declared policy says how `subsystem` behaves while paused.
    ///
    /// `None` means this declaration does not cover that subsystem — a gap to
    /// record, never a default to assume.
    #[must_use]
    pub fn pause_policy_for(&self, subsystem: OriginalSubsystem) -> Option<PausePolicy> {
        if self.frozen_subsystems.contains(&subsystem) {
            Some(PausePolicy::Freeze)
        } else if self.keeps_running_subsystems.contains(&subsystem) {
            Some(PausePolicy::KeepRunning)
        } else {
            None
        }
    }

    /// Whether the weapon cooldown and the objective timer read one shared dt
    /// source.
    #[must_use]
    pub const fn gameplay_timers_share_dt_source(&self) -> bool {
        self.gameplay_timers_share_dt_source
    }

    /// Whether those two timers are nevertheless separate accumulators,
    /// advanced at different points of the frame.
    #[must_use]
    pub const fn gameplay_timers_have_separate_accumulators(&self) -> bool {
        self.gameplay_timers_have_separate_accumulators
    }

    /// The original's speed-up policy.
    #[must_use]
    pub const fn speed_up(&self) -> OriginalSpeedUp {
        self.speed_up
    }

    /// The claim this declaration's evidence supports.
    ///
    /// `verified_original` is **unreachable here by construction**: a policy
    /// declared in source is compiled from static code analysis, never from
    /// an observation of the running original program, so a record that would
    /// [`verify`](EvidenceRecord::verifies_original) the original is still
    /// reported as [`ORIGINAL_CLOCK_POLICY_STATUS`] (`inferred`). Synthetic
    /// fixture evidence is `unknown`, authored evidence `designed`, a cited
    /// document `documented`.
    #[must_use]
    pub fn claim_status(&self) -> ClaimStatus {
        match claim_status_for(&self.evidence) {
            ClaimStatus::VerifiedOriginal => ORIGINAL_CLOCK_POLICY_STATUS,
            status => status,
        }
    }

    /// Compares this declared original policy with the project's clock
    /// policies at `rate`, under `tolerance`.
    ///
    /// The tolerance is an **input chosen before the comparison**, never
    /// fitted afterwards: see [`ORIGINAL_POLICY_TOLERANCE`].
    #[must_use]
    pub fn compare_project_clocks(
        &self,
        rate: TickRate,
        tolerance: PolicyTolerance,
    ) -> OriginalPolicyComparison {
        let gameplay: [(&'static str, ClockPolicy); 3] = [
            (
                "single-player simulation",
                ClockPolicy::single_player_simulation(),
            ),
            (
                "multiplayer simulation",
                ClockPolicy::multiplayer_simulation(),
            ),
            (
                "authoritative gameplay",
                ClockPolicy::authoritative_gameplay(),
            ),
        ];
        let presentation: [(&'static str, ClockPolicy); 2] = [
            ("ui wall", ClockPolicy::ui_wall()),
            ("media unscaled", ClockPolicy::media_unscaled()),
        ];
        let gameplay_freezes = gameplay
            .iter()
            .all(|(_, policy)| policy.pause() == PausePolicy::Freeze);
        let presentation_keeps_running = presentation
            .iter()
            .all(|(_, policy)| policy.pause() == PausePolicy::KeepRunning);
        // The declaration side of the same questions: the comparison reads
        // what the measured policy says, not what the project already does.
        let original_freezes_flight_world = [
            OriginalSubsystem::WorldSimulation,
            OriginalSubsystem::VehicleClock,
            OriginalSubsystem::MissionObjectives,
            OriginalSubsystem::CameraHudInput,
        ]
        .into_iter()
        .all(|subsystem| self.pause_policy_for(subsystem) == Some(PausePolicy::Freeze));
        let original_keeps_frame_clock_running =
            self.pause_policy_for(OriginalSubsystem::FrameClock) == Some(PausePolicy::KeepRunning);
        let single_player_speeds_up =
            ClockPolicy::single_player_simulation().speed_up() == SpeedUpPolicy::AdvanceFixedTicks;
        let multiplayer_refuses_speed_up =
            ClockPolicy::multiplayer_simulation().speed_up() == SpeedUpPolicy::NoLocalAuthority;
        let gameplay_has_no_local_authority =
            ClockPolicy::authoritative_gameplay().speed_up() == SpeedUpPolicy::NoLocalAuthority;

        let dt_nanos = NANOS_PER_SECOND / u128::from(rate.ticks_per_second());
        let slack = u128::from(tolerance.dt_slack_nanos);
        let dt_inside_original_window = dt_nanos + slack >= self.min_frame_dt.as_nanos()
            && dt_nanos <= self.max_frame_dt.as_nanos() + slack;

        let tick_source_relation = match self.dt_source {
            OriginalDtSource::VariableFrameDelta => PolicyRelation::Diverges,
            OriginalDtSource::FixedDebugFrequency { period } => {
                if period.as_nanos().abs_diff(dt_nanos) <= slack {
                    PolicyRelation::Agrees
                } else {
                    PolicyRelation::Diverges
                }
            }
        };

        let findings = vec![
            PolicyFinding {
                field: "pause.gameplay",
                original: format!(
                    "the flight world freezes: {}, because only the top screen \
                     state is updated per frame",
                    list_subsystems(self.frozen_subsystems),
                ),
                project: pause_summary(&gameplay),
                relation: if original_freezes_flight_world && gameplay_freezes {
                    PolicyRelation::Agrees
                } else {
                    PolicyRelation::Diverges
                },
            },
            PolicyFinding {
                field: "pause.presentation",
                original: format!(
                    "keeps running while paused: {}",
                    list_subsystems(self.keeps_running_subsystems),
                ),
                project: pause_summary(&presentation),
                relation: if original_keeps_frame_clock_running && presentation_keeps_running {
                    PolicyRelation::Agrees
                } else {
                    PolicyRelation::Diverges
                },
            },
            PolicyFinding {
                field: "pause.sound-playback",
                original:
                    "playing sounds are snapshot-paused when the escape screen is pushed and \
                     resumed when it is popped"
                        .to_string(),
                project: "cs_sim::time declares no audio policy; audio pause belongs to the audio \
                     subsystem (F41/F46), not to a clock"
                    .to_string(),
                relation: PolicyRelation::NotModeled,
            },
            PolicyFinding {
                field: "pause.network",
                original: "network code keeps running and reads the OS tick count directly"
                    .to_string(),
                project: "cs_sim::time declares no network policy; session timing belongs to \
                          the networking crate"
                    .to_string(),
                relation: PolicyRelation::NotModeled,
            },
            PolicyFinding {
                field: "pause.bank",
                original: format!(
                    "paused time is not banked: the frame clock keeps ticking and the first \
                     resumed frame carries one normal frame delta (at most {} ns)",
                    self.max_frame_dt.as_nanos(),
                ),
                project: "PausePolicy::Freeze clocks drop paused wall time: SimClock::advance \
                          returns zero and leaves the carry untouched, so resume never releases \
                          a burst"
                    .to_string(),
                relation: if !self.banks_paused_time && gameplay_freezes {
                    PolicyRelation::Agrees
                } else {
                    PolicyRelation::Diverges
                },
            },
            PolicyFinding {
                field: "clock.tick-source",
                original: match self.dt_source {
                    OriginalDtSource::VariableFrameDelta => format!(
                        "{}: one variable game_dt per rendered frame, from the OS tick count",
                        self.dt_source.label(),
                    ),
                    OriginalDtSource::FixedDebugFrequency { period } => format!(
                        "{}: game_dt pinned to {} ns by the debug -freq option",
                        self.dt_source.label(),
                        period.as_nanos(),
                    ),
                },
                project: format!(
                    "fixed dt of {} ns (integer accumulator at {} Hz)",
                    dt_nanos,
                    rate.ticks_per_second(),
                ),
                relation: tick_source_relation,
            },
            PolicyFinding {
                field: "clock.frame-dt-bound",
                original: format!(
                    "game_dt is clamped to [{}, {}] ns per frame (clamp on by default)",
                    self.min_frame_dt.as_nanos(),
                    self.max_frame_dt.as_nanos(),
                ),
                project: format!(
                    "one tick advances {dt_nanos} ns; the declared tolerance admits {} ns of \
                     slack",
                    tolerance.dt_slack_nanos,
                ),
                relation: if dt_inside_original_window {
                    PolicyRelation::Agrees
                } else {
                    PolicyRelation::Diverges
                },
            },
            PolicyFinding {
                field: "clock.frame-dt-cap",
                original: if self.frame_dt_clamped {
                    format!(
                        "a stalled frame still advances at most {} ns of game time because the \
                         clamp is applied after scaling",
                        self.max_frame_dt.as_nanos(),
                    )
                } else {
                    "the frame clamp is off, so a stalled frame advances its whole delta"
                        .to_string()
                },
                project:
                    "SimClock::advance accepts any Duration and commits all of its whole ticks; \
                     the fixed-step accumulator declares no per-frame cap"
                        .to_string(),
                relation: if self.frame_dt_clamped {
                    PolicyRelation::Diverges
                } else {
                    PolicyRelation::Agrees
                },
            },
            PolicyFinding {
                field: "timers.shared-dt-source",
                original: "one dt source for both, with separate accumulators: the weapon, AI and \
                     turret timers count the vehicle clock while the objective timers advance \
                     inside the mission update, at a different point of the frame"
                    .to_string(),
                project:
                    "one SimClock commits whole ticks and GameplayTimeline feeds both TickTimers \
                     from the same commit in advance_frame, each timer keeping its own \
                     remaining count"
                        .to_string(),
                relation: if self.gameplay_timers_share_dt_source
                    && self.gameplay_timers_have_separate_accumulators
                {
                    PolicyRelation::Agrees
                } else {
                    PolicyRelation::Diverges
                },
            },
            PolicyFinding {
                field: "timers.player-down-gate",
                original:
                    "the mission update is skipped while the player flag is set, so objectives \
                     freeze while the vehicle clock keeps advancing"
                        .to_string(),
                project: "GameplayTimeline has no player-down gate: every committed tick advances \
                     both timers"
                    .to_string(),
                relation: PolicyRelation::NotModeled,
            },
            PolicyFinding {
                field: "speed-up.network-gate",
                original: format!(
                    "{}× dt while the network setting is zero, refused when it is not; the \
                     command has no default binding",
                    self.speed_up.factor,
                ),
                project: format!(
                    "single-player simulation {}, multiplayer simulation {}",
                    speed_up_label(ClockPolicy::single_player_simulation().speed_up()),
                    speed_up_label(ClockPolicy::multiplayer_simulation().speed_up()),
                ),
                relation: if self.speed_up.network_gated
                    && single_player_speeds_up
                    && multiplayer_refuses_speed_up
                {
                    PolicyRelation::Agrees
                } else {
                    PolicyRelation::Diverges
                },
            },
            PolicyFinding {
                field: "speed-up.mechanism",
                original: format!(
                    "a one-frame {}× multiplier on the variable dt, still clamped to {} ns",
                    self.speed_up.factor,
                    self.max_frame_dt.as_nanos(),
                ),
                project: "whole fixed ticks (F16 non-negotiable 4: speed-up advances fixed ticks, \
                     never a variable dt)"
                    .to_string(),
                relation: PolicyRelation::Diverges,
            },
            PolicyFinding {
                field: "speed-up.reaches-gameplay-timers",
                original:
                    "everything that reads dt scales together, so the speed-up accelerates the \
                     vehicle clock, the weapon cooldowns and the objective timers alike"
                        .to_string(),
                project: format!(
                    "authoritative gameplay {} and GameplayTimeline::advance_fixed_ticks \
                     always refuses",
                    speed_up_label(ClockPolicy::authoritative_gameplay().speed_up()),
                ),
                relation: if gameplay_has_no_local_authority {
                    PolicyRelation::Diverges
                } else {
                    PolicyRelation::Agrees
                },
            },
            PolicyFinding {
                field: "speed-up.default-binding",
                original:
                    "the speed-up command has no default binding and is absent from the controls \
                     screen, so a player cannot reach it without an external binding"
                        .to_string(),
                project:
                    "cs_sim::time exposes the clock API only; the input layer binds no speed-up \
                     command yet"
                        .to_string(),
                relation: PolicyRelation::NotModeled,
            },
        ];

        OriginalPolicyComparison {
            policy: self.name,
            project_rate: rate,
            tolerance,
            findings,
            claim: self.claim_status(),
            verified_original: false,
        }
    }
}

/// The evidence behind [`OriginalClockPolicy::measured_original`]: a cited
/// document over the original image, with a **non-runtime** method.
///
/// `ObservationMethod::Inference` is deliberate. The policy was read out of
/// the executable's code, not watched in a running original program, so
/// `RuntimeObservation` would be fabricated evidence and
/// [`EvidenceRecord::verifies_original`] must stay false. The image sha256 is
/// recorded so the analysed bytes are named, while the locator points at the
/// findings entry a reviewer can actually read.
fn code_derived_evidence() -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::Document(ORIGINAL_POLICY_FINDINGS.to_string()),
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256: ContentHash::from_hex(ORIGINAL_IMAGE_SHA256)
                .expect("ORIGINAL_IMAGE_SHA256 is 64 lowercase hex characters"),
        }),
        locator: Some(ObservationLocator {
            container: ORIGINAL_POLICY_FINDINGS.to_string(),
            span: None,
        }),
        method: ObservationMethod::Inference,
        limitations: vec![
            "static code analysis of the owner-supplied decrypted executable (Kuna decompiler \
             v1.692, checked against the disassembly); never a run of the original program, so \
             it is not a runtime observation"
                .to_string(),
            "frame pacing, timing uncertainty and the real distribution of frame deltas are \
             unmeasured: only an original run could give them"
                .to_string(),
            "addresses are virtual addresses in the analysed image; no executable byte, \
             disassembly listing or decompiled code is committed"
                .to_string(),
        ],
    }
}

/// The strongest claim one [`EvidenceRecord`] supports, for a record that
/// does not verify the original.
///
/// Synthetic fixtures claim `unknown`, a cited document `documented`, static
/// reasoning `inferred`, authored work `designed`, and a tool run
/// `observed_tool`. `verified_original` is decided first, by
/// [`EvidenceRecord::verifies_original`] itself, and is then capped away for a
/// declared policy by [`OriginalClockPolicy::claim_status`].
fn claim_status_for(evidence: &EvidenceRecord) -> ClaimStatus {
    if matches!(evidence.source, EvidenceSource::SyntheticFixture) {
        return ClaimStatus::Unknown;
    }
    if evidence.verifies_original() {
        return ClaimStatus::VerifiedOriginal;
    }
    match evidence.method {
        ObservationMethod::Authored => ClaimStatus::Designed,
        ObservationMethod::Inference => ClaimStatus::Inferred,
        ObservationMethod::DocumentReview => ClaimStatus::Documented,
        ObservationMethod::ToolProbe => ClaimStatus::ObservedTool,
        // A direct method that still failed to verify the original observed
        // nothing *original*: no original fingerprint or no locator.
        ObservationMethod::ByteInspection | ObservationMethod::RuntimeObservation => {
            ClaimStatus::Unknown
        }
    }
}

/// What a policy comparison may accept as "the same" — declared before the
/// comparison runs, never fitted to its result (`FLIGHT-PHYSICS`,
/// "Calibration acceptance").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PolicyTolerance {
    /// Nanoseconds of slack allowed when a project dt is checked against the
    /// original's measured per-frame window, and when a declared fixed debug
    /// dt is compared with the project's fixed dt.
    pub dt_slack_nanos: u64,
}

/// How one fact relates the measured original policy to the project's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PolicyRelation {
    /// The original and the project agree on this fact.
    Agrees,
    /// They differ. Every divergence is recorded in `docs/findings/` and
    /// filed as a task or an owner decision; a policy pairing is never
    /// changed silently to make it go away.
    Diverges,
    /// The project's clock policies do not cover this subsystem at all, so
    /// the comparison claims neither agreement nor divergence.
    NotModeled,
}

impl PolicyRelation {
    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Agrees => "agrees",
            Self::Diverges => "diverges",
            Self::NotModeled => "not-modeled",
        }
    }
}

/// One fact the comparison checked: what the original does, what the project
/// declares, and how they relate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyFinding {
    /// The fact, as a stable dotted name.
    pub field: &'static str,
    /// What the measured original policy says.
    pub original: String,
    /// What the project's policies say.
    pub project: String,
    /// How the two relate.
    pub relation: PolicyRelation,
}

impl std::fmt::Display for PolicyFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}] original: {} | project: {}",
            self.field,
            self.relation.label(),
            self.original,
            self.project
        )
    }
}

/// The result of comparing the declared [`OriginalClockPolicy`] with the
/// project's [`ClockPolicy`]s.
///
/// Unlike [`ProbeComparison`], divergences here are mostly **designed**: the
/// project's fixed tick is a spec requirement (F16 non-negotiable 3 and 4),
/// so a divergence does not contradict the evidence and never changes the
/// claim. The claim is the original policy's own
/// [`claim_status`](OriginalClockPolicy::claim_status) — `inferred` for
/// static code evidence, never `verified_original`.
#[derive(Clone, Debug)]
pub struct OriginalPolicyComparison {
    policy: &'static str,
    project_rate: TickRate,
    tolerance: PolicyTolerance,
    findings: Vec<PolicyFinding>,
    claim: ClaimStatus,
    verified_original: bool,
}

impl OriginalPolicyComparison {
    /// The declared original policy's name.
    #[must_use]
    pub const fn policy(&self) -> &'static str {
        self.policy
    }

    /// The project tick rate the comparison used.
    #[must_use]
    pub const fn project_rate(&self) -> TickRate {
        self.project_rate
    }

    /// The tolerance the comparison was declared with.
    #[must_use]
    pub const fn tolerance(&self) -> PolicyTolerance {
        self.tolerance
    }

    /// Every fact that was checked, in declaration order.
    #[must_use]
    pub fn findings(&self) -> &[PolicyFinding] {
        &self.findings
    }

    /// The findings with relation `relation`.
    #[must_use]
    pub fn findings_with(&self, relation: PolicyRelation) -> Vec<&PolicyFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.relation == relation)
            .collect()
    }

    /// The facts where the original and the project differ.
    #[must_use]
    pub fn divergences(&self) -> Vec<&PolicyFinding> {
        self.findings_with(PolicyRelation::Diverges)
    }

    /// The facts the project's clock policies do not model.
    #[must_use]
    pub fn not_modeled(&self) -> Vec<&PolicyFinding> {
        self.findings_with(PolicyRelation::NotModeled)
    }

    /// The claim the original policy's evidence supports.
    #[must_use]
    pub const fn claim(&self) -> ClaimStatus {
        self.claim
    }

    /// Always false for a declared policy: static code evidence never
    /// verifies the original (see
    /// [`OriginalClockPolicy::claim_status`]).
    #[must_use]
    pub const fn verified_original(&self) -> bool {
        self.verified_original
    }

    /// A one-line summary suitable for an evidence record.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} vs project clock policies at {} Hz: {} findings ({} agree, {} diverge, {} not \
             modelled), tolerance {} ns, claim {}",
            self.policy,
            self.project_rate.ticks_per_second(),
            self.findings.len(),
            self.findings_with(PolicyRelation::Agrees).len(),
            self.divergences().len(),
            self.not_modeled().len(),
            self.tolerance.dt_slack_nanos,
            self.claim.label(),
        )
    }
}

/// `a, b, c` for a subsystem list, for a readable finding.
fn list_subsystems(subsystems: &[OriginalSubsystem]) -> String {
    subsystems
        .iter()
        .copied()
        .map(OriginalSubsystem::label)
        .collect::<Vec<&str>>()
        .join(", ")
}

/// `single-player simulation freeze, multiplayer simulation freeze, …`
fn pause_summary(policies: &[(&'static str, ClockPolicy)]) -> String {
    policies
        .iter()
        .map(|(name, policy)| format!("{name} {}", pause_label(policy.pause())))
        .collect::<Vec<String>>()
        .join(", ")
}

fn pause_label(pause: PausePolicy) -> &'static str {
    match pause {
        PausePolicy::Freeze => "freeze",
        PausePolicy::KeepRunning => "keep-running",
    }
}

fn speed_up_label(speed_up: SpeedUpPolicy) -> &'static str {
    match speed_up {
        SpeedUpPolicy::AdvanceFixedTicks => "advance-fixed-ticks",
        SpeedUpPolicy::NoLocalAuthority => "no-local-authority",
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

    /// Non-negotiable behavior 4 reaches the gameplay quantities too: the
    /// authoritative gameplay clock grants no local speed-up authority, so
    /// nobody can inject ticks straight into a weapon cooldown or an objective
    /// timer, and a refused injection moves neither the tick nor a gameplay
    /// quantity. The refusal is `NoSpeedUpAuthority` whether or not the clock
    /// is paused — the authority check comes first, so `ClockPaused` is not
    /// reachable through this entry point.
    #[test]
    fn accept_f16_d_gameplay_timeline_grants_no_local_speed_up_authority() {
        let rate = TickRate::new(64).expect("64 Hz is valid");
        let mut timeline = GameplayTimeline::new(rate, 120, 600).expect("periods are positive");
        timeline
            .advance_frame(Duration::from_millis(500))
            .expect("runs");
        let before = (
            timeline.tick(),
            timeline.cooldown().remaining_ticks(),
            timeline.cooldown().elapsed_ticks(),
            timeline.objective_timer().remaining_ticks(),
            timeline.objective_timer().elapsed_ticks(),
        );

        assert_eq!(
            timeline.advance_fixed_ticks(5),
            Err(TimeError::NoSpeedUpAuthority),
            "the gameplay clock must expose no local speed-up authority"
        );
        timeline.set_paused(true);
        assert_eq!(
            timeline.advance_fixed_ticks(5),
            Err(TimeError::NoSpeedUpAuthority),
            "a paused clock reports the missing authority, not a pause: the \
             authority check comes first"
        );
        assert_eq!(
            (
                timeline.tick(),
                timeline.cooldown().remaining_ticks(),
                timeline.cooldown().elapsed_ticks(),
                timeline.objective_timer().remaining_ticks(),
                timeline.objective_timer().elapsed_ticks(),
            ),
            before,
            "a refused speed-up moves neither the tick nor a gameplay quantity"
        );
    }

    /// A comparison that agrees can claim only what its reference's evidence
    /// supports. A synthetic-fixture reference is `observed_tool` and never
    /// `verified_original`; a diverging trace is `contradicted` whatever the
    /// evidence says; and a trace that observed *less* than the reference
    /// diverges rather than passing as agreement.
    #[test]
    fn accept_f16_d_probe_comparison_claims_only_what_its_evidence_supports() {
        static STEPS: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: NANOS_PER_SECOND as u64,
            },
            ProbeStep::Observe("after-one-second"),
        ];
        let probe = BehavioralProbe::new("one-second", 30, STEPS).expect("valid probe");
        let make = || GameplayTimeline::new(TickRate::new(64).expect("valid"), 120, 600);
        let measured = probe.run(make).expect("the probe runs");

        let reference = ProbeReference::new("reference", measured.clone(), fixture_evidence(), 0);
        let comparison = ProbeComparison::new(&measured, &reference);
        assert!(comparison.agrees(), "{}", comparison.summary());
        assert_eq!(
            comparison.claim(),
            ClaimStatus::ObservedTool,
            "agreement against a synthetic fixture is not original verification"
        );
        assert!(!comparison.verified_original());
        assert!(comparison.summary().contains("claim observed_tool"));

        // A trace that does not match is contradicted, whatever the evidence.
        let mut diverged = probe
            .run(|| GameplayTimeline::new(TickRate::new(64).expect("valid"), 120, 900))
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

        // A shorter trace diverges too, and names the missing sample.
        let mut short = probe.run(make).expect("runs");
        short.samples.clear();
        let comparison = ProbeComparison::new(&short, &reference);
        assert!(!comparison.agrees());
        assert_eq!(comparison.divergences().len(), 1);
        assert_eq!(
            comparison.divergences()[0].field,
            "sample-present-in-measurement"
        );

        // A tolerance is selected before the comparison, not fitted to it: one
        // tick of difference is inside a one-tick tolerance and outside a
        // zero-tick one.
        diverged.samples[0].objective_remaining_ticks =
            measured.samples()[0].objective_remaining_ticks + 1;
        let within = ProbeComparison::new(
            &diverged,
            &ProbeReference::new("reference", measured.clone(), fixture_evidence(), 1),
        );
        assert!(within.agrees(), "{}", within.summary());
        let outside = ProbeComparison::new(&diverged, &reference);
        assert!(!outside.agrees(), "{}", outside.summary());
    }

    /// A refused timeline is reported, never traced: the probe propagates the
    /// clock's own error instead of producing a trace a caller could compare
    /// as a shorter run.
    #[test]
    fn accept_f16_d_probe_propagates_a_refused_timeline_instead_of_tracing_it() {
        static STEPS: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: NANOS_PER_SECOND as u64,
            },
            ProbeStep::Observe("never-observed"),
        ];
        let probe = BehavioralProbe::new("refused", 30, STEPS).expect("valid probe");
        assert_eq!(
            probe.run(|| GameplayTimeline::new(TickRate::new(64).expect("valid"), 0, 600)),
            Err(ProbeError::Time(TimeError::ZeroTimerPeriod)),
            "a refused timeline must not produce a trace"
        );
    }

    /// The probe's own refusals: an unusable render rate, a script that
    /// observes nothing, a repeated label, and a step that would ask for an
    /// unbounded number of frames.
    #[test]
    fn accept_f16_d_probe_refuses_unusable_rates_labels_and_frame_counts() {
        static OBSERVES: &[ProbeStep] = &[ProbeStep::Observe("only")];
        assert_eq!(
            BehavioralProbe::new("zero-rate", 0, OBSERVES),
            Err(ProbeError::UnusableRenderRate { render_fps: 0 })
        );
        assert_eq!(
            BehavioralProbe::new("sub-nanosecond-rate", u32::MAX, OBSERVES),
            Err(ProbeError::UnusableRenderRate {
                render_fps: u32::MAX
            })
        );
        assert_eq!(
            BehavioralProbe::new("silent", 60, &[ProbeStep::Pause(true)]),
            Err(ProbeError::NoObservations)
        );
        static REPEATED: &[ProbeStep] = &[
            ProbeStep::Observe("twice"),
            ProbeStep::Observe("once"),
            ProbeStep::Observe("twice"),
        ];
        assert_eq!(
            BehavioralProbe::new("repeated", 60, REPEATED),
            Err(ProbeError::DuplicateObservationLabel { label: "twice" })
        );

        static HUGE: &[ProbeStep] = &[
            ProbeStep::Advance { nanos: u64::MAX },
            ProbeStep::Observe("never"),
        ];
        let probe = BehavioralProbe::new("huge", 60, HUGE).expect("valid probe");
        let refused = probe.run(|| GameplayTimeline::new(TickRate::new(64).expect("valid"), 1, 1));
        assert_eq!(
            refused,
            Err(ProbeError::FrameCountTooLarge {
                nanos: u64::MAX,
                frames: u64::MAX / (NANOS_PER_SECOND as u64 / 60) + 1
            }),
            "a step of unbounded frame count must be refused, not attempted"
        );

        // The bound is `MAX_FRAMES_PER_STEP` frames exactly, and the refusal
        // names the exact count. At 60 fps a frame is 16 666 666 ns, so
        // `MAX_FRAMES_PER_STEP` whole frames plus one nanosecond needs one
        // frame more than the limit allows, and the same whole frames with no
        // remainder need exactly the limit.
        let frame = NANOS_PER_SECOND as u64 / 60;
        let over_by_remainder = frame * MAX_FRAMES_PER_STEP + 1;
        static OVER_BY_REMAINDER: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: (1_000_000_000 / 60) * MAX_FRAMES_PER_STEP + 1,
            },
            ProbeStep::Observe("never"),
        ];
        let probe = BehavioralProbe::new("over", 60, OVER_BY_REMAINDER).expect("valid probe");
        assert_eq!(
            probe.run(|| GameplayTimeline::new(TickRate::new(64).expect("valid"), 1, 1)),
            Err(ProbeError::FrameCountTooLarge {
                nanos: over_by_remainder,
                frames: MAX_FRAMES_PER_STEP + 1,
            }),
            "a limit's worth of whole frames plus a remainder frame is one over"
        );
        static EXACTLY_THE_LIMIT: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: (1_000_000_000 / 60) * MAX_FRAMES_PER_STEP,
            },
            ProbeStep::Observe("never"),
        ];
        let probe = BehavioralProbe::new("limit", 60, EXACTLY_THE_LIMIT).expect("valid probe");
        assert_eq!(
            probe.run(|| GameplayTimeline::new(TickRate::new(64).expect("valid"), 1, 1)),
            Ok(ProbeTrace {
                name: "limit",
                render_fps: 60,
                committed_ticks: 1_066_666,
                samples: vec![ProbeSample {
                    label: "never",
                    tick: Tick(1_066_666),
                    paused: false,
                    cooldown_remaining_ticks: 0,
                    cooldown_period_ticks: 1,
                    cooldown_expirations: 1,
                    objective_remaining_ticks: 0,
                    objective_period_ticks: 1,
                }],
            }),
            "exactly the limit is accepted, and no remainder frame is invented \
             when the wall time is a whole number of frames"
        );
    }

    /// The frame split loses nothing: the same wall time commits the same
    /// ticks at every render rate, including one that does not divide a second
    /// evenly, where the remainder frame carries the difference.
    #[test]
    fn accept_f16_d_frame_split_is_exact_at_every_delivery_rate() {
        static ONE_SECOND: &[ProbeStep] = &[
            ProbeStep::Advance {
                nanos: NANOS_PER_SECOND as u64,
            },
            ProbeStep::Observe("after-one-second"),
        ];
        for render_fps in [1_u32, 7, 30, 60, 144, 1000] {
            let probe = BehavioralProbe::new("split", render_fps, ONE_SECOND).expect("valid probe");
            let trace = probe
                .run(|| GameplayTimeline::new(TickRate::new(64).expect("valid"), 120, 600))
                .expect("the probe runs");
            assert_eq!(
                trace.committed_ticks(),
                64,
                "one second at 64 Hz is 64 ticks, delivered at {render_fps} fps"
            );
            assert_eq!(
                trace.sample("after-one-second").expect("observed").tick(),
                Tick(64)
            );
            assert_eq!(
                trace
                    .sample("after-one-second")
                    .expect("observed")
                    .cooldown_remaining_ticks(),
                120 - 64,
                "the cooldown counted the same ticks at {render_fps} fps"
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
