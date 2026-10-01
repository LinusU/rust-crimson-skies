//! F51-B screen text pipeline: the one production path that resolves a
//! localized id, parses its markup, lays it out and surfaces every diagnostic.
//!
//! The stage's minimum scenario — *malformed markup and absent glyphs produce
//! visible diagnostics* — lives here. The strings are decoded from authored F12
//! resource rows, so the decode bridge is on the path too. No original data and
//! no `CS_GAME_DIR` access.

use cs_app::text::{
    LayoutDiagnostic, ScreenTextError, ScreenTextRequest, layout_localized_text,
    synthetic_monospace,
};
use cs_content::config::StringRow;
use cs_content::localization::{ResourceDecode, TextCatalog, TextId};
use cs_types::content::{Origin, Provenance};

use crate::common::{
    PANEL, buttons, chain, claim, grammar, language_map, locale, resource_row, substitutions,
};

/// Decodes authored resource rows into a catalog exactly as the production
/// bridge does, with synthetic provenance.
fn decode(rows: &[StringRow], entries: &[(u32, &str)]) -> TextCatalog {
    ResourceDecode::decode(
        rows,
        &language_map(entries),
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    )
    .into_catalog()
}

/// The minimum scenario: a decoded string whose markup the grammar refuses and
/// whose text carries a character the font has no glyph for produces **visible**
/// diagnostics — a typed code per problem and a one-line rendering of each — and
/// the refused markup is still displayed rather than dropped.
#[test]
fn accept_f51_b_malformed_markup_and_absent_glyphs_are_visible_diagnostics() {
    let rows = [resource_row(
        0,
        1033,
        Some("Confirm café [blink]now[/blink]"),
    )];
    let catalog = decode(&rows, &[(1033, "en-us")]);
    let id = TextId::from_resource_id(0);
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let required = buttons();
    let grammar = grammar();
    let chain = chain("en-us", &[]);

    let screen = layout_localized_text(&ScreenTextRequest {
        catalog: &catalog,
        id: &id,
        chain: &chain,
        grammar: &grammar,
        metrics: &metrics,
        substitutions: &values,
        panel: PANEL,
        required: &required,
    })
    .expect("the panel has a free band for the text");

    let codes = screen.diagnostic_codes();
    assert!(
        codes.contains(&"unknown_tag"),
        "a tag the grammar refuses must be reported: {codes:?}"
    );
    assert!(
        codes.contains(&"unbalanced_control"),
        "the unmatched closing control must be reported: {codes:?}"
    );

    let missing: Vec<char> = screen
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            LayoutDiagnostic::MissingGlyph { ch, .. } => Some(*ch),
            _ => None,
        })
        .collect();
    assert!(
        missing.contains(&'é'),
        "the accented character the ASCII coverage lacks must be counted: {missing:?}"
    );

    // "Visible": each diagnostic renders as a line a screen can show.
    let report = screen.diagnostic_report();
    assert!(screen.has_diagnostics());
    assert!(
        report.contains("unknown_tag"),
        "the rendered report names the refused tag: {report:?}"
    );
    assert!(
        report.contains('é'),
        "the rendered report names the absent glyph: {report:?}"
    );

    // Nothing is silently dropped: the refused controls stay literal text.
    assert!(screen.layout().text().contains("[blink]"));
    assert!(screen.layout().text().contains("[/blink]"));
    assert_eq!(screen.id(), &id);
    assert_eq!(screen.locale().as_str(), "en-us");
    assert!(!screen.used_fallback());
    for control in &required {
        assert!(
            !screen.layout().covers(control),
            "the text is laid out clear of the required controls"
        );
    }
}

/// A clean decoded string with no malformed markup and full coverage has no
/// diagnostics, so a screen does not flag correct text.
#[test]
fn accept_f51_b_a_clean_string_has_no_diagnostics() {
    let rows = [resource_row(
        0,
        1033,
        Some("Confirm [bold]your corridor[/bold]"),
    )];
    let catalog = decode(&rows, &[(1033, "en-us")]);
    let id = TextId::from_resource_id(0);
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let required = buttons();
    let grammar = grammar();
    let chain = chain("en-us", &[]);

    let screen = layout_localized_text(&ScreenTextRequest {
        catalog: &catalog,
        id: &id,
        chain: &chain,
        grammar: &grammar,
        metrics: &metrics,
        substitutions: &values,
        panel: PANEL,
        required: &required,
    })
    .expect("the panel has a free band");

    assert!(!screen.has_diagnostics(), "{:?}", screen.diagnostics());
    assert_eq!(screen.diagnostic_report(), "");
    assert!(screen.layout().text().contains("your corridor"));
}

/// A string no locale in the chain answers is a named miss that lists every
/// locale that was tried, never an empty label.
#[test]
fn accept_f51_b_a_missing_string_is_a_named_miss_not_a_blank() {
    let rows = [resource_row(0, 1033, Some("Target lost"))];
    let catalog = decode(&rows, &[(1033, "en-us")]);
    let id = TextId::from_resource_id(7);
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let required = buttons();
    let grammar = grammar();
    let chain = chain("de-de", &["fr-fr"]);

    let error = layout_localized_text(&ScreenTextRequest {
        catalog: &catalog,
        id: &id,
        chain: &chain,
        grammar: &grammar,
        metrics: &metrics,
        substitutions: &values,
        panel: PANEL,
        required: &required,
    })
    .expect_err("id 7 has no row in any tried locale");

    match error {
        ScreenTextError::Missing { id: missing, tried } => {
            assert_eq!(missing, TextId::from_resource_id(7));
            assert_eq!(tried, vec![locale("de-de"), locale("fr-fr")]);
        }
        other => panic!("expected a named miss, got {other:?}"),
    }
}

/// The chain's fallback answers, and the screen reports which locale actually
/// did so: a translation that fell through is visible, not passed off as the
/// selected locale.
#[test]
fn accept_f51_b_a_fallback_locale_answers_and_is_reported() {
    let rows = [resource_row(0, 1033, Some("Target lost"))];
    let catalog = decode(&rows, &[(1033, "en-us")]);
    let id = TextId::from_resource_id(0);
    let metrics = synthetic_monospace(16.0);
    let values = substitutions();
    let required = buttons();
    let grammar = grammar();
    let chain = chain("de-de", &["en-us"]);

    let screen = layout_localized_text(&ScreenTextRequest {
        catalog: &catalog,
        id: &id,
        chain: &chain,
        grammar: &grammar,
        metrics: &metrics,
        substitutions: &values,
        panel: PANEL,
        required: &required,
    })
    .expect("the fallback locale answers");

    assert_eq!(screen.locale().as_str(), "en-us");
    assert!(screen.used_fallback());
    assert_eq!(screen.layout().text(), "Target lost");
}
