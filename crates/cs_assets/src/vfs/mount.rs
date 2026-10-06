//! Mounted sources: what a mount declares and how its members are indexed.
//!
//! A mount is one immutable source — an archive, a directory tree or an
//! overlay — registered under a stable [`MountId`], inside exactly one
//! mount namespace, with an explicit [`PrecedenceClass`] and a
//! [`MountScope`] saying which contexts it may serve (spec F04,
//! "Deliverable and interfaces"; non-negotiable behavior 2).
//!
//! Members are indexed by (variant, logical path), so a mount answers the
//! key it holds, not a basename guess: two archives that both contain
//! `textures\hud\alert.dds` stay two members of two mounts, and nothing
//! here flattens them into a first-wins map (non-negotiable behavior 3).
//!
//! Nothing in this module opens files. A member added here already carries
//! its container offset and size so the immutable [`SourceSpan`] a
//! resolution returns can be built without IO; mounting a host directory
//! and reading its bytes is [`crate::vfs::source`] (F04-B).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use cs_types::asset_id::{
    AssetKey, AssetVariant, MissionScope, ModId, MountId, MountNamespace, PrecedenceClass,
    ResolveContext, WorldGroup,
};
use cs_types::evidence::ContentHash;
use cs_types::install::{LocaleLabel, RelativePath, RelativePathError};

/// Why a context does not admit a mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkipReason {
    /// The mount is bound to a world group, mission or locale this context
    /// does not select.
    ScopeMismatch,
    /// The mount belongs to a mod this context has not opted into
    /// (non-negotiable behavior 2, "opt-in mods").
    ModNotOptedIn,
}

impl SkipReason {
    /// The stable label used in traces and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ScopeMismatch => "scope_mismatch",
            Self::ModNotOptedIn => "mod_not_opted_in",
        }
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which contexts a mount may serve.
///
/// Every present binding must match the [`ResolveContext`] for the mount
/// to be eligible; absent bindings restrict nothing, and an all-`None`
/// scope serves every context. Matching is logical, like every legacy
/// spelling in this workspace: world groups compare by their case-folded
/// key and locales case-insensitively.
///
/// Which retail sources are actually bound to a world, a mission or a
/// locale is unknown at this stage; the mechanism is the designed half of
/// spec F04 non-negotiable behavior 2 and is recorded as such.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MountScope {
    /// The world group this mount belongs to, if any.
    pub world_group: Option<WorldGroup>,
    /// The mission this mount belongs to, if any.
    pub mission: Option<MissionScope>,
    /// The locale this mount serves, if any.
    pub locale: Option<LocaleLabel>,
    /// The mod this mount provides, if any. Required for the
    /// [`PrecedenceClass::Mod`] class.
    pub mod_id: Option<ModId>,
}

impl MountScope {
    /// Whether `context` admits this mount.
    pub fn matches(&self, context: &ResolveContext) -> bool {
        self.admit(context).is_ok()
    }

    /// Why `context` does not admit this mount, or `Ok(())` when it does.
    pub fn admit(&self, context: &ResolveContext) -> Result<(), SkipReason> {
        if let Some(mod_id) = &self.mod_id
            && !context.mods.contains(mod_id)
        {
            return Err(SkipReason::ModNotOptedIn);
        }
        if let Some(world_group) = &self.world_group
            && context.world_group.as_ref() != Some(world_group)
        {
            return Err(SkipReason::ScopeMismatch);
        }
        if let Some(mission) = &self.mission
            && context.mission.as_ref() != Some(mission)
        {
            return Err(SkipReason::ScopeMismatch);
        }
        if let Some(locale) = &self.locale {
            let selected = context
                .locale
                .as_ref()
                .is_some_and(|chosen| chosen.as_str().eq_ignore_ascii_case(locale.as_str()));
            if !selected {
                return Err(SkipReason::ScopeMismatch);
            }
        }
        Ok(())
    }
}

impl fmt::Display for MountScope {
    /// `shared` for an unbound scope, otherwise the bindings it declares.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(group) = &self.world_group {
            parts.push(format!("world={group}"));
        }
        if let Some(mission) = &self.mission {
            parts.push(format!("mission={mission}"));
        }
        if let Some(locale) = &self.locale {
            parts.push(format!("locale={}", locale.as_str()));
        }
        if let Some(mod_id) = &self.mod_id {
            parts.push(format!("mod={mod_id}"));
        }
        if parts.is_empty() {
            f.write_str("shared")
        } else {
            f.write_str(&parts.join(","))
        }
    }
}

/// Why a member (or a whole mount) was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountError {
    /// The member spelling could escape its container: `..`, an absolute
    /// spelling, a drive prefix, an empty or `.` component, or a NUL byte.
    ///
    /// The raw spelling is carried so a diagnostic can quote exactly what
    /// the archive contained (spec F04 non-negotiable behavior 1; the
    /// export-directory half is F04-C).
    InvalidMemberPath {
        /// The rejected spelling, exactly as given.
        spelling: String,
        /// Which path rule rejected it.
        reason: RelativePathError,
    },
    /// The member's byte range leaves the unsigned 64-bit range.
    SpanOverflow {
        /// The rejected spelling.
        spelling: String,
        /// The declared offset.
        offset: u64,
        /// The declared length.
        length: u64,
    },
    /// A second member folded to a key the mount already holds, so the two
    /// spellings collide inside one mount.
    ///
    /// Both spellings are carried because both origins are what a case-only
    /// collision diagnostic has to show (spec F04 AC02).
    DuplicateMember {
        /// The case-folded `variant/path` both spellings map to.
        logical_key: String,
        /// The spelling already indexed.
        first_spelling: String,
        /// The spelling that collided with it.
        second_spelling: String,
    },
    /// The container path was empty, which locates nothing.
    EmptyContainer,
    /// The container path contained a NUL byte.
    ContainerNul,
    /// The mount is [`PrecedenceClass::Mod`] but does not name the mod it
    /// provides, so no context could ever opt into it.
    ModClassWithoutBinding {
        /// The mount being built.
        id: MountId,
    },
    /// The mount names a mod but is not [`PrecedenceClass::Mod`], which
    /// would let a non-mod source hide behind a mod binding.
    BindingWithoutModClass {
        /// The mount being built.
        id: MountId,
        /// The class that was declared instead.
        class: PrecedenceClass,
    },
    /// The mount id is already registered in the VFS.
    DuplicateMountId {
        /// The repeated id.
        id: MountId,
    },
    /// A second, different GOS name-matching rule was requested for one
    /// VFS. Two chains in one VFS answering the same key space under
    /// different rules would make the answer depend on which chain a caller
    /// happened to mount, so the conflict is reported rather than resolved.
    ConflictingGosNameMatch {
        /// The rule the VFS already answers with.
        installed: crate::vfs::gos::GosNameMatch,
        /// The rule that was asked for.
        requested: crate::vfs::gos::GosNameMatch,
    },
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMemberPath { spelling, reason } => {
                write!(f, "member spelling {spelling:?} is rejected: {reason}")
            }
            Self::SpanOverflow {
                spelling,
                offset,
                length,
            } => write!(
                f,
                "member {spelling:?} declares {offset}+{length}, which overflows \
                 the 64-bit byte range"
            ),
            Self::DuplicateMember {
                logical_key,
                first_spelling,
                second_spelling,
            } => write!(
                f,
                "member {second_spelling:?} collides with {first_spelling:?} at \
                 {logical_key:?}; equal-priority duplicates are never flattened \
                 to a first-wins entry"
            ),
            Self::EmptyContainer => write!(f, "mount container path must not be empty"),
            Self::ContainerNul => write!(f, "mount container path must not contain a NUL byte"),
            Self::ModClassWithoutBinding { id } => write!(
                f,
                "mount {id} is a mod source but does not name its mod; a mod mount \
                 must declare which mod it provides"
            ),
            Self::BindingWithoutModClass { id, class } => write!(
                f,
                "mount {id} names a mod but has precedence class {class}; only a mod \
                 source may bind to a mod"
            ),
            Self::DuplicateMountId { id } => write!(f, "mount id {id} is already mounted"),
            Self::ConflictingGosNameMatch {
                installed,
                requested,
            } => write!(
                f,
                "this VFS already matches GOS names by {installed}; {requested} was \
                 requested, which would make one key space answer under two rules"
            ),
        }
    }
}

impl MountError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidMemberPath { .. } => "invalid_member_path",
            Self::SpanOverflow { .. } => "span_overflow",
            Self::DuplicateMember { .. } => "duplicate_member",
            Self::EmptyContainer => "empty_container",
            Self::ContainerNul => "container_nul",
            Self::ModClassWithoutBinding { .. } => "mod_class_without_binding",
            Self::BindingWithoutModClass { .. } => "binding_without_mod_class",
            Self::DuplicateMountId { .. } => "duplicate_mount_id",
            Self::ConflictingGosNameMatch { .. } => "conflicting_gos_name_match",
        }
    }
}

impl std::error::Error for MountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidMemberPath { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// One member of a mount: its original spelling inside the container plus
/// the byte range and digest a resolution reports.
///
/// The fields are private and the only constructor validates the range,
/// so every `MemberRecord` that exists has an in-range `offset + size`
/// (IDENTITY-CONTENT numeric contract).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberRecord {
    spelling: RelativePath,
    size_bytes: u64,
    offset: u64,
    sha256: Option<ContentHash>,
    /// The member's host path relative to the mount's backing directory,
    /// exactly as the walk observed it; `None` for a declared member that
    /// has no host bytes.
    host_relative: Option<PathBuf>,
}

impl MemberRecord {
    /// Validates and records one member's location.
    pub fn new(
        spelling: RelativePath,
        size_bytes: u64,
        offset: u64,
        sha256: Option<ContentHash>,
    ) -> Result<Self, MountError> {
        offset
            .checked_add(size_bytes)
            .ok_or(MountError::SpanOverflow {
                spelling: spelling.as_str().to_owned(),
                offset,
                length: size_bytes,
            })?;
        Ok(Self {
            spelling,
            size_bytes,
            offset,
            sha256,
            host_relative: None,
        })
    }

    /// The member's original spelling inside the container.
    pub fn spelling(&self) -> &RelativePath {
        &self.spelling
    }

    /// The member's length in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// The member's first byte inside the container.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The member's digest, or `None` when it was not hashed.
    pub fn sha256(&self) -> Option<ContentHash> {
        self.sha256
    }

    /// The member's host path relative to its mount's backing directory,
    /// or `None` for a declared member without host bytes.
    pub(crate) fn host_relative(&self) -> Option<&Path> {
        self.host_relative.as_deref()
    }
}

/// The index key of a member inside one mount: variant plus logical path.
///
/// Private to this module: lookups go through [`Mount::member`], which
/// builds the key from an [`AssetKey`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MemberKey {
    variant: String,
    path_key: String,
}

/// Where a mount's bytes can be read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Backing {
    /// Members were declared with their locations only; there are no host
    /// bytes behind them (a fixture, or an archive index whose reader has
    /// not landed yet).
    Declared,
    /// Members are regular files below this host directory, walked by
    /// [`crate::vfs::source::mount_directory`].
    Directory(PathBuf),
}

/// One immutable mounted source.
///
/// Built through [`MountBuilder`]; the member index can no longer change
/// once the mount exists, which is what lets a resolution hand back an
/// immutable [`SourceSpan`] (spec F04, "The VFS returns immutable
/// `SourceSpan` plus a resolution trace").
#[derive(Clone, Debug)]
pub struct Mount {
    id: MountId,
    namespace: MountNamespace,
    precedence: PrecedenceClass,
    scope: MountScope,
    container: String,
    members: BTreeMap<MemberKey, MemberRecord>,
    backing: Backing,
    retail: bool,
}

impl Mount {
    /// The stable id this mount is registered under.
    pub fn id(&self) -> &MountId {
        &self.id
    }

    /// The key space this mount serves.
    pub fn namespace(&self) -> &MountNamespace {
        &self.namespace
    }

    /// How strongly this mount competes for a key.
    pub fn precedence(&self) -> PrecedenceClass {
        self.precedence
    }

    /// Which contexts this mount may serve.
    pub fn scope(&self) -> &MountScope {
        &self.scope
    }

    /// The container (archive or directory) recorded in every span this
    /// mount produces.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The host directory this mount reads from, or `None` for a declared
    /// mount without host bytes.
    pub fn host_root(&self) -> Option<&Path> {
        match &self.backing {
            Backing::Declared => None,
            Backing::Directory(root) => Some(root),
        }
    }

    /// Whether the mount's bytes are original installation data
    /// ([`MountBuilder::retail`]). Only retail sources are subject to the
    /// unmeasured-order block of `Vfs::resolve_blocking_unmeasured`.
    pub fn is_retail(&self) -> bool {
        self.retail
    }

    /// How many members the mount holds.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Every member with the variant it is indexed under, in key order.
    pub fn members(&self) -> impl Iterator<Item = (&str, &MemberRecord)> {
        self.members
            .iter()
            .map(|(key, member)| (key.variant.as_str(), member))
    }

    /// The member serving `key`, or `None` when this mount does not hold
    /// it.
    ///
    /// A key in another [`MountNamespace`] is never served, even when this
    /// is called directly instead of through `Vfs::resolve`: a mount
    /// answers only its own key space, so two mounts in different
    /// namespaces cannot collide by construction.
    ///
    /// The comparison is the key's logical form, so a case- or
    /// separator-differing spelling of the same legacy path still finds
    /// the member, while the member keeps its own original spelling.
    pub fn member(&self, key: &AssetKey) -> Option<&MemberRecord> {
        if self.namespace != *key.namespace() {
            return None;
        }
        self.members.get(&MemberKey {
            variant: key.variant().as_str().to_owned(),
            path_key: key.path_key().to_owned(),
        })
    }

    /// The member serving `key` **only when its stored spelling equals the
    /// request's spelling**, byte for byte — case and separators included.
    ///
    /// [`Mount::member`] compares the *logical* form, so `Alert.TGA` finds
    /// `alert.tga`. That folding is right for every key space whose matching
    /// rule is the legacy case-insensitive one, and wrong to assume for the
    /// GOS space, where the original's own matching rule folds **only the
    /// request** (`MetaOpenFile`, task #693,
    /// `docs/findings/2026-10-06-t693-metaopenfile-name-matching.md`): it
    /// upper-cases the request and compares it byte for byte against the
    /// stored name, so `crimson.rof`'s `ASSETS/GRAPHICS/ARIAL8.TGA` answers
    /// the loose tree's `ASSETS/GRAPHICS/arial8.tga` under every casing,
    /// while a stored name holding a lowercase letter could never be
    /// answered. This lookup is the exact-spelling candidate: it
    /// reports a member only when the two spellings are identical, so a
    /// caller can offer both rules instead of assuming one.
    ///
    /// The namespace and variant must match exactly, as in
    /// [`Mount::member`], so a key of another key space is never answered.
    pub fn member_spelled_exactly(&self, key: &AssetKey) -> Option<&MemberRecord> {
        if self.namespace != *key.namespace() || *key.variant() != AssetVariant::default() {
            return None;
        }
        let asked = key.path().as_str();
        self.members
            .values()
            .find(|member| member.spelling().as_str() == asked)
    }
}

impl fmt::Display for Mount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, {}, {})",
            self.id, self.container, self.precedence, self.scope
        )
    }
}

/// Builds one [`Mount`] member by member.
///
/// Membership is where spec F04 non-negotiable behavior 1 is enforced for
/// content: a raw member spelling is validated as a [`RelativePath`] (no
/// `..`, absolute spelling, drive prefix, empty/`.` component or NUL), and
/// a second member that folds onto an existing key is refused with both
/// spellings instead of overwriting the first (non-negotiable behavior 3).
#[derive(Clone, Debug)]
pub struct MountBuilder {
    id: MountId,
    namespace: MountNamespace,
    precedence: PrecedenceClass,
    scope: MountScope,
    container: String,
    members: BTreeMap<MemberKey, MemberRecord>,
    backing: Backing,
    retail: bool,
}

impl MountBuilder {
    /// Starts a mount of `container` in `namespace` with the given
    /// precedence.
    ///
    /// The container is stored verbatim and validated by
    /// [`MountBuilder::build`]: it is provenance, so it is never used to
    /// read anything in this stage.
    pub fn new(
        id: MountId,
        namespace: MountNamespace,
        precedence: PrecedenceClass,
        container: &str,
    ) -> Self {
        Self {
            id,
            namespace,
            precedence,
            scope: MountScope::default(),
            container: container.to_owned(),
            members: BTreeMap::new(),
            backing: Backing::Declared,
            retail: false,
        }
    }

    /// Declares that the mount's bytes are original installation data, so
    /// a lookup that only the unmeasured precedence order decides between
    /// it and another retail mount is blocked (spec F04 non-negotiable
    /// behavior 2).
    pub fn retail(mut self) -> Self {
        self.retail = true;
        self
    }

    /// Binds the mount to a world group.
    pub fn with_world_group(mut self, world_group: WorldGroup) -> Self {
        self.scope.world_group = Some(world_group);
        self
    }

    /// Binds the mount to a mission.
    pub fn with_mission(mut self, mission: MissionScope) -> Self {
        self.scope.mission = Some(mission);
        self
    }

    /// Binds the mount to a locale.
    pub fn with_locale(mut self, locale: LocaleLabel) -> Self {
        self.scope.locale = Some(locale);
        self
    }

    /// Binds the mount to the mod it provides; requires
    /// [`PrecedenceClass::Mod`] and is checked by [`MountBuilder::build`].
    pub fn with_mod(mut self, mod_id: ModId) -> Self {
        self.scope.mod_id = Some(mod_id);
        self
    }

    /// Adds a member at the default variant ([`AssetVariant::default`]).
    pub fn add_member(
        &mut self,
        spelling: &str,
        size_bytes: u64,
        offset: u64,
        sha256: Option<ContentHash>,
    ) -> Result<&mut Self, MountError> {
        self.add_member_variant(
            spelling,
            AssetVariant::default(),
            size_bytes,
            offset,
            sha256,
        )
    }

    /// Adds a member at `variant`, validating its spelling, its byte range
    /// and its uniqueness inside this mount.
    pub fn add_member_variant(
        &mut self,
        spelling: &str,
        variant: AssetVariant,
        size_bytes: u64,
        offset: u64,
        sha256: Option<ContentHash>,
    ) -> Result<&mut Self, MountError> {
        let path = RelativePath::new(spelling).map_err(|reason| MountError::InvalidMemberPath {
            spelling: spelling.to_owned(),
            reason,
        })?;
        let member = MemberRecord::new(path, size_bytes, offset, sha256)?;
        self.insert(variant, member)
    }

    /// The id the mount will be registered under.
    pub(crate) fn id(&self) -> &MountId {
        &self.id
    }

    /// The key space the mount will serve.
    pub(crate) fn namespace(&self) -> &MountNamespace {
        &self.namespace
    }

    /// The container label every span of this mount will record.
    pub(crate) fn container(&self) -> &str {
        &self.container
    }

    /// Whether any member has been added yet.
    pub(crate) fn has_members(&self) -> bool {
        !self.members.is_empty()
    }

    /// Backs the mount by the host directory `root`.
    pub(crate) fn set_directory_backing(&mut self, root: PathBuf) {
        self.backing = Backing::Directory(root);
    }

    /// Adds a whole host file at the default variant: offset 0, its hashed
    /// length and digest, and the host path it was found at, relative to
    /// the backing directory.
    pub(crate) fn add_host_file(
        &mut self,
        spelling: &str,
        host_relative: PathBuf,
        size_bytes: u64,
        sha256: ContentHash,
    ) -> Result<&mut Self, MountError> {
        let path = RelativePath::new(spelling).map_err(|reason| MountError::InvalidMemberPath {
            spelling: spelling.to_owned(),
            reason,
        })?;
        let mut member = MemberRecord::new(path, size_bytes, 0, Some(sha256))?;
        member.host_relative = Some(host_relative);
        self.insert(AssetVariant::default(), member)
    }

    /// Indexes `member` at `variant`, refusing a second spelling of a key
    /// the mount already holds.
    fn insert(
        &mut self,
        variant: AssetVariant,
        member: MemberRecord,
    ) -> Result<&mut Self, MountError> {
        let key = MemberKey {
            variant: variant.as_str().to_owned(),
            path_key: member.spelling().logical_key(),
        };
        if let Some(existing) = self.members.get(&key) {
            return Err(MountError::DuplicateMember {
                logical_key: format!("{}/{}", variant.as_str(), key.path_key),
                first_spelling: existing.spelling().as_str().to_owned(),
                second_spelling: member.spelling().as_str().to_owned(),
            });
        }
        self.members.insert(key, member);
        Ok(self)
    }

    /// Validates the mount as a whole and freezes it.
    pub fn build(self) -> Result<Mount, MountError> {
        if self.container.is_empty() {
            return Err(MountError::EmptyContainer);
        }
        if self.container.contains('\0') {
            return Err(MountError::ContainerNul);
        }
        match (self.precedence, &self.scope.mod_id) {
            (PrecedenceClass::Mod, None) => {
                return Err(MountError::ModClassWithoutBinding { id: self.id });
            }
            (class, Some(_)) if class != PrecedenceClass::Mod => {
                return Err(MountError::BindingWithoutModClass { id: self.id, class });
            }
            _ => {}
        }
        Ok(Mount {
            id: self.id,
            namespace: self.namespace,
            precedence: self.precedence,
            scope: self.scope,
            container: self.container,
            members: self.members,
            backing: self.backing,
            retail: self.retail,
        })
    }
}
