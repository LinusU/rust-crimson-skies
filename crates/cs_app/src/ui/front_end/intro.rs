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
//! * **Once per pilot.** The gate is the engine callback `2151`, whose value
//!   domain is unread (the finding's "Unknown"). So the intro plays on every
//!   entry from the pilot page and nothing is persisted per profile; this is
//!   the measured script rule, not a "first time only" rule.
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
