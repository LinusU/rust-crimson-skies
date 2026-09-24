//! The typed installation-inventory path (F02-A) plus safe discovery,
//! hashing and diagnosis (F02-B).
//!
//! [`inventory`] turns discovered host paths plus their per-file facts into
//! the validated typed output, an [`InstallManifest`]: it derives each
//! relative spelling by stripping the host root (matching components
//! case-insensitively, so a root recorded with different letter case still
//! matches — spec F02 non-negotiable behavior 2), preserves the original
//! spelling, and refuses anything that is not under the root instead of
//! silently omitting it.
//!
//! [`discover`] is the F02-B production path: it walks a real installation
//! without following symbolic links, reads every regular file, hashes its
//! bytes with the in-module streaming [`Sha256`] (FIPS 180-4), and reports
//! what it saw through [`Discovery::diagnosis`] — the case-insensitive
//! `ZBD/planes.zbd`, world-group and ROF candidates of spec F02
//! non-negotiable behavior 2/3, plus every entry that was deliberately not
//! followed. [`fingerprint`] and [`content_fingerprint`] describe the actual
//! bytes of the installation, not an EXE version string, and
//! [`AnalysisCache`] reuses recorded analysis only while those fingerprints
//! still match (spec F02-B: a one-byte edit changes the fingerprint and
//! invalidates the cache entries). The original installation is opened
//! read-only; no discovered content is ever written back.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Component, Path, PathBuf};

use cs_types::evidence::ContentHash;
use cs_types::install::{
    FileFamily, FileRole, InstallFileRecord, InstallManifest, ManifestError, ParseState,
    RelativePath,
};

/// Typed input: one regular source file discovered under a host root.
///
/// Discovery supplies the host path as found on this machine plus the facts
/// it measured (size, SHA-256) and its current analysis result
/// (family/role/parse state; all of them may be the explicit unknowns).
/// The host path is input only: [`inventory`] reduces it to a relative
/// spelling, and it never reaches the manifest's logical identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredFile {
    /// The path as discovered on this host (root-joined).
    pub host_path: PathBuf,
    /// The measured size in bytes.
    pub size_bytes: u64,
    /// The measured SHA-256 of the file's bytes.
    pub sha256: ContentHash,
    /// The detected family, `None` when undetected.
    pub family: Option<FileFamily>,
    /// The classification recorded so far.
    pub role: FileRole,
    /// The parse state recorded so far.
    pub parse_state: ParseState,
}

/// Builds the typed output: a validated [`InstallManifest`] over `host_root`.
///
/// Every discovered file either becomes a manifest row or the whole
/// inventory fails with a named error — no entry is ever dropped, because
/// the inventory must contain every regular source file (spec F02
/// non-negotiable behavior 4). Two files that differ only in letter case
/// collide on one logical key and are rejected as ambiguous rather than
/// silently merged.
pub fn inventory(
    host_root: &Path,
    discovered: Vec<DiscoveredFile>,
) -> Result<InstallManifest, ManifestError> {
    if host_root.as_os_str().is_empty() {
        return Err(ManifestError::EmptyRoot);
    }
    let mut files = Vec::with_capacity(discovered.len());
    for file in discovered {
        let relative_spelling = relative_spelling(host_root, &file.host_path)?;
        files.push(InstallFileRecord {
            relative_spelling,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            family: file.family,
            role: file.role,
            parse_state: file.parse_state,
        });
    }
    InstallManifest::new(host_root.to_path_buf(), files)
}

/// Strips `host_root` from `host_path` into a validated relative spelling.
///
/// Root components are matched case-insensitively (ASCII), which is what
/// lets a `CS_GAME_DIR` whose letter case disagrees with the discovered
/// paths still inventory the same data under one logical identity. The
/// remainder keeps each component's original case and is joined with `/`,
/// the portable spelling of a relative path.
fn relative_spelling(host_root: &Path, host_path: &Path) -> Result<RelativePath, ManifestError> {
    let root: Vec<&OsStr> = host_root
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| component.as_os_str())
        .collect();
    let path: Vec<&OsStr> = host_path
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| component.as_os_str())
        .collect();

    if path.len() < root.len()
        || !path.iter().zip(root.iter()).all(|(part, prefix)| {
            part.as_encoded_bytes()
                .eq_ignore_ascii_case(prefix.as_encoded_bytes())
        })
    {
        return Err(ManifestError::NotUnderRoot {
            path: host_path.to_path_buf(),
        });
    }
    if path.len() == root.len() {
        return Err(ManifestError::EmptyRelative {
            path: host_path.to_path_buf(),
        });
    }

    let mut components = Vec::with_capacity(path.len() - root.len());
    for component in &path[root.len()..] {
        let text = component
            .to_str()
            .ok_or_else(|| ManifestError::NonUtf8Path {
                path: host_path.to_path_buf(),
            })?;
        components.push(text);
    }
    RelativePath::new(&components.join("/")).map_err(|error| ManifestError::RelativePath {
        path: host_path.to_path_buf(),
        error,
    })
}

// --- F02-B: safe discovery, hashing and diagnosis --------------------------

/// The FIPS 180-4 SHA-256 initial hash value `H(0)`.
const SHA256_INITIAL: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The FIPS 180-4 SHA-256 round constants `K`.
const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// Streaming SHA-256 (FIPS 180-4) over installation bytes (F02-B).
///
/// Discovery hashes every regular file through this implementation, in
/// 64 KiB reads, so the inventory's `sha256` columns describe the bytes that
/// were actually read. The algorithm is the published FIPS 180-4
/// specification — the acceptance tests check it against the NIST example
/// vectors, and the evidence harness cross-checks it against `python3`
/// `hashlib` on original installation files. No third-party crate is added:
/// the owner paths of this task allow no dependency change.
#[derive(Clone)]
pub struct Sha256 {
    /// The eight 32-bit chaining values.
    state: [u32; 8],
    /// The partial message block.
    block: [u8; 64],
    /// How many bytes of `block` are filled.
    buffered: usize,
    /// Total message bytes received by [`Sha256::update`].
    total_bytes: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A hasher in the FIPS 180-4 initial state.
    pub fn new() -> Self {
        Self {
            state: SHA256_INITIAL,
            block: [0u8; 64],
            buffered: 0,
            total_bytes: 0,
        }
    }

    /// Absorbs `data`, compressing every completed 64-byte block.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total_bytes = self.total_bytes.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.buffered).min(data.len());
            self.block[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.block;
                sha256_compress(&mut self.state, &block);
                self.buffered = 0;
            }
        }
    }

    /// Applies the padding and length suffix, returning the digest.
    pub fn finalize(mut self) -> ContentHash {
        let bit_length = self.total_bytes.wrapping_mul(8);
        self.block[self.buffered] = 0x80;
        let mut cursor = self.buffered + 1;
        if cursor > 56 {
            while cursor < 64 {
                self.block[cursor] = 0;
                cursor += 1;
            }
            let block = self.block;
            sha256_compress(&mut self.state, &block);
            cursor = 0;
        }
        while cursor < 56 {
            self.block[cursor] = 0;
            cursor += 1;
        }
        self.block[56..64].copy_from_slice(&bit_length.to_be_bytes());
        let block = self.block;
        sha256_compress(&mut self.state, &block);

        let mut digest = [0u8; 32];
        for (word, value) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            word.copy_from_slice(&value.to_be_bytes());
        }
        ContentHash::from_bytes(digest)
    }
}

/// The FIPS 180-4 compression function over one 64-byte block.
fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut schedule = [0u32; 64];
    let (words, _rest) = block.as_chunks::<4>();
    for (slot, word) in schedule.iter_mut().take(16).zip(words) {
        *slot = u32::from_be_bytes(*word);
    }
    for index in 16..64 {
        let low = schedule[index - 15];
        let high = schedule[index - 2];
        let s0 = low.rotate_right(7) ^ low.rotate_right(18) ^ (low >> 3);
        let s1 = high.rotate_right(17) ^ high.rotate_right(19) ^ (high >> 10);
        schedule[index] = schedule[index - 16]
            .wrapping_add(s0)
            .wrapping_add(schedule[index - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(SHA256_ROUND_CONSTANTS[index])
            .wrapping_add(schedule[index]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *slot = slot.wrapping_add(value);
    }
}

/// One-shot SHA-256 of `bytes` as a canonical lowercase-hex [`ContentHash`].
pub fn sha256(bytes: &[u8]) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize()
}

/// Why installation discovery failed.
///
/// Every variant names the host path it happened at; discovery never
/// silently drops a file it could not read (spec F02 non-negotiable
/// behavior 4: nothing is omitted).
#[derive(Debug)]
pub enum DiscoveryError {
    /// The validated inventory refused the discovered set — see
    /// [`ManifestError`] for the individual named rules.
    Inventory(ManifestError),
    /// The host root is missing, unreadable, or not a directory.
    RootUnavailable {
        /// The host root that was asked for.
        path: PathBuf,
        /// Why the root could not be used.
        source: io::Error,
    },
    /// A directory in the tree could not be read.
    UnreadableDirectory {
        /// The directory that could not be read.
        path: PathBuf,
        /// Why it could not be read.
        source: io::Error,
    },
    /// A regular file could not be opened or read.
    UnreadableFile {
        /// The file that could not be read.
        path: PathBuf,
        /// Why it could not be read.
        source: io::Error,
    },
    /// A file changed on disk while it was being hashed, so no digest of a
    /// coherent byte sequence exists; discovery refuses instead of
    /// recording a torn fingerprint.
    FileChangedDuringDiscovery {
        /// The file that changed underneath the read.
        path: PathBuf,
    },
    /// A cached analysis could not be applied — see [`CacheError`] for the
    /// named refusal (a fingerprint mismatch or an analysis the
    /// per-record rules would not accept).
    Cache(CacheError),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inventory(error) => write!(f, "{error}"),
            Self::RootUnavailable { path, source } => {
                write!(f, "host root {} is unavailable: {source}", path.display())
            }
            Self::UnreadableDirectory { path, source } => {
                write!(f, "cannot read directory {}: {source}", path.display())
            }
            Self::UnreadableFile { path, source } => {
                write!(f, "cannot read file {}: {source}", path.display())
            }
            Self::FileChangedDuringDiscovery { path } => write!(
                f,
                "{} changed while it was being inventoried; refusing the torn digest",
                path.display()
            ),
            Self::Cache(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Inventory(error) => Some(error),
            Self::RootUnavailable { source, .. }
            | Self::UnreadableDirectory { source, .. }
            | Self::UnreadableFile { source, .. } => Some(source),
            Self::Cache(error) => Some(error),
            Self::FileChangedDuringDiscovery { .. } => None,
        }
    }
}

impl From<ManifestError> for DiscoveryError {
    fn from(error: ManifestError) -> Self {
        Self::Inventory(error)
    }
}

impl From<CacheError> for DiscoveryError {
    fn from(error: CacheError) -> Self {
        Self::Cache(error)
    }
}

/// Why an entry observed during discovery was not inventoried (F02-B).
///
/// Skipping is always visible in [`Diagnosis::skipped`], never silent: the
/// inventory covers every *regular* source file, and anything else that was
/// present is reported rather than dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// A symbolic link. Discovery does not follow links: a link could point
    /// outside the installation root, and the inventory must stay inside it
    /// (safe discovery).
    SymbolicLink,
    /// A non-regular filesystem object: socket, FIFO, device node or a
    /// type the host reports as none of the above.
    NonRegular,
}

impl SkipReason {
    /// The short vocabulary label used in reports and diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::SymbolicLink => "symbolic-link",
            Self::NonRegular => "non-regular",
        }
    }
}

/// One observed entry that discovery deliberately did not inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedEntry {
    /// The host path exactly as observed.
    pub host_path: PathBuf,
    /// Why it was not inventoried.
    pub reason: SkipReason,
}

/// The reference world-group leads named by spec F02
/// (non-negotiable behavior 3): `c1,c1b,c1c,c2,c2b,c3,c4,c5`.
///
/// The sheet is explicit that these are *reference leads, not the
/// authoritative mission list*: [`Diagnosis::world_groups`] records every
/// group actually observed (additional groups included), and
/// [`Diagnosis::absent_reference_groups`] only reports which of these leads
/// were not found. Absence of a lead is a report, never a claim that the
/// installation is incomplete — the expected-content denominator is F02-D
/// discovery work.
pub const REFERENCE_WORLD_GROUP_LEADS: [&str; 8] =
    ["c1", "c1b", "c1c", "c2", "c2b", "c3", "c4", "c5"];

/// What one discovery run observed about the installation itself (F02-B
/// diagnosis): counts, deliberately skipped entries, and the
/// case-insensitive candidates of spec F02 non-negotiable behavior 2/3
/// with their original spellings preserved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnosis {
    /// Regular files inventoried (equals the manifest's row count).
    pub file_count: usize,
    /// Directories visited below the host root.
    pub directory_count: usize,
    /// Total inventoried bytes.
    pub total_bytes: u64,
    /// Entries that were observed and deliberately not inventoried.
    pub skipped: Vec<SkippedEntry>,
    /// Every directory observed below the host root, original spellings
    /// preserved, sorted by logical key. Consumers that need the real
    /// directory set — for example the F02-C dependency-impact report, which
    /// derives mission directories under each world group — read it from
    /// here rather than inferring directories from file paths, so a
    /// directory that carries no regular file still reports its expected
    /// archives as unavailable instead of being silently omitted.
    pub directories: Vec<RelativePath>,
    /// The installation's top-level `zbd` directory as spelled on disk,
    /// found case-insensitively; `None` when it is absent.
    pub zbd_dir: Option<RelativePath>,
    /// `ZBD/planes.zbd`, found case-insensitively under [`Self::zbd_dir`];
    /// the original spelling is preserved, `None` when it is absent.
    pub planes_zbd: Option<RelativePath>,
    /// The directories directly under [`Self::zbd_dir`]: every world group
    /// observed, original spellings preserved, sorted by logical key.
    pub world_groups: Vec<RelativePath>,
    /// The [`REFERENCE_WORLD_GROUP_LEADS`] not observed under
    /// [`Self::zbd_dir`], in lead order. Empty means every lead is present
    /// case-insensitively; it never claims the list is authoritative.
    pub absent_reference_groups: Vec<String>,
    /// Files whose extension is `rof` case-insensitively, anywhere in the
    /// tree, original spellings preserved, sorted by logical key.
    pub rof_candidates: Vec<RelativePath>,
}

/// The result of one discovery run: the validated typed inventory plus what
/// the walk observed about the installation (F02-B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    /// The validated inventory of every regular source file.
    pub manifest: InstallManifest,
    /// What the run observed beyond the rows themselves.
    pub diagnosis: Diagnosis,
    /// How many rows took their family/role/parse analysis from the cache
    /// passed to [`discover_with_cache`]: `0` for [`discover`] and whenever
    /// the cache no longer matches the installation's fingerprint.
    pub cached_rows: usize,
}

/// Discovers, hashes and diagnoses the installation under `host_root` (F02-B).
///
/// The walk is read-only and safe: directories are visited in a
/// deterministic, sorted order; symbolic links are never followed, so the
/// inventory can never escape `host_root`; non-regular entries are recorded
/// in [`Discovery::diagnosis`] instead of being opened; and a file that
/// cannot be read, or that changes underneath the read, fails the whole run
/// by name rather than being silently omitted. Every regular file's bytes
/// are hashed with [`Sha256`], and its `size_bytes` is exactly the number of
/// bytes that were hashed. Fresh rows carry the explicit unknown analysis
/// (`family: None`, `FileRole::Unknown`, `ParseState::Unparsed`): family
/// detection belongs to the format tasks and classification to the F02-D
/// audit, so this stage never guesses one.
pub fn discover(host_root: &Path) -> Result<Discovery, DiscoveryError> {
    discover_inner(host_root, None)
}

/// [`discover`] reusing recorded analysis from `cache` where it still
/// matches (F02-B).
///
/// An entry is reused only while the cache's fingerprint equals the
/// fingerprint of the freshly discovered manifest *and* the recorded
/// per-file digest still matches: a one-byte edit therefore changes the
/// fingerprint and invalidates the cached entries, and the affected rows
/// fall back to the explicit unknown analysis instead of keeping stale
/// results.
pub fn discover_with_cache(
    host_root: &Path,
    cache: &AnalysisCache,
) -> Result<Discovery, DiscoveryError> {
    discover_inner(host_root, Some(cache))
}

/// The shared body of [`discover`] and [`discover_with_cache`].
fn discover_inner(
    host_root: &Path,
    cache: Option<&AnalysisCache>,
) -> Result<Discovery, DiscoveryError> {
    if host_root.as_os_str().is_empty() {
        return Err(ManifestError::EmptyRoot.into());
    }
    let root_metadata =
        fs::metadata(host_root).map_err(|source| DiscoveryError::RootUnavailable {
            path: host_root.to_path_buf(),
            source,
        })?;
    if !root_metadata.is_dir() {
        return Err(DiscoveryError::RootUnavailable {
            path: host_root.to_path_buf(),
            source: io::Error::new(io::ErrorKind::NotADirectory, "host root is not a directory"),
        });
    }

    let mut walked = Walked::default();
    walk_directory(host_root, "", &mut walked)?;

    let mut discovered = Vec::with_capacity(walked.files.len());
    for host_path in walked.files {
        let (size_bytes, digest) = hash_file(&host_path)?;
        discovered.push(DiscoveredFile {
            host_path,
            size_bytes,
            sha256: digest,
            family: None,
            role: FileRole::Unknown,
            parse_state: ParseState::Unparsed,
        });
    }

    let mut manifest = inventory(host_root, discovered)?;
    let cached_rows = match cache {
        Some(cache) => cache.apply_to(&mut manifest)?,
        None => 0,
    };
    let diagnosis = diagnose(&manifest, &walked.directories, walked.skipped);
    Ok(Discovery {
        manifest,
        diagnosis,
        cached_rows,
    })
}

/// What one directory walk collected.
#[derive(Debug, Default)]
struct Walked {
    /// Host paths of every regular file, in deterministic walk order.
    files: Vec<PathBuf>,
    /// Relative spellings of every directory below the root.
    directories: Vec<RelativePath>,
    /// Observed entries that were deliberately not followed.
    skipped: Vec<SkippedEntry>,
}

/// Recursively collects `directory`'s entries without following links.
///
/// Entries are processed in sorted byte order so two runs over the same
/// tree walk identically. Component letter case is preserved as discovered.
fn walk_directory(
    directory: &Path,
    relative: &str,
    walked: &mut Walked,
) -> Result<(), DiscoveryError> {
    let entries =
        fs::read_dir(directory).map_err(|source| DiscoveryError::UnreadableDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
    let mut collected: Vec<(OsString, PathBuf, fs::FileType)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| DiscoveryError::UnreadableDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let file_type =
            entry
                .file_type()
                .map_err(|source| DiscoveryError::UnreadableDirectory {
                    path: entry.path(),
                    source,
                })?;
        collected.push((entry.file_name(), entry.path(), file_type));
    }
    collected.sort_by(|left, right| left.0.as_encoded_bytes().cmp(right.0.as_encoded_bytes()));

    for (name, path, file_type) in collected {
        let component = name
            .to_str()
            .ok_or_else(|| ManifestError::NonUtf8Path { path: path.clone() })?;
        let child = if relative.is_empty() {
            component.to_owned()
        } else {
            format!("{relative}/{component}")
        };
        if file_type.is_symlink() {
            walked.skipped.push(SkippedEntry {
                host_path: path,
                reason: SkipReason::SymbolicLink,
            });
            continue;
        }
        if file_type.is_dir() {
            let spelling =
                RelativePath::new(&child).map_err(|error| ManifestError::RelativePath {
                    path: path.clone(),
                    error,
                })?;
            walked.directories.push(spelling);
            walk_directory(&path, &child, walked)?;
        } else if file_type.is_file() {
            walked.files.push(path);
        } else {
            walked.skipped.push(SkippedEntry {
                host_path: path,
                reason: SkipReason::NonRegular,
            });
        }
    }
    Ok(())
}

/// Hashes one regular file, returning the hashed byte count and digest.
///
/// The size reported is the number of bytes actually read, and the read is
/// bracketed by metadata checks: if the file's length or modification time
/// moved while it was read, no coherent digest exists and the run fails by
/// name. A same-length rewrite landing inside the read window is the
/// documented residual race; the installation is owner-read-only in
/// practice.
fn hash_file(path: &Path) -> Result<(u64, ContentHash), DiscoveryError> {
    let mut file = fs::File::open(path).map_err(|source| DiscoveryError::UnreadableFile {
        path: path.to_path_buf(),
        source,
    })?;
    let before = file
        .metadata()
        .map_err(|source| DiscoveryError::UnreadableFile {
            path: path.to_path_buf(),
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| DiscoveryError::UnreadableFile {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    let after = fs::metadata(path).map_err(|source| DiscoveryError::UnreadableFile {
        path: path.to_path_buf(),
        source,
    })?;
    let before_modified = before
        .modified()
        .map_err(|source| DiscoveryError::UnreadableFile {
            path: path.to_path_buf(),
            source,
        })?;
    let after_modified = after
        .modified()
        .map_err(|source| DiscoveryError::UnreadableFile {
            path: path.to_path_buf(),
            source,
        })?;
    if before.len() != total || after.len() != total || before_modified != after_modified {
        return Err(DiscoveryError::FileChangedDuringDiscovery {
            path: path.to_path_buf(),
        });
    }
    Ok((total, hasher.finalize()))
}

/// Derives the install-level diagnosis from the finished manifest and walk.
fn diagnose(
    manifest: &InstallManifest,
    directories: &[RelativePath],
    skipped: Vec<SkippedEntry>,
) -> Diagnosis {
    let mut sorted_directories = directories.to_vec();
    sorted_directories.sort_by_key(|directory| directory.logical_key());
    let zbd_dir = directories
        .iter()
        .find(|directory| directory.logical_key() == "zbd")
        .cloned();
    let world_groups = match &zbd_dir {
        Some(zbd) => {
            let prefix = format!("{}/", zbd.logical_key());
            let mut groups: Vec<RelativePath> = directories
                .iter()
                .filter(|directory| {
                    directory
                        .logical_key()
                        .strip_prefix(&prefix)
                        .is_some_and(|rest| !rest.contains('/'))
                })
                .cloned()
                .collect();
            groups.sort_by_key(|left| left.logical_key());
            groups
        }
        None => Vec::new(),
    };
    let absent_reference_groups = REFERENCE_WORLD_GROUP_LEADS
        .iter()
        .filter(|lead| {
            let expected = format!("zbd/{lead}");
            !world_groups
                .iter()
                .any(|group| group.logical_key() == expected)
        })
        .map(|lead| (*lead).to_owned())
        .collect();
    let planes_zbd = manifest
        .files
        .iter()
        .find(|row| row.relative_spelling.logical_key() == "zbd/planes.zbd")
        .map(|row| row.relative_spelling.clone());
    let mut rof_candidates: Vec<RelativePath> = manifest
        .files
        .iter()
        .filter(|row| row.relative_spelling.logical_key().ends_with(".rof"))
        .map(|row| row.relative_spelling.clone())
        .collect();
    rof_candidates.sort_by_key(|left| left.logical_key());

    Diagnosis {
        file_count: manifest.files.len(),
        directory_count: directories.len(),
        total_bytes: manifest.files.iter().map(|row| row.size_bytes).sum(),
        skipped,
        directories: sorted_directories,
        zbd_dir,
        planes_zbd,
        world_groups,
        absent_reference_groups,
        rof_candidates,
    }
}

/// The installation fingerprint (F02-B): SHA-256 over the manifest's
/// canonical logical identity.
///
/// It describes the actual installation — every inventoried key, size and
/// content digest — never an EXE version string, and it deliberately
/// excludes the host root, the letter case of spellings and every analysis
/// result (the guarantees [`InstallManifest::logical_identity`] already
/// makes). A one-byte content edit changes it; that is what invalidates an
/// [`AnalysisCache`] bound to the previous value.
pub fn fingerprint(manifest: &InstallManifest) -> ContentHash {
    sha256(manifest.logical_identity().canonical_bytes())
}

/// The content fingerprint (F02-B): SHA-256 over the per-file content
/// digests alone, sorted by logical key.
///
/// Where [`fingerprint`] covers keys and sizes as well, this digest
/// describes only the bytes of the installation — the `content_sha256` of
/// an evidence record (`schemas/evidence.schema.json`). No extraction or
/// normalization happens at this stage, so it fingerprints the raw
/// installed content, not a derived canonical form.
pub fn content_fingerprint(manifest: &InstallManifest) -> ContentHash {
    let mut rows: Vec<(&RelativePath, &ContentHash)> = manifest
        .files
        .iter()
        .map(|row| (&row.relative_spelling, &row.sha256))
        .collect();
    rows.sort_by_key(|left| left.0.logical_key());
    let mut hasher = Sha256::new();
    for (_, digest) in rows {
        hasher.update(digest.as_bytes());
    }
    hasher.finalize()
}

/// One recorded analysis result for one exact file content (F02-B).
///
/// The cache never stores analysis without the digest it was recorded
/// against, so a reused result can only ever describe the bytes it was
/// measured on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedAnalysis {
    /// The detected family, `None` when undetected.
    pub family: Option<FileFamily>,
    /// The classification recorded for that content.
    pub role: FileRole,
    /// The parse state recorded for that content.
    pub parse_state: ParseState,
}

/// Why an [`AnalysisCache`] operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheError {
    /// The manifest and the cache describe different installation
    /// fingerprints: entries from one installation state may never be
    /// recorded against or applied to another.
    FingerprintMismatch {
        /// The fingerprint the cache is bound to.
        cache: ContentHash,
        /// The fingerprint of the manifest that was passed in.
        manifest: ContentHash,
    },
    /// No row of the manifest uses this logical key.
    UnknownKey {
        /// The logical key that was asked for.
        key: String,
    },
    /// The analysis would violate the per-record inventory rules (an
    /// `unused` role without a reason, or a failed parse without a
    /// diagnostic).
    InvalidAnalysis(ManifestError),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FingerprintMismatch { cache, manifest } => write!(
                f,
                "cache fingerprint {cache} does not match manifest fingerprint {manifest}"
            ),
            Self::UnknownKey { key } => {
                write!(f, "no inventoried row uses logical key {key:?}")
            }
            Self::InvalidAnalysis(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidAnalysis(error) => Some(error),
            Self::FingerprintMismatch { .. } | Self::UnknownKey { .. } => None,
        }
    }
}

/// A derived cache of per-file analysis, bound to one installation
/// fingerprint (F02-B).
///
/// It is a performance optimization, never the authoritative data source
/// (`docs/01-ARCHITECTURE.md`, "derived cache"): [`Self::apply_to`] only
/// ever *reuses* recorded analysis, and only while both the installation
/// fingerprint and the per-file digest still match. A one-byte edit changes
/// the fingerprint, which invalidates every entry at once — the minimum
/// acceptance scenario of spec F02-B.
#[derive(Clone, Debug)]
pub struct AnalysisCache {
    /// The fingerprint of the installation state the entries belong to.
    fingerprint: ContentHash,
    /// Logical key → (content digest the analysis was recorded against,
    /// recorded analysis).
    entries: BTreeMap<String, (ContentHash, CachedAnalysis)>,
}

impl AnalysisCache {
    /// An empty cache bound to `manifest`'s fingerprint.
    pub fn for_manifest(manifest: &InstallManifest) -> Self {
        Self {
            fingerprint: fingerprint(manifest),
            entries: BTreeMap::new(),
        }
    }

    /// The installation fingerprint this cache is bound to.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }

    /// How many analysis entries the cache holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds no analysis entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether `fingerprint` still describes the installation state these
    /// entries belong to. `false` means every entry is invalidated.
    pub fn is_valid_for(&self, fingerprint: &ContentHash) -> bool {
        *fingerprint == self.fingerprint
    }

    /// Records `analysis` for the row of `manifest` named `logical_key`,
    /// binding it to that row's current content digest.
    ///
    /// The manifest must still be at this cache's fingerprint, and the
    /// analysis must satisfy the per-record inventory rules — a cache entry
    /// can never smuggle in a classification the manifest constructor would
    /// have refused.
    pub fn record(
        &mut self,
        manifest: &InstallManifest,
        logical_key: &str,
        analysis: CachedAnalysis,
    ) -> Result<(), CacheError> {
        let manifest_fingerprint = fingerprint(manifest);
        if manifest_fingerprint != self.fingerprint {
            return Err(CacheError::FingerprintMismatch {
                cache: self.fingerprint,
                manifest: manifest_fingerprint,
            });
        }
        let row = manifest
            .files
            .iter()
            .find(|row| row.relative_spelling.logical_key() == logical_key)
            .ok_or_else(|| CacheError::UnknownKey {
                key: logical_key.to_owned(),
            })?;
        let scratch = InstallFileRecord {
            relative_spelling: row.relative_spelling.clone(),
            size_bytes: row.size_bytes,
            sha256: row.sha256,
            family: analysis.family.clone(),
            role: analysis.role.clone(),
            parse_state: analysis.parse_state.clone(),
        };
        scratch.validate().map_err(CacheError::InvalidAnalysis)?;
        self.entries
            .insert(logical_key.to_owned(), (row.sha256, analysis));
        Ok(())
    }

    /// The reusable analysis for one row, if the cache still matches the
    /// installation fingerprint and the recorded digest still matches
    /// `sha256`.
    pub fn entry(
        &self,
        fingerprint: &ContentHash,
        logical_key: &str,
        sha256: ContentHash,
    ) -> Option<&CachedAnalysis> {
        if !self.is_valid_for(fingerprint) {
            return None;
        }
        let (recorded, analysis) = self.entries.get(logical_key)?;
        (*recorded == sha256).then_some(analysis)
    }

    /// Applies every still-valid entry to `manifest`, returning how many
    /// rows took their analysis.
    ///
    /// Returns `0` without touching a row when the cache fingerprint no
    /// longer matches the manifest (the invalidated state): stale analysis
    /// is never reused, and rows that have no matching entry keep their
    /// explicit unknown analysis.
    pub fn apply_to(&self, manifest: &mut InstallManifest) -> Result<usize, CacheError> {
        if !self.is_valid_for(&fingerprint(manifest)) {
            return Ok(0);
        }
        let mut applied = 0;
        for row in &mut manifest.files {
            let logical_key = row.relative_spelling.logical_key();
            let Some((recorded, analysis)) = self.entries.get(&logical_key) else {
                continue;
            };
            if *recorded != row.sha256 {
                continue;
            }
            let scratch = InstallFileRecord {
                relative_spelling: row.relative_spelling.clone(),
                size_bytes: row.size_bytes,
                sha256: row.sha256,
                family: analysis.family.clone(),
                role: analysis.role.clone(),
                parse_state: analysis.parse_state.clone(),
            };
            scratch.validate().map_err(CacheError::InvalidAnalysis)?;
            row.family = analysis.family.clone();
            row.role = analysis.role.clone();
            row.parse_state = analysis.parse_state.clone();
            applied += 1;
        }
        Ok(applied)
    }
}
