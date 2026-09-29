//! Acceptance stage F08-B.02: the Crimson Skies ZBD texture package variant
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
//! `### F08-B`, AC01/AC02 and non-negotiables #1–#4).
//!
//! Every package here is built byte by byte in this file from the layout
//! recorded in `docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md`:
//! newly authored synthetic content, no original game data, no
//! `CS_GAME_DIR` access. Expected texels are written per coordinate,
//! independently of the builders.

use cs_formats::ParseErrorKind;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::zbd::{
    FLAG_BYTES_PER_PIXEL2, FLAG_FULL_ALPHA, FLAG_GLOBAL_PALETTE, FLAG_HAS_ALPHA, FLAG_NO_ALPHA,
    ZBD_TEXTURE_ENTRY_BYTES, ZBD_TEXTURE_HEADER_BYTES, ZBD_TEXTURE_INFO_BYTES,
    ZBD_TEXTURE_ROW_ORDER,
};
use cs_formats::texture::{
    AlphaSource, AlphaTest, ColorSpace, DecodedFormat, Extent, Palette, PixelFormat, RowOrder,
    TextureError, ZbdAlpha, ZbdStretch, ZbdTextureEntryError, ZbdTextureError, ZbdTexturePackage,
    read_zbd_textures,
};
use cs_types::evidence::ClaimStatus;

const CONTAINER: &str = "synthetic/f08_b_02_texture_package";

// RGB565 words whose two stored bytes differ, so a byte swap is visible.
const RED: u16 = 0xF800;
const GREEN: u16 = 0x07E0;
const BLUE: u16 = 0x001F;
const YELLOW: u16 = 0xFFE0;
const CYAN: u16 = 0x07FF;
const MAGENTA: u16 = 0xF81F;

/// `NO_ALPHA`, as retail direct-color textures store it (0x5).
const OPAQUE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_NO_ALPHA;
/// `HAS_ALPHA` (0x3).
const SIMPLE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_HAS_ALPHA;
/// `HAS_ALPHA | FULL_ALPHA` (0xb).
const FULL: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_HAS_ALPHA | FLAG_FULL_ALPHA;
/// Bits 5 and 7 as retail palette textures carry them (0xa0).
const RUNTIME: u32 = 0xA0;

/// One authored texture: the words a package stores for it.
#[derive(Clone)]
struct Tex {
    name: &'static str,
    palette_index: i32,
    flags: u32,
    width: u16,
    height: u16,
    zero08: u32,
    stretch: u16,
    /// RGB565 texels (direct color) in stored order; empty for a palette
    /// texture.
    words: Vec<u16>,
    /// Palette indices in stored order; empty for direct color.
    indices: Vec<u8>,
    alpha: Vec<u8>,
    palette: Vec<u16>,
}

impl Tex {
    fn direct(name: &'static str, flags: u32, width: u16, height: u16, words: &[u16]) -> Self {
        Self {
            name,
            palette_index: -1,
            flags,
            width,
            height,
            zero08: 0,
            stretch: 0,
            words: words.to_vec(),
            indices: Vec::new(),
            alpha: Vec::new(),
            palette: Vec::new(),
        }
    }

    fn indexed(
        name: &'static str,
        flags: u32,
        width: u16,
        height: u16,
        indices: &[u8],
        palette: &[u16],
    ) -> Self {
        Self {
            indices: indices.to_vec(),
            palette: palette.to_vec(),
            ..Self::direct(name, flags, width, height, &[])
        }
    }

    fn with_alpha(mut self, alpha: &[u8]) -> Self {
        self.alpha = alpha.to_vec();
        self
    }

    fn body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.zero08.to_le_bytes());
        out.extend_from_slice(&(self.palette.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.stretch.to_le_bytes());
        for word in &self.words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&self.indices);
        out.extend_from_slice(&self.alpha);
        for word in &self.palette {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }
}

/// A package whose entry offsets are exactly where each texture starts.
fn package(textures: &[Tex], global_palettes: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for word in [0u32, 1, global_palettes as u32, textures.len() as u32, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let mut offset = ZBD_TEXTURE_HEADER_BYTES + textures.len() * 40 + global_palettes * 512;
    let bodies: Vec<Vec<u8>> = textures.iter().map(Tex::body).collect();
    for (texture, body) in textures.iter().zip(&bodies) {
        let mut name = [0u8; 32];
        name[..texture.name.len()].copy_from_slice(texture.name.as_bytes());
        out.extend_from_slice(&name);
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        out.extend_from_slice(&texture.palette_index.to_le_bytes());
        offset += body.len();
    }
    out.extend(std::iter::repeat_n(0x5A, global_palettes * 512));
    for body in bodies {
        out.extend_from_slice(&body);
    }
    out
}

/// Offset of entry `index`'s start-offset field.
fn start_offset_field(index: usize) -> usize {
    ZBD_TEXTURE_HEADER_BYTES + index * 40 + 32
}

fn read(bytes: &[u8]) -> Result<ZbdTexturePackage<'_>, ZbdTextureError> {
    read_zbd_textures(
        CONTAINER,
        bytes,
        &mut AllocationBudget::with_defaults(CONTAINER),
    )
}

fn budget() -> AllocationBudget {
    AllocationBudget::with_defaults(CONTAINER)
}

fn entry_error(error: &ZbdTextureError) -> &ZbdTextureEntryError {
    error.entry_error().expect("a texture error")
}

/// The asymmetric 3x2 image:
///
/// ```text
///        x=0     x=1    x=2
/// y=0    red     green  blue
/// y=1    yellow  cyan   magenta
/// ```
const EXPECTED_3X2: [(u32, u32, u16); 6] = [
    (0, 0, RED),
    (1, 0, GREEN),
    (2, 0, BLUE),
    (0, 1, YELLOW),
    (1, 1, CYAN),
    (2, 1, MAGENTA),
];

#[test]
fn accept_f08_b_02_asymmetric_non_square_565_textures_decode_every_texel_in_place() {
    let wide = Tex::direct(
        "wide",
        OPAQUE,
        3,
        2,
        &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA],
    );
    // The same six words as 2x3 are a different image.
    let tall = Tex::direct(
        "tall",
        OPAQUE,
        2,
        3,
        &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA],
    );
    let bytes = package(&[wide, tall], 0);
    let package = read(&bytes).expect("a valid package");
    assert_eq!(package.global_palette_count(), 0);
    assert_eq!(package.textures().len(), 2);

    let texture = &package.textures()[0];
    assert_eq!(texture.entry_index(), 0);
    assert_eq!(texture.name(), "wide");
    assert_eq!(texture.alpha(), ZbdAlpha::None);
    assert_eq!(texture.stretch(), ZbdStretch::None);
    let descriptor = texture.descriptor();
    assert_eq!(descriptor.extent(), Extent::new(3, 2));
    assert_eq!(descriptor.format(), PixelFormat::Rgb565);
    assert_eq!(descriptor.row_order(), RowOrder::TopDown);
    assert_eq!(ZBD_TEXTURE_ROW_ORDER, RowOrder::TopDown);
    assert_eq!(descriptor.palette(), None);
    assert!(descriptor.mips().is_empty());
    assert_eq!(descriptor.alpha_source(), AlphaSource::Opaque);
    assert_eq!(descriptor.alpha_test(), AlphaTest::Unknown);
    assert_eq!(descriptor.color_space(), ColorSpace::Unknown);
    assert_eq!(texture.stored().len(), 12);

    let image = texture.decode(&mut budget()).expect("decodes");
    assert_eq!(image.format(), DecodedFormat::Rgb565);
    assert_eq!(image.extent(), Extent::new(3, 2));
    for (x, y, word) in EXPECTED_3X2 {
        assert_eq!(image.texel565(x, y), Some(word), "texel ({x}, {y})");
    }
    // Raw words, little-endian, not expanded to 8-bit channels.
    assert_eq!(image.texel(0, 0), Some(&[0x00, 0xF8][..]));
    assert_eq!(image.texel(3, 0), None);
    assert_eq!(image.texel(0, 2), None);
    assert_eq!(image.alpha(), None);

    let tall = package.textures()[1]
        .decode(&mut budget())
        .expect("decodes");
    assert_eq!(tall.extent(), Extent::new(2, 3));
    assert_eq!(tall.texel565(1, 0), Some(GREEN));
    assert_eq!(tall.texel565(0, 1), Some(BLUE));
    assert_eq!(tall.texel565(1, 2), Some(MAGENTA));
    assert_eq!(tall.texel565(2, 0), None);
}

#[test]
fn accept_f08_b_02_local_palette_order_differs_from_index_order() {
    // Palette entries in an order unrelated to the image order.
    let palette = [MAGENTA, BLUE, 0x0000, YELLOW, RED, CYAN, GREEN];
    let indices = [4, 6, 1, 3, 5, 0];
    let bytes = package(
        &[Tex::indexed(
            "pal",
            OPAQUE | RUNTIME,
            3,
            2,
            &indices,
            &palette,
        )],
        0,
    );
    let package = read(&bytes).expect("a valid package");
    let texture = &package.textures()[0];
    assert_eq!(texture.flags(), 0xA5);
    assert_eq!(texture.runtime_flags(), 0xA0);
    let descriptor = texture.descriptor();
    assert_eq!(descriptor.format(), PixelFormat::Indexed8);
    assert_eq!(
        descriptor.palette(),
        Some(&Palette::Rgb565(palette.to_vec()))
    );
    assert_eq!(descriptor.alpha_source(), AlphaSource::Opaque);
    // The stored level is the indices only; the palette is in the
    // descriptor.
    assert_eq!(texture.stored(), &indices[..]);

    let image = texture.decode(&mut budget()).expect("decodes");
    assert_eq!(image.format(), DecodedFormat::Rgb565);
    for (x, y, word) in EXPECTED_3X2 {
        assert_eq!(image.texel565(x, y), Some(word), "texel ({x}, {y})");
    }
    assert_eq!(image.index(0, 0), Some(4));
    assert_eq!(image.index(2, 1), Some(0));
    assert_eq!(image.indices(), Some(&indices[..]));
}

#[test]
fn accept_f08_b_02_full_alpha_plane_keeps_0_and_255_at_the_edges() {
    let alpha = [0, 17, 255, 255, 128, 0];
    let direct = Tex::direct(
        "direct",
        FULL,
        3,
        2,
        &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA],
    )
    .with_alpha(&alpha);
    let indexed = Tex::indexed(
        "indexed",
        FULL | RUNTIME,
        3,
        2,
        &[2, 1, 0, 0, 1, 2],
        &[BLUE, GREEN, RED],
    )
    .with_alpha(&alpha);
    let bytes = package(&[direct, indexed], 0);
    let package = read(&bytes).expect("a valid package");

    for (texture, last) in package.textures().iter().zip([MAGENTA, RED]) {
        assert_eq!(texture.alpha(), ZbdAlpha::Full);
        assert_eq!(texture.descriptor().alpha_source(), AlphaSource::Plane);
        let image = texture.decode(&mut budget()).expect("decodes");
        assert_eq!(image.alpha(), Some(&alpha[..]), "{}", texture.name());
        assert_eq!(image.alpha_at(0, 0), Some(0));
        assert_eq!(image.alpha_at(2, 0), Some(255));
        assert_eq!(image.alpha_at(0, 1), Some(255));
        assert_eq!(image.alpha_at(2, 1), Some(0));
        assert_eq!(image.alpha_at(3, 1), None);
        // Coverage 0 does not touch the color.
        assert_eq!(image.texel565(0, 0), Some(RED));
        assert_eq!(image.texel565(2, 1), Some(last));
    }
    // The stored level is the texels followed by exactly one plane byte per
    // texel.
    assert_eq!(package.textures()[0].stored().len(), 12 + 6);
    assert_eq!(package.textures()[1].stored().len(), 6 + 6);
}

#[test]
fn accept_f08_b_02_simple_alpha_keeps_black_texels_black_and_flags_the_key() {
    let direct = Tex::direct(
        "keyed",
        SIMPLE,
        3,
        2,
        &[RED, 0x0000, BLUE, 0x0000, CYAN, MAGENTA],
    );
    let indexed = Tex::indexed("keyed_pal", SIMPLE | RUNTIME, 2, 1, &[1, 0], &[0x0000, RED]);
    let bytes = package(&[direct, indexed], 0);
    let package = read(&bytes).expect("a valid package");

    let texture = &package.textures()[0];
    assert_eq!(texture.alpha(), ZbdAlpha::Simple);
    assert_eq!(texture.alpha_basis(), ClaimStatus::ObservedTool);
    assert_eq!(
        texture.descriptor().alpha_source(),
        AlphaSource::StoredValueKey { value: 0x0000 }
    );
    let image = texture.decode(&mut budget()).expect("decodes");
    assert_eq!(
        image.alpha_source(),
        AlphaSource::StoredValueKey { value: 0x0000 }
    );
    // Black stays black and nothing is baked into a coverage plane.
    assert_eq!(image.texel565(1, 0), Some(0x0000));
    assert_eq!(image.texel565(0, 1), Some(0x0000));
    assert_eq!(image.texel565(0, 0), Some(RED));
    assert_eq!(image.alpha(), None);
    assert_eq!(image.texels().len(), 12);

    // For a palette texture the source does not establish the key.
    let texture = &package.textures()[1];
    assert_eq!(texture.alpha(), ZbdAlpha::Simple);
    assert_eq!(texture.descriptor().alpha_source(), AlphaSource::Unknown);
    let image = texture.decode(&mut budget()).expect("decodes");
    assert_eq!(image.texel565(1, 0), Some(0x0000));
    assert_eq!(image.index(1, 0), Some(0));
}

#[test]
fn accept_f08_b_02_palette_index_equal_to_palette_count_is_rejected_with_texel_coordinates() {
    let good = Tex::direct("first", OPAQUE, 1, 1, &[RED]);
    // Index 4 at stored offset 4, texel (1, 1), in a 4-entry palette.
    let bad = Tex::indexed(
        "bad",
        OPAQUE,
        3,
        2,
        &[0, 1, 2, 3, 4, 0],
        &[RED, GREEN, BLUE, CYAN],
    );
    let bytes = package(&[good.clone(), bad], 0);
    let error = read(&bytes).expect_err("index 4 is outside the 4-entry palette");
    assert_eq!(error.code(), "palette_index_out_of_range");
    assert_eq!(error.entry_index(), Some(1));
    match entry_error(&error) {
        ZbdTextureEntryError::Pixels(TextureError::PaletteIndexOutOfRange {
            offset,
            x,
            y,
            index,
            entries,
            ..
        }) => {
            assert_eq!((*offset, *x, *y, *index, *entries), (4, 1, 1, 4, 4));
        }
        other => panic!("unexpected error {other:?}"),
    }
    assert!(error.to_string().contains("texel 1,1"), "{error}");

    // The last valid index is accepted.
    let edge = Tex::indexed("edge", OPAQUE, 2, 1, &[3, 0], &[RED, GREEN, BLUE, CYAN]);
    let bytes = package(&[good, edge], 0);
    let package = read(&bytes).expect("index 3 is inside the 4-entry palette");
    let image = package.textures()[1]
        .decode(&mut budget())
        .expect("decodes");
    assert_eq!(image.texel565(0, 0), Some(CYAN));
}

#[test]
fn accept_f08_b_02_wrong_start_offset_is_rejected() {
    let a = Tex::direct("a", OPAQUE, 1, 1, &[RED]);
    let b = Tex::direct("b", OPAQUE, 1, 1, &[GREEN]);
    let mut bytes = package(&[a, b], 0);
    let field = start_offset_field(1);
    let declared = u32::from_le_bytes(bytes[field..field + 4].try_into().unwrap());
    bytes[field..field + 4].copy_from_slice(&(declared + 2).to_le_bytes());

    let error = read(&bytes).expect_err("texture 1 does not start where declared");
    assert_eq!(error.code(), "start_offset_mismatch");
    assert_eq!(error.entry_index(), Some(1));
    assert_eq!(
        entry_error(&error),
        &ZbdTextureEntryError::StartOffset {
            declared: declared + 2,
            expected: u64::from(declared),
        }
    );
}

#[test]
fn accept_f08_b_02_truncated_alpha_plane_is_rejected() {
    let texture = Tex::direct(
        "cut",
        FULL,
        3,
        2,
        &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA],
    )
    .with_alpha(&[0, 1, 2, 3, 4, 255]);
    let mut bytes = package(&[texture], 0);
    assert!(read(&bytes).is_ok());
    bytes.pop();

    let error = read(&bytes).expect_err("the alpha plane is one byte short");
    assert_eq!(error.code(), "unexpected_eof");
    assert_eq!(error.entry_index(), Some(0));
    match entry_error(&error) {
        ZbdTextureEntryError::Parse(parse) => {
            assert_eq!(parse.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(parse.field, "texture.alpha_plane");
            assert_eq!(parse.offset, bytes.len() as u64 - 5);
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn accept_f08_b_02_trailing_bytes_are_rejected() {
    let mut bytes = package(&[Tex::direct("a", OPAQUE, 1, 1, &[RED])], 0);
    let end = bytes.len() as u64;
    bytes.push(0);
    let error = read(&bytes).expect_err("one byte after the last texture");
    assert_eq!(error.code(), "trailing_bytes");
    assert_eq!(
        error,
        ZbdTextureError::TrailingBytes {
            container: CONTAINER.to_owned(),
            expected_end: end,
            observed_len: end + 1,
        }
    );
}

#[test]
fn accept_f08_b_02_unknown_or_inconsistent_flags_are_rejected() {
    let check = |flags: u32, code: &str| {
        let bytes = package(&[Tex::direct("f", flags, 1, 1, &[RED])], 0);
        let error = read(&bytes).expect_err(code);
        assert_eq!(error.code(), code, "flags {flags:#x}");
        assert_eq!(error.entry_index(), Some(0));
        error
    };
    let error = check(OPAQUE | 0x100, "unknown_flag_bits");
    assert_eq!(
        entry_error(&error),
        &ZbdTextureEntryError::UnknownFlagBits {
            offset: (ZBD_TEXTURE_HEADER_BYTES + 40) as u64,
            flags: 0x105,
            unknown: 0x100,
        }
    );
    check(OPAQUE | 0x8000_0000, "unknown_flag_bits");
    check(FLAG_NO_ALPHA, "bytes_per_pixel_flag_clear");
    check(OPAQUE | FLAG_HAS_ALPHA, "alpha_flags");
    check(OPAQUE | FLAG_FULL_ALPHA, "alpha_flags");
    check(FLAG_BYTES_PER_PIXEL2, "alpha_flags");
    check(FLAG_BYTES_PER_PIXEL2 | FLAG_FULL_ALPHA, "alpha_flags");

    // The runtime bits alone are kept raw and accepted.
    let bytes = package(&[Tex::direct("f", OPAQUE | 0xE0, 1, 1, &[RED])], 0);
    let package = read(&bytes).expect("runtime bits are known");
    assert_eq!(package.textures()[0].runtime_flags(), 0xE0);
}

#[test]
fn accept_f08_b_02_global_palette_texture_is_explicitly_unsupported() {
    let mut texture = Tex::indexed("global", OPAQUE | FLAG_GLOBAL_PALETTE, 2, 1, &[0, 1], &[]);
    texture.palette_index = 0;
    // A global texture stores a palette count but no local palette; build
    // the count by hand.
    let mut bytes = package(&[texture.clone()], 1);
    let info = ZBD_TEXTURE_HEADER_BYTES + 40 + 512;
    bytes[info + 12..info + 14].copy_from_slice(&2u16.to_le_bytes());
    let error = read(&bytes).expect_err("global palettes are not supported");
    assert_eq!(error.code(), "global_palette_unsupported");
    assert_eq!(
        entry_error(&error),
        &ZbdTextureEntryError::GlobalPaletteUnsupported {
            flags: OPAQUE | FLAG_GLOBAL_PALETTE,
            palette_index: 0,
            palette_count: 2,
        }
    );

    // Either half of the declaration alone is refused the same way.
    let mut flag_only = texture.clone();
    flag_only.palette_index = -1;
    let error = read(&package(&[flag_only], 1)).expect_err("flag without index");
    assert_eq!(error.code(), "global_palette_unsupported");
    let mut index_only = texture.clone();
    index_only.flags = OPAQUE;
    let error = read(&package(&[index_only], 1)).expect_err("index without flag");
    assert_eq!(error.code(), "global_palette_unsupported");

    // An index beyond the declared global palettes is a table error.
    let mut beyond = texture;
    beyond.palette_index = 1;
    let error = read(&package(&[beyond], 1)).expect_err("only palette 0 exists");
    assert_eq!(error.code(), "entry_palette_index_range");
}

#[test]
fn accept_f08_b_02_duplicate_names_are_kept_with_their_entry_index() {
    let bytes = package(
        &[
            Tex::direct("sky", OPAQUE, 1, 1, &[RED]),
            Tex::direct("sea", OPAQUE, 1, 1, &[GREEN]),
            Tex::direct("sky", OPAQUE, 1, 1, &[BLUE]),
        ],
        0,
    );
    let package = read(&bytes).expect("duplicate names are valid");
    let names: Vec<_> = package.textures().iter().map(|t| t.name()).collect();
    assert_eq!(names, ["sky", "sea", "sky"]);
    let skies: Vec<_> = package.named("sky").collect();
    assert_eq!(skies.len(), 2);
    assert_eq!(skies[0].entry_index(), 0);
    assert_eq!(skies[1].entry_index(), 2);
    assert_ne!(skies[0].label(), skies[1].label());
    let first = skies[0].decode(&mut budget()).expect("decodes");
    let second = skies[1].decode(&mut budget()).expect("decodes");
    assert_eq!(first.texel565(0, 0), Some(RED));
    assert_eq!(second.texel565(0, 0), Some(BLUE));
}

#[test]
fn accept_f08_b_02_header_and_info_fields_are_checked() {
    let valid = package(&[Tex::direct("a", OPAQUE, 1, 1, &[RED])], 0);
    for (offset, value, field) in [
        (0, 1u32, "header.zero00"),
        (4, 0, "header.has_entries"),
        (8, u32::MAX, "header.global_palette_count"),
        (12, 0, "header.texture_count"),
        (16, 1, "header.zero16"),
        (20, 1, "header.zero20"),
    ] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let error = read(&bytes).expect_err(field);
        match error {
            ZbdTextureError::HeaderField {
                offset: at,
                field: f,
                ..
            } => assert_eq!((at, f), (offset as u64, field)),
            other => panic!("{field}: unexpected error {other:?}"),
        }
    }
    let error = read(&valid[..20]).expect_err("truncated header");
    assert_eq!(error.code(), "unexpected_eof");

    let mut texture = Tex::direct("a", OPAQUE, 1, 1, &[RED]);
    texture.zero08 = 7;
    assert_eq!(
        read(&package(&[texture], 0)).unwrap_err().code(),
        "info_field"
    );

    let texture = Tex::direct("a", OPAQUE, 0, 1, &[]);
    assert_eq!(
        read(&package(&[texture], 0)).unwrap_err().code(),
        "zero_dimension"
    );
}

#[test]
fn accept_f08_b_02_stretch_is_kept_raw_and_unlisted_values_are_refused() {
    for (stored, expected) in [
        (0, ZbdStretch::None),
        (1, ZbdStretch::Vertical),
        (2, ZbdStretch::Horizontal),
        (3, ZbdStretch::Both),
        (4, ZbdStretch::Unexplained(4)),
        (7, ZbdStretch::Unexplained(7)),
        (8, ZbdStretch::Unexplained(8)),
    ] {
        let mut texture = Tex::direct("s", OPAQUE, 1, 1, &[RED]);
        texture.stretch = stored;
        let bytes = package(&[texture], 0);
        let package = read(&bytes).expect("listed stretch");
        assert_eq!(package.textures()[0].stretch(), expected);
        assert_eq!(package.textures()[0].stretch_raw(), stored);
    }
    for stored in [5, 6, 9, u16::MAX] {
        let mut texture = Tex::direct("s", OPAQUE, 1, 1, &[RED]);
        texture.stretch = stored;
        let error = read(&package(&[texture], 0)).expect_err("unlisted stretch");
        assert_eq!(error.code(), "unknown_stretch");
    }
}

#[test]
fn accept_f08_b_02_name_field_must_be_terminated_ascii_with_zero_padding() {
    let valid = package(&[Tex::direct("abc", OPAQUE, 1, 1, &[RED])], 0);
    let name = ZBD_TEXTURE_HEADER_BYTES;

    let mut bytes = valid.clone();
    bytes[name + 10] = b'x';
    let error = read(&bytes).expect_err("padding after the terminator");
    assert_eq!(error.code(), "name_padding");
    assert_eq!(
        entry_error(&error),
        &ZbdTextureEntryError::NamePadding {
            offset: (name + 10) as u64
        }
    );

    let mut bytes = valid.clone();
    bytes[name + 1] = 0xE9;
    assert_eq!(read(&bytes).unwrap_err().code(), "name_not_ascii");

    let mut bytes = valid;
    bytes[name..name + 32].fill(b'a');
    assert_eq!(read(&bytes).unwrap_err().code(), "missing_terminator");
}

#[test]
fn accept_f08_b_02_decoding_is_charged_to_the_budget() {
    let bytes = package(
        &[
            Tex::direct("a", FULL, 3, 2, &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA])
                .with_alpha(&[1; 6]),
        ],
        0,
    );
    let package = read(&bytes).expect("valid");
    let texture = &package.textures()[0];
    // 6 texels * 2 bytes + 6 coverage bytes.
    let mut exact = AllocationBudget::new(CONTAINER, 18);
    texture.decode(&mut exact).expect("exact budget");
    assert_eq!(exact.remaining(), 0);
    let mut short = AllocationBudget::new(CONTAINER, 17);
    let error = texture.decode(&mut short).expect_err("one byte short");
    assert_eq!(error.code(), "allocation_budget_exceeded");
}

#[test]
fn accept_f08_b_02_descriptor_rejects_misplaced_565_alpha_sources() {
    use cs_formats::texture::{DescriptorParts, ImageDescriptor};
    let parts = |format, alpha_source| DescriptorParts {
        extent: Extent::new(3, 2),
        format,
        row_order: RowOrder::TopDown,
        palette: None,
        mips: Vec::new(),
        alpha_source,
        alpha_test: AlphaTest::Unknown,
        color_space: ColorSpace::Unknown,
    };
    let key = AlphaSource::StoredValueKey { value: 0 };
    let error = ImageDescriptor::new(parts(PixelFormat::Rgb8, key)).unwrap_err();
    assert_eq!(error.code(), "stored_value_key_format");
    let error = ImageDescriptor::new(parts(PixelFormat::Rgba8, AlphaSource::Plane)).unwrap_err();
    assert_eq!(error.code(), "alpha_plane_with_channel");
    let mut rgb565 = parts(PixelFormat::Rgb565, AlphaSource::Opaque);
    rgb565.palette = Some(Palette::Rgb565(vec![RED]));
    let error = ImageDescriptor::new(rgb565).unwrap_err();
    assert_eq!(error.code(), "palette_not_allowed");

    let descriptor = ImageDescriptor::new(parts(PixelFormat::Rgb565, AlphaSource::Plane))
        .expect("a 565 image with a plane");
    assert_eq!(descriptor.base_level_bytes(), 6 * 2 + 6);
    let descriptor = ImageDescriptor::new(parts(PixelFormat::Rgb565, key)).expect("keyed");
    assert_eq!(descriptor.base_level_bytes(), 6 * 2);
}

/// The level is the container window between the position the info block
/// ends at and the position the level's own bytes end at:
/// `Reader::window_bytes` states that bound instead of a slice re-deriving
/// it with a cast.
///
/// The window's length is what a texture's stored level holds, so these are
/// the two ends of it. A window that ran to the end of the container would be
/// *longer* than the level: for the palette texture below it would swallow
/// the palette, and for the first of the two direct-color textures it would
/// swallow the second texture's info block. Both are refused by
/// [`check_level`](cs_formats::texture) — a level that holds more than its
/// descriptor says is not a level — so the stored bytes are asserted here as
/// well as the parse, and a container cut inside the level is still refused
/// by the checked read that reached for it, at the offset it ran out at.
#[test]
fn accept_f05_g_texture_level_is_a_window_of_the_container() {
    // 1. A palette texture: the level is the index run, and the palette that
    //    follows it is not part of the level.
    let indices = [0u8, 1, 2, 3, 0, 1];
    let palette = [RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA];
    let bytes = package(
        &[Tex::indexed(
            "idx",
            OPAQUE | RUNTIME,
            3,
            2,
            &indices,
            &palette,
        )],
        0,
    );
    let parsed = read(&bytes).expect("an indexed texture reads");
    let texture = &parsed.textures()[0];
    assert_eq!(texture.stored(), &indices[..]);
    assert_eq!(texture.stored().len(), indices.len());
    assert_eq!(
        texture.descriptor().base_level_bytes(),
        u64::from(indices.len() as u32)
    );

    // 2. Two direct-color textures: the first level ends where the second
    //    info block begins.
    let first = Tex::direct(
        "first",
        OPAQUE,
        3,
        2,
        &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA],
    );
    let second = Tex::direct("second", OPAQUE, 2, 2, &[RED, GREEN, BLUE, YELLOW]);
    let bytes = package(&[first, second], 0);
    let first_start = ZBD_TEXTURE_HEADER_BYTES + 2 * ZBD_TEXTURE_ENTRY_BYTES;
    let first_level = first_start + ZBD_TEXTURE_INFO_BYTES;
    let second_start = first_level + 3 * 2 * 2;
    let second_level = second_start + ZBD_TEXTURE_INFO_BYTES;
    for (index, expected) in [(0usize, first_start), (1, second_start)] {
        let declared =
            u32::from_le_bytes(bytes[start_offset_field(index)..][..4].try_into().unwrap());
        assert_eq!(
            declared as usize, expected,
            "entry {index} names where the texture starts"
        );
    }
    let parsed = read(&bytes).expect("two direct-color textures read");
    for (index, (at, expected)) in [(0usize, (first_level, 12)), (1, (second_level, 8))] {
        let stored = parsed.textures()[index].stored();
        assert_eq!(
            stored.len(),
            expected,
            "texture {index} holds its own bytes"
        );
        assert_eq!(
            stored.as_ptr(),
            bytes[at..].as_ptr(),
            "texture {index} is a borrow of the container at its level"
        );
    }

    // 3. A container cut inside the level: the checked read that reached for
    //    the texels refuses it, reported at the position it started from, with
    //    the count it needed and the count it found.
    let texture = Tex::direct(
        "cut",
        FULL,
        3,
        2,
        &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA],
    )
    .with_alpha(&[0, 1, 2, 3, 4, 255]);
    let whole = package(&[texture], 0);
    let level_start = ZBD_TEXTURE_HEADER_BYTES + ZBD_TEXTURE_ENTRY_BYTES + ZBD_TEXTURE_INFO_BYTES;
    assert_eq!(level_start, 80, "header, entry and info block");
    assert!(read(&whole).is_ok(), "the whole package reads");
    for available in [0usize, 3, 11] {
        let mut bytes = whole.clone();
        bytes.truncate(level_start + available);
        let error = read(&bytes).expect_err("the texel run is cut short");
        assert_eq!(
            error.code(),
            "unexpected_eof",
            "{available} bytes available"
        );
        assert_eq!(error.entry_index(), Some(0));
        match entry_error(&error) {
            ZbdTextureEntryError::Parse(parse) => {
                assert_eq!(parse.kind, ParseErrorKind::UnexpectedEof);
                assert_eq!(parse.field, "texture.texels");
                assert_eq!(parse.offset, level_start as u64);
                assert_eq!(parse.expected, "12 bytes available");
                assert_eq!(parse.observed, format!("{available} bytes available"));
            }
            other => panic!("unexpected error {other:?}"),
        }
    }
}
