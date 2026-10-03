//! F27-E.1 acceptance tests: the measured binding gate.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`
//! (non-negotiable 1 "no unverified multiplier table" and non-negotiable 2
//! "mount transforms come from the live aircraft hierarchy, never a fixed
//! centre-screen origin"; F27-D's AC04 closure target). Task #547. Owner paths:
//! `crates/cs_content/src/weapons.rs` and this file.
//!
//! The stage's own scenario is AC04 again, one level down: given the original's
//! ammunition damage amounts and per-airframe gun mounts **once they are
//! measurable**, the gate binds exactly the measured values and refuses
//! everything else. So the fast half drives
//! [`bind_gun_mount`](cs_content::weapons::bind_gun_mount),
//! [`bind_ammunition_damage`](cs_content::weapons::bind_ammunition_damage),
//! the two closure helpers and
//! [`OriginalLimitReport`](cs_content::weapons::OriginalLimitReport) with
//! measurements that are shaped like the ones a capture would carry, and every
//! test below removes one decision the gate makes.
//!
//! Nothing here is a claim about the original's numbers. The measurements in
//! this file are **authored test inputs** and are marked
//! `ClaimStatus::ObservedTool` over a span no installation has, exactly as
//! F27-E's fast half does for its catalog stand-in. What the file asserts is
//! that the gate cannot be talked into inventing a mount, a mount kind, a
//! scene binding or a damage amount — which is the property that has to hold
//! whether the numbers arrive from a capture, a decoded image or nowhere.

use cs_content::damage::DamageNodeKey;
use cs_content::scene::SceneNodeId;
use cs_content::weapons::{
    AmmunitionAudit, AmmunitionId, DamageBindingRefusal, DeclaredAmmunition, DeclaredDamageChannel,
    DeclaredGunDefinition, DeclaredGunMountKind, DeclaredLoadout, DeclaredSpreadCone,
    DeclaredWeaponDamage, GunMountField, GunMountRefusal, InteractionOption, InteractionRules,
    LimitEvidence, LimitOutcome, LimitReportError, MeasuredAmmunitionDamage, MeasuredGunMount,
    ORIGINAL_AMMUNITION_TYPES, ORIGINAL_GUN_GROUPS, ORIGINAL_GUN_SLOTS, ORIGINAL_HARDPOINT_POINTS,
    ORIGINAL_ROCKET_SLOTS, ORIGINAL_SELECTABLE_GUNS, OriginalGunLoadout, OriginalLimitClaim,
    OriginalLimitReport, OriginalLoadoutCounts, WeaponSchemaError, bind_ammunition_damage,
    bind_gun_mount, is_observed_evidence, unmeasured_ammunition_types, unmeasured_gun_mounts,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimStatus;

/// The claim every measurement in this file is recorded under.
const MEASURED: &str = "f27.e1.test-measurement";

/// The claim the placeholder values the fixtures carry are recorded under, so a
/// `Designed` provenance and an `ObservedTool` one never share a claim id.
const DESIGNED: &str = "f27.e1.test-placeholder";

/// A claim id this file uses.
fn claim(id: &str) -> cs_types::evidence::ClaimId {
    cs_types::evidence::ClaimId::new(id).expect("a valid claim id")
}

/// A measurement's provenance: somebody read it, in this file's authored input.
fn observed() -> Provenance {
    Provenance::new(claim(MEASURED), ClaimStatus::ObservedTool, None)
        .expect("an observed-tool provenance with no span is valid")
}

/// A placeholder's provenance: this project's own design, not a measurement.
fn designed() -> Provenance {
    Provenance::designed(claim(DESIGNED))
}

/// A known value with its provenance.
fn known<T>(value: T, provenance: Provenance) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance))
}

/// An explicit unknown, as the importer would record it.
fn unknown<T>(id: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(claim(id), reason).expect("a nonempty reason is valid")
}

/// Interaction rules that are declared and know nothing: the state an original
/// type is in for penetration, ricochet and ammo switching.
fn unmeasured_rules() -> InteractionRules {
    InteractionRules {
        self_hit: unknown("f27.e1.test.self_hit", "unmeasured in this fixture"),
        friendly_fire: unknown("f27.e1.test.friendly_fire", "unmeasured in this fixture"),
        penetration: unknown("f27.e1.test.penetration", "unmeasured in this fixture"),
        ricochet: unknown("f27.e1.test.ricochet", "unmeasured in this fixture"),
        ammo_switching: unknown("f27.e1.test.ammo_switching", "unmeasured in this fixture"),
    }
}

/// The `weapon` id of one gun in this file.
fn gun_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Weapon, key).expect("the gun id is valid")
}

/// The `ammo` id of one ammunition type in this file.
fn ammunition_id(key: &str) -> AmmunitionId {
    AmmunitionId::try_new(
        ContentId::from_source(ContentKind::Ammo, key).expect("the ammunition id is valid"),
    )
    .expect("the ammunition id is in the ammo namespace")
}

/// A `scene_node` id of one visual node in this file.
fn node_id(key: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, key).expect("the node id is valid"),
    )
    .expect("the node id is in the scene_node namespace")
}

/// A declared gun record that measures nothing at all.
///
/// The two mount fields **cannot** be unknown in the schema — that is the whole
/// reason this stage exists — so the fixture carries a placeholder mount and a
/// placeholder kind, and every test that binds a measurement asserts the
/// placeholder is *replaced*, not kept.
fn unmeasured_gun(key: &str) -> DeclaredGunDefinition {
    DeclaredGunDefinition::try_new(
        gun_id(key),
        Origin::SyntheticFixture,
        DamageNodeKey::new("placeholder_mount").expect("the placeholder mount key is valid"),
        DeclaredGunMountKind::Gondola,
        None,
        unknown("f27.e1.test.caliber", "unmeasured in this fixture"),
        unknown("f27.e1.test.ammunition", "unmeasured in this fixture"),
        unknown("f27.e1.test.rate", "unmeasured in this fixture"),
        unknown("f27.e1.test.velocity", "unmeasured in this fixture"),
        unknown("f27.e1.test.lifetime", "unmeasured in this fixture"),
        DeclaredSpreadCone {
            half_angle: unknown("f27.e1.test.spread", "unmeasured in this fixture"),
        },
        DeclaredWeaponDamage {
            armor: unknown("f27.e1.test.damage_armor", "unmeasured in this fixture"),
            internal: unknown("f27.e1.test.damage_internal", "unmeasured in this fixture"),
        },
        unknown("f27.e1.test.inheritance", "unmeasured in this fixture"),
        unknown("f27.e1.test.effect", "unmeasured in this fixture"),
        unknown("f27.e1.test.sound", "unmeasured in this fixture"),
        unmeasured_rules(),
        designed(),
    )
    .expect("the unmeasured gun record is structurally valid")
}

/// A declared ammunition record that measures nothing: no caliber, no damage on
/// either channel, no known interaction option. This is the state F27-E leaves
/// every imported original type in.
fn unmeasured_ammunition(key: &str) -> DeclaredAmmunition {
    DeclaredAmmunition::try_new(
        ammunition_id(key),
        Origin::SyntheticFixture,
        unknown(
            "f27.e1.test.caliber",
            "the original enumerates caliber per gun",
        ),
        DeclaredWeaponDamage {
            armor: unknown(
                "f27.e1.test.damage_armor",
                "the original's amount is unmeasured",
            ),
            internal: unknown(
                "f27.e1.test.damage_internal",
                "the original's amount is unmeasured",
            ),
        },
        unmeasured_rules(),
        designed(),
    )
    .expect("the unmeasured ammunition record is structurally valid")
}

/// A measurement of one gun's whole mount.
fn measured_mount(key: &str) -> MeasuredGunMount {
    MeasuredGunMount::new(
        gun_id(key),
        known(
            DamageNodeKey::new("wing_left_gun_1").expect("the measured mount key is valid"),
            observed(),
        ),
        known(DeclaredGunMountKind::WingLeft, observed()),
        known(node_id("lancaster.wing_left.1"), observed()),
    )
}

/// The measurement the mount helpers start from, so a test can blank exactly one
/// field.
fn mount_with(field: GunMountField) -> MeasuredGunMount {
    let complete = measured_mount(GUN);
    match field {
        GunMountField::Mount => MeasuredGunMount::new(
            complete.gun().clone(),
            unknown("f27.e1.test.mount", "the side of this group is unmeasured"),
            complete.mount_kind().clone(),
            complete.scene_binding().clone(),
        ),
        GunMountField::MountKind => MeasuredGunMount::new(
            complete.gun().clone(),
            complete.mount().clone(),
            unknown("f27.e1.test.mount_kind", "this group names no side"),
            complete.scene_binding().clone(),
        ),
        GunMountField::SceneBinding => MeasuredGunMount::new(
            complete.gun().clone(),
            complete.mount().clone(),
            complete.mount_kind().clone(),
            unknown("f27.e1.test.scene_binding", "no mesh node was matched"),
        ),
    }
}

/// The gun every mount test binds.
const GUN: &str = "fixture.gun_alpha";

/// The ammunition every damage test binds.
const AMMO: &str = "fixture.ammo_alpha";

/// A measurement of one type's armor channel only.
fn measured_damage_armor_only(key: &str) -> MeasuredAmmunitionDamage {
    MeasuredAmmunitionDamage::new(
        ammunition_id(key),
        DeclaredWeaponDamage {
            armor: known(6.5_f64, observed()),
            internal: unknown(
                "f27.e1.test.internal",
                "this capture measured the armor channel",
            ),
        },
    )
}

/// **A measurement replaces the placeholder mount, and leaves every other field
/// exactly as unmeasured as it was.**
#[test]
fn accept_f27_e_1_a_measured_mount_replaces_the_placeholder_and_keeps_the_rest_unknown() {
    let record = unmeasured_gun(GUN);
    assert_eq!(record.mount().as_str(), "placeholder_mount");
    assert_eq!(record.mount_kind(), DeclaredGunMountKind::Gondola);
    assert!(record.scene_binding().is_none());

    let measured = measured_mount(GUN);
    assert!(
        measured.unmeasured().is_empty() && measured.is_complete(),
        "the fixture measurement is observation-backed in all three fields"
    );
    let bound = bind_gun_mount(&record, &measured).expect("a complete measurement binds");

    // The three mount fields are the measurement's, each with its provenance.
    assert_eq!(bound.mount().as_str(), "wing_left_gun_1");
    assert_eq!(bound.mount_kind(), DeclaredGunMountKind::WingLeft);
    let binding = bound
        .scene_binding()
        .expect("a measured scene binding is kept");
    match binding {
        Resolved::Known(known) => {
            assert_eq!(known.value, node_id("lancaster.wing_left.1"));
            assert!(is_observed_evidence(known.provenance.class));
        }
        Resolved::Unknown { .. } => panic!("the measured scene binding must stay known"),
    }

    // Nothing else moved: the gate binds the mount and carries the rest across.
    assert_eq!(bound.gun(), record.gun());
    assert_eq!(bound.origin(), record.origin());
    assert_eq!(bound.caliber(), record.caliber());
    assert_eq!(bound.ammunition(), record.ammunition());
    assert_eq!(bound.rate(), record.rate());
    assert_eq!(bound.muzzle_velocity_mps(), record.muzzle_velocity_mps());
    assert_eq!(bound.lifetime_ticks(), record.lifetime_ticks());
    assert_eq!(bound.spread(), record.spread());
    assert_eq!(bound.damage(), record.damage());
    assert_eq!(bound.inheritance(), record.inheritance());
    assert_eq!(bound.effect(), record.effect());
    assert_eq!(bound.sound(), record.sound());
    assert_eq!(bound.rules(), record.rules());
    assert_eq!(bound.provenance(), record.provenance());
    for option in InteractionOption::ALL {
        assert!(
            !bound.rules().is_known(*option),
            "{option} must stay unmeasured through a mount binding"
        );
    }
}

/// **A measurement missing any one of the three fields is refused *by that
/// field's name*** — no default mount, no neighbouring record's kind.
#[test]
fn accept_f27_e_1_a_mount_missing_any_one_field_is_refused_by_name() {
    let record = unmeasured_gun(GUN);
    for field in GunMountField::ALL {
        let measured = mount_with(field);
        assert_eq!(
            measured.unmeasured(),
            vec![(
                field,
                cs_content::weapons::UnmeasuredCause::Unknown {
                    reason: measured_unmeasured_reason(field),
                }
            )],
            "{field} is the one field this measurement leaves open"
        );
        assert!(!measured.is_complete());
        match bind_gun_mount(&record, &measured) {
            Err(GunMountRefusal::Unmeasured {
                gun, field: named, ..
            }) => {
                assert_eq!(named, field);
                assert_eq!(&gun, record.gun());
            }
            other => panic!("a mount missing {field} must be refused, got {other:?}"),
        }
    }
}

/// The reason each fixture field carries, so the refusal's cause can be checked
/// rather than merely counted.
fn measured_unmeasured_reason(field: GunMountField) -> String {
    match field {
        GunMountField::Mount => "the side of this group is unmeasured".to_owned(),
        GunMountField::MountKind => "this group names no side".to_owned(),
        GunMountField::SceneBinding => "no mesh node was matched".to_owned(),
    }
}

/// **A value that is *known* but whose provenance is not an observation is not
/// a measurement**, whichever of the five non-observation classes it carries.
#[test]
fn accept_f27_e_1_a_known_field_that_is_not_an_observation_is_refused() {
    let record = unmeasured_gun(GUN);
    for class in [
        ClaimStatus::Documented,
        ClaimStatus::Designed,
        ClaimStatus::Inferred,
        ClaimStatus::Unknown,
        ClaimStatus::Contradicted,
    ] {
        assert!(
            !is_observed_evidence(class),
            "{class} must not count as an observation"
        );
        let provenance = Provenance::new(claim(MEASURED), class, None)
            .unwrap_or_else(|_| Provenance::designed(claim(MEASURED)));
        let measured = MeasuredGunMount::new(
            gun_id(GUN),
            known(
                DamageNodeKey::new("designed_mount").expect("the mount key is valid"),
                provenance,
            ),
            known(DeclaredGunMountKind::WingRight, observed()),
            known(node_id("lancaster.wing_right.1"), observed()),
        );
        assert_eq!(
            measured.unmeasured(),
            vec![(
                GunMountField::Mount,
                cs_content::weapons::UnmeasuredCause::Unobserved { class }
            )],
            "a {class} mount is not a measurement"
        );
        match bind_gun_mount(&record, &measured) {
            Err(GunMountRefusal::Unmeasured { field, .. }) => {
                assert_eq!(field, GunMountField::Mount)
            }
            other => panic!("a {class} mount must be refused, got {other:?}"),
        }
    }
    // The two observation classes are what the gate accepts.
    assert!(is_observed_evidence(ClaimStatus::ObservedTool));
    assert!(is_observed_evidence(ClaimStatus::VerifiedOriginal));
}

/// **A mount measured for one gun cannot be bound to another gun's record**, in
/// either direction.
#[test]
fn accept_f27_e_1_a_mount_measured_for_another_gun_is_refused() {
    let record = unmeasured_gun(GUN);
    let measured = measured_mount("fixture.gun_beta");
    match bind_gun_mount(&record, &measured) {
        Err(GunMountRefusal::GunMismatch {
            measured: m,
            record: r,
        }) => {
            assert_eq!(m, gun_id("fixture.gun_beta"));
            assert_eq!(r, gun_id(GUN));
        }
        other => panic!("a mount for another gun must be refused, got {other:?}"),
    }
}

/// **A measurement binds the channel it carries and only that one**; the other
/// channel keeps its own unknown, with its own claim id and reason.
#[test]
fn accept_f27_e_1_a_measured_damage_binds_only_the_measured_channel() {
    let record = unmeasured_ammunition(AMMO);
    let measured = measured_damage_armor_only(AMMO);
    assert_eq!(
        measured.measured_channels(),
        vec![DeclaredDamageChannel::Armor]
    );
    assert_eq!(
        measured.unmeasured().len(),
        1,
        "the internal channel is the one this measurement leaves open"
    );

    let bound = bind_ammunition_damage(&record, &measured).expect("one measured channel binds");

    assert_eq!(bound.ammunition(), record.ammunition());
    assert_eq!(
        bound.known_damage(DeclaredDamageChannel::Armor),
        Some(6.5),
        "the measured amount is carried across verbatim"
    );
    match &bound.damage().armor {
        Resolved::Known(known) => assert!(is_observed_evidence(known.provenance.class)),
        Resolved::Unknown { .. } => panic!("the measured channel must stay known"),
    }
    assert_eq!(
        bound.known_damage(DeclaredDamageChannel::Internal),
        None,
        "a channel the measurement leaves open stays unknown; it is never scaled \
         from the measured one"
    );
    // The gate says nothing about the caliber or the interaction rules.
    assert_eq!(bound.caliber(), record.caliber());
    assert_eq!(bound.rules(), record.rules());
    assert_eq!(bound.origin(), record.origin());
    assert_eq!(bound.provenance(), record.provenance());
}

/// **A profile that carries no usable amount binds nothing**, whether the
/// channels are unknown or merely known with a non-observation provenance.
#[test]
fn accept_f27_e_1_a_damage_measurement_with_no_usable_amount_binds_nothing() {
    let record = unmeasured_ammunition(AMMO);

    let all_unknown = MeasuredAmmunitionDamage::new(
        ammunition_id(AMMO),
        DeclaredWeaponDamage {
            armor: unknown("f27.e1.test.armor", "nothing measured it"),
            internal: unknown("f27.e1.test.internal", "nothing measured it"),
        },
    );
    assert!(!all_unknown.is_measurable());
    assert_eq!(
        bind_ammunition_damage(&record, &all_unknown),
        Err(DamageBindingRefusal::NoMeasuredAmount {
            ammunition: ammunition_id(AMMO)
        })
    );

    let designed_profile = MeasuredAmmunitionDamage::new(
        ammunition_id(AMMO),
        DeclaredWeaponDamage {
            armor: known(1.0_f64, designed()),
            internal: known(1.0_f64, designed()),
        },
    );
    assert_eq!(
        designed_profile.measured_channels(),
        Vec::new(),
        "a designed table of two ones measures nothing"
    );
    assert_eq!(
        bind_ammunition_damage(&record, &designed_profile),
        Err(DamageBindingRefusal::NoMeasuredAmount {
            ammunition: ammunition_id(AMMO)
        })
    );
}

/// **A profile measured for one type cannot be bound to another's record**, so a
/// five-by-four multiplier table has nowhere to enter.
#[test]
fn accept_f27_e_1_damage_measured_for_another_type_is_refused() {
    let record = unmeasured_ammunition(AMMO);
    let other = MeasuredAmmunitionDamage::new(
        ammunition_id("fixture.ammo_beta"),
        DeclaredWeaponDamage {
            armor: known(30.0_f64, observed()),
            internal: known(12.0_f64, observed()),
        },
    );
    match bind_ammunition_damage(&record, &other) {
        Err(DamageBindingRefusal::TypeMismatch {
            measured,
            record: r,
        }) => {
            assert_eq!(measured, ammunition_id("fixture.ammo_beta"));
            assert_eq!(r, ammunition_id(AMMO));
        }
        other => panic!("another type's profile must be refused, got {other:?}"),
    }
}

/// **Binding is what the audit is waiting for**: an unmeasured type is reported
/// as consumed by nothing, and the same type bound by a measurement is not.
#[test]
fn accept_f27_e_1_a_bound_type_stops_the_audit_reporting_no_damage_consumer() {
    let record = unmeasured_ammunition(AMMO);
    let gun = unmeasured_gun(GUN);
    let loadout = DeclaredLoadout::try_new(
        ContentId::from_source(ContentKind::Loadout, "fixture.loadout_alpha")
            .expect("the loadout id is valid"),
        Origin::SyntheticFixture,
        vec![gun.gun().clone()],
        vec![record.ammunition().clone()],
        designed(),
    )
    .expect("the fixture loadout is valid");
    let surface = OriginalGunLoadout::try_new(
        Origin::SyntheticFixture,
        OriginalLoadoutCounts::try_new(
            ORIGINAL_AMMUNITION_TYPES,
            ORIGINAL_SELECTABLE_GUNS,
            ORIGINAL_GUN_SLOTS,
            ORIGINAL_ROCKET_SLOTS,
            ORIGINAL_HARDPOINT_POINTS,
        )
        .expect("every measured count is nonzero"),
        ORIGINAL_GUN_GROUPS.to_vec(),
        designed(),
    )
    .expect("the surface is internally consistent");

    let mut unmeasured_audit = AmmunitionAudit::new();
    unmeasured_audit.add_ammunition(record.clone());
    unmeasured_audit.add_gun(gun.clone());
    unmeasured_audit.add_loadout(loadout.clone());
    let before = unmeasured_audit.run(&surface);
    assert_eq!(
        before.findings_of("no_damage_consumer").len(),
        1,
        "a type with no measured amount is consumed by nothing"
    );

    let bound = bind_ammunition_damage(&record, &measured_damage_armor_only(AMMO))
        .expect("one measured channel binds");
    let mut bound_audit = AmmunitionAudit::new();
    bound_audit.add_ammunition(bound);
    bound_audit.add_gun(gun);
    bound_audit.add_loadout(loadout);
    let after = bound_audit.run(&surface);
    assert!(
        after.findings_of("no_damage_consumer").is_empty(),
        "a bound type has a damage consumer; findings: {:?}",
        after.findings()
    );
}

/// **The closure helpers count what a claim is not finished with**: a gun whose
/// mount measurement exists but is incomplete is as unmeasured as one nothing
/// measured, and a type nothing measured is listed by id.
#[test]
fn accept_f27_e_1_the_closure_helpers_count_incomplete_mounts_as_unmeasured() {
    let guns = vec![unmeasured_gun(GUN), unmeasured_gun("fixture.gun_beta")];
    // One complete measurement and one with a hole: only the complete one counts.
    let measured = vec![
        measured_mount(GUN),
        MeasuredGunMount::new(
            gun_id("fixture.gun_beta"),
            known(
                DamageNodeKey::new("wing_right_gun_1").expect("the mount key is valid"),
                observed(),
            ),
            unknown("f27.e1.test.mount_kind", "this group names no side"),
            known(node_id("peashooter.wing_right.1"), observed()),
        ),
    ];
    assert_eq!(
        unmeasured_gun_mounts(&guns, &measured),
        vec![gun_id("fixture.gun_beta")],
        "the complete measurement covers its gun; the one with a hole covers nothing"
    );

    // Blank the only complete one: now both are unmeasured.
    let incomplete = vec![
        mount_with(GunMountField::SceneBinding),
        MeasuredGunMount::new(
            gun_id("fixture.gun_beta"),
            known(
                DamageNodeKey::new("wing_right_gun_1").expect("the mount key is valid"),
                observed(),
            ),
            known(DeclaredGunMountKind::WingRight, observed()),
            unknown("f27.e1.test.scene_binding", "no mesh node was matched"),
        ),
    ];
    assert_eq!(
        unmeasured_gun_mounts(&guns, &incomplete),
        vec![gun_id(GUN), gun_id("fixture.gun_beta")],
        "a mount measurement with a hole covers nothing"
    );

    let types = vec![
        unmeasured_ammunition(AMMO),
        unmeasured_ammunition("fixture.ammo_beta"),
    ];
    assert_eq!(
        unmeasured_ammunition_types(&types, &[measured_damage_armor_only(AMMO)]),
        vec![ammunition_id("fixture.ammo_beta")]
    );
    assert_eq!(
        unmeasured_ammunition_types(&types, &[]),
        vec![ammunition_id(AMMO), ammunition_id("fixture.ammo_beta")]
    );

    // A profile that exists but measures nothing is the input
    // `bind_ammunition_damage` refuses, so counting it as coverage would let
    // four empty measurements report every type's damage as measured.
    let empty = MeasuredAmmunitionDamage::new(
        ammunition_id(AMMO),
        DeclaredWeaponDamage {
            armor: unknown(
                "f27.e1.test.armor",
                "this type's armor amount is unmeasured",
            ),
            internal: unknown(
                "f27.e1.test.internal",
                "this type's internal amount is unmeasured",
            ),
        },
    );
    assert!(!empty.is_measurable());
    assert_eq!(
        unmeasured_ammunition_types(&types, &[empty]),
        vec![ammunition_id(AMMO), ammunition_id("fixture.ammo_beta")],
        "an empty measurement covers no type"
    );
}

/// **A measured amount the schema rejects is refused, not bound**: a NaN or a
/// negative armor amount carries an observed provenance and still cannot become
/// a declared damage, because the reassembled record would not reassemble.
#[test]
fn accept_f27_e_1_a_measured_amount_the_schema_rejects_is_refused() {
    let record = unmeasured_ammunition(AMMO);
    for (amount, expected) in [
        (
            f64::NAN,
            WeaponSchemaError::NonFiniteDamage {
                channel: DeclaredDamageChannel::Armor,
            },
        ),
        (
            -1.0_f64,
            WeaponSchemaError::NegativeDamage {
                channel: DeclaredDamageChannel::Armor,
                amount: -1.0,
            },
        ),
    ] {
        let measured = MeasuredAmmunitionDamage::new(
            ammunition_id(AMMO),
            DeclaredWeaponDamage {
                armor: known(amount, observed()),
                internal: unknown("f27.e1.test.internal", "not measured"),
            },
        );
        assert_eq!(
            measured.measured_channels(),
            vec![DeclaredDamageChannel::Armor],
            "the amount is observed; the schema is what refuses it"
        );
        match bind_ammunition_damage(&record, &measured) {
            Err(DamageBindingRefusal::Assembly { source }) => assert_eq!(source, expected),
            other => panic!("a {amount} amount must be refused, got {other:?}"),
        }
        assert_eq!(
            record.known_damage(DeclaredDamageChannel::Armor),
            None,
            "the refused record is untouched"
        );
    }
}

/// **Binding replaces a scene binding the record already carries**: a bound mount
/// may not keep another gun's visual node beside a measured mount, which is why
/// the gate treats the binding as part of the mount rather than as an optional
/// extra.
#[test]
fn accept_f27_e_1_a_bound_mount_replaces_a_scene_binding_the_record_already_carried() {
    let stale = known(node_id("fixture.other_airframe.wing_left.9"), designed());
    let record = DeclaredGunDefinition::try_new(
        gun_id(GUN),
        Origin::SyntheticFixture,
        DamageNodeKey::new("placeholder_mount").expect("the placeholder mount key is valid"),
        DeclaredGunMountKind::Gondola,
        Some(stale.clone()),
        unknown("f27.e1.test.caliber", "unmeasured in this fixture"),
        unknown("f27.e1.test.ammunition", "unmeasured in this fixture"),
        unknown("f27.e1.test.rate", "unmeasured in this fixture"),
        unknown("f27.e1.test.velocity", "unmeasured in this fixture"),
        unknown("f27.e1.test.lifetime", "unmeasured in this fixture"),
        DeclaredSpreadCone {
            half_angle: unknown("f27.e1.test.spread", "unmeasured in this fixture"),
        },
        DeclaredWeaponDamage {
            armor: unknown("f27.e1.test.damage_armor", "unmeasured in this fixture"),
            internal: unknown("f27.e1.test.damage_internal", "unmeasured in this fixture"),
        },
        unknown("f27.e1.test.inheritance", "unmeasured in this fixture"),
        unknown("f27.e1.test.effect", "unmeasured in this fixture"),
        unknown("f27.e1.test.sound", "unmeasured in this fixture"),
        unmeasured_rules(),
        designed(),
    )
    .expect("the record with a designed binding is structurally valid");
    assert_eq!(record.scene_binding(), Some(&stale));

    let bound =
        bind_gun_mount(&record, &measured_mount(GUN)).expect("a complete measurement binds");
    assert_eq!(
        bound.scene_binding(),
        Some(measured_mount(GUN).scene_binding()),
        "the measured node replaces the designed one"
    );
}

/// **A partial measurement cannot record a claim as resolved**: the report keeps
/// it deferred, counts the shortfall and leaves it unaccounted until it is
/// re-filed.
#[test]
fn accept_f27_e_1_a_partial_measurement_cannot_resolve_a_claim() {
    let mut report = OriginalLimitReport::new();
    report
        .record(
            OriginalLimitClaim::AmmoNamesDamage,
            LimitEvidence::PartlyMeasured {
                provenance: observed(),
                unmeasured: 1,
            },
        )
        .expect("a partial measurement is recorded as a deferral");

    assert!(
        report.bound().is_empty(),
        "three measured types out of four do not resolve the claim"
    );
    assert_eq!(report.deferred(), vec![OriginalLimitClaim::AmmoNamesDamage]);
    assert_eq!(
        report.unaccounted(),
        OriginalLimitClaim::ALL.to_vec(),
        "a deferral with no destination is unaccounted, and so is every claim with no row"
    );
    assert!(!report.is_complete());
    let row = report
        .row(OriginalLimitClaim::AmmoNamesDamage)
        .expect("the claim has a row");
    assert_eq!(row.claim_id(), "f27.d.limit.ammo_names_damage");
    match row.outcome() {
        LimitOutcome::Deferred {
            reason,
            unmeasured,
            refiled_to,
        } => {
            assert_eq!(*unmeasured, 1);
            assert!(reason.contains("1 of the claim's subject"));
            assert!(reason.contains(MEASURED));
            assert_eq!(refiled_to, &None);
        }
        other => panic!("a partial measurement defers, got {other:?}"),
    }

    report
        .refile(OriginalLimitClaim::AmmoNamesDamage, "#547 follow-up")
        .expect("a deferred claim can be re-filed");
    assert_eq!(
        report.unaccounted(),
        vec![
            OriginalLimitClaim::GunGroupAssignment,
            OriginalLimitClaim::Convergence,
            OriginalLimitClaim::Inheritance,
            OriginalLimitClaim::InteractionRules,
        ],
        "re-filing one claim accounts for that claim and nothing else"
    );
    assert!(!report.is_complete());
    assert!(report.bound().is_empty());
    assert!(
        report
            .row(OriginalLimitClaim::AmmoNamesDamage)
            .expect("the claim has a row")
            .outcome()
            .is_refiled()
    );
}

/// **Every `f27.d.limit.*` claim is accounted for or the report says it is
/// not**: an empty report leaves all five unaccounted, and a report that
/// defers and re-files all five is complete while resolving none of them.
#[test]
fn accept_f27_e_1_every_f27_d_limit_claim_is_accounted_or_the_report_is_incomplete() {
    let mut report = OriginalLimitReport::new();
    assert_eq!(report.unaccounted(), OriginalLimitClaim::ALL.to_vec());
    assert!(!report.is_complete());
    assert!(report.rows().is_empty());

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
    assert_eq!(
        report.unaccounted(),
        OriginalLimitClaim::ALL.to_vec(),
        "recording a reason is not accounting for the claim"
    );
    for claim in OriginalLimitClaim::ALL {
        report
            .refile(claim, "#547: re-filed with the F27-E.1 finding")
            .unwrap_or_else(|error| panic!("{claim} must be re-fileable: {error}"));
    }

    assert!(report.is_complete());
    assert_eq!(report.rows().len(), OriginalLimitClaim::ALL.len());
    assert!(
        report.bound().is_empty(),
        "five re-filed deferrals resolve nothing"
    );
    assert_eq!(report.deferred(), OriginalLimitClaim::ALL.to_vec());
    for row in report.rows() {
        assert!(row.claim_id().starts_with("f27.d.limit."));
        match row.outcome() {
            LimitOutcome::Deferred {
                reason,
                unmeasured,
                refiled_to,
            } => {
                assert!(!reason.is_empty(), "{} needs a reason", row.claim_id());
                assert_eq!(*unmeasured, 0);
                assert_eq!(
                    refiled_to.as_deref(),
                    Some("#547: re-filed with the F27-E.1 finding")
                );
            }
            other => panic!("{} must stay deferred, got {other:?}", row.claim_id()),
        }
    }

    // One measurement resolves exactly one claim, and the other four stay
    // deferred — which is what a partial resolution looks like.
    report
        .record(
            OriginalLimitClaim::GunGroupAssignment,
            LimitEvidence::Measured {
                provenance: observed(),
            },
        )
        .expect("an observed measurement resolves the claim it is recorded for");
    assert_eq!(report.bound(), vec![OriginalLimitClaim::GunGroupAssignment]);
    assert_eq!(report.deferred().len(), 4);
    assert!(report.is_complete());
}

/// **Re-filing is refused where it would be meaningless**: an unresolved claim
/// with no row, a resolved claim, and an empty destination.
#[test]
fn accept_f27_e_1_a_refiling_refuses_a_bound_claim_and_an_empty_target() {
    let mut report = OriginalLimitReport::new();
    assert_eq!(
        report.refile(OriginalLimitClaim::Convergence, "#547"),
        Err(LimitReportError::NotRecorded(
            OriginalLimitClaim::Convergence
        ))
    );

    report
        .record(
            OriginalLimitClaim::Convergence,
            LimitEvidence::Measured {
                provenance: observed(),
            },
        )
        .expect("an observed measurement resolves the claim");
    assert_eq!(
        report.refile(OriginalLimitClaim::Convergence, "#547"),
        Err(LimitReportError::AlreadyBound(
            OriginalLimitClaim::Convergence
        ))
    );

    report
        .record(
            OriginalLimitClaim::Inheritance,
            LimitEvidence::Unmeasurable {
                reason: OriginalLimitClaim::Inheritance.deferral_reason().to_owned(),
            },
        )
        .expect("a deferral is recorded");
    assert_eq!(
        report.refile(OriginalLimitClaim::Inheritance, "  "),
        Err(LimitReportError::EmptyTarget(
            OriginalLimitClaim::Inheritance
        ))
    );
    assert!(
        report
            .unaccounted()
            .contains(&OriginalLimitClaim::Inheritance),
        "a refused re-filing does not account for the claim"
    );
    report
        .refile(OriginalLimitClaim::Inheritance, "  #358 capture  ")
        .expect("a nonempty target is accepted");
    match report
        .row(OriginalLimitClaim::Inheritance)
        .expect("the claim has a row")
        .outcome()
    {
        LimitOutcome::Deferred { refiled_to, .. } => {
            assert_eq!(refiled_to.as_deref(), Some("#358 capture"))
        }
        other => panic!("the claim must stay deferred, got {other:?}"),
    }
}

/// **A claim cannot be recorded as resolved on evidence that is not an
/// observation.** Otherwise this project's own designed fixture could report
/// that the original's damage amounts, mount assignment, convergence, inherited
/// velocity and interaction rules were all measured, and the machine-readable
/// report would say so with an empty list of unknowns.
#[test]
fn accept_f27_e_1_a_claim_is_not_resolved_on_an_unobserved_provenance() {
    let mut report = OriginalLimitReport::new();
    for class in [
        ClaimStatus::Designed,
        ClaimStatus::Documented,
        ClaimStatus::Inferred,
        ClaimStatus::Unknown,
        ClaimStatus::Contradicted,
    ] {
        let provenance = Provenance::new(claim(DESIGNED), class, None)
            .unwrap_or_else(|_| Provenance::designed(claim(DESIGNED)));
        assert_eq!(
            report.record(
                OriginalLimitClaim::AmmoNamesDamage,
                LimitEvidence::Measured { provenance }
            ),
            Err(LimitReportError::Unobserved {
                claim: OriginalLimitClaim::AmmoNamesDamage,
                class,
            }),
            "a {class} provenance resolves nothing"
        );
        assert_eq!(
            report.row(OriginalLimitClaim::AmmoNamesDamage),
            None,
            "the refused recording leaves the claim exactly as open as it was"
        );
        assert!(report.bound().is_empty(), "no {class} claim is ever bound");
        assert_eq!(report.unaccounted(), OriginalLimitClaim::ALL.to_vec());
        assert!(!report.is_complete());
    }
    // An observation resolves the claim. (`VerifiedOriginal` is the other
    // observation class and is asserted as such in
    // `a_known_field_that_is_not_an_observation_is_refused`; building one here
    // would need a source span, which belongs to a retail measurement.)
    report
        .record(
            OriginalLimitClaim::AmmoNamesDamage,
            LimitEvidence::Measured {
                provenance: observed(),
            },
        )
        .unwrap_or_else(|error| panic!("an observed measurement must resolve a claim: {error}"));
    assert_eq!(report.bound(), vec![OriginalLimitClaim::AmmoNamesDamage]);
}

/// **The claim ids are traceable to F27-D's own report**: four of the five are
/// F27-D's id verbatim, and the damage-amount claim is the narrowed remainder of
/// F27-D's `f27.d.limit.ammo_names`, which is stated rather than papered over.
#[test]
fn accept_f27_e_1_every_tracked_claim_carries_f27_ds_own_id() {
    let ids: Vec<&str> = OriginalLimitClaim::ALL
        .iter()
        .map(|claim| claim.claim_id())
        .collect();
    assert_eq!(
        ids,
        vec![
            "f27.d.limit.ammo_names_damage",
            "f27.d.limit.gun_group_assignment",
            "f27.d.limit.convergence",
            "f27.d.limit.inheritance",
            "f27.d.limit.interaction_rules",
        ]
    );
    // F27-D's own report spells four of the five identically; the fifth is the
    // damage-amount remainder of its `f27.d.limit.ammo_names`.
    let f27_d: Vec<&str> = OriginalLimitClaim::ALL
        .iter()
        .map(|claim| claim.f27_d_claim_id())
        .collect();
    assert_eq!(
        f27_d,
        vec![
            "f27.d.limit.ammo_names",
            "f27.d.limit.gun_group_assignment",
            "f27.d.limit.convergence",
            "f27.d.limit.inheritance",
            "f27.d.limit.interaction_rules",
        ]
    );
    assert_eq!(
        OriginalLimitClaim::ALL
            .iter()
            .filter(|claim| claim.claim_id() != claim.f27_d_claim_id())
            .count(),
        1,
        "exactly one claim is tracked under a narrower id than F27-D recorded"
    );
    // F27-D's sixth claim, the set of five guns, is measured by F27-E from the
    // shipped `IDS_GUNLONGNAME` rows, so this stage does not carry it; nothing
    // here may claim to resolve it.
    for claim in OriginalLimitClaim::ALL {
        assert_ne!(claim.f27_d_claim_id(), "f27.d.limit.gun_set");
    }
    for claim in OriginalLimitClaim::ALL {
        assert!(!claim.subject().is_empty());
        assert!(
            claim.deferral_reason().len() > 40,
            "{claim}'s reason has to say why it cannot be measured"
        );
        assert_eq!(claim.to_string(), claim.claim_id());
    }
    assert_eq!(
        MeasuredAmmunitionDamage::new(
            ammunition_id(AMMO),
            DeclaredWeaponDamage {
                armor: known(0.0_f64, observed()),
                internal: unknown("f27.e1.test.internal", "not measured"),
            },
        )
        .measured_channels(),
        vec![DeclaredDamageChannel::Armor],
        "a measured zero is a measurement, not a gap"
    );
    // The designed values a `Known` may carry elsewhere are still not
    // observations.
    assert!(!is_observed_evidence(ClaimStatus::Designed));
}
