//! Scrapbook records: latest/best, achievement facts and the cabin memento
//! choice (F47-A).
//!
//! Spec: `specs/F47-scrapbook-records-mementos-and-mission-replay.md`, stage
//! `### F47-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`ScrapbookRecords`] is the persistent half of the scrapbook. It holds three
//! things that deliberately do not share a transaction with campaign
//! progression ([`crate::campaign::CampaignState`]):
//!
//! * a [`RecordBook`] of distinct **best** and **latest** runs per subject.
//!   A better replay raises best; a worse replay replaces latest only; a
//!   better score never erases a newer latest run.
//! * an [`AchievementLedger`] of [`Fact`]s (a mission succeeded, a stunt was
//!   completed, ...). Unlock predicates in `cs_content::scrapbook` read these,
//!   so one stunt photo unlocks from one stunt fact and nothing else.
//! * a [`MementoSelection`] — the cabin memento choice, validated against the
//!   unlocked set and otherwise independent of campaign progress.
//!
//! Every submission carries its [`OutcomeId`] and is applied at most once, so
//! a result replayed after a crash cannot move a record twice.
//!
//! # Designed policies
//!
//! Which direction is "better" for a score, whether records are kept per
//! difficulty and how ties resolve in the original are **unmeasured**. The
//! engine's designed policy is explicit in [`RecordRule`]: the direction and
//! the difficulty scope are declared per subject, and a tie never replaces the
//! earlier best (the first run to reach a score keeps it) while it does
//! replace latest. See `docs/findings/2026-10-01-f47-a-scrapbook-records.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::ContentId;

use crate::campaign::{DifficultyId, OutcomeId};

/// Which direction of a score is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BetterIs {
    /// A larger score beats a smaller one (points, kills).
    Higher,
    /// A smaller score beats a larger one (times).
    Lower,
}

/// Whether a record is shared by every difficulty or kept per difficulty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifficultyScope {
    /// One record regardless of difficulty.
    AllDifficulties,
    /// A separate record for each difficulty.
    PerDifficulty,
}

/// How one subject's records are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordRule {
    /// The score direction.
    pub better: BetterIs,
    /// The difficulty scope.
    pub scope: DifficultyScope,
}

/// The key a record is stored under.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordKey {
    /// The mission, stunt or ace the record belongs to.
    pub subject: ContentId,
    /// The difficulty, present exactly when the rule is per difficulty.
    pub difficulty: Option<DifficultyId>,
}

/// One recorded run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRecord {
    /// The outcome transaction that produced it.
    pub outcome: OutcomeId,
    /// The score.
    pub score: u64,
}

/// A subject's best and latest runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The best run so far.
    pub best: RunRecord,
    /// The most recent run.
    pub latest: RunRecord,
}

/// What a submission did to the best record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BestChange {
    /// The first run for the key.
    First,
    /// The score beat the previous best.
    Improved {
        /// The score it replaced.
        previous: u64,
    },
    /// The score equalled the best; the earlier run keeps it.
    Tied,
    /// The score did not beat the best.
    NotBetter,
}

/// What [`RecordBook::submit`] answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordReceipt {
    /// The run was applied.
    Applied {
        /// The key it was stored under.
        key: RecordKey,
        /// What happened to best.
        best: BestChange,
    },
    /// This [`OutcomeId`] was already applied; nothing changed.
    AlreadyApplied,
}

/// Why a submission was refused; the book is untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// The same subject was submitted under a rule that differs from the
    /// one it was first kept under.
    RuleChanged {
        /// The subject.
        subject: ContentId,
    },
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuleChanged { subject } => {
                write!(f, "{subject} was first recorded under a different rule")
            }
        }
    }
}

impl std::error::Error for RecordError {}

/// Best/latest records and the outcome dedup ledger.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordBook {
    slots: BTreeMap<RecordKey, Slot>,
    rules: BTreeMap<ContentId, RecordRule>,
    applied: BTreeSet<(OutcomeId, ContentId)>,
}

impl RecordBook {
    /// An empty book.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one run of `subject`.
    ///
    /// # Errors
    ///
    /// [`RecordError`]; the book is unchanged.
    pub fn submit(
        &mut self,
        outcome: &OutcomeId,
        subject: &ContentId,
        difficulty: &DifficultyId,
        score: u64,
        rule: RecordRule,
    ) -> Result<RecordReceipt, RecordError> {
        if self.rules.get(subject).is_some_and(|seen| *seen != rule) {
            return Err(RecordError::RuleChanged {
                subject: subject.clone(),
            });
        }
        let ledger_key = (outcome.clone(), subject.clone());
        if self.applied.contains(&ledger_key) {
            return Ok(RecordReceipt::AlreadyApplied);
        }
        let key = RecordKey {
            subject: subject.clone(),
            difficulty: match rule.scope {
                DifficultyScope::PerDifficulty => Some(difficulty.clone()),
                DifficultyScope::AllDifficulties => None,
            },
        };
        let run = RunRecord {
            outcome: outcome.clone(),
            score,
        };
        let change = match self.slots.get_mut(&key) {
            None => {
                self.slots.insert(
                    key.clone(),
                    Slot {
                        best: run.clone(),
                        latest: run,
                    },
                );
                BestChange::First
            }
            Some(slot) => {
                let change = match (score.cmp(&slot.best.score), rule.better) {
                    (std::cmp::Ordering::Equal, _) => BestChange::Tied,
                    (std::cmp::Ordering::Greater, BetterIs::Higher)
                    | (std::cmp::Ordering::Less, BetterIs::Lower) => BestChange::Improved {
                        previous: slot.best.score,
                    },
                    _ => BestChange::NotBetter,
                };
                if matches!(change, BestChange::Improved { .. }) {
                    slot.best = run.clone();
                }
                slot.latest = run;
                change
            }
        };
        self.rules.insert(subject.clone(), rule);
        self.applied.insert(ledger_key);
        Ok(RecordReceipt::Applied { key, best: change })
    }

    /// The slot stored under `key`.
    #[must_use]
    pub fn slot(&self, key: &RecordKey) -> Option<&Slot> {
        self.slots.get(key)
    }

    /// How many keys hold a record.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no record exists.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

/// What kind of thing happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FactKind {
    /// A mission ended `Succeeded`.
    MissionSucceeded,
    /// A stunt was completed.
    StuntCompleted,
    /// An ace was defeated.
    AceDefeated,
}

/// One achievement: a kind and the content it is about.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fact {
    /// What happened.
    pub kind: FactKind,
    /// To which mission, stunt or ace.
    pub subject: ContentId,
}

/// The set of achievements, only ever growing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AchievementLedger {
    facts: BTreeSet<Fact>,
}

impl AchievementLedger {
    /// Notes `fact`; returns whether it was new.
    #[must_use]
    pub fn note(&mut self, fact: Fact) -> bool {
        self.facts.insert(fact)
    }

    /// Whether `fact` has happened.
    #[must_use]
    pub fn has(&self, fact: &Fact) -> bool {
        self.facts.contains(fact)
    }

    /// How many facts are noted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Whether no fact is noted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

/// Why a memento choice was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MementoError {
    /// The memento is not in the unlocked set.
    Locked {
        /// The refused memento.
        memento: ContentId,
    },
}

impl fmt::Display for MementoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked { memento } => write!(f, "memento {memento} is not unlocked"),
        }
    }
}

impl std::error::Error for MementoError {}

/// The cabin memento choice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MementoSelection {
    chosen: Option<ContentId>,
}

impl MementoSelection {
    /// Chooses `memento`, which must be in `unlocked`.
    ///
    /// # Errors
    ///
    /// [`MementoError::Locked`]; the choice is unchanged.
    pub fn choose(
        &mut self,
        memento: &ContentId,
        unlocked: &BTreeSet<ContentId>,
    ) -> Result<(), MementoError> {
        if !unlocked.contains(memento) {
            return Err(MementoError::Locked {
                memento: memento.clone(),
            });
        }
        self.chosen = Some(memento.clone());
        Ok(())
    }

    /// Clears the choice (an empty cabin).
    pub fn clear(&mut self) {
        self.chosen = None;
    }

    /// The chosen memento.
    #[must_use]
    pub fn chosen(&self) -> Option<&ContentId> {
        self.chosen.as_ref()
    }
}

/// The scrapbook's persistent state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrapbookRecords {
    /// Best/latest runs.
    pub records: RecordBook,
    /// Achievement facts.
    pub facts: AchievementLedger,
    /// The cabin memento choice.
    pub memento: MementoSelection,
}
