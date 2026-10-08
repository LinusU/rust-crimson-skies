//! F53-D AC04, end to end through the consumer that actually reopens
//! saves: a save written while a mod was mounted is reopened after the mod
//! is disabled, and the dependency is *reported* — the document comes back
//! whole, unmodified and unwritten — rather than degraded into a stock
//! save or refused as corrupt.
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-D`. Every tree and every byte here is newly authored
//! synthetic data below the system temporary directory; no original game
//! data and no `CS_GAME_DIR` access.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::{ProfileSession, SessionOrigin, population_dir};
use cs_app::ui::mods::mount_selection;
use cs_content::mods::{
    ContentOverride, ModManifest, ModSelection, SaveSignatureStatus, content_signature,
    mark_save_document, provided_ids, save_dependency, save_population, signature_fingerprint,
    synthetic_blueprint_mod, synthetic_mount_request,
};
use cs_content::save::settings::SettingCatalog;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::profile::{ProfileKind, Revision};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A disposable directory, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f53-d-dependent-save-{label}-{}-{}",
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

/// The designed base fingerprint the fixture mounts against.
fn base_fingerprint() -> ContentHash {
    ContentHash::from_bytes([7u8; 32])
}

/// The synthetic bytes one declared override ships, as in the crate's own
/// fixtures.
fn payload_bytes(entry: &ContentOverride) -> Vec<u8> {
    let mut bytes = entry.target().as_str().as_bytes().to_vec();
    bytes.extend_from_slice(b"::synthetic payload");
    bytes
}

/// Writes one mod's declared sources below `root`.
fn write_mod(root: &Path, manifest: &ModManifest) {
    for entry in manifest.overrides() {
        let path = root.join(entry.source().as_str());
        fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
        fs::write(path, payload_bytes(entry)).expect("bytes are written");
    }
}

/// Every file below `root`, keyed by relative path, with its bytes — the
/// snapshot "the reopen wrote nothing" is asserted against.
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

/// The blueprint id only `synthetic.zephyr-blueprint` provides.
fn zephyr() -> ContentId {
    ContentId::from_source(ContentKind::Blueprint, "synthetic.zephyr")
        .expect("the synthetic blueprint id is valid")
}

/// **F53 AC04 — the minimum scenario.** A run with the blueprint mod
/// enabled writes a save owning the added blueprint, marked with the
/// mounted signature, into the modded population. The mod is then
/// disabled and the save is reopened through `ProfileSession` under the
/// stock announcement: it reads whole, it is not selected into a rewrite,
/// the dependency report names the differing signature and the unprovided
/// blueprint, and `finish` records that nothing was committed. The stock
/// population shows no trace of the save, and re-enabling the mod
/// reproduces the signature the save was marked with.
#[test]
fn accept_f53_d_disabling_a_mod_reopens_a_dependent_save_without_destructive_fallback() {
    let base = base_fingerprint();
    let request = synthetic_mount_request();
    let catalog = SettingCatalog::empty();
    let user_data = Temp::new("user-data");

    // The mod is offered, enabled and mounted through the host's own
    // mount path (the bounded validator attached, as production does).
    let manifest = synthetic_blueprint_mod();
    let mod_root = Temp::new(manifest.id().as_str());
    write_mod(mod_root.path(), &manifest);
    let mut selection = ModSelection::new();
    selection
        .offer(manifest.clone(), mod_root.path())
        .expect("the mod is offered");
    selection.enable(manifest.id()).expect("enabled");
    let mounted = mount_selection(&selection, &request, base).expect("the mod mounts");
    let modded_signature = content_signature(Some(&mounted), base);
    assert!(
        mounted.marks_sessions(),
        "a blueprint add marks the session"
    );

    // A run under the mounted set writes its save into the population
    // `save_population` assigns — modded, not production — marked with
    // what the session announced and owning the mod-added blueprint.
    let mut session = ProfileSession::open(
        user_data.path(),
        SessionOrigin::Automated,
        save_population(Some(&mounted)),
        &catalog,
    )
    .expect("the modded session opens");
    let profile = session
        .create("Zephyr pilot")
        .expect("the profile is created");
    session
        .commit_with(|document| {
            mark_save_document(document, modded_signature);
            document.blueprints.push(zephyr());
            Ok(())
        })
        .expect("the mark and the blueprint commit as one revision");
    let written = session.finish().expect("the session ends cleanly");
    assert_eq!(written.revision, Some(Revision(2)));
    assert!(!written.uncommitted_changes);

    let population = population_dir(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
    )
    .expect("the modded population has a directory");
    let on_disk = snapshot(&population);

    // The mod is disabled: the next run announces stock content and can
    // serve only the base ids.
    assert!(selection.disable(manifest.id()));
    assert!(selection.enabled_set().is_empty());
    let stock_signature = content_signature(None, base);
    let stock_provided = provided_ids(&request, None);
    assert!(!stock_provided.contains(&zephyr()));

    // Isolation, first half: the save is not a production save. An
    // interactive launch's own population holds no trace of it — the id
    // is not reissued and nothing is selected.
    let production = ProfileSession::open(
        user_data.path(),
        SessionOrigin::Interactive,
        ProfileKind::Production,
        &catalog,
    )
    .expect("the production population opens");
    assert!(production.live().is_empty());
    assert!(production.selected().is_none());
    production.finish().expect("ends cleanly");

    // The dependent save is reopened where it lives: the session
    // auto-selects it from the active pointer, reads the whole revision
    // and reports what it needs. Nothing fails and nothing is repaired.
    let reopened = ProfileSession::open(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
        &catalog,
    )
    .expect("the modded population reopens");
    assert_eq!(reopened.selected(), Some(profile));
    assert!(
        reopened.warnings().is_empty(),
        "no recovery ran: {:?}",
        reopened.warnings()
    );
    let document = reopened.document().expect("the save is loaded");
    assert_eq!(document.blueprints, vec![zephyr()]);
    assert_eq!(
        document
            .fingerprints
            .iter()
            .filter(|entry| entry.name == cs_content::mods::SAVE_CONTENT_FINGERPRINT)
            .map(|entry| entry.hash)
            .collect::<Vec<_>>(),
        vec![signature_fingerprint(modded_signature)],
        "the recorded mark is still the mounted signature's fold"
    );

    let report = save_dependency(document, stock_signature, &stock_provided);
    assert_eq!(
        report.signature(),
        SaveSignatureStatus::Differs {
            recorded: signature_fingerprint(modded_signature),
            announced: signature_fingerprint(stock_signature),
        },
        "the differing content is a report, not a silent downgrade"
    );
    assert_eq!(report.unprovided_blueprints(), &[zephyr()]);
    assert!(!report.is_satisfied());
    let lines = report.diagnostic_lines();
    assert!(
        lines.iter().any(|line| line.contains(zephyr().as_str())),
        "the dependency names the blueprint: {lines:?}"
    );

    // The reopen is read-only: the population is byte-for-byte what the
    // write left — no revision burned, no fallback save, no registry
    // change — and the session ends with nothing to commit.
    let ended = reopened.finish().expect("the reopen ends cleanly");
    assert!(!ended.uncommitted_changes);
    assert_eq!(ended.revision, Some(Revision(2)));
    assert_eq!(
        snapshot(&population),
        on_disk,
        "reopening the dependent save wrote nothing"
    );

    // Re-enable the mod: the mount reproduces the announced signature, and
    // the same untouched save satisfies it — the verdict was a function of
    // the content, never of which session asked.
    selection.enable(manifest.id()).expect("re-enabled");
    let remounted = mount_selection(&selection, &request, base).expect("the set remounts");
    assert_eq!(remounted.signature(), modded_signature);
    let again = ProfileSession::open(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
        &catalog,
    )
    .expect("reopens under the restored mount");
    let report = save_dependency(
        again.document().expect("the save is loaded"),
        content_signature(Some(&remounted), base),
        &provided_ids(&request, Some(&remounted)),
    );
    assert_eq!(report.signature(), SaveSignatureStatus::Matches);
    assert!(report.is_satisfied());
    assert!(again.finish().expect("ends cleanly").revision == Some(Revision(2)));
    assert_eq!(snapshot(&population), on_disk);
}
