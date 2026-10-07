//! The legacy-import surface inventory and the declared-layout reader its
//! measurements will plug into (stage F64-A).
//!
//! Spec `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`,
//! section `### F64-A`. Two files, two responsibilities:
//!
//! * [`inventory`] declares *what* may be imported and on whose authority: one
//!   [`LegacyArtifactClass`] per row, whether the row is a hard requirement or
//!   a separately labeled optional enhancement, the evidence state of its
//!   layout, and the questions that must be measured before it can leave
//!   `Unknown`. A candidate source is admitted as an
//!   [`ArtifactProposal`](inventory::ArtifactProposal) that carries the
//!   caller's declared class — the class is never inferred from a file name —
//!   and a source over [`MAX_LEGACY_SOURCE_BYTES`] is refused at the door.
//! * [`document`] reads a legacy document **through a declared layout**:
//!   [`LegacyLayout`](document::LegacyLayout) is data, every extent is declared
//!   rather than read from the input, the record count is checked against
//!   [`LegacyLimits`](document::LegacyLimits) and an F03
//!   [`AllocationBudget`](crate::io::AllocationBudget) before a `Vec` is
//!   reserved, and every byte the layout does not name is retained instead of
//!   dropped or guessed.
//!
//! **Nothing here is original-verified.** F64-B's retail measurement found
//! that the installation ships no legacy save or custom-plane file at all —
//! the originals are runtime-created under `Planes\` and
//! `SavedGames\<profile>\`, observed as path templates in the owner-supplied
//! decrypted engine image — so no legacy byte layout has ever been read and
//! every inventory row's `evidence` stays
//! [`ClaimStatus::Unknown`](cs_types::evidence::ClaimStatus::Unknown). What the
//! measurement did establish is recorded in [`LEGACY_LAYOUT_INVENTORY`]: the
//! construction screen's four-slot saved-plane grid references custom
//! aircraft, and the shipped layouts ([`synthetic_layout`],
//! [`synthetic_blueprint_layout`]) remain
//! [`ClaimStatus::Designed`](cs_types::evidence::ClaimStatus::Designed)
//! fixtures, never claims about an original file.
//!
//! F64-A's own minimum scenario is sheet **AC01**: a malicious or oversized
//! old profile fails without touching the source or any new save. The reader
//! above is where that refusal is produced, and
//! `crates/cs_content::legacy_import` is where it is proven to have written
//! nothing.

pub mod document;
pub mod inventory;

pub use document::{
    LEGACY_MAGIC_BYTES, LegacyIdClass, LegacyIdSlot, LegacyLayout, LegacyLayoutError, LegacyLimits,
    LegacyProfileDocument, LegacyProfileError, LegacyProfileErrorKind, LegacyRecord, LegacySlot,
    LegacySlotType, LegacyValue, TrailingPolicy, check_slot_widths, read_legacy_profile,
    synthetic_blueprint_layout, synthetic_layout,
};
pub use inventory::{
    ArtifactProposal, ArtifactProposalError, ImportRequirement, LEGACY_LAYOUT_INVENTORY,
    LEGACY_PROFILE_ENTRYPOINT, LegacyArtifactClass, LegacyLayoutRecord, MAX_LEGACY_SOURCE_BYTES,
    layout_record,
};
