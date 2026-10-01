//! F57-A acceptance: the authoritative network state, its generations and its
//! once-per-generation destruction gate.
//!
//! The sheet's `### F57-A` minimum scenario is "Inject latency/loss/reordering;
//! no duplicate destruction or permanent ghost aircraft", whose *authority* half
//! lives here: however many reports arrive, for whichever tick, from whichever
//! producer, one actor's destruction is awarded once. The transport half — the
//! same scenario driven over a lossy, delayed, reordered link — is
//! `crates/cs_app/tests/accept_f57_a_latency_loss_and_reordering.rs`, and the
//! schema half is `crates/cs_net/tests/accept_f57_a_snapshot_schema.rs`.
//!
//! Every test here calls production code in `cs_sim::net_state`.

use cs_sim::net_state::{
    ActorGeneration, Destruction, NetActorState, NetControlMode, NetDamage, NetFlight,
    NetLifecycle, NetPose, NetStateError, NetStateLedger, NetWeapons,
};
use cs_types::Tick;
use cs_types::net::{ActorAllocator, ActorId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

const SESSION: SessionId = match SessionId::new(21) {
    Some(id) => id,
    None => unreachable!(),
};

/// A ledger with three live actors, allocated through the real allocator.
fn three_actor_ledger() -> (NetStateLedger, [ActorId; 3]) {
    let mut ledger = NetStateLedger::new(SESSION);
    let mut allocator = ActorAllocator::new(SESSION);
    let mut actors = [ActorId {
        session: SESSION,
        serial: 0,
    }; 3];
    for slot in &mut actors {
        let actor = allocator.allocate().expect("serial space");
        let state = NetActorState::spawn(
            actor,
            WorldPosition::try_new([0.0, 100.0, 0.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        );
        ledger.spawn(state).expect("the first spawn succeeds");
        *slot = actor;
    }
    (ledger, actors)
}

fn generation_of(ledger: &NetStateLedger, actor: ActorId) -> ActorGeneration {
    ledger
        .generation(actor)
        .expect("the ledger knows the actor")
}

/// The ledger allocates a nonzero generation per actor, refuses a duplicate
/// registration of the same id, and refuses an id from another session. Actor
/// serials are never recycled, so a second registration is a caller bug rather
/// than a respawn.
#[test]
fn accept_f57_a_generations_are_allocated_once_per_actor_and_never_recycled() {
    let (mut ledger, actors) = three_actor_ledger();
    assert_eq!(ledger.actor_count(), 3);
    assert_eq!(ledger.session(), SESSION);

    for (index, actor) in actors.iter().enumerate() {
        let generation = generation_of(&ledger, *actor);
        assert_eq!(
            generation.get(),
            index as u16 + 1,
            "generations are allocated from 1 in registration order"
        );
        assert!(
            ActorGeneration::try_new(0).is_none(),
            "generation zero must never be a live generation"
        );
    }

    // A duplicate registration under a live id is refused.
    let duplicate = NetActorState::spawn(
        actors[0],
        WorldPosition::try_new([1.0, 0.0, 0.0]).expect("finite"),
        Quaternion::IDENTITY,
        10,
    );
    assert_eq!(
        ledger.spawn(duplicate),
        Err(NetStateError::DuplicateActor { actor: actors[0] })
    );

    // A foreign session, and a malformed id, are both refused by name.
    let foreign = SessionId::new(77).expect("nonzero");
    let foreign_actor = ActorId {
        session: foreign,
        serial: 9,
    };
    let state = NetActorState::spawn(
        foreign_actor,
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
        Quaternion::IDENTITY,
        10,
    );
    assert_eq!(
        ledger.spawn(state),
        Err(NetStateError::InvalidActorId {
            actor: foreign_actor
        })
    );
    let zero_serial = ActorId {
        session: SESSION,
        serial: 0,
    };
    let state = NetActorState::spawn(
        zero_serial,
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
        Quaternion::IDENTITY,
        10,
    );
    assert_eq!(
        ledger.spawn(state),
        Err(NetStateError::InvalidActorId { actor: zero_serial })
    );
}

/// Publishing state for a stale or foreign generation is refused, so a late
/// packet or a reused buffer can never write into a live actor's record. This is
/// the server-side half of "interpolation buffers separate actor generations".
#[test]
fn accept_f57_a_publishing_with_a_stale_generation_is_refused() {
    let (mut ledger, actors) = three_actor_ledger();
    let target = actors[1];
    let owned = generation_of(&ledger, target);
    let other = ActorGeneration::try_new(owned.get() + 1).expect("next generation is valid");

    let stale = NetActorState {
        actor: target,
        generation: other,
        ..NetActorState::spawn(
            target,
            WorldPosition::try_new([5.0, 5.0, 5.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        )
    };
    assert_eq!(
        ledger.publish(stale),
        Err(NetStateError::StaleGeneration {
            actor: target,
            presented: other,
            owned,
        })
    );

    // The refused write changed nothing.
    let state = ledger.state(target).expect("the actor is known");
    assert_eq!(state.generation, owned);
    assert_eq!(state.pose.position.y(), 100.0);

    // The owned generation applies.
    let good = NetActorState {
        generation: owned,
        ..NetActorState::spawn(
            target,
            WorldPosition::try_new([5.0, 5.0, 5.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        )
    };
    assert_eq!(ledger.publish(good), Ok(()));
    assert_eq!(
        ledger
            .state(target)
            .expect("the actor is known")
            .pose
            .position
            .x(),
        5.0
    );

    // A well-formed but unregistered actor is refused as unknown, not created.
    let unknown = ActorId {
        session: SESSION,
        serial: 999,
    };
    assert_eq!(
        ledger.publish(NetActorState::spawn(
            unknown,
            WorldPosition::try_new([0.0; 3]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        )),
        Err(NetStateError::UnknownActor { actor: unknown })
    );
}

/// The destruction gate: one award per `(actor, generation)`, whatever the tick
/// and whatever reports it. A second resolver batch, a retransmitted snapshot
/// and a reconnect replay all land here and none of them awards a second kill.
#[test]
fn accept_f57_a_destruction_is_awarded_once_per_generation() {
    let (mut ledger, actors) = three_actor_ledger();
    let target = actors[2];
    let generation = generation_of(&ledger, target);

    assert!(!ledger.destruction_recorded(target));
    assert_eq!(
        ledger.record_destruction(target, generation, Tick(120)),
        Ok(Destruction::Recorded { tick: Tick(120) })
    );
    assert!(ledger.destruction_recorded(target));
    assert_eq!(
        ledger.state(target).expect("the actor is known").lifecycle,
        NetLifecycle::Destroyed
    );

    // Every later report, on any tick, is absorbed with the first recorded tick.
    for later in [Tick(121), Tick(400), Tick(0)] {
        let outcome = ledger
            .record_destruction(target, generation, later)
            .expect("the actor is known");
        assert_eq!(
            outcome,
            Destruction::AlreadyRecorded { tick: Tick(120) },
            "a repeated report on tick {} must not award again or move the tick",
            later.0
        );
        assert!(!outcome.awarded(), "only the first report awards");
        assert_eq!(outcome.tick(), Tick(120));
    }

    // A different actor's destruction is independent.
    let other = actors[0];
    assert_eq!(
        ledger.record_destruction(other, generation_of(&ledger, other), Tick(150)),
        Ok(Destruction::Recorded { tick: Tick(150) })
    );
    assert!(ledger.destruction_recorded(other));

    // Once destroyed, no later state write and no non-destruction lifecycle may
    // revive the record.
    let revive = NetActorState {
        actor: target,
        generation,
        lifecycle: NetLifecycle::Alive,
        ..NetActorState::spawn(
            target,
            WorldPosition::try_new([9.0, 9.0, 9.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        )
    };
    assert_eq!(
        ledger.publish(revive),
        Err(NetStateError::AlreadyTerminal {
            actor: target,
            generation,
            lifecycle: NetLifecycle::Destroyed,
        })
    );
    assert_eq!(
        ledger.end_lifecycle(target, generation, NetLifecycle::Despawned),
        Err(NetStateError::AlreadyTerminal {
            actor: target,
            generation,
            lifecycle: NetLifecycle::Destroyed,
        })
    );

    // The dedup state is bounded by actor lifetime: forgetting the actor drops
    // the record, and a later report is then simply unknown rather than a second
    // award.
    ledger.forget(target).expect("the actor is known");
    assert!(!ledger.destruction_recorded(target));
    assert_eq!(ledger.actor_count(), 2);
}

/// A bailout is not a kill: the first terminal lifecycle for a generation is the
/// one kept, so a later destruction report cannot turn a bailout into an award.
#[test]
fn accept_f57_a_a_bailout_is_not_turned_into_a_destruction() {
    let (mut ledger, actors) = three_actor_ledger();
    let target = actors[0];
    let generation = generation_of(&ledger, target);

    assert_eq!(
        ledger.end_lifecycle(target, generation, NetLifecycle::BailedOut),
        Ok(())
    );
    assert_eq!(
        ledger.state(target).expect("the actor is known").lifecycle,
        NetLifecycle::BailedOut
    );
    assert_eq!(
        ledger.record_destruction(target, generation, Tick(200)),
        Err(NetStateError::AlreadyTerminal {
            actor: target,
            generation,
            lifecycle: NetLifecycle::BailedOut,
        })
    );
    assert!(
        !ledger.destruction_recorded(target),
        "a bailout must not leave a destruction record behind"
    );
    // And an `Alive` lifecycle is not a lifecycle transition at all.
    assert_eq!(
        ledger.end_lifecycle(
            actors[1],
            generation_of(&ledger, actors[1]),
            NetLifecycle::Alive
        ),
        Err(NetStateError::OutOfRange { field: "lifecycle" })
    );
}

/// Input acknowledgment is monotonic and is the *only* thing a client input
/// touches: rounds, boost capacity and pose are unchanged by an accepted input,
/// so a client's predicted shot cannot become authoritative (F57 AC02's
/// authority half).
#[test]
fn accept_f57_a_input_acknowledgment_is_monotonic_and_changes_nothing_else() {
    let (mut ledger, actors) = three_actor_ledger();
    let target = actors[0];
    let mut state = *ledger.state(target).expect("the actor is known");
    state.weapons.primary_rounds = 120;
    state.flight.boost_capacity = 0.25;
    ledger.publish(state).expect("the owned generation applies");
    let before = *ledger.state(target).expect("the actor is known");

    assert_eq!(ledger.acknowledged_input(), 0);
    assert_eq!(
        ledger.observe_input(5, Tick(30)),
        Ok(cs_sim::net_state::InputAck {
            sequence: 5,
            tick: Tick(30)
        })
    );
    assert_eq!(ledger.acknowledged_input(), 5);

    // A replayed or reordered sequence is refused, whatever its tick.
    for replay in [5_u32, 4, 0] {
        assert_eq!(
            ledger.observe_input(replay, Tick(31)),
            Err(NetStateError::StaleInput {
                presented: replay,
                acknowledged: 5
            })
        );
    }
    assert_eq!(ledger.acknowledged_input(), 5);

    let after = *ledger.state(target).expect("the actor is known");
    assert_eq!(after.weapons.primary_rounds, before.weapons.primary_rounds);
    assert_eq!(after.flight.boost_capacity, before.flight.boost_capacity);
    assert_eq!(after.pose.position, before.pose.position);
    assert_eq!(after.lifecycle, before.lifecycle);
}

/// The state boundary rejects non-finite input and out-of-range fractions, so
/// nothing non-finite ever reaches a quantizer (contract FLIGHT-PHYSICS: "Reject
/// nonfinite inputs at boundaries").
#[test]
fn accept_f57_a_nonfinite_and_out_of_range_state_is_refused() {
    let (mut ledger, actors) = three_actor_ledger();
    let target = actors[2];
    let good = *ledger.state(target).expect("the actor is known");

    let mut broken = good;
    broken.linear_velocity_mps[1] = f64::NAN;
    assert_eq!(
        ledger.publish(broken),
        Err(NetStateError::OutOfRange {
            field: "state.linear_velocity_mps[1]"
        })
    );

    let mut broken = good;
    broken.angular_velocity_radps[2] = f64::INFINITY;
    assert_eq!(
        ledger.publish(broken),
        Err(NetStateError::OutOfRange {
            field: "state.angular_velocity_radps[2]"
        })
    );

    for (field, mutate) in [
        ("state.flight.throttle", 0_usize),
        ("state.flight.engine_spool", 1),
        ("state.flight.boost_capacity", 2),
    ] {
        let mut broken = good;
        broken.flight = NetFlight {
            throttle: 0.5,
            engine_spool: 0.5,
            boost_capacity: 0.5,
        };
        match mutate {
            0 => broken.flight.throttle = 1.5,
            1 => broken.flight.engine_spool = -0.1,
            _ => broken.flight.boost_capacity = f64::NAN,
        }
        assert_eq!(
            ledger.publish(broken),
            Err(NetStateError::OutOfRange { field }),
            "{field} must be refused before it can be quantized"
        );
    }

    let mut broken = good;
    broken.damage = NetDamage {
        remaining: 1.2,
        disabled_mounts: 0,
    };
    assert_eq!(
        ledger.publish(broken),
        Err(NetStateError::OutOfRange {
            field: "state.damage.remaining"
        })
    );

    // More rounds than the wire's declared 16-bit budget can carry is refused at
    // the authoritative number, not truncated on the way out.
    let mut broken = good;
    broken.weapons = NetWeapons {
        primary_rounds: NetWeapons::MAX_ROUNDS + 1,
        secondary_rounds: 10,
        selected_bank: 0,
    };
    assert_eq!(
        ledger.publish(broken),
        Err(NetStateError::OutOfRange {
            field: "state.weapons.rounds"
        })
    );

    // An unknown selected bank is refused rather than mapped onto a neighbour.
    let mut broken = good;
    broken.weapons.selected_bank = 2;
    assert_eq!(
        ledger.publish(broken),
        Err(NetStateError::OutOfRange {
            field: "state.weapons.selected_bank"
        })
    );

    // None of the refused writes reached the ledger.
    let after = *ledger.state(target).expect("the actor is known");
    assert_eq!(after, good);
    assert_eq!(
        after.pose.position,
        WorldPosition::try_new([0.0, 100.0, 0.0]).expect("finite")
    );
}

/// The lifecycle and control vocabularies are closed sets with stable labels, and
/// the pose record is the canonical world space the FLIGHT-PHYSICS contract
/// names. Nothing here is a claim about an original measurement.
#[test]
fn accept_f57_a_lifecycle_vocabulary_is_closed_and_labelled() {
    assert_eq!(NetLifecycle::ALL.len(), 4);
    let labels: std::collections::BTreeSet<&str> =
        NetLifecycle::ALL.iter().map(|kind| kind.label()).collect();
    assert_eq!(labels.len(), NetLifecycle::ALL.len());
    assert_eq!(NetLifecycle::Alive.label(), "alive");
    assert!(!NetLifecycle::Alive.is_terminal());
    assert!(NetLifecycle::Destroyed.is_terminal());
    assert!(NetLifecycle::BailedOut.is_terminal());
    assert!(NetLifecycle::Despawned.is_terminal());
    assert_eq!(NetLifecycle::Destroyed.to_string(), "destroyed");

    assert_eq!(NetControlMode::ALL.len(), 3);
    let control_labels: std::collections::BTreeSet<&str> = NetControlMode::ALL
        .iter()
        .map(|mode| mode.label())
        .collect();
    assert_eq!(control_labels.len(), NetControlMode::ALL.len());

    // A spawned actor's pose is the canonical frame: body forward is -Z.
    let pose = NetPose {
        position: WorldPosition::try_new([1.0, 2.0, 3.0]).expect("finite"),
        orientation: Quaternion::IDENTITY,
    };
    assert_eq!(pose.position.to_array(), [1.0, 2.0, 3.0]);
    assert_eq!(pose.orientation, Quaternion::IDENTITY);
    assert_eq!(NetWeapons::MAX_ROUNDS, 65_535);
    assert_eq!(NetWeapons::BANK_CODES, [0, 1]);
}
