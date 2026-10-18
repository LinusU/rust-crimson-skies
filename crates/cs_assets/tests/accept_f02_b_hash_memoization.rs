//! F02-B follow-up: the many readers of one installation each call
//! `discover`, so a file's digest is remembered for the process — but only
//! while its metadata is unchanged and the file has been quiet long enough
//! for the metadata to be trusted.
//!
//! These tests exercise `cs_assets::install::hash_file_reporting_memo` and
//! `discover` over newly authored fixture bytes. Removing the memo makes the
//! second-call test fail; making it trust a changed file or a fresh file
//! makes the others fail.

use std::fs::{self, File};
use std::time::{Duration, SystemTime};

use crate::common::TempTree;
use cs_assets::install::{discover, fingerprint, hash_file_reporting_memo};

const PAYLOAD: &[u8] = b"authored memoization fixture payload";

/// Backdates `spelling` so the memo may trust it.
fn age(tree: &TempTree, spelling: &str) {
    let file = File::options()
        .write(true)
        .open(tree.root().join(spelling))
        .expect("the fixture opens");
    file.set_modified(SystemTime::now() - Duration::from_secs(3600))
        .expect("the fixture is backdated");
}

#[test]
fn accept_f02_b_memo_second_hash_of_a_settled_file_is_remembered() {
    let tree = TempTree::new("memo-settled");
    tree.write("ZBD/planes.zbd", PAYLOAD);
    age(&tree, "ZBD/planes.zbd");
    let path = tree.root().join("ZBD/planes.zbd");

    let (size, first, first_remembered) = hash_file_reporting_memo(&path).unwrap();
    let (_, second, second_remembered) = hash_file_reporting_memo(&path).unwrap();

    assert_eq!(size, PAYLOAD.len() as u64);
    assert!(!first_remembered);
    assert!(
        second_remembered,
        "a settled, unchanged file is not re-hashed"
    );
    assert_eq!(first, second);
    assert_eq!(first, cs_assets::install::sha256(PAYLOAD));
}

#[test]
fn accept_f02_b_memo_never_remembers_a_freshly_written_file() {
    let tree = TempTree::new("memo-fresh");
    tree.write("ZBD/planes.zbd", PAYLOAD);
    let path = tree.root().join("ZBD/planes.zbd");

    hash_file_reporting_memo(&path).unwrap();
    let (_, _, remembered) = hash_file_reporting_memo(&path).unwrap();

    assert!(
        !remembered,
        "a file modified moments ago is always re-hashed"
    );
}

#[test]
fn accept_f02_b_memo_same_length_rewrite_with_restored_mtime_changes_the_digest() {
    let tree = TempTree::new("memo-rewrite");
    tree.write("ZBD/planes.zbd", PAYLOAD);
    age(&tree, "ZBD/planes.zbd");
    let path = tree.root().join("ZBD/planes.zbd");
    let old_modified = fs::metadata(&path).unwrap().modified().unwrap();
    let (_, before, _) = hash_file_reporting_memo(&path).unwrap();

    tree.edit_byte("ZBD/planes.zbd", 0, 0x01);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old_modified)
        .unwrap();
    let (_, after, remembered) = hash_file_reporting_memo(&path).unwrap();

    assert!(!remembered);
    assert_ne!(before, after, "the edited bytes are what is hashed");
}

#[test]
fn accept_f02_b_memo_discover_fingerprint_follows_an_edit_after_a_cached_run() {
    let tree = TempTree::new("memo-discover");
    tree.write("ZBD/planes.zbd", PAYLOAD);
    tree.write("Readme.rtf", b"authored memoization readme");
    age(&tree, "ZBD/planes.zbd");
    age(&tree, "Readme.rtf");

    let first = fingerprint(&discover(tree.root()).unwrap().manifest);
    let again = fingerprint(&discover(tree.root()).unwrap().manifest);
    tree.edit_byte("ZBD/planes.zbd", 3, 0x80);
    let edited = fingerprint(&discover(tree.root()).unwrap().manifest);

    assert_eq!(first, again);
    assert_ne!(first, edited);
}
