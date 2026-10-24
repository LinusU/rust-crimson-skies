//! F49-D.03-FU-RULES-ROSTER (#1639): which of the declared aircraft the
//! original's Instant Action spawn routine actually seats per mission type,
//! and the wingmen's plane.
//!
//! The measurement (`docs/findings/2026-10-24-f49-d-03-fu-rules-roster.md`) is
//! the spawn routine `0x45a390` and the scenario-record reader `0x459390`. The
//! engine-image tests re-read the bytes at the named virtual addresses
//! independently of the production code; the retail tests rebuild the expected
//! roster from the installation's descriptors, `vehicle.zrd` and the image's
//! airframe table and compare it to the catalog the production code assembles.
//!
//! Required capabilities: **retail** (`$CS_GAME_DIR`) and `$CS_ENGINE_IMAGE`,
//! read-only. Nothing is run, rendered or played, so the claims stay
//! `implemented`, never `verified_original`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use cs_app::mission_start::AIRFRAME_TABLE;
use cs_app::ui::instant_action::{
    IA_WINGMAN_BASE_ROLE, IA_WINGMAN_FOLLOWER_ROLE, lower_preset_roster,
    retail_instant_action_catalog,
};
use cs_content::instant_action::{
    IA_ENGINE_DEFAULT_ACE_PLANE_INDEX, IA_ENGINE_DEFAULT_GROUP_PLANE_INDEX,
    IA_ENGINE_DEFAULT_WINGMAN_PLANE_INDEX, IA_MISSION_TYPE_VOCABULARY, IA_SPAWN_ROSTER_CLAIM,
    IaPlayerPlane, IaWingmanPlane, IaWingmanTemplate, IaWingmen, ia_spawns_named_ace,
    ia_wingman_template, survey_retail_instant_action,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::content::{ContentId, ContentKind};

const FINDING: &str = "docs/findings/2026-10-24-f49-d-03-fu-rules-roster.md";

// ------------------------------------------------------- engine image --------

/// The image's bytes at virtual address `va`, through its PE section table.
fn image_bytes(image: &[u8], va: u32, len: usize) -> &[u8] {
    let u16_at = |at: usize| u16::from_le_bytes(image[at..at + 2].try_into().expect("2 bytes"));
    let u32_at = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().expect("4 bytes"));
    let pe = u32_at(0x3c) as usize;
    assert_eq!(&image[pe..pe + 4], b"PE\0\0", "a PE image");
    let sections = usize::from(u16_at(pe + 6));
    let optional = usize::from(u16_at(pe + 20));
    let image_base = u32_at(pe + 24 + 28);
    let table = pe + 24 + optional;
    for index in 0..sections {
        let entry = table + index * 40;
        let virtual_size = u32_at(entry + 8);
        let section_va = image_base + u32_at(entry + 12);
        let raw_size = u32_at(entry + 16);
        let raw = u32_at(entry + 20) as usize;
        if va >= section_va && va - section_va + len as u32 <= virtual_size.min(raw_size) {
            let start = raw + (va - section_va) as usize;
            return &image[start..start + len];
        }
    }
    panic!("virtual address {va:#x} is in no section");
}

fn image_u32(image: &[u8], va: u32) -> u32 {
    u32::from_le_bytes(
        image_bytes(image, va, 4)
            .try_into()
            .expect("a dword is 4 bytes"),
    )
}

/// The NUL-terminated printable-ASCII text at `va`.
fn c_string(image: &[u8], va: u32) -> String {
    let mut text = String::new();
    for offset in 0..64 {
        match image_bytes(image, va + offset, 1)[0] {
            0 => return text,
            byte @ 0x20..=0x7e => text.push(char::from(byte)),
            byte => panic!("non-text byte {byte:#x} at {:#x}", va + offset),
        }
    }
    panic!("unterminated text at {va:#x}");
}

fn engine_image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    fs::read(path).expect("the engine image reads")
}

fn assert_bytes(image: &[u8], va: u32, want: &[u8], what: &str) {
    assert_eq!(
        image_bytes(image, va, want.len()),
        want,
        "{what} at {va:#x}"
    );
}

/// The target of the `call rel32` (`e8`) or near `jcc rel32` (`0f 8x`)
/// instruction at `va`.
fn branch_target(image: &[u8], va: u32) -> u32 {
    let (opcode_len, total) = match image_bytes(image, va, 1)[0] {
        0xe8 | 0xe9 => (1, 5),
        0x0f => (2, 6),
        other => panic!("{other:#x} at {va:#x} is not a near branch"),
    };
    let rel = image_u32(image, va + opcode_len) as i32;
    va.wrapping_add(total).wrapping_add(rel as u32)
}

/// One row of the airframe table at `0x620c70`: its seven string pointers.
struct ImageRow {
    display: String,
    wingman: String,
    base: String,
}

fn image_rows(image: &[u8]) -> Vec<ImageRow> {
    (0..11_u32)
        .map(|index| {
            let row = 0x0062_0c70 + index * 28;
            let text = |field: u32| c_string(image, image_u32(image, row + field * 4));
            ImageRow {
                display: text(0),
                wingman: text(4),
                base: text(6),
            }
        })
        .collect()
}

/// The register the wingman lookup name is moved into per mission type: the
/// jump table `0x45b9b0` selects one of five cases, and each case starts
/// `push imm32; mov ebx, edi|esi`. `edi` holds the airframe row's `w` field
/// (accessor `0x426d60`), `esi` its `base` field (accessor `0x426d20`).
fn lookup_register(image: &[u8], type_index: u32) -> &'static str {
    let case = image_u32(image, 0x0045_b9b0 + 4 * type_index);
    match image_bytes(image, case + 5, 2) {
        [0x8b, 0xdf] => "edi",
        [0x8b, 0xde] => "esi",
        other => panic!("unexpected lookup move {other:x?} at case {case:#x}"),
    }
}

/// `ia.zrd` `mission_type` name → the engine's type index (`0x458c20`).
fn type_index(name: &str) -> u32 {
    match name {
        "dogfight_ace" => 0,
        "dogfight_squadron" => 1,
        "zeppelin_run" => 2,
        "ground_target" => 3,
        "stunt_flying" => 4,
        other => panic!("{other} is not an Instant Action mission type"),
    }
}

/// The spawn routine's wingman lookup is the airframe row's `w` record for
/// types 0, 2 and 4 and its `base` record for types 1 and 3.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_roster_image_the_wingman_lookup_name_depends_on_the_type() {
    let image = engine_image();

    // The two accessors: `i*7` rows of 28 bytes, field 4 (`w`, +0x10) and
    // field 6 (`base`, +0x18) of the table at 0x620c70.
    assert_bytes(
        &image,
        0x0042_6d6d,
        &[0x8b, 0x04, 0x8d, 0x80, 0x0c, 0x62, 0x00],
        "0x426d60: mov eax, [ecx*4 + 0x620c80] — the w field",
    );
    assert_bytes(
        &image,
        0x0042_6d2d,
        &[0x8b, 0x04, 0x8d, 0x88, 0x0c, 0x62, 0x00],
        "0x426d20: mov eax, [ecx*4 + 0x620c88] — the base field",
    );

    // The routine looks the record's `wingman_plane` (`[0x718ce4]`) up through
    // both accessors, before the type switch: w → edi, base → esi.
    assert_bytes(
        &image,
        0x0045_a4b9,
        &[0xa1, 0xe4, 0x8c, 0x71, 0x00],
        "mov eax, [0x718ce4]: the wingman plane row",
    );
    assert_eq!(branch_target(&image, 0x0045_a4cd), 0x0042_6d60);
    assert_bytes(
        &image,
        0x0045_a4d9,
        &[0x8b, 0xf8],
        "mov edi, eax: the w name",
    );
    assert_eq!(branch_target(&image, 0x0045_a4db), 0x0042_6d20);
    assert_bytes(
        &image,
        0x0045_a4e0,
        &[0x8b, 0xf0],
        "mov esi, eax: the base name",
    );
    assert_bytes(
        &image,
        0x0045_a4f2,
        &[0x8b, 0xd0, 0xff, 0x24, 0x95, 0xb0, 0xb9, 0x45, 0x00],
        "mov edx, eax; jmp [edx*4 + 0x45b9b0]: the type switch",
    );

    let registers: Vec<_> = (0..5).map(|index| lookup_register(&image, index)).collect();
    assert_eq!(registers, ["edi", "esi", "edi", "esi", "edi"]);

    // The production binding says the same for every name of the vocabulary.
    for name in IA_MISSION_TYPE_VOCABULARY {
        let template = ia_wingman_template(name).expect("a vocabulary name has a template");
        let register = lookup_register(&image, type_index(name));
        assert_eq!(
            template,
            if register == "edi" {
                IaWingmanTemplate::Follower
            } else {
                IaWingmanTemplate::Base
            },
            "{name}: the production template is the engine's lookup register {register}"
        );
    }
    assert_eq!(ia_wingman_template("not_a_type"), None);

    // The rows those names come from: row 5 is `wingman` / `devastator`.
    let rows = image_rows(&image);
    assert_eq!(rows[5].display, "Devastator");
    assert_eq!(
        (rows[5].wingman.as_str(), rows[5].base.as_str()),
        ("wingman", "devastator")
    );
}

/// The named ace is seated for type 0 (`dogfight_ace`) only.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_roster_image_the_ace_is_seated_for_type_zero_only() {
    let image = engine_image();
    assert_bytes(
        &image,
        0x0045_aa24,
        &[0xa1, 0xd8, 0x8c, 0x71, 0x00, 0x85, 0xc0, 0x0f, 0x85],
        "mov eax, [0x718cd8]; test eax, eax; jne: any type but 0 skips the ace block",
    );
    assert_eq!(
        branch_target(&image, 0x0045_aa2b),
        0x0045_ad18,
        "the jne lands past the ace block, on the group loop's setup"
    );
    // The ace's lookup name is the `base` field (accessor 0x426d20) of the
    // record's `ace_plane` row (`[0x718d3c]`).
    assert_bytes(
        &image,
        0x0045_aa4d,
        &[0xa1, 0x3c, 0x8d, 0x71, 0x00],
        "mov eax, [0x718d3c]: the ace plane row",
    );
    assert_eq!(branch_target(&image, 0x0045_aa53), 0x0042_6d20);

    for name in IA_MISSION_TYPE_VOCABULARY {
        assert_eq!(
            ia_spawns_named_ace(name),
            Some(type_index(name) == 0),
            "{name}"
        );
    }
    assert_eq!(ia_spawns_named_ace("not_a_type"), None);
}

/// The enemy-group loop has no per-type gate on *which* groups spawn: four
/// records, each skipped only for a zero count or a lookup miss; the first
/// seated group takes a spawn point (and the wave counter), every other group
/// the default placement, and `zeppelin_run` places all of them by default.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_roster_image_the_group_loop_seats_every_group_whatever_the_type() {
    let image = engine_image();

    // The only type test before the loop: with fewer than two spawn points
    // for the type, every type but 2 skips the groups (0x45b17c). Every
    // offered retail type has four or eight points (checked below).
    assert_bytes(
        &image,
        0x0045_ad36,
        &[0x83, 0x3d, 0xd8, 0x8c, 0x71, 0x00, 0x02, 0x0f, 0x85],
        "cmp [0x718cd8], 2; jne",
    );
    assert_eq!(branch_target(&image, 0x0045_ad3d), 0x0045_b17c);

    // Four group records, 0x60 bytes apart, starting at 0x718dcc.
    assert_bytes(
        &image,
        0x0045_ad94,
        &[0xbd, 0xcc, 0x8d, 0x71, 0x00],
        "mov ebp, 0x718dcc: the first group record + 0x20",
    );
    assert_bytes(&image, 0x0045_b16d, &[0x83, 0xc5, 0x60], "add ebp, 0x60");
    assert_bytes(
        &image,
        0x0045_b170,
        &[0x81, 0xfd, 0x4c, 0x8f, 0x71, 0x00],
        "cmp ebp, 0x718f4c: the end of the fourth record",
    );
    assert_eq!((0x0071_8f4c - 0x0071_8dcc) / 0x60, 4);

    // A zero count skips the group (and the placed-group counter increment).
    assert_bytes(
        &image,
        0x0045_ad99,
        &[0x8b, 0x45, 0xe0],
        "mov eax, [ebp-0x20]: the group's count",
    );
    assert_bytes(
        &image,
        0x0045_ad9e,
        &[0x0f, 0x84],
        "je: a zero count leaves the group",
    );
    assert_eq!(branch_target(&image, 0x0045_ad9e), 0x0045_b16d);
    assert_bytes(
        &image,
        0x0045_ad8c,
        &[0xc7, 0x44, 0x24, 0x10, 0x01, 0, 0, 0],
        "the placed-group counter starts at 1",
    );
    assert_bytes(
        &image,
        0x0045_b169,
        &[0xff, 0x44, 0x24, 0x10],
        "inc the counter once per seated group",
    );

    // The lookup name is the `base` field of the group's `enemy_plane` row.
    assert_bytes(
        &image,
        0x0045_ada4,
        &[0x8b, 0x55, 0xf4, 0x52],
        "mov edx, [ebp-0xc]; push edx: the group's plane row",
    );
    assert_eq!(branch_target(&image, 0x0045_ada8), 0x0042_6d20);

    // Placement: type 2, or any group but the first seated, takes the default
    // placement; the first seated group of the other types takes a spawn point.
    assert_bytes(
        &image,
        0x0045_b001,
        &[0x83, 0x3d, 0xd8, 0x8c, 0x71, 0x00, 0x02],
        "cmp [0x718cd8], 2",
    );
    assert_eq!(branch_target(&image, 0x0045_b018), 0x0045_b0d3);
    assert_bytes(
        &image,
        0x0045_b01e,
        &[0x39, 0x7c, 0x24, 0x10],
        "cmp [esp+0x10], edi (1): the first seated group",
    );
    assert_eq!(branch_target(&image, 0x0045_b022), 0x0045_b0d3);
    assert_bytes(
        &image,
        0x0045_b0cb,
        &[0x89, 0x3d, 0xd0, 0x8c, 0x71, 0x00],
        "mov [0x718cd0], edi: the wave counter, set when a group takes a spawn point",
    );
}

/// The record initialisers and the readers' fall-backs: where a descriptor
/// that leaves a key out lands, and `ZBD/c2b/IA1` re-verified.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_roster_image_the_record_defaults_and_the_wingman_plane_key() {
    let image = engine_image();

    // The scenario record initialiser 0x458ff0 (esi = 5).
    assert_bytes(
        &image,
        0x0045_9001,
        &[0xbe, 5, 0, 0, 0],
        "mov esi, 5: the default row",
    );
    assert_bytes(
        &image,
        0x0045_9008,
        &[0x89, 0x75, 0x04],
        "[+4] = 5 player_plane",
    );
    assert_bytes(
        &image,
        0x0045_900b,
        &[0xc7, 0x45, 0x08, 0, 0, 0, 0],
        "[+8] = 0 num_wingmen",
    );
    assert_bytes(
        &image,
        0x0045_9012,
        &[0x89, 0x75, 0x0c],
        "[+0xc] = 5 wingman_plane",
    );
    assert_bytes(
        &image,
        0x0045_9082,
        &[0x89, 0x75, 0x64],
        "[+0x64] = 5 ace_plane",
    );
    assert_eq!(IA_ENGINE_DEFAULT_WINGMAN_PLANE_INDEX, 5);
    assert_eq!(IA_ENGINE_DEFAULT_ACE_PLANE_INDEX, 5);
    // The group initialiser 0x458d00: enemy_plane defaults to row 6.
    assert_bytes(
        &image,
        0x0045_8dd6,
        &[0xc7, 0x45, 0x14, 6, 0, 0, 0],
        "[+0x14] = 6 enemy_plane",
    );
    assert_eq!(IA_ENGINE_DEFAULT_GROUP_PLANE_INDEX, 6);

    // The `wingman_plane` key (0x625588) and its store: a present name
    // resolves through the name-to-index routine 0x426d80, stored at [+0xc];
    // the sentinel 11 becomes the default 5; an absent key skips the store.
    assert_bytes(
        &image,
        0x0045_943f,
        &[0x68, 0x88, 0x55, 0x62, 0x00],
        "push 0x625588",
    );
    assert_eq!(c_string(&image, 0x0062_5588), "wingman_plane");
    assert_eq!(branch_target(&image, 0x0045_9458), 0x0042_6d80);
    assert_bytes(&image, 0x0045_9460, &[0x83, 0xf8, 0x0b], "cmp eax, 0xb");
    assert_bytes(
        &image,
        0x0045_9463,
        &[0x89, 0x43, 0x0c],
        "mov [ebx+0xc], eax",
    );
    assert_bytes(
        &image,
        0x0045_9468,
        &[0x89, 0x7b, 0x0c],
        "mov [ebx+0xc], edi (5): an unknown name",
    );
    assert_bytes(
        &image,
        0x0045_944d,
        &[0x85, 0xc0, 0x74, 0x1a],
        "test eax, eax; je 0x45946b: an absent key skips the store",
    );
    // `player_plane` skips the same way (0x4593f3), which is what leaves
    // ZBD/c2b/IA1 on row 5 and no wingmen.
    assert_bytes(
        &image,
        0x0045_93f3,
        &[0x85, 0xc0, 0x74, 0x1a],
        "test eax, eax; je 0x459411",
    );
    assert_bytes(
        &image,
        0x0045_9421,
        &[0x74, 0x1c],
        "num_wingmen: je 0x45943f when absent",
    );
    assert_eq!(
        0x0045_9421 + 2 + 0x1c,
        0x0045_943f,
        "the absent num_wingmen skips to the wingman_plane read"
    );

    // The production reading of c2b agrees with the image.
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let survey = survey_retail_instant_action(&root).expect("the survey reads");
    let c2b = survey
        .directories()
        .iter()
        .find(|directory| directory.container() == "zbd/c2b/ia1/zrdr.zbd")
        .expect("c2b/IA1 is surveyed");
    assert_eq!(
        c2b.player_plane(),
        IaPlayerPlane::EngineDefault { index: 5 }
    );
    assert_eq!(c2b.wingmen(), IaWingmen::EngineDefault(0));
    assert_eq!(
        c2b.wingman_plane(),
        IaWingmanPlane::EngineDefault { index: 5 }
    );
}

// ------------------------------------------------------------ retail ----------

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// Every record of the installation's `ZBD/zrdr.zbd#vehicle.zrd` by name, with
/// its `mode` (the registry the spawn routine's lookups search, `[0x71daac]`).
fn vehicle_registry(root: &std::path::Path) -> BTreeMap<String, Option<String>> {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == "zbd/zrdr.zbd")
        .expect("the installation has ZBD/zrdr.zbd");
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = cs_types::install::RelativePath::new(&spelling.to_lowercase()).expect("a path");
    let bytes = fs::read(found.manifest.host_root.join(&spelling)).expect("readable");
    let discovery = cs_formats::script_raw::discover_container("zbd/zrdr.zbd", &path, &bytes);
    let program = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case("vehicle.zrd"))
        })
        .expect("ZBD/zrdr.zbd declares vehicle.zrd");
    let document: ZrdValue = decode_zrd(program.bytes()).expect("vehicle.zrd decodes");
    let mut registry = BTreeMap::new();
    for record in document.as_list().unwrap_or_default() {
        for pair in record.as_list().unwrap_or_default().chunks(2) {
            let [name, fields] = pair else { continue };
            let Some(name) = name.as_text() else { continue };
            let mode = fields.as_list().and_then(|fields| {
                fields.chunks(2).find_map(|field| {
                    let [key, value] = field else { return None };
                    (key.as_text() == Some("mode"))
                        .then(|| value.as_list()?.first()?.as_text().map(str::to_owned))?
                })
            });
            registry.insert(name.to_owned(), mode);
        }
    }
    registry
}

fn table_airframe(display_name: &str) -> ContentId {
    let row = AIRFRAME_TABLE
        .iter()
        .find(|row| row.display_name.eq_ignore_ascii_case(display_name))
        .unwrap_or_else(|| panic!("`{display_name}` is a row of the airframe table"));
    ContentId::from_source(ContentKind::Airframe, row.scene_root)
        .expect("a table scene root is a valid airframe key")
}

/// No retail descriptor overrides `wingman_plane`, so every wingman flies the
/// initialiser's row 5 — and the registry the spawn routine searches holds the
/// `w` and `base` record of every row, so no lookup misses.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_rules_roster_retail_no_descriptor_overrides_the_wingman_plane() {
    let root = game_dir();
    let survey = survey_retail_instant_action(&root).expect("the survey reads");
    assert_eq!(survey.directories().len(), 8);
    for directory in survey.directories() {
        assert_eq!(
            directory.wingman_plane(),
            IaWingmanPlane::EngineDefault {
                index: IA_ENGINE_DEFAULT_WINGMAN_PLANE_INDEX
            },
            "{} declares no wingman_plane",
            directory.container()
        );
    }

    // An independent byte search: no `ia.zrd` member carries the key text.
    let found = cs_assets::install::discover(&root).expect("the installation discovers");
    for directory in survey.directories() {
        let record = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == directory.container())
            .expect("the surveyed container is in the manifest");
        let bytes = fs::read(
            found
                .manifest
                .host_root
                .join(record.relative_spelling.as_str()),
        )
        .expect("the container reads");
        let (offset, length) = directory.member_span();
        let member = &bytes[offset as usize..(offset + length) as usize];
        assert!(
            !member
                .windows(b"wingman_plane".len())
                .any(|window| window == b"wingman_plane"),
            "{}: the key text is absent from the ia.zrd member",
            directory.container()
        );
    }

    let registry = vehicle_registry(&root);
    let image = engine_image();
    for row in image_rows(&image) {
        assert_eq!(
            registry.get(&row.wingman),
            Some(&Some("wingman".to_owned())),
            "{}: the w record {} runs the wingman follower",
            row.display,
            row.wingman
        );
        let base = registry
            .get(&row.base)
            .unwrap_or_else(|| panic!("{}: the registry holds {}", row.display, row.base));
        assert_ne!(
            base.as_deref(),
            Some("wingman"),
            "{}: the base record {} is a plain aircraft",
            row.display,
            row.base
        );
    }
}

/// Every retail preset's roster is the roster the spawn routine seats: the
/// declared player, the wingmen on row 5 under the type's template, every
/// declared group, and the named ace for `dogfight_ace` only.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_rules_roster_retail_each_preset_seats_what_the_spawn_routine_seats() {
    let root = game_dir();
    let image = engine_image();
    let survey = survey_retail_instant_action(&root).expect("the survey reads");
    let catalog = retail_instant_action_catalog(&root).expect("the retail catalog assembles");
    let registry = vehicle_registry(&root);
    let rows = image_rows(&image);
    let row_of = |display: &str| {
        rows.iter()
            .find(|row| row.display.eq_ignore_ascii_case(display))
            .unwrap_or_else(|| panic!("`{display}` is a row of the image's table"))
    };

    let mut checked = 0usize;
    let mut aces = 0usize;
    for directory in survey.directories() {
        let scenario = directory.scenario_id().expect("a scenario id");
        let aircraft = directory.aircraft();
        for offered in directory.offered() {
            // The groups and the ace can only spawn at a spawn point.
            assert!(
                offered.spawn_points() >= 2,
                "{} {}: two spawn points seat the ace and groups",
                directory.container(),
                offered.mission_type()
            );
        }
        for preset in catalog.presets_for_scenario(&scenario) {
            let mission_type = preset.original_mission_type().expect("a mission type");
            let lowered =
                lower_preset_roster(&catalog, preset.id()).expect("the measured roster lowers");
            let roles = |side| {
                lowered
                    .entries
                    .iter()
                    .filter(move |entry| entry.side == side)
                    .map(|entry| entry.role.clone().unwrap_or_default())
                    .collect::<Vec<_>>()
            };

            // Wingmen: the declared count, on row 5, under the template the
            // image's lookup register selects for this type.
            let wingmen: Vec<_> = lowered
                .entries
                .iter()
                .filter(|entry| entry.side == cs_content::instant_action::ScenarioSide::Ally)
                .collect();
            assert_eq!(
                wingmen.len(),
                aircraft.wingmen().unwrap_or(0) as usize,
                "{}",
                preset.id()
            );
            let want_role = match lookup_register(&image, type_index(mission_type)) {
                "edi" => IA_WINGMAN_FOLLOWER_ROLE,
                _ => IA_WINGMAN_BASE_ROLE,
            };
            for wingman in &wingmen {
                assert_eq!(wingman.role.as_deref(), Some(want_role), "{}", preset.id());
                assert_eq!(
                    wingman
                        .geometry
                        .as_ref()
                        .expect("row 5 lowers")
                        .as_content(),
                    &table_airframe(&rows[5].display),
                    "{}: wingmen fly row 5",
                    preset.id()
                );
            }

            // Enemy side: the declared groups in record order, then the ace
            // iff the type is 0.
            let enemy_roles = roles(cs_content::instant_action::ScenarioSide::Enemy);
            let mut want: Vec<String> = aircraft
                .enemy_groups()
                .iter()
                .map(|group| format!("group{}", group.index()))
                .collect();
            let seats_ace = type_index(mission_type) == 0;
            if seats_ace {
                want.push("ace".to_owned());
                aces += 1;
            }
            assert_eq!(enemy_roles, want, "{}", preset.id());
            assert_eq!(
                catalog_seats_ace(&enemy_roles),
                ia_spawns_named_ace(mission_type).expect("a vocabulary name"),
                "{}: the production predicate agrees with the roster",
                preset.id()
            );

            // The lookups every seated aircraft needs resolve in the registry.
            for group in aircraft.enemy_groups() {
                let row = row_of(group.plane().expect("a declared group plane"));
                assert!(
                    registry.contains_key(&row.base),
                    "{}: group{}'s base record {} is in vehicle.zrd",
                    preset.id(),
                    group.index(),
                    row.base
                );
            }
            if seats_ace {
                let row = row_of(aircraft.ace().plane().expect("a declared ace plane"));
                assert!(registry.contains_key(&row.base), "{}", preset.id());
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 30, "all thirty presets were checked");
    assert_eq!(aces, 8, "eight directories offer dogfight_ace");
}

fn catalog_seats_ace(enemy_roles: &[String]) -> bool {
    enemy_roles.iter().any(|role| role == "ace")
}

// ------------------------------------------------------------- finding --------

#[test]
fn accept_f49_d_03_fu_rules_roster_the_finding_records_the_measurement() {
    assert_eq!(IA_SPAWN_ROSTER_CLAIM, "f49.d.03.ia-spawn-roster");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let finding = fs::read_to_string(root.join(FINDING)).expect("the finding is committed");
    for needle in [
        IA_SPAWN_ROSTER_CLAIM,
        "0x45a390",
        "0x459390",
        "0x426d60",
        "0x426d20",
        "0x45aa24",
        "0x45ad36",
        "0x458ff0",
        "wingman_plane",
        "dogfight_ace",
        "dogfight_squadron",
        "zeppelin_run",
        "stunt_flying",
        "ZBD/c2b/IA1",
        "static",
        "unmeasured",
    ] {
        assert!(finding.contains(needle), "the finding mentions {needle}");
    }
}
