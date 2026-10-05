//! The cinematic session driver: skip, pause, state transitions and failure
//! recovery wired to their producers and consumer (F40-C).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! [`CinematicSession`] joins the F40-A [`CinematicPlayer`] (semantic state
//! machine), the F40-B [`MediaClock`] and [`VideoPlayback`] (presentation) and a
//! [`SemanticSink`] (the mission-side consumer). The producers are the audio
//! device callback ([`CinematicSession::step`]) or the simulation tick, the
//! player's skip press ([`CinematicSession::skip`]) and the decoder. The
//! consumer receives every applied semantic action **exactly once**, even
//! across a retry, and receives control back for the aircraft the player is
//! flying *now* ([`CinematicSession::player_changed_aircraft`]).
//!
//! Failure policy (non-negotiable behavior 1 and 5): a decode error or missing
//! media fails the session, never completes it. By the declared recovery the
//! remaining semantics are applied (and control returned) or the session
//! blocks and keeps control held until [`CinematicSession::retry`] or
//! [`CinematicSession::cancel`]. All of this is designed behavior, not an
//! original-fidelity claim; see `docs/findings/2026-10-05-f40-c-session-wiring.md`.

use std::collections::BTreeSet;
use std::fmt;

use cs_sim::cinematic_state::{
    AppliedEffect, CinematicPlayer, CinematicState, MasterClock, MediaFailure, PausePolicy,
    PlayerError,
};
use cs_sim::damage::ActorId;
use cs_types::content::ContentId;

use super::playback::{FrameDue, FrameSource, MediaClock, VideoPlayback};
use super::{CinematicPlan, MediaAvailability, PresentationPlan, begin};

/// The mission-side consumer of a cinematic's semantic actions.
pub trait SemanticSink {
    /// The scene took player control away.
    fn take_control(&mut self);
    /// An objective event fired.
    fn objective_event(&mut self, objective: &ContentId);
    /// Control returns to `actor`.
    fn return_control(&mut self, actor: ActorId);
}

/// Mission state a cinematic acts on: who the player controls and the
/// objective events raised so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionHandback {
    controlled: ActorId,
    control_held: bool,
    events: Vec<ContentId>,
    returns: u32,
}

impl MissionHandback {
    /// Mission state with the player flying `actor`.
    #[must_use]
    pub const fn new(actor: ActorId) -> Self {
        Self {
            controlled: actor,
            control_held: false,
            events: Vec::new(),
            returns: 0,
        }
    }

    /// The aircraft the player controls (the last one control returned to).
    #[must_use]
    pub const fn controlled(&self) -> ActorId {
        self.controlled
    }

    /// Whether a scene currently holds control.
    #[must_use]
    pub const fn control_held(&self) -> bool {
        self.control_held
    }

    /// The objective events raised, in order.
    #[must_use]
    pub fn events(&self) -> &[ContentId] {
        &self.events
    }

    /// How many times control was returned.
    #[must_use]
    pub const fn control_returns(&self) -> u32 {
        self.returns
    }
}

impl SemanticSink for MissionHandback {
    fn take_control(&mut self) {
        self.control_held = true;
    }

    fn objective_event(&mut self, objective: &ContentId) {
        self.events.push(objective.clone());
    }

    fn return_control(&mut self, actor: ActorId) {
        self.controlled = actor;
        self.control_held = false;
        self.returns += 1;
    }
}

/// Why a session call was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// The player refused the transition.
    Player(PlayerError),
    /// The plan is a video but no playback was supplied although the media is
    /// present.
    MissingPlayback,
    /// The cinematic tick rate is zero.
    ZeroTickRate,
    /// Retry is only valid after a failure that left the semantics unapplied.
    RetryNotAllowed {
        /// The state label.
        state: &'static str,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Player(source) => write!(f, "{source}"),
            Self::MissingPlayback => write!(f, "media is present but no playback was supplied"),
            Self::ZeroTickRate => write!(f, "the cinematic tick rate must not be zero"),
            Self::RetryNotAllowed { state } => {
                write!(f, "retry is not valid in state {state}")
            }
        }
    }
}

impl std::error::Error for SessionError {}

impl From<PlayerError> for SessionError {
    fn from(source: PlayerError) -> Self {
        Self::Player(source)
    }
}

/// One running cinematic.
pub struct CinematicSession<S: FrameSource> {
    plan: CinematicPlan,
    player: CinematicPlayer,
    clock: MediaClock,
    video: Option<VideoPlayback<S>>,
    ticks_per_second: u32,
    delivered_actions: BTreeSet<u32>,
    delivered_upto: usize,
    control_taken: bool,
    control_released: bool,
    last_tick: u64,
}

impl<S: FrameSource> CinematicSession<S> {
    /// Starts `plan`. `video` is the playback for a present video; anything but
    /// [`MediaAvailability::Present`] fails the session on the spot (and, by the
    /// declared recovery, applies the remaining semantics to `sink`).
    ///
    /// # Errors
    ///
    /// [`SessionError`] for a zero tick rate, a present video without
    /// playback, or a player refusal.
    pub fn begin(
        plan: CinematicPlan,
        availability: &MediaAvailability,
        video: Option<VideoPlayback<S>>,
        clock: MediaClock,
        ticks_per_second: u32,
        player_actor: ActorId,
        sink: &mut impl SemanticSink,
    ) -> Result<Self, SessionError> {
        if ticks_per_second == 0 {
            return Err(SessionError::ZeroTickRate);
        }
        if matches!(plan.presentation, PresentationPlan::Video { .. })
            && *availability == MediaAvailability::Present
            && video.is_none()
        {
            return Err(SessionError::MissingPlayback);
        }
        let player = begin(&plan, availability, player_actor)?;
        let mut session = Self {
            plan,
            player,
            clock,
            video,
            ticks_per_second,
            delivered_actions: BTreeSet::new(),
            delivered_upto: 0,
            control_taken: false,
            control_released: false,
            last_tick: 0,
        };
        sink.take_control();
        session.control_taken = true;
        session.settle(sink);
        Ok(session)
    }

    /// The player state.
    #[must_use]
    pub const fn state(&self) -> &CinematicState {
        self.player.state()
    }

    /// The underlying player.
    #[must_use]
    pub const fn player(&self) -> &CinematicPlayer {
        &self.player
    }

    /// The media clock.
    #[must_use]
    pub const fn clock(&self) -> &MediaClock {
        &self.clock
    }

    /// The video playback, while the session has not been torn down.
    #[must_use]
    pub const fn video(&self) -> Option<&VideoPlayback<S>> {
        self.video.as_ref()
    }

    /// Whether the simulation must stand still this frame: the scene declares
    /// [`PausePolicy::SimulationPaused`] and has not ended.
    #[must_use]
    pub fn simulation_paused(&self) -> bool {
        self.plan.script.pause() == PausePolicy::SimulationPaused && !self.state().is_terminal()
    }

    /// The failure reason, if the session failed.
    #[must_use]
    pub const fn failure(&self) -> Option<&MediaFailure> {
        match self.player.state() {
            CinematicState::Failed { reason, .. } => Some(reason),
            _ => None,
        }
    }

    /// The player changed aircraft during the scene: control will return to
    /// `actor`, not to whoever flew when the scene began.
    pub const fn player_changed_aircraft(&mut self, actor: ActorId) {
        self.player.set_player_actor(actor);
    }

    /// Pauses the scene: the media clock stops and `step` does nothing until
    /// [`CinematicSession::resume`].
    ///
    /// # Errors
    ///
    /// [`SessionError::Player`] unless the scene is playing.
    pub fn pause(&mut self) -> Result<(), SessionError> {
        self.require_playing("pause")?;
        self.clock.pause();
        Ok(())
    }

    /// Resumes a paused scene.
    ///
    /// # Errors
    ///
    /// [`SessionError::Player`] unless the scene is playing.
    pub fn resume(&mut self) -> Result<(), SessionError> {
        self.require_playing("resume")?;
        self.clock.resume();
        Ok(())
    }

    fn require_playing(&self, call: &'static str) -> Result<(), SessionError> {
        if matches!(self.state(), CinematicState::Playing { .. }) {
            Ok(())
        } else {
            Err(PlayerError::WrongState {
                call,
                state: self.state().label(),
            }
            .into())
        }
    }

    /// One presentation step. `samples` is what the audio device played since
    /// the last step (ignored while paused). For an audio-mastered scene the
    /// cinematic tick follows the clock; for a simulation-mastered scene it
    /// advances by one. A decode error fails the session.
    ///
    /// # Errors
    ///
    /// [`SessionError::Player`] unless the scene is playing.
    pub fn step(&mut self, samples: u64, sink: &mut impl SemanticSink) -> Result<(), SessionError> {
        self.require_playing("step")?;
        if self.clock.is_paused() {
            return Ok(());
        }
        self.clock.audio_played(samples);
        let target = match self.plan.script.clock() {
            MasterClock::Audio => {
                let ticks = u128::from(self.clock.position_us())
                    * u128::from(self.ticks_per_second)
                    / 1_000_000;
                u64::try_from(ticks).unwrap_or(u64::MAX)
            }
            MasterClock::Simulation => self.last_tick.saturating_add(1),
        };
        let delta = target.saturating_sub(self.last_tick);
        self.last_tick = self.last_tick.max(target);
        self.player.advance(delta)?;
        if let Some(video) = &mut self.video {
            match video.frame_due(&self.clock) {
                Ok(FrameDue::Show { .. } | FrameDue::Hold | FrameDue::Ended) => {}
                Err(error) => {
                    self.player
                        .media_failed(MediaFailure::DecodeFailed { detail: error.0 })?;
                }
            }
        }
        self.settle(sink);
        Ok(())
    }

    /// Skips the scene: every action not yet applied is applied once and the
    /// semantic end state is reached. Not an abort.
    ///
    /// # Errors
    ///
    /// [`SessionError::Player`] if the scene is not skippable or not playing.
    pub fn skip(&mut self, sink: &mut impl SemanticSink) -> Result<(), SessionError> {
        self.player.request_skip()?;
        self.player.settle_skip()?;
        self.settle(sink);
        Ok(())
    }

    /// Aborts the scene without applying further semantics. Control is handed
    /// back to the current aircraft so teardown never strands the player.
    ///
    /// # Errors
    ///
    /// [`SessionError::Player`] once the scene is terminal.
    pub fn cancel(&mut self, sink: &mut impl SemanticSink) -> Result<(), SessionError> {
        self.player.cancel()?;
        self.settle(sink);
        Ok(())
    }

    /// Gives up on a failed scene whose recovery blocked: control returns to
    /// the current aircraft and no further semantics are applied, so a blocked
    /// failure that will not be retried cannot strand the player.
    ///
    /// # Errors
    ///
    /// [`SessionError::RetryNotAllowed`] unless the scene failed.
    pub fn abandon(&mut self, sink: &mut impl SemanticSink) -> Result<(), SessionError> {
        if !matches!(self.state(), CinematicState::Failed { .. }) {
            return Err(SessionError::RetryNotAllowed {
                state: self.state().label(),
            });
        }
        if self.control_taken && !self.control_released {
            sink.return_control(self.player.player_actor());
            self.control_released = true;
        }
        Ok(())
    }

    /// Retries a failed scene from its start with fresh media. Actions already
    /// delivered to the sink are not delivered again. Only valid after a
    /// failure whose recovery blocked; a scene whose semantics were applied is
    /// already at its semantic end.
    ///
    /// # Errors
    ///
    /// [`SessionError::RetryNotAllowed`] outside a blocked failure.
    pub fn retry(
        &mut self,
        availability: &MediaAvailability,
        video: Option<VideoPlayback<S>>,
        clock: MediaClock,
        sink: &mut impl SemanticSink,
    ) -> Result<(), SessionError> {
        if !matches!(
            self.state(),
            CinematicState::Failed {
                semantics_applied: false,
                ..
            }
        ) {
            return Err(SessionError::RetryNotAllowed {
                state: self.state().label(),
            });
        }
        if matches!(self.plan.presentation, PresentationPlan::Video { .. })
            && *availability == MediaAvailability::Present
            && video.is_none()
        {
            return Err(SessionError::MissingPlayback);
        }
        self.player = begin(&self.plan, availability, self.player.player_actor())?;
        self.clock = clock;
        self.video = video;
        self.delivered_upto = 0;
        self.last_tick = 0;
        self.settle(sink);
        Ok(())
    }

    /// Delivers newly applied actions to the sink, each id once, and tears the
    /// presentation down when the scene ended.
    fn settle(&mut self, sink: &mut impl SemanticSink) {
        let applied = self.player.applied();
        for action in &applied[self.delivered_upto.min(applied.len())..] {
            if !self.delivered_actions.insert(action.id.0) {
                continue;
            }
            match &action.effect {
                AppliedEffect::ObjectiveEvent(id) => sink.objective_event(id),
                AppliedEffect::ControlReturned(actor) => {
                    sink.return_control(*actor);
                    self.control_released = true;
                }
            }
        }
        self.delivered_upto = applied.len();
        if self.control_taken
            && !self.control_released
            && matches!(self.player.state(), CinematicState::Canceled)
        {
            sink.return_control(self.player.player_actor());
            self.control_released = true;
        }
        if self.player.state().is_terminal() {
            self.video = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cinematics::playback::{DecodeError, DecodedFrame};
    use cs_sim::cinematic_state::{
        CompletionKind, FailureRecovery, SYNTHETIC_DURATION_TICKS, SYNTHETIC_PLAYER,
        SYNTHETIC_SWAPPED_PLAYER, synthetic_cinematic_id, synthetic_script,
    };

    const RATE: u32 = 48_000;
    const TICKS_PER_SECOND: u32 = 25;
    const FRAME_US: u64 = 40_000;
    /// Samples in one cinematic tick (one frame period).
    const TICK_SAMPLES: u64 = 1_920;

    struct Frames {
        next: u64,
        fail_at: Option<u64>,
    }

    impl FrameSource for Frames {
        fn next_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
            if self.fail_at == Some(self.next) {
                return Err(DecodeError("corrupt packet".into()));
            }
            let frame = DecodedFrame {
                pts_us: self.next * FRAME_US,
                index: self.next,
            };
            self.next += 1;
            Ok(Some(frame))
        }
    }

    fn video(fail_at: Option<u64>) -> VideoPlayback<Frames> {
        VideoPlayback::new(Frames { next: 0, fail_at }, FRAME_US)
    }

    fn plan(skippable: bool, recovery: FailureRecovery) -> CinematicPlan {
        CinematicPlan {
            script: synthetic_script(skippable, recovery),
            presentation: PresentationPlan::Video {
                media: synthetic_cinematic_id(),
                format: "synthetic".into(),
                frame_size: (640, 480),
            },
        }
    }

    fn clock() -> MediaClock {
        MediaClock::new(RATE).unwrap()
    }

    fn start(
        skippable: bool,
        recovery: FailureRecovery,
        fail_at: Option<u64>,
        mission: &mut MissionHandback,
    ) -> CinematicSession<Frames> {
        CinematicSession::begin(
            plan(skippable, recovery),
            &MediaAvailability::Present,
            Some(video(fail_at)),
            clock(),
            TICKS_PER_SECOND,
            SYNTHETIC_PLAYER,
            mission,
        )
        .unwrap()
    }

    fn play_ticks(
        session: &mut CinematicSession<Frames>,
        ticks: u64,
        mission: &mut MissionHandback,
    ) {
        for _ in 0..ticks {
            if !matches!(session.state(), CinematicState::Playing { .. }) {
                break;
            }
            session.step(TICK_SAMPLES, mission).unwrap();
        }
    }

    fn played_reference() -> MissionHandback {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let mut session = start(true, FailureRecovery::Block, None, &mut mission);
        session.player_changed_aircraft(SYNTHETIC_SWAPPED_PLAYER);
        play_ticks(&mut session, SYNTHETIC_DURATION_TICKS, &mut mission);
        assert_eq!(
            session.state(),
            &CinematicState::Completed(CompletionKind::Played)
        );
        mission
    }

    #[test]
    fn accept_f40_c_aircraft_change_returns_control_to_the_current_actor() {
        let mission = played_reference();
        assert_eq!(mission.controlled(), SYNTHETIC_SWAPPED_PLAYER);
        assert!(!mission.control_held());
        assert_eq!(mission.control_returns(), 1);
        assert_eq!(mission.events().len(), 2);
    }

    #[test]
    fn accept_f40_c_skip_at_any_point_reaches_the_played_mission_state_once() {
        let reference = played_reference();
        for at in [
            0,
            SYNTHETIC_DURATION_TICKS / 2,
            SYNTHETIC_DURATION_TICKS - 1,
        ] {
            let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
            let mut session = start(true, FailureRecovery::Block, None, &mut mission);
            assert!(mission.control_held());
            play_ticks(&mut session, at, &mut mission);
            session.player_changed_aircraft(SYNTHETIC_SWAPPED_PLAYER);
            session.skip(&mut mission).unwrap();
            assert_eq!(
                session.state(),
                &CinematicState::Completed(CompletionKind::Skipped)
            );
            assert_eq!(mission, reference, "skip at {at}");
            // A second press neither errors into a double application nor
            // re-delivers anything.
            assert!(session.skip(&mut mission).is_err());
            assert_eq!(mission, reference, "second skip at {at}");
            assert!(session.video().is_none(), "presentation torn down");
        }
    }

    #[test]
    fn accept_f40_c_unskippable_scene_refuses_skip_and_keeps_control_held() {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let mut session = start(false, FailureRecovery::Block, None, &mut mission);
        assert_eq!(
            session.skip(&mut mission),
            Err(SessionError::Player(PlayerError::NotSkippable))
        );
        assert!(mission.control_held());
        assert!(matches!(session.state(), CinematicState::Playing { .. }));
    }

    #[test]
    fn accept_f40_c_pause_holds_clock_video_and_actions_and_resume_continues() {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let mut session = start(true, FailureRecovery::Block, None, &mut mission);
        assert!(session.simulation_paused());
        play_ticks(&mut session, 10, &mut mission);
        let state = session.state().clone();
        let shown = session.video().unwrap().shown().cloned();
        session.pause().unwrap();
        // The device keeps calling back; nothing may move while paused.
        play_ticks(&mut session, 500, &mut mission);
        assert_eq!(session.state(), &state);
        assert_eq!(session.video().unwrap().shown().cloned(), shown);
        assert_eq!(mission.events().len(), 1);
        session.resume().unwrap();
        play_ticks(&mut session, 1, &mut mission);
        assert_eq!(session.state(), &CinematicState::Playing { elapsed: 11 });
        assert!(
            session
                .video()
                .unwrap()
                .drift(session.clock())
                .unwrap()
                .within_tolerance()
        );
        play_ticks(&mut session, SYNTHETIC_DURATION_TICKS - 11, &mut mission);
        assert!(!session.simulation_paused());
        assert!(session.pause().is_err());
    }

    #[test]
    fn accept_f40_c_decode_failure_with_apply_recovery_finishes_semantics_without_completing() {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let mut session = start(
            true,
            FailureRecovery::ApplyRemainingSemantics,
            Some(20),
            &mut mission,
        );
        session.player_changed_aircraft(SYNTHETIC_SWAPPED_PLAYER);
        play_ticks(&mut session, 30, &mut mission);
        assert!(matches!(
            session.failure(),
            Some(MediaFailure::DecodeFailed { detail }) if detail == "corrupt packet"
        ));
        assert!(!matches!(session.state(), CinematicState::Completed(_)));
        assert_eq!(mission, played_reference());
        assert!(session.video().is_none());
        assert!(session.step(TICK_SAMPLES, &mut mission).is_err());
    }

    #[test]
    fn accept_f40_c_blocked_failure_holds_control_then_retry_delivers_each_action_once() {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let mut session = start(true, FailureRecovery::Block, Some(30), &mut mission);
        // The decoder fails at frame 30, before the midpoint action at tick 50.
        play_ticks(&mut session, 40, &mut mission);
        assert!(session.failure().is_some());
        assert!(mission.control_held(), "blocked: control stays held");
        assert_eq!(mission.control_returns(), 0);
        assert!(session.video().is_none());

        session
            .retry(
                &MediaAvailability::Present,
                Some(video(None)),
                clock(),
                &mut mission,
            )
            .unwrap();
        session.player_changed_aircraft(SYNTHETIC_SWAPPED_PLAYER);
        play_ticks(&mut session, SYNTHETIC_DURATION_TICKS, &mut mission);
        assert_eq!(
            session.state(),
            &CinematicState::Completed(CompletionKind::Played)
        );
        assert_eq!(mission, played_reference());
        // A completed scene cannot be retried.
        assert_eq!(
            session
                .retry(&MediaAvailability::Present, None, clock(), &mut mission)
                .unwrap_err(),
            SessionError::RetryNotAllowed { state: "completed" }
        );
    }

    #[test]
    fn accept_f40_c_missing_media_fails_and_cancel_returns_control_without_completion() {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let mut session = CinematicSession::<Frames>::begin(
            plan(true, FailureRecovery::Block),
            &MediaAvailability::MissingFile,
            None,
            clock(),
            TICKS_PER_SECOND,
            SYNTHETIC_PLAYER,
            &mut mission,
        )
        .unwrap();
        assert!(matches!(
            session.failure(),
            Some(MediaFailure::MissingMedia { .. })
        ));
        assert!(!session.player().semantic_end_reached());
        assert!(mission.control_held());
        // Retrying while the file is still missing fails again, usefully.
        session
            .retry(&MediaAvailability::NoDecoder, None, clock(), &mut mission)
            .unwrap();
        assert!(matches!(
            session.failure(),
            Some(MediaFailure::MissingDecoder { .. })
        ));
        // A failed player cannot be canceled; giving up hands control back.
        assert!(session.cancel(&mut mission).is_err());
        assert!(mission.control_held());
        session.player_changed_aircraft(SYNTHETIC_SWAPPED_PLAYER);
        session.abandon(&mut mission).unwrap();
        assert!(!mission.control_held());
        assert_eq!(mission.controlled(), SYNTHETIC_SWAPPED_PLAYER);
        assert!(!session.player().semantic_end_reached());
    }

    #[test]
    fn accept_f40_c_cancel_during_play_applies_nothing_and_returns_control() {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let mut session = start(true, FailureRecovery::Block, None, &mut mission);
        session.player_changed_aircraft(SYNTHETIC_SWAPPED_PLAYER);
        play_ticks(&mut session, 10, &mut mission);
        session.cancel(&mut mission).unwrap();
        assert_eq!(session.state(), &CinematicState::Canceled);
        assert_eq!(mission.controlled(), SYNTHETIC_SWAPPED_PLAYER);
        assert!(!mission.control_held());
        assert_eq!(mission.events().len(), 1, "no further semantics");
    }

    #[test]
    fn accept_f40_c_present_video_without_playback_and_zero_rate_are_refused() {
        let mut mission = MissionHandback::new(SYNTHETIC_PLAYER);
        let refused = CinematicSession::<Frames>::begin(
            plan(true, FailureRecovery::Block),
            &MediaAvailability::Present,
            None,
            clock(),
            TICKS_PER_SECOND,
            SYNTHETIC_PLAYER,
            &mut mission,
        );
        assert_eq!(refused.err(), Some(SessionError::MissingPlayback));
        let zero = CinematicSession::begin(
            plan(true, FailureRecovery::Block),
            &MediaAvailability::Present,
            Some(video(None)),
            clock(),
            0,
            SYNTHETIC_PLAYER,
            &mut mission,
        );
        assert_eq!(zero.err(), Some(SessionError::ZeroTickRate));
        assert!(!mission.control_held());
    }
}
