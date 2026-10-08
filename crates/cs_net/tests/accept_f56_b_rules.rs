//! Acceptance scenario F56-B (network half): a resolved mode carries the
//! spawn and victory fields F56-A could not express, and hands the resolver
//! exactly the limits it can hold (`specs/F56-original-multiplayer-scenarios-
//! and-mode-rules.md`, `### F56-B`).
//!
//! Every test drives the production `cs_net::rules` types; the values are
//! authored test inputs, not original rules.

use cs_net::lobby::{LateJoin, TeamMode};
use cs_net::rules::{
    CustomPlanes, DisconnectPolicy, HumanRange, HumanScaling, Limit, Lives, MatchRules,
    ResolveError, Respawn, RuleDraft, RuleField, RulesError, Spawn, Victory,
};
use cs_types::Tick;

fn complete() -> RuleDraft {
    RuleDraft {
        teams: Some(TeamMode::Teams { teams: 2 }),
        late_join: Some(LateJoin::Closed),
        time_limit: Some(Limit::At(18_000)),
        score_limit: Some(Limit::At(5)),
        lives: Some(Lives::Unlimited),
        respawn: Some(Respawn::AfterTicks(180)),
        spawn: Some(Spawn::OwnSide),
        friendly_fire: Some(false),
        victory: Some(Victory::HighestScore),
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

#[test]
fn accept_f56_b_spawn_and_victory_are_mode_fields_that_block_while_unknown() {
    for (field, blank) in [
        (
            RuleField::Spawn,
            (|d: &mut RuleDraft| d.spawn = None) as fn(&mut RuleDraft),
        ),
        (
            RuleField::Victory,
            (|d: &mut RuleDraft| d.victory = None) as fn(&mut RuleDraft),
        ),
    ] {
        let mut draft = complete();
        blank(&mut draft);
        match draft.resolve() {
            Err(ResolveError::Blocked(blocked)) => assert_eq!(
                blocked.missing,
                [field],
                "{field:?} unknown must block the launch"
            ),
            other => panic!("{field:?} unknown must block: {other:?}"),
        }
    }

    let rules = resolved();
    assert_eq!(rules.spawn(), Spawn::OwnSide);
    assert_eq!(rules.victory(), Victory::HighestScore);
}

#[test]
fn accept_f56_b_resolver_limits_bridge_without_copying_authority() {
    // The resolver's limits are derived from the resolved rules, not
    // re-authored: `resolver_limits` is the one conversion.
    let limits = resolved().resolver_limits();
    assert_eq!(limits.time_limit, Some(Tick(18_000)));
    assert_eq!(limits.score_limit, Some(5));

    // `Limit::None` is an absent limit, not a sentinel value.
    let mut draft = complete();
    draft.time_limit = Some(Limit::None);
    draft.score_limit = Some(Limit::At(3));
    let limits = draft.resolve().unwrap().resolver_limits();
    assert_eq!(limits.time_limit, None);
    assert_eq!(limits.score_limit, Some(3));
}

#[test]
fn accept_f56_b_a_score_limit_the_resolver_cannot_hold_is_refused() {
    let mut draft = complete();
    draft.score_limit = Some(Limit::At(i32::MAX as u32));
    assert!(draft.resolve().is_ok(), "i32::MAX still resolves");

    let mut draft = complete();
    draft.score_limit = Some(Limit::At(i32::MAX as u32 + 1));
    match draft.resolve() {
        Err(ResolveError::Invalid(RulesError::LimitTooLarge(field))) => {
            assert_eq!(field, RuleField::ScoreLimit)
        }
        other => panic!("an overwide score limit must be refused: {other:?}"),
    }

    // A time limit has no such ceiling: every u32 tick is representable.
    let mut draft = complete();
    draft.time_limit = Some(Limit::At(u32::MAX));
    assert!(draft.resolve().is_ok());
}
