//! Acceptance scenario F14-D.9: the `material` collection of the retail baseline
//! inventory (`specs/F14-canonical-content-catalog-and-dependency-closure.md`,
//! stage `### F14-D`; follow-up task #558).
//!
//! `docs/contracts/IDENTITY-CONTENT.md` requires a catalog collection for
//! "render meshes/materials/images". F14-D.4 populated `scene_node` and `mesh`
//! but not `material`, because `cs_formats::gamez::RawMaterial` carried no byte
//! offset. The reader now reports each record's own extent, and
//! `cs_content::catalog::baseline::retail_baseline` holds one `material` row per
//! present material record, located by that extent.
//!
//! The non-retail tests write a **synthetic installation tree** whose
//! `ZBD/planes.zbd`, `ZBD/C1/gamez.zbd` and `ZBD/C1C/gamez.zbd` are authored CS
//! GameZ containers (header, texture-name table, 1000-slot material section,
//! mesh section carrying material references, node array) and run the production
//! baseline builder over it. Removing the collection fails every one of them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! installation and pins what it really holds; run it with `--include-ignored`.
//! Without `CS_GAME_DIR` it fails loudly rather than passing vacuously.
//!
//! Every byte, name and count in the fixtures is authored for this file; no
//! original content is written to Git.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_content::catalog::baseline::{
    Baseline, GEOMETRY_CONTAINER_FILE, GEOMETRY_CONTAINER_PATTERN, baseline_report_json,
    install_file_key, retail_baseline,
};
use cs_types::content::{
    CatalogElement, ContentId, ContentKind, NormalizeState, Origin, Readiness, UnsupportedReason,
};
use cs_types::install::ParseState;

/// The GameZ signature a Crimson Skies container stores.
const SIGNATURE: u32 = 0x0297_1222;
/// The Crimson Skies container version.
const VERSION: u32 = 42;
/// `textures_offset` must equal the header size; the reader asserts it.
const TEXTURES_OFFSET: u32 = 40;
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

/// Present material records across the nine retail containers, measured by
/// `docs/findings/2026-10-03-f14-d-9-material-collection.md`.
const RETAIL_MATERIALS: usize = 4669;

/// Texture-name slots a fixture container stores.
const FIXTURE_TEXTURES: u32 = 2;
/// Material slots in the array, present or not.
const MATERIAL_SLOTS: u32 = 1000;
/// Bytes of one material slot: the 40-byte record plus two link words.
const MATERIAL_SLOT: u32 = 44;
/// Bytes of the material section header.
const MATERIAL_HEADER: u32 = 16;
/// The flag bits the reference names: textured, and the always-set bit.
const FLAG_TEXTURED: u8 = 1;
const FLAG_ALWAYS: u8 = 1 << 4;
/// The free-slot flag every zero slot stores.
const FLAG_FREE: u8 = 1 << 5;

/// One present material the fixture stores.
#[derive(Clone, Copy)]
struct FixtureMaterial {
    textured: bool,
    texture_index: u32,
}

/// The four present materials every fixture container stores:
///
/// * 0 — textured, names texture 0 (inside the two-name table)
/// * 1 — flat colour, referenced by no mesh
/// * 2 — textured, names texture 7, which the table does not have
/// * 3 — flat colour, referenced by no mesh
const FIXTURE_MATERIALS: [FixtureMaterial; 4] = [
    FixtureMaterial {
        textured: true,
        texture_index: 0,
    },
    FixtureMaterial {
        textured: false,
        texture_index: 0,
    },
    FixtureMaterial {
        textured: true,
        texture_index: 7,
    },
    FixtureMaterial {
        textured: false,
        texture_index: 0,
    },
];

/// What each present mesh of a fixture container references: mesh 0 names
/// materials 0 and 2, mesh 1 names material 0 and slot 5, which holds no record.
fn fixture_mesh_references() -> Vec<Vec<u32>> {
    vec![vec![0, 2], vec![0, 5]]
}

/// The material section: the header words, the present slots, then the zero
/// slots, the way the producing reader walks them.
fn material_section(materials: &[FixtureMaterial]) -> Vec<u8> {
    let count = materials.len() as u32;
    let mut out = Vec::new();
    for word in [count as i32, count as i32, count as i32, count as i32 - 1] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    for slot in 0..MATERIAL_SLOTS {
        let mut record = [0u8; 40];
        let (link1, link2): (i16, i16);
        if let Some(material) = materials.get(slot as usize) {
            record[0] = 255;
            record[1] = FLAG_ALWAYS | if material.textured { FLAG_TEXTURED } else { 0 };
            record[16..20].copy_from_slice(&material.texture_index.to_le_bytes());
            link1 = if slot + 1 >= count {
                -1
            } else {
                (slot + 1) as i16
            };
            link2 = if slot == 0 { -1 } else { (slot - 1) as i16 };
        } else {
            record[1] = FLAG_FREE;
            link1 = if slot == count { -1 } else { (slot - 1) as i16 };
            link2 = if slot + 1 >= MATERIAL_SLOTS {
                -1
            } else {
                (slot + 1) as i16
            };
        }
        out.extend_from_slice(&record);
        out.extend_from_slice(&link1.to_le_bytes());
        out.extend_from_slice(&link2.to_le_bytes());
    }
    assert_eq!(
        out.len() as u32,
        MATERIAL_HEADER + MATERIAL_SLOTS * MATERIAL_SLOT
    );
    out
}

/// The byte offset of present material `slot` inside a container the way this
/// fixture lays it out, computed from the fixture's own sections.
fn material_offset(textures: u32, slot: u32) -> u32 {
    TEXTURES_OFFSET + 44 * textures + MATERIAL_HEADER + slot * MATERIAL_SLOT
}

/// Writes one CS GameZ container: header, a texture-name table of `textures`
/// stem-only names, `section` as the material section, a mesh array of
/// `mesh_slots` slots whose first `references.len()` are present (each storing
/// three vertices and the listed material references), and the node array.
fn write_container(
    nodes: &[FixtureNode],
    mesh_slots: u32,
    references: &[Vec<u32>],
    textures: u32,
    section: &[u8],
) -> Vec<u8> {
    assert!(!nodes.is_empty(), "a fixture always declares a node");
    let present_meshes = references.len() as u32;
    assert!(present_meshes <= mesh_slots);
    const VERTICES_PER_MESH: u32 = 3;

    let materials_offset = TEXTURES_OFFSET + 44 * textures;
    let meshes_offset = materials_offset + section.len() as u32;
    let records_start = meshes_offset + 12;
    let data_start = records_start + mesh_slots * 104;
    // Present meshes are stored back to back; each is its vertices then its
    // 12-byte material references.
    let mut mesh_at = Vec::new();
    let mut at_data = data_start;
    for refs in references {
        mesh_at.push(at_data);
        at_data += VERTICES_PER_MESH * 12 + 12 * refs.len() as u32;
    }
    let nodes_offset = at_data;
    let info_end = nodes_offset + NODE_SLOT * nodes.len() as u32;

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
        (8, 0),
        (12, textures),
        (16, TEXTURES_OFFSET),
        (20, materials_offset),
        (24, meshes_offset),
        (28, nodes.len() as u32),
        (32, 0),
        (36, nodes_offset),
    ] {
        word(&mut bytes, offset, value);
    }

    // The texture-name table: 44-byte records, the name at offset 12 as a
    // NUL-terminated stem, `used == 2` at 32 and `-1` at 40.
    for texture in 0..textures {
        let record = (TEXTURES_OFFSET + 44 * texture) as usize;
        let name = format!("tex{texture}");
        bytes[record + 12..record + 12 + name.len()].copy_from_slice(name.as_bytes());
        word(&mut bytes, record + 32, 2);
        signed(&mut bytes, record + 40, -1);
    }
    bytes[materials_offset as usize..meshes_offset as usize].copy_from_slice(section);

    signed(&mut bytes, meshes_offset as usize, mesh_slots as i32);
    signed(
        &mut bytes,
        meshes_offset as usize + 4,
        present_meshes as i32,
    );
    signed(
        &mut bytes,
        meshes_offset as usize + 8,
        if present_meshes == 0 || present_meshes == mesh_slots {
            -1
        } else {
            present_meshes as i32
        },
    );
    for slot in 0..mesh_slots {
        let record = records_start as usize + (slot as usize) * 104;
        if slot < present_meshes {
            word(&mut bytes, record + 12, 1);
            word(&mut bytes, record + 20, VERTICES_PER_MESH);
            // `material_count`, the mesh record's 24th word.
            word(
                &mut bytes,
                record + 92,
                references[slot as usize].len() as u32,
            );
            word(&mut bytes, record + 100, mesh_at[slot as usize]);
        } else {
            signed(&mut bytes, record + 100, -1);
        }
    }
    for (slot, refs) in references.iter().enumerate() {
        let mesh = mesh_at[slot] as usize;
        for vertex in 0..VERTICES_PER_MESH {
            for axis in 0..3u32 {
                let value = (slot as u32 * 100 + vertex * 10 + axis) as f32;
                float(&mut bytes, mesh + (vertex * 12 + axis * 4) as usize, value);
            }
        }
        let refs_at = mesh + (VERTICES_PER_MESH * 12) as usize;
        for (n, material) in refs.iter().enumerate() {
            word(&mut bytes, refs_at + 12 * n, *material);
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
            "cs-f14-d-9-{label}-{}-{}",
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

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
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
fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
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

/// A fixture tree: the campaign mission, the shared planes container and the
/// own geometry container of every world group the tree discovers.
fn tree(label: &str, section: &[u8]) -> TempInstall {
    let temp = TempInstall::new(label);
    write_campaign(&temp);
    let container = |_: ()| {
        write_container(
            &fixture_nodes(),
            3,
            &fixture_mesh_references(),
            FIXTURE_TEXTURES,
            section,
        )
    };
    temp.write("ZBD/planes.zbd", &container(()));
    for group in ["C1", "C1C"] {
        temp.write(
            &format!("ZBD/{group}/{GEOMETRY_CONTAINER_FILE}"),
            &container(()),
        );
    }
    temp
}

/// The nodes of a fixture container: a root naming mesh 0, a child naming mesh
/// 1 and a child naming mesh slot 2, which the array leaves absent.
fn fixture_nodes() -> Vec<FixtureNode> {
    let mut nodes = vec![
        object("main", 0, None),
        object("wing", 1, Some(0)),
        object("ghost", 2, Some(0)),
    ];
    let parents: Vec<Option<u32>> = nodes.iter().map(|node| node.parent).collect();
    for (index, node) in nodes.iter_mut().enumerate() {
        node.children = parents
            .iter()
            .enumerate()
            .filter(|(_, parent)| **parent == Some(index as u32))
            .map(|(child, _)| child as u32)
            .collect();
    }
    nodes
}

/// How many containers `tree` writes a geometry container into.
const FIXTURE_CONTAINERS: usize = 3;
/// Present material records per fixture container.
const FIXTURE_MATERIAL_ROWS: usize = FIXTURE_MATERIALS.len();

fn container_key(spelling: &str) -> String {
    install_file_key(spelling)
}

fn unknown_claims(element: &CatalogElement) -> Vec<&str> {
    element
        .unsupported_reasons
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::Unknown { claim_id, .. } => Some(claim_id.as_str()),
            _ => None,
        })
        .collect()
}

fn target_keys(element: &CatalogElement) -> BTreeSet<String> {
    element
        .dependencies
        .iter()
        .map(|dependency| {
            format!(
                "{}:{}",
                dependency.target.kind().label(),
                dependency.target.key()
            )
        })
        .collect()
}

#[test]
fn accept_f14_d_9_a_gamez_container_yields_one_material_row_per_present_record() {
    let temp = tree("rows", &material_section(&FIXTURE_MATERIALS));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    // Four present records out of a thousand slots, three containers: a zero stub
    // slot is not content.
    assert_eq!(
        rows_of(&baseline, ContentKind::Material).len(),
        FIXTURE_CONTAINERS * FIXTURE_MATERIAL_ROWS
    );
    assert!(
        !ContentKind::Material.is_launchable(),
        "a material is not launchable content, so the denominator cannot move"
    );

    for spelling in ["ZBD/planes.zbd", "ZBD/C1/gamez.zbd", "ZBD/C1C/gamez.zbd"] {
        let key = container_key(spelling);
        let file = format!("install_file:{key}");
        let mesh = |slot: u32| format!("mesh:{key}.{slot}");
        let bytes = fs::read(temp.0.join(spelling)).expect("the container reads");

        for slot in 0..FIXTURE_MATERIAL_ROWS as u32 {
            let id = cid(ContentKind::Material, &format!("{key}.{slot}"));
            let element = row(&baseline, &id);
            assert_eq!(element.kind, ContentKind::Material);
            assert_eq!(element.parse_state, ParseState::Parsed);
            assert_eq!(element.normalize_state, NormalizeState::NotNormalized);
            assert_eq!(element.readiness, Readiness::Unavailable);
            assert!(element.runtime_consumers.is_empty());
            assert!(element.fingerprint.is_some());
            assert!(
                element
                    .unsupported_reasons
                    .iter()
                    .any(|reason| reason.code() == "not_normalized"),
                "the stored colour is raw, so no row is normalized"
            );

            // The span is the record's own extent: the reader's offset, one
            // 44-byte slot, and the bytes there are the record that was written.
            let Origin::Installation { source } = &element.origin else {
                panic!("{id} is an installation row");
            };
            let offset = u64::from(material_offset(FIXTURE_TEXTURES, slot));
            assert_eq!(source.offset(), offset, "{id}");
            assert_eq!(source.length(), u64::from(MATERIAL_SLOT), "{id}");
            assert_eq!(
                source.member_key(),
                None,
                "the container is the source itself"
            );
            let material = FIXTURE_MATERIALS[slot as usize];
            assert_eq!(
                bytes[offset as usize + 1] & FLAG_TEXTURED != 0,
                material.textured,
                "the span begins at the record whose flags were written"
            );

            // Edges: the container's file row, and the rows of the meshes that
            // reference the slot. Mesh 0 names materials 0 and 2, mesh 1 names 0.
            let mut expected = BTreeSet::from([file.clone()]);
            match slot {
                0 => expected.extend([mesh(0), mesh(1)]),
                2 => {
                    expected.insert(mesh(0));
                }
                _ => {}
            }
            assert_eq!(target_keys(element), expected, "{id}");
            for dependency in &element.dependencies {
                assert_eq!(dependency.kind, cs_types::content::DependencyKind::Static);
                assert!(
                    baseline.catalog.get(&dependency.target).is_some(),
                    "{id} points at {} which the catalog holds",
                    dependency.target
                );
            }

            // Only the material naming texture 7 of a 2-name table is unknown.
            let expected_unknowns: &[&str] = if slot == 2 {
                &["f14.d.9.baseline.material_texture_unresolved"]
            } else {
                &[]
            };
            assert_eq!(unknown_claims(element), expected_unknowns, "{id}");
        }
        // A zero stub slot has no row.
        let stub = cid(
            ContentKind::Material,
            &format!("{key}.{FIXTURE_MATERIAL_ROWS}"),
        );
        assert!(baseline.catalog.get(&stub).is_none(), "{stub} is a stub");
    }
}

#[test]
fn accept_f14_d_9_the_collection_status_counts_what_it_could_not_resolve() {
    let temp = tree("status", &material_section(&FIXTURE_MATERIALS));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let status = status_of(&baseline, ContentKind::Material);
    assert_eq!(status.rows, FIXTURE_CONTAINERS * FIXTURE_MATERIAL_ROWS);
    assert_eq!(status.source, GEOMETRY_CONTAINER_PATTERN);
    assert_eq!(status.diagnostic, None);
    assert_eq!(status.gaps.get("unreadable_container"), Some(&0));
    assert_eq!(status.gaps.get("container_read"), Some(&FIXTURE_CONTAINERS));
    assert_eq!(
        status.gaps.get("texture_index_without_texture_name"),
        Some(&FIXTURE_CONTAINERS),
        "one material per container names texture 7"
    );
    assert_eq!(
        status.gaps.get("referenced_slot_without_material"),
        Some(&FIXTURE_CONTAINERS),
        "one mesh per container references slot 5, which holds no record"
    );
    for report in &baseline.geometry_containers {
        assert_eq!(report.materials, FIXTURE_MATERIAL_ROWS);
        assert_eq!(report.material_textures_unresolved, 1);
        assert_eq!(report.material_references_without_record, 1);
    }
}

#[test]
fn accept_f14_d_9_the_material_collection_does_not_move_the_coverage_denominator() {
    let temp = tree("denominator", &material_section(&FIXTURE_MATERIALS));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    assert_eq!(baseline.roots, vec![cid(ContentKind::Mission, "ch1-m01")]);
    assert_eq!(baseline.catalog.launchable_count(), 1);
    assert!(
        rows_of(&baseline, ContentKind::Material)
            .iter()
            .all(|element| element.readiness == Readiness::Unavailable
                && element.runtime_consumers.is_empty()),
        "no material row claims a runtime consumer"
    );
}

#[test]
fn accept_f14_d_9_a_refused_material_section_costs_only_its_material_rows() {
    // `index_last` contradicts `count`: the producing reader refuses the section.
    let mut section = material_section(&FIXTURE_MATERIALS);
    section[12..16].copy_from_slice(&41i32.to_le_bytes());
    let temp = tree("refused", &section);
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");

    assert!(rows_of(&baseline, ContentKind::Material).is_empty());
    assert_eq!(
        rows_of(&baseline, ContentKind::SceneNode).len(),
        3 * FIXTURE_CONTAINERS
    );
    assert_eq!(
        rows_of(&baseline, ContentKind::Mesh).len(),
        2 * FIXTURE_CONTAINERS
    );
    let status = status_of(&baseline, ContentKind::Material);
    assert_eq!(status.rows, 0);
    assert_eq!(
        status.gaps.get("unreadable_container"),
        Some(&FIXTURE_CONTAINERS)
    );
    let diagnostic = status.diagnostic.as_deref().expect("a refusal is named");
    assert!(diagnostic.contains("ZBD/planes.zbd"), "{diagnostic}");
    assert_eq!(
        status_of(&baseline, ContentKind::Mesh).diagnostic,
        None,
        "the mesh collection is unaffected"
    );
}

#[test]
fn accept_f14_d_9_the_report_renders_the_collection_and_the_per_container_counts() {
    let temp = tree("report", &material_section(&FIXTURE_MATERIALS));
    let baseline = retail_baseline(&temp.0).expect("the fixture installation reads");
    let report = baseline_report_json(&baseline);
    let record = format!(
        "\"kind\":\"material\",\"source\":\"{GEOMETRY_CONTAINER_PATTERN}\",\"language\":null,\"rows\":{},",
        FIXTURE_CONTAINERS * FIXTURE_MATERIAL_ROWS
    );
    assert!(report.contains(&record), "{report}");
    assert!(
        report.contains(
            "\"materials\":4,\"material_textures_unresolved\":1,\
             \"material_references_without_record\":1}"
        ),
        "{report}"
    );
    let again = retail_baseline(&temp.0).expect("re-read");
    assert_eq!(
        report,
        baseline_report_json(&again),
        "the report is byte-stable"
    );
}

/// The retail half: the nine GameZ containers each yield one row per present
/// material record, every row is the extent the reader reports, and the
/// denominator did not move.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_9_retail_every_gamez_container_yields_its_present_materials() {
    let dir = game_dir();
    let baseline = retail_baseline(&dir).expect("the original installation reads");
    assert_eq!(baseline.geometry_containers.len(), 9, "nine containers");

    let status = status_of(&baseline, ContentKind::Material);
    assert_eq!(status.diagnostic, None, "every material section read");
    assert_eq!(status.gaps.get("container_visited"), Some(&9));
    assert_eq!(status.gaps.get("container_read"), Some(&9));
    assert_eq!(status.gaps.get("unreadable_container"), Some(&0));

    let rows = rows_of(&baseline, ContentKind::Material);
    assert_eq!(
        rows.len(),
        RETAIL_MATERIALS,
        "the measured present-record corpus"
    );
    assert_eq!(status.rows, rows.len());
    // 9 x 1000 slots, of which only the present records are content.
    assert!(rows.len() < 9 * 1000);

    let mut unresolved_rows = 0usize;
    for report in &baseline.geometry_containers {
        let prefix = format!("{}.", install_file_key(&report.spelling));
        let mine: Vec<&&CatalogElement> = rows
            .iter()
            .filter(|element| element.id.key().starts_with(&prefix))
            .collect();
        assert_eq!(mine.len(), report.materials, "{}", report.spelling);

        // Re-read through the production reader: each row's span is the record
        // extent it reports, one 44-byte slot, and consecutive.
        let bytes = fs::read(dir.join(&report.spelling)).expect("the container reads");
        let mut context = cs_formats::ParseContext::with_defaults(report.spelling.clone());
        let parsed =
            cs_formats::gamez::read_gamez_materials(&mut context, &report.spelling, &bytes)
                .expect("the material section reads");
        assert_eq!(parsed.materials.len(), report.materials);
        for material in &parsed.materials {
            let id = cid(
                ContentKind::Material,
                &format!("{}{}", prefix, material.index),
            );
            let element = row(&baseline, &id);
            let Origin::Installation { source } = &element.origin else {
                panic!("{id} is an installation row");
            };
            assert_eq!(source.offset(), material.data_offset, "{id}");
            assert_eq!(source.length(), 44, "{id}");
            assert_eq!(material.data_end - material.data_offset, 44, "{id}");
            assert!(element.runtime_consumers.is_empty());
            assert_eq!(element.readiness, Readiness::Unavailable);
            assert_eq!(element.normalize_state, NormalizeState::NotNormalized);
            if !unknown_claims(element).is_empty() {
                unresolved_rows += 1;
            }
        }
        assert_eq!(
            parsed.materials.last().map(|m| m.data_end),
            Some(u64::from(parsed.header.materials_offset) + 16 + 44 * u64::from(parsed.count())),
            "{}: the present slots run back to back from the section header",
            report.spelling
        );
    }
    assert_eq!(
        status.gaps.get("texture_index_without_texture_name"),
        Some(&unresolved_rows)
    );

    assert!(
        baseline
            .roots
            .iter()
            .all(|root| root.kind() != ContentKind::Material),
        "no material is a launchable root, so the denominator cannot move"
    );
}
