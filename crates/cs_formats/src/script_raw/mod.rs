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
//! * [`discovery`] locates and classifies the programs of one container: the
//!   INTERP loading scripts, the reader-archive members (by name and mission
//!   scope) and the animation containers, each with its byte range and the
//!   confidence of its role.
//! * [`ledger`] is the checked table of reachable opcodes and the fail-closed
//!   program walk: an opcode the ledger does not name stops the walk with the
//!   mission and the program's source location.
//! * [`ui_host_calls`] is the F38-B measured host-call corpus of the shipped UI
//!   script programs: the two native dispatch forms their text contains, the
//!   shape of every argument expression and the byte span of every site — a
//!   measurement, never a meaning. F38-D.1 adds the measured five-native
//!   dispatch-target table (`$$A$$` goscallback, `$$E$$` uidata, `$$UB$$`
//!   uicontrol, `$$JI$$` gospageinit, `$$NB$$` goslocalize; owner static
//!   analysis on Rally #159) that keys every family by (host, dispatch value).
//! * [`source_map`] (F38-C) gives every measured site its member, byte offset,
//!   line and column, read from the program's own bytes and never retained.
//! * [`probe`] is the isolated signature probe (F13-C): a caller-supplied set
//!   of measured instruction/native signatures, one walk per located program
//!   whose stop is recorded as data, a retry for exactly the stops more
//!   evidence fixes, and the reachability verdict every inventory record
//!   keeps — an unused unknown record stays visible with its evidence.
//! * [`model`] is the F13-D classification (task #640): which of the located
//!   program families follow a **real model** — a `.zrd` typed tree read by
//!   keyword, a named text-command script, a stream of typed animation event
//!   records — instead of being an unknown numeric opcode stream. The owner's
//!   static analysis of the decrypted executable established that none of the
//!   three families F13-B walked as binary words is an opcode stream; a
//!   program with such a model is filed with its evidence and never walked.
//!
//! The classification of the retail corpus is F13-B's `retail` work; F13-C
//! resolves signatures; F13-D (task #640, owner note 2026-10-05) corrects the
//! premise that any of the three families is a numeric opcode stream — see
//! `docs/findings/2026-10-14-f13-d-every-program-family-has-its-real-model.md`.
//! The design decisions and the recorded unknowns are in
//! `docs/findings/2026-09-29-f13-a-script-inventory-and-evidence-schema.md`
//! and `docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md`.
//! Fixtures exercised by `crates/cs_formats/tests/script_raw/` are newly
//! authored synthetic bytes; nothing here is derived from original game data.

pub mod discovery;
pub mod evidence;
pub mod inventory;
pub mod ledger;
pub mod model;
pub mod probe;
pub mod source_map;
pub mod ui_host_calls;

pub use discovery::{
    ANIMATION_HEADER_BYTES, CAM_ANIM_MEMBER, ContainerDiscovery, DiscoveryFinding, LocatedProgram,
    MIS_ANIM_MEMBER, MISSION_CONTROL_MEMBERS, MISSION_MEMBER_REASON, ProgramKind,
    READER_MEMBER_REASON, discover_container, mission_scope,
};
pub use evidence::{
    ByteSpan, Confidence, EvidenceError, EvidenceLocator, LocatorKind, MAX_NOTE_BYTES,
    ResearchMethod, ScriptEvidence,
};
pub use inventory::{
    Candidacy, DispatchOutcome, EXCLUDED_FAMILY_REASON, FormatDiscriminator,
    INTERP_BODY_UNESTABLISHED, InstructionClaimError, InstructionStatus, InventoryFinding,
    InventoryStats, MAX_LEADS_PER_RECORD, MIN_LEAD_BYTES, RecordKind, RecordReachability,
    RecordReference, ReferenceKind, STRUCTURE_UNUSED_REASON, ScriptContainerEntry, ScriptInventory,
    ScriptRecord, ScriptRole, ScriptSource, UNDECODED_UNESTABLISHED, UNREACHED_UNUSED_REASON,
    inventory_scripts,
};
pub use ledger::{
    LedgerError, MAX_OPCODE_BYTES, OpcodeEntry, OpcodeLedger, ProgramError, ProgramLocator,
    ReachedOpcode, walk_program,
};
pub use model::{
    ANALYSIS_MODULE, EVENT_RECORDS_RVA, ModelledProgramProbe, NAMED_COMMANDS_RVA,
    NamedCommandsError, ProgramModel, TYPED_TREE_RVA, model_basis, reads_as_named_commands,
};
pub use probe::{
    ClaimError, DirectiveKeyRow, DirectiveKeyStatus, DirectiveProgramProbe, ModelError,
    ProbeConfig, ProbeError, ProbeReport, ProbeSession, ProgramProbe, RecordFieldSupport,
    RecordKeyRow, RecordKeyStatus, RecordProbeError, RecordProbeStats, SignatureClaim,
    SignatureShape, SignatureTable, probe_records,
};
pub use ui_host_calls::{
    ArgShape, ArgShapeCounts, CorpusMember, DispatchForm, HostCallCorpus, HostCallSite, HostKind,
    MAX_ARG_EXPR_BYTES, MAX_BLOCK_LABEL_BYTES, MAX_HOST_CALL_ARGS, MAX_HOST_CALL_SITES,
    MAX_UI_SCRIPT_BYTES, ObservedHostCall, OtherCallHead, UiNative, UiProgramScan, UiScriptError,
    UiScriptLimits, measure_host_call_corpus, scan_ui_program,
};
