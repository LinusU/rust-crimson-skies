//! Acceptance scenario F01-D (AC04) for the release-inventory provenance
//! gate: a committed retail binary in the release inventory is detected —
//! by content signature, by name, or by the absence of any declared
//! provenance — and every violation is named in the report.
//!
//! These tests exercise production code only:
//! `cs_types::evidence::check_release_inventory` plus the `cs_inspect`
//! producers (`scan_inventory_dir`, `committed_inventory`) and the
//! `audit_release_inventory` wiring over `AUTHORED_CONTENT_ROOTS`. Removing
//! or neutering that implementation — dropping a signature table, letting
//! executables hide under authored roots, or returning a clean report
//! unconditionally — makes them fail.
//!
//! All binary fixtures here are newly authored bytes (a two-byte `MZ` stub
//! padded with zeros, invented blobs); nothing is copied from the original
//! installation.

use std::fs;
use std::path::Path;

use cs_inspect::evidence::{
    AUTHORED_CONTENT_ROOTS, audit_release_inventory, committed_inventory, scan_inventory_dir,
};
use cs_types::evidence::{
    INVENTORY_HEADER_LEN, InventoryEntry, InventoryMatch, ProhibitedContent,
    check_release_inventory,
};

/// An authored stand-in for a committed game executable: the `MZ` magic, a
/// PE header pointer at `0x3c` and the `PE\0\0` signature at `0x80`, zeros
/// elsewhere. Constructed, not copied.
fn synthetic_pe() -> Vec<u8> {
    let mut bytes = vec![0u8; 0x84];
    bytes[0] = b'M';
    bytes[1] = b'Z';
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes
}

/// A binary inventory entry: `text` is what the producer's sample verdict
/// would be for non-decodable content.
fn binary(path: &str, bytes: &[u8]) -> InventoryEntry {
    InventoryEntry {
        path: path.to_owned(),
        len: bytes.len() as u64,
        header: bytes[..bytes.len().min(INVENTORY_HEADER_LEN)].to_vec(),
        text: false,
    }
}

/// A text inventory entry, the shape source files take in the inventory.
fn text(path: &str, contents: &str) -> InventoryEntry {
    InventoryEntry {
        path: path.to_owned(),
        len: contents.len() as u64,
        header: contents.as_bytes()[..contents.len().min(INVENTORY_HEADER_LEN)].to_vec(),
        text: true,
    }
}

fn violation_paths(report: &cs_types::evidence::InventoryReport) -> Vec<&str> {
    report
        .violations
        .iter()
        .map(|violation| violation.path.as_str())
        .collect()
}

/// **Minimum acceptance scenario (AC04):** a committed retail binary — a
/// PE executable sitting in the shipped file set next to ordinary source
/// files — is detected and named, by content signature, not by trusting
/// its extension.
///
/// Observable failure if the check is removed or stubbed: the report comes
/// back clean and the `expect` on the violation fails.
#[test]
fn accept_f01_d_committed_retail_binary_is_detected() {
    let entries = vec![
        text("Cargo.toml", "[workspace]\nmembers = []\n"),
        text("crates/cs_types/src/lib.rs", "//! ids and units\n"),
        binary("crimson.exe", &synthetic_pe()),
    ];

    let report = audit_release_inventory(&entries);

    assert!(!report.is_clean(), "a committed executable cannot pass");
    assert_eq!(report.checked, 3);
    assert_eq!(report.violations.len(), 1);
    let violation = &report.violations[0];
    assert_eq!(violation.path, "crimson.exe");
    assert_eq!(violation.content, ProhibitedContent::ExecutableImage);
    assert_eq!(
        violation.matched,
        InventoryMatch::Signature("MZ (DOS/PE executable)")
    );
    assert!(
        report
            .diagnostic_lines()
            .iter()
            .any(|line| line.contains("crimson.exe") && line.contains("executable")),
        "the diagnostic must name the entry and why: {:?}",
        report.diagnostic_lines()
    );
}

/// Renaming does not help: the executable signature condemns the bytes
/// whatever the file is called, and other executable families (ELF,
/// Mach-O) are caught the same way. An executable extension condemns a
/// name even without binary content to match.
#[test]
fn accept_f01_d_executables_are_caught_by_bytes_or_name() {
    let entries = vec![
        binary("payload.dat", &synthetic_pe()), // renamed PE
        binary("libbackend.so", b"\x7fELF\x02\x01\x01\x00"),
        binary("helper.bin", &[0xfe, 0xed, 0xfa, 0xcf, 0, 0, 0, 0]), // Mach-O 64
        text("SETUPENU.DLL", "not a real dll but claims the name"),
        text("notes.txt", "ordinary text"),
    ];

    let report = audit_release_inventory(&entries);
    let paths = violation_paths(&report);

    assert_eq!(
        paths,
        ["payload.dat", "libbackend.so", "helper.bin", "SETUPENU.DLL"],
        "every executable form must be condemned, case-insensitive name included"
    );
    assert!(
        report
            .violations
            .iter()
            .all(|v| v.content == ProhibitedContent::ExecutableImage),
        "all four are executable images: {:?}",
        report.violations
    );
    assert_eq!(
        report.violations[3].matched,
        InventoryMatch::Extension("dll".to_owned()),
        "the upper-cased .DLL name matched by extension, lowercased"
    );
}

/// Original data and bundled media outside the authored roots: container
/// signatures (INTERP family, RIFF/WAVE, TrueType), text-compatible
/// document signatures (PDF/RTF) and retail-format names are all
/// prohibited, and a binary nothing recognizes is still condemned as
/// unidentified rather than passed.
#[test]
fn accept_f01_d_game_data_media_and_blobs_outside_authored_roots() {
    let entries = vec![
        // The real INTERP/ZBD-family signature on authored bytes.
        binary("ZBD/interp.zbd", &[0x19, 0x11, 0x97, 0x08, 7, 0, 0, 0]),
        // A ROF has no signature; the name carries it.
        binary("GOSDATA/ASSETS/crimson.rof", &[1, 0, 0, 0, 7, 0, 0, 0]),
        binary("audio/theme.wav", b"RIFF\x24\x00\x00\x00WAVEfmt "),
        binary("fonts/panel.ttf", &[0x00, 0x01, 0x00, 0x00, 0, 0]),
        text("docs/EULA.RTF", "{\\rtf1\\ansi commercial license text}"),
        text("manual.dat", "%PDF-1.4 renamed manual"),
        binary("blob.bin", &[0xde, 0xad, 0xbe, 0xef, 0x00]),
        text("docs/design.md", "# authored documentation"),
    ];

    let report = audit_release_inventory(&entries);
    let kinds: Vec<(&str, ProhibitedContent)> = report
        .violations
        .iter()
        .map(|v| (v.path.as_str(), v.content))
        .collect();

    assert_eq!(
        kinds,
        [
            ("ZBD/interp.zbd", ProhibitedContent::GameData),
            ("GOSDATA/ASSETS/crimson.rof", ProhibitedContent::GameData),
            ("audio/theme.wav", ProhibitedContent::MediaOrDocument),
            ("fonts/panel.ttf", ProhibitedContent::MediaOrDocument),
            ("docs/EULA.RTF", ProhibitedContent::MediaOrDocument),
            ("manual.dat", ProhibitedContent::MediaOrDocument),
            ("blob.bin", ProhibitedContent::UnidentifiedBinary),
        ],
        "each entry must be condemned under the right prohibition"
    );
    // The informative match: signature beats extension when both apply.
    assert_eq!(
        report.violations[0].matched,
        InventoryMatch::Signature("INTERP/ZBD-family signature 0x08971119")
    );
    assert_eq!(
        report.violations[6].matched,
        InventoryMatch::NonText,
        "an unrecognized binary is condemned by what it is, not by a name"
    );
}

/// The authored roots are the one place binary lookalikes are legitimate —
/// authored fixtures are the reason they exist — but they never excuse an
/// executable. The exemption matches case-insensitively, the way a
/// case-insensitive filesystem resolves the directory.
#[test]
fn accept_f01_d_authored_roots_exempt_data_never_executables() {
    let entries = vec![
        // The real committed fixture set: authored binaries with
        // retail-format signatures and names.
        binary(
            "fixtures/synthetic/synthetic.interp",
            &[0x19, 0x11, 0x97, 0x08, 7, 0, 0, 0],
        ),
        binary("fixtures/synthetic/flat-uncompressed.rof", &[2, 0, 0, 0]),
        binary("fixtures/synthetic/rectangular.bm", &[3, 0, 2, 0]),
        binary("Fixtures/SYNTHETIC/cased.dat", &[0xde, 0xad, 0x00]),
        // An executable is prohibited anywhere — the exemption is for
        // authored data, not for hiding binaries.
        binary("fixtures/synthetic/smuggled.exe", &synthetic_pe()),
    ];

    let report = check_release_inventory(&entries, &["fixtures/synthetic"]);

    assert_eq!(
        violation_paths(&report),
        ["fixtures/synthetic/smuggled.exe"],
        "only the executable is condemned; the authored lookalikes stand"
    );
    assert_eq!(
        report.violations[0].content,
        ProhibitedContent::ExecutableImage
    );
    assert_eq!(report.checked, 5);
}

/// An inventory of plain text — sources, manifests, documentation — is
/// clean, and so is an empty inventory; `checked` reports what was seen.
#[test]
fn accept_f01_d_text_only_and_empty_inventories_are_clean() {
    let entries = vec![
        text("Cargo.toml", "[workspace]\n"),
        text("crates/cs_types/src/evidence.rs", "//! records\n"),
        text("docs/findings/note.md", "# finding\n"),
        text(
            "tools/make_synthetic_fixtures.py",
            "#!/usr/bin/env python3\n",
        ),
        text(".gitignore", "/target/\n"),
        text("fixtures/synthetic/README.md", "# authored fixtures\n"),
    ];

    let report = audit_release_inventory(&entries);
    assert!(
        report.is_clean(),
        "text sources have nothing to answer for: {:?}",
        report.diagnostic_lines()
    );
    assert_eq!(report.checked, entries.len());

    let empty = audit_release_inventory(&[]);
    assert!(empty.is_clean() && empty.checked == 0);
}

/// The directory producer reads real bytes: a fixture tree under `target/`
/// mixing an authored-text file, a smuggled executable and an authored-root
/// binary scans into an inventory whose only violation is the executable —
/// and the recorded `len`/`path` are the real file's.
#[test]
fn accept_f01_d_scan_inventory_dir_reads_real_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/f01-d-inventory-fixtures/scanned");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("fixtures/synthetic"))
        .expect("the fixture tree must be creatable");
    fs::write(root.join("README.md"), "# authored\n").expect("writable");
    fs::write(root.join("payload.dat"), synthetic_pe()).expect("writable");
    fs::write(
        root.join("fixtures/synthetic/flat-uncompressed.rof"),
        [2u8, 0, 0, 0, 4, 0, 0, 0],
    )
    .expect("writable");

    let entries = scan_inventory_dir(&root).expect("the fixture tree must scan");
    assert_eq!(entries.len(), 3, "every file enters the inventory");

    let report = audit_release_inventory(&entries);
    assert_eq!(violation_paths(&report), ["payload.dat"]);
    let scanned = entries
        .iter()
        .find(|entry| entry.path == "payload.dat")
        .expect("the executable was scanned");
    assert_eq!(scanned.len, 0x84, "the recorded size is the real file's");
    assert!(!scanned.text, "MZ bytes are not text");
    assert!(
        entries
            .iter()
            .find(|entry| entry.path == "README.md")
            .is_some_and(|entry| entry.text),
        "markdown scans as text"
    );
}

/// The committed inventory of this very checkout — `git ls-files`, real
/// bytes — is clean: every binary in the tree lives under the declared
/// authored root. This is the integration evidence for AC04 on the actual
/// first vertical slice.
#[test]
fn accept_f01_d_committed_tree_of_this_repo_is_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let entries = committed_inventory(&root).expect("this checkout must inventory");

    assert_eq!(
        AUTHORED_CONTENT_ROOTS,
        &["fixtures/synthetic"],
        "the audit must apply the declared authored root"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "fixtures/synthetic/flat-uncompressed.rof"),
        "the scan must really cover the authored fixtures"
    );
    assert!(entries.iter().any(|entry| entry.path == "Cargo.toml"));

    let report = audit_release_inventory(&entries);
    assert!(
        report.is_clean(),
        "the committed tree must hold no prohibited content: {:?}",
        report.diagnostic_lines()
    );
}

/// A mispointed inventory is an error, never a clean report: `git
/// ls-files` under a subdirectory of a checkout exits 0 while reporting
/// only the tracked prefix (here: nothing, `target/` is ignored), so the
/// producer refuses a root that is not the work-tree root rather than
/// emitting a vacuously clean inventory.
#[test]
fn accept_f01_d_subdirectory_of_a_checkout_fails_loudly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/f01-d-inventory-fixtures/not-a-checkout-root");
    fs::create_dir_all(&root).expect("the fixture dir must be creatable");

    let error = committed_inventory(&root)
        .expect_err("a subdirectory must not produce a committed inventory");
    assert!(
        error.to_string().contains("work-tree root"),
        "the error must say why the root was refused, got: {error}"
    );
}

/// Positive control on the owner's read-only installation (`retail`
/// capability): the same check that passes the committed tree must flag
/// the real install — executables by signature, containers by name and
/// signature, media and documents by both. Run with `--include-ignored`;
/// without `CS_GAME_DIR` it fails loudly rather than passing vacuously.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f01_d_retail_installation_is_flagged() {
    let install = std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR is required for the retail control and is unset");
    let entries = scan_inventory_dir(Path::new(&install))
        .expect("the installation must be scannable read-only");
    assert!(
        entries.len() > 100,
        "the control must cover the real tree, got {} entries",
        entries.len()
    );

    let report = audit_release_inventory(&entries);
    let kinds = |content: ProhibitedContent| {
        report
            .violations
            .iter()
            .filter(|v| v.content == content)
            .count()
    };
    eprintln!(
        "retail control: {} entries, {} violations ({} executable, {} game data, {} media/doc, {} unidentified)",
        report.checked,
        report.violations.len(),
        kinds(ProhibitedContent::ExecutableImage),
        kinds(ProhibitedContent::GameData),
        kinds(ProhibitedContent::MediaOrDocument),
        kinds(ProhibitedContent::UnidentifiedBinary),
    );

    assert!(
        report
            .violations
            .iter()
            .any(|v| v.content == ProhibitedContent::ExecutableImage
                && v.path.to_lowercase().ends_with("crimson.exe")),
        "the game executable itself must be flagged: {:?}",
        report.diagnostic_lines()
    );
    assert!(
        kinds(ProhibitedContent::GameData) > 0,
        "original data containers must be flagged"
    );
    assert!(
        kinds(ProhibitedContent::MediaOrDocument) > 0,
        "bundled media/documents must be flagged"
    );
}

/// Mutation check, stated: replacing `check_release_inventory`'s body with
/// an always-clean report fails every test above that expects a violation,
/// and dropping the committed-tree producer fails the real-repo test.
#[test]
fn accept_f01_d_report_names_what_it_condemned() {
    let entries = vec![
        binary("a.exe", &synthetic_pe()),
        binary("b.zbd", &[0, 0, 0, 0]),
    ];
    let report = audit_release_inventory(&entries);
    let lines = report.diagnostic_lines();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("a.exe") && lines[0].contains("MZ"));
    assert!(lines[1].contains("b.zbd") && lines[1].contains(".zbd"));
}
