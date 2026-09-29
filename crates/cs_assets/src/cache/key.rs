//! The cache-key contract: exactly what identifies a derived asset (F15-A).
//!
//! Spec F15, "Deliverable and interfaces": "Cache entries identify
//! installation hash, source span hash, decoder/IR version and conversion
//! options." [`CacheKey`] is that identity as a typed record plus the
//! [`ContentHash`] digest a store keys on. Every facet is a construction
//! input of the digest, so two keys are equal **iff** every facet is equal:
//!
//! * `install` — the F02 installation fingerprint
//!   ([`crate::install::fingerprint`]), so an entry built against one
//!   installation never serves another;
//! * `inputs` — the [`SourceSpanHash`] of every source span the derived
//!   bytes were built from. Per-input granularity is what makes spec F15
//!   AC03 possible at all: editing one livery source changes only the span
//!   hashes of that source, so only keys listing it change;
//! * `converter` — which decoder produced the entry ([`DecoderId`] plus
//!   `decoder_version`) and under which intermediate-representation schema
//!   ([`IrVersion`]). A decoder bug fix or an IR change bumps a version and
//!   invalidates every entry it produced — never silently reusing bytes a
//!   different decoder would not have written;
//! * `options` — the normalized [`ConversionOptions`] the derived asset was
//!   built with, so two derivations of the same source with different
//!   options are different entries.
//!
//! The digest is computed once at construction over a domain-separated,
//! length-prefixed canonical encoding, so equal identities produce equal
//! digests on every run and every platform — the warm/cold equality spec
//! F15 AC04 requires. Nothing in this module decides where or how entries
//! are stored; that is `super::entry` and F15-B.

use std::fmt;

use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;

use crate::install::Sha256;

/// Maximum byte length of a label in this module: decoder ids, conversion
/// option names and values.
///
/// Same grammar as `cs_types::asset_id`'s labels (lowercase `[a-z0-9._-]`,
/// at most 64 bytes, starting with a letter or digit), re-validated here so
/// the cache contract does not borrow a mount-namespace type for a decoder
/// name.
const MAX_LABEL_LEN: usize = 64;

/// The domain separator of the [`SourceSpanHash`] encoding.
const SPAN_HASH_DOMAIN: &[u8] = b"cs-assets/cache/span-hash/v1\0";

/// The domain separator of the [`CacheKey`] digest encoding.
const KEY_DIGEST_DOMAIN: &[u8] = b"cs-assets/cache/key/v1\0";

/// Why a cache label was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheLabelError {
    /// The label was empty.
    Empty {
        /// Which label failed.
        label: &'static str,
    },
    /// The label exceeded [`MAX_LABEL_LEN`] bytes.
    TooLong {
        /// Which label failed.
        label: &'static str,
        /// Its length in bytes.
        len: usize,
    },
    /// The label contained a character outside `[a-z0-9._-]` or did not
    /// start with a letter or digit.
    BadCharacter {
        /// Which label failed.
        label: &'static str,
        /// The offending character.
        ch: char,
    },
}

impl fmt::Display for CacheLabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { label } => write!(f, "{label} label must not be empty"),
            Self::TooLong { label, len } => {
                write!(f, "{label} label is {len} bytes, max is {MAX_LABEL_LEN}")
            }
            Self::BadCharacter { label, ch } => {
                write!(f, "{label} label contains disallowed character {ch:?}")
            }
        }
    }
}

impl std::error::Error for CacheLabelError {}

/// Validates a cache label: lowercase ASCII alphanumerics plus `.`, `_` and
/// `-`, starting with a letter or digit, at most [`MAX_LABEL_LEN`] bytes.
///
/// Uppercase is rejected, not folded: option spellings are engine-authored,
/// and a refused spelling is louder than a silently rewritten one.
fn cache_label(label: &'static str, raw: &str) -> Result<String, CacheLabelError> {
    if raw.is_empty() {
        return Err(CacheLabelError::Empty { label });
    }
    if raw.len() > MAX_LABEL_LEN {
        return Err(CacheLabelError::TooLong {
            label,
            len: raw.len(),
        });
    }
    let mut chars = raw.chars();
    let first = chars.next().expect("the label is not empty");
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(CacheLabelError::BadCharacter { label, ch: first });
    }
    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && !matches!(ch, '.' | '_' | '-') {
            return Err(CacheLabelError::BadCharacter { label, ch });
        }
    }
    Ok(raw.to_owned())
}

/// Which decoder produced a derived entry: an engine-authored label such
/// as `zbd-texture` or `gamez-mesh`.
///
/// The id is part of the cache identity, so a renamed or replaced decoder
/// never reads an entry written under another decoder's name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DecoderId(String);

impl DecoderId {
    /// Validates and wraps a decoder id label.
    pub fn new(label: &str) -> Result<Self, CacheLabelError> {
        cache_label("decoder id", label).map(Self)
    }

    /// The label as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DecoderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The version of the normalized intermediate representation a converter
/// emits.
///
/// Bumped whenever the IR schema changes in a way an old entry cannot
/// satisfy; entries recorded under another IR version are rebuilt, never
/// reinterpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IrVersion(pub u32);

impl fmt::Display for IrVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ir#{}", self.0)
    }
}

/// Which converter produced a derived entry: the decoder identity, the
/// decoder's own version and the IR schema version it wrote under.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConverterVersion {
    /// Which decoder ran.
    pub decoder: DecoderId,
    /// The decoder's own version.
    pub decoder_version: u32,
    /// The IR schema version the entry was written under.
    pub ir: IrVersion,
}

/// One conversion option: a validated `name=value` pair.
///
/// Options are part of the cache identity, so the same source converted
/// with `mipmaps=full` and `mipmaps=none` produces two entries, and a
/// changed option set invalidates exactly the entries it describes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConversionOption {
    /// The option name.
    pub name: String,
    /// The option value.
    pub value: String,
}

impl ConversionOption {
    /// Validates and builds one option.
    pub fn new(name: &str, value: &str) -> Result<Self, CacheLabelError> {
        Ok(Self {
            name: cache_label("conversion option name", name)?,
            value: cache_label("conversion option value", value)?,
        })
    }
}

impl fmt::Display for ConversionOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.name, self.value)
    }
}

/// Why a [`ConversionOptions`] set was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionsError {
    /// One option failed label validation.
    Label(CacheLabelError),
    /// The same option name appeared twice, which would make the set
    /// depend on which occurrence a reader honours.
    DuplicateName {
        /// The repeated name.
        name: String,
    },
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Label(error) => write!(f, "{error}"),
            Self::DuplicateName { name } => {
                write!(f, "conversion option {name:?} is declared twice")
            }
        }
    }
}

impl std::error::Error for OptionsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Label(error) => Some(error),
            Self::DuplicateName { .. } => None,
        }
    }
}

/// The normalized conversion-option set of a derivation.
///
/// Options are sorted by name at construction and a repeated name is
/// refused, so `[(a,x),(b,y)]` and `[(b,y),(a,x)]` are one identity and an
/// ambiguous set cannot be constructed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ConversionOptions(Vec<ConversionOption>);

impl ConversionOptions {
    /// An empty option set.
    pub fn none() -> Self {
        Self(Vec::new())
    }

    /// Builds a normalized set: sorted by name, duplicates refused.
    pub fn new(mut options: Vec<ConversionOption>) -> Result<Self, OptionsError> {
        options.sort();
        for pair in options.windows(2) {
            if pair[0].name == pair[1].name {
                return Err(OptionsError::DuplicateName {
                    name: pair[0].name.clone(),
                });
            }
        }
        Ok(Self(options))
    }

    /// Parses and validates `name=value` spellings in one call.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Result<Self, OptionsError> {
        let options = pairs
            .iter()
            .map(|(name, value)| ConversionOption::new(name, value))
            .collect::<Result<_, _>>()
            .map_err(OptionsError::Label)?;
        Self::new(options)
    }

    /// The options in canonical order.
    pub fn as_slice(&self) -> &[ConversionOption] {
        &self.0
    }
}

/// Feeds one length-prefixed field into the canonical encoding, so no two
/// field sequences can collide on a shared byte boundary.
fn put_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn put_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(&value.to_le_bytes());
}

/// The identity hash of one source input: where its bytes live and what
/// they hash to.
///
/// This is the "source span hash" of spec F15's deliverable line, taken
/// over every field of the [`SourceSpan`]: installation, container,
/// member, range and member digest. Two spans differing in any field are
/// different inputs, so a remapped member or a moved range rebuilds the
/// derived asset instead of trusting an entry whose name merely matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpanHash(ContentHash);

// `ContentHash` carries no ordering; the span hash orders by its digest
// bytes so `CacheKey` inputs have one canonical sequence.
impl PartialOrd for SourceSpanHash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SourceSpanHash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}

impl SourceSpanHash {
    /// Hashes the span's identity fields under this module's domain.
    pub fn of(span: &SourceSpan) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(SPAN_HASH_DOMAIN);
        put_field(&mut hasher, span.install_sha256().as_bytes());
        put_field(&mut hasher, span.container_path().as_bytes());
        match span.member_key() {
            Some(member) => {
                put_u64(&mut hasher, 1);
                put_field(&mut hasher, member.as_bytes());
            }
            None => put_u64(&mut hasher, 0),
        }
        put_u64(&mut hasher, span.offset());
        put_u64(&mut hasher, span.length());
        match span.member_sha256() {
            Some(digest) => {
                put_u64(&mut hasher, 1);
                put_field(&mut hasher, digest.as_bytes());
            }
            None => put_u64(&mut hasher, 0),
        }
        Self(hasher.finalize())
    }

    /// Wraps an already-computed span hash — for example one decoded from
    /// a stored cache record, which is how
    /// [`CacheKey::from_hashed_inputs`] gets its inputs back.
    pub const fn from_digest(digest: ContentHash) -> Self {
        Self(digest)
    }

    /// The digest bytes.
    pub const fn digest(&self) -> ContentHash {
        self.0
    }
}

impl fmt::Display for SourceSpanHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Why a [`CacheKey`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheKeyError {
    /// A derived asset with no inputs derives from nothing; it has no
    /// invalidation granularity and cannot be cached honestly.
    NoInputs,
}

impl fmt::Display for CacheKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputs => write!(
                f,
                "a cache key needs at least one source span input to derive from"
            ),
        }
    }
}

impl std::error::Error for CacheKeyError {}

/// The identity of one derived cache entry: installation, source inputs,
/// converter and options — plus the digest a store keys on.
///
/// `inputs` are sorted and deduplicated at construction, so the key does
/// not depend on traversal order. The recorded facets stay available for
/// diagnostics; comparisons and storage go through [`CacheKey::digest`].
#[derive(Clone, Debug)]
pub struct CacheKey {
    install: ContentHash,
    inputs: Vec<SourceSpanHash>,
    converter: ConverterVersion,
    options: ConversionOptions,
    digest: ContentHash,
}

impl CacheKey {
    /// Builds the key and its digest.
    ///
    /// # Errors
    ///
    /// [`CacheKeyError::NoInputs`] when `inputs` is empty.
    pub fn new(
        install: ContentHash,
        inputs: &[SourceSpan],
        converter: ConverterVersion,
        options: ConversionOptions,
    ) -> Result<Self, CacheKeyError> {
        if inputs.is_empty() {
            return Err(CacheKeyError::NoInputs);
        }
        let mut hashed: Vec<SourceSpanHash> = inputs.iter().map(SourceSpanHash::of).collect();
        hashed.sort();
        hashed.dedup();
        let key = Self {
            install,
            inputs: hashed,
            converter,
            options,
            digest: ContentHash::from_bytes([0; 32]),
        };
        let digest = key.compute_digest();
        Ok(Self { digest, ..key })
    }

    /// Rebuilds the key from already-hashed inputs (for example from a
    /// stored index), applying the same sort/dedup normalization.
    ///
    /// # Errors
    ///
    /// [`CacheKeyError::NoInputs`] when `inputs` is empty.
    pub fn from_hashed_inputs(
        install: ContentHash,
        inputs: &[SourceSpanHash],
        converter: ConverterVersion,
        options: ConversionOptions,
    ) -> Result<Self, CacheKeyError> {
        if inputs.is_empty() {
            return Err(CacheKeyError::NoInputs);
        }
        let mut hashed = inputs.to_vec();
        hashed.sort();
        hashed.dedup();
        let key = Self {
            install,
            inputs: hashed,
            converter,
            options,
            digest: ContentHash::from_bytes([0; 32]),
        };
        let digest = key.compute_digest();
        Ok(Self { digest, ..key })
    }

    /// The canonical encoding of this key's facets.
    fn compute_digest(&self) -> ContentHash {
        let mut hasher = Sha256::new();
        hasher.update(KEY_DIGEST_DOMAIN);
        put_field(&mut hasher, self.install.as_bytes());
        put_u64(&mut hasher, self.inputs.len() as u64);
        for input in &self.inputs {
            put_field(&mut hasher, input.digest().as_bytes());
        }
        put_field(&mut hasher, self.converter.decoder.as_str().as_bytes());
        put_u64(&mut hasher, u64::from(self.converter.decoder_version));
        put_u64(&mut hasher, u64::from(self.converter.ir.0));
        put_u64(&mut hasher, self.options.as_slice().len() as u64);
        for option in self.options.as_slice() {
            put_field(&mut hasher, option.name.as_bytes());
            put_field(&mut hasher, option.value.as_bytes());
        }
        hasher.finalize()
    }

    /// The installation this entry derives from.
    pub fn install(&self) -> ContentHash {
        self.install
    }

    /// The sorted, deduplicated source inputs this entry derives from.
    ///
    /// Per-input granularity is the invalidation contract: a store that
    /// rewrites one source span invalidates exactly the entries whose
    /// `inputs` contain that span's hash, nothing else (spec F15 AC03).
    pub fn inputs(&self) -> &[SourceSpanHash] {
        &self.inputs
    }

    /// Whether this entry derives from `input`.
    pub fn depends_on(&self, input: SourceSpanHash) -> bool {
        self.inputs.contains(&input)
    }

    /// The converter identity the entry was produced under.
    pub fn converter(&self) -> &ConverterVersion {
        &self.converter
    }

    /// The conversion options the entry was produced with.
    pub fn options(&self) -> &ConversionOptions {
        &self.options
    }

    /// The store-facing digest of the whole identity.
    pub fn digest(&self) -> ContentHash {
        self.digest
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest
    }
}

impl Eq for CacheKey {}

impl std::hash::Hash for CacheKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.digest.hash(state);
    }
}
