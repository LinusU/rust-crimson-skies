//! The GOS file system: registration order for GOS-style requests (#686).
//!
//! The original engine does not look a GOS request up by precedence class.
//! `0x4116c0` registers its sources into `roffile.dll` once, at startup, in
//! one fixed order, and `MetaOpenFile` (`roffile.dll` `0x100016e0`) tries
//! the registered entries **in that order and takes the first that has the
//! name** — there is no ranking, no ambiguity test and no scope test
//! (`docs/findings/2026-10-05-f04-d-original-lookup-order.md`, section D).
//! The order is:
//!
//! 1. `crimptch.rof` under `<EXE Path>\GOSDATA\Assets\`, **only if it
//!    exists**, where `<EXE Path>` comes from the registry key
//!    `HKLM\SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0`;
//! 2. `<UIAssetPath>\Assets\crimson.rof`;
//! 3. the loose `<UIAssetPath>\` directory;
//! 4. the current directory.
//!
//! `AddNewROFDirectory` (`0x10001000`) only `push_back`s, so that sequence
//! *is* the lookup order.
//!
//! **Step 1 is conditional, and the condition is a registry fact no file in
//! the installation can settle.** With the key present — how a registered
//! installation is set up, and then step 1 is this installation's own
//! `GOSDATA/ASSETS/crimptch.rof` — the order is the four steps above.
//! Without the key the path does not exist, the patch is never registered,
//! and the order starts at `crimson.rof`. This module therefore takes the
//! registry outcome as an explicit input ([`ExePathOrigin`]) and reports
//! which of the cases a chain is ([`PatchRegistration`]); it never decides
//! the case itself and never assumes the patch is present.
//!
//! What *is* unconditional, and therefore what a chain pins, is the
//! relative order of steps 2, 3 and 4 and the position of step 1 when it is
//! registered. [`SessionBuilder::mount_gos_chain`] mounts the sources in
//! exactly that order into the [`GOS_NAMESPACE`] key space, and
//! [`crate::vfs::resolve::Vfs::resolve`] answers a key of that namespace in
//! registration order ([`crate::vfs::resolve::LookupOrder::GosRegistration`]).
//! A GOS answer is therefore not a first-wins map over unrelated keys: every
//! candidate is still reported as an attempt, and the reason one lost is
//! that an earlier registration holds the name.
//!
//! Two things this module deliberately does **not** claim:
//!
//! * [`GOS_ORDER_STATUS`] is [`ClaimStatus::Inferred`], never
//!   `verified_original`. The order is derived from static analysis of the
//!   owner-supplied `crimson.decrypted.exe` and
//!   `GOSDATA/ASSETS/BINARIES/roffile.dll`; no run of the original engine
//!   produced it, and the registry condition of step 1 is not measurable
//!   here.
//! * How `MetaOpenFile` matches a request's name against a registered name
//!   **was settled by #693**
//!   (`docs/findings/2026-10-06-t693-metaopenfile-name-matching.md`): a
//!   container request is upper-cased and compared byte for byte against the
//!   stored name, which is never folded, and a loose request reaches
//!   `CreateFileA` unfolded, so the host decides it. On this installation
//!   that answers `crimson.rof`'s `ASSETS/GRAPHICS/ARIAL8.TGA` against the
//!   loose tree's `ASSETS/GRAPHICS/arial8.tga` from the **container**, under
//!   every casing of the request. It is still an explicit input
//!   ([`GosNameMatch`]), with the case-insensitive fold as the documented
//!   default, never assumed silently. `crimptch.rof` shadows `crimson.rof`
//!   for `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` under either rule, so that part of
//!   the order does not depend on it.
//!
//! Mounting a chain reads the two containers and walks the loose directories
//! read-only and writes nothing anywhere, exactly as the rest of the VFS
//! does.

use std::fmt;
use std::path::{Path, PathBuf};

use cs_types::asset_id::{AssetKey, AssetKeyError, MountId, MountNamespace, PrecedenceClass};
use cs_types::evidence::ClaimStatus;

use crate::rof::{RofMountError, RofReadError, RofSource, mount_rof_into};
use crate::vfs::mount::MountBuilder;
use crate::vfs::resolve::LookupOrder;
use crate::vfs::session::{SessionBuilder, SessionError};

/// The mount namespace GOS requests live in.
///
/// A namespace of its own because the original's rule for this file system
/// is registration order, not precedence: mixing these sources into the
/// installation namespace would let the designed precedence order — which
/// is `designed`, not measured — decide them, and would change every
/// existing F04 lookup that shares those mounts.
pub const GOS_NAMESPACE: &str = "gos";

/// The mount id of the patch container, `crimptch.rof`.
pub const PATCH_MOUNT_ID: &str = "gos-patch";

/// The mount id of the main container, `crimson.rof`.
pub const MAIN_MOUNT_ID: &str = "gos-main";

/// The mount id of the loose `<UIAssetPath>` directory.
pub const LOOSE_MOUNT_ID: &str = "gos-loose";

/// The mount id of the current directory.
pub const CURRENT_DIRECTORY_MOUNT_ID: &str = "gos-current-directory";

/// How well the GOS registration order is known.
///
/// `Inferred`, not `verified_original`: it is derived from static analysis of
/// the original executable and `roffile.dll`
/// (`docs/findings/2026-10-05-f04-d-original-lookup-order.md` section D). No
/// run of the original engine produced it, and the registry condition of
/// step 1 is a registry fact no file in the installation settles. A task may
/// not raise this constant itself; only owner-supplied original-run
/// evidence can.
pub const GOS_ORDER_STATUS: ClaimStatus = ClaimStatus::Inferred;

/// The original's default `<UIAssetPath>` (`0x43fb50`).
pub const DEFAULT_UI_ASSET_PATH: &str = "GOSData";

/// Which of the original's sources a mount is.
///
/// The declaration order of this enum **is** the lookup order, so the
/// implementation can be checked against it directly. Step 1 is
/// conditional on the registry and is absent when the patch was never
/// registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GosSource {
    /// `crimptch.rof` under `<EXE Path>\GOSDATA\Assets\` — the patch,
    /// registered first, and only when it exists.
    PatchContainer,
    /// `<UIAssetPath>\Assets\crimson.rof`.
    MainContainer,
    /// The loose `<UIAssetPath>\` directory.
    LooseUiAssets,
    /// The current directory.
    CurrentDirectory,
}

impl GosSource {
    /// Every source in registration order, patch first.
    ///
    /// The patch's position among the sources is unconditional; whether it
    /// is registered at all is [`PatchRegistration`]'s business.
    pub const ALL: [Self; 4] = [
        Self::PatchContainer,
        Self::MainContainer,
        Self::LooseUiAssets,
        Self::CurrentDirectory,
    ];

    /// Its mount id, the stable name a trace and a report use.
    pub const fn mount_id(self) -> &'static str {
        match self {
            Self::PatchContainer => PATCH_MOUNT_ID,
            Self::MainContainer => MAIN_MOUNT_ID,
            Self::LooseUiAssets => LOOSE_MOUNT_ID,
            Self::CurrentDirectory => CURRENT_DIRECTORY_MOUNT_ID,
        }
    }

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::PatchContainer => "patch_container",
            Self::MainContainer => "main_container",
            Self::LooseUiAssets => "loose_ui_assets",
            Self::CurrentDirectory => "current_directory",
        }
    }
}

impl fmt::Display for GosSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How a GOS request's name is matched against a registered source's names.
///
/// Which of the original's two spellings of the same image a request gets
/// depends on this rule. #693 settled what the original does
/// (`docs/findings/2026-10-06-t693-metaopenfile-name-matching.md`): it
/// upper-cases the request and compares it byte for byte against the stored
/// name, so a container whose names are all uppercase — this installation's,
/// measured — is answered the way [`GosNameMatch::AsciiInsensitive`] answers
/// it, and [`GosNameMatch::ExactSpelling`] is **not** what it does. The rule
/// is still an explicit input rather than an assumption, so a caller states
/// it and the rule it chose is recorded with the chain and reported on every
/// lookup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GosNameMatch {
    /// Fold ASCII case and separators, like every other legacy lookup in
    /// this VFS.
    ///
    /// The documented default, because it is the rule the rest of the VFS
    /// applies to legacy spellings. On this installation it agrees with
    /// `MetaOpenFile`'s own rule on **case** (every container name is
    /// uppercase, #693), not on the separator dimension: the original splits
    /// a request only on `\`.
    #[default]
    AsciiInsensitive,
    /// A source answers only when its stored spelling equals the request's
    /// spelling byte for byte.
    ExactSpelling,
}

impl GosNameMatch {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::AsciiInsensitive => "ascii_insensitive",
            Self::ExactSpelling => "exact_spelling",
        }
    }

    /// Whether this rule folds case and separators.
    pub const fn folds(self) -> bool {
        match self {
            Self::AsciiInsensitive => true,
            Self::ExactSpelling => false,
        }
    }
}

impl fmt::Display for GosNameMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where the original's `<EXE Path>` came from — the one input this module
/// cannot derive, because it is a **registry** fact.
///
/// The key is `HKLM\SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0`.
/// With no such key the path the original falls back to does not exist, the
/// patch is never registered, and the chain starts at `crimson.rof`. No
/// agent has registry capability on this machine, so neither case is
/// measured here and the caller states which one it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExePathOrigin {
    /// The key named this path and `crimptch.rof` exists under
    /// `<path>\GOSDATA\Assets\`, so the patch is registered first.
    RegistryWithPatch(PathBuf),
    /// The key named this path, but no `crimptch.rof` exists there.
    RegistryWithoutPatch(PathBuf),
    /// No such key: the fallback path does not exist and the patch is never
    /// registered.
    RegistryKeyAbsent,
}

impl ExePathOrigin {
    /// Decides by looking, for a caller that has a registry value: the
    /// original registers the container *if it exists*, so a caller holding
    /// the path answers by checking it.
    ///
    /// The existence check is the one retail fact this module reads by
    /// itself, and it reads it from the read-only installation. It resolves
    /// each directory component case-insensitively (see
    /// [`resolve_container`]): the installation spells them `GOSDATA` and
    /// `ASSETS` while the original asks for `GOSDATA` and `Assets`, so
    /// asking a case-sensitive host for the original's spelling alone would
    /// report "no patch container" for a container that is present — a wrong
    /// answer about the installation, not a missing file.
    pub fn inspect_registry(exe_path: &Path) -> Self {
        if resolve_container(exe_path, "crimptch.rof").is_file() {
            Self::RegistryWithPatch(exe_path.to_path_buf())
        } else {
            Self::RegistryWithoutPatch(exe_path.to_path_buf())
        }
    }

    /// `<EXE Path>\GOSDATA\Assets\crimptch.rof`, spelled the way the original
    /// spells it.
    ///
    /// This is the path the original *asks for*, so it is what a report names
    /// as where the original looked. Whether that is the file on disk is a
    /// host question, which is what [`resolve_container`] answers.
    pub fn patch_container(exe_path: &Path) -> PathBuf {
        let mut path = exe_path.to_path_buf();
        path.push("GOSDATA");
        path.push("Assets");
        path.push("crimptch.rof");
        path
    }
}

/// Which case of step 1 a chain is, and therefore where its order starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchRegistration {
    /// The patch is registered first and the chain starts there.
    Registered {
        /// The container it was found at.
        container: PathBuf,
    },
    /// The registry key is absent: the patch is never registered and the
    /// chain starts at `crimson.rof`. This is the documented "registry
    /// missing" case, which is not the same as a missing file.
    RegistryKeyAbsent,
    /// The key named a path with no patch container in it, so the chain
    /// starts at `crimson.rof` for the original's own reason (the container
    /// does not exist), not because the key was missing.
    ContainerAbsent {
        /// Where the container was looked for.
        searched: PathBuf,
    },
}

impl PatchRegistration {
    /// The outcome for a stated [`ExePathOrigin`].
    ///
    /// A registered patch records the container **as the host spells it**,
    /// because that is the file the chain mounts and reads; `ContainerAbsent`
    /// records where the *original* looked, which is the literal
    /// [`ExePathOrigin::patch_container`] path. The two answers name
    /// different things on purpose: one is what was read, the other is where
    /// the original would have looked.
    fn of(origin: &ExePathOrigin) -> Self {
        match origin {
            ExePathOrigin::RegistryWithPatch(path) => Self::Registered {
                container: resolve_container(path, "crimptch.rof"),
            },
            ExePathOrigin::RegistryWithoutPatch(path) => Self::ContainerAbsent {
                searched: ExePathOrigin::patch_container(path),
            },
            ExePathOrigin::RegistryKeyAbsent => Self::RegistryKeyAbsent,
        }
    }

    /// Whether the patch is the first source of the chain.
    pub fn is_registered(&self) -> bool {
        matches!(self, Self::Registered { .. })
    }

    /// The stable label used in reports.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Registered { .. } => "registered",
            Self::RegistryKeyAbsent => "registry_key_absent",
            Self::ContainerAbsent { .. } => "container_absent",
        }
    }
}

impl fmt::Display for PatchRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registered { container } => {
                write!(f, "registered ({})", container.display())
            }
            Self::RegistryKeyAbsent => f.write_str(
                "skipped: no HKLM\\SOFTWARE\\Microsoft\\Microsoft Games\\Crimson Skies\\1.0, so \
                 the fallback path does not exist and the patch is never registered",
            ),
            Self::ContainerAbsent { searched } => {
                write!(f, "skipped: no crimptch.rof under {}", searched.display())
            }
        }
    }
}

/// Why a GOS chain could not be mounted.
///
/// A refusal names the step it happened at. Mounts that already joined stay
/// in the session, so the caller can retry that one step or drop the
/// builder; nothing is reordered to make a chain build.
#[derive(Debug)]
pub enum GosError {
    /// The patch container could not be mounted.
    Patch {
        /// The container that failed.
        container: PathBuf,
        /// Why.
        source: Box<RofMountError>,
    },
    /// `crimson.rof` could not be mounted. Unlike the other steps this one
    /// is not optional: it is step 2 of the order in every registry case, so
    /// a chain without it is not the original's chain.
    Main {
        /// The container that failed.
        container: PathBuf,
        /// Why.
        source: Box<RofMountError>,
    },
    /// The loose UI asset directory could not be mounted.
    Loose {
        /// The directory that failed.
        path: PathBuf,
        /// Why.
        source: Box<crate::vfs::source::SourceError>,
    },
    /// The current directory could not be mounted.
    CurrentDirectory {
        /// The directory that failed.
        path: PathBuf,
        /// Why.
        source: Box<crate::vfs::source::SourceError>,
    },
    /// The session refused the mount.
    Session(Box<SessionError>),
}

impl fmt::Display for GosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Patch { container, source } => write!(
                f,
                "cannot mount the patch container {}: {source}",
                container.display()
            ),
            Self::Main { container, source } => write!(
                f,
                "cannot mount {}: {source}; it is step 2 of the GOS order in every registry \
                 case, so the chain cannot be built without it",
                container.display()
            ),
            Self::Loose { path, source } => write!(
                f,
                "cannot mount the loose directory {}: {source}",
                path.display()
            ),
            Self::CurrentDirectory { path, source } => write!(
                f,
                "cannot mount the current directory {}: {source}",
                path.display()
            ),
            Self::Session(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for GosError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Patch { source, .. } | Self::Main { source, .. } => Some(source),
            Self::Loose { source, .. } | Self::CurrentDirectory { source, .. } => Some(source),
            Self::Session(error) => Some(error),
        }
    }
}

/// What to mount as one GOS chain.
///
/// Every field is an input the caller states rather than something this
/// module guesses: the installation root, the registry outcome it has, the
/// UI asset path, the directory the process considers current, and the
/// name-matching rule lookups will answer with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GosInstall<'a> {
    /// The read-only installation root.
    pub host_root: &'a Path,
    /// The registry outcome for `<EXE Path>`.
    pub exe_path: ExePathOrigin,
    /// `<UIAssetPath>`, spelled as the installation spells it.
    pub ui_asset_path: &'a str,
    /// The current directory, or `None` to leave step 4 unregistered — what
    /// a caller that does not know the process's working directory must do,
    /// because this module never guesses it.
    pub current_directory: Option<&'a Path>,
    /// The name-matching rule this chain's lookups answer with.
    pub name_match: GosNameMatch,
}

impl<'a> GosInstall<'a> {
    /// A chain request for `host_root` with the documented defaults: the
    /// caller states the registry outcome, `<UIAssetPath>` is the original's
    /// default, step 4 is unregistered and names are matched
    /// case-insensitively.
    pub fn new(host_root: &'a Path, exe_path: ExePathOrigin) -> Self {
        Self {
            host_root,
            exe_path,
            ui_asset_path: DEFAULT_UI_ASSET_PATH,
            current_directory: None,
            name_match: GosNameMatch::default(),
        }
    }

    /// Registers step 4, the current directory.
    pub fn with_current_directory(mut self, path: &'a Path) -> Self {
        self.current_directory = Some(path);
        self
    }

    /// Sets the name-matching rule.
    pub fn with_name_match(mut self, name_match: GosNameMatch) -> Self {
        self.name_match = name_match;
        self
    }
}

/// One registered source of a chain: which [`GosSource`] it is, the mount
/// holding it, the container it was read from and how many members it
/// contributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GosStep {
    /// Which of the original's sources this is.
    pub source: GosSource,
    /// The mount that holds it.
    pub mount: MountId,
    /// The container as a host path, as the caller named it.
    pub container: PathBuf,
    /// How many members it contributes.
    pub members: usize,
}

/// The registered sources of one GOS chain, in the original's order.
///
/// A chain is the record of a registration: [`steps`] is in the order
/// `AddNewROFDirectory` pushed the sources, which is the order
/// `MetaOpenFile` walks them. [`patch`] says which registry case this chain
/// is and [`name_match`] the rule its lookups answer with, so
/// nothing about a chain has to be recalled from the code that built it.
#[derive(Debug)]
pub struct GosChain {
    steps: Vec<GosStep>,
    patch: PatchRegistration,
    name_match: GosNameMatch,
    /// The container reader of every ROF source this chain registered.
    ///
    /// A chain keeps them, and they are the only way to read the bytes of a
    /// container member: a [`crate::vfs::mount::Mount`] built from a ROF
    /// container indexes its members' locations and digests but has no host
    /// backing, so `ContentSession::read_all` refuses such a mount with
    /// [`crate::vfs::source::ReadError::NoBacking`]. Dropping these here would
    /// register members nothing could ever read.
    ///
    /// They are compared by mount id, not by bytes: a container's bytes are
    /// its own, so two chains of the same installation are equal when they
    /// registered the same sources with the same result.
    sources: Vec<(MountId, RofSource)>,
}

impl PartialEq for GosChain {
    fn eq(&self, other: &Self) -> bool {
        self.steps == other.steps
            && self.patch == other.patch
            && self.name_match == other.name_match
            && self.sources.len() == other.sources.len()
            && self
                .sources
                .iter()
                .zip(&other.sources)
                .all(|(left, right)| left.0 == right.0)
    }
}

impl Eq for GosChain {}

impl GosChain {
    /// The sources, in registration order.
    pub fn steps(&self) -> &[GosStep] {
        &self.steps
    }

    /// Which registry case this chain is.
    pub fn patch(&self) -> &PatchRegistration {
        &self.patch
    }

    /// The name-matching rule this chain's lookups answer with.
    pub fn name_match(&self) -> GosNameMatch {
        self.name_match
    }

    /// How well the order this chain pins is known
    /// ([`GOS_ORDER_STATUS`], `inferred`).
    pub fn order_status(&self) -> ClaimStatus {
        GOS_ORDER_STATUS
    }

    /// The order this chain pins, as the labels of its registered sources:
    /// the sequence `MetaOpenFile` walks, with the patch absent when it was
    /// never registered.
    pub fn order(&self) -> Vec<&'static str> {
        self.steps.iter().map(|step| step.source.label()).collect()
    }

    /// The step of `source`, if this chain registered it.
    pub fn step_of(&self, source: GosSource) -> Option<&GosStep> {
        self.steps.iter().find(|step| step.source == source)
    }

    /// Every source of the original's order this chain left unregistered.
    ///
    /// Compared with [`Self::order`] this shows what is *missing*, which is
    /// the difference between "the patch lost" and "the patch was never
    /// registered" — the distinction the registry case makes.
    pub fn unregistered(&self) -> Vec<GosSource> {
        GosSource::ALL
            .into_iter()
            .filter(|source| self.step_of(*source).is_none())
            .collect()
    }

    /// The container reader of the ROF source registered for `mount`, if it
    /// is one of this chain's containers.
    pub fn rof_source(&self, mount: &MountId) -> Option<&RofSource> {
        self.sources
            .iter()
            .find(|(registered, _)| registered == mount)
            .map(|(_, source)| source)
    }

    /// The bytes of a GOS resolution: through the ROF reader for a container
    /// mount, through the session's own read for a directory-backed one.
    ///
    /// Both paths are the production ones — [`RofSource::read`] and
    /// [`crate::vfs::session::ContentSession::read_all`] — so the bytes are
    /// the ones the origin the order chose actually holds, checked against
    /// the digest recorded when it was mounted.
    pub fn read(
        &self,
        session: &crate::vfs::session::ContentSession,
        asset: &crate::vfs::session::SessionAsset,
    ) -> Result<Vec<u8>, GosReadError> {
        let mount = &asset.resolved().mount;
        if let Some(source) = self.rof_source(mount) {
            return source
                .read(&asset.resolved().key)
                .map(|read| read.data)
                .map_err(GosReadError::Rof);
        }
        if self.steps.iter().any(|step| &step.mount == mount) {
            return session
                .read_all(asset)
                .map_err(GosReadError::DirectorySource);
        }
        Err(GosReadError::ForeignMount(mount.clone()))
    }
}

/// Why a GOS resolution's bytes could not be read.
#[derive(Debug)]
pub enum GosReadError {
    /// A container member the ROF reader refused: the member is unknown to
    /// that container, its extent is corrupt, or its decode exceeded the
    /// ceiling.
    Rof(RofReadError),
    /// A directory-backed member the session's read path refused, including a
    /// resolution stamped by another session.
    DirectorySource(crate::vfs::source::ReadError),
    /// The resolution names a mount that is not part of this chain, so this
    /// chain does not hold the reader for it.
    ForeignMount(MountId),
}

impl fmt::Display for GosReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rof(error) => write!(f, "{error}"),
            Self::DirectorySource(error) => write!(f, "{error}"),
            Self::ForeignMount(mount) => write!(
                f,
                "mount {mount} is not registered by this chain, so this chain holds no reader \
                 for it"
            ),
        }
    }
}

impl std::error::Error for GosReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rof(error) => Some(error),
            Self::DirectorySource(error) => Some(error),
            Self::ForeignMount(_) => None,
        }
    }
}

impl SessionBuilder {
    /// Mounts one GOS chain into this session, in the original's
    /// registration order, and returns what was registered.
    ///
    /// The mounts join in exactly the order [`GosSource::ALL`] declares —
    /// patch, `crimson.rof`, the loose UI asset directory, the current
    /// directory — skipping the patch when [`PatchRegistration`] says it was
    /// never registered. That order is the answer this chain pins, so a
    /// failure names the step it happened at and **nothing is reordered to
    /// work around it**.
    ///
    /// Every mount is an original-data ([`MountBuilder::retail`]) member of
    /// the [`GOS_NAMESPACE`] namespace, read-only, with no precedence class
    /// of its own deciding anything: in this key space the registration
    /// order is the rule (see
    /// [`crate::vfs::resolve::LookupOrder::GosRegistration`]). The class is
    /// still declared, so a trace and a collision report name it rather than
    /// leaving it implicit.
    pub fn mount_gos_chain(&mut self, install: &GosInstall<'_>) -> Result<GosChain, GosError> {
        self.set_gos_name_match(install.name_match)
            .map_err(|error| GosError::Session(Box::new(error)))?;
        let patch = PatchRegistration::of(&install.exe_path);
        let mut steps: Vec<GosStep> = Vec::new();
        let mut sources: Vec<(MountId, RofSource)> = Vec::new();
        let namespace = || {
            MountNamespace::new(GOS_NAMESPACE).expect("the GOS namespace label is a valid label")
        };
        // The `<UIAssetPath>` directory as the installation actually spells
        // it, which is what every container label of this chain records: the
        // original's own spelling is `GOSData`, the installation may spell
        // it `GOSDATA`, and provenance names what was read.
        let ui_assets = found_child(install.host_root, install.ui_asset_path);

        if let PatchRegistration::Registered { container } = &patch {
            let builder = MountBuilder::new(
                mount_id(GosSource::PatchContainer),
                namespace(),
                PrecedenceClass::Patch,
                &container_spelling(&ui_assets, "crimptch.rof"),
            )
            .retail();
            let source =
                mount_rof_into(self, builder, container).map_err(|source| GosError::Patch {
                    container: container.clone(),
                    source: Box::new(source),
                })?;
            steps.push(GosStep {
                source: GosSource::PatchContainer,
                mount: source.mount_id().clone(),
                container: container.clone(),
                members: source.member_count(),
            });
            sources.push((source.mount_id().clone(), source));
        }

        let main = assets_container(&ui_assets, "crimson.rof");
        let main_builder = MountBuilder::new(
            mount_id(GosSource::MainContainer),
            namespace(),
            PrecedenceClass::Shared,
            &container_spelling(&ui_assets, "crimson.rof"),
        )
        .retail();
        let main_source =
            mount_rof_into(self, main_builder, &main).map_err(|source| GosError::Main {
                container: main.clone(),
                source: Box::new(source),
            })?;
        steps.push(GosStep {
            source: GosSource::MainContainer,
            mount: main_source.mount_id().clone(),
            container: main,
            members: main_source.member_count(),
        });
        sources.push((main_source.mount_id().clone(), main_source));

        let loose = ui_assets.clone();
        self.mount_gos_directory(
            MountBuilder::new(
                mount_id(GosSource::LooseUiAssets),
                namespace(),
                PrecedenceClass::Shared,
                &ui_assets
                    .file_name()
                    .map(|spelling| spelling.to_string_lossy().into_owned())
                    .unwrap_or_else(|| DEFAULT_UI_ASSET_PATH.to_owned()),
            )
            .retail(),
            &loose,
        )
        .map_err(|source| GosError::Loose {
            path: loose.clone(),
            source: Box::new(source),
        })?;
        steps.push(GosStep {
            source: GosSource::LooseUiAssets,
            mount: mount_id(GosSource::LooseUiAssets),
            container: loose,
            members: self.mount_members(&mount_id(GosSource::LooseUiAssets)),
        });

        if let Some(current) = install.current_directory {
            self.mount_gos_directory(
                MountBuilder::new(
                    mount_id(GosSource::CurrentDirectory),
                    namespace(),
                    PrecedenceClass::Shared,
                    ".",
                )
                .retail(),
                current,
            )
            .map_err(|source| GosError::CurrentDirectory {
                path: current.to_path_buf(),
                source: Box::new(source),
            })?;
            steps.push(GosStep {
                source: GosSource::CurrentDirectory,
                mount: mount_id(GosSource::CurrentDirectory),
                container: current.to_path_buf(),
                members: self.mount_members(&mount_id(GosSource::CurrentDirectory)),
            });
        }

        Ok(GosChain {
            steps,
            patch,
            name_match: install.name_match,
            sources,
        })
    }
}

/// `name` inside `parent` as the host spells it, falling back to the
/// requested spelling when there is no such entry — the mount then fails at
/// that path with the refusal that says why.
///
/// An **exact** spelling wins over one differing only in case, and the
/// case-insensitive match is a fallback rather than the rule. That ordering
/// is not cosmetic: a directory can hold two entries differing only in case
/// (a case-sensitive host permits it), and `read_dir` returns them in an
/// unspecified order, so a rule that accepted either one would resolve a
/// chain differently from run to run. The exact entry is always the one the
/// original asked for; the fold only rescues a host that spells it
/// differently.
fn found_child(parent: &Path, name: &str) -> PathBuf {
    let requested = parent.join(name);
    let Ok(entries) = std::fs::read_dir(parent) else {
        return requested;
    };
    let mut folded = None;
    for entry in entries.flatten() {
        let entry_name = entry.file_name();
        if entry_name.as_os_str() == name {
            return entry.path();
        }
        if folded.is_none() && entry_name.to_string_lossy().eq_ignore_ascii_case(name) {
            folded = Some(entry.path());
        }
    }
    folded.unwrap_or(requested)
}

/// `<exe>/GOSDATA/Assets/<name>`, each directory component resolved as the
/// host spells it.
///
/// The original asks for `GOSDATA` then `Assets`; the installation spells
/// them `GOSDATA` and `ASSETS`. A case-sensitive host therefore needs the
/// case-insensitive walk [`found_child`] performs, exactly as `<UIAssetPath>`
/// already needed it — without it a chain cannot be mounted at all on such a
/// host, and [`ExePathOrigin::inspect_registry`] would conclude the patch
/// container is absent when it is present.
fn resolve_container(exe_path: &Path, name: &str) -> PathBuf {
    found_child(&found_child(exe_path, "GOSDATA"), "Assets").join(name)
}

/// `<UIAssetPath>/Assets/<name>`, the `Assets` component resolved as the host
/// spells it for the same reason as [`resolve_container`].
fn assets_container(ui_assets: &Path, name: &str) -> PathBuf {
    found_child(ui_assets, "Assets").join(name)
}

/// The container label of a GOS container: its installation-relative
/// spelling with `/` separators.
fn container_spelling(ui_assets: &Path, name: &str) -> String {
    let mut label = ui_assets
        .file_name()
        .map(|spelling| spelling.to_string_lossy().into_owned())
        .unwrap_or_else(|| DEFAULT_UI_ASSET_PATH.to_owned());
    label.push_str("/Assets/");
    label.push_str(name);
    label
}

/// The key a GOS request is made with: the source-relative spelling in the
/// GOS namespace.
///
/// The spelling is validated like any other key, so a request can neither
/// escape a source root nor lose its original spelling (spec F04
/// non-negotiable behavior 1).
pub fn gos_key(spelling: &str) -> Result<AssetKey, AssetKeyError> {
    AssetKey::from_spelling(GOS_NAMESPACE, spelling, "default")
}

/// The order every [`GOS_NAMESPACE`] key resolves by, and how well that
/// order is known.
pub const fn gos_order() -> LookupOrder {
    LookupOrder::GosRegistration
}

/// The evidence status of that order.
pub const fn gos_order_status() -> ClaimStatus {
    GOS_ORDER_STATUS
}

/// The validated mount id of one GOS source.
///
/// # Panics
///
/// Never: the labels are the four constants of this module, each validated
/// by the same rule as any other label, and [`GosSource::mount_id`] returns
/// only those.
fn mount_id(source: GosSource) -> MountId {
    MountId::new(source.mount_id()).expect("the GOS mount id labels are valid labels")
}
