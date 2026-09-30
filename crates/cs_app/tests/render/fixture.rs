//! Shared synthetic canonical inputs for the `accept_f17_b_` tests.
//!
//! Everything here is newly authored content, built through the *production*
//! readers: a [`RawMesh`] through `cs_content::mesh::RenderMesh::build` and a
//! stored byte buffer through
//! `cs_formats::texture::decode_base_level`. No `CS_GAME_DIR`, no original
//! data, no original-behavior claim — the fixtures exist so the F17-B
//! adapters can be tested against the canonical IR they actually consume.

use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{
    AlphaSource, AlphaTest, ColorSpace, DecodedImage, DescriptorParts, Extent, ImageDescriptor,
    Palette, PaletteEntry, PixelFormat, RowOrder, decode_base_level,
};

/// The presentation questions an original mesh hands over unsettled. The
/// adapter must carry these through untouched: none of them is F17-B's to
/// answer.
pub const MESH_UNKNOWNS: [MeshPresentationUnknown; 3] = [
    MeshPresentationUnknown::FrontFaceWinding,
    MeshPresentationUnknown::UvOrigin,
    MeshPresentationUnknown::VertexColor,
];

/// The quad every mesh fixture is built from.
pub const QUAD_POSITIONS: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
];

/// A deliberately unnormalized normal: an adapter that normalized on the way
/// to the GPU would change these bits, and the test compares bit patterns.
pub const QUAD_NORMALS: [[f32; 3]; 4] = [
    [0.0, 0.0, 2.0],
    [0.0, 0.0, 2.0],
    [0.0, 0.0, 2.0],
    [0.0, 0.0, 2.0],
];

/// Deliberately outside `0..=1` on both axes: an adapter that wrapped or
/// clamped a coordinate would change these bits.
pub const QUAD_UVS: [[f32; 2]; 4] = [[-0.25, 1.5], [1.25, 1.5], [1.25, 2.5], [-0.25, 2.5]];

/// Four distinguishable per-corner colors.
pub const QUAD_COLORS: [[f32; 3]; 4] = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.5, 0.25, 0.125],
];

/// Which per-corner attributes a fixture mesh stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuadShape {
    /// Store a normal on every corner.
    pub normals: bool,
    /// Store a UV on this many corners: `0` for none, `4` for all,
    /// anything between for the partially stored case.
    pub uvs: usize,
    /// Store a color on this many corners, counted the same way.
    pub colors: usize,
}

impl QuadShape {
    /// A quad with every per-corner attribute on every corner.
    pub const fn full() -> Self {
        Self {
            normals: true,
            uvs: 4,
            colors: 4,
        }
    }

    /// A quad with no per-corner attributes at all.
    pub const fn bare() -> Self {
        Self {
            normals: false,
            uvs: 0,
            colors: 0,
        }
    }
}

/// The canonical mesh IR of one quad in material group `material`.
pub fn quad_mesh(shape: QuadShape, material: u32) -> RenderMesh {
    quad_mesh_with_colors(shape, material, QUAD_COLORS)
}

/// [`quad_mesh`] with an explicit per-corner color table, so a comparison can
/// change exactly one stored value and see it move.
pub fn quad_mesh_with_colors(shape: QuadShape, material: u32, colors: [[f32; 3]; 4]) -> RenderMesh {
    let corners = (0..4u32)
        .map(|corner| {
            let slot = corner as usize;
            RawCorner {
                position: corner,
                normal: shape.normals.then_some(corner),
                uv: (slot < shape.uvs).then_some(QUAD_UVS[slot]),
                color: (slot < shape.colors).then_some(colors[slot]),
            }
        })
        .collect();
    let mesh = RawMesh {
        positions: QUAD_POSITIONS.to_vec(),
        normals: if shape.normals {
            QUAD_NORMALS.to_vec()
        } else {
            Vec::new()
        },
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material,
            corners,
        }],
    };
    RenderMesh::build(&mesh).expect("the authored quad has a decodable outline")
}

/// The canonical mesh IR of a triangle strip whose middle triangle repeats a
/// position index, so the IR carries exactly one degenerate triangle.
pub fn degenerate_strip_mesh() -> RenderMesh {
    let corners = [0u32, 1, 2, 1, 3]
        .into_iter()
        .map(|position| RawCorner {
            position,
            normal: None,
            uv: None,
            color: None,
        })
        .collect();
    let mesh = RawMesh {
        positions: QUAD_POSITIONS.to_vec(),
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::TriangleStrip,
            raw_flags: 0,
            material: 0,
            corners,
        }],
    };
    RenderMesh::build(&mesh).expect("the authored strip has a decodable outline")
}

/// The shape of one synthetic stored image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageShape {
    /// Stored texel layout.
    pub format: PixelFormat,
    /// Where coverage comes from.
    pub alpha_source: AlphaSource,
    /// The alpha test.
    pub alpha_test: AlphaTest,
    /// The stored color space.
    pub color_space: ColorSpace,
}

impl ImageShape {
    /// A 2x2 RGBA8 image whose coverage lives in the stored alpha channel,
    /// stored as sRGB: the common case, and the one the golden scene's fence
    /// and glass sample.
    pub const fn rgba8_srgb() -> Self {
        Self {
            format: PixelFormat::Rgba8,
            alpha_source: AlphaSource::Channel,
            alpha_test: AlphaTest::Threshold(0x80),
            color_space: ColorSpace::Srgb,
        }
    }
}

/// The two stored texel rows of the synthetic image, as `(r, g, b, a)`.
///
/// The four texels are distinguishable in every channel, and exactly one of
/// them is not fully opaque, so a test can see a coverage plane that was
/// ignored and one that was applied.
pub const IMAGE_TEXELS: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [0, 255, 0, 0],
    [0, 0, 255, 255],
    [16, 32, 48, 128],
];

/// Decodes a 2x2 image with the given shape, through the production decoder.
///
/// # Panics
///
/// If the descriptor rejects the shape or the stored bytes do not match it,
/// which is a fixture bug rather than a case under test.
pub fn decoded_image(shape: ImageShape) -> DecodedImage {
    let extent = Extent {
        width: 2,
        height: 2,
    };
    let palette = match shape.format {
        PixelFormat::Indexed8 => Some(Palette::Rgb8(vec![
            PaletteEntry::new(0, 0, 0),
            PaletteEntry::new(255, 255, 255),
        ])),
        _ => None,
    };
    let descriptor = ImageDescriptor::new(DescriptorParts {
        extent,
        format: shape.format,
        row_order: RowOrder::BottomUp,
        palette,
        mips: Vec::new(),
        alpha_source: shape.alpha_source,
        alpha_test: shape.alpha_test,
        color_space: shape.color_space,
    })
    .expect("the authored descriptor is consistent");
    let stored = stored_bytes(shape, extent);
    let mut budget = AllocationBudget::with_defaults("synthetic.f17b");
    decode_base_level("synthetic.f17b", &descriptor, &stored, &mut budget)
        .expect("the authored stored bytes decode")
}

/// The stored bytes of a 2x2 image with the given shape, bottom row first.
fn stored_bytes(shape: ImageShape, extent: Extent) -> Vec<u8> {
    let texel_bytes = match shape.format {
        PixelFormat::Rgb8 => 3usize,
        PixelFormat::Rgba8 => 4,
        PixelFormat::Indexed8 => 1,
        PixelFormat::Rgb565 => 2,
    };
    // The descriptor stores bottom-up, so the first stored row is the image's
    // *last* row: writing the rows in reverse is the test for the decoder's
    // flip staying where the contract put it. The decoded image therefore
    // reads back in `IMAGE_TEXELS` order.
    let mut stored = Vec::new();
    for texel in [2, 3, 0, 1] {
        let [r, g, b, a] = IMAGE_TEXELS[texel];
        match shape.format {
            PixelFormat::Rgb8 => stored.extend_from_slice(&[r, g, b]),
            PixelFormat::Rgba8 => stored.extend_from_slice(&[r, g, b, a]),
            // Two palette entries only, so the indices are the low bit of the
            // stored red channel.
            PixelFormat::Indexed8 => stored.push(u8::from(r > 127)),
            // 5/6/5 words, stored little-endian.
            PixelFormat::Rgb565 => {
                let word = (u16::from(r >> 3) << 11) | (u16::from(g >> 2) << 5) | u16::from(b >> 3);
                stored.extend_from_slice(&word.to_le_bytes());
            }
        }
    }
    if shape.alpha_source == AlphaSource::Plane {
        // A coverage plane in the same row order: the first stored plane row
        // is the image's last row, so the second stored row holds the first
        // image row and is the one with a clear texel.
        stored.extend_from_slice(&[255, 255, 255, 0]);
    }
    let expected = u64::from(extent.width)
        * u64::from(extent.height)
        * u64::try_from(texel_bytes).expect("a texel size fits in u64")
        + u64::from(shape.alpha_source == AlphaSource::Plane)
            * u64::from(extent.width)
            * u64::from(extent.height);
    assert_eq!(
        stored.len() as u64,
        expected,
        "the authored stored buffer matches the descriptor"
    );
    stored
}
