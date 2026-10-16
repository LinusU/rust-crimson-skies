//! Acceptance stage M01-PLAY-TRIGGERS-ANIMS-LAND (Rally #1498), the landing
//! half of parent #1482.
//!
//! Four things have to hold, and each is proved by a member of this suite:
//!
//! * **The measured table is read verbatim.** The world-group reader
//!   `zbd/c1c/zrdr.zbd :: landings.zrd` is read through production
//!   discovery and the production `.zrd` grammar; every row's `anim`,
//!   `node`, `angle`, `speed` and `auto` come out as the member spelled
//!   them, and any shape the grammar cannot read is refused **by name**
//!   while its readable siblings still parse. On a fixture installation
//!   that declares no landing member the reader answers the absence by
//!   name instead of an empty guess.
//! * **The landing rule is pinned.** The designed detection bounds and the
//!   designed window reading (`LANDING_DETECTION_CLAIM`,
//!   `LANDING_WINDOW_CLAIM`) are pure functions, and their boundaries are
//!   asserted directly so a drift fails a test rather than a playthrough.
//! * **A landing inside the window runs the row's record and its measured
//!   calls, and the gated state appears.** On the composed synthetic stage
//!   (CI) and on the composed retail M01 (an ignored member), a body at an
//!   armed landing node with the row's window satisfied starts the row's
//!   record through the production player, starts the names of its
//!   measured `animation_refs` through the same path
//!   (`LANDING_CALL_CHAIN_CLAIM`), and the record player's state — the
//!   state `ANIM_STATE` gates read — appears through the host's stage-4
//!   fold, with the real program's own gate block completing.
//! * **Nothing else runs anything.** No landing, a landing outside the
//!   window, and a landing at a node whose vocabulary no `OBJECT_ACTIVE_STATE`
//!   statement has recorded each leave the ledger empty — fail-closed, not
//!   a refusal.
//!
//! What stays unknown is written in
//! `docs/findings/2026-10-16-m01-play-triggers-anims-land.md`: the original
//! landing geometry, the meaning of the table's `angle`/`speed` units, the
//! `pz` nodes' arming circularity under the designed arming surface, and
//! whether the original tied a record's calls to their statements. Nothing
//! here is `verified_original`; no original run exists.

use std::path::Path;

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::{App, Quat, Vec3, With};
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::animation::events::{EVENT_HEADER_BYTES, decode_event_stream};
use cs_app::animation::mission::{
    AnimationRecordFacts, AuthoredMissionAnimations, CarrierFact, CarrierRecords,
    DECLARATION_MATCH_CLAIM, MissionAnimationBinding, RecordSequence, SequenceEvents,
};
use cs_app::animation::programs::{
    ANIMATION_DEFINITION_FIELD, ANIMATION_DEFINITIONS_RECORD, ANIMATION_LIST_FIELD,
    ANIMATION_NAME_FIELD, AnimationDefinitionSite, NAME_FIELD, SEQUENCE_FIELD, SEQUENCE_NAME_FIELD,
    read_animation_definition_member,
};
use cs_app::animation::survey::CarrierKind;
use cs_app::mission_animations::ActivationOutcome;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    LANDING_RADIUS_M, LANDING_SINK_RATE_MPS, LandingNodeEntry, LandingReadError, LandingRow,
    MissionHost, MissionHostReport, MissionHostTick, MissionLandings, MissionPlayerBody,
    MissionStage, PLAYER_MEMBER, build_headless, landing_reached, parse_landings, read_landings,
    stage_for, teardown, window_satisfied,
};
use cs_app::world_facts::member_chain;
use cs_content::stunts::ZrdValue;
use cs_formats::zbd::AnimationRecordSequenceKind;
use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId, Value,
};
use cs_script::runtime::ObjectiveLifecycle;
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

use crate::common::{cid, label};
use crate::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

// ---------------------------------------------------------------------------
// The synthetic stage: five records, five declarations, one wake, and an
// authored landing table. Every name below is synthetic content, never a
// reading of any original mission's record.
// ---------------------------------------------------------------------------

/// The record whose `OBJECT_ACTIVE_STATE` statement arms the landing nodes.
const SYNTHETIC_ARM: &str = "synthetic_arm_anim";

/// The record a landing at `synthetic_landing_node` runs.
const SYNTHETIC_LAND: &str = "synthetic_land_anim";

/// A record a landing never reaches: its node is never armed.
const SYNTHETIC_OTHER: &str = "synthetic_other_anim";

/// A record a landing never reaches: its node is ten kilometres away.
const SYNTHETIC_FAR: &str = "synthetic_far_anim";

/// The record [`SYNTHETIC_LAND`]'s `animation_refs` chain starts.
const SYNTHETIC_CALLED: &str = "synthetic_called_anim";

/// A name in [`SYNTHETIC_LAND`]'s refs that no member declares: the chain's
/// refusal leg.
const SYNTHETIC_CHAIN_UNBOUND: &str = "synthetic_chain_binds_nothing";

/// The one ordinary sequence block every synthetic record carries.
const SYNTHETIC_SEQUENCE: &str = "PLAY";

/// The carrier every synthetic record lives in.
const SYNTHETIC_CARRIER: &str = "zbd/synthetic/mis_anim.zbd";

// ---------------------------------------------------------------------------
// Synthetic `.zrd` authoring, in the measured grammar the definition reader
// accepts (the same shapes `m01_play_triggers_anims_arm.rs` authors).
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
    ContentHash::from_bytes([9u8; 32])
}

/// One synthetic event: the measured eight-byte tag/length header, a payload
/// whose word `0` is `start` and whose last word is `run`, and the byte length
/// each measured opcode's payload has.
fn synthetic_event(opcode: u8, group: u8, start: f32, run: f32) -> Vec<u8> {
    let payload_len = match opcode {
        6 => 12,
        11 => 136,
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

/// One synthetic record with its own nodes, refs and one ordinary sequence.
fn synthetic_record(
    index: usize,
    name: &str,
    nodes: Vec<String>,
    animation_refs: Vec<String>,
    events: Vec<u8>,
) -> AnimationRecordFacts {
    AnimationRecordFacts {
        carrier: CarrierKind::Mission,
        carrier_key: SYNTHETIC_CARRIER.to_owned(),
        index,
        span: SourceSpan::new(
            synthetic_hash(),
            SYNTHETIC_CARRIER,
            None,
            u64::try_from(index * 2_048).expect("a small offset"),
            672,
            None,
        )
        .expect("the synthetic span is valid"),
        provenance: synthetic_provenance(DECLARATION_MATCH_CLAIM),
        anim_name: name.to_owned(),
        object_name: "synthetic_actor".to_owned(),
        root_name: "synthetic_actor".to_owned(),
        flags: 0x0044_48B0,
        status: 0,
        activation: 3,
        execution_priority: 4,
        reset_time: -1.0,
        max_health: 0.0,
        objects: Vec::new(),
        nodes,
        animation_refs,
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

/// The declaration of one synthetic name, in the measured `.zrd` shape,
/// read through the production definition reader.
fn synthetic_declaration(name: &str) -> AnimationDefinitionSite {
    let member = animation_definition_member(vec![zrd_flat(vec![
        (ANIMATION_NAME_FIELD, zrd_text(name)),
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
    let records = vec![
        // The arm record: its `OBJECT_ACTIVE_STATE` statement carries the
        // two armed node names a landing row and the far row name.
        synthetic_record(
            0,
            SYNTHETIC_ARM,
            vec![
                String::new(),
                "synthetic_landing_node".to_owned(),
                "synthetic_far_node".to_owned(),
            ],
            Vec::new(),
            synthetic_event(6, 1, 0.0, 0.0),
        ),
        // The landing row's record: its measured refs chain starts the
        // called record and refuses the unbindable name.
        synthetic_record(
            1,
            SYNTHETIC_LAND,
            Vec::new(),
            vec![
                SYNTHETIC_CALLED.to_owned(),
                SYNTHETIC_CHAIN_UNBOUND.to_owned(),
            ],
            synthetic_event(11, 1, 0.0, 1.0),
        ),
        synthetic_record(
            2,
            SYNTHETIC_OTHER,
            Vec::new(),
            Vec::new(),
            synthetic_event(11, 1, 0.0, 1.0),
        ),
        synthetic_record(
            3,
            SYNTHETIC_FAR,
            Vec::new(),
            Vec::new(),
            synthetic_event(11, 1, 0.0, 1.0),
        ),
        synthetic_record(
            4,
            SYNTHETIC_CALLED,
            Vec::new(),
            Vec::new(),
            synthetic_event(11, 1, 0.0, 1.0),
        ),
    ];
    MissionAnimationBinding::authored(AuthoredMissionAnimations {
        scope: "zbd/synthetic/m01".to_owned(),
        group: "synthetic".to_owned(),
        world_container: "zbd/synthetic/gamez.zbd".to_owned(),
        archives: vec!["zbd/synthetic/zrdr.zbd".to_owned()],
        carriers: vec![CarrierFact {
            kind: CarrierKind::Mission,
            container_key: SYNTHETIC_CARRIER.to_owned(),
            record_count: 5,
            declared_record_count: 5,
            blockers: Vec::new(),
        }],
        records: vec![CarrierRecords::authored(
            CarrierKind::Mission,
            SYNTHETIC_CARRIER.to_owned(),
            records,
        )],
        declarations: vec![
            synthetic_declaration(SYNTHETIC_ARM),
            synthetic_declaration(SYNTHETIC_LAND),
            synthetic_declaration(SYNTHETIC_OTHER),
            synthetic_declaration(SYNTHETIC_FAR),
            synthetic_declaration(SYNTHETIC_CALLED),
        ],
        startup: Vec::new(),
        placements: Vec::new(),
        measured_placements: Vec::new(),
        world: None,
    })
    .expect("the authored binding assembles")
}

/// The synthetic control program: one objective that completes on its first
/// evaluated tick and wakes the arm record — the designed arming path the
/// landing then consumes.
fn synthetic_wake_program() -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-mission-anim-land"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(ContentKind::Objective, "synthetic-land-wake"),
            condition: Condition::Const(true),
            actions: vec![Action::Directive {
                operation: DirectiveOperation::WakeAnimation,
                args: vec![Value::Str(SYNTHETIC_ARM.to_owned())],
            }],
            span: None,
        }],
    }
}

/// The synthetic stage over the production harbor world whose control
/// program wakes the arm record and whose animation join resolves every
/// name the landing table and the call chain touch. Everything else is
/// `synthetic_stage`'s own content.
fn synthetic_land_stage(scratch: &Path) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.host.control = synthetic_wake_program();
    stage.host.animation = Some(synthetic_binding());
    stage
}

/// The authored landing table: two rows at nodes the player's own row
/// anchors to (so the observation composes their positions from the body
/// the test drives), one armed node ten kilometres away, and one row whose
/// `speed` arity the designed window cannot read.
///
/// Anchoring on `PLAYER_MEMBER` is synthetic content exercising the
/// production join — the writer composes each node's position from its
/// anchor's observed row plus the authored offset
/// (`LANDING_NODE_POSITION_CLAIM`) — while the retail members below anchor
/// on the real bound zeppelins.
fn synthetic_landings() -> MissionLandings {
    MissionLandings {
        rows: vec![
            // The success row: armed, at the body, window satisfiable.
            LandingRow::authored(
                0,
                SYNTHETIC_LAND,
                "synthetic_landing_node",
                &[35.0],
                &[50.0, 220.0],
                false,
            ),
            // Its node never appears in any armed vocabulary.
            LandingRow::authored(
                1,
                SYNTHETIC_OTHER,
                "synthetic_never_armed",
                &[35.0],
                &[30.0, 40.0],
                false,
            ),
            // Armed and satisfiable, but ten kilometres from the body: the
            // radius rule alone stops it.
            LandingRow::authored(
                2,
                SYNTHETIC_FAR,
                "synthetic_far_node",
                &[35.0],
                &[50.0, 220.0],
                false,
            ),
            // Armed and at the body, but a `speed` window the designed
            // reading refuses by name.
            LandingRow::authored(
                3,
                SYNTHETIC_OTHER,
                "synthetic_landing_node",
                &[35.0],
                &[50.0, 60.0, 70.0],
                false,
            ),
        ],
        nodes: vec![
            LandingNodeEntry {
                node: "synthetic_landing_node".to_owned(),
                anchor: PLAYER_MEMBER.to_owned(),
                offset: [0.0, 0.0, 0.0],
            },
            LandingNodeEntry {
                node: "synthetic_never_armed".to_owned(),
                anchor: PLAYER_MEMBER.to_owned(),
                offset: [0.0, 0.0, 0.0],
            },
            LandingNodeEntry {
                node: "synthetic_far_node".to_owned(),
                anchor: PLAYER_MEMBER.to_owned(),
                offset: [10_000.0, 0.0, 0.0],
            },
        ],
        refusals: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Composed-run helpers.
// ---------------------------------------------------------------------------

/// One leg's player-body state: an optional position write, and the
/// velocity and attitude the physics step of that tick starts from.
struct Body {
    /// When `Some`, the body's position is written too — a retail leg
    /// landing at a composed node. A synthetic leg never writes it: its
    /// nodes anchor on the player's own row, and a position write to an
    /// arbitrary point would slam the body into the world's geometry.
    position: Option<Vec3>,
    /// The velocity the next physics tick integrates from.
    velocity: Vec3,
    /// The attitude the next physics tick integrates from.
    rotation: Quat,
}

/// Commits `frames` composed frames, re-placing the player body (when the
/// leg wants it placed) before each one so the physics step of that very
/// tick starts from the leg's own state.
fn pump(app: &mut App, frames: usize, place: Option<&Body>) {
    for _ in 0..frames {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(app.world().resource::<Time<Fixed>>().timestep());
        if let Some(body) = place {
            place_player(app, body.position, body.velocity, body.rotation);
        }
        app.update();
    }
}

/// Writes the player body's pose, velocity and attitude — the very
/// components the physics step commits and the stage-4 observation reads.
///
/// The body spawns in the composition's `Startup` schedule, so the very
/// first frame's placement finds nothing and skips: every frame after it
/// has exactly one body, and the legs' assertions prove the states took.
fn place_player(app: &mut App, position: Option<Vec3>, velocity: Vec3, rotation: Quat) {
    let world = app.world_mut();
    let mut query = world.query_filtered::<(
        &mut Position,
        &mut LinearVelocity,
        &mut Rotation,
    ), With<MissionPlayerBody>>();
    let Some((mut body_position, mut linear, mut spin)) = query.iter_mut(world).next() else {
        return;
    };
    if let Some(position) = position {
        *body_position = Position(position);
    }
    *linear = LinearVelocity(velocity);
    *spin = Rotation(rotation);
}

/// Whether any armed node vocabulary carries this node name.
fn armed_has(app: &App, node: &str) -> bool {
    app.world()
        .resource::<MissionHost>()
        .armed_nodes()
        .iter()
        .any(|armed| armed.nodes().iter().any(|name| name == node))
}

/// Commits frames until the armed set carries `node`, or panics naming it.
///
/// `place` is applied to every one of those frames: a synthetic stage whose
/// node anchors on the player's own row must hold a state that **cannot**
/// land (a too-fast descent here) while the wake records run, or the first
/// armed tick would already satisfy a row.
fn pump_until_armed(app: &mut App, node: &str, limit: usize, place: Option<&Body>) {
    for _ in 0..limit {
        pump(app, 1, place);
        if armed_has(app, node) {
            return;
        }
    }
    panic!("no armed node vocabulary carried {node} within {limit} frames");
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

/// The composed landing position of one node — the stage-4 observation's
/// own row for it, which is also the proof the writer composed one.
fn composed_node_position(app: &App, node: &str) -> Vec3 {
    app.world()
        .resource::<MissionHost>()
        .world_facts()
        .observation()
        .members()
        .get(&member_chain([node]))
        .map(|row| {
            let [x, y, z] = row.position;
            Vec3::new(x as f32, y as f32, z as f32)
        })
        .unwrap_or_else(|| panic!("the landing node {node} was composed a position"))
}

// ---------------------------------------------------------------------------
// The measured table: shapes, refusals, absence (runs in CI).
// ---------------------------------------------------------------------------

fn zv_text(text: &str) -> ZrdValue {
    ZrdValue::Text(text.to_owned())
}

fn zv_list(items: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(items)
}

fn zv_numbers(values: &[u32]) -> ZrdValue {
    ZrdValue::List(values.iter().copied().map(ZrdValue::Int).collect())
}

fn zv_flat(entries: Vec<(&str, ZrdValue)>) -> ZrdValue {
    let mut cells = Vec::with_capacity(entries.len() * 2);
    for (key, value) in entries {
        cells.push(zv_text(key));
        cells.push(value);
    }
    ZrdValue::List(cells)
}

fn member_span() -> SourceSpan {
    SourceSpan::new(
        synthetic_hash(),
        "zbd/synthetic/zrdr.zbd",
        Some("landings.zrd"),
        128,
        256,
        None,
    )
    .expect("the synthetic member span is valid")
}

/// **The parser reads the measured row shapes verbatim, at any nesting, and
/// keeps each row's member span.**
#[test]
fn accept_m01_play_triggers_anims_land_the_parser_reads_the_measured_row_shapes_verbatim() {
    let measured = zv_flat(vec![
        ("anim", zv_list(vec![zv_text("wv_initiate_hookup")])),
        ("node", zv_list(vec![zv_text("wv_manual_land")])),
        ("angle", zv_numbers(&[35])),
        ("speed", zv_numbers(&[50, 220])),
    ]);
    let automatic = zv_flat(vec![
        ("anim", zv_list(vec![zv_text("hooked_to_klondike")])),
        ("node", zv_list(vec![zv_text("pz_auto_land")])),
        ("auto", zv_list(Vec::new())),
    ]);
    let span = member_span();

    // The document may hold its rows directly or behind one wrapper level,
    // the two shapes the retail corpus stores them in.
    for document in [
        zv_list(vec![measured.clone(), automatic.clone()]),
        zv_list(vec![zv_list(vec![measured.clone(), automatic.clone()])]),
    ] {
        let parsed = parse_landings(&document, &span).expect("the measured shapes parse");
        assert!(
            parsed.refusals.is_empty(),
            "no measured shape is refused: {:?}",
            parsed.refusals
        );
        assert_eq!(parsed.rows.len(), 2, "both rows are read");
        let row = &parsed.rows[0];
        assert_eq!(row.row, 0);
        assert_eq!(row.anim, "wv_initiate_hookup");
        assert_eq!(row.node, "wv_manual_land");
        assert_eq!(row.angle, vec![35.0]);
        assert_eq!(row.speed, vec![50.0, 220.0]);
        assert!(!row.auto);
        assert_eq!(
            row.span.as_ref(),
            Some(&span),
            "the row carries its member span"
        );
        let row = &parsed.rows[1];
        assert_eq!(row.anim, "hooked_to_klondike");
        assert_eq!(row.node, "pz_auto_land");
        assert!(row.auto, "the auto key's presence is the flag");
        assert!(row.angle.is_empty() && row.speed.is_empty());
    }

    // One flat row as the whole document is a one-row table.
    let parsed = parse_landings(&measured, &span).expect("a single row parses");
    assert_eq!(parsed.rows.len(), 1);
    assert!(parsed.refusals.is_empty());
}

/// **A shape the reader cannot read is refused by name, and the readable
/// sibling row still parses — no defaults, no guessed arity.**
#[test]
fn accept_m01_play_triggers_anims_land_an_unreadable_row_is_refused_by_name() {
    let good = zv_flat(vec![
        ("anim", zv_list(vec![zv_text("wv_initiate_hookup")])),
        ("node", zv_list(vec![zv_text("wv_manual_land")])),
        ("speed", zv_numbers(&[50, 220])),
    ]);
    let unknown_key = zv_flat(vec![
        ("anim", zv_list(vec![zv_text("hooked_to_klondike")])),
        ("node", zv_list(vec![zv_text("pz_manual_land")])),
        ("mystery", zv_numbers(&[1])),
    ]);
    let anim_not_text = zv_flat(vec![
        ("anim", zv_numbers(&[7])),
        ("node", zv_list(vec![zv_text("pz_manual_land")])),
    ]);
    let duplicated = zv_flat(vec![
        ("anim", zv_list(vec![zv_text("hooked_to_klondike")])),
        ("anim", zv_list(vec![zv_text("hooked_to_klondike")])),
        ("node", zv_list(vec![zv_text("pz_manual_land")])),
    ]);
    let speed_not_numbers = zv_flat(vec![
        ("anim", zv_list(vec![zv_text("wv_initiate_hookup")])),
        ("node", zv_list(vec![zv_text("wv_manual_land")])),
        ("speed", zv_list(vec![zv_text("fast"), zv_text("faster")])),
    ]);
    let document = zv_list(vec![
        good,
        unknown_key,
        anim_not_text,
        duplicated,
        speed_not_numbers,
    ]);
    let parsed = parse_landings(&document, &member_span()).expect("the document is a table");
    assert_eq!(parsed.rows.len(), 1, "only the good row parses");
    assert_eq!(parsed.rows[0].anim, "wv_initiate_hookup");
    assert_eq!(
        parsed.refusals.len(),
        4,
        "four named refusals: {:?}",
        parsed.refusals
    );
    assert!(
        parsed.refusals[0].contains("row 1") && parsed.refusals[0].contains("mystery"),
        "the unknown key is named: {}",
        parsed.refusals[0]
    );
    assert!(
        parsed.refusals[1].contains("row 2") && parsed.refusals[1].contains("not exactly one text"),
        "the non-text anim is named: {}",
        parsed.refusals[1]
    );
    assert!(
        parsed.refusals[2].contains("row 3") && parsed.refusals[2].contains("appears twice"),
        "the duplicate key is named: {}",
        parsed.refusals[2]
    );
    assert!(
        parsed.refusals[3].contains("row 4")
            && parsed.refusals[3].contains("not a list of numbers"),
        "the non-numeric speed is named: {}",
        parsed.refusals[3]
    );
}

/// **An installation that declares no landing member is refused by name —
/// the absence is the answer, never an empty table read as a measurement.**
#[test]
fn accept_m01_play_triggers_anims_land_an_absent_archive_is_refused_by_name() {
    let scratch = Scratch::new("landings-absent");
    let error = read_landings(scratch.path(), "zbd/synthetic")
        .expect_err("the fixture installation declares no such archive");
    assert!(
        matches!(
            &error,
            LandingReadError::ArchiveAbsent { archive } if archive == "zbd/synthetic/zrdr.zbd"
        ),
        "the missing archive is named, got {error}"
    );
}

/// **The designed landing rule and the designed window are pinned at their
/// boundaries.** Both are labelled designed
/// ([`LANDING_DETECTION_CLAIM`], [`LANDING_WINDOW_CLAIM`]); this member
/// makes a drift in either a test failure instead of a silent behaviour
/// change.
#[test]
fn accept_m01_play_triggers_anims_land_the_designed_detection_and_window_are_pinned() {
    // Detection: at the node and level → landed; at the radius → landed;
    // past it → not; at the sink bound → landed; past it → not.
    assert!(landing_reached([0.0; 3], [0.0; 3], 0.0));
    assert!(landing_reached([LANDING_RADIUS_M, 0.0, 0.0], [0.0; 3], 0.0));
    assert!(!landing_reached(
        [LANDING_RADIUS_M + 1.0, 0.0, 0.0],
        [0.0; 3],
        0.0
    ));
    assert!(landing_reached([0.0; 3], [0.0; 3], LANDING_SINK_RATE_MPS));
    assert!(!landing_reached(
        [0.0; 3],
        [0.0; 3],
        LANDING_SINK_RATE_MPS + 1.0
    ));

    let row = LandingRow::authored(0, "anim", "node", &[35.0], &[50.0, 220.0], false);
    assert_eq!(window_satisfied(&row, 60.0, 0.0), Ok(true));
    assert_eq!(
        window_satisfied(&row, 50.0, 35.0),
        Ok(true),
        "bounds are inclusive"
    );
    assert_eq!(
        window_satisfied(&row, 300.0, 0.0),
        Ok(false),
        "speed outside"
    );
    assert_eq!(
        window_satisfied(&row, 30.0, 0.0),
        Ok(false),
        "speed outside"
    );
    assert_eq!(
        window_satisfied(&row, 60.0, 40.0),
        Ok(false),
        "tilt outside"
    );
    let automatic = LandingRow::authored(0, "anim", "node", &[], &[], true);
    assert_eq!(
        window_satisfied(&automatic, 900.0, 80.0),
        Ok(true),
        "an auto row admits any landing its detection reaches"
    );
    let malformed = LandingRow::authored(0, "anim", "node", &[35.0], &[50.0, 60.0, 70.0], false);
    let refusal = window_satisfied(&malformed, 60.0, 0.0)
        .expect_err("a three-value speed window cannot be read");
    assert!(
        refusal.contains("3 value(s)"),
        "the arity is named, got {refusal}"
    );
    let backwards = LandingRow::authored(0, "anim", "node", &[35.0], &[220.0, 50.0], false);
    let refusal = window_satisfied(&backwards, 60.0, 0.0)
        .expect_err("a backwards speed window cannot be read");
    assert!(
        refusal.contains("backwards"),
        "the inversion is named, got {refusal}"
    );
}

// ---------------------------------------------------------------------------
// The composed landing path, synthetic (runs in CI).
// ---------------------------------------------------------------------------

/// **On the composed synthetic stage: no landing runs nothing, a landing
/// outside the window runs nothing, a landing at an unarmed node runs
/// nothing, and a landing inside the window runs the row's record, its
/// measured call chain, and only those — with the chain's unbindable name
/// and the malformed window each refused by name, once.**
#[test]
fn accept_m01_play_triggers_anims_land_a_composed_landing_starts_the_row_and_its_call_chain() {
    let scratch = Scratch::new("landings");
    let mut app = build_headless(&synthetic_land_stage(scratch.path()))
        .expect("the synthetic stage composes");
    // The composed table is the fixture installation's absence; the test's
    // authored table replaces it before the first tick.
    app.world_mut().insert_resource(synthetic_landings());

    // The four leg states. The nodes anchor on the player's own row, so
    // only the velocity and attitude matter: a descent far too fast to be
    // a landing, an in-window descent far too fast for the row's speed
    // bound, a speed only the unarmed row's window admits, and the
    // landing itself.
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
    let unarmed_window = Body {
        position: None,
        velocity: Vec3::new(35.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    let landed = Body {
        position: None,
        velocity: Vec3::new(60.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };

    // The program's `WAKE_ANIM` starts the arm record, whose
    // `OBJECT_ACTIVE_STATE` statement arms the two node names — the arm
    // task's production surface, consumed by this task's landing path. The
    // body holds a too-fast descent the whole time, so no row can land
    // before the legs below choose their own states.
    pump_until_armed(&mut app, "synthetic_landing_node", 600, Some(&too_fast));
    assert!(
        armed_has(&app, "synthetic_far_node"),
        "the arm record's vocabulary carries the far node too"
    );
    assert!(
        !armed_has(&app, "synthetic_never_armed"),
        "no statement ever armed the never-armed node"
    );

    // Leg 1 — no landing: descending far too fast to be one, even at the
    // node, in the window. Nothing runs, nothing refuses.
    pump(&mut app, 30, Some(&too_fast));
    let host = app.world().resource::<MissionHost>();
    assert!(
        host.landings().activated().is_empty(),
        "a too-fast pass is not a landing: {:?}",
        host.landings().activated()
    );
    assert!(
        host.landings().refused().is_empty(),
        "a pass earns no refusal either: {:?}",
        host.landings().refused()
    );

    // Leg 2 — a landing, but outside the success row's speed window. Its
    // own row runs nothing; the malformed-window row at the same node is
    // refused by name exactly once.
    pump(&mut app, 30, Some(&off_window));
    let host = app.world().resource::<MissionHost>();
    assert!(
        host.landings().activated().is_empty(),
        "an off-window landing runs nothing: {:?}",
        host.landings().activated()
    );
    assert_eq!(
        host.landings().refused().len(),
        1,
        "the malformed window is refused once: {:?}",
        host.landings().refused()
    );
    let refusal = &host.landings().refused()[0];
    assert_eq!(refusal.row, 3);
    assert!(
        refusal.reason.contains("3 value(s)"),
        "the arity is named: {}",
        refusal.reason
    );

    // Leg 3 — the unarmed node's own window is satisfied at this speed,
    // and its radius is zero: nothing runs, because no statement ever
    // armed it.
    pump(&mut app, 30, Some(&unarmed_window));
    let host = app.world().resource::<MissionHost>();
    assert!(
        host.landings().activated().is_empty(),
        "an unarmed node runs nothing: {:?}",
        host.landings().activated()
    );
    assert_eq!(
        host.landings().refused().len(),
        1,
        "the malformed window is still refused exactly once: {:?}",
        host.landings().refused()
    );

    // Leg 4 — the landing itself.
    pump(&mut app, 30, Some(&landed));
    let host = app.world().resource::<MissionHost>();
    let activated = host.landings().activated();
    assert_eq!(activated.len(), 1, "exactly one row ran: {activated:?}");
    assert_eq!(activated[0].row, 0);
    assert_eq!(activated[0].node, "synthetic_landing_node");
    assert_eq!(activated[0].anim, SYNTHETIC_LAND);
    assert_eq!(
        activated[0].outcome,
        ActivationOutcome::Started,
        "the row's record started here"
    );
    let calls = &activated[0].calls;
    assert_eq!(calls.len(), 1, "one name of the chain bound: {calls:?}");
    assert_eq!(calls[0].name, SYNTHETIC_CALLED);
    assert_eq!(calls[0].outcome, ActivationOutcome::Started);
    let refused = host.landings().refused();
    assert_eq!(
        refused.len(),
        2,
        "window refusal + chain refusal: {refused:?}"
    );
    let chain = &refused[1];
    assert_eq!(chain.row, 0);
    assert_eq!(chain.name, SYNTHETIC_CHAIN_UNBOUND);
    assert_eq!(
        chain.matches.len(),
        0,
        "nothing matched the unbindable name"
    );

    // The records' states are observed through the production fold — the
    // same rows an `ANIM_STATE` gate reads — and the two rows that never
    // landed stay unobserved.
    assert!(
        observed_state(&app, SYNTHETIC_LAND).is_some(),
        "the row's record is observed after the landing"
    );
    assert!(
        observed_state(&app, SYNTHETIC_CALLED).is_some(),
        "the chain's called record is observed after the landing"
    );
    assert!(
        observed_state(&app, SYNTHETIC_OTHER).is_none(),
        "the unarmed row's record never started"
    );
    assert!(
        observed_state(&app, SYNTHETIC_FAR).is_none(),
        "the far row's record never started"
    );

    // One attempt per row per run: more in-window frames neither repeat
    // the row nor re-refuse the malformed window.
    pump(&mut app, 60, Some(&landed));
    let host = app.world().resource::<MissionHost>();
    assert_eq!(host.landings().activated().len(), 1, "one attempt per row");
    assert_eq!(host.landings().refused().len(), 2, "each refusal once");

    teardown(&mut app);
}

// ---------------------------------------------------------------------------
// The retail members (require $CS_GAME_DIR; CI skips them).
// ---------------------------------------------------------------------------

/// The production stage for retail M01 with exactly the given blocks
/// declared awake and the recovered engine model set aside — the arm
/// task's own declared-lifecycle device, so this test's subject (the
/// landing chain) runs while nothing else can complete and settle the run.
fn retail_land_stage(awake: &[u32]) -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let mut stage =
        stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let mut table = BlockLifecycleTable::new();
    for index in awake {
        table
            .declare(*index, LifecycleDecl::awake())
            .expect("each block is declared once");
    }
    stage.host.block_lifecycles = table;
    stage.host.engine_objectives = None;
    stage
}

/// The composed entry's answer for the last tick, or a panic naming the
/// refusal.
fn last_tick(app: &App) -> u64 {
    let report = app
        .world()
        .get_resource::<MissionHostReport>()
        .expect("the composed entry wrote a report");
    match &report.answer {
        Ok(MissionHostTick { tick, .. }) => tick.0,
        Err(error) => panic!("the composed step refused: {error}"),
    }
}

/// **The retail reader reports M01's three measured rows verbatim, and the
/// position rules for their three nodes come out of the world container's
/// own tree with the measured offsets.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_anims_land_the_retail_reader_reports_m01s_rows_and_nodes() {
    let table = read_landings(&game_dir(), "zbd/c1c")
        .expect("M01's world-group reader archive and landing member read");
    assert!(
        table.refusals.is_empty(),
        "no retail row or node is refused: {:?}",
        table.refusals
    );
    let rows: Vec<(&str, &str)> = table
        .rows
        .iter()
        .map(|row| (row.anim.as_str(), row.node.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("wv_initiate_hookup", "wv_manual_land"),
            ("hooked_to_klondike", "pz_manual_land"),
            ("hooked_to_klondike", "pz_auto_land"),
        ],
        "the measured three rows, in stored order"
    );
    assert_eq!(table.rows[0].angle, vec![35.0]);
    assert_eq!(table.rows[0].speed, vec![50.0, 220.0]);
    assert!(!table.rows[0].auto);
    assert_eq!(table.rows[1].angle, vec![35.0]);
    assert_eq!(table.rows[1].speed, vec![50.0, 220.0]);
    assert!(
        table.rows[2].auto,
        "the third row spells auto and no window"
    );
    assert!(table.rows[2].angle.is_empty() && table.rows[2].speed.is_empty());

    let nodes: Vec<(&str, &str)> = table
        .nodes
        .iter()
        .map(|entry| (entry.node.as_str(), entry.anchor.as_str()))
        .collect();
    assert_eq!(
        nodes,
        vec![
            ("wv_manual_land", "workersvoyagezep"),
            ("pz_manual_land", "piratezep"),
            ("pz_auto_land", "piratezep"),
        ],
        "each node anchors on the bound actor whose subject node it hangs from"
    );
    // The measured container offsets: the wv node 150.46 m astern and
    // 67.05 m below its anchor, the pz nodes 116.61 m astern and 35.34 m
    // below theirs — read from the same node tree the composition loads.
    let wv = &table.nodes[0].offset;
    assert!(
        (wv[1] + 67.05).abs() < 0.01 && (wv[2] - 150.46).abs() < 0.01,
        "the measured wv offset, got {wv:?}"
    );
    let pz = &table.nodes[1].offset;
    assert!(
        (pz[1] + 35.34).abs() < 0.01 && (pz[2] - 116.61).abs() < 0.01,
        "the measured pz offset, got {pz:?}"
    );
}

/// **On the composed retail M01: a landing inside the window at the armed
/// `wv_manual_land` node runs the row's record (`wv_initiate_hookup`), the
/// measured call chain it carries starts `wv_pickup_copilot` through the
/// same player, and the program's own `OBJECTIVE15` gate block — whose
/// condition is `wv_pickup_copilot EXECUTED` — completes only after that
/// real landing.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_triggers_anims_land_a_retail_landing_runs_the_row_and_the_gated_state() {
    // OBJECTIVE49 (block index 48) is the non-circular arming path: its
    // only condition is its own awakeness, and its action wakes
    // `activate_dropoff_node`, whose `OBJECT_ACTIVE_STATE` statement
    // carries `wv_manual_land`. OBJECTIVE15 (14) is the gate this landing
    // must close. Both declarations are test-side inputs; every step after
    // them is production.
    let mut app =
        build_headless(&retail_land_stage(&[48, 14])).expect("M01's stage composes headlessly");

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

    // No landing yet: nothing has run any row.
    pump(&mut app, 30, None);
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "no landing runs nothing"
    );

    // The composed landing position of the node — the writer's own row,
    // which anchors on the bound workersvoyagezep.
    let position = composed_node_position(&app, "wv_manual_land");

    // Place the player at the node with the row's window satisfied. The
    // pose, velocity and attitude are re-written before each committed
    // frame, so the observed state is the leg's own.
    let landed = Body {
        position: Some(position),
        velocity: Vec3::new(60.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    pump(&mut app, 30, Some(&landed));
    {
        let host = app.world().resource::<MissionHost>();
        let activated = host.landings().activated();
        assert_eq!(activated.len(), 1, "exactly one row ran: {activated:?}");
        assert_eq!(activated[0].node, "wv_manual_land");
        assert_eq!(activated[0].anim, "wv_initiate_hookup");
        assert_eq!(activated[0].outcome, ActivationOutcome::Started);
        let calls = &activated[0].calls;
        assert!(
            calls.iter().any(|call| call.name == "wv_pickup_copilot"
                && call.outcome == ActivationOutcome::Started),
            "the measured call chain started wv_pickup_copilot: {calls:?}"
        );
        assert!(
            calls.iter().any(|call| call.name == "wv_hookup_player"),
            "the measured call chain started wv_hookup_player: {calls:?}"
        );
        // `letterbox` matches a carrier record but no member declares it:
        // the player refuses the row, and the chain records that outcome —
        // the refusal's own reasons live on the player's ledger.
        assert!(
            calls
                .iter()
                .any(|call| call.name == "letterbox" && call.outcome == ActivationOutcome::Refused),
            "the chain's undeclared name is refused by name: {calls:?}"
        );
        assert!(
            !host
                .landings()
                .refused()
                .iter()
                .any(|refusal| refusal.row == 1 || refusal.row == 2),
            "the pz rows never landed, so they earn no refusal"
        );
    }

    // The gated state: `wv_pickup_copilot` runs its measured 6.0 stored
    // time units (720 ticks at the player's 120 Hz) and reaches
    // EXECUTED — the state `OBJECTIVE15` waits for — and the real program
    // completes that block. Nothing completes it without the landing: the
    // frames above before the placement proved the landing is the cause.
    let mut executed = false;
    for _ in 0..30 {
        pump(&mut app, 60, Some(&landed));
        if observed_state(&app, "wv_pickup_copilot") == Some(3) {
            executed = true;
            break;
        }
    }
    assert!(
        executed,
        "wv_pickup_copilot reached EXECUTED, observed {:?}",
        observed_state(&app, "wv_pickup_copilot")
    );
    assert!(
        observed_state(&app, "wv_initiate_hookup").is_some(),
        "the row's own record is observed"
    );
    let host = app.world().resource::<MissionHost>();
    assert_eq!(
        host.lifecycles().state(14),
        Some(ObjectiveLifecycle::Done),
        "OBJECTIVE15 completed — its gate read true because the landing ran the record"
    );
    assert_eq!(
        host.lifecycles().state(48),
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

/// **On the composed retail M01: nothing runs without a landing, a landing
/// outside the row's window runs nothing, a too-fast pass runs nothing,
/// and a landing at the (unarmed) pz nodes runs nothing.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_triggers_anims_land_retail_no_landing_and_off_window_landings_run_nothing() {
    let mut app =
        build_headless(&retail_land_stage(&[48])).expect("M01's stage composes headlessly");
    pump_until_armed(&mut app, "wv_manual_land", 600, None);

    // Leg 1 — no landing: the player is where the mission started,
    // kilometres from the node.
    pump(&mut app, 60, None);
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "no landing runs nothing"
    );

    let position = composed_node_position(&app, "wv_manual_land");

    // Leg 2 — at the node, in the sink bound, but far outside the row's
    // speed window.
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

    // Leg 3 — at the node, in the window, but descending far too fast to
    // be a landing.
    let too_fast = Body {
        position: Some(position),
        velocity: Vec3::new(60.0, -30.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    pump(&mut app, 60, Some(&too_fast));
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "a too-fast pass runs nothing"
    );

    // Leg 4 — a real landing at the pz nodes, whose vocabulary no armed
    // statement carries under the designed arming surface.
    assert!(
        !armed_has(&app, "pz_manual_land") && !armed_has(&app, "pz_auto_land"),
        "the pz nodes stay unarmed: pzhomebase, the only record whose \
         vocabulary carries them, starts only when OBJECTIVE18 completes, \
         and that block's condition is the landing's own output"
    );
    let pz = composed_node_position(&app, "pz_manual_land");
    let at_pz = Body {
        position: Some(pz),
        velocity: Vec3::new(60.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    };
    pump(&mut app, 60, Some(&at_pz));
    let host = app.world().resource::<MissionHost>();
    assert!(
        host.landings().activated().is_empty(),
        "a landing at an unarmed node runs nothing: {:?}",
        host.landings().activated()
    );
    assert!(
        observed_state(&app, "hooked_to_klondike").is_none(),
        "the unarmed row's record never started"
    );
    let _ = last_tick(&app);

    teardown(&mut app);
}
