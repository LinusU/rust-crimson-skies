//! M01-C3-PLAYER-CONFIG (Rally #1580): the launch plan's
//! `player_configuration` surface for the real M01 — "The Lost Treasure",
//! `mission/ch1-m01`, `world/c3`, program archive `ZBD/C3/M01/zrdr.zbd`
//! (#1565's corrected layout) — is `Satisfied` and names its sources.
//!
//! The two facts the surface needs both bind through
//! [`recover_retail_start_configuration`], the same production function
//! `mission_launch::measure_player` calls:
//!
//! * **airframe** — the measured fresh-profile campaign default (#770):
//!   the `.data` plane-roster record at image VA `0x61a81c` whose `+0x2c`
//!   dword is 5, row 5 of the executable's airframe table — `Devastator`,
//!   scene root `player_pfighter`, model `piratefighter`, vehicle record
//!   `pdevastator`. With `CS_ENGINE_IMAGE` the span names the image bytes
//!   directly; without it the committed `zbd/c3/m01` row of
//!   `data/owner-static/engine-facts.toml` names the same span.
//! * **start pose** — the mission archive's own `aiv.zrd` `player` record:
//!   field 1 the position in metres (scale 1.0, #436) and field 2 the
//!   stored heading in degrees, converted through the image's own
//!   π/180 double by the sequence at VA `0x47c4e5` (#770). C3/M01 stores
//!   `(-1426, 150, -1813)` and `40`.
//!
//! The pre-correction `zbd/c1c/m01` rows were M06's ("The Red Menace",
//! `ch2-m01`): they stay committed as `unknown`, like every other mission
//! reached only through campaign progression — no per-mission claim is
//! measured for them.

use std::path::{Path, PathBuf};

use crate::campaign::common::label;
use cs_app::engine_facts::engine_facts;
use cs_app::mission_launch::{LaunchSurface, plan_mission_launch};
use cs_app::mission_start::{
    ENGINE_IMAGE, MissionStartConfiguration, recover_retail_start_configuration,
    stored_heading_radians,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Resolved;
use cs_types::evidence::ClaimStatus;
use cs_types::install::RelativePath;

/// The member decode path `recover_retail_start_configuration` walks for a
/// mission's `aiv.zrd` — the production discovery and decoder, so the
/// committed-table leg is exercised on real retail bytes.
fn member_document(root: &Path, container_key: &str, member: &str) -> (ZrdValue, SourceSpan) {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key.to_ascii_lowercase())
        .unwrap_or_else(|| panic!("the installation has {container_key}"));
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = RelativePath::new(&spelling.to_lowercase()).expect("a path");
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

/// C3/M01's `aiv.zrd` `player` record carries the stored pose this
/// configuration reports: position `(-1426, 150, -1813)` metres and heading
/// `40` stored degrees.
fn assert_m01_configuration(configuration: &MissionStartConfiguration) {
    let Resolved::Known(airframe) = configuration.airframe() else {
        panic!(
            "the player airframe binds measured: {:?}",
            configuration.airframe()
        );
    };
    assert_eq!(airframe.value.key(), "player_pfighter");
    assert_eq!(airframe.provenance.class, ClaimStatus::ObservedTool);
    let airframe_source = airframe.provenance.source.as_ref().expect("a span");
    assert_eq!(airframe_source.container_path(), ENGINE_IMAGE);
    assert_eq!(airframe_source.offset(), 0x21a81c);

    let Resolved::Known(stored) = configuration.stored_pose() else {
        panic!("the player record has the pose shape");
    };
    assert_eq!(stored.value.position, [-1426.0, 150.0, -1813.0]);
    assert_eq!(stored.value.heading, 40.0);
    let pose_source = stored.provenance.source.as_ref().expect("a span");
    assert_eq!(pose_source.member_key(), Some("aiv.zrd"));
    assert!(
        pose_source
            .container_path()
            .eq_ignore_ascii_case("ZBD/C3/M01/zrdr.zbd"),
        "container {}",
        pose_source.container_path()
    );

    let Resolved::Known(pose) = configuration.initial_pose() else {
        panic!("the initial pose binds through the measured conversion");
    };
    assert_eq!(pose.value.position, [-1426.0, 150.0, -1813.0]);
    assert_eq!(pose.value.heading, stored_heading_radians(40.0));
    let pose_provenance = pose.provenance.source.as_ref().expect("a span");
    assert_eq!(pose_provenance.container_path(), ENGINE_IMAGE);
    assert_eq!(pose_provenance.offset(), 0x7c4e5);
}

/// The launch plan's `player_configuration` surface is `Satisfied` for the
/// real M01 and names its production consumer, and the configuration it
/// judges names the image spans and the archive member the answers come
/// from. `recover_retail_start_configuration` picks the image leg when
/// `CS_ENGINE_IMAGE` is set and the committed-table leg when it is not;
/// both cite the same measured spans, so this asserts the sources either
/// way.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_c3_player_config_retail_plan_satisfied_and_names_sources() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let report = plan
        .surface(LaunchSurface::PlayerConfiguration)
        .expect("the player-configuration surface reports");
    assert!(
        report.verdict.is_satisfied(),
        "player_configuration is satisfied: {}",
        report.verdict.describe()
    );
    let cs_app::mission_launch::SurfaceVerdict::Satisfied { consumer } = &report.verdict else {
        unreachable!();
    };
    assert!(
        consumer.contains("mission_start::MissionStartConfiguration"),
        "the verdict names the consumer the surface binds: {consumer}"
    );

    assert_m01_configuration(
        &recover_retail_start_configuration(&root, "zbd/c3/m01").expect("M01 reads"),
    );
}

/// Without naming the image, the committed owner-static table's `zbd/c3/m01`
/// row binds the same airframe and pose on real retail bytes — the leg a
/// normal launch stands on.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_c3_player_config_committed_table_binds_m01() {
    let root = game_dir();
    let (document, source) = member_document(&root, "zbd/c3/m01/zrdr.zbd", "aiv.zrd");
    let mut configuration = MissionStartConfiguration::read("zbd/c3/m01", &document, &source)
        .expect("M01's aircraft table reads");
    configuration
        .bind_engine_facts(
            engine_facts().expect("the committed table parses"),
            "$CS_ENGINE_IMAGE is unset in this test",
        )
        .expect("the table binds");
    assert_m01_configuration(&configuration);
}
