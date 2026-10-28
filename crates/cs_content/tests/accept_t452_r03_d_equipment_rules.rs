//! Task #2167 (T452-R.03-D): the equipment-rule census over the catalogued
//! `weapons.zrd` records.
//!
//! Task test prefix: `accept_t452_r03_d_`. Finding:
//! `docs/findings/2026-10-28-t452-r03-d-equipment-rule-census.md`.

use std::collections::{BTreeMap, BTreeSet};

use cs_content::ordnance::{
    OriginalWeaponRecord, classify_original_weapons, original_equipment_rule_census,
    read_original_equipment_rule_census,
};

fn record(name: &str, keys: &[&str]) -> OriginalWeaponRecord {
    OriginalWeaponRecord {
        record: name.to_owned(),
        keys: keys
            .iter()
            .map(|key| (*key).to_owned())
            .collect::<BTreeSet<_>>(),
        first_values: BTreeMap::new(),
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    }
}

#[test]
fn accept_t452_r03_d_a_requirement_key_is_reported_not_assumed_away() {
    let census = classify_original_weapons(vec![
        record("wep_04", &["DAMAGE", "REQUIRES_NITRO"]),
        record("wep_05", &["DAMAGE", "CLUSTER_SIZE"]),
        record("wep_99", &["FORBIDS_X"]),
    ]);
    let names = [
        "wep_04".to_owned(),
        "wep_05".to_owned(),
        "wep_77".to_owned(),
    ];
    let result = original_equipment_rule_census(&census, &names);
    assert_eq!(result.records, ["wep_04", "wep_05"]);
    assert!(result.declares_rules());
    assert_eq!(
        result.rule_keys,
        [("wep_04".to_owned(), "REQUIRES_NITRO".to_owned())]
    );
    assert!(!result.keys.contains("FORBIDS_X"));
}

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    ))
}

/// Every top-level key the sixteen catalogued records declare.
const MEASURED_KEYS: [&str; 41] = [
    "ACCELERATION",
    "AMMO_LIMIT",
    "ARMOR_DAMAGE",
    "BEEPER",
    "BEEPER_SEEKER",
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

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r03_d_retail_catalogued_records_declare_no_equipment_rule() {
    let result = read_original_equipment_rule_census(&game_dir()).expect("retail reads");
    assert_eq!(result.records.len(), 16, "{:?}", result.records);
    assert!(
        !result.declares_rules(),
        "a requirement/permission key exists: {:?}",
        result.rule_keys
    );
    let keys: Vec<&str> = result.keys.iter().map(String::as_str).collect();
    assert_eq!(keys, MEASURED_KEYS);
}
