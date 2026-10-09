//! Rally #1157 `EVIDENCE-CONTENT-DIGEST-OUTLIERS`: three committed
//! acceptance reports carry a `content_sha256` that production discovery
//! does not reproduce.
//!
//! `source.content_sha256` is the installation's canonical-content
//! fingerprint — `cs_assets::install::content_fingerprint` over **every**
//! manifest row (`docs/contracts/CLI-EVIDENCE.md`: "installation/
//! canonical-content hash"). Every committed report carries it except
//! three: F05-D, F12-J and T351 each recorded a *task-scoped* digest of
//! only the content their stage exercised, written by private harnesses
//! that were never committed and no longer exist
//! (`docs/findings/2026-10-10-evidence-content-digest-outliers.md`).
//!
//! These tests are the sentinel that keeps the anomaly bounded to those
//! three reports until Rally #1174 repairs them: the first runs anywhere
//! and fails on a new outlier or on a repair that forgets to update the
//! allowlist; the two retail runs re-derive the canonical fingerprints
//! and each scoped value through production code, proving the recorded
//! values are scoped digests rather than install drift — and that the
//! field they sit in means something else.

use std::path::PathBuf;

/// The installation fingerprint every committed retail report carries:
/// production `cs_assets::install::fingerprint` over the manifest.
const CANONICAL_INSTALL: &str = "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";
/// The canonical content fingerprint the same reports carry: production
/// `cs_assets::install::content_fingerprint` over the manifest.
const CANONICAL_CONTENT: &str = "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

/// The three reports that record a task-scoped digest in
/// `source.content_sha256`, with the value each committed. #1174's repair
/// removes entries from this table; anything else changing it is drift.
const OUTLIERS: [(&str, &str); 3] = [
    (
        "F05-D",
        "869f5afcfa632f19c9cfef7a4a1fa1024471a15b7747fab373513a61b50cd23e",
    ),
    (
        "F12-J",
        "89228009451e1841ecf352bc83f1a7ea5a68d51717a04d10d52007c64fd6e502",
    ),
    (
        "T351",
        "e1f7274c663055d3c83fe1401e63d51e493ef51fcd6d3d2d4468dfb2ffdc488f",
    ),
];

/// The ROF container two of the three scoped digests read, as the
/// installation spells it.
const CRIMSON_ROF: &str = "GOSDATA/ASSETS/crimson.rof";
/// The mount namespace `cs-inspect rof` mounts containers under, and the
/// variant the member keys carry.
const ROF_NAMESPACE: &str = "install";

/// Every committed acceptance report, sorted, so a run that finds nothing
/// is visible as a short list rather than as a green empty loop.
fn committed_evidence_reports() -> Vec<PathBuf> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/findings/evidence")
        .canonicalize()
        .expect("docs/findings/evidence exists");
    let mut reports: Vec<PathBuf> = std::fs::read_dir(&directory)
        .expect("the evidence directory is readable")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    reports.sort();
    assert!(
        !reports.is_empty(),
        "{} must hold the committed reports",
        directory.display()
    );
    reports
}

/// The value of a top-level `"key": "value"` string field, `None` when the
/// report carries `null` there (a report measured over no installation).
/// The committed reports are not all pretty-printed — F32-D writes compact
/// JSON — so the colon may be followed by nothing, one space or a newline.
fn report_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    let mut start = text.find(&needle)? + needle.len();
    while text
        .as_bytes()
        .get(start)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        start += 1;
    }
    if text.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    start += 1;
    let end = start + text[start..].find('"')?;
    Some(text[start..end].to_owned())
}

/// One committed report reduced to the fields this sentinel reads.
struct ReportSource {
    task_id: String,
    name: String,
    install_sha256: Option<String>,
    content_sha256: Option<String>,
}

fn report_sources() -> Vec<ReportSource> {
    let reports = committed_evidence_reports();
    assert!(
        reports.len() > 100,
        "every harness's committed copy is here: {} reports",
        reports.len()
    );
    reports
        .iter()
        .map(|report| {
            let text = std::fs::read_to_string(report).expect("a committed report is readable");
            let name = report
                .file_name()
                .expect("a report has a file name")
                .to_string_lossy()
                .into_owned();
            ReportSource {
                task_id: report_string_field(&text, "task_id")
                    .unwrap_or_else(|| panic!("{name}: no task_id")),
                name,
                install_sha256: report_string_field(&text, "install_sha256"),
                content_sha256: report_string_field(&text, "content_sha256"),
            }
        })
        .collect()
}

/// Compare one report's `source` pair against a measured installation:
/// the install fingerprint must match exactly, and the content fingerprint
/// must match unless the report is one of `OUTLIERS` carrying its recorded
/// scoped value. Returns an offender line or `None`.
fn check_report(
    report: &ReportSource,
    install_sha256: &str,
    content_sha256: &str,
) -> Option<String> {
    let measured = report.install_sha256.as_ref()?;
    if measured != install_sha256 {
        return Some(format!(
            "{}: install_sha256 {measured} is not this installation's {install_sha256}",
            report.name
        ));
    }
    let content = report
        .content_sha256
        .as_ref()
        .expect("a report with an install fingerprint carries a content fingerprint");
    if content == content_sha256 {
        // A canonical value where the allowlist still records an outlier
        // means a repair landed without updating this sentinel.
        return OUTLIERS
            .iter()
            .any(|(task_id, _)| *task_id == report.task_id)
            .then(|| {
                format!(
                    "{}: now carries the canonical {content_sha256} — remove it from OUTLIERS",
                    report.name
                )
            });
    }
    match OUTLIERS
        .iter()
        .find(|(task_id, _)| *task_id == report.task_id)
    {
        Some((_, recorded)) if content == recorded => None,
        Some((_, recorded)) => Some(format!(
            "{}: the recorded outlier is {recorded}, but the report carries {content}",
            report.name
        )),
        None => Some(format!(
            "{}: content_sha256 {content} is neither this installation's {content_sha256} \
             nor a recorded outlier — a report must carry the canonical fingerprint",
            report.name
        )),
    }
}

/// The outlier set is exactly the three recorded reports: a report that
/// carries the canonical installation fingerprint but a different content
/// fingerprint is either one of them (with its recorded value) or a new
/// outlier this sentinel exists to catch. Runs anywhere — the constants
/// are the measured values the retail half re-derives.
#[test]
fn accept_evidence_content_digest_outliers_the_outlier_set_is_exactly_the_three_recorded_reports() {
    let mut offenders = Vec::new();
    let mut seen = Vec::new();
    for report in report_sources() {
        if let Some(offender) = check_report(&report, CANONICAL_INSTALL, CANONICAL_CONTENT) {
            offenders.push(offender);
        }
        if OUTLIERS
            .iter()
            .any(|(task_id, _)| *task_id == report.task_id)
        {
            seen.push(report.task_id);
        }
    }
    assert_eq!(
        seen.len(),
        OUTLIERS.len(),
        "each recorded outlier still has its committed report: {seen:?}"
    );
    assert!(
        offenders.is_empty(),
        "reports whose fingerprints a reader cannot explain:\n  {}",
        offenders.join("\n  ")
    );
}

/// The installation whose fingerprints the constants claim.
fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR: the fingerprints are measured over the installation"),
    )
}

/// The same bound, measured: production discovery answers the canonical
/// fingerprints, every non-outlier report agrees with them, and the three
/// outliers still carry exactly their recorded scoped values.
#[test]
#[ignore = "requires CS_GAME_DIR: the installation fingerprint is measured over the original"]
fn accept_evidence_content_digest_outliers_production_fingerprints_reproduce_the_committed_reports()
{
    use cs_assets::install::{content_fingerprint, discover, fingerprint};

    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();
    assert_eq!(install_sha256, CANONICAL_INSTALL, "the constant drifts");
    assert_eq!(content_sha256, CANONICAL_CONTENT, "the constant drifts");

    let mut checked = 0;
    let mut offenders = Vec::new();
    for report in report_sources() {
        if report.install_sha256.is_none() {
            continue;
        }
        if let Some(offender) = check_report(&report, &install_sha256, &content_sha256) {
            offenders.push(offender);
        }
        checked += 1;
    }
    assert!(
        checked > 100,
        "the reports measured over an installation are compared: {checked} of them"
    );
    assert!(
        offenders.is_empty(),
        "reports whose fingerprints production discovery cannot explain:\n  {}",
        offenders.join("\n  ")
    );
}

/// Each outlier's recorded value is re-derived as exactly the scoped
/// digest its own `review.method` (and the #1157 findings) describe —
/// never the canonical fingerprint. This is what proves the divergence is
/// a scoped computation and not stale data: the same installation bytes
/// answer `install_sha256` and the scoped value deterministically.
#[test]
#[ignore = "requires CS_GAME_DIR: the scoped digests are re-derived over the original"]
fn accept_evidence_content_digest_outliers_each_outlier_is_its_documented_scoped_digest() {
    use cs_assets::install::{content_fingerprint, discover, sha256};
    use cs_assets::rof::mount_rof;
    use cs_assets::vfs::MountBuilder;
    use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass};

    let game_dir = game_dir();
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let canonical = content_fingerprint(&found.manifest).to_hex();

    // F05-D: SHA-256 of `"<relpath> <file sha256>\n"` lines for the two ROF
    // containers only — not the whole manifest `content_fingerprint`
    // covers. The container digests come out of the manifest rows.
    let mut lines: Vec<String> = found
        .manifest
        .files
        .iter()
        .filter(|row| row.relative_spelling.logical_key().ends_with(".rof"))
        .map(|row| {
            format!(
                "{} {}\n",
                row.relative_spelling.as_str(),
                row.sha256.to_hex()
            )
        })
        .collect();
    lines.sort();
    let f05_d = sha256(lines.concat().as_bytes()).to_hex();
    assert_eq!(lines.len(), 2, "two containers, sorted: {lines:?}");
    assert_eq!(f05_d, OUTLIERS[0].1, "the F05-D scoped digest re-derives");
    assert_ne!(f05_d, canonical, "a scoped digest is not the canonical one");

    // Both member digests below come out of the production ROF mount:
    // `RofMemberInfo::sha256` is the stored extent digest, and
    // `RofSource::read` returns the decoded bytes its own digest covers.
    let builder = MountBuilder::new(
        MountId::new("rof-evidence-outliers").expect("a valid mount id"),
        MountNamespace::new(ROF_NAMESPACE).expect("a valid namespace"),
        PrecedenceClass::Shared,
        CRIMSON_ROF,
    )
    .retail();
    let mounted =
        mount_rof(builder, &game_dir.join(CRIMSON_ROF)).expect("the retail container mounts");
    let key = |spelling: &str| {
        AssetKey::from_spelling(ROF_NAMESPACE, spelling, "default")
            .expect("a member spelling is a key")
    };

    // F12-J: SHA-256 over the concatenated raw 32-byte decoded digests of
    // ASSETS/LAYOUT.CSV then ASSETS/SCRIPTS/SCRAPBOOKZOOM.SCRIPT.
    let mut raw = Vec::with_capacity(64);
    for spelling in ["ASSETS/LAYOUT.CSV", "ASSETS/SCRIPTS/SCRAPBOOKZOOM.SCRIPT"] {
        let read = mounted
            .source
            .read(&key(spelling))
            .unwrap_or_else(|error| panic!("{spelling} reads: {error:?}"));
        raw.extend_from_slice(sha256(&read.data).as_bytes());
    }
    let f12_j = sha256(&raw).to_hex();
    assert_eq!(f12_j, OUTLIERS[1].1, "the F12-J scoped digest re-derives");
    assert_ne!(f12_j, canonical, "a scoped digest is not the canonical one");

    // T351: SHA-256 of `"<member spelling> <stored sha256>\n"` lines over
    // the 65 members the stage read — every `.CSV`, `.SCRIPT` and `.H` of
    // the container. Stored, not decoded: the recorded value re-derives
    // from `RofMemberInfo::sha256`, the stored extent digest (see the
    // findings: the report's method says "decoded", the value disagrees).
    let mut lines: Vec<String> = mounted
        .source
        .members()
        .filter(|member| {
            member.spelling.ends_with(".CSV")
                || member.spelling.ends_with(".SCRIPT")
                || member.spelling.ends_with(".H")
        })
        .map(|member| format!("{} {}\n", member.spelling, member.sha256.to_hex()))
        .collect();
    lines.sort();
    assert_eq!(
        lines.len(),
        65,
        "the T351 member set: {} members",
        lines.len()
    );
    let t351 = sha256(lines.concat().as_bytes()).to_hex();
    assert_eq!(t351, OUTLIERS[2].1, "the T351 scoped digest re-derives");
    assert_ne!(t351, canonical, "a scoped digest is not the canonical one");
}
