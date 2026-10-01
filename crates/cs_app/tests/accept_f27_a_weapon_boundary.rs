//! Acceptance scenario F27-A: the declared → runtime conversion boundary,
//! the ECS binding records, and AC01's minimum scenario end to end.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! stage `### F27-A`. Task test prefix: `accept_f27_a_`.
//!
//! These tests drive production code only: [`cs_app::weapons`]'s
//! [`lower_gun`], [`lower_rules`] and [`lower_ammunition`], the
//! [`WeaponActorBinding`] and [`MountPoseBinding`] records, and the
//! `cs_sim::weapons` resolver and sweep the lowered records feed — AC01's
//! thin-target hit runs end to end through the boundary, so removing the
//! conversion or silently defaulting an unknown fails them.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use std::collections::BTreeMap;

use cs_app::scene::SceneGeneration;
use cs_app::weapons::{
    MountPoseBinding, WeaponActorBinding, WeaponLowerError, declared_scene_binding,
    lower_ammunition, lower_gun, lower_rules,
};
use cs_content::damage::DamageNodeKey;
use cs_content::weapons::{
    DeclaredDamageChannel, DeclaredGunDefinition, DeclaredGunRate, DeclaredInheritanceRule,
    DeclaredSelfHitRule, DeclaredSpreadCone, DeclaredWeaponDamage, InteractionRules,
    declared_synthetic_ammunition, declared_synthetic_gun, declared_synthetic_loadout,
    synthetic_ammunition_id, synthetic_effect_id, synthetic_sound_id,
};
use cs_sim::damage::ActorId;
use cs_sim::weapons::{
    AmmunitionId, Ballistics, FriendlyFireRule, GunBank, GunMountKind, GunStateError,
    InheritanceRule, IntentRefusal, MountTransform, ProjectileSegment, SYNTHETIC_STARTING_ROUNDS,
    SelfHitRule, SweepTarget, WeaponState, synthetic_mount,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 13;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

fn claim() -> ClaimId {
    ClaimId::new("f27a.boundary-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(cs_types::content::Known::new(
        value,
        Provenance::designed(claim()),
    ))
}

fn unknown<T>() -> Resolved<T> {
    Resolved::unknown(claim(), "the original ballistic parameter is unmeasured")
        .expect("a nonempty reason")
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

/// A declared gun built from the synthetic fixture's values, so a test can
/// replace exactly one field and leave the rest known.
fn declared_with(
    mount: &str,
    muzzle_velocity: Resolved<f64>,
    damage: DeclaredWeaponDamage,
    rules: InteractionRules,
    inheritance: Resolved<DeclaredInheritanceRule>,
) -> DeclaredGunDefinition {
    DeclaredGunDefinition::try_new(
        ContentId::from_source(ContentKind::Weapon, "test.gun").expect("valid"),
        Origin::SyntheticFixture,
        DamageNodeKey::new(mount).expect("a valid mount key"),
        cs_content::weapons::DeclaredGunMountKind::Nose,
        None,
        known(cs_content::weapons::DeclaredCaliber::try_new("test caliber").expect("valid")),
        known(synthetic_ammunition_id()),
        known(DeclaredGunRate {
            ticks_between_shots: 3,
        }),
        muzzle_velocity,
        known(90),
        DeclaredSpreadCone {
            half_angle: known(cs_types::space::Radians(0.004)),
        },
        damage,
        inheritance,
        known(synthetic_effect_id()),
        known(synthetic_sound_id()),
        rules,
        Provenance::designed(claim()),
    )
    .expect("a valid declared gun")
}

fn full_rules() -> InteractionRules {
    InteractionRules {
        self_hit: known(DeclaredSelfHitRule::Excluded),
        friendly_fire: known(cs_content::weapons::DeclaredFriendlyFireRule::HostileOnly),
        penetration: known(false),
        ricochet: known(false),
        ammo_switching: known(false),
    }
}

/// `lower_gun` preserves the whole declared structure: every field of the
/// sheet's deliverable arrives on the runtime record, with the mount keyed
/// by the same damage-node key the declared damage graph uses.
#[test]
fn accept_f27_a_lower_gun_preserves_the_declared_structure() {
    let declared = declared_synthetic_gun();
    let lowered = lower_gun(&declared).expect("the synthetic fixture lowers");

    assert_eq!(lowered.mount(), &synthetic_mount());
    assert_eq!(lowered.kind(), GunMountKind::Nose);
    assert_eq!(lowered.caliber(), "synthetic fixture caliber");
    assert_eq!(
        lowered.ammunition(),
        &lower_ammunition(&declared_synthetic_ammunition()).expect("the ammo lowers"),
        "the gun's declared ammunition type is the lowered ammunition id"
    );
    assert_eq!(lowered.rate().ticks_between_shots(), 4);
    assert_eq!(lowered.muzzle_velocity_mps(), 640.0);
    assert_eq!(lowered.lifetime_ticks(), 90);
    assert_eq!(lowered.spread().half_angle_radians(), 0.004);
    assert_eq!(lowered.damage().armor, 6.0);
    assert_eq!(lowered.damage().internal, 3.0);
    assert_eq!(lowered.inheritance(), InheritanceRule::Full);
    assert_eq!(lowered.effect(), &synthetic_effect_id());
    assert_eq!(lowered.sound(), &synthetic_sound_id());

    // The lowered gun's mount key is the *same key* the declared damage
    // graph uses for its weapon-mount node, which is what makes a destroyed
    // node disable exactly this gun.
    assert_eq!(
        lowered.mount().as_str(),
        declared.mount().as_str(),
        "the mount key crosses the boundary by text and stays one identity"
    );

    // `lower_rules` maps the five interaction options field-wise.
    let rules = lower_rules(declared.rules()).expect("the fixture rules lower");
    assert_eq!(rules.self_hit, SelfHitRule::Excluded);
    assert_eq!(rules.friendly_fire, FriendlyFireRule::HostileOnly);
    assert!(!rules.penetration);
    assert!(!rules.ricochet);
    assert!(!rules.ammo_switching);
}

/// The refusals that matter: an unresolved ballistic field never becomes a
/// default, and the refusal names *which* field and under whose claim.
#[test]
fn accept_f27_a_unknown_fields_refuse_to_lower() {
    let unknown_muzzle = declared_with(
        "gun_mount_1",
        unknown(),
        DeclaredWeaponDamage {
            armor: known(6.0),
            internal: known(3.0),
        },
        full_rules(),
        known(DeclaredInheritanceRule::Full),
    );
    assert_eq!(
        lower_gun(&unknown_muzzle),
        Err(WeaponLowerError::UnknownField {
            field: "muzzle_velocity_mps",
            claim_id: claim(),
            reason: "the original ballistic parameter is unmeasured".to_owned(),
        }),
        "a gun with an unmeasured muzzle velocity must not be lowered into a session"
    );

    let unknown_damage = declared_with(
        "gun_mount_1",
        known(640.0),
        DeclaredWeaponDamage {
            armor: unknown(),
            internal: known(3.0),
        },
        full_rules(),
        known(DeclaredInheritanceRule::Full),
    );
    assert_eq!(
        lower_gun(&unknown_damage),
        Err(WeaponLowerError::UnknownField {
            field: "damage.armor",
            claim_id: claim(),
            reason: "the original ballistic parameter is unmeasured".to_owned(),
        }),
        "an unmeasured damage amount is refused rather than defaulted to zero"
    );

    let unknown_inheritance = declared_with(
        "gun_mount_1",
        known(640.0),
        DeclaredWeaponDamage {
            armor: known(6.0),
            internal: known(3.0),
        },
        full_rules(),
        unknown(),
    );
    assert_eq!(
        lower_gun(&unknown_inheritance),
        Err(WeaponLowerError::UnknownField {
            field: "inheritance",
            claim_id: claim(),
            reason: "the original ballistic parameter is unmeasured".to_owned(),
        }),
        "the inherited-velocity rule is an explicit rule, not an assumption"
    );

    // Each interaction option refuses independently and by name.
    for (field, rules) in [
        (
            "rules.self_hit",
            InteractionRules {
                self_hit: unknown(),
                ..full_rules()
            },
        ),
        (
            "rules.friendly_fire",
            InteractionRules {
                friendly_fire: unknown(),
                ..full_rules()
            },
        ),
        (
            "rules.penetration",
            InteractionRules {
                penetration: unknown(),
                ..full_rules()
            },
        ),
        (
            "rules.ricochet",
            InteractionRules {
                ricochet: unknown(),
                ..full_rules()
            },
        ),
        (
            "rules.ammo_switching",
            InteractionRules {
                ammo_switching: unknown(),
                ..full_rules()
            },
        ),
    ] {
        assert_eq!(
            lower_rules(&rules),
            Err(WeaponLowerError::UnknownField {
                field,
                claim_id: claim(),
                reason: "the original ballistic parameter is unmeasured".to_owned(),
            }),
            "an unmeasured interaction option must refuse to lower"
        );
    }

    // A declared record whose every field is unknown is still a *valid*
    // declared record; the refusal happens at the boundary, not before it.
    let all_unknown = DeclaredGunDefinition::try_new(
        ContentId::from_source(ContentKind::Weapon, "test.gun").expect("valid"),
        Origin::SyntheticFixture,
        DamageNodeKey::new("gun_mount_1").expect("valid"),
        cs_content::weapons::DeclaredGunMountKind::Nose,
        None,
        unknown(),
        unknown(),
        unknown(),
        unknown(),
        unknown(),
        DeclaredSpreadCone {
            half_angle: unknown(),
        },
        DeclaredWeaponDamage {
            armor: unknown(),
            internal: unknown(),
        },
        unknown(),
        unknown(),
        unknown(),
        InteractionRules {
            self_hit: unknown(),
            friendly_fire: unknown(),
            penetration: unknown(),
            ricochet: unknown(),
            ammo_switching: unknown(),
        },
        Provenance::designed(claim()),
    )
    .expect("a record of unknowns is a valid declared record");
    assert!(lower_gun(&all_unknown).is_err());
    assert!(lower_rules(all_unknown.rules()).is_err());
}

/// `lower_ammunition` returns the opaque `ammo` id the gun's declared
/// ammunition names — and nothing else. No multiplier, no derived damage:
/// F27 non-negotiable 1 forbids an unverified table standing in for the
/// original catalogue.
#[test]
fn accept_f27_a_lower_ammunition_yields_only_the_catalog_id() {
    let declared = declared_synthetic_ammunition();
    let lowered = lower_ammunition(&declared).expect("the fixture ammunition lowers");
    assert_eq!(
        lowered,
        AmmunitionId::try_new(
            ContentId::from_source(ContentKind::Ammo, "synthetic.fixture_slug").expect("valid")
        )
        .expect("a valid ammunition id")
    );
    assert_eq!(lowered.as_str(), "ammo/synthetic.fixture_slug");
    assert_eq!(
        lowered.id().kind(),
        ContentKind::Ammo,
        "the lowered identity is namespaced, so it cannot be confused with a gun"
    );

    // The declared loadout's pairings are exactly the audit rows a lowered
    // session walks: each gun id paired with each lowered ammunition id.
    let loadout = declared_synthetic_loadout();
    let lowered_ids: Vec<AmmunitionId> = loadout
        .ammunition()
        .iter()
        .filter_map(|declared_id| AmmunitionId::try_new(declared_id.id().clone()).ok())
        .collect();
    assert_eq!(
        loadout
            .pairings()
            .iter()
            .map(|(gun, _)| gun.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec![loadout.guns()[0].as_str().to_owned()],
        "every declared pairing survives the boundary"
    );
    assert_eq!(lowered_ids.len(), loadout.ammunition().len());
}

/// AC01's minimum scenario end to end: a lowered gun fires through the
/// boundary, its spawn sweeps a thin target at high velocity, and the hit
/// lands exactly once.
#[test]
fn accept_f27_a_lowered_gun_sweeps_a_thin_target_and_hits_once() {
    let declared = declared_synthetic_gun();
    let gun = lower_gun(&declared).expect("the synthetic fixture lowers");
    let rules = lower_rules(declared.rules()).expect("the fixture rules lower");

    let mount = gun.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount.clone()]).expect("a valid bank"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid weapon state");

    let mut resolver = cs_sim::weapons::FireResolver::new(SESSION, Tick(0));
    resolver
        .register(actor(1), vec![gun], state)
        .expect("the lowered gun registers");

    // 640 m/s at 1/30 s is 21.3 m per tick against a 0.5 m thick target:
    // forty times the thickness, so an endpoint-only check would miss it.
    let half = [3.0, 3.0, 0.25];
    let muzzle = [0.0, 0.0, half[2] + 10.0];

    // The muzzle pose comes from the live hierarchy, supplied here as the
    // transform F27-B's walk would produce.
    let transform = MountTransform::try_new(position(muzzle), UnitVec3::FORWARD, [0.0; 3])
        .expect("a valid mount transform");
    let transforms = BTreeMap::from([(mount.clone(), transform)]);

    let resolution = resolver
        .resolve(
            &cs_sim::weapons::FireIntent {
                id: cs_sim::weapons::FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &transforms,
        )
        .expect("the intent resolves");

    assert_eq!(resolution.accepted.len(), 1, "the lowered gun fires once");
    let spawn = &resolution.accepted[0].projectile;

    let travel_m = spawn.velocity_mps[2].abs() * (1.0 / 30.0);
    assert!(
        travel_m > half[2] * 10.0,
        "the fixture's own muzzle velocity keeps speed*dt far above the obstacle thickness"
    );
    assert!(
        (spawn.origin.z() - half[2]).abs() > half[2],
        "the spawn starts clear of the thin target, so an endpoint-only check cannot see the hit"
    );

    let segment = ProjectileSegment {
        projectile: spawn.projectile,
        previous: spawn.origin,
        current: position([
            spawn.origin.x(),
            spawn.origin.y(),
            spawn.origin.z() - travel_m,
        ]),
    };
    let target =
        SweepTarget::try_new(actor(2), [0.0; 3], [0.0; 3], half).expect("a valid sweep target");

    // The declared rules decide eligibility first: actor 2 is hostile.
    let eligible = rules.eligible(
        actor(1),
        [(target, Some(cs_sim::targeting::Allegiance::Hostile))],
    );
    assert_eq!(eligible.len(), 1);

    let mut ballistics = Ballistics::new();
    let hits = ballistics.sweep(&segment, &eligible);
    assert_eq!(
        hits.len(),
        1,
        "the spawn the boundary produced crosses the thin target exactly once"
    );
    assert_eq!(hits[0].target, actor(2));
    assert!(
        ballistics.sweep(&segment, &eligible).is_empty(),
        "the once-per-projectile rule holds for a boundary-produced spawn too"
    );

    // A duplicate network packet on the same tick drains nothing through the
    // boundary-produced gun.
    let before = resolver
        .state(&actor(1))
        .expect("registered")
        .ammunition(&mount);
    let replay = resolver.resolve(
        &cs_sim::weapons::FireIntent {
            id: cs_sim::weapons::FireIntentId {
                session: SESSION,
                tick: Tick(0),
                producer: 1,
                sequence: 0,
            },
            shooter: actor(1),
        },
        &transforms,
    );
    assert!(matches!(replay, Err(IntentRefusal::DuplicateIntent { .. })));
    assert_eq!(
        resolver
            .state(&actor(1))
            .expect("registered")
            .ammunition(&mount),
        before
    );
}

/// The declared interaction rules are load-bearing through the boundary: a
/// lowered `HostileOnly` gun admits a hostile actor and refuses the shooter
/// and an undeclared pair, exactly as the declared record said.
#[test]
fn accept_f27_a_lowered_rules_filter_candidates_by_declaration() {
    let rules = lower_rules(declared_synthetic_gun().rules()).expect("the rules lower");
    let target_for = |serial: u64| {
        SweepTarget::try_new(actor(serial), [0.0; 3], [0.0; 3], [2.0; 3]).expect("a valid target")
    };

    let eligible = rules.eligible(
        actor(1),
        [
            (target_for(2), Some(cs_sim::targeting::Allegiance::Hostile)),
            (target_for(3), Some(cs_sim::targeting::Allegiance::Friendly)),
            (target_for(4), None),
            (target_for(1), Some(cs_sim::targeting::Allegiance::Hostile)),
        ],
    );
    assert_eq!(
        eligible
            .iter()
            .map(|target| target.actor)
            .collect::<Vec<_>>(),
        vec![actor(2)],
        "only the declared hostile is admitted through the boundary"
    );

    // Widen the *declared* rule and the lowered rule widens with it.
    let permissive = lower_rules(&InteractionRules {
        friendly_fire: known(cs_content::weapons::DeclaredFriendlyFireRule::Everyone),
        ..declared_synthetic_gun().rules().clone()
    })
    .expect("permissive rules lower");
    let eligible = permissive.eligible(
        actor(1),
        [(target_for(3), Some(cs_sim::targeting::Allegiance::Friendly))],
    );
    assert_eq!(
        eligible.len(),
        1,
        "the lowered rule follows the declared one, not a hardcoded hostility"
    );
}

/// The ECS records are session- and generation-qualified: a reloaded
/// hierarchy stamps a new binding, and a stale one is identifiable by
/// mismatch rather than by a surviving pointer.
#[test]
fn accept_f27_a_binding_records_are_generation_stamped() {
    let loadout = declared_synthetic_loadout();
    let binding = WeaponActorBinding {
        actor: actor(1),
        guns: vec![declared_synthetic_gun().gun().clone()],
        loadout: loadout.subject().clone(),
        generation: SceneGeneration(4),
    };
    assert_eq!(binding.actor.session, SESSION);
    assert_eq!(
        binding.loadout.as_str(),
        "loadout/synthetic.fixture_loadout"
    );
    assert_eq!(binding.guns.len(), 1);
    assert_eq!(binding.guns[0].kind(), ContentKind::Weapon);
    assert_eq!(binding.generation, SceneGeneration(4));

    // A different generation is a different binding.
    let stale = WeaponActorBinding {
        generation: SceneGeneration(3),
        ..binding.clone()
    };
    assert_ne!(binding, stale);

    // The mount pose binding names the mount it was read for and the
    // hierarchy generation it was read under.
    let pose = MountPoseBinding {
        mount: synthetic_mount(),
        generation: SceneGeneration(4),
    };
    assert_eq!(
        pose.mount.as_str(),
        synthetic_mount().as_str(),
        "the pose's mount key is the same identity the damage graph disables"
    );
    let stale_pose = MountPoseBinding {
        generation: SceneGeneration(3),
        ..pose.clone()
    };
    assert_ne!(pose, stale_pose);
}

/// A mount's *visual* scene binding stays on the declared record and never
/// enters the runtime gun: gameplay state must not depend on a presentation
/// reference (F27 non-negotiable 2).
#[test]
fn accept_f27_a_scene_binding_stays_on_the_declared_record() {
    let declared = declared_synthetic_gun();
    assert!(
        declared_scene_binding(&declared).is_none(),
        "the synthetic fixture declares no visual mount binding"
    );

    // Declaring one keeps it available to F27-B's hierarchy walk and leaves
    // the runtime gun identical.
    let with_binding = DeclaredGunDefinition::try_new(
        declared.gun().clone(),
        declared.origin().clone(),
        declared.mount().clone(),
        declared.mount_kind(),
        Some(known(
            cs_content::scene::SceneNodeId::from_content_id(
                ContentId::from_source(ContentKind::SceneNode, "test.wing_mount").expect("valid"),
            )
            .expect("a valid scene node id"),
        )),
        declared.caliber().clone(),
        declared.ammunition().clone(),
        declared.rate().clone(),
        declared.muzzle_velocity_mps().clone(),
        declared.lifetime_ticks().clone(),
        declared.spread().clone(),
        declared.damage().clone(),
        declared.inheritance().clone(),
        declared.effect().clone(),
        declared.sound().clone(),
        declared.rules().clone(),
        Provenance::designed(claim()),
    )
    .expect("a declared gun with a scene binding is valid");
    assert!(declared_scene_binding(&with_binding).is_some());
    assert_eq!(
        lower_gun(&with_binding).expect("it still lowers"),
        lower_gun(&declared).expect("it still lowers"),
        "a visual mount binding does not change the runtime gun's gameplay state"
    );
}

/// The boundary keeps the weapon state honest across a registration: a
/// lowered gun whose mount the declared damage graph would disable cannot
/// share a mount with a second gun, and the resolver refuses the duplicate.
#[test]
fn accept_f27_a_lowered_guns_cannot_share_a_mount() {
    let gun = lower_gun(&declared_synthetic_gun()).expect("the fixture lowers");
    let state = WeaponState::try_new(
        &[gun.clone(), gun.clone()],
        GunBank::try_new([gun.mount().clone()]).expect("a valid bank"),
        10,
    );
    assert_eq!(
        state,
        Err(GunStateError::DuplicateMount {
            mount: gun.mount().clone()
        }),
        "two lowered guns on one mount would make a single damage disable ambiguous"
    );

    // The channel vocabulary survives the boundary one-for-one.
    assert_eq!(
        gun.damage().amount_on(cs_sim::damage::DamageChannel::Armor),
        gun.damage().armor
    );
    assert_eq!(
        gun.damage()
            .amount_on(cs_sim::damage::DamageChannel::Internal),
        gun.damage().internal
    );
    assert_eq!(
        DeclaredDamageChannel::ALL.len(),
        2,
        "the declared channel vocabulary and the runtime one stay the same size"
    );
}
