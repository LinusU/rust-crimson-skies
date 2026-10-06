//! The original's font sources as measured: the ten `rimage.zbd` bitmap
//! fonts, the two loose TGAs that are **not** fonts, and the OS font
//! `gfont3d`/`print3d` resolves to (Rally #466, `F51-FONTCELL`).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-D`'s follow-up. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! # Where every number here comes from
//!
//! The owner supplied a decrypted image of the executable
//! (`$CS_GAME_DIR/crimson.decrypted.exe`, SHA-256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`)
//! and had it analysed statically; the owner note of 2026-10-05 on Rally task
//! #466 records the addresses below. This module commits **only** addresses,
//! ids, sizes, counts and behaviour — never the image, never decompiled code,
//! never a glyph pixel. Static code evidence is not a run of the original, so
//! nothing here is `verified_original`.
//!
//! * **`font.tga`/`arial8.tga` are never read as fonts.** `GLOBALS.SCRIPT`
//!   declares `font3d gfont3d = "assets\\graphics\" "arial8.tga"`; the GOS
//!   interpreter instantiates it through `gos_LoadFont` at `0x5ce9e0`, which
//!   calls `_splitpath` and passes **only the base name** to `0x530700` and
//!   never opens the path (`0x5ce9e0`–`0x5cea34` contains no file, ROF or
//!   reader call). `0x530700` then looks the base name up case-sensitively in
//!   the Windows-font registry `0x7581e0`, which `fonts.zrd` → `WINDOWS_FONTS`
//!   builds through `0x52fc00`, and `0x530360` calls `CreateFontIndirectA`.
//!   Neither file's name appears anywhere in the executable (byte search), so
//!   they are unused data in this build: [`UNUSED_FONT_TGA_REASON`] is the
//!   audit's recorded verdict for them.
//! * **The real bitmap fonts** are the ten names in `fonts.zrd` `FONTS`
//!   ([`ORIGINAL_BITMAP_FONT_NAMES`], font id = position, 0..9), loaded by
//!   `0x5345c0` and fetched by the texture lookup `0x531b60(name)`, which
//!   resolves into `ZBD/rimage.zbd`. The image is scanned by `0x534750`
//!   (column test `0x534830`), measured by `0x534890` and drawn by `0x5bfad0`
//!   (wrapped draw `0x5bfb90`).
//! * **The colour key** is set to `0` by zImgInit `0x52fa50`, so a **separator
//!   column** is a column whose pixels all hold the stored value
//!   [`COLOR_KEY`].
//! * **The mapping** is cell `c - 0x21`: cell 0 is `'!'`, cell 93 is `'~'`.
//!   `' '` advances by the average advance and draws nothing, `'\r'` is
//!   ignored, `'\n'` starts a new line, and a `c` whose `c - 0x21` is outside
//!   `[0, 0x5f)` draws **cell 0 (`'!'`)** instead. `char` is signed in the
//!   original, so every byte `>= 0x80` renders as `'!'`: that is the measured
//!   answer to F51 non-negotiable 3 — a missing glyph is a visible `'!'`, not
//!   an invisible gap ([`MISSING_GLYPH_SUBSTITUTION`]).
//! * **The scan** walks the columns left to right: skip separator columns to
//!   the glyph start `x0`, advance over non-separator columns to the first
//!   separator after the glyph `x1`, take the cell `[x0, x1 + 1)` ×
//!   `[0, height - 1]` (it includes one trailing separator column) and resume
//!   the scan at `x1 + gap/2`. Every resume point inside that separator run
//!   skips to the same next glyph column, so the cells are exactly the maximal
//!   runs of non-separator columns, each extended by one separator column.
//!   The scan must find [`CELL_COUNT`] cells so the routine returns
//!   [`CHARACTER_COUNT`] (`0x5f`), otherwise the original logs
//!   `Only found %d characters in font %s`.
//! * **The average advance** is `width / 95 - 1` ([`MeasuredBitmapFont::average_advance`]).
//! * **`gfont3d`/`print3d` text** is drawn by Windows, not by game data:
//!   descriptor defaults at `0x530330` are `lfCharSet = 0` (`ANSI_CHARSET`,
//!   i.e. Windows-1252), `lfOutPrecision = 4`, `lfQuality = 1`, and `fonts.zrd`
//!   may override them per font. [`gfont3d_coverage`] records that design
//!   target as the Windows-1252 assigned character set — an OS property, not
//!   an original font file.
//!
//! # What this module refuses
//!
//! * A texture that is not direct-colour RGB565 has no stored colour key to
//!   compare against, so [`measure_rimage_bitmap_fonts`] reports
//!   [`BitmapFontError::NotDirectColor`] instead of inventing one.
//! * A declared font the package does not hold, or holds twice, is reported
//!   ([`RimageBitmapFonts::missing`], [`BitmapFontError::AmbiguousName`]), never
//!   skipped.
//! * A character in `'!'`..`'~'` whose cell was not found stays out of the
//!   [`GlyphCoverage`] and is listed in [`MeasuredBitmapFont::unresolved`]; a
//!   cell no character of the original's bound maps to is counted in
//!   [`MeasuredBitmapFont::stray_cells`]. Both are reported to the audit as
//!   named blockers rather than silently claimed covered.

use cs_content::localization::GlyphCoverage;
use cs_content::textures::folded_texture_name;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{DecodedFormat, DecodedImage, PixelFormat, read_zbd_textures};

/// The container label used for every read of the shared image archive.
pub const RIMAGE_CONTAINER: &str = "ZBD/rimage.zbd";

/// The ten bitmap fonts `fonts.zrd` `FONTS` declares, in declaration order:
/// the original's font id is the position in this list, `0..9`
/// (`0x5345c0`), and an empty slot falls back to font 0 (`0x534560`).
///
/// The spellings are `fonts.zrd`'s own, including the two the archive does
/// not store verbatim (`5PointHUD`, `verdana_Offwhite_11`): a texture lookup
/// folds the *request* first and compares the folded spelling against the
/// archive's stored spelling byte for byte
/// ([`folded_texture_name`], the measured rule of task #689), which is why
/// [`measure_rimage_bitmap_fonts`] folds before it searches.
///
/// Source: owner note of 2026-10-05 on Rally #466, read from `fonts.zrd`
/// `FONTS` in the decrypted image.
pub const ORIGINAL_BITMAP_FONT_NAMES: [&str; 10] = [
    "lucida_console_8",
    "lucida_console_8b",
    "modern_white_12",
    "verdana_red_11",
    "verdana_white_11",
    "modern_red_12",
    "verdana_green_11",
    "5PointHUD",
    "5PointHUDBrite",
    "verdana_Offwhite_11",
];

/// The stored colour-key value a separator column is made of: zImgInit
/// (`0x52fa50`) sets the colour key to `0`, i.e. the stored word `0x0000`.
pub const COLOR_KEY: u16 = 0x0000;

/// The character cell 0 holds: the original's substitution glyph for a
/// character it cannot map.
pub const FIRST_CELL_CHAR: char = '!';

/// The code point of cell 0 (`'!'`): the mapping is cell `c - 0x21`.
pub const FIRST_CELL_CODE: u32 = 0x21;

/// The last code point a cell can be addressed by: the original accepts
/// `c - 0x21` in `[0, 0x5f)`, so the last cell a `char` reaches is `0x7f`.
///
/// Retail stores [`CELL_COUNT`] cells (`'!'`..`'~'`), so `0x7f` has no cell
/// there and is not claimed covered by any retail font.
pub const LAST_MAPPED_CODE: u32 = 0x7F;

/// The character the last mapped retail cell holds: cell 93 is `'~'`.
pub const LAST_CELL_CHAR: char = '~';

/// How many cells a font must store: `'!'`..`'~'` is 94 characters
/// (`0x5f - 1`).
pub const CELL_COUNT: usize = 94;

/// The count the original's scan requires before it stops logging
/// `Only found %d characters in font %s`: `0x5f` = the 94 cells plus the
/// space character, which has no cell of its own.
pub const CHARACTER_COUNT: usize = 95;

/// What a character outside the measured coverage draws: cell 0, `'!'`
/// (`0x5bfad0`, `0x534890`).
///
/// This is the measured form of F51 non-negotiable 3: a missing glyph is
/// counted *and* visible, never silently dropped.
pub const MISSING_GLYPH_SUBSTITUTION: char = '!';

/// Why `GOSDATA/ASSETS/GRAPHICS/font.tga` and `arial8.tga` are recorded as
/// **not used as fonts by the original**, replacing the former
/// `GlyphEvidence::Unmeasured` verdict for them.
///
/// Evidence: owner static analysis of the decrypted image, Rally #466 owner
/// note of 2026-10-05 (`gos_LoadFont` `0x5ce9e0` never opens the path; the
/// base name resolves through the `fonts.zrd` `WINDOWS_FONTS` registry
/// `0x7581e0` to `CreateFontIndirectA` at `0x530360`; no `font.tga`/`arial8`
/// string occurs in the executable). Static code evidence, never a run of the
/// original and never `verified_original`.
pub const UNUSED_FONT_TGA_REASON: &str = "not a font in the original: `font3d \"…arial8.tga\"` \
resolves by base name through gos_LoadFont (0x5ce9e0), which passes only the base name to \
0x530700 and never opens the path (0x5ce9e0–0x5cea34); 0x530700 looks the name up in the \
fonts.zrd WINDOWS_FONTS registry (0x7581e0) and 0x530360 calls CreateFontIndirectA, so no \
cell-to-character mapping exists for either loose TGA (owner static analysis of \
crimson.decrypted.exe, Rally #466 owner note 2026-10-05)";

/// One measured cell: the character the original maps it to and the column
/// range it occupies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitmapFontCell {
    /// The character this cell is drawn for (`c - 0x21`).
    pub ch: char,
    /// First column of the cell, inclusive (`x0`).
    pub start_col: u32,
    /// One past the last column of the cell, exclusive: `[x0, x1 + 1)`, so
    /// the cell includes the separator column that ends the glyph.
    pub end_col: u32,
}

/// One `rimage.zbd` bitmap font measured by the cell rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredBitmapFont {
    /// The `fonts.zrd` `FONTS` spelling the measurement was requested with.
    pub name: String,
    /// The spelling `rimage.zbd` stores the image under (the fold of `name`
    /// compared against the archive byte for byte).
    pub stored_name: String,
    /// The image width in texels.
    pub width: u32,
    /// The image height in texels — the cell height.
    pub height: u32,
    /// The cells the scan found, in column order, each mapped to its
    /// character by `c - 0x21`.
    pub cells: Vec<BitmapFontCell>,
    /// The measured coverage: the space character plus every character whose
    /// cell was found. A character with no cell is **not** in this set.
    pub coverage: GlyphCoverage,
    /// The characters of `'!'`..`'~'` whose cell was not found, in character
    /// order. They are reported, never claimed covered: the original draws
    /// `'!'` for them.
    pub unresolved: Vec<char>,
    /// How many cells the scan found beyond the [`CHARACTER_COUNT`] - 1
    /// positions a character can address. They have no established
    /// character, so they are reported rather than mapped.
    pub stray_cells: usize,
    /// The measured average advance, `width / 95 - 1`, which is what the
    /// original advances by for `' '`.
    pub average_advance: i32,
}

impl MeasuredBitmapFont {
    /// Whether every character of `'!'`..`'~'` has a measured cell and no
    /// cell was left without a character.
    #[must_use]
    pub fn is_fully_mapped(&self) -> bool {
        self.unresolved.is_empty() && self.stray_cells == 0
    }

    /// The characters of `text` this font cannot draw, counted: they render
    /// as [`MISSING_GLYPH_SUBSTITUTION`] in the original.
    #[must_use]
    pub fn missing_glyphs(&self, text: &str) -> cs_content::localization::MissingGlyphReport {
        self.coverage.missing_in(text)
    }
}

/// Every declared bitmap font measured out of one `rimage.zbd` read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RimageBitmapFonts {
    /// One record per declared font, in [`ORIGINAL_BITMAP_FONT_NAMES`] order.
    pub fonts: Vec<MeasuredBitmapFont>,
    /// Declared fonts the package does not hold, in declaration order. They
    /// are reported, never silently dropped.
    pub missing: Vec<String>,
}

impl RimageBitmapFonts {
    /// The measurement for the declared font `name`, if it was found.
    #[must_use]
    pub fn font(&self, name: &str) -> Option<&MeasuredBitmapFont> {
        self.fonts.iter().find(|font| font.name == name)
    }

    /// Whether every declared font was found and every one of them is fully
    /// mapped: no missing font, no unresolved character, no stray cell.
    #[must_use]
    pub fn is_fully_mapped(&self) -> bool {
        self.missing.is_empty() && self.fonts.iter().all(MeasuredBitmapFont::is_fully_mapped)
    }
}

/// Why a font could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BitmapFontError {
    /// The package could not be read at all, so no font was measured.
    Read {
        /// The container label.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// The declared font's name occurs more than once in the package, so the
    /// lookup would have to pick one.
    AmbiguousName {
        /// The declared font name.
        font: String,
        /// How many entries hold the folded name.
        matches: usize,
    },
    /// The image is not direct-colour RGB565, so the stored colour key a
    /// separator column is compared against cannot be measured.
    NotDirectColor {
        /// The declared font name.
        font: String,
        /// What the image stores instead.
        reason: String,
    },
    /// The image could not be decoded.
    Decode {
        /// The declared font name.
        font: String,
        /// Why the decode failed.
        reason: String,
    },
}

impl std::fmt::Display for BitmapFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { container, reason } => {
                write!(
                    f,
                    "the texture package {container} could not be read: {reason}"
                )
            }
            Self::AmbiguousName { font, matches } => write!(
                f,
                "the font {font:?} matches {matches} entries of the package, so one lookup is ambiguous"
            ),
            Self::NotDirectColor { font, reason } => write!(
                f,
                "the font {font} is not direct-colour RGB565, so its colour key is unmeasured: {reason}"
            ),
            Self::Decode { font, reason } => write!(f, "the font {font} did not decode: {reason}"),
        }
    }
}

impl std::error::Error for BitmapFontError {}

/// Reads `bytes` as a texture package and measures every declared bitmap font
/// out of it with the cell rule.
///
/// The ten names of [`ORIGINAL_BITMAP_FONT_NAMES`] are requested through
/// [`folded_texture_name`] — the original's own lookup order — and compared
/// against the archive's stored spellings. A declared font the package does
/// not hold is reported in [`RimageBitmapFonts::missing`]; a package that
/// cannot be read, an ambiguous name, a non-RGB565 image or a failed decode is
/// an error, because none of them can be answered by guessing.
///
/// # Errors
///
/// [`BitmapFontError::Read`] for a package that is not a readable texture
/// package, [`BitmapFontError::AmbiguousName`] when one requested name is
/// stored twice, [`BitmapFontError::NotDirectColor`] when an image has no
/// stored RGB565 colour key to scan against and [`BitmapFontError::Decode`]
/// when an image cannot be decoded.
pub fn measure_rimage_bitmap_fonts(bytes: &[u8]) -> Result<RimageBitmapFonts, BitmapFontError> {
    let container = RIMAGE_CONTAINER;
    let mut budget = AllocationBudget::with_defaults(container);
    let package = read_zbd_textures(container, bytes, &mut budget).map_err(|error| {
        BitmapFontError::Read {
            container: container.to_owned(),
            reason: error.to_string(),
        }
    })?;

    let mut fonts = Vec::with_capacity(ORIGINAL_BITMAP_FONT_NAMES.len());
    let mut missing = Vec::new();
    for requested in ORIGINAL_BITMAP_FONT_NAMES {
        let folded = folded_texture_name(requested);
        let matches: Vec<_> = package
            .textures()
            .iter()
            .filter(|texture| texture.name() == folded)
            .collect();
        match matches.as_slice() {
            [] => missing.push(requested.to_owned()),
            [texture] => fonts.push(measure_font(requested, texture)?),
            several => {
                return Err(BitmapFontError::AmbiguousName {
                    font: requested.to_owned(),
                    matches: several.len(),
                });
            }
        }
    }

    Ok(RimageBitmapFonts { fonts, missing })
}

/// Measures one decoded font image with the cell rule.
fn measure_font(
    requested: &str,
    texture: &cs_formats::texture::ZbdTexture<'_>,
) -> Result<MeasuredBitmapFont, BitmapFontError> {
    let mut budget = AllocationBudget::with_defaults(texture.label());
    // The measured rule compares *stored* RGB565 words against the colour
    // key, and every retail font image is direct colour (flags `0x5`, no
    // palette). What the original would do with a paletted strip is not
    // established, so one is refused rather than scanned by a guessed key.
    if texture.descriptor().format() != PixelFormat::Rgb565 {
        return Err(BitmapFontError::NotDirectColor {
            font: requested.to_owned(),
            reason: format!(
                "the colour-key rule is measured on direct-colour RGB565 images only, and this \
                 one stores {:?}",
                texture.descriptor().format()
            ),
        });
    }
    let image = texture
        .decode(&mut budget)
        .map_err(|error| BitmapFontError::Decode {
            font: requested.to_owned(),
            reason: error.to_string(),
        })?;
    let extent = image.extent();
    let cells = scan_cells(requested, &image)?;
    Ok(build_font(
        requested,
        texture.name(),
        extent.width,
        extent.height,
        cells,
    ))
}

/// The column runs of `image`, each as `(x0, x1 + 1)`.
///
/// A separator column is one whose pixels all hold [`COLOR_KEY`]; the scan
/// walks left to right, skipping separator columns to a glyph start `x0`,
/// advancing to the first separator after the glyph `x1`, taking the cell
/// `[x0, x1 + 1)` and resuming at `x1 + gap/2`. Every resume point inside the
/// separator run that follows the cell reaches the same next glyph column, so
/// the cells are the maximal runs of non-separator columns, each extended by
/// one separator column.
fn scan_cells(font: &str, image: &DecodedImage) -> Result<Vec<(u32, u32)>, BitmapFontError> {
    if image.format() != DecodedFormat::Rgb565 {
        return Err(BitmapFontError::NotDirectColor {
            font: font.to_owned(),
            reason: format!("the decoded format is {:?}", image.format()),
        });
    }
    let extent = image.extent();
    let separator =
        |x: u32| -> bool { (0..extent.height).all(|y| image.texel565(x, y) == Some(COLOR_KEY)) };

    let mut cells = Vec::new();
    let mut x = 0u32;
    while x < extent.width {
        while x < extent.width && separator(x) {
            x += 1;
        }
        if x >= extent.width {
            break;
        }
        let x0 = x;
        while x < extent.width && !separator(x) {
            x += 1;
        }
        // `x` is now the first separator column after the glyph — the `x1`
        // of the measured rule, and a resume point inside that separator run.
        cells.push((x0, (x + 1).min(extent.width)));
    }
    Ok(cells)
}

/// Maps the scanned cells to characters by `c - 0x21` and builds the measured
/// coverage: the space character (drawn as an advance) plus every character
/// whose cell was found.
fn build_font(
    requested: &str,
    stored_name: &str,
    width: u32,
    height: u32,
    cells: Vec<(u32, u32)>,
) -> MeasuredBitmapFont {
    let mut coverage = GlyphCoverage::from_chars([' ']);
    let mut mapped = Vec::with_capacity(cells.len());
    let mut stray_cells = 0usize;
    for (index, (start_col, end_col)) in cells.iter().enumerate() {
        // The original's own bound: `c - 0x21` must be in `[0, 0x5f)`, so
        // cell `index` is addressed only while `0x21 + index <= 0x7f`.
        let ch = u32::try_from(index)
            .ok()
            .and_then(|index| FIRST_CELL_CODE.checked_add(index))
            .filter(|code| *code <= LAST_MAPPED_CODE)
            .and_then(char::from_u32);
        let Some(ch) = ch else {
            stray_cells += 1;
            continue;
        };
        mapped.push(BitmapFontCell {
            ch,
            start_col: *start_col,
            end_col: *end_col,
        });
        coverage.insert(ch);
    }
    let unresolved = (FIRST_CELL_CHAR..=LAST_CELL_CHAR)
        .filter(|ch| !coverage.covers(*ch))
        .collect();
    let per_character = i32::try_from(CHARACTER_COUNT).expect("95 fits in an i32");
    let width_i32 = i32::try_from(width).unwrap_or(i32::MAX);
    MeasuredBitmapFont {
        name: requested.to_owned(),
        stored_name: stored_name.to_owned(),
        width,
        height,
        cells: mapped,
        coverage,
        unresolved,
        stray_cells,
        average_advance: width_i32 / per_character - 1,
    }
}

/// The glyph coverage of `gfont3d`/`print3d` text: the characters
/// Windows-1252 assigns, which is the design target of the OS font the
/// original resolves.
///
/// `font3d gfont3d` resolves by base name to the GDI font `arial8` of
/// `fonts.zrd` `WINDOWS_FONTS` (`face "Arial"`, `height -8`) and is rasterised
/// by Windows with `lfCharSet = 0` (`ANSI_CHARSET`) — coverage provided by the
/// user's operating system, **not** by game data, and never bundled by the
/// release (F51 non-negotiable 1). This set is the Windows-1252 assigned
/// character set: printable ASCII `0x20`–`0x7E`, the C1 positions Windows-1252
/// assigns in `0x80`–`0x9F` (the rest are undefined), and `0xA0`–`0xFF`.
///
/// It is recorded as the original's *design target*, so a layout can count
/// what a Windows-1252 resource string would need from a substitute font; it
/// is not a claim about which glyphs one particular Arial file contains.
#[must_use]
pub fn gfont3d_coverage() -> GlyphCoverage {
    const C1_ASSIGNED: [char; 27] = [
        '\u{20AC}', // 0x80 EURO SIGN
        '\u{201A}', // 0x82 SINGLE LOW-9 QUOTATION MARK
        '\u{0192}', // 0x83 LATIN SMALL LETTER F WITH HOOK
        '\u{201E}', // 0x84 DOUBLE LOW-9 QUOTATION MARK
        '\u{2026}', // 0x85 HORIZONTAL ELLIPSIS
        '\u{2020}', // 0x86 DAGGER
        '\u{2021}', // 0x87 DOUBLE DAGGER
        '\u{02C6}', // 0x88 MODIFIER LETTER CIRCUMFLEX ACCENT
        '\u{2030}', // 0x89 PER MILLE SIGN
        '\u{0160}', // 0x8A LATIN CAPITAL LETTER S WITH CARON
        '\u{2039}', // 0x8B SINGLE LEFT-POINTING ANGLE QUOTATION MARK
        '\u{0152}', // 0x8C LATIN CAPITAL LIGATURE OE
        '\u{017D}', // 0x8E LATIN CAPITAL LETTER Z WITH CARON
        '\u{2018}', // 0x91 LEFT SINGLE QUOTATION MARK
        '\u{2019}', // 0x92 RIGHT SINGLE QUOTATION MARK
        '\u{201C}', // 0x93 LEFT DOUBLE QUOTATION MARK
        '\u{201D}', // 0x94 RIGHT DOUBLE QUOTATION MARK
        '\u{2022}', // 0x95 BULLET
        '\u{2013}', // 0x96 EN DASH
        '\u{2014}', // 0x97 EM DASH
        '\u{02DC}', // 0x98 SMALL TILDE
        '\u{2122}', // 0x99 TRADE MARK SIGN
        '\u{0161}', // 0x9A LATIN SMALL LETTER S WITH CARON
        '\u{203A}', // 0x9B SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
        '\u{0153}', // 0x9C LATIN SMALL LIGATURE OE
        '\u{017E}', // 0x9E LATIN SMALL LETTER Z WITH CARON
        '\u{0178}', // 0x9F LATIN CAPITAL LETTER Y WITH DIAERESIS
    ];
    let ascii = (0x20u32..=0x7E).filter_map(char::from_u32);
    let latin1 = (0xA0u32..=0xFF).filter_map(char::from_u32);
    GlyphCoverage::from_chars(ascii.chain(C1_ASSIGNED).chain(latin1))
}
