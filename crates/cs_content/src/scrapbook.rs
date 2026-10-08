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
//! `ASSETS/SCRAPBOOK.CSV` member through the production ROF mount and the
//! production keyed-list reader, groups the records it holds into
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};
use cs_types::content::{ContentId, ContentKind, Resolved};

use crate::catalog::Catalog;
use crate::catalog::baseline::{SCRAPBOOK_CONTAINER, SCRAPBOOK_MEMBER, install_file_key};
use crate::config::{ConfigDocument, RecordSchema, RecordView};

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
/// only one that could carry progression, and what it *means* is unmeasured:
/// the audit records its value distribution and refuses to read it as a rule.
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
    /// The `Objective` field, **whose meaning is unmeasured**: the number is
    /// converted because the position's kind is measured (F12-I), and no rule
    /// is read out of it here.
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
        let record = found
            .manifest
            .files
            .iter()
            .find(|record| {
                record
                    .relative_spelling
                    .as_str()
                    .eq_ignore_ascii_case(SCRAPBOOK_CONTAINER)
            })
            .ok_or(ScrapbookSourceError::MissingContainer)?;
        let path = install_root.join(record.relative_spelling.as_str());

        let mut builder = SessionBuilder::new(ResolveContext::new(install));
        let mount = MountBuilder::new(
            MountId::new("rof-scrapbook").map_err(|error| ScrapbookSourceError::Mount {
                reason: error.to_string(),
            })?,
            MountNamespace::new(INSTALL_NAMESPACE).map_err(|error| {
                ScrapbookSourceError::Mount {
                    reason: error.to_string(),
                }
            })?,
            PrecedenceClass::Shared,
            SCRAPBOOK_CONTAINER,
        )
        .retail();
        let source = mount_rof_into(&mut builder, mount, &path).map_err(|error| {
            ScrapbookSourceError::Mount {
                reason: error.to_string(),
            }
        })?;

        let key = AssetKey::from_spelling(source.namespace().as_str(), SCRAPBOOK_MEMBER, "default")
            .map_err(|error| ScrapbookSourceError::Document {
                reason: error.to_string(),
            })?;
        let info = source
            .member(&key)
            .cloned()
            .ok_or(ScrapbookSourceError::MissingMember)?;
        let read = source
            .read(&key)
            .map_err(|error| ScrapbookSourceError::Read {
                reason: error.to_string(),
            })?;
        let bytes = read.data.as_slice();

        // The artwork side: every member of the container, keyed by its file
        // stem, so an item's picture is found by name rather than by a path
        // this stage invented.
        let mut stems: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for member in source.members() {
            let spelling = &member.spelling;
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

        let span = cs_types::asset_id::SourceSpan::new(
            install,
            SCRAPBOOK_CONTAINER,
            Some(SCRAPBOOK_MEMBER),
            info.offset,
            info.declared_decoded_len,
            Some(cs_assets::install::sha256(bytes)),
        )
        .map_err(|error| ScrapbookSourceError::Span {
            reason: error.to_string(),
        })?;
        let mut context = cs_formats::ParseContext::with_defaults(SCRAPBOOK_CONTAINER);
        let document = ConfigDocument::read(&mut context, span, bytes).map_err(|error| {
            ScrapbookSourceError::Document {
                reason: error.to_string(),
            }
        })?;

        let mut gaps: BTreeMap<String, u32> = BTreeMap::new();
        let bump = |gaps: &mut BTreeMap<String, u32>, code: &str, by: u32| {
            *gaps.entry(code.to_owned()).or_default() += by;
        };

        // A key the table declares twice names one item twice: records that
        // agree are one item with the repeat counted, records that disagree
        // are reported with neither, because nothing in the table says which
        // one the engine reads (the rule F14-D.8 measured for the same
        // member).
        let mut grouped: BTreeMap<Vec<u8>, Vec<&crate::config::ConfigEntry>> = BTreeMap::new();
        for entry in document.entries() {
            if RecordSchema::for_entry(entry) != Some(RecordSchema::Scrapbook) {
                bump(&mut gaps, "entry_not_a_scrapbook_item", 1);
                continue;
            }
            grouped.entry(entry.key.clone()).or_default().push(entry);
        }
        if grouped.is_empty() {
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
        for (raw_key, group) in grouped {
            let Some(first) = group.first().copied() else {
                continue;
            };
            if group.iter().any(|entry| entry.value != first.value) {
                bump(&mut gaps, "ambiguous_entry_key", group.len() as u32);
                continue;
            }
            if group.len() > 1 {
                bump(&mut gaps, "duplicate_entry_key", (group.len() - 1) as u32);
            }
            records += 1;
            let Ok(key) = String::from_utf8(raw_key.clone()) else {
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
            member_sha256: cs_assets::install::sha256(bytes).to_hex(),
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
            quote(UNLOCK_BACKING),
            quote(REPLAY_BACKING),
            quote(MEMENTO_BACKING),
        )
    }
}

/// What this stage measured about the original's unlock path: the table
/// documents no unlock field, and the one field whose documented name could
/// carry progression (`Objective`) has a measured value distribution and an
/// unmeasured meaning.
pub const UNLOCK_BACKING: &str = "unmeasured: the table documents no unlock field; the Objective field's meaning is unknown, \
     so a Known rule is reported as not original-backed";

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
