//! The intro movie between the pilot page and the cabin (PARITY5-CABIN-INTRO.03).
//!
//! Spec: `docs/findings/2026-10-21-parity5-cabin-intro-measurement.md`.
//!
//! The original's `PASSENGERCABIN` script plays `CAMPAIGNINTRO` (the movie
//! `chap1.mpg`) when the cabin is entered **from the Campaign page**, and not
//! on a return from Plane Name, Construction or the Scrapbook. Esc, Space,
//! Return or a left press skip it; the natural end reaches the same handler.
//! Either way the cabin follows.
//!
//! # What is **not** claimed
//!
//! * **Once per pilot, as a profile rule.** The gate is the engine callback
//!   `2151`, read statically in
//!   `docs/findings/2026-10-24-intro-callback-2151-static.md`: [`chapter_gate`]
//!   is its measured rule. The campaign progress it reads (the current
//!   mission's chapter and the two flags of the chapter's first mission
//!   record) has no writer in this build, so every pilot is still a fresh
//!   one and the intro plays on every entry from the pilot page; nothing is
//!   persisted per profile until the progress exists.
//! * **The menu music during the movie** is stopped while it plays and
//!   restarted after; the original's handling is unread.
//! * A movie that cannot be opened or fails part-way is recorded in
//!   [`IntroState::outcomes`] and the cabin is entered; nothing is faked.

use std::path::PathBuf;

use bevy::ecs::message::MessageCursor;
use bevy::prelude::{Messages, World};
use cs_types::content::ContentId;

use super::Screen;
use super::movie::movie_media;
use super::retail::MOVIE_DIRECTORY;
use super::sound::MenuAudio;
use super::start_flow::StartStep;
use crate::cinematics::movie_player::{
    AudioOutput, MovieFinished, MovieOutcome, MoviePlayer, MoviePresenter,
};

/// The measured intro movie, under [`MOVIE_DIRECTORY`].
pub const INTRO_FILE: &str = "chap1.mpg";

/// The engine callback that gates the intro (`callback($$E$$, 2151, FC)`).
pub const GATE_CALLBACK: u32 = 2151;

/// Missions per chapter in the callback's record arithmetic: it reads the
/// record `5 * (chapter - 1)`, the chapter's first mission.
pub const MISSIONS_PER_CHAPTER: usize = 5;

/// The two words of a mission record callback 2151 reads (record offsets
/// `0x04` and `0x58`); the engine sets both to 1 on records it advances past.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MissionFlags {
    /// Record offset `0x04`.
    pub word_04: u32,
    /// Record offset `0x58`.
    pub word_58: u32,
}

/// The record index callback 2151 reads for `chapter` (1-based); `None` for
/// chapter 0, where the callback returns 0 without a record.
#[must_use]
pub fn gate_record(chapter: u32) -> Option<usize> {
    let chapter = usize::try_from(chapter).ok()?;
    chapter
        .checked_sub(1)
        .map(|before| before * MISSIONS_PER_CHAPTER)
}

/// Callback 2151's result `FC`: the chapter of the pilot's current mission
/// when both flag words of that chapter's first mission record are zero, and
/// `0` otherwise. The cabin script runs the intro only when it is non-zero,
/// and names the movie `chap<FC>.mpg`.
#[must_use]
pub fn chapter_gate(chapter: u32, first_mission: MissionFlags) -> u32 {
    if first_mission.word_04 != 0 || first_mission.word_58 != 0 {
        0
    } else {
        chapter
    }
}

/// Where the intro movie is and where its sound goes.
#[derive(Clone, Debug)]
pub struct IntroMovie {
    /// The program stream's path.
    pub path: PathBuf,
    /// Its sound output.
    pub output: AudioOutput,
}

impl IntroMovie {
    /// The retail intro under the installation at `cs_path`.
    #[must_use]
    pub fn retail(cs_path: &std::path::Path, output: AudioOutput) -> Self {
        Self {
            path: cs_path.join(MOVIE_DIRECTORY).join(INTRO_FILE),
            output,
        }
    }
}

/// Whether `step` is the pilot page handing over to the cabin, the one entry
/// that plays the intro.
#[must_use]
pub fn follows(step: &StartStep) -> bool {
    matches!(
        step,
        StartStep::Moved {
            from: Screen::ProfileSelect,
            to: Screen::Cabin
        }
    )
}

/// Where the intro is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    /// The cabin was entered this frame; the movie opens on the next one so
    /// the press that confirmed the pilot cannot skip it.
    Pending,
    Playing,
}

/// The intro's live state inside the shell resource.
pub struct IntroState {
    movie: Option<IntroMovie>,
    phase: Phase,
    /// How each intro ended, in order.
    pub outcomes: Vec<MovieOutcome>,
    finished: MessageCursor<MovieFinished>,
}

impl IntroState {
    pub(super) fn new(movie: Option<IntroMovie>) -> Self {
        Self {
            movie,
            phase: Phase::Idle,
            outcomes: Vec::new(),
            finished: MessageCursor::default(),
        }
    }

    /// Whether the movie is up (or about to be): the shell takes no input.
    #[must_use]
    pub fn active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Whether this run plays an intro at all.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.movie.is_some()
    }

    /// The pilot page confirmed: the intro starts on the next frame. `false`
    /// when this run has none.
    pub(super) fn begin(&mut self, audio: Option<&mut MenuAudio>) -> bool {
        if self.movie.is_none() || self.phase != Phase::Idle {
            return false;
        }
        self.phase = Phase::Pending;
        if let Some(audio) = audio {
            audio.stop_music();
        }
        true
    }

    /// Advances the intro one frame; `true` while the frame's input belongs
    /// to it (including the frame it ends on).
    pub(super) fn advance(&mut self, world: &mut World) -> bool {
        match self.phase {
            Phase::Idle => false,
            Phase::Pending => {
                self.open(world);
                true
            }
            Phase::Playing => {
                let done: Vec<MovieOutcome> = world
                    .get_resource::<Messages<MovieFinished>>()
                    .map(|messages| {
                        self.finished
                            .read(messages)
                            .map(|finished| finished.outcome.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(outcome) = done.into_iter().next() {
                    if let MovieOutcome::Failed(failure) = &outcome {
                        eprintln!("cs: the intro movie stopped: {failure:?}");
                    }
                    self.outcomes.push(outcome);
                    self.phase = Phase::Idle;
                }
                true
            }
        }
    }

    fn open(&mut self, world: &mut World) {
        self.phase = Phase::Idle;
        let Some(movie) = &self.movie else { return };
        let media: ContentId = movie_media(INTRO_FILE);
        let opened = MoviePlayer::open(&movie.path, &media, &movie.output);
        // Messages written before this start belong to earlier movies.
        if let Some(messages) = world.get_resource::<Messages<MovieFinished>>() {
            self.finished.clear(messages);
        }
        match (opened, world.get_resource_mut::<MoviePresenter>()) {
            (Ok(player), Some(mut presenter)) => {
                presenter.start(player);
                self.phase = Phase::Playing;
            }
            (Err(failure), _) => {
                eprintln!("cs: the intro movie cannot play: {failure:?}");
                self.outcomes.push(MovieOutcome::Failed(failure));
            }
            (Ok(_), None) => eprintln!("cs: no movie presenter is installed; the intro is skipped"),
        }
    }
}
