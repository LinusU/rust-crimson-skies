//! Acceptance for task #718 (`M01-LC-ACTOR-ANIM-CONSUMERS`): the mission
//! session's consumer of one mission scope's animation records, and the
//! composed tick that carries the animation log into the mission session.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`
//! (`### F20-D`, non-negotiable behaviors 1, 2 and 5). Shared contracts:
//! `docs/contracts/IDENTITY-CONTENT.md` (session generations) and
//! `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering"). Task test
//! prefix: `accept_t718_`.
//!
//! Every synthetic fixture below is newly authored data in the **measured
//! shapes** the production readers accept — the same `.zrd` authoring and the
//! same event-stream grammar the #678 and #690 acceptance files use — and no
//! value here claims anything about the original game. The `#[ignore]`d case
//! is the one that measures the installation: M01's own startup rows, joined
//! by `bind_mission_animation` and played through the consumer.

#![allow(clippy::too_many_lines)]

use std::collections::BTreeSet;

use bevy::ecs::world::World;
use bevy::math::Mat4;
use bevy::prelude::{GlobalTransform, Vec3};
use cs_app::animation::events::{EVENT_HEADER_BYTES, EventClass, decode_event_stream};
use cs_app::animation::mission::{
    AnimationRecordFacts, DECLARATION_MATCH_CLAIM, EVENTS_NOT_DECODED_CLAIM,
    PLACEMENT_FIELDS_CLAIM, PLACEMENT_FIELDS_REASON, RecordResolution, RecordSequence,
    SequenceEvents, StartupAnimation, bind_mission_animation, join_startup_animation,
};
use cs_app::animation::programs::{
    ANIMATION_DEFINITION_FIELD, ANIMATION_DEFINITIONS_RECORD, ANIMATION_LIST_FIELD,
    ANIMATION_NAME_FIELD, AnimationDefinitionSite, BindingResolution, LOAD_GAME_START, NAME_FIELD,
    NEW_GAME_START, SEQUENCE_FIELD, SEQUENCE_NAME_FIELD, StartupAnimationBinding,
    read_animation_definition_member,
};
use cs_app::animation::survey::CarrierKind;
use cs_app::animation::{
    AnimationInstance, AnimationLog, AnimationPlayback, advance_animation, bind_animated_node,
};
use cs_app::mission_animations::{
    MissionAnimationPlayer, MissionAnimationStepRefusal, PlayerError, PlayerTeardown,
    TickAdvanceRefusal, step_mission_animations,
};
use cs_app::mission_markers::{
    MissionMarkerBinding, MissionMarkerBindings, MissionMarkerConsumer, step_mission_with_markers,
};
use cs_app::objectives::{ObjectiveSession, lower_program};
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneNodeBinding};
use cs_content::animation::{
    SYNTHETIC_DOOR_MARKER, SYNTHETIC_DOOR_OPEN_TICK, declared_synthetic_door_clip,
};
use cs_content::objectives::{SYNTHETIC_REACHED_WRECK, declared_synthetic_objectives};
use cs_formats::zbd::AnimationRecordSequenceKind;
use cs_script::ir::SymbolId;
use cs_script::runtime::SessionGeneration;
use cs_sim::objectives::runtime::TickInput;
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::net::SessionId;

/// The mission generation the objective session is launched under.
const GEN: SessionGeneration = SessionGeneration(1);

/// The signal the fixture program's reveal rule names.
const REACHED_WRECK: SymbolId = SymbolId(SYNTHETIC_REACHED_WRECK.0);

/// The gameplay cue the synthetic door clip authors; the mission host's table
/// below binds it to `REACHED_WRECK`.
const DOOR_CUE: &str = "synthetic.hangar.door_opened";

/// The mission scope the retail case binds: `missions/bindings/M01.json`'s own
/// `zbd/c1c/m01`.
const M01: &str = "zbd/c1c/m01";

// ---------------------------------------------------------------------------
// Synthetic `.zrd` authoring, in the measured grammar the readers accept
// (tags `3` text, `4` list, a record's root as a one-element list holding a
// flat alternating `KEY, value` body).
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

fn zrd_sequence(name: Option<&str>, statements: &[&str]) -> Vec<u8> {
    let mut entries: Vec<(&str, Vec<u8>)> = Vec::new();
    if let Some(name) = name {
        entries.push((SEQUENCE_NAME_FIELD, zrd_text(name)));
    }
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

/// The stored fields of one synthetic record.
struct RecordSpec<'a> {
    anim_name: &'a str,
    object_name: &'a str,
    root_name: &'a str,
    sequences: Vec<(&'a str, AnimationRecordSequenceKind, u64)>,
}

fn record(spec: &RecordSpec<'_>, carrier: CarrierKind, index: usize) -> AnimationRecordFacts {
    AnimationRecordFacts {
        carrier,
        carrier_key: match carrier {
            CarrierKind::Mission => "zbd/c1c/m01/mis_anim.zbd".to_owned(),
            CarrierKind::Camera => "zbd/c1c/cam_anim.zbd".to_owned(),
        },
        index,
        span: SourceSpan::new(
            synthetic_hash(),
            "zbd/c1c/m01/mis_anim.zbd",
            None,
            4_096 + index as u64 * 672,
            672,
            None,
        )
        .expect("the synthetic span is valid"),
        provenance: synthetic_provenance(DECLARATION_MATCH_CLAIM),
        anim_name: spec.anim_name.to_owned(),
        object_name: spec.object_name.to_owned(),
        root_name: spec.root_name.to_owned(),
        flags: 0x0044_48B0,
        status: 0,
        activation: 3,
        execution_priority: 4,
        reset_time: -1.0,
        max_health: 0.0,
        objects: Vec::new(),
        nodes: Vec::new(),
        animation_refs: Vec::new(),
        sequences: spec
            .sequences
            .iter()
            .map(|(name, kind, event_bytes)| RecordSequence {
                kind: *kind,
                name: (*name).to_owned(),
                event_bytes: *event_bytes,
                events: SequenceEvents::Absent,
            })
            .collect(),
    }
}

fn bound(record: AnimationRecordFacts) -> RecordResolution {
    RecordResolution::Bound(Box::new(record))
}

fn site(
    archive: &str,
    member: &str,
    animation_name: &str,
    objects: Vec<&str>,
    sequences: Vec<Option<&str>>,
) -> AnimationDefinitionSite {
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        (ANIMATION_NAME_FIELD, zrd_text(animation_name)),
        (
            NAME_FIELD,
            zrd_list(objects.iter().copied().map(zrd_text).collect()),
        ),
    ];
    for name in &sequences {
        entries.push((
            SEQUENCE_FIELD,
            zrd_sequence(name.as_deref(), &["CALL_ANIMATION"]),
        ));
    }
    let member_bytes = animation_definition_member(vec![zrd_flat(entries)]);
    let read = read_animation_definition_member(member, &member_bytes)
        .expect("the synthetic member reads");
    let span = SourceSpan::new(synthetic_hash(), archive, Some(member), 128, 256, None)
        .expect("the synthetic span is valid");
    AnimationDefinitionSite::new(archive, member, span, read.definitions()[0].clone())
}

fn declaration(
    event: &str,
    identity: &str,
    resolution: BindingResolution,
) -> StartupAnimationBinding {
    StartupAnimationBinding::new(event, identity, resolution)
}

/// One event's bytes: the measured eight-byte tag/length header and a payload
/// whose word `0` is `start` and whose last word is `run`.
fn synthetic_event(opcode: u8, group: u8, start: f32, run: f32) -> Vec<u8> {
    let payload_len = match opcode {
        5 => 104,
        6 => 12,
        11 => 136,
        13 => 16,
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

fn stream(events: &[Vec<u8>]) -> Vec<u8> {
    events.concat()
}

/// One synthetic record whose sequences are decoded by the **production**
/// decoder, exactly as the mission consumer reads a walked carrier.
fn facts_with(
    anim: &str,
    object_name: &str,
    sequences: &[(&str, AnimationRecordSequenceKind, Vec<u8>)],
) -> AnimationRecordFacts {
    let spec = RecordSpec {
        anim_name: anim,
        object_name,
        root_name: object_name,
        sequences: Vec::new(),
    };
    let mut facts = record(&spec, CarrierKind::Mission, 36);
    facts.sequences = sequences
        .iter()
        .map(|(name, kind, bytes)| RecordSequence {
            kind: *kind,
            name: (*name).to_owned(),
            event_bytes: bytes.len() as u64,
            events: match decode_event_stream(bytes) {
                Ok(decoded) => SequenceEvents::Decoded(decoded),
                Err(error) => SequenceEvents::refused(&error),
            },
        })
        .collect();
    facts
}

/// Joins a declaration over a record: one archive, one member, one selector
/// (`piratezep`) and the sequence names the record must agree with.
fn joined_over(
    identity: &str,
    sequences: &[&str],
    facts: AnimationRecordFacts,
) -> StartupAnimation {
    let site = site(
        "zbd/zrdr.zbd",
        "pirate_zep_nacelles.zrd",
        identity,
        vec!["piratezep"],
        sequences.iter().map(|name| Some(*name)).collect(),
    );
    join_startup_animation(
        declaration(
            NEW_GAME_START,
            identity,
            BindingResolution::Single(Box::new(site)),
        ),
        bound(facts),
        None,
    )
}

/// The one synthetic row the timeline tests play: a motion statement at stored
/// time `0.5` that runs to `2.5`, and a call statement at `1.5`.
fn playable_row(identity: &str) -> StartupAnimation {
    let block = stream(&[
        synthetic_event(11, 1, 0.5, 2.0),
        synthetic_event(24, 3, 1.5, 0.0),
    ]);
    joined_over(
        identity,
        &["call_eachengine"],
        facts_with(
            identity,
            "piratezep",
            &[(
                "call_eachengine",
                AnimationRecordSequenceKind::Sequence,
                block,
            )],
        ),
    )
}

// ---------------------------------------------------------------------------
// Session fixtures.
// ---------------------------------------------------------------------------

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn node(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::SceneNode, key).expect("valid content id")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

/// A mission host's declared table binding both fixture gameplay cues.
fn bound_cues(target: SymbolId) -> MissionMarkerBindings {
    MissionMarkerBindings::new([
        MissionMarkerBinding::new(DOOR_CUE, target),
        MissionMarkerBinding::new("synthetic.plane.engine_started", target),
    ])
    .expect("the fixture cues are distinct and bind a live signal")
}

/// The mission host's tick input: its own facts, no signals of its own,
/// because the marker supplies them.
fn facts<'a>(tick: u64, committed: u64) -> TickInput<'a> {
    TickInput {
        tick: Tick(tick),
        committed_ticks: committed,
        lifecycles: &[],
        movements: &[],
        signals: &[],
        timer_requests: &[],
        objective_requests: &[],
        terminal_requests: &[],
    }
}

fn mission() -> ObjectiveSession {
    ObjectiveSession::launch(
        lower_program(&declared_synthetic_objectives()).expect("the fixture program lowers"),
        GEN,
    )
    .expect("the fixture mission launches")
}

fn spawn_scene_node(
    world: &mut World,
    key: &str,
    generation: SceneGeneration,
) -> bevy::ecs::entity::Entity {
    world
        .spawn((
            SceneNodeBinding {
                node: node(key),
                generation,
            },
            NodeVisualTransform(GlobalTransform::from(Mat4::from_translation(Vec3::ZERO))),
        ))
        .id()
}

/// A world that owns the animation log and the playback, with the synthetic
/// door clip bound to its node, and the clip advanced to the tick its gameplay
/// marker fires on — so the log holds exactly one fired marker for the
/// composed step to drain.
fn world_with_one_marker(session_value: u64) -> World {
    let generation = SceneGeneration::default().next();
    let mut world = World::new();
    world.insert_resource(AnimationLog::new());
    world.insert_resource(AnimationPlayback::new(session(session_value)));
    let declared = declared_synthetic_door_clip();
    let entity = spawn_scene_node(&mut world, "synthetic.hangar.door", generation);
    bind_animated_node(
        &mut world,
        &declared,
        entity,
        &node("synthetic.hangar.door"),
        instance(1),
        generation,
        Tick(0),
    )
    .expect("the production spawn path binds the clip");
    advance_animation(&mut world, Tick(SYNTHETIC_DOOR_OPEN_TICK));
    assert_eq!(
        world.resource::<AnimationLog>().events().len(),
        1,
        "the fixture world holds one fired gameplay marker"
    );
    assert_eq!(
        world.resource::<AnimationLog>().events()[0].marker,
        SYNTHETIC_DOOR_MARKER,
        "and it is the door clip's own gameplay marker"
    );
    world
}

fn retail_root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    )
}

// ---------------------------------------------------------------------------
// The record timeline.
// ---------------------------------------------------------------------------

/// A playable record starts, advances **once per committed tick**, publishes
/// each statement exactly once and finishes at its own measured duration —
/// and a record that finished may be started again as a new activation while
/// a repeated start of a running one is not a second activation.
#[test]
fn accept_t718_a_playable_record_starts_advances_once_and_finishes() {
    let row = playable_row("pzep_engines_start");
    assert!(row.is_playable(), "the fixture row joins: {row:?}");
    let mut player = MissionAnimationPlayer::new(session(71), 64)
        .expect("a nonzero tick rate cannot be refused");

    let started = player.start(NEW_GAME_START, Tick(100), std::slice::from_ref(&row));
    assert_eq!(
        started
            .started()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["pzep_engines_start"]
    );
    assert!(started.refused().is_empty());
    assert!(started.already_running().is_empty());

    // The member → actor binding the timeline carries with it: the archive and
    // member that declare this animation, and the record that stores it.
    let running = player
        .running_record("pzep_engines_start")
        .expect("the row is running");
    assert_eq!(running.archive(), "zbd/zrdr.zbd");
    assert_eq!(running.member(), "pirate_zep_nacelles.zrd");
    assert_eq!(running.carrier(), CarrierKind::Mission);
    assert_eq!(running.record_index(), 36);
    assert_eq!(running.span().container_path(), "zbd/c1c/m01/mis_anim.zbd");
    assert_eq!(running.started_at(), Tick(100));
    assert_eq!(
        running.duration_time(),
        2.5,
        "the duration is the measured end time of the record's own events"
    );
    assert_eq!(running.reached_time(), None);
    assert_eq!(running.statements(), 0);

    // The first committed tick reaches stored time 0: nothing has started yet.
    let first = player.advance(Tick(100)).expect("the first tick advances");
    assert_eq!(first.tick(), Tick(100));
    assert!(first.statements().is_empty());
    assert!(first.finished().is_empty());
    assert_eq!(
        player
            .running_record("pzep_engines_start")
            .expect("still running")
            .reached_time(),
        Some(0.0)
    );

    // A repeated committed tick publishes nothing at all (F20-C's rule).
    let repeated = player
        .advance(Tick(100))
        .expect_err("a repeated committed tick is refused");
    assert_eq!(
        repeated,
        TickAdvanceRefusal::NotAfter {
            tick: Tick(100),
            advanced_through: Some(Tick(100))
        }
    );
    assert_eq!(
        player
            .running_record("pzep_engines_start")
            .expect("still running")
            .statements(),
        0,
        "the refused tick published nothing"
    );

    // Tick 132 is stored time 0.5: the motion statement starts.
    let step = player.advance(Tick(132)).expect("the timeline advances");
    assert_eq!(step.statements().len(), 1);
    let statement = &step.statements()[0];
    assert_eq!(statement.identity(), "pzep_engines_start");
    assert_eq!(statement.archive(), "zbd/zrdr.zbd");
    assert_eq!(statement.member(), "pirate_zep_nacelles.zrd");
    assert_eq!(statement.session(), session(71));
    assert_eq!(statement.tick(), Tick(132));
    assert_eq!(statement.sequence(), 0);
    assert_eq!(statement.sequence_name(), "call_eachengine");
    assert_eq!(statement.event_index(), 0);
    assert_eq!(statement.opcode(), 11);
    assert_eq!(statement.statement(), "OBJECT_MOTION_FROM_TO");
    assert_eq!(statement.class(), EventClass::Motion);
    assert_eq!(statement.start_time(), 0.5);
    assert_eq!(statement.end_time(), 2.5);

    // Tick 196 is stored time 1.5: the call statement starts, and the motion
    // statement is not published a second time.
    let step = player.advance(Tick(196)).expect("the timeline advances");
    assert_eq!(step.statements().len(), 1);
    assert_eq!(step.statements()[0].statement(), "CALL_ANIMATION");
    assert_eq!(step.statements()[0].class(), EventClass::Control);
    assert_eq!(step.statements()[0].event_index(), 1);

    // Tick 260 is stored time 2.5: the record finishes, after its statements.
    let step = player.advance(Tick(260)).expect("the timeline advances");
    assert!(step.statements().is_empty());
    assert_eq!(step.finished().len(), 1);
    let finished = &step.finished()[0];
    assert_eq!(finished.identity(), "pzep_engines_start");
    assert_eq!(finished.member(), "pirate_zep_nacelles.zrd");
    assert_eq!(finished.duration_time(), 2.5);
    assert_eq!(finished.started_at(), Tick(100));
    assert_eq!(finished.finished_at(), Tick(260));
    assert_eq!(finished.statements(), 2);
    assert_eq!(player.running().count(), 0);
    assert_eq!(player.finished().count(), 1);

    // A finished record may be started again: that is a new activation with a
    // fresh timeline, and the finished one stays on record as its own row.
    let again = player.start(NEW_GAME_START, Tick(300), std::slice::from_ref(&row));
    assert_eq!(again.started().len(), 1);
    assert_eq!(player.running().count(), 1);
    assert_eq!(player.finished().count(), 1);

    // Starting the identity that is already running is not a second
    // activation: one timeline per live identity.
    let repeat = player.start(NEW_GAME_START, Tick(301), std::slice::from_ref(&row));
    assert_eq!(repeat.started().len(), 0);
    assert_eq!(
        repeat
            .already_running()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["pzep_engines_start"]
    );
    assert_eq!(player.running().count(), 1);
    assert_eq!(
        player
            .running_record("pzep_engines_start")
            .expect("still running")
            .started_at(),
        Tick(300),
        "the repeated start did not move the live timeline"
    );
}

/// A row the join refused is never started, whatever its bytes said: its
/// refusals — with their claim ids and the record's own source locator — are
/// kept, and advancing the player publishes nothing for it (F20
/// non-negotiable behavior 2).
#[test]
fn accept_t718_a_row_the_join_refused_is_never_started() {
    // 1. a record whose blocks hold no decoded events here.
    let absent = joined_over(
        "no_events",
        &["call_eachengine"],
        record(
            &RecordSpec {
                anim_name: "no_events",
                object_name: "piratezep",
                root_name: "piratezep",
                sequences: vec![(
                    "call_eachengine",
                    AnimationRecordSequenceKind::Sequence,
                    960,
                )],
            },
            CarrierKind::Mission,
            36,
        ),
    );
    assert!(!absent.is_playable());

    // 2. a record whose stored object name is not one of the declaration's
    // selectors: a content mismatch, refused with no claim id.
    let disagree = joined_over(
        "disagrees",
        &["call_eachengine"],
        facts_with(
            "disagrees",
            "blackswanzep",
            &[(
                "call_eachengine",
                AnimationRecordSequenceKind::Sequence,
                stream(&[synthetic_event(11, 1, 0.5, 2.0)]),
            )],
        ),
    );
    assert!(!disagree.is_playable());

    let mut player = MissionAnimationPlayer::new(session(72), 64)
        .expect("a nonzero tick rate cannot be refused");
    let rows = vec![absent, disagree];
    let report = player.start(NEW_GAME_START, Tick(0), &rows);
    assert!(
        report.started().is_empty(),
        "nothing the join refused is started"
    );
    assert_eq!(
        report
            .refused()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["no_events", "disagrees"],
        "the refused rows keep their stored order"
    );
    assert_eq!(player.running().count(), 0);

    let refused = player
        .refused_record("no_events")
        .expect("the refusal is kept");
    assert_eq!(refused.event(), NEW_GAME_START);
    assert_eq!(refused.refusals().len(), 1);
    assert_eq!(refused.refusals()[0].label(), "events_not_decoded");
    assert_eq!(refused.claim_ids(), vec![EVENTS_NOT_DECODED_CLAIM]);
    assert_eq!(
        refused
            .span()
            .expect("a bound record keeps its source locator")
            .container_path(),
        "zbd/c1c/m01/mis_anim.zbd"
    );

    let disagree = player
        .refused_record("disagrees")
        .expect("the refusal is kept");
    assert_eq!(disagree.refusals()[0].label(), "object_name_disagrees");
    assert!(
        disagree.claim_ids().is_empty(),
        "a content mismatch is read content, not an unmeasured claim: {:?}",
        disagree.claim_ids()
    );

    // Advancing publishes nothing for a refused row, however far the timeline
    // runs.
    for tick in 1..=64_u64 {
        let step = player.advance(Tick(tick)).expect("the timeline advances");
        assert!(step.statements().is_empty());
        assert!(step.finished().is_empty());
    }
    assert_eq!(player.running().count(), 0);
    assert_eq!(player.refused().count(), 2);
}

/// The two host-contract refusals: a player that could never advance, and a
/// generation that cannot be retried onto the mission it already serves.
#[test]
fn accept_t718_a_zero_tick_rate_and_a_repeated_generation_are_refused() {
    let first = session(73);
    let zero = MissionAnimationPlayer::new(first, 0)
        .expect_err("a tick rate of zero cannot advance anything");
    assert_eq!(zero, PlayerError::ZeroTickRate);
    assert!(!zero.to_string().is_empty());

    let mut player = MissionAnimationPlayer::new(first, 64).expect("a usable rate");
    let _ = player.start(
        NEW_GAME_START,
        Tick(0),
        &[playable_row("pzep_engines_start")],
    );
    player.advance(Tick(1)).expect("the first tick advances");
    assert_eq!(player.running().count(), 1);

    let same = player
        .retry(first)
        .expect_err("the generation the player already serves cannot be retried");
    assert_eq!(same, PlayerError::SameSession { served: first });
    assert_eq!(
        player.running().count(),
        1,
        "a refused retry leaves the live timeline untouched"
    );
    assert_eq!(player.advanced_through(), Some(Tick(1)));

    let teardown = player
        .retry(session(74))
        .expect("a new generation starts a new timeline");
    assert_eq!(
        teardown,
        PlayerTeardown {
            session: session(74),
            running: 1,
            refused: 0,
            finished: 0,
        }
    );
    assert_eq!(player.served(), session(74));
    assert_eq!(player.running().count(), 0);
    assert_eq!(player.advanced_through(), None);
    let restarted = player
        .advance(Tick(1))
        .expect("the new generation's timeline starts over");
    assert_eq!(restarted.tick(), Tick(1));
    assert!(restarted.statements().is_empty());
}

// ---------------------------------------------------------------------------
// The composed step: the animation log into the mission session.
// ---------------------------------------------------------------------------

/// The composed tick does both halves for one committed tick: the animation
/// log's fired gameplay marker becomes one mission signal in the objective
/// runtime, and the mission's own record publishes the statements its timeline
/// reaches — with the log drained exactly once.
#[test]
fn accept_t718_the_composed_step_raises_the_animation_log_and_advances_the_records() {
    let mut world = world_with_one_marker(41);
    let mut player = MissionAnimationPlayer::new(session(41), 64)
        .expect("a nonzero tick rate cannot be refused");
    let _ = player.start(
        NEW_GAME_START,
        Tick(0),
        &[playable_row("pzep_engines_start")],
    );
    let mut markers = MissionMarkerConsumer::new(session(41), bound_cues(REACHED_WRECK));
    let mut objectives = mission();

    let step = step_mission_animations(
        &mut world,
        &mut player,
        &mut markers,
        &mut objectives,
        &facts(1, 1),
    )
    .expect("both halves accept the first tick");

    assert_eq!(
        step.mission.markers.drained(),
        1,
        "the log was drained once"
    );
    assert_eq!(step.mission.markers.raised().len(), 1);
    let raised = &step.mission.markers.raised()[0];
    assert_eq!(raised.cue, DOOR_CUE);
    assert_eq!(raised.signal, REACHED_WRECK);
    assert!(
        step.mission.markers.refusals().is_empty(),
        "a bound, live marker is not refused: {:?}",
        step.mission.markers.refusals()
    );
    assert_eq!(
        world.resource::<AnimationLog>().events().len(),
        0,
        "the composed step drained the log"
    );

    assert_eq!(step.records.tick(), Tick(1));
    assert!(
        step.records.statements().is_empty(),
        "stored time 0.5 is not reached at tick 1 of a 64 Hz timeline"
    );
    assert_eq!(
        player.advanced_through(),
        Some(Tick(1)),
        "the record half ran"
    );

    // The next committed tick: the record's own statement reaches stored time
    // 0.5, and the log stays drained however often the mission is stepped.
    let step = step_mission_animations(
        &mut world,
        &mut player,
        &mut markers,
        &mut objectives,
        &facts(33, 1),
    )
    .expect("both halves accept the second tick");
    assert_eq!(step.mission.markers.drained(), 0);
    assert!(step.mission.markers.raised().is_empty());
    assert_eq!(step.records.statements().len(), 1);
    assert_eq!(
        step.records.statements()[0].statement(),
        "OBJECT_MOTION_FROM_TO"
    );
    assert_eq!(step.records.statements()[0].session(), session(41));
}

/// The order of the halves, as a refusal: the objective session refuses a tick
/// it has already stepped, and the record half has **already** advanced that
/// tick — its report is carried in the refusal so the tick's statements stay
/// with the host, and the retry is the mission half alone.
#[test]
fn accept_t718_a_refused_mission_tick_carries_the_record_report() {
    let mut world = world_with_one_marker(42);
    let mut player = MissionAnimationPlayer::new(session(42), 64)
        .expect("a nonzero tick rate cannot be refused");
    let _ = player.start(
        NEW_GAME_START,
        Tick(0),
        &[playable_row("pzep_engines_start")],
    );
    let mut markers = MissionMarkerConsumer::new(session(42), bound_cues(REACHED_WRECK));
    let mut objectives = mission();

    // The host's documented retry shape: the mission half alone, with the
    // delivery the first refusal carried.
    step_mission_with_markers(&mut world, &mut markers, &mut objectives, &facts(1, 1))
        .expect("the objective session accepts its first tick");

    let refusal = step_mission_animations(
        &mut world,
        &mut player,
        &mut markers,
        &mut objectives,
        &facts(1, 1),
    )
    .expect_err("the objective session refuses a tick it already stepped");
    let (records, mission) = match refusal {
        MissionAnimationStepRefusal::Mission(half) => (half.records, half.refusal),
        other => panic!("the mission half refused, not the record half: {other:?}"),
    };
    assert_eq!(records.tick(), Tick(1));
    assert!(records.statements().is_empty());
    assert!(
        mission.markers.raised().is_empty(),
        "the log was already drained by the first pass"
    );
    assert_eq!(
        player.advanced_through(),
        Some(Tick(1)),
        "the record half ran before the mission half refused — a retry must not \
         offer this tick to the player again"
    );
}

/// The other refusal: a player that is ahead of the mission session refuses
/// **before** the mission half runs, so the animation log is not drained by a
/// tick that could not be completed.
#[test]
fn accept_t718_a_player_ahead_of_the_mission_refuses_before_the_mission_runs() {
    let mut world = world_with_one_marker(43);
    let mut player = MissionAnimationPlayer::new(session(43), 64)
        .expect("a nonzero tick rate cannot be refused");
    let _ = player.start(
        NEW_GAME_START,
        Tick(0),
        &[playable_row("pzep_engines_start")],
    );
    let mut markers = MissionMarkerConsumer::new(session(43), bound_cues(REACHED_WRECK));
    let mut objectives = mission();

    // The player is stepped on its own first, so it is ahead of the mission.
    player.advance(Tick(5)).expect("the player's own tick");

    let refusal = step_mission_animations(
        &mut world,
        &mut player,
        &mut markers,
        &mut objectives,
        &facts(1, 1),
    )
    .expect_err("the player is already through tick 1");
    let (tick, advanced_through) = match &refusal {
        MissionAnimationStepRefusal::Records(TickAdvanceRefusal::NotAfter {
            tick,
            advanced_through,
        }) => (*tick, *advanced_through),
        other => panic!("the record half refused before the mission ran: {other:?}"),
    };
    assert_eq!(tick, Tick(1));
    assert_eq!(advanced_through, Some(Tick(5)));
    assert_eq!(
        world.resource::<AnimationLog>().events().len(),
        1,
        "the mission half never ran, so the log still holds its marker"
    );
    assert!(
        markers.applied().is_empty(),
        "no marker was admitted by a refused tick"
    );
}

// ---------------------------------------------------------------------------
// Retail: M01's own startup rows, played through the consumer.
// ---------------------------------------------------------------------------

/// **M01's startup animations play in the mission session**: all six
/// `NEW_GAME_START` rows of the closure start with the archive and member that
/// declares each one, every statement they reach is stamped with the session
/// and the member, every row finishes at its own measured duration, nothing is
/// refused — and the mission's own `placezeps.zrd` placements stay refused to
/// place under their claim, because this consumer spawns nothing.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t718_retail_m01_startup_animations_play_in_the_mission_session() {
    let binding = bind_mission_animation(&retail_root(), M01).expect("M01 binds");
    let run = binding.run(NEW_GAME_START);
    assert_eq!(run.len(), 6, "M01's six NEW_GAME_START identities");
    assert_eq!(
        run.playable().count(),
        6,
        "every row decoded after #690 measured the event grammar"
    );

    let mut player = MissionAnimationPlayer::new(session(91), 64)
        .expect("a nonzero tick rate cannot be refused");
    let started = player.start(NEW_GAME_START, Tick(0), run.rows());
    assert_eq!(started.started().len(), 6);
    assert!(
        started.refused().is_empty(),
        "nothing in M01's startup set is refused: {:?}",
        started.refused()
    );
    assert_eq!(player.running().count(), 6);
    assert_eq!(player.refused().count(), 0);

    // The member → actor binding, carried by the running records.
    let located: Vec<(&str, &str, &str)> = player
        .running()
        .map(|record| (record.identity(), record.archive(), record.member()))
        .collect();
    assert!(
        located.contains(&(
            "pzep_engines_start",
            "zbd/zrdr.zbd",
            "pirate_zep_nacelles.zrd"
        )),
        "the shared root's zeppelin member drives pzep_engines_start: {located:?}"
    );
    assert!(
        located.contains(&("wv_hookup_state", "zbd/c1c/m01/zrdr.zbd", "wv_tailhook.zrd")),
        "the mission's own tailhook member drives wv_hookup_state: {located:?}"
    );
    let mission_carrier = player
        .running()
        .filter(|record| record.carrier() == CarrierKind::Mission)
        .count();
    let camera_carrier = player
        .running()
        .filter(|record| record.carrier() == CarrierKind::Camera)
        .count();
    assert!(
        mission_carrier > 0 && camera_carrier > 0,
        "M01 starts rows of both carriers: {mission_carrier} mission, {camera_carrier} camera"
    );

    // Advance the whole closure until every row reached the end of its own
    // measured duration.
    let mut published: BTreeSet<String> = BTreeSet::new();
    let mut statements = 0_usize;
    let mut tick = 0_u64;
    while player.running_count() > 0 {
        assert!(
            tick <= 100_000,
            "M01's startup rows must finish inside the measured durations"
        );
        let report = player.advance(Tick(tick)).expect("the timeline advances");
        for statement in report.statements() {
            assert_eq!(statement.session(), session(91));
            assert!(!statement.statement().is_empty());
            assert!(!statement.member().is_empty());
            published.insert(statement.identity().to_owned());
            statements += 1;
        }
        for finished in report.finished() {
            let row = run
                .rows()
                .iter()
                .find(|row| row.identity() == finished.identity())
                .expect("a finished identity is one of the startup rows");
            assert_eq!(
                finished.duration_time(),
                row.playback()
                    .expect("a started row decoded")
                    .duration_time(),
                "the finished duration is the record's own measured duration"
            );
        }
        tick += 1;
    }
    assert_eq!(player.running().count(), 0);
    assert_eq!(player.finished().count(), 6, "every startup row finished");
    assert!(
        statements > 0,
        "M01's startup animations publish their statements"
    );
    assert!(
        published.contains("generic_intro"),
        "the camera animation's statements reached the session: {published:?}"
    );
    assert!(
        published.contains("pzep_engines_start"),
        "the mission animation's statements reached the session: {published:?}"
    );
    assert_eq!(player.refused().count(), 0);

    // The other startup event of the same binding, on its own player: one row,
    // started and finished the same way.
    let load = binding.run(LOAD_GAME_START);
    assert_eq!(load.len(), 1);
    let mut load_player = MissionAnimationPlayer::new(session(92), 64)
        .expect("a nonzero tick rate cannot be refused");
    let started = load_player.start(LOAD_GAME_START, Tick(0), load.rows());
    assert_eq!(
        started
            .started()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["player_setup"]
    );
    let mut tick = 0_u64;
    while load_player.running_count() > 0 {
        assert!(tick <= 100_000, "player_setup has a measured duration");
        load_player
            .advance(Tick(tick))
            .expect("the timeline advances");
        tick += 1;
    }
    assert_eq!(load_player.finished_count(), 1);

    // The world actors the mission's own archive places: identity measured,
    // placement still refused under its own claim — this consumer starts no
    // record and spawns no actor for them.
    let placements = binding.placements();
    assert_eq!(placements.len(), 3, "M01 places three capital ships");
    for placement in placements {
        assert_eq!(placement.member(), "placezeps.zrd");
        assert_eq!(placement.archive(), "zbd/c1c/m01/zrdr.zbd");
        assert_eq!(placement.claim_id().as_str(), PLACEMENT_FIELDS_CLAIM);
        assert_eq!(placement.unplaced_reason(), PLACEMENT_FIELDS_REASON);
        assert_eq!(
            placement
                .targets()
                .first()
                .and_then(|target| target.resolution().occurrences()),
            Some(1),
            "each placed actor names exactly one world record"
        );
    }
}
