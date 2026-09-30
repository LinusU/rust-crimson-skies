//! The Rgb565 texel expansion policy and the coverage-key policy for the
//! renderer adapter (task #408, `F17-B-followup-rgb565-expansion`; shared
//! contract `docs/contracts/IDENTITY-CONTENT.md`; decided in
//! `docs/findings/2026-09-30-f17-b-rgb565-expansion-and-coverage-keys.md`).
//!
//! `cs_content::textures::TextureUpload` puts
//! [`cs_content::textures::PresentationUnknown::Rgb565Expansion`] on every
//! row whose stored texels are 16-bit packed words, and the F17-B adapter
//! refused those rows instead of choosing an expansion. The refusal was
//! correct while nothing had decided it, and it is expensive: **every**
//! texture the installation stores is 565-backed, directly or through a 565
//! palette, so the refusal covers the whole texture set. This module is the
//! decision.
//!
//! # Decision 1: the expansion is [`ClaimStatus::Designed`], not an
//! # F17-D evidence gate
//!
//! The bit layout of the stored word is already an established project
//! claim ([`cs_formats::texture::PixelFormat::Rgb565`]: five red bits at
//! 15..=11, six green at 10..=5, five blue at 4..=0). Given that layout,
//! "widen an *n*-bit unsigned channel to eight bits" has one standard
//! answer and no content-dependent choice in it: **bit replication**,
//! `(level << (8 - n)) | (level >> n)`. It is what the `D3DFMT_R5G6B5`
//! hardware format means on a 2000-era card, what a 16-to-8 bit unpack
//! means in every image container of the era, and it is the only candidate
//! besides the fixed-point scale that reaches both endpoints exactly
//! ([`Rule::reaches_white`]).
//!
//! This is the same kind of claim as F17-A's class-to-phase map: a
//! new-engine design decision over a *format* property, with no original
//! behaviour asserted and no measurement needed before it can be used. It
//! is `Designed`, never `VerifiedOriginal`, and this module never claims to
//! know what the original loader emitted.
//!
//! ## What stays unmeasured, and how far it can move
//!
//! The original may have uploaded the words and let the card expand, or
//! expanded them in its own loader, and a loader expansion might have
//! rounded differently. That is unmeasured and stays an unknown for F17-D.
//! It is *bounded*, because the candidate set is enumerated here rather
//! than described: [`Rule::max_channel_deviation`] computes the worst
//! per-channel difference between two rules over **all** 32 and all 64
//! levels, so the bound is a number this module can be held to rather than
//! a claim in prose. Replication and the fixed-point scale differ by at
//! most 1/255; truncation — which cannot represent pure white at all, so
//! `0xFFFF` would come out `(248, 252, 248)` — differs by at most 7/255 on
//! a 5-bit channel and 3/255 on a 6-bit one.
//!
//! # Decision 2: both coverage keys survive, and neither needs a decision
//!
//! F08's decoder keeps the plane a key lives in, and says why: the index
//! plane is retained "so palette-key transparency can be evaluated at
//! presentation instead of being baked in here" (F08 non-negotiable #1).
//! So the key is read while the stored plane is still there and becomes a
//! coverage **byte**, which is all a GPU alpha channel ever is:
//!
//! | Stored key | Read from | Coverage byte |
//! | --- | --- | --- |
//! | [`AlphaSource::StoredValueKey`] | [`DecodedImage::texel565`] | `255` when the stored word differs from the key, `0` when it equals it |
//! | [`AlphaSource::PaletteKey`] | [`DecodedImage::index`] | `255` when the stored index differs from the key, `0` when it equals it |
//!
//! The F17-B refusal said the index plane "is not a GPU input". It is not,
//! and it does not have to be: the compare happens on the CPU while the
//! index plane is intact and only the resulting byte is uploaded. This is
//! the same composition the adapter already performs for a stored alpha
//! channel and a separate coverage plane, and it is not the forbidden
//! baking — nothing is written into a color texel.
//!
//! One consequence is worth stating because it is what makes the decision
//! safe: **a coverage key is compared on the stored value, never on the
//! expanded one, so the coverage result is independent of
//! [`ExpansionPolicy`].** [`expand_texel`] takes the two separately
//! precisely so that invariance is testable, and
//! `accept_f17_b_rgb565_a_coverage_key_does_not_depend_on_the_expansion_rule`
//! holds every non-opaque source against every rule.
//!
//! The key also stays exact where a color-first order would lose it. A
//! 565 palette may hold two entries with the same word, and then
//! `index == key` is *not* `color == palette[key]`; the index plane is
//! the only faithful source either way, and F08 keeps it for that case.
//!
//! # What this module still refuses
//!
//! Nothing here guesses. [`AlphaSource::Unknown`] has no plane to read and
//! stays a refusal ([`Rgb565PolicyError::CoverageSourceUnknown`]), as does
//! a source whose plane the image does not carry
//! ([`Rgb565PolicyError::KeyPlaneAbsent`]). The decided cases upload; the
//! undecided ones name the missing fact. Quantifying which retail rows
//! fall in which class is the retail census in the findings document, and
//! it is a separate gate: the ZBD reader declares
//! [`cs_formats::texture::ColorSpace::Unknown`] and
//! [`cs_formats::texture::AlphaTest::Unknown`] for every package
//! texture, so deciding the expansion is *necessary* for a 565 row to
//! reach a GPU and is *not* sufficient.

use std::fmt;

use cs_formats::texture::{AlphaSource, DecodedFormat, DecodedImage};
use cs_types::evidence::ClaimStatus;

/// A texel whose stored value equals the coverage key is fully transparent.
const TRANSPARENT: u8 = 0;
/// A texel the coverage key does not match is fully opaque.
const OPAQUE: u8 = u8::MAX;

/// How one stored channel level is widened to eight bits.
///
/// Enumerated rather than described: the decided rule is one of these, and
/// [`Rule::max_channel_deviation`] turns the difference between any two of
/// them into a number over the whole input domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    /// Bit replication, `(level << (8 - n)) | (level >> n)`. The decided
    /// rule: exact at both endpoints, monotone, and the meaning of the
    /// `D3DFMT_R5G6B5` hardware format.
    Replication,
    /// Fixed-point scale, `level * 255 / (2^n - 1)` rounded to nearest.
    /// Also exact at both endpoints; differs from replication by at most
    /// 1/255.
    FixedPointScale,
    /// Drop the low bits, `level << (8 - n)`. Cannot represent a full-scale
    /// channel: 31 becomes 248, 63 becomes 252. Kept because the deviation
    /// bound has to include it to be a bound.
    Truncation,
}

impl Rule {
    /// Every rule, so a sweep or a bound cannot miss one.
    pub const ALL: [Self; 3] = [Self::Replication, Self::FixedPointScale, Self::Truncation];

    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Replication => "replication",
            Self::FixedPointScale => "fixed_point_scale",
            Self::Truncation => "truncation",
        }
    }

    /// Widen a 5-bit level under this rule.
    pub const fn expand5(self, level: u8) -> u8 {
        match self {
            Self::Replication => expand5(level),
            Self::FixedPointScale => scale5(level),
            Self::Truncation => level << 3,
        }
    }

    /// Widen a 6-bit level under this rule.
    pub const fn expand6(self, level: u8) -> u8 {
        match self {
            Self::Replication => expand6(level),
            Self::FixedPointScale => scale6(level),
            Self::Truncation => level << 2,
        }
    }

    /// Widen a whole stored word under this rule: red, green, blue.
    pub const fn expand(self, word: u16) -> [u8; 3] {
        let (red, green, blue) = fields(word);
        [self.expand5(red), self.expand6(green), self.expand5(blue)]
    }

    /// Whether a full-scale channel can reach `255` under this rule.
    ///
    /// The edge that separates the two plausible rules from the lossy one.
    pub const fn reaches_white(self) -> bool {
        self.expand5(31) == u8::MAX && self.expand6(63) == u8::MAX
    }

    /// The largest absolute per-channel difference between `self` and
    /// `other`, over every 5-bit and every 6-bit level.
    ///
    /// Computed over the whole domain rather than sampled, so the number is
    /// the bound and not an estimate of it.
    pub const fn max_channel_deviation(self, other: Self) -> u8 {
        let mut worst = 0u8;
        let mut level = 0u32;
        while level < 32 {
            let a = self.expand5(level as u8);
            let b = other.expand5(level as u8);
            let gap = if a > b { a - b } else { b - a };
            if gap > worst {
                worst = gap;
            }
            level += 1;
        }
        level = 0;
        while level < 64 {
            let a = self.expand6(level as u8);
            let b = other.expand6(level as u8);
            let gap = if a > b { a - b } else { b - a };
            if gap > worst {
                worst = gap;
            }
            level += 1;
        }
        worst
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// The decided 5-bit widening: bit replication.
///
/// `(level << 3) | (level >> 2)`, so 0 stays 0, 31 becomes 255, and the
/// result is monotone in the level.
pub const fn expand5(level: u8) -> u8 {
    let level = level & 0x1F;
    (level << 3) | (level >> 2)
}

/// The decided 6-bit widening: bit replication.
///
/// `(level << 2) | (level >> 4)`, so 0 stays 0 and 63 becomes 255.
pub const fn expand6(level: u8) -> u8 {
    let level = level & 0x3F;
    (level << 2) | (level >> 4)
}

/// The 5/6/5 fields of one stored word, in stored order: red, green, blue.
///
/// The layout is the established claim of
/// [`cs_formats::texture::PixelFormat::Rgb565`]; this only reads it.
pub const fn fields(word: u16) -> (u8, u8, u8) {
    (
        ((word >> 11) & 0x1F) as u8,
        ((word >> 5) & 0x3F) as u8,
        (word & 0x1F) as u8,
    )
}

/// The decided widening of a whole stored word.
///
/// The function the renderer adapter calls; [`ExpansionPolicy::expand`]
/// calls it with the policy's rule.
pub const fn expand(word: u16) -> [u8; 3] {
    Rule::Replication.expand(word)
}

const fn scale5(level: u8) -> u8 {
    let level = level & 0x1F;
    ((level as u16 * 255 + 15) / 31) as u8
}

const fn scale6(level: u8) -> u8 {
    let level = level & 0x3F;
    ((level as u16 * 255 + 31) / 63) as u8
}

/// The expansion policy as a value that carries its evidence status.
///
/// A consumer can therefore read *why* the bytes are what they are, the
/// same way [`super::material::DeclaredClass`] carries the status of a
/// class assertion. [`ExpansionPolicy::DECIDED`] is this project's answer;
/// it is [`ClaimStatus::Designed`] and must never be
/// [`ClaimStatus::VerifiedOriginal`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpansionPolicy {
    rule: Rule,
    status: ClaimStatus,
}

/// Why an [`ExpansionPolicy`] could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpansionPolicyError(pub ClaimStatus);

impl fmt::Display for ExpansionPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "an expansion policy needs an asserting status, not {}",
            self.0.label()
        )
    }
}

impl std::error::Error for ExpansionPolicyError {}

impl ExpansionPolicy {
    /// The decided policy: bit replication, claimed as
    /// [`ClaimStatus::Designed`].
    pub const DECIDED: Self = Self {
        rule: Rule::Replication,
        status: ClaimStatus::Designed,
    };

    /// A policy that widens by `rule` and claims `status`.
    ///
    /// # Errors
    ///
    /// [`ExpansionPolicyError`] when `status` is [`ClaimStatus::Unknown`] or
    /// [`ClaimStatus::Contradicted`]: a refusal is not an expansion.
    pub fn new(rule: Rule, status: ClaimStatus) -> Result<Self, ExpansionPolicyError> {
        match status {
            ClaimStatus::Unknown | ClaimStatus::Contradicted => Err(ExpansionPolicyError(status)),
            _ => Ok(Self { rule, status }),
        }
    }

    /// The widening rule.
    pub const fn rule(&self) -> Rule {
        self.rule
    }

    /// The evidence status of the rule.
    pub const fn status(&self) -> ClaimStatus {
        self.status
    }

    /// Widen one stored word under this policy.
    pub const fn expand(&self, word: u16) -> [u8; 3] {
        self.rule.expand(word)
    }
}

/// Where the adapter reads one texel's coverage from.
///
/// This is the projection of a stored [`AlphaSource`] onto the two planes
/// the GPU never has to see. [`Self::from_source`] is the only constructor,
/// so a coverage source can never be a raw `AlphaSource` the adapter would
/// have to interpret for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageSource {
    /// No coverage anywhere: every texel is fully opaque.
    Opaque,
    /// The stored alpha channel of an Rgba8 image.
    Channel,
    /// A separate coverage plane stored after the color texels.
    StoredPlane,
    /// The stored 16-bit word of a 565 texel, compared against the key.
    StoredWord {
        /// The transparent stored word.
        key: u16,
    },
    /// The stored palette index, compared against the key.
    PaletteIndex {
        /// The transparent palette index.
        key: u8,
    },
}

impl CoverageSource {
    /// Project a stored [`AlphaSource`] onto the plane its coverage is
    /// read from.
    ///
    /// # Errors
    ///
    /// [`Rgb565PolicyError::CoverageSourceUnknown`] for
    /// [`AlphaSource::Unknown`], which names no plane: it is a refusal,
    /// not a default of "opaque".
    pub const fn from_source(source: AlphaSource) -> Result<Self, Rgb565PolicyError> {
        match source {
            AlphaSource::Opaque => Ok(Self::Opaque),
            AlphaSource::Channel => Ok(Self::Channel),
            AlphaSource::Plane => Ok(Self::StoredPlane),
            AlphaSource::StoredValueKey { value } => Ok(Self::StoredWord { key: value }),
            AlphaSource::PaletteKey { index } => Ok(Self::PaletteIndex { key: index }),
            AlphaSource::Unknown => Err(Rgb565PolicyError::CoverageSourceUnknown),
        }
    }

    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Opaque => "opaque",
            Self::Channel => "coverage_channel",
            Self::StoredPlane => "coverage_plane",
            Self::StoredWord { .. } => "coverage_stored_word_key",
            Self::PaletteIndex { .. } => "coverage_palette_index_key",
        }
    }

    /// Whether the source carries coverage a mask or a blend can use.
    pub const fn carries_coverage(self) -> bool {
        !matches!(self, Self::Opaque)
    }
}

impl fmt::Display for CoverageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoredWord { key } => write!(f, "coverage keyed on stored word 0x{key:04X}"),
            Self::PaletteIndex { key } => write!(f, "coverage keyed on palette index {key}"),
            other => f.write_str(other.code()),
        }
    }
}

/// Why a texel could not be expanded and resolved for upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rgb565PolicyError {
    /// The stored `AlphaSource` is [`AlphaSource::Unknown`], so no plane
    /// names the covered texels. This is the one coverage case that stays
    /// a refusal: the installation has rows in it, and they are counted in
    /// the findings document.
    CoverageSourceUnknown,
    /// The coverage source names a plane this image does not carry. The
    /// stored descriptor validated the pairing, so reaching this means the
    /// image and its source disagree — a bug, not a content fact.
    KeyPlaneAbsent {
        /// The source that named the absent plane.
        source: CoverageSource,
        /// The layout the image actually stores.
        format: DecodedFormat,
    },
    /// The texel is outside the image, so neither the stored word nor the
    /// index plane has a value to read.
    TexelOutOfBounds {
        /// Column, from the left.
        x: u32,
        /// Row, from the top.
        y: u32,
    },
    /// The image does not store 16-bit packed words, so there is no
    /// expansion to apply. A caller that reaches this resolved the format
    /// wrongly.
    NotRgb565 {
        /// The layout the image actually stores.
        format: DecodedFormat,
    },
}

impl Rgb565PolicyError {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::CoverageSourceUnknown => "coverage_source_unknown",
            Self::KeyPlaneAbsent { .. } => "coverage_key_plane_absent",
            Self::TexelOutOfBounds { .. } => "texel_out_of_bounds",
            Self::NotRgb565 { .. } => "not_rgb565",
        }
    }
}

impl fmt::Display for Rgb565PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoverageSourceUnknown => {
                write!(f, "the stored coverage source is not established")
            }
            Self::KeyPlaneAbsent { source, format } => write!(
                f,
                "coverage is read from {source}, which a {format:?} image does not store"
            ),
            Self::TexelOutOfBounds { x, y } => {
                write!(f, "texel ({x}, {y}) is outside the image")
            }
            Self::NotRgb565 { format } => {
                write!(f, "a {format:?} image stores no 16-bit texel word")
            }
        }
    }
}

impl std::error::Error for Rgb565PolicyError {}

/// The coverage byte of texel `(x, y)`, read from the stored plane the
/// source names.
///
/// This is the whole of Decision 2: the key is compared against the stored
/// value — the 16-bit word for [`CoverageSource::StoredWord`], the palette
/// index for [`CoverageSource::PaletteIndex`] — and the result is
/// [`u8::MAX`] or `0`. Nothing here knows about [`Rule`], so the answer
/// cannot depend on the expansion.
///
/// # Errors
///
/// [`Rgb565PolicyError::TexelOutOfBounds`] outside the image, and
/// [`Rgb565PolicyError::KeyPlaneAbsent`] when the image does not carry the
/// plane the source names.
pub fn coverage_byte(
    image: &DecodedImage,
    source: CoverageSource,
    x: u32,
    y: u32,
) -> Result<u8, Rgb565PolicyError> {
    let format = image.format();
    match source {
        CoverageSource::Opaque => {
            inside(image, x, y)?;
            Ok(OPAQUE)
        }
        CoverageSource::Channel => {
            inside(image, x, y)?;
            match image.texel(x, y) {
                Some(texel) if texel.len() > 3 => Ok(texel[3]),
                _ => Err(Rgb565PolicyError::KeyPlaneAbsent { source, format }),
            }
        }
        CoverageSource::StoredPlane => {
            inside(image, x, y)?;
            image
                .alpha_at(x, y)
                .ok_or(Rgb565PolicyError::KeyPlaneAbsent { source, format })
        }
        CoverageSource::StoredWord { key } => {
            inside(image, x, y)?;
            match image.texel565(x, y) {
                // The compare is on the stored word, so no expansion rule
                // is involved and none can change this answer.
                Some(word) => Ok(if word == key { TRANSPARENT } else { OPAQUE }),
                None => Err(Rgb565PolicyError::KeyPlaneAbsent { source, format }),
            }
        }
        CoverageSource::PaletteIndex { key } => {
            inside(image, x, y)?;
            match image.index(x, y) {
                // The index plane is F08's retained record of which palette
                // entry a texel used. A palette may map two indices to the
                // same word, so the index — not the resolved color — is what
                // "this entry is transparent" means.
                Some(index) => Ok(if index == key { TRANSPARENT } else { OPAQUE }),
                None => Err(Rgb565PolicyError::KeyPlaneAbsent { source, format }),
            }
        }
    }
}

fn inside(image: &DecodedImage, x: u32, y: u32) -> Result<(), Rgb565PolicyError> {
    let extent = image.extent();
    if x < extent.width && y < extent.height {
        Ok(())
    } else {
        Err(Rgb565PolicyError::TexelOutOfBounds { x, y })
    }
}

/// One texel of a 565 image as the adapter uploads it: red, green, blue from
/// the expansion, alpha from the coverage key.
///
/// The two halves are read from different stored planes on purpose. The
/// color comes from the 16-bit word through [`ExpansionPolicy`]; the
/// coverage comes from whatever plane [`coverage_byte`] names. Passing the
/// same `policy` therefore cannot change the alpha, which is the property
/// that makes deciding the expansion safe for the 137 keyed retail rows.
///
/// # Errors
///
/// Any [`Rgb565PolicyError`] from [`coverage_byte`], plus
/// [`Rgb565PolicyError::NotRgb565`] when the image stores no 16-bit word.
pub fn expand_texel(
    image: &DecodedImage,
    source: CoverageSource,
    policy: &ExpansionPolicy,
    x: u32,
    y: u32,
) -> Result<[u8; 4], Rgb565PolicyError> {
    inside(image, x, y)?;
    let word = image
        .texel565(x, y)
        .ok_or(Rgb565PolicyError::NotRgb565 {
            format: image.format(),
        })?;
    let alpha = coverage_byte(image, source, x, y)?;
    let [red, green, blue] = policy.expand(word);
    Ok([red, green, blue, alpha])
}

/// The four channels of a whole image, row-major from the top-left, exactly
/// as the adapter writes them into its texel buffer.
///
/// # Errors
///
/// The first [`Rgb565PolicyError`] any texel reports; the same
/// [`Rgb565PolicyError::NotRgb565`] for a non-565 image.
pub fn expand_image(
    image: &DecodedImage,
    source: CoverageSource,
    policy: &ExpansionPolicy,
) -> Result<Vec<[u8; 4]>, Rgb565PolicyError> {
    let extent = image.extent();
    let count = usize::try_from(u64::from(extent.width) * u64::from(extent.height))
        .map_err(|_| Rgb565PolicyError::NotRgb565 {
            format: image.format(),
        })?;
    let mut out = Vec::with_capacity(count);
    for y in 0..extent.height {
        for x in 0..extent.width {
            out.push(expand_texel(image, source, policy, x, y)?);
        }
    }
    Ok(out)
}

/// Whether the image stores 16-bit packed texel words, directly or through a
/// 565 palette.
///
/// Both resolve to [`DecodedFormat::Rgb565`], which is why one decision
/// covers the direct and the indexed rows; the stored color space is *not*
/// read here, because whether the GPU must linearize is a separate fact with
/// its own refusal.
pub const fn stores_texel_words(format: DecodedFormat) -> bool {
    matches!(format, DecodedFormat::Rgb565)
}
