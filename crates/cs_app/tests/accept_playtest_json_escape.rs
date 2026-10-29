//! `accept_playtest_json_escape_*` acceptance tests (Rally #1141,
//! PLAYTEST-JSON-ESCAPE).
//!
//! The playtest's `playtest sources:` / `playtest flight:` stdout lines, the
//! smoke's `report.json` and the evidence harness's `playtest-flight.json` are
//! all built with `format!`, so every field that comes from the original
//! installation or from the host filesystem (`--cs-path`, a stored node name,
//! an engine name, a `vehicle.zrd` record id) is only JSON if it is written as
//! a JSON **string literal**. Before this task two renderers interpolated such
//! values between hand-typed quotes with no escape at all
//! (`RetailContent::manifest_json`'s `installation` and `area_node`,
//! `RetailFlight::json`'s `record`/`chain`/`engine`/`provenance`), so a `"` or
//! a control byte in any of them produced a line that no JSON parser accepts.
//!
//! Every test here drives the production renderers and parses what they
//! produce with a real parser (`serde_json`), so "it looks balanced" is never
//! the acceptance: the object either parses and round-trips or it does not.
//! Nothing needs the installation — the fixtures are authored here, and no
//! original data is read or committed.

use std::path::PathBuf;

use bevy::math::Quat;
use cs_app::json::{json_escape, json_string};
use cs_app::playtest::retail::{RETAIL_LABEL, RetailContent, RetailFlight};
use cs_app::playtest_retail::{PlaytestAircraftReport, PlaytestAreaReport, UndrawnBinding};
use cs_app::playtest_textures::PlaytestTextureReport;
use cs_content::original_airframe::OriginalCrashParameters;
use cs_content::world::{Aabb, WorldId};
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_types::content::Provenance;
use cs_types::evidence::{ClaimId, ClaimStatus};

/// A value that holds every character a JSON string cannot carry raw: both
/// string delimiters, both whitespace controls, a below-`0x20` control byte
/// and a non-ASCII character that must survive verbatim.
const HOSTILE: &str = "quote\" backslash\\ tab\t nl\n ctl\u{1} del\u{7f} caf\u{e9}";

/// The provenance label of the flight fixture when it is asked to be
/// hostile: `RetailFlight`'s is a `&'static str`, so the spelling is authored
/// as a constant.
const HOSTILE_PROVENANCE: &str = "fixture \"static\" \\ law\u{2}";

/// The provenance label of the flight fixture's documented case — the label
/// the real `playtest flight` line carries today.
const PLAIN_PROVENANCE: &str = "OWNER-STATIC-2026-10-08";

/// The installation-relative container the fixture scene was read from.
const FIXTURE_CONTAINER: &str = "zbd/planes.zbd";
/// Its digest, spelled the way the playtest reports one.
const FIXTURE_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The `RetailContent` the renderers are asked about: the prop_spin suite's
/// fixture shape, with the strings the task names supplied by the caller.
fn fixture_content(installation: &str, node_name: &str) -> RetailContent {
    RetailContent {
        installation: installation.to_owned(),
        containers: vec![(FIXTURE_CONTAINER.to_owned(), FIXTURE_SHA256.to_owned())],
        area: PlaytestAreaReport {
            world: WorldId::from_key("c1c").expect("c1c is a world id"),
            node_slot: 517,
            node_name: node_name.to_owned(),
            nodes: 793,
            mesh_records: 401,
            stored_bindings: 401,
            lod_distance_m: 300.0,
            hidden_lods: Vec::new(),
            undrawn: Vec::new(),
            triangles: 8_673,
            bounds: Aabb::try_new([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]).expect("a fixture box"),
            refused: Vec::new(),
            gaps: Vec::new(),
        },
        aircraft: PlaytestAircraftReport {
            container_key: FIXTURE_CONTAINER.to_owned(),
            root_name: "bloodhawk".to_owned(),
            intact_node_slot: 2_296,
            intact_node_name: "healthy".to_owned(),
            lod_distance_m: 20.0,
            selected_lod: Some((2_297, "nearest".to_owned())),
            lod_coverage: None,
            hidden_lods: Vec::new(),
            parts: Vec::new(),
            undrawn: vec![UndrawnBinding {
                node_slot: 2_542,
                node_name: "prop1".to_owned(),
                mesh_index: 9,
                reason: "a propeller state the original shows under conditions that are \
                         unmeasured"
                    .to_owned(),
            }],
            airframe_bindings: 17,
            extent_m: [2.1, 1.5, 10.2],
        },
        area_entities: Vec::new(),
        spawn_m: [-60.0, 200.0, 200.0],
        half_extents_m: [1.0, 1.0, 1.0],
        visual_rotation: Quat::IDENTITY,
        parts: Vec::new(),
        propeller: None,
        textures: PlaytestTextureReport {
            claims: [
                "playtest-retail.texture-archive-tier-is-designed",
                "playtest-retail.texture-name-reading-is-designed",
                "playtest-retail.texture-material-is-designed",
                "playtest-textures.decal-offset-is-designed",
            ],
            archive: "ZBD/C1C/rtexture10.zbd".to_owned(),
            archive_sha256: "00".to_owned(),
            group: "c1c".to_owned(),
            selection: "the world group's highest-numbered tier".to_owned(),
            name_reading: "stored texture name up to the first dot, ASCII lower case",
            subjects: Vec::new(),
        },
    }
}

/// The per-airframe numbers of the flight fixture: finite values, written so
/// the renderer's number formatting is exercised rather than stubbed.
fn fixture_airframe() -> OriginalAirframe {
    OriginalAirframe {
        roll_torque: 1.0,
        pitch_torque: 1.0,
        rudder_torque: 1.0,
        level_off_rate: 0.5,
        return_rate: 0.5,
        ang_momentum_damp: 0.1,
        rec_moments_inertia: [1.0, 1.0, 1.0],
        fd_speed: 135.0,
        engine_factor: 1.0,
        drag_factor: 1.0,
        veh_weight: 1.0,
        ref_area: 1.0,
        gravity: 9.81,
    }
}

/// The globals of the flight fixture (see [`fixture_airframe`]).
fn fixture_globals() -> OriginalGlobals {
    OriginalGlobals {
        nom_gravity: 9.81,
        lift_aoa_deg: [1.0, 2.0],
        max_aoa_deg: 3.0,
        high_g: [1.0, 2.0],
        low_g: [1.0, 2.0],
        lift_accel_rate: 0.5,
        stall_mag: 0.5,
        turn_fade_mph: [1.0, 2.0],
        yaw_low_speed: 1.0,
        yaw_high_speed: 2.0,
        yaw_fade_mph: [1.0, 2.0, 3.0],
        groundblow_elev_m: 400.0,
        groundblow_mag: 10.0,
        ai_groundblow: 0.5,
    }
}

/// The `RetailFlight` the renderer is asked about, with the strings the task
/// named supplied by the caller.
fn fixture_flight(
    record: &str,
    chain: &[&str],
    engine_name: &str,
    provenance: &'static str,
) -> RetailFlight {
    let crash_provenance = Provenance::new(
        ClaimId::new("playtest.json-escape.fixture").expect("a fixture claim id"),
        ClaimStatus::Designed,
        None,
    )
    .expect("a designed provenance needs no source span");
    let crash = OriginalCrashParameters {
        bounce_factor: 0.5,
        armor_damage_range: [0.0, 1.0],
        health_damage_range: [0.0, 1.0],
        bounce_provenance: crash_provenance.clone(),
        damage_provenance: crash_provenance,
    };
    RetailFlight {
        model: OriginalFlightModel::full(fixture_airframe(), fixture_globals()),
        record: record.to_owned(),
        inheritance_chain: chain.iter().map(|name| (*name).to_owned()).collect(),
        engine: (7, engine_name.to_owned(), 1.25),
        fd_speed_m_s: 135.0,
        fuel: 100.0,
        crash,
        provenance,
    }
}

/// **A hostile installation path and node name survive as one JSON object.**
///
/// The task's own failure mode: `manifest_json` interpolated the installation
/// and the area's stored node name between hand-typed quotes, so a `"` or a
/// control byte in either closed the string early and the `playtest sources:`
/// line stopped being JSON. The rendered object now has to parse with a real
/// parser and hand every one of those values back unchanged.
#[test]
fn accept_playtest_json_escape_hostile_manifest_values_stay_one_json_object() {
    let content = fixture_content(HOSTILE, HOSTILE);
    let rendered = content.manifest_json();
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_else(|error| {
        panic!("the manifest is not JSON ({error}): {rendered}");
    });
    assert_eq!(parsed["installation"].as_str(), Some(HOSTILE), "{rendered}");
    assert_eq!(parsed["area_node"].as_str(), Some(HOSTILE), "{rendered}");
    assert_eq!(
        parsed["containers"][0]["container"].as_str(),
        Some(FIXTURE_CONTAINER),
        "{rendered}"
    );
    assert_eq!(
        parsed["aircraft_selection"]["intact_node"].as_str(),
        Some("healthy"),
        "{rendered}"
    );

    // The sub-objects the manifest composes carry original-data strings too:
    // the texture report's container keys and the flight line's record and
    // engine name are the same class of value, and a hostile spelling there
    // must be equally invisible to a parser.
    let mut hostile_subobjects = fixture_content("fixture", "piratezep");
    hostile_subobjects.aircraft.selected_lod = Some((2_297, HOSTILE.to_owned()));
    hostile_subobjects.aircraft.undrawn[0].node_name = HOSTILE.to_owned();
    hostile_subobjects.aircraft.undrawn[0].reason = HOSTILE.to_owned();
    hostile_subobjects.textures.archive = HOSTILE.to_owned();
    let rendered = hostile_subobjects.manifest_json();
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_else(|error| {
        panic!("the manifest with hostile sub-objects is not JSON ({error}): {rendered}");
    });
    assert_eq!(
        parsed["aircraft_selection"]["selected_lod"]["name"].as_str(),
        Some(HOSTILE),
        "{rendered}"
    );
    assert_eq!(
        parsed["aircraft_undrawn"][0]["name"].as_str(),
        Some(HOSTILE),
        "{rendered}"
    );
    assert_eq!(
        parsed["aircraft_undrawn"][0]["reason"].as_str(),
        Some(HOSTILE),
        "{rendered}"
    );
    assert_eq!(
        parsed["textures"]["subjects"],
        serde_json::json!([]),
        "{rendered}"
    );
}

/// **A hostile record id, engine name and provenance label survive the
/// `playtest flight` line.**
///
/// `RetailFlight::json` interpolated all four of its original-data strings
/// raw; the rendered statement now has to parse and give each one back.
#[test]
fn accept_playtest_json_escape_hostile_flight_values_stay_one_json_object() {
    let flight = fixture_flight(
        "p\"blood\\hawk",
        &["pFighter", "pblood\nhawk"],
        "engine \"seven\"",
        HOSTILE_PROVENANCE,
    );
    let rendered = flight.json();
    let parsed: serde_json::Value = serde_json::from_str(&rendered)
        .unwrap_or_else(|error| panic!("the flight statement is not JSON ({error}): {rendered}"));
    assert_eq!(
        parsed["record"].as_str(),
        Some("p\"blood\\hawk"),
        "{rendered}"
    );
    assert_eq!(
        parsed["chain"],
        serde_json::json!(["pFighter", "pblood\nhawk"]),
        "{rendered}"
    );
    assert_eq!(
        parsed["engine"]["name"].as_str(),
        Some("engine \"seven\""),
        "{rendered}"
    );
    assert_eq!(
        parsed["provenance"].as_str(),
        Some(HOSTILE_PROVENANCE),
        "{rendered}"
    );
    // The rest of the statement is unchanged data: the flags still say what
    // they said, and the numbers still parse as numbers.
    assert_eq!(parsed["verified_original"], serde_json::json!(false));
    assert_eq!(
        parsed["level_off_binding"].as_str(),
        Some("key.l"),
        "{rendered}"
    );
    assert_eq!(parsed["fd_speed_m_s"].as_f64(), Some(135.0), "{rendered}");
}

/// **The installation and record the playtest holds today render byte for
/// byte what they rendered before.**
///
/// The escaping is a no-op for a value that holds nothing escapable, so the
/// documented case (`--cs-path` a plain path, the `bloodhawk` record) has to
/// produce the same bytes: `docs/PLAYTEST*.md`'s examples, the substring
/// assertions of the playtest suites and the committed evidence artifacts all
/// depend on it. The expected strings below are the renderers' templates as
/// they were written before this task, with the same fixture values
/// interpolated the way they were interpolated then.
#[test]
fn accept_playtest_json_escape_the_documented_case_keeps_its_bytes() {
    // A plain value must not move a single byte.
    for plain in [
        FIXTURE_CONTAINER,
        FIXTURE_SHA256,
        "bloodhawk",
        "piratezep",
        "ZBD/C1C/rtexture10.zbd",
        RETAIL_LABEL,
    ] {
        assert_eq!(json_string(plain), format!("\"{plain}\""), "{plain}");
        assert_eq!(json_escape(plain), plain, "{plain}");
    }

    let content = fixture_content("/opt/crimson skis/GOSDATA", "piratezep");
    let rendered = content.manifest_json();
    let label = RETAIL_LABEL;
    let key = FIXTURE_CONTAINER;
    let sha = FIXTURE_SHA256;
    let installation = "/opt/crimson skis/GOSDATA";
    let node_name = "piratezep";
    let containers = format!("{{\"container\":\"{key}\",\"sha256\":\"{sha}\"}}");
    let expected = format!(
        "{{\"label\":\"{label}\",\"installation\":\"{installation}\",\"containers\":[{containers}],\
\"area_node\":\"{node_name}\",\"area_mesh_records\":{},\"area_triangles\":{},\
\"area_colliders\":{},{},{},{},\"textures\":{}}}",
        content.area.mesh_records,
        content.area.triangles,
        content.area.colliders(),
        content.area.json_fields(),
        content.aircraft.json_fields(),
        cs_app::playtest::propeller::propeller_spin_json(content.propeller.as_ref()),
        content.textures.json(),
    );
    assert_eq!(
        rendered, expected,
        "the documented manifest bytes moved: {rendered}"
    );

    let flight = fixture_flight(
        "pbloodhawk",
        &["pbloodhawk", "pFighter"],
        "pbloodhawk_engine",
        PLAIN_PROVENANCE,
    );
    let rendered = flight.json();
    let expected = format!(
        "{{\"record\":\"pbloodhawk\",\"chain\":[\"pbloodhawk\",\"pFighter\"],\
\"engine\":{{\"id\":7,\"name\":\"pbloodhawk_engine\",\"factor\":{}}},\
\"fd_speed_m_s\":{},\"veh_weight\":{},\"ref_area\":{},\"gravity\":{},\"start_speed_m_s\":{},\
\"provenance\":\"{PLAIN_PROVENANCE}\",\"verified_original\":false,\
\"calibrated_against_an_original_run\":false,\"level_off_toggle_wired\":true,\
\"level_off_binding\":\"key.l\"}}",
        flight.engine.2,
        flight.model.airframe.fd_speed,
        flight.model.airframe.veh_weight,
        flight.model.airframe.ref_area,
        flight.model.airframe.gravity,
        flight.start_speed_m_s(),
    );
    assert_eq!(
        rendered, expected,
        "the documented flight-statement bytes moved: {rendered}"
    );

    // The evidence harness's `playtest-flight.json` quotes its installation
    // path through the same helper; a plain path must be unchanged there too.
    let path = PathBuf::from("/opt/crimson skis");
    assert_eq!(
        json_string(&path.display().to_string()),
        "\"/opt/crimson skis\""
    );
}

/// **The shared escaper covers every character a JSON string cannot hold.**
///
/// The helper is what the four writers above delegate to, so the acceptance
/// pins its own behaviour too: the two delimiters, the three whitespace
/// controls, every other control byte as a `\u00xx` escape in lower-case hex,
/// and everything else verbatim. Each escaped literal is parsed back with the
/// real parser, so the escape spellings themselves are checked rather than
/// assumed.
#[test]
fn accept_playtest_json_escape_the_shared_escaper_covers_every_control_byte() {
    assert_eq!(json_string("plain"), "\"plain\"");
    assert_eq!(json_string(""), "\"\"");
    assert_eq!(json_string("caf\u{e9}"), "\"caf\u{e9}\"");
    assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
    assert_eq!(json_escape("nl\nr\rt\t"), "nl\\nr\\rt\\t");
    for code in 0u32..0x20 {
        let character = char::from_u32(code).expect("the code is a scalar value");
        let escaped = json_escape(&character.to_string());
        let expected = match character {
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            other => format!("\\u{:04x}", other as u32),
        };
        assert_eq!(escaped, expected, "code {code:#x}");
        let literal = json_string(&character.to_string());
        let parsed = serde_json::from_str::<String>(&literal)
            .unwrap_or_else(|error| panic!("code {code:#x}: {literal} is not JSON: {error}"));
        assert_eq!(parsed, character.to_string(), "code {code:#x}");
    }
    // A value that mixes them all round-trips through the real parser.
    let round_trip = serde_json::from_str::<String>(&json_string(HOSTILE))
        .unwrap_or_else(|error| panic!("the hostile fixture is not JSON: {error}"));
    assert_eq!(round_trip, HOSTILE);
}
