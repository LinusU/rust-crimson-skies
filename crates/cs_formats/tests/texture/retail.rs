//! Acceptance stage F08-B: every texture variant the installation stores,
//! read and decoded through the production readers
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
//! `### F08-B`, AC02 and non-negotiables #1, #2 and #5).
//!
//! Requires `$CS_GAME_DIR` and reads it only. Nothing from the installation
//! is written or committed: the test asserts the variant census recorded in
//! `docs/findings/2026-09-28-f08-b-texture-variants.md` (counts, sizes and
//! header facts) and fails when a reader refuses, or a decode rejects, any
//! stored image. The census is a tool observation, not an evidence report;
//! the fingerprinted pixel audit against the pinned reference is F08-D.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_formats::io::AllocationBudget;
use cs_formats::texture::{
    AlphaSource, Extent, PixelFormat, RowOrder, ZbdAlpha, looks_like_bmp, read_bmp, read_tga,
    read_zbd_textures,
};

fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR must point at the original installation for this test");
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// Every `texture*.zbd`, `rtexture*.zbd` and `rimage.zbd` below `dir`, in a
/// stable order.
fn texture_archives(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            texture_archives(&path, out);
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let is_package = name.ends_with(".zbd")
            && (name.starts_with("texture")
                || name.starts_with("rtexture")
                || name == "rimage.zbd");
        if is_package {
            out.push(path);
        }
    }
}

fn read_file(dir: &Path, relative: &str) -> Vec<u8> {
    let path = dir.join(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f08_b_retail_zbd_texture_packages_read_and_decode_with_the_recorded_census() {
    let dir = game_dir();
    let mut archives = Vec::new();
    texture_archives(&dir.join("ZBD"), &mut archives);
    assert_eq!(archives.len(), 49, "texture archives under ZBD/");

    let mut textures = 0usize;
    let mut kinds: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut stretch: BTreeMap<u16, usize> = BTreeMap::new();
    let mut non_square = 0usize;
    let mut non_power_of_two = 0usize;
    for path in &archives {
        let container = path.strip_prefix(&dir).unwrap().display().to_string();
        let bytes = std::fs::read(path).unwrap();
        let mut budget = AllocationBudget::with_defaults(&container);
        let package = read_zbd_textures(&container, &bytes, &mut budget)
            .unwrap_or_else(|error| panic!("{container}: {error}"));
        assert_eq!(package.global_palette_count(), 0, "{container}");
        for texture in package.textures() {
            textures += 1;
            let descriptor = texture.descriptor();
            assert!(descriptor.mips().is_empty(), "{}", texture.label());
            assert_eq!(descriptor.row_order(), RowOrder::TopDown);
            let color = match descriptor.format() {
                PixelFormat::Rgb565 => "565",
                PixelFormat::Indexed8 => "palette",
                other => panic!("{}: unexpected format {other:?}", texture.label()),
            };
            *kinds.entry((color, texture.alpha().as_str())).or_default() += 1;
            *stretch.entry(texture.stretch_raw()).or_default() += 1;
            let Extent { width, height } = descriptor.extent();
            non_square += usize::from(width != height);
            non_power_of_two += usize::from(!width.is_power_of_two() || !height.is_power_of_two());

            let image = texture
                .decode(&mut AllocationBudget::with_defaults(texture.label()))
                .unwrap_or_else(|error| panic!("{}: {error}", texture.label()));
            assert_eq!(image.extent(), descriptor.extent());
            // Full alpha is a separate plane, never folded into the color.
            assert_eq!(
                image.alpha().map(<[u8]>::len),
                (texture.alpha() == ZbdAlpha::Full).then(|| (width * height) as usize),
                "{}",
                texture.label()
            );
        }
    }

    assert_eq!(textures, 37_004);
    let expected_kinds: BTreeMap<(&str, &str), usize> = [
        (("565", "none"), 15_399),
        (("565", "full"), 15_343),
        (("565", "simple"), 137),
        (("palette", "none"), 3_089),
        (("palette", "full"), 3_014),
        (("palette", "simple"), 22),
    ]
    .into_iter()
    .collect();
    assert_eq!(kinds, expected_kinds);
    let expected_stretch: BTreeMap<u16, usize> = [
        (0, 34_820),
        (1, 168),
        (2, 294),
        (3, 366),
        (4, 648),
        (7, 576),
        (8, 132),
    ]
    .into_iter()
    .collect();
    assert_eq!(stretch, expected_stretch);
    assert_eq!(non_square, 11_406);
    assert_eq!(non_power_of_two, 217);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f08_b_retail_conventional_bmps_are_recognised_by_content() {
    let dir = game_dir();
    // Neither name ends in `.bmp`: the content decides.
    for (relative, bits_per_pixel) in [("00000409.016", 4), ("00000409.256", 8)] {
        let bytes = read_file(&dir, relative);
        assert!(looks_like_bmp(&bytes), "{relative}");
        let mut budget = AllocationBudget::with_defaults(relative);
        let image =
            read_bmp(relative, &bytes, &mut budget).unwrap_or_else(|e| panic!("{relative}: {e}"));
        assert_eq!(image.bits_per_pixel(), bits_per_pixel, "{relative}");
        let descriptor = image.descriptor();
        assert_eq!(descriptor.extent(), Extent::new(640, 480), "{relative}");
        assert_eq!(descriptor.format(), PixelFormat::Indexed8);
        assert_eq!(descriptor.row_order(), RowOrder::BottomUp, "{relative}");
        assert!(descriptor.mips().is_empty());
        assert_eq!(descriptor.alpha_source(), AlphaSource::Opaque);
        let decoded = image
            .decode(&mut AllocationBudget::with_defaults(relative))
            .unwrap_or_else(|e| panic!("{relative}: {e}"));
        assert_eq!(decoded.extent(), Extent::new(640, 480));
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f08_b_retail_tgas_keep_the_declared_alpha_bits() {
    let dir = game_dir();
    // (file, image type, extent, alpha bits, alpha source). arial8.tga is
    // the alpha edge: 32 bpp that declares no alpha bits.
    let cases = [
        (
            "GOSDATA/ASSETS/GRAPHICS/font.tga",
            2,
            Extent::new(128, 128),
            8,
            AlphaSource::Channel,
        ),
        (
            "GOSDATA/ASSETS/GRAPHICS/arial8.tga",
            10,
            Extent::new(256, 256),
            0,
            AlphaSource::Unknown,
        ),
    ];
    for (relative, image_type, extent, alpha_bits, alpha_source) in cases {
        let bytes = read_file(&dir, relative);
        assert!(!looks_like_bmp(&bytes), "{relative}");
        let mut budget = AllocationBudget::with_defaults(relative);
        let image =
            read_tga(relative, &bytes, &mut budget).unwrap_or_else(|e| panic!("{relative}: {e}"));
        assert_eq!(image.image_type(), image_type, "{relative}");
        assert_eq!(image.pixel_depth(), 32, "{relative}");
        assert_eq!(image.alpha_bits(), alpha_bits, "{relative}");
        let descriptor = image.descriptor();
        assert_eq!(descriptor.extent(), extent, "{relative}");
        assert_eq!(descriptor.format(), PixelFormat::Rgba8);
        assert_eq!(descriptor.row_order(), RowOrder::BottomUp, "{relative}");
        assert!(descriptor.mips().is_empty());
        assert_eq!(descriptor.alpha_source(), alpha_source, "{relative}");
        let decoded = image
            .decode(&mut AllocationBudget::with_defaults(relative))
            .unwrap_or_else(|e| panic!("{relative}: {e}"));
        assert_eq!(decoded.extent(), extent);
        assert_eq!(decoded.alpha_source(), alpha_source);
    }
}
