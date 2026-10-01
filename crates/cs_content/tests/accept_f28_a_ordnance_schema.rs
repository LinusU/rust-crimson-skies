//! Acceptance scenario F28-A (content half): the declared ordnance schema
//! validates its identity, keeps unknowns explicit, and refuses a record
//! whose family and details disagree.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-A`. Task test prefix: `accept_f28_a_`.
//!
//! These tests drive production code only: `cs_content::ordnance`'s
//! [`DeclaredOrdnance::try_new`] and the `declared_synthetic_*` fixtures.
//! Removing the validation, replacing an unknown with a default, or letting a
//! family and its details disagree fails a test or fails to compile.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use std::collections::BTreeSet;

use cs_content::ordnance::{
    DECLARED_SYNTHETIC_AREA_DENIAL_KEY, DECLARED_SYNTHETIC_DIRECT_KEY, DECLARED_SYNTHETIC_FLAK_KEY,
    DECLARED_SYNTHETIC_GUIDED_KEY, DECLARED_SYNTHETIC_NITRO_KEY, DECLARED_SYNTHETIC_TORPEDO_KEY,
    DeclaredAreaEffect, DeclaredEquipmentRules, DeclaredFuseRule, DeclaredGuidanceRule,
    DeclaredHardpointKind, DeclaredInheritanceRule, DeclaredNitro, DeclaredNitroActivationRule,
    DeclaredNitroParameters, DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily,
    DeclaredProjectile, DeclaredProximityFuse, DeclaredStatusEffect, DeclaredStatusEffectKind,
    OrdnanceSchemaError, declared_known, declared_synthetic_area_denial, declared_synthetic_direct,
    declared_synthetic_flak, declared_synthetic_guided, declared_synthetic_media,
    declared_synthetic_nitro, declared_synthetic_provenance, declared_synthetic_torpedo,
    declared_unknown,
};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim() -> ClaimId {
    ClaimId::new("f28a.test-schema").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(cs_types::content::Known::new(
        value,
        Provenance::designed(claim()),
    ))
}

/// The value of a resolved record, for a field the fixture resolves.
fn resolved_value<T: Clone + PartialEq + std::fmt::Debug>(resolved: &Resolved<T>) -> T {
    resolved
        .clone()
        .known()
        .expect("the fixture resolves this field")
}

fn unknown<T>() -> Resolved<T> {
    Resolved::unknown(claim(), "the original value is unmeasured").expect("a nonempty reason")
}

fn weapon_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Weapon, key).expect("a valid weapon id")
}

fn declared_projectile() -> DeclaredProjectile {
    match declared_synthetic_flak().details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the flak fixture is a projectile"),
    }
}

/// The declared record must name a `weapon` id; anything else is refused
/// rather than silently coerced.
#[test]
fn accept_f28_a_the_declared_schema_requires_the_weapon_namespace() {
    let projectile = declared_projectile();
    let refusal = DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Gun, "synthetic.fixture_direct_explosive")
            .expect("a valid gun id"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::DirectExplosive,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    );
    assert!(
        matches!(
            refusal,
            Err(OrdnanceSchemaError::OrdnanceKindMismatch { .. })
        ),
        "a gun-namespace id is not an ordnance component: {refusal:?}"
    );
}

/// A family that disagrees with its details is refused: a booster family
/// carrying projectile details, and a projectile family carrying booster
/// details. This is the first of the family-coherence refusals F28
/// non-negotiable 1 needs.
#[test]
fn accept_f28_a_a_family_must_agree_with_its_declared_details() {
    let booster = match declared_synthetic_nitro().details() {
        DeclaredOrdnanceDetails::Nitro(nitro) => (**nitro).clone(),
        DeclaredOrdnanceDetails::Projectile(_) => panic!("the nitro fixture is a booster"),
    };
    let refusal = DeclaredOrdnance::try_new(
        weapon_id("synthetic.fixture_mislabelled"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::NitroBooster,
        DeclaredOrdnanceDetails::Projectile(Box::new(declared_projectile())),
        None,
        declared_synthetic_provenance(),
    );
    assert!(
        matches!(
            refusal,
            Err(OrdnanceSchemaError::FamilyDetailsMismatch {
                family: DeclaredOrdnanceFamily::NitroBooster
            })
        ),
        "a booster family may not carry a fuse, a lifetime and a blast: {refusal:?}"
    );

    let refusal = DeclaredOrdnance::try_new(
        weapon_id("synthetic.fixture_mislabelled"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::DirectExplosive,
        DeclaredOrdnanceDetails::Nitro(Box::new(booster)),
        None,
        declared_synthetic_provenance(),
    );
    assert!(
        matches!(
            refusal,
            Err(OrdnanceSchemaError::FamilyDetailsMismatch { .. })
        ),
        "a projectile family may not carry booster details: {refusal:?}"
    );
}

/// An unresolved field is a legal declared record. Nothing in the schema
/// repairs it into a plausible number — that refusal belongs to the
/// lowering boundary.
#[test]
fn accept_f28_a_an_unknown_field_stays_unknown_in_the_declared_record() {
    let mut projectile = declared_projectile();
    projectile.fuse = DeclaredFuseRule::Proximity(DeclaredProximityFuse {
        trigger_radius_m: unknown(),
    });

    let declared = DeclaredOrdnance::try_new(
        weapon_id("synthetic.fixture_unmeasured_radius"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::ProximityFlak,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("an unresolved field is a legal record");

    let DeclaredOrdnanceDetails::Projectile(stored) = declared.details() else {
        panic!("the record is a projectile");
    };
    let DeclaredFuseRule::Proximity(fuse) = &stored.fuse else {
        panic!("the record declares a proximity fuse");
    };
    assert!(
        !fuse.trigger_radius_m.is_known(),
        "the trigger radius is still unknown, not defaulted"
    );
    match &fuse.trigger_radius_m {
        Resolved::Unknown { claim_id, reason } => {
            assert_eq!(claim_id, &claim());
            assert!(!reason.is_empty(), "an unknown carries a reason");
        }
        Resolved::Known(_) => panic!("an unknown must not be repaired"),
    }
}

/// The same rule for the lost-target behavior: F28 non-negotiable 4 requires
/// it to be *specified*, so an unresolved one is preserved rather than
/// defaulted to coast or detonate.
#[test]
fn accept_f28_a_an_unresolved_lost_target_behavior_is_preserved() {
    let mut projectile = declared_projectile();
    projectile.guidance = DeclaredGuidanceRule::Targeted {
        lost_target: unknown(),
    };

    let declared = DeclaredOrdnance::try_new(
        weapon_id("synthetic.fixture_unmeasured_lost_target"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::GuidedRocket,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("an unresolved lost-target behavior is a legal record");

    let DeclaredOrdnanceDetails::Projectile(stored) = declared.details() else {
        panic!("the record is a projectile");
    };
    let DeclaredGuidanceRule::Targeted { lost_target } = &stored.guidance else {
        panic!("the record declares a targeted rule");
    };
    assert!(
        matches!(lost_target, Resolved::Unknown { .. }),
        "the lost-target behavior is unknown, not defaulted: {lost_target:?}"
    );
}

/// The known-value sanity checks cover every load-bearing number, so a
/// corrupt known value is refused instead of reaching the boundary.
#[test]
fn accept_f28_a_known_values_are_sanity_checked() {
    // Each corrupt case below names the field it corrupts, so a refusal
    // that stops mentioning the field is a failure a reviewer can see.

    // A corrupt known trigger radius.
    let mut projectile = declared_projectile();
    projectile.fuse = DeclaredFuseRule::Proximity(DeclaredProximityFuse {
        trigger_radius_m: known(0.0),
    });
    assert!(
        matches!(
            DeclaredOrdnance::try_new(
                weapon_id("synthetic.fixture_zero_radius"),
                Origin::SyntheticFixture,
                DeclaredOrdnanceFamily::ProximityFlak,
                DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
                None,
                declared_synthetic_provenance(),
            ),
            Err(OrdnanceSchemaError::NonPositiveTriggerRadius { .. })
        ),
        "a zero trigger radius would be contact, which is the Impact fuse's job"
    );

    // A corrupt known launch speed.
    let mut projectile = declared_projectile();
    projectile.launch.launch_speed_mps = known(f64::NAN);
    assert!(
        matches!(
            DeclaredOrdnance::try_new(
                weapon_id("synthetic.fixture_nan_speed"),
                Origin::SyntheticFixture,
                DeclaredOrdnanceFamily::DirectExplosive,
                DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
                None,
                declared_synthetic_provenance(),
            ),
            Err(OrdnanceSchemaError::NonFiniteLaunchSpeed)
        ),
        "a non-finite launch speed is refused"
    );

    // A zero stack capacity: a launcher that can never be loaded.
    let mut projectile = declared_projectile();
    projectile.stack.capacity_units = known(0);
    assert!(
        matches!(
            DeclaredOrdnance::try_new(
                weapon_id("synthetic.fixture_zero_capacity"),
                Origin::SyntheticFixture,
                DeclaredOrdnanceFamily::DirectExplosive,
                DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
                None,
                declared_synthetic_provenance(),
            ),
            Err(OrdnanceSchemaError::ZeroStackCapacity)
        ),
        "a zero-capacity launcher is refused"
    );

    // A zero status duration: an instant effect is a hit, not a status
    // effect.
    let mut projectile = declared_projectile();
    projectile.status = vec![DeclaredStatusEffect {
        kind: DeclaredStatusEffectKind::Choke,
        duration_ticks: known(0),
        strength: known(0.4),
    }];
    assert!(
        matches!(
            DeclaredOrdnance::try_new(
                weapon_id("synthetic.fixture_instant_status"),
                Origin::SyntheticFixture,
                DeclaredOrdnanceFamily::DirectExplosive,
                DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
                None,
                declared_synthetic_provenance(),
            ),
            Err(OrdnanceSchemaError::ZeroStatusDuration)
        ),
        "an untimed status effect is refused"
    );

    // A zero area lifetime: an unbounded area.
    let mut projectile = declared_projectile();
    projectile.area_effect = Some(DeclaredAreaEffect {
        radius_m: known(50.0),
        lifetime_ticks: known(0),
    });
    assert!(
        matches!(
            DeclaredOrdnance::try_new(
                weapon_id("synthetic.fixture_unbounded_area"),
                Origin::SyntheticFixture,
                DeclaredOrdnanceFamily::DirectExplosive,
                DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
                None,
                declared_synthetic_provenance(),
            ),
            Err(OrdnanceSchemaError::ZeroAreaLifetime)
        ),
        "an unbounded area is refused"
    );
}

/// A declared media id in the wrong catalog namespace is refused.
#[test]
fn accept_f28_a_declared_media_must_name_the_right_namespaces() {
    let mut projectile = declared_projectile();
    projectile.media.sound = known(
        ContentId::from_source(
            ContentKind::HardpointEquipment,
            "synthetic.fixture_not_a_sound",
        )
        .expect("a valid id"),
    );
    assert!(
        matches!(
            DeclaredOrdnance::try_new(
                weapon_id("synthetic.fixture_wrong_sound"),
                Origin::SyntheticFixture,
                DeclaredOrdnanceFamily::ProximityFlak,
                DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
                None,
                declared_synthetic_provenance(),
            ),
            Err(OrdnanceSchemaError::MediaKindMismatch { .. })
        ),
        "a sound must name the sound namespace"
    );
}

/// The nitro booster's numbers are each checked: a booster that can never
/// run, or that removes thrust, is a different component rather than a
/// weaker one.
#[test]
fn accept_f28_a_declared_nitro_numbers_are_each_checked() {
    let parameters = |capacity: Resolved<f64>,
                      consumption: Resolved<f64>,
                      thrust: Resolved<f64>,
                      authority: Resolved<f64>| {
        DeclaredNitroParameters {
            capacity_units: capacity,
            consumption_per_s: consumption,
            recovery_per_s: known(1.0),
            extra_thrust_n: thrust,
            activation: known(DeclaredNitroActivationRule::WhileHeld),
            authority_multiplier: authority,
        }
    };
    let build = |parameters: DeclaredNitroParameters| {
        DeclaredOrdnance::try_new(
            weapon_id("synthetic.fixture_bad_nitro"),
            Origin::SyntheticFixture,
            DeclaredOrdnanceFamily::NitroBooster,
            DeclaredOrdnanceDetails::Nitro(Box::new(DeclaredNitro {
                parameters,
                media: declared_synthetic_media(),
                equipment_rules: DeclaredEquipmentRules::default(),
            })),
            None,
            declared_synthetic_provenance(),
        )
    };

    assert!(
        build(parameters(
            known(0.0),
            known(3.0),
            known(4200.0),
            known(1.0)
        ))
        .is_err(),
        "a booster with no capacity could never be accepted"
    );
    assert!(
        build(parameters(
            known(12.0),
            known(0.0),
            known(4200.0),
            known(1.0)
        ))
        .is_err(),
        "a booster whose capacity never runs down is not a capacity"
    );
    assert!(
        build(parameters(known(12.0), known(3.0), known(-1.0), known(1.0))).is_err(),
        "a booster that removes thrust is not a booster"
    );
    assert!(
        build(parameters(
            known(12.0),
            known(3.0),
            known(4200.0),
            known(0.0)
        ))
        .is_err(),
        "a zero authority multiplier is an unflyable airframe, not a tradeoff"
    );
    assert!(
        build(parameters(
            known(12.0),
            known(3.0),
            known(4200.0),
            known(1.5)
        ))
        .is_err(),
        "an authority multiplier above 1 would be a bonus nobody measured"
    );
    assert!(
        build(parameters(unknown(), known(3.0), known(4200.0), known(1.0))).is_ok(),
        "an unresolved capacity is a legal record; the boundary refuses it"
    );
}

/// The nitro fixture declares no invented tradeoff, and an unresolved
/// tradeoff is preserved rather than defaulted to "no penalty".
#[test]
fn accept_f28_a_the_declared_nitro_tradeoff_is_not_invented() {
    let declared = declared_synthetic_nitro();
    let DeclaredOrdnanceDetails::Nitro(nitro) = declared.details() else {
        panic!("the nitro fixture is a booster");
    };
    assert_eq!(
        resolved_value(&nitro.parameters.authority_multiplier),
        1.0,
        "the fixture declares no authority penalty"
    );
    match &nitro.parameters.authority_multiplier {
        Resolved::Known(known) => assert_eq!(
            known.provenance.class,
            cs_types::evidence::ClaimStatus::Designed,
            "the declared value is designed, never original"
        ),
        Resolved::Unknown { .. } => panic!("the fixture's tradeoff must be resolved"),
    }

    let parameters = DeclaredNitroParameters {
        authority_multiplier: unknown(),
        ..nitro.parameters.clone()
    };
    assert!(
        matches!(parameters.authority_multiplier, Resolved::Unknown { .. }),
        "an unmeasured tradeoff stays unmeasured"
    );
}

/// Every declared field of a projectile is a `Resolved`: none of them is a
/// bare value that a consumer could read as a measured one.
#[test]
fn accept_f28_a_every_declared_projectile_field_is_resolved() {
    let projectile = declared_projectile();
    assert!(
        projectile.launch.hardpoint.is_known()
            && projectile.launch.launch_speed_mps.is_known()
            && projectile.launch.inheritance.is_known()
            && projectile.launch.release_delay_ticks.is_known()
            && projectile.stack.capacity_units.is_known()
            && projectile.stack.unit_mass_kg.is_known()
            && projectile.lifetime_ticks.is_known()
            && projectile.armor_damage.is_known()
            && projectile.internal_damage.is_known(),
        "the fixture resolves every scalar the deliverable names"
    );
    assert_eq!(
        resolved_value(&projectile.launch.hardpoint),
        DeclaredHardpointKind::WingLeft,
        "the fixture declares a hardpoint kind"
    );
    assert_eq!(
        resolved_value(&projectile.launch.inheritance),
        DeclaredInheritanceRule::None,
        "the fixture inherits none of the launcher's velocity, as a declared resolved option"
    );
}

/// The synthetic fixture set covers all six families with six distinct ids
/// and five different declared behaviors.
#[test]
fn accept_f28_a_the_declared_fixtures_cover_every_family() {
    let fixtures = [
        declared_synthetic_direct(),
        declared_synthetic_flak(),
        declared_synthetic_guided(),
        declared_synthetic_area_denial(),
        declared_synthetic_torpedo(),
        declared_synthetic_nitro(),
    ];
    assert_eq!(fixtures.len(), 6);
    let ids: BTreeSet<&str> = fixtures
        .iter()
        .map(|fixture| fixture.ordnance().as_str())
        .collect();
    assert_eq!(ids.len(), 6, "six distinct catalog ids: {ids:?}");
    let families: BTreeSet<DeclaredOrdnanceFamily> =
        fixtures.iter().map(DeclaredOrdnance::family).collect();
    assert_eq!(families.len(), 6, "six distinct families: {families:?}");
    for family in DeclaredOrdnanceFamily::ALL {
        assert!(
            families.contains(family),
            "the fixture set covers {family}: {families:?}"
        );
    }

    // Every fixture carries the synthetic origin and designed provenance,
    // so no declared record can be mistaken for observed original data.
    for fixture in &fixtures {
        assert_eq!(
            fixture.origin(),
            &Origin::SyntheticFixture,
            "every fixture is synthetic: {}",
            fixture.ordnance()
        );
        assert_eq!(
            fixture.provenance().class,
            cs_types::evidence::ClaimStatus::Designed,
            "every fixture's provenance is designed, never original"
        );
    }
    assert_eq!(
        fixtures[0].ordnance().as_str(),
        format!("weapon/{DECLARED_SYNTHETIC_DIRECT_KEY}"),
    );
    assert_eq!(
        fixtures[1].ordnance().as_str(),
        format!("weapon/{DECLARED_SYNTHETIC_FLAK_KEY}"),
    );
    assert_eq!(
        fixtures[2].ordnance().as_str(),
        format!("weapon/{DECLARED_SYNTHETIC_GUIDED_KEY}"),
    );
    assert_eq!(
        fixtures[3].ordnance().as_str(),
        format!("weapon/{DECLARED_SYNTHETIC_AREA_DENIAL_KEY}"),
    );
    assert_eq!(
        fixtures[4].ordnance().as_str(),
        format!("weapon/{DECLARED_SYNTHETIC_TORPEDO_KEY}"),
    );
    assert_eq!(
        fixtures[5].ordnance().as_str(),
        format!("weapon/{DECLARED_SYNTHETIC_NITRO_KEY}"),
    );
}

/// The booster has no projectile fields at all — no fuse, no lifetime, no
/// blast — so a declared record cannot imply a detonation nobody measured.
#[test]
fn accept_f28_a_the_declared_booster_carries_no_fuse_or_lifetime() {
    assert!(declared_synthetic_nitro().projectile().is_none());
    assert!(declared_synthetic_nitro().nitro().is_some());
    assert!(
        declared_synthetic_flak().nitro().is_none(),
        "a projectile record carries no booster parameters"
    );
    assert!(declared_synthetic_flak().projectile().is_some());
}

/// The media record is presentation only and separate from the status
/// effects: a fixture may declare a particle and no gameplay field carries
/// it.
#[test]
fn accept_f28_a_declared_media_is_separate_from_the_status_effects() {
    let media = declared_synthetic_media();
    assert!(media.particles.is_some(), "the fixture declares a particle");
    let projectile = declared_projectile();
    assert!(
        projectile.status.is_empty(),
        "a proximity shell declares no status effect next to its particle"
    );
    let denial = match declared_synthetic_area_denial().details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the area-denial fixture is a projectile"),
    };
    assert_eq!(
        denial.status.len(),
        1,
        "the area-denial fixture's choke is a status effect, not a particle"
    );
    assert_eq!(denial.status[0].kind, DeclaredStatusEffectKind::Choke);
    assert!(
        denial.media.particles.is_some(),
        "and its visual particle is a separate declared resource"
    );
    for kind in DeclaredStatusEffectKind::ALL {
        assert!(!kind.label().is_empty());
    }
}

/// An explicit unknown with a stated reason is accepted; an empty reason is
/// refused, so an unknown can never be recorded without saying why.
#[test]
fn accept_f28_a_an_unknown_must_carry_a_reason() {
    assert!(Resolved::<f64>::unknown(claim(), "the original trigger radius is unmeasured").is_ok());
    assert!(
        Resolved::<f64>::unknown(claim(), "   ").is_err(),
        "a whitespace reason records nothing"
    );
    assert!(
        matches!(
            declared_unknown::<f64>("a stated reason"),
            Resolved::Unknown { .. }
        ),
        "the fixture's unknown helper records an unknown"
    );
}

/// The declared helpers produce known values carrying designed provenance
/// and unknowns carrying the fixture claim, so the fixture can never look
/// observed.
#[test]
fn accept_f28_a_the_declared_helpers_carry_designed_provenance() {
    let value = declared_known(7.0);
    match &value {
        Resolved::Known(known) => {
            assert_eq!(known.value, 7.0);
            assert_eq!(
                known.provenance.class,
                cs_types::evidence::ClaimStatus::Designed,
                "the fixture's known values are designed"
            );
            assert!(
                known.provenance.source.is_none(),
                "a designed value names no source span"
            );
        }
        Resolved::Unknown { .. } => panic!("declared_known resolves a value"),
    }
    match declared_unknown::<f64>("unmeasured") {
        Resolved::Unknown { claim_id, reason } => {
            assert_eq!(claim_id.as_str(), "f28.fixture.declared");
            assert_eq!(reason, "unmeasured");
        }
        Resolved::Known(_) => panic!("declared_unknown records an unknown"),
    }
}
