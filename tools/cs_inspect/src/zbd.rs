//! The `zbd-audit` command (F06-D): every ZBD container of an installation,
//! family by family, with one row per member.
//!
//! ```text
//! cs-inspect zbd-audit [--cs-path <dir>] [--strict] [--out <file>]
//! ```
//!
//! The installation is discovered (F02-B), mounted into one
//! [`cs_assets::vfs::ContentSession`] with the designed layout, and every
//! inventoried `.zbd` file is audited through the F06-C producer
//! ([`cs_assets::zbd::audit_container`]): opened by its `install:` key,
//! routed by the two-key dispatch, and — for the sound and reader families,
//! whose member index is the container's own version-one trailer — listed
//! member by member. A corrupt member is a `failed` row **beside** its valid
//! siblings; a corrupt container is a `failed` row beside the other
//! containers. Nothing stops at the first error (spec F06 non-negotiable #4).
//!
//! The report never claims playability: `readiness` is always
//! `not_assessed`. A member is `decoded` (its content was interpreted under
//! its own declaration), `readable` (structurally sound, not interpreted,
//! with the reason) or `failed` (with a stable code).
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` the audit passed;
//! `3` a container or member is corrupt, or — with `--strict` — any content
//! is left uninterpreted (a readable member, a container no F06 reader
//! lists, an index anomaly or an uncovered range); `2` invalid input; `4` no
//! installation selected; `1` a runtime failure. The report is written on
//! exit 3 too, because it is the evidence of why the audit failed.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install;
use cs_assets::vfs::{SessionBuilder, SessionError, SourceError};
use cs_assets::zbd::{ContainerAudit, ContainerVerdict, MemberVerdict, ZbdAudit, audit_containers};
use cs_formats::zbd::ZbdFamily;
use cs_types::asset_id::{AssetKey, ResolveContext};

/// The report format version.
pub const ZBD_AUDIT_REPORT_VERSION: &str = "cs-inspect-zbd-audit/1";

/// Parsed `zbd-audit` arguments.
#[derive(Debug, Default)]
struct AuditArgs {
    cs_path: Option<PathBuf>,
    strict: bool,
    out: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<AuditArgs, String> {
    let mut parsed = AuditArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        match arg.as_str() {
            "--strict" => parsed.strict = true,
            flag @ ("--cs-path" | "--out") => {
                let Some(value) = cursor.next() else {
                    return Err(format!("cs-inspect zbd-audit: {flag} needs a value"));
                };
                let slot = if flag == "--cs-path" {
                    &mut parsed.cs_path
                } else {
                    &mut parsed.out
                };
                *slot = Some(PathBuf::from(value));
            }
            other => {
                return Err(format!(
                    "cs-inspect zbd-audit: unsupported argument {other:?}; expected --cs-path \
                     <dir>, --strict, --out <file>"
                ));
            }
        }
    }
    Ok(parsed)
}

/// Everything one `zbd-audit` run produced.
#[derive(Debug)]
pub struct ZbdAuditRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The audit, when it ran.
    pub audit: Option<ZbdAudit>,
    /// The JSON report, when the audit ran.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
}

impl ZbdAuditRun {
    fn failed(exit_code: u8, message: String) -> Self {
        Self {
            exit_code,
            audit: None,
            report: None,
            out: None,
            diagnostics: vec![message],
        }
    }
}

/// Runs the `zbd-audit` command and returns its exit code.
pub fn zbd_audit_command(args: &[String]) -> ExitCode {
    let run = zbd_audit_command_result(args, std::env::var_os("CS_GAME_DIR"));
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote zbd audit report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// The body of [`zbd_audit_command`], with the environment's installation
/// passed in so tests can drive it.
pub fn zbd_audit_command_result(args: &[String], env_cs_path: Option<OsString>) -> ZbdAuditRun {
    let parsed = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return ZbdAuditRun::failed(2, message),
    };
    let cs_path = parsed.cs_path.clone().or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return ZbdAuditRun::failed(
            4,
            "no installation selected: pass --cs-path <dir> or set CS_GAME_DIR".to_owned(),
        );
    };
    if let Some(out) = &parsed.out
        && inside(out, &cs_path)
    {
        return ZbdAuditRun::failed(
            2,
            format!(
                "{} lies inside the installation; cs-inspect never writes there",
                out.display()
            ),
        );
    }

    let found = match install::discover(&cs_path) {
        Ok(found) => found,
        Err(error) => return ZbdAuditRun::failed(1, error.to_string()),
    };
    let context = ResolveContext::new(install::fingerprint(&found.manifest));
    let mut builder = SessionBuilder::new(context);
    if let Err(error) = builder.mount_installation(&cs_path, &found.diagnosis) {
        let code = match &error {
            SessionError::Source {
                error: SourceError::Member { .. } | SourceError::NonUtf8Name { .. },
                ..
            } => 3,
            _ => 1,
        };
        return ZbdAuditRun::failed(code, error.to_string());
    }
    let session = builder.open();

    // Every inventoried `.zbd` file, in spelling order, with its hash.
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut keys = Vec::new();
    let mut diagnostics = Vec::new();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        if !record.relative_spelling.logical_key().ends_with(".zbd") {
            continue;
        }
        match AssetKey::from_spelling("install", spelling, "default") {
            Ok(key) => {
                hashes.insert(key.to_string(), record.sha256.to_hex());
                keys.push(key);
            }
            Err(error) => diagnostics.push(format!("{spelling}: not an asset key: {error}")),
        }
    }
    keys.sort_by_key(ToString::to_string);
    let audit = audit_containers(&session, &keys);
    let _teardown = session.close();

    let passes = audit.passes(parsed.strict) && diagnostics.is_empty();
    for container in &audit.containers {
        diagnostics.extend(container_diagnostics(container));
    }
    let report = zbd_audit_report_json(&found, &audit, &hashes, parsed.strict, passes);
    let mut exit_code = if passes { 0 } else { 3 };

    let out = match &parsed.out {
        Some(out) => match write_report(out, &report) {
            Ok(()) => Some(out.clone()),
            Err(error) => {
                diagnostics.push(format!("cannot write report to {}: {error}", out.display()));
                exit_code = 1;
                None
            }
        },
        None => None,
    };
    ZbdAuditRun {
        exit_code,
        audit: Some(audit),
        report: Some(report),
        out,
        diagnostics,
    }
}

/// One stderr line per corrupt container or member.
fn container_diagnostics(container: &ContainerAudit) -> Vec<String> {
    let label = container
        .path
        .clone()
        .unwrap_or_else(|| container.key.to_string());
    let mut lines = Vec::new();
    if let ContainerVerdict::Failed { code, reason } = &container.verdict {
        lines.push(format!("{label}: failed ({code}): {reason}"));
    }
    for member in container.failed_members() {
        if let MemberVerdict::Failed { code, reason } = &member.verdict {
            lines.push(format!(
                "{label}: member {} {}: failed ({code}): {reason}",
                member.index,
                name_text(&member.name)
            ));
        }
    }
    lines
}

/// Per-family totals.
#[derive(Default)]
struct FamilyTotals {
    containers: usize,
    listed: usize,
    not_listed: usize,
    failed: usize,
    members: usize,
    decoded: usize,
    readable: usize,
    failed_members: usize,
}

/// Renders the audit report.
pub fn zbd_audit_report_json(
    found: &install::Discovery,
    audit: &ZbdAudit,
    hashes: &BTreeMap<String, String>,
    strict: bool,
    passes: bool,
) -> String {
    let mut families: BTreeMap<&'static str, FamilyTotals> = ZbdFamily::ALL
        .iter()
        .map(|family| (family.as_str(), FamilyTotals::default()))
        .collect();
    let mut unrouted = FamilyTotals::default();
    for container in &audit.containers {
        let totals = match container.family {
            Some(family) => families.entry(family.as_str()).or_default(),
            None => &mut unrouted,
        };
        totals.containers += 1;
        match container.verdict {
            ContainerVerdict::Listed => totals.listed += 1,
            ContainerVerdict::NotListed { .. } => totals.not_listed += 1,
            ContainerVerdict::Failed { .. } => totals.failed += 1,
        }
        totals.members += container.members.len();
        totals.decoded += container.decoded_members();
        totals.readable += container.readable_members();
        totals.failed_members += container.failed_members().count();
    }
    let mut family_rows: Vec<String> = families
        .iter()
        .map(|(name, totals)| family_json(&jstr(name), totals))
        .collect();
    if unrouted.containers > 0 {
        family_rows.push(family_json("null", &unrouted));
    }
    let members: usize = audit.containers.iter().map(|c| c.members.len()).sum();
    let containers: Vec<String> = audit
        .containers
        .iter()
        .map(|container| container_json(container, hashes))
        .collect();
    format!(
        "{{\n\
         \x20\"report_version\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"content_sha256\": {},\n\
         \x20\"check\": \"zbd_containers_and_members\",\n\
         \x20\"strict\": {strict},\n\
         \x20\"passes\": {passes},\n\
         \x20\"readiness\": \"not_assessed\",\n\
         \x20\"summary\": {{\"containers\": {}, \"members\": {members}, \"failures\": {}, \
         \"uninterpreted\": {}}},\n\
         \x20\"families\": [\n  {}\n ],\n\
         \x20\"containers\": [\n  {}\n ]\n\
         }}\n",
        jstr(ZBD_AUDIT_REPORT_VERSION),
        jstr(&install::fingerprint(&found.manifest).to_hex()),
        jstr(&install::content_fingerprint(&found.manifest).to_hex()),
        audit.containers.len(),
        audit.failures(),
        audit.uninterpreted(),
        family_rows.join(",\n  "),
        containers.join(",\n  "),
    )
}

fn family_json(name: &str, totals: &FamilyTotals) -> String {
    format!(
        "{{\"family\": {name}, \"containers\": {}, \"listed\": {}, \"not_listed\": {}, \
         \"failed\": {}, \"members\": {}, \"decoded\": {}, \"readable\": {}, \
         \"failed_members\": {}}}",
        totals.containers,
        totals.listed,
        totals.not_listed,
        totals.failed,
        totals.members,
        totals.decoded,
        totals.readable,
        totals.failed_members,
    )
}

fn container_json(container: &ContainerAudit, hashes: &BTreeMap<String, String>) -> String {
    let key = container.key.to_string();
    let mut out = format!(
        "{{\"key\": {}, \"sha256\": {}, \"mount\": {}, \"generation\": {}, \"path\": {}, \
         \"bytes\": {}, \"family\": {}, \"basis\": {}, \"header\": {}, \"verdict\": {}",
        jstr(&key),
        opt_str(hashes.get(&key).map(String::as_str)),
        opt_str(container.mount.as_deref()),
        opt_num(container.generation),
        opt_str(container.path.as_deref()),
        opt_num(container.container_len),
        opt_str(container.family.map(ZbdFamily::as_str)),
        opt_str(container.basis),
        opt_str(container.header),
        jstr(container.verdict.label()),
    );
    match &container.verdict {
        ContainerVerdict::Listed => {}
        ContainerVerdict::NotListed { reason } => {
            let _ = write!(out, ", \"reason\": {}", jstr(reason));
        }
        ContainerVerdict::Failed { code, reason } => {
            let _ = write!(
                out,
                ", \"code\": {}, \"reason\": {}",
                jstr(code),
                jstr(reason)
            );
        }
    }
    let uncovered: Vec<String> = container
        .uncovered
        .iter()
        .map(|span| format!("[{}, {}]", span.offset, span.length))
        .collect();
    let members: Vec<String> = container.members.iter().map(member_json).collect();
    let _ = write!(
        out,
        ", \"failures\": {}, \"uninterpreted\": {}, \"uncovered\": [{}], \"members\": [{}]}}",
        container.failures(),
        container.uninterpreted(),
        uncovered.join(", "),
        members.join(", ")
    );
    out
}

fn member_json(member: &cs_assets::zbd::MemberAudit) -> String {
    let anomalies: Vec<String> = member.anomalies.iter().map(|code| jstr(code)).collect();
    let detail = match &member.verdict {
        MemberVerdict::Decoded { detail } => format!(", \"detail\": {}", jstr(detail)),
        MemberVerdict::Readable { reason } => format!(", \"reason\": {}", jstr(reason)),
        MemberVerdict::Failed { code, reason } => {
            format!(", \"code\": {}, \"reason\": {}", jstr(code), jstr(reason))
        }
    };
    format!(
        "{{\"index\": {}, \"name\": {}, \"offset\": {}, \"length\": {}, \"anomalies\": [{}], \
         \"verdict\": {}{detail}}}",
        member.index,
        jstr(&name_text(&member.name)),
        member.span.offset,
        member.span.length,
        anomalies.join(", "),
        jstr(member.verdict.label()),
    )
}

/// A member name as text: printable ASCII verbatim, every other byte as
/// `\xNN`, so the rendering is lossless and never decodes the bytes.
pub fn name_text(name: &[u8]) -> String {
    let mut text = String::with_capacity(name.len());
    for &byte in name {
        if (0x20..0x7F).contains(&byte) && byte != b'\\' {
            text.push(char::from(byte));
        } else {
            let _ = write!(text, "\\x{byte:02x}");
        }
    }
    text
}

fn opt_str(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), jstr)
}

fn opt_num(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_owned(), |number| number.to_string())
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
    //! Acceptance stage F06-D (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
    //! section `### F06-D`, AC04): a corrupt member is shown beside its valid
    //! siblings in the audit output while the strict status is nonzero.
    //!
    //! The synthetic trees are newly authored bytes under the system
    //! temporary directory, removed on drop. The retail tests read
    //! `$CS_GAME_DIR` (never write it), fail loudly without it, and copy the
    //! two small archives the corruption test needs into the Git-ignored
    //! `private/` directory of this checkout. The inline shape follows F04-C's
    //! `resolve.rs`: `tools/cs_inspect/tests/` is not an owner path of F06-D.
    //! `evidence_report_f06_d_writes_the_acceptance_report` is the evidence
    //! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.

    use std::collections::{BTreeMap, VecDeque};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{ZbdAuditRun, zbd_audit_command_result};
    use cs_assets::zbd::{ContainerAudit, ContainerVerdict, MemberVerdict, ZbdAudit};
    use cs_formats::zbd::{
        GAMEZ_SIGNATURE, GAMEZ_VERSION, INDEX_ENTRY_BYTES, INDEX_NAME_BYTES,
        INDEX_UNEXPLAINED_BYTES, INTERP_SIGNATURE, INTERP_VERSION, TRAILER_VERSION_ONE,
        WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_PCM, ZbdFamily,
    };

    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable fixture directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn under(parent: &Path, label: &str) -> Self {
            let root = parent.join(format!(
                "cs-f06-d-{label}-{}-{}",
                std::process::id(),
                NEXT_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("the fixture root is created");
            Self(root)
        }

        fn new(label: &str) -> Self {
            Self::under(&std::env::temp_dir(), label)
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

    fn run(root: &Path, extra: &[&str]) -> ZbdAuditRun {
        let mut list = vec!["--cs-path", root.to_str().expect("a UTF-8 fixture path")];
        list.extend_from_slice(extra);
        zbd_audit_command_result(&args(&list), None)
    }

    // --- authored members and archives --------------------------------------

    /// A RIFF/WAVE member: `fmt ` (tag, channels, rate, block align, bits) and
    /// `data`, laid out as the RIFF specification (IBM/Microsoft 1991) states.
    fn wave(tag: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let (channels, rate) = (1u16, 11_025u32);
        let align = if tag == WAVE_FORMAT_PCM {
            bits / 8
        } else {
            256
        };
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&tag.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
        fmt.extend_from_slice(&align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        let mut chunks = b"fmt ".to_vec();
        chunks.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        chunks.extend_from_slice(&fmt);
        chunks.extend_from_slice(b"data");
        chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
        chunks.extend_from_slice(data);
        if data.len() % 2 == 1 {
            chunks.push(0);
        }
        let mut member = b"RIFF".to_vec();
        member.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
        member.extend_from_slice(b"WAVE");
        member.extend_from_slice(&chunks);
        member
    }

    /// 8-bit PCM with `frames` samples.
    fn pcm(frames: usize) -> Vec<u8> {
        wave(WAVE_FORMAT_PCM, 8, &vec![0x80; frames])
    }

    /// Members back to back, then task #343's version-one index and trailer.
    /// `stretch` lengthens one entry's declared length so it reaches into the
    /// index (past `table_start`, not past the file).
    fn archive(members: &[(&[u8], Vec<u8>)], stretch: Option<(usize, u32)>) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut index = Vec::new();
        for (position, (name, body)) in members.iter().enumerate() {
            let extra = stretch
                .filter(|(which, _)| *which == position)
                .map_or(0, |(_, by)| by);
            index.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            index.extend_from_slice(&(body.len() as u32 + extra).to_le_bytes());
            let mut field = vec![0u8; INDEX_NAME_BYTES];
            field[..name.len()].copy_from_slice(name);
            index.extend_from_slice(&field);
            index.extend_from_slice(&[0x5A; INDEX_UNEXPLAINED_BYTES]);
            bytes.extend_from_slice(body);
        }
        assert_eq!(index.len(), members.len() * INDEX_ENTRY_BYTES as usize);
        bytes.extend_from_slice(&index);
        bytes.extend_from_slice(&TRAILER_VERSION_ONE.to_le_bytes());
        bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
        bytes
    }

    fn header(signature: u32, version: u32) -> Vec<u8> {
        let mut bytes = signature.to_le_bytes().to_vec();
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&[0; 32]);
        bytes
    }

    fn container<'a>(audit: &'a ZbdAudit, path: &str) -> &'a ContainerAudit {
        audit
            .containers
            .iter()
            .find(|row| row.key.to_string().ends_with(path))
            .unwrap_or_else(|| panic!("{path} has a row"))
    }

    fn verdicts(row: &ContainerAudit) -> Vec<&'static str> {
        row.members.iter().map(|m| m.verdict.label()).collect()
    }

    fn failure_code(verdict: &MemberVerdict) -> Option<&'static str> {
        match verdict {
            MemberVerdict::Failed { code, .. } => Some(code),
            _ => None,
        }
    }

    // --- synthetic ------------------------------------------------------------

    /// AC04, the stage's minimum scenario.
    #[test]
    fn accept_f06_d_a_corrupt_member_is_shown_beside_valid_siblings_with_a_nonzero_strict_status() {
        let tree = Temp::new("ac04");
        tree.write(
            "ZBD/soundsl.zbd",
            &archive(
                &[
                    (b"first.wav", pcm(6)),
                    (b"broken.wav", b"this member is not a RIFF file".to_vec()),
                    (b"second.wav", pcm(4)),
                    (b"first.wav", pcm(2)),
                    (b"last.wav", pcm(3)),
                ],
                Some((4, 40)),
            ),
        );
        tree.write(
            "ZBD/zrdr.zbd",
            &archive(
                &[(b"a.zrd", b"alpha".to_vec()), (b"b.zrd", b"bravo".to_vec())],
                None,
            ),
        );
        tree.write("ZBD/interp.zbd", &header(INTERP_SIGNATURE, INTERP_VERSION));
        let out = tree
            .0
            .parent()
            .expect("temp")
            .join(format!("cs-f06-d-ac04-report-{}.json", std::process::id()));

        for strict in [true, false] {
            let mut extra = vec!["--out", out.to_str().expect("UTF-8")];
            if strict {
                extra.push("--strict");
            }
            let result = run(&tree.0, &extra);
            assert_eq!(
                result.exit_code, 3,
                "strict={strict}: {:?}",
                result.diagnostics
            );
            let audit = result.audit.as_ref().expect("the audit ran");
            assert_eq!(audit.containers.len(), 3);

            let sound = container(audit, "ZBD/soundsl.zbd");
            assert_eq!(sound.verdict, ContainerVerdict::Listed);
            assert_eq!(sound.family, Some(ZbdFamily::Sound));
            // Every declared member has a row, duplicates included, in order.
            assert_eq!(
                verdicts(sound),
                ["decoded", "failed", "decoded", "decoded", "failed"]
            );
            let names: Vec<&[u8]> = sound.members.iter().map(|m| m.name.as_slice()).collect();
            assert_eq!(names[0], names[3], "the duplicate name stays two rows");
            assert_eq!(failure_code(&sound.members[1].verdict), Some("not_riff"));
            assert_eq!(
                failure_code(&sound.members[4].verdict),
                Some("member_out_of_bounds")
            );
            assert_eq!(sound.failures(), 2);
            match &sound.members[0].verdict {
                MemberVerdict::Decoded { detail } => {
                    assert!(detail.contains("6 frames"), "{detail}");
                }
                other => panic!("{other:?}"),
            }

            let reader = container(audit, "ZBD/zrdr.zbd");
            assert_eq!(verdicts(reader), ["readable", "readable"]);
            let interp = container(audit, "ZBD/interp.zbd");
            assert!(matches!(interp.verdict, ContainerVerdict::NotListed { .. }));
            assert_eq!(interp.header, Some("validated"));
            assert_eq!(audit.failures(), 2);

            // The written report shows the corrupt rows beside their siblings.
            let report = fs::read_to_string(&out).expect("the report was written");
            assert_eq!(result.report.as_deref(), Some(report.as_str()));
            assert!(report.contains(&format!("\"strict\": {strict}")));
            assert!(report.contains("\"passes\": false"));
            assert!(report.contains("\"readiness\": \"not_assessed\""));
            assert!(report.contains(
                "{\"index\": 1, \"name\": \"broken.wav\", \"offset\": 50, \"length\": 30, \
                 \"anomalies\": [], \"verdict\": \"failed\", \"code\": \"not_riff\""
            ));
            assert!(report.contains("{\"index\": 2, \"name\": \"second.wav\""));
            assert!(report.contains(
                "{\"family\": \"sound\", \"containers\": 1, \"listed\": 1, \"not_listed\": 0, \
                 \"failed\": 0, \"members\": 5, \"decoded\": 3, \"readable\": 0, \
                 \"failed_members\": 2}"
            ));
            // And stderr names each corrupt member.
            let named: Vec<&String> = result
                .diagnostics
                .iter()
                .filter(|line| line.contains("ZBD/soundsl.zbd: member"))
                .collect();
            assert_eq!(named.len(), 2, "{:?}", result.diagnostics);
            assert!(named[0].contains("member 1 broken.wav: failed (not_riff)"));
        }
        let _ = fs::remove_file(&out);
    }

    #[test]
    fn accept_f06_d_strict_fails_on_uninterpreted_content_and_passes_a_decoded_corpus() {
        let tree = Temp::new("strict");
        tree.write(
            "ZBD/soundsl.zbd",
            &archive(&[(b"a.wav", pcm(2)), (b"b.wav", pcm(4))], None),
        );
        let clean = run(&tree.0, &["--strict"]);
        assert_eq!(clean.exit_code, 0, "{:?}", clean.diagnostics);
        let report = clean.report.expect("a report");
        assert!(report.contains("\"passes\": true"));
        assert!(report.contains("\"summary\": {\"containers\": 1, \"members\": 2, \"failures\": 0, \"uninterpreted\": 0}"));

        // An ADPCM member is sound but not decoded: a readable row carrying
        // its own tag. Only strict fails on it.
        tree.write(
            "ZBD/soundsh.zbd",
            &archive(
                &[(b"ima.wav", wave(WAVE_FORMAT_IMA_ADPCM, 4, &[0; 256]))],
                None,
            ),
        );
        let lenient = run(&tree.0, &[]);
        assert_eq!(lenient.exit_code, 0, "{:?}", lenient.diagnostics);
        let strict = run(&tree.0, &["--strict"]);
        assert_eq!(strict.exit_code, 3);
        let audit = strict.audit.expect("the audit ran");
        let adpcm = container(&audit, "ZBD/soundsh.zbd");
        match &adpcm.members[0].verdict {
            MemberVerdict::Readable { reason } => {
                assert!(reason.contains("0x0011 (ima_adpcm)"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(audit.uninterpreted(), 1);
        assert_eq!(audit.failures(), 0);

        // A family F06 does not member-list is uninterpreted too.
        let tree = Temp::new("strict-gamez");
        tree.write("ZBD/planes.zbd", &header(GAMEZ_SIGNATURE, GAMEZ_VERSION));
        assert_eq!(run(&tree.0, &[]).exit_code, 0);
        let strict = run(&tree.0, &["--strict"]);
        assert_eq!(strict.exit_code, 3);
        let audit = strict.audit.expect("the audit ran");
        let planes = container(&audit, "ZBD/planes.zbd");
        match &planes.verdict {
            ContainerVerdict::NotListed { reason } => assert!(reason.contains("F10"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn accept_f06_d_a_corrupt_container_is_a_row_beside_the_others() {
        let tree = Temp::new("container");
        tree.write("ZBD/soundsl.zbd", &archive(&[(b"a.wav", pcm(2))], None));
        // A trailer of another version, and a GameZ header at the interp role.
        let mut versioned = archive(&[(b"a.wav", pcm(2))], None);
        let at = versioned.len() - 8;
        versioned[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
        tree.write("ZBD/soundsh.zbd", &versioned);
        tree.write("ZBD/interp.zbd", &header(GAMEZ_SIGNATURE, GAMEZ_VERSION));

        let result = run(&tree.0, &[]);
        assert_eq!(result.exit_code, 3);
        let audit = result.audit.expect("the audit ran");
        assert_eq!(audit.containers.len(), 3);
        let codes: BTreeMap<String, &str> = audit
            .containers
            .iter()
            .map(|row| {
                let code = match &row.verdict {
                    ContainerVerdict::Failed { code, .. } => code,
                    other => other.label(),
                };
                (row.key.to_string(), code)
            })
            .collect();
        assert_eq!(
            codes.values().copied().collect::<Vec<_>>(),
            ["dispatch", "unsupported_trailer_version", "listed"],
            "{codes:?}"
        );
        let good = container(&audit, "ZBD/soundsl.zbd");
        assert_eq!(verdicts(good), ["decoded"]);
        assert_eq!(audit.failures(), 2);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|line| line.contains("ZBD/soundsh.zbd: failed (unsupported_trailer_version)"))
        );
    }

    #[test]
    fn accept_f06_d_cli_refuses_bad_input_and_a_missing_installation() {
        assert_eq!(zbd_audit_command_result(&[], None).exit_code, 4);
        assert_eq!(
            zbd_audit_command_result(&args(&["--bogus"]), None).exit_code,
            2
        );
        assert_eq!(
            zbd_audit_command_result(&args(&["--out"]), None).exit_code,
            2
        );
        let tree = Temp::new("inside");
        tree.write("ZBD/soundsl.zbd", &archive(&[(b"a.wav", pcm(2))], None));
        let inside = tree.0.join("report.json");
        let result = run(&tree.0, &["--out", inside.to_str().expect("UTF-8")]);
        assert_eq!(result.exit_code, 2);
        assert!(
            !inside.exists(),
            "nothing is written inside the installation"
        );
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

    /// Every `.zbd` file under `root`, as `(spelling, host path)`.
    fn zbd_files(root: &Path) -> Vec<(String, PathBuf)> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).expect("readable directory") {
                let path = entry.expect("readable entry").path();
                if path.is_dir() {
                    pending.push(path);
                } else if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("zbd"))
                {
                    let spelling = path
                        .strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    found.push((spelling, path));
                }
            }
        }
        found.sort();
        found
    }

    fn le_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    /// Independent of the production readers: for one version-one archive,
    /// the declared member count and how many members' `fmt ` tag (at member
    /// offset 20, where task #344 found every retail member's `fmt ` payload)
    /// is PCM.
    fn independent_counts(bytes: &[u8], sound: bool) -> (usize, usize) {
        let size = bytes.len();
        assert_eq!(le_u32(bytes, size - 8), 1, "trailer version one");
        let count = le_u32(bytes, size - 4) as usize;
        let table = size - 8 - count * INDEX_ENTRY_BYTES as usize;
        let mut pcm = 0;
        for entry in 0..count {
            let at = table + entry * INDEX_ENTRY_BYTES as usize;
            let start = le_u32(bytes, at) as usize;
            if sound && bytes[start + 12..start + 16] == *b"fmt " {
                pcm += usize::from(u16::from_le_bytes([bytes[start + 20], bytes[start + 21]]) == 1);
            }
        }
        (count, pcm)
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f06_d_retail_every_zbd_container_is_audited_family_by_family() {
        let root = game_dir();
        let result = run(&root, &["--strict"]);
        let audit = result
            .audit
            .as_ref()
            .expect("the audit ran over the installation");

        // Every `.zbd` file on disk has exactly one row.
        let files = zbd_files(&root);
        assert_eq!(audit.containers.len(), files.len());
        assert_eq!(files.len(), 184, "task #340 counted 184 archives");

        let mut per_family: BTreeMap<&str, usize> = BTreeMap::new();
        for row in &audit.containers {
            let family = row.family.map_or("unrouted", ZbdFamily::as_str);
            *per_family.entry(family).or_default() += 1;
        }
        assert_eq!(
            per_family.into_iter().collect::<Vec<_>>(),
            [
                ("animation", 61),
                ("gamez", 9),
                ("interp", 1),
                ("reader", 62),
                ("sound", 2),
                ("texture", 49),
            ]
        );
        // The retail corpus is structurally intact: nothing is corrupt.
        assert_eq!(audit.failures(), 0, "{:?}", result.diagnostics);
        assert!(audit.passes(false));

        // Member rows match an independent read of every trailer, and the
        // decoded members are exactly the PCM members.
        let mut expected_members = 0;
        let mut expected_pcm = 0;
        for (spelling, host) in &files {
            let row = container(audit, spelling);
            match row.family {
                Some(family @ (ZbdFamily::Sound | ZbdFamily::Reader)) => {
                    let bytes = fs::read(host).expect("readable archive");
                    let (count, pcm) = independent_counts(&bytes, family == ZbdFamily::Sound);
                    assert_eq!(row.members.len(), count, "{spelling}");
                    assert_eq!(row.decoded_members(), pcm, "{spelling}");
                    assert_eq!(row.verdict, ContainerVerdict::Listed, "{spelling}");
                    assert!(
                        row.uncovered.is_empty(),
                        "{spelling}: members tile the data"
                    );
                    expected_members += count;
                    expected_pcm += pcm;
                }
                _ => {
                    assert!(
                        matches!(row.verdict, ContainerVerdict::NotListed { .. }),
                        "{spelling}"
                    );
                    assert!(row.members.is_empty());
                }
            }
        }
        let members: usize = audit.containers.iter().map(|row| row.members.len()).sum();
        let decoded: usize = audit
            .containers
            .iter()
            .map(ContainerAudit::decoded_members)
            .sum();
        assert_eq!(members, expected_members);
        assert_eq!(members, 6334, "task #343 counted 6334 members");
        assert_eq!(decoded, expected_pcm);
        // Task #344: 11 PCM members in `soundsl`, 11 in `soundsh`.
        assert_eq!(decoded, 22, "task #344 counted 22 PCM members");

        // Strict: the corpus is not fully interpreted, and says so.
        assert_eq!(result.exit_code, 3);
        assert_eq!(audit.uninterpreted(), members - decoded + 184 - 64);
        println!(
            "{} containers, {members} members, {decoded} decoded, {} uninterpreted",
            audit.containers.len(),
            audit.uninterpreted()
        );
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f06_d_retail_a_corrupted_copy_fails_beside_its_valid_siblings() {
        let root = game_dir();
        let private = workspace_root().join("private");
        fs::create_dir_all(&private).expect("the private directory exists");
        let tree = Temp::under(&private, "retail-copy");
        let original = fs::read(root.join("ZBD/C1/MP1/zrdr.zbd")).expect("a retail archive");
        let sibling = fs::read(root.join("ZBD/C1/MP2/zrdr.zbd")).expect("a retail archive");
        let (count, _) = independent_counts(&original, false);
        assert!(count > 2);

        // Stretch the last member's declared length into the index.
        let mut corrupted = original.clone();
        let size = corrupted.len();
        let last = size - 8 - INDEX_ENTRY_BYTES as usize;
        let length = le_u32(&corrupted, last + 4);
        corrupted[last + 4..last + 8].copy_from_slice(&(length + 16).to_le_bytes());
        tree.write("ZBD/C1/MP1/zrdr.zbd", &corrupted);
        tree.write("ZBD/C1/MP2/zrdr.zbd", &sibling);

        let intact = Temp::under(&private, "retail-intact");
        intact.write("ZBD/C1/MP1/zrdr.zbd", &original);
        let baseline = run(&intact.0, &[]);
        assert_eq!(baseline.exit_code, 0, "{:?}", baseline.diagnostics);

        let out = private.join(format!("f06-d-corrupted-{}.json", std::process::id()));
        let result = run(
            &tree.0,
            &["--strict", "--out", out.to_str().expect("UTF-8")],
        );
        assert_eq!(result.exit_code, 3);
        let audit = result.audit.expect("the audit ran");
        let row = container(&audit, "ZBD/C1/MP1/zrdr.zbd");
        assert_eq!(row.members.len(), count);
        assert_eq!(row.failures(), 1);
        assert_eq!(
            failure_code(&row.members[count - 1].verdict),
            Some("member_out_of_bounds")
        );
        assert!(
            row.members[..count - 1]
                .iter()
                .all(|member| member.verdict.label() == "readable")
        );
        let base = container(baseline.audit.as_ref().expect("ran"), "ZBD/C1/MP1/zrdr.zbd");
        assert_eq!(&row.members[..count - 1], &base.members[..count - 1]);
        let other = container(&audit, "ZBD/C1/MP2/zrdr.zbd");
        assert_eq!(other.failures(), 0);
        let report = fs::read_to_string(&out).expect("the report was written");
        assert!(report.contains("\"code\": \"member_out_of_bounds\""));
        assert!(report.contains("\"passes\": false"));
        let _ = fs::remove_file(&out);
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
        // Howard Hinnant's `civil_from_days`.
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

    /// Evidence-report harness for F06-D (`docs/contracts/CLI-EVIDENCE.md`,
    /// schema `schemas/evidence.schema.json`). Not an acceptance test: it
    /// fails loudly when its inputs are missing. From the workspace root:
    ///
    /// 1. ```sh
    ///    mkdir -p private/evidence/F06-D
    ///    cargo test --workspace --locked -- accept_f06_d_ --include-ignored \
    ///      2>&1 | tee private/evidence/F06-D/cargo-test.log
    ///    ```
    ///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
    /// 2. ```sh
    ///    CS_EVIDENCE_DIR=private/evidence/F06-D \
    ///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
    ///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f06_d_ --include-ignored" \
    ///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
    ///      cargo test --locked -p cs_inspect --lib -- evidence_report_f06_d --ignored
    ///    ```
    ///    This runs the production `zbd-audit` command over `$CS_GAME_DIR`
    ///    with `--strict` and keeps its report as the artifact
    ///    `zbd-audit.json`.
    /// 3. ```sh
    ///    python3 tools/validate_evidence.py private/evidence/F06-D/acceptance.json \
    ///      --artifact-root private/evidence/F06-D --require-pass
    ///    ```
    /// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F06-D.json`.
    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f06_d_writes_the_acceptance_report() {
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
        let ([passed, failed, ignored], results) = parse_suite(&log, "accept_f06_d_");
        assert!(
            passed > 0 && !results.is_empty(),
            "no accept_f06_d_ tests in the log"
        );
        for retail in [
            "accept_f06_d_retail_every_zbd_container_is_audited_family_by_family",
            "accept_f06_d_retail_a_corrupted_copy_fails_beside_its_valid_siblings",
        ] {
            let status = results
                .iter()
                .find(|(name, _)| short_name(name) == retail)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| panic!("{retail} did not run: use --include-ignored"));
            assert_eq!(status, "pass", "{retail} must pass");
        }

        // The production command over the installation, kept as the artifact.
        let audit_path = evidence_dir.join("zbd-audit.json");
        let audited = run(
            &root,
            &["--strict", "--out", audit_path.to_str().expect("UTF-8")],
        );
        assert_eq!(audited.out.as_deref(), Some(audit_path.as_path()));
        let audit = audited.audit.expect("the audit ran");
        assert_eq!(
            audit.failures(),
            0,
            "the retail corpus audit must find no corruption"
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
        let members: usize = audit.containers.iter().map(|row| row.members.len()).sum();
        let decoded: usize = audit
            .containers
            .iter()
            .map(ContainerAudit::decoded_members)
            .sum();
        // What stays uninterpreted is part of the method text: the schema's
        // `unknowns` must be empty for a passing report, and the unknowns
        // themselves are recorded in the F06-D findings.
        let method = format!(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production `cs-inspect zbd-audit --strict` run over every retail ZBD container \
             (zbd-audit.json; the retail acceptance test checks its member and PCM counts \
             against an independent read of every trailer and fmt tag), rustc and Cargo.lock; \
             validated with tools/validate_evidence.py --require-pass. The audit found {} \
             corrupt containers or members; {} of {members} members are structurally sound but \
             not interpreted (reader encoding undocumented, ADPCM not decoded) and the \
             texture, interp, GameZ and animation containers are routed but not member-listed, \
             so the strict audit exits {}; these unknowns are recorded in \
             docs/findings/2026-09-28-f06-d-zbd-corpus-audit.md",
            audit.failures(),
            members - decoded,
            audited.exit_code
        );
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F06-D\",\n\
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
            artifact(&audit_path, "json"),
            super::jstr(
                "claude-1 (implementing agent, self-check; the Rally reviewer regenerates this \
                 report on the rebased commit)"
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
