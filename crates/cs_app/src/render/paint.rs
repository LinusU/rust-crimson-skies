//! Per-instance paint: how a committed livery reaches the GPU
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! AC03; Rally task #410, the F17-C follow-up "Decide how a per-instance
//! paint reaches the GPU"; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! F17-C keyed batches on the committed livery digest and bound the shared
//! canonical image, so the paint lived in the frame's identity, not in the
//! texels
//! (`docs/findings/2026-09-30-f17-c-profiles-and-instance-batching.md`).
//! This module is the decision that follow-up left open, and the smallest
//! production path that carries it out.
//!
//! # What the original data establishes
//!
//! * **Storage (retail-measured).** The installation stores no painted
//!   texture: `GOSDATA/ASSETS/crimson.rof` holds 184 `.bm` members under
//!   `GRAPHICS/<FACTION>/<PREFIX>_<PART>.bm`, each storing an RGB base, three
//!   grayscale paint masks and an RGBA overlay. A paint is the mask planes
//!   plus a color choice, never a baked image (F09-D,
//!   `docs/findings/2026-09-29-f09-d-stock-liveries-and-combinations.md`).
//! * **Composition (ObservedTool).** The pinned helper [S10] composes those
//!   planes into one whole RGB8 image per (member, paint), and F09-D matched
//!   the production composition to it at every texel of the library. The
//!   paint therefore reaches the texture **in the texels**: the composition
//!   is done on the CPU, not by a per-pixel selection at sample time.
//! * **The part association (observed, unresolved).** The member names are
//!   per airframe part — `BRI_WING`, `DEV_ENGINE`, `PEA_COWLING` — so a
//!   livery addresses a part of the airframe, not the whole model. Which
//!   scene node a part names is not established (F09-PREFIX, Rally #386),
//!   and nothing here resolves it.
//!
//! # The decision
//!
//! A batch's [`crate::render::batch::BatchKey`] already carries the
//! committed variant, so every instance in one batch paints the same, and
//! "a per-instance paint" reduces to "one image per variant, bound per
//! batch". At sync time a textured batch with an established paint binds
//! the composed variant image **in place of** the surface's canonical
//! image: for a livery surface those are two reads of the same texture —
//! the unpainted upload and the painted variant — and the painted one is
//! what the original's paint data describes.
//!
//! The alternatives the task named are not implemented:
//!
//! * **a shared atlas with a per-instance region** — no atlas layout,
//!   region table or packing stride exists anywhere in the observed data;
//! * **a texture-array layer** — no array stride or layer index exists in
//!   the observed data either.
//!
//! Both are `Unknown`, not wrong: nothing establishes them. The binding
//! mechanism chosen is `Designed` — how the original renderer bound a
//! composed texture is unmeasured, because no original run exists.
//!
//! # The boundaries the decision does not cross
//!
//! * **A surface that samples no image is not painted.** A batch whose
//!   upload carries no image binds nothing: the paint is a texture, and a
//!   material with no sampled image has no texture slot to put it in.
//!   Inventing one would paint surfaces the material never declared.
//! * **A composed variant is opaque.** It is RGB8 with no coverage plane
//!   (the overlay's alpha is baked by the composition). A painted surface
//!   that declares texture coverage therefore samples alpha `255`
//!   everywhere — where coverage for a painted surface would come from is
//!   `Unknown`, recorded rather than patched over with another plane.
//! * **The color space is `Inferred`.** The composed bytes are authored
//!   8-bit RGB and the F09-D reference reads them as sRGB, so the upload is
//!   `Rgba8UnormSrgb` — the same "the GPU corrects the stored values
//!   exactly once" rule [`crate::render::bevy_image`] follows. The original
//!   renderer's color handling is unmeasured.
//! * **A paint that is not composed is a refusal, not a fallback.**
//!   [`sync_frame`](crate::render::sync::sync_frame) resolves every batch's
//!   variant before writing an entity and refuses a frame whose paint the
//!   [`PaintSource`] never composed, because binding the unpainted image
//!   would draw the aircraft in a paint it never chose — exactly the
//!   failure this stage exists to prevent.
//!
//! [S10]: https://github.com/rozab/crimsonskies2blend/blob/main/set_paintjob.py

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use cs_assets::install::sha256;
use cs_content::livery::{ComposedLivery, LiveryVariantKey, LiveryVariantStore};
use cs_types::evidence::ContentHash;

use crate::livery::LiveryRuntime;
use crate::render::bevy_image::address_mode;
use crate::render::material::TextureAddress;

/// Where the consumer resolves a batch's committed paint to composed bytes.
///
/// Implemented by the two owners of a variant store: [`LiveryRuntime`], the
/// F09-C session runtime a model instance is bound through, and
/// [`LiveryVariantStore`] itself. A source that never composed `key`
/// returns `None`, and the consumer refuses the frame rather than bind an
/// unpainted image — see the module docs for why a fallback is worse than a
/// refusal.
pub trait PaintSource {
    /// The composed variant stored under `key`, when this source composed
    /// it.
    fn variant(&self, key: &LiveryVariantKey) -> Option<&ComposedLivery>;
}

impl PaintSource for LiveryRuntime {
    fn variant(&self, key: &LiveryVariantKey) -> Option<&ComposedLivery> {
        self.composed(key)
    }
}

impl PaintSource for LiveryVariantStore {
    fn variant(&self, key: &LiveryVariantKey) -> Option<&ComposedLivery> {
        self.get(key)
    }
}

/// One composed livery uploaded as a Bevy texture: the paint, in texels.
///
/// The texels are the `ComposedLivery`'s RGB8 exactly as composed, widened
/// to RGBA8 with an opaque alpha (the composition is opaque by
/// construction), sampled with the surface's declared addressing. Nothing
/// is rescaled, gamma-adjusted or blended here: the variant's bytes are the
/// whole claim.
#[derive(Debug)]
pub struct PaintUpload {
    variant: LiveryVariantKey,
    image: Image,
    fingerprint: ContentHash,
}

/// Uploads `livery` as a Bevy texture, sampling it with `address`.
///
/// Infallible because every `ComposedLivery` is already a valid image: the
/// BM reader refuses an empty extent, the composition is total over the
/// extent, and `address` is a fact the surface's render state already
/// established (a state that did not establish it was refused upstream).
pub fn upload_paint(livery: &ComposedLivery, address: TextureAddress) -> PaintUpload {
    let composite = livery.image();
    let (width, height) = (composite.width(), composite.height());
    let mut data = Vec::with_capacity(composite.rgb().len() / 3 * 4);
    for texel in composite.rgb().as_chunks::<3>().0 {
        data.extend_from_slice(texel);
        data.push(u8::MAX);
    }
    debug_assert_eq!(data.len() % 4, 0);

    let mut texture = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        // The composed bytes are authored 8-bit RGB, read as sRGB — the same
        // single-correction rule the canonical-image adapter applies. The
        // claim class is `Inferred`: the original renderer's own color
        // handling is unmeasured (see the module docs).
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    texture.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address_mode(address.u),
        address_mode_v: address_mode(address.v),
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        // One mip level, the same policy the canonical adapter holds: the
        // original's mip policy is unmeasured and generating levels is not
        // this stage's decision.
        mipmap_filter: ImageFilterMode::Nearest,
        ..ImageSamplerDescriptor::default()
    });

    let variant = *livery.key();
    let fingerprint = paint_fingerprint(&texture, variant, address);
    PaintUpload {
        variant,
        image: texture,
        fingerprint,
    }
}

/// A digest of the variant identity, the sampler state and every texel: a
/// changed color, a changed source or a changed byte of the image changes
/// this digest.
fn paint_fingerprint(
    texture: &Image,
    variant: LiveryVariantKey,
    address: TextureAddress,
) -> ContentHash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"cs/render/paint/v1\0");
    bytes.extend_from_slice(variant.digest().as_bytes());
    bytes.extend_from_slice(&texture.width().to_le_bytes());
    bytes.extend_from_slice(&texture.height().to_le_bytes());
    bytes.extend_from_slice(address.u.code().as_bytes());
    bytes.push(b':');
    bytes.extend_from_slice(address.v.code().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(
        texture
            .data
            .as_deref()
            .expect("a paint upload always has CPU-side data"),
    );
    sha256(&bytes)
}

impl PaintUpload {
    /// The variant this upload was composed for.
    pub const fn variant(&self) -> &LiveryVariantKey {
        &self.variant
    }

    /// The Bevy texture: the composed RGB8 as opaque RGBA, with the
    /// surface's declared addressing.
    pub const fn image(&self) -> &Image {
        &self.image
    }

    /// A digest of the upload: variant, extent, addressing and every texel.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }
}
