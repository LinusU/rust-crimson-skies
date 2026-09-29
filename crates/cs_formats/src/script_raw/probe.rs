//! Isolated instruction/native signature probes and record reachability
//! (spec F13, stage F13-C; shared contract `docs/contracts/SCRIPT-MISSION.md`).
//!
//! F13-B located the campaign's programs and left the opcode ledger empty: the
//! public research establishes no Crimson Skies mission opcode table. This
//! stage is the typed path a **measured** signature travels from a probe into
//! that ledger, and the reachability answer for every record nobody used:
//!
//! * [`SignatureClaim`] is one resolved instruction or native signature: the
//!   opcode value, its spelling, the [`ProgramKind`] it was measured in, its
//!   arity, its argument/effect/timing/error shape and the
//!   [`ScriptEvidence`] behind it. A claim whose evidence cannot establish a
//!   meaning (`ScriptEvidence::establishes_semantics`) is refused: a scan
//!   never resolves a signature (spec F13 non-negotiable #1/#2).
//! * [`SignatureTable`] is the checked set of claims. It refuses a duplicate
//!   opcode — one value with two meanings is exactly the ambiguity F13 has to
//!   resolve — and [`SignatureTable::ledger`] turns it into the
//!   [`OpcodeLedger`] the walk consults. The table ships **empty**; nothing in
//!   this crate measures a mission opcode.
//! * [`ProbeSession`] walks one located program at a time **in isolation**:
//!   a program that stops at an unknown opcode does not stop its neighbours,
//!   the stop is recorded as data instead of being swallowed, and
//!   [`ProbeSession::retry`] re-runs exactly the probes more evidence can fix
//!   ([`ProgramError::UnknownOpcode`]) while refusing a structural stop.
//!   [`ProbeSession::teardown`] snapshots the report and closes the session,
//!   so a probe after teardown is an error, not a silent success.
//! * [`probe_records`] answers the other half of AC03: for **every** record of
//!   one container it attaches [`RecordReachability`] — which located programs
//!   overlap the record, or that none does. A record no reader decodes, no
//!   program covers and no claim names (an *unused unknown record*) stays in
//!   the inventory and in the report with that evidence instead of being
//!   dropped by the resolution pass.
//!
//! The claim data, the instruction unit and the budget are all caller-supplied
//! exactly as F07-D's classification is: this crate ships no opcode table and
//! hides no instruction width (spec F13 "Research boundary"). Every fixture
//! exercised by `crates/cs_formats/tests/script_raw/` is newly authored
//! synthetic bytes; nothing here is derived from original game data.

use std::collections::BTreeMap;
use std::fmt;

use super::discovery::{ContainerDiscovery, LocatedProgram, ProgramKind};
use super::evidence::{ByteSpan, Confidence, ResearchMethod, ScriptEvidence};
use super::inventory::{
    InstructionStatus, RecordReachability, STRUCTURE_UNUSED_REASON, ScriptContainerEntry,
    UNREACHED_UNUSED_REASON,
};
use super::ledger::{
    MAX_OPCODE_BYTES, OpcodeEntry, OpcodeLedger, ProgramError, ProgramLocator, ReachedOpcode,
};

/// Why a [`SignatureClaim`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimError {
    /// A required text field is empty: a signature that says nothing is not
    /// a resolved signature.
    EmptyField {
        /// The opcode the refused claim named.
        opcode: u32,
        /// Which field was empty.
        field: &'static str,
    },
    /// The evidence cannot establish a meaning: a scan, or a confidence below
    /// [`Confidence::Documented`]. Such a claim stays a hypothesis.
    WeakEvidence {
        /// The opcode the refused claim named.
        opcode: u32,
        /// The evidence's method.
        method: ResearchMethod,
        /// The evidence's confidence.
        confidence: Confidence,
    },
    /// Another claim already resolves this opcode value.
    Duplicate {
        /// The opcode both claims named.
        opcode: u32,
        /// The spelling already in the table.
        existing: String,
    },
}

impl ClaimError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyField { .. } => "empty_field",
            Self::WeakEvidence { .. } => "weak_evidence",
            Self::Duplicate { .. } => "duplicate_opcode",
        }
    }
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { opcode, field } => {
                write!(f, "opcode {opcode:#x} has an empty `{field}`")
            }
            Self::WeakEvidence {
                opcode,
                method,
                confidence,
            } => write!(
                f,
                "opcode {opcode:#x}: {} evidence at `{}` cannot resolve a signature",
                method.label(),
                confidence.label()
            ),
            Self::Duplicate { opcode, existing } => write!(
                f,
                "opcode {opcode:#x} is already resolved as `{existing}`; two signatures for one \
                 value is ambiguous"
            ),
        }
    }
}

impl std::error::Error for ClaimError {}

/// The argument, effect, timing and error shape one probe resolved for an
/// opcode. The four strings are the caller's measured summary (SCRIPT-MISSION
/// "Host interface": source signature, argument domains, effect phase and
/// error result); this crate never fills them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureShape {
    signature: String,
    effects: String,
    timing: String,
    errors: String,
}

impl SignatureShape {
    /// The measured shape. Emptiness is checked by [`SignatureClaim::new`],
    /// which knows the opcode the refusal is about.
    pub fn new(
        signature: impl Into<String>,
        effects: impl Into<String>,
        timing: impl Into<String>,
        errors: impl Into<String>,
    ) -> Self {
        Self {
            signature: signature.into(),
            effects: effects.into(),
            timing: timing.into(),
            errors: errors.into(),
        }
    }

    /// The argument/result shape, for example `actor,scalar -> bool`.
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// What the call changes.
    pub fn effects(&self) -> &str {
        &self.effects
    }

    /// When the effect lands relative to the instruction.
    pub fn timing(&self) -> &str {
        &self.timing
    }

    /// How a failed call reports itself.
    pub fn errors(&self) -> &str {
        &self.errors
    }
}

/// One resolved instruction or native signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureClaim {
    opcode: u32,
    spelling: String,
    program: ProgramKind,
    arity: u32,
    shape: SignatureShape,
    evidence: ScriptEvidence,
}

impl SignatureClaim {
    /// Validates one measured signature.
    ///
    /// # Errors
    ///
    /// [`ClaimError::EmptyField`] naming the first empty field and
    /// [`ClaimError::WeakEvidence`] when `evidence` cannot establish a
    /// meaning (a [`ResearchMethod::ContainerScan`] or anything below
    /// [`Confidence::Documented`]).
    pub fn new(
        opcode: u32,
        spelling: impl Into<String>,
        program: ProgramKind,
        arity: u32,
        shape: SignatureShape,
        evidence: ScriptEvidence,
    ) -> Result<Self, ClaimError> {
        let spelling = spelling.into();
        for (field, value) in [
            ("spelling", spelling.as_str()),
            ("signature", shape.signature()),
            ("effects", shape.effects()),
            ("timing", shape.timing()),
            ("errors", shape.errors()),
        ] {
            if value.trim().is_empty() {
                return Err(ClaimError::EmptyField { opcode, field });
            }
        }
        if !evidence.establishes_semantics() {
            return Err(ClaimError::WeakEvidence {
                opcode,
                method: evidence.method(),
                confidence: evidence.confidence(),
            });
        }
        Ok(Self {
            opcode,
            spelling,
            program,
            arity,
            shape,
            evidence,
        })
    }

    /// The opcode value this signature resolves.
    pub const fn opcode(&self) -> u32 {
        self.opcode
    }

    /// The claimed spelling of the opcode.
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// The program family the signature was measured in.
    pub const fn program(&self) -> ProgramKind {
        self.program
    }

    /// The operand words the probe observed.
    pub const fn arity(&self) -> u32 {
        self.arity
    }

    /// The argument, effect, timing and error shape.
    pub const fn shape(&self) -> &SignatureShape {
        &self.shape
    }

    /// The evidence behind the claim.
    pub const fn evidence(&self) -> &ScriptEvidence {
        &self.evidence
    }

    /// How far the claim is supported.
    pub const fn confidence(&self) -> Confidence {
        self.evidence.confidence()
    }

    /// The provenance of the claim, as the ledger's source.
    pub fn source(&self) -> &str {
        self.evidence.note()
    }
}

/// The checked set of resolved signatures, in opcode order. Empty by
/// construction: every insert is a caller's measurement, never a built-in
/// table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignatureTable {
    claims: BTreeMap<u32, SignatureClaim>,
}

impl SignatureTable {
    /// An empty table: no opcode is resolved.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `claim`, refusing an ambiguous claim.
    ///
    /// # Errors
    ///
    /// [`ClaimError::Duplicate`] when the opcode is already resolved.
    pub fn insert(&mut self, claim: SignatureClaim) -> Result<(), ClaimError> {
        if let Some(existing) = self.claims.get(&claim.opcode) {
            return Err(ClaimError::Duplicate {
                opcode: claim.opcode,
                existing: existing.spelling.clone(),
            });
        }
        self.claims.insert(claim.opcode, claim);
        Ok(())
    }

    /// The claim resolving `opcode`, when one was recorded.
    pub fn get(&self, opcode: u32) -> Option<&SignatureClaim> {
        self.claims.get(&opcode)
    }

    /// Every claim, in opcode order.
    pub fn claims(&self) -> impl Iterator<Item = &SignatureClaim> {
        self.claims.values()
    }

    /// Number of resolved opcodes.
    pub fn len(&self) -> usize {
        self.claims.len()
    }

    /// Whether no opcode is resolved.
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    /// The [`OpcodeLedger`] the program walk consults, built from these
    /// claims.
    ///
    /// Every accepted claim builds a valid entry, so this cannot fail: the
    /// table already refused an empty field, an empty source and a weak
    /// confidence on the way in.
    pub fn ledger(&self) -> OpcodeLedger {
        let mut ledger = OpcodeLedger::new();
        for claim in self.claims.values() {
            let entry = OpcodeEntry::new(
                claim.opcode,
                claim.spelling.clone(),
                claim.program,
                claim.confidence(),
                claim.source(),
            )
            .expect("an accepted claim builds a ledger entry");
            ledger
                .insert(entry)
                .expect("opcode keys are unique in the table");
        }
        ledger
    }
}

/// Why a probe configuration or a probe run was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// The instruction unit is outside `1..=MAX_OPCODE_BYTES`.
    InvalidWordWidth {
        /// The width asked for.
        word_bytes: u32,
    },
    /// The session was torn down, so it no longer runs probes.
    SessionClosed,
    /// No probe has that index.
    NoSuchProbe {
        /// The index asked for.
        index: usize,
        /// How many probes the session holds.
        probes: usize,
    },
    /// The recorded stop is structural, so more evidence cannot change it.
    NotRetryable {
        /// The index of the refused probe.
        index: usize,
        /// The stop's `ProgramError::code`, or `resolved`.
        code: &'static str,
    },
    /// The program handed to `retry` is not the one that was probed.
    MismatchedProgram {
        /// The index of the recorded probe.
        index: usize,
        /// Where the recorded probe lives.
        expected: String,
        /// Where the program handed in lives.
        found: String,
    },
    /// A claim could not be added to the table.
    Claim(ClaimError),
}

impl ProbeError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidWordWidth { .. } => "invalid_word_width",
            Self::SessionClosed => "session_closed",
            Self::NoSuchProbe { .. } => "no_such_probe",
            Self::NotRetryable { .. } => "not_retryable",
            Self::MismatchedProgram { .. } => "mismatched_program",
            Self::Claim(error) => error.code(),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWordWidth { word_bytes } => write!(
                f,
                "an opcode word of {word_bytes} bytes is not in 1..={MAX_OPCODE_BYTES}"
            ),
            Self::SessionClosed => f.write_str("the probe session was torn down"),
            Self::NoSuchProbe { index, probes } => {
                write!(
                    f,
                    "probe {index} does not exist; the session holds {probes}"
                )
            }
            Self::NotRetryable { index, code } => write!(
                f,
                "probe {index} stopped with `{code}`, which more evidence cannot change"
            ),
            Self::MismatchedProgram {
                index,
                expected,
                found,
            } => write!(
                f,
                "probe {index} recorded {expected}, but the program handed in is {found}"
            ),
            Self::Claim(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ProbeError {}

impl From<ClaimError> for ProbeError {
    fn from(error: ClaimError) -> Self {
        Self::Claim(error)
    }
}

/// What one probe run assumes and how far it may go.
///
/// Neither value is a constant of this crate: the fleet has not measured the
/// original instruction unit, so a probe states the width and the budget it
/// used instead of hiding a guess (spec F13; `2026-09-29-f13-b` findings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeConfig {
    word_bytes: u32,
    budget: u32,
}

impl ProbeConfig {
    /// The instruction unit and the per-program instruction budget.
    ///
    /// # Errors
    ///
    /// [`ProbeError::InvalidWordWidth`] for a width outside
    /// `1..=MAX_OPCODE_BYTES`. A budget of zero is accepted: it decodes
    /// nothing, which the walk reports as `budget_exceeded`.
    pub fn new(word_bytes: u32, budget: u32) -> Result<Self, ProbeError> {
        if word_bytes == 0 || word_bytes > MAX_OPCODE_BYTES {
            return Err(ProbeError::InvalidWordWidth { word_bytes });
        }
        Ok(Self { word_bytes, budget })
    }

    /// The assumed instruction unit in bytes.
    pub const fn word_bytes(self) -> u32 {
        self.word_bytes
    }

    /// The instruction budget of one program.
    pub const fn budget(self) -> u32 {
        self.budget
    }
}

/// One isolated probe of one located program: where the walk went, how many
/// attempts it took and why it stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramProbe {
    mission: String,
    locator: ProgramLocator,
    kind: ProgramKind,
    confidence: Confidence,
    attempts: u32,
    reached: Vec<ReachedOpcode>,
    stop: Option<ProgramError>,
}

impl ProgramProbe {
    /// The scope the walk reported: the program's mission, or its container
    /// label when the path scopes it to none.
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// Where the probed program lives.
    pub const fn locator(&self) -> &ProgramLocator {
        &self.locator
    }

    /// The role a name or a path supports.
    pub const fn kind(&self) -> ProgramKind {
        self.kind
    }

    /// How far that role is supported.
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// How many times this program was probed (1 on the first run).
    pub const fn attempts(&self) -> u32 {
        self.attempts
    }

    /// The opcodes of a walk that ran to the end. A stopped walk carries its
    /// stop instead: `walk_program` never returns partial progress, and this
    /// probe never pretends it did.
    pub fn reached(&self) -> &[ReachedOpcode] {
        &self.reached
    }

    /// Why the walk stopped; `None` when it ran to the end of the program.
    pub const fn stop(&self) -> Option<&ProgramError> {
        self.stop.as_ref()
    }

    /// Whether every reached opcode was resolved, so the whole program walked.
    pub const fn resolved(&self) -> bool {
        self.stop.is_none()
    }

    /// Whether the stop is an unknown opcode — the one failure more measured
    /// signatures can fix. A structural stop (empty program, bad width,
    /// truncated word, exhausted budget) is not retryable.
    pub fn retryable(&self) -> bool {
        matches!(self.stop, Some(ProgramError::UnknownOpcode { .. }))
    }

    /// The first unknown opcode, when that is what stopped the walk.
    pub fn first_unknown(&self) -> Option<(ByteSpan, u32)> {
        match &self.stop {
            Some(ProgramError::UnknownOpcode { pc, opcode, .. }) => Some((*pc, *opcode)),
            _ => None,
        }
    }

    /// `resolved`, or the stop's `ProgramError::code`.
    pub fn code(&self) -> &'static str {
        self.stop.as_ref().map_or("resolved", ProgramError::code)
    }
}

/// The outcome of a whole probe session, taken by
/// [`ProbeSession::teardown`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeReport {
    table: SignatureTable,
    config: ProbeConfig,
    probes: Vec<ProgramProbe>,
}

impl ProbeReport {
    /// The claims the session resolved with.
    pub const fn table(&self) -> &SignatureTable {
        &self.table
    }

    /// The instruction unit and budget the probes used.
    pub const fn config(&self) -> ProbeConfig {
        self.config
    }

    /// Every probe, in program order.
    pub fn probes(&self) -> &[ProgramProbe] {
        &self.probes
    }

    /// Number of probes.
    pub fn len(&self) -> usize {
        self.probes.len()
    }

    /// Whether no program was probed.
    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }

    /// Probes that walked their whole program.
    pub fn resolved(&self) -> usize {
        self.probes.iter().filter(|probe| probe.resolved()).count()
    }

    /// Probes that stopped somewhere.
    pub fn unresolved(&self) -> usize {
        self.probes.len() - self.resolved()
    }

    /// Probes that stopped at an unknown opcode, so more evidence could
    /// resolve them.
    pub fn retryable(&self) -> usize {
        self.probes.iter().filter(|probe| probe.retryable()).count()
    }

    /// Whether every probed program was resolved: the check a caller makes
    /// before it calls a signature table complete.
    pub fn complete(&self) -> bool {
        self.resolved() == self.probes.len()
    }
}

/// Walks one located program against `table`, in isolation.
fn run_probe(
    program: &LocatedProgram<'_>,
    table: &SignatureTable,
    config: ProbeConfig,
    attempts: u32,
) -> ProgramProbe {
    let ledger = table.ledger();
    let (reached, stop) = match program.walk(&ledger, config.word_bytes(), config.budget()) {
        Ok(reached) => (reached, None),
        Err(stop) => (Vec::new(), Some(stop)),
    };
    ProgramProbe {
        mission: program.mission_label().to_owned(),
        locator: program.locator().clone(),
        kind: program.kind(),
        confidence: program.confidence(),
        attempts,
        reached,
        stop,
    }
}

/// The isolated probes of a set of located programs.
///
/// One program at a time: a stop is recorded as data and never aborts the
/// run, the table can grow between probes with [`Self::extend`], and
/// [`Self::teardown`] closes the session.
#[derive(Clone, Debug)]
pub struct ProbeSession {
    table: SignatureTable,
    config: ProbeConfig,
    probes: Vec<ProgramProbe>,
    closed: bool,
}

impl ProbeSession {
    /// A session over `table` that walks with `config`.
    pub const fn new(table: SignatureTable, config: ProbeConfig) -> Self {
        Self {
            table,
            config,
            probes: Vec::new(),
            closed: false,
        }
    }

    /// The instruction unit and budget in force.
    pub const fn config(&self) -> ProbeConfig {
        self.config
    }

    /// The claims resolved so far.
    pub const fn table(&self) -> &SignatureTable {
        &self.table
    }

    /// Whether [`Self::teardown`] has run.
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Adds a newly measured signature, so a later probe can use it.
    ///
    /// # Errors
    ///
    /// [`ProbeError::SessionClosed`] after teardown and
    /// [`ProbeError::Claim`] for the refusal `SignatureTable::insert`
    /// reports.
    pub fn extend(&mut self, claim: SignatureClaim) -> Result<(), ProbeError> {
        if self.closed {
            return Err(ProbeError::SessionClosed);
        }
        self.table.insert(claim)?;
        Ok(())
    }

    /// Probes `program` once and returns its index.
    ///
    /// # Errors
    ///
    /// [`ProbeError::SessionClosed`] after teardown.
    pub fn probe(&mut self, program: &LocatedProgram<'_>) -> Result<usize, ProbeError> {
        if self.closed {
            return Err(ProbeError::SessionClosed);
        }
        let index = self.probes.len();
        let probe = run_probe(program, &self.table, self.config, 1);
        self.probes.push(probe);
        Ok(index)
    }

    /// Re-runs the probe at `index` against the current table.
    ///
    /// Only a probe that stopped at an unknown opcode can be retried: that is
    /// the one stop more measured signatures fix. A structural stop is
    /// refused instead of being re-run and silently left alone.
    ///
    /// # Errors
    ///
    /// [`ProbeError::SessionClosed`] after teardown,
    /// [`ProbeError::NoSuchProbe`] for an unknown index,
    /// [`ProbeError::MismatchedProgram`] when `program` is not the program
    /// that was probed and [`ProbeError::NotRetryable`] when the recorded
    /// stop is structural or the program already resolved.
    pub fn retry(&mut self, index: usize, program: &LocatedProgram<'_>) -> Result<(), ProbeError> {
        if self.closed {
            return Err(ProbeError::SessionClosed);
        }
        let (attempts, expected) = match self.probes.get(index) {
            Some(probe) if probe.retryable() => (probe.attempts, probe.locator.clone()),
            Some(probe) => {
                return Err(ProbeError::NotRetryable {
                    index,
                    code: probe.code(),
                });
            }
            None => {
                return Err(ProbeError::NoSuchProbe {
                    index,
                    probes: self.probes.len(),
                });
            }
        };
        if expected != *program.locator() {
            return Err(ProbeError::MismatchedProgram {
                index,
                expected: expected.to_string(),
                found: program.locator().to_string(),
            });
        }
        let probe = run_probe(program, &self.table, self.config, attempts + 1);
        self.probes[index] = probe;
        Ok(())
    }

    /// The probes recorded so far, without closing the session.
    pub fn probes(&self) -> &[ProgramProbe] {
        &self.probes
    }

    /// Snapshots the report and closes the session: the table and the probes
    /// are handed over, and every later [`Self::probe`], [`Self::retry`] or
    /// [`Self::extend`] fails with [`ProbeError::SessionClosed`] instead of
    /// continuing against a state nobody will read.
    pub fn teardown(&mut self) -> ProbeReport {
        self.closed = true;
        ProbeReport {
            table: self.table.clone(),
            config: self.config,
            probes: std::mem::take(&mut self.probes),
        }
    }
}

/// Why [`probe_records`] refused a pair of inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordProbeError {
    /// The inventory entry and the discovery are about different containers,
    /// so a reachability join would attach one container's programs to
    /// another's records.
    PathMismatch {
        /// The inventory entry's path.
        entry: String,
        /// The discovery's path.
        discovery: String,
    },
}

impl RecordProbeError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::PathMismatch { .. } => "path_mismatch",
        }
    }
}

impl fmt::Display for RecordProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathMismatch { entry, discovery } => write!(
                f,
                "inventory entry {entry} cannot be probed against the discovery of {discovery}"
            ),
        }
    }
}

impl std::error::Error for RecordProbeError {}

/// Counts over one container's reachability probe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordProbeStats {
    /// Records probed.
    pub records: usize,
    /// Records at least one located program overlaps.
    pub used: usize,
    /// Records no located program overlaps.
    pub unused: usize,
    /// Records that are undecoded, unknown and unused: the AC03 case.
    pub unused_unknown: usize,
}

/// Attaches [`RecordReachability`] to **every** record of `entry`.
///
/// The join is one directional and structural: a record is `used` when a
/// located program's byte range overlaps it, and `unused` otherwise, with a
/// reason that separates decoded container structure from a byte range
/// nobody's probe reaches. Nothing is dropped and no record becomes
/// instructions (spec F13 AC01/AC03).
///
/// # Errors
///
/// [`RecordProbeError::PathMismatch`] when `entry` and `discovery` are not
/// the same container.
pub fn probe_records(
    entry: &mut ScriptContainerEntry,
    discovery: &ContainerDiscovery<'_>,
) -> Result<RecordProbeStats, RecordProbeError> {
    if entry.path().as_str() != discovery.path().as_str() {
        return Err(RecordProbeError::PathMismatch {
            entry: entry.path().as_str().to_owned(),
            discovery: discovery.path().as_str().to_owned(),
        });
    }
    let programs: Vec<ByteSpan> = discovery
        .programs()
        .iter()
        .map(|program| program.locator().span())
        .collect();
    let mut stats = RecordProbeStats::default();
    for record in entry.records_mut() {
        let span = record.span();
        let overlapping = programs
            .iter()
            .filter(|program| overlaps(**program, span))
            .count();
        let reachability = if overlapping > 0 {
            RecordReachability::Used {
                programs: overlapping,
            }
        } else if matches!(record.instructions(), InstructionStatus::ContainerStructure) {
            RecordReachability::Unused {
                reason: STRUCTURE_UNUSED_REASON,
            }
        } else {
            RecordReachability::Unused {
                reason: UNREACHED_UNUSED_REASON,
            }
        };
        record.set_reachability(reachability);
        stats.records += 1;
        match reachability {
            RecordReachability::Used { .. } => stats.used += 1,
            RecordReachability::Unused { .. } => stats.unused += 1,
        }
        if record.is_unused_unknown() {
            stats.unused_unknown += 1;
        }
    }
    Ok(stats)
}

/// Whether two byte ranges share a byte.
const fn overlaps(a: ByteSpan, b: ByteSpan) -> bool {
    a.offset < b.end() && b.offset < a.end()
}
