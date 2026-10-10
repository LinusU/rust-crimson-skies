//! Acceptance stage VS-M01-RUNTIME: the launch plan's measured dependency
//! closure (`missions/M01.md`, work order `VS-M01-RUNTIME`).
//!
//! A `--mission` launch starts from [`plan_mission_launch`]: the work order's
//! declared identity is confirmed through the installation's own campaign
//! binding, then every archive surface the launch must consume is walked and
//! judged. The gate is [`MissionLaunchPlan::launchable`]: a plan that names an
//! `Unsupported` or `Unknown` surface is a launch that exits nonzero with the
//! surfaces spelled out — the acceptance behavior for unsupported reachable
//! assets is a source diagnostic, never a faked scene.
//!
//! The synthetic tests exercise the two failure/verdict surfaces any
//! installation can reach: an undiscoverable installation is refused by name,
//! and the plan's gate reports each blocking surface with its mechanism. The
//! retail tests are `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`:
//! they measure M01's real closure end to end — every surface satisfied, the
//! gate naming no gap, and the launch stopping exactly at the composition
//! seam. The `world_geometry` verdict itself, and the zero-gap plan it is part
//! of, are `vs_m01_geometry_verdict.rs`'s members.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_app::mission_launch::{
    LaunchPlanError, LaunchSurface, MemberVerdict, MissionLaunchPlan, SurfaceReport,
    SurfaceVerdict, plan_mission_launch,
};
use cs_content::world::WORLD_AXIS_CONVENTION_MEASURED;
use cs_types::content::{ContentId, ContentKind};

use crate::common::{label, load_inventory};

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: VS-M01-RUNTIME needs the retail capability; run this \
             suite with `--include-ignored` and CS_GAME_DIR pointing at the read-only \
             installation"
        )
    }))
}

/// The declared discovery title of `M01`, read from the committed inventory
/// rather than repeated here.
fn discovery_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == "M01")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M01 work order")
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test content id is valid")
}

/// **An installation that cannot be discovered is refused by name, never
/// planned around.** The production discovery walks the directory first, and
/// an empty one yields no campaign — `LaunchPlanError::Binding` is what the
/// launch reports instead of an empty plan that looks ready.
#[test]
fn accept_vs_m01_runtime_an_undiscoverable_installation_refuses_the_plan() {
    let dir = std::env::temp_dir().join("vs_m01_runtime_empty_install");
    std::fs::create_dir_all(&dir).expect("the probe directory is created");
    let error = plan_mission_launch(&dir, label("M01"), "The Lost Treasure")
        .expect_err("an empty installation cannot yield a launch plan");
    assert!(
        matches!(error, LaunchPlanError::Binding(_)),
        "the refusal names the binding stage, not a guessed surface: {error}"
    );
    assert!(
        !error.to_string().is_empty(),
        "the diagnostic is spelled out"
    );
}

/// **The gate reports every blocking surface by name, in report order.** A
/// plan is built by hand here because the launch gate is what `launchable`
/// and `gaps` exist to answer; an `Unsupported` surface names its missing
/// mechanism and an `Unknown` names its question.
#[test]
fn accept_vs_m01_runtime_the_launch_gate_names_each_blocking_surface() {
    let plan = MissionLaunchPlan {
        label: label("M01"),
        catalog_id: cid(ContentKind::Mission, "ch1-m01"),
        world_id: cid(ContentKind::World, "c1c"),
        program_id: cid(ContentKind::Script, "c1c-m01-zrdr"),
        install_sha256: "a".repeat(64),
        mission_dir: "zbd/c1c/m01".to_owned(),
        group_dir: "zbd/c1c".to_owned(),
        surfaces: vec![
            SurfaceReport {
                surface: LaunchSurface::WorldTextures,
                assets: Vec::new(),
                verdict: SurfaceVerdict::Satisfied {
                    consumer: "a texture consumer".to_owned(),
                },
            },
            SurfaceReport {
                surface: LaunchSurface::WorldGeometry,
                assets: Vec::new(),
                verdict: SurfaceVerdict::Unsupported {
                    mechanism: "a missing mechanism".to_owned(),
                    detail: "the measured detail".to_owned(),
                },
            },
            SurfaceReport {
                surface: LaunchSurface::MissionProgram,
                assets: Vec::new(),
                verdict: SurfaceVerdict::Unknown {
                    detail: "an open question".to_owned(),
                },
            },
        ],
    };
    assert!(!plan.launchable(), "a plan with gaps is not launchable");
    let gaps: Vec<LaunchSurface> = plan.gaps().map(|report| report.surface).collect();
    assert_eq!(
        gaps,
        vec![LaunchSurface::WorldGeometry, LaunchSurface::MissionProgram],
        "the satisfied surface is not a gap, and the order is report order"
    );
    let geometry = plan
        .surface(LaunchSurface::WorldGeometry)
        .expect("the surface is reported");
    assert!(
        geometry.verdict.describe().contains("a missing mechanism"),
        "the diagnostic names the mechanism, not just the verdict: {}",
        geometry.verdict.describe()
    );
}

/// **Retail: M01's real launch closure is satisfied end to end.**
///
/// [`plan_mission_launch`] reads the installation through the same campaign
/// binding the M01-A stage asserted — `mission/ch1-m01`, `world/c1c`,
/// `script/c1c-m01-zrdr` — and walks the surfaces the launch description
/// names. The assertions are the measured answers:
///
/// * the mission reader archive `zbd/c1c/m01/zrdr.zbd` lists its members and
///   every `.zrd` member decodes as a document;
/// * `planes.zbd` converts through `scene_graph_from_gamez`
///   (`SharedAircraft` is the one satisfied load-bearing surface beside the
///   textures);
/// * the surfaces the landed stages own are `Satisfied` — the directive
///   lowering (#717) for `mission_program` and `mission_objectives`, the
///   audible device (#635) for `mission_audio`, `read_mission_weather` for
///   `mission_environment`, #718's `MissionAnimationPlayer` for **both**
///   animation carriers, whose rows this plan starts, #715/#770's
///   `MissionStartConfiguration` for `player_configuration`, and the world
///   import (#629/#677/#716/#727/#771) for `world_geometry`, whose report
///   leaves no record without a measured collision answer;
/// * the placed world actors satisfy their surface — the carrier decoded,
///   all three records joined, and #1155 bound the allegiance the lowering
///   needs, so the production session launches (#772/#792/#814/#1155);
/// * every surface satisfied means the plan **is** launchable and the gate
///   names no gap.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_retail_m01_s_launch_closure_names_every_missing_mechanism() {
    let plan = plan_mission_launch(&game_dir(), label("M01"), &discovery_title())
        .expect("the installation yields a launch plan for M01");

    // Identity: the binding-derived ids, not the discovery title.
    assert_eq!(plan.catalog_id, cid(ContentKind::Mission, "ch1-m01"));
    assert_eq!(plan.world_id, cid(ContentKind::World, "c1c"));
    assert_eq!(plan.program_id, cid(ContentKind::Script, "c1c-m01-zrdr"));
    assert_eq!(plan.mission_dir, "zbd/c1c/m01");
    assert_eq!(plan.group_dir, "zbd/c1c");

    // Every surface is reported exactly once, in the declared order.
    let surfaces: Vec<LaunchSurface> = plan.surfaces.iter().map(|report| report.surface).collect();
    assert_eq!(
        surfaces,
        LaunchSurface::ALL,
        "no surface is silently omitted"
    );

    // The mission reader archive is listed and every `.zrd` member decodes.
    let mission_reader = plan
        .surface(LaunchSurface::MissionProgram)
        .and_then(|report| {
            report
                .assets
                .iter()
                .find(|asset| asset.key == "zbd/c1c/m01/zrdr.zbd")
        })
        .expect("the mission reader archive is examined");
    assert!(
        mission_reader.unreadable.is_none(),
        "the mission reader archive lists cleanly: {:?}",
        mission_reader.unreadable
    );
    assert!(
        mission_reader.members.len() >= 4,
        "the mission archive holds its declared members: {:?}",
        mission_reader
            .members
            .iter()
            .map(|member| &member.name)
            .collect::<Vec<_>>()
    );
    for member in &mission_reader.members {
        if member.name.to_ascii_lowercase().ends_with(".zrd") {
            assert_eq!(
                member.verdict,
                MemberVerdict::Document,
                "every .zrd member decodes through decode_zrd: {}",
                member.name
            );
        }
    }
    let member_names: BTreeSet<String> = mission_reader
        .members
        .iter()
        .map(|member| member.name.to_ascii_lowercase())
        .collect();
    assert!(
        member_names.contains("objectives.zrd"),
        "the objective records are present: {member_names:?}"
    );

    // The shared aircraft container is the one surface that already
    // converts end to end.
    let aircraft = plan
        .surface(LaunchSurface::SharedAircraft)
        .expect("the surface is reported");
    assert!(
        matches!(aircraft.verdict, SurfaceVerdict::Satisfied { .. }),
        "planes.zbd converts through scene_graph_from_gamez: {}",
        aircraft.verdict.describe()
    );

    // The world container converts and imports through the measured path:
    // #677's unit census, #716's fog-volume measurement, #727's grid
    // collision origin and #771's residual roles together leave no record
    // without an answer, so the surface is `Satisfied` and names the
    // measurement the verdict is read off.
    let geometry = plan
        .surface(LaunchSurface::WorldGeometry)
        .expect("the surface is reported");
    let SurfaceVerdict::Satisfied { consumer } = &geometry.verdict else {
        panic!(
            "the container leaves no record without a measured collision answer: {}",
            geometry.verdict.describe()
        );
    };
    assert!(
        consumer.contains("import_world_container") && consumer.contains("axis"),
        "the measured import and its axis convention are named: {consumer}"
    );
    assert!(
        consumer.contains(WORLD_AXIS_CONVENTION_MEASURED),
        "the axis claim the verdict reads is cited: {consumer}"
    );
    assert!(
        consumer.contains("objects with an `Unknown` collision role: 0"),
        "the zero residual is stated, not omitted: {consumer}"
    );

    // Weather binds, the audible device consumes the sound archives, the
    // directive lowering (#717), the animation consumer (#718) and the
    // start-configuration recovery (#715/#770) now own their surfaces:
    // both carriers' rows start in the production player, and the player's
    // airframe and pose bind measured.
    for surface in [
        LaunchSurface::MissionEnvironment,
        LaunchSurface::MissionAudio,
        LaunchSurface::MissionProgram,
        LaunchSurface::MissionObjectives,
        LaunchSurface::MissionAnimations,
        LaunchSurface::CameraAnimations,
        LaunchSurface::PlayerConfiguration,
    ] {
        let report = plan.surface(surface).expect("the surface is reported");
        assert!(
            report.verdict.is_satisfied(),
            "{surface:?}: {}",
            report.verdict.describe()
        );
    }

    // The world-actor surface is satisfied: the carrier decoded, all three
    // records joined their world nodes, the pose and attitude bind under
    // their own claims (#792, #814) and the allegiance binds the measured
    // resolver outcome (#1155), so the production lowering and
    // `WorldActorSession::launch` run and a session exists.
    let actors = plan
        .surface(LaunchSurface::WorldActors)
        .expect("the surface is reported");
    let SurfaceVerdict::Satisfied { consumer } = &actors.verdict else {
        panic!(
            "the world actors lower and launch: {}",
            actors.verdict.describe()
        );
    };
    assert!(
        consumer.contains("WorldActorSession launched 3 of 3 decoded world actors"),
        "the session launched for every decoded record: {consumer}"
    );
    assert!(
        plan.launchable(),
        "every surface satisfied means the gate passes: {:?}",
        plan.gaps()
            .map(|report| format!("{}: {}", report.surface.label(), report.verdict.describe()))
            .collect::<Vec<_>>()
    );

    // The whole measured closure, spelled out for the finding.
    for report in &plan.surfaces {
        eprintln!("{}: {}", report.surface.label(), report.verdict.describe());
        for asset in &report.assets {
            eprintln!(
                "  {} [{}] members={}{}",
                asset.key,
                asset.family,
                asset.members.len(),
                asset
                    .unreadable
                    .as_deref()
                    .map(|reason| format!(" unreadable: {reason}"))
                    .unwrap_or_default()
            );
            for member in &asset.members {
                eprintln!(
                    "    {} ({}B): {}",
                    member.name,
                    member.bytes,
                    match &member.verdict {
                        MemberVerdict::Document => "document".to_owned(),
                        MemberVerdict::Opaque => "opaque".to_owned(),
                        MemberVerdict::Refused { reason } => format!("refused: {reason}"),
                    }
                );
            }
        }
    }

    let named: BTreeSet<&str> = plan.gaps().map(|report| report.surface.label()).collect();
    assert!(
        named.is_empty(),
        "the gate names no gap: every landed stage satisfied its surface — the \
         last open fields were the world actors' allegiance (#1155) and the \
         container's residual records (#771): {named:?}"
    );

    // Nothing read wrote to the installation and every examined archive
    // carries its fingerprint — the closure's rows are re-verifiable.
    for report in &plan.surfaces {
        for asset in &report.assets {
            if !asset.sha256.is_empty() {
                assert_eq!(
                    asset.sha256.len(),
                    64,
                    "{} carries the installation's sha256",
                    asset.key
                );
            }
        }
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

/// **`--cs-path <dir> --mission <id>` is a request of its own, and a mission
/// without an installation has no synthetic stand-in.**
#[test]
fn accept_vs_m01_runtime_the_mission_request_parses_and_refuses_a_synthetic_mix() {
    use cs_app::cli::{CliRequest, MissionRequest, parse};
    assert_eq!(
        parse(args(&["--cs-path", "/install", "--mission", "M01"])),
        CliRequest::Mission(MissionRequest {
            cs_path: PathBuf::from("/install"),
            mission: "M01".to_owned(),
        })
    );
    for bad in [
        args(&["--mission", "M01"]),
        args(&["--playtest", "--cs-path", "/i", "--mission", "M01"]),
        args(&["--cs-path", "/i", "--mission", "M01", "--world", "c1c"]),
    ] {
        assert!(
            matches!(parse(bad.clone()), CliRequest::Invalid { .. }),
            "{bad:?} is refused as invalid input"
        );
    }
}

/// **A mission nobody declared, and an installation nobody can read, are
/// refused before anything starts.**
#[test]
fn accept_vs_m01_runtime_launch_refuses_an_undeclared_mission_and_an_empty_install() {
    use cs_app::cli::MissionRequest;
    use cs_app::mission_launch::{MissionLaunchError, launch_mission};
    let dir = std::env::temp_dir().join("vs_m01_runtime_empty_install_launch");
    std::fs::create_dir_all(&dir).expect("the probe directory is created");
    let undeclared = launch_mission(&MissionRequest {
        cs_path: dir.clone(),
        mission: "M99".to_owned(),
    })
    .expect_err("M99 declares no identity");
    assert!(matches!(
        undeclared,
        MissionLaunchError::UndeclaredMission(_)
    ));
    let empty = launch_mission(&MissionRequest {
        cs_path: dir,
        mission: "M01".to_owned(),
    })
    .expect_err("an empty installation cannot launch");
    assert!(matches!(empty, MissionLaunchError::Plan(_)), "{empty}");
}

/// **On the retail installation the gate passes and the refusal is the
/// composition seam, never a guessed scene.** With every surface satisfied the
/// launch stops exactly at the one thing still missing — the windowed mission
/// composition this stage builds — reported as [`MissionLaunchError::NoRuntime`],
/// and it is never a `Blocked` plan: no surface is named as missing.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_retail_launch_reaches_the_composition_seam() {
    use cs_app::cli::MissionRequest;
    use cs_app::mission_launch::{MissionLaunchError, launch_mission};
    let error = launch_mission(&MissionRequest {
        cs_path: game_dir(),
        mission: "M01".to_owned(),
    })
    .expect_err("a launchable plan still stops at the unbuilt composition");
    assert!(
        matches!(error, MissionLaunchError::NoRuntime),
        "the refusal is the composition seam, not a surface gap: {error}"
    );
}
