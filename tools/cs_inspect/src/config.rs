//! The `config` command (F12-C): resolve typed tuning fields and localized
//! string ids through the `cs_content::config` consumers of the F12-B
//! readers.
//!
//! ```text
//! cs-inspect config --file <path> [--cs-path <dir>] [--container <spelling>]
//!     [--member <member>] [--install-sha256 <hex>]
//!     [--string <id>[:<language>]]...
//!     [--field <consumer>=<section>:<key>:<index>:<width>:<signed>]...
//!     [--out <file>]
//! ```
//!
//! The file's bytes are routed by the observed member rule
//! ([`dialect_for_member`]), never by an extension (spec F12, non-negotiable
//! #1): a `strings.dll` image is read as inert PE data into a
//! [`StringCatalog`], and a `LAYOUT.CSV` keyed list is read into a
//! [`ConfigDocument`] with [`resolve_tunings`]. The routing overrides let a
//! loose export be read as the surveyed member it came from:
//! `--container GOSDATA/ASSETS/crimson.rof --member ASSETS/LAYOUT.CSV` reads a
//! loose file as that keyed list. A member no rule covers is refused; an
//! extension alone routes nothing.
//!
//! # Which spelling a member is routed under
//!
//! The inventory writes every loose member rule as a path **relative to the
//! installation root** ([`MemberRule::Loose`]), and
//! `GOSDATA/ASSETS/BINARIES/langui.dll` is such a rule: its file name alone
//! (`langui.dll`) is not the spelling the rule speaks, so a `--file` under
//! the installation would be unrouted by name. The routing therefore tries,
//! most specific first ([`routing_candidates`]):
//!
//! 1. the explicit `--container` spelling, when one was given, and nothing
//!    else — an override names the member, so no inference may contradict it;
//! 2. the file's **installation-relative** spelling, when the file lies under
//!    the selected root (`--cs-path`, which wins over `CS_GAME_DIR`);
//! 3. the file name, which is what a loose export carries.
//!
//! The spelling that routed is the one the report's `source.container`
//! records, so provenance names the member the rules describe rather than a
//! guess. A root that does not contain the file contributes no candidate: it
//! is a routing hint, not a precondition for reading the file, and a stale
//! `CS_GAME_DIR` can therefore never turn a working run into a failure.
//! Nothing is mounted and no other file of the installation is read, and the
//! root is never looked up on disk. Its letter case does not decide the
//! member either: root components are compared ASCII case-insensitively, the
//! way installation paths are keyed, and the remainder keeps its own case. A
//! root with no components (`""`, or `.` against a relative `--file`) is the
//! empty prefix, so a relative `--file` is already its own
//! installation-relative spelling and routing degrades to the file name in
//! every other case.
//!
//! The JSON report is one of two shapes, named by `"kind"`:
//!
//! * `pe_resources`: the image layout, the string accounting, the languages
//!   and every localizable string with its id, language, code page, decoded
//!   text (or `null` when the code units do not decode), its exact code units
//!   and its source span. `--string <id>[:<language>]` adds a `lookups` entry
//!   answering `found` (with the row), `missing` or `ambiguous`.
//! * `keyed_list`: the entry accounting and every entry no declaration
//!   consumed. `--field` adds a `tunings` entry per declaration, resolving
//!   the looked-up value against the declared width, signedness and approved
//!   range (`known`), or reporting `missing`, `ambiguous` or `refused` with
//!   the [`TuningOutcome`] code.
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` the file was read
//! and every requested lookup and declaration resolved; `2` invalid input (no
//! `--file`, an unreadable path, a path that is not a file, a malformed
//! declaration, a request the member's shape cannot answer — a `--string`
//! against a keyed list or a `--field` against a PE image, which is refused
//! rather than silently dropped — a bad `--install-sha256`); `3` the bytes
//! were refused — an
//! unrouted member, a dialect no F12-C consumer reads, a malformed or hostile
//! PE image (a resource offset outside its table is a structured refusal,
//! never a platform load), a document the reader refused — or the file read
//! but a requested lookup or declaration did not resolve; `1` a runtime
//! failure. A refusal is never reported as success.
//!
//! The `pe_resources` path is the AC03 consumer: the image is handed to the
//! bounded, cycle-checked reader as bytes, and this command has no
//! dynamic-loading surface at all. The string path never loads the DLL it
//! inspects.
//!
//! A loose `--file` belongs to no fingerprinted installation, so without
//! `--install-sha256` its provenance carries
//! [`UNAFFILIATED_INSTALL_SHA256`], a documented sentinel rather than an
//! invented digest.

use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use cs_content::config::{
    ConfigDocument, ConfigError, FieldBinding, FieldSpec, StringCatalog, StringCatalogError,
    StringLookup, TuningOutcome, TuningReport, ValueWidth, resolve_tunings,
};
use cs_formats::ParseContext;
use cs_formats::text::{TextDialect, dialect_for_member};
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;
use cs_types::install::RelativePath;

/// The report format version.
pub const CONFIG_REPORT_VERSION: &str = "cs-inspect-config/1";

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE).
const EXIT_INVALID_INPUT: u8 = 2;

/// Exit code for a failed validation or a reported anomaly.
const EXIT_REFUSED: u8 = 3;

/// Exit code for a runtime failure.
const EXIT_RUNTIME: u8 = 1;

/// The installation fingerprint a loose `--file` carries when the caller
/// supplies no `--install-sha256`.
///
/// A loose file belongs to no fingerprinted installation. Recording the
/// sentinel — the 32 ASCII bytes of `cs-inspect:unaffiliated-install0` — says
/// so in the report instead of inventing a digest that could be mistaken for
/// a real installation fingerprint. A caller that knows the installation
/// passes `--install-sha256`.
pub const UNAFFILIATED_INSTALL_SHA256: ContentHash =
    ContentHash::from_bytes(*b"cs-inspect:unaffiliated-install0");

/// Why the `config` command could not produce a report.
#[derive(Debug)]
pub enum ConfigCommandError {
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
    /// No observed dialect covers the member.
    UnknownDialect {
        /// Every container spelling routing was attempted with, in the
        /// order it was tried.
        candidates: Vec<String>,
        /// The member key, `None` for a loose file.
        member: Option<String>,
    },
    /// The member's dialect has no F12-C consumer.
    UnsupportedDialect {
        /// The dialect the inventory routed the member to.
        dialect: TextDialect,
    },
    /// The PE image was refused by the bounded reader.
    Catalog(StringCatalogError),
    /// The keyed-list document was refused.
    Document(ConfigError),
}

impl fmt::Display for ConfigCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::Read { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::NotAFile(path) => write!(f, "{} is not a regular file", path.display()),
            Self::Output { path, source } => {
                write!(f, "cannot write report to {}: {source}", path.display())
            }
            Self::UnknownDialect { candidates, member } => {
                let tried = candidates
                    .iter()
                    .map(|candidate| format!("{candidate:?}"))
                    .collect::<Vec<String>>()
                    .join(" or ");
                match member {
                    Some(member) => write!(
                        f,
                        "no observed dialect covers member {member:?} of {tried}; an extension \
                         alone routes nothing"
                    ),
                    None => write!(
                        f,
                        "no observed dialect covers {tried}; an extension alone routes nothing"
                    ),
                }
            }
            Self::UnsupportedDialect { dialect } => write!(
                f,
                "dialect {} has no F12-C consumer; this command reads keyed lists (typed tuning) \
                 and PE resources (localized strings)",
                dialect.code()
            ),
            Self::Catalog(error) => write!(f, "config[{}]: {error}", error.code()),
            Self::Document(error) => write!(f, "config[{}]: {error}", error.code()),
        }
    }
}

impl std::error::Error for ConfigCommandError {}

/// One requested `--string <id>[:<language>]` lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StringQuery {
    id: u32,
    language: Option<u32>,
}

/// One requested `--field` declaration, owned so it can be borrowed as a
/// [`FieldBinding`] while the document is resolved.
#[derive(Clone, Debug, PartialEq)]
struct FieldDecl {
    consumer: String,
    section: Option<Vec<u8>>,
    key: Vec<u8>,
    index: usize,
    spec: FieldSpec,
}

/// Parsed `config` arguments.
#[derive(Debug, Default)]
struct ConfigArgs {
    file: Option<PathBuf>,
    cs_path: Option<PathBuf>,
    container: Option<String>,
    member: Option<String>,
    install_sha256: Option<ContentHash>,
    strings: Vec<StringQuery>,
    fields: Vec<FieldDecl>,
    out: Option<PathBuf>,
}

fn parse_u32(text: &str, flag: &str) -> Result<u32, ConfigCommandError> {
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse::<u32>(),
    };
    parsed.map_err(|_| {
        ConfigCommandError::Usage(format!(
            "cs-inspect config: {flag} value {text:?} is not a 32-bit number"
        ))
    })
}

fn parse_string_query(text: &str) -> Result<StringQuery, ConfigCommandError> {
    let (id, language) = match text.split_once(':') {
        Some((id, language)) => (id, Some(language)),
        None => (text, None),
    };
    Ok(StringQuery {
        id: parse_u32(id, "--string id")?,
        language: language
            .map(|value| parse_u32(value, "--string language"))
            .transpose()?,
    })
}

fn parse_width(text: &str) -> Option<ValueWidth> {
    match text {
        "8" => Some(ValueWidth::Bits8),
        "16" => Some(ValueWidth::Bits16),
        "32" => Some(ValueWidth::Bits32),
        "64" => Some(ValueWidth::Bits64),
        "f32" => Some(ValueWidth::Float32),
        "f64" => Some(ValueWidth::Float64),
        _ => None,
    }
}

fn parse_field_decl(text: &str) -> Result<FieldDecl, ConfigCommandError> {
    let refuse = || {
        ConfigCommandError::Usage(format!(
            "cs-inspect config: --field must be \
             <consumer>=<section>:<key>:<index>:<width>:<signed>, got {text:?}"
        ))
    };
    let (consumer, rest) = text.split_once('=').ok_or_else(refuse)?;
    if consumer.is_empty() {
        return Err(refuse());
    }
    let parts: Vec<&str> = rest.split(':').collect();
    let [section, key, index, width, signed] = parts.as_slice() else {
        return Err(refuse());
    };
    if key.is_empty() {
        return Err(refuse());
    }
    let index = index.parse::<usize>().map_err(|_| {
        ConfigCommandError::Usage(format!(
            "cs-inspect config: --field index {index:?} is not a non-negative integer"
        ))
    })?;
    let width = parse_width(width).ok_or_else(refuse)?;
    let spec = if width.is_float() {
        FieldSpec::float(width)
    } else {
        match *signed {
            "signed" => FieldSpec::integer(width, true),
            "unsigned" => FieldSpec::integer(width, false),
            _ => return Err(refuse()),
        }
    };
    Ok(FieldDecl {
        consumer: consumer.to_owned(),
        section: (!section.is_empty()).then(|| section.as_bytes().to_vec()),
        key: key.as_bytes().to_vec(),
        index,
        spec,
    })
}

fn parse_config_args(args: &[String]) -> Result<ConfigArgs, ConfigCommandError> {
    let mut parsed = ConfigArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        let value_flag = matches!(
            flag,
            "--file"
                | "--cs-path"
                | "--container"
                | "--member"
                | "--install-sha256"
                | "--out"
                | "--string"
                | "--field"
        );
        if !value_flag {
            return Err(ConfigCommandError::Usage(format!(
                "cs-inspect config: unsupported argument {flag:?}; expected --file, --cs-path, \
                 --container, --member, --install-sha256, --string, --field or --out"
            )));
        }
        let Some(value) = cursor.next() else {
            return Err(ConfigCommandError::Usage(format!(
                "cs-inspect config: {flag} needs a value"
            )));
        };
        match flag {
            "--file" => parsed.file = Some(PathBuf::from(value)),
            "--cs-path" => parsed.cs_path = Some(PathBuf::from(value)),
            "--container" => parsed.container = Some(value.clone()),
            "--member" => parsed.member = Some(value.clone()),
            "--out" => parsed.out = Some(PathBuf::from(value)),
            "--install-sha256" => {
                parsed.install_sha256 = Some(ContentHash::from_hex(value).map_err(|error| {
                    ConfigCommandError::Usage(format!(
                        "cs-inspect config: --install-sha256 is not a lowercase sha256 hex \
                         digest: {error}"
                    ))
                })?);
            }
            "--string" => parsed.strings.push(parse_string_query(value)?),
            _ => parsed.fields.push(parse_field_decl(value)?),
        }
    }
    Ok(parsed)
}

/// Everything one `config` run produced.
#[derive(Debug)]
pub struct ConfigRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The JSON report, when the file was read.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
}

impl ConfigRun {
    fn failed(exit_code: u8, error: &ConfigCommandError) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![error.to_string()],
        }
    }
}

/// Every container spelling one `--file` is routed under, most specific
/// first (see the module documentation).
///
/// An explicit `--container` is the only candidate, because an override names
/// the member and no inference may contradict it. Otherwise the file's
/// installation-relative spelling comes first, because that is how the
/// inventory writes every loose rule ([`MemberRule::Loose`]), and the file
/// name last, because that is what a loose export carries. A file name that
/// already *is* the installation-relative spelling (`<root>/strings.dll`) is
/// listed once. A name that differs from it only in letter case stays a
/// second candidate: [`dialect_for_member`] matches ASCII case-insensitively,
/// so the more specific candidate already routes and the case-differing one
/// is never reached.
fn routing_candidates(
    container: Option<&str>,
    file: &Path,
    label: &str,
    root: Option<&Path>,
) -> Vec<String> {
    if let Some(container) = container {
        return vec![container.to_owned()];
    }
    let mut candidates = Vec::new();
    if let Some(spelling) = root.and_then(|root| installation_relative_spelling(root, file)) {
        candidates.push(spelling);
    }
    if !candidates.iter().any(|candidate| candidate == label) {
        candidates.push(label.to_owned());
    }
    candidates
}

/// The installation-relative spelling of `file`, or `None` when `file` does
/// not lie under `root`.
///
/// Root components are matched ASCII case-insensitively and the remainder
/// keeps its own case, joined with `/`: the same rule (and the same reason)
/// as `cs_assets::install::relative_spelling`, so a `CS_GAME_DIR` whose
/// letter case disagrees with the `--file` path still routes the same member.
/// A relative spelling that is absolute, empty, or carries a `.`/`..`
/// component is no spelling at all, so it routes nothing.
///
/// A root with no components — `""`, or `.` on a relative `--file` — is the
/// empty prefix, so a relative `--file` is its own installation-relative
/// spelling and an absolute one begins with a root component whose `as_os_str`
/// is `/`, which makes the joined spelling absolute and therefore invalid.
/// Either way the empty root contributes nothing an absolute path can use, so
/// routing degrades to the file name exactly as if no root were selected.
fn installation_relative_spelling(root: &Path, file: &Path) -> Option<String> {
    fn parts(path: &Path) -> Vec<&OsStr> {
        path.components()
            .filter(|component| !matches!(component, Component::CurDir))
            .map(|component| component.as_os_str())
            .collect()
    }
    let (root_parts, file_parts) = (parts(root), parts(file));
    if file_parts.len() <= root_parts.len()
        || !file_parts
            .iter()
            .zip(root_parts.iter())
            .all(|(part, prefix)| {
                part.as_encoded_bytes()
                    .eq_ignore_ascii_case(prefix.as_encoded_bytes())
            })
    {
        return None;
    }
    let spelling: Option<String> = file_parts[root_parts.len()..]
        .iter()
        .map(|component| component.to_str().map(str::to_owned))
        .collect::<Option<Vec<String>>>()
        .map(|components| components.join("/"));
    let spelling = RelativePath::new(spelling.as_deref()?).ok()?;
    Some(spelling.as_str().to_owned())
}

/// Runs the `config` command and returns its exit code.
///
/// `CS_GAME_DIR` selects the installation whose relative spelling routes a
/// file inside it; `--cs-path` wins over it, as in every other command of
/// this binary.
pub fn config_command(args: &[String]) -> ExitCode {
    let run = config_command_result(args, std::env::var_os("CS_GAME_DIR"));
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote config report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// The body of [`config_command`].
///
/// `env_cs_path` is the `CS_GAME_DIR` value, passed in so a test decides the
/// environment instead of inheriting the machine's.
pub fn config_command_result(args: &[String], env_cs_path: Option<OsString>) -> ConfigRun {
    let parsed = match parse_config_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return ConfigRun::failed(EXIT_INVALID_INPUT, &error),
    };
    let Some(path) = parsed.file.as_ref() else {
        return ConfigRun::failed(
            EXIT_INVALID_INPUT,
            &ConfigCommandError::Usage(
                "cs-inspect config: --file <path> is required; it must be a surveyed \
                 configuration member or PE resource image"
                    .to_owned(),
            ),
        );
    };
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) => {
            return ConfigRun::failed(
                EXIT_INVALID_INPUT,
                &ConfigCommandError::Read {
                    path: path.clone(),
                    source,
                },
            );
        }
    };
    if !metadata.is_file() {
        return ConfigRun::failed(
            EXIT_INVALID_INPUT,
            &ConfigCommandError::NotAFile(path.clone()),
        );
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(source) => {
            return ConfigRun::failed(
                EXIT_RUNTIME,
                &ConfigCommandError::Read {
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

    // The selected installation root: `--cs-path` wins over `CS_GAME_DIR`, the
    // same precedence every other command of this binary uses. Only the
    // routing depends on it, so a root that does not exist costs nothing.
    let root = parsed
        .cs_path
        .as_deref()
        .or_else(|| env_cs_path.as_deref().map(Path::new));
    let candidates = routing_candidates(parsed.container.as_deref(), path, &label, root);
    let member = parsed.member.clone();
    let Some((container, dialect)) = candidates.iter().find_map(|candidate| {
        dialect_for_member(candidate, member.as_deref()).map(|dialect| (candidate.clone(), dialect))
    }) else {
        return ConfigRun::failed(
            EXIT_REFUSED,
            &ConfigCommandError::UnknownDialect { candidates, member },
        );
    };
    let install_sha256 = parsed.install_sha256.unwrap_or(UNAFFILIATED_INSTALL_SHA256);
    // A member's digest is the bytes read; a loose file is its own source, so
    // there is no member digest to claim.
    let member_sha256 = member.as_ref().map(|_| cs_assets::install::sha256(&bytes));
    let source = match SourceSpan::new(
        install_sha256,
        &container,
        member.as_deref(),
        0,
        bytes.len() as u64,
        member_sha256,
    ) {
        Ok(source) => source,
        Err(error) => {
            return ConfigRun::failed(
                EXIT_INVALID_INPUT,
                &ConfigCommandError::Usage(format!(
                    "cs-inspect config: cannot build provenance: {error}"
                )),
            );
        }
    };

    // Each request shape addresses exactly one consumer: `--string` a PE
    // image's string catalog, `--field` a keyed list's document. A request the
    // member's shape cannot answer is a malformed request, so it is refused
    // rather than silently dropped: a run that ignored it and exited 0 would
    // report success for a lookup that never happened.
    let mismatched = match dialect {
        TextDialect::PeResources if !parsed.fields.is_empty() => {
            Some("--field declares a keyed-list field; a PE resource image has none")
        }
        TextDialect::KeyedList if !parsed.strings.is_empty() => {
            Some("--string looks up a PE resource id; a keyed list has none")
        }
        _ => None,
    };
    if let Some(message) = mismatched {
        return ConfigRun::failed(
            EXIT_INVALID_INPUT,
            &ConfigCommandError::Usage(format!("cs-inspect config: {message}")),
        );
    }

    match dialect {
        TextDialect::PeResources => run_pe_resources(&parsed, source, &bytes, &label),
        TextDialect::KeyedList => run_keyed_list(&parsed, source, &bytes, &label),
        other => ConfigRun::failed(
            EXIT_REFUSED,
            &ConfigCommandError::UnsupportedDialect { dialect: other },
        ),
    }
}

/// The `pe_resources` path: read the image as inert data into a
/// [`StringCatalog`] and answer the requested lookups.
fn run_pe_resources(
    parsed: &ConfigArgs,
    source: SourceSpan,
    bytes: &[u8],
    label: &str,
) -> ConfigRun {
    let mut context = ParseContext::with_defaults(label.to_owned());
    let catalog = match StringCatalog::read(&mut context, source, bytes) {
        Ok(catalog) => catalog,
        Err(error) => {
            return ConfigRun::failed(EXIT_REFUSED, &ConfigCommandError::Catalog(error));
        }
    };
    let (report, resolved) = pe_report_json(&catalog, &parsed.strings);
    let mut diagnostics = Vec::new();
    for unresolved in &resolved.unresolved {
        diagnostics.push(format!("{label}: string {unresolved}"));
    }
    finish(
        report,
        if resolved.all { 0 } else { EXIT_REFUSED },
        parsed.out.as_deref(),
        diagnostics,
    )
}

/// The `keyed_list` path: read the document and resolve every declared field.
fn run_keyed_list(parsed: &ConfigArgs, source: SourceSpan, bytes: &[u8], label: &str) -> ConfigRun {
    let mut context = ParseContext::with_defaults(label.to_owned());
    let mut document = match ConfigDocument::read(&mut context, source, bytes) {
        Ok(document) => document,
        Err(error) => {
            return ConfigRun::failed(EXIT_REFUSED, &ConfigCommandError::Document(error));
        }
    };
    let bindings: Vec<FieldBinding<'_>> = parsed
        .fields
        .iter()
        .map(|decl| FieldBinding {
            consumer: &decl.consumer,
            section: decl.section.as_deref(),
            key: &decl.key,
            index: decl.index,
            spec: decl.spec,
        })
        .collect();
    let report = resolve_tunings(&mut document, &bindings);
    let mut diagnostics = Vec::new();
    for entry in report.failures() {
        diagnostics.push(format!(
            "{label}: {} did not resolve: {}",
            entry.binding.consumer,
            outcome_code(entry.outcome)
        ));
    }
    let all_known = report.all_known();
    let json = keyed_report_json(&document, &report);
    finish(
        json,
        if all_known { 0 } else { EXIT_REFUSED },
        parsed.out.as_deref(),
        diagnostics,
    )
}

/// Writes the report to `--out` if one was given, else returns it to print.
fn finish(
    report: String,
    exit_code: u8,
    out: Option<&Path>,
    diagnostics: Vec<String>,
) -> ConfigRun {
    match out {
        Some(path) => match write_report(path, &report) {
            Ok(()) => ConfigRun {
                exit_code,
                report: Some(report),
                out: Some(path.to_path_buf()),
                diagnostics,
            },
            Err(source) => {
                let mut diagnostics = diagnostics;
                diagnostics.push(
                    ConfigCommandError::Output {
                        path: path.to_path_buf(),
                        source,
                    }
                    .to_string(),
                );
                ConfigRun {
                    exit_code: EXIT_RUNTIME,
                    report: None,
                    out: None,
                    diagnostics,
                }
            }
        },
        None => ConfigRun {
            exit_code,
            report: Some(report),
            out: None,
            diagnostics,
        },
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

/// Whether every requested lookup resolved.
struct ResolvedLookups {
    all: bool,
    unresolved: Vec<String>,
}

/// Renders the `pe_resources` report and answers the requested lookups.
fn pe_report_json(catalog: &StringCatalog, queries: &[StringQuery]) -> (String, ResolvedLookups) {
    let source = catalog.source();
    let layout = catalog.resources().layout();
    let accounting = catalog.accounting();
    let languages: Vec<String> = catalog.languages().iter().map(u32::to_string).collect();
    let rows: Vec<String> = catalog
        .rows()
        .iter()
        .map(|row| {
            format!(
                "{{\"id\":{},\"language\":{},\"code_page\":{},\"text\":{},\"code_units\":[{}],\
                 \"span\":{}}}",
                row.id,
                row.language,
                row.code_page,
                opt_str(row.text.as_deref()),
                row.code_units
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<String>>()
                    .join(","),
                span_json(&row.span),
            )
        })
        .collect();
    let resource_directory = layout.resource_directory().map_or_else(
        || "null".to_owned(),
        |directory| {
            format!(
                "{{\"virtual_address\":{},\"size\":{}}}",
                directory.virtual_address, directory.size
            )
        },
    );

    let mut unresolved = Vec::new();
    let mut all = true;
    let lookups: Vec<String> = queries
        .iter()
        .map(|query| {
            let (outcome, body) = match catalog.resolve(query.id, query.language) {
                StringLookup::Missing => {
                    all = false;
                    unresolved.push(format!(
                        "id {} language {} is missing",
                        query.id,
                        opt_num(query.language.map(u64::from))
                    ));
                    (
                        "missing",
                        "\"text\":null,\"code_page\":null,\"span\":null".to_owned(),
                    )
                }
                StringLookup::Ambiguous(count) => {
                    all = false;
                    unresolved.push(format!(
                        "id {} language {} is ambiguous ({count})",
                        query.id,
                        opt_num(query.language.map(u64::from))
                    ));
                    (
                        "ambiguous",
                        "\"text\":null,\"code_page\":null,\"span\":null".to_owned(),
                    )
                }
                StringLookup::Found(row) => (
                    "found",
                    format!(
                        "\"text\":{},\"code_page\":{},\"span\":{}",
                        opt_str(row.text.as_deref()),
                        row.code_page,
                        span_json(&row.span),
                    ),
                ),
            };
            let count = match catalog.resolve(query.id, query.language) {
                StringLookup::Ambiguous(count) => count,
                _ => 0,
            };
            format!(
                "{{\"id\":{},\"language\":{},\"outcome\":{},\"count\":{},{}}}",
                query.id,
                opt_num(query.language.map(u64::from)),
                jstr(outcome),
                count,
                body,
            )
        })
        .collect();

    let report = format!(
        "{{\"version\":{},\"source\":{},\"dialect\":{},\"kind\":\"pe_resources\",\
         \"pe_resources\":{{\"machine\":{},\"optional_magic\":{},\"headers_size\":{},\
         \"section_count\":{},\"resource_directory\":{},\"accounting\":{{\"strings\":{},\
         \"undecodable\":{},\"other_leaves\":{},\"duplicate_ids\":{}}},\"languages\":[{}],\
         \"strings\":[{}]}},\"lookups\":[{}],\"tunings\":[]}}\n",
        jstr(CONFIG_REPORT_VERSION),
        source_json(source),
        jstr(TextDialect::PeResources.code()),
        layout.machine(),
        layout.optional_magic(),
        layout.headers_size(),
        layout.sections().len(),
        resource_directory,
        accounting.strings,
        accounting.undecodable,
        accounting.other_leaves,
        accounting.duplicate_ids,
        languages.join(","),
        rows.join(","),
        lookups.join(","),
    );
    (report, ResolvedLookups { all, unresolved })
}

/// Renders the `keyed_list` report.
fn keyed_report_json(document: &ConfigDocument, report: &TuningReport<'_>) -> String {
    let source = document.source();
    let accounting = document.accounting();
    let consumed: Vec<String> = document
        .unconsumed()
        .map(|(node, entry)| {
            format!(
                "{{\"line\":{},\"section\":{},\"key\":{}}}",
                node.line,
                opt_str(entry.section.as_deref().map(bytes_text).as_deref()),
                jstr(&bytes_text(&entry.key)),
            )
        })
        .collect();
    let tunings: Vec<String> = report
        .resolved()
        .iter()
        .map(|entry| {
            let binding = entry.binding;
            let (outcome, code, body) = match entry.outcome {
                TuningOutcome::Known(tuning) => (
                    "known",
                    "null".to_owned(),
                    format!(
                        "\"value\":{},\"signed\":{},\"unsigned\":{},\"unit\":{},\"line\":{}",
                        float_json(tuning.value),
                        tuning
                            .signed
                            .map_or_else(|| "null".to_owned(), |value| value.to_string()),
                        opt_num(tuning.unsigned),
                        jstr(tuning.unit),
                        tuning.line,
                    ),
                ),
                TuningOutcome::Missing => ("missing", "null".to_owned(), String::new()),
                TuningOutcome::Ambiguous(count) => {
                    ("ambiguous", "null".to_owned(), format!("\"count\":{count}"))
                }
                TuningOutcome::Refused(error) => ("refused", jstr(error.code()), String::new()),
            };
            let body = if body.is_empty() {
                String::new()
            } else {
                format!(",{body}")
            };
            format!(
                "{{\"consumer\":{},\"section\":{},\"key\":{},\"index\":{},\"outcome\":{},\
                 \"code\":{}{}}}",
                jstr(binding.consumer),
                opt_str(binding.section.map(bytes_text).as_deref()),
                jstr(&bytes_text(binding.key)),
                binding.index,
                jstr(outcome),
                code,
                body,
            )
        })
        .collect();
    format!(
        "{{\"version\":{},\"source\":{},\"dialect\":{},\"kind\":\"keyed_list\",\
         \"keyed_list\":{{\"accounting\":{{\"entries\":{},\"consumed\":{},\"unconsumed\":{},\
         \"unclassified_lines\":{},\"unsplit_values\":{}}},\"unconsumed\":[{}]}},\
         \"lookups\":[],\"tunings\":[{}]}}\n",
        jstr(CONFIG_REPORT_VERSION),
        source_json(source),
        jstr(TextDialect::KeyedList.code()),
        accounting.entries,
        accounting.consumed,
        accounting.unconsumed,
        accounting.unclassified_lines,
        accounting.unsplit_values,
        consumed.join(","),
        tunings.join(","),
    )
}
/// The stable code of a tuning outcome, for a diagnostic.
fn outcome_code(outcome: TuningOutcome) -> &'static str {
    match outcome {
        TuningOutcome::Known(_) => "known",
        TuningOutcome::Missing => "missing",
        TuningOutcome::Ambiguous(_) => "ambiguous",
        TuningOutcome::Refused(error) => error.code(),
    }
}

/// A [`SourceSpan`] as `{install_sha256, container, member, offset, length}`.
fn source_json(source: &SourceSpan) -> String {
    format!(
        "{{\"install_sha256\":{},\"container\":{},\"member\":{},\"offset\":{},\"length\":{}}}",
        jstr(&source.install_sha256().to_hex()),
        jstr(source.container_path()),
        opt_str(source.member_key()),
        source.offset(),
        source.length(),
    )
}

/// A row's span as `{offset, length}`.
fn span_json(span: &SourceSpan) -> String {
    format!(
        "{{\"offset\":{},\"length\":{}}}",
        span.offset(),
        span.length()
    )
}

/// A `u32` option as a JSON number or `null`.
fn opt_num(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_owned(), |number| number.to_string())
}

/// A finite `f64` as a JSON number.
fn float_json(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn opt_str(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), jstr)
}

/// Bytes as lossless text: printable ASCII verbatim, every other byte as
/// `\xNN`. Identifiers keep their exact bytes; nothing is decoded as a code
/// page this stage has not measured.
fn bytes_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for &byte in bytes {
        if (0x20..0x7F).contains(&byte) && byte != b'\\' {
            text.push(char::from(byte));
        } else {
            let _ = write!(text, "\\x{byte:02x}");
        }
    }
    text
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use cs_formats::RT_STRING;
    use cs_formats::text::{MemberRule, TEXT_DIALECT_INVENTORY};

    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable fixture directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f12-c-{label}-{}-{}",
                std::process::id(),
                NEXT_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("the fixture root is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs are created");
            fs::write(&path, bytes).expect("fixture bytes are written");
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

    // ------------------------------------------------ authored PE fixtures
    //
    // Newly authored: the public PE/COFF layout and the resource-tree shape,
    // with ids, language, code page and text invented for the test. No
    // original byte or string is reproduced.

    const RSRC_RVA: u32 = 0x2000;

    fn fixture_image(rsrc: &[u8]) -> Vec<u8> {
        let header_end = 0x80 + 4 + 20 + 224 + 40;
        let raw = (header_end + 0x1ff) & !0x1ff;
        let mut out = vec![0u8; header_end];
        out[0..2].copy_from_slice(b"MZ");
        out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        out[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84;
        out[coff..coff + 2].copy_from_slice(&0x014cu16.to_le_bytes());
        out[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        out[coff + 16..coff + 18].copy_from_slice(&224u16.to_le_bytes());
        let optional = coff + 20;
        out[optional..optional + 2].copy_from_slice(&0x010bu16.to_le_bytes());
        out[optional + 92..optional + 96].copy_from_slice(&16u32.to_le_bytes());
        out[optional + 60..optional + 64].copy_from_slice(&(header_end as u32).to_le_bytes());
        out[optional + 112..optional + 116].copy_from_slice(&RSRC_RVA.to_le_bytes());
        out[optional + 116..optional + 120].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        let section = optional + 224;
        out[section..section + 5].copy_from_slice(b".rsrc");
        out[section + 8..section + 12].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        out[section + 12..section + 16].copy_from_slice(&RSRC_RVA.to_le_bytes());
        out[section + 16..section + 20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        out[section + 20..section + 24].copy_from_slice(&(raw as u32).to_le_bytes());
        out.resize(raw + rsrc.len(), 0);
        out[raw..raw + rsrc.len()].copy_from_slice(rsrc);
        out
    }

    fn rsrc_dir(bytes: &mut Vec<u8>, ids: usize) -> usize {
        let at = bytes.len();
        bytes.extend_from_slice(&[0u8; 16]);
        bytes.extend_from_slice(&vec![0u8; ids * 8]);
        bytes[at + 14..at + 16].copy_from_slice(&(ids as u16).to_le_bytes());
        at
    }

    fn rsrc_row(dir: usize, index: usize) -> usize {
        dir + 16 + index * 8
    }

    fn rsrc_id(bytes: &mut [u8], dir: usize, index: usize, id: u32) {
        let row = rsrc_row(dir, index);
        bytes[row..row + 4].copy_from_slice(&id.to_le_bytes());
    }

    fn rsrc_sub(bytes: &mut [u8], dir: usize, index: usize, child: usize) {
        let row = rsrc_row(dir, index);
        bytes[row + 4..row + 8].copy_from_slice(&(0x8000_0000u32 | child as u32).to_le_bytes());
    }

    fn rsrc_data(bytes: &mut [u8], dir: usize, index: usize, entry: usize) {
        let row = rsrc_row(dir, index);
        bytes[row + 4..row + 8].copy_from_slice(&(entry as u32).to_le_bytes());
    }

    /// Sixteen counted UTF-16LE units, the first `entries.len()` non-empty.
    fn string_payload(entries: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for index in 0..16 {
            let text = entries.get(index).copied().unwrap_or("");
            let units: Vec<u16> = text.encode_utf16().collect();
            out.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for unit in units {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        out
    }

    /// A one-block `strings.dll` image: block 1, language 1033, code page
    /// 1252, `alpha` at id 0.
    fn one_block_image() -> Vec<u8> {
        let mut bytes = Vec::new();
        let root = rsrc_dir(&mut bytes, 1);
        rsrc_id(&mut bytes, root, 0, RT_STRING);
        let strings = rsrc_dir(&mut bytes, 1);
        rsrc_sub(&mut bytes, root, 0, strings);
        rsrc_id(&mut bytes, strings, 0, 1);
        let languages = rsrc_dir(&mut bytes, 1);
        rsrc_id(&mut bytes, languages, 0, 1033);
        rsrc_sub(&mut bytes, strings, 0, languages);
        let payload = string_payload(&["alpha"]);
        let rva = RSRC_RVA + bytes.len() as u32;
        bytes.extend_from_slice(&payload);
        let entry = bytes.len();
        bytes.extend_from_slice(&rva.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&1252u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        rsrc_data(&mut bytes, languages, 0, entry);
        fixture_image(&bytes)
    }

    /// A two-block image: `alpha` at id 0 and `beta` at id 16. Its
    /// accounting (32 counted units) differs from [`one_block_image`]'s 16,
    /// so a report says which of two files it read.
    fn two_block_image() -> Vec<u8> {
        let mut bytes = Vec::new();
        let root = rsrc_dir(&mut bytes, 2);
        rsrc_id(&mut bytes, root, 0, RT_STRING);
        let strings = rsrc_dir(&mut bytes, 2);
        rsrc_sub(&mut bytes, root, 0, strings);
        for (index, block) in [1u32, 2].into_iter().enumerate() {
            rsrc_id(&mut bytes, strings, index, block);
            let languages = rsrc_dir(&mut bytes, 1);
            rsrc_id(&mut bytes, languages, 0, 1033);
            rsrc_sub(&mut bytes, strings, index, languages);
            let payload = string_payload(&[if index == 0 { "alpha" } else { "beta" }]);
            let rva = RSRC_RVA + bytes.len() as u32;
            bytes.extend_from_slice(&payload);
            let entry = bytes.len();
            bytes.extend_from_slice(&rva.to_le_bytes());
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&1252u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            rsrc_data(&mut bytes, languages, 0, entry);
        }
        fixture_image(&bytes)
    }

    const LAYOUT: &str = "GOSDATA/ASSETS/crimson.rof";
    const LAYOUT_MEMBER: &str = "ASSETS/LAYOUT.CSV";

    /// The command with no installation selected, so a test routes by
    /// `--cs-path` alone and never inherits the machine's `CS_GAME_DIR`.
    fn config_run(args: &[String]) -> ConfigRun {
        config_command_result(args, None)
    }

    /// A small authored keyed-list member, routed as `LAYOUT.CSV`.
    fn layout_member() -> &'static [u8] {
        b"; authored\r\n[GUN]\r\nRATE=120,9\r\nAMMO=-1\r\nBIG=300\r\nEXTRA=5\r\n"
    }

    fn keyed_run(extra: &[&str]) -> ConfigRun {
        let temp = Temp::new("keyed");
        let path = temp.write("LAYOUT.CSV", layout_member());
        let mut list = vec![
            "--file".to_owned(),
            path.to_str().expect("utf-8 path").to_owned(),
            "--container".to_owned(),
            LAYOUT.to_owned(),
            "--member".to_owned(),
            LAYOUT_MEMBER.to_owned(),
        ];
        list.extend(extra.iter().map(|arg| (*arg).to_owned()));
        config_run(&list)
    }

    /// The `config` command reads a PE image through the bounded reader and
    /// resolves a stable id to its text, language, code page and provenance.
    #[test]
    fn accept_f12_c_config_reports_pe_strings_and_resolves_a_lookup() {
        let temp = Temp::new("pe");
        let path = temp.write("strings.dll", &one_block_image());
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--string",
            "0:1033",
            "--string",
            "1",
        ]));
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a report");
        assert!(report.contains("\"kind\":\"pe_resources\""), "{report}");
        assert!(report.contains("\"dialect\":\"pe.resources\""), "{report}");
        assert!(report.contains("\"install_sha256\":\""), "{report}");
        assert!(report.contains("\"container\":\"strings.dll\""), "{report}");
        assert!(report.contains("\"languages\":[1033]"), "{report}");
        assert!(report.contains("\"strings\":16"), "{report}");
        // The id 0 lookup found `alpha`; id 1 is the present-but-empty unit.
        assert!(report.contains("\"outcome\":\"found\""), "{report}");
        assert!(report.contains("\"text\":\"alpha\""), "{report}");
        assert!(report.contains("\"code_page\":1252"), "{report}");
    }

    /// A malformed resource offset is refused structurally, with a nonzero
    /// exit and no report, and the image is never loaded (AC03 through the
    /// CLI).
    #[test]
    fn accept_f12_c_config_refuses_a_malformed_pe_offset() {
        let mut rsrc = Vec::new();
        let root = rsrc_dir(&mut rsrc, 1);
        rsrc_sub(&mut rsrc, root, 0, root); // a directory that points at itself
        let temp = Temp::new("cycle");
        let path = temp.write("strings.dll", &fixture_image(&rsrc));
        let run = config_run(&args(&["--file", path.to_str().expect("utf-8 path")]));
        assert_eq!(run.exit_code, EXIT_REFUSED);
        assert!(run.report.is_none());
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("config[directory_cycle]")),
            "{:?}",
            run.diagnostics
        );

        // The same shape reads when the offset is sound, so the refusal is
        // the corruption's, not the fixture's.
        let good = Temp::new("good");
        let good_path = good.write("strings.dll", &one_block_image());
        let run = config_run(&args(&["--file", good_path.to_str().expect("utf-8 path")]));
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
    }

    /// The command reads a loose keyed list as the surveyed member it came
    /// from and resolves declared tuning fields, including the AC02 failures.
    #[test]
    fn accept_f12_c_config_resolves_declared_tuning_fields() {
        let run = keyed_run(&[
            "--field",
            "gun.rate=GUN:RATE:0:16:signed",
            "--field",
            "gun.ammo=GUN:AMMO:0:16:unsigned",
            "--field",
            "gun.big=GUN:BIG:0:8:unsigned",
            "--field",
            "gun.absent=GUN:ABSENT:0:8:unsigned",
        ]);
        assert_eq!(run.exit_code, EXIT_REFUSED, "{:?}", run.diagnostics);
        let report = run.report.expect("a report even when a field is refused");
        assert!(report.contains("\"kind\":\"keyed_list\""), "{report}");
        assert!(
            report.contains("\"dialect\":\"text.keyed_list\""),
            "{report}"
        );
        // gun.rate resolves to the signed 120.
        assert!(
            report.contains(
                "\"consumer\":\"gun.rate\",\"section\":\"GUN\",\"key\":\"RATE\",\"index\":0,\
                 \"outcome\":\"known\""
            ),
            "{report}"
        );
        assert!(report.contains("\"value\":120"), "{report}");
        assert!(report.contains("\"signed\":120"), "{report}");
        // The negative and over-wide values are refused, the absent key is
        // missing, and the accounting says what the document holds.
        assert!(report.contains("\"code\":\"negative\""), "{report}");
        assert!(report.contains("\"code\":\"overflow\""), "{report}");
        assert!(report.contains("\"outcome\":\"missing\""), "{report}");
        // Three entries were looked up (one known, two refused), the fourth
        // was not, and the accounting says so.
        assert!(report.contains("\"entries\":4"), "{report}");
        assert!(report.contains("\"consumed\":3"), "{report}");
        assert!(report.contains("\"unconsumed\":1"), "{report}");
        assert!(report.contains("\"key\":\"EXTRA\""), "{report}");
    }

    /// A member no observed rule covers is refused; an extension alone
    /// routes nothing.
    #[test]
    fn accept_f12_c_config_refuses_an_unrouted_member() {
        let temp = Temp::new("unrouted");
        let path = temp.write("mystery.bin", b"not a surveyed member");
        let run = config_run(&args(&["--file", path.to_str().expect("utf-8 path")]));
        assert_eq!(run.exit_code, EXIT_REFUSED);
        assert!(run.report.is_none());
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("no observed dialect covers")),
            "{:?}",
            run.diagnostics
        );
    }

    /// A request that does not resolve is reported, not swallowed: the file
    /// reads, the report is written, and the exit code says the lookup
    /// failed.
    #[test]
    fn accept_f12_c_config_reports_an_unresolved_lookup() {
        let temp = Temp::new("missing");
        let path = temp.write("strings.dll", &one_block_image());
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--string",
            "9000",
        ]));
        assert_eq!(run.exit_code, EXIT_REFUSED);
        let report = run.report.expect("a report for a decoded image");
        assert!(report.contains("\"outcome\":\"missing\""), "{report}");
        assert!(
            run.diagnostics.iter().any(|line| line.contains("id 9000")),
            "{:?}",
            run.diagnostics
        );
    }

    /// A request shape the member cannot answer is refused as invalid input,
    /// never silently dropped: `--field` addresses a keyed list and `--string`
    /// a PE resource image, so asking the other is a malformed request rather
    /// than a lookup that resolved.
    #[test]
    fn accept_f12_c_config_refuses_a_request_of_the_wrong_shape() {
        let temp = Temp::new("pe-field");
        let path = temp.write("strings.dll", &one_block_image());
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--field",
            "gun.rate=GUN:RATE:0:16:signed",
        ]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.report.is_none());
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("a PE resource image has none")),
            "{:?}",
            run.diagnostics
        );

        // The mirror case: a `--string` against a keyed list.
        let run = keyed_run(&["--string", "0:1033"]);
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.report.is_none());
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("a keyed list has none")),
            "{:?}",
            run.diagnostics
        );
    }

    // ------------------------------------------------------- F12-D routing
    //
    // The inventory writes a loose member rule as a path relative to the
    // installation root, so `GOSDATA/ASSETS/BINARIES/langui.dll` is not
    // routable by the file name `langui.dll`. These cases pin the routing
    // order the command applies to reach it, and they are written against
    // authored fixture images: no original byte is used here.

    const LANGUI: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

    /// The localized UI image under a selected installation routes without
    /// `--container`, reports the string accounting and resolves `--string`
    /// against it, and records the installation-relative spelling as its
    /// source container.
    #[test]
    fn accept_f12_d_config_routes_a_localized_image_by_its_installation_path() {
        let temp = Temp::new("langui");
        let root = temp.0.clone();
        let path = temp.write(LANGUI, &one_block_image());
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--cs-path",
            root.to_str().expect("utf-8 root"),
            "--string",
            "0:1033",
        ]));
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a report");
        assert!(report.contains("\"kind\":\"pe_resources\""), "{report}");
        assert!(report.contains("\"dialect\":\"pe.resources\""), "{report}");
        // The provenance names the member the inventory rule spells, not the
        // file name that failed to route it.
        assert!(
            report.contains(&format!("\"container\":\"{LANGUI}\"")),
            "{report}"
        );
        assert!(report.contains("\"member\":null"), "{report}");
        assert!(report.contains("\"languages\":[1033]"), "{report}");
        assert!(report.contains("\"strings\":16"), "{report}");
        assert!(report.contains("\"undecodable\":0"), "{report}");
        assert!(report.contains("\"outcome\":\"found\""), "{report}");
        assert!(report.contains("\"text\":\"alpha\""), "{report}");
    }

    /// `--cs-path` wins over `CS_GAME_DIR`. The selected root derives the
    /// routing spelling, so a root that contains the file under one more
    /// component produces a *different* spelling and the report's source
    /// container says which root routed it.
    #[test]
    fn accept_f12_d_config_cs_path_wins_over_the_environment() {
        // The file lies under both roots: the outer one (the environment's)
        // spells it `inner/GOSDATA/…`, the inner one spells it
        // `GOSDATA/…`. Only the second is a surveyed member.
        let outer = Temp::new("env-root");
        let file = outer.write(&format!("inner/{LANGUI}"), &one_block_image());
        let inner = outer.0.join("inner");
        let env = Some(outer.0.clone().into_os_string());
        let file_arg = file.to_str().expect("utf-8 path");

        // The explicit root wins: its spelling routes, and it is the
        // installation-relative one.
        let mut explicit = args(&["--file", file_arg]);
        explicit.extend(args(&["--cs-path", inner.to_str().expect("utf-8 root")]));
        let run = config_command_result(&explicit, env.clone());
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a report");
        assert!(
            report.contains(&format!("\"container\":\"{LANGUI}\"")),
            "--cs-path routed the file: {report}"
        );

        // Without it the environment's root is the one that derives a
        // spelling, and that spelling covers no observed dialect — so the
        // refusal names it instead of inventing another member.
        let run = config_command_result(&args(&["--file", file_arg]), env);
        assert_eq!(run.exit_code, EXIT_REFUSED);
        assert!(run.report.is_none());
        let refusal = run
            .diagnostics
            .iter()
            .find(|line| line.contains("no observed dialect covers"))
            .expect("a routing refusal");
        assert!(
            refusal.contains(&format!("\"inner/{LANGUI}\"")),
            "the environment's root derived this spelling: {refusal}"
        );
    }

    /// A root that does not contain the file contributes no candidate: the
    /// name is still tried, and the diagnostic names every spelling routing
    /// was attempted with, so a refusal says which member it looked for.
    #[test]
    fn accept_f12_d_config_refuses_a_file_no_rule_covers_and_says_what_it_tried() {
        let temp = Temp::new("unrouted-root");
        let root = temp.0.clone();
        let path = temp.write(
            "GOSDATA/ASSETS/BINARIES/mystery.dll",
            b"not a surveyed member",
        );
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--cs-path",
            root.to_str().expect("utf-8 root"),
        ]));
        assert_eq!(run.exit_code, EXIT_REFUSED);
        assert!(run.report.is_none());
        let refusal = run
            .diagnostics
            .iter()
            .find(|line| line.contains("no observed dialect covers"))
            .expect("a routing refusal");
        assert!(
            refusal.contains("GOSDATA/ASSETS/BINARIES/mystery.dll")
                && refusal.contains("\"mystery.dll\""),
            "both spellings are named, the installation-relative one first: {refusal}"
        );

        // The file outside every selected root is refused the same way, with
        // the name as its only candidate.
        let outside = Temp::new("outside");
        let path = outside.write(LANGUI, &one_block_image());
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--cs-path",
            root.to_str().expect("utf-8 root"),
        ]));
        assert_eq!(run.exit_code, EXIT_REFUSED);
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("no observed dialect covers \"langui.dll\"")),
            "{:?}",
            run.diagnostics
        );
    }

    /// An explicit `--container` is the only candidate: an override names the
    /// member, and the installation-relative spelling never contradicts it.
    #[test]
    fn accept_f12_d_config_keeps_an_explicit_container_over_the_installation_path() {
        let temp = Temp::new("override");
        let root = temp.0.clone();
        let path = temp.write(LANGUI, &two_block_image());
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--cs-path",
            root.to_str().expect("utf-8 root"),
            "--container",
            "strings.dll",
            "--string",
            "16:1033",
        ]));
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a report");
        assert!(report.contains("\"container\":\"strings.dll\""), "{report}");
        // The override read the same bytes through the other member, and the
        // two-block accounting says so: the file is not the one-block image
        // the routing test reads at the same path.
        assert!(report.contains("\"strings\":32"), "{report}");
        assert!(report.contains("\"text\":\"beta\""), "{report}");
    }

    /// The root's letter case does not decide the member. Root components
    /// are matched ASCII case-insensitively — the way installation paths are
    /// keyed — so a root spelled `/VAR/FOLDERS/…` routes a file spelled
    /// `/var/folders/…`, and the routing never looks the root up on disk.
    #[test]
    fn accept_f12_d_config_routes_a_file_whose_root_disagrees_in_case() {
        let temp = Temp::new("case");
        let root = temp.0.clone();
        let path = temp.write(LANGUI, &one_block_image());
        let shouted: PathBuf = root
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_uppercase())
            .collect();
        assert_ne!(
            shouted, root,
            "the fixture root carries letters worth changing"
        );
        let run = config_run(&args(&[
            "--file",
            path.to_str().expect("utf-8 path"),
            "--cs-path",
            shouted.to_str().expect("utf-8 root"),
            "--string",
            "0:1033",
        ]));
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a report");
        // The remainder keeps its own case, so the routed spelling is still
        // the inventory's, not the shouted one.
        assert!(
            report.contains(&format!("\"container\":\"{LANGUI}\"")),
            "{report}"
        );
    }

    /// A root with no components is the empty prefix, which routes nothing
    /// new: a relative `--file` is its own installation-relative spelling,
    /// and an absolute one is refused by `RelativePath` for being absolute,
    /// so the file name is the only candidate left. An exported but empty
    /// `CS_GAME_DIR=` reaches the same place.
    #[test]
    fn accept_f12_d_config_an_empty_root_leaves_the_file_name_routing() {
        let temp = Temp::new("empty-root");
        let path = temp.write(LANGUI, &one_block_image());
        let file = path.to_str().expect("utf-8 path");
        for (source, run) in [
            (
                "--cs-path",
                config_run(&args(&["--file", file, "--cs-path", ""])),
            ),
            (
                "CS_GAME_DIR=",
                config_command_result(&args(&["--file", file]), Some(OsString::new())),
            ),
        ] {
            assert_eq!(
                run.exit_code, EXIT_REFUSED,
                "{source}: {:?}",
                run.diagnostics
            );
            assert!(
                run.diagnostics
                    .iter()
                    .any(|line| line.contains("no observed dialect covers \"langui.dll\"")),
                "{source}: the empty root contributed no other candidate: {:?}",
                run.diagnostics
            );
        }
        // The relative half of the rule is a pure path computation, so it is
        // pinned here rather than by changing the process's directory.
        assert_eq!(
            installation_relative_spelling(Path::new(""), Path::new(LANGUI)).as_deref(),
            Some(LANGUI),
            "an empty root leaves a relative spelling as it is"
        );
        assert_eq!(
            installation_relative_spelling(Path::new(""), &path),
            None,
            "an absolute spelling is not a relative one"
        );
    }

    /// The real installation: each of the three surveyed PE images routes by
    /// its installation-relative spelling with no `--container` override,
    /// reports the recorded accounting, and resolves a recorded id to a row
    /// with a recorded code page and block extent. Structure and ids are
    /// compared; no original text is asserted, printed or kept.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f12_d_retail_config_routes_every_surveyed_pe_image() {
        /// One surveyed image: its installation-relative spelling, the number
        /// of counted string units the F12-B survey recorded, the code page
        /// it recorded, and an id the image is observed to carry.
        struct Surveyed {
            path: &'static str,
            strings: u64,
            code_page: u32,
            /// Other, non-string leaves the image carries.
            other_leaves: u64,
            probe: u32,
        }
        const SURVEY: &[Surveyed] = &[
            Surveyed {
                path: "strings.dll",
                strings: 1_792,
                code_page: 1252,
                other_leaves: 2,
                probe: 101,
            },
            Surveyed {
                path: "GOSDATA/ASSETS/BINARIES/language.dll",
                strings: 48,
                code_page: 0,
                other_leaves: 0,
                probe: 0,
            },
            Surveyed {
                path: "GOSDATA/ASSETS/BINARIES/langui.dll",
                strings: 1_616,
                code_page: 0,
                other_leaves: 0,
                probe: 0,
            },
        ];

        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this test needs the original installation");
        let dir = PathBuf::from(dir);
        assert!(dir.is_dir(), "CS_GAME_DIR is not a directory");

        for expected in SURVEY {
            let file = dir.join(expected.path);
            let length = fs::metadata(&file)
                .unwrap_or_else(|error| panic!("{}: {error}", file.display()))
                .len();
            // The inventory records the length of every rule it matches, and
            // the rule is what routes the file; read the recorded value
            // rather than repeating the survey's number in this test.
            let rule = TEXT_DIALECT_INVENTORY
                .iter()
                .flat_map(|record| record.members.iter())
                .find_map(|rule| match rule {
                    MemberRule::Loose { path, length }
                        if path.eq_ignore_ascii_case(expected.path) =>
                    {
                        Some(*length)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the inventory does not route {}", expected.path));
            assert_eq!(rule, length, "{}: the surveyed length", expected.path);

            let mut list = vec![
                "--file".to_owned(),
                file.to_str().expect("utf-8 path").to_owned(),
            ];
            list.extend(args(&["--string", &format!("{}:1033", expected.probe)]));
            let run = config_command_result(&list, Some(dir.clone().into_os_string()));
            assert_eq!(run.exit_code, 0, "{}: {:?}", expected.path, run.diagnostics);
            let report = run.report.expect("a report");
            assert!(
                report.contains(&format!("\"container\":\"{}\"", expected.path)),
                "{}: the installation-relative spelling is the source",
                expected.path
            );
            assert!(report.contains("\"kind\":\"pe_resources\""), "{report}");
            assert!(report.contains("\"languages\":[1033]"), "{report}");
            assert!(
                report.contains(&format!("\"strings\":{}", expected.strings)),
                "{}: the surveyed string accounting",
                expected.path
            );
            assert!(
                report.contains(&format!("\"other_leaves\":{}", expected.other_leaves)),
                "{}: the surveyed non-string leaves",
                expected.path
            );
            assert!(report.contains("\"undecodable\":0"), "{report}");
            assert!(report.contains("\"duplicate_ids\":0"), "{report}");
            assert!(
                report.contains(&format!("\"code_page\":{}", expected.code_page)),
                "{}: the surveyed code page",
                expected.path
            );
            assert!(report.contains("\"outcome\":\"found\""), "{report}");
        }

        // The mission-title table is reachable by id as well: the row M01-A
        // recorded for the localized M01 title is at id 3480, in the block
        // at byte offset 92088 of 610 bytes. The id is this engine's own
        // numbering (`(block - 1) * 16 + index`, `Documented`); which
        // numbering the original game addresses strings with is #374 and is
        // not settled by this test.
        let file = dir.join("GOSDATA/ASSETS/BINARIES/langui.dll");
        let list = vec![
            "--file".to_owned(),
            file.to_str().expect("utf-8 path").to_owned(),
            "--string".to_owned(),
            "3480:1033".to_owned(),
        ];
        let run = config_command_result(&list, Some(dir.clone().into_os_string()));
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        let report = run.report.expect("a report");
        // The `lookups` array answers the request itself, so the resolved row
        // is read from there rather than from the whole report (which holds
        // every row of the image, most of them empty). The array ends at the
        // `tunings` field that follows it, which a decoded title may contain
        // any character inside, so the closing is not searched for.
        let lookups = report
            .split_once("\"lookups\":[")
            .expect("a lookups array")
            .1;
        let lookups = &lookups[..lookups
            .rfind("\"tunings\":")
            .expect("the tunings field follows the lookups")];
        assert!(
            lookups.contains("\"id\":3480,\"language\":1033"),
            "{lookups}"
        );
        assert!(lookups.contains("\"outcome\":\"found\""), "{lookups}");
        assert!(lookups.contains("\"code_page\":0"), "{lookups}");
        assert!(
            lookups.contains("\"span\":{\"offset\":92088,\"length\":610}"),
            "the recorded block extent of the M01 title row: {lookups}"
        );
        // The row carries a non-empty localized title; it is not quoted, kept
        // or compared here, because no original text belongs in this
        // repository.
        assert!(
            !lookups.contains("\"text\":null") && !lookups.contains("\"text\":\"\""),
            "the recorded row carries the localized title: {lookups}"
        );
    }
}
