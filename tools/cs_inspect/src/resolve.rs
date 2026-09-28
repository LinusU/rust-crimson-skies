//! The `resolve` command (F04-C): one lookup against a mounted content
//! session, reported with its full resolution trace.
//!
//! ```text
//! cs-inspect resolve [--cs-path <dir>] --asset <namespace>:<path>
//!     [--variant <label>] [--world <group>] [--locale <label>]
//!     [--mission <label>] [--out <file>] [--export-dir <dir>]
//! ```
//!
//! The installation is discovered (F02-B, which gives the installation
//! fingerprint the context resolves against), mounted into one
//! [`ContentSession`] with the designed layout of
//! [`SessionBuilder::mount_installation`], and the key is resolved in that
//! session. The JSON report names the installation fingerprint, the
//! context, every mount of the session (with the entries it refused), the
//! key, the result — the winning span or both origins of an ambiguity —
//! and every ordered attempt with its outcome, plus the evidence status of
//! the precedence order (`designed`).
//!
//! `--export-dir` is the explicit private research export of spec F04
//! non-negotiable behavior 5: the resolved member is read (digest-checked)
//! and written below that directory by
//! [`cs_assets::vfs::export_asset`], which refuses hostile member names
//! and a directory inside the installation. Neither `--out` nor
//! `--export-dir` may lie inside the installation.
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` resolved (and
//! exported, if asked); `2` invalid input; `3` the key did not resolve to
//! exactly one origin, a mount refused content, or an export was refused;
//! `4` no installation selected; `1` a runtime failure. When the lookup
//! ran, the report is written even on exit 3, because the trace is the
//! evidence of why it failed. The session is closed before the command
//! returns, on every path.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install::{self, DiscoveryError};
use cs_assets::vfs::{
    ContentSession, ExportDirectory, ExportError, ResolutionTrace, ResolveError, SessionAsset,
    SessionBuilder, SessionError, SourceError, export_asset,
};
use cs_types::asset_id::{
    AssetKey, AssetKeyError, LabelError, MissionScope, ResolveContext, WorldGroup,
};
use cs_types::install::{LocaleLabel, RelativePathError};

/// The report format version.
pub const RESOLVE_REPORT_VERSION: &str = "cs-inspect-resolve/1";

/// Why the `resolve` command failed before or around the lookup.
#[derive(Debug)]
pub enum ResolveCommandError {
    /// The command line was malformed.
    Usage(String),
    /// The asset key was invalid.
    Key(AssetKeyError),
    /// `--world` was not a valid spelling or names no discovered group.
    World(String),
    /// `--locale` or `--mission` was invalid.
    Label(String),
    /// Neither `--cs-path` nor `CS_GAME_DIR` selected an installation.
    MissingInstallation,
    /// Discovery refused the installation.
    Discovery(DiscoveryError),
    /// A mount of the session was refused.
    Session(SessionError),
    /// `--out` or `--export-dir` lies inside the installation.
    OutputInsideInstallation(PathBuf),
    /// The report could not be written.
    Output {
        /// The requested path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
}

impl fmt::Display for ResolveCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::World(message) | Self::Label(message) => {
                write!(f, "{message}")
            }
            Self::Key(error) => write!(f, "invalid --asset: {error}"),
            Self::MissingInstallation => write!(
                f,
                "no installation selected: pass --cs-path <dir> or set CS_GAME_DIR"
            ),
            Self::Discovery(error) => write!(f, "{error}"),
            Self::Session(error) => write!(f, "{error}"),
            Self::OutputInsideInstallation(path) => write!(
                f,
                "{} lies inside the installation; cs-inspect never writes there",
                path.display()
            ),
            Self::Output { path, source } => {
                write!(f, "cannot write report to {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ResolveCommandError {}

/// Parsed `resolve` arguments.
#[derive(Debug, Default)]
struct ResolveArgs {
    cs_path: Option<PathBuf>,
    asset: Option<String>,
    variant: Option<String>,
    world: Option<String>,
    locale: Option<String>,
    mission: Option<String>,
    out: Option<PathBuf>,
    export_dir: Option<PathBuf>,
}

fn parse_resolve_args(args: &[String]) -> Result<ResolveArgs, ResolveCommandError> {
    let mut parsed = ResolveArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        let slot = match flag {
            "--cs-path" | "--out" | "--export-dir" => None,
            "--asset" => Some(&mut parsed.asset),
            "--variant" => Some(&mut parsed.variant),
            "--world" => Some(&mut parsed.world),
            "--locale" => Some(&mut parsed.locale),
            "--mission" => Some(&mut parsed.mission),
            _ => {
                return Err(ResolveCommandError::Usage(format!(
                    "cs-inspect resolve: unsupported argument {flag:?}; expected --cs-path, \
                     --asset, --variant, --world, --locale, --mission, --out, --export-dir"
                )));
            }
        };
        let Some(value) = cursor.next() else {
            return Err(ResolveCommandError::Usage(format!(
                "cs-inspect resolve: {flag} needs a value"
            )));
        };
        match (flag, slot) {
            (_, Some(slot)) => *slot = Some(value.clone()),
            ("--cs-path", None) => parsed.cs_path = Some(PathBuf::from(value)),
            ("--out", None) => parsed.out = Some(PathBuf::from(value)),
            (_, None) => parsed.export_dir = Some(PathBuf::from(value)),
        }
    }
    Ok(parsed)
}

/// Splits `<namespace>:<path>` and validates it as an [`AssetKey`].
fn parse_key(asset: &str, variant: Option<&str>) -> Result<AssetKey, ResolveCommandError> {
    let Some((namespace, path)) = asset.split_once(':') else {
        return Err(ResolveCommandError::Usage(format!(
            "cs-inspect resolve: --asset {asset:?} must be <namespace>:<path>, \
             e.g. world:texture.zbd"
        )));
    };
    AssetKey::from_spelling(namespace, path, variant.unwrap_or("default"))
        .map_err(ResolveCommandError::Key)
}

/// Runs the `resolve` command and returns its exit code.
pub fn resolve_command(args: &[String]) -> ExitCode {
    let run = resolve_command_result(args, std::env::var_os("CS_GAME_DIR"));
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote resolve report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// Everything one `resolve` run produced.
#[derive(Debug)]
pub struct ResolveRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The JSON report, when the lookup ran.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
}

impl ResolveRun {
    fn failed(exit_code: u8, error: &ResolveCommandError) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![error.to_string()],
        }
    }
}

/// The body of [`resolve_command`], with the environment's installation
/// passed in so tests can drive it.
pub fn resolve_command_result(args: &[String], env_cs_path: Option<OsString>) -> ResolveRun {
    let parsed = match parse_resolve_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return ResolveRun::failed(2, &error),
    };
    let key = match parsed.asset.as_deref() {
        None => {
            return ResolveRun::failed(
                2,
                &ResolveCommandError::Usage(
                    "cs-inspect resolve: --asset <namespace>:<path> is required".to_owned(),
                ),
            );
        }
        Some(asset) => match parse_key(asset, parsed.variant.as_deref()) {
            Ok(key) => key,
            Err(error) => return ResolveRun::failed(2, &error),
        },
    };
    let cs_path = parsed.cs_path.clone().or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return ResolveRun::failed(4, &ResolveCommandError::MissingInstallation);
    };
    for output in [&parsed.out, &parsed.export_dir].into_iter().flatten() {
        if inside(output, &cs_path) {
            return ResolveRun::failed(
                2,
                &ResolveCommandError::OutputInsideInstallation(output.clone()),
            );
        }
    }

    let found = match install::discover(&cs_path) {
        Ok(found) => found,
        Err(error) => return ResolveRun::failed(1, &ResolveCommandError::Discovery(error)),
    };
    let context = match build_context(&parsed, &found.diagnosis.world_groups, &found) {
        Ok(context) => context,
        Err(error) => return ResolveRun::failed(2, &error),
    };

    let mut builder = SessionBuilder::new(context);
    if let Err(error) = builder.mount_installation(&cs_path, &found.diagnosis) {
        let code = match &error {
            SessionError::Source {
                error: SourceError::Member { .. } | SourceError::NonUtf8Name { .. },
                ..
            } => 3,
            _ => 1,
        };
        // Dropping the builder releases the mounts that did succeed.
        return ResolveRun::failed(code, &ResolveCommandError::Session(error));
    }
    let session = builder.open();

    let mut diagnostics = Vec::new();
    let lookup = session.resolve(&key);
    let mut exit_code = match &lookup {
        Ok(_) => 0,
        Err(error) => {
            diagnostics.push(error.to_string());
            3
        }
    };
    let export = match (&lookup, &parsed.export_dir) {
        (Ok(asset), Some(directory)) => {
            let result = ExportDirectory::open(directory, &session)
                .and_then(|directory| export_asset(&session, asset, &directory));
            if let Err(error) = &result {
                diagnostics.push(error.to_string());
                exit_code = exit_code.max(export_exit_code(error));
            }
            Some(result)
        }
        _ => None,
    };

    let report = resolve_report_json(&session, &cs_path, &key, &lookup, export.as_ref());
    let generation = session.generation();
    let teardown = session.close();
    debug_assert_eq!(teardown.generation, generation);

    let out = match &parsed.out {
        Some(out) => {
            if let Err(source) = write_report(out, &report) {
                let error = ResolveCommandError::Output {
                    path: out.clone(),
                    source,
                };
                diagnostics.push(error.to_string());
                return ResolveRun {
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
    ResolveRun {
        exit_code,
        report: Some(report),
        out,
        diagnostics,
    }
}

/// The exit code an export failure maps to.
fn export_exit_code(error: &ExportError) -> u8 {
    match error {
        ExportError::RootUnavailable { .. } | ExportError::RootInsideMount { .. } => 2,
        ExportError::UnsafeName { .. }
        | ExportError::TargetInsideMount { .. }
        | ExportError::UnsafeExportTree { .. }
        | ExportError::TargetExists { .. }
        | ExportError::Read(_) => 3,
        ExportError::Io { .. } => 1,
    }
}

/// Builds the resolve context from the discovered fingerprint and the
/// `--world`, `--locale` and `--mission` flags.
fn build_context(
    parsed: &ResolveArgs,
    world_groups: &[cs_types::install::RelativePath],
    found: &install::Discovery,
) -> Result<ResolveContext, ResolveCommandError> {
    let mut context = ResolveContext::new(install::fingerprint(&found.manifest));
    if let Some(world) = &parsed.world {
        let group = WorldGroup::new(world).map_err(|error: RelativePathError| {
            ResolveCommandError::World(format!("invalid --world {world:?}: {error}"))
        })?;
        if !world_groups
            .iter()
            .any(|known| known.logical_key() == group.logical_key())
        {
            let known: Vec<&str> = world_groups.iter().map(|known| known.as_str()).collect();
            return Err(ResolveCommandError::World(format!(
                "--world {world:?} is not a discovered world group; discovered: [{}]",
                known.join(", ")
            )));
        }
        context = context.with_world_group(group);
    }
    if let Some(locale) = &parsed.locale {
        let label = LocaleLabel::new(locale).map_err(|error| {
            ResolveCommandError::Label(format!("invalid --locale {locale:?}: {error}"))
        })?;
        context = context.with_locale(label);
    }
    if let Some(mission) = &parsed.mission {
        let scope = MissionScope::new(mission).map_err(|error: LabelError| {
            ResolveCommandError::Label(format!("invalid --mission {mission:?}: {error}"))
        })?;
        context = context.with_mission(scope);
    }
    Ok(context)
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

/// Renders the resolve report.
pub fn resolve_report_json(
    session: &ContentSession,
    host_root: &Path,
    key: &AssetKey,
    lookup: &Result<SessionAsset, ResolveError>,
    export: Option<&Result<cs_assets::vfs::ExportedFile, ExportError>>,
) -> String {
    let context = session.context();
    let mounts: Vec<String> = session
        .mounts()
        .map(|mount| {
            let rejected: Vec<String> = session
                .rejected()
                .iter()
                .filter(|rejection| rejection.mount == *mount.id())
                .map(|rejection| {
                    format!(
                        "{{\"path\": {}, \"reason\": {}}}",
                        jstr(&rejection.entry.host_relative.to_string_lossy()),
                        jstr(rejection.entry.reason.label())
                    )
                })
                .collect();
            format!(
                "{{\"id\": {}, \"namespace\": {}, \"container\": {}, \"precedence\": {}, \
                 \"scope\": {}, \"members\": {}, \"rejected\": [{}]}}",
                jstr(mount.id().as_str()),
                jstr(mount.namespace().as_str()),
                jstr(mount.container()),
                jstr(mount.precedence().label()),
                jstr(&mount.scope().to_string()),
                mount.member_count(),
                rejected.join(", ")
            )
        })
        .collect();
    let mods: Vec<String> = context
        .mods
        .as_slice()
        .iter()
        .map(|id| jstr(id.as_str()))
        .collect();

    let (result, trace) = match lookup {
        Ok(asset) => {
            let resolved = asset.resolved();
            let span = &resolved.span;
            (
                format!(
                    "{{\"status\": \"resolved\", \"mount\": {}, \"precedence\": {}, \
                     \"span\": {{\"install_sha256\": {}, \"container_path\": {}, \
                     \"member_key\": {}, \"offset\": {}, \"length\": {}, \
                     \"member_sha256\": {}}}}}",
                    jstr(resolved.mount.as_str()),
                    jstr(resolved.precedence.label()),
                    jstr(&span.install_sha256().to_hex()),
                    jstr(span.container_path()),
                    span.member_key().map_or_else(|| "null".to_owned(), jstr),
                    span.offset(),
                    span.length(),
                    span.member_sha256()
                        .map_or_else(|| "null".to_owned(), |hash| jstr(&hash.to_hex())),
                ),
                &resolved.trace,
            )
        }
        Err(ResolveError::NotFound { trace, .. }) => {
            ("{\"status\": \"not_found\"}".to_owned(), trace.as_ref())
        }
        Err(ResolveError::Ambiguous {
            candidates, trace, ..
        }) => {
            let rendered: Vec<String> = candidates
                .iter()
                .map(|origin| {
                    format!(
                        "{{\"mount\": {}, \"container\": {}, \"member_spelling\": {}, \
                         \"precedence\": {}, \"sha256\": {}}}",
                        jstr(origin.mount.as_str()),
                        jstr(&origin.container),
                        jstr(&origin.member_spelling),
                        jstr(origin.precedence.label()),
                        origin
                            .sha256
                            .map_or_else(|| "null".to_owned(), |hash| jstr(&hash.to_hex())),
                    )
                })
                .collect();
            (
                format!(
                    "{{\"status\": \"ambiguous\", \"candidates\": [{}]}}",
                    rendered.join(", ")
                ),
                trace.as_ref(),
            )
        }
    };

    let export = match export {
        None => "null".to_owned(),
        Some(Ok(file)) => format!(
            "{{\"status\": \"written\", \"path\": {}, \"size_bytes\": {}, \"sha256\": {}}}",
            jstr(&file.path.to_string_lossy()),
            file.size_bytes,
            jstr(&file.sha256.to_hex())
        ),
        Some(Err(error)) => format!(
            "{{\"status\": \"refused\", \"error\": {}}}",
            jstr(&error.to_string())
        ),
    };

    format!(
        "{{\n\
         \x20\"report\": {},\n\
         \x20\"host_root\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"context\": {{\"world_group\": {}, \"locale\": {}, \"mission\": {}, \"mods\": [{}]}},\n\
         \x20\"session\": {{\"generation\": {}, \"mounts\": [{}]}},\n\
         \x20\"key\": {{\"namespace\": {}, \"path\": {}, \"variant\": {}, \"logical_key\": {}}},\n\
         \x20\"precedence_status\": {},\n\
         \x20\"result\": {},\n\
         \x20\"trace\": {},\n\
         \x20\"export\": {}\n\
         }}\n",
        jstr(RESOLVE_REPORT_VERSION),
        jstr(&host_root.to_string_lossy()),
        jstr(&context.installation.to_hex()),
        context
            .world_group
            .as_ref()
            .map_or_else(|| "null".to_owned(), |group| jstr(&group.to_string())),
        context
            .locale
            .as_ref()
            .map_or_else(|| "null".to_owned(), |locale| jstr(locale.as_str())),
        context
            .mission
            .as_ref()
            .map_or_else(|| "null".to_owned(), |mission| jstr(mission.as_str())),
        mods.join(", "),
        session.generation().get(),
        mounts.join(", "),
        jstr(key.namespace().as_str()),
        jstr(key.path().as_str()),
        jstr(key.variant().as_str()),
        jstr(&key.logical_key()),
        jstr(trace.precedence_status.label()),
        result,
        trace_json(trace),
        export,
    )
}

/// The ordered attempts of a trace as a JSON array.
fn trace_json(trace: &ResolutionTrace) -> String {
    let attempts: Vec<String> = trace
        .attempts
        .iter()
        .map(|attempt| {
            format!(
                "{{\"mount\": {}, \"container\": {}, \"precedence\": {}, \"outcome\": {}}}",
                jstr(attempt.mount.as_str()),
                jstr(&attempt.container),
                jstr(attempt.precedence.label()),
                jstr(attempt.outcome.label())
            )
        })
        .collect();
    format!("[{}]", attempts.join(", "))
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
    //! F04-C acceptance tests for the `resolve` command. Every tree is
    //! newly authored fixture bytes under the system temporary directory;
    //! the retail test reads `$CS_GAME_DIR` read-only and writes only below
    //! `private/`.

    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f04-c-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("temp dir is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            fs::write(path, bytes).expect("bytes are written");
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn install() -> Temp {
        let tree = Temp::new("install");
        tree.write("ZBD/c1/texture.zbd", b"world one texture bytes");
        tree.write("ZBD/c2/texture.zbd", b"world two texture bytes, longer");
        tree
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn path_arg(path: &Path) -> &str {
        path.to_str().expect("temp paths are UTF-8")
    }

    /// The command wires the session end to end: the world key resolves to
    /// the selected world's bytes, the report carries the fingerprint, the
    /// span, every ordered attempt and the designed status, and the
    /// explicit export writes exactly those bytes outside the installation.
    #[test]
    fn accept_f04_c_cli_resolves_world_texture_with_trace_and_export() {
        let tree = install();
        let private = Temp::new("private");
        let out = private.0.join("resolve.json");
        let export = private.0.join("export");
        fs::create_dir(&export).expect("export dir");

        let run = resolve_command_result(
            &args(&[
                "--cs-path",
                path_arg(&tree.0),
                "--world",
                "zbd/C2",
                "--asset",
                "world:Texture.zbd",
                "--out",
                path_arg(&out),
                "--export-dir",
                path_arg(&export),
            ]),
            None,
        );
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        assert_eq!(run.out.as_deref(), Some(out.as_path()));
        let report = fs::read_to_string(&out).expect("the report is written");
        assert_eq!(Some(&report), run.report.as_ref());

        let found = install::discover(&tree.0).expect("fixture discovers");
        let fingerprint = install::fingerprint(&found.manifest).to_hex();
        let digest = install::sha256(b"world two texture bytes, longer").to_hex();
        for needle in [
            format!("\"install_sha256\": \"{fingerprint}\""),
            "\"world_group\": \"zbd/C2\"".to_owned(),
            "\"status\": \"resolved\", \"mount\": \"world-1\"".to_owned(),
            "\"container_path\": \"ZBD/c2\", \"member_key\": \"texture.zbd\"".to_owned(),
            format!("\"member_sha256\": \"{digest}\""),
            "\"precedence_status\": \"designed\"".to_owned(),
            "{\"mount\": \"world-0\", \"container\": \"ZBD/c1\", \"precedence\": \
             \"mission_world\", \"outcome\": \"scope_mismatch\"}"
                .to_owned(),
            "{\"mount\": \"world-1\", \"container\": \"ZBD/c2\", \"precedence\": \
             \"mission_world\", \"outcome\": \"selected\"}"
                .to_owned(),
            "\"status\": \"written\"".to_owned(),
            format!("\"sha256\": \"{digest}\""),
        ] {
            assert!(report.contains(&needle), "missing {needle} in {report}");
        }
        assert_eq!(
            fs::read(export.join("texture.zbd")).expect("the export is written"),
            b"world two texture bytes, longer"
        );
    }

    /// A key no mount holds exits 3 and still writes the trace that says
    /// why; an unknown world is invalid input.
    #[test]
    fn accept_f04_c_cli_unresolved_key_exits_3_with_trace() {
        let tree = install();
        let run = resolve_command_result(
            &args(&[
                "--cs-path",
                path_arg(&tree.0),
                "--asset",
                "world:texture.zbd",
            ]),
            None,
        );
        assert_eq!(run.exit_code, 3);
        let report = run.report.expect("the trace is reported on failure");
        assert!(report.contains("\"status\": \"not_found\""), "{report}");
        assert!(
            report.contains("\"outcome\": \"scope_mismatch\""),
            "{report}"
        );
        assert!(
            run.diagnostics
                .iter()
                .any(|line| line.contains("no mount holds"))
        );

        let unknown_world = resolve_command_result(
            &args(&[
                "--cs-path",
                path_arg(&tree.0),
                "--world",
                "zbd/c9",
                "--asset",
                "world:texture.zbd",
            ]),
            None,
        );
        assert_eq!(unknown_world.exit_code, 2);
        assert!(unknown_world.report.is_none());
    }

    /// Nothing is ever written inside the installation: `--out` and
    /// `--export-dir` there are refused before anything runs.
    #[test]
    fn accept_f04_c_cli_refuses_outputs_inside_installation() {
        let tree = install();
        let private = Temp::new("private-inside");
        let export = private.0.join("export");
        fs::create_dir(&export).expect("export dir");
        let inside_out = tree.0.join("ZBD").join("resolve.json");
        let inside_export = tree.0.join("ZBD");

        for extra in [
            ["--out", path_arg(&inside_out)],
            ["--export-dir", path_arg(&inside_export)],
        ] {
            let mut list = vec![
                "--cs-path",
                path_arg(&tree.0),
                "--world",
                "zbd/c1",
                "--asset",
                "world:texture.zbd",
            ];
            list.extend(extra);
            let run = resolve_command_result(&args(&list), None);
            assert_eq!(run.exit_code, 2, "{:?}", run.diagnostics);
            assert!(run.report.is_none());
        }
        assert!(!inside_out.exists());
        let mut listing: Vec<String> = fs::read_dir(tree.0.join("ZBD"))
            .expect("listable")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        listing.sort();
        assert_eq!(listing, ["c1", "c2"], "the installation is unchanged");
    }

    /// Invalid input and a missing installation fail with their contract
    /// exit codes, never a success.
    #[test]
    fn accept_f04_c_cli_invalid_input_and_missing_installation() {
        let tree = install();
        for (list, code) in [
            (vec!["--asset", "world:../escape.dds"], 2),
            (vec!["--asset", "texture.zbd"], 2),
            (vec!["--world", "zbd/c1"], 2),
            (vec!["--asset", "world:texture.zbd", "--bogus", "x"], 2),
            (vec!["--asset"], 2),
        ] {
            let mut full = vec!["--cs-path", path_arg(&tree.0)];
            full.extend(list.iter().copied());
            let run = resolve_command_result(&args(&full), None);
            assert_eq!(run.exit_code, code, "{list:?}: {:?}", run.diagnostics);
            assert!(run.report.is_none());
        }
        let missing = resolve_command_result(&args(&["--asset", "world:texture.zbd"]), None);
        assert_eq!(missing.exit_code, 4);
        let from_env = resolve_command_result(
            &args(&["--world", "zbd/c1", "--asset", "world:texture.zbd"]),
            Some(tree.0.clone().into_os_string()),
        );
        assert_eq!(from_env.exit_code, 0, "{:?}", from_env.diagnostics);
    }

    /// Retail: the installation mounts into one session with the designed
    /// layout; the first world group that holds a `texture.zbd` resolves
    /// it from its own directory, and the trace lists every other world
    /// group as skipped for its scope. The report is written below
    /// `private/`. Fails loudly without `CS_GAME_DIR`.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f04_c_retail_world_resolves_its_own_texture() {
        let game = std::env::var_os("CS_GAME_DIR")
            .filter(|value| !value.is_empty())
            .expect("CS_GAME_DIR must name the retail installation for this test");
        let root = PathBuf::from(game);
        let found = install::discover(&root).expect("the retail installation is discovered");
        let groups = &found.diagnosis.world_groups;
        let group = groups
            .iter()
            .find(|group| {
                let wanted = format!("{}/texture.zbd", group.logical_key());
                found
                    .manifest
                    .files
                    .iter()
                    .any(|row| row.relative_spelling.logical_key() == wanted)
            })
            .expect("a retail world group holds its own texture.zbd");

        let private = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../private/f04-c");
        fs::create_dir_all(&private).expect("the private output dir is created");
        let out = private.join("resolve-retail.json");
        let run = resolve_command_result(
            &args(&[
                "--cs-path",
                path_arg(&root),
                "--world",
                group.as_str(),
                "--asset",
                "world:texture.zbd",
                "--out",
                path_arg(&out),
            ]),
            None,
        );
        assert_eq!(run.exit_code, 0, "{group}: {:?}", run.diagnostics);
        let report = fs::read_to_string(&out).expect("the report is written");
        let fingerprint = install::fingerprint(&found.manifest).to_hex();
        assert!(report.contains(&format!("\"install_sha256\": \"{fingerprint}\"")));
        assert!(
            report.contains(&format!("\"container_path\": \"{}\"", group.as_str())),
            "{report}"
        );
        for (index, other) in groups.iter().enumerate() {
            let outcome = if other == group {
                "selected"
            } else {
                "scope_mismatch"
            };
            let attempt = format!(
                "{{\"mount\": \"world-{index}\", \"container\": \"{}\", \"precedence\": \
                 \"mission_world\", \"outcome\": \"{outcome}\"}}",
                other.as_str()
            );
            assert!(report.contains(&attempt), "missing {attempt}");
        }
        assert!(report.contains("\"precedence_status\": \"designed\""));
    }
}
