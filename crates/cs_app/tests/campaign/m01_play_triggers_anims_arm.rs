//! Acceptance stage M01-PLAY-TRIGGERS-ANIMS-ARM (Rally #1497), the arming half
//! of parent #1482.
//!
//! Three things have to hold, and each is proved by a member of this suite:
//!
//! * **The carrier-record join resolves a name a directive spells.** The scope
//!   binding carries a measured animation **name → carrier record** index
//!   (`MissionAnimationBinding::names`), built over the mission carrier first
//!   and the world group's camera carrier second by the record's stored
//!   `anim_name`, exactly as the startup identities are. A name no record
//!   stores, a name two records store and a carrier whose walk refused are
//!   three different refusals by name, never a nearest match.
//! * **The `WAKE_ANIM` consumer runs the record.** `MissionHost::step`
//!   consumes every `WakeAnimation` emission the session's directive log
//!   carries that it has not applied (deduplicated by execution key, exactly
//!   as the mission countdown deduplicates the timer directives), resolves the
//!   name through that index and starts the record through the record player
//!   at the tick being stepped. The record's state is observed through the
//!   host's own stage-4 fold — the running record's identity → `RUNNING`, the
//!   finished one's → `EXECUTED` — and by nothing else.
//! * **The woken record's active-state statements are recorded.** The designed
//!   node-arming surface ([`cs_app::mission_animations::ArmedNode`]) keeps what
//!   each running record's published `OBJECT_ACTIVE_STATE` statements state,
//!   with the publishing record's own node names and the record's own span, for
//!   the landing half (M01-PLAY-TRIGGERS-ANIMS-LAND, #1498) to consume.
//!
//! # What is synthetic here, and what is not
//!
//! The synthetic members author a record, a declaration and a control program
//! in the **measured shapes** the production readers accept — the same `.zrd`
//! authoring and the same event-stream grammar the #678/#690/#718 acceptance
//! files use — and read them through `read_animation_definition_member`,
//! `decode_event_stream`, `bind_mission_animation`'s own join rules
//! (`match_record_by_name`, `join_named_animation`) and the composed host. No
//! value in them claims anything about the original game.
//!
//! The `#[ignore]`d retail members read `$CS_GAME_DIR`: they bind M01's real
//! animation closure and show the three woken names resolving to the records
//! #1482 measured, an unbound name refused by name, and the real `WAKE_ANIM`
//! directive of `OBJECTIVE11` starting its named record and the host's fold
//! observing it `RUNNING` and then `EXECUTED`. The gate the block waits for is
//! supplied by the test as the world-observation row the landing half will
//! produce; that is stated where it happens and in the finding.
//!
//! Nothing here is `verified_original`: `retail` is read access to the owner's
//! installation, and no original executable ran.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use bevy::ecs::world::World;
use bevy::prelude::App;
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::animation::carrier::{UNBOUND_REASON_AMBIGUOUS, UNBOUND_REASON_NO_RECORD};
use cs_app::animation::events::{EVENT_HEADER_BYTES, decode_event_stream};
use cs_app::animation::mission::{
    AnimationNameIndex, AnimationRecordFacts, AnimationTarget, AuthoredMissionAnimations,
    CarrierFact, CarrierRecords, DECLARATION_MATCH_CLAIM, MissionAnimationBinding,
    NamedAnimationResolution, NamedRecordMatch, RecordSequence, SequenceEvents, TargetSource,
    WAKE_ANIMATION_SOURCE, bind_mission_animation, join_named_animation, match_record_by_name,
};
use cs_app::animation::programs::{
    ANIMATION_DEFINITION_FIELD, ANIMATION_DEFINITIONS_RECORD, ANIMATION_LIST_FIELD,
    ANIMATION_NAME_FIELD, AnimationDefinitionSite, NAME_FIELD, SEQUENCE_FIELD, SEQUENCE_NAME_FIELD,
    read_animation_definition_member,
};
use cs_app::animation::survey::CarrierKind;
use cs_app::mission_animations::{ActivationOutcome, OBJECT_ACTIVE_STATE_STATEMENT};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionHostReport, MissionHostTick, MissionStage, MissionWorldObservation,
    build_headless, stage_for, teardown,
};
use cs_app::world_facts::WorldObservation;
use cs_formats::zbd::AnimationRecordSequenceKind;
use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId, Value,
};
use cs_script::runtime::{SessionGeneration, TerminalState};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::net::SessionId;

use crate::campaign::common::{cid, label};
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The animation name the synthetic stage's `WAKE_ANIM` site wakes.
const SYNTHETIC_WAKE: &str = "synthetic_wake_anim";

/// A name nothing in the synthetic stage's carriers stores.
const SYNTHETIC_UNBOUND: &str = "synthetic_nothing_binds_this";

/// The synthetic record's one ordinary sequence block.
const SYNTHETIC_SEQUENCE: &str = "PLAY";

// ---------------------------------------------------------------------------
// Synthetic `.zrd` authoring, in the measured grammar the readers accept
// (tags `3` text, `4` list, a record's root as a one-element list holding a
// flat alternating `KEY, value` body). The same shapes
// `accept_t718_mission_animations.rs` uses.
// ---------------------------------------------------------------------------

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

fn zrd_flat(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::with_capacity(entries.len() * 2);
    for (key, value) in entries {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(children)
}

fn zrd_record(body: Vec<u8>) -> Vec<u8> {
    zrd_list(vec![body])
}

fn zrd_sequence(name: &str, statements: &[&str]) -> Vec<u8> {
    let mut entries: Vec<(&str, Vec<u8>)> = Vec::new();
    entries.push((SEQUENCE_NAME_FIELD, zrd_text(name)));
    for statement in statements {
        entries.push((statement, zrd_list(vec![zrd_text("statement")])));
    }
    zrd_flat(entries)
}

fn animation_definition_member(definitions: Vec<Vec<u8>>) -> Vec<u8> {
    let mut list = Vec::with_capacity(definitions.len() * 2);
    for definition in definitions {
        list.push(zrd_text(ANIMATION_DEFINITION_FIELD));
        list.push(definition);
    }
    zrd_record(zrd_flat(vec![(
        ANIMATION_DEFINITIONS_RECORD,
        zrd_flat(vec![(ANIMATION_LIST_FIELD, zrd_list(list))]),
    )]))
}

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a static claim id is valid")
}

fn synthetic_provenance(id: &str) -> Provenance {
    Provenance::new(claim(id), ClaimStatus::ObservedTool, None)
        .expect("an observed-tool claim with no source span is valid")
}

fn synthetic_hash() -> ContentHash {
    ContentHash::from_bytes([7u8; 32])
}

/// One synthetic event: the measured eight-byte tag/length header, a payload
/// whose word `0` is `start` and whose last word is `run`, and the byte length
/// each measured opcode's payload has.
fn synthetic_event(opcode: u8, group: u8, start: f32, run: f32) -> Vec<u8> {
    let payload_len = match opcode {
        6 => 12,
        11 => 136,
        24 => 72,
        _ => 32,
    };
    let mut bytes = (u32::from(opcode) | (u32::from(group) << 8))
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(
        &u32::try_from(EVENT_HEADER_BYTES + payload_len)
            .expect("a small length")
            .to_le_bytes(),
    );
    let words = payload_len / 4;
    for index in 0..words {
        let value = if index == 0 {
            start
        } else if index + 1 == words {
            run
        } else {
            0.0
        };
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// The record's own nodes, in the measured shape: entry `0` is the empty name
/// every retail table starts with.
fn synthetic_nodes() -> Vec<String> {
    vec![
        String::new(),
        "synthetic_actor".to_owned(),
        "synthetic_landing_node".to_owned(),
    ]
}

/// The synthetic record: one ordinary `PLAY` block carrying an
/// `OBJECT_ACTIVE_STATE` statement (opcode 6) and a motion statement (opcode
/// 11) whose `RUN_TIME` of 2.0 gives the record a measured duration, so its
/// state is observably `RUNNING` for a stretch and then `EXECUTED`.
fn synthetic_record() -> AnimationRecordFacts {
    let events: Vec<u8> = [
        synthetic_event(6, 1, 0.0, 0.0),
        synthetic_event(11, 1, 0.0, 2.0),
    ]
    .concat();
    AnimationRecordFacts {
        carrier: CarrierKind::Mission,
        carrier_key: "zbd/synthetic/mis_anim.zbd".to_owned(),
        index: 0,
        span: SourceSpan::new(
            synthetic_hash(),
            "zbd/synthetic/mis_anim.zbd",
            None,
            4_096,
            672,
            None,
        )
        .expect("the synthetic span is valid"),
        provenance: synthetic_provenance(DECLARATION_MATCH_CLAIM),
        anim_name: SYNTHETIC_WAKE.to_owned(),
        object_name: "synthetic_actor".to_owned(),
        root_name: "synthetic_actor".to_owned(),
        flags: 0x0044_48B0,
        status: 0,
        activation: 3,
        execution_priority: 4,
        reset_time: -1.0,
        max_health: 0.0,
        objects: Vec::new(),
        nodes: synthetic_nodes(),
        animation_refs: vec!["synthetic_called_anim".to_owned()],
        call_targets: Vec::new(),
        sequences: vec![RecordSequence {
            kind: AnimationRecordSequenceKind::Sequence,
            name: SYNTHETIC_SEQUENCE.to_owned(),
            event_bytes: events.len() as u64,
            events: match decode_event_stream(&events) {
                Ok(decoded) => SequenceEvents::Decoded(decoded),
                Err(error) => SequenceEvents::refused(&error),
            },
        }],
    }
}

/// The synthetic declaration of [`SYNTHETIC_WAKE`], in the measured `.zrd`
/// shape, read through the production definition reader.
fn synthetic_declaration() -> AnimationDefinitionSite {
    let member = animation_definition_member(vec![zrd_flat(vec![
        (ANIMATION_NAME_FIELD, zrd_text(SYNTHETIC_WAKE)),
        (NAME_FIELD, zrd_list(vec![zrd_text("synthetic_actor")])),
        (
            SEQUENCE_FIELD,
            zrd_sequence(SYNTHETIC_SEQUENCE, &["CALL_ANIMATION"]),
        ),
    ])]);
    let read = read_animation_definition_member("synthetic_anim.zrd", &member)
        .expect("the authored member reads");
    let span = SourceSpan::new(
        synthetic_hash(),
        "zbd/synthetic/zrdr.zbd",
        Some("synthetic_anim.zrd"),
        128,
        256,
        None,
    )
    .expect("the synthetic span is valid");
    AnimationDefinitionSite::new(
        "zbd/synthetic/zrdr.zbd",
        "synthetic_anim.zrd",
        span,
        read.definitions()[0].clone(),
    )
}

/// The synthetic stage's animation binding, assembled from authored pieces
/// through the production [`MissionAnimationBinding::authored`] seam.
fn synthetic_binding() -> MissionAnimationBinding {
    let key = "zbd/synthetic/mis_anim.zbd".to_owned();
    MissionAnimationBinding::authored(AuthoredMissionAnimations {
        scope: "zbd/synthetic/m01".to_owned(),
        group: "synthetic".to_owned(),
        world_container: "zbd/synthetic/gamez.zbd".to_owned(),
        archives: vec!["zbd/synthetic/zrdr.zbd".to_owned()],
        carriers: vec![CarrierFact {
            kind: CarrierKind::Mission,
            container_key: key.clone(),
            record_count: 1,
            declared_record_count: 1,
            blockers: Vec::new(),
        }],
        records: vec![CarrierRecords::authored(
            CarrierKind::Mission,
            key,
            vec![synthetic_record()],
        )],
        declarations: vec![synthetic_declaration()],
        startup: Vec::new(),
        placements: Vec::new(),
        measured_placements: Vec::new(),
        world: None,
    })
    .expect("the authored binding assembles")
}

/// The synthetic control program: two objectives that both complete on their
/// first evaluated tick — one waking [`SYNTHETIC_WAKE`], one waking
/// [`SYNTHETIC_UNBOUND`] — authored here as synthetic content, never a reading
/// of any original mission's record.
fn synthetic_wake_program() -> MissionProgram {
    let directive = |name: &str| Action::Directive {
        operation: DirectiveOperation::WakeAnimation,
        args: vec![Value::Str(name.to_owned())],
    };
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-mission-anim-arm"),
        variables: Vec::new(),
        objectives: vec![
            Objective {
                id: SymbolId(0),
                content: cid(ContentKind::Objective, "synthetic-arm-wake"),
                condition: Condition::Const(true),
                actions: vec![directive(SYNTHETIC_WAKE)],
                span: None,
            },
            Objective {
                id: SymbolId(1),
                content: cid(ContentKind::Objective, "synthetic-arm-unbound"),
                condition: Condition::Const(true),
                actions: vec![directive(SYNTHETIC_UNBOUND)],
                span: None,
            },
        ],
    }
}

/// A synthetic stage over the production harbor world whose control program
/// carries the two `WAKE_ANIM` sites and whose animation join resolves the
/// first. Everything else is `synthetic_stage`'s own content.
fn synthetic_arm_stage(scratch: &Path) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.host.control = synthetic_wake_program();
    stage.host.animation = Some(synthetic_binding());
    stage
}

/// Pumps a composed app one committed fixed tick per frame until the host
/// reaches `until`, and answers the last tick the composed entry wrote.
fn pump_to(app: &mut App, until: u64) -> u64 {
    let timestep = app.world().resource::<Time<Fixed>>().timestep();
    let mut tick = 0;
    for _ in 0..20_000 {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        let Some(report) = app.world().get_resource::<MissionHostReport>() else {
            continue;
        };
        tick = report.tick.0;
        if tick >= until {
            break;
        }
    }
    tick
}

/// The state the host's own stage-4 fold holds for one record, as the
/// observation carries it.
fn observed_state(app: &App, name: &str) -> Option<u32> {
    app.world().get_resource::<MissionHost>().and_then(|host| {
        host.world_facts()
            .observation()
            .animations()
            .get(name)
            .copied()
    })
}

/// The composed entry's answer for the last tick, or a panic naming the
/// refusal.
fn last_answer(app: &App) -> Tick {
    let report = app
        .world()
        .get_resource::<MissionHostReport>()
        .expect("the composed entry wrote a report");
    match &report.answer {
        Ok(MissionHostTick { tick, .. }) => *tick,
        Err(error) => panic!("the composed step refused: {error}"),
    }
}

// ---------------------------------------------------------------------------
// The join surface, on authored content (runs in CI)
// ---------------------------------------------------------------------------

/// **The name → record match binds exactly one match, refuses none, more than
/// one, and a carrier whose walk refused — and the join over the bound name
/// produces a playable row with the name as its identity and
/// `wake_animation` as its activation source.**
#[test]
fn accept_m01_play_triggers_anims_arm_the_name_join_binds_one_match_and_refuses_the_rest() {
    let record = synthetic_record();
    let key = "zbd/synthetic/mis_anim.zbd".to_owned();
    let carrier = CarrierRecords::authored(CarrierKind::Mission, key.clone(), vec![record.clone()]);

    assert_eq!(
        match_record_by_name(std::slice::from_ref(&carrier), SYNTHETIC_WAKE),
        NamedRecordMatch::One {
            carrier: CarrierKind::Mission,
            index: 0,
        },
        "the one record that stores the name binds"
    );
    assert_eq!(
        match_record_by_name(std::slice::from_ref(&carrier), SYNTHETIC_UNBOUND),
        NamedRecordMatch::None,
        "a name nothing stores matches nothing"
    );

    // A second carrier storing the same name makes it ambiguous: the join
    // never prefers one of two records.
    let second = CarrierRecords::authored(
        CarrierKind::Camera,
        "zbd/synthetic/cam_anim.zbd".to_owned(),
        vec![record.clone()],
    );
    let both = vec![carrier.clone(), second];
    let ambiguous = match_record_by_name(&both, SYNTHETIC_WAKE);
    assert!(
        matches!(ambiguous, NamedRecordMatch::Ambiguous(ref m) if m.len() == 2),
        "two records storing one name are ambiguous, got {ambiguous:?}"
    );

    // A carrier whose walk refused makes "exactly one" unestablishable.
    let refused = CarrierRecords::unwalked(CarrierKind::Camera, "zbd/synthetic/cam_anim.zbd");
    let refused_walk = vec![carrier.clone(), refused];
    assert_eq!(
        match_record_by_name(&refused_walk, SYNTHETIC_WAKE),
        NamedRecordMatch::NotWalked,
        "a refused walk leaves the name unresolved, never bound"
    );

    // The index's own verdicts, over the same carriers.
    let index = AnimationNameIndex::new(vec![carrier.clone()], vec![synthetic_declaration()], None);
    let bound = index
        .resolve(SYNTHETIC_WAKE)
        .bound()
        .expect("the name binds")
        .clone();
    assert_eq!(bound.identity(), SYNTHETIC_WAKE);
    assert_eq!(bound.event(), WAKE_ANIMATION_SOURCE);
    assert!(
        bound.is_playable(),
        "the authored row is playable, refusals: {:?}",
        bound.refusals()
    );
    assert_eq!(
        bound.bound_record().map(AnimationRecordFacts::index),
        Some(0)
    );
    let playback = bound.playback().expect("the authored record plays");
    assert!(
        playback.duration_time() > 0.0,
        "the CALL_ANIMATION run time gives the record a duration, got {}",
        playback.duration_time()
    );
    assert_eq!(
        bound
            .targets()
            .iter()
            .filter(|target| target.source() == TargetSource::RecordNode)
            .map(AnimationTarget::stored)
            .collect::<Vec<_>>(),
        synthetic_nodes(),
        "the row carries the record's own node table, verbatim"
    );

    match index.resolve(SYNTHETIC_UNBOUND) {
        NamedAnimationResolution::Unbound { reason, .. } => {
            assert_eq!(reason, UNBOUND_REASON_NO_RECORD)
        }
        other => panic!("a name nothing binds must be refused, got {other:?}"),
    }

    // A second carrier of the *same* kind does not shadow the match: the
    // record read back is the one whose own table states the name, never the
    // first carrier of that kind. The first carrier's record 0 is stored
    // under another name.
    let mut other = record.clone();
    other.anim_name = "synthetic_other_anim".to_owned();
    let decoy = CarrierRecords::authored(
        CarrierKind::Mission,
        "zbd/synthetic/mis_anim_first.zbd".to_owned(),
        vec![other],
    );
    let two_mission = AnimationNameIndex::new(
        vec![decoy, carrier.clone()],
        vec![synthetic_declaration()],
        None,
    );
    let shadowed = two_mission
        .row(SYNTHETIC_WAKE)
        .expect("the name binds in the second carrier");
    assert_eq!(
        shadowed
            .bound_record()
            .map(|facts| (facts.index(), facts.anim_name().to_owned())),
        Some((0, SYNTHETIC_WAKE.to_owned())),
        "the record read back must be the one the matched table states"
    );
    match index.resolve(SYNTHETIC_WAKE) {
        NamedAnimationResolution::Bound(_) => {}
        other => panic!("the stored name must bind, got {other:?}"),
    }
    assert_eq!(
        AnimationNameIndex::new(both.clone(), vec![synthetic_declaration()], None)
            .resolve(SYNTHETIC_WAKE)
            .unbound()
            .map(|(reason, _)| reason),
        Some(UNBOUND_REASON_AMBIGUOUS),
        "two records storing the name are ambiguous"
    );

    // The join helper itself, over a declaration nobody wrote: the same
    // `Undeclared` refusal the startup join produces.
    let undeclared = join_named_animation(SYNTHETIC_WAKE, record, &[], None);
    assert!(
        !undeclared.is_playable(),
        "a name no member declares must not play"
    );
}

/// **The declared startup binding of an authored name is not what a woken name
/// is held to: a row whose declaration names another animation is refused by
/// the same agreement rule the startup rows use, and a name whose declaring
/// member disagrees about the object keeps both spellings.**
#[test]
fn accept_m01_play_triggers_anims_arm_a_named_row_agrees_like_a_startup_row() {
    let record = synthetic_record();
    let mut renamed = record.clone();
    renamed.object_name = "another_actor".to_owned();
    renamed.root_name = "another_actor".to_owned();
    let index = AnimationNameIndex::new(
        vec![CarrierRecords::authored(
            CarrierKind::Mission,
            "zbd/synthetic/mis_anim.zbd",
            vec![renamed],
        )],
        vec![synthetic_declaration()],
        None,
    );
    let row = index.row(SYNTHETIC_WAKE).expect("the name binds");
    assert!(
        !row.is_playable(),
        "the record's object_name is not one of the declaring member's selectors"
    );
    assert!(
        row.refusals()
            .iter()
            .any(|refusal| refusal.label() == "object_name_disagrees"),
        "the disagreement is named, got {:?}",
        row.refusals()
    );
}

// ---------------------------------------------------------------------------
// The consumer and the fold, on the composed host (runs in CI)
// ---------------------------------------------------------------------------

/// **The composed host runs the record a `WAKE_ANIM` directive names: the
/// directive is consumed once, the record starts, its state is `RUNNING`
/// through the host's stage-4 fold and then `EXECUTED`, its active-state
/// statement reaches the designed arming surface, and a name nothing binds is
/// refused by name.**
#[test]
fn accept_m01_play_triggers_anims_arm_synthetic_wake_directive_runs_the_record_through_the_fold() {
    let scratch = Scratch::new("arm");
    let stage = synthetic_arm_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    // Pump until the woken record reaches its measured duration: 2.0 stored
    // time units on the player's 120 Hz timeline is 240 committed ticks, so a
    // fixed budget would either be far too long or too short.
    let mut tick = 0;
    for _ in 0..1_200 {
        tick = pump_to(&mut app, tick + 1);
        if observed_state(&app, SYNTHETIC_WAKE)
            == Some(cs_script::ir::AnimationState::Executed.code())
        {
            break;
        }
    }
    let _ = last_answer(&app);

    let host = app.world().resource::<MissionHost>();
    let ledger = host.wake_animations();

    // The wake site ran and started the record it names.
    let activation = ledger
        .activations()
        .iter()
        .find(|activation| activation.name() == SYNTHETIC_WAKE)
        .unwrap_or_else(|| panic!("no activation for {SYNTHETIC_WAKE}: {ledger:?}"));
    assert_eq!(
        activation.outcome(),
        ActivationOutcome::Started,
        "the record must start on the directive"
    );
    // One activation per name: the directive is applied exactly once, whatever
    // the host kept stepping.
    assert_eq!(
        ledger
            .activations()
            .iter()
            .filter(|activation| activation.name() == SYNTHETIC_WAKE)
            .count(),
        1,
        "the execution key is applied exactly once"
    );
    assert!(
        ledger
            .refused()
            .iter()
            .all(|refusal| refusal.name() != SYNTHETIC_WAKE),
        "a name that binds is never refused"
    );

    // A name nothing binds is refused by name, with the join's own reason.
    let refusal = ledger
        .refused()
        .iter()
        .find(|refusal| refusal.name() == SYNTHETIC_UNBOUND)
        .unwrap_or_else(|| panic!("{SYNTHETIC_UNBOUND} must be refused: {ledger:?}"));
    assert_eq!(refusal.reason(), UNBOUND_REASON_NO_RECORD);

    // The record's own identity reaches the host's stage-4 fold: `RUNNING`
    // while it runs and `EXECUTED` once it reaches its measured duration.
    let states = host.world_facts().observation().animations();
    assert_eq!(
        states.get(SYNTHETIC_WAKE),
        Some(&cs_script::ir::AnimationState::Executed.code()),
        "the finished record must be observed executed, got {states:?}"
    );

    // The designed arming input: the record's own OBJECT_ACTIVE_STATE
    // statement, with the record's own node table.
    let armed = host
        .armed_nodes()
        .iter()
        .find(|armed| armed.animation() == SYNTHETIC_WAKE)
        .unwrap_or_else(|| panic!("no armed node for {SYNTHETIC_WAKE}"));
    assert_eq!(armed.statement(), OBJECT_ACTIVE_STATE_STATEMENT);
    assert_eq!(armed.nodes(), synthetic_nodes());
    assert_eq!(
        armed.claim_id(),
        cs_app::mission_animations::OBJECT_ACTIVE_ARMING_CLAIM
    );
    assert!(
        armed.tick() <= Tick(tick),
        "the arming tick must be one the host stepped"
    );

    // The same record the runtime started is the player's own running/finished
    // record: the fold has no other source.
    assert!(
        host.animation()
            .finished()
            .any(|record| record.identity() == SYNTHETIC_WAKE),
        "the record the directive started must be the one that finished"
    );

    teardown(&mut app);
}

/// **A record started by a woken name is `RUNNING` before it is `EXECUTED`:
/// the fold observes the running state on the tick after the activation and
/// the executed state only when the record's measured duration is reached.**
///
/// The first member asserts the end state; this one pins the order, which is
/// what the gated blocks of the parent task read.
#[test]
fn accept_m01_play_triggers_anims_arm_a_woken_record_is_running_before_executed() {
    let scratch = Scratch::new("order");
    let stage = synthetic_arm_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    let mut running_seen = None;
    let mut executed_seen = None;
    for _ in 0..400 {
        let next = app
            .world()
            .get_resource::<MissionHostReport>()
            .map_or(1, |report| report.tick.0 + 1);
        let tick = pump_to(&mut app, next);
        let states = app
            .world()
            .resource::<MissionHost>()
            .world_facts()
            .observation()
            .animations();
        match states.get(SYNTHETIC_WAKE) {
            Some(state) if *state == cs_script::ir::AnimationState::Running.code() => {
                running_seen.get_or_insert(tick);
            }
            Some(state) if *state == cs_script::ir::AnimationState::Executed.code() => {
                executed_seen = Some(tick);
                break;
            }
            _ => {}
        }
    }

    let running = running_seen.expect("the woken record must be observed running");
    let executed = executed_seen.expect("the woken record must be observed executed");
    assert!(
        running < executed,
        "RUNNING must be observed before EXECUTED (running {running:?}, executed {executed:?})"
    );

    teardown(&mut app);
}

// ---------------------------------------------------------------------------
// M01's own records, through the installation (retail)
// ---------------------------------------------------------------------------

/// M01's launch plan and stage, read once per retail member.
fn m01_stage() -> (MissionStage, std::path::PathBuf) {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    assert!(
        plan.launchable(),
        "every launch surface must be satisfied before the composition runs: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    (stage, root)
}

/// **Against the owner's installation, the three `WAKE_ANIM` names M01's own
/// blocks spell resolve to the records #1482 measured, and a name nothing
/// stores is refused by name.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_anims_arm_retail_the_three_woken_names_join_their_records() {
    // This member binds the scope's animation closure and nothing else, so it
    // does not need the launch closure or the composition: it reads exactly
    // the join a `WAKE_ANIM` name resolves through.
    let binding = bind_mission_animation(&game_dir(), "zbd/c1c/m01")
        .expect("M01's animation closure reads through the production readers");
    let names = binding.names();
    assert!(
        names.record_count() > 800,
        "M01's two carriers hold hundreds of records, got {}",
        names.record_count()
    );

    // The measured join, #1482's own table: the woken name, the record's index
    // in the mission carrier and the member that declares it.
    let measured = [
        ("activate_dropoff_node", 498_usize, "wv_tailhook.zrd"),
        ("activate_pickup_node", 500, "wv_tailhook.zrd"),
        ("pzhomebase", 188, "pzep_hookup.zrd"),
    ];
    for (name, index, member) in measured {
        let row = names
            .row(name)
            .unwrap_or_else(|| panic!("{name} must resolve through M01's join"));
        let record = row.bound_record().unwrap_or_else(|| {
            panic!(
                "{name} must bind a carrier record, refusals: {:?}",
                row.refusals()
            )
        });
        assert_eq!(
            record.carrier(),
            CarrierKind::Mission,
            "{name} lives in the mission carrier"
        );
        assert_eq!(record.index(), index, "{name}'s measured record index");
        assert_eq!(record.anim_name(), name);
        let site = row
            .declaration()
            .resolution()
            .single()
            .unwrap_or_else(|| panic!("{name} must have exactly one declaring site"));
        assert_eq!(site.member(), member, "{name}'s declaring member");
        assert!(
            row.is_playable(),
            "{name}'s row must be playable, refusals: {:?}",
            row.refusals()
        );
    }

    // The durations the join measures are what the player's timeline is
    // scaled by, so they are pinned here too: the two node-activating records
    // state no `RUN_TIME` at all (their measured duration is zero), while
    // `pzhomebase` runs for three stored time units.
    for (name, duration) in [
        ("activate_dropoff_node", 0.0_f32),
        ("activate_pickup_node", 0.0),
        ("pzhomebase", 3.0),
    ] {
        let row = names.row(name).expect("the woken name binds");
        let playback = row.playback().expect("the woken record plays");
        assert!(
            (playback.duration_time() - duration).abs() < 1.0e-6,
            "{name}'s measured duration is {duration}, got {}",
            playback.duration_time()
        );
    }

    // `fadein_bszep` is the fourth `WAKE_ANIM` name of M01's record and has
    // no `ANIMATION_NAME` anywhere: the name matches carrier record 344, but
    // no member declares it, so the join refuses the row as undeclared — the
    // same rule a startup identity gets, and never a silent start.
    let fadein = names
        .row("fadein_bszep")
        .expect("the name matches a record");
    assert_eq!(
        fadein.bound_record().map(AnimationRecordFacts::index),
        Some(344)
    );
    assert!(
        !fadein.is_playable(),
        "a name no member declares must not play, refusals: {:?}",
        fadein.refusals()
    );

    // A name nothing in M01's closure stores is refused by name.
    match names.resolve("m01_nothing_binds_this_name") {
        NamedAnimationResolution::Unbound { reason, .. } => {
            assert_eq!(reason, UNBOUND_REASON_NO_RECORD);
        }
        other => panic!("a name nothing stores must be refused, got {other:?}"),
    }

    // The binding's own public seam answers the same verdict.
    assert!(
        matches!(
            binding.animation_by_name("activate_dropoff_node"),
            NamedAnimationResolution::Bound(_)
        ),
        "the binding's own name seam must bind the measured name"
    );
}

/// **Against the owner's installation, M01's own `OBJECTIVE18` block emits its
/// `WAKE_ANIM` directive, the host consumes it, and the host's stage-4 fold
/// observes the woken record `RUNNING` and then `EXECUTED`.**
///
/// `pzhomebase` is the woken record with a positive measured duration (three
/// stored time units, pinned by the join member above), so its `RUNNING` state
/// is observable for a stretch of the player's timeline before the record
/// reaches its end. `OBJECTIVE11`'s own record (`activate_dropoff_node`) states
/// no run time at all, so it goes straight from its activation to `EXECUTED`
/// on the first advance — this member observes that record too, and pins the
/// difference instead of pretending the two behave alike.
///
/// # What the test supplies
///
/// `OBJECTIVE18` completes when its own gate (`hooked_to_klondike`
/// `EXECUTED`) holds, and `OBJECTIVE11` when `wv_drop_copilot` `RUNNING`
/// holds. Those states come from the *landing* path — M01's `landings.zrd`
/// rows run `wv_initiate_hookup` (which calls `wv_drop_copilot`) and
/// `hooked_to_klondike` — and the landing half is
/// M01-PLAY-TRIGGERS-ANIMS-LAND (#1498). This member therefore supplies those
/// two world-observation rows itself, through the same port
/// `mission_session::observation` writes, so the **real** directive sites run;
/// everything after them (the join, the consumer, the record player, the fold)
/// is production code. The two blocks' lifecycles are declared awake by the
/// test for the same reason: the measured wake chain is #1498's subject, not
/// this member's.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_triggers_anims_arm_retail_a_woken_record_runs_through_the_host_fold() {
    let (mut stage, _root) = m01_stage();

    // Only OBJECTIVE11 (block index 10) and OBJECTIVE18 (17) are awake, and
    // the recovered engine model is not folded, so no other block can complete
    // and settle the run before the wake sites run.
    let mut table = BlockLifecycleTable::new();
    table
        .declare(10, LifecycleDecl::awake())
        .expect("OBJECTIVE11 is declared once");
    table
        .declare(17, LifecycleDecl::awake())
        .expect("OBJECTIVE18 is declared once");
    stage.host.block_lifecycles = table;
    stage.host.engine_objectives = None;

    let generation = SessionGeneration(1);
    let served = SessionId::new(1).expect("session 1 is valid");
    let mut host = MissionHost::launch(&stage, generation, served).expect("M01's host launches");

    // The two woken names, their measured durations and the gate row each
    // block waits for.
    let cases = [
        ("activate_dropoff_node", "wv_drop_copilot", 0.0_f32),
        ("pzhomebase", "hooked_to_klondike", 3.0),
    ];
    let rate = host.animation().ticks_per_second();
    let longest = cases
        .iter()
        .map(|(name, _, duration)| {
            let row = host
                .animation_names()
                .row(name)
                .unwrap_or_else(|| panic!("{name} must resolve through M01's join"));
            let measured = row
                .playback()
                .unwrap_or_else(|| panic!("{name} must play"))
                .duration_time();
            assert!(
                (measured - duration).abs() < 1.0e-6,
                "{name}'s measured duration is {duration}, got {measured}"
            );
            u64::from((f64::from(measured) * f64::from(rate)).ceil() as u32)
        })
        .max()
        .expect("two cases");

    let mut world = World::new();
    let timestep = Duration::from_secs_f64(1.0 / f64::from(rate));
    let mut running: BTreeMap<&str, u64> = BTreeMap::new();
    let mut executed: BTreeMap<&str, u64> = BTreeMap::new();
    let mut armed_tick = None;
    for tick in 1..=(longest + 8) {
        // The world's own observation port for this tick: the gate states the
        // landing half will publish, and nothing else (see the member doc).
        world.insert_resource(MissionWorldObservation {
            observation: WorldObservation::new()
                .animation(
                    "wv_drop_copilot",
                    cs_script::ir::AnimationState::Running.code(),
                )
                .animation(
                    "hooked_to_klondike",
                    cs_script::ir::AnimationState::Executed.code(),
                ),
        });
        world.insert_resource(cs_app::physics::PhysicsTickLedger {
            ticks: tick,
            ..Default::default()
        });
        let produced = host
            .step(&mut world, timestep)
            .unwrap_or_else(|error| panic!("M01's composed step at tick {tick} refused: {error}"));
        assert_eq!(produced.tick, Tick(tick));
        assert_eq!(
            produced.script.terminal,
            TerminalState::Running,
            "the run must not settle before the records finish"
        );

        let states = host.world_facts().observation().animations().clone();
        for (name, _, _) in cases {
            match states.get(name).copied() {
                Some(state) if state == cs_script::ir::AnimationState::Running.code() => {
                    running.entry(name).or_insert(tick);
                }
                Some(state) if state == cs_script::ir::AnimationState::Executed.code() => {
                    executed.entry(name).or_insert(tick);
                }
                _ => {}
            }
        }
        if host
            .armed_nodes()
            .iter()
            .any(|armed| armed.animation() == "activate_dropoff_node")
        {
            armed_tick.get_or_insert(tick);
        }
        if executed.len() == cases.len() {
            break;
        }
    }

    // Both real directives are M01's own sites, and the host consumed both.
    for (name, _, _) in cases {
        let emitted = host.script().state().directives().iter().any(|emission| {
            emission.operation == DirectiveOperation::WakeAnimation
                && emission.args == vec![Value::Str(name.to_owned())]
        });
        assert!(
            emitted,
            "M01's own record must spell a WAKE_ANIM site for {name}: {:?}",
            host.script().state().directives()
        );
        let activation = host
            .wake_animations()
            .activations()
            .iter()
            .find(|activation| activation.name() == name)
            .unwrap_or_else(|| {
                panic!("the host must consume {name}: {:?}", host.wake_animations())
            });
        assert_eq!(activation.outcome(), ActivationOutcome::Started, "{name}");
        assert!(
            host.wake_animations()
                .activations()
                .iter()
                .filter(|activation| activation.name() == name)
                .count()
                == 1,
            "{name}'s directive is applied exactly once"
        );
    }

    // `pzhomebase` has a measured duration, so the fold sees it running and
    // then executed.
    let running_homebase = *running
        .get("pzhomebase")
        .unwrap_or_else(|| panic!("pzhomebase must be observed running, got {running:?}"));
    let executed_homebase = *executed
        .get("pzhomebase")
        .unwrap_or_else(|| panic!("pzhomebase must be observed executed, got {executed:?}"));
    assert!(
        running_homebase < executed_homebase,
        "RUNNING must precede EXECUTED for pzhomebase (running {running_homebase}, executed \
         {executed_homebase})"
    );
    // `activate_dropoff_node` states no run time, so its measured duration is
    // zero and the fold observes it executed on the first advance after its
    // activation — never running, because the record never ran for a tick.
    assert!(
        !running.contains_key("activate_dropoff_node"),
        "a record with a zero measured duration is never observed running: {running:?}"
    );
    assert!(
        executed.contains_key("activate_dropoff_node"),
        "the zero-duration record must still be observed executed, got {executed:?}"
    );

    // The designed arming input: the woken record's own OBJECT_ACTIVE_STATE
    // statement, with the record's own node table.
    let armed = host
        .armed_nodes()
        .iter()
        .find(|armed| armed.animation() == "activate_dropoff_node")
        .unwrap_or_else(|| {
            panic!(
                "activate_dropoff_node's active-state statement must be recorded: {:?}",
                host.armed_nodes()
            )
        });
    assert_eq!(armed.statement(), OBJECT_ACTIVE_STATE_STATEMENT);
    assert_eq!(armed.opcode(), 6);
    assert_eq!(
        armed.claim_id(),
        cs_app::mission_animations::OBJECT_ACTIVE_ARMING_CLAIM
    );
    assert!(
        !armed.nodes().is_empty(),
        "the record's own node table must travel with the statement, got {:?}",
        armed.nodes()
    );
    assert_eq!(
        armed.nodes().first().map(String::as_str),
        Some(""),
        "entry 0 is the empty name every measured node table starts with"
    );
    assert!(
        armed_tick.is_some_and(|stepped| Tick(stepped) <= armed.tick()),
        "the arming tick must be one the host stepped"
    );
}

/// **On the owner's installation, the gated names resolve and the row
/// record's measured `CALL_ANIMATION` targets spell the names the win path
/// needs — the decode `call_targets()` exposes is production.**
///
/// `wv_initiate_hookup` is the record `wv_manual_land`'s landing row starts.
/// Its compiled event stream's `CALL_ANIMATION` statements spell eight names
/// — measured under `SPELLED_NAME_CLAIM` — and among them are the two the
/// win path's `ANIM_STATE` gates read: `wv_drop_copilot` (the drop-off beat,
/// `RUNNING`) and `wv_pickup_copilot` (the pick-up, `EXECUTED`). The
/// `animation_refs` table names only four of them; this test asserts the
/// spelled surface `call_targets()` reads so the gap cannot silently
/// return.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_anims_arm_the_row_records_spelled_calls_name_the_gated_states() {
    let binding =
        bind_mission_animation(&game_dir(), "zbd/c1c/m01").expect("M01's animation closure reads");
    let index = binding.names();

    let row = match index.resolve("wv_initiate_hookup") {
        NamedAnimationResolution::Bound(row) => row,
        other => panic!("wv_initiate_hookup must bind: {other:?}"),
    };
    let facts = row
        .bound_record()
        .expect("wv_initiate_hookup carries a record");
    for gated in ["wv_drop_copilot", "wv_pickup_copilot", "wv_hookup_player"] {
        assert!(
            facts.call_targets().iter().any(|name| name == gated),
            "{gated} is spelled by a CALL_ANIMATION event: {:?}",
            facts.call_targets()
        );
    }
    // Every name the record's reference table states is a spelled call too:
    // the designed landing remainder has nothing to add on the owner's data.
    for reference in facts.animation_refs() {
        assert!(
            facts.call_targets().iter().any(|name| name == reference),
            "ref {reference} is also spelled: {:?}",
            facts.call_targets()
        );
    }

    // The two gated names themselves bind to real records.
    for gated in ["wv_drop_copilot", "wv_pickup_copilot", "hooked_to_klondike"] {
        assert!(
            matches!(index.resolve(gated), NamedAnimationResolution::Bound(_)),
            "{gated} must bind"
        );
    }
}
