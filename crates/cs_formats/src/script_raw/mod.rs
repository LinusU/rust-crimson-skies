//! Script-container inventory and its disassembly-neutral evidence schema
//! (`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
//! stage `### F13-A`; shared contract `docs/contracts/SCRIPT-MISSION.md`).
//!
//! F13 has to find the mission language in the installation before anyone
//! picks a VM. This stage defines the typed output of that search and fills
//! it from what the project can already read, without deciding anything the
//! corpus has not shown:
//!
//! * [`evidence`] is the schema a claim about a script record is recorded
//!   in: method, confidence, locator and a short note. It is neutral about
//!   the research method and cannot hold lifted code.
//! * [`inventory`] lists every candidate container, splits the ones a
//!   reader decodes into byte-ranged records, keeps every other byte as an
//!   opaque or unclaimed record and marks no record as instructions.
//!
//! The opcode ledger, reachability and the classification of the retail
//! corpus are later stages (F13-B, F13-C). The design decisions and the
//! recorded unknowns are in
//! `docs/findings/2026-09-29-f13-a-script-inventory-and-evidence-schema.md`.
//! Fixtures exercised by `crates/cs_formats/tests/script_raw/` are newly
//! authored synthetic bytes; nothing here is derived from original game data.

pub mod evidence;
pub mod inventory;

pub use evidence::{
    ByteSpan, Confidence, EvidenceError, EvidenceLocator, LocatorKind, MAX_NOTE_BYTES,
    ResearchMethod, ScriptEvidence,
};
pub use inventory::{
    Candidacy, DispatchOutcome, EXCLUDED_FAMILY_REASON, FormatDiscriminator,
    INTERP_BODY_UNESTABLISHED, InstructionClaimError, InstructionStatus, InventoryFinding,
    InventoryStats, MAX_LEADS_PER_RECORD, MIN_LEAD_BYTES, RecordKind, RecordReference,
    ReferenceKind, ScriptContainerEntry, ScriptInventory, ScriptRecord, ScriptRole, ScriptSource,
    UNDECODED_UNESTABLISHED, inventory_scripts,
};
