//! M01-AIV-LAYOUT (#1453): the `aiv.zrd` aircraft-record layout — which
//! fields give the pose, the team, the AI group, the enabled flag, the
//! primary target, the title and the initial sleep state, and how the
//! mission program wakes a sleeping aircraft.
//!
//! The field map is static analysis of the owner-supplied decrypted image
//! (finding `docs/findings/2026-10-15-m01-aiv-layout.md`) and is committed
//! as the `[aiv_layout]` row of `data/owner-static/engine-facts.toml`, so a
//! normal run never needs `$CS_ENGINE_IMAGE`. The synthetic tests exercise
//! the production reader over authored records; the retail member re-derives
//! the measured values of all twelve of the `zbd/c1c/m01` records from
//! `$CS_GAME_DIR` — M06's archive under #1565's corrected layout
//! through the production decoder; the image member re-reads the cited bytes
//! so the committed table cannot silently drift. Nothing here is
//! `verified_original`.

use std::path::{Path, PathBuf};

use cs_app::mission_start::{
    AIV_FIELD_COUNT, DEACTIVATED_FIELD, ENABLED_FIELD, GROUP_FIELD, HEADING_FIELD, NETIDS_FIELD,
    Netids, POSITION_FIELD, PRIMARY_TARGET_FIELD, ScriptedAircraft, SpawnDispatch, TEAM_FIELD,
    TITLE_FIELD, read_aircraft_records, spawn_dispatch,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::{ClaimStatus, ContentHash};

/// Builds an 81-field record: the measured retail shape, with the measured
/// fields at the indices the production constants name.
fn full_record(name: &str, overrides: &[(usize, ZrdValue)]) -> ZrdValue {
    let mut fields = vec![ZrdValue::Int(0); AIV_FIELD_COUNT];
    fields[NETIDS_FIELD] = ZrdValue::Int(0xFFFFFFFF);
    fields[POSITION_FIELD] = ZrdValue::List(vec![
        ZrdValue::Float(1.0),
        ZrdValue::Float(2.0),
        ZrdValue::Float(3.0),
    ]);
    fields[HEADING_FIELD] = ZrdValue::Float(90.0);
    fields[TEAM_FIELD] = ZrdValue::Int(1);
    fields[GROUP_FIELD] = ZrdValue::Int(2);
    fields[ENABLED_FIELD] = ZrdValue::Int(1);
    fields[PRIMARY_TARGET_FIELD] = ZrdValue::Text(String::new());
    fields[TITLE_FIELD] = ZrdValue::Text("MSG_TEST_NAME".to_owned());
    fields[DEACTIVATED_FIELD] = ZrdValue::Int(0);
    for (index, value) in overrides {
        fields[*index] = value.clone();
    }
    ZrdValue::List(vec![
        ZrdValue::Text(name.to_owned()),
        ZrdValue::List(fields),
    ])
}

fn span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_hex(&"cd".repeat(32)).expect("hash"),
        "ZBD/C0/M00/zrdr.zbd",
        Some("aiv.zrd"),
        16,
        64,
        None,
    )
    .expect("span")
}

fn only(records: Vec<ZrdValue>) -> Vec<ScriptedAircraft> {
    let mut document = vec![ZrdValue::List(vec![ZrdValue::Int(51)])];
    document.extend(records);
    read_aircraft_records("zbd/c0/m00", &ZrdValue::List(document), &span()).expect("read")
}

#[test]
fn accept_m01_aiv_layout_reads_every_measured_field_of_every_record() {
    let record = full_record(
        "ace_1",
        &[
            (NETIDS_FIELD, ZrdValue::Int(24)),
            (TEAM_FIELD, ZrdValue::Int(2)),
            (GROUP_FIELD, ZrdValue::Int(3)),
            (ENABLED_FIELD, ZrdValue::Int(0)),
            (PRIMARY_TARGET_FIELD, ZrdValue::Text("player".to_owned())),
            (TITLE_FIELD, ZrdValue::Text(String::new())),
            (DEACTIVATED_FIELD, ZrdValue::Int(1)),
        ],
    );
    let aircraft = only(vec![record]);
    assert_eq!(aircraft.len(), 1);
    let ace = &aircraft[0];
    assert_eq!(ace.name, "ace_1");
    assert_eq!((ace.index, ace.field_count), (1, AIV_FIELD_COUNT));
    assert_eq!(ace.netids, Some(Netids::One(24)));
    let pose = ace.stored_pose.expect("the measured pose shape");
    assert_eq!(pose.position, [1.0, 2.0, 3.0]);
    assert_eq!(pose.heading, 90.0);
    assert_eq!(ace.team, Some(2));
    assert_eq!(ace.group, Some(3));
    assert_eq!(ace.enabled, Some(false));
    assert_eq!(ace.primary_target.as_deref(), Some("player"));
    // An empty title is the parser's null, never a name.
    assert_eq!(ace.title, None);
    assert_eq!(ace.starts_asleep, Some(true));
    assert_eq!(ace.dispatch, SpawnDispatch::VehicleRecord("ace".to_owned()));
    assert_eq!(ace.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(ace.provenance.source.as_ref(), Some(&span()));
}

#[test]
fn accept_m01_aiv_layout_netids_accept_the_list_form_the_loader_reads() {
    let many = only(vec![full_record(
        "patrol_1",
        &[(
            NETIDS_FIELD,
            ZrdValue::List(vec![ZrdValue::Int(7), ZrdValue::Int(9)]),
        )],
    )]);
    assert_eq!(
        many[0].netids,
        Some(Netids::Many(vec![7, 9])),
        "a list of two is kept as spelled; the loader takes nothing from it"
    );
    let empty = only(vec![full_record(
        "patrol_2",
        &[(NETIDS_FIELD, ZrdValue::List(vec![]))],
    )]);
    assert_eq!(empty[0].netids, Some(Netids::Many(vec![])));
    let malformed = only(vec![full_record(
        "patrol_3",
        &[(
            NETIDS_FIELD,
            ZrdValue::List(vec![ZrdValue::Text("no".to_owned())]),
        )],
    )]);
    assert_eq!(
        malformed[0].netids, None,
        "a non-integer list is an unknown"
    );
}

#[test]
fn accept_m01_aiv_layout_dispatch_matches_the_measured_name_rules() {
    // The three literals the original compares case-sensitively…
    assert_eq!(spawn_dispatch("player"), SpawnDispatch::Player);
    assert_eq!(spawn_dispatch("wingman_1"), SpawnDispatch::WingmanOne);
    assert_eq!(spawn_dispatch("wingman_4"), SpawnDispatch::WingmanFour);
    assert_eq!(
        spawn_dispatch("Player"),
        SpawnDispatch::Unmatched,
        "the original's literal compare is case-sensitive: `Player` takes the \
         generic branch, and with no underscore it matches no registry record"
    );
    // …and the generic truncation at the LAST underscore.
    assert_eq!(
        spawn_dispatch("bsfury_1"),
        SpawnDispatch::VehicleRecord("bsfury".to_owned())
    );
    assert_eq!(
        spawn_dispatch("rusdevastator_3"),
        SpawnDispatch::VehicleRecord("rusdevastator".to_owned())
    );
    assert_eq!(
        spawn_dispatch("Devastator_2"),
        SpawnDispatch::VehicleRecord("Devastator".to_owned()),
        "the registry match is case-insensitive; the prefix is returned as spelled"
    );
    assert_eq!(
        spawn_dispatch("wingman"),
        SpawnDispatch::Unmatched,
        "a name with no underscore matches no registry record"
    );
}

#[test]
fn accept_m01_aiv_layout_short_or_odd_records_stay_unknown() {
    // The measured pose shape with only six fields: the fields past the end
    // answer none, never a guess.
    let short = vec![
        ZrdValue::Int(0),
        ZrdValue::List(vec![
            ZrdValue::Float(1.0),
            ZrdValue::Float(2.0),
            ZrdValue::Float(3.0),
        ]),
        ZrdValue::Float(90.0),
        ZrdValue::Int(1),
        ZrdValue::Int(2),
        ZrdValue::Int(1),
    ];
    let aircraft = only(vec![ZrdValue::List(vec![
        ZrdValue::Text("stub".to_owned()),
        ZrdValue::List(short),
    ])]);
    let stub = &aircraft[0];
    assert_eq!(stub.field_count, 6);
    assert_eq!(stub.netids, Some(Netids::One(0)));
    assert!(stub.stored_pose.is_some());
    assert_eq!(stub.team, Some(1));
    assert_eq!(stub.group, Some(2));
    assert_eq!(stub.enabled, Some(true));
    assert_eq!(stub.primary_target, None);
    assert_eq!(
        stub.title, None,
        "a record this short carries no title field"
    );
    assert_eq!(
        stub.starts_asleep, None,
        "and no deactivated field: the reader answers none, never a guess"
    );
    // A float where an integer is measured is an unknown, not a coerced value.
    let float_team = only(vec![full_record(
        "drift_1",
        &[(TEAM_FIELD, ZrdValue::Float(1.0))],
    )]);
    assert_eq!(float_team[0].team, None);
    // The header (a bare pair list, not `[name, fields]`) is skipped.
    assert_eq!(only(vec![]).len(), 0);
}

#[test]
fn accept_m01_aiv_layout_constants_match_the_committed_table() {
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    let layout = facts.aiv_layout();
    assert_eq!(layout.claim, "OWNER-STATIC-2026-10-15");
    assert_eq!(layout.field_count as usize, AIV_FIELD_COUNT);
    assert_eq!(layout.netids_field as usize, NETIDS_FIELD);
    assert_eq!(layout.position_field as usize, POSITION_FIELD);
    assert_eq!(layout.heading_field as usize, HEADING_FIELD);
    assert_eq!(layout.team_field as usize, TEAM_FIELD);
    assert_eq!(layout.group_field as usize, GROUP_FIELD);
    assert_eq!(layout.enabled_field as usize, ENABLED_FIELD);
    assert_eq!(layout.primary_target_field as usize, PRIMARY_TARGET_FIELD);
    assert_eq!(layout.title_field as usize, TITLE_FIELD);
    assert_eq!(layout.deactivated_field as usize, DEACTIVATED_FIELD);
}

/// The member decode path `recover_retail_start_configuration` walks for a
/// mission's `aiv.zrd` — the production discovery and decoder.
pub(crate) fn member_document(
    root: &Path,
    container_key: &str,
    member: &str,
) -> (ZrdValue, SourceSpan) {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key.to_ascii_lowercase())
        .unwrap_or_else(|| panic!("the installation has {container_key}"));
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = cs_types::install::RelativePath::new(&spelling.to_lowercase()).expect("a path");
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("readable");
    let discovery = cs_formats::script_raw::discover_container(container_key, &path, &bytes);
    let program = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(member))
        })
        .unwrap_or_else(|| panic!("{container_key} declares a {member} member"));
    let document =
        decode_zrd(program.bytes()).unwrap_or_else(|error| panic!("{member} decodes: {error:?}"));
    let span = program.locator().span();
    let source = SourceSpan::new(
        cs_assets::install::fingerprint(&found.manifest),
        &spelling,
        Some(member),
        span.offset,
        span.len,
        None,
    )
    .expect("a member span");
    (document, source)
}

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// One measured M01 row: name, netids, stored position and heading, team,
/// group, enabled, primary target, title, asleep-at-spawn and the vehicle
/// record the name dispatch selects.
type MeasuredRow = (
    &'static str,
    Option<u32>,
    [f32; 3],
    f32,
    u32,
    u32,
    bool,
    Option<&'static str>,
    Option<&'static str>,
    bool,
    SpawnDispatch,
);

/// The measured values of all twelve of M06's records, in record order.
/// Re-derived from the installation through the production decoder.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_aiv_layout_retail_m06_records_match_the_measured_values() {
    let (document, source) = member_document(&game_dir(), "zbd/c1c/m01/zrdr.zbd", "aiv.zrd");
    let aircraft = read_aircraft_records("zbd/c1c/m01", &document, &source).expect("read");
    let rows: Vec<MeasuredRow> = vec![
        (
            "player",
            Some(0xFFFFFFFF),
            [-3694.0, 1318.0, -12482.0],
            170.0,
            1,
            5,
            true,
            None,
            Some("MSG_PLAYER_NAME"),
            false,
            SpawnDispatch::Player,
        ),
        (
            "bsfury_1",
            Some(34),
            [-8590.46, 1300.0, 3675.44],
            0.0,
            2,
            3,
            true,
            Some("player"),
            Some("MSG_BSWAN_NAME"),
            true,
            SpawnDispatch::VehicleRecord("bsfury".to_owned()),
        ),
        (
            "devastator_3",
            Some(29),
            [-4355.0, 1350.0, -12881.0],
            180.0,
            1,
            5,
            true,
            None,
            Some("MSG_TEX_NAME"),
            false,
            SpawnDispatch::VehicleRecord("devastator".to_owned()),
        ),
        (
            "wingman_3",
            Some(0xFFFFFFFF),
            [-4531.0, 1280.0, -13065.0],
            180.0,
            1,
            5,
            true,
            Some("devastator_3"),
            Some("MSG_BUCK_NAME"),
            false,
            SpawnDispatch::VehicleRecord("wingman".to_owned()),
        ),
        (
            "devastator_2",
            Some(28),
            [-3590.0, 1300.0, -12909.0],
            180.0,
            1,
            5,
            true,
            None,
            Some("MSG_BJOHN_NAME"),
            false,
            SpawnDispatch::VehicleRecord("devastator".to_owned()),
        ),
        (
            "wingman_2",
            Some(0xFFFFFFFF),
            [-3421.0, 1350.0, -13114.0],
            180.0,
            1,
            5,
            true,
            Some("devastator_2"),
            Some("MSG_BETTY_NAME"),
            false,
            SpawnDispatch::VehicleRecord("wingman".to_owned()),
        ),
        (
            "rusdevastator_5",
            Some(24),
            [-5146.36, 1100.0, -9893.7],
            -20.0,
            2,
            1,
            true,
            None,
            None,
            false,
            SpawnDispatch::VehicleRecord("rusdevastator".to_owned()),
        ),
        (
            "rusdevastator_4",
            Some(24),
            [-5403.59, 1150.0, -9880.1],
            -20.0,
            2,
            1,
            true,
            None,
            None,
            false,
            SpawnDispatch::VehicleRecord("rusdevastator".to_owned()),
        ),
        (
            "bsfury_2",
            Some(27),
            [-5794.054, 1155.66, -2527.709],
            0.0,
            2,
            4,
            false,
            None,
            None,
            false,
            SpawnDispatch::VehicleRecord("bsfury".to_owned()),
        ),
        (
            "rusdevastator_1",
            Some(24),
            [-5364.11, 1300.0, -10057.0],
            -20.0,
            2,
            1,
            true,
            None,
            None,
            false,
            SpawnDispatch::VehicleRecord("rusdevastator".to_owned()),
        ),
        (
            "rusdevastator_2",
            Some(24),
            [-5675.75, 1220.0, -10036.5],
            -20.0,
            2,
            1,
            true,
            None,
            None,
            false,
            SpawnDispatch::VehicleRecord("rusdevastator".to_owned()),
        ),
        (
            "rusdevastator_3",
            Some(25),
            [-4993.029, 1409.176, -5411.109],
            0.0,
            2,
            2,
            false,
            None,
            None,
            false,
            SpawnDispatch::VehicleRecord("rusdevastator".to_owned()),
        ),
    ];
    assert_eq!(
        aircraft.len(),
        rows.len(),
        "M06's aiv.zrd holds a header plus twelve aircraft records"
    );
    for (
        aircraft,
        (name, netids, position, heading, team, group, enabled, target, title, asleep, dispatch),
    ) in aircraft.iter().zip(&rows)
    {
        assert_eq!(aircraft.name.as_str(), *name);
        assert_eq!(aircraft.field_count, AIV_FIELD_COUNT);
        assert_eq!(
            aircraft.netids,
            netids.map(Netids::One),
            "{name}: the measured netids"
        );
        let pose = aircraft
            .stored_pose
            .unwrap_or_else(|| panic!("{name}: pose"));
        assert_eq!(pose.position, *position, "{name}: stored position");
        assert_eq!(pose.heading, *heading, "{name}: stored heading");
        assert_eq!(aircraft.team, Some(*team), "{name}: team");
        assert_eq!(aircraft.group, Some(*group), "{name}: group");
        assert_eq!(aircraft.enabled, Some(*enabled), "{name}: enabled");
        assert_eq!(
            aircraft.primary_target.as_deref(),
            *target,
            "{name}: primary target"
        );
        assert_eq!(aircraft.title.as_deref(), *title, "{name}: title");
        assert_eq!(
            aircraft.starts_asleep,
            Some(*asleep),
            "{name}: only bsfury_1 carries the non-zero deactivated field, and \
             it is the aircraft M06's WAKEUP_ENEMIES (OBJECTIVE13) wakes"
        );
        assert_eq!(&aircraft.dispatch, dispatch, "{name}: name dispatch");
        assert_eq!(
            aircraft.provenance.source.as_ref(),
            Some(&source),
            "{name}: the member's span"
        );
    }
}

fn image_bytes() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    std::fs::read(path).expect("the engine image is readable")
}

/// The committed `[aiv_layout]` row cites bytes that are really there: each
/// deciding instruction is re-read from `$CS_ENGINE_IMAGE`, and the cited
/// `vehicle.zrd` site really pushes that member name.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_aiv_layout_image_sites_match_the_committed_table() {
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    let layout = facts.aiv_layout();
    let image = image_bytes();
    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("the va is inside the image"))
            .expect("the offset fits")
    };
    let check = |va: u64, expected: &[u8], what: &str| {
        let offset = at(va);
        assert_eq!(
            &image[offset..offset + expected.len()],
            expected,
            "{what} at va 0x{va:x}"
        );
    };
    // The record parser: `mov eax, dword fs:[0]` prologue.
    check(
        layout.parse_va,
        &[0x64, 0xa1, 0x00, 0x00, 0x00, 0x00],
        "parser",
    );
    // The per-record spawn loop: `mov ecx, dword [0x64f620]` — the aiv
    // record container #770 measured.
    check(
        layout.spawn_va,
        &[0x8b, 0x0d, 0x20, 0xf6, 0x64, 0x00],
        "spawn loop",
    );
    // The vehicle loader: `push ebp` (function entry).
    check(layout.loader_va, &[0x55, 0x8b, 0xec], "loader");
    // The deactivated byte: `mov al, byte [ebx + 0x40]`.
    check(layout.sleep_va, &[0x8a, 0x43, 0x40], "sleep test");
    // The team: `mov eax, dword [edi + 0x34]`.
    check(layout.team_va, &[0x8b, 0x47, 0x34], "team read");
    // The group: `mov eax, dword [edi + 0x38]`.
    check(layout.group_va, &[0x8b, 0x47, 0x38], "group read");
    // The enabled gate: `mov eax, dword [ebp + 0x3c]`.
    check(layout.spawn_gate_va, &[0x8b, 0x45, 0x3c], "enabled gate");
    // The generic name dispatch: `mov eax, [ebp + 8]; push 0x5f` — the
    // record name and the '_' of `strrchr(name, '_')`.
    check(
        layout.name_dispatch_va,
        &[0x8b, 0x45, 0x08, 0x6a, 0x5f],
        "name dispatch",
    );
    // The vehicle.zrd site: `push 0x627b18`, whose string is `vehicle.zrd`.
    check(
        layout.vehicle_zrd_va,
        &[0x68, 0x18, 0x7b, 0x62, 0x00],
        "vehicle.zrd push",
    );
    let string_va = u32::from_le_bytes(
        image[at(layout.vehicle_zrd_va) + 1..at(layout.vehicle_zrd_va) + 5]
            .try_into()
            .expect("four bytes"),
    );
    let offset = at(u64::from(string_va));
    assert_eq!(
        &image[offset..offset + b"vehicle.zrd".len()],
        b"vehicle.zrd",
        "the pushed pointer names the vehicle.zrd member"
    );
}
