//! What the engine does with an objective block's `INACTIVE<n>` stage conditions,
//! its `INACTIVE_COMPLETION_COUNT` and its terminal outcome keys.
//!
//! M01-B-OBJ-3 measured these from the executable image (read-only static
//! analysis; nothing was run). `docs/findings/2026-10-12-m01-b-obj-3-stage-conditions-and-outcomes.md`
//! holds the addresses and the evidence; this module is the behavior, kept pure
//! (no Bevy, no I/O, no game state) so the objective recovery and a future
//! runtime consumer share one reading.
//!
//! # The reading, in short
//!
//! * The loader reads the keys `INACTIVE1` … `INACTIVE100` by formatting
//!   `INACTIVE%d`, so only the canonical spelling of a number in `1..=100` is
//!   ever read, gaps between numbers are skipped and the first occurrence of a
//!   spelling wins ([`ENGINE_MAX_STAGE`], [`StageConditions::read`]).
//! * Each condition is a chain of names: the first names an entry in the
//!   engine's class-node registry and every later one names a node found
//!   depth-first below the previous (a node also matches its own name). A chain
//!   that does not resolve is **dropped silently**, so it counts neither as a
//!   condition nor towards the threshold ([`NodeState::Unresolved`]).
//! * `INACTIVE_COMPLETION_COUNT` is a signed threshold, defaulting to the number
//!   of conditions that resolved. The condition set is satisfied when the number
//!   of resolved nodes whose *active* bit is clear reaches the threshold, and
//!   **never** when no condition resolved ([`StageConditions::is_satisfied`]).
//! * `INSTANTWIN` and `INSTANTLOSS` are read by presence; `WON` and `LOST` are
//!   read the same way after them and override them
//!   ([`ObjectiveOutcome::of_keys`]). A completed `INSTANTWIN` sets the mission's
//!   won flag, an `INSTANTLOSS` its lost flag, and after every pass the lost
//!   flag is tested first ([`MissionEndFlags::end`]).
//!
//! # What is not decided here
//!
//! Which gameplay event clears a node's active bit, and whether a given name
//! resolves against the loaded airframe records, are not measured: see the
//! finding's unknowns. [`NodeState`] is therefore an input the caller supplies,
//! never something this module guesses.

use crate::objectives::{OBJECTIVE_INACTIVE_COUNT_KEY, OBJECTIVE_INACTIVE_STAGE_PREFIX};
use crate::stunts::ZrdValue;

/// The highest `INACTIVE<n>` number the engine's loader tries (inclusive).
pub const ENGINE_MAX_STAGE: u32 = 100;

/// The spellings of the outcome keys the engine's loader looks up.
pub mod keys {
    /// Sets outcome 3.
    pub const INSTANT_WIN: &str = "INSTANTWIN";
    /// Sets outcome 4.
    pub const INSTANT_LOSS: &str = "INSTANTLOSS";
    /// Sets outcome 2. No retail record spells it.
    pub const WON: &str = "WON";
    /// Sets outcome 1. No retail record spells it.
    pub const LOST: &str = "LOST";
}

/// One `INACTIVE<n>` condition: the names the engine resolves in order.
///
/// The first element names a class node; each following element names a node
/// found below the one before. The original records one to three names; the
/// engine reads any number of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagePath {
    elements: Vec<String>,
}

impl StagePath {
    /// Reads a condition's value as the engine does: a list of one or more texts.
    /// Anything else is not a shape the loader reads (an integer element would be
    /// dereferenced as a string), so it is `None`.
    #[must_use]
    pub fn from_record(value: &ZrdValue) -> Option<Self> {
        let children = value.as_list()?;
        if children.is_empty() {
            return None;
        }
        children
            .iter()
            .map(|child| child.as_text().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .map(|elements| Self { elements })
    }

    /// Every name, in resolution order. Never empty.
    #[must_use]
    pub fn elements(&self) -> &[String] {
        &self.elements
    }

    /// The name looked up in the class-node registry.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.elements[0]
    }
}

/// What a caller knows about one condition's node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeState {
    /// The chain does not resolve to a node; the engine drops the condition.
    Unresolved,
    /// The node resolves and its active bit is set.
    Active,
    /// The node resolves and its active bit is clear.
    Inactive,
}

/// An objective block's stage conditions and threshold, as the loader reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StageConditions {
    stages: Vec<(u32, StagePath)>,
    threshold: Option<i32>,
}

/// Which of a block's fields [`StageConditions::read`] recovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageConditionRead {
    /// The conditions and threshold the engine would hold.
    pub conditions: StageConditions,
    /// One entry per input field, in order: whether the loader reads it.
    pub recovered: Vec<bool>,
}

/// The canonical key of stage `number`, the only spelling the loader tries.
fn canonical_stage_key(number: u32) -> String {
    format!("{OBJECTIVE_INACTIVE_STAGE_PREFIX}{number}")
}

impl StageConditions {
    /// Reads a block's flat `key, value` fields as the loader does.
    ///
    /// A field is recovered when it is a stage key in `1..=`[`ENGINE_MAX_STAGE`]
    /// in canonical spelling, is the first occurrence of that spelling and holds
    /// a list of texts, or when it is the first `INACTIVE_COMPLETION_COUNT` and
    /// holds exactly one integer that fits the engine's signed word. Everything
    /// else (`INACTIVE0`, `INACTIVE101`, `INACTIVE01`, a repeat, a malformed
    /// value) is not read by the loader the way the record spells it and stays
    /// unrecovered.
    #[must_use]
    pub fn read(fields: &[(&str, &ZrdValue)]) -> StageConditionRead {
        let mut stages: Vec<(u32, StagePath)> = Vec::new();
        let mut threshold = None;
        let mut threshold_seen = false;
        let mut seen_stage = [false; ENGINE_MAX_STAGE as usize + 1];
        let mut recovered = Vec::with_capacity(fields.len());
        for (key, value) in fields {
            let mut read = false;
            if *key == OBJECTIVE_INACTIVE_COUNT_KEY {
                if !threshold_seen {
                    threshold_seen = true;
                    if let Some([ZrdValue::Int(count)]) = value.as_list()
                        && let Ok(count) = i32::try_from(*count)
                    {
                        threshold = Some(count);
                        read = true;
                    }
                }
            } else if let Some(number) = crate::objectives::inactive_stage_number(key) {
                let in_range = (1..=ENGINE_MAX_STAGE).contains(&number);
                if in_range && *key == canonical_stage_key(number) {
                    let first = !seen_stage[number as usize];
                    seen_stage[number as usize] = true;
                    if first && let Some(path) = StagePath::from_record(value) {
                        stages.push((number, path));
                        read = true;
                    }
                }
            }
            recovered.push(read);
        }
        // The loader walks the numbers upward, whatever order the record spells them.
        stages.sort_by_key(|(number, _)| *number);
        StageConditionRead {
            conditions: Self { stages, threshold },
            recovered,
        }
    }

    /// The declared conditions in stage order, with their `n`.
    #[must_use]
    pub fn stages(&self) -> &[(u32, StagePath)] {
        &self.stages
    }

    /// The declared `INACTIVE_COMPLETION_COUNT`, or `None` when the block has none.
    #[must_use]
    pub const fn threshold(&self) -> Option<i32> {
        self.threshold
    }

    /// Whether the block declares anything the engine would test.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty() && self.threshold.is_none()
    }

    /// The threshold in force when `resolved` conditions resolved: the declared
    /// count, or `resolved` when the block declares none.
    #[must_use]
    pub fn required(&self, resolved: usize) -> i64 {
        self.threshold
            .map_or_else(|| i64::try_from(resolved).unwrap_or(i64::MAX), i64::from)
    }

    /// The engine's test: `state` reports each declared condition's node. The set
    /// is satisfied when at least [`required`](Self::required) resolved nodes are
    /// [`NodeState::Inactive`]; with no resolved condition it is never satisfied,
    /// whatever the threshold.
    pub fn is_satisfied(&self, mut state: impl FnMut(u32, &StagePath) -> NodeState) -> bool {
        let mut resolved = 0_usize;
        let mut inactive = 0_i64;
        for (number, path) in &self.stages {
            match state(*number, path) {
                NodeState::Unresolved => {}
                NodeState::Active => resolved += 1,
                NodeState::Inactive => {
                    resolved += 1;
                    inactive += 1;
                }
            }
        }
        resolved != 0 && inactive >= self.required(resolved)
    }
}

/// A block's terminal outcome key, as the engine stores it (`1..=4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObjectiveOutcome {
    /// `LOST`: counts towards the mission being lost once every such objective completes.
    Lost,
    /// `WON`: counts towards the mission being won once every such objective completes.
    Won,
    /// `INSTANTWIN`: completing it wins the mission at once.
    InstantWin,
    /// `INSTANTLOSS`: completing it loses the mission at once.
    InstantLoss,
}

impl ObjectiveOutcome {
    /// The value the engine stores in the objective record.
    #[must_use]
    pub const fn engine_value(self) -> u8 {
        match self {
            Self::Lost => 1,
            Self::Won => 2,
            Self::InstantWin => 3,
            Self::InstantLoss => 4,
        }
    }

    /// The outcome the engine ends up with for the outcome keys a block holds.
    ///
    /// The loader looks the keys up by presence (the value is not read) in the
    /// order `INSTANTWIN`, `INSTANTLOSS`, `WON`, `LOST`, and each hit overwrites
    /// the last, so the later spelling wins: `LOST` over `WON` over `INSTANTLOSS`
    /// over `INSTANTWIN`.
    #[must_use]
    pub fn of_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        keys.into_iter()
            .filter_map(|key| match key {
                keys::INSTANT_WIN => Some(Self::InstantWin),
                keys::INSTANT_LOSS => Some(Self::InstantLoss),
                keys::WON => Some(Self::Won),
                keys::LOST => Some(Self::Lost),
                _ => None,
            })
            .max_by_key(|outcome| outcome.load_order())
    }

    /// Where the loader looks the key up; a later lookup overwrites an earlier one.
    const fn load_order(self) -> u8 {
        match self {
            Self::InstantWin => 0,
            Self::InstantLoss => 1,
            Self::Won => 2,
            Self::Lost => 3,
        }
    }
}

/// The delay the engine passes with an ending reached by an instant outcome.
/// The unit is not measured (the value is a float handed to the end-of-mission
/// presentation); seconds is an inference.
pub const INSTANT_END_DELAY: f32 = 0.1;

/// The delay the engine passes with an ending reached any other way.
pub const ORDINARY_END_DELAY: f32 = 3.0;

/// How a mission ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionEnd {
    /// The won flag is set and the lost flag is not.
    Win,
    /// The lost flag is set.
    Loss,
}

/// The mission's won and lost flags and the end the engine derives from them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MissionEndFlags {
    won: bool,
    lost: bool,
    instant_win: bool,
    instant_loss: bool,
}

impl MissionEndFlags {
    /// Starts a pass: the instant markers are per pass in the engine (locals of the
    /// pass function), while the won and lost flags persist in the mission.
    pub const fn begin_pass(&mut self) {
        self.instant_win = false;
        self.instant_loss = false;
    }

    /// An objective with `outcome` has just completed.
    pub fn completed(&mut self, outcome: ObjectiveOutcome) {
        match outcome {
            ObjectiveOutcome::InstantWin => {
                self.won = true;
                self.instant_win = true;
            }
            ObjectiveOutcome::InstantLoss => {
                self.lost = true;
                self.instant_loss = true;
            }
            // WON and LOST are tallies over every objective of the category.
            ObjectiveOutcome::Won | ObjectiveOutcome::Lost => {}
        }
    }

    /// The end of a pass: `total` objectives carry `outcome` and `completed` of
    /// them have completed. When there is at least one and all have completed,
    /// the category sets its flag.
    pub fn tally(&mut self, outcome: ObjectiveOutcome, total: usize, completed: usize) {
        if total == 0 || total != completed {
            return;
        }
        match outcome {
            ObjectiveOutcome::Won => self.won = true,
            ObjectiveOutcome::Lost => self.lost = true,
            ObjectiveOutcome::InstantWin | ObjectiveOutcome::InstantLoss => {}
        }
    }

    /// The ending after a pass and the delay it carries. The lost flag is tested
    /// first, so a mission whose both flags are set is lost.
    #[must_use]
    pub const fn end(&self) -> Option<(MissionEnd, f32)> {
        if self.lost {
            Some((
                MissionEnd::Loss,
                if self.instant_loss {
                    INSTANT_END_DELAY
                } else {
                    ORDINARY_END_DELAY
                },
            ))
        } else if self.won {
            Some((
                MissionEnd::Win,
                if self.instant_win {
                    INSTANT_END_DELAY
                } else {
                    ORDINARY_END_DELAY
                },
            ))
        } else {
            None
        }
    }
}
