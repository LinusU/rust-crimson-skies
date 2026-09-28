//! Asset identity, resolution context and precedence contracts (F04-A).
//!
//! Spec F04, "Deliverable and interfaces": an [`AssetKey`] is
//! **(mount namespace, logical path, variant)**, and a [`ResolveContext`]
//! carries the installation fingerprint, world group, locale, mission and
//! mod stack a lookup is made under. [`PrecedenceClass`] is the explicit
//! order required by spec F04 non-negotiable behavior 2 (opt-in mods over
//! verified patch overlays over mission/world-specific sources over shared
//! sources), and [`PRECEDENCE_ORDER_STATUS`] states how much that order is
//! known to be right today: `designed`, until F04-D measures original
//! lookup behavior.
//!
//! The lookup machinery itself lives in `cs_assets::vfs`, which consumes
//! these records and answers `resolve(context, key)` with an immutable
//! [`SourceSpan`] plus a resolution trace (`docs/contracts/
//! IDENTITY-CONTENT.md`, "Lookup contract"). Nothing in this module is
//! derived from original game data.
//!
//! **Vocabulary is open and validated.** Mount namespaces, variants, mod
//! ids and mission scopes are engine-authored lowercase labels
//! ([`MAX_LABEL_LEN`] bytes of `[a-z0-9._-]`, the `install::FileFamily`
//! rule). The *retail* sets of those labels are unknown and are not
//! guessed here; an exhaustive set is not claimed (spec F04, "Research
//! boundary"; `docs/findings/2026-09-28-f04-a-asset-key-and-precedence-
//! contracts.md`).
//!
//! **Spellings are retained, lookups are logical.** The logical path of a
//! key is a `install::RelativePath`, so `..`, absolute spellings, drive
//! prefixes, `.`/empty components and NUL bytes are rejected before a key
//! can exist (`install::RelativePathError`), while the original spelling —
//! letter case and separator style — survives for display and diagnostics.
//! Two keys that differ only in that spelling are the same key: they
//! compare equal, hash equal and order equal (spec F04 non-negotiable
//! behavior 1; the F02 logical-identity principle applied to lookups).

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::evidence::SourceSpan as ByteSpan;
use crate::evidence::{ClaimStatus, ContentHash};
use crate::install::{LocaleLabel, RelativePath, RelativePathError};

/// Maximum byte length of a validated label in this module: mount
/// namespace, asset variant, mod id and mission scope.
pub const MAX_LABEL_LEN: usize = 64;

/// How well the precedence order implemented by [`PrecedenceClass`] is
/// known.
///
/// Spec F04 non-negotiable behavior 2 requires the baseline order to be
/// labeled *designed* until original ordering is measured. That
/// measurement is F04-D work (retail capability), so every resolution
/// report built on this order inherits `designed` and may never be
/// presented as measured original behavior.
pub const PRECEDENCE_ORDER_STATUS: ClaimStatus = ClaimStatus::Designed;

/// Validates one of this module's labels: lowercase ASCII alphanumerics
/// plus `.`, `_` and `-`, starting with a lowercase letter or digit, at
/// most [`MAX_LABEL_LEN`] bytes.
///
/// The rule is `install::FileFamily`'s, so every engine-authored label in
/// the workspace is validated the same way. Uppercase is rejected rather
/// than folded: labels are new-engine spelling, not legacy content
/// spelling, and a rejected input is louder than a silently rewritten one.
fn validate_label(label: &'static str, raw: &str) -> Result<String, LabelError> {
    if raw.is_empty() {
        return Err(LabelError::Empty { label });
    }
    if raw.len() > MAX_LABEL_LEN {
        return Err(LabelError::TooLong {
            label,
            len: raw.len(),
        });
    }
    let mut chars = raw.chars();
    let first = chars.next().expect("label is not empty");
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(LabelError::BadFirst { label, ch: first });
    }
    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && !matches!(ch, '.' | '_' | '-') {
            return Err(LabelError::BadCharacter { label, ch });
        }
    }
    Ok(raw.to_owned())
}

/// Why a validated label was rejected.
///
/// `label` names the field that failed, so one error type serves every
/// label in this module without repeating four near-identical enums.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelError {
    /// The label was empty.
    Empty {
        /// Which label failed, e.g. `mount namespace`.
        label: &'static str,
    },
    /// The label exceeded [`MAX_LABEL_LEN`] bytes.
    TooLong {
        /// Which label failed.
        label: &'static str,
        /// Its length in bytes.
        len: usize,
    },
    /// The label did not start with a lowercase ASCII letter or digit.
    BadFirst {
        /// Which label failed.
        label: &'static str,
        /// The offending character.
        ch: char,
    },
    /// The label contained a character outside `[a-z0-9._-]`.
    BadCharacter {
        /// Which label failed.
        label: &'static str,
        /// The offending character.
        ch: char,
    },
}

impl fmt::Display for LabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { label } => write!(f, "{label} label must not be empty"),
            Self::TooLong { label, len } => {
                write!(f, "{label} label is {len} bytes, max is {MAX_LABEL_LEN}")
            }
            Self::BadFirst { label, ch } => write!(
                f,
                "{label} label must start with a lowercase letter or digit, got {ch:?}"
            ),
            Self::BadCharacter { label, ch } => {
                write!(f, "{label} label contains disallowed character {ch:?}")
            }
        }
    }
}

impl std::error::Error for LabelError {}

/// The mount namespace of an [`AssetKey`]: which key space a lookup
/// targets.
///
/// Namespaces partition lookups: a mount only ever answers keys in its own
/// namespace, so two mounts in different namespaces cannot collide by
/// construction. The retail set of namespaces is unknown; the label is an
/// open, validated, engine-authored vocabulary.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MountNamespace(String);

impl MountNamespace {
    /// Validates and wraps a mount namespace label.
    pub fn new(label: &str) -> Result<Self, LabelError> {
        validate_label("mount namespace", label).map(Self)
    }

    /// The label as a string slice (already lowercase).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MountNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The stable identity of one mounted source: an archive, a directory
/// tree or an overlay, named once and referenced by every resolution
/// trace, attempt and conflict report.
///
/// Ids are engine-authored and validated like the other labels in this
/// module; they are not original game names. Two mounts may not share an
/// id (`cs_assets::vfs::Vfs::mount` rejects the second one), so a trace
/// always points at exactly one mount.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MountId(String);

impl MountId {
    /// Validates and wraps a mount id label.
    pub fn new(label: &str) -> Result<Self, LabelError> {
        validate_label("mount id", label).map(Self)
    }

    /// The label as a string slice (already lowercase).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The variant of an [`AssetKey`]: which alternative of one logical asset
/// is requested.
///
/// A variant is part of the key, so two members with the same path but
/// different variants are distinct members of a mount. The retail variant
/// vocabulary is unknown; [`AssetVariant::default`] is the engine's own
/// neutral label, used whenever a request does not specialize.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetVariant(String);

impl AssetVariant {
    /// Validates and wraps an asset variant label.
    pub fn new(label: &str) -> Result<Self, LabelError> {
        validate_label("asset variant", label).map(Self)
    }

    /// The label as a string slice (already lowercase).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AssetVariant {
    /// The neutral variant an ordinary request uses when it does not
    /// specialize. This label is authored engine design, not an observed
    /// retail value.
    fn default() -> Self {
        Self::new("default").expect("the constant asset variant label is valid")
    }
}

impl fmt::Display for AssetVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why an [`AssetKey`] was rejected when built from raw spellings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetKeyError {
    /// The mount namespace label was invalid.
    Namespace(LabelError),
    /// The logical path could escape a mount root or was malformed.
    Path(RelativePathError),
    /// The variant label was invalid.
    Variant(LabelError),
}

impl fmt::Display for AssetKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Namespace(err) => write!(f, "asset key namespace is invalid: {err}"),
            Self::Path(err) => write!(f, "asset key path is invalid: {err}"),
            Self::Variant(err) => write!(f, "asset key variant is invalid: {err}"),
        }
    }
}

impl std::error::Error for AssetKeyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Namespace(err) => Some(err),
            Self::Path(err) => Some(err),
            Self::Variant(err) => Some(err),
        }
    }
}

/// What is asked for: (mount namespace, logical path, variant).
///
/// The key keeps the caller's original path spelling for display and
/// diagnostics but compares, hashes and orders on the **logical** form
/// (components joined with `/`, ASCII-lowercased), so
/// `Content\Texture.Archive` and `content/texture.archive` name one asset
/// — spec F04 non-negotiable behavior 1. `..`, absolute spellings, drive
/// prefixes and NUL bytes cannot appear: the path is a
/// [`RelativePath`], which rejects all of them at construction.
///
/// Keys never collide across namespaces or variants, and a key says
/// nothing about *which* file serves it: that is the `cs_assets::vfs`
/// resolution decision, made against a [`ResolveContext`].
#[derive(Clone, Debug)]
pub struct AssetKey {
    namespace: MountNamespace,
    path: RelativePath,
    /// `path.logical_key()` computed once, so equality and hashing never
    /// reallocate.
    path_key: String,
    variant: AssetVariant,
}

impl AssetKey {
    /// Builds a key from already validated parts.
    pub fn new(namespace: MountNamespace, path: RelativePath, variant: AssetVariant) -> Self {
        let path_key = path.logical_key();
        Self {
            namespace,
            path,
            path_key,
            variant,
        }
    }

    /// Validates and builds a key from raw spellings.
    ///
    /// This is the entry point for untrusted input (a CLI argument, a
    /// scripted lookup): it applies the path rules of spec F04
    /// non-negotiable behavior 1 — no `..`, no absolute spelling, no drive
    /// prefix, no NUL — before a key can exist at all.
    pub fn from_spelling(
        namespace: &str,
        path: &str,
        variant: &str,
    ) -> Result<Self, AssetKeyError> {
        let namespace = MountNamespace::new(namespace).map_err(AssetKeyError::Namespace)?;
        let path = RelativePath::new(path).map_err(AssetKeyError::Path)?;
        let variant = AssetVariant::new(variant).map_err(AssetKeyError::Variant)?;
        Ok(Self::new(namespace, path, variant))
    }

    /// The mount namespace the key addresses.
    pub fn namespace(&self) -> &MountNamespace {
        &self.namespace
    }

    /// The logical path with its original spelling and letter case.
    pub fn path(&self) -> &RelativePath {
        &self.path
    }

    /// The requested variant.
    pub fn variant(&self) -> &AssetVariant {
        &self.variant
    }

    /// The case-folded, `/`-separated path a lookup compares against,
    /// cached at construction.
    ///
    /// This is the string a mount's member index is keyed by, so resolving
    /// a key never recomputes the fold once per candidate mount.
    pub fn path_key(&self) -> &str {
        &self.path_key
    }

    /// The canonical comparison form: `namespace/variant/logical-path`.
    ///
    /// Labels contain no `/` (they are validated without one), so the
    /// encoding is injective: equal strings mean equal keys.
    pub fn logical_key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.namespace.as_str(),
            self.variant.as_str(),
            self.path_key
        )
    }

    /// The three components compared by equality, hashing and ordering.
    fn identity(&self) -> (&str, &str, &str) {
        (
            self.namespace.as_str(),
            &self.path_key,
            self.variant.as_str(),
        )
    }
}

impl PartialEq for AssetKey {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for AssetKey {}

impl Hash for AssetKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity().hash(state);
    }
}

impl Ord for AssetKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.identity().cmp(&other.identity())
    }
}

impl PartialOrd for AssetKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for AssetKey {
    /// The original path spelling, so a diagnostic quotes what the caller
    /// (or the archive) actually spelled.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}",
            self.namespace.as_str(),
            self.variant.as_str(),
            self.path.as_str()
        )
    }
}

/// The world group a context is in, or a mount is bound to: a directory of
/// the installation's `ZBD` tree as discovery records it (F02-B
/// `Diagnosis::world_groups`, e.g. `zbd/c1`).
///
/// Equality is logical — the case-folded key — because world groups are
/// looked up case-insensitively like every other legacy spelling; the
/// original spelling stays available through [`WorldGroup::as_relative`].
#[derive(Clone, Debug)]
pub struct WorldGroup(RelativePath);

impl WorldGroup {
    /// Validates and wraps a world-group spelling.
    pub fn new(spelling: &str) -> Result<Self, RelativePathError> {
        RelativePath::new(spelling).map(Self)
    }

    /// Wraps an already validated relative path.
    pub fn from_relative(path: RelativePath) -> Self {
        Self(path)
    }

    /// The original spelling inside the installation.
    pub fn as_relative(&self) -> &RelativePath {
        &self.0
    }

    /// The case-insensitive comparison key.
    pub fn logical_key(&self) -> String {
        self.0.logical_key()
    }
}

impl PartialEq for WorldGroup {
    fn eq(&self, other: &Self) -> bool {
        self.logical_key() == other.logical_key()
    }
}

impl Eq for WorldGroup {}

impl Hash for WorldGroup {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.logical_key().hash(state);
    }
}

impl fmt::Display for WorldGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// The mission a context (or a mount) is scoped to.
///
/// The retail mission catalogue ids arrive with the mission/language
/// tasks; this stage only needs a validated scope label to restrict
/// precedence, and no id set is guessed here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MissionScope(String);

impl MissionScope {
    /// Validates and wraps a mission scope label.
    pub fn new(label: &str) -> Result<Self, LabelError> {
        validate_label("mission scope", label).map(Self)
    }

    /// The label as a string slice (already lowercase).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MissionScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The identity of one opted-in mod.
///
/// Mods are opt-in only: a mount bound to a mod answers nothing unless
/// that mod is present in the [`ModStack`] of the [`ResolveContext`]
/// (spec F04 non-negotiable behavior 2, "opt-in mods"). The retail mod
/// id vocabulary is owned by the mod-manifest task; this is a validated
/// label, not an observed set.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModId(String);

impl ModId {
    /// Validates and wraps a mod id label.
    pub fn new(label: &str) -> Result<Self, LabelError> {
        validate_label("mod id", label).map(Self)
    }

    /// The label as a string slice (already lowercase).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a [`ModStack`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextError {
    /// The same mod was opted into twice, which would make mod-versus-mod
    /// precedence depend on which duplicate is meant.
    DuplicateMod {
        /// The repeated mod.
        id: ModId,
        /// Index of its first occurrence.
        first: usize,
        /// Index of the repeat.
        second: usize,
    },
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateMod { id, first, second } => write!(
                f,
                "mod {id} is opted into twice (positions {first} and {second}); \
                 a mod may appear once in the stack"
            ),
        }
    }
}

impl std::error::Error for ContextError {}

/// The mods a lookup has opted into, in load order.
///
/// The order is meaningful: a **later** entry outranks an earlier one when
/// both provide the same key, which makes mod precedence deterministic
/// instead of ambiguous. That load order is a designed convention, not an
/// observation (spec F04 non-negotiable behavior 2 labels the baseline
/// order `designed`); duplicates are rejected at construction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModStack(Vec<ModId>);

impl ModStack {
    /// Builds a stack, rejecting a mod that appears twice.
    pub fn new(mods: Vec<ModId>) -> Result<Self, ContextError> {
        for (second, id) in mods.iter().enumerate() {
            if let Some(first) = mods[..second].iter().position(|earlier| earlier == id) {
                return Err(ContextError::DuplicateMod {
                    id: id.clone(),
                    first,
                    second,
                });
            }
        }
        Ok(Self(mods))
    }

    /// An empty stack: nothing is opted into.
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Whether `id` is opted into.
    pub fn contains(&self, id: &ModId) -> bool {
        self.0.contains(id)
    }

    /// The load-order position of `id`, or `None` when it is not opted
    /// in. A higher position outranks a lower one.
    pub fn position(&self, id: &ModId) -> Option<usize> {
        self.0.iter().position(|earlier| earlier == id)
    }

    /// The stack in load order.
    pub fn as_slice(&self) -> &[ModId] {
        &self.0
    }

    /// Whether no mod is opted into.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many mods are opted into.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for ModStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            &self
                .0
                .iter()
                .map(ModId::as_str)
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

/// How strongly a mounted source competes for a key.
///
/// Spec F04 non-negotiable behavior 2 fixes the order: opt-in mods over
/// verified patch overlays over mission/world-specific sources over shared
/// sources. The ranking is [`PrecedenceClass::rank`] (higher wins) and its
/// evidence status is [`PRECEDENCE_ORDER_STATUS`] — `designed`, because
/// original lookup behavior has not been measured yet (F04-D).
///
/// What makes a patch overlay *verified*, and which retail source belongs
/// to which class, are mount-construction questions owned by F04-B/F04-D;
/// this enum only fixes the order every consumer must agree on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrecedenceClass {
    /// An opted-in mod, ranked by its position in the [`ModStack`].
    Mod,
    /// A patch overlay.
    Patch,
    /// A mission- or world-specific source.
    MissionWorld,
    /// A source that serves every context.
    Shared,
}

impl PrecedenceClass {
    /// The precedence rank; a higher rank wins.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Mod => 3,
            Self::Patch => 2,
            Self::MissionWorld => 1,
            Self::Shared => 0,
        }
    }

    /// The stable label used in traces and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Patch => "patch",
            Self::MissionWorld => "mission_world",
            Self::Shared => "shared",
        }
    }
}

impl fmt::Display for PrecedenceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Everything that decides *which* file serves a key (spec F04,
/// "Deliverable and interfaces").
///
/// The installation fingerprint is required: every [`SourceSpan`] the VFS
/// returns must name the installation its bytes belong to
/// (`docs/contracts/IDENTITY-CONTENT.md`), so a context without one cannot
/// be constructed. The other four fields scope precedence — a mount bound
/// to any of them only answers a context that matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveContext {
    /// The F02 installation fingerprint (`cs_assets::install::fingerprint`)
    /// the resolution is made against.
    pub installation: ContentHash,
    /// The world group being loaded, if the context is world-specific.
    pub world_group: Option<WorldGroup>,
    /// The locale being loaded, if it is known.
    pub locale: Option<LocaleLabel>,
    /// The mission being loaded, if any.
    pub mission: Option<MissionScope>,
    /// The mods opted into, in load order.
    pub mods: ModStack,
}

impl ResolveContext {
    /// A context against `installation` with no world, locale, mission or
    /// mods selected.
    pub fn new(installation: ContentHash) -> Self {
        Self {
            installation,
            world_group: None,
            locale: None,
            mission: None,
            mods: ModStack::empty(),
        }
    }

    /// Selects the world group.
    pub fn with_world_group(mut self, world_group: WorldGroup) -> Self {
        self.world_group = Some(world_group);
        self
    }

    /// Selects the locale.
    pub fn with_locale(mut self, locale: LocaleLabel) -> Self {
        self.locale = Some(locale);
        self
    }

    /// Selects the mission.
    pub fn with_mission(mut self, mission: MissionScope) -> Self {
        self.mission = Some(mission);
        self
    }

    /// Opts into `mods`, replacing the current stack.
    pub fn with_mods(mut self, mods: ModStack) -> Self {
        self.mods = mods;
        self
    }
}

/// Why a source span was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanError {
    /// `offset + length` left the unsigned 64-bit range, so the span is
    /// not a range that can be read (IDENTITY-CONTENT numeric contract:
    /// treat offsets as unsigned checked ranges).
    RangeOverflow {
        /// The offset the span starts at.
        offset: u64,
        /// The declared length.
        length: u64,
    },
    /// The container path was empty, which locates nothing.
    EmptyContainer,
    /// A member key was present but empty, which names no member.
    EmptyMemberKey,
}

impl fmt::Display for SourceSpanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RangeOverflow { offset, length } => write!(
                f,
                "source span {offset}+{length} overflows the 64-bit byte range"
            ),
            Self::EmptyContainer => write!(f, "source span container path must not be empty"),
            Self::EmptyMemberKey => write!(f, "source span member key must not be empty"),
        }
    }
}

impl std::error::Error for SourceSpanError {}

/// Where immutable bytes came from: the six-field span of
/// `docs/contracts/IDENTITY-CONTENT.md`.
///
/// This is **not** `evidence::SourceSpan`, which is the two-field byte
/// span recorded inside an `ObservationLocator`; [`SourceSpan::byte_span`]
/// converts one of these into that smaller record. The contract sketch
/// spells the hashes as `String`; the typed [`ContentHash`] is used
/// instead, which is the same value with the canonical lowercase-hex
/// guarantee already enforced (`IDENTITY-CONTENT`, "Hash strings must be
/// canonical lowercase hexadecimal").
///
/// Construction validates the checked byte range, and the record is
/// immutable afterwards: fields are private and read through accessors.
/// `member_key` is **provenance only** — it is never joined to a
/// filesystem path, so a hostile archive spelling is recorded here as
/// found, while escaping names are rejected where members are mounted
/// (`cs_assets::vfs::MountBuilder::add_member`) and before any export
/// (F04-C).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    install_sha256: ContentHash,
    container_path: String,
    member_key: Option<String>,
    offset: u64,
    length: u64,
    member_sha256: Option<ContentHash>,
}

impl SourceSpan {
    /// Validates and records where bytes live.
    ///
    /// `member_sha256` is `None` when the member has not been hashed, not
    /// a claim that it has no digest; `member_key` is `None` for a
    /// container that is itself the source (a loose file).
    pub fn new(
        install_sha256: ContentHash,
        container_path: &str,
        member_key: Option<&str>,
        offset: u64,
        length: u64,
        member_sha256: Option<ContentHash>,
    ) -> Result<Self, SourceSpanError> {
        if container_path.is_empty() {
            return Err(SourceSpanError::EmptyContainer);
        }
        if member_key == Some("") {
            return Err(SourceSpanError::EmptyMemberKey);
        }
        offset
            .checked_add(length)
            .ok_or(SourceSpanError::RangeOverflow { offset, length })?;
        Ok(Self {
            install_sha256,
            container_path: container_path.to_owned(),
            member_key: member_key.map(str::to_owned),
            offset,
            length,
            member_sha256,
        })
    }

    /// The installation the bytes belong to.
    pub fn install_sha256(&self) -> ContentHash {
        self.install_sha256
    }

    /// The container (archive or file) the bytes live in, as recorded.
    pub fn container_path(&self) -> &str {
        &self.container_path
    }

    /// The member's original spelling inside the container, or `None` when
    /// the container is the source itself.
    pub fn member_key(&self) -> Option<&str> {
        self.member_key.as_deref()
    }

    /// First byte of the range inside the container.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Length of the range in bytes.
    pub fn length(&self) -> u64 {
        self.length
    }

    /// The member's digest, or `None` when it was not hashed.
    pub fn member_sha256(&self) -> Option<ContentHash> {
        self.member_sha256
    }

    /// The same range as the two-field byte span used by observation
    /// records.
    pub fn byte_span(&self) -> ByteSpan {
        ByteSpan {
            offset: self.offset,
            length: self.length,
        }
    }
}

impl fmt::Display for SourceSpan {
    /// `container[member] offset+length`: enough to quote the origin in a
    /// resolution trace without re-reading anything.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{} {}+{}",
            self.container_path,
            self.member_key
                .as_deref()
                .map(|member| format!(" [{member}]"))
                .unwrap_or_default(),
            self.offset,
            self.length
        )
    }
}
