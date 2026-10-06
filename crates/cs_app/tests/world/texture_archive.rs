//! F08-C acceptance tests (#688): the consumer that chooses a world's texture
//! archive is wired end to end — the detail-settings surface writes the
//! measured `TextureMemory_HW`/`TextureMemory_SW` members, the renderer-side
//! detail reports the project's device total, and the world load opens the one
//! archive the measured rule selects.
//!
//! Spec: `specs/F08-texture-archives-and-conventional-image-decoding.md`;
//! the measured rule is
//! `docs/findings/2026-10-05-t352-texture-archive-selection-rule.md`.
//!
//! Every assertion goes through the production path —
//! [`read_world_containers`]`::container`, which discovers the installation,
//! builds the world's `TextureFiles` from the manifest, calls
//! `TextureCatalog::open_world` and runs the mesh audit with the measured
//! lookup order. The synthetic installation is authored bytes under a
//! temporary directory: a GameZ container laid out the way
//! `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` records it, texture
//! packages the way
//! `docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md` records them, and
//! nothing committed from the original. The retail test is `#[ignore]`d for
//! CI and reads `$CS_GAME_DIR` only.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::render::detail::RendererDetail;
use cs_app::world::{read_world_container, read_world_containers};
use cs_content::detail::{DetailDevice, DetailSettings};
use cs_content::mesh::MaterialState;
use cs_content::textures::{PROJECT_HARDWARE_TEXTURE_MIB, TextureDetailRow};
use cs_formats::gamez::{
    CORNER_COUNT_MASK, FLAG_NORMALS, FLAG_SHIFT, MATERIAL_FLAG_ALWAYS, MATERIAL_FLAG_TEXTURED,
    NG_MATERIAL_SLOTS, NODE_INDEX_TOP, NODE_SLOT_BYTES, OBJECT3D_DATA_BYTES,
    OBJECT3D_FLAGS_IDENTITY,
};
use cs_formats::texture::zbd::{FLAG_BYTES_PER_PIXEL2, FLAG_NO_ALPHA, ZBD_TEXTURE_HEADER_BYTES};
use cs_formats::zbd::{GAMEZ_SIGNATURE, GAMEZ_VERSION};

// ---------------------------------------------------------- the fixture bytes --

/// The texture-archive layout's own numbers, spelled so the fixture does not
/// borrow the reader's arithmetic: a 24-byte header, one 40-byte table entry
/// per texture whose first 32 bytes are the name.
const PACKAGE_ENTRY: usize = 40;
const PACKAGE_NAME: usize = 32;
/// A 1x1 direct-colour texture: no alpha, two bytes per texel.
const PACKAGE_OPAQUE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_NO_ALPHA;

/// A synthetic texture package: `names`, each a 1x1 texture whose single
/// texel word is its table position plus one.
fn texture_package(names: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for word in [0u32, 1, 0, names.len() as u32, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let mut offset = ZBD_TEXTURE_HEADER_BYTES + names.len() * PACKAGE_ENTRY;
    let mut bodies = Vec::new();
    for (position, name) in names.iter().enumerate() {
        let mut table = vec![0u8; PACKAGE_NAME];
        table[..name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&table);
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        out.extend_from_slice(&(-1i32).to_le_bytes());

        let mut body = Vec::new();
        body.extend_from_slice(&PACKAGE_OPAQUE.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&((position as u16) + 1).to_le_bytes());
        offset += body.len();
        bodies.push(body);
    }
    for body in bodies {
        out.extend_from_slice(&body);
    }
    out
}

/// One stored texture-name record: three `u32` words, the 20-byte name and
/// three more words, as the material reader lays them out.
fn texture_name(name: &str) -> Vec<u8> {
    assert!(name.len() < 20, "a fixture name fits the 20-byte field");
    let mut out = Vec::new();
    for word in [0u32, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let mut field = [0u8; 20];
    field[..name.len()].copy_from_slice(name.as_bytes());
    out.extend_from_slice(&field);
    for word in [2u32, 0, u32::MAX] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    assert_eq!(out.len(), 44, "a texture-name record is 44 bytes");
    out
}

/// One present material record plus its two link words: the forty stored
/// bytes and the four that follow them. The values are the reference's
/// asserted profile so the reader raises no finding for them.
fn material_slot(index: u32, count: u32, texture_index: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0xFF); // alpha
    out.push(MATERIAL_FLAG_ALWAYS | MATERIAL_FLAG_TEXTURED); // flags
    out.extend_from_slice(&0x7FFFu16.to_le_bytes()); // rgb
    for _ in 0..3 {
        out.extend_from_slice(&255.0f32.to_le_bytes()); // color
    }
    out.extend_from_slice(&texture_index.to_le_bytes());
    for value in [0.0f32, 0.5, 0.5, 0.25 + index as f32] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // cycle_ptr
    let link1: i16 = if index + 1 >= count {
        -1
    } else {
        (index + 1) as i16
    };
    let link2: i16 = if index == 0 { -1 } else { (index - 1) as i16 };
    out.extend_from_slice(&link1.to_le_bytes());
    out.extend_from_slice(&link2.to_le_bytes());
    assert_eq!(
        out.len(),
        44,
        "a material slot is forty bytes and two words"
    );
    out
}

/// One zero material slot, of which a container stores `1000 - count`.
fn zero_material_slot(index: u32, count: u32) -> Vec<u8> {
    let mut out = vec![0u8; 40];
    let link1: i16 = if index == count {
        -1
    } else {
        (index - 1) as i16
    };
    let link2: i16 = if index + 1 >= NG_MATERIAL_SLOTS {
        -1
    } else {
        (index + 1) as i16
    };
    out.extend_from_slice(&link1.to_le_bytes());
    out.extend_from_slice(&link2.to_le_bytes());
    out
}

/// `count` distinct `Vec3`s from one named block.
fn block(base: f32, count: usize) -> Vec<[f32; 3]> {
    (0..count)
        .map(|index| {
            let value = (index + 1) as f32;
            [base + value, base + value * 2.0, base + value * 3.0]
        })
        .collect()
}

/// The seam quad as stored bytes: positions, normals, the polygon records,
/// each polygon's corner arrays, then the mesh-level material references.
/// The mesh-level list is `[0, 1]` so the audit sees a stored reference to
/// both materials — the one the world's archive serves and the one only
/// `rimage.zbd` holds.
fn mesh_data(positions: &[[f32; 3]], normals: &[[f32; 3]], polygons: &[&[u32]]) -> Vec<u8> {
    let mut out = Vec::new();
    for vector in positions.iter().chain(normals) {
        for channel in vector {
            out.extend_from_slice(&channel.to_le_bytes());
        }
    }
    for polygon in polygons {
        // The ten words of the polygon record: the packed corner count plus
        // the normals flag, the four pointers and the material-group count.
        let vertex_info = (polygon.len() as u32 & CORNER_COUNT_MASK) | (FLAG_NORMALS << FLAG_SHIFT);
        for word in [
            vertex_info,
            0,
            0xAAAA_0000,
            0xAAAA_1000,
            1, // one stored material group
            0xAAAA_2000,
            0xAAAA_3000,
            0xAAAA_4000,
            0xAAAA_5000,
            0xAAAA_6000,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
    }
    for polygon in polygons {
        for index in polygon.iter() {
            out.extend_from_slice(&index.to_le_bytes()); // position indices
        }
        for (corner, _) in polygon.iter().enumerate() {
            out.extend_from_slice(&(corner as u32).to_le_bytes()); // normal indices
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // the group's material index
        for uv in [[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]] {
            out.extend_from_slice(&uv[0].to_le_bytes());
            out.extend_from_slice(&uv[1].to_le_bytes());
        }
        for (corner, _) in polygon.iter().enumerate() {
            let value = 16.0 * (corner + 1) as f32;
            for channel in [value, value * 2.0, value * 3.0] {
                out.extend_from_slice(&channel.to_le_bytes());
            }
        }
    }
    // The mesh-level material references: material 0 and material 1.
    for material in [0u32, 1] {
        for word in [material, 1, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
    }
    out
}

/// A whole CS GameZ container with one stored mesh and `nodes` Object3d
/// nodes, laid out the way the mesh and node readers walk it: the 40-byte
/// header, the texture-name table, the 16-byte material info plus the 1000
/// fixed slots, the mesh index and the mesh data ending exactly at
/// `nodes_offset`, then the node array and its records to the file's end.
///
/// The container's texture names are `sky` and `hud`; material 0 names `sky`,
/// material 1 names `hud`, and the mesh's stored material list references
/// both. No suffix is authored — the audit compares the stored name exactly.
fn world_container() -> Vec<u8> {
    let names = ["sky", "hud"];
    let texture_of = [0u32, 1];
    let positions = block(0.0, 4);
    let normals = block(1000.0, 4);
    let polygons: [&[u32]; 2] = [&[0, 1, 2], &[2, 1, 3]];
    let data = mesh_data(&positions, &normals, &polygons);

    let textures_offset = 40usize;
    let materials_offset = textures_offset + names.len() * 44;
    let material_section = 16 + NG_MATERIAL_SLOTS as usize * 44;
    let meshes_offset = materials_offset + material_section;
    let record_bytes = 104usize;
    let nodes_offset = meshes_offset + 12 + record_bytes + data.len();
    let node_count = 1u32;
    let nodes_data = nodes_offset + NODE_SLOT_BYTES as usize * node_count as usize;

    let mut out = Vec::new();
    for word in [
        GAMEZ_SIGNATURE,
        GAMEZ_VERSION,
        1_234_567_890, // unk08: neither measured fixup table
        names.len() as u32,
        textures_offset as u32,
        materials_offset as u32,
        meshes_offset as u32,
        node_count,
        0, // light_index
        nodes_offset as u32,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    assert_eq!(
        out.len(),
        textures_offset,
        "the header is ten 4-byte fields"
    );

    for name in names {
        out.extend_from_slice(&texture_name(name));
    }
    let materials = texture_of.len() as u32;
    for word in [
        materials as i32,
        materials as i32,
        materials as i32,
        materials as i32 - 1,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    for (index, texture_index) in texture_of.iter().enumerate() {
        out.extend_from_slice(&material_slot(index as u32, materials, *texture_index));
    }
    for index in materials..NG_MATERIAL_SLOTS {
        out.extend_from_slice(&zero_material_slot(index, materials));
    }
    assert_eq!(
        out.len(),
        meshes_offset,
        "the material section ends at the mesh index"
    );

    // The mesh index: one present record whose trailing word is the data
    // offset just past the record array.
    for word in [1i32, 1, -1] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let mesh_offset = (meshes_offset + 12 + record_bytes) as u32;
    for word in [
        1u32,                   // file_ptr
        0,                      // unk04
        0,                      // unk08
        1,                      // parent_count: non-zero marks present
        polygons.len() as u32,  // polygon_count
        positions.len() as u32, // vertex_count
        normals.len() as u32,   // normal_count
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        2, // material_count: the mesh-level references
        0,
        mesh_offset,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&data);
    assert_eq!(
        out.len(),
        nodes_offset,
        "the mesh data ends at nodes_offset"
    );

    // One Object3d node naming the mesh, with the identity record the layout
    // asserts for `flags == 40`.
    out.resize(nodes_data + OBJECT3D_DATA_BYTES as usize, 0);
    let word = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let half = |bytes: &mut Vec<u8>, at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, at: usize, value: f32| {
        bytes[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
    };
    let slot = nodes_offset;
    out[slot..slot + 6].copy_from_slice(b"object");
    word(&mut out, slot + 44, 1);
    word(&mut out, slot + 52, 5); // node_type: Object3d
    word(&mut out, slot + 56, nodes_data as u32); // data_ptr
    word(&mut out, slot + 60, 0); // mesh_index
    word(&mut out, slot + 68, 1); // action_priority
    half(&mut out, slot + 84, 0); // parent_count
    half(&mut out, slot + 86, 0); // children_count
    word(&mut out, slot + 196, 160); // unk196
    word(&mut out, slot + 208, NODE_INDEX_TOP); // node_index
    let record = nodes_data;
    word(&mut out, record, OBJECT3D_FLAGS_IDENTITY);
    for axis in 0..3 {
        float(&mut out, record + 36 + 4 * axis, 1.0); // scale
        float(&mut out, record + 48 + 16 * axis, 1.0); // matrix diagonal
    }
    out
}

// --------------------------------------------------------- the installation --

static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

/// A disposable synthetic installation under the temporary directory, dropped
/// with the test. No original game data is ever written here.
struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f08-c-renderer-{}-{}",
            std::process::id(),
            NEXT_TREE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture root is created");
        Self(root)
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.0.join(spelling);
        fs::create_dir_all(path.parent().expect("a parent")).expect("fixture dirs");
        fs::write(path, bytes).expect("fixture bytes are written");
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The group these tests load, and the installation around it:
///
/// ```text
/// zbd/c1/gamez.zbd        the world container — materials naming `sky` and `hud`
/// zbd/c1/texture.zbd      the unnumbered world archive, holding `sky`
/// zbd/c1/texture6.zbd     the 6 MiB tier
/// zbd/c1/texture8.zbd     the 8 MiB tier
/// zbd/c1/rtexture16.zbd   the reduced-format 16 MiB tier
/// zbd/rimage.zbd          the shared image archive, holding `hud`
/// ```
///
/// A world group is any direct child of `zbd`, so this tree is a whole
/// installation to production discovery. Every package stores `sky`, so the
/// tiers are told apart by *which file* the rule opened, never by content —
/// and `hud` exists only in `rimage.zbd`, so a lookup that reaches it went
/// through the measured fallthrough and nowhere else.
fn install() -> Tree {
    let tree = Tree::new();
    tree.write("zbd/c1/gamez.zbd", &world_container());
    for tier in [
        "texture.zbd",
        "texture6.zbd",
        "texture8.zbd",
        "rtexture16.zbd",
    ] {
        tree.write(&format!("zbd/c1/{tier}"), &texture_package(&["sky"]));
    }
    tree.write("zbd/rimage.zbd", &texture_package(&["hud"]));
    tree
}

// ---------------------------------------------------------------- the tests --

/// **F08-C AC: a world load opens the archive the rule selects.** Under the
/// project's renderer — hardware with the designed 16 MiB device total — the
/// measured walk probes `rtexture16.zbd` first and opens it, not the
/// `texture.zbd` a guessed name would open. The mesh catalog's archive is the
/// same choice, and a name the archive does not hold resolves through the
/// shared image list — the production lookup order.
#[test]
fn accept_f08_c_renderer_a_world_load_opens_the_archive_the_rule_selects() {
    let tree = install();
    let settings = DetailSettings::designed();
    let load = RendererDetail::project().world_load(&settings);
    let found = read_world_containers(&tree.0).expect("the installation discovers");
    let container = found
        .container("c1", &load)
        .expect("the world container reads");

    let choice = container.texture_archive();
    assert_eq!(choice.budget().mib, PROJECT_HARDWARE_TEXTURE_MIB);
    assert!(
        choice.budget().reduced_prefix_first,
        "a device total sets the r-flag"
    );
    assert_eq!(choice.opened_name(), Some("rtexture16.zbd"));
    assert_eq!(
        choice.probes().len(),
        1,
        "the first existing candidate wins: {:?}",
        choice.probes()
    );
    let archive = choice
        .opened()
        .expect("an archive was opened")
        .key()
        .clone();
    assert_eq!(
        archive.to_string(),
        "world/default/rtexture16.zbd",
        "the selected archive resolves in the world's own directory"
    );
    assert_eq!(
        container.mesh_catalog().archive(),
        &archive,
        "the mesh audit searched the selected archive"
    );

    // The production lookup order, end to end: `sky` resolves from the
    // world's archive, `hud` — which no world tier holds — is followed to the
    // shared `rimage.zbd` and resolves there.
    let mesh = container
        .mesh_catalog()
        .containers()
        .next()
        .expect("the mesh container opened");
    let audit = mesh.audit();
    assert_eq!(audit.resolved, 2, "both stored names resolve");
    let MaterialState::Resolved { texture } = &audit.rows[0].state else {
        panic!("material 0 resolves: {:?}", audit.rows[0].state);
    };
    assert!(
        texture.archive.as_str().ends_with("c1/rtexture16.zbd"),
        "sky came from {texture}, not the selected world archive"
    );
    let MaterialState::Resolved { texture } = &audit.rows[1].state else {
        panic!("material 1 resolves: {:?}", audit.rows[1].state);
    };
    assert_eq!(
        texture.archive.as_str(),
        "zbd/rimage.zbd",
        "hud came from the shared image list"
    );
    assert_eq!(
        audit.rows[1].dependencies,
        vec![archive.clone()],
        "the row's dependency stays the world's archive"
    );
}

/// **F08-C AC: the dropdown moves the archive where the measurement says it
/// can — and provably does not where it cannot.** Under the software renderer
/// the budget reads `TextureMemory_SW`, so picking a row changes the file the
/// world load opens; under hardware with no device the same is true of
/// `TextureMemory_HW`. Under a hardware device total the descriptor ignores
/// both members, so no pick changes anything.
#[test]
fn accept_f08_c_renderer_the_dropdown_moves_the_archive_only_where_it_can() {
    let tree = install();
    let found = read_world_containers(&tree.0).expect("the installation discovers");
    let mut settings = DetailSettings::designed();

    // The software renderer reads TextureMemory_SW: Low writes 6 MiB, Middle
    // 8 MiB, High the unnumbered maximum — and each pick selects the
    // measured tier.
    for (row, expected) in [
        (TextureDetailRow::Low, "texture6.zbd"),
        (TextureDetailRow::Middle, "texture8.zbd"),
        (TextureDetailRow::High, "texture.zbd"),
    ] {
        settings.select_texture_row(DetailDevice::Software, row);
        let load = RendererDetail::software().world_load(&settings);
        let container = found
            .container("c1", &load)
            .expect("the world container reads");
        assert_eq!(
            container.texture_archive().opened_name(),
            Some(expected),
            "{row:?} under software"
        );
    }

    // The no-DirectDraw hardware renderer reads TextureMemory_HW: the row
    // applied under the *hardware* device moves it, and a software pick does
    // not.
    settings.select_texture_row(DetailDevice::Hardware, TextureDetailRow::Middle);
    let load = RendererDetail::hardware(None).world_load(&settings);
    let container = found
        .container("c1", &load)
        .expect("the world container reads");
    assert_eq!(
        container.texture_archive().opened_name(),
        Some("texture8.zbd"),
        "the hardware member answers for a hardware renderer with no device"
    );
    settings.select_texture_row(DetailDevice::Software, TextureDetailRow::Low);
    let load = RendererDetail::hardware(None).world_load(&settings);
    let container = found
        .container("c1", &load)
        .expect("the world container reads");
    assert_eq!(
        container.texture_archive().opened_name(),
        Some("texture8.zbd"),
        "a software pick never moves the hardware member"
    );

    // A hardware device total outranks the settings: every row leaves the
    // load on the tier the device reached.
    for device in DetailDevice::ALL {
        for row in TextureDetailRow::ALL {
            settings.select_texture_row(device, row);
        }
    }
    let load = RendererDetail::project().world_load(&settings);
    let container = found
        .container("c1", &load)
        .expect("the world container reads");
    assert_eq!(
        container.texture_archive().opened_name(),
        Some("rtexture16.zbd"),
        "the device total decides alone"
    );
}

/// **F08-C AC, retail.** On the original installation the same rule picks
/// `rtexture15.zbd` for `C1` — the highest tier the world lists below the
/// designed 16 MiB device total — and the software renderer's 8 MiB pick
/// falls through every numbered name to the unnumbered `texture.zbd`, because
/// the world lists no plain `textureN.zbd` at all.
///
/// Run with `cargo test -p cs_app --test world -- accept_f08_c_renderer_
/// --include-ignored` and `CS_GAME_DIR` set to the original installation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f08_c_renderer_retail_opens_the_archive_the_rule_selects() {
    let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail tests");
    let found = read_world_containers(std::path::Path::new(&root))
        .expect("the retail installation discovers");

    let hardware = found
        .container(
            "C1",
            &RendererDetail::project().world_load(&DetailSettings::designed()),
        )
        .expect("the c1 container reads");
    let choice = hardware.texture_archive();
    assert_eq!(choice.budget().mib, PROJECT_HARDWARE_TEXTURE_MIB);
    assert!(choice.budget().reduced_prefix_first);
    assert_eq!(
        choice.opened_name(),
        Some("rtexture15.zbd"),
        "the measured retail answer for C1 under a 16 MiB device"
    );

    let mut settings = DetailSettings::designed();
    settings.select_texture_row(DetailDevice::Software, TextureDetailRow::Middle);
    let software = found
        .container("C1", &RendererDetail::software().world_load(&settings))
        .expect("the c1 container reads under software");
    assert_eq!(
        software.texture_archive().opened_name(),
        Some("texture.zbd"),
        "software falls through the numbered names to the unnumbered archive"
    );
}

/// The convenience form takes the same descriptor: one discovery, one read,
/// same archive.
#[test]
fn accept_f08_c_renderer_read_world_container_takes_the_same_load() {
    let tree = install();
    let load = RendererDetail::project().world_load(&DetailSettings::designed());
    let container = read_world_container(&tree.0, "c1", &load).expect("the world container reads");
    assert_eq!(
        container.texture_archive().opened_name(),
        Some("rtexture16.zbd")
    );
}
