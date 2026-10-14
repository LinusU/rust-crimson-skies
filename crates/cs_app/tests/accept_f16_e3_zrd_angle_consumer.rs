//! F16-E-3 (#720): the first production `.zrd` angle consumer — the
//! `zeppelins.zrd` spawn attitude — converts through the F16-E-measured
//! [`CoordinateSource::retail_zrd`] adapter.
//!
//! The retail case binds M01's carrier through production and asserts, per
//! record, that every converted angle is **bit-identical** to the engine's
//! own rounding of the document's degrees (the π/180 double measured at
//! `0x6040e8`, #770) — the adapter moved where the conversion is declared,
//! from a hand-rolled multiply to the measured source, and changed never
//! the value the original stores. A declaration that stops saying degrees
//! is refused at the conversion; that refusal path is covered by the
//! synthetic `accept_f16_e3_` tests in `cs_app::mission_world_actors`.

use cs_app::mission_start::STORED_HEADING_DEGREES_TO_RADIANS;
use cs_app::mission_world_actors::{AttitudeSource, bind_mission_world_actors};
use cs_assets::install;
use cs_content::coordinates::{AngleUnit, CoordinateSource, SourceAdapter};
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::zeppelins::{ZEPPELINS_MEMBER, read_zeppelins_member};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind};

const ARCHIVE: &str = "zbd/c1c/m01/zrdr.zbd";

/// The engine's own degrees→radians rounding over one stored number (#770):
/// `fld` the record's degrees, `fmul` the image's π/180 double, store the
/// `f32` result — the value the load leaves at `obj+0x2c`/`obj+0x30`.
fn engine_round(degrees: f32) -> f32 {
    (f64::from(degrees) * STORED_HEADING_DEGREES_TO_RADIANS) as f32
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f16_e3_retail_zrd_spawn_angles_convert_through_the_measured_source() {
    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    );
    let found = install::discover(&root).expect("the installation is discoverable");
    let manifest_record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == ARCHIVE)
        .expect("M01's reader archive is installed");
    let bytes =
        std::fs::read(root.join(manifest_record.relative_spelling.as_str())).expect("readable");
    let discovery = discover_container(ARCHIVE, &manifest_record.relative_spelling, &bytes);
    let member = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(ZEPPELINS_MEMBER))
        })
        .expect("zeppelins.zrd is in the archive");
    let carrier = read_zeppelins_member(member.bytes()).expect("decodes");
    let span = member.locator().span();
    let source_span = SourceSpan::new(
        install::fingerprint(&found.manifest),
        ARCHIVE,
        Some(ZEPPELINS_MEMBER),
        span.offset,
        span.len,
        None,
    )
    .expect("the member spans");
    let adapter = SourceAdapter::new(CoordinateSource::retail_zrd(source_span));
    assert_eq!(
        adapter.source().convention().angle_unit(),
        AngleUnit::Degrees,
        "the F16-E measurement spells `.zrd` angle fields in degrees (#390)"
    );

    // The production binding, over the same installation and archive.
    let subject = ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id");
    let bound = bind_mission_world_actors(&root, &found, "zbd/c1c/m01", "zbd/c1c", &subject, 64);
    assert_eq!(bound.rows().len(), carrier.records().len());
    assert!(
        bound
            .open_fields()
            .iter()
            .all(|open| open.field != "orientation"),
        "no retail record's angle conversion refuses: {:?}",
        bound.open_fields()
    );

    let mut spawn_attitudes = 0_usize;
    for record in carrier.records() {
        let row = bound
            .rows()
            .iter()
            .find(|row| row.node == record.node())
            .unwrap_or_else(|| panic!("{} joined a row", record.node()));
        // The two nodes the startup carrier rotates take that write last;
        // the spawn attitude is their named residue, not their orientation
        // (`accept_m01_lc_zeppelin_attitude` composes both sources).
        let Some(AttitudeSource::CarrierSpawn {
            yaw_radians,
            pitch_radians,
        }) = row.attitude_source()
        else {
            continue;
        };
        spawn_attitudes += 1;

        // The converted radians are the engine's own stored values, bit for
        // bit: the adapter's declaration moved, the numbers did not.
        assert_eq!(
            *yaw_radians,
            engine_round(record.yaw()),
            "{}'s yaw is the engine's rounding of the document degrees",
            record.node()
        );
        let min = engine_round(record.min_pitch());
        let max = engine_round(record.max_pitch());
        let pitch = engine_round(record.pitch());
        let clamped = if min <= max {
            pitch.clamp(min, max)
        } else {
            pitch
        };
        assert_eq!(
            *pitch_radians,
            clamped,
            "{}'s pitch is the engine's rounding, clamped to the record's own limits",
            record.node()
        );

        // And the stored degrees really did arrive as radians — a hand-rolled
        // pass-through of the document number would fail this.
        assert!(
            (f64::from(*yaw_radians) - f64::from(record.yaw()).to_radians()).abs() < 1e-6,
            "{}'s {}° arrive as a canonical angle, not as the stored number: {} rad",
            record.node(),
            record.yaw(),
            yaw_radians
        );
    }
    assert_eq!(
        spawn_attitudes, 1,
        "M01 has exactly one record the startup carrier does not rotate"
    );
}
