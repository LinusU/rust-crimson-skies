//! Task T452-REG-TANGLER-LEDGER (#2196): the measured tangler field ledger and
//! the tangle status timer, through the production ordnance session. Task test
//! prefix: `accept_t452_tangler_`.
//!
//! `docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md` §1
//! (`TANGLER`) measured statically, in the owner-supplied decrypted engine
//! image, that a `TANGLER` record builds an area field at fire time
//! (`0x4b94e0`) whose tick (`0x4b9590`) subtracts the frame time from its
//! `TIME` and, while that stays positive, re-applies the weapon's per-hit
//! routine to every live actor inside `RADIUS²` of the field position
//! (`0x4b95db`–`0x4b961a`), the owner included; the victim's tangle is a timed
//! status that a re-hit raises but never shortens. The pure vocabulary is
//! `cs_content`'s (task #2185); these tests drive the ledger that applies it
//! through [`OrdnanceSession`] and [`step_ordnance_session`].
//!
//! Every value here is synthetic fixture data shaped like `wep_12` (a
//! `TANGLER` status with `TIME 2`, no `RADIUS`), never original game data. No
//! `CS_GAME_DIR` access, and no test awards `verified_original`.

use bevy::prelude::World;
use cs_app::ordnance::{
    OrdnanceEventKind, OrdnanceOrder, OrdnanceSession, OrdnanceSessionTick, OrdnanceStep,
    OrdnanceStepRefusal, T452_REG_TANGLER_FIELD_ORIGIN_CLAIM, TanglerFieldId, TanglerFieldOrder,
    TanglerFieldRefusal, TanglerLedger, step_ordnance_session,
};
use cs_app::scene::SceneGeneration;
use cs_content::ordnance::{
    DeclaredOrdnanceFamily, OriginalAreaDeclaration, OriginalClusterShape, OriginalComponentLaunch,
    OriginalDamageDeclaration, OriginalLifetime, OriginalMountDeclaration,
    OriginalStatusEffectDeclaration, T452_REG_TANGLER_FORCE_SCALAR, TANGLE_FLOOR_S,
    TANGLE_STATUS_NAME, TanglerFieldDeclaration, lower_tangler_field,
};
use cs_sim::damage::{ActorId, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{OrdnanceRegistry, TargetPath};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

const SESSION: u64 = 196;
const PRODUCER: u32 = 197;
const DAMAGE_PRODUCER: u32 = 198;
const GENERATION: SceneGeneration = SceneGeneration(1);
const DT_S: f64 = 1.0 / 30.0;
const TICKS_PER_SECOND: u32 = 30;

// ------------------------------------------------------------------ fixtures --

/// The measured shape of `wep_12`'s record as task #2185 declared it: one
/// `TANGLER` status with the given `TIME` and no `RADIUS`.
fn row(time_s: f64) -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_12".to_owned(),
        family: DeclaredOrdnanceFamily::AreaDenialEngine,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: None,
        fire_rate: None,
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration::default(),
        damage: OriginalDamageDeclaration::default(),
        statuses: vec![OriginalStatusEffectDeclaration {
            key: "TANGLER",
            duration_s: Some(time_s),
            duration_unit: Some("TIME".to_owned()),
        }],
        cluster: OriginalClusterShape::Undeclared,
        unknowns: Vec::new(),
    }
}

/// The declaration through the production lowering of the measured record.
fn declaration(time_s: f64) -> TanglerFieldDeclaration {
    lower_tangler_field(&row(time_s), None).expect("a TANGLER record lowers to its field")
}

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the fixture session is nonzero")
}

fn other_session() -> SessionId {
    SessionId::new(SESSION + 1).expect("the second fixture session is nonzero")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn position(at: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(at).expect("the fixture positions are finite")
}

fn path(target: ActorId, at: [f64; 3]) -> TargetPath {
    TargetPath::try_new(target, at, at).expect("the fixture path is finite")
}

fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("the fixture rate is nonzero")
}

fn field_id(serial: u64) -> TanglerFieldId {
    TanglerFieldId {
        session: SESSION,
        serial,
    }
}

/// A session with an empty catalogue: no component is registered at all, so a
/// field that opens here is driven by its declaration and nothing else.
fn session() -> OrdnanceSession {
    OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, OrdnanceRegistry::new())
        .expect("a nonzero session opens")
}

fn damage() -> DamageResolver {
    DamageResolver::new(session_id(), DAMAGE_PRODUCER)
}

fn field_order(field: &TanglerFieldDeclaration, owner: ActorId, centre: [f64; 3]) -> OrdnanceOrder {
    OrdnanceOrder::Tangler(TanglerFieldOrder {
        declaration: field.clone(),
        owner,
        centre: position(centre),
    })
}

fn step(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    targets: &[TargetPath],
    at: u64,
) -> OrdnanceSessionTick {
    step_ordnance_session(
        world,
        session,
        damage,
        orders,
        &OrdnanceStep {
            at: Tick(at),
            dt_s: DT_S,
            wind_velocity_m_s: [0.0; 3],
            generation: GENERATION,
            guidance: None,
            targets,
            impacts: &[],
            blast_recipients: None,
            victim_nodes: None,
        },
    )
}

// ------------------------------------------------------------------- tests ---

/// A field opened at fire time lives exactly its `TIME` in whole 30 Hz ticks
/// and re-applies its tangle on every one of them.
#[test]
fn accept_t452_tangler_a_field_lasts_its_time_in_ticks_and_reapplies_each_tick() {
    let field = declaration(2.0);
    assert_eq!(field.lifetime_s, 2.0);
    assert_eq!(field.radius_m, 10.0);
    let mut session = session();
    let mut world = World::new();
    let mut damage = damage();
    let target = actor(2);
    let targets = [path(target, [0.0, 0.0, 0.0])];

    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[field_order(&field, actor(1), [0.0, 0.0, 0.0])],
        &targets,
        0,
    );
    assert_eq!(
        first.tangler_opened,
        vec![field_id(0)],
        "the order opens the field on its own tick"
    );
    assert_eq!(first.tangler.len(), 1);
    assert_eq!(first.tangler[0].field, field_id(0));
    assert_eq!(
        first.tangler[0].applied.len(),
        1,
        "the victim at the centre is inside"
    );
    // d2 = 0 is the measured duration formula's maximum: 10 s at 30 Hz.
    assert_eq!(first.tangler[0].applied[0].duration_ticks, 300);
    assert_eq!(first.tangler[0].applied[0].remaining_ticks, 300);
    assert_eq!(first.tangles_started.len(), 1, "the tangle begins");
    assert_eq!(first.tangles_started[0].target, target);
    assert_eq!(first.tangles_started[0].field, field_id(0));
    assert!(
        session.events().iter().any(|event| matches!(
            event.kind,
            OrdnanceEventKind::TangleStarted { target: t, field, .. }
                if t == target && field == field_id(0)
        )),
        "the session announces the start once"
    );
    let opened = session.tangler_fields();
    assert_eq!(opened.len(), 1);
    assert_eq!(
        opened[0].lifetime_ticks, 60,
        "TIME 2 s at 30 Hz is 60 ticks"
    );
    assert_eq!(opened[0].expires_at, Tick(60));

    let mut applications = 1;
    for at in 1..=59 {
        let tick = step(&mut world, &mut session, &mut damage, &[], &targets, at);
        assert!(
            tick.tangler_expired.is_empty(),
            "tick {at}: the field is still live"
        );
        assert_eq!(tick.tangler.len(), 1, "tick {at}: the field is open");
        assert_eq!(
            tick.tangler[0].applied.len(),
            1,
            "tick {at}: the field re-applies"
        );
        assert!(
            tick.tangles_started.is_empty(),
            "tick {at}: a re-hit is the same continuous status, not a new episode"
        );
        applications += 1;
    }
    assert_eq!(applications, 60, "one application per live tick");

    // The boundary tick: the field is gone and reported once, and no longer
    // applies.
    let boundary = step(&mut world, &mut session, &mut damage, &[], &targets, 60);
    assert!(
        boundary.tangler.is_empty(),
        "the field applies no longer on its boundary tick"
    );
    assert_eq!(boundary.tangler_expired.len(), 1);
    assert_eq!(boundary.tangler_expired[0].instance, field_id(0));
    assert_eq!(boundary.tangler_expired[0].expired_at, Tick(60));
    assert_eq!(boundary.tangler_expired[0].record, "wep_12");
    assert!(session.tangler_fields().is_empty());
    // The tangle outlives the field by its own measured duration: it was last
    // raised on tick 59 to 300 ticks.
    assert!(session.is_tangled(&target));
    assert_eq!(
        session.tangle_status(&target).expect("tangled").expires_at,
        Tick(359)
    );
}

/// The radius is a hard edge: an actor strictly outside it is never tangled,
/// and one exactly at it is not either, because the measured tick compares the
/// squared distance with `RADIUS²`.
#[test]
fn accept_t452_tangler_an_actor_outside_the_radius_is_untouched() {
    let field = declaration(2.0);
    let mut session = session();
    let mut world = World::new();
    let mut damage = damage();
    let inside = actor(2);
    let at_the_edge = actor(3);
    let outside = actor(4);
    let targets = [
        path(inside, [0.0, 0.0, 9.9]),
        path(at_the_edge, [0.0, 0.0, 10.0]),
        path(outside, [0.0, 0.0, 10.1]),
    ];
    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[field_order(&field, actor(1), [0.0, 0.0, 0.0])],
        &targets,
        0,
    );
    let applied: Vec<ActorId> = first.tangler[0]
        .applied
        .iter()
        .map(|application| application.target)
        .collect();
    assert_eq!(applied, vec![inside], "only the actor inside is tangled");
    assert!(session.is_tangled(&inside));
    assert!(
        !session.is_tangled(&at_the_edge),
        "10 m exactly is at the edge, not inside"
    );
    assert!(!session.is_tangled(&outside));

    // Ten more ticks: the outside actors stay untouched for the whole field.
    for at in 1..=10 {
        let tick = step(&mut world, &mut session, &mut damage, &[], &targets, at);
        assert_eq!(
            tick.tangler[0].applied.len(),
            1,
            "tick {at}: still one victim"
        );
        assert_eq!(tick.tangler[0].applied[0].target, inside);
    }
    assert!(!session.is_tangled(&at_the_edge));
    assert!(!session.is_tangled(&outside));
}

/// The owner is hit like every other actor: the measured tick excludes nobody.
///
/// The session holds no actor list (the same reason
/// [`OrdnanceRuntime::open_area`](cs_sim::weapons::OrdnanceRuntime::open_area)
/// takes one), so the owner is supplied in the tick's target list like any
/// other actor. What this pins is that the pass does **not** skip the field's
/// owner: an owner the caller lists is tangled exactly like a stranger.
#[test]
fn accept_t452_tangler_the_owner_inside_the_field_is_tangled() {
    let field = declaration(2.0);
    let mut session = session();
    let mut world = World::new();
    let mut damage = damage();
    let owner = actor(1);
    let far_away = actor(2);
    let targets = [
        path(owner, [0.0, 0.0, 3.0]),
        path(far_away, [0.0, 0.0, 50.0]),
    ];
    let tick = step(
        &mut world,
        &mut session,
        &mut damage,
        &[field_order(&field, owner, [0.0, 0.0, 0.0])],
        &targets,
        0,
    );
    let applied: Vec<ActorId> = tick.tangler[0]
        .applied
        .iter()
        .map(|application| application.target)
        .collect();
    assert_eq!(applied, vec![owner], "the owner is inside and is tangled");
    assert!(session.is_tangled(&owner));
    assert_eq!(
        session.tangle_status(&owner).expect("tangled").target,
        owner
    );
    assert_eq!(
        session.tangler_fields()[0].owner,
        owner,
        "the field names its owner"
    );
    assert!(!session.is_tangled(&far_away));
}

/// A re-hit raises the timer but never shortens it, and the timer counts down
/// and clears on its own boundary tick.
#[test]
fn accept_t452_tangler_the_status_timer_never_shortens_and_expires() {
    // Field A: TIME 0.5 s, so it is gone after 15 ticks.
    let near_field = declaration(0.5);
    // Field B: TIME 2 s, opened after A has gone.
    let far_field = declaration(2.0);
    let mut session = session();
    let mut world = World::new();
    let mut damage = damage();
    let target = actor(2);
    // d2 = 4: 10 * (1 - 4/10) = 6 s = 180 ticks.
    let near = [path(target, [0.0, 0.0, 2.0])];
    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[field_order(&near_field, actor(1), [0.0, 0.0, 0.0])],
        &near,
        0,
    );
    assert_eq!(first.tangler[0].applied[0].duration_ticks, 180);
    assert_eq!(first.tangler[0].applied[0].remaining_ticks, 180);
    for at in 1..=14 {
        step(&mut world, &mut session, &mut damage, &[], &near, at);
    }
    let before = session.tangle_status(&target).expect("still tangled");
    assert_eq!(
        before.expires_at,
        Tick(194),
        "the last near hit was on tick 14"
    );
    assert_eq!(before.remaining_ticks, 180);

    // Field A has expired on tick 15; a far hit would give only 60 ticks
    // (d2 = 9 → the 2 s floor), so the timer must keep its 179 remaining ticks.
    let far = [path(target, [0.0, 0.0, 3.0])];
    let rehit = step(
        &mut world,
        &mut session,
        &mut damage,
        &[field_order(&far_field, actor(1), [0.0, 0.0, 0.0])],
        &far,
        15,
    );
    assert_eq!(
        rehit
            .tangler_expired
            .iter()
            .map(|expired| expired.instance)
            .collect::<Vec<_>>(),
        vec![field_id(0)],
        "the short field expired first"
    );
    assert_eq!(rehit.tangler_opened, vec![field_id(1)]);
    let application = rehit.tangler[0].applied[0];
    assert_eq!(application.duration_ticks, 60, "the far hit's own duration");
    assert_eq!(
        application.remaining_ticks, 179,
        "the shorter re-hit did not shorten the timer"
    );
    assert_eq!(application.expires_at, Tick(194));
    let after = session.tangle_status(&target).expect("still tangled");
    assert_eq!(after.remaining_ticks, 179);
    assert_eq!(
        after.duration_ticks, 180,
        "the ceiling stays the longest hit"
    );

    // Count down: no target is supplied any more, so nothing re-applies.
    for at in 16..=193 {
        let tick = step(&mut world, &mut session, &mut damage, &[], &[], at);
        assert!(
            tick.tangles_expired.is_empty(),
            "tick {at}: the timer is still live"
        );
    }
    let last = session
        .tangle_status(&target)
        .expect("live up to its boundary");
    assert_eq!(last.remaining_ticks, 1);
    let boundary = step(&mut world, &mut session, &mut damage, &[], &[], 194);
    assert_eq!(boundary.tangles_expired.len(), 1);
    assert_eq!(boundary.tangles_expired[0].target, target);
    assert_eq!(boundary.tangles_expired[0].expired_at, Tick(194));
    assert_eq!(boundary.tangles_expired[0].duration_ticks, 180);
    assert!(
        session.events().iter().any(|event| matches!(
            event.kind,
            OrdnanceEventKind::TangleExpired { target: t, expired_at, .. }
                if t == target && expired_at == Tick(194)
        )),
        "the session announces the clear once"
    );
    assert!(
        !session.is_tangled(&target),
        "the tangle clears on its boundary tick"
    );
    assert!(session.tangled().is_empty());
}

/// The field is driven by the declared `TANGLER` record alone: no component is
/// registered, and the named claims are the measured ones.
#[test]
fn accept_t452_tangler_the_field_opens_from_the_declaration_with_no_registered_component() {
    let field = declaration(2.0);
    let mut session = session();
    assert!(
        session.registered_shooters().is_empty(),
        "the fixture registers no component"
    );
    let mut world = World::new();
    let mut damage = damage();
    let target = actor(2);
    let tick = step(
        &mut world,
        &mut session,
        &mut damage,
        &[field_order(&field, actor(1), [1.0, 2.0, 3.0])],
        &[path(target, [1.0, 2.0, 3.0])],
        0,
    );
    assert_eq!(tick.tangler_opened.len(), 1);
    assert!(
        session.registered_shooters().is_empty(),
        "opening the field registered nothing"
    );
    let opened = &session.tangler_fields()[0];
    assert_eq!(opened.record, "wep_12");
    assert_eq!(opened.centre, position([1.0, 2.0, 3.0]));
    assert_eq!(opened.radius_m, 10.0);
    assert_eq!(opened.status.name, TANGLE_STATUS_NAME);
    assert_eq!(opened.status.claim, T452_REG_TANGLER_FORCE_SCALAR);
    assert_eq!(opened.status.floor_s, TANGLE_FLOOR_S);
    assert_eq!(
        T452_REG_TANGLER_FIELD_ORIGIN_CLAIM, "t452_reg.limit.tangler_field_origin",
        "where the fire path centres the field stays a named claim"
    );
}

/// Teardown releases the fields and the timers with the rest of the session.
#[test]
fn accept_t452_tangler_teardown_releases_the_fields_and_tangles() {
    let field = declaration(2.0);
    let mut session = session();
    let mut world = World::new();
    let mut damage = damage();
    let target = actor(2);
    let targets = [path(target, [0.0, 0.0, 1.0])];
    step(
        &mut world,
        &mut session,
        &mut damage,
        &[field_order(&field, actor(1), [0.0, 0.0, 0.0])],
        &targets,
        0,
    );
    assert!(session.is_tangled(&target));

    let report = session.close(&mut world);
    assert!(report.projectiles.is_empty());
    assert!(
        session.tangler_fields().is_empty(),
        "the fields are released"
    );
    assert!(session.tangled().is_empty(), "the timers are released");
    assert!(!session.is_tangled(&target));

    let after = step(&mut world, &mut session, &mut damage, &[], &targets, 1);
    assert_eq!(after.refused, Some(OrdnanceStepRefusal::Closed));
    assert!(after.tangler.is_empty());
}

/// The ledger refuses another generation, a tick that goes backwards, a
/// declaration with no lifetime and a foreign victim, rather than opening or
/// applying anything.
#[test]
fn accept_t452_tangler_the_ledger_refuses_what_it_cannot_express() {
    let field = declaration(2.0);
    let mut ledger = TanglerLedger::new(session_id(), Tick(5), rate());
    let order = TanglerFieldOrder {
        declaration: field.clone(),
        owner: actor(1),
        centre: position([0.0; 3]),
    };
    assert_eq!(
        ledger.open(other_session(), Tick(5), &order),
        Err(TanglerFieldRefusal::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );
    assert_eq!(
        ledger.open(session_id(), Tick(4), &order),
        Err(TanglerFieldRefusal::NonMonotonicTick {
            at: Tick(5),
            found: Tick(4),
        })
    );
    let mut timeless = field.clone();
    timeless.lifetime_s = 0.0;
    assert_eq!(
        ledger.open(
            session_id(),
            Tick(5),
            &TanglerFieldOrder {
                declaration: timeless,
                ..order.clone()
            },
        ),
        Err(TanglerFieldRefusal::NonPositiveLifetime { seconds: 0.0 })
    );
    assert!(ledger.is_empty(), "nothing was opened by a refused call");

    // A victim from another generation is named, not tangled.
    let foreign = TargetPath::try_new(
        ActorId {
            session: other_session(),
            serial: 9,
        },
        [0.0; 3],
        [0.0; 3],
    )
    .expect("the fixture path is finite");
    assert_eq!(
        ledger.apply(session_id(), Tick(5), &[foreign]),
        Err(TanglerFieldRefusal::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );
}
