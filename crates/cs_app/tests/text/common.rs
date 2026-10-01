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
use cs_content::config::StringRow;
use cs_content::localization::{
    LocaleChain, LocaleId, LocalizedText, MarkupDocument, MarkupGrammar, TextId, parse_markup,
    synthetic_markup_grammar,
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
