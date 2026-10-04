//! #628: the world containers' `scene_node` id verdicts.
//!
//! Task test prefix: `accept_m01_lc_world_scene_`. Finding:
//! `docs/findings/2026-10-04-m01-lc-world-scene-ids.md`.
//!
//! Two refusals were left after F11-E1's escaping: two siblings sharing one
//! name (`SceneError::DuplicateNodeId`) and a name-path past the key bound
//! (`SceneError::NodeId`). The rule under test is that siblings which would
//! share an id each take the stored `node_index` word as a second component,
//! and that an over-long path is spelled by a digest, with the authored path
//! staying on the node. The synthetic half runs in CI; the retail half reads the
//! original installation and claims nothing about the original engine.

use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{
    BindingMap, ParsedNode, ParsedNodeKind, SceneError, SceneGraph, SceneNodeId,
};
use cs_content::world::world_scene_graph_from_gamez;
use cs_types::content::{ContentId, ContentKind};

use crate::hierarchy::{cid, radian_adapter, retail_container};

fn adapter() -> SourceAdapter {
    radian_adapter()
}

/// A parent/children pair of records: `nodes[i]` has slot `i`.
fn tree(spec: &[(&str, Option<u32>, Option<u32>)]) -> Vec<ParsedNode> {
    let mut nodes: Vec<ParsedNode> = spec
        .iter()
        .enumerate()
        .map(|(slot, (name, parent, word))| {
            let mut node = ParsedNode::new(slot as u32, *name, ParsedNodeKind::Object3d);
            node.parent = *parent;
            node.disambiguator = *word;
            node
        })
        .collect();
    for slot in 0..nodes.len() {
        if let Some(parent) = nodes[slot].parent {
            nodes[parent as usize].children.push(slot as u32);
        }
    }
    nodes
}

fn container() -> ContentId {
    cid(ContentKind::SceneNode, "container.zbd.c9")
}

fn build(nodes: &[ParsedNode]) -> Result<SceneGraph, SceneError> {
    SceneGraph::build(&container(), nodes, &adapter(), &BindingMap::default())
}

#[test]
fn accept_m01_lc_world_scene_siblings_sharing_a_name_take_their_stored_word() {
    let nodes = tree(&[
        ("world1", None, Some(1)),
        ("Lamp", Some(0), Some(0x20)),
        ("lamp", Some(0), Some(0x21)),
        ("post", Some(0), Some(0x22)),
        ("bulb", Some(1), Some(0x30)),
        ("bulb", Some(2), Some(0x31)),
    ]);
    let graph = build(&nodes).expect("the stored words tell the siblings apart");
    let keys: Vec<&str> = graph.nodes().iter().map(|node| node.id().key()).collect();
    for expected in [
        "container.zbd.c9.world1",
        "container.zbd.c9.world1.lamp-i20",
        "container.zbd.c9.world1.lamp-i21",
        "container.zbd.c9.world1.post",
        "container.zbd.c9.world1.lamp-i20.bulb",
        "container.zbd.c9.world1.lamp-i21.bulb",
    ] {
        assert!(keys.contains(&expected), "{expected} missing from {keys:?}");
    }
    for node in graph.nodes() {
        let names = node
            .id()
            .authored_names(&container())
            .expect("a spelled id reads back");
        assert_eq!(
            names.join(".").to_ascii_lowercase(),
            node.path().to_ascii_lowercase(),
            "the id reads back to the authored path"
        );
    }
}

#[test]
fn accept_m01_lc_world_scene_the_rule_never_resolves_by_position() {
    // No stored word: the collision is still refused.
    let none = tree(&[
        ("w", None, None),
        ("a", Some(0), None),
        ("a", Some(0), None),
    ]);
    assert!(matches!(
        build(&none),
        Err(SceneError::DuplicateNodeId { .. })
    ));
    // One stored word missing, or two equal: still refused.
    let half = tree(&[
        ("w", None, None),
        ("a", Some(0), Some(5)),
        ("a", Some(0), None),
    ]);
    assert!(matches!(
        build(&half),
        Err(SceneError::DuplicateNodeId { .. })
    ));
    let equal = tree(&[
        ("w", None, None),
        ("a", Some(0), Some(5)),
        ("a", Some(0), Some(5)),
    ]);
    assert!(matches!(
        build(&equal),
        Err(SceneError::DuplicateNodeId { .. })
    ));
    // Reordering the records moves no id.
    let one = tree(&[
        ("w", None, None),
        ("a", Some(0), Some(1)),
        ("a", Some(0), Some(2)),
    ]);
    let two = tree(&[
        ("w", None, None),
        ("a", Some(0), Some(2)),
        ("a", Some(0), Some(1)),
    ]);
    let id_of_word = |nodes: &[ParsedNode], word: u32| {
        let graph = build(nodes).expect("builds");
        let slot = nodes
            .iter()
            .find(|n| n.disambiguator == Some(word))
            .expect("word")
            .index;
        graph
            .nodes()
            .iter()
            .find(|n| n.index() == slot)
            .expect("node")
            .id()
            .clone()
    };
    assert_eq!(id_of_word(&one, 1), id_of_word(&two, 1));
    assert_eq!(id_of_word(&one, 2), id_of_word(&two, 2));
}

#[test]
fn accept_m01_lc_world_scene_an_overlong_path_is_spelled_by_a_digest() {
    let mut spec: Vec<(String, Option<u32>)> = vec![("root".to_owned(), None)];
    for depth in 1..=12u32 {
        spec.push((format!("segment_number_{depth}"), Some(depth - 1)));
    }
    let nodes = tree(
        &spec
            .iter()
            .map(|(name, parent)| (name.as_str(), *parent, None))
            .collect::<Vec<_>>(),
    );
    let graph = build(&nodes).expect("an over-long path no longer refuses");
    let deepest = graph.nodes().last().expect("nodes");
    assert!(deepest.id().key().len() <= 128, "the key fits the bound");
    assert!(
        deepest.id().key().starts_with("container.zbd.c9.-h"),
        "{}",
        deepest.id().key()
    );
    assert_eq!(deepest.id().authored_names(&container()), None);
    assert_eq!(
        deepest.path().split('.').count(),
        13,
        "the authored path is kept whole on the node"
    );
    assert!(
        graph.nodes()[1].id().key().ends_with("segment_number_1"),
        "a path that fits keeps its spelled id"
    );
}

/// The measured retail counts: group, nodes, roots, digest-spelled ids, nodes
/// whose own component carries a stored word and is not hidden by a digest.
const RETAIL: [(&str, usize, usize, usize, usize); 8] = [
    ("C1", 7_064, 165, 83, 124),
    ("C1B", 5_603, 148, 98, 103),
    ("C1C", 5_644, 149, 292, 30),
    ("C2", 4_956, 166, 72, 151),
    ("C2B", 4_901, 146, 97, 34),
    ("C3", 5_408, 157, 72, 46),
    ("C4", 8_289, 163, 102, 179),
    ("C5", 11_438, 215, 382, 732),
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_scene_retail_every_world_container_converts() {
    for (group, nodes, roots, digests, disambiguated) in RETAIL {
        let (label, records) = retail_container(group);
        let world = world_scene_graph_from_gamez(
            &cid(
                ContentKind::SceneNode,
                &format!("container.zbd.{}", group.to_lowercase()),
            ),
            &records,
            &[],
            &adapter(),
            &BindingMap::default(),
        )
        .unwrap_or_else(|error| panic!("{label}: must convert: {error}"));
        let graph = world.graph();
        let container = cid(
            ContentKind::SceneNode,
            &format!("container.zbd.{}", group.to_lowercase()),
        );
        let mut digest_ids = 0;
        let mut word_ids = 0;
        for node in graph.nodes() {
            let id: &SceneNodeId = node.id();
            assert!(id.key().len() <= 128, "{label}: key bound");
            match id.authored_names(&container) {
                None => {
                    digest_ids += 1;
                    assert!(id.key().contains(".-h"), "{label}: only digests are opaque");
                }
                Some(names) => assert!(
                    names.join(".").eq_ignore_ascii_case(node.path()),
                    "{label}: the id reads back to the authored path"
                ),
            }
            let last = id.key().rsplit('.').next().expect("component");
            if let Some(at) = last.rfind("-i") {
                word_ids += 1;
                let record = records.get(node.index()).expect("stored record");
                assert_eq!(
                    last[at + 2..],
                    format!("{:x}", record.node_index & 0x00ff_ffff),
                    "{label}: the suffix is the stored word"
                );
            }
        }
        assert_eq!(graph.len(), nodes, "{label}: nodes");
        assert_eq!(graph.roots().len(), roots, "{label}: roots");
        assert_eq!(word_ids, disambiguated, "{label}: disambiguated components");
        assert_eq!(digest_ids, digests, "{label}: digest-spelled ids");
    }
}
