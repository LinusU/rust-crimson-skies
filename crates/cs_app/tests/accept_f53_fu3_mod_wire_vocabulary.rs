//! F53-FU3 acceptance: the enabled-mod vocabulary the handshake announces.
//!
//! `Compatibility.mods` carries [`ModId`]s — the mod-manifest identity —
//! not catalog ids, and `lobby_compatibility` populates it from the mounted
//! plan's load order. The list stays informational: the content signature
//! remains the launch gate, so two different mounted sets refuse on
//! `ContentMismatch` as before, while an equal-signature offer with a
//! differing list still reaches `ModSetMismatch` and names the mod id.
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! follow-up `F53-FU3-MOD-KIND`; finding
//! `docs/findings/2026-10-07-f53-c-selection-diagnostics-and-private-export.md`.
//! Every tree and byte here is newly authored synthetic data below the
//! system temporary directory; no original game data and no `CS_GAME_DIR`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::ui::mods::{lobby_compatibility, mount_selection};
use cs_content::mods::{
    ContentOverride, ModManifest, ModSelection, synthetic_blueprint_mod, synthetic_mount_request,
};
use cs_net::compat::{
    ClientHello, HandshakeReject, PROTOCOL_VERSION, SessionParameters, evaluate_hello,
};
use cs_types::evidence::ContentHash;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A disposable directory, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f53-fu3-mod-wire-{label}-{}-{}",
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

/// A designed rules hash: a marker, not a digest of anything.
fn rules_fingerprint() -> ContentHash {
    ContentHash::from_bytes([0x52; 32])
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

/// Offers and enables `manifest`, then mounts the enabled set through the
/// host's own mount path.
fn mounted_mod(manifest: &ModManifest) -> cs_content::mods::MountedMods {
    let root = Temp::new(manifest.id().as_str());
    write_mod(root.path(), manifest);
    let mut selection = ModSelection::new();
    selection
        .offer(manifest.clone(), root.path())
        .expect("the mod is offered");
    selection.enable(manifest.id()).expect("enabled");
    mount_selection(&selection, &synthetic_mount_request(), base_fingerprint())
        .expect("the mod mounts")
}

fn session(compatibility: cs_net::compat::Compatibility) -> SessionParameters {
    SessionParameters { compatibility }
}

fn hello(compatibility: cs_net::compat::Compatibility) -> ClientHello {
    ClientHello {
        protocol: PROTOCOL_VERSION,
        compatibility,
    }
}

/// **F53-FU3.** The announced mod list is populated under the mod-id
/// vocabulary: a mounted session's `Compatibility.mods` names the enabled
/// [`ModId`] in plan order — not a catalog id borrowed from another
/// vocabulary — and an unmounted session names none. Two sessions whose
/// enabled sets differ produce different signatures and refuse on
/// `ContentMismatch` (the gate is unchanged), and an equal-signature offer
/// whose list differs still reaches `ModSetMismatch` naming the mod id.
#[test]
fn accept_f53_fu3_enabled_mods_are_named_by_mod_id_on_the_wire() {
    let rules = rules_fingerprint();
    let base = base_fingerprint();

    // Stock announces no mods; a mounted session announces the manifest's
    // own id in the plan's load order.
    let manifest = synthetic_blueprint_mod();
    let mounted = mounted_mod(&manifest);
    let stock = lobby_compatibility(rules, base, None);
    let modded = lobby_compatibility(rules, base, Some(&mounted));
    assert_eq!(stock.mods, Vec::new(), "stock enables no mods");
    assert_eq!(
        modded.mods,
        mounted.plan().order().to_vec(),
        "the wire list is the plan's own order"
    );
    assert_eq!(modded.mods, [manifest.id().clone()]);

    // Two different enabled sets still refuse on the content signature —
    // the gate is the signature, not the list.
    let Err(reject) = evaluate_hello(&session(stock), &hello(modded.clone())) else {
        panic!("a modded hello against a stock session must be refused");
    };
    assert!(
        matches!(reject, HandshakeReject::ContentMismatch { .. }),
        "different mounted sets refuse on the signature: {reject:?}"
    );

    // An equal-signature offer whose *list* differs still reaches the
    // mod-set gate and names the differing mod id.
    let mut listed = modded.clone();
    listed.mods.clear();
    let Err(HandshakeReject::ModSetMismatch {
        missing,
        unexpected,
    }) = evaluate_hello(&session(modded), &hello(listed))
    else {
        panic!("a list that drops the enabled mod must be refused");
    };
    assert_eq!(missing.ids(), [manifest.id().clone()]);
    assert!(unexpected.is_empty());
}
