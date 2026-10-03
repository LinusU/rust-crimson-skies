//! #494: which side of a world container's stored hierarchy is authoritative.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-A`. Task test prefix: `accept_f18_a_`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Finding:
//! `docs/findings/2026-10-03-f18-world-hierarchy-authority.md`.
//!
//! A GameZ node record carries its hierarchy twice: a `parent` slot in its own
//! record and a `children_count`-length list that follows it. In the aircraft
//! container the two agree on every link; in all eight world containers 155 to
//! 471 records name the **world node** as their parent while the world node's
//! own list omits them, which is why `SceneGraph::build` refused all eight with
//! `InconsistentParentage`.
//!
//! The rule this stage adopts is that the **parent slot is the authoritative
//! statement of ownership and the child list is an index that cannot veto it**.
//! `cs_content::world::world_scene_graph_from_gamez` derives the children from
//! the parent slots, so the strict build is satisfied by reconciled records
//! rather than by a relaxed build — nothing in `SceneGraph::build`'s own
//! `InconsistentParentage` refusal is weakened.
//!
//! These tests drive production code only: `cs_content::world::audit_stored_hierarchy`
//! is the measurement, `world_hierarchy_from_gamez` is the rule and
//! `world_scene_graph_from_gamez` is the whole container path. No test carries
//! its own hierarchy walk.
//!
//! The synthetic half builds [`GameZNodes`] records directly, so CI runs it. The
//! retail half is `#[ignore]`d and reads the original installation: no original
//! run happened, so nothing here claims `verified_original`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_content::scene::{BindingMap, GameZSceneError, SceneError, scene_graph_from_gamez};
use cs_content::world::{
    HierarchyVerdict, StoredHierarchyAudit, WorldHierarchyError, WorldSceneError,
    audit_stored_hierarchy, world_hierarchy_from_gamez, world_node_slot,
    world_scene_graph_from_gamez,
};
use cs_formats::gamez::reader::GAMEZ_HEADER_BYTES;
use cs_formats::gamez::{
    GameZHeader, GameZNodes, NODE_TYPE_OBJECT3D, NODE_TYPE_WORLD, OBJECT3D_FLAGS_IDENTITY, RawNode,
    RawNodeInfo, RawObject3dData, RawWorldData, read_gamez_nodes,
};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{GAMEZ_SIGNATURE, GAMEZ_VERSION};
use cs_types::content::{ContentId, ContentKind};

/// The F16-A `canonical` adapter: identity axis map, radians, one unit per
/// metre.
///
/// A CS node record stores its euler triple in radians, and the F11-A
/// conversion routes the triple through the declared adapter's angle unit, so a
/// degrees adapter would make these assertions about the adapter rather than
/// about the hierarchy.
fn radian_adapter() -> cs_content::coordinates::SourceAdapter {
    cs_content::coordinates::SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source")
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id is valid")
}

/// One synthetic stored record.
///
/// Only the fields the hierarchy measurement reads are given values; the rest
/// are the store's own zeros, which the reader would have produced for a record
/// that stores nothing in them.
fn stored_node(index: u32, name: &str, parent: Option<u32>, children: &[u32]) -> RawNode {
    RawNode {
        index,
        name: name.to_owned(),
        node_index: 0x0200_0000 | index,
        // The store's own asserted profile, spelled out rather than defaulted:
        // a test that invented a different profile would be testing itself.
        info: RawNodeInfo {
            flags: if index == 0 { 0 } else { 0x8010_0000 },
            unk044: 1,
            zone_id: 255,
            node_type: if index == 0 {
                NODE_TYPE_WORLD
            } else {
                NODE_TYPE_OBJECT3D
            },
            data_ptr: 1,
            mesh_index: -1,
            action_priority: 1,
            parent_count: u16::from(parent.is_some()),
            children_count: children.len() as u16,
            unk196: if index == 0 { 0 } else { 160 },
            unk040: 0,
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
            unk200: 0,
            unk204: 0,
            parent_array_ptr: 0,
            children_array_ptr: 0,
            action_callback: 0,
            environment_data: 0,
            area_partition: [-1, -1, 0, 0],
        },
        kind: if index == 0 {
            cs_formats::gamez::NodeKind::World(RawWorldData {
                partition_x_count: 1,
                partition_y_count: 1,
                partition_bytes: 0,
                partition_values: 0,
            })
        } else {
            cs_formats::gamez::NodeKind::Object3d(RawObject3dData {
                flags: OBJECT3D_FLAGS_IDENTITY,
                rotation: [0.0; 3],
                scale: [1.0; 3],
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                translation: [0.0; 3],
            })
        },
        data_offset: 0,
        data_bytes: 0,
        parent,
        children: children.to_vec(),
    }
}

/// A container holding exactly `nodes`, read as a [`GameZNodes`].
fn container(records: Vec<RawNode>) -> GameZNodes {
    let nodes = records;
    GameZNodes {
        header: GameZHeader {
            signature: GAMEZ_SIGNATURE,
            version: GAMEZ_VERSION,
            unk08: 0,
            texture_count: 0,
            textures_offset: GAMEZ_HEADER_BYTES as u32,
            materials_offset: GAMEZ_HEADER_BYTES as u32,
            meshes_offset: GAMEZ_HEADER_BYTES as u32,
            node_array_size: nodes.len() as u32,
            light_index: u32::MAX,
            nodes_offset: GAMEZ_HEADER_BYTES as u32,
        },
        nodes,
        info_offset: 0,
        info_end: 0,
        data_offset: 0,
        data_end: 0,
        findings: Vec::new(),
    }
}

// ---------------------------------------------------------------- synthetic ---

/// The world node's child list omits two records that name it, and nothing
/// disagrees the other way: the verdict is a partial index, over exactly one
/// parent, and the adopted rule converts the container into a real hierarchy
/// where those two records hang off the world node.
#[test]
fn accept_f18_a_a_world_child_list_that_omits_records_is_a_partial_index_not_a_refusal() {
    // 0 `world1` lists only record 1; records 2 and 3 name it and are omitted.
    // Record 1's own list agrees with its one child's parent slot, so the whole
    // disagreement is confined to the world node's list.
    let records = container(vec![
        stored_node(0, "world1", None, &[1]),
        stored_node(1, "listed", Some(0), &[4]),
        stored_node(2, "omitted_a", Some(0), &[]),
        stored_node(3, "omitted_b", Some(0), &[]),
        stored_node(4, "grandchild", Some(1), &[]),
    ]);

    let audit = audit_stored_hierarchy(&records);
    assert_eq!(audit.nodes(), 5);
    assert_eq!(
        audit.verdict(),
        &HierarchyVerdict::PartialChildIndex {
            omitted: 2,
            parents: 1,
            child_slots: 2
        },
        "the list is a partial index over one parent, and that is the whole disagreement"
    );
    assert_eq!(audit.named_but_unlisted(), 2, "the two omitted records");
    assert_eq!(
        audit.listed_but_not_naming(),
        0,
        "nothing disagrees the other way"
    );
    assert_eq!(audit.listed_twice(), 0, "no record is listed twice");
    assert_eq!(
        audit.partial_parents(),
        1,
        "only the world node's list is partial"
    );
    assert_eq!(audit.roots(), 1);
    assert_eq!(
        audit.unreachable(),
        0,
        "every record is reachable from the root once children are derived"
    );
    assert!(
        audit.is_convertible(),
        "the adopted rule applies to this container"
    );

    let hierarchy = world_hierarchy_from_gamez(&records, &[])
        .expect("the partial child list is reconciled, not refused");
    let nodes = hierarchy.nodes();
    assert_eq!(nodes.len(), 5, "every stored record survives the rule");
    assert_eq!(
        nodes[0].children,
        vec![1, 2, 3],
        "the world node's children are derived from the parent slots, in stored order"
    );
    assert_eq!(
        nodes[1].children,
        vec![4],
        "and an agreeing list is unchanged by the derivation"
    );
    assert!(
        nodes[2].children.is_empty() && nodes[3].children.is_empty(),
        "an omitted record is a leaf: the rule adds no child the store did not state"
    );

    // The whole container path: the reconciled hierarchy satisfies the strict
    // build, and the build's own `InconsistentParentage` refusal is what the
    // rule has to satisfy rather than something it weakened.
    let graph = world_scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.synthetic"),
        &records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect("the reconciled hierarchy builds");
    assert_eq!(graph.graph().len(), 5);
    assert_eq!(graph.graph().roots().len(), 1);
    assert_eq!(
        graph.audit().named_but_unlisted(),
        2,
        "the conversion carries the measurement it was made under"
    );
    let children = graph
        .graph()
        .node(&graph.graph().roots()[0])
        .expect("the world node is the root")
        .children()
        .len();
    assert_eq!(
        children, 3,
        "all three records reach the graph under the world node"
    );

    // Removing the adopted rule is exactly `scene_graph_from_gamez` on the same
    // records, and it refuses: the disagreement is real and this test would fail
    // without the reconciliation.
    let refused = scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.synthetic"),
        &records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect_err("without the rule the build refuses the omitted record");
    let GameZSceneError::Build(SceneError::InconsistentParentage { node, parent }) = refused else {
        panic!("expected the hierarchy refusal, got {refused}");
    };
    assert_eq!(
        (node, parent),
        (2, 0),
        "the first omitted record, and the parent it names"
    );
}

/// A container whose child lists contradict the parent slots is **blocked with
/// its counts**, not converted under a rule its own bytes contradict.
#[test]
fn accept_f18_a_a_child_list_that_names_the_wrong_parent_blocks_the_container_with_its_counts() {
    // Record 2 names record 3, so record 0's list of it names the wrong
    // parent. Record 2 is listed exactly once, so this is the wrong-parent
    // contradiction alone and not also the two-parents one.
    let records = container(vec![
        stored_node(0, "world1", None, &[1, 2]),
        stored_node(1, "listed", Some(0), &[]),
        stored_node(2, "mislisted", Some(3), &[]),
        stored_node(3, "real_parent", None, &[]),
    ]);

    let audit = audit_stored_hierarchy(&records);
    assert_eq!(
        audit.verdict(),
        &HierarchyVerdict::Contradictory {
            listed_but_not_naming: 1,
            listed_twice: 0,
            child_slots: 2
        },
        "one record is listed by a node it does not name as its parent"
    );
    assert!(
        !audit.is_convertible(),
        "the adopted rule does not apply to a container that contradicts it"
    );
    // Record 2 naming record 3, which does not list it, is also the
    // partial-index shape; the verdict is the contradiction because that is the
    // worse of the two and a caller must not read this container as a partial
    // index it could convert.
    assert_eq!(
        audit.named_but_unlisted(),
        1,
        "the partial-index count is reported too"
    );
    assert_eq!(
        audit.partial_parents(),
        1,
        "and names the parent whose list omits it"
    );

    let error = world_hierarchy_from_gamez(&records, &[])
        .expect_err("a contradicting container is blocked, not converted");
    let WorldHierarchyError::Contradictory { verdict } = &error else {
        panic!("expected the contradiction refusal, got {error}");
    };
    assert_eq!(
        **verdict,
        *audit.verdict(),
        "the refusal carries the measurement, so the report can name the counts"
    );

    // The same container through the whole path reports the same blocker.
    let blocked = world_scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.synthetic"),
        &records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect_err("the container path reports the blocker");
    assert!(matches!(blocked, WorldSceneError::Hierarchy(_)));
}

/// A record two parents list at once is the other contradiction, and it blocks
/// for the same reason: ownership would be ambiguous.
#[test]
fn accept_f18_a_a_record_two_parents_list_blocks_the_container() {
    // Record 2 names record 1 and both 0 and 1 list it: the two-parents
    // contradiction. One of those two listings is also a wrong-parent listing,
    // so the count of that direction is 1 and this test pins both.
    let records = container(vec![
        stored_node(0, "world1", None, &[1, 2]),
        stored_node(1, "first", Some(0), &[2]),
        stored_node(2, "claimed_twice", Some(1), &[]),
    ]);

    let audit = audit_stored_hierarchy(&records);
    assert_eq!(audit.listed_twice(), 1, "record 2 is listed by two parents");
    assert_eq!(
        audit.verdict(),
        &HierarchyVerdict::Contradictory {
            listed_but_not_naming: 1,
            listed_twice: 1,
            child_slots: 3
        },
        "two parents claiming one record is the contradiction the rule cannot decide"
    );
    assert!(
        world_hierarchy_from_gamez(&records, &[]).is_err(),
        "and it blocks"
    );
}

/// A container whose two sides agree needs no rule at all: the verdict is
/// [`HierarchyVerdict::Consistent`] and the derived children are exactly the
/// stored ones. This is the aircraft container's shape, and it is what makes the
/// rule a no-op on an already-consistent container rather than a second way of
/// reading every hierarchy.
#[test]
fn accept_f18_a_a_consistent_container_needs_no_rule_and_the_derivation_is_a_no_op() {
    let records = container(vec![
        stored_node(0, "world1", None, &[1, 2]),
        stored_node(1, "a", Some(0), &[3]),
        stored_node(2, "b", Some(0), &[]),
        stored_node(3, "c", Some(1), &[]),
    ]);

    let audit = audit_stored_hierarchy(&records);
    assert_eq!(
        audit.verdict(),
        &HierarchyVerdict::Consistent { child_slots: 3 },
        "every link agrees in both directions"
    );
    assert_eq!(audit.unreachable(), 0);
    assert_eq!(
        world_node_slot(&records),
        Some(0),
        "the world node is record 0"
    );

    let hierarchy = world_hierarchy_from_gamez(&records, &[]).expect("no rule to apply");
    for node in hierarchy.nodes() {
        let stored = &records.nodes[node.index as usize].children;
        assert_eq!(
            node.children, *stored,
            "the derived children are the stored ones for record {}",
            node.index
        );
    }
    assert!(
        world_scene_graph_from_gamez(
            &cid(ContentKind::SceneNode, "container.synthetic"),
            &records,
            &[],
            &radian_adapter(),
            &BindingMap::default(),
        )
        .is_ok(),
        "a consistent container converts exactly as it did before this stage"
    );
}

/// The measurement reports a hierarchy nothing reaches instead of letting the
/// build discover it: the count is what a caller needs to refuse the container,
/// and it is the count `SceneGraph::build` would refuse with `Cycle`.
#[test]
fn accept_f18_a_an_unreachable_parent_chain_is_measured_before_the_build_sees_it() {
    // Records 1 and 2 name each other and agree in both directions: a detached
    // ownership cycle, which the adopted rule does not repair because no rule
    // can. Nothing outside the loop lists either of them, so the container's two
    // statements are consistent and only reachability fails.
    let records = container(vec![
        stored_node(0, "world1", None, &[]),
        stored_node(1, "loop_a", Some(2), &[2]),
        stored_node(2, "loop_b", Some(1), &[1]),
    ]);

    let audit: StoredHierarchyAudit = audit_stored_hierarchy(&records);
    assert_eq!(
        audit.verdict(),
        &HierarchyVerdict::Consistent { child_slots: 2 },
        "a loop whose two sides agree is still consistent as a *statement*"
    );
    assert_eq!(
        audit.unreachable(),
        2,
        "and two records no root reaches, which is the refusal measured in advance"
    );

    let error = world_scene_graph_from_gamez(
        &cid(ContentKind::SceneNode, "container.synthetic"),
        &records,
        &[],
        &radian_adapter(),
        &BindingMap::default(),
    )
    .expect_err("the build still refuses a detached cycle");
    let WorldSceneError::Build { source, audit } = error else {
        panic!("expected the build refusal, got {error}");
    };
    assert!(
        matches!(source, SceneError::Cycle { .. }),
        "with its own reason"
    );
    assert_eq!(
        audit.unreachable(),
        2,
        "and the blocker still carries the measurement"
    );
}

// ------------------------------------------------------------------- retail ---

/// The retail root, or a loud failure.
///
/// CI has no `CS_GAME_DIR` and the tests that call this are `#[ignore]`d, so the
/// expectation is that the variable is set when they run. A test that skipped
/// itself here would report a pass it never earned.
fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"))
}

/// One world container of the installation, decoded by the production reader.
fn retail_container(group: &str) -> (String, GameZNodes) {
    let bytes = std::fs::read(retail_root().join(format!("ZBD/{group}/gamez.zbd"))).unwrap_or_else(
        |error| panic!("zbd/{group}/gamez.zbd: the installation must hold it: {error}"),
    );
    let label = format!("zbd/{group}/gamez.zbd");
    let records = read_gamez_nodes(&mut ParseContext::with_defaults(label.clone()), &bytes)
        .unwrap_or_else(|error| panic!("{label}: the node array must read: {error}"));
    (label, records)
}

/// The measured counts of each world container, from the corpus. Every number
/// here is the production reader's own output; the test recomputes them and
/// compares, so a reader that stopped reading the child lists or the parent
/// slots fails here rather than silently agreeing.
const RETAIL_WORLD_HIERARCHY: [(&str, u32, usize, usize, usize, usize); 8] = [
    // group, nodes, world node's stored list, records naming it, omitted from it
    ("C1", 7_064, 6_553, 66, 412, 346),
    ("C1B", 5_603, 5_300, 78, 233, 155),
    ("C1C", 5_644, 5_202, 53, 346, 293),
    ("C2", 4_956, 4_532, 24, 282, 258),
    ("C2B", 4_901, 4_465, 48, 338, 290),
    ("C3", 5_408, 4_812, 14, 453, 439),
    ("C4", 8_289, 7_776, 51, 401, 350),
    ("C5", 11_438, 10_752, 105, 576, 471),
];

/// **The disagreement is real, it is entirely about the world node, and it
/// goes one way only.**
///
/// This is the measurement that decides the question the task asked. Over all
/// eight world containers of the installation: no record is listed by a node it
/// does not name as its parent, no record is listed by two parents, and every
/// record that names a parent which does not list it names **the world node** —
/// the single parent whose list is partial, in every container. Under the
/// adopted rule every record of every container is reachable from a root, so no
/// container is refused for a cycle or a dangling link.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_a_retail_every_world_container_says_which_side_its_hierarchy_disagrees_on() {
    for (group, nodes, child_slots, world_list, claims, omitted) in RETAIL_WORLD_HIERARCHY {
        let (label, records) = retail_container(group);
        assert_eq!(
            records.nodes.len(),
            nodes as usize,
            "{label}: the stored record count"
        );

        let audit = audit_stored_hierarchy(&records);
        assert_eq!(
            audit.verdict(),
            &HierarchyVerdict::PartialChildIndex {
                omitted,
                parents: 1,
                child_slots
            },
            "{label}: the verdict is a partial child index over exactly one parent"
        );
        assert_eq!(
            audit.child_slots(),
            child_slots,
            "{label}: every stored child slot"
        );
        assert_eq!(
            audit.named_but_unlisted(),
            omitted,
            "{label}: the omitted records"
        );
        assert_eq!(
            audit.listed_but_not_naming(),
            0,
            "{label}: nothing disagrees the other way, so no link is contradicted"
        );
        assert_eq!(
            audit.listed_twice(),
            0,
            "{label}: no record is claimed twice"
        );
        assert_eq!(
            audit.partial_parents(),
            1,
            "{label}: exactly one parent holds a partial list"
        );
        assert_eq!(
            audit.unreachable(),
            0,
            "{label}: every record is reachable from a root under the adopted rule"
        );
        assert!(audit.is_convertible(), "{label}: the rule applies");

        // The partial list belongs to the world node, and that node's own stored
        // list is shorter than the number of records naming it. That is the
        // finding: a world node's child list is not the whole story.
        let world = world_node_slot(&records).expect("a world container holds a world node");
        let world_record = records.get(world).expect("the world node's record");
        assert_eq!(
            world_record.children.len(),
            world_list,
            "{label}: the world node's stored child list, as the reader read it"
        );
        let naming = records
            .nodes
            .iter()
            .filter(|node| node.parent == Some(world))
            .count();
        assert_eq!(naming, claims, "{label}: the records that name it");
        assert_eq!(
            naming - world_list,
            omitted,
            "{label}: and the difference is exactly the omitted count"
        );
    }
}

/// **The adopted rule is what gets the eight world containers past the
/// hierarchy; what still refuses them is F11-A's id scheme, reported as a typed
/// blocker with the exact count.**
///
/// Read through `world_scene_graph_from_gamez`, every world container reconciles
/// and then reaches `SceneGraph::build`, which refuses it — never with
/// `InconsistentParentage`, which is the refusal the rule exists to resolve, but
/// with the authored node names meeting F11-A's id grammar. The blocker carries
/// both halves: the count the rule resolved and the refusal that remains, so a
/// caller can say exactly which is which.
///
/// The same test reads **two** containers end to end and asserts the verdict
/// itself, and the aircraft container is read too: its count is 0, so the rule
/// is a no-op there and its own build refusal is unchanged.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_a_retail_the_world_containers_convert_their_hierarchy_and_report_the_refusal_that_remains()
 {
    let mut reconciled = 0usize;
    let mut blocked = 0usize;
    for (group, nodes, _, _, _, omitted) in RETAIL_WORLD_HIERARCHY {
        let (label, records) = retail_container(group);
        let error = world_scene_graph_from_gamez(
            &cid(
                ContentKind::SceneNode,
                &format!("container.zbd.{}", group.to_lowercase()),
            ),
            &records,
            &[],
            &radian_adapter(),
            &BindingMap::default(),
        )
        .expect_err("the canonical build refuses these world node names");

        let WorldSceneError::Build { source, audit } = error else {
            panic!(
                "{label}: the hierarchy must reconcile before anything else can refuse it, got \
                 {error}"
            );
        };
        assert!(
            !matches!(source, SceneError::InconsistentParentage { .. }),
            "{label}: the refusal the adopted rule resolves must not be the one that remains: {source}"
        );
        assert_eq!(
            audit.named_but_unlisted(),
            omitted,
            "{label}: the blocker carries the exact count the rule resolved"
        );
        assert_eq!(
            audit.nodes(),
            nodes as usize,
            "{label}: and the record count it measured"
        );
        reconciled += 1;
        blocked += 1;
        println!("{label}: hierarchy reconciles, then {source}");
    }
    assert_eq!(
        (reconciled, blocked),
        (8, 8),
        "all eight world containers were read"
    );

    // Two containers end to end, asserted individually: this is the pair the
    // task's acceptance criterion names, and the numbers are the measurement
    // rather than a property of the fixture.
    for (group, expected_omitted) in [("C1", 346usize), ("C5", 471usize)] {
        let (label, records) = retail_container(group);
        let hierarchy = world_hierarchy_from_gamez(&records, &[])
            .unwrap_or_else(|error| panic!("{label}: the rule applies: {error}"));
        assert_eq!(
            hierarchy.audit().named_but_unlisted(),
            expected_omitted,
            "{label}: the measured disagreement"
        );
        let world = world_node_slot(&records).expect("a world node");
        let derived = &hierarchy.nodes()[world as usize].children;
        assert!(
            derived.len() > records.get(world).expect("the world node").children.len(),
            "{label}: the derived child list is longer than the stored one, by exactly the \
             omitted count: {} vs {}",
            derived.len(),
            records.get(world).expect("the world node").children.len()
        );
        let stored: BTreeSet<u32> = records
            .get(world)
            .expect("the world node")
            .children
            .iter()
            .copied()
            .collect();
        let derived_set: BTreeSet<u32> = derived.iter().copied().collect();
        assert!(
            derived.iter().all(|child| records
                .get(*child)
                .is_some_and(|node| node.parent == Some(world))),
            "{label}: every derived child is a record that names the world node, so the rule adds \
             no link the store did not state"
        );
        assert!(
            stored.is_subset(&derived_set),
            "{label}: and the stored list is a subset of the derived one, so the rule removed \
             nothing either"
        );
    }

    // The aircraft container: the disagreement is 0, the verdict is
    // `Consistent`, and its build refusal is the one F11-A's id scheme has
    // always produced — untouched by this stage.
    let bytes = std::fs::read(retail_root().join("ZBD/planes.zbd"))
        .expect("the installation must hold zbd/planes.zbd");
    let planes = read_gamez_nodes(
        &mut ParseContext::with_defaults("zbd/planes.zbd".to_owned()),
        &bytes,
    )
    .expect("the aircraft container's node array must read");
    let audit = audit_stored_hierarchy(&planes);
    assert_eq!(
        audit.verdict(),
        &HierarchyVerdict::Consistent { child_slots: 3_289 },
        "the aircraft container's two sides agree on all 3 289 links"
    );
    assert_eq!(audit.nodes(), 3_317);
    assert_eq!(audit.roots(), 28);
    assert_eq!(audit.unreachable(), 0);
}
