//! Rally #1157 `EVIDENCE-CONTENT-DIGEST-OUTLIERS`: three committed
//! acceptance reports carried a `content_sha256` that production discovery
//! does not reproduce — a *task-scoped* digest of only the content their
//! stage exercised, written by private harnesses that were never committed
//! (`docs/findings/2026-10-10-evidence-content-digest-outliers.md`).
//!
//! Rally #1174 `EVIDENCE-CONTENT-DIGEST-OUTLIERS-REPAIR` repaired all three
//! by reissuing them through the committed harnesses
//! `crates/cs_app/tests/campaign/evidence/{f05_d,f12_j,t351}.rs`, which
//! derive `source.content_sha256` from production `discover` +
//! `content_fingerprint`, keep `review.identity` byte-unchanged and append
//! the regeneration facts to `review.method`. The allowlist of outliers is
//! therefore **empty**, and that is what this file pins:
//!
//! * the first test runs anywhere and fails on any report whose content
//!   fingerprint is not this installation's canonical one, so a scoped
//!   digest can never come back — including by reverting one of the three
//!   reissued reports;
//! * the second re-measures the pair through production discovery and
//!   compares every report against it;
//! * the third re-derives each of the three documented scoped digests from
//!   the installation through the production readers, so the findings
//!   document's record stays reproducible, and fails if any committed report
//!   still carries one of them;
//! * the two `..._repair_` tests are the repair's own acceptance: the three
//!   reissued reports carry the canonical fingerprint, record their
//!   regeneration, keep their original reviewer and name the committed
//!   harness that writes them — and, with `CS_GAME_DIR`, carry exactly the
//!   fingerprint production discovery answers for this installation.

use std::path::PathBuf;

/// The installation fingerprint every committed retail report carries:
/// production `cs_assets::install::fingerprint` over the manifest.
const CANONICAL_INSTALL: &str = "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";
/// The canonical content fingerprint the same reports carry: production
/// `cs_assets::install::content_fingerprint` over the manifest.
const CANONICAL_CONTENT: &str = "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

/// The three reports #1174 reissued, with the task-scoped digest each one
/// carried before the repair. The value is the *record* of the divergence —
/// what each private writer computed — kept here so the findings document's
/// measurement stays re-derivable and so a report that starts carrying it
/// again is recognisable. No committed report may carry any of them.
const REISSUED: [(&str, &str); 3] = [
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

/// `(task id, the reviewer fact the original report recorded)` — the
/// regeneration must never rewrite who reviewed a report, so the repair's
/// own test pins each identity to the agent the original review named.
const ORIGINAL_REVIEWER: [(&str, &str); 3] = [
    ("F05-D", "bunny-1/bunny-1"),
    ("F12-J", "devin-1 (SWE-2)"),
    ("T351", "bunny-1/bunny-1"),
];

/// Where each reissued report's committed writer lives, relative to the
/// workspace root, and the harness test it declares.
const HARNESS: [(&str, &str, &str); 3] = [
    (
        "F05-D",
        "crates/cs_app/tests/campaign/evidence/f05_d.rs",
        "evidence_report_f05_d_writes_the_acceptance_report",
    ),
    (
        "F12-J",
        "crates/cs_app/tests/campaign/evidence/f12_j.rs",
        "evidence_report_f12_j_writes_the_acceptance_report",
    ),
    (
        "T351",
        "crates/cs_app/tests/campaign/evidence/t351.rs",
        "evidence_report_t351_writes_the_acceptance_report",
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
    review: String,
    method: String,
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
            let task_id = report_string_field(&text, "task_id")
                .unwrap_or_else(|| panic!("{name}: no task_id"));
            let review = report_string_field(&text, "identity")
                .unwrap_or_else(|| panic!("{name}: no review.identity"));
            let method = report_string_field(&text, "method")
                .unwrap_or_else(|| panic!("{name}: no review.method"));
            ReportSource {
                task_id,
                name,
                install_sha256: report_string_field(&text, "install_sha256"),
                content_sha256: report_string_field(&text, "content_sha256"),
                review,
                method,
            }
        })
        .collect()
}

/// Compare one report's `source` pair against a measured installation: the
/// install fingerprint must match exactly and the content fingerprint must
/// be this installation's canonical one. Returns an offender line or `None`.
///
/// There is no allowlist any more: #1174 repaired every committed report, so
/// any other content fingerprint — a scoped digest, a stale value, a value
/// from another installation — is an offender a reader cannot explain.
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
    (content != content_sha256).then(|| {
        let scoped = REISSUED
            .iter()
            .find(|(_, recorded)| content.as_str() == *recorded)
            .map(|(task_id, _)| format!(
                " — this is the task-scoped digest {task_id} carried before Rally #1174 reissued \
                 it, so the repair has been reverted"
            ))
            .unwrap_or_default();
        format!(
            "{name}: content_sha256 {content} is not this installation's {content_sha256}{scoped}",
            name = report.name
        )
    })
}

/// Every committed report whose `source` was measured over an installation
/// carries exactly the pair production discovery answers: #1174 repaired the
/// three outliers, so nothing may disagree with them any more.
#[test]
fn accept_evidence_content_digest_outliers_every_committed_report_carries_the_canonical_content_fingerprint()
 {
    let mut offenders = Vec::new();
    let mut seen = Vec::new();
    for report in report_sources() {
        if let Some(offender) = check_report(&report, CANONICAL_INSTALL, CANONICAL_CONTENT) {
            offenders.push(offender);
        }
        if REISSUED
            .iter()
            .any(|(task_id, _)| *task_id == report.task_id)
        {
            seen.push(report.task_id);
        }
    }
    assert_eq!(
        seen.len(),
        REISSUED.len(),
        "the three reissued reports are still committed: {seen:?}"
    );
    assert!(
        offenders.is_empty(),
        "reports whose fingerprints a reader cannot explain (a scoped digest is never \
         acceptable here — its own harness must reissue the report):\n  {}",
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
/// fingerprints and every report measured over an installation agrees with
/// them — the reissued three included.
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

/// The three documented scoped digests still re-derive from the installation
/// through production code — exactly as
/// `docs/findings/2026-10-10-evidence-content-digest-outliers.md` records
/// them — and **no** committed report carries any of them any more. This is
/// what proves #1157's divergence was a scoped computation that #1174
/// repaired, rather than stale data or installation drift: the same
/// installation bytes answer the canonical fingerprint and the three scoped
/// values deterministically.
#[test]
#[ignore = "requires CS_GAME_DIR: the scoped digests are re-derived over the original"]
fn accept_evidence_content_digest_outliers_each_documented_scoped_digest_still_re_derives() {
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
    assert_eq!(f05_d, REISSUED[0].1, "the F05-D scoped digest re-derives");
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
    assert_eq!(f12_j, REISSUED[1].1, "the F12-J scoped digest re-derives");
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
    assert_eq!(t351, REISSUED[2].1, "the T351 scoped digest re-derives");
    assert_ne!(t351, canonical, "a scoped digest is not the canonical one");

    // And not one committed report still carries a scoped value: they are
    // records of a computation, never a report's `content_sha256`.
    let mut carriers = Vec::new();
    for report in report_sources() {
        let Some(content) = report.content_sha256.as_ref() else {
            continue;
        };
        let Some((task_id, scoped)) = REISSUED
            .iter()
            .find(|(_, scoped)| content.as_str() == *scoped)
        else {
            continue;
        };
        carriers.push(format!(
            "{}: carries the scoped digest {scoped} that {task_id} recorded",
            report.name
        ));
    }
    assert!(
        carriers.is_empty(),
        "Rally #1174 repaired these reports; a scoped digest is back:\n  {}",
        carriers.join("\n  ")
    );
}

/// The repair itself, anywhere: the three reissued reports carry the
/// canonical content fingerprint, record the regeneration that produced them,
/// keep the reviewer the original review named and name the committed harness
/// that writes them — so the reports can never again exist only as
/// hand-written JSON.
#[test]
fn accept_evidence_content_digest_outliers_repair_the_three_reissued_reports_carry_the_canonical_fingerprint_and_name_their_committed_harness()
 {
    let sources = report_sources();
    for (task_id, harness, test) in HARNESS {
        let report = sources
            .iter()
            .find(|report| report.task_id == task_id)
            .unwrap_or_else(|| panic!("{task_id}: the reissued report is committed"));
        assert_eq!(
            report.content_sha256.as_deref(),
            Some(CANONICAL_CONTENT),
            "{task_id}: a reissued report must carry the canonical content fingerprint"
        );
        let reviewer = ORIGINAL_REVIEWER
            .iter()
            .find(|(key, _)| *key == task_id)
            .map(|(_, reviewer)| *reviewer)
            .expect("the reviewer table names this task");
        assert!(
            report.review.contains(reviewer),
            "{task_id}: the regeneration rewrote review.identity, which must stay \
             byte-unchanged with {reviewer} named in it"
        );
        assert!(
            report.method.contains("Regeneration for Rally #1174"),
            "{task_id}: review.method does not record the regeneration that reissued it"
        );

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(harness);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let task_marker = format!("\\\"task_id\\\": \\\"{task_id}\\\"");
        assert!(
            source.contains(&task_marker),
            "{}: does not spell {task_marker}, so it is not the writer of {task_id}",
            path.display()
        );
        assert!(
            source.contains(test),
            "{}: does not declare the {test} harness",
            path.display()
        );
    }
}

/// The repair, measured: the fingerprint the three reissued reports carry is
/// the one production discovery answers for this installation right now, not
/// a constant the reports and this test could agree on together.
#[test]
#[ignore = "requires CS_GAME_DIR: the fingerprint is measured over the original"]
fn accept_evidence_content_digest_outliers_repair_production_discovery_answers_the_fingerprint_the_three_reports_carry()
 {
    use cs_assets::install::{content_fingerprint, discover};

    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let measured = content_fingerprint(&found.manifest).to_hex();

    let sources = report_sources();
    for (task_id, _, _) in HARNESS {
        let report = sources
            .iter()
            .find(|report| report.task_id == task_id)
            .unwrap_or_else(|| panic!("{task_id}: the reissued report is committed"));
        assert_eq!(
            report.content_sha256.as_deref(),
            Some(measured.as_str()),
            "{task_id}: production discovery answers {measured} for this installation"
        );
    }
}
