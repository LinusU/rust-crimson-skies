//! Acceptance scenario F56-A (network half): per-mode rules block while any
//! field is unknown, and a lobby is checked against them before it starts
//! (spec non-negotiable behaviors 1 and 5). Every test drives the production
//! `cs_net::rules` types; the values are authored test inputs, not original
//! rules.

use cs_net::lobby::{LateJoin, TeamMode};
use cs_net::rules::{
    CustomPlanes, DisconnectPolicy, HumanRange, HumanScaling, Limit, Lives, MatchRules,
    ResolveError, Respawn, RuleDraft, RuleField, RulesError, StartError, StartRequest,
};

fn complete() -> RuleDraft {
    RuleDraft {
        teams: Some(TeamMode::Teams { teams: 2 }),
        late_join: Some(LateJoin::Closed),
        time_limit: Some(Limit::At(18_000)),
        score_limit: Some(Limit::None),
        lives: Some(Lives::Unlimited),
        respawn: Some(Respawn::AfterTicks(180)),
        friendly_fire: Some(false),
        disconnect: Some(DisconnectPolicy::KeepScore),
        humans: Some(HumanRange { min: 2, max: 4 }),
        human_scaling: Some(HumanScaling::None),
        custom_planes: Some(CustomPlanes::Forbidden),
        component_limit: Some(Limit::At(10)),
    }
}

fn resolved() -> MatchRules {
    complete().resolve().expect("the authored draft resolves")
}

fn invalid(mutate: impl FnOnce(&mut RuleDraft)) -> RulesError {
    let mut draft = complete();
    mutate(&mut draft);
    match draft.resolve() {
        Err(ResolveError::Invalid(error)) => error,
        other => panic!("expected an invalid rule set, got {other:?}"),
    }
}

#[test]
fn accept_f56_a_an_empty_draft_is_blocked_naming_every_unknown_rule() {
    match RuleDraft::default().resolve() {
        Err(ResolveError::Blocked(blocked)) => assert_eq!(blocked.missing, RuleField::ALL),
        other => panic!("an unknown mode must not resolve: {other:?}"),
    }
}

#[test]
fn accept_f56_a_one_unknown_field_blocks_the_mode_and_is_named() {
    for field in RuleField::ALL {
        let mut draft = complete();
        match field {
            RuleField::Teams => draft.teams = None,
            RuleField::LateJoin => draft.late_join = None,
            RuleField::TimeLimit => draft.time_limit = None,
            RuleField::ScoreLimit => draft.score_limit = None,
            RuleField::Lives => draft.lives = None,
            RuleField::Respawn => draft.respawn = None,
            RuleField::FriendlyFire => draft.friendly_fire = None,
            RuleField::Disconnect => draft.disconnect = None,
            RuleField::Humans => draft.humans = None,
            RuleField::HumanScaling => draft.human_scaling = None,
            RuleField::CustomPlanes => draft.custom_planes = None,
            RuleField::ComponentLimit => draft.component_limit = None,
        }
        match draft.resolve() {
            Err(ResolveError::Blocked(blocked)) => assert_eq!(blocked.missing, [field]),
            other => panic!("{field:?} unknown must block: {other:?}"),
        }
    }
}

#[test]
fn accept_f56_a_contradictory_known_rules_are_refused() {
    assert_eq!(
        invalid(|d| d.time_limit = Some(Limit::At(0))),
        RulesError::ZeroValue(RuleField::TimeLimit)
    );
    assert_eq!(
        invalid(|d| d.lives = Some(Lives::Limited(0))),
        RulesError::ZeroValue(RuleField::Lives)
    );
    assert_eq!(
        invalid(|d| d.respawn = Some(Respawn::AfterTicks(0))),
        RulesError::ZeroValue(RuleField::Respawn)
    );
    assert_eq!(
        invalid(|d| d.time_limit = Some(Limit::None)),
        RulesError::NoLimit,
        "no time limit and no score limit: it could never end"
    );
    assert_eq!(
        invalid(|d| d.humans = Some(HumanRange { min: 0, max: 4 })),
        RulesError::BadHumanRange
    );
    assert_eq!(
        invalid(|d| d.humans = Some(HumanRange { min: 5, max: 4 })),
        RulesError::BadHumanRange
    );
    assert_eq!(
        invalid(|d| d.humans = Some(HumanRange { min: 2, max: 200 })),
        RulesError::BadHumanRange
    );
    assert_eq!(
        invalid(|d| d.teams = Some(TeamMode::Teams { teams: 3 })),
        RulesError::BadTeams,
        "three teams cannot start with two humans"
    );
    assert_eq!(
        invalid(|d| d.teams = Some(TeamMode::Teams { teams: 1 })),
        RulesError::BadTeams
    );
}

#[test]
fn accept_f56_a_human_count_scaling_must_cover_every_supported_count() {
    let table =
        |factors: Vec<u16>| invalid(|d| d.human_scaling = Some(HumanScaling::PerHuman(factors)));
    assert_eq!(
        table(vec![1000, 1500]),
        RulesError::BadScaling,
        "2..=4 needs three"
    );
    assert_eq!(table(vec![1000, 0, 2000]), RulesError::BadScaling);

    let mut draft = complete();
    draft.human_scaling = Some(HumanScaling::PerHuman(vec![1000, 1500, 2000]));
    let rules = draft.resolve().unwrap();
    assert_eq!(rules.scaling_permille(2), Some(1000));
    assert_eq!(rules.scaling_permille(4), Some(2000));
    assert_eq!(rules.scaling_permille(1), None);
    assert_eq!(rules.scaling_permille(5), None);
    assert_eq!(
        resolved().scaling_permille(3),
        None,
        "an unscaled mode has no factor"
    );
}

#[test]
fn accept_f56_a_a_launch_is_checked_against_human_count_planes_and_component_limit() {
    let rules = resolved();
    let ok = StartRequest {
        humans: 3,
        custom_planes: 0,
        largest_loadout: 10,
    };
    assert_eq!(rules.validate_start(&ok), Ok(()));
    assert_eq!(
        rules.validate_start(&StartRequest { humans: 1, ..ok }),
        Err(StartError::TooFewHumans { got: 1, min: 2 })
    );
    assert_eq!(
        rules.validate_start(&StartRequest { humans: 5, ..ok }),
        Err(StartError::TooManyHumans { got: 5, max: 4 })
    );
    assert_eq!(
        rules.validate_start(&StartRequest {
            custom_planes: 1,
            ..ok
        }),
        Err(StartError::CustomPlanesForbidden)
    );
    assert_eq!(
        rules.validate_start(&StartRequest {
            largest_loadout: 11,
            ..ok
        }),
        Err(StartError::ComponentLimitExceeded { got: 11, limit: 10 })
    );

    let mut open = complete();
    open.custom_planes = Some(CustomPlanes::Allowed);
    open.component_limit = Some(Limit::None);
    let open = open.resolve().unwrap();
    assert_eq!(
        open.validate_start(&StartRequest {
            humans: 2,
            custom_planes: 2,
            largest_loadout: 64
        }),
        Ok(())
    );
}
