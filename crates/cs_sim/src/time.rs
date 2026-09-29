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

use std::time::Duration;

use cs_types::Tick;

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
}

impl std::fmt::Display for TimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroTickRate => write!(f, "tick rate must be greater than zero"),
            Self::NoSpeedUpAuthority => write!(f, "this clock grants no local speed-up authority"),
            Self::ClockPaused => write!(f, "the clock is paused and freezes its ticks"),
            Self::TickOverflow => write!(f, "tick counter would overflow"),
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
}
