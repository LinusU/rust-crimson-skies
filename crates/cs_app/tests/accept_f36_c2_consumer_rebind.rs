//! F36-C.2 acceptance: HUD, spyglass, sound, weapons, damage, input and
//! network ownership rebind from the applied `TransferReport`, and never
//! from an aborted transfer (synthetic).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! non-negotiable behavior 5, through the F36-C session and the F36-C.2
//! consumer entries in `cs_app::interaction`. Ordinary build/test only;
//! nothing here is original data and every rebinding rule is designed — the
//! original swap lifecycle is unrecovered (F36-D).
//!
//! The tests drive the production [`InteractionSession`] to a **real**
//! applied transfer (latch, release, complete) and hand its real
//! [`TransferReport`] to the production [`apply_transfer_consumers`],
//! [`teardown_transfer_consumers`] and [`rebind_network_ownership`] entries
//! over a real `bevy` [`World`] and a real [`ActorOwnership`] table, so
//! removing the report check, the joint record or the ownership ordering
//! fails an assertion.

use bevy::ecs::world::World;
use cs_app::interaction::{
    InteractionRuntime, NetworkRebindError, TransferConsumerError, TransferConsumers,
    apply_transfer_consumers, rebind_network_ownership, teardown_transfer_consumers,
};
use cs_net::validation::ActorOwnership;
use cs_sim::interaction::{
    CameraTransfer, ControlHolder, ControlOwner, EligibilityRefusal, InitiatorMotion,
    InteractionKind, InteractionSession, InteractionState, InteractionTransaction,
    InventoryTransfer, PilotId, PilotTransfer, SYNTHETIC_HOOK_TARGET, SYNTHETIC_INITIATOR,
    SYNTHETIC_SESSION, TransferLedger, TransferPolicy, TransferReport, VelocityTransfer,
    evaluate_motion_eligibility, synthetic_docking_anchor, synthetic_docking_envelope,
    synthetic_docking_id, synthetic_docking_objective, synthetic_hook_trajectory,
};
use cs_types::Tick;
use cs_types::net::{ActorId, PeerId};

const PILOT: PilotId = PilotId(9);

fn ledger() -> TransferLedger {
    TransferLedger::new(
        SYNTHETIC_SESSION,
        [(SYNTHETIC_INITIATOR, PILOT)],
        [(SYNTHETIC_INITIATOR, 3)],
    )
}

fn good_pass() -> Result<cs_sim::world_actors::anchor::AnchorSample, EligibilityRefusal> {
    let motion = InitiatorMotion::try_new([9.4, 0.0, 0.0], [10.0, 0.0, 0.0], 1, 10).unwrap();
    evaluate_motion_eligibility(
        Tick(20),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        &motion,
        &synthetic_docking_envelope(),
    )
}

/// An aircraft-swap transaction, the policy that moves pilot, inventory,
/// camera and control to the target.
fn swap_transaction() -> InteractionTransaction {
    swap_transaction_under(TransferPolicy::aircraft_swap())
}

/// An aircraft-swap transaction under `policy`, so a test can drive a real
/// report whose camera and control fields differ.
fn swap_transaction_under(policy: TransferPolicy) -> InteractionTransaction {
    InteractionTransaction::begin(
        synthetic_docking_id(),
        InteractionKind::AircraftSwap,
        cs_sim::interaction::InteractionAuthorization::new(
            SYNTHETIC_SESSION,
            InteractionKind::AircraftSwap,
            synthetic_docking_objective(),
        ),
        policy,
    )
}

/// Drives the real session to a latched aircraft swap.
fn latched() -> (InteractionSession, cs_sim::interaction::InteractionId) {
    let tx = swap_transaction();
    let id = tx.id();
    let mut session = InteractionSession::new(ledger());
    session.open(tx).expect("opens");
    assert_eq!(
        session.observe(id, good_pass()).expect("known"),
        Ok(InteractionState::Latching)
    );
    (session, id)
}

/// Drives the real session all the way to one applied transfer and returns
/// its real report.
fn applied_swap() -> (InteractionSession, TransferReport) {
    applied_swap_under(TransferPolicy::aircraft_swap())
}

/// Drives the real session to one applied transfer under `policy` and
/// returns its real report.
fn applied_swap_under(policy: TransferPolicy) -> (InteractionSession, TransferReport) {
    let tx = swap_transaction_under(policy);
    let id = tx.id();
    let mut session = InteractionSession::new(ledger());
    session.open(tx).expect("opens");
    assert_eq!(
        session.observe(id, good_pass()).expect("known"),
        Ok(InteractionState::Latching)
    );
    session.release(id).expect("released");
    let (_outcome, report) = session
        .complete(id, &synthetic_docking_objective(), true, [6.0; 3], [5.0; 3])
        .expect("the transfer applies");
    (session, report)
}

fn world_with(session: InteractionSession) -> World {
    let mut world = World::new();
    world.insert_resource(InteractionRuntime(session));
    world
}

#[test]
fn accept_f36_c2_each_consumer_follows_the_applied_report() {
    let (session, report) = applied_swap();
    let mut world = world_with(session);

    let rebind = apply_transfer_consumers(&mut world, &report).expect("applies");
    assert!(!rebind.rebound, "the first bind is not a rebind");

    let consumers = world
        .get_resource::<TransferConsumers>()
        .expect("the consumers are published");
    let binding = consumers.bound().expect("bound to the swap");
    assert_eq!(binding.session, SYNTHETIC_SESSION);
    assert_eq!(binding.interaction, report.id);

    // Input, HUD, sound, weapons, damage and network ownership follow the
    // actor that owns pose/control after the transfer.
    assert_eq!(consumers.input(), Some(SYNTHETIC_HOOK_TARGET));
    assert_eq!(consumers.hud(), Some(SYNTHETIC_HOOK_TARGET));
    assert_eq!(consumers.sound(), Some(SYNTHETIC_HOOK_TARGET));
    assert_eq!(consumers.weapons(), Some(SYNTHETIC_HOOK_TARGET));
    assert_eq!(consumers.damage(), Some(SYNTHETIC_HOOK_TARGET));
    assert_eq!(consumers.network(), Some(SYNTHETIC_HOOK_TARGET));
    // The spyglass follows the camera binding the policy transferred.
    assert_eq!(consumers.spyglass(), Some(report.camera_actor));

    // The session's own control resolution agrees with the bindings.
    assert_eq!(
        session_control_actor(&world),
        Some(SYNTHETIC_HOOK_TARGET),
        "control_of names the same actor the consumers now fly"
    );
}

#[test]
fn accept_f36_c2_each_view_follows_its_own_report_field() {
    // A real policy whose two report fields differ: the camera moves to the
    // new actor while control stays with the initiator. The spyglass must
    // follow `camera_actor` and every other view `control_actor`, so the
    // per-consumer mapping is exercised by a report that actually
    // discriminates between the two fields rather than one where they
    // coincide.
    let (session, report) = applied_swap_under(TransferPolicy {
        velocity: VelocityTransfer::PreserveInitiator,
        pilot: PilotTransfer::MoveToTarget,
        inventory: InventoryTransfer::MoveToTarget,
        camera: CameraTransfer::FollowTarget,
        control_after_release: ControlOwner::Initiator,
    });
    assert_ne!(
        report.camera_actor, report.control_actor,
        "the fixture must distinguish the two fields or the mapping is untested"
    );
    let mut world = world_with(session);
    apply_transfer_consumers(&mut world, &report).expect("applies");

    let consumers = world
        .get_resource::<TransferConsumers>()
        .expect("the consumers are published");
    assert_eq!(consumers.spyglass(), Some(SYNTHETIC_HOOK_TARGET));
    assert_eq!(
        consumers.spyglass(),
        Some(report.camera_actor),
        "the spyglass follows the camera binding the policy transferred"
    );
    for view in [
        consumers.input(),
        consumers.hud(),
        consumers.sound(),
        consumers.weapons(),
        consumers.damage(),
        consumers.network(),
    ] {
        assert_eq!(
            view,
            Some(report.control_actor),
            "every other view follows the actor that owns pose/control"
        );
    }
    assert_eq!(consumers.input(), Some(SYNTHETIC_INITIATOR));
}

/// Reads the runtime's control holder back as an actor, the way a consumer
/// would.
fn session_control_actor(world: &World) -> Option<ActorId> {
    let runtime = world.get_resource::<InteractionRuntime>()?;
    match runtime.0.control_of(SYNTHETIC_INITIATOR) {
        ControlHolder::Actor(actor) => Some(actor),
        ControlHolder::Latch(_) => None,
    }
}

#[test]
fn accept_f36_c2_swap_rebinds_a_second_time_and_drops_the_first_aircraft() {
    let (session, first) = applied_swap();
    let mut world = world_with(session);
    apply_transfer_consumers(&mut world, &first).expect("applies");

    // A second swap from the aircraft the first one moved control to. The
    // session's ledger holds no pilot for the target, so its own ledger is
    // rebuilt for the second attempt: this is the consumer record under test,
    // driven by two real applied reports.
    let second_id = cs_sim::interaction::InteractionId::new(
        SYNTHETIC_SESSION,
        2,
        SYNTHETIC_HOOK_TARGET,
        SYNTHETIC_INITIATOR,
    );
    let second_tx = InteractionTransaction::begin(
        second_id,
        InteractionKind::AircraftSwap,
        cs_sim::interaction::InteractionAuthorization::new(
            SYNTHETIC_SESSION,
            InteractionKind::AircraftSwap,
            synthetic_docking_objective(),
        ),
        TransferPolicy::aircraft_swap(),
    );
    let mut session = InteractionSession::new(TransferLedger::new(
        SYNTHETIC_SESSION,
        [(SYNTHETIC_HOOK_TARGET, PILOT)],
        [],
    ));
    session.open(second_tx).expect("opens");
    assert_eq!(
        session.observe(second_id, good_pass()).expect("known"),
        Ok(InteractionState::Latching)
    );
    session.release(second_id).expect("released");
    let (_outcome, second) = session
        .complete(
            second_id,
            &synthetic_docking_objective(),
            true,
            [6.0; 3],
            [5.0; 3],
        )
        .expect("the second swap applies");
    *world
        .get_resource_mut::<InteractionRuntime>()
        .expect("runtime") = InteractionRuntime(session);

    let rebind = apply_transfer_consumers(&mut world, &second).expect("applies");
    assert!(rebind.rebound, "the second pass rebinds");
    assert_eq!(rebind.binding.interaction, second_id);

    let consumers = world
        .get_resource::<TransferConsumers>()
        .expect("the consumers are published");
    // Every consumer left the first swap's aircraft: nothing still names it.
    assert_eq!(consumers.input(), Some(SYNTHETIC_INITIATOR));
    assert_eq!(consumers.hud(), Some(SYNTHETIC_INITIATOR));
    assert_eq!(consumers.sound(), Some(SYNTHETIC_INITIATOR));
    assert_eq!(consumers.weapons(), Some(SYNTHETIC_INITIATOR));
    assert_eq!(consumers.damage(), Some(SYNTHETIC_INITIATOR));
    assert_eq!(consumers.network(), Some(SYNTHETIC_INITIATOR));
    assert_eq!(consumers.spyglass(), Some(second.camera_actor));
}

#[test]
fn accept_f36_c2_aborted_transfer_never_rebinds_the_consumers() {
    // A swap aborted mid-latch (destroyed carrier): the session publishes no
    // report, and a fabricated report claiming that aborted interaction is
    // refused and moves nothing.
    let (mut session, id) = latched();
    let aborts = session.actor_destroyed(SYNTHETIC_HOOK_TARGET);
    assert_eq!(aborts.len(), 1);
    assert_eq!(session.state(id), Some(InteractionState::Aborted));

    let fabricated = cs_sim::interaction::TransferReport {
        id,
        velocity_m_s: [6.0; 3],
        pilot_moved: Some(PILOT),
        inventory_moved: 3,
        camera_actor: SYNTHETIC_HOOK_TARGET,
        control_actor: SYNTHETIC_HOOK_TARGET,
    };
    let mut world = world_with(session);
    let refused = apply_transfer_consumers(&mut world, &fabricated).unwrap_err();
    assert_eq!(
        refused,
        TransferConsumerError::NotCompleted {
            id,
            state: Some(InteractionState::Aborted)
        }
    );
    assert_eq!(
        world.get_resource::<TransferConsumers>(),
        None,
        "an aborted transfer publishes no consumer binding at all"
    );

    // The same is true of an interaction the live session never knew.
    let unknown = cs_sim::interaction::TransferReport {
        id: cs_sim::interaction::InteractionId::new(
            SYNTHETIC_SESSION,
            99,
            SYNTHETIC_INITIATOR,
            SYNTHETIC_HOOK_TARGET,
        ),
        ..fabricated
    };
    let refused = apply_transfer_consumers(&mut world, &unknown).unwrap_err();
    assert!(matches!(
        refused,
        TransferConsumerError::NotCompleted { state: None, .. }
    ));
    assert_eq!(world.get_resource::<TransferConsumers>(), None);
}

#[test]
fn accept_f36_c2_retry_tears_down_bindings_and_a_stale_report_is_refused() {
    let (session, report) = applied_swap();
    let mut world = world_with(session);
    apply_transfer_consumers(&mut world, &report).expect("applies");
    assert!(world.get_resource::<TransferConsumers>().is_some());

    // Retry replaces the session generation and restores the initial
    // bindings (the F36-C retry path).
    {
        let mut runtime = world
            .get_resource_mut::<InteractionRuntime>()
            .expect("runtime");
        runtime.on_retry(SYNTHETIC_SESSION + 1);
        assert_eq!(
            runtime.0.ledger().pilot_of(SYNTHETIC_INITIATOR),
            Some(PILOT),
            "the retry restored the starting pilot binding"
        );
    }

    // The teardown for the generation that ended clears the bindings.
    assert!(teardown_transfer_consumers(&mut world, SYNTHETIC_SESSION));
    assert_eq!(
        world
            .get_resource::<TransferConsumers>()
            .and_then(TransferConsumers::bound),
        None,
        "the teardown dropped the binding"
    );

    // A late report from the replaced generation is refused, so a consumer
    // can never be rebound into a retried world.
    let refused = apply_transfer_consumers(&mut world, &report).unwrap_err();
    assert_eq!(
        refused,
        TransferConsumerError::StaleSession {
            live: SYNTHETIC_SESSION + 1,
            report: SYNTHETIC_SESSION,
        }
    );
    assert_eq!(
        world
            .get_resource::<TransferConsumers>()
            .and_then(TransferConsumers::bound),
        None
    );
}

#[test]
fn accept_f36_c2_teardown_refuses_a_different_sessions_bindings() {
    let (session, report) = applied_swap();
    let mut world = world_with(session);
    apply_transfer_consumers(&mut world, &report).expect("applies");

    // A late teardown for another generation cannot blank the live one.
    assert!(!teardown_transfer_consumers(
        &mut world,
        SYNTHETIC_SESSION + 4
    ));
    assert!(world.get_resource::<TransferConsumers>().is_some());
    assert!(teardown_transfer_consumers(&mut world, SYNTHETIC_SESSION));

    // With nothing bound there is nothing to clear, and the caller is told.
    assert!(!teardown_transfer_consumers(&mut world, SYNTHETIC_SESSION));
}

#[test]
fn accept_f36_c2_a_consumers_record_without_a_runtime_is_dropped() {
    let (session, report) = applied_swap();
    let mut world = world_with(session);
    apply_transfer_consumers(&mut world, &report).expect("applies");

    // The session ends and its runtime is removed; the record that outlived
    // it must not stay readable.
    world.remove_resource::<InteractionRuntime>();
    let refused = apply_transfer_consumers(&mut world, &report).unwrap_err();
    assert_eq!(refused, TransferConsumerError::NoRuntime);
    let consumers = world
        .get_resource::<TransferConsumers>()
        .expect("a reader finds a resource it can tell from a live one");
    assert_eq!(consumers.bound(), None, "and it holds no binding");
    for view in [
        consumers.input(),
        consumers.hud(),
        consumers.spyglass(),
        consumers.sound(),
        consumers.weapons(),
        consumers.damage(),
        consumers.network(),
    ] {
        assert_eq!(
            view, None,
            "no consumer keeps a binding the runtime cannot stand behind"
        );
    }
}

#[test]
fn accept_f36_c2_network_ownership_follows_the_applied_report() {
    let (_session, report) = applied_swap();
    let peer = PeerId::new(1).expect("peer 1");
    let mut ownership = ActorOwnership::new();
    ownership.bind(peer, SYNTHETIC_INITIATOR).expect("binds");

    let rebind = rebind_network_ownership(&mut ownership, peer, &report).expect("rebinds");
    assert_eq!(rebind.bound, SYNTHETIC_HOOK_TARGET);
    assert_eq!(rebind.released, Some(SYNTHETIC_INITIATOR));
    assert_eq!(ownership.actor_of(peer), Some(SYNTHETIC_HOOK_TARGET));
    assert_eq!(ownership.owner(SYNTHETIC_INITIATOR), None);
}

#[test]
fn accept_f36_c2_network_ownership_refuses_a_peer_that_is_not_the_pilot() {
    let (_session, report) = applied_swap();
    let pilot = PeerId::new(1).expect("peer 1");
    let other = PeerId::new(2).expect("peer 2");
    let mut ownership = ActorOwnership::new();
    ownership.bind(pilot, SYNTHETIC_INITIATOR).expect("binds");

    let refused = rebind_network_ownership(&mut ownership, other, &report).unwrap_err();
    assert_eq!(
        refused,
        NetworkRebindError::PeerDoesNotOwnInitiator {
            peer: other,
            actor: SYNTHETIC_INITIATOR,
        }
    );
    // Nothing moved: the pilot still owns the aircraft it flew.
    assert_eq!(ownership.actor_of(pilot), Some(SYNTHETIC_INITIATOR));
    assert_eq!(ownership.owner(SYNTHETIC_HOOK_TARGET), None);
}

#[test]
fn accept_f36_c2_network_ownership_refuses_an_aircraft_another_pilot_owns() {
    let (_session, report) = applied_swap();
    let pilot = PeerId::new(1).expect("peer 1");
    let squatter = PeerId::new(2).expect("peer 2");
    let mut ownership = ActorOwnership::new();
    ownership.bind(pilot, SYNTHETIC_INITIATOR).expect("binds");
    ownership
        .bind(squatter, SYNTHETIC_HOOK_TARGET)
        .expect("binds");

    let refused = rebind_network_ownership(&mut ownership, pilot, &report).unwrap_err();
    assert_eq!(
        refused,
        NetworkRebindError::DestinationOwned {
            peer: pilot,
            actor: SYNTHETIC_HOOK_TARGET,
            owner: squatter,
        }
    );
    // The refusal left the pilot with the aircraft it owned — never nothing,
    // and the squatter's binding is untouched.
    assert_eq!(ownership.actor_of(pilot), Some(SYNTHETIC_INITIATOR));
    assert_eq!(ownership.actor_of(squatter), Some(SYNTHETIC_HOOK_TARGET));
}

#[test]
fn accept_f36_c2_network_ownership_keeps_the_binding_when_control_does_not_move() {
    // A policy that leaves control with the initiator: the report names the
    // initiator as the control actor, so there is nothing to rebind.
    let (_session, report) = applied_swap();
    let unchanged = cs_sim::interaction::TransferReport {
        control_actor: SYNTHETIC_INITIATOR,
        camera_actor: SYNTHETIC_INITIATOR,
        ..report
    };
    let peer = PeerId::new(1).expect("peer 1");
    let mut ownership = ActorOwnership::new();
    ownership.bind(peer, SYNTHETIC_INITIATOR).expect("binds");

    let rebind = rebind_network_ownership(&mut ownership, peer, &unchanged).expect("rebinds");
    assert_eq!(rebind.bound, SYNTHETIC_INITIATOR);
    assert_eq!(rebind.released, None);
    assert_eq!(ownership.actor_of(peer), Some(SYNTHETIC_INITIATOR));
}

#[test]
fn accept_f36_c2_network_ownership_refuses_a_non_pilot_even_when_control_does_not_move() {
    // The no-control-moved path must not be a way around the pilot check: a
    // peer that owns nothing is refused there too, so no caller can be told
    // it is bound to an aircraft the table says it does not fly.
    let (_session, report) = applied_swap();
    let unchanged = cs_sim::interaction::TransferReport {
        control_actor: SYNTHETIC_INITIATOR,
        camera_actor: SYNTHETIC_INITIATOR,
        ..report
    };
    let pilot = PeerId::new(1).expect("peer 1");
    let other = PeerId::new(2).expect("peer 2");
    let mut ownership = ActorOwnership::new();
    ownership.bind(pilot, SYNTHETIC_INITIATOR).expect("binds");

    let refused = rebind_network_ownership(&mut ownership, other, &unchanged).unwrap_err();
    assert_eq!(
        refused,
        NetworkRebindError::PeerDoesNotOwnInitiator {
            peer: other,
            actor: SYNTHETIC_INITIATOR,
        }
    );
    // Nothing moved for either peer.
    assert_eq!(ownership.actor_of(pilot), Some(SYNTHETIC_INITIATOR));
    assert_eq!(ownership.actor_of(other), None);
}

#[test]
fn accept_f36_c2_one_record_seven_bindings_no_partial_write() {
    // The joint-record property: a reader that sees input bound sees all
    // seven bound to the same report, and an aborted pass between two
    // applied ones leaves the previous complete record, never a mix.
    let (session, first) = applied_swap();
    let mut world = world_with(session);
    apply_transfer_consumers(&mut world, &first).expect("applies");

    // An aborted second attempt (target destroyed at completion) publishes
    // nothing and leaves the first record intact and complete.
    let second_id = cs_sim::interaction::InteractionId::new(
        SYNTHETIC_SESSION,
        2,
        SYNTHETIC_INITIATOR,
        SYNTHETIC_HOOK_TARGET,
    );
    let second_tx = InteractionTransaction::begin(
        second_id,
        InteractionKind::AircraftSwap,
        cs_sim::interaction::InteractionAuthorization::new(
            SYNTHETIC_SESSION,
            InteractionKind::AircraftSwap,
            synthetic_docking_objective(),
        ),
        TransferPolicy::aircraft_swap(),
    );
    {
        let session = &mut world
            .get_resource_mut::<InteractionRuntime>()
            .expect("runtime")
            .0;
        session.open(second_tx).expect("opens");
        assert_eq!(
            session.observe(second_id, good_pass()).expect("known"),
            Ok(InteractionState::Latching)
        );
        session.release(second_id).expect("released");
        assert!(
            session
                .complete(
                    second_id,
                    &synthetic_docking_objective(),
                    false,
                    [6.0; 3],
                    [5.0; 3]
                )
                .is_err(),
            "a lost target aborts instead of completing"
        );
        assert_eq!(session.state(second_id), Some(InteractionState::Aborted));
    }

    let refused = apply_transfer_consumers(
        &mut world,
        &cs_sim::interaction::TransferReport {
            id: second_id,
            velocity_m_s: [0.0; 3],
            pilot_moved: None,
            inventory_moved: 0,
            camera_actor: SYNTHETIC_HOOK_TARGET,
            control_actor: SYNTHETIC_HOOK_TARGET,
        },
    )
    .unwrap_err();
    assert!(matches!(
        refused,
        TransferConsumerError::NotCompleted {
            state: Some(InteractionState::Aborted),
            ..
        }
    ));

    // The first record is still whole: every consumer still names the one
    // aircraft that actually committed.
    let consumers = world
        .get_resource::<TransferConsumers>()
        .expect("the previous record survives");
    assert_eq!(consumers.bound().map(|b| b.interaction), Some(first.id));
    let committed = [
        consumers.input(),
        consumers.hud(),
        consumers.weapons(),
        consumers.damage(),
        consumers.network(),
    ];
    assert!(committed.iter().all(|a| *a == Some(SYNTHETIC_HOOK_TARGET)));
    assert_eq!(consumers.spyglass(), Some(first.camera_actor));
}
