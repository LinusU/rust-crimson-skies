//! Reader archives (`zrdr.zbd`) as mounted members: **basename keys** and the
//! original's **[root, mission, world] mount order**, first hit wins (F04-D,
//! task #685).
//!
//! What the original engine does, as the owner's static analysis of the
//! pinned executable records it (`docs/findings/
//! 2026-10-05-f04-d-original-lookup-order.md`, sections A and G):
//!
//! 1. the persistent **root** archive `ZBD/zrdr.zbd` is mounted at startup;
//! 2. loading a world/mission unmounts every archive but the root, then mounts
//!    `ZBD/<world>/<mission>/zrdr.zbd` and then `ZBD/<world>/zrdr.zbd`, so the
//!    mount list is **[root, mission, world]**;
//! 3. an open reduces the requested name to its **basename**, walks the
//!    archives in mount order, and inside one archive scans its index
//!    case-insensitively and serves the **first** entry that matches — so the
//!    first archive holding the name wins, and inside it the first entry.
//!
//! Those three rules plus one of the two loose rules of section A are what
//! this module implements:
//!
//! 4. **loose directory fallback** — only when *no* mounted archive holds the
//!    basename, the loose pass searches the **most recently added** loose
//!    directory first and `zbd` last (task #700). A loose file is registered
//!    per directory ([`ReaderLooseDirectory`]) and serves a lookup no archive
//!    can answer, exactly as the original's loose pass does.
//!
//! # The loose-file override is deliberately **not** decided
//!
//! The original also compares a loose file of the same basename against the
//! archive copy and keeps the newer one (`CompareFileTime >= 1`, section A).
//! Deciding that needs a time on the archive side, and the only candidate the
//! bytes offer — the index entry's trailing `u64` — is **unknown**: nothing
//! measured shows the original reading it, and task **#692** owns those 76
//! bytes. So this module does not compare anything. Instead a lookup that
//! finds **both** an archive member and a loose file of the same basename is
//! **refused** ([`ReaderLookupError::LooseOverrideUndecided`]) with the real
//! refusal, never answered with the archive member and never answered with the
//! loose file: either choice would be a comparison nobody measured. That
//! refusal happens whether the loose file is newer or older, because the
//! archive side of the comparison is unknown — the host's modification time is
//! not the original's `CompareFileTime` argument.
//!
//! A lookup that finds a loose file of the same basename and **no** archive
//! member is decided by rule 4 alone and needs no comparison at all.
//!
//! [`READER_LOOSE_OVERRIDE_STATUS`] is therefore [`ClaimStatus::Unknown`]:
//! the rule exists, the decision is not implemented.
//!
//! # Why a reader member is not a `Vfs::resolve` answer
//!
//! The designed precedence classes rank a mission- or world-specific source
//! **above** a shared one, while the original's reader order is the other way
//! round: the root archive shadows both, and the order is *mount order*, not
//! rank. [`PRECEDENCE_ORDER_STATUS`] therefore stays `designed` and is not
//! consulted by a reader lookup at all; a reader trace reports its own
//! [`READER_LOOKUP_ORDER_STATUS`] instead, so no consumer can read these
//! answers as the designed order or as a measured runtime behaviour. The
//! evidence for the order is static analysis of the original executable:
//! real evidence, but `inferred`, never `verified_original`.
//!
//! # What a duplicate member name means
//!
//! Inside one archive the first index entry wins and a **later** entry of the
//! same name is unreachable by name: the retail root archive declares
//! `player.zrd` twice (entry 22 is the only copy a lookup can return, entry
//! 100 is not). That entry is not dropped — it stays an inventory row with its
//! own extent, digest and [`Unreachable::DuplicateName`] reason
//! ([`ReaderMember`]) — while only the first entry becomes a mount member.
//! This is the original's own first-hit rule inside one archive, not a
//! flattening of equal-priority sources: three archives holding one name stay
//! three members of three mounts, and the order that decides between them is
//! the trace's.
//!
//! # Variants and mount ids
//!
//! A reader archive declares **one** copy of each name and indexes it at
//! [`AssetVariant::default`], so [`ReaderMounts::resolve`] refuses a request
//! that specializes a variant ([`ReaderLookupError::ForeignVariant`]) instead of
//! answering with the default copy's bytes, and a request in another namespace
//! is refused outright. Two archives registered under the same mount id make
//! [`ReaderMounts::read`]'s attribution ambiguous, so that read is refused too
//! ([`ReaderReadError::AmbiguousArchive`]) rather than served from whichever
//! archive happens to come first.
//!
//! # Bytes
//!
//! A reader member is stored uncompressed inside its container, so
//! [`ReaderArchive`] owns the bytes the mount was built from and
//! [`ReaderArchive::read`] slices a member's extent out of them, checking it
//! against the digest recorded at mount time. The mount itself has no
//! directory backing, so `Vfs::read_all` on a reader member reports `no
//! backing` instead of returning container bytes as if they were host files —
//! the same split a ROF mount has ([`crate::rof`]).
//!
//! A **loose** file is a host file instead, so it is read through the same
//! digest check ([`ReaderMounts::read`]) and never through a mount: it has no
//! [`Mount`] and no [`SourceSpan`] member key, only the installation-relative
//! path of the file itself.
//!
//! Nothing here writes to the installation.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use cs_formats::ParseContext;
use cs_formats::zbd::{
    IndexError, ReaderError, ZbdDispatchError, ZbdProbe, dispatch, read_reader_archive,
    read_version_one_index,
};
use cs_types::asset_id::{
    AssetKey, AssetKeyError, AssetVariant, MountId, MountNamespace, ResolveContext, SourceSpan,
};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::{RelativePath, RelativePathError};

use crate::install::sha256;
use crate::vfs::mount::{Mount, MountBuilder, MountError, SkipReason};

/// The key space reader members are mounted and looked up in: an
/// engine-authored label (spec F04's open, validated namespace vocabulary),
/// not an observed retail namespace.
pub const READER_NAMESPACE: &str = "reader";

/// How well the reader lookup order implemented here is known.
///
/// The order — root, then mission, then world; basename keys; first entry of
/// the first archive that holds the name — is read off the original
/// executable's code (`docs/findings/2026-10-05-f04-d-original-lookup-order.md`
/// section A), which is evidence but **not** a runtime capture. It is
/// therefore `inferred`, and this module never presents it as
/// [`ClaimStatus::VerifiedOriginal`].
///
/// [`PRECEDENCE_ORDER_STATUS`] — the *designed* precedence order, which the
/// original lookup was compared against — is deliberately left at `designed`
/// and is not consulted by a reader lookup.
pub const READER_LOOKUP_ORDER_STATUS: ClaimStatus = ClaimStatus::Inferred;

/// How well the original's **loose directory** search order is known.
///
/// The most recently added directory is searched first and `zbd` last, because
/// `zbd` is the oldest addition (section A, startup `0x4a6ff0`). That order is
/// read off the original executable's code, exactly like the archive order
/// above, so it is [`ClaimStatus::Inferred`] and never
/// [`ClaimStatus::VerifiedOriginal`].
pub const READER_LOOSE_ORDER_STATUS: ClaimStatus = ClaimStatus::Inferred;

/// How well the original's **loose-file override** is known, and why this
/// module refuses instead of deciding it.
///
/// The rule exists in the original (section A: a loose file of the same
/// basename that is newer, `CompareFileTime >= 1`, overrides the archive copy
/// the archive pass found). The comparison is **not implemented**, because the
/// archive side of it needs the index entry's trailing `u64`, whose meaning is
/// unknown and which nothing measured shows the original reading (section F;
/// task #692 owns those 76 bytes). So the rule's status here is
/// [`ClaimStatus::Unknown`]: known to exist, not implemented, and a lookup that
/// would have to apply it is refused
/// ([`ReaderLookupError::LooseOverrideUndecided`]).
pub const READER_LOOSE_OVERRIDE_STATUS: ClaimStatus = ClaimStatus::Unknown;

/// The installation-relative spelling of the original's default reader
/// directory `zbd`: the archive directory itself, which the loose pass also
/// searches as the **oldest** addition (section A).
pub const READER_ROOT_DIRECTORY: &str = "zbd";

/// The loose reader directories the original adds for one world/mission load,
/// **in the order the executable appends them** (section A, startup `0x4a6ff0`
/// and world/mission load `0x463cb0`):
///
/// # Case
///
/// The finding spells these paths in lower case (`common`, `<w>`, `<w>\nets`,
/// `<w>\<m>`) while this installation's `ZBD` directories are upper case, so
/// on a case-sensitive host `data/c1c` and `data/C1C` are different
/// directories. Nothing measured shows the original's Windows filesystem
/// case-insensitivity choosing between them — no loose reader directory exists
/// in retail — so **this function folds no case**: it builds what it is given,
/// and the caller's spelling stands. A caller that knows the host's spelling
/// passes it.
///
/// | order | spelling the original adds | added by |
/// | --- | --- | --- |
/// | 1 | `zbd` | startup default directory list |
/// | 2 | `data/common/zrdr` | startup loose reader path `..\data\common\zrdr` |
/// | 3 | `data/common` | world/mission load |
/// | 4 | `data/<w>` | world/mission load |
/// | 5 | `data/<w>/nets` | world/mission load |
/// | 6 | `data/<w>/<m>` | world/mission load |
///
/// A world/mission load adds the last four; a startup-only list is the first
/// two.
///
/// The loose pass searches this list **backwards** — the most recently added
/// directory first, `zbd` last (section A: "searches the most recently added
/// directory first, then `zbd`").
///
/// `world` and `mission` are the world **directory** and mission **directory**
/// names the original's loose path spells (`c1c`, `mp1`), not the `zbd/<group>`
/// world-group spelling; a value carrying a separator is refused
/// ([`ReaderLooseError::NotADirectoryName`]) instead of being joined into a
/// path nobody measured.
///
/// `world = None` yields the two startup directories only; `mission = None`
/// with a world yields the four directories a world load adds. A `mission` with
/// no `world` is refused ([`ReaderLooseError::MissionWithoutWorld`]) rather than
/// dropped, because the original spells a mission directory below its world
/// directory.
pub fn original_loose_reader_directories(
    world: Option<&str>,
    mission: Option<&str>,
) -> Result<Vec<RelativePath>, ReaderLooseError> {
    let directory_name = |spelling: &str| {
        if spelling.contains(['/', '\\']) {
            return Err(ReaderLooseError::NotADirectoryName {
                spelling: spelling.to_owned(),
            });
        }
        RelativePath::new(spelling).map_err(ReaderLooseError::Spelling)
    };
    let joined = |prefix: &str, suffix: &str| {
        let spelling = format!("{prefix}/{suffix}");
        RelativePath::new(&spelling).map_err(ReaderLooseError::Spelling)
    };

    // Startup `0x4a6ff0`: the default directory list gets `zbd`, and the loose
    // reader path is `..\data\common\zrdr`.
    let mut directories = vec![
        RelativePath::new(READER_ROOT_DIRECTORY).map_err(ReaderLooseError::Spelling)?,
        RelativePath::new("data/common/zrdr").map_err(ReaderLooseError::Spelling)?,
    ];
    let Some(world) = world else {
        // A mission directory is spelled below its world directory, so a mission
        // without a world cannot be placed in the list. It is refused rather than
        // dropped, because silently returning a list without the mission the
        // caller asked for would answer a different lookup than the one requested.
        if let Some(mission) = mission {
            return Err(ReaderLooseError::MissionWithoutWorld {
                mission: mission.to_owned(),
            });
        }
        return Ok(directories);
    };
    // World/mission load `0x463cb0` appends `common`, `<w>`, `<w>\nets` and
    // `<w>\<m>` below `..\data`.
    let world = directory_name(world)?;
    directories.push(RelativePath::new("data/common").map_err(ReaderLooseError::Spelling)?);
    directories.push(joined("data", world.as_str())?);
    directories.push(joined(&format!("data/{world}"), "nets")?);
    if let Some(mission) = mission {
        let mission = directory_name(mission)?;
        directories.push(joined(&format!("data/{world}"), mission.as_str())?);
    }
    Ok(directories)
}

/// Why a loose reader directory could not be declared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderLooseError {
    /// The spelling is not a valid installation-relative spelling, so no key,
    /// span or host path could be built from it.
    Spelling(RelativePathError),
    /// A world or mission component carries a separator, so it cannot name one
    /// loose directory. [`original_loose_reader_directories`] takes the world
    /// **directory** name (`c1c`), not the `zbd/<group>` world-group spelling.
    NotADirectoryName {
        /// The component that was refused.
        spelling: String,
    },
    /// A mission was named without a world, so its directory cannot be spelled.
    MissionWithoutWorld {
        /// The mission that could not be placed.
        mission: String,
    },
}

impl ReaderLooseError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Spelling(_) => "spelling",
            Self::NotADirectoryName { .. } => "not_a_directory_name",
            Self::MissionWithoutWorld { .. } => "mission_without_world",
        }
    }
}

impl fmt::Display for ReaderLooseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spelling(reason) => {
                write!(
                    f,
                    "the loose reader directory spelling is refused: {reason}"
                )
            }
            Self::NotADirectoryName { spelling } => write!(
                f,
                "{spelling:?} carries a separator, so it cannot name one loose reader directory; \
                 pass the world/mission directory name (c1c, mp1), not a world-group spelling \
                 (zbd/c1c)"
            ),
            Self::MissionWithoutWorld { mission } => write!(
                f,
                "mission {mission:?} was named without a world, so its loose reader directory \
                 cannot be spelled; the original places `<w>\\<m>` below its world directory"
            ),
        }
    }
}

impl std::error::Error for ReaderLooseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spelling(reason) => Some(reason),
            Self::NotADirectoryName { .. } | Self::MissionWithoutWorld { .. } => None,
        }
    }
}

/// One loose reader directory: an installation-relative spelling and the host
/// root it is searched in.
///
/// The original adds its loose directories to one list at startup and per
/// world/mission load, and searches that list **most recently added first**. A
/// [`ReaderMounts`] set therefore records them in *addition* order
/// ([`ReaderMounts::add_loose_directory`]) and searches them backwards
/// ([`ReaderMounts::loose_search_order`]); a directory that does not exist on
/// this host counts for nothing, exactly as the original counts "only existing
/// directories" (section A) — and none of them exists in retail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderLooseDirectory {
    spelling: RelativePath,
    host_root: PathBuf,
}

impl ReaderLooseDirectory {
    /// Declares the loose reader directory `spelling` below the installation
    /// root `root`. Nothing is read or written and the directory need not
    /// exist: its absence is what the lookup reports
    /// ([`ReaderLooseOutcome::Absent`]).
    pub fn declare(root: &Path, spelling: &str) -> Result<Self, ReaderLooseError> {
        let spelling = RelativePath::new(spelling).map_err(ReaderLooseError::Spelling)?;
        Ok(Self {
            spelling,
            host_root: root.to_path_buf(),
        })
    }

    /// The directory's installation-relative spelling, as the original adds it.
    pub fn spelling(&self) -> &str {
        self.spelling.as_str()
    }

    /// The host path of the directory itself.
    pub fn host_path(&self) -> PathBuf {
        self.host_root.join(self.spelling.as_str())
    }

    /// The host path of `basename` inside this directory.
    ///
    /// `basename` is the last component of a validated [`RelativePath`], so it
    /// is one component with no separator, no `..` and no NUL: joining it here
    /// cannot leave the declared directory.
    pub fn candidate_path(&self, basename: &str) -> PathBuf {
        self.host_path().join(basename)
    }
}

/// One level of the original's reader mount list.
///
/// The level **is** the mount order: the root archive is mounted first and
/// therefore searched first, the mission archive before the world archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReaderLevel {
    /// `ZBD/zrdr.zbd`: the persistent root archive, mounted at startup.
    Root,
    /// `ZBD/<world>/<mission>/zrdr.zbd`: mounted for the mission being loaded.
    Mission,
    /// `ZBD/<world>/zrdr.zbd`: mounted for the world being loaded.
    World,
}

impl ReaderLevel {
    /// The original's mount order: the root archive is searched before the
    /// mission archive, and the mission archive before the world archive. A
    /// higher rank is searched earlier.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Root => 2,
            Self::Mission => 1,
            Self::World => 0,
        }
    }

    /// The stable label used in traces and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Mission => "mission",
            Self::World => "world",
        }
    }
}

impl fmt::Display for ReaderLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why one declared entry of a reader archive cannot be reached by name.
///
/// Every variant keeps the entry an inventory row; none of them hides bytes or
/// drops a record (IDENTITY-CONTENT: "collections cannot exclude failed
/// entries").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreachable {
    /// The entry's extent failed the listing's bounds check, so the archive
    /// declares a member it cannot hand out.
    FailedBounds,
    /// The entry's name is not UTF-8, so no key spelling could match it.
    NonUtf8Name,
    /// The entry's name cannot be spelled as a key (`..`, a `.` component, an
    /// absolute spelling, a drive prefix or a NUL byte).
    InvalidName,
    /// An **earlier** entry of the same archive declares the same name, so the
    /// original's first-hit scan serves that one and this entry is
    /// unreachable. `served_by` is the entry index a lookup returns.
    DuplicateName {
        /// The entry index that answers this member's name.
        served_by: usize,
    },
}

impl Unreachable {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FailedBounds => "failed_bounds",
            Self::NonUtf8Name => "non_utf8_name",
            Self::InvalidName => "invalid_name",
            Self::DuplicateName { .. } => "duplicate_name",
        }
    }
}

impl fmt::Display for Unreachable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FailedBounds => {
                f.write_str("the member's extent failed the archive's bounds check")
            }
            Self::NonUtf8Name => f.write_str("the member's name is not UTF-8"),
            Self::InvalidName => f.write_str("the member's name cannot be spelled as a key"),
            Self::DuplicateName { served_by } => {
                write!(f, "an earlier entry ({served_by}) serves this name")
            }
        }
    }
}

/// One declared entry of a reader archive.
///
/// The row exists whether or not a lookup can reach the entry, so the archive's
/// inventory keeps every member the original declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderMember {
    /// The entry's position in the archive's declared index.
    pub entry_index: usize,
    /// The member's name exactly as the archive spells it, or `None` when
    /// those bytes are not UTF-8.
    pub name: Option<String>,
    /// The member's first byte inside the container, as the index declares it.
    pub offset: u64,
    /// The member's length in bytes, as the index declares it.
    pub length: u64,
    /// SHA-256 of exactly those bytes, when the archive could hand them out.
    pub sha256: Option<ContentHash>,
    /// Why no lookup by name reaches this entry, or `None` when one does.
    pub unreachable: Option<Unreachable>,
}

impl ReaderMember {
    /// Whether a lookup by this member's name returns this entry.
    pub fn reachable_by_name(&self) -> bool {
        self.unreachable.is_none()
    }

    /// The member's name, or `None` when the archive spelled it in bytes that
    /// are not UTF-8. An entry can be named and still be unreachable — see
    /// [`Self::unreachable`] — so this is the name, not the answer.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

/// One mounted reader archive: its [`Mount`] and the member rows its own index
/// declares.
#[derive(Debug)]
pub struct ReaderArchive {
    mount: Mount,
    level: ReaderLevel,
    container: String,
    path: PathBuf,
    bytes: Vec<u8>,
    members: Vec<ReaderMember>,
}

impl ReaderArchive {
    /// The VFS mount holding the members a lookup can reach by name.
    pub fn mount(&self) -> &Mount {
        &self.mount
    }

    /// Which level of the original's mount list this archive occupies.
    pub const fn level(&self) -> ReaderLevel {
        self.level
    }

    /// The archive's installation spelling, recorded in every span it produces.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The host path the archive was read from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every declared entry, in index order, reachable or not.
    pub fn members(&self) -> &[ReaderMember] {
        &self.members
    }

    /// How many entries the archive's index declares.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the archive declares no entry.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The entry at `entry_index`, reachable or not.
    pub fn member_at(&self, entry_index: usize) -> Option<&ReaderMember> {
        self.members
            .iter()
            .find(|member| member.entry_index == entry_index)
    }

    /// The entry a lookup by `name` returns: the first reachable entry whose
    /// name matches case-insensitively, exactly as the original's scan does.
    ///
    /// A name that no entry reaches answers `None`, so an entry the archive
    /// declares twice serves its **first** copy and the later one is only
    /// reachable through [`Self::member_at`].
    pub fn member(&self, name: &str) -> Option<&ReaderMember> {
        self.members.iter().find(|member| {
            member.reachable_by_name()
                && member
                    .name
                    .as_deref()
                    .is_some_and(|declared| declared.eq_ignore_ascii_case(name))
        })
    }

    /// The entries no lookup by name reaches.
    pub fn unreachable(&self) -> impl Iterator<Item = &ReaderMember> {
        self.members
            .iter()
            .filter(|member| !member.reachable_by_name())
    }

    /// The bytes a resolution names, digest-checked against the digest this
    /// archive was mounted with.
    ///
    /// # Errors
    ///
    /// [`ReaderReadError::LooseOrigin`] when the resolution names a loose host
    /// file instead of a member of this archive,
    /// [`ReaderReadError::ForeignArchive`] when the resolution names another
    /// archive, [`ReaderReadError::StaleResolution`] when it no longer
    /// describes the entry it was made for, and
    /// [`ReaderReadError::DigestMismatch`] when the stored bytes do not hash to
    /// what the mount recorded.
    pub fn read(&self, resolved: &ReaderResolution) -> Result<Vec<u8>, ReaderReadError> {
        // A loose resolution names no archive member at all: reading it through
        // an archive would return another source's bytes under this archive's
        // provenance, so it is refused (`ReaderMounts::read` reads it from the
        // host instead).
        let ReaderOrigin::Archive {
            mount, entry_index, ..
        } = &resolved.origin
        else {
            return Err(ReaderReadError::LooseOrigin {
                archive: self.mount.id().to_string(),
            });
        };
        if self.mount.id() != mount {
            return Err(ReaderReadError::ForeignArchive {
                mount: mount.to_string(),
                expected: self.mount.id().to_string(),
            });
        }
        let member =
            self.member_at(*entry_index)
                .ok_or_else(|| ReaderReadError::StaleResolution {
                    origin: self.mount.id().to_string(),
                })?;
        let span = &resolved.span;
        let describes_entry = span.container_path() == self.container
            && span.member_key() == member.name.as_deref()
            && span.offset() == member.offset
            && span.length() == member.length
            && span.member_sha256() == member.sha256;
        if !describes_entry {
            return Err(ReaderReadError::StaleResolution {
                origin: self.mount.id().to_string(),
            });
        }
        self.read_entry(member)
    }

    /// The stored bytes of one entry, digest-checked.
    ///
    /// # Errors
    ///
    /// [`ReaderReadError::OutOfBounds`] when the declared extent does not lie
    /// inside the bytes this archive was built from, and
    /// [`ReaderReadError::DigestMismatch`] when the bytes do not hash to what
    /// the mount recorded.
    pub fn read_entry(&self, member: &ReaderMember) -> Result<Vec<u8>, ReaderReadError> {
        let archive_len = self.bytes.len() as u64;
        let out_of_bounds = || ReaderReadError::OutOfBounds {
            entry_index: member.entry_index,
            offset: member.offset,
            length: member.length,
            archive_len,
        };
        // The listing already proved both ends of every reachable extent lie
        // inside the container, so these checks only guard a caller that hands
        // this function a row from somewhere else.
        let start = usize::try_from(member.offset).map_err(|_| out_of_bounds())?;
        let length = usize::try_from(member.length).map_err(|_| out_of_bounds())?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(out_of_bounds)?;
        let bytes = self.bytes[start..end].to_vec();
        let Some(mounted) = member.sha256 else {
            return Ok(bytes);
        };
        let found = sha256(&bytes);
        if found != mounted {
            return Err(ReaderReadError::DigestMismatch {
                member: member.name.clone().unwrap_or_default(),
                entry_index: member.entry_index,
                mounted,
                found,
            });
        }
        Ok(bytes)
    }
}

/// The reader archives mounted for one context, in the original's mount order,
/// plus the loose reader directories the original's loose pass would search.
///
/// The set owns its archives and their bytes, so a resolution stays readable
/// for as long as the set lives and no file handle outlives a session (spec F04
/// non-negotiable behavior 4). Loose directories own no bytes: a loose file is
/// read from the host when a lookup serves or re-reads it, never held open.
#[derive(Debug, Default)]
pub struct ReaderMounts {
    archives: Vec<ReaderArchive>,
    loose: Vec<ReaderLooseDirectory>,
}

impl ReaderMounts {
    /// An empty set: no reader archive and no loose directory is mounted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one archive. The set searches by [`ReaderLevel::rank`], not by
    /// insertion order, so the order the original mounts in is the order the
    /// lookup uses whatever order the archives were added in.
    ///
    /// Mount ids are expected to be unique in one set: [`Self::read`] finds its
    /// archive by id, and refuses to guess when more than one answers to it
    /// ([`ReaderReadError::AmbiguousArchive`]).
    pub fn push(&mut self, archive: ReaderArchive) -> &mut Self {
        self.archives.push(archive);
        self
    }

    /// Adds one loose reader directory, **in the order the original appends
    /// it**. The lookup searches the registered directories backwards — the
    /// most recently added first, `zbd` last — so the addition order is the
    /// caller's declaration of the original's append order, not a search
    /// order.
    ///
    /// [`original_loose_reader_directories`] returns the original's list in that
    /// append order, and [`Self::add_original_loose_directories`] registers all
    /// of it.
    pub fn add_loose_directory(&mut self, directory: ReaderLooseDirectory) -> &mut Self {
        self.loose.push(directory);
        self
    }

    /// Registers the original's loose reader directories for one world/mission
    /// load below the installation root `root`, in the order the executable
    /// appends them.
    ///
    /// `world` and `mission` are the world and mission **directory** names the
    /// original's loose path spells (`c1c`, `mp1`), not the `zbd/<group>`
    /// world-group spelling; see [`original_loose_reader_directories`].
    ///
    /// # Errors
    ///
    /// [`ReaderLooseError`] when a directory spelling is not a valid
    /// installation-relative spelling. Nothing is registered when the list
    /// cannot be built.
    pub fn add_original_loose_directories(
        &mut self,
        root: &Path,
        world: Option<&str>,
        mission: Option<&str>,
    ) -> Result<&mut Self, ReaderLooseError> {
        let spellings = original_loose_reader_directories(world, mission)?;
        for spelling in spellings {
            self.loose
                .push(ReaderLooseDirectory::declare(root, spelling.as_str())?);
        }
        Ok(self)
    }

    /// The loose reader directories, in the order they were added — the
    /// original's append order, **not** its search order.
    pub fn loose_directories(&self) -> &[ReaderLooseDirectory] {
        &self.loose
    }

    /// The loose reader directories in the order the original's loose pass
    /// searches them: most recently added first, and the oldest addition (the
    /// `zbd` default directory) last.
    pub fn loose_search_order(&self) -> impl Iterator<Item = &ReaderLooseDirectory> {
        self.loose.iter().rev()
    }

    /// How many archives are mounted. A loose reader directory is not an archive,
    /// so it is not counted here; [`Self::loose_directories`] counts those.
    pub fn len(&self) -> usize {
        self.archives.len()
    }

    /// Whether no **archive** is mounted. This says nothing about loose reader
    /// directories: a set with none of them mounted still consults the original's
    /// loose pass. Use [`Self::len`] and [`Self::loose_directories`] together for
    /// what a set holds.
    pub fn is_empty(&self) -> bool {
        self.archives.is_empty()
    }

    /// The mounted archives, in the order they were added.
    pub fn archives(&self) -> &[ReaderArchive] {
        &self.archives
    }

    /// The archive registered under `mount`.
    pub fn archive(&self, mount: &MountId) -> Option<&ReaderArchive> {
        self.archives
            .iter()
            .find(|archive| archive.mount.id() == mount)
    }

    /// How many archives are registered under `mount`. One is the answer a
    /// resolution can name; more than one is ambiguous, and
    /// [`Self::archive`] cannot say which one a resolution came from.
    pub fn archives_named(&self, mount: &MountId) -> usize {
        self.archives
            .iter()
            .filter(|archive| archive.mount.id() == mount)
            .count()
    }

    /// Looks `requested` up the way the original engine opens a reader name.
    ///
    /// The lookup is four rules, in order:
    ///
    /// 1. the requested path is reduced to its **basename** (`targets.zrd`,
    ///    whatever directory the caller spelled);
    /// 2. archives whose [`MountScope`](crate::vfs::mount::MountScope) the
    ///    context does not admit are skipped and recorded as such — the
    ///    original only mounts the archives of the world and mission being
    ///    loaded;
    /// 3. the remaining archives are searched in the original's mount order
    ///    (root, then mission, then world) and the **first** one holding the
    ///    name serves it;
    /// 4. the loose pass (`0x579710` → `0x59d170`) runs **only when no
    ///    archive held the name**: the registered loose reader directories are
    ///    searched most recently added first, `zbd` last, and the first
    ///    regular file of that basename serves the lookup.
    ///
    /// Archives searched after the winner are still reported, as
    /// [`ReaderAttemptOutcome::Shadowed`], because "the world copy was there
    /// and the mission copy won" is the fact a diagnostic has to be able to
    /// see. The same holds for loose directories searched after a loose
    /// winner ([`ReaderLooseOutcome::Shadowed`]).
    ///
    /// When an archive **and** a loose file of the same basename both exist,
    /// rule 3's answer is not the original's answer and this lookup **refuses**
    /// ([`ReaderLookupError::LooseOverrideUndecided`]): deciding would mean
    /// applying the `CompareFileTime` override, whose archive-side timestamp is
    /// unknown ([`READER_LOOSE_OVERRIDE_STATUS`]).
    ///
    /// # Errors
    ///
    /// [`ReaderLookupError::ForeignNamespace`] when `requested` is not a
    /// reader key, [`ReaderLookupError::ForeignVariant`] when it specializes a
    /// variant (a reader archive declares one copy of each name and indexes it
    /// at [`AssetVariant::default`], so it cannot answer a variant), and
    /// [`ReaderLookupError::NotFound`] when neither a mounted archive the
    /// context admits nor a loose reader directory holds the name.
    ///
    /// [`ReaderLookupError::UnspellableBasename`] and
    /// [`ReaderLookupError::EmptyBasename`] exist so the reduction is total
    /// rather than panicking; both are unreachable for a key built by
    /// [`AssetKey::from_spelling`], whose logical key comes from a validated
    /// [`RelativePath`].
    pub fn resolve(
        &self,
        context: &ResolveContext,
        requested: &AssetKey,
    ) -> Result<ReaderResolution, ReaderLookupError> {
        if requested.namespace().as_str() != READER_NAMESPACE {
            return Err(ReaderLookupError::ForeignNamespace {
                namespace: requested.namespace().clone(),
            });
        }
        // A reader archive declares exactly one copy of each name and indexes it
        // at the default variant, so a request that specializes a variant asks
        // for something no mounted archive can answer. It is refused rather
        // than answered with the default copy, which would be different bytes
        // than the caller asked for. The retail variant vocabulary is unknown
        // (`AssetVariant::default`), so no variant is claimed to exist.
        if requested.variant() != &AssetVariant::default() {
            return Err(ReaderLookupError::ForeignVariant {
                requested: Box::new(requested.clone()),
                variant: requested.variant().as_str().to_owned(),
            });
        }
        // `AssetKey`'s path is a validated `RelativePath`, so its logical key
        // has no empty component and the last one is the basename the original
        // reduces the request to.
        let path_key = requested.path_key();
        let Some(basename) = path_key.rsplit('/').next().filter(|part| !part.is_empty()) else {
            return Err(ReaderLookupError::EmptyBasename {
                requested: Box::new(requested.clone()),
            });
        };
        let member_key =
            AssetKey::from_spelling(READER_NAMESPACE, basename, AssetVariant::default().as_str())
                .map_err(|error| ReaderLookupError::UnspellableBasename {
                requested: Box::new(requested.clone()),
                basename: basename.to_owned(),
                error,
            })?;

        let mut order: Vec<usize> = (0..self.archives.len()).collect();
        order.sort_by_key(|index| {
            (
                std::cmp::Reverse(self.archives[*index].level.rank()),
                *index,
            )
        });

        let mut attempts: Vec<ReaderAttempt> = Vec::with_capacity(order.len());
        // `(archive index, entry index)` of the first archive that held the
        // name: the original serves exactly that one and stops.
        let mut selected: Option<(usize, usize)> = None;
        for index in order {
            let archive = &self.archives[index];
            let attempt = ReaderAttempt {
                mount: archive.mount.id().clone(),
                container: archive.container.clone(),
                level: archive.level,
                outcome: match archive.mount.scope().admit(context) {
                    Err(reason) => ReaderAttemptOutcome::Skipped(reason),
                    Ok(()) => match archive.member(basename) {
                        None => ReaderAttemptOutcome::Miss,
                        Some(member) if selected.is_none() => {
                            selected = Some((index, member.entry_index));
                            ReaderAttemptOutcome::Selected {
                                entry_index: member.entry_index,
                            }
                        }
                        Some(member) => ReaderAttemptOutcome::Shadowed {
                            entry_index: member.entry_index,
                        },
                    },
                },
            };
            attempts.push(attempt);
        }

        let Some((index, entry_index)) = selected else {
            // Rule 4: the original's loose pass runs exactly here, because no
            // archive holds the name.
            return self.resolve_loose(context, requested, basename, attempts);
        };

        // An archive holds the name. The original would still look for a loose
        // file of the same basename and keep the newer one, so the loose pass
        // is run for what it can decide — nothing — and its finding is a
        // refusal, not an answer.
        let LoosePass {
            attempts: loose,
            candidate,
        } = self.loose_pass(LoosePassMode::Override, requested, basename)?;
        if let Some(candidate) = candidate {
            let archive = &self.archives[index];
            return Err(ReaderLookupError::LooseOverrideUndecided {
                requested: Box::new(requested.clone()),
                basename: basename.to_owned(),
                directory: candidate.directory,
                host_path: candidate.host_path.into_boxed_path(),
                size_bytes: candidate.size_bytes,
                archive_container: archive.container.clone(),
                trace: Box::new(reader_trace(attempts, loose)),
            });
        }

        let archive = &self.archives[index];
        // `ReaderArchive::member` answers from the rows this mount was built
        // from, so the mount holds that row under exactly this key.
        let member = archive
            .mount
            .member(&member_key)
            .expect("the selected archive holds the key its own member index reported");
        let spelling = member.spelling().as_str().to_owned();
        let span = SourceSpan::new(
            context.installation,
            archive.container(),
            Some(&spelling),
            member.offset(),
            member.size_bytes(),
            member.sha256(),
        )
        .expect(
            "MountBuilder validated the container spelling, the member spelling and the member \
             byte range before the mount existed",
        );

        Ok(ReaderResolution {
            requested: requested.clone(),
            basename: basename.to_owned(),
            container: archive.container.clone(),
            member: spelling,
            span,
            origin: ReaderOrigin::Archive {
                mount: archive.mount.id().clone(),
                level: archive.level,
                entry_index,
            },
            trace: reader_trace(attempts, loose),
        })
    }

    /// Rule 4 of the lookup, and the answer when no archive holds the name: the
    /// original's loose pass.
    ///
    /// [`LoosePassMode::Fallback`] selects the first regular candidate, whose
    /// bytes are then read once and digest-checked; a name no directory holds
    /// is [`ReaderLookupError::NotFound`] with the loose attempts in its trace.
    fn resolve_loose(
        &self,
        context: &ResolveContext,
        requested: &AssetKey,
        basename: &str,
        attempts: Vec<ReaderAttempt>,
    ) -> Result<ReaderResolution, ReaderLookupError> {
        // The original opens the file its loose pass found, so the bytes are
        // read once here and digest-checked; a name no directory holds is
        // [`ReaderLookupError::NotFound`] with the loose attempts in its trace.
        let LoosePass {
            attempts: loose,
            candidate,
        } = self.loose_pass(LoosePassMode::Fallback, requested, basename)?;
        let Some(candidate) = candidate else {
            return Err(ReaderLookupError::NotFound {
                requested: Box::new(requested.clone()),
                basename: basename.to_owned(),
                trace: Box::new(reader_trace(attempts, loose)),
            });
        };
        // The fallback pass read the candidate's bytes, so the digest it
        // resolved is the digest of exactly those bytes.
        let digest = candidate
            .sha256
            .expect("the fallback pass reads a candidate's bytes, so it has their digest");
        let file = format!("{}/{}", candidate.directory, basename);
        // The directory spelling was validated when the directory was declared
        // and `basename` is one component of a validated key, so the joined
        // spelling is a valid relative one.
        let span = SourceSpan::new(
            context.installation,
            &file,
            // The file is the container itself: a loose file is not a member
            // inside another container, which is what `member_key: None` means.
            None,
            0,
            candidate.size_bytes,
            Some(digest),
        )
        .expect(
            "a validated loose directory spelling plus a validated basename is a relative path",
        );

        Ok(ReaderResolution {
            requested: requested.clone(),
            basename: basename.to_owned(),
            container: file.clone(),
            member: basename.to_owned(),
            span,
            origin: ReaderOrigin::Loose {
                directory: candidate.directory,
                file,
                host_path: candidate.host_path,
                size_bytes: candidate.size_bytes,
                sha256: digest,
            },
            trace: reader_trace(attempts, loose),
        })
    }

    /// The original's loose pass over the registered directories: most
    /// recently added first, the oldest addition (`zbd`) last.
    ///
    /// `mode` decides what a regular candidate means. In
    /// [`LoosePassMode::Fallback`] the first one is the answer the original
    /// serves when no archive held the name; in [`LoosePassMode::Override`] the
    /// original would compare it with the archive copy by file time, which is
    /// not implemented, so every regular candidate only records that the
    /// decision is undecidable ([`ReaderLooseOutcome::UndecidableShadow`]).
    ///
    /// Both passes probe every registered directory even after the answer is
    /// known, so the trace reports the whole search; only the **first**
    /// candidate is ever a candidate.
    ///
    /// In the fallback pass the candidate's bytes are read once, here, exactly
    /// as the original opens the file its loose pass found; a file that is not a
    /// regular file, cannot be read or changes under the read is refused rather
    /// than skipped, because a lower-priority directory's copy is not the
    /// original's answer.
    ///
    /// # When a probe failure refuses the lookup
    ///
    /// A directory that cannot be probed is refused with
    /// [`ReaderLookupError::LooseUnreadable`] only while **no** candidate has been
    /// found, because then the answer depends on that directory: the original
    /// searches this one before any lower-priority one, and skipping it could
    /// skip the file the original would have opened. Once a candidate exists the
    /// answer no longer depends on any later directory, so a probe failure there
    /// is **recorded** ([`ReaderLooseOutcome::Unreadable`]) and the pass
    /// continues: an I/O failure in a directory that holds nothing cannot un-decide
    /// a lookup the original decides from a file already found. The candidate's
    /// **own** bytes are still read under that rule, so a candidate that cannot be
    /// read coherently refuses rather than being reported.
    fn loose_pass(
        &self,
        mode: LoosePassMode,
        requested: &AssetKey,
        basename: &str,
    ) -> Result<LoosePass, ReaderLookupError> {
        let mut pass = LoosePass::default();
        for directory in self.loose_search_order() {
            let spelling = directory.spelling().to_owned();
            let refuse = |source: LooseFileError| ReaderLookupError::LooseUnreadable {
                requested: Box::new(requested.clone()),
                basename: basename.to_owned(),
                directory: spelling.clone(),
                source: Box::new(source),
            };
            // A **probe** failure refuses the lookup only while no candidate
            // exists, because only then does the answer still depend on this
            // directory. Once a candidate exists the answer is already decided, so
            // the failure is recorded and the pass keeps searching: an I/O error in
            // a directory the answer no longer depends on must not un-decide it.
            // The candidate's own **bytes** are a different matter: they are the
            // answer, so a candidate that cannot be read coherently refuses.
            let host_directory = directory.host_path();
            let host_path = host_directory.join(basename);
            let outcome = match probe_loose_directory(&host_directory) {
                Err(source) if pass.candidate.is_some() => ReaderLooseOutcome::Unreadable {
                    source: Box::new(source),
                },
                Err(source) => return Err(refuse(source)),
                // The original counts only existing directories, so an absent one is
                // reported as absent and never descended into.
                Ok(LooseDirectoryProbe::Absent) => ReaderLooseOutcome::Absent,
                Ok(LooseDirectoryProbe::NotADirectory) => ReaderLooseOutcome::NotADirectory,
                Ok(LooseDirectoryProbe::Directory) => match probe_loose(&host_path) {
                    Err(source) if pass.candidate.is_some() => ReaderLooseOutcome::Unreadable {
                        source: Box::new(source),
                    },
                    Err(source) => return Err(refuse(source)),
                    Ok(LooseProbe::Miss) => ReaderLooseOutcome::Miss,
                    Ok(LooseProbe::NotRegular) => ReaderLooseOutcome::NotRegular,
                    Ok(LooseProbe::Regular { size_bytes }) => match mode {
                        LoosePassMode::Fallback if pass.candidate.is_some() => {
                            ReaderLooseOutcome::Shadowed { size_bytes }
                        }
                        LoosePassMode::Fallback => {
                            let bytes = read_loose_file(&host_path).map_err(refuse)?;
                            let digest = sha256(&bytes);
                            pass.candidate = Some(LooseCandidate {
                                directory: spelling.clone(),
                                host_path: host_path.clone(),
                                size_bytes,
                                sha256: Some(digest),
                            });
                            ReaderLooseOutcome::Selected {
                                size_bytes,
                                sha256: digest,
                            }
                        }
                        LoosePassMode::Override => {
                            // The original would compare this file with the
                            // archive copy by time; production cannot, so
                            // the candidate is recorded and the lookup is
                            // refused. The file's own modification time is
                            // never read here: it is not the original's
                            // comparison argument.
                            pass.candidate.get_or_insert(LooseCandidate {
                                directory: spelling.clone(),
                                host_path: host_path.clone(),
                                size_bytes,
                                sha256: None,
                            });
                            ReaderLooseOutcome::UndecidableShadow { size_bytes }
                        }
                    },
                },
            };
            pass.attempts.push(ReaderLooseAttempt {
                directory: spelling,
                host_path,
                outcome,
            });
        }
        Ok(pass)
    }

    /// The bytes a resolution names, digest-checked, through the archive it came
    /// from — or, for a loose answer, through the host file it named.
    ///
    /// The archive is found by mount id, so a set that registered the **same**
    /// mount id twice cannot say which of the two a resolution came from: that
    /// is refused ([`ReaderReadError::AmbiguousArchive`]) instead of picking
    /// the first one and returning its bytes as another archive's.
    ///
    /// A **loose** resolution names no mount: the host path is rebuilt from the
    /// registered directory the resolution names (never from the path the
    /// resolution carries, which is only reported), and the file is re-read and
    /// checked against the digest the resolution recorded. A directory that is
    /// no longer registered is refused
    /// ([`ReaderReadError::UnknownLooseDirectory`]), so a resolution cannot be
    /// read against a path the set does not declare any more.
    ///
    /// # Errors
    ///
    /// [`ReaderReadError::UnknownArchive`] when the set no longer holds the
    /// archive the resolution names, [`ReaderReadError::AmbiguousArchive`] when
    /// it holds more than one under that id, plus everything
    /// [`ReaderArchive::read`] and [`Self::read_loose`] report.
    pub fn read(&self, resolved: &ReaderResolution) -> Result<Vec<u8>, ReaderReadError> {
        match &resolved.origin {
            ReaderOrigin::Archive { mount, .. } => {
                if self.archives_named(mount) > 1 {
                    return Err(ReaderReadError::AmbiguousArchive {
                        mount: mount.to_string(),
                    });
                }
                self.archive(mount)
                    .ok_or_else(|| ReaderReadError::UnknownArchive {
                        mount: mount.to_string(),
                    })?
                    .read(resolved)
            }
            ReaderOrigin::Loose {
                directory,
                host_path,
                size_bytes,
                sha256,
                ..
            } => self.read_loose(resolved, directory, host_path, *size_bytes, *sha256),
        }
    }

    /// The bytes of a loose resolution, re-read from the host and
    /// digest-checked against what the resolution named.
    fn read_loose(
        &self,
        resolved: &ReaderResolution,
        directory: &str,
        host_path: &Path,
        size_bytes: u64,
        recorded: ContentHash,
    ) -> Result<Vec<u8>, ReaderReadError> {
        let declared = self
            .loose
            .iter()
            .find(|entry| entry.spelling() == directory)
            .ok_or_else(|| ReaderReadError::UnknownLooseDirectory {
                directory: directory.to_owned(),
            })?;
        // The resolution must still describe this file: its container spelling
        // is the file's own installation-relative path, it has no member key
        // inside another container, its extent is the whole file, and the host
        // path it named is the one the registered directory builds today.
        let expected_container = format!("{}/{}", directory, resolved.basename);
        let expected_host_path = declared.candidate_path(&resolved.basename);
        let describes_file = resolved.container == expected_container
            && resolved.span.container_path() == expected_container
            && resolved.span.member_key().is_none()
            && resolved.span.offset() == 0
            && resolved.span.length() == size_bytes
            && resolved.span.member_sha256() == Some(recorded)
            && host_path == expected_host_path;
        if !describes_file {
            return Err(ReaderReadError::StaleResolution {
                origin: expected_container,
            });
        }
        let bytes =
            read_loose_file(&expected_host_path).map_err(|source| ReaderReadError::LooseFile {
                directory: directory.to_owned(),
                path: expected_host_path.clone(),
                source: Box::new(source),
            })?;
        let found = sha256(&bytes);
        if found != recorded {
            return Err(ReaderReadError::LooseDigestMismatch {
                path: expected_host_path,
                recorded,
                found,
            });
        }
        Ok(bytes)
    }
}

/// Which of the original's two loose rules a pass is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoosePassMode {
    /// The archive pass found the name: the original would compare a loose file
    /// of the same basename with the archive copy and keep the newer one. The
    /// comparison is not implemented, so the pass only records that a candidate
    /// exists and the lookup is refused.
    Override,
    /// No archive holds the name: the original's loose pass serves the first
    /// file it finds, and no comparison is involved.
    Fallback,
}

/// One loose reader directory's candidate: the file the original's loose pass
/// would open.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LooseCandidate {
    directory: String,
    host_path: PathBuf,
    size_bytes: u64,
    /// The digest of the bytes read for a fallback candidate. An override
    /// candidate is never read — the lookup is refused before that — so it
    /// carries no digest.
    sha256: Option<ContentHash>,
}

/// What one run of the original's loose pass saw.
#[derive(Debug, Default)]
struct LoosePass {
    /// One attempt per registered directory, in search order.
    attempts: Vec<ReaderLooseAttempt>,
    /// The first regular candidate, in search order.
    candidate: Option<LooseCandidate>,
}

/// The trace of a lookup: the archive attempts in mount order, the loose
/// attempts in the loose search order, and the status of each of the two
/// orders — which is never the designed precedence status.
fn reader_trace(attempts: Vec<ReaderAttempt>, loose: Vec<ReaderLooseAttempt>) -> ReaderTrace {
    ReaderTrace {
        attempts,
        loose,
        order_status: READER_LOOKUP_ORDER_STATUS,
        loose_order_status: READER_LOOSE_ORDER_STATUS,
    }
}

/// What a loose candidate **file** holds, without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LooseProbe {
    /// The directory holds nothing of that name.
    Miss,
    /// Something of that name exists but is not a regular file, or is a symbolic
    /// link. Links are never followed, exactly as in
    /// [`crate::vfs::mount_directory`], so no target outside the installation
    /// can be served or shadow an archive member.
    NotRegular,
    /// A regular file of that name.
    Regular {
        /// Its length in bytes.
        size_bytes: u64,
    },
}

/// What the loose reader **directory** at a declared spelling holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LooseDirectoryProbe {
    /// The directory does not exist on this host, so the original never adds it
    /// and it cannot hold anything. This is the state of every loose reader
    /// directory below `data` in the owner's installation.
    Absent,
    /// The path exists but is not a directory, or is a symbolic link, which is
    /// never followed.
    NotADirectory,
    /// A real directory, which the pass searches.
    Directory,
}

/// Whether the loose reader directory at `path` counts at all.
///
/// The original counts only existing directories (section A), so a directory
/// that is not there is [`LooseDirectoryProbe::Absent`] and never reports a
/// miss: "the original never added it" and "it was added and holds no such file"
/// are different facts, and the trace keeps them apart.
fn probe_loose_directory(path: &Path) -> Result<LooseDirectoryProbe, LooseFileError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(LooseDirectoryProbe::Absent);
        }
        // A component above this path is not a directory, so this path cannot be
        // one either. That is a statement about the host's shape, like
        // [`LooseDirectoryProbe::NotADirectory`] and not an I/O failure: reporting
        // it as one would refuse a lookup over a spelling the host can never
        // satisfy, and would report a readable file elsewhere as unusable.
        Err(source) if source.kind() == io::ErrorKind::NotADirectory => {
            return Ok(LooseDirectoryProbe::NotADirectory);
        }
        Err(source) => return Err(LooseFileError::io(path, &source)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(LooseDirectoryProbe::NotADirectory);
    }
    Ok(LooseDirectoryProbe::Directory)
}

/// What a loose candidate file holds on this host, inside a directory that
/// exists.
///
/// A `NotFound` is [`LooseProbe::Miss`] — the directory is there and holds no
/// such file — while every other failure is reported rather than read as an
/// absence, because "there is no such file" and "the file could not be asked
/// about" are different facts.
fn probe_loose(path: &Path) -> Result<LooseProbe, LooseFileError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(LooseProbe::Miss),
        Err(source) => return Err(LooseFileError::io(path, &source)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(LooseProbe::NotRegular);
    }
    Ok(LooseProbe::Regular {
        size_bytes: metadata.len(),
    })
}

/// Reads one loose reader file whole, refusing a symbolic link, a non-regular
/// file and a file that changes while it is read.
///
/// The link check is repeated after the open by comparing the file identity the
/// probe saw with the one the handle reports, so a link swapped in between the
/// two cannot get its target served.
fn read_loose_file(path: &Path) -> Result<Vec<u8>, LooseFileError> {
    let walked = fs::symlink_metadata(path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            LooseFileError::NotRegular {
                path: path.to_path_buf(),
            }
        } else {
            LooseFileError::io(path, &source)
        }
    })?;
    if walked.file_type().is_symlink() || !walked.is_file() {
        return Err(LooseFileError::NotRegular {
            path: path.to_path_buf(),
        });
    }
    let mut file = fs::File::open(path).map_err(|source| LooseFileError::io(path, &source))?;
    let opened = file
        .metadata()
        .map_err(|source| LooseFileError::io(path, &source))?;
    if !opened.is_file() || !same_file(&walked, &opened) {
        return Err(LooseFileError::NotRegular {
            path: path.to_path_buf(),
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)
        .map_err(|source| LooseFileError::io(path, &source))?;
    let after = file
        .metadata()
        .map_err(|source| LooseFileError::io(path, &source))?;
    let before_modified = opened
        .modified()
        .map_err(|source| LooseFileError::io(path, &source))?;
    let after_modified = after
        .modified()
        .map_err(|source| LooseFileError::io(path, &source))?;
    if opened.len() != bytes.len() as u64
        || after.len() != bytes.len() as u64
        || before_modified != after_modified
    {
        return Err(LooseFileError::Changed {
            path: path.to_path_buf(),
        });
    }
    Ok(bytes)
}

/// Whether two metadata records describe the same host file, so a link swapped
/// in between the probe and the open is refused.
#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// Whether two metadata records describe the same host file. Without inode
/// numbers the length is the only check available.
#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
}

/// What one archive did for a reader lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderAttemptOutcome {
    /// The archive served the lookup: it is the first mounted archive that
    /// held the name, and `entry_index` is the entry its first-hit scan
    /// returned.
    Selected {
        /// The entry index inside that archive.
        entry_index: usize,
    },
    /// The archive holds the name, but a higher-mounted archive was searched
    /// first and served it — the fact the original's mount order decides.
    Shadowed {
        /// The entry index inside that archive.
        entry_index: usize,
    },
    /// The archive is mounted for this context but holds no member of this
    /// name.
    Miss,
    /// The archive is bound to a world, mission, locale or mod this context
    /// does not select, so the original would not have mounted it here either.
    Skipped(SkipReason),
}

impl ReaderAttemptOutcome {
    /// The stable label used in traces and reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Selected { .. } => "selected",
            Self::Shadowed { .. } => "shadowed",
            Self::Miss => "miss",
            Self::Skipped(reason) => reason.label(),
        }
    }
}

impl fmt::Display for ReaderAttemptOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selected { entry_index } => write!(f, "selected(#{entry_index})"),
            Self::Shadowed { entry_index } => write!(f, "shadowed(#{entry_index})"),
            Self::Miss => f.write_str("miss"),
            Self::Skipped(reason) => write!(f, "skipped({reason})"),
        }
    }
}

/// One archive as a reader lookup consulted it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderAttempt {
    /// The archive's mount id.
    pub mount: MountId,
    /// The archive's installation spelling.
    pub container: String,
    /// Which level of the mount list it occupies.
    pub level: ReaderLevel,
    /// What it did for the lookup.
    pub outcome: ReaderAttemptOutcome,
}

impl fmt::Display for ReaderAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, {}) [{}]",
            self.mount, self.container, self.level, self.outcome
        )
    }
}

/// What one loose reader directory held of a requested basename.
///
/// Every variant is a fact about this host, never about the original's
/// decision: the original's `CompareFileTime` comparison is not implemented
/// ([`READER_LOOSE_OVERRIDE_STATUS`]), so a directory that holds the name while
/// an archive also holds it reports [`Self::UndecidableShadow`] and the lookup
/// is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderLooseOutcome {
    /// The directory does not exist on this host. The original counts only
    /// existing directories, so this one cannot hold anything — which is the
    /// state of every loose reader directory in the owner's installation.
    Absent,
    /// The directory exists and holds no file of this basename.
    Miss,
    /// The **declared directory itself** is not a directory on this host, or is a
    /// symbolic link, so it was never searched. This is a different fact from
    /// [`Self::NotRegular`], which is about the entry of the requested basename
    /// *inside* a directory that does exist: conflating them would report a
    /// readable file as unusable because its parent is not a directory.
    NotADirectory,
    /// The directory or the entry could not be probed because of an I/O failure.
    ///
    /// This is **recorded, not fatal**, once the pass holds a candidate: the answer
    /// no longer depends on this directory, so a failure to inspect it cannot
    /// change it. Before any candidate exists the same failure refuses the lookup
    /// ([`ReaderLookupError::LooseUnreadable`]), because the answer would then
    /// depend on a directory this engine could not look at.
    Unreadable {
        /// What the operating system refused with, and on which path.
        source: Box<LooseFileError>,
    },
    /// An entry of this name exists but is not a regular file, or is a symbolic
    /// link. This engine never follows a link out of an installation and never
    /// serves a directory (the same guard as [`crate::vfs::mount_directory`]),
    /// so such an entry is reported and can neither serve a lookup nor shadow an
    /// archive member. **This is this engine's guard, not a rule measured in
    /// the original**: a linked loose file would be opened and compared there.
    NotRegular,
    /// The directory holds the file and **no** archive did, so the loose pass
    /// served it: the original's fallback, which needs no time comparison.
    Selected {
        /// The file's length in bytes.
        size_bytes: u64,
        /// SHA-256 of exactly those bytes.
        sha256: ContentHash,
    },
    /// The directory holds the file, and so does an archive the original would
    /// have served first. Which copy the original keeps is the unmodelled
    /// `CompareFileTime` comparison, so the lookup is **refused**
    /// ([`ReaderLookupError::LooseOverrideUndecided`]) instead of picking one.
    UndecidableShadow {
        /// The loose file's length in bytes.
        size_bytes: u64,
    },
    /// The directory holds the file, but a directory searched earlier already
    /// served it.
    Shadowed {
        /// The loose file's length in bytes.
        size_bytes: u64,
    },
}

impl ReaderLooseOutcome {
    /// The stable label used in traces and reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Miss => "miss",
            Self::NotADirectory => "not_a_directory",
            Self::Unreadable { .. } => "unreadable",
            Self::NotRegular => "not_regular",
            Self::Selected { .. } => "selected",
            Self::UndecidableShadow { .. } => "undecidable_shadow",
            Self::Shadowed { .. } => "shadowed",
        }
    }
}

impl fmt::Display for ReaderLooseOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => f.write_str("absent"),
            Self::Miss => f.write_str("miss"),
            Self::NotADirectory => f.write_str("not_a_directory"),
            Self::Unreadable { source } => write!(f, "unreadable({source})"),
            Self::NotRegular => f.write_str("not_regular"),
            Self::Selected { size_bytes, .. } => write!(f, "selected({size_bytes} B)"),
            Self::UndecidableShadow { size_bytes } => {
                write!(f, "undecidable_shadow({size_bytes} B)")
            }
            Self::Shadowed { size_bytes } => write!(f, "shadowed({size_bytes} B)"),
        }
    }
}

/// One loose reader directory as a reader lookup consulted it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderLooseAttempt {
    /// The directory's installation-relative spelling.
    pub directory: String,
    /// The host path the directory lives in.
    pub host_path: PathBuf,
    /// What it held of the requested basename.
    pub outcome: ReaderLooseOutcome,
}

impl fmt::Display for ReaderLooseAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) [{}]",
            self.directory,
            self.host_path.display(),
            self.outcome
        )
    }
}

/// Why one loose reader file could not be handed out.
///
/// The guards mirror [`crate::vfs::source`]'s read path: a symbolic link is
/// never followed, a non-regular file is never served, and a file that changes
/// while it is read has no coherent bytes to serve.
///
/// An `io::Error` is neither [`Clone`] nor [`PartialEq`], and the lookup error
/// family it travels in is both, so the OS error is kept as its
/// [`io::ErrorKind`] plus its message instead of as the error itself. That is
/// why this type does not implement
/// [`Error::source`](std::error::Error::source): there is no error left to
/// chain to, and the text the OS gave is what a diagnostic prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LooseFileError {
    /// The entry is a symbolic link or not a regular file. Links are never
    /// followed: a target could lie outside the installation.
    NotRegular {
        /// The entry that was refused.
        path: PathBuf,
    },
    /// The file could not be opened, read or asked about.
    Io {
        /// The file.
        path: PathBuf,
        /// What the operating system refused with.
        kind: io::ErrorKind,
        /// The message the operating system gave.
        message: String,
    },
    /// The file's length or modification time moved during the read, so no
    /// coherent digest exists.
    Changed {
        /// The file.
        path: PathBuf,
    },
}

impl LooseFileError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotRegular { .. } => "not_regular",
            Self::Io { .. } => "io",
            Self::Changed { .. } => "changed",
        }
    }

    /// The file a refusal names.
    pub fn path(&self) -> &Path {
        match self {
            Self::NotRegular { path } | Self::Io { path, .. } | Self::Changed { path } => path,
        }
    }

    /// Builds an I/O refusal from an [`io::Error`], keeping its kind and text.
    fn io(path: &Path, source: &io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            kind: source.kind(),
            message: source.to_string(),
        }
    }
}

impl fmt::Display for LooseFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegular { path } => write!(
                f,
                "{} is a symbolic link or not a regular file; a loose reader file is only ever a \
                 regular file and a link is never followed",
                path.display()
            ),
            Self::Io { path, message, .. } => {
                write!(
                    f,
                    "cannot read the loose reader file {}: {message}",
                    path.display()
                )
            }
            Self::Changed { path } => write!(
                f,
                "the loose reader file {} changed while it was read; refusing the torn bytes",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LooseFileError {}

/// Everything a reader lookup saw: every mounted archive in the original's
/// search order, every loose reader directory the caller registered in the
/// original's loose search order, what each did, and how well each of the two
/// orders is known.
///
/// The archive order is [`READER_LOOKUP_ORDER_STATUS`] and the loose-directory
/// order is [`READER_LOOSE_ORDER_STATUS`], **not**
/// [`PRECEDENCE_ORDER_STATUS`]: a reader lookup never consults the designed
/// precedence order, so reporting that status here would credit this trace
/// with an order it did not use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderTrace {
    /// The archives consulted, in the original's mount order.
    pub attempts: Vec<ReaderAttempt>,
    /// The loose reader directories consulted, in the original's search order
    /// (most recently added first, `zbd` last). Empty when the caller
    /// registered none, which is every retail installation: none of the
    /// original's loose reader directories exists there.
    pub loose: Vec<ReaderLooseAttempt>,
    /// Evidence status of the archive order.
    pub order_status: ClaimStatus,
    /// Evidence status of the loose-directory order, which is a separate rule
    /// with separate evidence from the archive order.
    pub loose_order_status: ClaimStatus,
}

impl fmt::Display for ReaderTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = self
            .attempts
            .iter()
            .map(ReaderAttempt::to_string)
            .chain(
                self.loose
                    .iter()
                    .map(|attempt| format!("loose {}", ReaderLooseAttempt::to_string(attempt))),
            )
            .collect();
        write!(
            f,
            "{} [{}, loose order {}]",
            rendered.join("; "),
            self.order_status.label(),
            self.loose_order_status.label()
        )
    }
}

/// What served a reader lookup.
///
/// A reader answer is either a member of a mounted archive or a **loose** host
/// file found by the original's loose pass. Keeping the two apart is what makes
/// a loose file a visible collision class instead of a second spelling of an
/// archive member: `ReaderOrigin::Loose` names the file, and a lookup where
/// both exist is refused rather than decided (see the module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderOrigin {
    /// A member of a mounted reader archive.
    Archive {
        /// The archive's mount id.
        mount: MountId,
        /// Which level of the original's mount list served it.
        level: ReaderLevel,
        /// The entry index its first-hit scan returned.
        entry_index: usize,
    },
    /// A loose host file, served by the original's loose directory pass
    /// **only** because no mounted archive held the name.
    Loose {
        /// The loose directory's installation-relative spelling.
        directory: String,
        /// The file's installation-relative path: `<directory>/<basename>`.
        file: String,
        /// The host path the file was read from.
        host_path: PathBuf,
        /// Its length in bytes.
        size_bytes: u64,
        /// SHA-256 of exactly those bytes.
        sha256: ContentHash,
    },
}

impl ReaderOrigin {
    /// The archive's mount id, or `None` when a loose file served the lookup.
    pub fn mount(&self) -> Option<&MountId> {
        match self {
            Self::Archive { mount, .. } => Some(mount),
            Self::Loose { .. } => None,
        }
    }

    /// Which level of the original's mount list served the lookup, or `None`
    /// when a loose file did — a loose file is not one of the three archive
    /// levels, and [`ReaderLevel`] is not widened to pretend otherwise.
    pub const fn level(&self) -> Option<ReaderLevel> {
        match self {
            Self::Archive { level, .. } => Some(*level),
            Self::Loose { .. } => None,
        }
    }

    /// The archive entry index, or `None` when a loose file served the lookup.
    pub const fn entry_index(&self) -> Option<usize> {
        match self {
            Self::Archive { entry_index, .. } => Some(*entry_index),
            Self::Loose { .. } => None,
        }
    }

    /// Whether a mounted archive member served the lookup.
    pub const fn is_archive(&self) -> bool {
        matches!(self, Self::Archive { .. })
    }

    /// Whether a loose host file served the lookup.
    pub const fn is_loose(&self) -> bool {
        matches!(self, Self::Loose { .. })
    }
}

/// One reader member a lookup resolved: its exact origin plus the trace that
/// justified it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderResolution {
    /// The key the caller asked for, as it spelled it.
    pub requested: AssetKey,
    /// The basename the lookup actually searched for.
    pub basename: String,
    /// The container the served bytes live in: the archive's installation
    /// spelling, or the loose file's own installation-relative path.
    pub container: String,
    /// The member's spelling, exactly as the archive declares it, or the loose
    /// file's own name.
    pub member: String,
    /// The immutable origin of the member's bytes.
    pub span: SourceSpan,
    /// What served the lookup: an archive member or a loose file.
    pub origin: ReaderOrigin,
    /// The ordered attempts that produced this answer.
    pub trace: ReaderTrace,
}

impl ReaderResolution {
    /// Which level of the original's mount list served it, or `None` when a
    /// loose file did.
    pub fn level(&self) -> Option<ReaderLevel> {
        self.origin.level()
    }

    /// The archive's mount id, or `None` when a loose file served it.
    pub fn mount(&self) -> Option<&MountId> {
        self.origin.mount()
    }

    /// The archive entry index, or `None` when a loose file served it.
    pub fn entry_index(&self) -> Option<usize> {
        self.origin.entry_index()
    }
}

/// Why a reader lookup produced no member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderLookupError {
    /// The key is not a reader key: a reader member is reached through
    /// [`ReaderMounts::resolve`], and this key names another key space.
    ForeignNamespace {
        /// The namespace the key was asked for.
        namespace: MountNamespace,
    },
    /// The request specializes a variant. A reader archive declares one copy of
    /// each name and indexes it at [`AssetVariant::default`], so no mounted
    /// reader archive can serve another variant; the request is refused rather
    /// than answered with the default copy.
    ForeignVariant {
        /// The key that was asked for.
        requested: Box<AssetKey>,
        /// The variant it asked for.
        variant: String,
    },
    /// The requested path has no basename to search for.
    EmptyBasename {
        /// The key that was asked for.
        requested: Box<AssetKey>,
    },
    /// The reduced basename cannot be spelled as a key. Unreachable for a key
    /// built by [`AssetKey::from_spelling`], whose logical key comes from a
    /// validated [`RelativePath`]; kept so the reduction is total rather than
    /// panicking, and carrying the real refusal rather than a substituted one.
    UnspellableBasename {
        /// The key that was asked for.
        requested: Box<AssetKey>,
        /// The basename the request reduced to.
        basename: String,
        /// The refusal the key construction itself reported.
        error: AssetKeyError,
    },
    /// A mounted archive holds the name **and** a loose file of the same
    /// basename exists, so the original's `CompareFileTime` override decides
    /// which copy answers — and it is not implemented
    /// ([`READER_LOOSE_OVERRIDE_STATUS`]). The lookup is refused rather than
    /// answered with either copy, because answering would mean comparing
    /// against the index entry's trailing `u64`, whose meaning is unknown
    /// (task #692). The refusal is raised whatever the loose file's own
    /// modification time says: the host's timestamp is not the original's
    /// comparison argument.
    LooseOverrideUndecided {
        /// The key that was asked for.
        requested: Box<AssetKey>,
        /// The basename the lookup searched for.
        basename: String,
        /// The loose directory's installation-relative spelling.
        directory: String,
        /// The loose file's host path.
        host_path: Box<Path>,
        /// The loose file's length in bytes.
        size_bytes: u64,
        /// The archive that would otherwise have served the name.
        archive_container: String,
        /// Everything the lookup saw, including the loose pass.
        trace: Box<ReaderTrace>,
    },
    /// The loose pass found a file the lookup would serve, but the file could
    /// not be handed out coherently (a link, a non-regular entry, an I/O
    /// failure or a file that changed under the read). It is refused rather
    /// than skipped, because the original opens the first file the loose pass
    /// finds: a lower-priority directory's copy is not the original's answer.
    LooseUnreadable {
        /// The key that was asked for.
        requested: Box<AssetKey>,
        /// The basename the lookup searched for.
        basename: String,
        /// The loose directory's installation-relative spelling.
        directory: String,
        /// Why the file could not be handed out.
        source: Box<LooseFileError>,
    },
    /// No mounted archive the context admits holds the name, and no loose
    /// reader directory holds it either.
    NotFound {
        /// The key that was asked for.
        requested: Box<AssetKey>,
        /// The basename the lookup searched for.
        basename: String,
        /// The archives and loose directories it searched, in order.
        trace: Box<ReaderTrace>,
    },
}

impl fmt::Display for ReaderLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignNamespace { namespace } => write!(
                f,
                "{namespace} is not the reader key space {READER_NAMESPACE}; a reader member is \
                 looked up through ReaderMounts::resolve with a reader key"
            ),
            Self::ForeignVariant { requested, variant } => write!(
                f,
                "{requested} asks for variant {variant:?}; a reader archive declares one copy of \
                 each name at the default variant and cannot answer another one"
            ),
            Self::EmptyBasename { requested } => {
                write!(
                    f,
                    "{requested} has no basename a reader lookup could search for"
                )
            }
            Self::UnspellableBasename {
                requested,
                basename,
                error,
            } => write!(
                f,
                "{requested} reduces to {basename:?}, which cannot be spelled as a key: {error}"
            ),
            Self::LooseOverrideUndecided {
                requested,
                basename,
                directory,
                host_path,
                size_bytes,
                archive_container,
                trace,
            } => write!(
                f,
                "{requested} ({basename:?}) could be answered by the archive member of \
                 {archive_container} or by the loose file {} ({size_bytes} B) under {directory}, \
                 and the original decides between them with CompareFileTime >= 1 against a \
                 timestamp this engine does not know; refusing instead of guessing. The loose \
                 file's own modification time decides nothing here. Attempts: {trace}",
                host_path.display()
            ),
            Self::LooseUnreadable {
                requested,
                basename,
                directory,
                source,
            } => write!(
                f,
                "{requested} ({basename:?}) cannot be answered: the loose file the original's \
                 loose pass would serve under {directory} cannot be handed out: {source}"
            ),
            Self::NotFound {
                requested,
                basename,
                trace,
            } => write!(
                f,
                "no mounted reader archive and no loose reader directory hold {basename:?} for \
                 {requested}; attempts: {trace}"
            ),
        }
    }
}

impl std::error::Error for ReaderLookupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnspellableBasename { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Why a reader archive could not be mounted.
///
/// Every variant is raised **before** a [`ReaderArchive`] exists, so a refused
/// archive leaves the caller's mount list exactly as it was (spec F04-C's
/// mount-failure contract).
///
/// The payload-carrying variants are boxed so the error itself stays small on
/// the `Result` path; match on [`Self::code`] for a stable identifier,
/// [`Self::container`] for provenance and [`Self::path`] /
/// [`Self::namespace`] for the values a refusal names.
#[derive(Debug)]
pub enum ReaderMountError {
    /// The archive file could not be read at all.
    UnreadableContainer {
        /// The mount's container label (the provenance the caller gave).
        container: Box<str>,
        /// The host path that was read.
        path: Box<Path>,
        /// Why the read failed.
        source: io::Error,
    },
    /// The container label is not a valid relative spelling, so it cannot be
    /// dispatched or recorded in a span.
    ContainerPath {
        /// The mount's container label.
        container: Box<str>,
        /// Which path rule refused it.
        reason: RelativePathError,
    },
    /// The mount is not in the [`READER_NAMESPACE`] key space, so a reader
    /// lookup could never reach its members.
    ForeignNamespace {
        /// The mount's container label.
        container: Box<str>,
        /// The namespace it was mounted in.
        namespace: Box<MountNamespace>,
    },
    /// The container could not be routed to the reader family.
    Dispatch {
        /// The mount's container label.
        container: Box<str>,
        /// The dispatcher's refusal.
        source: Box<ZbdDispatchError>,
    },
    /// The container's own version-one member index did not read.
    Index {
        /// The mount's container label.
        container: Box<str>,
        /// The reader's refusal.
        source: Box<IndexError>,
    },
    /// The reader-family listing refused the container.
    Reader {
        /// The mount's container label.
        container: Box<str>,
        /// The reader's refusal.
        source: Box<ReaderError>,
    },
    /// A member the archive declares was refused by the mount index.
    Member {
        /// The mount's container label.
        container: Box<str>,
        /// The refusal.
        source: Box<MountError>,
    },
    /// The mount as a whole was refused.
    Mount(Box<MountError>),
}

impl ReaderMountError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnreadableContainer { .. } => "unreadable_container",
            Self::ContainerPath { .. } => "container_path",
            Self::ForeignNamespace { .. } => "foreign_namespace",
            Self::Dispatch { source, .. } => source.code(),
            Self::Index { source, .. } => source.code(),
            Self::Reader { source, .. } => source.code(),
            Self::Member { .. } => "member",
            Self::Mount(_) => "mount",
        }
    }

    /// The mount's container label.
    pub fn container(&self) -> &str {
        match self {
            Self::UnreadableContainer { container, .. }
            | Self::ContainerPath { container, .. }
            | Self::ForeignNamespace { container, .. }
            | Self::Dispatch { container, .. }
            | Self::Index { container, .. }
            | Self::Reader { container, .. }
            | Self::Member { container, .. } => container,
            Self::Mount(error) => match error.as_ref() {
                MountError::ModClassWithoutBinding { id }
                | MountError::BindingWithoutModClass { id, .. }
                | MountError::DuplicateMountId { id } => id.as_str(),
                _ => "",
            },
        }
    }

    /// The host path a refusal names, when the refusal is about reading one.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::UnreadableContainer { path, .. } => Some(path.as_ref()),
            _ => None,
        }
    }

    /// The namespace a refusal names, when the mount is in the wrong key
    /// space.
    pub fn namespace(&self) -> Option<&MountNamespace> {
        match self {
            Self::ForeignNamespace { namespace, .. } => Some(namespace.as_ref()),
            _ => None,
        }
    }

    /// The underlying refusal's own stable code, so a caller can report the
    /// specific reason behind [`Self::Member`] and [`Self::Mount`] too.
    pub fn source_code(&self) -> &'static str {
        match self {
            Self::Member { source, .. } => source.code(),
            Self::Mount(source) => source.code(),
            other => other.code(),
        }
    }
}

impl fmt::Display for ReaderMountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnreadableContainer {
                container,
                path,
                source,
            } => write!(
                f,
                "cannot read the reader archive {path} ({container}): {source}",
                path = path.display()
            ),
            Self::ContainerPath { container, reason } => {
                write!(
                    f,
                    "the reader archive container {container:?} is refused: {reason}"
                )
            }
            Self::ForeignNamespace {
                container,
                namespace,
            } => write!(
                f,
                "the reader archive {container} is mounted in namespace {namespace}; a reader \
                 lookup only serves the {READER_NAMESPACE} key space"
            ),
            Self::Dispatch { source, .. } => write!(f, "{source}"),
            Self::Index { source, .. } => write!(f, "{source}"),
            Self::Reader { source, .. } => write!(f, "{source}"),
            Self::Member { container, source } => write!(f, "in {container}: {source}"),
            Self::Mount(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for ReaderMountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnreadableContainer { source, .. } => Some(source),
            Self::ContainerPath { reason, .. } => Some(reason),
            Self::Dispatch { source, .. } => Some(source),
            Self::Index { source, .. } => Some(source),
            Self::Reader { source, .. } => Some(source),
            Self::Member { source, .. } | Self::Mount(source) => Some(source),
            Self::ForeignNamespace { .. } => None,
        }
    }
}

/// Why a reader member's bytes could not be read.
#[derive(Debug)]
pub enum ReaderReadError {
    /// The resolution names a loose file, not a member of this archive.
    LooseOrigin {
        /// The archive that was asked to read it.
        archive: String,
    },
    /// The loose resolution names a directory the mount set no longer holds,
    /// so its host path cannot be rebuilt.
    UnknownLooseDirectory {
        /// The directory spelling the resolution names.
        directory: String,
    },
    /// The resolution names an archive other than the one asked to read it.
    ForeignArchive {
        /// The mount the resolution names.
        mount: String,
        /// The mount of the archive asked to read.
        expected: String,
    },
    /// The mount set no longer holds the archive the resolution names.
    UnknownArchive {
        /// The mount the resolution names.
        mount: String,
    },
    /// The mount set holds more than one archive under the id the resolution
    /// names, so the bytes it points at cannot be attributed to one archive.
    AmbiguousArchive {
        /// The mount the resolution names.
        mount: String,
    },
    /// The resolution does not describe the entry or file it was made for any
    /// more.
    StaleResolution {
        /// The mount id, or the loose file's installation-relative path.
        origin: String,
    },
    /// A loose reader file could not be read coherently: a link, a non-regular
    /// entry, an I/O failure or a file that changed under the read.
    LooseFile {
        /// The loose directory's installation-relative spelling.
        directory: String,
        /// The file that was refused.
        path: PathBuf,
        /// Why it was refused.
        source: Box<LooseFileError>,
    },
    /// The entry's declared extent does not lie inside the container bytes the
    /// archive was built from.
    OutOfBounds {
        /// The entry index.
        entry_index: usize,
        /// The declared offset.
        offset: u64,
        /// The declared length.
        length: u64,
        /// The archive's length.
        archive_len: u64,
    },
    /// The stored bytes do not hash to what the mount recorded.
    DigestMismatch {
        /// The member's spelling.
        member: String,
        /// The entry index.
        entry_index: usize,
        /// The digest recorded when the archive was mounted.
        mounted: ContentHash,
        /// The digest of the bytes that were read.
        found: ContentHash,
    },
    /// The loose file's bytes do not hash to what the resolution recorded.
    LooseDigestMismatch {
        /// The loose file.
        path: PathBuf,
        /// The digest recorded when the lookup resolved it.
        recorded: ContentHash,
        /// The digest of the bytes that were read.
        found: ContentHash,
    },
}

impl ReaderReadError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::LooseOrigin { .. } => "loose_origin",
            Self::UnknownLooseDirectory { .. } => "unknown_loose_directory",
            Self::LooseFile { .. } => "loose_file",
            Self::ForeignArchive { .. } => "foreign_archive",
            Self::UnknownArchive { .. } => "unknown_archive",
            Self::AmbiguousArchive { .. } => "ambiguous_archive",
            Self::StaleResolution { .. } => "stale_resolution",
            Self::OutOfBounds { .. } => "out_of_bounds",
            Self::DigestMismatch { .. } => "digest_mismatch",
            Self::LooseDigestMismatch { .. } => "loose_digest_mismatch",
        }
    }
}

impl fmt::Display for ReaderReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LooseOrigin { archive } => write!(
                f,
                "the resolution names a loose host file, not a member of the reader archive \
                 {archive}"
            ),
            Self::UnknownLooseDirectory { directory } => write!(
                f,
                "no registered loose reader directory answers to {directory}, so the host path of \
                 the file a resolution names cannot be rebuilt"
            ),
            Self::ForeignArchive { mount, expected } => write!(
                f,
                "the resolution names archive {mount}, not the archive {expected} it was read from"
            ),
            Self::UnknownArchive { mount } => {
                write!(f, "no mounted reader archive answers to {mount}")
            }
            Self::AmbiguousArchive { mount } => write!(
                f,
                "{mount} names more than one mounted reader archive, so the bytes a resolution \
                 points at cannot be attributed to one of them"
            ),
            Self::StaleResolution { origin } => write!(
                f,
                "the resolution of {origin} no longer describes the entry or file it was made for"
            ),
            Self::LooseFile {
                directory,
                path,
                source,
            } => write!(
                f,
                "the loose reader file {} under {directory} cannot be read: {source}",
                path.display()
            ),
            Self::OutOfBounds {
                entry_index,
                offset,
                length,
                archive_len,
            } => write!(
                f,
                "entry {entry_index} declares {offset}+{length}, outside the {archive_len}-byte \
                 reader archive"
            ),
            Self::DigestMismatch {
                member,
                entry_index,
                mounted,
                found,
            } => write!(
                f,
                "entry {entry_index} ({member:?}) hashed to {found}, not the {mounted} it was \
                 mounted with"
            ),
            Self::LooseDigestMismatch {
                path,
                recorded,
                found,
            } => write!(
                f,
                "the loose reader file {} hashed to {found}, not the {recorded} the resolution \
                 named",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ReaderReadError {}

/// Mounts the reader archive at `container_path` as `builder` describes it,
/// at `level` of the original's mount list.
///
/// The archive is read once and dispatched through the production reader chain
/// (`dispatch` → [`read_version_one_index`] → [`read_reader_archive`]), so the
/// index, the extents and the family gate are the ones `cs_formats` reads for
/// every other ZBD container. Every declared entry becomes a
/// [`ReaderMember`] row; an entry a lookup can reach by name also becomes a
/// mount member, spelled with the name the archive declares, at its declared
/// offset and length, with the SHA-256 of exactly those bytes.
///
/// `builder` must mount in [`READER_NAMESPACE`]: a reader lookup only ever
/// searches that key space, so a mount elsewhere could never be served (and is
/// refused rather than silently unreachable). The precedence class the builder
/// declares describes the mount to the **designed** precedence machinery; this
/// lookup does not rank by it, because the original's reader order is mount
/// order and puts the root archive first.
///
/// **Nothing is written anywhere, and a refusal leaves no trace**: every error
/// above is returned before the [`ReaderArchive`] exists.
///
/// # Errors
///
/// [`ReaderMountError`] when the archive cannot be read, does not dispatch to
/// the reader family, has no readable version-one index, is refused by the
/// family gate or listing, or when its mount is refused.
///
/// # Which refusals retail exercises
///
/// All 62 `zrdr.zbd` of the owner's installation declare 1293 entries and
/// **every one** passes the dispatch, the index read, the bounds check and the
/// spelling check, so no retail archive reaches a [`ReaderMountError`] at all.
/// Those refusals are guards exercised by synthetic archives; they exist
/// because a container that cannot be read must be reportable, not because the
/// original data shows one failing.
pub fn mount_reader_archive(
    mut builder: MountBuilder,
    container_path: &Path,
    level: ReaderLevel,
) -> Result<ReaderArchive, ReaderMountError> {
    let container = builder.container().to_owned();
    if builder.namespace().as_str() != READER_NAMESPACE {
        return Err(ReaderMountError::ForeignNamespace {
            container: container.into_boxed_str(),
            namespace: Box::new(builder.namespace().clone()),
        });
    }
    let bytes =
        fs::read(container_path).map_err(|source| ReaderMountError::UnreadableContainer {
            container: container.clone().into_boxed_str(),
            path: container_path.to_path_buf().into_boxed_path(),
            source,
        })?;
    let spelling =
        RelativePath::new(&container).map_err(|reason| ReaderMountError::ContainerPath {
            container: container.clone().into_boxed_str(),
            reason,
        })?;

    let (members, mount) = {
        let mut context = ParseContext::with_defaults(&container);
        let decision =
            dispatch(ZbdProbe::new(&container, &spelling, &bytes)).map_err(|source| {
                ReaderMountError::Dispatch {
                    container: container.clone().into_boxed_str(),
                    source: Box::new(source),
                }
            })?;
        let index = read_version_one_index(&mut context, decision, &bytes).map_err(|source| {
            ReaderMountError::Index {
                container: container.clone().into_boxed_str(),
                source: Box::new(source),
            }
        })?;
        let table = index.member_table();
        let archive =
            read_reader_archive(&mut context, &table, index.data()).map_err(|source| {
                ReaderMountError::Reader {
                    container: container.clone().into_boxed_str(),
                    source: Box::new(source),
                }
            })?;

        let listing = archive.listing();
        let mut members: Vec<ReaderMember> = Vec::with_capacity(listing.len());
        // The first entry index per case-folded name: the original's scan
        // serves the first entry, so this is what "already answered by an
        // earlier entry" means, and it is what keeps a second copy of one name
        // out of the mount index.
        let mut served: BTreeMap<String, usize> = BTreeMap::new();
        for row in listing.rows() {
            let entry_index = row.index();
            let offset = row.span().offset;
            let length = row.span().length;
            let content = listing.member_bytes(entry_index);
            let digest = content.map(sha256);
            let name = std::str::from_utf8(row.name()).ok().map(str::to_owned);

            // The three refusals are ordered by how much they depend on: the
            // listing's bounds check first (an entry with no bytes has no
            // name to serve), then the spelling, then the first-hit rule.
            let spellable = match name.as_deref() {
                None => Err(Unreachable::NonUtf8Name),
                Some(declared) => RelativePath::new(declared)
                    .map(|_| ())
                    .map_err(|_| Unreachable::InvalidName),
            };
            let unreachable = if content.is_none() {
                Some(Unreachable::FailedBounds)
            } else {
                match spellable {
                    Err(reason) => Some(reason),
                    Ok(()) => {
                        let folded = name
                            .as_deref()
                            .expect("a spellable entry has a name")
                            .to_ascii_lowercase();
                        // The original's scan serves the **first** entry whose
                        // name matches case-insensitively, so a later entry of
                        // one name is unreachable by name and stays a row.
                        let first = served.entry(folded).or_insert(entry_index);
                        (*first != entry_index)
                            .then_some(Unreachable::DuplicateName { served_by: *first })
                    }
                }
            };
            if unreachable.is_none() {
                builder
                    .add_member(
                        name.as_deref().expect("a reachable entry has a name"),
                        length,
                        offset,
                        digest,
                    )
                    .map_err(|source| ReaderMountError::Member {
                        container: container.clone().into_boxed_str(),
                        source: Box::new(source),
                    })?;
            }
            members.push(ReaderMember {
                entry_index,
                name,
                offset,
                length,
                sha256: digest,
                unreachable,
            });
        }
        let mount = builder
            .build()
            .map_err(|source| ReaderMountError::Mount(Box::new(source)))?;
        (members, mount)
    };

    Ok(ReaderArchive {
        mount,
        level,
        container,
        path: container_path.to_path_buf(),
        bytes,
        members,
    })
}
