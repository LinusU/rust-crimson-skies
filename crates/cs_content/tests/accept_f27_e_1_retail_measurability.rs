//! F27-E.1 retail acceptance tests: is the original's per-type ammunition damage
//! and per-airframe gun-mount assignment measurable from the files the
//! installation ships?
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`
//! (non-negotiable 1 "no unverified multiplier table", non-negotiable 2 "mount
//! transforms come from the live aircraft hierarchy", and F27-D's AC04 closure
//! target). Task #547. Owner paths: `crates/cs_content/src/weapons.rs` and this
//! file.
//!
//! F27-E recorded, as a fidelity limitation, that the damage amounts and the
//! mount assignment live in the executable and in no shipped file. Recording
//! that is cheap; **measuring** it is not, and this file is the measurement. It
//! reads four places a table like that would have to be, through production
//! readers only, and asserts a falsifiable property of each:
//!
//! 1. the language image's ammunition rows state **no number at all**
//!    (`accept_f27_e_1_retail_no_shipped_ammunition_row_states_a_damage_amount`);
//! 2. its gun rows are **caliber labels**, five distinct bores and nothing else
//!    (`accept_f27_e_1_retail_the_five_caliber_rows_carry_only_a_caliber`);
//! 3. neither generated header declares a damage or ballistic constant
//!    (`accept_f27_e_1_retail_the_two_generated_headers_declare_no_damage_constant`);
//! 4. the image that would hold the table has a packed code section and a
//!    resource directory with no room for a table in it
//!    (`accept_f27_e_1_retail_the_image_that_would_hold_the_table_carries_no_plaintext`);
//! 5. the shared aircraft geometry names gun meshes and **never** one of the
//!    twenty declared gun groups
//!    (`accept_f27_e_1_retail_no_plane_node_names_a_declared_gun_group`).
//!
//! The last test is the accounting the stage owes: every `f27.d.limit.*` claim
//! is recorded as deferred **and re-filed**, so "we could not measure it" has a
//! destination instead of disappearing
//! (`accept_f27_e_1_retail_every_f27_d_limit_claim_is_deferred_and_re_filed`).
//!
//! **No original display text is asserted or committed.** Each row is reduced to
//! its id, its length and its digit set, which is what "does it state a number?"
//! is a question about; the prose the original wrote is read and only measured.

#[path = "f27_e_1_support/mod.rs"]
mod support;

use std::collections::BTreeSet;

use cs_content::weapons::{
    LimitEvidence, LimitOutcome, ORIGINAL_AMMO_NAME_BLOCKS, ORIGINAL_GUN_GROUPS,
    ORIGINAL_GUN_LONG_NAME_IDS, ORIGINAL_GUN_SHORT_NAME_IDS, ORIGINAL_SELECTABLE_GUNS,
    OriginalLimitClaim, OriginalLimitReport,
};
use support::*;

/// The resource **types** `crimson.exe`'s resource directory holds, measured:
/// icon, version and group icon. `RT_STRING` is type 6 and is **not** among
/// them, so the executable that would hold the damage table carries no string
/// table at all.
const EXECUTABLE_RESOURCE_TYPES: [u32; 3] = [3, 14, 16];

/// The `RT_STRING` resource type, named so the assertion above reads as a
/// statement about it rather than about `6`.
const RT_STRING: u32 = 6;

/// **The original's own description of each ammunition type states no damage
/// amount**: after the shipped markup code is split off, none of the four rows
/// carries an ASCII digit. That is the falsifiable form of "the shipped text
/// holds no damage table" — a `12.5` or a `30%` in any of the four rows fails
/// here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_1_retail_no_shipped_ammunition_row_states_a_damage_amount() {
    let root = game_dir();
    let (description_base, description_count) = ammunition_description_block(&root);
    assert_eq!(
        description_count, 4,
        "the original declares four ammunition types"
    );

    let descriptions = read_rows(&root, description_base, description_count);
    assert_eq!(descriptions.len(), 4);
    for row in &descriptions {
        assert_eq!(
            row.language, ENGLISH_US,
            "row {} is the English string the screens ask for",
            row.id
        );
        assert!(row.marked, "row {} carries the shipped markup code", row.id);
        assert!(!row.empty, "row {} is not empty", row.id);
        assert!(
            row.digits.is_empty(),
            "row {} states a damage amount: digits {:?}",
            row.id,
            row.digits
        );
        assert!(
            row.code_units > 40,
            "row {} is prose, not a table row: {} code units",
            row.id,
            row.code_units
        );
    }

    // The three ammunition name blocks are checked the same way: a type's name,
    // short name and abbreviation are the vocabulary F27 non-negotiable 1 asks
    // for, and a number in any of them would be an amount wearing a label.
    let header = read_member(&root, RESOURCE_HEADER);
    for (macro_name, role) in MEASURED_BLOCK_MACROS {
        if macro_name == "IDS_GUNSHORTNAME" || macro_name == "IDS_GUNLONGNAME" {
            continue;
        }
        let base = declared_id(&header, macro_name);
        let rows = read_rows(&root, base, description_count);
        assert!(!rows.is_empty(), "{macro_name} must carry rows: {role}");
        for row in &rows {
            assert!(!row.empty, "row {} of {macro_name} is not empty", row.id);
            assert!(
                row.digits.is_empty(),
                "row {} of {macro_name} states a number: {:?}",
                row.id,
                row.digits
            );
            assert!(
                row.marked,
                "row {} of {macro_name} carries the shipped markup code",
                row.id
            );
        }
    }

    // The gun *long* names are the one run that legitimately states a number —
    // their own caliber — so the allowance is exactly the caliber row at the
    // same index and nothing more. All five are measured: the run is as wide as
    // the caliber run, and each row is paired with that row's own caliber rather
    // than with whichever row happens to sit at the same offset.
    let (caliber_base, caliber_count) = gun_caliber_block(&root);
    let caliber_digits: Vec<Vec<char>> = read_rows(&root, caliber_base, caliber_count)
        .iter()
        .map(|row| row.digits.clone())
        .collect();
    let long_base = declared_id(&header, "IDS_GUNLONGNAME");
    let long_rows = read_rows(&root, long_base, caliber_count);
    assert_eq!(
        long_rows.len(),
        caliber_digits.len(),
        "the gun name run is as wide as the caliber run"
    );
    for (index, row) in long_rows.iter().enumerate() {
        assert!(!row.empty, "row {} of IDS_GUNLONGNAME is not empty", row.id);
        assert!(
            !row.marked,
            "row {} of IDS_GUNLONGNAME carries no markup code: it is the one run that does not",
            row.id
        );
        assert_eq!(
            &row.digits, &caliber_digits[index],
            "a gun name states its own caliber and no other number"
        );
    }
}

/// **The five gun rows are caliber labels and nothing else**: each carries
/// exactly the two digits of its bore, and the five bores are the five distinct
/// digits `3`–`7` followed by `0`. A row that carried a damage amount, or two
/// guns sharing a caliber, fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_1_retail_the_five_caliber_rows_carry_only_a_caliber() {
    let root = game_dir();
    let (base, count) = gun_caliber_block(&root);
    assert_eq!(count, 5, "the original offers five selectable guns");

    let rows = read_rows(&root, base, count);
    assert_eq!(rows.len(), 5);
    let mut bores = BTreeSet::new();
    for row in &rows {
        assert!(row.marked, "row {} carries the markup code", row.id);
        assert!(!row.empty, "row {} is not empty", row.id);
        assert_eq!(
            row.digits.len(),
            2,
            "row {} is a caliber label, so it carries two digits: {:?}",
            row.id,
            row.digits
        );
        assert_eq!(
            row.digits[1], '0',
            "row {} spells its bore as a tenth: {:?}",
            row.id, row.digits
        );
        assert!(
            row.code_units <= 20,
            "row {} is a label, not a sentence: {} code units",
            row.id,
            row.code_units
        );
        bores.insert(row.digits[0]);
    }
    assert_eq!(
        bores,
        BTreeSet::from(['3', '4', '5', '6', '7']),
        "five guns, five distinct bores"
    );
}

/// **Neither generated header declares a damage or ballistic constant.** Every
/// `#define` value in both is a resource id or a `0x` literal — none spells a
/// decimal point or a comma — and every define whose *name* mentions a gun, an
/// ammunition or armor is an **identifier**: not one of them is named after a
/// damage, caliber, penetration, ricochet or bullet quantity.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_1_retail_the_two_generated_headers_declare_no_damage_constant() {
    let root = game_dir();
    let mut ballistic = 0_usize;
    for member in MEMBERS {
        let rows = defines(&root, member);
        assert!(!rows.is_empty(), "{member} must declare defines");
        for (name, value) in &rows {
            assert!(
                !value.contains('.') && !value.contains(','),
                "{member}: {name} spells a decimal or a comma in {value:?}, and a damage \
                 multiplier would need one"
            );
            let lower = name.to_ascii_lowercase();
            if BALLISTIC_WORDS.iter().any(|word| lower.contains(word)) {
                ballistic += 1;
                for word in AMOUNT_WORDS {
                    assert!(
                        !lower.contains(word),
                        "{member}: {name} names a {word} quantity, so the header does declare one"
                    );
                }
            }
        }
    }
    assert!(
        ballistic >= 26,
        "the headers' gun, ammunition and armor identifiers are measured ({ballistic} of them), \
         so the assertion above had something to check"
    );

    // The vocabulary the stage relies on is measured here rather than asserted:
    // the header declares all twenty gun groups and all six row blocks.
    let header = read_member(&root, RESOURCE_HEADER);
    for group in ORIGINAL_GUN_GROUPS {
        let macro_name = gun_group_macro(&group);
        let id = declared_id(&header, &macro_name);
        assert_eq!(
            id,
            group.id(),
            "{macro_name} must be declared at the id the schema pins"
        );
    }
    for (macro_name, _) in MEASURED_BLOCK_MACROS {
        declared_id(&header, macro_name);
    }
    // The four ammunition row blocks are pinned against the schema's own table as
    // well, so a header that renumbered them fails here instead of quietly
    // changing which ids this stage reads rows from — and the two gun name blocks
    // are pinned against F27-E's measured gun-name runs.
    for (macro_name, label) in [
        ("IDS_AMMOLONGNAME", "ammo_long_name"),
        ("IDS_AMMOSHORTNAME", "ammo_short_name"),
        ("IDS_AMMOABBRNAME", "ammo_abbreviation"),
        ("IDS_AMMODESCRIPTION", "ammo_description"),
    ] {
        let pinned = ORIGINAL_AMMO_NAME_BLOCKS
            .iter()
            .find(|(_, block)| *block == label)
            .map(|(id, _)| *id)
            .unwrap_or_else(|| panic!("the schema carries the {label} block"));
        assert_eq!(
            declared_id(&header, macro_name),
            pinned,
            "{macro_name} must be declared at the id the schema pins"
        );
    }
    for (macro_name, pinned) in [
        ("IDS_GUNLONGNAME", ORIGINAL_GUN_LONG_NAME_IDS[0]),
        ("IDS_GUNSHORTNAME", ORIGINAL_GUN_SHORT_NAME_IDS[0]),
    ] {
        assert_eq!(
            declared_id(&header, macro_name),
            pinned,
            "{macro_name} must be declared at the base id of its measured run"
        );
    }
    // The rows the runs are read from are the schema's own ids, one by one.
    let long_rows = read_rows(
        &root,
        ORIGINAL_GUN_LONG_NAME_IDS[0],
        ORIGINAL_SELECTABLE_GUNS,
    );
    assert_eq!(
        long_rows.iter().map(|row| row.id).collect::<Vec<u32>>(),
        ORIGINAL_GUN_LONG_NAME_IDS.to_vec(),
        "the gun long-name run is exactly the five measured ids"
    );
    let caliber_rows = read_rows(
        &root,
        ORIGINAL_GUN_SHORT_NAME_IDS[0],
        ORIGINAL_SELECTABLE_GUNS,
    );
    assert_eq!(
        caliber_rows.iter().map(|row| row.id).collect::<Vec<u32>>(),
        ORIGINAL_GUN_SHORT_NAME_IDS.to_vec(),
        "the caliber run is exactly the five measured ids"
    );
}

/// **The image that would hold the table carries no plaintext to read.** The
/// executable's first code section is at the entropy of packed or encrypted
/// bytes, its `.rsrc` section is almost entirely zero with a resource directory
/// far too small to describe a table, and the directory holds no `RT_STRING`.
/// The second image beside it is a dense `MZ` blob whose bytes carry at most a
/// handful of the words a damage table would be spelled with.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_1_retail_the_image_that_would_hold_the_table_carries_no_plaintext() {
    let root = game_dir();
    let image = read_image(&root, CRIMSON_EXE);
    assert!(image.is_mz, "{CRIMSON_EXE} is an MZ image");
    let names: Vec<&str> = image
        .sections
        .iter()
        .map(|section| section.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            ".txt", ".text", ".txt2", ".rdata", ".data", ".rsrc", ".reloc"
        ],
        "the executable's section names are measured"
    );
    let packed = &image.sections[0];
    assert_eq!(packed.name, ".txt", "the first code section is .txt");
    assert!(
        packed.entropy >= 7.9,
        ".txt is at the entropy of packed bytes: {}",
        packed.entropy
    );
    let resources = image
        .sections
        .iter()
        .find(|section| section.name == ".rsrc")
        .expect("the executable has a .rsrc section");
    assert!(
        resources.raw_size >= 131_072,
        ".rsrc is {} bytes",
        resources.raw_size
    );
    assert!(
        resources.zero_bytes * 100 >= resources.raw_size as u64 * 99,
        "{} of {} .rsrc bytes are zero",
        resources.zero_bytes,
        resources.raw_size
    );
    let directory = image
        .resource_directory_size
        .expect("the executable declares a resource directory");
    assert!(
        f64::from(directory) / f64::from(resources.raw_size) <= 0.05,
        "the resource directory describes {directory} bytes of a {} byte section, so there is \
         no room in it for an ammunition table",
        resources.raw_size
    );
    assert_eq!(
        image.resource_types,
        EXECUTABLE_RESOURCE_TYPES.to_vec(),
        "the executable's resource types are icon, version and group icon"
    );
    assert!(
        !image.resource_types.contains(&RT_STRING),
        "the executable carries no RT_STRING resource"
    );

    let icd = read_image(&root, CRIMSON_ICD);
    assert!(icd.is_mz, "{CRIMSON_ICD} is a second MZ image");
    let whole = entropy(&read_loose(&root, CRIMSON_ICD));
    assert!(
        whole >= 7.8,
        "{CRIMSON_ICD} is dense across its whole length: {whole}"
    );
    let counts = keyword_counts(&root, CRIMSON_ICD);
    let total: u64 = counts.iter().map(|(_, count)| *count).sum();
    assert!(
        total <= 3,
        "{CRIMSON_ICD} carries {total} occurrences of the eleven words a damage table would be \
         spelled with: {counts:?}"
    );
}

/// **The shared aircraft geometry names gun meshes, never a declared gun
/// group.** None of the twenty `IDS_*GUNS` labels appears in any of the
/// container's node names, which is why the eleven wing-station groups cannot be
/// put on a side from the mesh data and why `covers_group` maps only the nine
/// the original's own labels determine.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_1_retail_no_plane_node_names_a_declared_gun_group() {
    let root = game_dir();
    let census = read_node_census(&root);
    assert_eq!(
        census.nodes, 3_317,
        "the container's node count is measured"
    );
    assert!(
        census.gun_bearing.len() > 10,
        "the container does name gun meshes: {:?}",
        census.gun_bearing
    );
    assert!(
        census.declared_groups_present.is_empty(),
        "no declared gun group is a node name; found {:?}",
        census.declared_groups_present
    );
}

/// **Every `f27.d.limit.*` claim is deferred *and* re-filed**, so the stage's
/// open questions have a destination in the machine-readable report rather than
/// only in a findings file. The report is complete as *accounting* and resolves
/// nothing: `bound()` is empty.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_e_1_retail_every_f27_d_limit_claim_is_deferred_and_re_filed() {
    let root = game_dir();
    // The measurements above are what make each claim unmeasurable; reading one
    // keeps the test from passing on an installation that was never opened.
    let census = read_node_census(&root);
    assert!(census.declared_groups_present.is_empty());

    let table: BTreeSet<&str> = REFILED.iter().map(|(claim, _)| *claim).collect();
    let claims: BTreeSet<&str> = OriginalLimitClaim::ALL
        .iter()
        .map(|claim| claim.claim_id())
        .collect();
    assert_eq!(
        table, claims,
        "the re-filing table covers exactly the five claims, no more and no fewer"
    );

    let mut report = OriginalLimitReport::new();
    for claim in OriginalLimitClaim::ALL {
        report
            .record(
                claim,
                LimitEvidence::Unmeasurable {
                    reason: claim.deferral_reason().to_owned(),
                },
            )
            .expect("a deferral is recorded");
    }
    for (claim_id, target) in REFILED {
        let claim = OriginalLimitClaim::ALL
            .iter()
            .copied()
            .find(|claim| claim.claim_id() == claim_id)
            .unwrap_or_else(|| panic!("{claim_id} is not a known claim"));
        report
            .refile(claim, target)
            .unwrap_or_else(|error| panic!("{claim_id} must be re-fileable: {error}"));
    }

    assert!(
        report.is_complete(),
        "every claim is accounted for: {:?}",
        report.unaccounted()
    );
    assert!(
        report.unaccounted().is_empty(),
        "nothing is left unaccounted"
    );
    assert!(
        report.bound().is_empty(),
        "no claim was resolved: {:?}",
        report.bound()
    );
    assert_eq!(report.deferred(), OriginalLimitClaim::ALL.to_vec());
    for row in report.rows() {
        match row.outcome() {
            LimitOutcome::Deferred {
                reason,
                unmeasured,
                refiled_to,
            } => {
                assert!(reason.len() > 40, "{} needs a real reason", row.claim_id());
                assert_eq!(*unmeasured, 0);
                let expected = REFILED
                    .iter()
                    .find(|(claim, _)| *claim == row.claim_id())
                    .map(|(_, target)| *target)
                    .expect("every claim is in the re-filing table");
                assert_eq!(refiled_to.as_deref(), Some(expected));
            }
            other => panic!("{} must stay deferred, got {other:?}", row.claim_id()),
        }
    }
}
