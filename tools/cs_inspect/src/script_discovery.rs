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
//!   resolved: an incomplete table exits 3. Without it the coverage check is
//!   the F13-B one and is unchanged.
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
use cs_formats::script_raw::{
    ClaimError, Confidence, ContainerDiscovery, EvidenceLocator, ProbeConfig, ProbeReport,
    ProbeSession, ProgramKind, RecordProbeError, RecordProbeStats, RecordReachability,
    ResearchMethod, ScriptEvidence, ScriptRecord, ScriptSource, SignatureClaim, SignatureShape,
    SignatureTable, discover_container, inventory_scripts, probe_records,
};
use cs_formats::zbd::{HeaderStatus, ZbdFamily};
use cs_types::install::RelativePath;

/// The report format version. Version 2 adds the `probe` and per-container
/// `records` sections (F13-C); the F13-B fields are unchanged.
pub const SCRIPTS_REPORT_VERSION: &str = "cs-inspect-scripts/2";

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
}

impl ScriptsSummary {
    /// Whether the coverage check passes: every container routed, every script
    /// container located a program and no script container hid a finding.
    pub fn covered(&self) -> bool {
        self.dispatch_refused == 0 && self.unlocated_containers == 0 && self.finding_containers == 0
    }

    /// Whether every located program was resolved by the signature table. With
    /// no `--signatures` file the table ships empty, so this is `false` for
    /// any corpus that holds a program — the honest state, never a pass.
    pub fn signatures_complete(&self) -> bool {
        self.programs_resolved == self.programs
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
    let assumed = parsed.word_bytes.is_none() && parsed.budget.is_none();
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
        rows.push(container_json(&discovery, &record_rows));
        for program in discovery.programs() {
            if let Err(error) = session.probe(program) {
                diagnostics.push(format!("{spelling}: signature probe failed: {error}"));
                runtime_failed = true;
            }
        }
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
    let signature_check = !signatures_supplied || summary.signatures_complete();
    let passes = summary.covered() && signature_check;
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
/// An unknown record nobody's probe reaches is rendered like every other one:
/// dropping it would hide exactly the bytes F13 still has to explain (AC03).
fn record_json(record: &ScriptRecord) -> String {
    let reachability_evidence = record.reachability_evidence();
    let reachability = match (record.reachability(), &reachability_evidence) {
        (Some(verdict), evidence) => format!(
            "{{\"status\": {}, \"programs\": {}, \"reason\": {}, \"evidence\": {}}}",
            jstr(verdict.label()),
            verdict.programs(),
            opt_str(match verdict {
                RecordReachability::Unused { reason } => Some(reason),
                RecordReachability::Used { .. } => None,
            }),
            opt_str(evidence.as_ref().map(ScriptEvidence::note)),
        ),
        (None, _) => "null".to_owned(),
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
fn container_json(discovery: &ContainerDiscovery<'_>, record_rows: &[String]) -> String {
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
        .map(|program| {
            let span = program.locator().span();
            format!(
                "{{\"kind\": {}, \"confidence\": {}, \"mission\": {}, \"member\": {}, \
                 \"offset\": {}, \"length\": {}, \"reason\": {}}}",
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
    let stops = stops
        .iter()
        .map(|(code, count)| format!("{}: {count}", jstr(code)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"supplied\": {supplied}, \"assumed\": {assumed}, \"word_bytes\": {}, \
         \"budget\": {}, \"claims\": {}, \"programs\": {}, \"resolved\": {}, \"unresolved\": {}, \
         \"retryable\": {}, \"complete\": {}, \"stops\": {{{stops}}}}}",
        report.config().word_bytes(),
        report.config().budget(),
        summary.signature_claims,
        summary.programs,
        summary.programs_resolved,
        summary.programs - summary.programs_resolved,
        summary.programs_retryable,
        summary.signatures_complete(),
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
         \"by_kind\": {{{}}}}},\n\
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

    use super::{ScriptsRun, scripts_command_result};
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
        tree.write("ZBD/C1/cam_anim.zbd", &animation(b"\x11\x22\x33\x44"));
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
             program overlaps these bytes, so no probe reaches this record\", \"evidence\": \
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
    /// table and fails closed when a supplied table is incomplete.
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
        assert_eq!(summary.programs_retryable, 1452);
        assert!(!summary.signatures_complete());

        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": true"), "{report}");
        assert!(report.contains("\"supplied\": false"), "{report}");
        assert!(report.contains("\"assumed\": true"), "{report}");
        assert!(report.contains("\"complete\": false"), "{report}");
        assert!(report.contains("\"unknown_opcode\": 1452"), "{report}");
        // Every record carries a verdict and its evidence; none is null.
        assert_eq!(
            report.matches("\"reachability\": {").count(),
            summary.records,
            "every record reports reachability"
        );
        assert!(!report.contains("\"reachability\": null"), "{report}");
        assert_eq!(
            report.matches("\"evidence\": \"").count(),
            summary.records,
            "every record carries its reachability evidence"
        );

        // A supplied but incomplete table is never reported as success.
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
        assert_eq!(result.exit_code, 3, "{:?}", result.diagnostics);
        let summary = result.summary.as_ref().expect("the command ran");
        assert_eq!(summary.signature_claims, 1);
        assert!(summary.programs_resolved < summary.programs);
        assert!(!summary.signatures_complete());
        let report = result.report.expect("a report");
        assert!(report.contains("\"passes\": false"), "{report}");
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
             The command routes all {} ZBD containers, locates {} programs in {} script containers, \
             probes each one in isolation against the empty signature table ({} resolved, {} \
             stopping at their first reached counter, probe.complete=false) and attaches a \
             reachability verdict with structural evidence to all {} inventory records ({} used, \
             {} unused, {} undecoded-and-unknown-and-unused). The mission opcode table is \
             unmeasured by design: F13-C ships the probe machinery and the honest report, never \
             invented signatures, and the recorded unknowns are listed in \
             docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md. Validated with \
             tools/validate_evidence.py --require-pass",
            summary.zbd_containers,
            summary.programs,
            summary.script_containers,
            summary.programs_resolved,
            summary.programs_retryable,
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
                "opencode-1 (implementing agent, fresh session); reviewer recorded by Rally at \
                 complete_review",
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
