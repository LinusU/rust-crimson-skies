//! Acceptance scenarios F29-B: armor zones route a hit separately from the
//! internal channel, overkill continues into the guarded part, and a
//! destroyed carrier's system disablement is queryable state, not only a
//! transient event.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-B`. Task test prefix: `accept_f29_b_`.
//!
//! Minimum scenario: armor and internal channels produce distinguishable
//! results without invented multipliers. These tests drive the production
//! [`DamageResolver`] and graph records only; removing the channel routing,
//! the guard fallback or the system-state derivation fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. Which parts the original modeled and whether it aggregated a
//! system across carriers is unrecovered (F29 "Research boundary"); see
//! `docs/findings/2026-10-02-f29-b-zones-armor-and-system-disablement.md`.

use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageEventKind, DamageGraph, DamageNode,
    DamageNodeKey, DamageNodeKind, DamagePolicy, DamageResolver, HitEvent, HitEventId, PartState,
    RefusalReason, SYNTHETIC_ARMOR_INTEGRITY, SYNTHETIC_ARMOR_NODE, SYNTHETIC_ENGINE_INTEGRITY,
    SYNTHETIC_ENGINE_NODE, SYNTHETIC_HULL_INTEGRITY, SYNTHETIC_HULL_NODE,
    SYNTHETIC_MOUNT_INTEGRITY, SYNTHETIC_MOUNT_NODE, SystemKind, SystemState,
    synthetic_airframe_graph,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const SESSION: u64 = 17;
const PRODUCER: u32 = 1;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn integrity(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f29b.test").expect("valid claim id")),
    ))
}

fn unknown_integrity(claim: &str) -> Resolved<f64> {
    Resolved::Unknown {
        claim_id: ClaimId::new(claim).expect("valid claim id"),
        reason: "integrity unmeasured".to_owned(),
    }
}

fn policy() -> DamagePolicy {
    DamagePolicy {
        attribution: AttributionRule::FirstLethalHit,
    }
}

#[allow(clippy::too_many_arguments)]
fn hit(
    producer: u32,
    sequence: u32,
    attacker: Option<ActorId>,
    target: ActorId,
    node: &str,
    channel: DamageChannel,
    damage: f64,
) -> HitEvent {
    hit_at(
        producer, sequence, attacker, target, node, channel, damage, 3,
    )
}

#[allow(clippy::too_many_arguments)]
fn hit_at(
    producer: u32,
    sequence: u32,
    attacker: Option<ActorId>,
    target: ActorId,
    node: &str,
    channel: DamageChannel,
    damage: f64,
    tick: u64,
) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: SESSION,
            tick: Tick(tick),
            producer,
            sequence,
        },
        attacker,
        target,
        key(node),
        channel,
        damage,
    )
    .expect("a finite, non-negative hit")
}

fn register(graph: DamageGraph, target: ActorId) -> DamageResolver {
    let mut resolver = DamageResolver::new(SESSION, PRODUCER);
    resolver
        .register_actor(target, graph, policy())
        .expect("the actor registers under its own session");
    resolver
}

/// The applied hops of a resolution, in emission order.
fn applied(resolution: &cs_sim::damage::TickResolution) -> Vec<(DamageNodeKey, f64)> {
    resolution
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            DamageEventKind::HitApplied { node, applied, .. } => Some((node.clone(), *applied)),
            _ => None,
        })
        .collect()
}

/// AC02 minimum scenario: the same raw damage routed on the armor channel
/// and on the internal channel leaves visibly different pools, with no
/// multiplier anywhere — the armor pool absorbs first and only the
/// remainder reaches the part, while the internal channel lands everything
/// on the named part and never touches the armor.
#[test]
fn accept_f29_b_armor_and_internal_channels_produce_distinguishable_results() {
    let target = actor(1);

    let mut armored = register(synthetic_airframe_graph(), target);
    let armor_resolution = armored
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_HULL_NODE,
                DamageChannel::Armor,
                25.0,
            )],
        )
        .expect("valid batch");
    assert_eq!(
        applied(&armor_resolution),
        vec![
            (key(SYNTHETIC_ARMOR_NODE), SYNTHETIC_ARMOR_INTEGRITY),
            (key(SYNTHETIC_HULL_NODE), 25.0 - SYNTHETIC_ARMOR_INTEGRITY),
        ],
        "the armor pool absorbs its whole integrity and the remainder lands on the hull"
    );
    assert_eq!(
        armored.remaining_integrity(&target, &key(SYNTHETIC_HULL_NODE)),
        Some(SYNTHETIC_HULL_INTEGRITY - (25.0 - SYNTHETIC_ARMOR_INTEGRITY))
    );

    let mut internal = register(synthetic_airframe_graph(), target);
    internal
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_HULL_NODE,
                DamageChannel::Internal,
                25.0,
            )],
        )
        .expect("valid batch");
    assert_eq!(
        internal.remaining_integrity(&target, &key(SYNTHETIC_HULL_NODE)),
        Some(SYNTHETIC_HULL_INTEGRITY - 25.0),
        "the internal channel lands its full damage on the named node"
    );
    assert_eq!(
        internal.part_state(&target, &key(SYNTHETIC_ARMOR_NODE)),
        Some(PartState::Intact),
        "the armor pool is untouched by an internal hit"
    );

    // The two channels really diverge: the internal hull pool is lower than
    // the armored one by exactly the armor's absorbed share.
    let armored_hull = armored
        .remaining_integrity(&target, &key(SYNTHETIC_HULL_NODE))
        .expect("known pool");
    let internal_hull = internal
        .remaining_integrity(&target, &key(SYNTHETIC_HULL_NODE))
        .expect("known pool");
    assert_eq!(internal_hull, armored_hull - SYNTHETIC_ARMOR_INTEGRITY);
}

/// A guard that declares no `overflow` of its own does not swallow the
/// shot's remainder: the overkill continues into the part the guard
/// protects, because that is the node the shot named.
///
/// Before the fallback, this hit left the hull at full integrity and the
/// remainder vanished; the guard was an infinite shield, not an armor pool.
#[test]
fn accept_f29_b_armor_overkill_continues_into_the_guarded_part() {
    let target = actor(1);
    let armor = key("plate");
    let hull = key("hull");
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29b-overkill")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(armor.clone(), DamageNodeKind::ArmorZone, integrity(10.0)),
            DamageNode::new(
                hull.clone(),
                DamageNodeKind::InternalStructure,
                integrity(40.0),
            )
            .with_lethal(true)
            .with_guard(armor.clone()),
        ],
    )
    .expect("the guard graph is valid");

    let mut resolver = register(graph, target);
    let resolution = resolver
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                "hull",
                DamageChannel::Armor,
                15.0,
            )],
        )
        .expect("valid batch");

    assert_eq!(
        applied(&resolution),
        vec![(armor.clone(), 10.0), (hull.clone(), 5.0)],
        "the guard absorbs its pool and the remaining five points reach the hull"
    );
    assert_eq!(
        resolver.part_state(&target, &armor),
        Some(PartState::Destroyed)
    );
    assert_eq!(resolver.remaining_integrity(&target, &hull), Some(35.0));
    assert_eq!(
        resolver.part_state(&target, &hull),
        Some(PartState::Damaged)
    );
    assert!(
        !resolver.is_destroyed(&target),
        "five overkill points do not destroy a forty-point hull"
    );
}

/// One armor zone can guard several parts: each shot's overkill continues
/// into the part that shot named, never into whichever part the guard
/// happened to overflow into.
#[test]
fn accept_f29_b_one_armor_zone_routes_each_shots_overkill_to_its_own_part() {
    let target = actor(1);
    let armor = key("shared_plate");
    let wing = key("wing");
    let tail = key("tail");
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29b-shared")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(armor.clone(), DamageNodeKind::ArmorZone, integrity(20.0)),
            DamageNode::new(
                wing.clone(),
                DamageNodeKind::InternalStructure,
                integrity(10.0),
            )
            .with_guard(armor.clone()),
            DamageNode::new(
                tail.clone(),
                DamageNodeKind::InternalStructure,
                integrity(10.0),
            )
            .with_guard(armor.clone()),
        ],
    )
    .expect("the shared-guard graph is valid");

    let mut resolver = register(graph, target);
    // The first shot depletes the shared plate and carries ten points into
    // the wing.
    let first = resolver
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                "wing",
                DamageChannel::Armor,
                30.0,
            )],
        )
        .expect("valid batch");
    assert_eq!(
        applied(&first),
        vec![(armor.clone(), 20.0), (wing.clone(), 10.0)],
        "the overkill lands on the wing the shot named"
    );
    assert_eq!(
        resolver.remaining_integrity(&target, &tail),
        Some(10.0),
        "the tail is untouched by a shot at the wing"
    );

    // A second shot names the tail; the plate is already gone, so all five
    // points reach the tail.
    let second = resolver
        .resolve(
            Tick(4),
            &[hit_at(
                2,
                0,
                Some(actor(2)),
                target,
                "tail",
                DamageChannel::Armor,
                5.0,
                4,
            )],
        )
        .expect("valid batch");
    assert_eq!(
        applied(&second),
        vec![(armor.clone(), 0.0), (tail.clone(), 5.0)],
        "a depleted shared guard routes the whole shot into the part it names"
    );
    assert_eq!(resolver.remaining_integrity(&target, &tail), Some(5.0));
}

/// The internal channel bypasses the guard entirely: the named part takes
/// the full damage and the armor behind the same part stays intact, so the
/// guard is a routing decision, never a damage multiplier applied to every
/// channel.
#[test]
fn accept_f29_b_internal_channel_bypasses_the_guard() {
    let target = actor(1);
    let armor = key("plate");
    let hull = key("hull");
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29b-bypass")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(armor.clone(), DamageNodeKind::ArmorZone, integrity(10.0)),
            DamageNode::new(
                hull.clone(),
                DamageNodeKind::InternalStructure,
                integrity(40.0),
            )
            .with_guard(armor.clone()),
        ],
    )
    .expect("the guard graph is valid");

    let mut resolver = register(graph, target);
    let resolution = resolver
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                "hull",
                DamageChannel::Internal,
                12.0,
            )],
        )
        .expect("valid batch");

    assert_eq!(
        applied(&resolution),
        vec![(hull.clone(), 12.0)],
        "the internal shot never enters the armor node"
    );
    assert_eq!(
        resolver.part_state(&target, &armor),
        Some(PartState::Intact),
        "the armor is not consumed by an internal hit"
    );
}

/// An armor-channel hit that names a node with no guard goes straight to
/// that node; the armor channel is not a blanket shield for the whole
/// airframe.
#[test]
fn accept_f29_b_armor_channel_on_an_unguarded_part_lands_directly() {
    let target = actor(1);
    let mut resolver = register(synthetic_airframe_graph(), target);
    let resolution = resolver
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_ENGINE_NODE,
                DamageChannel::Armor,
                4.0,
            )],
        )
        .expect("valid batch");

    assert_eq!(
        applied(&resolution),
        vec![(key(SYNTHETIC_ENGINE_NODE), 4.0)],
        "an unguarded part takes the armor-channel hit itself"
    );
    assert_eq!(
        resolver.remaining_integrity(&target, &key(SYNTHETIC_ARMOR_NODE)),
        Some(SYNTHETIC_ARMOR_INTEGRITY),
        "the nose armor guards the hull, not every part"
    );
}

/// Destroying a weapon mount disables the weapon system and leaves
/// propulsion enabled; the state is queryable after the event stream is
/// gone, and the reverse carrier's destruction disables only propulsion.
#[test]
fn accept_f29_b_destroying_a_carrier_disables_only_its_own_system() {
    let target = actor(1);
    let mut resolver = register(synthetic_airframe_graph(), target);

    assert_eq!(
        resolver.system_state(&target, SystemKind::Weapon),
        Some(SystemState::Enabled)
    );
    assert_eq!(
        resolver.system_state(&target, SystemKind::Propulsion),
        Some(SystemState::Enabled)
    );

    resolver
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_MOUNT_NODE,
                DamageChannel::Internal,
                SYNTHETIC_MOUNT_INTEGRITY,
            )],
        )
        .expect("valid batch");
    assert_eq!(
        resolver.system_state(&target, SystemKind::Weapon),
        Some(SystemState::Disabled)
    );
    assert_eq!(
        resolver.system_state(&target, SystemKind::Propulsion),
        Some(SystemState::Enabled),
        "the mount and the engine are distinct systems"
    );
    assert_eq!(
        resolver.disabled_systems(&target),
        Some([SystemKind::Weapon].into_iter().collect())
    );

    resolver
        .resolve(
            Tick(4),
            &[hit_at(
                2,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_ENGINE_NODE,
                DamageChannel::Internal,
                SYNTHETIC_ENGINE_INTEGRITY,
                4,
            )],
        )
        .expect("valid batch");
    assert_eq!(
        resolver.system_state(&target, SystemKind::Propulsion),
        Some(SystemState::Disabled)
    );
    assert_eq!(
        resolver.disabled_systems(&target),
        Some(
            [SystemKind::Weapon, SystemKind::Propulsion]
                .into_iter()
                .collect()
        ),
        "both carriers are down, each disabling its own system"
    );
}

/// Scratch damage does not disable a system; a destroyed carrier does; and
/// an unresolved carrier pool is reported `Unknown` rather than guessed.
#[test]
fn accept_f29_b_system_state_tracks_damage_destruction_and_unknown() {
    let target = actor(1);
    let mount = key(SYNTHETIC_MOUNT_NODE);
    let engine = key(SYNTHETIC_ENGINE_NODE);

    let mut resolver = register(synthetic_airframe_graph(), target);
    resolver
        .resolve(
            Tick(3),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_MOUNT_NODE,
                DamageChannel::Internal,
                1.0,
            )],
        )
        .expect("valid batch");
    assert_eq!(
        resolver.part_state(&target, &mount),
        Some(PartState::Damaged)
    );
    assert_eq!(
        resolver.system_state(&target, SystemKind::Weapon),
        Some(SystemState::Enabled),
        "a scratched mount still fires"
    );

    // A graph whose engine pool is unresolved: nothing is asserted.
    let graph = DamageGraph::try_new(
        synthetic_airframe_graph().subject().clone(),
        vec![
            DamageNode::new(
                key(SYNTHETIC_ARMOR_NODE),
                DamageNodeKind::ArmorZone,
                integrity(SYNTHETIC_ARMOR_INTEGRITY),
            ),
            DamageNode::new(
                key(SYNTHETIC_HULL_NODE),
                DamageNodeKind::InternalStructure,
                integrity(SYNTHETIC_HULL_INTEGRITY),
            ),
            DamageNode::new(
                engine.clone(),
                DamageNodeKind::Engine,
                unknown_integrity("f29b.engine-integrity"),
            )
            .with_disables(SystemKind::Propulsion),
        ],
    )
    .expect("an unresolved engine pool still validates");
    let unresolved = register(graph, actor(9));
    assert_eq!(
        unresolved.system_state(&actor(9), SystemKind::Propulsion),
        Some(SystemState::Unknown),
        "an unresolved carrier pool asserts nothing"
    );
    assert_eq!(
        unresolved.disabled_systems(&actor(9)),
        Some(Default::default())
    );
}

/// "No such system" is not "the system is down": an actor whose graph
/// declares no weapon carrier reports `None` for weapons, and an
/// unregistered actor reports `None` for everything.
#[test]
fn accept_f29_b_system_state_is_none_when_no_part_declares_the_system() {
    let target = actor(1);
    let engine = key(SYNTHETIC_ENGINE_NODE);
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29b-engine-only")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(engine.clone(), DamageNodeKind::Engine, integrity(15.0))
                .with_disables(SystemKind::Propulsion),
        ],
    )
    .expect("the engine-only graph is valid");
    let resolver = register(graph, target);

    assert_eq!(resolver.system_state(&target, SystemKind::Weapon), None);
    assert_eq!(
        resolver.system_state(&target, SystemKind::Propulsion),
        Some(SystemState::Enabled)
    );
    assert_eq!(
        resolver.system_state(&actor(99), SystemKind::Propulsion),
        None
    );
    assert_eq!(resolver.disabled_systems(&actor(99)), None);
}

/// The routing never turns a refusal into a silent success: an armor hit on
/// an unknown target or an unknown node is still a named refusal, and the
/// valid part of the batch is unaffected.
#[test]
fn accept_f29_b_refusals_stay_named_on_the_armor_channel() {
    let target = actor(1);
    let mut resolver = register(synthetic_airframe_graph(), target);
    let resolution = resolver
        .resolve(
            Tick(3),
            &[
                hit(
                    1,
                    0,
                    None,
                    actor(99),
                    SYNTHETIC_HULL_NODE,
                    DamageChannel::Armor,
                    5.0,
                ),
                hit(1, 1, None, target, "tailplane", DamageChannel::Armor, 5.0),
                hit(
                    1,
                    2,
                    Some(actor(2)),
                    target,
                    SYNTHETIC_HULL_NODE,
                    DamageChannel::Armor,
                    5.0,
                ),
            ],
        )
        .expect("valid batch");

    let refusals: Vec<_> = resolution
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            DamageEventKind::HitRefused { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect();
    assert_eq!(
        refusals,
        vec![
            RefusalReason::UnknownTargetActor,
            RefusalReason::UnknownNode,
        ]
    );
    assert_eq!(
        applied(&resolution),
        vec![(key(SYNTHETIC_ARMOR_NODE), 5.0)],
        "the valid armor hit still resolves"
    );
}
