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
//! * **Against the owner's installation, the read runs to its end.** The
//!   retail member runs `prepare` over M01's real plan and asserts where it
//!   stops.
//!
//! # Where it stops today, and why
//!
//! The retail member asserts an `Objectives` refusal, not a prepared value:
//! `ObjectiveRecovery::program()` (`crates/cs_app/src/objectives.rs:5609`)
//! returns `Err(ObjectiveRecoveryRefusal)` **unconditionally** — its own doc
//! says "always today" — and M01's `objectives.zrd` reads 58 blocks / 358
//! fields with none of them recovered
//! (`accept_m01_lc_objectives_retail_m01_drops_no_record` ends in
//! `recovery.program().expect_err(...)`). Item 7 of #1214 therefore cannot
//! succeed for M01, and a test that claimed M01's content "prepares" would be
//! claiming something production does not do.
//!
//! Preparation reads the objective recovery **last**, so an `Objectives`
//! refusal is also the proof that every other reader on the list succeeded:
//! a failure in the world, the start configuration, the flight record, the
//! weather, the world actors, the animation join, the control program or the
//! sound walk would have been reported under its own variant instead.
//! `docs/findings/2026-10-10-vs-m01-rt-content-objectives-program-refuses-and-plan-premises.md`
//! holds the measurement and the follow-up (#1219); when a recovery can yield
//! a program for M01, this member becomes the assertion that the value is
//! prepared.

use std::path::PathBuf;

use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::{MissionSessionError, prepare_mission_content};
use cs_content::campaign_bindings::MissionLabel;
use cs_types::content::{ContentId, ContentKind};

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

/// **Against the owner's installation, M01's content prepares everything the
/// production readers can stand behind — and stops exactly at the objective
/// recovery, naming it.**
///
/// The variant alone is the evidence: preparation reads the objective recovery
/// last, so `Objectives` means the world container imported, the start
/// airframe and pose resolved `Known`, the campaign flight record imported,
/// the weather session started, the three world actors launched, the animation
/// join bound, the control lowering completed and the sound walk found an
/// archive — a refusal in any of those would have been reported under its own
/// variant instead.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_content_m01_prepares_every_record_but_the_objectives_program() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");

    let error = prepare_mission_content(&root, &plan)
        .expect_err("M01's objective record yields no program today (see the module doc)");
    match error {
        MissionSessionError::Objectives { mission, reason } => {
            assert_eq!(mission, plan.mission_dir);
            assert!(
                reason.contains("no declared objective program can be recovered"),
                "the refusal must be the recovery's own message, got: {reason}"
            );
            assert!(
                reason.contains(&plan.mission_dir),
                "the refusal must name the mission it was read for, got: {reason}"
            );
        }
        other => panic!(
            "M01 must stop at the objective recovery; every earlier reader succeeded, so any \
             other variant means one of them refused — got: {other}"
        ),
    }
}
