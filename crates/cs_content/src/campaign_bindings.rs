//! Engine-independent mission binding and campaign coverage records (F50-A).
//!
//! Spec F50, stage `### F50-A`, and the owner ruling of 2026-09-28
//! (`specs/README.md`, "Owner ruling 2026-09-28"): define the *complete*
//! mission binding/coverage records early, without waiting for every
//! gameplay subsystem's runtime schema. Each required subsystem appears as
//! a stable identity plus an explicit unresolved dependency row, so a
//! mission can be recorded — and counted — long before the subsystem that
//! will eventually satisfy it exists.
//!
//! The shared contract for mission execution is
//! `docs/contracts/SCRIPT-MISSION.md`; this module deliberately stops
//! *before* it. Nothing here parses original bytes, normalizes original
//! fields, runs a mission or claims gameplay success. The records are new
//! engine vocabulary (`docs/contracts/IDENTITY-CONTENT.md` calls a kind
//! "engine-authored vocabulary, not an observed retail label"), they carry
//! [`Provenance`] for every known value and an explicit unknown for every
//! value nobody has bound yet.
//!
//! Four ideas carry the stage:
//!
//! * **[`MissionLabel`]** is a work-order discovery label (`M01` … `M24`).
//!   It is *not* a retail identity: the catalog mission id lives in the
//!   record's `mission` row and stays [`Resolved::Unknown`] until a
//!   binding task reads it from original data. Labels are uppercase-only so
//!   `m01` can never be a second, contradictory spelling of `M01`.
//! * **[`BindingCategory`]** is the closed set of required content
//!   categories the owner ruling preserves: mission identity/world/program,
//!   actors and forced airframes, assets/media, objectives/branches/failure
//!   causes, interactions, rewards/progression and reference/evidence
//!   links. A category that is not recorded is [`CellState::Missing`], a
//!   category or row that is explicitly unknown is [`CellState::Unknown`]
//!   and an explicitly unsupported one is [`CellState::Unsupported`]. None
//!   of the three is "unused" and none of the three is ready.
//! * **[`SubsystemDependency`]** represents one required subsystem
//!   ([`REQUIRED_SUBSYSTEMS`], the F50 prerequisite list) by its stable
//!   identity and a [`DependencyState`] row. Every mission must carry every
//!   required row — an empty dependency list can never read as "nothing
//!   left to do".
//! * **[`CampaignBindings`]`::coverage`** fixes the denominator from the
//!   declared inventory, counts every `(mission, category)` cell exactly
//!   once and reports discovered missions beyond the denominator instead
//!   of hiding them. Nothing removes a declared mission; [`CellState`]s
//!   only accumulate.
//!
//! The declared inventory is data, not a Rust constant: it lives in
//! `missions/bindings/campaign-inventory.tsv` and is read with the small
//! strict reader in [`CampaignInventory`]. `missions/README.md` lists the
//! same work orders, and `crates/cs_app/tests/campaign/` asserts the two
//! agree, so the denominator cannot shrink without a failing test.
//!
//! ## Source-derived binding
//!
//! The records above are schema: [`SourceContext`] and [`SourceBinding`]
//! are the first stage that reads original data. `SourceContext::read`
//! fingerprints `$CS_GAME_DIR`, walks its `ZBD/<chapter><variant>/<mission>`
//! campaign layout and loads the localized UI string table;
//! `SourceContext::bind` then confirms one work order's discovery title
//! against the local strings and resolves the five [`CriticalDependency`]
//! anchors of the mission sheets' data-binding checklist. The result is the
//! `missions/bindings/M01.json` record (`schemas/mission-binding.schema.json`)
//! and the [`MissionBinding`] the campaign records consume.
//!
//! Two states stay deliberately apart. **Source-derived** means every
//! critical dependency resolved, which is what the `M01-A` acceptance
//! scenario asks for. **Verified** ([`SourceBinding::is_verified`]) means
//! nothing at all is left unknown: the checklist entries this stage does
//! not bind — actors, objectives, media, rewards, difficulty branches,
//! precedence, progression and the dependency closure hash — stay in
//! [`SourceBinding::unknowns`] and keep `verified` false. A binding can
//! therefore be source-derived *and* honestly incomplete at the same time.
//!
//! The join from a work-order title to a retail mission directory is an
//! inference, not a direct observation: it rests on the localized title
//! block's position matching the campaign the directory layout declares. It
//! is recorded with [`ClaimStatus::Inferred`] provenance, never as
//! `verified_original` (AGENTS.md rule 8).
//!
//! A single block of the right length is weak evidence for that join — any
//! `campaign.len()` unrelated strings would form one. [`SourceContext`]
//! therefore checks the join against the second structure the installation
//! offers: the region-prefixed long mission names, whose row *grouping* must
//! fall into the layout's per-chapter sizes ([`SourceContext::chapter_sizes`],
//! [`SourceContext::join_agreement`], [`classify_join`]). Only the grouping is
//! used, never the name a group carries, because nothing establishes what a
//! region name means. An installation that offers no such block is reported
//! [`JoinCorroboration::Unavailable`] and keeps the inference unchallenged;
//! one whose grouping contradicts the layout is [`JoinCorroboration::Disagreed`]
//! and yields no campaign position at all, so a binding can never read as
//! resolved while the two structures disagree about the campaign.
//!
//! The installation also carries the same 24 missions a second time as bare
//! short names, and the two campaign-length runs name the same position in the
//! same order. [`SourceContext::join_agreement`] measures that row-to-row
//! correspondence as a further check ([`blocks_correspond`],
//! [`classify_correspondence`], [`join_state`]): every pair of
//! campaign-length blocks must agree by a mutual strict argmax of shared
//! content tokens — a rule that holds for all 24 retail rows and rejects every
//! non-zero rotation, and that is not tuned to any one spelling because it
//! compares no whole string. A pair that does not correspond is a
//! [`JoinCorroboration::Disagreed`] exactly like a contradicting grouping, so
//! the join is refused, not merely doubted.
//!
//! The decision itself is [`campaign_position_for`], a pure function over a
//! confirmed row and that agreement, so every arm — including the
//! contradiction no retail installation produces — is reachable without an
//! installation and each refusal names its own cause instead of leaving an
//! identity blank.
//!
//! ## How a localized row carries a discovery title
//!
//! The work-order titles in `missions/README.md` are *discovery labels* from a
//! public guide, and the installation does not always spell a mission the way
//! the guide does. The campaign-length mission-name rows come in two observed
//! display forms ([`TitleForm`]): a bare short name, and a long name prefixed
//! with a region and a `" - "` separator. A title is therefore confirmed by
//! [`SourceContext::confirm_title`] in exactly one of two ways — verbatim, or
//! as the title part of a region-prefixed long name — and the verbatim form
//! wins whenever it is available, so a mission the guide spells correctly is
//! bound to the row that carries it exactly. No fuzzy comparison happens at
//! either end: the tail must equal the title byte for byte, and the region
//! prefix is dropped without ever being compared with or interpreted as
//! anything.
//!
//! Where the two forms disagree about the same mission — the short name of
//! campaign position 4 omits the leading article its long name and the
//! declared title carry — the binding does not choose between them. It records
//! the second spelling in [`SourceBinding::unknowns`] as measured, with the
//! rows it read, and keeps `verified` false.
//!
//! ## The whole campaign (F50-B)
//!
//! `SourceContext::bind` answers for one work order; [`SourceContext::bind_campaign`]
//! answers for the declared denominator: one read of the installation, one bind
//! per declared work order in inventory order, and one assembly over the frozen
//! denominator ([`assemble_campaign`]). What comes back is a [`BoundCampaign`]
//! — the [`CampaignBindings`] record the coverage and closure reports are
//! measured against, beside the [`SourceBinding`]s it was derived from — so
//! profile continuity, campaign position and byte ranges stay readable without
//! deriving the campaign twice.
//!
//! Two things this stage deliberately does **not** do, because neither is
//! measured: it does not resolve a work order whose discovery title the local
//! strings carry in neither display form (such a mission is recorded with an
//! explicitly unresolved identity, never dropped and never guessed at), and it
//! does not bind the campaign *progression*. The successor relation between
//! missions lives in original scripts that have not been measured, so every
//! assembled mission keeps [`Progression::Unknown`] and the campaign is never
//! ready on this stage's evidence alone.
//!
//! ## Per-mission probe routes (F50-C)
//!
//! Stage F50-C turns the bound campaign into the thing every later mission
//! stage plugs into: one **probe route** per declared work order
//! ([`probe_routes`]). A route is the checkable retry contract of spec F50
//! acceptance test AC03 — "retry selected missions after death, bailout,
//! skip-media, save/restart and settings changes" ([`ProbeInterruption::ALL`]
//! is exactly that minimum scenario) — anchored to the mission identities the
//! installation resolved for the work order and to the one installation
//! fingerprint the whole campaign was read under.
//!
//! What a route states is narrow and deliberately so: *whichever* of the five
//! interruptions ends a run of this mission, the next entry re-enters the
//! **same** mission, world and program, under the **same** installation
//! fingerprint ([`ProbeReentry`]). A work order whose identity did not resolve
//! has no identity to re-enter, so its route is *refused* with the reason —
//! it stays in the plan, is counted and can never read as probed
//! ([`MissionProbeRoute::is_ready`]).
//!
//! What it does **not** state is what the mission runtime does not yet do: no
//! mission is played here, no runtime death is observed and no retry has been
//! executed. Executing a route needs the mission launch path
//! (`VS-M01-RUNTIME`) and the controlled runs (`VS-M01-CONTROLLED-RUNS`);
//! this module supplies the plan those consumers drive, and the acceptance
//! suite verifies the plan against the installation it was derived from.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved, ResolvedError};
use cs_types::evidence::{ClaimId, ClaimIdError, ClaimStatus, ContentHash};

use crate::catalog::Catalog;
use crate::config::{StringCatalog, StringRow};

/// The identities of the subsystems every mission binding depends on.
///
/// One entry per prerequisite feature of spec F50
/// (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
/// "Prerequisite features"). The identity is the feature stage id, which is
/// stable across refactors and traceable back to the sheet; which runtime
/// type eventually satisfies it is deliberately *not* recorded here, so the
/// record does not change shape when a subsystem lands.
pub const REQUIRED_SUBSYSTEMS: &[&str] = &[
    "F18", "F19", "F20", "F24", "F25", "F27", "F28", "F29", "F32", "F33", "F34", "F35", "F36",
    "F38", "F39", "F40", "F41", "F42", "F43", "F44", "F45", "F46", "F47",
];

/// The row role of a mission's own catalog identity inside
/// [`BindingCategory::MissionIdentity`].
pub const MISSION_ROLE: &str = "mission";
/// The row role of the world variant inside
/// [`BindingCategory::MissionIdentity`].
pub const WORLD_ROLE: &str = "world";
/// The row role of the mission program inside
/// [`BindingCategory::MissionIdentity`].
pub const PROGRAM_ROLE: &str = "program";

/// The three identity rows [`BindingCategory::MissionIdentity`] is checked
/// against, and the catalog kind each content target must have.
///
/// These are engine-authored kinds (`ContentKind`), not observed retail
/// labels: they say what a row *means*, never which original file it came
/// from.
const IDENTITY_ROLE_KINDS: &[(&str, ContentKind)] = &[
    (MISSION_ROLE, ContentKind::Mission),
    (WORLD_ROLE, ContentKind::World),
    (PROGRAM_ROLE, ContentKind::Script),
];

/// Column separator of `missions/bindings/campaign-inventory.tsv`.
pub const INVENTORY_DELIMITER: char = '\t';
/// A line starting with this character carries no mission.
pub const INVENTORY_COMMENT: char = '#';

/// The claim id a declared-but-unbound placeholder carries.
const PLACEHOLDER_CLAIM: &str = "f50.a.declared_inventory";
/// Why a declared-but-unbound mission's categories are unknown.
const PLACEHOLDER_CATEGORY_REASON: &str = "mission not yet bound to original data";
/// Why a declared-but-unbound mission's subsystem rows are unresolved.
const PLACEHOLDER_DEPENDENCY_REASON: &str =
    "subsystem dependency not yet resolved for this mission";
/// Why a declared-but-unbound mission's progression is unknown.
const PLACEHOLDER_PROGRESSION_REASON: &str =
    "campaign progression not yet bound to original mission ids";

/// Why a label or identity string was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelError {
    /// The string was empty.
    Empty,
    /// The string exceeded `max` bytes.
    TooLong {
        /// How long the string was.
        len: usize,
        /// The limit it broke.
        max: usize,
    },
    /// The string contained a character outside its grammar.
    BadCharacter {
        /// The offending character.
        ch: char,
    },
}

impl fmt::Display for LabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "identity must not be empty"),
            Self::TooLong { len, max } => write!(f, "identity is {len} bytes, max is {max}"),
            Self::BadCharacter { ch } => write!(f, "identity contains disallowed character {ch:?}"),
        }
    }
}

impl std::error::Error for LabelError {}

/// Validates an uppercase identity: `[A-Z0-9._-]`, first character
/// alphanumeric, at most `max` bytes.
///
/// Uppercase-only is deliberate. Two spellings of one mission would be two
/// rows with one meaning, and a duplicate identity must be refused rather
/// than reconciled (`IDENTITY-CONTENT`).
fn validate_upper_identity(text: &str, max: usize) -> Result<(), LabelError> {
    if text.is_empty() {
        return Err(LabelError::Empty);
    }
    if text.len() > max {
        return Err(LabelError::TooLong {
            len: text.len(),
            max,
        });
    }
    for (index, ch) in text.char_indices() {
        let allowed =
            ch.is_ascii_uppercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-');
        if !allowed {
            return Err(LabelError::BadCharacter { ch });
        }
        if index == 0 && !ch.is_ascii_alphanumeric() {
            return Err(LabelError::BadCharacter { ch });
        }
    }
    Ok(())
}

/// The discovery label of a mission work order: `M01` … `M24`.
///
/// A label identifies a *work order*, never a retail mission. Spec F50
/// non-negotiable behavior 1: "Titles in this pack are discovery labels
/// until matched to localized original ids." The catalog identity a mission
/// is actually bound to lives in its [`BindingCategory::MissionIdentity`]
/// `mission` row and starts [`Resolved::Unknown`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MissionLabel(String);

impl MissionLabel {
    /// Maximum byte length of a label.
    pub const MAX_LEN: usize = 32;

    /// Validates and wraps a work-order label.
    ///
    /// # Errors
    ///
    /// [`LabelError`] when the label is empty, longer than
    /// [`MissionLabel::MAX_LEN`] or contains anything outside
    /// `[A-Z0-9._-]`.
    pub fn new(text: &str) -> Result<Self, LabelError> {
        validate_upper_identity(text, Self::MAX_LEN)?;
        Ok(Self(text.to_owned()))
    }

    /// The label as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MissionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The stable identity of one required subsystem.
///
/// Same grammar as [`MissionLabel`] with a tighter limit: it names a spec
/// stage (`F39`), nothing else.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubsystemId(String);

impl SubsystemId {
    /// Maximum byte length of a subsystem identity.
    pub const MAX_LEN: usize = 16;

    /// Validates and wraps a subsystem identity.
    ///
    /// # Errors
    ///
    /// [`LabelError`] when the identity is empty, longer than
    /// [`SubsystemId::MAX_LEN`] or contains anything outside `[A-Z0-9._-]`.
    pub fn new(text: &str) -> Result<Self, LabelError> {
        validate_upper_identity(text, Self::MAX_LEN)?;
        Ok(Self(text.to_owned()))
    }

    /// The identity as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SubsystemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One required content category of a mission binding.
///
/// The set is closed and comes from the F50 owner ruling of 2026-09-28:
/// "Preserve every required content category: mission identity, world
/// variant and program; actors and forced/captured airframes; assets and
/// media; objectives, branches and failure causes; interactions; rewards
/// and progression; reference and evidence links." A mission binding that
/// does not record one of them has a [`CellState::Missing`] cell, which
/// counts in the totals and blocks readiness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingCategory {
    /// The mission's catalog identity, its world variant and its program.
    MissionIdentity,
    /// The mission's actors and its forced or captured airframes.
    Actors,
    /// The assets the mission loads and the media it plays.
    AssetsMedia,
    /// The objectives, their branches and the mission's failure causes.
    Objectives,
    /// The interactions the mission authorizes.
    Interactions,
    /// The rewards the mission grants and the progression it advances.
    RewardsProgression,
    /// The reference and evidence links the binding rests on.
    ReferenceEvidence,
}

impl BindingCategory {
    /// Every required category, in report order.
    pub const ALL: &'static [BindingCategory] = &[
        Self::MissionIdentity,
        Self::Actors,
        Self::AssetsMedia,
        Self::Objectives,
        Self::Interactions,
        Self::RewardsProgression,
        Self::ReferenceEvidence,
    ];

    /// The stable label used in ids and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::MissionIdentity => "mission_identity",
            Self::Actors => "actors",
            Self::AssetsMedia => "assets_media",
            Self::Objectives => "objectives",
            Self::Interactions => "interactions",
            Self::RewardsProgression => "rewards_progression",
            Self::ReferenceEvidence => "reference_evidence",
        }
    }

    /// The stable label back to a category, if the label names one.
    ///
    /// The table is [`BindingCategory::ALL`], so `label` and `from_label`
    /// cannot disagree about a category.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|category| category.label() == label)
    }

    /// The row roles this category must contain before its cell can be
    /// complete.
    ///
    /// Only [`BindingCategory::MissionIdentity`] names sub-fields in the
    /// owner ruling ("mission identity, world variant and program"). The
    /// other categories are open: their rows are discovered from original
    /// data, so the vocabulary of an `actors` or `objectives` row is
    /// deliberately not frozen here.
    pub const fn required_roles(self) -> &'static [&'static str] {
        match self {
            Self::MissionIdentity => &[MISSION_ROLE, WORLD_ROLE, PROGRAM_ROLE],
            _ => &[],
        }
    }
}

impl fmt::Display for BindingCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What one binding row points at.
///
/// A row is either a reference to another catalog element — typed by
/// [`ContentId`], which validates its own namespace at construction — or a
/// reference to an evidence claim in the F01 ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingTarget {
    /// A stable content identity.
    Content(ContentId),
    /// An evidence claim the row rests on.
    Evidence(ClaimId),
}

impl fmt::Display for BindingTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Content(id) => write!(f, "content:{id}"),
            Self::Evidence(claim) => write!(f, "evidence:{claim}"),
        }
    }
}

/// Why a row was rejected when built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingError {
    /// A label or subsystem identity broke its grammar.
    Label(LabelError),
    /// A claim id broke its grammar.
    ClaimId(ClaimIdError),
    /// A row role was empty, too long or outside its grammar.
    InvalidRole {
        /// What was wrong with the role.
        error: LabelError,
    },
    /// A value that must carry a reason carried none (or a blank one).
    EmptyReason {
        /// Which field was blank.
        what: &'static str,
    },
    /// A known target could not be built.
    Resolved(ResolvedError),
    /// A row list was empty. An empty list is an absent category, not a
    /// category with no children.
    EmptyRows,
    /// A mission with this label is already recorded. Two contradictory
    /// rows are never merged.
    DuplicateId {
        /// The repeated label.
        label: MissionLabel,
    },
    /// The mission is not in the set, so it cannot be declared or bound.
    UnknownMission {
        /// The missing label.
        label: MissionLabel,
    },
    /// The mission is already bound, so its placeholder cannot be replaced
    /// again.
    AlreadyBound {
        /// The already-bound label.
        label: MissionLabel,
    },
    /// A required category recorded no rows at all.
    EmptyCategory {
        /// The category that is empty.
        category: BindingCategory,
    },
    /// The required subsystem rows do not cover [`REQUIRED_SUBSYSTEMS`].
    MissingSubsystem {
        /// The subsystem with no row.
        subsystem: SubsystemId,
    },
    /// A subsystem row names an identity outside [`REQUIRED_SUBSYSTEMS`].
    UnknownSubsystem {
        /// The unknown subsystem identity.
        subsystem: SubsystemId,
    },
    /// A mission carries the same subsystem row twice.
    DuplicateSubsystem {
        /// The repeated subsystem identity.
        subsystem: SubsystemId,
    },
    /// An identity row points at a catalog element of the wrong kind.
    WrongKind {
        /// The row role that was wrong.
        role: String,
        /// The kind the role means.
        expected: ContentKind,
        /// The kind the row actually carries.
        found: ContentKind,
    },
    /// A progression lists the same successor twice.
    DuplicateProgression {
        /// The repeated successor.
        label: MissionLabel,
    },
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Label(error) => write!(f, "{error}"),
            Self::ClaimId(error) => write!(f, "{error}"),
            Self::InvalidRole { error } => write!(f, "invalid row role: {error}"),
            Self::EmptyReason { what } => write!(f, "{what} must carry a reason"),
            Self::Resolved(error) => write!(f, "{error}"),
            Self::EmptyRows => write!(f, "a row list must not be empty"),
            Self::DuplicateId { label } => {
                write!(
                    f,
                    "a mission binding with label {label} is already recorded"
                )
            }
            Self::UnknownMission { label } => write!(f, "no mission binding has label {label}"),
            Self::AlreadyBound { label } => {
                write!(
                    f,
                    "mission {label} is already bound; a placeholder can be bound once"
                )
            }
            Self::EmptyCategory { category } => {
                write!(
                    f,
                    "category {} recorded no rows; use an explicit unresolved state instead",
                    category.label()
                )
            }
            Self::MissingSubsystem { subsystem } => {
                write!(
                    f,
                    "every mission must carry a dependency row for {subsystem}"
                )
            }
            Self::UnknownSubsystem { subsystem } => {
                write!(f, "{subsystem} is not one of the required subsystems")
            }
            Self::DuplicateSubsystem { subsystem } => {
                write!(f, "dependency row {subsystem} is recorded twice")
            }
            Self::WrongKind {
                role,
                expected,
                found,
            } => write!(
                f,
                "identity row {role} must be a {expected}, found a {found}"
            ),
            Self::DuplicateProgression { label } => {
                write!(f, "progression lists mission {label} twice")
            }
        }
    }
}

impl std::error::Error for BindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Label(error) => Some(error),
            Self::ClaimId(error) => Some(error),
            Self::InvalidRole { error } => Some(error),
            Self::Resolved(error) => Some(error),
            Self::EmptyReason { .. }
            | Self::EmptyRows
            | Self::DuplicateId { .. }
            | Self::UnknownMission { .. }
            | Self::AlreadyBound { .. }
            | Self::EmptyCategory { .. }
            | Self::MissingSubsystem { .. }
            | Self::UnknownSubsystem { .. }
            | Self::DuplicateSubsystem { .. }
            | Self::WrongKind { .. }
            | Self::DuplicateProgression { .. } => None,
        }
    }
}

impl From<LabelError> for BindingError {
    fn from(error: LabelError) -> Self {
        Self::Label(error)
    }
}

impl From<ClaimIdError> for BindingError {
    fn from(error: ClaimIdError) -> Self {
        Self::ClaimId(error)
    }
}

impl From<ResolvedError> for BindingError {
    fn from(error: ResolvedError) -> Self {
        Self::Resolved(error)
    }
}

/// One row of one required category of one mission.
///
/// The role is an open, engine-authored label (`world`, `forced_airframe`,
/// `failure_cause`, …): which roles original data needs is discovered by
/// the per-mission binding tasks, so the vocabulary is not frozen while the
/// *categories* are. The target is always a [`Resolved`], so a row nobody
/// has bound yet is an explicit unknown with a claim id and a reason —
/// never an empty string standing in for a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingRow {
    role: String,
    target: Resolved<BindingTarget>,
}

impl BindingRow {
    /// Maximum byte length of a row role.
    pub const MAX_ROLE_LEN: usize = 64;

    /// Builds a row from a role and an already-resolved target.
    ///
    /// # Errors
    ///
    /// [`BindingError::InvalidRole`] for an empty, over-long or
    /// out-of-grammar role.
    pub fn new(role: &str, target: Resolved<BindingTarget>) -> Result<Self, BindingError> {
        validate_role(role).map_err(|error| BindingError::InvalidRole { error })?;
        Ok(Self {
            role: role.to_owned(),
            target,
        })
    }

    /// A row bound to a catalog element with provenance.
    ///
    /// # Errors
    ///
    /// [`BindingError::InvalidRole`] for a bad role.
    pub fn content(
        role: &str,
        id: ContentId,
        provenance: Provenance,
    ) -> Result<Self, BindingError> {
        Self::new(
            role,
            Resolved::Known(Known::new(BindingTarget::Content(id), provenance)),
        )
    }

    /// A row resting on an evidence claim.
    ///
    /// # Errors
    ///
    /// [`BindingError::InvalidRole`] for a bad role.
    pub fn evidence(
        role: &str,
        claim: ClaimId,
        provenance: Provenance,
    ) -> Result<Self, BindingError> {
        Self::new(
            role,
            Resolved::Known(Known::new(BindingTarget::Evidence(claim), provenance)),
        )
    }

    /// An explicitly unknown row: the binding knows what is missing and
    /// why, and invents nothing in its place.
    ///
    /// # Errors
    ///
    /// [`BindingError::InvalidRole`] for a bad role, or
    /// [`BindingError::Resolved`] when `reason` is blank.
    pub fn unknown(role: &str, claim: ClaimId, reason: &str) -> Result<Self, BindingError> {
        Self::new(role, Resolved::unknown(claim, reason)?)
    }

    /// The row's role.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// The row's target: a value with provenance, or an explicit unknown.
    pub fn target(&self) -> &Resolved<BindingTarget> {
        &self.target
    }

    /// Whether the row is bound to a value.
    pub fn is_known(&self) -> bool {
        self.target.is_known()
    }

    /// The content identity, when the row is known and points at content.
    pub fn content_target(&self) -> Option<&ContentId> {
        match &self.target {
            Resolved::Known(known) => match &known.value {
                BindingTarget::Content(id) => Some(id),
                BindingTarget::Evidence(_) => None,
            },
            Resolved::Unknown { .. } => None,
        }
    }

    /// The evidence claim, when the row is known and points at evidence.
    pub fn evidence_target(&self) -> Option<&ClaimId> {
        match &self.target {
            Resolved::Known(known) => match &known.value {
                BindingTarget::Content(_) => None,
                BindingTarget::Evidence(claim) => Some(claim),
            },
            Resolved::Unknown { .. } => None,
        }
    }
}

/// A row role is a short engine-authored label: `[A-Za-z0-9._:-]`, never
/// empty.
fn validate_role(role: &str) -> Result<(), LabelError> {
    if role.is_empty() {
        return Err(LabelError::Empty);
    }
    if role.len() > BindingRow::MAX_ROLE_LEN {
        return Err(LabelError::TooLong {
            len: role.len(),
            max: BindingRow::MAX_ROLE_LEN,
        });
    }
    for ch in role.chars() {
        if !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-')) {
            return Err(LabelError::BadCharacter { ch });
        }
    }
    Ok(())
}

/// What one mission recorded for one required category.
///
/// Absence of the variant from a binding's map is [`CellState::Missing`];
/// everything here is explicit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryState {
    /// At least one row. Rows may individually be unknown, which makes the
    /// whole cell [`CellState::Unknown`].
    Rows(Vec<BindingRow>),
    /// The category is explicitly unresolved for this mission: nobody has
    /// bound it yet and the record says so.
    Unresolved {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why the category is unresolved.
        reason: String,
    },
    /// The engine cannot support this category for this mission today.
    Unsupported {
        /// Why the category is unsupported.
        reason: String,
    },
}

impl CategoryState {
    /// A category of rows.
    ///
    /// # Errors
    ///
    /// [`BindingError::EmptyRows`] when `rows` is empty: an empty list is
    /// not a category, it is an absent one.
    pub fn rows(rows: Vec<BindingRow>) -> Result<Self, BindingError> {
        if rows.is_empty() {
            return Err(BindingError::EmptyRows);
        }
        Ok(Self::Rows(rows))
    }

    /// An explicitly unresolved category.
    ///
    /// # Errors
    ///
    /// [`BindingError::EmptyReason`] when `reason` is blank.
    pub fn unresolved(claim_id: ClaimId, reason: &str) -> Result<Self, BindingError> {
        require_reason("an unresolved category", reason)?;
        Ok(Self::Unresolved {
            claim_id,
            reason: reason.to_owned(),
        })
    }

    /// An explicitly unsupported category.
    ///
    /// # Errors
    ///
    /// [`BindingError::EmptyReason`] when `reason` is blank.
    pub fn unsupported(reason: &str) -> Result<Self, BindingError> {
        require_reason("an unsupported category", reason)?;
        Ok(Self::Unsupported {
            reason: reason.to_owned(),
        })
    }
}

/// Why one `(mission, category)` cell is or is not complete.
///
/// Spec F50 non-negotiable behavior 5 and the F50 owner ruling: "Missing,
/// unknown and unsupported are not unused, and none may count as ready."
/// There is deliberately no `Unused` state to fall back to — a cell is
/// either complete or one of the three blocking states, and all four are
/// counted in [`CoverageReport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellState {
    /// Every required row of the category is present and known.
    Complete,
    /// The category (or one of its rows) is explicitly unknown.
    Unknown,
    /// The category was not recorded at all, or a required identity row is
    /// absent.
    Missing,
    /// The category is explicitly unsupported.
    Unsupported,
}

impl CellState {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Unknown => "unknown",
            Self::Missing => "missing",
            Self::Unsupported => "unsupported",
        }
    }

    /// Whether this state counts as ready.
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Complete)
    }
}

impl fmt::Display for CellState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The state of one required category of one mission.
///
/// # Arguments
///
/// * `category` — which required category is being classified.
/// * `state` — what the binding recorded for it, or [`None`] when the
///   binding does not record the category at all.
#[must_use]
pub fn cell_state(category: BindingCategory, state: Option<&CategoryState>) -> CellState {
    let Some(state) = state else {
        return CellState::Missing;
    };
    match state {
        CategoryState::Unresolved { .. } => CellState::Unknown,
        CategoryState::Unsupported { .. } => CellState::Unsupported,
        CategoryState::Rows(rows) => {
            if rows.is_empty() {
                return CellState::Missing;
            }
            for role in category.required_roles() {
                if !rows.iter().any(|row| row.role() == *role) {
                    return CellState::Missing;
                }
            }
            if rows.iter().any(|row| !row.is_known()) {
                CellState::Unknown
            } else {
                CellState::Complete
            }
        }
    }
}

/// How one mission dependency on one required subsystem stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencyState {
    /// The subsystem satisfies the mission's need, with provenance.
    Resolved {
        /// Where the claim that it is satisfied comes from.
        provenance: Provenance,
    },
    /// Nobody has resolved the dependency yet. It is recorded, not absent.
    Unresolved {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unresolved.
        reason: String,
    },
    /// The subsystem exists as a plan but cannot support this mission yet.
    Unsupported {
        /// Why it is unsupported.
        reason: String,
    },
}

impl DependencyState {
    /// A satisfied dependency.
    pub fn resolved(provenance: Provenance) -> Self {
        Self::Resolved { provenance }
    }

    /// An explicitly unresolved dependency.
    ///
    /// # Errors
    ///
    /// [`BindingError::EmptyReason`] when `reason` is blank.
    pub fn unresolved(claim_id: ClaimId, reason: &str) -> Result<Self, BindingError> {
        require_reason("an unresolved dependency", reason)?;
        Ok(Self::Unresolved {
            claim_id,
            reason: reason.to_owned(),
        })
    }

    /// An explicitly unsupported dependency.
    ///
    /// # Errors
    ///
    /// [`BindingError::EmptyReason`] when `reason` is blank.
    pub fn unsupported(reason: &str) -> Result<Self, BindingError> {
        require_reason("an unsupported dependency", reason)?;
        Ok(Self::Unsupported {
            reason: reason.to_owned(),
        })
    }
}

/// One mission's dependency on one required subsystem: a stable identity
/// plus its explicit state.
///
/// This is the owner ruling's "stable identity and an explicit unresolved
/// dependency row" in full: the mission is complete information about what
/// it waits for, even though no subsystem's runtime type exists yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubsystemDependency {
    /// Which subsystem.
    pub subsystem: SubsystemId,
    /// How it stands.
    pub state: DependencyState,
}

/// How a mission's campaign continuation is recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progression {
    /// The successors this mission leads to, in authored order.
    Known {
        /// The missions that may follow this one.
        next: Vec<MissionLabel>,
    },
    /// The continuation is not known. Empty is *not* a stand-in for
    /// "unknown": a mission with no recorded successor chain would claim
    /// the campaign ends there (`IDENTITY-CONTENT`: a dynamic lookup that
    /// cannot be bounded is an unresolved dependency, not proof of no
    /// dependencies).
    Unknown {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why the continuation is unknown.
        reason: String,
    },
}

impl Progression {
    /// A recorded continuation.
    pub fn known(next: Vec<MissionLabel>) -> Self {
        Self::Known { next }
    }

    /// An explicitly unknown continuation.
    ///
    /// # Errors
    ///
    /// [`BindingError::EmptyReason`] when `reason` is blank.
    pub fn unknown(claim_id: ClaimId, reason: &str) -> Result<Self, BindingError> {
        require_reason("an unknown progression", reason)?;
        Ok(Self::Unknown {
            claim_id,
            reason: reason.to_owned(),
        })
    }

    /// Whether the continuation is recorded.
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known { .. })
    }
}

fn require_reason(what: &'static str, reason: &str) -> Result<(), BindingError> {
    if reason.trim().is_empty() {
        return Err(BindingError::EmptyReason { what });
    }
    Ok(())
}

/// The complete engine-independent record of one mission's binding.
///
/// Every field is data: no subsystem's concrete runtime type appears, so
/// the record keeps its shape while F18 … F47 are still unwritten. A
/// binding built by [`MissionBinding::unresolved`] starts exactly as the
/// work orders start — "Every binding starts unresolved"
/// (`missions/README.md`) — and every later stage replaces rows with
/// provenance-bearing values instead of inventing defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionBinding {
    /// The work-order discovery label.
    pub label: MissionLabel,
    /// The discovery title, outside identity, until the localized original
    /// string is bound.
    pub discovery_title: Option<String>,
    /// The required categories this mission records. Absent keys are
    /// [`CellState::Missing`].
    pub categories: BTreeMap<BindingCategory, CategoryState>,
    /// One row per required subsystem.
    pub dependencies: Vec<SubsystemDependency>,
    /// The campaign continuation.
    pub progression: Progression,
    /// Whether this record only stands in for a mission that has not been
    /// bound yet. A placeholder can be bound exactly once
    /// ([`CampaignBindings::bind`]); two real records with one label are a
    /// duplicate identity failure.
    pub placeholder: bool,
}

impl MissionBinding {
    /// A declared but unbound mission: all seven required categories
    /// explicitly unresolved, every required subsystem row explicitly
    /// unresolved, progression unknown.
    ///
    /// # Errors
    ///
    /// [`BindingError`] when a label or one of the built-in claim ids or
    /// reasons is invalid.
    pub fn unresolved(
        label: MissionLabel,
        discovery_title: Option<String>,
    ) -> Result<Self, BindingError> {
        let claim = ClaimId::new(PLACEHOLDER_CLAIM)?;
        let mut categories = BTreeMap::new();
        for category in BindingCategory::ALL {
            categories.insert(
                *category,
                CategoryState::unresolved(claim.clone(), PLACEHOLDER_CATEGORY_REASON)?,
            );
        }
        let dependencies = REQUIRED_SUBSYSTEMS
            .iter()
            .map(|subsystem| {
                Ok(SubsystemDependency {
                    subsystem: SubsystemId::new(subsystem)?,
                    state: DependencyState::unresolved(
                        claim.clone(),
                        PLACEHOLDER_DEPENDENCY_REASON,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, BindingError>>()?;
        Ok(Self {
            label,
            discovery_title,
            categories,
            dependencies,
            progression: Progression::unknown(claim, PLACEHOLDER_PROGRESSION_REASON)?,
            placeholder: true,
        })
    }

    /// The mission's catalog identity, when its `mission` identity row is
    /// bound to a content id.
    ///
    /// Until a binding task reads the localized original identity, this is
    /// [`None`]: the discovery label never stands in for it.
    pub fn catalog_identity(&self) -> Option<&ContentId> {
        let Some(CategoryState::Rows(rows)) =
            self.categories.get(&BindingCategory::MissionIdentity)
        else {
            return None;
        };
        let row = rows.iter().find(|row| row.role() == MISSION_ROLE)?;
        row.content_target()
    }

    /// The state of one required category.
    pub fn category(&self, category: BindingCategory) -> Option<&CategoryState> {
        self.categories.get(&category)
    }

    /// The state of every required category, in report order.
    pub fn cells(&self) -> impl Iterator<Item = (BindingCategory, CellState)> {
        BindingCategory::ALL.iter().copied().map(|category| {
            (
                category,
                cell_state(category, self.categories.get(&category)),
            )
        })
    }

    /// The per-record admission rules.
    ///
    /// # Errors
    ///
    /// [`BindingError`] for an empty recorded category, a blank reason, a
    /// missing/unknown/duplicated subsystem row, an identity row of the
    /// wrong kind or a duplicated successor.
    pub fn validate(&self) -> Result<(), BindingError> {
        for (category, state) in &self.categories {
            match state {
                CategoryState::Rows(rows) => {
                    if rows.is_empty() {
                        return Err(BindingError::EmptyCategory {
                            category: *category,
                        });
                    }
                }
                CategoryState::Unresolved { reason, .. }
                | CategoryState::Unsupported { reason } => {
                    require_reason("a recorded category", reason)?;
                }
            }
        }
        if let Some(CategoryState::Rows(rows)) =
            self.categories.get(&BindingCategory::MissionIdentity)
        {
            // Every row that claims an identity role is checked, not only the
            // first: a second `world` row pointing at a mission is the same
            // wrong-kind identity as a first one, and inspecting just the
            // first would let a contradictory duplicate through.
            for row in rows {
                let Some((role, expected)) = IDENTITY_ROLE_KINDS
                    .iter()
                    .find(|(name, _)| *name == row.role())
                    .copied()
                else {
                    continue;
                };
                let Some(found) = row.content_target() else {
                    continue;
                };
                if found.kind() != expected {
                    return Err(BindingError::WrongKind {
                        role: role.to_owned(),
                        expected,
                        found: found.kind(),
                    });
                }
            }
        }

        let mut seen = BTreeSet::new();
        for required in REQUIRED_SUBSYSTEMS {
            let subsystem = SubsystemId::new(required)?;
            if !self
                .dependencies
                .iter()
                .any(|row| row.subsystem == subsystem)
            {
                return Err(BindingError::MissingSubsystem { subsystem });
            }
        }
        for row in &self.dependencies {
            if !REQUIRED_SUBSYSTEMS
                .iter()
                .any(|required| row.subsystem.as_str() == *required)
            {
                return Err(BindingError::UnknownSubsystem {
                    subsystem: row.subsystem.clone(),
                });
            }
            if !seen.insert(row.subsystem.clone()) {
                return Err(BindingError::DuplicateSubsystem {
                    subsystem: row.subsystem.clone(),
                });
            }
        }

        if let Progression::Known { next } = &self.progression {
            let mut successors = BTreeSet::new();
            for label in next {
                if !successors.insert(label.clone()) {
                    return Err(BindingError::DuplicateProgression {
                        label: label.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Whether the record is a declared-but-unbound placeholder.
    pub fn is_placeholder(&self) -> bool {
        self.placeholder
    }
}

/// Why the declared campaign inventory was rejected.
///
/// The I/O variant carries a live [`std::io::Error`], so this type is not
/// `PartialEq`: callers discriminate with `matches!`, as they do for the
/// rest of the module's errors.
#[derive(Debug)]
pub enum InventoryError {
    /// The file could not be read.
    Io {
        /// The path that failed.
        path: String,
        /// The I/O error.
        source: std::io::Error,
    },
    /// A data line broke the format.
    Line {
        /// The 1-based line number.
        line: usize,
        /// What was wrong with it.
        kind: InventoryLineError,
    },
    /// The file declared no mission at all, so there would be no
    /// denominator to freeze.
    Empty,
}

/// Why one inventory line was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryLineError {
    /// The line did not have exactly `label<TAB>title`.
    FieldCount {
        /// How many tab-separated fields the line had.
        found: usize,
    },
    /// The label did not parse as a [`MissionLabel`].
    Label(LabelError),
    /// The title was empty or blank.
    EmptyTitle,
    /// The label was already declared earlier in the file.
    DuplicateLabel {
        /// The repeated label.
        label: MissionLabel,
    },
}

impl fmt::Display for InventoryLineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldCount { found } => write!(
                f,
                "expected `label{DELIM}title`, found {found} field(s)",
                DELIM = INVENTORY_DELIMITER
            ),
            Self::Label(error) => write!(f, "{error}"),
            Self::EmptyTitle => write!(f, "mission title must not be empty"),
            Self::DuplicateLabel { label } => {
                write!(f, "mission {label} is declared twice")
            }
        }
    }
}

impl std::error::Error for InventoryLineError {}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Line { line, kind } => write!(f, "line {line}: {kind}"),
            Self::Empty => write!(f, "the campaign inventory declares no mission"),
        }
    }
}

impl std::error::Error for InventoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Line { kind, .. } => Some(kind),
            Self::Empty => None,
        }
    }
}

/// The declared campaign inventory: the frozen denominator.
///
/// One line per mission work order in
/// `missions/bindings/campaign-inventory.tsv`:
///
/// ```text
/// # comment lines and blank lines carry no mission
/// M01<TAB>The Lost Treasure
/// ```
///
/// The reader is strict on purpose. A line with anything other than
/// exactly two tab-separated fields, a blank title, a malformed label or a
/// repeated label fails the whole file, so a broken edit can never silently
/// drop a mission from the denominator. The file may gain missions —
/// discovered content stays visible — and no code path removes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignInventory {
    missions: Vec<(MissionLabel, String)>,
}

impl CampaignInventory {
    /// Parses the inventory text.
    ///
    /// Blank lines and lines starting with [`INVENTORY_COMMENT`] carry no
    /// mission. A trailing `\r` is accepted so an edited file with Windows
    /// line endings is not a different inventory.
    ///
    /// # Errors
    ///
    /// [`InventoryError::Line`] for a malformed or duplicated line, and
    /// [`InventoryError::Empty`] when no mission is declared at all.
    pub fn parse(text: &str) -> Result<Self, InventoryError> {
        let mut missions = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, raw) in text.lines().enumerate() {
            let line_number = index + 1;
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if line.trim().is_empty() || line.starts_with(INVENTORY_COMMENT) {
                continue;
            }
            let fields: Vec<&str> = line.split(INVENTORY_DELIMITER).collect();
            if fields.len() != 2 {
                return Err(InventoryError::Line {
                    line: line_number,
                    kind: InventoryLineError::FieldCount {
                        found: fields.len(),
                    },
                });
            }
            let label = MissionLabel::new(fields[0]).map_err(|error| InventoryError::Line {
                line: line_number,
                kind: InventoryLineError::Label(error),
            })?;
            if fields[1].trim().is_empty() {
                return Err(InventoryError::Line {
                    line: line_number,
                    kind: InventoryLineError::EmptyTitle,
                });
            }
            if !seen.insert(label.clone()) {
                return Err(InventoryError::Line {
                    line: line_number,
                    kind: InventoryLineError::DuplicateLabel { label },
                });
            }
            missions.push((label, fields[1].to_owned()));
        }
        if missions.is_empty() {
            return Err(InventoryError::Empty);
        }
        Ok(Self { missions })
    }

    /// Reads and parses an inventory file.
    ///
    /// # Errors
    ///
    /// [`InventoryError::Io`] when the file cannot be read as UTF-8 text,
    /// then everything [`CampaignInventory::parse`] reports.
    pub fn load(path: &Path) -> Result<Self, InventoryError> {
        let text = std::fs::read_to_string(path).map_err(|source| InventoryError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&text)
    }

    /// How many missions the denominator declares.
    pub fn len(&self) -> usize {
        self.missions.len()
    }

    /// Whether the inventory declares no mission.
    pub fn is_empty(&self) -> bool {
        self.missions.is_empty()
    }

    /// Every declared mission, in file order.
    pub fn iter(&self) -> impl Iterator<Item = &(MissionLabel, String)> {
        self.missions.iter()
    }

    /// Every declared label, in file order.
    pub fn labels(&self) -> impl Iterator<Item = &MissionLabel> {
        self.missions.iter().map(|(label, _)| label)
    }
}

/// Why a closure was refused.
///
/// Identity failures are returned, never repaired: a cycle, a duplicated
/// identity (refused on admission by [`CampaignBindings::insert`]) or a
/// dangling reference must be visible as a failure, not resolved by
/// guessing which of the two records was meant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClosureError {
    /// The root (or a node reached while traversing) is not recorded.
    UnknownMission {
        /// The missing label.
        label: MissionLabel,
    },
    /// The progression graph contains a cycle; the chain is the cycle.
    Cycle {
        /// The labels forming the cycle, first repeated at the end.
        chain: Vec<MissionLabel>,
    },
    /// A progression names a mission that is not recorded.
    DanglingProgression {
        /// The mission holding the edge.
        from: MissionLabel,
        /// The missing successor.
        target: MissionLabel,
    },
    /// A row names a content id no supplied catalog holds.
    DanglingContent {
        /// The mission holding the row.
        mission: MissionLabel,
        /// The row role.
        role: String,
        /// The missing content identity.
        target: ContentId,
    },
}

impl fmt::Display for ClosureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMission { label } => write!(f, "no mission binding has label {label}"),
            Self::Cycle { chain } => {
                let labels: Vec<&str> = chain.iter().map(MissionLabel::as_str).collect();
                write!(
                    f,
                    "mission progression forms a cycle: {}",
                    labels.join(" -> ")
                )
            }
            Self::DanglingProgression { from, target } => {
                write!(
                    f,
                    "mission {from} names successor {target}, which is not recorded"
                )
            }
            Self::DanglingContent {
                mission,
                role,
                target,
            } => write!(
                f,
                "mission {mission} row {role} names content {target}, which is not in the catalog"
            ),
        }
    }
}

impl std::error::Error for ClosureError {}

/// What one mission's dependency closure reached, counted.
///
/// The closure walks the recorded mission graph from one root and counts
/// every required category, every row and every subsystem dependency it
/// reaches. [`ClosureReport::cells`] lists one entry per
/// `(reached mission, required category)` pair, so "no mission and no
/// required category was silently omitted" is a length and membership
/// assertion rather than a hope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosureReport {
    /// The closure's root.
    pub root: MissionLabel,
    /// Every mission the closure reached, in canonical label order.
    pub reached: Vec<MissionLabel>,
    /// One entry per reached mission and required category, in canonical
    /// label order and then report order.
    pub cells: Vec<(MissionLabel, BindingCategory, CellState)>,
    /// How many rows the closure counted.
    pub rows: usize,
    /// How many of those rows are explicitly unknown.
    pub unknown_rows: usize,
    /// How many subsystem dependency rows the closure counted.
    pub subsystem_rows: usize,
    /// How many subsystem rows are unresolved.
    pub unresolved_subsystems: usize,
    /// How many subsystem rows are unsupported.
    pub unsupported_subsystems: usize,
    /// How many reached missions record an unknown progression.
    pub unknown_progression: usize,
}

impl ClosureReport {
    /// How many cells are in [`ClosureReport::cells`].
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// How many cells are complete.
    pub fn complete_cells(&self) -> usize {
        self.cells
            .iter()
            .filter(|(_, _, state)| state.is_ready())
            .count()
    }

    /// Whether every reached mission is complete, every subsystem row is
    /// resolved and every progression is recorded.
    pub fn is_complete(&self) -> bool {
        self.complete_cells() == self.cells.len()
            && self.unresolved_subsystems == 0
            && self.unsupported_subsystems == 0
            && self.unknown_progression == 0
    }
}

/// The campaign coverage totals over the frozen denominator.
///
/// Spec F50 non-negotiable behavior 5: "Aggregate completion fixes the
/// denominator before testing. A missing/unparseable mission stays red; no
/// filtering to the working subset." Every `(mission, required category)`
/// pair of every *recorded* mission — declared or discovered — is counted
/// exactly once, so a missing or unknown child cannot disappear from the
/// totals, and [`CoverageReport::is_ready`] is false while any cell or
/// subsystem row is not complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageReport {
    /// The frozen denominator: how many missions the inventory declares.
    pub declared_missions: usize,
    /// Every recorded mission, including discovered ones beyond the
    /// denominator.
    pub total_missions: usize,
    /// How many required categories every mission is measured against.
    pub required_categories: usize,
    /// `total_missions × required_categories`: the cell total nothing may
    /// fall out of.
    pub cells: usize,
    /// Cells that are complete.
    pub complete_cells: usize,
    /// Cells that are explicitly unknown.
    pub unknown_cells: usize,
    /// Cells that were never recorded.
    pub missing_cells: usize,
    /// Cells that are explicitly unsupported.
    pub unsupported_cells: usize,
    /// Rows recorded inside [`CategoryState::Rows`] categories.
    pub rows: usize,
    /// Of those rows, how many are explicitly unknown.
    pub unknown_rows: usize,
    /// Subsystem dependency rows recorded across all missions.
    pub subsystem_rows: usize,
    /// Subsystem rows that are resolved.
    pub subsystem_resolved: usize,
    /// Subsystem rows that are unresolved.
    pub subsystem_unresolved: usize,
    /// Subsystem rows that are unsupported.
    pub subsystem_unsupported: usize,
    /// Missions whose campaign continuation is recorded.
    pub progression_known: usize,
    /// Missions whose campaign continuation is explicitly unknown.
    pub progression_unknown: usize,
}

impl CoverageReport {
    /// Missions recorded beyond the frozen denominator: discovered content
    /// that is visible instead of silently folded into the baseline.
    pub fn discovered_extra(&self) -> usize {
        self.total_missions - self.declared_missions
    }

    /// The cells the declared denominator alone accounts for.
    pub fn declared_cells(&self) -> usize {
        self.declared_missions * self.required_categories
    }

    /// Whether every cell of every recorded mission is complete, every
    /// subsystem row is resolved and every progression is recorded.
    ///
    /// The frozen denominator is part of the question, not a precondition of
    /// it. A campaign with no declared denominator has nothing to be complete
    /// against and is never ready, and a recorded mission still outside the
    /// denominator keeps the campaign unready until it is declared — spec F50
    /// non-negotiable behavior 5, "no filtering to the working subset". An
    /// empty or under-declared aggregate is exactly how a short campaign would
    /// otherwise report itself complete.
    pub fn is_ready(&self) -> bool {
        self.declared_missions > 0
            && self.declared_missions == self.total_missions
            && self.cells == self.complete_cells
            && self.subsystem_unresolved == 0
            && self.subsystem_unsupported == 0
            && self.progression_unknown == 0
    }
}

/// The campaign's mission bindings plus the frozen denominator.
///
/// Admission rules live here, next to F14's catalog: an invalid record is
/// refused with a [`BindingError`] instead of being stored and discovered
/// later, and a duplicate identity is never merged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CampaignBindings {
    missions: BTreeMap<MissionLabel, MissionBinding>,
    declared: BTreeSet<MissionLabel>,
}

impl CampaignBindings {
    /// An empty campaign with no declared denominator.
    pub fn new() -> Self {
        Self::default()
    }

    /// The declared inventory as placeholders: one recorded mission per
    /// declared label, each explicitly unbound, and every label in the
    /// denominator.
    ///
    /// # Errors
    ///
    /// [`BindingError`] when a built-in claim id or reason is invalid, or
    /// when an inventory label collides with an already recorded mission.
    pub fn from_inventory(inventory: &CampaignInventory) -> Result<Self, BindingError> {
        let mut bindings = Self::new();
        for (label, title) in inventory.iter() {
            let binding = MissionBinding::unresolved(label.clone(), Some(title.clone()))?;
            bindings.insert(binding)?;
            bindings.declare(label)?;
        }
        Ok(bindings)
    }

    /// Records one mission binding.
    ///
    /// The record is validated before it is looked up, so a record that is
    /// both invalid and a duplicate is reported as the invalid record it is.
    ///
    /// # Errors
    ///
    /// Every rule of [`MissionBinding::validate`], then
    /// [`BindingError::DuplicateId`] when the label is already recorded.
    pub fn insert(&mut self, binding: MissionBinding) -> Result<(), BindingError> {
        binding.validate()?;
        if self.missions.contains_key(&binding.label) {
            return Err(BindingError::DuplicateId {
                label: binding.label,
            });
        }
        self.missions.insert(binding.label.clone(), binding);
        Ok(())
    }

    /// Adds a recorded mission to the frozen denominator.
    ///
    /// The denominator only ever grows: there is no call that removes a
    /// declared mission, so "the denominator cannot shrink" is structural
    /// rather than a promise.
    ///
    /// # Errors
    ///
    /// [`BindingError::UnknownMission`] when nothing is recorded under
    /// `label`.
    pub fn declare(&mut self, label: &MissionLabel) -> Result<(), BindingError> {
        if !self.missions.contains_key(label) {
            return Err(BindingError::UnknownMission {
                label: label.clone(),
            });
        }
        self.declared.insert(label.clone());
        Ok(())
    }

    /// Replaces the placeholder standing in for `binding.label`.
    ///
    /// This is the hand-off every binding stage uses: the inventory declares
    /// a mission as unresolved, the binding task fills it in once. Like
    /// [`CampaignBindings::insert`], the incoming record is validated before
    /// the existing one is looked up.
    ///
    /// # Errors
    ///
    /// Every rule of [`MissionBinding::validate`], then
    /// [`BindingError::UnknownMission`] when the label was never recorded, and
    /// [`BindingError::AlreadyBound`] when the recorded mission is no longer a
    /// placeholder (two real records with one identity would be a duplicate).
    pub fn bind(&mut self, binding: MissionBinding) -> Result<(), BindingError> {
        binding.validate()?;
        let label = binding.label.clone();
        let Some(existing) = self.missions.get(&label) else {
            return Err(BindingError::UnknownMission { label });
        };
        if !existing.placeholder {
            return Err(BindingError::AlreadyBound { label });
        }
        self.missions.insert(label, binding);
        Ok(())
    }

    /// The recorded mission with `label`, if any.
    pub fn get(&self, label: &MissionLabel) -> Option<&MissionBinding> {
        self.missions.get(label)
    }

    /// Every recorded mission, in canonical label order.
    pub fn missions(&self) -> impl Iterator<Item = &MissionBinding> {
        self.missions.values()
    }

    /// How many missions are recorded.
    pub fn len(&self) -> usize {
        self.missions.len()
    }

    /// Whether no mission is recorded.
    pub fn is_empty(&self) -> bool {
        self.missions.is_empty()
    }

    /// Every declared label, in canonical order: the frozen denominator.
    pub fn declared(&self) -> impl Iterator<Item = &MissionLabel> {
        self.declared.iter()
    }

    /// How many missions the denominator declares.
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }

    /// Runs one mission's dependency closure.
    ///
    /// Pass `catalog` to check every known content row against the catalog;
    /// without it only the mission graph is checked. Nothing is repaired:
    /// a cycle, a dangling successor or a dangling content identity comes
    /// back as a [`ClosureError`].
    ///
    /// # Errors
    ///
    /// [`ClosureError::UnknownMission`] when `root` is not recorded, then
    /// any [`ClosureError`] the traversal finds.
    pub fn closure(
        &self,
        root: &MissionLabel,
        catalog: Option<&Catalog>,
    ) -> Result<ClosureReport, ClosureError> {
        if !self.missions.contains_key(root) {
            return Err(ClosureError::UnknownMission {
                label: root.clone(),
            });
        }
        let mut reached = BTreeSet::new();
        let mut path = Vec::new();
        let mut report = ClosureReport {
            root: root.clone(),
            reached: Vec::new(),
            cells: Vec::new(),
            rows: 0,
            unknown_rows: 0,
            subsystem_rows: 0,
            unresolved_subsystems: 0,
            unsupported_subsystems: 0,
            unknown_progression: 0,
        };
        self.visit(root, catalog, &mut path, &mut reached, &mut report)?;
        report.reached = reached.into_iter().collect();
        Ok(report)
    }

    /// Runs one closure per recorded mission, in canonical label order.
    ///
    /// Every recorded mission gets its own root, so no mission can be
    /// skipped by a shared visited-set optimisation: "Run all mission
    /// dependency closures and assert none are silently omitted" (F50
    /// AC01) is checked against this list.
    ///
    /// # Errors
    ///
    /// The first [`ClosureError`] of any closure.
    pub fn closures(&self, catalog: Option<&Catalog>) -> Result<Vec<ClosureReport>, ClosureError> {
        self.missions
            .keys()
            .map(|label| self.closure(label, catalog))
            .collect()
    }

    /// The coverage totals over every recorded mission.
    pub fn coverage(&self) -> CoverageReport {
        let mut report = CoverageReport {
            declared_missions: self.declared.len(),
            total_missions: self.missions.len(),
            required_categories: BindingCategory::ALL.len(),
            cells: 0,
            complete_cells: 0,
            unknown_cells: 0,
            missing_cells: 0,
            unsupported_cells: 0,
            rows: 0,
            unknown_rows: 0,
            subsystem_rows: 0,
            subsystem_resolved: 0,
            subsystem_unresolved: 0,
            subsystem_unsupported: 0,
            progression_known: 0,
            progression_unknown: 0,
        };
        for binding in self.missions.values() {
            for (category, state) in binding.cells() {
                match state {
                    CellState::Complete => report.complete_cells += 1,
                    CellState::Unknown => report.unknown_cells += 1,
                    CellState::Missing => report.missing_cells += 1,
                    CellState::Unsupported => report.unsupported_cells += 1,
                }
                if let Some(CategoryState::Rows(rows)) = binding.category(category) {
                    report.rows += rows.len();
                    report.unknown_rows += rows.iter().filter(|row| !row.is_known()).count();
                }
            }
            for dependency in &binding.dependencies {
                report.subsystem_rows += 1;
                match dependency.state {
                    DependencyState::Resolved { .. } => report.subsystem_resolved += 1,
                    DependencyState::Unresolved { .. } => report.subsystem_unresolved += 1,
                    DependencyState::Unsupported { .. } => report.subsystem_unsupported += 1,
                }
            }
            if binding.progression.is_known() {
                report.progression_known += 1;
            } else {
                report.progression_unknown += 1;
            }
        }
        report.cells = report.total_missions * report.required_categories;
        report
    }

    /// Depth-first traversal of the mission graph from `id`.
    ///
    /// `path` holds the current chain, which is where a cycle is reported
    /// from; `reached` holds every mission already fully explored, so a
    /// diamond converges instead of being counted twice.
    fn visit(
        &self,
        id: &MissionLabel,
        catalog: Option<&Catalog>,
        path: &mut Vec<MissionLabel>,
        reached: &mut BTreeSet<MissionLabel>,
        report: &mut ClosureReport,
    ) -> Result<(), ClosureError> {
        if let Some(index) = path.iter().position(|entry| entry == id) {
            let mut chain = path[index..].to_vec();
            chain.push(id.clone());
            return Err(ClosureError::Cycle { chain });
        }
        if !reached.insert(id.clone()) {
            return Ok(());
        }
        let Some(binding) = self.missions.get(id) else {
            return Err(ClosureError::UnknownMission { label: id.clone() });
        };

        for (category, state) in binding.cells() {
            report.cells.push((id.clone(), category, state));
            if let Some(CategoryState::Rows(rows)) = binding.category(category) {
                report.rows += rows.len();
                for row in rows {
                    if !row.is_known() {
                        report.unknown_rows += 1;
                    }
                    if let Resolved::Known(known) = row.target()
                        && let BindingTarget::Content(target) = &known.value
                        && let Some(catalog) = catalog
                        && catalog.get(target).is_none()
                    {
                        return Err(ClosureError::DanglingContent {
                            mission: id.clone(),
                            role: row.role().to_owned(),
                            target: target.clone(),
                        });
                    }
                }
            }
        }
        for dependency in &binding.dependencies {
            report.subsystem_rows += 1;
            match dependency.state {
                DependencyState::Resolved { .. } => {}
                DependencyState::Unresolved { .. } => report.unresolved_subsystems += 1,
                DependencyState::Unsupported { .. } => report.unsupported_subsystems += 1,
            }
        }
        if !binding.progression.is_known() {
            report.unknown_progression += 1;
        }

        path.push(id.clone());
        if let Progression::Known { next } = &binding.progression {
            for target in next {
                if !self.missions.contains_key(target) {
                    return Err(ClosureError::DanglingProgression {
                        from: id.clone(),
                        target: target.clone(),
                    });
                }
                self.visit(target, catalog, path, reached, report)?;
            }
        }
        path.pop();
        Ok(())
    }
}

// ---------------------------------------------------------------------
// Source-derived binding (the per-mission `M01-A` … `M24-A` stages).
// ---------------------------------------------------------------------

/// The dependencies a **source-derived** binding must resolve before any
/// branch or content work can start.
///
/// These are the first five entries of the data-binding checklist every
/// mission sheet carries (`missions/M01.md`, "Data binding checklist":
/// *canonical mission id; installation/rules hash; title string; program and
/// source map; world group/variant*). They are the anchors every later
/// entry of the checklist is read *through*: without a mission id, an
/// installation hash, a confirmed title, a program source map and a world
/// group there is nothing for actors, objectives, media, rewards or
/// difficulty branches to hang on. That is what the `M01-A` minimum
/// acceptance scenario ("Source-derived binding has no unresolved critical
/// dependencies") asks for, and it is why this list exists as data instead
/// of an adjective.
///
/// The rest of the checklist is *content* the binding records rather than a
/// dependency it needs: those entries are kept in
/// [`SourceBinding::unknowns`], keep [`SourceBinding::is_verified`] false
/// and keep the campaign unready. Resolving the five critical dependencies
/// therefore never hides an unbound checklist entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CriticalDependency {
    /// The canonical mission id of the checklist's first entry.
    MissionId,
    /// The installation/rules hash of the second entry.
    InstallHash,
    /// The localized title string of the third entry.
    TitleString,
    /// The program identity and its source map of the fourth entry.
    ProgramSourceMap,
    /// The world group/variant of the fifth entry.
    WorldGroupVariant,
}

impl CriticalDependency {
    /// Every critical dependency, in checklist order.
    pub const ALL: &'static [CriticalDependency] = &[
        Self::MissionId,
        Self::InstallHash,
        Self::TitleString,
        Self::ProgramSourceMap,
        Self::WorldGroupVariant,
    ];

    /// The stable label of this dependency, used in claim ids and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::MissionId => "mission_id",
            Self::InstallHash => "installation_hash",
            Self::TitleString => "title_string",
            Self::ProgramSourceMap => "program_source_map",
            Self::WorldGroupVariant => "world_group_variant",
        }
    }

    /// The claim id this dependency carries for one mission label.
    fn claim(self, label: &MissionLabel) -> Result<ClaimId, ClaimIdError> {
        ClaimId::new(&format!(
            "{}.{}",
            label.as_str().to_ascii_lowercase(),
            self.label()
        ))
    }
}

impl fmt::Display for CriticalDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One critical dependency of one source-derived binding and how it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDependency {
    /// Which critical dependency.
    pub id: CriticalDependency,
    /// Resolved with provenance, or explicitly unresolved with a reason.
    pub state: DependencyState,
}

/// One byte range of one original asset a source-derived binding rests on.
///
/// `sha256` is the digest of the **whole** asset named by `asset_id`, not of
/// the cited range: every span of the same asset carries the same digest, so
/// it pins the asset but not the bytes. `offset`/`length` alone locate the
/// span; a reader that needs these exact bytes re-reads that range and a
/// reader that needs the asset unchanged compares `sha256`. A span carries no
/// bytes, so a binding record can cite original data without containing any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpanRecord {
    /// The asset's logical path inside the installation.
    pub asset_id: String,
    /// Where the span starts inside that asset.
    pub offset: u64,
    /// How many bytes the span covers.
    pub length: u64,
    /// The digest of the whole asset.
    pub sha256: String,
}

/// One campaign mission as the installation's directory layout declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignMission {
    /// The chapter the mission belongs to (`ZBD/C1*` → 1).
    pub chapter: u32,
    /// The mission's number inside its chapter (`ZBD/*/M01` → 1).
    pub mission_number: u32,
    /// The world group directory, lowercased for identity.
    pub world_group: String,
    /// The mission reader archive's logical path, as spelled on disk.
    pub program_asset: String,
    /// Whether that archive is present in this installation.
    pub program_present: bool,
}

/// One campaign mission directory with the digest of its program archive
/// (F14-E).
///
/// The layout is the same derivation [`SourceContext::read`] and the
/// per-mission binding stages already use ([`scan_campaign`]): one walk of
/// `ZBD/<chapter><variant>/<mission>` giving each mission's chapter, number,
/// world group and program archive. This record adds the one fact a
/// read-only inspection report needs on top of it — the SHA-256 of every
/// *present* program archive — and retains no original bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignLayoutEntry {
    /// The mission as the directory layout declares it.
    pub mission: CampaignMission,
    /// The digest of the whole program archive named by
    /// [`CampaignMission::program_asset`], or [`None`] when no archive is
    /// present there. A present archive that cannot be read is an error, not
    /// a `None`.
    pub program_sha256: Option<String>,
}

/// Reads the installation's campaign directory layout with the digest of
/// every present mission program archive (F14-E).
///
/// The walk is exactly [`scan_campaign`], the derivation
/// [`SourceContext::read`] uses for its campaign, so the inspection report
/// and the per-mission bindings cannot disagree about chapter, mission
/// number, world group, program path or presence. The only addition is
/// hashing each present program archive: no original bytes are returned.
///
/// # Errors
///
/// [`SourceBindingError::NoCampaign`] when the installation declares no
/// `ZBD/<chapter>/<mission>` directory, [`SourceBindingError::Io`] when
/// the layout cannot be walked or a present program archive cannot be read,
/// and [`SourceBindingError::Inconsistent`] when the layout itself is
/// ambiguous — a chapter that stores one mission number in two world groups
/// is refused rather than resolved to one of them. A mission directory whose
/// `zrdr.zbd` is absent is reported with
/// [`CampaignLayoutEntry::program_sha256`] `None`, never omitted.
pub fn campaign_layout(
    install_root: &Path,
) -> Result<Vec<CampaignLayoutEntry>, SourceBindingError> {
    let campaign = scan_campaign(install_root)?;
    if campaign.is_empty() {
        return Err(SourceBindingError::NoCampaign);
    }
    let mut layout = Vec::with_capacity(campaign.len());
    for mission in campaign {
        let program_sha256 = if mission.program_present {
            let bytes = read_file(&install_root.join(&mission.program_asset))?;
            Some(cs_assets::install::sha256(&bytes).to_hex())
        } else {
            None
        };
        layout.push(CampaignLayoutEntry {
            mission,
            program_sha256,
        });
    }
    Ok(layout)
}

/// The checklist entries a source binding deliberately does **not**
/// resolve, with why. Every one of them keeps `verified` false.
const SOURCE_BINDING_UNKNOWNS: &[&str] = &[
    "initial player and wingmate configurations: not bound from original data at this stage",
    "actor/spawn/route sets: not bound from original data at this stage",
    "objective graph: not bound from original data at this stage",
    "interaction authorizations: not bound from original data at this stage",
    "script/native coverage: the mission opcode table is unmeasured, so F13-C probes every \
     program against an empty signature table",
    "required geometry/collision/materials: not bound from original data at this stage",
    "audio/dialogue/video/camera cues: not bound from original data at this stage",
    "initial and terminal campaign state: not bound from original data at this stage",
    "optional/stunt/reward ids: not bound from original data at this stage",
    "difficulty branches: not bound from original data at this stage",
    "exact failure/success precedence: not measured",
    "campaign progression: the successor mission is not bound to original mission ids",
    "closure_sha256: the mission dependency closure hash needs the retail content catalog \
     (F14-D) and decoded mission programs (F37/F38)",
];

/// Why a source-derived binding could not be produced at all.
///
/// A *semantic* miss — a title that the local strings do not carry, a
/// campaign position that cannot be resolved — is never an error: it becomes
/// an unresolved [`CriticalDependency`] so the record says what is missing.
/// Only I/O and a structurally unusable installation stop the derivation.
#[derive(Debug)]
pub enum SourceBindingError {
    /// Installation discovery refused the directory.
    Discover {
        /// The directory that was refused.
        path: String,
        /// Why discovery refused it.
        source: cs_assets::install::DiscoveryError,
    },
    /// A file could not be read.
    Io {
        /// The path that failed.
        path: String,
        /// The I/O error.
        source: std::io::Error,
    },
    /// The UI string image is not a PE resource table this engine can read.
    Strings {
        /// The asset that failed to read.
        path: String,
        /// Why it was refused.
        message: String,
    },
    /// The installation holds no campaign mission directories.
    NoCampaign,
    /// A provenance, claim id, content id or dependency row could not be
    /// built from facts that were read successfully.
    Inconsistent {
        /// What did not hold.
        reason: String,
    },
}

impl fmt::Display for SourceBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discover { path, source } => {
                write!(f, "cannot discover the installation at {path}: {source}")
            }
            Self::Io { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Strings { path, message } => {
                write!(f, "cannot read the UI string table {path}: {message}")
            }
            Self::NoCampaign => write!(
                f,
                "the installation declares no campaign mission directories under ZBD/"
            ),
            Self::Inconsistent { reason } => write!(f, "inconsistent source binding: {reason}"),
        }
    }
}

impl std::error::Error for SourceBindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discover { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::Strings { .. } | Self::NoCampaign | Self::Inconsistent { .. } => None,
        }
    }
}

impl From<ClaimIdError> for SourceBindingError {
    fn from(error: ClaimIdError) -> Self {
        Self::Inconsistent {
            reason: format!("a claim id does not validate: {error}"),
        }
    }
}

impl From<BindingError> for SourceBindingError {
    fn from(error: BindingError) -> Self {
        Self::Inconsistent {
            reason: format!("a dependency row does not validate: {error}"),
        }
    }
}

/// The expensive half of a source binding: everything read once from the
/// installation, so one run can bind several work orders.
///
/// Reading fingerprints the installation, walks the campaign directory
/// layout and loads the localized UI string table. [`SourceContext::bind`]
/// then answers one work order from those facts without touching the disk
/// again (except for the one mission program archive it cites).
#[derive(Clone, Debug)]
pub struct SourceContext {
    /// Where the installation lives on this host.
    install_root: PathBuf,
    /// The installation fingerprint of the read bytes.
    install_hash: ContentHash,
    /// The hex form of [`SourceContext::install_hash`].
    install_sha256: String,
    /// The localized UI string asset, as spelled inside the installation.
    string_asset: String,
    /// The digest of that whole asset.
    string_asset_sha256: String,
    /// Every localizable string of that asset.
    strings: StringCatalog,
    /// The campaign, ordered by `(chapter, mission number)`.
    campaign: Vec<CampaignMission>,
}

impl SourceContext {
    /// Fingerprints `install_root`, reads its campaign directory layout and
    /// its localized UI string table.
    ///
    /// # Errors
    ///
    /// [`SourceBindingError::Discover`] when installation discovery refuses
    /// the directory, [`SourceBindingError::Io`] when a required file cannot
    /// be read, [`SourceBindingError::Strings`] when the UI string asset is
    /// not a PE resource image this engine reads, and
    /// [`SourceBindingError::NoCampaign`] when no campaign mission directory
    /// exists.
    pub fn read(install_root: &Path) -> Result<Self, SourceBindingError> {
        let found = cs_assets::install::discover(install_root).map_err(|source| {
            SourceBindingError::Discover {
                path: install_root.display().to_string(),
                source,
            }
        })?;
        let install_hash = cs_assets::install::fingerprint(&found.manifest);

        let campaign = scan_campaign(install_root)?;
        if campaign.is_empty() {
            return Err(SourceBindingError::NoCampaign);
        }

        let string_asset = "GOSDATA/ASSETS/BINARIES/langui.dll";
        let string_path = install_root.join(string_asset);
        let bytes = read_file(&string_path)?;
        let string_asset_sha256 = cs_assets::install::sha256(&bytes).to_hex();
        // A loose file is its own source, so it carries no member digest
        // (the same rule `cs-inspect config` applies to `strings.dll`).
        let source = SourceSpan::new(
            install_hash,
            string_asset,
            None,
            0,
            bytes.len() as u64,
            None,
        )
        .map_err(|error| SourceBindingError::Inconsistent {
            reason: format!("the UI string span does not validate: {error}"),
        })?;
        let mut context = ParseContext::with_defaults(string_asset);
        let strings = StringCatalog::read(&mut context, source, &bytes).map_err(|error| {
            SourceBindingError::Strings {
                path: string_asset.to_owned(),
                message: error.to_string(),
            }
        })?;

        Ok(Self {
            install_root: install_root.to_path_buf(),
            install_hash,
            install_sha256: install_hash.to_hex(),
            string_asset: string_asset.to_owned(),
            string_asset_sha256,
            strings,
            campaign,
        })
    }

    /// The installation fingerprint this context was read under.
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The campaign, ordered by `(chapter, mission number)`.
    pub fn campaign(&self) -> &[CampaignMission] {
        &self.campaign
    }

    /// The localized UI string rows, for callers that need the table itself.
    pub fn string_rows(&self) -> &[StringRow] {
        self.strings.rows()
    }

    /// Binds one work order to the original data this context was read from.
    ///
    /// `discovery_title` is the work-order title from the declared
    /// inventory; it is *confirmed* against the local strings rather than
    /// assumed to match. The confirmation is [`SourceContext::confirm_title`]:
    /// a row carries the title verbatim or as the title part of a
    /// region-prefixed long name, byte for byte, and the verbatim form wins
    /// whenever any row offers it. A title the local strings carry in neither
    /// form, or carry more than once, leaves
    /// [`CriticalDependency::TitleString`] unresolved — which in turn leaves
    /// the mission, world and program unresolved, because there is no campaign
    /// position to read them from. When the confirmation came through the
    /// long-name form and the installation spells the same campaign position
    /// differently in another campaign-length block, that second spelling is
    /// recorded in [`SourceBinding::unknowns`] rather than reconciled.
    ///
    /// # Errors
    ///
    /// [`SourceBindingError::Io`] when the mission program archive cannot be
    /// read, and [`SourceBindingError::Inconsistent`] when a fact that was
    /// read cannot be turned into an id, a claim or a provenance.
    pub fn bind(
        &self,
        label: MissionLabel,
        discovery_title: &str,
    ) -> Result<SourceBinding, SourceBindingError> {
        let mut source_spans = Vec::new();

        // --- title string: confirm the discovery title against the local
        // strings, exactly once, and keep the block span it was read from.
        // Verbatim and region-prefixed long-name rows are two display forms of
        // one fact (see `SourceContext::confirm_title`); the record keeps the
        // row, not the form, and only the form decides which refusal applies.
        let confirmation = self.confirm_title(discovery_title);
        let title_row = confirmation.confirmed().map(|(row_id, _)| {
            self.strings
                .rows()
                .iter()
                .find(|row| row.id == row_id)
                .expect("a confirmed row is one of the table's rows")
        });
        let title_reason = confirmation.refusal();

        let mut title_source = None;
        let mut title_enclosure = None;
        let mut title_row_span_recovered = false;
        if let Some(row) = title_row {
            let enclosure = row.span.clone();
            title_enclosure = Some(enclosure.clone());
            // `StringRow::span` locates the whole `RT_STRING` block, not the
            // row; `SourceSpanRecord` must cite the row's own bytes so a
            // reader can point at the string that was matched. The block stays
            // as the named enclosing source.
            match row_byte_span(&self.strings, row) {
                Some(span) => {
                    title_source = Some(span.clone());
                    title_row_span_recovered = true;
                    source_spans.push(SourceSpanRecord {
                        asset_id: self.string_asset.clone(),
                        offset: span.offset(),
                        length: span.length(),
                        sha256: self.string_asset_sha256.clone(),
                    });
                }
                None => {
                    // Only the bundle is recoverable: cite it and let the
                    // unknown below say so instead of implying it is the row.
                    title_source = Some(enclosure.clone());
                    source_spans.push(SourceSpanRecord {
                        asset_id: self.string_asset.clone(),
                        offset: enclosure.offset(),
                        length: enclosure.length(),
                        sha256: self.string_asset_sha256.clone(),
                    });
                }
            }
        }

        // --- campaign position: the localized titles form contiguous id
        // blocks, a block must be exactly as long as the campaign the
        // directory layout declares, and the localized table must not
        // contradict that layout. Anything else is not a position.
        let agreement = self.join_agreement();
        let resolved = campaign_position_for(title_row.map(|row| row.id), &agreement);
        let position = resolved.as_ref().ok().copied();
        let refusal = resolved.as_ref().err().copied();
        let entry = position.and_then(|index| self.campaign.get(index));

        // --- program source map: the mission's reader archive.
        let mut program_source = None;
        let mut program_digest = None;
        if let Some(entry) = entry
            && entry.program_present
        {
            let path = self.install_root.join(&entry.program_asset);
            let bytes = read_file(&path)?;
            program_digest = Some((
                entry.program_asset.clone(),
                bytes.len() as u64,
                cs_assets::install::sha256(&bytes).to_hex(),
            ));
            let span = SourceSpan::new(
                self.install_hash,
                &entry.program_asset,
                None,
                0,
                bytes.len() as u64,
                None,
            )
            .map_err(|error| SourceBindingError::Inconsistent {
                reason: format!("the program span does not validate: {error}"),
            })?;
            program_source = Some(span);
        }
        if let Some((asset_id, length, sha256)) = &program_digest {
            source_spans.push(SourceSpanRecord {
                asset_id: asset_id.clone(),
                offset: 0,
                length: *length,
                sha256: sha256.clone(),
            });
        }

        // --- identities.
        let catalog_id = entry
            .map(mission_key)
            .map(|key| ContentId::from_source(ContentKind::Mission, &key))
            .transpose()
            .map_err(|error| SourceBindingError::Inconsistent {
                reason: format!("the mission id is not valid: {error}"),
            })?;
        let world_id = entry
            .map(|entry| ContentId::from_source(ContentKind::World, &entry.world_group))
            .transpose()
            .map_err(|error| SourceBindingError::Inconsistent {
                reason: format!("the world id is not valid: {error}"),
            })?;
        let program_id = if program_source.is_some() {
            entry
                .map(|entry| ContentId::from_source(ContentKind::Script, &program_key(entry)))
                .transpose()
                .map_err(|error| SourceBindingError::Inconsistent {
                    reason: format!("the program id is not valid: {error}"),
                })?
        } else {
            None
        };

        // --- the five critical dependencies.
        let mut dependencies = Vec::new();
        for &id in CriticalDependency::ALL {
            let state = match id {
                CriticalDependency::InstallHash => DependencyState::resolved(
                    Provenance::new(id.claim(&label)?, ClaimStatus::ObservedTool, None)
                        .map_err(map_provenance)?,
                ),
                CriticalDependency::TitleString => match &title_source {
                    Some(source) => DependencyState::resolved(
                        Provenance::new(
                            id.claim(&label)?,
                            ClaimStatus::ObservedTool,
                            Some(source.clone()),
                        )
                        .map_err(map_provenance)?,
                    ),
                    None => DependencyState::unresolved(
                        id.claim(&label)?,
                        title_reason.expect("an unconfirmed title refuses with its own reason"),
                    )?,
                },
                CriticalDependency::MissionId => match &catalog_id {
                    Some(_) => DependencyState::resolved(
                        Provenance::new(
                            id.claim(&label)?,
                            ClaimStatus::Inferred,
                            program_source.clone(),
                        )
                        .map_err(map_provenance)?,
                    ),
                    None => DependencyState::unresolved(
                        id.claim(&label)?,
                        match refusal {
                            Some(reason) => reason,
                            None => {
                                "the campaign position could not be resolved, so no original \
                                     mission id was located"
                            }
                        },
                    )?,
                },
                CriticalDependency::WorldGroupVariant => match &world_id {
                    Some(_) => DependencyState::resolved(
                        Provenance::new(
                            id.claim(&label)?,
                            ClaimStatus::Inferred,
                            program_source.clone(),
                        )
                        .map_err(map_provenance)?,
                    ),
                    None => DependencyState::unresolved(
                        id.claim(&label)?,
                        match refusal {
                            Some(reason) => reason,
                            None => {
                                "the world group could not be located because the campaign \
                                     position is unknown"
                            }
                        },
                    )?,
                },
                CriticalDependency::ProgramSourceMap => match &program_source {
                    Some(source) => DependencyState::resolved(
                        Provenance::new(
                            id.claim(&label)?,
                            ClaimStatus::ObservedTool,
                            Some(source.clone()),
                        )
                        .map_err(map_provenance)?,
                    ),
                    None => DependencyState::unresolved(
                        id.claim(&label)?,
                        match refusal {
                            Some(reason) => reason,
                            None => {
                                "the mission program archive was not located in the \
                                     installation"
                            }
                        },
                    )?,
                },
            };
            dependencies.push(SourceDependency { id, state });
        }

        let mut unknowns: Vec<String> = SOURCE_BINDING_UNKNOWNS
            .iter()
            .map(|entry| (*entry).to_owned())
            .collect();
        // A title confirmed through the long-name form means the installation
        // carries this mission's name in two display forms, and they may
        // disagree: the bare short name of this campaign position can omit a
        // word the long name and the declared title carry. That difference is
        // measured, recorded and *not* reconciled — the record must not read as
        // if the original program had settled which spelling is the mission's.
        if let (Some(TitleForm::RegionPrefixedLongName), Some(position)) =
            (confirmation.confirmed().map(|(_, form)| form), position)
        {
            let row_id = confirmation
                .confirmed()
                .map(|(row_id, _)| row_id)
                .expect("a form was confirmed");
            let differing: Vec<String> = self
                .other_spellings(position, row_id)
                .into_iter()
                .filter(|(_, display)| display != discovery_title)
                .map(|(id, display)| format!("row {id} reads {display:?}"))
                .collect();
            if !differing.is_empty() {
                unknowns.push(format!(
                    "title spelling: the declared title is carried only by the region-prefixed \
                     long-name row {row_id}, while the same campaign position is spelled \
                     differently elsewhere in the same installation ({}) — the difference is \
                     recorded, not reconciled",
                    differing.join(", ")
                ));
            }
        }

        // A confirmed title whose own bytes could not be located inside its
        // decoded block is cited by the enclosing block; the record says so
        // rather than implying the block is the row.
        if title_row.is_some() && !title_row_span_recovered {
            unknowns.push(
                "title span: the confirmed row's own byte range could not be located inside its \
                 decoded RT_STRING block, so the cited title span is the enclosing block, not the \
                 row"
                .to_owned(),
            );
        }

        Ok(SourceBinding {
            label,
            discovery_title: discovery_title.to_owned(),
            install_sha256: self.install_sha256.clone(),
            campaign_position: position,
            campaign_size: self.campaign.len(),
            catalog_id,
            world_id,
            program_id,
            localized_title_id: title_row.map(|row| row.id),
            localized_title_language: title_row.map(|row| row.language),
            dependencies,
            source_spans,
            identity_source: program_source,
            title_source,
            title_enclosure,
            closure_sha256: None,
            evidence_ids: Vec::new(),
            unknowns,
        })
    }

    /// How many missions each chapter of the directory layout holds, in
    /// chapter order.
    ///
    /// The campaign is ordered by `(chapter, mission number)`, so this is the
    /// run-length of its chapter numbers. It is one of the two shapes the
    /// title→campaign join is checked against: the localized table groups its
    /// long mission names by region, and a block whose region groups fall into
    /// these sizes describes the same campaign the layout declares.
    pub fn chapter_sizes(&self) -> Vec<usize> {
        let mut sizes: Vec<usize> = Vec::new();
        let mut previous: Option<u32> = None;
        for entry in &self.campaign {
            if previous == Some(entry.chapter) {
                *sizes.last_mut().expect("a counted chapter has a size") += 1;
            } else {
                sizes.push(1);
            }
            previous = Some(entry.chapter);
        }
        sizes
    }

    /// Every maximal run of consecutive localized rows whose length equals
    /// the campaign the directory layout declares, ascending by first row.
    ///
    /// A run is measured over the rows whose display text is not empty, so a
    /// row that decodes to nothing ends it. More than one run can be exactly
    /// as long as the campaign — the retail installation holds both the
    /// mission's short name and its region-prefixed long name, 24 rows each —
    /// and every one of them is listed rather than only the first, so a
    /// caller can see how many independent row groups the join rests on.
    pub fn campaign_title_blocks(&self) -> Vec<TitleBlock> {
        let present = self.present_string_ids();
        let blocks = title_blocks(&present);
        let campaign = self.campaign.len();
        blocks
            .into_iter()
            .filter(|block| block.len() == campaign)
            .collect()
    }

    /// The string ids whose display text is not empty.
    fn present_string_ids(&self) -> BTreeSet<u32> {
        self.strings
            .rows()
            .iter()
            .filter_map(|row| {
                row.text
                    .as_deref()
                    .is_some_and(|text| !strip_font_tag(text).is_empty())
                    .then_some(row.id)
            })
            .collect()
    }

    /// Confirms one discovery title against the localized table.
    ///
    /// Two steps, and the order matters: a row whose display text *is* the
    /// title confirms it verbatim ([`TitleForm::Verbatim`]), and only when no
    /// row does is a region-prefixed long name consulted
    /// ([`TitleForm::RegionPrefixedLongName`]). A mission the guide spells
    /// exactly therefore keeps binding to the row that carries it exactly,
    /// even where a long name carries the same title, and the long-name form
    /// is the fallback for the missions the guide spells with a word the
    /// bare short name omits — never a fuzzy comparison, and never a
    /// long-name row once any verbatim row carries the title.
    ///
    /// Where both forms carry the title, the choice of the verbatim row is a
    /// *measurement-backed* preference, not an assumption: the two rows are
    /// expected to name the same campaign position, and a disagreement about
    /// that is not something this function can see — it only knows the rows.
    /// A caller that binds an identity reads the position the confirmed row
    /// yields, and the acceptance stage measures that both forms agree.
    pub fn confirm_title(&self, discovery_title: &str) -> TitleConfirmation {
        let carrying = |form: TitleForm| {
            self.strings
                .rows()
                .iter()
                .filter_map(|row| {
                    let display = strip_font_tag(row.text.as_deref()?);
                    (title_form(display, discovery_title) == Some(form)).then_some(row.id)
                })
                .collect::<Vec<_>>()
        };
        let verbatim = carrying(TitleForm::Verbatim);
        match verbatim.as_slice() {
            [only] => TitleConfirmation::Confirmed {
                row_id: *only,
                form: TitleForm::Verbatim,
            },
            // A verbatim row exists and it is not unique: the title names no
            // single row, and no long-name row may rescue it.
            [_, ..] => TitleConfirmation::Ambiguous,
            [] => match carrying(TitleForm::RegionPrefixedLongName).as_slice() {
                [only] => TitleConfirmation::Confirmed {
                    row_id: *only,
                    form: TitleForm::RegionPrefixedLongName,
                },
                [] => TitleConfirmation::Uncarried,
                [_, ..] => TitleConfirmation::Ambiguous,
            },
        }
    }

    /// What the localized table says about the campaign order the directory
    /// layout declares, and whether the two agree.
    ///
    /// The join from a work order to a retail mission directory is an
    /// inference, and one campaign-length row run is not evidence for it: a
    /// run of the right length would be produced by any 24 unrelated strings.
    /// The installation offers a second structure to check it against — the
    /// region-prefixed long names — and its *grouping* is checkable without
    /// claiming what any region name means: a long-name block whose rows fall
    /// into groups of the layout's per-chapter sizes describes a campaign laid
    /// out the way this one is. When such a block disagrees, the join is not
    /// established and no mission identity is derived from a title at all.
    ///
    /// A second check is available whenever the installation carries more than
    /// one campaign-length row block: the blocks must name the same missions in
    /// the same order ([`blocks_correspond`]). The two checks are combined by
    /// [`join_state`], so a disagreement from either one refuses the join and
    /// an installation that offers neither keeps the inference.
    pub fn join_agreement(&self) -> JoinAgreement {
        let layout_chapters = self.chapter_sizes();
        let blocks = self.campaign_title_blocks();
        let mut grouped = Vec::new();
        for &block in &blocks {
            if let Some(groups) = self.region_group_sizes(&block) {
                grouped.push(GroupedTitleBlock { block, groups });
            }
        }
        let state = join_state(
            &layout_chapters,
            &grouped,
            &self.block_correspondences(&blocks),
        );
        JoinAgreement {
            layout_chapters,
            blocks,
            grouped,
            state,
        }
    }

    /// The other localized spellings the installation carries for one campaign
    /// position: the display text of the row at the same offset of every
    /// campaign-length row block *other* than `confirmed`'s.
    ///
    /// This is how a binding reports that the two display forms disagree about
    /// one mission — the bare short name omitting a word its long name carries.
    /// The texts are returned so the record can name what it read; they are
    /// not a substitute for the confirmed row, which stays the one the title
    /// was confirmed in.
    pub fn other_spellings(&self, position: usize, confirmed: u32) -> Vec<(u32, String)> {
        self.campaign_title_blocks()
            .into_iter()
            .filter(|block| !block.contains(confirmed))
            .filter_map(|block| {
                let id = block
                    .first_id()
                    .checked_add(u32::try_from(position).ok()?)?;
                if !block.contains(id) {
                    return None;
                }
                let row = self.strings.rows().iter().find(|row| row.id == id)?;
                let display = strip_font_tag(row.text.as_deref()?).to_owned();
                Some((id, display))
            })
            .collect()
    }

    /// How many consecutive rows of `block` share a region prefix, when every
    /// row of it carries one.
    ///
    /// The prefix is the part of a row's display text before its first
    /// `" - "` separator. That separator is an observed display convention of
    /// the long-name rows (`Hawaii - The Lost Treasure of Sir Francis
    /// Drake`), not a documented format: only the *grouping* it produces is
    /// used, never the name a group carries.
    fn region_group_sizes(&self, block: &TitleBlock) -> Option<Vec<usize>> {
        let mut groups: Vec<usize> = Vec::new();
        let mut previous: Option<String> = None;
        for id in block.first_id..=block.last_id() {
            let row = self.strings.rows().iter().find(|row| row.id == id)?;
            let display = strip_font_tag(row.text.as_deref()?);
            let prefix = region_prefix(display)?.to_owned();
            match &previous {
                Some(last) if *last == prefix => *groups.last_mut()? += 1,
                _ => groups.push(1),
            }
            previous = Some(prefix);
        }
        Some(groups)
    }

    /// The display text of every row of `block`, in row order, with the
    /// presentation tag stripped — the text [`blocks_correspond`] compares.
    ///
    /// [`None`] when a row of the block does not decode or holds no text after
    /// its tag. A block from [`Self::campaign_title_blocks`] always has a
    /// display text on every row, so this is total for those; the option keeps
    /// the comparison from assuming it and from silently comparing an empty
    /// text against another.
    fn block_displays(&self, block: &TitleBlock) -> Option<Vec<&str>> {
        let mut displays = Vec::with_capacity(block.len());
        for id in block.first_id..=block.last_id() {
            let row = self.strings.rows().iter().find(|row| row.id == id)?;
            let display = strip_font_tag(row.text.as_deref()?);
            if display.is_empty() {
                return None;
            }
            displays.push(display);
        }
        Some(displays)
    }

    /// One [`blocks_correspond`] measurement per unordered pair of the given
    /// campaign-length blocks, in block order.
    ///
    /// An empty vector means there were fewer than two blocks, so the
    /// installation offers no second block to check the first against —
    /// *unavailable*, not a disagreement. A pair whose rows cannot all be read
    /// as text does not correspond.
    fn block_correspondences(&self, blocks: &[TitleBlock]) -> Vec<bool> {
        let displays: Vec<Option<Vec<&str>>> = blocks
            .iter()
            .map(|block| self.block_displays(block))
            .collect();
        let mut correspondences = Vec::new();
        for (index, left) in displays.iter().enumerate() {
            for right in displays.iter().skip(index + 1) {
                correspondences.push(match (left, right) {
                    (Some(left), Some(right)) => blocks_correspond(left, right),
                    _ => false,
                });
            }
        }
        correspondences
    }

    /// Binds every work order `inventory` declares to the original data this
    /// context was read from.
    ///
    /// This is the whole-campaign production path of stage F50-B: one read of
    /// the installation ([`SourceContext::read`]), one [`SourceContext::bind`]
    /// per declared work order in inventory order, then one assembly over the
    /// frozen denominator ([`assemble_campaign`]). It is what separates the
    /// per-mission `M01-A` … `M24-A` stages — each answering for a single work
    /// order — from the campaign record the F50 coverage, closure and
    /// playthrough stages are measured against.
    ///
    /// Every work order is answered by *this* context, so every returned
    /// [`SourceBinding`] carries this context's installation fingerprint:
    /// profile continuity is a property of the call, not a hope a caller has
    /// to check afterwards. A work order whose discovery title the local
    /// strings carry in neither display form still produces a record, with an
    /// explicitly unresolved identity ([`SourceBinding::to_mission_binding`])
    /// — the denominator is never shrunk to the subset that happened to
    /// resolve (spec F50 non-negotiable behavior 5).
    ///
    /// # Errors
    ///
    /// Everything [`SourceContext::bind`] reports for any one work order (an
    /// unreadable program archive, or a fact that was read but cannot be
    /// turned into an id, a claim or a provenance), then everything
    /// [`assemble_campaign`] reports about the assembled set.
    pub fn bind_campaign(
        &self,
        inventory: &CampaignInventory,
    ) -> Result<BoundCampaign, SourceBindingError> {
        let mut sources = Vec::with_capacity(inventory.len());
        for (label, title) in inventory.iter() {
            sources.push(self.bind(label.clone(), title)?);
        }
        let bindings = assemble_campaign(inventory, &sources)?;
        Ok(BoundCampaign { bindings, sources })
    }
}

/// Why a confirmed localized row selects no campaign position: the row it was
/// confirmed in is carried by no single localized row.
pub const NO_CONFIRMED_ROW_REFUSAL: &str =
    "the discovery title is carried by no single localized row";

/// Why a confirmed localized row selects no campaign position when the
/// localized table contradicts the campaign layout the join would follow.
pub const CONTRADICTED_JOIN_REFUSAL: &str = "the localized mission-name rows do not fall into the \
     campaign directory layout — either their region groups do not match its chapter sizes, or two \
     campaign-length row blocks do not name the same missions in the same order — so the localized \
     table contradicts the layout and no position was derived from the title";

/// Why a confirmed localized row selects no campaign position when its row
/// block is not as long as the campaign.
pub const SHORT_ROW_BLOCK_REFUSAL: &str = "the localized row does not sit in a row block as long as the campaign the directory layout \
     declares, so no campaign position was derived from the title";

/// Turns a confirmed localized title row into a campaign position.
///
/// `title_row` is the id of the row that confirmed the discovery title, or
/// [`None`] when no single row does. `agreement` is the localized table's
/// account of the campaign. `Ok(index)` means the row sits in a row block
/// exactly as long as the campaign *and* the localized table does not
/// contradict the layout; the `Err` reason names which of the two failed, so
/// a binding records what is missing instead of an empty identity.
///
/// The whole rule is a pure function so it can be exercised on every arm —
/// including the contradiction no retail installation produces — without an
/// installation.
pub fn campaign_position_for(
    title_row: Option<u32>,
    agreement: &JoinAgreement,
) -> Result<usize, &'static str> {
    let row = title_row.ok_or(NO_CONFIRMED_ROW_REFUSAL)?;
    // `establishes`, not a second reading of `state` here: the guard a binding
    // obeys and the predicate callers read are one rule, so they cannot drift.
    if !agreement.establishes() {
        return Err(CONTRADICTED_JOIN_REFUSAL);
    }
    agreement
        .blocks
        .iter()
        .find(|block| block.contains(row))
        .map(|block| (row - block.first_id()) as usize)
        .ok_or(SHORT_ROW_BLOCK_REFUSAL)
}

/// Why a discovery title selects no localized row: no row carries it in either
/// display form.
pub const UNCARRIED_TITLE_REFUSAL: &str = "no localized string carries the discovery title, neither \
     verbatim nor as the title part of a region-prefixed long name";

/// Why a discovery title selects no localized row: several rows carry it, so
/// no single row names a mission and therefore no single campaign position.
pub const AMBIGUOUS_TITLE_REFUSAL: &str = "several localized strings carry the discovery title, so \
     no single row names a mission and no campaign position was derived from the title";

/// Which display form of a localized string row carried a discovery title.
///
/// Both forms are observed retail display conventions of the campaign-length
/// mission-name rows, not a documented format, and the text is compared
/// exactly in either case. What the form says is *where* in the row the title
/// was found, never what the mission is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleForm {
    /// The row's display text is the discovery title, byte for byte. This is
    /// the strongest form and it wins whenever any row offers it.
    Verbatim,
    /// The row's display text is a region-prefixed long name — a region, the
    /// observed `" - "` separator and a title — and the part after the
    /// separator is the discovery title, byte for byte. The prefix is dropped
    /// without ever being compared with or interpreted as anything, exactly as
    /// [`SourceContext::region_group_sizes`] groups rows by it.
    RegionPrefixedLongName,
}

/// How one localized row carries a discovery title.
///
/// Pure over the two strings, so the rule a binding obeys is checkable on
/// every arm without an installation. A row carrying the title in neither
/// form is [`None`]; a tail that is merely *close* — a prefix of the title, the
/// title with different capitalization, the title with a prefix of its own —
/// is also [`None`], because no fuzzy comparison is made.
pub fn title_form(display: &str, title: &str) -> Option<TitleForm> {
    if display == title {
        return Some(TitleForm::Verbatim);
    }
    // Region-prefixed long names are the only rows allowed to carry a title
    // that is not the whole display text, and only after their separator.
    let (prefix, tail) = display.split_once(LONG_NAME_SEPARATOR)?;
    if prefix.is_empty() || tail.is_empty() {
        return None;
    }
    (tail == title).then_some(TitleForm::RegionPrefixedLongName)
}

/// The separator between a region prefix and a long mission name: an observed
/// display convention of those rows (`Hawaii - The Lost Treasure of Sir
/// Francis Drake`), never a documented format.
const LONG_NAME_SEPARATOR: &str = " - ";

/// How the localized table confirms one discovery title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleConfirmation {
    /// Exactly one localized row carries the title, in this display form.
    Confirmed {
        /// The row that carries it.
        row_id: u32,
        /// The display form that carried it.
        form: TitleForm,
    },
    /// No localized row carries the title in either form.
    Uncarried,
    /// More than one row carries it in the *same* display form — several
    /// verbatim rows, or several long names — so no single row identifies
    /// the mission. Carrying it once verbatim *and* once as a long name is
    /// not this: that is the both-forms case, and it confirms verbatim.
    Ambiguous,
}

impl TitleConfirmation {
    /// The confirmed row and form, when one row carries the title.
    pub fn confirmed(&self) -> Option<(u32, TitleForm)> {
        match self {
            Self::Confirmed { row_id, form } => Some((*row_id, *form)),
            Self::Uncarried | Self::Ambiguous => None,
        }
    }

    /// Why the confirmation failed, naming its own cause.
    ///
    /// [`None`] for a confirmation: a record that resolved its title has no
    /// refusal to report.
    pub fn refusal(&self) -> Option<&'static str> {
        match self {
            Self::Confirmed { .. } => None,
            Self::Uncarried => Some(UNCARRIED_TITLE_REFUSAL),
            Self::Ambiguous => Some(AMBIGUOUS_TITLE_REFUSAL),
        }
    }
}

/// One maximal run of consecutive localized string rows.
///
/// Only the boundaries are recorded: a block never holds text, so a binding
/// can name the row group it joined through without carrying a string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TitleBlock {
    first_id: u32,
    last_id: u32,
}

impl TitleBlock {
    /// A run spanning `first_id …= last_id`.
    ///
    /// [`None`] when `first_id` is after `last_id`: a run cannot end before it
    /// starts, and a zero-length block would let a caller claim an empty row
    /// group.
    pub const fn new(first_id: u32, last_id: u32) -> Option<Self> {
        if first_id > last_id {
            return None;
        }
        Some(Self { first_id, last_id })
    }

    /// The first row id of the run.
    pub const fn first_id(&self) -> u32 {
        self.first_id
    }

    /// The last row id of the run, inclusive.
    pub const fn last_id(&self) -> u32 {
        self.last_id
    }

    /// How many rows the run spans.
    pub const fn len(&self) -> usize {
        (self.last_id - self.first_id + 1) as usize
    }

    /// Always false: a run spans at least the one row it was found at.
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Whether `id` is one of the run's rows.
    pub const fn contains(&self, id: u32) -> bool {
        self.first_id <= id && id <= self.last_id
    }
}

impl fmt::Display for TitleBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.first_id, self.last_id)
    }
}

/// The maximal runs of consecutive ids in `present`, ascending by first id.
///
/// A gap in `present` ends a run. The function is public and takes only the
/// ids so the rule it implements can be exercised without an installation.
pub fn title_blocks(present: &BTreeSet<u32>) -> Vec<TitleBlock> {
    let mut blocks = Vec::new();
    let mut iter = present.iter().copied();
    let Some(mut first) = iter.next() else {
        return blocks;
    };
    let mut last = first;
    for id in iter {
        if id == last + 1 {
            last = id;
            continue;
        }
        blocks.push(TitleBlock {
            first_id: first,
            last_id: last,
        });
        first = id;
        last = id;
    }
    blocks.push(TitleBlock {
        first_id: first,
        last_id: last,
    });
    blocks
}

/// One campaign-length title block that carries a region prefix on every
/// row, with the group sizes those prefixes fall into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupedTitleBlock {
    /// The block the grouping was read from.
    pub block: TitleBlock,
    /// How many consecutive rows share a region prefix, in row order.
    pub groups: Vec<usize>,
}

/// How far the localized table corroborates the campaign order the directory
/// layout declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinCorroboration {
    /// Neither check is available: no campaign-length block carries a region
    /// prefix on every row, and there is no second campaign-length block to
    /// compare the first against. The join stays an inference and is recorded
    /// as one.
    Unavailable,
    /// At least one check is available and every available check agrees: every
    /// region-prefixed campaign-length block groups its rows exactly as the
    /// layout groups the campaign's chapters, and every pair of
    /// campaign-length blocks corresponds row to row. A check that is
    /// unavailable on its own is not a disagreement.
    Agreed,
    /// At least one check disagrees: a region-prefixed block groups its rows
    /// differently, or a pair of campaign-length blocks does not correspond.
    /// The join is not established and no position is derived from a title.
    Disagreed,
}

/// The localized table's account of the campaign, and the layout's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinAgreement {
    /// How many missions each chapter of the directory layout holds, in
    /// chapter order.
    pub layout_chapters: Vec<usize>,
    /// Every campaign-length title block, ascending by first row.
    pub blocks: Vec<TitleBlock>,
    /// The campaign-length blocks whose every row carries a region prefix.
    pub grouped: Vec<GroupedTitleBlock>,
    /// Whether every available check of the join agrees.
    pub state: JoinCorroboration,
}

impl JoinAgreement {
    /// Whether a campaign position may be derived from a title.
    pub fn establishes(&self) -> bool {
        self.state != JoinCorroboration::Disagreed
    }
}

/// Decides how far the localized table corroborates the campaign order.
///
/// Pure, so the rule that guards the join is checkable without an
/// installation: no grouped block is
/// [`JoinCorroboration::Unavailable`] (nothing to compare against, which is
/// not a contradiction), every grouped block matching `layout_chapters` is
/// [`JoinCorroboration::Agreed`], and any block that groups differently is
/// [`JoinCorroboration::Disagreed`].
pub fn classify_join(
    layout_chapters: &[usize],
    grouped: &[GroupedTitleBlock],
) -> JoinCorroboration {
    if grouped.is_empty() {
        JoinCorroboration::Unavailable
    } else if grouped.iter().all(|entry| entry.groups == layout_chapters) {
        JoinCorroboration::Agreed
    } else {
        JoinCorroboration::Disagreed
    }
}

/// Whether two equally long blocks of display texts name the same entries in
/// the same order.
///
/// The rule is a *mutual strict argmax of shared content tokens*: both blocks
/// must be non-empty and equally long, every
/// row of one must share at least one content token with its own row of the
/// other, and that own pairing must share strictly more tokens than any other
/// row of the opposite block — checked from both sides, so neither a row nor a
/// column is allowed to tie. A tie, a missing pairing or a length difference is
/// `false`: a permutation that ties carries no order information, and this
/// function guards the join, so it errs toward refusing.
///
/// Pure over the texts, so the arm a retail installation never produces is
/// reachable without one. The rule was measured to hold for all 24 retail rows
/// of `langui.dll` and to reject every non-zero rotation of the short-name
/// block; see `docs/findings/2026-10-02-m02-t3-title-row-correspondence.md`.
pub fn blocks_correspond(left: &[&str], right: &[&str]) -> bool {
    if left.is_empty() || left.len() != right.len() {
        return false;
    }
    let left_tokens: Vec<BTreeSet<String>> = left.iter().map(|text| content_tokens(text)).collect();
    let right_tokens: Vec<BTreeSet<String>> =
        right.iter().map(|text| content_tokens(text)).collect();
    let shared = |i: usize, j: usize| left_tokens[i].intersection(&right_tokens[j]).count();
    let rows = (0..left.len()).all(|i| {
        let own = shared(i, i);
        own > 0 && (0..left.len()).all(|j| j == i || shared(i, j) < own)
    });
    let columns = (0..left.len()).all(|j| {
        let own = shared(j, j);
        (0..left.len()).all(|i| i == j || shared(i, j) < own)
    });
    rows && columns
}

/// Decides how far a set of per-pair block-correspondence measurements
/// corroborates the join.
///
/// Pure and total: no measurement at all (fewer than two campaign-length
/// blocks, so there is no pair to measure) is
/// [`JoinCorroboration::Unavailable`]; every measured pair corresponding is
/// [`JoinCorroboration::Agreed`]; and any pair that does not correspond is
/// [`JoinCorroboration::Disagreed`] — a contradiction the guard acts on, never
/// a soft warning.
pub fn classify_correspondence(correspondences: &[bool]) -> JoinCorroboration {
    if correspondences.is_empty() {
        JoinCorroboration::Unavailable
    } else if correspondences.iter().all(|&corresponds| corresponds) {
        JoinCorroboration::Agreed
    } else {
        JoinCorroboration::Disagreed
    }
}

/// Combines two corroborations of the same join.
///
/// A contradiction decides on its own ([`JoinCorroboration::Disagreed`]); an
/// available agreement corroborates ([`JoinCorroboration::Agreed`]); and only
/// when neither check offers anything does the join stay
/// [`JoinCorroboration::Unavailable`]. The arguments are symmetric.
pub fn merge_corroboration(
    region_groups: JoinCorroboration,
    correspondence: JoinCorroboration,
) -> JoinCorroboration {
    match (region_groups, correspondence) {
        (JoinCorroboration::Disagreed, _) | (_, JoinCorroboration::Disagreed) => {
            JoinCorroboration::Disagreed
        }
        (JoinCorroboration::Agreed, _) | (_, JoinCorroboration::Agreed) => {
            JoinCorroboration::Agreed
        }
        (JoinCorroboration::Unavailable, JoinCorroboration::Unavailable) => {
            JoinCorroboration::Unavailable
        }
    }
}

/// The corroboration of the join over every structure the installation offers.
///
/// Pure and total, so the combination is reachable without an installation:
/// [`classify_join`] judges the region grouping, [`classify_correspondence`]
/// judges the row-to-row correspondence, and [`merge_corroboration`] decides
/// between them. A contradiction from either side is
/// [`JoinCorroboration::Disagreed`]; otherwise an available agreement is
/// [`JoinCorroboration::Agreed`]; and only two unavailable checks stay
/// [`JoinCorroboration::Unavailable`].
pub fn join_state(
    layout_chapters: &[usize],
    grouped: &[GroupedTitleBlock],
    correspondences: &[bool],
) -> JoinCorroboration {
    merge_corroboration(
        classify_join(layout_chapters, grouped),
        classify_correspondence(correspondences),
    )
}

/// The content tokens of a display text: the lowercase alphanumeric runs with
/// apostrophes kept inside a token, minus the articles `a`, `an` and `the`.
///
/// This is the unit [`blocks_correspond`] compares. It drops only what the
/// retail display convention varies on its own — capitalization and a leading
/// article — so it never invents a word either text does not carry, and it
/// keeps an apostrophe inside a token (`jack's` is not `jacks`) so two
/// genuinely different spellings cannot collapse.
fn content_tokens(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !(ch.is_alphanumeric() || ch == '\''))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .filter(|token| !matches!(token.as_str(), "a" | "an" | "the"))
        .collect()
}

/// One campaign mission's canonical mission id key.
fn mission_key(entry: &CampaignMission) -> String {
    format!("ch{}-m{:02}", entry.chapter, entry.mission_number)
}

/// One campaign mission's program id key: the reader archive of the world
/// group the mission is stored in.
fn program_key(entry: &CampaignMission) -> String {
    format!("{}-m{:02}-zrdr", entry.world_group, entry.mission_number)
}

/// Drops a leading presentation tag such as `[AB14I]` from a localized
/// string, so the *text* can be compared with a discovery title. The tag is
/// a display instruction, not part of the title.
fn strip_font_tag(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some(end) = rest.find(']') else {
        return text;
    };
    &text[end + 2..]
}

/// The region prefix of a region-prefixed long mission name: the part before
/// the first `" - "` separator.
///
/// `Hawaii - The Lost Treasure of Sir Francis Drake` carries `Hawaii`. The
/// separator is an observed display convention of those rows, not a
/// documented format, so only the grouping it produces is ever used — the
/// name itself is never bound to a chapter.
fn region_prefix(display: &str) -> Option<&str> {
    let (prefix, rest) = display.split_once(LONG_NAME_SEPARATOR)?;
    if prefix.is_empty() || rest.is_empty() {
        return None;
    }
    Some(prefix)
}

/// The byte range one decoded `RT_STRING` unit occupies inside its asset.
///
/// [`StringRow::span`] locates the whole `RT_STRING` block — up to sixteen
/// unrelated strings — and names no single string. The block's units are
/// encoded in order as a little-endian `u16` code-unit count followed by that
/// many UTF-16 code units, so the row's own range starts at the block's
/// `file_offset` plus the encoded lengths of every earlier unit and is
/// `2 + 2 * code_units.len()` bytes long. [`StringCatalog`] keeps both the
/// block extent and the decoded units, so this is arithmetic over bytes it
/// already read, not a second parse of the image.
///
/// [`None`] when no decoded block carries this row's id, so a caller cites the
/// enclosing block, says the row was not located, and never invents a range.
fn row_byte_span(catalog: &StringCatalog, row: &StringRow) -> Option<SourceSpan> {
    for block in catalog.resources().strings() {
        let mut offset = block.data.file_offset;
        for unit in &block.units {
            let length = 2 + 2 * unit.code_units.len() as u64;
            if unit.id == row.id {
                return SourceSpan::new(
                    row.span.install_sha256(),
                    row.span.container_path(),
                    row.span.member_key(),
                    offset,
                    length,
                    row.span.member_sha256(),
                )
                .ok();
            }
            offset += length;
        }
    }
    None
}

/// Reads one file, naming it in the error.
fn read_file(path: &Path) -> Result<Vec<u8>, SourceBindingError> {
    fs::read(path).map_err(|source| SourceBindingError::Io {
        path: path.display().to_string(),
        source,
    })
}

/// Names the provenance that could not be built.
fn map_provenance(error: cs_types::content::ProvenanceError) -> SourceBindingError {
    SourceBindingError::Inconsistent {
        reason: format!("provenance does not validate: {error}"),
    }
}

/// Walks `ZBD/<chapter><variant>/<mission>` and returns the campaign in
/// `(chapter, mission number)` order.
///
/// A chapter is a `ZBD/C<digits><letters>` directory and a mission is an
/// `M<digits>` directory inside one of them. Mission numbers must be unique
/// inside a chapter: two groups of one chapter holding the same mission
/// number would make the mission's world ambiguous, so the whole layout is
/// refused instead of picking one.
fn scan_campaign(install_root: &Path) -> Result<Vec<CampaignMission>, SourceBindingError> {
    let zbd = find_container_dir(install_root, "ZBD")?;
    let zbd_name = zbd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ZBD".to_owned());
    let entries = fs::read_dir(&zbd).map_err(|source| SourceBindingError::Io {
        path: zbd.display().to_string(),
        source,
    })?;
    let mut chapters: Vec<(u32, String, PathBuf)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SourceBindingError::Io {
            path: zbd.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(chapter) = chapter_number(&name) else {
            continue;
        };
        chapters.push((chapter, name, path));
    }

    let mut campaign: Vec<CampaignMission> = Vec::new();
    let mut seen = BTreeSet::new();
    for (chapter, group, path) in chapters {
        let groups = fs::read_dir(&path).map_err(|source| SourceBindingError::Io {
            path: path.display().to_string(),
            source,
        })?;
        for entry in groups {
            let entry = entry.map_err(|source| SourceBindingError::Io {
                path: path.display().to_string(),
                source,
            })?;
            let mission_path = entry.path();
            if !mission_path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(mission_number) = mission_number(&name) else {
                continue;
            };
            if !seen.insert((chapter, mission_number)) {
                return Err(SourceBindingError::Inconsistent {
                    reason: format!(
                        "chapter {chapter} stores mission {mission_number} in more than one world \
                         group, so the mission's world is ambiguous"
                    ),
                });
            }
            let program = mission_path.join("zrdr.zbd");
            campaign.push(CampaignMission {
                chapter,
                mission_number,
                world_group: group.to_ascii_lowercase(),
                program_asset: format!("{zbd_name}/{group}/{name}/zrdr.zbd"),
                program_present: program.is_file(),
            });
        }
    }
    campaign.sort_by_key(|entry| (entry.chapter, entry.mission_number));
    Ok(campaign)
}

/// The installation directory holding one container family, matched without
/// regard to case (the original installation spells it `ZBD`).
fn find_container_dir(install_root: &Path, wanted: &str) -> Result<PathBuf, SourceBindingError> {
    let entries = fs::read_dir(install_root).map_err(|source| SourceBindingError::Io {
        path: install_root.display().to_string(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| SourceBindingError::Io {
            path: install_root.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(wanted)
        {
            return Ok(path);
        }
    }
    Err(SourceBindingError::Io {
        path: install_root.join(wanted).display().to_string(),
        source: std::io::Error::from(std::io::ErrorKind::NotFound),
    })
}

/// The chapter a `ZBD/C<digits><letters>` directory name declares.
fn chapter_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(['c', 'C'])?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let suffix = &rest[digits.len()..];
    if !suffix.is_empty() && !suffix.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    digits.parse().ok()
}

/// The mission number an `M<digits>` directory name declares.
fn mission_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(['m', 'M'])?;
    if rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// The facts one work order was bound to, as read from the installation.
///
/// A binding is *source-derived* when every [`CriticalDependency`] is
/// resolved; it is *verified* only when nothing at all is left unknown,
/// which additionally needs the dependency closure hash and evidence claims
/// that later stages produce. Keeping those two states apart is deliberate:
/// a source-derived binding with `unknowns` says exactly what is still
/// missing instead of reading as finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBinding {
    /// The work-order discovery label the record belongs to.
    pub label: MissionLabel,
    /// The declared discovery title, confirmed against the local strings.
    pub discovery_title: String,
    /// The installation fingerprint the binding was read under.
    pub install_sha256: String,
    /// The binding's index in the retail campaign, when one was resolved.
    pub campaign_position: Option<usize>,
    /// How many missions the installation's campaign declares.
    pub campaign_size: usize,
    /// The canonical mission id, when one was resolved.
    pub catalog_id: Option<ContentId>,
    /// The world group/variant, when one was resolved.
    pub world_id: Option<ContentId>,
    /// The mission program identity, when one was resolved.
    pub program_id: Option<ContentId>,
    /// The localized string id that confirmed the discovery title.
    pub localized_title_id: Option<u32>,
    /// The language of that string.
    pub localized_title_language: Option<u32>,
    /// The five critical dependencies, in checklist order.
    pub dependencies: Vec<SourceDependency>,
    /// The original byte ranges the binding cites, with no bytes in them.
    pub source_spans: Vec<SourceSpanRecord>,
    /// Provenance source for the identity rows, when a program was read.
    pub identity_source: Option<SourceSpan>,
    /// Provenance source for the confirmed title string.
    ///
    /// When the title row's own bytes are recoverable this is the row's range,
    /// not the enclosing `RT_STRING` block: the block holds up to sixteen
    /// unrelated strings and identifies none of them. See also
    /// [`SourceBinding::title_enclosure`].
    pub title_source: Option<SourceSpan>,
    /// The enclosing `RT_STRING` block the confirmed title row was decoded
    /// from, when a title row exists.
    ///
    /// The localized UI image stores up to sixteen unrelated strings per
    /// `RT_STRING` block, so the block encloses the title without being it:
    /// [`SourceBinding::title_source`] (and the title entry of
    /// [`SourceBinding::source_spans`]) cite the row's own bytes. Keeping the
    /// enclosure distinct and named is what stops a reader from taking a
    /// block span for a string. [`None`] when the title was not confirmed or
    /// no title row exists.
    pub title_enclosure: Option<SourceSpan>,
    /// The mission dependency closure hash, produced by a later stage.
    pub closure_sha256: Option<String>,
    /// Evidence claim ids this binding rests on.
    pub evidence_ids: Vec<ClaimId>,
    /// The checklist entries that stay unknown, each saying so.
    pub unknowns: Vec<String>,
}

impl SourceBinding {
    /// The state of one critical dependency.
    pub fn dependency(&self, id: CriticalDependency) -> Option<&DependencyState> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.id == id)
            .map(|dependency| &dependency.state)
    }

    /// Every critical dependency that is not resolved, in checklist order.
    ///
    /// This is the M01-A acceptance question: an empty vector is "the
    /// source-derived binding has no unresolved critical dependencies".
    pub fn unresolved_critical(&self) -> Vec<CriticalDependency> {
        CriticalDependency::ALL
            .iter()
            .copied()
            .filter(|id| !matches!(self.dependency(*id), Some(DependencyState::Resolved { .. })))
            .collect()
    }

    /// Whether the binding says nothing is left unknown.
    ///
    /// True only when every critical dependency is resolved, no checklist
    /// entry remains unknown, the dependency closure hash is present and at
    /// least one evidence claim is recorded — the conditions
    /// `schemas/mission-binding.schema.json` imposes on `verified: true`.
    pub fn is_verified(&self) -> bool {
        self.unresolved_critical().is_empty()
            && self.unknowns.is_empty()
            && self.closure_sha256.is_some()
            && !self.evidence_ids.is_empty()
    }

    /// The record's own invariants.
    ///
    /// # Errors
    ///
    /// [`SourceBindingError::Inconsistent`] when a critical dependency is
    /// missing or repeated, when a resolved dependency has no value or an
    /// unresolved one still carries one, when the installation hash is not
    /// canonical lowercase hex, or when `verified` disagrees with the state
    /// the record actually holds.
    pub fn validate(&self) -> Result<(), SourceBindingError> {
        let inconsistent = |reason: String| SourceBindingError::Inconsistent { reason };

        if self.dependencies.len() != CriticalDependency::ALL.len() {
            return Err(inconsistent(format!(
                "expected {} critical dependency rows, found {}",
                CriticalDependency::ALL.len(),
                self.dependencies.len()
            )));
        }
        for (expected, found) in CriticalDependency::ALL.iter().zip(&self.dependencies) {
            if found.id != *expected {
                return Err(inconsistent(format!(
                    "critical dependency {} is recorded as {}",
                    expected.label(),
                    found.id.label()
                )));
            }
            let has_value = match found.id {
                CriticalDependency::MissionId => self.catalog_id.is_some(),
                CriticalDependency::InstallHash => true,
                CriticalDependency::TitleString => self.localized_title_id.is_some(),
                CriticalDependency::ProgramSourceMap => self.program_id.is_some(),
                CriticalDependency::WorldGroupVariant => self.world_id.is_some(),
            };
            match &found.state {
                DependencyState::Resolved { .. } if !has_value => {
                    return Err(inconsistent(format!(
                        "critical dependency {} is resolved but carries no value",
                        found.id.label()
                    )));
                }
                DependencyState::Unresolved { reason, .. } if has_value => {
                    return Err(inconsistent(format!(
                        "critical dependency {} is unresolved ({reason}) but carries a value",
                        found.id.label()
                    )));
                }
                DependencyState::Unresolved { reason, .. } if reason.trim().is_empty() => {
                    return Err(inconsistent(format!(
                        "critical dependency {} is unresolved without a reason",
                        found.id.label()
                    )));
                }
                _ => {}
            }
        }
        if self.install_sha256.len() != 64
            || !self
                .install_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(inconsistent(
                "the installation hash is not canonical lowercase hex".to_owned(),
            ));
        }
        if self.is_verified() != (self.unresolved_critical().is_empty() && self.unknowns.is_empty())
        {
            return Err(inconsistent(
                "verified disagrees with the unknowns the record still carries".to_owned(),
            ));
        }
        Ok(())
    }

    /// The binding record as `schemas/mission-binding.schema.json` describes
    /// it: the bytes committed as `missions/bindings/<label>.json`.
    ///
    /// The emission is hand-written like the rest of the workspace's JSON,
    /// and holds identities, hashes and spans only — never original content.
    pub fn to_json(&self) -> String {
        let optional = |value: &Option<String>| match value {
            Some(value) => json_string(value),
            None => "null".to_owned(),
        };
        let id = |value: &Option<ContentId>| match value {
            Some(value) => json_string(value.as_str()),
            None => "null".to_owned(),
        };
        let spans = self
            .source_spans
            .iter()
            .map(|span| {
                format!(
                    "{{\"asset_id\": {}, \"offset\": {}, \"length\": {}, \"sha256\": {}}}",
                    json_string(&span.asset_id),
                    span.offset,
                    span.length,
                    json_string(&span.sha256)
                )
            })
            .collect::<Vec<_>>();
        let unknowns: Vec<String> = self
            .unknowns
            .iter()
            .map(|entry| json_string(entry))
            .collect();
        let evidence: Vec<String> = self
            .evidence_ids
            .iter()
            .map(|claim| json_string(claim.as_str()))
            .collect();

        format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"work_order\": {},\n\
             \x20\"discovery_title\": {},\n\
             \x20\"verified\": {},\n\
             \x20\"install_sha256\": {},\n\
             \x20\"catalog_id\": {},\n\
             \x20\"world_id\": {},\n\
             \x20\"program_id\": {},\n\
             \x20\"closure_sha256\": {},\n\
             \x20\"source_spans\": {},\n\
             \x20\"required_actor_ids\": [],\n\
             \x20\"objective_ids\": [],\n\
             \x20\"interaction_ids\": [],\n\
             \x20\"media_ids\": [],\n\
             \x20\"stunt_ids\": [],\n\
             \x20\"reward_ids\": [],\n\
             \x20\"difficulty_ids\": [],\n\
             \x20\"unknowns\": {},\n\
             \x20\"evidence_ids\": {}\n\
             }}\n",
            json_string(self.label.as_str()),
            json_string(&self.discovery_title),
            self.is_verified(),
            json_string(&self.install_sha256),
            id(&self.catalog_id),
            id(&self.world_id),
            id(&self.program_id),
            optional(&self.closure_sha256),
            array_of_objects(&spans),
            array_of_strings(&unknowns),
            array_of_strings(&evidence),
        )
    }

    /// The campaign record this binding contributes: the identity category
    /// bound when every identity was resolved, everything else explicitly
    /// unresolved, every required subsystem row unresolved because the
    /// subsystems themselves are not implemented yet, progression unknown.
    ///
    /// # Errors
    ///
    /// [`BindingError`] when a claim id, role or reason built here is
    /// rejected by the record's admission rules.
    pub fn to_mission_binding(&self) -> Result<MissionBinding, BindingError> {
        let claim = ClaimId::new(&format!(
            "{}.source_binding",
            self.label.as_str().to_ascii_lowercase()
        ))?;
        let mut categories = BTreeMap::new();

        match (&self.catalog_id, &self.world_id, &self.program_id) {
            (Some(mission), Some(world), Some(program)) => {
                // `Provenance::new` only refuses a `verified_original` claim
                // with no source span; an inferred identity never does.
                let provenance = Provenance::new(
                    ClaimId::new("source_binding.identity").map_err(BindingError::ClaimId)?,
                    ClaimStatus::Inferred,
                    self.identity_source.clone(),
                )
                .map_err(|_| BindingError::EmptyReason {
                    what: "the identity provenance",
                })?;
                categories.insert(
                    BindingCategory::MissionIdentity,
                    CategoryState::rows(vec![
                        BindingRow::content(MISSION_ROLE, mission.clone(), provenance.clone())?,
                        BindingRow::content(WORLD_ROLE, world.clone(), provenance.clone())?,
                        BindingRow::content(PROGRAM_ROLE, program.clone(), provenance)?,
                    ])?,
                );
            }
            _ => {
                categories.insert(
                    BindingCategory::MissionIdentity,
                    CategoryState::unresolved(
                        claim.clone(),
                        "the source-derived mission identity is incomplete",
                    )?,
                );
            }
        }
        for category in BindingCategory::ALL {
            if !categories.contains_key(category) {
                categories.insert(
                    *category,
                    CategoryState::unresolved(claim.clone(), UNBOUND_CATEGORY_REASON)?,
                );
            }
        }

        let dependencies = REQUIRED_SUBSYSTEMS
            .iter()
            .map(|subsystem| {
                Ok(SubsystemDependency {
                    subsystem: SubsystemId::new(subsystem)?,
                    state: DependencyState::unresolved(claim.clone(), UNBOUND_SUBSYSTEM_REASON)?,
                })
            })
            .collect::<Result<Vec<_>, BindingError>>()?;

        Ok(MissionBinding {
            label: self.label.clone(),
            discovery_title: Some(self.discovery_title.clone()),
            categories,
            dependencies,
            progression: Progression::unknown(claim, UNBOUND_PROGRESSION_REASON)?,
            placeholder: false,
        })
    }
}

/// One campaign bound from one installation: the record, and the source
/// bindings it was assembled from.
///
/// [`SourceContext::bind_campaign`] returns both halves because they answer
/// different questions and neither may stand in for the other. `bindings` is
/// what coverage, closure and readiness are measured against — the frozen
/// denominator with one [`MissionBinding`] per declared work order. `sources`
/// is what those records were derived from: each work order's
/// [`SourceBinding`] in declared inventory order, carrying the installation
/// fingerprint it was read under, the campaign position its discovery title
/// resolved to, the identities it located and the byte ranges it cites.
///
/// A reader that needs *where an identity came from* reads `sources`; a
/// reader that needs *what the campaign holds* reads `bindings`. Keeping them
/// in one value is what lets a caller assert profile continuity — every
/// source under one installation fingerprint — without deriving the campaign
/// a second time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundCampaign {
    /// The campaign record over the frozen denominator.
    pub bindings: CampaignBindings,
    /// Every work order's source binding, in declared inventory order:
    /// `sources[i]` is the record derived for `inventory`'s line `i`.
    pub sources: Vec<SourceBinding>,
}

/// Assembles source-derived bindings into the campaign over `inventory`.
///
/// This is the assembly rule [`SourceContext::bind_campaign`] obeys, lifted
/// into a function of its own so the rule can be exercised without an
/// installation: the denominator is frozen first from `inventory`
/// ([`CampaignBindings::from_inventory`]), and every supplied binding
/// replaces its work order's placeholder exactly once
/// ([`CampaignBindings::bind`]).
///
/// What this function refuses is everything that would make a declared work
/// order *silently* lighter: a supplied binding naming a work order the
/// inventory does not declare (an undeclared label would otherwise grow the
/// denominator without a `declare` call), two supplied bindings naming one
/// work order (a placeholder is bound once), and a declared work order
/// supplied no binding at all (its placeholder would otherwise survive and
/// read as an ordinary unresolved mission rather than as a missing input).
///
/// What it never inspects is whether an identity *resolved*. A source binding
/// whose mission id, world group or program could not be located still
/// replaces its placeholder, through [`SourceBinding::to_mission_binding`],
/// with an explicitly unresolved identity category; the other six required
/// categories and all [`REQUIRED_SUBSYSTEMS`] rows stay unresolved because
/// this stage reads original data, it does not implement subsystems. A
/// mission that cannot be bound is recorded as unbound — never dropped from
/// the denominator, never read as complete.
///
/// # Errors
///
/// [`SourceBindingError::Inconsistent`] naming the offending work order for
/// each refusal above, and [`SourceBindingError::Inconsistent`] when a
/// built record fails [`MissionBinding::validate`] (carried through
/// [`From<BindingError>`]).
pub fn assemble_campaign(
    inventory: &CampaignInventory,
    sources: &[SourceBinding],
) -> Result<CampaignBindings, SourceBindingError> {
    let mut campaign = CampaignBindings::from_inventory(inventory)?;

    let mut seen = BTreeSet::new();
    for source in sources {
        let label = &source.label;
        if campaign.get(label).is_none() {
            return Err(SourceBindingError::Inconsistent {
                reason: format!(
                    "source binding for {label} names a work order the declared inventory does \
                     not declare, so assembling it would grow the denominator without a declare"
                ),
            });
        }
        if !seen.insert(label.clone()) {
            return Err(SourceBindingError::Inconsistent {
                reason: format!(
                    "work order {label} was supplied more than one source binding, and a \
                     placeholder is bound exactly once"
                ),
            });
        }
        campaign.bind(source.to_mission_binding()?)?;
    }

    for label in inventory.labels() {
        let recorded = campaign
            .get(label)
            .expect("the frozen denominator records every declared work order");
        if recorded.is_placeholder() {
            return Err(SourceBindingError::Inconsistent {
                reason: format!(
                    "declared work order {label} was supplied no source binding, so it would \
                     stay a placeholder instead of a bound record"
                ),
            });
        }
    }

    Ok(campaign)
}

/// Why a category outside `mission_identity` is unresolved in a
/// source-derived record.
const UNBOUND_CATEGORY_REASON: &str =
    "not bound from original data; see the binding record's unknowns";

/// Why a required subsystem row is unresolved in a source-derived record:
/// the binding reads original data, it does not implement subsystems.
const UNBOUND_SUBSYSTEM_REASON: &str =
    "subsystem not implemented; M01-A binds source identity only";

/// Why progression is unknown in a source-derived record.
const UNBOUND_PROGRESSION_REASON: &str = "campaign progression not bound to original mission ids";

/// A JSON string literal: quoted and escaped, so no record field can break
/// out of its string.
fn json_string(value: &str) -> String {
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

/// A JSON array of already-rendered objects: `[]` when empty, otherwise one
/// object per line inside the brackets.
fn array_of_objects(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    let joined = items
        .iter()
        .map(|item| format!("\n    {item}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{joined}\n  ]")
}

/// A JSON array of already-rendered strings on one line.
fn array_of_strings(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    format!("[{}]", items.join(", "))
}

// ---------------------------------------------------------------------------
// Per-mission probe routes (F50-C)
// ---------------------------------------------------------------------------

/// One interruption a per-mission probe route must survive.
///
/// The set is exactly the minimum acceptance scenario of spec F50-C
/// (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
/// section `### F50-C`): "Retry selected missions after death, bailout,
/// skip-media, save/restart and settings changes". It is a closed enum so a
/// new retry case is a reviewed change to the scenario itself, never a string
/// a caller can forget to plan for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProbeInterruption {
    /// The player's aircraft is destroyed and the mission is retried.
    Death,
    /// The pilot bails out and the mission is retried.
    Bailout,
    /// Linked media is skipped and the mission starts (or is retried) without
    /// it.
    SkipMedia,
    /// The profile is saved and the application restarted; the mission is
    /// entered again from the stored profile.
    SaveRestart,
    /// A settings change is applied mid-session and the mission is retried.
    SettingsChange,
}

impl ProbeInterruption {
    /// The complete minimum scenario, in the order the spec names it.
    pub const ALL: [ProbeInterruption; 5] = [
        Self::Death,
        Self::Bailout,
        Self::SkipMedia,
        Self::SaveRestart,
        Self::SettingsChange,
    ];

    /// The stable identity of the interruption: `DEATH`, `BAILOUT`,
    /// `SKIP_MEDIA`, `SAVE_RESTART`, `SETTINGS_CHANGE`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Death => "DEATH",
            Self::Bailout => "BAILOUT",
            Self::SkipMedia => "SKIP_MEDIA",
            Self::SaveRestart => "SAVE_RESTART",
            Self::SettingsChange => "SETTINGS_CHANGE",
        }
    }

    /// What the interruption does to a running mission, one sentence each.
    pub const fn scenario(self) -> &'static str {
        match self {
            Self::Death => "the mission ends in aircraft destruction and is retried",
            Self::Bailout => "the pilot bails out and the mission is retried",
            Self::SkipMedia => "linked media is skipped and the mission continues without it",
            Self::SaveRestart => {
                "the profile is saved, the process restarts and the mission is entered again"
            }
            Self::SettingsChange => "a settings change is applied and the mission is retried",
        }
    }
}

impl fmt::Display for ProbeInterruption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where one interruption's reentry lands: the mission identity, and the
/// installation fingerprint every value was derived under.
///
/// The three ids are copied from the work order's own source binding
/// ([`SourceBinding`]) — never re-parsed, never defaulted — so a reentry is
/// exactly the identity the campaign was bound to. The fingerprint is what
/// makes a *save/restart* reentry checkable: a restart that re-enters under a
/// different installation is a different game, not a retry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeReentry {
    /// The interruption this reentry answers.
    pub interruption: ProbeInterruption,
    /// The mission identity re-entered.
    pub mission: ContentId,
    /// The world variant re-entered.
    pub world: ContentId,
    /// The mission program re-entered.
    pub program: ContentId,
    /// The installation fingerprint the reentry is anchored to.
    pub install_sha256: String,
}

/// The probe route of one declared work order.
///
/// A route is either **ready** — all five [`ProbeInterruption::ALL`] entries
/// planned, each carrying the mission identity the work order resolved to —
/// or **refused** — the identity did not resolve, no reentry can name a
/// mission, and [`MissionProbeRoute::refusal`] says why. There is no third
/// state: a route is never silently partial, and a refused route is never
/// dropped from the plan (spec F50 non-negotiable behavior 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionProbeRoute {
    /// The work order this route belongs to.
    pub label: MissionLabel,
    /// The declared discovery title the route was planned under.
    pub discovery_title: String,
    /// The retail campaign position, when one was resolved.
    pub campaign_position: Option<usize>,
    /// The installation fingerprint every reentry of this route re-enters
    /// under — the fingerprint the source binding was read under.
    pub install_sha256: String,
    /// One planned reentry per [`ProbeInterruption::ALL`], in that order.
    /// Empty exactly when the route is refused.
    pub reentries: Vec<ProbeReentry>,
    /// Why the route cannot be probed, when it cannot.
    pub refusal: Option<String>,
}

impl MissionProbeRoute {
    /// Whether every interruption of the minimum scenario has a planned
    /// reentry. False exactly for a refused route.
    pub fn is_ready(&self) -> bool {
        self.refusal.is_none() && self.reentries.len() == ProbeInterruption::ALL.len()
    }

    /// The planned reentry of one interruption, when the route is ready.
    pub fn reentry(&self, interruption: ProbeInterruption) -> Option<&ProbeReentry> {
        self.reentries
            .iter()
            .find(|reentry| reentry.interruption == interruption)
    }

    /// The mission identity every reentry of this route lands on, when the
    /// route is ready.
    pub fn mission_id(&self) -> Option<&ContentId> {
        self.reentries.first().map(|reentry| &reentry.mission)
    }
}

/// Every declared work order's probe route, in declared denominator order.
///
/// The plan inherits the denominator's frozen shape: one route per declared
/// work order, ready or refused, none dropped, none added. Its single
/// installation fingerprint ([`ProbePlan::install_sha256`]) is the save/restart
/// anchor of the whole campaign — planning a campaign that was read under two
/// fingerprints is refused, because "restart and re-enter" would then name two
/// different games.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbePlan {
    routes: Vec<MissionProbeRoute>,
}

impl ProbePlan {
    /// Every route, in declared denominator order.
    pub fn routes(&self) -> &[MissionProbeRoute] {
        &self.routes
    }

    /// How many routes the plan holds (one per declared work order).
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Whether the plan holds no route at all.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// The route of one work order.
    pub fn route(&self, label: &MissionLabel) -> Option<&MissionProbeRoute> {
        self.routes.iter().find(|route| &route.label == label)
    }

    /// The routes that can be probed right now.
    pub fn ready(&self) -> impl Iterator<Item = &MissionProbeRoute> {
        self.routes.iter().filter(|route| route.is_ready())
    }

    /// The routes an unresolved identity refused, each naming its reason.
    pub fn refused(&self) -> impl Iterator<Item = &MissionProbeRoute> {
        self.routes.iter().filter(|route| !route.is_ready())
    }

    /// How many routes are ready.
    pub fn ready_count(&self) -> usize {
        self.routes.iter().filter(|route| route.is_ready()).count()
    }

    /// How many routes an unresolved identity refused.
    pub fn refused_count(&self) -> usize {
        self.len() - self.ready_count()
    }

    /// The one installation fingerprint every route re-enters under.
    ///
    /// # Panics
    ///
    /// Only on a plan built by [`probe_routes`], which always fills it; an
    /// empty plan cannot come from a declared inventory.
    pub fn install_sha256(&self) -> &str {
        &self
            .routes
            .first()
            .expect("a planned campaign declares at least one work order")
            .install_sha256
    }
}

/// Plans one probe route per declared work order of a bound campaign.
///
/// The producer is F50-B's [`SourceContext::bind_campaign`]; this is the
/// consumer-side rule that turns its output into the retry contract later
/// stages drive. The denominator is read first (every declared work order of
/// `campaign.bindings` must have a source binding, in the same terms
/// [`assemble_campaign`] refuses a missing one), then each work order's route
/// is planned from its own [`SourceBinding`]:
///
/// * all three identities resolved → a **ready** route whose five reentries
///   carry those identities and the binding's installation fingerprint;
/// * any identity unresolved → a **refused** route naming the unresolved
///   [`CriticalDependency`]s, still counted, still in its declared position.
///
/// What it refuses with an error — rather than planning around it — is every
/// input the route contract could not stand on: a source binding naming a
/// work order the denominator does not declare (the plan would grow it
/// without a `declare`), two source bindings naming one work order (a route
/// is planned once), a declared work order with no source binding (its route
/// would silently vanish), a fingerprint that is not canonical lowercase hex
/// (the save/restart anchor would be unreadable), and sources read under two
/// different fingerprints (a restart could not re-enter "the same game").
///
/// # Errors
///
/// [`SourceBindingError::Inconsistent`] naming the offending work order(s)
/// for each refusal above.
pub fn probe_routes(campaign: &BoundCampaign) -> Result<ProbePlan, SourceBindingError> {
    let inconsistent = |reason: String| SourceBindingError::Inconsistent { reason };

    let mut by_label: BTreeMap<&MissionLabel, &SourceBinding> = BTreeMap::new();
    let mut anchor: Option<(&MissionLabel, &str)> = None;
    for source in &campaign.sources {
        if campaign.bindings.get(&source.label).is_none() {
            return Err(inconsistent(format!(
                "source binding for {} names a work order the declared inventory does not \
                 declare, so planning its route would grow the denominator without a declare",
                source.label
            )));
        }
        if by_label.insert(&source.label, source).is_some() {
            return Err(inconsistent(format!(
                "work order {} was supplied more than one source binding, and a probe route is \
                 planned exactly once",
                source.label
            )));
        }
        if source.install_sha256.len() != 64
            || !source
                .install_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(inconsistent(format!(
                "work order {} was read under {}, which is not a canonical lowercase hex \
                 installation fingerprint, so a save/restart reentry could not be anchored to it",
                source.label, source.install_sha256
            )));
        }
        anchor = match anchor {
            None => Some((&source.label, source.install_sha256.as_str())),
            Some((first_label, first_fingerprint)) => {
                if *first_fingerprint != source.install_sha256 {
                    return Err(inconsistent(format!(
                        "work orders {first_label} and {} were read under different installation \
                         fingerprints ({first_fingerprint} and {}), so a restart could not \
                         re-enter the same game for the whole campaign",
                        source.label, source.install_sha256
                    )));
                }
                Some((first_label, first_fingerprint))
            }
        };
    }

    let mut routes = Vec::new();
    for label in campaign.bindings.declared() {
        let source = by_label.get(label).copied().ok_or_else(|| {
            inconsistent(format!(
                "declared work order {label} was supplied no source binding, so no probe route \
                 could be planned for it and it would silently vanish from the plan"
            ))
        })?;
        routes.push(plan_route(source));
    }

    Ok(ProbePlan { routes })
}

/// Plans one work order's route from its source binding: a ready route with
/// one reentry per [`ProbeInterruption::ALL`] when every identity resolved,
/// a refused route naming the unresolved critical dependencies otherwise.
fn plan_route(source: &SourceBinding) -> MissionProbeRoute {
    let identity = match (&source.catalog_id, &source.world_id, &source.program_id) {
        (Some(mission), Some(world), Some(program)) => Some((mission, world, program)),
        _ => None,
    };
    let (reentries, refusal) = match identity {
        Some((mission, world, program)) => (
            ProbeInterruption::ALL
                .iter()
                .map(|interruption| ProbeReentry {
                    interruption: *interruption,
                    mission: mission.clone(),
                    world: world.clone(),
                    program: program.clone(),
                    install_sha256: source.install_sha256.clone(),
                })
                .collect(),
            None,
        ),
        None => {
            let mut missing = Vec::new();
            if source.catalog_id.is_none() {
                missing.push(CriticalDependency::MissionId.label());
            }
            if source.world_id.is_none() {
                missing.push(CriticalDependency::WorldGroupVariant.label());
            }
            if source.program_id.is_none() {
                missing.push(CriticalDependency::ProgramSourceMap.label());
            }
            (
                Vec::new(),
                Some(format!(
                    "{} has no probe route to run: the mission identity is incomplete, the \
                     installation located no {}",
                    source.label,
                    missing.join(", ")
                )),
            )
        }
    };
    MissionProbeRoute {
        label: source.label.clone(),
        discovery_title: source.discovery_title.clone(),
        campaign_position: source.campaign_position,
        install_sha256: source.install_sha256.clone(),
        reentries,
        refusal,
    }
}
