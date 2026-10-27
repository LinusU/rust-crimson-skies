//! Task T452.01 against the owner's installation: enumerate every `wep_`
//! record of `weapons.zrd` — guns, rockets, torpedo, flare and effect
//! records alike — and classify each onto an `OrdnanceFamily` by stated key
//! rules, recording a limitation for any record no rule reaches.
//!
//! Task test prefix: `accept_t452_01_`. Findings:
//! `docs/findings/2026-10-25-t452-01-weapon-census.md`.
//!
//! The synthetic half builds `OriginalWeaponRecord`s by hand and pins the
//! stated rules plus the declared/runtime family correspondence — it runs
//! in CI. The retail half re-reads the owner's installation through the
//! production reader [`read_original_weapon_census`] and fails loudly
//! without `CS_GAME_DIR`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cs_content::ordnance::{
    DeclaredOrdnanceFamily, ORIGINAL_ROCKET_ORDNANCE_TYPES, OriginalWeaponClass,
    OriginalWeaponFinding, OriginalWeaponRecord, classify_original_weapons,
    read_original_rocket_blasts, read_original_weapon_census, read_original_weapon_records,
};

/// A hand-built record: only its top-level keys (and the first-value map
/// when a rule reads a number).
fn record(name: &str, keys: &[&str], first: &[(&str, f64)]) -> OriginalWeaponRecord {
    OriginalWeaponRecord {
        record: name.to_owned(),
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        first_values: first
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect(),
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    }
}

fn class_of(record: &OriginalWeaponRecord) -> OriginalWeaponClass {
    classify_original_weapons(vec![record.clone()])
        .rows
        .into_iter()
        .next()
        .expect("one row")
        .class
}

fn assert_family(record: &OriginalWeaponRecord, family: DeclaredOrdnanceFamily, rule: &str) {
    match class_of(record) {
        OriginalWeaponClass::Classified {
            family: assigned,
            rule: used,
        } => {
            assert_eq!(assigned, family, "{}", record.record);
            assert_eq!(used, rule, "{}", record.record);
        }
        other => panic!("{} should classify, got {other:?}", record.record),
    }
}

/// The stated rules, one shape each, plus the orderings that keep a
/// torpedo's `LOCK_ON` and a guided flare's `FLASH` from stealing their
/// records: `TORPEDO` beats `LOCK_ON`, and `LOCK_ON` beats an effect flag.
#[test]
fn accept_t452_01_the_stated_rules_classify_each_shape() {
    // wep_14 shape: TORPEDO and LOCK_ON both declared -> the torpedo rule wins.
    assert_family(
        &record("t", &["TORPEDO", "LOCK_ON", "ROCKET"], &[]),
        DeclaredOrdnanceFamily::AerialTorpedo,
        "t452_01.rule.torpedo",
    );
    // wep_15/wep_09 shape: LOCK_ON and FLASH -> guided, not effect.
    assert_family(
        &record("g", &["LOCK_ON", "FLASH", "ROCKET"], &[]),
        DeclaredOrdnanceFamily::GuidedRocket,
        "t452_01.rule.guided",
    );
    assert_family(
        &record("b", &["LOCK_ON", "BEEPER_SEEKER", "ROCKET"], &[]),
        DeclaredOrdnanceFamily::GuidedRocket,
        "t452_01.rule.guided",
    );
    // wep_12/wep_13 shape: an effect flag, unguided -> area denial engine.
    for flag in [
        "TANGLER",
        "SMOKE_SCREEN",
        "FLASH",
        "SONIC",
        "BEEPER",
        "BEEPER_SEEKER",
    ] {
        assert_family(
            &record("e", &["ROCKET", flag], &[]),
            DeclaredOrdnanceFamily::AreaDenialEngine,
            "t452_01.rule.effect",
        );
    }
    // wep_26 shape: a blast radius or proximity trigger, unguided and no
    // effect flag -> proximity flak.
    assert_family(
        &record("p", &["ROCKET", "IMPACT_PROXIMITY"], &[]),
        DeclaredOrdnanceFamily::ProximityFlak,
        "t452_01.rule.airburst",
    );
    assert_family(
        &record("p", &["ROCKET", "DETONATION_DISTANCE"], &[]),
        DeclaredOrdnanceFamily::ProximityFlak,
        "t452_01.rule.airburst",
    );
    // A bare ROCKET -> direct explosive.
    assert_family(
        &record("d", &["ROCKET", "ARMOR_DAMAGE"], &[]),
        DeclaredOrdnanceFamily::DirectExplosive,
        "t452_01.rule.direct",
    );
}

/// A record no stated rule reaches is a limitation naming it: a gun-shape
/// record names its gun shape, anything else names the absent ordnance
/// keys — and `family()` is `None` either way.
#[test]
fn accept_t452_01_an_unclassified_record_is_a_named_limitation() {
    let gun = class_of(&record("wep_00", &["CANNON", "FIRE_RATE", "IMPACT"], &[]));
    match &gun {
        OriginalWeaponClass::Limitation { claim, reason } => {
            assert_eq!(claim, "t452_01.limit.wep_00");
            assert!(reason.contains("CANNON"), "{reason}");
        }
        other => panic!("a CANNON record must not classify: {other:?}"),
    }
    assert_eq!(gun.family(), None);

    let odd = class_of(&record("wep_28", &["GRAVITY", "RANGE", "FIRE_RATE"], &[]));
    match &odd {
        OriginalWeaponClass::Limitation { claim, reason } => {
            assert_eq!(claim, "t452_01.limit.wep_28");
            assert!(reason.contains("not an ordnance record shape"), "{reason}");
        }
        other => panic!("a non-ordnance record must not classify: {other:?}"),
    }

    let census = classify_original_weapons(vec![
        record("wep_00", &["CANNON"], &[]),
        record("wep_28", &["RANGE"], &[]),
        record("wep_04", &["ROCKET", "LOCK_ON"], &[]),
    ]);
    let limitations: Vec<&str> = census
        .limitations()
        .iter()
        .map(|row| row.record.record.as_str())
        .collect();
    assert_eq!(limitations, ["wep_00", "wep_28"]);
    let guided = census.records_of(DeclaredOrdnanceFamily::GuidedRocket);
    assert_eq!(guided.len(), 1);
    assert_eq!(guided[0].record, "wep_04");
}

/// The census's one aggregate check: the `ROCKET`-flagged record count
/// against the measured eleven selectable types. Fifteen records (what the
/// member actually carries) is a recorded mismatch; a hand-built set of
/// eleven is not.
#[test]
fn accept_t452_01_the_rocket_count_mismatch_is_a_finding() {
    let fifteen: Vec<OriginalWeaponRecord> = (0..15)
        .map(|index| record(&format!("wep_{index:02}"), &["ROCKET", "LOCK_ON"], &[]))
        .collect();
    let census = classify_original_weapons(fifteen);
    assert_eq!(census.rocket_records, 15);
    assert_eq!(
        census.findings,
        [OriginalWeaponFinding::RocketTypeCountMismatch {
            rocket_records: 15,
            screen_types: ORIGINAL_ROCKET_ORDNANCE_TYPES,
        }]
    );
    assert!(census.findings[0].to_string().contains("15"));

    let eleven: Vec<OriginalWeaponRecord> = (0..11)
        .map(|index| record(&format!("wep_{index:02}"), &["ROCKET"], &[]))
        .collect();
    let matched = classify_original_weapons(eleven);
    assert_eq!(matched.rocket_records, 11);
    assert!(matched.findings.is_empty());
}

/// `cs_content` may not link `cs_sim`, so the classification lands on
/// `DeclaredOrdnanceFamily`; this test-only edge pins it variant-for-variant
/// against `cs_sim::weapons::ordnance::OrdnanceFamily` — the runtime enum
/// the task names — so the two cannot drift.
#[test]
fn accept_t452_01_the_declared_and_runtime_families_carry_the_same_variants() {
    use cs_sim::weapons::ordnance::OrdnanceFamily as RuntimeFamily;
    assert_eq!(DeclaredOrdnanceFamily::ALL.len(), RuntimeFamily::ALL.len());
    let declared: Vec<&str> = DeclaredOrdnanceFamily::ALL
        .iter()
        .map(|family| family.label())
        .collect();
    let runtime: Vec<&str> = RuntimeFamily::ALL
        .iter()
        .map(|family| family.label())
        .collect();
    assert_eq!(declared, runtime);
    // And the labels are the families the sheet names.
    assert_eq!(
        declared,
        [
            "direct_explosive",
            "proximity_flak",
            "guided_rocket",
            "area_denial_engine",
            "aerial_torpedo",
            "nitro_booster",
        ]
    );
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    );
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// Every `wep_` record name, in member order (measured).
const MEASURED_RECORD_NAMES: [&str; 48] = [
    "wep_00", "wep_01", "wep_02", "wep_03", "wep_04", "wep_05", "wep_06", "wep_07", "wep_08",
    "wep_09", "wep_10", "wep_11", "wep_12", "wep_13", "wep_14", "wep_15", "wep_23", "wep_24",
    "wep_26", "wep_25", "wep_27", "wep_28", "wep_29", "wep_30", "wep_31", "wep_32", "wep_33",
    "wep_40", "wep_41", "wep_42", "wep_43", "wep_50", "wep_51", "wep_52", "wep_53", "wep_60",
    "wep_61", "wep_62", "wep_63", "wep_70", "wep_71", "wep_72", "wep_73", "wep_130", "wep_140",
    "wep_150", "wep_160", "wep_170",
];

/// The top-level keys every retail `wep_` record declares, unioned
/// (measured): a key the census has not seen is an unmodelled family,
/// arming or channel declaration, so the set is pinned exactly.
const MEASURED_KEY_UNION: [&str; 46] = [
    "ACCELERATION",
    "AMMO_LIMIT",
    "ARMOR_DAMAGE",
    "BEEPER",
    "BEEPER_SEEKER",
    "CALIBER",
    "CANNON",
    "CANNON_SPREAD",
    "CLUSTER_SIZE",
    "CRATER",
    "DAMAGE",
    "DAMAGES_ZEPPELIN",
    "DESC",
    "DESTROY_ANIMATION",
    "DETONATION_DISTANCE",
    "DETONATION_DOT_PRODUCT",
    "DETONATION_TIME",
    "FIRE",
    "FIRE_RATE",
    "FIRING_HEAT",
    "FLASH",
    "FLYOUT",
    "FLYOUT_HEALTH",
    "GRAVITY",
    "HEALTH_DAMAGE",
    "HIGH_EXPLOSIVE",
    "IMPACT",
    "IMPACT_PROXIMITY",
    "LOCK_ON",
    "LOCK_ON_LEAD",
    "LOOPED_SOUND_NAME",
    "NAME",
    "PRIORITY",
    "PROJECTILE_BBOX",
    "RANGE",
    "RANGE_MINIMUM",
    "REAR",
    "ROCKET",
    "SHAKES_CAMERA",
    "SMOKE_SCREEN",
    "SONIC",
    "TANGLER",
    "TARGETABLE",
    "TORPEDO",
    "TURN_RATE",
    "VELOCITY",
];

/// The records that classify to `guided_rocket` (measured). `wep_25` is
/// the record the rocket-only reader structurally cannot see: it declares
/// `LOCK_ON` and every projectile key but **no `ROCKET` flag** — the
/// glidebomb — so the twelve-record guided census of `accept_t454_`
/// (ROCKET records only) never reached it.
const MEASURED_GUIDED: [&str; 12] = [
    "wep_04", "wep_05", "wep_06", "wep_07", "wep_08", "wep_09", "wep_10", "wep_11", "wep_15",
    "wep_24", "wep_25", "wep_27",
];

/// The member carries every `wep_` record: 48, in member order, each with
/// a nonempty key set. A new record or a new top-level key is a census
/// event the suite must see rather than absorb silently.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_01_retail_every_wep_record_is_enumerated() {
    let records = read_original_weapon_records(&game_dir()).expect("the retail records read");
    let names: Vec<&str> = records
        .iter()
        .map(|record| record.record.as_str())
        .collect();
    assert_eq!(names, MEASURED_RECORD_NAMES);
    for record in &records {
        assert!(
            !record.keys.is_empty(),
            "{} declares no top-level key",
            record.record
        );
    }
    let union: BTreeSet<&str> = records
        .iter()
        .flat_map(|record| record.keys.iter().map(String::as_str))
        .collect();
    let expected: BTreeSet<&str> = MEASURED_KEY_UNION.into_iter().collect();
    assert_eq!(
        union, expected,
        "a new top-level key is an unmodelled family, arming or channel declaration"
    );
}

/// The classification of the whole member: thirteen `LOCK_ON` records
/// (twelve `ROCKET`-flagged plus the glidebomb `wep_25`), two effect
/// records, one torpedo, one airburst — and thirty-two limitations, the
/// gun and non-ordnance records the families do not cover. No record
/// reaches `direct_explosive` or `nitro_booster`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_01_retail_the_census_classifies_every_record() {
    let census = read_original_weapon_census(&game_dir()).expect("the census reads");
    assert_eq!(census.rows.len(), 48, "one row per record");
    assert_eq!(
        census
            .records_of(DeclaredOrdnanceFamily::GuidedRocket)
            .iter()
            .map(|record| record.record.as_str())
            .collect::<Vec<_>>(),
        MEASURED_GUIDED
    );
    assert_eq!(
        census
            .records_of(DeclaredOrdnanceFamily::AreaDenialEngine)
            .iter()
            .map(|record| record.record.as_str())
            .collect::<Vec<_>>(),
        ["wep_12", "wep_13"]
    );
    assert_eq!(
        census
            .records_of(DeclaredOrdnanceFamily::AerialTorpedo)
            .iter()
            .map(|record| record.record.as_str())
            .collect::<Vec<_>>(),
        ["wep_14"]
    );
    assert_eq!(
        census
            .records_of(DeclaredOrdnanceFamily::ProximityFlak)
            .iter()
            .map(|record| record.record.as_str())
            .collect::<Vec<_>>(),
        ["wep_26"]
    );
    assert!(
        census
            .records_of(DeclaredOrdnanceFamily::DirectExplosive)
            .is_empty()
    );
    assert!(
        census
            .records_of(DeclaredOrdnanceFamily::NitroBooster)
            .is_empty()
    );

    let limitations = census.limitations();
    assert_eq!(limitations.len(), 32, "the gun and non-ordnance records");
    // Thirty-one CANNON machine/turret guns plus the cannonball wep_28 —
    // the one non-gun record that reaches no rule.
    let cannon: usize = limitations
        .iter()
        .filter(|row| row.record.declares("CANNON"))
        .count();
    assert_eq!(cannon, 31);
    for row in &limitations {
        assert!(
            matches!(&row.class, OriginalWeaponClass::Limitation { claim, .. }
                if claim == &format!("t452_01.limit.{}", row.record.record)),
            "{} must carry its own limitation claim",
            row.record.record
        );
    }
}

/// The count comparison the task asks for: fifteen `ROCKET`-flagged
/// records against eleven measured selectable types — a recorded
/// `rocket_type_count_mismatch`, not an error. The census's `ROCKET` count
/// is also re-derived through `read_original_rocket_blasts`: the same
/// fifteen records, the same names.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_01_retail_rocket_records_outnumber_the_selectable_types() {
    let census = read_original_weapon_census(&game_dir()).expect("the census reads");
    assert_eq!(census.rocket_records, 15);
    assert_eq!(
        census.findings,
        [OriginalWeaponFinding::RocketTypeCountMismatch {
            rocket_records: 15,
            screen_types: ORIGINAL_ROCKET_ORDNANCE_TYPES,
        }]
    );

    // Cross-check against the rocket-only reader: identical record names.
    let blasts = read_original_rocket_blasts(&game_dir()).expect("the rocket records read");
    let rocket_flagged: BTreeSet<&str> = census
        .records
        .iter()
        .filter(|record| record.declares("ROCKET"))
        .map(|record| record.record.as_str())
        .collect();
    let blast_names: BTreeSet<&str> = blasts.iter().map(|blast| blast.record.as_str()).collect();
    assert_eq!(rocket_flagged, blast_names);
}

/// The family-deciding first values, pinned on the records that carry
/// them: `LOCK_ON`, `DETONATION_*`, `RANGE*` and the torpedo's
/// `RANGE_MINIMUM` pair whose second value suppresses the blast inside the
/// arming window.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_01_retail_the_family_deciding_first_values_are_read() {
    let census = read_original_weapon_census(&game_dir()).expect("the census reads");
    let get = |name: &str| census.record(name).unwrap_or_else(|| panic!("no {name}"));

    let incendiary = get("wep_04");
    assert_eq!(incendiary.first("LOCK_ON"), Some(3.0));
    assert_eq!(incendiary.first("DETONATION_DISTANCE"), Some(30.0));
    assert_eq!(incendiary.first("IMPACT_PROXIMITY"), Some(25.0));
    assert_eq!(incendiary.first("RANGE"), Some(900.0));
    assert_eq!(incendiary.first("CLUSTER_SIZE"), Some(1.0));

    let torpedo = get("wep_14");
    assert_eq!(torpedo.first("RANGE_MINIMUM"), Some(300.0));
    assert_eq!(torpedo.second_values.get("RANGE_MINIMUM"), Some(&0.0));
    assert_eq!(torpedo.first("DETONATION_DISTANCE"), Some(1.0));
    assert_eq!(torpedo.first("VELOCITY"), Some(60.0));

    let flare = get("wep_15");
    assert_eq!(flare.first("DETONATION_TIME"), Some(2.0));
    assert_eq!(flare.first("RANGE"), None);

    let fake = get("wep_26");
    assert_eq!(fake.first("IMPACT_PROXIMITY"), Some(100.0));
    assert_eq!(fake.first("LOCK_ON"), None);

    // The effect records' declared second values: the tangle time and the
    // smoke lifetime.
    let choker = get("wep_12");
    assert_eq!(choker.second_values.get("TANGLER"), Some(&2.0));
    let smoke = get("wep_13");
    assert_eq!(smoke.second_values.get("SMOKE_SCREEN"), Some(&8.0));

    // No record declares the range-end or swarm keys the detonation rule
    // would read differently on.
    for name in MEASURED_RECORD_NAMES {
        let record = get(name);
        assert!(
            !record.declares("EXPIRES") && !record.declares("DETONATE_AT_RANGE"),
            "{name} would change the measured range-end rule"
        );
    }
}
