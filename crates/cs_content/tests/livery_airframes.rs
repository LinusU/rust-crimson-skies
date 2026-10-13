//! `F09-PREFIX` acceptance tests (Rally #386): the stock-livery prefix to
//! airframe mapping, measured from the original scene/airframe records.
//!
//! The synthetic tests run in CI and pin every arm of the matching and
//! resolution rules; the `#[ignore]`d retail test measures the real
//! installation and pins the whole mapping with its evidence. Tests that need
//! the installation fail loudly without `$CS_GAME_DIR` rather than skipping.
//!
//! The evidence harness (`evidence_report_f09_prefix_writes_the_acceptance_report`,
//! deliberately **not** named with the task prefix) derives
//! `private/evidence/F09-PREFIX/acceptance.json` from the recorded acceptance
//! log, production discovery of `$CS_GAME_DIR` and a rerun of the production
//! roster discovery plus prefix resolution. Follow the sequence in its doc
//! comment; the committed copy lands at
//! `docs/findings/evidence/F09-PREFIX.json`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_content::livery::StockLiveryCatalog;
use cs_content::livery_airframes::{
    AirframeCandidate, CLAIM_PREFIX_AIRFRAME, CLAIM_PREFIX_UNRESOLVED, PrefixAirframeError,
    resolve_prefix_airframes,
};
use cs_formats::gamez::reader::Fixup;
use cs_formats::gamez::{
    GameZHeader, GameZMaterials, GameZMesh, GameZMeshes, GameZNodes, GameZTextureName,
    MATERIAL_FLAG_TEXTURED, MaterialInfo, MeshIndex, NODE_TYPE_OBJECT3D, OBJECT3D_FLAGS_IDENTITY,
    RawMaterial, RawMaterialRecord, RawMesh, RawMeshInfo, RawMeshMaterialInfo, RawNode,
    RawNodeInfo, RawObject3dData, TextureNameEncoding, read_gamez_materials, read_gamez_meshes,
    read_gamez_nodes,
};
use cs_formats::io::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ClaimStatus;

// ---------------------------------------------------------------- fixtures ---

/// A zeroed container header: the synthetic sections below are constructed
/// structs, not byte walks, and nothing in the resolution reads the header.
fn header() -> GameZHeader {
    GameZHeader {
        signature: 0,
        version: 0,
        unk08: 0,
        texture_count: 0,
        textures_offset: 0,
        materials_offset: 0,
        meshes_offset: 0,
        node_array_size: 0,
        light_index: 0,
        nodes_offset: 0,
    }
}

/// One object node with the given mesh association and links. The unmeasured
/// words carry the reference's asserted profile values so the record is
/// plausible; the resolution reads only the name, the links and the mesh
/// association.
fn node(index: u32, name: &str, mesh_index: i32, parent: Option<u32>, children: &[u32]) -> RawNode {
    RawNode {
        index,
        name: name.to_owned(),
        node_index: index,
        info: RawNodeInfo {
            flags: 0,
            unk040: 0,
            unk044: 1,
            zone_id: 255,
            node_type: NODE_TYPE_OBJECT3D,
            data_ptr: 0,
            mesh_index,
            environment_data: 0,
            action_priority: 1,
            action_callback: 0,
            area_partition: [-1, -1, 0, 0],
            parent_count: u16::from(parent.is_some()),
            children_count: u16::try_from(children.len()).expect("a child count fits u16"),
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
            unk196: 160,
            unk200: 0,
            unk204: 0,
        },
        kind: object_kind(),
        data_offset: 0,
        data_bytes: 212,
        parent,
        children: children.to_vec(),
    }
}

/// The object kind every synthetic node carries.
fn object_kind() -> cs_formats::gamez::NodeKind {
    cs_formats::gamez::NodeKind::Object3d(RawObject3dData {
        flags: OBJECT3D_FLAGS_IDENTITY,
        rotation: [0.0; 3],
        scale: [1.0; 3],
        matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [0.0; 3],
    })
}

fn nodes_of(list: Vec<RawNode>) -> GameZNodes {
    GameZNodes {
        header: header(),
        nodes: list,
        info_offset: 0,
        info_end: 0,
        data_offset: 0,
        data_end: 0,
        findings: vec![],
    }
}

/// A present mesh slot naming the given material records, or `None` for an
/// all-zero stub slot.
fn mesh_slot(index: u32, material_indices: &[u32]) -> Option<GameZMesh> {
    let info = RawMeshInfo {
        file_ptr: 0,
        unk04: 0,
        unk08: 0,
        parent_count: 1,
        polygon_count: 0,
        vertex_count: 0,
        normal_count: 0,
        morph_count: 0,
        light_count: 0,
        unk36: 0,
        unk40: 0.0,
        unk44: 0.0,
        unk48: 0,
        polygons_ptr: 0,
        vertices_ptr: 0,
        normals_ptr: 0,
        lights_ptr: 0,
        morphs_ptr: 0,
        unk72: 0.0,
        unk76: 0.0,
        unk80: 0.0,
        unk84: 0.0,
        unk88: 0,
        material_count: material_indices.len() as u32,
        materials_ptr: 0,
    };
    Some(GameZMesh {
        index,
        info,
        mesh: RawMesh {
            positions: vec![],
            normals: vec![],
            polygons: vec![],
        },
        polygon_records: vec![],
        lights: vec![],
        morphs: vec![],
        materials: material_indices
            .iter()
            .map(|&material_index| RawMeshMaterialInfo {
                material_index,
                polygon_usage_count: 0,
                unk_ptr: 0,
            })
            .collect(),
        material_groups: vec![],
        data_offset: 0,
        data_end: 0,
    })
}

fn meshes_of(slots: Vec<Option<GameZMesh>>) -> GameZMeshes {
    GameZMeshes {
        header: header(),
        index: MeshIndex {
            array_size: slots.len() as i32,
            count: slots.iter().filter(|slot| slot.is_some()).count() as i32,
            last_index: slots.len() as i32,
        },
        fixup: Fixup::None,
        meshes: slots,
        findings: vec![],
        unchecked_material_references: 0,
        data_offset: 0,
        data_end: 0,
    }
}

/// One texture-name record at the given container offset.
fn texture(name: &str, index: u32, offset: u64) -> GameZTextureName {
    GameZTextureName {
        index,
        name: name.to_owned(),
        stem: name.to_owned(),
        suffix: None,
        encoding: TextureNameEncoding::StemOnly,
        field00: 0,
        field32: 2,
        field36: 0,
        field40: -1,
        data_offset: offset,
        data_end: offset + 44,
    }
}

/// A material section with one textured material per given texture, material
/// `i` naming texture `i`, at the texture's own offset.
fn materials_of(textures: &[(&str, u64)]) -> GameZMaterials {
    let materials: Vec<RawMaterial> = textures
        .iter()
        .enumerate()
        .map(|(index, (_, offset))| RawMaterial {
            index: index as u32,
            record: RawMaterialRecord {
                alpha: 0xFF,
                flags: MATERIAL_FLAG_TEXTURED,
                rgb: 0x7FFF,
                color: [255.0; 3],
                texture_index: index as u32,
                field20: 0.0,
                field24: 0.5,
                field28: 0.5,
                field32: 0.0,
                cycle_ptr: 0,
            },
            link1: 0,
            link2: 0,
            cycle: None,
            data_offset: *offset,
            data_end: offset + 44,
        })
        .collect();
    let count = textures.len() as i32;
    GameZMaterials {
        header: header(),
        textures: textures
            .iter()
            .enumerate()
            .map(|(index, (name, offset))| texture(name, index as u32, *offset))
            .collect(),
        info: MaterialInfo {
            array_size: count,
            count,
            index_max: count,
            index_last: count - 1,
        },
        materials,
        free_slots: 0,
        findings: vec![],
        textures_offset: 0,
        materials_offset: 0,
        data_end: 0,
    }
}

fn candidate(airframe: &str, root_index: u32, root_name: &str) -> AirframeCandidate {
    AirframeCandidate::new(
        ContentId::from_source(ContentKind::Airframe, airframe).expect("a valid airframe id"),
        root_index,
        root_name,
    )
}

const SYNTHETIC_CONTAINER: &str = "zbd/synthetic.zbd";

/// The synthetic corpus the first two tests share: three candidate airframes
/// over six texture records.
///
/// - `player_a` (slot 0) owns mesh slots 0..3: `kes_wing`, `kes_flap`,
///   `war_wing`, `fur_a`
/// - `player_b` (slot 5) owns mesh slots 1, 2 and 4: `kes_flap`, `war_wing`,
///   `fur_b`
/// - `player_c` (slot 9) owns mesh slot 5: `gun_barrel`
/// - `warhawk_canopy.tif` sits in `player_a`'s subtree but must match no
///   prefix: it starts with `war` yet carries no `war_` separator, so a
///   matching rule that drops the separator changes the WAR verdict from
///   ambiguous to resolved and fails the test below.
fn synthetic_sections() -> (GameZNodes, GameZMeshes, GameZMaterials) {
    let textures = [
        ("kes_wing.tif", 100u64),
        ("kes_flap.tif", 200),
        ("war_wing.tif", 300),
        ("fur_a.tif", 400),
        ("fur_b.tif", 500),
        ("gun_barrel.tif", 600),
        ("warhawk_canopy.tif", 700),
    ];
    let nodes = nodes_of(vec![
        node(0, "player_a", -1, None, &[1, 2, 3, 4, 10]),
        node(1, "a_wing", 0, Some(0), &[]),
        node(2, "a_flap", 1, Some(0), &[]),
        node(3, "a_canopy", 2, Some(0), &[]),
        node(4, "a_fur", 3, Some(0), &[]),
        node(5, "player_b", -1, None, &[6, 7, 8]),
        node(6, "b_flap", 1, Some(5), &[]),
        node(7, "b_canopy", 2, Some(5), &[]),
        node(8, "b_fur", 4, Some(5), &[]),
        node(9, "player_c", 5, None, &[]),
        node(10, "a_hawk_canopy", 6, Some(0), &[]),
    ]);
    let meshes = meshes_of(vec![
        mesh_slot(0, &[0]), // kes_wing
        mesh_slot(1, &[1]), // kes_flap
        mesh_slot(2, &[2]), // war_wing
        mesh_slot(3, &[3]), // fur_a
        mesh_slot(4, &[4]), // fur_b
        mesh_slot(5, &[5]), // gun_barrel
        mesh_slot(6, &[6]), // warhawk_canopy: no prefix names it
    ]);
    (nodes, meshes, materials_of(&textures))
}

// ------------------------------------------------------------- synthetic ---

/// Containment decides, the span anchors the evidence, and absence is an
/// explicit unknown: `KES` resolves by containment, `WAR`'s equal evidence
/// and `FUR`'s incomparable evidence stay unknown with reasons, `GUN` names
/// only one candidate, and `NOPE` names none.
#[test]
fn accept_f09_prefix_resolves_by_contained_evidence_and_reports_the_span() {
    let (nodes, meshes, materials) = synthetic_sections();
    let candidates = [
        candidate("player_a", 0, "player_a"),
        candidate("player_b", 5, "player_b"),
        candidate("player_c", 9, "player_c"),
    ];
    let prefixes: Vec<String> = ["KES", "WAR", "FUR", "GUN", "NOPE"]
        .iter()
        .map(|prefix| (*prefix).to_owned())
        .collect();
    let install = cs_assets::install::sha256(b"synthetic install");
    let container_hash = cs_assets::install::sha256(b"synthetic container");
    let map = resolve_prefix_airframes(
        SYNTHETIC_CONTAINER,
        install,
        container_hash,
        &nodes,
        &meshes,
        &materials,
        &candidates,
        &prefixes,
    )
    .expect("the caller data is consistent");
    assert_eq!(map.container(), SYNTHETIC_CONTAINER);
    assert_eq!(map.len(), 5);
    assert!(!map.is_empty());

    // KES: player_a's {kes_wing, kes_flap} contains player_b's {kes_flap}.
    let kes = map.mapping("kes").expect("the prefix is in the map");
    let Resolved::Known(known) = kes.mapping() else {
        panic!("contained evidence must resolve: {:?}", kes.mapping())
    };
    assert_eq!(known.value.as_str(), "airframe/player_a");
    assert_eq!(known.provenance.claim_id.as_str(), CLAIM_PREFIX_AIRFRAME);
    assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
    let span: &SourceSpan = known
        .provenance
        .source
        .as_ref()
        .expect("a resolved mapping carries its span");
    assert_eq!(span.install_sha256(), install);
    assert_eq!(span.container_path(), SYNTHETIC_CONTAINER);
    assert_eq!(span.member_key(), None);
    assert_eq!(span.member_sha256(), Some(container_hash));
    // The anchor is `kes_wing`'s record: the smallest matched offset, 100.
    assert_eq!(span.offset(), 100);
    assert_eq!(span.length(), 44);
    // Both naming candidates are reported, with the containment visible in
    // the distinct-name counts.
    let evidence: Vec<(&str, usize, usize)> = kes
        .naming()
        .iter()
        .map(|evidence| {
            (
                evidence.airframe().as_str(),
                evidence.distinct_names(),
                evidence.referencing_meshes(),
            )
        })
        .collect();
    assert_eq!(
        evidence,
        vec![("airframe/player_a", 2, 2), ("airframe/player_b", 1, 1),]
    );
    assert!(
        kes.naming()
            .iter()
            .all(|evidence| evidence.unresolved_meshes() == 0
                && evidence.unresolved_materials() == 0)
    );

    // WAR: equal evidence on two candidates is ambiguity, not a winner.
    let war = map.mapping("WAR").expect("the prefix is in the map");
    let Resolved::Unknown { claim_id, reason } = war.mapping() else {
        panic!("equal evidence must stay unknown: {:?}", war.mapping())
    };
    assert_eq!(claim_id.as_str(), CLAIM_PREFIX_UNRESOLVED);
    assert!(reason.contains("airframe/player_a"), "{reason}");
    assert!(reason.contains("airframe/player_b"), "{reason}");
    assert!(
        reason.contains("0 of them no other candidate names"),
        "{reason}"
    );
    assert_eq!(war.naming().len(), 2);

    // FUR: incomparable single-name evidence is ambiguity too, and the
    // reason names each side's exclusive evidence.
    let fur = map.mapping("FUR").expect("the prefix is in the map");
    let Resolved::Unknown { reason, .. } = fur.mapping() else {
        panic!(
            "incomparable evidence must stay unknown: {:?}",
            fur.mapping()
        )
    };
    assert!(
        reason.contains("1 of them no other candidate names"),
        "{reason}"
    );

    // GUN: a single naming candidate resolves on its own.
    let gun = map.mapping("GUN").expect("the prefix is in the map");
    let Resolved::Known(known) = gun.mapping() else {
        panic!("a lone naming candidate must resolve: {:?}", gun.mapping())
    };
    assert_eq!(known.value.as_str(), "airframe/player_c");
    let span = known.provenance.source.as_ref().expect("the span");
    assert_eq!(span.offset(), 600);
    assert_eq!(span.length(), 44);
    assert_eq!(gun.naming().len(), 1);

    // NOPE: no candidate names it — an explicit unknown, not a gap.
    let nope = map.mapping("NOPE").expect("the prefix is in the map");
    let Resolved::Unknown { claim_id, reason } = nope.mapping() else {
        panic!("an unnamed prefix must be unknown: {:?}", nope.mapping())
    };
    assert_eq!(claim_id.as_str(), CLAIM_PREFIX_UNRESOLVED);
    assert!(reason.contains("nope_"), "{reason}");
    assert!(nope.naming().is_empty());
    assert!(map.resolved_airframe("GUN").is_some_and(Resolved::is_known));
    assert!(
        map.resolved_airframe("WAR")
            .is_some_and(|resolved| !resolved.is_known())
    );
    assert!(map.resolved_airframe("missing").is_none());
}

/// The walk terminates on a cyclic, self-listing child structure, counts
/// every link it cannot resolve (a mesh index past the array, a material
/// index past the table) and still resolves the evidence that does read.
#[test]
fn accept_f09_prefix_walk_is_cycle_safe_and_counts_unresolvable_links() {
    let textures = [("kes_wing.tif", 100u64)];
    let nodes = nodes_of(vec![
        // A duplicate child slot and a back edge: neither may loop the walk
        // or double-count the evidence.
        node(0, "player_a", -1, None, &[1, 2, 1]),
        node(1, "a_missing", 9, Some(0), &[]), // mesh 9 is past the 2-slot array
        node(2, "a_wing", 1, Some(0), &[0]),   // back edge to the root
    ]);
    let meshes = meshes_of(vec![
        mesh_slot(0, &[0]),
        // Slot 1 names material 0 (present) and material 7 (past the table).
        mesh_slot(1, &[0, 7]),
    ]);
    let materials = materials_of(&textures);
    let prefixes: Vec<String> = vec!["KES".to_owned()];
    let map = resolve_prefix_airframes(
        SYNTHETIC_CONTAINER,
        cs_assets::install::sha256(b"synthetic install"),
        cs_assets::install::sha256(b"synthetic container"),
        &nodes,
        &meshes,
        &materials,
        &[candidate("player_a", 0, "player_a")],
        &prefixes,
    )
    .expect("the caller data is consistent");
    let kes = map.mapping("KES").expect("the prefix is in the map");
    let Resolved::Known(known) = kes.mapping() else {
        panic!("the readable evidence must resolve: {:?}", kes.mapping())
    };
    assert_eq!(known.value.as_str(), "airframe/player_a");
    let evidence = &kes.naming()[0];
    assert_eq!(evidence.distinct_names(), 1);
    assert_eq!(evidence.referencing_meshes(), 1);
    assert_eq!(
        evidence.unresolved_meshes(),
        1,
        "the out-of-range mesh index must be reported, not skipped"
    );
    assert_eq!(
        evidence.unresolved_materials(),
        1,
        "the out-of-range material index must be reported, not skipped"
    );
}

/// Inconsistent caller data is refused with its own variant, never repaired:
/// a slot the container does not hold, a slot whose stored name disagrees, a
/// doubly claimed slot, two prefixes that fold together, and an empty prefix.
#[test]
fn accept_f09_prefix_refuses_inconsistent_caller_data() {
    let (nodes, meshes, materials) = synthetic_sections();
    let one_prefix = vec!["KES".to_owned()];
    let install = cs_assets::install::sha256(b"synthetic install");
    let container_hash = cs_assets::install::sha256(b"synthetic container");

    let missing = resolve_prefix_airframes(
        SYNTHETIC_CONTAINER,
        install,
        container_hash,
        &nodes,
        &meshes,
        &materials,
        &[candidate("player_a", 99, "player_a")],
        &one_prefix,
    )
    .expect_err("a slot past the node array must be refused");
    assert!(
        matches!(
            missing,
            PrefixAirframeError::CandidateNodeMissing { root_index: 99, .. }
        ),
        "{missing:?}"
    );

    let mismatch = resolve_prefix_airframes(
        SYNTHETIC_CONTAINER,
        install,
        container_hash,
        &nodes,
        &meshes,
        &materials,
        &[candidate("player_a", 0, "some_other_root")],
        &one_prefix,
    )
    .expect_err("a stale root name must be refused");
    assert!(
        matches!(
            mismatch,
            PrefixAirframeError::CandidateRootNameMismatch { root_index: 0, .. }
        ),
        "{mismatch:?}"
    );

    let duplicate_root = resolve_prefix_airframes(
        SYNTHETIC_CONTAINER,
        install,
        container_hash,
        &nodes,
        &meshes,
        &materials,
        &[
            candidate("player_a", 0, "player_a"),
            candidate("player_alt", 0, "player_a"),
        ],
        &one_prefix,
    )
    .expect_err("one subtree may not be two airframes");
    assert!(
        matches!(
            duplicate_root,
            PrefixAirframeError::DuplicateCandidateRoot { root_index: 0 }
        ),
        "{duplicate_root:?}"
    );

    let duplicate_prefix = resolve_prefix_airframes(
        SYNTHETIC_CONTAINER,
        install,
        container_hash,
        &nodes,
        &meshes,
        &materials,
        &[candidate("player_a", 0, "player_a")],
        &["KES".to_owned(), "kes".to_owned()],
    )
    .expect_err("two spellings of one folded prefix must be refused");
    assert!(
        matches!(
            duplicate_prefix,
            PrefixAirframeError::DuplicatePrefix { .. }
        ),
        "{duplicate_prefix:?}"
    );

    let empty_prefix = resolve_prefix_airframes(
        SYNTHETIC_CONTAINER,
        install,
        container_hash,
        &nodes,
        &meshes,
        &materials,
        &[candidate("player_a", 0, "player_a")],
        &["  ".to_owned()],
    )
    .expect_err("an empty prefix names no separator rule");
    assert!(
        matches!(empty_prefix, PrefixAirframeError::EmptyPrefix),
        "{empty_prefix:?}"
    );
}

// ----------------------------------------------------------------- retail ---

/// The read-only original installation, or a loud failure when the `retail`
/// capability is missing.
fn retail_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect(
        "CS_GAME_DIR must point at the original installation: this test measures the \
         prefix-to-airframe mapping against real container bytes and cannot pass without them",
    ))
}

/// Mounts the shared airframe library through the production ROF mount of an
/// already-discovered installation.
fn retail_source(root: &Path, found: &cs_assets::install::Discovery) -> cs_assets::rof::RofSource {
    use cs_assets::rof::mount_rof_into;
    use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
    use cs_types::asset_id::{MountId, MountNamespace, PrecedenceClass, ResolveContext};

    const CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";
    let context = ResolveContext::new(cs_assets::install::fingerprint(&found.manifest));
    let mut builder = SessionBuilder::new(context);
    let mount = MountBuilder::new(
        MountId::new("rof-gosdata-assets-crimson-rof").expect("a valid mount id"),
        MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
        PrecedenceClass::Shared,
        CONTAINER,
    )
    .retail();
    mount_rof_into(&mut builder, mount, &root.join(CONTAINER)).expect("the airframe library mounts")
}

/// The eleven stock prefixes, the declared airframe each one names and that
/// airframe's distinct matched texture-name count — all measured from the
/// installation by this suite and pinned here so a drift fails a test.
///
/// The candidate airframes are the eleven rows the loading-script container
/// declares (`support\planes.gw`, through the production roster discovery),
/// and the prefixes are the eleven the production stock-livery catalog
/// measures over `GOSDATA/ASSETS/crimson.rof`.
const RETAIL_EXPECTED: [(&str, &str, usize); 11] = [
    ("AGYRO", "airframe/player_autogyro", 12),
    ("BAL", "airframe/player_balmoral", 11),
    ("BLO", "airframe/player_bhawk", 8),
    ("BRI", "airframe/player_brigand", 14),
    ("DEV", "airframe/player_pfighter", 14),
    ("FIR", "airframe/player_fbrand", 9),
    ("FUR", "airframe/player_fury", 10),
    ("HEL", "airframe/player_avenger", 7),
    ("KES", "airframe/player_kestrel", 9),
    ("PEA", "airframe/player_peacemaker", 10),
    ("WAR", "airframe/player_warhawk", 10),
];

/// Everything the retail measurement needs, run through production code.
struct RetailMeasurement {
    install_sha256: cs_types::evidence::ContentHash,
    container_sha256: cs_types::evidence::ContentHash,
    bytes: Vec<u8>,
    prefixes: Vec<String>,
    map: cs_content::livery_airframes::PrefixAirframeMap,
}

/// The production pipeline this task establishes: roster discovery names the
/// candidate airframes, the stock-livery catalog measures the prefixes, and
/// the GameZ sections of `ZBD/planes.zbd` carry the evidence chain.
fn retail_measurement(game_dir: &Path) -> RetailMeasurement {
    use cs_content::catalog::baseline::airframe_roster_declarations;
    use cs_content::scene::{discover_airframe_roster, escape_scene_node_name};
    use cs_formats::interp::decode_interp;

    let found = cs_assets::install::discover(game_dir)
        .expect("production discovery must read the original installation");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest);

    let interp_bytes = fs::read(game_dir.join("ZBD").join("interp.zbd"))
        .expect("the loading-script container must be there");
    let decoded = decode_interp(
        &mut ParseContext::with_defaults("zbd/interp.zbd".to_owned()),
        &interp_bytes,
    )
    .expect("the loading-script container must decode");
    let declarations = airframe_roster_declarations(install_sha256, "ZBD/interp.zbd", &decoded)
        .expect("the production idiom builds over the real container");
    let roster = discover_airframe_roster(&decoded, &declarations)
        .expect("eleven distinct roots are not a contradiction");
    assert_eq!(
        roster.discovered().len(),
        RETAIL_EXPECTED.len(),
        "the shared archive declares one candidate per stock airframe"
    );

    let container = "ZBD/planes.zbd";
    let bytes = fs::read(game_dir.join("ZBD").join("planes.zbd"))
        .expect("the shared aircraft archive must be there");
    let container_sha256 = cs_assets::install::sha256(&bytes);
    let mut context = ParseContext::with_defaults(container.to_owned());
    let nodes = read_gamez_nodes(&mut context, &bytes).expect("the node array must read");
    let meshes = read_gamez_meshes(&mut context, container, &bytes).expect("the mesh section");
    let materials =
        read_gamez_materials(&mut context, container, &bytes).expect("the material section");

    // Each declared root must name exactly one node of the container; a
    // missing or duplicated name is a stale transcription, not a mapping.
    // The roster stores the root as its escaped scene-node key
    // (`<container key>.<escaped name>`), so the candidate compares escaped
    // names through the production escape rule and keeps the stored name.
    let candidates: Vec<AirframeCandidate> = roster
        .discovered()
        .iter()
        .map(|row| {
            let key_prefix = format!("{}.", row.container().key());
            let escaped = row
                .root()
                .root()
                .key()
                .strip_prefix(&key_prefix)
                .unwrap_or_else(|| {
                    panic!(
                        "{}: the roster root must live in its own container, got {}",
                        row.airframe().as_str(),
                        row.root().root().key()
                    )
                });
            let mut matches = nodes
                .nodes
                .iter()
                .filter(|node| escape_scene_node_name(&node.name) == escaped);
            let found = matches.next().unwrap_or_else(|| {
                panic!(
                    "the container must hold the declared root {}: {}",
                    row.airframe().as_str(),
                    escaped
                )
            });
            assert!(
                matches.next().is_none(),
                "root name {escaped} must be unique in the node array"
            );
            AirframeCandidate::new(row.airframe().clone(), found.index, &found.name)
        })
        .collect();

    let source = retail_source(game_dir, &found);
    let catalog = StockLiveryCatalog::discover(&source);
    assert_eq!(
        catalog.prefixes().len(),
        RETAIL_EXPECTED.len(),
        "the stock library stores the eleven measured prefixes"
    );

    let map = resolve_prefix_airframes(
        container,
        install_sha256,
        container_sha256,
        &nodes,
        &meshes,
        &materials,
        &candidates,
        &catalog.prefixes(),
    )
    .expect("the measured caller data is consistent");
    RetailMeasurement {
        install_sha256,
        container_sha256,
        bytes,
        prefixes: catalog.prefixes(),
        map,
    }
}

/// AC over the real installation: every stock prefix resolves to exactly the
/// declared airframe whose subtree names its textures, with a span over the
/// container bytes that literally spell the prefix, the measured per-airframe
/// evidence, and no unresolved link anywhere in the walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f09_prefix_retail_every_stock_prefix_resolves_to_its_declared_airframe() {
    let measured = retail_measurement(&retail_dir());
    let RetailMeasurement {
        install_sha256,
        container_sha256,
        bytes,
        prefixes,
        map,
    } = measured;

    assert_eq!(map.mappings().len(), RETAIL_EXPECTED.len());
    for (prefix, airframe, distinct) in RETAIL_EXPECTED {
        assert!(
            prefixes
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(prefix)),
            "the stock library must store the prefix {prefix}"
        );
        let mapping = map
            .mapping(prefix)
            .unwrap_or_else(|| panic!("{prefix} must have a verdict"));
        let Resolved::Known(known) = mapping.mapping() else {
            panic!("{prefix} must resolve from original data: {mapping:?}")
        };
        assert_eq!(
            known.value.as_str(),
            airframe,
            "{prefix} names the airframe whose subtree carries its textures"
        );
        assert_eq!(known.provenance.claim_id.as_str(), CLAIM_PREFIX_AIRFRAME);
        assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
        let span = known
            .provenance
            .source
            .as_ref()
            .expect("a resolved mapping carries its span");
        assert_eq!(span.install_sha256(), install_sha256);
        assert_eq!(span.container_path(), "ZBD/planes.zbd");
        assert_eq!(span.member_key(), None);
        assert_eq!(span.member_sha256(), Some(container_sha256));
        // The span must cover bytes that spell the prefix, so the evidence
        // cannot point at some unrelated record.
        let offset = usize::try_from(span.offset()).expect("an offset fits usize");
        let end = offset
            .checked_add(usize::try_from(span.length()).expect("a length fits usize"))
            .expect("the span end fits usize");
        assert!(
            end <= bytes.len(),
            "{prefix}: the span must lie in the container"
        );
        let anchored = String::from_utf8_lossy(&bytes[offset..end]).to_ascii_lowercase();
        let head = format!("{prefix}_").to_ascii_lowercase();
        assert!(
            anchored.contains(&head),
            "{prefix}: the anchored record must cover bytes spelling the prefix, got {anchored:?}"
        );
        // The winner's measured evidence: distinct names, meshes referencing
        // them, and zero unresolved links in the whole container.
        let winner = mapping
            .naming()
            .iter()
            .find(|evidence| evidence.airframe().as_str() == airframe)
            .unwrap_or_else(|| panic!("{prefix}: the resolved airframe must be naming evidence"));
        assert_eq!(winner.distinct_names(), distinct, "{prefix}");
        assert!(winner.referencing_meshes() > 0, "{prefix}");
        assert_eq!(winner.unresolved_meshes(), 0, "{prefix}");
        assert_eq!(winner.unresolved_materials(), 0, "{prefix}");
    }

    // The one measured cross-reference, pinned so it cannot appear or move
    // silently: the autogyro prefix's cockpit texture is also referenced by
    // the player avenger and the player fury, and both carry strictly less
    // evidence than the autogyro itself.
    let agyro = map.mapping("AGYRO").expect("AGYRO has a verdict");
    let mut leak: Vec<(String, usize)> = agyro
        .naming()
        .iter()
        .map(|evidence| {
            (
                evidence.airframe().as_str().to_owned(),
                evidence.distinct_names(),
            )
        })
        .collect();
    leak.sort();
    assert_eq!(
        leak,
        vec![
            ("airframe/player_autogyro".to_owned(), 12),
            ("airframe/player_avenger".to_owned(), 1),
            ("airframe/player_fury".to_owned(), 1),
        ],
        "the AGYRO evidence and its two dominated cross-references are measured"
    );

    // No prefix is left unknown and none is resolved twice: the map is a
    // total function over the measured library.
    assert!(
        map.mappings()
            .iter()
            .all(|mapping| mapping.mapping().is_known())
    );
    let resolved: BTreeSet<&str> = map
        .mappings()
        .iter()
        .map(|mapping| {
            let Resolved::Known(known) = mapping.mapping() else {
                panic!("every measured prefix resolves")
            };
            known.value.as_str()
        })
        .collect();
    assert_eq!(
        resolved.len(),
        RETAIL_EXPECTED.len(),
        "each stock prefix names its own airframe; no airframe is claimed by two prefixes"
    );
}

// -------------------------------------------------------- evidence harness ---

/// Writes `private/evidence/F09-PREFIX/acceptance.json` from the recorded
/// acceptance log, production discovery and a rerun of the production
/// measurement. Run it exactly as documented in the module doc; the report
/// refuses a stale `CS_CANDIDATE_TREE`, a missing log and a failed run.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f09_prefix_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f09_prefix_` tests were recorded in {}",
        log_path.display()
    );
    let retail = suite
        .assertions
        .iter()
        .find(|(name, _)| name.contains("accept_f09_prefix_retail_"))
        .unwrap_or_else(|| {
            panic!(
                "the retail acceptance test did not run: F09-PREFIX requires capability \
                 `retail`, run step 1 with `--include-ignored` and CS_GAME_DIR set"
            )
        });
    assert_eq!(retail.1, "pass", "the retail acceptance test must pass");
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name.contains("accept_f09_prefix_")
                && !name.contains("accept_f09_prefix_retail_")),
        "synthetic task tests must be present alongside the retail one"
    );

    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();

    // The substantive measurement, rerun through production code: the map
    // artifact carries ids, counts and verdicts only — never a texture name
    // or any other original string.
    let measured = retail_measurement(&game_dir);
    assert_eq!(
        measured.map.mappings().len(),
        RETAIL_EXPECTED.len(),
        "the rerun must resolve one verdict per measured prefix"
    );
    let map_path = evidence_dir.join("prefix-airframe-map.json");
    fs::write(&map_path, map_json(&measured))
        .unwrap_or_else(|error| panic!("write {}: {error}", map_path.display()));
    for needle in [
        "\"schema\":\"cs-f09-prefix-airframe-map/1\"",
        "\"retail\":true",
        &format!("\"install_sha256\":\"{install_sha256}\""),
        &format!(
            "\"container_sha256\":\"{}\"",
            measured.container_sha256.to_hex()
        ),
        "\"resolved\":11",
        "\"unknown\":0",
    ] {
        assert!(
            map_path_content(&map_path).contains(needle),
            "the map artifact is missing {needle:?}"
        );
    }

    let engine = format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&rustc_version()),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d"))
    );
    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&map_path, "json", &evidence_dir),
    ];

    let review = std::env::var("CS_EVIDENCE_REVIEW").unwrap_or_else(|_| {
        "pending: written by the implementing agent bunny-alpha-1. Rally assigns the reviewing \
         agent, who must regenerate this report on the reviewed and rebased commit and replace \
         this text with their own identity and method (CS_EVIDENCE_REVIEW); prefer a different \
         agent instance or model with a fresh context for this evidence machinery, and no agent \
         review awards more than `checked`. Method: the acceptance suite ran locally with the \
         retail capability over $CS_GAME_DIR; the map artifact is the production roster \
         discovery, the production stock-livery catalog and the production GameZ sections of \
         ZBD/planes.zbd rerun over the same installation. Deferred boundaries recorded in \
         docs/findings/2026-10-14-f09-prefix-airframe-mapping.md: the containment rule is a \
         designed decision procedure over measured evidence and gates no unknown prefix in this \
         corpus; the mapping covers the shared airframe archive only (mission-only airframes \
         are a later roster stage); evidence class is observed_tool, retail is file access and \
         nothing here claims verified_original."
            .to_owned()
    });

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F09-PREFIX\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine,
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&review),
        jstr(&review),
    );

    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    for needle in [
        "\"task_id\": \"F09-PREFIX\"",
        "\"claim\": \"implemented\"",
        "\"unknowns\": []",
        "accept_f09_prefix_retail_",
        "prefix-airframe-map.json",
    ] {
        assert!(report.contains(needle), "the report is missing {needle:?}");
    }
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    println!("wrote {}", out.display());
}

fn map_path_content(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The measured map as a JSON artifact: the caller prefixes, each verdict and
/// its per-airframe evidence counts. Ids, counts and offsets only — no
/// original texture name is written anywhere.
fn map_json(measured: &RetailMeasurement) -> String {
    use cs_content::livery_airframes::PrefixAirframeMapping;
    use cs_types::content::Resolved as R;

    let verdicts: Vec<String> = measured
        .map
        .mappings()
        .iter()
        .map(|mapping: &PrefixAirframeMapping| {
            let (status, airframe, claim, span) = match mapping.mapping() {
                R::Known(known) => {
                    let span = known.provenance.source.as_ref().expect("a span");
                    (
                        "resolved".to_owned(),
                        jstr(known.value.as_str()),
                        jstr(known.provenance.claim_id.as_str()),
                        format!(
                            "{{\"offset\": {}, \"length\": {}}}",
                            span.offset(),
                            span.length()
                        ),
                    )
                }
                R::Unknown { claim_id, reason } => (
                    "unknown".to_owned(),
                    "null".to_owned(),
                    jstr(claim_id.as_str()),
                    format!("{{\"reason\": {}}}", jstr(reason)),
                ),
            };
            let evidence: Vec<String> = mapping
                .naming()
                .iter()
                .map(|evidence| {
                    format!(
                        "{{\"airframe\": {}, \"root_index\": {}, \"distinct_names\": {}, \
                         \"referencing_meshes\": {}, \"unresolved_meshes\": {}, \
                         \"unresolved_materials\": {}}}",
                        jstr(evidence.airframe().as_str()),
                        evidence.root_index(),
                        evidence.distinct_names(),
                        evidence.referencing_meshes(),
                        evidence.unresolved_meshes(),
                        evidence.unresolved_materials(),
                    )
                })
                .collect();
            format!(
                "{{\"prefix\": {}, \"status\": {status:?}, \"airframe\": {airframe}, \
                 \"claim\": {claim}, \"anchor\": {span}, \"naming\": [{}]}}",
                jstr(mapping.prefix()),
                evidence.join(", ")
            )
        })
        .collect();
    let resolved = measured
        .map
        .mappings()
        .iter()
        .filter(|mapping| mapping.mapping().is_known())
        .count();
    let unknown = measured.map.mappings().len() - resolved;
    format!(
        "{{\"schema\":\"cs-f09-prefix-airframe-map/1\",\"retail\":true,\
         \"install_sha256\":{},\"container_sha256\":{},\"container\":\"ZBD/planes.zbd\",\
         \"prefixes\":{},\"resolved\":{},\"unknown\":{},\"verdicts\":[{}]}}",
        jstr(&measured.install_sha256.to_hex()),
        jstr(&measured.container_sha256.to_hex()),
        measured.prefixes.len(),
        resolved,
        unknown,
        verdicts.join(", ")
    )
}

// --------------------------------------------------------------- helpers ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set: this harness only runs through the sequence in its doc comment")
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path written relative to the workspace root must be re-anchored.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f09_prefix_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let prefix = "accept_f09_prefix_";
    let mut suite = Suite::default();
    let mut pending: Vec<String> = Vec::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if !pending.is_empty() {
            if trimmed == "ok" {
                let name = pending.remove(0);
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.remove(0);
                record(&mut suite, name, "fail");
                continue;
            }
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            if !full.contains(prefix) {
                cursor = &after[separator + 5..];
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
        }
    }
    fields
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

/// One referenced artifact, hashed with the **production** SHA-256.
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = year_of_day + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
