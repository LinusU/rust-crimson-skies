//! The cinematic player state machine and its semantic actions (F40-A).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage **F40-A** is the typed contract and a minimal synthetic fixture — not
//! decoded playback or authored camera timelines (F40-B), the skip/pause wiring
//! (F40-C) or the original cinematic validation (F40-D). Two things are kept
//! apart on purpose (non-negotiable behavior 1):
//!
//! * the **semantic actions** ([`SemanticAction`]) — objective events and the
//!   return of player control — which change mission state; and
//! * the **media presentation** — the video or camera timeline — which only
//!   shows it. The player owns the first and merely reports on the second.
//!
//! [`CinematicPlayer`] has the explicit states [`CinematicState::Start`],
//! `Playing`, `SkipRequested`, `Completed`, `Failed` and `Canceled`. Every
//! semantic action is applied **exactly once**, in `(tick, id)` order, whether
//! the scene plays out, is skipped at the start, midpoint or final frame, or
//! (by the declared [`FailureRecovery`]) its media fails. Skipping is therefore
//! not an abort: [`CinematicPlayer::cancel`] applies nothing further and is the
//! only way to leave actions unapplied.
//!
//! Missing media is [`CinematicState::Failed`], never `Completed`
//! (non-negotiable behavior 5). The original cinematic inventory, durations,
//! skip rules and pause rules are unrecovered, so everything here is designed
//! behavior and no original-fidelity claim: see
//! `docs/findings/2026-10-01-f40-a-cinematic-media-inventory.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

use crate::damage::ActorId;

/// A stable id of one semantic action within its script.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticActionId(pub u32);

/// What a semantic action does to mission state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticEffect {
    /// Raises an objective/capture event the mission must still see.
    ObjectiveEvent(ContentId),
    /// Hands control back to whichever aircraft the player flies when it
    /// fires, so an aircraft swap during the scene is honored.
    ReturnControlToPlayer,
}

/// One semantic action at a cinematic tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticAction {
    /// The action's id.
    pub id: SemanticActionId,
    /// The cinematic tick, `0..=duration_ticks`, it fires at.
    pub at_tick: u64,
    /// The effect.
    pub effect: SemanticEffect,
}

/// Whether the simulation runs while the scene plays (non-negotiable
/// behavior 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PausePolicy {
    /// The simulation is paused for the scene.
    SimulationPaused,
    /// The simulation keeps ticking under the scene.
    SimulationRuns,
}

/// The master clock lip-sync, dialogue and video follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MasterClock {
    /// The audio device clock.
    Audio,
    /// The simulation tick clock (no audio in the scene).
    Simulation,
}

/// What happens to the semantic actions when media fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FailureRecovery {
    /// Apply every remaining action once, then report `Failed`: the story
    /// proceeds, the missing media is still reported.
    ApplyRemainingSemantics,
    /// Apply nothing more and report `Failed`: the scene must be fixed first.
    Block,
}

/// Why a script was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptError {
    /// The duration was zero.
    ZeroDuration,
    /// Two actions shared an id.
    DuplicateAction(SemanticActionId),
    /// An action fired after the end of the scene.
    ActionAfterEnd {
        /// The action.
        id: SemanticActionId,
        /// Its tick.
        at_tick: u64,
        /// The scene duration.
        duration_ticks: u64,
    },
    /// An objective event named content that is not an objective.
    NotAnObjective(ContentId),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDuration => write!(f, "a cinematic must last at least one tick"),
            Self::DuplicateAction(id) => write!(f, "semantic action {} is declared twice", id.0),
            Self::ActionAfterEnd {
                id,
                at_tick,
                duration_ticks,
            } => write!(
                f,
                "semantic action {} fires at tick {at_tick}, after the end at {duration_ticks}",
                id.0
            ),
            Self::NotAnObjective(id) => write!(f, "objective event {id} is not an objective"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// The validated runtime description of one cinematic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CinematicScript {
    id: ContentId,
    duration_ticks: u64,
    skippable: bool,
    pause: PausePolicy,
    clock: MasterClock,
    recovery: FailureRecovery,
    actions: Vec<SemanticAction>,
}

impl CinematicScript {
    /// Builds a script; actions are ordered by `(tick, id)`.
    ///
    /// # Errors
    ///
    /// [`ScriptError`] for a zero duration, a duplicate action id, an action
    /// past the end or an objective event that names a non-objective.
    pub fn try_new(
        id: ContentId,
        duration_ticks: u64,
        skippable: bool,
        pause: PausePolicy,
        clock: MasterClock,
        recovery: FailureRecovery,
        mut actions: Vec<SemanticAction>,
    ) -> Result<Self, ScriptError> {
        if duration_ticks == 0 {
            return Err(ScriptError::ZeroDuration);
        }
        let mut seen = BTreeSet::new();
        for action in &actions {
            if !seen.insert(action.id) {
                return Err(ScriptError::DuplicateAction(action.id));
            }
            if action.at_tick > duration_ticks {
                return Err(ScriptError::ActionAfterEnd {
                    id: action.id,
                    at_tick: action.at_tick,
                    duration_ticks,
                });
            }
            if let SemanticEffect::ObjectiveEvent(objective) = &action.effect
                && objective.kind() != ContentKind::Objective
            {
                return Err(ScriptError::NotAnObjective(objective.clone()));
            }
        }
        actions.sort_by_key(|action| (action.at_tick, action.id));
        Ok(Self {
            id,
            duration_ticks,
            skippable,
            pause,
            clock,
            recovery,
            actions,
        })
    }

    /// The cinematic's content id.
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    /// The scene length in cinematic ticks.
    #[must_use]
    pub const fn duration_ticks(&self) -> u64 {
        self.duration_ticks
    }

    /// Whether the player may skip.
    #[must_use]
    pub const fn skippable(&self) -> bool {
        self.skippable
    }

    /// The simulation pause policy.
    #[must_use]
    pub const fn pause(&self) -> PausePolicy {
        self.pause
    }

    /// The master clock.
    #[must_use]
    pub const fn clock(&self) -> MasterClock {
        self.clock
    }

    /// The media-failure recovery.
    #[must_use]
    pub const fn recovery(&self) -> FailureRecovery {
        self.recovery
    }

    /// The actions, in firing order.
    #[must_use]
    pub fn actions(&self) -> &[SemanticAction] {
        &self.actions
    }
}

/// How a completed scene ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    /// It played to the final frame.
    Played,
    /// The player skipped it.
    Skipped,
}

/// Why presentation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaFailure {
    /// The video or media file is missing from the installation.
    MissingMedia {
        /// What was looked for.
        media: ContentId,
    },
    /// No decoder is available for the media's format.
    MissingDecoder {
        /// The format label that has no decoder.
        format: String,
    },
    /// Decoding failed part-way.
    DecodeFailed {
        /// The decoder's message.
        detail: String,
    },
}

impl fmt::Display for MediaFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingMedia { media } => write!(f, "missing media {media}"),
            Self::MissingDecoder { format } => write!(f, "no decoder for format {format}"),
            Self::DecodeFailed { detail } => write!(f, "decode failed: {detail}"),
        }
    }
}

impl std::error::Error for MediaFailure {}

/// The player's explicit state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CinematicState {
    /// Not started.
    Start,
    /// Playing at `elapsed` ticks.
    Playing {
        /// Ticks played so far.
        elapsed: u64,
    },
    /// A skip was requested at `elapsed` ticks and awaits settling.
    SkipRequested {
        /// Ticks played when the skip was requested.
        elapsed: u64,
    },
    /// The scene finished and its semantic end state was reached.
    Completed(CompletionKind),
    /// Presentation failed; never a completion.
    Failed {
        /// Why.
        reason: MediaFailure,
        /// Whether the remaining semantic actions were applied anyway.
        semantics_applied: bool,
    },
    /// Aborted: no further semantic action was applied.
    Canceled,
}

impl CinematicState {
    /// Whether no further transition is possible.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed(_) | Self::Failed { .. } | Self::Canceled
        )
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Playing { .. } => "playing",
            Self::SkipRequested { .. } => "skip_requested",
            Self::Completed(_) => "completed",
            Self::Failed { .. } => "failed",
            Self::Canceled => "canceled",
        }
    }
}

/// A semantic effect as it landed on mission state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppliedEffect {
    /// An objective event was raised.
    ObjectiveEvent(ContentId),
    /// Control returned to this aircraft.
    ControlReturned(ActorId),
}

/// One applied action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedAction {
    /// The action.
    pub id: SemanticActionId,
    /// What it did.
    pub effect: AppliedEffect,
}

/// Why a player call was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerError {
    /// The call is not valid in the current state.
    WrongState {
        /// The call.
        call: &'static str,
        /// The state label.
        state: &'static str,
    },
    /// The scene declares itself not skippable.
    NotSkippable,
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongState { call, state } => write!(f, "{call} is not valid in state {state}"),
            Self::NotSkippable => write!(f, "this cinematic cannot be skipped"),
        }
    }
}

impl std::error::Error for PlayerError {}

/// Drives one cinematic and applies its semantic actions exactly once.
#[derive(Clone, Debug)]
pub struct CinematicPlayer {
    script: CinematicScript,
    state: CinematicState,
    player_actor: ActorId,
    next_action: usize,
    applied: Vec<AppliedAction>,
}

impl CinematicPlayer {
    /// A player at [`CinematicState::Start`] with `player_actor` flying.
    #[must_use]
    pub const fn new(script: CinematicScript, player_actor: ActorId) -> Self {
        Self {
            script,
            state: CinematicState::Start,
            player_actor,
            next_action: 0,
            applied: Vec::new(),
        }
    }

    /// The script.
    #[must_use]
    pub const fn script(&self) -> &CinematicScript {
        &self.script
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> &CinematicState {
        &self.state
    }

    /// The actions applied so far, in order. Comparing this across a played
    /// and a skipped run is the "skipping reaches the same end state" check.
    #[must_use]
    pub fn applied(&self) -> &[AppliedAction] {
        &self.applied
    }

    /// The aircraft that control would return to now.
    #[must_use]
    pub const fn player_actor(&self) -> ActorId {
        self.player_actor
    }

    /// Records an aircraft change during the scene (non-negotiable behavior
    /// 1's control hand-back).
    pub const fn set_player_actor(&mut self, actor: ActorId) {
        self.player_actor = actor;
    }

    fn wrong_state(&self, call: &'static str) -> PlayerError {
        PlayerError::WrongState {
            call,
            state: self.state.label(),
        }
    }

    fn apply_through(&mut self, tick: u64) {
        while let Some(action) = self.script.actions.get(self.next_action) {
            if action.at_tick > tick {
                break;
            }
            let effect = match &action.effect {
                SemanticEffect::ObjectiveEvent(id) => AppliedEffect::ObjectiveEvent(id.clone()),
                SemanticEffect::ReturnControlToPlayer => {
                    AppliedEffect::ControlReturned(self.player_actor)
                }
            };
            self.applied.push(AppliedAction {
                id: action.id,
                effect,
            });
            self.next_action += 1;
        }
    }

    fn apply_all(&mut self) {
        self.apply_through(u64::MAX);
    }

    /// `Start` → `Playing { elapsed: 0 }`, applying actions at tick 0.
    ///
    /// # Errors
    ///
    /// [`PlayerError::WrongState`] outside `Start`.
    pub fn start(&mut self) -> Result<(), PlayerError> {
        if self.state != CinematicState::Start {
            return Err(self.wrong_state("start"));
        }
        self.state = CinematicState::Playing { elapsed: 0 };
        self.apply_through(0);
        Ok(())
    }

    /// Advances the scene by `ticks` of the master clock, applying every action
    /// that falls due; reaching the end completes it as played.
    ///
    /// # Errors
    ///
    /// [`PlayerError::WrongState`] outside `Playing`.
    pub fn advance(&mut self, ticks: u64) -> Result<(), PlayerError> {
        let CinematicState::Playing { elapsed } = self.state else {
            return Err(self.wrong_state("advance"));
        };
        let elapsed = elapsed
            .saturating_add(ticks)
            .min(self.script.duration_ticks);
        self.apply_through(elapsed);
        self.state = if elapsed == self.script.duration_ticks {
            CinematicState::Completed(CompletionKind::Played)
        } else {
            CinematicState::Playing { elapsed }
        };
        Ok(())
    }

    /// `Playing` → `SkipRequested`. Asking again while pending is a no-op, so a
    /// double press cannot apply anything twice.
    ///
    /// # Errors
    ///
    /// [`PlayerError::NotSkippable`] for an unskippable scene,
    /// [`PlayerError::WrongState`] otherwise outside `Playing`.
    pub fn request_skip(&mut self) -> Result<(), PlayerError> {
        match self.state {
            CinematicState::SkipRequested { .. } => Ok(()),
            CinematicState::Playing { elapsed } => {
                if !self.script.skippable {
                    return Err(PlayerError::NotSkippable);
                }
                self.state = CinematicState::SkipRequested { elapsed };
                Ok(())
            }
            _ => Err(self.wrong_state("request_skip")),
        }
    }

    /// `SkipRequested` → `Completed(Skipped)`, applying every action not yet
    /// applied, once.
    ///
    /// # Errors
    ///
    /// [`PlayerError::WrongState`] outside `SkipRequested`.
    pub fn settle_skip(&mut self) -> Result<(), PlayerError> {
        if !matches!(self.state, CinematicState::SkipRequested { .. }) {
            return Err(self.wrong_state("settle_skip"));
        }
        self.apply_all();
        self.state = CinematicState::Completed(CompletionKind::Skipped);
        Ok(())
    }

    /// Reports a presentation failure. By the declared [`FailureRecovery`] the
    /// remaining actions are applied once or not at all; the state is
    /// `Failed` either way and never `Completed`.
    ///
    /// # Errors
    ///
    /// [`PlayerError::WrongState`] once the player is terminal.
    pub fn media_failed(&mut self, reason: MediaFailure) -> Result<(), PlayerError> {
        if self.state.is_terminal() {
            return Err(self.wrong_state("media_failed"));
        }
        let semantics_applied = self.script.recovery == FailureRecovery::ApplyRemainingSemantics;
        if semantics_applied {
            self.apply_all();
        }
        self.state = CinematicState::Failed {
            reason,
            semantics_applied,
        };
        Ok(())
    }

    /// Aborts the scene without applying anything further.
    ///
    /// # Errors
    ///
    /// [`PlayerError::WrongState`] once the player is terminal.
    pub fn cancel(&mut self) -> Result<(), PlayerError> {
        if self.state.is_terminal() {
            return Err(self.wrong_state("cancel"));
        }
        self.state = CinematicState::Canceled;
        Ok(())
    }

    /// Whether the scene's semantic end state was reached: completed, or failed
    /// with the remaining actions applied.
    #[must_use]
    pub const fn semantic_end_reached(&self) -> bool {
        matches!(
            self.state,
            CinematicState::Completed(_)
                | CinematicState::Failed {
                    semantics_applied: true,
                    ..
                }
        )
    }
}

/// The session generation the synthetic fixture belongs to.
pub const SYNTHETIC_SESSION: u64 = 9;
/// [`SYNTHETIC_SESSION`] as the shared nonzero session type.
pub const SYNTHETIC_SESSION_ID: SessionId = match SessionId::new(SYNTHETIC_SESSION) {
    Some(id) => id,
    None => unreachable!(),
};
/// The aircraft the player starts the synthetic scene in.
pub const SYNTHETIC_PLAYER: ActorId = ActorId {
    session: SYNTHETIC_SESSION_ID,
    serial: 1,
};
/// The aircraft the player swaps to during the synthetic scene.
pub const SYNTHETIC_SWAPPED_PLAYER: ActorId = ActorId {
    session: SYNTHETIC_SESSION_ID,
    serial: 2,
};
/// The synthetic scene length: 10 s at 10 ticks/s.
pub const SYNTHETIC_DURATION_TICKS: u64 = 100;

/// The synthetic scene's cinematic id.
#[must_use]
pub fn synthetic_cinematic_id() -> ContentId {
    ContentId::from_source(ContentKind::Video, "synthetic.f40a.intro")
        .expect("the synthetic cinematic id is valid")
}

/// The synthetic capture objective the scene must still raise.
#[must_use]
pub fn synthetic_objective() -> ContentId {
    ContentId::from_source(ContentKind::Objective, "synthetic.f40a.briefed")
        .expect("the synthetic objective id is valid")
}

/// A newly authored three-action scene: an objective event at the start, one
/// at the midpoint and a control hand-back on the final frame.
#[must_use]
pub fn synthetic_script(skippable: bool, recovery: FailureRecovery) -> CinematicScript {
    CinematicScript::try_new(
        synthetic_cinematic_id(),
        SYNTHETIC_DURATION_TICKS,
        skippable,
        PausePolicy::SimulationPaused,
        MasterClock::Audio,
        recovery,
        vec![
            SemanticAction {
                id: SemanticActionId(1),
                at_tick: 0,
                effect: SemanticEffect::ObjectiveEvent(synthetic_objective()),
            },
            SemanticAction {
                id: SemanticActionId(2),
                at_tick: SYNTHETIC_DURATION_TICKS / 2,
                effect: SemanticEffect::ObjectiveEvent(
                    ContentId::from_source(ContentKind::Objective, "synthetic.f40a.midpoint")
                        .expect("the synthetic objective id is valid"),
                ),
            },
            SemanticAction {
                id: SemanticActionId(3),
                at_tick: SYNTHETIC_DURATION_TICKS,
                effect: SemanticEffect::ReturnControlToPlayer,
            },
        ],
    )
    .expect("the synthetic script is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn played() -> CinematicPlayer {
        let mut player = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::ApplyRemainingSemantics),
            SYNTHETIC_PLAYER,
        );
        player.start().unwrap();
        player.advance(SYNTHETIC_DURATION_TICKS).unwrap();
        player
    }

    fn skipped_after(ticks: u64) -> CinematicPlayer {
        let mut player = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::ApplyRemainingSemantics),
            SYNTHETIC_PLAYER,
        );
        player.start().unwrap();
        if ticks > 0 {
            player.advance(ticks).unwrap();
        }
        player.request_skip().unwrap();
        assert_eq!(player.state().label(), "skip_requested");
        player.settle_skip().unwrap();
        player
    }

    #[test]
    fn accept_f40_a_skip_at_start_midpoint_and_final_frame_matches_full_playback() {
        let reference = played();
        assert_eq!(
            reference.state(),
            &CinematicState::Completed(CompletionKind::Played)
        );
        assert_eq!(reference.applied().len(), 3);
        for at in [
            0,
            SYNTHETIC_DURATION_TICKS / 2,
            SYNTHETIC_DURATION_TICKS - 1,
        ] {
            let skipped = skipped_after(at);
            assert_eq!(
                skipped.state(),
                &CinematicState::Completed(CompletionKind::Skipped),
                "skip at {at}"
            );
            assert_eq!(skipped.applied(), reference.applied(), "skip at {at}");
        }
    }

    #[test]
    fn accept_f40_a_each_action_applies_exactly_once_even_when_skip_is_repeated() {
        let mut player = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::ApplyRemainingSemantics),
            SYNTHETIC_PLAYER,
        );
        player.start().unwrap();
        player.advance(50).unwrap();
        assert_eq!(player.applied().len(), 2);
        player.request_skip().unwrap();
        player.request_skip().unwrap();
        player.settle_skip().unwrap();
        assert_eq!(player.applied().len(), 3);
        assert!(player.settle_skip().is_err());
        assert!(player.request_skip().is_err());
        assert_eq!(player.applied().len(), 3);
    }

    #[test]
    fn accept_f40_a_unskippable_scene_refuses_skip_and_keeps_playing() {
        let mut player = CinematicPlayer::new(
            synthetic_script(false, FailureRecovery::Block),
            SYNTHETIC_PLAYER,
        );
        player.start().unwrap();
        assert_eq!(player.request_skip(), Err(PlayerError::NotSkippable));
        assert_eq!(player.state(), &CinematicState::Playing { elapsed: 0 });
    }

    #[test]
    fn accept_f40_a_cancel_applies_nothing_further_unlike_skip() {
        let mut player = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::ApplyRemainingSemantics),
            SYNTHETIC_PLAYER,
        );
        player.start().unwrap();
        player.cancel().unwrap();
        assert_eq!(player.state(), &CinematicState::Canceled);
        assert_eq!(player.applied().len(), 1);
        assert!(!player.semantic_end_reached());
    }

    #[test]
    fn accept_f40_a_media_failure_is_never_a_completion() {
        let failure = MediaFailure::MissingMedia {
            media: synthetic_cinematic_id(),
        };
        let mut recovering = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::ApplyRemainingSemantics),
            SYNTHETIC_PLAYER,
        );
        recovering.start().unwrap();
        recovering.media_failed(failure.clone()).unwrap();
        assert_eq!(recovering.state().label(), "failed");
        assert!(recovering.semantic_end_reached());
        assert_eq!(recovering.applied(), played().applied());
        assert!(recovering.advance(1).is_err());

        let mut blocking = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::Block),
            SYNTHETIC_PLAYER,
        );
        blocking.start().unwrap();
        blocking.media_failed(failure).unwrap();
        assert!(!blocking.semantic_end_reached());
        assert_eq!(blocking.applied().len(), 1);
        assert!(blocking.cancel().is_err());
    }

    #[test]
    fn accept_f40_a_control_returns_to_the_aircraft_flown_when_the_scene_ends() {
        let mut player = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::ApplyRemainingSemantics),
            SYNTHETIC_PLAYER,
        );
        player.start().unwrap();
        player.advance(40).unwrap();
        player.set_player_actor(SYNTHETIC_SWAPPED_PLAYER);
        player.request_skip().unwrap();
        player.settle_skip().unwrap();
        assert_eq!(
            player.applied().last().unwrap().effect,
            AppliedEffect::ControlReturned(SYNTHETIC_SWAPPED_PLAYER)
        );
    }

    #[test]
    fn accept_f40_a_script_validation_refuses_bad_scripts() {
        let id = synthetic_cinematic_id();
        let build = |duration, actions| {
            CinematicScript::try_new(
                id.clone(),
                duration,
                true,
                PausePolicy::SimulationRuns,
                MasterClock::Simulation,
                FailureRecovery::Block,
                actions,
            )
        };
        let action = |n, at_tick, effect| SemanticAction {
            id: SemanticActionId(n),
            at_tick,
            effect,
        };
        assert_eq!(build(0, vec![]), Err(ScriptError::ZeroDuration));
        assert_eq!(
            build(
                10,
                vec![
                    action(1, 0, SemanticEffect::ReturnControlToPlayer),
                    action(1, 5, SemanticEffect::ReturnControlToPlayer),
                ]
            ),
            Err(ScriptError::DuplicateAction(SemanticActionId(1)))
        );
        assert!(matches!(
            build(
                10,
                vec![action(1, 11, SemanticEffect::ReturnControlToPlayer)]
            ),
            Err(ScriptError::ActionAfterEnd { .. })
        ));
        assert!(matches!(
            build(
                10,
                vec![action(1, 1, SemanticEffect::ObjectiveEvent(id.clone()))]
            ),
            Err(ScriptError::NotAnObjective(_))
        ));
    }

    #[test]
    fn accept_f40_a_calls_in_the_wrong_state_are_refused() {
        let mut player = CinematicPlayer::new(
            synthetic_script(true, FailureRecovery::Block),
            SYNTHETIC_PLAYER,
        );
        assert!(player.advance(1).is_err());
        assert!(player.request_skip().is_err());
        player.start().unwrap();
        assert!(player.start().is_err());
        assert_eq!(player.script().pause(), PausePolicy::SimulationPaused);
        assert_eq!(player.script().clock(), MasterClock::Audio);
    }
}
