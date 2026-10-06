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
//! Those three rules are what this module implements, and nothing else: the
//! loose-file override, the loose directory fallback and the meaning of the
//! index entry's trailing timestamp stay unmeasured (sections A and F) and are
//! not modelled here.
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
//! Nothing here writes to the installation.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
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
    /// [`ReaderReadError`] when the resolution names another archive
    /// ([`ReaderReadError::ForeignArchive`]), no longer describes the entry it
    /// was made for ([`ReaderReadError::StaleResolution`]), or the stored
    /// bytes do not hash to what the mount recorded
    /// ([`ReaderReadError::DigestMismatch`]).
    pub fn read(&self, resolved: &ReaderResolution) -> Result<Vec<u8>, ReaderReadError> {
        if self.mount.id() != &resolved.mount {
            return Err(ReaderReadError::ForeignArchive {
                mount: resolved.mount.to_string(),
                expected: self.mount.id().to_string(),
            });
        }
        let member = self.member_at(resolved.entry_index).ok_or_else(|| {
            ReaderReadError::StaleResolution {
                mount: resolved.mount.to_string(),
            }
        })?;
        let span = &resolved.span;
        let describes_entry = span.container_path() == self.container
            && span.member_key() == member.name.as_deref()
            && span.offset() == member.offset
            && span.length() == member.length
            && span.member_sha256() == member.sha256;
        if !describes_entry {
            return Err(ReaderReadError::StaleResolution {
                mount: resolved.mount.to_string(),
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

/// The reader archives mounted for one context, in the original's mount order.
///
/// The set owns its archives and their bytes, so a resolution stays readable
/// for as long as the set lives and no file handle outlives a session (spec F04
/// non-negotiable behavior 4).
#[derive(Debug, Default)]
pub struct ReaderMounts {
    archives: Vec<ReaderArchive>,
}

impl ReaderMounts {
    /// An empty set: no reader archive is mounted.
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

    /// How many archives are mounted.
    pub fn len(&self) -> usize {
        self.archives.len()
    }

    /// Whether no archive is mounted.
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
    /// The lookup is exactly three rules, in order:
    ///
    /// 1. the requested path is reduced to its **basename** (`targets.zrd`,
    ///    whatever directory the caller spelled);
    /// 2. archives whose [`MountScope`](crate::vfs::mount::MountScope) the
    ///    context does not admit are skipped and recorded as such — the
    ///    original only mounts the archives of the world and mission being
    ///    loaded;
    /// 3. the remaining archives are searched in the original's mount order
    ///    (root, then mission, then world) and the **first** one holding the
    ///    name serves it.
    ///
    /// Archives searched after the winner are still reported, as
    /// [`ReaderAttemptOutcome::Shadowed`], because "the world copy was there
    /// and the mission copy won" is the fact a diagnostic has to be able to
    /// see.
    ///
    /// # Errors
    ///
    /// [`ReaderLookupError::ForeignNamespace`] when `requested` is not a
    /// reader key, [`ReaderLookupError::ForeignVariant`] when it specializes a
    /// variant (a reader archive declares one copy of each name and indexes it
    /// at [`AssetVariant::default`], so it cannot answer a variant), and
    /// [`ReaderLookupError::NotFound`] when no mounted archive the context
    /// admits holds the name.
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

        let trace = ReaderTrace {
            attempts,
            order_status: READER_LOOKUP_ORDER_STATUS,
        };
        let Some((index, entry_index)) = selected else {
            return Err(ReaderLookupError::NotFound {
                requested: Box::new(requested.clone()),
                basename: basename.to_owned(),
                trace: Box::new(trace),
            });
        };

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
            mount: archive.mount.id().clone(),
            container: archive.container.clone(),
            level: archive.level,
            entry_index,
            member: spelling,
            span,
            trace,
        })
    }

    /// The bytes a resolution names, digest-checked, through the archive it
    /// came from.
    ///
    /// The archive is found by mount id, so a set that registered the **same**
    /// mount id twice cannot say which of the two a resolution came from: that
    /// is refused ([`ReaderReadError::AmbiguousArchive`]) instead of picking
    /// the first one and returning its bytes as another archive's.
    ///
    /// # Errors
    ///
    /// [`ReaderReadError::UnknownArchive`] when the set no longer holds the
    /// archive the resolution names, [`ReaderReadError::AmbiguousArchive`] when
    /// it holds more than one under that id, plus everything
    /// [`ReaderArchive::read`] reports.
    pub fn read(&self, resolved: &ReaderResolution) -> Result<Vec<u8>, ReaderReadError> {
        if self.archives_named(&resolved.mount) > 1 {
            return Err(ReaderReadError::AmbiguousArchive {
                mount: resolved.mount.to_string(),
            });
        }
        self.archive(&resolved.mount)
            .ok_or_else(|| ReaderReadError::UnknownArchive {
                mount: resolved.mount.to_string(),
            })?
            .read(resolved)
    }
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

/// Everything a reader lookup saw: every mounted archive in the original's
/// search order, what it did, and how well that order is known.
///
/// The status is [`READER_LOOKUP_ORDER_STATUS`], **not**
/// [`PRECEDENCE_ORDER_STATUS`]: a reader lookup never consults the designed
/// precedence order, so reporting that status here would credit this trace
/// with an order it did not use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderTrace {
    /// The archives consulted, in the original's mount order.
    pub attempts: Vec<ReaderAttempt>,
    /// Evidence status of that order.
    pub order_status: ClaimStatus,
}

impl fmt::Display for ReaderTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = self.attempts.iter().map(ReaderAttempt::to_string).collect();
        write!(f, "{} [{}]", rendered.join("; "), self.order_status.label())
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
    /// The archive that served it.
    pub mount: MountId,
    /// That archive's installation spelling.
    pub container: String,
    /// Which level of the mount list served it.
    pub level: ReaderLevel,
    /// The entry index inside that archive.
    pub entry_index: usize,
    /// The member's spelling, exactly as the archive declares it.
    pub member: String,
    /// The immutable origin of the member's bytes.
    pub span: SourceSpan,
    /// The ordered attempts that produced this answer.
    pub trace: ReaderTrace,
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
    /// No mounted archive the context admits holds the name.
    NotFound {
        /// The key that was asked for.
        requested: Box<AssetKey>,
        /// The basename the lookup searched for.
        basename: String,
        /// The archives it searched, in order.
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
            Self::NotFound {
                requested,
                basename,
                trace,
            } => write!(
                f,
                "no mounted reader archive holds {basename:?} for {requested}; attempts: {trace}"
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
    /// The resolution does not describe the entry it was made for any more.
    StaleResolution {
        /// The mount the resolution names.
        mount: String,
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
}

impl ReaderReadError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ForeignArchive { .. } => "foreign_archive",
            Self::UnknownArchive { .. } => "unknown_archive",
            Self::AmbiguousArchive { .. } => "ambiguous_archive",
            Self::StaleResolution { .. } => "stale_resolution",
            Self::OutOfBounds { .. } => "out_of_bounds",
            Self::DigestMismatch { .. } => "digest_mismatch",
        }
    }
}

impl fmt::Display for ReaderReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Self::StaleResolution { mount } => write!(
                f,
                "the resolution of {mount} no longer describes the entry it was made for"
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
