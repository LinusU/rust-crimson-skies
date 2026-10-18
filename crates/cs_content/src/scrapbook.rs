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
//!
//! ## The discovered original table and its audit (stage F47-D)
//!
//! [`DiscoveredScrapbook::discover`] reads the installation's own
//! `ASSETS/SCRAPBOOK.CSV` member through [`read_scrapbook_table`] — the one
//! shared production read of the member (F47-C3), which F14-D.8's baseline
//! `scrapbook_item` rows bind as well — groups the records it holds into
//! [`DiscoveredPage`]s and records, for every item, which of the
//! installation's artwork members hold its picture. [`audit`] then compares
//! that discovery against a declared [`ScrapbookCatalog`] and against the
//! original progression (the mission rows of the content catalog), answering
//! AC04: every discovered page, every declared memento and every declared
//! replay link, each with what the original bytes actually state about it.
//!
//! The audit **measures**; it never fills a hole. The table's one documented
//! record kind carries no unlock field, no mission field and no memento field
//! ([`ORIGINAL_UNLOCK_FIELDS`], [`ORIGINAL_REPLAY_FIELDS`],
//! [`ORIGINAL_MEMENTO_RECORDS`]), so a declared rule that `Known` is reported
//! as not original-backed rather than assumed to be, and the original's own
//! progression link stays an explicit unknown in the report. See
//! `docs/findings/2026-10-08-f47-d-retail-scrapbook-audit.md`.
//!
//! ## What the `Objective` position indexes (stage F47-D.2)
//!
//! F47-D left the meaning of the `Objective` position unmeasured. F47-D.2
//! measured it and binds it here: [`DiscoveredScrapbook::objective_rules`]
//! reads every item's `Objective` position as the **numbered objective**
//! (`OBJECTIVE<n>`) of the campaign mission its page names, and returns a
//! `Resolved::Known<ScrapbookObjectiveRule>` per item with the member span
//! as provenance. The measurement is the member's own rows: 165 of the 167
//! `Snap_<page>_<n>` picture names the table references carry `Objective = n`
//! for the page the key spells, and the campaign missions declare contiguous
//! `OBJECTIVE1` runs that cover every referenced number. `0` (the initial
//! page) and the negative positions name no numbered objective and stay
//! explicit unknowns, as does a page the progression holds no mission for.
//!
//! The original's **replay control** and its **memento list** are native: the
//! screen scripts call engine commands this engine has not decoded
//! ([`TOC_REPLAY_COMMAND`], [`MEMENTO_COMMAND`]) and carry no mission identity
//! or memento state of their own, so neither is bound to a rule. See
//! `docs/findings/2026-10-17-f47-d2-objective-replay-and-mementos.md`.
//!
//! ## The declared runtime catalog (stage F47-C2)
//!
//! [`declare`] turns a [`DiscoveredScrapbook`] into the production
//! [`ScrapbookCatalog`] the runtime projects — one [`ScrapbookEntry`] per
//! [`DiscoveredItem`], keyed so the entry, the discovery and F14-D.8's
//! baseline rows name the same item — declaring only what the table states:
//! an explicit unknown unlock for every record, no mementos, no replay
//! links, no placeholder artwork. [`resource_names`] is the production read
//! of the `TitleResID` name→id join through the archive's own resource
//! headers. See `docs/findings/2026-10-17-f47-c2-declared-scrapbook-catalog.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use cs_assets::rof::{RofMemberInfo, mount_rof_into};
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_formats::text::resource_header::read_resource_header;
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan, SourceSpanError,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::InstallFileRecord;

use crate::catalog::Catalog;
use crate::catalog::baseline::{SCRAPBOOK_CONTAINER, SCRAPBOOK_MEMBER, install_file_key};
use crate::config::{ConfigDocument, ConfigEntry, RecordSchema, RecordView};
use crate::weapons::ORIGINAL_RESOURCE_HEADER;

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

// ---------------------------------------------------------------------------
// The discovered original scrapbook and its audit (stage F47-D)
// ---------------------------------------------------------------------------

/// The directory the installation stores its scrapbook artwork in.
///
/// Compared case-insensitively: the container spells it in upper case on the
/// owner's installation, and a member spelling is content this code reads
/// rather than rewrites.
pub const ARTWORK_DIRECTORY: &str = "ASSETS/GRAPHICS/SCRAPBOOK";

/// The image-name prefix the original memento-selection script names when it
/// builds its picture path.
///
/// The script's own literal comparison is one picture
/// (`ms_p_initialpinup1.jpg`), so the prefix is a **lead the original script
/// itself gives**, not a measurement that every `MS_P_*` picture is a
/// selectable memento: the selection order and the unlock state reach the
/// engine through a native callback the workspace has not decoded. The audit
/// reports how many table images carry the prefix and says the selection is
/// unmeasured instead of ranking them.
pub const MEMENTO_IMAGE_PREFIX: &str = "ms_p_";

/// The image extensions production code can hand a decoder, in the order a
/// capture prefers them.
///
/// This is a **declared** preference of this stage, not an original rule: the
/// table's `ImageType` codes are the three unknown-kind positions F12-I
/// recorded, so nothing in the member says which stored file a picture comes
/// from. Every artwork-bearing item of the owner's installation resolves to a
/// `.png` member, so the order only decides which of several stored formats a
/// capture draws when the container stores more than one.
const CAPTURE_FORMATS: [&str; 5] = [".png", ".jpg", ".jpeg", ".bmp", ".tga"];

/// How many fields of the table's one documented record kind could state an
/// unlock rule: **none**.
///
/// Measured from `RecordSchema::Scrapbook`'s documented field list (F12-I):
/// `Objective`, `ResourceID`, `ImageName`, `ImageType`, the coordinates,
/// `Alpha`, `Width`, `Height`, `DrawOrder`, the rectangle, `Zoom`, `ZoomX`,
/// `ZoomY`, `TitleResID`, `TextResID`. The documented name `Objective` is the
/// only one that could carry progression; what it *names* was measured by
/// F47-D.2 ([`DiscoveredScrapbook::objective_rules`]), but no documented field
/// states an unlock rule, so the audit still refuses to read one as a rule.
pub const ORIGINAL_UNLOCK_FIELDS: usize = 0;

/// How many fields of the table's one documented record kind name a mission
/// a replay could launch: **none**. See [`ORIGINAL_UNLOCK_FIELDS`]; the
/// original's replay control exists (the table-of-contents script names one),
/// but no table field names its target.
pub const ORIGINAL_REPLAY_FIELDS: usize = 0;

/// How many memento records the table holds: **none**. Every record the
/// documented schema covers is a `Mission_Spread_Item`, so a record the
/// schema does *not* cover would be counted under
/// [`DiscoveredScrapbook::gaps`] as `entry_not_a_scrapbook_item` rather than
/// read as a memento.
pub const ORIGINAL_MEMENTO_RECORDS: usize = 0;

/// The claim behind the measured meaning of the table's `Objective` position:
/// the numbered objective (`OBJECTIVE<n>`) of the campaign mission the item's
/// page names. See [`DiscoveredScrapbook::objective_rules`].
pub const OBJECTIVE_RULE_CLAIM: &str = "f47.d.2.scrapbook_objective";

/// The claim behind the measured replay-control mechanism.
pub const REPLAY_CONTROL_CLAIM: &str = "f47.d.2.scrapbook_replay_control";

/// The claim behind the measured memento-source mechanism.
pub const MEMENTO_SOURCE_CLAIM: &str = "f47.d.2.scrapbook_memento_source";

/// The engine command the original table-of-contents replay control calls
/// (`sbtoc_b_replay` → `callback($$E$$, 2104, <selected list index>)` in
/// `ASSETS/SCRIPTS/SCRAPBOOK_TOC.SCRIPT`), plus the three commands that fill
/// and gate the list it indexes: the per-entry list filler, the "current
/// mission" reader and the replay-eligibility test.
///
/// Measured from the script member's own bytes (F47-D.2): each is the number
/// in the script's `callback($$E$$, <n>, …)` form, not an engine address. The
/// **target** of a replay is decided by the native command, which this engine
/// has not decoded, so no mission is read out of the table or the script.
pub const TOC_REPLAY_COMMAND: u32 = 2104;
/// The per-entry filler command of the table-of-contents list.
pub const TOC_LIST_COMMAND: u32 = 2409;
/// The command returning the mission the scrapbook is currently showing.
pub const TOC_CURRENT_COMMAND: u32 = 2405;
/// The command answering whether one list entry can be replayed.
pub const TOC_REPLAYABLE_COMMAND: u32 = 2411;

/// The engine command the original memento-selection script reads and writes
/// its picture through (`callback($$E$$, 2150, mode, index, name)` in
/// `ASSETS/SCRIPTS/MOMENTOSELECTION.SCRIPT`): mode `0` gets the name at an
/// index (`100` forward, `101` backward), mode `1` selects the name.
///
/// Measured from the script member's own bytes (F47-D.2). The memento set,
/// its order and its unlock state live in the native state the command reads,
/// which this engine has not decoded, so the script carries none of them.
pub const MEMENTO_COMMAND: u32 = 2150;

/// Why the original scrapbook table could not be read.
///
/// Every variant names the source that refused: a missing archive or member
/// and a member that does not read as the table are errors here, never an
/// empty reading, because an audit of nothing must not look like an audit of
/// an empty scrapbook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScrapbookSourceError {
    /// The installation could not be inventoried.
    Discover {
        /// Why.
        reason: String,
    },
    /// The shared archive refused to mount.
    Mount {
        /// Why.
        reason: String,
    },
    /// The installation inventories no `GOSDATA/ASSETS/crimson.rof`.
    MissingContainer,
    /// The archive holds no `ASSETS/SCRAPBOOK.CSV` member.
    MissingMember,
    /// The member's stored extent could not be decoded.
    Read {
        /// Why.
        reason: String,
    },
    /// The member's extent could not be fingerprinted into a span.
    Span {
        /// Why.
        reason: String,
    },
    /// The decoded member is not the keyed-list scrapbook table.
    Document {
        /// Why.
        reason: String,
    },
}

impl fmt::Display for ScrapbookSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discover { reason } => {
                write!(f, "the installation cannot be inventoried: {reason}")
            }
            Self::Mount { reason } => {
                write!(
                    f,
                    "{SCRAPBOOK_CONTAINER} does not mount as an ROF container: {reason}"
                )
            }
            Self::MissingContainer => write!(
                f,
                "the installation inventories no {SCRAPBOOK_CONTAINER}, so the scrapbook table \
                 has no bytes to read"
            ),
            Self::MissingMember => write!(
                f,
                "{SCRAPBOOK_CONTAINER} holds no {SCRAPBOOK_MEMBER} member, so the scrapbook table \
                 has no bytes to read"
            ),
            Self::Read { reason } => write!(
                f,
                "the scrapbook member {SCRAPBOOK_MEMBER} does not decode: {reason}"
            ),
            Self::Span { reason } => write!(
                f,
                "the scrapbook member {SCRAPBOOK_MEMBER} has no checkable span: {reason}"
            ),
            Self::Document { reason } => write!(
                f,
                "the member {SCRAPBOOK_MEMBER} does not read as the keyed-list scrapbook table: \
                 {reason}"
            ),
        }
    }
}

impl std::error::Error for ScrapbookSourceError {}

// ---------------------------------------------------------------------------
// The shared scrapbook-table read (F47-C3)
// ---------------------------------------------------------------------------

/// Gap code: one [`SCRAPBOOK_MEMBER`] entry the documented
/// [`RecordSchema::Scrapbook`] does not cover — counted, never dropped.
pub const GAP_ENTRY_NOT_ITEM: &str = "entry_not_a_scrapbook_item";

/// Gap code: one entry key the table declares more than once with
/// **identical** field text — one item declared twice, so the repeat is
/// counted instead of becoming a second row with the same identity.
pub const GAP_DUPLICATE_KEY: &str = "duplicate_entry_key";

/// Gap code: one entry key the table declares more than once with
/// **different** field text. The key is the only identity the table states,
/// so neither record binds — counted, because nothing in the table says
/// which one the engine would read.
pub const GAP_AMBIGUOUS_KEY: &str = "ambiguous_entry_key";

/// Where the shared scrapbook-table read can refuse.
///
/// Each caller maps the variants onto its own refusal behaviour: F14-D.8's
/// baseline reports a collection gap with a diagnostic (or a
/// [`BaselineError`](crate::catalog::baseline::BaselineError) for the refusals
/// it treats as errors), and [`DiscoveredScrapbook::discover`] returns the
/// matching [`ScrapbookSourceError`]. The variants are staged by the step
/// that refused, and `Span` keeps the typed [`SourceSpanError`] because the
/// baseline's error enum carries it.
///
/// [`BaselineError`]: crate::catalog::baseline::BaselineError
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScrapbookTableFailure {
    /// The inventory holds no [`SCRAPBOOK_CONTAINER`] record.
    MissingContainer,
    /// The mount id or namespace could not be built.
    MountIdentity {
        /// Why.
        reason: String,
    },
    /// The archive does not mount as the observed ROF container.
    Mount {
        /// Why.
        reason: String,
    },
    /// The member [`AssetKey`] could not be built.
    MemberKey {
        /// Why.
        reason: String,
    },
    /// The archive holds no [`SCRAPBOOK_MEMBER`] member.
    MissingMember,
    /// The member's stored extent could not be read.
    Read {
        /// Why.
        reason: String,
    },
    /// The member's extent could not be fingerprinted into a span.
    Span {
        /// The typed refusal, for callers whose error enum carries it.
        source: SourceSpanError,
    },
    /// The decoded member does not read as the keyed-list scrapbook table.
    Document {
        /// Why.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// The shared container member read (F47-C4)
// ---------------------------------------------------------------------------

/// Where the shared find + mount + member + read steps refuse.
///
/// These are only the steps a caller performs before it touches the member's
/// bytes; the scrapbook-table read's own `Span` and `Document` refusals stay
/// in [`ScrapbookTableFailure`], which adds them after this read. Each caller
/// maps these variants onto its own refusal type, so the two keep exactly the
/// diagnostics they had before the steps were shared.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ContainerReadFailure {
    /// The inventory holds no [`SCRAPBOOK_CONTAINER`] record.
    MissingContainer,
    /// The mount id or namespace could not be built.
    MountIdentity {
        /// Why.
        reason: String,
    },
    /// The archive does not mount as the observed ROF container.
    Mount {
        /// Why.
        reason: String,
    },
    /// The member [`AssetKey`] could not be built.
    MemberKey {
        /// Why.
        reason: String,
    },
    /// The archive holds no such member.
    MissingMember,
    /// The member's stored extent could not be read.
    Read {
        /// Why.
        reason: String,
    },
}

impl From<ContainerReadFailure> for ScrapbookTableFailure {
    fn from(failure: ContainerReadFailure) -> Self {
        match failure {
            ContainerReadFailure::MissingContainer => Self::MissingContainer,
            ContainerReadFailure::MountIdentity { reason } => Self::MountIdentity { reason },
            ContainerReadFailure::Mount { reason } => Self::Mount { reason },
            ContainerReadFailure::MemberKey { reason } => Self::MemberKey { reason },
            ContainerReadFailure::MissingMember => Self::MissingMember,
            ContainerReadFailure::Read { reason } => Self::Read { reason },
        }
    }
}

/// One member of [`SCRAPBOOK_CONTAINER`], found, mounted and read: the shared
/// find + mount + member + read steps (F47-C4).
///
/// The scrapbook-table read (F47-C3) continues with the span, the parse and
/// the grouping; [`resource_header_bytes`] keeps only the bytes. Either way
/// the container is located by its F02 logical key, mounted through the
/// production ROF reader, and the member is looked up and read exactly once
/// by this code, so the two readers cannot drift apart in how they reach the
/// archive.
struct ContainerMemberRead {
    /// The container record the member was read from, for the caller's own
    /// row or edge.
    container: InstallFileRecord,
    /// The member's stored extent: the mount's offset and declared decoded
    /// length, which the scrapbook table's span cites.
    member: RofMemberInfo,
    /// The member's decoded bytes.
    bytes: Vec<u8>,
    /// Every member spelling the mounted archive declares, in the container's
    /// own order — for the caller that joins member data against the
    /// archive's other members.
    member_spellings: Vec<String>,
}

/// Finds `member` inside [`SCRAPBOOK_CONTAINER`] and reads it: the one
/// production find + mount + member + read path both the scrapbook-table read
/// (F47-C3) and the resource-header read (F47-C4) use.
///
/// The container record is found by its F02 logical key
/// ([`RelativePath::logical_key`]) in `files`, so a spelling the inventory
/// recorded with the other host's separator or letter case still names the
/// same archive. The mount is the production `rof` mount under
/// [`SCRAPBOOK_CONTAINER`]'s own label, and the member is looked up and read
/// through the mount's own namespace.
///
/// [`RelativePath::logical_key`]: cs_types::install::RelativePath::logical_key
///
/// # Errors
///
/// [`ContainerReadFailure`], staged by the step that refused.
fn read_container_member<'a>(
    install_root: &Path,
    install_hash: cs_types::evidence::ContentHash,
    files: impl IntoIterator<Item = &'a InstallFileRecord>,
    member: &str,
) -> Result<ContainerMemberRead, ContainerReadFailure> {
    let container_key = SCRAPBOOK_CONTAINER.to_ascii_lowercase();
    let record = files
        .into_iter()
        .find(|record| record.relative_spelling.logical_key() == container_key)
        .ok_or(ContainerReadFailure::MissingContainer)?;
    let path = install_root.join(record.relative_spelling.as_str());

    let mut builder = SessionBuilder::new(ResolveContext::new(install_hash));
    let mount = MountBuilder::new(
        MountId::new("rof-scrapbook").map_err(|error| ContainerReadFailure::MountIdentity {
            reason: error.to_string(),
        })?,
        MountNamespace::new(INSTALL_NAMESPACE).map_err(|error| {
            ContainerReadFailure::MountIdentity {
                reason: error.to_string(),
            }
        })?,
        PrecedenceClass::Shared,
        SCRAPBOOK_CONTAINER,
    )
    .retail();
    let source = mount_rof_into(&mut builder, mount, &path).map_err(|error| {
        ContainerReadFailure::Mount {
            reason: error.to_string(),
        }
    })?;
    let key = AssetKey::from_spelling(source.namespace().as_str(), member, "default").map_err(
        |error| ContainerReadFailure::MemberKey {
            reason: error.to_string(),
        },
    )?;
    let info = source
        .member(&key)
        .cloned()
        .ok_or(ContainerReadFailure::MissingMember)?;
    let read = source
        .read(&key)
        .map_err(|error| ContainerReadFailure::Read {
            reason: error.to_string(),
        })?;
    let member_spellings = source.members().map(|info| info.spelling.clone()).collect();
    Ok(ContainerMemberRead {
        container: record.clone(),
        member: info,
        bytes: read.data,
        member_spellings,
    })
}

/// One key group of the scrapbook table after the shared read: the records
/// the member declares under one entry key, in member order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrapbookGroup {
    /// The entry key the group shares, verbatim bytes — the item's identity,
    /// never its line number or position.
    pub raw_key: Vec<u8>,
    /// The group's records, in member order. `entries[0]` is the record both
    /// callers bind (`line`, fields); the vector is never empty.
    pub entries: Vec<ConfigEntry>,
    /// Whether any record's field text disagrees with the first's. An
    /// ambiguous group was counted under [`GAP_AMBIGUOUS_KEY`] and no record
    /// of it binds.
    pub ambiguous: bool,
}

impl ScrapbookGroup {
    /// The group's first record, the one every caller binds.
    pub fn first(&self) -> &ConfigEntry {
        &self.entries[0]
    }
}

/// The member's shared read product: the parsed table plus what the shared
/// reader could group, with the shared gap tally already counted.
#[derive(Clone, Debug)]
pub struct ScrapbookTable {
    /// SHA-256 of the decoded member bytes.
    pub member_sha256: cs_types::evidence::ContentHash,
    /// The member's span: the container, the member key, the decoded extent
    /// and the member digest — the provenance the item binding cites.
    pub span: SourceSpan,
    /// The parsed document, kept for callers that count the entries the
    /// member declared or read records it holds.
    pub document: ConfigDocument,
    /// The `Mission_Spread_Item` records the documented schema covers,
    /// grouped by their own entry key in key order.
    pub groups: Vec<ScrapbookGroup>,
    /// The shared gap tally: [`GAP_ENTRY_NOT_ITEM`], [`GAP_DUPLICATE_KEY`]
    /// and [`GAP_AMBIGUOUS_KEY`], counted once here so the two readers can
    /// never apply the rule differently.
    pub gaps: BTreeMap<&'static str, usize>,
    /// The archive record the member was read from, for the caller's own
    /// row or edge.
    pub container: InstallFileRecord,
    /// Every member spelling the mounted archive declares, in the
    /// container's own order — for callers that join member data against
    /// the archive's other members.
    pub member_spellings: Vec<String>,
}

/// Reads the original scrapbook table: [`SCRAPBOOK_MEMBER`] of
/// [`SCRAPBOOK_CONTAINER`], found in `files` by the record's logical key,
/// mounted through the production ROF reader, read and parsed through the
/// production keyed-list reader ([`ConfigDocument`]), with the
/// `Mission_Spread_Item` records grouped by their own entry key.
///
/// The grouping rule is the F14-D.8 measurement both callers share: records
/// under one key that agree on field text are one item with the repeat
/// counted under [`GAP_DUPLICATE_KEY`]; records that disagree are counted
/// under [`GAP_AMBIGUOUS_KEY`] and neither binds, because nothing in the
/// table says which one the engine would read. A record the documented
/// schema does not cover is counted under [`GAP_ENTRY_NOT_ITEM`] rather
/// than dropped.
///
/// This is the one production read of the member (F47-C3): F14-D.8's
/// `scrapbook_item` rows and [`DiscoveredScrapbook::discover`] both bind
/// this product, so a change to the find, mount, read, parse or grouping
/// rule moves both readers together. The find, mount, member and read steps
/// themselves are one shared production path, shared with the resource-header
/// read (F47-C4).
///
/// # Errors
///
/// [`ScrapbookTableFailure`], staged by the step that refused.
pub fn read_scrapbook_table<'a>(
    install_root: &Path,
    install_hash: cs_types::evidence::ContentHash,
    files: impl IntoIterator<Item = &'a InstallFileRecord>,
) -> Result<ScrapbookTable, ScrapbookTableFailure> {
    let ContainerMemberRead {
        container,
        member,
        bytes,
        member_spellings,
    } = read_container_member(install_root, install_hash, files, SCRAPBOOK_MEMBER)
        .map_err(ScrapbookTableFailure::from)?;
    let bytes = bytes.as_slice();
    let member_sha256 = cs_assets::install::sha256(bytes);
    let span = cs_types::asset_id::SourceSpan::new(
        install_hash,
        SCRAPBOOK_CONTAINER,
        Some(SCRAPBOOK_MEMBER),
        member.offset,
        member.declared_decoded_len,
        Some(member_sha256),
    )
    .map_err(|source| ScrapbookTableFailure::Span { source })?;
    let mut context = cs_formats::ParseContext::with_defaults(SCRAPBOOK_CONTAINER);
    let document = ConfigDocument::read(&mut context, span.clone(), bytes).map_err(|error| {
        ScrapbookTableFailure::Document {
            reason: error.to_string(),
        }
    })?;

    // The entry key is the item's identity, so a key the table declares
    // twice names one item twice. Records that agree are one item with the
    // repeat counted under `duplicate_entry_key`; records that disagree are
    // counted under `ambiguous_entry_key` and neither binds, because nothing
    // in the table says which the engine would read (the F14-D.8
    // measurement).
    let mut gaps: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut grouped: BTreeMap<Vec<u8>, Vec<ConfigEntry>> = BTreeMap::new();
    for entry in document.entries() {
        if RecordSchema::for_entry(entry) != Some(RecordSchema::Scrapbook) {
            *gaps.entry(GAP_ENTRY_NOT_ITEM).or_default() += 1;
            continue;
        }
        grouped
            .entry(entry.key.clone())
            .or_default()
            .push(entry.clone());
    }
    let mut groups = Vec::with_capacity(grouped.len());
    for (raw_key, entries) in grouped {
        let ambiguous = entries.iter().any(|entry| entry.value != entries[0].value);
        if ambiguous {
            *gaps.entry(GAP_AMBIGUOUS_KEY).or_default() += entries.len();
        } else if entries.len() > 1 {
            *gaps.entry(GAP_DUPLICATE_KEY).or_default() += entries.len() - 1;
        }
        groups.push(ScrapbookGroup {
            raw_key,
            entries,
            ambiguous,
        });
    }
    Ok(ScrapbookTable {
        member_sha256,
        span,
        document,
        groups,
        gaps,
        container,
        member_spellings,
    })
}

/// One discovered original scrapbook item: what the table and the container
/// say about it, with nothing filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredItem {
    /// The record's own entry key, the item's identity.
    pub key: String,
    /// [`install_file_key`] of [`DiscoveredItem::key`], the id key a declared
    /// entry must carry to be the same item, or `None` when the key is not
    /// valid UTF-8 (counted under `entry_key_not_utf8`, never merged with
    /// another key).
    pub id_key: Option<String>,
    /// The entry key's first component, the page this item belongs to.
    pub page: u32,
    /// The entry key's second component, the spread inside the page.
    pub spread: u32,
    /// The entry key's third component, the item's slot in the spread.
    pub slot: u32,
    /// The `Objective` field: the number is converted because the position's
    /// kind is measured (F12-I). What the position *names* is measured
    /// separately by [`DiscoveredScrapbook::objective_rules`] (F47-D.2): the
    /// numbered objective of the campaign mission the item's page names. No
    /// rule is read out of the raw number here.
    ///
    /// [`DiscoveredScrapbook::objective_rules`]: DiscoveredScrapbook::objective_rules
    pub objective: Option<i64>,
    /// The `ImageName` field.
    pub image: String,
    /// Every member of the container whose file stem equals
    /// [`DiscoveredItem::image`] (case-insensitively), sorted; empty when the
    /// installation holds no such picture.
    pub artwork: Vec<String>,
    /// The artwork member a capture prefers ([`CAPTURE_FORMATS`]), or `None`
    /// when none of the stored formats is one production code can decode.
    pub capture: Option<String>,
    /// The `TitleResID` field, a string-resource name or `0`.
    pub title: String,
    /// The `TextResID` field, a string-resource name or `0`.
    pub text: String,
    /// The member's own line number: the order this stage keeps items in, so
    /// a page is read in the table's order rather than in key order.
    pub line: u64,
}

/// One page of the discovered scrapbook.
///
/// A page is the run of records whose entry keys share their **first
/// component**: on the owner's installation every key is `<n>_<n>_<n>` and
/// the 461 records form exactly 25 contiguous runs, `0` to `24`. The member
/// documents no key grammar, so "page" is this stage's reading of a measured
/// structure — the runs, the member's own section comments and the sibling
/// `SB_<page>_<spread>_<name>` picture naming all agree, and whether the
/// original *calls* the component a page is recorded as unmeasured in
/// `docs/findings/2026-10-08-f47-d-retail-scrapbook-audit.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredPage {
    /// The entry key's first component.
    pub page: u32,
    /// The page's items, in the member's own order.
    pub items: Vec<DiscoveredItem>,
}

/// The original scrapbook table, discovered: every record of
/// `ASSETS/SCRAPBOOK.CSV` grouped into the pages its keys spell, each item
/// joined to the artwork the container actually holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredScrapbook {
    /// The installation fingerprint of the read bytes.
    pub install_sha256: String,
    /// The canonical-content fingerprint of the inventoried manifest.
    pub content_sha256: String,
    /// SHA-256 of the decoded member bytes the records were read from.
    pub member_sha256: String,
    /// Where those bytes live: the container, the member key, the decoded
    /// extent and the member digest. This is the provenance every
    /// [`DiscoveredItem::objective`] binding ([`objective_rules`]) cites.
    ///
    /// [`objective_rules`]: DiscoveredScrapbook::objective_rules
    pub member_span: SourceSpan,
    /// Every record the documented schema covers, before grouping.
    pub records: usize,
    /// The pages, ordered by [`DiscoveredPage::page`].
    pub pages: Vec<DiscoveredPage>,
    /// What discovery could not turn into a paged item, counted and named:
    /// `entry_not_a_scrapbook_item`, `duplicate_entry_key`,
    /// `ambiguous_entry_key`, `entry_key_not_page_structured`,
    /// `entry_key_not_utf8`, `item_without_artwork`,
    /// `artwork_outside_artwork_directory` and `artwork_undecodable_format`.
    pub gaps: BTreeMap<String, u32>,
}

impl DiscoveredScrapbook {
    /// Reads the original scrapbook table out of `install_root`.
    ///
    /// Production discovery finds the archive, the production ROF mount opens
    /// it, the production keyed-list reader parses the member, and every item
    /// is joined to the container's own artwork members. Nothing is written;
    /// the installation is read-only.
    ///
    /// # Errors
    ///
    /// [`ScrapbookSourceError`]: every variant names the source that refused,
    /// so a missing or unreadable table is an error and never an empty
    /// reading.
    pub fn discover(install_root: &Path) -> Result<Self, ScrapbookSourceError> {
        let found = cs_assets::install::discover(install_root).map_err(|error| {
            ScrapbookSourceError::Discover {
                reason: error.to_string(),
            }
        })?;
        let install = cs_assets::install::fingerprint(&found.manifest);
        let content = cs_assets::install::content_fingerprint(&found.manifest);
        let table = read_scrapbook_table(install_root, install, found.manifest.files.iter())
            .map_err(|failure| match failure {
                ScrapbookTableFailure::MissingContainer => ScrapbookSourceError::MissingContainer,
                ScrapbookTableFailure::MountIdentity { reason }
                | ScrapbookTableFailure::Mount { reason } => ScrapbookSourceError::Mount { reason },
                ScrapbookTableFailure::MemberKey { reason } => {
                    ScrapbookSourceError::Document { reason }
                }
                ScrapbookTableFailure::MissingMember => ScrapbookSourceError::MissingMember,
                ScrapbookTableFailure::Read { reason } => ScrapbookSourceError::Read { reason },
                ScrapbookTableFailure::Span { source } => ScrapbookSourceError::Span {
                    reason: source.to_string(),
                },
                ScrapbookTableFailure::Document { reason } => {
                    ScrapbookSourceError::Document { reason }
                }
            })?;
        let member_span = table.span.clone();

        // The artwork side: every member of the container, keyed by its file
        // stem, so an item's picture is found by name rather than by a path
        // this stage invented.
        let mut stems: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for spelling in &table.member_spellings {
            let Some((_, name)) = spelling.rsplit_once('/') else {
                continue;
            };
            let stem = name.split('.').next().unwrap_or(name);
            stems
                .entry(stem.to_ascii_lowercase())
                .or_default()
                .push(spelling.clone());
        }
        for spellings in stems.values_mut() {
            spellings.sort();
            spellings.dedup();
        }

        let mut gaps: BTreeMap<String, u32> = BTreeMap::new();
        let bump = |gaps: &mut BTreeMap<String, u32>, code: &str, by: u32| {
            *gaps.entry(code.to_owned()).or_default() += by;
        };
        for (code, count) in &table.gaps {
            bump(&mut gaps, code, *count as u32);
        }
        if table.groups.is_empty() {
            // The keyed-list reader accepts a member that holds no record the
            // documented schema covers, so an unreadable table would otherwise
            // look like a table with nothing in it. It is an error instead: an
            // audit of nothing must never read as an audit of an empty
            // scrapbook.
            return Err(ScrapbookSourceError::Document {
                reason: "the member holds no record the documented Mission_Spread_Item schema \
                         covers"
                    .to_owned(),
            });
        }

        let mut records = 0usize;
        let mut items: Vec<DiscoveredItem> = Vec::new();
        let artwork_directory = format!("{}/", ARTWORK_DIRECTORY.to_ascii_lowercase());
        for group in &table.groups {
            if group.ambiguous {
                continue;
            }
            records += 1;
            let first = group.first();
            let Ok(key) = String::from_utf8(group.raw_key.clone()) else {
                bump(&mut gaps, "entry_key_not_utf8", 1);
                continue;
            };
            let Some((page, spread, slot)) = parse_key_components(&key) else {
                bump(&mut gaps, "entry_key_not_page_structured", 1);
                continue;
            };
            let view = RecordView::new(RecordSchema::Scrapbook, first);
            let at = |position: usize| -> String {
                view.fields()
                    .get(position)
                    .map(|field| String::from_utf8_lossy(field.field.value()).into_owned())
                    .unwrap_or_default()
            };
            let image = at(IMAGE_NAME_POSITION);
            let artwork = stems
                .get(&image.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default();
            if artwork.iter().any(|spelling| {
                !spelling
                    .to_ascii_lowercase()
                    .starts_with(&artwork_directory)
            }) {
                bump(&mut gaps, "artwork_outside_artwork_directory", 1);
            }
            let capture = CAPTURE_FORMATS.iter().find_map(|extension| {
                artwork
                    .iter()
                    .find(|spelling| spelling.to_ascii_lowercase().ends_with(extension))
                    .cloned()
            });
            if artwork.is_empty() {
                bump(&mut gaps, "item_without_artwork", 1);
            } else if capture.is_none() {
                bump(&mut gaps, "artwork_undecodable_format", 1);
            }
            items.push(DiscoveredItem {
                id_key: Some(install_file_key(&key)),
                objective: at(OBJECTIVE_POSITION).trim().parse::<i64>().ok(),
                image,
                artwork,
                capture,
                title: at(TITLE_RES_ID_POSITION),
                text: at(TEXT_RES_ID_POSITION),
                key,
                page,
                spread,
                slot,
                line: first.line,
            });
        }

        // Pages: the runs the keys spell, each page's items in the member's
        // own order.
        items.sort_by_key(|item| item.line);
        let mut by_page: BTreeMap<u32, Vec<DiscoveredItem>> = BTreeMap::new();
        for item in items {
            by_page.entry(item.page).or_default().push(item);
        }
        let pages = by_page
            .into_iter()
            .map(|(page, items)| DiscoveredPage { page, items })
            .collect();

        Ok(Self {
            install_sha256: install.to_hex(),
            content_sha256: content.to_hex(),
            member_sha256: table.member_sha256.to_hex(),
            member_span,
            records,
            pages,
            gaps,
        })
    }

    /// The number of pages discovery found.
    #[must_use]
    pub const fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The number of items that belong to a page.
    #[must_use]
    pub fn paged_items(&self) -> usize {
        self.pages.iter().map(|page| page.items.len()).sum()
    }

    /// The number of items the container holds a picture for.
    #[must_use]
    pub fn items_with_artwork(&self) -> usize {
        self.pages
            .iter()
            .flat_map(|page| &page.items)
            .filter(|item| !item.artwork.is_empty())
            .count()
    }

    /// Every item in the table's own order: pages in page order, items in
    /// member-line order inside their page.
    pub fn items(&self) -> impl Iterator<Item = &DiscoveredItem> {
        self.pages.iter().flat_map(|page| &page.items)
    }

    /// The pages the container holds no picture for.
    #[must_use]
    pub fn pages_without_artwork(&self) -> Vec<u32> {
        self.pages
            .iter()
            .filter(|page| page.items.iter().all(|item| item.artwork.is_empty()))
            .map(|page| page.page)
            .collect()
    }

    /// The distinct image names of [`MEMENTO_IMAGE_PREFIX`]'s shape the table
    /// references. See that constant: this counts a prefix, it does not rank
    /// a memento.
    #[must_use]
    pub fn memento_named_images(&self) -> usize {
        self.pages
            .iter()
            .flat_map(|page| &page.items)
            .map(|item| item.image.to_ascii_lowercase())
            .filter(|image| image.starts_with(MEMENTO_IMAGE_PREFIX))
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// The discovery as the artifact this stage writes: fingerprints, the
    /// per-page rows and every named gap. It holds identifiers and counts,
    /// never original text beyond the identifiers the member spells.
    #[must_use]
    pub fn json(&self) -> String {
        let pages: Vec<String> = self
            .pages
            .iter()
            .map(|page| {
                let with_artwork = page
                    .items
                    .iter()
                    .filter(|item| !item.artwork.is_empty())
                    .count();
                format!(
                    "{{\"page\": {}, \"items\": {}, \"with_artwork\": {}, \"without_artwork\": \
                     {}}}",
                    page.page,
                    page.items.len(),
                    with_artwork,
                    page.items.len() - with_artwork
                )
            })
            .collect();
        format!(
            "{{\n \"install_sha256\": {},\n \"content_sha256\": {},\n \"member_sha256\": {},\n \
             \"records\": {},\n \"paged_items\": {},\n \"page_count\": {},\n \"items_with_artwork\": \
             {},\n \"memento_named_images\": {},\n \"gaps\": {},\n \"pages\": [{}]\n}}\n",
            quote(&self.install_sha256),
            quote(&self.content_sha256),
            quote(&self.member_sha256),
            self.records,
            self.paged_items(),
            self.pages.len(),
            self.items_with_artwork(),
            self.memento_named_images(),
            map_json(&self.gaps),
            pages.join(", ")
        )
    }
}

/// The `Objective` position of the documented record (F12-I).
const OBJECTIVE_POSITION: usize = 0;

/// The `ImageName` position of the documented record (F12-I).
const IMAGE_NAME_POSITION: usize = 2;

/// The `TitleResID` position of the documented record (F12-I).
const TITLE_RES_ID_POSITION: usize = 14;

/// The `TextResID` position of the documented record (F12-I).
const TEXT_RES_ID_POSITION: usize = 15;

/// The measured `Objective` rule of one discovered item: the numbered
/// objective (`OBJECTIVE<n>`) of the campaign mission the item's page names.
///
/// This is a measurement of the table's own `Objective` position, bound to the
/// mission the page names through `progression`'s campaign rows — never a
/// runtime-verified unlock predicate. Whether the original engine gates an
/// item's visibility on objective completion is native behavior this engine
/// has not decoded, so a `Known` rule here means "this is the objective the
/// position names", not "the original unlocks this here".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrapbookObjectiveRule {
    /// The campaign mission, in campaign order, the item's page names.
    pub mission: ContentId,
    /// The numbered objective inside that mission.
    pub objective: u32,
}

/// One discovered item's `Objective` binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveRule {
    /// The item's identity key ([`DiscoveredItem::id_key`]), or `None` when
    /// the entry key is not valid UTF-8.
    pub item: Option<String>,
    /// The measured rule, or an explicit unknown when the position names no
    /// numbered objective of a mission the progression holds.
    pub rule: Resolved<ScrapbookObjectiveRule>,
}

/// What the table's `Objective` position was measured to say (F47-D.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectiveCensus {
    /// Items examined.
    pub items: usize,
    /// Items whose position is a positive number: a numbered objective.
    pub numbered: usize,
    /// Items whose position is `0` or negative, which names no numbered
    /// objective.
    pub special: usize,
    /// Items whose position is not a whole number at all.
    pub unparsed: usize,
    /// Numbered items bound to a campaign mission row.
    pub bound: usize,
    /// Pages that carry a numbered objective but no campaign mission row.
    pub page_without_mission: Vec<u32>,
    /// Items whose `ImageName` is a `Snap_<page>_<n>` runtime snapshot.
    pub snapshot_items: usize,
    /// Snapshot items whose position is exactly the number their own name
    /// spells, for the page their key spells.
    pub snapshot_agree: usize,
    /// Snapshot items whose position disagrees with their name, by id key.
    pub snapshot_disagree: Vec<String>,
    /// Every item's binding, in the member's own order.
    pub rules: Vec<ObjectiveRule>,
}

impl DiscoveredScrapbook {
    /// Binds every item's `Objective` position to the numbered objective of
    /// the campaign mission its page names (F47-D.2).
    ///
    /// The page is the key's first component; page `p` is the `p`-th campaign
    /// mission of `progression` in canonical id order (`mission/ch1-m01` …
    /// `mission/ch5-m04`), which is the installation's campaign order. A
    /// position of `0` (the initial page), a negative position, a
    /// non-numeric position and a page the progression holds no mission for
    /// all stay explicit unknowns, never a guessed rule.
    ///
    /// Provenance is [`OBJECTIVE_RULE_CLAIM`] at
    /// [`Provenance::class`] `observed_tool`, over the table member's own
    /// span ([`DiscoveredScrapbook::member_span`]).
    #[must_use]
    pub fn objective_rules(&self, progression: &Catalog) -> ObjectiveCensus {
        let missions: Vec<&ContentId> = progression
            .elements()
            .filter(|element| element.kind == ContentKind::Mission)
            .map(|element| &element.id)
            .collect();
        let provenance = objective_provenance(&self.member_span);

        let mut census = ObjectiveCensus::default();
        let mut pages_without_mission: BTreeSet<u32> = BTreeSet::new();
        for page in &self.pages {
            for item in &page.items {
                census.items += 1;
                if let Some((named_page, named_objective)) = snapshot_name_components(&item.image) {
                    census.snapshot_items += 1;
                    if named_page == page.page && item.objective == Some(i64::from(named_objective))
                    {
                        census.snapshot_agree += 1;
                    } else if let Some(id_key) = &item.id_key {
                        census.snapshot_disagree.push(id_key.clone());
                    }
                }

                let rule = match item.objective {
                    None => {
                        census.unparsed += 1;
                        objective_unknown(
                            "the Objective position is not a whole number, so it names no objective",
                        )
                    }
                    Some(objective) if objective <= 0 => {
                        census.special += 1;
                        objective_unknown(
                            "the Objective position is not a numbered objective: 0 is the \
                             initial page and a negative value names no objective",
                        )
                    }
                    Some(objective) => match u32::try_from(objective) {
                        Err(_) => {
                            census.unparsed += 1;
                            objective_unknown(
                                "the Objective position is larger than a numbered objective can be",
                            )
                        }
                        Ok(objective) => {
                            census.numbered += 1;
                            let mission = if page.page == 0 {
                                None
                            } else {
                                missions.get((page.page - 1) as usize).copied()
                            };
                            match mission {
                                Some(mission) => {
                                    census.bound += 1;
                                    Resolved::Known(Known::new(
                                        ScrapbookObjectiveRule {
                                            mission: mission.clone(),
                                            objective,
                                        },
                                        provenance.clone(),
                                    ))
                                }
                                None => {
                                    pages_without_mission.insert(page.page);
                                    objective_unknown(
                                        "the progression holds no campaign mission for this \
                                         page, so the numbered objective belongs to none here",
                                    )
                                }
                            }
                        }
                    },
                };
                census.rules.push(ObjectiveRule {
                    item: item.id_key.clone(),
                    rule,
                });
            }
        }
        census.page_without_mission = pages_without_mission.into_iter().collect();
        census
    }
}

/// The provenance of an [`ScrapbookObjectiveRule`]: an `observed_tool` claim
/// over the table member's own span.
fn objective_provenance(span: &SourceSpan) -> Provenance {
    Provenance::new(
        ClaimId::new(OBJECTIVE_RULE_CLAIM).expect("a valid claim id"),
        ClaimStatus::ObservedTool,
        Some(span.clone()),
    )
    .expect("an observed_tool provenance is never refused")
}

/// An explicit unknown `Objective` rule with `reason`.
fn objective_unknown(reason: &str) -> Resolved<ScrapbookObjectiveRule> {
    Resolved::unknown(
        ClaimId::new(OBJECTIVE_RULE_CLAIM).expect("a valid claim id"),
        reason,
    )
    .expect("the reason is never empty")
}

/// The `<page>_<objective>` a `Snap_<page>_<n>` runtime-snapshot picture name
/// spells, or `None` when the name is not that shape.
fn snapshot_name_components(image: &str) -> Option<(u32, u32)> {
    let lowered = image.to_ascii_lowercase();
    let rest = lowered.strip_prefix("snap_")?;
    let (page, rest) = rest.split_once('_')?;
    let objective = rest.split('.').next()?;
    Some((page.parse().ok()?, objective.parse().ok()?))
}

/// The `<page>_<spread>_<slot>` components of an entry key, or `None` when
/// the key is not that shape (a counted gap, never a guessed component).
fn parse_key_components(key: &str) -> Option<(u32, u32, u32)> {
    let mut parts = key.split('_');
    let page = parts.next()?.parse::<u32>().ok()?;
    let spread = parts.next()?.parse::<u32>().ok()?;
    let slot = parts.next()?.parse::<u32>().ok()?;
    parts.next().is_none().then_some((page, spread, slot))
}

/// What the audit found, comparing the discovered original table with a
/// declared [`ScrapbookCatalog`] and with the original progression.
///
/// Every field is a measurement of one of the three things AC04 names —
/// pages, mementos and replay links — plus the unlock paths behind them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditReport {
    /// SHA-256 of the decoded member the discovery read.
    pub member_sha256: String,
    /// Records the documented schema covers.
    pub records: usize,
    /// Pages discovery found.
    pub pages: usize,
    /// Items that belong to a page.
    pub paged_items: usize,
    /// Named gaps of the discovery itself.
    pub gaps: BTreeMap<String, u32>,
    /// Items the container holds a picture for.
    pub items_with_artwork: usize,
    /// Items the container holds no picture for.
    pub items_without_artwork: usize,
    /// Pages with no picture at all.
    pub pages_without_artwork: usize,
    /// Declared entries handed to the audit.
    pub declared: usize,
    /// Declared entries whose id is one the original table declares.
    pub declared_matched: usize,
    /// Declared entries the original table does **not** declare, by id: a
    /// fabricated entry is a defect of the declaration, never of the original.
    pub declared_without_original: Vec<String>,
    /// Original items no declared entry names (a declared catalog of nothing
    /// reports every item here).
    pub undeclared_items: usize,
    /// Declared entries whose rule is `Known`.
    pub unlock_known: usize,
    /// Declared entries whose rule is an explicit unknown (never unlocks).
    pub unlock_unknown: usize,
    /// `Known` rules the original table cannot back, because it documents no
    /// unlock field ([`ORIGINAL_UNLOCK_FIELDS`]).
    pub unlock_unbacked: usize,
    /// Rule subjects (missions, stunts, aces) the original catalog holds no
    /// row for, by id.
    pub unlock_subjects_missing: Vec<String>,
    /// Declared replay links.
    pub replay_links: usize,
    /// Replay links naming a mission the original catalog holds no row for.
    pub replay_missions_missing: Vec<String>,
    /// Declared entries that are cabin mementos.
    pub declared_mementos: usize,
    /// Distinct table images shaped like a memento picture
    /// ([`MEMENTO_IMAGE_PREFIX`]).
    pub memento_named_images: usize,
    /// Campaign mission rows of the original progression handed to the audit.
    pub progression_missions: usize,
    /// What the table's `Objective` position says (F47-D.2): the numbered
    /// objective of the campaign mission each item's page names.
    pub objective: ObjectiveCensus,
}

impl AuditReport {
    /// Whether the audit found nothing to report: no gaps, every declared
    /// entry matched the original, every original item declared, every rule
    /// unknown (so nothing is awarded without evidence), no missing subjects
    /// or missions, and no page without artwork.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.gaps.is_empty()
            && self.declared == self.declared_matched
            && self.undeclared_items == 0
            && self.unlock_known == 0
            && self.unlock_unbacked == 0
            && self.unlock_subjects_missing.is_empty()
            && self.replay_missions_missing.is_empty()
            && self.pages_without_artwork == 0
            && self.declared_mementos == ORIGINAL_MEMENTO_RECORDS
            && self.objective.page_without_mission.is_empty()
    }

    /// The report as the artifact this stage writes.
    #[must_use]
    pub fn json(&self) -> String {
        format!(
            "{{\n \"member_sha256\": {},\n \"records\": {},\n \"pages\": {},\n \"paged_items\": \
             {},\n \"items_with_artwork\": {},\n \"items_without_artwork\": {},\n \
             \"pages_without_artwork\": {},\n \"declared\": {},\n \"declared_matched\": {},\n \
             \"declared_without_original\": [{}],\n \"undeclared_items\": {},\n \"unlock_known\": \
             {},\n \"unlock_unknown\": {},\n \"unlock_unbacked\": {},\n \"unlock_subjects_missing\": \
             [{}],\n \"replay_links\": {},\n \"replay_missions_missing\": [{}],\n \
             \"original_unlock_fields\": {},\n \"original_replay_fields\": {},\n \
             \"original_memento_records\": {},\n \"declared_mementos\": {},\n \
             \"memento_named_images\": {},\n \"progression_missions\": {},\n \"gaps\": {},\n \
             \"objective\": {{\"items\": {}, \"numbered\": {}, \"special\": {}, \"unparsed\": {}, \
             \"bound\": {}, \"page_without_mission\": [{}], \"snapshot_items\": {}, \
             \"snapshot_agree\": {}, \"snapshot_disagree\": [{}], \"backing\": {}}},\n \
             \"backing\": {{\"unlock\": {}, \"replay\": {}, \"memento\": {}}}\n}}\n",
            quote(&self.member_sha256),
            self.records,
            self.pages,
            self.paged_items,
            self.items_with_artwork,
            self.items_without_artwork,
            self.pages_without_artwork,
            self.declared,
            self.declared_matched,
            string_array(&self.declared_without_original),
            self.undeclared_items,
            self.unlock_known,
            self.unlock_unknown,
            self.unlock_unbacked,
            string_array(&self.unlock_subjects_missing),
            self.replay_links,
            string_array(&self.replay_missions_missing),
            ORIGINAL_UNLOCK_FIELDS,
            ORIGINAL_REPLAY_FIELDS,
            ORIGINAL_MEMENTO_RECORDS,
            self.declared_mementos,
            self.memento_named_images,
            self.progression_missions,
            map_json(&self.gaps),
            self.objective.items,
            self.objective.numbered,
            self.objective.special,
            self.objective.unparsed,
            self.objective.bound,
            self.objective
                .page_without_mission
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            self.objective.snapshot_items,
            self.objective.snapshot_agree,
            string_array(&self.objective.snapshot_disagree),
            quote(OBJECTIVE_BACKING),
            quote(UNLOCK_BACKING),
            quote(REPLAY_BACKING),
            quote(MEMENTO_BACKING),
        )
    }
}

/// What this stage (F47-D.2) measured about the `Objective` position: it
/// indexes the numbered objective of the campaign mission the item's page
/// names. The snapshot naming and the missions' own `OBJECTIVE<n>` runs are
/// the evidence; whether the engine gates visibility on it stays native.
pub const OBJECTIVE_BACKING: &str = "measured: Objective indexes the numbered objective (OBJECTIVE<n>) of the campaign \
     mission the item's page names; 0 and negative values name none; whether the engine gates visibility on it is \
     native and undecoded";

/// What this stage measured about the original's unlock path: the table
/// documents no unlock field, and the one field whose documented name could
/// carry progression (`Objective`) is F47-D.2's measured numbered objective,
/// but whether the engine gates an item's visibility on it is native
/// behaviour this engine has not decoded, so a `Known` rule is still reported
/// as not original-backed.
pub const UNLOCK_BACKING: &str = "unmeasured: the table documents no unlock field; the Objective position names a \
     mission's numbered objective (F47-D.2) but whether the engine gates an item on it is undecoded, so a Known rule \
     is reported as not original-backed";

/// What this stage measured about the original's replay path: the table
/// documents no mission field, while the original table-of-contents script
/// does name a replay control.
pub const REPLAY_BACKING: &str = "unmeasured: the table documents no mission field; the original table-of-contents script \
     names a replay control, so a link's target cannot be read from the table";

/// What this stage measured about the original's mementos: the table holds no
/// memento record, and the original memento-selection script receives its
/// picture from a native callback this engine has not decoded.
pub const MEMENTO_BACKING: &str = "unmeasured: the table holds no memento record and the memento-selection script reads its \
     picture from an undecoded native callback";

/// Audits the discovered original table against a declared [`ScrapbookCatalog`]
/// and against `progression` (the original mission rows).
///
/// This is AC04's audit: every discovered page is counted with its artwork,
/// every declared entry is matched back to an original record, every declared
/// memento and replay link is reported against what the original actually
/// states, and a rule the original cannot back is reported as unbacked rather
/// than trusted.
#[must_use]
pub fn audit(
    discovered: &DiscoveredScrapbook,
    declared: &ScrapbookCatalog,
    progression: &Catalog,
) -> AuditReport {
    let mut original: BTreeSet<&str> = BTreeSet::new();
    let mut items_without_artwork = 0usize;
    for page in &discovered.pages {
        for item in &page.items {
            if let Some(id_key) = &item.id_key {
                original.insert(id_key.as_str());
            }
            if item.artwork.is_empty() {
                items_without_artwork += 1;
            }
        }
    }

    let missions: BTreeSet<&ContentId> = progression
        .elements()
        .filter(|element| element.kind == ContentKind::Mission)
        .map(|element| &element.id)
        .collect();

    let mut report = AuditReport {
        member_sha256: discovered.member_sha256.clone(),
        records: discovered.records,
        pages: discovered.pages.len(),
        paged_items: discovered.paged_items(),
        gaps: discovered.gaps.clone(),
        items_with_artwork: discovered.items_with_artwork(),
        items_without_artwork,
        pages_without_artwork: discovered.pages_without_artwork().len(),
        declared: 0,
        declared_matched: 0,
        declared_without_original: Vec::new(),
        undeclared_items: 0,
        unlock_known: 0,
        unlock_unknown: 0,
        unlock_unbacked: 0,
        unlock_subjects_missing: Vec::new(),
        replay_links: 0,
        replay_missions_missing: Vec::new(),
        declared_mementos: 0,
        memento_named_images: discovered.memento_named_images(),
        progression_missions: missions.len(),
        objective: discovered.objective_rules(progression),
    };

    let mut declared_keys: BTreeSet<&str> = BTreeSet::new();
    for entry in declared.entries() {
        report.declared += 1;
        if original.contains(entry.id.key()) {
            report.declared_matched += 1;
        } else {
            report.declared_without_original.push(entry.id.to_string());
        }
        declared_keys.insert(entry.id.key());
        if entry.kind == EntryKind::Memento {
            report.declared_mementos += 1;
        }
        match &entry.unlock {
            Resolved::Known(known) => {
                report.unlock_known += 1;
                // The table documents no unlock field, so no `Known` rule
                // can come from it: the rule is reported as unbacked instead
                // of being assumed original.
                report.unlock_unbacked += 1;
                let mut subjects = Vec::new();
                collect_facts(&known.value, &mut subjects);
                for fact in subjects {
                    if progression.get(&fact.subject).is_none() {
                        let id = fact.subject.to_string();
                        if !report.unlock_subjects_missing.contains(&id) {
                            report.unlock_subjects_missing.push(id);
                        }
                    }
                }
            }
            Resolved::Unknown { .. } => report.unlock_unknown += 1,
        }
        if let Some(link) = &entry.replay {
            report.replay_links += 1;
            if !missions.contains(&link.mission) {
                let id = link.mission.to_string();
                if !report.replay_missions_missing.contains(&id) {
                    report.replay_missions_missing.push(id);
                }
            }
            if let Some(variant) = &link.variant
                && !missions.contains(variant)
            {
                let id = variant.to_string();
                if !report.replay_missions_missing.contains(&id) {
                    report.replay_missions_missing.push(id);
                }
            }
        }
    }
    report.undeclared_items = original
        .iter()
        .filter(|key| !declared_keys.contains(*key))
        .count();
    report
}

/// Every fact a predicate is built from, in order.
fn collect_facts<'a>(unlock: &'a Unlock, out: &mut Vec<&'a UnlockFact>) {
    match unlock {
        Unlock::Always => {}
        Unlock::Fact(fact) => out.push(fact),
        Unlock::All(parts) | Unlock::Any(parts) => {
            for part in parts {
                collect_facts(part, out);
            }
        }
    }
}

// --------------------------------------- declared runtime catalog ----

/// The claim every unknown [`Unlock`] [`declare`] writes carries: the
/// `Objective` column is parsed but its meaning is unmeasured, so no rule
/// is declared in its place.
const CLAIM_DECLARED_UNLOCK: &str = "f47.c2.declared_scrapbook.unlock_unmeasured";

/// Why [`declare`] could not build a catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclareError {
    /// A resource-header member could not be read out of
    /// [`SCRAPBOOK_CONTAINER`]: the reason names the member, so a missing
    /// resource header is never mistaken for a missing scrapbook table.
    ResourceHeader {
        /// Why — the failing step and the member it failed on.
        reason: String,
    },
    /// A discovered item has no field the schema can carry:
    /// [`DiscoveredItem::id_key`] is `None` (the record's key is not UTF-8),
    /// so no `scrapbook_item` id names it.
    UncoveredItem {
        /// The record's raw key.
        key: String,
    },
    /// A field does not form the [`ContentId`] its role needs.
    Identity {
        /// The item the field belongs to.
        item: String,
        /// The role (`entry id`, `title` or `image`).
        role: &'static str,
        /// Why.
        reason: String,
    },
    /// The assembled entries failed [`ScrapbookCatalog`] validation — a
    /// duplicate entry id, say — which [`audit`] would then misread as a
    /// declaration gap, so the build refuses instead.
    Catalog(ScrapbookError),
}

impl fmt::Display for DeclareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceHeader { reason } => {
                write!(f, "a scrapbook resource header cannot be read: {reason}")
            }
            Self::UncoveredItem { key } => write!(
                f,
                "scrapbook item {key:?} has no UTF-8 id key, so no declared entry can name it"
            ),
            Self::Identity { item, role, reason } => write!(
                f,
                "scrapbook item {item:?}: the {role} is not a content id: {reason}"
            ),
            Self::Catalog(error) => write!(f, "the declared scrapbook catalog is invalid: {error}"),
        }
    }
}

impl std::error::Error for DeclareError {}

/// What [`declare`] built: the runtime catalog plus what the build could
/// not resolve, so the unresolved state stays inspectable rather than
/// disappearing into an id.
#[derive(Clone, Debug)]
pub struct DeclaredScrapbook {
    /// One [`ScrapbookEntry`] per [`DiscoveredItem`], in the table's own
    /// order.
    pub catalog: ScrapbookCatalog,
    /// The entries whose `TitleResID` name none of
    /// [`RESOURCE_HEADER_MEMBERS`] defines (or defines ambiguously): their
    /// `title` id keeps the unresolved name in its key instead of a
    /// string-table number.
    pub unresolved_titles: Vec<ContentId>,
}

/// The second resource header the archive holds: the member the
/// `IDS_SB_*`/`IDS_NT_*` scrapbook names are actually defined in
/// ([`ORIGINAL_RESOURCE_HEADER`] carries only 24 of them, `RESRC1.H` the
/// other 172 — measured by enumerating the container's `*.H` members).
const RESOURCE_HEADER_MEMBER_RESRC1: &str = "ASSETS/SCRIPTS/RESRC1.H";

/// The resource-header members `TitleResID`/`TextResID` names resolve
/// through: both headers the measured archive holds.
const RESOURCE_HEADER_MEMBERS: &[&str] = &[ORIGINAL_RESOURCE_HEADER, RESOURCE_HEADER_MEMBER_RESRC1];

/// Reads `member`'s decoded bytes out of
/// [`SCRAPBOOK_CONTAINER`] through the shared find + mount + member + read
/// steps ([`read_container_member`]) the scrapbook-table read also uses, so a
/// missing or unreadable header is an error, never an empty name table, and
/// the two readers cannot locate or open the archive differently.
fn resource_header_bytes(install_root: &Path, member: &str) -> Result<Vec<u8>, DeclareError> {
    let fail = |reason: String| DeclareError::ResourceHeader { reason };
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| fail(format!("the installation cannot be inventoried: {error}")))?;
    let install = cs_assets::install::fingerprint(&found.manifest);
    let read = read_container_member(install_root, install, found.manifest.files.iter(), member)
        .map_err(|failure| match failure {
            ContainerReadFailure::MissingContainer => fail(format!(
                "the installation inventories no {SCRAPBOOK_CONTAINER}, so {member} has no \
             bytes to read"
            )),
            ContainerReadFailure::MountIdentity { reason }
            | ContainerReadFailure::MemberKey { reason } => fail(reason),
            ContainerReadFailure::Mount { reason } => fail(format!(
                "{SCRAPBOOK_CONTAINER} does not mount as an ROF container: {reason}"
            )),
            ContainerReadFailure::MissingMember => fail(format!(
                "{SCRAPBOOK_CONTAINER} holds no {member} member, so the scrapbook's resource \
             names cannot be resolved"
            )),
            ContainerReadFailure::Read { reason } => {
                fail(format!("the member {member} does not decode: {reason}"))
            }
        })?;
    Ok(read.bytes)
}

/// The resource names the archive's headers define, each mapped to the id
/// its single `#define` names — the join the `TitleResID` field of a
/// discovered record needs before its name can become a
/// `string_resource/<n>` id.
///
/// The scrapbook names live mostly in `ASSETS/SCRIPTS/RESRC1.H`; the
/// measured archive holds exactly the two `*.H` members
/// [`RESOURCE_HEADER_MEMBERS`] reads. A name defined more than once across
/// them, or whose one define names no id, is absent rather than guessed:
/// [`ResourceHeader::resource_id`]'s own "missing or ambiguous → `None`"
/// rule, generalized across the headers.
///
/// # Errors
///
/// [`DeclareError::ResourceHeader`]: the archive cannot be mounted or a
/// member cannot be read or decoded — a missing header is an error, never
/// an empty name table.
pub fn resource_names(install_root: &Path) -> Result<BTreeMap<Vec<u8>, u32>, DeclareError> {
    let mut counts: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    let mut names = BTreeMap::new();
    for member in RESOURCE_HEADER_MEMBERS {
        let bytes = resource_header_bytes(install_root, member)?;
        let mut context = cs_formats::ParseContext::with_defaults(*member);
        let header = read_resource_header(&mut context, &bytes).map_err(|error| {
            DeclareError::ResourceHeader {
                reason: format!("{member}: {error}"),
            }
        })?;
        for define in header.defines() {
            *counts.entry(define.name.to_vec()).or_default() += 1;
            if let Some(id) = define.resource_id() {
                names.insert(define.name.to_vec(), id);
            }
        }
    }
    names.retain(|name, _| counts.get(name.as_slice()) == Some(&1));
    Ok(names)
}

/// Declares the runtime [`ScrapbookCatalog`] the discovered original table
/// states: one [`ScrapbookEntry`] per [`DiscoveredItem`], keyed by
/// `scrapbook_item/<install_file_key(entry_key)>` so it joins F14-D.8's
/// baseline rows and this stage's discovery.
///
/// `resource_id` resolves a `TitleResID` name to its string-table number —
/// the production call is [`resource_names`] over the original
/// installation's own headers; a caller without headers passes a resolver
/// that answers `None` and every named title lands in
/// [`DeclaredScrapbook::unresolved_titles`].
///
/// Only what the bytes state is declared:
///
/// * `title` — a decimal `TitleResID` (the table's `"0"` spelling, and an
///   empty field as the same null resource id) becomes
///   `string_resource/<n>`; an `IDS_*` name `resource_id` answers becomes
///   `string_resource/<n>`; a name it does not answer keeps the name
///   itself escaped in the key — the dead reference is the name the table
///   stated, and it is reported in [`DeclaredScrapbook::unresolved_titles`].
/// * `image` — the item's preferred capture member ([`DiscoveredItem::capture`]),
///   or `None` when the installation holds no decodable picture: the state
///   the archive carries, never a placeholder.
/// * `unlock` — [`Resolved::Unknown`] under [`CLAIM_DECLARED_UNLOCK`]:
///   `Objective` is parsed but its meaning is unmeasured, so no rule is
///   invented, and an unknown rule never unlocks.
/// * `kind` — [`EntryKind::Page`] for every record: the `Objective`
///   discriminator's meaning is unmeasured, so no record is read as a
///   stunt photo, kill trophy or memento. The table holds no memento
///   records ([`ORIGINAL_MEMENTO_RECORDS`]), so none are declared.
/// * `visibility` — [`EntryVisibility::Shown`]: whether the original hides
///   locked entries is unmeasured; the visible-but-never-unlocked state is
///   what the table proves.
/// * `replay` — `None`: the table carries no measured replay-mission
///   field ([`ORIGINAL_REPLAY_FIELDS`]).
/// * `text` — the `TextResID` field has no slot in [`ScrapbookEntry`]'s
///   schema and is not carried; `docs/findings/` records that limitation.
///
/// # Errors
///
/// [`DeclareError`]: an item's key has no id the schema covers
/// ([`DiscoveredItem::id_key`] is `None`), a field does not form the id
/// its role needs, or the assembled entries fail [`ScrapbookCatalog`]
/// validation.
pub fn declare(
    discovered: &DiscoveredScrapbook,
    resource_id: &impl Fn(&[u8]) -> Option<u32>,
) -> Result<DeclaredScrapbook, DeclareError> {
    let unlock_claim =
        ClaimId::new(CLAIM_DECLARED_UNLOCK).expect("the claim constant is a valid claim id");
    let unlock_reason = "the table's Objective column is parsed but its meaning is unmeasured, \
                         so no unlock rule is declared";

    let mut entries = Vec::new();
    let mut unresolved_titles = Vec::new();
    for item in discovered.items() {
        let identity =
            |role: &'static str, error: cs_types::content::ContentIdError| DeclareError::Identity {
                item: item.key.clone(),
                role,
                reason: error.to_string(),
            };
        let id_key = item
            .id_key
            .as_deref()
            .ok_or_else(|| DeclareError::UncoveredItem {
                key: item.key.clone(),
            })?;
        let id = ContentId::from_source(ContentKind::ScrapbookItem, id_key)
            .map_err(|error| identity("entry id", error))?;

        let field = item.title.as_str();
        let (title_key, resolved) = if field.is_empty() {
            ("0".to_owned(), true)
        } else if let Ok(number) = field.parse::<u32>() {
            (number.to_string(), true)
        } else if let Some(number) = resource_id(field.as_bytes()) {
            (number.to_string(), true)
        } else {
            (install_file_key(field), false)
        };
        let title = ContentId::from_source(ContentKind::StringResource, &title_key)
            .map_err(|error| identity("title", error))?;
        if !resolved {
            unresolved_titles.push(id.clone());
        }

        let image = item
            .capture
            .as_ref()
            .map(|member| ContentId::from_source(ContentKind::Image, &install_file_key(member)))
            .transpose()
            .map_err(|error| identity("image", error))?;

        let unlock = Resolved::unknown(unlock_claim.clone(), unlock_reason)
            .expect("the reason is a non-empty constant");

        entries.push(ScrapbookEntry {
            id,
            kind: EntryKind::Page,
            title,
            image,
            unlock,
            visibility: EntryVisibility::Shown,
            replay: None,
        });
    }

    let catalog = ScrapbookCatalog::new(entries).map_err(DeclareError::Catalog)?;
    Ok(DeclaredScrapbook {
        catalog,
        unresolved_titles,
    })
}

/// A JSON string literal: quoted and escaped, so no identifier can break out
/// of its string.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// A map of counter to count as a JSON object.
fn map_json(counts: &BTreeMap<String, u32>) -> String {
    let items: Vec<String> = counts
        .iter()
        .map(|(code, count)| format!("{}: {count}", quote(code)))
        .collect();
    format!("{{{}}}", items.join(", "))
}

/// A list of strings as a JSON array.
fn string_array(values: &[String]) -> String {
    values
        .iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(", ")
}
