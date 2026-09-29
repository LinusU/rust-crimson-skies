//! Acceptance scenario F15-A (AC01): cancel a mission load, switch world,
//! then complete the old IO future; no old entities appear.
//!
//! These tests exercise production code only — `cs_app::loading`'s
//! [`LoadTransaction`], [`IoTicket`]/[`IoCompletion`] identity stamps and
//! [`ReadyBundle::attach`], against real `cs_assets::vfs` session
//! generations and a real Bevy [`World`]. Removing the identity check in
//! `accept` or `attach` lets the stale completion in: the old world's
//! entities appear and these tests fail.

mod common;

use bevy::ecs::world::World;

use cs_app::loading::{
    CompletionVerdict, Criticality, HandoffError, IoOutcome, IoTicket, IssueError, LoadFailure,
    LoadIdentity, LoadItem, LoadRequest, LoadState, LoadTarget, LoadTransaction, LoadedItemBinding,
    RecoveryPath, TransitionError,
};
use cs_assets::vfs::SessionBuilder;
use cs_types::content::ContentKind;
use cs_types::evidence::ContentHash;

use common::{fixed_hash, synthetic_content, synthetic_context, synthetic_key};

fn install() -> ContentHash {
    fixed_hash(0x77)
}

fn critical_item(path: &str, name: &str, work_units: u64) -> LoadItem {
    LoadItem::new(
        synthetic_key("world", path),
        synthetic_content(ContentKind::Image, name),
        Criticality::GameplayCritical,
        work_units,
    )
    .expect("nonzero work units")
}

/// Opens a fresh content session selecting `world` — each `open` takes the
/// next `SessionGeneration`, so two sessions model the world switch.
fn open_session(world: &str) -> cs_assets::vfs::ContentSession {
    SessionBuilder::new(synthetic_context(install(), world)).open()
}

fn mission_request(
    session: cs_assets::vfs::SessionGeneration,
    world: &str,
    mission: &str,
    items: Vec<LoadItem>,
) -> LoadRequest {
    LoadRequest {
        session,
        target: LoadTarget::world(
            cs_types::asset_id::WorldGroup::new(world).expect("valid world spelling"),
        )
        .with_mission(cs_types::asset_id::MissionScope::new(mission).expect("valid mission")),
        items,
    }
}

fn read_outcome(byte: u8) -> IoOutcome {
    IoOutcome::Read {
        payload_sha256: fixed_hash(byte),
    }
}

/// How many entities in `world` carry a binding of `load`.
fn entities_of(world: &mut World, load: LoadIdentity) -> usize {
    world
        .query::<&LoadedItemBinding>()
        .iter(world)
        .filter(|binding| binding.load == load)
        .count()
}

/// How many entities in `world` carry any [`LoadedItemBinding`].
fn bound_entities(world: &mut World) -> usize {
    world.query::<&LoadedItemBinding>().iter(world).count()
}

/// The minimum acceptance scenario: the c1 mission load is cancelled
/// mid-flight, the world switches to c2, and only then does the old read
/// finish. Its result must be discarded everywhere and nothing of c1 may
/// appear in c2's world.
#[test]
fn accept_f15_a_cancelled_loads_late_completion_spawns_nothing() {
    // Mission load for world c1, one item already in flight.
    let session_c1 = open_session("zbd/c1");
    let mut load_c1 = LoadTransaction::issue(mission_request(
        session_c1.generation(),
        "zbd/c1",
        "m01",
        vec![
            critical_item("texture/hull.bmp", "c1.hull", 100),
            critical_item("texture/mask.bmp", "c1.mask", 300),
        ],
    ));
    load_c1.begin().expect("a requested load begins");
    let ticket_hull = load_c1.issue_io(0).expect("item 0 is issuable");
    let ticket_mask = load_c1.issue_io(1).expect("item 1 is issuable");

    // The first read lands while the load is still live; progress is
    // measured in its work units.
    assert_eq!(
        load_c1.accept(ticket_hull.complete(read_outcome(0xC1))),
        CompletionVerdict::Accepted
    );
    let progress = load_c1.progress();
    assert_eq!(progress.completed_units, 100);
    assert_eq!(progress.total_units, 400);

    // The user backs out: cancel the load, switch world to c2.
    let report = load_c1.cancel().expect("a loading transaction cancels");
    assert_eq!(report.detached, 1);
    assert_eq!(load_c1.state(), LoadState::Cancelled);
    // The in-flight read's own switch is flagged, so the IO worker can
    // stop at its next chunk boundary.
    assert!(ticket_mask.cancel_handle().is_cancelled());

    let session_c2 = open_session("zbd/c2");
    assert_ne!(session_c1.generation(), session_c2.generation());
    let mut load_c2 = LoadTransaction::issue(mission_request(
        session_c2.generation(),
        "zbd/c2",
        "m02",
        vec![critical_item("texture/wing.bmp", "c2.wing", 50)],
    ));
    load_c2.begin().expect("the successor load begins");
    let ticket_wing = load_c2.issue_io(0).expect("c2's item is issuable");

    // The old IO future completes *now*, after the switch. It is stale
    // everywhere: foreign to the new load, discarded by its own cancelled
    // one.
    let late = ticket_mask.complete(read_outcome(0xD0));
    assert_eq!(
        load_c2.accept(late.clone()),
        CompletionVerdict::Foreign {
            issued_by: load_c1.identity()
        },
        "the successor load must refuse a completion stamped by the old one"
    );
    assert_eq!(
        load_c1.accept(late),
        CompletionVerdict::Discarded {
            state: LoadState::Cancelled
        },
        "a cancelled load discards its own late completions"
    );
    assert!(
        matches!(
            load_c1.ready_bundle(),
            Err(HandoffError::NotReady {
                state: LoadState::Cancelled
            })
        ),
        "a cancelled load has no bundle to hand off"
    );

    // The new world's own load completes and hands off.
    assert_eq!(
        load_c2.accept(ticket_wing.complete(read_outcome(0xC2))),
        CompletionVerdict::Accepted
    );
    load_c2.validate().expect("a settled load validates");
    assert_eq!(load_c2.state(), LoadState::Ready);
    assert!(load_c2.is_world_interactive());
    let bundle = load_c2.ready_bundle().expect("the ready load has a bundle");

    let mut world = World::new();
    let spawned = bundle
        .attach(&mut world, load_c2.identity())
        .expect("the bundle of the expected load attaches");
    assert_eq!(spawned.len(), 1);
    assert_eq!(entities_of(&mut world, load_c2.identity()), 1);
    assert_eq!(
        entities_of(&mut world, load_c1.identity()),
        0,
        "no entity of the cancelled load may appear in the new world"
    );
}

/// The sibling race: the c1 load *finished* before the switch, so a bundle
/// exists — but the world now expects c2's identity, and attaching the old
/// bundle must spawn nothing.
#[test]
fn accept_f15_a_stale_bundle_attaches_no_entities_to_successor_world() {
    let session_c1 = open_session("zbd/c1");
    let mut load_c1 = LoadTransaction::issue(mission_request(
        session_c1.generation(),
        "zbd/c1",
        "m01",
        vec![critical_item("texture/hull.bmp", "c1.hull", 100)],
    ));
    load_c1.begin().expect("begins");
    let ticket = load_c1.issue_io(0).expect("issuable");
    assert_eq!(
        load_c1.accept(ticket.complete(read_outcome(0xC1))),
        CompletionVerdict::Accepted
    );
    load_c1.validate().expect("validates");
    let stale = load_c1.ready_bundle().expect("the finished load bundled");

    // The world switched: c2's load is what the world expects.
    let session_c2 = open_session("zbd/c2");
    let mut load_c2 = LoadTransaction::issue(mission_request(
        session_c2.generation(),
        "zbd/c2",
        "m02",
        vec![critical_item("texture/wing.bmp", "c2.wing", 50)],
    ));
    load_c2.begin().expect("begins");
    let ticket2 = load_c2.issue_io(0).expect("issuable");

    let mut world = World::new();
    assert!(matches!(
        stale.attach(&mut world, load_c2.identity()),
        Err(HandoffError::Foreign {
            bundle,
            expected,
        }) if bundle == load_c1.identity() && expected == load_c2.identity()
    ));
    // A fresh World already contains Bevy's internal entities; the count
    // that matters is bound entities, which a refused attach must not grow.
    assert_eq!(
        bound_entities(&mut world),
        0,
        "a stale bundle must leave the world untouched"
    );

    // Sanity: the same bundle still attaches to the world it belongs to —
    // the refusal is the identity check, not a broken bundle.
    let mut old_world = World::new();
    assert_eq!(
        stale
            .attach(&mut old_world, load_c1.identity())
            .expect("its own world accepts it")
            .len(),
        1
    );

    assert_eq!(
        load_c2.accept(ticket2.complete(read_outcome(0xC2))),
        CompletionVerdict::Accepted
    );
    load_c2.validate().expect("validates");
    let fresh = load_c2.ready_bundle().expect("bundled");
    fresh
        .attach(&mut world, load_c2.identity())
        .expect("the expected bundle attaches");
    assert_eq!(entities_of(&mut world, load_c1.identity()), 0);
    assert_eq!(entities_of(&mut world, load_c2.identity()), 1);
}

/// The closure hash is a content identity: two loads delivering equal
/// `(content id, payload digest)` sets hash equal whatever order the reads
/// finished in — the warm/cold equality of spec F15 AC04.
#[test]
fn accept_f15_a_closure_hash_depends_on_content_not_completion_order() {
    let make = |order: [usize; 2]| {
        let session = open_session("zbd/c1");
        let mut load = LoadTransaction::issue(mission_request(
            session.generation(),
            "zbd/c1",
            "m01",
            vec![
                critical_item("texture/hull.bmp", "c1.hull", 100),
                critical_item("texture/mask.bmp", "c1.mask", 300),
            ],
        ));
        load.begin().expect("begins");
        let mut tickets: Vec<Option<IoTicket>> = (0..2)
            .map(|index| load.issue_io(index).map(Some).expect("issuable"))
            .collect();
        // Item i's payload digest is fixed per item, not per order.
        let digests = [0xC1u8, 0xC5u8];
        for index in order {
            let ticket = tickets[index].take().expect("issued once");
            load.accept(ticket.complete(read_outcome(digests[index])));
        }
        load.validate().expect("validates");
        load.ready_bundle().expect("bundled").closure_hash()
    };
    assert_eq!(make([0, 1]), make([1, 0]));
}

/// Failure cases of the transaction contract: wrong-phase IO issue,
/// illegal transitions, a critical item's failure ending the load with the
/// recovery path recorded, and sticky cancellation.
#[test]
fn accept_f15_a_transaction_refuses_illegal_moves_and_names_failures() {
    let session = open_session("zbd/c1");
    let mut load = LoadTransaction::issue(mission_request(
        session.generation(),
        "zbd/c1",
        "m01",
        vec![
            critical_item("texture/hull.bmp", "c1.hull", 100),
            LoadItem::new(
                synthetic_key("world", "sound/engine.wav"),
                synthetic_content(ContentKind::Sound, "c1.engine"),
                Criticality::Deferred,
                50,
            )
            .expect("nonzero units"),
        ],
    ));

    // IO before the load began is refused, as is an early validate.
    assert!(matches!(
        load.issue_io(0),
        Err(IssueError::WrongState {
            state: LoadState::Requested
        })
    ));
    assert!(matches!(
        load.validate(),
        Err(TransitionError {
            from: LoadState::Requested,
            to: LoadState::Ready
        })
    ));

    load.begin().expect("begins");

    // A deferred item's fault is recorded and does not end the load; the
    // load still fails validation honestly by keeping the failure on record.
    let deferred_ticket = load.issue_io(1).expect("issuable");
    assert_eq!(
        load.accept(deferred_ticket.complete(IoOutcome::Fault {
            code: "io_fault",
            detail: "host file unreadable".to_owned(),
            recovery: RecoveryPath::Retry,
        })),
        CompletionVerdict::ItemFailed
    );
    assert_eq!(load.state(), LoadState::Loading);
    assert_eq!(load.failures().len(), 1);
    assert_eq!(load.failures()[0].recovery, RecoveryPath::Retry);
    assert_eq!(
        load.failures()[0].key,
        synthetic_key("world", "sound/engine.wav")
    );

    // A gameplay-critical fault ends the load: Failed, with the missing
    // dependency named.
    let critical_ticket = load.issue_io(0).expect("issuable");
    assert_eq!(
        load.accept(critical_ticket.complete(IoOutcome::Fault {
            code: "not_found",
            detail: "no mount holds the member".to_owned(),
            recovery: RecoveryPath::MissingDependency,
        })),
        CompletionVerdict::ItemFailed
    );
    assert_eq!(load.state(), LoadState::Failed);
    assert!(!load.is_world_interactive());
    assert_eq!(load.failures()[1].recovery, RecoveryPath::MissingDependency);
    assert!(matches!(
        load.ready_bundle(),
        Err(HandoffError::NotReady {
            state: LoadState::Failed
        })
    ));

    // Terminal is sticky: no cancel, no restart, no completions.
    assert!(matches!(
        load.cancel(),
        Err(TransitionError {
            from: LoadState::Failed,
            to: LoadState::Cancelled
        })
    ));
    assert!(matches!(
        load.begin(),
        Err(TransitionError {
            from: LoadState::Failed,
            to: LoadState::Loading
        })
    ));

    // Cancelling detaches outstanding work and refuses a second cancel.
    let session2 = open_session("zbd/c2");
    let mut load2 = LoadTransaction::issue(mission_request(
        session2.generation(),
        "zbd/c2",
        "m02",
        vec![critical_item("texture/wing.bmp", "c2.wing", 50)],
    ));
    load2.begin().expect("begins");
    load2.issue_io(0).expect("issuable");
    load2.cancel().expect("cancels");
    assert!(matches!(
        load2.cancel(),
        Err(TransitionError {
            from: LoadState::Cancelled,
            to: LoadState::Cancelled
        })
    ));
}

/// A ready bundle carries its deferred omissions on record rather than
/// silently shrinking the closure.
#[test]
fn accept_f15_a_ready_bundle_reports_deferred_omissions() {
    let session = open_session("zbd/c1");
    let mut load = LoadTransaction::issue(mission_request(
        session.generation(),
        "zbd/c1",
        "m01",
        vec![
            critical_item("texture/hull.bmp", "c1.hull", 100),
            LoadItem::new(
                synthetic_key("world", "sound/engine.wav"),
                synthetic_content(ContentKind::Sound, "c1.engine"),
                Criticality::Deferred,
                50,
            )
            .expect("nonzero units"),
        ],
    ));
    load.begin().expect("begins");
    let critical = load.issue_io(0).expect("issuable");
    let deferred = load.issue_io(1).expect("issuable");
    load.accept(critical.complete(read_outcome(0xC1)));
    load.accept(deferred.complete(IoOutcome::Fault {
        code: "io_fault",
        detail: "device timeout".to_owned(),
        recovery: RecoveryPath::Retry,
    }));
    load.validate()
        .expect("a load with only deferred failures validates");
    assert!(load.is_world_interactive());
    let bundle = load.ready_bundle().expect("bundled");
    assert_eq!(bundle.items().len(), 1);
    assert_eq!(
        bundle.omitted(),
        &[synthetic_key("world", "sound/engine.wav")]
    );
}

/// An empty closure is a legal load: there is nothing to read, so `begin`
/// walks it straight to `Validating` — without that rule an empty load
/// could never leave `Loading` — and its bundle is still the only entity
/// path, attaching zero entities.
#[test]
fn accept_f15_a_empty_closure_load_advances_and_bundles_empty() {
    let session = open_session("zbd/c1");
    let mut load = LoadTransaction::issue(mission_request(
        session.generation(),
        "zbd/c1",
        "m01",
        vec![],
    ));
    load.begin().expect("a requested load begins");
    assert_eq!(
        load.state(),
        LoadState::Validating,
        "an empty closure has nothing to read and advances like a settled load"
    );
    load.validate().expect("an empty closure validates");
    assert_eq!(load.state(), LoadState::Ready);
    assert!(load.is_world_interactive());
    let bundle = load.ready_bundle().expect("an empty ready bundle exists");
    assert!(bundle.items().is_empty());
    assert!(bundle.omitted().is_empty());

    let mut world = World::new();
    let spawned = bundle
        .attach(&mut world, load.identity())
        .expect("the empty bundle still attaches under its identity");
    assert!(spawned.is_empty());
    assert_eq!(bound_entities(&mut world), 0);
}

/// A gameplay-critical failure ends the load like a cancel does for the
/// reads still in flight: they are flagged cancelled — `cancel` cannot be
/// invoked on the now-terminal transaction — and their late completions
/// are discarded.
#[test]
fn accept_f15_a_critical_failure_detaches_in_flight_reads() {
    let session = open_session("zbd/c1");
    let mut load = LoadTransaction::issue(mission_request(
        session.generation(),
        "zbd/c1",
        "m01",
        vec![
            critical_item("texture/hull.bmp", "c1.hull", 100),
            critical_item("texture/mask.bmp", "c1.mask", 300),
        ],
    ));
    load.begin().expect("begins");
    let hull = load.issue_io(0).expect("issuable");
    let mask = load.issue_io(1).expect("issuable");

    assert_eq!(
        load.accept(hull.complete(IoOutcome::Fault {
            code: "not_found",
            detail: "no mount holds the member".to_owned(),
            recovery: RecoveryPath::MissingDependency,
        })),
        CompletionVerdict::ItemFailed
    );
    assert_eq!(load.state(), LoadState::Failed);
    assert!(
        mask.cancel_handle().is_cancelled(),
        "the failed load's remaining read must be detached, not left running"
    );
    assert_eq!(
        load.accept(mask.complete(read_outcome(0xD0))),
        CompletionVerdict::Discarded {
            state: LoadState::Failed
        },
        "the detached read's late completion is still discarded"
    );
}

/// Validation can refuse the closure it checks: the transaction table's
/// `Validating -> Failed` arc, reachable through `reject_validation`,
/// records the offending item's failure and recovery path.
#[test]
fn accept_f15_a_validation_refusal_fails_the_load_with_its_reason() {
    let session = open_session("zbd/c1");
    let mut load = LoadTransaction::issue(mission_request(
        session.generation(),
        "zbd/c1",
        "m01",
        vec![critical_item("texture/hull.bmp", "c1.hull", 100)],
    ));
    let refusal = || LoadFailure {
        key: synthetic_key("world", "texture/hull.bmp"),
        code: "digest_mismatch",
        detail: "the re-verified payload disagrees with its read digest".to_owned(),
        recovery: RecoveryPath::RebuildDerived,
    };

    // Too early: a load still reading is not validating yet, and a
    // refused call records nothing.
    load.begin().expect("begins");
    assert!(matches!(
        load.reject_validation(refusal()),
        Err(TransitionError {
            from: LoadState::Loading,
            to: LoadState::Failed
        })
    ));
    assert_eq!(load.state(), LoadState::Loading);
    assert!(load.failures().is_empty());

    let ticket = load.issue_io(0).expect("issuable");
    assert_eq!(
        load.accept(ticket.complete(read_outcome(0xC1))),
        CompletionVerdict::Accepted
    );
    assert_eq!(load.state(), LoadState::Validating);

    load.reject_validation(refusal())
        .expect("validation can refuse the closure");
    assert_eq!(load.state(), LoadState::Failed);
    assert!(!load.is_world_interactive());
    assert_eq!(load.failures().len(), 1);
    assert_eq!(load.failures()[0].code, "digest_mismatch");
    assert_eq!(load.failures()[0].recovery, RecoveryPath::RebuildDerived);
    assert_eq!(
        load.failures()[0].key,
        synthetic_key("world", "texture/hull.bmp")
    );
    assert!(matches!(
        load.ready_bundle(),
        Err(HandoffError::NotReady {
            state: LoadState::Failed
        })
    ));
}
