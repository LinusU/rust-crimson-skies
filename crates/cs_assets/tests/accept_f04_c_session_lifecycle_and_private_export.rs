//! Acceptance scenario F04-C (AC03): malicious ZIP/ROF member names cannot
//! leave a private export directory — plus the session mount lifecycle the
//! export and every read go through (non-negotiable behavior 4).
//!
//! Every tree is newly authored fixture bytes under the system temporary
//! directory (`common::TempTree`), never original content. The tests call
//! production code only: `cs_assets::install::discover`,
//! `cs_assets::vfs::SessionBuilder`/`ContentSession`, `PendingRead`,
//! `ExportDirectory` and `export_asset`.
//!
//! Removing the behavior makes them fail: joining a raw member name to the
//! export root writes outside it; following a link planted in the export
//! tree writes outside it; dropping the mount containment check exports
//! into the installation; dropping the generation check lets a replaced
//! session's texture be read or delivered by its successor; holding a
//! borrowed mount instead of an owned one would not let a pending read
//! finish after close (and would not compile).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use common::TempTree;
use cs_assets::install::{self, sha256};
use cs_assets::vfs::{
    AttemptOutcome, ContentSession, ExportDirectory, ExportError, MountBuilder, ReadError,
    SessionBuilder, SessionError, SkipReason, SourceError, UnsafeName, WORLD_NAMESPACE,
    export_asset, export_components,
};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::install::RelativePathError;

/// Archive member names a hostile ZIP or ROF could carry, each with the
/// rule that must refuse it.
fn hostile_names() -> Vec<(&'static str, UnsafeName)> {
    vec![
        (
            "../escape.dds",
            UnsafeName::Path(RelativePathError::ParentComponent),
        ),
        (
            "..\\escape.dds",
            UnsafeName::Path(RelativePathError::ParentComponent),
        ),
        (
            "hud/../../escape.dds",
            UnsafeName::Path(RelativePathError::ParentComponent),
        ),
        (
            "hud\\..\\..\\escape.dds",
            UnsafeName::Path(RelativePathError::ParentComponent),
        ),
        (
            "/tmp/escape.dds",
            UnsafeName::Path(RelativePathError::Absolute),
        ),
        (
            "\\escape.dds",
            UnsafeName::Path(RelativePathError::Absolute),
        ),
        (
            "\\\\server\\share\\escape.dds",
            UnsafeName::Path(RelativePathError::Absolute),
        ),
        (
            "C:\\escape.dds",
            UnsafeName::Path(RelativePathError::Absolute),
        ),
        (
            "C:escape.dds",
            UnsafeName::Path(RelativePathError::Absolute),
        ),
        (
            "hud//escape.dds",
            UnsafeName::Path(RelativePathError::EmptyComponent),
        ),
        (
            "hud/./escape.dds",
            UnsafeName::Path(RelativePathError::CurrentComponent),
        ),
        ("hud/", UnsafeName::Path(RelativePathError::EmptyComponent)),
        (
            "esc\0ape.dds",
            UnsafeName::Path(RelativePathError::InteriorNul),
        ),
        ("", UnsafeName::Path(RelativePathError::Empty)),
        ("hud/C:escape.dds", UnsafeName::Colon),
        ("escape.dds:stream", UnsafeName::Colon),
        (".. /escape.dds", UnsafeName::TrailingDotOrSpace),
        ("hud/.../escape.dds", UnsafeName::TrailingDotOrSpace),
        ("escape.dds.", UnsafeName::TrailingDotOrSpace),
        ("hud\u{1}/escape.dds", UnsafeName::ControlCharacter),
        ("NUL.dds", UnsafeName::DeviceName),
        ("hud/con", UnsafeName::DeviceName),
        ("Com1.tga", UnsafeName::DeviceName),
        ("lpt9", UnsafeName::DeviceName),
        ("hud/COM\u{b9}.dds", UnsafeName::DeviceName),
        ("lpt\u{b3}", UnsafeName::DeviceName),
        ("CONIN$", UnsafeName::DeviceName),
        ("conout$.txt", UnsafeName::DeviceName),
    ]
}

/// Every path below `root`, relative, with the bytes of each file.
fn snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(root: &Path, directory: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for entry in fs::read_dir(directory).expect("fixture directory is listable") {
            let entry = entry.expect("fixture entry is readable");
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("below the root")
                .to_string_lossy()
                .into_owned();
            let file_type = entry.file_type().expect("fixture file type");
            if file_type.is_dir() {
                out.insert(relative, None);
                walk(root, &path, out);
            } else if file_type.is_symlink() {
                out.insert(relative, Some(b"<link>".to_vec()));
            } else {
                out.insert(relative, Some(fs::read(&path).expect("fixture bytes")));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// A session with no mounts, for export checks that need no bytes.
fn empty_session() -> ContentSession {
    SessionBuilder::new(ResolveContext::new(sha256(b"no installation"))).open()
}

/// A fixture installation with two world groups that each hold a
/// `texture.zbd` of different bytes.
fn two_world_install() -> TempTree {
    let tree = TempTree::new("f04-c-install");
    tree.write("ZBD/c1/texture.zbd", b"world one texture archive bytes");
    tree.write(
        "ZBD/c2/texture.zbd",
        b"world two texture archive, other bytes",
    );
    tree.write("ZBD/planes.zbd", b"shared planes bytes");
    tree
}

/// Opens a session of `tree` for `world` with the production layout.
fn world_session(tree: &TempTree, world: &str) -> ContentSession {
    let found = install::discover(tree.root()).expect("fixture installation is discovered");
    let context = ResolveContext::new(install::fingerprint(&found.manifest))
        .with_world_group(WorldGroup::new(world).expect("fixture world is valid"));
    let mut builder = SessionBuilder::new(context);
    builder
        .mount_installation(tree.root(), &found.diagnosis)
        .expect("fixture installation mounts");
    builder.open()
}

fn world_key(path: &str) -> AssetKey {
    AssetKey::from_spelling(WORLD_NAMESPACE, path, "default").expect("fixture key is valid")
}

/// AC03: every hostile member name is refused before any byte is written,
/// with the rule that refused it, and nothing appears inside or outside
/// the export directory. The names `RelativePath` covers are also refused
/// where an archive index would be mounted.
#[test]
fn accept_f04_c_malicious_archive_names_cannot_leave_export_directory() {
    let sandbox = TempTree::new("f04-c-sandbox");
    fs::create_dir(sandbox.root().join("export")).expect("export dir is created");
    fs::create_dir(sandbox.root().join("hud")).expect("sibling dir is created");
    let before = snapshot(sandbox.root());

    let session = empty_session();
    let export = ExportDirectory::open(&sandbox.root().join("export"), &session)
        .expect("a private directory outside every mount is usable");

    for (name, expected) in hostile_names() {
        match export.write(name, b"hostile payload") {
            Err(ExportError::UnsafeName {
                name: refused,
                reason,
            }) => {
                assert_eq!(refused, name, "the error quotes the name as found");
                assert_eq!(reason, expected, "{name:?} is refused by the right rule");
            }
            other => panic!("{name:?} must be refused as unsafe, got {other:?}"),
        }
        assert!(
            export_components(name).is_err(),
            "{name:?} has no export components"
        );
        if let UnsafeName::Path(_) = expected {
            let mut index = MountBuilder::new(
                MountId::new("archive").expect("valid"),
                MountNamespace::new("content").expect("valid"),
                PrecedenceClass::Shared,
                "data/hostile.zip",
            );
            assert!(
                index.add_member(name, 1, 0, None).is_err(),
                "{name:?} is refused as an archive member too"
            );
        }
    }

    assert_eq!(
        snapshot(sandbox.root()),
        before,
        "no hostile name created, changed or removed anything"
    );

    // The same guard writes a harmless nested name, and only below the
    // export root.
    let written = export
        .write("Hud\\Alert.dds", b"harmless bytes")
        .expect("a harmless nested name is exported");
    assert!(written.path.starts_with(export.root()));
    assert_eq!(
        fs::read(export.root().join("Hud").join("Alert.dds")).expect("export is readable"),
        b"harmless bytes"
    );
    assert_eq!(written.sha256, sha256(b"harmless bytes"));
    assert_eq!(written.size_bytes, 14);
    // Exports never overwrite, so a second member folding onto the same
    // host name cannot silently replace the first.
    assert!(matches!(
        export.write("Hud/Alert.dds", b"other bytes"),
        Err(ExportError::TargetExists { .. })
    ));
    assert_eq!(
        fs::read(export.root().join("Hud").join("Alert.dds")).expect("export is readable"),
        b"harmless bytes"
    );
}

/// AC03 through links: a symbolic link planted inside the export tree —
/// as a directory or as the target file — is refused, never followed, so
/// a legitimate name still cannot write outside the export directory.
#[cfg(unix)]
#[test]
fn accept_f04_c_link_planted_in_export_tree_is_not_followed() {
    use std::os::unix::fs::symlink;

    let sandbox = TempTree::new("f04-c-links");
    let export_root = sandbox.root().join("export");
    let outside = sandbox.root().join("outside");
    fs::create_dir(&export_root).expect("export dir is created");
    fs::create_dir(&outside).expect("outside dir is created");
    fs::write(outside.join("victim.dds"), b"victim bytes").expect("victim is written");
    symlink(&outside, export_root.join("hud")).expect("directory link is planted");
    symlink(outside.join("victim.dds"), export_root.join("alert.dds"))
        .expect("file link is planted");
    let outside_before = snapshot(&outside);

    let session = empty_session();
    let export = ExportDirectory::open(&export_root, &session).expect("export root is usable");

    assert!(matches!(
        export.write("hud/escape.dds", b"payload"),
        Err(ExportError::UnsafeExportTree { .. })
    ));
    assert!(matches!(
        export.write("HUD/escape.dds", b"payload"),
        Err(ExportError::UnsafeExportTree { .. }) | Ok(_)
    ));
    assert!(matches!(
        export.write("alert.dds", b"payload"),
        Err(ExportError::TargetExists { .. })
    ));
    assert_eq!(
        snapshot(&outside),
        outside_before,
        "nothing outside the export directory changed"
    );

    // An export root that is itself a link is refused.
    let linked_root = sandbox.root().join("linked-export");
    symlink(&outside, &linked_root).expect("root link is planted");
    assert!(matches!(
        ExportDirectory::open(&linked_root, &session),
        Err(ExportError::RootUnavailable { .. })
    ));
}

/// An export directory inside a mounted source — the installation — is
/// refused, so extracting can never write into original data.
#[test]
fn accept_f04_c_export_root_inside_a_mount_is_refused() {
    let tree = two_world_install();
    fs::create_dir(tree.root().join("private-export")).expect("dir is created");
    let before = snapshot(tree.root());
    let session = world_session(&tree, "zbd/c1");

    for root in [
        tree.root().to_path_buf(),
        tree.root().join("private-export"),
    ] {
        match ExportDirectory::open(&root, &session) {
            Err(ExportError::RootInsideMount { mount, .. }) => assert_eq!(mount, "install"),
            other => panic!("{} must be refused, got {other:?}", root.display()),
        }
    }
    let asset = session
        .resolve(&world_key("texture.zbd"))
        .expect("world texture resolves");
    // The world mount's own directory is refused as well.
    match ExportDirectory::open(&tree.root().join("ZBD").join("c1"), &session) {
        Err(ExportError::RootInsideMount { .. }) => {}
        other => panic!("a world directory must be refused, got {other:?}"),
    }
    drop(asset);
    assert_eq!(
        snapshot(tree.root()),
        before,
        "the installation is unchanged"
    );
}

/// An export root that *contains* the installation is usable, but a name
/// that would descend into the installation is refused, so extracting
/// still never writes into original data.
#[test]
fn accept_f04_c_export_never_descends_into_a_mount_below_the_root() {
    let sandbox = TempTree::new("f04-c-parent");
    sandbox.write("install/ZBD/c1/texture.zbd", b"world one texture bytes");
    sandbox.write("install/ZBD/c2/texture.zbd", b"world two texture bytes");
    let install_root = sandbox.root().join("install");
    let before = snapshot(&install_root);

    let found = install::discover(&install_root).expect("fixture installation is discovered");
    let mut builder =
        SessionBuilder::new(ResolveContext::new(install::fingerprint(&found.manifest)));
    builder
        .mount_installation(&install_root, &found.diagnosis)
        .expect("fixture installation mounts");
    let session = builder.open();
    let export =
        ExportDirectory::open(sandbox.root(), &session).expect("a parent of a mount is usable");

    for name in [
        "install/escape.dds",
        "install/ZBD/escape.dds",
        "install/ZBD/c1/escape.dds",
    ] {
        match export.write(name, b"hostile payload") {
            Err(ExportError::TargetInsideMount { mount, .. }) => assert_eq!(mount, "install"),
            other => panic!("{name:?} must not descend into the mount, got {other:?}"),
        }
    }
    assert_eq!(
        snapshot(&install_root),
        before,
        "the installation is unchanged"
    );

    // A sibling of the installation below the same root is still exported.
    let written = export
        .write("notes/escape.dds", b"harmless bytes")
        .expect("a name outside every mount is exported");
    assert!(written.path.starts_with(export.root()));
    assert!(
        !written
            .path
            .starts_with(fs::canonicalize(&install_root).expect("canonical"))
    );
}

/// The export consumer end to end: a resolved world texture is read
/// through its session (digest-checked) and written under its own member
/// spelling, with the bytes and digest of the selected world only.
#[test]
fn accept_f04_c_resolved_member_exports_its_own_world_bytes() {
    let tree = two_world_install();
    let before = snapshot(tree.root());
    let sandbox = TempTree::new("f04-c-export");

    for (world, bytes) in [
        ("zbd/c1", &b"world one texture archive bytes"[..]),
        ("zbd/c2", &b"world two texture archive, other bytes"[..]),
    ] {
        let session = world_session(&tree, world);
        let asset = session
            .resolve(&world_key("TEXTURE.ZBD"))
            .expect("each world resolves its own texture");
        assert_eq!(asset.resolved().span.member_sha256(), Some(sha256(bytes)));

        let target = sandbox.root().join(world.replace('/', "-"));
        fs::create_dir(&target).expect("per-world export dir is created");
        let directory = ExportDirectory::open(&target, &session).expect("export dir is usable");
        let exported = export_asset(&session, &asset, &directory).expect("the member exports");
        assert_eq!(exported.path, directory.root().join("texture.zbd"));
        assert_eq!(fs::read(&exported.path).expect("export is readable"), bytes);
        assert_eq!(exported.sha256, sha256(bytes));
        session.close();
    }
    assert_eq!(
        snapshot(tree.root()),
        before,
        "the installation is unchanged"
    );
}

/// Non-negotiable behavior 4 across a world switch: after the `c1` session
/// closes and `c2` opens, a `c1` asset cannot be read through `c2`, and a
/// read `c1` issued before closing still completes from its own owned
/// mount description — but its bytes are refused by `c2`.
#[test]
fn accept_f04_c_world_switch_never_reuses_previous_session_texture() {
    let tree = two_world_install();
    let key = world_key("texture.zbd");

    let first = world_session(&tree, "zbd/c1");
    let first_asset = first.resolve(&key).expect("c1 resolves");
    let pending = first.begin_read(&first_asset).expect("c1 issues a read");
    let first_generation = first.generation();
    let teardown = first.close();
    assert_eq!(teardown.generation, first_generation);
    let released: Vec<&str> = teardown.released.iter().map(MountId::as_str).collect();
    assert_eq!(released, ["install", "world-0", "world-1"]);

    let second = world_session(&tree, "zbd/c2");
    assert_ne!(second.generation(), first_generation);

    match second.read_all(&first_asset) {
        Err(ReadError::ForeignSession { session, issued_by }) => {
            assert_eq!(session, second.generation().get());
            assert_eq!(issued_by, first_generation.get());
        }
        other => panic!("a c1 asset must not be read through c2, got {other:?}"),
    }
    assert!(matches!(
        second.read_range(&first_asset, 0, 4),
        Err(ReadError::ForeignSession { .. })
    ));
    assert!(matches!(
        second.begin_read(&first_asset),
        Err(ReadError::ForeignSession { .. })
    ));

    let completed = pending
        .complete()
        .expect("an in-flight read survives its session closing");
    assert_eq!(completed.generation(), first_generation);
    assert_eq!(completed.key(), &key);
    assert!(matches!(
        second.accept(completed),
        Err(ReadError::ForeignSession { .. })
    ));

    let own = second.resolve(&key).expect("c2 resolves its own texture");
    let bytes = second
        .accept(
            second
                .begin_read(&own)
                .expect("c2 issues a read")
                .complete()
                .expect("read"),
        )
        .expect("c2 accepts its own read");
    assert_eq!(bytes, b"world two texture archive, other bytes");
    assert_eq!(
        second.read_all(&own).expect("c2 reads its own asset"),
        bytes
    );
}

/// A pending read re-checks the bytes when it completes: a member changed
/// on disk after the session closed is an error, never stale bytes.
#[test]
fn accept_f04_c_pending_read_after_close_refuses_changed_bytes() {
    let tree = two_world_install();
    let session = world_session(&tree, "zbd/c1");
    let asset = session
        .resolve(&world_key("texture.zbd"))
        .expect("c1 resolves");
    let pending = session.begin_read(&asset).expect("read is issued");
    session.close();

    tree.edit_byte("ZBD/c1/texture.zbd", 3, 0x20);
    match pending.complete() {
        Err(ReadError::DigestMismatch { mounted, found, .. }) => {
            assert_eq!(mounted, sha256(b"world one texture archive bytes"));
            assert_ne!(found, mounted);
        }
        other => panic!("a changed member must be refused, got {other:?}"),
    }
}

/// Resolve tracing inside a session: the world lookup names the selected
/// world mount and the other world as skipped for its scope, and reports
/// the designed precedence status.
#[test]
fn accept_f04_c_session_trace_names_every_world_attempt() {
    let tree = two_world_install();
    let session = world_session(&tree, "zbd/c2");
    let asset = session
        .resolve(&world_key("texture.zbd"))
        .expect("c2 resolves");
    let trace = &asset.resolved().trace;
    let attempts: Vec<(&str, &str, AttemptOutcome)> = trace
        .attempts
        .iter()
        .map(|attempt| {
            (
                attempt.mount.as_str(),
                attempt.container.as_str(),
                attempt.outcome.clone(),
            )
        })
        .collect();
    assert_eq!(
        attempts,
        [
            (
                "world-0",
                "ZBD/c1",
                AttemptOutcome::Skipped(SkipReason::ScopeMismatch)
            ),
            ("world-1", "ZBD/c2", AttemptOutcome::Selected),
        ]
    );
    assert_eq!(trace.precedence_status.label(), "designed");
    assert_eq!(asset.resolved().span.container_path(), "ZBD/c2");
    assert_eq!(asset.resolved().span.member_key(), Some("texture.zbd"));
    assert_eq!(
        asset.resolved().span.install_sha256(),
        session.context().installation
    );

    // The shared install mount answers installation-relative keys.
    let shared = session
        .resolve(&AssetKey::from_spelling("install", "zbd\\PLANES.zbd", "default").expect("key"))
        .expect("the shared mount answers");
    assert_eq!(shared.resolved().mount.as_str(), "install");
    assert_eq!(shared.resolved().span.member_key(), Some("ZBD/planes.zbd"));
}

/// Error propagation and retry: a failing mount is named and leaves the
/// mounts that succeeded in place; retrying that mount with a good root
/// succeeds; dropping the builder releases everything.
#[test]
fn accept_f04_c_failed_mount_is_named_and_can_be_retried() {
    let good = TempTree::new("f04-c-good");
    good.write("hud/alert.dds", b"shared alert");
    let missing = good.root().join("does-not-exist");

    let builder_for = |id: &str| {
        MountBuilder::new(
            MountId::new(id).expect("valid"),
            MountNamespace::new("content").expect("valid"),
            PrecedenceClass::Patch,
            "patch",
        )
    };
    let mut session = SessionBuilder::new(ResolveContext::new(sha256(b"fixture")));
    session
        .mount_directory(
            MountBuilder::new(
                MountId::new("shared").expect("valid"),
                MountNamespace::new("content").expect("valid"),
                PrecedenceClass::Shared,
                "shared",
            ),
            good.root(),
        )
        .expect("the good mount joins");

    match session.mount_directory(builder_for("patch"), &missing) {
        Err(SessionError::Source {
            mount,
            error: SourceError::RootUnavailable { path, .. },
        }) => {
            assert_eq!(mount.as_str(), "patch");
            assert_eq!(path, missing);
        }
        other => panic!("a missing root must fail by name, got {other:?}"),
    }
    assert_eq!(
        session.len(),
        1,
        "the good mount stays; nothing of the failed one"
    );

    let patch = TempTree::new("f04-c-patch");
    patch.write("HUD/Alert.dds", b"patched alert");
    session
        .mount_directory(builder_for("patch"), patch.root())
        .expect("the retry with a good root joins");
    assert!(matches!(
        session.mount_directory(builder_for("patch"), patch.root()),
        Err(SessionError::Mount(_))
    ));
    let session = session.open();
    let asset = session
        .resolve(&AssetKey::from_spelling("content", "hud/alert.dds", "default").expect("key"))
        .expect("the patch wins over the shared source");
    assert_eq!(asset.resolved().mount.as_str(), "patch");
    assert_eq!(session.read_all(&asset).expect("read"), b"patched alert");
}
