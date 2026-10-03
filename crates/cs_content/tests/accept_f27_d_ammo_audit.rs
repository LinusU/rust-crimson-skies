//! Acceptance scenario F27-D: the original ammunition/loadout audit maps every
//! ammunition type to its behavior and its damage consumer.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-D`, AC04. Task test prefix: `accept_f27_d_`.
//! Decision record:
//! `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`.
//!
//! What these tests pin is the audit's **verdict discipline**, not a catalogue:
//! an audit that could not fail would let a one-type fixture stand in for the
//! four ammunition types the original declares, which is exactly what F27
//! non-negotiable 1 forbids. So every test here drives the production
//! [`AmmunitionAudit`] over a *synthetic* measured surface and asserts the
//! verdict. The retail measurement of the real surface is
//! `accept_f27_d_retail_ammo_catalogue.rs` (`#[ignore]`, needs `CS_GAME_DIR`).
//!
//! Every value here is newly authored synthetic fixture data. Nothing in this
//! file is original game data, and the synthetic surfaces carry
//! `Origin::SyntheticFixture` so they can never be read as an installation.

use cs_content::damage::DamageNodeKey;
use cs_content::weapons::{
    AmmoAuditFinding, AmmunitionAudit, AmmunitionId, DAMAGE_CONSUMED_BY_ROUTER, DeclaredAmmunition,
    DeclaredCaliber, DeclaredDamageChannel, DeclaredGunDefinition, DeclaredGunGroup,
    DeclaredGunMountKind, DeclaredGunRate, DeclaredInheritanceRule, DeclaredLoadout,
    DeclaredSelfHitRule, DeclaredSpreadCone, DeclaredWeaponDamage, InteractionOption,
    InteractionRules, ORIGINAL_AMMO_NAME_BLOCKS, ORIGINAL_AMMUNITION_TYPES, ORIGINAL_GUN_GROUPS,
    OriginalGunLoadout, OriginalLoadoutCounts, OriginalLoadoutError, SYNTHETIC_LIFETIME_TICKS,
    SYNTHETIC_MUZZLE_VELOCITY_MPS, SYNTHETIC_SPREAD_HALF_ANGLE_RAD, SYNTHETIC_TICKS_BETWEEN_SHOTS,
    synthetic_effect_id, synthetic_sound_id, uncovered_original_gun_groups,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

const PREFIX: &str = "accept_f27_d_";

/// The first string-id block the resource header declares **after** the
/// ammunition blocks, with the macro that declares it.
///
/// **Measured** (the retail test re-reads both out of the installation's own
/// resource header): the four ammunition bases `3350`, `3360`, `3365` and `3370`
/// are gaps of `10`, `5` and `5` apart, *not* four ids wide each, which is why
/// the type count is measured from the screens instead.
pub const MEASURED_NEXT_AMMO_BLOCK_MACRO: &str = "IDS_ROCKETLONGNAME";

/// The identifier [`MEASURED_NEXT_AMMO_BLOCK_MACRO`] declares.
pub const MEASURED_NEXT_AMMO_BLOCK_BASE: u32 = 3380;

fn claim() -> ClaimId {
    ClaimId::new("f27d.ammo-audit-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn unknown<T>(reason: &'static str) -> Resolved<T> {
    Resolved::unknown(claim(), reason).expect("a nonempty reason")
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id")
}

fn gun_id(key: &str) -> ContentId {
    id(ContentKind::Weapon, key)
}

fn ammo_id(key: &str) -> AmmunitionId {
    AmmunitionId::try_new(id(ContentKind::Ammo, key)).expect("an ammo id")
}

fn loadout_id(key: &str) -> ContentId {
    id(ContentKind::Loadout, key)
}

fn mount(key: &str) -> DamageNodeKey {
    DamageNodeKey::new(key).expect("a valid node key")
}

fn caliber() -> Resolved<DeclaredCaliber> {
    known(DeclaredCaliber::try_new("synthetic caliber").expect("a valid caliber"))
}

/// Every option measured with designed provenance.
///
/// "Measured" here means *this record knows the value*, not that the value is
/// the original's. The synthetic fixture declares all five so a session has
/// something to run under; the tests below that need a gap leave one out.
fn rules() -> InteractionRules {
    InteractionRules {
        self_hit: known(DeclaredSelfHitRule::Allowed),
        ..cs_content::weapons::synthetic_interaction_rules()
    }
}

fn damage(armor: f64, internal: f64) -> DeclaredWeaponDamage {
    DeclaredWeaponDamage {
        armor: known(armor),
        internal: known(internal),
    }
}

fn declare_ammunition(
    key: &str,
    profile: DeclaredWeaponDamage,
    interaction: InteractionRules,
) -> DeclaredAmmunition {
    DeclaredAmmunition::try_new(
        ammo_id(key),
        Origin::SyntheticFixture,
        caliber(),
        profile,
        interaction,
        Provenance::designed(claim()),
    )
    .expect("a valid declared ammunition record")
}

/// One declared gun that fires `ammo_key`, on `mount_key` of `kind`.
fn declare_gun(
    key: &str,
    ammo_key: &str,
    mount_key: &str,
    kind: DeclaredGunMountKind,
    profile: DeclaredWeaponDamage,
) -> DeclaredGunDefinition {
    DeclaredGunDefinition::try_new(
        gun_id(key),
        Origin::SyntheticFixture,
        mount(mount_key),
        kind,
        None,
        caliber(),
        known(ammo_id(ammo_key)),
        known(DeclaredGunRate {
            ticks_between_shots: SYNTHETIC_TICKS_BETWEEN_SHOTS,
        }),
        known(SYNTHETIC_MUZZLE_VELOCITY_MPS),
        known(SYNTHETIC_LIFETIME_TICKS),
        DeclaredSpreadCone {
            half_angle: known(Radians(SYNTHETIC_SPREAD_HALF_ANGLE_RAD)),
        },
        profile,
        known(DeclaredInheritanceRule::Full),
        known(synthetic_effect_id()),
        known(synthetic_sound_id()),
        rules(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared gun record")
}

fn declare_loadout(key: &str, guns: &[&str], ammunition: &[&str]) -> DeclaredLoadout {
    DeclaredLoadout::try_new(
        loadout_id(key),
        Origin::SyntheticFixture,
        guns.iter().map(|gun| gun_id(gun)).collect(),
        ammunition.iter().map(|key| ammo_id(key)).collect(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared loadout")
}

/// Five measured counts, all equal to `value`.
fn counts(value: u32) -> OriginalLoadoutCounts {
    OriginalLoadoutCounts::try_new(value, value, value, value, value).expect("a nonzero count set")
}

/// A **synthetic** measured surface: one ammunition type, one selectable gun,
/// one gun slot, and exactly the gun groups the declared mount kinds cover.
///
/// It is not the original's surface and never claims to be: the retail
/// measurement is `accept_f27_d_retail_ammo_catalogue.rs`. A synthetic surface
/// here is what lets the audit's *verdict* be exercised without an
/// installation, and it is labelled `Origin::SyntheticFixture` so no report can
/// mistake it for one.
fn synthetic_surface() -> OriginalGunLoadout {
    let covered: Vec<DeclaredGunGroup> = ORIGINAL_GUN_GROUPS
        .iter()
        .copied()
        .filter(|group| group.is_covered())
        .collect();
    OriginalGunLoadout::try_new(
        Origin::SyntheticFixture,
        counts(1),
        covered,
        Provenance::designed(claim()),
    )
    .expect("a synthetic surface with only covered gun groups")
}

/// The audit the healthy case shares: one type, one gun, one loadout, all
/// measured.
fn complete_audit() -> AmmunitionAudit {
    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(declare_ammunition("type_a", damage(6.0, 3.0), rules()))
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            damage(6.0, 3.0),
        ))
        .add_loadout(declare_loadout("loadout_a", &["gun_a"], &["type_a"]));
    audit
}

/// The healthy case: every type reaches its behavior, its damage consumer and
/// its pairing, and the audit says so.
///
/// This is the only assertion that may ever be `is_complete() == true`, and it
/// needs a surface that matches the records exactly — which is why the retail
/// test, whose surface has four types and twenty groups, is a deliberately
/// *incomplete* case.
#[test]
fn accept_f27_d_a_fully_declared_catalogue_is_complete() {
    let audit = complete_audit();
    let surface = synthetic_surface();
    assert!(
        !surface.origin().is_original(),
        "this test's surface is a synthetic fixture and must not claim otherwise"
    );

    let report = audit.run(&surface);
    assert_eq!(
        report.declared_types(),
        1,
        "the catalogue enumerates exactly the one type it declares"
    );
    assert_eq!(report.consumed_types(), 1);
    assert!(
        report.is_complete(),
        "a catalogue that covers its surface completely is complete; its findings \
         are {:?}",
        report.findings()
    );

    // And the row carries the three things AC04 names.
    let row = report
        .row(&ammo_id("type_a"))
        .expect("the declared type has a row");
    assert_eq!(row.known_caliber(), Some("synthetic caliber"));
    assert_eq!(row.guns(), [gun_id("gun_a")]);
    assert_eq!(row.loadouts(), [loadout_id("loadout_a")]);
    assert_eq!(
        row.consumer().consumer(),
        Some(DAMAGE_CONSUMED_BY_ROUTER),
        "the row names the production path that applies the declared amounts"
    );
    assert_eq!(
        row.consumer().amount(DeclaredDamageChannel::Armor),
        Some(6.0)
    );
    assert_eq!(
        row.consumer().amount(DeclaredDamageChannel::Internal),
        Some(3.0)
    );
    assert_eq!(
        row.behavior().applied(),
        [InteractionOption::SelfHit, InteractionOption::FriendlyFire],
        "the row separates the options a production path applies"
    );
    assert_eq!(
        row.behavior().deferred().len(),
        3,
        "the row names the three options no production path applies, without \
         treating their deferral as a gap in this record"
    );
    assert!(row.behavior().is_measurable());
    assert_eq!(row.origin(), &Origin::SyntheticFixture);
}

/// The **failure** case that makes the audit worth having: a catalogue holding
/// fewer types than the measured surface declares must not pass.
///
/// The mutation this kills is any implementation whose verdict was a function of
/// the records alone and never compared against the surface — which would let a
/// one-type catalogue stand in for the original's four.
#[test]
fn accept_f27_d_a_catalogue_that_under_counts_the_installation_is_incomplete() {
    let audit = complete_audit();
    let base = synthetic_surface();
    let surface = OriginalGunLoadout::try_new(
        base.origin().clone(),
        counts(base.ammunition_types() + 2),
        base.gun_groups().to_vec(),
        Provenance::designed(claim()),
    )
    .expect("a bigger surface is still a valid surface");

    let report = audit.run(&surface);
    assert!(
        !report.is_complete(),
        "one declared type cannot cover three measured types"
    );
    let findings = report.findings_of("undeclared_ammunition_type");
    assert_eq!(findings.len(), 1, "the shortfall is reported exactly once");
    match findings[0] {
        AmmoAuditFinding::UndeclaredAmmunitionType { observed, declared } => {
            assert_eq!(*observed, 3);
            assert_eq!(*declared, 1);
        }
        other => panic!("unexpected finding {other}"),
    }
    let text = findings[0].to_string();
    assert!(text.contains("3 ammunition types"), "named gap: {text}");
    assert!(text.contains("only 1"), "named gap: {text}");
}

/// A type whose damage is entirely unmeasured has **no consumer**, and the audit
/// says so instead of naming a path that could never receive anything.
#[test]
fn accept_f27_d_a_type_with_no_measured_damage_has_no_damage_consumer() {
    let profile = DeclaredWeaponDamage {
        armor: unknown("the original's per-type damage is unmeasured"),
        internal: unknown("the original's per-type damage is unmeasured"),
    };
    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(declare_ammunition("type_a", profile.clone(), rules()))
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            profile,
        ))
        .add_loadout(declare_loadout("loadout_a", &["gun_a"], &["type_a"]));

    let report = audit.run(&synthetic_surface());
    assert!(!report.is_complete());
    let row = report
        .row(&ammo_id("type_a"))
        .expect("the declared type still has a row");
    assert!(
        !row.consumer().is_consumed(),
        "an entirely unknown profile reaches no consumer"
    );
    assert_eq!(
        row.consumer().consumer(),
        None,
        "naming a production path for a type that delivers nothing would be a \
         consumer that can never fire"
    );
    assert_eq!(row.consumer().unmeasured().len(), 2);
    assert_eq!(
        report.findings_of("no_damage_consumer").len(),
        1,
        "the missing consumer is reported"
    );
    assert_eq!(report.consumed_types(), 0);
}

/// One *unmeasured channel* is distinguishable from an unmeasured type: the type
/// reaches a consumer, but not on the channel the record did not declare.
#[test]
fn accept_f27_d_a_half_measured_profile_reports_the_missing_channel() {
    let profile = DeclaredWeaponDamage {
        armor: known(4.0),
        internal: unknown("the internal-channel amount is unmeasured"),
    };
    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(declare_ammunition("type_a", profile.clone(), rules()))
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            profile,
        ))
        .add_loadout(declare_loadout("loadout_a", &["gun_a"], &["type_a"]));

    let report = audit.run(&synthetic_surface());
    let row = report
        .row(&ammo_id("type_a"))
        .expect("the declared type has a row");
    assert!(row.consumer().is_consumed());
    assert_eq!(
        row.consumer().amount(DeclaredDamageChannel::Armor),
        Some(4.0)
    );
    assert_eq!(row.consumer().amount(DeclaredDamageChannel::Internal), None);
    assert_eq!(
        row.consumer().unmeasured(),
        [DeclaredDamageChannel::Internal]
    );
    assert_eq!(
        report.findings_of("no_damage_consumer").len(),
        0,
        "one measured channel is still a consumer"
    );
}

/// A known amount of **zero** is a measurement, not a gap: the original's data
/// can declare a channel it does not damage. The audit must not confuse it with
/// an unknown amount, and the runtime registry is where "delivers nothing" is
/// decided.
#[test]
fn accept_f27_d_a_known_zero_amount_is_not_reported_as_unmeasured() {
    let profile = damage(0.0, 0.0);
    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(declare_ammunition("type_a", profile.clone(), rules()))
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            profile,
        ))
        .add_loadout(declare_loadout("loadout_a", &["gun_a"], &["type_a"]));

    let report = audit.run(&synthetic_surface());
    let row = report
        .row(&ammo_id("type_a"))
        .expect("the declared type has a row");
    assert_eq!(row.consumer().unmeasured().len(), 0);
    assert_eq!(
        row.consumer().amount(DeclaredDamageChannel::Armor),
        Some(0.0),
        "a measured zero is still a measured amount"
    );
    assert!(
        row.consumer().is_consumed(),
        "the declared record reaches a consumer; whether a zero delivers anything \
         is the runtime registry's question, not the declared record's"
    );
}

/// An interaction rule left unknown is reported by option name, so a record
/// cannot present a behavior it never declared.
#[test]
fn accept_f27_d_an_unmeasured_interaction_rule_is_named_by_option() {
    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(declare_ammunition(
            "type_a",
            damage(6.0, 3.0),
            InteractionRules {
                penetration: unknown("the original's penetration behavior is unmeasured"),
                ..rules()
            },
        ))
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            damage(6.0, 3.0),
        ))
        .add_loadout(declare_loadout("loadout_a", &["gun_a"], &["type_a"]));

    let report = audit.run(&synthetic_surface());
    assert!(!report.is_complete());
    let unmeasured = report.findings_of("unmeasured_rule");
    assert_eq!(
        unmeasured.len(),
        1,
        "only the option that is actually unknown is reported"
    );
    match unmeasured[0] {
        AmmoAuditFinding::UnmeasuredRule { option, .. } => {
            assert_eq!(*option, InteractionOption::Penetration);
        }
        other => panic!("unexpected finding {other}"),
    }
    assert!(
        unmeasured[0].to_string().contains("penetration"),
        "the finding names the option: {}",
        unmeasured[0]
    );
    assert!(
        !unmeasured[0].to_string().contains("ricochet"),
        "and names only the option that is unknown: {}",
        unmeasured[0]
    );
}

/// An unmeasured caliber is reported, so a type cannot present itself as a
/// measured round.
#[test]
fn accept_f27_d_an_unmeasured_caliber_is_reported() {
    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(
            DeclaredAmmunition::try_new(
                ammo_id("type_a"),
                Origin::SyntheticFixture,
                unknown("the original caliber vocabulary is unmeasured"),
                damage(6.0, 3.0),
                rules(),
                Provenance::designed(claim()),
            )
            .expect("an unknown caliber is a legal record"),
        )
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            damage(6.0, 3.0),
        ))
        .add_loadout(declare_loadout("loadout_a", &["gun_a"], &["type_a"]));

    let report = audit.run(&synthetic_surface());
    assert_eq!(report.findings_of("unmeasured_caliber").len(), 1);
    let row = report
        .row(&ammo_id("type_a"))
        .expect("the declared type has a row");
    assert_eq!(row.known_caliber(), None);
}

/// A type paired with no gun in any loadout is unreachable and is reported: the
/// original lets every hardpoint choose its ammunition, so an unpaired type is
/// not "available but idle", it is not in the game at all.
#[test]
fn accept_f27_d_an_unpaired_type_is_reported() {
    let mut audit = AmmunitionAudit::new();
    audit.add_ammunition(declare_ammunition("orphan", damage(6.0, 3.0), rules()));
    let report = audit.run(&synthetic_surface());
    assert_eq!(report.findings_of("unpaired").len(), 1);
    let row = report
        .row(&ammo_id("orphan"))
        .expect("the declared type still has a row");
    assert!(row.guns().is_empty());
    assert!(row.loadouts().is_empty());
    // An unpaired type is still a *measured* type: its damage still reaches a
    // consumer, and only the pairing is missing. Reporting it as an unmeasured
    // type would hide which half of the record is intact.
    assert_eq!(
        report.consumed_types(),
        1,
        "the type's own damage profile is intact; only the pairing is missing"
    );
    assert_eq!(
        report.findings_of("no_damage_consumer").len(),
        0,
        "and it is not reported as unmeasured"
    );
}

/// A loadout that names a gun or a type nothing describes is reported on both
/// sides: the dangling reference and the missing record.
#[test]
fn accept_f27_d_a_dangling_loadout_reference_is_reported_both_ways() {
    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(declare_ammunition("type_a", damage(6.0, 3.0), rules()))
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            damage(6.0, 3.0),
        ))
        // `gun_b` and `type_b` are named but declared by nothing.
        .add_loadout(declare_loadout(
            "loadout_a",
            &["gun_a", "gun_b"],
            &["type_a", "type_b"],
        ));

    let report = audit.run(&synthetic_surface());
    assert_eq!(report.findings_of("undescribed_gun").len(), 1);
    assert_eq!(report.findings_of("undescribed_ammunition").len(), 1);
    let gun = report.findings_of("undescribed_gun")[0].to_string();
    assert!(gun.contains("gun_b"), "named gap: {gun}");
    assert!(gun.contains("loadout_a"), "named gap: {gun}");
    let ammunition = report.findings_of("undescribed_ammunition")[0].to_string();
    assert!(ammunition.contains("type_b"), "named gap: {ammunition}");

    // The described pairing is still walked, so one dangling entry does not lose
    // the good row's pairing.
    let row = report
        .row(&ammo_id("type_a"))
        .expect("the described type has a row");
    assert_eq!(row.guns(), [gun_id("gun_a"), gun_id("gun_b")]);
}

/// Two records for the same ammunition type must not inflate the type count past
/// the closure check.
#[test]
fn accept_f27_d_a_repeated_type_record_is_audited_once() {
    let mut audit = complete_audit();
    audit.add_ammunition(declare_ammunition("type_a", damage(9.0, 4.0), rules()));
    let surface = synthetic_surface();
    let report = audit.run(&surface);
    assert_eq!(
        report.declared_types(),
        1,
        "two records naming one type are one type"
    );
    // The first record in insertion order wins, and the disagreement is *not*
    // silently a finding: the declared schema treats two records for one type
    // as a data defect the importer must resolve, and the audit reports the
    // first rather than averaging or picking the higher number.
    let row = report
        .row(&ammo_id("type_a"))
        .expect("the declared type has a row");
    assert_eq!(
        row.consumer().amount(DeclaredDamageChannel::Armor),
        Some(6.0)
    );
}

/// The mount side of AC04, and this stage's substantive finding: the original
/// declares twenty gun groups and the designed five mount kinds cover only the
/// nine whose kind the original's own label determines.
///
/// The mutation this kills is one that assigned every group a kind — for
/// example by guessing that `INNERWINGGUNS` is a left-wing group — which would
/// make the audit pass on a side the original never states.
#[test]
fn accept_f27_d_the_measured_gun_group_vocabulary_is_larger_than_the_mount_kinds() {
    assert_eq!(
        ORIGINAL_GUN_GROUPS.len(),
        20,
        "the original declares twenty gun groups"
    );
    let uncovered = uncovered_original_gun_groups();
    assert_eq!(
        uncovered.len(),
        11,
        "the designed mount kinds cover nine of the twenty measured gun groups; \
         the other eleven name a wing station and often omit the side, which \
         needs the executable's per-airframe tables"
    );
    assert_eq!(
        uncovered.iter().map(|group| group.id()).collect::<Vec<_>>(),
        vec![
            3061, 3062, 3065, 3069, 3070, 3074, 3075, 3076, 3077, 3078, 3079
        ],
        "the uncovered groups are exactly the side-less inner/outer/middle wing, \
         upper/lower wing and centre groups"
    );
    let covered: Vec<u32> = ORIGINAL_GUN_GROUPS
        .iter()
        .filter(|group| group.is_covered())
        .map(|group| group.id())
        .collect();
    assert_eq!(
        covered,
        vec![3063, 3064, 3066, 3067, 3068, 3071, 3072, 3073, 3080],
        "and the covered ones are the nine the labels do determine"
    );
    assert_eq!(
        DeclaredGunMountKind::WingLeft.original_groups().len(),
        1,
        "only LEFTWINGGUNS names a left wing group"
    );
    assert_eq!(
        DeclaredGunMountKind::WingRight.original_groups().len(),
        1,
        "only RIGHTWINGGUNS names a right wing group"
    );
    assert_eq!(
        DeclaredGunMountKind::Tail.original_groups()[0].id(),
        3073,
        "REARTURRET is the group the designed tail kind covers"
    );
    assert_eq!(
        DeclaredGunMountKind::Gondola.original_groups()[0].id(),
        3066,
        "RIGHTFUSELAGEGUNS is the fuselage group the gondola kind covers"
    );
    assert_eq!(
        DeclaredGunMountKind::Nose.original_groups().len(),
        5,
        "the lower nose, upper nose, two nose and nose-turret groups are nose"
    );
    assert_eq!(
        DeclaredGunGroup::by_id(3073).map(|group| group.label()),
        Some("REARTURRET"),
        "a group is addressable by id"
    );
    assert_eq!(DeclaredGunGroup::by_id(1), None);
}

/// A mount kind with no counterpart in the measured surface is reported, so a
/// loadout that uses a designed-only kind is visible.
#[test]
fn accept_f27_d_a_mount_kind_the_installation_never_names_is_reported() {
    // A surface that names only a nose group: the wing kind a declared gun uses
    // corresponds to nothing the installation declares.
    let surface = OriginalGunLoadout::try_new(
        Origin::SyntheticFixture,
        counts(1),
        vec![DeclaredGunGroup::new(3071, "NOSEGUNS")],
        Provenance::designed(claim()),
    )
    .expect("a one-group surface is valid");

    let mut audit = AmmunitionAudit::new();
    audit
        .add_ammunition(declare_ammunition("type_a", damage(6.0, 3.0), rules()))
        .add_gun(declare_gun(
            "gun_a",
            "type_a",
            "mount_a",
            DeclaredGunMountKind::Nose,
            damage(6.0, 3.0),
        ))
        .add_gun(declare_gun(
            "gun_b",
            "type_a",
            "mount_b",
            DeclaredGunMountKind::WingLeft,
            damage(6.0, 3.0),
        ))
        .add_loadout(declare_loadout(
            "loadout_a",
            &["gun_a", "gun_b"],
            &["type_a"],
        ));

    let report = audit.run(&surface);
    let unobserved = report.findings_of("unobserved_mount_kind");
    assert_eq!(
        unobserved.len(),
        1,
        "only the kind in use that the surface never names is reported"
    );
    match unobserved[0] {
        AmmoAuditFinding::UnobservedMountKind { kind } => {
            assert_eq!(*kind, DeclaredGunMountKind::WingLeft);
        }
        other => panic!("unexpected finding {other}"),
    }
    assert_eq!(
        report.findings_of("uncovered_gun_group").len(),
        0,
        "the one group the surface names *is* covered"
    );
}

/// An uncovered gun group is reported *by name*, once per group, so a caller
/// can act on the list instead of re-deriving it.
#[test]
fn accept_f27_d_every_uncovered_gun_group_is_reported_once_by_name() {
    let surface = OriginalGunLoadout::try_new(
        Origin::SyntheticFixture,
        counts(1),
        ORIGINAL_GUN_GROUPS.to_vec(),
        Provenance::designed(claim()),
    )
    .expect("the full measured group list is a valid surface");

    let report = complete_audit().run(&surface);
    let findings = report.findings_of("uncovered_gun_group");
    assert_eq!(
        findings.len(),
        11,
        "each of the eleven uncovered groups is reported exactly once"
    );
    let named: Vec<u32> = findings
        .iter()
        .map(|finding| match finding {
            AmmoAuditFinding::UncoveredGunGroup { group } => group.id(),
            other => panic!("unexpected finding {other}"),
        })
        .collect();
    assert_eq!(
        named,
        uncovered_original_gun_groups()
            .iter()
            .map(|group| group.id())
            .collect::<Vec<_>>(),
        "the reported groups are exactly the uncovered ones, in the measured order"
    );
    assert!(
        findings[0].to_string().contains("INNERWINGGUNS"),
        "the first reported group is named by the original's own label: {}",
        findings[0]
    );
}

/// An empty audit is not vacuously complete: a catalogue with nothing in it
/// cannot cover any surface.
#[test]
fn accept_f27_d_an_empty_catalogue_is_incomplete() {
    let report = AmmunitionAudit::new().run(&synthetic_surface());
    assert_eq!(report.declared_types(), 0);
    assert_eq!(report.consumed_types(), 0);
    assert!(
        !report.is_complete(),
        "an audit with no records covers nothing and must not pass"
    );
    assert_eq!(
        report.findings_of("undeclared_ammunition_type").len(),
        1,
        "and it says which surface it failed to cover"
    );
}

/// A contradictory measured surface is refused rather than audited: a zero
/// count, an out-of-range group or a duplicated group all mean the surface was
/// misread, and auditing a misread surface would produce a wrong verdict.
#[test]
fn accept_f27_d_a_contradictory_measured_surface_is_refused() {
    let group = DeclaredGunGroup::new(3071, "NOSEGUNS");

    // A zero count is refused by name, before a surface exists at all.
    for (at, field) in [
        (0usize, "ammunition_types"),
        (1, "selectable_guns"),
        (2, "gun_slots"),
        (3, "rocket_slots"),
        (4, "hardpoint_points"),
    ] {
        let mut values = [1u32; 5];
        values[at] = 0;
        let error =
            OriginalLoadoutCounts::try_new(values[0], values[1], values[2], values[3], values[4])
                .expect_err("a zero count is refused");
        assert_eq!(error, OriginalLoadoutError::ZeroCount { field }, "{field}");
    }

    let duplicated = OriginalGunLoadout::try_new(
        Origin::SyntheticFixture,
        counts(1),
        vec![group, group],
        Provenance::designed(claim()),
    )
    .expect_err("a duplicated group is refused");
    assert_eq!(
        duplicated,
        OriginalLoadoutError::DuplicateGunGroup { group }
    );

    let out_of_range = DeclaredGunGroup::new(9999, "NOTAGROUP");
    let error = OriginalGunLoadout::try_new(
        Origin::SyntheticFixture,
        counts(1),
        vec![out_of_range],
        Provenance::designed(claim()),
    )
    .expect_err("an out-of-range group is refused");
    assert_eq!(
        error,
        OriginalLoadoutError::GunGroupOutOfRange {
            group: out_of_range
        }
    );
    assert!(
        error.to_string().contains("3061..=3080"),
        "the refusal names the range it refused against: {error}"
    );

    let none = OriginalGunLoadout::try_new(
        Origin::SyntheticFixture,
        counts(1),
        Vec::new(),
        Provenance::designed(claim()),
    )
    .expect_err("a surface with no group is refused");
    assert_eq!(none, OriginalLoadoutError::NoGunGroups);
}

/// The measured ammunition-block **bases** are fixed, and they are not four ids
/// wide each: the header's own allocation makes the blocks 10, 5 and 5 apart, and
/// the retail test re-measures the next block after them out of the same header.
/// The type count is therefore measured from the screens, not from these gaps.
#[test]
fn accept_f27_d_the_measured_ammunition_blocks_are_not_four_ids_wide_each() {
    assert_eq!(ORIGINAL_AMMO_NAME_BLOCKS.len(), 4, "four name blocks");
    let bases: Vec<u32> = ORIGINAL_AMMO_NAME_BLOCKS
        .iter()
        .map(|(base, _)| *base)
        .collect();
    assert_eq!(bases, vec![3350, 3360, 3365, 3370]);
    assert_eq!(ORIGINAL_AMMUNITION_TYPES, 4);
    // The gaps between consecutive bases are 10, 5 and 5 — deliberately *not*
    // the type count, so an implementation that derived the count from a gap
    // would read 5 or 10. The count is four because the description block is
    // indexed `3370 + selection - 1` for `selection` in `1..=4`.
    let description = ORIGINAL_AMMO_NAME_BLOCKS[3];
    assert_eq!(description.0, 3370);
    assert_eq!(
        description.0 + ORIGINAL_AMMUNITION_TYPES - 1,
        3373,
        "the last description id is 3373"
    );
    // Every block must lie inside the header's own ammunition run and stop before
    // the next block the header declares (`IDS_ROCKETLONGNAME 3380`). The block
    // widths are therefore *unequal*, which is the property that makes reading a
    // type count off the bases wrong.
    let run_start = ORIGINAL_AMMO_NAME_BLOCKS[0].0;
    let run_end = description.0 + ORIGINAL_AMMUNITION_TYPES;
    assert_eq!(run_start, 3350);
    assert!(
        run_end <= MEASURED_NEXT_AMMO_BLOCK_BASE,
        "the ammunition blocks end at {run_end}, before the header's next block \
         {} at {MEASURED_NEXT_AMMO_BLOCK_BASE}",
        MEASURED_NEXT_AMMO_BLOCK_MACRO
    );
    for (base, label) in ORIGINAL_AMMO_NAME_BLOCKS {
        assert!(
            base >= run_start && base + ORIGINAL_AMMUNITION_TYPES <= run_end,
            "block {label} at {base} must lie inside {run_start}..={run_end} and not \
             spill into the next block"
        );
    }
    // The gaps are the header's allocation, and none of them is the type count, so
    // an implementation that derived the count from a gap would read 10 or 5.
    let gaps: Vec<u32> = ORIGINAL_AMMO_NAME_BLOCKS
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .collect();
    assert_eq!(
        gaps,
        vec![10, 5, 5],
        "the measured block gaps; the type count is not among them"
    );
}

/// The prefix is a namespace, not a decoration: every test in this file is
/// named with it, so `cargo test --workspace -- accept_f27_d_` selects exactly
/// these and the ignored retail tests and nothing else.
#[test]
fn accept_f27_d_the_catalogue_suite_declares_only_the_task_test_prefix() {
    assert_eq!(PREFIX, "accept_f27_d_");
    for name in [
        "accept_f27_d_a_fully_declared_catalogue_is_complete",
        "accept_f27_d_a_catalogue_that_under_counts_the_installation_is_incomplete",
        "accept_f27_d_a_type_with_no_measured_damage_has_no_damage_consumer",
        "accept_f27_d_a_half_measured_profile_reports_the_missing_channel",
        "accept_f27_d_a_known_zero_amount_is_not_reported_as_unmeasured",
        "accept_f27_d_an_unmeasured_interaction_rule_is_named_by_option",
        "accept_f27_d_an_unmeasured_caliber_is_reported",
        "accept_f27_d_an_unpaired_type_is_reported",
        "accept_f27_d_a_dangling_loadout_reference_is_reported_both_ways",
        "accept_f27_d_a_repeated_type_record_is_audited_once",
        "accept_f27_d_the_measured_gun_group_vocabulary_is_larger_than_the_mount_kinds",
        "accept_f27_d_a_mount_kind_the_installation_never_names_is_reported",
        "accept_f27_d_every_uncovered_gun_group_is_reported_once_by_name",
        "accept_f27_d_an_empty_catalogue_is_incomplete",
        "accept_f27_d_a_contradictory_measured_surface_is_refused",
        "accept_f27_d_the_measured_ammunition_blocks_are_not_four_ids_wide_each",
        "accept_f27_d_the_catalogue_suite_declares_only_the_task_test_prefix",
    ] {
        assert!(name.starts_with(PREFIX), "{name} is outside the prefix");
    }
}
