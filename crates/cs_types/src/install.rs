//! Installation inventory and compatibility-profile schema (F02-A).
//!
//! Typed inputs and outputs of installation discovery: an [`InstallManifest`]
//! holds one [`InstallFileRecord`] per regular source file — relative
//! spelling, byte size, SHA-256, detected family, role and parse state
//! (spec F02, "Deliverable and interfaces") — and [`CompatibilityProfile`]
//! carries the four independent dimensions from `docs/01-ARCHITECTURE.md`
//! ("Compatibility profiles").
//!
//! [`InstallManifest::logical_identity`] is the AC01 logical identity: a
//! canonical encoding of the inventoried data (case-folded relative keys,
//! byte sizes, SHA-256 digests) that deliberately excludes the host root, the
//! letter case of the original spelling and every analysis result, so
//! identical data copied under differently cased host paths keeps one
//! identity.
//!
//! This module defines the records, their validation rules and the identity
//! encoding. Walking a real installation, reading bytes and computing the
//! SHA-256 values carried by [`InstallFileRecord`] arrive with F02-B in
//! `cs_assets` (`discover`, `fingerprint`, `AnalysisCache`); `cs-inspect`
//! report wiring is F02-C work; family labels are emitted by the format
//! tasks (F05/F06/F07) as they recognize layouts. Nothing in this module is
//! derived from original game data.

use std::fmt;
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::evidence::ContentHash;

/// Maximum byte length of a [`FileFamily`] dispatch label.
pub const MAX_FAMILY_LABEL_LEN: usize = 64;

/// Maximum byte length of a [`LocaleLabel`].
pub const MAX_LOCALE_LABEL_LEN: usize = 32;

/// Maximum byte length of a [`DimensionLabel`].
pub const MAX_DIMENSION_LABEL_LEN: usize = 64;

/// Header of the canonical identity encoding produced by
/// [`InstallManifest::logical_identity`]. The `/v1` suffix versions the
/// encoding: a future format change must bump it instead of silently
/// reinterpreting stored identities.
pub const INSTALL_IDENTITY_HEADER: &str = "crimson-skies-install-identity/v1";

/// A file's relative spelling inside an installation, exactly as discovered.
///
/// The original spelling — including letter case — is preserved for display,
/// diagnostics and case-preserving re-opens; [`RelativePath::logical_key`]
/// produces the case-insensitive comparison form used for identity and
/// lookups (spec F02 non-negotiable behavior 2: enumerate case-insensitively
/// while preserving originals).
///
/// Validation rejects anything that could escape an installation root:
/// absolute spellings, drive letters, `.`/`..` components, empty components
/// and interior NUL bytes. No unchecked path join is possible with an
/// unvalidated spelling (IDENTITY-CONTENT contract).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelativePath(String);

/// Why a relative spelling was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelativePathError {
    /// The spelling was empty.
    Empty,
    /// The spelling was absolute: a leading separator or a drive letter.
    Absolute,
    /// The spelling contained a `..` component.
    ParentComponent,
    /// The spelling contained a `.` component.
    CurrentComponent,
    /// The spelling contained an empty component (`//`, trailing separator).
    EmptyComponent,
    /// The spelling contained an interior NUL byte.
    InteriorNul,
}

impl fmt::Display for RelativePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "relative spelling must not be empty"),
            Self::Absolute => write!(f, "relative spelling must not be absolute"),
            Self::ParentComponent => {
                write!(f, "relative spelling must not contain a `..` component")
            }
            Self::CurrentComponent => {
                write!(f, "relative spelling must not contain a `.` component")
            }
            Self::EmptyComponent => write!(f, "relative spelling has an empty component"),
            Self::InteriorNul => write!(f, "relative spelling contains a NUL byte"),
        }
    }
}

impl std::error::Error for RelativePathError {}

impl RelativePath {
    /// Validates and wraps a relative spelling.
    ///
    /// Both `/` and `\` separate components, so a spelling recorded on one
    /// host type stays valid when the same installation is examined on the
    /// other; the spelling itself is stored byte-for-byte as given.
    pub fn new(spelling: &str) -> Result<Self, RelativePathError> {
        if spelling.is_empty() {
            return Err(RelativePathError::Empty);
        }
        if spelling.contains('\0') {
            return Err(RelativePathError::InteriorNul);
        }
        let bytes = spelling.as_bytes();
        if bytes[0] == b'/' || bytes[0] == b'\\' {
            return Err(RelativePathError::Absolute);
        }
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(RelativePathError::Absolute);
        }
        for component in spelling.split(['/', '\\']) {
            match component {
                "" => return Err(RelativePathError::EmptyComponent),
                "." => return Err(RelativePathError::CurrentComponent),
                ".." => return Err(RelativePathError::ParentComponent),
                _ => {}
            }
        }
        Ok(Self(spelling.to_owned()))
    }

    /// The original relative spelling, unchanged.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The case-insensitive comparison key: components joined with `/`,
    /// ASCII-lowercased.
    ///
    /// The letter case of the spelling never reaches this key, which is what
    /// makes the logical identity of a case-cased copy of an installation
    /// stable (F02 AC01); the original spelling stays available through
    /// [`RelativePath::as_str`].
    pub fn logical_key(&self) -> String {
        self.0
            .split(['/', '\\'])
            .map(str::to_ascii_lowercase)
            .collect::<Vec<String>>()
            .join("/")
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A detected file-family dispatch label.
///
/// Families are an open vocabulary: format tasks (F05 ROF, F06 ZBD families,
/// F07 interp, ...) emit validated lowercase labels such as `rof` or
/// `zbd.sound` as they recognize layouts. This pack lists no exhaustive
/// family set, because none was observed here; an undetected family is
/// `None` on the record, never a guessed variant (spec F02 "Research
/// boundary").
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileFamily(String);

/// Why a file-family label was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FamilyError {
    /// The label was empty.
    Empty,
    /// The label exceeded [`MAX_FAMILY_LABEL_LEN`] bytes.
    TooLong { len: usize },
    /// The first character was not a lowercase ASCII alphanumeric.
    BadFirst { ch: char },
    /// A character was outside `[a-z0-9._-]`.
    BadCharacter { ch: char },
}

impl fmt::Display for FamilyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "file family label must not be empty"),
            Self::TooLong { len } => {
                write!(
                    f,
                    "file family label is {len} bytes, max is {MAX_FAMILY_LABEL_LEN}"
                )
            }
            Self::BadFirst { ch } => write!(
                f,
                "file family label must start with a lowercase letter or digit, got {ch:?}"
            ),
            Self::BadCharacter { ch } => {
                write!(f, "file family label contains disallowed character {ch:?}")
            }
        }
    }
}

impl std::error::Error for FamilyError {}

impl FileFamily {
    /// Validates and wraps a family dispatch label.
    pub fn new(label: &str) -> Result<Self, FamilyError> {
        if label.is_empty() {
            return Err(FamilyError::Empty);
        }
        if label.len() > MAX_FAMILY_LABEL_LEN {
            return Err(FamilyError::TooLong { len: label.len() });
        }
        let mut chars = label.chars();
        let first = chars.next().expect("label is not empty");
        if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
            return Err(FamilyError::BadFirst { ch: first });
        }
        for ch in chars {
            if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && !matches!(ch, '.' | '_' | '-') {
                return Err(FamilyError::BadCharacter { ch });
            }
        }
        Ok(Self(label.to_owned()))
    }

    /// The label as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FileFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a file participates in the reimplementation (spec F02 non-negotiable
/// behavior 4: every file is classified as consumed, needed-unimplemented,
/// optional-media, unused-with-reason, platform-support or unknown).
///
/// `Unknown` is an explicit classification: an unknown gameplay dependency
/// fails completeness, and unknown never means unused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRole {
    /// Read and consumed by supported original content.
    Consumed,
    /// Needed by original content whose implementation is not done yet.
    NeededUnimplemented,
    /// Optional media whose absence does not block gameplay content.
    OptionalMedia,
    /// Present but unused; the reason is mandatory (an unused
    /// classification without a reason is rejected).
    UnusedWithReason(String),
    /// Platform/support content (installers, DLLs and similar) that is not
    /// gameplay content.
    PlatformSupport,
    /// Not classified yet. Never treated as unused.
    Unknown,
}

/// How the record's bytes were parsed.
///
/// Every inventoried file keeps a row regardless of parse outcome: an opaque
/// unparsed member and a failed parse are still inventory rows (IDENTITY-
/// CONTENT contract, "Collections cannot exclude failed entries").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseState {
    /// Not parsed yet (deliberately or not).
    Unparsed,
    /// Parsed by a format reader.
    Parsed,
    /// A parse was attempted and failed; the diagnostic is mandatory.
    Failed { diagnostic: String },
}

/// One regular source file in an installation inventory.
///
/// This is the F02 deliverable row: relative spelling, byte size, SHA-256,
/// detected family, role and parse state. Hashes are canonical lowercase
/// hexadecimal through [`ContentHash`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallFileRecord {
    /// The relative spelling exactly as discovered (case preserved).
    pub relative_spelling: RelativePath,
    /// The file's size in bytes as inventoried.
    pub size_bytes: u64,
    /// SHA-256 of the file's bytes.
    pub sha256: ContentHash,
    /// The detected family, or `None` when none was detected.
    pub family: Option<FileFamily>,
    /// The file's classification.
    pub role: FileRole,
    /// The parse outcome recorded for the file.
    pub parse_state: ParseState,
}

/// Why an inventory record or manifest was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The host root was an empty path.
    EmptyRoot,
    /// A discovered path is not under the host root (or is the root
    /// itself's parent-side mismatch). Such a file is an error, never a
    /// silent omission: the inventory contains every regular source file.
    NotUnderRoot { path: PathBuf },
    /// A discovered path equals the host root, leaving no relative
    /// spelling.
    EmptyRelative { path: PathBuf },
    /// A discovered path component is not valid UTF-8.
    NonUtf8Path { path: PathBuf },
    /// A relative spelling failed validation.
    RelativePath {
        path: PathBuf,
        error: RelativePathError,
    },
    /// Two rows share one case-insensitive logical key: such rows could not
    /// be looked up or re-opened unambiguously.
    DuplicateLogicalKey { key: String },
    /// An `unused` classification carried no reason.
    EmptyRoleReason,
    /// A failed parse state carried no diagnostic.
    EmptyParseDiagnostic,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRoot => write!(f, "host root must not be an empty path"),
            Self::NotUnderRoot { path } => {
                write!(f, "{} is not under the host root", path.display())
            }
            Self::EmptyRelative { path } => {
                write!(f, "{} leaves no relative spelling", path.display())
            }
            Self::NonUtf8Path { path } => {
                write!(
                    f,
                    "{} has a component that is not valid UTF-8",
                    path.display()
                )
            }
            Self::RelativePath { path, error } => {
                write!(
                    f,
                    "{} has an invalid relative spelling: {error}",
                    path.display()
                )
            }
            Self::DuplicateLogicalKey { key } => {
                write!(f, "duplicate case-insensitive key {key:?} in the inventory")
            }
            Self::EmptyRoleReason => {
                write!(f, "an unused classification must carry a reason")
            }
            Self::EmptyParseDiagnostic => {
                write!(f, "a failed parse state must carry a diagnostic")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

impl InstallFileRecord {
    /// The per-record inventory rules (F02-A): classifications that promise
    /// text must carry non-empty text. Row uniqueness across the manifest is
    /// a set-level rule checked by [`InstallManifest::new`].
    pub fn validate(&self) -> Result<(), ManifestError> {
        if let FileRole::UnusedWithReason(reason) = &self.role
            && reason.trim().is_empty()
        {
            return Err(ManifestError::EmptyRoleReason);
        }
        if let ParseState::Failed { diagnostic } = &self.parse_state
            && diagnostic.trim().is_empty()
        {
            return Err(ManifestError::EmptyParseDiagnostic);
        }
        Ok(())
    }
}

/// The logical identity of an installation inventory (F02 AC01).
///
/// Its canonical string form is what [`InstallManifest::logical_identity`]
/// produces; equality of identities is equality of the inventoried data
/// under case-folded keys. The form is versioned by
/// [`INSTALL_IDENTITY_HEADER`] so stored identities stay interpretable.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstallIdentity(String);

impl InstallIdentity {
    /// The canonical encoding as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The canonical encoding as bytes, ready to be digested.
    pub fn canonical_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Display for InstallIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The inventory of every regular source file of one installation.
///
/// Built through [`InstallManifest::new`] (or, from discovered host paths,
/// through `cs_assets::install::inventory`), which enforce the set-level
/// rules. The host root is recorded for diagnostics and re-opens only: it is
/// deliberately absent from [`InstallManifest::logical_identity`], so the
/// same data reached through a differently cased host path keeps one
/// identity (F02 AC01).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallManifest {
    /// The host root the inventory was taken under (`CS_GAME_DIR` or
    /// `--cs-path`). Never part of the logical identity.
    pub host_root: PathBuf,
    /// One row per inventoried regular file, in discovery order. The
    /// collection excludes nothing: unknown, unparsed and failed rows stay.
    pub files: Vec<InstallFileRecord>,
}

impl InstallManifest {
    /// Validates rows and set-level rules, then wraps them into a manifest.
    ///
    /// Every row must pass [`InstallFileRecord::validate`], and two rows may
    /// not share one case-insensitive logical key — the installation trees
    /// this engine accepts are case-insensitively unique, so a collision
    /// would make lookups ambiguous.
    pub fn new(host_root: PathBuf, files: Vec<InstallFileRecord>) -> Result<Self, ManifestError> {
        if host_root.as_os_str().is_empty() {
            return Err(ManifestError::EmptyRoot);
        }
        let mut seen: Vec<String> = Vec::with_capacity(files.len());
        for file in &files {
            file.validate()?;
            let key = file.relative_spelling.logical_key();
            if seen.contains(&key) {
                return Err(ManifestError::DuplicateLogicalKey { key });
            }
            seen.push(key);
        }
        Ok(Self { host_root, files })
    }

    /// The logical identity of the inventoried data (F02 AC01).
    ///
    /// Rows are encoded sorted by case-folded logical key, each as
    /// `length:key|size|sha256`, preceded by
    /// [`INSTALL_IDENTITY_HEADER`]. The encoding is injective (the key
    /// length disambiguates any separator bytes inside a key) and excludes:
    ///
    /// * the host root and the input order of the rows,
    /// * the letter case and separator style of the original spellings,
    /// * every analysis result (family, role, parse state) — those describe
    ///   the engine's knowledge, not the installation's data.
    ///
    /// Identical data copied under differently cased host paths therefore
    /// yields byte-identical identities, while a one-byte content change
    /// changes them (the row's SHA-256 is part of the encoding).
    pub fn logical_identity(&self) -> InstallIdentity {
        let mut rows: Vec<(String, &InstallFileRecord)> = self
            .files
            .iter()
            .map(|file| (file.relative_spelling.logical_key(), file))
            .collect();
        rows.sort_by(|left, right| left.0.cmp(&right.0));

        let mut canonical = String::from(INSTALL_IDENTITY_HEADER);
        canonical.push('\n');
        for (key, file) in rows {
            writeln!(
                canonical,
                "{}:{}|{}|{}",
                key.len(),
                key,
                file.size_bytes,
                file.sha256
            )
            .expect("writing to a String never fails");
        }
        InstallIdentity(canonical)
    }
}

/// One recognized installation class (spec F02 non-negotiable behavior 1:
/// full, partial, patched, localized and demo-like installations are
/// recognized without conflating them).
///
/// Classes are tracked as a set, not a single label: an installation can be
/// patched *and* localized at the same time, and collapsing that into one
/// variant would conflate the facts the spec keeps apart. Which evidence
/// makes an installation carry a class is F02-B/F02-D discovery work; this
/// schema records the outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InstallationClass {
    /// Meets the full expected content set.
    Full,
    /// Missing expected content (never passes full-content readiness).
    Partial,
    /// Carries an applied patch.
    Patched,
    /// Localized distribution.
    Localized,
    /// Demo-like, trimmed distribution.
    DemoLike,
}

impl InstallationClass {
    /// The spec-vocabulary name of the class.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::Patched => "patched",
            Self::Localized => "localized",
            Self::DemoLike => "demo-like",
        }
    }
}

/// Why a compatibility-profile value was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// The locale label was empty or all whitespace.
    EmptyLocale,
    /// The locale label exceeded [`MAX_LOCALE_LABEL_LEN`] bytes.
    LocaleTooLong { len: usize },
    /// A `modified` rule-compatibility value carried no note.
    EmptyModifiedNote,
    /// A dimension label was empty or all whitespace.
    EmptyDimensionLabel,
    /// A dimension label exceeded [`MAX_DIMENSION_LABEL_LEN`] bytes.
    DimensionTooLong { len: usize },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyLocale => write!(f, "locale label must not be empty"),
            Self::LocaleTooLong { len } => {
                write!(
                    f,
                    "locale label is {len} bytes, max is {MAX_LOCALE_LABEL_LEN}"
                )
            }
            Self::EmptyModifiedNote => {
                write!(f, "a modified ruleset must carry a note")
            }
            Self::EmptyDimensionLabel => {
                write!(f, "a profile dimension label must not be empty")
            }
            Self::DimensionTooLong { len } => {
                write!(
                    f,
                    "profile dimension label is {len} bytes, max is {MAX_DIMENSION_LABEL_LEN}"
                )
            }
        }
    }
}

impl std::error::Error for ProfileError {}

/// An observed installation locale label.
///
/// The label is opaque on purpose: no locale list was observed here, so the
/// value is whatever discovery later evidences, and `None` on the profile
/// stays "unknown" rather than defaulting to a guessed locale (spec F02
/// "Research boundary").
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocaleLabel(String);

impl LocaleLabel {
    /// Validates and wraps a locale label.
    pub fn new(label: &str) -> Result<Self, ProfileError> {
        if label.trim().is_empty() {
            return Err(ProfileError::EmptyLocale);
        }
        if label.len() > MAX_LOCALE_LABEL_LEN {
            return Err(ProfileError::LocaleTooLong { len: label.len() });
        }
        Ok(Self(label.to_owned()))
    }

    /// The label as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LocaleLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A label for a compatibility dimension whose concrete schema belongs to a
/// later feature (presentation settings: F17/F52; gameplay assists and mods:
/// F24/F52).
///
/// `None` on the owning field means "not recorded" — it never reads as
/// "stock" or "original", so a modded or enhanced configuration cannot be
/// mistaken for the original one (docs/01-ARCHITECTURE.md, "Compatibility
/// profiles").
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DimensionLabel(String);

impl DimensionLabel {
    /// Validates and wraps a dimension label.
    pub fn new(label: &str) -> Result<Self, ProfileError> {
        if label.trim().is_empty() {
            return Err(ProfileError::EmptyDimensionLabel);
        }
        if label.len() > MAX_DIMENSION_LABEL_LEN {
            return Err(ProfileError::DimensionTooLong { len: label.len() });
        }
        Ok(Self(label.to_owned()))
    }

    /// The label as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DimensionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The original-rule compatibility dimension of a compatibility profile.
///
/// A modified ruleset can never be mistaken for a stock one: they are
/// distinct variants, `Modified` requires a note, and "unknown" is its own
/// explicit state instead of a silent default to stock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleCompatibility {
    /// The stock original ruleset.
    Stock,
    /// A modified ruleset; the note says what changed.
    Modified { note: String },
    /// Not determined yet.
    Unknown,
}

impl RuleCompatibility {
    /// Builds a `Modified` value, requiring a non-empty note.
    pub fn modified(note: &str) -> Result<Self, ProfileError> {
        if note.trim().is_empty() {
            return Err(ProfileError::EmptyModifiedNote);
        }
        Ok(Self::Modified {
            note: note.to_owned(),
        })
    }
}

/// The installation-edition dimension of a compatibility profile: which
/// installation this profile describes (by logical identity), which classes
/// it carries and which locale it was observed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationEdition {
    /// The logical identity of the installation this profile describes: a
    /// profile never floats free of the actual installation.
    pub identity: InstallIdentity,
    /// The recognized classes, canonicalized (sorted, deduplicated) so two
    /// profiles naming the same classes compare equal regardless of input
    /// order.
    pub classes: Vec<InstallationClass>,
    /// The observed locale, or `None` while unknown.
    pub locale: Option<LocaleLabel>,
}

impl InstallationEdition {
    /// Builds the edition dimension, canonicalizing the class set.
    pub fn new(
        identity: InstallIdentity,
        classes: impl IntoIterator<Item = InstallationClass>,
        locale: Option<LocaleLabel>,
    ) -> Self {
        let mut classes: Vec<InstallationClass> = classes.into_iter().collect();
        classes.sort();
        classes.dedup();
        Self {
            identity,
            classes,
            locale,
        }
    }
}

/// A compatibility profile: the four independent dimensions of
/// docs/01-ARCHITECTURE.md ("Compatibility profiles").
///
/// The dimensions never collapse into one label: presentation can look
/// original while the rules are modified, and an unknown value stays
/// explicitly unknown. Every replay, network handshake and evidence record
/// will capture these dimensions once their later features fill them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityProfile {
    /// Installation edition/locale, tied to an installation identity.
    pub installation: InstallationEdition,
    /// Original-rule compatibility.
    pub rules: RuleCompatibility,
    /// Presentation settings dimension; `None` while not recorded
    /// (schema contents: F17/F52).
    pub presentation: Option<DimensionLabel>,
    /// Gameplay assists/mods dimension; `None` while not recorded
    /// (schema contents: F24/F52).
    pub assists_mods: Option<DimensionLabel>,
}

impl CompatibilityProfile {
    /// The per-profile admission rules (F02-A).
    ///
    /// Constructors validate labels eagerly; this check catches values built
    /// through the public enum variants directly — the first violation found
    /// is returned and any non-`Ok` result refuses the profile.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if let RuleCompatibility::Modified { note } = &self.rules
            && note.trim().is_empty()
        {
            return Err(ProfileError::EmptyModifiedNote);
        }
        Ok(())
    }
}
