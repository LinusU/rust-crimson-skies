//! The legacy-import surface inventory: which kinds of legacy artifact the
//! engine may be asked to import, whether importing them is required or an
//! optional enhancement, and how a candidate source is admitted (stage
//! F64-A).
//!
//! Spec `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`,
//! "Deliverable and interfaces": custom-aircraft import is required **when an
//! original content path references it**, while existing campaign-save import
//! is "a separately labeled compatibility enhancement" whose absence "does
//! not justify corrupting it or blocking a new-engine fresh campaign". This
//! module encodes that distinction as data ([`ImportRequirement`]) instead of
//! as prose, and every row carries its evidence state
//! ([`ClaimStatus::Unknown`]) plus the open questions that row would need
//! answered by measurement.
//!
//! Two rules are load-bearing and are enforced by tests rather than comments:
//!
//! 1. **A candidate source is classified by declared evidence, never by its
//!    name.** [`ArtifactProposal::class`] returns exactly what the caller
//!    declared and [`ArtifactProposalError::Unclassified`] is the answer for
//!    an undeclared source. No code path in this module inspects a file
//!    extension, so a file called `weapon.cfg` cannot become a weapon and an
//!    original member called `*.DAT` cannot be assumed to be a save (spec F12
//!    non-negotiable 1, applied to imports).
//! 2. **A source is bounded before it is read.** [`ArtifactProposal::new`]
//!    refuses a declared size above [`MAX_LEGACY_SOURCE_BYTES`] and
//!    [`validate_against`] re-checks it, so a hostile or simply broken
//!    profile is turned away at the door rather than after an allocation.
//!
//! F64-A wrote the rows without the `retail` capability, so every
//! `referenced_by` was empty and every `evidence` `Unknown`. F64-B's retail
//! measurement then found that the installation ships **no** legacy save or
//! custom-plane file (they are runtime-created), but it did measure original
//! content that references them: `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT`
//! fills a four-slot grid of saved planes, and the owner-supplied decrypted
//! engine image holds the `Planes\%s`, `SavedGames\%s\...` and registry path
//! templates. `CustomAircraft` is therefore `referenced_by` a measured path
//! and its requirement is on, while every row's byte layout stays `Unknown`
//! because no file of any class exists to read.

use std::fmt;

use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::{RelativePath, RelativePathError};

/// Entry point of the bounded legacy-profile reader, for the evidence ledger.
pub const LEGACY_PROFILE_ENTRYPOINT: &str = "cs_formats::legacy_profile::read_legacy_profile";

/// The largest legacy source this engine admits as a candidate for import.
///
/// **Designed, not measured.** A legacy profile, save or custom-aircraft
/// definition is a small record set, not a content archive: the original
/// files that would be imported are expected to be far below this, and the
/// bound exists so a hostile or corrupt file is refused before any allocation
/// rather than after one. What the original actually weighs is unmeasured
/// (F64-B); if a measured original file ever exceeds this, the cap is raised
/// with the measurement recorded, never worked around.
pub const MAX_LEGACY_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

/// One kind of legacy artifact the engine may be asked to import.
///
/// The classes are the *roles* this engine has to reason about, not claims
/// about files: each row says what importing the artifact is for and what is
/// still unknown about it. A class is a closed set because it is a vocabulary
/// the import layer switches on; the set of legacy *files* is open and is
/// discovered by measurement (F64-B), never by guessing from a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LegacyArtifactClass {
    /// A player-authored aircraft definition from the original game.
    CustomAircraft,
    /// A player-authored loadout/blueprint of such an aircraft.
    CustomLoadout,
    /// An existing campaign profile/save.
    CampaignSave,
    /// A stored settings blob.
    SettingsBlob,
    /// A stored pointer to the active profile.
    ProfilePointer,
}

impl LegacyArtifactClass {
    /// Every class, in inventory order.
    pub const ALL: [Self; 5] = [
        Self::CustomAircraft,
        Self::CustomLoadout,
        Self::CampaignSave,
        Self::SettingsBlob,
        Self::ProfilePointer,
    ];

    /// Stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::CustomAircraft => "legacy.custom_aircraft",
            Self::CustomLoadout => "legacy.custom_loadout",
            Self::CampaignSave => "legacy.campaign_save",
            Self::SettingsBlob => "legacy.settings_blob",
            Self::ProfilePointer => "legacy.profile_pointer",
        }
    }

    /// Parses a label back to its class.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "legacy.custom_aircraft" => Some(Self::CustomAircraft),
            "legacy.custom_loadout" => Some(Self::CustomLoadout),
            "legacy.campaign_save" => Some(Self::CampaignSave),
            "legacy.settings_blob" => Some(Self::SettingsBlob),
            "legacy.profile_pointer" => Some(Self::ProfilePointer),
            _ => None,
        }
    }
}

impl fmt::Display for LegacyArtifactClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Whether importing one artifact class is required or an optional
/// enhancement (spec F64, "Deliverable and interfaces").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportRequirement {
    /// Required as soon as a **verified original content path** references the
    /// artifact class. The recorded references are the measured evidence that
    /// switched the requirement on; while the list is empty the requirement is
    /// *not triggered*, and no stage turns it on by itself.
    RequiredWhenReferenced {
        /// Original content paths measured to reference the class. F64-B
        /// measured `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` to fill a
        /// four-slot saved-plane grid, so `CustomAircraft` carries it; an
        /// empty list means "not required yet", never "never required".
        referenced_by: &'static [&'static str],
    },
    /// A separately labeled compatibility enhancement. Not importing it is
    /// never a corruption of the source and never blocks a new-engine fresh
    /// campaign.
    OptionalEnhancement {
        /// The label a report, a save or a UI must carry so the enhancement is
        /// never presented as part of a supported core feature.
        label: &'static str,
        /// The named switch that turns this enhancement off. AC04's
        /// "migration can be disabled" is exercised at F64-D; the switch
        /// itself is this field, typed here so no later stage invents a
        /// different one.
        disable_switch: &'static str,
    },
}

impl ImportRequirement {
    /// Whether the recorded evidence has made this a hard requirement.
    ///
    /// Only [`Self::RequiredWhenReferenced`] with at least one measured
    /// referencing path is required; an optional enhancement is never
    /// required, no matter what references it.
    pub const fn is_required(self) -> bool {
        match self {
            Self::RequiredWhenReferenced { referenced_by } => !referenced_by.is_empty(),
            Self::OptionalEnhancement { .. } => false,
        }
    }

    /// Whether this build may import the class *only* when the owner has
    /// asked for the optional enhancement.
    pub const fn is_optional_enhancement(self) -> bool {
        matches!(self, Self::OptionalEnhancement { .. })
    }

    /// The measured original content paths that reference the class.
    pub const fn referenced_by(self) -> &'static [&'static str] {
        match self {
            Self::RequiredWhenReferenced { referenced_by } => referenced_by,
            Self::OptionalEnhancement { .. } => &[],
        }
    }
}

/// One row of the import-surface inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyLayoutRecord {
    /// The artifact class this row describes.
    pub class: LegacyArtifactClass,
    /// Whether importing it is required or an optional enhancement.
    pub requirement: ImportRequirement,
    /// The evidence state of this row's *layout*. Every row is `Unknown` at
    /// this stage: no legacy file was opened, so nothing about the byte
    /// layout, the id encoding or the version field is known. A row may only
    /// leave `Unknown` through measurement.
    pub evidence: ClaimStatus,
    /// What must be measured before this row can leave `Unknown`.
    pub unknowns: &'static [&'static str],
}

/// The import-surface inventory, one row per [`LegacyArtifactClass`].
///
/// This is an inventory of *what must be known* plus what a retail stage has
/// since measured. F64-B read the installation and found that no legacy file
/// of any class ships with it — the originals are runtime-created under
/// `Planes\` and `SavedGames\<profile>\`, both observed as path templates in
/// the owner-supplied decrypted engine image — so no byte layout has ever
/// been seen and every row's `evidence` stays `Unknown`. What the
/// measurement *did* establish is recorded per row: `CustomAircraft` is
/// referenced by the original construction screen's four-slot saved-plane
/// grid, which turns its requirement on; the save classes stay separately
/// labeled optional enhancements because an optional enhancement is never
/// required no matter what references it.
pub static LEGACY_LAYOUT_INVENTORY: [LegacyLayoutRecord; 5] = [
    LegacyLayoutRecord {
        class: LegacyArtifactClass::CustomAircraft,
        requirement: ImportRequirement::RequiredWhenReferenced {
            referenced_by: &["ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT"],
        },
        evidence: ClaimStatus::Unknown,
        unknowns: &[
            "the byte layout of a `Planes\\` custom-plane file — none ships \
             with the installation, so the format needs an original run or an \
             owner-captured file",
            "the version field and the id encoding inside a stored plane",
            "which parts of a stored plane are aircraft data and which are \
             player data",
        ],
    },
    LegacyLayoutRecord {
        class: LegacyArtifactClass::CustomLoadout,
        requirement: ImportRequirement::RequiredWhenReferenced { referenced_by: &[] },
        evidence: ClaimStatus::Unknown,
        unknowns: &[
            "whether the loadout is stored inside the `Planes\\` custom-plane \
             file or separately — the screen that edits it is the same one \
             that fills the four saved-plane slots, and no file ships to \
             distinguish the two",
            "how a stored component is identified (name, numeric id or index)",
            "what the original enforces about a stored loadout, if anything",
        ],
    },
    LegacyLayoutRecord {
        class: LegacyArtifactClass::CampaignSave,
        requirement: ImportRequirement::OptionalEnhancement {
            label: "legacy-save-import",
            disable_switch: "cs.profile.legacy_save_import",
        },
        evidence: ClaimStatus::Unknown,
        unknowns: &[
            "the byte layout of `SavedGames\\<profile>` files — the \
             `Status.dat`, `Mission.%1d%02d`, `Persist.%1d%02d`, `AutoSave.sav` \
             and `%s.sav` templates are observed in the engine image, but no \
             such file ships to read",
            "the save's version field and its checksum",
            "whether the original campaign's mission index, currency and \
             equipment ids are recoverable, and which are not",
        ],
    },
    LegacyLayoutRecord {
        class: LegacyArtifactClass::SettingsBlob,
        requirement: ImportRequirement::OptionalEnhancement {
            label: "legacy-save-import",
            disable_switch: "cs.profile.legacy_save_import",
        },
        evidence: ClaimStatus::Unknown,
        unknowns: &[
            "which registry values under `SOFTWARE\\Microsoft\\Microsoft \
             Games\\Crimson Skies\\1.0` carry settings and their formats — \
             the key name is observed in the engine image but the original \
             runs on Windows, so no hive ships to read",
            "which settings keys exist and which are live/restart",
        ],
    },
    LegacyLayoutRecord {
        class: LegacyArtifactClass::ProfilePointer,
        requirement: ImportRequirement::OptionalEnhancement {
            label: "legacy-save-import",
            disable_switch: "cs.profile.legacy_save_import",
        },
        evidence: ClaimStatus::Unknown,
        unknowns: &[
            "which store names the active profile — the per-profile \
             `SavedGames\\%s` directory and the registry `Savegame` string \
             are both observed in the engine image, and which is \
             authoritative is unmeasured",
            "whether a missing or damaged pointer is recoverable by the original",
        ],
    },
];

/// The inventory row for one class.
#[must_use]
pub fn layout_record(class: LegacyArtifactClass) -> &'static LegacyLayoutRecord {
    LEGACY_LAYOUT_INVENTORY
        .iter()
        .find(|record| record.class == class)
        .expect("every class has one inventory row")
}

/// Why a candidate source was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtifactProposalError {
    /// The relative spelling is not a valid in-installation path.
    Spelling(RelativePathError),
    /// The declared size is above [`MAX_LEGACY_SOURCE_BYTES`].
    SourceTooLarge {
        /// The declared size in bytes.
        size: u64,
        /// The cap that refused it.
        max: u64,
    },
    /// The fingerprint did not describe the bytes that were read.
    FingerprintMismatch {
        /// The size the fingerprint declares.
        declared: u64,
        /// The size of the bytes actually supplied.
        actual: u64,
    },
    /// The proposal's declared SHA-256 does not describe the supplied bytes.
    ///
    /// [`Self::FingerprintMismatch`] is the size half of the same fact, checked
    /// by [`ArtifactProposal::validate_against`]. This is the digest half, and
    /// it is raised by the import layer rather than here, because hashing lives
    /// in `cs_assets` and this crate is below it. A source whose bytes changed
    /// between being inventoried and being read is refused instead of being
    /// imported under the identity the inventory gave it.
    HashMismatch {
        /// The SHA-256 the proposal declared.
        declared: ContentHash,
        /// The SHA-256 of the bytes actually supplied.
        actual: ContentHash,
    },
    /// No class was declared for the source, so it cannot be routed.
    Unclassified,
}

impl fmt::Display for ArtifactProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spelling(error) => write!(f, "invalid relative spelling: {error}"),
            Self::SourceTooLarge { size, max } => {
                write!(f, "declared source size {size} exceeds the {max} byte cap")
            }
            Self::FingerprintMismatch { declared, actual } => write!(
                f,
                "the fingerprint declares {declared} bytes but {actual} bytes were supplied"
            ),
            Self::HashMismatch { declared, actual } => write!(
                f,
                "the fingerprint declares sha256 {} but the supplied bytes hash to {}",
                declared.to_hex(),
                actual.to_hex()
            ),
            Self::Unclassified => write!(
                f,
                "the source carries no declared artifact class and its name is never \
                 used to infer one"
            ),
        }
    }
}

impl std::error::Error for ArtifactProposalError {}

/// A candidate legacy source, with the evidence needed to import it.
///
/// The proposal is *data*: it names a source, the fingerprint that identifies
/// it and the class the caller measured. It owns no file handle, opens nothing
/// and writes nothing, which is what lets the import planner take it by shared
/// reference and still be incapable of touching the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactProposal {
    spelling: RelativePath,
    size_bytes: u64,
    sha256: ContentHash,
    declared_class: Option<LegacyArtifactClass>,
}

impl ArtifactProposal {
    /// Validates and wraps a candidate source.
    ///
    /// # Errors
    ///
    /// [`ArtifactProposalError::Spelling`] when the spelling is not a valid
    /// relative in-installation path (an absolute path, a `..` component or a
    /// NUL byte is refused, so no path can escape the source root), and
    /// [`ArtifactProposalError::SourceTooLarge`] when the declared size is
    /// above [`MAX_LEGACY_SOURCE_BYTES`].
    pub fn new(
        spelling: &str,
        size_bytes: u64,
        sha256: ContentHash,
        declared_class: Option<LegacyArtifactClass>,
    ) -> Result<Self, ArtifactProposalError> {
        let spelling = RelativePath::new(spelling).map_err(ArtifactProposalError::Spelling)?;
        if size_bytes > MAX_LEGACY_SOURCE_BYTES {
            return Err(ArtifactProposalError::SourceTooLarge {
                size: size_bytes,
                max: MAX_LEGACY_SOURCE_BYTES,
            });
        }
        Ok(Self {
            spelling,
            size_bytes,
            sha256,
            declared_class,
        })
    }

    /// The source's relative spelling, exactly as recorded.
    pub fn spelling(&self) -> &RelativePath {
        &self.spelling
    }

    /// The declared size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// The SHA-256 that identifies the source's bytes.
    pub fn sha256(&self) -> &ContentHash {
        &self.sha256
    }

    /// The declared class, or `None` when the caller declared none.
    ///
    /// This is the only source of the class. The spelling is never inspected,
    /// so no extension, no base name and no directory can turn a source into a
    /// class this engine did not measure.
    pub fn class(&self) -> Option<LegacyArtifactClass> {
        self.declared_class
    }

    /// The class, refusing a source that declared none.
    ///
    /// # Errors
    ///
    /// [`ArtifactProposalError::Unclassified`].
    pub fn require_class(&self) -> Result<LegacyArtifactClass, ArtifactProposalError> {
        self.declared_class
            .ok_or(ArtifactProposalError::Unclassified)
    }

    /// Re-checks the proposal against the bytes that were actually supplied.
    ///
    /// # Errors
    ///
    /// [`ArtifactProposalError::FingerprintMismatch`] when the supplied
    /// length differs from the declared one, and
    /// [`ArtifactProposalError::SourceTooLarge`] when the supplied bytes are
    /// above the cap even though the declared size was not.
    pub fn validate_against(&self, actual_len: u64) -> Result<(), ArtifactProposalError> {
        if actual_len > MAX_LEGACY_SOURCE_BYTES {
            return Err(ArtifactProposalError::SourceTooLarge {
                size: actual_len,
                max: MAX_LEGACY_SOURCE_BYTES,
            });
        }
        if actual_len != self.size_bytes {
            return Err(ArtifactProposalError::FingerprintMismatch {
                declared: self.size_bytes,
                actual: actual_len,
            });
        }
        Ok(())
    }
}
