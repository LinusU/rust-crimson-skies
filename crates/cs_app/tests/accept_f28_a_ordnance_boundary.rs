//! Acceptance scenario F28-A (boundary half): the declared ordnance schema
//! lowers into the runtime registry, and every unresolved field is refused
//! by name rather than guessed.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-A`. Task test prefix: `accept_f28_a_`.
//!
//! These tests drive production code only: `cs_app::ordnance`'s
//! [`lower_ordnance`], [`lower_equipment_rules`] and
//! [`declared_scene_binding`], over the `cs_content::ordnance` declared
//! fixtures. Replacing a refusal with a default, dropping a declared field,
//! or smuggling the presentation binding into the runtime record fails a
//! test.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use std::collections::BTreeSet;

use cs_app::ordnance::{
    OrdnanceLauncherBinding, OrdnanceLowerError, declared_scene_binding, lower_equipment_rules,
    lower_ordnance,
};
use cs_content::damage::DamageNodeKey;
use cs_content::ordnance::{
    DeclaredAreaEffect, DeclaredArmingRule, DeclaredEquipmentRules, DeclaredFuseRule,
    DeclaredGuidanceRule, DeclaredHardpointKind, DeclaredInheritanceRule, DeclaredNitro,
    DeclaredNitroActivationRule, DeclaredNitroParameters, DeclaredOrdnance,
    DeclaredOrdnanceDetails, DeclaredOrdnanceFamily, DeclaredProximityFuse, DeclaredStatusEffect,
    DeclaredStatusEffectKind, declared_synthetic_area_denial, declared_synthetic_direct,
    declared_synthetic_flak, declared_synthetic_guided, declared_synthetic_nitro,
    declared_synthetic_provenance, declared_synthetic_torpedo,
};
use cs_sim::damage::{ActorId, DamageNodeKey as RuntimeDamageNodeKey};
use cs_sim::weapons::InheritanceRule;
use cs_sim::weapons::ordnance::{
    ArmingRule, CompatibilityVerdict, FuseRule, GuidanceRule, HardpointKind, LostTargetBehavior,
    NitroActivationRule, OrdnanceFamily, OrdnanceRegistry, OrdnanceRegistryError, StatusEffectKind,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim() -> ClaimId {
    ClaimId::new("f28a.test-boundary").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn unknown<T>() -> Resolved<T> {
    Resolved::unknown(claim(), "the original value is unmeasured").expect("a nonempty reason")
}

fn weapon_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Weapon, key).expect("a valid weapon id")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: 11,
        serial,
    }
}

/// The declared projectile half of a fixture, cloned so a test can mutate it.
fn projectile_of(declared: &DeclaredOrdnance) -> cs_content::ordnance::DeclaredProjectile {
    match declared.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the fixture is a projectile"),
    }
}

/// Rebuilds a declared record around a mutated projectile.
fn with_projectile(
    family: DeclaredOrdnanceFamily,
    key: &str,
    projectile: cs_content::ordnance::DeclaredProjectile,
) -> DeclaredOrdnance {
    DeclaredOrdnance::try_new(
        weapon_id(key),
        Origin::SyntheticFixture,
        family,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the mutated record is structurally valid")
}

/// Every declared fixture lowers into a runtime component that keeps every
/// declared field: the family, the arming rule, the fuse, the guidance rule,
/// the lifetime, the area, the damage channels, the status effects and the
/// equipment rules.
#[test]
fn accept_f28_a_every_declared_fixture_lowers_with_its_fields_intact() {
    let flak = lower_ordnance(&declared_synthetic_flak()).expect("the flak fixture lowers");
    let projectile = flak
        .as_projectile()
        .expect("the flak fixture is a projectile");
    assert_eq!(projectile.family(), OrdnanceFamily::ProximityFlak);
    assert_eq!(
        projectile.arming(),
        ArmingRule::AfterTicks(3),
        "the declared arming delay lowers"
    );
    assert!(
        matches!(projectile.fuse(), FuseRule::Proximity(_)),
        "the declared proximity fuse lowers: {:?}",
        projectile.fuse()
    );
    assert_eq!(
        projectile.guidance(),
        GuidanceRule::Unguided,
        "an unguided family lowers as unguided"
    );
    assert_eq!(
        projectile.launch().hardpoint(),
        HardpointKind::WingLeft,
        "the declared hardpoint kind lowers variant-wise"
    );
    assert_eq!(
        projectile.launch().inheritance(),
        InheritanceRule::None,
        "the declared inheritance rule lowers variant-wise"
    );
    assert!(projectile.area_effect().is_none());
    assert!(projectile.status().is_empty());

    let guided = lower_ordnance(&declared_synthetic_guided()).expect("the guided fixture lowers");
    let guided = guided
        .as_projectile()
        .expect("the guided fixture is a projectile");
    assert_eq!(guided.family(), OrdnanceFamily::GuidedRocket);
    assert_eq!(
        guided.arming(),
        ArmingRule::AfterTravelMetres(60.0),
        "the declared travel arming lowers"
    );
    assert_eq!(
        guided.guidance(),
        GuidanceRule::Targeted {
            lost_target: LostTargetBehavior::Detonate,
        },
        "the declared lost-target behavior lowers variant-wise"
    );

    let denial =
        lower_ordnance(&declared_synthetic_area_denial()).expect("the denial fixture lowers");
    let denial = denial
        .as_projectile()
        .expect("the denial fixture is a projectile");
    assert_eq!(denial.family(), OrdnanceFamily::AreaDenialEngine);
    let area = denial.area_effect().expect("the declared area lowers");
    assert_eq!(area.radius_m(), 55.0);
    assert_eq!(area.lifetime_ticks(), 90);
    assert_eq!(denial.status().len(), 1);
    assert_eq!(denial.status()[0].kind(), StatusEffectKind::Choke);
    assert_eq!(denial.status()[0].duration_ticks(), 60);
    assert!(
        area.lifetime_ticks() <= denial.lifetime_ticks(),
        "the lowered area is still bounded by the lowered lifetime"
    );

    let direct = lower_ordnance(&declared_synthetic_direct()).expect("the direct fixture lowers");
    let direct = direct
        .as_projectile()
        .expect("the direct fixture is a projectile");
    assert_eq!(direct.family(), OrdnanceFamily::DirectExplosive);
    assert_eq!(direct.fuse(), FuseRule::Impact);

    let torpedo =
        lower_ordnance(&declared_synthetic_torpedo()).expect("the torpedo fixture lowers");
    let torpedo = torpedo
        .as_projectile()
        .expect("the torpedo fixture is a projectile");
    assert_eq!(torpedo.family(), OrdnanceFamily::AerialTorpedo);
    assert_ne!(
        direct.fuse(),
        torpedo.fuse(),
        "the two families do not collapse into one component"
    );
}

/// A booster lowers as a booster and carries no fuse, lifetime or blast: the
/// runtime sum's second arm is reachable and distinct.
#[test]
fn accept_f28_a_a_declared_booster_lowers_without_a_fuse_or_lifetime() {
    let nitro = lower_ordnance(&declared_synthetic_nitro()).expect("the nitro fixture lowers");
    assert_eq!(nitro.family(), OrdnanceFamily::NitroBooster);
    assert!(
        nitro.as_projectile().is_none(),
        "a booster is not a launched item"
    );
    let booster = nitro.as_nitro().expect("the nitro fixture is a booster");
    assert_eq!(booster.parameters().capacity_units(), 12.0);
    assert_eq!(booster.parameters().consumption_per_s(), 3.0);
    assert_eq!(booster.parameters().recovery_per_s(), 1.0);
    assert_eq!(booster.parameters().extra_thrust_n(), 4200.0);
    assert_eq!(
        booster.parameters().activation(),
        NitroActivationRule::WhileHeld,
        "the declared activation rule lowers variant-wise"
    );
    assert!(
        booster.parameters().tradeoffs().is_unmeasured(),
        "the declared tradeoff lowers without being invented"
    );
}

/// Every unresolved declared field is refused **by name**, with its claim
/// and reason — one refusal per field, so a caller can see which parameter
/// was not measured.
#[test]
fn accept_f28_a_each_unresolved_field_is_refused_by_name() {
    let cases: Vec<(&'static str, DeclaredOrdnance)> = vec![
        ("fuse.proximity.trigger_radius_m", {
            let mut projectile = projectile_of(&declared_synthetic_flak());
            projectile.fuse = DeclaredFuseRule::Proximity(DeclaredProximityFuse {
                trigger_radius_m: unknown(),
            });
            with_projectile(
                DeclaredOrdnanceFamily::ProximityFlak,
                "synthetic.fixture_unmeasured_radius",
                projectile,
            )
        }),
        ("arming.after_ticks", {
            let mut projectile = projectile_of(&declared_synthetic_flak());
            projectile.arming = DeclaredArmingRule::AfterTicks(unknown());
            with_projectile(
                DeclaredOrdnanceFamily::ProximityFlak,
                "synthetic.fixture_unmeasured_arming",
                projectile,
            )
        }),
        ("guidance.lost_target", {
            let mut projectile = projectile_of(&declared_synthetic_guided());
            projectile.guidance = DeclaredGuidanceRule::Targeted {
                lost_target: unknown(),
            };
            with_projectile(
                DeclaredOrdnanceFamily::GuidedRocket,
                "synthetic.fixture_unmeasured_lost_target",
                projectile,
            )
        }),
        ("launch.launch_speed_mps", {
            let mut projectile = projectile_of(&declared_synthetic_direct());
            projectile.launch.launch_speed_mps = unknown();
            with_projectile(
                DeclaredOrdnanceFamily::DirectExplosive,
                "synthetic.fixture_unmeasured_speed",
                projectile,
            )
        }),
        ("stack.capacity_units", {
            let mut projectile = projectile_of(&declared_synthetic_direct());
            projectile.stack.capacity_units = unknown();
            with_projectile(
                DeclaredOrdnanceFamily::DirectExplosive,
                "synthetic.fixture_unmeasured_capacity",
                projectile,
            )
        }),
        ("lifetime_ticks", {
            let mut projectile = projectile_of(&declared_synthetic_direct());
            projectile.lifetime_ticks = unknown();
            with_projectile(
                DeclaredOrdnanceFamily::DirectExplosive,
                "synthetic.fixture_unmeasured_lifetime",
                projectile,
            )
        }),
        ("damage.armor", {
            let mut projectile = projectile_of(&declared_synthetic_direct());
            projectile.armor_damage = unknown();
            with_projectile(
                DeclaredOrdnanceFamily::DirectExplosive,
                "synthetic.fixture_unmeasured_armor_damage",
                projectile,
            )
        }),
        ("media.sound", {
            let mut projectile = projectile_of(&declared_synthetic_direct());
            projectile.media.sound = unknown();
            with_projectile(
                DeclaredOrdnanceFamily::DirectExplosive,
                "synthetic.fixture_unmeasured_sound",
                projectile,
            )
        }),
        ("area_effect.radius_m", {
            let mut projectile = projectile_of(&declared_synthetic_area_denial());
            projectile.area_effect = Some(DeclaredAreaEffect {
                radius_m: unknown(),
                lifetime_ticks: known(90),
            });
            with_projectile(
                DeclaredOrdnanceFamily::AreaDenialEngine,
                "synthetic.fixture_unmeasured_area",
                projectile,
            )
        }),
        ("status.duration_ticks", {
            let mut projectile = projectile_of(&declared_synthetic_area_denial());
            projectile.status = vec![DeclaredStatusEffect {
                kind: DeclaredStatusEffectKind::Choke,
                duration_ticks: unknown(),
                strength: known(0.4),
            }];
            with_projectile(
                DeclaredOrdnanceFamily::AreaDenialEngine,
                "synthetic.fixture_unmeasured_status",
                projectile,
            )
        }),
    ];

    for (field, declared) in cases {
        match lower_ordnance(&declared) {
            Err(OrdnanceLowerError::UnknownField {
                field: refused,
                claim_id,
                reason,
            }) => {
                assert_eq!(refused, field, "{field} must be refused by its own name");
                assert_eq!(claim_id, claim());
                assert!(!reason.is_empty(), "{field} must carry its reason");
            }
            other => panic!("{field} must refuse, got {other:?}"),
        }
    }
}

/// The nitro refusal names each unresolved number too — capacity,
/// consumption, recovery, thrust, activation rule and the tradeoff.
#[test]
fn accept_f28_a_each_unresolved_nitro_field_is_refused_by_name() {
    let declared_nitro = |parameters: DeclaredNitroParameters| {
        DeclaredOrdnance::try_new(
            weapon_id("synthetic.fixture_unmeasured_nitro"),
            Origin::SyntheticFixture,
            DeclaredOrdnanceFamily::NitroBooster,
            DeclaredOrdnanceDetails::Nitro(Box::new(DeclaredNitro {
                parameters,
                media: declared_synthetic_flak().media().clone(),
                equipment_rules: DeclaredEquipmentRules::default(),
            })),
            None,
            declared_synthetic_provenance(),
        )
        .expect("the record is structurally valid")
    };
    let base = match declared_synthetic_nitro().details() {
        DeclaredOrdnanceDetails::Nitro(nitro) => (**nitro).clone(),
        DeclaredOrdnanceDetails::Projectile(_) => panic!("the nitro fixture is a booster"),
    };

    let cases: Vec<(&'static str, DeclaredNitroParameters)> = vec![
        (
            "nitro.capacity_units",
            DeclaredNitroParameters {
                capacity_units: unknown(),
                ..base.parameters.clone()
            },
        ),
        (
            "nitro.consumption_per_s",
            DeclaredNitroParameters {
                consumption_per_s: unknown(),
                ..base.parameters.clone()
            },
        ),
        (
            "nitro.recovery_per_s",
            DeclaredNitroParameters {
                recovery_per_s: unknown(),
                ..base.parameters.clone()
            },
        ),
        (
            "nitro.extra_thrust_n",
            DeclaredNitroParameters {
                extra_thrust_n: unknown(),
                ..base.parameters.clone()
            },
        ),
        (
            "nitro.activation",
            DeclaredNitroParameters {
                activation: unknown(),
                ..base.parameters.clone()
            },
        ),
        (
            "nitro.authority_multiplier",
            DeclaredNitroParameters {
                authority_multiplier: unknown(),
                ..base.parameters.clone()
            },
        ),
    ];

    for (field, parameters) in cases {
        match lower_ordnance(&declared_nitro(parameters)) {
            Err(OrdnanceLowerError::UnknownField {
                field: refused,
                claim_id,
                reason,
            }) => {
                assert_eq!(refused, field, "{field} must be refused by its own name");
                assert_eq!(claim_id, claim());
                assert!(!reason.is_empty(), "{field} must carry its reason");
            }
            other => panic!("{field} must refuse, got {other:?}"),
        }
    }

    // The declared fixture's own activation rule lowers through its inner
    // value rather than through a blanket default.
    let fixed = DeclaredNitroParameters {
        activation: known(DeclaredNitroActivationRule::FixedTicks { ticks: unknown() }),
        ..base.parameters.clone()
    };
    assert!(
        matches!(
            lower_ordnance(&declared_nitro(fixed)),
            Err(OrdnanceLowerError::UnknownField {
                field: "nitro.activation.fixed_ticks",
                ..
            })
        ),
        "an unresolved burn length is refused by its own name"
    );
    let fixed = DeclaredNitroParameters {
        activation: known(DeclaredNitroActivationRule::FixedTicks { ticks: known(45) }),
        ..base.parameters.clone()
    };
    let lowered = lower_ordnance(&declared_nitro(fixed)).expect("the fixed burn lowers");
    assert_eq!(
        lowered
            .as_nitro()
            .expect("a booster")
            .parameters()
            .activation(),
        NitroActivationRule::FixedTicks { ticks: 45 },
        "a declared fixed burn lowers as declared"
    );
}

/// The equipment rules refuse each unresolved option independently: an
/// unmeasured requirement does not become "no requirement", and an
/// unmeasured prohibition does not become "nothing is forbidden".
#[test]
fn accept_f28_a_each_unresolved_equipment_option_is_refused_independently() {
    let rack = known(
        ContentId::from_source(ContentKind::HardpointEquipment, "synthetic.fixture_rack")
            .expect("a valid id"),
    );
    let ballast = known(
        ContentId::from_source(ContentKind::HardpointEquipment, "synthetic.fixture_ballast")
            .expect("a valid id"),
    );

    let rack_id = rack
        .clone()
        .known()
        .expect("the requirement is resolved")
        .clone();
    let ballast_id = ballast
        .clone()
        .known()
        .expect("the prohibition is resolved")
        .clone();
    let lowered = lower_equipment_rules(&DeclaredEquipmentRules {
        requires: Some(rack.clone()),
        forbids: vec![ballast.clone()],
    })
    .expect("resolved rules lower");
    assert_eq!(lowered.forbids().len(), 1, "the prohibition lowers");

    // The requirement is missing from the airframe, and the forbidden item is
    // present: the two are named apart.
    assert_eq!(
        lowered.check(&BTreeSet::from([ballast_id.clone()])),
        CompatibilityVerdict::MissingRequired {
            required: rack_id.clone()
        },
        "a missing requirement is named"
    );
    assert!(
        matches!(
            lowered.check(&BTreeSet::from([rack_id.clone(), ballast_id])),
            CompatibilityVerdict::Forbidden { .. }
        ),
        "a forbidden item is named"
    );

    // An unresolved requirement refuses; it never becomes "no requirement".
    assert!(
        matches!(
            lower_equipment_rules(&DeclaredEquipmentRules {
                requires: Some(unknown()),
                forbids: Vec::new(),
            }),
            Err(OrdnanceLowerError::UnknownField {
                field: "equipment_rules.requires",
                ..
            })
        ),
        "an unmeasured requirement refuses rather than becoming optional"
    );
    // An unresolved prohibition refuses; it never becomes "allowed".
    assert!(
        matches!(
            lower_equipment_rules(&DeclaredEquipmentRules {
                requires: None,
                forbids: vec![unknown()],
            }),
            Err(OrdnanceLowerError::UnknownField {
                field: "equipment_rules.forbids",
                ..
            })
        ),
        "an unmeasured prohibition refuses rather than becoming permitted"
    );
}

/// The declared launcher mount is the F29 weapon-mount node on both sides of
/// the boundary, so a destroyed weapon node disables the launcher that
/// actually stops firing.
#[test]
fn accept_f28_a_the_declared_and_runtime_launcher_mounts_are_the_same_key() {
    let declared = projectile_of(&declared_synthetic_flak());
    let lowered = lower_ordnance(&declared_synthetic_flak()).expect("the fixture lowers");
    let runtime_mount = &lowered
        .as_projectile()
        .expect("a projectile")
        .launch()
        .mount();
    assert_eq!(
        runtime_mount.as_str(),
        declared.launch.mount.as_str(),
        "the mount survives the boundary unchanged"
    );
    assert_ne!(
        runtime_mount.as_str(),
        lowered.ordnance().as_str(),
        "a mount and a component stay distinct identities"
    );
    // Both sides apply the same key grammar.
    let declared_key: &DamageNodeKey = &declared.launch.mount;
    assert_eq!(
        RuntimeDamageNodeKey::new(declared_key.as_str())
            .expect("the runtime accepts the declared grammar")
            .as_str(),
        declared_key.as_str(),
        "one grammar, one key"
    );
}

/// Every lowered fixture registers into the runtime registry, and the
/// registry's own import refusal is what stops an unsupported installation.
#[test]
fn accept_f28_a_lowered_fixtures_register_and_an_import_is_still_refused() {
    let fixtures = [
        declared_synthetic_direct(),
        declared_synthetic_flak(),
        declared_synthetic_guided(),
        declared_synthetic_area_denial(),
        declared_synthetic_torpedo(),
        declared_synthetic_nitro(),
    ];
    let mut registry = OrdnanceRegistry::new();
    for fixture in &fixtures {
        let lowered = lower_ordnance(fixture).expect("the fixture lowers");
        registry
            .register(lowered)
            .expect("each fixture id is distinct");
    }
    assert_eq!(registry.len(), 6, "all six families registered");

    let installed: Vec<cs_sim::weapons::ordnance::OrdnanceId> = fixtures
        .iter()
        .map(|fixture| {
            cs_sim::weapons::ordnance::OrdnanceId::try_new(fixture.ordnance().clone())
                .expect("a weapon-namespace id")
        })
        .collect();
    let verdicts = registry
        .resolve_installation(&installed, &BTreeSet::new())
        .expect("every installed component is known");
    assert_eq!(verdicts.len(), 6);
    assert!(
        verdicts
            .iter()
            .all(|verdict| verdict.compatibility.is_compatible()),
        "the fixture components need no extra equipment: {verdicts:?}"
    );

    // An import naming an id the registry does not have is refused whole.
    let unsupported = cs_sim::weapons::ordnance::OrdnanceId::try_new(
        ContentId::from_source(ContentKind::Weapon, "synthetic.fixture_imported_unknown")
            .expect("a valid id"),
    )
    .expect("a weapon-namespace id");
    let mut with_hole = installed.clone();
    with_hole.push(unsupported.clone());
    assert_eq!(
        registry.resolve_installation(&with_hole, &BTreeSet::new()),
        Err(OrdnanceRegistryError::UnknownOrdnance {
            ordnance: unsupported
        }),
        "an unsupported custom plane cannot bypass the shop through an import"
    );
}

/// The scene binding stays on the declared record: the runtime component
/// carries no presentation reference, so gameplay state cannot depend on one.
#[test]
fn accept_f28_a_the_presentation_binding_stays_on_the_declared_record() {
    assert!(
        declared_scene_binding(&declared_synthetic_flak()).is_none(),
        "the fixture declares no scene binding"
    );

    let projectile = projectile_of(&declared_synthetic_flak());
    let scene = cs_content::scene::SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, "synthetic.fixture_launcher_pylon")
            .expect("a valid id"),
    )
    .expect("a scene-node id");
    let declared = DeclaredOrdnance::try_new(
        weapon_id("synthetic.fixture_bound_flak"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::ProximityFlak,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile.clone())),
        Some(known(scene.clone())),
        declared_synthetic_provenance(),
    )
    .expect("the bound record is structurally valid");

    assert_eq!(
        declared_scene_binding(&declared).and_then(|resolved| resolved.clone().known()),
        Some(scene),
        "the binding is reachable on the declared record for F28-B's hierarchy walk"
    );
    let lowered = lower_ordnance(&declared).expect("the bound record still lowers");
    let runtime = lowered.as_projectile().expect("a projectile");
    assert_eq!(
        runtime.family(),
        OrdnanceFamily::ProximityFlak,
        "the runtime component lowers independently of the binding"
    );
    assert_eq!(
        runtime.launch().hardpoint(),
        HardpointKind::WingLeft,
        "and carries no presentation reference"
    );
}

/// The launcher binding is generation-stamped, so a reload cannot leave a
/// stale binding looking live.
#[test]
fn accept_f28_a_the_launcher_binding_is_generation_stamped() {
    let first = OrdnanceLauncherBinding {
        actor: actor(1),
        ordnance: vec![weapon_id("synthetic.fixture_direct_explosive")],
        loadout: ContentId::from_source(ContentKind::Loadout, "synthetic.fixture_loadout")
            .expect("a valid id"),
        generation: cs_app::scene::SceneGeneration(1),
    };
    let reloaded = OrdnanceLauncherBinding {
        generation: cs_app::scene::SceneGeneration(2),
        ordnance: first.ordnance.clone(),
        ..first.clone()
    };
    assert_ne!(
        first.generation, reloaded.generation,
        "a reload stamps a new generation, so a stale binding is identifiable by mismatch"
    );
    assert_eq!(
        first.ordnance, reloaded.ordnance,
        "the ids survive the reload; only the generation changes"
    );
    assert_eq!(first.actor, reloaded.actor);
}

/// The declared inheritance rule is what composes the release velocity, so
/// a `Full` declaration and a `Fraction` declaration are not the same
/// component.
#[test]
fn accept_f28_a_each_declared_inheritance_rule_lowers_distinctly() {
    let base = projectile_of(&declared_synthetic_flak());
    let declared = |inheritance: DeclaredInheritanceRule| {
        let mut projectile = base.clone();
        projectile.launch.inheritance = known(inheritance);
        with_projectile(
            DeclaredOrdnanceFamily::ProximityFlak,
            "synthetic.fixture_inheritance",
            projectile,
        )
    };
    for (declared_rule, expected) in [
        (DeclaredInheritanceRule::Full, InheritanceRule::Full),
        (
            DeclaredInheritanceRule::Fraction { share: 0.25 },
            InheritanceRule::Fraction { share: 0.25 },
        ),
        (DeclaredInheritanceRule::None, InheritanceRule::None),
    ] {
        let lowered = lower_ordnance(&declared(declared_rule)).expect("the record lowers");
        assert_eq!(
            lowered
                .as_projectile()
                .expect("a projectile")
                .launch()
                .inheritance(),
            expected,
            "{declared_rule} lowers as itself"
        );
    }
}

/// The declared hardpoint kinds lower variant-wise rather than collapsing
/// onto one value.
#[test]
fn accept_f28_a_each_declared_hardpoint_kind_lowers_distinctly() {
    let base = projectile_of(&declared_synthetic_flak());
    for (declared_kind, expected) in [
        (DeclaredHardpointKind::Nose, HardpointKind::Nose),
        (DeclaredHardpointKind::WingLeft, HardpointKind::WingLeft),
        (DeclaredHardpointKind::WingRight, HardpointKind::WingRight),
        (DeclaredHardpointKind::Fuselage, HardpointKind::Fuselage),
        (DeclaredHardpointKind::Underslung, HardpointKind::Underslung),
    ] {
        let mut projectile = base.clone();
        projectile.launch.hardpoint = known(declared_kind);
        let declared = with_projectile(
            DeclaredOrdnanceFamily::ProximityFlak,
            "synthetic.fixture_hardpoint",
            projectile,
        );
        let lowered = lower_ordnance(&declared).expect("the record lowers");
        assert_eq!(
            lowered
                .as_projectile()
                .expect("a projectile")
                .launch()
                .hardpoint(),
            expected,
            "{declared_kind} lowers as itself"
        );
    }
}

/// The declared status kinds lower variant-wise, so a choke is not a stall
/// and neither is a marker.
#[test]
fn accept_f28_a_each_declared_status_kind_lowers_distinctly() {
    let base = projectile_of(&declared_synthetic_area_denial());
    for (declared_kind, expected) in [
        (DeclaredStatusEffectKind::Damage, StatusEffectKind::Damage),
        (DeclaredStatusEffectKind::Choke, StatusEffectKind::Choke),
        (DeclaredStatusEffectKind::Stall, StatusEffectKind::Stall),
        (DeclaredStatusEffectKind::Marker, StatusEffectKind::Marker),
    ] {
        let mut projectile = base.clone();
        projectile.status = vec![DeclaredStatusEffect {
            kind: declared_kind,
            duration_ticks: known(30),
            strength: known(0.5),
        }];
        let declared = with_projectile(
            DeclaredOrdnanceFamily::AreaDenialEngine,
            "synthetic.fixture_status",
            projectile,
        );
        let lowered = lower_ordnance(&declared).expect("the record lowers");
        let projectile = lowered.as_projectile().expect("a projectile");
        assert_eq!(
            projectile.status().len(),
            1,
            "{declared_kind} lowers as one effect"
        );
        assert_eq!(
            projectile.status()[0].kind(),
            expected,
            "{declared_kind} lowers as itself"
        );
        assert_eq!(projectile.status()[0].strength(), 0.5);
    }
}

/// A declared family that contradicts the runtime's family rules is
/// refused at the boundary too: the coherence rule is not a content-only
/// convention.
#[test]
fn accept_f28_a_the_boundary_refuses_a_family_whose_rules_contradict_it() {
    let mut projectile = projectile_of(&declared_synthetic_flak());
    projectile.guidance = DeclaredGuidanceRule::Targeted {
        lost_target: known(cs_content::ordnance::DeclaredLostTargetBehavior::Coast),
    };
    let declared = with_projectile(
        DeclaredOrdnanceFamily::ProximityFlak,
        "synthetic.fixture_guided_flak",
        projectile,
    );
    match lower_ordnance(&declared) {
        Err(OrdnanceLowerError::Definition(source)) => assert!(
            matches!(
                source,
                cs_sim::weapons::ordnance::OrdnanceDefinitionError::IncoherentFamily {
                    field: "guidance",
                    ..
                }
            ),
            "a flak shell declared as a seeker is refused by the runtime rule: {source}"
        ),
        other => panic!("an incoherent family must refuse, got {other:?}"),
    }
}
