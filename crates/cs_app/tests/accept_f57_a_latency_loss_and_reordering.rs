//! F57-A acceptance: the end-to-end network scenario.
//!
//! Minimum scenario (spec F57 `### F57-A`, and the sheet's AC01): **inject
//! latency/loss/reordering; no duplicate destruction or permanent ghost
//! aircraft.** This file drives the whole designed path —
//! [`cs_sim::net_state::NetStateLedger`] → [`publish_snapshot`] → bytes →
//! [`Snapshot::decode`] → [`RemoteMirror::ingest`] — over a synthetic link that
//! delays, drops and reorders packets under the sheet's declared test conditions
//! (0/50/150 ms RTT and 0/2/10 percent loss, F57 non-negotiable behavior 5:
//! "designed test conditions, not original network specifications").
//!
//! Also here, because they are the same boundaries rather than separate
//! machinery: the origin-epoch refusal (AC03), the generation separation that
//! makes recycled ids safe (AC04), and the authority of ammunition and boost
//! capacity against a client input (AC02).
//!
//! The link is a test-side construction: no transport exists yet (F54-B), so
//! impairment is modeled here deterministically rather than pretended to be a
//! measured network. Nothing in this file claims original networked behavior.

use std::collections::BTreeMap;

use cs_app::network::physics::{IngestOutcome, IngestRefusal, RemoteMirror, publish_snapshot};
use cs_app::origin::{OriginEpoch, WorldOrigin};
use cs_net::message::{EventBody, ReliableEvent};
use cs_net::snapshot::{
    OriginEpoch as WireEpoch, POSITION_QUANTIZATION, Snapshot, UNIT_FRACTION_QUANTIZATION,
};
use cs_sim::net_state::{Destruction, NetActorState, NetStateLedger, NetWeapons};
use cs_types::Tick;
use cs_types::net::{ActorAllocator, ActorId, EventId, SessionId};
use cs_types::random::SplitMix64;
use cs_types::space::{Quaternion, WorldPosition};

const SESSION: SessionId = match SessionId::new(31) {
    Some(id) => id,
    None => unreachable!(),
};

/// The simulation rate of the synthetic session, in ticks per second.
const TICKS_PER_SECOND: u32 = 60;

/// The sheet's designed latency/loss test conditions (F57 non-negotiable 5).
/// Not original network specifications: no original network behavior is known.
#[derive(Clone, Copy)]
struct Conditions {
    rtt_ms: u32,
    loss_per_thousand: u32,
    reorder_window: u64,
}

const DESIGNED_CONDITIONS: [Conditions; 9] = [
    Conditions {
        rtt_ms: 0,
        loss_per_thousand: 0,
        reorder_window: 0,
    },
    Conditions {
        rtt_ms: 0,
        loss_per_thousand: 20,
        reorder_window: 0,
    },
    Conditions {
        rtt_ms: 0,
        loss_per_thousand: 100,
        reorder_window: 0,
    },
    Conditions {
        rtt_ms: 50,
        loss_per_thousand: 0,
        reorder_window: 0,
    },
    Conditions {
        rtt_ms: 50,
        loss_per_thousand: 20,
        reorder_window: 0,
    },
    Conditions {
        rtt_ms: 50,
        loss_per_thousand: 100,
        reorder_window: 0,
    },
    Conditions {
        rtt_ms: 150,
        loss_per_thousand: 0,
        reorder_window: 3,
    },
    Conditions {
        rtt_ms: 150,
        loss_per_thousand: 20,
        reorder_window: 3,
    },
    Conditions {
        rtt_ms: 150,
        loss_per_thousand: 100,
        reorder_window: 3,
    },
];

/// One packet in flight.
struct InFlight {
    sent_tick: u64,
    deliver_at: u64,
    sequence: u64,
    payload: Vec<u8>,
    event: Option<ReliableEvent>,
}

/// A deterministic synthetic link: delay, drop and reorder, nothing else.
///
/// The model is deliberately simple and stated: a snapshot is dropped with the
/// configured probability and is never retransmitted (motion snapshots are
/// sequenced and droppable); a reliable event is delayed but never dropped; and
/// a packet's delivery time gets up to `reorder_window` extra ticks of jitter,
/// which is what produces out-of-order arrival.
struct Link {
    conditions: Conditions,
    rng: SplitMix64,
    in_flight: Vec<InFlight>,
    sent: u64,
    dropped: u64,
    delivered: u64,
    out_of_order: u64,
    highest_sent_tick_delivered: Option<u64>,
}

impl Link {
    fn new(conditions: Conditions, seed: u64) -> Self {
        Self {
            conditions,
            rng: SplitMix64::for_domain(seed, 0x4646_3700_0000),
            in_flight: Vec::new(),
            sent: 0,
            dropped: 0,
            delivered: 0,
            out_of_order: 0,
            highest_sent_tick_delivered: None,
        }
    }

    /// One-way delay in ticks, from the round-trip time.
    fn one_way_ticks(&self) -> u64 {
        u64::from(self.conditions.rtt_ms) * u64::from(TICKS_PER_SECOND) / 2_000
    }

    fn send_snapshot(&mut self, sent_tick: u64, payload: Vec<u8>, sequence: u64) {
        self.sent += 1;
        let drop_roll = (cs_types::random::unit_f64(self.rng.next_u64()) * 1000.0) as u64;
        if drop_roll < u64::from(self.conditions.loss_per_thousand) {
            self.dropped += 1;
            return;
        }
        let jitter = self.jitter();
        self.in_flight.push(InFlight {
            sent_tick,
            deliver_at: sent_tick + self.one_way_ticks() + jitter,
            sequence,
            payload,
            event: None,
        });
    }

    fn send_event(&mut self, sent_tick: u64, event: ReliableEvent) {
        self.sent += 1;
        let jitter = self.jitter();
        self.in_flight.push(InFlight {
            sent_tick,
            deliver_at: sent_tick + self.one_way_ticks() + jitter,
            sequence: self.sent,
            payload: Vec::new(),
            event: Some(event),
        });
    }

    fn jitter(&mut self) -> u64 {
        if self.conditions.reorder_window == 0 {
            return 0;
        }
        u64::from(
            (cs_types::random::unit_f64(self.rng.next_u64())
                * (self.conditions.reorder_window + 1) as f64) as u32,
        )
    }

    /// Everything the link delivers at or before `now`, in arrival order.
    fn receive(&mut self, now: u64) -> Vec<InFlight> {
        let mut ready: Vec<InFlight> = Vec::new();
        let mut remaining: Vec<InFlight> = Vec::new();
        for packet in self.in_flight.drain(..) {
            if packet.deliver_at <= now {
                ready.push(packet);
            } else {
                remaining.push(packet);
            }
        }
        self.in_flight = remaining;
        // Arrival order: what the network hands over, which is delivery time and
        // not send order once jitter is in play.
        ready.sort_by_key(|packet| (packet.deliver_at, packet.sequence));
        for packet in &ready {
            self.delivered += 1;
            // Reordering as the *receiver* sees it: a packet whose simulation tick
            // is older than one already delivered arrives late.
            if let Some(highest) = self.highest_sent_tick_delivered
                && packet.sent_tick < highest
            {
                self.out_of_order += 1;
            }
            self.highest_sent_tick_delivered = Some(
                self.highest_sent_tick_delivered
                    .map_or(packet.sent_tick, |highest| highest.max(packet.sent_tick)),
            );
        }
        ready
    }
}

/// The world's origin epoch: both ends agree, so positions are comparable.
fn origin() -> WorldOrigin {
    WorldOrigin::new(
        OriginEpoch(1),
        WorldPosition::try_new([50_000.0, 500.0, -80_000.0]).expect("finite"),
    )
}

/// A ledger with three live aircraft, positions moving along a slow linear path
/// so a lost snapshot's absence is not the only way to notice a stale pose.
fn seeded_ledger() -> (NetStateLedger, [ActorId; 3]) {
    let mut ledger = NetStateLedger::new(SESSION);
    let mut allocator = ActorAllocator::new(SESSION);
    let mut actors = [ActorId {
        session: SESSION,
        serial: 0,
    }; 3];
    for (index, slot) in actors.iter_mut().enumerate() {
        let actor = allocator.allocate().expect("serial space");
        let state = NetActorState::spawn(
            actor,
            WorldPosition::try_new([
                50_000.0 + index as f64 * 200.0,
                500.0 + index as f64 * 20.0,
                -80_000.0,
            ])
            .expect("finite"),
            Quaternion::IDENTITY,
            400,
        );
        ledger.spawn(state).expect("the first spawn succeeds");
        *slot = actor;
    }
    (ledger, actors)
}

/// The full designed scenario under one set of conditions.
///
/// The server runs 200 ticks: it publishes a snapshot every tick, destroys one
/// aircraft at tick 100 (reporting the destruction twice, as a retransmission
/// would), despawns another at tick 150, and publishes a reliable
/// `ActorRemoved` event for the destroyed one as well. The client applies
/// everything that arrives, in arrival order.
fn run_scenario(conditions: Conditions, seed: u64) {
    let (mut ledger, actors) = seeded_ledger();
    let world_origin = origin();
    let mut link = Link::new(conditions, seed);
    let mut mirror = RemoteMirror::new(SESSION, world_origin);

    let destroyed = actors[1];
    let despawned = actors[2];
    let destroyed_generation = ledger
        .generation(destroyed)
        .expect("the ledger knows the actor");
    let despawned_generation = ledger
        .generation(despawned)
        .expect("the ledger knows the actor");

    // Every destruction the server awards over the whole run; the snapshot-path
    // retirements the client observes; and the reliable removals it applies.
    // The awarded count must be one, and no actor may be retired twice by the
    // same path, however the packets arrived.
    let mut server_destructions: Vec<(ActorId, Tick)> = Vec::new();
    let mut retired_by_snapshot: BTreeMap<ActorId, u32> = BTreeMap::new();
    let mut retired_by_event: BTreeMap<ActorId, u32> = BTreeMap::new();
    let mut snapshots_applied = 0_u32;
    let mut out_of_order_refusals = 0_u32;
    // What the authority published on each tick, so the mirrored pose can be
    // checked against the value it came from rather than against "the newest".
    let mut authoritative_positions: BTreeMap<u64, BTreeMap<ActorId, WorldPosition>> =
        BTreeMap::new();

    for tick in 0_u64..200 {
        let tick_value = Tick(tick);

        // Move every live actor a little each tick, publishing through the
        // ledger so the authority and the wire agree.
        let live: Vec<NetActorState> = ledger.states().copied().collect();
        for state in live {
            let mut next = state;
            next.pose.position = WorldPosition::try_new([
                state.pose.position.x() + 0.5,
                state.pose.position.y(),
                state.pose.position.z() - 1.5,
            ])
            .expect("finite");
            next.linear_velocity_mps = [0.5, 0.0, -1.5];
            if state.lifecycle == cs_sim::net_state::NetLifecycle::Alive {
                ledger.publish(next).expect("the owned generation applies");
            }
        }

        if tick == 100 {
            // Reported twice: a second resolver batch and a replayed request
            // must not award twice.
            for report_tick in [tick_value, Tick(101)] {
                let outcome = ledger
                    .record_destruction(destroyed, destroyed_generation, report_tick)
                    .expect("the actor is known");
                if outcome.awarded() {
                    server_destructions.push((destroyed, outcome.tick()));
                }
            }
            // And the removal is *also* published reliably, because a sequenced
            // snapshot carrying the flag may itself be dropped.
            link.send_event(
                tick,
                ReliableEvent {
                    id: EventId {
                        session: SESSION,
                        tick: tick_value,
                        producer: 0,
                        sequence: 1,
                    },
                    body: EventBody::ActorRemoved { actor: destroyed },
                },
            );
        }
        if tick == 150 {
            ledger
                .end_lifecycle(
                    despawned,
                    despawned_generation,
                    cs_sim::net_state::NetLifecycle::Despawned,
                )
                .expect("the actor is known");
            link.send_event(
                tick,
                ReliableEvent {
                    id: EventId {
                        session: SESSION,
                        tick: tick_value,
                        producer: 0,
                        sequence: 2,
                    },
                    body: EventBody::ActorRemoved { actor: despawned },
                },
            );
        }

        // What the authority holds on this tick, recorded before publication so the
        // mirrored pose can be checked against the value its record came from.
        authoritative_positions.insert(
            tick,
            ledger
                .states()
                .map(|state| (state.actor, state.pose.position))
                .collect(),
        );

        // One snapshot per tick, always published from the current authority.
        let snapshot = publish_snapshot(&ledger, &world_origin).expect("the ledger publishes");
        let payload = snapshot
            .encode(SESSION)
            .expect("the snapshot encodes within the envelope cap");
        link.send_snapshot(tick, payload, tick);

        for packet in link.receive(tick) {
            if let Some(event) = packet.event {
                if let EventBody::ActorRemoved { actor } = event.body
                    && mirror.apply_event(&event)
                {
                    *retired_by_event.entry(actor).or_default() += 1;
                }
                continue;
            }
            let delivered =
                Snapshot::decode(&packet.payload, SESSION).expect("every delivered byte decodes");
            let report = mirror.ingest(&delivered, Tick(packet.sent_tick));
            match report.outcome {
                IngestOutcome::Refused(IngestRefusal::OutOfOrder { .. }) => {
                    out_of_order_refusals += 1;
                    // A refused late packet changed nothing at all.
                    assert_eq!(report.applied, 0);
                    assert!(report.destroyed.is_empty());
                    assert!(report.despawned.is_empty());
                }
                IngestOutcome::Refused(other) => {
                    panic!("an in-epoch snapshot must only be refused as out of order: {other}")
                }
                IngestOutcome::Applied => {
                    snapshots_applied += 1;
                    for actor in report.destroyed.iter().chain(report.despawned.iter()) {
                        *retired_by_snapshot.entry(*actor).or_default() += 1;
                    }
                    for (actor, refusal) in &report.refused {
                        assert!(
                            matches!(
                                refusal,
                                IngestRefusal::GenerationEnded { .. }
                                    | IngestRefusal::ReliablyRemoved
                            ) && (*actor == destroyed || *actor == despawned),
                            "only a leaving actor may be refused: {actor} {refusal}"
                        );
                    }
                }
            }
        }
    }

    // Drain whatever is still in flight at the end of the run.
    for packet in link.receive(400) {
        if let Some(event) = packet.event {
            if let EventBody::ActorRemoved { actor } = event.body
                && mirror.apply_event(&event)
            {
                *retired_by_event.entry(actor).or_default() += 1;
            }
            continue;
        }
        let delivered =
            Snapshot::decode(&packet.payload, SESSION).expect("every delivered byte decodes");
        let report = mirror.ingest(&delivered, Tick(packet.sent_tick));
        if matches!(report.outcome, IngestOutcome::Applied) {
            for actor in report.destroyed.iter().chain(report.despawned.iter()) {
                *retired_by_snapshot.entry(*actor).or_default() += 1;
            }
        }
    }

    let label = format!(
        "{} ms RTT, {}% loss, reorder window {}",
        conditions.rtt_ms,
        conditions.loss_per_thousand / 10,
        conditions.reorder_window
    );

    // The authority awarded exactly one destruction, on the first report's tick.
    assert_eq!(
        server_destructions,
        vec![(destroyed, Tick(100))],
        "the server must award one destruction under {label}"
    );
    // The client retired each leaving actor at most once per path: the snapshot
    // flag and the reliable event both arrive, but neither applies twice.
    for (actor, count) in &retired_by_snapshot {
        assert!(
            *count <= 1,
            "the snapshot path retired {actor} {count} times under {label}"
        );
        assert!(
            *actor == destroyed || *actor == despawned,
            "only a leaving actor may be retired under {label}"
        );
    }
    for (actor, count) in &retired_by_event {
        assert!(
            *count == 1,
            "the reliable event retired {actor} {count} times under {label}"
        );
    }
    // Both removals are reliable, so both are recorded exactly once whichever
    // snapshot flags were dropped along the way.
    assert_eq!(
        retired_by_event.get(&destroyed).copied(),
        Some(1),
        "the destroyed aircraft's reliable removal must be applied once under {label}"
    );
    assert_eq!(
        retired_by_event.get(&despawned).copied(),
        Some(1),
        "the despawned aircraft's reliable removal must be applied once under {label}"
    );
    // On a perfect link the sequenced path also carried the flags, so that path is
    // exercised too rather than only the reliable one.
    if conditions.rtt_ms == 0 && conditions.loss_per_thousand == 0 {
        assert_eq!(retired_by_snapshot.get(&destroyed).copied(), Some(1));
        assert_eq!(retired_by_snapshot.get(&despawned).copied(), Some(1));
    }
    assert!(
        snapshots_applied > 0,
        "some snapshots must have been applied"
    );

    // No permanent ghost: the destroyed and despawned aircraft are gone from the
    // mirror for good, and the surviving one is still there.
    assert!(
        mirror.aircraft(destroyed).is_none(),
        "the destroyed aircraft must not remain a ghost under {label}"
    );
    assert!(
        mirror.aircraft(despawned).is_none(),
        "the despawned aircraft must not remain a ghost under {label}"
    );
    assert_eq!(
        mirror.aircraft_count(),
        1,
        "one survivor expected under {label}"
    );
    let survivor = actors[0];
    let shown = mirror
        .aircraft(survivor)
        .expect("the survivor is still mirrored under {label}");
    assert_eq!(shown.actor, survivor);
    assert_eq!(
        shown.generation,
        ledger.generation(survivor).expect("owned").get()
    );
    // The mirrored pose is the *authoritative* pose, epoch-relative all the way:
    // whatever latency and loss did to the packet, the reconstructed world
    // position is inside the declared tolerance of the server's own value on the
    // tick the record came from.
    let authoritative = authoritative_positions
        .get(&shown.tick.0)
        .and_then(|at_tick| at_tick.get(&survivor))
        .copied()
        .expect("the record's tick was published under these conditions");
    let tolerance = mirrored_position_tolerance_m(authoritative, &world_origin);
    let distance = distance_m(shown.position, authoritative);
    assert!(
        distance <= tolerance,
        "the mirrored position is {distance} m from the authoritative one at tick {} under {label}, tolerance is {tolerance}",
        shown.tick.0
    );

    if conditions.loss_per_thousand > 0 {
        assert!(
            link.dropped > 0,
            "the configured loss must actually drop under {label}"
        );
    }
    if conditions.reorder_window > 0 && conditions.rtt_ms > 0 {
        assert!(
            link.out_of_order > 0,
            "the configured reordering must actually reorder under {label}"
        );
        assert!(
            out_of_order_refusals > 0,
            "a reordered arrival must be refused as out of order under {label}"
        );
    }
}

/// The sheet's minimum scenario, under every designed latency/loss condition:
/// injecting latency, loss and reordering leaves neither a duplicate destruction
/// nor a permanent ghost aircraft.
#[test]
fn accept_f57_a_latency_loss_and_reordering_leave_no_duplicate_destruction_and_no_ghosts() {
    for (index, conditions) in DESIGNED_CONDITIONS.iter().enumerate() {
        // Two seeds per condition: the guarantee cannot depend on one draw of the
        // impairment model.
        run_scenario(
            Conditions {
                rtt_ms: conditions.rtt_ms,
                loss_per_thousand: conditions.loss_per_thousand,
                reorder_window: conditions.reorder_window,
            },
            0x0000_5EED_0000_0000 + index as u64,
        );
        run_scenario(
            Conditions {
                rtt_ms: conditions.rtt_ms,
                loss_per_thousand: conditions.loss_per_thousand,
                reorder_window: conditions.reorder_window,
            },
            0x0000_C0FF_EE00_0000 + index as u64,
        );
    }
}

/// Loss is not evidence: an actor missing from one snapshot is not despawned, so
/// a dropped packet cannot empty the world and a later packet restores it
/// exactly.
#[test]
fn accept_f57_a_a_lost_snapshot_is_not_evidence_of_a_despawn() {
    let (ledger, actors) = seeded_ledger();
    let world_origin = origin();
    let mut mirror = RemoteMirror::new(SESSION, world_origin);
    let survivor = actors[0];

    let first = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&first, Tick(1));
    assert_eq!(mirror.aircraft_count(), 3);
    let before = mirror.aircraft(survivor).expect("mirrored").position;

    // A snapshot that carries only one actor — which is what a partially
    // populated frame looks like — must not retire the other two.
    let partial = Snapshot::new(
        first.origin,
        first.input_ack,
        vec![*first.actor(survivor).expect("present")],
    );
    let report = mirror.ingest(&partial, Tick(2));
    assert!(report.is_applied());
    assert!(report.despawned.is_empty(), "absence must not despawn");
    assert!(report.destroyed.is_empty());
    assert_eq!(
        mirror.aircraft_count(),
        3,
        "a snapshot missing two actors must leave them mirrored"
    );

    // And a snapshot that never arrives at all changes nothing.
    assert_eq!(
        mirror.aircraft(survivor).expect("mirrored").position,
        before
    );

    // The next complete snapshot updates them normally.
    let complete = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&complete, Tick(3));
    assert_eq!(mirror.aircraft_count(), 3);
}

/// An origin epoch the receiver does not share is refused by name, so a rebase is
/// never read as a world-scale jump, and a snapshot from the *new* epoch after the
/// origin moved reconstructs the same world position (F57 AC03).
#[test]
fn accept_f57_a_a_foreign_origin_epoch_is_refused_and_a_rebase_is_not_a_jump() {
    let (ledger, actors) = seeded_ledger();
    let survivor = actors[0];
    let first_origin = origin();
    let mut mirror = RemoteMirror::new(SESSION, first_origin);

    let snapshot = publish_snapshot(&ledger, &first_origin).expect("publishes");
    assert_eq!(snapshot.origin, WireEpoch(1));
    mirror.ingest(&snapshot, Tick(1));
    let before = mirror.aircraft(survivor).expect("mirrored").position;

    // A snapshot claiming a different epoch cannot be interpreted, and changes
    // nothing.
    let foreign = Snapshot::new(WireEpoch(2), snapshot.input_ack, snapshot.actors.clone());
    let report = mirror.ingest(&foreign, Tick(2));
    assert_eq!(
        report.whole_refusal(),
        Some(IngestRefusal::EpochMismatch {
            snapshot: 2,
            local: 1
        })
    );
    assert_eq!(report.applied, 0);
    assert_eq!(
        mirror.aircraft(survivor).expect("mirrored").position,
        before
    );
    assert_eq!(mirror.applied_tick(), Tick(1));

    // After a rebase the world origin moves but the aircraft's world position does
    // not: the position is reconstructed through the shared epoch, so the
    // apparent jump is inside the declared tolerance, not the size of the rebase.
    let rebased = first_origin
        .rebased(WorldPosition::try_new([250_000.0, 900.0, -400_000.0]).expect("finite"))
        .expect("the epoch counter does not wrap");
    assert_eq!(rebased.epoch(), OriginEpoch(2));
    let mut rebased_mirror = RemoteMirror::new(SESSION, rebased);
    let after_snapshot = publish_snapshot(&ledger, &rebased).expect("publishes");
    assert_eq!(after_snapshot.origin, WireEpoch(2));
    let report = rebased_mirror.ingest(&after_snapshot, Tick(2));
    assert!(report.is_applied());
    let after = rebased_mirror
        .aircraft(survivor)
        .expect("mirrored in the new epoch")
        .position;
    let tolerance = mirrored_position_tolerance_m(before, &rebased);
    let distance = distance_m(before, after);
    assert!(
        distance <= tolerance,
        "a rebase moved the mirrored aircraft {distance} m; tolerance is {tolerance}"
    );
}

/// Two targets that share an actor id across generations never share mirror
/// state: a generation change retires the old record outright (F57 AC04, the
/// schema-level half — F57-D measures the interpolation-history consequence).
#[test]
fn accept_f57_a_recycled_generations_never_share_mirror_state() {
    let (ledger, actors) = seeded_ledger();
    let world_origin = origin();
    let mut mirror = RemoteMirror::new(SESSION, world_origin);
    let recycled = actors[1];
    let first_generation = ledger
        .generation(recycled)
        .expect("the ledger knows the actor")
        .get();

    let snapshot = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&snapshot, Tick(1));
    let original = *mirror.aircraft(recycled).expect("mirrored");

    // The same actor id comes back under a new generation at a different pose,
    // which is what an id-reusing host migration would look like.
    let mut moved = original;
    moved.generation = first_generation + 1;
    moved.tick = Tick(9);
    moved.position = WorldPosition::try_new([123_456.0, 12.0, -34.0]).expect("finite");
    moved.rounds = [7, 9];
    let record = cs_app::network::physics::publish_actor(
        &{
            let mut state = *ledger.state(recycled).expect("the ledger knows the actor");
            state.pose.position = moved.position;
            state
        },
        &world_origin,
    )
    .expect("the moved state publishes");
    let mut next_record = record;
    next_record.generation = first_generation + 1;
    next_record.weapons.primary_rounds = 7;
    next_record.weapons.secondary_rounds = 9;
    let replacement = Snapshot::new(snapshot.origin, snapshot.input_ack, vec![next_record]);
    let report = mirror.ingest(&replacement, Tick(9));

    assert_eq!(
        report.replaced,
        vec![recycled],
        "the old generation must be retired"
    );
    assert_eq!(
        report.spawned,
        vec![recycled],
        "the new generation is a new record"
    );
    assert!(
        report.updated.is_empty(),
        "a generation change is not an update"
    );
    let now = mirror.aircraft(recycled).expect("mirrored");
    assert_eq!(now.generation, first_generation + 1);
    assert_ne!(now.generation, original.generation);
    assert_eq!(
        now.rounds,
        [7, 9],
        "no field of the old generation may survive"
    );
    assert_eq!(now.tick, Tick(9));

    // A stale record for the *old* generation is refused, not applied.
    let mut stale = next_record;
    stale.generation = first_generation;
    let stale_snapshot = Snapshot::new(snapshot.origin, snapshot.input_ack, vec![stale]);
    let report = mirror.ingest(&stale_snapshot, Tick(10));
    assert_eq!(
        report.refused,
        vec![(
            recycled,
            IngestRefusal::GenerationEnded {
                generation: first_generation
            }
        )]
    );
    assert_eq!(
        mirror.aircraft(recycled).expect("mirrored").generation,
        first_generation + 1
    );
}

/// A client input changes the acknowledgment and nothing else: rounds, boost
/// capacity and pose stay authoritative even across a server correction in the
/// middle of the client's tick (F57 AC02's authority half).
#[test]
fn accept_f57_a_client_input_leaves_rounds_and_boost_authoritative() {
    let (mut ledger, actors) = seeded_ledger();
    let world_origin = origin();
    let target = actors[0];
    let mut mirror = RemoteMirror::new(SESSION, world_origin);

    let mut state = *ledger.state(target).expect("the ledger knows the actor");
    state.weapons = NetWeapons {
        primary_rounds: 300,
        secondary_rounds: 40,
        selected_bank: 0,
    };
    state.flight.boost_capacity = 0.8;
    ledger.publish(state).expect("the owned generation applies");
    ledger
        .observe_input(1, Tick(1))
        .expect("the first input is accepted");

    let snapshot = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&snapshot, Tick(1));
    let shown = mirror.aircraft(target).expect("mirrored");
    assert_eq!(shown.rounds, [300, 40]);
    let boost_before = shown.flight[2];

    // More inputs arrive — a client's prediction loop claiming shots and boost —
    // and the authority does not move.
    for sequence in 2_u32..40 {
        ledger
            .observe_input(sequence, Tick(u64::from(sequence)))
            .expect("a newer sequence is accepted");
    }
    let correction = publish_snapshot(&ledger, &world_origin).expect("publishes");
    assert_eq!(correction.input_ack, 39);
    mirror.ingest(&correction, Tick(2));
    let after = mirror.aircraft(target).expect("mirrored");
    assert_eq!(
        after.rounds,
        [300, 40],
        "a client input must never consume an authoritative round"
    );
    assert!(
        (after.flight[2] - boost_before).abs() <= UNIT_FRACTION_QUANTIZATION.max_error(),
        "a client input must never spend authoritative boost capacity: {} vs {boost_before}",
        after.flight[2]
    );

    // Only the server's own state change moves the counters.
    let mut spent = *ledger.state(target).expect("the ledger knows the actor");
    spent.weapons.primary_rounds = 299;
    spent.flight.boost_capacity = 0.5;
    ledger.publish(spent).expect("the owned generation applies");
    let correction = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&correction, Tick(3));
    let after = mirror.aircraft(target).expect("mirrored");
    assert_eq!(after.rounds, [299, 40]);
    assert!(
        (after.flight[2] - 0.5).abs() <= UNIT_FRACTION_QUANTIZATION.max_error(),
        "the server's boost spend must reach the mirror inside the declared budget, got {}",
        after.flight[2]
    );
}

/// The mirror holds presentation state only: it can retire an actor and report a
/// lifecycle change, and it has no path that awards damage, a reward or a mission
/// result — the exhaustive inventory below is what a later stage has to extend
/// deliberately if it wants one.
#[test]
fn accept_f57_a_the_mirror_awards_nothing() {
    let (ledger, actors) = seeded_ledger();
    let world_origin = origin();
    let mut mirror = RemoteMirror::new(SESSION, world_origin);
    let snapshot = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&snapshot, Tick(1));
    assert_eq!(mirror.aircraft_count(), 3);

    // The only things a report can say are these.
    let report = mirror.ingest(&snapshot, Tick(2));
    assert_eq!(report.outcome, IngestOutcome::Applied);
    assert_eq!(report.spawned, Vec::<ActorId>::new());
    assert_eq!(report.replaced, Vec::<ActorId>::new());
    assert_eq!(report.destroyed, Vec::<ActorId>::new());
    assert_eq!(report.despawned, Vec::<ActorId>::new());
    assert_eq!(report.applied, 3);

    // A non-lifecycle reliable event is observed and ignored: the mirror runs no
    // mission logic.
    let peer_event = ReliableEvent {
        id: EventId {
            session: SESSION,
            tick: Tick(2),
            producer: 1,
            sequence: 0,
        },
        body: EventBody::PeerJoined {
            peer: cs_types::net::PeerId::new(1).expect("nonzero"),
        },
    };
    assert!(!mirror.apply_event(&peer_event));
    assert_eq!(mirror.aircraft_count(), 3);

    // A removal event retires once, and a replay of the same event does nothing.
    let removal = ReliableEvent {
        id: EventId {
            session: SESSION,
            tick: Tick(3),
            producer: 1,
            sequence: 1,
        },
        body: EventBody::ActorRemoved { actor: actors[0] },
    };
    assert!(mirror.apply_event(&removal));
    assert!(!mirror.apply_event(&removal));
    assert_eq!(mirror.aircraft_count(), 2);

    // And a later snapshot for that actor cannot bring it back.
    let report = mirror.ingest(&snapshot, Tick(4));
    assert_eq!(
        report.refused,
        vec![(actors[0], IngestRefusal::ReliablyRemoved)],
        "a reliably removed id must stay removed whatever a later snapshot carries"
    );
    assert_eq!(mirror.aircraft_count(), 2);
}

/// A record from an *older* generation than the mirror holds is refused rather
/// than treated as a new actor, and a reliable event stamped with another session
/// generation never retires anything: generations only ever increase for an id, so
/// an older one is a late packet, not a recycled id.
#[test]
fn accept_f57_a_an_older_generation_is_refused_rather_than_replayed_back() {
    let (ledger, actors) = seeded_ledger();
    let world_origin = origin();
    let mut mirror = RemoteMirror::new(SESSION, world_origin);
    let target = actors[0];
    let generation = ledger
        .generation(target)
        .expect("the ledger knows the actor")
        .get();

    let snapshot = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&snapshot, Tick(1));
    let held = *mirror.aircraft(target).expect("mirrored");
    assert_eq!(held.generation, generation);

    // The same actor comes back two generations on, which is what a lost
    // intermediate snapshot looks like: the mirror never applied the generation in
    // between, so it has no ended record for it.
    let mut newer = *snapshot.actor(target).expect("the record is present");
    newer.generation = generation + 2;
    let replacement = Snapshot::new(snapshot.origin, snapshot.input_ack, vec![newer]);
    let report = mirror.ingest(&replacement, Tick(2));
    assert_eq!(report.replaced, vec![target]);
    let now = *mirror
        .aircraft(target)
        .expect("mirrored under the new generation");
    assert_eq!(now.generation, generation + 2);

    // An intermediate generation arriving afterwards is refused by name and
    // changes nothing: it neither replaces the newer record nor resurrects the old
    // pose. Without that check it would look like an id recycled forward.
    let mut older = *snapshot.actor(target).expect("the record is present");
    older.generation = generation + 1;
    let late = Snapshot::new(snapshot.origin, snapshot.input_ack, vec![older]);
    let report = mirror.ingest(&late, Tick(3));
    assert_eq!(
        report.refused,
        vec![(
            target,
            IngestRefusal::StaleGeneration {
                record: generation + 1,
                held: generation + 2,
            }
        )],
        "an older generation must be refused, not applied as a new actor"
    );
    assert!(report.applied == 0 && report.replaced.is_empty() && report.spawned.is_empty());
    assert_eq!(*mirror.aircraft(target).expect("still mirrored"), now);

    // A reliable removal stamped with another session generation is not this
    // session's event: it retires nothing, and the actor stays mirrored.
    let foreign_event = ReliableEvent {
        id: EventId {
            session: SessionId::new(97).expect("nonzero"),
            tick: Tick(4),
            producer: 0,
            sequence: 0,
        },
        body: EventBody::ActorRemoved { actor: target },
    };
    assert!(!mirror.apply_event(&foreign_event));
    assert!(mirror.aircraft(target).is_some());
    assert_eq!(mirror.aircraft_count(), 3);

    // A stored integer that does not fit the declared width of the field it
    // arrived in is named by that field: the mirror's whole contract is that a
    // refusal says which part of the record it is about. `Snapshot::decode` and
    // `Snapshot::validate` already refuse such a record on the wire, so this is the
    // receiver's own boundary — and it names the field rather than blaming the
    // rotation, which is what the mirror could not do before.
    let broken = actors[1];
    let mut unreadable = *snapshot.actor(broken).expect("the record is present");
    unreadable.linear_velocity = cs_net::snapshot::QuantizedVector::quantize(
        cs_net::snapshot::POSITION_QUANTIZATION,
        [1.0e6, 0.0, 0.0],
    )
    .expect("a million meters is inside the declared position range");
    assert_eq!(
        cs_app::network::physics::RemoteAircraft::from_record(&unreadable, Tick(5), &world_origin),
        Err(cs_app::network::physics::MirrorError::UnreadableField {
            field: "linear_velocity",
        })
    );
    let report = mirror.ingest(
        &Snapshot::new(snapshot.origin, snapshot.input_ack, vec![unreadable]),
        Tick(5),
    );
    assert_eq!(
        report.refused,
        vec![(
            broken,
            IngestRefusal::UnreadableField {
                field: "linear_velocity",
            }
        )],
        "an unreadable field must be named, not reported as an unusable rotation"
    );
    assert_eq!(mirror.aircraft_count(), 3);
}

/// The mirror's per-actor bookkeeping is keyed by actor id and stays bounded by
/// the actors the session knows; forgetting an actor is the only way its
/// ended-generation record is dropped.
#[test]
fn accept_f57_a_mirror_bookkeeping_is_keyed_by_actor_and_is_forgettable() {
    let (ledger, actors) = seeded_ledger();
    let world_origin = origin();
    let mut mirror = RemoteMirror::new(SESSION, world_origin);
    let snapshot = publish_snapshot(&ledger, &world_origin).expect("publishes");
    mirror.ingest(&snapshot, Tick(1));
    assert_eq!(mirror.session(), SESSION);
    assert_eq!(mirror.origin().epoch(), OriginEpoch(1));
    assert_eq!(mirror.applied_tick(), Tick(1));

    let inventory: BTreeMap<ActorId, u16> = mirror
        .all_aircraft()
        .map(|aircraft| (aircraft.actor, aircraft.generation))
        .collect();
    assert_eq!(inventory.len(), 3);
    assert!(inventory.contains_key(&actors[0]));

    mirror.forget(actors[0]);
    assert!(mirror.aircraft(actors[0]).is_none());
    assert_eq!(mirror.aircraft_count(), 2);
}

/// Distance in meters between two canonical world positions.
fn distance_m(a: WorldPosition, b: WorldPosition) -> f64 {
    let [ax, ay, az] = a.to_array();
    let [bx, by, bz] = b.to_array();
    ((bx - ax).powi(2) + (by - ay).powi(2) + (bz - az).powi(2)).sqrt()
}

/// The tolerance a mirrored pose may differ from the authority's own value by,
/// per axis.
///
/// Two declared bounds compose here, and comparing a *three-axis distance*
/// against a *per-axis* budget would be the wrong unit in any case:
/// [`POSITION_QUANTIZATION`]'s half-step, and the f32 world → local → world round
/// trip the publish/mirror path goes through
/// ([`cs_app::origin::local_round_trip_tolerance_m`]).
fn mirrored_position_tolerance_m(authoritative: WorldPosition, world_origin: &WorldOrigin) -> f64 {
    let local = world_origin.local_of(authoritative).expect("finite");
    POSITION_QUANTIZATION.max_error()
        + cs_app::origin::local_round_trip_tolerance_m(authoritative, local)
}

/// The destruction ledger's own award is visible from this test's server side, so
/// the scenario above's "reported twice" really is two reports of one actor.
#[test]
fn accept_f57_a_the_scenario_reports_one_destruction_for_one_actor() {
    let (mut ledger, actors) = seeded_ledger();
    let target = actors[1];
    let generation = ledger
        .generation(target)
        .expect("the ledger knows the actor");
    let first = ledger
        .record_destruction(target, generation, Tick(100))
        .expect("the actor is known");
    let second = ledger
        .record_destruction(target, generation, Tick(101))
        .expect("the actor is known");
    assert!(matches!(first, Destruction::Recorded { tick: Tick(100) }));
    assert!(!second.awarded());
    assert_eq!(second.tick(), Tick(100));
}
