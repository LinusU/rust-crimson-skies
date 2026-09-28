//! Acceptance scenario F04-B (AC02): a case-only collision at equal
//! priority fails with both origins — here over real mounted host
//! directories — plus the path validation and read path around it.
//!
//! Every tree is newly authored fixture bytes under the system temporary
//! directory (`common::TempTree`), never original content. The tests call
//! production code only: `cs_assets::vfs::mount_directory`, `Vfs::resolve`,
//! `Vfs::read_range` and `Vfs::read_all`.
//!
//! Removing the behavior makes them fail: a first-wins member map turns the
//! collisions into silent successes; following links mounts the outside
//! file; skipping the spelling check mounts `..\..\escape.dds`; dropping
//! the staleness, length or digest checks returns bytes that no longer
//! match the resolution.

mod common;

use std::fs;

use common::TempTree;
use cs_assets::install::sha256;
use cs_assets::vfs::{
    MountBuilder, MountError, ReadError, RejectReason, ResolveError, SourceError, Vfs,
    mount_directory,
};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::evidence::ContentHash;
use cs_types::install::RelativePathError;

const NAMESPACE: &str = "content";

fn install() -> ContentHash {
    ContentHash::from_bytes([0x5b; 32])
}

fn context() -> ResolveContext {
    ResolveContext::new(install())
}

fn key(path: &str) -> AssetKey {
    AssetKey::from_spelling(NAMESPACE, path, "default").expect("fixture key is valid")
}

fn builder(id: &str, container: &str, class: PrecedenceClass) -> MountBuilder {
    MountBuilder::new(
        MountId::new(id).expect("fixture mount id is valid"),
        MountNamespace::new(NAMESPACE).expect("fixture namespace is valid"),
        class,
        container,
    )
}

fn mount_tree(vfs: &mut Vfs, id: &str, container: &str, tree: &TempTree, class: PrecedenceClass) {
    let mounted =
        mount_directory(builder(id, container, class), tree.root()).expect("fixture tree mounts");
    assert!(mounted.rejected.is_empty(), "{:?}", mounted.rejected);
    vfs.mount(mounted.mount)
        .expect("fixture mount ids are unique");
}

/// AC02: two mounted directories at equal (shared) priority hold the same
/// logical path in different letter case; the lookup fails and names both
/// origins — mount, container, original spelling and the digest of the
/// bytes actually on disk.
#[test]
fn accept_f04_b_case_only_collision_at_equal_priority_fails_with_both_origins() {
    let first = TempTree::new("f04-b-upper");
    first.write("Textures/Alert.DDS", b"upper-case texture bytes");
    let second = TempTree::new("f04-b-lower");
    second.write("textures/alert.dds", b"lower-case texture bytes, longer");

    let mut vfs = Vfs::new();
    mount_tree(
        &mut vfs,
        "upper",
        "Data/Upper",
        &first,
        PrecedenceClass::Shared,
    );
    mount_tree(
        &mut vfs,
        "lower",
        "Data/lower",
        &second,
        PrecedenceClass::Shared,
    );

    let error = vfs
        .resolve(&context(), &key("TEXTURES\\ALERT.dds"))
        .expect_err("an equal-priority case-only collision must not resolve");
    let ResolveError::Ambiguous { candidates, .. } = &error else {
        panic!("expected an ambiguity, got {error}");
    };
    assert_eq!(candidates.len(), 2, "{error}");
    let upper = candidates
        .iter()
        .find(|origin| origin.mount.as_str() == "upper")
        .expect("the upper-case origin is named");
    let lower = candidates
        .iter()
        .find(|origin| origin.mount.as_str() == "lower")
        .expect("the lower-case origin is named");
    assert_eq!(upper.container, "Data/Upper");
    assert_eq!(upper.member_spelling, "Textures/Alert.DDS");
    assert_eq!(upper.sha256, Some(sha256(b"upper-case texture bytes")));
    assert_eq!(lower.container, "Data/lower");
    assert_eq!(lower.member_spelling, "textures/alert.dds");
    assert_eq!(
        lower.sha256,
        Some(sha256(b"lower-case texture bytes, longer"))
    );
    let rendered = error.to_string();
    assert!(rendered.contains("Textures/Alert.DDS"), "{rendered}");
    assert!(rendered.contains("textures/alert.dds"), "{rendered}");

    // A higher class breaks the tie by precedence, never by mount order.
    let patch = TempTree::new("f04-b-patch");
    patch.write("TEXTURES/ALERT.DDS", b"patched");
    mount_tree(
        &mut vfs,
        "patch",
        "Data/Patch",
        &patch,
        PrecedenceClass::Patch,
    );
    let resolved = vfs
        .resolve(&context(), &key("textures/alert.dds"))
        .expect("the patch overlay outranks both shared sources");
    assert_eq!(resolved.mount.as_str(), "patch");
    assert_eq!(resolved.span.member_key(), Some("TEXTURES/ALERT.DDS"));
    assert_eq!(
        vfs.read_all(&resolved).expect("patched bytes read"),
        b"patched"
    );
}

/// Inside one mounted directory, two host names that fold onto one key are
/// refused with both spellings. The separator case (`hud\x.dds` as a
/// literal Unix file name next to `hud/x.dds`) needs no case-sensitive
/// filesystem, so it runs on every Unix host.
#[cfg(unix)]
#[test]
fn accept_f04_b_separator_collision_inside_one_directory_names_both_spellings() {
    let tree = TempTree::new("f04-b-separator");
    tree.write("hud/x.dds", b"nested");
    fs::write(
        tree.root().join("hud\\x.dds"),
        b"flat name with a backslash",
    )
    .expect("a backslash is an ordinary Unix file-name byte");

    let error = mount_directory(
        builder("hud", "Data/Hud", PrecedenceClass::Shared),
        tree.root(),
    )
    .expect_err("two spellings of one key must not mount");
    let SourceError::Member {
        container,
        error:
            MountError::DuplicateMember {
                logical_key,
                first_spelling,
                second_spelling,
            },
    } = &error
    else {
        panic!("expected a duplicate-member refusal, got {error}");
    };
    assert_eq!(container, "Data/Hud");
    assert_eq!(logical_key, "default/hud/x.dds");
    let mut spellings = [first_spelling.as_str(), second_spelling.as_str()];
    spellings.sort_unstable();
    assert_eq!(spellings, ["hud/x.dds", "hud\\x.dds"]);
}

/// The letter-case form of the same refusal, where the host filesystem can
/// hold both names. On a case-insensitive host the second write lands on
/// the first file, which is checked so the test cannot pass vacuously.
#[test]
fn accept_f04_b_case_only_collision_inside_one_directory_names_both_spellings() {
    let tree = TempTree::new("f04-b-case");
    tree.write("Alert.dds", b"first");
    tree.write("ALERT.DDS", b"second");
    let names = fs::read_dir(tree.root())
        .expect("fixture root lists")
        .count();
    let result = mount_directory(
        builder("case", "Data/Case", PrecedenceClass::Shared),
        tree.root(),
    );
    if names == 1 {
        // Case-insensitive host: one file, one member, no collision exists.
        let mounted = result.expect("a single file mounts");
        assert_eq!(mounted.mount.member_count(), 1);
        return;
    }
    let error = result.expect_err("case-only duplicates must not mount");
    let SourceError::Member {
        error:
            MountError::DuplicateMember {
                first_spelling,
                second_spelling,
                ..
            },
        ..
    } = &error
    else {
        panic!("expected a duplicate-member refusal, got {error}");
    };
    assert_eq!(first_spelling, "ALERT.DDS", "walk order is sorted by bytes");
    assert_eq!(second_spelling, "Alert.dds");
}

/// A host file literally named with `..` components and backslashes cannot
/// become a member: the mount fails with the spelling that was found.
#[cfg(unix)]
#[test]
fn accept_f04_b_escaping_host_name_is_refused() {
    let tree = TempTree::new("f04-b-escape-name");
    tree.write("ok.dds", b"fine");
    fs::write(tree.root().join("..\\..\\escape.dds"), b"hostile")
        .expect("the hostile name is a valid Unix file name");

    let error = mount_directory(
        builder("hostile", "Data/Hostile", PrecedenceClass::Shared),
        tree.root(),
    )
    .expect_err("an escaping member name must not mount");
    let SourceError::Member {
        error: MountError::InvalidMemberPath { spelling, reason },
        ..
    } = &error
    else {
        panic!("expected an invalid member path, got {error}");
    };
    assert_eq!(spelling, "..\\..\\escape.dds");
    assert_eq!(*reason, RelativePathError::ParentComponent);
}

/// Symbolic links are reported and never followed, so a key cannot reach
/// a file outside the mount root through them — neither a file link nor a
/// directory link.
#[cfg(unix)]
#[test]
fn accept_f04_b_symlink_escape_is_rejected_not_followed() {
    use std::os::unix::fs::symlink;

    let outside = TempTree::new("f04-b-outside");
    outside.write("secret.dds", b"outside the mount");
    outside.write("dir/inner.dds", b"also outside");
    let tree = TempTree::new("f04-b-links");
    tree.write("real.dds", b"inside");
    symlink(
        outside.root().join("secret.dds"),
        tree.root().join("link.dds"),
    )
    .expect("file link is created");
    symlink(outside.root().join("dir"), tree.root().join("linked"))
        .expect("directory link is created");

    let mounted = mount_directory(
        builder("links", "Data/Links", PrecedenceClass::Shared),
        tree.root(),
    )
    .expect("the tree mounts without its links");
    let mut rejected: Vec<(String, RejectReason)> = mounted
        .rejected
        .iter()
        .map(|entry| (entry.host_relative.display().to_string(), entry.reason))
        .collect();
    rejected.sort();
    assert_eq!(
        rejected,
        [
            ("link.dds".to_owned(), RejectReason::SymbolicLink),
            ("linked".to_owned(), RejectReason::SymbolicLink),
        ]
    );
    assert_eq!(mounted.mount.member_count(), 1);

    let mut vfs = Vfs::new();
    vfs.mount(mounted.mount).expect("mount id is unique");
    for path in ["link.dds", "linked/inner.dds"] {
        let error = vfs
            .resolve(&context(), &key(path))
            .expect_err("a link target is never a member");
        assert!(matches!(error, ResolveError::NotFound { .. }), "{error}");
    }

    // A mount root that is itself a link is not followed either.
    let root_link = tree.root().join("root-link");
    symlink(outside.root(), &root_link).expect("root link is created");
    let error = mount_directory(
        builder("rootlink", "Data/RootLink", PrecedenceClass::Shared),
        &root_link,
    )
    .expect_err("a linked root must not mount");
    assert!(
        matches!(error, SourceError::RootUnavailable { .. }),
        "{error}"
    );
}

/// A member replaced by a link after mounting is refused at read time
/// instead of reading the link target.
#[cfg(unix)]
#[test]
fn accept_f04_b_member_swapped_for_link_after_mount_is_not_read() {
    use std::os::unix::fs::symlink;

    let outside = TempTree::new("f04-b-swap-outside");
    outside.write("sky.dds", b"same-length!");
    let tree = TempTree::new("f04-b-swap");
    tree.write("sky/sky.dds", b"inside bytes");

    let mut vfs = Vfs::new();
    mount_tree(
        &mut vfs,
        "swap",
        "Data/Swap",
        &tree,
        PrecedenceClass::Shared,
    );
    let resolved = vfs
        .resolve(&context(), &key("SKY/SKY.DDS"))
        .expect("the member resolves");
    assert_eq!(vfs.read_all(&resolved).expect("reads"), b"inside bytes");

    fs::remove_dir_all(tree.root().join("sky")).expect("directory removed");
    symlink(outside.root(), tree.root().join("sky")).expect("directory link swapped in");
    let error = vfs
        .read_range(&resolved, 0, 4)
        .expect_err("a swapped-in link is not followed");
    assert!(
        matches!(error, ReadError::NotARegularFile { .. }),
        "{error}"
    );
}

/// A mount root replaced by a link after mounting is refused at read time,
/// even when the link target holds a same-named, same-length file.
#[cfg(unix)]
#[test]
fn accept_f04_b_mount_root_swapped_for_link_after_mount_is_not_read() {
    use std::os::unix::fs::symlink;

    let outside = TempTree::new("f04-b-root-swap-outside");
    outside.write("sky.dds", b"same-length!");
    let tree = TempTree::new("f04-b-root-swap");
    tree.write("root/sky.dds", b"inside bytes");
    let root = tree.root().join("root");

    let mounted = mount_directory(
        builder("rootswap", "Data/RootSwap", PrecedenceClass::Shared),
        &root,
    )
    .expect("the root mounts");
    let mut vfs = Vfs::new();
    vfs.mount(mounted.mount).expect("mount id is unique");
    let resolved = vfs
        .resolve(&context(), &key("sky.dds"))
        .expect("the member resolves");
    assert_eq!(vfs.read_all(&resolved).expect("reads"), b"inside bytes");

    fs::remove_dir_all(&root).expect("root removed");
    symlink(outside.root(), &root).expect("root link swapped in");
    let error = vfs
        .read_range(&resolved, 0, 4)
        .expect_err("a swapped-in root link is not followed");
    assert!(
        matches!(error, ReadError::NotARegularFile { .. }),
        "{error}"
    );
}

/// AC01 over real bytes: two world directories hold the same-named texture
/// with different contents; each world's context resolves and reads its
/// own file, with random reads inside the member and refusal outside it.
#[test]
fn accept_f04_b_world_mounts_read_their_own_bytes() {
    let c1 = TempTree::new("f04-b-c1");
    c1.write("Textures/Sky.dds", b"world one sky texture");
    let c2 = TempTree::new("f04-b-c2");
    c2.write("textures/sky.DDS", b"WORLD TWO SKY");

    let mut vfs = Vfs::new();
    for (id, group, tree) in [("c1", "ZBD/c1", &c1), ("c2", "ZBD/c2", &c2)] {
        let world = WorldGroup::new(group).expect("fixture world group is valid");
        let mounted = mount_directory(
            builder(id, group, PrecedenceClass::MissionWorld).with_world_group(world),
            tree.root(),
        )
        .expect("world tree mounts");
        vfs.mount(mounted.mount).expect("mount ids are unique");
    }

    let in_world =
        |group: &str| context().with_world_group(WorldGroup::new(group).expect("valid group"));
    let one = vfs
        .resolve(&in_world("zbd/C1"), &key("textures/sky.dds"))
        .expect("world one resolves");
    let two = vfs
        .resolve(&in_world("zbd/c2"), &key("textures/sky.dds"))
        .expect("world two resolves");
    assert_eq!(one.span.container_path(), "ZBD/c1");
    assert_eq!(one.span.member_key(), Some("Textures/Sky.dds"));
    assert_eq!(
        one.span.member_sha256(),
        Some(sha256(b"world one sky texture"))
    );
    assert_eq!(one.span.install_sha256(), install());
    assert_eq!(two.span.member_sha256(), Some(sha256(b"WORLD TWO SKY")));

    assert_eq!(vfs.read_all(&one).expect("reads"), b"world one sky texture");
    assert_eq!(vfs.read_all(&two).expect("reads"), b"WORLD TWO SKY");
    assert_eq!(vfs.read_range(&one, 6, 3).expect("reads"), b"one");
    assert_eq!(vfs.read_range(&two, 13, 0).expect("empty tail"), b"");
    for (start, length) in [(10, 4), (14, 0), (u64::MAX, 2)] {
        let error = vfs
            .read_range(&two, start, length)
            .expect_err("a range outside the member is refused");
        assert!(matches!(error, ReadError::OutOfRange { .. }), "{error}");
    }

    // A resolution from another VFS, or one doctored to another range, is
    // stale here: the bytes it would read are not what it describes.
    let mut other = Vfs::new();
    mount_tree(
        &mut other,
        "c1",
        "ZBD/elsewhere",
        &c1,
        PrecedenceClass::Shared,
    );
    let foreign = other
        .resolve(&context(), &key("textures/sky.dds"))
        .expect("resolves in the other vfs");
    let error = vfs.read_all(&foreign).expect_err("foreign container");
    assert!(
        matches!(error, ReadError::StaleResolution { .. }),
        "{error}"
    );
    let error = Vfs::new().read_all(&one).expect_err("unknown mount");
    assert!(matches!(error, ReadError::UnknownMount { .. }), "{error}");
}

/// Bytes that changed on disk after mounting are refused, by length for
/// any read and by digest for a whole read, and mounting writes nothing.
#[test]
fn accept_f04_b_changed_member_is_refused_and_mounting_writes_nothing() {
    let tree = TempTree::new("f04-b-change");
    tree.write("a/b.dds", b"0123456789");
    let listing = |tree: &TempTree| -> Vec<(String, Vec<u8>)> {
        let mut seen = Vec::new();
        let mut stack = vec![tree.root().to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).expect("lists") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let bytes = fs::read(&path).expect("reads");
                    seen.push((path.display().to_string(), bytes));
                }
            }
        }
        seen.sort();
        seen
    };
    let before = listing(&tree);

    let mut vfs = Vfs::new();
    mount_tree(
        &mut vfs,
        "change",
        "Data/Change",
        &tree,
        PrecedenceClass::Shared,
    );
    let resolved = vfs.resolve(&context(), &key("A/B.DDS")).expect("resolves");
    assert_eq!(vfs.read_all(&resolved).expect("reads"), b"0123456789");
    assert_eq!(listing(&tree), before, "mounting and reading wrote nothing");

    tree.edit_byte("a/b.dds", 3, 0x20);
    let error = vfs
        .read_all(&resolved)
        .expect_err("edited bytes are refused");
    let ReadError::DigestMismatch { mounted, found, .. } = &error else {
        panic!("expected a digest mismatch, got {error}");
    };
    assert_eq!(*mounted, sha256(b"0123456789"));
    assert_eq!(*found, sha256(b"012\x13456789"));

    tree.write("a/b.dds", b"0123");
    let error = vfs
        .read_range(&resolved, 0, 2)
        .expect_err("a shortened file is refused");
    let ReadError::ChangedOnDisk {
        mounted_length,
        found_length,
        ..
    } = error
    else {
        panic!("expected a length change, got {error}");
    };
    assert_eq!((mounted_length, found_length), (10, 4));

    // Declared mounts carry locations but no host bytes.
    let mut declared = builder("declared", "Data/Declared.zip", PrecedenceClass::Shared);
    declared
        .add_member("x.dds", 4, 0, None)
        .expect("declared member is valid");
    let mut vfs = Vfs::new();
    vfs.mount(declared.build().expect("builds"))
        .expect("mounts");
    let resolved = vfs.resolve(&context(), &key("x.dds")).expect("resolves");
    let error = vfs.read_all(&resolved).expect_err("no host bytes");
    assert!(matches!(error, ReadError::NoBacking { .. }), "{error}");
}

/// Mount roots that are missing or files fail by name, and a builder that
/// already declared members cannot become a directory mount.
#[test]
fn accept_f04_b_bad_roots_and_builders_are_refused() {
    let tree = TempTree::new("f04-b-roots");
    tree.write("file.dds", b"x");

    let missing = tree.root().join("missing");
    let error = mount_directory(builder("m", "Data/M", PrecedenceClass::Shared), &missing)
        .expect_err("missing root");
    assert!(
        matches!(error, SourceError::RootUnavailable { .. }),
        "{error}"
    );
    let file = tree.root().join("file.dds");
    let error = mount_directory(builder("f", "Data/F", PrecedenceClass::Shared), &file)
        .expect_err("file root");
    assert!(
        matches!(error, SourceError::RootUnavailable { .. }),
        "{error}"
    );

    let mut declared = builder("d", "Data/D", PrecedenceClass::Shared);
    declared.add_member("y.dds", 1, 0, None).expect("valid");
    let error = mount_directory(declared, tree.root()).expect_err("declared members");
    assert!(matches!(error, SourceError::BuilderHasMembers), "{error}");

    let error = mount_directory(builder("e", "", PrecedenceClass::Shared), tree.root())
        .expect_err("empty container");
    assert!(
        matches!(error, SourceError::Mount(MountError::EmptyContainer)),
        "{error}"
    );
}
