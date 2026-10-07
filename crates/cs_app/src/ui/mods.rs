//! Mod screen projection and the lobby compatibility fold (F53-C).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-C`. The selection truth is
//! [`cs_content::mods::ModSelection`], owned by the host's producer; this
//! module is the view's projection of it plus the one place the mounted
//! set's compatibility signature enters `cs_net` vocabulary
//! ([`lobby_compatibility`]). No game state lives here: a view is rebuilt
//! from a selection and a mount outcome, never mutated into one.
//!
//! [`ModsView::project`] turns a selection and the mount it produced into
//! display rows and notices. A refused mount keeps its
//! [`cs_content::mods::MountError::code`] and message verbatim — the
//! diagnosis is a property of the projection, not of a renderer — and the
//! mount's full [`cs_content::mods::mount_to_text`] report is carried for
//! the diagnostics pane.
//!
//! [`lobby_compatibility`] answers what the session announces on the wire
//! (F53 AC03): a stock session announces the base installation fingerprint;
//! a mounted session announces [`cs_content::mods::MountedMods::signature`],
//! which covers the plan and the measured payload bytes, so a tuning mod
//! can never silently share a lobby with stock content — the handshake
//! refuses the pair as `ContentMismatch` before launch.
//!
//! No widget layout or input is wired (that is the front-end's stage);
//! notices carry their stable code and the designed English fallback only.

use cs_content::mods::{
    AvailableMod, ModManifest, ModModification, ModSelection, ModVersion, MountError, MountedMods,
    content_signature, mount_to_text,
};
use cs_net::compat::Compatibility;
use cs_types::asset_id::ModId;
use cs_types::evidence::ContentHash;

/// The `cs_net` handshake record a session announces, with the F53-B
/// compatibility signature folded into `content_sha256` (AC03).
///
/// `rules_sha256` stays the caller's own rules hash; `base_fingerprint` is
/// the same installation fingerprint the mount was measured against, so a
/// stock session announces exactly the bytes it runs and a mounted session
/// announces the domain-separated signature of its plan and payload bytes.
///
/// The wire's `mods` field names enabled mods as *catalog content ids* —
/// and no [`cs_types::content::ContentKind`] names a mod yet, so this stage
/// leaves it empty rather than mislabel a mod as another kind. Nothing is
/// lost: the signature already carries the enabled set and its bytes, so
/// the gate is complete; the list is display information pending the
/// vocabulary (filed as a follow-up).
#[must_use]
pub fn lobby_compatibility(
    rules_sha256: ContentHash,
    base_fingerprint: ContentHash,
    mounted: Option<&MountedMods>,
) -> Compatibility {
    Compatibility {
        rules_sha256,
        content_sha256: content_signature(mounted, base_fingerprint),
        mods: Vec::new(),
    }
}

/// One row of the mod screen: a discovered mod and the mount's verdict on
/// it. `load_position` and `modification` are set only when the enabled
/// set mounted — a disabled or refused mod has no order to report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModRow {
    /// The mod's id.
    pub id: ModId,
    /// Its display name, from the manifest.
    pub name: String,
    /// Its declared version.
    pub version: ModVersion,
    /// Whether the selection enables it.
    pub enabled: bool,
    /// Its position in the mounted load order, if mounted.
    pub load_position: Option<usize>,
    /// Its modification class under the mounted plan, if mounted.
    pub modification: Option<ModModification>,
}

/// A line the mod screen shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModsNotice {
    /// The mount was refused. `code` is the stable
    /// [`MountError::code`] for machine-readable reports; `detail` is the
    /// error's own message, verbatim.
    MountRefused {
        /// The stable refusal code.
        code: &'static str,
        /// The refusal message.
        detail: String,
    },
    /// One concrete plan problem inside a refused mount, kept separate
    /// from the summary so the screen can list each.
    PlanProblem {
        /// The stable problem code.
        code: &'static str,
        /// The problem message.
        detail: String,
    },
    /// A mounted session marks its saves, replays and handshakes for
    /// gameplay reasons (F53 non-negotiable 3) — shown so the operator
    /// knows the run is modified before they join anything.
    SessionMarked,
}

impl ModsNotice {
    /// The designed English fallback line. Original strings are not
    /// involved; localization (F51) supplies the real text by `code`.
    pub fn fallback_text(&self) -> String {
        match self {
            Self::MountRefused { code, detail } => format!("Mods are disabled ({code}): {detail}"),
            Self::PlanProblem { code, detail } => format!("Mod plan problem ({code}): {detail}"),
            Self::SessionMarked => {
                "This session is modified: saves, replays and multiplayer joins are marked"
                    .to_owned()
            }
        }
    }
}

/// The view's projection of a [`ModSelection`] and the mount it produced.
#[derive(Clone, Debug, Default)]
pub struct ModsView {
    rows: Vec<ModRow>,
    notices: Vec<ModsNotice>,
    signature: Option<ContentHash>,
    mount_report: Option<String>,
}

impl ModsView {
    /// An empty view.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Projects a selection and its mount outcome into rows, notices and
    /// the announced signature.
    ///
    /// `outcome` is `Some(Ok(mounted))` when the enabled set mounted,
    /// `Some(Err(error))` when [`cs_content::mods::mount_mods`] refused it —
    /// the refusal is surfaced per problem, never silently dropped — and
    /// `None` when the host has not mounted the current selection yet
    /// (rows still show what is enabled, with no load order to report).
    #[must_use]
    pub fn project(
        selection: &ModSelection,
        outcome: Option<Result<&MountedMods, &MountError>>,
    ) -> Self {
        let mounted = outcome.and_then(Result::ok);
        let mut view = Self {
            rows: selection
                .available()
                .map(|available| Self::row(selection, available, mounted))
                .collect(),
            notices: Vec::new(),
            signature: mounted.map(MountedMods::signature),
            mount_report: mounted.map(mount_to_text),
        };
        if let Some(Err(error)) = outcome {
            view.notices.push(ModsNotice::MountRefused {
                code: error.code(),
                detail: error.to_string(),
            });
            if let Some(plan) = error.plan_error() {
                view.notices.extend(plan.problems().iter().map(|problem| {
                    ModsNotice::PlanProblem {
                        code: problem.code(),
                        detail: problem.to_string(),
                    }
                }));
            }
        }
        if mounted.is_some_and(MountedMods::marks_sessions) {
            view.notices.push(ModsNotice::SessionMarked);
        }
        view
    }

    fn row(
        selection: &ModSelection,
        available: &AvailableMod,
        mounted: Option<&MountedMods>,
    ) -> ModRow {
        let manifest: &ModManifest = available.manifest();
        let planned = mounted.and_then(|mounted| mounted.plan().mod_entry(manifest.id()));
        ModRow {
            id: manifest.id().clone(),
            name: manifest.header().name().to_owned(),
            version: manifest.version(),
            enabled: selection.is_enabled(manifest.id()),
            load_position: planned.map(|entry| entry.position()),
            modification: planned.map(|entry| entry.modification()),
        }
    }

    /// The rows, in mod-id order (the selection's own order).
    pub fn rows(&self) -> &[ModRow] {
        &self.rows
    }

    /// The notices, in the order they were diagnosed.
    pub fn notices(&self) -> &[ModsNotice] {
        &self.notices
    }

    /// The compatibility signature the enabled set announced, if mounted.
    /// This is what [`lobby_compatibility`] puts in `content_sha256`.
    pub fn signature(&self) -> Option<ContentHash> {
        self.signature
    }

    /// The full mount report (`mount_to_text`), for the diagnostics pane.
    pub fn mount_report(&self) -> Option<&str> {
        self.mount_report.as_deref()
    }
}

#[cfg(test)]
mod tests {
    //! F53-C acceptance tests for the projection and the lobby fold.
    //! Every tree and payload is newly authored synthetic bytes below the
    //! system temporary directory; no original game data and no
    //! `CS_GAME_DIR` access.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_content::mods::{
        ModManifest, ModSelection, MountedMods, synthetic_conflicting_mods,
        synthetic_mount_request, synthetic_tuning_mod,
    };
    use cs_net::compat::{
        ClientHello, HandshakeReject, PROTOCOL_VERSION, SessionParameters, evaluate_hello,
    };

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f53-c-ui-{label}-{}-{}",
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

    /// Writes one manifest's declared sources below `root`, so the mount's
    /// measured digests are over real files.
    fn write_mod(root: &Temp, manifest: &ModManifest) {
        for entry in manifest.overrides() {
            let path = root.path().join(entry.source().as_str());
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            let mut bytes = entry.target().as_str().as_bytes().to_vec();
            bytes.extend_from_slice(b"::synthetic payload");
            fs::write(path, bytes).expect("bytes are written");
        }
    }

    /// The designed installation fingerprint the test mounts against.
    fn base_fingerprint() -> ContentHash {
        ContentHash::from_bytes([7u8; 32])
    }

    /// The designed rules hash the handshake also compares.
    fn rules() -> ContentHash {
        ContentHash::from_bytes([9u8; 32])
    }

    /// One manifest under its own shipped root, enabled.
    fn enabled(manifest: &ModManifest) -> (ModSelection, Temp) {
        let root = Temp::new(manifest.id().as_str());
        write_mod(&root, manifest);
        let mut selection = ModSelection::new();
        selection
            .offer(manifest.clone(), root.path())
            .expect("offered");
        selection.enable(manifest.id()).expect("enabled");
        (selection, root)
    }

    /// Mounts a selection the way the host does.
    fn mount(selection: &ModSelection) -> MountedMods {
        selection
            .mount(&synthetic_mount_request(), base_fingerprint(), None)
            .expect("the fixture set mounts")
    }

    fn hello(compatibility: &Compatibility) -> ClientHello {
        ClientHello {
            protocol: PROTOCOL_VERSION,
            compatibility: compatibility.clone(),
        }
    }

    fn parameters(compatibility: &Compatibility) -> SessionParameters {
        SessionParameters {
            compatibility: compatibility.clone(),
        }
    }

    /// **F53 AC03, the acceptance scenario.** A stock lobby announces the
    /// base fingerprint; a session running a tuning mod announces the
    /// mount signature. The handshake refuses the pair in both directions
    /// with `ContentMismatch` naming both signatures, and admits either
    /// side against itself.
    #[test]
    fn accept_f53_c_a_tuning_mod_changes_the_lobby_signature_and_a_stock_lobby_refuses_it() {
        let tuning = synthetic_tuning_mod();
        let (selection, _root) = enabled(&tuning);
        let mounted = mount(&selection);
        let base = base_fingerprint();

        let stock = lobby_compatibility(rules(), base, None);
        let modded = lobby_compatibility(rules(), base, Some(&mounted));
        assert_eq!(stock.content_sha256, base);
        assert_eq!(modded.content_sha256, mounted.signature());
        assert_ne!(stock.content_sha256, modded.content_sha256);

        // A modded client cannot join the stock lobby: the refusal names
        // both signatures, not a bare disconnect.
        let error = evaluate_hello(&parameters(&stock), &hello(&modded))
            .expect_err("a tuning mod cannot join a stock lobby");
        assert_eq!(
            error,
            HandshakeReject::ContentMismatch {
                expected: base,
                offered: mounted.signature()
            },
            "the refusal identifies the incompatibility"
        );
        assert!(
            error.to_string().contains("content mismatch"),
            "the reason is readable: {error}"
        );

        // Nor can a stock client join the modded lobby.
        let error = evaluate_hello(&parameters(&modded), &hello(&stock))
            .expect_err("a stock client cannot join a modded lobby");
        assert_eq!(
            error,
            HandshakeReject::ContentMismatch {
                expected: mounted.signature(),
                offered: base
            }
        );

        // Identical sets admit each other.
        evaluate_hello(&parameters(&modded), &hello(&modded)).expect("modded admits modded");
        evaluate_hello(&parameters(&stock), &hello(&stock)).expect("stock admits stock");
    }

    /// **Projection.** The view carries the selection's rows with the
    /// mount's verdict — positions, the modification class, the announced
    /// signature and the marking notice — and surfaces a refused mount as
    /// its own code plus every plan problem, verbatim.
    #[test]
    fn accept_f53_c_the_view_projects_rows_refusals_and_the_marking_notice() {
        let (repaint, bright) = synthetic_conflicting_mods();
        let tuning = synthetic_tuning_mod();
        let mut selection = ModSelection::new();
        let mut roots = Vec::new();
        for manifest in [&repaint, &bright, &tuning] {
            let root = Temp::new(manifest.id().as_str());
            write_mod(&root, manifest);
            selection
                .offer(manifest.clone(), root.path())
                .expect("offered");
            selection.enable(manifest.id()).expect("enabled");
            roots.push(root);
        }
        let mounted = mount(&selection);

        let view = ModsView::project(&selection, Some(Ok(&mounted)));
        assert_eq!(view.rows().len(), 3);
        let tuning_row = view
            .rows()
            .iter()
            .find(|row| row.id == *tuning.id())
            .expect("the tuning mod has a row");
        assert!(tuning_row.enabled);
        assert_eq!(
            tuning_row.load_position,
            Some(
                mounted
                    .plan()
                    .mod_entry(tuning.id())
                    .expect("planned")
                    .position()
            )
        );
        assert_eq!(tuning_row.modification, Some(ModModification::Gameplay));
        // Rows that mounted but had no gameplay overrides stay cosmetic.
        let bright_row = view
            .rows()
            .iter()
            .find(|row| row.id == *bright.id())
            .expect("a row");
        assert_eq!(bright_row.modification, Some(ModModification::CosmeticOnly));
        assert!(
            view.notices().contains(&ModsNotice::SessionMarked),
            "the marking verdict is on screen"
        );
        assert_eq!(view.signature(), Some(mounted.signature()));
        // The wire record and the view agree about what is announced.
        assert_eq!(
            lobby_compatibility(rules(), base_fingerprint(), Some(&mounted)).content_sha256,
            view.signature().expect("announced")
        );
        assert!(
            view.mount_report()
                .is_some_and(|report| report.contains(mounted.signature().to_hex().as_str())),
            "the diagnostics pane holds the full report"
        );

        // A refused mount projects the refusal and every plan problem.
        let mut broken = ModSelection::new();
        broken
            .offer(repaint.clone(), Temp::new("unused").path())
            .expect("offered");
        broken.enable(repaint.id()).expect("enabled");
        let error = broken
            .mount(&synthetic_mount_request(), base_fingerprint(), None)
            .expect_err("a required dependency missing from the set is refused");
        let refused = ModsView::project(&broken, Some(Err(&error)));
        assert!(refused.signature().is_none());
        assert!(refused.notices().iter().any(|notice| matches!(
            notice,
            ModsNotice::MountRefused {
                code: "plan_refused",
                ..
            }
        )));
        assert!(
            refused.notices().iter().any(|notice| matches!(
                notice,
                ModsNotice::PlanProblem {
                    code: "missing_dependency",
                    ..
                }
            )),
            "the plan's own problem codes surface: {:?}",
            refused.notices()
        );
        assert!(
            refused
                .notices()
                .iter()
                .all(|notice| !notice.fallback_text().is_empty())
        );

        // Before any mount, the rows still say what is enabled.
        let pending = ModsView::project(&selection, None);
        assert!(pending.notices().is_empty());
        assert!(pending.rows().iter().all(|row| row.enabled));
        assert!(pending.rows().iter().all(|row| row.load_position.is_none()));
        assert_eq!(pending.signature(), None);
    }
}
