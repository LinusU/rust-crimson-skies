//! Release contents and user-data directory policy (F61-A).
//!
//! Spec: `specs/F61-distribution-installation-ux-notices-and-release-artifacts.md`
//! (deliverable and interfaces, non-negotiable 1–4). This module is the
//! **contract** half of F61: the typed description of what a candidate release
//! archive may contain, the scan that refuses one that carries proprietary
//! content or drops a required notice, and the user-data directory policy the
//! first launch of F61-C resolves.
//!
//! Two things are deliberately *not* here, and both are recorded in
//! `docs/findings/2026-10-02-f61-a-release-contents-and-user-data-policy.md`:
//!
//! * **Content is not proven by a name.** [`scan`] classifies a member from the
//!   only thing a packaging step knows before the archive is built — its path
//!   and size — plus the release [`ReleasePolicy`]. A member named
//!   `textures/plane00.dds` is refused because a redistributable archive
//!   carries no game media, not because this module opened it and recognised
//!   Crimson Skies. F61-B adds the strong check: compare every member's digest
//!   against the recorded original-content digest set.
//! * **No original extensions are guessed.** [`PROPRIETARY_SUFFIXES`] lists
//!   publicly known media, font, executable, document and archive formats that a
//!   redistributable archive never carries. It says nothing about what the
//!   original game called its own files, which is unknown here.
//!   [`ORIGINAL_CONTENT_ROOTS`] is the name-based second rule, and it is
//!   deliberately blunt: any member below a path component naming an
//!   original-content root is refused whatever its suffix says.
//!
//! A source hash manifest is **not** an asset bundle (non-negotiable 1), so
//! [`MemberClass::HashManifest`] is accepted content and never a
//! [`ProprietaryKind`]: a release may state which original content it was
//! verified against without carrying any of it.
//!
//! The user-data half ([`UserDataPolicy`]) answers the open item F48-A left
//! ("User-data base directory choice: F61", `docs/findings/2026-10-01-f48-a-
//! profile-and-save-schema.md`): the base is derived from the platform's
//! per-user data location and from nothing else — not the working directory,
//! not the executable's directory — so a release build behaves identically when
//! it is started from anywhere, and [`UserDataPolicy::check`] refuses a base
//! inside the original installation or next to the executable.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::transient;

// ---------------------------------------------------------------------------
// Release contents
// ---------------------------------------------------------------------------

/// A document every release archive must carry.
///
/// The list is the spec's, sentence by sentence: the deliverable requires
/// "versioned compatibility reports, licenses and user instructions";
/// non-negotiable 2 requires third-party notices *and* reference-tool
/// licensing decisions "recorded separately from new-engine code"; and
/// non-negotiable 3's "not official Crimson Skies, Microsoft or Zipper
/// software" statement is the provenance notice. A release that drops one of
/// them is refused rather than shipped with the gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoticeKind {
    /// The new engine code's own license.
    License,
    /// Transitive dependency licensing notices.
    ThirdPartyNotices,
    /// Licensing decisions for reference tools, kept separate from the
    /// engine's own license (non-negotiable 2).
    ReferenceToolLicensing,
    /// Provenance and non-affiliation statement (non-negotiable 3).
    Provenance,
    /// The versioned compatibility report.
    CompatibilityReport,
    /// User instructions.
    UserInstructions,
}

impl NoticeKind {
    /// Every required notice, in a stable order so reports are deterministic.
    pub const ALL: [Self; 6] = [
        Self::License,
        Self::ThirdPartyNotices,
        Self::ReferenceToolLicensing,
        Self::Provenance,
        Self::CompatibilityReport,
        Self::UserInstructions,
    ];

    /// A short name for reports and error text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::License => "license",
            Self::ThirdPartyNotices => "third-party notices",
            Self::ReferenceToolLicensing => "reference-tool licensing",
            Self::Provenance => "provenance",
            Self::CompatibilityReport => "compatibility report",
            Self::UserInstructions => "user instructions",
        }
    }
}

impl fmt::Display for NoticeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The required notices and the member path each one occupies at the root of a
/// release archive.
///
/// Paths are compared case-insensitively (see [`classify`]) because a release
/// built on a case-insensitive filesystem must not depend on the case a
/// packaging step happened to use.
pub const REQUIRED_NOTICES: [(NoticeKind, &str); 6] = [
    (NoticeKind::License, "LICENSE"),
    (NoticeKind::ThirdPartyNotices, "THIRD-PARTY-NOTICES.md"),
    (NoticeKind::ReferenceToolLicensing, "REFERENCE-TOOLS.md"),
    (NoticeKind::Provenance, "NOTICE.md"),
    (NoticeKind::CompatibilityReport, "COMPATIBILITY.md"),
    (NoticeKind::UserInstructions, "README.md"),
];

/// The engine executables a release archive may carry, without a proprietary
/// finding.
///
/// The new engine *is* an executable, so an `Executable` suffix on its own
/// cannot be the rule; the executable has to be one of these names. Every other
/// executable, DLL or installer in the archive is refused. This is a policy
/// decision about what this project ships, not a claim about the original game.
pub const ENGINE_BINARIES: [&str; 2] = ["crimson-skies", "crimson-skies.exe"];

/// Suffixes of a source hash manifest: hashes naming original content, none of
/// the content itself.
///
/// A `.sha256` file whose lines are digests is what non-negotiable 1 explicitly
/// allows a release to carry, and it is classified before the media suffixes so
/// a file named after original content is still recognised as a manifest.
pub const HASH_MANIFEST_SUFFIXES: [&str; 5] = [".sha256", ".sha512", ".sha1", ".md5", ".sha256sum"];

/// Whole file names that are hash manifests despite a documentation suffix.
pub const HASH_MANIFEST_NAMES: [&str; 2] = ["SHA256SUMS", "SHA256SUMS.txt"];

/// Suffixes of newly authored documentation shipped with a release.
pub const DOCUMENTATION_SUFFIXES: [&str; 3] = [".md", ".txt", ".html"];

/// The classes of proprietary content a redistributable archive may not carry
/// (non-negotiable 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProprietaryKind {
    /// Game textures.
    Texture,
    /// Game audio.
    Audio,
    /// Extracted game scripts.
    Script,
    /// Commercial fonts.
    Font,
    /// Original executables, DLLs and installers.
    Executable,
    /// Scanned manual pages and the manual itself.
    ManualScan,
    /// Extracted archive members.
    ExtractedArchive,
    /// Decompiled source.
    DecompiledSource,
    /// Anything under a path component that names an original-content root,
    /// whatever its suffix says.
    OriginalContentTree,
}

impl ProprietaryKind {
    /// Every class, so a table can be walked in a stable order.
    pub const ALL: [Self; 9] = [
        Self::Texture,
        Self::Audio,
        Self::Script,
        Self::Font,
        Self::Executable,
        Self::ManualScan,
        Self::ExtractedArchive,
        Self::DecompiledSource,
        Self::OriginalContentTree,
    ];

    /// A short name for reports and error text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Texture => "texture",
            Self::Audio => "audio",
            Self::Script => "script",
            Self::Font => "font",
            Self::Executable => "executable",
            Self::ManualScan => "manual scan",
            Self::ExtractedArchive => "extracted archive",
            Self::DecompiledSource => "decompiled source",
            Self::OriginalContentTree => "original-content tree",
        }
    }
}

impl fmt::Display for ProprietaryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Suffixes a redistributable archive may never contain, with the class of
/// proprietary content each one names.
///
/// This is a policy table, not a format table: `.dds` and `.tga` are here
/// because a redistributable archive carries no raster game media of any kind,
/// not because the original game is known to use them. `.png` and `.svg` are
/// absent for the same reason they would be refused if present — nothing in a
/// release needs artwork, and F61-B can add a case deliberately rather than an
/// agent widening this list by habit.
pub const PROPRIETARY_SUFFIXES: &[(&str, ProprietaryKind)] = &[
    (".tex", ProprietaryKind::Texture),
    (".dds", ProprietaryKind::Texture),
    (".tga", ProprietaryKind::Texture),
    (".bmp", ProprietaryKind::Texture),
    (".pcx", ProprietaryKind::Texture),
    (".jpg", ProprietaryKind::Texture),
    (".jpeg", ProprietaryKind::Texture),
    (".png", ProprietaryKind::Texture),
    (".gif", ProprietaryKind::Texture),
    (".tif", ProprietaryKind::Texture),
    (".tiff", ProprietaryKind::Texture),
    (".psd", ProprietaryKind::Texture),
    (".wav", ProprietaryKind::Audio),
    (".ogg", ProprietaryKind::Audio),
    (".mp3", ProprietaryKind::Audio),
    (".mid", ProprietaryKind::Audio),
    (".midi", ProprietaryKind::Audio),
    (".flac", ProprietaryKind::Audio),
    (".xmi", ProprietaryKind::Audio),
    (".it", ProprietaryKind::Audio),
    (".mod", ProprietaryKind::Audio),
    (".s3m", ProprietaryKind::Audio),
    (".lua", ProprietaryKind::Script),
    (".gam", ProprietaryKind::Script),
    (".ttf", ProprietaryKind::Font),
    (".otf", ProprietaryKind::Font),
    (".fon", ProprietaryKind::Font),
    (".fnt", ProprietaryKind::Font),
    (".bdf", ProprietaryKind::Font),
    (".pfb", ProprietaryKind::Font),
    (".exe", ProprietaryKind::Executable),
    (".dll", ProprietaryKind::Executable),
    (".com", ProprietaryKind::Executable),
    (".bat", ProprietaryKind::Executable),
    (".cmd", ProprietaryKind::Executable),
    (".msi", ProprietaryKind::Executable),
    (".scr", ProprietaryKind::Executable),
    (".sys", ProprietaryKind::Executable),
    (".ocx", ProprietaryKind::Executable),
    (".pdf", ProprietaryKind::ManualScan),
    (".djvu", ProprietaryKind::ManualScan),
    (".chm", ProprietaryKind::ManualScan),
    (".zip", ProprietaryKind::ExtractedArchive),
    (".cab", ProprietaryKind::ExtractedArchive),
    (".rar", ProprietaryKind::ExtractedArchive),
    (".7z", ProprietaryKind::ExtractedArchive),
    (".tar", ProprietaryKind::ExtractedArchive),
    (".gz", ProprietaryKind::ExtractedArchive),
    (".bz2", ProprietaryKind::ExtractedArchive),
    (".xz", ProprietaryKind::ExtractedArchive),
    (".iso", ProprietaryKind::ExtractedArchive),
    (".asm", ProprietaryKind::DecompiledSource),
    (".c", ProprietaryKind::DecompiledSource),
    (".cpp", ProprietaryKind::DecompiledSource),
    (".h", ProprietaryKind::DecompiledSource),
    (".hpp", ProprietaryKind::DecompiledSource),
    (".map", ProprietaryKind::DecompiledSource),
    (".pdb", ProprietaryKind::DecompiledSource),
];

/// Path components that name an original-content root.
///
/// A member below one of these is refused whatever its suffix says, which is
/// what catches extracted archives, decompiled sources and any format this
/// table has never heard of. The names are this project's own convention for a
/// release staging tree, not a claim about the original installation.
pub const ORIGINAL_CONTENT_ROOTS: [&str; 4] = [
    "original",
    "original-data",
    "crimson-skies-data",
    "extracted",
];

/// What a release archive member is, under the release policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberClass {
    /// The new engine executable (see [`ENGINE_BINARIES`]).
    EngineBinary,
    /// A required notice.
    Notice(NoticeKind),
    /// Newly authored documentation.
    Documentation,
    /// A source hash manifest: digests of original content, no content.
    HashManifest,
    /// Proprietary content, which a release may not carry.
    Proprietary(ProprietaryKind),
    /// Nothing in the policy claims it, so it must be classified deliberately.
    Unclassified,
}

impl MemberClass {
    /// Whether a member of this class may ship.
    pub const fn is_allowed(self) -> bool {
        !matches!(self, Self::Proprietary(_) | Self::Unclassified)
    }
}

/// Classifies one archive member from its path.
///
/// Order matters and each step is there for a reason:
///
/// 1. an original-content root wins over everything, so a hash manifest cannot
///    launder proprietary content by sitting in `original/`;
/// 2. the engine executable is matched by name, before the `Executable` suffix
///    rule that would otherwise refuse the engine this project ships;
/// 3. a required notice is matched by its exact root path, so `README.md`
///    counts as user instructions rather than as generic documentation;
/// 4. a hash manifest is matched next, because its names are frequently
///    original-looking (non-negotiable 1's explicit exception);
/// 5. the media/font/executable/archive/source suffixes;
/// 6. documentation suffixes;
/// 7. anything else is [`MemberClass::Unclassified`] and refused — a gate that
///    passes what it does not understand is not a gate.
///
/// Comparison is ASCII-case-insensitive, and a backslash is a separator, so the
/// answer does not depend on how the archive spelled its members. Without that
/// rule `docs\crimson-skies` would classify as the engine while the very same
/// path spelled `docs/crimson-skies` is refused, and
/// `original\data\plane00.dat` would lose the original-content-root rule and
/// fall through to `Unclassified`. `unsafe_path` already reads `\` as a
/// separator, and the two must not disagree about what a member path is.
#[must_use]
pub fn classify(path: &str) -> MemberClass {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let components: Vec<String> = lower.split('/').map(str::to_owned).collect();
    let file_name = components.last().map_or("", String::as_str);

    if components
        .iter()
        .any(|component| ORIGINAL_CONTENT_ROOTS.contains(&component.as_str()))
    {
        return MemberClass::Proprietary(ProprietaryKind::OriginalContentTree);
    }

    if ENGINE_BINARIES.contains(&file_name) && in_engine_directory(&lower) {
        return MemberClass::EngineBinary;
    }

    for (notice, member) in REQUIRED_NOTICES {
        if member.eq_ignore_ascii_case(&lower) {
            return MemberClass::Notice(notice);
        }
    }

    if HASH_MANIFEST_NAMES
        .iter()
        .any(|name| name.eq_ignore_ascii_case(file_name))
        || suffix_is(file_name, &HASH_MANIFEST_SUFFIXES)
    {
        return MemberClass::HashManifest;
    }

    if let Some((_, kind)) = PROPRIETARY_SUFFIXES
        .iter()
        .find(|(suffix, _)| lower.ends_with(suffix))
    {
        return MemberClass::Proprietary(*kind);
    }

    if suffix_is(file_name, &DOCUMENTATION_SUFFIXES) {
        return MemberClass::Documentation;
    }

    MemberClass::Unclassified
}

/// Whether `name` ends in one of `suffixes`, ASCII-case-insensitively.
fn suffix_is(name: &str, suffixes: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    suffixes.iter().any(|suffix| lower.ends_with(suffix))
}

/// Whether the engine executable may sit at this member's path.
///
/// Only the archive root and `bin/` count, and the restriction is what stops a
/// documentation file that happens to be *named* after the engine —
/// `docs/crimson-skies` — from standing in for the executable and letting a
/// release satisfy "ships an engine" while shipping none.
fn in_engine_directory(lower_path: &str) -> bool {
    match lower_path.rsplit_once('/') {
        None => true,
        Some((directory, _)) => directory.is_empty() || directory == "bin",
    }
}

/// One member of a candidate release archive, as a packaging step knows it
/// before the archive exists.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageMember {
    /// Archive-relative path, `/`-separated.
    pub path: String,
    /// Uncompressed size in bytes.
    pub size_bytes: u64,
}

impl PackageMember {
    /// A member with the given path and size.
    pub fn new(path: impl Into<String>, size_bytes: u64) -> Self {
        Self {
            path: path.into(),
            size_bytes,
        }
    }

    /// What the release policy makes of this member.
    pub fn class(&self) -> MemberClass {
        classify(&self.path)
    }
}

/// A candidate release: its version and the members an archive would hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidatePackage {
    /// The release version the manifest declares.
    pub version: String,
    /// The members, in manifest order.
    pub members: Vec<PackageMember>,
}

impl CandidatePackage {
    /// A candidate with the given version and members.
    pub fn new(version: impl Into<String>, members: Vec<PackageMember>) -> Self {
        Self {
            version: version.into(),
            members,
        }
    }

    /// Scans this candidate against the default release policy.
    pub fn scan(&self) -> PackageReport {
        ReleasePolicy::default_scan(self)
    }
}

/// Why a member path cannot be a member of a release archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsafePath {
    /// Empty or only separators.
    Empty,
    /// Rooted (`/x`, `\x`) or drive-qualified (`C:\x`).
    Absolute,
    /// A `..` component: an archive-slip path.
    ParentTraversal,
    /// An empty or `.` component, which two spellings of one path would hide.
    NotCanonical,
}

impl UnsafePath {
    /// A short name for reports and error text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Empty => "is not a member name",
            Self::Absolute => "is an absolute path",
            Self::ParentTraversal => "escapes the archive with a `..` component",
            Self::NotCanonical => "is not a canonical archive-relative path",
        }
    }
}

impl fmt::Display for UnsafePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Whether a member path is safe to write out of an archive.
///
/// Backslashes are treated as separators first, because an archive written on
/// Windows spells its members that way and a member named `..\x` must be seen
/// as the traversal it is rather than as one odd file name. What is left is
/// exactly what a safe extraction needs: a name, a `..`-free sequence of
/// ordinary components, and nothing rooted or drive-qualified.
pub fn unsafe_path(path: &str) -> Option<UnsafePath> {
    let normalized = path.replace('\\', "/");
    if normalized.trim_matches('/').is_empty() {
        return Some(UnsafePath::Empty);
    }
    if normalized.starts_with('/') {
        return Some(UnsafePath::Absolute);
    }
    let mut saw_component = false;
    for component in normalized.split('/') {
        match component {
            "" | "." => return Some(UnsafePath::NotCanonical),
            ".." => return Some(UnsafePath::ParentTraversal),
            // A drive-qualified root: `C:/x` is absolute on the platform that
            // wrote it, whatever the platform extracting it looks like.
            other if is_drive_root(other) => return Some(UnsafePath::Absolute),
            _ => saw_component = true,
        }
    }
    if saw_component {
        None
    } else {
        Some(UnsafePath::Empty)
    }
}

/// Whether `component` is a Windows drive root such as `C:`.
fn is_drive_root(component: &str) -> bool {
    let bytes = component.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Why a candidate manifest could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The manifest file could not be read.
    Io { path: String },
    /// A line is not a directive this format knows.
    Syntax {
        file: String,
        line: usize,
        text: String,
    },
    /// A `member` directive has no size, or a size that is not a `u64`.
    MemberArity {
        file: String,
        line: usize,
        text: String,
    },
    /// The manifest states no version.
    MissingVersion { file: String },
    /// The version is empty or holds whitespace.
    VersionSyntax { file: String, version: String },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path } => write!(f, "cannot read candidate manifest {path}"),
            Self::Syntax { file, line, text } => {
                write!(
                    f,
                    "{file}:{line}: {text:?} is not a candidate-manifest directive"
                )
            }
            Self::MemberArity { file, line, text } => write!(
                f,
                "{file}:{line}: {text:?} must be `member <path> <size-bytes>`, and a member \
                 path may not contain whitespace"
            ),
            Self::MissingVersion { file } => {
                write!(
                    f,
                    "{file} states no `version:` line, so a release would ship unversioned"
                )
            }
            Self::VersionSyntax { file, version } => write!(
                f,
                "{file}: {version:?} is not a usable release version (it is empty or holds \
                 whitespace)"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// The candidate-manifest format this module reads.
///
/// One directive per line, `#` comments and blank lines ignored:
///
/// ```text
/// version: 0.1.0
/// member crimson-skies 12582912
/// member NOTICE.md 2048
/// ```
///
/// It is the *packaging input*, not the archive: a candidate manifest is what a
/// packaging step knows before it writes one. F61-B reads a real archive and
/// produces a manifest from it.
pub fn parse_manifest(file: &str, text: &str) -> Result<CandidatePackage, ManifestError> {
    let mut version: Option<String> = None;
    let mut members = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let directive = words.next().unwrap_or_default();
        match directive {
            "version:" => {
                let Some(value) = line.strip_prefix("version:") else {
                    return Err(ManifestError::Syntax {
                        file: file.to_string(),
                        line: number,
                        text: line.to_string(),
                    });
                };
                let value = value.trim();
                if value.is_empty() || value.chars().any(char::is_whitespace) {
                    return Err(ManifestError::VersionSyntax {
                        file: file.to_string(),
                        version: value.to_string(),
                    });
                }
                version = Some(value.to_string());
            }
            "member" => {
                // Exactly three words: the directive, the path and the size. A
                // path with whitespace is refused rather than silently split,
                // because guessing where the path ended is how a member called
                // `docs/my notes.md` becomes two members.
                let fields: Vec<&str> = words.collect();
                if fields.len() != 2 {
                    return Err(ManifestError::MemberArity {
                        file: file.to_string(),
                        line: number,
                        text: line.to_string(),
                    });
                }
                let size = fields[1]
                    .parse::<u64>()
                    .map_err(|_| ManifestError::MemberArity {
                        file: file.to_string(),
                        line: number,
                        text: line.to_string(),
                    })?;
                members.push(PackageMember::new(fields[0], size));
            }
            _ => {
                return Err(ManifestError::Syntax {
                    file: file.to_string(),
                    line: number,
                    text: line.to_string(),
                });
            }
        }
    }

    let Some(version) = version else {
        return Err(ManifestError::MissingVersion {
            file: file.to_string(),
        });
    };
    Ok(CandidatePackage::new(version, members))
}

/// Reads and parses a candidate manifest file.
pub fn read_manifest(path: &Path) -> Result<CandidatePackage, ManifestError> {
    let text =
        transient::read_to_string(path, transient::PATIENT).map_err(|_| ManifestError::Io {
            path: path.display().to_string(),
        })?;
    parse_manifest(&path.display().to_string(), &text)
}

/// One reason a candidate package is not releasable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    /// A member carries proprietary content.
    ProprietaryContent { path: String, kind: ProprietaryKind },
    /// A required notice is absent.
    MissingNotice {
        kind: NoticeKind,
        expected: &'static str,
    },
    /// The archive carries no new engine executable.
    MissingEngineBinary,
    /// A member path could escape or escape the archive root.
    UnsafeMemberPath { path: String, reason: UnsafePath },
    /// The same member path appears twice.
    DuplicateMember { path: String },
    /// A member nothing in the policy classifies.
    UnclassifiedMember { path: String },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProprietaryContent { path, kind } => write!(
                f,
                "{path} is proprietary {kind} content and may not ship in a release archive \
                 (F61 non-negotiable 1)"
            ),
            Self::MissingNotice { kind, expected } => write!(
                f,
                "the release archive has no {kind}: it must carry {expected} at its root \
                 (F61 non-negotiable 2 and 3)"
            ),
            Self::MissingEngineBinary => write!(
                f,
                "the release archive carries none of {:?}, so it ships no new engine \
                 (F61 deliverable)",
                ENGINE_BINARIES
            ),
            Self::UnsafeMemberPath { path, reason } => {
                write!(
                    f,
                    "member {path:?} {reason} and cannot be written out of an archive"
                )
            }
            Self::DuplicateMember { path } => {
                write!(f, "{path} appears more than once in the candidate archive")
            }
            Self::UnclassifiedMember { path } => write!(
                f,
                "{path} is not a class the release policy recognises; a release carries the \
                 engine, its notices, documentation and hash manifests only, so an \
                 unclassified member has to be classified deliberately"
            ),
        }
    }
}

/// The result of scanning a candidate package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageReport {
    /// The candidate's declared version.
    pub version: String,
    /// How many members were scanned.
    pub member_count: usize,
    /// The uncompressed bytes those members declare.
    ///
    /// The total saturates at [`u64::MAX`] rather than wrapping or panicking,
    /// because the sizes are declared in a text file and an absurd manifest must
    /// be scanned and reported on, not crash the gate that exists to refuse it.
    pub total_bytes: u64,
    /// Everything that blocks the release, in scan order.
    pub findings: Vec<Finding>,
}

impl PackageReport {
    /// Whether the candidate may be released.
    pub fn is_releasable(&self) -> bool {
        self.findings.is_empty()
    }

    /// The findings as lines a human reads, one per member or missing notice.
    pub fn lines(&self) -> Vec<String> {
        self.findings.iter().map(Finding::to_string).collect()
    }
}

impl fmt::Display for PackageReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "candidate {}: {} member(s), {} byte(s), {} finding(s)",
            self.version,
            self.member_count,
            self.total_bytes,
            self.findings.len()
        )
    }
}

/// The release policy: the notices a release must carry and the engine
/// executable it must ship.
///
/// A separate type from [`scan`] so a later stage can state a *different*
/// policy (an internal build, a platform package with a different launcher)
/// without editing the rules the tests pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleasePolicy {
    /// The notices that must be present, with the root path each occupies.
    pub required_notices: Vec<(NoticeKind, &'static str)>,
    /// Executables that may carry an `Executable` suffix.
    pub engine_binaries: Vec<String>,
}

impl ReleasePolicy {
    /// The policy a public release is scanned against.
    pub fn release() -> Self {
        Self {
            required_notices: REQUIRED_NOTICES.to_vec(),
            engine_binaries: ENGINE_BINARIES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        }
    }

    /// Scans `package` against this policy.
    pub fn scan(&self, package: &CandidatePackage) -> PackageReport {
        let mut findings = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut notices: BTreeSet<NoticeKind> = BTreeSet::new();
        let mut engine = false;

        for member in &package.members {
            if let Some(reason) = unsafe_path(&member.path) {
                findings.push(Finding::UnsafeMemberPath {
                    path: member.path.clone(),
                    reason,
                });
                continue;
            }
            if !seen.insert(member.path.as_str()) {
                findings.push(Finding::DuplicateMember {
                    path: member.path.clone(),
                });
                continue;
            }
            if self.is_engine_binary(&member.path) {
                engine = true;
                continue;
            }
            match self.notice_for(&member.path) {
                Some(kind) => {
                    notices.insert(kind);
                }
                None => match classify(&member.path) {
                    MemberClass::Proprietary(kind) => {
                        findings.push(Finding::ProprietaryContent {
                            path: member.path.clone(),
                            kind,
                        });
                    }
                    MemberClass::Unclassified => {
                        findings.push(Finding::UnclassifiedMember {
                            path: member.path.clone(),
                        });
                    }
                    // A notice this policy does not require is still a notice
                    // (or, if the path is not one of the required ones,
                    // documentation): allowing it is not a hole, because a
                    // policy asks for less, not for less plus a refusal.
                    MemberClass::EngineBinary
                    | MemberClass::Notice(_)
                    | MemberClass::Documentation
                    | MemberClass::HashManifest => {}
                },
            }
        }

        for (kind, member) in &self.required_notices {
            if !notices.contains(kind) {
                findings.push(Finding::MissingNotice {
                    kind: *kind,
                    expected: member,
                });
            }
        }
        if !engine {
            findings.push(Finding::MissingEngineBinary);
        }

        PackageReport {
            version: package.version.clone(),
            member_count: package.members.len(),
            total_bytes: package.members.iter().fold(0u64, |total, member| {
                total.saturating_add(member.size_bytes)
            }),
            findings,
        }
    }

    /// Scans `package` against the default release policy.
    pub fn default_scan(package: &CandidatePackage) -> PackageReport {
        Self::release().scan(package)
    }

    /// Whether this member is one of the policy's engine executables, at the
    /// archive root or in `bin/`.
    fn is_engine_binary(&self, path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        in_engine_directory(&lower)
            && self
                .engine_binaries
                .iter()
                .any(|name| lower.rsplit('/').next() == Some(name.as_str()))
    }

    /// The notice a member path satisfies, if any.
    fn notice_for(&self, path: &str) -> Option<NoticeKind> {
        self.required_notices
            .iter()
            .find(|(_, member)| member.eq_ignore_ascii_case(path))
            .map(|(kind, _)| *kind)
    }
}

/// Scans a candidate package against the default release policy.
pub fn scan(package: &CandidatePackage) -> PackageReport {
    ReleasePolicy::default_scan(package)
}

// ---------------------------------------------------------------------------
// User-data directory policy
// ---------------------------------------------------------------------------

/// The directory name this project owns inside the platform's per-user data
/// location.
///
/// The name is deliberately not `Crimson Skies` (non-negotiable 3): nothing
/// this project writes may present itself as Microsoft or Zipper software. The
/// choice is this project's, and the owner reviews branding — it is recorded in
/// `docs/findings/2026-10-02-f61-a-release-contents-and-user-data-policy.md`
/// rather than asserted as settled.
pub const APP_DIR_NAME: &str = "CrimsonSkiesRust";

/// The platforms the user-data policy resolves a base for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostPlatform {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux and other XDG systems.
    Linux,
}

impl HostPlatform {
    /// Every platform, for a table walk.
    pub const ALL: [Self; 3] = [Self::Windows, Self::MacOs, Self::Linux];

    /// A short name for reports and error text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Linux => "linux",
        }
    }
}

impl fmt::Display for HostPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The environment values a user-data base is derived from.
///
/// A typed view rather than a call to `std::env` inside the policy, so the
/// resolution rules are testable for every platform from one machine and a
/// release cannot depend on an environment variable it never declared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserDataEnv {
    /// `%APPDATA%` on Windows.
    pub app_data: Option<PathBuf>,
    /// `%HOME%` (or `%USERPROFILE%`) on macOS and Linux.
    pub home: Option<PathBuf>,
    /// `%XDG_DATA_HOME%` on Linux.
    pub xdg_data_home: Option<PathBuf>,
}

impl UserDataEnv {
    /// Reads the three variables from this process's environment.
    #[must_use]
    pub fn from_process() -> Self {
        let read = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            app_data: read("APPDATA"),
            home: read("HOME").or_else(|| read("USERPROFILE")),
            xdg_data_home: read("XDG_DATA_HOME"),
        }
    }
}

/// The areas a resolved user-data base holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UserDataArea {
    /// Profile populations. This is the base itself, because F48 derives
    /// `base/<population>/profile-<id>` and its own code says choosing the base
    /// is F61's job.
    Profiles,
    /// The derived content cache (F15). Deleting it costs time, never state.
    Cache,
    /// Diagnostics written by first-run and troubleshooting paths.
    Logs,
}

impl UserDataArea {
    /// Every area, for a table walk.
    pub const ALL: [Self; 3] = [Self::Profiles, Self::Cache, Self::Logs];

    /// The directory name the area occupies, or `None` for the base itself.
    pub const fn subdir(self) -> Option<&'static str> {
        match self {
            Self::Profiles => None,
            Self::Cache => Some("cache"),
            Self::Logs => Some("logs"),
        }
    }

    /// A short name for reports and error text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Profiles => "profiles",
            Self::Cache => "cache",
            Self::Logs => "logs",
        }
    }
}

impl fmt::Display for UserDataArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Whether `path` is absolute **for the platform it is being resolved for**.
///
/// [`Path::is_absolute`] answers for the host, which would make a Windows-shaped
/// `%APPDATA%` look relative when the policy is exercised on macOS — and the
/// rule under test would then be the host's, not the policy's. Drive roots and
/// UNC prefixes are therefore recognised explicitly for Windows, and every other
/// platform keeps the ordinary rule.
fn is_absolute_for(platform: HostPlatform, path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    if platform != HostPlatform::Windows {
        return false;
    }
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with("//") {
        return true;
    }
    text.split('/').next().is_some_and(is_drive_root)
}

/// The directories a resolved user-data base holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDataLayout {
    root: PathBuf,
}

impl UserDataLayout {
    /// The layout rooted at `root`, which [`UserDataPolicy::check`] has
    /// already accepted.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The base directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// One area's directory.
    pub fn area(&self, area: UserDataArea) -> PathBuf {
        match area.subdir() {
            Some(name) => self.root.join(name),
            None => self.root.clone(),
        }
    }

    /// Every area's directory, in [`UserDataArea::ALL`] order.
    pub fn directories(&self) -> Vec<(UserDataArea, PathBuf)> {
        UserDataArea::ALL
            .into_iter()
            .map(|area| (area, self.area(area)))
            .collect()
    }
}

/// Why a user-data base could not be resolved or is not acceptable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserDataError {
    /// The platform's data-location variable is not set.
    MissingEnvironment {
        platform: HostPlatform,
        variable: &'static str,
    },
    /// The variable is set to something that is not an absolute path.
    NotAbsolute {
        platform: HostPlatform,
        variable: &'static str,
        value: String,
    },
    /// The base would be inside the original installation, which is read-only.
    InsideInstallation {
        root: PathBuf,
        installation: PathBuf,
    },
    /// The base would be inside the executable's own directory, which a
    /// release cannot assume it may write to.
    InsideApplicationDirectory { root: PathBuf, application: PathBuf },
}

impl fmt::Display for UserDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment { platform, variable } => write!(
                f,
                "{platform} needs {variable} to locate a per-user data directory, and it is \
                 not set; there is no invented default to fall back to"
            ),
            Self::NotAbsolute {
                platform,
                variable,
                value,
            } => write!(
                f,
                "{platform} {variable} is {value:?}, which is not an absolute path; a user-data \
                 base derived from it would depend on the working directory"
            ),
            Self::InsideInstallation { root, installation } => write!(
                f,
                "the user-data base {} is inside the original installation {}, which is \
                 read-only and never written to",
                root.display(),
                installation.display()
            ),
            Self::InsideApplicationDirectory { root, application } => write!(
                f,
                "the user-data base {} is inside the application directory {}, so the release \
                 would need write access where it was unpacked",
                root.display(),
                application.display()
            ),
        }
    }
}

impl std::error::Error for UserDataError {}

/// Where the user-data base lives and what may not be used as one.
///
/// Non-negotiable 4 ("Release build startup must work outside the source
/// checkout and without developer absolute paths") is a *structural* rule here:
/// [`UserDataPolicy::resolve`] takes a platform and an environment and nothing
/// else, so there is no argument through which a build path, a working
/// directory or an executable location can enter the answer. [`check`] then
/// refuses a base that lands inside the original installation or inside the
/// application directory, so a misconfigured environment produces a refusal
/// rather than a write into read-only original data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDataPolicy {
    /// The directory name owned under the platform's data location.
    pub app_dir_name: String,
}

impl Default for UserDataPolicy {
    fn default() -> Self {
        Self::release()
    }
}

impl UserDataPolicy {
    /// The policy a release build uses.
    pub fn release() -> Self {
        Self {
            app_dir_name: APP_DIR_NAME.to_string(),
        }
    }

    /// Resolves the user-data base for `platform` from `env` alone.
    ///
    /// * Windows: `%APPDATA%\CrimsonSkiesRust`.
    /// * macOS: `~/Library/Application Support/CrimsonSkiesRust`.
    /// * Linux: `%XDG_DATA_HOME%\CrimsonSkiesRust`, falling back to
    ///   `~/.local/share/CrimsonSkiesRust` when `XDG_DATA_HOME` is unset, as
    ///   the XDG Base Directory specification requires.
    ///
    /// # Errors
    ///
    /// [`UserDataError::MissingEnvironment`] when the platform's variable is
    /// unset, and [`UserDataError::NotAbsolute`] when it is set to a relative
    /// path — a relative `XDG_DATA_HOME` is ignored by the specification rather
    /// than honoured, and honouring it would make the base depend on the
    /// working directory.
    pub fn resolve(
        &self,
        platform: HostPlatform,
        env: &UserDataEnv,
    ) -> Result<PathBuf, UserDataError> {
        let absolute = |variable: &'static str, value: Option<&PathBuf>| match value {
            None => Err(UserDataError::MissingEnvironment { platform, variable }),
            Some(path) if is_absolute_for(platform, path) => Ok(path.clone()),
            Some(path) => Err(UserDataError::NotAbsolute {
                platform,
                variable,
                value: path.display().to_string(),
            }),
        };

        match platform {
            HostPlatform::Windows => {
                let app_data = absolute("APPDATA", env.app_data.as_ref())?;
                Ok(app_data.join(&self.app_dir_name))
            }
            HostPlatform::MacOs => {
                let home = absolute("HOME", env.home.as_ref())?;
                Ok(home
                    .join("Library")
                    .join("Application Support")
                    .join(&self.app_dir_name))
            }
            HostPlatform::Linux => {
                if let Some(xdg) = env.xdg_data_home.as_ref() {
                    let xdg = absolute("XDG_DATA_HOME", Some(xdg))?;
                    return Ok(xdg.join(&self.app_dir_name));
                }
                let home = absolute("HOME", env.home.as_ref())?;
                Ok(home.join(".local").join("share").join(&self.app_dir_name))
            }
        }
    }

    /// Resolves the base and returns its layout.
    ///
    /// The layout is what F61-C's first launch creates; this stage only
    /// describes it.
    pub fn layout(
        &self,
        platform: HostPlatform,
        env: &UserDataEnv,
    ) -> Result<UserDataLayout, UserDataError> {
        self.resolve(platform, env).map(UserDataLayout::new)
    }

    /// Refuses a base that must not be written to.
    ///
    /// Containment is compared component by component through
    /// [`Path::starts_with`], so `/games/CrimsonSkies2` is not inside
    /// `/games/CrimsonSkies` — a string-prefix check would refuse a legitimate
    /// sibling, and a check that is too loose here writes into read-only
    /// original data.
    ///
    /// # Errors
    ///
    /// [`UserDataError::InsideInstallation`] when `root` is `installation` or
    /// below it, and [`UserDataError::InsideApplicationDirectory`] when it is
    /// `application` or below it. Either argument may be `None` when the caller
    /// does not know it yet.
    pub fn check(
        &self,
        root: &Path,
        installation: Option<&Path>,
        application: Option<&Path>,
    ) -> Result<(), UserDataError> {
        if let Some(installation) = installation
            && root.starts_with(installation)
        {
            return Err(UserDataError::InsideInstallation {
                root: root.to_path_buf(),
                installation: installation.to_path_buf(),
            });
        }
        if let Some(application) = application
            && root.starts_with(application)
        {
            return Err(UserDataError::InsideApplicationDirectory {
                root: root.to_path_buf(),
                application: application.to_path_buf(),
            });
        }
        Ok(())
    }
}
