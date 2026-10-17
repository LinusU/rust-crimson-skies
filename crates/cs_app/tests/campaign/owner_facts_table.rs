//! OWNER-FACTS-TABLE (#1432): the committed owner-static engine-facts table
//! `data/owner-static/engine-facts.toml` stands in for `CS_ENGINE_IMAGE` on
//! a normal launch — M01's player airframe (`pdevastator`) and the stored
//! heading convention are committed facts with citations, never
//! `verified_original`.
//!
//! Four things are pinned:
//!
//! * the committed file parses under its declared strict-TOML subset and its
//!   rows are self-consistent (eleven airframe rows at the measured stride,
//!   the M01 row's `record` is its airframe row's own `p` field);
//! * [`MissionStartConfiguration::bind_engine_facts`] binds M01's airframe
//!   and pose identically to [`MissionStartConfiguration::bind_engine_state`]
//!   — same claims, same spans — and keeps every other mission's airframe
//!   refused, naming both `CS_ENGINE_IMAGE` and the table;
//! * against the owner's installation, the M01 plan is launchable and its
//!   content prepares with only `CS_GAME_DIR` set — the ignored test never
//!   names `CS_ENGINE_IMAGE`;
//! * against the owner's decrypted image, every cited byte is read back out
//!   of the image itself, so the committed table cannot silently drift.

use std::path::{Path, PathBuf};

use cs_app::engine_facts::{ENGINE_FACTS_PATH, MissionAirframe, engine_facts};
use cs_app::mission_launch::{LaunchSurface, plan_mission_launch};
use cs_app::mission_session::prepare_mission_content;
use cs_app::mission_start::{
    AIRFRAME_TABLE, CAMPAIGN_AIRFRAME_ROW, ENGINE_IMAGE, ENGINE_IMAGE_SHA256,
    MissionStartConfiguration, STORED_HEADING_DEGREES_TO_RADIANS, STORED_POSITION_METRES_PER_UNIT,
    engine_state_source, recover_retail_start_configuration, stored_heading_radians,
};
use cs_content::campaign_bindings::{MissionLabel, SourceContext};
use cs_content::coordinates::{load_engine_image, original_image_digest};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ClaimStatus;
use cs_types::install::RelativePath;

/// A player record with M01's measured stored pose (#676), synthetic.
fn player_document() -> ZrdValue {
    let mut fields = vec![ZrdValue::Int(0); 8];
    fields[0] = ZrdValue::Int(u32::MAX);
    fields[1] = ZrdValue::List(vec![
        ZrdValue::Float(-3694.0),
        ZrdValue::Float(1318.0),
        ZrdValue::Float(-12482.0),
    ]);
    fields[2] = ZrdValue::Float(170.0);
    ZrdValue::List(vec![
        ZrdValue::List(vec![ZrdValue::Int(1)]),
        ZrdValue::List(vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::List(fields),
        ]),
    ])
}

fn span(offset: u64, length: u64) -> SourceSpan {
    SourceSpan::new(
        original_image_digest(),
        ENGINE_IMAGE,
        None,
        offset,
        length,
        None,
    )
    .expect("a span")
}

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

/// The committed table parses, spells the measured rows and carries the
/// required citations.
#[test]
fn accept_owner_facts_table_parses_and_spells_the_measured_facts() {
    let facts = engine_facts().expect("the committed table parses");

    // The evidence image is the one the codebase measures — a table citing a
    // different digest would be a claim about other bytes.
    assert_eq!(facts.image(), ENGINE_IMAGE);
    assert_eq!(facts.image_sha256(), ENGINE_IMAGE_SHA256);

    // The heading convention and position unit are the production
    // constants — a table disagreeing is a drift, refused at bind time and
    // pinned here directly.
    assert_eq!(
        facts.heading().degrees_to_radians,
        STORED_HEADING_DEGREES_TO_RADIANS
    );
    assert_eq!(facts.position().metres_per_unit, 1.0);
    assert_eq!(
        facts.position().metres_per_unit,
        f64::from(STORED_POSITION_METRES_PER_UNIT)
    );
    for needle in ["+Z", "+X", "Ry"] {
        assert!(
            facts.heading().compose.contains(needle)
                || facts.heading().positive.contains(needle)
                || facts.heading().zero.contains(needle),
            "the heading convention must record {needle:?}"
        );
    }

    // The eleven-row airframe table is the one the code measures — every
    // measured column agrees, and the uninterpreted pointers are
    // transcribed whole (row 5's odd `wingman` included).
    assert_eq!(facts.airframes().len(), AIRFRAME_TABLE.len());
    for (position, (row, code)) in facts
        .airframes()
        .iter()
        .zip(AIRFRAME_TABLE.iter())
        .enumerate()
    {
        assert_eq!(row.index as usize, position);
        assert_eq!(row.display, code.display_name);
        assert_eq!(row.scene_root, code.scene_root);
        assert_eq!(row.model, code.model);
        assert!(row.claim.starts_with("OWNER-STATIC-"), "{}", row.claim);
        assert!(!row.p.is_empty() && !row.r.is_empty() && !row.w.is_empty());
    }
    let devastator = facts.airframe(CAMPAIGN_AIRFRAME_ROW as u32).expect("row 5");
    assert_eq!(
        (
            devastator.display.as_str(),
            devastator.scene_root.as_str(),
            devastator.model.as_str(),
            devastator.p.as_str(),
            devastator.w.as_str()
        ),
        (
            "Devastator",
            "player_pfighter",
            "piratefighter",
            "pdevastator",
            "wingman"
        )
    );

    // One row per campaign mission directory: M01 commits `pdevastator`;
    // every other is an explicit unknown with a reason, never a guess.
    let known: Vec<_> = facts
        .missions()
        .iter()
        .filter(|row| matches!(row.airframe, MissionAirframe::Known { .. }))
        .collect();
    assert_eq!(known.len(), 1, "only M01's airframe is committed");
    let m01 = &known[0];
    assert_eq!(m01.dir, "zbd/c1c/m01");
    let MissionAirframe::Known { index, record, .. } = &m01.airframe else {
        panic!("M01's row is known");
    };
    assert_eq!(*index, CAMPAIGN_AIRFRAME_ROW as u32);
    assert_eq!(record, "pdevastator");
    for row in facts.missions() {
        if let MissionAirframe::Unknown { reason } = &row.airframe {
            assert!(
                !reason.trim().is_empty(),
                "{} refuses without a reason",
                row.dir
            );
        }
    }

    // The designed values are labelled, not measured.
    let speed = facts
        .provisional("player_spawn_speed_m_s")
        .expect("the spawn speed is recorded as designed");
    assert_eq!(speed.status, "designed");
    assert_eq!(speed.value, 55.0);
}

/// Without the image, the committed table binds M01's airframe and pose to
/// the same claims and spans the image itself would produce.
#[test]
fn accept_owner_facts_table_binds_m01_like_the_image_does() {
    let facts = engine_facts().expect("the committed table parses");
    let source = span(16, 64);
    let why = "$CS_ENGINE_IMAGE is unset in this test";

    let mut config = MissionStartConfiguration::read("zbd/c1c/m01", &player_document(), &source)
        .expect("the synthetic record reads");
    config
        .bind_engine_facts(facts, why)
        .expect("the table binds");

    let Resolved::Known(airframe) = config.airframe() else {
        panic!("M01's airframe is committed");
    };
    assert_eq!(
        airframe.value,
        ContentId::from_source(ContentKind::Airframe, "player_pfighter").expect("a valid id")
    );
    assert_eq!(airframe.provenance.class, ClaimStatus::ObservedTool);
    // The span is the same citation bind_engine_state produces for the
    // measured image: the roster record's deciding bytes.
    let image_engine = engine_state_source(&cs_content::coordinates::EngineImage {
        path: PathBuf::from("/owner").join(ENGINE_IMAGE),
        bytes: Vec::new(),
        digest: original_image_digest(),
    })
    .expect("the measured image names");
    assert_eq!(
        airframe.provenance.source.as_ref(),
        Some(&image_engine.airframe),
        "the committed-table binding cites the same image span"
    );

    let Resolved::Known(pose) = config.initial_pose() else {
        panic!("M01's pose convention is committed");
    };
    assert_eq!(pose.value.position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(pose.value.heading, stored_heading_radians(170.0));
    assert_eq!(
        pose.provenance.source.as_ref(),
        Some(&image_engine.heading),
        "the committed-table binding cites the same heading span"
    );
}

/// A mission whose row is `unknown`, and one with no row at all, both stay
/// refused — naming `CS_ENGINE_IMAGE` and the committed table — while the
/// pose still binds through the committed convention.
#[test]
fn accept_owner_facts_table_unknown_missions_refuse_with_both_sources_named() {
    let facts = engine_facts().expect("the committed table parses");
    let source = span(16, 64);
    let why = "$CS_ENGINE_IMAGE is unset in this test";

    // `zbd/c4/m02` (M17's directory) carries an explicit unknown row.
    let mut config = MissionStartConfiguration::read("zbd/c4/m02", &player_document(), &source)
        .expect("the synthetic record reads");
    config
        .bind_engine_facts(facts, why)
        .expect("the table binds");
    let Resolved::Unknown { reason, .. } = config.airframe() else {
        panic!("no per-mission airframe is committed for zbd/c4/m02");
    };
    for needle in ["CS_ENGINE_IMAGE", ENGINE_FACTS_PATH, "unknown"] {
        assert!(
            reason.contains(needle),
            "the refusal must name {needle:?}: {reason}"
        );
    }
    assert!(
        config.initial_pose().is_known(),
        "the heading convention is global, not per-mission"
    );

    // A directory the table does not cover refuses by naming that absence.
    let mut config = MissionStartConfiguration::read("zbd/c9/m99", &player_document(), &source)
        .expect("the synthetic record reads");
    config
        .bind_engine_facts(facts, why)
        .expect("the table binds");
    let Resolved::Unknown { reason, .. } = config.airframe() else {
        panic!("an unlisted mission's airframe stays unknown");
    };
    for needle in ["CS_ENGINE_IMAGE", ENGINE_FACTS_PATH, "no", "zbd/c9/m99"] {
        assert!(
            reason.contains(needle),
            "the refusal must name {needle:?}: {reason}"
        );
    }
}

/// Against the owner's installation, the committed-table leg binds M01's
/// airframe and pose end to end — this never reads `CS_ENGINE_IMAGE`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_owner_facts_table_retail_m01_binds_through_the_committed_table() {
    let root = game_dir();
    let (document, source) = member_document(&root, "zbd/c1c/m01/zrdr.zbd", "aiv.zrd");
    let mut config = MissionStartConfiguration::read("zbd/c1c/m01", &document, &source)
        .expect("M01's aircraft table reads");
    config
        .bind_engine_facts(
            engine_facts().expect("the committed table parses"),
            "$CS_ENGINE_IMAGE is unset in this test",
        )
        .expect("the table binds");

    let Resolved::Known(airframe) = config.airframe() else {
        panic!("M01's airframe is committed: {:?}", config.airframe());
    };
    assert_eq!(airframe.value.key(), "player_pfighter");
    assert_eq!(airframe.provenance.class, ClaimStatus::ObservedTool);
    let Resolved::Known(pose) = config.initial_pose() else {
        panic!("M01's pose is committed: {:?}", config.initial_pose());
    };
    assert_eq!(pose.value.position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(pose.value.heading, stored_heading_radians(170.0));
}

/// The whole launch gate: with only `CS_GAME_DIR` named, M01's plan is
/// launchable and its content prepares. Wherever `CS_ENGINE_IMAGE` happens
/// to point, this test runs the same production path the binary does —
/// under `env -u CS_ENGINE_IMAGE` it is the committed table that binds.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_owner_facts_table_retail_m01_plans_and_prepares() {
    let root = game_dir();
    let label = MissionLabel::new("M06").expect("the label is valid");
    let plan = plan_mission_launch(&root, label, "The Red Menace").expect("M06's plan");
    assert!(
        plan.launchable(),
        "M01 must be launchable without naming the image; gaps: {:?}",
        plan.gaps().collect::<Vec<_>>()
    );
    let report = plan
        .surface(LaunchSurface::PlayerConfiguration)
        .expect("the player surface reports");
    assert!(
        report.verdict.is_satisfied(),
        "the player configuration binds: {:?}",
        report.verdict
    );

    let content = prepare_mission_content(&root, &plan).expect("M01's content prepares");
    assert_eq!(
        content.flight.record, "pdevastator",
        "the committed table's row-5 record drives the flight law"
    );
}

/// Every campaign mission directory the installation scans has a committed
/// `[[mission]]` row, so no launchable dir is silently unlisted.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_owner_facts_table_retail_inventory_covers_every_campaign_dir() {
    let root = game_dir();
    let context = SourceContext::read(&root).expect("the source context reads");
    let facts = engine_facts().expect("the committed table parses");
    let dirs: Vec<String> = context
        .campaign()
        .iter()
        .map(|mission| {
            mission
                .program_asset
                .rsplit_once('/')
                .map(|(dir, _)| dir.to_ascii_lowercase())
                .expect("a program asset names its directory")
        })
        .collect();
    assert_eq!(
        dirs.len(),
        facts.missions().len(),
        "the committed table must cover exactly the scanned campaign missions: {dirs:?}"
    );
    for dir in &dirs {
        assert!(
            facts.mission(dir).is_some(),
            "{dir} has no committed mission row"
        );
    }
}

/// With the owner's image named, every fact the table commits is read back
/// out of the image's own bytes — the drift guard.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_owner_facts_table_image_rows_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let facts = engine_facts().expect("the committed table parses");
    assert_eq!(
        loaded.digest,
        original_image_digest(),
        "the table cites the measured image"
    );
    let image = loaded.bytes.as_slice();

    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("the va is inside the image"))
            .expect("an offset")
    };
    let cstr = |va: u64| -> String {
        let offset = at(va);
        let end = image[offset..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|nul| offset + nul)
            .expect("a NUL terminator");
        String::from_utf8(image[offset..end].to_vec()).expect("the table's strings are utf-8")
    };
    let u32_at = |offset: usize| -> u32 {
        u32::from_le_bytes(image[offset..offset + 4].try_into().expect("four bytes"))
    };
    // A row field is a VA pointing at its C-string in the image.
    let pointer = |va: u64| -> String { cstr(u64::from(u32_at(at(va)))) };

    // Each committed airframe row is the image's own seven pointers.
    assert_eq!(facts.airframes().len(), 11);
    for row in facts.airframes() {
        // The image's own pointer order: display, scene_root, p, r, w,
        // model, base — the committed row's fields spell the same order.
        let fields = [
            row.display.as_str(),
            row.scene_root.as_str(),
            row.p.as_str(),
            row.r.as_str(),
            row.w.as_str(),
            row.model.as_str(),
            row.base.as_str(),
        ];
        for (field, expected) in fields.iter().enumerate() {
            assert_eq!(
                &pointer(row.va + (field as u64) * 4),
                expected,
                "airframe row {} field {field} drifted",
                row.index
            );
        }
    }

    // The mission row's deciding record carries the committed index at +0x2c.
    let m01 = facts.mission("zbd/c1c/m01").expect("the M01 row");
    let MissionAirframe::Known { index, record, va } = &m01.airframe else {
        panic!("M01's row is committed");
    };
    assert_eq!(
        u32_at(at(*va) + 0x2c),
        *index,
        "the roster record's +0x2c dword drifted from the committed airframe index"
    );
    assert_eq!(record, &facts.airframe(*index).expect("the row").p);

    // The heading convention: the f64 at `constant_va` is the committed
    // double, and the sequence at `va` multiplies by the dword that names it.
    let heading = facts.heading();
    let constant = f64::from_le_bytes(
        image[at(heading.constant_va)..at(heading.constant_va) + 8]
            .try_into()
            .expect("eight bytes for the double"),
    );
    assert_eq!(
        constant, heading.degrees_to_radians,
        "the image's π/180 double drifted from the committed value"
    );
    // The sequence's `fmul` names the constant's address as its operand —
    // the pointer-level fact the committed `constant_va` records (the
    // instruction starts at va+0x9; its dword operand sits at +0xb).
    assert_eq!(
        u64::from(u32_at(at(heading.va) + 0xb)),
        heading.constant_va,
        "the conversion sequence's constant operand drifted"
    );

    // M01's production path still binds from the image, and the committed
    // table's M01 agrees.
    if std::env::var_os("CS_GAME_DIR").is_some() {
        let config =
            recover_retail_start_configuration(&game_dir(), "zbd/c1c/m01").expect("M01 reads");
        let Resolved::Known(airframe) = config.airframe() else {
            panic!("M01's airframe binds from the image");
        };
        assert_eq!(
            airframe.value.key(),
            facts.airframe(*index).expect("the row").scene_root.as_str(),
            "the image path and the committed row must name the same scene root"
        );
    }
}
