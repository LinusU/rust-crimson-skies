//! Acceptance stage VS-M01-RT-CONTENT: the content-preparation read
//! (`crates/cs_app/src/mission_session/`, Rally #1214).
//!
//! [`MissionContent::prepare`] is the one production entry that gathers every
//! record a windowed mission composition consumes out of a satisfied
//! [`MissionLaunchPlan`], through the readers the launch closure already
//! judges. Two things have to be proved about it, and this file proves both:
//!
//! * **A refusal names its source.** An installation nothing can be read from
//!   is refused with the path or logical key the reader failed at — never a
//!   partially filled `MissionContent`, never a default (AGENTS.md rules 4
//!   and 5). The synthetic member drives that path in CI.
//! * **Against the owner's installation, M01's content prepares every
//!   record.** The retail member runs `prepare` over M01's real plan and
//!   asserts each record the composition will consume.
//!
//! # The objectives source, since the owner's amendment (2026-10-10)
//!
//! Acceptance item 7 originally asked for the `ObjectiveRecovery::program()`
//! path (`crates/cs_app/src/objectives.rs:5609`), which returns
//! `Err(ObjectiveRecoveryRefusal)` **unconditionally** — its own doc says
//! "always today" — so no original mission's content could ever prepare.
//! The owner decided (Rally #1214, 2026-10-10, option 2): M01's declared
//! objective program **is** the measured control/directive lowering
//! (`mission_control::survey_mission_control_programs` → the mission's row →
//! `lowering()` → `complete()` → `lowering_attempt().program()`), the same
//! program the `mission_objectives` launch surface is judged by
//! (`crates/cs_app/src/mission_launch.rs:908-921`). "Objectives program
//! lowers" in the acceptance list therefore means that lowering completes —
//! asserted below through `content.control` — and the separate
//! `ObjectiveRecovery` path stays in `objectives.rs`, measured by #1219,
//! consulted by no reader here. The measurement that forced the amendment is
//! recorded in
//! `docs/findings/2026-10-10-vs-m01-rt-content-objectives-program-refuses-and-plan-premises.md`.

use std::path::PathBuf;

use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::prepare_mission_content;
use cs_content::campaign_bindings::MissionLabel;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Resolved};

use crate::common::label;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: VS-M01-RT-CONTENT needs the retail capability; run this \
             suite with `--include-ignored` and CS_GAME_DIR pointing at the read-only \
             installation"
        )
    }))
}

/// The identity a plan is made of, for a directory that holds no installation
/// at all: the refusal under test is about the reader, not about binding a
/// campaign, so the plan's own ids are spelled directly.
fn empty_plan() -> MissionLaunchPlan {
    MissionLaunchPlan {
        label: MissionLabel::new("M01").expect("M01 is a valid work-order label"),
        catalog_id: ContentId::from_source(ContentKind::Mission, "ch1-m01")
            .expect("the catalog id is valid"),
        world_id: ContentId::from_source(ContentKind::World, "c1c").expect("the world id is valid"),
        program_id: ContentId::from_source(ContentKind::Script, "c1c-m01-zrdr")
            .expect("the program id is valid"),
        install_sha256: "0".repeat(64),
        mission_dir: "zbd/c1c/m01".to_owned(),
        group_dir: "zbd/c1c".to_owned(),
        surfaces: Vec::new(),
    }
}

/// **An installation that cannot be read is refused with its source, never
/// planned around or filled in.**
///
/// The scratch directory holds no installation at all, so the first reader
/// `prepare` reaches refuses; the caller gets that reader's own message with
/// the path or the logical key it failed at, and no `MissionContent` exists
/// to be mistaken for a prepared one.
#[test]
fn accept_vs_m01_runtime_content_an_unreadable_installation_is_refused_with_its_source() {
    let root = std::env::temp_dir().join(format!(
        "cs_vs_m01_rt_content_empty_{}_{}",
        std::process::id(),
        "refusal"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the scratch directory can be created");

    let error = prepare_mission_content(&root, &empty_plan())
        .expect_err("an empty directory cannot prepare a mission's content");
    let text = error.to_string();
    let names_path = text.contains(&root.display().to_string());
    let names_key = text.contains("zbd/c1c/gamez.zbd") || text.contains("zrdr.zbd");
    assert!(
        names_path || names_key,
        "the refusal must name the source it failed at, got: {text}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// **Against the owner's installation, M01's content prepares: every record
/// the composition consumes is read through its production reader, and the
/// declared objective program — the measured control/directive lowering —
/// lowers completely.**
///
/// Each assertion pins one record of the nine-item list; a refusal in any
/// reader would have kept `prepare` from returning at all.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_content_m01_prepares_every_record() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");

    let content = prepare_mission_content(&root, &plan)
        .expect("M01's content prepares through the production readers");

    // 1. World: the definition imported from the mission's group container,
    // and the engine meshes the definition names.
    assert!(
        !content.world.definition().objects().is_empty(),
        "M01's world definition must import objects from {}",
        plan.group_dir
    );
    assert!(
        content.meshes.len() > 0,
        "M01's world must upload the meshes its definition names"
    );

    // 2. Player start: the airframe and the initial pose, both Known.
    let Resolved::Known(airframe) = content.start.airframe() else {
        panic!("M01's start airframe must resolve Known");
    };
    assert_eq!(
        airframe.value.key(),
        "player_pfighter",
        "the campaign start must resolve the measured scene root"
    );
    assert!(
        matches!(content.start.initial_pose(), Resolved::Known(_)),
        "M01's start pose must resolve Known"
    );

    // 3. Player flight law: the measured row's record, imported with its
    // fuel load (an explicit unknown would have refused, never zero).
    assert_eq!(
        content.flight.record, "pdevastator",
        "the campaign airframe row must map the pdevastator record"
    );
    assert!(
        content.flight.fuel.is_finite() && content.flight.fuel > 0.0,
        "the pdevastator record must import a positive fuel load, got {}",
        content.flight.fuel
    );

    // 4. Environment: the bound weather session, started at tick zero of the
    // composition's timeline.
    assert_eq!(
        content.environment.clock().tick(),
        Tick(0),
        "the weather session must start at tick zero"
    );

    // 5. World actors: satisfied, and the three actors this scope places.
    assert!(
        content.world_actors.is_satisfied(),
        "M01's world-actor binding must be satisfied"
    );
    assert_eq!(
        content.world_actors.rows().len(),
        3,
        "M01's scope must place its three measured world actors"
    );

    // 6. Animations: the join bound against this mission's scope and its
    // world group's archives.
    assert_eq!(
        content.animation.scope(),
        plan.mission_dir,
        "the animation join must bind M01's own scope"
    );
    assert_eq!(
        content.animation.group(),
        plan.group_dir.trim_start_matches("zbd/"),
        "the animation join must bind M01's world group"
    );
    assert!(
        !content.animation.archives().is_empty(),
        "the animation join must name the reader archives it read"
    );

    // 7 + 8. Objectives and script host: one measured control/directive
    // lowering (the owner's 2026-10-10 amendment), lowered completely into
    // the program the composition runs — objectives included.
    assert!(
        !content.control.objectives.is_empty(),
        "M01's control lowering must produce a program with declared objectives"
    );

    // 9. Audio: at least one sound-family archive in the mission's scope.
    assert!(
        !content.sound_archives.is_empty(),
        "M01's scope must hold at least one sound-family archive"
    );
}
