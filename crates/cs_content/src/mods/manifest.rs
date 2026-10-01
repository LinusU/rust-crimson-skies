//! The typed mod manifest: identity, version, engine range, dependencies,
//! payloads and overrides (F53-A).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-A`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! A [`ModManifest`] is the **typed input** of a mod mount. It is what an
//! F53-B manifest reader produces and what the mount plan validates; this
//! stage reads no bytes and names no on-disk format. Everything that
//! arrives as text is validated by a constructor before the record can
//! exist, so an untrusted manifest cannot carry an id that escapes its
//! namespace or a source path that escapes the mod root
//! (`IDENTITY-CONTENT`: "`ContentId` … validate at construction; no unchecked
//! path join").
//!
//! # What the record holds
//!
//! * [`ModHeader`] — the stable [`ModId`], a display name, a
//!   [`ModVersion`], the closed [`EngineRange`] the mod declares it works
//!   with, the author's `declared_cosmetic_only` claim and the
//!   [`Provenance`] of the document. The claim is *checked against the
//!   effect policy* by the plan (F53 non-negotiable 3), never believed.
//! * [`ModDependency`] — another mod's id, a [`VersionRange`] and a
//!   [`DependencyStrength`]. `Required` and `Conflict` decide whether the
//!   set may be enabled at all; `Optional` is recorded and reported.
//! * [`ModPayload`] — one file the manifest ships, with a validated
//!   mod-root-relative spelling and the [`PayloadKind`] its extension
//!   classifies to. A native library is a record of what the manifest said
//!   and a refusal at the same time: `ModManifest::try_new` rejects the
//!   manifest that carries one, because F53 non-negotiable 2 forbids
//!   executing native DLL/plugin code from an original or mod archive.
//! * [`ContentOverride`] — see [`super::overrides`].
//!
//! # Designed vocabulary, not original data
//!
//! The original game's mod manifest format, id grammar, version scheme,
//! engine range semantics and native-code rules are **unmeasured** (F53
//! "Research boundary"). Nothing here is a claim about them; the labels and
//! bounds are this engine's own declaration, recorded in
//! `docs/findings/2026-10-01-f53-a-mod-manifest-and-override-validation.md`.
//! The one thing this module *does* take from the codebase is the
//! [`ModId`] and the label grammar it shares with
//! [`cs_types::asset_id`], so a mod id in a manifest is the same identity a
//! [`cs_types::asset_id::ModStack`] opts into.

use std::fmt;

use cs_types::asset_id::ModId;
use cs_types::content::{ContentId, Provenance};
use cs_types::install::{RelativePath, RelativePathError};

use super::overrides::ContentOverride;

/// Largest accepted mod display name, in bytes.
///
/// A designed bound: the name is for display and diagnostics only, and the
/// stable identity is the [`ModId`]. It is bounded because a manifest is
/// untrusted input and an unbounded display string is a denial-of-service
/// on every report that quotes it.
pub const MAX_MOD_NAME_BYTES: usize = 64;

/// A `major.minor.patch` version triple.
///
/// One type serves both a mod's own version and the engine's, so an
/// [`EngineRange`] and a [`VersionRange`] are compared with the same
/// ordering. The triples are integers on purpose: `IDENTITY-CONTENT` keeps
/// money, ticks, counts, ammo and ids integral, and a version that could
/// round is a version that could accept an incompatible build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl ModVersion {
    /// Builds a version triple.
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The major component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// The minor component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// The patch component.
    #[must_use]
    pub const fn patch(self) -> u16 {
        self.patch
    }
}

impl fmt::Display for ModVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A half-open-below, optional-above version interval.
///
/// A `None` maximum means "no upper bound declared", which is a *declared*
/// statement, not a measured one: it is reported verbatim so a reader can
/// see that the mod never said which future versions it works with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionRange {
    min: ModVersion,
    max_inclusive: Option<ModVersion>,
}

impl VersionRange {
    /// Validates and builds a range.
    ///
    /// # Errors
    ///
    /// [`ManifestError::DependencyRangeInverted`] when `max_inclusive`
    /// precedes `min`; an interval that can never be satisfied is refused
    /// rather than stored.
    pub fn new(min: ModVersion, max_inclusive: Option<ModVersion>) -> Result<Self, ManifestError> {
        if let Some(max) = max_inclusive
            && max < min
        {
            return Err(ManifestError::DependencyRangeInverted { min, max });
        }
        Ok(Self { min, max_inclusive })
    }

    /// A range with no declared upper bound.
    #[must_use]
    pub const fn at_least(min: ModVersion) -> Self {
        Self {
            min,
            max_inclusive: None,
        }
    }

    /// A closed range from `min` to `max`, both inclusive.
    ///
    /// # Errors
    ///
    /// [`ManifestError::DependencyRangeInverted`] when `max` precedes `min`.
    pub fn between(min: ModVersion, max: ModVersion) -> Result<Self, ManifestError> {
        Self::new(min, Some(max))
    }

    /// Whether `version` satisfies the range.
    #[must_use]
    pub fn contains(&self, version: &ModVersion) -> bool {
        *version >= self.min
            && match self.max_inclusive {
                Some(max) => *version <= max,
                None => true,
            }
    }

    /// The inclusive lower bound. Named `minimum` rather than `min` so it
    /// cannot be confused with the `Ord::min` of two bounds.
    #[must_use]
    pub const fn minimum(&self) -> ModVersion {
        self.min
    }

    /// The inclusive upper bound, or `None` when none was declared.
    #[must_use]
    pub const fn max_inclusive(&self) -> Option<ModVersion> {
        self.max_inclusive
    }
}

impl fmt::Display for VersionRange {
    /// `1.2.0..=2.0.0`, or `1.2.0..` for an open upper bound.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.max_inclusive {
            Some(max) => write!(f, "{}..={max}", self.min),
            None => write!(f, "{}..", self.min),
        }
    }
}

/// The closed engine-version window a mod declares it works with.
///
/// Both bounds are required and inclusive. An unbounded window is refused at
/// construction: F53 non-negotiable 1 validates a "declared engine range"
/// before enabling a mod, and a range that cannot fail cannot be validated.
/// Whether the original game expressed compatibility at all is unmeasured;
/// this is the engine's declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EngineRange {
    min: ModVersion,
    max_inclusive: ModVersion,
}

impl EngineRange {
    /// Validates and builds a closed range.
    ///
    /// # Errors
    ///
    /// [`ManifestError::EngineRangeInverted`] when `max_inclusive` precedes
    /// `min`.
    pub fn new(min: ModVersion, max_inclusive: ModVersion) -> Result<Self, ManifestError> {
        if max_inclusive < min {
            return Err(ManifestError::EngineRangeInverted {
                min,
                max: max_inclusive,
            });
        }
        Ok(Self { min, max_inclusive })
    }

    /// Whether `engine` falls inside the declared window.
    #[must_use]
    pub fn contains(&self, engine: &ModVersion) -> bool {
        *engine >= self.min && *engine <= self.max_inclusive
    }

    /// The inclusive lower bound. Named `minimum` for the same reason as
    /// [`VersionRange::minimum`].
    #[must_use]
    pub const fn minimum(&self) -> ModVersion {
        self.min
    }

    /// The inclusive upper bound.
    #[must_use]
    pub const fn max_inclusive(&self) -> ModVersion {
        self.max_inclusive
    }
}

impl fmt::Display for EngineRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..={}", self.min, self.max_inclusive)
    }
}

/// How strongly a declared dependency is asserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DependencyStrength {
    /// The mod cannot be enabled without it.
    Required,
    /// The mod works better with it; absence is not an error and presence is
    /// reported but not required.
    Optional,
    /// The mod refuses to be enabled alongside it.
    Conflict,
}

impl DependencyStrength {
    /// The stable label used in reports and in the plan's JSON.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Conflict => "conflict",
        }
    }

    /// Whether a mod in the set must decide this dependency before the set
    /// can be enabled.
    #[must_use]
    pub const fn gates_enabling(self) -> bool {
        matches!(self, Self::Required | Self::Conflict)
    }
}

impl fmt::Display for DependencyStrength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One declared dependency on another mod.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModDependency {
    mod_id: ModId,
    range: VersionRange,
    strength: DependencyStrength,
}

impl ModDependency {
    /// Records one dependency. The id is already validated by [`ModId`] and
    /// the range by [`VersionRange::new`], so there is nothing left to
    /// refuse here.
    #[must_use]
    pub fn new(mod_id: ModId, range: VersionRange, strength: DependencyStrength) -> Self {
        Self {
            mod_id,
            range,
            strength,
        }
    }

    /// The mod this dependency names.
    pub fn mod_id(&self) -> &ModId {
        &self.mod_id
    }

    /// The versions of that mod this dependency accepts.
    pub fn range(&self) -> VersionRange {
        self.range
    }

    /// How strongly the dependency is asserted.
    pub fn strength(&self) -> DependencyStrength {
        self.strength
    }
}

impl fmt::Display for ModDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.strength, self.mod_id, self.range)
    }
}

/// What a shipped file is, decided by its extension.
///
/// This is a *classification of what a manifest said*, not a capability:
/// [`ModManifest::try_new`] refuses a manifest that carries
/// [`NativeLibrary`], so a native library can be named in a report and can
/// never be mounted. F53 non-negotiable 2 forbids executing native
/// DLL/plugin code from an original or a mod archive, and this engine has no
/// loader for it at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PayloadKind {
    /// A file a bounded reader parses as data.
    DeclarativeData,
    /// A native library or executable, by extension.
    NativeLibrary,
}

impl PayloadKind {
    /// Extensions classified as native code, lowercased and without the dot.
    ///
    /// A closed list, not a detection: an extension outside it is
    /// [`DeclarativeData`], which is the safe direction because a
    /// declarative file that turns out to be unreadable is refused by its
    /// reader rather than executed.
    pub const NATIVE_EXTENSIONS: &'static [&'static str] =
        &["dll", "so", "dylib", "exe", "com", "ocx", "sys"];

    /// Classifies a file spelling by its last component's extension.
    ///
    /// The check is on the *extension only*, never on the directory it sits
    /// in, and the spelling itself is not validated here: that is
    /// [`ModPayload::new`]'s job, so a hostile spelling is refused for being
    /// hostile rather than mis-classified by its extension.
    #[must_use]
    pub fn from_spelling(spelling: &str) -> Self {
        let name = spelling.rsplit(['/', '\\']).next().unwrap_or(spelling);
        let Some((_, extension)) = name.rsplit_once('.') else {
            return Self::DeclarativeData;
        };
        if Self::NATIVE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
            Self::NativeLibrary
        } else {
            Self::DeclarativeData
        }
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DeclarativeData => "declarative_data",
            Self::NativeLibrary => "native_library",
        }
    }

    /// Whether the payload is data a bounded reader may parse.
    #[must_use]
    pub const fn is_declarative(self) -> bool {
        matches!(self, Self::DeclarativeData)
    }
}

impl fmt::Display for PayloadKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One file a manifest ships, with a validated mod-root-relative spelling.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModPayload {
    path: RelativePath,
    kind: PayloadKind,
}

impl ModPayload {
    /// Validates the spelling and records the file together with its
    /// classified [`PayloadKind`].
    ///
    /// # Errors
    ///
    /// [`ManifestError::UnsafePath`] when `spelling` is empty, absolute,
    /// drive-prefixed or contains a `.`/`..`/empty component or a NUL byte.
    /// The spelling is kept byte-for-byte for diagnostics; it is joined to a
    /// mod root only by the F53-B mount.
    pub fn new(spelling: &str) -> Result<Self, ManifestError> {
        let path = RelativePath::new(spelling).map_err(|error| ManifestError::UnsafePath {
            spelling: spelling.to_owned(),
            error,
        })?;
        let kind = PayloadKind::from_spelling(path.as_str());
        Ok(Self { path, kind })
    }

    /// The mod-root-relative spelling, as the manifest wrote it.
    pub fn path(&self) -> &RelativePath {
        &self.path
    }

    /// What the extension classified this file to.
    pub fn kind(&self) -> PayloadKind {
        self.kind
    }
}

impl fmt::Display for ModPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.path, self.kind)
    }
}

/// The identity half of a manifest: what the mod calls itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModHeader {
    id: ModId,
    name: String,
    version: ModVersion,
    engine: EngineRange,
    declared_cosmetic_only: bool,
    provenance: Provenance,
}

impl ModHeader {
    /// Validates and builds a header.
    ///
    /// `declared_cosmetic_only` is the author's claim that the mod changes
    /// nothing in the simulation. It is recorded and **checked** against
    /// [`super::overrides::classify_effect`] by the mount plan; a mod that
    /// claims cosmetic while overriding a gameplay kind is refused
    /// (F53 non-negotiable 3).
    ///
    /// # Errors
    ///
    /// [`ManifestError::NameEmpty`], [`ManifestError::NameTooLong`] and
    /// [`ManifestError::NameNotPrintable`] when `name` is not a bounded
    /// printable display string.
    pub fn try_new(
        id: ModId,
        name: &str,
        version: ModVersion,
        engine: EngineRange,
        declared_cosmetic_only: bool,
        provenance: Provenance,
    ) -> Result<Self, ManifestError> {
        if name.is_empty() {
            return Err(ManifestError::NameEmpty { id });
        }
        if name.len() > MAX_MOD_NAME_BYTES {
            return Err(ManifestError::NameTooLong {
                id,
                len: name.len(),
            });
        }
        if let Some(ch) = name.chars().find(|ch| ch.is_control()) {
            return Err(ManifestError::NameNotPrintable { id, ch });
        }
        Ok(Self {
            id,
            name: name.to_owned(),
            version,
            engine,
            declared_cosmetic_only,
            provenance,
        })
    }

    /// The stable mod id, the identity a [`cs_types::asset_id::ModStack`]
    /// opts into.
    pub fn id(&self) -> &ModId {
        &self.id
    }

    /// The display name. Never an identity.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The mod's own version.
    pub fn version(&self) -> ModVersion {
        self.version
    }

    /// The engine window the mod declares.
    pub fn engine(&self) -> EngineRange {
        self.engine
    }

    /// The author's cosmetic-only claim, to be checked by the plan.
    pub fn declared_cosmetic_only(&self) -> bool {
        self.declared_cosmetic_only
    }

    /// Where this manifest document came from.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One complete, validated mod manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModManifest {
    header: ModHeader,
    dependencies: Vec<ModDependency>,
    payloads: Vec<ModPayload>,
    overrides: Vec<ContentOverride>,
}

impl ModManifest {
    /// Validates a manifest and refuses the four structural faults that
    /// cannot be fixed by anything downstream.
    ///
    /// # Errors
    ///
    /// * [`ManifestError::SelfDependency`] — the manifest requires or
    ///   conflicts with itself, which is a cycle of length one.
    /// * [`ManifestError::DuplicateDependency`] — two rows name the same
    ///   dependency, so which requirement wins would depend on row order.
    /// * [`ManifestError::DuplicateOverride`] — two overrides claim one
    ///   target, so which claim wins would depend on row order.
    /// * [`ManifestError::DuplicatePayload`] — the same file is shipped
    ///   twice.
    /// * [`ManifestError::NativePayloadForbidden`] — the manifest ships
    ///   native code (F53 non-negotiable 2).
    pub fn try_new(
        header: ModHeader,
        dependencies: Vec<ModDependency>,
        payloads: Vec<ModPayload>,
        overrides: Vec<ContentOverride>,
    ) -> Result<Self, ManifestError> {
        for dependency in &dependencies {
            if dependency.mod_id == header.id {
                return Err(ManifestError::SelfDependency { id: header.id });
            }
            if dependencies
                .iter()
                .filter(|other| other.mod_id == dependency.mod_id)
                .count()
                > 1
            {
                return Err(ManifestError::DuplicateDependency {
                    id: header.id,
                    on: dependency.mod_id.clone(),
                });
            }
        }
        for override_entry in &overrides {
            if overrides
                .iter()
                .filter(|other| other.target() == override_entry.target())
                .count()
                > 1
            {
                return Err(ManifestError::DuplicateOverride {
                    id: header.id,
                    target: override_entry.target().clone(),
                });
            }
        }
        for payload in &payloads {
            if !payload.kind.is_declarative() {
                return Err(ManifestError::NativePayloadForbidden {
                    id: header.id,
                    path: payload.path.as_str().to_owned(),
                });
            }
            if payloads
                .iter()
                .filter(|other| other.path == payload.path)
                .count()
                > 1
            {
                return Err(ManifestError::DuplicatePayload {
                    id: header.id,
                    path: payload.path.as_str().to_owned(),
                });
            }
        }
        Ok(Self {
            header,
            dependencies,
            payloads,
            overrides,
        })
    }

    /// The manifest's identity half.
    pub fn header(&self) -> &ModHeader {
        &self.header
    }

    /// The stable mod id.
    pub fn id(&self) -> &ModId {
        self.header.id()
    }

    /// The mod's own version.
    pub fn version(&self) -> ModVersion {
        self.header.version()
    }

    /// The engine window the mod declares.
    pub fn engine(&self) -> EngineRange {
        self.header.engine()
    }

    /// The author's cosmetic-only claim.
    pub fn declared_cosmetic_only(&self) -> bool {
        self.header.declared_cosmetic_only()
    }

    /// Where the manifest document came from.
    pub fn provenance(&self) -> &Provenance {
        self.header.provenance()
    }

    /// Every declared dependency, in the order the manifest listed them.
    pub fn dependencies(&self) -> &[ModDependency] {
        &self.dependencies
    }

    /// The declared dependency on `mod_id`, if there is one.
    pub fn dependency(&self, mod_id: &ModId) -> Option<&ModDependency> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.mod_id() == mod_id)
    }

    /// Every shipped file.
    pub fn payloads(&self) -> &[ModPayload] {
        &self.payloads
    }

    /// Every content override, in the order the manifest listed them.
    pub fn overrides(&self) -> &[ContentOverride] {
        &self.overrides
    }

    /// The override claiming `target`, if there is one.
    pub fn override_of(&self, target: &ContentId) -> Option<&ContentOverride> {
        self.overrides.iter().find(|entry| entry.target() == target)
    }

    /// How many content ids the manifest claims.
    pub fn override_count(&self) -> usize {
        self.overrides.len()
    }

    /// The manifest's total declared payload size.
    ///
    /// Saturating on purpose: a manifest that declares an absurd total must
    /// stay over budget, and a wrapped sum would put it back under.
    pub fn declared_bytes(&self) -> u64 {
        self.overrides.iter().fold(0_u64, |total, entry| {
            total.saturating_add(entry.declared_bytes())
        })
    }
}

impl fmt::Display for ModManifest {
    /// `id@version`, the form a report quotes. The display name is
    /// deliberately not here: it is untrusted text and never an identity.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.header.id, self.header.version)
    }
}

/// Why a manifest, or one of its parts, was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The display name was empty.
    NameEmpty {
        /// The mod whose name failed.
        id: ModId,
    },
    /// The display name exceeded [`MAX_MOD_NAME_BYTES`].
    NameTooLong {
        /// The mod whose name failed.
        id: ModId,
        /// Its length in bytes.
        len: usize,
    },
    /// The display name contained a control character, so it could rewrite
    /// a terminal or a log line it is quoted in.
    NameNotPrintable {
        /// The mod whose name failed.
        id: ModId,
        /// The offending character.
        ch: char,
    },
    /// The engine range's upper bound precedes its lower bound.
    EngineRangeInverted {
        /// The declared lower bound.
        min: ModVersion,
        /// The declared upper bound.
        max: ModVersion,
    },
    /// A dependency range's upper bound precedes its lower bound.
    DependencyRangeInverted {
        /// The declared lower bound.
        min: ModVersion,
        /// The declared upper bound.
        max: ModVersion,
    },
    /// The manifest depends on, or declares a conflict with, itself.
    SelfDependency {
        /// The offending mod.
        id: ModId,
    },
    /// Two dependency rows name the same mod.
    DuplicateDependency {
        /// The mod carrying the duplicate.
        id: ModId,
        /// The dependency named twice.
        on: ModId,
    },
    /// Two overrides claim the same content id.
    DuplicateOverride {
        /// The mod carrying the duplicate.
        id: ModId,
        /// The target claimed twice.
        target: ContentId,
    },
    /// The same file is shipped twice.
    DuplicatePayload {
        /// The mod carrying the duplicate.
        id: ModId,
        /// The path shipped twice.
        path: String,
    },
    /// The manifest ships a native library or executable. F53
    /// non-negotiable 2 forbids executing native code from a mod archive,
    /// and this engine has no loader for it.
    NativePayloadForbidden {
        /// The offending mod.
        id: ModId,
        /// The native file it shipped.
        path: String,
    },
    /// A source spelling could escape the mod root or is otherwise not a
    /// safe relative path.
    UnsafePath {
        /// The spelling as written, kept for the report.
        spelling: String,
        /// Why it was refused.
        error: RelativePathError,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameEmpty { id } => write!(f, "mod {id} has an empty display name"),
            Self::NameTooLong { id, len } => write!(
                f,
                "mod {id} display name is {len} bytes, max is {MAX_MOD_NAME_BYTES}"
            ),
            Self::NameNotPrintable { id, ch } => write!(
                f,
                "mod {id} display name contains the control character {ch:?}"
            ),
            Self::EngineRangeInverted { min, max } => {
                write!(f, "engine range {min}..={max} is inverted")
            }
            Self::DependencyRangeInverted { min, max } => {
                write!(f, "dependency range {min}..={max} is inverted")
            }
            Self::SelfDependency { id } => {
                write!(f, "mod {id} declares a dependency on itself")
            }
            Self::DuplicateDependency { id, on } => {
                write!(f, "mod {id} declares a dependency on {on} more than once")
            }
            Self::DuplicateOverride { id, target } => {
                write!(f, "mod {id} overrides {target} more than once")
            }
            Self::DuplicatePayload { id, path } => {
                write!(f, "mod {id} ships {path} more than once")
            }
            Self::NativePayloadForbidden { id, path } => write!(
                f,
                "mod {id} ships the native payload {path}, which this engine never loads"
            ),
            Self::UnsafePath { spelling, error } => {
                write!(f, "mod source path {spelling:?} is unsafe: {error}")
            }
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnsafePath { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::overrides::OverrideAction;
    use crate::mods::synthetic_mod_claim;
    use cs_types::content::ContentKind;
    use cs_types::evidence::ClaimStatus;

    fn designed() -> Provenance {
        Provenance::designed(synthetic_mod_claim())
    }

    fn mod_id(label: &str) -> ModId {
        ModId::new(label).expect("the fixture mod id is valid")
    }

    fn engine_range() -> EngineRange {
        EngineRange::new(ModVersion::new(0, 1, 0), ModVersion::new(0, 9, 9))
            .expect("the fixture engine range is valid")
    }

    fn header(label: &str) -> ModHeader {
        ModHeader::try_new(
            mod_id(label),
            "Synthetic Mod",
            ModVersion::new(1, 2, 0),
            engine_range(),
            true,
            designed(),
        )
        .expect("the fixture header is valid")
    }

    fn image_id(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Image, key).expect("valid image id")
    }

    fn override_of(key: &str, source: &str) -> ContentOverride {
        ContentOverride::try_new(image_id(key), OverrideAction::Replace, source, 1_024)
            .expect("the fixture override is valid")
    }

    /// The typed input is complete, bounded and honestly labelled: the
    /// stable id and version are identity, the display name is not, the
    /// engine window is closed, and the provenance says "designed" — a mod
    /// manifest is newly authored content and can never be reported as
    /// original evidence.
    #[test]
    fn accept_f53_a_manifest_records_identity_version_and_provenance() {
        let manifest = ModManifest::try_new(
            header("synthetic.panel-repaint"),
            vec![ModDependency::new(
                mod_id("synthetic.bright-panels"),
                VersionRange::at_least(ModVersion::new(1, 0, 0)),
                DependencyStrength::Optional,
            )],
            vec![ModPayload::new("art/panel.png").expect("a declarative payload")],
            vec![override_of("synthetic.hull-panel", "art/panel.png")],
        )
        .expect("the fixture manifest is valid");

        assert_eq!(manifest.id().as_str(), "synthetic.panel-repaint");
        assert_eq!(manifest.to_string(), "synthetic.panel-repaint@1.2.0");
        assert_eq!(manifest.header().name(), "Synthetic Mod");
        assert_eq!(manifest.version(), ModVersion::new(1, 2, 0));
        assert_eq!(manifest.version().major(), 1);
        assert_eq!(manifest.version().minor(), 2);
        assert_eq!(manifest.version().patch(), 0);
        assert!(manifest.declared_cosmetic_only());
        assert_eq!(manifest.engine().to_string(), "0.1.0..=0.9.9");
        assert!(manifest.engine().contains(&ModVersion::new(0, 5, 0)));
        assert!(!manifest.engine().contains(&ModVersion::new(1, 0, 0)));
        assert_eq!(manifest.override_count(), 1);
        assert_eq!(manifest.declared_bytes(), 1_024);
        assert_eq!(manifest.payloads().len(), 1);
        assert!(manifest.payloads()[0].kind().is_declarative());
        assert_eq!(manifest.payloads()[0].kind().label(), "declarative_data");

        let dependency = manifest
            .dependency(&mod_id("synthetic.bright-panels"))
            .expect("the fixture declares the dependency");
        assert_eq!(dependency.strength(), DependencyStrength::Optional);
        assert_eq!(dependency.strength().label(), "optional");
        assert!(!dependency.strength().gates_enabling());
        assert_eq!(dependency.range().minimum(), ModVersion::new(1, 0, 0));
        assert_eq!(dependency.range().max_inclusive(), None);
        assert_eq!(
            dependency.to_string(),
            "optional synthetic.bright-panels 1.0.0.."
        );
        assert!(DependencyStrength::Required.gates_enabling());
        assert!(DependencyStrength::Conflict.gates_enabling());
        assert!(manifest.dependency(&mod_id("synthetic.absent")).is_none());
        assert!(
            manifest
                .override_of(&image_id("synthetic.hull-panel"))
                .is_some()
        );
        assert!(manifest.override_of(&image_id("synthetic.other")).is_none());

        // Provenance is designed, never verified_original: this stage reads
        // no installation bytes and can claim nothing about the original
        // game.
        assert_eq!(manifest.provenance().class, ClaimStatus::Designed);
        assert_eq!(
            manifest.provenance().claim_id,
            synthetic_mod_claim(),
            "a manifest names the claim that produced it"
        );
        assert!(manifest.provenance().source.is_none());
        assert_ne!(manifest.provenance().class, ClaimStatus::VerifiedOriginal);
    }

    /// An interval that can never be satisfied is refused at construction
    /// rather than stored, and an engine window must be closed on both ends
    /// so it can actually fail.
    #[test]
    fn accept_f53_a_inverted_ranges_are_refused_at_construction() {
        assert_eq!(
            EngineRange::new(ModVersion::new(2, 0, 0), ModVersion::new(1, 0, 0))
                .expect_err("2.0.0..=1.0.0 can never contain a version"),
            ManifestError::EngineRangeInverted {
                min: ModVersion::new(2, 0, 0),
                max: ModVersion::new(1, 0, 0),
            }
        );
        assert_eq!(
            VersionRange::new(ModVersion::new(3, 0, 0), Some(ModVersion::new(2, 9, 9)))
                .expect_err("3.0.0..=2.9.9 can never contain a version"),
            ManifestError::DependencyRangeInverted {
                min: ModVersion::new(3, 0, 0),
                max: ModVersion::new(2, 9, 9),
            }
        );
        // A single-version window is legal and inclusive at both ends.
        let single = VersionRange::between(ModVersion::new(1, 4, 0), ModVersion::new(1, 4, 0))
            .expect("1.4.0..=1.4.0 is a legal window");
        assert!(single.contains(&ModVersion::new(1, 4, 0)));
        assert!(!single.contains(&ModVersion::new(1, 4, 1)));
        assert_eq!(single.to_string(), "1.4.0..=1.4.0");
        assert!(
            VersionRange::at_least(ModVersion::new(0, 0, 1)).contains(&ModVersion::new(9, 9, 9))
        );
        assert!(
            !VersionRange::at_least(ModVersion::new(1, 0, 0)).contains(&ModVersion::new(0, 9, 9))
        );
    }

    /// F53 AC02's path half, at the manifest boundary: a payload or an
    /// override source that could escape the mod root never becomes a
    /// record, and the refusal says which spelling failed and why.
    #[test]
    fn accept_f53_a_malicious_relative_paths_are_rejected() {
        for hostile in [
            "../outside.png",
            "art/../../outside.png",
            "/etc/passwd",
            "C:/mods/panel.png",
            "./panel.png",
            "art//panel.png",
            "panel.png\u{0}",
            "",
        ] {
            let refused = ModPayload::new(hostile)
                .expect_err("a payload spelling that could escape the mod root");
            match &refused {
                ManifestError::UnsafePath { spelling, error } => {
                    assert_eq!(spelling, hostile);
                    assert!(
                        std::error::Error::source(&refused).is_some(),
                        "the path refusal names the underlying reason"
                    );
                    assert!(!error.to_string().is_empty());
                }
                other => panic!("{hostile:?} was refused as {other:?}"),
            }
        }
        // The same rule applies to an override's source, and the accepted
        // spelling is preserved for diagnostics.
        let good = ModPayload::new("art\\panel.png").expect("a safe payload spelling");
        assert_eq!(good.path().as_str(), "art\\panel.png");
        assert_eq!(good.to_string(), "art\\panel.png (declarative_data)");
    }

    /// F53 non-negotiable 2: a native library is classified so it can be
    /// named in a report, and refused so it can never be mounted. This
    /// engine has no loader for native code from any archive.
    #[test]
    fn accept_f53_a_native_payloads_are_classified_and_refused() {
        for native in [
            "bin/plugin.dll",
            "bin/libhelper.so",
            "bin/libhelper.dylib",
            "bin/game.exe",
            "bin/legacy.com",
            "bin/driver.ocx",
            "bin/driver.sys",
            "bin/MIXED.DLL",
        ] {
            let payload = ModPayload::new(native).expect("the spelling itself is safe");
            assert_eq!(
                payload.kind(),
                PayloadKind::NativeLibrary,
                "{native} is native code by extension"
            );
            assert!(!payload.kind().is_declarative());
            let refused = ModManifest::try_new(
                header("synthetic.native"),
                Vec::new(),
                vec![payload],
                Vec::new(),
            )
            .expect_err("a manifest that ships native code is refused");
            assert_eq!(
                refused,
                ManifestError::NativePayloadForbidden {
                    id: mod_id("synthetic.native"),
                    path: native.to_owned(),
                }
            );
        }
        for declarative in ["manifest.json", "art/panel.png", "README", "a.tar.zst"] {
            assert_eq!(
                ModPayload::new(declarative)
                    .expect("the spelling is safe")
                    .kind(),
                PayloadKind::DeclarativeData,
                "{declarative} is not native code"
            );
        }
        // The extension list is closed and is the whole rule.
        assert_eq!(PayloadKind::NATIVE_EXTENSIONS.len(), 7);
    }

    /// Every structural fault that row order could decide is refused at
    /// construction, so a plan never has to break a tie between two rows of
    /// one manifest.
    #[test]
    fn accept_f53_a_manifest_refuses_self_dependency_and_duplicate_rows() {
        let requires = |target: &str| {
            ModDependency::new(
                mod_id(target),
                VersionRange::at_least(ModVersion::new(0, 0, 1)),
                DependencyStrength::Required,
            )
        };

        assert_eq!(
            ModManifest::try_new(
                header("synthetic.self"),
                vec![requires("synthetic.self")],
                Vec::new(),
                Vec::new(),
            )
            .expect_err("a mod that requires itself is a cycle of length one"),
            ManifestError::SelfDependency {
                id: mod_id("synthetic.self"),
            }
        );
        assert_eq!(
            ModManifest::try_new(
                header("synthetic.self-conflict"),
                vec![ModDependency::new(
                    mod_id("synthetic.self-conflict"),
                    VersionRange::at_least(ModVersion::new(0, 0, 1)),
                    DependencyStrength::Conflict,
                )],
                Vec::new(),
                Vec::new(),
            )
            .expect_err("a mod that conflicts with itself is refused too"),
            ManifestError::SelfDependency {
                id: mod_id("synthetic.self-conflict"),
            }
        );
        assert_eq!(
            ModManifest::try_new(
                header("synthetic.dup-dep"),
                vec![requires("synthetic.other"), requires("synthetic.other"),],
                Vec::new(),
                Vec::new(),
            )
            .expect_err("two rows for one dependency would make the winner row order"),
            ManifestError::DuplicateDependency {
                id: mod_id("synthetic.dup-dep"),
                on: mod_id("synthetic.other"),
            }
        );
        assert_eq!(
            ModManifest::try_new(
                header("synthetic.dup-override"),
                Vec::new(),
                Vec::new(),
                vec![
                    override_of("synthetic.hull-panel", "art/a.png"),
                    override_of("synthetic.hull-panel", "art/b.png"),
                ],
            )
            .expect_err("two claims on one target would make the winner row order"),
            ManifestError::DuplicateOverride {
                id: mod_id("synthetic.dup-override"),
                target: image_id("synthetic.hull-panel"),
            }
        );
        assert_eq!(
            ModManifest::try_new(
                header("synthetic.dup-payload"),
                Vec::new(),
                vec![
                    ModPayload::new("art/a.png").expect("safe"),
                    ModPayload::new("art/a.png").expect("safe"),
                ],
                Vec::new(),
            )
            .expect_err("the same file twice is refused"),
            ManifestError::DuplicatePayload {
                id: mod_id("synthetic.dup-payload"),
                path: "art/a.png".to_owned(),
            }
        );
        // An empty override list is legal: a mod may ship only a dependency
        // pin or only private assets, and the plan reports zero claims.
        let library_only = ModManifest::try_new(
            header("synthetic.library-only"),
            Vec::new(),
            vec![ModPayload::new("docs/readme.txt").expect("safe")],
            Vec::new(),
        )
        .expect("a mod with no overrides is still a manifest");
        assert_eq!(library_only.override_count(), 0);
        assert_eq!(library_only.declared_bytes(), 0);
    }
}
