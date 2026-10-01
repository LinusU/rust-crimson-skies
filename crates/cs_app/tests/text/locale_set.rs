//! F51-LOCALE-SET acceptance tests: the supported-locale set **measured** from
//! the original installation instead of declared by a caller, and the
//! id-stability comparison (F12 AC04) that one installation cannot answer.
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`
//! (deliverable, AC03 and AC04) and
//! `specs/F12-text-configuration-strings-and-pe-resources.md` AC04; shared
//! contract `docs/contracts/UI-NETWORK.md`. Task test prefix:
//! `accept_f51_locale_set_`.
//!
//! Every test calls production code: `cs_app::text::locale_measure` measures the
//! resource language table and the installation census,
//! `cs_content::localization::MeasuredLocales` derives the declaration from that
//! table, and `cs_content::localization::measure_id_stability` answers the
//! id-numbering question. No test declares a locale, a language id or a locale
//! label for an original measurement, and none carries its own counting or its
//! own decode.
//!
//! The synthetic tests are unignored so CI runs them. The retail test needs
//! `$CS_GAME_DIR`, so CI skips it with `#[ignore]`; the implementing and
//! reviewing agents run it with `--include-ignored`, and it fails loudly when the
//! capability is absent.

use cs_app::text::{
    LocalizationAuditRequest, StringImageMeasurement, StringImageSource, audit_localization,
    measure_installation_languages, measure_string_image_languages, string_image_languages,
    synthetic_monospace,
};
use cs_content::config::StringRow;
use cs_content::localization::{
    IdStability, LanguageMap, MAX_LANGUAGE_MAP_LEN, MAX_SUPPORTED_LOCALES, MeasuredLocales,
    MeasuredLocalesError, ResourceDecode, ResourceLanguageTable, SupportedLocales, TextCatalog,
    TextId, measure_id_stability, measured_locale_label,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::ContentHash;

use crate::common::{
    PANEL, RETAIL_STRING_IMAGES, buttons, claim, grammar, measured_locale, resource_row,
    retail_audit, retail_game_dir, retail_locale_measurement, substitutions,
};

/// The image digest a synthetic span carries. A declared value, not a
/// measurement: no synthetic fixture stands for an original installation.
const FIXTURE_INSTALL: ContentHash = ContentHash::from_bytes([9; 32]);

/// A container span for a synthetic image.
fn fixture_span(container: &str, length: u64) -> SourceSpan {
    SourceSpan::new(FIXTURE_INSTALL, container, None, 0, length, None)
        .expect("the fixture span is valid")
}

/// Rows of one synthetic string image, exactly as F12 hands them out: a resource
/// id and the third-level language id the image stored.
fn rows_of(language: u32, ids: &[u32]) -> Vec<StringRow> {
    ids.iter()
        .map(|id| resource_row(*id, language, Some("authored development string")))
        .collect()
}

/// A catalog of one synthetic image's strings in one language, decoded through
/// the production [`ResourceDecode`] so the fixture is production-shaped.
fn catalog_of(language: u32, texts: &[&str]) -> TextCatalog {
    let rows: Vec<StringRow> = texts
        .iter()
        .enumerate()
        .map(|(index, text)| resource_row(index as u32, language, Some(text)))
        .collect();
    let languages =
        LanguageMap::new([(language, measured_locale(language))]).expect("one language is valid");
    ResourceDecode::decode(
        &rows,
        &languages,
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    )
    .into_catalog()
}

/// The locale labels a declaration carries, in declaration order.
fn labels(measured: &MeasuredLocales) -> Vec<String> {
    measured
        .supported()
        .locales()
        .iter()
        .map(|locale| locale.as_str().to_owned())
        .collect()
}

/// The measurement input for two synthetic images: the first carries two
/// languages, the second carries one of them again.
fn two_language_measurement() -> Vec<StringImageMeasurement<'static>> {
    // The spans and rows are leaked on purpose: a fixture is a constant, and
    // borrowing a local would force every caller to keep it alive.
    fn leaked<T: 'static>(value: T) -> &'static T {
        Box::leak(Box::new(value))
    }
    let mut first_rows = rows_of(1033, &[0, 1, 2]);
    first_rows.extend(rows_of(1031, &[3, 4]));
    // A row whose code units did not decode is still evidence of the language
    // the image was built with, so the measurement counts it.
    first_rows.push(resource_row(5, 1031, None));
    let second_rows = rows_of(1033, &[0]);
    vec![
        StringImageMeasurement {
            span: leaked(fixture_span("fixture/first.dll", 512)),
            rows: leaked(first_rows),
        },
        StringImageMeasurement {
            span: leaked(fixture_span("fixture/second.dll", 256)),
            rows: leaked(second_rows),
        },
    ]
}

/// AC: the declared supported-locale set is derived from the measured resource
/// languages, keeps the evidence, and a measurement that measured nothing
/// declares nothing.
#[test]
fn accept_f51_locale_set_measured_resource_languages_derive_the_declared_locale_set() {
    let images = two_language_measurement();

    // Per image, before anything is aggregated: the distinct language ids and
    // each id's row count, in ascending id order.
    assert_eq!(
        string_image_languages(images[0].rows),
        vec![(1031, 3), (1033, 3)],
        "a row that did not decode is still evidence of its language"
    );
    assert_eq!(string_image_languages(images[1].rows), vec![(1033, 1)]);

    let table = measure_string_image_languages(&images);
    assert_eq!(table.languages(), vec![1031, 1033]);
    assert_eq!(
        table.containers(),
        3,
        "one occurrence per image and language"
    );
    assert_eq!(table.rows_for(1031), 3);
    assert_eq!(table.rows_for(1033), 4);
    // The measurement keeps the evidence: each occurrence names the container it
    // was read from, under the digest the caller measured.
    for observation in table.observations() {
        assert_eq!(observation.source.install_sha256(), FIXTURE_INSTALL);
        assert!(matches!(
            observation.source.container_path(),
            "fixture/first.dll" | "fixture/second.dll"
        ));
    }

    // The declaration is a function of the table: the measured ids, ascending,
    // each labelled by the measured id and by nothing else.
    let measured = MeasuredLocales::from_table(table).expect("a measured table declares locales");
    assert_eq!(labels(&measured), vec!["resource-1031", "resource-1033"]);
    assert_eq!(measured.len(), 2);
    assert!(measured.len() <= MAX_SUPPORTED_LOCALES);
    assert_eq!(
        measured.locale_for(1031).map(|locale| locale.as_str()),
        Some("resource-1031")
    );
    assert_eq!(
        measured.locale_for(1033).map(|locale| locale.as_str()),
        Some("resource-1033")
    );
    assert!(
        measured.locale_for(1036).is_none(),
        "an unmeasured id is not declared"
    );
    // The map the audit decodes with is the measured one.
    assert_eq!(measured.language_map().len(), 2);
    assert_eq!(
        measured
            .language_map()
            .locale(1033)
            .map(|locale| locale.as_str()),
        Some("resource-1033")
    );

    // A measurement with no occurrence declares nothing, and a table holding
    // more languages than a language map may hold is refused, not truncated.
    assert_eq!(
        MeasuredLocales::from_table(ResourceLanguageTable::new()),
        Err(MeasuredLocalesError::NothingMeasured)
    );
    let mut wide = ResourceLanguageTable::new();
    for language in 0..=(MAX_LANGUAGE_MAP_LEN as u32) {
        wide.observe(fixture_span("fixture/wide.dll", 1), language, 1);
    }
    assert_eq!(
        MeasuredLocales::from_table(wide),
        Err(MeasuredLocalesError::TooManyLanguages {
            len: MAX_LANGUAGE_MAP_LEN + 1,
        })
    );
}

/// AC (anti-guess): the audit follows the **measurement** rather than a caller's
/// list — a language the declaration does not carry is reported as unmapped, and
/// an under-declared measurement is an incomplete audit rather than a pass.
#[test]
fn accept_f51_locale_set_the_audit_follows_the_measurement_and_never_a_guessed_locale() {
    let images = two_language_measurement();
    let measured =
        MeasuredLocales::from_table(measure_string_image_languages(&images)).expect("declared");
    // Declare only one of the two measured languages, the way a caller-declared
    // set could: the audit must report the rows it can no longer decode instead
    // of falling back to a language nobody measured.
    let under_declared =
        SupportedLocales::new([measured_locale(1033)]).expect("one declared locale is a valid set");
    let partial_map =
        LanguageMap::new([(1033, measured_locale(1033))]).expect("one declared language is valid");

    let sources: Vec<StringImageSource<'_>> = images
        .iter()
        .map(|image| StringImageSource {
            path: image.span.container_path(),
            rows: image.rows,
            origin: Origin::SyntheticFixture,
            provenance: Provenance::designed(claim()),
        })
        .collect();
    let audit = audit_localization(&LocalizationAuditRequest {
        images: &sources,
        locales: &under_declared,
        language_map: &partial_map,
        grammar: &grammar(),
        metrics: &synthetic_monospace(16.0),
        panel: PANEL,
        required: &buttons(),
        substitutions: &substitutions(),
        media: &[],
    });

    assert_eq!(audit.images.len(), 2);
    for image in &audit.images {
        assert_eq!(
            image.locales.len(),
            1,
            "only the declared locale is audited"
        );
        assert!(
            image.locale(&measured_locale(1031)).is_none(),
            "a locale the measurement did not declare is never audited"
        );
    }
    assert_eq!(
        audit.blockers_with_code("unmapped_language").count(),
        1,
        "the image that carries the undeclared language reports it"
    );
    // The fully measured declaration is the one that leaves nothing unmapped.
    let full = audit_localization(&LocalizationAuditRequest {
        images: &sources,
        locales: measured.supported(),
        language_map: measured.language_map(),
        grammar: &grammar(),
        metrics: &synthetic_monospace(16.0),
        panel: PANEL,
        required: &buttons(),
        substitutions: &substitutions(),
        media: &[],
    });
    assert_eq!(full.blockers_with_code("unmapped_language").count(), 0);
    assert_eq!(
        full.images[0].locales.len(),
        2,
        "both measured locales are audited"
    );
}

/// AC: the id-numbering comparison answers F12 AC04 — a localized installation
/// keeps the ids while the display text changes — and it names renumbering when
/// the ids do not match.
#[test]
fn accept_f51_locale_set_id_numbering_is_compared_across_two_measured_locales() {
    let first = catalog_of(1033, &["Attack the ferry", "Out of fuel"]);
    // The same ids, translated: stable numbering, changed display text.
    let localized = catalog_of(1031, &["Greift die Fähre an", "Der Treibstoff ist leer"]);
    let stability = measure_id_stability(
        &first,
        &measured_locale(1033),
        Some((&localized, &measured_locale(1031))),
    );
    assert!(stability.is_compared());
    assert!(stability.is_stable(), "the same ids under both locales");
    let numbering = stability.numbering().expect("two locales were compared");
    assert_eq!(numbering.compared(), 2);
    assert_eq!(numbering.renumbered(), 0);
    assert_eq!(numbering.changed(), 2, "the display text changed");
    assert_eq!(numbering.identical_text, 0);

    // A localized build that dropped an id and added another is not stable, and
    // the difference is named per id rather than summarised away.
    let renumbered = catalog_of(1031, &["Eins", "Zwei", "Drei"]);
    let unstable = measure_id_stability(
        &first,
        &measured_locale(1033),
        Some((&renumbered, &measured_locale(1031))),
    );
    assert!(!unstable.is_stable());
    let numbering = unstable.numbering().expect("two locales were compared");
    assert_eq!(numbering.only_second, vec![TextId::from_resource_id(2)]);
    assert_eq!(numbering.only_first, Vec::new());
    assert_eq!(numbering.renumbered(), 1);
    assert_eq!(numbering.compared(), 2);

    // An untranslated duplicate is stable, and the text counts say why.
    let identical = catalog_of(1031, &["Attack the ferry", "Out of fuel"]);
    let unchanged = measure_id_stability(
        &first,
        &measured_locale(1033),
        Some((&identical, &measured_locale(1031))),
    );
    assert!(unchanged.is_stable());
    let numbering = unchanged.numbering().expect("two locales were compared");
    assert_eq!(numbering.identical_text, 2);
    assert_eq!(numbering.changed(), 0);
}

/// AC: with one installation the id-numbering question is **not** answered, and
/// the result names the locale that was measured instead of claiming stability.
#[test]
fn accept_f51_locale_set_one_measured_locale_reports_no_comparison_rather_than_stability() {
    let only = catalog_of(1033, &["Attack the ferry"]);
    let stability = measure_id_stability(&only, &measured_locale(1033), None);
    assert!(
        !stability.is_compared(),
        "no second installation was measured"
    );
    assert!(
        !stability.is_stable(),
        "a single installation is not evidence of id stability (F12 AC04)"
    );
    assert!(stability.numbering().is_none());
    assert_eq!(
        stability,
        IdStability::SingleLocale {
            measured: measured_locale(1033),
            ids: 1,
        }
    );

    // Comparing a locale with itself is stable, which is exactly why a claim
    // needs a *second* catalog: the ids a locale answers are its own set.
    let against_itself = only.compare_locale_ids(&measured_locale(1033), &measured_locale(1033));
    assert!(against_itself.is_stable());
    assert_eq!(against_itself.compared(), 1);
}

/// Retail (capability `retail`): the supported-locale set is what the
/// installation's own files measure, the whole-installation census shows no
/// other resource language hiding elsewhere, and the id-stability answer is the
/// honest one-locale one.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f51_locale_set_retail_the_installation_declares_only_measured_locales() {
    let dir = retail_game_dir();
    let measured = retail_locale_measurement(&dir);

    // 1. The measured table: which resource languages the localization surface
    //    carries, and which container each came from.
    assert_eq!(
        measured.table.languages(),
        vec![1033],
        "the three routed string images carry one measured resource language"
    );
    assert_eq!(
        measured.table.containers(),
        RETAIL_STRING_IMAGES.len(),
        "each routed image contributed one occurrence"
    );
    assert_eq!(
        measured.table.rows_for(1033),
        1_792 + 1_616 + 48,
        "every F12 row of every routed image is evidence of the measured language"
    );
    for (observation, spelling) in measured
        .table
        .observations()
        .iter()
        .zip(RETAIL_STRING_IMAGES)
    {
        assert_eq!(observation.source.container_path(), *spelling);
        assert_eq!(observation.source.install_sha256(), measured.install);
    }

    // 2. The declaration derived from it names the measured id and no language.
    assert_eq!(labels(&measured.declared), vec!["resource-1033"]);
    assert_eq!(measured.declared.languages(), vec![1033]);
    assert_eq!(
        measured
            .declared
            .locale_for(1033)
            .map(|locale| locale.as_str()),
        Some(measured_locale_label(1033).as_str()),
        "the label spells the measured id, never a language name"
    );

    // 3. The F51-D audit of the installation now runs against that measured
    //    declaration: no language id is unmapped and no locale is undeclared.
    let audit = retail_audit(&dir);
    for spelling in RETAIL_STRING_IMAGES {
        let image = audit.image(spelling).expect("every image was audited");
        assert_eq!(image.locales.len(), 1, "{spelling} has one measured locale");
        let coverage = image
            .locale(&measured_locale(1033))
            .expect("the measured locale is audited");
        assert_eq!(coverage.missing, 0, "{spelling} is fully answered");
        assert_eq!(coverage.translated, image.ids);
    }
    assert_eq!(
        audit.blockers_with_code("unmapped_language").count(),
        0,
        "the measured language map covers every measured row"
    );
    assert_eq!(audit.blockers_with_code("undeclared_locale").count(), 0);

    // 4. The corroborating census. Every PE image the production discovery
    //    inventoried is measured, and the result is the reason an
    //    installation-wide PE census must not be used to declare supported
    //    locales: the *game's* images carry only the measured language, while
    //    the other language ids in the tree belong to third-party runtime
    //    images that ship localized resources whatever language the game is in.
    let census = measure_installation_languages(&dir).expect("the installation census reads");
    assert_eq!(census.install, measured.install, "one installation digest");
    assert_eq!(
        census.images.len() + census.without_resources.len() + census.not_pe.len(),
        census.files,
        "every inventoried file is measured as a PE image or recorded as not one"
    );
    assert!(
        census.images.len() > measured.table.containers(),
        "the census walked every inventoried file, not only the routed images"
    );
    for spelling in RETAIL_STRING_IMAGES {
        let image = census
            .image(spelling)
            .expect("each routed image has resources");
        assert!(
            image.carries_only(1033),
            "{spelling} carries only the measured language"
        );
        assert!(
            image.leaves > 0 && image.string_blocks > 0,
            "{spelling} has strings"
        );
    }
    // The game executable and the English setup are the game too, and they are
    // single-language as well.
    for spelling in ["crimson.exe", "SETUPENU.DLL"] {
        let image = census
            .image(spelling)
            .expect("the game executable and setup are PE images");
        assert!(
            image.carries_only(1033),
            "{spelling} carries only the measured language"
        );
    }
    // The census classified every file rather than skipping it: a game archive
    // is not a PE image, and a PE image whose resource data directory declares
    // zero bytes is an image without resources.
    assert!(
        census
            .not_pe
            .iter()
            .any(|file| file.path == "GOSDATA/ASSETS/crimson.rof"),
        "a game archive is recorded as not a PE image"
    );
    assert!(
        census
            .without_resources
            .iter()
            .any(|file| file.path == "GOSDATA/ASSETS/BINARIES/roffile.dll"),
        "a PE image with an empty resource directory is recorded separately"
    );

    // The language ids that are *not* the measured one exist installation-wide,
    // and every one of them comes from a third-party image.
    let other_languages: Vec<u32> = census
        .languages()
        .into_iter()
        .filter(|language| *language != 1033)
        .collect();
    assert!(
        !other_languages.is_empty(),
        "the installation does carry other resource languages, so the census is not a tautology"
    );
    let mut carriers: Vec<&str> = census
        .images
        .iter()
        .filter(|image| image.languages.iter().any(|language| *language != 1033))
        .map(|image| image.path.as_str())
        .collect();
    carriers.sort_unstable();
    assert_eq!(
        carriers,
        vec!["clokspl.exe", "dsetup32.dll", "mcp.dll"],
        "only third-party runtime images carry another resource language"
    );

    // 5. F12 AC04. With one installation the answer is the honest "one locale"
    //    one. When the owner exposes a second, localized installation through
    //    `CS_LOCALIZED_GAME_DIR`, the same measurement compares the two.
    let decoded = measured.catalog(0);
    let first = measured_locale(1033);
    let stability = match std::env::var_os("CS_LOCALIZED_GAME_DIR") {
        Some(second) => {
            let localized = retail_locale_measurement(std::path::Path::new(&second));
            assert_ne!(
                localized.install, measured.install,
                "the second installation must be a different installation"
            );
            let other = localized.catalog(0);
            let other_language = *localized
                .declared
                .languages()
                .first()
                .expect("a measured installation declares a locale");
            let other_locale = localized
                .declared
                .locale_for(other_language)
                .expect("the measured locale is declared")
                .clone();
            assert_ne!(
                other_language, 1033,
                "a localized installation must carry a language of its own"
            );
            measure_id_stability(
                decoded.catalog(),
                &first,
                Some((other.catalog(), &other_locale)),
            )
        }
        None => measure_id_stability(decoded.catalog(), &first, None),
    };
    match &stability {
        IdStability::Compared { numbering, .. } => {
            println!(
                "two installations measured: compared {}, renumbered {}, changed {}",
                numbering.compared(),
                numbering.renumbered(),
                numbering.changed()
            );
            assert_eq!(
                numbering.renumbered(),
                0,
                "F12 AC04: a localized installation keeps the id numbering"
            );
        }
        IdStability::SingleLocale { measured, ids } => {
            assert_eq!(measured, &first);
            assert_eq!(*ids, decoded.catalog().ids_for(&first).len());
            assert!(
                !stability.is_stable(),
                "one locale cannot show id stability"
            );
        }
    }
}
