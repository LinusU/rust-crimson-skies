//! F39-E7: the shared contract's **six** condition distinctions against the
//! engine's five, and what the original's records actually spell.
//!
//! `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering") requires
//! conditions to distinguish "disabled, dead, captured, escaped, detached and
//! despawned". The engine's counter vocabulary declares five
//! ([`cs_sim::objectives::counters::CountKind`],
//! [`cs_content::objectives::DeclaredCountKind`]) and the F39 sheet's
//! non-negotiable 2 names the same five; the sixth, `detached`, was in neither,
//! with nothing recorded about it. These tests make the reconciliation
//! **queryable** and pin the measured verdict, so the five can never again read
//! as the contract's six.
//!
//! Each test fails when the answer it pins is removed:
//!
//! * [`contract_condition_distinctions`] losing a row, a vocabulary entry, a
//!   producer or a named reason fails the first two tests.
//! * `CountKind::producer` drifting from `CountKind::from_lifecycle` in either
//!   direction fails the third.
//! * The spelling rule matching a substring, or a part-state spelling, fails the
//!   fourth.
//! * The reader merging its three surfaces, or counting a stage that stands
//!   beside no threshold, fails the fifth.
//! * The census changing shape over the owner's installation fails the retail
//!   test, which re-derives every figure on each run.
//!
//! Capability `retail` is needed for the last one, so that test is `#[ignore]`d
//! in CI, which has no original data. The finding is
//! `docs/findings/2026-10-04-f39-e7-detached-condition-vocabulary.md`.

use cs_app::objectives::{
    ContractDistinction, ContractDistinctionReading, DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY,
    NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY, ReaderScope, contract_condition_distinctions,
    survey_retail_detached_declarations,
};
use cs_content::objectives::{
    DETACHED_SPELLING_STEMS, DeclaredCountKind, DetachedVocabularySurface,
    detached_spelling_family, measure_detached_vocabulary,
};
use cs_content::stunts::ZrdValue;
use cs_script::ir::ActorState;
use cs_script::ir::{ActorId, SymbolId};
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::counters::CountKind;
use cs_sim::world_actors::anchor::{AnchorSample, AnchorSocket, anchor_sample};
use cs_sim::world_actors::release::{PayloadSpec, release_payload};
use cs_sim::world_actors::trajectory::synthetic_train_trajectory;
use cs_types::Tick;
use cs_types::content::ContentId;

/// The five categories the engine declares, spelled out so a test can assert the
/// table covers all of them and not merely the ones it happens to name.
///
/// A list, not `CountKind::ALL`/`DeclaredCountKind::all()`, because what this
/// pins is the **pairing**: which contract word means which declared category
/// and which counted one, and the fact that the five resolve to exactly the
/// categories those vocabularies declare (asserted below against both `ALL`
/// lists). The list itself would say nothing that the pairing does not.
const DECLARED_CATEGORIES: &[(ContractDistinction, DeclaredCountKind, CountKind)] = &[
    (
        ContractDistinction::Disabled,
        DeclaredCountKind::Disabled,
        CountKind::Disabled,
    ),
    (
        ContractDistinction::Dead,
        DeclaredCountKind::Destroyed,
        CountKind::Destroyed,
    ),
    (
        ContractDistinction::Captured,
        DeclaredCountKind::Captured,
        CountKind::Captured,
    ),
    (
        ContractDistinction::Escaped,
        DeclaredCountKind::Escaped,
        CountKind::Escaped,
    ),
    (
        ContractDistinction::Despawned,
        DeclaredCountKind::Despawned,
        CountKind::Despawned,
    ),
];

/// The row for one distinction, or a panic naming the six that were resolved.
fn reading(distinction: ContractDistinction) -> ContractDistinctionReading {
    contract_condition_distinctions()
        .into_iter()
        .find(|reading| reading.distinction == distinction)
        .unwrap_or_else(|| {
            panic!(
                "{distinction:?} is missing from contract_condition_distinctions: the contract's six \
                 must all resolve"
            )
        })
}

// ---------------------------------------------------------------------------

#[test]
fn accept_f39_e7_the_six_distinctions_resolve_in_the_contract_s_own_order() {
    let readings = contract_condition_distinctions();
    assert_eq!(
        readings.len(),
        6,
        "the contract names six distinctions: {:?}",
        ContractDistinction::ALL
            .iter()
            .map(|distinction| distinction.contract_spelling())
            .collect::<Vec<_>>()
    );
    let spellings: Vec<&str> = readings
        .iter()
        .map(|reading| reading.contract_spelling)
        .collect();
    assert_eq!(
        spellings,
        vec![
            "disabled",
            "dead",
            "captured",
            "escaped",
            "detached",
            "despawned"
        ],
        "the contract's sentence spells six, in this order"
    );
    let distinctions: Vec<ContractDistinction> =
        readings.iter().map(|reading| reading.distinction).collect();
    assert_eq!(distinctions, ContractDistinction::ALL.to_vec());
    // The enum's own list is the contract's six, and the table has one row per
    // entry of it: no seventh row can appear and none of the six can go missing.
    assert_eq!(ContractDistinction::ALL.len(), 6);
    // Each contract word resolves to its own condition state, so the six are six
    // distinctions and not one word twice.
    let states: Vec<ActorState> = readings
        .iter()
        .map(|reading| reading.condition_state)
        .collect();
    let distinct: std::collections::BTreeSet<ActorState> = states.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        states.len(),
        "two contract words resolving to one condition state would not be six distinctions"
    );
    assert!(
        !states.contains(&ActorState::Alive),
        "the contract's six are states an actor is not alive in"
    );
    for spelling in &spellings {
        assert!(
            ContractDistinction::ALL
                .iter()
                .any(|distinction| distinction.contract_spelling() == *spelling),
            "{spelling} is not a spelling any distinction reports"
        );
    }
}

#[test]
fn accept_f39_e7_each_distinction_names_a_vocabulary_entry_and_a_producer_or_a_named_reason() {
    // The five that have a category name it, and the sixth says so by name.
    for (distinction, declared, counted) in DECLARED_CATEGORIES {
        let row = reading(*distinction);
        assert_eq!(
            row.declared,
            Some(*declared),
            "{distinction:?} must name its declared category"
        );
        assert_eq!(
            row.counted,
            Some(*counted),
            "{distinction:?} must name the counted category a session would count"
        );
        assert_eq!(
            ContractDistinction::counted_kind(*distinction),
            row.counted,
            "{distinction:?}: the counted category must come from the production lowering"
        );
        assert_eq!(
            ContractDistinction::producer(*distinction),
            row.producer,
            "{distinction:?}: the producer must come from CountKind::producer"
        );
    }

    let detached = reading(ContractDistinction::Detached);
    assert_eq!(
        detached.declared, None,
        "the declared vocabulary must not gain a Detached category: F39-E7 measured none"
    );
    assert_eq!(detached.counted, None);
    assert_eq!(detached.producer, None);
    assert_eq!(
        detached.unproduced_reason,
        Some(DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY)
    );
    assert!(!detached.distinction.is_counted_with_a_producer());
    assert_eq!(
        detached.condition_state,
        ActorState::Detached,
        "the script IR's condition vocabulary does have the sixth"
    );

    // Exactly three distinctions have a producer, and it is the transition the
    // damage lifecycle reports.
    let produced: Vec<(ContractDistinction, LifecycleKind)> = contract_condition_distinctions()
        .into_iter()
        .filter_map(|row| row.producer.map(|producer| (row.distinction, producer)))
        .collect();
    assert_eq!(
        produced,
        vec![
            (ContractDistinction::Dead, LifecycleKind::Destroyed),
            (
                ContractDistinction::Captured,
                LifecycleKind::OwnershipCaptured
            ),
            (ContractDistinction::Despawned, LifecycleKind::Despawned),
        ]
    );
    // ... and the other three name why they have none.
    for distinction in [ContractDistinction::Disabled, ContractDistinction::Escaped] {
        let row = reading(distinction);
        assert_eq!(
            row.producer, None,
            "{distinction:?} must report no producer"
        );
        assert_eq!(
            row.unproduced_reason,
            Some(NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY),
            "{distinction:?} must name the structural reason it has none"
        );
        assert!(!row.distinction.is_counted_with_a_producer());
    }
    for distinction in [
        ContractDistinction::Dead,
        ContractDistinction::Captured,
        ContractDistinction::Despawned,
    ] {
        let row = reading(distinction);
        assert_eq!(row.unproduced_reason, None);
        assert!(row.distinction.is_counted_with_a_producer());
        assert!(
            row.producer
                .is_some_and(|producer| CountKind::from_lifecycle(producer) == row.counted),
            "{distinction:?}: the named producer must be the transition that reports its category"
        );
    }

    // The five named categories are exactly the categories the two vocabularies
    // declare, so a sixth `CountKind`/`DeclaredCountKind` could not appear
    // without this failing, and the sixth distinction resolves to none of them.
    // Compared as sets: `DECLARED_CATEGORIES` is in the contract's order and the
    // `ALL` lists in each enum's declaration order, which are different orders.
    let mut declared: Vec<DeclaredCountKind> = DECLARED_CATEGORIES
        .iter()
        .map(|(_, declared, _)| *declared)
        .collect();
    declared.sort_unstable();
    assert_eq!(declared, DeclaredCountKind::all());
    let mut counted: Vec<CountKind> = DECLARED_CATEGORIES
        .iter()
        .map(|(_, _, counted)| *counted)
        .collect();
    counted.sort_unstable();
    assert_eq!(counted, CountKind::ALL);
    let mut resolved: Vec<CountKind> = contract_condition_distinctions()
        .into_iter()
        .filter_map(|row| row.counted)
        .collect();
    resolved.sort_unstable();
    assert_eq!(resolved, counted);
    assert_eq!(
        contract_condition_distinctions()
            .iter()
            .filter(|row| row.counted.is_none())
            .count(),
        1,
        "exactly one of the six resolves to no counted category: detached"
    );
}

#[test]
fn accept_f39_e7_the_producer_column_is_the_inverse_of_the_only_counting_path() {
    // The runtime's only producer for a category is the lifecycle transition
    // `from_lifecycle` maps to it, so the two directions must agree for every
    // category and every transition. Adding a transition that reports a category
    // — or removing one — breaks this test rather than quietly widening the table.
    for counted in CountKind::ALL.iter().copied() {
        let reported: Vec<LifecycleKind> = LifecycleKind::ALL
            .iter()
            .copied()
            .filter(|lifecycle| CountKind::from_lifecycle(*lifecycle) == Some(counted))
            .collect();
        assert!(
            reported.len() <= 1,
            "{counted:?} is reported by {reported:?}: a category must have one producer"
        );
        assert_eq!(
            counted.producer(),
            reported.first().copied(),
            "{counted:?}: producer() must be from_lifecycle asked the other way"
        );
    }
    // The two categories no transition reports are exactly the two the table
    // names as unproduced, and the two transitions that count toward no category
    // at all are named here so a new one cannot be added silently.
    let unreported: Vec<CountKind> = CountKind::ALL
        .iter()
        .copied()
        .filter(|counted| counted.producer().is_none())
        .collect();
    assert_eq!(unreported, vec![CountKind::Disabled, CountKind::Escaped]);
    // One assertion per transition, not one per category: `from_lifecycle` takes
    // only the lifecycle, so pairing it with a category would assert the same
    // fact once per category and read as if it checked a pairing.
    for lifecycle in [LifecycleKind::PilotBailout, LifecycleKind::MissionRemoved] {
        assert_eq!(
            CountKind::from_lifecycle(lifecycle),
            None,
            "{lifecycle:?} must count toward no category"
        );
    }
    // Every transition that reports a category must be that category's producer,
    // so the forward map and the derived reverse cannot drift in either
    // direction.
    for lifecycle in LifecycleKind::ALL.iter().copied() {
        if let Some(counted) = CountKind::from_lifecycle(lifecycle) {
            assert_eq!(
                counted.producer(),
                Some(lifecycle),
                "{lifecycle:?} reports {counted:?}, so that category's producer is this transition"
            );
        }
    }
}

#[test]
fn accept_f39_e7_a_detached_actor_is_represented_by_a_release_that_keeps_its_objective() {
    // The named verdict says the mechanic lives in the release/attach path,
    // which keeps objective identity instead of ending the actor's life. That is
    // a production behaviour, so it is asserted here rather than described: the
    // anchor is the production `anchor_sample` of the synthetic carrier, and the
    // payload is the production `release_payload`.
    let carrier = synthetic_train_trajectory();
    let tick = Tick(50);
    let socket = AnchorSocket {
        actor: ActorId(1),
        socket: 0,
        offset_m: [0.0, 3.0, 0.0],
    };
    let anchor: AnchorSample = anchor_sample(tick, &carrier.sample(tick), &socket);
    let objective = SymbolId(7);
    let released = release_payload(
        &anchor,
        PayloadSpec {
            actor: ActorId(9),
            faction: ContentId::parse("faction/allies").expect("a faction id"),
            objective: Some(objective),
            eject_m_s: [0.0, 0.0, 2.0],
        },
    );
    assert_eq!(
        released.objective,
        Some(objective),
        "a detached payload keeps the objective it counts for (F34 non-negotiable 4)"
    );
    assert_eq!(released.actor, ActorId(9));
    assert_eq!(released.tick, tick);
    assert_eq!(
        released.position_m, anchor.position_m,
        "the payload starts at the carrier's anchor"
    );
    assert_eq!(
        released.velocity_m_s,
        [10.0, 0.0, 2.0],
        "the payload inherits the carrier's velocity plus the ejection"
    );
    // The mission-removal transition is the one that leaves mission accounting
    // without counting a category, which is why the sixth distinction has no
    // counter entry: nothing reports it.
    assert_eq!(
        CountKind::from_lifecycle(LifecycleKind::MissionRemoved),
        None
    );
    assert!(LifecycleKind::MissionRemoved.is_terminal());
    assert!(!LifecycleKind::Destroyed.is_terminal());
}

#[test]
fn accept_f39_e7_the_spelling_rule_is_a_segment_match_over_a_published_stem_list() {
    // Every stem finds itself, is uppercase and is distinct, so the list cannot
    // carry a dead entry.
    let mut seen: Vec<&str> = Vec::new();
    for stem in DETACHED_SPELLING_STEMS {
        assert_eq!(
            detached_spelling_family(stem),
            Some(*stem),
            "{stem} must match its own stem"
        );
        assert_eq!(
            detached_spelling_family(&stem.to_ascii_lowercase()),
            Some(*stem),
            "{stem} must match case-insensitively"
        );
        assert!(!seen.contains(stem), "{stem} is listed twice");
        seen.push(stem);
    }
    assert!(seen.contains(&DETACHED_SPELLING_STEMS[0]));

    // A segment must *begin* with a stem: a prefix such as `MSG_OBJ_` is why the
    // rule is segment-based, and a stem inside a word is not a match.
    assert_eq!(detached_spelling_family("MSG_OBJ_RELEASE"), Some("RELEASE"));
    assert_eq!(detached_spelling_family("DETACHMENT"), Some("DETACH"));
    assert_eq!(detached_spelling_family("drop_paratroopers"), Some("DROP"));
    assert_eq!(
        detached_spelling_family("activate_dropoff_node"),
        Some("DROP")
    );
    assert_eq!(detached_spelling_family("free_the_goose"), Some("FREE"));
    assert_eq!(detached_spelling_family("redetached"), None);
    assert_eq!(detached_spelling_family("undroppable"), None);
    assert_eq!(detached_spelling_family(""), None);
    assert_eq!(detached_spelling_family("__"), None);

    // The ten part-state and actor spellings the installation's counted
    // conditions use match nothing, so no stem can swallow a state.
    for name in [
        "healthy",
        "healthy_part",
        "healthy_balloon",
        "panels",
        "reng11",
        "gasbag3",
        "piratezep",
        "cargozep2",
        "balmoral_1",
        "sprucegoose",
    ] {
        assert_eq!(
            detached_spelling_family(name),
            None,
            "{name} is a measured part-state or actor spelling and must match no stem"
        );
    }
}

// ---------------------------------------------------------------------------
// The three surfaces, on a hand-built record
// ---------------------------------------------------------------------------

/// One `[key, value]` pair, the shape a `targets.zrd` objective uses.
fn field(key: &str, value: ZrdValue) -> ZrdValue {
    ZrdValue::List(vec![ZrdValue::Text(key.to_owned()), value])
}

/// A flat `key, value, key, value, …` document, the shape `objectives.zrd`'s
/// root uses.
fn flat(fields: &[(&str, ZrdValue)]) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(ZrdValue::Text((*key).to_owned()));
        children.push(value.clone());
    }
    ZrdValue::List(children)
}

/// A block's own declarations, in the flat shape `zrd_flat_fields` reads.
fn block(fields: &[(&str, ZrdValue)]) -> ZrdValue {
    flat(fields)
}

/// The fixture: one block whose stages stand beside a threshold (the only
/// counted condition the original writes), one block whose stage stands beside
/// **no** threshold, one block that declares a detach as an event, and a
/// `targets.zrd` record carrying an objective kind.
fn fixture() -> (ZrdValue, ZrdValue) {
    let counted_block = block(&[
        (
            "INACTIVE_COMPLETION_COUNT",
            ZrdValue::List(vec![ZrdValue::Int(2)]),
        ),
        (
            "INACTIVE1",
            ZrdValue::List(vec![
                ZrdValue::Text("cargozep2".to_owned()),
                ZrdValue::Text("reng11".to_owned()),
                ZrdValue::Text("healthy".to_owned()),
            ]),
        ),
        (
            "INACTIVE2",
            ZrdValue::List(vec![ZrdValue::Text("cargozep3".to_owned())]),
        ),
        (
            "COMPLETED_SOUND_GROUP",
            ZrdValue::List(vec![ZrdValue::Text("snd_MN3Cargo1Half".to_owned())]),
        ),
    ]);
    let thresholdless_block = block(&[(
        "INACTIVE1",
        ZrdValue::List(vec![ZrdValue::Text("sprucegoose".to_owned())]),
    )]);
    let event_block = block(&[
        (
            "BEGIN_DORMANT",
            ZrdValue::List(vec![ZrdValue::Int(u32::MAX)]),
        ),
        (
            "WAKE_ANIM",
            ZrdValue::List(vec![ZrdValue::Text("drop_paratroopers".to_owned())]),
        ),
    ]);
    let objectives = flat(&[
        ("OBJECTIVE1", counted_block),
        ("OBJECTIVE2", thresholdless_block),
        ("OBJECTIVE3", event_block),
        // A key that is not a numbered block, and one that only looks like one.
        ("BEGIN_DORMANT", ZrdValue::List(vec![ZrdValue::Int(0)])),
        (
            "OBJECTIVEX",
            ZrdValue::List(vec![ZrdValue::Text("healthy".to_owned())]),
        ),
    ]);
    let targets = ZrdValue::List(vec![ZrdValue::List(vec![
        field("target_name", ZrdValue::Text("cargozep1".to_owned())),
        field("help_label", ZrdValue::Text("MSG_OBJ_RELEASE".to_owned())),
        field(
            "category_label",
            ZrdValue::Text("MSG_OBJ_ZEPPELIN".to_owned()),
        ),
        field(
            "description",
            ZrdValue::Text("MSG_TRGT_CARGO_ZEP".to_owned()),
        ),
        // A key the reader does not read, so it contributes nothing.
        field(
            "nodes",
            ZrdValue::List(vec![ZrdValue::Text("wing1".to_owned())]),
        ),
    ])]);
    (objectives, targets)
}

#[test]
fn accept_f39_e7_the_reader_keeps_the_three_surfaces_apart() {
    let (objectives, targets) = fixture();
    let measured = measure_detached_vocabulary(&objectives, Some(&targets));

    // Surface 1: only the stages of a block that declares a threshold, and only
    // its condition elements — the sound group is a declaration, not a count.
    assert_eq!(
        measured.distinct(DetachedVocabularySurface::CountedCondition),
        vec!["cargozep2", "cargozep3", "healthy", "reng11"],
        "a stage beside no threshold is not a counted condition"
    );
    assert!(
        !measured
            .distinct(DetachedVocabularySurface::CountedCondition)
            .contains(&"sprucegoose"),
        "OBJECTIVE2 declares no INACTIVE_COMPLETION_COUNT, so its stage is not counted"
    );
    assert!(
        !measured
            .distinct(DetachedVocabularySurface::CountedCondition)
            .contains(&"snd_MN3Cargo1Half"),
        "a sound group is a declaration, never a counted condition"
    );

    // Surface 2: the three localized objective kinds, and nothing else.
    assert_eq!(
        measured.distinct(DetachedVocabularySurface::ObjectiveKind),
        vec!["MSG_OBJ_RELEASE", "MSG_OBJ_ZEPPELIN", "MSG_TRGT_CARGO_ZEP"],
        "only help_label, category_label and description are objective kinds"
    );
    assert!(
        !measured
            .distinct(DetachedVocabularySurface::ObjectiveKind)
            .contains(&"wing1"),
        "a nodes entry is not an objective kind"
    );

    // Surface 3: the block declarations, including the detach as an event.
    let declarations = measured.distinct(DetachedVocabularySurface::BlockDeclaration);
    for name in ["drop_paratroopers", "sprucegoose", "snd_MN3Cargo1Half"] {
        assert!(
            declarations.contains(&name),
            "{name} is declared by a block and must be read as one"
        );
    }
    assert!(
        !declarations.contains(&"healthy") && !declarations.contains(&"reng11"),
        "a stage beside a threshold is read once, as a counted condition"
    );

    // The surfaces have different denominators: merging them would hide exactly
    // what this stage measured.
    let counted = measured.surface_sites(DetachedVocabularySurface::CountedCondition);
    let kinds = measured.surface_sites(DetachedVocabularySurface::ObjectiveKind);
    let declared = measured.surface_sites(DetachedVocabularySurface::BlockDeclaration);
    assert_eq!((counted, kinds, declared), (4, 3, 3));
    // The surfaces hold **disjoint** name sets here: merging them would let a
    // release-family objective kind read as a counted condition, which is exactly
    // the confusion this stage measures against.
    let counted_names = measured.distinct(DetachedVocabularySurface::CountedCondition);
    let kind_names = measured.distinct(DetachedVocabularySurface::ObjectiveKind);
    let declaration_names = measured.distinct(DetachedVocabularySurface::BlockDeclaration);
    for name in &counted_names {
        assert!(!kind_names.contains(name) && !declaration_names.contains(name));
    }
    for name in &kind_names {
        assert!(!declaration_names.contains(name));
    }
    assert_eq!(
        counted_names.len() + kind_names.len() + declaration_names.len(),
        measured.names.len(),
        "every name is read once, on one surface"
    );
    assert_eq!(
        measured.distinct_names(),
        vec![
            (DetachedVocabularySurface::CountedCondition, 4),
            (DetachedVocabularySurface::ObjectiveKind, 3),
            (DetachedVocabularySurface::BlockDeclaration, 3),
        ]
    );

    // The family is attributed per surface: the release family appears once as an
    // objective kind and never as a counted condition, which is the whole
    // measured shape of the original's declarations.
    assert_eq!(
        measured.family_sites(DetachedVocabularySurface::CountedCondition, "RELEASE"),
        0
    );
    assert_eq!(
        measured.family_sites(DetachedVocabularySurface::ObjectiveKind, "RELEASE"),
        1
    );
    assert_eq!(
        measured.family_sites(DetachedVocabularySurface::BlockDeclaration, "DROP"),
        1
    );
    assert_eq!(measured.sites_in_family("DETACH"), 0);
    assert_eq!(
        measured.unclaimed_sites(DetachedVocabularySurface::ObjectiveKind),
        2
    );
    let release = measured.sites_of_family("RELEASE");
    assert_eq!(release.len(), 1);
    assert_eq!(release[0].name, "MSG_OBJ_RELEASE");
    assert_eq!(release[0].surface, DetachedVocabularySurface::ObjectiveKind);
    assert_eq!(release[0].block, None);

    // An absent `targets.zrd` is a measured absence: no objective-kind name, and
    // the other two surfaces are unchanged.
    let without_targets = measure_detached_vocabulary(&objectives, None);
    assert_eq!(
        without_targets.surface_sites(DetachedVocabularySurface::ObjectiveKind),
        0
    );
    assert_eq!(
        without_targets.surface_sites(DetachedVocabularySurface::CountedCondition),
        counted
    );
    // An empty record measures nothing, which is a measurement.
    assert_eq!(
        measure_detached_vocabulary(&ZrdValue::List(vec![]), Some(&ZrdValue::List(vec![]))),
        Default::default()
    );
}

// ---------------------------------------------------------------------------
// The measured census over the owner's installation
// ---------------------------------------------------------------------------

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e7_the_installation_spell_no_detached_category_on_either_counting_surface() {
    let census = survey_retail_detached_declarations(&game_dir()).expect("the census surveys");

    // The installation this stage measured, fingerprinted by production
    // discovery. A different installation must not pass this test: the numbers
    // below are this one's.
    assert_eq!(
        census.install_sha256(),
        "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978"
    );

    // Both denominators: F39-D's census and F39-E4's covered the mission-scoped
    // readers only, so the shared and world-group readers are counted here.
    assert_eq!(census.readers(), 62);
    assert_eq!(census.mission_readers(), 53);
    assert_eq!(census.shared_readers(), 9);
    assert_eq!(
        census.readers(),
        census.mission_readers() + census.shared_readers()
    );
    assert_eq!(
        census
            .rows()
            .iter()
            .filter(|row| matches!(row.scope, ReaderScope::Mission(_)))
            .count(),
        53
    );
    assert!(
        census
            .rows()
            .iter()
            .any(|row| row.scope.archive() == "zbd/zrdr.zbd"),
        "the shared reader must be in the denominator"
    );
    // A mission reader with no objective record is a refusal, so every archive
    // that declares none is a shared or world-group reader — and **measured**,
    // all nine of them are. That is the wider denominator F39-D left open,
    // answered: the objective-declaration vocabulary is mission-scoped. The
    // objective-kind surface is not, because one shared reader does declare
    // targets — F39-E4's unknown #3 seen from this side.
    assert_eq!(census.archives_without_objectives().len(), 9);
    assert_eq!(
        census.archives_without_objectives(),
        census
            .rows()
            .iter()
            .filter(|row| matches!(row.scope, ReaderScope::Shared(_)))
            .map(|row| row.scope.archive())
            .collect::<Vec<_>>()
    );
    assert_eq!(census.archives_without_targets().len(), 9);
    assert!(
        !census
            .archives_without_targets()
            .contains(&"zbd/c1c/zrdr.zbd"),
        "the shared c1c reader declares a targets.zrd, so it contributes objective kinds"
    );
    assert_eq!(
        census
            .rows()
            .iter()
            .filter(|row| row.targets_sha256.is_some())
            .count(),
        53,
        "52 mission readers plus the shared c1c reader"
    );
    assert!(
        census.archives_without_targets().contains(&"zbd/c1c/m01"),
        "the one mission reader with no targets.zrd is named: {:?}",
        census.archives_without_targets()
    );

    // The three surfaces, with this installation's own denominators.
    assert_eq!(census.counted_conditions(), 3244);
    assert_eq!(census.objective_kinds(), 758);
    assert_eq!(
        census.distinct_names(DetachedVocabularySurface::CountedCondition),
        191
    );
    assert_eq!(
        census.distinct_names(DetachedVocabularySurface::ObjectiveKind),
        170
    );
    assert!(census.counted_conditions() > 0 && census.objective_kinds() > 0);

    // The measured answer: not one name on either surface that could declare a
    // category spells the contract's own word, and not one spells any other stem
    // of the family except the single `MSG_OBJ_RELEASE` objective kind below.
    assert_eq!(
        census.detached_category_sites(),
        0,
        "no counted condition and no objective kind spells a detached category"
    );
    assert_eq!(
        census.family_sites(DetachedVocabularySurface::CountedCondition, "DETACH"),
        0
    );
    assert_eq!(
        census.family_sites(DetachedVocabularySurface::ObjectiveKind, "DETACH"),
        0
    );
    assert_eq!(
        census.family_sites(DetachedVocabularySurface::BlockDeclaration, "DETACH"),
        0
    );
    assert_eq!(
        census.family_sites(DetachedVocabularySurface::CountedCondition, "RELEASE"),
        0
    );
    assert_eq!(
        census.family_sites(DetachedVocabularySurface::CountedCondition, "DROP"),
        0
    );
    assert_eq!(
        census.family_sites(DetachedVocabularySurface::ObjectiveKind, "DROP"),
        0
    );

    // The one objective kind in the family, named: a localized label on a cargo
    // zeppelin, which F39-E4's finding already says is not a counted transition.
    let release = census.sites_of_family("RELEASE");
    assert_eq!(
        release,
        vec![(
            "zbd/c4/m03".to_owned(),
            DetachedVocabularySurface::ObjectiveKind,
            None,
            "MSG_OBJ_RELEASE".to_owned(),
        )]
    );

    // The non-zero that keeps the absence above from being vacuous: the original
    // does write detaches, in the block-declaration vocabulary, where they are
    // events. `zbd/c2/m05 OBJECTIVE23` is the clearest: a paratrooper drop that
    // completes an objective rather than counting detached actors.
    assert_eq!(census.detach_family_declaration_sites(), 24);
    let drop_sites = census.sites_of_family("DROP");
    assert_eq!(drop_sites.len(), 7);
    assert!(
        drop_sites.iter().all(|site| {
            site.1 == DetachedVocabularySurface::BlockDeclaration && site.2.is_some()
        }),
        "every drop the corpus spells is a block declaration, never a counted condition"
    );
    assert!(
        drop_sites
            .iter()
            .any(|site| site.2.as_deref() == Some("OBJECTIVE23")
                && site.0 == "zbd/c2/m05"
                && site.3 == "drop_paratroopers"),
        "the paratrooper drop must be measured where the corpus spells it: {drop_sites:?}"
    );
    // Every stem, over both surfaces that could declare a category.
    for (stem, per_surface) in census.family_counts() {
        assert_eq!(
            per_surface[0],
            (DetachedVocabularySurface::CountedCondition, 0),
            "{stem} must claim no counted condition"
        );
    }
    // ...and the finding's per-stem table, so the prose cannot drift from the
    // census: `DROP` 7 and `LAUNCH` 16 and `FREE` 1 are block declarations, the
    // one `RELEASE` site is an objective kind, and the other eight stems claim
    // nothing at all. (A hand-written table in a finding is not a measurement;
    // this is.)
    let block_declarations: Vec<(&str, usize)> = census
        .family_counts()
        .iter()
        .map(|(stem, per_surface)| {
            let count = per_surface
                .iter()
                .find(|(surface, _)| *surface == DetachedVocabularySurface::BlockDeclaration)
                .map_or(0, |(_, count)| *count);
            (*stem, count)
        })
        .collect();
    assert_eq!(
        block_declarations,
        vec![
            ("DETACH", 0),
            ("RELEASE", 0),
            ("DROP", 7),
            ("EJECT", 0),
            ("JETTISON", 0),
            ("LAUNCH", 16),
            ("LOOSE", 0),
            ("FREE", 1),
            ("UNDOCK", 0),
            ("UNCOUPLE", 0),
            ("DISCONNECT", 0),
            ("CASTOFF", 0),
        ]
    );
    assert_eq!(
        block_declarations
            .iter()
            .map(|(_, count)| count)
            .sum::<usize>(),
        census.detach_family_declaration_sites(),
        "the per-stem table and the family total must be the same sites"
    );
    // The `LAUNCH` sites the finding names, counted rather than asserted about:
    // spawn-group names in one archive, `launch_warhawk` seven times,
    // `launch_brigand` six and `launch_autogyro` three.
    let launch = census.sites_of_family("LAUNCH");
    assert!(launch.iter().all(|site| site.0 == "zbd/c4/m04"));
    let mut launch_names: Vec<&str> = launch.iter().map(|site| site.3.as_str()).collect();
    launch_names.sort_unstable();
    assert_eq!(
        launch_names,
        vec![
            "launch_autogyro",
            "launch_autogyro",
            "launch_autogyro",
            "launch_brigand",
            "launch_brigand",
            "launch_brigand",
            "launch_brigand",
            "launch_brigand",
            "launch_brigand",
            "launch_warhawk",
            "launch_warhawk",
            "launch_warhawk",
            "launch_warhawk",
            "launch_warhawk",
            "launch_warhawk",
            "launch_warhawk",
        ]
    );
    // The one `FREE` site, named.
    assert_eq!(
        census.sites_of_family("FREE"),
        vec![(
            "zbd/c5/m03".to_owned(),
            DetachedVocabularySurface::BlockDeclaration,
            Some("OBJECTIVE37".to_owned()),
            "free_the_goose".to_owned(),
        )]
    );
}
