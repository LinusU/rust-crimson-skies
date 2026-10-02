//! Acceptance scenarios F29-C: the damage consumers.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C`. Task test prefix: `accept_f29_c_`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Minimum scenario: **destroying a mount disables its firing and updates its
//! visual state.** These tests drive the production path end to end: the
//! declared graph is lowered by [`cs_app::damage::lower_graph`] /
//! [`lower_policy`], the real [`DamageResolver`] resolves the hit, and
//! [`apply_damage_state`] reflects the authoritative state onto the two
//! consumers the sheet names — the weapon firing gate
//! ([`cs_sim::weapons::FireResolver`]) and the visual damage record
//! ([`AirframeDamageState`]). The firing half is observed off the *gate
//! itself*: a `FireIntent` on a disabled mount is refused with
//! `MountDisabled` and consumes no round, so a bridge that disabled nothing
//! fails at the shot, not merely at a record comparison.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. Whether the original disabled firing from a destroyed mount, and
//! which consumer debris, scoring or bailout had, is unrecovered (F29 "Research
//! boundary"); see `docs/findings/2026-10-02-f29-c-damage-consumers.md`.

use std::collections::BTreeMap;

use cs_app::damage::{
    DamageConsumerEvent, DamageConsumerRefusal, DamageConsumerReport, apply_damage_state,
    lower_graph, lower_policy,
};
use cs_app::scene::AirframeDamageState;
use cs_app::weapons::lower_gun;
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_content::weapons::declared_synthetic_gun;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageGraph, DamageNode, DamageNodeKey,
    DamageNodeKind, DamagePolicy, DamageResolver, HitEvent, HitEventId, PartState,
    SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE, SYNTHETIC_MOUNT_INTEGRITY,
    SYNTHETIC_MOUNT_NODE,
};
use cs_sim::weapons::{
    FireDenialReason, FireIntent, FireIntentId, FireResolver, GunBank, MountTransform,
    SYNTHETIC_STARTING_ROUNDS, WeaponState,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 41;
const PRODUCER: u32 = 1;

// ----------------------------------------------------------------- helpers ---

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn mount() -> DamageNodeKey {
    key(SYNTHETIC_MOUNT_NODE)
}

/// The visual [`SceneNodeId`](cs_content::scene::SceneNodeId) the declared
/// fixture binds the named part to, as its raw catalog id.
fn scene_node(name: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("synthetic.devastator.{name}"),
    )
    .expect("a valid scene node id")
}

fn claim() -> ClaimId {
    ClaimId::new("f29c.test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn policy() -> DamagePolicy {
    DamagePolicy {
        attribution: AttributionRule::FirstLethalHit,
    }
}

/// The production declared → lowered → resolver path for one actor.
fn registered() -> DamageResolver {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    let mut resolver = DamageResolver::new(SESSION, PRODUCER);
    resolver
        .register_actor(actor(1), graph, policy)
        .expect("the lowered actor registers");
    resolver
}

/// A firing gate armed with the fixture gun on `gun_mount_1` through the
/// production declared → lowered boundary.
fn armed() -> FireResolver {
    let gun = lower_gun(&declared_synthetic_gun()).expect("the fixture gun lowers");
    let mount = gun.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount.clone()]).expect("a valid bank"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid weapon state");
    let mut resolver = FireResolver::new(SESSION, Tick(0));
    resolver
        .register(actor(1), vec![gun], state)
        .expect("the fixture gun registers");
    resolver
}

/// An internal hit that lands on `node` for `damage`, at the resolver's tick.
fn hit(node: &DamageNodeKey, damage: f64) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: SESSION,
            tick: Tick(0),
            producer: PRODUCER,
            sequence: 0,
        },
        Some(actor(2)),
        actor(1),
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit")
}

/// Resolves exactly one hit of `damage` on `node`.
fn resolve_hit(resolver: &mut DamageResolver, node: &DamageNodeKey, damage: f64) {
    resolver
        .resolve(Tick(0), &[hit(node, damage)])
        .expect("the resolver accepts the batch");
}

/// The mount pose a real hierarchy walk would supply, so an enabled mount can
/// actually fire.
fn transforms() -> BTreeMap<DamageNodeKey, MountTransform> {
    let transform = MountTransform::try_new(
        WorldPosition::try_new([0.0, 0.0, 10.0]).expect("a finite muzzle"),
        UnitVec3::FORWARD,
        [0.0; 3],
    )
    .expect("a valid mount transform");
    BTreeMap::from([(mount(), transform)])
}

/// Asks the firing gate to fire the selected bank once and returns the
/// refusal reasons it produced.
fn fire_once(fire: &mut FireResolver) -> Vec<FireDenialReason> {
    let resolution = fire
        .resolve(
            &FireIntent {
                id: FireIntentId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: 1,
                    sequence: 0,
                },
                shooter: actor(1),
            },
            &transforms(),
        )
        .expect("the intent resolves");
    resolution.refused
}

// -------------------------------------------------- the minimum scenario ---

/// The minimum scenario: destroying `gun_mount_1` disables its firing and
/// updates its visual state; a scratch keeps both alive.
#[test]
fn accept_f29_c_destroying_a_mount_disables_firing_and_updates_its_visual_state() {
    let mut resolver = registered();
    let mut fire = armed();
    let mut visuals = AirframeDamageState::new();

    // The intact state is already what the consumers say: the pass changes
    // nothing and refuses nothing.
    let baseline = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);
    assert!(
        baseline.report.is_noop() && baseline.log.is_empty(),
        "an intact actor needs no consumer update: {baseline:?}"
    );
    assert!(!fire.state(&actor(1)).expect("armed").is_disabled(&mount()));
    assert!(
        !visuals.is_destroyed(
            &cs_content::scene::SceneNodeId::from_content_id(scene_node(SYNTHETIC_MOUNT_NODE))
                .expect("a scene node id")
        )
    );

    // Destroy exactly the mount.
    resolve_hit(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    assert_eq!(
        resolver.part_state(&actor(1), &mount()),
        Some(PartState::Destroyed)
    );

    let outcome = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);
    assert_eq!(
        outcome.report,
        DamageConsumerReport {
            mounts_disabled: 1,
            visuals_destroyed: 1,
            ..Default::default()
        },
        "the destroyed mount disables its firing and records its destruction"
    );
    assert_eq!(
        outcome.log.events(),
        &[
            DamageConsumerEvent::VisualDestroyed {
                actor: actor(1),
                node: mount(),
                scene_node: cs_content::scene::SceneNodeId::from_content_id(scene_node(
                    SYNTHETIC_MOUNT_NODE
                ))
                .expect("a scene node id"),
            },
            DamageConsumerEvent::MountDisabled {
                actor: actor(1),
                mount: mount(),
            },
        ],
        "the visual and the firing gate are both updated, once"
    );

    assert!(
        fire.state(&actor(1)).expect("armed").is_disabled(&mount()),
        "the firing gate carries the disablement"
    );
    assert!(
        visuals.is_destroyed(
            &cs_content::scene::SceneNodeId::from_content_id(scene_node(SYNTHETIC_MOUNT_NODE))
                .expect("a scene node id")
        ),
        "the visual record carries the destruction"
    );

    // The gate is real: the shot is refused with `MountDisabled` and no round
    // is consumed.
    let before = fire.state(&actor(1)).expect("armed").ammunition(&mount());
    let refusals = fire_once(&mut fire);
    assert_eq!(
        refusals,
        vec![FireDenialReason::MountDisabled { mount: mount() }],
        "a disabled mount cannot fire"
    );
    assert_eq!(
        fire.state(&actor(1)).expect("armed").ammunition(&mount()),
        before,
        "a refused shot consumes no round"
    );
}

/// A scratched mount is not destroyed: the gate stays open, the part is not
/// presented destroyed, and the shot really fires.
#[test]
fn accept_f29_c_a_scratched_mount_still_fires_and_keeps_its_visual() {
    let mut resolver = registered();
    let mut fire = armed();
    let mut visuals = AirframeDamageState::new();

    resolve_hit(&mut resolver, &mount(), 1.0);
    assert_eq!(
        resolver.part_state(&actor(1), &mount()),
        Some(PartState::Damaged)
    );

    let outcome = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);
    assert!(
        outcome.report.is_noop() && outcome.log.is_empty(),
        "a scratch changes no consumer: {outcome:?}"
    );
    assert!(!fire.state(&actor(1)).expect("armed").is_disabled(&mount()));
    assert!(
        !visuals.is_destroyed(
            &cs_content::scene::SceneNodeId::from_content_id(scene_node(SYNTHETIC_MOUNT_NODE))
                .expect("a scene node id")
        ),
        "a scratched mount is still presented"
    );

    let before = fire.state(&actor(1)).expect("armed").ammunition(&mount());
    let refusals = fire_once(&mut fire);
    assert!(
        refusals.is_empty(),
        "a scratched mount still fires: {refusals:?}"
    );
    assert_eq!(
        fire.state(&actor(1)).expect("armed").ammunition(&mount()),
        before - 1,
        "the accepted shot consumed exactly one round"
    );
}

/// The pass is convergent, and a mount the state says is intact is re-enabled:
/// applying the same state twice changes nothing, and a stale disable cannot
/// outlive an authoritative intact part.
#[test]
fn accept_f29_c_the_consumer_pass_is_convergent_and_re_enables_a_stale_disable() {
    let mut resolver = registered();
    let mut fire = armed();
    let mut visuals = AirframeDamageState::new();

    // A stale disable the state disagrees with: the pass clears it.
    fire.state_mut(&actor(1)).expect("armed").disable(&mount());
    let re_enabled = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);
    assert_eq!(
        re_enabled.report,
        DamageConsumerReport {
            mounts_enabled: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        re_enabled.log.last(),
        Some(&DamageConsumerEvent::MountEnabled {
            actor: actor(1),
            mount: mount(),
        })
    );
    assert!(!fire.state(&actor(1)).expect("armed").is_disabled(&mount()));

    // Destroy the mount, then apply the same state twice: one change, then
    // nothing and no repeated event.
    resolve_hit(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    let first = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);
    assert_eq!(first.report.mounts_disabled, 1);
    assert_eq!(first.report.visuals_destroyed, 1);
    assert!(fire.state(&actor(1)).expect("armed").is_disabled(&mount()));

    let second = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);
    assert!(
        second.report.is_noop() && second.log.is_empty(),
        "a converged pass reports nothing: {second:?}"
    );
    assert!(fire.state(&actor(1)).expect("armed").is_disabled(&mount()));
}

/// A destroyed engine updates its own visual and leaves the weapon gate alone:
/// the pass wires each carrier to the consumer it actually drives.
#[test]
fn accept_f29_c_a_destroyed_engine_updates_its_visual_but_not_the_weapon_gate() {
    let mut resolver = registered();
    let mut fire = armed();
    let mut visuals = AirframeDamageState::new();

    resolve_hit(
        &mut resolver,
        &key(SYNTHETIC_ENGINE_NODE),
        SYNTHETIC_ENGINE_INTEGRITY,
    );
    let outcome = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);

    assert_eq!(
        outcome.report,
        DamageConsumerReport {
            visuals_destroyed: 1,
            ..Default::default()
        },
        "the engine's own visual is destroyed"
    );
    assert_eq!(
        outcome.log.events(),
        &[DamageConsumerEvent::VisualDestroyed {
            actor: actor(1),
            node: key(SYNTHETIC_ENGINE_NODE),
            scene_node: cs_content::scene::SceneNodeId::from_content_id(scene_node(
                SYNTHETIC_ENGINE_NODE
            ))
            .expect("a scene node id"),
        }]
    );
    assert!(
        !fire.state(&actor(1)).expect("armed").is_disabled(&mount()),
        "the engine does not disable the gun"
    );
    assert!(
        fire_once(&mut fire).is_empty(),
        "the gun still fires after the engine is destroyed"
    );
}

// --------------------------------------------------------- the refusals ---

/// A foreign-session or unregistered actor is refused and changes no consumer.
#[test]
fn accept_f29_c_a_foreign_or_unknown_actor_is_refused_and_changes_nothing() {
    let resolver = registered();
    let mut fire = armed();
    let mut visuals = AirframeDamageState::new();

    let foreign = ActorId {
        session: SESSION + 1,
        serial: 1,
    };
    let outcome = apply_damage_state(&resolver, foreign, &mut fire, &mut visuals);
    assert_eq!(outcome.report.refused, 1);
    assert_eq!(
        outcome.log.last(),
        Some(&DamageConsumerEvent::Refused(
            DamageConsumerRefusal::ForeignSession {
                expected: SESSION,
                found: SESSION + 1,
            }
        )),
        "a generation-qualified actor is never applied to another generation"
    );
    assert!(!fire.state(&actor(1)).expect("armed").is_disabled(&mount()));
    assert!(visuals.is_empty());

    let unknown = apply_damage_state(&resolver, actor(99), &mut fire, &mut visuals);
    assert_eq!(unknown.report.refused, 1);
    assert_eq!(
        unknown.log.last(),
        Some(&DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnknownActor { actor: actor(99) }
        ))
    );
    assert!(!fire.state(&actor(1)).expect("armed").is_disabled(&mount()));
    assert!(visuals.is_empty());
}

/// A weapon carrier the actor has no weapon state for, and a carrier whose
/// mount the gate does not carry, are both reported rather than dropped.
#[test]
fn accept_f29_c_an_unarmed_actor_and_an_unmounted_carrier_are_reported() {
    // No weapon state at all: the carrier cannot be taken out of the gate.
    let resolver = registered();
    let mut unarmed = FireResolver::new(SESSION, Tick(0));
    let mut visuals = AirframeDamageState::new();
    let outcome = apply_damage_state(&resolver, actor(1), &mut unarmed, &mut visuals);
    assert_eq!(outcome.report.refused, 1);
    assert_eq!(
        outcome.log.last(),
        Some(&DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnarmedActor { actor: actor(1) }
        )),
        "the carrier is reported once, not once per part"
    );

    // A weapon state exists, but the graph's carrier is not a mount it carries.
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29c-unmounted")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(
                key("phantom_mount"),
                DamageNodeKind::WeaponMount,
                known(10.0),
            )
            .with_disables(cs_sim::damage::SystemKind::Weapon),
        ],
    )
    .expect("the phantom graph is valid");
    let mut phantom = DamageResolver::new(SESSION, PRODUCER);
    phantom
        .register_actor(actor(1), graph, policy())
        .expect("the actor registers");
    let mut fire = armed();
    let outcome = apply_damage_state(&phantom, actor(1), &mut fire, &mut visuals);
    assert_eq!(
        outcome.log.last(),
        Some(&DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnmountedMount {
                actor: actor(1),
                mount: key("phantom_mount"),
            }
        )),
        "a disable that names no gun is reported, not written"
    );
    assert!(!fire.state(&actor(1)).expect("armed").is_disabled(&mount()));
}

/// A declared-but-unresolved visual binding and a binding that is not a scene
/// node are both refused: the visual consumer never guesses a scene node.
#[test]
fn accept_f29_c_unresolvable_visual_bindings_are_refused_not_guessed() {
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29c-visuals")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(key("hull"), DamageNodeKind::InternalStructure, known(40.0))
                .with_scene_binding(Resolved::Unknown {
                    claim_id: claim(),
                    reason: "the hull's scene node is unresolved".to_owned(),
                }),
            DamageNode::new(key("wing"), DamageNodeKind::InternalStructure, known(20.0))
                .with_scene_binding(known(
                    ContentId::from_source(ContentKind::Airframe, "not.a.scene_node")
                        .expect("a valid airframe id"),
                )),
        ],
    )
    .expect("the visual graph is valid");
    let mut resolver = DamageResolver::new(SESSION, PRODUCER);
    resolver
        .register_actor(actor(1), graph, policy())
        .expect("the actor registers");

    let mut fire = FireResolver::new(SESSION, Tick(0));
    let mut visuals = AirframeDamageState::new();
    let outcome = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);

    assert_eq!(outcome.report.refused, 2);
    assert!(!outcome.report.is_noop());
    assert_eq!(outcome.report.visuals_destroyed, 0);
    assert!(
        outcome.log.events().iter().any(|event| matches!(
            event,
            DamageConsumerEvent::Refused(DamageConsumerRefusal::UnresolvedVisual { node, claim_id, .. })
                if node == &key("hull") && claim_id == &claim()
        )),
        "the unresolved binding carries its claim: {:?}",
        outcome.log
    );
    assert!(outcome.log.events().iter().any(|event| matches!(
        event,
        DamageConsumerEvent::Refused(DamageConsumerRefusal::NonSceneVisual { node, .. })
            if node == &key("wing")
    )));
    assert!(
        visuals.is_empty(),
        "nothing was guessed into the visual record"
    );
}

/// A pool that is unresolved asserts neither destruction nor repair: the
/// visual and the firing gate are left exactly as they are.
#[test]
fn accept_f29_c_an_unresolved_pool_asserts_neither_visual_nor_mount() {
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29c-unknown-pool")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(
                mount(),
                DamageNodeKind::WeaponMount,
                Resolved::Unknown {
                    claim_id: claim(),
                    reason: "the mount's integrity is unmeasured".to_owned(),
                },
            )
            .with_disables(cs_sim::damage::SystemKind::Weapon)
            .with_scene_binding(known(scene_node(SYNTHETIC_MOUNT_NODE))),
        ],
    )
    .expect("an unknown pool still validates");
    let mut resolver = DamageResolver::new(SESSION, PRODUCER);
    resolver
        .register_actor(actor(1), graph, policy())
        .expect("the actor registers");

    let mut fire = armed();
    let mut visuals = AirframeDamageState::new();
    let outcome = apply_damage_state(&resolver, actor(1), &mut fire, &mut visuals);

    assert_eq!(
        outcome.log.last(),
        Some(&DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnresolvedIntegrity {
                actor: actor(1),
                node: mount(),
                claim_id: claim(),
                reason: "the mount's integrity is unmeasured".to_owned(),
            }
        )),
        "an unmeasured pool is named, not guessed"
    );
    assert!(!fire.state(&actor(1)).expect("armed").is_disabled(&mount()));
    assert!(visuals.is_empty());
    assert!(
        fire_once(&mut fire).is_empty(),
        "an unresolved mount does not take the gun down"
    );
}
