//! The `rof` command (F05-C): mount one ROF container into a content
//! session and report what it holds.
//!
//! ```text
//! cs-inspect rof [--cs-path <dir>] --container <spelling>
//!     [--member <spelling>] [--max-decoded-bytes <n>]
//!     [--out <file>] [--export-dir <dir>]
//! ```
//!
//! The installation is discovered (F02-B, which gives the installation
//! fingerprint every span of the report names), the container named by
//! `--container` — a spelling relative to the installation root — is walked
//! by the production ROF reader and mounted as one retail source, and the
//! report lists every member with the stored extent, id, compression bit
//! and digest the mount recorded. `--member` resolves and reads one member
//! through the bounded decoder, and `--export-dir` hands its **decoded**
//! bytes to the explicit private research export of spec F04
//! non-negotiable behavior 5.
//!
//! Neither `--out` nor `--export-dir` may lie inside the installation, and
//! nothing here writes anywhere but those two: a container the reader
//! refuses (a cycle, a name table its records disagree with, an extent
//! past the end of the file) fails before a mount exists, and a member
//! that cannot be decoded — an expansion bomb above
//! `--max-decoded-bytes` — fails before the export writes a byte
//! (spec F05 acceptance AC03).
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` the container
//! mounted (and the requested member was read and exported); `2` invalid
//! input (a bad spelling, a missing flag, an output inside the
//! installation); `3` the content was refused (the container or the
//! member); `4` no installation selected; `1` a runtime failure. The
//! report is written even on exit 3, because it is the evidence of why
//! the command refused. The session is closed before the command returns,
//! on every path.
//!
//! `inside`, `write_report` and `jstr` mirror the private helpers of the
//! `resolve` and `inventory` commands: this task owns `rof.rs` only, so
//! rather than editing those modules the same rules are applied here —
//! an output path outside the installation, an atomic report write and
//! escaped JSON strings.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install::{self, DiscoveryError};
use cs_assets::rof::{
    RofExportError, RofMountError, RofSource, export_rof_member, mount_rof_with_limits,
};
use cs_assets::vfs::{
    ContentSession, ExportDirectory, ExportError, ExportedFile, MountBuilder, ResolveError,
    SessionAsset, SessionBuilder, SessionError,
};
use cs_formats::{RofLimits, RofMemberRead};
use cs_types::asset_id::{
    AssetKey, MAX_LABEL_LEN, MountId, MountNamespace, PRECEDENCE_ORDER_STATUS, PrecedenceClass,
    ResolveContext,
};
use cs_types::install::RelativePath;

/// The report format version.
pub const ROF_REPORT_VERSION: &str = "cs-inspect-rof/1";

/// The namespace [`mount_builder`] mounts the container's members under:
/// keys are spellings inside the container, exactly as the installation
/// mount spells its files.
const ROF_NAMESPACE: &str = "install";

/// Why the `rof` command failed before or around the mount.
#[derive(Debug)]
pub enum RofCommandError {
    /// The command line was malformed.
    Usage(String),
    /// Neither `--cs-path` nor `CS_GAME_DIR` selected an installation.
    MissingInstallation,
    /// Discovery refused the installation.
    Discovery(DiscoveryError),
    /// `--out` or `--export-dir` lies inside the installation.
    OutputInsideInstallation(PathBuf),
    /// A path the command needed could not be read.
    Unreadable {
        /// The path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The report could not be written.
    Output {
        /// The requested path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
}

impl fmt::Display for RofCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::MissingInstallation => write!(
                f,
                "no installation selected: pass --cs-path <dir> or set CS_GAME_DIR"
            ),
            Self::Discovery(error) => write!(f, "{error}"),
            Self::OutputInsideInstallation(path) => write!(
                f,
                "{} lies inside the installation; cs-inspect never writes there",
                path.display()
            ),
            Self::Unreadable { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::Output { path, source } => {
                write!(f, "cannot write report to {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for RofCommandError {}

/// Parsed `rof` arguments.
#[derive(Debug, Default)]
struct RofArgs {
    cs_path: Option<PathBuf>,
    container: Option<String>,
    member: Option<String>,
    max_decoded_bytes: Option<u64>,
    out: Option<PathBuf>,
    export_dir: Option<PathBuf>,
}

fn parse_rof_args(args: &[String]) -> Result<RofArgs, RofCommandError> {
    let mut parsed = RofArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        if !matches!(
            flag,
            "--cs-path"
                | "--container"
                | "--member"
                | "--max-decoded-bytes"
                | "--out"
                | "--export-dir"
        ) {
            return Err(RofCommandError::Usage(format!(
                "cs-inspect rof: unsupported argument {flag:?}; expected --cs-path, \
                 --container, --member, --max-decoded-bytes, --out, --export-dir"
            )));
        }
        let Some(value) = cursor.next() else {
            return Err(RofCommandError::Usage(format!(
                "cs-inspect rof: {flag} needs a value"
            )));
        };
        match flag {
            "--cs-path" => parsed.cs_path = Some(PathBuf::from(value)),
            "--container" => parsed.container = Some(value.clone()),
            "--member" => parsed.member = Some(value.clone()),
            "--out" => parsed.out = Some(PathBuf::from(value)),
            "--export-dir" => parsed.export_dir = Some(PathBuf::from(value)),
            "--max-decoded-bytes" => match value.parse::<u64>() {
                Ok(limit) => parsed.max_decoded_bytes = Some(limit),
                Err(_) => {
                    return Err(RofCommandError::Usage(format!(
                        "cs-inspect rof: --max-decoded-bytes {value:?} must be a u64 byte count"
                    )));
                }
            },
            _ => unreachable!("the check above listed every accepted flag"),
        }
    }
    Ok(parsed)
}

/// Runs the `rof` command and returns its exit code.
pub fn rof_command(args: &[String]) -> ExitCode {
    let run = rof_command_result(args, std::env::var_os("CS_GAME_DIR"));
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote rof report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// Everything one `rof` run produced.
#[derive(Debug)]
pub struct RofRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The JSON report, once the container was located.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
}

impl RofRun {
    fn failed(exit_code: u8, error: &RofCommandError) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![error.to_string()],
        }
    }
}

/// The body of [`rof_command`], with the environment's installation
/// passed in so tests can drive it.
pub fn rof_command_result(args: &[String], env_cs_path: Option<OsString>) -> RofRun {
    let parsed = match parse_rof_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return RofRun::failed(2, &error),
    };
    let Some(container) = parsed.container.clone() else {
        return RofRun::failed(
            2,
            &RofCommandError::Usage(
                "cs-inspect rof: --container <spelling relative to the installation> is required"
                    .to_owned(),
            ),
        );
    };
    if parsed.export_dir.is_some() && parsed.member.is_none() {
        return RofRun::failed(
            2,
            &RofCommandError::Usage(
                "cs-inspect rof: --export-dir needs --member, so a known member is exported"
                    .to_owned(),
            ),
        );
    }
    if let Err(reason) = RelativePath::new(&container) {
        return RofRun::failed(
            2,
            &RofCommandError::Usage(format!(
                "cs-inspect rof: invalid --container {container:?}: {reason}"
            )),
        );
    }
    let cs_path = parsed.cs_path.clone().or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return RofRun::failed(4, &RofCommandError::MissingInstallation);
    };
    for output in [&parsed.out, &parsed.export_dir].into_iter().flatten() {
        if inside(output, &cs_path) {
            return RofRun::failed(
                2,
                &RofCommandError::OutputInsideInstallation(output.clone()),
            );
        }
    }

    let found = match install::discover(&cs_path) {
        Ok(found) => found,
        Err(error) => return RofRun::failed(1, &RofCommandError::Discovery(error)),
    };
    // The spelling is relative with no `..`, so the join cannot leave the
    // installation lexically; it must still name a regular file rather
    // than a directory or a link out of it.
    let host_path = cs_path.join(&container);
    match fs::symlink_metadata(&host_path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return RofRun::failed(
                2,
                &RofCommandError::Usage(format!(
                    "cs-inspect rof: --container {container:?} is not a regular file of the \
                     installation"
                )),
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return RofRun::failed(
                2,
                &RofCommandError::Usage(format!(
                    "cs-inspect rof: the installation holds no container {container:?}"
                )),
            );
        }
        Err(source) => {
            return RofRun::failed(
                1,
                &RofCommandError::Unreadable {
                    path: host_path,
                    source,
                },
            );
        }
    }
    // The leaf is a regular file, but a *directory* link on the way to it
    // would have been followed out of the installation, and the report
    // would then claim a fingerprint for bytes that are not the
    // installation's. Resolve every link and require the file to still
    // live below this installation — the rule `mount_directory` applies
    // to a tree (spec F04 non-negotiable behavior 1), here for the one
    // container the command reads.
    match (fs::canonicalize(&cs_path), fs::canonicalize(&host_path)) {
        (Ok(root), Ok(resolved)) if resolved.starts_with(&root) => {}
        (Ok(_), Ok(_)) => {
            return RofRun::failed(
                2,
                &RofCommandError::Usage(format!(
                    "cs-inspect rof: --container {container:?} resolves outside the installation \
                     through a symbolic link; cs-inspect never reads through links"
                )),
            );
        }
        (_, Err(source)) => {
            return RofRun::failed(
                1,
                &RofCommandError::Unreadable {
                    path: host_path,
                    source,
                },
            );
        }
        (Err(source), Ok(_)) => {
            return RofRun::failed(
                1,
                &RofCommandError::Unreadable {
                    path: cs_path,
                    source,
                },
            );
        }
    }

    let limits = RofLimits::new(
        parsed
            .max_decoded_bytes
            .unwrap_or(RofLimits::DEFAULT_MAX_DECODED_BYTES),
    );
    let context = ResolveContext::new(install::fingerprint(&found.manifest));
    let mut builder = SessionBuilder::new(context);
    let mut diagnostics: Vec<String> = Vec::new();
    let mut exit_code: u8 = 0;

    // The container either mounts or is refused; a refusal leaves the
    // session untouched and still produces a report that says why.
    let (source, mount_failure) =
        match mount_rof_with_limits(mount_builder(&container), &host_path, limits) {
            Ok(mounted) => match builder.mount(mounted.mount) {
                Ok(_) => (Some(mounted.source), None),
                Err(error) => {
                    diagnostics.push(error.to_string());
                    exit_code = 1;
                    (None, Some(MountFailure::session(&error)))
                }
            },
            Err(error) => {
                diagnostics.push(error.to_string());
                exit_code = mount_exit_code(&error);
                (None, Some(MountFailure::mount(&error)))
            }
        };

    let session = builder.open();
    let mut read = ReadReport::skipped();
    let mut export = ExportReport::skipped();
    if let Some(member) = parsed.member.as_deref() {
        let outcome = inspect_member(
            &session,
            source.as_ref(),
            member,
            parsed.export_dir.as_deref(),
            &mut diagnostics,
        );
        match outcome {
            // `--member` itself was invalid.
            Err((code, message)) => {
                diagnostics.push(message.clone());
                read = ReadReport::refused("invalid_member", None, message);
                exit_code = exit_code.max(code);
            }
            // The container did not mount: nothing exists to read, and the
            // mount's own refusal already chose the exit code — 3 when the
            // content was refused, 1 when the container could not be read
            // at all (CLI-EVIDENCE: a runtime failure is never reported as
            // a validation failure). Requesting a member must not change
            // it, so this outcome keeps the code the mount produced.
            Ok(None) => {}
            Ok(Some(outcome)) => {
                read = outcome.read;
                export = outcome.export;
                exit_code = exit_code.max(outcome.exit_code);
            }
        }
    }

    let report = rof_report_json(
        &session,
        &cs_path,
        &container,
        source.as_ref(),
        mount_failure.as_ref(),
        &read,
        &export,
    );
    let generation = session.generation();
    let teardown = session.close();
    debug_assert_eq!(teardown.generation, generation);

    let out = match &parsed.out {
        Some(out) => {
            if let Err(source) = write_report(out, &report) {
                let error = RofCommandError::Output {
                    path: out.clone(),
                    source,
                };
                diagnostics.push(error.to_string());
                return RofRun {
                    exit_code: 1,
                    report: None,
                    out: None,
                    diagnostics,
                };
            }
            Some(out.clone())
        }
        None => None,
    };
    RofRun {
        exit_code,
        report: Some(report),
        out,
        diagnostics,
    }
}

/// One member's outcome: what the read produced, what the export
/// produced, and the exit code the result maps to.
struct MemberOutcome {
    read: ReadReport,
    export: ExportReport,
    exit_code: u8,
}

impl MemberOutcome {
    fn new(read: ReadReport, export: ExportReport, exit_code: u8) -> Self {
        Self {
            read,
            export,
            exit_code,
        }
    }
}

/// Resolves, reads and (on request) exports one member of the container.
///
/// Returns `Ok(None)` when the container did not mount — nothing exists
/// to read, and no write is attempted — `Err((exit_code, diagnostic))`
/// when `--member` itself is not a key, and `Ok(Some(...))` with the two
/// report fragments and the exit code the outcome maps to once the read
/// was attempted. The order is the spec's: **read first, write second**,
/// so a refused member never reaches the export directory.
fn inspect_member(
    session: &ContentSession,
    source: Option<&RofSource>,
    member: &str,
    export_dir: Option<&Path>,
    diagnostics: &mut Vec<String>,
) -> Result<Option<MemberOutcome>, (u8, String)> {
    let key = AssetKey::from_spelling(ROF_NAMESPACE, member, "default").map_err(|error| {
        (
            2,
            format!("cs-inspect rof: invalid --member {member:?}: {error}"),
        )
    })?;
    let Some(source) = source else {
        diagnostics.push(
            "cs-inspect rof: --member was not read and nothing was written: the container was \
             refused"
                .to_owned(),
        );
        return Ok(None);
    };
    let asset = match session.resolve(&key) {
        Ok(asset) => asset,
        Err(error) => {
            diagnostics.push(error.to_string());
            return Ok(Some(MemberOutcome::new(
                ReadReport::refused(resolve_error_code(&error), None, error.to_string()),
                ExportReport::skipped(),
                3,
            )));
        }
    };
    let read = match source.read(&asset.resolved().key) {
        Ok(read) => ReadReport::read(&asset, &read),
        Err(error) => {
            diagnostics.push(error.to_string());
            return Ok(Some(MemberOutcome::new(
                ReadReport::refused(error.code(), error.offset(), error.to_string()),
                ExportReport::skipped(),
                3,
            )));
        }
    };
    let Some(export_dir) = export_dir else {
        return Ok(Some(MemberOutcome::new(read, ExportReport::skipped(), 0)));
    };
    let directory = match ExportDirectory::open(export_dir, session) {
        Ok(directory) => directory,
        Err(error) => {
            diagnostics.push(error.to_string());
            return Ok(Some(MemberOutcome::new(
                read,
                ExportReport::refused(export_error_code(&error), error.to_string()),
                2,
            )));
        }
    };
    match export_rof_member(session, &asset, source, &directory) {
        Ok(file) => Ok(Some(MemberOutcome::new(
            read,
            ExportReport::written(&file),
            0,
        ))),
        Err(error) => {
            diagnostics.push(error.to_string());
            Ok(Some(MemberOutcome::new(
                read,
                ExportReport::refused(rof_export_error_code(&error), error.to_string()),
                export_exit_code(&error),
            )))
        }
    }
}

/// The exit code a mount refusal maps to.
fn mount_exit_code(error: &RofMountError) -> u8 {
    match error {
        RofMountError::Format { .. }
        | RofMountError::NonUtf8Name { .. }
        | RofMountError::InvalidMemberPath { .. }
        | RofMountError::Member { .. } => 3,
        RofMountError::UnreadableContainer { .. }
        | RofMountError::Mount(_)
        | RofMountError::Session(_) => 1,
    }
}

/// The mount a container becomes: one retail source in the install key
/// space, labeled with the spelling that named it.
fn mount_builder(container: &str) -> MountBuilder {
    MountBuilder::new(
        mount_id(container),
        MountNamespace::new(ROF_NAMESPACE).expect("a valid namespace"),
        PrecedenceClass::Shared,
        container,
    )
    .retail()
}

/// The mount id a container spelling becomes: `[a-z0-9._-]`, at most
/// [`MAX_LABEL_LEN`] bytes, always starting with the `rof-` prefix.
fn mount_id(container: &str) -> MountId {
    let mut id = String::from("rof-");
    for ch in container.chars() {
        if id.len() >= MAX_LABEL_LEN {
            break;
        }
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '-' | '_') {
            id.push(ch);
        } else {
            id.push('-');
        }
    }
    MountId::new(&id).unwrap_or_else(|_| MountId::new("rof").expect("a valid fallback id"))
}

/// What one member read produced, for the report.
#[derive(Debug)]
struct ReadReport {
    json: String,
}

impl ReadReport {
    fn skipped() -> Self {
        Self {
            json: "{\"status\": \"skipped\"}".to_owned(),
        }
    }

    fn refused(code: &str, offset: Option<u64>, detail: String) -> Self {
        Self {
            json: format!(
                "{{\"status\": \"refused\", \"code\": {}, \"offset\": {}, \"detail\": {}}}",
                jstr(code),
                offset.map_or_else(|| "null".to_owned(), |offset| offset.to_string()),
                jstr(&detail)
            ),
        }
    }

    fn read(asset: &SessionAsset, read: &RofMemberRead) -> Self {
        let decoded = install::sha256(&read.data);
        Self {
            json: format!(
                "{{\"status\": \"read\", \"spelling\": {}, \"stored_len\": {}, \"decoded_len\": {}, \
                 \"trailing_len\": {}, \"sha256\": {}}}",
                jstr(asset.resolved().span.member_key().unwrap_or_default()),
                read.stored_len,
                read.decoded_len,
                read.trailing_len,
                jstr(&decoded.to_hex())
            ),
        }
    }
}

/// What the explicit export produced, for the report.
#[derive(Debug)]
struct ExportReport {
    json: String,
}

impl ExportReport {
    fn skipped() -> Self {
        Self {
            json: "{\"status\": \"skipped\"}".to_owned(),
        }
    }

    fn refused(code: &str, detail: String) -> Self {
        Self {
            json: format!(
                "{{\"status\": \"refused\", \"code\": {}, \"detail\": {}}}",
                jstr(code),
                jstr(&detail)
            ),
        }
    }

    fn written(file: &ExportedFile) -> Self {
        Self {
            json: format!(
                "{{\"status\": \"written\", \"path\": {}, \"size_bytes\": {}, \"sha256\": {}}}",
                jstr(&file.path.to_string_lossy()),
                file.size_bytes,
                jstr(&file.sha256.to_hex())
            ),
        }
    }
}

/// The mount refusal the report carries: its stable code, the container
/// offset it points at, and the diagnostic text.
#[derive(Debug)]
struct MountFailure {
    json: String,
}

impl MountFailure {
    fn mount(error: &RofMountError) -> Self {
        Self {
            json: Self::render(error.code(), error.offset(), &error.to_string()),
        }
    }

    fn session(error: &SessionError) -> Self {
        Self {
            json: Self::render("session", None, &error.to_string()),
        }
    }

    fn render(code: &str, offset: Option<u64>, detail: &str) -> String {
        format!(
            "{{\"code\": {}, \"offset\": {}, \"detail\": {}}}",
            jstr(code),
            offset.map_or_else(|| "null".to_owned(), |offset| offset.to_string()),
            jstr(detail)
        )
    }
}

/// Renders the rof report: what the container is, what the mount recorded
/// about it, and what the read and the export produced.
fn rof_report_json(
    session: &ContentSession,
    host_root: &Path,
    container: &str,
    source: Option<&RofSource>,
    mount_failure: Option<&MountFailure>,
    read: &ReadReport,
    export: &ExportReport,
) -> String {
    let context = session.context();
    let (status, mount_json) = match session.mounts().next() {
        Some(mount) => (
            "mounted",
            format!(
                "{{\"id\": {}, \"namespace\": {}, \"container\": {}, \"precedence\": {}, \
                 \"scope\": {}, \"retail\": {}}}",
                jstr(mount.id().as_str()),
                jstr(mount.namespace().as_str()),
                jstr(mount.container()),
                jstr(mount.precedence().label()),
                jstr(&mount.scope().to_string()),
                mount.is_retail(),
            ),
        ),
        None => ("refused", "null".to_owned()),
    };
    let (member_count, members) = match source {
        Some(source) => {
            let listed: Vec<String> = source
                .members()
                .map(|member| {
                    format!(
                        "{{\"spelling\": {}, \"id\": {}, \"offset\": {}, \"stored_len\": {}, \
                         \"compressed\": {}, \"sha256\": {}}}",
                        jstr(&member.spelling),
                        member.id,
                        member.offset,
                        member.stored_len,
                        member.compressed,
                        jstr(&member.sha256.to_hex())
                    )
                })
                .collect();
            (source.member_count(), format!("[{}]", listed.join(", ")))
        }
        None => (0, "[]".to_owned()),
    };

    let error_json =
        mount_failure.map_or_else(|| "null".to_owned(), |failure| failure.json.clone());

    format!(
        "{{\n\
         \x20\"report\": {},\n\
         \x20\"host_root\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"container\": {{\"spelling\": {}, \"status\": {}}},\n\
         \x20\"mount\": {},\n\
         \x20\"error\": {},\n\
         \x20\"member_count\": {},\n\
         \x20\"members\": {},\n\
         \x20\"session\": {{\"generation\": {}}},\n\
         \x20\"precedence_status\": {},\n\
         \x20\"read\": {},\n\
         \x20\"export\": {}\n\
         }}\n",
        jstr(ROF_REPORT_VERSION),
        jstr(&host_root.to_string_lossy()),
        jstr(&context.installation.to_hex()),
        jstr(container),
        jstr(status),
        mount_json,
        error_json,
        member_count,
        members,
        session.generation().get(),
        jstr(PRECEDENCE_ORDER_STATUS.label()),
        read.json,
        export.json,
    )
}

/// The stable code a resolution refusal reports.
fn resolve_error_code(error: &ResolveError) -> &'static str {
    match error {
        ResolveError::NotFound { .. } => "not_found",
        ResolveError::Ambiguous { .. } => "ambiguous",
        ResolveError::UnmeasuredOrder { .. } => "blocked_unmeasured_order",
    }
}

/// The exit code an export refusal maps to.
fn export_exit_code(error: &RofExportError) -> u8 {
    match error {
        RofExportError::Read(_)
        | RofExportError::ForeignSession { .. }
        | RofExportError::ForeignMount { .. } => 3,
        RofExportError::Export(error) => match error {
            ExportError::RootUnavailable { .. } | ExportError::RootInsideMount { .. } => 2,
            ExportError::UnsafeName { .. }
            | ExportError::TargetInsideMount { .. }
            | ExportError::UnsafeExportTree { .. }
            | ExportError::TargetExists { .. }
            | ExportError::Read(_) => 3,
            ExportError::Io { .. } => 1,
        },
    }
}

/// The stable code an export refusal reports.
fn rof_export_error_code(error: &RofExportError) -> &'static str {
    match error {
        RofExportError::ForeignSession { .. } => "foreign_session",
        RofExportError::ForeignMount { .. } => "foreign_mount",
        RofExportError::Read(read) => read.code(),
        RofExportError::Export(error) => export_error_code(error),
    }
}

/// The stable code an export-directory refusal reports.
fn export_error_code(error: &ExportError) -> &'static str {
    match error {
        ExportError::UnsafeName { .. } => "unsafe_name",
        ExportError::RootUnavailable { .. } => "root_unavailable",
        ExportError::RootInsideMount { .. } => "root_inside_mount",
        ExportError::TargetInsideMount { .. } => "target_inside_mount",
        ExportError::UnsafeExportTree { .. } => "unsafe_export_tree",
        ExportError::TargetExists { .. } => "target_exists",
        ExportError::Read(_) => "read",
        ExportError::Io { .. } => "io",
    }
}

/// Whether `path` would land inside `root` once both are resolved as far
/// as they exist.
fn inside(path: &Path, root: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    // Resolve the longest existing ancestor; the rest does not exist yet
    // and so cannot be a link.
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

/// Escapes `value` as a JSON string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    //! F05-C acceptance tests for the `rof` command (prefix
    //! `accept_f05_c_`): one container mounted, reported and exported
    //! through the command, and the AC03 minimum scenario — a cycle and an
    //! expansion bomb — refusing with exit 3 while the export directory
    //! stays empty.
    //!
    //! Every container is authored here except the committed shared
    //! fixture, whose bytes are written by `tools/make_synthetic_fixtures.py`
    //! and read through `include_bytes!`: newly authored synthetic
    //! content, no original game data, no `CS_GAME_DIR` access.

    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_DIRECTORY, RECORD_BYTES};

    use super::*;

    /// The committed shared fixture: a root block with `HELLO.TXT` (id
    /// 101, a 34-byte payload) and the empty `EMPTY.DAT` (id 102).
    const SHARED_ROF: &[u8] = include_bytes!("../../../fixtures/synthetic/flat-uncompressed.rof");
    const SHARED_PAYLOAD: &[u8] = b"Newly authored synthetic archive.\n";

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable installation with sibling output directories, all
    /// removed on drop.
    struct Temp {
        root: PathBuf,
    }

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f05-c-cmd-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("install")).expect("install dir is created");
            Self { root }
        }

        fn install(&self) -> PathBuf {
            self.root.join("install")
        }

        /// Writes one container into the installation.
        fn container(&self, name: &str, bytes: &[u8]) {
            fs::write(self.install().join(name), bytes).expect("fixture bytes are written");
        }

        fn export(&self) -> PathBuf {
            let path = self.root.join("export");
            fs::create_dir_all(&path).expect("export dir is created");
            path
        }

        fn out(&self) -> PathBuf {
            self.root.join("rof.json")
        }

        /// Every entry below `path`, sorted — an empty vec means *nothing*
        /// was written, not even a temporary file.
        fn entries(&self, path: &Path) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(path)
                .expect("directory is readable")
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn path_arg(path: &Path) -> &str {
        path.to_str().expect("temp paths are UTF-8")
    }

    /// One record exactly as authored: the six on-disk fields, verbatim.
    #[derive(Clone, Copy)]
    struct RawRecord {
        start: u32,
        raw_length: u32,
        raw_length_on_disk: u32,
        flags: u32,
        name_length: u32,
        id: u32,
    }

    fn name_table(names: &[&str]) -> Vec<u8> {
        let mut table = Vec::new();
        for name in names {
            table.extend_from_slice(name.as_bytes());
            table.push(0);
        }
        table
    }

    fn valid_block(records: &[RawRecord], names: &[u8]) -> Vec<u8> {
        let mut bytes =
            Vec::with_capacity(DIRECTORY_HEADER_BYTES + records.len() * RECORD_BYTES + names.len());
        bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
        for record in records {
            bytes.extend_from_slice(&record.start.to_le_bytes());
            bytes.extend_from_slice(&record.raw_length.to_le_bytes());
            bytes.extend_from_slice(&record.raw_length_on_disk.to_le_bytes());
            bytes.extend_from_slice(&record.flags.to_le_bytes());
            bytes.extend_from_slice(&record.name_length.to_le_bytes());
            bytes.extend_from_slice(&record.id.to_le_bytes());
        }
        bytes.extend_from_slice(names);
        bytes
    }

    /// A root block whose only record is a directory pointing at offset
    /// zero — itself.
    fn cycle_container() -> Vec<u8> {
        let names = name_table(&["LOOP"]);
        let record = RawRecord {
            flags: FLAG_DIRECTORY,
            start: 0,
            raw_length: 0,
            raw_length_on_disk: 0,
            name_length: names.len() as u32,
            id: 1,
        };
        valid_block(&[record], &names)
    }

    /// One uncompressed member of `payload` bytes, for a command run with
    /// a `--max-decoded-bytes` ceiling below its size.
    fn single_member(name: &'static str, payload: &[u8]) -> Vec<u8> {
        let names = name_table(&[name]);
        let block_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len();
        let record = RawRecord {
            start: block_len as u32,
            raw_length: payload.len() as u32,
            raw_length_on_disk: payload.len() as u32,
            flags: 0,
            name_length: names.len() as u32,
            id: 7,
        };
        let mut bytes = valid_block(&[record], &names);
        bytes.extend_from_slice(payload);
        bytes
    }

    /// **The positive path:** the command mounts the shared fixture,
    /// reports every member it holds, reads the requested one through the
    /// bounded decoder and exports its bytes — exit 0, report written.
    #[test]
    fn accept_f05_c_rof_command_mounts_reports_and_exports() {
        let temp = Temp::new("mount");
        temp.container("pack.rof", SHARED_ROF);
        let export = temp.export();
        let out = temp.out();

        let run = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "pack.rof",
                "--member",
                "HELLO.TXT",
                "--export-dir",
                path_arg(&export),
                "--out",
                path_arg(&out),
            ]),
            None,
        );
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        assert_eq!(run.out.as_deref(), Some(out.as_path()));
        let report = fs::read_to_string(&out).expect("the report is written");
        assert_eq!(Some(&report), run.report.as_ref());

        for needle in [
            "\"report\": \"cs-inspect-rof/1\"".to_owned(),
            "\"container\": {\"spelling\": \"pack.rof\", \"status\": \"mounted\"}".to_owned(),
            "\"id\": \"rof-pack.rof\", \"namespace\": \"install\", \"container\": \"pack.rof\", \
             \"precedence\": \"shared\""
                .to_owned(),
            "\"retail\": true".to_owned(),
            "\"member_count\": 2".to_owned(),
            "{\"spelling\": \"HELLO.TXT\", \"id\": 101, \"offset\": 76, \"stored_len\": 34, \
             \"compressed\": false"
                .to_owned(),
            "{\"spelling\": \"EMPTY.DAT\", \"id\": 102".to_owned(),
            "\"install_sha256\": \"".to_owned(),
            "\"status\": \"read\", \"spelling\": \"HELLO.TXT\", \"stored_len\": 34, \
             \"decoded_len\": 34, \"trailing_len\": 0"
                .to_owned(),
            "\"status\": \"written\"".to_owned(),
        ] {
            assert!(report.contains(&needle), "missing {needle} in {report}");
        }
        assert_eq!(
            fs::read(export.join("HELLO.TXT")).expect("the export is written"),
            SHARED_PAYLOAD
        );
        assert_eq!(temp.entries(&export), vec!["HELLO.TXT".to_owned()]);
    }

    /// **AC03 (cycle):** a container whose directory record points at
    /// itself is refused with exit 3 before a mount exists, the report
    /// says why, and the export directory is left empty.
    #[test]
    fn accept_f05_c_rof_command_refuses_a_cycle_container_without_writing() {
        let temp = Temp::new("cycle");
        temp.container("cycle.rof", &cycle_container());
        let export = temp.export();
        let out = temp.out();

        let run = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "cycle.rof",
                "--member",
                "LOOP",
                "--export-dir",
                path_arg(&export),
                "--out",
                path_arg(&out),
            ]),
            None,
        );
        assert_eq!(run.exit_code, 3, "{:?}", run.diagnostics);
        assert!(
            run.diagnostics.iter().any(|line| line.contains("cycle")),
            "the refusal is reported: {:?}",
            run.diagnostics
        );
        let report = run.report.as_ref().expect("a refused run still reports");
        assert_eq!(
            fs::read_to_string(&out).expect("the report is written"),
            *report,
            "the report of a refusal is the evidence and is written"
        );
        for needle in [
            "\"container\": {\"spelling\": \"cycle.rof\", \"status\": \"refused\"}".to_owned(),
            "\"mount\": null".to_owned(),
            "\"member_count\": 0".to_owned(),
            "\"members\": []".to_owned(),
            "\"read\": {\"status\": \"skipped\"}".to_owned(),
            "\"export\": {\"status\": \"skipped\"}".to_owned(),
            "\"code\": \"cycle\"".to_owned(),
        ] {
            assert!(report.contains(&needle), "missing {needle} in {report}");
        }
        assert!(
            temp.entries(&export).is_empty(),
            "the refusal wrote {:?}",
            temp.entries(&export)
        );

        // The refusal itself carries the exit code: a run that asks for
        // nothing but the report still fails, so no code path can turn a
        // refused container into a success.
        let report_only = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "cycle.rof",
            ]),
            None,
        );
        assert_eq!(report_only.exit_code, 3, "{:?}", report_only.diagnostics);
        assert!(
            report_only
                .report
                .as_ref()
                .is_some_and(|report| report.contains("\"status\": \"refused\"")),
            "the refusal is reported"
        );
    }

    /// **AC03 (expansion bomb):** the container mounts, but a member over
    /// `--max-decoded-bytes` is refused at read time — exit 3, the report
    /// names `expansion_bomb` and its offset, and no byte reaches the
    /// export directory.
    #[test]
    fn accept_f05_c_rof_command_refuses_an_expansion_bomb_before_writing() {
        let temp = Temp::new("bomb");
        let payload = [b'x'; 100];
        temp.container("big.rof", &single_member("BIG.DAT", &payload));
        let export = temp.export();
        let out = temp.out();

        let run = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "big.rof",
                "--member",
                "BIG.DAT",
                "--max-decoded-bytes",
                "32",
                "--export-dir",
                path_arg(&export),
                "--out",
                path_arg(&out),
            ]),
            None,
        );
        assert_eq!(run.exit_code, 3, "{:?}", run.diagnostics);
        let report = run.report.as_ref().expect("a refused read still reports");
        assert_eq!(run.out.as_deref(), Some(out.as_path()));
        for needle in [
            "\"status\": \"mounted\"".to_owned(),
            "\"member_count\": 1".to_owned(),
            "\"read\": {\"status\": \"refused\", \"code\": \"expansion_bomb\", \"offset\": 40,"
                .to_owned(),
            "\"export\": {\"status\": \"skipped\"}".to_owned(),
        ] {
            assert!(report.contains(&needle), "missing {needle} in {report}");
        }
        assert!(
            temp.entries(&export).is_empty(),
            "the bomb refused before writing, found {:?}",
            temp.entries(&export)
        );
    }

    /// Invalid input never reaches the installation's contents: a missing
    /// or escaping container, a member-less export, an output inside the
    /// installation and a missing installation all fail with their
    /// documented exit codes and no report of content that was never
    /// read.
    #[test]
    fn accept_f05_c_rof_command_rejects_invalid_input() {
        let temp = Temp::new("input");
        temp.container("pack.rof", SHARED_ROF);

        let missing_flag =
            rof_command_result(&args(&["--cs-path", path_arg(&temp.install())]), None);
        assert_eq!(missing_flag.exit_code, 2, "{:?}", missing_flag.diagnostics);
        assert!(missing_flag.report.is_none());

        let escaping = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "../outside.rof",
            ]),
            None,
        );
        assert_eq!(escaping.exit_code, 2, "{:?}", escaping.diagnostics);

        let unknown_container = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "absent.rof",
            ]),
            None,
        );
        assert_eq!(
            unknown_container.exit_code, 2,
            "{:?}",
            unknown_container.diagnostics
        );

        let no_member = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "pack.rof",
                "--export-dir",
                path_arg(&temp.export()),
            ]),
            None,
        );
        assert_eq!(no_member.exit_code, 2, "{:?}", no_member.diagnostics);

        let output_inside = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "pack.rof",
                "--out",
                path_arg(&temp.install().join("report.json")),
            ]),
            None,
        );
        assert_eq!(
            output_inside.exit_code, 2,
            "{:?}",
            output_inside.diagnostics
        );
        assert!(
            !temp.install().join("report.json").exists(),
            "a refused output must not be written into the installation"
        );

        let bad_limit = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "pack.rof",
                "--max-decoded-bytes",
                "lots",
            ]),
            None,
        );
        assert_eq!(bad_limit.exit_code, 2, "{:?}", bad_limit.diagnostics);

        let no_installation = rof_command_result(&args(&["--container", "pack.rof"]), None);
        assert_eq!(
            no_installation.exit_code, 4,
            "{:?}",
            no_installation.diagnostics
        );
        assert!(no_installation.report.is_none());
    }

    /// A member the container does not hold is refused as content (exit
    /// 3): the mounted container is still reported, the read says
    /// `not_found`, the export stays skipped and the directory stays
    /// empty.
    #[test]
    fn accept_f05_c_rof_command_refuses_an_unknown_member() {
        let temp = Temp::new("member");
        temp.container("pack.rof", SHARED_ROF);
        let export = temp.export();
        let out = temp.out();

        let run = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "pack.rof",
                "--member",
                "NOTHING.DAT",
                "--export-dir",
                path_arg(&export),
                "--out",
                path_arg(&out),
            ]),
            None,
        );
        assert_eq!(run.exit_code, 3, "{:?}", run.diagnostics);
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("NOTHING.DAT")),
            "the refusal names the member: {:?}",
            run.diagnostics
        );
        let report = run.report.as_ref().expect("a refused read still reports");
        assert_eq!(run.out.as_deref(), Some(out.as_path()));
        for needle in [
            "\"status\": \"mounted\"".to_owned(),
            "\"member_count\": 2".to_owned(),
            "\"read\": {\"status\": \"refused\", \"code\": \"not_found\"".to_owned(),
            "\"export\": {\"status\": \"skipped\"}".to_owned(),
        ] {
            assert!(report.contains(&needle), "missing {needle} in {report}");
        }
        assert!(
            temp.entries(&export).is_empty(),
            "the refusal wrote {:?}",
            temp.entries(&export)
        );
    }

    /// The exit codes a mount refusal maps to, as `CLI-EVIDENCE.md`
    /// defines them: refused content is *failed validation* (3), a
    /// container that could not be read or a mount the session refused is
    /// a *runtime failure* (1). The unreachable-through-the-CLI variants
    /// are pinned here so no mapping can quietly change.
    #[test]
    fn accept_f05_c_mount_exit_codes_follow_cli_evidence() {
        use cs_assets::vfs::MountError;

        let temp = Temp::new("codes");
        temp.container("cycle.rof", &cycle_container());
        let refusal = mount_rof_with_limits(
            mount_builder("cycle.rof"),
            &temp.install().join("cycle.rof"),
            RofLimits::default(),
        )
        .expect_err("the cycle is refused");
        assert_eq!(mount_exit_code(&refusal), 3, "{refusal}");

        let unreadable = RofMountError::UnreadableContainer {
            container: "gone.rof".to_owned(),
            path: temp.install().join("gone.rof"),
            source: io::Error::from(io::ErrorKind::NotFound),
        };
        assert_eq!(mount_exit_code(&unreadable), 1, "{unreadable}");

        let repeated = RofMountError::Session(SessionError::Mount(MountError::DuplicateMountId {
            id: MountId::new("rof-cycle-rof").expect("a valid mount id"),
        }));
        assert_eq!(mount_exit_code(&repeated), 1, "{repeated}");
    }

    /// A container reached through a directory link planted inside the
    /// installation is never read: the report claims this installation's
    /// fingerprint, so bytes that only lexically sit below it would be a
    /// provenance lie. Refused as invalid input (exit 2) before a single
    /// container byte is opened, with no report.
    #[cfg(unix)]
    #[test]
    fn accept_f05_c_rof_command_refuses_a_container_reached_through_a_link() {
        use std::os::unix::fs::symlink;

        let temp = Temp::new("link");
        let outside = temp.root.join("outside");
        fs::create_dir_all(&outside).expect("outside dir is created");
        fs::write(outside.join("pack.rof"), SHARED_ROF).expect("fixture bytes are written");
        symlink(&outside, temp.install().join("linked")).expect("directory link is created");
        let out = temp.out();

        let run = rof_command_result(
            &args(&[
                "--cs-path",
                path_arg(&temp.install()),
                "--container",
                "linked/pack.rof",
                "--out",
                path_arg(&out),
            ]),
            None,
        );
        assert_eq!(run.exit_code, 2, "{:?}", run.diagnostics);
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("symbolic link")),
            "the refusal explains itself: {:?}",
            run.diagnostics
        );
        assert!(
            run.report.is_none(),
            "a container outside the installation is never read or reported"
        );
        assert!(!out.exists(), "no report was written for it");
    }
}
