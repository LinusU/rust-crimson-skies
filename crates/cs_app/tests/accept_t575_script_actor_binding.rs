//! Acceptance for #575: the session-qualified `ActorId` is paired with the
//! mission script's `ir::ActorId` by `cs_app::roster::ScriptActorBindings`, and
//! the F33-C `AllyEvent` callback reaches `MissionFacts` through it.
//!
//! The mission program, the roster and the damage records are newly authored
//! synthetic fixtures; the retail test only measures which bytes of the
//! original reference an actor (`docs/findings/2026-10-12-t575-session-actor-to-script-actor.md`).

use std::collections::BTreeMap;

use cs_app::damage::{lower_graph, lower_policy};
use cs_app::mission_control::survey_mission_control_programs;
use cs_app::roster::{
    ScriptActorBindings, ScriptBindingError, apply_roster_lifecycle, lower_roster, open_roster,
};
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_content::pilots::declared_synthetic_roster;
use cs_script::ir::{
    Action, ActorId as ScriptActor, ActorState, Condition, IR_VERSION, MissionProgram, Objective,
    SymbolId,
};
use cs_script::runtime::SessionGeneration;
use cs_sim::allies::{AlliesRoster, BriefingPlan, WingmateSlot};
use cs_sim::damage::{ActorId, DamageResolver, LifecycleKind};
use cs_sim::mission::MissionSession;
use cs_sim::weapons::FireResolver;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const SESSION: u64 = 61;
const WINGMAN_SCRIPT: ScriptActor = ScriptActor(4);

fn session_id(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(session: u64, serial: u64) -> ActorId {
    ActorId {
        session: session_id(session),
        serial,
    }
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn reward_id() -> ContentId {
    cid(ContentKind::Blueprint, "t575-wingman-lost")
}

/// An authored mission whose only objective fires when script actor 4 is Dead.
fn mission(state: ActorState) -> MissionSession {
    let program = MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-t575"),
        variables: vec![],
        objectives: vec![Objective {
            id: SymbolId(1),
            content: cid(ContentKind::Objective, "t575-obj"),
            condition: Condition::ActorIs {
                actor: WINGMAN_SCRIPT,
                state,
            },
            actions: vec![Action::GrantReward {
                reward: reward_id(),
            }],
            span: None,
        }],
    };
    MissionSession::launch(program, SessionGeneration(9), vec![reward_id()]).unwrap()
}

fn opened() -> AlliesRoster {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("fixture roster lowers");
    open_roster(SESSION, &lowered, &BriefingPlan::new()).expect("wings assign")
}

fn damage_for(actor: ActorId) -> DamageResolver {
    let declared = declared_synthetic_airframe_damage();
    let mut resolver = DamageResolver::new(session_id(SESSION), 1);
    resolver
        .register_actor(
            actor,
            lower_graph(&declared).unwrap(),
            lower_policy(&declared).unwrap(),
        )
        .unwrap();
    resolver
}

/// A wingmate's authoritative death reaches `ActorIs { Dead }` on the script
/// side only through the binding. Without it the objective never fires.
#[test]
fn accept_t575_a_wingmate_death_reaches_the_script_actor_condition() {
    let wingman = actor(SESSION, 2);
    let mut roster = opened();
    roster.register_wingmate(wingman, WingmateSlot(1)).unwrap();
    let mut damage = damage_for(wingman);
    let mut fire = FireResolver::new(SESSION, Tick(0));

    let mut bindings = ScriptActorBindings::new(session_id(SESSION));
    bindings
        .bind(wingman, WINGMAN_SCRIPT, Some("wingman_one"))
        .unwrap();
    assert_eq!(bindings.script_actor(&wingman), Some(WINGMAN_SCRIPT));
    assert_eq!(bindings.session_actor(&WINGMAN_SCRIPT), Some(wingman));
    assert_eq!(bindings.name_of(&WINGMAN_SCRIPT), Some("wingman_one"));

    let mut mission = mission(ActorState::Dead);
    let admit = bindings.translate(&Default::default());
    mission.advance_observed(&admit.input(), Tick(1)).unwrap();

    damage
        .record_lifecycle(wingman, LifecycleKind::Destroyed, Tick(2))
        .unwrap();
    let outcome = apply_roster_lifecycle(&mut roster, &damage, &mut fire);
    let batch = bindings.translate(&outcome.log);
    assert_eq!(
        batch.lifecycles,
        [(WINGMAN_SCRIPT, LifecycleKind::Destroyed)]
    );
    assert!(batch.unbound.is_empty());

    let tick = mission.advance_observed(&batch.input(), Tick(2)).unwrap();
    assert_eq!(
        mission.actor_facts().facts().actors.get(&WINGMAN_SCRIPT),
        Some(&ActorState::Dead)
    );
    assert_eq!(tick.mission.host.rewards_granted, [reward_id()]);
}

/// A bailout is a callback that writes no `ActorState`, so a `Dead` condition
/// stays unfired; an actor with no binding is reported, not dropped.
#[test]
fn accept_t575_unbound_and_bailout_callbacks_are_named_not_fabricated() {
    let wingman = actor(SESSION, 2);
    let stranger = actor(SESSION, 3);
    let mut roster = opened();
    roster.register_wingmate(wingman, WingmateSlot(1)).unwrap();
    let mut damage = damage_for(wingman);
    let mut fire = FireResolver::new(SESSION, Tick(0));
    let mut bindings = ScriptActorBindings::new(session_id(SESSION));
    bindings.bind(wingman, WINGMAN_SCRIPT, None).unwrap();

    damage
        .record_lifecycle(wingman, LifecycleKind::PilotBailout, Tick(2))
        .unwrap();
    let outcome = apply_roster_lifecycle(&mut roster, &damage, &mut fire);
    let batch = bindings.translate(&outcome.log);
    assert_eq!(
        batch.lifecycles,
        [(WINGMAN_SCRIPT, LifecycleKind::PilotBailout)]
    );
    let mut mission = mission(ActorState::Dead);
    let tick = mission.advance_observed(&batch.input(), Tick(2)).unwrap();
    assert!(tick.mission.host.rewards_granted.is_empty());

    // A second translate admits nobody twice.
    assert!(
        bindings
            .translate(&Default::default())
            .registered
            .is_empty()
    );
    assert_eq!(bindings.script_actor(&stranger), None);
}

#[test]
fn accept_t575_bindings_refuse_foreign_and_duplicate_pairs() {
    let mut bindings = ScriptActorBindings::new(session_id(SESSION));
    let a = actor(SESSION, 1);
    bindings.bind(a, ScriptActor(1), None).unwrap();
    assert!(matches!(
        bindings.bind(actor(SESSION + 1, 2), ScriptActor(2), None),
        Err(ScriptBindingError::ForeignSession { .. })
    ));
    assert_eq!(
        bindings.bind(a, ScriptActor(2), None),
        Err(ScriptBindingError::DuplicateSessionActor { actor: a })
    );
    assert_eq!(
        bindings.bind(actor(SESSION, 2), ScriptActor(1), None),
        Err(ScriptBindingError::DuplicateScriptActor {
            script: ScriptActor(1)
        })
    );
    assert_eq!(bindings.script_actor(&actor(SESSION, 2)), None);
}

/// Retail measurement: which control-record keys name placed actors, and with
/// what argument shapes. Run with `--include-ignored` and `CS_GAME_DIR`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t575_retail_actor_references_are_text_names_not_numeric_ids() {
    let dir = std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR: retail measurement needs it");
    let census = survey_mission_control_programs(std::path::Path::new(&dir)).unwrap();
    let mut shapes: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for row in census.measured_rows() {
        let Some(record) = row.record() else { continue };
        for key in record.keys() {
            let family =
                if key.key.strip_prefix("INACTIVE").is_some_and(|rest| {
                    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
                }) {
                    "INACTIVE<n>".to_owned()
                } else {
                    key.key.clone()
                };
            for (shape, sites) in &key.shapes {
                *shapes
                    .entry(family.clone())
                    .or_default()
                    .entry(shape.label())
                    .or_default() += sites;
            }
        }
    }
    println!("install_sha256={}", census.install_sha256());
    // The target-naming families carry text only: no integer anywhere.
    for family in [
        "ADD_OBJECTIVE_TARGET",
        "REMOVE_OBJECTIVE_TARGET",
        "ADD_OTHER_TARGET",
        "REMOVE_OTHER_TARGET",
        "INACTIVE<n>",
    ] {
        let found = shapes
            .get(family)
            .unwrap_or_else(|| panic!("{family} absent"));
        for (shape, sites) in found {
            println!("{family}: {shape} x{sites}");
            assert!(!shape.contains("int"), "{family} carries an int: {shape}");
        }
    }
    // TRAVELERS names its subject by text at most sites; an int leads at five
    // sites whose meaning is unmeasured and is not read as an actor id.
    let travelers = &shapes["TRAVELERS"];
    let int_led: u32 = travelers
        .iter()
        .filter(|(shape, _)| shape.starts_with("[int,"))
        .map(|(_, sites)| sites)
        .sum();
    println!("TRAVELERS int-led sites={int_led}");
    assert_eq!(int_led, 6);
}
