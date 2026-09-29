//! The script inventory: every candidate script container, every byte range
//! in it and what is — and is not — known about each range (spec F13,
//! stage F13-A, AC01).
//!
//! [`inventory_scripts`] takes the containers a caller found (provenance
//! label, installation-relative path, bytes) and returns a
//! [`ScriptInventory`]:
//!
//! * each container is routed through the F06 two-key [`dispatch`]; a
//!   refusal is kept as [`DispatchOutcome::Refused`] with its code and the
//!   container stays a candidate of [`ScriptRole::Unknown`] — an unroutable
//!   container is not evidence that it holds no script;
//! * the interp, animation and reader families are candidates with the
//!   role their evidence supports; texture, sound and GameZ containers are
//!   listed as [`Candidacy::Excluded`] with the reason, never dropped;
//! * an INTERP container is split by the F07 reader into its header, index
//!   entries and scripts, plus an [`RecordKind::Unclaimed`] record for every
//!   byte range none of them covers; any other candidate is one
//!   [`RecordKind::Opaque`] record spanning the container;
//! * every record carries its byte range, format discriminator, references
//!   and an [`InstructionStatus`]. The inventory never marks a record as
//!   instructions: that takes [`ScriptRecord::establish_instructions`] with
//!   evidence that can establish a meaning, and a scan cannot
//!   (non-negotiable #1, "a scan is not a decoder");
//! * printable-string runs are recorded as [`ReferenceKind::StringLead`]
//!   spans at [`Confidence::Lead`] — where to look next, not what anything
//!   means. References are spans: no container bytes are copied into the
//!   inventory.

use std::fmt;

use cs_types::install::RelativePath;

use super::evidence::{ByteSpan, Confidence, EvidenceLocator, ResearchMethod, ScriptEvidence};
use crate::interp::{INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, read_interp};
use crate::io::ParseContext;
use crate::zbd::{DispatchBasis, HeaderStatus, ZbdFamily, ZbdProbe, dispatch};

/// Shortest printable run recorded as a string lead.
pub const MIN_LEAD_BYTES: usize = 4;

/// Most string leads kept per record; the rest are counted in an
/// [`InventoryFinding::LeadsTruncated`].
pub const MAX_LEADS_PER_RECORD: usize = 256;

/// Why an INTERP script body has no established instruction meaning.
pub const INTERP_BODY_UNESTABLISHED: &str = "INTERP lines are F07 loading commands; no evidence \
     relates them to the mission language yet";

/// Why an undecoded record has no established instruction meaning.
pub const UNDECODED_UNESTABLISHED: &str =
    "no reader decodes these bytes; they are not known to be instructions";

/// Why a candidate family is excluded from the script search.
pub const EXCLUDED_FAMILY_REASON: &str = "no committed evidence ties this family to script, \
     animation-event or binding records";

/// Why a record no located program overlaps is unused when the record is
/// decoded container structure (an INTERP header or index entry): it is read
/// by the reader, not reached as a program.
pub const STRUCTURE_UNUSED_REASON: &str =
    "decoded container structure; no located program overlaps it";

/// Why a record no located program overlaps is unused when the record is a
/// body: no probe reaches these bytes.
pub const UNREACHED_UNUSED_REASON: &str =
    "no located program overlaps these bytes, so no probe reaches this record";

/// One container handed to the inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptSource<'a> {
    container: &'a str,
    path: &'a RelativePath,
    bytes: &'a [u8],
}

impl<'a> ScriptSource<'a> {
    /// A container: its provenance label (opaque to this crate), its
    /// installation-relative path and its whole bytes.
    pub const fn new(container: &'a str, path: &'a RelativePath, bytes: &'a [u8]) -> Self {
        Self {
            container,
            path,
            bytes,
        }
    }
}

/// The role a candidate container likely plays. Kept distinct until
/// evidence proves a relationship (spec F13 deliverable).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptRole {
    /// The INTERP loading-script container (F07).
    LoadingScript,
    /// A camera animation archive (`cam_anim.zbd`).
    CameraAnimation,
    /// A mission animation archive (`mis_anim.zbd`).
    MissionAnimation,
    /// A reader archive (`zrdr.zbd`).
    ReaderFile,
    /// Nothing identifies the role.
    Unknown,
}

impl ScriptRole {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::LoadingScript => "loading_script",
            Self::CameraAnimation => "camera_animation",
            Self::MissionAnimation => "mission_animation",
            Self::ReaderFile => "reader_file",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether a container is searched for script records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Candidacy {
    /// A candidate, with its likely role and how well that role is supported.
    Candidate {
        /// The likely role.
        role: ScriptRole,
        /// Support for the role.
        confidence: Confidence,
    },
    /// Listed but not searched: the family is known and nothing ties it to
    /// scripts.
    Excluded {
        /// The dispatched family.
        family: ZbdFamily,
        /// Why it is excluded.
        reason: &'static str,
    },
}

/// What the F06 dispatch decided for the container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Routed to a family.
    Dispatched {
        /// The family.
        family: ZbdFamily,
        /// Which key identified it.
        basis: DispatchBasis,
    },
    /// Dispatch refused the container; the code is
    /// `ZbdDispatchError::code`.
    Refused {
        /// The refusal code.
        code: &'static str,
    },
}

/// What identifies a record's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatDiscriminator {
    /// A documented signature and version validated.
    Signature {
        /// The signature word.
        signature: u32,
        /// The version word.
        version: u32,
    },
    /// The family is known but its header layout is undocumented.
    Unvalidated {
        /// The family's recorded reason.
        reason: &'static str,
    },
    /// Nothing identifies the format.
    Unknown,
}

impl FormatDiscriminator {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Signature { .. } => "signature",
            Self::Unvalidated { .. } => "unvalidated",
            Self::Unknown => "unknown",
        }
    }
}

/// What a record is, structurally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    /// The 12-byte INTERP header.
    InterpHeader,
    /// One 128-byte INTERP index entry.
    InterpIndexEntry {
        /// Position in the index.
        position: usize,
    },
    /// One INTERP script body, first line to terminator.
    InterpScript {
        /// Index position of the entry that points at it.
        position: usize,
    },
    /// Bytes of a decoded container that no decoded record covers.
    Unclaimed,
    /// A container no reader decodes, as one record.
    Opaque,
}

impl RecordKind {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::InterpHeader => "interp_header",
            Self::InterpIndexEntry { .. } => "interp_index_entry",
            Self::InterpScript { .. } => "interp_script",
            Self::Unclaimed => "unclaimed",
            Self::Opaque => "opaque",
        }
    }
}

/// What a reference inside a record is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    /// A script name stored in an INTERP index entry.
    ScriptName,
    /// A printable-string run a scan found: a lead, nothing more.
    StringLead,
}

/// A reference inside a record, as a span (no bytes are copied).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordReference {
    /// What the reference is.
    pub kind: ReferenceKind,
    /// Where it is stored.
    pub span: ByteSpan,
    /// How well its kind is supported.
    pub confidence: Confidence,
}

/// Whether a record's bytes are known to be instructions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstructionStatus {
    /// Decoded container structure (a header, an index entry), not a
    /// program body.
    ContainerStructure,
    /// Not known. The default for every body the inventory records.
    Unestablished {
        /// Why.
        reason: &'static str,
    },
    /// Established by evidence that can establish a meaning.
    Established {
        /// The evidence.
        evidence: ScriptEvidence,
    },
}

impl InstructionStatus {
    /// Stable lowercase label for reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ContainerStructure => "container_structure",
            Self::Unestablished { .. } => "unestablished",
            Self::Established { .. } => "established",
        }
    }
}

/// Whether any located program reaches a record, attached by
/// `crate::script_raw::probe::probe_records` (spec F13 AC03).
///
/// A record nobody's probe uses stays in the inventory with this verdict and
/// its evidence: dropping it would hide exactly the bytes F13 still has to
/// explain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordReachability {
    /// At least one located program's byte range overlaps the record.
    Used {
        /// How many located programs overlap it.
        programs: usize,
    },
    /// No located program overlaps the record; `reason` says why that is
    /// expected (structure) or that nothing reaches it.
    Unused {
        /// Why nothing overlaps it.
        reason: &'static str,
    },
}

impl RecordReachability {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Used { .. } => "used",
            Self::Unused { .. } => "unused",
        }
    }

    /// How many located programs overlap the record (0 when unused).
    pub const fn programs(self) -> usize {
        match self {
            Self::Used { programs } => programs,
            Self::Unused { .. } => 0,
        }
    }

    /// Whether no located program overlaps the record.
    pub const fn is_unused(self) -> bool {
        matches!(self, Self::Unused { .. })
    }

    /// The reachability evidence for `span` in `container`: a structural
    /// tool observation, never a claim that the bytes are instructions.
    pub fn evidence(self, container: &str, span: ByteSpan) -> ScriptEvidence {
        let note = match self {
            Self::Used { programs } => format!("{programs} located program(s) overlap this record"),
            Self::Unused { reason } => (*reason).to_owned(),
        };
        ScriptEvidence::new(
            ResearchMethod::StructuralDecode,
            Confidence::ObservedTool,
            EvidenceLocator::ContainerSpan {
                container: container.to_owned(),
                span,
            },
            note,
        )
        .expect("reachability evidence is a short structural note")
    }
}

/// Why [`ScriptRecord::establish_instructions`] refused a claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstructionClaimError {
    /// The evidence cannot establish a meaning: a scan, or a confidence
    /// below [`Confidence::Documented`].
    InsufficientEvidence {
        /// The evidence's method.
        method: ResearchMethod,
        /// The evidence's confidence.
        confidence: Confidence,
    },
    /// The record is decoded container structure, not a body.
    ContainerStructure {
        /// The record's span.
        span: ByteSpan,
    },
    /// The evidence points at a span of a different container.
    OtherContainer {
        /// The record's container label.
        record: String,
        /// The evidence's container label.
        evidence: String,
    },
    /// The evidence points at bytes outside the record.
    OutsideRecord {
        /// The record's span.
        record: ByteSpan,
        /// The evidence's span.
        evidence: ByteSpan,
    },
}

impl InstructionClaimError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InsufficientEvidence { .. } => "insufficient_evidence",
            Self::ContainerStructure { .. } => "container_structure",
            Self::OtherContainer { .. } => "other_container",
            Self::OutsideRecord { .. } => "outside_record",
        }
    }
}

impl fmt::Display for InstructionClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientEvidence { method, confidence } => write!(
                f,
                "{} evidence at `{}` cannot establish that bytes are instructions",
                method.label(),
                confidence.label()
            ),
            Self::ContainerStructure { span } => {
                write!(
                    f,
                    "record {span} is container structure, not a program body"
                )
            }
            Self::OtherContainer { record, evidence } => write!(
                f,
                "evidence about container `{evidence}` cannot establish a record of `{record}`"
            ),
            Self::OutsideRecord { record, evidence } => {
                write!(f, "evidence span {evidence} lies outside record {record}")
            }
        }
    }
}

impl std::error::Error for InstructionClaimError {}

/// One byte range of a candidate container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptRecord {
    container: String,
    span: ByteSpan,
    kind: RecordKind,
    discriminator: FormatDiscriminator,
    references: Vec<RecordReference>,
    instructions: InstructionStatus,
    reachability: Option<RecordReachability>,
    evidence: Vec<ScriptEvidence>,
}

impl ScriptRecord {
    /// Provenance label of the container the record is in.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The byte range.
    pub const fn span(&self) -> ByteSpan {
        self.span
    }

    /// What the record is, structurally.
    pub const fn kind(&self) -> RecordKind {
        self.kind
    }

    /// What identifies its format.
    pub const fn discriminator(&self) -> FormatDiscriminator {
        self.discriminator
    }

    /// References inside it.
    pub fn references(&self) -> &[RecordReference] {
        &self.references
    }

    /// Whether its bytes are known to be instructions.
    pub const fn instructions(&self) -> &InstructionStatus {
        &self.instructions
    }

    /// The evidence behind its kind.
    pub fn evidence(&self) -> &[ScriptEvidence] {
        &self.evidence
    }

    /// Whether a reachability probe has run over this record yet.
    pub const fn reachability(&self) -> Option<RecordReachability> {
        self.reachability
    }

    /// Records where a located program reaches this record. Called by the
    /// reachability probe; probing again replaces the previous verdict
    /// instead of appending a second, contradictory one.
    pub fn set_reachability(&mut self, reachability: RecordReachability) {
        self.reachability = Some(reachability);
    }

    /// The reachability evidence of this record, when it has been probed.
    ///
    /// It is derived from the verdict rather than stored beside the kind
    /// evidence, so a re-probe can never leave two notes disagreeing about
    /// whether anything reaches these bytes.
    pub fn reachability_evidence(&self) -> Option<ScriptEvidence> {
        self.reachability
            .map(|reachability| reachability.evidence(&self.container, self.span))
    }

    /// Whether this is an **unused unknown record**: a byte range no reader
    /// decodes, whose format nothing identifies, that no located program
    /// overlaps and so no probe reaches (spec F13 AC03).
    ///
    /// `false` until [`Self::set_reachability`] has run: "unused" is a
    /// measured verdict, never a default for a record nobody looked at.
    pub fn is_unused_unknown(&self) -> bool {
        matches!(self.kind, RecordKind::Opaque | RecordKind::Unclaimed)
            && self.discriminator == FormatDiscriminator::Unknown
            && matches!(self.instructions, InstructionStatus::Unestablished { .. })
            && matches!(self.reachability, Some(RecordReachability::Unused { .. }))
    }

    /// Records that this record is an instruction stream.
    ///
    /// The only way a record becomes [`InstructionStatus::Established`]; the
    /// inventory never calls it.
    ///
    /// # Errors
    ///
    /// [`InstructionClaimError::InsufficientEvidence`] for a scan or a
    /// confidence below [`Confidence::Documented`],
    /// [`InstructionClaimError::ContainerStructure`] for a header or index
    /// record, [`InstructionClaimError::OtherContainer`] for a container
    /// span of another container and [`InstructionClaimError::OutsideRecord`]
    /// for a container span that does not lie inside the record.
    pub fn establish_instructions(
        &mut self,
        evidence: ScriptEvidence,
    ) -> Result<(), InstructionClaimError> {
        if !evidence.establishes_semantics() {
            return Err(InstructionClaimError::InsufficientEvidence {
                method: evidence.method(),
                confidence: evidence.confidence(),
            });
        }
        if self.instructions == InstructionStatus::ContainerStructure {
            return Err(InstructionClaimError::ContainerStructure { span: self.span });
        }
        if let EvidenceLocator::ContainerSpan { container, .. } = evidence.locator()
            && *container != self.container
        {
            return Err(InstructionClaimError::OtherContainer {
                record: self.container.clone(),
                evidence: container.clone(),
            });
        }
        if let EvidenceLocator::ContainerSpan { span, .. } = evidence.locator()
            && (span.offset < self.span.offset || span.end() > self.span.end())
        {
            return Err(InstructionClaimError::OutsideRecord {
                record: self.span,
                evidence: *span,
            });
        }
        self.instructions = InstructionStatus::Established { evidence };
        Ok(())
    }
}

/// Something the inventory could not do, kept rather than dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryFinding {
    /// The family's reader refused the container; it is recorded as one
    /// opaque record instead.
    ReaderRefused {
        /// The reader (`interp`).
        reader: &'static str,
        /// The reader's error code.
        code: &'static str,
        /// The offset the reader reported.
        offset: u64,
    },
    /// A record had more string leads than [`MAX_LEADS_PER_RECORD`].
    LeadsTruncated {
        /// The record's span.
        record: ByteSpan,
        /// Leads kept.
        kept: usize,
        /// Leads found.
        found: usize,
    },
}

impl InventoryFinding {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ReaderRefused { .. } => "reader_refused",
            Self::LeadsTruncated { .. } => "leads_truncated",
        }
    }
}

/// One container of the inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptContainerEntry {
    container: String,
    path: RelativePath,
    len: u64,
    dispatch: DispatchOutcome,
    candidacy: Candidacy,
    records: Vec<ScriptRecord>,
    findings: Vec<InventoryFinding>,
}

impl ScriptContainerEntry {
    /// Provenance label.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Installation-relative path.
    pub const fn path(&self) -> &RelativePath {
        &self.path
    }

    /// Container length in bytes.
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether the container is empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The F06 dispatch outcome.
    pub const fn dispatch(&self) -> DispatchOutcome {
        self.dispatch
    }

    /// Whether and as what it is searched.
    pub const fn candidacy(&self) -> Candidacy {
        self.candidacy
    }

    /// Its records in offset order (empty when excluded).
    pub fn records(&self) -> &[ScriptRecord] {
        &self.records
    }

    /// Mutable records, for a later stage attaching evidence.
    pub fn records_mut(&mut self) -> &mut [ScriptRecord] {
        &mut self.records
    }

    /// What the inventory could not do.
    pub fn findings(&self) -> &[InventoryFinding] {
        &self.findings
    }
}

/// Counts over an inventory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InventoryStats {
    /// Containers listed.
    pub containers: usize,
    /// Candidate containers.
    pub candidates: usize,
    /// Excluded containers.
    pub excluded: usize,
    /// Candidates dispatch refused.
    pub refused: usize,
    /// Records over all candidates.
    pub records: usize,
    /// Records whose instruction status is established.
    pub established: usize,
    /// Records whose instruction status is unestablished.
    pub unestablished: usize,
    /// Opaque or unclaimed records.
    pub undecoded: usize,
}

/// Every container handed to [`inventory_scripts`], ordered by logical path
/// then provenance label.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptInventory {
    entries: Vec<ScriptContainerEntry>,
}

impl ScriptInventory {
    /// Every container.
    pub fn entries(&self) -> &[ScriptContainerEntry] {
        &self.entries
    }

    /// Mutable containers, for a later stage attaching evidence.
    pub fn entries_mut(&mut self) -> &mut [ScriptContainerEntry] {
        &mut self.entries
    }

    /// Candidate containers only.
    pub fn candidates(&self) -> impl Iterator<Item = &ScriptContainerEntry> {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.candidacy, Candidacy::Candidate { .. }))
    }

    /// The counts.
    pub fn stats(&self) -> InventoryStats {
        let mut stats = InventoryStats {
            containers: self.entries.len(),
            ..InventoryStats::default()
        };
        for entry in &self.entries {
            match entry.candidacy {
                Candidacy::Candidate { .. } => stats.candidates += 1,
                Candidacy::Excluded { .. } => stats.excluded += 1,
            }
            if matches!(entry.dispatch, DispatchOutcome::Refused { .. }) {
                stats.refused += 1;
            }
            for record in &entry.records {
                stats.records += 1;
                match record.instructions {
                    InstructionStatus::Established { .. } => stats.established += 1,
                    InstructionStatus::Unestablished { .. } => stats.unestablished += 1,
                    InstructionStatus::ContainerStructure => {}
                }
                if matches!(record.kind, RecordKind::Opaque | RecordKind::Unclaimed) {
                    stats.undecoded += 1;
                }
            }
        }
        stats
    }
}

/// Builds the inventory of `sources`.
///
/// Never fails: a container dispatch refuses or its reader rejects is still
/// listed, as an opaque candidate with the refusal recorded.
pub fn inventory_scripts(sources: &[ScriptSource<'_>]) -> ScriptInventory {
    let mut entries: Vec<ScriptContainerEntry> = sources.iter().map(inventory_one).collect();
    entries.sort_by(|a, b| {
        (a.path.logical_key(), &a.container).cmp(&(b.path.logical_key(), &b.container))
    });
    ScriptInventory { entries }
}

fn inventory_one(source: &ScriptSource<'_>) -> ScriptContainerEntry {
    let len = source.bytes.len() as u64;
    let probe = ZbdProbe::new(source.container, source.path, source.bytes);
    let (dispatch, discriminator, candidacy) = match dispatch(probe) {
        Ok(decision) => {
            let family = decision.family();
            let discriminator = match decision.header_status() {
                HeaderStatus::Validated { signature, version } => {
                    FormatDiscriminator::Signature { signature, version }
                }
                HeaderStatus::Unvalidated { reason } => FormatDiscriminator::Unvalidated { reason },
            };
            (
                DispatchOutcome::Dispatched {
                    family,
                    basis: decision.basis(),
                },
                discriminator,
                candidacy_for(family, source.path),
            )
        }
        Err(error) => (
            DispatchOutcome::Refused { code: error.code() },
            FormatDiscriminator::Unknown,
            Candidacy::Candidate {
                role: ScriptRole::Unknown,
                confidence: Confidence::Unknown,
            },
        ),
    };

    let mut findings = Vec::new();
    let records = match (dispatch, candidacy) {
        (_, Candidacy::Excluded { .. }) => Vec::new(),
        (
            DispatchOutcome::Dispatched {
                family: ZbdFamily::Interp,
                ..
            },
            _,
        ) => match interp_records(source, discriminator, &mut findings) {
            Some(records) => records,
            None => vec![opaque_record(
                source,
                RecordKind::Opaque,
                ByteSpan::new(0, len),
                discriminator,
                &mut findings,
            )],
        },
        _ => vec![opaque_record(
            source,
            RecordKind::Opaque,
            ByteSpan::new(0, len),
            discriminator,
            &mut findings,
        )],
    };

    ScriptContainerEntry {
        container: source.container.to_owned(),
        path: source.path.clone(),
        len,
        dispatch,
        candidacy,
        records,
        findings,
    }
}

/// The candidacy of a dispatched family.
fn candidacy_for(family: ZbdFamily, path: &RelativePath) -> Candidacy {
    let candidate = |role, confidence| Candidacy::Candidate { role, confidence };
    match family {
        ZbdFamily::Interp => candidate(ScriptRole::LoadingScript, Confidence::Documented),
        ZbdFamily::Reader => candidate(ScriptRole::ReaderFile, Confidence::Inferred),
        ZbdFamily::Animation => {
            let key = path.logical_key();
            let basename = key.rsplit('/').next().unwrap_or(&key);
            match basename {
                "cam_anim.zbd" => candidate(ScriptRole::CameraAnimation, Confidence::Inferred),
                "mis_anim.zbd" => candidate(ScriptRole::MissionAnimation, Confidence::Inferred),
                _ => candidate(ScriptRole::Unknown, Confidence::Unknown),
            }
        }
        ZbdFamily::Texture | ZbdFamily::Sound | ZbdFamily::GameZ => Candidacy::Excluded {
            family,
            reason: EXCLUDED_FAMILY_REASON,
        },
    }
}

/// Splits an INTERP container into header, index entries, scripts and the
/// unclaimed ranges between them. `None` (with a finding) when the F07
/// reader refuses the bytes.
fn interp_records(
    source: &ScriptSource<'_>,
    discriminator: FormatDiscriminator,
    findings: &mut Vec<InventoryFinding>,
) -> Option<Vec<ScriptRecord>> {
    let mut context = ParseContext::with_defaults(source.container);
    let file = match read_interp(&mut context, source.bytes) {
        Ok(file) => file,
        Err(error) => {
            findings.push(InventoryFinding::ReaderRefused {
                reader: "interp",
                code: error.code(),
                offset: error.offset(),
            });
            return None;
        }
    };

    let decoded = |span: ByteSpan, what: &str| {
        ScriptEvidence::new(
            ResearchMethod::StructuralDecode,
            Confidence::ObservedTool,
            EvidenceLocator::ContainerSpan {
                container: source.container.to_owned(),
                span,
            },
            format!("read_interp validated this {what}"),
        )
        .expect("the structural-decode evidence is well formed")
    };

    let mut records = Vec::new();
    let header = ByteSpan::new(0, INTERP_HEADER_BYTES as u64);
    records.push(ScriptRecord {
        container: source.container.to_owned(),
        span: header,
        kind: RecordKind::InterpHeader,
        discriminator,
        references: Vec::new(),
        instructions: InstructionStatus::ContainerStructure,
        reachability: None,
        evidence: vec![decoded(header, "header")],
    });
    for script in file.scripts() {
        let entry = script.entry;
        let span = ByteSpan::new(entry.entry_offset, INDEX_ENTRY_BYTES as u64);
        records.push(ScriptRecord {
            container: source.container.to_owned(),
            span,
            kind: RecordKind::InterpIndexEntry {
                position: entry.index,
            },
            discriminator,
            references: vec![RecordReference {
                kind: ReferenceKind::ScriptName,
                span: ByteSpan::new(entry.entry_offset, entry.name_bytes().len() as u64),
                confidence: Confidence::ObservedTool,
            }],
            instructions: InstructionStatus::ContainerStructure,
            reachability: None,
            evidence: vec![decoded(span, "index entry")],
        });
    }
    for script in file.scripts() {
        let span = ByteSpan::from_range(u64::from(script.entry.script_offset), script.end())
            .expect("a read script ends after it starts");
        records.push(ScriptRecord {
            container: source.container.to_owned(),
            span,
            kind: RecordKind::InterpScript {
                position: script.entry.index,
            },
            discriminator,
            references: Vec::new(),
            instructions: InstructionStatus::Unestablished {
                reason: INTERP_BODY_UNESTABLISHED,
            },
            reachability: None,
            evidence: vec![decoded(span, "script body")],
        });
    }

    // Every byte no decoded record covers becomes an unclaimed record.
    let mut covered: Vec<ByteSpan> = records.iter().map(|record| record.span).collect();
    covered.sort();
    let mut cursor = 0u64;
    let mut gaps = Vec::new();
    for span in covered {
        if span.offset > cursor {
            gaps.push(ByteSpan::new(cursor, span.offset - cursor));
        }
        cursor = cursor.max(span.end());
    }
    let len = source.bytes.len() as u64;
    if cursor < len {
        gaps.push(ByteSpan::new(cursor, len - cursor));
    }
    for gap in gaps {
        records.push(opaque_record(
            source,
            RecordKind::Unclaimed,
            gap,
            FormatDiscriminator::Unknown,
            findings,
        ));
    }
    records.sort_by_key(|record| (record.span.offset, record.span.len));
    Some(records)
}

/// A record no reader decodes, with its string leads.
fn opaque_record(
    source: &ScriptSource<'_>,
    kind: RecordKind,
    span: ByteSpan,
    discriminator: FormatDiscriminator,
    findings: &mut Vec<InventoryFinding>,
) -> ScriptRecord {
    let (references, found) = string_leads(source.bytes, span);
    if found > references.len() {
        findings.push(InventoryFinding::LeadsTruncated {
            record: span,
            kept: references.len(),
            found,
        });
    }
    ScriptRecord {
        container: source.container.to_owned(),
        span,
        kind,
        discriminator,
        references,
        instructions: InstructionStatus::Unestablished {
            reason: UNDECODED_UNESTABLISHED,
        },
        reachability: None,
        evidence: Vec::new(),
    }
}

/// Printable-ASCII runs of at least [`MIN_LEAD_BYTES`] inside `span`: the
/// first [`MAX_LEADS_PER_RECORD`] as leads, plus the total found.
fn string_leads(bytes: &[u8], span: ByteSpan) -> (Vec<RecordReference>, usize) {
    let start = usize::try_from(span.offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let end = usize::try_from(span.end())
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let mut leads = Vec::new();
    let mut found = 0;
    let mut run_start = None;
    for position in start..=end {
        let printable = position < end
            && bytes
                .get(position)
                .is_some_and(|byte| (0x20..=0x7e).contains(byte));
        match (printable, run_start) {
            (true, None) => run_start = Some(position),
            (false, Some(first)) => {
                run_start = None;
                if position - first >= MIN_LEAD_BYTES {
                    found += 1;
                    if leads.len() < MAX_LEADS_PER_RECORD {
                        leads.push(RecordReference {
                            kind: ReferenceKind::StringLead,
                            span: ByteSpan::new(first as u64, (position - first) as u64),
                            confidence: Confidence::Lead,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    (leads, found)
}
