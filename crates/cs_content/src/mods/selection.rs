//! Selection, session wiring, diagnostics and private export (F53-C).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-C`; shared contract `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F53-A says what a set of mods *means* and F53-B mounts it safely. This
//! module is the stage that wires both into their actual producer and their
//! actual consumers:
//!
//! * **Producer — [`ModSelection`].** The host's list of discovered mods
//!   (a [`ModManifest`] plus the directory it ships under) and which of them
//!   are enabled. The selection produces the [`ModSet`] and the
//!   [`MountEnvironment`] a mount consumes together, so an enabled mod can
//!   never lose its root on the way to [`mount_mods`], and
//!   [`mount_request`] supplies `MountRequest::base_ids` from the real
//!   catalog ([`crate::catalog::Catalog::sorted_ids`]) rather than from a
//!   fixture list.
//! * **Consumer — content session.** [`open_mod_session`] hands
//!   [`MountedMods::mounts`] to a [`SessionBuilder`] under a
//!   [`ResolveContext`] that has opted into the plan's
//!   [`cs_types::asset_id::ModStack`], so the payload mounts can only ever
//!   serve a session that selected those mods (F04 non-negotiable 2). The
//!   whole open is atomic: a failure drops the partial builder, so a retry
//!   is the same call again and teardown is
//!   [`ContentSession::close`]'s owned release.
//! * **Consumer — lobby handshake.** [`content_signature`] is the
//!   `content_sha256` a session announces: the base installation
//!   fingerprint for a stock session, [`MountedMods::signature`] for a
//!   mounted one. A tuning mod's signature covers the resolved payload
//!   bytes, so it differs from the stock fingerprint and the F54
//!   compatibility gate refuses the mismatched pair before launch
//!   (F53 AC03). The record is assembled in `cs_app::ui::mods`, the only
//!   place `cs_net` vocabulary may be named (`cs_content` may not depend on
//!   `cs_net`).
//! * **Diagnostics — [`mount_to_text`].** One deterministic report over
//!   the mount: the plan, the signature, the measured payloads and every
//!   entry the walks refused.
//! * **Private export — [`export_mounted_mods`].** Writes each *winning*
//!   payload's bytes — read from its own mod root, digest-checked — below a
//!   private [`ExportDirectory`], plus a report that names the original
//!   dependencies the export deliberately does not contain (F53
//!   non-negotiable 4: export custom blueprints and manifests *without*
//!   copying retail bytes, and explain the unresolved original
//!   dependencies).
//!
//! # Designed, not original
//!
//! The original game's mod support is unmeasured (F53 "Research
//! boundary"; F53-D). The selection, the report layout, the export layout
//! and the fixtures here are newly authored project design carrying
//! designed provenance; nothing in this module is evidence about the
//! original game.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use cs_assets::mods::ModMountError;
use cs_assets::vfs::{
    ContentSession, ExportDirectory, ExportError, ExportedFile, ReadError, SessionBuilder,
    SessionError,
};
use cs_types::asset_id::{ModId, ResolveContext};
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;

use super::manifest::{ModManifest, ModVersion};
use super::mount::{
    MountEnvironment, MountError, MountedMods, MountedPayload, ProgramValidator, mount_mods,
};
use super::overrides::OverrideAction;
use super::{ModSet, MountRequest, plan_to_text};
use crate::catalog::Catalog;

/// One discovered mod: the manifest a caller already holds and the
/// directory it ships under.
///
/// There is deliberately no manifest *file* reader: the F53 sheet never
/// specifies an on-disk manifest format, and inventing one would be
/// unmeasured vocabulary (F53 "Research boundary"). The caller supplies
/// the typed record and the root it belongs to; how the record was
/// produced stays the caller's concern.
#[derive(Clone, Debug)]
pub struct AvailableMod {
    manifest: ModManifest,
    root: PathBuf,
}

impl AvailableMod {
    /// The mod's manifest.
    pub fn manifest(&self) -> &ModManifest {
        &self.manifest
    }

    /// The directory the mod's payloads ship under.
    pub fn root(&self) -> &PathBuf {
        &self.root
    }
}

/// Why a selection change was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionError {
    /// A second discovery offered a mod id the selection already holds.
    /// Two different roots for one id would make the mounted bytes depend
    /// on which was offered last, so the duplicate is refused, never
    /// merged.
    DuplicateModId {
        /// The repeated id.
        id: ModId,
    },
    /// An enable named a mod that was never offered. Silently ignoring it
    /// would leave a selection that looks enabled but mounts nothing.
    UnknownMod {
        /// The id that is not available.
        id: ModId,
    },
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateModId { id } => write!(f, "mod {id} is already in the selection"),
            Self::UnknownMod { id } => write!(f, "mod {id} was never offered to the selection"),
        }
    }
}

impl std::error::Error for SelectionError {}

/// The producer half of F53-C: which discovered mods are enabled, and the
/// [`ModSet`] plus [`MountEnvironment`] a mount needs for exactly them.
///
/// The truth lives here, not in a UI: a view projects a selection, it does
/// not own one (`AGENTS.md`: no game state hidden in UI code).
#[derive(Clone, Debug, Default)]
pub struct ModSelection {
    available: BTreeMap<ModId, AvailableMod>,
    enabled: BTreeMap<ModId, ()>,
}

impl ModSelection {
    /// An empty selection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Offers one discovered mod: its manifest and the root it ships under.
    ///
    /// # Errors
    ///
    /// [`SelectionError::DuplicateModId`] when the id is already offered.
    pub fn offer(
        &mut self,
        manifest: ModManifest,
        root: impl Into<PathBuf>,
    ) -> Result<(), SelectionError> {
        let id = manifest.id().clone();
        if self.available.contains_key(&id) {
            return Err(SelectionError::DuplicateModId { id });
        }
        self.available.insert(
            id,
            AvailableMod {
                manifest,
                root: root.into(),
            },
        );
        Ok(())
    }

    /// Enables one offered mod.
    ///
    /// # Errors
    ///
    /// [`SelectionError::UnknownMod`] when `id` was never offered.
    pub fn enable(&mut self, id: &ModId) -> Result<(), SelectionError> {
        if !self.available.contains_key(id) {
            return Err(SelectionError::UnknownMod { id: id.clone() });
        }
        self.enabled.insert(id.clone(), ());
        Ok(())
    }

    /// Disables one mod; returns whether it was enabled.
    pub fn disable(&mut self, id: &ModId) -> bool {
        self.enabled.remove(id).is_some()
    }

    /// Disables every mod (the selection teardown).
    pub fn clear(&mut self) {
        self.enabled.clear();
    }

    /// Every offered mod, in mod-id order.
    pub fn available(&self) -> impl Iterator<Item = &AvailableMod> {
        self.available.values()
    }

    /// The offered entry for `id`, if any.
    pub fn available_mod(&self, id: &ModId) -> Option<&AvailableMod> {
        self.available.get(id)
    }

    /// Whether `id` is enabled.
    pub fn is_enabled(&self, id: &ModId) -> bool {
        self.enabled.contains_key(id)
    }

    /// The enabled mod ids, in mod-id order.
    pub fn enabled(&self) -> impl Iterator<Item = &ModId> {
        self.enabled.keys()
    }

    /// The [`ModSet`] the enabled mods form, exactly as [`mount_mods`]
    /// wants it.
    pub fn enabled_set(&self) -> ModSet {
        ModSet::new(
            self.enabled
                .keys()
                .map(|id| self.available[id].manifest.clone())
                .collect(),
        )
    }

    /// The [`MountEnvironment`] the enabled mods need: the base fingerprint
    /// plus one declared root per enabled mod, so [`mount_mods`] can never
    /// meet a planned mod whose root was forgotten.
    pub fn environment<'a>(&self, base_fingerprint: ContentHash) -> MountEnvironment<'a> {
        let mut environment = MountEnvironment::new(base_fingerprint);
        for id in self.enabled.keys() {
            environment = environment.with_root(id.clone(), self.available[id].root.clone());
        }
        environment
    }

    /// Plans and mounts the enabled set under `request` against
    /// `base_fingerprint` — the producer path in one call: set, roots and
    /// validator cannot disagree because the selection supplies all three.
    ///
    /// # Errors
    ///
    /// Every [`MountError`] of [`mount_mods`], propagated unchanged: a plan
    /// problem, a missing or unreadable root, an unsafe or unshipped
    /// source, a measured budget, an unvalidated or refused program, or a
    /// payload read failure.
    pub fn mount(
        &self,
        request: &MountRequest,
        base_fingerprint: ContentHash,
        validator: Option<&dyn ProgramValidator>,
    ) -> Result<MountedMods, MountError> {
        let mut environment = self.environment(base_fingerprint);
        if let Some(validator) = validator {
            environment = environment.with_program_validator(validator);
        }
        mount_mods(&self.enabled_set(), request, &environment)
    }
}

/// A [`MountRequest`] whose `base_ids` are the real catalog's ids.
///
/// This is the producer wiring the F53-A note left open
/// ("`MountRequest::base_ids` … F53-B supplies it from the real catalog"):
/// the plan's `Add`-of-existing and `Replace`-of-missing checks then run
/// against what the installation's content claims to provide, not against
/// a fixture list. Custom bounds are [`MountRequest::with_limits`].
#[must_use]
pub fn mount_request(engine: ModVersion, catalog: &Catalog) -> MountRequest {
    MountRequest::new(engine, catalog.sorted_ids().into_iter().cloned())
}

/// The context a session resolving mod payloads must run under: `context`
/// opted into the plan's load order. Without it, every payload mount is
/// scope-skipped (`SkipReason::ModNotOptedIn`) and the mod can serve
/// nothing — opt-in is the point of the scope.
#[must_use]
pub fn modded_context(context: &ResolveContext, mounted: &MountedMods) -> ResolveContext {
    context.clone().with_mods(mounted.plan().mod_stack())
}

/// A [`SessionBuilder`] under [`modded_context`], ready for
/// [`mount_payloads`] and for the host's own sources.
#[must_use]
pub fn session_builder(context: &ResolveContext, mounted: &MountedMods) -> SessionBuilder {
    SessionBuilder::new(modded_context(context, mounted))
}

/// Hands every payload mount of `mounted` to `builder`, in load order.
///
/// Each mount is [`cs_types::asset_id::PrecedenceClass::Mod`], scoped to
/// its own [`ModId`] and cloned — the mount index is immutable, so the
/// clone serves the same members the walk indexed.
///
/// # Errors
///
/// [`SessionError`] from [`SessionBuilder::mount`], propagated with the
/// builder's own semantics: the mounts that already joined stay in, so the
/// caller can complete the missing one or drop the builder to release them
/// all. A repeated call is refused by the VFS
/// (`MountError::DuplicateMountId`) — it is not the retry path; rebuilding
/// through [`open_mod_session`] is.
pub fn mount_payloads(
    builder: &mut SessionBuilder,
    mounted: &MountedMods,
) -> Result<(), SessionError> {
    for mount in mounted.mounts() {
        builder.mount(mount.clone())?;
    }
    Ok(())
}

/// Opens a content session holding `mounted`'s payload mounts on a context
/// that opted into its plan — the consumer path in one call.
///
/// The open is atomic: a payload mount that fails drops the partial
/// builder, so no session ever holds half the set and a retry is the same
/// call again after the cause is fixed. Teardown is
/// [`ContentSession::close`]'s owned release of the mounts it reports.
///
/// The session deliberately holds **only** the payload mounts; the host
/// adds its installation mounts to the same builder before or after
/// [`mount_payloads`].
///
/// # Errors
///
/// [`SessionError`] naming the mount that could not join.
pub fn open_mod_session(
    context: &ResolveContext,
    mounted: &MountedMods,
) -> Result<ContentSession, SessionError> {
    let mut builder = session_builder(context, mounted);
    mount_payloads(&mut builder, mounted)?;
    Ok(builder.open())
}

/// The content signature a session running `mounted` announces — AC03's
/// handshake input.
///
/// A stock session (no mods enabled) announces the base installation
/// fingerprint itself; a mounted session announces
/// [`MountedMods::signature`], which folds that same fingerprint in with
/// the plan hash and the measured payload digests. The two can never agree
/// by construction: the signature is domain-separated from the bare
/// fingerprint, so a tuning mod not only changes the hash, it changes it
/// for a reason — different bytes in the same scheme, never an accidental
/// collision between "no mount" and "a mount".
#[must_use]
pub fn content_signature(
    mounted: Option<&MountedMods>,
    base_fingerprint: ContentHash,
) -> ContentHash {
    mounted.map_or(base_fingerprint, MountedMods::signature)
}

/// A stable multi-line rendering of a mount, for diagnostics and reports.
///
/// Derived entirely from the mount, in canonical order — the plan section
/// is [`plan_to_text`], then the signature, the measured totals, the
/// marking verdict, every mounted payload and every refused walk entry —
/// so two mounts of the same files render byte-for-byte identically.
#[must_use]
pub fn mount_to_text(mounted: &MountedMods) -> String {
    let mut out = plan_to_text(mounted.plan());
    out.push_str(&format!("signature {}\n", mounted.signature().to_hex()));
    out.push_str(&format!("measured_bytes {}\n", mounted.measured_bytes()));
    out.push_str(&format!("marks_sessions {}\n", mounted.marks_sessions()));
    out.push_str("payloads\n");
    for payload in mounted.payloads() {
        out.push_str(&format!(
            "  {} {} {} position {} source {} bytes {} sha256 {} effect {} validation {}\n",
            payload.target(),
            payload.mod_id(),
            payload_action(mounted, payload).label(),
            payload.position(),
            payload.source(),
            payload.size_bytes(),
            payload.sha256().to_hex(),
            payload.effect().label(),
            payload.validation().label(),
        ));
    }
    out.push_str("rejections\n");
    for (id, entry) in mounted.rejections() {
        out.push_str(&format!(
            "  {} {} {}\n",
            id,
            entry.host_relative.display(),
            entry.reason
        ));
    }
    out
}

/// One original dependency a private export cannot and must not contain:
/// a base-provided content id a mounted `Replace` takes over. The mod's
/// own bytes ship; the original bytes it stands in for never do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalDependency {
    /// The base content the mod replaces.
    pub target: ContentId,
    /// The mod that needs the original to exist.
    pub needed_by: ModId,
}

/// What [`export_mounted_mods`] wrote and what it deliberately did not.
#[derive(Clone, Debug)]
pub struct ModExport {
    /// Every payload file written, in content-id order of the payloads.
    pub files: Vec<ExportedFile>,
    /// The manifest/precedence report file (`mod-export.txt` at the export
    /// root).
    pub report: ExportedFile,
    /// The original dependencies the export does not satisfy: every
    /// winning `Replace` whose target the base game provides, named so the
    /// package can never pretend to be self-contained (F53 non-negotiable
    /// 4). A `Replace` of an id the *set* provides is not listed: the
    /// export contains the `Add` that satisfies it.
    pub original_dependencies: Vec<OriginalDependency>,
}

/// Why an export was refused or failed.
#[derive(Debug)]
pub enum ModExportError {
    /// A planned mod's root is not in the mount — defensive: `mount_mods`
    /// cannot return one.
    RootMissing {
        /// The mod whose root is missing.
        mod_id: ModId,
    },
    /// A mounted payload's declared source no longer resolves at its own
    /// root.
    Resolve {
        /// The mod that declared it.
        mod_id: ModId,
        /// The content id it serves.
        target: ContentId,
        /// The spelling that was asked for.
        spelling: String,
        /// Why the root refused it. Boxed like the VFS's own error
        /// payloads, so the error itself stays small.
        source: Box<ModMountError>,
    },
    /// A payload's bytes could not be read coherently — for example a file
    /// swapped after the walk, refused by the digest re-check rather than
    /// exported.
    Read {
        /// The mod that ships it.
        mod_id: ModId,
        /// The content id it serves.
        target: ContentId,
        /// Why the read failed.
        source: Box<ReadError>,
    },
    /// A file could not be written — an unsafe name, an export tree the
    /// guard refuses to descend into, or an existing target (exports never
    /// overwrite).
    Write {
        /// The export spelling that failed.
        name: String,
        /// Why the write failed.
        source: ExportError,
    },
}

impl fmt::Display for ModExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMissing { mod_id } => {
                write!(f, "mod {mod_id} is mounted but has no root")
            }
            Self::Resolve {
                mod_id,
                target,
                spelling,
                source,
            } => write!(
                f,
                "the payload mod {mod_id} ships for {target} no longer resolves {spelling:?}: \
                 {source}"
            ),
            Self::Read {
                mod_id,
                target,
                source,
            } => write!(
                f,
                "cannot read the payload mod {mod_id} ships for {target}: {source}"
            ),
            Self::Write { name, source } => {
                write!(f, "cannot write the export {name:?}: {source}")
            }
        }
    }
}

impl std::error::Error for ModExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resolve { source, .. } => Some(source),
            Self::Read { source, .. } => Some(source),
            Self::Write { source, .. } => Some(source),
            Self::RootMissing { .. } => None,
        }
    }
}

/// The report file's name at the export root.
pub const MOD_EXPORT_REPORT: &str = "mod-export.txt";

/// The deterministic body of [`MOD_EXPORT_REPORT`]: what the export
/// contains, and which original dependencies it leaves to the base
/// installation.
#[must_use]
pub fn mod_export_text(mounted: &MountedMods, request: &MountRequest) -> String {
    let mut out = String::new();
    out.push_str("mod-export\n");
    out.push_str(&format!("signature {}\n", mounted.signature().to_hex()));
    for entry in mounted.plan().mods() {
        out.push_str(&format!(
            "mod {}@{} position {} modification {}\n",
            entry.id(),
            entry.version(),
            entry.position(),
            entry.modification().label(),
        ));
        for dependency in entry.dependencies() {
            out.push_str(&format!(
                "  depends {} {} {} satisfied={}\n",
                dependency.strength.label(),
                dependency.mod_id,
                dependency.range,
                dependency.satisfied
            ));
        }
    }
    for payload in mounted.payloads() {
        out.push_str(&format!(
            "file {}/{} for {} bytes {} sha256 {}\n",
            payload.mod_id(),
            payload.source(),
            payload.target(),
            payload.size_bytes(),
            payload.sha256().to_hex(),
        ));
    }
    for dependency in original_dependencies(mounted, request) {
        out.push_str(&format!(
            "unresolved-original {} needed-by {} — base content the export deliberately does \
             not contain\n",
            dependency.target, dependency.needed_by,
        ));
    }
    out
}

/// The base-provided ids the mounted set takes over, in payload order.
fn original_dependencies(mounted: &MountedMods, request: &MountRequest) -> Vec<OriginalDependency> {
    mounted
        .payloads()
        .iter()
        .filter(|payload| {
            payload_action(mounted, payload) == OverrideAction::Replace
                && request.base_ids().contains(payload.target())
        })
        .map(|payload| OriginalDependency {
            target: payload.target().clone(),
            needed_by: payload.mod_id().clone(),
        })
        .collect()
}

/// The winning claim's action for one mounted payload.
fn payload_action(mounted: &MountedMods, payload: &MountedPayload) -> OverrideAction {
    mounted
        .plan()
        .precedence()
        .entry(payload.target())
        .map_or(OverrideAction::Replace, |entry| entry.action())
}

/// Exports the mounted set's own bytes and manifest — never a byte of the
/// base installation (F53 non-negotiable 4).
///
/// Every *winning* payload is read from its own mod root — digest-checked
/// by [`cs_assets::mods::ModRoot::read`], so a file swapped after the walk
/// is refused rather than exported — and written below `directory` as
/// `<mod-id>/<declared source>`. A shadowed claim serves nothing, so
/// nothing of it is written. The report file [`MOD_EXPORT_REPORT`] lists
/// every mounted mod, its dependencies, its exported files and the
/// original dependencies the export deliberately does not contain: a
/// `Replace` of base content depends on the installation providing it,
/// and copying retail textures or models into the package would break
/// source protection.
///
/// `directory` is an [`ExportDirectory`] opened against a session that
/// holds the payload mounts (open one with [`open_mod_session`]), so an
/// export root inside a mod root — or a path descending into one — is
/// refused by the same guard that protects the installation.
///
/// # Errors
///
/// [`ModExportError`]: a payload that no longer resolves or reads, or a
/// write the export guard refuses.
pub fn export_mounted_mods(
    mounted: &MountedMods,
    request: &MountRequest,
    directory: &ExportDirectory,
) -> Result<ModExport, ModExportError> {
    let mut files: Vec<ExportedFile> = Vec::with_capacity(mounted.payloads().len());
    for payload in mounted.payloads() {
        let mod_id = payload.mod_id().clone();
        let root = mounted
            .root(&mod_id)
            .ok_or_else(|| ModExportError::RootMissing {
                mod_id: mod_id.clone(),
            })?;
        let member =
            root.resolve(payload.source().as_str())
                .map_err(|source| ModExportError::Resolve {
                    mod_id: mod_id.clone(),
                    target: payload.target().clone(),
                    spelling: payload.source().as_str().to_owned(),
                    source: Box::new(source),
                })?;
        let bytes = root.read(member).map_err(|source| ModExportError::Read {
            mod_id: mod_id.clone(),
            target: payload.target().clone(),
            source: Box::new(source),
        })?;
        let name = format!("{}/{}", mod_id.as_str(), payload.source().as_str());
        files.push(
            directory
                .write(&name, &bytes)
                .map_err(|source| ModExportError::Write { name, source })?,
        );
    }
    let report = directory
        .write(
            MOD_EXPORT_REPORT,
            mod_export_text(mounted, request).as_bytes(),
        )
        .map_err(|source| ModExportError::Write {
            name: MOD_EXPORT_REPORT.to_owned(),
            source,
        })?;
    Ok(ModExport {
        files,
        report,
        original_dependencies: original_dependencies(mounted, request),
    })
}

#[cfg(test)]
mod tests {
    //! F53-C acceptance tests for the producer/consumer wiring. Every tree
    //! and every payload here is newly authored synthetic bytes below the
    //! system temporary directory; no original game data and no
    //! `CS_GAME_DIR` access.

    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_assets::vfs::{AttemptOutcome, ExportDirectory, SessionError, SkipReason};
    use cs_types::asset_id::{AssetKey, AssetVariant, MountNamespace};
    use cs_types::content::{CatalogElement, ContentKind, NormalizeState, Origin, Readiness};
    use cs_types::install::{ParseState, RelativePath};

    use super::super::{
        ContentOverride, PlanProblem, synthetic_base_ids, synthetic_conflicting_mods,
        synthetic_engine_version, synthetic_mount_request, synthetic_tuning_mod,
    };
    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f53-c-selection-{label}-{}-{}",
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

        fn read(&self, spelling: &str) -> Vec<u8> {
            fs::read(self.0.join(spelling)).expect("the export file is readable")
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
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
    fn write_mod(root: &Temp, manifest: &ModManifest) {
        for entry in manifest.overrides() {
            let path = root.path().join(entry.source().as_str());
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            fs::write(path, payload_bytes(entry)).expect("bytes are written");
        }
    }

    /// The designed base fingerprint the fixtures mount against.
    fn base_fingerprint() -> ContentHash {
        ContentHash::from_bytes([7u8; 32])
    }

    /// A catalog row a `Catalog` accepts, for the base ids `mount_request`
    /// is asked to mirror.
    fn catalog_element(id: &ContentId) -> CatalogElement {
        CatalogElement {
            kind: id.kind(),
            id: id.clone(),
            display_name: None,
            origin: Origin::SyntheticFixture,
            dependencies: Vec::new(),
            parse_state: ParseState::Parsed,
            normalize_state: NormalizeState::Normalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Ready,
            unsupported_reasons: Vec::new(),
            fingerprint: None,
        }
    }

    /// A catalog holding exactly `ids`.
    fn catalog_with(ids: &[ContentId]) -> Catalog {
        let mut catalog = Catalog::new();
        for id in ids {
            catalog
                .insert(catalog_element(id))
                .expect("the row inserts");
        }
        catalog
    }

    /// A selection holding the two conflicting cosmetic mods and the
    /// tuning mod, each under its own shipped root; the guards keep the
    /// roots alive and remove them on drop.
    fn fixture_selection() -> (
        ModSelection,
        Vec<Temp>,
        ModManifest,
        ModManifest,
        ModManifest,
    ) {
        let (repaint, bright) = synthetic_conflicting_mods();
        let tuning = synthetic_tuning_mod();
        let mut selection = ModSelection::new();
        let mut roots = Vec::new();
        for manifest in [&repaint, &bright, &tuning] {
            let root = Temp::new(manifest.id().as_str());
            write_mod(&root, manifest);
            selection
                .offer(manifest.clone(), root.path())
                .expect("the mod is offered once");
            roots.push(root);
        }
        (selection, roots, repaint, bright, tuning)
    }

    /// The three-mod selection, enabled and mountable.
    fn enabled_fixture() -> (
        ModSelection,
        Vec<Temp>,
        ModManifest,
        ModManifest,
        ModManifest,
    ) {
        let (mut selection, roots, repaint, bright, tuning) = fixture_selection();
        for manifest in [&repaint, &bright, &tuning] {
            selection
                .enable(manifest.id())
                .expect("an offered mod enables");
        }
        (selection, roots, repaint, bright, tuning)
    }

    /// A selection holding one manifest under its own shipped root.
    fn single_mod_selection(manifest: &ModManifest) -> (ModSelection, Temp) {
        let root = Temp::new(manifest.id().as_str());
        write_mod(&root, manifest);
        let mut selection = ModSelection::new();
        selection
            .offer(manifest.clone(), root.path())
            .expect("offered");
        selection.enable(manifest.id()).expect("enabled");
        (selection, root)
    }

    /// The fixture mount the tests share.
    fn fixture_mount(selection: &ModSelection) -> (MountedMods, MountRequest) {
        let request = synthetic_mount_request();
        let mounted = selection
            .mount(&request, base_fingerprint(), None)
            .expect("the fixture set mounts");
        (mounted, request)
    }

    /// **Producer.** The selection is the one place the enabled set and
    /// its roots live: enabling names a mod that was offered, disabling
    /// drops it from the *next* mount, and offering or enabling an id the
    /// selection does not know is an explicit refusal, never a silent
    /// merge.
    #[test]
    fn accept_f53_c_the_selection_mounts_what_it_enables_and_refuses_what_it_does_not_know() {
        let (mut selection, _roots, repaint, bright, tuning) = enabled_fixture();

        // Every enabled mod lands in the set and gets its declared root.
        assert_eq!(selection.enabled_set().len(), 3);
        let environment = selection.environment(base_fingerprint());
        for manifest in [&repaint, &bright, &tuning] {
            assert!(
                environment.root(manifest.id()).is_some(),
                "mod {} has a declared root",
                manifest.id()
            );
        }
        let (mounted, _request) = fixture_mount(&selection);
        assert_eq!(mounted.plan().order().len(), 3);

        // Enabling or offering an unknown id is refused, not absorbed.
        let unknown = ModId::new("synthetic.never-offered").expect("valid");
        assert_eq!(
            selection.enable(&unknown),
            Err(SelectionError::UnknownMod { id: unknown })
        );
        assert_eq!(
            selection.offer(tuning.clone(), Temp::new("other").path()),
            Err(SelectionError::DuplicateModId {
                id: tuning.id().clone()
            })
        );

        // Disabling the repaint without its required dependency is a plan
        // refusal — the dependency is still declared, so nothing silently
        // degrades.
        assert!(selection.disable(bright.id()));
        let error = selection
            .mount(&synthetic_mount_request(), base_fingerprint(), None)
            .expect_err("a required dependency missing from the set is refused");
        assert_eq!(error.code(), "plan_refused");
        assert!(
            error.plan_error().is_some_and(|plan| plan
                .problems()
                .iter()
                .any(|problem| matches!(problem, PlanProblem::MissingDependency { .. }))),
            "the plan names the missing dependency: {error}"
        );

        // Disabling the dependent too leaves a coherent set again, and the
        // new mount holds only what remains enabled — the tuning mod alone.
        assert!(selection.disable(repaint.id()));
        let (mounted, _) = fixture_mount(&selection);
        assert_eq!(mounted.plan().order().len(), 1);
        assert!(
            mounted
                .payloads()
                .iter()
                .all(|payload| payload.mod_id() != repaint.id()),
            "nothing of the disabled mod serves"
        );
    }

    /// **Producer → plan.** `mount_request` carries the real catalog's ids
    /// into `MountRequest::base_ids`, and the plan's Replace checks run
    /// against them: a tuning mod can replace `gun/synthetic.vulcan`
    /// because the catalog provides it, while a catalog that lacks it
    /// refuses the same manifest.
    #[test]
    fn accept_f53_c_the_mount_request_carries_the_real_catalogs_base_ids() {
        let catalog = catalog_with(&synthetic_base_ids());
        let request = mount_request(synthetic_engine_version(), &catalog);
        assert_eq!(
            request.base_ids(),
            &synthetic_base_ids().into_iter().collect::<BTreeSet<_>>(),
            "every catalog id reaches the request"
        );

        let (mut selection, _roots, _, _, tuning) = fixture_selection();
        selection
            .enable(tuning.id())
            .expect("the tuning mod enables");
        selection
            .mount(&request, base_fingerprint(), None)
            .expect("a replace of a provided id mounts");

        // The same set against a catalog that does not provide the gun is
        // a plan refusal naming exactly the missing id — the difference is
        // the request, not the mod.
        let mut missing = synthetic_base_ids();
        missing.retain(|id| id.kind() != ContentKind::Gun);
        let request = mount_request(synthetic_engine_version(), &catalog_with(&missing));
        let error = selection
            .mount(&request, base_fingerprint(), None)
            .expect_err("a replace of nothing cannot be planned");
        assert!(
            error.plan_error().is_some_and(|plan| plan
                .problems()
                .iter()
                .any(|problem| matches!(problem, PlanProblem::ReplaceOfMissingContent { .. }))),
            "the report names the missing base id: {error}"
        );
    }

    /// **F53 AC03, signature half.** A tuning mod changes the announced
    /// content signature and marks its session for gameplay; a cosmetic
    /// set still changes the signature (it changes the bytes served) even
    /// though it does not mark. A stock session announces the base
    /// fingerprint itself.
    #[test]
    fn accept_f53_c_a_tuning_mod_changes_the_signature_and_marks_the_session() {
        let base = base_fingerprint();
        assert_eq!(
            content_signature(None, base),
            base,
            "a stock session announces the installation fingerprint"
        );

        // Tuning: gameplay payload, marked session, changed signature.
        let tuning = synthetic_tuning_mod();
        let (selection, _root) = single_mod_selection(&tuning);
        let (mounted, _) = fixture_mount(&selection);
        assert!(mounted.marks_sessions());
        let signature = content_signature(Some(&mounted), base);
        assert_ne!(
            signature, base,
            "a gameplay mod's signature differs from stock"
        );
        assert_eq!(signature, mounted.signature());
        // The signature is a function of the measured bytes, so mounting
        // the same files twice signs identically.
        let (remounted, _) = fixture_mount(&selection);
        assert_eq!(remounted.signature(), signature);

        // Cosmetic: the session is unmarked but the signature still
        // changes — the gate is about identical bytes, not about marking.
        let (repaint, bright) = synthetic_conflicting_mods();
        let repaint_root = Temp::new(repaint.id().as_str());
        write_mod(&repaint_root, &repaint);
        let bright_root = Temp::new(bright.id().as_str());
        write_mod(&bright_root, &bright);
        let mut cosmetic = ModSelection::new();
        for (manifest, root) in [(&repaint, &repaint_root), (&bright, &bright_root)] {
            cosmetic
                .offer(manifest.clone(), root.path())
                .expect("offered");
            cosmetic.enable(manifest.id()).expect("enabled");
        }
        let (cosmetic_mount, _) = fixture_mount(&cosmetic);
        assert!(!cosmetic_mount.marks_sessions());
        assert_ne!(content_signature(Some(&cosmetic_mount), base), base);
    }

    /// **Consumer.** `open_mod_session` hands the payload mounts to a
    /// session opted into the plan: the mod's file resolves and reads
    /// through the ordinary digest-checked path, `close` reports exactly
    /// the mounts it released, and an asset the closed session resolved is
    /// refused by its successor — the generation guard keeps stale state
    /// from ever serving.
    #[test]
    fn accept_f53_c_the_session_serves_the_opted_in_set_and_the_teardown_releases_it() {
        let (selection, _roots, _repaint, _, tuning) = enabled_fixture();
        let (mounted, _) = fixture_mount(&selection);
        let key = AssetKey::new(
            MountNamespace::new("mod").expect("valid"),
            RelativePath::new("tuning/vulcan.toml").expect("valid"),
            AssetVariant::default(),
        );

        let session = open_mod_session(&ResolveContext::new(base_fingerprint()), &mounted)
            .expect("the payload mounts join the session");
        assert_eq!(session.mounts().count(), 3);
        let asset = session.resolve(&key).expect("the payload resolves");
        assert_eq!(asset.resolved().mount.as_str(), tuning.id().as_str());
        let bytes = session.read_all(&asset).expect("the payload reads");
        assert_eq!(
            bytes,
            payload_bytes(tuning.overrides().first().expect("one claim"))
        );

        // A second context — the same mounts, no opt-in — cannot see the
        // mod's file: the scope is what makes "enabled" mean something.
        let mut builder = SessionBuilder::new(ResolveContext::new(base_fingerprint()));
        mount_payloads(&mut builder, &mounted).expect("payloads mount");
        let stock = builder.open();
        let error = stock
            .resolve(&key)
            .expect_err("a context that did not opt in cannot resolve a mod file");
        match error {
            cs_assets::vfs::ResolveError::NotFound { trace, .. } => assert!(
                trace.attempts.iter().any(|attempt| matches!(
                    attempt.outcome,
                    AttemptOutcome::Skipped(SkipReason::ModNotOptedIn)
                )),
                "the trace says why: {trace}"
            ),
            other => panic!("expected NotFound, got {other}"),
        }
        stock.close();

        // The teardown is the owned release: exactly the three payload
        // mounts, in load order. A retry mounts cleanly, and the stale
        // asset the first session resolved cannot be read by its
        // successor.
        let teardown = session.close();
        let released: Vec<String> = teardown
            .released
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        let expected: Vec<String> = mounted
            .plan()
            .order()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        assert_eq!(released, expected);

        let retry = open_mod_session(&ResolveContext::new(base_fingerprint()), &mounted)
            .expect("a retry after close mounts again");
        let error = retry
            .read_all(&asset)
            .expect_err("an asset the closed session resolved is foreign");
        assert!(
            matches!(error, cs_assets::vfs::ReadError::ForeignSession { .. }),
            "the stale answer is refused: {error}"
        );
        let fresh = retry.resolve(&key).expect("the retry resolves anew");
        assert_eq!(retry.read_all(&fresh).expect("reads"), bytes);
    }

    /// **Error propagation.** A root that does not exist refuses the mount
    /// with the error naming the mod — nothing silently proceeds — and
    /// fixing the cause lets the same selection mount on retry. Mounting
    /// the payloads twice into one builder is refused by the session
    /// builder rather than merging, and `open_mod_session` is the atomic
    /// retry path that never returns a partial session.
    #[test]
    fn accept_f53_c_mount_failures_propagate_and_a_retry_mounts() {
        let tuning = synthetic_tuning_mod();
        let mut selection = ModSelection::new();
        let temp = Temp::new("missing");
        let missing_root = temp.path().join("not-here");
        selection
            .offer(tuning.clone(), &missing_root)
            .expect("offered");
        selection.enable(tuning.id()).expect("enabled");
        let request = synthetic_mount_request();

        let error = selection
            .mount(&request, base_fingerprint(), None)
            .expect_err("an unreadable root refuses the mount");
        assert_eq!(error.code(), "root_unavailable");
        assert!(
            error.to_string().contains(tuning.id().as_str()),
            "the refusal names the mod: {error}"
        );

        // Retry the same selection once the root exists — no stale state
        // carries over from the refused attempt.
        fs::create_dir_all(missing_root.join("tuning")).expect("dirs");
        fs::write(
            missing_root.join("tuning/vulcan.toml"),
            payload_bytes(tuning.overrides().first().expect("one claim")),
        )
        .expect("bytes");
        selection
            .mount(&request, base_fingerprint(), None)
            .expect("the retry mounts");

        // `mount_payloads` is not the retry path: a second call into the
        // same builder is refused, the mounts already inside stay, and a
        // fresh `open_mod_session` opens atomically.
        let (selection, _roots, _, _, _) = enabled_fixture();
        let (mounted, _) = fixture_mount(&selection);
        let mut builder = session_builder(&ResolveContext::new(base_fingerprint()), &mounted);
        mount_payloads(&mut builder, &mounted).expect("the first hand mounts");
        let error = mount_payloads(&mut builder, &mounted)
            .expect_err("a repeated mount is refused, not merged");
        assert!(
            matches!(error, SessionError::Mount(_)),
            "the builder's own refusal surfaces: {error}"
        );
        assert_eq!(builder.open().mounts().count(), 3);
        open_mod_session(&ResolveContext::new(base_fingerprint()), &mounted)
            .expect("the atomic open mounts the whole set");
    }

    /// **Diagnostics.** `mount_to_text` is deterministic for the same
    /// bytes and names the signature, the marking verdict, every mounted
    /// payload with its claim position and digest, and the rejections
    /// section — the mount's own report, not a summary that could hide a
    /// refusal.
    #[test]
    fn accept_f53_c_the_mount_report_is_deterministic_and_names_what_it_mounted() {
        let (selection, _roots, _, bright, _) = enabled_fixture();
        let (mounted, _) = fixture_mount(&selection);
        let text = mount_to_text(&mounted);
        let (remounted, _) = fixture_mount(&selection);
        assert_eq!(text, mount_to_text(&remounted));

        assert!(
            text.contains(&format!("signature {}", mounted.signature().to_hex())),
            "the signature is in the report:\n{text}"
        );
        assert!(text.contains("marks_sessions true"));
        assert!(text.contains("gun/synthetic.vulcan"));
        assert!(text.contains("synthetic.tuning-pack"));
        assert!(text.contains("tuning/vulcan.toml"));
        assert!(text.contains("effect gameplay"));
        assert!(text.contains("rejections"));
        // The precedence report is the plan's own: the contested panel
        // names its winner and its shadowed claim.
        assert!(text.contains(bright.id().as_str()));
    }

    /// **Diagnostics, refusal half.** A symbolic link the walk refuses is
    /// not silently absent: it lands in `rejections` and in the report with
    /// its host-relative path and reason.
    #[cfg(unix)]
    #[test]
    fn accept_f53_c_a_refused_walk_entry_appears_in_the_mount_report() {
        let outside = Temp::new("link-outside");
        fs::write(outside.path().join("hidden.png"), b"outside").expect("bytes");
        let (selection, _roots, _, bright, _) = enabled_fixture();
        let bright_root = selection
            .available_mod(bright.id())
            .expect("the mod is available")
            .root()
            .to_path_buf();
        std::os::unix::fs::symlink(
            outside.path().join("hidden.png"),
            bright_root.join("art/hidden-link.png"),
        )
        .expect("the link is created");

        let (mounted, _) = fixture_mount(&selection);
        assert!(
            mounted
                .rejections()
                .iter()
                .any(|(id, entry)| *id == bright.id()
                    && entry.host_relative.ends_with("art/hidden-link.png")),
            "the refused link is named: {:?}",
            mounted.rejections()
        );
        let text = mount_to_text(&mounted);
        assert!(text.contains("art/hidden-link.png"));
        assert!(text.contains("symbolic_link"));
    }

    /// **Private export.** The export writes only the mounted set's own
    /// bytes — every file under the winning mod's directory, nothing of a
    /// shadowed claim — and names the original dependencies it refuses to
    /// copy. A repeated export is refused by the target-exists guard
    /// rather than overwriting.
    #[test]
    fn accept_f53_c_export_writes_only_mod_bytes_and_names_the_original_dependencies() {
        let (selection, _roots, repaint, bright, tuning) = enabled_fixture();
        let (mounted, request) = fixture_mount(&selection);
        let export_root = Temp::new("export");
        let session = open_mod_session(&ResolveContext::new(base_fingerprint()), &mounted)
            .expect("the export's session opens");
        let directory = ExportDirectory::open(export_root.path(), &session)
            .expect("the export root is outside every mount");

        let export =
            export_mounted_mods(&mounted, &request, &directory).expect("the mounted set exports");

        // Only winning payloads are written, under <mod>/<source>. The
        // bright-panels claim is shadowed, so nothing under its directory.
        let written: Vec<String> = export
            .files
            .iter()
            .map(|file| {
                file.path
                    .strip_prefix(directory.root())
                    .expect("under the canonical export root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            written,
            vec![
                format!("{}/tuning/vulcan.toml", tuning.id().as_str()),
                format!("{}/art/panel.png", repaint.id().as_str()),
                format!("{}/art/stripe.png", repaint.id().as_str()),
            ],
            "the export holds exactly the winning payloads, in content-id order"
        );
        assert!(
            written
                .iter()
                .all(|name| !name.contains(bright.id().as_str())),
            "a shadowed claim exports nothing"
        );

        // Every file's bytes are the mod's own, digest-checked bytes —
        // the gun file is the tuning mod's payload, not base content.
        for manifest in [&repaint, &tuning] {
            for entry in manifest.overrides() {
                let spelling = format!("{}/{}", manifest.id().as_str(), entry.source().as_str());
                assert_eq!(
                    export_root.read(&spelling),
                    payload_bytes(entry),
                    "{spelling} holds the mod's own bytes"
                );
            }
        }

        // The original dependencies are named and never contained: both
        // `Replace` targets the base provides. The `Add` is self-contained
        // and is not listed.
        let deps: Vec<String> = export
            .original_dependencies
            .iter()
            .map(|dependency| dependency.target.as_str().to_owned())
            .collect();
        assert_eq!(
            deps,
            vec![
                "gun/synthetic.vulcan".to_owned(),
                "image/synthetic.hull-panel".to_owned()
            ],
            "the replaced base ids are named"
        );
        assert!(export.original_dependencies.iter().all(|dependency| {
            dependency.needed_by == *tuning.id() || dependency.needed_by == *repaint.id()
        }));

        // The report exists, is deterministic and explains the gap.
        assert_eq!(
            export_root.read(MOD_EXPORT_REPORT),
            mod_export_text(&mounted, &request).as_bytes()
        );
        let report = String::from_utf8(export_root.read(MOD_EXPORT_REPORT)).expect("utf8");
        assert!(report.contains("unresolved-original image/synthetic.hull-panel"));
        assert!(report.contains("unresolved-original gun/synthetic.vulcan"));
        assert!(!report.contains("unresolved-original image/synthetic.repaint-stripe"));
        let (remounted, _) = fixture_mount(&selection);
        assert_eq!(report, mod_export_text(&remounted, &request));

        // A second export over the same tree is refused, not merged or
        // overwritten — error propagation, not silent success.
        let error = export_mounted_mods(&mounted, &request, &directory)
            .expect_err("re-exporting onto existing files is refused");
        assert!(
            matches!(error, ModExportError::Write { .. }),
            "the write refusal surfaces: {error}"
        );
    }

    /// **AC04 half the mount sees.** Disabling every mod and reopening the
    /// stock session needs no fallback at all: the stock announcement is
    /// the base fingerprint, the mounts release, and nothing mod-shaped
    /// remains to resolve.
    #[test]
    fn accept_f53_c_disabling_everything_leaves_a_stock_session() {
        let (mut selection, _roots, _, _, _) = enabled_fixture();
        selection.clear();
        assert_eq!(selection.enabled_set().len(), 0);
        assert_eq!(
            content_signature(None, base_fingerprint()),
            base_fingerprint()
        );
        // An empty set has no mount to open — the caller announces stock.
        let error = ModSelection::new()
            .mount(&synthetic_mount_request(), base_fingerprint(), None)
            .expect_err("an empty set cannot mount, so no mount can linger");
        assert_eq!(error.code(), "plan_refused");
    }
}
