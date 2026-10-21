//! Acceptance suite PARITY-M01-OUTCOME-B (#1654): the player's destruction
//! ends the mission as a failure through the terminal path, and the settled
//! run carries the debrief fields.
//!
//! The hull pool and the hit that empties it are **designed** test inputs
//! (`mission_session::player_victim`); Cash Earned is an explicit unknown.
//! Nothing here is an original measurement.

use bevy::app::AppExit;
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::App;
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_session::{
    MISSION_TARGET_NODE, MissionCockpitActions, MissionDebrief, MissionHost, MissionTally,
    MissionTerminalSource, PlayerVictim, build_headless, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::score::mission_outcome;
use cs_app::weapons::SessionDamageResolver;
use cs_script::ir::{Outcome, SymbolId};
use cs_sim::campaign::{
    CampaignNodeKey, CampaignRunId, EventKey, OutcomeAuthority, OutcomeId, ProfileId,
    SessionGeneration,
};
use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent};
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_types::Tick;
use cs_types::net::EventId;

use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

fn step(app: &mut App) {
    let timestep = app
        .world()
        .resource::<bevy::prelude::Time<bevy::time::Fixed>>()
        .timestep();
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(timestep);
    app.update();
}

fn exits(app: &App) -> usize {
    let mut cursor = MessageCursor::<AppExit>::default();
    app.world()
        .get_resource::<Messages<AppExit>>()
        .map_or(0, |messages| cursor.read(messages).count())
}

/// Empties the player's hull with one lethal hit through the shared resolver.
fn destroy_player(app: &mut App) {
    let victim = app.world().resource::<PlayerVictim>().clone();
    let tick = Tick(app.world().resource::<PhysicsTickLedger>().ticks);
    let resolver = app.world().resource::<SessionDamageResolver>();
    let hit = HitEvent::try_new(
        EventId {
            session: resolver.resolver().session(),
            tick,
            producer: 9_000,
            sequence: 0,
        },
        None,
        victim.actor,
        DamageNodeKey::new(MISSION_TARGET_NODE).expect("the hull key is valid"),
        DamageChannel::Internal,
        victim.integrity.value + 1.0,
    )
    .expect("a finite, non-negative hit");
    app.world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(tick, &[hit])
        .expect("the resolver accepts the lethal hit");
}

/// **A player who is never hurt does not end the run.**
#[test]
fn accept_m01_outcome_b_an_unhurt_player_leaves_the_run_going() {
    let scratch = Scratch::new("m01-outcome-b-unhurt");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    for _ in 0..40 {
        step(&mut app);
    }
    assert!(app.world().resource::<MissionHost>().terminal().is_none());
    assert_eq!(exits(&app), 0);
    teardown(&mut app);
}

/// **The scripted failure run: the player's destruction settles one failure
/// terminal, once, with the debrief fields.**
#[test]
fn accept_m01_outcome_b_player_destruction_settles_a_failure_with_the_debrief() {
    let scratch = Scratch::new("m01-outcome-b-failure");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    for _ in 0..130 {
        step(&mut app);
    }
    destroy_player(&mut app);
    let mut cursor = MessageCursor::<AppExit>::default();
    let mut exit_messages = 0;
    for _ in 0..8 {
        step(&mut app);
        exit_messages += app
            .world()
            .get_resource::<Messages<AppExit>>()
            .map_or(0, |messages| cursor.read(messages).count());
    }
    let host = app.world().resource::<MissionHost>();
    let terminal = host
        .terminal()
        .expect("the player's destruction settled")
        .clone();
    assert_eq!(terminal.outcome, TerminalOutcome::Failure);
    assert_eq!(terminal.source, MissionTerminalSource::PlayerDestroyed);
    assert_eq!(terminal.exit.code(), 1);
    assert_eq!(exit_messages, 1, "exactly one AppExit is sent");
    let settled_at = terminal.tick;
    for _ in 0..10 {
        step(&mut app);
    }
    assert_eq!(
        app.world()
            .resource::<MissionHost>()
            .terminal()
            .map(|t| t.tick),
        Some(settled_at),
        "a settled run settles once"
    );

    let debrief = terminal
        .debrief
        .as_ref()
        .expect("the terminal carries the debrief");
    assert_eq!(debrief.run_ticks, settled_at.0);
    assert!(debrief.run_ticks >= 130, "{debrief:?}");
    assert_eq!(
        debrief.run_time(),
        format!(
            "{}:{:02}",
            debrief.run_seconds() / 60,
            debrief.run_seconds() % 60
        )
    );
    assert_eq!(
        (debrief.shots, debrief.hits, debrief.planes_downed),
        (0, 0, 0)
    );
    assert!(
        !debrief.cash_earned.is_known(),
        "cash is an explicit unknown"
    );
    assert!(
        terminal.report_line.contains("outcome=failure"),
        "{}",
        terminal.report_line
    );
    assert!(terminal.report_line.contains("source=player_destroyed"));
    assert!(terminal.report_line.contains("cash_earned=unknown"));

    // The outcome transaction records a failure.
    let record = mission_outcome(
        &terminal,
        OutcomeId {
            profile: ProfileId::new("pilot.nathan").expect("valid profile"),
            run: CampaignRunId::new("run.one").expect("valid run"),
            session: SessionGeneration(terminal.session.0),
            terminal_event: EventKey {
                session: SessionGeneration(terminal.session.0),
                tick: terminal.tick,
                source: SymbolId(0),
                sequence: 0,
            },
        },
        CampaignNodeKey::new("m01").expect("valid node"),
        OutcomeAuthority::Authorized,
    )
    .expect("a failure maps to a record");
    assert_eq!(record.outcome, Outcome::Failed);
    teardown(&mut app);
}

/// **The debrief arithmetic: `m:ss` from ticks, hits over shots.**
#[test]
fn accept_m01_outcome_b_debrief_formats_run_time_and_hit_ratio() {
    let mut debrief = MissionDebrief::read(
        &bevy::prelude::World::new(),
        cs_types::net::SessionId::new(1).expect("nonzero"),
        0,
    );
    debrief.ticks_per_second = 120;
    debrief.run_ticks = 120 * 97 + 5;
    assert_eq!(debrief.run_time(), "1:37");
    assert_eq!(debrief.gun_hit_ratio(), None);
    debrief.shots = 8;
    debrief.hits = 2;
    assert_eq!(debrief.gun_hit_ratio(), Some(0.25));
}

/// **The player's trigger pulls count as shots in the debrief tally.**
#[test]
fn accept_m01_outcome_b_the_player_gun_tally_counts_fired_rounds() {
    let scratch = Scratch::new("m01-outcome-b-tally");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    for _ in 0..130 {
        step(&mut app);
    }
    for _ in 0..40 {
        app.world_mut()
            .resource_mut::<MissionCockpitActions>()
            .actions
            .push(cs_types::input::Action::Flight(
                cs_types::input::FlightCommand::FirePrimary,
            ));
        step(&mut app);
    }
    let tally = app.world().resource::<MissionTally>().clone();
    assert!(
        tally.shots > 0,
        "the player's rounds were counted: {tally:?}"
    );
    assert!(tally.hits <= tally.shots);
    teardown(&mut app);
}
