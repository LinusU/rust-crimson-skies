//! The canonical image IR to a Bevy texture
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-B`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! `cs_formats::texture::DecodedImage` is what F08's decoder produced, and
//! `docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`
//! is explicit about where the boundary sits: "No color-space conversion, no
//! 565 channel expansion, no alpha baking and no mip generation happen here:
//! they happen once, in the renderer adapter (F17-B), which must read
//! `TextureUpload::unknowns` before it chooses anything."
//!
//! That sentence is the whole design of this module. The four choices the
//! decoder deliberately left to the adapter are made here, and each one is
//! made from an established fact or **not made at all**:
//!
//! | Choice | Made from | Refused when |
//! | --- | --- | --- |
//! | color space | `DecodedImage::color_space` | `ColorSpace::Unknown` |
//! | coverage encoding | [`crate::render::rgb565::CoverageSource`] | `Unknown`, or a key whose plane the image does not carry |
//! | alpha test | `AlphaTest` | `Unknown` on an image that carries coverage |
//! | channel layout | [`crate::render::rgb565`] | never: the expansion is decided, so a 565 image uploads |
//! | addressing | `MaterialFacts::addressing` | never declared |
//!
//! The channel-layout and coverage-encoding rows are the two this module
//! used to refuse. They are decided in [`crate::render::rgb565`] (task
//! #408, `docs/findings/2026-09-30-f17-b-rgb565-expansion-and-coverage-keys.md`):
//! a 5/6/5 word is widened by bit replication, claimed
//! `ClaimStatus::Designed`, and both coverage keys are read from the plane
//! F08's decoder already retains. The adapter *applies* that policy; it
//! does not re-derive it, and it never widens or keys a value itself.
//!
//! # The double-correction rule (spec F17 non-negotiable 3)
//!
//! "Original texture decoding and GPU sRGB sampling must not double-correct
//! colors." The stored bytes are the original's bytes and this module never
//! touches a value, so the only place a correction could happen twice is the
//! GPU's sampler. A texture whose stored values are sRGB is uploaded as
//! `Rgba8UnormSrgb`, which makes the sampler linearize exactly once; one
//! whose stored values are already linear is uploaded as `Rgba8Unorm`, which
//! makes the sampler leave them alone. Choosing between the two is the
//! adapter's job and is driven by the decoder's `ColorSpace` — never by
//! assumption, which is why `Unknown` is a refusal and not a default.
//!
//! # What the adapter does not decide
//!
//! * **No mip levels are generated.** F08 forbids generating them here and
//!   the original's mip policy is unmeasured, so the upload carries
//!   [`ImageUpload::mip_levels`] = 1 and the minification filter is plain
//!   `Linear`. Mip generation is an F17-C enhancement, and the sheet's
//!   non-negotiable 5 forbids automatic texture upscaling.
//! * **No color is clamped or rescaled.** A stored `u8` becomes the same
//!   `u8` in the same channel.
//! * **No `ZbdStretch` is applied.** Stretching a decoded image to its
//!   stated extent is a presentation question; the decoded image is already
//!   at its extent, and any resampling is a filtering decision, not a decode.

use std::fmt;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use cs_assets::install::sha256;
use cs_formats::texture::{AlphaTest, ColorSpace, DecodedImage, Extent};
use cs_types::evidence::ContentHash;

use crate::render::material::{AddressMode, TextureAddress};
use crate::render::rgb565::{self, CoverageSource, ExpansionPolicy, Rgb565PolicyError, Rule};

/// Why a canonical image was refused instead of uploaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageAdapterError {
    /// The stored color space is not established, so whether the GPU must
    /// linearize the texels is unknown. Guessing would either double-correct
    /// sRGB texels or skip the correction of linear ones.
    ColorSpaceUnknown,
    /// The variant does not establish how coverage is encoded. The stored
    /// bytes are kept, but there is no way to know which texels are covered.
    AlphaSourceUnknown,
    /// The image carries coverage but the alpha test is unestablished, so
    /// the discard threshold would have to be invented.
    AlphaTestUnknown,
    /// A key plane the image does not carry, a texel outside the image, or
    /// an image that stores no 16-bit word to widen. The decided expansion
    /// and the two decided keys are *not* here any more: they upload.
    Rgb565Policy(Rgb565PolicyError),
    /// The material never declared texture addressing, so the sampler's
    /// address modes would be invented.
    AddressModeUnknown,
}

impl ImageAdapterError {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ColorSpaceUnknown => "color_space_unknown",
            Self::AlphaSourceUnknown => "alpha_source_unknown",
            Self::AlphaTestUnknown => "alpha_test_unknown",
            Self::Rgb565Policy(error) => error.code(),
            Self::AddressModeUnknown => "address_mode_unknown",
        }
    }
}

impl fmt::Display for ImageAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ColorSpaceUnknown => {
                write!(f, "the stored color space is not established")
            }
            Self::AlphaSourceUnknown => write!(f, "the coverage encoding is not established"),
            Self::AlphaTestUnknown => write!(
                f,
                "the image carries coverage but its alpha test is not established"
            ),
            Self::Rgb565Policy(error) => write!(f, "{error}"),
            Self::AddressModeUnknown => write!(
                f,
                "the material declares no texture addressing, so the sampler would be invented"
            ),
        }
    }
}

impl std::error::Error for ImageAdapterError {}

/// Where the decoder found the coverage the upload composed into its alpha
/// channel.
///
/// The two keyed variants were refusals in the first cut of this module;
/// they are planes like the others now, read through
/// [`crate::render::rgb565::coverage_byte`] while the stored plane is still
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoveragePlane {
    /// The stored alpha channel of an RGBA8 image.
    Channel,
    /// A separate coverage plane stored after the color texels.
    Separate,
    /// A stored 16-bit texel word compared against the key.
    KeyedWord {
        /// The transparent stored word.
        key: u16,
    },
    /// A retained palette index compared against the key.
    KeyedIndex {
        /// The transparent palette index.
        key: u8,
    },
    /// No coverage anywhere: every texel is opaque.
    None,
}

impl CoveragePlane {
    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Separate => "separate",
            Self::KeyedWord { .. } => "keyed_word",
            Self::KeyedIndex { .. } => "keyed_index",
            Self::None => "none",
        }
    }
}

impl From<CoverageSource> for CoveragePlane {
    fn from(source: CoverageSource) -> Self {
        match source {
            CoverageSource::Opaque => Self::None,
            CoverageSource::Channel => Self::Channel,
            CoverageSource::StoredPlane => Self::Separate,
            CoverageSource::StoredWord { key } => Self::KeyedWord { key },
            CoverageSource::PaletteIndex { key } => Self::KeyedIndex { key },
        }
    }
}

/// One canonical image uploaded to a Bevy texture.
///
/// The image is built CPU-side; uploading it to a GPU texture is the render
/// world's job once a handle is bound. Everything the renderer needs to know
/// about *how* the texels are to be read is on this record, not inside a
/// shader.
#[derive(Debug)]
pub struct ImageUpload {
    image: Image,
    source: DecodedImage,
    extent: Extent,
    format: TextureFormat,
    color_space: ColorSpace,
    coverage: CoveragePlane,
    alpha_test: AlphaTest,
    address: TextureAddress,
    expansion: Option<Rule>,
    translucent_texels: u32,
    fingerprint: ContentHash,
}

/// Uploads `image` as a Bevy texture, sampling it with `address`.
///
/// # Errors
///
/// [`ImageAdapterError`] when a choice this upload has to make is not
/// established by the image or by the material. Every refusal names the fact
/// that is missing; none of them falls back to a default.
pub fn upload_image(
    image: &DecodedImage,
    address: Option<TextureAddress>,
) -> Result<ImageUpload, ImageAdapterError> {
    let color_space = image.color_space();
    let format = match color_space {
        ColorSpace::Srgb => TextureFormat::Rgba8UnormSrgb,
        ColorSpace::Linear => TextureFormat::Rgba8Unorm,
        ColorSpace::Unknown => return Err(ImageAdapterError::ColorSpaceUnknown),
    };
    let address = address.ok_or(ImageAdapterError::AddressModeUnknown)?;
    // `CoverageSource` is the decided projection of a stored `AlphaSource`
    // onto the plane its coverage is read from, and the only constructor of
    // one. The single failure it has is `AlphaSource::Unknown`, which keeps
    // this module's own `alpha_source_unknown` reason code.
    let coverage_source = match CoverageSource::from_source(image.alpha_source()) {
        Ok(source) => source,
        Err(Rgb565PolicyError::CoverageSourceUnknown) => {
            return Err(ImageAdapterError::AlphaSourceUnknown);
        }
        Err(error) => return Err(ImageAdapterError::Rgb565Policy(error)),
    };
    let coverage = CoveragePlane::from(coverage_source);
    let alpha_test = image.alpha_test();
    if coverage != CoveragePlane::None && alpha_test == AlphaTest::Unknown {
        return Err(ImageAdapterError::AlphaTestUnknown);
    }
    // A 565 image — directly, or through a 565 palette, which decodes to the
    // same layout — is widened by the decided policy instead of being
    // refused. `image.format()` is the decoded layout, so this is one
    // decision covering both stored variants.
    let expansion = if rgb565::stores_texel_words(image.format()) {
        Some(ExpansionPolicy::DECIDED)
    } else {
        None
    };

    let extent = image.extent();
    let width = extent.width;
    let height = extent.height;
    let mut data = Vec::with_capacity(
        usize::try_from(width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height).unwrap_or(0))
            .saturating_mul(4),
    );
    let mut translucent_texels = 0u32;
    for y in 0..height {
        for x in 0..width {
            // Every texel comes from the policy, both halves of it: the
            // color from the stored word through the decided expansion, or
            // from the stored bytes when the image stores them directly,
            // and the alpha from whichever plane the key lives in. This
            // module widens and keys nothing itself.
            let [red, green, blue, alpha] = match expansion {
                Some(policy) => rgb565::expand_texel(image, coverage_source, &policy, x, y)
                    .map_err(ImageAdapterError::Rgb565Policy)?,
                None => {
                    // The decoder's texel length is the channel count of the
                    // decoded format; the alpha source has already been
                    // validated against it, so an RGB image carries coverage
                    // only in a separate plane.
                    let texel = image
                        .texel(x, y)
                        .expect("a decoded image has a texel at every texel of its extent");
                    let alpha = rgb565::coverage_byte(image, coverage_source, x, y)
                        .map_err(ImageAdapterError::Rgb565Policy)?;
                    [texel[0], texel[1], texel[2], alpha]
                }
            };
            if alpha != u8::MAX {
                translucent_texels = translucent_texels.saturating_add(1);
            }
            data.extend_from_slice(&[red, green, blue, alpha]);
        }
    }
    debug_assert_eq!(
        data.len(),
        usize::try_from(width).unwrap_or(0) * usize::try_from(height).unwrap_or(0) * 4
    );

    let mut texture = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::default(),
    );
    texture.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address_mode(address.u),
        address_mode_v: address_mode(address.v),
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        // One mip level, so there is no level to interpolate between. The
        // original's mip policy is unmeasured; generating levels here would
        // be a presentation decision this stage does not own.
        mipmap_filter: ImageFilterMode::Nearest,
        ..ImageSamplerDescriptor::default()
    });

    let descriptor = descriptor_bytes(
        extent,
        format,
        color_space,
        coverage,
        alpha_test,
        address,
        expansion.map_or(0u8, |policy| rule_descriptor(policy.rule())),
        translucent_texels,
    );
    let fingerprint = image_fingerprint(&descriptor, &texture);
    Ok(ImageUpload {
        image: texture,
        source: image.clone(),
        extent,
        format,
        color_space,
        coverage,
        alpha_test,
        address,
        expansion: expansion.map(|policy| policy.rule()),
        translucent_texels,
        fingerprint,
    })
}

const fn address_mode(mode: AddressMode) -> ImageAddressMode {
    match mode {
        AddressMode::Repeat => ImageAddressMode::Repeat,
        AddressMode::Clamp => ImageAddressMode::ClampToEdge,
    }
}

/// Everything about an upload except its texels, in a canonical byte order.
#[allow(clippy::too_many_arguments)]
fn descriptor_bytes(
    extent: Extent,
    format: TextureFormat,
    color_space: ColorSpace,
    coverage: CoveragePlane,
    alpha_test: AlphaTest,
    address: TextureAddress,
    expansion: u8,
    translucent_texels: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    // v2 adds the expansion rule. A 565 image used to be refused, so no v1
    // fingerprint can exist for one; the version is bumped anyway because
    // the byte count changed, so a digest computed by either version can
    // never be read as the other.
    bytes.extend_from_slice(b"cs/render/bevy_image/v2\0");
    bytes.extend_from_slice(&extent.width.to_le_bytes());
    bytes.extend_from_slice(&extent.height.to_le_bytes());
    bytes.extend_from_slice(&format_descriptor(format));
    bytes.extend_from_slice(&color_space_code(color_space));
    bytes.extend_from_slice(coverage.code().as_bytes());
    bytes.push(0);
    bytes.push(expansion);
    match alpha_test {
        AlphaTest::Disabled => bytes.push(0),
        AlphaTest::Unknown => bytes.push(1),
        AlphaTest::Threshold(t) => {
            bytes.push(2);
            bytes.push(t);
        }
    }
    bytes.extend_from_slice(address.u.code().as_bytes());
    bytes.push(b':');
    bytes.extend_from_slice(address.v.code().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&translucent_texels.to_le_bytes());
    bytes
}

/// Digests the descriptor and the texel bytes, in the order the GPU reads
/// them: a change in a single stored value changes this digest.
fn image_fingerprint(descriptor: &[u8], texture: &Image) -> ContentHash {
    let mut bytes = Vec::with_capacity(descriptor.len() + 4);
    bytes.extend_from_slice(descriptor);
    bytes.extend_from_slice(
        texture
            .data
            .as_deref()
            .expect("an image built by upload_image always has CPU-side data"),
    );
    sha256(&bytes)
}

/// A stable byte per `TextureFormat`, so the fingerprint does not depend on
/// wgpu's `Debug` spelling of a variant.
const fn format_descriptor(format: TextureFormat) -> [u8; 2] {
    match format {
        TextureFormat::Rgba8UnormSrgb => *b"sr",
        TextureFormat::Rgba8Unorm => *b"ln",
        _ => *b"??",
    }
}

const fn color_space_code(color_space: ColorSpace) -> [u8; 1] {
    match color_space {
        ColorSpace::Srgb => *b"s",
        ColorSpace::Linear => *b"l",
        ColorSpace::Unknown => *b"?",
    }
}

/// A stable byte per [`Rule`], so the fingerprint does not depend on the
/// `Debug` spelling of a variant. `0` is "no expansion": the image stores
/// 8-bit channels already.
const fn rule_descriptor(rule: Rule) -> u8 {
    match rule {
        Rule::Replication => 1,
        Rule::FixedPointScale => 2,
        Rule::Truncation => 3,
    }
}

impl ImageUpload {
    /// The Bevy texture. Its texels are the stored bytes, composed with the
    /// coverage the decoder found, and its sampler is the material's
    /// declared addressing.
    pub const fn image(&self) -> &Image {
        &self.image
    }

    /// The canonical image this upload came from, for a caller that has to
    /// compare the material's declared coverage against what the image
    /// actually stores.
    pub const fn image_ref(&self) -> &DecodedImage {
        &self.source
    }

    /// Consumes the upload, returning the Bevy texture.
    #[must_use]
    pub fn into_image(self) -> Image {
        self.image
    }

    /// The decoded extent, which the upload preserves.
    pub const fn extent(&self) -> Extent {
        self.extent
    }

    /// The GPU texel format, chosen from the stored color space. This is the
    /// single place a color correction is either requested or refused.
    pub const fn format(&self) -> TextureFormat {
        self.format
    }

    /// The stored color space the format was chosen from.
    pub const fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    /// Where coverage came from and how it reached the alpha channel.
    pub const fn coverage(&self) -> CoveragePlane {
        self.coverage
    }

    /// The declared alpha test, applied by the material's mask pass.
    pub const fn alpha_test(&self) -> AlphaTest {
        self.alpha_test
    }

    /// The declared addressing, as it reached the sampler.
    pub const fn address(&self) -> TextureAddress {
        self.address
    }

    /// The widening the stored 16-bit texel words went through, or `None`
    /// when the image stores 8-bit channels already.
    ///
    /// The answer is on the upload rather than inside
    /// [`crate::render::rgb565`] so a consumer can report which rule
    /// produced these bytes without re-deriving the format.
    pub const fn expansion(&self) -> Option<Rule> {
        self.expansion
    }

    /// How many texels are **not** fully opaque after composition — the texels
    /// the declared coverage actually reaches. `0` for an image with no
    /// coverage anywhere, and the whole image for one where every texel is
    /// cut. The name is the point: a caller asking whether an image is fully
    /// covered asks whether this is *less* than the texel count, not whether it
    /// is `0`.
    pub const fn translucent_texels(&self) -> u32 {
        self.translucent_texels
    }

    /// Mip levels in the upload. Always `1`: this stage generates none.
    pub const fn mip_levels(&self) -> u32 {
        1
    }

    /// A digest of the texel bytes, the format and the sampler state.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }
}
