//! The F51-D localization audit: every declared locale, every string, every
//! media file, with what was measured and what could not be.
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-D`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! F51-A declared the typed localization records, F51-B the fit-or-scroll
//! layout and F51-C the runtime menus, HUD and subtitles share. None of them
//! answers the stage's own scenario: *audit all strings and media for each
//! declared supported original locale*. This module is that audit. It does not
//! introduce a second reader — it consumes the F12 [`StringRow`]s the production
//! `StringCatalog::read` produced, the caller's declared locale set, language
//! map and grammar, and produces one machine-readable report.
//!
//! # One image at a time, because ids are per image
//!
//! The original ships **several** PE string images (`strings.dll`,
//! `langui.dll`, `language.dll`; F12-A/F12-D), each with its own `RT_STRING`
//! table. The id space is the same numbering in each, so the same resource id
//! can name different text in different images — a cross-image `(id, locale)`
//! collision is **not** a contradiction and must not be reported as one. The
//! audit therefore takes a list of [`StringImageSource`]s and decodes each in
//! isolation; a duplicate `(id, locale)` inside *one* image is the contradiction
//! [`AuditBlocker::DuplicateRow`] names.
//!
//! # What is measured, and what is refused
//!
//! * **String coverage**, per declared locale per image:
//!   [`StringImageAudit::locales`] carries each locale's own translation count,
//!   its fallback count and the ids it cannot answer at all
//!   ([`cs_content::localization::LocaleCoverage`]).
//! * **Overflow**, per declared locale per image: every id that resolves for a
//!   locale is parsed with the declared grammar and laid out in the caller's
//!   panel, and a string that has to scroll is counted. A layout that would cover
//!   a required control is counted separately, because that is AC01 failing, not
//!   merely a long translation.
//! * **Media**, per file: its length, digest, provenance and whether the release
//!   may distribute it, plus its [`GlyphEvidence`]. Every media carries a
//!   *measured* verdict: a coverage a caller declared, the per-font coverage
//!   [`crate::text::original_font`] derives from `rimage.zbd` with the colour-key
//!   cell rule, or the recorded verdict that the original never reads the file
//!   as a font. Only a media nobody could measure stays
//!   [`GlyphEvidence::Unmeasured`] and becomes an
//!   [`AuditBlocker::UnmeasuredGlyphs`]; a font whose scan left a character
//!   unmapped or a cell unaddressed becomes a named
//!   [`AuditBlocker::BitmapFontUnmapped`] / [`AuditBlocker::BitmapFontStrayCell`],
//!   and a declared font the archive does not hold becomes
//!   [`AuditBlocker::BitmapFontMissing`], so nothing unknown is claimed covered.
//! * **Accounting**: a row in a language the map does not declare, a row whose
//!   code units did not decode, a duplicated `(id, locale)` pair and an
//!   undeclared catalog locale are each a named [`AuditBlocker`], never dropped.
//!
//! [`LocalizationAudit::is_complete`] is therefore `false` whenever anything
//! was not measured — on the original installation that is, today, the
//! overflow the audit counts against declared development metrics and any font
//! cell the scan could not map. Every such reason is named in
//! [`LocalizationAudit::blockers`], and a measured verdict (a coverage, a
//! per-font scan or "the original never reads this file as a font") is never
//! reported as an unknown. That is the honest verdict, and it is what the
//! F51-D acceptance tests pin.

use std::fmt;

use bevy::math::Rect;

use cs_assets::install::sha256;
use cs_content::config::StringRow;
use cs_content::localization::{
    FontProvenance, GlyphCoverage, LanguageMap, LocaleChain, LocaleId, MarkupGrammar,
    ResourceDecode, SubstitutionTable, SupportedLocales, TextId, TextResolution, parse_markup,
};
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::ContentHash;

use super::layout::{LayoutRequest, RequiredControl, layout_text};
use super::metrics::TextMetrics;
use super::original_font::RimageBitmapFonts;

/// The glyph-coverage evidence for one media file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlyphEvidence {
    /// The characters the media was declared to cover. A caller that has
    /// measured the media supplies this; the synthetic tests do.
    Declared {
        /// The declared coverage.
        coverage: GlyphCoverage,
    },
    /// The coverage could not be measured, with why.
    ///
    /// This is the honest state of a media nobody could measure: a coverage
    /// asserted from it would be a guess, so the audit reports
    /// [`AuditBlocker::UnmeasuredGlyphs`] for it rather than passing.
    Unmeasured {
        /// Why the coverage is not measurable.
        reason: String,
    },
    /// Measured: the original never reads this media as a font, with the
    /// evidence that says so.
    ///
    /// There is no coverage to measure because there is no font here, so this
    /// is a measured verdict rather than an unknown and it produces no
    /// blocker — the two loose TGAs of the original installation are
    /// classified this way ([`crate::text::original_font::UNUSED_FONT_TGA_REASON`]).
    UnusedByOriginal {
        /// Why the original does not read this media as a font, with its
        /// evidence.
        reason: String,
    },
    /// Measured: the original's bitmap fonts, each scanned out of this
    /// package with the colour-key cell rule.
    ///
    /// The coverage of each font is in
    /// [`MeasuredBitmapFont::coverage`](crate::text::original_font::MeasuredBitmapFont::coverage),
    /// built only from cells the scan actually found; a character with no
    /// cell stays uncovered and becomes [`AuditBlocker::BitmapFontUnmapped`].
    BitmapFonts {
        /// The per-font measurements, and any declared font the package does
        /// not hold.
        fonts: RimageBitmapFonts,
    },
}

impl GlyphEvidence {
    /// Whether the media's glyph question was answered by measurement: a
    /// declared coverage, a scan of its fonts, or the verdict that it is not
    /// a font in the original at all.
    #[must_use]
    pub fn is_measured(&self) -> bool {
        !matches!(self, Self::Unmeasured { .. })
    }

    /// The declared coverage, if this media declares one.
    ///
    /// [`Self::BitmapFonts`] carries one coverage *per font*; ask
    /// [`Self::bitmap_fonts`] for those.
    #[must_use]
    pub fn coverage(&self) -> Option<&GlyphCoverage> {
        match self {
            Self::Declared { coverage } => Some(coverage),
            _ => None,
        }
    }

    /// The recorded reason this media is not a font in the original, if that
    /// is its verdict.
    #[must_use]
    pub fn unused_reason(&self) -> Option<&str> {
        match self {
            Self::UnusedByOriginal { reason } => Some(reason),
            _ => None,
        }
    }

    /// The measured bitmap fonts this package holds, if it is one.
    #[must_use]
    pub fn bitmap_fonts(&self) -> Option<&RimageBitmapFonts> {
        match self {
            Self::BitmapFonts { fonts } => Some(fonts),
            _ => None,
        }
    }
}

/// One media file handed to the audit: its bytes, its provenance and what is
/// known about its glyph coverage.
pub struct MediaSource<'a> {
    /// The installation-relative path the media lives at, for the report.
    pub path: &'a str,
    /// The file's bytes, read by the caller. Never retained by the audit.
    pub bytes: &'a [u8],
    /// Where the media came from, which decides whether it may be distributed.
    pub provenance: FontProvenance,
    /// The glyph evidence for this media.
    pub glyphs: GlyphEvidence,
}

/// The audited facts about one media file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAudit {
    /// The installation-relative path.
    pub path: String,
    /// The file's length in bytes.
    pub bytes: u64,
    /// SHA-256 of the file's bytes.
    pub sha256: ContentHash,
    /// Whether the release may distribute this file. An original private font
    /// never may; a licensed fallback only with verified permission.
    pub distributable: bool,
    /// The glyph evidence for this media.
    pub glyphs: GlyphEvidence,
}

/// One PE string image handed to the audit.
///
/// The rows are the F12 [`StringRow`]s `StringCatalog::read` produced from this
/// image; `origin` and `provenance` describe where they came from and are
/// attached to every decoded row.
pub struct StringImageSource<'a> {
    /// The installation-relative path, for the report and for blocker identity.
    pub path: &'a str,
    /// The F12 rows read from this image.
    pub rows: &'a [StringRow],
    /// Where the rows came from.
    pub origin: Origin,
    /// The provenance of the rows.
    pub provenance: Provenance,
}

/// The audited string and overflow facts for one declared locale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleTextAudit {
    /// The locale this record audits.
    pub locale: LocaleId,
    /// Ids the locale itself answers.
    pub translated: usize,
    /// Ids another declared locale answers.
    pub via_fallback: usize,
    /// Ids no declared locale in the chain answers.
    pub missing: usize,
    /// How many resolved strings this locale laid out.
    pub laid_out: usize,
    /// How many of those had to scroll rather than fit.
    pub overflowing: usize,
    /// How many laid a painted line over a required control. AC01 fails here.
    pub covering_controls: usize,
}

/// The F51-D audit of one PE string image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringImageAudit {
    /// The installation-relative path the rows were read from.
    pub path: String,
    /// Distinct string ids the decoded catalog holds — the coverage
    /// denominator, never the row count.
    pub ids: usize,
    /// One record per declared locale, in declaration order.
    pub locales: Vec<LocaleTextAudit>,
    /// Locales the catalog holds rows for that nobody declared.
    pub undeclared: Vec<LocaleId>,
    /// Ids no declared locale answers.
    pub missing_everywhere: Vec<TextId>,
    /// Resource rows handed in.
    pub rows: usize,
    /// Rows that became localized rows.
    pub decoded: usize,
    /// Language ids no language-map entry declared.
    pub unmapped_languages: Vec<u32>,
    /// Resource ids whose code units did not decode.
    pub undecodable_ids: Vec<u32>,
    /// Duplicated `(resource id, locale)` pairs inside this image.
    pub duplicates: Vec<(u32, LocaleId)>,
}

impl StringImageAudit {
    /// The audit record for `locale`, if it was declared.
    #[must_use]
    pub fn locale(&self, locale: &LocaleId) -> Option<&LocaleTextAudit> {
        self.locales.iter().find(|audit| &audit.locale == locale)
    }
}

/// One named reason the audit cannot call itself complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditBlocker {
    /// Ids no declared locale answers in one image.
    MissingEverywhere {
        /// The image the ids are missing from.
        image: String,
        /// How many ids.
        ids: usize,
    },
    /// A locale one image holds rows for that the declared set does not name.
    UndeclaredLocale {
        /// The image.
        image: String,
        /// The undeclared locale.
        locale: LocaleId,
    },
    /// A row's language id is not in the declared language map.
    UnmappedLanguage {
        /// The image.
        image: String,
        /// The resource language id.
        language: u32,
    },
    /// A row's code units did not decode.
    UndecodableRow {
        /// The image.
        image: String,
        /// The resource id.
        id: u32,
    },
    /// Two rows in one image share one `(id, locale)` pair, so neither was kept.
    DuplicateRow {
        /// The image.
        image: String,
        /// The resource id.
        id: u32,
        /// The locale.
        locale: LocaleId,
    },
    /// A media file's glyph coverage could not be measured.
    UnmeasuredGlyphs {
        /// The media path.
        path: String,
    },
    /// A declared bitmap font the package does not hold, so nothing about it
    /// was measured.
    BitmapFontMissing {
        /// The package path.
        path: String,
        /// The declared font name.
        font: String,
    },
    /// Characters of one bitmap font whose cell the scan did not find. The
    /// original draws `'!'` for them; they are never claimed covered.
    BitmapFontUnmapped {
        /// The package path.
        path: String,
        /// The font.
        font: String,
        /// How many characters have no cell.
        chars: usize,
    },
    /// Cells one bitmap font stores that no character of the original's
    /// `c - 0x21` bound maps to, so their character is unknown.
    BitmapFontStrayCell {
        /// The package path.
        path: String,
        /// The font.
        font: String,
        /// How many cells.
        cells: usize,
    },
    /// A locale's translation has to scroll in the panel.
    Overflow {
        /// The image.
        image: String,
        /// The locale.
        locale: LocaleId,
        /// How many strings.
        strings: usize,
    },
    /// A locale's layout painted over a required control.
    CoversControl {
        /// The image.
        image: String,
        /// The locale.
        locale: LocaleId,
        /// How many strings.
        strings: usize,
    },
}

impl AuditBlocker {
    /// A stable, machine-matchable label for reports and evidence files.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingEverywhere { .. } => "missing_everywhere",
            Self::UndeclaredLocale { .. } => "undeclared_locale",
            Self::UnmappedLanguage { .. } => "unmapped_language",
            Self::UndecodableRow { .. } => "undecodable_row",
            Self::DuplicateRow { .. } => "duplicate_row",
            Self::UnmeasuredGlyphs { .. } => "unmeasured_glyphs",
            Self::BitmapFontMissing { .. } => "bitmap_font_missing",
            Self::BitmapFontUnmapped { .. } => "bitmap_font_unmapped",
            Self::BitmapFontStrayCell { .. } => "bitmap_font_stray_cell",
            Self::Overflow { .. } => "overflow",
            Self::CoversControl { .. } => "covers_control",
        }
    }
}

impl fmt::Display for AuditBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEverywhere { image, ids } => {
                write!(
                    f,
                    "no declared locale answers {ids} string id(s) in {image}"
                )
            }
            Self::UndeclaredLocale { image, locale } => {
                write!(
                    f,
                    "the image {image} holds rows for undeclared locale {locale}"
                )
            }
            Self::UnmappedLanguage { image, language } => write!(
                f,
                "resource language {language} in {image} is not in the declared language map"
            ),
            Self::UndecodableRow { image, id } => {
                write!(
                    f,
                    "string id {id} in {image} has code units that do not decode"
                )
            }
            Self::DuplicateRow { image, id, locale } => {
                write!(
                    f,
                    "string id {id} is stored twice for locale {locale} in {image}"
                )
            }
            Self::UnmeasuredGlyphs { path } => {
                write!(
                    f,
                    "the glyph coverage of media {path} could not be measured"
                )
            }
            Self::BitmapFontMissing { path, font } => {
                write!(
                    f,
                    "the package {path} does not hold the declared font {font}"
                )
            }
            Self::BitmapFontUnmapped { path, font, chars } => write!(
                f,
                "the font {font} in {path} has no measured cell for {chars} character(s)"
            ),
            Self::BitmapFontStrayCell { path, font, cells } => write!(
                f,
                "the font {font} in {path} stores {cells} cell(s) no character maps to"
            ),
            Self::Overflow {
                image,
                locale,
                strings,
            } => write!(
                f,
                "{strings} string(s) for locale {locale} in {image} do not fit the panel"
            ),
            Self::CoversControl {
                image,
                locale,
                strings,
            } => write!(
                f,
                "{strings} string(s) for locale {locale} in {image} paint over a required control"
            ),
        }
    }
}

/// The complete F51-D localization audit of one installation or fixture: every
/// string image, then every media file, with one flat blocker list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalizationAudit {
    /// One record per audited string image, in the order handed in.
    pub images: Vec<StringImageAudit>,
    /// The audited media files, in the order handed in.
    pub media: Vec<MediaAudit>,
    /// Every reason the audit is not complete.
    pub blockers: Vec<AuditBlocker>,
}

impl LocalizationAudit {
    /// Whether the audit found nothing unmeasured and nothing missing.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.blockers.is_empty()
    }

    /// The audit of the string image at `path`, if it was audited.
    #[must_use]
    pub fn image(&self, path: &str) -> Option<&StringImageAudit> {
        self.images.iter().find(|image| image.path == path)
    }

    /// The media audit for `path`, if that media was audited.
    #[must_use]
    pub fn media(&self, path: &str) -> Option<&MediaAudit> {
        self.media.iter().find(|audit| audit.path == path)
    }

    /// The number of media files the release may distribute.
    #[must_use]
    pub fn distributable_media(&self) -> usize {
        self.media
            .iter()
            .filter(|audit| audit.distributable)
            .count()
    }

    /// The distinct ids across every audited image, summed.
    ///
    /// Ids are per image (see the module docs), so this is a total, not a union.
    #[must_use]
    pub fn ids(&self) -> usize {
        self.images.iter().map(|image| image.ids).sum()
    }

    /// Every blocker with the given [`AuditBlocker::code`].
    pub fn blockers_with_code<'a>(
        &'a self,
        code: &'a str,
    ) -> impl Iterator<Item = &'a AuditBlocker> + 'a {
        self.blockers
            .iter()
            .filter(move |blocker| blocker.code() == code)
    }
}

/// Everything one audit needs.
pub struct LocalizationAuditRequest<'a> {
    /// The PE string images to audit, each decoded in isolation.
    pub images: &'a [StringImageSource<'a>],
    /// The declared supported locales.
    pub locales: &'a SupportedLocales,
    /// The declared mapping from resource language id to locale.
    pub language_map: &'a LanguageMap,
    /// The declared control-markup grammar.
    pub grammar: &'a MarkupGrammar,
    /// The measured font the strings are laid out with.
    pub metrics: &'a TextMetrics,
    /// The panel the strings are laid out in.
    pub panel: Rect,
    /// The controls the text must never cover.
    pub required: &'a [RequiredControl],
    /// The substitution values supplied for every string.
    pub substitutions: &'a SubstitutionTable,
    /// The original media files to audit.
    pub media: &'a [MediaSource<'a>],
}

/// Runs the F51-D audit over one installation's string images and media.
///
/// Each image is decoded through the caller's [`LanguageMap`] exactly as
/// [`ResourceDecode`] does, every declared locale is audited against that
/// image's decoded catalog, and every resolved string is laid out in the
/// caller's panel so an overflow is counted rather than assumed absent. Nothing
/// is fabricated: a language, id or media the inputs do not cover is a named
/// [`AuditBlocker`].
#[must_use]
pub fn audit_localization(request: &LocalizationAuditRequest<'_>) -> LocalizationAudit {
    let mut images = Vec::with_capacity(request.images.len());
    let mut blockers: Vec<AuditBlocker> = Vec::new();

    for source in request.images {
        let image = audit_image(source, request);
        blockers.extend(image_blockers(&image));
        images.push(image);
    }

    let media: Vec<MediaAudit> = request
        .media
        .iter()
        .map(|source| MediaAudit {
            path: source.path.to_owned(),
            bytes: source.bytes.len() as u64,
            sha256: sha256(source.bytes),
            distributable: source.provenance.is_distributable(),
            glyphs: source.glyphs.clone(),
        })
        .collect();
    for audit in &media {
        blockers.extend(media_blockers(audit));
    }

    LocalizationAudit {
        images,
        media,
        blockers,
    }
}

/// Decodes and audits one string image.
fn audit_image(
    source: &StringImageSource<'_>,
    request: &LocalizationAuditRequest<'_>,
) -> StringImageAudit {
    let decode = ResourceDecode::decode(
        source.rows,
        request.language_map,
        source.origin.clone(),
        source.provenance.clone(),
    );
    let catalog = decode.catalog();
    let multi = catalog.audit_locales(request.locales);

    let mut locales = Vec::with_capacity(multi.locales.len());
    for coverage in &multi.locales {
        let chain = LocaleChain::new(
            coverage.locale.clone(),
            coverage
                .chain
                .iter()
                .filter(|locale| **locale != coverage.locale)
                .cloned(),
        )
        .expect("the coverage chain is a valid nonempty unique chain");
        let mut laid_out = 0usize;
        let mut overflowing = 0usize;
        let mut covering_controls = 0usize;
        for id in catalog.ids() {
            let TextResolution::Resolved { row, .. } = catalog.resolve(&id, &chain) else {
                continue;
            };
            laid_out += 1;
            let document = parse_markup(row.text(), request.grammar);
            let layout = layout_text(&LayoutRequest {
                id: Some(id.clone()),
                document: &document,
                metrics: request.metrics,
                substitutions: request.substitutions,
                panel: request.panel,
                required: request.required,
            });
            match layout {
                Ok(layout) => {
                    if layout.fit().is_scrolling() {
                        overflowing += 1;
                    }
                    if request
                        .required
                        .iter()
                        .any(|control| layout.covers(control))
                    {
                        covering_controls += 1;
                    }
                }
                // A layout that could not be produced at all (an empty panel,
                // no free band) is not a silent success: it is counted as an
                // overflow so the screen cannot pass by failing to lay out.
                Err(_) => overflowing += 1,
            }
        }
        locales.push(LocaleTextAudit {
            locale: coverage.locale.clone(),
            translated: coverage.translated,
            via_fallback: coverage.via_fallback,
            missing: coverage.missing.len(),
            laid_out,
            overflowing,
            covering_controls,
        });
    }

    StringImageAudit {
        path: source.path.to_owned(),
        ids: multi.ids,
        locales,
        undeclared: multi.undeclared,
        missing_everywhere: multi.missing_everywhere,
        rows: decode.rows(),
        decoded: decode.decoded(),
        unmapped_languages: decode.unmapped_languages().to_vec(),
        undecodable_ids: decode.undecodable_ids().to_vec(),
        duplicates: decode.duplicates().to_vec(),
    }
}

/// The named reasons one media file's glyph evidence leaves something
/// unmeasured.
///
/// A declared coverage and the verdict "the original never reads this file as
/// a font" are answers, not gaps; an unmeasured coverage, a declared font the
/// package does not hold, a character whose cell was not found and a cell no
/// character maps to are each one named blocker.
fn media_blockers(audit: &MediaAudit) -> Vec<AuditBlocker> {
    let mut blockers: Vec<AuditBlocker> = Vec::new();
    match &audit.glyphs {
        GlyphEvidence::Unmeasured { .. } => {
            blockers.push(AuditBlocker::UnmeasuredGlyphs {
                path: audit.path.clone(),
            });
        }
        GlyphEvidence::BitmapFonts { fonts } => {
            for font in &fonts.missing {
                blockers.push(AuditBlocker::BitmapFontMissing {
                    path: audit.path.clone(),
                    font: font.clone(),
                });
            }
            for font in &fonts.fonts {
                if !font.unresolved.is_empty() {
                    blockers.push(AuditBlocker::BitmapFontUnmapped {
                        path: audit.path.clone(),
                        font: font.name.clone(),
                        chars: font.unresolved.len(),
                    });
                }
                if font.stray_cells > 0 {
                    blockers.push(AuditBlocker::BitmapFontStrayCell {
                        path: audit.path.clone(),
                        font: font.name.clone(),
                        cells: font.stray_cells,
                    });
                }
            }
        }
        GlyphEvidence::Declared { .. } | GlyphEvidence::UnusedByOriginal { .. } => {}
    }
    blockers
}

/// The named reasons one image's audit is not complete.
fn image_blockers(image: &StringImageAudit) -> Vec<AuditBlocker> {
    let mut blockers: Vec<AuditBlocker> = Vec::new();
    for locale in &image.undeclared {
        blockers.push(AuditBlocker::UndeclaredLocale {
            image: image.path.clone(),
            locale: locale.clone(),
        });
    }
    if !image.missing_everywhere.is_empty() {
        blockers.push(AuditBlocker::MissingEverywhere {
            image: image.path.clone(),
            ids: image.missing_everywhere.len(),
        });
    }
    for language in &image.unmapped_languages {
        blockers.push(AuditBlocker::UnmappedLanguage {
            image: image.path.clone(),
            language: *language,
        });
    }
    for id in &image.undecodable_ids {
        blockers.push(AuditBlocker::UndecodableRow {
            image: image.path.clone(),
            id: *id,
        });
    }
    for (id, locale) in &image.duplicates {
        blockers.push(AuditBlocker::DuplicateRow {
            image: image.path.clone(),
            id: *id,
            locale: locale.clone(),
        });
    }
    for audit in &image.locales {
        if audit.overflowing > 0 {
            blockers.push(AuditBlocker::Overflow {
                image: image.path.clone(),
                locale: audit.locale.clone(),
                strings: audit.overflowing,
            });
        }
        if audit.covering_controls > 0 {
            blockers.push(AuditBlocker::CoversControl {
                image: image.path.clone(),
                locale: audit.locale.clone(),
                strings: audit.covering_controls,
            });
        }
    }
    blockers
}
