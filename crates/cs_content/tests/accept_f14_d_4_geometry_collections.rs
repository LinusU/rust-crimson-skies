//! Acceptance scenario F14-D.4: the `scene_node` and `mesh` collections of the
//! retail baseline inventory (`specs/F14-canonical-content-catalog-and-
//! dependency-closure.md`, stage `### F14-D`; follow-up task #487).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires a catalog collection for
//! "scene roots and nodes" and for "render meshes/materials/images". No
//! [`cs_types::content::ContentKind::SceneNode`] or
//! [`cs_types::content::ContentKind::Mesh`] row existed in
//! `cs_content::catalog::baseline::retail_baseline`; this stage adds both.
//!
//! The non-retail tests write a **synthetic installation tree** into a temporary
//! directory whose `ZBD/planes.zbd` and `ZBD/C1/gamez.zbd` are authored CS GameZ
//! containers (the header, the mesh section and the two-pass node array the
//! format defines), and run the production baseline builder over it. Removing
//! the collection from the baseline fails every one of them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! installation and pins what it really holds; run it with
//! `--include-ignored`. Without `CS_GAME_DIR` it fails loudly rather than
//! passing vacuously.
//!
//! Every byte, name and count in the fixtures below is authored for this file,
//! like the fixtures in `crates/cs_content/tests/scene.rs`; no original content
//! is written to Git.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::campaign_bindings::CampaignInventory;
use cs_content::catalog::baseline::{
    Baseline, GAMEZ_CONTAINER_RECORD, GEOMETRY_CONTAINER_FILE, baseline_report_json,
    install_file_key, retail_baseline,
};
use cs_types::content::{
    CatalogElement, ContentId, ContentKind, NormalizeState, Origin, Readiness, UnsupportedReason,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::ParseState;

/// The GameZ signature a Crimson Skies container stores.
const SIGNATURE: u32 = 0x0297_1222;
/// The Crimson Skies container version.
const VERSION: u32 = 42;
/// `textures_offset` must equal the header size; the reader asserts it.
const TEXTURES_OFFSET: u32 = 40;
/// `materials_offset`: one word past the texture table.
const MATERIALS_OFFSET: u32 = 44;
/// `meshes_offset`: one word past the material table.
const MESHES_OFFSET: u32 = 48;
/// Bytes of one node's info slot: the 36-byte name plus the 208-byte record.
const NODE_SLOT: u32 = 212;
/// Bytes of one object node's own data record.
const OBJECT_DATA: u32 = 144;
/// `NodeType::Object3d`.
const KIND_OBJECT3D: u32 = 5;
/// `NODE_INDEX_TOP | slot`: the reference asserts the top byte of every stored
/// `node_index` word, so the fixture stores it.
const NODE_INDEX_TOP: u32 = 0x0200_0000;

/// One node the fixture writes: a name, a mesh slot, and its parent.
#[derive(Clone)]
struct FixtureNode {
    name: String,
    /// `-1` stores "no mesh", the value the reader maps to no binding.
    mesh_index: i32,
    parent: Option<u32>,
    /// The child slots this node's record stores. A GameZ container's parent
    /// slots and its child lists are two independent claims about the hierarchy
    /// and the world containers' disagree (recorded as an unknown in
    /// `docs/findings/2026-10-02-gamez-node-array-layout.md`), so the fixture
    /// writes both and, for one node, makes them disagree on purpose.
    children: Vec<u32>,
}

/// A node whose record is 144 bytes and whose fields the layout pins.
fn object(name: &str, mesh_index: i32, parent: Option<u32>) -> FixtureNode {
    FixtureNode {
        name: name.to_owned(),
        mesh_index,
        parent,
        children: Vec::new(),
    }
}

/// Fills every node's child list from the parent slots, which is what the
/// reference asserts a container does.
///
/// `unlisted` names a node whose **parent slot is stored but which no parent's
/// child list names** — the disagreement the eight world containers carry. The
/// baseline walks the parent slots, so that node is still a row with the parent
/// its own record states.
fn with_child_lists(nodes: &mut [FixtureNode], unlisted: &[u32]) {
    let parents: Vec<Option<u32>> = nodes.iter().map(|node| node.parent).collect();
    for (index, node) in nodes.iter_mut().enumerate() {
        node.children = parents
            .iter()
            .enumerate()
            .filter(|(_, parent)| **parent == Some(index as u32))
            .map(|(child, _)| child as u32)
            .filter(|child| !unlisted.contains(child))
            .collect();
    }
}

/// Writes one CS GameZ container holding `nodes` and a mesh array of
/// `mesh_slots` entries whose first `present_meshes` hold three stored vertex
/// positions each.
///
/// The layout is the format's own, in the order the two production readers walk
/// it: the 40-byte header, the mesh index at `MESHES_OFFSET`, the
/// `100 + 4`-byte record array, the back-to-back mesh data, and then the
/// two-pass node array at `nodes_offset`. The header's `nodes_offset` is the
/// first byte of the node info array, so the mesh walk ends exactly where the
/// node walk begins — which is the boundary
/// `cs_content::catalog::baseline` cross-checks.
///
/// Nothing here is copied from an original container: every name, count and
/// offset is this fixture's own, and the two readers are what decides whether
/// the result is a container at all.
fn write_container(nodes: &[FixtureNode], mesh_slots: u32, present_meshes: u32) -> Vec<u8> {
    assert!(!nodes.is_empty(), "a fixture always declares a node");
    assert!(
        present_meshes <= mesh_slots,
        "only a prefix of the mesh array may be present"
    );
    const VERTICES_PER_MESH: u32 = 3;
    const MESH_DATA: u32 = VERTICES_PER_MESH * 12;

    let records_start = MESHES_OFFSET + 12;
    let data_start = records_start + mesh_slots * 104;
    let data_end = data_start + present_meshes * MESH_DATA;
    let nodes_offset = data_end;
    let info_end = nodes_offset + NODE_SLOT * nodes.len() as u32;

    // The data section starts where the info array ends, so every record's own
    // offset is computable before anything is written — exactly as the reader
    // computes it by walking. An object record is followed by its own 4-byte
    // parent slot, and that word is present exactly when the record's
    // `parent_count` boolean is set, so a root's slot is 144 bytes long.
    let mut record_offsets = Vec::with_capacity(nodes.len());
    let mut at = info_end;
    for node in nodes {
        record_offsets.push(at);
        at += record_length(node);
    }

    let mut bytes = vec![0u8; at as usize];
    let word = |bytes: &mut Vec<u8>, offset: usize, value: u32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, offset: usize, value: f32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    let signed = |bytes: &mut Vec<u8>, offset: usize, value: i32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };

    for (offset, value) in [
        (0usize, SIGNATURE),
        (4, VERSION),
        // `unk08` selects a fixup table; the fixture selects none, so the
        // sequential expectation applies.
        (8, 0),
        (12, 0),
        (16, TEXTURES_OFFSET),
        (20, MATERIALS_OFFSET),
        (24, MESHES_OFFSET),
        (28, nodes.len() as u32),
        (32, 0),
        (36, nodes_offset),
    ] {
        word(&mut bytes, offset, value);
    }

    // The mesh index, then one record per slot. A present record stores its own
    // data offset in the word after its 100 bytes; a stub stores the index the
    // next present record is expected to carry, which at the end of the array
    // is `-1`.
    signed(&mut bytes, MESHES_OFFSET as usize, mesh_slots as i32);
    signed(
        &mut bytes,
        MESHES_OFFSET as usize + 4,
        present_meshes as i32,
    );
    // `last_index` is what the index expects the **last present** record to be
    // followed by: its own index plus one, or `-1` at the end of the array. It is
    // the same expectation the stub records carry, computed once here.
    signed(
        &mut bytes,
        MESHES_OFFSET as usize + 8,
        if present_meshes == 0 || present_meshes == mesh_slots {
            -1
        } else {
            i32::try_from(present_meshes).expect("a fixture mesh count fits")
        },
    );
    for slot in 0..mesh_slots {
        let record = records_start as usize + (slot as usize) * 104;
        if slot < present_meshes {
            // `parent_count > 0` is the layout's own test for a present record.
            word(&mut bytes, record + 12, 1);
            word(&mut bytes, record + 20, VERTICES_PER_MESH);
            // The word after the 100-byte record is a present mesh's data offset.
            word(&mut bytes, record + 100, data_start + slot * MESH_DATA);
        } else {
            // The same word after a stub is the index the next present record is
            // expected to carry, which at the end of the array is `-1`.
            signed(&mut bytes, record + 100, -1);
        }
    }
    for slot in 0..present_meshes {
        let mesh = data_start as usize + (slot as usize) * MESH_DATA as usize;
        for vertex in 0..VERTICES_PER_MESH {
            for axis in 0..3u32 {
                let value = (slot * 100 + vertex * 10 + axis) as f32;
                float(&mut bytes, mesh + (vertex * 12 + axis * 4) as usize, value);
            }
        }
    }

    for (index, node) in nodes.iter().enumerate() {
        let at = nodes_offset as usize + NODE_SLOT as usize * index;
        let name = node.name.as_bytes();
        assert!(name.len() < 36, "a fixture name fits its 36-byte field");
        bytes[at..at + name.len()].copy_from_slice(name);
        word(&mut bytes, at + 36, 0x0180_0000);
        word(&mut bytes, at + 40, 0);
        word(&mut bytes, at + 44, 1);
        word(&mut bytes, at + 48, 255);
        word(&mut bytes, at + 52, KIND_OBJECT3D);
        word(&mut bytes, at + 56, record_offsets[index]);
        signed(&mut bytes, at + 60, node.mesh_index);
        word(&mut bytes, at + 64, 0);
        word(&mut bytes, at + 68, 1);
        word(&mut bytes, at + 72, 0);
        // `parent_count` is a boolean in this layout.
        bytes[at + 84..at + 86].copy_from_slice(&u16::from(node.parent.is_some()).to_le_bytes());
        bytes[at + 86..at + 88].copy_from_slice(&(node.children.len() as u16).to_le_bytes());
        // `unk196` is the layout's `160` for an object record.
        word(&mut bytes, at + 196, 160);
        word(&mut bytes, at + 208, NODE_INDEX_TOP | index as u32);

        let record = record_offsets[index] as usize;
        // The object record: an identity transform, which the layout pins to
        // `flags == 40` with an exactly identity matrix.
        word(&mut bytes, record, 40);
        for axis in 0..3u32 {
            float(&mut bytes, record + 36 + 4 * axis as usize, 1.0);
            float(
                &mut bytes,
                record + 48 + 4 * (3 * axis + axis) as usize,
                1.0,
            );
        }
        // The record's own parent slot, read only when the record declares one.
        if let Some(parent) = node.parent {
            word(&mut bytes, record + OBJECT_DATA as usize, parent);
        }
        // The child slots follow the parent word, in stored order.
        let mut at = record + OBJECT_DATA as usize;
        if node.parent.is_some() {
            at += 4;
        }
        for child in &node.children {
            word(&mut bytes, at, *child);
            at += 4;
        }
    }
    bytes
}

/// Bytes one node's own data slot occupies: the object record plus the 4-byte
/// parent word the layout stores when the record declares a parent.
fn record_length(node: &FixtureNode) -> u32 {
    OBJECT_DATA + if node.parent.is_some() { 4 } else { 0 } + 4 * node.children.len() as u32
}

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f14-d-4-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture directory is created");
        Self(root)
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("a parent directory"))
            .expect("the fixture directory is created");
        fs::write(&path, bytes).expect("the fixture file is written");
    }
}

impl Drop for TempInstall {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One campaign mission directory, so the baseline has a launchable root and
/// the coverage accounting has something to walk.
fn write_campaign(temp: &TempInstall) {
    temp.write("ZBD/C1C/M01/zrdr.zbd", b"mission program bytes");
    temp.write("ZBD/C1C/M01/mis_anim.zbd", b"mission animation bytes");
}

/// The airframe the tests build every fixture container around: a root, a child
/// carrying one mesh, a child carrying none, a sibling that names a mesh slot
/// the array leaves absent, and the two nodes that make a name path ambiguous
/// and a name unspellable.
fn fixture_nodes() -> Vec<FixtureNode> {
    let mut nodes = vec![
        object("main", 0, None),
        object("wing", 1, Some(0)),
        object("hatch", -1, Some(0)),
        object("ghost", 2, Some(0)),
        object("twig", -1, Some(0)),
        object("twig", -1, Some(0)),
        object("brigturret ", -1, Some(1)),
        // The node whose parent slot is stored but which `main`'s own child list
        // does not name: the world containers' disagreement, on purpose.
        object("gun", -1, Some(0)),
    ];
    with_child_lists(&mut nodes, &[7]);
    nodes
}

/// A fixture tree: the campaign mission, the shared planes container, and the
/// own geometry container of **every** world group the tree discovers.
///
/// The mission's own group is written too, so a fixture never carries a
/// discovered group whose `gamez.zbd` is absent: that case is what
/// `accept_f14_d_4_a_world_group_without_a_geometry_container_is_named_not_guessed`
/// builds deliberately.
fn tree(label: &str, planes_nodes: Vec<FixtureNode>, world_nodes: Vec<FixtureNode>) -> TempInstall {
    let temp = TempInstall::new(label);
    write_campaign(&temp);
    temp.write("ZBD/planes.zbd", &write_container(&planes_nodes, 3, 2));
    for group in ["C1", "C1C"] {
        temp.write(
            &format!("ZBD/{group}/{GEOMETRY_CONTAINER_FILE}"),
            &write_container(&world_nodes, 3, 2),
        );
    }
    temp
}

/// How many containers `tree` writes a geometry container into.
const FIXTURE_CONTAINERS: usize = 3;
/// Stored nodes per fixture container.
const FIXTURE_NODES: usize = 8;
/// Present mesh slots the node array names, per fixture container.
const FIXTURE_MESH_ROWS: usize = 2;

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

/// The container key the baseline derives for one spelling.
fn container_key(spelling: &str) -> String {
    install_file_key(spelling)
}

/// The rows of one kind, by id key, in canonical order.
fn rows_of(baseline: &Baseline, kind: ContentKind) -> Vec<&CatalogElement> {
    baseline
        .catalog
        .elements()
        .filter(|element| element.kind == kind)
        .collect()
}

fn row<'a>(baseline: &'a Baseline, id: &ContentId) -> &'a CatalogElement {
    baseline
        .catalog
        .get(id)
        .unwrap_or_else(|| panic!("the catalog holds {id}"))
}

fn unknown_claims(element: &CatalogElement) -> Vec<String> {
    element
        .unsupported_reasons
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

/// Every node the fixture stores, and the two meshes its array really holds.
#[test]
fn accept_f14_d_4_a_gamez_container_yields_one_node_row_per_node_and_one_mesh_row_per_named_slot() {
    let temp = tree("complete", fixture_nodes(), fixture_nodes());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    for s in &baseline.collection_status {
        println!("DIAG {} {:?}", s.kind.label(), s.diagnostic);
    }
    let world = "ZBD/C1/gamez.zbd";
    let planes = "ZBD/planes.zbd";
    let world_key = container_key(world);

    // Seven stored nodes per container, three containers: one row per node,
    // never one row per name.
    let nodes = rows_of(&baseline, ContentKind::SceneNode);
    assert_eq!(
        nodes.len(),
        FIXTURE_CONTAINERS * FIXTURE_NODES,
        "one row per stored node of each container the diagnosis names"
    );
    assert!(
        !ContentKind::SceneNode.is_launchable(),
        "a scene node is not launchable content, so the denominator cannot move"
    );

    // The rows the authored names identify are keyed by the container and the
    // authored name path — the identity F11-A published, not a position.
    for spelling in [world, planes] {
        // The fixture's own hierarchy is a root and three children under it, so
        // the authored name paths are `main` and `main.<child>`.
        let container = spelling;
        for path in ["main", "main.wing", "main.hatch", "main.ghost"] {
            let id = cid(
                ContentKind::SceneNode,
                &format!("{}.{path}", container_key(spelling)),
            );
            let element = row(&baseline, &id);
            assert_eq!(
                element.display_name.as_deref(),
                Some(path.rsplit('.').next().expect("a name path has a leaf")),
                "{id} is the node the name path names, and the display name is the name the \
                 container stores for it"
            );
            assert_eq!(element.kind, ContentKind::SceneNode);
            assert!(matches!(element.origin, Origin::Installation { .. }));
            assert_eq!(element.parse_state, ParseState::Parsed);
            assert_eq!(element.normalize_state, NormalizeState::NotNormalized);
            assert_eq!(element.readiness, Readiness::Unavailable);
            assert!(element.runtime_consumers.is_empty());
            assert!(
                !element.fingerprint.is_none(),
                "the row names the bytes it was read from"
            );
            // Nothing about this node's identity is unknown: its name path is
            // its own and the id grammar accepts it.
            assert_eq!(
                unknown_claims(element),
                Vec::<String>::new(),
                "a node the store distinguishes by its name needs no identity unknown"
            );
            assert!(
                element
                    .unsupported_reasons
                    .iter()
                    .any(|reason| reason.code() == "not_normalized"),
                "the stored transform is in source units, which nothing has calibrated"
            );

            // The span is the node's own record inside the container, and the
            // container is a loose file: there is no member key to record.
            let span = element.origin.source().expect("an installation span");
            assert_eq!(span.container_path(), container);
            assert_eq!(
                span.member_key(),
                None,
                "a GameZ container is a loose installation file and is its own container"
            );
            assert!(
                span.offset() > 0,
                "a record inside the container, not the container header"
            );
            assert!(span.length() > 0, "a record with bytes of its own");
            assert_eq!(
                span.install_sha256().to_hex(),
                baseline.install_sha256,
                "the span names the installation whose bytes were read"
            );

            // The static edge onto the inventory row of the file that holds the
            // bytes, plus whatever the record itself names.
            let file_id = cid(ContentKind::InstallFile, &install_file_key(container));
            assert!(
                element
                    .dependencies
                    .iter()
                    .any(|edge| edge.target == file_id),
                "{id} points at the inventory row of the file that holds its bytes"
            );
            for edge in &element.dependencies {
                assert_eq!(edge.kind.label(), "static");
                assert_eq!(
                    edge.provenance.class,
                    ClaimStatus::ObservedTool,
                    "an agent-observed edge is never verified_original"
                );
                assert_eq!(
                    edge.provenance.source.as_ref().map(|span| span.offset()),
                    Some(span.offset()),
                    "every edge of a row is observed in the row's own bytes"
                );
            }
        }
    }

    // The hierarchy comes from the **parent slots**, not from the child lists:
    // `gun`'s own record names `main` as its parent while `main`'s child list does
    // not list it — the disagreement the eight world containers carry. The row is
    // still there, its path still runs through `main`, and its parent edge still
    // resolves.
    let gun = row(
        &baseline,
        &cid(ContentKind::SceneNode, &format!("{world_key}.main.gun")),
    );
    assert_eq!(gun.display_name.as_deref(), Some("gun"));
    assert!(
        gun.dependencies
            .iter()
            .any(|edge| edge.target == cid(ContentKind::SceneNode, &format!("{world_key}.main"))),
        "a node's own parent slot is what names its parent: {:?}",
        gun.dependencies
            .iter()
            .map(|edge| edge.target.to_string())
            .collect::<Vec<_>>()
    );

    // The ownership edge: `wing` names `main` as its parent, so the row carries
    // an edge to the row the name path derives for `main`.
    let wing = row(
        &baseline,
        &cid(ContentKind::SceneNode, &format!("{world_key}.main.wing")),
    );
    assert!(
        wing.dependencies
            .iter()
            .any(|edge| edge.target == cid(ContentKind::SceneNode, &format!("{world_key}.main"))),
        "a child names its parent: {:?}",
        wing.dependencies
            .iter()
            .map(|edge| edge.target.to_string())
            .collect::<Vec<_>>()
    );
    let main = row(
        &baseline,
        &cid(ContentKind::SceneNode, &format!("{world_key}.main")),
    );
    assert!(
        !main
            .dependencies
            .iter()
            .any(|edge| { edge.target.kind() == ContentKind::SceneNode }),
        "a root declares no parent, so it carries no ownership edge"
    );

    // The mesh edge: `wing` stores slot 1 and `hatch` stores `-1`, so only the
    // first gets one.
    assert!(
        wing.dependencies
            .iter()
            .any(|edge| edge.target == cid(ContentKind::Mesh, &format!("{world_key}.1"))),
        "a stored mesh slot is a mesh edge"
    );
    let hatch = row(
        &baseline,
        &cid(ContentKind::SceneNode, &format!("{world_key}.main.hatch")),
    );
    assert!(
        !hatch
            .dependencies
            .iter()
            .any(|edge| edge.target.kind() == ContentKind::Mesh),
        "a stored mesh_index of -1 means no mesh, so no mesh edge follows"
    );

    // The mesh collection: one row per mesh slot the node array names **and**
    // that the array really holds. `ghost` names slot 2, which the fixture
    // leaves absent, so it has no row and is counted instead.
    let meshes = rows_of(&baseline, ContentKind::Mesh);
    let mut mesh_ids: Vec<String> = meshes
        .iter()
        .map(|element| element.id.key().to_owned())
        .collect();
    mesh_ids.sort();
    let mut expected_mesh_ids: Vec<String> = ["ZBD/planes.zbd", world, "ZBD/C1C/gamez.zbd"]
        .iter()
        .flat_map(|spelling| {
            [
                format!("{}.0", container_key(spelling)),
                format!("{}.1", container_key(spelling)),
            ]
        })
        .collect();
    expected_mesh_ids.sort();
    assert_eq!(
        mesh_ids, expected_mesh_ids,
        "one row per present slot the node array names, in canonical id order"
    );
    let mesh = row(
        &baseline,
        &cid(ContentKind::Mesh, &format!("{world_key}.1")),
    );
    assert_eq!(mesh.display_name, None, "a mesh stores no name");
    let span = mesh.origin.source().expect("an installation span");
    assert_eq!(span.container_path(), world);
    assert_eq!(span.member_key(), None);
    assert!(span.offset() > 0 && span.length() > 0);
    assert_eq!(
        span.length(),
        36,
        "the record's own extent: three stored vertex positions and nothing else"
    );
    assert_eq!(mesh.dependencies.len(), 1);
    assert_eq!(
        mesh.dependencies[0].target,
        cid(ContentKind::InstallFile, &install_file_key(world))
    );
    assert_eq!(
        mesh.dependencies[0].provenance.class,
        ClaimStatus::ObservedTool
    );
    assert_eq!(mesh.parse_state, ParseState::Parsed);
    assert_eq!(mesh.readiness, Readiness::Unavailable);
    assert!(
        mesh.unsupported_reasons
            .iter()
            .any(|reason| reason.code() == "not_normalized"),
        "the stored positions are in source units, which nothing has calibrated"
    );

    // Every edge in both collections resolves: no row references an id nothing
    // holds, so the closure's orphan count stays zero.
    assert_eq!(
        baseline.coverage.unresolved_references, 0,
        "a collection that pointed at ids it did not insert would report orphans"
    );
}

/// A name path the store spells for two nodes is not a semantic key, so those
/// nodes are **not** merged and **not** dropped: each is a row of its own, keyed
/// by its own record's address, carrying the one unknown that says the store does
/// not distinguish it.
#[test]
fn accept_f14_d_4_a_node_whose_name_path_is_shared_is_a_row_with_an_explicit_unknown() {
    let temp = tree("ambiguous", fixture_nodes(), fixture_nodes());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    // Seven stored nodes per container and one row each: the two `twig` nodes
    // are neither merged into one row nor dropped.
    assert_eq!(
        rows_of(&baseline, ContentKind::SceneNode).len(),
        FIXTURE_CONTAINERS * FIXTURE_NODES
    );

    let keyed_by_address: Vec<&CatalogElement> = rows_of(&baseline, ContentKind::SceneNode)
        .into_iter()
        .filter(|element| element.id.key().contains(GAMEZ_CONTAINER_RECORD))
        .collect();
    assert_eq!(
        keyed_by_address.len(),
        FIXTURE_CONTAINERS * 3,
        "two `twig` nodes and one `brigturret ` node per container, and nothing else"
    );
    let twigs: Vec<&&CatalogElement> = keyed_by_address
        .iter()
        .filter(|element| element.display_name.as_deref() == Some("twig"))
        .collect();
    assert_eq!(
        twigs.len(),
        FIXTURE_CONTAINERS * 2,
        "two `twig` nodes per container"
    );
    for element in keyed_by_address
        .iter()
        .filter(|element| element.display_name.as_deref() == Some("twig"))
    {
        assert_eq!(
            unknown_claims(element),
            vec!["f14.d.4.baseline.node_path_ambiguous".to_owned()],
            "the shared name path is this baseline's own claim, not a borrowed one"
        );
        let reason = element
            .unsupported_reasons
            .iter()
            .find_map(|reason| match reason {
                UnsupportedReason::Unknown { reason, .. } => Some(reason.as_str()),
                _ => None,
            })
            .expect("the row says what could not be derived");
        assert!(reason.contains("main.twig"), "{reason}");
        assert!(reason.contains("another node"), "{reason}");
        // Every address-keyed row still names the container it was read from, so
        // three containers that store the same names never share one identity.
        let own = install_file_key(
            element
                .origin
                .source()
                .expect("an installation span")
                .container_path(),
        );
        assert!(
            element.id.key().starts_with(&format!("{own}.")),
            "the identity is still the container's own: {}",
            element.id
        );
        // The row is located by its own record, so it is still a checked span
        // inside the container the row's own identity names.
        let span = element.origin.source().expect("an installation span");
        assert!(span.offset() > 0 && span.length() > 0);
        let size = fs::metadata(temp.0.join(span.container_path()))
            .expect("the container is a file")
            .len();
        assert!(
            span.offset() + span.length() <= size,
            "the record's own extent fits inside {}: {} bytes",
            span.container_path(),
            size
        );
    }
    // Two distinct records, so two distinct keys: the address is the node's own,
    // and it is unique **within** its container (the fixture containers happen to
    // be byte-identical, so the same offset recurs in more than one of them).
    let addresses: BTreeSet<(String, u64)> = keyed_by_address
        .iter()
        .map(|element| {
            let span = element.origin.source().expect("an installation span");
            (span.container_path().to_owned(), span.offset())
        })
        .collect();
    assert_eq!(addresses.len(), keyed_by_address.len());

    // The per-container record says how many nodes each way round, and the
    // collection's own gap counts agree with the rows.
    let world = baseline
        .geometry_containers
        .iter()
        .find(|report| report.spelling == "ZBD/C1/gamez.zbd")
        .expect("the world container is reported");
    assert_eq!(world.nodes, FIXTURE_NODES);
    assert_eq!(world.ambiguous, 2);
    assert_eq!(world.unspellable, 1);
    assert_eq!(world.named, 5);
    assert_eq!(
        world.paths, 7,
        "seven distinct paths for eight stored nodes"
    );
    assert_eq!(
        world.absent_meshes, 1,
        "`ghost` names the slot the array leaves absent"
    );

    let status = status_of(&baseline, ContentKind::SceneNode);
    assert_eq!(status.rows, FIXTURE_CONTAINERS * FIXTURE_NODES);
    assert_eq!(
        status.gaps.get("ambiguous_name_path"),
        Some(&(FIXTURE_CONTAINERS * 2))
    );
    assert_eq!(
        status.gaps.get("unspellable_name_path"),
        Some(&FIXTURE_CONTAINERS)
    );
    assert_eq!(
        status.gaps.get("container_visited"),
        Some(&FIXTURE_CONTAINERS)
    );
    assert_eq!(status.gaps.get("container_read"), Some(&FIXTURE_CONTAINERS));
    assert_eq!(status.gaps.get("unreadable_container"), Some(&0));
    assert_eq!(status.diagnostic, None);
}

/// A stored name carrying bytes the id grammar refuses is never transliterated
/// and never dropped: the row is keyed by the record's own address and the
/// unknown names the exact stored path.
#[test]
fn accept_f14_d_4_a_node_whose_name_carries_unspellable_bytes_is_a_row_with_an_explicit_unknown() {
    let temp = tree("unspellable", fixture_nodes(), fixture_nodes());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let world_key = container_key("ZBD/C1/gamez.zbd");

    // The one node per container whose stored name ends in a space. Its trailing
    // space survives outside identity as the display name, unchanged.
    let world_rows: Vec<&CatalogElement> = rows_of(&baseline, ContentKind::SceneNode)
        .into_iter()
        .filter(|element| element.id.key().starts_with(&format!("{world_key}.")))
        .collect();
    assert_eq!(world_rows.len(), FIXTURE_NODES);
    let element = world_rows
        .into_iter()
        .find(|element| element.display_name.as_deref() == Some("brigturret "))
        .expect("the node whose stored name ends in a space is a row of its own");
    assert!(
        element.id.key().contains(GAMEZ_CONTAINER_RECORD),
        "a name the grammar refuses is never transliterated into an identity: {}",
        element.id
    );
    assert_eq!(
        unknown_claims(element),
        vec!["f14.d.4.baseline.node_path_unspellable".to_owned()],
        "the refused spelling is this baseline's own claim"
    );
    let reason = element
        .unsupported_reasons
        .iter()
        .find_map(|reason| match reason {
            UnsupportedReason::Unknown { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
        .expect("the row says what could not be derived");
    assert!(reason.contains("main.wing.brigturret "), "{reason}");

    // The name is never repaired: nothing carries the trimmed spelling as an
    // identity, and no row anywhere holds a key with a space in it.
    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| !element.id.key().contains(' ')),
        "a name is never transliterated into an identity"
    );
    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.id.key() != format!("{world_key}.wing.brigturret")),
        "the trimmed spelling is not an identity either"
    );
    // The node's own parent edge still resolves, so the hierarchy is walkable
    // even though the node cannot be named.
    let parent = element
        .dependencies
        .iter()
        .find(|edge| edge.target.kind() == ContentKind::SceneNode)
        .unwrap_or_else(|| panic!("the node names the parent it stores"));
    assert_eq!(
        parent.target,
        cid(ContentKind::SceneNode, &format!("{world_key}.main.wing")),
        "the parent is the node the record itself names"
    );

    let planes = baseline
        .geometry_containers
        .iter()
        .find(|report| report.spelling == "ZBD/planes.zbd")
        .expect("the shared container is reported");
    assert_eq!(planes.unspellable, 1);
    assert_eq!(planes.ambiguous, 2);
}

/// The two walks meet at exactly one boundary and it is the container's own
/// header word: the mesh section ends where the node array's info array begins,
/// and the node walk ends on the container's last byte.
///
/// This pins the three equalities
/// `cs_content::catalog::baseline::read_geometry_container` cross-checks, and it
/// pins them through the **production readers** rather than through the
/// baseline. It is what makes that cross-check falsifiable: a check that compared
/// the wrong two fields, or stopped comparing one of them, changes what this test
/// measures. The cross-check itself is redundant with the two readers' own
/// boundary checks today — neither reader will hand back a container whose
/// boundary disagrees — so its value is that it fires and names all four numbers
/// if a reader's own check is ever relaxed.
#[test]
fn accept_f14_d_4_the_two_walks_meet_at_the_headers_own_node_offset() {
    let bytes = write_container(&fixture_nodes(), 3, 2);
    let mut parse = cs_formats::ParseContext::with_defaults("fixture.boundary");
    let nodes = cs_formats::gamez::read_gamez_nodes(&mut parse, &bytes).expect("the nodes read");
    let meshes = cs_formats::gamez::read_gamez_meshes(&mut parse, "fixture.boundary", &bytes)
        .expect("the meshes read");

    let header = u64::from(meshes.header.nodes_offset);
    assert_eq!(nodes.header, meshes.header, "both readers read one header");
    assert_eq!(
        meshes.data_end, header,
        "the mesh walk ends exactly where the header declares the node array"
    );
    assert_eq!(
        nodes.info_offset, header,
        "the node info array starts exactly there"
    );
    assert_eq!(
        nodes.data_end,
        bytes.len() as u64,
        "the node walk ends on the container's last byte"
    );
    // The three are the same number, and the whole node array sits between the
    // last two: `212 · node_array_size` bytes of info records.
    assert_eq!(
        nodes.info_end - nodes.info_offset,
        212 * FIXTURE_NODES as u64
    );
    assert_eq!(nodes.data_offset, nodes.info_end);
}

/// A mesh slot the node array names but the array leaves absent has no bytes of
/// its own, so it gets no row and is counted in the collection's own record.
#[test]
fn accept_f14_d_4_a_named_mesh_slot_without_a_present_mesh_is_counted_not_rowed() {
    let temp = tree("absent-slot", fixture_nodes(), fixture_nodes());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let world_key = container_key("ZBD/C1/gamez.zbd");

    // Slot 2 is named by `ghost` and absent from the array: no row.
    assert!(
        baseline
            .catalog
            .get(&cid(ContentKind::Mesh, &format!("{world_key}.2")))
            .is_none(),
        "no bytes of its own means no row"
    );
    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| !element.id.key().ends_with(".2")),
        "the absent slot is nowhere in the catalog"
    );
    let status = status_of(&baseline, ContentKind::Mesh);
    assert_eq!(status.rows, FIXTURE_CONTAINERS * FIXTURE_MESH_ROWS);
    assert_eq!(
        status.gaps.get("named_slot_without_mesh"),
        Some(&FIXTURE_CONTAINERS),
        "one named slot per container has no bytes of its own"
    );
    assert_eq!(status.diagnostic, None);
}

/// A container the producing readers refuse is a **named** diagnostic in
/// [`cs_content::catalog::baseline::CollectionStatus`] and no row at all, and the
/// rest of the inventory — including the container that does read — is still
/// built.
#[test]
fn accept_f14_d_4_a_container_the_readers_refuse_is_a_named_diagnostic_and_no_row() {
    let temp = tree("refused", fixture_nodes(), fixture_nodes());
    // One container of the three stops being a CS GameZ container; the other two
    // are untouched.
    temp.write("ZBD/planes.zbd", b"not a gamez container at all");
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    // Every row comes from a container that reads; the refused one contributes
    // none of its own.
    assert_eq!(
        rows_of(&baseline, ContentKind::SceneNode).len(),
        (FIXTURE_CONTAINERS - 1) * FIXTURE_NODES
    );
    assert_eq!(
        rows_of(&baseline, ContentKind::Mesh).len(),
        (FIXTURE_CONTAINERS - 1) * FIXTURE_MESH_ROWS
    );
    assert!(
        baseline
            .catalog
            .elements()
            .filter(|element| element.kind == ContentKind::SceneNode)
            .all(|element| !element
                .id
                .key()
                .starts_with(&container_key("ZBD/planes.zbd"))),
        "no row names the refused container"
    );
    for kind in [ContentKind::SceneNode, ContentKind::Mesh] {
        let status = status_of(&baseline, kind);
        let diagnostic = status
            .diagnostic
            .as_deref()
            .unwrap_or_else(|| panic!("{} names the container it could not read", kind.label()));
        assert!(diagnostic.contains("ZBD/planes.zbd"), "{diagnostic}");
        assert!(diagnostic.contains("1 of the 3"), "{diagnostic}");
        assert_eq!(status.gaps.get("unreadable_container"), Some(&1));
        assert_eq!(
            status.gaps.get("container_visited"),
            Some(&FIXTURE_CONTAINERS)
        );
        assert_eq!(
            status.gaps.get("container_read"),
            Some(&(FIXTURE_CONTAINERS - 1))
        );
        assert_eq!(status.gaps.get("unreadable_container"), Some(&1));
    }
    // The containers that read keep their rows and their own records.
    let world = baseline
        .geometry_containers
        .iter()
        .find(|report| report.spelling == "ZBD/C1/gamez.zbd")
        .expect("the world container is reported");
    assert_eq!(world.nodes, FIXTURE_NODES);
    let planes = baseline
        .geometry_containers
        .iter()
        .find(|report| report.spelling == "ZBD/planes.zbd")
        .expect("the refused container is still named");
    assert_eq!(planes.nodes, 0, "a refused container contributes no rows");

    // The inventory the F14-D stage built is unchanged.
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
}

/// Neither geometry kind is launchable, so the coverage denominator this task
/// must not move is exactly the one the earlier stages declared.
#[test]
fn accept_f14_d_4_the_geometry_collections_do_not_move_the_coverage_denominator() {
    let temp = tree("denominator", fixture_nodes(), fixture_nodes());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    for kind in [ContentKind::SceneNode, ContentKind::Mesh] {
        assert!(
            !kind.is_launchable(),
            "{} is not launchable content",
            kind.label()
        );
    }
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.coverage.roots, 1);
    assert_eq!(baseline.catalog.launchable_count(), baseline.roots.len());
    assert!(
        baseline
            .roots
            .iter()
            .all(|id| !id.as_str().starts_with("scene_node/") && !id.as_str().starts_with("mesh/")),
        "no geometry row is a closure root"
    );
    assert!(
        baseline
            .catalog
            .unsupported_launchables()
            .iter()
            .all(|element| element.kind == ContentKind::Mission),
        "the only unsupported launchable is the mission that was already there"
    );
    // The rows are unreachable from the declared roots, so they stay counted as
    // unknown content that still needs a classification before a full release.
    assert_eq!(
        baseline
            .coverage
            .unreachable_by_kind
            .get("scene_node")
            .copied(),
        Some(FIXTURE_CONTAINERS * FIXTURE_NODES)
    );
    assert_eq!(
        baseline.coverage.unreachable_by_kind.get("mesh").copied(),
        Some(FIXTURE_CONTAINERS * FIXTURE_MESH_ROWS)
    );
    assert!(baseline.coverage.unreachable_needing_classification >= 27);
    assert_eq!(
        baseline.coverage.reachable, 3,
        "the mission, program and file"
    );
}

/// The deterministic report renders both collections, their own records and the
/// per-container counts, and renders the same bytes for the same installation.
#[test]
fn accept_f14_d_4_the_report_renders_both_collections_and_the_per_container_counts() {
    let temp = tree("report", fixture_nodes(), fixture_nodes());
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let report = baseline_report_json(&baseline);
    let node_rows = FIXTURE_CONTAINERS * FIXTURE_NODES;
    let mesh_rows = FIXTURE_CONTAINERS * FIXTURE_MESH_ROWS;

    // Each collection appears twice in the report: once in the catalog's own
    // `collections` map and once in its `collection_status` record, and the two
    // counts have to agree with each other.
    assert!(
        report.contains(&format!(
            "\"collections\":{{\"install_file\":5,\"mesh\":{mesh_rows},\"mission\":1,\
             \"scene_node\":{node_rows},\"script\":1}}"
        )),
        "{report}"
    );
    for (kind, rows) in [
        ("scene_node", FIXTURE_CONTAINERS * FIXTURE_NODES),
        ("mesh", FIXTURE_CONTAINERS * FIXTURE_MESH_ROWS),
    ] {
        let record = format!(
            "\"kind\":\"{kind}\",\"source\":\"{GEOMETRY_CONTAINER_FILE}\",\
             \"language\":null,\"rows\":{rows},"
        );
        assert!(report.contains(&record), "{kind} record: {report}");
    }
    assert!(
        report.contains("\"geometry_containers\":["),
        "the per-container counts are reported: {report}"
    );
    assert!(
        report.contains(
            "\"container\":\"ZBD/C1/gamez.zbd\",\"nodes\":8,\"named\":5,\"ambiguous\":2,\
             \"unspellable\":1,\"paths\":7,\"roots\":1,\"named_meshes\":3,\"mesh_rows\":2,\
             \"absent_meshes\":1}"
        ),
        "{report}"
    );
    assert!(report.contains("\"launchable\":1"), "{report}");
    assert!(
        !report.contains("\"origin\":\"synthetic_fixture\""),
        "{report}"
    );

    let again = retail_baseline(&temp.0).expect("re-read");
    assert_eq!(
        report,
        baseline_report_json(&again),
        "the report is byte-stable for the same installation"
    );
}

/// The shared planes container and the world group's own container are both read,
/// and neither is reached by guessing at a file name: a world group the
/// installation stores under another file contributes no rows and is named.
#[test]
fn accept_f14_d_4_a_world_group_without_a_geometry_container_is_named_not_guessed() {
    let temp = TempInstall::new("no-geometry");
    write_campaign(&temp);
    // A group directory that holds no `gamez.zbd` at all: production discovery
    // still discovers the group, and the collection has to say so rather than
    // mint a row for a file that is not there.
    temp.write("ZBD/C2/rtexture2.zbd", b"world texture archive bytes");
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(
        baseline
            .catalog
            .elements()
            .all(|element| element.kind != ContentKind::SceneNode
                && element.kind != ContentKind::Mesh),
        "no row is invented from a directory's name"
    );
    for kind in [ContentKind::SceneNode, ContentKind::Mesh] {
        let status = status_of(&baseline, kind);
        assert_eq!(status.rows, 0);
        assert_eq!(status.gaps.get("container_read"), Some(&0));
        assert_eq!(
            status.gaps.get("container_visited").copied(),
            Some(2),
            "the mission's own world group and the group without a container"
        );
        assert_eq!(status.gaps.get("unreadable_container"), Some(&2));
        let diagnostic = status
            .diagnostic
            .as_deref()
            .unwrap_or_else(|| panic!("{} names the absent container", kind.label()));
        assert!(diagnostic.contains("zbd/c2/gamez.zbd"), "{diagnostic}");
    }
    // The shared container is absent from this installation too, so it is not
    // counted as one that was read.
    assert!(
        baseline.geometry_containers.is_empty(),
        "no container was read, and none is claimed"
    );
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
}

// ------------------------------------------------------------- retail ----

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The retail half: the nine GameZ containers the installation holds each yield
/// every stored node and every mesh slot their node arrays name, every row is
/// located by a checked span inside its own container, and the denominator did
/// not move.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_4_retail_every_gamez_container_yields_its_nodes_and_named_meshes() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    let discovery = cs_assets::install::discover(&dir).expect("production discovery reads");
    let install_sha = cs_assets::install::fingerprint(&discovery.manifest).to_hex();

    // The containers are the ones production discovery names: the shared planes
    // container plus every discovered world group's own `gamez.zbd`. The
    // expectation is built from the diagnosis itself rather than from a list, so
    // a group the installation stores under another file is a failure here too.
    let mut expected_containers: BTreeSet<String> = discovery
        .diagnosis
        .world_groups
        .iter()
        .map(|group| format!("{group}/{GEOMETRY_CONTAINER_FILE}"))
        .collect();
    let planes = discovery
        .diagnosis
        .planes_zbd
        .as_ref()
        .expect("the installation ships the shared planes container");
    expected_containers.insert(planes.as_str().to_owned());
    let reported: BTreeSet<String> = baseline
        .geometry_containers
        .iter()
        .map(|report| report.spelling.clone())
        .collect();
    assert_eq!(
        reported, expected_containers,
        "one record per geometry container, named as the installation spells it"
    );

    // Every container read: nothing was refused, so no diagnostic is reported.
    let node_status = status_of(&baseline, ContentKind::SceneNode);
    let mesh_status = status_of(&baseline, ContentKind::Mesh);
    assert_eq!(node_status.source, GEOMETRY_CONTAINER_FILE);
    assert_eq!(mesh_status.source, GEOMETRY_CONTAINER_FILE);
    assert_eq!(node_status.language, None, "a container has no language");
    assert_eq!(node_status.gaps.get("unreadable_container"), Some(&0));
    assert_eq!(mesh_status.gaps.get("unreadable_container"), Some(&0));
    assert_eq!(
        node_status.gaps.get("container_visited").copied(),
        Some(expected_containers.len()),
        "every discovered container was visited"
    );
    assert_eq!(node_status.diagnostic, None, "every container read");
    assert_eq!(mesh_status.diagnostic, None, "every container read");

    // One row per stored node and per present named mesh slot, and the totals
    // are the corpus measurement F11-A and F10 measured (56 620 node records and
    // 17 139 present mesh records across the nine archives).
    let node_rows = rows_of(&baseline, ContentKind::SceneNode).len();
    let mesh_rows = rows_of(&baseline, ContentKind::Mesh).len();
    assert_eq!(node_rows, 56_620, "the measured node-record corpus");
    assert_eq!(mesh_rows, 17_139, "the measured present mesh-record corpus");
    assert_eq!(node_status.rows, node_rows);
    assert_eq!(mesh_status.rows, mesh_rows);
    assert_eq!(
        node_status.gaps.get("container_visited").copied(),
        Some(baseline.geometry_containers.len())
    );

    // Every row is a checked span inside the container it came from, and every
    // container's rows sum to what its own report says. The record extents are
    // re-derived from the **production readers** once per container rather than
    // once per row, so this is a cross-check of the report against the readers
    // and not a restatement of the code under test.
    let dir = game_dir();
    for report in &baseline.geometry_containers {
        let prefix = install_file_key(&report.spelling);
        let nodes: Vec<&CatalogElement> = rows_of(&baseline, ContentKind::SceneNode)
            .into_iter()
            .filter(|element| element.id.key().starts_with(&format!("{prefix}.")))
            .collect();
        let meshes: Vec<&CatalogElement> = rows_of(&baseline, ContentKind::Mesh)
            .into_iter()
            .filter(|element| element.id.key().starts_with(&format!("{prefix}.")))
            .collect();
        assert_eq!(
            nodes.len(),
            report.nodes,
            "{} holds one row per stored node",
            report.spelling
        );
        assert_eq!(
            meshes.len(),
            report.mesh_rows,
            "{} holds one row per present mesh slot its nodes name",
            report.spelling
        );
        assert_eq!(
            report.named + report.ambiguous + report.unspellable,
            report.nodes,
            "{}: every node is named, ambiguous or unspellable",
            report.spelling
        );
        assert_eq!(
            report.mesh_rows + report.absent_meshes,
            report.named_meshes,
            "{}: every named mesh slot either holds a record or is counted",
            report.spelling
        );

        let bytes = std::fs::read(dir.join(&report.spelling))
            .unwrap_or_else(|error| panic!("{} reads: {error}", report.spelling));
        let mut context = cs_formats::ParseContext::with_defaults(report.spelling.clone());
        let decoded = cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
            .expect("the node array reads");
        let mesh_section =
            cs_formats::gamez::read_gamez_meshes(&mut context, &report.spelling, &bytes)
                .expect("the mesh section reads");
        let records: BTreeSet<(u64, u64)> = decoded
            .nodes
            .iter()
            .map(|node| (u64::from(node.data_offset), node.data_bytes))
            .chain(
                mesh_section
                    .present()
                    .map(|mesh| (mesh.data_offset, mesh.data_end - mesh.data_offset)),
            )
            .collect();
        assert_eq!(
            records.len(),
            decoded.nodes.len() + mesh_section.present_count(),
            "{}: no two records share an address inside one container",
            report.spelling
        );

        let digest = discovery
            .manifest
            .files
            .iter()
            .find(|file| file.relative_spelling.as_str() == report.spelling)
            .map(|file| file.sha256.to_hex())
            .unwrap_or_else(|| panic!("{} is inventoried", report.spelling));
        for element in nodes.iter().chain(meshes.iter()) {
            let span = element
                .origin
                .source()
                .unwrap_or_else(|| panic!("{} is located by a span", element.id));
            assert_eq!(span.container_path(), report.spelling);
            assert_eq!(
                span.member_key(),
                None,
                "a GameZ container is a loose file and is its own container"
            );
            assert!(span.offset() > 0, "a record inside the container");
            assert!(span.length() > 0, "a record with bytes of its own");
            assert_eq!(span.install_sha256().to_hex(), install_sha);
            assert!(
                records.contains(&(span.offset(), span.length())),
                "{}: its span is a record of {}, not a range between two records",
                element.id,
                report.spelling
            );
            assert_eq!(
                unknown_claims(element).len(),
                usize::from(element.id.key().contains(GAMEZ_CONTAINER_RECORD)),
                "{}: exactly a row keyed by its record address carries an identity unknown",
                element.id
            );
            assert_eq!(
                element
                    .fingerprint
                    .as_ref()
                    .map(|value| value.sha256.to_hex()),
                Some(digest.clone()),
                "{}: the row fingerprints the bytes it was read from",
                element.id
            );
        }
    }

    // The denominator did not move: the campaign part of it is still the frozen
    // F50 inventory, and no geometry row is a root.
    assert!(
        !ContentKind::SceneNode.is_launchable() && !ContentKind::Mesh.is_launchable(),
        "neither geometry kind is launchable content, so the denominator cannot move"
    );
    let inventory = CampaignInventory::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../missions/bindings/campaign-inventory.tsv"),
    )
    .expect("the frozen F50 campaign inventory reads");
    let campaign_roots = baseline
        .roots
        .iter()
        .filter(|id| id.as_str().starts_with("mission/"))
        .count();
    assert_eq!(
        campaign_roots,
        inventory.len(),
        "the campaign part of the denominator is the frozen F50 inventory"
    );
    assert_eq!(baseline.catalog.launchable_count(), baseline.roots.len());
    assert_eq!(
        baseline.coverage.unresolved_references, 0,
        "every edge of both collections resolves"
    );

    let report = baseline_report_json(&baseline);
    assert!(
        report.contains(&format!("\"scene_node\":{node_rows}")),
        "{report}"
    );
    assert!(
        report.contains(&format!("\"mesh\":{mesh_rows}")),
        "{report}"
    );
    assert!(report.contains("\"geometry_containers\":["), "{report}");
}

/// The [`cs_content::catalog::baseline::CollectionStatus`] of one kind.
fn status_of(
    baseline: &Baseline,
    kind: ContentKind,
) -> &cs_content::catalog::baseline::CollectionStatus {
    baseline
        .collection_status
        .iter()
        .find(|status| status.kind == kind)
        .unwrap_or_else(|| panic!("{} reports its collection status", kind.label()))
}
