//! Task #2130 (T452.04): the per-component launch table — the hardpoint
//! layout the two ordnance screens declare, and per `wep_` record the launch
//! speed, release rate, lifetime, area, damage and status declarations, with
//! every value no file states recorded as a named unknown.
//!
//! Task test prefix: `accept_t452_04_`. Findings:
//! `docs/findings/2026-10-25-t452-04-component-hardpoints-launch-speeds-and-lifetimes.md`.
//!
//! The synthetic half builds screen text and `OriginalWeaponRecord`s by hand
//! and drives the production readers and table builder — it runs in CI. The
//! retail half re-reads the owner's installation through
//! [`read_original_component_table`] and fails loudly without `CS_GAME_DIR`.

use std::collections::BTreeMap;

use cs_content::ordnance::{
    DeclaredOrdnanceFamily, ORIGINAL_AMMUNITION_SLOT_ARRAY, ORIGINAL_HARDPOINT_POINT_ARRAY,
    ORIGINAL_HARDPOINTS_SCREEN, ORIGINAL_ORDNANCE_LAYOUT_SCREEN, ORIGINAL_ROCKET_SLOT_ARRAY,
    OriginalClusterShape, OriginalComponentTable, OriginalLifetime, OriginalMountDeclaration,
    OriginalMountLayout, OriginalScreenError, OriginalSlotArray, OriginalWeaponRecord,
    T452_04_AREA_LIFETIME, T452_04_AREA_RADIUS, T452_04_CLUSTER, T452_04_MOUNT, T452_04_NO_EXPIRES,
    T452_04_PER_AIRFRAME, T452_04_RELEASE_DELAY, T452_04_STATUS_DURATION, T452_04_STATUS_KIND,
    T452_04_VELOCITY_UNIT, classify_original_weapons, original_component_table,
    read_original_component_table,
};

// -------------------------------------------------------------- fixtures ---

/// A hardpoint screen in the shape the original's own declares — one array,
/// the loop that fills it, no `deactivate` — written with **invented**
/// identifiers and spacing: the only original facts it carries are the array
/// name and its count, which the committed findings already state. This is a
/// synthetic fixture, not the original's text (AGENTS rule 3).
const HARDPOINTS: &str = "\
main
{
gui_create
{
object DT[2]
for (Q = 0; Q < 2; Q++)
{
DT[Q] = @ctl@ZQ
}
}
}
";

/// An ordnance screen in the shape the original's own declares — four
/// ammunition slots, eight rocket slots, and a `deactivate` for an empty slot
/// — again with invented identifiers; the array names and counts are the
/// measured facts, the statements around them are not the original's.
const ORDNANCE_LAYOUT: &str = "\
main
{
gui_create
{
object QKA[4]
for (K = 0; K < 4; K++)
{
QKA[K] = @ctl@ZQ
}
object RKA[8]
for (K = 0; K < 8; K++)
{
RKA[K] = @ctl@ZQ
}
}
gui_init
{
for (K = 0; K < 8; K++)
{
if (-1 == SLOT)
{
deactivate(RKA[K])
}
}
for (K = 0; K < 4; K++)
{
if (-1 == SLOT)
{
deactivate(QKA[K])
}
}
}
}
";

fn layout() -> OriginalMountLayout {
    OriginalMountLayout::from_screens(HARDPOINTS, ORDNANCE_LAYOUT)
        .expect("the fixture layout reads")
}

/// A hand-built record: the top-level keys, plus whichever value maps the
/// fixture needs.
fn record(
    name: &str,
    keys: &[&str],
    first: &[(&str, f64)],
    second: &[(&str, f64)],
    texts: &[(&str, &str)],
) -> OriginalWeaponRecord {
    OriginalWeaponRecord {
        record: name.to_owned(),
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        first_values: first
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect::<BTreeMap<_, _>>(),
        second_values: second
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect(),
        first_texts: texts
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

fn table_with(
    records: Vec<OriginalWeaponRecord>,
    layout: &OriginalMountLayout,
) -> OriginalComponentTable {
    original_component_table(&classify_original_weapons(records), layout)
}

/// A `wep_04`-shaped incendiary rocket: every value the row can carry.
fn incendiary() -> OriginalWeaponRecord {
    record(
        "wep_04",
        &[
            "ACCELERATION",
            "AMMO_LIMIT",
            "ARMOR_DAMAGE",
            "CLUSTER_SIZE",
            "DETONATION_DISTANCE",
            "FIRE_RATE",
            "HEALTH_DAMAGE",
            "IMPACT_PROXIMITY",
            "LOCK_ON",
            "RANGE",
            "ROCKET",
            "VELOCITY",
        ],
        &[
            ("VELOCITY", 450.0),
            ("FIRE_RATE", 1.0),
            ("CLUSTER_SIZE", 1.0),
            ("AMMO_LIMIT", 100.0),
            ("IMPACT_PROXIMITY", 25.0),
            ("DETONATION_DISTANCE", 30.0),
            ("ARMOR_DAMAGE", 25.0),
            ("HEALTH_DAMAGE", 25.0),
            ("RANGE", 900.0),
            ("LOCK_ON", 3.0),
        ],
        &[],
        &[],
    )
}

// -------------------------------------------------------------- synthetic ---

/// A screen states its slot count twice — in the array declaration and in the
/// loop that fills it — and the reader takes both, so a disagreement is a
/// finding rather than an average. `deactivate` is the screen's own handling of
/// an empty slot, matched per array, and the hardpoint screen does not have it.
#[test]
fn accept_t452_04_a_screen_states_its_slot_array_and_fill_loop() {
    let rockets =
        OriginalSlotArray::read("m.SCRIPT", ORDNANCE_LAYOUT, "RKA").expect("RKA is declared");
    assert_eq!(rockets.array, "RKA");
    assert_eq!(rockets.member, "m.SCRIPT");
    assert_eq!(rockets.slots, 8, "`object RKA[8]`");
    assert_eq!(rockets.loop_bound, 8, "`for (K = 0; K < 8; K++)`");
    assert!(rockets.is_consistent());
    assert!(
        rockets.deactivates_empty_slots,
        "the screen deactivates RKA"
    );

    let ammunition = OriginalSlotArray::read("m.SCRIPT", ORDNANCE_LAYOUT, "QKA").expect("QKA");
    assert_eq!(ammunition.slots, 4);
    assert_eq!(ammunition.loop_bound, 4);
    assert!(
        ammunition.deactivates_empty_slots,
        "the screen deactivates QKA too"
    );

    let points = OriginalSlotArray::read("h.SCRIPT", HARDPOINTS, "DT").expect("DT");
    assert_eq!(points.slots, 2);
    assert_eq!(points.loop_bound, 2);
    assert!(
        !points.deactivates_empty_slots,
        "HARDPOINTS.SCRIPT has no `deactivate`, so both points always show"
    );

    // The empty-slot flag belongs to the array, not to the screen: taking the
    // call off QKA leaves RKA's handling stated.
    let mixed = ORDNANCE_LAYOUT.replace("deactivate(QKA[K])", "SLOT = 0");
    assert_ne!(mixed, ORDNANCE_LAYOUT, "the fixture must have changed");
    let ammunition = OriginalSlotArray::read("m.SCRIPT", &mixed, "QKA").expect("QKA");
    let rockets = OriginalSlotArray::read("m.SCRIPT", &mixed, "RKA").expect("RKA");
    assert!(
        !ammunition.deactivates_empty_slots,
        "QKA's empty slots are no longer hidden"
    );
    assert!(
        rockets.deactivates_empty_slots,
        "RKA's are still hidden: the flag is per array"
    );
    let spaced = ORDNANCE_LAYOUT.replace("deactivate(QKA[K])", "deactivate ( QKA[K] )");
    assert_ne!(spaced, ORDNANCE_LAYOUT, "the fixture must have changed");
    assert!(
        OriginalSlotArray::read("m.SCRIPT", &spaced, "QKA")
            .expect("QKA")
            .deactivates_empty_slots,
        "the call's spacing is the original's, not the reader's"
    );

    assert!(OriginalSlotArray::read("m.SCRIPT", "no arrays here", "RKA").is_none());
    assert!(
        OriginalSlotArray::read("m.SCRIPT", "object RKA[8]\nno loop at all", "RKA").is_none(),
        "a declared array with no fill loop states its count once, so nothing checks it"
    );

    let disagreeing = ORDNANCE_LAYOUT.replace(
        "object RKA[8]\nfor (K = 0; K < 8; K++)",
        "object RKA[8]\nfor (K = 0; K < 4; K++)",
    );
    assert_ne!(
        disagreeing, ORDNANCE_LAYOUT,
        "the fixture must have changed"
    );
    let refused = OriginalMountLayout::from_screens(HARDPOINTS, &disagreeing)
        .expect("the layout still reads");
    assert!(
        !refused.is_consistent(),
        "8 declared, 4 filled is a finding"
    );
    assert_eq!(refused.rocket_slots.slots, 8);
    assert_eq!(refused.rocket_slots.loop_bound, 4);
    assert_eq!(refused.findings.len(), 1, "{:?}", refused.findings);
    assert!(
        refused.findings[0].to_string().contains("RKA[8]"),
        "{}",
        refused.findings[0]
    );
}

/// The layout is read from the two screens under their own member names, and a
/// screen that declares no array is refused rather than filled in.
#[test]
fn accept_t452_04_the_layout_is_read_from_both_screens() {
    let layout = layout();
    assert!(layout.is_consistent(), "{:?}", layout.findings);
    assert_eq!(layout.rocket_slots.member, ORIGINAL_ORDNANCE_LAYOUT_SCREEN);
    assert_eq!(layout.rocket_slots.array, ORIGINAL_ROCKET_SLOT_ARRAY);
    assert_eq!(
        layout.ammunition_slots.member,
        ORIGINAL_ORDNANCE_LAYOUT_SCREEN
    );
    assert_eq!(
        layout.ammunition_slots.array,
        ORIGINAL_AMMUNITION_SLOT_ARRAY
    );
    assert_eq!(layout.hardpoint_points.member, ORIGINAL_HARDPOINTS_SCREEN);
    assert_eq!(
        layout.hardpoint_points.array,
        ORIGINAL_HARDPOINT_POINT_ARRAY
    );
    assert_eq!(layout.rocket_slots.slots, 8);
    assert_eq!(layout.ammunition_slots.slots, 4);
    assert_eq!(layout.hardpoint_points.slots, 2);

    let missing = OriginalMountLayout::from_screens("nothing here", ORDNANCE_LAYOUT);
    match missing {
        Err(OriginalScreenError::SlotArrayMissing { member, array }) => {
            assert_eq!(member, ORIGINAL_HARDPOINTS_SCREEN);
            assert_eq!(array, ORIGINAL_HARDPOINT_POINT_ARRAY);
        }
        other => panic!("a screen with no array must be refused: {other:?}"),
    }
}

/// One rocket record's whole row: launch speed, release rate, cluster, area,
/// damage and mount, each read from its own declared key.
#[test]
fn accept_t452_04_a_rocket_record_carries_its_declared_launch_values() {
    let table = table_with(vec![incendiary()], &layout());
    let row = table.row("wep_04").expect("one row");
    assert_eq!(row.family, DeclaredOrdnanceFamily::GuidedRocket);
    assert_eq!(row.velocity, Some(450.0), "VELOCITY");
    assert_eq!(row.fire_rate, Some(1.0), "FIRE_RATE");
    assert_eq!(row.cluster, OriginalClusterShape::Volley { munitions: 1 });
    assert_eq!(row.area.radius_m, Some(25.0), "IMPACT_PROXIMITY");
    assert_eq!(row.area.lifetime_s, None, "no area lifetime is declared");
    assert_eq!(row.damage.armor, Some(25.0));
    assert_eq!(row.damage.health, Some(25.0));
    assert_eq!(row.damage.single, None);
    assert_eq!(row.damage.channels(), 2);
    assert_eq!(
        row.mount,
        OriginalMountDeclaration::RocketSlot { slots: 8 },
        "a ROCKET record is a rocket-slot component"
    );
    assert_eq!(row.mount.slots(), Some(8));
    assert!(row.statuses.is_empty());

    // The values no file states are named on the row, not defaulted.
    for claim in [
        T452_04_VELOCITY_UNIT,
        T452_04_RELEASE_DELAY,
        T452_04_NO_EXPIRES,
        T452_04_AREA_LIFETIME,
    ] {
        assert!(row.names_claim(claim), "{claim} must be recorded");
    }
    assert!(
        !row.names_claim(T452_04_CLUSTER),
        "a single sub-munition raises no swarm question"
    );
    assert!(table.claims().contains(T452_04_PER_AIRFRAME));
}

/// `CLUSTER_SIZE` is a volley only when it is not the ammunition count: on the
/// member's gun records the two keys are equal, and a 2800-round count must
/// never become a 2800-sub-munition launch.
#[test]
fn accept_t452_04_a_cluster_size_equal_to_the_ammo_limit_is_not_a_volley() {
    let gun_shaped = record(
        "wep_g",
        &["ROCKET", "CLUSTER_SIZE", "AMMO_LIMIT"],
        &[("CLUSTER_SIZE", 2800.0), ("AMMO_LIMIT", 2800.0)],
        &[],
        &[],
    );
    let table = table_with(vec![gun_shaped], &layout());
    assert_eq!(
        table.row("wep_g").expect("one row").cluster,
        OriginalClusterShape::AmmunitionCount { rounds: 2800 }
    );

    let swarm = record(
        "wep_s",
        &["ROCKET", "LOCK_ON", "CLUSTER_SIZE", "AMMO_LIMIT"],
        &[("CLUSTER_SIZE", 4.0), ("AMMO_LIMIT", 100.0)],
        &[],
        &[],
    );
    let table = table_with(vec![swarm], &layout());
    let row = table.row("wep_s").expect("one row");
    assert_eq!(row.cluster, OriginalClusterShape::Volley { munitions: 4 });
    assert!(
        row.names_claim(T452_04_CLUSTER),
        "a cluster raises the swarm unknown"
    );

    let undeclared = record("wep_u", &["ROCKET"], &[], &[], &[]);
    let table = table_with(vec![undeclared], &layout());
    assert_eq!(
        table.row("wep_u").expect("one row").cluster,
        OriginalClusterShape::Undeclared
    );
}

/// A status flag carries the duration the record states and nothing else: the
/// key's unit word and second value are read, and a flag with no duration is an
/// explicit unknown.
#[test]
fn accept_t452_04_a_status_flag_records_the_duration_the_record_states() {
    let choker = record(
        "wep_12",
        &["ROCKET", "TANGLER"],
        &[],
        &[("TANGLER", 2.0)],
        &[("TANGLER", "TIME")],
    );
    let table = table_with(vec![choker], &layout());
    let row = table.row("wep_12").expect("one row");
    assert_eq!(
        row.family,
        DeclaredOrdnanceFamily::AreaDenialEngine,
        "an effect flag with no LOCK_ON is the area-denial family"
    );
    assert!(row.declares_status());
    assert_eq!(row.statuses.len(), 1);
    assert_eq!(row.statuses[0].key, "TANGLER");
    assert_eq!(row.statuses[0].duration_s, Some(2.0));
    assert_eq!(row.statuses[0].duration_unit.as_deref(), Some("TIME"));
    assert!(
        row.names_claim(T452_04_STATUS_KIND),
        "the kind is not stated"
    );
    assert!(
        !row.names_claim(T452_04_STATUS_DURATION),
        "this flag states its duration"
    );
    // Its whole effect is an area and the record states no area at all.
    assert!(
        row.names_claim(T452_04_AREA_RADIUS),
        "an area-denial record declares no radius"
    );
    assert!(
        row.names_claim(T452_04_AREA_LIFETIME),
        "its effect duration is not stated to be the area's lifetime"
    );

    let undated = record("wep_08", &["ROCKET", "SONIC"], &[], &[], &[]);
    let table = table_with(vec![undated], &layout());
    let row = table.row("wep_08").expect("one row");
    assert_eq!(row.statuses[0].key, "SONIC");
    assert_eq!(row.statuses[0].duration_s, None);
    assert!(row.statuses[0].duration_unit.is_none());
    assert!(row.names_claim(T452_04_STATUS_DURATION));
    assert!(row.names_claim(T452_04_STATUS_KIND));

    // A guided effect record does state its blast radius, so only the area's
    // lifetime is unknown there.
    let flash = record(
        "wep_09",
        &["ROCKET", "LOCK_ON", "FLASH", "IMPACT_PROXIMITY"],
        &[("IMPACT_PROXIMITY", 450.0)],
        &[],
        &[],
    );
    let table = table_with(vec![flash], &layout());
    let row = table.row("wep_09").expect("one row");
    assert_eq!(row.family, DeclaredOrdnanceFamily::GuidedRocket);
    assert_eq!(row.area.radius_m, Some(450.0));
    assert!(
        !row.names_claim(T452_04_AREA_RADIUS),
        "this record states its radius"
    );
    assert!(
        row.names_claim(T452_04_AREA_LIFETIME),
        "a declared radius with no stated lifetime is an unknown"
    );
}

/// A lifetime is only what a record states: `DETONATION_TIME` is one, an absent
/// `EXPIRES` key is not a zero-second life.
#[test]
fn accept_t452_04_a_lifetime_is_only_what_a_record_states() {
    let flare = record(
        "wep_15",
        &["ROCKET", "LOCK_ON", "DETONATION_TIME", "FLASH"],
        &[("DETONATION_TIME", 2.0)],
        &[],
        &[],
    );
    let table = table_with(vec![flare], &layout());
    let row = table.row("wep_15").expect("one row");
    assert_eq!(
        row.lifetime,
        OriginalLifetime::Declared {
            seconds: 2.0,
            key: "DETONATION_TIME"
        }
    );
    assert!(!row.names_claim(T452_04_NO_EXPIRES));

    let table = table_with(vec![incendiary()], &layout());
    let row = table.row("wep_04").expect("one row");
    assert_eq!(row.lifetime, OriginalLifetime::Undeclared);
    assert!(row.names_claim(T452_04_NO_EXPIRES));
}

/// An ordnance-shaped record with no `ROCKET` flag states no slot; the same
/// record with the flag is a rocket-slot component. Nothing is assumed about the
/// glidebomb shape.
#[test]
fn accept_t452_04_a_record_without_a_rocket_flag_leaves_the_mount_unknown() {
    let glidebomb = record(
        "wep_25",
        &["LOCK_ON", "CLUSTER_SIZE", "VELOCITY"],
        &[("VELOCITY", 450.0), ("CLUSTER_SIZE", 1.0)],
        &[],
        &[],
    );
    let table = table_with(vec![glidebomb.clone()], &layout());
    let row = table.row("wep_25").expect("one row");
    assert_eq!(row.family, DeclaredOrdnanceFamily::GuidedRocket);
    assert_eq!(
        row.mount,
        OriginalMountDeclaration::Unknown {
            claim: T452_04_MOUNT.to_owned(),
            reason:
                "the record reaches an ordnance family but declares no ROCKET flag, so no screen \
                 states which slot holds it"
                    .to_owned(),
        }
    );
    assert_eq!(row.mount.slots(), None);
    assert!(row.names_claim(T452_04_MOUNT));

    let mut flagged = glidebomb;
    flagged.record = "wep_27".to_owned();
    flagged.keys.insert("ROCKET".to_owned());
    let table = table_with(vec![flagged], &layout());
    assert_eq!(
        table.row("wep_27").expect("one row").mount,
        OriginalMountDeclaration::RocketSlot { slots: 8 }
    );
}

/// The slot count in a row is the one the screens state, so a different
/// declared layout produces a different row: the eight is measured, not
/// assumed.
#[test]
fn accept_t452_04_the_mount_slot_counts_follow_the_declared_layout() {
    let narrow = ORDNANCE_LAYOUT
        .replace("object RKA[8]", "object RKA[3]")
        .replace(
            "for (K = 0; K < 8; K++)\n{\nRKA[K]",
            "for (K = 0; K < 3; K++)\n{\nRKA[K]",
        );
    assert_ne!(narrow, ORDNANCE_LAYOUT, "the fixture must have changed");
    let three = OriginalMountLayout::from_screens(HARDPOINTS, &narrow).expect("a 3-slot layout");
    assert!(three.is_consistent(), "{:?}", three.findings);
    let table = table_with(vec![incendiary()], &three);
    assert_eq!(
        table.row("wep_04").expect("one row").mount,
        OriginalMountDeclaration::RocketSlot { slots: 3 }
    );
    let eight = table_with(vec![incendiary()], &layout());
    assert_eq!(eight.row("wep_04").expect("one row").mount.slots(), Some(8));
}

/// A record no stated family rule reaches is excluded by name, not folded into
/// a row or silently dropped.
#[test]
fn accept_t452_04_a_record_with_no_family_is_excluded_by_name() {
    let gun = record(
        "wep_00",
        &["CANNON", "CLUSTER_SIZE", "AMMO_LIMIT"],
        &[("CLUSTER_SIZE", 2800.0), ("AMMO_LIMIT", 2800.0)],
        &[],
        &[],
    );
    let table = table_with(vec![gun, incendiary()], &layout());
    assert_eq!(table.excluded, ["wep_00"]);
    assert_eq!(table.rows.len(), 1);
    assert!(table.row("wep_00").is_none());
    assert_eq!(table.row("wep_04").expect("one row").record, "wep_04");
}

// ---------------------------------------------------------------- retail ---

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    );
    let dir = std::path::PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The two screens the layout is read from (measured, the real spellings).
const MEASURED_ORDNANCE_SCREEN: &str = "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT";

/// The sixteen records the T452.01 rules classify, with the family each states
/// (measured).
const MEASURED_ROWS: [(&str, &str); 16] = [
    ("wep_04", "guided_rocket"),
    ("wep_05", "guided_rocket"),
    ("wep_06", "guided_rocket"),
    ("wep_07", "guided_rocket"),
    ("wep_08", "guided_rocket"),
    ("wep_09", "guided_rocket"),
    ("wep_10", "guided_rocket"),
    ("wep_11", "guided_rocket"),
    ("wep_12", "area_denial_engine"),
    ("wep_13", "area_denial_engine"),
    ("wep_14", "aerial_torpedo"),
    ("wep_15", "guided_rocket"),
    ("wep_24", "guided_rocket"),
    ("wep_26", "proximity_flak"),
    ("wep_25", "guided_rocket"),
    ("wep_27", "guided_rocket"),
];

/// The two screens declare the layout: eight rocket slots, four
/// gun/ammunition slots, two hardpoint points, each with the array's own bound
/// and its fill loop agreeing, and only the ordnance screen deactivating an
/// empty slot.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_04_retail_the_two_screens_declare_the_layout() {
    use cs_content::ordnance::read_original_mount_layout;

    let root = game_dir();
    assert_eq!(ORIGINAL_ORDNANCE_LAYOUT_SCREEN, MEASURED_ORDNANCE_SCREEN);
    let layout = read_original_mount_layout(&root).expect("the layout reads");
    assert!(layout.is_consistent(), "{:?}", layout.findings);
    assert_eq!(layout.rocket_slots.array, "RKA");
    assert_eq!(layout.rocket_slots.slots, 8);
    assert_eq!(layout.rocket_slots.loop_bound, 8);
    assert!(layout.rocket_slots.deactivates_empty_slots);
    assert_eq!(layout.ammunition_slots.array, "QKA");
    assert_eq!(layout.ammunition_slots.slots, 4);
    assert_eq!(layout.ammunition_slots.loop_bound, 4);
    assert!(layout.ammunition_slots.deactivates_empty_slots);
    assert_eq!(layout.hardpoint_points.array, "DT");
    assert_eq!(layout.hardpoint_points.slots, 2);
    assert_eq!(layout.hardpoint_points.loop_bound, 2);
    assert!(
        !layout.hardpoint_points.deactivates_empty_slots,
        "HARDPOINTS.SCRIPT has no `deactivate`: both points always show"
    );

    // The measured layout agrees with the constants the rest of the project
    // already carries, which are the same two screens read a second way.
    use cs_content::weapons::{
        ORIGINAL_GUN_SLOTS, ORIGINAL_HARDPOINT_POINTS, ORIGINAL_ROCKET_SLOTS,
    };
    assert_eq!(layout.rocket_slots.slots, ORIGINAL_ROCKET_SLOTS);
    assert_eq!(layout.hardpoint_points.slots, ORIGINAL_HARDPOINT_POINTS);
    assert_eq!(layout.ammunition_slots.slots, ORIGINAL_GUN_SLOTS);
}

/// Every classified record becomes one row with the family its keys state, and
/// the values it declares are the ones stored — pinned on the records whose
/// declarations differ most.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_04_retail_every_component_row_is_read_from_its_record() {
    let table = read_original_component_table(&game_dir()).expect("the table reads");
    let rows: Vec<(&str, &str)> = table
        .rows
        .iter()
        .map(|row| (row.record.as_str(), row.family.label()))
        .collect();
    assert_eq!(rows, MEASURED_ROWS);
    assert_eq!(table.excluded.len(), 32, "the guns and the cannonball");

    let get = |name: &str| table.row(name).unwrap_or_else(|| panic!("no {name}"));

    let incendiary = get("wep_04");
    assert_eq!(incendiary.velocity, Some(450.0));
    assert_eq!(incendiary.fire_rate, Some(1.0));
    assert_eq!(
        incendiary.cluster,
        OriginalClusterShape::Volley { munitions: 1 }
    );
    assert_eq!(incendiary.area.radius_m, Some(25.0));
    assert_eq!(incendiary.damage.armor, Some(25.0));
    assert_eq!(incendiary.damage.health, Some(25.0));
    assert_eq!(
        incendiary.mount,
        OriginalMountDeclaration::RocketSlot { slots: 8 }
    );

    let torpedo = get("wep_14");
    assert_eq!(torpedo.family, DeclaredOrdnanceFamily::AerialTorpedo);
    assert_eq!(torpedo.velocity, Some(60.0));
    assert_eq!(torpedo.area.radius_m, Some(30.0));
    assert_eq!(torpedo.damage.armor, Some(200.0));
    assert_eq!(torpedo.damage.health, Some(200.0));
    assert_eq!(
        torpedo.cluster,
        OriginalClusterShape::Volley { munitions: 1 }
    );

    let flare = get("wep_15");
    assert_eq!(flare.velocity, Some(1.0));
    assert_eq!(
        flare.lifetime,
        OriginalLifetime::Declared {
            seconds: 2.0,
            key: "DETONATION_TIME"
        }
    );
    assert_eq!(flare.cluster, OriginalClusterShape::Volley { munitions: 2 });
    assert_eq!(flare.area.radius_m, Some(500.0));
    assert_eq!(flare.damage.single, Some(0.0), "the flash declares DAMAGE");
    assert_eq!(flare.damage.armor, None);

    let choker = get("wep_12");
    assert_eq!(choker.statuses.len(), 1);
    assert_eq!(choker.statuses[0].key, "TANGLER");
    assert_eq!(choker.statuses[0].duration_s, Some(2.0));
    assert_eq!(choker.statuses[0].duration_unit.as_deref(), Some("TIME"));
    assert_eq!(choker.area.radius_m, None, "the choker declares no radius");
    assert!(
        choker.names_claim(T452_04_AREA_RADIUS),
        "an area effect with no stated area is an unknown"
    );
    assert!(
        choker.names_claim(T452_04_AREA_LIFETIME),
        "and so is its unstated lifetime"
    );

    let smoke = get("wep_13");
    assert_eq!(
        smoke.velocity, None,
        "the smoke screen declares no VELOCITY"
    );
    assert_eq!(smoke.area.lifetime_s, Some(8.0), "SMOKE_SCREEN's 8 s");
    assert_eq!(smoke.statuses[0].duration_s, Some(8.0));
    assert_eq!(smoke.cluster, OriginalClusterShape::Volley { munitions: 6 });
    assert!(
        smoke.names_claim(T452_04_AREA_RADIUS),
        "the smoke screen states no radius either"
    );
    assert!(
        !smoke.names_claim(T452_04_AREA_LIFETIME),
        "but it does state how long its smoke lasts"
    );

    let beeper = get("wep_10");
    assert_eq!(beeper.statuses[0].key, "BEEPER");
    assert_eq!(beeper.statuses[0].duration_s, Some(20.0));

    // The glidebomb: an ordnance record with no ROCKET flag, so no slot.
    let glidebomb = get("wep_25");
    assert_eq!(glidebomb.mount.slots(), None);
    assert!(glidebomb.names_claim(T452_04_MOUNT));
    assert_eq!(glidebomb.area.radius_m, Some(155.0));
    assert_eq!(glidebomb.velocity, Some(450.0));
}

/// The member states no `EXPIRES` lifetime for any record, so every row that
/// does not state a duration of its own carries the named unknown — and the one
/// row that does state one is the flare.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_04_retail_no_record_declares_a_lifetime_key() {
    use cs_content::ordnance::read_original_weapon_census;

    let root = game_dir();
    let census = read_original_weapon_census(&root).expect("the census reads");
    for record in &census.records {
        assert!(
            !record.declares("EXPIRES") && !record.declares("DETONATE_AT_RANGE"),
            "{} would change the measured lifetime rule",
            record.record
        );
    }

    let table = read_original_component_table(&root).expect("the table reads");
    let declared: Vec<&str> = table
        .rows
        .iter()
        .filter(|row| row.lifetime != OriginalLifetime::Undeclared)
        .map(|row| row.record.as_str())
        .collect();
    assert_eq!(
        declared,
        ["wep_15"],
        "only DETONATION_TIME states a duration"
    );
    for row in &table.rows {
        if row.record == "wep_15" {
            continue;
        }
        assert!(
            row.names_claim(T452_04_NO_EXPIRES),
            "{} must record the absent lifetime",
            row.record
        );
        assert_eq!(row.lifetime, OriginalLifetime::Undeclared);
    }

    // Every row records the unknowns its own declarations raise, and nothing
    // else: a record with no VELOCITY does not claim a velocity unit question.
    for row in &table.rows {
        assert_eq!(
            row.names_claim(T452_04_VELOCITY_UNIT),
            row.velocity.is_some(),
            "{}",
            row.record
        );
        assert_eq!(
            row.names_claim(T452_04_RELEASE_DELAY),
            row.fire_rate.is_some(),
            "{}",
            row.record
        );
        assert_eq!(
            row.names_claim(T452_04_AREA_LIFETIME),
            row.area.lifetime_s.is_none()
                && (row.area.radius_m.is_some()
                    || row.family == DeclaredOrdnanceFamily::AreaDenialEngine),
            "{}",
            row.record
        );
        assert_eq!(
            row.names_claim(T452_04_AREA_RADIUS),
            row.family == DeclaredOrdnanceFamily::AreaDenialEngine && row.area.radius_m.is_none(),
            "{}",
            row.record
        );
        assert_eq!(
            row.names_claim(T452_04_STATUS_KIND),
            row.declares_status(),
            "{}",
            row.record
        );
        assert!(
            !row.unknowns
                .iter()
                .any(|unknown| unknown.reason.trim().is_empty()),
            "{} records an unknown with no reason",
            row.record
        );
    }
}

/// No gun's ammunition count can become a sub-munition count: every `CANNON`
/// record's `CLUSTER_SIZE` equals its `AMMO_LIMIT`, and every row's volley is a
/// small whole number.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_04_retail_a_guns_cluster_size_is_its_ammunition_count() {
    use cs_content::ordnance::read_original_weapon_census;

    let census = read_original_weapon_census(&game_dir()).expect("the census reads");
    let guns: Vec<&_> = census
        .records
        .iter()
        .filter(|record| record.declares("CANNON"))
        .collect();
    assert_eq!(guns.len(), 31, "the machine and turret guns");
    let mut declared = 0;
    for gun in &guns {
        let Some(cluster) = gun.first("CLUSTER_SIZE") else {
            // The two turret guns (wep_23, wep_29) declare no CLUSTER_SIZE.
            assert!(
                gun.first("AMMO_LIMIT").is_some(),
                "{} declares neither a cluster nor an ammunition limit",
                gun.record
            );
            continue;
        };
        declared += 1;
        assert_eq!(
            Some(cluster),
            gun.first("AMMO_LIMIT"),
            "{}: a gun's cluster is its ammunition",
            gun.record
        );
        assert!(cluster > 100.0, "{}", gun.record);
    }
    assert_eq!(declared, 29, "every gun but the two turret guns");

    let table = read_original_component_table(&game_dir()).expect("the table reads");
    for row in &table.rows {
        match row.cluster {
            OriginalClusterShape::Volley { munitions } => {
                assert!(
                    (1..=7).contains(&munitions),
                    "{} reports a {munitions}-sub-munition volley",
                    row.record
                );
            }
            OriginalClusterShape::AmmunitionCount { rounds } => {
                panic!("{} is not a gun but reports {rounds} rounds", row.record);
            }
            OriginalClusterShape::Undeclared => {}
        }
    }
}

/// Every claim the table records, with the number of rows it affects
/// (measured). A new claim, a claim that stops being raised or a change in which
/// records it covers is a measurement event, not a silent drift: the committed
/// finding's table says the same numbers.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_04_retail_every_unknown_names_the_rows_it_affects() {
    let table = read_original_component_table(&game_dir()).expect("the table reads");
    let counts: Vec<(String, usize)> = table
        .claims()
        .into_iter()
        .map(|claim| {
            let affected = table
                .rows
                .iter()
                .filter(|row| row.names_claim(&claim))
                .count();
            (claim, affected)
        })
        .collect();
    assert_eq!(
        counts,
        [
            (T452_04_AREA_LIFETIME.to_owned(), 15),
            (T452_04_AREA_RADIUS.to_owned(), 2),
            (T452_04_NO_EXPIRES.to_owned(), 15),
            (T452_04_PER_AIRFRAME.to_owned(), 0),
            (T452_04_RELEASE_DELAY.to_owned(), 16),
            (T452_04_MOUNT.to_owned(), 1),
            (T452_04_STATUS_DURATION.to_owned(), 4),
            (T452_04_STATUS_KIND.to_owned(), 7),
            (T452_04_VELOCITY_UNIT.to_owned(), 15),
            (T452_04_CLUSTER.to_owned(), 11),
        ]
    );
    assert_eq!(table.rows.len(), 16);
    assert_eq!(table.unknowns.len(), 1, "one value no single record owns");
    assert_eq!(table.unknowns[0].claim, T452_04_PER_AIRFRAME);
    assert!(!table.unknowns[0].reason.trim().is_empty());
}
