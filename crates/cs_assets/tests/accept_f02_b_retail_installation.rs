//! F02-B retail acceptance (capability `retail`, requires CS_GAME_DIR):
//! production discovery inventories the owner's original installation —
//! every regular file, hashed from its actual bytes — and the diagnosis
//! finds the ZBD/world-group/ROF candidates through independent direct
//! reads of the same tree.
//!
//! The test fails loudly when CS_GAME_DIR is missing; it never writes
//! inside the installation. SHA-256 results are cross-checked against
//! `python3` `hashlib`, an implementation this workspace does not ship, so
//! a wrong hashing implementation cannot self-confirm.

use std::path::{Path, PathBuf};
use std::process::Command;

use cs_assets::install::{REFERENCE_WORLD_GROUP_LEADS, content_fingerprint, discover, fingerprint};

/// The read-only original installation under test.
fn game_dir() -> PathBuf {
    let value = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR must point at the read-only original installation (capability `retail`)",
    );
    assert!(!value.is_empty(), "CS_GAME_DIR must not be empty");
    PathBuf::from(value)
}

/// Independent regular-file count: a plain walk that never hashes and
/// never opens file contents. Symbolic links are not followed here either,
/// matching the production walk's semantics.
fn count_regular_files(root: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("installation directories read") {
            let entry = entry.expect("directory entries read");
            let file_type = entry.file_type().expect("entry types read");
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                count += 1;
            }
        }
    }
    count
}

/// The case-folded logical keys of every `.rof` file under `root`,
/// found by this test's own walk.
fn observed_rof_candidates(root: &Path) -> Vec<String> {
    let mut keys = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("installation directories read") {
            let entry = entry.expect("directory entries read");
            let file_type = entry.file_type().expect("entry types read");
            let name = entry.file_name().to_string_lossy().into_owned();
            let key = if prefix.is_empty() {
                name.to_lowercase()
            } else {
                format!("{prefix}/{}", name.to_lowercase())
            };
            if file_type.is_dir() {
                stack.push((entry.path(), key));
            } else if file_type.is_file() && key.ends_with(".rof") {
                keys.push(key);
            }
        }
    }
    keys.sort();
    keys
}

/// The case-folded directory names directly under the installation's ZBD
/// directory, read by this test directly. Regular files such as
/// `ZBD/planes.zbd` live there too and are not world groups.
fn observed_world_groups(root: &Path, zbd_spelling: &str) -> Vec<String> {
    let mut groups: Vec<String> = std::fs::read_dir(root.join(zbd_spelling))
        .expect("the ZBD directory reads")
        .filter_map(|entry| {
            let entry = entry.expect("directory entries read");
            entry
                .file_type()
                .expect("entry types read")
                .is_dir()
                .then(|| entry.file_name().to_string_lossy().to_ascii_lowercase())
        })
        .collect();
    groups.sort();
    groups
}

/// Cross-checks one production digest against `python3` `hashlib`.
fn cross_check_digest(path: &Path, expected: &str) {
    let output = Command::new("python3")
        .arg("-c")
        .arg(
            "import hashlib,sys; \
             print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())",
        )
        .arg(path)
        .output()
        .expect("python3 runs (the same interpreter tools/validate_evidence.py requires)");
    assert!(
        output.status.success(),
        "python3 hashlib must run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed = String::from_utf8(output.stdout)
        .expect("python3 prints UTF-8")
        .trim()
        .to_owned();
    assert_eq!(
        observed,
        expected,
        "the production digest of {} must match python3 hashlib",
        path.display()
    );
}

/// Host path of an inventoried row, re-opened under the manifest root.
fn host_path(root: &Path, spelling: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in spelling.split('/') {
        path.push(component);
    }
    path
}

#[ignore = "requires CS_GAME_DIR"]
#[test]
fn accept_f02_b_retail_installation_inventories_every_regular_file() {
    let root = game_dir();
    let found = discover(&root).expect("production discovery reads the original installation");

    // Every regular file, counted independently by this test, is a row —
    // nothing omitted, nothing invented.
    let expected_files = count_regular_files(&root);
    assert!(expected_files > 0, "the installation contains files");
    assert_eq!(
        found.manifest.files.len(),
        expected_files,
        "every regular file of the original installation inventories"
    );
    assert_eq!(found.diagnosis.file_count, expected_files);

    // Every row's size matches the filesystem's own metadata, and the row
    // re-opens under the host root through its preserved spelling.
    for file in &found.manifest.files {
        let metadata = std::fs::metadata(host_path(&root, file.relative_spelling.as_str()))
            .expect("inventoried paths re-open under the host root");
        assert_eq!(
            metadata.len(),
            file.size_bytes,
            "the inventoried size of {} matches the file",
            file.relative_spelling
        );
    }

    // Fingerprints describe the actual installation: a second full walk of
    // the same bytes yields exactly the same fingerprints.
    let again = discover(&root).expect("rediscovery of the installation succeeds");
    let install_fingerprint = fingerprint(&found.manifest);
    assert_eq!(
        install_fingerprint,
        fingerprint(&again.manifest),
        "the installation fingerprint is stable across runs"
    );
    assert_eq!(
        content_fingerprint(&found.manifest),
        content_fingerprint(&again.manifest),
        "the content fingerprint is stable across runs"
    );

    // Diagnosis cross-checked against this test's own direct reads.
    let diagnosis = &found.diagnosis;
    let zbd = diagnosis
        .zbd_dir
        .as_ref()
        .expect("the installation carries a ZBD directory");
    assert_eq!(zbd.logical_key(), "zbd");
    let planes = diagnosis
        .planes_zbd
        .as_ref()
        .expect("the installation carries ZBD/planes.zbd");
    assert_eq!(planes.logical_key(), "zbd/planes.zbd");

    let mut diagnosed_groups: Vec<String> = diagnosis
        .world_groups
        .iter()
        .map(|group| group.logical_key())
        .collect();
    diagnosed_groups.sort();
    let observed_groups: Vec<String> = observed_world_groups(&root, zbd.as_str())
        .into_iter()
        .map(|group| format!("zbd/{group}"))
        .collect();
    assert_eq!(
        diagnosed_groups, observed_groups,
        "the diagnosed world groups are exactly the directories under ZBD"
    );

    let observed_absent: Vec<String> = REFERENCE_WORLD_GROUP_LEADS
        .iter()
        .filter(|lead| {
            let expected = format!("zbd/{lead}");
            !diagnosed_groups.contains(&expected)
        })
        .map(|lead| (*lead).to_owned())
        .collect();
    assert_eq!(
        diagnosis.absent_reference_groups, observed_absent,
        "absent reference leads are derived from observed data only"
    );

    let diagnosed_rofs: Vec<String> = diagnosis
        .rof_candidates
        .iter()
        .map(|candidate| candidate.logical_key())
        .collect();
    assert_eq!(
        diagnosed_rofs,
        observed_rof_candidates(&root),
        "the diagnosed ROF candidates are exactly the `.rof` files in the tree"
    );
    assert!(
        !diagnosed_rofs.is_empty(),
        "the installation carries ROF candidates to diagnose"
    );

    // Independent digest cross-check on the smallest, middle and largest
    // inventoried file: the production SHA-256 must equal `hashlib`'s.
    let mut rows: Vec<_> = found.manifest.files.iter().collect();
    rows.sort_by_key(|row| row.size_bytes);
    let picks = [
        rows.first().expect("the installation has rows"),
        rows[rows.len() / 2],
        rows.last().expect("the installation has rows"),
    ];
    for file in picks {
        cross_check_digest(
            &host_path(&root, file.relative_spelling.as_str()),
            &file.sha256.to_hex(),
        );
    }
}
