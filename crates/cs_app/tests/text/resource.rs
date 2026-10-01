//! F51-B resource decoding: F12's PE string rows become localized rows through
//! the caller's declared language map, and every row that cannot be decoded is
//! reported rather than dropped or guessed.
//!
//! The stage's minimum scenario lives in `screen`; this module owns the decode
//! report. No original data and no `CS_GAME_DIR` access: every row is authored
//! development content, so these tests prove the bridge and its error paths,
//! never the original installation.

use cs_content::localization::{LanguageMap, LanguageMapError, LocaleId, ResourceDecode, TextId};
use cs_types::content::{Origin, Provenance};

use crate::common::{claim, language_map, resource_row};

/// The decode maps a declared language id to its locale, keeps the row's text
/// verbatim under the stable resource id, and refuses to invent the rest.
#[test]
fn accept_f51_b_decoding_maps_declared_languages_and_reports_the_rest() {
    let locales = language_map(&[(1033, "en-us"), (1031, "de-de")]);
    let rows = [
        resource_row(0, 1033, Some("Confirm {pilot}")),
        resource_row(1, 1033, Some("Target lost")),
        // Undecodable units (an unpaired surrogate in the image): the row has
        // no text, so it must be reported, not turned into an empty string.
        resource_row(2, 1033, None),
        // A language the caller's map does not declare: reported, never
        // guessed from a built-in table.
        resource_row(3, 9999, Some("Unmapped")),
        // Two strings for one (id, locale) contradict each other; neither copy
        // is kept.
        resource_row(0, 1033, Some("Confirm again")),
    ];

    let decode = ResourceDecode::decode(
        &rows,
        &locales,
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    );

    assert_eq!(decode.rows(), 5, "every handed-in row is accounted for");
    assert_eq!(
        decode.decoded(),
        1,
        "only the unique, mapped, decodable row survives"
    );
    assert_eq!(decode.unmapped_languages(), &[9999]);
    assert_eq!(decode.undecodable_ids(), &[2]);
    assert_eq!(
        decode.duplicates(),
        &[(0, LocaleId::new("en-us").expect("valid locale"))]
    );
    assert!(!decode.is_complete());

    let en = LocaleId::new("en-us").expect("valid locale");
    let live = decode
        .catalog()
        .get(&TextId::from_resource_id(1), &en)
        .expect("the unique row decoded");
    assert_eq!(live.text(), "Target lost");
    assert_eq!(live.locale(), &en);
    assert_eq!(live.origin(), &Origin::SyntheticFixture);
    assert!(
        decode
            .catalog()
            .get(&TextId::from_resource_id(0), &en)
            .is_none(),
        "a duplicated (id, locale) pair keeps neither copy"
    );
}

/// A clean decode reports nothing missing and carries both declared languages.
#[test]
fn accept_f51_b_a_clean_decode_is_complete_and_locale_keyed() {
    let locales = language_map(&[(1033, "en-us"), (1031, "de-de")]);
    let rows = [
        resource_row(0, 1033, Some("Target lost")),
        resource_row(0, 1031, Some("Ziel verloren")),
    ];
    let decode = ResourceDecode::decode(
        &rows,
        &locales,
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    );

    assert!(decode.is_complete());
    assert_eq!(decode.rows(), 2);
    assert_eq!(decode.decoded(), 2);
    assert!(decode.unmapped_languages().is_empty());
    assert!(decode.undecodable_ids().is_empty());
    assert!(decode.duplicates().is_empty());

    let id = TextId::from_resource_id(0);
    assert_eq!(
        decode
            .catalog()
            .get(&id, &LocaleId::new("de-de").expect("valid locale"))
            .expect("the german row decoded")
            .text(),
        "Ziel verloren"
    );
    assert_eq!(decode.catalog().locales().len(), 2);
}

/// A language map cannot be empty (nothing could ever decode) or map one
/// resource language id twice (a row's locale would depend on order).
#[test]
fn accept_f51_b_a_language_map_refuses_empty_and_duplicate_languages() {
    assert_eq!(
        LanguageMap::new([]),
        Err(LanguageMapError::Empty),
        "an empty map would silently decode nothing"
    );

    let duplicate = LanguageMap::new([
        (1033, LocaleId::new("en-us").expect("valid locale")),
        (1033, LocaleId::new("de-de").expect("valid locale")),
    ]);
    assert_eq!(
        duplicate,
        Err(LanguageMapError::DuplicateLanguage { language: 1033 })
    );

    let too_long: Vec<(u32, LocaleId)> = (0..=cs_content::localization::MAX_LANGUAGE_MAP_LEN
        as u32)
        .map(|language| (language, LocaleId::new("en-us").expect("valid locale")))
        .collect();
    assert_eq!(
        LanguageMap::new(too_long),
        Err(LanguageMapError::TooLong {
            len: cs_content::localization::MAX_LANGUAGE_MAP_LEN + 1
        })
    );
}
