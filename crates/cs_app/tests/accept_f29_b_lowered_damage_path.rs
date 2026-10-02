//! Acceptance scenarios F29-B: the declared damage content reaches the
//! session resolver through the production lowering boundary, and there the
//! armor channel, the internal channel and system disablement behave as
//! declared.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-B`. Task test prefix: `accept_f29_b_`.
//!
//! These tests drive [`lower_graph`] / [`lower_policy`] and the
//! [`DamageResolver`] — the production path a content import takes — never a
//! test-only reimplementation. Removing the lowering, the channel routing or
//! the system-state derivation fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::damage::{DamageLowerError, lower_graph, lower_policy};
use cs_content::damage::{
    AttributionRule as DeclaredAttribution, DamageNodeKind as DeclaredNodeKind,
    DeclaredDamageGraph, DeclaredDamageNode, GraphRules, GraphSubjectKind,
    SystemKind as DeclaredSystem, declared_synthetic_airframe_damage,
};
use cs_sim::damage::{
    ActorId, DamageChannel, DamageEventKind, DamageNodeKey, DamageResolver, HitEvent, HitEventId,
    PartState, SystemKind, SystemState,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 23;
const PRODUCER: u32 = 1;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn hit(sequence: u32, node: &str, channel: DamageChannel, damage: f64) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session(SESSION),
            tick: Tick(2),
            producer: 1,
            sequence,
        },
        Some(actor(2)),
        actor(1),
        key(node),
        channel,
        damage,
    )
    .expect("a finite, non-negative hit")
}

fn registered(declared: &DeclaredDamageGraph) -> DamageResolver {
    let graph = lower_graph(declared).expect("the declared graph lowers");
    let policy = lower_policy(declared).expect("the declared policy lowers");
    let mut resolver = DamageResolver::new(session(SESSION), PRODUCER);
    resolver
        .register_actor(actor(1), graph, policy)
        .expect("the lowered actor registers");
    resolver
}

/// AC02 minimum scenario through the production boundary: the declared
/// synthetic airframe lowers and the armor and internal channels then leave
/// distinguishable pools, with no multiplier anywhere.
#[test]
fn accept_f29_b_lowered_graph_routes_armor_and_internal_distinguishably() {
    let declared = declared_synthetic_airframe_damage();

    let mut armored = registered(&declared);
    armored
        .resolve(Tick(2), &[hit(0, "hull", DamageChannel::Armor, 25.0)])
        .expect("valid batch");
    assert_eq!(
        armored.remaining_integrity(&actor(1), &key("hull")),
        Some(35.0),
        "the declared armor pool absorbs twenty and the hull takes the five-point remainder"
    );
    assert_eq!(
        armored.part_state(&actor(1), &key("nose_armor")),
        Some(PartState::Destroyed)
    );

    let mut internal = registered(&declared);
    internal
        .resolve(Tick(2), &[hit(0, "hull", DamageChannel::Internal, 25.0)])
        .expect("valid batch");
    assert_eq!(
        internal.remaining_integrity(&actor(1), &key("hull")),
        Some(15.0),
        "the internal channel lands all twenty-five on the hull"
    );
    assert_eq!(
        internal.part_state(&actor(1), &key("nose_armor")),
        Some(PartState::Intact),
        "and leaves the declared armor untouched"
    );
}

/// A destroyed carrier's declared system is disabled, and the state a gate
/// reads is the same transition the emitted event records.
#[test]
fn accept_f29_b_lowered_graph_reports_a_disabled_system() {
    let declared = declared_synthetic_airframe_damage();
    let mut resolver = registered(&declared);

    assert_eq!(
        resolver.system_state(&actor(1), SystemKind::Weapon),
        Some(SystemState::Enabled)
    );
    let resolution = resolver
        .resolve(
            Tick(2),
            &[hit(0, "gun_mount_1", DamageChannel::Internal, 10.0)],
        )
        .expect("valid batch");
    assert!(
        resolution.events.iter().any(|event| matches!(
            &event.kind,
            DamageEventKind::SystemDisabled {
                node,
                system: SystemKind::Weapon,
            } if node == &key("gun_mount_1")
        )),
        "the declared mount's destruction emits its system transition: {:?}",
        resolution.events
    );
    assert_eq!(
        resolver.system_state(&actor(1), SystemKind::Weapon),
        Some(SystemState::Disabled)
    );
    assert_eq!(
        resolver.system_state(&actor(1), SystemKind::Propulsion),
        Some(SystemState::Enabled)
    );
}

/// The lowering carries a declared attribution rule and a declared guard
/// without an overflow; the guard's overkill then continues into the part
/// the shot named.
#[test]
fn accept_f29_b_lowered_guard_without_overflow_keeps_the_overkill() {
    let declared = DeclaredDamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29b-lowered")
            .expect("a valid airframe id"),
        cs_types::content::Origin::SyntheticFixture,
        GraphSubjectKind::Aircraft,
        GraphRules {
            lethal_attribution: Resolved::Known(Known::new(
                DeclaredAttribution::FirstLethalHit,
                Provenance::designed(ClaimId::new("f29b.test").expect("valid claim id")),
            )),
        },
        vec![
            DeclaredDamageNode {
                key: cs_content::damage::DamageNodeKey::new("plate").expect("valid key"),
                kind: DeclaredNodeKind::ArmorZone,
                scene_binding: None,
                integrity: Resolved::Known(Known::new(
                    10.0,
                    Provenance::designed(ClaimId::new("f29b.test").expect("valid claim id")),
                )),
                lethal: false,
                disables: None,
                guarded_by: None,
                overflow: None,
            },
            DeclaredDamageNode {
                key: cs_content::damage::DamageNodeKey::new("hull").expect("valid key"),
                kind: DeclaredNodeKind::InternalStructure,
                scene_binding: None,
                integrity: Resolved::Known(Known::new(
                    40.0,
                    Provenance::designed(ClaimId::new("f29b.test").expect("valid claim id")),
                )),
                lethal: true,
                disables: None,
                guarded_by: Some(
                    cs_content::damage::DamageNodeKey::new("plate").expect("valid key"),
                ),
                overflow: None,
            },
            DeclaredDamageNode {
                key: cs_content::damage::DamageNodeKey::new("engine").expect("valid key"),
                kind: DeclaredNodeKind::Engine,
                scene_binding: None,
                integrity: Resolved::Known(Known::new(
                    5.0,
                    Provenance::designed(ClaimId::new("f29b.test").expect("valid claim id")),
                )),
                lethal: false,
                disables: Some(DeclaredSystem::Propulsion),
                guarded_by: None,
                overflow: None,
            },
        ],
        Provenance::designed(ClaimId::new("f29b.test").expect("valid claim id")),
    )
    .expect("the declared guard graph is valid");

    let mut resolver = registered(&declared);
    resolver
        .resolve(Tick(2), &[hit(0, "hull", DamageChannel::Armor, 15.0)])
        .expect("valid batch");
    assert_eq!(
        resolver.remaining_integrity(&actor(1), &key("hull")),
        Some(35.0),
        "the declared guard's overkill continues into the guarded hull"
    );

    // The lowering boundary still refuses an unmeasured attribution rule.
    let mut nodes = declared.nodes().to_vec();
    nodes.pop();
    let unresolved = DeclaredDamageGraph::try_new(
        declared.subject().clone(),
        declared.origin().clone(),
        GraphSubjectKind::Aircraft,
        GraphRules {
            lethal_attribution: Resolved::Unknown {
                claim_id: ClaimId::new("f29b.test.unknown-rule").expect("valid"),
                reason: "attribution unmeasured".to_owned(),
            },
        },
        nodes,
        declared.provenance().clone(),
    )
    .expect("a graph with an unknown rule still validates");
    assert!(matches!(
        lower_policy(&unresolved),
        Err(DamageLowerError::UnknownAttribution { .. })
    ));
}
