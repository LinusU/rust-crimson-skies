//! Acceptance scenario F56-C (network half): the host's lobby options become
//! rule fields, the map it selected travels bound to the rules that resolved
//! from them, and everything the `cs_sim` resolver consumes is derived from
//! that one record — with every refusal propagated by name.
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-C`; contract `docs/contracts/UI-NETWORK.md` ("Lobby host changes
//! rules through a validated server action", "Content/rules mismatch rejects
//! launch before expensive asset loading"). The limits, roster counts and
//! score values are authored test inputs: the original per-mode values are
//! still unknown and block resolution (F56-A finding).

use std::collections::BTreeSet;

use cs_net::lobby::{LateJoin, LobbyRules, TeamMode};
use cs_net::rules::{
    CustomPlanes, DisconnectPolicy, HumanRange, HumanScaling, LaunchPlan, Limit, Lives, MatchRules,
    ResolveError, Respawn, RuleDraft, RuleField, RulesError, Spawn, StartError, StartRequest,
    Victory,
};
use cs_types::content::ContentId;

fn scenario() -> ContentId {
    ContentId::parse("multiplayer_scenario/slot.c1.mp2").expect("a scenario id")
}

fn lobby(team_mode: TeamMode, late_join: LateJoin) -> LobbyRules {
    LobbyRules {
        scenario: scenario(),
        banned: BTreeSet::new(),
        team_mode,
        late_join,
    }
}

/// The fields the installation does not answer: every rule except the two the
/// lobby's own options state.
fn unknown_without_lobby_options() -> Vec<RuleField> {
    RuleField::ALL
        .into_iter()
        .filter(|field| !matches!(field, RuleField::Teams | RuleField::LateJoin))
        .collect()
}

/// The rules a host would have measured for a mode, spelled as authored test
/// inputs — never as original values.
fn measured_rules(teams: TeamMode, late_join: LateJoin) -> MatchRules {
    let mut draft = RuleDraft {
        teams: Some(teams),
        late_join: Some(late_join),
        ..RuleDraft::default()
    };
    draft.time_limit = Some(Limit::At(18_000));
    draft.score_limit = Some(Limit::None);
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
    draft.resolve().expect("the authored draft resolves")
}

/// A host's options are rule values, not something a second component
/// re-derives: `from_lobby` fills them, everything the installation does not
/// answer stays blocked, and the selected map travels with the resolved rules.
#[test]
fn accept_f56_c_host_options_become_rule_fields_and_the_map_travels_with_them() {
    let lobby = lobby(TeamMode::Teams { teams: 2 }, LateJoin::Closed);
    let draft = RuleDraft::from_lobby(&lobby);

    assert_eq!(draft.teams, Some(TeamMode::Teams { teams: 2 }));
    assert_eq!(draft.late_join, Some(LateJoin::Closed));
    assert_eq!(
        draft.missing(),
        unknown_without_lobby_options(),
        "the lobby answers exactly two of the mode's rules"
    );

    // The unknown majority still blocks, and every unknown field is named:
    // host options never make an unmeasured mode launchable.
    match draft.resolve() {
        Err(ResolveError::Blocked(blocked)) => {
            assert_eq!(blocked.missing, unknown_without_lobby_options());
        }
        other => panic!("a mode whose rules are unknown must not resolve: {other:?}"),
    }

    // With the rest measured (here: authored), the launch is one record: the
    // lobby's map bound to its resolved rules.
    let mut draft = RuleDraft::from_lobby(&lobby);
    draft.time_limit = Some(Limit::At(18_000));
    draft.score_limit = Some(Limit::None);
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

    let plan = LaunchPlan::new(&lobby, rules).expect("the plan matches its lobby");
    assert_eq!(
        plan.scenario(),
        scenario(),
        "the map travels with the rules"
    );
    assert_eq!(
        plan.rules().teams(),
        TeamMode::Teams { teams: 2 },
        "the rules are the lobby's own options"
    );
    assert_eq!(plan.scenario(), lobby.scenario, "one lobby, one map");
}

/// Content/rules mismatch rejects the launch: rules that contradict the lobby
/// they are launched with are refused, by name, before anything starts.
#[test]
fn accept_f56_c_a_launch_that_contradicts_its_lobby_is_refused() {
    let team_lobby = lobby(TeamMode::Teams { teams: 2 }, LateJoin::Closed);
    let free_lobby = lobby(TeamMode::FreeForAll, LateJoin::Closed);
    let open_lobby = lobby(TeamMode::Teams { teams: 2 }, LateJoin::Open);

    // A team mode cannot start a free-for-all lobby, nor the other way round.
    let team_rules = measured_rules(TeamMode::Teams { teams: 2 }, LateJoin::Closed);
    assert_eq!(
        LaunchPlan::new(&free_lobby, team_rules.clone()),
        Err(StartError::TeamModeMismatch)
    );
    let free_rules = measured_rules(TeamMode::FreeForAll, LateJoin::Closed);
    assert_eq!(
        LaunchPlan::new(&team_lobby, free_rules),
        Err(StartError::TeamModeMismatch)
    );
    assert_eq!(
        LaunchPlan::new(&team_lobby, team_rules.clone())
            .expect("the matching pairing is accepted")
            .scenario(),
        scenario()
    );

    // The host may be stricter than the mode, never looser: opening late join
    // for a mode that forbids it is refused.
    let closed_rules = measured_rules(TeamMode::Teams { teams: 2 }, LateJoin::Closed);
    assert_eq!(
        LaunchPlan::new(&open_lobby, closed_rules),
        Err(StartError::LateJoinNotAllowed)
    );
    let open_rules = measured_rules(TeamMode::Teams { teams: 2 }, LateJoin::Open);
    assert!(
        LaunchPlan::new(&team_lobby, open_rules).is_ok(),
        "closing a mode that allows late join is the host's own choice"
    );
    assert!(
        !StartError::LateJoinNotAllowed.to_string().is_empty()
            && !StartError::TeamModeMismatch.to_string().is_empty(),
        "both refusals are displayable to the host"
    );
}

/// Scoring: the resolver's inputs are derived once, from the plan the host
/// launches — the limits and the victory label are never written a second
/// time — and the launch is checked against the lobby before it starts.
#[test]
fn accept_f56_c_the_resolved_rules_reach_the_resolver_inputs_once() {
    let lobby = lobby(TeamMode::Teams { teams: 2 }, LateJoin::Closed);
    let rules = measured_rules(TeamMode::Teams { teams: 2 }, LateJoin::Closed);
    let plan = LaunchPlan::new(&lobby, rules).expect("the plan matches its lobby");

    let limits = plan.resolver_limits();
    assert_eq!(limits, plan.rules().resolver_limits(), "one conversion");
    assert_eq!(limits.time_limit, Some(cs_types::Tick(18_000)));
    assert_eq!(
        limits.score_limit, None,
        "this authored mode has no score limit"
    );

    // The victory condition crosses the crate boundary by wire label, which
    // `cs_sim::multiplayer::result::VictoryRule::from_label` accepts or
    // refuses; an unknown label can never resolve a different match.
    assert_eq!(plan.rules().victory().label(), "highest_score");

    // Launch validation propagates its refusals out of the plan too.
    let ok = StartRequest {
        humans: 3,
        custom_planes: 0,
        largest_loadout: 10,
    };
    assert_eq!(plan.validate_start(&ok), Ok(()));
    assert_eq!(
        plan.validate_start(&StartRequest { humans: 1, ..ok }),
        Err(StartError::TooFewHumans { got: 1, min: 2 })
    );
    assert_eq!(
        plan.validate_start(&StartRequest {
            custom_planes: 1,
            ..ok
        }),
        Err(StartError::CustomPlanesForbidden)
    );
    assert_eq!(
        plan.validate_start(&StartRequest {
            largest_loadout: 11,
            ..ok
        }),
        Err(StartError::ComponentLimitExceeded { got: 11, limit: 10 })
    );

    // A limit the resolver could not represent is still refused where the
    // rules are resolved, not silently run differently.
    let mut draft = RuleDraft::from_lobby(&lobby);
    draft.time_limit = Some(Limit::At(18_000));
    draft.score_limit = Some(Limit::At(0));
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
    assert_eq!(
        draft.resolve(),
        Err(ResolveError::Invalid(RulesError::ZeroValue(
            RuleField::ScoreLimit
        )))
    );
}
