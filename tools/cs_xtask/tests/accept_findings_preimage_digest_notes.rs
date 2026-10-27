//! Task #1158 `FINDINGS-PREIMAGE-DIGEST-NOTES`: the two findings documents
//! that still presented the pre-image installation fingerprint as current
//! provenance now carry the same dated installation note the F16-E finding
//! already carried.
//!
//! Rally #804 regenerated every committed acceptance report that still carried
//! the fingerprint measured while the owner's decrypted engine image was an
//! inventoried file of `$CS_GAME_DIR`; the owner moved the image out on
//! 2026-10-08 (#798). Two *findings documents* were left quoting the old
//! digest with nothing saying so, which made them disagree with their own
//! committed reports:
//!
//! * `docs/findings/2026-10-07-f17-d-comparison-matrix-and-material-coverage.md`
//!   (`docs/findings/evidence/F17-D.json`);
//! * `docs/findings/2026-10-08-t574-zeppelins-zrd-placed-zeppelin-carrier.md`
//!   (`docs/findings/evidence/T574.json`).
//!
//! These tests are a documentation guard: they read the committed documents and
//! reports from the workspace. Removing either note, rewriting a digest the run
//! actually recorded, or leaving any findings document that quotes the
//! pre-image value without explanatory framing fails them.
//!
//! The values are historical measurements, so the note explains them instead of
//! replacing them — the same provenance rule that made #804 regenerate reports
//! rather than hand-edit them.

use std::path::{Path, PathBuf};

/// The installation fingerprint recorded while `crimson.decrypted.exe` was
/// still an inventoried file of `$CS_GAME_DIR`, before the owner moved it out
/// (#798, 2026-10-08).
const PRE_IMAGE_INSTALL: &str = "c14a876f4457d8710dee7986333ab636122c9549cf72b646fd69cbe7e72c5352";

/// The content fingerprint recorded over that same tree.
const PRE_IMAGE_CONTENT: &str = "148a24b7b0506812e8f1ee13d8d3137a05926abebbe10161994e8c4cd300c35e";

/// What the read-only tree fingerprints to now, as #804's regenerated reports
/// record it.
const CURRENT_INSTALL: &str = "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";

/// The content fingerprint of that same tree.
const CURRENT_CONTENT: &str = "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

/// The note heading both documents carry, the shape F16-E established.
const NOTE_HEADING: &str = "**Installation note (#798, 2026-10-08).**";

/// The one place the engine image may be read from since #798.
const IMAGE_VARIABLE: &str = "$CS_ENGINE_IMAGE";

/// `(findings document, its committed acceptance report)`.
const DOCUMENTS: [(&str, &str); 2] = [
    (
        "docs/findings/2026-10-07-f17-d-comparison-matrix-and-material-coverage.md",
        "docs/findings/evidence/F17-D.json",
    ),
    (
        "docs/findings/2026-10-08-t574-zeppelins-zrd-placed-zeppelin-carrier.md",
        "docs/findings/evidence/T574.json",
    ),
];

/// A framing that explains why a document quotes the pre-image digest. Every
/// findings document that quotes it has to carry at least one of these; the two
/// this task fixed carry the dated note.
const EXPLANATIONS: [&str; 5] = [
    "#798",
    "Installation note",
    "recorded at the time",
    "What was measured on",
    "as it stood at implement time",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_workspace_file(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Every `.md` file under `docs/findings/`, including the `missions/` subtree.
fn findings_documents() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut documents = Vec::new();
    let mut pending = vec![root.join("docs/findings")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                documents.push(path);
            }
        }
    }
    documents.sort();
    documents
}

/// The value of a top-level `"key": "…"` string field, the same reader #804's
/// guard uses.
fn report_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\": \"");
    let start = text.find(&needle)? + needle.len();
    let end = start + text[start..].find('"')?;
    Some(text[start..end].to_owned())
}

/// Both documents carry the dated #798 note, and the note names the current
/// digests, the image's new home and the recorded-at-the-time framing.
#[test]
fn accept_findings_preimage_digest_notes_both_documents_carry_the_dated_798_note() {
    for (document, _) in DOCUMENTS {
        let text = read_workspace_file(document);
        for needle in [
            NOTE_HEADING,
            CURRENT_INSTALL,
            CURRENT_CONTENT,
            IMAGE_VARIABLE,
            "inventoried",
            "recorded at the time of the run",
        ] {
            assert!(
                text.contains(needle),
                "{document}: the #798 installation note must name `{needle}`; the note is \
                 what tells a reader the quoted digests predate the image's move out of \
                 $CS_GAME_DIR"
            );
        }
        assert!(
            text.contains("moved it out"),
            "{document}: the note must say the owner moved the image out of the installation"
        );
    }
}

/// The recorded values are kept: both documents still quote the digests the run
/// actually measured, and those are not the current tree's.
#[test]
fn accept_findings_preimage_digest_notes_the_recorded_values_are_kept() {
    assert_ne!(
        PRE_IMAGE_INSTALL, CURRENT_INSTALL,
        "the pre-image and current installation fingerprints are different values"
    );
    assert_ne!(
        PRE_IMAGE_CONTENT, CURRENT_CONTENT,
        "the pre-image and current content fingerprints are different values"
    );

    for (document, _) in DOCUMENTS {
        let text = read_workspace_file(document);
        assert!(
            text.contains(PRE_IMAGE_INSTALL),
            "{document}: the pre-image installation fingerprint is a measurement record and \
             must stay in the document, not be rewritten to today's value"
        );
        assert!(
            text.contains(PRE_IMAGE_CONTENT),
            "{document}: the pre-image content fingerprint is a measurement record and must \
             stay in the document"
        );
    }
}

/// Each note names the same values as the committed report for the same stage,
/// so document and report can never disagree about the installation again.
#[test]
fn accept_findings_preimage_digest_notes_the_note_agrees_with_the_committed_report() {
    for (document, report) in DOCUMENTS {
        let text = read_workspace_file(document);
        let report_text = read_workspace_file(report);

        let install = report_string_field(&report_text, "install_sha256")
            .unwrap_or_else(|| panic!("{report}: must name the installation fingerprint"));
        let content = report_string_field(&report_text, "content_sha256")
            .unwrap_or_else(|| panic!("{report}: must name the content fingerprint"));

        assert_eq!(
            install, CURRENT_INSTALL,
            "{report}: #804 regenerated this report over the tree without the image"
        );
        assert_eq!(
            content, CURRENT_CONTENT,
            "{report}: #804 regenerated this report's content fingerprint"
        );
        assert!(
            text.contains(&install),
            "{document}: the note must name the report's installation fingerprint {install}"
        );
        assert!(
            text.contains(&content),
            "{document}: the note must name the report's content fingerprint {content}"
        );
    }
}

/// No findings document quotes the pre-image fingerprint without saying why:
/// every one carries a dated #798 installation note or an explicit
/// recorded-at-the-time framing.
#[test]
fn accept_findings_preimage_digest_notes_every_document_quoting_it_is_explained() {
    let root = workspace_root();
    let mut quoting = Vec::new();
    for path in findings_documents() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        if !text.contains(PRE_IMAGE_INSTALL) {
            continue;
        }
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let explained = EXPLANATIONS
            .iter()
            .find(|explanation| text.contains(**explanation));
        assert!(
            explained.is_some(),
            "{relative}: quotes the pre-image installation fingerprint {PRE_IMAGE_INSTALL} \
             with no explanatory note; add a dated #798 installation note naming the current \
             {CURRENT_INSTALL} / {CURRENT_CONTENT} values, or say the value was recorded at \
             the time"
        );
        quoting.push(relative);
    }

    // A walk that found nothing, or a document that stopped quoting the value,
    // would make the check above vacuous.
    assert!(
        findings_documents().len() > 500,
        "the walk reaches the findings tree: {} documents",
        findings_documents().len()
    );
    for (document, _) in DOCUMENTS {
        assert!(
            quoting.iter().any(|found| found == document),
            "{document}: must still quote the pre-image fingerprint the note explains"
        );
    }
}
