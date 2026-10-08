//! F53-D acceptance: source protection, opt-in isolation and the
//! reproducible load order, exercised end to end over the mounted session
//! and the private export.
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-D`. Every tree and every payload here is newly authored
//! synthetic bytes below the system temporary directory; no original game
//! data and no `CS_GAME_DIR` access.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_assets::vfs::{ExportDirectory, ExportError, MountBuilder};
use cs_content::mods::{
    ContentOverride, DependencyStrength, EngineRange, ModDependency, ModHeader, ModManifest,
    ModPayload, ModSelection, ModSet, ModVersion, MountEnvironment, OverrideAction, VersionRange,
    export_mounted_mods, mount_mods, mount_payloads, mount_to_text, open_mod_session, provided_ids,
    session_builder, synthetic_conflicting_mods, synthetic_mod_claim, synthetic_mount_request,
    synthetic_tuning_mod,
};
use cs_types::asset_id::{
    AssetKey, AssetVariant, ModId, MountId, MountNamespace, PrecedenceClass, ResolveContext,
};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::ContentHash;
use cs_types::install::RelativePath;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A disposable directory, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f53-d-isolation-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp dir is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The designed base fingerprint the fixtures mount against.
fn base_fingerprint() -> ContentHash {
    ContentHash::from_bytes([7u8; 32])
}

/// The synthetic bytes one declared override ships, as in the crate's own
/// fixtures: the id plus a fixed tail, so every payload is distinct and
/// traceable to its claim.
fn payload_bytes(entry: &ContentOverride) -> Vec<u8> {
    let mut bytes = entry.target().as_str().as_bytes().to_vec();
    bytes.extend_from_slice(b"::synthetic payload");
    bytes
}

/// Writes one mod's declared sources below `root`, as a shipped mod would
/// have them on disk.
fn write_mod(root: &Path, manifest: &ModManifest) {
    for entry in manifest.overrides() {
        let path = root.join(entry.source().as_str());
        fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
        fs::write(path, payload_bytes(entry)).expect("bytes are written");
    }
}

/// Every file below `root`, keyed by relative path, with its bytes — the
/// snapshot "the mounted source was never touched" is asserted against.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        for entry in fs::read_dir(&dir).expect("the directory lists") {
            let path = entry.expect("the entry reads").path();
            if path.is_dir() {
                queue.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).expect("below root").to_path_buf(),
                    fs::read(&path).expect("the file reads"),
                );
            }
        }
    }
    files
}

/// A mod whose one override ships `shared/hull.png` — the path both mods of
/// the contested fixture carry, so the VFS load order is what answers it.
/// `depends_on` makes the load order explicit rather than alphabetical.
fn contested_manifest(id: &str, target: &str, depends_on: Option<&str>) -> ModManifest {
    let dependencies = depends_on
        .into_iter()
        .map(|on| {
            ModDependency::new(
                ModId::new(on).expect("valid mod id"),
                VersionRange::at_least(ModVersion::new(1, 0, 0)),
                DependencyStrength::Required,
            )
        })
        .collect();
    ModManifest::try_new(
        ModHeader::try_new(
            ModId::new(id).expect("valid mod id"),
            id,
            ModVersion::new(1, 0, 0),
            EngineRange::new(ModVersion::new(0, 1, 0), ModVersion::new(0, 9, 9))
                .expect("the engine range is valid"),
            false,
            Provenance::designed(synthetic_mod_claim()),
        )
        .expect("the header is valid"),
        dependencies,
        vec![ModPayload::new("shared/hull.png").expect("valid path")],
        vec![
            ContentOverride::try_new(
                ContentId::from_source(ContentKind::Image, target).expect("valid id"),
                OverrideAction::Add,
                "shared/hull.png",
                64,
            )
            .expect("the override is valid"),
        ],
    )
    .expect("the manifest is valid")
}

/// **Source protection.** A mount, a session, reads and an export all
/// *look* at the mod roots and the installation stand-in; nothing writes
/// into them — the trees are byte-for-byte what they were — and the only
/// write path (the private export) refuses a destination inside any
/// mounted source, so original or mod bytes can never be modified or
/// silently re-shipped as original. Exported payloads carry exactly the
/// digests the mount measured, and the report names the base-provided ids
/// the export deliberately does not contain.
#[test]
fn accept_f53_d_mounted_sources_are_read_only_and_exports_never_enter_them() {
    let base = base_fingerprint();
    let request = synthetic_mount_request();

    // An installation stand-in and two mod roots.
    let install = Temp::new("install");
    fs::create_dir_all(install.path().join("data")).expect("dirs");
    fs::write(
        install.path().join("data/panel.png"),
        b"synthetic base bytes",
    )
    .expect("base bytes");
    let install_before = snapshot(install.path());

    let (repaint, bright) = synthetic_conflicting_mods();
    let tuning = synthetic_tuning_mod();
    let repaint_root = Temp::new("repaint");
    let bright_root = Temp::new("bright");
    let tuning_root = Temp::new("tuning");
    write_mod(repaint_root.path(), &repaint);
    write_mod(bright_root.path(), &bright);
    write_mod(tuning_root.path(), &tuning);
    let roots_before = [
        snapshot(repaint_root.path()),
        snapshot(bright_root.path()),
        snapshot(tuning_root.path()),
    ];

    let mut selection = ModSelection::new();
    for (manifest, root) in [
        (&repaint, &repaint_root),
        (&bright, &bright_root),
        (&tuning, &tuning_root),
    ] {
        selection
            .offer(manifest.clone(), root.path())
            .expect("offered");
        selection.enable(manifest.id()).expect("enabled");
    }
    let mounted = selection
        .mount(&request, base, None)
        .expect("the three-mod set mounts");

    // A session that also carries the installation stand-in as a shared
    // mount: mod payloads and base bytes resolve side by side from their
    // own mounts.
    let mut builder = session_builder(&ResolveContext::new(base), &mounted);
    mount_payloads(&mut builder, &mounted).expect("payloads mount");
    builder
        .mount_directory(
            MountBuilder::new(
                MountId::new("synthetic-install").expect("valid mount id"),
                MountNamespace::new("install").expect("valid namespace"),
                PrecedenceClass::Shared,
                "synthetic-install",
            ),
            install.path(),
        )
        .expect("the installation stand-in mounts");
    let session = builder.open();

    // Both halves answer from their own mounts: the mod payload through
    // the opted-in mod mount, the base file through the shared mount.
    let mod_key = AssetKey::new(
        MountNamespace::new("mod").expect("valid namespace"),
        RelativePath::new("tuning/vulcan.toml").expect("valid path"),
        AssetVariant::default(),
    );
    let asset = session.resolve(&mod_key).expect("the mod payload resolves");
    assert_eq!(asset.resolved().mount.as_str(), tuning.id().as_str());
    let install_key = AssetKey::new(
        MountNamespace::new("install").expect("valid namespace"),
        RelativePath::new("data/panel.png").expect("valid path"),
        AssetVariant::default(),
    );
    let asset = session
        .resolve(&install_key)
        .expect("the installation file resolves");
    assert_eq!(asset.resolved().mount.as_str(), "synthetic-install");
    assert_eq!(
        session.read_all(&asset).expect("the base file reads"),
        b"synthetic base bytes"
    );

    // The only write path refuses every destination inside a mounted
    // source — an export root inside the installation or inside a mod
    // root is refused by name.
    let inside_install = install.path().join("export");
    fs::create_dir_all(&inside_install).expect("dirs");
    let error = ExportDirectory::open(&inside_install, &session)
        .expect_err("an export root inside a mounted source is refused");
    assert!(
        matches!(error, ExportError::RootInsideMount { .. }),
        "the guard names the mount it protects: {error}"
    );
    let inside_mod = tuning_root.path().join("export");
    fs::create_dir_all(&inside_mod).expect("dirs");
    let error = ExportDirectory::open(&inside_mod, &session)
        .expect_err("an export root inside a mod root is refused");
    assert!(
        matches!(error, ExportError::RootInsideMount { .. }),
        "the mod root is protected the same way: {error}"
    );

    // A private export outside every source writes each winning payload's
    // own bytes — digest-identical to what the mount measured — and names
    // the base-provided ids it stands in for without shipping them.
    let export_root = Temp::new("export");
    let export_dir =
        ExportDirectory::open(export_root.path(), &session).expect("a private root opens");
    let export =
        export_mounted_mods(&mounted, &request, &export_dir).expect("the mounted set exports");
    let exported: BTreeMap<PathBuf, Vec<u8>> = snapshot(export_root.path());
    for payload in mounted.payloads() {
        let name = format!("{}/{}", payload.mod_id().as_str(), payload.source());
        let file = export
            .files
            .iter()
            .find(|file| file.path.ends_with(&name))
            .expect("every winning payload is exported");
        assert_eq!(
            file.sha256,
            payload.sha256(),
            "the exported bytes are the measured bytes"
        );
        assert!(exported.contains_key(Path::new(&name)));
    }
    assert_eq!(
        exported.len(),
        mounted.payloads().len() + 1,
        "the export is the payloads plus the report — nothing else"
    );
    assert!(
        export
            .original_dependencies
            .iter()
            .any(|dependency| &dependency.needed_by == tuning.id()),
        "the tuning mod's replace of base content is named as unresolved"
    );
    assert!(
        !exported
            .values()
            .any(|bytes| bytes == b"synthetic base bytes"),
        "no export file carries installation bytes"
    );
    fs::remove_dir_all(&inside_install).expect("cleanup");
    fs::remove_dir_all(&inside_mod).expect("cleanup");
    session.close();

    // And the sources themselves: after the mount, the session, the reads
    // and the export, every mounted tree is byte-for-byte untouched.
    assert_eq!(snapshot(install.path()), install_before);
    assert_eq!(
        [
            snapshot(repaint_root.path()),
            snapshot(bright_root.path()),
            snapshot(tuning_root.path()),
        ],
        roots_before,
        "mounting, serving and exporting never writes into a mod root"
    );
}

/// **Reproducible load order.** The order a set mounts in is a function of
/// the set, not of the order it was offered in: two `ModSet`s built from
/// the same manifests in opposite orders plan and sign identically, a
/// contested payload path resolves to the mod *later* in the plan's order
/// (the F04 mod stack, literally), and disabling then re-enabling a mod
/// reproduces the signature a save or lobby was recorded against.
#[test]
fn accept_f53_d_the_load_order_is_reproduced_and_the_later_mod_wins() {
    let base = base_fingerprint();
    let request = synthetic_mount_request();

    // Two mods that both ship `shared/hull.png` under different content
    // ids, with beta requiring alpha so the plan order is forced.
    let alpha = contested_manifest("synthetic.hull-alpha", "synthetic.hull-alpha", None);
    let beta = contested_manifest(
        "synthetic.hull-beta",
        "synthetic.hull-beta",
        Some("synthetic.hull-alpha"),
    );
    let alpha_root = Temp::new("alpha");
    let beta_root = Temp::new("beta");
    fs::create_dir_all(alpha_root.path().join("shared")).expect("dirs");
    fs::write(alpha_root.path().join("shared/hull.png"), b"alpha hull").expect("bytes");
    fs::create_dir_all(beta_root.path().join("shared")).expect("dirs");
    fs::write(beta_root.path().join("shared/hull.png"), b"beta hull").expect("bytes");

    let environment = || {
        MountEnvironment::new(base)
            .with_root(alpha.id().clone(), alpha_root.path().to_path_buf())
            .with_root(beta.id().clone(), beta_root.path().to_path_buf())
    };

    // Offered in either order, the plan and the signature are identical.
    let forward = mount_mods(
        &ModSet::new(vec![alpha.clone(), beta.clone()]),
        &request,
        &environment(),
    )
    .expect("the forward set mounts");
    let backward = mount_mods(
        &ModSet::new(vec![beta.clone(), alpha.clone()]),
        &request,
        &environment(),
    )
    .expect("the backward set mounts");
    assert_eq!(forward.plan().order(), backward.plan().order());
    assert_eq!(forward.signature(), backward.signature());
    assert_eq!(mount_to_text(&forward), mount_to_text(&backward));
    assert_eq!(
        forward.plan().order(),
        &[alpha.id().clone(), beta.id().clone()],
        "the dependency order is the load order"
    );

    // The contested path resolves to the later mount in that order, and
    // the served bytes are that mod's — the plan's order is the stack the
    // resolver consults.
    let session =
        open_mod_session(&ResolveContext::new(base), &forward).expect("the payload mounts open");
    let contested = AssetKey::new(
        MountNamespace::new("mod").expect("valid namespace"),
        RelativePath::new("shared/hull.png").expect("valid path"),
        AssetVariant::default(),
    );
    let asset = session
        .resolve(&contested)
        .expect("the contested path resolves");
    assert_eq!(asset.resolved().mount.as_str(), beta.id().as_str());
    assert_eq!(session.read_all(&asset).expect("reads"), b"beta hull");
    session.close();

    // A context that never opted in cannot resolve the same key: the same
    // mounts answer nothing without the stack.
    let unopted = {
        let mut builder = cs_assets::vfs::SessionBuilder::new(ResolveContext::new(base));
        mount_payloads(&mut builder, &forward).expect("payloads mount");
        builder.open()
    };
    assert!(
        unopted.resolve(&contested).is_err(),
        "an unopted context is refused"
    );
    unopted.close();

    // Disable then re-enable: the signature is reproduced exactly, which
    // is what makes the recorded save mark and the lobby handshake stable.
    let signature = forward.signature();
    let alpha_only = mount_mods(&ModSet::new(vec![alpha.clone()]), &request, &environment())
        .expect("the reduced set mounts");
    assert_ne!(alpha_only.signature(), signature);
    let reenabled = mount_mods(
        &ModSet::new(vec![beta.clone(), alpha.clone()]),
        &request,
        &environment(),
    )
    .expect("the re-enabled set mounts");
    assert_eq!(reenabled.signature(), signature);

    // The provided-id set follows the mount: without beta its added id is
    // unserved, and a save naming it reports it.
    assert!(
        !provided_ids(&request, Some(&alpha_only)).contains(
            &ContentId::from_source(ContentKind::Image, "synthetic.hull-beta").expect("valid id")
        ),
        "a disabled mod's added id is no longer provided"
    );
}
