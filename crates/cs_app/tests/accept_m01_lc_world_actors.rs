//! Task #632 (`M01-LC-WORLD-ACTORS`): the measured binding from a mission's
//! `startanims.zrd` startup event table to the `ANIMATION_DEFINITIONS` members
//! that declare the named animations, and from a definition's object names to
//! the scene nodes of a world container.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`
//! (`### F20-D`) and `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`
//! (`### F34-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//! Findings: `docs/findings/2026-10-04-m01-lc-world-actors.md`.
//!
//! These tests drive `cs_app::animation::programs` only. Every fixture below is
//! newly authored synthetic bytes in the **measured record shapes** (a `.zrd`
//! document is tag `1` int, `2` float, `3` text, `4` list of `count - 1`
//! children; a record's root is a one-element list holding a flat
//! `KEY, value` body), and no value is a claim about the original game: the
//! synthetic cases prove the reader, the resolver and the refusal rules, and
//! the `#[ignore]`d cases are the ones that measure the installation.
//!
//! The retail half is `ZBD/C1C/M01/zrdr.zbd`, its world group
//! `ZBD/C1C/zrdr.zbd`, the shared `ZBD/zrdr.zbd` and the world container
//! `ZBD/C1C/gamez.zbd`. That is the mission `missions/bindings/M01.json` names,
//! and it is the closure `VS-M01-RUNTIME` waits on.

#![allow(clippy::too_many_lines)]

use cs_app::animation::programs::{
    ACTIVATION_FIELD, ANIMATION_DEFINITIONS_RECORD, ANIMATION_NAME_FIELD, Activation,
    AnimationDefinitionMember, AnimationDefinitionSite, DefinitionObjects, LOAD_GAME_START,
    MINIMUM_TO_SATISFY, NAME_ALTERNATE_FIELD, NAME_FIELD, NEW_GAME_START, NODE_PATH_SEPARATOR,
    NodePathSelector, OPTIONS_PREREQUISITE, ObjectSelector, REQUIRED_PREREQUISITE, SEQUENCE_FIELD,
    STARTUP_MEMBER, SelectorMatch, SelectorSegment, StartupAnimationBinding, StartupAnimationTable,
    UnmeasuredFieldFamily, WorldActorProgramBinding, WorldNodeNames,
    read_animation_definition_member, read_startup_animations,
};
use cs_assets::install;
use cs_content::stunts::ZrdValue;
use cs_formats::gamez::{
    GAMEZ_HEADER_BYTES, GameZHeader, GameZNodes, NODE_TYPE_OBJECT3D, NodeKind, RawNode,
    RawNodeInfo, RawObject3dData, read_gamez_nodes,
};
use cs_formats::io::ParseContext;
use cs_formats::script_raw::{discover_container, mission_scope};
use cs_formats::zbd::{GAMEZ_SIGNATURE, GAMEZ_VERSION};
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;
use cs_types::install::RelativePath;

// ---------------------------------------------------------------------------
// Synthetic `.zrd` authoring. Every byte below is authored here.
// ---------------------------------------------------------------------------

/// A `.zrd` int node: tag `1` then the value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length, the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then `children.len() + 1`, then the children —
/// the measured `count - 1` grammar.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// A one-element list wrapping one text, the shape the original stores a name
/// list in (`startanims.zrd`'s animation entries, a definition's `NAME`).
fn zrd_name(text: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text(text)])
}

/// A flat alternating `KEY, value` record body.
fn zrd_flat(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::with_capacity(entries.len() * 2);
    for (key, value) in entries {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(children)
}

/// A record member's root: one element holding the flat body.
fn zrd_record(body: Vec<u8>) -> Vec<u8> {
    zrd_list(vec![body])
}

/// One definition's own flat record body, in the measured field order.
fn zrd_definition(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    zrd_flat(entries)
}

/// An `ANIMATION_LIST`: its definitions alternate their key with their record,
/// the same flat shape the startup table uses.
fn animation_list(definitions: Vec<Vec<u8>>) -> Vec<u8> {
    let mut children = Vec::with_capacity(definitions.len() * 2);
    for definition in definitions {
        children.push(zrd_text("ANIMATION_DEFINITION"));
        children.push(definition);
    }
    zrd_list(children)
}

/// One member's whole `ANIMATION_DEFINITIONS` record.
fn animation_definitions_document(body_fields: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    zrd_record(zrd_flat(vec![(
        ANIMATION_DEFINITIONS_RECORD,
        zrd_flat(body_fields),
    )]))
}

/// A `startanims.zrd` with the measured two-event shape.
fn startup_document(new_game: Vec<&str>, load_game: Vec<&str>) -> Vec<u8> {
    zrd_record(zrd_flat(vec![
        (
            NEW_GAME_START,
            zrd_list(new_game.into_iter().map(zrd_name).collect()),
        ),
        (
            LOAD_GAME_START,
            zrd_list(load_game.into_iter().map(zrd_name).collect()),
        ),
    ]))
}

/// A `SourceSpan` for a synthetic member: the reading is about provenance
/// plumbing, not about a hash the fixture does not have.
fn synthetic_span(container: &str, member: &str, offset: u64, len: u64) -> SourceSpan {
    let install_sha256 = ContentHash::from_bytes([7_u8; 32]);
    SourceSpan::new(install_sha256, container, Some(member), offset, len, None)
        .expect("the synthetic span is recordable")
}

/// A synthetic GameZ container holding the named records, built through the same
/// `RawNode` shape the production node reader emits.
fn synthetic_world(names: &[&str]) -> GameZNodes {
    let nodes = names
        .iter()
        .enumerate()
        .map(|(index, name)| RawNode {
            index: index as u32,
            name: (*name).to_owned(),
            node_index: 0x0200_0000 | index as u32,
            info: RawNodeInfo {
                flags: 0x0180_0000,
                zone_id: 255,
                node_type: NODE_TYPE_OBJECT3D,
                data_ptr: 1,
                mesh_index: -1,
                action_priority: 1,
                area_partition: [-1, -1, 0, 0],
                unk196: 160,
                unk116: [[0.0; 3]; 2],
                unk140: [[0.0; 3]; 2],
                unk164: [[0.0; 3]; 2],
                ..zero_info()
            },
            kind: NodeKind::Object3d(RawObject3dData {
                flags: 0,
                rotation: [0.0; 3],
                scale: [1.0; 3],
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                translation: [0.0; 3],
            }),
            data_offset: 1,
            data_bytes: 148,
            parent: None,
            children: Vec::new(),
        })
        .collect();
    GameZNodes {
        header: GameZHeader {
            signature: GAMEZ_SIGNATURE,
            version: GAMEZ_VERSION,
            unk08: 0,
            texture_count: 0,
            textures_offset: GAMEZ_HEADER_BYTES as u32,
            materials_offset: 0,
            meshes_offset: 0,
            node_array_size: names.len() as u32,
            light_index: 0,
            nodes_offset: 0,
        },
        nodes,
        info_offset: 0,
        info_end: 0,
        data_offset: 0,
        data_end: 0,
        findings: Vec::new(),
    }
}

fn zero_info() -> RawNodeInfo {
    RawNodeInfo {
        flags: 0,
        unk040: 0,
        unk044: 0,
        zone_id: 0,
        node_type: 0,
        data_ptr: 0,
        mesh_index: -1,
        environment_data: 0,
        action_priority: 1,
        action_callback: 0,
        area_partition: [0; 4],
        parent_count: 0,
        children_count: 0,
        parent_array_ptr: 0,
        children_array_ptr: 0,
        unk096: 0,
        unk100: 0,
        unk104: 0,
        unk108: 0,
        unk112: 0,
        unk116: [[0.0; 3]; 2],
        unk140: [[0.0; 3]; 2],
        unk164: [[0.0; 3]; 2],
        unk188: 0,
        unk192: 0,
        unk196: 0,
        unk200: 0,
        unk204: 0,
    }
}

/// A definition member holding two definitions: one named through `NAME` with a
/// startup activation and one through `NAME1` with a wildcard, a prerequisite
/// and two statements.
fn two_definition_member() -> AnimationDefinitionMember {
    let bytes = animation_definitions_document(vec![(
        "ANIMATION_LIST",
        animation_list(vec![
            zrd_definition(vec![
                (NAME_FIELD, zrd_list(vec![zrd_name("tailhook")])),
                (ANIMATION_NAME_FIELD, zrd_name("hookup_state")),
                (ACTIVATION_FIELD, zrd_name("ON_STARTUP")),
                ("SAVE_LOG", zrd_list(vec![zrd_text("ON")])),
                ("RESET_TIME", zrd_list(vec![zrd_int(4_294_967_295)])),
                (
                    SEQUENCE_FIELD,
                    zrd_flat(vec![
                        ("NAME", zrd_name("callback_sequence")),
                        (
                            "OBJECT_MOTION_FROM_TO",
                            zrd_flat(vec![
                                ("NAME", zrd_name("MAIN_ROOT_NODE")),
                                ("ROTATE_FROM", zrd_list(vec![zrd_int(0)])),
                                ("ROTATE_TO", zrd_list(vec![zrd_int(60)])),
                                ("RUN_TIME", zrd_list(vec![zrd_int(1)])),
                            ]),
                        ),
                    ]),
                ),
            ]),
            zrd_definition(vec![
                (
                    NAME_ALTERNATE_FIELD,
                    zrd_list(vec![
                        zrd_text("destroy_aagun01"),
                        zrd_list(vec![zrd_text("aagun"), zrd_text("turret01")]),
                    ]),
                ),
                (ANIMATION_NAME_FIELD, zrd_name("destroy_aagun")),
                (
                    "ACTIVATION_PREREQUISITE",
                    zrd_list(vec![
                        zrd_text(OPTIONS_PREREQUISITE),
                        zrd_list(vec![
                            zrd_text(MINIMUM_TO_SATISFY),
                            zrd_list(vec![zrd_int(2)]),
                            zrd_text("OBJECT_INACTIVE_LIST"),
                            zrd_list(vec![zrd_list(vec![zrd_text("aagun01")])]),
                        ]),
                    ]),
                ),
                (
                    SEQUENCE_FIELD,
                    zrd_flat(vec![
                        ("NAME", zrd_name("callback_sequence")),
                        ("CALL_ANIMATION", zrd_list(vec![zrd_name("boom")])),
                    ]),
                ),
                (
                    SEQUENCE_FIELD,
                    zrd_flat(vec![
                        ("NAME", zrd_name("callback_sequence")),
                        ("OBJECT_ROTATE_STATE", zrd_flat(vec![("STATE", zrd_int(1))])),
                    ]),
                ),
            ]),
        ]),
    )]);
    read_animation_definition_member("synthetic.zrd", &bytes)
        .expect("the synthetic member reads as an ANIMATION_DEFINITIONS record")
}

// ---------------------------------------------------------------------------
// The two measured record shapes read.
// ---------------------------------------------------------------------------

/// **The startup table is an event table, not a member list, and the reader
/// keeps the two events apart.**
///
/// The task's central question is *which member a mission activates at
/// startup*, and the measurement is that a mission never names a member: it
/// names an animation, under one of exactly two event keys. So this asserts the
/// table reads, both events are distinct, `LOAD_GAME_START` is reachable by name
/// and reachable-as-absent, and the flattened name walk returns both events'
/// names in stored order.
#[test]
fn accept_m01_lc_world_actors_the_startup_table_reads_two_events_and_names_no_member() {
    let bytes = startup_document(
        vec!["wv_hookup_state", "call_add_jack"],
        vec!["player_setup"],
    );
    let table = read_startup_animations(&bytes).expect("the startup table reads");

    assert_eq!(
        table.events().len(),
        2,
        "the table declares exactly two events"
    );
    assert_eq!(table.events()[0].event(), NEW_GAME_START);
    assert_eq!(table.events()[1].event(), LOAD_GAME_START);
    assert_eq!(
        table.event(NEW_GAME_START),
        Some(["wv_hookup_state".to_owned(), "call_add_jack".to_owned()].as_slice()),
        "an event answers its stored names in order"
    );
    assert_eq!(
        table.event(LOAD_GAME_START),
        Some(["player_setup".to_owned()].as_slice())
    );
    assert_eq!(
        table.event("SOME_OTHER_EVENT"),
        None,
        "an event the member does not declare is a measured absence, not an empty list"
    );
    assert_eq!(
        table.animation_names().collect::<Vec<_>>(),
        vec!["wv_hookup_state", "call_add_jack", "player_setup"],
        "the flattened walk crosses the event boundary in stored order"
    );
    assert!(!table.is_empty());
}

/// **A definition member reads its animations, its objects, its activation and
/// its statements, and names every field it did not interpret.**
///
/// Removing any one read loses a specific assertion here: the object key
/// (`NAME` vs `NAME1`), the activation, the prerequisite's structure, either
/// statement kind, or the uninterpreted-field list that is what makes "no
/// record is silently dropped" checkable.
#[test]
fn accept_m01_lc_world_actors_a_definition_member_reads_objects_activation_and_statements() {
    let member = two_definition_member();

    assert_eq!(member.member(), "synthetic.zrd");
    assert_eq!(member.record(), ANIMATION_DEFINITIONS_RECORD);
    assert!(
        member.fields().is_empty(),
        "the record body's own fields are listed: {:?}",
        member.fields()
    );
    assert_eq!(member.definitions().len(), 2, "both definitions read");
    assert!(
        member.definition_files().is_empty(),
        "a member with no ANIMATION_DEFINITION_FILE entry declares none"
    );

    let first = &member.definitions()[0];
    assert_eq!(first.index(), 0);
    assert_eq!(first.animation_name(), Some("hookup_state"));
    assert_eq!(
        first.objects().field(),
        Some(NAME_FIELD),
        "a flat NAME list is the node-name shape"
    );
    assert_eq!(first.objects().node_names().count(), 1);
    assert_eq!(
        first
            .objects()
            .node_names()
            .next()
            .map(ObjectSelector::stored),
        Some("tailhook")
    );
    assert_eq!(
        first.activation().map(Activation::stored),
        Some("ON_STARTUP"),
        "the activation is read, not the default ON_CALL"
    );
    assert!(first.activation().is_some_and(Activation::is_startup));
    assert_eq!(first.sequences().len(), 1);
    assert_eq!(
        first.sequences()[0].entries().len(),
        1,
        "the sequence's own NAME is its name, so the sequence states one statement"
    );
    assert_eq!(
        first.sequences()[0].entries()[0].kind(),
        "OBJECT_MOTION_FROM_TO"
    );
    assert_eq!(
        first.sequences()[0].entries()[0].fields(),
        ["NAME", "ROTATE_FROM", "ROTATE_TO", "RUN_TIME"],
        "a statement's own fields are kept in stored order"
    );
    assert_eq!(
        first.uninterpreted_fields(),
        ["RESET_TIME", "SAVE_LOG"],
        "a field the reader does not interpret is named, never dropped"
    );

    let second = &member.definitions()[1];
    assert_eq!(second.index(), 1);
    let DefinitionObjects::StateBindings(bindings) = second.objects() else {
        panic!("NAME1 is the state-binding shape: {:?}", second.objects());
    };
    assert_eq!(
        second.objects().field(),
        Some(NAME_ALTERNATE_FIELD),
        "the shape names the field it came from"
    );
    assert_eq!(bindings.len(), 1, "one state bound to one path");
    assert_eq!(bindings[0].state(), "destroy_aagun01");
    assert_eq!(bindings[0].path().stored(), "aagun/turret01");
    assert_eq!(bindings[0].path().steps().len(), 2);
    assert_eq!(
        bindings[0]
            .path()
            .terminal()
            .and_then(ObjectSelector::literal_name),
        Some("turret01"),
        "the terminal step is the node the state moves"
    );
    assert!(!bindings[0].state_is_wildcarded());
    assert_eq!(
        second.objects().node_names().count(),
        1,
        "a state binding contributes its path's terminal node name"
    );
    assert_eq!(
        second.objects().selectors().count(),
        2,
        "and every step of the path is retained"
    );
    assert_eq!(
        second.activation(),
        None,
        "a definition that states no activation reads as none, not as ON_CALL"
    );
    assert!(!second.activation().is_some_and(Activation::is_startup));
    let prerequisite = second.prerequisite().expect("the prerequisite reads");
    assert_eq!(prerequisite.requirement(), OPTIONS_PREREQUISITE);
    assert_eq!(prerequisite.minimum_to_satisfy(), Some(2));
    assert_eq!(prerequisite.condition().stored(), "OBJECT_INACTIVE_LIST");
    assert!(
        prerequisite.condition().names_node_paths(),
        "an object list names node paths, not animations"
    );
    assert_eq!(prerequisite.animation_names().len(), 0);
    assert_eq!(prerequisite.paths().len(), 1);
    assert_eq!(prerequisite.paths()[0].stored(), "aagun01");
    assert_eq!(REQUIRED_PREREQUISITE, "REQUIRED");
    assert_eq!(
        second.sequences().len(),
        2,
        "SEQUENCE_DEFINITION is repeatable and both sequences read"
    );
    assert_eq!(
        second.sequence_kinds().into_iter().collect::<Vec<_>>(),
        vec!["CALL_ANIMATION", "OBJECT_ROTATE_STATE"]
    );
    assert!(
        second.uninterpreted_fields().is_empty(),
        "every field of this definition was read, so none is left to name"
    );

    assert_eq!(
        member.startup_definitions().count(),
        1,
        "the ON_STARTUP definition is the member's placement statement and the other is not"
    );
    assert_eq!(
        member.sequence_kinds().into_iter().collect::<Vec<_>>(),
        vec![
            "CALL_ANIMATION",
            "OBJECT_MOTION_FROM_TO",
            "OBJECT_ROTATE_STATE",
        ],
        "a sequence's own NAME is its name, not a statement kind"
    );
    assert_eq!(
        member.definitions()[0].sequences()[0].name(),
        Some("callback_sequence"),
        "and it is read as the sequence's name"
    );
    assert_eq!(
        member.definitions_of("hookup_state").count(),
        1,
        "a definition is found by the animation it implements"
    );
    assert_eq!(
        member.definitions_of("destroy_aagun").count(),
        1,
        "and the second definition is found the same way"
    );
    assert_eq!(member.definitions_of("nothing_declares_this").count(), 0);
}

// ---------------------------------------------------------------------------
// The resolver: one declaration, two declarations, none.
// ---------------------------------------------------------------------------

/// **A name one member declares resolves to that member and its byte span; a
/// name two members declare is ambiguous and a name nobody declares is
/// unresolved.**
///
/// This is the acceptance branch that matters: the task asks whether the members
/// a mission requires resolve to a program. They resolve to a **measured
/// binding** — archive, member, span, selectors — and the resolver never picks a
/// winner when two members declare the same animation, because nothing measured
/// says which one the original ran.
#[test]
fn accept_m01_lc_world_actors_a_resolved_binding_names_its_member_and_its_span() {
    let mission_bytes = animation_definitions_document(vec![(
        "ANIMATION_LIST",
        animation_list(vec![zrd_definition(vec![
            (NAME_FIELD, zrd_list(vec![zrd_name("tailhook")])),
            (ANIMATION_NAME_FIELD, zrd_name("hookup_state")),
            (ACTIVATION_FIELD, zrd_name("ON_STARTUP")),
        ])]),
    )]);
    let shared_bytes = animation_definitions_document(vec![(
        "ANIMATION_LIST",
        animation_list(vec![
            zrd_definition(vec![
                (NAME_FIELD, zrd_list(vec![zrd_name("piratezep")])),
                (ANIMATION_NAME_FIELD, zrd_name("engines_start")),
            ]),
            // The same animation the mission also declares: ambiguous.
            zrd_definition(vec![
                (NAME_FIELD, zrd_list(vec![zrd_name("piratezep")])),
                (ANIMATION_NAME_FIELD, zrd_name("hookup_state")),
            ]),
        ]),
    )]);
    let mission = read_animation_definition_member("mission.zrd", &mission_bytes)
        .expect("the mission member reads");
    let shared = read_animation_definition_member("shared.zrd", &shared_bytes)
        .expect("the shared member reads");

    let mut declarations = Vec::new();
    for (archive, member, bytes, reader) in [
        ("zbd/c1c/m01/zrdr.zbd", "mission.zrd", &mission, &mission),
        ("zbd/zrdr.zbd", "shared.zrd", &shared, &shared),
    ] {
        let _ = bytes;
        for definition in reader.definitions() {
            declarations.push(AnimationDefinitionSite::new(
                archive,
                member,
                synthetic_span(archive, member, 4, 128),
                definition.clone(),
            ));
        }
    }

    let table = read_startup_animations(&startup_document(
        vec!["hookup_state", "engines_start", "nobody_declares_this"],
        vec!["also_missing"],
    ))
    .expect("the startup table reads");
    let binding = WorldActorProgramBinding::new(
        "zbd/c1c/m01/zrdr.zbd",
        vec!["zbd/c1c/m01/zrdr.zbd".to_owned(), "zbd/zrdr.zbd".to_owned()],
        vec!["mission.zrd".to_owned(), "shared.zrd".to_owned()],
        table,
        &declarations,
    );

    assert_eq!(binding.scope(), "zbd/c1c/m01/zrdr.zbd");
    assert_eq!(
        binding.archives().len(),
        2,
        "both searched archives are reported"
    );
    assert_eq!(binding.members().len(), 2);
    assert_eq!(
        binding.startup().len(),
        4,
        "four animation names across two events"
    );

    let new_game: Vec<&StartupAnimationBinding> = binding.startup_of(NEW_GAME_START).collect();
    assert_eq!(
        new_game.len(),
        3,
        "three animations fire on a fresh game in this fixture"
    );

    let hookup = binding
        .startup_of(NEW_GAME_START)
        .find(|entry| entry.animation_name() == "hookup_state")
        .expect("the startup table fires hookup_state");
    assert!(
        hookup.resolution().is_ambiguous(),
        "two members declare it, so it is ambiguous and no member wins: {:?}",
        hookup.resolution()
    );
    let ambiguous = hookup
        .resolution()
        .ambiguous()
        .expect("two sites are carried");
    assert_eq!(ambiguous.len(), 2);
    assert_eq!(
        ambiguous
            .iter()
            .map(AnimationDefinitionSite::member)
            .collect::<Vec<_>>(),
        vec!["mission.zrd", "shared.zrd"],
        "the search order is the reporting order, not a preference"
    );
    assert_eq!(ambiguous[0].archive(), "zbd/c1c/m01/zrdr.zbd");
    assert_eq!(
        ambiguous[0].span().offset(),
        4,
        "the site carries the declaring member's own byte span"
    );
    assert_eq!(
        ambiguous[0].span().member_key(),
        Some("mission.zrd"),
        "the span names the member, so the binding is traceable to bytes"
    );

    let engines = binding
        .startup_of(NEW_GAME_START)
        .find(|entry| entry.animation_name() == "engines_start")
        .expect("the startup table fires engines_start");
    let site = engines
        .resolution()
        .single()
        .expect("exactly one member declares it");
    assert_eq!(site.member(), "shared.zrd");
    assert_eq!(site.archive(), "zbd/zrdr.zbd");
    assert_eq!(site.animation_name(), Some("engines_start"));
    assert_eq!(site.objects().node_names().count(), 1);
    assert_eq!(
        site.objects()
            .node_names()
            .next()
            .map(ObjectSelector::stored),
        Some("piratezep")
    );

    assert_eq!(
        binding
            .unresolved()
            .map(|entry| entry.animation_name())
            .collect::<Vec<_>>(),
        vec!["nobody_declares_this", "also_missing"],
        "a name no member declares is reported unresolved, in event order"
    );
    assert_eq!(
        binding
            .ambiguous()
            .map(|entry| entry.animation_name())
            .collect::<Vec<_>>(),
        vec!["hookup_state"],
        "and an ambiguous name is listed apart from an unresolved one"
    );
    assert_eq!(
        binding
            .resolved()
            .map(|entry| entry.animation_name())
            .collect::<Vec<_>>(),
        vec!["engines_start"],
        "so the three failures stay distinguishable"
    );
    assert_eq!(
        binding.startup_activations().len(),
        1,
        "the scope's own ON_STARTUP definition is reported as a placement statement"
    );
    assert_eq!(binding.startup_activations()[0].member(), "mission.zrd");
    assert_eq!(
        binding
            .selected_objects()
            .map(|(_, selector)| selector.stored())
            .collect::<Vec<_>>(),
        vec!["piratezep"],
        "only a resolved binding contributes a selected object"
    );
}

// ---------------------------------------------------------------------------
// The selector, measured against a container's records.
// ---------------------------------------------------------------------------

/// **A wildcard selects by its stored prefix, a bare name selects the record
/// that carries it, a node path is reported unmeasured and a name nothing
/// carries is reported unmatched.**
///
/// The corpus measurement behind this is that no stored scene-node name of any
/// measured GameZ container contains `*`, while stored object names do — so the
/// wildcard reading is the only one the two vocabularies can both support. The
/// test also pins that the path case is refused rather than matched on its last
/// step: a node-name table carries no hierarchy, and inventing one would be a
/// guess about which node a path means.
#[test]
fn accept_m01_lc_world_actors_a_wildcard_selects_a_prefix_and_a_path_is_left_unmeasured() {
    let world = synthetic_world(&["aagun01", "aagun02", "aagun10", "tailhook", "tail"]);
    let names = WorldNodeNames::from_gamez("zbd/c1c/gamez.zbd", &world);
    assert_eq!(names.container(), "zbd/c1c/gamez.zbd");
    assert_eq!(names.len(), 5);
    assert!(!names.is_empty());

    let literal = ObjectSelector::parse("aagun01").expect("a bare name parses");
    assert!(!literal.has_wildcard());
    assert_eq!(literal.literal_name(), Some("aagun01"));
    assert_eq!(
        names.resolve(&literal).occurrences(),
        Some(1),
        "a bare name selects the one record that carries it"
    );

    let twin = ObjectSelector::parse("tail").expect("a bare name parses");
    assert_eq!(
        names.resolve(&twin).occurrences(),
        Some(1),
        "a prefix that is not a wildcard is compared whole, so 'tail' is not 'tailhook'"
    );

    let wildcard = ObjectSelector::parse("aagun**").expect("a wildcard parses");
    assert!(wildcard.has_wildcard());
    assert_eq!(
        wildcard.literal_name(),
        None,
        "a wildcard is not a literal name and is never reported as one"
    );
    assert_eq!(
        wildcard.segment(),
        &SelectorSegment::PrefixWildcard("aagun".to_owned()),
        "the wildcard is read up to its first '*', which is what the corpus matches"
    );
    assert_eq!(
        names.resolve(&wildcard).occurrences(),
        Some(3),
        "aagun** selects aagun01, aagun02 and aagun10"
    );
    assert!(
        !names.resolve(&wildcard).is_unmeasured(),
        "a wildcard over a node-name table is a measurement, not a gap"
    );
    assert_eq!(
        wildcard.wildcard_evidence(),
        Some((
            "aagun**".to_owned(),
            "f20-anim.object-selector-wildcard-is-a-node-name-prefix"
        )),
        "the wildcard's own claim id is reported, so the rule is never read as a measurement"
    );
    assert_eq!(
        wildcard.rule_provenance().class,
        cs_types::evidence::ClaimStatus::Designed,
        "the matching rule is designed; only the presence of '*' is measured"
    );
    assert_eq!(
        wildcard.rule_provenance().claim_id.as_str(),
        "f20-anim.object-selector-wildcard-is-a-node-name-prefix",
        "and it carries the one rule's own claim id"
    );

    let path = NodePathSelector::parse("wv_tailhook/pickup_node").expect("a path parses");
    assert_eq!(path.steps().len(), 2);
    assert_eq!(
        path.terminal().and_then(ObjectSelector::literal_name),
        Some("pickup_node")
    );
    assert!(!path.has_wildcard());
    let matched = names.resolve_path(&path);
    assert!(
        matched.is_unmeasured(),
        "a node path is not answerable from a node-name table and is reported as such: {matched:?}"
    );
    assert_eq!(matched.occurrences(), None);
    assert_eq!(
        matched,
        SelectorMatch::Path {
            path: "wv_tailhook/pickup_node".to_owned()
        }
    );
    let wildcard_step = format!("lkgasbag0*{NODE_PATH_SEPARATOR}panelleftb1");
    let wildcard_path = NodePathSelector::parse(&wildcard_step).expect("a wildcard path parses");
    assert!(
        wildcard_path.has_wildcard(),
        "a path step carries the same wildcard a bare name does"
    );
    assert!(
        names.resolve_path(&wildcard_path).is_unmeasured(),
        "and it is still unmeasured here, because resolving it needs the hierarchy"
    );

    let missing = ObjectSelector::parse("nothing_here").expect("a bare name parses");
    let matched = names.resolve(&missing);
    assert_eq!(
        matched.occurrences(),
        Some(0),
        "a name no record carries selects nothing, which is a measured zero"
    );
    assert!(
        !matched.is_unmeasured(),
        "a zero is a measurement of this container, not a gap: {matched:?}"
    );
    assert_eq!(
        matched,
        SelectorMatch::Node {
            name: "nothing_here".to_owned(),
            occurrences: 0,
        }
    );
    assert!(
        ObjectSelector::parse("*").is_err(),
        "a wildcard with no prefix would select every record and is refused, exactly like an empty \
         selector: it is not a name the original can have authored"
    );

    assert!(
        ObjectSelector::parse("").is_err(),
        "an empty selector would select every node and is refused"
    );
    assert!(
        ObjectSelector::parse("*").is_err(),
        "a prefix-less wildcard would select every node too and is refused"
    );
    assert!(
        NodePathSelector::parse("tailhook/").is_err(),
        "an empty path step would match nothing and is refused"
    );
    assert!(
        NodePathSelector::parse("a//b").is_err(),
        "an interior empty step is refused too"
    );
}

// ---------------------------------------------------------------------------
// The field families that stop this becoming a world-actor program.
// ---------------------------------------------------------------------------

/// **Every field family this measurement reached is named, with its own claim
/// id, and none of them is ever reported as measured.**
///
/// The task's acceptance has two branches: resolve the members into a program,
/// **or** name every field that cannot be recovered. This is the second branch,
/// and it is load-bearing rather than a disclaimer: the families are what tell a
/// consumer that a missing motion, socket, pickup or tick rate is *the archive
/// not saying it* rather than a reader that dropped a field.
#[test]
fn accept_m01_lc_world_actors_every_unmeasured_field_family_is_named_and_never_measured() {
    assert_eq!(
        UnmeasuredFieldFamily::ALL.len(),
        15,
        "the family list is closed and this is its size"
    );
    let mut labels = std::collections::BTreeSet::new();
    let mut claims = std::collections::BTreeSet::new();
    for family in UnmeasuredFieldFamily::ALL {
        assert!(
            labels.insert(family.label()),
            "the label {:?} is unique",
            family.label()
        );
        assert!(
            claims.insert(family.claim_id()),
            "the claim id {:?} is its own claim",
            family.claim_id()
        );
        assert_eq!(
            family.claim().as_str(),
            family.claim_id(),
            "the validated claim matches the spelling"
        );
        assert_eq!(
            family.absence_provenance().class,
            cs_types::evidence::ClaimStatus::Designed,
            "a gap is never filed as installation-derived"
        );
        assert!(
            family.absence_provenance().source.is_none(),
            "and never as original data, because it locates no bytes: {}",
            family.label()
        );
        assert!(
            !family.is_measured(),
            "{} is a statement about what has not been established",
            family.label()
        );
        assert_eq!(family.origin().label(), "designed");
        assert!(
            !family.reason().is_empty(),
            "{} names what it blocks",
            family.label()
        );
        assert!(
            family.content_id().is_ok(),
            "{} has a usable content id",
            family.label()
        );
    }
    assert_eq!(labels.len(), 15);
    assert!(
        claims.contains("f20-anim.sequence-entry-kinds-measured-not-interpreted"),
        "the statement family is one of the named claims"
    );
    assert!(
        claims.contains("f20-anim.name1-state-binding-resolution-unmeasured"),
        "and the NAME1 state binding has its own claim, not the wildcard's"
    );
    assert!(
        claims.contains("f18-world.stored-vertex-unit-unmeasured"),
        "the stored unit stays task #436's claim, not a new one"
    );
    assert!(
        claims.contains("f34-world.placement-records-unmeasured"),
        "the placement family is filed under the world-actor contract"
    );
}

// ---------------------------------------------------------------------------
// The retail measurement.
// ---------------------------------------------------------------------------

fn retail_root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    )
}

/// Every definition site the three archives of one mission scope declare, plus
/// the member names of the mission archive, read through the production
/// discovery and the production `.zrd` decoder.
fn retail_scope(
    found: &install::Discovery,
    group: &str,
    mission: &str,
) -> (
    Vec<String>,
    Vec<String>,
    Vec<AnimationDefinitionSite>,
    StartupAnimationTable,
) {
    let install_sha256 = install::fingerprint(&found.manifest);
    let mut archives = Vec::new();
    let mut mission_members = Vec::new();
    let mut declarations: Vec<AnimationDefinitionSite> = Vec::new();
    let mut startup: Option<StartupAnimationTable> = None;
    let mission_key = format!("zbd/{group}/{mission}/zrdr.zbd");
    for key in [
        mission_key.clone(),
        format!("zbd/{group}/zrdr.zbd"),
        "zbd/zrdr.zbd".to_owned(),
    ] {
        let Some(record) = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == key)
        else {
            continue;
        };
        archives.push(key.clone());
        let spelling = record.relative_spelling.as_str().to_owned();
        let container_sha256 = record.sha256;
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("archive reads");
        let path = RelativePath::new(&spelling.to_lowercase()).expect("a relative path");
        let discovery = discover_container(&key, &path, &bytes);
        let is_mission = key == mission_key;
        for program in discovery.programs() {
            let locator = program.locator();
            let Some(member) = locator.member().map(str::to_owned) else {
                continue;
            };
            if is_mission {
                mission_members.push(member.clone());
                if member.eq_ignore_ascii_case(STARTUP_MEMBER) {
                    startup = Some(
                        read_startup_animations(program.bytes())
                            .unwrap_or_else(|error| panic!("{key}:{member} reads: {error}")),
                    );
                }
            }
            let span = locator.span();
            let span = SourceSpan::new(
                install_sha256,
                &key,
                Some(&member),
                span.offset,
                span.len,
                Some(container_sha256),
            )
            .expect("the member span is recordable");
            // Only a member whose record *is* the definition record is read as
            // one: the reader refuses anything else, so a member this scope
            // carries for another purpose is skipped rather than forced.
            if !declares_definition_record(program.bytes()) {
                continue;
            }
            let read = read_animation_definition_member(&member, program.bytes())
                .unwrap_or_else(|error| panic!("{key}:{member} reads: {error}"));
            for definition in read.definitions() {
                declarations.push(AnimationDefinitionSite::new(
                    &key,
                    &member,
                    span.clone(),
                    definition.clone(),
                ));
            }
        }
    }
    (
        mission_members,
        archives,
        declarations,
        startup.expect("the mission archive carries a startup member"),
    )
}

/// Whether a member's root record is the `ANIMATION_DEFINITIONS` record.
///
/// The production `.zrd` decoder is asked first, so a member that is not a
/// document at all is skipped rather than refused, and a document whose first
/// record names something else is skipped too. This is the same record-name
/// check the reader performs, done by the caller so the survey can walk a whole
/// archive without treating a non-definition member as a failure.
fn declares_definition_record(bytes: &[u8]) -> bool {
    let Ok(document) = cs_content::stunts::decode_zrd(bytes) else {
        return false;
    };
    document
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdValue::as_list)
        .and_then(|record| record.first())
        .and_then(ZrdValue::as_text)
        == Some(ANIMATION_DEFINITIONS_RECORD)
}

// ---------------------------------------------------------------------------
// Retail: M01's own closure, end to end.
// ---------------------------------------------------------------------------

/// **Every animation M01's startup event table fires resolves to exactly one
/// member, and the measurement is which member and which object selector.**
///
/// This is the acceptance criterion read as a measurement over the installation.
/// `ZBD/C1C/M01/zrdr.zbd` is the archive `missions/bindings/M01.json` names. The
/// counts, the member names, the selectors and the activation are all read out
/// of the bytes by the production readers; nothing here is authored.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_actors_retail_m01_startup_animations_resolve_to_their_members() {
    let found = install::discover(&retail_root()).expect("the installation is discoverable");
    let (members, archives, declarations, table) = retail_scope(&found, "c1c", "m01");

    assert_eq!(
        members.len(),
        12,
        "ZBD/C1C/M01/zrdr.zbd declares twelve members"
    );
    assert_eq!(
        archives.len(),
        3,
        "the mission, its group and the shared root"
    );
    assert_eq!(archives[0], "zbd/c1c/m01/zrdr.zbd");
    assert_eq!(archives[1], "zbd/c1c/zrdr.zbd");
    assert_eq!(archives[2], "zbd/zrdr.zbd");
    assert!(
        members
            .iter()
            .any(|name| name.eq_ignore_ascii_case(STARTUP_MEMBER)),
        "the mission archive carries its startup member"
    );

    let binding = WorldActorProgramBinding::new(
        "zbd/c1c/m01/zrdr.zbd",
        archives,
        members,
        table,
        &declarations,
    );

    assert_eq!(
        binding.startup().len(),
        7,
        "two events, seven animation names"
    );
    assert_eq!(
        binding.startup_of(NEW_GAME_START).count(),
        6,
        "NEW_GAME_START fires six animations"
    );
    assert_eq!(
        binding.startup_of(LOAD_GAME_START).count(),
        1,
        "LOAD_GAME_START fires one"
    );
    assert_eq!(
        binding.unresolved().count(),
        0,
        "every animation M01 fires is declared by exactly one member of the closure: {:?}",
        binding.unresolved().collect::<Vec<_>>()
    );

    // The measured member→actor binding, one row per fired animation.
    let mut resolved: Vec<(String, String, String, String)> = Vec::new();
    for entry in binding.startup() {
        let site = entry
            .resolution()
            .single()
            .unwrap_or_else(|| panic!("{} resolves once", entry.animation_name()));
        assert!(
            matches!(site.objects(), DefinitionObjects::Names(_)),
            "{} names its objects as node names: {:?}",
            entry.animation_name(),
            site.objects()
        );
        assert!(
            site.objects()
                .selectors()
                .all(|selector| selector.literal_name().is_some()),
            "{} selects literal node names: {:?}",
            entry.animation_name(),
            site.objects()
        );
        resolved.push((
            entry.event().to_owned(),
            entry.animation_name().to_owned(),
            site.archive().to_owned(),
            site.member().to_owned(),
        ));
    }
    let expected: Vec<(&str, &str, &str)> = vec![
        (NEW_GAME_START, "generic_intro", "zbd/zrdr.zbd"),
        (NEW_GAME_START, "wv_hookup_state", "zbd/c1c/m01/zrdr.zbd"),
        (NEW_GAME_START, "pzep_engines_start", "zbd/zrdr.zbd"),
        (NEW_GAME_START, "wvzep_engines_start", "zbd/zrdr.zbd"),
        (NEW_GAME_START, "bszep_engines_start", "zbd/zrdr.zbd"),
        (NEW_GAME_START, "call_add_jack", "zbd/zrdr.zbd"),
        (LOAD_GAME_START, "player_setup", "zbd/zrdr.zbd"),
    ];
    assert_eq!(
        resolved
            .iter()
            .map(|(event, animation, archive, _)| (
                event.as_str(),
                animation.as_str(),
                archive.as_str()
            ))
            .collect::<Vec<_>>(),
        expected,
        "the mission's startup fires the measured animations from the measured archives"
    );

    // The one mission-scoped member of the binding: the tailhook the animation
    // drives is the mission's own world machinery, and the shared archive
    // supplies the rest.
    let hookup = resolved
        .iter()
        .find(|(_, animation, _, _)| animation == "wv_hookup_state")
        .expect("wv_hookup_state is fired");
    assert_eq!(hookup.3, "wv_tailhook.zrd");
    for (animation, member) in [
        ("pzep_engines_start", "pirate_zep_nacelles.zrd"),
        ("wvzep_engines_start", "wv_zep_nacelles.zrd"),
        ("bszep_engines_start", "bswan_zep_nacelles.zrd"),
        ("call_add_jack", "passengers.zrd"),
        ("generic_intro", "generic_intro.zrd"),
        ("player_setup", "player_setup.zrd"),
    ] {
        assert_eq!(
            resolved
                .iter()
                .find(|(_, name, _, _)| name == animation)
                .map(|(_, _, _, member)| member.as_str()),
            Some(member),
            "{animation} is declared by {member}"
        );
    }

    // The spawn half: M01 places its capital ships through an ON_STARTUP
    // definition in a member of its own archive.
    let placements: Vec<&str> = binding
        .startup_activations()
        .iter()
        .map(|site| site.member())
        .collect();
    assert!(
        placements.contains(&"placezeps.zrd"),
        "the mission places actors at startup: {placements:?}"
    );
    let placed: Vec<String> = binding
        .startup_activations()
        .iter()
        .filter(|site| site.member().eq_ignore_ascii_case("placezeps.zrd"))
        .flat_map(|site| site.objects().selectors().map(|s| s.stored().to_owned()))
        .collect();
    assert_eq!(
        placed,
        vec!["piratezep", "workersvoyagezep", "blackswanzep"],
        "the placed objects are the three zeppelins the startup animations also drive"
    );

    for site in binding.startup_activations() {
        assert_eq!(
            site.definition().activation().map(Activation::stored),
            Some("ON_STARTUP"),
            "a placement definition is ON_STARTUP: {}",
            site.member()
        );
    }
}

/// **Every object M01's startup animations select resolves to exactly one record
/// of the world container its scope names.**
///
/// This is the "which actor" half of the task's question, measured against the
/// original container: `ZBD/C1C/gamez.zbd` holds one record named `piratezep`,
/// one `workersvoyagezep`, one `blackswanzep`, one `wv_tailhook`, one
/// `apassengers` and one `camera1`, and each is what the definition that fires at
/// startup drives.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_actors_retail_the_selected_objects_resolve_in_the_c1c_world() {
    let found = install::discover(&retail_root()).expect("the installation is discoverable");
    let (_, _, declarations, table) = retail_scope(&found, "c1c", "m01");
    let binding = WorldActorProgramBinding::new(
        "zbd/c1c/m01/zrdr.zbd",
        Vec::new(),
        Vec::new(),
        table,
        &declarations,
    );

    let label = "zbd/c1c/gamez.zbd".to_owned();
    let bytes =
        std::fs::read(retail_root().join("ZBD/C1C/gamez.zbd")).expect("the world container");
    let records = read_gamez_nodes(&mut ParseContext::with_defaults(label.clone()), &bytes)
        .expect("the world container's node array reads");
    let world = WorldNodeNames::from_gamez(label.clone(), &records);
    assert_eq!(world.len(), 5_644, "C1C holds 5 644 records");

    let mut selected: Vec<(String, String)> = Vec::new();
    for (site, selector) in binding.selected_objects() {
        assert!(
            selector.literal_name().is_some(),
            "{} selects a literal node name: {selector}",
            site.member()
        );
        let matched = world.resolve(selector);
        assert_eq!(
            matched.occurrences(),
            Some(1),
            "{} drives exactly one record of {label}: {selector} -> {matched:?}",
            site.member()
        );
        selected.push((site.member().to_owned(), selector.stored().to_owned()));
    }
    assert_eq!(
        selected,
        vec![
            ("generic_intro.zrd".to_owned(), "camera1".to_owned()),
            ("wv_tailhook.zrd".to_owned(), "wv_tailhook".to_owned()),
            ("pirate_zep_nacelles.zrd".to_owned(), "piratezep".to_owned()),
            (
                "wv_zep_nacelles.zrd".to_owned(),
                "workersvoyagezep".to_owned()
            ),
            (
                "bswan_zep_nacelles.zrd".to_owned(),
                "blackswanzep".to_owned()
            ),
            ("passengers.zrd".to_owned(), "apassengers".to_owned()),
            ("player_setup.zrd".to_owned(), "camera1".to_owned()),
        ],
        "seven fired animations, six distinct objects, each in the measured member"
    );

    // And the objects the **mission's own** placements name resolve in the same
    // container, so the spawn statement and the startup statements name one set
    // of actors. The shared and group archives also carry ON_STARTUP
    // definitions, and their objects are placed elsewhere, which is why the
    // check is scoped to the mission archive rather than to the whole closure.
    let placed_in_mission: Vec<&AnimationDefinitionSite> = binding
        .startup_activations()
        .iter()
        .filter(|site| site.archive() == "zbd/c1c/m01/zrdr.zbd")
        .collect();
    assert_eq!(
        placed_in_mission.len(),
        3,
        "the mission archive declares three startup placements"
    );
    for selector in placed_in_mission
        .iter()
        .flat_map(|site| site.objects().selectors())
    {
        assert_eq!(
            world.resolve(selector).occurrences(),
            Some(1),
            "the placed object {selector} is one record of {label}"
        );
    }
}

/// **A wildcard object name resolves in the world container that places the
/// family and in no other, which is what makes it a wildcard reading rather
/// than a spelling.**
///
/// The measured corpus: `g_engine*` is declared by the shared spruce-goose
/// destruction definitions, `ZBD/C2/gamez.zbd` and `ZBD/C5/gamez.zbd` hold
/// `g_engine1..8` and `ZBD/C1C/gamez.zbd` holds none. A reader that treated `*`
/// as part of a node name would select zero everywhere; one that treated it as a
/// literal prefix would select in both worlds. Only the prefix rule gives the
/// measured answer, which is why it is filed as a designed claim with its own id
/// rather than as a measurement of the bytes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_actors_retail_a_wildcard_resolves_only_where_its_family_is_placed() {
    let found = install::discover(&retail_root()).expect("the installation is discoverable");

    // The declaration: which member of the shared archive declares `g_engine*`.
    let (_, _, declarations, _) = retail_scope(&found, "c1c", "m01");
    let declaring: Vec<&AnimationDefinitionSite> = declarations
        .iter()
        .filter(|site| {
            site.objects()
                .selectors()
                .any(|selector| selector.stored() == "g_engine*")
        })
        .collect();
    assert!(
        !declaring.is_empty(),
        "the shared archive declares a g_engine* selector"
    );
    assert!(
        declaring
            .iter()
            .all(|site| site.archive() == "zbd/zrdr.zbd"),
        "and it is the shared root archive that declares it"
    );

    let wildcard = ObjectSelector::parse("g_engine*").expect("the stored selector parses");
    assert!(wildcard.has_wildcard());

    let mut per_container = Vec::new();
    for group in ["C1C", "C2", "C5"] {
        let label = format!("zbd/{}/gamez.zbd", group.to_lowercase());
        let bytes =
            std::fs::read(retail_root().join(format!("ZBD/{group}/gamez.zbd"))).expect("reads");
        let records = read_gamez_nodes(&mut ParseContext::with_defaults(label.clone()), &bytes)
            .expect("the container's node array reads");
        let world = WorldNodeNames::from_gamez(label.clone(), &records);
        per_container.push((label, world.resolve(&wildcard).occurrences()));
    }
    assert_eq!(
        per_container,
        vec![
            ("zbd/c1c/gamez.zbd".to_owned(), Some(0)),
            ("zbd/c2/gamez.zbd".to_owned(), Some(8)),
            ("zbd/c5/gamez.zbd".to_owned(), Some(8)),
        ],
        "the wildcard selects the numbered family where it is placed and nothing where it is not"
    );
    let empty = WorldNodeNames::from_gamez("zbd/none/gamez.zbd", &synthetic_world(&["g_engine1"]));
    assert_eq!(
        empty.resolve(&wildcard).occurrences(),
        Some(1),
        "the rule is prefix matching and nothing else"
    );
}

// ---------------------------------------------------------------------------
// Retail: the census that backs the findings.
// ---------------------------------------------------------------------------

/// **The installation-wide census: the two measured startup events, the
/// measured activation vocabulary and the measured statement kinds, with an
/// unknown value retained rather than refused.**
///
/// The corpus numbers the findings record are re-derived here from the
/// production reader so the claim ids this module files are checkable, and so a
/// value outside the measured vocabulary has a test: `Activation::Other` keeps
/// its spelling, which is what lets the census be a measurement of *this*
/// installation rather than a closed enumeration.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_actors_retail_the_startup_and_activation_vocabulary_is_measured() {
    let found = install::discover(&retail_root()).expect("the installation is discoverable");
    let mut mission_scopes = 0usize;
    let mut reader_archives = 0usize;
    let mut with_startup = 0usize;
    let mut events = std::collections::BTreeSet::new();
    let mut activation = std::collections::BTreeSet::new();
    let mut kinds = std::collections::BTreeSet::new();
    let mut wildcard_definitions = 0usize;
    let mut definition_files = 0usize;
    let mut definitions = 0usize;
    let mut definition_members = 0usize;

    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = RelativePath::new(&spelling.to_lowercase()).expect("a relative path");
        // Every reader archive of the installation, so the activation
        // vocabulary and the statement census cover the shared definitions too
        // and not only the mission scopes' own members.
        if !path.logical_key().ends_with("/zrdr.zbd") && path.logical_key() != "zbd/zrdr.zbd" {
            continue;
        }
        // F13-B's own rule: exactly `zbd/<group>/<mission>`, so neither the
        // shared reader nor a world-group reader counts as a mission.
        let mission = mission_scope(&path).is_some();
        reader_archives += 1;
        if mission {
            mission_scopes += 1;
        }
        let key = path.logical_key().to_owned();
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("reads");
        let discovery = discover_container(&key, &path, &bytes);
        let startup = discovery.programs().iter().find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(STARTUP_MEMBER))
        });
        if mission {
            // A mission archive with no startup member is a measured absence and
            // is counted as such; it is not silently skipped.
            if startup.is_some() {
                with_startup += 1;
            }
            if let Some(startup) = startup {
                let table =
                    read_startup_animations(startup.bytes()).expect("the startup member reads");
                for entry in table.events() {
                    events.insert(entry.event().to_owned());
                }
            }
        }

        for program in discovery.programs() {
            let Some(member) = program.locator().member() else {
                continue;
            };
            let read = read_animation_definition_member(member, program.bytes());
            // A member that declares the definition record must read as one; a
            // member that does not must be refused. Neither is ever skipped, so
            // the census cannot quietly lose eight definitions.
            if !declares_definition_record(program.bytes()) {
                assert!(
                    read.is_err(),
                    "{key}:{member} does not declare ANIMATION_DEFINITIONS, so the reader must \
                     refuse it rather than read it as one"
                );
                continue;
            }
            let read = read.unwrap_or_else(|error| panic!("{key}:{member} reads: {error}"));
            definition_members += 1;
            definition_files += read.definition_files().len();
            for definition in read.definitions() {
                definitions += 1;
                if let Some(value) = definition.activation() {
                    activation.insert(value.stored().to_owned());
                }
                kinds.extend(definition.sequence_kinds().into_iter().map(str::to_owned));
                wildcard_definitions += usize::from(definition.objects().has_wildcard());
            }
        }
    }

    assert_eq!(
        mission_scopes, 53,
        "the installation declares 53 mission reader archives"
    );
    assert_eq!(
        reader_archives, 62,
        "and 62 reader archives in total: the shared root, eight world groups and 53 missions"
    );
    assert_eq!(
        with_startup, 53,
        "every one of the 53 mission archives carries a startup member"
    );
    assert_eq!(
        events.into_iter().collect::<Vec<_>>(),
        vec!["LOAD_GAME_START".to_owned(), "NEW_GAME_START".to_owned()],
        "the startup vocabulary is exactly these two keys"
    );
    assert_eq!(
        activation.into_iter().collect::<Vec<_>>(),
        vec![
            "ON_CALL".to_owned(),
            "ON_STARTUP".to_owned(),
            "WEAPON_OR_COLLIDE_HIT".to_owned(),
        ],
        "the activation vocabulary is exactly these three values, and the family names the \
         possibility of more"
    );
    assert!(
        kinds.len() >= 30,
        "at least thirty statement kinds are in use: {} measured",
        kinds.len()
    );
    assert!(
        kinds.contains("OBJECT_MOTION_FROM_TO"),
        "the motion statement that would carry a route is in use"
    );
    assert!(
        kinds.contains("CALL_ANIMATION"),
        "and the statement that chains one definition to the next"
    );
    assert_eq!(
        definition_members, 446,
        "446 members of the installation declare an ANIMATION_DEFINITIONS record"
    );
    assert_eq!(
        definitions, 1_533,
        "and together they declare 1 533 inline animation definitions"
    );
    assert_eq!(
        definition_files, 828,
        "plus 828 references to the original's authoring paths"
    );
    assert!(
        wildcard_definitions > 0,
        "{} definitions name an object family with '*'",
        wildcard_definitions
    );

    // A value outside the measured three is retained, never refused.
    let unseen = Activation::measured("ON_SOMETHING_ELSE");
    assert_eq!(unseen.stored(), "ON_SOMETHING_ELSE");
    assert!(!unseen.is_measured());
    assert!(!unseen.is_startup());
    assert!(
        !unseen.clone().eq(&Activation::measured("ON_CALL")),
        "an unseen activation is never folded into a measured one"
    );
}
