//! Acceptance stage F10-C.05: the cross-section check between one GameZ
//! container's node array and its mesh array
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, section
//! `### F10-C`; task F10-C.05).
//!
//! Every fixture here is authored in this file: newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access, except the one
//! `#[ignore]`d retail test at the bottom.
//!
//! The container fixtures reuse the mesh-section writer in [`super::reader`] —
//! the same writer F10-B's tests use, so a fixture here cannot differ from
//! theirs in a way that matters — and append a **node array** after the mesh
//! data, which is where the layout puts it: the header's `nodes_offset` is
//! exactly where the mesh data ends, and both readers are gated by it.

use cs_formats::ParseContext;
use cs_formats::gamez::reader::Fixup;
use cs_formats::gamez::{
    GameZMeshes, GameZNodeError, GameZNodes, MeshSlotIssue, NODE_INDEX_TOP, NODE_SLOT_BYTES,
    NodeMeshBindings, NodeMeshFinding, OBJECT3D_DATA_BYTES, OBJECT3D_FLAGS_IDENTITY, RemovedBinding,
    UNK08_PLANES, read_gamez_meshes, read_gamez_nodes,
};

use super::reader::{
    HeaderSpec, MeshSpec, authored_container, c4_remap, planes_remap, sequential_index, stub_words,
};

use std::path::Path;

// --------------------------------------------------------------- fixtures ---

/// One node to write: a name, the `mesh_index` it stores and the child slots
/// its record declares.
///
/// The record is an **object** node storing the identity transform, which is
/// what the measured corpus holds for a geometry node, so the only things a
/// fixture varies are the association under test and the subtree under it.
struct NodeSpec {
    name: &'static str,
    mesh_index: i32,
    children: &'static [u32],
}

impl NodeSpec {
    const fn named(name: &'static str, mesh_index: i32) -> Self {
        Self {
            name,
            mesh_index,
            children: &[],
        }
    }

    const fn with_children(name: &'static str, mesh_index: i32, children: &'static [u32]) -> Self {
        Self {
            name,
            mesh_index,
            children,
        }
    }

    /// Bytes of this node's own data record: the object record, no parent
    /// word (`parent_count` is 0) and the child slots `children_count`
    /// declares.
    const fn data_bytes(&self) -> usize {
        OBJECT3D_DATA_BYTES as usize + 4 * self.children.len()
    }
}

/// The bytes of one container's node section, which is `node_array_size` times a
/// 212-byte info slot followed by the data section. It excludes the mesh bytes
/// that precede it, so it can be appended to a container laid out by
/// [`authored_container`].
///
/// Offsets inside the section are absolute, because that is where the reader
/// reads them from; the leading `nodes_offset` bytes of scratch are dropped on
/// the way out.
///
/// Field offsets are the ones the reader reads them at, so nothing here depends
/// on a hand-computed layout: a name up to its `NUL` terminator, `flags` at slot
/// offset 36, the kind tag at 52, `data_ptr` at 56, `mesh_index` at 60, the two
/// counts at 84 and 86, `unk196` at 196, and the trailing `node_index` word at
/// 208.
fn node_section(nodes_offset: u32, nodes: &[NodeSpec]) -> Vec<u8> {
    assert!(!nodes.is_empty(), "an empty node array is refused outright");
    let info_bytes = NODE_SLOT_BYTES as usize * nodes.len();
    let mut data_offset = nodes_offset as usize + info_bytes;
    let mut data_offsets = Vec::with_capacity(nodes.len());
    for node in nodes {
        data_offsets.push(data_offset as u32);
        data_offset += node.data_bytes();
    }

    let mut out = vec![0u8; data_offset];
    let word = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let half = |bytes: &mut Vec<u8>, at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, at: usize, value: f32| {
        bytes[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
    };

    for (index, node) in nodes.iter().enumerate() {
        let at = nodes_offset as usize + NODE_SLOT_BYTES as usize * index;
        let name = node.name.as_bytes();
        assert!(
            name.len() < 36,
            "the fixture's names fit their 36-byte field"
        );
        out[at..at + name.len()].copy_from_slice(name);
        word(&mut out, at + 44, 1); // unk044
        word(&mut out, at + 52, 5); // node_type: Object3d
        word(&mut out, at + 56, data_offsets[index]);
        word(&mut out, at + 60, node.mesh_index as u32);
        word(&mut out, at + 68, 1); // action_priority
        half(&mut out, at + 84, 0); // parent_count
        half(&mut out, at + 86, node.children.len() as u16); // children_count
        word(&mut out, at + 196, 160); // unk196
        word(&mut out, at + 208, NODE_INDEX_TOP | index as u32);

        // The object record: the identity transform the reference asserts for
        // `flags == 40`, with the rotation and translation zero and the matrix
        // the identity those three derive. The child slots follow it, which is
        // where the reader's own walk reads them.
        let record = data_offsets[index] as usize;
        word(&mut out, record, OBJECT3D_FLAGS_IDENTITY);
        for axis in 0..3 {
            float(&mut out, record + 24 + 4 * axis, 0.0); // rotation
            float(&mut out, record + 36 + 4 * axis, 1.0); // scale
            float(&mut out, record + 48 + 16 * axis, 1.0); // matrix diagonal
            float(&mut out, record + 84 + 4 * axis, 0.0); // translation
        }
        for (position, child) in node.children.iter().enumerate() {
            word(
                &mut out,
                record + OBJECT3D_DATA_BYTES as usize + 4 * position,
                *child,
            );
        }
    }
    out[nodes_offset as usize..].to_vec()
}

/// One whole container: a mesh array of `meshes`, and a node array of `nodes`
/// laid out after the mesh data.
///
/// `present` names the array slots that stored a mesh; every other slot is the
/// all-zero stub the layout stores. `unk08` selects the mesh-index fixup, so a
/// fixture can be one of the two **remapped** archives whose stub words follow a
/// different order than their positions.
fn container(unk08: u32, meshes: &[MeshSpec], present: &[u32], nodes: &[NodeSpec]) -> Vec<u8> {
    let array_size = meshes.len() as u32;
    let spec = HeaderSpec {
        unk08,
        node_array_size: u32::try_from(nodes.len()).expect("the fixture array fits"),
        ..HeaderSpec::default()
    };
    let remap: fn(i32) -> i32 = match Fixup::for_unk08(unk08) {
        Fixup::Planes => planes_remap,
        Fixup::C4 => c4_remap,
        Fixup::None => |expected| expected,
    };
    let mut bytes = authored_container(
        &spec,
        meshes,
        sequential_index(array_size, present),
        &stub_words(array_size, present, remap),
    );
    // `authored_container` ends exactly at `nodes_offset`, which is where the
    // node array starts: the two sections are back to back, and both readers
    // are gated by that one word.
    let nodes_offset = u64::from(bytes[36..40].read_le_u32());
    assert_eq!(
        nodes_offset,
        bytes.len() as u64,
        "mesh data ends at nodes_offset"
    );
    bytes.extend_from_slice(&node_section(u32::try_from(nodes_offset).expect(""), nodes));
    bytes
}

/// Reads one synthetic container through both production readers.
fn read_container(bytes: &[u8]) -> (GameZNodes, GameZMeshes) {
    let label = "synthetic/gamez.zbd";
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(label), bytes)
        .unwrap_or_else(|error| panic!("{label}: the node array must read, got {error}"));
    let meshes = read_gamez_meshes(&mut ParseContext::with_defaults(label), label, bytes)
        .unwrap_or_else(|error| panic!("{label}: the mesh section must read, got {error}"));
    (nodes, meshes)
}

/// `u32` from four little-endian bytes, so a fixture reads the header back
/// rather than remembering an offset into a buffer another fixture laid out.
trait ReadLe {
    fn read_le_u32(&self) -> u32;
}

impl ReadLe for [u8] {
    fn read_le_u32(&self) -> u32 {
        u32::from_le_bytes(self[0..4].try_into().expect("four bytes"))
    }
}

// ------------------------------------------------------- synthetic tests ---

/// An index past the mesh array is reported with **the node that stores it** and
/// **the slot it names**, and the array size it is past.
///
/// The node reader is deliberately **not** the place this happens: it carries
/// every index raw and raises no finding about any of them, which is what the
/// test asserts alongside the verdict.
#[test]
fn accept_f10_c_05_a_node_naming_a_slot_outside_the_mesh_array_is_named_with_its_node_and_slot() {
    let meshes = vec![
        MeshSpec::triangle(),
        MeshSpec::triangle(),
        MeshSpec::triangle(),
        MeshSpec::triangle(),
    ];
    let nodes = [
        NodeSpec::named("present", 0),
        NodeSpec::named("unbound", -1),
        NodeSpec::named("past_the_array", 7),
    ];
    let (nodes, section) =
        read_container(&container(1_234_567_890, &meshes, &[0, 1, 2, 3], &nodes));
    assert_eq!(section.slot_count(), 4, "four array slots");
    assert_eq!(section.present_count(), 4);

    // The node reader carries the index raw and checks nothing about it.
    assert!(
        nodes.findings.is_empty(),
        "no node-level finding: {:#?}",
        nodes.findings
    );
    assert_eq!(
        nodes.mesh_index_bounds().bound,
        2,
        "two nodes store an index"
    );
    assert_eq!(nodes.get(2).expect("node 2").mesh_index(), 7, "carried raw");

    let bindings = section.node_bindings(&nodes);
    assert_eq!(bindings, NodeMeshBindings::of(&nodes, &section));
    assert_eq!(bindings.nodes, 3);
    assert_eq!(bindings.bound, 2);
    assert_eq!(bindings.resolved, 1);
    assert_eq!(bindings.unnamed, 1, "the `-1` sentinel names no slot");
    assert_eq!(bindings.slots, 4);
    assert_eq!(bindings.present, 4);
    assert_eq!(bindings.unresolved(), 1);
    assert!(!bindings.is_complete());

    let finding = bindings.finding(2).expect("node 2 is reported");
    assert_eq!(
        *finding,
        NodeMeshFinding {
            node: 2,
            mesh_index: 7,
            issue: MeshSlotIssue::OutOfRange { slot: 7, slots: 4 },
        }
    );
    assert_eq!(finding.code(), "mesh_slot_out_of_range");
    assert_eq!(finding.slot(), 7, "the slot the node named");
    assert_eq!(
        bindings.findings.len(),
        1,
        "the `-1` node and the present one are not findings"
    );

    // The message names the node, the stored index, the slot and the array size,
    // because a caller gets this line and nothing else.
    let line = finding.to_string();
    for expected in ["mesh_slot_out_of_range", "node 2", "mesh_index 7", "4-slot"] {
        assert!(
            line.contains(expected),
            "{line:?} does not name {expected:?}"
        );
    }
}

/// A slot **inside** the array that holds an all-zero stub is a different fact
/// from an index past the array, and the two are reported as two codes.
///
/// This is the distinction the check exists for: an absent mesh is a position
/// the container really has and stores nothing in, while an out-of-range index
/// names a position it never had. Resolving both to one "unknown mesh" would
/// lose that.
#[test]
fn accept_f10_c_05_a_node_naming_an_all_zero_stub_is_absent_not_out_of_range() {
    let meshes = vec![
        MeshSpec::stub(),
        MeshSpec::triangle(),
        MeshSpec::stub(),
        MeshSpec::triangle(),
    ];
    let nodes = [
        NodeSpec::named("slot_zero", 0),
        NodeSpec::named("slot_one", 1),
        NodeSpec::named("slot_two", 2),
        NodeSpec::named("slot_three", 3),
    ];
    let (nodes, section) = read_container(&container(1_234_567_890, &meshes, &[1, 3], &nodes));
    assert_eq!(section.slot_count(), 4);
    assert_eq!(
        section.present_count(),
        2,
        "two of the four slots stored a mesh"
    );
    assert!(section.get(0).is_none(), "slot 0 is the all-zero stub");
    assert!(section.get(2).is_none(), "slot 2 is the all-zero stub");

    // The node reader sees four stored indices and objects to none of them.
    assert!(
        nodes.findings.is_empty(),
        "no node-level finding: {:#?}",
        nodes.findings
    );
    assert_eq!(nodes.mesh_index_bounds().bound, 4);

    let bindings = NodeMeshBindings::of(&nodes, &section);
    assert_eq!(bindings.bound, 4);
    assert_eq!(bindings.resolved, 2);
    assert_eq!(bindings.unresolved(), 2);
    assert!(!bindings.is_complete());
    assert_eq!(
        bindings.findings,
        vec![
            NodeMeshFinding {
                node: 0,
                mesh_index: 0,
                issue: MeshSlotIssue::Absent { slot: 0 },
            },
            NodeMeshFinding {
                node: 2,
                mesh_index: 2,
                issue: MeshSlotIssue::Absent { slot: 2 },
            },
        ]
    );
    for finding in &bindings.findings {
        assert_eq!(finding.code(), "mesh_slot_absent");
        assert_ne!(
            finding.code(),
            "mesh_slot_out_of_range",
            "an absent slot is not an out-of-range index"
        );
        assert!(finding.to_string().contains("all-zero stub"), "{}", finding);
    }
    assert!(bindings.finding(0).is_some());
    assert!(bindings.finding(2).is_some());
    assert!(
        bindings.finding(1).is_none(),
        "a present slot is not a finding"
    );
    assert!(bindings.finding(3).is_none());
}

/// The slot is looked up **where the node stored it**, and not in a compact list
/// of the meshes that happen to be present.
///
/// The mesh index is non-sequential: an absent record stores the index the next
/// present one is expected to carry, under a remap table in `planes.zbd` and C4
/// `gamez.zbd`. So the slot a node names is not its position in a compact
/// enumeration. In this container exactly one of five slots stores a mesh and it
/// is slot **3**: a compact enumeration would answer for slot 0 and report
/// every other index out of range, which is exactly backwards.
#[test]
fn accept_f10_c_05_the_slot_is_looked_up_where_it_is_stored_not_in_a_compact_list() {
    let meshes = vec![
        MeshSpec::stub(),
        MeshSpec::stub(),
        MeshSpec::stub(),
        MeshSpec::triangle(),
        MeshSpec::stub(),
    ];
    let nodes = [
        NodeSpec::named("the_present_slot", 3),
        NodeSpec::named("first_stub", 0),
        NodeSpec::named("middle_stub", 2),
        NodeSpec::named("last_stub", 4),
    ];
    // `UNK08_PLANES` selects the measured remap table, so this container is the
    // shape a compact enumeration gets wrong.
    let (nodes, section) = read_container(&container(UNK08_PLANES, &meshes, &[3], &nodes));
    assert_eq!(
        section.fixup,
        Fixup::Planes,
        "the fixture is a remapped archive"
    );
    assert_eq!(section.slot_count(), 5);
    assert_eq!(section.present_count(), 1);
    assert!(section.get(3).is_some(), "slot 3 is the present one");

    let bindings = NodeMeshBindings::of(&nodes, &section);
    assert_eq!(bindings.resolved, 1, "the node that stored 3 resolves");
    assert_eq!(bindings.unresolved(), 3);
    assert_eq!(
        bindings.findings,
        vec![
            NodeMeshFinding {
                node: 1,
                mesh_index: 0,
                issue: MeshSlotIssue::Absent { slot: 0 },
            },
            NodeMeshFinding {
                node: 2,
                mesh_index: 2,
                issue: MeshSlotIssue::Absent { slot: 2 },
            },
            NodeMeshFinding {
                node: 3,
                mesh_index: 4,
                issue: MeshSlotIssue::Absent { slot: 4 },
            },
        ],
        "every stub slot is reported as absent, and none as out of range: \
         a compact enumeration would have resolved slot 0 and refused 3"
    );
    assert!(
        bindings.finding(0).is_none(),
        "the node that stored 3 is fine"
    );
}

/// Every stored index in one container that is inside its bounds resolves, and
/// the counts the check reports are the ones the two readers measured
/// independently — the node reader's own `mesh_index_bounds` and the mesh
/// section's own `present_count`.
#[test]
fn accept_f10_c_05_a_container_whose_every_index_resolves_is_complete() {
    let meshes = vec![
        MeshSpec::triangle(),
        MeshSpec::triangle(),
        MeshSpec::triangle(),
    ];
    let nodes = [
        NodeSpec::named("a", 2),
        NodeSpec::named("b", 0),
        NodeSpec::named("c", -1),
        NodeSpec::named("d", 1),
    ];
    let (nodes, section) = read_container(&container(1_234_567_890, &meshes, &[0, 1, 2], &nodes));
    assert!(nodes.findings.is_empty(), "{:#?}", nodes.findings);
    let bounds = nodes.mesh_index_bounds();
    let bindings = section.node_bindings(&nodes);
    assert_eq!(
        bindings.bound as usize, bounds.bound,
        "the two readers agree"
    );
    assert_eq!(bounds.min, Some(0));
    assert_eq!(bounds.max, Some(2));
    assert_eq!(bindings.slots as usize, section.meshes.len());
    assert_eq!(bindings.present as usize, section.present_count());
    assert!(bindings.is_complete(), "{:?}", bindings.findings);
    assert_eq!(bindings.unresolved(), 0);
    assert_eq!(bindings.unnamed, 1);
    let summary = bindings.to_string();
    assert!(
        summary.contains("3 of 4 nodes") && summary.contains("3 slots (3 present)"),
        "{summary}"
    );
}

/// A node whose stored index is below the `-1` sentinel is a node reader's own
/// finding, and this check does not double-count it: a negative index names no
/// position in the array, so there is nothing to range-check.
#[test]
fn accept_f10_c_05_a_negative_index_names_no_slot_and_is_not_a_finding_here() {
    let meshes = vec![MeshSpec::triangle(), MeshSpec::triangle()];
    let nodes = [
        NodeSpec::named("sentinel", -1),
        NodeSpec::named("below_sentinel", -5),
        NodeSpec::named("present", 1),
    ];
    let (nodes, section) = read_container(&container(1_234_567_890, &meshes, &[0, 1], &nodes));
    let codes: Vec<String> = nodes.findings.iter().map(|f| f.to_string()).collect();
    assert_eq!(
        codes.len(),
        1,
        "the node reader reports the bad sentinel: {codes:?}"
    );
    assert!(
        codes[0].contains("mesh_index stores -5"),
        "the node reader's own finding: {}",
        codes[0]
    );

    let bindings = section.node_bindings(&nodes);
    assert_eq!(bindings.unnamed, 2, "both negative indices name no slot");
    assert_eq!(bindings.bound, 1);
    assert_eq!(bindings.resolved, 1);
    assert!(bindings.is_complete(), "{:?}", bindings.findings);
}

/// A container whose **node** data section does not end at the container's end
/// is refused by the node reader, so the check is not reachable for it at all:
/// the verdict is over two sections that both have to be read, and it is exactly
/// as unavailable as its missing half.
///
/// The mesh section of the very same bytes still reads, because `nodes_offset`
/// and the mesh data are untouched — which is what shows the two readers are
/// independent and that nothing here smuggles the check into either one.
#[test]
fn accept_f10_c_05_a_broken_node_array_is_refused_before_the_check_runs() {
    let meshes = vec![MeshSpec::triangle(), MeshSpec::triangle()];
    let nodes = [NodeSpec::named("a", 0), NodeSpec::named("b", 1)];
    let mut bytes = container(1_234_567_890, &meshes, &[0, 1], &nodes);
    let read = |bytes: &[u8]| read_container(bytes);
    let (nodes, section) = read(&bytes);
    assert_eq!(
        nodes.data_end,
        bytes.len() as u64,
        "the walk ends on the last byte"
    );
    assert_eq!(section.present_count(), 2);

    // Eight bytes the data walk does not account for: the node array claims the
    // container ends later than its own records do.
    bytes.extend_from_slice(&[0u8; 8]);
    let error = read_gamez_nodes(&mut ParseContext::with_defaults("synthetic"), &bytes)
        .expect_err("trailing bytes the walk does not account for are refused");
    assert_eq!(error.code(), "data_end", "{error}");
    assert!(
        matches!(error, GameZNodeError::DataEnd { found, expected } if found < expected),
        "{error:?}"
    );

    // The mesh section of the same bytes still reads, so the check is missing
    // only its node half.
    let section = read_gamez_meshes(
        &mut ParseContext::with_defaults("synthetic"),
        "synthetic",
        &bytes,
    )
    .expect("the mesh section is unaffected");
    assert_eq!(
        section.present_count(),
        2,
        "the mesh section reads on its own"
    );
}

/// **F10-D.1's node half:** the original destroys every `dzpath`-named node —
/// with its whole subtree — during world load (`OWNER-STATIC-2026-10-05`,
/// `dzpath.cpp` `0x4459f0` collecting, `0x4cd800` detaching, `0x4d7a50`
/// destroying), so a mesh only those nodes bind is never drawn, while the
/// faces of that mesh still **decode** (their not being drawn is a
/// node-removal fact, never a face refusal).
///
/// The fixture exercises the three parts of the measured rule on one
/// container: the plain prefix match (the `dzpaths` group node itself starts
/// with `dzpath` and matches), the subtree propagation (`prop`'s own name
/// does not match; it is removed because it hangs under `dzpath1`), and the
/// live-binding exception (a slot a surviving node also binds still draws
/// through that node, so it is never listed).
#[test]
fn accept_f10_d_a_dzpath_subtree_is_removed_at_load_and_its_mesh_is_never_drawn() {
    let meshes = vec![
        MeshSpec::triangle(),
        MeshSpec::triangle(),
        MeshSpec::triangle(),
    ];
    let nodes = [
        NodeSpec::named("world1", -1),
        NodeSpec::with_children("dzpaths", -1, &[2, 4]),
        NodeSpec::with_children("dzpath1", 0, &[5]),
        NodeSpec::named("hangar", 2),
        NodeSpec::named("dzpath2", 1),
        NodeSpec::named("prop", 0),
        NodeSpec::named("tower", 0),
    ];
    let (nodes, section) = read_container(&container(1_234_567_890, &meshes, &[0, 1, 2], &nodes));

    let removed = nodes.removed_at_load();
    assert_eq!(
        removed,
        std::collections::BTreeSet::from([1, 2, 4, 5]),
        "the `dzpaths` group, the two `dzpath` zones and `dzpath1`'s subtree — \
         never `world1`, `hangar` or `tower`"
    );

    let bindings = section.node_bindings(&nodes);
    assert_eq!(bindings.nodes, 7);
    assert_eq!(bindings.bound, 5, "every node but `world1` and `dzpaths`");
    assert_eq!(
        bindings.removed,
        vec![
            RemovedBinding {
                node: 2,
                mesh_index: 0
            },
            RemovedBinding {
                node: 4,
                mesh_index: 1
            },
            RemovedBinding {
                node: 5,
                mesh_index: 0
            },
        ],
        "the bound removed nodes, in stored node order — `dzpaths` binds no mesh"
    );
    assert_eq!(
        bindings.never_drawn,
        vec![1],
        "slot 1 is bound only by `dzpath2`; slot 0 is also bound by the \
         surviving `tower`, so it still draws through that node"
    );

    // The never-drawn fact is about the nodes, not the faces: the meshes the
    // removed nodes bind decode cleanly, and the census reports no missing
    // face for them.
    let census = section.face_census();
    assert_eq!(census.stored_faces, 3);
    assert_eq!(census.missing_faces(), 0, "{:?}", census.missing);
}

// ---------------------------------------------------------- retail corpus ---

/// The two archives whose mesh index is **remapped**, read through the
/// production readers: `ZBD/planes.zbd` (the `Fixup::Planes` table) and
/// `ZBD/C4/gamez.zbd` (the `Fixup::C4` table).
///
/// The measured verdict is pinned here rather than derived, so a reader that
/// resolved an index against a compact enumeration instead of the stored slot
/// fails on real bytes rather than only on a fixture. The reference asserts
/// that a non-negative `mesh_index` is inside the mesh array **and** holds a
/// present mesh; this test states, for both archives, the two readers'
/// independent counts and the verdict that follows from them.
///
/// `#[ignore]`d because CI has no `CS_GAME_DIR`; the implementer and the
/// reviewer run it with `--include-ignored`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f10_c_05_retail_both_remapped_archives_resolve_every_node_mesh_index() {
    /// What the two production readers measured over the original installation.
    ///
    /// `records` is the node array's record count, `bound` how many of those
    /// store a non-negative `mesh_index` (the node reader's own figure), `slots`
    /// the mesh array's size, `present` how many of those slots stored a mesh
    /// (the mesh reader's own figure), and `min`/`max` the node reader's own
    /// index bounds.
    const PLANES: Corpus = Corpus {
        spelling: "ZBD/planes.zbd",
        fixup: Fixup::Planes,
        records: 3_317,
        bound: 1_766,
        slots: 2_250,
        present: 1_766,
        min: 0,
        max: 1_778,
    };
    const C4: Corpus = Corpus {
        spelling: "ZBD/C4/gamez.zbd",
        fixup: Fixup::C4,
        records: 8_289,
        bound: 4_929,
        slots: 3_000,
        present: 2_431,
        min: 0,
        max: 2_489,
    };

    for corpus in [PLANES, C4] {
        corpus.check();
    }
}

/// One measured archive of the retail test.
struct Corpus {
    /// Installation-relative spelling of the container.
    spelling: &'static str,
    /// The fixup its `unk08` word must select.
    fixup: Fixup,
    /// Node array records.
    records: u32,
    /// Nodes storing a non-negative `mesh_index`.
    bound: u32,
    /// Mesh array slots, absent stubs included.
    slots: usize,
    /// Slots that stored a mesh.
    present: usize,
    /// Lowest non-negative `mesh_index` stored.
    min: u32,
    /// Highest non-negative `mesh_index` stored.
    max: u32,
}

impl Corpus {
    /// Reads this archive with both production readers and checks the verdict.
    fn check(&self) {
        let path = Path::new(&game_dir()).join(self.spelling);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{} must read: {error}", path.display()));
        let label = self.spelling.to_owned();

        let parsed = read_gamez_nodes(&mut ParseContext::with_defaults(label.clone()), &bytes)
            .unwrap_or_else(|error| panic!("{label}: the node array must read, got {error}"));
        let section = read_gamez_meshes(
            &mut ParseContext::with_defaults(label.clone()),
            &label,
            &bytes,
        )
        .unwrap_or_else(|error| panic!("{label}: the mesh section must read, got {error}"));

        // Both archives really are the remapped ones the check has to get right:
        // a compact enumeration of the present meshes would answer for a
        // different slot than the nodes stored, for exactly these two.
        assert_eq!(
            section.fixup, self.fixup,
            "{label}: the remap table its `unk08` selects"
        );
        assert_eq!(
            parsed.nodes.len() as u32,
            self.records,
            "{label}: node records"
        );
        let bounds = parsed.mesh_index_bounds();
        assert_eq!(
            bounds.bound, self.bound as usize,
            "{label}: nodes storing an index"
        );
        assert_eq!(
            bounds.min,
            Some(self.min),
            "{label}: the node reader's low bound"
        );
        assert_eq!(
            bounds.max,
            Some(self.max),
            "{label}: the node reader's high bound"
        );
        assert_eq!(
            section.slot_count(),
            self.slots,
            "{label}: mesh array slots"
        );
        assert_eq!(
            section.present_count(),
            self.present,
            "{label}: present meshes"
        );

        let bindings = section.node_bindings(&parsed);
        assert_eq!(
            bindings.nodes, self.records,
            "{label}: every node reaches the check"
        );
        assert_eq!(
            bindings.bound, self.bound,
            "{label}: the check and the node reader agree on `bound`"
        );
        assert_eq!(bindings.slots as usize, self.slots);
        assert_eq!(bindings.present as usize, self.present);
        assert_eq!(
            bindings.resolved, self.bound,
            "{label}: every stored index names a present mesh"
        );
        assert_eq!(bindings.unresolved(), 0, "{label}");
        assert!(bindings.is_complete(), "{label}: {:?}", bindings.findings);
    }
}

/// **F10-D.1 on the real corpus:** every world container's `dzpath` route
/// nodes are the nodes the original destroys at world load
/// (`OWNER-STATIC-2026-10-05`), and the mesh slots no surviving node binds
/// are exactly the route meshes of those zones.
///
/// The pinned rows are what the production readers measure on this
/// installation: `removed` is the node count `removed_at_load` reports (the
/// `dzpaths` group plus the numbered zones under it), `bound` how many of
/// those bind a mesh, and `never_drawn` the exact slot list
/// [`NodeMeshBindings::never_drawn`] produces — the stored route geometry no
/// draw call can reach. A container that gains or loses a zone, or a slot
/// that becomes reachable another way, changes the row on purpose.
///
/// `#[ignore]`d because CI has no `CS_GAME_DIR`; the implementer and the
/// reviewer run it with `--include-ignored`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f10_d_retail_dzpath_route_nodes_are_removed_at_load_and_never_drawn() {
    /// `(spelling, removed nodes, bound removed nodes, never-drawn slots)`.
    const EXPECTED: [(&str, usize, usize, &[u32]); 9] = [
        ("ZBD/C1/gamez.zbd", 7, 6, &[998, 999, 1000, 1001, 1002, 1003]),
        ("ZBD/C1B/gamez.zbd", 8, 7, &[432, 433, 434, 435, 436, 437, 438]),
        ("ZBD/C1C/gamez.zbd", 0, 0, &[]),
        (
            "ZBD/C2/gamez.zbd",
            14,
            13,
            &[593, 594, 595, 596, 597, 598, 599, 600, 601, 602, 603, 604, 605],
        ),
        ("ZBD/C2B/gamez.zbd", 0, 0, &[]),
        ("ZBD/C3/gamez.zbd", 6, 5, &[802, 803, 804, 805, 806]),
        (
            "ZBD/C4/gamez.zbd",
            16,
            15,
            &[
                863, 864, 865, 866, 867, 868, 869, 870, 871, 872, 873, 874, 875, 876, 877,
            ],
        ),
        (
            "ZBD/C5/gamez.zbd",
            35,
            34,
            &[
                949, 950, 951, 952, 953, 954, 955, 956, 957, 958, 959, 960, 961, 962, 963,
                964, 965, 966, 967, 968, 969, 970, 971, 972, 973, 974, 975, 976, 977, 978,
                979, 980, 981, 982,
            ],
        ),
        ("ZBD/planes.zbd", 0, 0, &[]),
    ];

    for (spelling, removed_nodes, bound_removed, never_drawn) in EXPECTED {
        let path = Path::new(&game_dir()).join(spelling);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{spelling} must read: {error}"));
        let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(spelling), &bytes)
            .unwrap_or_else(|error| panic!("{spelling}: the node array must read, got {error}"));
        let meshes = read_gamez_meshes(
            &mut ParseContext::with_defaults(spelling),
            spelling,
            &bytes,
        )
        .unwrap_or_else(|error| panic!("{spelling}: the mesh section must read, got {error}"));

        // The removal is the prefix plus the subtree, and every bound node it
        // takes is a `dzpath` name: nothing else of the container is removed.
        let removed = nodes.removed_at_load();
        assert_eq!(
            removed.len(),
            removed_nodes,
            "{spelling}: nodes the original destroys at load"
        );
        for index in &removed {
            let node = nodes.get(*index).expect("the slot exists");
            assert!(
                node.name.starts_with("dzpath")
                    || removed.contains(&node.parent.unwrap_or(u32::MAX)),
                "{spelling}: node {} ({:?}) is removed by the prefix or its ancestor",
                node.index,
                node.name
            );
        }

        let bindings = meshes.node_bindings(&nodes);
        assert_eq!(
            bindings.removed.len(),
            bound_removed,
            "{spelling}: bound nodes the removal takes"
        );
        for binding in &bindings.removed {
            let node = nodes.get(binding.node).expect("the slot exists");
            assert!(
                node.name.starts_with("dzpath"),
                "{spelling}: node {} ({:?}) binds {} and is removed",
                binding.node,
                node.name,
                binding.mesh_index
            );
            assert_eq!(
                binding.mesh_index,
                node.mesh_index(),
                "{spelling}: the binding carries the stored index raw"
            );
        }
        assert_eq!(
            bindings.never_drawn, *never_drawn,
            "{spelling}: the mesh slots no surviving node binds"
        );

        // Each never-drawn slot is a present mesh whose faces still decode —
        // the route outline is stored geometry the census counts as decoded,
        // not a face the format layer lost. And no missing face of the
        // container lives on one of these meshes: the faces that draw nothing
        // (C4's three zero-normal outlines) hang under surviving nodes.
        for slot in never_drawn {
            let mesh = meshes
                .get(*slot)
                .unwrap_or_else(|| panic!("{spelling}: slot {slot} is a present mesh"));
            assert!(
                !mesh.draw_topology().faces.is_empty(),
                "{spelling}: mesh {slot} stores faces"
            );
            assert!(
                !mesh
                    .draw_topology()
                    .faces
                    .iter()
                    .any(|face| matches!(face, cs_formats::gamez::FaceStatus::Rejected(_))),
                "{spelling}: mesh {slot}'s faces all decode under the measured rule"
            );
        }
        let census = meshes.face_census();
        assert!(
            census
                .missing
                .iter()
                .all(|face| !never_drawn.contains(&face.mesh)),
            "{spelling}: no missing face sits on a never-drawn mesh"
        );
    }
}

/// The read-only original installation, or a refusal that says so.
fn game_dir() -> String {
    let dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the installation");
    assert!(
        Path::new(&dir).join("ZBD").is_dir(),
        "CS_GAME_DIR holds no ZBD directory: {dir}"
    );
    dir
}
