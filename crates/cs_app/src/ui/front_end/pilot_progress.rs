//! The pilot's progress across the start flow's process images (Rally #1973,
//! part of #1747).
//!
//! The start flow is a chain of processes (front end → mission → front end,
//! winit allows one event loop per process), so nothing in memory survives
//! the return to the cabin. What does is the profile store: the pilot is the
//! population's persisted *active* pointer, and the mission record lives in
//! the pilot's own document as named counters.
//!
//! * [`chain_profile_base`] is the one directory every leg of a smoke chain
//!   opens, whatever subdirectory of the capture directory the leg writes to.
//! * [`open_session`] opens that population: a smoke gets the synthetic
//!   population under the chain's directory (the library refuses automation
//!   the production tree), a person the production one in `$CS_PROFILE_DIR`,
//!   else `~/.crimson-skies`.
//! * [`record_mission_end`] is the mission leg's write when the player leaves
//!   for the cabin; [`progress_of`] is the front end's read after the return.
//!
//! The intro gate (#2077) reads the same counters: [`intro_chapter`] feeds the
//! engine's callback 2151 rule ([`chapter_gate`]) from them. The original
//! writes the gate's record word `+0x04` (the mission's elapsed time) in its
//! end-of-mission handler for any single-player ending, so a mission with an
//! attempt counted here stands for a record with that word set; the second
//! word (`+0x58`) has no ordinary writer and stays 0
//! (`docs/findings/2026-10-25-intro-gate-flag-writer-static.md`).
//!
//! **Our own layout, not the original's.** The original keeps this in
//! `Status.dat` / `Mission.*`, whose layouts are unknown (F64 owner capture,
//! #746; `2026-10-22-parity5-start-flow-transitions.md`). The counters below
//! are this reimplementation's profile fields and make no claim about the
//! original's bytes. Nothing here is `verified_original`.

use std::path::{Path, PathBuf};

use cs_content::save::settings::SettingCatalog;
use cs_types::profile::{ProfileKind, RecordEntry};

use super::intro::{
    CAMPAIGN_MISSIONS, MISSIONS_PER_CHAPTER, MissionFlags, chapter_gate, chapter_of_mission,
};
use super::start_flow::{
    FRONT_END_SMOKE_SUBDIR, MISSION_SMOKE_SUBDIR, MissionEnd, REPLAY_SMOKE_SUBDIR,
    RESUMED_SMOKE_SUBDIR,
};
use super::window::{PROFILE_DIR_ENV, SMOKE_PROFILE_SUBDIR};
use crate::profile::{ChangeRefusal, ProfileSession, SessionOrigin};

/// The record holding [`Attempt::code`] of the pilot's latest attempt.
const LAST_ATTEMPT_KEY: &str = "pilot.last_attempt";

/// How a mission attempt ended, as the profile counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attempt {
    /// The flight failed and the player returned to the cabin.
    Failed,
    /// The player quit from the Esc pause map.
    Abandoned,
    /// The mission was completed. The Mission Succeeded return (#1972) is the
    /// caller; the counter is here so the completed count has one home.
    Completed,
}

impl Attempt {
    /// The record suffix this ending counts under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
            Self::Completed => "completed",
        }
    }

    /// The numeric code `pilot.last_attempt` holds (0 is "never attempted").
    #[must_use]
    pub const fn code(self) -> u64 {
        match self {
            Self::Failed => 1,
            Self::Abandoned => 2,
            Self::Completed => 3,
        }
    }

    /// The attempt a [`MissionEnd`] counts as.
    #[must_use]
    pub const fn of_end(end: MissionEnd) -> Self {
        match end {
            MissionEnd::ReturnedToCabin => Self::Failed,
            MissionEnd::Abandoned => Self::Abandoned,
        }
    }

    fn from_code(code: u64) -> Option<Self> {
        [Self::Failed, Self::Abandoned, Self::Completed]
            .into_iter()
            .find(|attempt| attempt.code() == code)
    }
}

/// What the pilot's document says about the missions flown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PilotProgress {
    /// The pilot's name as stored.
    pub name: String,
    /// Missions completed at least once.
    pub completed: usize,
    /// Attempts at any mission, however they ended.
    pub attempts: u64,
    /// How the most recent attempt, at any mission, ended.
    pub last: Option<Attempt>,
}

/// Why a mission end could not be written.
#[derive(Debug)]
pub enum ProgressError {
    /// No profile directory could be named (`$HOME` unset, no `$CS_PROFILE_DIR`).
    NoBase,
    /// The population could not be opened.
    Open(String),
    /// No pilot is selected, so there is nobody to credit.
    NoPilot,
    /// The write was refused or failed.
    Write(String),
}

impl std::fmt::Display for ProgressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBase => write!(f, "no profile directory (set {PROFILE_DIR_ENV} or HOME)"),
            Self::Open(reason) => write!(f, "the pilot population cannot open: {reason}"),
            Self::NoPilot => write!(f, "no pilot is selected"),
            Self::Write(reason) => write!(f, "the mission record cannot be written: {reason}"),
        }
    }
}

impl std::error::Error for ProgressError {}

/// The directory a smoke chain keeps its pilots in. The chain writes its
/// screens under `front-end/`, `mission/`, `resumed/` and `replay/` of one
/// capture directory; the pilots live under the first leg's directory so
/// every leg opens the same population. Any other capture directory (a
/// `--front-end` smoke, a typed one) keeps them under itself.
#[must_use]
pub fn chain_profile_base(capture_dir: &Path) -> PathBuf {
    let leg = capture_dir.file_name().and_then(|name| name.to_str());
    match (leg, capture_dir.parent()) {
        (Some(RESUMED_SMOKE_SUBDIR | MISSION_SMOKE_SUBDIR | REPLAY_SMOKE_SUBDIR), Some(root)) => {
            root.join(FRONT_END_SMOKE_SUBDIR).join(SMOKE_PROFILE_SUBDIR)
        }
        _ => capture_dir.join(SMOKE_PROFILE_SUBDIR),
    }
}

/// Opens the pilot population for a run: the chain's synthetic one when
/// `smoke_dir` (the run's capture directory) is given, else the production one.
///
/// # Errors
///
/// [`ProgressError::NoBase`] without a place to keep pilots,
/// [`ProgressError::Open`] when the population cannot open.
pub fn open_session(smoke_dir: Option<&Path>) -> Result<ProfileSession, ProgressError> {
    let catalog = SettingCatalog::empty();
    let opened = match smoke_dir {
        Some(dir) => ProfileSession::open_sandbox(&chain_profile_base(dir), &catalog),
        None => {
            let base = std::env::var_os(PROFILE_DIR_ENV)
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".crimson-skies"))
                })
                .ok_or(ProgressError::NoBase)?;
            ProfileSession::open(
                &base,
                SessionOrigin::Interactive,
                ProfileKind::Production,
                &catalog,
            )
        }
    };
    opened.map_err(|error| ProgressError::Open(error.to_string()))
}

fn counter_key(mission: &str, suffix: &str) -> String {
    format!("mission.{mission}.{suffix}")
}

fn value_of(records: &[RecordEntry], key: &str) -> u64 {
    records
        .iter()
        .find(|entry| entry.key == key)
        .map_or(0, |entry| entry.value)
}

fn set(records: &mut Vec<RecordEntry>, key: &str, value: u64) {
    match records.iter_mut().find(|entry| entry.key == key) {
        Some(entry) => entry.value = value,
        None => records.push(RecordEntry {
            key: key.to_owned(),
            value,
        }),
    }
}

/// Counts one attempt at `mission` against the selected pilot and commits it
/// as one whole revision: `mission.<id>.attempts`, `mission.<id>.<ending>`
/// and `pilot.last_attempt`.
///
/// # Errors
///
/// [`ProgressError::NoPilot`] without a selected pilot,
/// [`ProgressError::Write`] when the label is not a usable key or the commit
/// fails.
pub fn record_attempt(
    session: &mut ProfileSession,
    mission: &str,
    attempt: Attempt,
) -> Result<(), ProgressError> {
    if session.selected().is_none() {
        return Err(ProgressError::NoPilot);
    }
    let attempts = counter_key(mission, "attempts");
    let ending = counter_key(mission, attempt.key());
    let last = LAST_ATTEMPT_KEY.to_owned();
    for key in [&attempts, &ending] {
        cs_types::profile::validate_key("mission record", key)
            .map_err(|reason| ProgressError::Write(reason.to_string()))?;
    }
    session
        .commit_with(|document| {
            let held = |key: &str| value_of(&document.records, key);
            let (tried, ended) = (held(&attempts) + 1, held(&ending) + 1);
            set(&mut document.records, &attempts, tried);
            set(&mut document.records, &ending, ended);
            set(&mut document.records, &last, attempt.code());
            Ok::<(), ChangeRefusal>(())
        })
        .map(|_| ())
        .map_err(|error| ProgressError::Write(error.to_string()))
}

/// The mission leg's write when the player leaves the flight for the cabin:
/// opens the chain's population, credits the selected pilot and releases it.
///
/// # Errors
///
/// As [`open_session`] and [`record_attempt`].
pub fn record_mission_end(
    smoke_dir: Option<&Path>,
    mission: &str,
    end: MissionEnd,
) -> Result<(), ProgressError> {
    let mut session = open_session(smoke_dir)?;
    record_attempt(&mut session, mission, Attempt::of_end(end))
}

/// What the selected pilot's document says about the missions flown, or
/// `None` without a selected pilot.
#[must_use]
pub fn progress_of(session: &ProfileSession) -> Option<PilotProgress> {
    let document = session.document()?;
    let records = &document.records;
    let mut completed = 0;
    let mut attempts = 0;
    for entry in records {
        let Some(rest) = entry.key.strip_prefix("mission.") else {
            continue;
        };
        match rest.rsplit_once('.').map(|(_, field)| field) {
            Some("completed") if entry.value > 0 => completed += 1,
            Some("attempts") => attempts += entry.value,
            _ => {}
        }
    }
    let last = Attempt::from_code(value_of(records, LAST_ATTEMPT_KEY));
    Some(PilotProgress {
        name: document.display_name.clone(),
        completed,
        attempts,
        last,
    })
}

/// The label the profile keeps campaign mission `index` (0-based, in
/// `cm_sequence.zrd` order) under: `M01` for the first.
#[must_use]
pub fn mission_label(index: usize) -> String {
    format!("M{:02}", index + 1)
}

/// The pilot's current campaign mission: the first mission not yet completed
/// (`CAMPAIGN_MISSIONS` once all are). This build's reading of the engine's
/// current mission, which advances past a passed mission; the original's
/// advance rule is not traced.
#[must_use]
pub fn current_mission(records: &[RecordEntry]) -> usize {
    (0..CAMPAIGN_MISSIONS)
        .find(|&index| value_of(records, &counter_key(&mission_label(index), "completed")) == 0)
        .unwrap_or(CAMPAIGN_MISSIONS)
}

/// The two words callback 2151 reads from the first mission record of
/// `chapter`, as the pilot's counters stand: `word_04` is non-zero once the
/// mission has an attempt on record, `word_58` is always 0.
#[must_use]
pub fn gate_flags(records: &[RecordEntry], chapter: u32) -> MissionFlags {
    let first = usize::try_from(chapter)
        .ok()
        .and_then(|chapter| chapter.checked_sub(1))
        .map(|before| before * MISSIONS_PER_CHAPTER);
    let attempted = first.is_some_and(|index| {
        value_of(records, &counter_key(&mission_label(index), "attempts")) > 0
    });
    MissionFlags {
        word_04: u32::from(attempted),
        word_58: 0,
    }
}

/// Callback 2151's result for the selected pilot: the chapter whose intro the
/// cabin plays, or `0` for none. `None` without a selected pilot.
#[must_use]
pub fn intro_chapter(session: &ProfileSession) -> Option<u32> {
    let records = &session.document()?.records;
    let chapter = chapter_of_mission(current_mission(records));
    Some(chapter_gate(chapter, gate_flags(records, chapter)))
}
