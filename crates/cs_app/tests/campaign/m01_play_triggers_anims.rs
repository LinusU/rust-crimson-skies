//! Acceptance stage M01-PLAY-TRIGGERS-ANIMS (Rally #1482), the integration
//! step that checks the full acceptance of the two halves it was split into:
//! M01-PLAY-TRIGGERS-ANIMS-ARM (#1497, the `WAKE_ANIM` consumer and the
//! carrier-record join) and M01-PLAY-TRIGGERS-ANIMS-LAND (#1498, the
//! `landings.zrd` reader and the player's own landing).
//!
//! The parent's acceptance, re-scoped by the owner on 2026-10-15, is the whole
//! chain end to end: a `WAKE_ANIM` directive runs the record it names, that
//! record's `OBJECT_ACTIVE_STATE` statement arms a landing node, the player
//! body lands inside the measured window at that node, and only then does the
//! gated block — whose condition reads an `ANIM_STATE` row the landing's own
//! call chain published — complete. The gate closes through the player's
//! landing, never through a `WAKE_ANIM` shortcut; no name-to-name mapping
//! between a woken name and a gated name is invented (they are different
//! measured records).
//!
//! Three members:
//!
//! * **The synthetic composed stage (runs in CI)** authors a control program
//!   with a `WAKE_ANIM` site and a second block whose condition is
//!   `Condition::AnimationStates` over the name the landing's chain starts. It
//!   walks the three legs the owner named — armed with no landing, a landing
//!   outside the row's window, and the landing itself — and asserts the gate
//!   stays open on the first two, is still open on the tick the landing runs
//!   its row, and closes only once the chain's own record reaches `EXECUTED`
//!   through the production player, having been observed `RUNNING` on the way.
//! * **The never-firing directive (runs in CI)** is the same synthetic stage
//!   with a `WAKE_ANIM` site whose condition never runs: the node is never
//!   armed, a body that would otherwise land inside the row's window runs
//!   nothing, and the gate never completes.
//! * **Against the owner's installation (ignored without `CS_GAME_DIR` and
//!   `CS_ENGINE_IMAGE`)** the same three legs run over the composed retail M01:
//!   the real program's `WAKE_ANIM` of `OBJECTIVE49` arms `wv_manual_land`, and
//!   `OBJECTIVE15` — whose gate is `wv_pickup_copilot EXECUTED`, a state only a
//!   real landing runs — stays open until the player really lands there and the
//!   row's chain reaches that state.
//!
//! Nothing here is `verified_original`: `retail` is read access to the owner's
//! installation, and no original executable ran. The designed rules the chain
//! runs on (the node-arming surface, the landing geometry, the row window and
//! the call-chain timing) are recorded with their claims in
//! `docs/findings/2026-10-15-m01-play-triggers-anims-wake-anim-record-join.md`,
//! `docs/findings/2026-10-15-m01-play-triggers-anims-arm.md`,
//! `docs/findings/2026-10-16-m01-play-triggers-anims-land.md` and
//! `docs/findings/2026-10-16-m01-play-triggers-anims.md`.

use std::path::Path;

use bevy::prelude::{App, Quat, Vec3};
use cs_app::mission_animations::ActivationOutcome;
use cs_app::mission_session::{
    LandingNodeEntry, LandingRow, MissionHost, MissionLandings, MissionStage, PLAYER_MEMBER,
    build_headless, teardown,
};
use cs_script::ir::{
    Action, AnimationState, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective,
    SymbolId, Value,
};
use cs_script::runtime::ObjectiveLifecycle;
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_types::content::ContentKind;

use crate::common::cid;
use crate::m01_play_triggers_anims_land::{
    Body, SYNTHETIC_ARM, SYNTHETIC_CALLED, SYNTHETIC_LAND, armed_has, composed_node_position,
    last_tick, observed_state, pump, pump_until_armed, retail_land_stage, synthetic_land_stage,
};
use crate::vs_m01_rt_window::Scratch;

/// The synthetic landing node the arm record's statement carries.
const SYNTHETIC_NODE: &str = "synthetic_landing_node";

// ---------------------------------------------------------------------------
// The synthetic gate program and table (authored content, never a reading of
// any original mission's record).
// ---------------------------------------------------------------------------

/// The synthetic control program of this member: block 0 wakes the arm record
/// through the real `WAKE_ANIM` directive when `wake` is set, and block 1 is
/// the gate whose own condition reads the `ANIM_STATE` row the landing's call
/// chain publishes. A `wake` of `false` is the negative leg's program: the
/// same shape with a directive whose block never wakes — a `WAKE_ANIM` site
/// runs on its block's wake transition (the measured role), so a block the
/// lifecycle model never admits to `Awake` is what keeps it from firing.
///
/// The second block mirrors M01's measured gate shape (`ANIM_STATE` with one
/// appended pair) on authored names, so the composed member exercises the one
/// path the retail member's `OBJECTIVE15` uses.
fn synthetic_gate_program(wake: bool) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-mission-anim-parent"),
        variables: Vec::new(),
        objectives: vec![
            Objective {
                id: SymbolId(0),
                content: cid(ContentKind::Objective, "synthetic-parent-wake"),
                condition: Condition::Const(wake),
                actions: vec![Action::Directive {
                    operation: DirectiveOperation::WakeAnimation,
                    args: vec![Value::Str(SYNTHETIC_ARM.to_owned())],
                }],
                span: None,
            },
            Objective {
                id: SymbolId(1),
                content: cid(ContentKind::Objective, "synthetic-parent-gate"),
                condition: Condition::AnimationStates {
                    required: 1,
                    animations: vec![(SYNTHETIC_CALLED.to_owned(), AnimationState::Executed)],
                },
                actions: Vec::new(),
                span: None,
            },
        ],
    }
}

/// The authored landing table: one row at the node the arm record arms, whose
/// node anchors on the player's own observed row so the writer composes its
/// position from the body this member drives.
fn synthetic_parent_landings() -> MissionLandings {
    MissionLandings {
        rows: vec![LandingRow::authored(
            0,
            SYNTHETIC_LAND,
            SYNTHETIC_NODE,
            &[35.0],
            &[50.0, 220.0],
            false,
        )],
        nodes: vec![LandingNodeEntry {
            node: SYNTHETIC_NODE.to_owned(),
            anchor: PLAYER_MEMBER.to_owned(),
            offset: [0.0, 0.0, 0.0],
        }],
        refusals: Vec::new(),
    }
}

/// The synthetic stage: the landing half's own composed stage with this
/// member's gate program ([`synthetic_gate_program`]'s `wake` chooses the
/// positive chain or the never-firing directive), and a declared lifecycle
/// table that keeps both authored blocks awake so each evaluates every tick
/// (an absent table leaves `state` answering `None`, which cannot show a
/// completion). The negative leg keeps block 0 **dormant**: a `WAKE_ANIM`
/// site runs on its block's wake transition (the measured role, not on
/// completion), so a block that never wakes is a directive that never fires.
fn synthetic_parent_stage(scratch: &Path, wake: bool) -> MissionStage {
    let mut stage = synthetic_land_stage(scratch);
    stage.host.control = synthetic_gate_program(wake);
    let mut table = BlockLifecycleTable::new();
    table
        .declare(0, if wake {
            LifecycleDecl::awake()
        } else {
            LifecycleDecl::dormant(None)
        })
        .expect("block 0 is declared once");
    table
        .declare(1, LifecycleDecl::awake())
        .expect("block 1 is declared once");
    stage.host.block_lifecycles = table;
    stage
}

/// One block's own lifecycle row, read from the host's lifecycle model.
fn block_state(app: &App, index: u32) -> Option<ObjectiveLifecycle> {
    app.world()
        .resource::<MissionHost>()
        .lifecycles()
        .state(index)
}

/// The gate block's own lifecycle row — block 1 of the authored synthetic
/// program.
fn gate_state(app: &App) -> Option<ObjectiveLifecycle> {
    block_state(app, 1)
}

// ---------------------------------------------------------------------------
// The whole chain, on the composed synthetic stage (runs in CI).
// ---------------------------------------------------------------------------

/// **The whole chain closes its gate only when the player really lands inside
/// the row's window: the `WAKE_ANIM` directive arms the node, an armed but
/// unlanded run and a landing outside the window keep the gate open, and the
/// gate is still open on the tick the real landing runs its row — it closes
/// only after the row's own call chain reaches `EXECUTED` through the player.**
#[test]
fn accept_m01_play_triggers_anims_the_whole_chain_closes_its_gate_only_on_the_landing() {
    let scratch = Scratch::new("triggers-anims");
    let mut app = build_headless(&synthetic_parent_stage(scratch.path(), true))
        .expect("the synthetic stage composes");
    // The composed table is the fixture installation's absence; this member's
    // authored table replaces it before the first tick.
    app.world_mut().insert_resource(synthetic_parent_landings());

    // The player's node anchors on its own row, so only the velocity and
    // attitude matter: a descent far too fast to be a landing, an in-window
    // descent far too fast for the row's speed bound, and the landing itself.
    let too_fast = Body {
        position: None,
        velocity: Vec3::new(60.0, -30.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    let off_window = Body {
        position: None,
        velocity: Vec3::new(300.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    let landed = Body {
        position: None,
        velocity: Vec3::new(60.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };

    // The real `WAKE_ANIM` directive starts the arm record, whose statement
    // arms the node. The body holds a too-fast descent the whole time, so no
    // row can land while the arm runs.
    pump_until_armed(&mut app, SYNTHETIC_NODE, 600, Some(&too_fast));

    // Leg 1 — armed, no landing: the gate's animation never ran, so the gate
    // stays open.
    pump(&mut app, 30, Some(&too_fast));
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "an armed node the player never reaches runs nothing"
    );
    assert!(
        observed_state(&app, SYNTHETIC_CALLED).is_none(),
        "the gate's animation must not have run without a landing"
    );
    assert_ne!(
        gate_state(&app),
        Some(ObjectiveLifecycle::Done),
        "the gate must stay open while no landing has happened"
    );

    // Leg 2 — a landing outside the row's window: still nothing runs, and the
    // gate stays open.
    pump(&mut app, 30, Some(&off_window));
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "a landing outside the window runs nothing"
    );
    assert!(
        observed_state(&app, SYNTHETIC_CALLED).is_none(),
        "an off-window landing must not run the gate's animation"
    );
    assert_ne!(
        gate_state(&app),
        Some(ObjectiveLifecycle::Done),
        "the gate must stay open for a landing outside the window"
    );

    // Leg 3 — the landing itself. One frame at a time so the gate's state on
    // the very tick the landing runs its row can be asserted.
    let mut landing_tick = None;
    for _ in 0..60 {
        pump(&mut app, 1, Some(&landed));
        let host = app.world().resource::<MissionHost>();
        if let Some(activation) = host.landings().activated().first() {
            assert_eq!(activation.row, 0);
            assert_eq!(activation.node, SYNTHETIC_NODE);
            assert_eq!(activation.anim, SYNTHETIC_LAND);
            assert_eq!(activation.outcome, ActivationOutcome::Started);
            landing_tick = Some(activation.at.0);
            break;
        }
    }
    let landing_tick = landing_tick.expect("the real landing runs the row");

    // On the landing tick the row's record has only just started at stage 6d:
    // its chain has published nothing yet, so the gate is not already closed —
    // the completion below is caused by this landing, not preceded by it.
    assert!(
        observed_state(&app, SYNTHETIC_CALLED).is_none(),
        "the chain's animation has published nothing on the landing tick"
    );
    assert_ne!(
        gate_state(&app),
        Some(ObjectiveLifecycle::Done),
        "the gate must not already be closed on the tick the landing happens"
    );

    // The chain runs through the production player and reaches EXECUTED; only
    // then does the program complete the gate block. The chain's own record is
    // watched through the fold's own rows on the way, so the RUNNING -> EXECUTED
    // order the player reports is asserted and not just the end state.
    let mut running_seen = false;
    let mut closed_tick = None;
    for _ in 0..400 {
        pump(&mut app, 1, Some(&landed));
        if observed_state(&app, SYNTHETIC_CALLED) == Some(AnimationState::Running.code()) {
            running_seen = true;
        }
        if gate_state(&app) == Some(ObjectiveLifecycle::Done) {
            closed_tick = Some(last_tick(&app));
            break;
        }
    }
    let closed_tick = closed_tick.expect("the gate closes after the landing runs the chain");
    assert!(
        running_seen,
        "the chain's animation was observed RUNNING before it was observed EXECUTED"
    );
    assert_eq!(
        observed_state(&app, SYNTHETIC_CALLED),
        Some(AnimationState::Executed.code()),
        "the gate's animation reached EXECUTED through the record player"
    );
    assert!(
        closed_tick > landing_tick,
        "the gate closes after the landing tick (landing {landing_tick}, closed {closed_tick})"
    );
    assert_eq!(
        gate_state(&app),
        Some(ObjectiveLifecycle::Done),
        "the gate is closed once its animation ran"
    );

    teardown(&mut app);
}

/// **A `WAKE_ANIM` directive whose block never wakes leaves the whole chain
/// dead: the landing node is never armed, a body that would otherwise land
/// inside the row's window runs nothing, the gate's animation is never
/// observed and the gate never completes.**
///
/// The positive member proves the chain closes when the event runs its record;
/// this one proves the same closure cannot happen without it, which is the
/// acceptance's "a run where the event never fires never completes it" leg.
/// `WAKE_ANIM` is a wake-role site (the measured `DirectiveRole::WakeEffect`),
/// so "never fires" is the block never waking — its dormant declaration —
/// not merely a condition that never holds: an awake block's wake effects
/// run ahead of whatever its own completion gate later answers.
#[test]
fn accept_m01_play_triggers_anims_a_wake_that_never_fires_never_closes_the_gate() {
    let scratch = Scratch::new("triggers-anims-no-wake");
    let mut app = build_headless(&synthetic_parent_stage(scratch.path(), false))
        .expect("the synthetic stage composes");
    app.world_mut().insert_resource(synthetic_parent_landings());

    // The body holds a state the row's own window fully admits (at the node,
    // inside the speed bound, level), so only the missing arming can keep the
    // row from running.
    let landed = Body {
        position: None,
        velocity: Vec3::new(60.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    pump(&mut app, 240, Some(&landed));

    assert!(
        !armed_has(&app, SYNTHETIC_NODE),
        "no arm statement can run: its WAKE_ANIM directive never fired"
    );
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "a landing at a node nothing armed runs nothing"
    );
    assert!(
        observed_state(&app, SYNTHETIC_CALLED).is_none(),
        "the gate's animation never ran"
    );
    assert_ne!(
        gate_state(&app),
        Some(ObjectiveLifecycle::Done),
        "the gate stays open when the event that arms its node never fires"
    );

    teardown(&mut app);
}

// ---------------------------------------------------------------------------
// The whole chain, on the composed retail M01 (requires original data).
// ---------------------------------------------------------------------------

/// OBJECTIVE15's block index in M01's control program — the gate this member
/// closes (its condition is `wv_pickup_copilot EXECUTED`).
const RETAIL_GATE_BLOCK: u32 = 14;

/// OBJECTIVE49's block index — the block whose own `WAKE_ANIM` arms the
/// `wv_manual_land` node.
const RETAIL_ARM_BLOCK: u32 = 48;

/// **On the composed retail M01 the same three legs hold: `OBJECTIVE49`'s own
/// `WAKE_ANIM` arms `wv_manual_land`, `OBJECTIVE15` stays open while nothing
/// lands there and for a landing outside the row's window, and it is still
/// open on the tick the real landing runs `wv_initiate_hookup` — it closes only
/// after the row's measured chain reaches `wv_pickup_copilot EXECUTED`.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_triggers_anims_retail_the_gate_closes_only_after_the_real_landing() {
    // OBJECTIVE49 (block index 48) is the non-circular arming path; OBJECTIVE15
    // (14) is the gate this landing must close. Both declarations are test-side
    // inputs, exactly as the landing half's own retail member documents.
    let mut app = build_headless(&retail_land_stage(&[RETAIL_ARM_BLOCK, RETAIL_GATE_BLOCK]))
        .expect("M01's stage composes headlessly");
    pump_until_armed(&mut app, "wv_manual_land", 600, None);
    {
        let host = app.world().resource::<MissionHost>();
        let publisher = host
            .armed_nodes()
            .iter()
            .find(|armed| armed.nodes().iter().any(|node| node == "wv_manual_land"))
            .expect("an armed vocabulary carries wv_manual_land");
        assert_eq!(
            publisher.animation(),
            "activate_dropoff_node",
            "the real program's own wake ran the arming record"
        );
    }

    // Leg 1 — armed, no landing: the gate's animation never ran.
    pump(&mut app, 60, None);
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "an armed node the player never reaches runs nothing"
    );
    assert!(
        observed_state(&app, "wv_pickup_copilot").is_none(),
        "OBJECTIVE15's animation must not run without a landing"
    );
    assert_ne!(
        block_state(&app, RETAIL_GATE_BLOCK),
        Some(ObjectiveLifecycle::Done),
        "OBJECTIVE15 must stay open while no landing has happened"
    );

    // The composed landing position of the node — the writer's own row, which
    // anchors on the bound workersvoyagezep.
    let position = composed_node_position(&app, "wv_manual_land");

    // Leg 2 — at the node, in the sink bound, but far outside the row's speed
    // window: nothing runs, and the gate stays open.
    let off_window = Body {
        position: Some(position),
        velocity: Vec3::new(300.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    pump(&mut app, 60, Some(&off_window));
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "a landing outside the window runs nothing"
    );
    assert!(
        observed_state(&app, "wv_pickup_copilot").is_none(),
        "an off-window landing must not run OBJECTIVE15's animation"
    );
    assert_ne!(
        block_state(&app, RETAIL_GATE_BLOCK),
        Some(ObjectiveLifecycle::Done),
        "OBJECTIVE15 must stay open for a landing outside the window"
    );

    // Leg 3 — the landing itself, one frame at a time so its tick can be read.
    let landed = Body {
        position: Some(position),
        velocity: Vec3::new(60.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    let mut landing_tick = None;
    for _ in 0..60 {
        pump(&mut app, 1, Some(&landed));
        let host = app.world().resource::<MissionHost>();
        if let Some(activation) = host.landings().activated().first() {
            assert_eq!(activation.node, "wv_manual_land");
            assert_eq!(activation.anim, "wv_initiate_hookup");
            assert_eq!(activation.outcome, ActivationOutcome::Started);
            landing_tick = Some(activation.at.0);
            break;
        }
    }
    let landing_tick = landing_tick.expect("the real landing runs the row");

    // On the landing tick the row's record has only just started: its chain
    // has published nothing, so the gate is not already closed.
    assert!(
        observed_state(&app, "wv_pickup_copilot").is_none(),
        "the chain's copilot animation has published nothing on the landing tick"
    );
    assert_ne!(
        block_state(&app, RETAIL_GATE_BLOCK),
        Some(ObjectiveLifecycle::Done),
        "OBJECTIVE15 must not already be closed on the tick the landing happens"
    );

    // The chain runs `wv_pickup_copilot` through the production player (its
    // measured 6.0 stored time units, 720 ticks at the player's 120 Hz) until
    // it reaches EXECUTED; the program then completes OBJECTIVE15.
    let mut closed = false;
    for _ in 0..30 {
        pump(&mut app, 60, Some(&landed));
        if block_state(&app, RETAIL_GATE_BLOCK) == Some(ObjectiveLifecycle::Done) {
            closed = true;
            break;
        }
    }
    assert!(
        closed,
        "OBJECTIVE15 closes after the landing runs the chain, observed {:?}",
        observed_state(&app, "wv_pickup_copilot")
    );
    assert_eq!(
        observed_state(&app, "wv_pickup_copilot"),
        Some(AnimationState::Executed.code()),
        "wv_pickup_copilot reached EXECUTED through the record player"
    );
    assert!(
        last_tick(&app) > landing_tick,
        "OBJECTIVE15 closes after the landing tick (landing {landing_tick}, closed {})",
        last_tick(&app)
    );

    let host = app.world().resource::<MissionHost>();
    assert_eq!(
        host.lifecycles().state(RETAIL_ARM_BLOCK),
        Some(ObjectiveLifecycle::Done),
        "OBJECTIVE49 completed — its wake armed the node the landing used"
    );
    assert!(
        host.landings()
            .activated()
            .iter()
            .all(|activation| activation.row == 0),
        "no pz row ran: the designed arming surface arms those nodes only \
         through OBJECTIVE18, whose own condition is the landing's output"
    );
    assert!(
        observed_state(&app, "hooked_to_klondike").is_none(),
        "the pz rows' record never started"
    );

    teardown(&mut app);
}
