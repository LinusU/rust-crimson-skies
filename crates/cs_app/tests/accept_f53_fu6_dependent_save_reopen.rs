//! F53-FU6 (#768) acceptance: the host's reopen check for a dependent save.
//!
//! F53-D implemented `cs_content::mods::save_dependency` and verified it at
//! the library boundary. This stage is the host half: a save written under a
//! mounted set is reopened through the host's own `ProfileSession`, the host
//! runs the check against the signature the session announces and the content
//! ids it provides, and the verdict is a decided block-or-proceed policy plus
//! the report's own lines — never a silent downgrade, never a rewrite of the
//! document.
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! AC04 and stage `### F53-D`; policy finding
//! `docs/findings/2026-10-17-f53-fu6-save-reopen-policy.md`. Every tree and
//! byte here is newly authored synthetic data below the system temporary
//! directory; no original game data and no `CS_GAME_DIR` access.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::{
    ProfileSession, SessionContent, SessionError, SessionOrigin, population_dir,
};
use cs_app::ui::mods::{ModsNotice, SaveReopenDecision, mount_selection, save_reopen_verdict};
use cs_content::mods::{
    ContentOverride, ModManifest, ModSelection, content_signature, mark_save_document,
    provided_ids, save_population, signature_fingerprint, synthetic_blueprint_mod,
    synthetic_mount_request,
};
use cs_content::save::settings::SettingCatalog;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::profile::{ProfileDocument, ProfileId, ProfileKind, Revision};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A disposable directory, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f53-fu6-save-reopen-{label}-{}-{}",
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

/// The blueprint id only the blueprint mod provides.
fn zephyr() -> ContentId {
    ContentId::from_source(ContentKind::Blueprint, "synthetic.zephyr")
        .expect("the synthetic blueprint id is valid")
}

/// **F53 AC04 through the host.** A run with the blueprint mod enabled writes
/// a marked save owning the mod-added blueprint. The mod is disabled and the
/// save is reopened through `ProfileSession` under the stock announcement:
/// the host's check names the differing signature and the unprovided
/// blueprint, the decision refuses the open, and the document on disk is
/// byte-for-byte unchanged — no revision burned, no fallback save, no
/// recovery warning. Removing the `save_dependency` call from
/// `save_reopen_verdict` leaves the verdict `Open` and fails here.
#[test]
fn accept_f53_fu6_disabling_a_mod_refuses_the_reopen_through_the_host() {
    let base = base_fingerprint();
    let request = synthetic_mount_request();
    let catalog = SettingCatalog::empty();
    let user_data = Temp::new("user-data");

    // The mod is offered, enabled and mounted through the host's own mount
    // path (the bounded validator attached, as production does).
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
    let stock_signature = content_signature(None, base);
    assert_ne!(modded_signature, stock_signature);

    // A run under the mounted set writes its save into the population
    // `save_population` assigns — modded, not production — marked with what
    // the session announced and owning the mod-added blueprint.
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
    assert!(!provided_ids(&request, None).contains(&zephyr()));

    // The dependent save is reopened where it lives, through the host's own
    // session declaring the stock content it now runs: the open-time check
    // names the differing signature and the unprovided blueprint, refuses to
    // select the save, and reports no recovery. Nothing is repaired or
    // substituted.
    let stock_content = SessionContent::new(stock_signature, provided_ids(&request, None));
    let mut reopened = ProfileSession::open_with_content(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
        &catalog,
        stock_content,
    )
    .expect("the modded population reopens");
    assert_eq!(
        reopened.selected(),
        None,
        "a save needing absent content is not selected"
    );
    assert!(
        reopened
            .warnings()
            .iter()
            .all(|line| !line.contains("recover")),
        "no recovery ran: {:?}",
        reopened.warnings()
    );
    assert!(
        reopened
            .warnings()
            .iter()
            .any(|line| line.contains("save_content_unprovided")),
        "the refusal is named: {:?}",
        reopened.warnings()
    );
    let (refused, verdict) = reopened.reopen_verdict().expect("the open made a verdict");
    let verdict = verdict.clone();
    assert_eq!(refused, profile);
    assert_eq!(
        verdict.decision(),
        SaveReopenDecision::Refuse,
        "a save needing absent content is not opened"
    );
    assert!(!verdict.proceeds());
    let report_lines = verdict.lines();
    assert_eq!(
        report_lines.len(),
        2,
        "the signature and the blueprint are both named: {report_lines:?}"
    );
    assert!(
        report_lines[0].contains(&format!("{:016x}", signature_fingerprint(modded_signature)))
            && report_lines[0]
                .contains(&format!("{:016x}", signature_fingerprint(stock_signature))),
        "the verdict names both folds: {}",
        report_lines[0]
    );
    assert!(
        report_lines[1].contains(zephyr().as_str()),
        "the missing blueprint is named: {}",
        report_lines[1]
    );
    match verdict.notice().expect("a refused reopen is shown") {
        ModsNotice::SaveDependency { decision, lines } => {
            assert_eq!(decision, SaveReopenDecision::Refuse);
            assert_eq!(decision.code(), "save_content_unprovided");
            assert_eq!(lines.as_slice(), report_lines);
        }
        other => panic!("the reopen verdict is its own notice, got {other:?}"),
    }

    // An explicit select is refused by name too, and leaves the session as it
    // was.
    match reopened.select(profile) {
        Err(SessionError::SaveRefused {
            profile: named,
            code,
            lines,
        }) => {
            assert_eq!(named, profile);
            assert_eq!(code, "save_content_unprovided");
            assert_eq!(lines.as_slice(), report_lines);
        }
        other => panic!("select must refuse by name, got {other:?}"),
    }
    assert_eq!(reopened.selected(), None);

    // The reopen is read-only: the population is byte-for-byte what the
    // write left — no revision burned, no fallback save, no registry
    // change — and the session ends with nothing to commit.
    let ended = reopened.finish().expect("the reopen ends cleanly");
    assert!(!ended.uncommitted_changes);
    assert_eq!(ended.revision, None, "nothing was selected");
    assert_eq!(
        snapshot(&population),
        on_disk,
        "reopening the dependent save wrote nothing"
    );

    // Re-enable the mod: the mount reproduces the announced signature, and
    // the same untouched save is satisfied — the verdict is a function of the
    // content, never of which session asked.
    selection.enable(manifest.id()).expect("re-enabled");
    let remounted = mount_selection(&selection, &request, base).expect("the set remounts");
    assert_eq!(remounted.signature(), modded_signature);
    let again = ProfileSession::open_with_content(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
        &catalog,
        SessionContent::new(
            content_signature(Some(&remounted), base),
            provided_ids(&request, Some(&remounted)),
        ),
    )
    .expect("reopens under the restored mount");
    assert_eq!(again.selected(), Some(profile));
    let (_, verdict) = again.reopen_verdict().expect("the open made a verdict");
    assert_eq!(verdict.decision(), SaveReopenDecision::Open);
    assert!(verdict.proceeds());
    assert!(verdict.lines().is_empty());
    assert!(verdict.notice().is_none());
    assert_eq!(
        again.finish().expect("ends cleanly").revision,
        Some(Revision(2))
    );
    assert_eq!(snapshot(&population), on_disk);
}

/// **The policy's other two arms.** A differing signature with nothing
/// missing is a warning the open proceeds under — the report is still shown;
/// a satisfied report opens with nothing to show. The check keys on content
/// this session can serve, so a save owning an unprovided blueprint refuses
/// even when its recorded mark matches.
#[test]
fn accept_f53_fu6_a_differing_signature_with_all_content_provided_warns_and_proceeds() {
    let base = base_fingerprint();
    let request = synthetic_mount_request();

    // A save marked with a signature this session does not announce, owning
    // nothing beyond the base content: only the mark differs.
    let foreign = ContentHash::from_bytes([0xAB; 32]);
    let mut document =
        ProfileDocument::synthetic(ProfileId::new(1).expect("a non-zero id"), Revision(1));
    mark_save_document(&mut document, foreign);
    document.validate().expect("the marked document is valid");

    let verdict = save_reopen_verdict(&document, &request, base, None);
    assert_eq!(verdict.decision(), SaveReopenDecision::OpenMarked);
    assert!(
        verdict.proceeds(),
        "a differing signature alone does not refuse the save"
    );
    assert_eq!(
        verdict.lines().len(),
        1,
        "the difference is still shown: {:?}",
        verdict.lines()
    );
    assert_eq!(verdict.decision().code(), "save_content_differs");
    let notice = verdict
        .notice()
        .expect("a marked reopen is shown as a warning");
    assert_eq!(
        notice,
        ModsNotice::SaveDependency {
            decision: SaveReopenDecision::OpenMarked,
            lines: verdict.lines().to_vec(),
        }
    );
    assert!(notice.fallback_text().contains("different content"));
    assert!(notice.fallback_text().contains("save_content_differs"));

    // A save whose recorded mark is this session's signature opens with
    // nothing to show.
    let stock = content_signature(None, base);
    let mut satisfied =
        ProfileDocument::synthetic(ProfileId::new(2).expect("a non-zero id"), Revision(1));
    mark_save_document(&mut satisfied, stock);
    let verdict = save_reopen_verdict(&satisfied, &request, base, None);
    assert_eq!(verdict.decision(), SaveReopenDecision::Open);
    assert!(verdict.proceeds());
    assert!(verdict.lines().is_empty());
    assert!(
        verdict.notice().is_none(),
        "a satisfied reopen shows nothing"
    );

    // The provided set is the session's own: a save owning the mod-added
    // blueprint is refused while the mod is not mounted, even though its
    // recorded mark is this session's signature.
    let mut owning =
        ProfileDocument::synthetic(ProfileId::new(3).expect("a non-zero id"), Revision(1));
    mark_save_document(&mut owning, stock);
    owning.blueprints.push(zephyr());
    owning.validate().expect("the document is valid");
    let verdict = save_reopen_verdict(&owning, &request, base, None);
    assert_eq!(verdict.decision(), SaveReopenDecision::Refuse);
    assert_eq!(verdict.lines().len(), 1);
    assert!(verdict.lines()[0].contains(zephyr().as_str()));
}

/// **The warn arm through the session.** A save marked with another
/// signature but owning only base content is selected on open and on an
/// explicit select; the warning stays available and the population is
/// untouched. A session that declares nothing checks nothing.
#[test]
fn accept_f53_fu6_a_differing_mark_is_selected_with_the_warning_kept() {
    let base = base_fingerprint();
    let request = synthetic_mount_request();
    let catalog = SettingCatalog::empty();
    let user_data = Temp::new("warn");
    let foreign = ContentHash::from_bytes([0xAB; 32]);

    let mut writer = ProfileSession::open(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
        &catalog,
    )
    .expect("opens");
    let profile = writer.create("Marked pilot").expect("created");
    writer
        .commit_with(|document| {
            mark_save_document(document, foreign);
            Ok(())
        })
        .expect("marked");
    writer.finish().expect("ends");
    let population = population_dir(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
    )
    .expect("directory");
    let on_disk = snapshot(&population);

    let content = SessionContent::new(content_signature(None, base), provided_ids(&request, None));
    let mut session = ProfileSession::open_with_content(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
        &catalog,
        content,
    )
    .expect("opens");
    assert_eq!(session.selected(), Some(profile), "a warning still opens");
    let (_, verdict) = session.reopen_verdict().expect("verdict");
    assert_eq!(verdict.decision(), SaveReopenDecision::OpenMarked);
    assert_eq!(verdict.decision().code(), "save_content_differs");
    assert_eq!(verdict.lines().len(), 1);
    assert_eq!(snapshot(&population), on_disk, "the open wrote nothing");
    // `select` rewrites the active pointer by F48's own rule; the document
    // is still untouched.
    session.select(profile).expect("select proceeds too");
    assert_eq!(
        session.reopen_verdict().map(|(_, v)| v.decision()),
        Some(SaveReopenDecision::OpenMarked)
    );
    session.finish().expect("ends");

    let plain = ProfileSession::open(
        user_data.path(),
        SessionOrigin::Automated,
        ProfileKind::Modded,
        &catalog,
    )
    .expect("opens");
    assert_eq!(plain.selected(), Some(profile));
    assert!(
        plain.reopen_verdict().is_none(),
        "nothing declared, nothing checked"
    );
    plain.finish().expect("ends");
    let after = snapshot(&population);
    assert_eq!(
        after.get(Path::new("profile-1/profile.sav")),
        on_disk.get(Path::new("profile-1/profile.sav")),
        "no reopen rewrote the document"
    );
}
