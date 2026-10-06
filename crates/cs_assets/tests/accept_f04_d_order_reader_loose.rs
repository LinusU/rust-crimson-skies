//! The original engine's reader **loose** rules: the loose-directory fallback
//! and the loose-file override (task #700,
//! `docs/findings/2026-10-05-f04-d-original-lookup-order.md` section A,
//! followed up by #685 in
//! `docs/findings/2026-10-06-f04-d-reader-member-lookup.md` section E).
//!
//! The original's reader open at `0x579c60` has three rules beyond the
//! basename reduction: the archive pass in mount order
//! `[root, mission, world]`, then
//!
//! 1. a loose file of the same basename **overrides** the archive copy when it
//!    is newer (`CompareFileTime >= 1`);
//! 2. only when *no* archive holds the name does the loose pass
//!    (`0x579710` → `0x59d170`) search the **most recently added** directory
//!    first, then `zbd`.
//!
//! # What these tests hold the engine to
//!
//! Rule 2 is **modelled**: a [`ReaderMounts`] set registers the original's
//! loose directories in the order the executable appends them
//! ([`original_loose_reader_directories`]) and searches them backwards, so a
//! name no archive holds is served by the first regular file of the most
//! recently added directory, and the whole search is in the trace.
//!
//! Rule 1 is **deliberately not decided**. Deciding it needs a time on the
//! archive side, and the only candidate the bytes offer — the index entry's
//! trailing `u64` — is unknown (task #692). So a lookup that finds both an
//! archive member and a loose file of the same basename is **refused**
//! ([`ReaderLookupError::LooseOverrideUndecided`]), never answered with either
//! copy. Two tests differ only in the loose file's host modification time — one
//! far newer, one far older — and both must refuse: the host clock is not the
//! original's comparison argument, and reading it would be inventing a rule.
//!
//! The synthetic tests author loose files, missing directories, a non-regular
//! entry and a symbolic link under a temporary tree; the `retail_` tests read
//! `$CS_GAME_DIR` read-only and pin the fact that makes both rules unreachable
//! from this installation: **none of the original's loose reader directories
//! holds a reader member name** (the `data/...` ones do not exist at all, and
//! `zbd` holds only the archives). CI skips the retail tests.

mod common;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::TempTree;
use cs_assets::install::{self, Discovery, sha256};
use cs_assets::vfs::{
    MountBuilder, READER_LOOKUP_ORDER_STATUS, READER_LOOSE_ORDER_STATUS,
    READER_LOOSE_OVERRIDE_STATUS, READER_NAMESPACE, READER_ROOT_DIRECTORY, ReaderArchive,
    ReaderAttemptOutcome, ReaderLevel, ReaderLookupError, ReaderLooseAttempt, ReaderLooseDirectory,
    ReaderLooseError, ReaderLooseOutcome, ReaderMounts, ReaderOrigin, mount_reader_archive,
    original_loose_reader_directories,
};
use cs_formats::zbd::{INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES};
use cs_types::asset_id::{
    AssetKey, MissionScope, MountId, MountNamespace, PRECEDENCE_ORDER_STATUS, PrecedenceClass,
    ResolveContext, WorldGroup,
};
use cs_types::evidence::ClaimStatus;

/// The acceptance prefix of this task.
const PREFIX: &str = "accept_f04_d_order_reader_loose_";

// -------------------------------------------------------------- fixtures ---

/// The `u32` every retail reader index entry carries at the head of its 76
/// unexplained bytes; production reads nothing from it.
const ENTRY_WORD: u32 = 2;

/// A trailing `u64` every retail entry carries and never zero.
const ENTRY_STAMP: u64 = 1_000_000_000;

/// One authored index entry of a reader archive.
struct Entry {
    start: u32,
    length: u32,
    name: Vec<u8>,
}

/// `data`, then `entries.len()` entries of 148 bytes, then the version-one
/// trailer — the layout [`cs_formats::zbd::read_version_one_index`] reads.
fn archive(data: &[u8], entries: &[Entry]) -> Vec<u8> {
    let mut bytes = data.to_vec();
    for entry in entries {
        assert!(entry.name.len() < INDEX_NAME_BYTES, "a fixture name fits");
        let mut name = [0u8; INDEX_NAME_BYTES];
        name[..entry.name.len()].copy_from_slice(&entry.name);
        bytes.extend_from_slice(&entry.start.to_le_bytes());
        bytes.extend_from_slice(&entry.length.to_le_bytes());
        bytes.extend_from_slice(&name);

        // The 76 unexplained bytes hold the retail-observed `u32 word`, a
        // byte-identical NUL-padded copy of the name and a never-zero `u64`.
        let mut tail = [0u8; INDEX_UNEXPLAINED_BYTES];
        tail[..4].copy_from_slice(&ENTRY_WORD.to_le_bytes());
        tail[4..4 + name.len()].copy_from_slice(&name);
        tail[INDEX_UNEXPLAINED_BYTES - 8..].copy_from_slice(&ENTRY_STAMP.to_le_bytes());
        bytes.extend_from_slice(&tail);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    assert_eq!(
        bytes.len(),
        data.len() + entries.len() * INDEX_ENTRY_BYTES as usize + 8,
        "the fixture archive is exactly data + entries + trailer"
    );
    bytes
}

/// An archive holding `members` back to back, each indexed under its own name.
fn reader_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (name, bytes) in members {
        let name = name.as_bytes();
        entries.push(Entry {
            start: u32::try_from(data.len()).expect("a fixture member is short"),
            length: u32::try_from(bytes.len()).expect("a fixture member is short"),
            name: name.to_vec(),
        });
        data.extend_from_slice(bytes);
    }
    archive(&data, &entries)
}

/// The builder a reader mount is declared with.
fn builder(id: &str, container: &str) -> MountBuilder {
    MountBuilder::new(
        MountId::new(id).expect("a valid mount id"),
        MountNamespace::new(READER_NAMESPACE).expect("a valid namespace"),
        PrecedenceClass::Shared,
        container,
    )
}

/// A reader key naming `name`.
fn key(name: &str) -> AssetKey {
    AssetKey::from_spelling(READER_NAMESPACE, name, "default").expect("a valid reader key")
}

/// The fixture world group and installation fingerprint.
fn fixture_world() -> WorldGroup {
    WorldGroup::new("zbd/c1c").expect("a valid world group")
}

fn fixture_context(mission: &str) -> ResolveContext {
    ResolveContext::new(sha256(b"700 synthetic installation"))
        .with_world_group(fixture_world())
        .with_mission(MissionScope::new(mission).expect("a valid mission scope"))
}

/// Writes `bytes` under `container` in `tree` and mounts it at `level`.
fn mount(tree: &TempTree, container: &str, level: ReaderLevel, bytes: &[u8]) -> ReaderArchive {
    tree.write(container, bytes);
    let path = tree.root().join(container);
    mount_reader_archive(
        builder(&format!("reader-{level}"), container).with_world_group(fixture_world()),
        &path,
        level,
    )
    .unwrap_or_else(|error| {
        panic!(
            "{} mounts as a {level} reader archive: {error}",
            path.display()
        )
    })
}

/// Registers one loose reader directory below `tree`, in the order the original
/// appends it.
fn loose(tree: &TempTree, spelling: &str) -> ReaderLooseDirectory {
    ReaderLooseDirectory::declare(tree.root(), spelling)
        .unwrap_or_else(|error| panic!("{spelling} declares a loose directory: {error}"))
}

/// `(directory, outcome label)` of a lookup's loose pass, in search order.
fn loose_trace(resolution_trace_loose: &[ReaderLooseAttempt]) -> Vec<(&str, String)> {
    resolution_trace_loose
        .iter()
        .map(|attempt| {
            (
                attempt.directory.as_str(),
                attempt.outcome.label().to_owned(),
            )
        })
        .collect()
}

/// The label a reader trace reports for a loose outcome, for readable asserts.
fn label(outcome: &ReaderLooseOutcome) -> &'static str {
    outcome.label()
}

// ------------------------------------------------------------- the fallback ---

#[test]
fn accept_f04_d_order_reader_loose_the_fallback_serves_a_name_no_archive_holds() {
    let tree = TempTree::new("reader-loose-fallback");
    let mut mounts = ReaderMounts::new();
    // No archive holds `targets.zrd`: the root archive holds another member, so
    // the archive pass misses and the original's loose pass runs.
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        &reader_archive(&[("soils.zrd", b"root soils")]),
    ));
    // Registered in the original's append order: `zbd` first, `zbd` last in the
    // search. `data/c1c/mp1` never exists on this host.
    mounts.add_loose_directory(loose(&tree, READER_ROOT_DIRECTORY));
    mounts.add_loose_directory(loose(&tree, "data/common/zrdr"));
    mounts.add_loose_directory(loose(&tree, "data/common"));
    tree.write("data/common/targets.zrd", b"loose targets");

    let context = fixture_context("mp1");
    let resolution = mounts
        .resolve(&context, &key("targets.zrd"))
        .expect("no archive holds the name, so the loose pass serves it");

    let ReaderOrigin::Loose {
        directory,
        file,
        host_path,
        size_bytes,
        sha256: digest,
    } = &resolution.origin
    else {
        panic!(
            "the loose pass served the lookup, so the origin is loose: {:?}",
            resolution.origin
        )
    };
    assert_eq!(
        directory, "data/common",
        "the directory the file was found in"
    );
    assert_eq!(file, "data/common/targets.zrd");
    assert_eq!(
        host_path,
        &tree.root().join("data/common/targets.zrd"),
        "the host path is the declared directory plus the basename"
    );
    assert_eq!(*size_bytes, 13);
    assert_eq!(*digest, sha256(b"loose targets"));
    assert!(resolution.origin.is_loose() && !resolution.origin.is_archive());
    assert_eq!(
        resolution.level(),
        None,
        "a loose file is not one of the three archive levels"
    );
    assert_eq!(resolution.mount(), None, "a loose file belongs to no mount");
    assert_eq!(resolution.member, "targets.zrd");

    // The span names the file itself: it is the container, not a member inside
    // one, so there is no member key and the extent is the whole file.
    assert_eq!(resolution.container, "data/common/targets.zrd");
    assert_eq!(resolution.span.container_path(), "data/common/targets.zrd");
    assert_eq!(resolution.span.member_key(), None);
    assert_eq!(resolution.span.offset(), 0);
    assert_eq!(resolution.span.length(), 13);
    assert_eq!(
        resolution.span.member_sha256(),
        Some(sha256(b"loose targets"))
    );
    assert_eq!(resolution.span.install_sha256(), context.installation);
    assert_eq!(
        mounts.read(&resolution).expect("the loose file reads"),
        b"loose targets",
        "the bytes read back are the bytes the span names"
    );

    // The whole search is in the trace, most recently added first and `zbd`
    // last, with the archive attempts above it.
    assert_eq!(
        loose_trace(&resolution.trace.loose),
        vec![
            ("data/common", "selected".to_owned()),
            ("data/common/zrdr", "absent".to_owned()),
            (READER_ROOT_DIRECTORY, "miss".to_owned()),
        ],
        "the search order is the reverse of the append order, so `zbd` is last"
    );
    assert_eq!(
        resolution.trace.loose[0].outcome,
        ReaderLooseOutcome::Selected {
            size_bytes: 13,
            sha256: sha256(b"loose targets"),
        }
    );
    assert_eq!(
        resolution.trace.attempts[0].outcome,
        ReaderAttemptOutcome::Miss,
        "the root archive was searched first and holds no such member"
    );
    assert_eq!(resolution.trace.order_status, READER_LOOKUP_ORDER_STATUS);
    assert_eq!(
        resolution.trace.loose_order_status,
        READER_LOOSE_ORDER_STATUS
    );
}

#[test]
fn accept_f04_d_order_reader_loose_the_fallback_serves_the_most_recently_added_directory() {
    let tree = TempTree::new("reader-loose-most-recent");
    let mut mounts = ReaderMounts::new();
    // Three directories, each holding the same basename with different bytes, in
    // the original's append order. The search runs the other way round.
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    tree.write("zbd/targets.zrd", b"oldest addition");
    tree.write("data/common/zrdr/targets.zrd", b"startup loose path");
    tree.write("data/common/targets.zrd", b"world load");
    tree.write("data/c1c/targets.zrd", b"world");
    tree.write("data/c1c/nets/targets.zrd", b"nets");
    tree.write("data/c1c/mp1/targets.zrd", b"mission");

    assert_eq!(
        mounts
            .loose_directories()
            .iter()
            .map(ReaderLooseDirectory::spelling)
            .collect::<Vec<_>>(),
        vec![
            "zbd".to_owned(),
            "data/common/zrdr".to_owned(),
            "data/common".to_owned(),
            "data/c1c".to_owned(),
            "data/c1c/nets".to_owned(),
            "data/c1c/mp1".to_owned(),
        ],
        "the registered order is the original's append order"
    );
    assert_eq!(
        mounts
            .loose_search_order()
            .map(ReaderLooseDirectory::spelling)
            .collect::<Vec<_>>(),
        vec![
            "data/c1c/mp1",
            "data/c1c/nets",
            "data/c1c",
            "data/common",
            "data/common/zrdr",
            "zbd",
        ],
        "the search order is the most recently added first and `zbd` last"
    );

    let resolution = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the mission loose directory holds the name");
    assert!(
        matches!(
            &resolution.origin,
            ReaderOrigin::Loose { directory, .. } if directory == "data/c1c/mp1"
        ),
        "the most recently added directory serves the lookup: {:?}",
        resolution.origin
    );
    assert_eq!(mounts.read(&resolution).expect("reads"), b"mission");
    assert_eq!(
        resolution
            .trace
            .loose
            .iter()
            .map(|attempt| (attempt.directory.as_str(), label(&attempt.outcome)))
            .collect::<Vec<_>>(),
        vec![
            ("data/c1c/mp1", "selected"),
            ("data/c1c/nets", "shadowed"),
            ("data/c1c", "shadowed"),
            ("data/common", "shadowed"),
            ("data/common/zrdr", "shadowed"),
            ("zbd", "shadowed"),
        ],
        "every earlier-and-later copy is reported, and only the most recently \
         added one served"
    );
}

#[test]
fn accept_f04_d_order_reader_loose_the_fallback_is_skipped_when_an_archive_holds_the_name() {
    let tree = TempTree::new("reader-loose-fallback-skipped");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    // The loose directories exist and hold a file, but not of the archive's
    // name.
    tree.write("data/common/soils.zrd", b"loose soils");
    tree.write("data/c1c/mp1/soils.zrd", b"mission loose soils");

    let resolution = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the root archive holds the name");
    assert!(
        resolution.origin.is_archive(),
        "an archive copy is served: the loose pass only runs when no archive holds the name"
    );
    assert_eq!(mounts.read(&resolution).expect("reads"), b"root targets");
    assert_eq!(
        resolution
            .trace
            .loose
            .iter()
            .map(|attempt| (attempt.directory.as_str(), label(&attempt.outcome)))
            .collect::<Vec<_>>(),
        vec![
            ("data/c1c/mp1", "miss"),
            ("data/c1c/nets", "absent"),
            ("data/c1c", "miss"),
            ("data/common", "miss"),
            ("data/common/zrdr", "absent"),
            ("zbd", "miss"),
        ],
        "no loose directory holds the archive's name, so no override arises"
    );

    // The same loose set does serve the name no archive holds: the fallback is
    // decided per name, not per mount set.
    let fallback = mounts
        .resolve(&fixture_context("mp1"), &key("soils.zrd"))
        .expect("no archive holds soils.zrd, so the loose pass serves it");
    assert!(
        matches!(
            &fallback.origin,
            ReaderOrigin::Loose { directory, .. } if directory == "data/c1c/mp1"
        ),
        "the most recently added loose directory serves it: {:?}",
        fallback.origin
    );
    assert_eq!(
        mounts.read(&fallback).expect("reads"),
        b"mission loose soils"
    );
}

// ------------------------------------------------------- the undecidable rule ---

#[test]
fn accept_f04_d_order_reader_loose_an_archive_copy_and_a_loose_file_refuse_the_lookup() {
    let tree = TempTree::new("reader-loose-override-refused");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    tree.write("data/c1c/mp1/targets.zrd", b"loose targets");

    let error = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect_err("both an archive member and a loose file hold the name");
    let ReaderLookupError::LooseOverrideUndecided {
        basename,
        directory,
        host_path,
        size_bytes,
        archive_container,
        trace,
        ..
    } = &error
    else {
        panic!("the override is undecidable, so the lookup is refused: {error:?}");
    };
    assert_eq!(basename, "targets.zrd");
    assert_eq!(directory, "data/c1c/mp1");
    assert_eq!(
        host_path.to_path_buf(),
        tree.root().join("data/c1c/mp1/targets.zrd")
    );
    assert_eq!(*size_bytes, 13);
    assert_eq!(archive_container, "zbd/zrdr.zbd");
    assert_eq!(
        loose_trace(&trace.loose),
        vec![
            ("data/c1c/mp1", "undecidable_shadow".to_owned()),
            ("data/c1c/nets", "absent".to_owned()),
            ("data/c1c", "miss".to_owned()),
            ("data/common", "absent".to_owned()),
            ("data/common/zrdr", "absent".to_owned()),
            ("zbd", "miss".to_owned()),
        ],
        "the loose candidate is reported as an undecidable shadow, not a winner"
    );
    assert_eq!(
        trace.attempts[0].outcome,
        ReaderAttemptOutcome::Selected { entry_index: 0 },
        "the archive copy is what the archive pass found; refusing is the point"
    );
    assert!(
        error.to_string().contains("CompareFileTime"),
        "the refusal names the comparison it cannot make: {error}"
    );
    assert_eq!(READER_LOOSE_OVERRIDE_STATUS, ClaimStatus::Unknown);
}

#[test]
fn accept_f04_d_order_reader_loose_the_override_is_refused_whatever_the_loose_files_time() {
    // The original decides the override with `CompareFileTime >= 1` against the
    // index entry's trailing `u64`, whose meaning is unknown (#692). Production
    // knows neither side, so it refuses — and it must refuse identically whether
    // the loose file looks newer or older, which is what this test pins: a
    // lookup that read the host clock would answer one of these two cases and
    // refuse the other.
    let mut refusals = Vec::new();
    for (label, modified) in [
        ("newer", SystemTime::now()),
        ("older", UNIX_EPOCH + Duration::from_secs(315_532_800)),
    ] {
        let tree = TempTree::new(&format!("reader-loose-override-{label}"));
        let mut mounts = ReaderMounts::new();
        mounts.push(mount(
            &tree,
            "zbd/zrdr.zbd",
            ReaderLevel::Root,
            &reader_archive(&[("targets.zrd", b"root targets")]),
        ));
        mounts
            .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
            .expect("the original's loose directories declare");
        tree.write("data/c1c/mp1/targets.zrd", b"loose targets");
        let path = tree.root().join("data/c1c/mp1/targets.zrd");
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("the fixture file opens")
            .set_modified(modified)
            .unwrap_or_else(|error| panic!("the {label} modification time is set: {error}"));

        let error = mounts
            .resolve(&fixture_context("mp1"), &key("targets.zrd"))
            .expect_err("a loose file of the same basename makes the lookup undecidable");
        let ReaderLookupError::LooseOverrideUndecided {
            basename,
            directory,
            size_bytes,
            archive_container,
            ..
        } = &error
        else {
            panic!("{label}: the override is undecidable, so the lookup is refused");
        };
        refusals.push(format!(
            "{basename}|{directory}|{size_bytes}|{archive_container}"
        ));
        assert!(
            mounts
                .archives()
                .first()
                .expect("the root archive is mounted")
                .member("targets.zrd")
                .is_some(),
            "{label}: the archive copy exists, which is why the lookup is refused"
        );
    }
    assert_eq!(
        refusals[0], refusals[1],
        "a far newer and a far older loose file are refused identically: no host \
         time is compared"
    );
}

// --------------------------------------------------------- orders and status ---

#[test]
fn accept_f04_d_order_reader_loose_the_orders_are_reported_with_their_own_status() {
    assert_eq!(
        READER_LOOSE_ORDER_STATUS,
        ClaimStatus::Inferred,
        "the loose search order is read off the original executable, not a runtime capture"
    );
    assert_ne!(
        READER_LOOSE_ORDER_STATUS,
        ClaimStatus::VerifiedOriginal,
        "code-derived evidence is never verified_original"
    );
    assert_eq!(
        READER_LOOSE_OVERRIDE_STATUS,
        ClaimStatus::Unknown,
        "the override rule exists but its decision is not implemented"
    );
    assert_eq!(READER_LOOKUP_ORDER_STATUS, ClaimStatus::Inferred);
    assert_eq!(
        PRECEDENCE_ORDER_STATUS,
        ClaimStatus::Designed,
        "the designed precedence order is untouched by a reader lookup"
    );
    assert_ne!(READER_LOOSE_ORDER_STATUS, PRECEDENCE_ORDER_STATUS);

    // A trace reports both orders and neither is the designed one.
    let tree = TempTree::new("reader-loose-status");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        &reader_archive(&[("soils.zrd", b"root soils")]),
    ));
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    let error = mounts
        .resolve(&fixture_context("mp1"), &key("nothing.zrd"))
        .expect_err("nothing holds the name");
    let ReaderLookupError::NotFound { trace, .. } = error else {
        panic!("nothing holds the name");
    };
    assert_eq!(trace.order_status, READER_LOOKUP_ORDER_STATUS);
    assert_eq!(trace.loose_order_status, READER_LOOSE_ORDER_STATUS);
    assert_eq!(
        trace.loose.len(),
        6,
        "every registered directory is reported, absent ones included"
    );
}

#[test]
fn accept_f04_d_order_reader_loose_the_original_directory_list_is_in_its_append_order() {
    assert_eq!(
        original_loose_reader_directories(None, None)
            .expect("the startup list declares")
            .iter()
            .map(|spelling| spelling.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec!["zbd", "data/common/zrdr"],
        "startup adds `zbd` and the loose reader path `..\\data\\common\\zrdr`"
    );
    assert_eq!(
        original_loose_reader_directories(Some("c1c"), None)
            .expect("the world list declares")
            .iter()
            .map(|spelling| spelling.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec![
            "zbd",
            "data/common/zrdr",
            "data/common",
            "data/c1c",
            "data/c1c/nets",
        ],
        "a world load adds `common`, `<w>` and `<w>\\nets` below `..\\data`"
    );
    assert_eq!(
        original_loose_reader_directories(Some("c1c"), Some("mp1"))
            .expect("the mission list declares")
            .iter()
            .map(|spelling| spelling.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec![
            "zbd",
            "data/common/zrdr",
            "data/common",
            "data/c1c",
            "data/c1c/nets",
            "data/c1c/mp1",
        ],
        "a mission load adds `<w>\\<m>` last, so it is searched first"
    );
    assert_eq!(READER_ROOT_DIRECTORY, "zbd");

    // A world-group spelling is refused rather than joined into a path nobody
    // measured.
    assert_eq!(
        original_loose_reader_directories(Some("zbd/c1c"), Some("mp1")),
        Err(ReaderLooseError::NotADirectoryName {
            spelling: "zbd/c1c".to_owned()
        }),
        "the loose path names a world directory, not a world group"
    );
    assert_eq!(
        ReaderLooseDirectory::declare(Path::new("/tmp"), "../escape"),
        Err(ReaderLooseError::Spelling(
            cs_types::install::RelativePathError::ParentComponent
        )),
        "an escaping spelling cannot name a loose directory"
    );
    assert!(
        original_loose_reader_directories(Some("c1c"), Some("mp1"))
            .expect("the mission list declares")
            .iter()
            .all(|spelling| spelling.as_str().starts_with("zbd")
                || spelling.as_str().starts_with("data/"))
    );
}

// ------------------------------------------------------------- refusals ---

#[test]
fn accept_f04_d_order_reader_loose_a_name_no_directory_holds_is_not_found() {
    let tree = TempTree::new("reader-loose-not-found");
    let mut mounts = ReaderMounts::new();
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    tree.write("data/common/other.zrd", b"another name");

    let error = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect_err("no directory holds targets.zrd");
    let ReaderLookupError::NotFound {
        basename, trace, ..
    } = &error
    else {
        panic!("nothing holds the name: {error}");
    };
    assert_eq!(basename, "targets.zrd");
    assert_eq!(
        loose_trace(&trace.loose),
        vec![
            ("data/c1c/mp1", "absent".to_owned()),
            ("data/c1c/nets", "absent".to_owned()),
            ("data/c1c", "absent".to_owned()),
            ("data/common", "miss".to_owned()),
            ("data/common/zrdr", "absent".to_owned()),
            ("zbd", "absent".to_owned()),
        ],
        "the refusal lists every directory it searched and what each held"
    );
    assert!(
        trace.attempts.is_empty(),
        "no archive is mounted in this set"
    );
    assert!(
        error.to_string().contains("data/common"),
        "the refusal names the directories: {error}"
    );
}

#[test]
fn accept_f04_d_order_reader_loose_a_non_regular_entry_is_reported_and_never_served() {
    let tree = TempTree::new("reader-loose-not-regular");
    let mut mounts = ReaderMounts::new();
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    // A directory named like the member: not a regular file.
    fs::create_dir_all(tree.root().join("data/common/targets.zrd"))
        .expect("the fixture directory is created");

    let error = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect_err("a directory is not a reader file");
    let ReaderLookupError::NotFound { trace, .. } = &error else {
        panic!("a non-regular entry is not served: {error}");
    };
    assert_eq!(
        loose_trace(&trace.loose)
            .into_iter()
            .filter(|(_, outcome)| outcome == "not_regular")
            .collect::<Vec<_>>(),
        vec![("data/common", "not_regular".to_owned())],
        "the entry is reported as not regular rather than served or ignored"
    );

    // The same guard applies to the shadow case: a non-regular entry is not a
    // loose candidate, so it cannot make the archive answer undecidable. That is
    // this engine's guard (links are never followed), not a rule measured in the
    // original.
    let mut with_archive = ReaderMounts::new();
    with_archive.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    with_archive
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    let resolution = with_archive
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("a directory cannot shadow an archive member");
    assert!(resolution.origin.is_archive());
    assert_eq!(
        loose_trace(&resolution.trace.loose)
            .into_iter()
            .filter(|(_, outcome)| outcome == "not_regular")
            .collect::<Vec<_>>(),
        vec![("data/common", "not_regular".to_owned())],
        "the non-regular entry is reported, and the archive still serves"
    );
}

#[cfg(unix)]
#[test]
fn accept_f04_d_order_reader_loose_a_symbolic_link_is_never_followed() {
    let tree = TempTree::new("reader-loose-link");
    let outside = TempTree::new("reader-loose-link-target");
    outside.write("targets.zrd", b"outside the installation");

    let mut mounts = ReaderMounts::new();
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    fs::create_dir_all(tree.root().join("data/common")).expect("the fixture directory is created");
    std::os::unix::fs::symlink(
        outside.root().join("targets.zrd"),
        tree.root().join("data/common/targets.zrd"),
    )
    .expect("the fixture link is created");

    let error = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect_err("a link is not followed, so its target is never served");
    let ReaderLookupError::NotFound { trace, .. } = &error else {
        panic!("the link is not served: {error}");
    };
    assert_eq!(
        loose_trace(&trace.loose)
            .into_iter()
            .filter(|(_, outcome)| outcome == "not_regular")
            .collect::<Vec<_>>(),
        vec![("data/common", "not_regular".to_owned())],
        "the link is reported, never followed out of the declared directory"
    );

    // And it cannot shadow an archive member either.
    let mut with_archive = ReaderMounts::new();
    with_archive.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    with_archive
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    let resolution = with_archive
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("a link cannot shadow an archive member");
    assert_eq!(
        with_archive.read(&resolution).expect("reads"),
        b"root targets"
    );
}

#[test]
fn accept_f04_d_order_reader_loose_read_refuses_a_changed_file_or_an_unregistered_directory() {
    let tree = TempTree::new("reader-loose-read-refusals");
    let mut mounts = ReaderMounts::new();
    mounts
        .add_original_loose_directories(tree.root(), Some("c1c"), Some("mp1"))
        .expect("the original's loose directories declare");
    tree.write("data/c1c/mp1/targets.zrd", b"loose targets");
    let resolution = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the loose file serves the lookup");
    assert_eq!(mounts.read(&resolution).expect("reads"), b"loose targets");

    // A loose resolution is not a member of any archive: reading it through one
    // would return another source's bytes under that archive's provenance.
    let archive = mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        &reader_archive(&[("soils.zrd", b"root soils")]),
    );
    assert_eq!(
        archive
            .read(&resolution)
            .expect_err("an archive cannot read a loose resolution")
            .code(),
        "loose_origin"
    );

    // The file changed after the resolution named it, so the digest it recorded
    // no longer describes what is there.
    tree.write("data/c1c/mp1/targets.zrd", b"loose targets and more");
    assert_eq!(
        mounts
            .read(&resolution)
            .expect_err("a changed loose file is refused")
            .code(),
        "loose_digest_mismatch"
    );

    // A resolution whose span no longer describes the file is stale, like an
    // archive member's.
    let mut moved = resolution.clone();
    moved.span = cs_types::asset_id::SourceSpan::new(
        resolution.span.install_sha256(),
        resolution.span.container_path(),
        resolution.span.member_key(),
        resolution.span.offset() + 1,
        resolution.span.length(),
        resolution.span.member_sha256(),
    )
    .expect("a shifted extent is still a valid span");
    assert_eq!(
        mounts
            .read(&moved)
            .expect_err("a moved extent is refused")
            .code(),
        "stale_resolution"
    );

    // A set that no longer registers the directory cannot rebuild the host
    // path, so the resolution is refused instead of read through a path it does
    // not declare.
    assert_eq!(
        ReaderMounts::new()
            .read(&resolution)
            .expect_err("an unregistered directory is refused")
            .code(),
        "unknown_loose_directory"
    );

    // A fresh lookup of the changed file reads its new bytes: the guard refuses
    // the stale resolution, not the file.
    let fresh = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the changed file is still served");
    assert_eq!(
        mounts.read(&fresh).expect("the fresh resolution reads"),
        b"loose targets and more"
    );
}

// ---------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The one production discovery of the installation this binary shares.
fn installation() -> &'static Discovery {
    static FOUND: OnceLock<Discovery> = OnceLock::new();
    FOUND.get_or_init(|| {
        install::discover(&game_dir()).expect("production discovery reads the installation")
    })
}

fn install_sha256() -> cs_types::evidence::ContentHash {
    install::fingerprint(&installation().manifest)
}

/// A context loading `world_group`/`mission` of the owner's installation.
fn retail_context(world_group: &str, mission: &str) -> ResolveContext {
    ResolveContext::new(install_sha256())
        .with_world_group(WorldGroup::new(world_group).expect("a valid world group"))
        .with_mission(MissionScope::new(mission).expect("a valid mission scope"))
}

/// Mounts one retail reader archive by its installation spelling.
fn retail_archive(relative: &str, level: ReaderLevel) -> ReaderArchive {
    // Mount ids are validated labels, so the installation spelling is folded
    // into one; the container keeps its original spelling.
    let id = format!(
        "reader-{}",
        relative
            .to_ascii_lowercase()
            .replace(['/', '.'], "-")
            .trim_matches('-')
    );
    let mut builder = builder(&id, relative).retail();
    if level != ReaderLevel::Root {
        let component = relative.split('/').nth(1).expect("a world archive");
        builder = builder.with_world_group(
            WorldGroup::new(&format!("zbd/{component}")).expect("a valid world group"),
        );
    }
    if level == ReaderLevel::Mission {
        let component = relative.split('/').nth(2).expect("a mission archive");
        builder = builder.with_mission(
            MissionScope::new(&component.to_ascii_lowercase()).expect("a valid mission scope"),
        );
    }
    mount_reader_archive(builder, &game_dir().join(relative), level)
        .unwrap_or_else(|error| panic!("{relative} mounts as a {level} reader archive: {error}"))
}

/// The reader archives the installation declares, by installation spelling.
fn declared_reader_archives() -> Vec<String> {
    installation()
        .manifest
        .files
        .iter()
        .map(|record| record.relative_spelling.as_str().to_owned())
        .filter(|relative| relative.to_ascii_lowercase().ends_with("/zrdr.zbd"))
        .collect()
}

fn level_of(relative: &str) -> ReaderLevel {
    match relative.split('/').count() {
        2 => ReaderLevel::Root,
        3 => ReaderLevel::World,
        _ => ReaderLevel::Mission,
    }
}

/// Every reader member name the installation's archives declare, case-folded and
/// deduplicated: the names a loose file could shadow or fall back to.
fn declared_member_names() -> Vec<String> {
    let mut names: Vec<String> = declared_reader_archives()
        .iter()
        .flat_map(|relative| {
            let archive = retail_archive(relative, level_of(relative));
            archive
                .members()
                .iter()
                .filter_map(|member| member.name.clone())
                .map(|name| name.to_ascii_lowercase())
                .collect::<Vec<String>>()
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_reader_loose_retail_no_loose_directory_holds_a_member_name() {
    // The measurement that makes both loose rules unreachable from this
    // installation: not one of the original's loose reader directories exists
    // below `data`, and `zbd` — which does exist — holds no loose reader file of
    // any declared member name.
    let mut checked_directories = 0;
    let mut checked_names = 0usize;
    let mut world_groups: Vec<String> = installation()
        .diagnosis
        .world_groups
        .iter()
        .map(|group| group.as_str().to_owned())
        .collect();
    world_groups.sort();
    assert!(
        !world_groups.is_empty(),
        "the installation declares world groups"
    );
    for group in &world_groups {
        let world = group
            .rsplit('/')
            .next()
            .expect("a world group spelling has a last component")
            .to_owned();
        for mission in retail_missions(&world) {
            let mission = mission.to_ascii_lowercase();
            let mounts = loose_set(&world, Some(&mission));
            for directory in mounts.loose_search_order() {
                checked_directories += 1;
                let spelling = directory.spelling();
                let host = directory.host_path();
                if host.exists() {
                    assert_eq!(
                        spelling,
                        READER_ROOT_DIRECTORY,
                        "only the default reader directory exists in this installation; \
                         {spelling} ({}) does not, so nothing measured here can exercise the \
                         loose pass",
                        host.display()
                    );
                }
            }
        }
    }
    // Every declared member name, against every registered directory: no loose
    // file of that name exists anywhere the original would search.
    let names = declared_member_names();
    assert!(names.len() > 200, "the archives declare their members");
    let world = world_groups[0]
        .rsplit('/')
        .next()
        .expect("a world group spelling has a last component")
        .to_owned();
    let mission = retail_missions(&world)
        .first()
        .map(|mission| mission.to_ascii_lowercase())
        .unwrap_or_default();
    let mounts = loose_set(&world, Some(&mission));
    for name in &names {
        checked_names += 1;
        for directory in mounts.loose_search_order() {
            assert!(
                !directory.candidate_path(name).exists(),
                "{} holds {name}, which the loose rules would decide differently",
                directory.host_path().display()
            );
        }
    }
    println!(
        "no loose reader file exists: {checked_names} names x 6 directories, \
         {checked_directories} directories probed"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_reader_loose_retail_every_member_still_resolves_with_the_loose_pass() {
    // With the original's loose directories registered, every member of the
    // root archive is still served by that archive: registering them cannot
    // change a retail answer, because none of them holds anything. This is the
    // production path, not a test-only parser.
    let world = installation()
        .diagnosis
        .world_groups
        .first()
        .expect("the installation declares a world group")
        .as_str()
        .rsplit('/')
        .next()
        .expect("a world group spelling has a last component")
        .to_owned();
    let mission = retail_missions(&world)
        .first()
        .map(|mission| mission.to_ascii_lowercase())
        .unwrap_or_default();
    let context = retail_context(&format!("zbd/{world}"), &mission);

    let root = retail_archive("ZBD/zrdr.zbd", ReaderLevel::Root);
    let member_names: Vec<String> = root
        .members()
        .iter()
        .filter_map(|member| member.name.clone())
        .collect();
    assert_eq!(
        member_names.len(),
        221,
        "the root archive declares 221 entries"
    );

    let mut mounts = ReaderMounts::new();
    mounts.push(root);
    mounts
        .add_original_loose_directories(&game_dir(), Some(&world), Some(&mission))
        .expect("the original's loose directories declare");
    assert_eq!(
        mounts.loose_directories().len(),
        6,
        "a world/mission load registers six loose directories"
    );

    for name in &member_names {
        let resolution = mounts
            .resolve(&context, &key(name))
            .unwrap_or_else(|error| panic!("{name} resolves: {error}"));
        assert!(
            resolution.origin.is_archive(),
            "{name} is served by the archive: a loose file of that name does not exist"
        );
        assert!(
            resolution.trace.loose.iter().all(|attempt| !matches!(
                attempt.outcome,
                ReaderLooseOutcome::Selected { .. } | ReaderLooseOutcome::UndecidableShadow { .. }
            )),
            "{name}: no loose directory holds it, so no override arises"
        );
    }
}

/// The mission directories of one world group, in installation spelling.
fn retail_missions(world: &str) -> Vec<String> {
    declared_reader_archives()
        .iter()
        .filter_map(|relative| {
            let components: Vec<&str> = relative.split('/').collect();
            match components.as_slice() {
                ["ZBD", group, mission, "zrdr.zbd"] if group.eq_ignore_ascii_case(world) => {
                    Some((*mission).to_owned())
                }
                _ => None,
            }
        })
        .collect()
}

/// A mount set with the original's loose directories registered for one
/// world/mission load of the owner's installation.
fn loose_set(world: &str, mission: Option<&str>) -> ReaderMounts {
    let mut mounts = ReaderMounts::new();
    mounts
        .add_original_loose_directories(&game_dir(), Some(world), mission)
        .expect("the original's loose directories declare");
    mounts
}

// ------------------------------------------------------ the measured survey ---

/// What the owner's installation says about the original's two loose rules,
/// measured through production code: the registered directory list, which of
/// those directories exist, whether any of them holds a declared reader member
/// name, and whether registering them changes what the root archive serves.
struct LooseSurvey {
    install_sha256: String,
    content_sha256: String,
    /// `(world group spelling, world directory)` of every declared world group.
    world_groups: Vec<(String, String)>,
    /// `(world directory, mission directory)` of every declared mission.
    missions: Vec<(String, String)>,
    /// `(spelling, exists)` for one world/mission's registered list, in search
    /// order.
    directories: Vec<(String, bool)>,
    /// Declared reader member names, case-folded and deduplicated.
    member_names: usize,
    /// How many of those names a registered loose directory actually holds. The
    /// original's two loose rules can only differ from a plain archive lookup
    /// when this is not zero.
    loose_files_found: usize,
    /// Root archive members resolved with the loose pass registered.
    root_members_resolved: usize,
    /// How many of those were served by the archive.
    root_members_from_archive: usize,
}

/// The survey, measured with the same production helpers the acceptance tests
/// use: `original_loose_reader_directories`, `add_original_loose_directories`,
/// `mount_reader_archive` and `ReaderMounts::resolve`.
fn survey() -> LooseSurvey {
    let found = installation();
    let mut world_groups: Vec<String> = found
        .diagnosis
        .world_groups
        .iter()
        .map(|group| group.as_str().to_owned())
        .collect();
    world_groups.sort();
    let world_directories: Vec<String> = world_groups
        .iter()
        .map(|group| {
            group
                .rsplit('/')
                .next()
                .expect("a last component")
                .to_owned()
        })
        .collect();
    let missions: Vec<(String, String)> = world_directories
        .iter()
        .flat_map(|world| {
            retail_missions(world)
                .into_iter()
                .map(|mission| (world.clone(), mission))
        })
        .collect();
    assert!(
        !missions.is_empty(),
        "the installation declares mission reader archives"
    );

    // One world's registered list, in the original's search order, and whether
    // each directory exists on this host.
    let probe_world = &world_directories[0];
    let probe_mission = missions
        .iter()
        .find(|(world, _)| world == probe_world)
        .map(|(_, mission)| mission.to_ascii_lowercase())
        .expect("a mission of the first world");
    let mounts = loose_set(probe_world, Some(&probe_mission));
    let directories: Vec<(String, bool)> = mounts
        .loose_search_order()
        .map(|directory| {
            (
                directory.spelling().to_owned(),
                directory.host_path().exists(),
            )
        })
        .collect();

    // Every declared member name against every registered directory.
    let names = declared_member_names();
    let mut loose_files_found = 0;
    for name in &names {
        for directory in mounts.loose_search_order() {
            if directory.candidate_path(name).exists() {
                loose_files_found += 1;
            }
        }
    }

    // And the root archive's members, resolved with the loose pass registered.
    let root = retail_archive("ZBD/zrdr.zbd", ReaderLevel::Root);
    let root_member_names: Vec<String> = root
        .members()
        .iter()
        .filter_map(|member| member.name.clone())
        .collect();
    let context = retail_context(&format!("zbd/{probe_world}"), &probe_mission);
    let mut with_root = loose_set(probe_world, Some(&probe_mission));
    with_root.push(root);
    let mut from_archive = 0;
    for name in &root_member_names {
        let resolution = with_root
            .resolve(&context, &key(name))
            .unwrap_or_else(|error| panic!("{name} resolves: {error}"));
        if resolution.origin.is_archive() {
            from_archive += 1;
        }
    }

    LooseSurvey {
        install_sha256: install::fingerprint(&found.manifest).to_hex(),
        content_sha256: install::content_fingerprint(&found.manifest).to_hex(),
        world_groups: world_groups.into_iter().zip(world_directories).collect(),
        missions,
        directories,
        member_names: names.len(),
        loose_files_found,
        root_members_resolved: root_member_names.len(),
        root_members_from_archive: from_archive,
    }
}

/// The JSON artifact of the survey: the registered directory list with what this
/// host holds, and the counts that say whether the loose rules can matter here.
/// Spellings and counts only — never original file bytes.
fn reader_loose_json(candidate_tree: &str, survey: &LooseSurvey) -> String {
    let directories: Vec<String> = survey
        .directories
        .iter()
        .map(|(spelling, exists)| {
            format!("{{\"spelling\": {}, \"exists\": {exists}}}", jstr(spelling))
        })
        .collect();
    let world_groups: Vec<String> = survey
        .world_groups
        .iter()
        .map(|(group, world)| {
            format!(
                "{{\"world_group\": {}, \"world\": {}}}",
                jstr(group),
                jstr(world)
            )
        })
        .collect();
    let missions: Vec<String> = survey
        .missions
        .iter()
        .map(|(world, mission)| {
            format!(
                "{{\"world\": {}, \"mission\": {}}}",
                jstr(world),
                jstr(mission)
            )
        })
        .collect();
    format!(
        "{{\n\
         \x20\"task_id\": \"F04-D-order-reader-loose\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"content_sha256\": {},\n\
         \x20\"modelled\": \"the loose directory fallback: only when no mounted archive holds the \
         name, the registered loose directories are searched most recently added first and `zbd` \
         last; the first regular file of that basename serves the lookup\",\n\
         \x20\"not_modelled\": \"the loose-file override: a loose file of the same basename that is \
         newer (CompareFileTime >= 1) overrides the archive copy. It needs the index entry's \
         trailing u64, whose meaning is unknown (#692), so a lookup that finds both an archive \
         member and a loose file is refused (ReaderLookupError::LooseOverrideUndecided)\",\n\
         \x20\"loose_order_status\": \"inferred (read off the original executable, not a runtime \
         capture)\",\n\
         \x20\"override_status\": \"unknown (the rule exists, the decision is not implemented)\",\n\
         \x20\"designed_precedence_order_status\": \"designed (PRECEDENCE_ORDER_STATUS, untouched)\",\n\
         \x20\"world_groups\": [{}],\n\
         \x20\"missions\": [{}],\n\
         \x20\"registered_directories_in_search_order\": [\n  {}\n ],\n\
         \x20\"declared_member_names\": {},\n\
         \x20\"loose_files_of_a_declared_member_name\": {},\n\
         \x20\"root_members_resolved_with_the_loose_pass\": {},\n\
         \x20\"root_members_served_by_the_archive\": {}\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        jstr(&survey.install_sha256),
        jstr(&survey.content_sha256),
        world_groups.join(", "),
        missions.join(", "),
        directories.join(",\n  "),
        survey.member_names,
        survey.loose_files_found,
        survey.root_members_resolved,
        survey.root_members_from_archive,
    )
}

// ------------------------------------------------------- evidence harness ---

/// Evidence-report harness for task #700 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. `CS_EVIDENCE_REVIEWER` names the agent
/// that ran it and is recorded in the report; it is not baked in, because the
/// reviewer regenerates the report on the rebased commit. Run from the
/// workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/T700
///    cargo test --workspace --locked -- accept_f04_d_order_reader_loose_ --include-ignored \
///      2>&1 | tee private/evidence/T700/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. with `pipefail`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T700 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f04_d_order_reader_loose_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///    CS_EVIDENCE_REVIEWER="<agent running this harness>" \
///      cargo test --locked -p cs_assets --test accept_f04_d_order_reader_loose -- evidence_report_t700 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T700/acceptance.json \
///      --artifact-root private/evidence/T700
///    ```
///    **Without** `--require-pass`: that flag rejects a report that still lists
///    unresolved issues, and this task's `unknowns` are exactly the limitations
///    its acceptance pins — the unmodelled loose-file override and its unknown
///    timestamp (#692), which loose file the override compares, the
///    case-sensitivity of the loose pass, the never-followed link, the caller's
///    choice of loose directories (#687) and the code-derived-only orders. They
///    are recorded rather than dropped, so the flag exits 3 with "Unresolved
///    issues" and that is the expected result. `--require-pass` on this report
///    is a false green.
/// 4. Commit a copy of `acceptance.json` as
///    `docs/findings/evidence/T700.json`.
///
/// Every field is derived from real inputs: the recorded log, production
/// discovery of `$CS_GAME_DIR`, the production loose-directory survey
/// (`reader-loose.json`: spellings, existence and counts only), `rustc
/// --version` and `Cargo.lock`. The `unknowns` are the literal limitations of
/// `docs/findings/2026-10-06-f04-d-reader-loose-rules.md`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t700_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(!argv.is_empty(), "CS_EVIDENCE_ARGV must hold the command");
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit; old reports cannot be reused \
         for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "no `{PREFIX}` results were understood in {}",
        log_path.display()
    );
    for retail_test in [
        format!("{PREFIX}retail_no_loose_directory_holds_a_member_name"),
        format!("{PREFIX}retail_every_member_still_resolves_with_the_loose_pass"),
    ] {
        assert_eq!(
            suite
                .assertions
                .iter()
                .find(|(name, _)| *name == retail_test)
                .map(|(_, status)| *status),
            Some("pass"),
            "{retail_test} must have run and passed (step 1 needs --include-ignored and \
             CS_GAME_DIR)"
        );
    }
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| !name.contains("_retail_")),
        "synthetic task tests must be present alongside the retail ones"
    );

    // The survey is production work: the original's directory list is built by
    // `original_loose_reader_directories`, registered by
    // `add_original_loose_directories`, and consulted by `ReaderMounts::resolve`.
    let measured = survey();
    assert_eq!(
        measured.loose_files_found, 0,
        "no loose reader directory of this installation holds a declared member name, so retail \
         data cannot exercise either loose rule"
    );
    assert_eq!(
        measured.root_members_from_archive, measured.root_members_resolved,
        "registering the loose directories changes no root archive answer, because none of them \
         holds a loose file"
    );
    let lookup_path = evidence_dir.join("reader-loose.json");
    fs::write(&lookup_path, reader_loose_json(&candidate_tree, &measured))
        .unwrap_or_else(|error| panic!("write {}: {error}", lookup_path.display()));
    let artifacts = [artifact(&log_path, "log"), artifact(&lookup_path, "json")];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F04-D-order-reader-loose\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [{}],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
        jstr(&iso_utc_now()),
        argv.iter()
            .map(|arg| jstr(arg))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&measured.install_sha256),
        jstr(&measured.content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        suite
            .assertions
            .iter()
            .map(|(name, status)| format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        artifacts
            .iter()
            .map(|(name, digest, kind)| format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        UNKNOWNS
            .iter()
            .map(|unknown| jstr(unknown))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR, the production \
             loose-directory survey (original_loose_reader_directories, \
             add_original_loose_directories, mount_reader_archive, ReaderMounts::resolve), rustc \
             and Cargo.lock. The loose directory fallback is modelled and the loose-file override \
             is deliberately not: a lookup that would have to apply CompareFileTime is refused, \
             because the archive-side timestamp is unknown (#692), so nothing here is inferred \
             from a run of the original and PRECEDENCE_ORDER_STATUS stays `designed`. The \
             `unknowns` array is deliberately non-empty and names the affected content and its \
             resolving task for each limitation, so this report must be validated WITHOUT \
             --require-pass: that flag rejects a report with unresolved issues and would only be \
             green if they had been dropped. The claim is `implemented`, never \
             `verified_original` or `release_approved`."
        ),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ------------------------------------------------------------ harness utils ---

fn env_var(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is not set: run the harness through the sequence above"))
}

/// Cargo runs a test binary in the package root; re-anchor paths described
/// relative to the workspace root.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The locked version of one `Cargo.lock` package, read, never assumed.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
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
    panic!("package {package:?} is not in {}", lock_path.display());
}

/// What the recorded `cargo test` output says happened.
#[derive(Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// The libtest summaries and the per-test results of this task's tests.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if let Some(summary) = trimmed.strip_prefix("test result:") {
            for segment in summary.split(';') {
                let words: Vec<&str> = segment.split_whitespace().collect();
                for pair in words.windows(2) {
                    if let Ok(count) = pair[0].parse::<u64>() {
                        match pair[1] {
                            "passed" => suite.passed += count,
                            "failed" => suite.failed += count,
                            "ignored" => suite.ignored += count,
                            _ => continue,
                        }
                        break;
                    }
                }
            }
            continue;
        }
        if !pending.is_empty() && (trimmed == "ok" || trimmed == "FAILED") {
            let name = pending.pop_front().expect("pending test");
            record(
                &mut suite,
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
            cursor = &after[separator + 5..];
            if !name.starts_with(PREFIX) {
                continue;
            }
            match cursor.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.executed + suite.ignored;
    suite
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if !suite.assertions.iter().any(|(seen, _)| *seen == name) {
        suite.assertions.push((name, status));
    }
}

/// `(file name, sha256, kind)` of an artifact inside the evidence directory.
fn artifact(path: &Path, kind: &str) -> (String, String, String) {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    (
        path.file_name()
            .expect("artifact has a file name")
            .to_string_lossy()
            .into_owned(),
        sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

/// A JSON string literal, quoted and escaped.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 UTC with whole seconds (Hinnant's `civil_from_days`).
fn iso_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_era + era * 400 + 1
    } else {
        year_of_era + era * 400
    };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// The paths this task leaves unresolved, each naming the affected content and
/// the task or evidence that owns it. They are recorded, never dropped.
const UNKNOWNS: [&str; 6] = [
    "The original's loose-file override is NOT modelled and the lookup that would have to apply \
     it is refused (ReaderLookupError::LooseOverrideUndecided): the rule exists (CompareFileTime \
     >= 1 against the archive copy) but its archive side needs the index entry's trailing u64, \
     whose meaning is unknown. Affected content: every reader (.zrd) member that a loose file of \
     the same basename could shadow — a new collision class, invisible in this installation \
     because no loose reader directory exists. Resolving task: #692 F04-D-index-tail (the \
     timestamp); modelling the comparison needs that first.",
    "Which loose file the original compares in the override is not pinned by the finding: the \
     loose pass (0x579710 -> 0x59d170) is described as running only when no archive holds the \
     name, while the override needs a loose file to compare. This task records the most \
     recently added regular file as the candidate and refuses. Affected content: the refusal's \
     choice of candidate, not its refusal. Resolving task: further static work or an \
     owner-supplied capture (#358).",
    "The loose pass matches a host file name case-exactly on a case-sensitive host, while the \
     original's Windows filesystem was case-insensitive; nothing measured distinguishes the two \
     because no loose reader file exists in this installation. Affected content: reader lookups \
     against a modded or hand-edited installation with loose files. Resolving task: none filed; \
     it needs a host whose loose directories exist.",
    "A loose reader file that is a symbolic link, or any non-regular entry, is never served and \
     can never shadow an archive member: this engine never follows a link (the guard \
     mount_directory already applies), while the original's host would have opened and compared \
     it. Affected content: installations that ship loose reader files through links. Resolving \
     task: none filed; the guard is a deliberate engine decision, recorded here rather than \
     claimed as the original's behaviour.",
    "Which loose directories belong to a context is the caller's declaration: \
     ReaderMounts::add_original_loose_directories registers them without a MountScope, so a set \
     registered for one mission is not refused for another. Affected content: every reader \
     resolution that consults the loose pass. Resolving task: #687 F04-D-order-archives (the \
     unbound mission level).",
    "Both loose orders remain code-derived only: no run of the original observed them, so \
     READER_LOOSE_ORDER_STATUS is `inferred`, READER_LOOSE_OVERRIDE_STATUS is `unknown` and \
     PRECEDENCE_ORDER_STATUS stays `designed`. Affected content: every loose pass and every \
     refusal. Settling them against a run of the original needs owner-supplied capture \
     (REF-OWNER-FIRST-CAPTURE, #358) or further static work.",
];
