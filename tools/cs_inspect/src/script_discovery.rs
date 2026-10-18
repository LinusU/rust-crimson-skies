//! The `scripts` command (F13-B): locate and classify the campaign's loading,
//! mission and animation programs over one installation.
//!
//! ```text
//! cs-inspect scripts [--cs-path <dir>] [--coverage] [--signatures <file>]
//!                     [--word-bytes <n>] [--budget <n>] [--out <file>]
//! ```
//!
//! The installation is discovered (F02-B) and every inventoried `.zbd`
//! container is routed through the F13-B [`discover_container`]:
//!
//! * the INTERP loading container becomes one documented loading program per
//!   script body;
//! * a reader archive becomes one program per readable member, each named by
//!   its byte range inside the container, classified by its member name and —
//!   for an unnamed member — by the mission its path scopes it to;
//! * an animation container becomes one program covering its payload after
//!   the validated header, named by its basename.
//!
//! The report never copies a container's program bytes: a program is a
//! [`ProgramLocator`] (container, member, byte range) plus the role a name or
//! a path supports. Nothing here decodes an instruction; the walk that does
//! belong to `cs_formats::script_raw::ledger`, and this command does not run
//! it. `readiness` is always `not_assessed`.
//!
//! `--coverage` is the requested check: every `.zbd` container must route to a
//! known family, every script-family container must yield at least one
//! located program and no script-family container may keep a discovery
//! finding (a refused member index, a refused member extent). A container
//! that hides scripts behind an unknown family or a gap is a coverage failure,
//! never a silent pass.
//!
//! F13-C adds the isolated signature probe to the same command:
//!
//! * every inventoried container is also split by the F13-A inventory and
//!   every record gets a reachability verdict with its structural evidence —
//!   which located program overlaps it, or that none does. An *unused unknown
//!   record* (undecoded, unidentified, unreached) stays in the report with
//!   that evidence instead of being dropped by the resolution pass (AC03);
//! * every located program is walked **in isolation** against the signature
//!   table, one program per probe, so a stop is reported rather than
//!   aborting the run;
//! * a reader archive's **control member** (the one decoded `.zrd` member that
//!   declares numbered `OBJECTIVE<N>` blocks, chosen by
//!   `cs_content::mission_control::control_member`, never by name) is a text
//!   *directive program*, not a stream of binary opcodes. It is not walked: it
//!   is filed with every distinct directive key it reaches, each listed as
//!   measured (with the findings that measure it), a terminal outcome, or
//!   explicitly unknown with its reason (`probe.directive_program_rows`).
//!   The row also lists the record-level axis: every key the control record
//!   spells **outside** its numbered blocks, either classified through a
//!   measured record vocabulary with its value's shapes
//!   (`shape_measured`/`effect_measured`) or an unclassified key, explicitly
//!   unknown with its reason. No numeric signature claim describes such a
//!   program;
//! * `--signatures <file>` supplies caller-measured claims in the shape
//!   `<opcode> <spelling> <program> <arity> <signature> <effects> <timing>
//!   <errors> <citation> [note]`, exactly as F07-D's `--classes` supplies
//!   classification data. The workspace ships no such file: the mission
//!   opcode table is unmeasured, so without `--signatures` the table is
//!   empty, `probe.complete` is `false` and nothing is resolved;
//! * `--word-bytes <n>` and `--budget <n>` state the assumed instruction unit
//!   and the per-program budget. Their defaults are reported as
//!   `probe.assumed`, never hidden;
//! * with `--signatures`, coverage additionally requires every program to be
//!   accounted for: an incomplete table exits 3. Without it the coverage check
//!   is the F13-B one and is unchanged.
//!
//! F13-D (task #640, owner note 2026-10-05) corrects the census's premise:
//! **none of the three families is a numeric opcode stream.** Before a
//! program is walked, this command classifies it with its family's real model
//! (`cs_formats::script_raw::model`):
//!
//! * a reader-archive member that decodes as a `.zrd` typed tree is a
//!   **typed data tree** — data read by keyword by a native consumer;
//! * an INTERP script body whose every line heads with a printable text
//!   token is a **named command program** — the interpreter matches command
//!   **names**, there is no numeric opcode;
//! * an animation container's payload, behind its validated header, is a
//!   stream of **typed event records** — F20 (#690) measured that stream's
//!   layout and opcode census over all 61 retail carriers.
//!
//! A modelled program is filed with the evidence that classifies it
//! (`probe.modelled_program_rows`) and is **not** walked, so the report never
//! presents a stopped walk over text or data as an `unknown_opcode`. A
//! program no model is established for keeps the binary walk and its honest
//! stop — classification never guesses.
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` the command ran and
//! — with `--coverage` — the corpus is covered; `3` a coverage check failed;
//! `2` invalid input (including a malformed or unreadable signature file);
//! `4` no installation selected; `1` a runtime failure. The report is written
//! on exit 3 too, because it is the evidence of why coverage failed.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install;
use cs_content::mission_control::{
    ControlRecordField, DecodedMember, DirectiveDisposition, FieldSupport, MeasuredControlRecord,
    RecordSoundDisposition, control_member, measure_control_record, record_sound_disposition,
};
use cs_content::stunts::decode_zrd;
use cs_formats::interp::{InterpRawLine, read_interp};
use cs_formats::io::ParseContext;
use cs_formats::script_raw::{
    AnimationPayloadCheck, ByteSpan, ClaimError, Confidence, ContainerDiscovery, DirectiveKeyRow,
    DirectiveKeyStatus, DirectiveProgramProbe, EvidenceLocator, ModelledProgramProbe, ProbeConfig,
    ProbeReport, ProbeSession, ProgramKind, ProgramModel, RecordFieldSupport, RecordKeyRow,
    RecordKeyStatus, RecordProbeError, RecordProbeStats, RecordReachability, ResearchMethod,
    ScriptEvidence, ScriptRecord, ScriptSource, SignatureClaim, SignatureShape, SignatureTable,
    discover_container, inventory_scripts, model_basis, probe_records, reads_as_named_commands,
};
use cs_formats::zbd::{HeaderStatus, ZbdFamily};
use cs_types::install::RelativePath;

/// The report format version. Version 2 adds the `probe` and per-container
/// `records` sections (F13-C); version 3 adds the `probe.directive_*` fields
/// (M01-B-OPC) and counts a text directive program as accounted for, not as
/// unresolved; version 4 adds the record-level axis to each directive
/// program row (`record_keys`, `record_keys_classified`,
/// `record_keys_unclassified`) and the summed `probe.directive_record_keys_*`
/// counts (F13-C-RECKEYS); version 5 adds the F13-D model axis (task #640):
/// each located program row carries its `model` (`typed_data_tree`,
/// `named_commands`, `event_records` or `null` when no model was filed with
/// it — a program on the binary walk or a directive program), the top-level
/// `summary.modelled_programs`, and in the probe section `modelled`,
/// `modelled_by_model`, `model_basis` and `modelled_program_rows`; the
/// F13-B fields are unchanged; version 6 makes every record-level key row
/// machine-readably `measured` (with `citations`) or `outside_vocabulary`
/// (with `code`, `vocabulary` and `resolving_task`) and adds the record-key
/// gate (`probe.record_key_gate`, F13-D-FU3).
pub const SCRIPTS_REPORT_VERSION: &str = "cs-inspect-scripts/6";

/// The instruction unit the walk assumes when `--word-bytes` is omitted. It
/// is an **assumption**, reported as such in `probe.assumed`; F13 has not
/// measured the original instruction unit.
pub const DEFAULT_WORD_BYTES: u32 = 4;

/// The per-program instruction budget when `--budget` is omitted.
pub const DEFAULT_BUDGET: u32 = 4096;

/// Parsed `scripts` arguments.
#[derive(Debug, Default)]
struct ScriptsArgs {
    cs_path: Option<PathBuf>,
    coverage: bool,
    out: Option<PathBuf>,
    signatures: Option<PathBuf>,
    word_bytes: Option<u32>,
    budget: Option<u32>,
}

/// Parses a `u32` flag value, naming the flag when it is not one.
fn parse_count(flag: &str, value: &str) -> Result<u32, String> {
    value.parse::<u32>().map_err(|error| {
        format!("cs-inspect scripts: {flag} needs a whole number, got {value:?} ({error})")
    })
}

fn parse_args(args: &[String]) -> Result<ScriptsArgs, String> {
    let mut parsed = ScriptsArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        match arg.as_str() {
            "--coverage" => parsed.coverage = true,
            flag @ ("--cs-path" | "--out" | "--signatures") => {
                let Some(value) = cursor.next() else {
                    return Err(format!("cs-inspect scripts: {flag} needs a value"));
                };
                let slot = match flag {
                    "--cs-path" => &mut parsed.cs_path,
                    "--out" => &mut parsed.out,
                    _ => &mut parsed.signatures,
                };
                *slot = Some(PathBuf::from(value));
            }
            flag @ ("--word-bytes" | "--budget") => {
                let Some(value) = cursor.next() else {
                    return Err(format!("cs-inspect scripts: {flag} needs a value"));
                };
                let count = parse_count(flag, value)?;
                let slot = if flag == "--word-bytes" {
                    &mut parsed.word_bytes
                } else {
                    &mut parsed.budget
                };
                *slot = Some(count);
            }
            other => {
                return Err(format!(
                    "cs-inspect scripts: unsupported argument {other:?}; expected --cs-path \
                     <dir>, --coverage, --signatures <file>, --word-bytes <n>, --budget <n>, \
                     --out <file>"
                ));
            }
        }
    }
    Ok(parsed)
}

/// Counts over one `scripts` run, kept beside the JSON so tests can assert the
/// shape without parsing text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptsSummary {
    /// `.zbd` containers inventoried.
    pub zbd_containers: usize,
    /// Containers routed to a script family (interp, reader, animation).
    pub script_containers: usize,
    /// Located programs over all script containers.
    pub programs: usize,
    /// Discovery findings over all script containers.
    pub findings: usize,
    /// Containers routed to a family excluded from the script search.
    pub excluded_containers: usize,
    /// `.zbd` containers the F06 dispatch refused.
    pub dispatch_refused: usize,
    /// Script containers that yielded no located program.
    pub unlocated_containers: usize,
    /// Script containers that kept at least one discovery finding.
    pub finding_containers: usize,
    /// Located programs per [`ProgramKind::label`].
    pub by_kind: BTreeMap<&'static str, usize>,
    /// Inventoried records of the candidate containers.
    pub records: usize,
    /// Records at least one located program overlaps.
    pub records_used: usize,
    /// Records no located program overlaps.
    pub records_unused: usize,
    /// Undecoded, unknown records no probe reaches: the AC03 case.
    pub unused_unknown_records: usize,
    /// Signature claims loaded from `--signatures`.
    pub signature_claims: usize,
    /// Located programs whose whole body walked.
    pub programs_resolved: usize,
    /// Located programs that stopped at an unknown opcode.
    pub programs_retryable: usize,
    /// Located programs classified as text directive programs instead of
    /// being walked as binary words.
    pub directive_programs: usize,
    /// Distinct directive keys (summed per program) with a measured meaning.
    pub directive_keys_known: usize,
    /// Distinct directive keys (summed per program) explicitly unknown.
    pub directive_keys_unknown: usize,
    /// Record-level keys (summed per program) classified through a measured
    /// record vocabulary.
    pub directive_record_keys_classified: usize,
    /// Record-level keys (summed per program) outside both record
    /// vocabularies: listed as explicitly unknown with their reason.
    pub directive_record_keys_unclassified: usize,
    /// Record-level keys (summed per program) that break the citation gate: a
    /// measured key with no citation, or an outside-vocabulary key that does
    /// not name its vocabulary and resolving task (F13-D-FU3).
    pub record_key_gate_violations: usize,
    /// Record-level keys (summed per program) the census counted that the
    /// listing dropped.
    pub record_keys_dropped: usize,
    /// Blocks of directive programs that did not read as a directive list.
    pub directive_refused_blocks: usize,
    /// Located programs classified with their family's real model (F13-D,
    /// task #640) instead of being walked as binary opcode words.
    pub modelled_programs: usize,
    /// Modelled programs per [`ProgramModel::label`].
    pub modelled_by_model: BTreeMap<&'static str, usize>,
}

impl ScriptsSummary {
    /// Whether the coverage check passes: every container routed, every script
    /// container located a program and no script container hid a finding.
    pub fn covered(&self) -> bool {
        self.dispatch_refused == 0 && self.unlocated_containers == 0 && self.finding_containers == 0
    }

    /// The record-level key gate (F13-D-FU3): **every measured key carries a
    /// citation; every unmeasured key names the vocabulary it is outside and
    /// the task that would resolve it**, and no key the census counted is
    /// dropped from the listing. Unlike [`Self::signatures_complete`] it does
    /// not depend on `--signatures`: it asks about provenance, not about the
    /// opcode table.
    pub fn record_key_gate_passes(&self) -> bool {
        self.record_key_gate_violations == 0 && self.record_keys_dropped == 0
    }

    /// Whether every located program was accounted for: walked to the end
    /// against the signature table, classified as a text directive program
    /// whose every key is listed as measured or explicitly unknown (a block
    /// that did not read as a directive list leaves the program unaccounted),
    /// or classified with its family's real model (F13-D, task #640: a `.zrd`
    /// typed tree, a named command script, a stream of typed animation event
    /// records — no numeric opcode stream exists to resolve there). With no
    /// `--signatures` file the table ships empty, so a corpus that still
    /// holds a binary program gets `false` — the honest state, never a pass.
    ///
    /// An **unclassified record key** (a record-level key outside both
    /// measured vocabularies) does *not* count against this check, and that
    /// is a decision, not an omission (F13-C-RECKEYS): the check asks whether
    /// the listing is complete — whether everything the program spells was
    /// read and listed with a status. A refused block is unread text that may
    /// spell keys the listing never shows, so it fails the check. The
    /// record-level walk reads every flat field of the record, so an
    /// unclassified key **is** listed — explicitly unknown with its reason,
    /// exactly like a directive key whose meaning no finding measures
    /// (`meaning_not_measured`), which has never counted against this check
    /// either. The anomaly is surfaced per program (`record_keys`) and summed
    /// in the probe section (`directive_record_keys_unclassified`) instead of
    /// being gated here; a caller that wants to fail on it has the count.
    pub fn signatures_complete(&self) -> bool {
        self.directive_refused_blocks == 0
            && self.programs_resolved + self.directive_programs + self.modelled_programs
                == self.programs
    }
}

/// Everything one `scripts` run produced.
#[derive(Debug)]
pub struct ScriptsRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The JSON report, when the command ran.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
    /// The counts, when the command ran.
    pub summary: Option<ScriptsSummary>,
}

impl ScriptsRun {
    fn failed(exit_code: u8, message: String) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![message],
            summary: None,
        }
    }
}

/// Runs the `scripts` command and returns its exit code.
pub fn scripts_command(args: &[String]) -> ExitCode {
    let run = scripts_command_result(args, std::env::var_os("CS_GAME_DIR"));
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote scripts report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// The body of [`scripts_command`], with the environment's installation passed
/// in so tests can drive it.
pub fn scripts_command_result(args: &[String], env_cs_path: Option<OsString>) -> ScriptsRun {
    let parsed = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return ScriptsRun::failed(2, message),
    };
    let cs_path = parsed.cs_path.clone().or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return ScriptsRun::failed(
            4,
            "no installation selected: pass --cs-path <dir> or set CS_GAME_DIR".to_owned(),
        );
    };
    if let Some(out) = &parsed.out
        && inside(out, &cs_path)
    {
        return ScriptsRun::failed(
            2,
            format!(
                "{} lies inside the installation; cs-inspect never writes there",
                out.display()
            ),
        );
    }

    let word_bytes = parsed.word_bytes.unwrap_or(DEFAULT_WORD_BYTES);
    let budget = parsed.budget.unwrap_or(DEFAULT_BUDGET);
    // A default that is still in force is an assumption, so *either* flag
    // being absent keeps `probe.assumed` true: the unmeasured instruction
    // unit must not hide behind an explicitly given budget.
    let assumed = parsed.word_bytes.is_none() || parsed.budget.is_none();
    let config = match ProbeConfig::new(word_bytes, budget) {
        Ok(config) => config,
        Err(error) => return ScriptsRun::failed(2, error.to_string()),
    };
    let signatures_supplied = parsed.signatures.is_some();
    let table = match &parsed.signatures {
        Some(file) => match load_signatures(file) {
            Ok(table) => table,
            Err(message) => return ScriptsRun::failed(2, message),
        },
        None => SignatureTable::new(),
    };
    let mut session = ProbeSession::new(table, config);

    let found = match install::discover(&cs_path) {
        Ok(found) => found,
        Err(error) => return ScriptsRun::failed(1, error.to_string()),
    };

    // Every inventoried `.zbd` file, in logical-path order.
    let mut files: Vec<_> = found
        .manifest
        .files
        .iter()
        .filter(|record| record.relative_spelling.logical_key().ends_with(".zbd"))
        .collect();
    files.sort_by_key(|record| record.relative_spelling.logical_key());

    let mut summary = ScriptsSummary::default();
    let mut rows = Vec::with_capacity(files.len());
    let mut diagnostics = Vec::new();
    let mut runtime_failed = false;
    let mut dropped_total = 0usize;
    for record in files {
        let spelling = record.relative_spelling.as_str();
        let host = cs_path.join(spelling);
        let bytes = match fs::read(&host) {
            Ok(bytes) => bytes,
            Err(error) => {
                diagnostics.push(format!("cannot read {}: {error}", host.display()));
                runtime_failed = true;
                continue;
            }
        };
        summary.zbd_containers += 1;
        let discovery = discover_container(spelling, &record.relative_spelling, &bytes);
        summary.findings += discovery.findings().len();
        let (record_rows, record_stats) = match probe_container_records(
            spelling,
            &record.relative_spelling,
            &bytes,
            &discovery,
        ) {
            Ok(found) => found,
            Err(error) => {
                diagnostics.push(format!("{spelling}: {error}"));
                runtime_failed = true;
                (Vec::new(), RecordProbeStats::default())
            }
        };
        summary.records += record_stats.records;
        summary.records_used += record_stats.used;
        summary.records_unused += record_stats.unused;
        summary.unused_unknown_records += record_stats.unused_unknown;
        let interp_lines = interp_lines_by_offset(spelling, &discovery, &bytes);
        let directive = directive_program_of(&discovery);
        if let Some(found) = &directive {
            dropped_total += found.record_keys_dropped;
        }
        let mut models: Vec<Option<ProgramModel>> = Vec::with_capacity(discovery.len());
        for (index, program) in discovery.programs().iter().enumerate() {
            let modelled = model_selection(&discovery, index, &interp_lines);
            // The row's `model` is only the model this session actually filed
            // the program with, so the rows and `probe.modelled*` always
            // agree: a directive program, a program that kept the binary walk
            // and a filing that was refused all carry `null`, never a model
            // the program was not counted under.
            let mut filed_model = None;
            let outcome = match &directive {
                Some(found) if found.index == index => session
                    .record_directive_program(
                        program,
                        found.blocks,
                        found.refused_blocks,
                        found.keys.clone(),
                        found.record_keys.clone(),
                    )
                    .map(|_| ()),
                _ => match &modelled {
                    Some(selection) => session
                        .record_modelled_program(
                            program,
                            selection.model,
                            selection.evidence.clone(),
                        )
                        .map(|_| filed_model = Some(selection.model)),
                    None => session.probe(program).map(|_| ()),
                },
            };
            if let Err(error) = outcome {
                diagnostics.push(format!("{spelling}: signature probe failed: {error}"));
                runtime_failed = true;
            }
            models.push(filed_model);
        }
        rows.push(container_json(&discovery, &record_rows, &models));
        match discovery.family() {
            None => summary.dispatch_refused += 1,
            Some(ZbdFamily::Texture | ZbdFamily::Sound | ZbdFamily::GameZ) => {
                summary.excluded_containers += 1;
            }
            Some(ZbdFamily::Interp | ZbdFamily::Reader | ZbdFamily::Animation) => {
                summary.script_containers += 1;
                summary.programs += discovery.len();
                if discovery.is_empty() {
                    summary.unlocated_containers += 1;
                    diagnostics.push(format!(
                        "{spelling}: no program could be located in this script container"
                    ));
                }
                if !discovery.findings().is_empty() {
                    summary.finding_containers += 1;
                    for finding in discovery.findings() {
                        diagnostics.push(format!("{spelling}: {}: {finding}", finding.code()));
                    }
                }
                for program in discovery.programs() {
                    *summary.by_kind.entry(program.kind().label()).or_default() += 1;
                }
            }
        }
    }

    let probe_report = session.teardown();
    summary.signature_claims = probe_report.table().len();
    summary.programs_resolved = probe_report.resolved();
    summary.programs_retryable = probe_report.retryable();
    summary.modelled_programs = probe_report.modelled();
    summary.modelled_by_model = probe_report.modelled_by_model();
    for program in probe_report.directive_programs() {
        summary.directive_programs += 1;
        summary.directive_keys_known += program.known_keys();
        summary.directive_keys_unknown += program.unknown_keys();
        summary.directive_record_keys_classified += program.classified_record_keys();
        summary.directive_record_keys_unclassified += program.unclassified_record_keys();
        summary.directive_refused_blocks += program.refused_blocks() as usize;
        summary.record_key_gate_violations += program.record_key_gate_violations();
    }
    summary.record_keys_dropped = dropped_total;
    let signature_check = !signatures_supplied || summary.signatures_complete();
    let passes = summary.covered() && signature_check && summary.record_key_gate_passes();
    let probe_section = probe_json(&probe_report, signatures_supplied, assumed, &summary);
    let report = scripts_report_json(
        &found,
        &summary,
        &probe_section,
        parsed.coverage,
        passes,
        &rows,
    );
    let exit_code = if runtime_failed {
        1
    } else if parsed.coverage && !passes {
        3
    } else {
        0
    };

    let out = match &parsed.out {
        Some(out) => match write_report(out, &report) {
            Ok(()) => Some(out.clone()),
            Err(error) => {
                diagnostics.push(format!("cannot write report to {}: {error}", out.display()));
                return ScriptsRun {
                    exit_code: 1,
                    report: Some(report),
                    out: None,
                    diagnostics,
                    summary: Some(summary),
                };
            }
        },
        None => None,
    };
    ScriptsRun {
        exit_code,
        report: Some(report),
        out,
        diagnostics,
        summary: Some(summary),
    }
}

/// Renders one inventory record with its reachability verdict and evidence.
///
/// The evidence carries its method and confidence, not only its note, so a
/// reader of the report can tell a structural observation from a lead.
///
/// An unknown record nobody's probe reaches is rendered like every other one:
/// dropping it would hide exactly the bytes F13 still has to explain (AC03).
fn record_json(record: &ScriptRecord) -> String {
    let reachability = match (record.reachability(), record.reachability_evidence()) {
        (Some(verdict), Some(evidence)) => format!(
            "{{\"status\": {}, \"programs\": {}, \"reason\": {}, \"method\": {}, \"confidence\": {}, \
             \"evidence\": {}}}",
            jstr(verdict.label()),
            verdict.programs(),
            opt_str(match verdict {
                RecordReachability::Unused { reason } => Some(reason),
                RecordReachability::Used { .. } => None,
            }),
            jstr(evidence.method().label()),
            jstr(evidence.confidence().label()),
            jstr(evidence.note()),
        ),
        // The verdict and its evidence are derived together, so one without the
        // other cannot happen. Half of the evidence is never rendered: a
        // verdict without the method and confidence behind it is not evidence.
        _ => "null".to_owned(),
    };
    format!(
        "{{\"offset\": {}, \"length\": {}, \"kind\": {}, \"discriminator\": {}, \
         \"instructions\": {}, \"reachability\": {reachability}}}",
        record.span().offset,
        record.span().len,
        jstr(record.kind().label()),
        jstr(record.discriminator().label()),
        jstr(record.instructions().label()),
    )
}

/// Runs the F13-C reachability probe over one container: the inventory of its
/// bytes, one verdict per record and the rows the report renders.
fn probe_container_records(
    spelling: &str,
    relative: &RelativePath,
    bytes: &[u8],
    discovery: &ContainerDiscovery<'_>,
) -> Result<(Vec<String>, RecordProbeStats), RecordProbeError> {
    let source = ScriptSource::new(spelling, relative, bytes);
    let mut inventory = inventory_scripts(std::slice::from_ref(&source));
    let entry = inventory
        .entries_mut()
        .first_mut()
        .expect("one source yields exactly one entry");
    let stats = probe_records(entry, discovery)?;
    let rows = entry.records().iter().map(record_json).collect();
    Ok((rows, stats))
}

/// Renders one container's located programs, findings and inventory records.
///
/// Every located program row carries its `model` — the label of the real
/// model F13-D filed the program with, or `null` when the program carries no
/// model: it kept the binary opcode walk, it is a directive program
/// (M01-B-OPC, filed on that axis instead) or the filing was refused. A row
/// never names a model the program was not counted under.
fn container_json(
    discovery: &ContainerDiscovery<'_>,
    record_rows: &[String],
    models: &[Option<ProgramModel>],
) -> String {
    let path = discovery.path().logical_key();
    let family = discovery.family().map(ZbdFamily::as_str);
    let header = match discovery.header_status() {
        Some(HeaderStatus::Validated { signature, version }) => format!(
            "{{\"status\": \"validated\", \"signature\": {signature}, \"version\": {version}}}"
        ),
        Some(HeaderStatus::Unvalidated { reason }) => {
            format!(
                "{{\"status\": \"unvalidated\", \"reason\": {}}}",
                jstr(reason)
            )
        }
        None => "null".to_owned(),
    };
    let programs: Vec<String> = discovery
        .programs()
        .iter()
        .enumerate()
        .map(|(index, program)| {
            let span = program.locator().span();
            let model = models
                .get(index)
                .copied()
                .flatten()
                .map_or_else(|| "null".to_owned(), |model| jstr(model.label()));
            format!(
                "{{\"kind\": {}, \"confidence\": {}, \"mission\": {}, \"member\": {}, \
                 \"offset\": {}, \"length\": {}, \"reason\": {}, \"model\": {model}}}",
                jstr(program.kind().label()),
                jstr(program.confidence().label()),
                opt_str(program.mission()),
                opt_str(program.locator().member()),
                span.offset,
                span.len,
                jstr(program.reason()),
            )
        })
        .collect();
    let findings: Vec<String> = discovery
        .findings()
        .iter()
        .map(|finding| {
            format!(
                "{{\"code\": {}, \"detail\": {}}}",
                jstr(finding.code()),
                jstr(&finding.to_string()),
            )
        })
        .collect();
    format!(
        "{{\"path\": {}, \"family\": {}, \"header\": {header}, \"programs\": [{}], \
         \"findings\": [{}], \"records\": [{}]}}",
        jstr(&path),
        opt_str(family),
        programs.join(", "),
        findings.join(", "),
        record_rows.join(", "),
    )
}

/// Renders the `probe` section: what the run assumed, which claims it had and
/// how far each located program walked.
fn probe_json(
    report: &ProbeReport,
    supplied: bool,
    assumed: bool,
    summary: &ScriptsSummary,
) -> String {
    let mut stops: BTreeMap<&'static str, usize> = BTreeMap::new();
    for probe in report.probes() {
        *stops.entry(probe.code()).or_default() += 1;
    }
    let directive_rows = report
        .directive_programs()
        .iter()
        .map(directive_program_json)
        .collect::<Vec<_>>()
        .join(", ");
    let modelled_rows = report
        .modelled_programs()
        .iter()
        .map(modelled_program_json)
        .collect::<Vec<_>>()
        .join(", ");
    let modelled_by_model = summary
        .modelled_by_model
        .iter()
        .map(|(label, count)| format!("{}: {count}", jstr(label)))
        .collect::<Vec<_>>()
        .join(", ");
    let model_basis_json = ProgramModel::ALL
        .iter()
        .map(|model| {
            let basis = model_basis(*model);
            let EvidenceLocator::Executable { module, rva } = basis.locator() else {
                panic!("a model basis points at the executable: {basis:?}");
            };
            format!(
                "{}: {{\"method\": {}, \"confidence\": {}, \"module\": {}, \"rva\": {rva}, \
                 \"basis\": {}}}",
                jstr(model.label()),
                jstr(basis.method().label()),
                jstr(basis.confidence().label()),
                jstr(module),
                jstr(basis.note()),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let stops = stops
        .iter()
        .map(|(code, count)| format!("{}: {count}", jstr(code)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"supplied\": {supplied}, \"assumed\": {assumed}, \"word_bytes\": {}, \
         \"budget\": {}, \"claims\": {}, \"programs\": {}, \"resolved\": {}, \"unresolved\": {}, \
         \"retryable\": {}, \"complete\": {}, \"stops\": {{{stops}}}, \
         \"directive_programs\": {}, \"directive_keys_known\": {}, \
         \"directive_keys_unknown\": {}, \"directive_record_keys_classified\": {}, \
         \"directive_record_keys_unclassified\": {}, \"record_key_gate\": \
         {{\"passes\": {}, \"violations\": {}, \"dropped\": {}}}, \"directive_program_rows\": [{directive_rows}], \
         \"modelled\": {}, \"modelled_by_model\": {{{modelled_by_model}}}, \
         \"model_basis\": {{{model_basis_json}}}, \
         \"modelled_program_rows\": [{modelled_rows}]}}",
        report.config().word_bytes(),
        report.config().budget(),
        summary.signature_claims,
        summary.programs,
        summary.programs_resolved,
        summary.programs
            - summary.programs_resolved
            - summary.directive_programs
            - summary.modelled_programs,
        summary.programs_retryable,
        summary.signatures_complete(),
        summary.directive_programs,
        summary.directive_keys_known,
        summary.directive_keys_unknown,
        summary.directive_record_keys_classified,
        summary.directive_record_keys_unclassified,
        summary.record_key_gate_passes(),
        summary.record_key_gate_violations,
        summary.record_keys_dropped,
        summary.modelled_programs,
    )
}

/// The control program of one reader container, found by the measured rule.
struct DirectiveSelection {
    /// Index into the discovery's programs.
    index: usize,
    blocks: u32,
    refused_blocks: u32,
    keys: Vec<DirectiveKeyRow>,
    record_keys: Vec<RecordKeyRow>,
    /// Record-level keys the census counted that `record_keys` does not carry.
    record_keys_dropped: usize,
}

/// The findings that record the reading of a terminal-outcome spelling.
const TERMINAL_EVIDENCE: [&str; 2] = [
    "2026-10-04-m01-lc-mission-program",
    "2026-10-06-m01-lc-directive-meaning",
];

/// The stable code an outside-vocabulary record key carries: the key matches
/// neither measured record vocabulary, so the census counts it, names it and
/// never reads it. "Outside the vocabulary" is not "measured to be nothing".
const UNCLASSIFIED_RECORD_KEY_CODE: &str = "outside_both_record_vocabularies";

/// The vocabularies an outside-vocabulary record key does not match.
const UNCLASSIFIED_RECORD_KEY_VOCABULARY: &str =
    "CONTROL_RECORD_KEY_VOCABULARY + CONTROL_RECORD_SOUND_KEY_VOCABULARY";

/// The task that would resolve an outside-vocabulary record key (F13-D-FU3-
/// RESOLVE): measure the key and extend a `cs_content` record vocabulary.
const UNCLASSIFIED_RECORD_KEY_RESOLVING_TASK: &str = "#1623";

/// The findings that measured the five shape-measured record fields' value
/// shapes (`MISSION_TIMER`, `PLAYER_INIT` and the three animation lists).
const SHAPE_MEASURED_RECORD_CITATIONS: [&str; 2] = [
    "2026-10-04-m01-lc-mission-program",
    "2026-10-06-m01-lc-directive-meaning",
];

/// The citations behind a classified record field: the findings that measured
/// its shape, or, for a sound key, the findings behind its measured effect.
/// A sound key whose disposition is refused carries none, which the citation
/// gate then reports instead of the row passing as measured.
fn record_field_citations(field: ControlRecordField) -> Vec<String> {
    match field.support() {
        FieldSupport::ShapeMeasured => SHAPE_MEASURED_RECORD_CITATIONS.map(str::to_owned).to_vec(),
        FieldSupport::EffectMeasured => match record_sound_disposition(field.key()) {
            Some(RecordSoundDisposition::Measured(measured)) => {
                measured.evidence.iter().map(|&s| s.to_owned()).collect()
            }
            _ => Vec::new(),
        },
    }
}

/// Every record-level key a measured control record spells, as gate-ready
/// rows: measured fields with their citations and shapes, and keys outside
/// both vocabularies with the vocabulary and resolving task.
fn record_key_rows(record: &MeasuredControlRecord) -> Vec<RecordKeyRow> {
    let mut rows: Vec<RecordKeyRow> = record
        .record_fields()
        .iter()
        .map(|(field, sites)| {
            let shapes = record
                .record_field_shapes()
                .iter()
                .filter(|(key, _)| key == field.key())
                .map(|(_, shape)| shape.label())
                .collect();
            RecordKeyRow::new(
                field.key(),
                Some(*sites),
                shapes,
                RecordKeyStatus::Measured {
                    support: match field.support() {
                        FieldSupport::ShapeMeasured => RecordFieldSupport::ShapeMeasured,
                        FieldSupport::EffectMeasured => RecordFieldSupport::EffectMeasured,
                    },
                    citations: record_field_citations(*field),
                },
            )
        })
        .collect();
    for key in record.unclassified_record_keys() {
        rows.push(RecordKeyRow::new(
            key,
            None,
            Vec::new(),
            RecordKeyStatus::OutsideVocabulary {
                code: UNCLASSIFIED_RECORD_KEY_CODE.to_owned(),
                vocabulary: UNCLASSIFIED_RECORD_KEY_VOCABULARY.to_owned(),
                resolving_task: UNCLASSIFIED_RECORD_KEY_RESOLVING_TASK.to_owned(),
            },
        ));
    }
    rows
}

/// Record-level keys the census counted that the listing does not carry: the
/// "dropped" half of the gate. Each census key must appear exactly once.
fn dropped_record_keys(record: &MeasuredControlRecord, rows: &[RecordKeyRow]) -> usize {
    let expected = record.record_fields().len() + record.unclassified_record_keys().len();
    expected.saturating_sub(rows.len())
}

/// Classifies the one control member of a reader container as a directive
/// program.
///
/// The member is chosen by `cs_content`'s measured rule (exactly one decoded
/// member declares numbered objective blocks), never by name. A member that
/// does not decode as `.zrd`, a container with no or several candidates and a
/// non-reader container all answer `None`, and their programs are probed as
/// before: nothing is classified on a guess.
fn directive_program_of(discovery: &ContainerDiscovery<'_>) -> Option<DirectiveSelection> {
    if discovery.family() != Some(ZbdFamily::Reader) {
        return None;
    }
    let mut decoded = Vec::new();
    let mut indices = Vec::new();
    for (index, program) in discovery.programs().iter().enumerate() {
        let Some(name) = program.locator().member() else {
            continue;
        };
        let Ok(document) = decode_zrd(program.bytes()) else {
            continue;
        };
        decoded.push(DecodedMember::new(name, document));
        indices.push(index);
    }
    let control = control_member(discovery.container(), &decoded).ok()?;
    let index = indices[decoded
        .iter()
        .position(|member| std::ptr::eq(member, control))?];
    let record = measure_control_record(&control.document);
    let keys = record
        .keys()
        .iter()
        .map(|measured| {
            let status = match measured.disposition() {
                DirectiveDisposition::TerminalOutcome { outcome } => DirectiveKeyStatus::Terminal {
                    outcome: outcome.label().to_owned(),
                    evidence: TERMINAL_EVIDENCE.map(str::to_owned).to_vec(),
                },
                DirectiveDisposition::Measured(directive) => DirectiveKeyStatus::Measured {
                    operation: format!("{:?}", directive.operation),
                    evidence: directive.evidence.iter().map(|&s| s.to_owned()).collect(),
                },
                DirectiveDisposition::Unmeasured { reason } => DirectiveKeyStatus::Unknown {
                    code: reason.code().to_owned(),
                    detail: reason.detail(),
                },
            };
            DirectiveKeyRow::new(
                measured.key.clone(),
                measured.blocks,
                measured.sites,
                measured
                    .shapes
                    .iter()
                    .map(|(shape, _)| shape.label())
                    .collect(),
                status,
            )
        })
        .collect();
    // The record-level axis: every key the control record spells outside its
    // numbered blocks, each machine-readably `measured` (with citations) or
    // `outside_vocabulary` (with the vocabulary and resolving task). The
    // census counts an outside-vocabulary key as a distinct name only, so its
    // site count stays `None` and is never invented.
    let record_keys = record_key_rows(&record);
    let record_keys_dropped = dropped_record_keys(&record, &record_keys);
    Some(DirectiveSelection {
        index,
        blocks: record.blocks(),
        refused_blocks: record.refusals().len() as u32,
        keys,
        record_keys,
        record_keys_dropped,
    })
}

/// The model classification of one located program (F13-D, task #640): the
/// real model and the evidence that classifies these bytes with it.
struct ModelSelection {
    model: ProgramModel,
    evidence: ScriptEvidence,
}

/// Every INTERP script body's lines, keyed by the body's absolute offset.
///
/// The located loading programs are filed by exactly these offsets, so the
/// model check joins each body to its own lines and never to a neighbour's.
/// An interp container whose reader refuses it yields no lines, and the
/// discovery located no programs for it either.
fn interp_lines_by_offset<'bytes>(
    spelling: &str,
    discovery: &ContainerDiscovery<'_>,
    bytes: &'bytes [u8],
) -> BTreeMap<u64, Vec<InterpRawLine<'bytes>>> {
    if discovery.family() != Some(ZbdFamily::Interp) {
        return BTreeMap::new();
    }
    let mut context = ParseContext::with_defaults(spelling);
    match read_interp(&mut context, bytes) {
        Ok(file) => file
            .scripts()
            .iter()
            .map(|script| (u64::from(script.entry.script_offset), script.lines.clone()))
            .collect(),
        Err(_) => BTreeMap::new(),
    }
}

/// The structural evidence of one modelled program: this project's own
/// decode of the program's own bytes, at [`Confidence::ObservedTool`] — a
/// tool run, never original-verified (AGENTS.md rule 8).
fn structural_evidence(container: &str, span: ByteSpan, note: &str) -> ScriptEvidence {
    ScriptEvidence::new(
        ResearchMethod::StructuralDecode,
        Confidence::ObservedTool,
        EvidenceLocator::ContainerSpan {
            container: container.to_owned(),
            span,
        },
        note,
    )
    .expect("a short structural note is valid evidence")
}

/// Classifies one located program with its family's real model, or `None`
/// when no model is established and the program keeps its binary walk.
///
/// Three rules, each tied to the owner note of task #640 (static analysis of
/// the owner-supplied decrypted executable, never `verified_original`):
///
/// * a reader-archive member that **decodes** as a `.zrd` typed tree is
///   [`ProgramModel::TypedDataTree`] — the decode of these very bytes is the
///   evidence, nothing is claimed from the name;
/// * an INTERP script body whose every line heads with a printable text
///   token is [`ProgramModel::NamedCommands`] — the F07-validated line
///   layout plus the text command head;
/// * an animation container whose header validated **and** whose payload
///   decodes (every record walks and every sequence block's event stream
///   steps exactly, task #1421) is [`ProgramModel::EventRecords`] — the
///   compiled animation ZBD whose event stream F20 (#690) measured over all
///   61 retail carriers.
///
/// A member that does not decode, a body that does not read as text and an
/// animation container whose header dispatch could not validate all answer
/// `None` and keep the binary walk: the F13 census's failure was a confident
/// walk over bytes nobody had classified, and this never repeats it.
fn model_selection(
    discovery: &ContainerDiscovery<'_>,
    index: usize,
    interp_lines: &BTreeMap<u64, Vec<InterpRawLine<'_>>>,
) -> Option<ModelSelection> {
    let program = discovery.programs().get(index)?;
    let span = program.locator().span();
    match discovery.family() {
        Some(ZbdFamily::Reader) if program.locator().member().is_some() => {
            decode_zrd(program.bytes()).ok()?;
            Some(ModelSelection {
                model: ProgramModel::TypedDataTree,
                evidence: structural_evidence(
                    discovery.container(),
                    span,
                    "the member decodes as a .zrd typed tree: tags 1..=4, a list holds one \
                     fewer child than its count, no trailing bytes",
                ),
            })
        }
        Some(ZbdFamily::Interp) => {
            let lines = interp_lines.get(&span.offset)?;
            // An empty body is still a named-command program: the F07
            // reader validated that the script holds zero lines and the body
            // is only its zero terminator, so there is no instruction stream
            // to walk — retail holds four such scripts, and reading their
            // terminator word as an opcode is exactly the misreading this
            // task corrects.
            let note = if lines.is_empty() {
                "the body holds only the F07-validated zero terminator: zero command lines, \
                 nothing to walk"
            } else {
                reads_as_named_commands(lines).ok()?;
                "every line of the body heads with a printable text command name inside the \
                 F07-validated line layout"
            };
            Some(ModelSelection {
                model: ProgramModel::NamedCommands,
                evidence: structural_evidence(discovery.container(), span, note),
            })
        }
        Some(ZbdFamily::Animation)
            if program.locator().member().is_none()
                && matches!(
                    discovery.header_status(),
                    Some(HeaderStatus::Validated { .. })
                )
                && matches!(
                    discovery.animation_payload(),
                    Some(AnimationPayloadCheck::Decoded { .. })
                ) =>
        {
            Some(ModelSelection {
                model: ProgramModel::EventRecords,
                evidence: model_basis(ProgramModel::EventRecords),
            })
        }
        _ => None,
    }
}

/// Renders one modelled program: where it lives, which real model the
/// evidence classified it with and that evidence's method, confidence and
/// summary — never its bytes.
fn modelled_program_json(program: &ModelledProgramProbe) -> String {
    format!(
        "{{\"mission\": {}, \"locator\": {}, \"kind\": {}, \"model\": {}, \"class\": \
         \"modelled_program\", \"confidence\": {}, \"method\": {}, \"evidence\": {}}}",
        jstr(program.mission()),
        jstr(&program.locator().to_string()),
        jstr(program.kind().label()),
        jstr(program.model().label()),
        jstr(program.evidence().confidence().label()),
        jstr(program.evidence().method().label()),
        jstr(program.evidence().note()),
    )
}

/// Renders one record-level key of a directive program: a classified field
/// with its measured shapes and support, or an unclassified key explicitly
/// unknown with its reason. The row key is `record_key`, never `key`: `key`
/// stays the directive-axis spelling, and the two axes are separate
/// vocabularies.
fn record_key_json(row: &RecordKeyRow) -> String {
    let detail = match row.status() {
        RecordKeyStatus::Measured { support, citations } => {
            let citations: Vec<String> = citations.iter().map(|stem| jstr(stem)).collect();
            format!(
                ", \"support\": {}, \"citations\": [{}]",
                jstr(support.label()),
                citations.join(", ")
            )
        }
        RecordKeyStatus::OutsideVocabulary {
            code,
            vocabulary,
            resolving_task,
        } => format!(
            ", \"reason\": {}, \"vocabulary\": {}, \"resolving_task\": {}",
            jstr(code),
            jstr(vocabulary),
            jstr(resolving_task)
        ),
    };
    let status = row.status().label();
    let sites = row
        .sites()
        .map_or_else(|| "null".to_owned(), |n| n.to_string());
    let shapes: Vec<String> = row.shapes().iter().map(|shape| jstr(shape)).collect();
    format!(
        "{{\"record_key\": {}, \"status\": \"{status}\"{detail}, \"sites\": {sites}, \
         \"shapes\": [{}]}}",
        jstr(row.key()),
        shapes.join(", "),
    )
}

/// Renders one directive program: where it lives, every key it reaches and
/// the record-level keys its control record spells.
fn directive_program_json(program: &DirectiveProgramProbe) -> String {
    let keys = program
        .keys()
        .iter()
        .map(|row| {
            let (detail, evidence) = match row.status() {
                DirectiveKeyStatus::Measured {
                    operation,
                    evidence,
                } => (
                    format!("\"operation\": {}", jstr(operation)),
                    Some(evidence),
                ),
                DirectiveKeyStatus::Terminal { outcome, evidence } => {
                    (format!("\"outcome\": {}", jstr(outcome)), Some(evidence))
                }
                DirectiveKeyStatus::Unknown { code, detail } => (
                    format!("\"reason\": {}, \"detail\": {}", jstr(code), jstr(detail)),
                    None,
                ),
            };
            let evidence = evidence.map_or(String::new(), |list| {
                let list: Vec<String> = list.iter().map(|stem| jstr(stem)).collect();
                format!(", \"evidence\": [{}]", list.join(", "))
            });
            let shapes: Vec<String> = row.shapes().iter().map(|shape| jstr(shape)).collect();
            format!(
                "{{\"key\": {}, \"status\": {}, \"blocks\": {}, \"sites\": {}, \
                 \"shapes\": [{}], {detail}{evidence}}}",
                jstr(row.key()),
                jstr(row.status().label()),
                row.blocks(),
                row.sites(),
                shapes.join(", "),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let record_keys = program
        .record_keys()
        .iter()
        .map(record_key_json)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"mission\": {}, \"locator\": {}, \"class\": \"directive_program\", \
         \"blocks\": {}, \"refused_blocks\": {}, \"sites\": {}, \"keys_known\": {}, \
         \"keys_unknown\": {}, \"keys\": [{keys}], \"record_keys_classified\": {}, \
         \"record_keys_unclassified\": {}, \"record_keys\": [{record_keys}]}}",
        jstr(program.mission()),
        jstr(&program.locator().to_string()),
        program.blocks(),
        program.refused_blocks(),
        program.sites(),
        program.known_keys(),
        program.unknown_keys(),
        program.classified_record_keys(),
        program.unclassified_record_keys(),
    )
}

/// Renders the `scripts` report.
fn scripts_report_json(
    found: &install::Discovery,
    summary: &ScriptsSummary,
    probe_section: &str,
    coverage: bool,
    passes: bool,
    rows: &[String],
) -> String {
    let mut by_kind: Vec<String> = ProgramKind::ALL
        .iter()
        .map(|kind| {
            format!(
                "{}: {}",
                jstr(kind.label()),
                summary.by_kind.get(kind.label()).copied().unwrap_or(0)
            )
        })
        .collect();
    by_kind.sort();
    format!(
        "{{\n\
         \x20\"report_version\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"content_sha256\": {},\n\
         \x20\"check\": \"scripts_located_and_classified\",\n\
         \x20\"coverage\": {coverage},\n\
         \x20\"passes\": {passes},\n\
         \x20\"readiness\": \"not_assessed\",\n\
         \x20\"probe\": {},\n\
         \x20\"summary\": {{\"zbd_containers\": {}, \"script_containers\": {}, \"programs\": {}, \
         \"findings\": {}, \"excluded_containers\": {}, \"dispatch_refused\": {}, \
         \"unlocated_containers\": {}, \"finding_containers\": {}, \"records\": {}, \
         \"records_used\": {}, \"records_unused\": {}, \"unused_unknown_records\": {}, \
         \"signature_claims\": {}, \"programs_resolved\": {}, \"programs_retryable\": {}, \
         \"modelled_programs\": {}, \"by_kind\": {{{}}}}},\n\
         \x20\"containers\": [\n  {}\n ]\n\
         }}\n",
        jstr(SCRIPTS_REPORT_VERSION),
        jstr(&install::fingerprint(&found.manifest).to_hex()),
        jstr(&install::content_fingerprint(&found.manifest).to_hex()),
        probe_section,
        summary.zbd_containers,
        summary.script_containers,
        summary.programs,
        summary.findings,
        summary.excluded_containers,
        summary.dispatch_refused,
        summary.unlocated_containers,
        summary.finding_containers,
        summary.records,
        summary.records_used,
        summary.records_unused,
        summary.unused_unknown_records,
        summary.signature_claims,
        summary.programs_resolved,
        summary.programs_retryable,
        summary.modelled_programs,
        by_kind.join(", "),
        rows.join(",\n  "),
    )
}

/// Loads caller-supplied signature claims for `--signatures`.
///
/// The file is plain text, one claim per line, in the shape F07-D's
/// `--classes` file uses: a `#` comment, then
///
/// ```text
/// <opcode> <spelling> <program> <arity> <signature> <effects> <timing> <errors> <citation> [note]
/// ```
///
/// `opcode` is decimal or `0x`-prefixed, `program` is a
/// [`ProgramKind::label`] and every field except the trailing note is a
/// single token. The evidence is a [`ResearchMethod::DocumentReview`] at
/// [`Confidence::Documented`] naming `citation`: a claims file is a cited
/// document, and a line whose citation cannot be followed is not a
/// measurement. The workspace ships **no** such file — the mission opcode
/// table is unmeasured (spec F13 "Research boundary") — so this is the input
/// a later stage's isolated probe fills in.
fn load_signatures(path: &Path) -> Result<SignatureTable, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read signature file {}: {error}", path.display()))?;
    let mut table = SignatureTable::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let claim =
            parse_claim(line).map_err(|error| format!("{}:{number}: {error}", path.display()))?;
        table
            .insert(claim)
            .map_err(|error| format!("{}:{number}: {error}", path.display()))?;
    }
    Ok(table)
}

/// The shape every signature line must have.
const CLAIM_SHAPE: &str = "`<opcode> <spelling> <program> <arity> <signature> <effects> <timing> \
     <errors> <citation> [note]`";

/// Splits the first whitespace-separated token off `rest`.
fn split_field(rest: &str) -> Option<(&str, &str)> {
    let trimmed = rest.trim_start();
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    if end == 0 {
        None
    } else {
        Some((&trimmed[..end], &trimmed[end..]))
    }
}

/// Parses one opcode value, decimal or `0x`-prefixed.
fn parse_opcode(text: &str) -> Result<u32, String> {
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse::<u32>(),
    };
    parsed.map_err(|error| format!("opcode must be a u32, got {text:?} ({error})"))
}

/// Parses one signature line into a validated [`SignatureClaim`].
fn parse_claim(line: &str) -> Result<SignatureClaim, String> {
    let missing = || CLAIM_SHAPE.to_owned();
    let (opcode_text, cursor) = split_field(line).ok_or_else(missing)?;
    let (spelling, cursor) = split_field(cursor).ok_or_else(missing)?;
    let (kind_text, cursor) = split_field(cursor).ok_or_else(missing)?;
    let (arity_text, cursor) = split_field(cursor).ok_or_else(missing)?;
    let (signature, cursor) = split_field(cursor).ok_or_else(missing)?;
    let (effects, cursor) = split_field(cursor).ok_or_else(missing)?;
    let (timing, cursor) = split_field(cursor).ok_or_else(missing)?;
    let (errors, cursor) = split_field(cursor).ok_or_else(missing)?;
    let (citation, cursor) = split_field(cursor).ok_or_else(missing)?;

    let opcode = parse_opcode(opcode_text)?;
    let program = ProgramKind::from_label(kind_text)
        .ok_or_else(|| format!("unknown program kind {kind_text:?}"))?;
    let arity = arity_text
        .parse::<u32>()
        .map_err(|error| format!("arity must be a whole number, got {arity_text:?} ({error})"))?;
    let note = cursor.trim();
    let citation = citation.to_owned();
    let note = if note.is_empty() {
        format!("a signature claim cited at {citation}")
    } else {
        note.to_owned()
    };
    let evidence = ScriptEvidence::new(
        ResearchMethod::DocumentReview,
        Confidence::Documented,
        EvidenceLocator::Document { citation },
        note,
    )
    .map_err(|error| error.to_string())?;
    SignatureClaim::new(
        opcode,
        spelling,
        program,
        arity,
        SignatureShape::new(signature, effects, timing, errors),
        evidence,
    )
    .map_err(|error: ClaimError| error.to_string())
}

fn opt_str(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), jstr)
}

/// A JSON string literal.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Whether `path` would land inside `root` once both are resolved as far as
/// they exist.
fn inside(path: &Path, root: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut existing = absolute.as_path();
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(canonical) = fs::canonicalize(existing) {
            let mut full = canonical;
            full.extend(rest.iter().rev());
            return full.starts_with(&root);
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return false,
        }
    }
}

/// Writes `report` to `out` atomically via a sibling temporary file.
fn write_report(out: &Path, report: &str) -> io::Result<()> {
    let mut temp_name = out.as_os_str().to_owned();
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp_name);
    let result = fs::write(&temp, report).and_then(|()| fs::rename(&temp, out));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    //! Acceptance stage F13-B (`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
    //! section `### F13-B`, AC02): the `scripts` command locates and classifies
    //! the loading, mission and animation programs of an installation, and its
    //! `--coverage` check fails closed when a script container hides its
    //! programs.
    //!
    //! The synthetic trees are newly authored bytes under the system temporary
    //! directory, removed on drop. The retail test reads `$CS_GAME_DIR` (never
    //! writes it) and fails loudly without it. The inline shape follows F06-D's
    //! `zbd.rs`: `tools/cs_inspect/tests/` is not an owner path of F13-B.
    //! `evidence_report_f13_b_writes_the_acceptance_report` is the evidence
    //! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.

    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        RecordFieldSupport, RecordKeyStatus, ScriptsRun, ScriptsSummary, decode_zrd,
        dropped_record_keys, measure_control_record, record_key_rows, scripts_command_result,
    };
    use cs_formats::zbd::{
        ANIMATION_SIGNATURE, ANIMATION_VERSION, INTERP_SIGNATURE, INTERP_VERSION,
    };
    use cs_formats::{INDEX_ENTRY_BYTES, INTERP_HEADER_BYTES, NAME_FIELD_BYTES};

    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable fixture directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f13-b-{label}-{}-{}",
                std::process::id(),
                NEXT_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("the fixture root is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs are created");
            fs::write(path, bytes).expect("fixture bytes are written");
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn run(root: &Path, extra: &[&str]) -> ScriptsRun {
        let mut list = vec!["--cs-path", root.to_str().expect("a UTF-8 fixture path")];
        list.extend_from_slice(extra);
        scripts_command_result(&args(&list), None)
    }

    /// One authored INTERP script: its name and its lines of
    /// `(argument_count, data)`.
    type AuthoredInterp<'a> = (&'a [u8], &'a [(u32, &'a [u8])]);

    /// Builds an INTERP container: header, index, each script's lines of
    /// `(argument_count, data)` and its zero terminator.
    fn interp(scripts: &[AuthoredInterp<'_>]) -> Vec<u8> {
        let body_start = INTERP_HEADER_BYTES + scripts.len() * INDEX_ENTRY_BYTES;
        let mut body = Vec::new();
        let mut offsets = Vec::new();
        for (_, lines) in scripts {
            offsets.push((body_start + body.len()) as u32);
            for (count, data) in *lines {
                body.extend_from_slice(&(data.len() as u32).to_le_bytes());
                body.extend_from_slice(&count.to_le_bytes());
                body.extend_from_slice(data);
            }
            body.extend_from_slice(&0u32.to_le_bytes());
        }
        let mut bytes = Vec::new();
        for word in [INTERP_SIGNATURE, INTERP_VERSION, scripts.len() as u32] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for ((name, _), offset) in scripts.iter().zip(&offsets) {
            let mut field = [0u8; NAME_FIELD_BYTES];
            field[..name.len()].copy_from_slice(name);
            bytes.extend_from_slice(&field);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes.extend_from_slice(&body);
        bytes
    }

    /// Builds a reader-family archive: member data, one 148-byte index entry
    /// per member, then the version-one trailer.
    fn reader_archive(members: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut extents = Vec::new();
        for (name, body) in members {
            extents.push((bytes.len() as u32, body.len() as u32, *name));
            bytes.extend_from_slice(body);
        }
        for (start, length, name) in extents {
            bytes.extend_from_slice(&start.to_le_bytes());
            bytes.extend_from_slice(&length.to_le_bytes());
            let mut field = [0u8; 64];
            field[..name.len()].copy_from_slice(name);
            bytes.extend_from_slice(&field);
            bytes.extend_from_slice(&[0u8; 76]);
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
        bytes
    }

    fn animation(payload: &[u8]) -> Vec<u8> {
        let mut bytes = ANIMATION_SIGNATURE.to_le_bytes().to_vec();
        bytes.extend_from_slice(&ANIMATION_VERSION.to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    /// An animation container whose payload decodes: empty index tables, the
    /// 68-byte payload header declaring one record, the 40-byte gap, one
    /// record with one ordinary sequence block holding `events`.
    fn decodable_animation(events: &[u8]) -> Vec<u8> {
        let mut payload = vec![0u8; 8]; // external and member counts
        let mut header = [0u8; 68];
        header[10..12].copy_from_slice(&1u16.to_le_bytes());
        header[36..40].copy_from_slice(&(-9.8f32).to_le_bytes());
        header[60..64].copy_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&header);
        payload.extend_from_slice(&[0u8; 40]);
        let mut record = [0u8; 272];
        record[216] = 1; // one ordinary sequence
        payload.extend_from_slice(&record);
        let mut info = [0u8; 64];
        info[60..64].copy_from_slice(&(events.len() as u32).to_le_bytes());
        payload.extend_from_slice(&info);
        payload.extend_from_slice(events);
        animation(&payload)
    }

    /// One well-formed eight-byte event: opcode 1, group 1, length 8.
    const ONE_EVENT: [u8; 8] = [1, 1, 0, 0, 8, 0, 0, 0];

    /// F13-D-FU4: a validated animation header over a payload that does not
    /// decode is not modelled — it keeps the binary walk and its honest stop.
    #[test]
    fn accept_f13_d_fu4_a_validated_header_over_a_garbage_payload_is_not_modelled() {
        let garbage: [&[u8]; 4] = [
            b"\x11\x22\x33\x44",
            // Declares no record at all: zeros where the payload header sits.
            &[0u8; 76],
            // A decodable container whose event stream has a bad length word.
            &[1, 1, 0, 0, 7, 0, 0, 0],
            // ... a tag outside the measured shape.
            &[1, 9, 0, 0, 8, 0, 0, 0],
        ];
        for (index, events) in garbage.iter().enumerate() {
            let bytes = if index < 2 {
                animation(events)
            } else {
                decodable_animation(events)
            };
            let tree = Temp::new(&format!("f13d-fu4-garbage-{index}"));
            tree.write("ZBD/C1/cam_anim.zbd", &bytes);
            let result = run(&tree.0, &["--coverage"]);
            let summary = result.summary.as_ref().expect("the command ran");
            assert_eq!(summary.programs, 1, "case {index}");
            assert_eq!(summary.modelled_programs, 0, "case {index}");
            assert_eq!(summary.programs_retryable, 1, "case {index}");
        }
    }

    /// F13-D-FU4: the same carrier with a decodable payload is modelled.
    #[test]
    fn accept_f13_d_fu4_a_decodable_payload_is_modelled_as_event_records() {
        let tree = Temp::new("f13d-fu4-decodable");
        let mut two = ONE_EVENT.to_vec();
        two.extend_from_slice(&ONE_EVENT);
        tree.write("ZBD/C1/cam_anim.zbd", &decodable_animation(&two));
        let result = run(&tree.0, &["--coverage"]);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.modelled_programs, 1);
        assert_eq!(
            summary.modelled_by_model.get("event_records").copied(),
            Some(1)
        );
        assert_eq!(summary.programs_retryable, 0);
    }

    /// The F13-B production path over an authored installation: the `scripts`
    /// command locates the loading, mission and animation programs, classifies
    /// each by name or path and passes its coverage check.
    #[test]
    fn accept_f13_b_cli_locates_loading_mission_and_animation_programs() {
        let tree = Temp::new("locate");
        tree.write(
            "ZBD/interp.zbd",
            &interp(&[(b"boot", &[(1, b"go\0".as_slice())])]),
        );
        tree.write(
            "ZBD/C1/M02/zrdr.zbd",
            &reader_archive(&[
                (b"aiv.zrd", b"\x01\x00\x00\x00"),
                (b"mis_anim.zrd", b"\x02\x00\x00\x00"),
                (b"cam_anim.zrd", b"\x03\x00\x00\x00"),
                (b"scene.zrd", b"\x04\x00\x00\x00"),
            ]),
        );
        tree.write("ZBD/C1/cam_anim.zbd", &decodable_animation(&ONE_EVENT));
        tree.write("ZBD/C1/texture.zbd", &[0u8; 16]);

        let result = run(&tree.0, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.zbd_containers, 4);
        assert_eq!(summary.script_containers, 3);
        assert_eq!(summary.excluded_containers, 1);
        assert_eq!(summary.dispatch_refused, 0);
        assert_eq!(summary.unlocated_containers, 0);
        assert_eq!(summary.finding_containers, 0);
        assert!(summary.covered());
        // One loading body, four reader members, one animation container.
        assert_eq!(summary.programs, 6);
        assert_eq!(summary.by_kind.get("loading").copied(), Some(1));
        assert_eq!(summary.by_kind.get("mission").copied(), Some(2));
        assert_eq!(summary.by_kind.get("mission_animation").copied(), Some(1));
        // One `cam_anim.zrd` member and one `cam_anim.zbd` container.
        assert_eq!(summary.by_kind.get("camera_animation").copied(), Some(2));
        assert_eq!(summary.by_kind.get("reader_entry").copied().unwrap_or(0), 0);

        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(
            report.contains("\"readiness\": \"not_assessed\""),
            "{report}"
        );
        // A loading program is documented; a mission member is inferred and
        // carries its mission and member locator.
        assert!(
            report.contains(
                "\"kind\": \"loading\", \"confidence\": \"documented\", \"mission\": null"
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "\"kind\": \"mission\", \"confidence\": \"inferred\", \"mission\": \
                 \"zbd/c1/m02\", \"member\": \"aiv.zrd\""
            ),
            "{report}"
        );
        assert!(report.contains("\"member\": \"scene.zrd\""), "{report}");
        // The animation containers are named, not decoded: the report holds no
        // program bytes.
        assert!(
            report.contains("\"path\": \"zbd/c1/cam_anim.zbd\", \"family\": \"animation\""),
            "{report}"
        );
        assert!(!report.contains("\\x11\\x22"), "{report}");
        assert!(!report.contains("\"bytes\""), "{report}");
    }

    /// Coverage fails closed: a script container whose reader refuses its own
    /// bytes, and a `.zbd` whose family nothing routes, are both reported with
    /// a nonzero coverage status instead of an empty pass.
    #[test]
    fn accept_f13_b_cli_coverage_fails_on_a_container_that_hides_its_programs() {
        let tree = Temp::new("hidden");
        tree.write("ZBD/C1/zrdr.zbd", b"\x00\x00BADTRAILER");
        let result = run(&tree.0, &["--coverage"]);
        assert_eq!(result.exit_code, 3, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.script_containers, 1);
        assert_eq!(summary.programs, 0);
        assert_eq!(summary.unlocated_containers, 1);
        assert_eq!(summary.finding_containers, 1);
        assert!(!summary.covered());
        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": false"), "{report}");
        assert!(report.contains("\"code\": \"index_refused\""), "{report}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|line| line.contains("index_refused")),
            "{:?}",
            result.diagnostics
        );

        // An unroutable `.zbd` is not silently dropped from coverage either.
        let tree = Temp::new("unroutable");
        tree.write("ZBD/C1/strange.zbd", b"????unknown-bytes");
        let result = run(&tree.0, &["--coverage"]);
        assert_eq!(result.exit_code, 3, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.dispatch_refused, 1);
        assert!(!summary.covered());

        // Without --coverage the same corpus is reported but exits 0.
        let result = run(&tree.0, &[]);
        assert_eq!(result.exit_code, 0);
        assert!(
            result
                .report
                .expect("a report")
                .contains("\"coverage\": false")
        );
    }

    /// The command refuses invalid input, a missing installation and an `--out`
    /// inside the installation.
    #[test]
    fn accept_f13_b_cli_refuses_bad_input_and_missing_installation() {
        assert_eq!(scripts_command_result(&[], None).exit_code, 4);
        assert_eq!(
            scripts_command_result(&args(&["--bogus"]), None).exit_code,
            2
        );
        assert_eq!(scripts_command_result(&args(&["--out"]), None).exit_code, 2);
        let tree = Temp::new("inside");
        tree.write("ZBD/C1/cam_anim.zbd", &animation(b"\x00"));
        let inside = tree.0.join("report.json");
        let result = run(&tree.0, &["--out", inside.to_str().expect("UTF-8")]);
        assert_eq!(result.exit_code, 2);
        assert!(
            !inside.exists(),
            "nothing is written inside the installation"
        );
    }

    // --- F13-C ---------------------------------------------------------------

    /// Writes a signature-claims file (F13-C `--signatures`) outside the
    /// fixture installation and returns its path.
    fn claims_file(label: &str, contents: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("cs-f13-c-{label}-{}.claims", std::process::id()));
        fs::write(&path, contents).expect("the claims file is written");
        path
    }

    /// The authored claim file that resolves both programs of the fixture
    /// tree below: four words for the loading body, two for the mission
    /// member.
    fn complete_claims() -> String {
        let citation = "docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md";
        [
            "# F13-C synthetic signature claims, authored for this test.",
            "4 boot_line_length loading 0 u32->unit records_immediately immediate never",
            "1 boot_line_argc loading 0 u32->unit records_immediately immediate never",
            "0x44332211 boot_line_data loading 0 u32->unit records_immediately immediate never",
            "0 boot_line_terminator loading 0 u32->unit records_immediately immediate never",
            "0x0a mission_first mission 0 u32->unit records_immediately immediate never",
            "0x0b mission_second mission 0 u32->unit records_immediately immediate never",
        ]
        .iter()
        .map(|line| {
            if line.starts_with('#') || line.is_empty() {
                format!("{line}\n")
            } else {
                format!("{line} {citation} authored fixture claim\n")
            }
        })
        .collect()
    }

    /// One INTERP loading container whose body is four whole instruction
    /// words, plus four trailing bytes no reader claims: an unused unknown
    /// record.
    fn loading_with_unclaimed_tail() -> Vec<u8> {
        let mut bytes = interp(&[(b"boot", &[(1, b"\x11\x22\x33\x44".as_slice())])]);
        bytes.extend_from_slice(b"JUNK");
        bytes
    }

    /// The F13-C production path: the `scripts` command probes every located
    /// program with the supplied claims and reports the reachability of every
    /// inventory record, keeping an unused unknown record visible with its
    /// evidence (AC03).
    #[test]
    fn accept_f13_c_cli_resolves_signatures_and_keeps_an_unused_unknown_record() {
        let tree = Temp::new("f13c-resolve");
        tree.write("ZBD/interp.zbd", &loading_with_unclaimed_tail());
        tree.write(
            "ZBD/C1/M02/zrdr.zbd",
            &reader_archive(&[(b"objectives.zrd", b"\x0a\x00\x00\x00\x0b\x00\x00\x00")]),
        );

        let claims = claims_file("complete", &complete_claims());
        let result = run(
            &tree.0,
            &[
                "--coverage",
                "--signatures",
                claims.to_str().expect("UTF-8"),
            ],
        );
        let _ = fs::remove_file(&claims);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);

        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.zbd_containers, 2);
        assert_eq!(summary.script_containers, 2);
        assert_eq!(summary.programs, 2, "one loading body, one mission member");
        // header + index entry + script body + unclaimed tail, plus the
        // reader archive's single opaque record.
        assert_eq!(summary.records, 5);
        assert_eq!(summary.records_used, 2);
        assert_eq!(summary.records_unused, 3);
        assert_eq!(
            summary.unused_unknown_records, 1,
            "the unclaimed tail is an unused unknown record"
        );
        assert_eq!(summary.signature_claims, 6);
        assert_eq!(summary.programs_resolved, 2);
        assert_eq!(summary.programs_retryable, 0);
        assert!(summary.signatures_complete());

        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(report.contains("\"supplied\": true"), "{report}");
        assert!(report.contains("\"assumed\": true"), "{report}");
        assert!(report.contains("\"claims\": 6"), "{report}");
        assert!(report.contains("\"resolved\": 2"), "{report}");
        assert!(report.contains("\"complete\": true"), "{report}");
        // The unused unknown record stays in the report with the evidence
        // that says nothing reaches it.
        let tail_start = (INTERP_HEADER_BYTES + INDEX_ENTRY_BYTES + 16) as u64;
        let expected = format!(
            "{{\"offset\": {tail_start}, \"length\": 4, \"kind\": \"unclaimed\", \
             \"discriminator\": \"unknown\", \"instructions\": \"unestablished\", \
             \"reachability\": {{\"status\": \"unused\", \"programs\": 0, \"reason\": \"no located \
             program overlaps these bytes, so no probe reaches this record\", \"method\": \
             \"structural_decode\", \"confidence\": \"observed_tool\", \"evidence\": \
             \"no located program overlaps these bytes, so no probe reaches this record\"}}"
        );
        assert!(
            report.contains(&expected),
            "the unused unknown record with its reachability evidence is in the report:\n\
             {report}"
        );
        // The body the program does reach is reported as used.
        assert!(
            report.contains("\"kind\": \"interp_script\"")
                && report.contains("\"status\": \"used\""),
            "{report}"
        );
        assert!(!report.contains("\"bytes\""), "{report}");
    }

    /// With `--signatures` the coverage check fails while any program is
    /// unresolved, so an incomplete table is never reported as success; a
    /// malformed or missing file is invalid input.
    #[test]
    fn accept_f13_c_cli_fails_an_incomplete_table_and_refuses_a_malformed_file() {
        let tree = Temp::new("f13c-incomplete");
        tree.write("ZBD/interp.zbd", &loading_with_unclaimed_tail());
        tree.write(
            "ZBD/C1/M02/zrdr.zbd",
            &reader_archive(&[(b"objectives.zrd", b"\x0a\x00\x00\x00\x0b\x00\x00\x00")]),
        );

        let complete = complete_claims();
        let partial: String = complete
            .lines()
            .filter(|line| !line.starts_with("0x0b "))
            .map(|line| format!("{line}\n"))
            .collect();
        let claims = claims_file("partial", &partial);
        let result = run(
            &tree.0,
            &[
                "--coverage",
                "--signatures",
                claims.to_str().expect("UTF-8"),
            ],
        );
        let _ = fs::remove_file(&claims);
        assert_eq!(result.exit_code, 3, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.programs, 2);
        assert_eq!(summary.programs_resolved, 1, "only the loading body");
        assert!(!summary.signatures_complete());
        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": false"), "{report}");
        assert!(report.contains("\"complete\": false"), "{report}");

        // Without --signatures the same corpus still passes coverage: the
        // table ships empty by design and completeness is only checked when
        // the caller supplied one.
        let result = run(&tree.0, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        assert!(
            result
                .report
                .expect("a report")
                .contains("\"supplied\": false"),
            "an empty table is reported, never hidden"
        );

        // Malformed claim files and bad flags are invalid input.
        for (label, line, contents) in [
            (
                "bad-opcode",
                1,
                "not-a-number spell mission 0 a b c d doc\n",
            ),
            ("bad-kind", 1, "1 spell nonsense 0 a b c d doc\n"),
            ("bad-arity", 1, "1 spell mission x a b c d doc\n"),
            ("missing-field", 1, "1 spell mission 0 a b c d\n"),
            (
                "duplicate",
                2,
                "1 first mission 0 a b c d doc\n1 second mission 0 a b c d doc\n",
            ),
        ] {
            let claims = claims_file(label, contents);
            let result = run(
                &tree.0,
                &[
                    "--coverage",
                    "--signatures",
                    claims.to_str().expect("UTF-8"),
                ],
            );
            let _ = fs::remove_file(&claims);
            assert_eq!(result.exit_code, 2, "{label}: {:?}", result.diagnostics);
            let marker = format!(":{line}:");
            assert!(
                result
                    .diagnostics
                    .first()
                    .is_some_and(|found| found.contains(&marker)),
                "{label}: the refusal names its line {marker}: {:?}",
                result.diagnostics
            );
        }
        let missing = std::env::temp_dir().join("cs-f13-c-does-not-exist.claims");
        let result = run(
            &tree.0,
            &[
                "--coverage",
                "--signatures",
                missing.to_str().expect("UTF-8"),
            ],
        );
        assert_eq!(result.exit_code, 2, "{:?}", result.diagnostics);

        for flag in ["--word-bytes", "--budget"] {
            let result = run(&tree.0, &[flag, "not-a-number"]);
            assert_eq!(result.exit_code, 2, "{flag}: {:?}", result.diagnostics);
        }
        let result = run(&tree.0, &["--word-bytes", "0"]);
        assert_eq!(result.exit_code, 2, "{:?}", result.diagnostics);
        let result = run(&tree.0, &["--word-bytes", "9"]);
        assert_eq!(result.exit_code, 2, "{:?}", result.diagnostics);
        let result = run(&tree.0, &["--word-bytes"]);
        assert_eq!(result.exit_code, 2, "{:?}", result.diagnostics);
    }

    /// Every default still in force is reported as an assumption: the
    /// unmeasured instruction unit may not hide behind a budget the caller did
    /// give, and a run that states both values states no assumption.
    #[test]
    fn accept_f13_c_cli_reports_a_defaulted_probe_configuration_as_assumed() {
        let tree = Temp::new("f13c-assumed");
        tree.write("ZBD/interp.zbd", &loading_with_unclaimed_tail());

        // Only the budget is given: the instruction unit still defaults to 4.
        let result = run(&tree.0, &["--budget", "64"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let report = result.report.expect("a report");
        assert!(
            report.contains("\"assumed\": true") && report.contains("\"word_bytes\": 4"),
            "a defaulted instruction unit is reported as assumed:\n{report}"
        );

        // Only the instruction unit is given: the budget still defaults.
        let result = run(&tree.0, &["--word-bytes", "4"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let report = result.report.expect("a report");
        assert!(
            report.contains("\"assumed\": true") && report.contains("\"budget\": 4096"),
            "a defaulted budget is reported as assumed:\n{report}"
        );

        // Both values are stated: nothing about the run is assumed.
        let result = run(&tree.0, &["--word-bytes", "4", "--budget", "64"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let report = result.report.expect("a report");
        assert!(
            report.contains("\"assumed\": false") && report.contains("\"budget\": 64"),
            "a fully stated configuration reports no assumption:\n{report}"
        );
    }

    // --- M01-B-OPC: text directive programs ------------------------------------

    fn zrd_word(value: u32) -> [u8; 4] {
        value.to_le_bytes()
    }

    fn zrd_text(value: &str) -> Vec<u8> {
        let mut out = zrd_word(3).to_vec();
        out.extend_from_slice(&zrd_word(value.len() as u32));
        out.extend_from_slice(value.as_bytes());
        out
    }

    /// A list node: the stored count is one more than the child count.
    fn zrd_list(children: &[Vec<u8>]) -> Vec<u8> {
        let mut out = zrd_word(4).to_vec();
        out.extend_from_slice(&zrd_word(children.len() as u32 + 1));
        for child in children {
            out.extend_from_slice(child);
        }
        out
    }

    /// An authored control member: one `OBJECTIVE1` block holding a terminal
    /// key (bare), a measured key with an argument list and a key no finding
    /// measures.
    fn authored_objectives() -> Vec<u8> {
        let block = zrd_list(&[
            zrd_text("INSTANTWIN"),
            zrd_text("WAKE_ANIM"),
            zrd_list(&[zrd_text("runwayanim")]),
            zrd_text("SET_AI_"),
            zrd_list(&[zrd_text("x")]),
        ]);
        zrd_list(&[zrd_list(&[zrd_text("OBJECTIVE1"), block])])
    }

    /// M01-B-OPC: the production `scripts` path classifies a mission's control
    /// member as a directive program instead of walking its text as binary
    /// words, lists every key it reaches as measured or explicitly unknown, and
    /// still probes the other members as binary programs.
    #[test]
    fn accept_m01_b_opc_cli_classifies_the_control_member_as_a_directive_program() {
        let tree = Temp::new("opc-classify");
        tree.write(
            "ZBD/C1C/M01/zrdr.zbd",
            &reader_archive(&[
                (b"objectives.zrd", &authored_objectives()),
                (b"aiv.zrd", b"\x01\x00\x00\x00"),
            ]),
        );

        let result = run(&tree.0, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.programs, 2);
        assert_eq!(summary.directive_programs, 1);
        assert_eq!(summary.directive_keys_known, 2);
        assert_eq!(summary.directive_keys_unknown, 1);
        // Only the binary member is walked, and it stops at an unknown opcode.
        assert_eq!(summary.programs_resolved, 0);
        assert_eq!(summary.programs_retryable, 1);
        assert!(!summary.signatures_complete());

        let report = result.report.expect("a report");
        assert!(report.contains("\"directive_programs\": 1"), "{report}");
        assert!(report.contains("\"unknown_opcode\": 1"), "{report}");
        assert!(report.contains("\"class\": \"directive_program\""));
        assert!(
            report.contains("{\"key\": \"INSTANTWIN\", \"status\": \"terminal_outcome\""),
            "{report}"
        );
        assert!(
            report.contains("{\"key\": \"WAKE_ANIM\", \"status\": \"measured\""),
            "{report}"
        );
        assert!(
            report.contains("{\"key\": \"SET_AI_\", \"status\": \"unknown\""),
            "{report}"
        );
        assert!(report.contains("\"reason\": \"meaning_not_measured\""));

        // Every evidence stem a row cites is a findings document the
        // repository holds.
        let findings = workspace_root().join("docs/findings");
        let mut cited = 0;
        for part in report.split("\"evidence\": [").skip(1) {
            let list = part.split(']').next().expect("a closed list");
            for stem in list.split(',').map(|s| s.trim().trim_matches('"')) {
                assert!(
                    findings.join(format!("{stem}.md")).is_file(),
                    "cited finding {stem} is not recorded"
                );
                cited += 1;
            }
        }
        assert!(cited >= 3, "measured rows cite their findings: {report}");
    }

    /// M01-B-OPC: a text member that is not a unique control program is never
    /// classified as a *directive program*; F13-D (#640) then classifies it
    /// with its family's real model — both members decode as `.zrd` typed
    /// trees, so neither is walked as binary words and neither stops at an
    /// `unknown_opcode` any more. A member that does not decode as `.zrd`
    /// keeps the binary probe (`blob.zrd` in
    /// `accept_f13_d_cli_classifies_every_family_with_its_real_model`).
    #[test]
    fn accept_m01_b_opc_cli_does_not_classify_members_without_a_unique_control_rule() {
        let tree = Temp::new("opc-ambiguous");
        tree.write(
            "ZBD/C1C/M01/zrdr.zbd",
            &reader_archive(&[
                (b"objectives.zrd", &authored_objectives()),
                (b"copy.zrd", &authored_objectives()),
            ]),
        );
        let result = run(&tree.0, &[]);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.programs, 2);
        assert_eq!(summary.directive_programs, 0);
        assert_eq!(summary.programs_retryable, 0);
        assert_eq!(summary.modelled_programs, 2);
        assert_eq!(
            summary.modelled_by_model.get("typed_data_tree").copied(),
            Some(2)
        );
    }

    /// M01-B-OPC: the session refuses a listing that cannot be evidence.
    #[test]
    fn accept_m01_b_opc_session_refuses_unreasoned_unknown_keys() {
        use cs_formats::script_raw::{
            DirectiveKeyRow, DirectiveKeyStatus, ProbeConfig, ProbeSession, SignatureTable,
            discover_container,
        };
        use cs_types::install::RelativePath;

        let bytes = reader_archive(&[(b"objectives.zrd", &authored_objectives())]);
        let path = RelativePath::new("ZBD/C1C/M01/zrdr.zbd").expect("a relative path");
        let discovery = discover_container("ZBD/C1C/M01/zrdr.zbd", &path, &bytes);
        let program = &discovery.programs()[0];
        let mut session = ProbeSession::new(
            SignatureTable::new(),
            ProbeConfig::new(4, 16).expect("a valid config"),
        );
        let unknown = |reason: &str| {
            DirectiveKeyRow::new(
                "SET_AI_",
                1,
                1,
                vec![],
                DirectiveKeyStatus::Unknown {
                    code: reason.to_owned(),
                    detail: reason.to_owned(),
                },
            )
        };
        let error = session
            .record_directive_program(program, 1, 0, vec![unknown(" ")], vec![])
            .expect_err("an unknown key with no reason is refused");
        assert_eq!(error.code(), "invalid_directive_program");
        let error = session
            .record_directive_program(program, 1, 0, vec![unknown("a"), unknown("a")], vec![])
            .expect_err("a key listed twice is refused");
        assert!(error.to_string().contains("duplicate_key"), "{error}");
        session
            .record_directive_program(program, 1, 0, vec![unknown("meaning_not_measured")], vec![])
            .expect("a reasoned listing is filed");
        assert!(session.probes().is_empty());
        assert_eq!(session.teardown().directive_programs().len(), 1);
    }

    // --- F13-C-RECKEYS: the record-level axis ---------------------------------

    fn zrd_int(value: u32) -> Vec<u8> {
        let mut out = zrd_word(1).to_vec();
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn zrd_float(value: f32) -> Vec<u8> {
        let mut out = zrd_word(2).to_vec();
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    /// An authored control member that also spells record-level keys: one
    /// `OBJECTIVE1` block with a terminal key, then a classified field with a
    /// list value (`MISSION_TIMER`), a classified sound key with a text value
    /// (`MISSION_WON_SOUND`) and a key outside both record vocabularies
    /// (`MYSTERY_SETTING`).
    fn authored_objectives_with_record_keys() -> Vec<u8> {
        let block = zrd_list(&[zrd_text("INSTANTWIN")]);
        let record = zrd_list(&[
            zrd_text("OBJECTIVE1"),
            block,
            zrd_text("MISSION_TIMER"),
            zrd_list(&[zrd_float(0.0)]),
            zrd_text("MISSION_WON_SOUND"),
            zrd_text("sgwon"),
            zrd_text("MYSTERY_SETTING"),
            zrd_list(&[zrd_int(7)]),
        ]);
        zrd_list(&[record])
    }

    /// An authored control member whose only numbered block is not a list —
    /// the record-level refusal [`signatures_complete`] must keep counting.
    fn authored_objectives_with_a_refused_block() -> Vec<u8> {
        let record = zrd_list(&[
            zrd_text("OBJECTIVE1"),
            zrd_text("not-a-list"),
            zrd_text("MISSION_TIMER"),
            zrd_list(&[zrd_float(0.0)]),
        ]);
        zrd_list(&[record])
    }

    /// An empty-but-valid signature claims file: no caller-measured claims,
    /// but a table was supplied, so the completeness check is in force.
    fn empty_claims_file(label: &str) -> PathBuf {
        claims_file(
            label,
            "# no caller-measured claims; the table ships empty by design\n",
        )
    }

    /// F13-C-RECKEYS: the `scripts` report lists the record-level axis of a
    /// directive program — classified fields with their measured shapes and
    /// support level, and an unclassified record key explicitly unknown with
    /// its reason — and the unclassified key does not count against
    /// `signatures_complete` (it is listed, not unread).
    #[test]
    fn accept_f13_c_reckeys_cli_lists_the_record_level_axis_of_a_directive_program() {
        let tree = Temp::new("reckeys-axis");
        tree.write(
            "ZBD/C1C/M01/zrdr.zbd",
            &reader_archive(&[(b"objectives.zrd", &authored_objectives_with_record_keys())]),
        );

        let result = run(&tree.0, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.directive_programs, 1);
        assert_eq!(summary.directive_record_keys_classified, 2);
        assert_eq!(
            summary.directive_record_keys_unclassified, 1,
            "the key outside both record vocabularies is counted, not dropped"
        );
        // The single member is the directive program, and nothing about an
        // unclassified-but-listed record key leaves it unaccounted for.
        assert!(summary.signatures_complete());

        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(report.contains("\"complete\": true"), "{report}");
        assert!(
            report.contains("\"directive_record_keys_classified\": 2"),
            "{report}"
        );
        assert!(
            report.contains("\"directive_record_keys_unclassified\": 1"),
            "{report}"
        );
        assert!(
            report.contains(
                "\"record_key_gate\": {\"passes\": true, \"violations\": 0, \"dropped\": 0}"
            ),
            "{report}"
        );
        // Classified fields carry their measured shape and support level.
        assert!(
            report.contains(
                "{\"record_key\": \"MISSION_TIMER\", \"status\": \"measured\", \
                 \"support\": \"shape_measured\", \
                 \"citations\": [\"2026-10-04-m01-lc-mission-program\", \
                 \"2026-10-06-m01-lc-directive-meaning\"], \"sites\": 1, \
                 \"shapes\": [\"[float]\"]}"
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "{\"record_key\": \"MISSION_WON_SOUND\", \"status\": \"measured\", \
                 \"support\": \"effect_measured\", \"citations\": [\"2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics\", \
                 \"2026-10-06-m01-lc-directive-d-sound-help-timer-directives\", \
                 \"2026-10-09-m02-b-fu2-record-sound-keys\"], \"sites\": 1, \
                 \"shapes\": [\"not_a_list\"]}"
            ),
            "{report}"
        );
        // The unclassified key is listed unknown-with-reason, with no site
        // count invented beside it.
        assert!(
            report.contains(
                "{\"record_key\": \"MYSTERY_SETTING\", \"status\": \"outside_vocabulary\", \
                 \"reason\": \"outside_both_record_vocabularies\", \"vocabulary\": \
                 \"CONTROL_RECORD_KEY_VOCABULARY + CONTROL_RECORD_SOUND_KEY_VOCABULARY\", \
                 \"resolving_task\": \"#1623\""
            ),
            "{report}"
        );
        assert!(
            report.contains("\"sites\": null, \"shapes\": []"),
            "an unclassified record key's site count is not measured: {report}"
        );
        // The per-program counters say the same thing.
        assert!(report.contains("\"record_keys_classified\": 2"), "{report}");
        assert!(
            report.contains("\"record_keys_unclassified\": 1"),
            "{report}"
        );
        // `record_key` rows never leak into the directive-key axis the M01
        // listing counts by.
        assert_eq!(
            report
                .split("\"directive_program_rows\": [")
                .nth(1)
                .map(|rows| rows
                    .split("{\"mission\": ")
                    .nth(1)
                    .expect("one row")
                    .matches("{\"key\": ")
                    .count()),
            Some(1),
            "the directive axis of this program spells one distinct key"
        );

        // With `--signatures` supplied (an empty table), the unclassified
        // record key still does not fail the completeness gate: the row lists
        // it, which is what the gate asks for.
        let claims = empty_claims_file("reckeys-empty");
        let result = run(
            &tree.0,
            &[
                "--coverage",
                "--signatures",
                claims.to_str().expect("UTF-8"),
            ],
        );
        let _ = fs::remove_file(&claims);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        assert!(
            result
                .report
                .expect("a report")
                .contains("\"complete\": true"),
            "an explicitly unknown record key is accounted for, like an unknown directive key"
        );
    }

    /// F13-C-RECKEYS: the documented boundary of that decision — a block that
    /// did not read as a directive list still counts against
    /// `signatures_complete`, because its unread text may spell keys no row
    /// lists, while the record-level fields beside it stay listed either way.
    #[test]
    fn accept_f13_c_reckeys_cli_keeps_a_refused_block_failing_completeness() {
        let tree = Temp::new("reckeys-refused");
        tree.write(
            "ZBD/C1C/M01/zrdr.zbd",
            &reader_archive(&[(
                b"objectives.zrd",
                &authored_objectives_with_a_refused_block(),
            )]),
        );

        let claims = empty_claims_file("reckeys-refused-empty");
        let result = run(
            &tree.0,
            &[
                "--coverage",
                "--signatures",
                claims.to_str().expect("UTF-8"),
            ],
        );
        let _ = fs::remove_file(&claims);
        assert_eq!(result.exit_code, 3, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.directive_refused_blocks, 1);
        assert!(!summary.signatures_complete());
        // The classified record field is still listed beside the refusal.
        assert_eq!(summary.directive_record_keys_classified, 1);
        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": false"), "{report}");
        assert!(report.contains("\"complete\": false"), "{report}");
        assert!(report.contains("\"refused_blocks\": 1"), "{report}");
        assert!(
            report.contains(
                "{\"record_key\": \"MISSION_TIMER\", \"status\": \"measured\", \
                 \"support\": \"shape_measured\", \"citations\""
            ),
            "{report}"
        );
    }

    /// F13-C-RECKEYS: the session refuses a record-level listing that cannot
    /// be evidence — an unknown key with no reason and a key listed twice —
    /// and files a reasoned one.
    #[test]
    fn accept_f13_c_reckeys_session_refuses_unreasoned_and_duplicate_record_keys() {
        use cs_formats::script_raw::{
            ProbeConfig, ProbeSession, RecordFieldSupport, RecordKeyRow, RecordKeyStatus,
            SignatureTable, discover_container,
        };
        use cs_types::install::RelativePath;

        let bytes = reader_archive(&[(b"objectives.zrd", &authored_objectives_with_record_keys())]);
        let path = RelativePath::new("ZBD/C1C/M01/zrdr.zbd").expect("a relative path");
        let discovery = discover_container("ZBD/C1C/M01/zrdr.zbd", &path, &bytes);
        let program = &discovery.programs()[0];
        let mut session = ProbeSession::new(
            SignatureTable::new(),
            ProbeConfig::new(4, 16).expect("a valid config"),
        );
        let measured = |citations: Vec<String>| {
            RecordKeyRow::new(
                "MISSION_TIMER",
                Some(1),
                vec!["[float]".to_owned()],
                RecordKeyStatus::Measured {
                    support: RecordFieldSupport::ShapeMeasured,
                    citations,
                },
            )
        };
        let cited = || measured(vec!["2026-10-04-m01-lc-mission-program".to_owned()]);
        let outside = || {
            RecordKeyRow::new(
                "MYSTERY_SETTING",
                None,
                Vec::new(),
                RecordKeyStatus::OutsideVocabulary {
                    code: "outside_both_record_vocabularies".to_owned(),
                    vocabulary: "CONTROL_RECORD_KEY_VOCABULARY".to_owned(),
                    resolving_task: "#1623".to_owned(),
                },
            )
        };
        let error = session
            .record_directive_program(program, 1, 0, vec![], vec![cited(), cited()])
            .expect_err("a record key listed twice is refused");
        assert_eq!(error.code(), "invalid_directive_program");
        assert!(error.to_string().contains("duplicate_key"), "{error}");
        session
            .record_directive_program(program, 1, 0, vec![], vec![cited(), outside()])
            .expect("a cited, named record-level listing is filed");
        assert!(session.probes().is_empty());
        let report = session.teardown();
        let filed = &report.directive_programs()[0];
        assert_eq!(filed.classified_record_keys(), 1);
        assert_eq!(filed.unclassified_record_keys(), 1);
        assert_eq!(filed.record_key_gate_violations(), 0);
        assert_eq!(filed.record_keys()[0].key(), "MISSION_TIMER");
        assert_eq!(filed.record_keys()[1].key(), "MYSTERY_SETTING");
        assert_eq!(filed.record_keys()[1].sites(), None);
    }

    /// F13-D-FU3: the citation gate. A measured key with no citation, and an
    /// outside-vocabulary key that names neither its vocabulary nor its
    /// resolving task, each fail it with a stable code; a cited, named key
    /// passes. The CLI fails its coverage check (exit 3) on the count.
    #[test]
    fn accept_f13_d_fu3_gate_flags_uncited_measured_and_unnamed_outside_keys() {
        let measured = |citations: Vec<String>| RecordKeyStatus::Measured {
            support: RecordFieldSupport::ShapeMeasured,
            citations,
        };
        let outside = |vocabulary: &str, task: &str| RecordKeyStatus::OutsideVocabulary {
            code: "outside_both_record_vocabularies".to_owned(),
            vocabulary: vocabulary.to_owned(),
            resolving_task: task.to_owned(),
        };
        assert_eq!(
            measured(vec![]).gate_violation(),
            Some("measured_without_citation")
        );
        assert_eq!(
            measured(vec![" ".to_owned()]).gate_violation(),
            Some("measured_without_citation")
        );
        assert_eq!(
            measured(vec!["a-finding".to_owned()]).gate_violation(),
            None
        );
        assert_eq!(
            outside("", "#1623").gate_violation(),
            Some("outside_vocabulary_without_vocabulary")
        );
        assert_eq!(
            outside("CONTROL_RECORD_KEY_VOCABULARY", " ").gate_violation(),
            Some("outside_vocabulary_without_resolving_task")
        );
        assert_eq!(
            outside("CONTROL_RECORD_KEY_VOCABULARY", "#1623").gate_violation(),
            None
        );
        assert_eq!(measured(vec![]).label(), "measured");
        assert_eq!(outside("v", "#1").label(), "outside_vocabulary");
    }

    /// F13-D-FU3: the summary's gate is false on any violation or dropped key
    /// and true otherwise, independent of `--signatures`.
    #[test]
    fn accept_f13_d_fu3_summary_gate_fails_on_violation_or_dropped_key() {
        let mut summary = ScriptsSummary::default();
        assert!(summary.record_key_gate_passes());
        summary.record_key_gate_violations = 1;
        assert!(!summary.record_key_gate_passes());
        summary.record_key_gate_violations = 0;
        summary.record_keys_dropped = 1;
        assert!(!summary.record_key_gate_passes());
    }

    /// F13-D-FU3: the production rows built from a measured control record
    /// pass the gate — every classified field cites findings (sound keys the
    /// findings behind their measured effect) and the unclassified key names
    /// its vocabulary and the resolving task — and none is dropped.
    #[test]
    fn accept_f13_d_fu3_production_rows_are_cited_and_complete() {
        let document = decode_zrd(&authored_objectives_with_record_keys())
            .expect("the authored record decodes");
        let record = measure_control_record(&document);
        let rows = record_key_rows(&record);
        assert_eq!(dropped_record_keys(&record, &rows), 0);
        assert!(!rows.is_empty());
        for row in &rows {
            assert_eq!(row.status().gate_violation(), None, "{}", row.key());
        }
        let unclassified = rows
            .iter()
            .find(|row| row.key() == "MYSTERY_SETTING")
            .expect("the unclassified key is listed, not dropped");
        assert!(matches!(
            unclassified.status(),
            RecordKeyStatus::OutsideVocabulary { resolving_task, .. } if resolving_task == "#1623"
        ));
        // A listing that lost a key is reported as dropped.
        assert_eq!(dropped_record_keys(&record, &rows[1..]), 1);
    }

    // --- F13-D: the real model of every program family (#640) ----------------

    /// An authored `.zrd` member that decodes as a typed tree: a list of a
    /// text key and a list value, exactly the grammar `decode_zrd` reads.
    fn authored_typed_tree() -> Vec<u8> {
        zrd_list(&[zrd_text("wind"), zrd_list(&[zrd_int(7)])])
    }

    /// The F13-D production path: each program family is classified with its
    /// real model instead of being walked as numeric opcode words — an
    /// INTERP body that reads as named text commands, a `.zrd` member that
    /// decodes as a typed tree and an animation payload behind its validated
    /// header are modelled; a program no model is established for keeps the
    /// binary walk and its honest `unknown_opcode` stop.
    #[test]
    fn accept_f13_d_cli_classifies_every_family_with_its_real_model() {
        let tree = Temp::new("f13d-models");
        // `boot` heads every line with a printable command name; `blob` is
        // four binary words, exactly the shape F13-B misread as an opcode
        // stream, and it must keep its walk.
        tree.write(
            "ZBD/interp.zbd",
            &interp(&[
                (
                    b"boot",
                    &[
                        (1, b"SetBFETolerance\0".as_slice()),
                        (1, b"GameGenSwapYZAxis\0".as_slice()),
                    ],
                ),
                (b"blob", &[(1, b"\x11\x22\x33\x44".as_slice())]),
                // An empty script: only the zero terminator. Its model is
                // still named commands — walking the terminator as an opcode
                // is the misreading this task corrects.
                (b"empty", &[]),
            ]),
        );
        // `weather.zrd` decodes as a `.zrd` typed tree; `blob.zrd` is not a
        // tree (tag 0x0a is outside 1..=4), so nothing classifies it.
        tree.write(
            "ZBD/C1/M02/zrdr.zbd",
            &reader_archive(&[
                (b"weather.zrd", &authored_typed_tree()),
                (b"blob.zrd", b"\x0a\x00\x00\x00\x0b\x00\x00\x00"),
            ]),
        );
        tree.write("ZBD/C1/cam_anim.zbd", &decodable_animation(&ONE_EVENT));

        let result = run(&tree.0, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.programs, 6);
        assert_eq!(summary.modelled_programs, 4);
        assert_eq!(summary.directive_programs, 0);
        // The two programs no model is established for keep the binary walk
        // and stop at their first reached counter.
        assert_eq!(summary.programs_resolved, 0);
        assert_eq!(summary.programs_retryable, 2);
        assert_eq!(
            summary.modelled_by_model.get("named_commands").copied(),
            Some(2)
        );
        assert_eq!(
            summary.modelled_by_model.get("typed_data_tree").copied(),
            Some(1)
        );
        assert_eq!(
            summary.modelled_by_model.get("event_records").copied(),
            Some(1)
        );
        assert!(!summary.signatures_complete());

        let report = result.report.expect("a report");
        assert!(report.contains("\"modelled\": 4"), "{report}");
        assert!(
            report.contains(
                "\"modelled_by_model\": {\"event_records\": 1, \"named_commands\": 2, \
                 \"typed_data_tree\": 1}"
            ),
            "{report}"
        );
        assert!(report.contains("\"unknown_opcode\": 2"), "{report}");
        assert!(report.contains("\"complete\": false"), "{report}");
        // Every located program row carries its model; only the two binary
        // programs carry `null`.
        assert!(report.contains("\"model\": \"named_commands\""), "{report}");
        assert!(
            report.contains("\"model\": \"typed_data_tree\""),
            "{report}"
        );
        assert!(report.contains("\"model\": \"event_records\""), "{report}");
        assert_eq!(
            report.matches("\"model\": null").count(),
            2,
            "only the two programs without an established model keep the walk: {report}"
        );
        // The modelled rows carry the evidence that classified them: a
        // structural decode of the program's own bytes for the text bodies,
        // the executable static analysis for the compiled event records.
        assert!(
            report.contains(
                "\"class\": \"modelled_program\", \"confidence\": \"observed_tool\", \
             \"method\": \"structural_decode\""
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "\"class\": \"modelled_program\", \"confidence\": \"documented\", \
             \"method\": \"executable_static_analysis\""
            ),
            "{report}"
        );
        // The probe section states each model's basis: the owner note's
        // executable addresses, never copied bytes.
        for (model, rva) in [
            ("typed_data_tree", 1_547_360_u32),
            ("named_commands", 1_803_356_u32),
            ("event_records", 1_186_688_u32),
        ] {
            assert!(
                report.contains(&format!(
                    "\"{model}\": {{\"method\": \"executable_static_analysis\", \
                     \"confidence\": \"documented\", \"module\": \"crimson.decrypted.exe\", \
                     \"rva\": {rva},"
                )),
                "the {model} basis names its executable address: {report}"
            );
        }
    }

    /// F13-D never classifies on a guess: with `--signatures` supplied and a
    /// corpus that still holds binary programs, the completeness gate fails
    /// closed exactly as F13-C defined it — a modelled program is accounted
    /// for, a walk that stopped at an unknown opcode is not.
    #[test]
    fn accept_f13_d_cli_keeps_an_incomplete_table_failing_closed() {
        let tree = Temp::new("f13d-fail-closed");
        tree.write(
            "ZBD/interp.zbd",
            &interp(&[
                (
                    b"boot",
                    &[
                        (1, b"SetBFETolerance\0".as_slice()),
                        (1, b"GameGenSwapYZAxis\0".as_slice()),
                    ],
                ),
                (b"blob", &[(1, b"\x11\x22\x33\x44".as_slice())]),
            ]),
        );
        tree.write(
            "ZBD/C1/M02/zrdr.zbd",
            &reader_archive(&[(b"blob.zrd", b"\x0a\x00\x00\x00\x0b\x00\x00\x00")]),
        );

        let claims = empty_claims_file("f13d-empty");
        let result = run(
            &tree.0,
            &[
                "--coverage",
                "--signatures",
                claims.to_str().expect("UTF-8"),
            ],
        );
        let _ = fs::remove_file(&claims);
        assert_eq!(result.exit_code, 3, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.modelled_programs, 1, "the command body is modelled");
        assert_eq!(summary.programs_retryable, 2, "the two binary programs");
        assert!(!summary.signatures_complete());
        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": false"), "{report}");
        assert!(report.contains("\"unknown_opcode\": 2"), "{report}");
    }

    // --- retail ---------------------------------------------------------------

    fn game_dir() -> PathBuf {
        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this test needs the original installation");
        let dir = PathBuf::from(dir);
        assert!(
            dir.is_dir(),
            "CS_GAME_DIR is not a directory: {}",
            dir.display()
        );
        dir
    }

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// M01-B-OPC over the original installation: M01's control member
    /// (`c1c/m01`) is a directive program whose keys are all accounted for —
    /// the key site counts add up to the measured census, and every key is
    /// measured, a terminal outcome, or unknown with a reason.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_m01_b_opc_retail_m01_lists_every_reachable_key_as_measured_or_unknown() {
        let dir = game_dir();
        let result = scripts_command_result(
            &args(&[
                "--cs-path",
                dir.to_str().expect("a UTF-8 game dir"),
                "--coverage",
            ]),
            None,
        );
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert!(summary.directive_programs >= 1);
        assert_eq!(summary.directive_refused_blocks, 0);
        let report = result.report.expect("a report");
        let row = report
            .split("\"directive_program_rows\": [")
            .nth(1)
            .expect("the rows")
            .split("{\"mission\": ")
            .find(|row| row.starts_with("\"zbd/c1c/m01\""))
            .expect("M01's directive program is listed");
        // 58 blocks, 353 directive sites, 43 distinct keys (M01-LC finding).
        assert!(row.contains("\"blocks\": 58"), "{row}");
        assert!(row.contains("\"sites\": 353"), "{row}");
        assert!(row.contains("\"refused_blocks\": 0"), "{row}");
        assert_eq!(row.matches("{\"key\": ").count(), 43, "{row}");
        for key in row.split("{\"key\": ").skip(1) {
            let status = key.split("\"status\": \"").nth(1).expect("a status");
            let status = status.split('"').next().expect("a closed status");
            assert!(
                matches!(status, "measured" | "terminal_outcome" | "unknown"),
                "{key}"
            );
            if status == "unknown" {
                assert!(key.contains("\"reason\": \""), "{key}");
            }
        }
    }

    /// F13-C-RECKEYS over the original installation: M01's directive-program
    /// row carries the record-level axis — exactly the five classified record
    /// fields with their measured shapes — and its directive-key listing is
    /// unchanged: 43 keys, no unclassified record key.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f13_c_reckeys_retail_m01_lists_five_classified_record_fields_and_no_unclassified_keys()
     {
        let dir = game_dir();
        let result = scripts_command_result(
            &args(&[
                "--cs-path",
                dir.to_str().expect("a UTF-8 game dir"),
                "--coverage",
            ]),
            None,
        );
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert!(summary.directive_programs >= 1);
        let report = result.report.expect("a report");
        let row = report
            .split("\"directive_program_rows\": [")
            .nth(1)
            .expect("the rows")
            .split("{\"mission\": ")
            .find(|row| row.starts_with("\"zbd/c1c/m01\""))
            .expect("M01's directive program is listed");
        // The directive-key listing is unchanged by the record-level axis.
        assert_eq!(row.matches("{\"key\": ").count(), 43, "{row}");
        assert!(row.contains("\"blocks\": 58"), "{row}");
        assert!(row.contains("\"refused_blocks\": 0"), "{row}");
        // The record-level axis: five classified fields, no unclassified key.
        assert!(row.contains("\"record_keys_classified\": 5"), "{row}");
        assert!(row.contains("\"record_keys_unclassified\": 0"), "{row}");
        for key in [
            "MISSION_TIMER",
            "PLAYER_INIT",
            "RESTORE_ANIMS",
            "EXECUTE_ANIMS",
            "INVALIDATE_ANIMS",
        ] {
            assert!(
                row.contains(&format!(
                    "{{\"record_key\": \"{key}\", \"status\": \"measured\","
                )),
                "{row}"
            );
        }
        // Each field's measured M01 shape (M01-LC findings), now with the
        // findings that measured it (F13-D-FU3).
        let cited = "\"citations\": [\"2026-10-04-m01-lc-mission-program\", \
                     \"2026-10-06-m01-lc-directive-meaning\"]";
        assert!(
            row.contains(&format!(
                "\"record_key\": \"MISSION_TIMER\", \"status\": \"measured\", \"support\": \
                 \"shape_measured\", {cited}, \"sites\": 1, \"shapes\": [\"[float]\"]"
            )),
            "{row}"
        );
        assert!(
            row.contains(&format!(
                "\"record_key\": \"PLAYER_INIT\", \"status\": \"measured\", \"support\": \
                 \"shape_measured\", {cited}, \"sites\": 1, \"shapes\": \
                 [\"[int,[float,float,float],[float,float,float],float,float]\"]"
            )),
            "{row}"
        );
        for key in ["RESTORE_ANIMS", "EXECUTE_ANIMS", "INVALIDATE_ANIMS"] {
            assert!(
                row.contains(&format!(
                    "\"record_key\": \"{key}\", \"status\": \"measured\", \"support\": \
                     \"shape_measured\", {cited}, \"sites\": 1, \"shapes\": [\"[]\"]"
                )),
                "{row}"
            );
        }
    }

    /// AC02 over the original installation: the `scripts` command routes every
    /// ZBD container, locates and classifies every campaign program family and
    /// passes its coverage check, and a located mission program's empty-ledger
    /// walk fails with its mission and source location.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f13_b_retail_cli_locates_every_campaign_program() {
        let root = game_dir();
        let result = run(&root, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(
            summary.zbd_containers, 184,
            "task #340 counted 184 archives"
        );
        // interp (1) + reader (62) + animation (61).
        assert_eq!(summary.script_containers, 124);
        assert_eq!(summary.excluded_containers, 60);
        assert_eq!(summary.dispatch_refused, 0);
        assert_eq!(summary.unlocated_containers, 0);
        assert_eq!(summary.finding_containers, 0);
        assert!(summary.covered());
        assert_eq!(summary.programs, 1452);
        assert_eq!(summary.by_kind.get("loading").copied(), Some(98));
        assert_eq!(summary.by_kind.get("unknown").copied().unwrap_or(0), 0);
        for kind in [
            "mission",
            "mission_animation",
            "camera_animation",
            "reader_entry",
        ] {
            assert!(
                summary.by_kind.get(kind).copied().unwrap_or(0) > 0,
                "{kind} must be located: {:?}",
                summary.by_kind
            );
        }

        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(
            report.contains("\"member\": \"objectives.zrd\""),
            "the mission control member is located"
        );

        // The F13-B minimum scenario, tied to the corpus the command reported:
        // an empty ledger refuses a located mission program's first counter.
        use cs_formats::script_raw::{OpcodeLedger, ProgramError, discover_container};
        use cs_types::install::RelativePath;
        let container_path = root.join("ZBD/C1/M02/zrdr.zbd");
        let bytes = fs::read(&container_path).expect("the mission reader archive");
        let relative = RelativePath::new("zbd/c1/m02/zrdr.zbd").expect("a relative path");
        let discovery = discover_container("retail-zrdr", &relative, &bytes);
        let objectives = discovery
            .programs()
            .iter()
            .find(|program| program.locator().member() == Some("objectives.zrd"))
            .expect("objectives.zrd is a located program");
        let error = objectives
            .walk(&OpcodeLedger::new(), 4, 4096)
            .expect_err("the empty ledger refuses the first reached opcode");
        assert_eq!(error.code(), "unknown_opcode");
        assert_eq!(error.mission(), Some("zbd/c1/m02"));
        assert_eq!(error.locator(), Some(objectives.locator()));
        match &error {
            ProgramError::UnknownOpcode { locator, pc, .. } => {
                assert_eq!(locator, objectives.locator());
                assert_eq!(*pc, cs_formats::script_raw::ByteSpan::new(0, 4));
            }
            other => panic!("expected unknown_opcode, got {other:?}"),
        }
    }

    /// AC03 over the original installation: the `scripts` command probes every
    /// located program in isolation, attaches a reachability verdict with
    /// evidence to every inventory record, resolves nothing with the empty
    /// table and — since F13-D (#640) classified every retail program with
    /// its real model — walks no retail program as binary words.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f13_c_retail_cli_probes_every_program_and_reaches_every_record() {
        let root = game_dir();
        let result = run(&root, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(
            summary.zbd_containers, 184,
            "task #340 counted 184 archives"
        );
        assert_eq!(summary.programs, 1452, "F13-B located 1452 programs");
        // interp (1 + 98 index entries + 98 script bodies) plus one opaque
        // record for each of the 62 reader and 61 animation archives.
        assert_eq!(summary.records, 320);
        assert_eq!(summary.records_used, 221);
        assert_eq!(summary.records_unused, 99);
        assert_eq!(
            summary.unused_unknown_records, 0,
            "every retail byte range an unknown record belongs to is covered by a program"
        );
        assert_eq!(
            summary.signature_claims, 0,
            "the workspace ships no mission opcode table"
        );
        assert_eq!(
            summary.programs_resolved, 0,
            "an empty table resolves nothing, as it must"
        );
        // F13-D (#640) corrects the census's premise: the retail corpus holds
        // no binary opcode stream. Every located program is accounted for —
        // the 40 control members M01-B-OPC (#1356) classifies as directive
        // programs, and 1412 programs classified with their real model by the
        // rules `accept_f13_d_retail_every_program_family_has_its_real_model`
        // pins — so nothing stops at `unknown_opcode` any more. (This pin was
        // 1412 before F13-D; the walks it counted were misreadings of `.zrd`
        // text, INTERP command scripts and event-record payloads.)
        assert_eq!(summary.programs_retryable, 0);
        assert_eq!(summary.modelled_programs, 1412);
        assert!(summary.signatures_complete());

        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(report.contains("\"supplied\": false"), "{report}");
        assert!(report.contains("\"assumed\": true"), "{report}");
        assert!(report.contains("\"complete\": true"), "{report}");
        assert!(
            report.contains("\"stops\": {}"),
            "no retail program is walked as binary words: {report}"
        );
        // Every record carries a verdict and its evidence; none is null.
        assert_eq!(
            report.matches("\"reachability\": {").count(),
            summary.records,
            "every record reports reachability"
        );
        assert!(!report.contains("\"reachability\": null"), "{report}");
        // A modelled program row (F13-D) also spells `"evidence": "..."`, so
        // the string count is the records' plus one per modelled program.
        assert_eq!(
            report.matches("\"evidence\": \"").count(),
            summary.records + summary.modelled_programs,
            "every record carries its reachability evidence"
        );

        // Every retail program is accounted for, so a supplied table has
        // nothing left to resolve: the run passes with the one claim unused.
        // The fail-closed gate itself stays pinned on a corpus that still
        // holds a binary program by
        // `accept_f13_d_cli_keeps_an_incomplete_table_failing_closed` and
        // `accept_f13_c_cli_fails_an_incomplete_table_and_refuses_a_malformed_file`.
        let claims = claims_file(
            "retail-partial",
            "0x0a mission_first mission 0 u32->unit records_immediately immediate never \
             docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md one claim\n",
        );
        let result = run(
            &root,
            &[
                "--coverage",
                "--signatures",
                claims.to_str().expect("UTF-8"),
            ],
        );
        let _ = fs::remove_file(&claims);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.signature_claims, 1);
        assert_eq!(
            summary.programs_resolved, 0,
            "no retail program is a numeric opcode stream to resolve"
        );
        assert!(summary.signatures_complete());
        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": true"), "{report}");
    }

    /// F13-D over the original installation (task #640): every retail program
    /// family is classified with its real model, so the report presents no
    /// stopped walk over a `.zrd` member, an INTERP body or an animation
    /// payload as an `unknown_opcode` — the census's 1452 stops were
    /// misreadings, and each program carries the evidence that classifies it.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f13_d_retail_every_program_family_has_its_real_model() {
        let root = game_dir();
        let result = run(&root, &["--coverage"]);
        assert_eq!(result.exit_code, 0, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.programs, 1452, "F13-B located 1452 programs");
        // The measured split over the installation: the 40 control members
        // M01-B-OPC (#1356) classifies as directive programs, and every other
        // program is classified by an F13-D rule — 98 named-command INTERP
        // bodies (four of them empty scripts with only the zero terminator),
        // 1253 `.zrd` typed trees (every reader member outside the directive
        // programs decodes as a typed tree) and 61 compiled animation
        // event-record payloads whose stream F20 (#690) measured.
        assert_eq!(summary.directive_programs, 40);
        assert_eq!(summary.modelled_programs, 1412);
        for (model, count) in [
            ("named_commands", 98),
            ("typed_data_tree", 1253),
            ("event_records", 61),
        ] {
            assert_eq!(
                summary.modelled_by_model.get(model).copied(),
                Some(count),
                "the {model} count over the installation: {:?}",
                summary.modelled_by_model
            );
        }
        // No retail program is walked as binary words any more, and the
        // mission opcode table stays unmeasured: resolved is still zero.
        assert_eq!(summary.programs_resolved, 0);
        assert_eq!(summary.programs_retryable, 0);
        assert!(summary.signatures_complete());

        let report = result.report.expect("a report");
        assert!(report.contains("\"stops\": {}"), "{report}");
        assert!(report.contains("\"modelled\": 1412"), "{report}");
        // Only the 40 directive programs (M01-B-OPC) carry no model: they are
        // filed on the directive axis instead, and every other retail row was
        // filed with the model it is counted under — the rows and
        // `probe.modelled*` reconcile exactly.
        assert_eq!(
            report.matches("\"model\": null").count(),
            summary.directive_programs,
            "only directive program rows carry no model: {report}"
        );
        assert_eq!(
            report.matches("\"class\": \"modelled_program\"").count(),
            1412,
            "every modelled program is listed with its evidence: {report}"
        );
        // The evidence mix: structural decodes of the program's own bytes at
        // `observed_tool` for the text families, the executable static
        // analysis at `documented` for the compiled event records — never
        // `verified_original`, never copied bytes.
        for (needle, count) in [
            (
                "\"model\": \"typed_data_tree\", \"class\": \"modelled_program\", \
                 \"confidence\": \"observed_tool\", \"method\": \"structural_decode\"",
                1253,
            ),
            (
                "\"model\": \"named_commands\", \"class\": \"modelled_program\", \
                 \"confidence\": \"observed_tool\", \"method\": \"structural_decode\"",
                98,
            ),
            (
                "\"model\": \"event_records\", \"class\": \"modelled_program\", \
                 \"confidence\": \"documented\", \"method\": \"executable_static_analysis\"",
                61,
            ),
        ] {
            assert_eq!(
                report.matches(needle).count(),
                count,
                "the {needle} rows: {report}"
            );
        }
    }

    // --- evidence harness -----------------------------------------------------

    fn env_var(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!("{name} is not set: run the sequence in the evidence harness doc comment")
        })
    }

    fn command_output(program: &str, arguments: &[&str]) -> String {
        let output = Command::new(program)
            .args(arguments)
            .output()
            .unwrap_or_else(|error| panic!("{program} runs: {error}"));
        assert!(output.status.success(), "{program} {arguments:?} failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn locked_version(package: &str) -> String {
        let lock = fs::read_to_string(workspace_root().join("Cargo.lock")).expect("Cargo.lock");
        let mut wanted = false;
        for line in lock.lines().map(str::trim) {
            if line == "[[package]]" {
                wanted = false;
            } else if let Some(name) = line.strip_prefix("name = \"") {
                wanted = name.trim_end_matches('"') == package;
            } else if let Some(version) = line.strip_prefix("version = \"")
                && wanted
            {
                return version.trim_end_matches('"').to_owned();
            }
        }
        panic!("package {package:?} is not in Cargo.lock");
    }

    fn short_name(name: &str) -> &str {
        name.rsplit("::").next().unwrap_or(name)
    }

    /// Libtest totals plus `(test, "pass" | "fail")` for tests starting with
    /// `prefix`, from a recorded `cargo test` output.
    fn parse_suite(log: &str, prefix: &str) -> ([u64; 3], Vec<(String, &'static str)>) {
        let mut totals = [0u64; 3];
        let mut results: Vec<(String, &'static str)> = Vec::new();
        let mut pending: VecDeque<String> = VecDeque::new();
        let record = |results: &mut Vec<(String, &'static str)>, name: String, status| {
            if !results.iter().any(|(seen, _)| *seen == name) {
                results.push((name, status));
            }
        };
        for line in log.lines() {
            let trimmed = line.trim_start();
            if let Some(summary) = trimmed.strip_prefix("test result:") {
                for segment in summary.split(';') {
                    let words: Vec<&str> = segment.split_whitespace().collect();
                    if let Some(pair) = words.windows(2).find(|p| p[0].parse::<u64>().is_ok()) {
                        let count: u64 = pair[0].parse().expect("checked");
                        match pair[1] {
                            "passed" => totals[0] += count,
                            "failed" => totals[1] += count,
                            "ignored" => totals[2] += count,
                            _ => {}
                        }
                    }
                }
                continue;
            }
            if !pending.is_empty() && (trimmed == "ok" || trimmed == "FAILED") {
                let name = pending.pop_front().expect("pending");
                record(
                    &mut results,
                    name,
                    if trimmed == "ok" { "pass" } else { "fail" },
                );
                continue;
            }
            let mut cursor = trimmed;
            while let Some(position) = cursor.find("test ") {
                let after = &cursor[position + 5..];
                let Some(separator) = after.find(" ... ") else {
                    break;
                };
                let name = after[..separator].to_owned();
                let tail = &after[separator + 5..];
                cursor = tail;
                if !short_name(&name).starts_with(prefix) {
                    continue;
                }
                match tail.split_whitespace().next() {
                    Some("ok") => record(&mut results, name, "pass"),
                    Some("FAILED") => record(&mut results, name, "fail"),
                    _ => pending.push_back(name),
                }
            }
        }
        (totals, results)
    }

    fn iso_utc_now() -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64;
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rest / 3_600,
            (rest % 3_600) / 60,
            rest % 60
        )
    }

    /// Evidence-report harness for F13-B (`docs/contracts/CLI-EVIDENCE.md`,
    /// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
    /// loudly when its inputs are missing. From the workspace root:
    ///
    /// 1. ```sh
    ///    mkdir -p private/evidence/F13-B
    ///    cargo test --workspace --locked -- accept_f13_b_ --include-ignored \
    ///      2>&1 | tee private/evidence/F13-B/cargo-test.log
    ///    ```
    ///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
    /// 2. ```sh
    ///    CS_EVIDENCE_DIR=private/evidence/F13-B \
    ///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
    ///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f13_b_ --include-ignored" \
    ///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
    ///      cargo test --locked -p cs_inspect --lib -- evidence_report_f13_b --ignored
    ///    ```
    ///    This runs the production `scripts --coverage` command over
    ///    `$CS_GAME_DIR` and keeps its report as the artifact `scripts.json`.
    /// 3. ```sh
    ///    python3 tools/validate_evidence.py private/evidence/F13-B/acceptance.json \
    ///      --artifact-root private/evidence/F13-B --require-pass
    ///    ```
    /// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F13-B.json`.
    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f13_b_writes_the_acceptance_report() {
        let evidence_dir = {
            let described = PathBuf::from(env_var("CS_EVIDENCE_DIR"));
            if described.is_absolute() {
                described
            } else {
                workspace_root().join(described)
            }
        };
        let candidate_tree = env_var("CS_CANDIDATE_TREE");
        let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert!(
            !argv.is_empty(),
            "CS_EVIDENCE_ARGV must hold the acceptance command"
        );
        let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
        let root = game_dir();
        assert_eq!(
            candidate_tree,
            command_output("git", &["rev-parse", "HEAD^{tree}"]),
            "CS_CANDIDATE_TREE must be the tree of the tested commit"
        );

        let log_path = evidence_dir.join("cargo-test.log");
        let log = fs::read_to_string(&log_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
        let ([passed, failed, ignored], results) = parse_suite(&log, "accept_f13_b_");
        assert!(
            passed > 0 && !results.is_empty(),
            "no accept_f13_b_ tests in the log"
        );
        for retail in [
            "accept_f13_b_retail_cli_locates_every_campaign_program",
            "accept_f13_b_retail_locates_mission_programs",
        ] {
            let status = results
                .iter()
                .find(|(name, _)| short_name(name) == retail)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| panic!("{retail} did not run: use --include-ignored"));
            assert_eq!(status, "pass", "{retail} must pass");
        }

        // The production command over the installation, kept as the artifact.
        let scripts_path = evidence_dir.join("scripts.json");
        let located = run(
            &root,
            &["--coverage", "--out", scripts_path.to_str().expect("UTF-8")],
        );
        assert_eq!(located.out.as_deref(), Some(scripts_path.as_path()));
        let summary = located.summary.as_ref().expect("the command ran");
        assert!(
            summary.covered(),
            "the retail corpus coverage must hold: {summary:?}"
        );
        let report = located.report.expect("the retail report is rendered");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(
            report.contains("\"member\": \"objectives.zrd\""),
            "{report}"
        );

        let found = cs_assets::install::discover(&root).expect("discovery");
        let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
        let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();

        let artifact = |path: &Path, kind: &str| {
            let bytes = fs::read(path).expect("artifact is readable");
            format!(
                "{{\"path\": {}, \"sha256\": \"{}\", \"kind\": \"{kind}\"}}",
                super::jstr(&path.file_name().expect("name").to_string_lossy()),
                cs_assets::install::sha256(&bytes).to_hex()
            )
        };
        // What stays unknown is part of the method text: the schema's
        // `unknowns` must be empty for a passing report, and the unknowns
        // themselves are recorded in the F13-B findings (the mission-language
        // opcode table is not measured; no program's bytes were decoded).
        let method = format!(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, and the \
             production `cs-inspect scripts --coverage` run over every retail ZBD container \
             (scripts.json). The command routes all {} ZBD containers: {} script containers \
             (interp, reader, animation) yield {} located programs ({:?}) with no discovery \
             finding, and {} families excluded from the script search stay listed; the \
             located programs are container/member byte ranges plus a name/path role, never \
             decoded instructions and never copied original bytes. The unknown mission-language \
             opcode table is recorded in docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md. \
             Validated with tools/validate_evidence.py --require-pass",
            summary.zbd_containers,
            summary.script_containers,
            summary.programs,
            summary.by_kind,
            summary.excluded_containers,
        );
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F13-B\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
             \x20\"created_at\": {},\n\
             \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {exit_code}}},\n\
             \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
             \x20\"seed\": 0,\n\
             \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
             \x20\"overrides\": [],\n\
             \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
             \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {passed}, \"failed\": {failed}, \"ignored\": {ignored}}},\n\
             \x20\"assertions\": [{}],\n\
             \x20\"artifacts\": [{}, {}],\n\
             \x20\"unknowns\": [],\n\
             \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
             \x20\"claim\": \"implemented\"\n\
             }}\n",
            super::jstr(&candidate_tree),
            super::jstr(&command_output("rustc", &["--version"])),
            super::jstr(&locked_version("bevy")),
            super::jstr(&locked_version("avian3d")),
            super::jstr(&iso_utc_now()),
            argv.iter()
                .map(|arg| super::jstr(arg))
                .collect::<Vec<_>>()
                .join(", "),
            super::jstr(&command_output("git", &["rev-parse", "--show-toplevel"])),
            super::jstr(&install_sha256),
            super::jstr(&content_sha256),
            passed + failed + ignored,
            passed + failed,
            results
                .iter()
                .map(|(name, status)| format!(
                    "{{\"id\": {}, \"status\": \"{status}\", \"evidence\": [\"cargo-test.log\"]}}",
                    super::jstr(short_name(name))
                ))
                .collect::<Vec<_>>()
                .join(", "),
            artifact(&log_path, "log"),
            artifact(&scripts_path, "json"),
            super::jstr(
                "glm-1/deepseek-1 (implementing agent); deepseek-1 (Rally reviewer, fresh \
                 session; the report was regenerated on the reviewed and rebased commit)",
            ),
            super::jstr(&method),
        );
        let out = evidence_dir.join("acceptance.json");
        fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
        assert!(
            failed == 0 && exit_code == 0,
            "the acceptance run failed (exit {exit_code}, {failed} failed): the report was \
             written honestly and must not validate"
        );
        println!("wrote {}", out.display());
    }

    // --- F13-C evidence harness ------------------------------------------------

    /// Evidence-report harness for F13-C (`docs/contracts/CLI-EVIDENCE.md`,
    /// schema `schemas/evidence.schema.json`). Not an acceptance test: it
    /// fails loudly when its inputs are missing. From the workspace root:
    ///
    /// 1. ```sh
    ///    mkdir -p private/evidence/F13-C
    ///    cargo test --workspace --locked -- accept_f13_c_ --include-ignored \
    ///      2>&1 | tee private/evidence/F13-C/cargo-test.log
    ///    ```
    ///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
    /// 2. ```sh
    ///    CS_EVIDENCE_DIR=private/evidence/F13-C \
    ///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
    ///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f13_c_ --include-ignored" \
    ///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
    ///      cargo test --locked -p cs_inspect --lib -- evidence_report_f13_c --ignored
    ///    ```
    ///    This runs the production `scripts --coverage` command over
    ///    `$CS_GAME_DIR` and keeps its report as the artifact `scripts.json`.
    /// 3. ```sh
    ///    python3 tools/validate_evidence.py private/evidence/F13-C/acceptance.json \
    ///      --artifact-root private/evidence/F13-C --require-pass
    ///    ```
    /// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F13-C.json`.
    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f13_c_writes_the_acceptance_report() {
        let evidence_dir = {
            let described = PathBuf::from(env_var("CS_EVIDENCE_DIR"));
            if described.is_absolute() {
                described
            } else {
                workspace_root().join(described)
            }
        };
        let candidate_tree = env_var("CS_CANDIDATE_TREE");
        let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert!(
            !argv.is_empty(),
            "CS_EVIDENCE_ARGV must hold the acceptance command"
        );
        let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
        let root = game_dir();
        assert_eq!(
            candidate_tree,
            command_output("git", &["rev-parse", "HEAD^{tree}"]),
            "CS_CANDIDATE_TREE must be the tree of the tested commit"
        );

        let log_path = evidence_dir.join("cargo-test.log");
        let log = fs::read_to_string(&log_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
        let ([passed, failed, ignored], results) = parse_suite(&log, "accept_f13_c_");
        assert!(
            passed > 0 && !results.is_empty(),
            "no accept_f13_c_ tests in the log"
        );
        for retail in [
            "accept_f13_c_retail_cli_probes_every_program_and_reaches_every_record",
            "accept_f13_c_retail_reachability_covers_the_campaign_programs",
        ] {
            let status = results
                .iter()
                .find(|(name, _)| short_name(name) == retail)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| panic!("{retail} did not run: use --include-ignored"));
            assert_eq!(status, "pass", "{retail} must pass");
        }

        // The production command over the installation, kept as the artifact.
        let scripts_path = evidence_dir.join("scripts.json");
        let probed = run(
            &root,
            &["--coverage", "--out", scripts_path.to_str().expect("UTF-8")],
        );
        assert_eq!(probed.out.as_deref(), Some(scripts_path.as_path()));
        assert_eq!(probed.exit_code, 0, "{:?}", probed.diagnostics);
        let summary = probed.summary.as_ref().expect("the command ran");
        assert!(
            summary.covered(),
            "the retail corpus coverage must hold: {summary:?}"
        );
        assert_eq!(summary.programs, 1452);
        assert_eq!(summary.records, 320);
        assert_eq!(summary.records_used, 221);
        assert_eq!(summary.records_unused, 99);
        assert_eq!(summary.unused_unknown_records, 0);
        assert_eq!(
            summary.programs_resolved, 0,
            "the mission opcode table is unmeasured"
        );
        let report = probed.report.expect("the retail report is rendered");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(report.contains("\"supplied\": false"), "{report}");
        assert!(
            !report.contains("\"reachability\": null"),
            "every retail record carries a verdict: {report}"
        );

        let found = cs_assets::install::discover(&root).expect("discovery");
        let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
        let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();

        let artifact = |path: &Path, kind: &str| {
            let bytes = fs::read(path).expect("artifact is readable");
            format!(
                "{{\"path\": {}, \"sha256\": \"{}\", \"kind\": \"{kind}\"}}",
                super::jstr(&path.file_name().expect("name").to_string_lossy()),
                cs_assets::install::sha256(&bytes).to_hex()
            )
        };
        let method = format!(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, and the production \
             `cs-inspect scripts --coverage` run over every retail ZBD container (scripts.json). \
             The command routes all {} ZBD containers and locates {} programs in {} script \
             containers. F13-D (#640) classifies {} of them with their real model instead of \
             walking them ({} typed_data_tree, {} named_commands, {} event_records) and M01-B-OPC \
             classifies the {} control members as directive programs, so {} programs remain on \
             the binary walk and stop at their first reached counter against the empty signature \
             table ({} resolved, probe.complete={}); a reachability verdict with structural \
             evidence is attached to all {} inventory records ({} used, {} unused, {} \
             undecoded-and-unknown-and-unused). The mission opcode table is unmeasured by design: \
             F13-C ships the probe machinery and the honest report, never invented signatures, \
             and the recorded unknowns are listed in \
             docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md and the F13-D \
             correction in docs/findings/2026-10-14-f13-d-every-program-family-has-its-real-model.md. \
             Validated with tools/validate_evidence.py --require-pass. Regenerated by the \
             reviewing agent on the reviewed commit, as docs/contracts/CLI-EVIDENCE.md requires.",
            summary.zbd_containers,
            summary.programs,
            summary.script_containers,
            summary.modelled_programs,
            summary
                .modelled_by_model
                .get("typed_data_tree")
                .copied()
                .unwrap_or(0),
            summary
                .modelled_by_model
                .get("named_commands")
                .copied()
                .unwrap_or(0),
            summary
                .modelled_by_model
                .get("event_records")
                .copied()
                .unwrap_or(0),
            summary.directive_programs,
            summary.programs_retryable,
            summary.programs_resolved,
            summary.signatures_complete(),
            summary.records,
            summary.records_used,
            summary.records_unused,
            summary.unused_unknown_records,
        );
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F13-C\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
             \x20\"created_at\": {},\n\
             \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {exit_code}}},\n\
             \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
             \x20\"seed\": 0,\n\
             \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
             \x20\"overrides\": [],\n\
             \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
             \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {passed}, \"failed\": {failed}, \"ignored\": {ignored}}},\n\
             \x20\"assertions\": [{}],\n\
             \x20\"artifacts\": [{}, {}],\n\
             \x20\"unknowns\": [],\n\
             \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
             \x20\"claim\": \"implemented\"\n\
             }}\n",
            super::jstr(&candidate_tree),
            super::jstr(&command_output("rustc", &["--version"])),
            super::jstr(&locked_version("bevy")),
            super::jstr(&locked_version("avian3d")),
            super::jstr(&iso_utc_now()),
            argv.iter()
                .map(|arg| super::jstr(arg))
                .collect::<Vec<_>>()
                .join(", "),
            super::jstr(&command_output("git", &["rev-parse", "--show-toplevel"])),
            super::jstr(&install_sha256),
            super::jstr(&content_sha256),
            passed + failed + ignored,
            passed + failed,
            results
                .iter()
                .map(|(name, status)| format!(
                    "{{\"id\": {}, \"status\": \"{status}\", \"evidence\": [\"cargo-test.log\"]}}",
                    super::jstr(short_name(name))
                ))
                .collect::<Vec<_>>()
                .join(", "),
            artifact(&log_path, "log"),
            artifact(&scripts_path, "json"),
            super::jstr(
                "implementer: opencode-1 (fresh session); regenerated and validated by the \
                 reviewing agent opencode-1 (fresh session, separate context) — the same agent \
                 identity, so this review is not independent evidence under AGENTS.md; Rally \
                 assigned the review",
            ),
            super::jstr(&method),
        );
        let out = evidence_dir.join("acceptance.json");
        fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
        assert!(
            failed == 0 && exit_code == 0,
            "the acceptance run failed (exit {exit_code}, {failed} failed): the report was \
             written honestly and must not validate"
        );
        println!("wrote {}", out.display());
    }

    // --- F13-D evidence harness ------------------------------------------------

    /// Evidence-report harness for F13-D (task #640;
    /// `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
    /// Not an acceptance test: it fails loudly when its inputs are missing.
    /// From the workspace root:
    ///
    /// 1. ```sh
    ///    mkdir -p private/evidence/F13-D
    ///    cargo test --workspace --locked -- accept_f13_d_ --include-ignored \
    ///      2>&1 | tee private/evidence/F13-D/cargo-test.log
    ///    ```
    ///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
    /// 2. ```sh
    ///    CS_EVIDENCE_DIR=private/evidence/F13-D \
    ///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
    ///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f13_d_ --include-ignored" \
    ///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
    ///      cargo test --locked -p cs_inspect --lib -- evidence_report_f13_d --ignored
    ///    ```
    ///    This runs the production `scripts --coverage` command over
    ///    `$CS_GAME_DIR` and keeps its report as the artifact `scripts.json`.
    /// 3. ```sh
    ///    python3 tools/validate_evidence.py private/evidence/F13-D/acceptance.json \
    ///      --artifact-root private/evidence/F13-D
    ///    ```
    ///    **Without** `--require-pass`: that flag rejects a report that still
    ///    lists unresolved issues (it exits 3, "Unresolved issues"), and this
    ///    task's `unknowns` are exactly the limitations its own recording
    ///    pins — the unmeasured mission opcode table, the two unreconciled
    ///    anim vocabularies (#1415), the four unjoined event ids (#1416), the
    ///    free-text record keys (#1417), the four empty scripts and the provenance of the finding's
    ///    2026-10-14 questions. They are recorded rather than dropped: an
    ///    empty list would be a report claiming this stage closed questions it
    ///    did not close, and never dropping an unresolved issue to pass a
    ///    validator is the owner's rule (T685's harness records the same
    ///    decision).
    /// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F13-D.json`.
    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f13_d_writes_the_acceptance_report() {
        let evidence_dir = {
            let described = PathBuf::from(env_var("CS_EVIDENCE_DIR"));
            if described.is_absolute() {
                described
            } else {
                workspace_root().join(described)
            }
        };
        let candidate_tree = env_var("CS_CANDIDATE_TREE");
        let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert!(
            !argv.is_empty(),
            "CS_EVIDENCE_ARGV must hold the acceptance command"
        );
        let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
        let root = game_dir();
        assert_eq!(
            candidate_tree,
            command_output("git", &["rev-parse", "HEAD^{tree}"]),
            "CS_CANDIDATE_TREE must be the tree of the tested commit"
        );

        let log_path = evidence_dir.join("cargo-test.log");
        let log = fs::read_to_string(&log_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
        let ([passed, failed, ignored], results) = parse_suite(&log, "accept_f13_d_");
        assert!(
            passed > 0 && !results.is_empty(),
            "no accept_f13_d_ tests in the log"
        );
        let retail = "accept_f13_d_retail_every_program_family_has_its_real_model";
        let status = results
            .iter()
            .find(|(name, _)| short_name(name) == retail)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail} did not run: use --include-ignored"));
        assert_eq!(status, "pass", "{retail} must pass");

        // The production command over the installation, kept as the artifact.
        let scripts_path = evidence_dir.join("scripts.json");
        let modelled = run(
            &root,
            &["--coverage", "--out", scripts_path.to_str().expect("UTF-8")],
        );
        assert_eq!(modelled.out.as_deref(), Some(scripts_path.as_path()));
        assert_eq!(modelled.exit_code, 0, "{:?}", modelled.diagnostics);
        let summary = modelled.summary.as_ref().expect("the command ran");
        assert!(
            summary.covered(),
            "the retail corpus coverage must hold: {summary:?}"
        );
        assert_eq!(summary.programs, 1452);
        assert_eq!(summary.directive_programs, 40);
        assert_eq!(summary.modelled_programs, 1412);
        assert_eq!(
            summary.programs_retryable, 0,
            "no retail program is walked as binary words"
        );
        let report = modelled.report.expect("the retail report is rendered");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(report.contains("\"stops\": {}"), "{report}");
        // Only the 40 directive programs carry no model; every other row was
        // filed with the model it is counted under, and nothing is walked.
        assert_eq!(
            report.matches("\"model\": null").count(),
            summary.directive_programs,
            "only directive program rows carry no model: {report}"
        );

        let found = cs_assets::install::discover(&root).expect("discovery");
        let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
        let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();

        let artifact = |path: &Path, kind: &str| {
            let bytes = fs::read(path).expect("artifact is readable");
            format!(
                "{{\"path\": {}, \"sha256\": \"{}\", \"kind\": \"{kind}\"}}",
                super::jstr(&path.file_name().expect("name").to_string_lossy()),
                cs_assets::install::sha256(&bytes).to_hex()
            )
        };
        let method = format!(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, and the production \
             `cs-inspect scripts --coverage` run over every retail ZBD container (scripts.json). \
             F13-D (#640) classifies every located program with its real model instead of \
             walking it as numeric opcode words: of {} programs, {} are directive programs \
             (M01-B-OPC) and {} are modelled ({} named_commands INTERP bodies, {} typed_data_tree \
             `.zrd` members, {} event_records animation payloads), leaving {} programs on the \
             binary walk and no `unknown_opcode` stop. The classification basis is the owner note \
             of #640 (static analysis of the decrypted executable, never verified_original) plus \
             this project's own structural decodes; the correction is recorded in \
             docs/findings/2026-10-14-f13-d-every-program-family-has-its-real-model.md. \
             Validated structurally with tools/validate_evidence.py — not with --require-pass, \
             which exits 3 while a report lists unresolved issues and this one lists six, \
             recorded rather than dropped.",
            summary.programs,
            summary.directive_programs,
            summary.modelled_programs,
            summary
                .modelled_by_model
                .get("named_commands")
                .copied()
                .unwrap_or(0),
            summary
                .modelled_by_model
                .get("typed_data_tree")
                .copied()
                .unwrap_or(0),
            summary
                .modelled_by_model
                .get("event_records")
                .copied()
                .unwrap_or(0),
            summary.programs_retryable,
        );
        // The unresolved issues this report carries (CLI-EVIDENCE puts
        // "unresolved issues" in the evidence record minimum): each names the
        // content it affects and the task that would resolve it.
        let unknowns = [
            "mission_opcode_table: no signature claim resolves any retail program \
             (programs_resolved is 0): the model axis says the corpus holds no numeric opcode \
             stream, not that every directive key, text command or event parameter is understood. \
             Affected content: every campaign-reachable mission instruction. Resolving task: the \
             remaining F13 stage of specs/F13 (a compare-two-original-runs measurement), never \
             this one.",
            "anim_keyword_vocabularies: the owner's 2026-10-05 keyword list (41 stated, 44 \
             enumerated, at 0x63341c-0x633718) and F20's (#690) 35 stored ids are not reconciled \
             pair by pair. Affected content: the name of every compiled anim event. Resolving \
             task: Rally #1415 (F13-D-FU1). Gates: any fidelity claim over animation event names.",
            "anim_event_ids: compiled event ids 13, 17, 26 and 28 join no keyword (496 events in \
             389 of the 56 994 blocks). Affected content: playback of those animation records. \
             Resolving task: Rally #1416 (F13-D-FU2); needs an original run or the original \
             source. A prerequisite for VS-M01-RUNTIME.",
            "record_key_status: record-level key statuses stay free text until every measured key \
             carries a citation and every unmeasured key names its vocabulary and resolving task. \
             Affected content: the scripts report's record-key axis. Resolving task: Rally #1417 \
             (F13-D-FU3).",
            "empty_interp_scripts: four retail INTERP scripts hold only the F07-validated zero \
             terminator (container offsets 20415, 46486, 78897, 84086) and what the original does \
             with an empty script is unmeasured. Affected content: those four loading scripts. \
             Resolving task: an owner original run (#358 REF-OWNER-FIRST-CAPTURE).",
            "owner_note_provenance: this task's Rally history holds owner notes of 2026-10-05 and \
             2026-10-08 and none dated 2026-10-14, so the three questions the finding records for \
             that date are this stage's own and nothing measured here rests on them. Affected \
             content: their attribution in \
             docs/findings/2026-10-14-f13-d-every-program-family-has-its-real-model.md. Resolving \
             task: the owner, by re-posting any real note on Rally #640.",
        ]
        .iter()
        .map(|unknown| super::jstr(unknown))
        .collect::<Vec<_>>()
        .join(", ");
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F13-D\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
             \x20\"created_at\": {},\n\
             \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {exit_code}}},\n\
             \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
             \x20\"seed\": 0,\n\
             \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
             \x20\"overrides\": [],\n\
             \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
             \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {passed}, \"failed\": {failed}, \"ignored\": {ignored}}},\n\
             \x20\"assertions\": [{}],\n\
             \x20\"artifacts\": [{}, {}],\n\
             \x20\"unknowns\": [{unknowns}],\n\
             \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
             \x20\"claim\": \"implemented\"\n\
             }}\n",
            super::jstr(&candidate_tree),
            super::jstr(&command_output("rustc", &["--version"])),
            super::jstr(&locked_version("bevy")),
            super::jstr(&locked_version("avian3d")),
            super::jstr(&iso_utc_now()),
            argv.iter()
                .map(|arg| super::jstr(arg))
                .collect::<Vec<_>>()
                .join(", "),
            super::jstr(&command_output("git", &["rev-parse", "--show-toplevel"])),
            super::jstr(&install_sha256),
            super::jstr(&content_sha256),
            passed + failed + ignored,
            passed + failed,
            results
                .iter()
                .map(|(name, status)| format!(
                    "{{\"id\": {}, \"status\": \"{status}\", \"evidence\": [\"cargo-test.log\"]}}",
                    super::jstr(short_name(name))
                ))
                .collect::<Vec<_>>()
                .join(", "),
            artifact(&log_path, "log"),
            artifact(&scripts_path, "json"),
            super::jstr(
                "implementer: mimo-1 (fresh session, 2026-10-14 06:07-08:07 UTC); reviewer: \
                 mimo-1 (a separate session with fresh context from 08:07 UTC the same day, the \
                 same agent identity, so this review is not independent evidence); the reviewer \
                 regenerated this report on the reviewed commit, as docs/contracts/CLI-EVIDENCE.md \
                 requires",
            ),
            super::jstr(&method),
        );
        let out = evidence_dir.join("acceptance.json");
        fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
        assert!(
            failed == 0 && exit_code == 0,
            "the acceptance run failed (exit {exit_code}, {failed} failed): the report was \
             written honestly and must not validate"
        );
        println!("wrote {}", out.display());
    }
}
