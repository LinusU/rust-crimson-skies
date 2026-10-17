//! Task #767 (F53-FU5): a run that has a mounted mod set commits saves that
//! carry the signature the session announced — the write half of "a modified
//! session marks its saves" (F53 non-negotiable 3) — and lands in the
//! population [`cs_content::mods::save_population`] assigns.
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-D`; shared contract `docs/contracts/IDENTITY-CONTENT.md`.
//! Task test prefix: `accept_f53_fu5_`.
//!
//! F53-D shipped the library half — `mark_save_document`, `save_population`,
//! `signature_fingerprint` — but nothing in the shipped game called it, so a
//! save written under mods carried no `fingerprint.content` in practice.
//! These tests open a real [`ProfileSession`] through `ProfileSession::open`
//! (the production entry point), declare the session's announced signature,
//! and read the committed revision back off disk:
//!
//! * a session announcing the mounted signature writes `fingerprint.content`
//!   equal to `signature_fingerprint(content_signature(Some(&mounted), base))`
//!   into the `Modded` population, and the mark survives repeated commits;
//! * a stock session announcing `content_signature(None, base)` — the base
//!   fingerprint — writes the base mark and stays `Production`;
//! * a session that announces nothing writes no `content` claim at all —
//!   the honest `Unrecorded`, not an implicit stock one.
//!
//! Every tree here is newly authored synthetic bytes below the system
//! temporary directory; no original game data and no `CS_GAME_DIR` access.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::{ProfileSession, SessionOrigin};
use cs_content::mods::{
    ModSelection, MountRequest, MountedMods, SAVE_CONTENT_FINGERPRINT, SaveSignatureStatus,
    content_signature, provided_ids, save_dependency, save_population, signature_fingerprint,
    synthetic_blueprint_mod, synthetic_mount_request,
};
use cs_content::save::library::{load_profile_slot, slot_name};
use cs_content::save::settings::SettingCatalog;
use cs_types::evidence::ContentHash;
use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind};

/// The designed base fingerprint the fixture mounts against.
fn base_fingerprint() -> ContentHash {
    ContentHash::from_bytes([7u8; 32])
}

/// A disposable directory, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f53-fu5-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture directory is created");
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

fn catalog() -> SettingCatalog {
    SettingCatalog::new([]).expect("an empty catalog holds together")
}

/// The synthetic bytes one declared override ships, as in F53-D's own
/// fixture: the id plus a fixed tail, so every payload is distinct.
fn payload_bytes(entry: &cs_content::mods::ContentOverride) -> Vec<u8> {
    let mut bytes = entry.target().as_str().as_bytes().to_vec();
    bytes.extend_from_slice(b"::synthetic payload");
    bytes
}

/// The blueprint mod enabled under a shipped root, and its mount — the same
/// mounted set F53-D's save-half tests build, produced here through the same
/// `ModSelection` a host uses.
fn blueprint_mount() -> (Temp, MountedMods, MountRequest) {
    let manifest = synthetic_blueprint_mod();
    let root = Temp::new(manifest.id().as_str());
    for entry in manifest.overrides() {
        let path = root.path().join(entry.source().as_str());
        fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
        fs::write(path, payload_bytes(entry)).expect("bytes are written");
    }
    let mut selection = ModSelection::new();
    selection
        .offer(manifest.clone(), root.path())
        .expect("the mod is offered");
    selection.enable(manifest.id()).expect("enabled");
    let request = synthetic_mount_request();
    let mounted = selection
        .mount(&request, base_fingerprint(), None)
        .expect("the blueprint mod mounts");
    (root, mounted, request)
}

/// The newest whole revision of `id` as it lies on disk in `base`'s `kind`
/// population — the stored save, not a session's in-memory copy.
fn stored_document(base: &Path, kind: ProfileKind, id: ProfileId) -> ProfileDocument {
    let directory = base.join(kind.label()).join(slot_name(id));
    load_profile_slot(&directory)
        .expect("the slot reads")
        .expect("a whole revision is installed")
}

/// The `fingerprint.content` marks `document` carries, in order.
fn content_marks(document: &ProfileDocument) -> Vec<u64> {
    document
        .fingerprints
        .iter()
        .filter(|entry| entry.name == SAVE_CONTENT_FINGERPRINT)
        .map(|entry| entry.hash)
        .collect()
}

/// **AC02, modded half.** A session opened on the population
/// `save_population` assigns to a mounted set and announcing
/// `content_signature(Some(&mounted), base)` commits a revision whose stored
/// document carries `fingerprint.content` equal to
/// `signature_fingerprint` of that signature and whose `kind` is
/// `ProfileKind::Modded`. The mark is idempotent across commits (AC03) and
/// the stored save satisfies the reopen check under what wrote it while
/// reporting the dependency once the mod is gone.
///
/// Under the pre-#767 wiring nothing called `mark_save_document`, so the
/// document carried no `content` entry and `marks` was empty.
#[test]
fn accept_f53_fu5_a_modded_session_marks_its_save_with_the_announced_signature() {
    let base = base_fingerprint();
    let (_root, mounted, request) = blueprint_mount();
    let kind = save_population(Some(&mounted));
    assert_eq!(kind, ProfileKind::Modded);
    let signature = content_signature(Some(&mounted), base);
    assert_ne!(
        signature, base,
        "a mounted session never announces the bare base fingerprint"
    );

    let tree = Temp::new("modded");
    let mut session =
        ProfileSession::open(tree.path(), SessionOrigin::Interactive, kind, &catalog())
            .expect("the modded population opens")
            .with_announced_signature(signature);
    assert_eq!(session.announced_signature(), Some(signature));

    let id = session.create("Modded pilot").expect("a pilot is created");

    // The very first revision already carries the mark, on disk and in the
    // modded population — not only after a later commit.
    let stored = stored_document(tree.path(), kind, id);
    assert_eq!(stored.kind, ProfileKind::Modded);
    assert_eq!(
        content_marks(&stored),
        vec![signature_fingerprint(signature)],
        "the committed document records the announced signature, folded"
    );
    stored.validate().expect("the marked document is valid");

    // AC03: a later commit keeps exactly one `content` entry — the mark
    // replaces, it does not accumulate (the F48 schema refuses a repeated
    // fingerprint name).
    session
        .record_outcome("modded.outcome")
        .expect("the outcome records");
    session.commit().expect("a second revision commits");
    let stored = stored_document(tree.path(), kind, id);
    assert_eq!(
        content_marks(&stored),
        vec![signature_fingerprint(signature)],
        "repeated commits write the same single mark"
    );

    // The save takes on the blueprint only the mod serves — the dependency
    // AC04's reopen check must then be able to name.
    let zephyr = cs_types::content::ContentId::from_source(
        cs_types::content::ContentKind::Blueprint,
        "synthetic.zephyr",
    )
    .expect("the synthetic blueprint id is valid");
    session
        .commit_with(|document| {
            document.blueprints.push(zephyr.clone());
            Ok(())
        })
        .expect("the blueprint lands in the save");

    let stored = stored_document(tree.path(), kind, id);
    assert_eq!(
        content_marks(&stored),
        vec![signature_fingerprint(signature)]
    );

    // Reopened under what wrote it, the save is satisfied — mark and
    // content both check out.
    let provided = provided_ids(&request, Some(&mounted));
    let report = save_dependency(&stored, signature, &provided);
    assert_eq!(report.signature(), SaveSignatureStatus::Matches);
    assert!(report.is_satisfied());

    // Reopened after the mod is gone, the save honestly reports the
    // dependency it was written under: a different recorded mark and the
    // blueprint nothing serves anymore — diagnosed, never repaired.
    let report = save_dependency(
        &stored,
        content_signature(None, base),
        &provided_ids(&request, None),
    );
    assert!(matches!(
        report.signature(),
        SaveSignatureStatus::Differs { .. }
    ));
    assert_eq!(report.unprovided_blueprints(), &[zephyr]);
    assert!(!report.is_satisfied());

    session.finish().expect("the session tears down");
}

/// **AC02, stock half.** A stock session announces the base fingerprint
/// itself (`content_signature(None, base) == base`), writes that mark and
/// stays `Production` — so a save that claims nothing and a save that
/// claims stock are told apart on reopen.
#[test]
fn accept_f53_fu5_a_stock_session_marks_the_base_fingerprint_and_stays_production() {
    let base = base_fingerprint();
    let kind = save_population(None);
    assert_eq!(kind, ProfileKind::Production);
    let signature = content_signature(None, base);
    assert_eq!(signature, base, "stock announces the base fingerprint");

    let tree = Temp::new("stock");
    let mut session =
        ProfileSession::open(tree.path(), SessionOrigin::Interactive, kind, &catalog())
            .expect("the production population opens")
            .with_announced_signature(signature);

    let id = session.create("Stock pilot").expect("a pilot is created");
    session
        .record_outcome("stock.outcome")
        .expect("the outcome records");

    let stored = stored_document(tree.path(), kind, id);
    assert_eq!(stored.kind, ProfileKind::Production);
    assert_eq!(
        content_marks(&stored),
        vec![signature_fingerprint(base)],
        "a stock session marks the base fingerprint it announces"
    );
    stored.validate().expect("the marked document is valid");

    // Under its own announcement the save satisfies the reopen check.
    let request = synthetic_mount_request();
    let report = save_dependency(&stored, signature, &provided_ids(&request, None));
    assert_eq!(report.signature(), SaveSignatureStatus::Matches);
    assert!(report.is_satisfied());

    session.finish().expect("the session tears down");
}

/// The honest default: a session that declares no announced signature
/// writes **no** `content` claim — `Unrecorded` on reopen, told apart from
/// both a stock claim and a modded one. This is what keeps the mark
/// meaningful: it is written because the session announced something, not
/// because every save quietly says "stock".
#[test]
fn accept_f53_fu5_a_session_that_announces_nothing_writes_no_content_claim() {
    let tree = Temp::new("unannounced");
    let mut session =
        ProfileSession::open_sandbox(tree.path(), &catalog()).expect("the sandbox session opens");
    assert_eq!(session.announced_signature(), None);

    let id = session.create("Pilot").expect("a pilot is created");
    session
        .record_outcome("pilot.outcome")
        .expect("the outcome records");

    let stored = stored_document(tree.path(), ProfileKind::Synthetic, id);
    assert!(
        content_marks(&stored).is_empty(),
        "a session that announces nothing records no content claim"
    );
    stored.validate().expect("the unmarked document is valid");

    let request = synthetic_mount_request();
    let report = save_dependency(
        &stored,
        content_signature(None, base_fingerprint()),
        &provided_ids(&request, None),
    );
    assert_eq!(report.signature(), SaveSignatureStatus::Unrecorded);
    assert!(
        report.is_satisfied(),
        "a save making no claim cannot fail a content check"
    );

    session.finish().expect("the session tears down");
}
