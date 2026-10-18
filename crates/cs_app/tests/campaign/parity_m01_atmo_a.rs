//! PARITY-M01-ATMO-A (#1648): the weather.zrd units and the sky, water and
//! cloud consumers for M01 "The Lost Treasure" (finding
//! `docs/findings/2026-10-18-parity-m01-atmo-a-weather-units-and-sky-consumers.md`).
//!
//! Three members:
//!
//! * the committed `[[atmo_weather]]` rows parse under the production loader
//!   and agree with the finding's numbers;
//! * with the owner's image named, every row is re-read from the image's
//!   bytes, and the instruction bytes the finding's claims rest on are
//!   re-read too;
//! * with the installation named, M01's own `weather.zrd` (through the
//!   production reader) and the `horizon` subtree, texture table and sea
//!   tiles of `ZBD/C3/gamez.zbd` (through the GameZ readers) carry the values
//!   the finding states.

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::{load_engine_image, original_image_digest};
use cs_formats::gamez::{read_gamez_materials, read_gamez_meshes, read_gamez_nodes};
use cs_formats::io::ParseContext;

const TABLE_TEXT: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// The number of `[[atmo_weather]]` rows the finding commits.
const ROW_COUNT: usize = 17;

struct Row {
    name: String,
    va: u64,
    kind: String,
    value: String,
}

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let mut current: Option<Vec<(String, String)>> = None;
    let finish = |entries: Option<Vec<(String, String)>>, rows: &mut Vec<Row>| {
        let Some(entries) = entries else { return };
        let get = |key: &str| -> String {
            entries
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("an [[atmo_weather]] row lacks `{key}`"))
                .1
                .clone()
        };
        assert_eq!(get("provenance"), "\"static\"");
        assert_eq!(get("claim"), "\"OWNER-STATIC-2026-10-18\"");
        let va = get("va");
        rows.push(Row {
            name: get("name").trim_matches('"').to_owned(),
            va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("a hex va"),
            kind: get("kind").trim_matches('"').to_owned(),
            value: get("value"),
        });
    };
    for line in TABLE_TEXT.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(current.take(), &mut rows);
            if line == "[[atmo_weather]]" {
                current = Some(Vec::new());
            }
        } else if let Some(entries) = current.as_mut()
            && let Some((key, value)) = line.split_once(" = ")
        {
            entries.push((key.to_owned(), value.to_owned()));
        }
    }
    finish(current.take(), &mut rows);
    rows
}

fn named<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("row {name} is committed"))
}

fn as_f64(row: &Row) -> f64 {
    row.value.parse().expect("a float value")
}

#[test]
fn accept_parity_m01_atmo_a_rows_parse_and_agree_with_the_finding() {
    engine_facts().expect("the committed table parses with the atmosphere rows in it");
    let rows = rows();
    assert_eq!(rows.len(), ROW_COUNT);
    assert_eq!(as_f64(named(&rows, "cloud_top_default")), 280.0);
    assert_eq!(as_f64(named(&rows, "cloud_bottom_default")), 240.0);
    assert_eq!(named(&rows, "cloud_thickness_default").value, "0");
    assert_eq!(as_f64(named(&rows, "cloud_colour_default")), 1.0);
    assert_eq!(
        as_f64(named(&rows, "colour_byte_scale")) as f32,
        1.0f32 / 255.0
    );
    assert!(
        (as_f64(named(&rows, "degrees_to_radians")) - std::f64::consts::PI / 180.0).abs() < 1e-13
    );
    assert!(
        (as_f64(named(&rows, "zone_default_sun_pitch")) + std::f64::consts::FRAC_PI_2).abs() < 1e-6
    );
    assert_eq!(as_f64(named(&rows, "cloud_band_half_factor")), 0.5);
    assert_eq!(as_f64(named(&rows, "cloud_inside_fraction")) as f32, 0.8);
    assert_eq!(as_f64(named(&rows, "clip_near_floor")), 6.0);
    assert_eq!(
        named(&rows, "cloud_floor_node_name").value,
        "\"zone2_cloud_floor\""
    );
    assert_eq!(named(&rows, "pfhorizon_name").value, "\"pfhorizon\"");
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity_m01_atmo_a_rows_and_instructions_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("va inside the image")).expect("an offset")
    };

    for row in rows() {
        let offset = at(row.va);
        match row.kind.as_str() {
            "f32" => {
                let bytes: [u8; 4] = image[offset..offset + 4].try_into().expect("four bytes");
                assert_eq!(
                    f32::from_le_bytes(bytes),
                    row.value.parse::<f32>().expect("an f32"),
                    "{} drifted at {:#x}",
                    row.name,
                    row.va
                );
            }
            "f64" => {
                let bytes: [u8; 8] = image[offset..offset + 8].try_into().expect("eight bytes");
                assert_eq!(
                    f64::from_le_bytes(bytes),
                    as_f64(&row),
                    "{} drifted at {:#x}",
                    row.name,
                    row.va
                );
            }
            "u32" => {
                let bytes: [u8; 4] = image[offset..offset + 4].try_into().expect("four bytes");
                assert_eq!(
                    u64::from(u32::from_le_bytes(bytes)),
                    row.value.parse::<u64>().expect("a u32"),
                    "{} drifted at {:#x}",
                    row.name,
                    row.va
                );
            }
            "cstr" => {
                let want = row.value.trim_matches('"').as_bytes();
                assert_eq!(&image[offset..offset + want.len()], want, "{}", row.name);
                assert_eq!(
                    image[offset + want.len()],
                    0,
                    "{} is NUL-terminated",
                    row.name
                );
            }
            other => panic!("{}: unknown kind {other}", row.name),
        }
    }

    let bytes = |va: u64, want: &[u8], what: &str| {
        let offset = at(va);
        assert_eq!(
            &image[offset..offset + want.len()],
            want,
            "{what} at {va:#x}"
        );
    };
    // The CLOUD_COVER defaults: `mov [ebx+4], 280.0f` and `mov [ebx+8], 240.0f`.
    bytes(
        0x4bc7d7,
        &[0xc7, 0x43, 0x04, 0x00, 0x00, 0x8c, 0x43],
        "TOP default",
    );
    bytes(
        0x4bc824,
        &[0xc7, 0x43, 0x08, 0x00, 0x00, 0x70, 0x43],
        "BOTTOM default",
    );
    // FOG_ALTITUDE: the first spelled number goes to ebx = zone+0xc, the
    // second to zone+8.
    bytes(0x4bc3f3, &[0x8d, 0x5e, 0x0c], "lea ebx, [esi+0xc]");
    bytes(0x4bc4a9, &[0x53, 0x52], "push ebx (first element dest)");
    bytes(0x4bc4b0, &[0x8d, 0x46, 0x08, 0x50], "lea eax,[esi+8]; push");
    // The zone apply passes (zone+0xc, zone+8) to the altitude setter.
    bytes(
        0x472f0e,
        &[
            0x8b, 0x56, 0x08, 0x8b, 0x46, 0x0c, 0x8b, 0x0d, 0xb8, 0xc0, 0x71, 0x00, 0x52, 0x50,
            0x51, 0xe8,
        ],
        "altitude setter call",
    );
    // The half factor on THICKNESS and the band midpoint, and the zone
    // selector's compare of object Y against the midpoint at 0x71c2cc.
    bytes(0x4736b1, &[0xd8, 0x0d, 0xe0, 0x32, 0x60, 0x00], "fmul 0.5");
    bytes(
        0x473593,
        &[0xd9, 0x40, 0x04, 0xd8, 0x1d, 0xcc, 0xc2, 0x71, 0x00],
        "zone selector: object Y vs the band midpoint",
    );
    // The camera function compares Y against the plateau start at 0x71c2d0.
    bytes(
        0x42eef1,
        &[0xd8, 0x1d, 0xd0, 0xc2, 0x71, 0x00],
        "camera Y vs plateau start",
    );
    // The per-node zone test: feature flag first.
    bytes(
        0x56c430,
        &[0xa1, 0x50, 0xcb, 0x62, 0x00, 0x85, 0xc0],
        "zone test head",
    );
}

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: this retail test needs the original installation; run it \
             with `--include-ignored`"
        )
    }))
}

fn read_container(root: &std::path::Path, key: &str) -> Vec<u8> {
    let found = cs_assets::install::discover(root).expect("the installation is discoverable");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
        .unwrap_or_else(|| panic!("no {key} in the discovered installation"));
    std::fs::read(root.join(record.relative_spelling.as_str()))
        .unwrap_or_else(|error| panic!("cannot read {key}: {error}"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_a_retail_m01_weather_matches_the_finding() {
    let root = game_dir();
    let found = cs_assets::install::discover(&root).expect("the installation is discoverable");
    let weather = cs_app::environment::retail::read_mission_weather(&found, "zbd/c3/m01")
        .expect("M01's weather.zrd reads through the production reader");
    let document = &weather.document;

    let cloud = &document.cloud_cover;
    assert_eq!(cloud.top.as_f64(), 11000.0);
    assert_eq!(cloud.bottom.as_f64(), 10000.0);
    assert_eq!(cloud.thickness.as_f64(), 50.0);
    let bytes = |colour: Option<[cs_content::weather::WeatherScalar; 3]>| {
        colour
            .expect("M01 spells a cloud colour")
            .map(cs_content::weather::WeatherScalar::as_f64)
    };
    assert_eq!(bytes(cloud.top_color), [32.0, 56.0, 72.0]);
    assert_eq!(bytes(cloud.bottom_color), [32.0, 56.0, 72.0]);

    let zone = |name: &str| {
        document
            .zones
            .iter()
            .find(|zone| zone.name == name)
            .unwrap_or_else(|| panic!("M01 has {name}"))
    };
    let triple =
        |values: [cs_content::weather::WeatherScalar; 3]| values.map(|v| v.as_f64() as f32);
    assert_eq!(triple(zone("ZONE1").fog_color), [0.79, 0.79, 0.79]);
    assert_eq!(triple(zone("ZONE2").fog_color), [0.06275, 0.09412, 0.1882]);
    assert_eq!(
        zone("ZONE1").fog_ranges.map(|v| v.as_f64()),
        [1000.0, 4500.0]
    );
    assert_eq!(
        zone("ZONE2").fog_ranges.map(|v| v.as_f64()),
        [1000.0, 4500.0]
    );
    assert_eq!(
        zone("SW_ZONE1").fog_ranges.map(|v| v.as_f64()),
        [600.0, 1700.0]
    );
    assert_eq!(
        zone("SW_ZONE2").fog_ranges.map(|v| v.as_f64()),
        [600.0, 2700.0]
    );
    for name in ["ZONE1", "ZONE2", "SW_ZONE1", "SW_ZONE2"] {
        assert_eq!(
            zone(name)
                .fog_altitude
                .expect("every zone spells a fog altitude")
                .map(|v| v.as_f64()),
            [9000.0, 10000.0],
            "{name}"
        );
    }
    assert_eq!(zone("ZONE1").clip_ranges.map(|v| v.as_f64()), [5.0, 4800.0]);
    assert_eq!(zone("ZONE2").clip_ranges.map(|v| v.as_f64()), [5.0, 5000.0]);
    assert_eq!(
        triple(zone("ZONE1").sunlight_orientation),
        [-25.0, 135.0, 0.0]
    );
    assert_eq!(zone("ZONE1").sunlight_active.as_f64(), 1.0);
    assert_eq!(zone("SW_ZONE1").sunlight_active.as_f64(), 0.0);
    assert!(document.precipitation.is_none());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_a_retail_c3_gamez_sky_graph_matches_the_finding() {
    let root = game_dir();
    let key = "zbd/c3/gamez.zbd";
    let bytes = read_container(&root, key);
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(key), &bytes)
        .expect("the node array reads");
    let materials = read_gamez_materials(&mut ParseContext::with_defaults(key), key, &bytes)
        .expect("the material section reads");
    let meshes = read_gamez_meshes(&mut ParseContext::with_defaults(key), key, &bytes)
        .expect("the mesh section reads");

    // The texture table: the sky, cloud and sun art is here; `water1` and
    // `cloudlayer` are C1C-only names.
    let textures: Vec<String> = (0..materials.count().max(4096))
        .map_while(|index| materials.texture(index).map(|t| t.name.clone()))
        .collect();
    for present in [
        "Sky1.tif",
        "cloud1.tif",
        "cloud2.tif",
        "cumulus1.tif",
        "Sun.tif",
    ] {
        assert!(textures.iter().any(|name| name == present), "{present}");
    }
    for absent in ["water1.tif", "water1", "cloudlayer.tif", "cloudlayer"] {
        assert!(!textures.iter().any(|name| name == absent), "{absent}");
    }
    assert_eq!(textures.len(), 465);
    for frame in 0..16 {
        let wanted = format!("wtr{frame:05}.tif");
        assert!(textures.contains(&wanted), "{wanted}");
    }

    // The horizon subtree.
    let named = |name: &str| -> Vec<&cs_formats::gamez::RawNode> {
        nodes
            .nodes
            .iter()
            .filter(|node| node.name == name)
            .collect()
    };
    let horizon = named("horizon");
    assert_eq!(horizon.len(), 1);
    let horizon = horizon[0];
    assert_eq!(horizon.zone_id(), 255);
    assert_eq!(horizon.mesh_index(), -1);
    let child_names = |node: &cs_formats::gamez::RawNode| -> Vec<(String, u32)> {
        node.children
            .iter()
            .map(|&child| {
                let child = &nodes.nodes[child as usize];
                (child.name.clone(), child.zone_id())
            })
            .collect()
    };
    assert_eq!(
        child_names(horizon),
        [("zone2".to_owned(), 2), ("zone1".to_owned(), 1)]
    );
    let zone1 = nodes.nodes[horizon.children[1] as usize].clone();
    assert_eq!(
        child_names(&zone1),
        [
            ("sun".to_owned(), 1),
            ("g1155".to_owned(), 1),
            ("h_zone2scroll".to_owned(), 1)
        ]
    );
    assert!(
        nodes.nodes[horizon.children[0] as usize]
            .children
            .is_empty()
    );

    // The dome wall and the sun quad use the named textures.
    let textures_of = |node: &cs_formats::gamez::RawNode| -> std::collections::BTreeSet<String> {
        let mesh = meshes.meshes[usize::try_from(node.mesh_index()).expect("a mesh")]
            .as_ref()
            .expect("a stored mesh");
        mesh.mesh
            .polygons
            .iter()
            .filter_map(|polygon| {
                materials
                    .material(polygon.material)
                    .and_then(|m| materials.texture_of(m))
                    .map(|t| t.name.clone())
            })
            .collect()
    };
    let child = |index: usize| &nodes.nodes[zone1.children[index] as usize];
    assert_eq!(textures_of(child(0)), ["Sun.tif".to_owned()].into());
    assert_eq!(textures_of(child(2)), ["Sky1.tif".to_owned()].into());
    let bank = textures_of(child(1));
    for name in [
        "Sky1.tif",
        "cloud1.tif",
        "cloud2.tif",
        "cumulus1.tif",
        "cumulus2.tif",
    ] {
        assert!(bank.contains(name), "g1155 uses {name}");
    }

    // M01's world has no sprite cloud nodes and no cloud-floor node.
    for absent in [
        "cloudparent",
        "cloudsprite1",
        "cloudsprite2",
        "zone2_cloud_floor",
    ] {
        assert!(named(absent).is_empty(), "{absent} must be absent from C3");
    }
    // The light node `SUNLIGHT_*` goes to.
    let sunlight = named("sunlight");
    assert_eq!(sunlight.len(), 1);
    assert_eq!(sunlight[0].kind.tag(), cs_formats::gamez::NODE_TYPE_LIGHT);

    // The sea: 347 meshes use `wtr00000.tif`, 251 of them terrain tiles that
    // are direct children of `world1`.
    let water_meshes: Vec<_> = nodes
        .nodes
        .iter()
        .filter(|node| node.mesh_index() >= 0 && textures_of(node).contains("wtr00000.tif"))
        .collect();
    assert_eq!(water_meshes.len(), 347);
    let sea_tiles = water_meshes
        .iter()
        .filter(|node| {
            node.parent
                .is_some_and(|parent| nodes.nodes[parent as usize].name == "world1")
        })
        .count();
    assert_eq!(sea_tiles, 251);
    // Its material is a 16-frame cycle.
    let sea_material = (0..materials.count())
        .filter_map(|index| materials.material(index))
        .find(|material| {
            materials
                .texture_of(material)
                .is_some_and(|t| t.name == "wtr00000.tif")
                && material.cycle.is_some()
        })
        .expect("a cycled wtr00000 material");
    assert_eq!(
        sea_material.cycle.as_ref().map(|c| c.textures.len()),
        Some(16)
    );
}
