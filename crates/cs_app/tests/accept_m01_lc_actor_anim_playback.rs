//! Task #678 (`M01-LC-ACTOR-ANIM-PLAYBACK`): the mission animation consumer —
//! one mission scope's startup world actors and animation records, joined and
//! refused, and (task #690, `F20-EVENT-GRAMMAR`) played once their event
//! streams decode.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`
//! (`### F20-D`, non-negotiable behavior 2). Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Findings:
//! `docs/findings/2026-10-05-m01-lc-actor-anim-playback.md` (#678, the join)
//! and `docs/findings/2026-10-06-f20-event-grammar.md` (#690, the grammar,
//! the census and the duration).
//!
//! These tests drive `cs_app::animation::mission` only. Every fixture below is
//! newly authored synthetic data in the **measured shapes** the readers accept,
//! and no value is a claim about the original game: the synthetic cases prove
//! the join, the two name agreements and every refusal rule, and the
//! `#[ignore]`d cases are the ones that measure the installation.
//!
//! The retail half is `ZBD/C1C/M01` (`missions/bindings/M01.json`'s mission),
//! its world group `ZBD/C1C`, the shared `ZBD` root, the two carriers
//! `ZBD/C1C/M01/mis_anim.zbd` and `ZBD/C1C/cam_anim.zbd`, and the world
//! container `ZBD/C1C/gamez.zbd`.

#![allow(clippy::too_many_lines)]

use cs_app::animation::carrier::{UNBOUND_REASON_AMBIGUOUS, UNBOUND_REASON_NO_RECORD};
use cs_app::animation::mission::{
    AMBIGUOUS_DECLARATION_REASON, AnimationRecordFacts, AnimationTarget, DECLARATION_MATCH_CLAIM,
    EVENTS_NOT_DECODED_CLAIM, EVENTS_NOT_DECODED_REASON, OBJECT_NAME_DISAGREES_REASON,
    PLACEMENT_FIELDS_CLAIM, PLACEMENT_FIELDS_REASON, PlayRefusal, RecordResolution, RecordSequence,
    SEQUENCE_NAMES_DISAGREE_REASON, SequenceEvents, StartupAnimation, TargetResolution,
    TargetSource, UNDECLARED_REASON, UNREADABLE_TARGET_REASON, WorldActorPlacement,
    bind_mission_animation, join_startup_animation,
};
use cs_app::animation::programs::{
    ANIMATION_DEFINITION_FIELD, ANIMATION_DEFINITIONS_RECORD, ANIMATION_LIST_FIELD,
    ANIMATION_NAME_FIELD, AnimationDefinitionSite, BindingResolution, LOAD_GAME_START, NAME_FIELD,
    NEW_GAME_START, SEQUENCE_FIELD, SEQUENCE_NAME_FIELD, SelectorMatch, StartupAnimationBinding,
    WorldNodeNames, read_animation_definition_member, read_startup_animations,
};
use cs_app::animation::survey::CarrierKind;
use cs_assets::install;
use cs_formats::gamez::{
    GAMEZ_HEADER_BYTES, GameZHeader, GameZNodes, NODE_TYPE_OBJECT3D, NodeKind, RawNode,
    RawNodeInfo, RawObject3dData,
};
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discover_container;
use cs_formats::zbd::{
    AnimationRecordSequenceKind, GAMEZ_SIGNATURE, GAMEZ_VERSION, ZbdFamily, ZbdProbe, dispatch,
    family_record, read_animation_index,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Provenance;
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::install::RelativePath;

// ---------------------------------------------------------------------------
// Synthetic `.zrd` authoring. Every byte below is authored here, in the
// measured grammar: tag `1` int, `3` text (length then bytes), `4` list
// (`children + 1`), and a record's root as a one-element list holding a flat
// alternating `KEY, value` body.
// ---------------------------------------------------------------------------

/// A `.zrd` text node: tag `3`, the byte length, the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, `children + 1`, then the children.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// A one-element list wrapping one text: the shape the original stores a name
/// list in.
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

/// One `.zrd` `SEQUENCE_DEFINITION`: a record whose body is the sequence's own
/// `NAME` — when it has one — and then its statements.
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

/// One member's whole `ANIMATION_DEFINITIONS` record, holding the given
/// definitions in an `ANIMATION_LIST`.
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

/// A `startanims.zrd` document with one identity list per event.
fn startup_document(events: &[(&str, Vec<&str>)]) -> Vec<u8> {
    let entries: Vec<(&str, Vec<u8>)> = events
        .iter()
        .map(|(key, names)| {
            (
                *key,
                zrd_list(names.iter().copied().map(zrd_name).collect()),
            )
        })
        .collect();
    zrd_record(zrd_flat(entries))
}

// ---------------------------------------------------------------------------
// Synthetic fixtures.
// ---------------------------------------------------------------------------

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a static claim id is valid")
}

fn synthetic_provenance(id: &str) -> Provenance {
    Provenance::new(claim(id), ClaimStatus::ObservedTool, None)
        .expect("an observed-tool claim with no source span is valid")
}

/// A synthetic record's facts, in the shape the consumer reads them back.
struct RecordSpec<'a> {
    anim_name: &'a str,
    object_name: &'a str,
    root_name: &'a str,
    objects: &'a [&'a str],
    nodes: &'a [&'a str],
    animation_refs: &'a [&'a str],
    sequences: Vec<(&'a str, AnimationRecordSequenceKind, u64)>,
}

/// One synthetic record's facts, owned.
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
        objects: spec.objects.iter().map(|name| (*name).to_owned()).collect(),
        nodes: spec.nodes.iter().map(|name| (*name).to_owned()).collect(),
        animation_refs: spec
            .animation_refs
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
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

fn synthetic_hash() -> ContentHash {
    ContentHash::from_bytes([7u8; 32])
}

/// One declaration site, as `programs::read_animation_definition_member`
/// produces it from a synthetic member.
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

// ---------------------------------------------------------------------------
// The join and its two name agreements.
// ---------------------------------------------------------------------------

/// The observable failure this module exists to prevent: a consumer that pairs
/// a startup identity with a record because their **names** agree, without
/// checking that the record's own object and sequences agree with the member
/// that declares the animation. A pair that does not agree is refused, both
/// spellings kept, and nothing is played.
#[test]
fn accept_m01_lc_actor_anim_playback_a_pair_is_joint_only_when_both_names_agree() {
    // The measured shape of `pzep_engines_start`: one selector, one sequence,
    // and a record that agrees on both.
    let site = site(
        "zbd/zrdr.zbd",
        "pirate_zep_nacelles.zrd",
        "pzep_engines_start",
        vec!["piratezep"],
        vec![Some("call_eachengine")],
    );
    let facts = record(
        &RecordSpec {
            anim_name: "pzep_engines_start",
            object_name: "piratezep",
            root_name: "piratezep",
            objects: &[],
            nodes: &[],
            animation_refs: &[],
            sequences: vec![(
                "call_eachengine",
                AnimationRecordSequenceKind::Sequence,
                960,
            )],
        },
        CarrierKind::Mission,
        36,
    );
    let joined = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "pzep_engines_start",
            BindingResolution::Single(Box::new(site.clone())),
        ),
        bound(facts),
        None,
    );

    // The two name agreements hold, so neither disagreement is reported...
    assert!(
        joined
            .refusals
            .iter()
            .all(|refusal| refusal.label() != "object_name_disagrees"),
        "an agreeing pair must not report an object disagreement: {joined:?}"
    );
    assert!(
        joined
            .refusals
            .iter()
            .all(|refusal| refusal.label() != "sequence_names_disagree"),
        "an agreeing pair must not report a sequence disagreement: {joined:?}"
    );
    // ...and the record is still not played, because its events are bytes.
    assert!(!joined.is_playable());
    assert_eq!(
        joined.refusals.len(),
        1,
        "an agreeing pair is refused for exactly one reason: {joined:?}"
    );
    let PlayRefusal::EventsNotDecoded {
        reason,
        claim_id,
        offset,
    } = joined.refusals[0].clone()
    else {
        panic!("the only refusal of an agreeing pair is the event gap: {joined:?}");
    };
    assert_eq!(reason, EVENTS_NOT_DECODED_REASON);
    assert_eq!(claim_id.as_str(), EVENTS_NOT_DECODED_CLAIM);
    assert_eq!(
        offset, None,
        "bytes this consumer does not hold have no offset to quote"
    );

    // The measured disagreement (`zbd/zrdr.zbd::autogyro_bus.zrd` declares
    // `agyrobus` over `agyro_rotors`, the record stores `autogyro`): reported
    // with both spellings, never repaired.
    let mismatched = site.clone();
    let record_facts = record(
        &RecordSpec {
            anim_name: "pzep_engines_start",
            object_name: "autogyro",
            root_name: "autogyro",
            objects: &[],
            nodes: &[],
            animation_refs: &[],
            sequences: vec![(
                "call_eachengine",
                AnimationRecordSequenceKind::Sequence,
                960,
            )],
        },
        CarrierKind::Mission,
        36,
    );
    let refused = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "pzep_engines_start",
            BindingResolution::Single(Box::new(mismatched)),
        ),
        bound(record_facts),
        None,
    );
    let Some(PlayRefusal::ObjectNameDisagrees {
        reason,
        declared,
        stored,
    }) = refused
        .refusals
        .iter()
        .find(|refusal| refusal.label() == "object_name_disagrees")
        .cloned()
    else {
        panic!("a disagreeing object name is refused: {refused:?}");
    };
    assert_eq!(reason, OBJECT_NAME_DISAGREES_REASON);
    assert_eq!(declared, vec!["piratezep".to_owned()]);
    assert_eq!(stored, "autogyro");

    // A sequence that does not agree is a separate refusal, and the two lists
    // are reported side by side rather than paired by position.
    let record_facts = record(
        &RecordSpec {
            anim_name: "pzep_engines_start",
            object_name: "piratezep",
            root_name: "piratezep",
            objects: &[],
            nodes: &[],
            animation_refs: &[],
            sequences: vec![
                ("other_sequence", AnimationRecordSequenceKind::Sequence, 120),
                (
                    "call_eachengine",
                    AnimationRecordSequenceKind::Sequence,
                    960,
                ),
            ],
        },
        CarrierKind::Mission,
        36,
    );
    let reordered = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "pzep_engines_start",
            BindingResolution::Single(Box::new(site)),
        ),
        bound(record_facts),
        None,
    );
    let Some(PlayRefusal::SequenceNamesDisagree {
        reason,
        declared,
        stored,
    }) = reordered
        .refusals
        .iter()
        .find(|refusal| refusal.label() == "sequence_names_disagree")
        .cloned()
    else {
        panic!("a reordered sequence list is refused: {reordered:?}");
    };
    assert_eq!(reason, SEQUENCE_NAMES_DISAGREE_REASON);
    assert_eq!(declared, vec![Some("call_eachengine".to_owned())]);
    assert_eq!(
        stored,
        vec!["other_sequence".to_owned(), "call_eachengine".to_owned()]
    );
}

/// The other half of the agreement: an unnamed `.zrd` sequence pairs with an
/// empty stored name (the measured shape of `wv_hookup_state`, whose single
/// sequence the member leaves unnamed and whose record block is named ``).
/// Treating the empty name as "no name" would break that pair.
#[test]
fn accept_m01_lc_actor_anim_playback_an_unnamed_sequence_agrees_with_an_empty_stored_name() {
    let site = site(
        "zbd/c1c/m01/zrdr.zbd",
        "wv_tailhook.zrd",
        "wv_hookup_state",
        vec!["wv_tailhook"],
        vec![None],
    );
    let facts = record(
        &RecordSpec {
            anim_name: "wv_hookup_state",
            object_name: "wv_tailhook",
            root_name: "wv_tailhook",
            objects: &[],
            nodes: &[],
            animation_refs: &[],
            sequences: vec![("", AnimationRecordSequenceKind::Sequence, 240)],
        },
        CarrierKind::Mission,
        496,
    );
    let joined = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "wv_hookup_state",
            BindingResolution::Single(Box::new(site)),
        ),
        bound(facts),
        None,
    );
    assert!(
        joined
            .refusals
            .iter()
            .all(|refusal| refusal.label() != "sequence_names_disagree"),
        "an unnamed sequence agrees with an empty stored name: {joined:?}"
    );
    assert_eq!(joined.refusals.len(), 1, "{joined:?}");
    assert!(!joined.is_playable());
}

/// The four failures stay distinct. A consumer that collapsed them would report
/// a missing record where the data is contradictory, and would lose the source
/// locator F20 behavior 2 requires.
#[test]
fn accept_m01_lc_actor_anim_playback_the_four_failures_stay_distinct() {
    let undeclared = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "nobody_declares_this",
            BindingResolution::Unresolved,
        ),
        RecordResolution::Unbound {
            reason: UNBOUND_REASON_NO_RECORD,
            matches: Vec::new(),
        },
        None,
    );
    let Some(PlayRefusal::Undeclared { reason }) = undeclared.refusals.first().cloned() else {
        panic!("an undeclared name is refused as undeclared: {undeclared:?}");
    };
    assert_eq!(reason, UNDECLARED_REASON);

    let ambiguous_declaration = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "pzep_engines_start",
            BindingResolution::Ambiguous(vec![
                site(
                    "zbd/zrdr.zbd",
                    "a.zrd",
                    "pzep_engines_start",
                    vec!["piratezep"],
                    vec![],
                ),
                site(
                    "zbd/zrdr.zbd",
                    "b.zrd",
                    "pzep_engines_start",
                    vec!["piratezep"],
                    vec![],
                ),
            ]),
        ),
        RecordResolution::Unbound {
            reason: UNBOUND_REASON_AMBIGUOUS,
            matches: vec![(CarrierKind::Mission, 36), (CarrierKind::Camera, 12)],
        },
        None,
    );
    let Some(PlayRefusal::AmbiguousDeclaration { reason, sites }) =
        ambiguous_declaration.refusals.first().cloned()
    else {
        panic!("two declaring members are an ambiguity: {ambiguous_declaration:?}");
    };
    assert_eq!(reason, AMBIGUOUS_DECLARATION_REASON);
    assert_eq!(sites, 2);

    let no_record = join_startup_animation(
        declaration(NEW_GAME_START, "pure_panic", BindingResolution::Unresolved),
        RecordResolution::Unbound {
            reason: UNBOUND_REASON_NO_RECORD,
            matches: Vec::new(),
        },
        None,
    );
    let Some(PlayRefusal::NoRecord { reason, matches }) = no_record.refusals.last().cloned() else {
        panic!("an unbound identity is refused: {no_record:?}");
    };
    assert_eq!(reason, UNBOUND_REASON_NO_RECORD);
    assert!(matches.is_empty());

    let ambiguous_record = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "pzep_engines_start",
            BindingResolution::Single(Box::new(site(
                "zbd/zrdr.zbd",
                "pirate_zep_nacelles.zrd",
                "pzep_engines_start",
                vec!["piratezep"],
                vec![],
            ))),
        ),
        RecordResolution::Unbound {
            reason: UNBOUND_REASON_AMBIGUOUS,
            matches: vec![(CarrierKind::Mission, 36), (CarrierKind::Mission, 99)],
        },
        None,
    );
    let Some(PlayRefusal::NoRecord { reason, matches }) = ambiguous_record.refusals.last().cloned()
    else {
        panic!("an ambiguous record is refused: {ambiguous_record:?}");
    };
    assert_eq!(reason, UNBOUND_REASON_AMBIGUOUS);
    assert_eq!(matches.len(), 2, "every match is kept, not just the first");

    // Four refusals, four different labels, and none of them claims a claim id
    // except the event gap.
    for refusal in ambiguous_record
        .refusals
        .iter()
        .chain(undeclared.refusals.iter())
    {
        assert!(
            refusal.claim_id().is_none(),
            "only the event-stream gap carries a claim: {refusal}"
        );
    }
}

// ---------------------------------------------------------------------------
// The world-node half.
// ---------------------------------------------------------------------------

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

/// Every stored name the consumer collects keeps its **source**, and every name
/// is resolved against the world's own record names — including the empty entry
/// that heads every non-empty reference table, which is reported as unreadable
/// rather than dropped.
#[test]
fn accept_m01_lc_actor_anim_playback_every_name_keeps_its_source_and_its_resolution() {
    let world = WorldNodeNames::from_gamez(
        "zbd/c1c/gamez.zbd",
        &synthetic_world(&["piratezep", "wv_tailhook", "camera1"]),
    );
    let site = site(
        "zbd/zrdr.zbd",
        "pirate_zep_nacelles.zrd",
        "pzep_engines_start",
        vec!["piratezep"],
        vec![Some("call_eachengine")],
    );
    let facts = record(
        &RecordSpec {
            anim_name: "pzep_engines_start",
            object_name: "piratezep",
            root_name: "piratezep",
            objects: &["", "piratezep"],
            nodes: &["", "piratezep", "wv_tailhook"],
            animation_refs: &[],
            sequences: vec![(
                "call_eachengine",
                AnimationRecordSequenceKind::Sequence,
                960,
            )],
        },
        CarrierKind::Mission,
        36,
    );
    let joined = join_startup_animation(
        declaration(
            NEW_GAME_START,
            "pzep_engines_start",
            BindingResolution::Single(Box::new(site)),
        ),
        bound(facts),
        Some(&world),
    );

    // Declared selector, record object, record root and both node-table entries:
    // 1 + 1 + 1 + 3, in that order. Nothing is merged or dropped.
    let sources: Vec<(TargetSource, &str)> = joined
        .targets()
        .iter()
        .map(|target| (target.source, target.stored()))
        .collect();
    assert_eq!(
        sources,
        vec![
            (TargetSource::DeclaredSelector, "piratezep"),
            (TargetSource::RecordObject, "piratezep"),
            (TargetSource::RecordRoot, "piratezep"),
            (TargetSource::RecordNode, ""),
            (TargetSource::RecordNode, "piratezep"),
            (TargetSource::RecordNode, "wv_tailhook"),
        ]
    );

    // The three real names each select exactly one world record...
    let piratezep = &joined.targets()[1];
    assert!(
        piratezep.resolution().occurrences() == Some(1),
        "piratezep resolves once in the synthetic world: {piratezep:?}"
    );
    assert!(!piratezep.resolution().is_unmeasured());
    assert_eq!(
        piratezep.resolution().occurrences(),
        joined.targets()[0].resolution().occurrences()
    );
    assert_eq!(joined.targets()[4].resolution().occurrences(), Some(1));
    assert_eq!(joined.targets()[5].resolution().occurrences(), Some(1));

    // ...and the table's empty zero entry is kept and reported, never counted
    // as selecting nothing.
    let empty = &joined.targets()[3];
    assert!(
        matches!(
            &empty.resolution(),
            TargetResolution::Unreadable {
                stored,
                reason: UNREADABLE_TARGET_REASON,
            } if stored.is_empty()
        ),
        "the empty zero entry is unreadable, not a zero match: {empty:?}"
    );
    assert!(empty.resolution().is_unmeasured());
    assert_eq!(empty.resolution().occurrences(), None);

    // Five of the six targets resolved; the refused one is the empty entry.
    assert_eq!(joined.resolved_targets().count(), 5);

    // A record with no container at all keeps every name and refuses to count
    // any of them: "no container" is never "nothing there".
    let no_container = AnimationTarget::resolve(TargetSource::RecordNode, "piratezep", None);
    assert!(matches!(
        no_container.resolution(),
        TargetResolution::Unreadable { .. }
    ));
    assert_eq!(no_container.resolution().occurrences(), None);
}

/// A wildcard selector keeps its measured reach: a prefix selector reports how
/// many world records carry it, and a narrowing suffix reports **no** count at
/// all (the finding's `lbroad*1`), so the consumer can never report a
/// guess wearing a measurement's clothes.
#[test]
fn accept_m01_lc_actor_anim_playback_a_wildcard_reports_its_reach_and_a_narrowing_suffix_reports_none()
 {
    let world = WorldNodeNames::from_gamez(
        "zbd/c1c/gamez.zbd",
        &synthetic_world(&["lbroad1", "lbroad2", "lbroad3"]),
    );
    let family = AnimationTarget::resolve(TargetSource::DeclaredSelector, "lbroad*", Some(&world));
    let TargetResolution::Selected(SelectorMatch::Family { occurrences, .. }) = family.resolution()
    else {
        panic!("a prefix selector is a family match: {family:?}");
    };
    assert_eq!(
        *occurrences, 3,
        "all three numbered siblings carry the prefix"
    );

    let narrowing =
        AnimationTarget::resolve(TargetSource::DeclaredSelector, "lbroad*1", Some(&world));
    let TargetResolution::Selected(SelectorMatch::UnmeasuredSuffix { .. }) = narrowing.resolution()
    else {
        panic!("a narrowing suffix is unmeasured, not counted: {narrowing:?}");
    };
    assert_eq!(narrowing.resolution().occurrences(), None);
    assert!(narrowing.resolution().is_unmeasured());

    let literal = AnimationTarget::resolve(TargetSource::DeclaredSelector, "lbroad1", Some(&world));
    assert_eq!(literal.resolution().occurrences(), Some(1));
}

// ---------------------------------------------------------------------------
// The world-actor half.
// ---------------------------------------------------------------------------

/// The mission's own archive states which world actors exist before the mission
/// runs. The consumer carries them with their selectors resolved and refuses to
/// spawn them, by claim and by reason — the placement fields are undecoded and
/// the world's unit is unmeasured.
#[test]
fn accept_m01_lc_actor_anim_playback_a_placement_names_its_world_actor_and_refuses_to_place_it() {
    let world = WorldNodeNames::from_gamez(
        "zbd/c1c/gamez.zbd",
        &synthetic_world(&["piratezep", "workersvoyagezep", "blackswanzep"]),
    );
    let placement = WorldActorPlacement {
        archive: "zbd/c1c/m01/zrdr.zbd".to_owned(),
        member: "placezeps.zrd".to_owned(),
        definition: 0,
        animation_name: Some("pzep_engines_start".to_owned()),
        targets: vec![
            AnimationTarget::resolve(TargetSource::DeclaredSelector, "piratezep", Some(&world)),
            AnimationTarget::resolve(
                TargetSource::DeclaredSelector,
                "workersvoyagezep",
                Some(&world),
            ),
            AnimationTarget::resolve(TargetSource::DeclaredSelector, "blackswanzep", Some(&world)),
        ],
        claim_id: claim(PLACEMENT_FIELDS_CLAIM),
    };

    assert_eq!(placement.archive(), "zbd/c1c/m01/zrdr.zbd");
    assert_eq!(placement.member(), "placezeps.zrd");
    assert_eq!(placement.definition(), 0);
    assert_eq!(placement.animation_name(), Some("pzep_engines_start"));
    assert_eq!(placement.claim_id().as_str(), PLACEMENT_FIELDS_CLAIM);
    assert_eq!(placement.unplaced_reason(), PLACEMENT_FIELDS_REASON);
    // Each of the three capital ships resolves to exactly one world record, so
    // the actor's identity is measured even though its placement is not.
    for target in placement.targets() {
        assert_eq!(
            target.resolution().occurrences(),
            Some(1),
            "{target:?} names one world record"
        );
        assert_eq!(target.source(), TargetSource::DeclaredSelector);
    }

    // The claim is not `verified_original` and no source span is invented for
    // it: a designed gap locates nothing.
    assert_eq!(placement.claim_id.as_str(), PLACEMENT_FIELDS_CLAIM);
}

// ---------------------------------------------------------------------------
// Synthetic: the run/seam a mission uses.
// ---------------------------------------------------------------------------

/// The joined rows keep the startup table's events and stored order: the
/// identities the production reader yields, joined one per row, carry their
/// own refusals — a bound row is refused for exactly the event gap, and the
/// unbound ones add the record refusal. The `run` seam that filters this set
/// per event is covered by the in-module test (the binding's fields stay
/// private) and exercised end to end by the retail case.
#[test]
fn accept_m01_lc_actor_anim_playback_the_rows_keep_their_events_in_stored_order() {
    // Read the startup table through the production reader, then join each row
    // the way `bind_mission_animation` does.
    let startup = read_startup_animations(&startup_document(&[
        (
            NEW_GAME_START,
            vec!["pzep_engines_start", "wvzep_engines_start"],
        ),
        (LOAD_GAME_START, vec!["player_setup"]),
    ]))
    .expect("the synthetic startup document reads");
    assert_eq!(startup.events().len(), 2);
    assert_eq!(startup.event(NEW_GAME_START).map(<[String]>::len), Some(2));
    assert_eq!(startup.animation_names().count(), 3);

    let known = site(
        "zbd/zrdr.zbd",
        "pirate_zep_nacelles.zrd",
        "pzep_engines_start",
        vec!["piratezep"],
        vec![Some("call_eachengine")],
    );
    let rows: Vec<StartupAnimation> = startup
        .events()
        .iter()
        .flat_map(|entry| {
            let known = known.clone();
            entry.animation_names().iter().map(move |name| {
                let resolution = if *name == "pzep_engines_start" {
                    BindingResolution::Single(Box::new(known.clone()))
                } else {
                    BindingResolution::Unresolved
                };
                let record = if *name == "pzep_engines_start" {
                    bound(record(
                        &RecordSpec {
                            anim_name: "pzep_engines_start",
                            object_name: "piratezep",
                            root_name: "piratezep",
                            objects: &[],
                            nodes: &[],
                            animation_refs: &[],
                            sequences: vec![(
                                "call_eachengine",
                                AnimationRecordSequenceKind::Sequence,
                                960,
                            )],
                        },
                        CarrierKind::Mission,
                        36,
                    ))
                } else {
                    RecordResolution::Unbound {
                        reason: UNBOUND_REASON_NO_RECORD,
                        matches: Vec::new(),
                    }
                };
                join_startup_animation(declaration(entry.event(), name, resolution), record, None)
            })
        })
        .collect();

    assert_eq!(rows.len(), 3);
    let identities: Vec<&str> = rows.iter().map(StartupAnimation::identity).collect();
    assert_eq!(
        identities,
        vec!["pzep_engines_start", "wvzep_engines_start", "player_setup"],
        "the joined rows keep every event's identities in stored order"
    );
    // One row is joined and refused only for its events; two are refused twice.
    assert_eq!(rows[0].refusals().len(), 1);
    assert_eq!(rows[1].refusals().len(), 2);
    assert_eq!(rows[2].refusals().len(), 2);
    assert!(rows.iter().all(|row| !row.is_playable()));
    assert!(rows[0].bound_record().is_some());
    assert!(rows[1].bound_record().is_none());
    assert_eq!(rows[0].bound_record().map(|facts| facts.index()), Some(36));
    assert_eq!(
        rows[0].bound_record().map(|facts| facts.carrier()),
        Some(CarrierKind::Mission)
    );
    assert_eq!(
        rows[0]
            .bound_record()
            .map(|facts| facts.ordinary_sequences().count()),
        Some(1)
    );
}

// ---------------------------------------------------------------------------
// Retail: M01's own closure.
// ---------------------------------------------------------------------------

fn retail_root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    )
}

const M01: &str = "zbd/c1c/m01";

/// **M01's startup animations are joined and played**: seven identities across
/// two events, every one resolved to exactly one declaring member and exactly
/// one carrier record, both name agreements holding for all seven, every world
/// name resolving, and every row playable with its decoded duration and its
/// per-tick pose report (the refusal this row set carried before #690 measured
/// the event grammar is gone because the bytes now decode, not because the
/// check was relaxed).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_actor_anim_playback_retail_m01_startup_animations_are_joined_and_played() {
    let binding = bind_mission_animation(&retail_root(), M01).expect("M01 binds");

    assert_eq!(binding.scope(), M01);
    assert_eq!(binding.group(), "c1c");
    assert_eq!(
        binding.archives(),
        [
            "zbd/c1c/m01/zrdr.zbd".to_owned(),
            "zbd/c1c/zrdr.zbd".to_owned(),
            "zbd/zrdr.zbd".to_owned(),
        ]
    );
    assert_eq!(binding.world_container(), "zbd/c1c/gamez.zbd");
    assert_eq!(
        binding.provenance().claim_id.as_str(),
        DECLARATION_MATCH_CLAIM
    );
    assert_eq!(
        binding.provenance().class,
        ClaimStatus::ObservedTool,
        "the join is measured over the installation, never verified_original"
    );

    // Two carriers, both walked, with their declared counts.
    let kinds: Vec<CarrierKind> = binding.carriers().iter().map(|fact| fact.kind).collect();
    assert_eq!(kinds, vec![CarrierKind::Mission, CarrierKind::Camera]);
    for fact in binding.carriers() {
        assert!(fact.is_walked(), "{fact:?} walked with no blocker");
        assert!(fact.blockers().is_empty());
        assert_eq!(
            fact.record_count(),
            usize::from(fact.declared_record_count()),
            "{fact:?} reached its declared record count"
        );
    }
    assert_eq!(
        binding
            .carriers()
            .iter()
            .map(|fact| fact.record_count)
            .sum::<usize>(),
        880,
        "M01's mission carrier (573) and C1C's camera carrier (307)"
    );

    // Seven identities: six on `NEW_GAME_START`, one on `LOAD_GAME_START`.
    assert_eq!(binding.startup().len(), 7);
    assert_eq!(binding.startup_of(NEW_GAME_START).len(), 6);
    assert_eq!(binding.startup_of(LOAD_GAME_START).len(), 1);
    // `run` is the seam a mission asks: the same identities in the same order,
    // and an event the table does not declare is an empty run, never a failure.
    let new_game = binding.run(NEW_GAME_START);
    assert_eq!(new_game.event(), NEW_GAME_START);
    assert_eq!(new_game.len(), 6);
    assert_eq!(
        new_game.playable().count(),
        6,
        "every NEW_GAME_START row decoded"
    );
    assert_eq!(new_game.refused().count(), 0);
    assert_eq!(binding.run(LOAD_GAME_START).len(), 1);
    assert!(
        binding.run("NO_SUCH_EVENT").is_empty(),
        "an event the table does not declare is measured content, not a failure"
    );
    let identities: Vec<(&str, &str)> = binding
        .startup()
        .iter()
        .map(|row| (row.event(), row.identity()))
        .collect();
    assert_eq!(
        identities,
        vec![
            (NEW_GAME_START, "generic_intro"),
            (NEW_GAME_START, "wv_hookup_state"),
            (NEW_GAME_START, "pzep_engines_start"),
            (NEW_GAME_START, "wvzep_engines_start"),
            (NEW_GAME_START, "bszep_engines_start"),
            (NEW_GAME_START, "call_add_jack"),
            (LOAD_GAME_START, "player_setup"),
        ],
        "the startup table's stored order, measured by #632 and #650"
    );

    // Every identity resolved to exactly one member and exactly one record, and
    // the records are the ones #650 measured.
    let located: Vec<(&str, &str, &str, CarrierKind, usize)> = binding
        .startup()
        .iter()
        .map(|row| {
            let site = row
                .declaration()
                .resolution()
                .single()
                .unwrap_or_else(|| panic!("{} resolves: {row:?}", row.identity()));
            let facts = row
                .bound_record()
                .unwrap_or_else(|| panic!("{} binds to a record: {row:?}", row.identity()));
            (
                site.archive(),
                site.member(),
                facts.anim_name(),
                facts.carrier,
                facts.index,
            )
        })
        .collect();
    assert_eq!(
        located,
        vec![
            (
                "zbd/zrdr.zbd",
                "generic_intro.zrd",
                "generic_intro",
                CarrierKind::Camera,
                23
            ),
            (
                "zbd/c1c/m01/zrdr.zbd",
                "wv_tailhook.zrd",
                "wv_hookup_state",
                CarrierKind::Mission,
                496
            ),
            (
                "zbd/zrdr.zbd",
                "pirate_zep_nacelles.zrd",
                "pzep_engines_start",
                CarrierKind::Mission,
                36
            ),
            (
                "zbd/zrdr.zbd",
                "wv_zep_nacelles.zrd",
                "wvzep_engines_start",
                CarrierKind::Mission,
                414
            ),
            (
                "zbd/zrdr.zbd",
                "bswan_zep_nacelles.zrd",
                "bszep_engines_start",
                CarrierKind::Mission,
                230
            ),
            (
                "zbd/zrdr.zbd",
                "passengers.zrd",
                "call_add_jack",
                CarrierKind::Camera,
                60
            ),
            (
                "zbd/zrdr.zbd",
                "player_setup.zrd",
                "player_setup",
                CarrierKind::Camera,
                115
            ),
        ]
    );

    // Both name agreements hold for all seven, and every one of their event
    // streams walks under the measured grammar (#690): all seven are playable,
    // each with its own decoded duration and per-tick pose report.
    assert_eq!(
        binding.playable_count(),
        7,
        "every joined record's events decode, so every row plays"
    );
    assert_eq!(binding.refused_count(), 0);
    for row in binding.startup() {
        assert!(
            row.refusals().is_empty(),
            "{} plays for exactly no reason: {row:?}",
            row.identity()
        );
        let playback = row.playback().expect("a playable row carries a playback");
        assert!(
            playback.duration_time() >= 0.0,
            "{}: a measured duration is never negative",
            row.identity()
        );
        assert!(
            playback.events().next().is_some(),
            "{}: the record states at least one statement",
            row.identity()
        );
        assert!(
            !playback.poses(30).is_empty(),
            "{}: the per-tick report covers the record's own timeline",
            row.identity()
        );
    }

    // The refusals keep their source locator: every bound record's span points
    // into its own carrier, and the span's length is the record's own length.
    for row in binding.startup() {
        let facts = row.bound_record().expect("every row binds");
        assert_eq!(
            facts.span().container_path(),
            facts.carrier_key,
            "the span names the carrier the record came from"
        );
        assert!(
            facts.span().length() >= 272,
            "a record is at least its fixed part"
        );
        assert!(facts.span().offset() >= facts.span().length());
        assert_eq!(
            facts.provenance().claim_id.as_str(),
            DECLARATION_MATCH_CLAIM,
            "the facts carry the claim they were read under"
        );
        assert_eq!(facts.root_name(), facts.object_name());
    }

    // Every world name these seven animations address resolves in the mission's
    // world container, apart from the empty zero entry of each reference table,
    // which is reported as unreadable rather than counted as nothing.
    let mut targets = 0_usize;
    let mut unreadable = 0_usize;
    for row in binding.startup() {
        for target in row.targets() {
            targets += 1;
            if matches!(target.resolution(), TargetResolution::Unreadable { .. }) {
                assert!(
                    target.stored().is_empty(),
                    "only the zero entry is unreadable"
                );
                unreadable += 1;
            } else {
                assert!(
                    target.resolution().occurrences().is_some(),
                    "{} names {target:?}, which resolves in C1C",
                    row.identity()
                );
            }
        }
    }
    assert!(
        targets > 7,
        "the seven animations address several names each"
    );
    assert_eq!(
        binding.world_targets().count(),
        targets,
        "the join view yields every row's targets exactly once"
    );
    assert_eq!(
        unreadable, 3,
        "`generic_intro`, `player_setup` and `call_add_jack` are the three records whose node \
         table starts with the empty zero entry, measured here as three unreadable targets"
    );

    // `generic_intro` is the camera-carrier record that names the objects the
    // camera animation drives, measured here as its own node table.
    let generic = binding.animation("generic_intro").expect("the row exists");
    let generic_record = generic.bound_record().expect("the record exists");
    assert_eq!(generic_record.object_name(), "camera1");
    assert_eq!(
        generic_record
            .nodes()
            .iter()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect::<Vec<String>>(),
        vec![
            "player",
            "piratezep",
            "world1",
            "camera1",
            "player_balmoral",
            "player_warhawk",
            "piratefighter",
            "interior",
            "front_door_left",
            "front_door_right",
            "healthy",
            "cockpit1",
        ]
    );
    assert_eq!(
        generic_record.animation_refs(),
        [
            "apzep_engines_start",
            "letterbox",
            "gi_scene1",
            "gi_scene2",
            "gi_1stperson",
            "gi_playerdrop",
        ],
        "the animations this record calls, named by its own reference table"
    );
    // Its six `.zrd` sequences and its six ordinary record blocks agree by
    // name, which is the second agreement.
    let generic_site = generic
        .declaration()
        .resolution()
        .single()
        .expect("the member resolves");
    assert_eq!(
        generic_site
            .definition()
            .sequences()
            .iter()
            .map(|sequence| sequence.name().unwrap_or_default().to_owned())
            .collect::<Vec<String>>(),
        generic_record
            .ordinary_sequences()
            .map(|block| block.name().to_owned())
            .collect::<Vec<String>>(),
        "the declaration's sequence names are the record's ordinary block names"
    );
    assert_eq!(
        generic_record
            .sequence(AnimationRecordSequenceKind::Reset)
            .map(|block| block.name()),
        Some("RESET_SEQUENCE"),
        "the record carries a reset block, named as every retail record's is"
    );

    // The mission's own placements: three capital ships, each one world record.
    assert_eq!(binding.placements().len(), 3);
    let placed: Vec<(&str, &str, Option<usize>)> = binding
        .placements()
        .iter()
        .map(|placement| {
            (
                placement.member(),
                placement.archive(),
                placement
                    .targets()
                    .first()
                    .and_then(|t| t.resolution().occurrences()),
            )
        })
        .collect();
    assert_eq!(
        placed,
        vec![
            ("placezeps.zrd", "zbd/c1c/m01/zrdr.zbd", Some(1)),
            ("placezeps.zrd", "zbd/c1c/m01/zrdr.zbd", Some(1)),
            ("placezeps.zrd", "zbd/c1c/m01/zrdr.zbd", Some(1)),
        ],
        "M01 places three capital ships, each naming exactly one world record"
    );
    for placement in binding.placements() {
        assert_eq!(placement.claim_id().as_str(), PLACEMENT_FIELDS_CLAIM);
        assert_eq!(placement.unplaced_reason(), PLACEMENT_FIELDS_REASON);
        assert_eq!(
            placement.targets()[0].stored(),
            match placement.definition() {
                0 => "piratezep",
                1 => "workersvoyagezep",
                _ => "blackswanzep",
            }
        );
    }
}

/// One declaration of the closure, reduced to what the census compares.
struct ClosureDeclaration {
    member: String,
    animation_name: String,
    selectors: Vec<String>,
    sequences: Vec<Option<String>>,
}

/// One animation record of the two carriers, reduced to what the census
/// compares.
type ClosureRecord = (String, String, Vec<String>);

/// The corpus-wide figure the finding records: 280 declaration/record pairs over
/// M01's closure, 280 sequence agreements, 279 object agreements, and the single
/// disagreement being `agyro_rotors` (`agyrobus` declared, `autogyro` stored).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_actor_anim_playback_retail_the_closure_has_one_object_disagreement() {
    let found = install::discover(&retail_root()).expect("the installation is discoverable");

    // Every declaration the closure declares, and every record the two carriers
    // hold, both read through production readers.
    let mut declarations: Vec<ClosureDeclaration> = Vec::new();
    for key in [
        "zbd/c1c/m01/zrdr.zbd".to_owned(),
        "zbd/c1c/zrdr.zbd".to_owned(),
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
        let spelling = record.relative_spelling.as_str().to_owned();
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("reads");
        let path = RelativePath::new(&spelling.to_lowercase()).expect("a relative path");
        for program in discover_container(&key, &path, &bytes).programs() {
            let locator = program.locator();
            let Some(member) = locator.member().map(str::to_owned) else {
                continue;
            };
            let Ok(read) = read_animation_definition_member(&member, program.bytes()) else {
                continue;
            };
            for definition in read.definitions() {
                let Some(name) = definition.animation_name() else {
                    continue;
                };
                declarations.push(ClosureDeclaration {
                    member: member.clone(),
                    animation_name: name.to_owned(),
                    selectors: definition
                        .objects()
                        .selectors()
                        .map(|selector| selector.stored().to_owned())
                        .collect(),
                    sequences: definition
                        .sequences()
                        .iter()
                        .map(|sequence| sequence.name().map(str::to_owned))
                        .collect(),
                });
            }
        }
    }
    assert_eq!(
        declarations.len(),
        395,
        "the closure's declarations that name an animation, over the 896 definition sites"
    );

    // Every record of the two carriers, by identity.
    let mut records: Vec<ClosureRecord> = Vec::new();
    for key in [
        "zbd/c1c/m01/mis_anim.zbd".to_owned(),
        "zbd/c1c/cam_anim.zbd".to_owned(),
    ] {
        let Some(record) = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == key)
        else {
            continue;
        };
        let spelling = record.relative_spelling.as_str().to_owned();
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("reads");
        let payload_offset = {
            let path = RelativePath::new(&key.to_lowercase()).expect("a relative path");
            let mut context = ParseContext::with_defaults(&key);
            let decision = dispatch(ZbdProbe::new(&key, &path, header_bytes(&bytes)))
                .expect("the carrier dispatches");
            let index =
                read_animation_index(&mut context, decision, &bytes).expect("the carrier indexes");
            let offset = index.payload_offset();
            let payload = index.payload().expect("the payload reads");
            let walked = payload.records().expect("the records walk");
            for record in walked.iter() {
                records.push((
                    String::from_utf8_lossy(record.anim_name()).into_owned(),
                    String::from_utf8_lossy(record.object_name()).into_owned(),
                    record
                        .sequences()
                        .iter()
                        .filter(|block| block.kind() == AnimationRecordSequenceKind::Sequence)
                        .map(|block| String::from_utf8_lossy(block.name()).into_owned())
                        .collect(),
                ));
            }
            offset
        };
        assert!(payload_offset > 0, "{key} carries a payload");
    }
    assert_eq!(
        records.len(),
        880,
        "M01's mission carrier (573) and C1C's camera carrier (307)"
    );

    // Pair each declaration with the records of its identity.
    let mut pairs = 0_usize;
    let mut single = 0_usize;
    let mut sequence_agreements = 0_usize;
    let mut object_agreements = 0_usize;
    let mut disagreements: Vec<(String, String, String)> = Vec::new();
    for declaration in &declarations {
        // Zero matches and several are the two failures the census leaves out:
        // neither is a pair, so neither is an agreement.
        let matches: Vec<&ClosureRecord> = records
            .iter()
            .filter(|(anim, _, _)| *anim == declaration.animation_name)
            .collect();
        let [record] = matches.as_slice() else {
            continue;
        };
        pairs += 1;
        single += 1;
        let (_, object, sequences) = record;
        let stored: Vec<Option<String>> = sequences
            .iter()
            .map(|stored| (!stored.is_empty()).then(|| stored.clone()))
            .collect();
        if declaration.sequences == stored {
            sequence_agreements += 1;
        }
        if declaration
            .selectors
            .iter()
            .any(|selector| selector == object)
        {
            object_agreements += 1;
        } else {
            disagreements.push((
                declaration.member.clone(),
                declaration.selectors.join(","),
                object.clone(),
            ));
        }
    }

    assert_eq!(single, pairs);
    assert_eq!(pairs, 280, "280 declaration/record pairs over the closure");
    assert_eq!(
        sequence_agreements, 280,
        "every pair agrees on its sequence names"
    );
    assert_eq!(
        object_agreements, 279,
        "every pair but one agrees on its object"
    );
    assert_eq!(
        disagreements,
        vec![(
            "autogyro_bus.zrd".to_owned(),
            "agyrobus".to_owned(),
            "autogyro".to_owned()
        )],
        "the single disagreement, reported and never repaired"
    );
}

/// The header bytes the documented animation rule evaluates.
fn header_bytes(bytes: &[u8]) -> &[u8] {
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    &bytes[..needed]
}

// ---------------------------------------------------------------------------
// Task #690 (`F20-EVENT-GRAMMAR`, prefix `accept_f20_event_`): the measured
// event grammar the refusal above was waiting for.
//
// The grammar is `cs_app::animation::events`: an eight-byte tag/length header
// per event, an opcode table whose statement spellings come from the
// installation's own declarations, `START_TIME` at payload word `0` and
// `RUN_TIME` at the payload's last word where the position is value-matched.
// These tests drive that decoder and the consumer half in `mission` together:
// a decoded record reports a duration and a per-tick pose report, and an
// opcode nobody joins — or a `RUN_TIME` nobody value-matched — refuses the
// whole record.
// ---------------------------------------------------------------------------

use cs_app::animation::events::{
    EVENT_HEADER_BYTES, EventClass, OPCODE_NOT_MEASURED_CLAIM, RUN_TIME_NOT_MEASURED_CLAIM,
    STORED_OPCODES, decode_event_stream, opcode_info, sequence_duration, walk_event_stream,
};
use cs_app::animation::mission::{PlaybackGap, TickPose};

/// The measured retail payload length of one opcode's event, so a synthetic
/// event has the shape the corpus stores.
const fn retail_payload(opcode: u8) -> usize {
    match opcode {
        5 => 104,
        6 => 12,
        11 => 136,
        13 => 16,
        24 => 72,
        _ => 32,
    }
}

/// One event's bytes: the measured header (tag word, then length word) and a
/// payload whose word `0` is `start` and whose last word is `run`.
fn synthetic_event(opcode: u8, group: u8, start: f32, run: f32) -> Vec<u8> {
    let payload_len = retail_payload(opcode);
    assert!(payload_len >= 8 && payload_len.is_multiple_of(4));
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

/// Concatenates events into one block's stream.
fn stream(events: &[Vec<u8>]) -> Vec<u8> {
    events.concat()
}

/// One synthetic record whose sequences are decoded by the **production**
/// decoder, exactly as `mission::own_record` does for a walked carrier.
fn event_facts(
    anim: &str,
    sequences: &[(&str, AnimationRecordSequenceKind, Vec<u8>)],
) -> AnimationRecordFacts {
    let spec = RecordSpec {
        anim_name: anim,
        object_name: "piratezep",
        root_name: "piratezep",
        objects: &[],
        nodes: &[],
        animation_refs: &[],
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

/// Joins a declaration over a record built by [`event_facts`].
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

/// The walk and the decode read the same eight bytes: a tag word whose low
/// byte is the opcode and whose second byte is the measured `1..=3`, then the
/// event's own length including that header.
#[test]
fn accept_f20_event_a_stream_walks_into_tag_and_length_records() {
    let bytes = stream(&[
        synthetic_event(24, 1, 1.5, 0.0),
        synthetic_event(11, 3, 2.0, 4.0),
    ]);

    let walked = walk_event_stream(&bytes).expect("the measured shape walks");
    assert_eq!(walked.len(), 2);
    assert_eq!(walked[0].offset, 0);
    assert_eq!(walked[0].opcode, 24);
    assert_eq!(walked[0].group, 1);
    assert_eq!(
        walked[0].length as usize,
        EVENT_HEADER_BYTES + retail_payload(24),
        "the length word steps over header and payload"
    );
    assert_eq!(walked[1].offset, u64::from(walked[0].length));
    assert_eq!(walked[1].opcode, 11);
    assert_eq!(walked[1].group, 3);
    assert_eq!(walked[1].payload_len() as usize, retail_payload(11));

    let decoded = decode_event_stream(&bytes).expect("both opcodes join a statement");
    assert_eq!(decoded[0].statement, "CALL_ANIMATION");
    assert_eq!(decoded[0].class, EventClass::Control);
    assert_eq!(decoded[0].start_time(), 1.5, "word 0 is START_TIME");
    assert_eq!(
        decoded[0].run_time(),
        None,
        "CALL_ANIMATION states no RUN_TIME"
    );
    assert_eq!(decoded[0].end_time(), 1.5);
    assert_eq!(decoded[1].statement, "OBJECT_MOTION_FROM_TO");
    assert_eq!(decoded[1].class, EventClass::Motion);
    assert_eq!(decoded[1].start_time(), 2.0);
    assert_eq!(
        decoded[1].run_time(),
        Some(4.0),
        "the last word is RUN_TIME where the position is value-matched"
    );
    assert_eq!(decoded[1].end_time(), 6.0);

    // Every way the bytes are not the measured shape is a named refusal with
    // its own offset, never a silent partial walk.
    let truncated = walk_event_stream(&bytes[..6]).expect_err("a header needs eight bytes");
    assert_eq!(truncated.code(), "truncated_header");
    assert_eq!(truncated.offset(), 0);

    let mut bad_length = synthetic_event(24, 1, 0.0, 0.0);
    bad_length[4..8].copy_from_slice(&6_u32.to_le_bytes());
    let refusal = walk_event_stream(&bad_length).expect_err("six is below the header");
    assert_eq!(refusal.code(), "bad_length");
    assert_eq!(refusal.offset(), 0);

    let mut bad_tag = synthetic_event(24, 1, 0.0, 0.0);
    bad_tag[0..4].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
    let refusal = walk_event_stream(&bad_tag).expect_err("the high half is zero");
    assert_eq!(refusal.code(), "bad_tag");

    let short = synthetic_event(24, 1, 0.0, 0.0);
    let refusal = walk_event_stream(&short[..short.len() - 4]).expect_err("payload cut short");
    assert_eq!(refusal.code(), "short_stream");
    assert_eq!(
        refusal.claim_id(),
        "f20-anim.sequence-event-stream-not-decoded"
    );

    let mut unjoined = synthetic_event(24, 1, 0.0, 0.0);
    unjoined[0] = 13;
    let refusal = decode_event_stream(&unjoined).expect_err("opcode 13 joins no statement");
    assert!(
        walk_event_stream(&unjoined).is_ok(),
        "the walk is structural; the vocabulary is what refuses"
    );
    assert_eq!(refusal.code(), "unknown_opcode");
    assert_eq!(refusal.opcode(), Some(13));
    assert_eq!(refusal.claim_id(), OPCODE_NOT_MEASURED_CLAIM);
    assert_eq!(refusal.offset(), 0);
}

/// **The observable failure this suite exists to prevent**: a duration that is
/// a constant rather than a reading of the payloads. The same decoder reads
/// three streams whose stored words differ, and each duration must equal the
/// arithmetic of its own bytes.
#[test]
fn accept_f20_event_a_decoded_duration_is_read_from_the_payloads() {
    // op 6 (start 0.5, no run time), op 24 (start 1.25), op 11 (start 2.0,
    // run 3.5): the record's duration is the largest end time it states.
    let facts = event_facts(
        "pzep_engines_start",
        &[(
            "call_eachengine",
            AnimationRecordSequenceKind::Sequence,
            stream(&[
                synthetic_event(6, 1, 0.5, 0.0),
                synthetic_event(24, 1, 1.25, 0.0),
                synthetic_event(11, 1, 2.0, 3.5),
            ]),
        )],
    );
    let playback = facts.playback().expect("every opcode joins a statement");
    assert_eq!(
        playback.duration_time(),
        5.5,
        "2.0 + 3.5, read from the bytes"
    );

    // The same block with a different stored RUN_TIME gives a different
    // duration: nothing here is a constant.
    let moved = event_facts(
        "pzep_engines_start",
        &[(
            "call_eachengine",
            AnimationRecordSequenceKind::Sequence,
            stream(&[
                synthetic_event(6, 1, 0.5, 0.0),
                synthetic_event(24, 1, 1.25, 0.0),
                synthetic_event(11, 1, 2.0, 7.0),
            ]),
        )],
    );
    let moved = moved.playback().expect("decoded");
    assert_eq!(moved.duration_time(), 9.0, "2.0 + 7.0");
    assert_ne!(
        moved.duration_time(),
        playback.duration_time(),
        "two stored words must not produce one duration"
    );

    // And a record whose statements all start together measures zero, which is
    // a reading rather than an absence: the same stream with a later start
    // measures that start.
    let all_at_once = event_facts(
        "wvzep_engines_start",
        &[(
            "",
            AnimationRecordSequenceKind::Sequence,
            stream(&[synthetic_event(24, 1, 0.0, 0.0)]),
        )],
    );
    assert_eq!(
        all_at_once.playback().expect("decoded").duration_time(),
        0.0
    );
    let later = event_facts(
        "wvzep_engines_start",
        &[(
            "",
            AnimationRecordSequenceKind::Sequence,
            stream(&[synthetic_event(24, 1, 2.5, 0.0)]),
        )],
    );
    assert_eq!(later.playback().expect("decoded").duration_time(), 2.5);

    // The production one-block entry point reads the same words.
    let bytes = stream(&[synthetic_event(11, 1, 2.0, 3.5)]);
    assert_eq!(sequence_duration(&bytes).expect("decoded"), 5.5);
}

/// An opcode no declaration of the installation joins refuses the **whole**
/// record: the block before it decoded, and none of that reaches a playback.
#[test]
fn accept_f20_event_an_undecoded_opcode_refuses_the_whole_record() {
    let good = synthetic_event(24, 1, 0.0, 0.0);
    let unjoined = synthetic_event(13, 1, 0.0, 0.0);
    let offset = u64::try_from(good.len()).expect("fits");
    let facts = event_facts(
        "player_setup",
        &[(
            "setup",
            AnimationRecordSequenceKind::Sequence,
            stream(&[good, unjoined]),
        )],
    );
    let SequenceEvents::Refused {
        code,
        reason,
        offset: stream_offset,
        claim_id: stream_claim,
        opcode: stream_opcode,
    } = facts.sequences()[0].events()
    else {
        panic!("opcode 13 refuses the block");
    };
    assert_eq!(*code, "unknown_opcode");
    assert_eq!(
        *stream_offset, offset,
        "the refusal names its byte in the stream"
    );
    assert_eq!(*stream_claim, OPCODE_NOT_MEASURED_CLAIM);
    assert_eq!(*stream_opcode, Some(13));
    assert_eq!(*reason, opcode_refusal_reason());
    let gap = facts.playback().expect_err("opcode 13 is unmeasured");
    assert_eq!(gap.label(), "event_opcode_not_decoded");
    assert_eq!(gap.claim_id(), OPCODE_NOT_MEASURED_CLAIM);
    assert_eq!(gap.opcode(), Some(13));
    assert_eq!(gap.offset(), Some(offset));

    let joined = joined_over("player_setup", &["setup"], facts);
    assert!(
        !joined.is_playable(),
        "no partial playback from a decoded half"
    );
    assert!(
        joined.playback().is_none(),
        "a refused record reports no duration"
    );
    assert_eq!(joined.refusals().len(), 1);
    let PlayRefusal::EventOpcodeUndecoded {
        reason,
        claim_id,
        opcode,
        offset: refusal_offset,
    } = &joined.refusals()[0]
    else {
        panic!("the refusal is the unjoined opcode: {joined:?}");
    };
    assert_eq!(*opcode, 13);
    assert_eq!(*refusal_offset, offset, "the refusal names its byte");
    assert_eq!(claim_id.as_str(), OPCODE_NOT_MEASURED_CLAIM);
    assert_eq!(*reason, opcode_refusal_reason());
    assert_eq!(joined.refusals()[0].label(), "event_opcode_undecoded");
}

/// The measured vocabulary's one open timing gap: opcode 5's statement states
/// a `RUN_TIME` whose payload position was never value-matched, so a duration
/// for such a record could only be guessed. It is refused instead.
#[test]
fn accept_f20_event_an_unmatched_run_time_refuses_the_whole_record() {
    let info = opcode_info(5).expect("opcode 5 is stored by the corpus");
    assert_eq!(info.statement, "LIGHT_ANIMATION");
    assert!(!info.timing_measured());
    assert_eq!(info.timing_gap_claim(), Some(RUN_TIME_NOT_MEASURED_CLAIM));

    let facts = event_facts(
        "lboat_destruction11",
        &[(
            "light_seq",
            AnimationRecordSequenceKind::Sequence,
            stream(&[synthetic_event(5, 1, 1.0, 2.0)]),
        )],
    );
    // The stream itself walked: this is a timing gap, not a shape failure.
    assert!(matches!(
        facts.sequences()[0].events(),
        SequenceEvents::Decoded(events) if events.len() == 1
    ));
    let PlaybackGap::TimingNotDecoded {
        opcode,
        reason,
        offset,
        claim_id,
    } = facts
        .playback()
        .expect_err("RUN_TIME is unmatched for opcode 5")
    else {
        panic!("opcode 5 is a timing gap");
    };
    assert_eq!(opcode, 5);
    assert_eq!(claim_id, RUN_TIME_NOT_MEASURED_CLAIM);
    assert_eq!(offset, 0);
    assert_eq!(reason, run_time_refusal_reason());
}

/// The census half of acceptance criterion 2, on the table itself: 35 stored
/// opcodes, 31 joined statements, four named unknowns, and a class on every
/// entry so a report can never drop one.
#[test]
fn accept_f20_event_every_stored_opcode_is_classed_and_the_unknowns_named() {
    assert_eq!(
        STORED_OPCODES.len(),
        35,
        "every stored opcode, nothing more"
    );
    let mut classes = std::collections::BTreeSet::new();
    let mut unknowns = Vec::new();
    for info in STORED_OPCODES {
        classes.insert(info.class);
        if info.class == EventClass::Unknown {
            unknowns.push(info.opcode);
            assert!(info.statement.is_empty(), "{info:?}");
            assert_eq!(
                info.timing_gap_claim(),
                Some(OPCODE_NOT_MEASURED_CLAIM),
                "{info:?}"
            );
            assert!(!info.timing_measured(), "{info:?}");
        } else {
            assert!(!info.statement.is_empty(), "{info:?}");
            assert_ne!(info.class, EventClass::Unknown, "{info:?}");
            assert_eq!(
                opcode_info(info.opcode),
                Some(info),
                "one table, one answer"
            );
        }
    }
    assert_eq!(unknowns, vec![13, 17, 26, 28], "the four unjoined opcodes");
    assert_eq!(
        classes,
        EventClass::ALL
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        "every class appears, so no class can vanish from a census"
    );
    // The measured spellings, by opcode, as the installation writes them.
    assert_eq!(
        opcode_info(11).expect("stored").statement,
        "OBJECT_MOTION_FROM_TO"
    );
    assert_eq!(opcode_info(24).expect("stored").statement, "CALL_ANIMATION");
    assert_eq!(
        opcode_info(41).expect("stored").statement,
        "DETONATE_WEAPON"
    );
    assert_eq!(opcode_info(41).expect("stored").class, EventClass::Marker);
    // A code no retail carrier stores is not invented to fill a table.
    for absent in [0_u8, 3, 43, 48, 255] {
        assert!(
            opcode_info(absent).is_none(),
            "opcode {absent} is not stored"
        );
    }
    // The two timing evidences that make a duration readable, and the one
    // that refuses it.
    assert_eq!(
        opcode_info(10).expect("stored").run_time.label(),
        "value_match"
    );
    assert_eq!(opcode_info(6).expect("stored").run_time.label(), "absent");
    assert_eq!(
        opcode_info(5).expect("stored").run_time.label(),
        "unmatched"
    );
    assert_eq!(
        opcode_info(13).expect("stored").start_time.label(),
        "unmeasured"
    );
}

/// The per-tick report is built from the measured timing alone: which
/// statements cover which tick of the **caller's** timeline.
#[test]
fn accept_f20_event_the_per_tick_report_follows_the_measured_timing() {
    let facts = event_facts(
        "generic_intro",
        &[(
            "gi_scene1",
            AnimationRecordSequenceKind::Sequence,
            stream(&[
                synthetic_event(6, 1, 0.0, 0.0),
                synthetic_event(24, 1, 2.0, 0.0),
            ]),
        )],
    );
    let playback = facts.playback().expect("decoded");
    assert_eq!(playback.duration_time(), 2.0);
    assert_eq!(playback.sequences().len(), 1);
    assert_eq!(playback.events().count(), 2);

    let poses = playback.poses(2);
    assert_eq!(
        poses.len(),
        5,
        "ticks 0..=4 cover a duration of 2.0 at 2 Hz"
    );
    let statements = |pose: &TickPose| {
        pose.statements()
            .iter()
            .map(|statement| (statement.opcode, statement.statement, statement.start_time))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        statements(&poses[0]),
        vec![(6_u8, "OBJECT_ACTIVE_STATE", 0.0)],
        "the statement with no run time starts the record"
    );
    assert_eq!(
        statements(&poses[1]),
        vec![(6, "OBJECT_ACTIVE_STATE", 0.0)],
        "a started statement stays in the record's configuration"
    );
    assert_eq!(
        statements(&poses[3]),
        vec![(6, "OBJECT_ACTIVE_STATE", 0.0)],
        "before the later statement starts, only the first has fired"
    );
    assert_eq!(
        statements(&poses[4]),
        vec![(6, "OBJECT_ACTIVE_STATE", 0.0), (24, "CALL_ANIMATION", 2.0)],
        "the later statement joins the configuration at its own start time"
    );
    assert!(
        playback.poses(0).is_empty(),
        "a rate of zero samples nothing instead of inventing a timeline"
    );
}

/// The measured reason text travels with the refusal, so a report and its
/// finding cannot drift apart.
fn opcode_refusal_reason() -> &'static str {
    cs_app::animation::events::OPCODE_NOT_MEASURED_REASON
}

fn run_time_refusal_reason() -> &'static str {
    cs_app::animation::events::RUN_TIME_NOT_MEASURED_REASON
}

/// The retail half of acceptance criterion 1: **every** event stream of every
/// block of every record in the installation walks with the two-word header,
/// and the opcode census over all of them is the measured one — including the
/// four opcodes no declaration joins, which are counted rather than dropped.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f20_event_retail_every_event_stream_walks_and_is_censused() {
    /// The measured opcode census of the installation, opcode → events.
    const CENSUS: &[(u8, u64)] = &[
        (1, 6_398),
        (2, 1_244),
        (4, 1_468),
        (5, 535),
        (6, 68_109),
        (7, 1_143),
        (8, 2_209),
        (9, 6_610),
        (10, 7_458),
        (11, 9_421),
        (12, 845),
        (13, 340),
        (14, 9_917),
        (15, 1_163),
        (16, 263),
        (17, 144),
        (20, 10),
        (22, 22_391),
        (23, 1_143),
        (24, 56_750),
        (25, 7_928),
        (26, 11),
        (27, 6_618),
        (28, 1),
        (30, 3_475),
        (31, 5_468),
        (32, 4_680),
        (33, 5_727),
        (34, 5_468),
        (35, 736),
        (36, 155),
        (41, 3),
        (42, 4_535),
        (46, 3),
        (47, 22),
    ];
    /// The measured class totals over those same events.
    const CLASSES: &[(&str, u64)] = &[
        ("control", 119_648),
        ("marker", 739),
        ("motion", 37_625),
        ("sound", 7_645),
        ("state", 76_238),
        ("unknown", 496),
    ];

    let root = retail_root();
    let found = install::discover(&root).expect("production discovery reads the installation");
    let mut blocks = 0_usize;
    let mut events = 0_usize;
    let mut bytes = 0_usize;
    let mut refused_blocks = 0_usize;
    let mut census = std::collections::BTreeMap::new();
    for file in &found.manifest.files {
        let key = file.relative_spelling.logical_key();
        if !(key.ends_with("/mis_anim.zbd") || key.ends_with("/cam_anim.zbd")) {
            continue;
        }
        let data = std::fs::read(root.join(file.relative_spelling.as_str())).expect("read carrier");
        let path =
            RelativePath::new(&file.relative_spelling.as_str().to_lowercase()).expect("a path");
        let mut context = ParseContext::with_defaults(&key);
        let decision = dispatch(ZbdProbe::new(&key, &path, &data[..8]))
            .expect("a carrier is an animation container");
        let index = read_animation_index(&mut context, decision, &data).expect("the index reads");
        let payload = index.payload().expect("the payload header reads");
        let walk = payload.records().expect("the records walk");
        for record in walk.iter() {
            for block in record.sequences() {
                blocks += 1;
                bytes += block.events().len();
                let walked = walk_event_stream(block.events())
                    .unwrap_or_else(|error| panic!("{key} record {}: {error}", record.index()));
                let stepped: usize = walked.iter().map(|event| event.length as usize).sum();
                assert_eq!(
                    stepped,
                    block.events().len(),
                    "{key} record {}: the walk consumes the block exactly",
                    record.index()
                );
                events += walked.len();
                for event in &walked {
                    *census.entry(event.opcode).or_insert(0_u64) += 1;
                }
                if decode_event_stream(block.events()).is_err() {
                    refused_blocks += 1;
                }
            }
        }
    }
    assert_eq!(blocks, 56_994, "reset, damage and ordinary blocks");
    assert_eq!(events, 242_391, "every event of every block");
    assert_eq!(bytes, 16_132_948, "every stored event byte");
    assert_eq!(
        census.iter().map(|(op, n)| (*op, *n)).collect::<Vec<_>>(),
        CENSUS.to_vec(),
        "the opcode census is the measured one, unknowns included"
    );
    assert_eq!(
        refused_blocks, 389,
        "only blocks carrying an unjoined opcode (13, 17, 26, 28) refuse to decode"
    );

    let mut by_class = std::collections::BTreeMap::new();
    for (opcode, count) in &census {
        let info = opcode_info(*opcode).expect("a stored opcode is in the table");
        *by_class.entry(info.class.label()).or_insert(0_u64) += count;
    }
    assert_eq!(
        by_class.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(),
        CLASSES.to_vec(),
        "every event is classed exactly once, and the unknowns are their own class"
    );
    assert_eq!(
        by_class.get("unknown"),
        Some(&496),
        "the four unjoined opcodes are counted, not dropped"
    );
}

/// The retail half of acceptance criterion 3: M01's seven startup records are
/// playable, each with the duration its own payloads state. These numbers are
/// read from the bytes on every run, so a duration replaced by a guess fails
/// here.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f20_event_retail_m01_startup_records_play_with_measured_durations() {
    /// identity → (measured duration, decoded events), from the retail bytes.
    const M01: &[(&str, f32, usize)] = &[
        ("pzep_engines_start", 0.0, 12),
        ("wvzep_engines_start", 0.0, 18),
        ("bszep_engines_start", 0.0, 14),
        ("wv_hookup_state", 5.0, 3),
        ("generic_intro", 15.9, 46),
        ("call_add_jack", 0.2, 1),
        ("player_setup", 0.0, 11),
    ];
    let binding =
        bind_mission_animation(&retail_root(), "zbd/c1c/m01").expect("M01 binds in production");
    assert_eq!(binding.playable_count(), 7, "every joined record decodes");
    assert_eq!(binding.refused_count(), 0);

    let mut seen = std::collections::BTreeSet::new();
    for row in binding.startup() {
        let (_, expected_duration, expected_events) = M01
            .iter()
            .find(|(identity, _, _)| *identity == row.identity())
            .unwrap_or_else(|| panic!("{} is one of M01's seven", row.identity()));
        assert!(
            row.refusals().is_empty(),
            "{} plays for no reason: {row:?}",
            row.identity()
        );
        let playback = row.playback().expect("a playable row carries a playback");
        assert!(
            (playback.duration_time() - expected_duration).abs() < 1e-6,
            "{}: measured {} instead of {}",
            row.identity(),
            playback.duration_time(),
            expected_duration
        );
        assert_eq!(
            playback.events().count(),
            *expected_events,
            "{}",
            row.identity()
        );
        for event in playback.events() {
            assert!(
                !event.statement.is_empty(),
                "{}: every decoded event carries the installation's own spelling",
                row.identity()
            );
            assert_ne!(
                event.class,
                EventClass::Unknown,
                "{}: an unknown opcode never reaches a playback",
                row.identity()
            );
        }
        assert!(
            !playback.poses(10).is_empty(),
            "{}: the per-tick report covers the record's own timeline",
            row.identity()
        );
        seen.insert(row.identity());
    }
    assert_eq!(seen.len(), M01.len(), "all seven, each once");

    // A statement that starts after tick zero is not in the first row: the
    // report reads its own timing rather than showing everything everywhere.
    let call_add_jack = binding
        .startup()
        .iter()
        .find(|row| row.identity() == "call_add_jack")
        .expect("the row");
    let poses = call_add_jack.playback().expect("decoded").poses(10);
    assert!(
        poses[0].statements().is_empty(),
        "at 10 Hz tick zero is before the statement's measured start of 0.2"
    );
    assert_eq!(
        poses[2].statements().len(),
        1,
        "by tick two (0.2) the statement has started"
    );
}

/// **The naming claim, re-derived from the installation.** Every other test of
/// `STORED_OPCODES` checks that table against itself: the census test counts
/// the events it holds, the synthetic tests read the spellings it states. This
/// one checks the spellings against the **files**: every `zrdr.zbd` declaration
/// that names an animation is read through
/// [`read_animation_definition_member`], every carrier record through
/// [`read_animation_index`], the two are paired by the one identity that
/// matches exactly one record (#678's rule), and each declaration sequence's
/// statement kinds are aligned position by position with its record's ordinary
/// block's decoded events. The installation's own text is the expectation, so
/// a spelling, an alignment rule or the table itself that stops matching the
/// files fails here — over all 477 pairs, not the first few.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f20_event_retail_every_opcode_spelling_is_read_from_a_declaration() {
    /// One declared sequence: its stored name (absent is stored as empty) and
    /// its statement kinds in stored order.
    struct DeclaredSequence {
        name: Option<String>,
        kinds: Vec<String>,
    }

    /// One definition of one declaring member, as the reader states it.
    struct Declaration {
        archive: String,
        member: String,
        animation: String,
        sequences: Vec<DeclaredSequence>,
    }

    /// One stored ordinary block of one record.
    struct StoredBlock {
        name: String,
        events: Vec<u8>,
    }

    /// One walked record of one carrier.
    struct StoredRecord {
        carrier: String,
        index: usize,
        animation: String,
        blocks: Vec<StoredBlock>,
    }

    let root = retail_root();
    let found = install::discover(&root).expect("production discovery reads the installation");

    // Every declaring archive of the installation, and every member of it that
    // is an `ANIMATION_DEFINITIONS` record naming an animation.
    let mut declarations: Vec<Declaration> = Vec::new();
    for file in &found.manifest.files {
        let key = file.relative_spelling.logical_key();
        if !key.ends_with("/zrdr.zbd") {
            continue;
        }
        let bytes =
            std::fs::read(root.join(file.relative_spelling.as_str())).expect("read the archive");
        let path =
            RelativePath::new(&file.relative_spelling.as_str().to_lowercase()).expect("a path");
        for program in discover_container(&key, &path, &bytes).programs() {
            let Some(member) = program.locator().member().map(str::to_owned) else {
                continue;
            };
            let Ok(read) = read_animation_definition_member(&member, program.bytes()) else {
                continue;
            };
            for definition in read.definitions() {
                let Some(animation) = definition.animation_name() else {
                    continue;
                };
                declarations.push(Declaration {
                    archive: key.clone(),
                    member: member.clone(),
                    animation: animation.to_owned(),
                    sequences: definition
                        .sequences()
                        .iter()
                        .map(|sequence| DeclaredSequence {
                            name: sequence.name().map(str::to_owned),
                            kinds: sequence
                                .entries()
                                .iter()
                                .map(|entry| entry.kind().to_owned())
                                .collect(),
                        })
                        .collect(),
                });
            }
        }
    }
    assert!(
        !declarations.is_empty(),
        "the installation declares animations the join can be read against"
    );

    // Every record of every carrier, with its ordinary blocks' event bytes.
    let mut records: Vec<StoredRecord> = Vec::new();
    for file in &found.manifest.files {
        let key = file.relative_spelling.logical_key();
        if !(key.ends_with("/mis_anim.zbd") || key.ends_with("/cam_anim.zbd")) {
            continue;
        }
        let bytes =
            std::fs::read(root.join(file.relative_spelling.as_str())).expect("read the carrier");
        let path =
            RelativePath::new(&file.relative_spelling.as_str().to_lowercase()).expect("a path");
        let mut context = ParseContext::with_defaults(&key);
        let decision = dispatch(ZbdProbe::new(&key, &path, &bytes[..8]))
            .expect("a carrier is an animation container");
        let index = read_animation_index(&mut context, decision, &bytes).expect("the index reads");
        let payload = index.payload().expect("the payload header reads");
        let walk = payload.records().expect("the records walk");
        for record in walk.iter() {
            records.push(StoredRecord {
                carrier: key.clone(),
                index: record.index(),
                animation: String::from_utf8_lossy(record.anim_name()).into_owned(),
                blocks: record
                    .sequences()
                    .iter()
                    .filter(|block| block.kind() == AnimationRecordSequenceKind::Sequence)
                    .map(|block| StoredBlock {
                        name: String::from_utf8_lossy(block.name()).into_owned(),
                        events: block.events().to_vec(),
                    })
                    .collect(),
            });
        }
    }
    assert!(!records.is_empty(), "the carriers hold records to join");

    // The join: a declaration pairs with the record its identity names exactly
    // once; an identity with no record or several is not a pair, as in #678's
    // census. Every paired sequence is then aligned, the declaration's leading
    // `ACTIVATION` — which the payload never stores as an event — dropped when
    // it is there.
    let mut pairs = 0_usize;
    let mut sequences = 0_usize;
    let mut events = 0_usize;
    let mut joined: std::collections::BTreeMap<u8, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut unpaired_shapes: Vec<String> = Vec::new();
    let mut name_disagreements: Vec<String> = Vec::new();
    for declaration in &declarations {
        let matches: Vec<&StoredRecord> = records
            .iter()
            .filter(|record| record.animation == declaration.animation)
            .collect();
        let [record] = matches.as_slice() else {
            continue;
        };
        pairs += 1;
        if declaration.sequences.len() != record.blocks.len() {
            unpaired_shapes.push(format!(
                "{}::{} declares {} sequences, {} record {} stores {}",
                declaration.archive,
                declaration.member,
                declaration.sequences.len(),
                record.carrier,
                record.index,
                record.blocks.len(),
            ));
            continue;
        }
        for (declared, block) in declaration.sequences.iter().zip(&record.blocks) {
            if declared.name != ((!block.name.is_empty()).then(|| block.name.clone())) {
                name_disagreements.push(format!(
                    "{}::{} / {} record {}: {:?} against {:?}",
                    declaration.archive,
                    declaration.member,
                    record.carrier,
                    record.index,
                    declared.name,
                    block.name,
                ));
                continue;
            }
            let decoded = decode_event_stream(&block.events).unwrap_or_else(|error| {
                panic!(
                    "{} record {} sequence {:?}: {error}",
                    record.carrier, record.index, block.name
                )
            });
            let kinds: &[String] = if declared.kinds.len() == decoded.len() {
                &declared.kinds
            } else if declared.kinds.len() == decoded.len() + 1 && declared.kinds[0] == "ACTIVATION"
            {
                &declared.kinds[1..]
            } else {
                unpaired_shapes.push(format!(
                    "{}::{} / {} record {} sequence {:?}: {} statements against {} events",
                    declaration.archive,
                    declaration.member,
                    record.carrier,
                    record.index,
                    block.name,
                    declared.kinds.len(),
                    decoded.len(),
                ));
                continue;
            };
            sequences += 1;
            events += decoded.len();
            for (kind, event) in kinds.iter().zip(&decoded) {
                let info = opcode_info(event.opcode).expect("a stored opcode");
                assert_eq!(
                    info.statement,
                    kind,
                    "{} record {} opcode {} at +{}: the table says {:?}, {}::{} says {:?}",
                    record.carrier,
                    record.index,
                    event.opcode,
                    event.offset,
                    info.statement,
                    declaration.archive,
                    declaration.member,
                    kind,
                );
                joined
                    .entry(event.opcode)
                    .or_default()
                    .insert((*kind).clone());
            }
        }
    }

    assert!(
        unpaired_shapes.is_empty(),
        "every pair aligns statement for statement: {unpaired_shapes:#?}"
    );
    assert!(
        name_disagreements.is_empty(),
        "every pair agrees on its sequence names: {name_disagreements:#?}"
    );
    assert_eq!(
        pairs, 477,
        "477 declaration/record pairs over the installation's carriers"
    );
    assert_eq!(sequences, 1_440, "every aligned sequence of every pair");
    assert_eq!(
        events, 5_340,
        "every event whose opcode a declaration statement names"
    );

    let conflicted: Vec<u8> = joined
        .iter()
        .filter(|(_, kinds)| kinds.len() > 1)
        .map(|(opcode, _)| *opcode)
        .collect();
    assert!(
        conflicted.is_empty(),
        "an opcode joined two declaration spellings: {conflicted:?}"
    );
    let table: std::collections::BTreeSet<u8> = STORED_OPCODES
        .iter()
        .filter(|info| info.class != EventClass::Unknown)
        .map(|info| info.opcode)
        .collect();
    assert_eq!(
        joined
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        table,
        "the joined opcodes are exactly the table's joined entries, spellings and all"
    );
}
