//! Acceptance scenario F04-D (AC04 plus the collision comparison):
//!
//! * cancel an asynchronous read during a world switch without
//!   use-after-free or cross-world texture reuse — the read runs on another
//!   thread, the world switch happens while it is between two chunks, and
//!   the cancel stops it with nothing delivered;
//! * every observed file-name collision is looked up member by member under
//!   every world context, and a retail lookup that only the unmeasured
//!   precedence order decides is blocked (spec F04 non-negotiable
//!   behavior 2).
//!
//! The synthetic tests use newly authored fixture bytes under the system
//! temporary directory (`common::TempTree`). The `retail_` tests read
//! `$CS_GAME_DIR` read-only and fail loudly without it; CI skips them.
//!
//! Every test calls production code: `install::discover`,
//! `SessionBuilder::mount_installation`, `ContentSession::resolve`,
//! `PendingRead::complete_with`/`cancel_handle`, `ContentSession::accept`
//! and `ContentSession::collision_report`. Removing the cancellation check,
//! the retail block, the generation check or the per-world scope makes
//! them fail.

mod common;

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use common::TempTree;
use cs_assets::install::{self, sha256};
use cs_assets::vfs::{
    CollisionReport, CollisionVerdict, ContentSession, LookupOutcome, MountBuilder,
    PENDING_READ_CHUNK, ReadError, ResolveError, SessionBuilder, WORLD_NAMESPACE,
};
use cs_types::asset_id::{
    AssetKey, ModId, ModStack, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::evidence::ClaimStatus;

// ------------------------------------------------------------ fixtures ---

/// Deterministic, newly authored bytes of `length`, distinct per `seed`.
fn authored(seed: u8, length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// A fixture installation shaped like the observed retail collisions:
/// `zrdr.zbd` at the `ZBD` root, world and mission level, and a
/// `texture.zbd` per world larger than one pending-read chunk.
fn collision_install() -> TempTree {
    let tree = TempTree::new("f04-d-install");
    let texture_length = (PENDING_READ_CHUNK * 3 + 17) as usize;
    tree.write("ZBD/c1/texture.zbd", &authored(1, texture_length));
    tree.write("ZBD/c2/texture.zbd", &authored(2, texture_length));
    tree.write("ZBD/zrdr.zbd", b"shared zrdr");
    tree.write("ZBD/c1/zrdr.zbd", b"world one zrdr");
    tree.write("ZBD/c1/m01/zrdr.zbd", b"world one mission one zrdr");
    tree.write("ZBD/c2/zrdr.zbd", b"world two zrdr");
    tree.write("ZBD/planes.zbd", b"shared planes");
    tree
}

fn world_session(root: &std::path::Path, world: Option<&str>) -> ContentSession {
    let found = install::discover(root).expect("installation is discovered");
    let mut context = ResolveContext::new(install::fingerprint(&found.manifest));
    if let Some(world) = world {
        context = context.with_world_group(WorldGroup::new(world).expect("world is valid"));
    }
    let mut builder = SessionBuilder::new(context);
    builder
        .mount_installation(root, &found.diagnosis)
        .expect("installation mounts");
    builder.open()
}

fn world_key(path: &str) -> AssetKey {
    AssetKey::from_spelling(WORLD_NAMESPACE, path, "default").expect("key is valid")
}

fn content_key(path: &str) -> AssetKey {
    AssetKey::from_spelling("content", path, "default").expect("key is valid")
}

fn content_mount(id: &str, class: PrecedenceClass) -> MountBuilder {
    MountBuilder::new(
        MountId::new(id).expect("valid"),
        MountNamespace::new("content").expect("valid"),
        class,
        id,
    )
}

// ------------------------------------------------------ AC04: cancel ---

/// What the reader thread observed.
enum ReaderEvent {
    FirstChunk { read: u64, total: u64 },
}

/// Issues a read of `world_from`'s `texture.zbd`, completes it on another
/// thread, and while that thread is parked after its first chunk closes
/// the session, opens `world_to` and cancels the read. Returns the
/// reader's result, the new session and the old texture digest.
fn cancel_during_world_switch(
    root: &std::path::Path,
    world_from: &str,
    world_to: &str,
) -> (
    Result<cs_assets::vfs::CompletedRead, ReadError>,
    ContentSession,
    Option<cs_types::evidence::ContentHash>,
) {
    let key = world_key("texture.zbd");
    let first = world_session(root, Some(world_from));
    let first_asset = first.resolve(&key).expect("the first world resolves");
    let first_digest = first_asset.resolved().span.member_sha256();
    let pending = first.begin_read(&first_asset).expect("the read is issued");
    let cancel = pending.cancel_handle();

    let (events, observed) = mpsc::channel();
    let (resume, resumed) = mpsc::channel::<()>();
    let reader = thread::spawn(move || {
        let mut first_chunk = true;
        pending.complete_with(move |progress| {
            if first_chunk {
                first_chunk = false;
                events
                    .send(ReaderEvent::FirstChunk {
                        read: progress.read,
                        total: progress.total,
                    })
                    .expect("the switching thread listens");
                resumed
                    .recv()
                    .expect("the switching thread resumes the reader");
            }
        })
    });

    let ReaderEvent::FirstChunk { read, total } = observed.recv().expect("the reader starts");
    assert_eq!(read, PENDING_READ_CHUNK, "the reader is between two chunks");
    assert!(total > read, "the read is still in flight");

    // The world switch, while the read is in flight: the old session is
    // torn down first, the new one opened, then the read is cancelled.
    let first_generation = first.generation();
    let teardown = first.close();
    assert_eq!(teardown.generation, first_generation);
    let second = world_session(root, Some(world_to));
    assert_ne!(second.generation(), first_generation);
    cancel.cancel();
    resume.send(()).expect("the reader is parked");

    let result = reader.join().expect("the reader does not panic");
    (result, second, first_digest)
}

/// AC04: the read is cancelled mid-flight during the world switch. It
/// stops at the next chunk boundary with `Cancelled`, delivers nothing,
/// and the new world resolves and reads its own texture.
#[test]
fn accept_f04_d_cancel_async_read_during_world_switch() {
    let tree = collision_install();
    let (result, second, first_digest) =
        cancel_during_world_switch(tree.root(), "zbd/c1", "zbd/c2");
    match result {
        Err(ReadError::Cancelled { mount, read, total }) => {
            assert_eq!(mount, "world-0");
            assert_eq!(
                read, PENDING_READ_CHUNK,
                "stopped at the next chunk boundary"
            );
            assert_eq!(total, PENDING_READ_CHUNK * 3 + 17);
        }
        other => panic!("a read cancelled during the world switch must stop, got {other:?}"),
    }

    let own = second
        .resolve(&world_key("texture.zbd"))
        .expect("c2 resolves");
    assert_eq!(own.resolved().span.container_path(), "ZBD/c2");
    assert_ne!(own.resolved().span.member_sha256(), first_digest);
    let bytes = second
        .accept(
            second
                .begin_read(&own)
                .expect("c2 issues its own read")
                .complete()
                .expect("c2 reads its own texture"),
        )
        .expect("c2 accepts its own read");
    assert_eq!(bytes, tree.read("ZBD/c2/texture.zbd"));
}

/// A read cancelled before it starts reads nothing at all.
#[test]
fn accept_f04_d_read_cancelled_before_it_starts_reads_nothing() {
    let tree = collision_install();
    let session = world_session(tree.root(), Some("zbd/c1"));
    let asset = session
        .resolve(&world_key("texture.zbd"))
        .expect("resolves");
    let pending = session.begin_read(&asset).expect("issued");
    pending.cancel_handle().cancel();
    let mut chunks = 0;
    match pending.complete_with(|_| chunks += 1) {
        Err(ReadError::Cancelled { read: 0, .. }) => {}
        other => panic!("a cancelled read must not start, got {other:?}"),
    }
    assert_eq!(chunks, 0);
}

/// Without a cancel, the read of the replaced world finishes on its own
/// mount description after the switch (no dangling state) — and the new
/// world refuses its bytes, so they are never reused.
#[test]
fn accept_f04_d_uncancelled_read_of_replaced_world_is_never_reused() {
    let tree = collision_install();
    let key = world_key("texture.zbd");
    let first = world_session(tree.root(), Some("zbd/c1"));
    let asset = first.resolve(&key).expect("c1 resolves");
    let pending = first.begin_read(&asset).expect("issued");
    let reader = thread::spawn(move || pending.complete());
    first.close();
    let second = world_session(tree.root(), Some("zbd/c2"));

    let completed = reader
        .join()
        .expect("the reader does not panic")
        .expect("the replaced world's read completes from its owned mount");
    assert!(matches!(
        second.accept(completed),
        Err(ReadError::ForeignSession { .. })
    ));
    assert!(matches!(
        second.read_all(&asset),
        Err(ReadError::ForeignSession { .. })
    ));
}

// ------------------------------------------- the unmeasured-order block ---

/// Spec F04 non-negotiable behavior 2: two retail sources holding
/// different bytes for one key, decided only by the designed order, are
/// blocked with both origins. Identical bytes, non-retail overlays and
/// opted-in mods are not blocked.
#[test]
fn accept_f04_d_retail_answer_decided_only_by_designed_order_is_blocked() {
    let shared = TempTree::new("f04-d-shared");
    shared.write("hud/alert.dds", b"shared alert");
    shared.write("hud/same.dds", b"same bytes");
    let patch = TempTree::new("f04-d-patch");
    patch.write("HUD/Alert.dds", b"patched alert");
    patch.write("hud/same.dds", b"same bytes");
    let installation = sha256(b"fixture installation");

    let retail_session = |mods: Option<(&TempTree, &str)>| {
        let mut context = ResolveContext::new(installation);
        let mut builder_mods = None;
        if let Some((tree, id)) = mods {
            let mod_id = ModId::new(id).expect("valid");
            context = context.with_mods(ModStack::new(vec![mod_id.clone()]).expect("valid"));
            builder_mods = Some((tree, mod_id));
        }
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_directory(
                content_mount("shared", PrecedenceClass::Shared).retail(),
                shared.root(),
            )
            .expect("shared mounts");
        builder
            .mount_directory(
                content_mount("patch", PrecedenceClass::Patch).retail(),
                patch.root(),
            )
            .expect("patch mounts");
        if let Some((tree, mod_id)) = builder_mods {
            builder
                .mount_directory(
                    content_mount("mod", PrecedenceClass::Mod).with_mod(mod_id),
                    tree.root(),
                )
                .expect("mod mounts");
        }
        builder.open()
    };

    let session = retail_session(None);
    match session.resolve(&content_key("hud/alert.dds")) {
        Err(ResolveError::UnmeasuredOrder {
            selected,
            shadowed,
            trace,
            ..
        }) => {
            assert_eq!(selected.mount.as_str(), "patch");
            assert_eq!(selected.member_spelling, "HUD/Alert.dds");
            assert_eq!(selected.sha256, Some(sha256(b"patched alert")));
            assert_eq!(shadowed.len(), 1);
            assert_eq!(shadowed[0].mount.as_str(), "shared");
            assert_eq!(shadowed[0].member_spelling, "hud/alert.dds");
            assert_eq!(shadowed[0].sha256, Some(sha256(b"shared alert")));
            assert_eq!(trace.precedence_status, ClaimStatus::Designed);
        }
        other => {
            panic!("a retail conflict decided by the designed order must block, got {other:?}")
        }
    }
    let error = session
        .resolve(&content_key("hud/alert.dds"))
        .expect_err("blocked");
    assert!(error.to_string().contains("designed"), "{error}");

    // Identical bytes: the order decides the origin, not the bytes.
    let same = session
        .resolve(&content_key("hud/same.dds"))
        .expect("identical copies are no conflict");
    assert_eq!(same.resolved().mount.as_str(), "patch");

    // An opted-in mod is the user's choice, not an original-order claim.
    let modded = TempTree::new("f04-d-mod");
    modded.write("hud/alert.dds", b"mod alert");
    let with_mod = retail_session(Some((&modded, "hudmod")));
    let asset = with_mod
        .resolve(&content_key("hud/alert.dds"))
        .expect("the opted-in mod serves");
    assert_eq!(asset.resolved().mount.as_str(), "mod");
    assert_eq!(with_mod.read_all(&asset).expect("read"), b"mod alert");

    // A non-retail overlay keeps the designed order.
    let mut builder = SessionBuilder::new(ResolveContext::new(installation));
    builder
        .mount_directory(
            content_mount("shared", PrecedenceClass::Shared).retail(),
            shared.root(),
        )
        .expect("shared mounts");
    builder
        .mount_directory(content_mount("patch", PrecedenceClass::Patch), patch.root())
        .expect("authored patch mounts");
    let authored = builder.open();
    assert_eq!(
        authored
            .resolve(&content_key("hud/alert.dds"))
            .expect("an authored overlay is not blocked")
            .resolved()
            .mount
            .as_str(),
        "patch"
    );
}

// ------------------------------------------------ collision comparison ---

fn contexts_for(session: &ContentSession, worlds: &[&str]) -> Vec<ResolveContext> {
    let installation = session.context().installation;
    let mut contexts = vec![ResolveContext::new(installation)];
    contexts.extend(worlds.iter().map(|world| {
        ResolveContext::new(installation)
            .with_world_group(WorldGroup::new(world).expect("world is valid"))
    }));
    contexts
}

/// Every name the fixture repeats is a collision group; each member is
/// looked up under every context, resolves to itself wherever its mount
/// is eligible, and is `not_eligible` elsewhere — never served by another
/// world's copy.
#[test]
fn accept_f04_d_collision_report_resolves_every_member_by_its_own_path() {
    let tree = collision_install();
    let session = world_session(tree.root(), None);
    let report = session.collision_report(&contexts_for(&session, &["zbd/c1", "zbd/c2"]));
    assert_eq!(report.precedence_status, ClaimStatus::Designed);

    let names: Vec<&str> = report
        .comparisons
        .iter()
        .map(|comparison| comparison.collision.file_name.as_str())
        .collect();
    assert_eq!(names, ["texture.zbd", "zrdr.zbd"], "planes.zbd is unique");

    let zrdr = &report.comparisons[1];
    // install: 4 spellings; world: c1 (2) + c2 (1).
    assert_eq!(zrdr.collision.members.len(), 7);
    assert_eq!(zrdr.collision.distinct_digests(), 4);
    assert_eq!(zrdr.verdict, CollisionVerdict::DistinctByPath);
    assert_eq!(zrdr.lookups.len(), 7 * 3);
    for lookup in &zrdr.lookups {
        let member = &zrdr.collision.members[lookup.member];
        let world_bound = member.scope.world_group.is_some();
        let context_world = report.contexts[lookup.context].world_group.as_ref();
        let expected = if !world_bound || context_world == member.scope.world_group.as_ref() {
            LookupOutcome::Own
        } else {
            LookupOutcome::NotEligible
        };
        assert_eq!(
            lookup.outcome, expected,
            "{} in {} under {context_world:?}",
            member.spelling, member.container
        );
    }
    assert_eq!(report.conflicting().count(), 0);
}

/// A collision the designed order would decide between different retail
/// bytes is reported as `conflicting`, with the blocked lookup.
#[test]
fn accept_f04_d_collision_report_flags_retail_shadowing() {
    let shared = TempTree::new("f04-d-report-shared");
    shared.write("hud/alert.dds", b"shared alert");
    let patch = TempTree::new("f04-d-report-patch");
    patch.write("hud/alert.dds", b"patched alert");
    let mut builder = SessionBuilder::new(ResolveContext::new(sha256(b"fixture")));
    builder
        .mount_directory(
            content_mount("shared", PrecedenceClass::Shared).retail(),
            shared.root(),
        )
        .expect("mounts");
    builder
        .mount_directory(
            content_mount("patch", PrecedenceClass::Patch).retail(),
            patch.root(),
        )
        .expect("mounts");
    let session = builder.open();
    let report: CollisionReport = session.collision_report(&[session.context().clone()]);
    assert_eq!(report.comparisons.len(), 1);
    let comparison = &report.comparisons[0];
    assert_eq!(comparison.collision.file_name, "alert.dds");
    assert_eq!(comparison.verdict, CollisionVerdict::Conflicting);
    assert!(comparison.lookups.iter().all(|lookup| matches!(
        &lookup.outcome,
        LookupOutcome::Blocked { selected, shadowed }
            if selected.mount.as_str() == "patch"
                && shadowed.len() == 1
                && shadowed[0].mount.as_str() == "shared"
    )));
    assert_eq!(report.conflicting().count(), 1);
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// Every file-name collision the retail installation contains, looked up
/// member by member under no world and under every discovered world group
/// through the production session layout: each eligible lookup resolves to
/// the member itself, nothing is ambiguous, blocked or served from another
/// world, and every world's `texture.zbd` stays its own.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_retail_every_observed_collision_resolves_by_path() {
    let root = game_dir();
    let found = install::discover(&root).expect("the installation is discovered");
    assert!(
        found.diagnosis.world_groups.len() >= 2,
        "a retail installation has several world groups"
    );
    let session = world_session(&root, None);
    assert!(session.rejected().is_empty(), "{:?}", session.rejected());
    let worlds: Vec<String> = found
        .diagnosis
        .world_groups
        .iter()
        .map(|group| group.as_str().to_owned())
        .collect();
    let world_refs: Vec<&str> = worlds.iter().map(String::as_str).collect();
    let report = session.collision_report(&contexts_for(&session, &world_refs));

    assert_eq!(report.precedence_status, ClaimStatus::Designed);
    assert!(!report.comparisons.is_empty(), "retail repeats file names");
    for name in ["texture.zbd", "zrdr.zbd"] {
        assert!(
            report
                .comparisons
                .iter()
                .any(|comparison| comparison.collision.file_name == name),
            "{name} is an observed collision"
        );
    }
    for comparison in &report.comparisons {
        assert_eq!(
            comparison.verdict,
            CollisionVerdict::DistinctByPath,
            "{}: {:?}",
            comparison.collision.file_name,
            comparison
                .lookups
                .iter()
                .filter(|lookup| !matches!(
                    lookup.outcome,
                    LookupOutcome::Own | LookupOutcome::NotEligible
                ))
                .collect::<Vec<_>>()
        );
        for member_index in 0..comparison.collision.members.len() {
            assert!(
                comparison
                    .lookups
                    .iter()
                    .any(|lookup| lookup.member == member_index
                        && lookup.outcome == LookupOutcome::Own),
                "every member is reachable by its own key under some context"
            );
        }
    }

    // AC01 on retail: under each world context, only that world's own
    // mount serves its `texture.zbd` (the `install` mount's copies are
    // other keys).
    let texture = report
        .comparisons
        .iter()
        .find(|comparison| comparison.collision.file_name == "texture.zbd")
        .expect("texture.zbd collides");
    for (context_index, context) in report.contexts.iter().enumerate().skip(1) {
        let world = context.world_group.as_ref().expect("world contexts");
        let served: Vec<&str> = texture
            .lookups
            .iter()
            .filter(|lookup| {
                lookup.context == context_index && lookup.outcome == LookupOutcome::Own
            })
            .map(|lookup| texture.collision.members[lookup.member].container.as_str())
            .filter(|container| *container != ".")
            .collect();
        assert!(
            served
                .iter()
                .all(|container| container.eq_ignore_ascii_case(world.as_relative().as_str())),
            "under {world} only its own world mount serves texture.zbd: {served:?}"
        );
    }
}

/// AC04 on retail: a `texture.zbd` read of the first world group is
/// cancelled mid-flight while the session switches to the second; the new
/// world resolves its own, different texture archive and reads it whole.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f04_d_retail_cancel_async_read_during_world_switch() {
    let root = game_dir();
    let found = install::discover(&root).expect("the installation is discovered");
    let groups = &found.diagnosis.world_groups;
    assert!(
        groups.len() >= 2,
        "a retail installation has several world groups"
    );
    let (result, second, first_digest) =
        cancel_during_world_switch(&root, groups[0].as_str(), groups[1].as_str());
    match result {
        Err(ReadError::Cancelled { read, total, .. }) => {
            assert_eq!(read, PENDING_READ_CHUNK);
            assert!(
                total > read,
                "the retail texture archive spans several chunks"
            );
        }
        other => panic!("the retail read must stop when cancelled, got {other:?}"),
    }
    let own = second
        .resolve(&world_key("texture.zbd"))
        .expect("the second world resolves its texture archive");
    assert!(
        own.resolved()
            .span
            .container_path()
            .eq_ignore_ascii_case(groups[1].as_str())
    );
    assert_ne!(
        own.resolved().span.member_sha256(),
        first_digest,
        "the two worlds hold different texture archives"
    );
    let bytes = second
        .accept(
            second
                .begin_read(&own)
                .expect("issued")
                .complete()
                .expect("the second world's texture reads whole and digest-checked"),
        )
        .expect("accepted by its own session");
    assert_eq!(bytes.len() as u64, own.resolved().span.length());
}
