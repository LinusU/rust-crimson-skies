//! F42-C acceptance: fame, AI observations and scrapbook rewards (synthetic).
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-C`. Task test prefix: `accept_f42_c_`.
//!
//! Minimum scenario: **visit the same world in another mission with a
//! different eligible set**. Two synthetic missions share one world; each
//! declares its own stunt over the same gate. Flying that gate in each
//! mission must pay only that mission's stunt, into fame and the scrapbook.
//! Fixtures are `SyntheticFixture` / reconstructed: no original-fidelity claim.

use cs_app::stunts::{FameTally, StuntConsumeError, consume_stunt_outcomes, lower_mission_stunts};
use cs_content::scrapbook::{
    EntryKind, EntryVisibility, ScrapbookCatalog, ScrapbookEntry, Unlock, UnlockFact,
    UnlockFactKind,
};
use cs_content::stunts::{
    MissionScope, StuntDefinition, StuntDraft, StuntRepeat, declared_synthetic_gate_stunt,
    synthetic_gate_mission,
};
use cs_script::ir::ActorId;
use cs_sim::campaign::{ProfileId, SessionGeneration};
use cs_sim::records::ScrapbookRecords;
use cs_sim::stunts::{StuntBook, StuntLedger, StuntMovement, TraversalOutcome, TraversalRequest};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Resolved};
use cs_types::space::WorldPosition;

const SUBJECT: ActorId = ActorId(1);

fn id(kind: ContentKind, name: &str) -> ContentId {
    ContentId::from_source(kind, name).expect("valid id")
}

fn other_mission() -> ContentId {
    id(ContentKind::Mission, "synthetic.m02")
}

/// The synthetic gate stunt re-declared under `name`, `mission` and `photo`.
fn stunt(name: &str, mission: ContentId, photo: &str, repeat: StuntRepeat) -> StuntDefinition {
    let base = declared_synthetic_gate_stunt();
    let mut reward = base.reward().clone();
    reward.media = Resolved::Known(Known::new(
        Some(id(ContentKind::ScrapbookItem, photo)),
        match &base.reward().media {
            Resolved::Known(known) => known.provenance.clone(),
            Resolved::Unknown { .. } => unreachable!("the fixture media is known"),
        },
    ));
    StuntDefinition::try_new(StuntDraft {
        id: id(ContentKind::Stunt, name),
        origin: base.origin().clone(),
        world: base.world().clone(),
        gate: base.gate().clone(),
        follow_on_gates: Vec::new(),
        rules: base.rules().clone(),
        scope: MissionScope::try_new(vec![mission]).expect("valid scope"),
        criticality: base.criticality(),
        repeat,
        reward,
        provenance: base.provenance().clone(),
    })
    .expect("the stunt is valid")
}

fn declared() -> Vec<StuntDefinition> {
    vec![
        stunt(
            "synthetic.gate-a",
            synthetic_gate_mission(),
            "synthetic.photo-a",
            StuntRepeat::Once,
        ),
        stunt(
            "synthetic.gate-b",
            other_mission(),
            "synthetic.photo-b",
            StuntRepeat::Repeatable,
        ),
    ]
}

fn entry(photo: &str, stunt_name: &str) -> ScrapbookEntry {
    ScrapbookEntry {
        id: id(ContentKind::ScrapbookItem, photo),
        kind: EntryKind::StuntPhoto,
        title: id(ContentKind::StringResource, &format!("{photo}.title")),
        image: None,
        unlock: Resolved::Known(Known::new(
            Unlock::Fact(UnlockFact {
                kind: UnlockFactKind::StuntCompleted,
                subject: id(ContentKind::Stunt, stunt_name),
            }),
            cs_types::content::Provenance::designed(
                cs_types::evidence::ClaimId::new("f42c.synthetic-unlock").expect("claim"),
            ),
        )),
        visibility: EntryVisibility::HiddenUntilUnlocked,
        replay: None,
    }
}

fn catalog() -> ScrapbookCatalog {
    ScrapbookCatalog::new(vec![
        entry("synthetic.photo-a", "synthetic.gate-a"),
        entry("synthetic.photo-b", "synthetic.gate-b"),
    ])
    .expect("the catalog is valid")
}

fn book(session: SessionGeneration, mission: &ContentId) -> StuntBook {
    let rules = lower_mission_stunts(mission, declared().iter()).expect("lowers");
    StuntBook::new(
        session,
        ProfileId::new("profile-1").expect("profile"),
        mission.clone(),
        SUBJECT,
        rules,
        StuntLedger::new(session, Vec::new()),
    )
    .expect("book")
}

fn fly(book: &mut StuntBook, session: SessionGeneration, tick: u64) -> Vec<TraversalOutcome> {
    let at = |z: f64| WorldPosition::try_new([0.0, 0.0, z]).expect("finite");
    book.observe(&TraversalRequest::player_flight(
        session,
        SUBJECT,
        Tick(tick),
        StuntMovement::Swept {
            from_m: at(100.0),
            to_m: at(-100.0),
        },
    ))
    .expect("observed")
}

#[test]
fn accept_f42_c_same_world_other_mission_pays_only_its_own_eligible_stunt() {
    let session = SessionGeneration(1);
    let catalog = catalog();
    let mut fame = FameTally::default();
    let mut scrapbook = ScrapbookRecords::default();

    // Mission one: only gate-a exists.
    let mut book_a = book(session, &synthetic_gate_mission());
    let outcomes = fly(&mut book_a, session, 1);
    let report =
        consume_stunt_outcomes(session, &outcomes, &catalog, &mut fame, &mut scrapbook).unwrap();
    assert_eq!(report.fame_added, 25);
    assert_eq!(fame.total(), 25);
    assert_eq!(
        report.unlocked_media,
        vec![id(ContentKind::ScrapbookItem, "synthetic.photo-a")]
    );
    assert_eq!(report.sightings.len(), 1);
    assert_eq!(
        report.sightings[0].stunt,
        id(ContentKind::Stunt, "synthetic.gate-a")
    );

    // Mission two, same world and same gate: only gate-b is eligible, so
    // photo-a is not re-awarded and photo-b is.
    let session_b = SessionGeneration(2);
    let mut book_b = book(session_b, &other_mission());
    let outcomes = fly(&mut book_b, session_b, 1);
    let report =
        consume_stunt_outcomes(session_b, &outcomes, &catalog, &mut fame, &mut scrapbook).unwrap();
    assert_eq!(
        report.unlocked_media,
        vec![id(ContentKind::ScrapbookItem, "synthetic.photo-b")]
    );
    assert_eq!(
        report.sightings[0].stunt,
        id(ContentKind::Stunt, "synthetic.gate-b")
    );
    assert_eq!(fame.total(), 50);
}

#[test]
fn accept_f42_c_a_replayed_hand_over_moves_fame_once() {
    let session = SessionGeneration(1);
    let catalog = catalog();
    let mut fame = FameTally::default();
    let mut scrapbook = ScrapbookRecords::default();
    let mut book_a = book(session, &synthetic_gate_mission());
    let outcomes = fly(&mut book_a, session, 1);
    consume_stunt_outcomes(session, &outcomes, &catalog, &mut fame, &mut scrapbook).unwrap();
    let again =
        consume_stunt_outcomes(session, &outcomes, &catalog, &mut fame, &mut scrapbook).unwrap();
    assert_eq!(again.already_applied, 1);
    assert_eq!(again.fame_added, 0);
    assert!(again.sightings.is_empty());
    assert_eq!(fame.total(), 25);
}

#[test]
fn accept_f42_c_retry_pays_one_time_stunt_once_and_repeatable_each_time() {
    let catalog = catalog();
    let mut fame = FameTally::default();
    let mut scrapbook = ScrapbookRecords::default();
    // One-time stunt: the retry's book is seeded with the paid key, so the
    // second completion is refused upstream and pays nothing here.
    let first = SessionGeneration(1);
    let mut one = book(first, &synthetic_gate_mission());
    let outcomes = fly(&mut one, first, 1);
    consume_stunt_outcomes(first, &outcomes, &catalog, &mut fame, &mut scrapbook).unwrap();
    let retry = SessionGeneration(2);
    let rules = lower_mission_stunts(&synthetic_gate_mission(), declared().iter()).unwrap();
    let mut retried = StuntBook::new(
        retry,
        ProfileId::new("profile-1").unwrap(),
        synthetic_gate_mission(),
        SUBJECT,
        rules,
        StuntLedger::new(retry, one.ledger().paid().cloned().collect::<Vec<_>>()),
    )
    .unwrap();
    let outcomes = fly(&mut retried, retry, 1);
    let report =
        consume_stunt_outcomes(retry, &outcomes, &catalog, &mut fame, &mut scrapbook).unwrap();
    assert_eq!(report.fame_added, 0);
    assert_eq!(fame.total(), 25);

    // Repeatable stunt: same tick number in a new session still pays.
    let mut total = 0;
    for generation in [3, 4] {
        let session = SessionGeneration(generation);
        let mut repeat = book(session, &other_mission());
        let outcomes = fly(&mut repeat, session, 1);
        total += consume_stunt_outcomes(session, &outcomes, &catalog, &mut fame, &mut scrapbook)
            .unwrap()
            .fame_added;
    }
    assert_eq!(total, 50);
    assert_eq!(fame.total(), 75);
}

#[test]
fn accept_f42_c_missing_scrapbook_media_refuses_and_changes_nothing() {
    let session = SessionGeneration(1);
    let empty = ScrapbookCatalog::new(vec![entry("synthetic.photo-b", "synthetic.gate-b")])
        .expect("catalog");
    let mut fame = FameTally::default();
    let mut scrapbook = ScrapbookRecords::default();
    let mut book_a = book(session, &synthetic_gate_mission());
    let outcomes = fly(&mut book_a, session, 1);
    let error = consume_stunt_outcomes(session, &outcomes, &empty, &mut fame, &mut scrapbook)
        .expect_err("photo-a is not declared");
    assert!(matches!(error, StuntConsumeError::MediaNotInCatalog { .. }));
    assert_eq!(fame, FameTally::default());
    assert_eq!(scrapbook, ScrapbookRecords::default());
}

#[test]
fn accept_f42_c_refusals_and_advances_pay_nothing() {
    let session = SessionGeneration(1);
    let mut fame = FameTally::default();
    let mut scrapbook = ScrapbookRecords::default();
    let mut book_a = book(session, &synthetic_gate_mission());
    // Flying beside the gate: a refusal, not a completion.
    let at = |x: f64, z: f64| WorldPosition::try_new([x, 0.0, z]).expect("finite");
    let outcomes = book_a
        .observe(&TraversalRequest::player_flight(
            session,
            SUBJECT,
            Tick(1),
            StuntMovement::Swept {
                from_m: at(60.0, 100.0),
                to_m: at(60.0, -100.0),
            },
        ))
        .unwrap();
    let report =
        consume_stunt_outcomes(session, &outcomes, &catalog(), &mut fame, &mut scrapbook).unwrap();
    assert_eq!(report.fame_added, 0);
    assert!(report.sightings.is_empty() && report.unlocked_media.is_empty());
    assert_eq!(scrapbook, ScrapbookRecords::default());
}
