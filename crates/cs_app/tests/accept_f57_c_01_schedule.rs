//! Acceptance scenario F57-C.01: the typed `NetSession` driven by the Bevy
//! fixed schedule behind the session lifecycle.
//!
//! Spec: `specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`,
//! stage `### F57-C.01`, and `docs/contracts/UI-NETWORK.md` (ownership table:
//! the server owns every actor and every session fact; the client owns its
//! input and nothing else). Follow-up task Rally #657; task test prefix:
//! `accept_f57_c_01_`.
//!
//! # What is under test
//!
//! [`cs_app::network::session::NetSessionDrivePlugin`] installed on a real
//! [`cs_app::physics::PhysicsSession`], so every stage runs in the production
//! `FixedPostUpdate` chain — `NetSessionSet::{Origin, Drive, Sample, Correct,
//! ExpireTracers}` ordered after `PhysicsSystems::StepSimulation` — over a real
//! loopback pair: `ServerSession` + `ClientSession` through the pinned `renet2`
//! stack, not a parallel test-side link.
//!
//! The scenario asserts the whole scheduled consumption the task named:
//!
//! * Decoded `SnapshotFrame`s drain into `NetSession::ingest` in tick order and
//!   the latest-wins mailbox is fed at most once per snapshot.
//! * `NetSession::sample` runs at the committed tick into `NetPresentation`;
//!   the local actor is never interpolated.
//! * The wire's `ActorSpawned` names the local actor; the one
//!   `NetPredictedBody` pose owner's pose is recorded, reconciled and
//!   corrected by `NetSession::next_correction` — and two markers are refused
//!   by name as `NetPoseOwner::Ambiguous`.
//! * The real `crate::origin::FixedTickDriver` advances once per committed
//!   tick; a policy rebase moves `NetHost`, `NetClient` and the live mirror to
//!   the same epoch through `NetSession::rebase`, and a snapshot published in
//!   the new epoch still applies.
//! * Client tracers expire on the schedule; a session end tears the mirror
//!   down by name; a reconnect builds a fresh one under the new generation.
//! * Every refusal — `SessionError`, `IngestRefusal`, `PublishError`,
//!   `SnapshotError` — reaches `NetSessionReport`; nothing is dropped.
//!
//! Loopback on one machine is `network_local` per `AGENTS.md`; no original
//! networked behavior is asserted anywhere here.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use avian3d::prelude::Position;
use bevy::ecs::entity::Entity;
use bevy::prelude::World;

use cs_app::network::physics::{
    IngestOutcome, IngestRefusal, InterpolationConfig, NetSession, PublishError, ReconcileKind,
    Tracers,
};
use cs_app::network::session::{
    NetClient, NetHost, NetMirror, NetOriginDriver, NetPoseOwner, NetPredictedBody,
    NetPresentation, NetPublish, NetSessionDrivePlugin, NetSessionReport,
};
use cs_app::origin::{FixedTickDriver, OriginEpoch, RebasePolicy, SpatialSubsystem, WorldOrigin};
use cs_app::physics::{BodyMode, BodySpec, PhysicsSession, spawn_body};
use cs_net::fixture::{synthetic_hello, synthetic_parameters};
use cs_net::lifecycle::{ClientSession, ServerSession};
use cs_net::message::DisconnectReason;
use cs_net::snapshot::SnapshotError;
use cs_net::transport::ConnectWindow;
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::net_state::{NetActorState, NetStateLedger, ShotId};
use cs_sim::time::{ClockPolicy, TickRate};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{ActorId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

/// The session generation the fixture runs.
const SESSION: u64 = 71;

/// The generation the host reopens under for the reconnect check.
const RECONNECTED: u64 = 72;

/// The serial of the aircraft the wire names this client's own.
const PLANE_SERIAL: u64 = 7;

/// A remote aircraft the same session also publishes.
const REMOTE_SERIAL: u64 = 9;

/// The serial the reconnected session publishes.
const RECONNECTED_SERIAL: u64 = 8;

/// The fixed rate the world commits, matching the interpolation config's
/// declared 60 ticks per second and the origin driver's rate.
const FIXED_HZ: u32 = 60;

/// How many committed ticks a wait gets before it fails.
///
/// The budget is the same reasoning `accept_f54_c_drive` documents: the
/// connection layer's window is ~120 s of pumped elapsed, and the tracer
/// expiry test alone needs the 120-tick presentation window to elapse —
/// every wait bounds *committed ticks*, never the wall clock.
const MAX_TICKS: u64 = 600;

/// The connection-layer window the fixture asks for; see
/// `accept_f54_c_lifecycle.rs`'s `LOOPBACK_WINDOW`.
const LOOPBACK_WINDOW: ConnectWindow = ConnectWindow::new(120);

/// Serializes the tests in this file that open real loopback sockets — one
/// live pair at a time is what the machine's loopback path carries without
/// loss, the same finding `accept_f54_c_lifecycle` measured.
static LOOPBACK: Mutex<()> = Mutex::new(());

fn loopback() -> MutexGuard<'static, ()> {
    LOOPBACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session id")
}

fn reconnected_id() -> SessionId {
    SessionId::new(RECONNECTED).expect("a nonzero session id")
}

fn plane() -> ActorId {
    ActorId {
        session: session_id(),
        serial: PLANE_SERIAL,
    }
}

fn remote() -> ActorId {
    ActorId {
        session: session_id(),
        serial: REMOTE_SERIAL,
    }
}

fn reconnected_plane() -> ActorId {
    ActorId {
        session: reconnected_id(),
        serial: RECONNECTED_SERIAL,
    }
}

/// The shared origin epoch the fixture starts in: host publisher, client
/// mirror and origin driver all read the same frame so relative coordinates
/// cross the wire without an epoch refusal.
fn origin() -> WorldOrigin {
    WorldOrigin::new(
        OriginEpoch(1),
        WorldPosition::try_new([50_000.0, 500.0, -80_000.0]).expect("finite"),
    )
}

/// The ledger the fixture serves: the local aircraft the wire will name this
/// client's own, plus one remote aircraft the interpolation path must show.
fn spawned_ledger() -> NetStateLedger {
    let mut ledger = NetStateLedger::new(session_id());
    ledger
        .spawn(NetActorState::spawn(
            plane(),
            WorldPosition::try_new([50_100.0, 520.0, -80_300.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        ))
        .expect("the first spawn succeeds");
    ledger
        .spawn(NetActorState::spawn(
            remote(),
            WorldPosition::try_new([50_200.0, 540.0, -80_600.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        ))
        .expect("the remote spawn succeeds");
    ledger
}

/// The ledger the host serves after reopening: a different generation, a
/// different actor serial — nothing of the spent epoch may survive in it.
fn reconnected_ledger() -> NetStateLedger {
    let mut ledger = NetStateLedger::new(reconnected_id());
    ledger
        .spawn(NetActorState::spawn(
            reconnected_plane(),
            WorldPosition::try_new([50_150.0, 530.0, -80_400.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        ))
        .expect("the reconnected spawn succeeds");
    ledger
}

/// The production origin driver the session world would run: 60 Hz, a rebase
/// when any tracked record drifts 80 m from the epoch's origin.
fn origin_driver() -> FixedTickDriver {
    FixedTickDriver::new(
        ClockPolicy::multiplayer_simulation(),
        TickRate::new(FIXED_HZ).expect("a nonzero rate"),
        origin(),
        RebasePolicy::at_limit(80.0).expect("a positive limit"),
    )
}

/// Spawns the one entity the session is allowed to write poses through: an
/// Avian body marked `NetPredictedBody` at local origin.
fn spawn_owner_body(world: &mut World) -> Entity {
    let entity = spawn_body(
        world,
        &BodySpec {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            mode: BodyMode::Kinematic,
            mass_kg: 0.0,
            half_extents_m: [1.0, 1.0, 1.0],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        },
    )
    .expect("the pose-owner spec validates");
    world.entity_mut(entity).insert(NetPredictedBody);
    entity
}

/// The canonical world position a `Position`/`Rotation` on `entity` encodes in
/// `frame`'s epoch.
fn held_world_pose(world: &World, entity: Entity, frame: WorldOrigin) -> WorldPosition {
    let held = world
        .get::<Position>(entity)
        .expect("the pose owner carries a Position")
        .0;
    frame
        .world_of(
            cs_types::space::LocalPosition::try_new(held.to_array()).expect("a finite local pose"),
        )
        .expect("the local pose converts")
}

/// One loopback pair carried by one physics world: `NetHost` and `NetClient`
/// are both resources of the same session world, so every committed tick runs
/// the whole chained `NetSessionSet` once — the shape a listen-server host
/// runs.
struct LinkedWorld {
    session: PhysicsSession,
    /// The host's bound address, captured while the socket is live; `reopen`
    /// keeps the socket, so the address stays valid across a session retry.
    addr: SocketAddr,
    _loopback: MutexGuard<'static, ()>,
}

impl LinkedWorld {
    /// Builds the world with the production plugin installed, binds the host,
    /// connects the client and installs both session resources before the
    /// first committed tick.
    fn new() -> Self {
        Self::build(true)
    }

    /// Builds the same world without `NetSessionDrivePlugin` when
    /// `with_plugin` is false — the test that proves the schedule wiring is
    /// the thing that moves the session.
    fn build(with_plugin: bool) -> Self {
        let loopback = loopback();
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let server =
            ServerSession::bind(session_id(), synthetic_parameters(), bind, Duration::ZERO)
                .expect("the host socket binds");
        let addr = server.local_addr().expect("the bound host has an address");
        let client = ClientSession::connect_with_window(
            synthetic_hello(),
            addr,
            0xC1,
            Duration::ZERO,
            LOOPBACK_WINDOW,
        )
        .expect("the client socket binds");

        let mut session = PhysicsSession::production_builder()
            .fixed_hz(FIXED_HZ)
            .configure(move |app| {
                if with_plugin {
                    app.add_plugins(NetSessionDrivePlugin);
                }
            })
            .build();
        let world = session.world_mut().expect("the session is active");
        world.insert_resource(
            NetHost::new(server, spawned_ledger(), origin()).expect("the epochs agree"),
        );
        world.insert_resource(NetClient::new(
            client,
            origin(),
            InterpolationConfig::default(),
        ));
        Self {
            session,
            addr,
            _loopback: loopback,
        }
    }

    fn world(&mut self) -> &mut World {
        self.session.world_mut().expect("the session is active")
    }

    /// Commits `ticks` fixed ticks through the production stepper.
    fn step(&mut self, ticks: u64) {
        self.session.step(ticks).expect("the session steps");
    }

    /// The just-committed run's report.
    fn report(&self) -> NetSessionReport {
        self.session
            .world()
            .and_then(|world| world.get_resource::<NetSessionReport>().cloned())
            .unwrap_or_default()
    }

    /// Commits one tick at a time until `done` holds on the resulting report.
    ///
    /// The predicate is `FnMut`, so it may keep a running observation across
    /// runs — a field that only ever sets once (an adoption, a refusal) is
    /// still observable two runs later.
    fn step_until(&mut self, what: &str, mut done: impl FnMut(&NetSessionReport) -> bool) {
        for _ in 0..MAX_TICKS {
            self.step(1);
            let report = self.report();
            if done(&report) {
                return;
            }
        }
        panic!(
            "{what}: {MAX_TICKS} committed ticks elapsed; last report {:?}",
            self.report()
        );
    }

    /// Commits one tick at a time until `done` holds on the world itself —
    /// for the persistent facts (`mirror.local_actor()`, a resource's
    /// presence) that a per-run report field only shows on the run they
    /// changed.
    fn step_until_state(&mut self, what: &str, mut done: impl FnMut(&World) -> bool) {
        for _ in 0..MAX_TICKS {
            self.step(1);
            if done(self.session.world().expect("the session is active")) {
                return;
            }
        }
        panic!(
            "{what}: {MAX_TICKS} committed ticks elapsed; last report {:?}",
            self.report()
        );
    }

    /// Runs the handshake to a grant and launches the match, then waits for
    /// the first applied ingest — the shared preamble every consumer test
    /// needs before a mirror can hold anything.
    fn join_and_launch(&mut self, start_tick: Tick) {
        self.step_until("the handshake never granted", |report| {
            report.client_session == Some(session_id())
        });
        self.world()
            .resource_mut::<NetHost>()
            .session_mut()
            .launch(start_tick)
            .expect("a gathering session launches");
        self.step_until("the client never ingested a snapshot", |report| {
            matches!(
                report.ingested.as_ref().map(|ingest| ingest.outcome),
                Some(IngestOutcome::Applied)
            )
        });
    }

    /// The blueprint id the spawn announcements carry; the id's own content is
    /// not under test, only the ownership word the event publishes.
    fn blueprint() -> ContentId {
        ContentId::from_source(ContentKind::Airframe, "zephyr").expect("a valid content id")
    }
}

#[test]
fn accept_f57_c_01_the_scheduled_drive_ingests_samples_corrects_and_expires() {
    let mut linked = LinkedWorld::new();
    let owner = spawn_owner_body(linked.world());
    linked.join_and_launch(Tick(100));

    // The wire names this client's aircraft: the gate binds the peer to the
    // actor and the reliable spawn event publishes the same pairing — the two
    // halves of "the server says who flies what".
    let peer = linked
        .world()
        .resource::<NetClient>()
        .session()
        .grant()
        .expect("the client holds a grant")
        .peer;
    let host_tick = linked.report().at.expect("a committed tick");
    linked
        .world()
        .resource_mut::<NetHost>()
        .bind_aircraft(peer, plane())
        .expect("the gate binds the peer's aircraft");
    linked
        .world()
        .resource_mut::<NetHost>()
        .session_mut()
        .announce_spawn(host_tick, plane(), LinkedWorld::blueprint(), Some(peer))
        .expect("the spawn announcement publishes");
    linked
        .world()
        .resource_mut::<NetHost>()
        .session_mut()
        .announce_spawn(host_tick, remote(), LinkedWorld::blueprint(), None)
        .expect("the remote announcement publishes");

    // The event names the actor; the Correct stage attaches it to the mirror
    // record the same tick's ingest produced.
    linked.step_until("the local actor was never attached", |report| {
        report.attached == Some(plane())
    });
    let report = linked.report();
    assert_eq!(
        report.local_actor,
        Some(plane()),
        "the wire's word for the local actor is recorded"
    );
    assert_eq!(
        report.pose_owner,
        NetPoseOwner::Bound(owner),
        "the marked entity is the one pose owner"
    );
    let mirror = linked
        .world()
        .get_resource::<NetMirror>()
        .expect("the drive built the client mirror");
    assert_eq!(mirror.local_actor(), Some(plane()));

    // By the time this wait runs the drive has reconciled the pose owner
    // onto the record — which earlier run carried which verdict depends on
    // when the wire named the local actor. The two contracts that hold on
    // every run: a reconcile ran through the production path, and a
    // `Snapped` verdict with no predicted pose to compare (unbounded error)
    // is enacted honestly — the owner adopts the record's pose outright.
    linked.step_until("no reconcile ever ran", |report| {
        report.reconciled.is_some()
    });
    let report = linked.report();
    let first = report.reconciled.expect("a reconcile ran this run");
    if first.kind == ReconcileKind::Snapped && first.error_m.is_infinite() {
        assert!(
            report.adopted_pose,
            "a record without a predicted pose is adopted outright: {first:?}"
        );
    }
    linked.step_until_state("the pose owner never reached the record's pose", |world| {
        let p = world.get::<Position>(owner).expect("a pose").0;
        (f64::from(p.x) - 100.0).abs() < 1.0
            && (f64::from(p.y) - 20.0).abs() < 1.0
            && (f64::from(p.z) - (-300.0)).abs() < 1.0
    });

    // Remote sampling runs every committed tick into `NetPresentation`; the
    // local actor is the predictor's job and is never interpolated.
    linked.step_until("the remote aircraft was never presented", |report| {
        report.sampled >= 1 && report.sample_refusals.is_empty()
    });
    let presentation = &linked.world().resource::<NetPresentation>().aircraft;
    assert!(
        presentation.contains_key(&remote()),
        "the remote aircraft is presented: {presentation:?}"
    );
    assert!(
        !presentation.contains_key(&plane()),
        "the local actor is the predictor's, never interpolated"
    );
    let presented = presentation
        .get(&remote())
        .copied()
        .expect("the remote aircraft is presented");
    assert_eq!(presented.state.actor, remote());

    // A real displacement produces a bounded correction: move the pose owner
    // 30 m sideways and the next arriving record snaps it back through
    // `next_correction` — the schedule writes it, the report names it.
    linked.world().get_mut::<Position>(owner).expect("a pose").0 +=
        bevy::math::Vec3::new(30.0, 0.0, 0.0);
    linked.step_until("the displacement was never corrected", |report| {
        report.corrected.is_some()
    });
    let corrected = linked.report().corrected.expect("a correction applied");
    assert!(
        (corrected.translation_m[0] + 30.0).abs() < 2.0,
        "the correction moves the pose owner back toward the record: {corrected:?}"
    );
    let restored = linked.world().get::<Position>(owner).expect("a pose").0;
    assert!(
        (f64::from(restored.x) - 100.0).abs() < 2.0,
        "the pose owner sits on the authoritative pose again: {restored:?}"
    );

    // Client tracers are schedule-expired cosmetics: spawn one on the live
    // mirror and the presentation window bound retires it by name.
    let at = linked.report().at.expect("a committed tick");
    linked
        .world()
        .resource_mut::<NetMirror>()
        .spawn_tracer(
            ShotId::try_new(9).expect("a nonzero shot id"),
            at,
            WorldPosition::try_new([50_000.0, 500.0, -80_000.0]).expect("finite"),
            [1.0, 0.0, 0.0],
        )
        .expect("the attached session accepts a tracer");
    linked.step_until("the tracer never expired", |report| {
        report.tracers_expired > 0
    });
    assert_eq!(linked.report().tracers_expired, 1);
    assert!(
        linked
            .world()
            .resource::<NetMirror>()
            .tracers()
            .is_none_or(Tracers::is_empty),
        "the expired tracer is gone"
    );

    // The one-pose-owner rule is the schedule's: a second marker is an
    // ambiguity the report names, not a winner the drive picks.
    let second = spawn_owner_body(linked.world());
    linked.step_until("the duplicate owner was never named", |report| {
        report.pose_owner == NetPoseOwner::Ambiguous(2)
    });
    linked
        .world()
        .entity_mut(second)
        .remove::<NetPredictedBody>();
    linked.step_until("the sole owner was never restored", |report| {
        report.pose_owner == NetPoseOwner::Bound(owner)
    });

    // A marked entity with no pose components is reported `Unposed`, not
    // silently written past.
    let unposed = linked.world().spawn(NetPredictedBody).id();
    linked
        .world()
        .entity_mut(owner)
        .remove::<NetPredictedBody>();
    linked.step_until("the unposed owner was never named", |report| {
        report.pose_owner == NetPoseOwner::Unposed(unposed)
    });
    assert!(linked.world().despawn(unposed));
    linked.world().entity_mut(owner).insert(NetPredictedBody);

    // Through all of it the drive reports no dropped refusals.
    assert!(
        linked.report().mirror_refusals.is_empty(),
        "the nominal run produced no refusals: {:?}",
        linked.report().mirror_refusals
    );
}

#[test]
fn accept_f57_c_01_the_real_origin_driver_moves_every_owner_to_one_epoch() {
    let mut linked = LinkedWorld::new();
    let owner = spawn_owner_body(linked.world());
    linked
        .world()
        .insert_resource(NetOriginDriver(origin_driver()));
    linked.join_and_launch(Tick(100));

    // A tracked spatial record already past the rebase limit: the driver's
    // own per-tick policy check is what fires the rebase — not a test calling
    // `rebase` itself.
    linked
        .world()
        .resource_mut::<NetOriginDriver>()
        .world_mut()
        .spawn(
            SpatialSubsystem::Body,
            WorldPosition::try_new([50_200.0, 500.0, -80_000.0]).expect("finite"),
            [0.0, 0.0, 0.0],
        )
        .expect("the spatial record spawns");

    let held_before = held_world_pose(linked.world(), owner, origin());
    linked.step_until("the driver's policy never rebased", |report| {
        report
            .origin
            .as_ref()
            .is_some_and(|step| !step.rebased.is_empty())
    });
    let report = linked.report();
    let step = report.origin.as_ref().expect("the origin stage reported");
    assert_eq!(step.advanced, 1, "the frame committed exactly one tick");
    let transition = step.rebased.last().expect("the mirror adopted an epoch");
    assert_eq!(transition.from, OriginEpoch(1));
    assert_eq!(transition.to, OriginEpoch(2));
    assert_eq!(
        transition.converted, 2,
        "both mirrored records converted through the frame change"
    );
    assert!(
        transition.max_conversion_m < 0.05,
        "a rebase is a conversion, not a displacement: {transition:?}"
    );
    assert_eq!(step.shifted_poses, 1, "the pose owner converted too");
    assert_eq!(step.fault, None);
    let held_after = linked
        .world()
        .resource::<NetMirror>()
        .origin()
        .local_of(held_before)
        .expect("the held pose converts back")
        .to_array();
    let held_now = linked.world().get::<Position>(owner).expect("a pose").0;
    assert!(
        (f64::from(held_after[0]) - f64::from(held_now.x)).abs() < 0.5
            && (f64::from(held_after[1]) - f64::from(held_now.y)).abs() < 0.5
            && (f64::from(held_after[2]) - f64::from(held_now.z)).abs() < 0.5,
        "the pose owner's canonical world pose survived the rebase"
    );

    // Every session owner sits on the same epoch now: the publish frame, the
    // frame a fresh mirror would build in, and the live mirror.
    assert_eq!(
        linked.world().resource::<NetHost>().origin().epoch(),
        OriginEpoch(2)
    );
    assert_eq!(
        linked.world().resource::<NetClient>().origin().epoch(),
        OriginEpoch(2)
    );
    assert_eq!(
        linked.world().resource::<NetMirror>().origin().epoch(),
        OriginEpoch(2)
    );

    // A snapshot published in the new epoch applies: no `EpochMismatch`
    // survives once both ends name the same frame.
    linked.step_until("no new-epoch snapshot ever applied", |report| {
        matches!(
            report.ingested.as_ref().map(|ingest| ingest.outcome),
            Some(IngestOutcome::Applied)
        ) && report
            .ingested
            .as_ref()
            .is_some_and(|ingest| ingest.origin.0 == 2)
    });
}

#[test]
fn accept_f57_c_01_disconnect_tears_down_and_reconnect_rebuilds_fresh() {
    let mut linked = LinkedWorld::new();
    linked.join_and_launch(Tick(100));

    // Fill the mirror with everything teardown must release: aircraft,
    // interpolation tracks, an attached local actor and a live tracer.
    let peer = linked
        .world()
        .resource::<NetClient>()
        .session()
        .grant()
        .expect("the client holds a grant")
        .peer;
    let announce_at = linked.report().at.expect("a committed tick");
    linked
        .world()
        .resource_mut::<NetHost>()
        .session_mut()
        .announce_spawn(announce_at, plane(), LinkedWorld::blueprint(), Some(peer))
        .expect("the spawn announcement publishes");
    linked.step_until("the local actor was never attached", |report| {
        report.attached == Some(plane())
    });
    let at = linked.report().at.expect("a committed tick");
    linked
        .world()
        .resource_mut::<NetMirror>()
        .spawn_tracer(
            ShotId::try_new(3).expect("a nonzero shot id"),
            at,
            WorldPosition::try_new([50_000.0, 500.0, -80_000.0]).expect("finite"),
            [1.0, 0.0, 0.0],
        )
        .expect("the attached session accepts a tracer");
    linked.step_until("the remote aircraft was never presented", |report| {
        report.sampled >= 1
    });
    assert!(
        linked
            .world()
            .resource::<NetMirror>()
            .tracers()
            .is_some_and(|tracers| tracers.len() == 1),
        "the tracer is live before teardown"
    );

    // The host ends the session: the reliable disconnect reaches the client,
    // the phase goes `Closed` and the drive releases the mirror by name —
    // aircraft, buffers, predictor and tracers all go with it.
    linked
        .world()
        .resource_mut::<NetHost>()
        .session_mut()
        .close(DisconnectReason::SessionEnded)
        .expect("the host closes its own session");
    linked.step_until("the closed session never released the mirror", |report| {
        report
            .tore_down
            .iter()
            .any(|down| down.side == "client" && down.session == Some(session_id()))
    });
    let report = linked.report();
    let teardown = report
        .tore_down
        .iter()
        .find(|down| down.side == "client")
        .expect("the client teardown was recorded");
    assert_eq!(teardown.mirror, Some(2), "both mirror aircraft released");
    assert!(
        linked.world().get_resource::<NetMirror>().is_none(),
        "no ghost session remains installed"
    );
    assert!(
        linked
            .world()
            .resource::<NetPresentation>()
            .aircraft
            .is_empty(),
        "no ghost aircraft remains presented"
    );
    assert_eq!(
        linked.report().local_actor,
        None,
        "the wire's ownership word died with the session"
    );

    // Reconnect: the host mints a fresh epoch on the same socket and a fresh
    // `ClientSession` joins it. The drive must build a fresh mirror for the
    // new generation — never feed the spent one's traffic.
    linked
        .world()
        .resource_mut::<NetHost>()
        .reopen(reconnected_id(), reconnected_ledger())
        .expect("the host mints a fresh epoch");
    let addr = linked.addr;
    let client = ClientSession::connect_with_window(
        synthetic_hello(),
        addr,
        0xC2,
        Duration::ZERO,
        LOOPBACK_WINDOW,
    )
    .expect("the reconnecting client socket binds");
    linked.world().insert_resource(NetClient::new(
        client,
        origin(),
        InterpolationConfig::default(),
    ));
    linked.step_until("the reconnect never granted", |report| {
        report.client_session == Some(reconnected_id())
    });
    let mirror = linked
        .world()
        .get_resource::<NetMirror>()
        .expect("a fresh mirror was built for the new generation");
    assert_eq!(mirror.session(), reconnected_id());
    assert_eq!(
        mirror.mirror().aircraft_count(),
        0,
        "the fresh mirror is empty"
    );
    assert!(mirror.predictor().is_none(), "no prediction survived");
    assert!(mirror.tracers().is_none(), "no tracers survived");
    assert!(!mirror.is_torn_down());

    // The fresh epoch launches, names the fresh aircraft and the snapshot
    // applies cleanly — the mirror holds exactly the new generation's world.
    linked
        .world()
        .resource_mut::<NetHost>()
        .session_mut()
        .launch(Tick(200))
        .expect("the fresh epoch launches");
    let new_peer = linked
        .world()
        .resource::<NetClient>()
        .session()
        .grant()
        .expect("the fresh client holds a grant")
        .peer;
    linked
        .world()
        .resource_mut::<NetHost>()
        .session_mut()
        .announce_spawn(
            Tick(200),
            reconnected_plane(),
            LinkedWorld::blueprint(),
            Some(new_peer),
        )
        .expect("the fresh spawn announcement publishes");
    linked.step_until("the fresh epoch never ingested", |report| {
        matches!(
            report.ingested.as_ref().map(|ingest| ingest.outcome),
            Some(IngestOutcome::Applied)
        )
    });
    let mirror = linked.world().resource::<NetMirror>();
    assert!(
        mirror
            .mirror()
            .aircraft(reconnected_plane())
            .is_some_and(|aircraft| aircraft.actor.session == reconnected_id()),
        "the fresh snapshot applied"
    );
    assert_eq!(
        mirror.mirror().aircraft_count(),
        1,
        "the mirror holds only the fresh generation's aircraft"
    );
    // The wire names the fresh actor and the drive attaches it — check the
    // persistent predictor binding, because `report.attached` is per-run and
    // the attach may already have landed during the ingest wait.
    linked.step_until_state("the fresh local actor was never attached", |world| {
        world
            .get_resource::<NetMirror>()
            .is_some_and(|mirror| mirror.local_actor() == Some(reconnected_plane()))
    });
    assert!(
        linked.report().mirror_refusals.is_empty(),
        "the fresh session applied cleanly: {:?}",
        linked.report().mirror_refusals
    );
}

#[test]
fn accept_f57_c_01_every_refusal_reaches_the_report() {
    let mut linked = LinkedWorld::new();
    linked.join_and_launch(Tick(100));

    // A mirror serving this session but a wrong epoch is *kept* (the session
    // is the grant's), and every feed is refused by name: `ingested` carries
    // the typed `IngestRefusal`, not a dropped packet.
    linked.world().insert_resource(NetMirror(NetSession::new(
        session_id(),
        WorldOrigin::new(
            OriginEpoch(9),
            WorldPosition::try_new([50_000.0, 500.0, -80_000.0]).expect("finite"),
        ),
        InterpolationConfig::default(),
    )));
    linked.step_until("the epoch mismatch was never reported", |report| {
        matches!(
            report.ingested.as_ref().map(|ingest| ingest.outcome),
            Some(IngestOutcome::Refused(IngestRefusal::EpochMismatch {
                snapshot: 1,
                local: 9,
            }))
        )
    });

    // Restore a serving mirror for the rest of the run.
    linked.world().remove_resource::<NetMirror>();
    linked.step_until("the rebuilt mirror never ingested", |report| {
        matches!(
            report.ingested.as_ref().map(|ingest| ingest.outcome),
            Some(IngestOutcome::Applied)
        )
    });

    // The wire naming a second local actor is a contradiction the drive
    // reports once and keeps readable: `local_actor` follows the wire while
    // `attached` still names the predictor's real owner.
    let peer = linked
        .world()
        .resource::<NetClient>()
        .session()
        .grant()
        .expect("the client holds a grant")
        .peer;
    let at = linked.report().at.expect("a committed tick");
    {
        let mut host = linked.world().resource_mut::<NetHost>();
        host.session_mut()
            .announce_spawn(at, plane(), LinkedWorld::blueprint(), Some(peer))
            .expect("the first announcement publishes");
    }
    linked.step_until("the local actor was never attached", |report| {
        report.attached == Some(plane())
    });
    {
        let mut host = linked.world().resource_mut::<NetHost>();
        host.session_mut()
            .announce_spawn(at, remote(), LinkedWorld::blueprint(), Some(peer))
            .expect("the second announcement publishes");
    }
    linked.step_until("the second attach refusal never surfaced", |report| {
        report.local_actor == Some(remote())
            && report
                .mirror_refusals
                .iter()
                .any(|refusal| refusal.contains("local actor"))
    });
    let report = linked.report();
    assert_eq!(
        linked.world().resource::<NetMirror>().local_actor(),
        Some(plane()),
        "the predictor still owns the first actor"
    );
    assert_eq!(report.local_actor, Some(remote()));

    // Two pose owners are an ambiguity, reported by name rather than picked.
    spawn_owner_body(linked.world());
    spawn_owner_body(linked.world());
    linked.step_until("the duplicate owners were never named", |report| {
        report.pose_owner == NetPoseOwner::Ambiguous(2)
    });
    assert_eq!(linked.report().corrected, None);

    // A ledger record the snapshot schema cannot express refuses the whole
    // publish — `PublishError::Snapshot` naming the quantizer's own bound.
    linked
        .world()
        .resource_mut::<NetHost>()
        .ledger_mut()
        .spawn(NetActorState::spawn(
            ActorId {
                session: session_id(),
                serial: 11,
            },
            WorldPosition::try_new([60_000_000.0, 500.0, -80_000.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        ))
        .expect("the far spawn enters the ledger");
    linked.step_until("the unquantizable record was never refused", |report| {
        matches!(
            report.published,
            Some(NetPublish::Refused(PublishError::Snapshot(
                SnapshotError::OutOfRange { field, .. }
            ))) if field == "position"
        )
    });
}

#[test]
fn accept_f57_c_01_without_the_plugin_the_schedule_drives_nothing() {
    // The wiring-essentiality check: the same world, the same sessions, every
    // input present — only the production plugin missing. If the schedule
    // ever stopped being the thing that moves the session, this test fails.
    let mut linked = LinkedWorld::build(false);
    linked.step(60);

    assert!(
        linked.world().get_resource::<NetSessionReport>().is_none(),
        "without the plugin no report exists — nothing drives the session"
    );
    assert!(
        linked.world().get_resource::<NetMirror>().is_none(),
        "without the drive no mirror is ever built"
    );
    assert!(
        linked.world().get_resource::<NetPresentation>().is_none(),
        "without the sampler no presentation exists"
    );
    assert!(
        linked
            .world()
            .resource::<NetClient>()
            .session()
            .grant()
            .is_none(),
        "without the pump the handshake never completes"
    );
}
