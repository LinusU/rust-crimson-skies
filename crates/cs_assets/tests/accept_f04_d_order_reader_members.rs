//! The original engine's reader-member lookup: **basename keys** over the
//! archives mounted as **[root, mission, world]**, first hit wins (task #685,
//! `docs/findings/2026-10-05-f04-d-original-lookup-order.md` sections A, F
//! and G).
//!
//! The finding records what the original's reader file system does — it
//! reduces a requested name to its **basename**, walks the archives in mount
//! order and serves the **first** case-insensitive match, the first index
//! entry inside the winning archive — and measures what that order means for
//! this installation: **mission shadows world** in exactly five cases
//! (`targets.zrd` in C1C/IA1, MP1 and MP3; `security_destroy.zrd` in C2/M01;
//! `fueltruck.zrd` in C3/M02), and the root archive declares `player.zrd`
//! **twice** — entry 22 is the only copy a name can reach, entry 100 is
//! unreachable by name.
//!
//! # What these tests hold the engine to
//!
//! Production [`cs_assets::vfs::reader`] mounts a `zrdr.zbd` through the
//! `cs_formats` reader chain and keys each member by its **basename**, so a
//! request for `data/common/zrdr/targets.zrd` reaches the member
//! `targets.zrd`. The search order is the original's: **root first**, then
//! **mission**, then **world**, and the first archive that holds the name
//! serves it. This is deliberately **not** `Vfs::resolve` with a different
//! `PrecedenceClass`: the designed classes rank mission/world **above**
//! shared, so no class expresses a root-first order, and a reader lookup
//! reports its own order status ([`READER_LOOKUP_ORDER_STATUS`], `inferred` —
//! code-derived, never a runtime capture) while leaving
//! [`PRECEDENCE_ORDER_STATUS`] at `designed`.
//!
//! The synthetic tests author reader archives byte by byte (a version-one
//! trailer plus 148-byte entries — the layout task #340 measured — carrying the
//! retail-observed 76 unexplained bytes) and mount them through production
//! code, including the duplicate-name, first-entry and non-key-spellable cases
//! retail does not show. The `retail_` tests read `$CS_GAME_DIR` read-only
//! through the same production mount and fail loudly without it; CI skips
//! them.

mod common;

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use common::TempTree;
use cs_assets::install::{self, Discovery, sha256};
use cs_assets::vfs::{
    MountBuilder, READER_LOOKUP_ORDER_STATUS, READER_NAMESPACE, ReaderArchive,
    ReaderAttemptOutcome, ReaderLevel, ReaderLookupError, ReaderMounts, SkipReason, Unreachable,
    mount_reader_archive,
};
use cs_formats::zbd::{INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES};
use cs_types::asset_id::{
    AssetKey, AssetVariant, MissionScope, MountId, MountNamespace, PRECEDENCE_ORDER_STATUS,
    PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::RelativePath;

/// The acceptance prefix of this task.
const PREFIX: &str = "accept_f04_d_order_reader_";

/// The `u32` every retail reader index entry carries at the head of its 76
/// unexplained bytes (section F of the finding). Measured on this
/// installation; **what it encodes is unknown**, so production reads nothing
/// from it and the fixture only writes the bytes the pinned source's layout
/// puts there.
const ENTRY_WORD: u32 = 2;

/// A trailing `u64` every retail entry carries and never zero (section F).
const ENTRY_STAMP: u64 = 1_000_000_000;

// -------------------------------------------------------------- fixtures ---

/// One authored index entry of a reader archive.
struct Entry {
    start: u32,
    length: u32,
    name: Vec<u8>,
}

/// An entry whose bytes live at `offset` in the archive's data region.
fn member(offset: u32, name: &str, bytes: &[u8]) -> Entry {
    Entry {
        start: offset,
        length: u32::try_from(bytes.len()).expect("a fixture member is short"),
        name: name.as_bytes().to_vec(),
    }
}

/// `data`, then `entries.len()` entries of 148 bytes, then the version-one
/// trailer — the layout [`read_version_one_index`] reads.
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

/// An archive holding `members` back to back, each indexed under its own name
/// in the order given.
fn reader_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (name, bytes) in members {
        entries.push(member(data.len() as u32, name, bytes));
        data.extend_from_slice(bytes);
    }
    archive(&data, &entries)
}

/// The builder a reader mount is declared with: the reader key space and an
/// installation spelling.
///
/// The precedence class describes the mount to the **designed** precedence
/// machinery; the reader lookup ranks by mount level instead, so the class
/// here is only what the mount reports about itself.
fn builder(id: &str, container: &str, precedence: PrecedenceClass) -> MountBuilder {
    MountBuilder::new(
        MountId::new(id).expect("a valid mount id"),
        MountNamespace::new(READER_NAMESPACE).expect("a valid namespace"),
        precedence,
        container,
    )
}

/// A reader key naming `name`.
fn key(name: &str) -> AssetKey {
    AssetKey::from_spelling(READER_NAMESPACE, name, "default").expect("a valid reader key")
}

/// A reader key for an arbitrary spelling, so a caller-spelled directory path
/// can be reduced to its basename the way the original reduces it.
fn spelled_key(spelling: &str) -> AssetKey {
    AssetKey::new(
        MountNamespace::new(READER_NAMESPACE).expect("a valid namespace"),
        RelativePath::new(spelling).unwrap_or_else(|error| panic!("{spelling:?}: {error}")),
        AssetVariant::default(),
    )
}

/// Writes `bytes` under `container` in `tree` and mounts it at `level`.
fn mount(
    tree: &TempTree,
    container: &str,
    level: ReaderLevel,
    builder: MountBuilder,
    bytes: &[u8],
) -> ReaderArchive {
    tree.write(container, bytes);
    let path = tree.root().join(container);
    mount_reader_archive(builder, &path, level).unwrap_or_else(|error| {
        panic!(
            "{} mounts as a {level} reader archive: {error}",
            path.display()
        )
    })
}

/// The fixture world group, and the fixture installation fingerprint.
fn fixture_world() -> WorldGroup {
    WorldGroup::new("zbd/c1c").expect("a valid world group")
}

fn fixture_context(mission: &str) -> ResolveContext {
    ResolveContext::new(sha256(b"685 synthetic installation"))
        .with_world_group(fixture_world())
        .with_mission(MissionScope::new(mission).expect("a valid mission scope"))
}

/// A builder bound to the fixture world (and, for a mission archive, to
/// `mission`).
fn fixture_builder(id: &str, container: &str, mission: Option<&str>) -> MountBuilder {
    let builder = builder(id, container, PrecedenceClass::Shared).with_world_group(fixture_world());
    match mission {
        Some(mission) => {
            builder.with_mission(MissionScope::new(mission).expect("a valid mission scope"))
        }
        None => builder,
    }
}

// ------------------------------------------------------------- synthetic ---

#[test]
fn accept_f04_d_order_reader_root_archive_shadows_mission_and_world() {
    let tree = TempTree::new("reader-root-first");
    let mut mounts = ReaderMounts::new();
    // Added world-first on purpose: the lookup must use the original's mount
    // order, not the order the archives were registered in.
    mounts.push(mount(
        &tree,
        "zbd/c1c/zrdr.zbd",
        ReaderLevel::World,
        fixture_builder("reader-world", "zbd/c1c/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"world targets")]),
    ));
    mounts.push(mount(
        &tree,
        "zbd/c1c/mp1/zrdr.zbd",
        ReaderLevel::Mission,
        fixture_builder("reader-mission", "zbd/c1c/mp1/zrdr.zbd", Some("mp1")),
        &reader_archive(&[("targets.zrd", b"mission targets")]),
    ));
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));

    let resolution = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the root archive holds targets.zrd");
    assert_eq!(resolution.level, ReaderLevel::Root);
    assert_eq!(resolution.container, "zbd/zrdr.zbd");
    assert_eq!(resolution.member, "targets.zrd");
    assert_eq!(resolution.basename, "targets.zrd");
    assert_eq!(resolution.entry_index, 0);
    assert_eq!(mounts.read(&resolution).expect("reads"), b"root targets");

    // The trace is in the original's mount order and says which archive lost
    // the name, which is the fact the order decides.
    let trace: Vec<(&str, ReaderLevel, ReaderAttemptOutcome)> = resolution
        .trace
        .attempts
        .iter()
        .map(|attempt| {
            (
                attempt.container.as_str(),
                attempt.level,
                attempt.outcome.clone(),
            )
        })
        .collect();
    assert_eq!(
        trace,
        vec![
            (
                "zbd/zrdr.zbd",
                ReaderLevel::Root,
                ReaderAttemptOutcome::Selected { entry_index: 0 }
            ),
            (
                "zbd/c1c/mp1/zrdr.zbd",
                ReaderLevel::Mission,
                ReaderAttemptOutcome::Shadowed { entry_index: 0 }
            ),
            (
                "zbd/c1c/zrdr.zbd",
                ReaderLevel::World,
                ReaderAttemptOutcome::Shadowed { entry_index: 0 }
            ),
        ],
        "the root archive is searched first and both lower archives are reported shadowed"
    );
    assert_eq!(
        resolution.trace.order_status, READER_LOOKUP_ORDER_STATUS,
        "a reader trace reports the reader order's own status, not the designed one"
    );
    assert_eq!(
        resolution.trace.order_status,
        ClaimStatus::Inferred,
        "the order is read off the executable, so it is never verified_original"
    );
    assert_eq!(
        resolution.span.container_path(),
        "zbd/zrdr.zbd",
        "the span names the archive that served the member"
    );
    assert_eq!(resolution.span.member_key(), Some("targets.zrd"));
}

#[test]
fn accept_f04_d_order_reader_mission_shadows_world_when_the_root_has_no_such_member() {
    let tree = TempTree::new("reader-mission-over-world");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/c1c/zrdr.zbd",
        ReaderLevel::World,
        fixture_builder("reader-world", "zbd/c1c/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"world targets")]),
    ));
    mounts.push(mount(
        &tree,
        "zbd/c1c/mp3/zrdr.zbd",
        ReaderLevel::Mission,
        fixture_builder("reader-mission", "zbd/c1c/mp3/zrdr.zbd", Some("mp3")),
        &reader_archive(&[("targets.zrd", b"mission targets")]),
    ));
    // The root archive holds other members, not this one, so it is searched
    // first and misses.
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("soils.zrd", b"root soils")]),
    ));

    let context = fixture_context("mp3");
    let resolution = mounts
        .resolve(&context, &key("targets.zrd"))
        .expect("the mission archive holds targets.zrd");
    assert_eq!(resolution.level, ReaderLevel::Mission);
    assert_eq!(resolution.container, "zbd/c1c/mp3/zrdr.zbd");
    assert_eq!(mounts.read(&resolution).expect("reads"), b"mission targets");
    assert_eq!(
        resolution.trace.attempts[0].outcome,
        ReaderAttemptOutcome::Miss,
        "the root archive is searched first and holds no such member"
    );
    assert_eq!(
        resolution.trace.attempts[1].outcome,
        ReaderAttemptOutcome::Selected { entry_index: 0 }
    );
    assert_eq!(
        resolution.trace.attempts[2].outcome,
        ReaderAttemptOutcome::Shadowed { entry_index: 0 }
    );

    // Without the mission archive the world copy serves instead: the two
    // archives hold different bytes, so this is the shadowing that decides.
    let mut world_only = ReaderMounts::new();
    world_only.push(mount(
        &tree,
        "zbd/c1c/zrdr.zbd",
        ReaderLevel::World,
        fixture_builder("reader-world-alone", "zbd/c1c/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"world targets")]),
    ));
    let fallback = world_only
        .resolve(&context, &key("targets.zrd"))
        .expect("the world archive holds targets.zrd");
    assert_eq!(fallback.level, ReaderLevel::World);
    assert_eq!(
        world_only.read(&fallback).expect("reads"),
        b"world targets",
        "the world copy differs from the mission copy that shadowed it"
    );
}

#[test]
fn accept_f04_d_order_reader_lookup_reduces_the_request_to_its_basename() {
    let tree = TempTree::new("reader-basename");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));

    // The original reduces the requested name to its basename before it
    // searches, so a spelling with directories reaches the same member, and
    // the member keeps its own spelling in the answer.
    for requested in [
        "targets.zrd",
        "data/common/zrdr/targets.zrd",
        "zbd/c1c/mp1/targets.zrd",
        "zrdr\\planes\\targets.zrd",
    ] {
        let requested = spelled_key(requested);
        let resolution = mounts
            .resolve(&fixture_context("mp1"), &requested)
            .unwrap_or_else(|error| panic!("{requested} resolves: {error}"));
        assert_eq!(
            resolution.basename,
            requested.path_key().rsplit('/').next().expect("a basename"),
            "{requested} is searched for by basename"
        );
        assert_eq!(
            resolution.member, "targets.zrd",
            "{requested} gets the member's own spelling back"
        );
        assert_eq!(resolution.requested, requested);
        assert_eq!(
            mounts.read(&resolution).expect("reads"),
            b"root targets",
            "{requested} reaches the same member"
        );
    }
}

#[test]
fn accept_f04_d_order_reader_matches_case_insensitively_and_serves_the_first_entry() {
    let tree = TempTree::new("reader-case-and-first-entry");
    // One archive declaring one name twice, with different spellings and
    // different bytes: the original's scan is case-insensitive and serves the
    // first entry, so the second is unreachable by name — and both stay rows.
    let bytes = {
        let first = b"first copy".to_vec();
        let second = b"second copy".to_vec();
        let mut data = first.clone();
        let offset = data.len() as u32;
        data.extend_from_slice(&second);
        archive(
            &data,
            &[
                member(0, "Player.zrd", &first),
                member(offset, "player.zrd", &second),
            ],
        )
    };
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &bytes,
    ));
    let archive = &mounts.archives()[0];

    assert_eq!(archive.len(), 2, "both declared entries are inventory rows");
    assert!(archive.members()[0].reachable_by_name());
    assert_eq!(
        archive.members()[1].unreachable,
        Some(Unreachable::DuplicateName { served_by: 0 }),
        "the later entry of one name is unreachable, naming the entry that answers it"
    );
    assert_eq!(
        archive.mount().member_count(),
        1,
        "only the entry a name can reach becomes a mount member"
    );
    assert_eq!(archive.unreachable().count(), 1);

    for requested in ["player.zrd", "PLAYER.ZRD", "Player.zrd"] {
        let resolution = mounts
            .resolve(&fixture_context("mp1"), &key(requested))
            .unwrap_or_else(|error| panic!("{requested} resolves: {error}"));
        assert_eq!(
            resolution.entry_index, 0,
            "{requested} reaches the first entry"
        );
        assert_eq!(
            mounts.read(&resolution).expect("reads"),
            b"first copy",
            "{requested} serves the first entry's bytes"
        );
    }
}

#[test]
fn accept_f04_d_order_reader_a_name_that_is_no_key_is_archived_unreachable() {
    let tree = TempTree::new("reader-unspellable");
    // `..` cannot be spelled as a key, so no lookup could ever reach it; the
    // archive must still carry the row rather than drop the member.
    let bytes = {
        let first = b"parent named".to_vec();
        let offset = first.len() as u32;
        let mut data = first.clone();
        data.extend_from_slice(b"soils");
        archive(
            &data,
            &[
                member(0, "..", &first),
                member(offset, "soils.zrd", b"soils"),
            ],
        )
    };
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &bytes,
    ));
    let archive = &mounts.archives()[0];
    assert_eq!(archive.len(), 2);
    assert_eq!(
        archive.members()[0].unreachable,
        Some(Unreachable::InvalidName),
        "a name no key could spell is unreachable, not dropped"
    );
    assert!(archive.members()[1].reachable_by_name());
    assert_eq!(archive.mount().member_count(), 1);
    assert_eq!(
        mounts
            .read(
                &mounts
                    .resolve(&fixture_context("mp1"), &key("soils.zrd"))
                    .expect("the sibling member is reachable")
            )
            .expect("reads"),
        b"soils"
    );

    // A request that names a `..` component cannot become a key at all, so it
    // is refused before the lookup and never reaches an archive.
    let escape = RelativePath::new("../zrdr/soils.zrd");
    assert!(
        escape.is_err(),
        "a parent component is refused when the key is built, as F04 requires"
    );
}

#[test]
fn accept_f04_d_order_reader_an_unreadable_or_unspellable_entry_stays_a_row() {
    // Two more ways a declared entry cannot be served, neither of which retail
    // shows: an extent that leaves the container (the listing's bounds check
    // refuses it, so the row has no digest) and a name that is not UTF-8 (no
    // key spelling could match it). Both must stay inventory rows with their
    // reason, and must not stop their siblings from being mounted.
    let bytes = {
        let first = b"soils".to_vec();
        let mut data = first.clone();
        let good = data.len() as u32;
        data.extend_from_slice(b"out of bounds");
        let mut entries = vec![member(0, "soils.zrd", &first)];
        // An extent that runs past the end of the data region.
        entries.push(Entry {
            start: 4_000,
            length: 64,
            name: b"beyond.zrd".to_vec(),
        });
        // A name that is not UTF-8 at all.
        entries.push(Entry {
            start: good,
            length: 0,
            name: vec![0xff, 0xfe, b'.', b'z', b'r', b'd'],
        });
        archive(&data, &entries)
    };
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree_of("reader-unreadable"),
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &bytes,
    ));
    let archive = &mounts.archives()[0];

    assert_eq!(archive.len(), 3, "every declared entry is a row");
    assert_eq!(
        archive.mount().member_count(),
        1,
        "only soils.zrd is servable"
    );
    let reasons: Vec<(usize, Unreachable)> = archive
        .unreachable()
        .map(|member| (member.entry_index, member.unreachable.expect("a reason")))
        .collect();
    assert_eq!(
        reasons,
        vec![
            (1, Unreachable::FailedBounds),
            (2, Unreachable::NonUtf8Name),
        ],
        "each refusal keeps the entry with the reason it was refused"
    );
    let beyond = archive.member_at(1).expect("entry 1 is a row");
    assert_eq!(beyond.name.as_deref(), Some("beyond.zrd"));
    assert_eq!(
        beyond.sha256, None,
        "an extent outside the archive has no bytes to hash"
    );
    assert!(!beyond.reachable_by_name());
    assert_eq!(
        archive.member("beyond.zrd"),
        None,
        "a member with no bytes is never served"
    );
    assert!(
        mounts
            .resolve(&fixture_context("mp1"), &key("soils.zrd"))
            .is_ok(),
        "the valid sibling is still mounted and served"
    );
}

/// A fresh fixture tree; each test owns one so no two share a host path.
fn tree_of(label: &str) -> TempTree {
    TempTree::new(label)
}

#[test]
fn accept_f04_d_order_reader_a_skipped_archive_is_reported_and_the_next_one_serves() {
    let tree = TempTree::new("reader-skip-order");
    // The mission archive belongs to `mp1`; a `mp3` context does not admit it,
    // so the world copy has to serve.
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/c1c/mp1/zrdr.zbd",
        ReaderLevel::Mission,
        fixture_builder("reader-mission-mp1", "zbd/c1c/mp1/zrdr.zbd", Some("mp1")),
        &reader_archive(&[("targets.zrd", b"mp1 targets")]),
    ));
    mounts.push(mount(
        &tree,
        "zbd/c1c/zrdr.zbd",
        ReaderLevel::World,
        fixture_builder("reader-world", "zbd/c1c/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"world targets")]),
    ));

    let resolution = mounts
        .resolve(&fixture_context("mp3"), &key("targets.zrd"))
        .expect("the world archive is admitted and holds the name");
    assert_eq!(resolution.level, ReaderLevel::World);
    assert_eq!(
        resolution.trace.attempts[0].outcome,
        ReaderAttemptOutcome::Skipped(SkipReason::ScopeMismatch),
        "the mission archive of another mission is not mounted for this context"
    );
    assert_eq!(
        resolution.trace.attempts[1].outcome,
        ReaderAttemptOutcome::Selected { entry_index: 0 }
    );
    assert_eq!(mounts.read(&resolution).expect("reads"), b"world targets");

    // The very same mount list serves the mission copy under the context that
    // does admit it, so the scope is what changed the answer.
    let admitted = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the mission archive is admitted here");
    assert_eq!(admitted.level, ReaderLevel::Mission);
    assert_eq!(mounts.read(&admitted).expect("reads"), b"mp1 targets");
}

#[test]
fn accept_f04_d_order_reader_archives_of_another_world_are_never_searched() {
    let tree = TempTree::new("reader-other-world");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/c1c/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-c1c-root", "zbd/c1c/zrdr.zbd", None),
        &reader_archive(&[("aiv.zrd", b"c1c root aiv")]),
    ));

    let other = ResolveContext::new(sha256(b"685 synthetic installation"))
        .with_world_group(WorldGroup::new("zbd/c1").expect("a valid world group"))
        .with_mission(MissionScope::new("m01").expect("a valid mission scope"));
    let error = mounts
        .resolve(&other, &key("aiv.zrd"))
        .expect_err("another world's archive is not mounted for this context");
    match &error {
        ReaderLookupError::NotFound {
            basename, trace, ..
        } => {
            assert_eq!(basename, "aiv.zrd");
            assert_eq!(
                trace.attempts[0].outcome,
                ReaderAttemptOutcome::Skipped(SkipReason::ScopeMismatch),
                "the only mounted archive is out of scope, so it is not searched"
            );
        }
        other => panic!("an out-of-scope archive serves nothing: {other}"),
    }
    assert!(
        mounts
            .resolve(&fixture_context("m01"), &key("aiv.zrd"))
            .is_ok(),
        "the same archive serves the world it is bound to"
    );
}

#[test]
fn accept_f04_d_order_reader_an_absent_name_reports_every_archive_it_searched() {
    let tree = TempTree::new("reader-not-found");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("soils.zrd", b"root soils")]),
    ));
    mounts.push(mount(
        &tree,
        "zbd/c1c/zrdr.zbd",
        ReaderLevel::World,
        fixture_builder("reader-world", "zbd/c1c/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"world targets")]),
    ));

    let error = mounts
        .resolve(&fixture_context("mp1"), &key("objectives.zrd"))
        .expect_err("no archive holds objectives.zrd");
    match &error {
        ReaderLookupError::NotFound {
            basename, trace, ..
        } => {
            assert_eq!(basename, "objectives.zrd");
            assert_eq!(
                trace
                    .attempts
                    .iter()
                    .map(|attempt| attempt.outcome.clone())
                    .collect::<Vec<_>>(),
                vec![ReaderAttemptOutcome::Miss, ReaderAttemptOutcome::Miss],
                "both archives were searched and neither held the name"
            );
        }
        other => panic!("an absent member is not served: {other}"),
    }
    assert!(
        error.to_string().contains("zbd/zrdr.zbd"),
        "the refusal names the archives it searched: {error}"
    );
}

#[test]
fn accept_f04_d_order_reader_a_mount_outside_the_reader_key_space_is_refused() {
    let tree = TempTree::new("reader-foreign-namespace");
    let bytes = reader_archive(&[("targets.zrd", b"root targets")]);
    tree.write("zbd/zrdr.zbd", &bytes);
    let builder = MountBuilder::new(
        MountId::new("reader-root").expect("a valid mount id"),
        MountNamespace::new("install").expect("a valid namespace"),
        PrecedenceClass::Shared,
        "zbd/zrdr.zbd",
    );
    let error = mount_reader_archive(
        builder,
        &tree.root().join("zbd/zrdr.zbd"),
        ReaderLevel::Root,
    )
    .expect_err("a reader archive outside the reader key space can never be served");
    assert_eq!(error.code(), "foreign_namespace");
    assert_eq!(error.container(), "zbd/zrdr.zbd");
}

#[test]
fn accept_f04_d_order_reader_an_empty_archive_is_mounted_and_serves_nothing() {
    let tree = TempTree::new("reader-empty");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-empty", "zbd/zrdr.zbd", None),
        &reader_archive(&[]),
    ));
    assert!(mounts.archives()[0].is_empty());
    assert_eq!(mounts.archives()[0].mount().member_count(), 0);
    assert!(matches!(
        mounts.resolve(&fixture_context("mp1"), &key("targets.zrd")),
        Err(ReaderLookupError::NotFound { .. })
    ));
}

#[test]
fn accept_f04_d_order_reader_read_refuses_a_stale_or_foreign_resolution() {
    let tree = TempTree::new("reader-stale");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    let resolution = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the root archive holds the name");

    // A resolution stamped with another mount names bytes this archive does
    // not hold, so the mount set refuses it before it looks at any archive.
    let mut foreign = resolution.clone();
    foreign.mount = MountId::new("reader-other").expect("a valid mount id");
    assert_eq!(
        mounts
            .read(&foreign)
            .expect_err("a foreign mount is refused")
            .code(),
        "unknown_archive"
    );
    // Read through the archive itself, the other mount is refused there.
    assert_eq!(
        mounts.archives()[0]
            .read(&foreign)
            .expect_err("the archive that does not hold the mount refuses it")
            .code(),
        "foreign_archive"
    );

    // A resolution that claims an entry the archive does not have is stale.
    let mut stale = resolution.clone();
    stale.entry_index = 7;
    assert_eq!(
        mounts
            .read(&stale)
            .expect_err("a stale resolution is refused")
            .code(),
        "stale_resolution"
    );

    // A resolution whose span claims another extent is refused too, instead of
    // reading whatever now sits there.
    let mut moved = resolution.clone();
    moved.span = cs_types::asset_id::SourceSpan::new(
        resolution.span.install_sha256(),
        resolution.span.container_path(),
        resolution.span.member_key(),
        resolution.span.offset() + 1,
        resolution.span.length(),
        resolution.span.member_sha256(),
    )
    .expect("a valid span");
    assert_eq!(
        mounts
            .read(&moved)
            .expect_err("a moved span is refused")
            .code(),
        "stale_resolution"
    );

    assert_eq!(
        mounts.read(&resolution).expect("the real resolution reads"),
        b"root targets"
    );
}

#[test]
fn accept_f04_d_order_reader_a_row_that_does_not_describe_the_bytes_is_refused() {
    // `read_entry` takes a row and returns the bytes that row describes. A row
    // from somewhere else — another archive, a tampered digest, an extent
    // outside the container — must not be read as if it were this archive's,
    // so the guard is checked rather than assumed.
    let tree = TempTree::new("reader-row-guard");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    let archive = &mounts.archives()[0];
    let member = archive.members()[0].clone();
    assert_eq!(
        archive.read_entry(&member).expect("the real row reads"),
        b"root targets"
    );

    let mut wrong_digest = member.clone();
    wrong_digest.sha256 = Some(sha256(b"different bytes"));
    assert_eq!(
        archive
            .read_entry(&wrong_digest)
            .expect_err("a digest the stored bytes do not have is refused")
            .code(),
        "digest_mismatch"
    );

    let mut beyond = member.clone();
    beyond.offset = 1 << 40;
    assert_eq!(
        archive
            .read_entry(&beyond)
            .expect_err("an extent outside the archive is refused")
            .code(),
        "out_of_bounds"
    );

    let mut overflowing = member.clone();
    overflowing.offset = u64::MAX - 1;
    assert_eq!(
        archive
            .read_entry(&overflowing)
            .expect_err("an extent that overflows the 64-bit range is refused")
            .code(),
        "out_of_bounds"
    );
}

#[test]
fn accept_f04_d_order_reader_a_key_of_another_namespace_is_not_a_reader_lookup() {
    let tree = TempTree::new("reader-wrong-key");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    let wrong = AssetKey::from_spelling("install", "targets.zrd", "default").expect("a valid key");
    assert!(matches!(
        mounts.resolve(&fixture_context("mp1"), &wrong),
        Err(ReaderLookupError::ForeignNamespace { .. })
    ));
}

#[test]
fn accept_f04_d_order_reader_a_request_that_specializes_a_variant_is_refused() {
    // A reader archive declares one copy of each name, indexed at the default
    // variant, so a request that asks for another variant asks for bytes no
    // mounted archive holds. Answering it with the default copy would be
    // different bytes than the caller asked for, so it is refused instead — and
    // refused before the archives are searched, never served by a wrong one.
    let tree = TempTree::new("reader-variant");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-root", "zbd/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));

    let specialized = AssetKey::from_spelling(READER_NAMESPACE, "targets.zrd", "night")
        .expect("a valid reader key");
    match mounts.resolve(&fixture_context("mp1"), &specialized) {
        Err(ReaderLookupError::ForeignVariant { requested, variant }) => {
            assert_eq!(*requested, specialized);
            assert_eq!(variant, "night");
        }
        other => panic!("a specialized variant is not served by the default copy: {other:?}"),
    }

    // The same name without the specialization is served, so the refusal is
    // about the variant and not about the member.
    let plain = AssetKey::from_spelling(READER_NAMESPACE, "targets.zrd", "default")
        .expect("a valid reader key");
    let resolution = mounts
        .resolve(&fixture_context("mp1"), &plain)
        .expect("the default variant resolves");
    assert_eq!(mounts.read(&resolution).expect("reads"), b"root targets");
}

#[test]
fn accept_f04_d_order_reader_two_archives_under_one_mount_id_are_refused() {
    // `ReaderMounts::read` finds the archive by mount id, so two archives
    // registered under one id leave the bytes a resolution points at
    // unattributable. Picking the first would silently answer with one
    // archive's bytes as another's.
    let tree = TempTree::new("reader-duplicate-id");
    let mut mounts = ReaderMounts::new();
    mounts.push(mount(
        &tree,
        "zbd/zrdr.zbd",
        ReaderLevel::Root,
        fixture_builder("reader-shared-id", "zbd/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"root targets")]),
    ));
    let first = mounts
        .resolve(&fixture_context("mp1"), &key("targets.zrd"))
        .expect("the archive holds the name");
    assert_eq!(
        mounts.read(&first).expect("one archive under the id reads"),
        b"root targets"
    );

    mounts.push(mount(
        &tree,
        "zbd/c1c/zrdr.zbd",
        ReaderLevel::World,
        fixture_builder("reader-shared-id", "zbd/c1c/zrdr.zbd", None),
        &reader_archive(&[("targets.zrd", b"world targets")]),
    ));
    assert_eq!(
        mounts.archives_named(&first.mount),
        2,
        "the second archive reused the mount id the resolution names"
    );
    assert_eq!(
        mounts
            .read(&first)
            .expect_err("the bytes cannot be attributed to one archive")
            .code(),
        "ambiguous_archive"
    );
}

#[test]
fn accept_f04_d_order_reader_the_designed_precedence_status_is_untouched() {
    // The reader order is a **separate** order from the designed precedence
    // order, and nothing here may promote either: the original order is read
    // off the executable, not measured from a run.
    assert_eq!(PRECEDENCE_ORDER_STATUS, ClaimStatus::Designed);
    assert_ne!(
        READER_LOOKUP_ORDER_STATUS, PRECEDENCE_ORDER_STATUS,
        "the two orders are labelled separately"
    );
    assert_ne!(
        READER_LOOKUP_ORDER_STATUS,
        ClaimStatus::VerifiedOriginal,
        "code-derived evidence is never verified_original"
    );
}

// ---------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The one production discovery of the installation this binary shares: it
/// walks and hashes the whole tree, so every retail test below reuses it.
fn installation() -> &'static Discovery {
    static FOUND: OnceLock<Discovery> = OnceLock::new();
    FOUND.get_or_init(|| {
        install::discover(&game_dir()).expect("production discovery reads the installation")
    })
}

/// The installation fingerprint every retail context resolves against.
fn install_sha256() -> ContentHash {
    install::fingerprint(&installation().manifest)
}

/// Mounts one retail reader archive by its installation spelling.
fn retail(
    id: &str,
    relative: &str,
    level: ReaderLevel,
    world_group: Option<&str>,
    mission: Option<&str>,
) -> ReaderArchive {
    let mut builder = builder(id, relative, PrecedenceClass::Shared).retail();
    if let Some(group) = world_group {
        builder = builder.with_world_group(WorldGroup::new(group).expect("a valid world group"));
    }
    if let Some(mission) = mission {
        builder = builder.with_mission(MissionScope::new(mission).expect("a valid mission scope"));
    }
    mount_reader_archive(builder, &game_dir().join(relative), level)
        .unwrap_or_else(|error| panic!("{relative} mounts as a {level} reader archive: {error}"))
}

/// A context loading `world_group`/`mission` of the owner's installation.
fn retail_context(world_group: &str, mission: &str) -> ResolveContext {
    ResolveContext::new(install_sha256())
        .with_world_group(WorldGroup::new(world_group).expect("a valid world group"))
        .with_mission(MissionScope::new(mission).expect("a valid mission scope"))
}

/// The five cases section F of the finding measures, as
/// `(world group, world directory, mission directory, member)` in installation
/// spelling.
const SHADOWED: [(&str, &str, &str, &str); 5] = [
    ("zbd/c1c", "ZBD/C1C", "ZBD/C1C/IA1", "targets.zrd"),
    ("zbd/c1c", "ZBD/C1C", "ZBD/C1C/MP1", "targets.zrd"),
    ("zbd/c1c", "ZBD/C1C", "ZBD/C1C/MP3", "targets.zrd"),
    ("zbd/c2", "ZBD/C2", "ZBD/C2/M01", "security_destroy.zrd"),
    ("zbd/c3", "ZBD/C3", "ZBD/C3/M02", "fueltruck.zrd"),
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_reader_retail_mission_shadows_world_in_five_cases() {
    // The root archive is the same archive for all five cases, so it is read
    // and hashed once.
    let mut with_root = ReaderMounts::new();
    with_root.push(retail(
        "reader-root",
        "ZBD/zrdr.zbd",
        ReaderLevel::Root,
        None,
        None,
    ));
    let root = &with_root.archives()[0];

    let mut served_cases: Vec<String> = Vec::new();
    for (world_group, world_dir, mission_dir, member) in SHADOWED {
        let mission = mission_dir
            .rsplit('/')
            .next()
            .expect("a mission directory")
            .to_ascii_lowercase();
        let world_archive = retail(
            "reader-world",
            &format!("{world_dir}/zrdr.zbd"),
            ReaderLevel::World,
            Some(world_group),
            None,
        );
        let world_member = world_archive
            .member(member)
            .unwrap_or_else(|| panic!("{world_dir} holds {member}"))
            .sha256;
        let mission_archive = retail(
            "reader-mission",
            &format!("{mission_dir}/zrdr.zbd"),
            ReaderLevel::Mission,
            Some(world_group),
            Some(mission.as_str()),
        );
        let mission_member = mission_archive
            .member(member)
            .unwrap_or_else(|| panic!("{mission_dir} holds {member}"))
            .sha256;

        let mut mounts = ReaderMounts::new();
        mounts.push(mission_archive).push(world_archive);
        let context = retail_context(world_group, mission.as_str());
        let resolution = mounts
            .resolve(&context, &key(member))
            .unwrap_or_else(|error| panic!("{mission_dir} resolves {member}: {error}"));

        // The original mounts [root, mission, world] and serves the first
        // archive holding the name, so the mission copy wins over the world
        // copy — and the two hold different bytes, so the order decides.
        assert_eq!(
            resolution.level,
            ReaderLevel::Mission,
            "{mission_dir}: the mission archive shadows the world archive"
        );
        assert_eq!(
            resolution.span.member_sha256(),
            mission_member,
            "{mission_dir}: the mission copy is served"
        );
        assert_ne!(
            mission_member, world_member,
            "{mission_dir}: the shadowed copies differ in bytes"
        );
        assert_eq!(
            resolution.trace.attempts[0].outcome,
            ReaderAttemptOutcome::Selected {
                entry_index: resolution.entry_index
            },
            "{mission_dir}: the mission archive is searched before the world archive"
        );
        assert!(
            resolution
                .trace
                .attempts
                .iter()
                .any(|attempt| matches!(attempt.outcome, ReaderAttemptOutcome::Shadowed { .. })),
            "{mission_dir}: the world copy is reported shadowed"
        );

        // The world copy alone would serve: this is a shadowing decision, not
        // an absent member.
        let mut world_only = ReaderMounts::new();
        world_only.push(retail(
            "reader-world-alone",
            &format!("{world_dir}/zrdr.zbd"),
            ReaderLevel::World,
            Some(world_group),
            None,
        ));
        let world_resolution = world_only
            .resolve(&context, &key(member))
            .expect("the world archive alone still holds the name");
        assert_eq!(world_resolution.span.member_sha256(), world_member);

        served_cases.push(format!("{mission_dir}/{member}"));
    }
    assert_eq!(
        served_cases,
        vec![
            "ZBD/C1C/IA1/targets.zrd",
            "ZBD/C1C/MP1/targets.zrd",
            "ZBD/C1C/MP3/targets.zrd",
            "ZBD/C2/M01/security_destroy.zrd",
            "ZBD/C3/M02/fueltruck.zrd",
        ],
        "the five mission-over-world shadowing cases section F measures"
    );

    // The root archive holds none of these names: section F measures that no
    // world or mission member name occurs in the root archive at all, so
    // root-first never has to decide between two retail copies here — that
    // half of the order stays code-derived, and only the mission-over-world
    // half is pinned by data.
    for (_, _, _, member) in SHADOWED {
        assert!(
            root.member(member).is_none(),
            "the root archive does not hold {member}"
        );
    }
    assert_eq!(
        root.len(),
        221,
        "the root archive declares 221 entries, 220 of them reachable by name"
    );
    assert_eq!(
        root.mount().member_count(),
        220,
        "one name is declared twice and only the first entry is mountable"
    );
    assert!(
        with_root
            .resolve(&retail_context("zbd/c1c", "mp1"), &key("soils.zrd"))
            .is_ok(),
        "a root-only member still resolves for a mission context"
    );
    assert!(
        root.member("objectives.zrd").is_none(),
        "objectives.zrd is a per-mission member, not a root one"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_order_reader_retail_root_player_is_served_by_its_first_entry() {
    // The root archive declares `player.zrd` twice. The original's scan serves
    // the first entry, so entry 22 is the copy a lookup returns and entry 100
    // is unreachable by name — while both stay inventory rows with their own
    // extents and digests.
    let mut mounts = ReaderMounts::new();
    mounts.push(retail(
        "reader-root",
        "ZBD/zrdr.zbd",
        ReaderLevel::Root,
        None,
        None,
    ));
    let archive = &mounts.archives()[0];

    let positions: Vec<usize> = archive
        .members()
        .iter()
        .filter(|member| member.name.as_deref() == Some("player.zrd"))
        .map(|member| member.entry_index)
        .collect();
    assert_eq!(positions, vec![22, 100], "player.zrd entry positions");

    let first = archive.member_at(22).expect("entry 22 exists");
    assert!(first.reachable_by_name(), "the first entry serves its name");
    assert_eq!(first.length, 3414);
    assert!(
        first
            .sha256
            .expect("a reachable member was hashed")
            .to_hex()
            .starts_with("a8cc7547"),
        "entry 22 is the 3414-byte player document"
    );

    let second = archive.member_at(100).expect("entry 100 exists");
    assert_eq!(
        second.unreachable,
        Some(Unreachable::DuplicateName { served_by: 22 }),
        "the later entry of one name is unreachable by name"
    );
    assert_eq!(second.length, 34711);
    assert_ne!(
        second.sha256, first.sha256,
        "the two copies are different documents"
    );
    assert_eq!(
        archive.mount().member_count(),
        archive.len() - 1,
        "only the entry a name can reach becomes a mount member"
    );
    assert_eq!(
        archive.unreachable().count(),
        1,
        "the root archive declares exactly one unreachable member"
    );

    let context = retail_context("zbd/c1c", "mp1");
    let resolution = mounts
        .resolve(&context, &key("player.zrd"))
        .expect("player.zrd resolves for any context");
    assert_eq!(
        resolution.entry_index, 22,
        "the first entry is served by name"
    );
    assert_eq!(resolution.span.offset(), first.offset);
    assert_eq!(resolution.span.length(), 3414);
    assert_eq!(resolution.span.member_sha256(), first.sha256);
    assert_eq!(resolution.span.install_sha256(), install_sha256());

    let bytes = mounts.read(&resolution).expect("the member reads");
    assert_eq!(bytes.len() as u64, 3414);
    assert_eq!(
        Some(sha256(&bytes)),
        first.sha256,
        "the bytes read back are the bytes the span names"
    );
    assert_ne!(
        Some(sha256(&bytes)),
        second.sha256,
        "the unreachable entry's bytes are never served by name"
    );
}
// ------------------------------------------------------ evidence harness ---

/// The measured reader-member lookup over the whole installation: every
/// `zrdr.zbd`, its member rows, the names no name can reach, and the five
/// cases a mission archive shadows a world archive in. Names, lengths and
/// hashes only — never original bytes.
struct Survey {
    install_sha256: String,
    content_sha256: String,
    archives: Vec<ArchiveRow>,
}

struct ArchiveRow {
    container: String,
    level: ReaderLevel,
    world_group: Option<String>,
    mission: Option<String>,
    declared: usize,
    reachable: usize,
    unreachable: Vec<(usize, String, String)>,
    /// The digests of the members a name reaches, by lower-cased name.
    members: Vec<(String, String)>,
}

/// Mounts every reader archive the installation declares and records what each
/// one holds. Production discovery supplies the archive list, and every
/// archive is mounted through [`mount_reader_archive`].
fn survey() -> Survey {
    let found = installation();
    let manifest = &found.manifest;
    let mut archives = Vec::new();
    for record in &manifest.files {
        let relative = record.relative_spelling.as_str();
        if !relative.to_ascii_lowercase().ends_with("/zrdr.zbd") {
            continue;
        }
        // `ZBD/zrdr.zbd` is the root archive, `ZBD/<group>/zrdr.zbd` the
        // world's and `ZBD/<group>/<mission>/zrdr.zbd` the mission's; the
        // original mounts no other reader archive (finding section A).
        let components: Vec<&str> = relative.split('/').collect();
        let level = match components.as_slice() {
            ["ZBD", "zrdr.zbd"] => ReaderLevel::Root,
            ["ZBD", _, "zrdr.zbd"] => ReaderLevel::World,
            ["ZBD", _, _, "zrdr.zbd"] => ReaderLevel::Mission,
            other => panic!("{relative} is not a reader archive path: {other:?}"),
        };
        let world_group = (level != ReaderLevel::Root).then(|| {
            WorldGroup::new(&format!("zbd/{}", components[1]))
                .expect("a valid world group")
                .logical_key()
        });
        let mission = (level == ReaderLevel::Mission).then(|| components[2].to_ascii_lowercase());
        let archive = retail(
            &format!("reader-{}", archives.len()),
            relative,
            level,
            world_group.as_deref(),
            mission.as_deref(),
        );
        archives.push(ArchiveRow {
            container: relative.to_owned(),
            level,
            world_group,
            mission,
            declared: archive.len(),
            reachable: archive.mount().member_count(),
            unreachable: archive
                .unreachable()
                .map(|member| {
                    (
                        member.entry_index,
                        member
                            .name
                            .clone()
                            .unwrap_or_else(|| "<non-utf8>".to_owned()),
                        member
                            .unreachable
                            .map(|reason| reason.label().to_owned())
                            .expect("an unreachable member has a reason"),
                    )
                })
                .collect(),
            members: archive
                .members()
                .iter()
                .filter(|member| member.reachable_by_name())
                .filter_map(|member| {
                    Some((
                        member.name.as_deref()?.to_ascii_lowercase(),
                        member.sha256?.to_hex(),
                    ))
                })
                .collect(),
        });
    }
    archives.sort_by(|left, right| left.container.cmp(&right.container));
    Survey {
        install_sha256: install::fingerprint(manifest).to_hex(),
        content_sha256: install::content_fingerprint(manifest).to_hex(),
        archives,
    }
}

/// The five cases section F measures, re-derived from the survey rather than
/// hardcoded: every mission archive that shadows a world archive of the same
/// world group, sorted by installation spelling.
fn shadowed_cases(survey: &Survey) -> Vec<(String, String, String)> {
    let mut cases: Vec<(String, String, String)> = Vec::new();
    for mission in survey
        .archives
        .iter()
        .filter(|archive| archive.level == ReaderLevel::Mission)
    {
        let Some(world) = survey.archives.iter().find(|archive| {
            archive.level == ReaderLevel::World && archive.world_group == mission.world_group
        }) else {
            continue;
        };
        let world_members: BTreeMap<&str, &str> = world
            .members
            .iter()
            .map(|(name, digest)| (name.as_str(), digest.as_str()))
            .collect();
        for (name, digest) in &mission.members {
            if let Some(world_digest) = world_members.get(name.as_str())
                && *world_digest != digest
            {
                cases.push((mission.container.clone(), name.clone(), jstr(world_digest)));
            }
        }
    }
    cases.sort();
    cases
}

/// What this task leaves unresolved, each naming the affected content and the
/// task that owns it (`docs/findings/2026-10-06-f04-d-reader-member-lookup.md`
/// section E). They are **recorded, not dropped**: `validate_evidence.py
/// --require-pass` rejects a report that lists them, and accepting that exit 3
/// is the point — an empty list would be a report that claims this stage closed
/// questions it did not close.
const UNKNOWNS: [&str; 5] = [
    "The original's loose-file override is not modelled: a loose file of the same basename that \
     is newer (CompareFileTime >= 1) wins over the archive copy. Affected content: every reader \
     (.zrd) member resolved through cs_assets::vfs::reader. Resolving task: #700 \
     F04-D-order-reader-loose. The only archive-side time candidate, the index entry's trailing \
     u64, stays unknown (#692).",
    "The original's loose-directory fallback is not modelled: only when no archive holds the name \
     does the original search the most recently added loose directory, then zbd. Affected content: \
     reader member lookups that find no archive copy. Resolving task: #700 \
     F04-D-order-reader-loose. No such loose directory exists in the owner's installation, so \
     retail data cannot exercise it.",
    "What a version-one index entry's u32 word (2 in all 1293 retail entries) and trailing u64 mean \
     stays unknown; cs_formats reads only start, length and name. Affected content: reader member \
     identity and the loose-override time candidate. Resolving task: #692.",
    "Which reader archives are mounted for a mission is not wired into SessionBuilder: the mission \
     level of ZBD/<world>/<mission>/zrdr.zbd is unbound, and whether this installation's archive set \
     is the campaign layout is unverified. Affected content: every reader resolution, which today \
     is mounted explicitly by the caller. Resolving task: #687 F04-D-order-archives.",
    "The lookup order itself (basename keys, [root, mission, world], first hit wins) is code-derived \
     from the original executable's code, never observed in a run of the original, so \
     READER_LOOKUP_ORDER_STATUS is `inferred` and PRECEDENCE_ORDER_STATUS stays `designed`. \
     Affected content: every reader resolution's order. Settling it against a run of the original \
     needs owner-supplied capture (REF-OWNER-FIRST-CAPTURE, #358) or further static work.",
];

/// The JSON artifact of the survey: one row per archive with its declared and
/// reachable member counts and the entries no name can reach.
fn reader_lookup_json(candidate_tree: &str, survey: &Survey) -> String {
    let archives: Vec<String> = survey
        .archives
        .iter()
        .map(|archive| {
            format!(
                "{{\"container\": {}, \"level\": {}, \"world_group\": {}, \"mission\": {}, \
                 \"declared_entries\": {}, \"reachable_members\": {}, \"unreachable\": [{}]}}",
                jstr(&archive.container),
                jstr(archive.level.label()),
                archive
                    .world_group
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), jstr),
                archive
                    .mission
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), jstr),
                archive.declared,
                archive.reachable,
                archive
                    .unreachable
                    .iter()
                    .map(|(index, name, reason)| format!(
                        "{{\"entry\": {index}, \"name\": {}, \"reason\": {}}}",
                        jstr(name),
                        jstr(reason)
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
        .collect();
    let cases: Vec<String> = shadowed_cases(survey)
        .iter()
        .map(|(container, name, world_digest)| {
            format!(
                "{{\"mission_archive\": {}, \"member\": {}, \"shadowed_world_sha256\": {world_digest}}}",
                jstr(container),
                jstr(name)
            )
        })
        .collect();
    format!(
        "{{\n\
         \x20\"task_id\": \"F04-D-order-reader-members\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"install_sha256\": {},\n\
         \x20\"content_sha256\": {},\n\
         \x20\"lookup\": \"reader member key = basename of the requested name; archives searched in the \
         original's mount order [root, mission, world]; first case-insensitive match wins; first \
         index entry inside the winning archive wins\",\n\
         \x20\"order_status\": \"inferred (read off the original executable, not a runtime capture)\",\n\
         \x20\"designed_precedence_order_status\": \"designed (PRECEDENCE_ORDER_STATUS, untouched)\",\n\
         \x20\"archives\": [\n  {}\n ],\n\
         \x20\"shadowed_cases\": [\n  {}\n ]\n\
         }}\n",
        jstr(candidate_tree),
        jstr(&iso_utc_now()),
        jstr(&survey.install_sha256),
        jstr(&survey.content_sha256),
        archives.join(",\n  "),
        cases.join(",\n  "),
    )
}

/// Evidence-report harness for task #685 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. `CS_EVIDENCE_REVIEWER` names the agent
/// that ran it and is recorded in the report; it is not baked in, because the
/// reviewer regenerates the report on the rebased commit. Run from the
/// workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    mkdir -p private/evidence/T685
///    cargo test --workspace --locked -- accept_f04_d_order_reader_ --include-ignored \
///      2>&1 | tee private/evidence/T685/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. with `pipefail`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T685 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f04_d_order_reader_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///    CS_EVIDENCE_REVIEWER="<agent running this harness>" \
///      cargo test --locked -p cs_assets --test accept_f04_d_order_reader_members -- evidence_report_t685 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T685/acceptance.json \
///      --artifact-root private/evidence/T685
///    ```
///    **Without** `--require-pass`: that flag rejects a report that still lists
///    unresolved issues, and this task's `unknowns` are exactly the limitations
///    its acceptance pins — the unmodelled loose-file override and loose
///    directory fallback (#700), the unexplained index bytes (#692), the
///    unbound mission level (#687) and the code-derived-only order. They are
///    recorded rather than dropped, so the flag exits 3 with "Unresolved
///    issues" and that is the expected result. `--require-pass` on this report
///    is a false green.
/// 4. Commit a copy of `acceptance.json` as
///    `docs/findings/evidence/T685.json`.
///
/// Every field is derived from real inputs: the recorded log, production
/// discovery of `$CS_GAME_DIR`, the production reader-mount survey of every
/// `zrdr.zbd` (`reader-lookup.json`: installation spellings, member counts and
/// hashes only), `rustc --version` and `Cargo.lock`. The `unknowns` are the
/// literal limitations of
/// `docs/findings/2026-10-06-f04-d-reader-member-lookup.md` section E.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t685_writes_the_acceptance_report() {
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
        format!("{PREFIX}retail_mission_shadows_world_in_five_cases"),
        format!("{PREFIX}retail_root_player_is_served_by_its_first_entry"),
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

    // The survey is production work: it mounts every declared reader archive
    // through `mount_reader_archive` and reads it back through `ReaderMounts`.
    let measured = survey();
    assert_eq!(
        measured.archives.len(),
        62,
        "every declared zrdr.zbd of the installation is mounted"
    );
    // Sorted by installation spelling, so this pins *which* archive shadows
    // which member and not the iteration order of the survey.
    assert_eq!(
        shadowed_cases(&measured)
            .iter()
            .map(|(container, name, _)| format!("{container}/{name}"))
            .collect::<Vec<_>>(),
        vec![
            "ZBD/C1C/IA1/zrdr.zbd/targets.zrd",
            "ZBD/C1C/MP1/zrdr.zbd/targets.zrd",
            "ZBD/C1C/MP3/zrdr.zbd/targets.zrd",
            "ZBD/C2/M01/zrdr.zbd/security_destroy.zrd",
            "ZBD/C3/M02/zrdr.zbd/fueltruck.zrd",
        ],
        "the five mission-over-world shadowing cases the acceptance suite pins, re-derived \
         from the production mounts of every declared reader archive"
    );
    let lookup_path = evidence_dir.join("reader-lookup.json");
    fs::write(&lookup_path, reader_lookup_json(&candidate_tree, &measured))
        .unwrap_or_else(|error| panic!("write {}: {error}", lookup_path.display()));
    let artifacts = [artifact(&log_path, "log"), artifact(&lookup_path, "json")];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F04-D-order-reader-members\",\n\
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
             reader-mount survey of all 62 declared zrdr.zbd archives, rustc and Cargo.lock. The \
             reader lookup order (basename keys, [root, mission, world], first match wins) is read \
             off the original executable's code, so it is `inferred`, not verified_original, and \
             PRECEDENCE_ORDER_STATUS stays `designed`. The `unknowns` array is deliberately \
             non-empty and names the affected content and its resolving task for each limitation, \
             so this report must be validated WITHOUT --require-pass: that flag rejects a report \
             with unresolved issues and would only be green if they had been dropped. The claim is \
             `implemented`, never `verified_original` or `release_approved`."
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

// --------------------------------------------------------- harness utils ---

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
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = era * 400 + year_of_era + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}
