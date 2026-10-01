//! The declared scrapbook: entries, stable ids, unlock predicates and replay
//! links (F47-A).
//!
//! Spec: `specs/F47-scrapbook-records-mementos-and-mission-replay.md`, stage
//! `### F47-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! A [`ScrapbookCatalog`] is the content half of the scrapbook. Its runtime
//! counterpart is `cs_sim::records` (the persisted facts and records) and the
//! projection is `cs_app::ui::scrapbook`. This crate cannot depend on
//! `cs_sim`, so the predicate leaves are the declared [`UnlockFact`]; the app
//! maps the ledger onto them.
//!
//! * **Stable ids.** An entry is identified by a `scrapbook_item/...`
//!   [`ContentId`]; its title is a `string_resource/...` id resolved per
//!   locale at presentation. Order, page number and display text are never
//!   identity, so a reorder or a locale change cannot move an achievement to
//!   another mission.
//! * **Evidence-backed unlocks.** An entry's rule is a [`Resolved<Unlock>`].
//!   A known rule is a predicate over [`UnlockFact`]s; an unknown rule is an
//!   explicit unknown that never unlocks, so nothing is awarded merely
//!   because a mission succeeded.
//! * **Hidden pages.** A [`EntryVisibility::HiddenUntilUnlocked`] entry is
//!   absent from [`ScrapbookCatalog::visible`] until its rule holds.
//! * **Replay links.** A [`ReplayLink`] names the mission (and variant) the
//!   replay launches through the normal loading path.
//!
//! No original page, rule or link is recorded here; the original scrapbook
//! encoding is not decoded. Entries are authored synthetic data until F47-D
//! audits them. See `docs/findings/2026-10-01-f47-a-scrapbook-records.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Resolved};

/// The deepest accepted `All`/`Any` nesting.
pub const MAX_UNLOCK_DEPTH: usize = 8;

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// A page of the scrapbook (mission summary, article, image).
    Page,
    /// A photo awarded for a stunt.
    StuntPhoto,
    /// A kill/trophy page. Its classification is unmeasured and absent.
    KillTrophy,
    /// A cabin memento.
    Memento,
}

/// Whether an entry is shown before it unlocks.
///
/// Named `EntryVisibility` rather than `Visibility`: `cs_content::scene` and
/// `cs_sim::animated_object` already own a node-draw `Visibility` in the same
/// vocabulary, and an unqualified import of two of them must not be possible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryVisibility {
    /// Shown locked.
    Shown,
    /// Not shown until its rule is satisfied.
    HiddenUntilUnlocked,
}

/// The kind of achievement a leaf names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnlockFactKind {
    /// A mission ended `Succeeded`.
    MissionSucceeded,
    /// A stunt was completed.
    StuntCompleted,
    /// An ace was defeated.
    AceDefeated,
}

impl UnlockFactKind {
    fn subject_kind(self) -> ContentKind {
        match self {
            Self::MissionSucceeded => ContentKind::Mission,
            Self::StuntCompleted => ContentKind::Stunt,
            Self::AceDefeated => ContentKind::Pilot,
        }
    }
}

/// One achievement leaf of an unlock predicate.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnlockFact {
    /// What must have happened.
    pub kind: UnlockFactKind,
    /// To which content.
    pub subject: ContentId,
}

/// An unlock predicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unlock {
    /// Unlocked from the start.
    Always,
    /// One achievement.
    Fact(UnlockFact),
    /// Every part holds.
    All(Vec<Unlock>),
    /// At least one part holds.
    Any(Vec<Unlock>),
}

impl Unlock {
    /// Evaluates against `has`. An empty `All` is satisfied and an empty
    /// `Any` is not; validation rejects both.
    pub fn holds(&self, has: &impl Fn(&UnlockFact) -> bool) -> bool {
        match self {
            Self::Always => true,
            Self::Fact(fact) => has(fact),
            Self::All(parts) => parts.iter().all(|part| part.holds(has)),
            Self::Any(parts) => parts.iter().any(|part| part.holds(has)),
        }
    }

    fn validate(&self, entry: &ContentId, depth: usize) -> Result<(), ScrapbookError> {
        if depth > MAX_UNLOCK_DEPTH {
            return Err(ScrapbookError::UnlockTooDeep {
                entry: entry.clone(),
            });
        }
        match self {
            Self::Always => Ok(()),
            Self::Fact(fact) => {
                let expected = fact.kind.subject_kind();
                if fact.subject.kind() == expected {
                    Ok(())
                } else {
                    Err(ScrapbookError::WrongFactSubject {
                        entry: entry.clone(),
                        subject: fact.subject.clone(),
                        expected,
                    })
                }
            }
            Self::All(parts) | Self::Any(parts) => {
                if parts.is_empty() {
                    return Err(ScrapbookError::EmptyCombinator {
                        entry: entry.clone(),
                    });
                }
                parts
                    .iter()
                    .try_for_each(|part| part.validate(entry, depth + 1))
            }
        }
    }
}

/// The mission a replay launches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayLink {
    /// The mission.
    pub mission: ContentId,
    /// The mission variant, when the original distinguishes one. How the
    /// original names variants is unmeasured; the id is the mission-kind
    /// content id of the variant.
    pub variant: Option<ContentId>,
}

/// One declared scrapbook entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrapbookEntry {
    /// The stable id (`scrapbook_item/...`).
    pub id: ContentId,
    /// What it is.
    pub kind: EntryKind,
    /// The localized title (`string_resource/...`).
    pub title: ContentId,
    /// The imported artwork (`image/...`); resolved privately at runtime.
    pub image: Option<ContentId>,
    /// The unlock rule, or an explicit unknown.
    pub unlock: Resolved<Unlock>,
    /// Shown before unlocking or not.
    pub visibility: EntryVisibility,
    /// The replay it offers, if any.
    pub replay: Option<ReplayLink>,
}

/// Why a catalog was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScrapbookError {
    /// Two entries share an id.
    DuplicateEntry {
        /// The id.
        id: ContentId,
    },
    /// A content id has the wrong kind for its role.
    WrongKind {
        /// The entry.
        entry: ContentId,
        /// The role.
        role: &'static str,
        /// The kind the role needs.
        expected: ContentKind,
    },
    /// A leaf names a subject of the wrong kind.
    WrongFactSubject {
        /// The entry.
        entry: ContentId,
        /// The subject.
        subject: ContentId,
        /// The kind the fact needs.
        expected: ContentKind,
    },
    /// An `All`/`Any` has no parts.
    EmptyCombinator {
        /// The entry.
        entry: ContentId,
    },
    /// A predicate nests deeper than [`MAX_UNLOCK_DEPTH`].
    UnlockTooDeep {
        /// The entry.
        entry: ContentId,
    },
    /// A memento carries a replay link; mementos have none.
    MementoWithReplay {
        /// The entry.
        entry: ContentId,
    },
}

impl fmt::Display for ScrapbookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateEntry { id } => write!(f, "duplicate scrapbook entry {id}"),
            Self::WrongKind {
                entry,
                role,
                expected,
            } => write!(f, "{entry}: {role} must be a {} id", expected.label()),
            Self::WrongFactSubject {
                entry,
                subject,
                expected,
            } => write!(
                f,
                "{entry}: fact subject {subject} must be a {} id",
                expected.label()
            ),
            Self::EmptyCombinator { entry } => {
                write!(f, "{entry}: an all/any predicate needs a part")
            }
            Self::UnlockTooDeep { entry } => write!(
                f,
                "{entry}: unlock predicate nests deeper than {MAX_UNLOCK_DEPTH}"
            ),
            Self::MementoWithReplay { entry } => {
                write!(f, "{entry}: a memento cannot carry a replay link")
            }
        }
    }
}

impl std::error::Error for ScrapbookError {}

fn expect_kind(
    entry: &ContentId,
    role: &'static str,
    id: &ContentId,
    expected: ContentKind,
) -> Result<(), ScrapbookError> {
    if id.kind() == expected {
        Ok(())
    } else {
        Err(ScrapbookError::WrongKind {
            entry: entry.clone(),
            role,
            expected,
        })
    }
}

/// A validated set of entries keyed by stable id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrapbookCatalog {
    entries: BTreeMap<ContentId, ScrapbookEntry>,
    order: Vec<ContentId>,
}

impl ScrapbookCatalog {
    /// Validates `entries`, keeping their order for display.
    ///
    /// # Errors
    ///
    /// [`ScrapbookError`].
    pub fn new(entries: Vec<ScrapbookEntry>) -> Result<Self, ScrapbookError> {
        let mut map = BTreeMap::new();
        let mut order = Vec::with_capacity(entries.len());
        for entry in entries {
            expect_kind(&entry.id, "entry id", &entry.id, ContentKind::ScrapbookItem)?;
            expect_kind(
                &entry.id,
                "title",
                &entry.title,
                ContentKind::StringResource,
            )?;
            if let Some(image) = &entry.image {
                expect_kind(&entry.id, "image", image, ContentKind::Image)?;
            }
            if let Some(replay) = &entry.replay {
                if entry.kind == EntryKind::Memento {
                    return Err(ScrapbookError::MementoWithReplay { entry: entry.id });
                }
                expect_kind(
                    &entry.id,
                    "replay mission",
                    &replay.mission,
                    ContentKind::Mission,
                )?;
                if let Some(variant) = &replay.variant {
                    expect_kind(&entry.id, "replay variant", variant, ContentKind::Mission)?;
                }
            }
            if let Resolved::Known(known) = &entry.unlock {
                known.value.validate(&entry.id, 0)?;
            }
            order.push(entry.id.clone());
            if map.insert(entry.id.clone(), entry).is_some() {
                let id = order.pop().expect("just pushed");
                return Err(ScrapbookError::DuplicateEntry { id });
            }
        }
        Ok(Self {
            entries: map,
            order,
        })
    }

    /// The entry with stable id `id`.
    #[must_use]
    pub fn entry(&self, id: &ContentId) -> Option<&ScrapbookEntry> {
        self.entries.get(id)
    }

    /// Every entry, in declared order.
    pub fn entries(&self) -> impl Iterator<Item = &ScrapbookEntry> {
        self.order.iter().filter_map(|id| self.entries.get(id))
    }

    /// Whether `entry`'s rule holds. An unknown rule never holds.
    #[must_use]
    pub fn is_unlocked(&self, entry: &ScrapbookEntry, has: &impl Fn(&UnlockFact) -> bool) -> bool {
        match &entry.unlock {
            Resolved::Known(known) => known.value.holds(has),
            Resolved::Unknown { .. } => false,
        }
    }

    /// The ids of every unlocked entry.
    #[must_use]
    pub fn unlocked(&self, has: &impl Fn(&UnlockFact) -> bool) -> BTreeSet<ContentId> {
        self.entries()
            .filter(|entry| self.is_unlocked(entry, has))
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// The entries to display, in declared order: unlocked ones and locked
    /// ones that are not hidden.
    pub fn visible<'a>(
        &'a self,
        has: &'a impl Fn(&UnlockFact) -> bool,
    ) -> impl Iterator<Item = (&'a ScrapbookEntry, bool)> {
        self.entries().filter_map(move |entry| {
            let unlocked = self.is_unlocked(entry, has);
            (unlocked || entry.visibility == EntryVisibility::Shown).then_some((entry, unlocked))
        })
    }
}
