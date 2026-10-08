//! What a save records about the content it was written under, and the
//! check a reopening session runs against it (F53-D).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-D`; shared contract `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F53 non-negotiable 3 says a modified session *marks* its saves, and AC04
//! says a save written under a mod must survive that mod being switched
//! off: reopening it is allowed to report, never to silently degrade.
//! This module is both halves of that rule, in the vocabulary the save
//! schema (`cs_types::profile::ProfileDocument`, F48-A) already provides:
//!
//! * **The mark — [`mark_save_document`].** A session writes the content
//!   signature it announces ([`super::selection::content_signature`]) into
//!   the save's `fingerprint.<name>` list under [`SAVE_CONTENT_FINGERPRINT`].
//!   The field is eight bytes and the signature is thirty-two, so the entry
//!   carries a designed fold of it ([`signature_fingerprint`]) — a record
//!   the save can be checked *against*, not a second hash of the content.
//! * **The population — [`save_population`].** A session that mounts any
//!   set at all writes its saves into the modded population, because their
//!   fingerprint is not the stock fingerprint; a stock session writes
//!   production. That is F48's separation rule applied to the mount.
//! * **The check — [`save_dependency`].** On reopen, the save's recorded
//!   mark is compared to what this session now announces and every
//!   blueprint the save owns is checked against the content ids this
//!   session can actually serve ([`provided_ids`]). The answer is a
//!   [`SaveDependencyReport`] — evidence a caller shows — never a rewrite:
//!   this module holds no write path back to the document, so a dependent
//!   save under a disabled mod is *diagnosed*, not repaired into
//!   something it is not.
//!
//! # Designed, not original
//!
//! The original game's mod support and its save format's mod fields are
//! unmeasured (F53 "Research boundary"). The fingerprint name, the fold,
//! the population choice and the report here are newly authored project
//! design carrying designed provenance; nothing in this module is evidence
//! about the original game.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;
use cs_types::profile::{FingerprintEntry, ProfileDocument, ProfileKind};

use super::MountRequest;
use super::mount::MountedMods;

/// The `fingerprint.<name>` a save records the session's announced content
/// signature under: `fingerprint.content=<16 lowercase hex>` in the encoded
/// document (F48-A field syntax).
///
/// `content` names what the value *is* — the
/// [`super::selection::content_signature`] the writing session announced —
/// in the same vocabulary the lobby handshake calls it (`content_sha256`).
pub const SAVE_CONTENT_FINGERPRINT: &str = "content";

/// The eight bytes a save's fingerprint field holds for `signature`.
///
/// A [`FingerprintEntry::hash`] is `u64` and the signature is SHA-256, so
/// the field records the signature's first eight bytes, big-endian. This
/// is a designed fold, not a second hash of the content: two saves under
/// the same announced signature record the same entry, and a save under a
/// different signature almost surely records a different one — which is
/// all the reopen check needs, because it compares *this* save's record
/// against *this* session's announcement, never against the content again.
#[must_use]
pub fn signature_fingerprint(signature: ContentHash) -> u64 {
    u64::from_be_bytes(
        signature.as_bytes()[..8]
            .try_into()
            .expect("a SHA-256 digest is 32 bytes"),
    )
}

/// The fingerprint entry a session announcing `signature` records into a
/// save it writes.
#[must_use]
pub fn save_fingerprint(signature: ContentHash) -> FingerprintEntry {
    FingerprintEntry {
        name: SAVE_CONTENT_FINGERPRINT.to_owned(),
        hash: signature_fingerprint(signature),
    }
}

/// Records the session's announced content signature in `document` — the
/// write half of "modified gameplay marks saves" (F53 non-negotiable 3).
///
/// The mark is idempotent: an earlier `content` entry is replaced, never
/// duplicated (the schema refuses a repeated fingerprint name), so a
/// session that commits many revisions under one mount writes the same
/// mark each time. The honest record cuts both ways: a *stock* session
/// marks `content` with the base fingerprint it announces, so a save that
/// claims nothing and a save that claims stock are told apart on reopen.
pub fn mark_save_document(document: &mut ProfileDocument, signature: ContentHash) {
    document
        .fingerprints
        .retain(|entry| entry.name != SAVE_CONTENT_FINGERPRINT);
    document.fingerprints.push(save_fingerprint(signature));
}

/// The population a session's saves belong to: any mounted set — cosmetic
/// or not — writes into [`ProfileKind::Modded`], because a save recorded
/// under content bytes the installation does not provide is not a
/// production save and a stock session must never pick it up unmarked. A
/// session with no mounted set writes [`ProfileKind::Production`].
///
/// This is the assignment F48's separation rule needs from the mount: the
/// library enforces that the kinds stay apart
/// ([`crate::save::library::LibraryError::ForeignDocument`]), and this
/// answers which side of the wall a content configuration belongs on.
#[must_use]
pub fn save_population(mounted: Option<&MountedMods>) -> ProfileKind {
    match mounted {
        Some(_) => ProfileKind::Modded,
        None => ProfileKind::Production,
    }
}

/// The content ids a session can serve: every base id the request names,
/// plus the target of every winning payload `mounted` serves.
///
/// A *shadowed* claim serves nothing, so it adds nothing — the payload
/// list holds winners only. A stock session (`mounted` is `None`) provides
/// exactly the base ids. The order the mods were offered in is irrelevant
/// by construction: the set is built from the mounted plan, which is
/// already canonical.
#[must_use]
pub fn provided_ids(request: &MountRequest, mounted: Option<&MountedMods>) -> BTreeSet<ContentId> {
    let mut ids = request.base_ids().clone();
    if let Some(mounted) = mounted {
        ids.extend(
            mounted
                .payloads()
                .iter()
                .map(|payload| payload.target().clone()),
        );
    }
    ids
}

/// How a reopened save's recorded content mark stands against the
/// signature this session announces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveSignatureStatus {
    /// The save records the signature this session announces: it was
    /// written under exactly this content.
    Matches,
    /// The save records no content fingerprint at all. It makes no claim
    /// about the content it was written under — an honest `Unknown`, not a
    /// match and not a mismatch.
    Unrecorded,
    /// The save records a different mark: it was written under content
    /// this session is not running — most concretely, a save written while
    /// a mod was enabled, reopened after the mod was disabled.
    Differs {
        /// The mark the save carries.
        recorded: u64,
        /// The fold of what this session announces.
        announced: u64,
    },
}

impl SaveSignatureStatus {
    /// The stable label used in diagnostics.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Matches => "matches",
            Self::Unrecorded => "unrecorded",
            Self::Differs { .. } => "differs",
        }
    }
}

impl fmt::Display for SaveSignatureStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Matches => f.write_str("the save's content mark is this session's signature"),
            Self::Unrecorded => f.write_str("the save records no content fingerprint"),
            Self::Differs {
                recorded,
                announced,
            } => write!(
                f,
                "the save records content signature {recorded:016x}, which is not the \
                 {announced:016x} this session announces"
            ),
        }
    }
}

/// What a reopened save needs that this session does not provide, with the
/// signature verdict — the report that stands where a destructive fallback
/// would.
///
/// A report is the whole answer; nothing here writes back to the save, so
/// a dependent save under a disabled mod is shown for what it is rather
/// than repaired into a stock save (F53 AC04).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveDependencyReport {
    signature: SaveSignatureStatus,
    unprovided_blueprints: Vec<ContentId>,
}

impl SaveDependencyReport {
    /// How the save's recorded content mark stands against this session.
    pub fn signature(&self) -> SaveSignatureStatus {
        self.signature
    }

    /// The blueprint ids the save owns that neither the base content nor
    /// any winning payload provides, in save order: the mods the save
    /// depends on, named by the content only they served.
    pub fn unprovided_blueprints(&self) -> &[ContentId] {
        &self.unprovided_blueprints
    }

    /// Whether the save states nothing this session contradicts: its
    /// recorded mark is this session's signature or it records none, and
    /// every blueprint it owns is provided.
    ///
    /// `Unrecorded` satisfies because a save that makes no content claim
    /// cannot fail a content check — the caller can see that half in
    /// [`SaveDependencyReport::signature`] either way.
    #[must_use]
    pub fn is_satisfied(&self) -> bool {
        !matches!(self.signature, SaveSignatureStatus::Differs { .. })
            && self.unprovided_blueprints.is_empty()
    }

    /// The report as text lines a caller can show: one line per thing the
    /// session does not satisfy. An empty list is the satisfied report.
    #[must_use]
    pub fn diagnostic_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let SaveSignatureStatus::Differs { .. } = self.signature {
            lines.push(self.signature.to_string());
        }
        for id in &self.unprovided_blueprints {
            lines.push(format!(
                "the save owns {id}, which the content this session provides does not include"
            ));
        }
        lines
    }
}

/// The reopen check of F53 AC04: what `document` says about the content it
/// was written under, against what this session announces and provides.
///
/// `announced` is the signature this session announces
/// ([`super::selection::content_signature`] — the base fingerprint for a
/// stock session, the mounted signature for a modded one), and `provided`
/// the content ids it can serve ([`provided_ids`]). The comparison is
/// read-only: the document is borrowed, nothing is repaired, removed or
/// rewritten — what the save states is reported, never adjusted.
#[must_use]
pub fn save_dependency(
    document: &ProfileDocument,
    announced: ContentHash,
    provided: &BTreeSet<ContentId>,
) -> SaveDependencyReport {
    let announced = signature_fingerprint(announced);
    let signature = match document
        .fingerprints
        .iter()
        .find(|entry| entry.name == SAVE_CONTENT_FINGERPRINT)
    {
        Some(entry) if entry.hash == announced => SaveSignatureStatus::Matches,
        Some(entry) => SaveSignatureStatus::Differs {
            recorded: entry.hash,
            announced,
        },
        None => SaveSignatureStatus::Unrecorded,
    };
    let unprovided_blueprints = document
        .blueprints
        .iter()
        .filter(|id| !provided.contains(*id))
        .cloned()
        .collect();
    SaveDependencyReport {
        signature,
        unprovided_blueprints,
    }
}

#[cfg(test)]
mod tests {
    //! F53-D acceptance tests for the save half of the mod system. Every
    //! tree here is newly authored synthetic bytes below the system
    //! temporary directory; no original game data and no `CS_GAME_DIR`
    //! access.

    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_types::content::ContentKind;
    use cs_types::profile::{ProfileId, ProfileKind, Revision};

    use crate::save::library::{LibraryError, ProfileLibrary, slot_name};

    use super::super::{
        ContentOverride, ModSelection, content_signature, synthetic_base_ids,
        synthetic_blueprint_mod, synthetic_mount_request,
    };
    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f53-d-save-{label}-{}-{}",
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

    /// The synthetic bytes one declared override ships, as in F53-B's
    /// tests: the id plus a fixed tail, so every payload is distinct and
    /// traceable to its claim.
    fn payload_bytes(entry: &ContentOverride) -> Vec<u8> {
        let mut bytes = entry.target().as_str().as_bytes().to_vec();
        bytes.extend_from_slice(b"::synthetic payload");
        bytes
    }

    /// Writes one mod's declared sources below `root`.
    fn write_mod(root: &Temp, manifest: &super::super::ModManifest) {
        for entry in manifest.overrides() {
            let path = root.path().join(entry.source().as_str());
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            fs::write(path, payload_bytes(entry)).expect("bytes are written");
        }
    }

    /// The blueprint mod enabled under a shipped root, and its mount.
    fn blueprint_mount() -> (ModSelection, Temp, MountedMods, MountRequest) {
        let manifest = synthetic_blueprint_mod();
        let root = Temp::new(manifest.id().as_str());
        write_mod(&root, &manifest);
        let mut selection = ModSelection::new();
        selection
            .offer(manifest.clone(), root.path())
            .expect("the mod is offered");
        selection.enable(manifest.id()).expect("enabled");
        let request = synthetic_mount_request();
        let mounted = selection
            .mount(&request, base_fingerprint(), None)
            .expect("the blueprint mod mounts");
        (selection, root, mounted, request)
    }

    /// Every file below `root`, keyed by relative path, with its bytes —
    /// the snapshot "nothing was written" is asserted against.
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

    /// The blueprint id only the mod provides.
    fn zephyr() -> ContentId {
        ContentId::from_source(ContentKind::Blueprint, "synthetic.zephyr")
            .expect("the synthetic blueprint id is valid")
    }

    /// **The mark and the check.** A save written under an announced
    /// signature records it under `fingerprint.content`; on reopen the
    /// recorded fold is compared to the announced one — `Matches`,
    /// `Unrecorded` or `Differs` naming both — and every owned blueprint is
    /// checked against what the session serves. A satisfied report carries
    /// no diagnostic; a differing one names the recorded and announced
    /// folds and each unprovided id.
    #[test]
    fn accept_f53_d_the_recorded_mark_is_checked_against_the_announced_signature() {
        let base = base_fingerprint();
        let (_selection, _root, mounted, request) = blueprint_mount();
        let modded_signature = content_signature(Some(&mounted), base);
        let stock_signature = content_signature(None, base);
        assert_ne!(modded_signature, stock_signature);

        // The write half: the mark lands in the document's fingerprint
        // list under `content`, folded to the field's eight bytes, and a
        // second mark replaces rather than duplicates (the schema refuses
        // a repeated name).
        let mut document =
            ProfileDocument::synthetic(ProfileId::new(1).expect("a non-zero id"), Revision(1));
        mark_save_document(&mut document, modded_signature);
        mark_save_document(&mut document, modded_signature);
        document.blueprints.push(zephyr());
        let entries: Vec<u64> = document
            .fingerprints
            .iter()
            .filter(|entry| entry.name == SAVE_CONTENT_FINGERPRINT)
            .map(|entry| entry.hash)
            .collect();
        assert_eq!(entries, vec![signature_fingerprint(modded_signature)]);
        document.validate().expect("the marked document is valid");

        // Reopened under what it was written against: satisfied, nothing
        // to show.
        let provided = provided_ids(&request, Some(&mounted));
        assert!(provided.contains(&zephyr()));
        assert_eq!(
            provided,
            synthetic_base_ids().into_iter().chain([zephyr()]).collect(),
            "the provided set is exactly the base ids plus the winners"
        );
        let report = save_dependency(&document, modded_signature, &provided);
        assert_eq!(report.signature(), SaveSignatureStatus::Matches);
        assert!(report.is_satisfied());
        assert!(report.diagnostic_lines().is_empty());

        // Reopened under the stock signature — the mod disabled: the
        // recorded fold differs and the added blueprint is unprovided.
        // Both are named; nothing is adjusted.
        let stock_provided = provided_ids(&request, None);
        assert!(!stock_provided.contains(&zephyr()));
        let report = save_dependency(&document, stock_signature, &stock_provided);
        assert_eq!(
            report.signature(),
            SaveSignatureStatus::Differs {
                recorded: signature_fingerprint(modded_signature),
                announced: signature_fingerprint(stock_signature),
            }
        );
        assert_eq!(report.unprovided_blueprints(), &[zephyr()]);
        assert!(!report.is_satisfied());
        let lines = report.diagnostic_lines();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].contains(&format!("{:016x}", signature_fingerprint(modded_signature)))
                && lines[0].contains(&format!("{:016x}", signature_fingerprint(stock_signature))),
            "the verdict names both folds: {}",
            lines[0]
        );
        assert!(
            lines[1].contains(zephyr().as_str()),
            "the missing content is named: {}",
            lines[1]
        );

        // A save that never recorded makes no claim to fail — the verdict
        // is `Unrecorded`, not a match and not a mismatch.
        let unrecorded =
            ProfileDocument::synthetic(ProfileId::new(2).expect("a non-zero id"), Revision(1));
        let report = save_dependency(&unrecorded, stock_signature, &stock_provided);
        assert_eq!(report.signature(), SaveSignatureStatus::Unrecorded);
        assert!(report.is_satisfied());

        // The population mapping: any mount at all — cosmetic or
        // gameplay — writes into the modded population, and only a stock
        // session writes production.
        assert_eq!(save_population(Some(&mounted)), ProfileKind::Modded);
        assert_eq!(save_population(None), ProfileKind::Production);
    }

    /// **F53 AC04.** A save written while the blueprint mod was enabled is
    /// reopened through a fresh library after the mod is disabled: the
    /// document comes back whole and untouched — the check *reports* the
    /// difference rather than repairing it into a stock save — and the
    /// dependency is named. Re-enabling the mod reproduces the same
    /// signature, against which the same save is satisfied again. And the
    /// populations stay apart on the way: a modded document is refused by
    /// a synthetic library on write and on read.
    #[test]
    fn accept_f53_d_a_dependent_save_reopens_whole_and_reports_what_it_needs() {
        let base = base_fingerprint();
        let (selection, _root, mounted, request) = blueprint_mount();
        let modded_signature = content_signature(Some(&mounted), base);

        // The save is written under the announced signature, into the
        // population `save_population` maps a mounted session to.
        let population = Temp::new("population");
        let mut library = ProfileLibrary::open(population.path(), save_population(Some(&mounted)))
            .expect("the modded population opens");
        let mut document =
            ProfileDocument::synthetic(ProfileId::new(1).expect("a non-zero id"), Revision(1));
        document.kind = save_population(Some(&mounted));
        document.blueprints.push(zephyr());
        mark_save_document(&mut document, modded_signature);
        let (id, written) = library.create(document).expect("the save is written");
        let on_disk = snapshot(population.path());
        drop(library);

        // The mod is disabled; the session announces the stock fingerprint
        // and serves only the base ids.
        let stock_signature = content_signature(None, base);
        let stock_provided = provided_ids(&request, None);

        // Reopened through a fresh library — the state a restart sees.
        // The document is identical to the one written, the slot carries
        // no recovery warnings, and the check reports the dependency
        // instead of removing it.
        let reopened = ProfileLibrary::open(population.path(), ProfileKind::Modded)
            .expect("the population reopens");
        let loaded = reopened.load(id).expect("the dependent save reads");
        assert!(
            loaded.warnings.is_empty(),
            "no recovery ran: {:?}",
            loaded.warnings
        );
        let document = loaded.document.expect("the document is present");
        assert_eq!(document, written, "the save is reopened byte-identical");

        let report = save_dependency(&document, stock_signature, &stock_provided);
        assert_eq!(
            report.signature(),
            SaveSignatureStatus::Differs {
                recorded: signature_fingerprint(modded_signature),
                announced: signature_fingerprint(stock_signature),
            }
        );
        assert_eq!(report.unprovided_blueprints(), &[zephyr()]);
        assert!(!report.is_satisfied());
        assert!(
            report
                .diagnostic_lines()
                .iter()
                .any(|line| line.contains(zephyr().as_str())),
            "the missing blueprint is named"
        );
        // Nothing was written back: the population is byte-for-byte the
        // state the write left, so a disabled mod never costs the save a
        // revision.
        assert_eq!(snapshot(population.path()), on_disk);

        // Re-enable the mod: the mount reproduces the same signature, and
        // against it the same unmodified document is satisfied — the
        // reopen check is a function of the content, not of the session
        // that wrote it.
        let remounted = selection
            .mount(&request, base, None)
            .expect("the re-enabled set mounts");
        assert_eq!(remounted.signature(), modded_signature);
        let report = save_dependency(
            &document,
            content_signature(Some(&remounted), base),
            &provided_ids(&request, Some(&remounted)),
        );
        assert_eq!(report.signature(), SaveSignatureStatus::Matches);
        assert!(report.is_satisfied());
        assert_eq!(snapshot(population.path()), on_disk);

        // The population wall holds in both directions: the modded
        // document cannot be *written* into another population, and the
        // same slot planted under a synthetic library is refused on read
        // as foreign rather than adopted.
        let foreign_write = Temp::new("foreign-write");
        let mut synthetic = ProfileLibrary::open(foreign_write.path(), ProfileKind::Synthetic)
            .expect("the synthetic population opens");
        let error = synthetic
            .create(written.clone())
            .expect_err("a modded document is foreign to a synthetic library");
        assert!(matches!(error, LibraryError::ForeignDocument { .. }));

        let planted = Temp::new("foreign-read");
        fs::create_dir_all(planted.path().join(slot_name(id))).expect("the slot is planted");
        for (name, bytes) in snapshot(&population.path().join(slot_name(id))) {
            fs::write(planted.path().join(slot_name(id)).join(&name), bytes)
                .expect("the planted file is written");
        }
        let synthetic =
            ProfileLibrary::open(planted.path(), ProfileKind::Synthetic).expect("opens");
        let error = synthetic
            .load(id)
            .expect_err("a modded slot under a synthetic library is refused");
        assert!(
            matches!(error, LibraryError::ForeignDocument { .. }),
            "the foreign document is refused: {error}"
        );
    }
}
