//! F27-E acceptance tests: the original's ammunition and gun identity
//! vocabulary, imported from the string catalog the installation ships.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`
//! (F27 "Enumerate all actual ammunition ids from original data"), AC04's
//! closure target. Task #545. Owner paths: `crates/cs_content/src/weapons.rs`
//! and this file.
//!
//! The fast half drives the production importer with a catalog shaped like the
//! measured one, so a change to the id tables, to the markup split, to the
//! refusals or to the declared records fails here. The retail half re-measures
//! every id against the owner's installation through the production PE
//! resource reader, so a stale constant fails instead of passing.
//!
//! Nothing here reproduces an original *sentence*: the assertions name the
//! original's own ammunition type labels, its abbreviations and its caliber
//! labels, which are the vocabulary F27 non-negotiable 1 asks to be
//! enumerated. The descriptive prose the same blocks carry is read by the
//! retail test only to assert it is present, never copied into this file.

use std::path::{Path, PathBuf};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_content::weapons::{
    AmmunitionId, DeclaredCaliber, DeclaredDamageChannel, InteractionOption,
    ORIGINAL_AMMUNITION_ABBREVIATION_IDS, ORIGINAL_AMMUNITION_DESCRIPTION_IDS,
    ORIGINAL_AMMUNITION_LONG_NAME_IDS, ORIGINAL_AMMUNITION_NONE_LABEL_IDS,
    ORIGINAL_AMMUNITION_SHORT_NAME_IDS, ORIGINAL_AMMUNITION_TYPE_COUNT,
    ORIGINAL_GUN_DESCRIPTION_IDS, ORIGINAL_GUN_LONG_NAME_IDS, ORIGINAL_GUN_SHORT_NAME_IDS,
    ORIGINAL_NO_GUN_LONG_NAME_ID, ORIGINAL_NO_GUN_SHORT_NAME_ID, ORIGINAL_SELECTABLE_GUN_COUNT,
    ORIGINAL_TEXT_MARKUP, OriginalAmmunitionIdentity, OriginalGunAmmunitionCatalogue,
    OriginalImportError, OriginalMeasuredText, OriginalStringTable,
    original_ammunition_behavior_claim, original_ammunition_caliber_claim,
};
use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimStatus, ContentHash};

/// The language id every measured row carries (F12-B's survey: `1033`,
/// `0x0409`, en-US, in all three surveyed images).
const ENGLISH_US: u32 = 1033;

/// The shipped UI language image whose `RT_STRING` blocks hold the text.
const LANGUI_DLL: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

/// The installation fingerprint every measurement in this file is bound to.
const INSTALL_SHA256: &str = "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";

/// The canonical-content fingerprint of the same installation.
const CONTENT_SHA256: &str = "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

/// The language image, whole file: size and digest as measured.
const LANGUI_DLL_LEN: u64 = 282_624;
const LANGUI_DLL_SHA256: &str = "357e6bb05f1d2872a00e0976fdde44561cd5bb6a56d9f555d85d0ff1481faf49";

/// The ammunition type labels the measured long-name block carries, in the
/// block's own order.
const MEASURED_LONG_NAMES: [&str; ORIGINAL_AMMUNITION_TYPE_COUNT] = [
    "Slugs",
    "Dum-Dum Bullets",
    "Armor-Piercing Bullets",
    "Explosive Bullets",
];

/// The ammunition type labels the measured short-name block carries.
const MEASURED_SHORT_NAMES: [&str; ORIGINAL_AMMUNITION_TYPE_COUNT] =
    ["Slug", "Dum-dum", "Armor-piercing", "Explosive"];

/// The abbreviations the measured abbreviation block carries.
const MEASURED_ABBREVIATIONS: [&str; ORIGINAL_AMMUNITION_TYPE_COUNT] = ["Slug", "DD", "AP", "EX"];

/// The caliber labels the measured gun short-name block carries, verbatim.
///
/// Each carries a leading space, so the measured row and the
/// [`DeclaredCaliber`] it normalizes to differ; the caliber is the trimmed
/// text, the row is what the installation holds.
const MEASURED_CALIBER_ROWS: [&str; ORIGINAL_SELECTABLE_GUN_COUNT] = [
    " .30-cal.",
    " .40-cal.",
    " .50-cal.",
    " .60-cal.",
    " .70-cal.",
];

/// A catalog shaped like the measured one: every required id carries a
/// `[COUR9]`-prefixed row, and the rows the blocks leave empty stay empty.
fn measured_catalog() -> OriginalStringTable {
    let mut table = OriginalStringTable::new();
    let rows = ORIGINAL_AMMUNITION_LONG_NAME_IDS
        .iter()
        .zip(MEASURED_LONG_NAMES)
        .map(|(id, text)| (*id, text))
        .chain(
            ORIGINAL_AMMUNITION_SHORT_NAME_IDS
                .iter()
                .zip(MEASURED_SHORT_NAMES)
                .map(|(id, text)| (*id, text)),
        )
        .chain(
            ORIGINAL_AMMUNITION_ABBREVIATION_IDS
                .iter()
                .zip(MEASURED_ABBREVIATIONS)
                .map(|(id, text)| (*id, text)),
        )
        .chain(
            ORIGINAL_AMMUNITION_DESCRIPTION_IDS
                .iter()
                .zip([
                    "slug text",
                    "dum-dum text",
                    "armor-piercing text",
                    "explosive text",
                ])
                .map(|(id, text)| (*id, text)),
        )
        .chain(
            ORIGINAL_GUN_LONG_NAME_IDS
                .iter()
                .zip(["gun one", "gun two", "gun three", "gun four", "gun five"])
                .map(|(id, text)| (*id, text)),
        )
        .chain(
            ORIGINAL_GUN_SHORT_NAME_IDS
                .iter()
                .zip(MEASURED_CALIBER_ROWS)
                .map(|(id, text)| (*id, text)),
        )
        .chain(
            ORIGINAL_GUN_DESCRIPTION_IDS
                .iter()
                .zip([
                    "blurb one",
                    "blurb two",
                    "blurb three",
                    "blurb four",
                    "blurb five",
                ])
                .map(|(id, text)| (*id, text)),
        );
    for (id, text) in rows {
        table.insert(id, format!("{ORIGINAL_TEXT_MARKUP}{text}"));
    }
    for id in ORIGINAL_AMMUNITION_NONE_LABEL_IDS {
        table.insert(id, format!("{ORIGINAL_TEXT_MARKUP}None"));
    }
    table.insert(
        ORIGINAL_NO_GUN_SHORT_NAME_ID,
        format!("{ORIGINAL_TEXT_MARKUP}No Gun"),
    );
    table.insert(ORIGINAL_NO_GUN_LONG_NAME_ID, "No Gun");
    table
}

/// The provenance a synthetic-but-measured catalog is imported with.
fn observed_provenance() -> Provenance {
    Provenance::new(
        cs_content::weapons::original_ammunition_claim(),
        ClaimStatus::ObservedTool,
        None,
    )
    .expect("an observed-tool provenance with no span is valid")
}

/// A source span over the synthetic catalog's stand-in, carrying a digest no
/// installation has.
fn synthetic_span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_bytes([0_u8; 32]),
        LANGUI_DLL,
        None,
        0,
        LANGUI_DLL_LEN,
        None,
    )
    .expect("the synthetic span is valid")
}

/// The id the importer derives for one ammunition selection.
fn ammunition_id(selection: u32) -> AmmunitionId {
    AmmunitionId::try_new(
        ContentId::parse(&format!("ammo/original-{selection}"))
            .expect("the derived ammunition id parses"),
    )
    .expect("the derived id is in the ammo namespace")
}

/// The measured blocks are four ids wide, at the ids the declares name, and
/// the loadout's "no ammunition" row follows each of the three name blocks.
#[test]
fn accept_f27_e_the_measured_name_blocks_are_four_wide_at_the_declares() {
    assert_eq!(ORIGINAL_AMMUNITION_TYPE_COUNT, 4);
    assert_eq!(ORIGINAL_SELECTABLE_GUN_COUNT, 5);
    assert_eq!(ORIGINAL_AMMUNITION_LONG_NAME_IDS, [3350, 3351, 3352, 3353]);
    assert_eq!(ORIGINAL_AMMUNITION_SHORT_NAME_IDS, [3360, 3361, 3362, 3363]);
    assert_eq!(
        ORIGINAL_AMMUNITION_ABBREVIATION_IDS,
        [3365, 3366, 3367, 3368]
    );
    assert_eq!(
        ORIGINAL_AMMUNITION_DESCRIPTION_IDS,
        [3370, 3371, 3372, 3373]
    );
    assert_eq!(ORIGINAL_AMMUNITION_NONE_LABEL_IDS, [3354, 3364, 3369]);
    assert_eq!(ORIGINAL_GUN_DESCRIPTION_IDS, [3330, 3331, 3332, 3333, 3334]);
    // Each ammunition block is a contiguous four-wide run, so its ids differ
    // from the block's first id by the type's index.
    for index in 0..ORIGINAL_AMMUNITION_TYPE_COUNT {
        let offset = index as u32;
        assert_eq!(ORIGINAL_AMMUNITION_LONG_NAME_IDS[index], 3350 + offset);
        assert_eq!(ORIGINAL_AMMUNITION_SHORT_NAME_IDS[index], 3360 + offset);
        assert_eq!(ORIGINAL_AMMUNITION_ABBREVIATION_IDS[index], 3365 + offset);
        assert_eq!(ORIGINAL_AMMUNITION_DESCRIPTION_IDS[index], 3370 + offset);
    }
}

/// Each gun block is a contiguous five-wide run at its declared id, and the
/// sixth row of the two name blocks is the empty gun slot.
#[test]
fn accept_f27_e_the_gun_rows_are_five_wide_runs_at_the_declares() {
    assert_eq!(ORIGINAL_GUN_LONG_NAME_IDS, [3310, 3311, 3312, 3313, 3314]);
    assert_eq!(ORIGINAL_GUN_SHORT_NAME_IDS, [3320, 3321, 3322, 3323, 3324]);
    assert_eq!(ORIGINAL_GUN_DESCRIPTION_IDS, [3330, 3331, 3332, 3333, 3334]);
    for index in 0..ORIGINAL_SELECTABLE_GUN_COUNT {
        let offset = index as u32;
        assert_eq!(ORIGINAL_GUN_LONG_NAME_IDS[index], 3310 + offset);
        assert_eq!(ORIGINAL_GUN_SHORT_NAME_IDS[index], 3320 + offset);
        assert_eq!(ORIGINAL_GUN_DESCRIPTION_IDS[index], 3330 + offset);
    }
    assert_eq!(
        ORIGINAL_NO_GUN_LONG_NAME_ID, 3315,
        "the sixth long-name row"
    );
    assert_eq!(
        ORIGINAL_NO_GUN_SHORT_NAME_ID, 3325,
        "the sixth short-name row"
    );
    // The empty slot must not be one of the five guns.
    for row in [ORIGINAL_NO_GUN_LONG_NAME_ID, ORIGINAL_NO_GUN_SHORT_NAME_ID] {
        assert!(!ORIGINAL_GUN_LONG_NAME_IDS.contains(&row));
        assert!(!ORIGINAL_GUN_SHORT_NAME_IDS.contains(&row));
    }
}

/// A catalog the importer accepts yields four named types and five named guns
/// with their caliber labels, in the loadout screen's own order.
#[test]
fn accept_f27_e_a_measured_catalog_imports_four_types_and_five_guns() {
    let catalogue =
        OriginalGunAmmunitionCatalogue::import(&measured_catalog(), observed_provenance())
            .expect("the measured catalog imports");

    assert_eq!(catalogue.ammunition().len(), ORIGINAL_AMMUNITION_TYPE_COUNT);
    assert_eq!(catalogue.guns().len(), ORIGINAL_SELECTABLE_GUN_COUNT);
    for (index, identity) in catalogue.ammunition().iter().enumerate() {
        let selection = index as u32 + 1;
        assert_eq!(identity.selection(), selection);
        assert_eq!(*identity.ammunition(), ammunition_id(selection));
        assert_eq!(identity.long_name().text(), MEASURED_LONG_NAMES[index]);
        assert_eq!(identity.short_name().text(), MEASURED_SHORT_NAMES[index]);
        assert_eq!(
            identity.abbreviation().text(),
            MEASURED_ABBREVIATIONS[index]
        );
        assert_eq!(identity.long_name().markup(), Some(ORIGINAL_TEXT_MARKUP));
        assert_eq!(
            identity.long_name().id(),
            ORIGINAL_AMMUNITION_LONG_NAME_IDS[index]
        );
        assert_eq!(
            identity.description().id(),
            ORIGINAL_AMMUNITION_DESCRIPTION_IDS[index]
        );
        assert!(!identity.description().is_empty());
    }
    for (index, gun) in catalogue.guns().iter().enumerate() {
        assert_eq!(gun.selection(), index as u32 + 1);
        assert_eq!(gun.short_name().text(), MEASURED_CALIBER_ROWS[index]);
        assert_eq!(gun.long_name().id(), ORIGINAL_GUN_LONG_NAME_IDS[index]);
        assert_eq!(
            gun.caliber(),
            &Resolved::Known(Known::new(
                DeclaredCaliber::try_new(MEASURED_CALIBER_ROWS[index])
                    .expect("the measured caliber label is valid"),
                observed_provenance(),
            ))
        );
    }
}

/// A row the catalog does not hold is refused **by id**, so a short catalog
/// cannot masquerade as a complete one.
#[test]
fn accept_f27_e_a_missing_row_is_refused_by_id() {
    let mut table = measured_catalog();
    table.remove(ORIGINAL_AMMUNITION_DESCRIPTION_IDS[3]);
    let error = OriginalGunAmmunitionCatalogue::import(&table, observed_provenance())
        .expect_err("a catalog without the fourth description is refused");
    assert_eq!(
        error,
        OriginalImportError::MissingString {
            id: 3373,
            role: "ammunition description",
        }
    );
}

/// A row the catalog holds but that carries no display text is refused too:
/// an empty gun row is not a gun.
#[test]
fn accept_f27_e_an_empty_row_is_refused_by_id() {
    let mut table = measured_catalog();
    table.insert(
        ORIGINAL_AMMUNITION_ABBREVIATION_IDS[2],
        ORIGINAL_TEXT_MARKUP.to_owned(),
    );
    let error = OriginalGunAmmunitionCatalogue::import(&table, observed_provenance())
        .expect_err("an empty abbreviation row is refused");
    assert_eq!(
        error,
        OriginalImportError::EmptyString {
            id: 3367,
            role: "ammunition abbreviation",
        }
    );
}

/// An empty catalog is refused on its first required row, naming that row.
#[test]
fn accept_f27_e_an_empty_catalog_is_refused_on_its_first_row() {
    let error =
        OriginalGunAmmunitionCatalogue::import(&OriginalStringTable::new(), observed_provenance())
            .expect_err("an empty catalog is refused");
    assert_eq!(
        error,
        OriginalImportError::MissingString {
            id: ORIGINAL_AMMUNITION_LONG_NAME_IDS[0],
            role: "ammunition long name",
        }
    );
}

/// Every declared record carries the measured identity and **no** invented
/// number: both damage channels and the caliber stay explicit unknowns with
/// their own claims.
#[test]
fn accept_f27_e_the_declared_records_carry_no_guessed_amount() {
    let catalogue =
        OriginalGunAmmunitionCatalogue::import(&measured_catalog(), observed_provenance())
            .expect("the measured catalog imports");
    let declared = catalogue
        .declared_ammunition(synthetic_span(), observed_provenance())
        .expect("records of unknown values are valid declared records");

    assert_eq!(declared.len(), ORIGINAL_AMMUNITION_TYPE_COUNT);
    for (record, identity) in declared.iter().zip(catalogue.ammunition()) {
        assert_eq!(record.ammunition(), identity.ammunition());
        assert!(matches!(record.origin(), Origin::Installation { .. }));
        assert_eq!(record.provenance().class, ClaimStatus::ObservedTool);
        let Resolved::Unknown { claim_id, reason } = record.caliber() else {
            panic!("the caliber belongs to the gun, so the type declares none");
        };
        assert_eq!(*claim_id, original_ammunition_caliber_claim());
        assert!(
            reason.contains("caliber"),
            "the caliber's reason names the caliber: {reason}"
        );
        assert_eq!(record.known_caliber(), None);
        for channel in DeclaredDamageChannel::ALL {
            assert_eq!(
                record.known_damage(*channel),
                None,
                "{channel} damage must stay unmeasured"
            );
            let Resolved::Unknown { claim_id, .. } = record
                .damage()
                .channel(*channel)
                .expect("both channels are declared")
            else {
                panic!("{channel} damage must stay unmeasured");
            };
            assert_eq!(*claim_id, original_ammunition_behavior_claim());
        }
        for option in [
            InteractionOption::SelfHit,
            InteractionOption::FriendlyFire,
            InteractionOption::Penetration,
            InteractionOption::Ricochet,
            InteractionOption::AmmoSwitching,
        ] {
            assert!(
                !record.rules().is_known(option),
                "{option} must stay unmeasured"
            );
        }
    }
}

/// The markup code is split off and kept, never interpreted: it is part of
/// the row, and the display text is what follows it.
#[test]
fn accept_f27_e_the_markup_code_is_split_off_and_kept() {
    let measured = OriginalMeasuredText::measure(3350, "[COUR9]Slug");
    assert_eq!(measured.markup(), Some("[COUR9]"));
    assert_eq!(measured.text(), "Slug");
    assert_eq!(measured.id(), 3350);

    let bare = OriginalMeasuredText::measure(3370, "no code here");
    assert_eq!(bare.markup(), None);
    assert_eq!(bare.text(), "no code here");

    // A bracket that is not an alphanumeric code is text, not markup.
    let prose = OriginalMeasuredText::measure(3371, "[not a code] tail");
    assert_eq!(prose.markup(), None);
    assert_eq!(prose.text(), "[not a code] tail");
}

/// The ammunition identity is reachable from the catalogue alone, so an audit
/// can ask what a type is called without a session.
#[test]
fn accept_f27_e_a_type_identity_is_queryable_by_selection() {
    let catalogue =
        OriginalGunAmmunitionCatalogue::import(&measured_catalog(), observed_provenance())
            .expect("the measured catalog imports");
    let third: &OriginalAmmunitionIdentity = catalogue
        .ammunition()
        .iter()
        .find(|identity| identity.selection() == 3)
        .expect("the third type is present");
    assert_eq!(third.short_name().text(), "Armor-piercing");
    assert_eq!(third.abbreviation().text(), "AP");
    assert_eq!(catalogue.ammunition().len(), 4);
}

// ---------------------------------------------------------------------------
// The retail half: the owner's installation, through production readers
// ---------------------------------------------------------------------------

fn retail_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the read-only original installation"),
    )
}

/// What the retail half measures once: the installed UI language image, read
/// through the production `StringCatalog`, and the catalog of the ids the
/// declares require.
struct Installed {
    table: OriginalStringTable,
    span: SourceSpan,
    strings: usize,
    blocks: usize,
}

fn measure_installed() -> Installed {
    let dir = retail_dir();
    assert!(dir.is_dir(), "CS_GAME_DIR {} is a directory", dir.display());
    let found = discover(&dir).expect("production discovery reads the installation");
    assert_eq!(
        fingerprint(&found.manifest).to_hex(),
        INSTALL_SHA256,
        "the installation fingerprint this measurement was taken against"
    );
    assert_eq!(
        content_fingerprint(&found.manifest).to_hex(),
        CONTENT_SHA256,
        "the canonical-content fingerprint this measurement was taken against"
    );

    let bytes = std::fs::read(dir.join(LANGUI_DLL))
        .unwrap_or_else(|error| panic!("{LANGUI_DLL}: the installation must hold it: {error}"));
    assert_eq!(bytes.len() as u64, LANGUI_DLL_LEN, "language image length");
    assert_eq!(
        sha256(&bytes).to_hex(),
        LANGUI_DLL_SHA256,
        "language image digest"
    );

    let span = SourceSpan::new(
        fingerprint(&found.manifest),
        LANGUI_DLL,
        None,
        0,
        bytes.len() as u64,
        None,
    )
    .expect("a valid span over the whole image");
    let mut context = ParseContext::with_defaults(LANGUI_DLL);
    let catalog = StringCatalog::read(&mut context, span.clone(), &bytes)
        .unwrap_or_else(|error| panic!("{LANGUI_DLL}: production catalog must read it: {error}"));

    let mut wanted: Vec<u32> = ORIGINAL_AMMUNITION_LONG_NAME_IDS
        .iter()
        .chain(&ORIGINAL_AMMUNITION_SHORT_NAME_IDS)
        .chain(&ORIGINAL_AMMUNITION_ABBREVIATION_IDS)
        .chain(&ORIGINAL_AMMUNITION_DESCRIPTION_IDS)
        .chain(ORIGINAL_AMMUNITION_NONE_LABEL_IDS.iter())
        .chain(&ORIGINAL_GUN_LONG_NAME_IDS)
        .chain(&ORIGINAL_GUN_SHORT_NAME_IDS)
        .chain(&ORIGINAL_GUN_DESCRIPTION_IDS)
        .copied()
        .collect();
    wanted.push(ORIGINAL_NO_GUN_LONG_NAME_ID);
    wanted.push(ORIGINAL_NO_GUN_SHORT_NAME_ID);
    wanted.sort_unstable();
    wanted.dedup();

    let mut table = OriginalStringTable::new();
    for id in wanted {
        let row = match catalog.resolve(id, Some(ENGLISH_US)) {
            StringLookup::Found(row) => row,
            StringLookup::Missing => {
                panic!("{LANGUI_DLL}: string {id} must resolve at {ENGLISH_US}")
            }
            StringLookup::Ambiguous(count) => {
                panic!("{LANGUI_DLL}: string {id} resolves {count} times at {ENGLISH_US}");
            }
        };
        let text = row
            .text
            .clone()
            .unwrap_or_else(|| panic!("{LANGUI_DLL}: string {id} must decode as text"));
        table.insert(id, text);
    }
    Installed {
        table,
        span,
        strings: catalog.accounting().strings,
        blocks: catalog.resources().strings().len(),
    }
}

/// The installation names exactly four ammunition types and five selectable
/// guns, and they are the ones this stage imports.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_retail_the_original_names_four_ammunition_types_and_five_guns() {
    let installed = measure_installed();
    assert_eq!(
        installed.blocks, 101,
        "the language image holds 101 RT_STRING blocks"
    );
    assert_eq!(installed.strings, 1616, "and 1616 counted rows across them");
    assert!(
        installed.table.len() < installed.strings,
        "this stage reads 36 of the image's rows"
    );

    // The "no ammunition" rows and the empty gun slot, as measured.
    for id in ORIGINAL_AMMUNITION_NONE_LABEL_IDS {
        let measured = OriginalMeasuredText::measure(
            id,
            installed.table.raw(id).expect("the none row is present"),
        );
        assert_eq!(
            measured.text(),
            "None",
            "string {id} is the empty-ammunition row"
        );
        assert_eq!(measured.markup(), Some(ORIGINAL_TEXT_MARKUP));
    }
    for id in [ORIGINAL_NO_GUN_LONG_NAME_ID, ORIGINAL_NO_GUN_SHORT_NAME_ID] {
        let measured =
            OriginalMeasuredText::measure(id, installed.table.raw(id).expect("the row is present"));
        assert_eq!(
            measured.text(),
            "No Gun",
            "string {id} is the empty gun slot"
        );
    }

    let catalogue = OriginalGunAmmunitionCatalogue::import(&installed.table, observed_provenance())
        .expect("the installation's catalog imports");
    assert_eq!(catalogue.ammunition().len(), ORIGINAL_AMMUNITION_TYPE_COUNT);
    assert_eq!(catalogue.guns().len(), ORIGINAL_SELECTABLE_GUN_COUNT);
    for (index, identity) in catalogue.ammunition().iter().enumerate() {
        assert_eq!(identity.selection(), index as u32 + 1);
        assert_eq!(identity.long_name().text(), MEASURED_LONG_NAMES[index]);
        assert_eq!(identity.short_name().text(), MEASURED_SHORT_NAMES[index]);
        assert_eq!(
            identity.abbreviation().text(),
            MEASURED_ABBREVIATIONS[index]
        );
        assert_eq!(identity.long_name().markup(), Some(ORIGINAL_TEXT_MARKUP));
        assert_eq!(
            identity.long_name().id(),
            ORIGINAL_AMMUNITION_LONG_NAME_IDS[index]
        );
        assert_eq!(
            identity.description().id(),
            ORIGINAL_AMMUNITION_DESCRIPTION_IDS[index]
        );
        assert!(
            identity.description().text().len() > 20,
            "every ammunition type carries a description the screens show"
        );
    }
    for (index, gun) in catalogue.guns().iter().enumerate() {
        assert_eq!(gun.selection(), index as u32 + 1);
        assert_eq!(gun.short_name().text(), MEASURED_CALIBER_ROWS[index]);
        assert_eq!(gun.short_name().markup(), Some(ORIGINAL_TEXT_MARKUP));
        assert_eq!(gun.long_name().id(), ORIGINAL_GUN_LONG_NAME_IDS[index]);
        assert_eq!(
            gun.long_name().markup(),
            None,
            "the measured gun long names carry no markup code"
        );
        assert!(
            gun.caliber().is_known(),
            "the caliber label is measured, so the caliber is known"
        );
        assert!(
            gun.description().text().len() > 20,
            "every gun carries a description the screens show"
        );
    }
}

/// The records built from the installation are original-origin and carry no
/// invented amount.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_retail_the_declared_ammunition_carries_no_guessed_amount() {
    let installed = measure_installed();
    let catalogue = OriginalGunAmmunitionCatalogue::import(&installed.table, observed_provenance())
        .expect("the installation's catalog imports");
    let declared = catalogue
        .declared_ammunition(installed.span, observed_provenance())
        .expect("records of unknown values are valid declared records");

    assert_eq!(declared.len(), ORIGINAL_AMMUNITION_TYPE_COUNT);
    for (record, identity) in declared.iter().zip(catalogue.ammunition()) {
        assert_eq!(record.ammunition(), identity.ammunition());
        match record.origin() {
            Origin::Installation { source } => {
                assert_eq!(source.container_path(), LANGUI_DLL);
                assert_eq!(source.length(), LANGUI_DLL_LEN);
            }
            other => panic!("an imported record must be original-origin, got {other:?}"),
        }
        assert_eq!(record.known_caliber(), None);
        for channel in DeclaredDamageChannel::ALL {
            assert_eq!(record.known_damage(*channel), None);
        }
        let Resolved::Unknown { claim_id, .. } = record.caliber() else {
            panic!("the type declares no caliber of its own");
        };
        assert_eq!(*claim_id, original_ammunition_caliber_claim());
    }
}

/// The measurement is bound to one installation: a different one would not
/// answer to these ids, these labels or these digests.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_retail_the_measured_surface_is_bound_to_one_installation() {
    let dir = retail_dir();
    let path: &Path = &dir.join(LANGUI_DLL);
    assert!(
        path.is_file(),
        "{} must be the shipped UI language image",
        path.display()
    );
    let installed = measure_installed();
    assert_eq!(installed.span.container_path(), LANGUI_DLL);
    assert_eq!(
        installed.table.len(),
        // four types x (long, short, abbreviation) + four descriptions
        // + three empty-ammunition rows + five guns x (long, short, description)
        // + two empty-gun rows.
        4 * 3 + 4 + 3 + 5 * 3 + 2,
        "the catalog holds every required row and nothing else"
    );
}
