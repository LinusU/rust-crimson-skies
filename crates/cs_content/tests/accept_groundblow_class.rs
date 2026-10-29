//! Task #2213 (`GROUNDBLOW-CLASS-TYPE`): where a node's runtime class type
//! (`node+0x34`) comes from, and what each of M01's collider kinds gets.
//!
//! Finding: `docs/findings/2026-10-29-groundblow-class-type.md`.
//!
//! The loader that reads a GameZ container's node array `fread`s whole 212-byte
//! slots into the runtime table, so a container node's `+0x34` is the file's
//! `node_type`; runtime-built nodes get the class from their class constructor,
//! and the node copy preserves it. Retail then says every M01 collider — the
//! water tiles and every other scenery node the port makes solid — is class 5
//! (object3d), so the ground blow's world sweep (`0x4c8f70`) never skips one of
//! them by class type, and the sea answers through its tiles' triangles.
//!
//! The first test needs no original data (it checks that the finding and the
//! committed table cite the same bytes and that those bytes still say what the
//! finding claims); the second and third are `#[ignore]`d because they need
//! `CS_ENGINE_IMAGE` and `CS_GAME_DIR`. Static measurement only: nothing here
//! awards `verified_original`.

use cs_formats::ParseContext;
use cs_formats::gamez::{
    CAMERA_DATA_BYTES, DISPLAY_DATA_BYTES, LIGHT_DATA_BYTES, LOD_DATA_BYTES, NODE_TYPE_CAMERA,
    NODE_TYPE_DISPLAY, NODE_TYPE_LIGHT, NODE_TYPE_LOD, NODE_TYPE_OBJECT3D, NODE_TYPE_OFFSET,
    NODE_TYPE_WINDOW, NODE_TYPE_WORLD, OBJECT3D_DATA_BYTES, WINDOW_DATA_BYTES, WORLD_DATA_BYTES,
    read_gamez_materials, read_gamez_meshes, read_gamez_nodes,
};

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const FINDING: &str = include_str!("../../../docs/findings/2026-10-29-groundblow-class-type.md");

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// The two bits of `node+0x24` the world sweep requires before it tests a node
/// (`docs/findings/2026-10-29-groundblow-world-segment-sweep.md`).
const CONTACT_SWEEP_NODE_FLAGS: u32 = 0x14;

/// Rows the finding's conclusions rest on; each must be cited by both the
/// finding and the committed table.
const REQUIRED: &[&str] = &[
    "class.node_alloc",
    "class.object3d_ctor",
    "class.lod_ctor",
    "class.camera_ctor",
    "class.copy_dispatch",
    "class.copy_table",
    "class.info_copy",
    "class.slot_read",
    "class.loader_dispatch",
    "class.loader_table",
    "class.loader_postpass",
    "class.name_register",
    "class.name_lookup",
    "class.plane_node_lookup",
    "class.node_copy_entry",
    "class.vehicle_type_read",
    "class.vehicle_type_switch",
    "class.message_unrecognized",
    "class.message_file",
];

struct Row {
    name: String,
    va: u64,
    bytes: Vec<u8>,
}

fn hex(text: &str) -> Vec<u8> {
    assert!(
        text.len().is_multiple_of(2) && !text.is_empty(),
        "even hex: {text}"
    );
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("a hex byte"))
        .collect()
}

/// The finding's `bytes` block: `va: hex  # name` per line.
fn finding_rows() -> Vec<Row> {
    let block = FINDING
        .split("```bytes\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("the finding has a `bytes` block");
    block
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (va, rest) = line.split_once(": ").expect("`va: hex  # name`");
            let hex_text = rest.split('#').next().expect("hex").trim();
            let name = rest
                .split('#')
                .nth(1)
                .expect("a row name")
                .trim()
                .to_string();
            Row {
                name,
                va: u64::from_str_radix(va, 16).expect("a hex va"),
                bytes: hex(hex_text),
            }
        })
        .collect()
}

/// The committed `[[groundblow_class_fact]]` rows, in file order.
fn table_rows() -> Vec<Row> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a groundblow_class_fact row lacks {name}"))
    };
    TABLE
        .split("[[groundblow_class_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-29");
            assert_eq!(field(chunk, "provenance"), "static");
            let va = field(chunk, "va");
            Row {
                name: field(chunk, "name"),
                va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va"),
                bytes: hex(&field(chunk, "bytes")),
            }
        })
        .collect()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("the {name} row"))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The dwords of an eleven-entry table row.
fn dwords(row: &Row) -> Vec<u32> {
    assert_eq!(row.bytes.len(), 44, "eleven dwords: {}", row.name);
    row.bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|d| u32::from_le_bytes(*d))
        .collect()
}

/// The finding and the committed table are the same citations, and the bytes
/// still say what the finding claims.
#[test]
fn accept_groundblow_class_rows_match_the_finding_and_the_table() {
    let finding = finding_rows();
    let table = table_rows();
    assert_eq!(
        finding.len(),
        table.len(),
        "the finding cites {} windows, the table {}",
        finding.len(),
        table.len()
    );
    for (cited, committed) in finding.iter().zip(&table) {
        assert_eq!(cited.name, committed.name, "row order");
        assert_eq!(cited.va, committed.va, "{} va", cited.name);
        assert_eq!(cited.bytes, committed.bytes, "{} bytes", cited.name);
    }
    for name in REQUIRED {
        row(&finding, name);
        row(&table, name);
    }

    // The class word lives at offset 0x34 of the 208-byte info record, which is
    // exactly `NODE_TYPE_OFFSET` of the 212-byte slot.
    assert_eq!(NODE_TYPE_OFFSET, 0x34, "node_type is at slot offset 0x34");

    // A runtime node is zeroed on allocation and then stamped with its class:
    // the allocator zeroes 52 dwords of the info record.
    assert!(
        contains(
            &row(&finding, "class.node_alloc").bytes,
            &[0xb9, 0x34, 0x00, 0x00, 0x00, 0x8b, 0xfe, 0xf3, 0xab]
        ),
        "`mov ecx, 0x34; mov edi, esi; rep stosd` zeroes the 208-byte info record"
    );
    // The object3d constructor writes class 5.
    assert!(
        contains(
            &row(&finding, "class.object3d_ctor").bytes,
            &[0xc7, 0x46, 0x34, 0x05, 0x00, 0x00, 0x00]
        ),
        "`mov dword [esi+0x34], 5` is the object3d class constant"
    );
    // The lod constructor writes class 6.
    assert!(
        contains(
            &row(&finding, "class.lod_ctor").bytes,
            &[0xc7, 0x46, 0x34, 0x06, 0x00, 0x00, 0x00]
        ),
        "`mov dword [esi+0x34], 6` is the lod class constant"
    );
    // The camera constructor writes class 1 through a register.
    assert!(
        contains(
            &row(&finding, "class.camera_ctor").bytes,
            &[0xbf, 0x01, 0x00, 0x00, 0x00]
        ) && contains(
            &row(&finding, "class.camera_ctor").bytes,
            &[0x89, 0x7e, 0x34]
        ),
        "`mov edi, 1; mov [esi+0x34], edi` is the camera class constant"
    );

    // The loader reads the slot (212-byte arithmetic, then fread through
    // [0xa201f8]) and dispatches on `node+0x34`.
    assert!(
        contains(
            &row(&finding, "class.slot_read").bytes,
            &[
                0x8d, 0x04, 0x49, // lea eax, [ecx+ecx*2]
                0x8d, 0x04, 0xc0, // lea eax, [eax+eax*8]
                0xd1, 0xe0, // shl eax, 1
                0x2b, 0xc1, // sub eax, ecx
                0xc1, 0xe0, 0x02, // shl eax, 2
            ]
        ),
        "the record size is 212 bytes per slot"
    );
    assert!(
        contains(
            &row(&finding, "class.slot_read").bytes,
            &[0xff, 0x15, 0xf8, 0x01, 0xa2, 0x00]
        ),
        "`call [0xa201f8]` is the fread into the runtime row"
    );
    assert!(
        contains(
            &row(&finding, "class.loader_dispatch").bytes,
            &[0x8b, 0x46, 0x34, 0x57, 0x33, 0xff, 0x83, 0xf8, 0x0a]
        ),
        "`mov eax,[esi+0x34]; cmp eax, 0xa` is the class dispatch"
    );
    // The copy dispatches on the source's class word too.
    assert!(
        contains(
            &row(&finding, "class.copy_dispatch").bytes,
            &[0x8b, 0x48, 0x34, 0x49, 0x83, 0xf9, 0x0a, 0x0f, 0x87]
        ),
        "`mov ecx,[eax+0x34]; dec ecx; cmp ecx,0xa; ja` is the copy dispatch"
    );
    // The vehicle path reads the same word and compares it with the world class.
    assert!(
        contains(
            &row(&finding, "class.vehicle_type_read").bytes,
            &[0x8b, 0x48, 0x34, 0x83, 0xc4, 0x04, 0x83, 0xf9, 0x02]
        ),
        "`mov ecx,[eax+0x34]; cmp ecx, 2` is the vehicle's world check"
    );

    // The loader's kind table entry sizes are the workspace's own per-kind
    // record sizes, which is what ties `node+0x34` to the container's kinds.
    let loader = dwords(row(&finding, "class.loader_table"));
    assert_eq!(loader.len(), 11, "values 0..10");
    assert_eq!(loader[NODE_TYPE_CAMERA as usize], 0x004e_2d90);
    assert_eq!(loader[NODE_TYPE_WORLD as usize], 0x004e_2f4a);
    assert_eq!(loader[NODE_TYPE_WINDOW as usize], 0x004e_2efa);
    assert_eq!(loader[NODE_TYPE_DISPLAY as usize], 0x004e_2e9b);
    assert_eq!(loader[NODE_TYPE_OBJECT3D as usize], 0x004e_2c83);
    assert_eq!(loader[NODE_TYPE_LOD as usize], 0x004e_2cbb);
    assert_eq!(loader[NODE_TYPE_LIGHT as usize], 0x004e_2cfd);
    assert_eq!(loader[7], 0x004e_3283, "class 7 is refused");
    assert_eq!(loader[8], 0x004e_3283, "class 8 is refused");
    assert_eq!(loader[10], 0x004e_2bcc, "class 10 is the sound record");

    // And the copy table's object3d entry is the one that calls 0x4d1330.
    let copy = dwords(row(&finding, "class.copy_table"));
    assert_eq!(copy.len(), 11, "values 1..11");
    assert_eq!(copy[NODE_TYPE_OBJECT3D as usize - 1], 0x004d_84bc);
    assert_eq!(
        u64::from(copy[NODE_TYPE_OBJECT3D as usize - 1]),
        row(&finding, "class.copy_dispatch").va + 0x21,
        "the object3d copy entry is the window's own `push eax; call 0x4d8160`"
    );

    // The post-pass window names the four fields the loader does rewrite and
    // no store to +0x34 (`89 46 34` / `89 4e 34` / `c7 46 34`).
    let postpass = &row(&finding, "class.loader_postpass").bytes;
    assert!(
        contains(postpass, &[0x89, 0x56, 0x70]),
        "the +0x70 box pointer fixup"
    );
    assert!(
        contains(postpass, &[0x89, 0x46, 0x3c]),
        "the +0x3c mesh index"
    );
    for store in [
        [0x89, 0x46, 0x34],
        [0x89, 0x4e, 0x34],
        [0x89, 0x56, 0x34],
        [0xc7, 0x46, 0x34],
    ] {
        assert!(
            !contains(postpass, &store),
            "the post-pass must not write +0x34: {store:02x?}"
        );
    }

    // The loader's message calls the word `class_type`.
    let message = &row(&finding, "class.message_unrecognized").bytes;
    assert!(
        message.starts_with(b"gClsBlockReadNode(): Unrecognized node class type:"),
        "the loader's own message"
    );
    assert!(
        contains(message, b"class_type = %d"),
        "the message names the word class_type"
    );
    assert_eq!(
        std::str::from_utf8(&row(&finding, "class.message_file").bytes).expect("ascii"),
        "D:\\zipper\\gamez\\zclass\\cls_zbd.c",
        "the loader's source file"
    );

    // The per-kind record sizes the table's entries allocate are the reader's
    // constants (the finding's table in section 0.2).
    assert_eq!(CAMERA_DATA_BYTES, 0x1e8);
    assert_eq!(WORLD_DATA_BYTES, 0xcc);
    assert_eq!(WINDOW_DATA_BYTES, 0xf8);
    assert_eq!(DISPLAY_DATA_BYTES, 0x1c);
    assert_eq!(OBJECT3D_DATA_BYTES, 0x90);
    assert_eq!(LOD_DATA_BYTES, 0x5c);
    assert_eq!(LIGHT_DATA_BYTES, 0x100);
}

/// Every cited window is in the owner's decrypted executable.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_groundblow_class_bytes_match_the_engine_image() {
    let loaded = cs_content::coordinates::load_engine_image()
        .expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(
        loaded.digest,
        cs_content::coordinates::original_image_digest(),
        "the loader only returns the measured image"
    );
    let image = loaded.bytes.as_slice();
    let rows = table_rows();
    let mut checked = 0usize;
    for row in &rows {
        let offset = usize::try_from(row.va - IMAGE_BASE).expect("an offset");
        assert_eq!(
            &image[offset..offset + row.bytes.len()],
            row.bytes.as_slice(),
            "{} at {:#x}",
            row.name,
            row.va
        );
        checked += 1;
    }
    assert_eq!(checked, rows.len());
    assert!(
        checked >= REQUIRED.len(),
        "the table cites {checked} windows"
    );

    // Each kind's reader allocates the record size the workspace's own constant
    // names: the entry begins with `push imm32` (0x68) or `push imm8` (0x6a).
    // Class 10 logs its "must complete software" line first, so its `push 0x94`
    // comes after that call.
    let table = dwords(row(&rows, "class.loader_table"));
    for (kind, bytes) in [
        (NODE_TYPE_CAMERA, CAMERA_DATA_BYTES),
        (NODE_TYPE_WORLD, WORLD_DATA_BYTES),
        (NODE_TYPE_WINDOW, WINDOW_DATA_BYTES),
        (NODE_TYPE_DISPLAY, DISPLAY_DATA_BYTES),
        (NODE_TYPE_OBJECT3D, OBJECT3D_DATA_BYTES),
        (NODE_TYPE_LOD, LOD_DATA_BYTES),
        (NODE_TYPE_LIGHT, LIGHT_DATA_BYTES),
    ] {
        let reader = u64::from(table[kind as usize]);
        let at = usize::try_from(reader - IMAGE_BASE).expect("an offset");
        let window = &image[at..at + 8];
        let size = match window[0] {
            0x68 => u32::from_le_bytes([window[1], window[2], window[3], window[4]]),
            0x6a => u32::from(window[1]),
            other => panic!("class {kind}'s reader at {reader:#x} starts with {other:#x}"),
        };
        assert_eq!(
            u64::from(size),
            bytes,
            "class {kind}'s reader allocates its record size"
        );
    }
    let sound = u64::from(table[10]);
    let at = usize::try_from(sound - IMAGE_BASE).expect("an offset");
    assert_eq!(
        &image[at + 0x21..at + 0x28],
        &[0x68, 0x94, 0x00, 0x00, 0x00, 0xff, 0xd5],
        "class 10 logs first, then allocates its 0x94-byte record"
    );
}

/// One container read through the production readers.
fn read_container(
    root: &str,
    path: &str,
) -> (
    cs_formats::gamez::GameZNodes,
    cs_formats::gamez::GameZMeshes,
    cs_formats::gamez::GameZMaterials,
) {
    let bytes = std::fs::read(format!("{root}/{path}")).expect("the container reads");
    let mut context = ParseContext::with_defaults(path.to_ascii_lowercase());
    let nodes = read_gamez_nodes(&mut context, &bytes).expect("nodes");
    let meshes = read_gamez_meshes(&mut context, path, &bytes).expect("meshes");
    let materials = read_gamez_materials(&mut context, path, &bytes).expect("materials");
    (nodes, meshes, materials)
}

/// Whether a mesh binds at least one water-textured material group, and whether
/// it binds only water textures — the two water populations #1850 (347 nodes)
/// and #2212 (344 "contactable water nodes") measured.
fn water_binding(
    meshes: &cs_formats::gamez::GameZMeshes,
    materials: &cs_formats::gamez::GameZMaterials,
    mesh_index: i32,
) -> (bool, bool) {
    let Some(mesh) = meshes
        .meshes
        .get(usize::try_from(mesh_index).expect("a non-negative mesh index"))
        .and_then(Option::as_ref)
    else {
        return (false, false);
    };
    let mut any = false;
    let mut all = true;
    let mut textured = false;
    for group in mesh.material_groups.iter().flatten() {
        let Some(material) = materials.materials.get(group.material as usize) else {
            all = false;
            continue;
        };
        let Some(name) = materials
            .texture_of(material)
            .map(|texture| texture.name.to_ascii_lowercase())
        else {
            continue;
        };
        textured = true;
        if name.starts_with("wtr0") || name.starts_with("water1") {
            any = true;
        } else {
            all = false;
        }
    }
    (any, any && all && textured)
}

/// M01's colliders, read from the installation: every one is class 5.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_class_retail_m01_colliders_are_object3d() {
    let root = std::env::var("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the owner installation's ZBD/C3 data",
    );

    // --- the world container M01's colliders come from ---
    let (nodes, meshes, materials) = read_container(&root, "ZBD/C3/gamez.zbd");
    assert_eq!(nodes.nodes.len(), 5408, "C3's stored node count");
    let mut histogram = std::collections::BTreeMap::<u32, usize>::new();
    for node in &nodes.nodes {
        *histogram.entry(node.info.node_type).or_default() += 1;
    }
    assert_eq!(
        histogram,
        std::collections::BTreeMap::from([
            (NODE_TYPE_CAMERA, 2),
            (NODE_TYPE_WORLD, 1),
            (NODE_TYPE_WINDOW, 2),
            (NODE_TYPE_DISPLAY, 1),
            (NODE_TYPE_OBJECT3D, 4972),
            (NODE_TYPE_LOD, 429),
            (NODE_TYPE_LIGHT, 1),
        ]),
        "only CS's seven kinds occur; no 0, 7, 8 or 10"
    );

    // The sweep's flag test, then the three populations.
    let mut flagged = 0usize;
    let mut flagged_without_mesh = 0usize;
    let mut colliders = 0usize;
    let mut collider_flags = std::collections::BTreeMap::<u32, usize>::new();
    let mut water_any = 0usize;
    let mut water_only = 0usize;
    let mut water_flags = std::collections::BTreeMap::<u32, usize>::new();
    let mut water_only_flags = std::collections::BTreeMap::<u32, usize>::new();
    let mut water_with_bbox = 0usize;
    let mut colliders_with_bbox = 0usize;
    let mut land = 0usize;
    let mut land_flags = std::collections::BTreeMap::<u32, usize>::new();
    let mut collider_zones = std::collections::BTreeMap::<u32, usize>::new();
    let mut furniture = std::collections::BTreeMap::<(u32, u32), usize>::new();
    let mut furniture_names: Vec<(&str, u32)> = Vec::new();
    // The class-6 claim is about every lod node in C3, not only the flagged
    // ones, so it is counted over the whole container.
    let lod_with_mesh = nodes
        .nodes
        .iter()
        .filter(|node| node.info.node_type == NODE_TYPE_LOD && node.mesh_index() >= 0)
        .count();
    for node in &nodes.nodes {
        if node.info.flags & CONTACT_SWEEP_NODE_FLAGS != CONTACT_SWEEP_NODE_FLAGS {
            continue;
        }
        flagged += 1;
        if node.mesh_index() < 0 {
            flagged_without_mesh += 1;
            if node.info.node_type != NODE_TYPE_OBJECT3D {
                *furniture
                    .entry((node.info.node_type, node.info.flags))
                    .or_default() += 1;
                // The classes the sweep's own dispatch refuses; the 429 lod
                // nodes are handled but bind no mesh.
                if matches!(
                    node.info.node_type,
                    NODE_TYPE_CAMERA | NODE_TYPE_WORLD | NODE_TYPE_WINDOW | NODE_TYPE_DISPLAY
                ) {
                    furniture_names.push((node.name.as_str(), node.info.node_type));
                }
            }
            continue;
        }
        // Every flagged node that binds a mesh is one the port considers for a
        // collider: `PlaytestSea` when a part is water, else
        // `WorldSceneryCollider`.
        colliders += 1;
        assert_eq!(
            node.info.node_type, NODE_TYPE_OBJECT3D,
            "collider node {} '{}' must be class 5 object3d",
            node.index, node.name
        );
        assert_eq!(node.info.unk196, 160, "every collider stores unk196 = 160");
        *collider_flags.entry(node.info.flags).or_default() += 1;
        *collider_zones.entry(node.info.zone_id).or_default() += 1;
        if node.info.flags & 0x20 != 0 {
            colliders_with_bbox += 1;
        }
        let (any_water, only_water) = water_binding(&meshes, &materials, node.mesh_index());
        if any_water {
            water_any += 1;
            *water_flags.entry(node.info.flags).or_default() += 1;
            assert_eq!(node.info.zone_id, 1, "every water tile stores zone 1");
            if node.info.flags & 0x20 != 0 {
                water_with_bbox += 1;
            }
        }
        if only_water {
            water_only += 1;
            *water_only_flags.entry(node.info.flags).or_default() += 1;
        }
        if !any_water {
            land += 1;
            *land_flags.entry(node.info.flags).or_default() += 1;
        }
    }
    assert_eq!(flagged, 4736, "nodes carrying 0x04|0x10");
    assert_eq!(flagged_without_mesh, 2343, "of them bind no mesh");
    assert_eq!(
        colliders, 2393,
        "of them bind a mesh: the collider population"
    );
    assert_eq!(lod_with_mesh, 0, "no lod node binds a mesh in C3");
    assert_eq!(
        collider_flags,
        std::collections::BTreeMap::from([
            (0x0308_031c, 1851),
            (0x0308_033c, 31),
            (0x0308_071c, 124),
            (0x0308_073c, 12),
            (0x0308_831c, 367),
            (0x0308_871c, 7),
            (0x0388_031c, 1),
        ]),
        "the collider flag words"
    );
    // The stored-box path: 0x20 makes the type-5 handler take the stored box
    // as the contact; no water tile carries it.
    assert_eq!(
        colliders_with_bbox, 43,
        "colliders whose contact is the box"
    );
    assert_eq!(water_with_bbox, 0, "no water tile takes the box path");
    // #1850 counted 347 water nodes (251 indexed, 96 not); #2212 censused the
    // 344 that bind only water textures.
    assert_eq!(water_any, 347, "nodes binding a water-textured group");
    assert_eq!(
        water_flags,
        std::collections::BTreeMap::from([(0x0308_031c, 96), (0x0308_831c, 251)]),
        "the water tiles' two flag words"
    );
    assert_eq!(water_only, 344, "nodes binding only water textures");
    assert_eq!(
        water_only_flags,
        std::collections::BTreeMap::from([(0x0308_031c, 94), (0x0308_831c, 250)]),
        "the only-water nodes' two flag words"
    );
    // The `WorldSceneryCollider` population: every collider that binds no water
    // group. 2393 colliders = 347 `PlaytestSea` + 2046 `WorldSceneryCollider`;
    // the three nodes that bind a water group *and* a non-water one go to
    // `PlaytestSea` because the port asks whether *any* part is water.
    assert_eq!(land, 2046, "colliders that bind no water-textured group");
    assert_eq!(
        land_flags,
        std::collections::BTreeMap::from([
            (0x0308_031c, 1755),
            (0x0308_033c, 31),
            (0x0308_071c, 124),
            (0x0308_073c, 12),
            (0x0308_831c, 116),
            (0x0308_871c, 7),
            (0x0388_031c, 1),
        ]),
        "the land colliders' flag words"
    );
    // The layer filter 0x56c430 sees the stored zone: all water is zone 1 and
    // 1456 of the 2393 colliders store 0xff (section 0.7 of the finding).
    assert_eq!(
        collider_zones,
        std::collections::BTreeMap::from([(1, 937), (0xff, 1456)]),
        "the colliders' stored zone ids"
    );
    // The furniture that carries the bits and no mesh, and which the sweep's
    // class dispatch therefore refuses (2/3/4) or finds empty (1/6).
    let expected_furniture: std::collections::BTreeMap<(u32, u32), usize> = [
        (NODE_TYPE_CAMERA, 0x0108_001c, 2usize),
        (NODE_TYPE_WORLD, 0x0108_001c, 1),
        (NODE_TYPE_WINDOW, 0x0108_001c, 2),
        (NODE_TYPE_DISPLAY, 0x0108_001c, 1),
        (NODE_TYPE_LOD, 0x0308_051c, 429),
    ]
    .into_iter()
    .map(|(kind, flags, count)| ((kind, flags), count))
    .collect();
    assert_eq!(furniture, expected_furniture, "the flagged scene furniture");
    furniture_names.sort_unstable();
    assert_eq!(
        furniture_names,
        vec![
            ("camera1", NODE_TYPE_CAMERA),
            ("display", NODE_TYPE_DISPLAY),
            ("sgwin", NODE_TYPE_WINDOW),
            ("spyglass", NODE_TYPE_CAMERA),
            ("window1", NODE_TYPE_WINDOW),
            ("world1", NODE_TYPE_WORLD),
        ],
        "the six class 1/2/3/4 nodes; the other 429 are lod"
    );

    // The two nodes #2212 measured are class 5 as well.
    for (slot, name, mesh, flags) in [
        (547u32, "g28530", 371i32, 0x0308_831c),
        (2873, "g28603", 761, 0x0308_831c),
    ] {
        let node = nodes
            .nodes
            .iter()
            .find(|node| node.index == slot)
            .unwrap_or_else(|| panic!("node {slot} '{name}' exists"));
        assert_eq!(node.name, name, "node {slot}'s stored name");
        assert_eq!(node.info.node_type, NODE_TYPE_OBJECT3D, "{name} is class 5");
        assert_eq!(node.mesh_index(), mesh, "{name}'s mesh");
        assert_eq!(node.info.flags, flags, "{name}'s flag word");
    }

    // --- the airframe container the aircraft's node is copied from ---
    let (planes, _plane_meshes, _plane_materials) = read_container(&root, "ZBD/planes.zbd");
    assert_eq!(planes.nodes.len(), 3317, "planes.zbd's stored node count");
    let mut plane_histogram = std::collections::BTreeMap::<u32, usize>::new();
    for node in &planes.nodes {
        *plane_histogram.entry(node.info.node_type).or_default() += 1;
    }
    assert_eq!(
        plane_histogram,
        std::collections::BTreeMap::from([(NODE_TYPE_OBJECT3D, 3230), (NODE_TYPE_LOD, 87)]),
        "planes.zbd holds object3d nodes and lod bands only"
    );
    let roots: Vec<&cs_formats::gamez::RawNode> = planes
        .nodes
        .iter()
        .filter(|node| node.parent.is_none())
        .collect();
    assert_eq!(roots.len(), 28, "planes.zbd's stored roots");
    for root_node in &roots {
        assert_eq!(
            root_node.info.node_type, NODE_TYPE_OBJECT3D,
            "airframe root '{}' must be class 5",
            root_node.name
        );
    }
    let bloodhawk = planes
        .nodes
        .iter()
        .find(|node| node.name == "bloodhawk")
        .expect("M01's airframe root 'bloodhawk' exists in planes.zbd");
    assert_eq!(bloodhawk.index, 2363, "bloodhawk's stored slot");
    assert_eq!(bloodhawk.info.node_type, NODE_TYPE_OBJECT3D);
    assert_eq!(bloodhawk.info.flags, 0x0308_051c);
    assert_eq!(bloodhawk.mesh_index(), -1, "the root itself binds no mesh");
    let mut children: Vec<(&str, u32)> = bloodhawk
        .children
        .iter()
        .map(|slot| {
            let child = planes
                .nodes
                .iter()
                .find(|node| node.index == *slot)
                .expect("a stored child");
            assert_eq!(
                child.info.node_type, NODE_TYPE_OBJECT3D,
                "bloodhawk's child '{}' is class 5",
                child.name
            );
            (child.name.as_str(), child.index)
        })
        .collect();
    children.sort_unstable();
    assert_eq!(
        children,
        vec![
            ("destroyed", 2535),
            ("dontmove", 2540),
            ("healthy", 2296),
            ("markers", 2547),
            ("shadow", 2534),
        ],
        "bloodhawk's five class-5 children"
    );
    // The 87 lod bands carry the sweep bits but bind no mesh, so the class-6
    // handler finds no geometry of its own on an aircraft either.
    let flagged_lods = planes
        .nodes
        .iter()
        .filter(|node| {
            node.info.node_type == NODE_TYPE_LOD
                && node.info.flags & CONTACT_SWEEP_NODE_FLAGS == CONTACT_SWEEP_NODE_FLAGS
        })
        .count();
    assert_eq!(flagged_lods, 87, "the airframe lod bands carry the bits");
    let mut band_flags = std::collections::BTreeSet::new();
    for node in planes
        .nodes
        .iter()
        .filter(|node| node.info.node_type == NODE_TYPE_LOD)
    {
        assert!(
            node.mesh_index() < 0,
            "no lod node in planes.zbd binds a mesh ('{}')",
            node.name
        );
        band_flags.insert(node.info.flags);
    }
    assert_eq!(
        band_flags,
        std::collections::BTreeSet::from([0x0308_051c]),
        "every airframe lod band carries the same flag word"
    );
}
