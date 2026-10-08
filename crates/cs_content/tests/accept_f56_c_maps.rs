//! Acceptance scenario F56-C (content half, and the wiring itself): a host's
//! selected map resolves to its slot, its lobby options become the rules, and
//! those rules start exactly one `MatchSession` — which ends into an
//! end-of-match record and restarts without leaking a score, a pickup or a
//! timer.
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-C`; contract `docs/contracts/UI-NETWORK.md`. This file is the one
//! place where all three producers and consumers meet: `cs_content` parses
//! bytes, `cs_net` holds the rules and the lobby's options, `cs_sim` owns the
//! running match. The limits and score values below are authored test inputs
//! — every original per-mode rule value is still `Resolved::Unknown` and
//! blocks resolution (F56-A finding).
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! installation and is run with `--include-ignored`; without `CS_GAME_DIR` it
//! fails loudly rather than passing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_content::multiplayer::{
    ScenarioSlot, SlotCatalog, SlotObjective, TARGET_DESCRIPTION_FLAG,
    TARGET_DESCRIPTION_REARM_BASE, discover_slots,
};
use cs_net::lobby::{LateJoin, LobbyRules, TeamMode};
use cs_net::rules::{
    CustomPlanes, DisconnectPolicy, HumanRange, HumanScaling, LaunchPlan, Limit, Lives, Respawn,
    RuleDraft, Spawn, Victory,
};
use cs_sim::multiplayer::objective::{ObjectiveAction, ObjectiveEvent, ObjectiveState};
use cs_sim::multiplayer::result::{
    LethalEvent, LethalKind, Limits, Roster, ScoreTable, Side, SubmitError, VictoryRule,
};
use cs_sim::multiplayer::session::{MatchSession, SessionConfig, SessionError};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::net::{EventId, PeerId, SessionId};

// ------------------------------------------------------------- fixtures ---

fn claim_id(name: &str) -> ClaimId {
    ClaimId::new(name).expect("a claim id")
}

fn record(description: &str, index: u32) -> SlotObjective {
    SlotObjective {
        index,
        description: Some(description.to_owned()),
        nodes: vec![format!("node{index}")],
        help_label: None,
        category_label: None,
        directives: vec!["objective".to_owned()],
    }
}

fn known_records(items: Vec<SlotObjective>) -> Resolved<Vec<SlotObjective>> {
    Resolved::Known(Known::new(
        items,
        Provenance {
            claim_id: claim_id("multiplayer.slot.objectives.fixture"),
            class: ClaimStatus::Designed,
            source: None,
        },
    ))
}

fn unknown_records(reason: &str) -> Resolved<Vec<SlotObjective>> {
    Resolved::unknown(claim_id("multiplayer.slot.objectives.fixture"), reason)
        .expect("a nonempty reason")
}

fn slot(
    id: &str,
    world_group: &str,
    number: u8,
    objectives: Resolved<Vec<SlotObjective>>,
) -> ScenarioSlot {
    ScenarioSlot {
        id: ContentId::parse(id).expect("a scenario id"),
        world_group: world_group.to_owned(),
        slot: number,
        program: SourceSpan::new(
            ContentHash::from_bytes([7_u8; 32]),
            &format!("ZBD/{world_group}/MP{number}/zrdr.zbd"),
            None,
            0,
            64,
            None,
        )
        .expect("a source span"),
        companions: Vec::new(),
        markers: Vec::new(),
        mode: Resolved::unknown(
            claim_id("multiplayer.slot.mode.fixture"),
            "the fixture states no mode family",
        )
        .expect("a nonempty reason"),
        objectives,
    }
}

fn fixture_catalog() -> SlotCatalog {
    SlotCatalog {
        slots: vec![
            slot(
                "multiplayer_scenario/slot.c1.mp1",
                "C1",
                1,
                known_records(vec![record(TARGET_DESCRIPTION_REARM_BASE, 0)]),
            ),
            slot(
                "multiplayer_scenario/slot.c1.mp2",
                "C1",
                2,
                known_records(vec![
                    record(TARGET_DESCRIPTION_FLAG, 0),
                    record(TARGET_DESCRIPTION_FLAG, 1),
                ]),
            ),
            slot(
                "multiplayer_scenario/slot.c1.mp3",
                "C1",
                3,
                unknown_records("the fixture's targets.zrd member is absent"),
            ),
        ],
        without_program: Vec::new(),
    }
}

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

fn generation(number: u64) -> SessionId {
    SessionId::new(number).expect("a nonzero session number")
}

fn event(session: SessionId, tick: u64, producer: u32, sequence: u32) -> EventId {
    EventId {
        session,
        tick: Tick(tick),
        producer,
        sequence,
    }
}

/// A lobby over a fixture map, with the host's own options set: free-for-all,
/// no late join.
fn lobby_for(scenario: &ScenarioSlot) -> LobbyRules {
    LobbyRules {
        scenario: scenario.id.clone(),
        banned: BTreeSet::new(),
        team_mode: TeamMode::FreeForAll,
        late_join: LateJoin::Closed,
    }
}

/// The host's launch: its options fill the two rules the lobby states
/// (`RuleDraft::from_lobby`), the remaining twelve are **authored test
/// inputs** standing in for values the installation does not answer, and the
/// map is bound to the rules that resolved.
fn launch_for(scenario: &ScenarioSlot) -> LaunchPlan {
    let lobby = lobby_for(scenario);
    let mut draft = RuleDraft::from_lobby(&lobby);
    draft.time_limit = Some(Limit::At(600));
    draft.score_limit = Some(Limit::At(2));
    draft.lives = Some(Lives::Unlimited);
    draft.respawn = Some(Respawn::AfterTicks(180));
    draft.spawn = Some(Spawn::OwnSide);
    draft.friendly_fire = Some(false);
    draft.victory = Some(Victory::HighestScore);
    draft.disconnect = Some(DisconnectPolicy::KeepScore);
    draft.humans = Some(HumanRange { min: 2, max: 4 });
    draft.human_scaling = Some(HumanScaling::None);
    draft.custom_planes = Some(CustomPlanes::Forbidden);
    draft.component_limit = Some(Limit::At(10));
    let rules = draft.resolve().expect("the authored draft resolves");
    LaunchPlan::new(&lobby, rules).expect("the plan matches its lobby")
}

/// The session a host starts from a launch plan and the map's objective
/// count: every field is read from a producer, none is invented here.
fn config_for(plan: &LaunchPlan, session: SessionId, objectives: u32) -> SessionConfig {
    let limits = plan.resolver_limits();
    SessionConfig {
        scenario: plan.scenario(),
        session,
        roster: Roster::free_for_all(&[peer(1), peer(2)]).expect("two pilots"),
        table: ScoreTable {
            kill: 1,
            crash: 1,
            team_kill: -1,
        },
        limits: Limits {
            time_limit: limits.time_limit,
            score_limit: limits.score_limit,
        },
        victory: VictoryRule::from_label(plan.rules().victory().label())
            .expect("the resolver runs this victory rule"),
        objectives,
    }
}

fn kill(session: SessionId, tick: u64, sequence: u32, killer: u16, victim: u16) -> LethalEvent {
    LethalEvent {
        id: event(session, tick, 0, sequence),
        victim: peer(victim),
        kind: LethalKind::Kill {
            killer: peer(killer),
        },
    }
}

fn claim(session: SessionId, tick: u64, objective: u32, claimant: u16) -> ObjectiveEvent {
    ObjectiveEvent {
        id: event(session, tick, 1, tick as u32),
        objective: cs_sim::multiplayer::objective::ObjectiveId::new(objective)
            .expect("a nonzero objective serial"),
        action: ObjectiveAction::Claim {
            claimant: peer(claimant),
        },
    }
}

fn delivery(session: SessionId, tick: u64, objective: u32) -> ObjectiveEvent {
    ObjectiveEvent {
        id: event(session, tick, 1, tick as u32),
        objective: cs_sim::multiplayer::objective::ObjectiveId::new(objective)
            .expect("a nonzero objective serial"),
        action: ObjectiveAction::Score,
    }
}

// ---------------------------------------------------------------- tests ---

/// The map side: a selected scenario id resolves to *its* slot (an id the
/// installation does not ship resolves to nothing, never to the first entry),
/// and a slot answers how many objectives a match on it declares — `None`
/// while its records are unknown, `Some(0)` when it genuinely declares none.
#[test]
fn accept_f56_c_a_selected_map_resolves_to_its_slot_and_objective_count() {
    let catalog = fixture_catalog();

    let capture = catalog
        .get(&ContentId::parse("multiplayer_scenario/slot.c1.mp2").expect("an id"))
        .expect("the shipped map resolves");
    assert_eq!(capture.slot, 2);
    assert_eq!(
        capture.possessable_objectives(),
        Some(2),
        "two carryable flags"
    );

    let deathmatch = catalog
        .get(&ContentId::parse("multiplayer_scenario/slot.c1.mp1").expect("an id"))
        .expect("the shipped map resolves");
    assert_eq!(
        deathmatch.possessable_objectives(),
        Some(0),
        "known records with nothing carryable declare no objectives"
    );

    let undecoded = catalog
        .get(&ContentId::parse("multiplayer_scenario/slot.c1.mp3").expect("an id"))
        .expect("the shipped map resolves");
    assert_eq!(
        undecoded.possessable_objectives(),
        None,
        "an undecodable targets.zrd refuses the launch instead of guessing a count"
    );

    assert!(
        catalog
            .get(&ContentId::parse("multiplayer_scenario/slot.zz.mp9").expect("an id"))
            .is_none(),
        "an id the installation does not ship is not the first slot"
    );
}

/// The wiring itself, end to end: map → host options → rules → launch plan →
/// session → end-of-match record → restart with nothing leaking across.
#[test]
fn accept_f56_c_map_options_and_rules_wire_into_one_match_that_restarts_cleanly() {
    let catalog = fixture_catalog();
    let map = catalog
        .get(&ContentId::parse("multiplayer_scenario/slot.c1.mp2").expect("an id"))
        .expect("the shipped map resolves");
    let objectives = map.possessable_objectives().expect("the records decoded");

    let plan = launch_for(map);
    assert_eq!(
        plan.scenario(),
        map.id,
        "the launch carries the map the host selected"
    );

    let mut played = MatchSession::start(config_for(&plan, generation(1), objectives as u32))
        .expect("the session starts");
    assert_eq!(played.scenario(), map.id, "the match runs on that map");
    assert_eq!(played.objectives(), objectives as u32);
    assert_eq!(
        played.limits().time_limit,
        Some(Tick(600)),
        "from the rules"
    );
    assert_eq!(played.limits().score_limit, Some(2), "from the rules");

    // Play it: pick up a flag, deliver it, and score twice.
    let flag = played.objective_ids().next().expect("a declared objective");
    played
        .submit_objective(claim(generation(1), 2, flag.get(), 1))
        .expect("queued");
    played.close_tick(Tick(2));
    played
        .submit_objective(delivery(generation(1), 3, flag.get()))
        .expect("queued");
    played.close_tick(Tick(3));
    played
        .submit_lethal(kill(generation(1), 4, 0, 1, 2))
        .expect("queued");
    played
        .submit_lethal(kill(generation(1), 4, 1, 1, 2))
        .expect("queued");
    assert!(
        played.close_tick(Tick(4)).ended,
        "the score limit ends the match"
    );

    let report = played.end_of_match().expect("the match ended");
    assert_eq!(report.scenario, map.id, "the results screen names the map");
    assert_eq!(report.session, generation(1));
    assert_eq!(report.reason_key(), "match.end.score_limit");
    assert_eq!(report.outcome_key(), "match.outcome.win");
    assert_eq!(report.captures.len(), 1, "one delivery was recorded");
    assert_eq!(report.captures[0].scorer, peer(1));
    assert_eq!(
        report.standings,
        vec![
            (Side::Participant(peer(1)), 2),
            (Side::Participant(peer(2)), 0)
        ],
        "both kills are the winner's; the delivery is on the ledger, not in the score"
    );

    // Restart on the same map under a new generation: no score, no pickup,
    // no timer and no stale results screen come across.
    played
        .restart(config_for(&plan, generation(2), objectives as u32))
        .expect("the restart is accepted");
    assert_eq!(played.scenario(), map.id, "the map is still selected");
    assert_eq!(played.clock(), None, "no timer leaked");
    assert_eq!(played.result(), None, "no result leaked");
    assert_eq!(played.end_of_match(), None, "no results screen leaked");
    assert_eq!(played.score(Side::Participant(peer(1))), Some(0));
    assert_eq!(played.score(Side::Participant(peer(2))), Some(0));
    for objective in played.objective_ids() {
        assert_eq!(
            played.objective_state(objective),
            Some(ObjectiveState::Home)
        );
        assert_eq!(played.captures_of(objective).map(<[_]>::len), Some(0));
    }
    assert!(
        matches!(
            played.submit_lethal(kill(generation(1), 9, 0, 1, 2)),
            Err(SessionError::Lethal(SubmitError::WrongSession { got }))
                if got == generation(1)
        ),
        "the finished match's packets never reach the new one"
    );
    assert!(
        !played.close_tick(Tick(1)).ended,
        "the new match runs from the start"
    );
    assert_eq!(played.clock(), Some(Tick(1)));
}

/// The two vocabularies the crate boundary separates are one set: every
/// victory condition `cs_net` can resolve is a rule the `cs_sim` resolver
/// runs, and a rule it does not run is refused instead of being coerced.
#[test]
fn accept_f56_c_the_rules_and_resolver_victory_vocabularies_are_one_set() {
    let mut from_rules: Vec<&str> = Victory::ALL.iter().map(|rule| rule.label()).collect();
    let mut from_resolver: Vec<&str> = VictoryRule::ALL.iter().map(|rule| rule.label()).collect();
    from_rules.sort_unstable();
    from_resolver.sort_unstable();
    assert_eq!(
        from_rules, from_resolver,
        "a condition the rules can express must be one the resolver runs"
    );
    for rule in Victory::ALL {
        assert_eq!(
            VictoryRule::from_label(rule.label()),
            Some(VictoryRule::HighestScore),
            "{} resolves in the resolver",
            rule.label()
        );
    }
    assert!(
        VictoryRule::from_label("hold_the_flag").is_none(),
        "a victory condition the resolver does not run is refused at the wiring"
    );
}

// -------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// Retail: every map the installation ships resolves through the catalog to
/// its own slot and answers how many objectives a match there declares, and
/// each of them starts and restarts a match with no score, pickup or timer
/// leak.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f56_c_retail_every_shipped_map_declares_its_objectives_and_restarts_cleanly() {
    use cs_assets::install::{discover, fingerprint};

    let dir = game_dir();
    let found = discover(&dir).expect("discovery reads the installation");
    let install = fingerprint(&found.manifest);
    let catalog = discover_slots(install, &found.manifest.files, |record| {
        std::fs::read(dir.join(record.relative_spelling.as_str()))
    })
    .expect("every slot archive reads");
    assert_eq!(catalog.slots.len(), 21);

    let mut with_objectives = 0;
    for map in &catalog.slots {
        let resolved = catalog
            .get(&map.id)
            .unwrap_or_else(|| panic!("{} must resolve to its own slot", map.id));
        assert_eq!(resolved.id, map.id);
        let count = map
            .possessable_objectives()
            .unwrap_or_else(|| panic!("{} must decode its objective records", map.id));
        if count > 0 {
            with_objectives += 1;
        }

        let plan = launch_for(map);
        let mut played = MatchSession::start(config_for(&plan, generation(1), count as u32))
            .unwrap_or_else(|error| panic!("{} must start a match: {error}", map.id));
        assert_eq!(played.scenario(), map.id);
        assert_eq!(played.objectives(), count as u32);
        assert_eq!(
            played.objective_ids().count(),
            count,
            "{} must declare exactly its objectives",
            map.id
        );
        played
            .restart(config_for(&plan, generation(2), count as u32))
            .unwrap_or_else(|error| panic!("{} must restart: {error}", map.id));
        assert_eq!(
            played.objective_ids().count(),
            count,
            "{} keeps its objectives across the restart",
            map.id
        );
        assert_eq!(played.clock(), None, "{} leaked a timer", map.id);
        assert_eq!(played.end_of_match(), None, "{} leaked a result", map.id);
        assert_eq!(
            played.score(Side::Participant(peer(1))),
            Some(0),
            "{} leaked a score",
            map.id
        );
        for objective in played.objective_ids() {
            assert_eq!(
                played.objective_state(objective),
                Some(ObjectiveState::Home),
                "{} leaked a pickup",
                map.id
            );
            assert_eq!(played.captures_of(objective).map(<[_]>::len), Some(0));
        }
    }

    assert_eq!(
        with_objectives, 5,
        "the five capture-the-flag slots are the maps that declare carryable flags"
    );
    assert!(
        catalog
            .get(&ContentId::parse("multiplayer_scenario/slot.zzz.mp9").expect("an id"))
            .is_none(),
        "an id the installation does not ship never resolves"
    );
}
