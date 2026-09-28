//! The `interp` command (F07-B and F07-C): decode and validate one INTERP
//! loading-script container, report its lossless tokens and the findings the
//! decoder retained, and with `--plan` read the same container as a *loading
//! plan* — classified against a command table, resolved through a content
//! session, with every failure naming its source offset and the world it
//! affects.
//!
//! ```text
//! cs-inspect interp --file <path> [--out <file>] [--raw]
//! cs-inspect interp --file <path> --plan [--commands <file>] [--out <file>]
//!     [--cs-path <dir>] [--world <group>]
//! ```
//!
//! The file is read as-is and handed to `cs_formats::decode_interp`, the
//! validating decoder of `specs/F07-interp-loading-script-container.md`
//! (`### F07-B`). The JSON report names the container, its header, and for
//! every script its origin (index position, index-entry offset, script
//! offset, extent and terminator), its lines and each line's arguments as
//! `(offset, bytes)` pairs — the arguments stay bytes, because no encoding is
//! established for names or arguments, so a name or argument is rendered as
//! its length plus a lossless hex form rather than as guessed text.
//!
//! `--raw` adds the untouched F07-A records (`read_interp`: the name field
//! with its padding, the `argument_count` word and the argument bytes
//! verbatim) next to the decoded tokens, which is what a researcher needs to
//! see both views of the same line at once.
//!
//! `--plan` runs stage `### F07-C`: `cs_formats::plan_interp_loading`
//! classifies every line and `cs_content::loading::resolve_loading_plan`
//! resolves the registered commands. **The workspace ships no command
//! registrations** — which commands load resources is F07-D's measurement —
//! so without `--commands` every line is reported `unclassified` and the
//! command exits 3. That is the honest result, not a gap: a plan that guessed
//! which commands load what would be a fabricated loading state.
//!
//! `--commands <file>` supplies the registrations from a researcher's own
//! table, so a corpus can be audited without editing code. The file is one
//! rule per line, `#` comments and blank lines ignored:
//!
//! ```text
//! # <spelling> <ns-arg> <path-arg> [<variant-arg|->] <literal|composed> <status> <source...>
//! LoadGameGen world 2 3 literal observed_tool docs/findings/<file>.md
//! ```
//!
//! The spelling is matched byte-for-byte against a line's head token; the
//! argument numbers are positions counted from that token (0 is the head
//! itself and is refused). `status` is an evidence status from
//! `cs_types::evidence::ClaimStatus`; `verified_original` is refused there and
//! here, because a table may not award a status only fingerprinted evidence
//! can. `literal` means the arguments spell the key; `composed` means the
//! original engine assembles it later, which is recorded as a dynamic lookup
//! and never resolved.
//!
//! `--cs-path` (or `CS_GAME_DIR`) plus `--world` mount the installation into
//! one content session so the plan's literal keys are resolved through the
//! VFS. Without them the plan is still built and every registered command is
//! reported as unresolved for want of a session — never as loaded. The session
//! is closed before the command returns, on every path.
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` the container
//! decoded and reported, and with `--plan` the plan is complete; `2` invalid
//! input (no `--file`, an unreadable path, a path that is not a file, a
//! malformed command table); `3` the bytes are not a valid INTERP container,
//! they decoded with findings, or the loading plan is incomplete — all
//! reported anomalies rather than clean passes; `1` a runtime failure. A
//! decode failure is never reported as success, and an incomplete plan never
//! exits 0.
//!
//! This command never exits `4`. Unlike `inventory`/`resolve`/`audit`, it does
//! not *require* an installation: `--plan` without `--cs-path` and without
//! `CS_GAME_DIR` still classifies the container and reports every registered
//! command as `no_session`, because the classification is worth having without
//! an installation and a missing one is recorded in the report rather than
//! thrown away. The exit code is decided by the plan's completeness, which
//! that report says is false.
//!
//! This command is the F07-B/F07-C *consumer*: it makes the decoder and the
//! plan reachable outside the library crates. It executes nothing. A command
//! it reports as `resolved` is one the VFS answered for a key this table
//! claims; whether the original engine would have loaded it is F07-D's
//! question.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install::{self, DiscoveryError};
use cs_assets::vfs::{ContentSession, SessionBuilder, SessionError};
use cs_content::loading::{DependencyState, LoadingError, LoadingPlanReport, resolve_loading_plan};
use cs_formats::{
    ClassifiedOpcode, DecodedInterp, InterpError, KeyArguments, KeySpelling, LoadCommand,
    LoadCommandTable, OpcodeAudit, OpcodeClass, OpcodeClassTable, ParseContext,
    audit_interp_opcodes, decode_interp, plan_interp_loading_classified, read_interp,
};
use cs_types::asset_id::{ResolveContext, WorldGroup};
use cs_types::evidence::ClaimStatus;

/// The report format version.
pub const INTERP_REPORT_VERSION: &str = "cs-inspect-interp/1";

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE).
const EXIT_INVALID_INPUT: u8 = 2;

/// Exit code for a failed validation or a reported anomaly.
const EXIT_FAILED_VALIDATION: u8 = 3;

/// Exit code for a runtime failure.
const EXIT_RUNTIME: u8 = 1;

/// Why the `interp` command could not produce a report.
#[derive(Debug)]
pub enum InterpCommandError {
    /// The command line was malformed.
    Usage(String),
    /// The selected path could not be read.
    Read {
        /// The requested path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The selected path is not a regular file.
    NotAFile(PathBuf),
    /// The report could not be written.
    Output {
        /// The requested path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The bytes are not a valid INTERP container, or are not the version
    /// this stage documents.
    Container(InterpError),
    /// `--commands` was given but the file could not be read.
    Commands {
        /// The table path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// A line of the command table was refused, or the table as a whole was.
    CommandsRefused {
        /// The table path.
        path: PathBuf,
        /// 1-based line number of the offending rule, or `0` when the whole
        /// table was refused (it is applied all or nothing, so no single line
        /// is at fault on its own).
        line: usize,
        /// The rule as it was read, or `<table>` for a whole-table refusal.
        rule: String,
        /// Why it was refused.
        reason: String,
    },
    /// `--classes` was given but the file could not be read.
    Classes {
        /// The classification path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// A line of the classification was refused, or the classification as a
    /// whole was.
    ClassesRefused {
        /// The classification path.
        path: PathBuf,
        /// 1-based line number of the offending entry, or `0` when the whole
        /// classification was refused (it is applied all or nothing, so no
        /// single line is at fault on its own).
        line: usize,
        /// The entry as it was read, or `<classification>` for a whole-set
        /// refusal.
        rule: String,
        /// Why it was refused.
        reason: String,
    },
    /// Discovery refused the installation.
    Discovery(DiscoveryError),
    /// A mount of the session was refused. The builder is dropped, which
    /// releases every mount that did succeed.
    Session(SessionError),
    /// `--world` was not a valid spelling.
    World(String),
    /// The plan could not be built at all.
    Plan(LoadingError),
}

impl fmt::Display for InterpCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::Read { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::NotAFile(path) => {
                write!(f, "{} is not a regular file", path.display())
            }
            Self::Output { path, source } => {
                write!(f, "cannot write report to {}: {source}", path.display())
            }
            Self::Container(error) => {
                // The code travels with the message so a caller can match the
                // diagnostic on the failure class rather than on wording.
                write!(f, "interp[{}]: {error}", error.code())
            }
            Self::Commands { path, source } => {
                write!(f, "cannot read command table {}: {source}", path.display())
            }
            Self::CommandsRefused {
                path,
                line,
                rule,
                reason,
            } if *line == 0 => write!(
                f,
                "command table {} is refused as a whole ({reason}); it is applied all or nothing, \
                 so no line of it was applied",
                path.display()
            ),
            Self::CommandsRefused {
                path,
                line,
                rule,
                reason,
            } => write!(
                f,
                "command table {} line {line} ({rule:?}) is refused: {reason}",
                path.display()
            ),
            Self::Classes { path, source } => {
                write!(f, "cannot read classification {}: {source}", path.display())
            }
            Self::ClassesRefused {
                path,
                line,
                rule,
                reason,
            } if *line == 0 => write!(
                f,
                "classification {} is refused as a whole ({reason}); it is applied all or \
                 nothing, so no entry of it was applied",
                path.display()
            ),
            Self::ClassesRefused {
                path,
                line,
                rule,
                reason,
            } => write!(
                f,
                "classification {} line {line} ({rule:?}) is refused: {reason}",
                path.display()
            ),
            Self::Discovery(error) => write!(f, "{error}"),
            Self::Session(error) => write!(f, "{error}"),
            Self::World(message) => write!(f, "{message}"),
            Self::Plan(error) => write!(f, "interp[{}]: {error}", error.code()),
        }
    }
}

impl std::error::Error for InterpCommandError {}

/// Parsed `interp` arguments.
#[derive(Debug, Default)]
struct InterpArgs {
    file: Option<PathBuf>,
    out: Option<PathBuf>,
    raw: bool,
    plan: bool,
    commands: Option<PathBuf>,
    classes: Option<PathBuf>,
    cs_path: Option<PathBuf>,
    world: Option<String>,
}

fn parse_interp_args(args: &[String]) -> Result<InterpArgs, InterpCommandError> {
    let mut parsed = InterpArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        match flag {
            "--raw" => {
                parsed.raw = true;
                continue;
            }
            "--plan" => {
                parsed.plan = true;
                continue;
            }
            "--file" | "--out" | "--commands" | "--classes" | "--cs-path" | "--world" => {}
            other => {
                return Err(InterpCommandError::Usage(format!(
                    "cs-inspect interp: unsupported argument {other:?}; expected --file, --out, \
                     --raw, --plan, --commands, --classes, --cs-path or --world"
                )));
            }
        }
        let Some(value) = cursor.next() else {
            return Err(InterpCommandError::Usage(format!(
                "cs-inspect interp: {flag} needs a value"
            )));
        };
        match flag {
            "--file" => parsed.file = Some(PathBuf::from(value)),
            "--out" => parsed.out = Some(PathBuf::from(value)),
            "--commands" => parsed.commands = Some(PathBuf::from(value)),
            "--classes" => parsed.classes = Some(PathBuf::from(value)),
            "--cs-path" => parsed.cs_path = Some(PathBuf::from(value)),
            _ => parsed.world = Some(value.clone()),
        }
    }
    // The two tables answer the same question at different widths: `--commands`
    // is the F07-C loading registry, `--classes` the F07-D classification that
    // contains it. Supplying both would be two sources of truth for one line.
    if parsed.commands.is_some() && parsed.classes.is_some() {
        return Err(InterpCommandError::Usage(
            "cs-inspect interp: --commands and --classes are mutually exclusive; --classes \
             subsumes --commands (a `loading` entry carries the same rule)"
                .to_owned(),
        ));
    }
    Ok(parsed)
}

/// Runs the `interp` command and returns its exit code.
pub fn interp_command(args: &[String]) -> ExitCode {
    let run = interp_command_result(args);
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote interp report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// Everything one `interp` run produced.
#[derive(Debug)]
pub struct InterpRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The JSON report, when the container decoded.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
}

impl InterpRun {
    fn failed(exit_code: u8, error: &InterpCommandError) -> Self {
        Self::failed_with(exit_code, error, Vec::new())
    }

    /// A run that failed *after* something was already observed. The earlier
    /// observations — the decoder's findings, the note about a missing command
    /// table — stay on stderr: a later failure does not un-happen them, and a
    /// refused command table must not swallow the findings of the container it
    /// was going to plan.
    fn failed_with(exit_code: u8, error: &InterpCommandError, earlier: Vec<String>) -> Self {
        let mut diagnostics = earlier;
        diagnostics.push(error.to_string());
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics,
        }
    }
}

/// The body of [`interp_command`].
///
/// The container label is the file name, so a diagnostic names the file the
/// bytes came from without leaking a path from the original installation into
/// the report.
pub fn interp_command_result(args: &[String]) -> InterpRun {
    let parsed = match parse_interp_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return InterpRun::failed(EXIT_INVALID_INPUT, &error),
    };
    let Some(ref path) = parsed.file else {
        return InterpRun::failed(
            EXIT_INVALID_INPUT,
            &InterpCommandError::Usage(
                "cs-inspect interp: --file <path> is required; it must be an INTERP \
                 container, for example one exported with `cs-inspect resolve \
                 --export-dir`"
                    .to_owned(),
            ),
        );
    };
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) => {
            return InterpRun::failed(
                EXIT_INVALID_INPUT,
                &InterpCommandError::Read {
                    path: path.clone(),
                    source,
                },
            );
        }
    };
    if !metadata.is_file() {
        return InterpRun::failed(
            EXIT_INVALID_INPUT,
            &InterpCommandError::NotAFile(path.clone()),
        );
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(source) => {
            return InterpRun::failed(
                EXIT_RUNTIME,
                &InterpCommandError::Read {
                    path: path.clone(),
                    source,
                },
            );
        }
    };
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let mut context = ParseContext::with_defaults(label.clone());
    let decoded = match decode_interp(&mut context, &bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            return InterpRun::failed(
                EXIT_FAILED_VALIDATION,
                &InterpCommandError::Container(error),
            );
        }
    };
    // `--raw` re-reads the same bytes through the F07-A reader. It is a
    // second view of one container, not a second source of truth: if the two
    // views ever disagree the report says so instead of preferring one.
    let raw = if parsed.raw {
        match read_interp(&mut ParseContext::with_defaults(label.clone()), &bytes) {
            Ok(raw) => Some(raw),
            Err(error) => {
                return InterpRun::failed(EXIT_RUNTIME, &InterpCommandError::Container(error));
            }
        }
    } else {
        None
    };

    let mut diagnostics: Vec<String> = decoded
        .findings()
        .iter()
        .map(|finding| format!("{label}: {finding}"))
        .collect();

    // `--plan` runs the F07-C path. It is a second, additive report field, not
    // a different command: the same decoded container is classified against
    // the caller's command table and resolved in a session, and an incomplete
    // plan is an anomaly, so it never exits 0.
    let plan = if parsed.plan {
        match build_plan(&parsed, &decoded, &label, &mut diagnostics) {
            Ok(plan) => Some(plan),
            // Whatever the decoder already reported stays on stderr: a plan
            // that could not be built does not make the container's findings
            // disappear.
            Err(failure) => {
                return InterpRun::failed_with(failure.0, &failure.1, diagnostics);
            }
        }
    } else {
        None
    };
    if let Some(plan) = &plan {
        for failure in plan.report().failures() {
            diagnostics.push(failure.to_string());
        }
        if !plan.audit.is_complete() {
            diagnostics.push(format!(
                "{label}: {} of {} distinct opcode heads are unclassified (F07-D audit)",
                plan.audit.unknown_heads(),
                plan.audit.distinct_heads(),
            ));
        }
    }

    let report = interp_report_json(&label, &decoded, raw.as_ref(), plan.as_ref());
    // Findings are reported, not fatal on their own: a container that decodes
    // but leaves unclaimed bytes is an anomaly the owner has to see, and the
    // exit code says so instead of reporting a clean pass. An incomplete
    // loading plan is the same kind of anomaly.
    let plan_incomplete = plan
        .as_ref()
        .map(|plan| !plan.report().is_complete() || !plan.audit.is_complete())
        .unwrap_or(false);
    let exit_code = if !decoded.findings().is_empty() || plan_incomplete {
        EXIT_FAILED_VALIDATION
    } else {
        0
    };
    let out = match &parsed.out {
        Some(out) => {
            if let Err(source) = write_report(out, &report) {
                let error = InterpCommandError::Output {
                    path: out.clone(),
                    source,
                };
                diagnostics.push(error.to_string());
                return InterpRun {
                    exit_code: EXIT_RUNTIME,
                    report: None,
                    out: None,
                    diagnostics,
                };
            }
            Some(out.clone())
        }
        None => None,
    };
    InterpRun {
        exit_code,
        report: Some(report),
        out,
        diagnostics,
    }
}

/// A built loading plan and the session it was resolved in.
///
/// The session is held so it is closed exactly once, on every path out of the
/// command — including the ones that fail after the plan was built — which is
/// the teardown the report depends on: a report whose session was never closed
/// would keep its mounts alive for the rest of the process.
pub struct PlanRun<'a> {
    report: LoadingPlanReport,
    session: Option<ContentSession>,
    plan: cs_formats::InterpLoadPlan<'a>,
    /// The audit over every distinct head of the same container, against the
    /// same classification the plan was built from.
    audit: OpcodeAudit,
}

impl PlanRun<'_> {
    fn report(&self) -> &LoadingPlanReport {
        &self.report
    }
}

impl Drop for PlanRun<'_> {
    fn drop(&mut self) {
        // `ContentSession::close` consumes the session and reports the mounts
        // it released; the report is already rendered by then, so nothing is
        // lost and nothing is left mounted.
        let _ = self.session.take().map(ContentSession::close);
    }
}

/// Builds the loading plan for `--plan`.
///
/// The order is table, then installation, then plan, so a bad table or a bad
/// installation is refused before any line is classified. The session is
/// opened only when an installation was selected; without one the plan is still
/// built and every registered command is reported unresolved, which is
/// recorded rather than treated as success.
fn build_plan<'a>(
    parsed: &InterpArgs,
    decoded: &'a DecodedInterp<'a>,
    label: &str,
    diagnostics: &mut Vec<String>,
) -> Result<PlanRun<'a>, Box<(u8, InterpCommandError)>> {
    let classes = if let Some(path) = &parsed.classes {
        match read_class_table(path) {
            Ok(classes) => classes,
            Err(error) => return Err(Box::new((EXIT_INVALID_INPUT, error))),
        }
    } else if let Some(path) = &parsed.commands {
        match read_command_table(path) {
            Ok(table) => OpcodeClassTable::from_load_commands(table.commands()),
            Err(error) => return Err(Box::new((EXIT_INVALID_INPUT, error))),
        }
    } else {
        OpcodeClassTable::new()
    };
    if classes.is_empty() && parsed.classes.is_none() && parsed.commands.is_none() {
        diagnostics.push(format!(
            "{label}: no classification or command table was given, so every head is \
             unclassified: which commands load resources is unmeasured (F07-D)"
        ));
    }

    let cs_path = parsed.cs_path.clone().or_else(|| {
        std::env::var_os("CS_GAME_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let mut session = None;
    if let Some(cs_path) = cs_path {
        let found = match install::discover(&cs_path) {
            Ok(found) => found,
            Err(error) => {
                return Err(Box::new((
                    EXIT_RUNTIME,
                    InterpCommandError::Discovery(error),
                )));
            }
        };
        let mut context = ResolveContext::new(install::fingerprint(&found.manifest));
        if let Some(world) = &parsed.world {
            let group = WorldGroup::new(world).map_err(|error| {
                (
                    EXIT_INVALID_INPUT,
                    InterpCommandError::World(format!("invalid --world {world:?}: {error}")),
                )
            })?;
            context = context.with_world_group(group);
        }
        let mut builder = SessionBuilder::new(context);
        // Dropping the builder on a refusal releases every mount that did
        // succeed, so a partial session is never left behind.
        if let Err(error) = builder.mount_installation(&cs_path, &found.diagnosis) {
            return Err(Box::new((EXIT_RUNTIME, InterpCommandError::Session(error))));
        }
        session = Some(builder.open());
    }

    let plan = plan_interp_loading_classified(decoded, &classes);
    let audit = audit_interp_opcodes(decoded, &classes);
    let report = match resolve_loading_plan(session.as_ref(), decoded, &plan) {
        Ok(report) => report,
        Err(error) => {
            // The session is released before the error leaves, on every path.
            if let Some(session) = session {
                session.close();
            }
            return Err(Box::new((EXIT_RUNTIME, InterpCommandError::Plan(error))));
        }
    };
    // From here the session belongs to `PlanRun`, whose `Drop` closes it.
    Ok(PlanRun {
        report,
        session,
        plan,
        audit,
    })
}

/// Reads a command table: one rule per line, `#` comments and blanks ignored.
///
/// ```text
/// <spelling> <ns-arg> <path-arg> [<variant-arg|->] <literal|composed> <status> <source...>
/// ```
///
/// The whole line is whitespace-split and the source may hold spaces, so the
/// first six fields are the rule and the rest is the provenance. A rule that
/// does not parse, names a position that is not a number or carries an unknown
/// status is a usage error naming *its* line. A rule the **table** refuses (a
/// duplicate spelling, a key argument at position 0, an empty source) is
/// reported against the whole table instead, because the table is applied all
/// or nothing: a table is never partially applied, so there is no single line
/// that caused the refusal on its own.
fn read_command_table(path: &Path) -> Result<LoadCommandTable, InterpCommandError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) => {
            return Err(InterpCommandError::Commands {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut table = LoadCommandTable::new();
    let mut rules = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let rule = line.trim();
        if rule.is_empty() || rule.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = rule.split_whitespace().collect();
        let refused = |reason: String| InterpCommandError::CommandsRefused {
            path: path.to_path_buf(),
            line: number + 1,
            rule: rule.to_owned(),
            reason,
        };
        if fields.len() < 6 {
            return Err(refused(format!(
                "expected <spelling> <ns-arg> <path-arg> [<variant-arg|->] \
                 <literal|composed> <status> <source>, found {} field(s)",
                fields.len()
            )));
        }
        let position = |field: &str, what: &str| {
            field.parse::<usize>().map_err(|error| {
                refused(format!(
                    "the {what} argument {field:?} is not a position: {error}"
                ))
            })
        };
        let variant = match fields[3] {
            "-" => None,
            field => Some(position(field, "variant")?),
        };
        let spelling_kind = match fields[4] {
            "literal" => KeySpelling::Literal,
            "composed" => KeySpelling::Composed,
            other => {
                return Err(refused(format!(
                    "the key spelling kind must be `literal` or `composed`, found {other:?}"
                )));
            }
        };
        let status = match parse_claim_status(fields[5]) {
            Some(status) => status,
            None => return Err(refused(unknown_status_message(fields[5]))),
        };
        rules.push(LoadCommand {
            spelling: fields[0].as_bytes().to_vec(),
            arguments: KeyArguments {
                namespace: position(fields[1], "namespace")?,
                path: position(fields[2], "path")?,
                variant,
            },
            spelling_kind,
            status,
            source: fields[6..].join(" "),
        });
    }
    // `extend` applies every rule or none, so a refused table never yields a
    // partially classified plan.
    if let Err(error) = table.extend(rules) {
        return Err(InterpCommandError::CommandsRefused {
            path: path.to_path_buf(),
            line: 0,
            rule: "<table>".to_owned(),
            reason: error.to_string(),
        });
    }
    Ok(table)
}

/// The evidence status one field names, or `None` for anything else.
///
/// `verified_original` is deliberately absent: only a fingerprinted evidence
/// record may award it, never a table a caller wrote.
fn parse_claim_status(field: &str) -> Option<ClaimStatus> {
    match field {
        "documented" => Some(ClaimStatus::Documented),
        "observed_tool" => Some(ClaimStatus::ObservedTool),
        "inferred" => Some(ClaimStatus::Inferred),
        "designed" => Some(ClaimStatus::Designed),
        "unknown" => Some(ClaimStatus::Unknown),
        _ => None,
    }
}

/// The one wording every table uses to refuse an unknown evidence status.
fn unknown_status_message(field: &str) -> String {
    format!(
        "{field:?} is not an evidence status; expected one of documented, observed_tool, \
         inferred, designed, unknown (verified_original may only be awarded by fingerprinted \
         evidence, never by a table)"
    )
}

/// Reads an opcode classification: one entry per line, `#` comments and blanks
/// ignored.
///
/// ```text
/// loading     <spelling> <ns-arg> <path-arg> [<variant-arg|->] <literal|composed> <status> <source...>
/// unsupported <spelling> <status> <source...>
/// behavior    <spelling> <status> <source...>
/// ```
///
/// A `loading` entry is a command-table rule with its class in front; the other
/// classes carry only a spelling and a provenance. The whole classification is
/// applied all or nothing, so an entry the table itself refuses (a duplicate
/// spelling, a key argument at position 0, an empty source) is reported against
/// the classification rather than against one line.
fn read_class_table(path: &Path) -> Result<OpcodeClassTable, InterpCommandError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) => {
            return Err(InterpCommandError::Classes {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut opcodes = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let entry = line.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = entry.split_whitespace().collect();
        let refused = |reason: String| InterpCommandError::ClassesRefused {
            path: path.to_path_buf(),
            line: number + 1,
            rule: entry.to_owned(),
            reason,
        };
        let class = fields[0];
        let position = |field: &str, what: &str| {
            field.parse::<usize>().map_err(|error| {
                refused(format!(
                    "the {what} argument {field:?} is not a position: {error}"
                ))
            })
        };
        let opcode = match class {
            "loading" => {
                if fields.len() < 7 {
                    return Err(refused(format!(
                        "a loading entry needs <spelling> <ns-arg> <path-arg> \
                         [<variant-arg|->] <literal|composed> <status> <source>, found {} field(s)",
                        fields.len()
                    )));
                }
                let variant = match fields[4] {
                    "-" => None,
                    field => Some(position(field, "variant")?),
                };
                let spelling_kind = match fields[5] {
                    "literal" => KeySpelling::Literal,
                    "composed" => KeySpelling::Composed,
                    other => {
                        return Err(refused(format!(
                            "the key spelling kind must be `literal` or `composed`, found {other:?}"
                        )));
                    }
                };
                let status = match parse_claim_status(fields[6]) {
                    Some(status) => status,
                    None => return Err(refused(unknown_status_message(fields[6]))),
                };
                ClassifiedOpcode {
                    spelling: fields[1].as_bytes().to_vec(),
                    class: OpcodeClass::Loading {
                        arguments: KeyArguments {
                            namespace: position(fields[2], "namespace")?,
                            path: position(fields[3], "path")?,
                            variant,
                        },
                        spelling_kind,
                    },
                    status,
                    source: fields[7..].join(" "),
                }
            }
            "unsupported" | "behavior" => {
                if fields.len() < 3 {
                    return Err(refused(format!(
                        "a {class} entry needs <spelling> <status> <source>, found {} field(s)",
                        fields.len()
                    )));
                }
                let status = match parse_claim_status(fields[2]) {
                    Some(status) => status,
                    None => return Err(refused(unknown_status_message(fields[2]))),
                };
                ClassifiedOpcode {
                    spelling: fields[1].as_bytes().to_vec(),
                    class: if class == "unsupported" {
                        OpcodeClass::Unsupported
                    } else {
                        OpcodeClass::Behavior
                    },
                    status,
                    source: fields[3..].join(" "),
                }
            }
            other => {
                return Err(refused(format!(
                    "the class must be `loading`, `unsupported` or `behavior`, found {other:?}"
                )));
            }
        };
        opcodes.push(opcode);
    }
    // `extend` applies every entry or none, so a refused classification never
    // yields a partially classified plan.
    let mut table = OpcodeClassTable::new();
    if let Err(error) = table.extend(opcodes) {
        return Err(InterpCommandError::ClassesRefused {
            path: path.to_path_buf(),
            line: 0,
            rule: "<classification>".to_owned(),
            reason: error.to_string(),
        });
    }
    Ok(table)
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

/// Renders the decode report.
///
/// Every byte string is a JSON object with its length and its lowercase hex,
/// never a decoded string: no encoding is established for names or arguments
/// (`specs/F07-*.md` non-negotiable #1), so a report that printed them as text
/// would be asserting an encoding this stage has not measured.
pub fn interp_report_json(
    label: &str,
    decoded: &DecodedInterp<'_>,
    raw: Option<&cs_formats::InterpFile<'_>>,
    plan: Option<&PlanRun<'_>>,
) -> String {
    let scripts: Vec<String> = decoded
        .scripts()
        .iter()
        .map(|script| {
            let lines: Vec<String> = script
                .lines()
                .iter()
                .enumerate()
                .map(|(line, decoded_line)| {
                    let tokens: Vec<String> = decoded_line
                        .tokens()
                        .iter()
                        .map(|token| {
                            format!(
                                "{{\"offset\": {}, \"bytes\": {}}}",
                                token.offset(),
                                jbytes(token.bytes())
                            )
                        })
                        .collect();
                    let mut row = format!(
                        "{{\"position\": {}, \"offset\": {}, \"data_offset\": {}, \"size\": {}, \
                         \"argument_count\": {}, \"token_count\": {}, \"tokens\": [{}]",
                        line,
                        decoded_line.offset(),
                        decoded_line.data_offset(),
                        decoded_line.size(),
                        decoded_line.argument_count(),
                        decoded_line.len(),
                        tokens.join(", ")
                    );
                    if let Some(head) = decoded_line.head() {
                        row.push_str(&format!(", \"head_offset\": {}", head.offset()));
                    }
                    if raw.is_some() {
                        // The unvalidated F07-A record of the same line, so
                        // both views of one line sit side by side.
                        row.push_str(&format!(
                            ", \"raw\": {{\"offset\": {}, \"size\": {}, \"argument_count\": {}, \
                             \"data\": {}}}",
                            decoded_line.raw().offset,
                            decoded_line.raw().size,
                            decoded_line.raw().argument_count,
                            jbytes(decoded_line.raw().data),
                        ));
                    }
                    row.push('}');
                    row
                })
                .collect();
            format!(
                "{{\"index\": {}, \"entry_offset\": {}, \"script_offset\": {}, \"limit\": {}, \
                 \"terminator_offset\": {}, \"end\": {}, \"line_count\": {}, \"name\": {}, \
                 \"name_field\": {}, \"timestamp_metadata_only\": {}, \"lines\": [{}]}}",
                script.entry().index,
                script.entry().entry_offset,
                script.entry().script_offset,
                script.limit(),
                script.terminator_offset(),
                script.end(),
                script.len(),
                jbytes(script.name()),
                jbytes(script.entry().name_field),
                script.entry().raw_timestamp,
                lines.join(", "),
            )
        })
        .collect();
    let findings: Vec<String> = decoded.findings().iter().map(finding_json).collect();
    format!(
        "{{\n  \"report\": {},\n  \"container\": {},\n  \"container_bytes\": {},\n  \
         \"arguments_encoding\": \"unknown: bytes are reported as length and hex\",\n  \
         \"header\": {{\"signature\": {}, \"version\": {}, \"script_count\": {}}},\n  \
         \"index_end\": {},\n  \"script_count\": {},\n  \"findings\": [{}],\n  \
         \"loading_plan\": {},\n  \"scripts\": [{}]\n}}\n",
        jstr(INTERP_REPORT_VERSION),
        jstr(label),
        decoded.container_len(),
        decoded.header().signature,
        decoded.header().version,
        decoded.header().script_count,
        decoded.index_end(),
        decoded.scripts().len(),
        findings.join(", "),
        plan.map(plan_json).unwrap_or_else(|| "null".to_owned()),
        scripts.join(",\n    "),
    )
}

/// Renders the loading plan (F07-C) as a JSON object.
///
/// Everything a plan cannot establish is present and says so: the statuses are
/// `complete`/`incomplete`, the per-line kinds are `loading`/`malformed`/
/// `unclassified`, and a key is reported as a validated key or as the part
/// that was refused. Nothing here reports a command as loaded that the VFS did
/// not answer.
fn plan_json(run: &PlanRun<'_>) -> String {
    let report = &run.report;
    let plan = &run.plan;
    let commands: Vec<String> = plan
        .commands()
        .iter()
        .map(|command| {
            format!(
                "{{\"spelling\": {}, \"namespace_arg\": {}, \"path_arg\": {}, \
                 \"variant_arg\": {}, \"spelling_kind\": {}, \"status\": {}, \"source\": {}}}",
                jbytes(&command.spelling),
                command.arguments.namespace,
                command.arguments.path,
                command
                    .arguments
                    .variant
                    .map_or("null".to_owned(), |at| at.to_string()),
                jstr(command.spelling_kind.code()),
                jstr(command.status.label()),
                jstr(&command.source),
            )
        })
        .collect();
    let opcodes: Vec<String> = plan
        .opcodes()
        .iter()
        .map(|opcode| {
            format!(
                "{{\"spelling\": {}, \"class\": {}, \"status\": {}, \"source\": {}}}",
                jbytes(&opcode.spelling),
                jstr(opcode.class.code()),
                jstr(opcode.status.label()),
                jstr(&opcode.source),
            )
        })
        .collect();
    let heads: Vec<String> = run
        .audit
        .entries()
        .iter()
        .map(|entry| {
            let class = entry
                .classification()
                .and_then(|index| run.audit.opcode(index))
                .map_or("unknown", |opcode| opcode.class.code());
            format!(
                "{{\"spelling\": {}, \"occurrences\": {}, \"class\": {}}}",
                jbytes(entry.spelling()),
                entry.occurrences(),
                jstr(class),
            )
        })
        .collect();
    let dependencies: Vec<String> = report
        .dependencies()
        .iter()
        .map(|dependency| {
            let key = dependency.key().map_or("null".to_owned(), |key| {
                format!(
                    "{{\"namespace\": {}, \"variant\": {}, \"path\": {}}}",
                    jstr(key.namespace().as_str()),
                    jstr(key.variant().as_str()),
                    jstr(key.path().as_str())
                )
            });
            let span = dependency.span().map_or("null".to_owned(), |span| {
                format!(
                    "{{\"install_sha256\": {}, \"container_path\": {}, \"member_key\": {}, \
                     \"offset\": {}, \"length\": {}}}",
                    jstr(&span.install_sha256().to_hex()),
                    jstr(span.container_path()),
                    match span.member_key() {
                        Some(member) => jstr(member),
                        None => "null".to_owned(),
                    },
                    span.offset(),
                    span.length(),
                )
            });
            format!(
                "{{\"site\": {{\"script\": {}, \"line\": {}, \"source_offset\": {}, \
                 \"head_offset\": {}}}, \"command\": {}, \"key\": {}, \"status\": {}, \
                 \"span\": {}, \"detail\": {}}}",
                dependency.site.script,
                dependency.site.line,
                dependency.site.source_offset,
                dependency.site.head_offset,
                jbytes(&dependency.command.spelling),
                key,
                jstr(dependency.state().code()),
                span,
                jstr(&dependency_detail(dependency)),
            )
        })
        .collect();
    let failures: Vec<String> = report
        .failures()
        .iter()
        .map(|failure| {
            format!(
                "{{\"code\": {}, \"script\": {}, \"line\": {}, \"source_offset\": {}, \
                 \"head_offset\": {}, \"world\": {}, \"command\": {}, \"detail\": {}}}",
                jstr(failure.code),
                failure.site.script,
                failure.site.line,
                failure.site.source_offset,
                failure.site.head_offset,
                match &failure.world {
                    Some(world) => jstr(world.as_relative().as_str()),
                    None => "null".to_owned(),
                },
                failure
                    .command
                    .map_or("null".to_owned(), |command| command.to_string()),
                jstr(&failure.detail),
            )
        })
        .collect();
    let scripts: Vec<String> = report
        .scripts()
        .iter()
        .map(|script| {
            format!(
                "{{\"index\": {}, \"entry_offset\": {}, \"script_offset\": {}, \
                 \"terminator_offset\": {}, \"end\": {}, \"name\": {}, \
                 \"timestamp_metadata_only\": {}, \"content_sha256\": {}, \"state\": {}, \
                 \"blocking_lines\": {}, \"failed_dependencies\": {}, \"dependencies\": [{}]}}",
                script.origin.index(),
                script.origin.entry_offset(),
                script.origin.script_offset(),
                script.origin.terminator_offset(),
                script.origin.end(),
                jbytes(&script.name),
                script.raw_timestamp,
                jstr(&script.content_sha256.to_hex()),
                jstr(script.state.code()),
                script.state.blocking_lines(),
                script.state.failed_dependencies(),
                script
                    .dependencies
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
        .collect();
    let stats = report.stats();
    format!(
        "{{\n  \"container_sha256\": {},\n  \"world\": {},\n  \
         \"installation_sha256\": {},\n  \"session_generation\": {},\n  \
         \"status\": {},\n  \"complete\": {},\n  \"summary\": {},\n  \
         \"commands\": [{}],\n  \"opcodes\": [{}],\n  \
         \"opcode_audit\": {{\"lines\": {}, \"distinct_heads\": {}, \"unknown_heads\": {}, \
         \"complete\": {}, \"heads\": [{}]}},\n  \
         \"stats\": {{\"scripts\": {}, \"lines\": {}, \
         \"loading_commands\": {}, \"malformed_commands\": {}, \"unsupported_commands\": {}, \
         \"behavior_commands\": {}, \"unclassified_commands\": {}, \
         \"distinct_heads\": {}, \"blocked_scripts\": {}}},\n  \
         \"dependencies\": [{}],\n  \"failures\": [{}],\n  \"resolved\": {}, \
         \"dynamic_lookups\": {},\n  \"scripts\": [{}]\n  }}",
        jstr(&report.container_sha256().to_hex()),
        match report.world() {
            Some(world) => jstr(world.as_relative().as_str()),
            None => "null".to_owned(),
        },
        match report.installation() {
            Some(hash) => jstr(&hash.to_hex()),
            None => "null".to_owned(),
        },
        match report.generation() {
            Some(generation) => generation.get().to_string(),
            None => "null".to_owned(),
        },
        jstr(if report.is_complete() {
            "complete"
        } else {
            "incomplete"
        }),
        report.is_complete(),
        jstr(&report.describe()),
        commands.join(", "),
        opcodes.join(",\n    "),
        run.audit.lines(),
        run.audit.distinct_heads(),
        run.audit.unknown_heads(),
        run.audit.is_complete(),
        heads.join(",\n    "),
        stats.scripts,
        stats.lines,
        stats.loading_commands,
        stats.malformed_commands,
        stats.unsupported_commands,
        stats.behavior_commands,
        stats.unclassified_commands,
        stats.distinct_heads,
        stats.blocked_scripts,
        dependencies.join(",\n    "),
        failures.join(", "),
        report.resolved_count(),
        report.dynamic_lookups(),
        scripts.join(",\n    "),
    )
}

/// The one-line reason a dependency ended, as its own state renders it.
fn dependency_detail(dependency: &cs_content::loading::LoadingDependency) -> String {
    match dependency.state() {
        DependencyState::Resolved { span } => {
            format!("{} at offset {}", span.container_path(), span.offset())
        }
        DependencyState::Unresolved { detail, .. } => detail.clone(),
        DependencyState::Composed { spelling_kind } => format!(
            "the key is assembled at run time ({})",
            spelling_kind.code()
        ),
        DependencyState::Invalid { part, detail } => {
            format!("the {part} argument is not a valid key part: {detail}")
        }
    }
}

/// One finding as a JSON object.
fn finding_json(finding: &cs_formats::InterpFinding) -> String {
    match finding {
        cs_formats::InterpFinding::Unclaimed { offset, length } => format!(
            "{{\"code\": \"unclaimed\", \"offset\": {}, \"length\": {}}}",
            offset, length
        ),
        cs_formats::InterpFinding::SharedScriptOffset { offset, entries } => {
            let entries: Vec<String> = entries.iter().map(usize::to_string).collect();
            format!(
                "{{\"code\": \"shared_script_offset\", \"offset\": {}, \"entries\": [{}]}}",
                offset,
                entries.join(", ")
            )
        }
        cs_formats::InterpFinding::UnterminatedName { index, offset } => format!(
            "{{\"code\": \"unterminated_name\", \"index\": {}, \"offset\": {}}}",
            index, offset
        ),
        cs_formats::InterpFinding::NamePadding {
            index,
            offset,
            length,
        } => format!(
            "{{\"code\": \"name_padding\", \"index\": {}, \"offset\": {}, \"length\": {}}}",
            index, offset, length
        ),
    }
}

/// A byte string as a JSON object: its length and its lowercase hex.
///
/// Deliberately not a text literal. Names and arguments have no established
/// encoding, so printing them as `str` would claim one; hex plus length is
/// lossless, comparable and says nothing the file does not say.
fn jbytes(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{{\"length\": {}, \"hex\": {}}}", bytes.len(), jstr(&hex))
}

/// Whether `report` is a well-formed JSON document: balanced braces and
/// brackets, every string literal terminated and escaped.
///
/// The report is assembled by hand, so the tests check its shape instead of
/// trusting it. A report that does not parse is not a report, and a substring
/// assertion on its own would not notice a stray brace.
#[cfg(test)]
fn is_well_formed_json(report: &str) -> bool {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for character in report.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_string && !escaped
}

/// A JSON string literal: quoted and escaped.
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
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    //! F07-B and F07-C acceptance tests for the `interp` command. Every
    //! container and installation here is newly authored synthetic bytes
    //! written below the system temporary directory; no original game data, no
    //! `CS_GAME_DIR` access.

    use super::*;
    use std::collections::VecDeque;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f07-b-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("temp dir is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(spelling);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent dirs are created");
            }
            fs::write(&path, bytes).expect("bytes are written");
            path
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

    /// Header, index and one script with one two-argument line.
    fn container(name: &[u8], data: &[u8], count: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [0x0897_1119u32, 7, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut field = [0u8; cs_formats::NAME_FIELD_BYTES];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&7u32.to_le_bytes()); // timestamp
        bytes.extend_from_slice(&140u32.to_le_bytes()); // script offset
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    /// The command wires the decoder end to end: a valid container decodes to
    /// exit 0 and a report whose script, extent, terminator and tokens match
    /// the bytes the test wrote.
    #[test]
    fn accept_f07_b_cli_decodes_a_container_and_reports_its_tokens() {
        let tree = Temp::new("valid");
        let path = tree.write("probe.interp", &container(b"probe", b"LOAD\0world\0", 2));
        let path = path.to_str().expect("temp paths are UTF-8");

        let run = interp_command_result(&args(&["--file", path]));
        assert_eq!(run.exit_code, 0, "diagnostics: {:?}", run.diagnostics);
        assert!(run.diagnostics.is_empty());
        let report = run.report.expect("the container decoded");
        assert!(
            is_well_formed_json(&report),
            "the report is a JSON document:\n{report}"
        );

        // The header and the container fingerprint.
        assert!(report.contains("\"report\": \"cs-inspect-interp/1\""));
        assert!(report.contains("\"container_bytes\": 163"));
        assert!(report.contains("\"script_count\": 1"));
        assert!(report.contains("\"index_end\": 140"));
        // The script's origin: index position, entry offset, script offset and
        // the extent the decoder validated it against.
        assert!(report.contains("\"index\": 0"));
        assert!(report.contains("\"entry_offset\": 12"));
        assert!(report.contains("\"script_offset\": 140"));
        assert!(report.contains("\"limit\": 163"));
        assert!(report.contains("\"terminator_offset\": 159"));
        assert!(report.contains("\"end\": 163"));
        // The name as bytes, never as guessed text.
        assert!(report.contains("\"length\": 5, \"hex\": \"70726f6265\""));
        // The two arguments with their absolute offsets, and the head offset.
        assert!(
            report.contains("\"offset\": 148, \"bytes\": {\"length\": 4, \"hex\": \"4c4f4144\"}")
        );
        assert!(
            report.contains("\"offset\": 153, \"bytes\": {\"length\": 5, \"hex\": \"776f726c64\"}")
        );
        assert!(report.contains("\"head_offset\": 148"));
        assert!(report.contains("\"token_count\": 2"));
        // A clean container claims every byte.
        assert!(report.contains("\"findings\": []"));

        // `--raw` adds the F07-A view of the same line next to the tokens.
        let run = interp_command_result(&args(&["--file", path, "--raw"]));
        let report = run.report.expect("the container decoded");
        assert_eq!(run.exit_code, 0);
        assert!(
            is_well_formed_json(&report),
            "the --raw report is a JSON document too:\n{report}"
        );
        assert!(report.contains("\"raw\": {\"offset\": 140, \"size\": 11, \"argument_count\": 2"));
    }

    /// A container whose argument count disagrees with the delimiters in its
    /// data is refused with the offset, and never reported as a clean pass.
    #[test]
    fn accept_f07_b_cli_refuses_a_container_that_fails_validation() {
        let tree = Temp::new("invalid");
        // Two delimiters, one declared argument.
        let path = tree.write("bad.interp", &container(b"bad", b"a\0b\0", 1));
        let path = path.to_str().expect("temp paths are UTF-8");

        let run = interp_command_result(&args(&["--file", path]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        assert!(run.report.is_none(), "a refused container reports nothing");
        let diagnostic = run.diagnostics.join("\n");
        assert!(diagnostic.contains("argument_count"), "{diagnostic}");
        assert!(diagnostic.contains("declares 1 arguments"), "{diagnostic}");
        assert!(diagnostic.contains("2 0x00 delimiters"), "{diagnostic}");

        // A missing --file and an unreadable path are invalid input, not a
        // runtime failure and not success.
        let run = interp_command_result(&args(&[]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.diagnostics[0].contains("--file"));

        let run = interp_command_result(&args(&["--file", "/nonexistent/nope.interp"]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.diagnostics[0].contains("cannot read"));

        // A container that is not INTERP at all is unsupported content.
        let path = tree.write("other.bin", b"not an interp container at all");
        let run = interp_command_result(&args(&["--file", path.to_str().expect("utf-8")]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        assert!(
            run.diagnostics[0].contains("signature"),
            "{:?}",
            run.diagnostics
        );
    }

    /// A container that decodes but leaves unclaimed bytes reports the regions
    /// and exits non-zero, so an anomaly is never read as a clean pass.
    #[test]
    fn accept_f07_b_cli_reports_unclaimed_regions_and_exits_nonzero() {
        let tree = Temp::new("gaps");
        let mut bytes = container(b"gapped", b"a\0", 1);
        let script_end = bytes.len() as u64;
        // Twelve bytes of tail after the script, which claims none of them.
        bytes.extend_from_slice(&[0xAB; 12]);
        let path = tree.write("gapped.interp", &bytes);
        let out = tree.0.join("report.json");

        let run = interp_command_result(&args(&[
            "--file",
            path.to_str().expect("utf-8"),
            "--out",
            out.to_str().expect("utf-8"),
        ]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        assert_eq!(run.out, Some(out.clone()));
        let report = fs::read_to_string(&out).expect("the report was written");
        assert!(
            report.contains(&format!(
                "\"code\": \"unclaimed\", \"offset\": {script_end}, \"length\": 12"
            )),
            "{report}"
        );
        // The anomaly is on stderr as well, for a human reading the run.
        assert!(
            run.diagnostics[0].contains("unclaimed"),
            "{:?}",
            run.diagnostics
        );
    }

    // --- Stage F07-C: the loading plan through the command.

    /// A container laid out with explicit bodies, so a test can place several
    /// scripts and several lines per script. Each line is its stored argument
    /// data, NUL-delimited, and the declared count is the number of
    /// delimiters — the rule the decoder enforces.
    fn image(entries: &[(&[u8], &[&[u8]])]) -> Vec<u8> {
        let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
        for (_, lines) in entries {
            let mut body = Vec::new();
            for data in *lines {
                body.extend_from_slice(&(data.len() as u32).to_le_bytes());
                body.extend_from_slice(
                    &(data.iter().filter(|byte| **byte == 0).count() as u32).to_le_bytes(),
                );
                body.extend_from_slice(data);
            }
            body.extend_from_slice(&0u32.to_le_bytes());
            bodies.push(body);
        }
        let mut offsets = Vec::with_capacity(entries.len());
        let start = 12 + entries.len() * 128;
        let mut cursor = start;
        for body in &bodies {
            offsets.push(cursor as u32);
            cursor += body.len();
        }
        let mut bytes = Vec::new();
        for word in [0x0897_1119u32, 7, entries.len() as u32] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for ((name, _), offset) in entries.iter().zip(&offsets) {
            let mut field = [0u8; cs_formats::NAME_FIELD_BYTES];
            field[..name.len()].copy_from_slice(name);
            bytes.extend_from_slice(&field);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        assert_eq!(bytes.len(), start);
        for body in &bodies {
            bytes.extend_from_slice(body);
        }
        bytes
    }

    /// A synthetic installation: two worlds, one file each, so a world-scoped
    /// resolution has something to pick.
    fn installation(label: &str) -> Temp {
        let tree = Temp::new(label);
        tree.write("ZBD/c1/plane.flt", b"world one plane");
        tree.write("ZBD/c2/plane.flt", b"world two plane, longer");
        tree
    }

    /// A command table with one rule: `loadmesh 1 2 - literal designed ...`.
    ///
    /// Written inside the test's own disposable directory, so a run leaves
    /// nothing behind in the system temporary directory.
    fn table(tree: &Temp, spelling: &str) -> PathBuf {
        tree.write(
            &format!("{spelling}-table.txt"),
            format!(
                "# synthetic table: exercises the plan, not an original command\n\
                 {spelling} 1 2 - literal designed docs/findings/<synthetic>.md\n"
            )
            .as_bytes(),
        )
    }

    /// AC04: a command nobody has classified yields its source offset and the
    /// affected world, and the plan never reports a loaded state.
    ///
    /// This is the state the workspace ships in: the command table is empty,
    /// because which commands load resources is unmeasured (F07-D). The
    /// command exits 3 and the report says `incomplete`, with every line's
    /// offset, the world it affects and no dependency at all.
    #[test]
    fn accept_f07_c_cli_reports_unclassified_commands_with_offsets_and_world() {
        let tree = installation("plan-empty");
        let bytes = image(&[(b"load", &[b"LoadGameGen\0world\0c1/plane.flt\0", b"Quit\0"])]);
        let path = tree.write("plan.interp", &bytes);
        let out = tree.0.join("plan.json");
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }

        let run = interp_command_result(&args(&[
            "--file",
            path_arg(&path),
            "--plan",
            "--cs-path",
            path_arg(&tree.0),
            "--world",
            "zbd/c1",
            "--out",
            path_arg(&out),
        ]));
        // The container decodes cleanly, but the plan cannot be built: two
        // lines, two unclassified commands. An incomplete plan never exits 0.
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        let report = fs::read_to_string(&out).expect("the report was written");
        assert_eq!(Some(&report), run.report.as_ref());
        assert!(
            is_well_formed_json(&report),
            "the plan report is a JSON document:\n{report}"
        );

        // The plan's own verdict, and the counts that produced it.
        assert!(report.contains("\"status\": \"incomplete\""), "{report}");
        assert!(report.contains("\"complete\": false"), "{report}");
        assert!(report.contains("\"unclassified_commands\": 2"), "{report}");
        assert!(report.contains("\"loading_commands\": 0"), "{report}");
        assert!(report.contains("\"resolved\": 0"), "{report}");
        assert!(report.contains("\"dynamic_lookups\": 0"), "{report}");
        assert!(report.contains("\"dependencies\": []"), "{report}");
        assert!(report.contains("\"world\": \"zbd/c1\""), "{report}");
        // The world is the one the context selected, and the session it was
        // resolved in is named, so the report is traceable.
        assert!(report.contains("\"session_generation\": "), "{report}");
        // The first line is at offset 140, the second after it.
        assert!(
            report.contains(
                "{\"code\": \"unclassified\", \"script\": 0, \"line\": 0, \
                 \"source_offset\": 140, \"head_offset\": 148, \"world\": \"zbd/c1\", \
                 \"command\": null, \"detail\": \"no registered loading command claims this \
                 line's head token\"}"
            ),
            "{report}"
        );
        assert!(
            report.contains("\"code\": \"unclassified\", \"script\": 0, \"line\": 1"),
            "{report}"
        );
        // The script is blocked, not ready.
        assert!(report.contains("\"state\": \"blocked\""), "{report}");
        assert!(!report.contains("\"state\": \"ready\""), "{report}");
        // Nothing claims to be loaded.
        assert!(!report.contains("\"status\": \"resolved\""), "{report}");
        // The same failures are on stderr for a human reading the run.
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("unclassified at offset 140")),
            "{:?}",
            run.diagnostics
        );
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("affects world zbd/c1")),
            "{:?}",
            run.diagnostics
        );
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("unmeasured (F07-D)")),
            "{:?}",
            run.diagnostics
        );
    }

    /// A script that is blocked *and* whose key did not resolve reports both
    /// counts, and a command table that is refused does not swallow what the
    /// decoder already found: a later failure does not un-happen the
    /// container's own anomalies.
    #[test]
    fn accept_f07_c_cli_reports_both_problems_and_keeps_the_containers_findings() {
        let tree = installation("plan-mixed");
        let commands = table(&tree, "loadmesh");
        // The first line is registered and unresolvable, the second is not
        // classified at all.
        let bytes = image(&[(b"load", &[b"loadmesh\0world\0absent.flt\0", b"Quit\0"])]);
        let path = tree.write("mixed.interp", &bytes);
        // Twelve unclaimed bytes after the script, which the decoder retains
        // as a finding and the command reports on stderr.
        let mut gapped = bytes.clone();
        let script_end = gapped.len();
        gapped.extend_from_slice(&[0xAB; 12]);
        let gapped_path = tree.write("gapped.interp", &gapped);
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }

        let run = interp_command_result(&args(&[
            "--file",
            path_arg(&path),
            "--plan",
            "--commands",
            path_arg(&commands),
            "--cs-path",
            path_arg(&tree.0),
            "--world",
            "zbd/c1",
        ]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        let report = run.report.expect("the plan is reported");
        assert!(
            is_well_formed_json(&report),
            "the plan report is a JSON document:\n{report}"
        );
        // One unclassified line and one dependency that did not resolve: the
        // script's own state names both, so neither is hidden behind the
        // other.
        assert!(
            report.contains(
                "\"state\": \"blocked\", \"blocking_lines\": 1, \"failed_dependencies\": 1"
            ),
            "{report}"
        );
        assert!(report.contains("\"code\": \"not_found\""), "{report}");
        assert!(report.contains("\"code\": \"unclassified\""), "{report}");

        // A command table that is refused exits 2, and the findings the
        // decoder retained for the same container are still on stderr.
        let bad_table = tree.write("bad-table.txt", b"loadmesh 1 2 - literal\n");
        let run = interp_command_result(&args(&[
            "--file",
            path_arg(&gapped_path),
            "--plan",
            "--commands",
            path_arg(&bad_table),
        ]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.report.is_none());
        let diagnostic = run.diagnostics.join("\n");
        assert!(diagnostic.contains("is refused"), "{diagnostic}");
        assert!(
            diagnostic.contains(&format!("unclaimed bytes at offset {script_end}")),
            "the container's own finding survives the table refusal: {diagnostic}"
        );
    }

    /// A registered command is resolved through the mounted session: the
    /// dependency carries the key, the world's own bytes' span and the site
    /// that asked for it, and the report is complete.
    #[test]
    fn accept_f07_c_cli_resolves_registered_commands_through_the_session() {
        let tree = installation("plan-resolve");
        let commands = table(&tree, "loadmesh");
        // A world-scoped key is spelled relative to the world group, which is
        // how the VFS mounts each group: `ZBD/c1/plane.flt` answers
        // `world:plane.flt` for the context that selected `zbd/c1`.
        let bytes = image(&[(b"load", &[b"loadmesh\0world\0plane.flt\0"])]);
        let path = tree.write("resolved.interp", &bytes);
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }

        let run = interp_command_result(&args(&[
            "--file",
            path_arg(&path),
            "--plan",
            "--commands",
            path_arg(&commands),
            "--cs-path",
            path_arg(&tree.0),
            "--world",
            "zbd/c1",
        ]));
        // The plan is complete: one registered command, one resolved key, no
        // failures and no dynamic lookups.
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a complete plan is reported");
        assert!(
            is_well_formed_json(&report),
            "the plan report is a JSON document:\n{report}"
        );
        assert!(report.contains("\"status\": \"complete\""), "{report}");
        assert!(report.contains("\"complete\": true"), "{report}");
        assert!(report.contains("\"loading_commands\": 1"), "{report}");
        assert!(report.contains("\"failures\": []"), "{report}");
        assert!(report.contains("\"state\": \"ready\""), "{report}");

        // The command as registered, with its claim and its provenance.
        assert!(
            report.contains("\"spelling_kind\": \"literal\""),
            "{report}"
        );
        assert!(report.contains("\"status\": \"designed\""), "{report}");
        assert!(
            report.contains("\"source\": \"docs/findings/<synthetic>.md\""),
            "{report}"
        );
        // The dependency: the key the arguments spelled, the site that asked,
        // and the span world c1's own file answers with.
        assert!(
            report.contains(
                "\"key\": {\"namespace\": \"world\", \"variant\": \"default\", \
                 \"path\": \"plane.flt\"}"
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "\"site\": {\"script\": 0, \"line\": 0, \"source_offset\": 140, \
                 \"head_offset\": 148}"
            ),
            "{report}"
        );
        assert!(report.contains("\"status\": \"resolved\""), "{report}");
        assert!(
            report.contains("\"container_path\": \"ZBD/c1\""),
            "{report}"
        );
        assert!(report.contains("\"member_key\": \"plane.flt\""), "{report}");
        // The span's length is the member's own length, so the report names
        // exactly the bytes world c1 holds.
        assert!(
            report.contains(&format!(
                "\"offset\": 0, \"length\": {}",
                b"world one plane".len()
            )),
            "{report}"
        );
        // The script's identity is a content hash of its own bytes, and the
        // timestamp stays labelled metadata.
        assert!(report.contains("\"content_sha256\": \""), "{report}");
        assert!(
            report.contains("\"timestamp_metadata_only\": 0"),
            "{report}"
        );
        assert!(report.contains("\"resolved\": 1"), "{report}");
        // The container itself is fingerprinted, so the report names the bytes.
        assert!(
            report.contains(&format!(
                "\"container_sha256\": \"{}\"",
                install::sha256(&bytes).to_hex()
            )),
            "{report}"
        );
    }

    /// A `composed` rule is a dynamic lookup: it is counted, named and never
    /// resolved, and it keeps the plan incomplete because the original engine
    /// assembles that key later.
    #[test]
    fn accept_f07_c_cli_counts_composed_keys_as_dynamic_lookups() {
        let tree = installation("plan-composed");
        let commands = tree.write(
            "composed-table.txt",
            b"loadmesh 1 2 - composed inferred docs/findings/<synthetic>.md\n",
        );
        let bytes = image(&[(b"load", &[b"loadmesh\0world\0%ZBD_DIR%/c1/plane.flt\0"])]);
        let path = tree.write("composed.interp", &bytes);
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }

        let run = interp_command_result(&args(&[
            "--file",
            path_arg(&path),
            "--plan",
            "--commands",
            path_arg(&commands),
            "--cs-path",
            path_arg(&tree.0),
            "--world",
            "zbd/c1",
        ]));
        // A dynamic lookup is not a failure, but the plan is not complete:
        // the world is not fully known while one key is still assembled later.
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        let report = run.report.expect("the plan is still reported");
        assert!(report.contains("\"status\": \"incomplete\""), "{report}");
        assert!(report.contains("\"dynamic_lookups\": 1"), "{report}");
        assert!(report.contains("\"resolved\": 0"), "{report}");
        assert!(report.contains("\"failures\": []"), "{report}");
        assert!(report.contains("\"status\": \"composed\""), "{report}");
        // The script itself is ready — no line failed — while the *plan* is
        // not complete, because one key is still assembled later. The
        // distinction is the point: a dynamic lookup is not an error, and
        // claiming the world is fully known anyway would be.
        assert!(report.contains("\"state\": \"ready\""), "{report}");
        // The `%NAME%` spelling was never resolved as if it were a path, and
        // the key is reported as absent rather than invented.
        assert!(report.contains("\"key\": null"), "{report}");
        assert!(!report.contains("\"status\": \"resolved\""), "{report}");
        assert!(
            report.contains("the key is assembled at run time (composed)"),
            "{report}"
        );
    }

    /// A command table is read whole or refused whole: a malformed rule, an
    /// unknown status, a self-awarded `verified_original` and a duplicate
    /// spelling are all invalid input, and no partially applied table ever
    /// produces a report.
    #[test]
    fn accept_f07_c_cli_refuses_a_malformed_command_table() {
        let tree = installation("plan-table");
        // A world-scoped key is spelled relative to the world group, which is
        // how the VFS mounts each group: `ZBD/c1/plane.flt` answers
        // `world:plane.flt` for the context that selected `zbd/c1`.
        let bytes = image(&[(b"load", &[b"loadmesh\0world\0plane.flt\0"])]);
        let path = tree.write("table.interp", &bytes);
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }
        let run_args = |table: &Path| {
            args(&[
                "--file",
                path_arg(&path),
                "--plan",
                "--commands",
                path_arg(table),
            ])
        };

        let cases: [(&str, &str); 6] = [
            ("short", "loadmesh 1 2\n"),
            (
                "position",
                "loadmesh one 2 - literal designed docs/findings/x.md\n",
            ),
            (
                "spelling-kind",
                "loadmesh 1 2 - guessed designed docs/findings/x.md\n",
            ),
            (
                "status",
                "loadmesh 1 2 - literal verified_original docs/findings/x.md\n",
            ),
            (
                "head-argument",
                "loadmesh 0 2 - literal designed docs/findings/x.md\n",
            ),
            (
                "duplicate",
                "loadmesh 1 2 - literal designed docs/findings/x.md\n\
                 loadmesh 1 3 - literal designed docs/findings/x.md\n",
            ),
        ];
        for (label, body) in cases {
            let table = tree.write(
                &format!("{label}.txt"),
                format!("# a synthetic table\n{body}").as_bytes(),
            );
            let run = interp_command_result(&run_args(&table));
            assert_eq!(
                run.exit_code, EXIT_INVALID_INPUT,
                "{label}: {:?}",
                run.diagnostics
            );
            assert!(
                run.report.is_none(),
                "{label}: a refused table reports nothing"
            );
            let diagnostic = run.diagnostics.join("\n");
            assert!(
                diagnostic.contains("is refused") || diagnostic.contains("needs a value"),
                "{label}: {diagnostic}"
            );
        }

        // A table that cannot be read is invalid input too, and a missing one
        // is named as such rather than read as an empty table.
        let missing = tree.0.join("nope.txt");
        let run = interp_command_result(&run_args(&missing));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.diagnostics[0].contains("cannot read command table"));

        // An unsupported argument is refused before anything is read.
        let run = interp_command_result(&args(&["--file", path_arg(&path), "--nope"]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.diagnostics[0].contains("unsupported argument"));
    }

    /// A container that does not decode is still refused with `--plan`: the
    /// plan is not built from bytes the decoder rejected, and the exit code is
    /// the decoder's, not a plan verdict.
    #[test]
    fn accept_f07_c_cli_refuses_an_undecodable_container_before_planning() {
        let tree = Temp::new("plan-invalid");
        // Two delimiters, one declared argument.
        let path = tree.write("bad.interp", &container(b"bad", b"a\0b\0", 1));
        let path = path.to_str().expect("temp paths are UTF-8");

        let run = interp_command_result(&args(&["--file", path, "--plan"]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        assert!(
            run.report.is_none(),
            "no plan is reported for refused bytes"
        );
        assert!(
            run.diagnostics[0].contains("argument_count"),
            "{:?}",
            run.diagnostics
        );
    }

    // --- Stage F07-D: opcode classification and audit through the command.

    /// A classification file with one `behavior` entry for `SetCamera` and one
    /// `loading` entry for `LoadGameGen`.
    fn classes_with_behavior(tree: &Temp) -> PathBuf {
        tree.write(
            "classes.txt",
            b"# synthetic classification: exercises the audit, not an original command\n\
              behavior    SetCamera    inferred docs/findings/<synthetic>.md\n\
              loading     LoadGameGen  1 2 - literal designed docs/findings/<synthetic>.md\n",
        )
    }

    /// AC04 end to end: a command the classification calls resource-loading but
    /// with no supported key domain yields its source offset and its affected
    /// world through the command, exits non-zero and reports no dependency.
    #[test]
    fn accept_f07_d_cli_reports_an_unsupported_command_with_its_offset_and_world() {
        let tree = installation("classes-unsupported");
        let classes = tree.write(
            "classes.txt",
            b"unsupported SetDir inferred docs/findings/<synthetic>.md\n",
        );
        let bytes = image(&[(b"load", &[b"SetDir\0world\0subdir\0"])]);
        let path = tree.write("unsupported.interp", &bytes);
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }

        let run = interp_command_result(&args(&[
            "--file",
            path_arg(&path),
            "--plan",
            "--classes",
            path_arg(&classes),
            "--cs-path",
            path_arg(&tree.0),
            "--world",
            "zbd/c1",
        ]));
        assert_eq!(
            run.exit_code, EXIT_FAILED_VALIDATION,
            "{:?}",
            run.diagnostics
        );
        let report = run.report.expect("the plan is reported");
        assert!(
            is_well_formed_json(&report),
            "the plan report is a JSON document:\n{report}"
        );

        // The classification is the one supplied, and the audit is complete:
        // the only head of this container is classified.
        assert!(
            report.contains(
                "\"opcodes\": [{\"spelling\": {\"length\": 6, \"hex\": \"536574446972\"}, \
                 \"class\": \"unsupported\""
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "\"opcode_audit\": {\"lines\": 1, \"distinct_heads\": 1, \"unknown_heads\": 0, \
                 \"complete\": true"
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "\"code\": \"unsupported_command\", \"script\": 0, \"line\": 0, \
                 \"source_offset\": 140, \"head_offset\": 148, \"world\": \"zbd/c1\""
            ),
            "{report}"
        );
        // It is not a fake loaded state: no dependency, no resolved key.
        assert!(report.contains("\"dependencies\": []"), "{report}");
        assert!(report.contains("\"unsupported_commands\": 1"), "{report}");
        assert!(report.contains("\"loading_commands\": 0"), "{report}");
        assert!(report.contains("\"state\": \"blocked\""), "{report}");
        assert!(!report.contains("\"status\": \"resolved\""), "{report}");
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("unsupported_command at offset 140")),
            "{:?}",
            run.diagnostics
        );
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("affects world zbd/c1")),
            "{:?}",
            run.diagnostics
        );
    }

    /// The audit names every distinct head and its class, and completeness is a
    /// checked property: leaving one head unclassified keeps the audit and the
    /// run incomplete even when every *other* line is understood.
    #[test]
    fn accept_f07_d_cli_audits_every_head_and_reports_completeness() {
        let tree = installation("classes-audit");
        let bytes = image(&[(
            b"load",
            &[
                b"SetCamera\0follow\0target\0",
                b"LoadGameGen\0world\0plane.flt\0",
            ],
        )]);
        let path = tree.write("audit.interp", &bytes);
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }
        let run_args = |classes: &Path| {
            args(&[
                "--file",
                path_arg(&path),
                "--plan",
                "--classes",
                path_arg(classes),
                "--cs-path",
                path_arg(&tree.0),
                "--world",
                "zbd/c1",
            ])
        };

        // One head is classified, the other is not: the audit is 1 of 2.
        let partial = tree.write(
            "partial-classes.txt",
            b"behavior SetCamera inferred docs/findings/<synthetic>.md\n",
        );
        let run = interp_command_result(&run_args(&partial));
        assert_eq!(
            run.exit_code, EXIT_FAILED_VALIDATION,
            "{:?}",
            run.diagnostics
        );
        let report = run.report.expect("the plan is reported");
        assert!(
            report.contains(
                "\"opcode_audit\": {\"lines\": 2, \"distinct_heads\": 2, \"unknown_heads\": 1, \
                 \"complete\": false"
            ),
            "{report}"
        );
        // Both heads are named, with their occurrence counts and classes.
        assert!(
            report.contains(
                "{\"spelling\": {\"length\": 9, \"hex\": \"53657443616d657261\"}, \
                 \"occurrences\": 1, \"class\": \"behavior\"}"
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "{\"spelling\": {\"length\": 11, \"hex\": \"4c6f616447616d6547656e\"}, \
                 \"occurrences\": 1, \"class\": \"unknown\"}"
            ),
            "{report}"
        );
        assert!(report.contains("\"unclassified_commands\": 1"), "{report}");
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("1 of 2 distinct opcode heads are unclassified")),
            "{:?}",
            run.diagnostics
        );

        // Classify every head: the audit is complete and, because the loading
        // key resolves and the behavior line contributes nothing, so is the
        // plan. A `behavior` line does not block its world.
        let classes = classes_with_behavior(&tree);
        let run = interp_command_result(&run_args(&classes));
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a complete plan is reported");
        assert!(
            report.contains("\"distinct_heads\": 2, \"unknown_heads\": 0, \"complete\": true"),
            "{report}"
        );
        assert!(report.contains("\"status\": \"complete\""), "{report}");
        assert!(report.contains("\"behavior_commands\": 1"), "{report}");
        assert!(report.contains("\"loading_commands\": 1"), "{report}");
        assert!(report.contains("\"resolved\": 1"), "{report}");
        assert!(report.contains("\"failures\": []"), "{report}");
        assert!(report.contains("\"class\": \"behavior\""), "{report}");
    }

    /// A classification is read whole or refused whole, and `--classes` and
    /// `--commands` cannot both be supplied.
    #[test]
    fn accept_f07_d_cli_refuses_a_malformed_classification() {
        let tree = installation("classes-refused");
        let bytes = image(&[(b"load", &[b"LoadGameGen\0world\0plane.flt\0"])]);
        let path = tree.write("classes-refused.interp", &bytes);
        fn path_arg(path: &Path) -> &str {
            path.to_str().expect("temp paths are UTF-8")
        }
        let run_args = |classes: &Path| {
            args(&[
                "--file",
                path_arg(&path),
                "--plan",
                "--classes",
                path_arg(classes),
            ])
        };

        // An unknown class keyword, an unknown status and a duplicate spelling
        // are all invalid input, before any plan is built.
        let bad_class = tree.write(
            "bad-class.txt",
            b"nonsense Foo inferred docs/findings/x.md\n",
        );
        let run = interp_command_result(&run_args(&bad_class));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.report.is_none());
        assert!(
            run.diagnostics[0].contains("the class must be `loading`, `unsupported` or `behavior`"),
            "{:?}",
            run.diagnostics
        );

        let bad_status = tree.write("bad-status.txt", b"behavior Foo bogus docs/findings/x.md\n");
        let run = interp_command_result(&run_args(&bad_status));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(
            run.diagnostics[0].contains("is not an evidence status"),
            "{:?}",
            run.diagnostics
        );

        let duplicate = tree.write(
            "duplicate.txt",
            b"behavior Foo inferred docs/findings/x.md\n\
              behavior Foo inferred docs/findings/x.md\n",
        );
        let run = interp_command_result(&run_args(&duplicate));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(
            run.diagnostics[0].contains("is refused as a whole"),
            "{:?}",
            run.diagnostics
        );

        // A missing classification is named as such, not read as empty.
        let missing = tree.0.join("nope.txt");
        let run = interp_command_result(&run_args(&missing));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.diagnostics[0].contains("cannot read classification"));

        // Two sources of truth for one line are refused.
        let classes = classes_with_behavior(&tree);
        let commands = table(&tree, "loadmesh");
        let run = interp_command_result(&args(&[
            "--file",
            path_arg(&path),
            "--plan",
            "--commands",
            path_arg(&commands),
            "--classes",
            path_arg(&classes),
        ]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(
            run.diagnostics[0].contains("mutually exclusive"),
            "{:?}",
            run.diagnostics
        );
    }

    // --- retail ---------------------------------------------------------------

    /// SHA-256 of the installed `ZBD/interp.zbd` this stage audited.
    const RETAIL_INTERP_SHA256: &str =
        "f5251cb559db1992320247b9674d159a149572e077bc8579ae34d5fbd16254c7";

    /// Workspace-relative path of the committed classification of that file.
    const RETAIL_CLASSES: &str = "docs/findings/2026-09-28-f07-d-retail-opcode-classes.txt";

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

    fn path_str(path: &Path) -> &str {
        path.to_str().expect("workspace paths are UTF-8")
    }

    /// The installed container, audited with the committed classification:
    /// every distinct head is classified, so the audit is complete, and this
    /// stage's minimum scenario is visible on real bytes — an unsupported
    /// loading command names its source offset and its affected world and never
    /// reports a loaded state.
    ///
    /// The second half proves the classification is load-bearing: with an empty
    /// classification the same container has 85 unknown heads.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f07_d_retail_interp_audit_classifies_every_head() {
        let root = game_dir();
        let container_path = root.join("ZBD").join("interp.zbd");
        let classes = workspace_root().join(RETAIL_CLASSES);
        assert!(
            classes.is_file(),
            "the committed classification is missing: {}",
            classes.display()
        );
        let bytes = fs::read(&container_path).expect("the retail container is readable");
        assert_eq!(
            install::sha256(&bytes).to_hex(),
            RETAIL_INTERP_SHA256,
            "the installed container is not the one F07-D audited"
        );

        let run = interp_command_result(&args(&[
            "--file",
            path_str(&container_path),
            "--plan",
            "--classes",
            path_str(&classes),
            "--cs-path",
            path_str(&root),
            "--world",
            "zbd/c1",
        ]));
        assert_eq!(
            run.exit_code, EXIT_FAILED_VALIDATION,
            "{:?}",
            run.diagnostics
        );
        let report = run.report.expect("the audit is reported");
        assert!(is_well_formed_json(&report), "{report}");

        // The decoder read the whole container: no unclaimed region.
        assert!(report.contains("\"script_count\": 98"), "{report}");
        assert!(report.contains("\"container_bytes\": 188296"), "{report}");
        assert!(report.contains("\"findings\": []"), "{report}");

        // Every distinct head is classified, and no line is unclassified.
        assert!(
            report.contains(
                "\"opcode_audit\": {\"lines\": 5083, \"distinct_heads\": 85, \
                 \"unknown_heads\": 0, \"complete\": true"
            ),
            "{report}"
        );
        assert!(report.contains("\"unclassified_commands\": 0"), "{report}");
        assert!(report.contains("\"behavior_commands\": 4131"), "{report}");
        assert!(report.contains("\"unsupported_commands\": 952"), "{report}");
        assert!(report.contains("\"loading_commands\": 0"), "{report}");

        // AC04 on real bytes: an unsupported command yields its source offset
        // and its affected world, and nothing is reported loaded.
        assert!(
            report.contains(
                "\"code\": \"unsupported_command\", \"script\": 0, \"line\": 0, \
                 \"source_offset\": 12556, \"head_offset\": 12564, \"world\": \"zbd/c1\""
            ),
            "{report}"
        );
        assert!(report.contains("\"dependencies\": []"), "{report}");
        assert!(!report.contains("\"status\": \"resolved\""), "{report}");

        // The committed classification is load-bearing: with an empty one, the
        // same container has 85 unknown heads and every line is unclassified.
        let scratch = Temp::new("retail-partial");
        let empty = scratch.write("empty-classes.txt", b"# no classifications\n");
        let run = interp_command_result(&args(&[
            "--file",
            path_str(&container_path),
            "--plan",
            "--classes",
            path_str(&empty),
            "--cs-path",
            path_str(&root),
            "--world",
            "zbd/c1",
        ]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        let report = run.report.expect("the empty audit is reported");
        assert!(
            report.contains(
                "\"opcode_audit\": {\"lines\": 5083, \"distinct_heads\": 85, \
                 \"unknown_heads\": 85, \"complete\": false"
            ),
            "{report}"
        );
        assert!(
            report.contains("\"unclassified_commands\": 5083"),
            "{report}"
        );
    }

    // --- evidence harness -----------------------------------------------------

    fn env_var(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!("{name} is not set: run the sequence in the evidence harness doc comment")
        })
    }

    fn command_output(program: &str, args: &[&str]) -> String {
        let output = Command::new(program)
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("{program} runs: {error}"));
        assert!(output.status.success(), "{program} {args:?} failed");
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

    /// Evidence-report harness for F07-D (`docs/contracts/CLI-EVIDENCE.md`,
    /// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
    /// loudly when its inputs are missing. From the workspace root:
    ///
    /// 1. ```sh
    ///    mkdir -p private/evidence/F07-D
    ///    cargo test --workspace --locked -- accept_f07_d_ --include-ignored \
    ///      2>&1 | tee private/evidence/F07-D/cargo-test.log
    ///    ```
    ///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
    /// 2. ```sh
    ///    CS_EVIDENCE_DIR=private/evidence/F07-D \
    ///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
    ///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f07_d_ --include-ignored" \
    ///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
    ///      cargo test --locked -p cs_inspect --lib -- evidence_report_f07_d --ignored
    ///    ```
    ///    This runs the production `interp --plan --classes` command over
    ///    `$CS_GAME_DIR/ZBD/interp.zbd` and keeps its report as the artifact
    ///    `interp-audit.json`.
    /// 3. ```sh
    ///    python3 tools/validate_evidence.py private/evidence/F07-D/acceptance.json \
    ///      --artifact-root private/evidence/F07-D --require-pass
    ///    ```
    /// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F07-D.json`.
    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f07_d_writes_the_acceptance_report() {
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
        let ([passed, failed, ignored], results) = parse_suite(&log, "accept_f07_d_");
        assert!(
            passed > 0 && !results.is_empty(),
            "no accept_f07_d_ tests in the log"
        );
        let retail = "accept_f07_d_retail_interp_audit_classifies_every_head";
        let status = results
            .iter()
            .find(|(name, _)| short_name(name) == retail)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail} did not run: use --include-ignored"));
        assert_eq!(status, "pass", "{retail} must pass");

        // The production command over the installation, kept as the artifact.
        let container_path = root.join("ZBD").join("interp.zbd");
        let classes = workspace_root().join(RETAIL_CLASSES);
        let audit_path = evidence_dir.join("interp-audit.json");
        let audited = interp_command_result(&args(&[
            "--file",
            path_str(&container_path),
            "--plan",
            "--classes",
            path_str(&classes),
            "--cs-path",
            path_str(&root),
            "--world",
            "zbd/c1",
            "--out",
            path_str(&audit_path),
        ]));
        assert_eq!(audited.out.as_deref(), Some(audit_path.as_path()));
        let report = audited.report.expect("the retail audit is reported");
        assert!(
            report.contains(
                "\"opcode_audit\": {\"lines\": 5083, \"distinct_heads\": 85, \
                 \"unknown_heads\": 0, \"complete\": true"
            ),
            "the retail audit must classify every head: {report}"
        );
        assert!(report.contains("\"unclassified_commands\": 0"), "{report}");
        assert!(report.contains("\"unsupported_commands\": 952"), "{report}");
        assert!(report.contains("\"behavior_commands\": 4131"), "{report}");
        assert!(
            report.contains(
                "\"source_offset\": 12556, \"head_offset\": 12564, \"world\": \"zbd/c1\""
            ),
            "{report}"
        );

        // The classification is copied into the artifact root, so the evidence
        // is self-contained and its hash is recorded beside the report.
        let classes_copy = evidence_dir.join("retail-opcode-classes.txt");
        fs::copy(&classes, &classes_copy)
            .unwrap_or_else(|error| panic!("copy {}: {error}", classes.display()));

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
             field from the recorded log, production discovery of $CS_GAME_DIR, and the \
             production `cs-inspect interp --plan --classes` run over \
             $CS_GAME_DIR/ZBD/interp.zbd (interp-audit.json; SHA-256 {RETAIL_INTERP_SHA256}), \
             whose committed classification (retail-opcode-classes.txt) classifies all 85 \
             distinct heads of the 98 scripts / 5083 lines, so the F07-D audit is complete and \
             reports 0 unknown heads, 952 unsupported lines and 4131 behavior lines; every \
             unsupported line names its source offset and affected world and no dependency is \
             invented. The classes are a corpus-inferred claim (status `inferred`) recorded in \
             docs/findings/2026-09-28-f07-d-retail-opcode-classes.md; their limitations are the \
             F07-D findings, not hidden, and the report run exits {} because the unsupported \
             lines block their worlds. Validated with tools/validate_evidence.py --require-pass",
            audited.exit_code
        );
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F07-D\",\n\
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
             \x20\"artifacts\": [{}, {}, {}],\n\
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
            artifact(&audit_path, "json"),
            artifact(&classes_copy, "txt"),
            super::jstr("deepseek-1 (implementing agent)"),
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
