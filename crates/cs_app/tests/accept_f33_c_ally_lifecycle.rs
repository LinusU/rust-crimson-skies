//! Acceptance scenario F33-C: AI roles, dialogue voices and mission callbacks.
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stage `### F33-C`. Task test prefix: `accept_f33_c_`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Minimum scenario (AC03): **an ally killed during a cutscene cannot later
//! fire from a stale actor.** These tests drive the production path end to
//! end: the declared roster is lowered and opened by
//! [`cs_app::roster::open_roster`], a wingmate is registered under its
//! authored role, the declared cinematic is lowered and begun by
//! [`cs_app::cinematics`], the real [`cs_sim::damage::DamageResolver`] records
//! the authoritative lifecycle, and [`cs_app::roster::apply_roster_lifecycle`]
//! feeds it into the identity record and the weapon firing gate
//! ([`cs_sim::weapons::FireResolver`]). The firing half is observed off the
//! *gate itself*: a later `FireIntent` on the dead actor is refused with
//! `MountDisabled` and consumes no round, so a bridge that only wrote a record
//! fails at the shot.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. Whether the original postponed a death that happened during a
//! cutscene, and which voice lines it played, is unrecovered (F33 "Research
//! boundary"); see
//! `docs/findings/2026-10-03-f33-c-ai-roles-dialogue-voices-and-mission-callbacks.md`.

use std::collections::BTreeMap;

use cs_app::cinematics::{MediaAvailability, begin, lower_cinematic};
use cs_app::damage::{lower_graph, lower_policy};
use cs_app::roster::{
    AllyConsumerEvent, AllyConsumerRefusal, DialogueCue, apply_ally_lifecycle,
    apply_roster_lifecycle, lower_roster, open_roster,
};
use cs_app::weapons::lower_gun;
use cs_content::cinematics::declared_synthetic_cinematic;
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_content::pilots::declared_synthetic_roster;
use cs_content::weapons::declared_synthetic_gun;
use cs_sim::allies::{
    AlliesRoster, AllyEvent, AllyEventKind, AllyRole, AllyStatus, BriefingPlan,
    SurvivabilityPolicy, WingmateSlot, synthetic_ally_roster, synthetic_faction,
};
use cs_sim::cinematic_state::{PausePolicy, SYNTHETIC_DURATION_TICKS};
use cs_sim::damage::{ActorId, DamageNodeKey, DamageResolver, LifecycleKind, SYNTHETIC_MOUNT_NODE};
use cs_sim::weapons::{
    FireDenialReason, FireIntent, FireIntentId, FireResolver, GunBank, MountTransform,
    SYNTHETIC_STARTING_ROUNDS, WeaponState,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 53;
const PRODUCER: u32 = 1;

// ----------------------------------------------------------------- helpers ---

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(session_value: u64, serial: u64) -> ActorId {
    ActorId {
        session: session(session_value),
        serial,
    }
}

fn voice(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Voice, key).expect("test voice id is valid")
}

fn mount() -> DamageNodeKey {
    DamageNodeKey::new(SYNTHETIC_MOUNT_NODE).expect("the fixture mount key is valid")
}

/// A session roster opened from the authored fixture with the player faction
/// committed and the wingmate assignments derived, but no actors yet.
fn opened(session_value: u64) -> AlliesRoster {
    let lowered = lower_roster(&declared_synthetic_roster()).expect("the fixture roster lowers");
    open_roster(session_value, &lowered, &BriefingPlan::new()).expect("the authored wings assign")
}

/// Registers one actor with the production lowered damage graph and policy.
fn register_damage_actor(resolver: &mut DamageResolver, actor: ActorId) {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    resolver
        .register_actor(actor, graph, policy)
        .expect("the actor registers");
}

/// A firing gate armed with the fixture gun through the production
/// declared → lowered boundary.
fn armed(actor: ActorId) -> FireResolver {
    let gun = lower_gun(&declared_synthetic_gun()).expect("the fixture gun lowers");
    let mount = gun.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount]).expect("a valid bank"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid weapon state");
    let mut resolver = FireResolver::new(SESSION, Tick(0));
    resolver
        .register(actor, vec![gun], state)
        .expect("the fixture gun registers");
    resolver
}

/// The mount pose a real hierarchy walk would supply, so an enabled mount can
/// actually fire.
fn transforms() -> BTreeMap<DamageNodeKey, MountTransform> {
    let transform = MountTransform::try_new(
        WorldPosition::try_new([0.0, 0.0, 10.0]).expect("a finite muzzle"),
        UnitVec3::FORWARD,
        [0.0; 3],
    )
    .expect("a valid mount transform");
    BTreeMap::from([(mount(), transform)])
}

fn intent(shooter: ActorId, sequence: u32) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session: SESSION,
            tick: Tick(0),
            producer: 1,
            sequence,
        },
        shooter,
    }
}

fn callback(outcome: &cs_app::roster::AllyConsumerOutcome) -> Option<&AllyEvent> {
    outcome.log.events().iter().find_map(|event| match event {
        AllyConsumerEvent::Callback(event) => Some(event),
        _ => None,
    })
}

fn dialogue(outcome: &cs_app::roster::AllyConsumerOutcome) -> Option<&DialogueCue> {
    outcome.log.events().iter().find_map(|event| match event {
        AllyConsumerEvent::Dialogue(cue) => Some(cue),
        _ => None,
    })
}

// -------------------------------------------------- the minimum scenario ---

/// AC03: a wingmate killed while a cutscene has the simulation paused cannot
/// later fire. The authoritative death is reconciled after the scene ends, the
/// mission callback and the authored-voice dialogue are produced, the guns are
/// taken down, a later shot is refused and a converged second pass is a no-op.
#[test]
fn accept_f33_c_an_ally_killed_during_a_cutscene_cannot_later_fire() {
    let wingman = actor(SESSION, 2);
    let mut roster = opened(SESSION);
    roster
        .register_wingmate(wingman, WingmateSlot(1))
        .expect("slot 1 is assigned and the faction is set");
    assert_eq!(
        roster.role_of(&wingman),
        Some(AllyRole::Wingmate(WingmateSlot(1))),
        "the wingmate carries its authored AI role"
    );

    let mut damage = DamageResolver::new(session(SESSION), PRODUCER);
    register_damage_actor(&mut damage, wingman);
    let mut fire = armed(wingman);

    // The cutscene the ally is killed during declares the simulation paused.
    let plan = lower_cinematic(&declared_synthetic_cinematic()).expect("the scene lowers");
    assert_eq!(plan.script.pause(), PausePolicy::SimulationPaused);
    let mut scene = begin(&plan, &MediaAvailability::Present, wingman).expect("the scene starts");

    // While the simulation is paused the authoritative lifecycle records the
    // death; no consumer has run yet, so the stale actor still looks live.
    damage
        .record_lifecycle(wingman, LifecycleKind::Destroyed, Tick(0))
        .expect("the wingmate is registered with the resolver");
    assert_eq!(roster.status(&wingman), Some(AllyStatus::Active));
    assert!(roster.may_fire(&wingman));
    assert!(!fire.state(&wingman).expect("armed").is_disabled(&mount()));

    // The scene reaches its end and the host reconciles the paused death.
    scene
        .advance(SYNTHETIC_DURATION_TICKS)
        .expect("the scene advances");
    assert!(scene.semantic_end_reached());

    let outcome = apply_roster_lifecycle(&mut roster, &damage, &mut fire);
    assert_eq!(
        outcome.report.recorded, 1,
        "exactly one transition was recorded"
    );
    assert_eq!(outcome.report.mounts_disabled, 1);
    assert_eq!(outcome.report.cues, 1);
    assert_eq!(outcome.report.refused, 0);

    // The mission callback is a lost wingmate, not a generic kill, and it
    // carries the pilot's authored voice.
    let callback = callback(&outcome).expect("a mission callback was produced");
    assert_eq!(callback.kind, AllyEventKind::WingmateLost);
    assert_eq!(callback.actor, wingman);
    assert_eq!(callback.faction, synthetic_faction("synthetic.nathan"));
    assert_eq!(callback.survivability, SurvivabilityPolicy::Mortal);
    assert_eq!(callback.voice, Some(voice("synthetic.betty")));

    // The dialogue resolves through the authored voice catalog id.
    assert_eq!(
        dialogue(&outcome),
        Some(&DialogueCue {
            actor: wingman,
            kind: AllyEventKind::WingmateLost,
            voice: voice("synthetic.betty"),
        }),
        "the voice is the authored one, never a random substitute"
    );

    // The stale actor is closed and its guns are down.
    assert_eq!(roster.status(&wingman), Some(AllyStatus::Destroyed));
    assert!(!roster.may_fire(&wingman));
    assert!(fire.state(&wingman).expect("armed").is_disabled(&mount()));

    // A later shot from the dead actor is refused and consumes no round.
    let before = fire.state(&wingman).expect("armed").ammunition(&mount());
    let resolution = fire
        .resolve(&intent(wingman, 0), &transforms())
        .expect("the intent resolves");
    assert_eq!(
        resolution.refused,
        vec![FireDenialReason::MountDisabled { mount: mount() }],
        "a dead ally cannot fire from a stale actor"
    );
    assert!(resolution.accepted.is_empty());
    assert_eq!(
        fire.state(&wingman).expect("armed").ammunition(&mount()),
        before,
        "a refused shot consumes no round"
    );

    // A converged second pass changes nothing and reports nothing.
    let again = apply_roster_lifecycle(&mut roster, &damage, &mut fire);
    assert!(
        again.report.is_noop() && again.log.is_empty(),
        "the pass is convergent: {again:?}"
    );
}

/// A capture and a bailout are recorded as their own mission callbacks but do
/// not ground the ally: only destruction, despawn and mission removal do.
#[test]
fn accept_f33_c_a_capture_or_bailout_does_not_ground_the_ally() {
    let trader = actor(SESSION, 4);
    let raider = actor(SESSION, 9);
    let mut roster = opened(SESSION);
    let records = synthetic_ally_roster(SESSION);
    roster
        .register_with_role(
            records
                .iter()
                .find(|record| record.actor.serial == 4)
                .expect("the fixture has a protected neutral")
                .clone(),
            AllyRole::Neutral(1),
        )
        .expect("the protected neutral registers");
    roster
        .register_with_role(
            records
                .iter()
                .find(|record| record.actor.serial == 9)
                .expect("the fixture has a raider")
                .clone(),
            AllyRole::Unassigned,
        )
        .expect("the raider registers");

    let mut damage = DamageResolver::new(session(SESSION), PRODUCER);
    register_damage_actor(&mut damage, trader);
    register_damage_actor(&mut damage, raider);
    let mut fire = FireResolver::new(SESSION, Tick(0));

    // A capture is a mission callback all its own, and the neutral still acts.
    damage
        .record_lifecycle(trader, LifecycleKind::OwnershipCaptured, Tick(0))
        .expect("the neutral is registered");
    let capture = apply_ally_lifecycle(&mut roster, trader, &damage, &mut fire);
    assert_eq!(
        callback(&capture).map(|event| event.kind),
        Some(AllyEventKind::Captured)
    );
    assert_eq!(roster.status(&trader), Some(AllyStatus::Captured));
    assert!(roster.may_fire(&trader));
    assert_eq!(capture.report.mounts_disabled, 0);

    // A bailout leaves the airframe a physical object that may still act.
    damage
        .record_lifecycle(raider, LifecycleKind::PilotBailout, Tick(0))
        .expect("the raider is registered");
    let bailout = apply_ally_lifecycle(&mut roster, raider, &damage, &mut fire);
    assert_eq!(
        callback(&bailout).map(|event| event.kind),
        Some(AllyEventKind::BailedOut)
    );
    assert_eq!(roster.status(&raider), Some(AllyStatus::BailedOut));
    assert!(roster.may_fire(&raider));
    assert_eq!(bailout.report.mounts_disabled, 0);

    // A protected neutral that is later destroyed is a protected-neutral loss,
    // not a kill, and only then is it grounded.
    damage
        .record_lifecycle(trader, LifecycleKind::Destroyed, Tick(0))
        .expect("the neutral is registered");
    let lost = apply_ally_lifecycle(&mut roster, trader, &damage, &mut fire);
    assert_eq!(
        callback(&lost).map(|event| event.kind),
        Some(AllyEventKind::ProtectedNeutralLost)
    );
    assert!(!roster.may_fire(&trader));
}

/// A foreign-session or unregistered actor is refused by name and changes no
/// consumer.
#[test]
fn accept_f33_c_foreign_and_unknown_actors_are_refused() {
    let mut roster = opened(SESSION);
    let damage = DamageResolver::new(session(SESSION), PRODUCER);
    let mut fire = FireResolver::new(SESSION, Tick(0));

    let foreign = actor(SESSION + 1, 2);
    let outcome = apply_ally_lifecycle(&mut roster, foreign, &damage, &mut fire);
    assert_eq!(outcome.report.refused, 1);
    assert_eq!(
        outcome.log.last(),
        Some(&AllyConsumerEvent::Refused(
            AllyConsumerRefusal::ForeignSession {
                expected: SESSION,
                found: SESSION + 1,
            }
        )),
        "a generation-qualified actor is never reconciled into another generation"
    );

    let unknown = actor(SESSION, 99);
    let outcome = apply_ally_lifecycle(&mut roster, unknown, &damage, &mut fire);
    assert_eq!(outcome.report.refused, 1);
    assert_eq!(
        outcome.log.last(),
        Some(&AllyConsumerEvent::Refused(
            AllyConsumerRefusal::UnknownActor { actor: unknown }
        ))
    );
}

/// Teardown/retry: a retry is a new session generation whose wingmate is alive
/// and armed, because the failed world's death is not carried across.
#[test]
fn accept_f33_c_a_retry_is_a_new_generation_with_no_carried_death() {
    // First attempt: the wingmate is killed and the pass grounds it.
    let first_wingman = actor(SESSION, 2);
    let mut first = opened(SESSION);
    first
        .register_wingmate(first_wingman, WingmateSlot(1))
        .expect("slot 1 is assigned");
    let mut first_damage = DamageResolver::new(session(SESSION), PRODUCER);
    register_damage_actor(&mut first_damage, first_wingman);
    let mut first_fire = armed(first_wingman);
    first_damage
        .record_lifecycle(first_wingman, LifecycleKind::Destroyed, Tick(0))
        .expect("the wingmate is registered");
    let first_outcome = apply_roster_lifecycle(&mut first, &first_damage, &mut first_fire);
    assert_eq!(first_outcome.report.recorded, 1);
    assert!(!first.may_fire(&first_wingman));

    // The retry: a new generation opened the same way, still alive.
    let retry_wingman = actor(SESSION + 1, 2);
    let mut retry = opened(SESSION + 1);
    retry
        .register_wingmate(retry_wingman, WingmateSlot(1))
        .expect("the retry slot is assigned");
    let mut retry_damage = DamageResolver::new(session(SESSION + 1), PRODUCER);
    register_damage_actor(&mut retry_damage, retry_wingman);
    let mut retry_fire = armed(retry_wingman);

    let outcome = apply_roster_lifecycle(&mut retry, &retry_damage, &mut retry_fire);
    assert!(
        outcome.report.is_noop() && outcome.log.is_empty(),
        "the retry carries no death: {outcome:?}"
    );
    assert_eq!(retry.status(&retry_wingman), Some(AllyStatus::Active));
    assert!(retry.may_fire(&retry_wingman));
    assert!(
        !retry_fire
            .state(&retry_wingman)
            .expect("armed")
            .is_disabled(&mount())
    );

    // The retry's guns really fire, so nothing stale was carried over.
    let resolution = retry_fire
        .resolve(&intent(retry_wingman, 0), &transforms())
        .expect("the intent resolves");
    assert!(
        resolution.refused.is_empty(),
        "the retry wingmate fires: {:?}",
        resolution.refused
    );
    assert_eq!(
        retry_fire
            .state(&retry_wingman)
            .expect("armed")
            .ammunition(&mount()),
        SYNTHETIC_STARTING_ROUNDS - 1
    );
}
