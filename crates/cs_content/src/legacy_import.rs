//! The legacy-import plan: a read-only, validated intent to import one legacy
//! source into one **new** profile (stage F64-A).
//!
//! Spec `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, section
//! `### F64-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The sheet's stage A instruction is "Define typed inputs/outputs and a
//! minimal synthetic fixture first; do not jump ahead to a whole runtime", so
//! this module is deliberately **plan-only**. [`plan_import`] is a pure
//! function: it takes the legacy source's bytes and its
//! [`ArtifactProposal`], a [`LegacyLayout`], [`LegacyLimits`], a
//! [`LegacyIdMap`], the [`Catalog`] and a [`TargetProfile`], and it returns
//! either an [`ImportPlan`] describing what *would* be imported, or an
//! [`ImportRefusal`] naming exactly why not. It holds no file handle, takes no
//! `&mut` anything, has no write path and never learns a host path, so it is
//! *structurally* incapable of writing to a source file or to a new save. The
//! AC01 test pins that on a real filesystem by comparing the source bytes, the
//! source's modification time and the whole destination directory before and
//! after a hostile payload is offered.
//!
//! Five properties are load-bearing, and each is enforced by a test rather
//! than a comment:
//!
//! 1. **The source fingerprint is retained, not recomputed from the copy.**
//!    [`SourceFingerprint`] records the spelling, size and SHA-256 of the
//!    bytes the plan was made from, and [`plan_import`] refuses a proposal
//!    whose declared size does not match the bytes supplied — so a plan can
//!    never describe a file that was not the one read (spec F64 non-negotiable
//!    1: "retain source fingerprint").
//! 2. **Ids resolve through content identities, never through positions.**
//!    [`LegacyIdMap`] is keyed by the legacy raw id and holds a
//!    [`ContentId`]; [`LegacyIdMap::resolve`] looks that id up in the
//!    [`Catalog`] and requires the element to be *ready*. There is no index
//!    arithmetic anywhere in the module, and reordering catalog rows cannot
//!    change an outcome (spec F64 non-negotiable 3).
//! 3. **Unknown stays unknown.** A record with bytes no declared slot covers, a
//!    record whose id is not mapped, and a target that is not ready all become
//!    named [`UnresolvedRow`]s. Nothing is defaulted, rounded or guessed, and
//!    no currency, mission index or equipment value is ever inferred from a
//!    nearby number (spec F64 non-negotiable 2).
//! 4. **Full, partial and unsupported are distinct, and "empty" is never
//!    "imported".** [`ImportClass`] has exactly those three states, a document
//!    with no records can never be [`ImportClass::Full`], and a class other
//!    than `Full` always carries its reasons (spec F64 non-negotiable 5).
//! 5. **Only a measured layout may be imported by default.**
//!    [`plan_import`] refuses a layout whose evidence is
//!    [`ClaimStatus::Designed`] or weaker; reading the designed fixture layout
//!    requires the caller to say so explicitly through
//!    [`LayoutAdmission::AllowDesignedFixtures`], which is what the acceptance
//!    tests do and what no production caller should.
//!
//! **Designed and synthetic, not original data.** No legacy profile, save or
//! custom-aircraft file was opened for this stage: it used ordinary
//! build/test only, and the layout evidence states in
//! `cs_formats::legacy_profile::LEGACY_LAYOUT_INVENTORY` are all
//! [`ClaimStatus::Unknown`]. Nothing here is `verified_original`, and the
//! byte layout, version field, id encoding and storage location of every
//! original legacy artifact remain unmeasured (F64-B, which needs `retail`).
//! What *is* claimed here is the contract: the typed inputs and outputs, the
//! refusal taxonomy, the identity-based id resolution and the plan-only shape
//! the later stages will fill with a measured layout and a real consumer.

use std::fmt;

use cs_assets::install::sha256;
use cs_formats::legacy_profile::{
    ArtifactProposal, ArtifactProposalError, ImportRequirement, LegacyArtifactClass, LegacyIdClass,
    LegacyLayout, LegacyLimits, LegacyProfileError, LegacyProfileErrorKind,
    MAX_LEGACY_SOURCE_BYTES, layout_record, read_legacy_profile,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimStatus;
use cs_types::install::InstallIdentity;
use cs_types::profile::{ProfileId, ProfileKind};

use crate::catalog::Catalog;

/// Which layout evidence a plan may be made from.
///
/// The default ([`Self::MeasuredOnly`]) is the only one a production caller
/// should pass: it accepts a layout backed by fingerprinted original bytes or
/// by a public format document, and refuses a designed or unknown one. The
/// second variant exists so the synthetic acceptance tests can read a designed
/// fixture layout; naming it at the call site is the point, so a designed
/// layout can never be reached by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayoutAdmission {
    /// Only measured evidence may be imported.
    #[default]
    MeasuredOnly,
    /// Additionally admit a designed fixture layout, for synthetic tests.
    ///
    /// The resulting [`ImportPlan`] records
    /// [`ImportPlan::admitted_designed_layout`] so a report can never present
    /// fixture data as a measured import.
    AllowDesignedFixtures,
}

impl LayoutAdmission {
    /// Whether a layout with this evidence may be imported.
    #[must_use]
    pub const fn admits(self, evidence: ClaimStatus) -> bool {
        match self {
            Self::MeasuredOnly => matches!(
                evidence,
                ClaimStatus::VerifiedOriginal | ClaimStatus::Documented
            ),
            Self::AllowDesignedFixtures => matches!(
                evidence,
                ClaimStatus::VerifiedOriginal
                    | ClaimStatus::Documented
                    | ClaimStatus::Designed
                    | ClaimStatus::ObservedTool
            ),
        }
    }
}

/// The identity of the legacy source a plan was made from.
///
/// Retained verbatim (spec F64 non-negotiable 1). The fingerprint describes the
/// bytes that were *read*; the plan never writes them, so the fingerprint is
/// the only way a later stage can prove afterwards which file an import came
/// from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFingerprint {
    spelling: String,
    size_bytes: u64,
    sha256: cs_types::evidence::ContentHash,
    install_identity: Option<InstallIdentity>,
}

impl SourceFingerprint {
    /// Records a source's identity.
    #[must_use]
    pub fn new(
        proposal: &ArtifactProposal,
        bytes: &[u8],
        install_identity: Option<InstallIdentity>,
    ) -> Self {
        Self {
            spelling: proposal.spelling().as_str().to_owned(),
            size_bytes: bytes.len() as u64,
            sha256: sha256(bytes),
            install_identity,
        }
    }

    /// The source's relative spelling, exactly as inventoried.
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// The size of the bytes the plan was made from.
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// The SHA-256 of the bytes the plan was made from.
    pub fn sha256(&self) -> &cs_types::evidence::ContentHash {
        &self.sha256
    }

    /// The logical identity of the installation the source came from, when the
    /// caller inventoried one.
    pub fn install_identity(&self) -> Option<&InstallIdentity> {
        self.install_identity.as_ref()
    }
}

impl fmt::Display for SourceFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} bytes)", self.spelling, self.size_bytes)
    }
}

/// The new profile an import would land in.
///
/// An import never targets an existing profile: the id is a freshly allocated
/// [`ProfileId`] and the name is the caller's, so an import cannot overwrite a
/// profile the player already had (spec F64 non-negotiable 1: "Import to a new
/// profile"). F64-C's transaction owns the actual allocation and write; this
/// stage only names the destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetProfile {
    id: ProfileId,
    kind: ProfileKind,
    display_name: String,
}

impl TargetProfile {
    /// Names the destination.
    ///
    /// # Errors
    ///
    /// [`TargetProfileError`] when the display name is empty.
    pub fn new(
        id: ProfileId,
        kind: ProfileKind,
        display_name: impl Into<String>,
    ) -> Result<Self, TargetProfileError> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() {
            return Err(TargetProfileError::EmptyName);
        }
        Ok(Self {
            id,
            kind,
            display_name,
        })
    }

    /// The new profile's id.
    pub fn id(&self) -> ProfileId {
        self.id
    }

    /// Which population the new profile belongs to.
    pub fn kind(&self) -> ProfileKind {
        self.kind
    }

    /// The new profile's display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

/// Why a target profile was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetProfileError {
    /// The display name was empty or only whitespace.
    EmptyName,
}

impl fmt::Display for TargetProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "the imported profile needs a display name"),
        }
    }
}

impl std::error::Error for TargetProfileError {}

/// One declared mapping from a legacy raw id to a content identity.
///
/// This is the *only* way a legacy id becomes a [`ContentId`]. The pair is
/// declared, never derived from a position: there is no constructor taking an
/// index, and the resolver looks the target up by identity in the
/// [`Catalog`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyIdBinding {
    class: LegacyIdClass,
    raw: u32,
    target: ContentId,
}

impl LegacyIdBinding {
    /// Declares that the legacy id `raw` of `class` means `target`.
    ///
    /// # Errors
    ///
    /// [`LegacyIdMapError::KindMismatch`] when the target's kind is not the
    /// namespace the class belongs to. A binding is refused rather than
    /// coerced, because a wrong-namespace binding is exactly the silent
    /// misreading non-negotiable 3 forbids.
    pub fn new(
        class: LegacyIdClass,
        raw: u32,
        target: ContentId,
    ) -> Result<Self, LegacyIdMapError> {
        let expected = namespace_for(class);
        if target.kind() != expected {
            return Err(LegacyIdMapError::KindMismatch {
                class,
                expected,
                found: target.kind(),
            });
        }
        Ok(Self { class, raw, target })
    }

    /// The id class.
    pub fn class(&self) -> LegacyIdClass {
        self.class
    }

    /// The legacy raw id.
    pub fn raw(&self) -> u32 {
        self.raw
    }

    /// The content identity the legacy id means.
    pub fn target(&self) -> &ContentId {
        &self.target
    }
}

/// The content namespace an id class resolves into.
///
/// A fixed mapping, declared once, so a class can never be read as a different
/// kind of element. It mirrors `cs_types::content::ContentKind`'s namespaces,
/// not an original game's numbering. [`LegacyIdClass::Ordnance`] shares the
/// weapon namespace because the canonical catalog requires it: F44's ordnance
/// fitment is a `ContentKind::Weapon` id, so a rocket and a gun are separated
/// by their *declared binding*, not by their namespace.
const fn namespace_for(class: LegacyIdClass) -> ContentKind {
    match class {
        LegacyIdClass::Airframe => ContentKind::Airframe,
        LegacyIdClass::Weapon | LegacyIdClass::Ordnance => ContentKind::Weapon,
        LegacyIdClass::Engine => ContentKind::Engine,
        LegacyIdClass::Mission => ContentKind::Mission,
    }
}

/// Why a legacy id map or one of its bindings was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyIdMapError {
    /// A binding's target is not in the namespace its class belongs to.
    KindMismatch {
        /// The declared id class.
        class: LegacyIdClass,
        /// The namespace the class resolves into.
        expected: ContentKind,
        /// The namespace the target actually has.
        found: ContentKind,
    },
    /// Two bindings declare the same class and raw id with different targets.
    ConflictingBinding {
        /// The id class.
        class: LegacyIdClass,
        /// The legacy raw id.
        raw: u32,
    },
}

impl fmt::Display for LegacyIdMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindMismatch {
                class,
                expected,
                found,
            } => write!(
                f,
                "a {} id must resolve into the {expected} namespace, not {found}",
                class.label()
            ),
            Self::ConflictingBinding { class, raw } => {
                write!(f, "legacy {} id {raw} is bound twice", class.label())
            }
        }
    }
}

impl std::error::Error for LegacyIdMapError {}

/// The declared legacy-id to content-identity table.
///
/// Ordering is canonical by `(class, raw)` and lookup is by that key alone, so
/// the table's answer cannot depend on the order rows were declared in or on
/// the order the catalog enumerates (spec F64 non-negotiable 3; the
/// end-to-end AC03 assertion is F64-C's, this type is the contract that makes
/// it hold).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacyIdMap {
    bindings: Vec<LegacyIdBinding>,
}

impl LegacyIdMap {
    /// An empty table; every id resolves unresolved.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a binding.
    ///
    /// # Errors
    ///
    /// [`LegacyIdMapError::ConflictingBinding`] when the same class and raw id
    /// is already bound.
    pub fn insert(&mut self, binding: LegacyIdBinding) -> Result<(), LegacyIdMapError> {
        if self
            .bindings
            .iter()
            .any(|row| row.class == binding.class && row.raw == binding.raw)
        {
            return Err(LegacyIdMapError::ConflictingBinding {
                class: binding.class,
                raw: binding.raw,
            });
        }
        self.bindings.push(binding);
        self.bindings
            .sort_by_key(|row| (row.class, row.raw));
        Ok(())
    }

    /// The number of declared bindings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// The bindings, in canonical `(class, raw)` order.
    #[must_use]
    pub fn bindings(&self) -> &[LegacyIdBinding] {
        &self.bindings
    }

    /// The target identity declared for `class`/`raw`, without consulting the
    /// catalog.
    #[must_use]
    pub fn declared_target(&self, class: LegacyIdClass, raw: u32) -> Option<&ContentId> {
        self.bindings
            .iter()
            .find(|row| row.class == class && row.raw == raw)
            .map(LegacyIdBinding::target)
    }

    /// Resolves a legacy id through the catalog.
    ///
    /// # Errors
    ///
    /// [`UnresolvedReason`] when no binding declares the id
    /// ([`UnresolvedReason::IdNotMapped`]), when the bound identity is not in
    /// the catalog ([`UnresolvedReason::TargetNotInCatalog`]) or when it is
    /// there but not ready ([`UnresolvedReason::TargetNotReady`]).
    pub fn resolve(
        &self,
        class: LegacyIdClass,
        raw: u32,
        catalog: &Catalog,
    ) -> Result<ContentId, UnresolvedReason> {
        let target = self
            .declared_target(class, raw)
            .ok_or(UnresolvedReason::IdNotMapped { class, raw })?;
        let element = catalog
            .get(target)
            .ok_or_else(|| UnresolvedReason::TargetNotInCatalog { id: target.clone() })?;
        if !element.is_ready() {
            return Err(UnresolvedReason::TargetNotReady {
                id: target.clone(),
                reasons: element.unsupported_codes().join(", "),
            });
        }
        Ok(target.clone())
    }
}

/// Why one piece of a legacy document could not be imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnresolvedReason {
    /// Record bytes that no declared slot covers. The measurement did not
    /// account for them, so they are reported, never interpreted.
    UndeclaredRecordBytes {
        /// How many bytes the layout did not declare.
        bytes: u32,
    },
    /// No declared binding maps this legacy id.
    IdNotMapped {
        /// The id class the layout declared for the slot.
        class: LegacyIdClass,
        /// The raw id the document carried.
        raw: u32,
    },
    /// The record slot the layout declared as an id carries no integer.
    ///
    /// A layout that declares a text field as an id slot is a measurement
    /// error, and the record is unresolved rather than read as `0`.
    IdSlotNotInteger {
        /// The declared field name.
        field: String,
        /// The id class the layout declared for the slot.
        class: LegacyIdClass,
    },
    /// A bound identity is not in the catalog.
    TargetNotInCatalog {
        /// The identity the binding names.
        id: ContentId,
    },
    /// A bound identity is in the catalog but not usable yet.
    TargetNotReady {
        /// The identity the binding names.
        id: ContentId,
        /// The element's unsupported reason codes.
        reasons: String,
    },
    /// The document carried no records at all, so there is nothing to import.
    ///
    /// This is what keeps an empty legacy profile from being reported as a
    /// successful import (spec F64 non-negotiable 5).
    NoRecords,
    /// The document's version major is not the one the layout supports.
    UnsupportedVersion {
        /// The version the document declared.
        found: u32,
        /// The version this build imports.
        supported: u32,
    },
}

impl UnresolvedReason {
    /// Stable lowercase identifier for reports.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UndeclaredRecordBytes { .. } => "undeclared_record_bytes",
            Self::IdNotMapped { .. } => "id_not_mapped",
            Self::IdSlotNotInteger { .. } => "id_slot_not_integer",
            Self::TargetNotInCatalog { .. } => "target_not_in_catalog",
            Self::TargetNotReady { .. } => "target_not_ready",
            Self::NoRecords => "no_records",
            Self::UnsupportedVersion { .. } => "unsupported_version",
        }
    }
}

impl fmt::Display for UnresolvedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredRecordBytes { bytes } => {
                write!(f, "{} undeclared record bytes", bytes)
            }
            Self::IdNotMapped { class, raw } => {
                write!(f, "legacy {} id {raw} is not mapped", class.label())
            }
            Self::IdSlotNotInteger { field, class } => write!(
                f,
                "slot {field:?} is declared as a {} id but carries no integer",
                class.label()
            ),
            Self::TargetNotInCatalog { id } => {
                write!(f, "mapped identity {id} is not in the catalog")
            }
            Self::TargetNotReady { id, reasons } => {
                write!(f, "mapped identity {id} is not ready ({reasons})")
            }
            Self::NoRecords => write!(f, "the document declares no records"),
            Self::UnsupportedVersion { found, supported } => {
                write!(
                    f,
                    "version {found} is not the supported version {supported}"
                )
            }
        }
    }
}

/// One unresolved piece of a legacy document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedRow {
    /// The record index the row belongs to.
    pub record_index: Option<u32>,
    /// The field or slot the row is about, when it is about one.
    pub field: Option<String>,
    /// Why it is unresolved.
    pub reason: UnresolvedReason,
}

impl fmt::Display for UnresolvedRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.record_index {
            Some(index) => write!(f, "record {index}: {}", self.reason),
            None => write!(f, "{}", self.reason),
        }
    }
}

/// How much of a legacy document an import would carry.
///
/// Exactly three states, and the third is not a synonym for "nothing to do"
/// (spec F64 non-negotiable 5: "no silent reset to a blank profile called
/// imported").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportClass {
    /// Every record resolved and no byte was left over.
    Full,
    /// Some records resolved; every leftover is a named
    /// [`UnresolvedRow`].
    Partial {
        /// The resolved record indices, in order.
        resolved: Vec<u32>,
        /// The unresolved rows, in record order.
        unresolved: Vec<UnresolvedRow>,
    },
    /// Nothing was imported, and the reason is named.
    Unsupported {
        /// Why the document could not be imported.
        reason: UnresolvedReason,
    },
}

impl ImportClass {
    /// The report label.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial { .. } => "partial",
            Self::Unsupported { .. } => "unsupported",
        }
    }

    /// Whether the class reports an unresolved or unsupported state.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Full)
    }
}

impl fmt::Display for ImportClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("full"),
            Self::Partial {
                resolved,
                unresolved,
            } => {
                write!(
                    f,
                    "partial ({} resolved, {} unresolved)",
                    resolved.len(),
                    unresolved.len()
                )
            }
            Self::Unsupported { reason } => write!(f, "unsupported: {reason}"),
        }
    }
}

/// Why a plan was refused outright.
///
/// A refusal is *not* a partial import: no profile is created, no report is
/// written and the source is left exactly as it was found. Every variant is a
/// condition this stage can decide from the data it was handed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportRefusal {
    /// The candidate source was refused before any byte was read.
    Source(ArtifactProposalError),
    /// The source is larger than the import cap.
    SourceTooLarge {
        /// The size that was offered.
        size: u64,
        /// The cap that refused it.
        max: u64,
    },
    /// The layout declaration is not one this build may import.
    LayoutEvidence {
        /// The layout's label.
        layout: String,
        /// The evidence state that was refused.
        evidence: ClaimStatus,
    },
    /// The source's class is an optional enhancement this build is not
    /// importing.
    EnhancementDisabled {
        /// The class that was declined.
        class: LegacyArtifactClass,
        /// The switch that controls it.
        switch: &'static str,
    },
    /// The document could not be read. The reader's own structured refusal is
    /// carried whole, with its offset, field and expected/observed pair.
    Unreadable(LegacyProfileError),
    /// The document read cleanly but is not importable as such.
    NotImportable {
        /// The class it would have been.
        class: LegacyArtifactClass,
        /// Why not.
        reason: UnresolvedReason,
    },
}

impl ImportRefusal {
    /// Stable lowercase identifier for reports and logs.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Source(_) => "source_refused",
            Self::SourceTooLarge { .. } => "source_too_large",
            Self::LayoutEvidence { .. } => "layout_evidence",
            Self::EnhancementDisabled { .. } => "enhancement_disabled",
            Self::Unreadable(_) => "unreadable",
            Self::NotImportable { .. } => "not_importable",
        }
    }
}

impl fmt::Display for ImportRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(f, "the source was refused: {error}"),
            Self::SourceTooLarge { size, max } => {
                write!(f, "the source is {size} bytes, over the {max} byte cap")
            }
            Self::LayoutEvidence { layout, evidence } => write!(
                f,
                "layout {layout} is {evidence} evidence, which this build does not import"
            ),
            Self::EnhancementDisabled { class, switch } => {
                write!(f, "{class} is an optional enhancement and {switch} is off")
            }
            Self::Unreadable(error) => write!(f, "the source could not be read: {error}"),
            Self::NotImportable { class, reason } => {
                write!(f, "{class} is not importable: {reason}")
            }
        }
    }
}

impl std::error::Error for ImportRefusal {}

/// One imported record, as the plan describes it.
///
/// `resolved_ids` holds one `(class, identity)` pair per id slot the layout
/// declared **and** the id map resolved. A record is only listed here when
/// every declared slot resolved; a record with an unresolved slot appears in
/// the report's unresolved rows instead, never half-imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedRecord {
    /// The record's index in the legacy document.
    pub record_index: u32,
    /// The content identities the record's id slots resolved to, in layout
    /// declaration order.
    pub resolved_ids: Vec<(LegacyIdClass, ContentId)>,
}

/// The read-only report an import carries, retained with the new profile.
///
/// This is the "migration report" of spec F64 non-negotiable 1. It describes
/// what was read and what stayed unresolved; it is not a success claim, and it
/// carries the source fingerprint so an imported profile can always be traced
/// back to the exact bytes it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    source: SourceFingerprint,
    layout_id: String,
    layout_evidence: ClaimStatus,
    class: ImportClass,
    records: Vec<ImportedRecord>,
    version_major: u32,
    version_minor: u32,
}

impl MigrationReport {
    /// The source this report describes.
    pub fn source(&self) -> &SourceFingerprint {
        &self.source
    }

    /// The layout the document was read through.
    pub fn layout_id(&self) -> &str {
        &self.layout_id
    }

    /// That layout's evidence state.
    pub fn layout_evidence(&self) -> ClaimStatus {
        self.layout_evidence
    }

    /// How much of the document would be imported.
    pub fn class(&self) -> &ImportClass {
        &self.class
    }

    /// The records the plan would carry.
    pub fn records(&self) -> &[ImportedRecord] {
        &self.records
    }

    /// The document's version major.
    pub fn version_major(&self) -> u32 {
        self.version_major
    }

    /// The document's version minor.
    pub fn version_minor(&self) -> u32 {
        self.version_minor
    }
}

/// Everything [`plan_import`] needs, all by shared reference.
///
/// There is no field through which a caller could hand this planner a mutable
/// handle, a writer or a host path. That is the structural half of AC01; the
/// filesystem half is pinned by the acceptance test.
pub struct ImportRequest<'a> {
    /// The candidate source and the evidence for its class.
    pub source: &'a ArtifactProposal,
    /// The legacy bytes, borrowed for the duration of the call.
    pub bytes: &'a [u8],
    /// The declared layout to read through.
    pub layout: &'a LegacyLayout,
    /// The bounds every read must respect.
    pub limits: LegacyLimits,
    /// The declared legacy-id to content-identity table.
    pub ids: &'a LegacyIdMap,
    /// The catalog identities are resolved against.
    pub catalog: &'a Catalog,
    /// The new profile the import would land in.
    pub target: &'a TargetProfile,
    /// Which layout evidence may be imported.
    pub admission: LayoutAdmission,
    /// Whether the optional legacy-save enhancement is switched on.
    pub legacy_save_import_enabled: bool,
    /// The installation identity the source was found under, when known.
    pub install_identity: Option<InstallIdentity>,
}

/// A validated, read-only intent to import one legacy source.
///
/// A plan is a value. Holding one changes nothing on disk, and there is no
/// method that turns it into a write: F64-B's verified read-only subset and
/// F64-C's transaction own that, and they will consume this plan rather than
/// re-derive it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    class: LegacyArtifactClass,
    requirement: ImportRequirement,
    target: TargetProfile,
    report: MigrationReport,
    admitted_designed_layout: bool,
}

impl ImportPlan {
    /// The artifact class this plan imports.
    pub fn class(&self) -> LegacyArtifactClass {
        self.class
    }

    /// The import requirement the class carries, from the inventory.
    pub fn requirement(&self) -> ImportRequirement {
        self.requirement
    }

    /// The new profile the import would land in.
    pub fn target(&self) -> &TargetProfile {
        &self.target
    }

    /// The retained migration report.
    pub fn report(&self) -> &MigrationReport {
        &self.report
    }

    /// Whether the plan was made through
    /// [`LayoutAdmission::AllowDesignedFixtures`].
    ///
    /// A report must never present fixture data as a measured import, so this
    /// is part of the plan rather than of the caller's intent.
    #[must_use]
    pub fn admitted_designed_layout(&self) -> bool {
        self.admitted_designed_layout
    }
}

/// Plans an import of one legacy source into one new profile.
///
/// # Errors
///
/// [`ImportRefusal`] when the source is refused at the door (a spelling that
/// escapes the root, a size over the cap, a fingerprint that does not describe
/// the bytes), when its class is an optional enhancement that is switched off,
/// when the layout's evidence is not admitted, when the document cannot be
/// read within the limits, or when it reads but is not importable. On any
/// refusal nothing is planned, nothing is written and the source is untouched.
pub fn plan_import(request: &ImportRequest<'_>) -> Result<ImportPlan, ImportRefusal> {
    let ImportRequest {
        source,
        bytes,
        layout,
        limits,
        ids,
        catalog,
        target,
        admission,
        legacy_save_import_enabled,
        install_identity,
    } = request;

    let class = source.require_class().map_err(ImportRefusal::Source)?;
    let requirement = layout_record(class).requirement;
    if let ImportRequirement::OptionalEnhancement { disable_switch, .. } = requirement
        && !legacy_save_import_enabled
    {
        return Err(ImportRefusal::EnhancementDisabled {
            class,
            switch: disable_switch,
        });
    }

    if !admission.admits(layout.evidence()) {
        return Err(ImportRefusal::LayoutEvidence {
            layout: layout.id().to_owned(),
            evidence: layout.evidence(),
        });
    }

    if bytes.len() as u64 > MAX_LEGACY_SOURCE_BYTES || bytes.len() as u64 > limits.max_bytes {
        let max = limits.max_bytes.min(MAX_LEGACY_SOURCE_BYTES);
        return Err(ImportRefusal::SourceTooLarge {
            size: bytes.len() as u64,
            max,
        });
    }
    source
        .validate_against(bytes.len() as u64)
        .map_err(ImportRefusal::Source)?;

    let document = read_legacy_profile(bytes, layout, limits).map_err(ImportRefusal::Unreadable)?;

    if document.records().is_empty() {
        return Err(ImportRefusal::NotImportable {
            class,
            reason: UnresolvedReason::NoRecords,
        });
    }

    let mut records = Vec::new();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for (index, row) in document.records().iter().enumerate() {
        let record_index = u32::try_from(index).unwrap_or(u32::MAX);
        let mut identities = Vec::new();
        let mut row_unresolved = Vec::new();
        if !row.undeclared.is_empty() {
            row_unresolved.push(UnresolvedRow {
                record_index: Some(record_index),
                field: None,
                reason: UnresolvedReason::UndeclaredRecordBytes {
                    bytes: u32::try_from(row.undeclared.len()).unwrap_or(u32::MAX),
                },
            });
        }
        for id_ref in layout.id_refs() {
            let Some(raw) = row.integer(id_ref.field()) else {
                row_unresolved.push(UnresolvedRow {
                    record_index: Some(record_index),
                    field: Some(id_ref.field().to_owned()),
                    reason: UnresolvedReason::IdSlotNotInteger {
                        field: id_ref.field().to_owned(),
                        class: id_ref.class(),
                    },
                });
                continue;
            };
            let raw = u32::try_from(raw).unwrap_or(u32::MAX);
            match ids.resolve(id_ref.class(), raw, catalog) {
                Ok(target) => identities.push((id_ref.class(), target)),
                Err(reason) => row_unresolved.push(UnresolvedRow {
                    record_index: Some(record_index),
                    field: Some(id_ref.field().to_owned()),
                    reason,
                }),
            }
        }
        if row_unresolved.is_empty() {
            resolved.push(record_index);
            records.push(ImportedRecord {
                record_index,
                resolved_ids: identities,
            });
        }
        unresolved.extend(row_unresolved);
    }

    let class_outcome = if unresolved.is_empty() {
        ImportClass::Full
    } else if resolved.is_empty() {
        ImportClass::Unsupported {
            reason: unresolved[0].reason.clone(),
        }
    } else {
        ImportClass::Partial {
            resolved,
            unresolved,
        }
    };

    Ok(ImportPlan {
        class,
        requirement,
        target: (*target).clone(),
        admitted_designed_layout: layout.evidence() == ClaimStatus::Designed
            && *admission == LayoutAdmission::AllowDesignedFixtures,
        report: MigrationReport {
            source: SourceFingerprint::new(source, bytes, install_identity.clone()),
            layout_id: document.layout_id().to_owned(),
            layout_evidence: document.layout_evidence(),
            class: class_outcome,
            records,
            version_major: document.version_major(),
            version_minor: document.version_minor(),
        },
    })
}

/// The reader refusals that mean "this file is hostile or broken", not "this
/// file is a different version".
///
/// Exposed so a caller (F64-B's read-only subset, F64-C's report) can group
/// diagnostics without matching on the whole error type.
#[must_use]
pub fn refusal_is_read_failure(kind: LegacyProfileErrorKind) -> bool {
    matches!(
        kind,
        LegacyProfileErrorKind::DocumentTooLarge
            | LegacyProfileErrorKind::TooManyRecords
            | LegacyProfileErrorKind::RecordTableOutOfRange
            | LegacyProfileErrorKind::FieldTooWide
            | LegacyProfileErrorKind::Layout
    )
}
