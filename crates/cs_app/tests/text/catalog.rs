//! Locale fallback, the explicit chain, and the locale-independent identity of
//! a localizable string (F51-A).

use cs_content::localization::{
    LocaleChain, LocaleChainError, LocaleId, LocaleIdError, MAX_LOCALE_CHAIN_LEN,
    MAX_LOCALE_ID_LEN, SYNTHETIC_LONG_TRANSLATION_KEY, TextCatalog, TextId, TextIdError,
    TextResolution, declared_synthetic_text_catalog,
};
use cs_types::content::{ContentId, ContentKind};

use crate::common::{chain, locale, text_id};

/// The fallback chain is walked in the caller's order and the resolution
/// reports which locale answered and how far down the chain it was, so a
/// caller can tell a real translation from a fallback without comparing labels.
#[test]
fn accept_f51_a_locale_chain_reports_the_locale_and_depth_that_answered() {
    let catalog = declared_synthetic_text_catalog();
    let id = text_id(SYNTHETIC_LONG_TRANSLATION_KEY);

    let selected = chain("de-de", &["en-us"]);
    let TextResolution::Resolved {
        row,
        locale: answered,
        depth,
        used_fallback,
    } = catalog.resolve(&id, &selected)
    else {
        panic!("the german row resolves for the selected locale");
    };
    assert_eq!(depth, 0);
    assert!(!used_fallback);
    assert_eq!(answered, locale("de-de"));
    assert_eq!(row.locale(), &answered);
    assert!(row.text().contains("Sturmzelle"));

    // A locale the fixture does not translate at all falls back, and the answer
    // says how far down the chain it had to walk. `hud.untranslated` exists only
    // in `en-us`.
    let untranslated = text_id("hud.untranslated");
    let spanish = chain("es-es", &["de-de", "en-us"]);
    let TextResolution::Resolved {
        locale: answered,
        depth,
        used_fallback,
        ..
    } = catalog.resolve(&untranslated, &spanish)
    else {
        panic!("the chain reaches the english row");
    };
    assert_eq!(answered, locale("en-us"));
    assert_eq!(depth, 2);
    assert!(used_fallback);
    assert_eq!(spanish.depth_of(&locale("en-us")), Some(2));
    assert_eq!(spanish.depth_of(spanish.selected()), Some(0));
    assert_eq!(spanish.depth_of(&locale("it-it")), None);
}

/// A locale that has no row anywhere in the chain is a **named miss**: the
/// resolution reports every locale it tried and returns no text, so a caller can
/// never display a placeholder as if it were the translation.
#[test]
fn accept_f51_a_a_missing_string_is_a_named_miss_not_a_placeholder() {
    let catalog = declared_synthetic_text_catalog();
    // `hud.untranslated` exists only in `en-us`.
    let id = text_id("hud.untranslated");

    let with_fallback = chain("de-de", &["en-us"]);
    assert!(matches!(
        catalog.resolve(&id, &with_fallback),
        TextResolution::Resolved { .. }
    ));

    let without_fallback = chain("de-de", &[]);
    let resolution = catalog.resolve(&id, &without_fallback);
    let TextResolution::Missing { id: missed, tried } = &resolution else {
        panic!("no locale in the chain has the row");
    };
    assert_eq!(missed, &id);
    assert_eq!(tried, &[locale("de-de")]);
    assert!(resolution.text().is_none());
    assert!(resolution.row().is_none());
    assert!(!resolution.used_fallback());

    // The audit keeps the miss in the denominator instead of dropping the id.
    let audit = catalog.audit(&chain("de-de", &["en-us"]));
    assert_eq!(audit.ids, catalog.ids().len());
    assert_eq!(audit.resolved, audit.ids);
    assert!(audit.missing.is_empty());
    assert!(audit.is_complete());

    let audit = catalog.audit(&chain("de-de", &[]));
    // Both strings that only `en-us` has stay in the denominator as misses.
    assert_eq!(
        audit.missing,
        vec![
            text_id("hud.untranslated"),
            text_id("mission.briefing.confirm")
        ]
    );
    assert!(audit.missing.contains(&id));
    assert!(!audit.is_complete());
    assert_eq!(audit.ids, catalog.ids().len());
    assert_eq!(audit.resolved + audit.missing.len(), audit.ids);
}

/// Changing the locale changes the text and nothing else: the string identity
/// is a `string_resource` id that is the same under every chain, and it can
/// never be spelled like a mission, a save or a protocol value (F51
/// non-negotiable behavior 5).
#[test]
fn accept_f51_a_a_translation_keeps_the_string_id_across_locales() {
    let catalog = declared_synthetic_text_catalog();
    let id = text_id(SYNTHETIC_LONG_TRANSLATION_KEY);

    let german = catalog.resolve(&id, &chain("de-de", &[]));
    let french = catalog.resolve(&id, &chain("fr-fr", &[]));
    assert_ne!(german.text(), french.text(), "the two rows differ");
    for resolution in [&german, &french] {
        let row = resolution.row().expect("both rows resolve");
        assert_eq!(row.id(), &id, "the id does not depend on the locale");
        assert_eq!(row.id().as_content_id().kind(), ContentKind::StringResource);
    }

    // A string id can only be a `string_resource`: a mission id is refused by
    // name, so a locale-dependent lookup can never be pointed at mission
    // identity.
    let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("valid mission id");
    assert_eq!(
        TextId::try_from_content(mission),
        Err(TextIdError::NotAStringResource {
            kind: ContentKind::Mission
        })
    );
}

/// The chain and the locale label are validated: no empty chain, no repeated
/// locale, no over-long chain, and a label that is a single safe token.
#[test]
fn accept_f51_a_a_malformed_locale_or_chain_is_refused() {
    assert_eq!(LocaleId::new("   "), Err(LocaleIdError::Empty));
    assert_eq!(
        LocaleId::new(&"x".repeat(MAX_LOCALE_ID_LEN + 1)),
        Err(LocaleIdError::TooLong {
            len: MAX_LOCALE_ID_LEN + 1
        })
    );
    assert_eq!(
        LocaleId::new("de/de"),
        Err(LocaleIdError::BadCharacter { ch: '/' })
    );
    assert!(LocaleId::new(&"x".repeat(MAX_LOCALE_ID_LEN)).is_ok());

    assert_eq!(
        chain("de-de", &[]).locales().len(),
        1,
        "a single-locale chain is valid"
    );
    let repeated = LocaleChain::new(locale("en-us"), [locale("de-de"), locale("en-us")]);
    assert_eq!(
        repeated,
        Err(LocaleChainError::Duplicate {
            locale: locale("en-us")
        })
    );
    let too_long: Vec<LocaleId> = (0..=MAX_LOCALE_CHAIN_LEN)
        .map(|index| locale(&format!("l{index}")))
        .collect();
    let mut iter = too_long.into_iter();
    let first = iter.next().expect("the chain is over-long");
    let error = LocaleChain::new(first, iter).expect_err("an over-long chain is refused");
    assert_eq!(
        error,
        LocaleChainError::TooLong {
            len: MAX_LOCALE_CHAIN_LEN + 1
        }
    );
}

/// A duplicate `(id, locale)` row is refused by name instead of silently
/// replacing the first one, matching the F12 string catalog's refusal of an
/// ambiguous `(id, language)` pair.
#[test]
fn accept_f51_a_a_duplicate_translation_row_is_refused() {
    let mut catalog = TextCatalog::new();
    let id = text_id("hud.untranslated");
    catalog
        .insert(crate::common::row(
            "hud.untranslated",
            "en-us",
            "Target lost",
        ))
        .expect("the first row inserts");
    let error = catalog
        .insert(crate::common::row(
            "hud.untranslated",
            "en-us",
            "Target destroyed",
        ))
        .expect_err("a duplicate row is refused");
    assert!(error.to_string().contains("hud.untranslated"));
    assert!(error.to_string().contains("en-us"));
    // The first row survives the refusal unchanged.
    assert_eq!(
        catalog.get(&id, &locale("en-us")).map(|row| row.text()),
        Some("Target lost")
    );
}
