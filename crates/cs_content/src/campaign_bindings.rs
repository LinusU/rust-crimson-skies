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
/// `sha256` is the digest of the whole asset named by `asset_id`;
/// `offset`/`length` locate this span inside it. A span carries no bytes, so
/// a binding record can cite original data without containing any.
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
    /// assumed to match. A title the local strings do not carry, or carry
    /// more than once, leaves [`CriticalDependency::TitleString`]
    /// unresolved — which in turn leaves the mission, world and program
    /// unresolved, because there is no campaign position to read them from.
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
        let matches: Vec<&StringRow> = self
            .strings
            .rows()
            .iter()
            .filter(|row| {
                row.text
                    .as_deref()
                    .is_some_and(|text| strip_font_tag(text) == discovery_title)
            })
            .collect();
        let title_row = match matches.as_slice() {
            [only] => Some(*only),
            _ => None,
        };
        let title_reason = if matches.is_empty() {
            "no local string equals the discovery title"
        } else {
            "several local strings equal the discovery title"
        };

        let mut title_source = None;
        if let Some(row) = title_row {
            title_source = Some(row.span.clone());
            source_spans.push(SourceSpanRecord {
                asset_id: self.string_asset.clone(),
                offset: row.span.offset(),
                length: row.span.length(),
                sha256: self.string_asset_sha256.clone(),
            });
        }

        // --- campaign position: the localized titles form one contiguous id
        // block, and that block must be exactly as long as the campaign the
        // directory layout declares. Anything else is not a position.
        let position = title_row.and_then(|row| self.campaign_position(row.id));
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
                    None => DependencyState::unresolved(id.claim(&label)?, title_reason)?,
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
                        "the campaign position could not be resolved, so no original mission id \
                         was located",
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
                        "the world group could not be located because the campaign position is \
                         unknown",
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
                        "the mission program archive was not located in the installation",
                    )?,
                },
            };
            dependencies.push(SourceDependency { id, state });
        }

        let unknowns = SOURCE_BINDING_UNKNOWNS
            .iter()
            .map(|entry| (*entry).to_owned())
            .collect();

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
            closure_sha256: None,
            evidence_ids: Vec::new(),
            unknowns,
        })
    }

    /// The index of the contiguous localized-title block `id` sits in, when
    /// that block is exactly as long as the declared campaign.
    fn campaign_position(&self, id: u32) -> Option<usize> {
        let present: BTreeSet<u32> = self
            .strings
            .rows()
            .iter()
            .filter(|row| {
                row.text
                    .as_deref()
                    .is_some_and(|text| !strip_font_tag(text).is_empty())
            })
            .map(|row| row.id)
            .collect();
        let mut start = id;
        while start > 0 && present.contains(&(start - 1)) {
            start -= 1;
        }
        let mut end = id;
        while present.contains(&(end + 1)) {
            end += 1;
        }
        if usize::try_from(end - start + 1).ok()? != self.campaign.len() {
            return None;
        }
        Some((id - start) as usize)
    }
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
    pub title_source: Option<SourceSpan>,
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
