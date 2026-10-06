//! The archives one world/mission load binds: `gamez.zbd`, `planes.zbd`,
//! `cam_anim.zbd`, `mis_anim.zbd` and the world's one texture archive
//! (F04-D, task #687).
//!
//! What the original engine does, as the owner's static analysis of the
//! pinned executable records it (`docs/findings/
//! 2026-10-05-f04-d-original-lookup-order.md`, sections B, C, E and G):
//!
//! 1. a world load runs the INTERP scripts `support\init.gw`,
//!    `support\<world>\init.gw` and `support\main.gw`, which name the
//!    archives in exactly this order — `AnimAddZBDFile zbd\<world>\cam_anim.zbd`,
//!    `AnimAddZBDFile zbd\<world>\<mission>\mis_anim.zbd`,
//!    `SetTextureDirectory zbd\<world>` and `MissionZBDFile = zbd\<world>\gamez.zbd`,
//!    then `GameZReadZBDFile %MissionZBDFile%` and `GameZReadZBDFile zbd\planes.zbd`;
//! 2. `gamez.zbd` is bound **per world group only** — never per mission — and
//!    `planes.zbd` is read from the ZBD **root**;
//! 3. both animation archives are appended to one list (de-duplicated by exact
//!    string) and then **every** listed file is loaded completely, in list
//!    order: the world's `cam_anim.zbd` first, then the mission's
//!    `mis_anim.zbd`. Neither replaces the other by name and **none of these
//!    four lookups falls back to another directory level**;
//! 4. exactly **one** texture archive is opened per world, looked up in
//!    `zbd\<world>` with the shared `zbd` directory as the fallback, never in
//!    another world's directory.
//!
//! [`WorldLayout`] records those bindings for one [`ResolveContext`], including
//! the **mission level** the designed layout had no place for: the mission's
//! `mis_anim.zbd` exists only when the context names a mission, and it is the
//! only binding spelled two levels below the shared directory.
//!
//! # What this does *not* decide
//!
//! * **Which texture file.** The measured selection rule — the budget and the
//!   descending walk over `rtextureN.zbd` / `textureN.zbd`, software renderer
//!   always `texture.zbd` — belongs to task #352 and lives in
//!   `cs_content::textures` ([`TextureBinding`] records where the search
//!   happens and holds the file that rule selected; it does not re-implement
//!   the walk, and `cs_assets` cannot call `cs_content` because the dependency
//!   runs the other way).
//! * **What is inside an archive.** Nothing here parses a container; this
//!   module names archives, and the format crates read them.
//! * **Reader archives.** `zrdr.zbd` binds at three levels and is looked up by
//!   its own measured order; that is [`crate::vfs::reader`]'s
//!   [`ReaderLevel`](crate::vfs::reader::ReaderLevel), not a binding here.
//!
//! # Evidence
//!
//! [`BINDING_ORDER_STATUS`] is `inferred`: these bindings are read off the
//! original executable's code, which is real evidence but not a runtime
//! capture, so nothing here is ever presented as
//! [`ClaimStatus::VerifiedOriginal`]. The designed precedence order
//! ([`PRECEDENCE_ORDER_STATUS`]) is **not** consulted — a binding is a name the
//! original opens at one path, not a competition between sources — and stays
//! `designed`.
//!
//! The world, mission and archive spellings this module composes come from the
//! caller's [`ResolveContext`] and the installation's own `ZBD` directory
//! ([`crate::install::Diagnosis::zbd_dir`]), so they keep the installation's
//! spelling rather than a hard-coded one; the comparison a mount makes is the
//! case-folded logical key either way.

use std::fmt;
use std::path::Path;

use cs_types::asset_id::{LabelError, MissionScope, ResolveContext, WorldGroup};
use cs_types::evidence::ClaimStatus;
use cs_types::install::{RelativePath, RelativePathError};

use crate::install::Diagnosis;

/// The world container the original's INTERP scripts read for a world group:
/// `ZBD/<world group>/gamez.zbd`.
pub const GAMEZ_FILE: &str = "gamez.zbd";

/// The shared aircraft container the original reads from the ZBD root:
/// `ZBD/planes.zbd`.
pub const PLANES_FILE: &str = "planes.zbd";

/// The world animation archive: `ZBD/<world group>/cam_anim.zbd`.
pub const CAMERA_ANIM_FILE: &str = "cam_anim.zbd";

/// The mission animation archive: `ZBD/<world group>/<mission>/mis_anim.zbd`.
pub const MISSION_ANIM_FILE: &str = "mis_anim.zbd";

/// How well the bindings recorded here are known.
///
/// The names, their levels, their order and the absence of a fallback are read
/// off the original executable's INTERP scripts and loader code
/// (`docs/findings/2026-10-05-f04-d-original-lookup-order.md` section B). That
/// is code-derived evidence, therefore `inferred`, and never
/// [`ClaimStatus::VerifiedOriginal`].
///
/// [`PRECEDENCE_ORDER_STATUS`] — the *designed* precedence order these bindings
/// are not — is deliberately untouched and not consulted here.
pub const BINDING_ORDER_STATUS: ClaimStatus = ClaimStatus::Inferred;

/// Which reader family loads an archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArchiveFamily {
    /// The GameZ mesh family (`GameZReadZBDFile`).
    GameZ,
    /// The animation family (`AnimAddZBDFile`, then one complete load per
    /// listed file, in list order).
    Animation,
    /// The texture family (exactly one archive per world).
    Texture,
}

impl ArchiveFamily {
    /// The stable label used in traces and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::GameZ => "gamez",
            Self::Animation => "animation",
            Self::Texture => "texture",
        }
    }
}

impl fmt::Display for ArchiveFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which level of the installation an archive is bound at.
///
/// This is the **directory level the original names**, not a precedence rank:
/// the original mounts these by explicit path, so a mission-level archive is
/// not "higher priority" than a world-level one — it is a different file the
/// original opens in addition to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BindingLevel {
    /// The shared `ZBD` directory itself (`planes.zbd`): read for every world
    /// and mission.
    Root,
    /// A world group directory (`ZBD/<world group>`): bound per world group,
    /// never per mission.
    World,
    /// A mission directory (`ZBD/<world group>/<mission>`): the only level a
    /// mission can bind, and the level the designed layout had no mount for.
    Mission,
}

impl BindingLevel {
    /// The stable label used in traces and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::World => "world",
            Self::Mission => "mission",
        }
    }

    /// How many directory components the level adds below the shared `ZBD`
    /// directory: the root level adds none, a world group one, a mission two.
    pub const fn depth(self) -> usize {
        match self {
            Self::Root => 0,
            Self::World => 1,
            Self::Mission => 2,
        }
    }
}

impl fmt::Display for BindingLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One archive the original binds by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BindingRole {
    /// `ZBD/<world group>/cam_anim.zbd`, added to the animation list first.
    CameraAnimation,
    /// `ZBD/<world group>/<mission>/mis_anim.zbd`, added to the animation list
    /// second and only when the load names a mission.
    MissionAnimation,
    /// The one texture archive of the world, looked up in the world's own
    /// directory with the shared directory as the fallback.
    TextureArchive,
    /// `ZBD/<world group>/gamez.zbd`, per world group only.
    GameZ,
    /// `ZBD/planes.zbd`, read from the ZBD root after the world's GameZ file.
    Planes,
}

impl BindingRole {
    /// Every role, in the order the INTERP scripts name them.
    pub const ALL: [Self; 5] = [
        Self::CameraAnimation,
        Self::MissionAnimation,
        Self::TextureArchive,
        Self::GameZ,
        Self::Planes,
    ];

    /// The stable label used in traces and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::CameraAnimation => "camera_animation",
            Self::MissionAnimation => "mission_animation",
            Self::TextureArchive => "texture_archive",
            Self::GameZ => "gamez",
            Self::Planes => "planes",
        }
    }

    /// The role a stable label names, or `None` when the label names none.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.label() == label)
    }

    /// The file name the original gives this role, without its directory.
    ///
    /// The texture archive has **no** fixed file name: which of
    /// `rtextureN.zbd` / `textureN.zbd` / `texture.zbd` a world opens is
    /// decided by the measured selection rule task #352 implements, so this
    /// returns the name of that decision's result only through
    /// [`TextureBinding`], never as a constant.
    pub const fn file(self) -> Option<&'static str> {
        match self {
            Self::CameraAnimation => Some(CAMERA_ANIM_FILE),
            Self::MissionAnimation => Some(MISSION_ANIM_FILE),
            Self::TextureArchive => None,
            Self::GameZ => Some(GAMEZ_FILE),
            Self::Planes => Some(PLANES_FILE),
        }
    }

    /// Which reader family loads this role's archive.
    pub const fn family(self) -> ArchiveFamily {
        match self {
            Self::CameraAnimation | Self::MissionAnimation => ArchiveFamily::Animation,
            Self::TextureArchive => ArchiveFamily::Texture,
            Self::GameZ | Self::Planes => ArchiveFamily::GameZ,
        }
    }

    /// Which directory level binds this role.
    pub const fn level(self) -> BindingLevel {
        match self {
            Self::CameraAnimation | Self::TextureArchive | Self::GameZ => BindingLevel::World,
            Self::MissionAnimation => BindingLevel::Mission,
            Self::Planes => BindingLevel::Root,
        }
    }

    /// The position at which the original's INTERP scripts name this role:
    /// the two `AnimAddZBDFile` calls in `support\init.gw`, then
    /// `SetTextureDirectory` in `support\<world>\init.gw`, then the
    /// `MissionZBDFile` assignment and the two `GameZReadZBDFile` reads of
    /// `support\main.gw`.
    ///
    /// The animation pair's relative order is the load order
    /// (`0x522ef0` loads every listed file in list order); the GameZ pair's is
    /// the order they are read in. The finding does not pin a single order
    /// across the two families, so this position is what the scripts say and
    /// nothing more is claimed from it.
    pub const fn script_order(self) -> u8 {
        match self {
            Self::CameraAnimation => 0,
            Self::MissionAnimation => 1,
            Self::TextureArchive => 2,
            Self::GameZ => 3,
            Self::Planes => 4,
        }
    }

    /// Whether the role's lookup falls back from one directory level to
    /// another.
    ///
    /// The four named archives are looked up at their explicit path with **no**
    /// fallback; the texture archive is searched in the world's own directory
    /// and then in the shared directory, which is the only level fallback this
    /// task records.
    pub const fn has_directory_fallback(self) -> bool {
        matches!(self, Self::TextureArchive)
    }
}

impl fmt::Display for BindingRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One archive this load binds, with the installation spelling the original
/// names it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundArchive {
    role: BindingRole,
    container: RelativePath,
}

impl BoundArchive {
    fn new(role: BindingRole, container: RelativePath) -> Self {
        Self { role, container }
    }

    /// Which archive this binding names.
    pub const fn role(&self) -> BindingRole {
        self.role
    }

    /// The installation spelling the original opens, e.g. `zbd/c1c/gamez.zbd`.
    ///
    /// The world group's and the mission's own spellings are preserved; the
    /// comparison a [`Mount`](crate::vfs::mount::Mount) makes is the case-folded
    /// logical key, so `zbd/C1C/...` and `zbd/c1c/...` are one file.
    pub fn container(&self) -> &RelativePath {
        &self.container
    }

    /// Which directory level binds this archive ([`BindingRole::level`]).
    pub const fn level(&self) -> BindingLevel {
        self.role.level()
    }

    /// Which reader family loads this archive ([`BindingRole::family`]).
    pub const fn family(&self) -> ArchiveFamily {
        self.role.family()
    }

    /// Where the original's scripts name this binding
    /// ([`BindingRole::script_order`]).
    pub const fn script_order(&self) -> u8 {
        self.role.script_order()
    }
}

impl fmt::Display for BoundArchive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.container.as_str(), self.role.level())
    }
}

/// Where the one texture archive of a world is searched for, and which file the
/// measured selection rule picked.
///
/// The **search** is recorded here because it is a layout fact: the world's own
/// directory first, the shared `ZBD` directory as the fallback, never another
/// world's directory (finding section C). The **choice of file** is not: it
/// belongs to the measured budget-and-tier rule of task #352 in
/// `cs_content::textures`, which cannot be called from here (that crate depends
/// on this one), so a binding holds the file that rule selected and refuses a
/// second one — exactly one archive per world — and refuses one that is not in
/// the two directories searched, because the original never looks in another
/// world's directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureBinding {
    directories: Vec<RelativePath>,
    archive: Option<RelativePath>,
}

impl TextureBinding {
    /// The world's search: its own directory first, then the shared `ZBD`
    /// directory as the fallback.
    pub fn new(world_directory: RelativePath, shared_directory: RelativePath) -> Self {
        Self {
            directories: vec![world_directory, shared_directory],
            archive: None,
        }
    }

    /// The directories the original searches, in its order.
    pub fn directories(&self) -> &[RelativePath] {
        &self.directories
    }

    /// The file the measured selection rule chose for this world, or `None`
    /// while no rule has run.
    pub fn archive(&self) -> Option<&RelativePath> {
        self.archive.as_ref()
    }

    /// Whether this world's search falls back from one directory level to
    /// another: the only level fallback the original makes among the archives
    /// this module records.
    pub const fn falls_back(&self) -> bool {
        self.directories.len() > 1
    }

    /// Records the archive the selection rule chose.
    ///
    /// # Errors
    ///
    /// * [`BindingError::TextureArchiveAlreadyBound`] when a file is already
    ///   bound: the original opens exactly one texture archive per world, so a
    ///   second one is refused rather than stored as a fallback the original
    ///   does not have (there is no tier-to-tier fallback for a name).
    /// * [`BindingError::TextureArchiveOutsideSearch`] when the file is not in
    ///   one of the directories this world searches: the original looks the
    ///   archive up in the world's own directory and then the shared one, and
    ///   **never** in another world's (finding section C), so a name from
    ///   anywhere else is one the original could not have opened.
    pub fn bind(&mut self, archive: RelativePath) -> Result<&mut Self, BindingError> {
        if let Some(bound) = &self.archive {
            return Err(BindingError::TextureArchiveAlreadyBound {
                bound: bound.as_str().to_owned(),
                second: archive.as_str().to_owned(),
            });
        }
        // The archive is looked up **in** a searched directory, so its own
        // directory is what decides; compared by logical key, as a mount
        // compares members.
        let archive_directory = archive
            .logical_key()
            .rsplit_once('/')
            .map(|(directory, _)| directory.to_owned());
        let searched: Vec<String> = self.directories.iter().map(|d| d.logical_key()).collect();
        if !archive_directory
            .as_ref()
            .is_some_and(|directory| searched.iter().any(|known| known == directory))
        {
            return Err(BindingError::TextureArchiveOutsideSearch {
                archive: archive.as_str().to_owned(),
                searched: self
                    .directories
                    .iter()
                    .map(|directory| directory.as_str().to_owned())
                    .collect(),
            });
        }
        self.archive = Some(archive);
        Ok(self)
    }
}

/// Why a binding could not be recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingError {
    /// The context names no world group, so no archive of this load can be
    /// spelled: every binding of a world load lives under the world group's
    /// directory. A mission without a world is reported the same way, because
    /// the original's `MISSION_DIR` is only ever used below `CAMPAIGN_DIR`.
    NoWorldGroup {
        /// The mission the context named, if any.
        mission: Option<String>,
    },
    /// The installation spelling this role needs could not be spelled as a
    /// relative path.
    ///
    /// Unreachable for a context built from the validating constructors
    /// ([`WorldGroup::new`], [`cs_types::asset_id::MissionScope::new`] and
    /// [`RelativePath::new`]), whose components are already spellable; kept so
    /// assembling a container is total rather than panicking, and carrying the
    /// real refusal instead of a substituted one.
    ContainerSpelling {
        /// The role whose container could not be spelled.
        role: BindingRole,
        /// The spelling that was assembled.
        spelling: String,
        /// Which path rule refused it.
        reason: RelativePathError,
    },
    /// A second texture archive was bound for one world, which the original
    /// never opens.
    TextureArchiveAlreadyBound {
        /// The archive already bound.
        bound: String,
        /// The archive a caller tried to add.
        second: String,
    },
    /// A texture archive was bound that is not inside one of the directories
    /// this world searches — in particular one from **another** world's
    /// directory, which the original never looks in (finding section C).
    ///
    /// The comparison is the case-folded logical key of the archive's own
    /// directory against the search's, exactly as a mount compares members, so
    /// `ZBD\C1C\RTexture15.zbd` binds for `zbd/c1c` while `zbd/c1/rtexture2.zbd`
    /// is refused.
    TextureArchiveOutsideSearch {
        /// The archive a caller tried to bind.
        archive: String,
        /// The directories this world searches, in its order.
        searched: Vec<String>,
    },
}

impl BindingError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoWorldGroup { .. } => "no_world_group",
            Self::ContainerSpelling { .. } => "container_spelling",
            Self::TextureArchiveAlreadyBound { .. } => "texture_archive_already_bound",
            Self::TextureArchiveOutsideSearch { .. } => "texture_archive_outside_search",
        }
    }
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWorldGroup { mission } => match mission {
                Some(mission) => write!(
                    f,
                    "the context names mission {mission:?} but no world group; every archive of a \
                     world load lives below ZBD/<world group>/, so the original's MISSION_DIR is \
                     only used below CAMPAIGN_DIR"
                ),
                None => f.write_str(
                    "the context names no world group; every archive of a world load lives below \
                     ZBD/<world group>/",
                ),
            },
            Self::ContainerSpelling {
                role,
                spelling,
                reason,
            } => write!(
                f,
                "the {role} container {spelling:?} cannot be spelled: {reason}"
            ),
            Self::TextureArchiveAlreadyBound { bound, second } => write!(
                f,
                "this world already binds the texture archive {bound:?}, so {second:?} cannot be \
                 added; the original opens exactly one texture archive per world"
            ),
            Self::TextureArchiveOutsideSearch { archive, searched } => write!(
                f,
                "the texture archive {archive:?} is not in this world's search ({}): the original \
                 looks the archive up in the world's own directory and then the shared one, never in \
                 another world",
                searched.join(", ")
            ),
        }
    }
}

impl std::error::Error for BindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ContainerSpelling { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// Every archive one world/mission load of the original binds.
///
/// This is the **layout** half of the measured lookup order: which archives are
/// opened, at which level and in which order the scripts name them. What
/// happens inside them is the format crates' work, and which of the world's
/// texture tiers is opened is task #352's rule, recorded here only as the file
/// it selected ([`TextureBinding`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldLayout {
    archives: Vec<BoundArchive>,
    texture: TextureBinding,
}

impl WorldLayout {
    /// The bindings of one context: the world's archives always, the mission's
    /// animation archive only when `context` names a mission.
    ///
    /// `shared_directory` is the installation's `ZBD` directory as
    /// [`crate::install::Diagnosis::zbd_dir`] spells it; it is where
    /// `planes.zbd` is read from and the texture search's fallback level.
    ///
    /// # Errors
    ///
    /// [`BindingError::NoWorldGroup`] when the context names no world group,
    /// and [`BindingError::ContainerSpelling`] when an assembled container
    /// cannot be spelled as a relative path (unreachable for spellings that
    /// came through the validating constructors).
    pub fn for_context(
        context: &ResolveContext,
        shared_directory: &RelativePath,
    ) -> Result<Self, BindingError> {
        let Some(world) = context.world_group.as_ref() else {
            return Err(BindingError::NoWorldGroup {
                mission: context
                    .mission
                    .as_ref()
                    .map(|mission| mission.as_str().to_owned()),
            });
        };
        let shared = shared_directory.as_str();
        let world_directory = world.as_relative().as_str();
        let mut archives = Vec::with_capacity(BindingRole::ALL.len());

        for role in BindingRole::ALL {
            if role == BindingRole::TextureArchive {
                continue;
            }
            // The mission archive is the one binding that exists only for a
            // mission: the original's `support\init.gw` spells it as
            // `zbd\<world>\<mission>\mis_anim.zbd` from MISSION_DIR, which a
            // world load without a mission does not set.
            if role == BindingRole::MissionAnimation && context.mission.is_none() {
                continue;
            }
            let mission = context
                .mission
                .as_ref()
                .map(|mission| format!("/{}/", mission.as_str()))
                .unwrap_or_default();
            let file = role
                .file()
                .expect("only the texture role has no fixed file name, and it is skipped here");
            // `ZBD/planes.zbd` is read from the shared directory; every other
            // named archive lives under the world group's directory, the
            // mission one one level deeper.
            let spelling = match role.level() {
                BindingLevel::Root => format!("{shared}/{file}"),
                BindingLevel::World => format!("{world_directory}/{file}"),
                BindingLevel::Mission => format!("{world_directory}{mission}{file}"),
            };
            let container =
                RelativePath::new(&spelling).map_err(|reason| BindingError::ContainerSpelling {
                    role,
                    spelling,
                    reason,
                })?;
            archives.push(BoundArchive::new(role, container));
        }
        archives.sort_by_key(BoundArchive::script_order);

        Ok(Self {
            archives,
            texture: TextureBinding::new(world.as_relative().clone(), shared_directory.clone()),
        })
    }

    /// The named archives, in the order the original's scripts name them.
    pub fn archives(&self) -> &[BoundArchive] {
        &self.archives
    }

    /// The archive bound for `role`, or `None` when this load does not bind it
    /// (the mission archive without a mission).
    pub fn archive(&self, role: BindingRole) -> Option<&BoundArchive> {
        self.archives.iter().find(|archive| archive.role() == role)
    }

    /// The world's one texture archive: where it is searched for, and the file
    /// the selection rule chose once it has run.
    pub fn texture(&self) -> &TextureBinding {
        &self.texture
    }

    /// The world's texture search, mutably, so a caller can record the file
    /// task #352's rule selected.
    pub fn texture_mut(&mut self) -> &mut TextureBinding {
        &mut self.texture
    }

    /// Evidence status of the bindings ([`BINDING_ORDER_STATUS`]).
    pub const fn order_status(&self) -> ClaimStatus {
        BINDING_ORDER_STATUS
    }

    /// The animation archives in load order: the world's `cam_anim.zbd` first,
    /// the mission's `mis_anim.zbd` second, each loaded completely.
    pub fn animation_archives(&self) -> impl Iterator<Item = &BoundArchive> {
        self.archives
            .iter()
            .filter(|archive| archive.family() == ArchiveFamily::Animation)
    }

    /// The GameZ archives in read order: the world's `gamez.zbd` first, then
    /// `planes.zbd` from the ZBD root.
    pub fn gamez_archives(&self) -> impl Iterator<Item = &BoundArchive> {
        self.archives
            .iter()
            .filter(|archive| archive.family() == ArchiveFamily::GameZ)
    }

    /// The installation-relative spellings this load binds that do not exist
    /// under `root`; empty when the installation holds every one of them.
    ///
    /// That covers the named archives, the texture archive the selection rule
    /// has bound ([`TextureBinding::archive`], when the rule has run) and the
    /// two directories the texture search walks. It is a **host listing** of
    /// the read-only installation: it never reads a byte of an archive and
    /// never writes. It exists so a consumer (and the acceptance tests) sees a
    /// binding this installation does not ship instead of opening it later and
    /// failing — so a layout naming an archive retail does not have is visible
    /// rather than silent.
    pub fn missing(&self, root: &Path) -> Vec<String> {
        let mut missing = Vec::new();
        for archive in &self.archives {
            if !root.join(archive.container().as_str()).is_file() {
                missing.push(archive.container().as_str().to_owned());
            }
        }
        if let Some(archive) = &self.texture.archive
            && !root.join(archive.as_str()).is_file()
        {
            missing.push(archive.as_str().to_owned());
        }
        for directory in self.texture.directories() {
            if !root.join(directory.as_str()).is_dir() {
                missing.push(format!("{}/", directory.as_str()));
            }
        }
        missing
    }
}

impl fmt::Display for WorldLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = self.archives.iter().map(BoundArchive::to_string).collect();
        write!(f, "{}", rendered.join("; "))
    }
}

/// One mission directory of the installation, below a world group.
///
/// The **mission level** of the measured layout: `ZBD/<world group>/<mission>`,
/// the directory whose `mis_anim.zbd` the original loads after the world's
/// `cam_anim.zbd` and whose `zrdr.zbd` it mounts between the root and world
/// reader archives (finding sections A and B). The designed layout mounted
/// whole world group directories and had no place for this level, so a mission
/// archive had no key at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionDirectory {
    /// The world group directory, in the installation's own spelling
    /// (`zbd/C1C`).
    pub world_group: RelativePath,
    /// The mission directory name, in the installation's own spelling (`M01`).
    pub mission: String,
    /// The whole directory, in the installation's own spelling
    /// (`zbd/C1C/M01`).
    pub directory: RelativePath,
}

impl MissionDirectory {
    /// The world group this mission belongs to.
    pub fn world(&self) -> WorldGroup {
        WorldGroup::from_relative(self.world_group.clone())
    }

    /// The mission scope this directory binds, which is its own name folded
    /// to a label ([`MissionScope`] holds a lower-case label, while a
    /// directory is spelled `M01`).
    pub fn scope(&self) -> Result<MissionScope, LabelError> {
        MissionScope::new(&self.mission.to_lowercase())
    }
}

impl fmt::Display for MissionDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.directory.as_str())
    }
}

/// Every mission directory the discovery observed, sorted by logical key.
///
/// A mission directory is one **directly** below a discovered world group
/// directory (`zbd/<world group>/<mission>`), which is the level the original's
/// `MISSION_DIR` names; the mission-name tables are campaign-type dependent
/// (finding section A) and are not consulted here, so the list is what the
/// installation holds rather than what any table claims.
///
/// The list is derived from [`Diagnosis::directories`] rather than from file
/// paths, so a directory that holds no regular file still appears — a mission
/// whose archives were all removed must be reportable, not silently absent.
pub fn mission_directories(diagnosis: &Diagnosis) -> Vec<MissionDirectory> {
    let mut missions = Vec::new();
    for group in &diagnosis.world_groups {
        let prefix = format!("{}/", group.logical_key());
        let mut found: Vec<MissionDirectory> = diagnosis
            .directories
            .iter()
            .filter(|directory| {
                directory
                    .logical_key()
                    .strip_prefix(&prefix)
                    .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
            })
            .filter_map(|directory| {
                // The installation's own spelling of the directory name, not
                // its folded form: discovery records spellings and
                // `MissionDirectory::scope` folds when it builds the label.
                let mission = directory.as_str().rsplit('/').next()?;
                (!mission.is_empty()).then(|| MissionDirectory {
                    world_group: group.clone(),
                    mission: mission.to_owned(),
                    directory: directory.clone(),
                })
            })
            .collect();
        found.sort_by_key(|mission| mission.directory.logical_key());
        missions.append(&mut found);
    }
    missions
}
