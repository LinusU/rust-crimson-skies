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
//! 1. **The source fingerprint is retained, and it is verified.** A source that
//!    changes length *or* digest between being inventoried and being read is
//!    refused, so a plan can never describe a file that was not the one the
//!    inventory fingerprinted (spec F64 non-negotiable 1: "retain source
//!    fingerprint"). [`SourceFingerprint`] then records that verified spelling,
//!    size and SHA-256 rather than a fresh digest of whatever arrived.
//! 2. **Ids resolve through content identities, never through positions.**
//!    [`LegacyIdMap`] is keyed by the legacy raw id and holds a
//!    [`ContentId`]; [`LegacyIdMap::resolve`] looks that id up in the
//!    [`Catalog`] and requires the element to be *ready*. There is no index
//!    arithmetic anywhere in the module, no value is ever clamped into the id
//!    range, and reordering catalog rows cannot change an outcome (spec F64
//!    non-negotiable 3).
//! 3. **Unknown stays unknown.** A record with bytes no declared slot covers, a
//!    record whose id is not mapped, a value wider than a legacy id, bytes past
//!    the record table and a target that is not ready all become named
//!    [`UnresolvedRow`]s. Nothing is defaulted, rounded, clamped or guessed, and
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
    LegacyLayout, LegacyLimits, LegacyProfileDocument, LegacyProfileError, LegacyProfileErrorKind,
    LegacyRecord, MAX_LEGACY_SOURCE_BYTES, layout_record, read_legacy_profile,
};
use cs_types::content::{
    ContentId, ContentIdError, ContentKind, Known, Origin, Provenance, Resolved,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::InstallIdentity;
use cs_types::profile::{ProfileId, ProfileKind};

use crate::catalog::Catalog;
use crate::construction::{
    AircraftBlueprint, BlueprintVerdict, ConstructionPolicy, ConstructionRules,
    ConstructionSchemaError, GunFitment, OrdnanceFitment, PaintSelection, PriceBook,
    ValidationRefusal,
};
use crate::damage::DamageNodeKey;

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
    ///
    /// The SHA-256 is the proposal's **declared** one, not a fresh digest of the
    /// bytes: the digest of the supplied bytes is compared against the declared
    /// one before this is called (see [`plan_import`]), so the retained value is
    /// the fingerprint the inventory measured *and* verified, and a report can
    /// trace an imported profile back to the exact inventoried file.
    #[must_use]
    pub fn new(proposal: &ArtifactProposal, install_identity: Option<InstallIdentity>) -> Self {
        Self {
            spelling: proposal.spelling().as_str().to_owned(),
            size_bytes: proposal.size_bytes(),
            sha256: *proposal.sha256(),
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
        self.bindings.sort_by_key(|row| (row.class, row.raw));
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
    /// The id slot carries a value the legacy id space cannot represent.
    ///
    /// A slot declared wider than a `u32` id can hold a value outside it. The
    /// record is unresolved: the value is **never** clamped into range, because
    /// a clamped value resolves to whichever element happens to be bound at the
    /// clamp boundary and a document would then be imported as a different
    /// aircraft or weapon than it names (spec F64 non-negotiable 3).
    IdOutOfRange {
        /// The declared field name.
        field: String,
        /// The id class the layout declared for the slot.
        class: LegacyIdClass,
        /// The value the document carried, which is wider than a legacy id.
        value: u64,
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
    /// Not one record resolved a content identity, so the plan carries nothing.
    ///
    /// The plan's whole payload is the identities its records resolved; a
    /// document that resolves none of them is a **blank profile**, whatever its
    /// record count says. Reporting that as a full import would be exactly the
    /// "silent reset to a blank profile called imported" non-negotiable 5
    /// forbids, so it is named here instead. In practice this is a measurement
    /// error — a layout that declares no id slot for a class whose records are
    /// made of content identities, or a document whose every id slot failed to
    /// resolve while nothing raised a row for it — which is why it is a named
    /// refusal rather than a silently empty success.
    NoResolvableIdentity {
        /// How many records the document declared.
        records: u32,
    },
    /// Bytes follow the record table that the layout declares no field for.
    ///
    /// The measurement did not account for them. They are reported, never
    /// interpreted and never dropped: a document whose tail the layout cannot
    /// explain is not a full import, whatever its records resolved to (spec F64
    /// non-negotiable 2).
    UndeclaredTrailingBytes {
        /// How many bytes follow the record table.
        bytes: u32,
    },
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
            Self::IdOutOfRange { .. } => "id_out_of_range",
            Self::TargetNotInCatalog { .. } => "target_not_in_catalog",
            Self::TargetNotReady { .. } => "target_not_ready",
            Self::NoRecords => "no_records",
            Self::NoResolvableIdentity { .. } => "no_resolvable_identity",
            Self::UndeclaredTrailingBytes { .. } => "undeclared_trailing_bytes",
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
            Self::IdOutOfRange {
                field,
                class,
                value,
            } => write!(
                f,
                "slot {field:?} carries legacy {} id {value}, which is wider than \
                 an id and is never clamped into range",
                class.label()
            ),
            Self::TargetNotInCatalog { id } => {
                write!(f, "mapped identity {id} is not in the catalog")
            }
            Self::TargetNotReady { id, reasons } => {
                write!(f, "mapped identity {id} is not ready ({reasons})")
            }
            Self::NoRecords => write!(f, "the document declares no records"),
            Self::NoResolvableIdentity { records } => write!(
                f,
                "no record of the {records} declared resolved a content identity, so \
                 the import would carry a blank profile"
            ),
            Self::UndeclaredTrailingBytes { bytes } => {
                write!(f, "{bytes} bytes follow the record table undeclared")
            }
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

    /// Whether the plan could only be made through
    /// [`LayoutAdmission::AllowDesignedFixtures`].
    ///
    /// The strict policy admits only a measured layout, so any layout that got
    /// in *because* the caller named the fixture admission — a designed one or
    /// an `observed_tool` one — must say so here. A report must never present
    /// fixture data as a measured import, and this flag is what makes that
    /// checkable rather than a matter of trusting the call site.
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
/// escapes the root, a size over the cap, a declared length or digest that does
/// not describe the bytes), when its class is an optional enhancement that is
/// switched off, when the layout's evidence is not admitted, when the document
/// cannot be read within the limits, or when it reads but is not importable. On
/// any refusal nothing is planned, nothing is written and the source is
/// untouched.
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
    // The size half of the fingerprint is checked above; this is the digest
    // half. A source whose bytes changed between being inventoried and being
    // read would otherwise be imported silently under the identity the
    // inventory gave it, which is the corruption non-negotiable 1 forbids.
    let actual_sha256 = sha256(bytes);
    if source.sha256() != &actual_sha256 {
        return Err(ImportRefusal::Source(ArtifactProposalError::HashMismatch {
            declared: *source.sha256(),
            actual: actual_sha256,
        }));
    }

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
            // A declared id wider than a legacy id is unresolved, never clamped:
            // clamping would resolve it against whatever happens to be bound at
            // the clamp boundary, which is the silent misreading non-negotiable
            // 3 forbids.
            let Ok(raw) = u32::try_from(raw) else {
                row_unresolved.push(UnresolvedRow {
                    record_index: Some(record_index),
                    field: Some(id_ref.field().to_owned()),
                    reason: UnresolvedReason::IdOutOfRange {
                        field: id_ref.field().to_owned(),
                        class: id_ref.class(),
                        value: raw,
                    },
                });
                continue;
            };
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

    // Bytes past the record table are exactly as unaccounted-for as bytes past
    // a record's last declared slot, so they are reported the same way instead
    // of being dropped: `ImportClass::Full` means "no byte was left over", and a
    // document whose tail the layout cannot explain has not left nothing over.
    if !document.trailing().is_empty() {
        unresolved.push(UnresolvedRow {
            record_index: None,
            field: None,
            reason: UnresolvedReason::UndeclaredTrailingBytes {
                bytes: u32::try_from(document.trailing().len()).unwrap_or(u32::MAX),
            },
        });
    }

    // A plan that carries no content identity at all is a blank profile,
    // however many records the document declared, and it must never be reported
    // as an import. This counts the identities the plan actually **carries**,
    // not the records it visited: a layout that declares its record fields and
    // no id slot resolves every record "successfully" with nothing to carry, so
    // the resolved-record count would call that a full import (spec F64
    // non-negotiable 5, "no silent reset to a blank profile called imported").
    let carried: usize = records.iter().map(|record| record.resolved_ids.len()).sum();
    let declared_records = u32::try_from(document.records().len()).unwrap_or(u32::MAX);
    let class_outcome = if unresolved.is_empty() && carried == 0 {
        ImportClass::Unsupported {
            reason: UnresolvedReason::NoResolvableIdentity {
                records: declared_records,
            },
        }
    } else if unresolved.is_empty() {
        ImportClass::Full
    } else if resolved.is_empty() || carried == 0 {
        // Nothing was carried, and the first unresolved row says why. This also
        // covers records that were each individually "resolved" yet carried no
        // identity: listing them as a partial import would report an import of
        // a blank profile.
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
        admitted_designed_layout: *admission == LayoutAdmission::AllowDesignedFixtures
            && !LayoutAdmission::MeasuredOnly.admits(layout.evidence()),
        report: MigrationReport {
            source: SourceFingerprint::new(source, install_identity.clone()),
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

// --------------------------------- verified blueprint import subset (F64-B) ----

/// The blueprint role one declared record field fills.
///
/// A legacy custom-aircraft record is a list of content identities plus, for
/// each fitted component, *where* it sits. Which record field fills which role
/// is a measurement, so it is declared as data in a [`BlueprintFieldMap`]
/// rather than guessed from a field name or a slot position.
#[derive(Clone, Debug, PartialEq)]
pub enum BlueprintRole {
    /// The airframe the plane is built on. The field must be a declared
    /// [`LegacyIdClass::Airframe`] id slot; the resolved identity is also the
    /// airframe a [`ConstructionRules`] profile must describe before any
    /// verdict exists.
    Airframe,
    /// The engine. The field must be a declared [`LegacyIdClass::Engine`] id
    /// slot.
    Engine,
    /// A gun fitted on `mount`, occupying `positions` gun positions. The field
    /// must be a declared [`LegacyIdClass::Weapon`] id slot. `positions` is
    /// [`Resolved`] because a measured file may not say how many positions its
    /// guns occupy; an unknown position count is an explicit unknown, never a
    /// guessed `1`.
    Gun {
        /// The mount the gun claims on the damage graph.
        mount: DamageNodeKey,
        /// How many of the airframe's gun positions the gun occupies.
        positions: Resolved<u32>,
    },
    /// An ordnance item fitted at `hardpoint`. The field must be a declared
    /// [`LegacyIdClass::Ordnance`] id slot.
    Ordnance {
        /// The hardpoint the item claims on the damage graph.
        hardpoint: DamageNodeKey,
    },
}

impl BlueprintRole {
    /// The id class a field carrying this role must be declared with.
    #[must_use]
    pub const fn id_class(&self) -> LegacyIdClass {
        match self {
            Self::Airframe => LegacyIdClass::Airframe,
            Self::Engine => LegacyIdClass::Engine,
            Self::Gun { .. } => LegacyIdClass::Weapon,
            Self::Ordnance { .. } => LegacyIdClass::Ordnance,
        }
    }

    /// The stable report label.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Airframe => "airframe",
            Self::Engine => "engine",
            Self::Gun { .. } => "gun",
            Self::Ordnance { .. } => "ordnance",
        }
    }
}

/// One declared record field and the blueprint role it fills.
#[derive(Clone, Debug, PartialEq)]
pub struct BlueprintFieldSlot {
    field: String,
    role: BlueprintRole,
}

impl BlueprintFieldSlot {
    /// Declares that `field` fills `role`.
    #[must_use]
    pub fn new(field: &str, role: BlueprintRole) -> Self {
        Self {
            field: field.to_owned(),
            role,
        }
    }

    /// The record field name, exactly as the layout declares it.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The role the field fills.
    #[must_use]
    pub const fn role(&self) -> &BlueprintRole {
        &self.role
    }
}

/// Why a [`BlueprintFieldMap`] could not be built or could not describe a
/// layout's records.
#[derive(Clone, Debug, PartialEq)]
pub enum BlueprintMapError {
    /// The same record field is assigned to two roles.
    DuplicateField {
        /// The field assigned twice.
        field: String,
    },
    /// A role that may appear once appears twice.
    DuplicateRole {
        /// The role repeated.
        role: &'static str,
    },
    /// A blueprint needs this role and the map declares none.
    MissingRole {
        /// The role no field fills.
        role: &'static str,
    },
    /// The map names a field the layout does not declare as a record slot.
    FieldNotDeclared {
        /// The field the layout does not declare.
        field: String,
    },
    /// The map names a record slot the layout does not declare as an id slot.
    FieldNotAnIdSlot {
        /// The field that carries no declared id class.
        field: String,
    },
    /// The role resolves a different id class than the layout declares for the
    /// field. Assigning a gun role to the airframe field is a declaration
    /// error, not an unresolved import, and it is refused before any record is
    /// read.
    RoleClassMismatch {
        /// The field whose declarations disagree.
        field: String,
        /// The class the role resolves through.
        role_class: LegacyIdClass,
        /// The class the layout declared for the field.
        declared_class: LegacyIdClass,
    },
}

impl fmt::Display for BlueprintMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateField { field } => {
                write!(f, "record field {field:?} is assigned to two roles")
            }
            Self::DuplicateRole { role } => {
                write!(f, "the {role} role is assigned more than once")
            }
            Self::MissingRole { role } => {
                write!(f, "no record field fills the {role} role")
            }
            Self::FieldNotDeclared { field } => write!(
                f,
                "record field {field:?} is not a record slot of the layout"
            ),
            Self::FieldNotAnIdSlot { field } => {
                write!(f, "record field {field:?} is not a declared id slot")
            }
            Self::RoleClassMismatch {
                field,
                role_class,
                declared_class,
            } => write!(
                f,
                "record field {field:?} fills a role that resolves {} ids, but the \
                 layout declares it a {} id slot",
                role_class.label(),
                declared_class.label()
            ),
        }
    }
}

impl std::error::Error for BlueprintMapError {}

/// The declared field-to-blueprint map for one layout.
///
/// Like [`LegacyLayout`], this is **data**: a measured file format is described
/// by a map with [`ClaimStatus`] evidence, and the acceptance tests use a
/// designed one. The strict admission policy refuses a designed or unknown
/// map, so a guessed field assignment can never reach a record. The map says
/// which record fields build the blueprint; record fields it does not name
/// (a name, a paint, a checksum) are not blueprint roles at this stage and
/// stay out of the verdict entirely rather than being interpreted.
#[derive(Clone, Debug, PartialEq)]
pub struct BlueprintFieldMap {
    id: String,
    evidence: ClaimStatus,
    slots: Vec<BlueprintFieldSlot>,
}

impl BlueprintFieldMap {
    /// Declares a field map, refusing one that cannot describe a blueprint.
    ///
    /// A blueprint holds exactly one airframe and one engine, so the map must
    /// declare exactly one of each; gun and ordnance slots are open-ended.
    ///
    /// # Errors
    ///
    /// [`BlueprintMapError::DuplicateField`] when a field is assigned twice,
    /// [`BlueprintMapError::DuplicateRole`] when the airframe or engine role
    /// is assigned more than once, and [`BlueprintMapError::MissingRole`]
    /// when either is absent.
    pub fn new(
        id: &str,
        evidence: ClaimStatus,
        slots: Vec<BlueprintFieldSlot>,
    ) -> Result<Self, BlueprintMapError> {
        for (index, slot) in slots.iter().enumerate() {
            if slots[..index]
                .iter()
                .any(|earlier| earlier.field == slot.field)
            {
                return Err(BlueprintMapError::DuplicateField {
                    field: slot.field.clone(),
                });
            }
            if matches!(slot.role, BlueprintRole::Airframe | BlueprintRole::Engine) {
                let role = slot.role.label();
                if slots[..index]
                    .iter()
                    .any(|earlier| earlier.role.label() == role)
                {
                    return Err(BlueprintMapError::DuplicateRole { role });
                }
            }
        }
        for role in ["airframe", "engine"] {
            if !slots.iter().any(|slot| slot.role.label() == role) {
                return Err(BlueprintMapError::MissingRole { role });
            }
        }
        Ok(Self {
            id: id.to_owned(),
            evidence,
            slots,
        })
    }

    /// The map's label.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// How strong the evidence for this map is.
    #[must_use]
    pub const fn evidence(&self) -> ClaimStatus {
        self.evidence
    }

    /// The declared field-to-role assignments.
    #[must_use]
    pub fn slots(&self) -> &[BlueprintFieldSlot] {
        &self.slots
    }

    /// Checks the map against the layout it describes.
    ///
    /// # Errors
    ///
    /// [`BlueprintMapError::FieldNotDeclared`] when a field is not a record
    /// slot, [`BlueprintMapError::FieldNotAnIdSlot`] when it is not a declared
    /// id slot, and [`BlueprintMapError::RoleClassMismatch`] when the role and
    /// the declared class disagree.
    pub fn validate_against(&self, layout: &LegacyLayout) -> Result<(), BlueprintMapError> {
        for slot in &self.slots {
            if !layout
                .record_slots()
                .iter()
                .any(|declared| declared.name() == slot.field)
            {
                return Err(BlueprintMapError::FieldNotDeclared {
                    field: slot.field.clone(),
                });
            }
            let declared = layout
                .id_refs()
                .iter()
                .find(|id_ref| id_ref.field() == slot.field);
            let Some(id_ref) = declared else {
                return Err(BlueprintMapError::FieldNotAnIdSlot {
                    field: slot.field.clone(),
                });
            };
            let role_class = slot.role.id_class();
            if id_ref.class() != role_class {
                return Err(BlueprintMapError::RoleClassMismatch {
                    field: slot.field.clone(),
                    role_class,
                    declared_class: id_ref.class(),
                });
            }
        }
        Ok(())
    }
}

/// Why one legacy record could not be judged against the stock rules.
#[derive(Clone, Debug, PartialEq)]
pub enum BlueprintRecordRefusal {
    /// The record's component ids could not be resolved — the same named
    /// [`UnresolvedRow`]s the plan reports, so an unimportable record is
    /// described the same way in both reports.
    Unresolved(Vec<UnresolvedRow>),
    /// [`AircraftBlueprint::try_new`] refused the assembled fitments (a
    /// wrong-namespace id, a repeated mount, a zero-position gun).
    Schema(ConstructionSchemaError),
    /// The record's blueprint identity could not be formed from the layout
    /// label and the record index.
    Identity(ContentIdError),
    /// The stock validator could not measure or judge the blueprint: a limit
    /// is unmeasured, a component is unpriced, the pairing rule is unknown or
    /// the rules describe a different airframe. An unjudged blueprint is
    /// refused, never passed as conforming.
    Validation(ValidationRefusal),
}

impl fmt::Display for BlueprintRecordRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved(rows) => {
                write!(f, "{} unresolved row(s): ", rows.len())?;
                for (index, row) in rows.iter().enumerate() {
                    if index > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{row}")?;
                }
                Ok(())
            }
            Self::Schema(error) => write!(f, "the blueprint does not assemble: {error}"),
            Self::Identity(error) => {
                write!(f, "the record's blueprint id could not be formed: {error}")
            }
            Self::Validation(error) => {
                write!(f, "the stock rules could not judge the blueprint: {error}")
            }
        }
    }
}

impl std::error::Error for BlueprintRecordRefusal {}

/// What judging one legacy record's blueprint against the stock rules found.
#[derive(Clone, Debug, PartialEq)]
pub enum BlueprintRecordOutcome {
    /// The record assembled into a blueprint that is inside every measured
    /// limit and constraint. The verdict is retained whole, totals included.
    Conforming {
        /// The blueprint the record became.
        blueprint: AircraftBlueprint,
        /// Its verdict under the stock rules.
        verdict: BlueprintVerdict,
    },
    /// The record assembled into a blueprint that the stock rules reject.
    ///
    /// The verdict carries the specific fields of every breach
    /// ([`crate::construction::LimitBreach`]'s `limit`/`total`/`used` pairs)
    /// and every [`crate::construction::ConstraintViolation`], so a rejection
    /// is machine-matchable rather than a prose message.
    Rejected {
        /// The blueprint the record became.
        blueprint: AircraftBlueprint,
        /// Its verdict under the stock rules, with the broken constraints.
        verdict: BlueprintVerdict,
    },
    /// No verdict could be produced; the reason is named.
    Refused {
        /// Why the record could not be judged.
        reason: BlueprintRecordRefusal,
    },
}

/// One record's blueprint verdict, keyed by its document index.
#[derive(Clone, Debug, PartialEq)]
pub struct BlueprintRecordReport {
    record_index: u32,
    outcome: BlueprintRecordOutcome,
}

impl BlueprintRecordReport {
    /// The record's index in the legacy document.
    #[must_use]
    pub const fn record_index(&self) -> u32 {
        self.record_index
    }

    /// What judging the record found.
    #[must_use]
    pub const fn outcome(&self) -> &BlueprintRecordOutcome {
        &self.outcome
    }
}

/// Why a blueprint assessment of a whole document was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum BlueprintImportRefusal {
    /// The field map's evidence is not admitted under the requested policy.
    MapEvidence {
        /// The map's label.
        map: String,
        /// The evidence state that was refused.
        evidence: ClaimStatus,
    },
    /// The field map could not describe this layout's records.
    Map(BlueprintMapError),
}

impl fmt::Display for BlueprintImportRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MapEvidence { map, evidence } => write!(
                f,
                "field map {map} is {evidence} evidence, which this admission does not allow"
            ),
            Self::Map(error) => write!(f, "the field map cannot describe the layout: {error}"),
        }
    }
}

impl std::error::Error for BlueprintImportRefusal {}

/// Everything [`assess_imported_blueprints`] needs, all by shared reference.
///
/// The same read-only shape as [`ImportRequest`]: no mutable handle, no
/// writer, no host path. The request judges records a caller has already
/// read; it never reads a file itself.
pub struct BlueprintImportRequest<'a> {
    /// The legacy document, already read through its declared layout.
    pub document: &'a LegacyProfileDocument,
    /// The layout the document was read through.
    pub layout: &'a LegacyLayout,
    /// The declared field-to-blueprint map for the layout.
    pub field_map: &'a BlueprintFieldMap,
    /// The declared legacy-id to content-identity table.
    pub ids: &'a LegacyIdMap,
    /// The catalog identities are resolved against.
    pub catalog: &'a Catalog,
    /// The stock construction rules the blueprints are judged by.
    pub rules: &'a ConstructionRules,
    /// The host policy the blueprints are judged by.
    pub policy: &'a ConstructionPolicy,
    /// The component prices the budgets are measured against.
    pub book: &'a PriceBook,
    /// The origin every assembled blueprint is stamped with.
    pub origin: Origin,
    /// The provenance every assembled blueprint carries.
    pub provenance: Provenance,
    /// Which field-map evidence may be assessed.
    pub admission: LayoutAdmission,
}

/// The blueprint verdicts of one legacy document, record by record.
///
/// Retained like [`ImportPlan::report`]: it describes what was judged and how
/// each record fared, and the `admitted_designed_map` flag marks a report
/// that could only be produced through the fixture admission so fixture data
/// can never present itself as a measured import.
#[derive(Clone, Debug, PartialEq)]
pub struct BlueprintImportReport {
    field_map_id: String,
    field_map_evidence: ClaimStatus,
    admitted_designed_map: bool,
    records: Vec<BlueprintRecordReport>,
}

impl BlueprintImportReport {
    /// The field map the assessment ran through.
    pub fn field_map_id(&self) -> &str {
        &self.field_map_id
    }

    /// That map's evidence state.
    pub const fn field_map_evidence(&self) -> ClaimStatus {
        self.field_map_evidence
    }

    /// Whether the report could only be made through
    /// [`LayoutAdmission::AllowDesignedFixtures`].
    #[must_use]
    pub const fn admitted_designed_map(&self) -> bool {
        self.admitted_designed_map
    }

    /// Every record's verdict, in document order.
    #[must_use]
    pub fn records(&self) -> &[BlueprintRecordReport] {
        &self.records
    }

    /// How many records assembled into a stock-legal blueprint.
    #[must_use]
    pub fn conforming_count(&self) -> usize {
        self.records
            .iter()
            .filter(|row| matches!(row.outcome, BlueprintRecordOutcome::Conforming { .. }))
            .count()
    }

    /// How many records assembled into a blueprint the stock rules reject.
    #[must_use]
    pub fn rejected_count(&self) -> usize {
        self.records
            .iter()
            .filter(|row| matches!(row.outcome, BlueprintRecordOutcome::Rejected { .. }))
            .count()
    }

    /// How many records could not be judged at all.
    #[must_use]
    pub fn refused_count(&self) -> usize {
        self.records
            .iter()
            .filter(|row| matches!(row.outcome, BlueprintRecordOutcome::Refused { .. }))
            .count()
    }
}

/// Judges every record of a legacy document against the stock construction
/// rules.
///
/// This is the F64-B subset: the resolved identities of [`plan_import`]'s
/// records are extended into real [`AircraftBlueprint`]s through the declared
/// [`BlueprintFieldMap`], and each is measured by the production
/// [`ConstructionRules::validate`], so a violating blueprint is rejected with
/// the same specific fields any other blueprint carries.
///
/// # Errors
///
/// [`BlueprintImportRefusal::MapEvidence`] when the field map's evidence is
/// not admitted, and [`BlueprintImportRefusal::Map`] when the map cannot
/// describe this layout's records. Per-record failures are *not* errors of
/// this function: they are [`BlueprintRecordOutcome`]s in the report.
pub fn assess_imported_blueprints(
    request: &BlueprintImportRequest<'_>,
) -> Result<BlueprintImportReport, BlueprintImportRefusal> {
    if !request.admission.admits(request.field_map.evidence()) {
        return Err(BlueprintImportRefusal::MapEvidence {
            map: request.field_map.id().to_owned(),
            evidence: request.field_map.evidence(),
        });
    }
    request
        .field_map
        .validate_against(request.layout)
        .map_err(BlueprintImportRefusal::Map)?;

    let mut records = Vec::with_capacity(request.document.records().len());
    for (index, record) in request.document.records().iter().enumerate() {
        let record_index = u32::try_from(index).unwrap_or(u32::MAX);
        records.push(BlueprintRecordReport {
            record_index,
            outcome: assess_record_blueprint(record, record_index, request),
        });
    }

    Ok(BlueprintImportReport {
        field_map_id: request.field_map.id().to_owned(),
        field_map_evidence: request.field_map.evidence(),
        admitted_designed_map: request.admission == LayoutAdmission::AllowDesignedFixtures
            && !LayoutAdmission::MeasuredOnly.admits(request.field_map.evidence()),
        records,
    })
}

/// Judges one record: assemble its blueprint, then validate it.
fn assess_record_blueprint(
    record: &LegacyRecord,
    record_index: u32,
    request: &BlueprintImportRequest<'_>,
) -> BlueprintRecordOutcome {
    let blueprint = match blueprint_from_record(record, record_index, request) {
        Ok(blueprint) => blueprint,
        Err(reason) => return BlueprintRecordOutcome::Refused { reason },
    };
    match request
        .rules
        .validate(request.policy, &blueprint, request.book)
    {
        Err(reason) => BlueprintRecordOutcome::Refused {
            reason: BlueprintRecordRefusal::Validation(reason),
        },
        Ok(verdict) if verdict.is_valid() => {
            BlueprintRecordOutcome::Conforming { blueprint, verdict }
        }
        Ok(verdict) => BlueprintRecordOutcome::Rejected { blueprint, verdict },
    }
}

/// Assembles one record into an [`AircraftBlueprint`].
///
/// Every field the map names resolves through the same [`LegacyIdMap`] and
/// [`Catalog`] the plan uses, so a record that cannot be judged is described
/// by the same unresolved rows the plan reports. A record's blueprint id is
/// derived from the layout label and the record index — the only identities
/// in scope at this stage — never guessed from a text field whose semantics
/// are unmeasured.
fn blueprint_from_record(
    record: &LegacyRecord,
    record_index: u32,
    request: &BlueprintImportRequest<'_>,
) -> Result<AircraftBlueprint, BlueprintRecordRefusal> {
    let mut unresolved = Vec::new();
    let mut airframe = None;
    let mut engine = None;
    let mut guns = Vec::new();
    let mut ordnance = Vec::new();

    for slot in request.field_map.slots() {
        let class = slot.role().id_class();
        let Some(raw) = record.integer(slot.field()) else {
            unresolved.push(UnresolvedRow {
                record_index: Some(record_index),
                field: Some(slot.field().to_owned()),
                reason: UnresolvedReason::IdSlotNotInteger {
                    field: slot.field().to_owned(),
                    class,
                },
            });
            continue;
        };
        // The same range rule as the planner: a value wider than a legacy id
        // is unresolved, never clamped into range.
        let Ok(raw) = u32::try_from(raw) else {
            unresolved.push(UnresolvedRow {
                record_index: Some(record_index),
                field: Some(slot.field().to_owned()),
                reason: UnresolvedReason::IdOutOfRange {
                    field: slot.field().to_owned(),
                    class,
                    value: raw,
                },
            });
            continue;
        };
        let id = match request.ids.resolve(class, raw, request.catalog) {
            Ok(id) => id,
            Err(reason) => {
                unresolved.push(UnresolvedRow {
                    record_index: Some(record_index),
                    field: Some(slot.field().to_owned()),
                    reason,
                });
                continue;
            }
        };
        match slot.role() {
            BlueprintRole::Airframe => airframe = Some(id),
            BlueprintRole::Engine => engine = Some(id),
            BlueprintRole::Gun { mount, positions } => guns.push(
                GunFitment::try_new(id, mount.clone(), positions.clone())
                    .map_err(BlueprintRecordRefusal::Schema)?,
            ),
            BlueprintRole::Ordnance { hardpoint } => ordnance.push(
                OrdnanceFitment::try_new(hardpoint.clone(), id)
                    .map_err(BlueprintRecordRefusal::Schema)?,
            ),
        }
    }
    if !unresolved.is_empty() {
        return Err(BlueprintRecordRefusal::Unresolved(unresolved));
    }
    // `BlueprintFieldMap::new` requires exactly one airframe and one engine
    // slot, and a slot that resolved always assigned above — a `None` here
    // would mean the map validation and this loop disagree, not a data
    // condition.
    let airframe = airframe.expect("the field map was validated to carry an airframe slot");
    let engine = engine.expect("the field map was validated to carry an engine slot");

    let mut key = String::with_capacity(request.layout.id().len() + 14);
    key.push_str("legacy.");
    for ch in request.layout.id().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            key.push(ch.to_ascii_lowercase());
        } else {
            key.push('.');
        }
    }
    key.push('.');
    key.push_str(&record_index.to_string());
    let id = ContentId::from_source(ContentKind::Blueprint, &key)
        .map_err(BlueprintRecordRefusal::Identity)?;

    // Armor, equipment and paint are not blueprint roles at this stage: how a
    // legacy file encodes them is unmeasured, so they are empty rather than
    // guessed. The stock rules measure what is declared, and a missing armor
    // or paint is a measured state, not a fabricated one.
    AircraftBlueprint::try_new(
        id,
        airframe,
        engine,
        vec![],
        guns,
        ordnance,
        vec![],
        PaintSelection::default(),
        request.origin.clone(),
        request.provenance.clone(),
    )
    .map_err(BlueprintRecordRefusal::Schema)
}

/// The designed fixture map for [`cs_formats::legacy_profile::synthetic_blueprint_layout`].
///
/// **Not a claim about any original file.** It assigns the fixture layout's
/// declared id slots to blueprint roles so the import subset can be exercised
/// end to end: the airframe and engine fields as themselves, `gun_1`..`gun_4`
/// as single-position guns on four distinct mounts, and `rocket_1`/`rocket_2`
/// on two hardpoints — the original construction screen's measured four gun
/// positions and two hardpoint points. Its evidence is
/// [`ClaimStatus::Designed`], which the strict admission refuses.
#[must_use]
pub fn synthetic_blueprint_map() -> BlueprintFieldMap {
    fn node(key: &str) -> DamageNodeKey {
        DamageNodeKey::new(key).expect("the synthetic mount key is valid")
    }
    fn positions(count: u32) -> Resolved<u32> {
        Resolved::Known(Known::new(
            count,
            Provenance::designed(
                cs_types::evidence::ClaimId::new("f64b.fixture.positions")
                    .expect("the claim id is valid"),
            ),
        ))
    }
    BlueprintFieldMap::new(
        "synthetic.fixture_blueprint_map/v1",
        ClaimStatus::Designed,
        vec![
            BlueprintFieldSlot::new("airframe_id", BlueprintRole::Airframe),
            BlueprintFieldSlot::new("engine_id", BlueprintRole::Engine),
            BlueprintFieldSlot::new(
                "gun_1",
                BlueprintRole::Gun {
                    mount: node("mount_1"),
                    positions: positions(1),
                },
            ),
            BlueprintFieldSlot::new(
                "gun_2",
                BlueprintRole::Gun {
                    mount: node("mount_2"),
                    positions: positions(1),
                },
            ),
            BlueprintFieldSlot::new(
                "gun_3",
                BlueprintRole::Gun {
                    mount: node("mount_3"),
                    positions: positions(1),
                },
            ),
            BlueprintFieldSlot::new(
                "gun_4",
                BlueprintRole::Gun {
                    mount: node("mount_4"),
                    positions: positions(1),
                },
            ),
            BlueprintFieldSlot::new(
                "rocket_1",
                BlueprintRole::Ordnance {
                    hardpoint: node("hardpoint_1"),
                },
            ),
            BlueprintFieldSlot::new(
                "rocket_2",
                BlueprintRole::Ordnance {
                    hardpoint: node("hardpoint_2"),
                },
            ),
        ],
    )
    .expect("the synthetic blueprint map is valid")
}
