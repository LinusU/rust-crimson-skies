//! Shared helpers for the F51-A text acceptance tests.
//!
//! Everything here is authored test scaffolding, never production logic: it
//! builds validated locales, ids and content ids exactly the way the
//! production fixtures do, and it declares the synthetic screen the AC01
//! scenario lays text out in (a 640x480 panel with an `Ok` and a `Cancel`
//! button along the bottom edge).
//!
//! No original data and no `CS_GAME_DIR` access: every value here is authored
//! development content, so these tests prove the interface and the layout
//! contract, never the original game.

use bevy::math::Rect;
pub use cs_app::text::layout::{LayoutRequest, RequiredControl};
use cs_app::text::{
    GlyphEvidence, LocalizationAudit, LocalizationAuditRequest, MediaSource, StringImageSource,
    audit_localization, synthetic_monospace,
};
use cs_content::config::StringRow;
use cs_content::localization::{
    FontProvenance, LanguageMap, LocaleChain, LocaleId, LocalizedText, MarkupDocument,
    MarkupGrammar, SupportedLocales, TextId, parse_markup, synthetic_markup_grammar,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance};
use cs_types::evidence::{ClaimId, ContentHash};

/// The panel the AC01 scenario lays text out in: 640x480 with a 40px button row
/// along the bottom edge.
pub const PANEL: Rect = Rect {
    min: bevy::math::Vec2::new(0.0, 0.0),
    max: bevy::math::Vec2::new(640.0, 480.0),
};

/// The button row's top edge: everything below it is a required control.
pub const BUTTON_ROW_TOP: f32 = 440.0;

/// The claim every designed test value is recorded under.
pub fn claim() -> ClaimId {
    ClaimId::new("f51a.text-test").expect("valid claim id")
}

/// A validated locale label.
pub fn locale(label: &str) -> LocaleId {
    LocaleId::new(label).expect("the fixture locale is a valid label")
}

/// A validated locale chain: the selected locale first, then the fallbacks.
pub fn chain(selected: &str, fallbacks: &[&str]) -> LocaleChain {
    LocaleChain::new(locale(selected), fallbacks.iter().copied().map(locale))
        .expect("the fixture chain is valid")
}

/// A validated localizable string id.
pub fn text_id(key: &str) -> TextId {
    TextId::new(key).expect("the fixture key is a valid string id")
}

/// A localized row with synthetic-fixture origin and designed provenance.
pub fn row(key: &str, locale_label: &str, text: &str) -> LocalizedText {
    LocalizedText::new(
        text_id(key),
        locale(locale_label),
        text,
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    )
}

/// A `ui_resource` content id for a required control.
pub fn control_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::UiResource, key).expect("the control id is valid")
}

/// A synthetic PE string row exactly as F12 hands one out: a resource id, the
/// third-level language id, a code page and the text (or `None` when the units
/// did not decode).
///
/// The span names an authored `strings.dll`; the bytes are never read, because
/// [`cs_content::localization::ResourceDecode`] consumes rows rather than
/// opening a file. No original byte or string is reproduced.
pub fn resource_row(id: u32, language: u32, text: Option<&str>) -> StringRow {
    let span = SourceSpan::new(
        ContentHash::from_bytes([7; 32]),
        "strings.dll",
        None,
        0,
        16,
        None,
    )
    .expect("the fixture span is valid");
    StringRow {
        id,
        language,
        code_page: 1252,
        code_units: Vec::new(),
        text: text.map(str::to_owned),
        span,
    }
}

/// A validated caller-declared language map.
pub fn language_map(entries: &[(u32, &str)]) -> cs_content::localization::LanguageMap {
    cs_content::localization::LanguageMap::new(
        entries
            .iter()
            .map(|(language, label)| (*language, locale(label))),
    )
    .expect("the fixture language map is valid")
}

/// The synthetic screen's required controls: `Ok` and `Cancel` in the bottom
/// row.
pub fn buttons() -> Vec<RequiredControl> {
    vec![
        RequiredControl::new(
            control_id("dialog.ok"),
            Rect::new(160.0, BUTTON_ROW_TOP, 300.0, 480.0),
        )
        .expect("the ok button is a ui_resource"),
        RequiredControl::new(
            control_id("dialog.cancel"),
            Rect::new(360.0, BUTTON_ROW_TOP, 500.0, 480.0),
        )
        .expect("the cancel button is a ui_resource"),
    ]
}

/// The declared synthetic grammar, with the production fixture spelling.
pub fn grammar() -> MarkupGrammar {
    synthetic_markup_grammar()
}

/// A validated document for `text` under the declared grammar.
pub fn document(text: &str) -> MarkupDocument {
    parse_markup(text, &grammar())
}

/// A layout request for `text` in the synthetic panel with the synthetic
/// buttons reserved.
pub fn request<'a>(
    id: Option<TextId>,
    document: &'a MarkupDocument,
    metrics: &'a cs_app::text::TextMetrics,
    substitutions: &'a cs_content::localization::SubstitutionTable,
    required: &'a [RequiredControl],
) -> LayoutRequest<'a> {
    LayoutRequest {
        id,
        document,
        metrics,
        substitutions,
        panel: PANEL,
        required,
    }
}

/// An empty substitution table.
pub fn substitutions() -> cs_content::localization::SubstitutionTable {
    cs_content::localization::SubstitutionTable::new()
}

// ------------------------------------------------------- retail (F51-D) ---

/// The three PE string images the F12-A/F12-D survey routed, in a fixed order.
///
/// Each is audited in isolation, because an id is meaningful only inside its
/// own image (see `cs_app::text::audit`). Names only: no original byte is read
/// or kept by this module, and nothing here opens `CS_GAME_DIR` unless a
/// retail test calls one of the `retail_*` helpers.
pub const RETAIL_STRING_IMAGES: &[&str] = &[
    "strings.dll",
    "GOSDATA/ASSETS/BINARIES/langui.dll",
    "GOSDATA/ASSETS/BINARIES/language.dll",
];

/// The two original bitmap-font files F51 audits as media, in a fixed order.
pub const RETAIL_FONT_MEDIA: &[&str] = &[
    "GOSDATA/ASSETS/GRAPHICS/font.tga",
    "GOSDATA/ASSETS/GRAPHICS/arial8.tga",
];

/// The read-only original installation root, or a loud failure: a retail test
/// must fail, not pass, when `CS_GAME_DIR` is absent.
pub fn retail_game_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this test needs the original installation (capability `retail`)",
    );
    let dir = std::path::PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The whole installation fingerprint, read through production discovery.
pub fn retail_install_hash(dir: &std::path::Path) -> ContentHash {
    let found = cs_assets::install::discover(dir)
        .expect("production discovery must read the original installation");
    cs_assets::install::fingerprint(&found.manifest)
}

/// The bytes of one installation file, named by its `/`-separated spelling.
pub fn retail_file(dir: &std::path::Path, spelling: &str) -> Vec<u8> {
    let path = dir.join(spelling);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The F12 rows of one PE string image, read through the production
/// `StringCatalog::read`.
///
/// A loose file is its own source, so the span carries no member digest —
/// exactly the rule `cs-inspect config` and the campaign binding apply.
pub fn retail_string_rows(
    dir: &std::path::Path,
    spelling: &str,
    install_hash: ContentHash,
) -> Vec<StringRow> {
    let bytes = retail_file(dir, spelling);
    let source = SourceSpan::new(install_hash, spelling, None, 0, bytes.len() as u64, None)
        .expect("the image span is valid");
    let mut context = cs_formats::ParseContext::with_defaults(spelling);
    cs_content::config::StringCatalog::read(&mut context, source, &bytes)
        .expect("the production reader reads the string image")
        .rows()
        .to_vec()
}

/// The declared supported-locale set the retail audit walks.
///
/// The single available installation declares one language id (1033), so this
/// declares one locale. It is **not** a claim about the original release's
/// supported-locale list, which F51-A recorded as unmeasured.
pub fn retail_declared_locales() -> SupportedLocales {
    SupportedLocales::new([locale("en-us")]).expect("one declared locale is a valid set")
}

/// The caller-declared language map for the retail audit: the one language id
/// (1033) the three routed string images carry (F12-A/F12-D).
pub fn retail_language_map() -> LanguageMap {
    language_map(&[(1033, "en-us")])
}

/// The F51-D audit of the original installation: all three routed string
/// images, each read through the production reader and audited in isolation,
/// plus the two original bitmap fonts with their **unmeasured** glyph coverage.
///
/// The returned audit owns every count and digest; no original bytes or text
/// escape this function.
pub fn retail_audit(dir: &std::path::Path) -> LocalizationAudit {
    let install_hash = retail_install_hash(dir);
    let declared = retail_declared_locales();
    let languages = retail_language_map();
    let grammar = grammar();
    let metrics = synthetic_monospace(16.0);
    let required = buttons();
    let substitutions = substitutions();

    let rows: Vec<Vec<StringRow>> = RETAIL_STRING_IMAGES
        .iter()
        .map(|spelling| retail_string_rows(dir, spelling, install_hash))
        .collect();
    let origins: Vec<Origin> = RETAIL_STRING_IMAGES
        .iter()
        .map(|spelling| {
            let length = std::fs::metadata(dir.join(spelling))
                .unwrap_or_else(|error| panic!("stat {spelling}: {error}"))
                .len();
            let span = SourceSpan::new(install_hash, spelling, None, 0, length, None)
                .expect("the retail image span is valid");
            Origin::Installation { source: span }
        })
        .collect();
    let provenance = Provenance::unknown(claim());
    let images: Vec<StringImageSource<'_>> = RETAIL_STRING_IMAGES
        .iter()
        .zip(&rows)
        .zip(&origins)
        .map(|((spelling, image_rows), origin)| StringImageSource {
            path: spelling,
            rows: image_rows,
            origin: origin.clone(),
            provenance: provenance.clone(),
        })
        .collect();

    let font_bytes: Vec<Vec<u8>> = RETAIL_FONT_MEDIA
        .iter()
        .map(|spelling| retail_file(dir, spelling))
        .collect();
    let media: Vec<MediaSource<'_>> = RETAIL_FONT_MEDIA
        .iter()
        .zip(&font_bytes)
        .map(|(spelling, bytes)| {
            let span = SourceSpan::new(install_hash, spelling, None, 0, bytes.len() as u64, None)
                .expect("the retail font span is valid");
            MediaSource {
                path: spelling,
                bytes,
                provenance: FontProvenance::OriginalPrivate {
                    span: Box::new(span),
                },
                glyphs: GlyphEvidence::Unmeasured {
                    reason: "the original bitmap font's cell-to-character mapping is unmeasured"
                        .to_owned(),
                },
            }
        })
        .collect();

    audit_localization(&LocalizationAuditRequest {
        images: &images,
        locales: &declared,
        language_map: &languages,
        grammar: &grammar,
        metrics: &metrics,
        panel: PANEL,
        required: &required,
        substitutions: &substitutions,
        media: &media,
    })
}
