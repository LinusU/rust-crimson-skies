//! The original-mission launch plan and its measured dependency closure
//! (VS-M01-RUNTIME).
//!
//! A `--mission` launch does not start from a file name: it starts from the
//! campaign binding the installation itself derives
//! ([`cs_content::campaign_bindings`]), and from there it walks every archive
//! surface the launch must consume — the world container, the shared aircraft
//! container, the mission's reader archive and its animation carriers, the
//! world group's own reader archive, textures and sound. Each surface gets a
//! measured verdict: the production consumer that owns it, the mechanism that
//! is still missing, or the question no stage has answered yet.
//!
//! A member whose bytes decode is not a member that plays. `decode_zrd` reads
//! every `.zrd` document of the mission reader cleanly, and the
//! `MissionProgram` surface still reports `Unsupported`: nothing in
//! `cs_script` or `cs_content` turns an original reader member into a
//! runnable `MissionProgram`, and `lower_program` refuses
//! `DeclaredSupport::Original` by design. The closure records that instead of
//! simulating a mission around it — `docs/contracts/SCRIPT-MISSION.md` keeps
//! an undecoded program an `Unsupported` mission, never a guessed one.
//!
//! [`MissionLaunchPlan::launchable`] is the gate the runner will read: a
//! launch whose plan names an `Unsupported` or `Unknown` surface exits
//! nonzero with the surfaces spelled out, which is also the acceptance
//! behavior for unsupported reachable assets — reject with source
//! diagnostics, never fake a scene.

use std::fmt;
use std::path::{Path, PathBuf};

use cs_assets::install::{self, Discovery};
use cs_content::campaign_bindings::{MissionLabel, SourceBindingError, SourceContext};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::scene::BindingMap;
use cs_content::stunts::decode_zrd;
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED, UNINDEXED_ROLE_UNMEASURED,
    WORLD_AXIS_CONVENTION_MEASURED, WorldImportReport, WorldSceneError,
    world_scene_graph_from_gamez,
};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Resolved};
use cs_types::evidence::ClaimStatus;
use cs_types::install::InstallFileRecord;
use cs_types::net::SessionId;

use crate::animation::mission::{
    MissionAnimationBinding, MissionAnimationError, PLACEMENT_FIELDS_CLAIM, StartupAnimation,
    bind_mission_animation,
};
use crate::animation::programs::UnmeasuredFieldFamily;
use crate::animation::survey::CarrierKind;
use crate::mission_animations::MissionAnimationPlayer;

/// The surfaces an original-mission launch must account for, in report
/// order. These are the nouns the launch description names — world, aircraft,
/// script host, authored objectives, world actors, animations, textures,
/// audio and environment — plus the binding the launch resolves first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LaunchSurface {
    /// `ZBD/<world group>/gamez.zbd` → `WorldSceneGraph` → a spawned world.
    WorldGeometry,
    /// `ZBD/<world group>/texture*.zbd` → decoded images → uploads.
    WorldTextures,
    /// `ZBD/planes.zbd` → `SceneGraph` → the shared airframe roster.
    SharedAircraft,
    /// Which airframe and wingmate the mission assigns the player.
    PlayerConfiguration,
    /// The mission reader archive's control members as a runnable program.
    MissionProgram,
    /// `objectives.zrd` and the target records as authored objectives.
    MissionObjectives,
    /// World-actor programs of the mission, group and shared reader
    /// archives.
    WorldActors,
    /// `mis_anim.zbd`, the mission's animation carrier.
    MissionAnimations,
    /// `cam_anim.zbd`, the world group's camera carrier.
    CameraAnimations,
    /// Sound archives inside the mission's scope.
    MissionAudio,
    /// `weather.zrd` and the environment records the mission declares.
    MissionEnvironment,
}

impl LaunchSurface {
    /// Every surface, in report order.
    pub const ALL: [LaunchSurface; 11] = [
        Self::WorldGeometry,
        Self::WorldTextures,
        Self::SharedAircraft,
        Self::PlayerConfiguration,
        Self::MissionProgram,
        Self::MissionObjectives,
        Self::WorldActors,
        Self::MissionAnimations,
        Self::CameraAnimations,
        Self::MissionAudio,
        Self::MissionEnvironment,
    ];

    /// Stable lowercase label for reports and diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::WorldGeometry => "world_geometry",
            Self::WorldTextures => "world_textures",
            Self::SharedAircraft => "shared_aircraft",
            Self::PlayerConfiguration => "player_configuration",
            Self::MissionProgram => "mission_program",
            Self::MissionObjectives => "mission_objectives",
            Self::WorldActors => "world_actors",
            Self::MissionAnimations => "mission_animations",
            Self::CameraAnimations => "camera_animations",
            Self::MissionAudio => "mission_audio",
            Self::MissionEnvironment => "mission_environment",
        }
    }
}

/// What the production readers could do with one archive member's bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberVerdict {
    /// The member decodes as a `.zrd` document through `decode_zrd`.
    Document,
    /// The member is not a `.zrd` document; its bytes were not interpreted.
    Opaque,
    /// A production reader refused the member, with its own reason.
    Refused { reason: String },
}

/// One archive the closure walked, with the verdict on each member it
/// lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetReport {
    /// Installation-relative logical key, e.g. `zbd/c1c/m01/zrdr.zbd`.
    pub key: String,
    /// The installation's own SHA-256 of the container bytes.
    pub sha256: String,
    /// The family `dispatch` classified it as, with the dispatch basis.
    pub family: String,
    /// Per-member verdicts, when the archive is a reader archive; empty for
    /// other families or when the listing itself refused.
    pub members: Vec<MemberReport>,
    /// Why the archive could not be read or listed, when it could not.
    pub unreadable: Option<String>,
}

/// One member row of an [`AssetReport`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberReport {
    /// The member's name bytes decoded lossily (the store is byte names).
    pub name: String,
    /// The member's byte length inside the container.
    pub bytes: u64,
    /// What the production reader made of it.
    pub verdict: MemberVerdict,
}

/// What one launch surface resolves to under the current production systems.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceVerdict {
    /// A production consumer owns this surface today.
    Satisfied {
        /// The consumer, named by its production type or stage.
        consumer: String,
    },
    /// The records read, but no production mechanism consumes them for a
    /// mission: the missing mechanism is named, with the measured detail.
    Unsupported {
        /// The missing mechanism (e.g. `mission-language semantics`).
        mechanism: String,
        /// The measured detail — a reader's own refusal or the stage's
        /// recorded state.
        detail: String,
    },
    /// No stage has answered this question yet; nothing guessed.
    Unknown {
        /// The open question.
        detail: String,
    },
}

impl SurfaceVerdict {
    /// Whether the launch can proceed through this surface.
    pub const fn is_satisfied(&self) -> bool {
        matches!(self, Self::Satisfied { .. })
    }

    /// One line naming the verdict and its reason, for diagnostics.
    pub fn describe(&self) -> String {
        match self {
            Self::Satisfied { consumer } => format!("satisfied by {consumer}"),
            Self::Unsupported { mechanism, detail } => {
                format!("unsupported: missing {mechanism} ({detail})")
            }
            Self::Unknown { detail } => format!("unknown: {detail}"),
        }
    }
}

/// One surface's row of the closure: the archives walked and the verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceReport {
    /// The surface.
    pub surface: LaunchSurface,
    /// The archives this surface examined, in logical-key order.
    pub assets: Vec<AssetReport>,
    /// What the surface resolves to.
    pub verdict: SurfaceVerdict,
}

/// The whole measured launch plan: the binding the installation derived,
/// and every surface's verdict.
#[derive(Clone, Debug)]
pub struct MissionLaunchPlan {
    /// The work-order label this launch was requested under.
    pub label: MissionLabel,
    /// The derived catalog identity, e.g. `mission/ch1-m01`.
    pub catalog_id: ContentId,
    /// The world group identity, e.g. `world/c1c`.
    pub world_id: ContentId,
    /// The mission program identity, e.g. `script/c1c-m01-zrdr`.
    pub program_id: ContentId,
    /// The installation fingerprint the binding was read under.
    pub install_sha256: String,
    /// The mission's own directory key, e.g. `zbd/c1c/m01`.
    pub mission_dir: String,
    /// The world group's directory key, e.g. `zbd/c1c`.
    pub group_dir: String,
    /// Every surface's report, in [`LaunchSurface::ALL`] order.
    pub surfaces: Vec<SurfaceReport>,
}

impl MissionLaunchPlan {
    /// Whether every surface is satisfied — the gate the runner reads.
    pub fn launchable(&self) -> bool {
        self.surfaces
            .iter()
            .all(|report| report.verdict.is_satisfied())
    }

    /// The surfaces blocking the launch, in report order.
    pub fn gaps(&self) -> impl Iterator<Item = &SurfaceReport> {
        self.surfaces
            .iter()
            .filter(|report| !report.verdict.is_satisfied())
    }

    /// One report's surface, looked up by kind.
    pub fn surface(&self, surface: LaunchSurface) -> Option<&SurfaceReport> {
        self.surfaces
            .iter()
            .find(|report| report.surface == surface)
    }
}

/// Why a launch plan could not be produced at all.
#[derive(Debug)]
pub enum LaunchPlanError {
    /// The installation could not be discovered or the binding context could
    /// not be read.
    Binding(SourceBindingError),
    /// The binding resolved but critical dependencies stayed unresolved —
    /// there is no mission to plan around.
    UnresolvedCritical { reasons: Vec<String> },
}

impl fmt::Display for LaunchPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(source) => write!(f, "the campaign binding failed: {source}"),
            Self::UnresolvedCritical { reasons } => write!(
                f,
                "the mission identity never resolved; critical dependencies: {}",
                reasons.join(", ")
            ),
        }
    }
}

impl std::error::Error for LaunchPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Binding(source) => Some(source),
            Self::UnresolvedCritical { .. } => None,
        }
    }
}

/// Measures the launch closure of one mission inside `install_root`.
///
/// `label` and `discovery_title` are the work order's declared identity —
/// the same arguments [`SourceContext::bind`] confirms against the local
/// strings, so a mission cannot be launched under a name the installation
/// does not carry. The plan reads every archive surface the mission's
/// directories hold, records a per-member verdict inside each reader
/// archive, and judges each surface against the production consumers that
/// exist today. Nothing is executed, spawned or drawn; this is the
/// measurement the launch path must satisfy.
///
/// # Errors
///
/// [`LaunchPlanError::Binding`] when discovery or binding fails, and
/// [`LaunchPlanError::UnresolvedCritical`] when the binding leaves a
/// critical dependency unresolved — both mean there is no mission to plan.
pub fn plan_mission_launch(
    install_root: &Path,
    label: MissionLabel,
    discovery_title: &str,
) -> Result<MissionLaunchPlan, LaunchPlanError> {
    let context = SourceContext::read(install_root).map_err(LaunchPlanError::Binding)?;
    let binding = context
        .bind(label.clone(), discovery_title)
        .map_err(LaunchPlanError::Binding)?;
    let mut unresolved: Vec<String> = binding
        .unresolved_critical()
        .iter()
        .map(ToString::to_string)
        .collect();
    // The three identities a plan is made of: a binding that resolved its
    // critical dependencies but hands over no id is refused the same way —
    // resolved-without-a-value is `validate`'s verdict too, and a plan
    // cannot carry `None`.
    for (role, id) in [
        ("catalog_id", &binding.catalog_id),
        ("world_id", &binding.world_id),
        ("program_id", &binding.program_id),
    ] {
        if id.is_none() {
            unresolved.push(format!("{role} resolved no identity"));
        }
    }
    if !unresolved.is_empty() {
        return Err(LaunchPlanError::UnresolvedCritical {
            reasons: unresolved,
        });
    }
    let catalog_id = binding.catalog_id.clone().expect("checked above");
    let world_id = binding.world_id.clone().expect("checked above");
    let program_id = binding.program_id.clone().expect("checked above");

    let found = install::discover(install_root).map_err(|source| {
        LaunchPlanError::Binding(SourceBindingError::Discover {
            path: install_root.display().to_string(),
            source,
        })
    })?;

    // The mission directory and the world-group directory are read out of
    // the binding's own campaign entry — `program_asset` spells
    // `ZBD/<GROUP>/<M>/zrdr.zbd` — never re-derived from the label.
    let program_asset = binding
        .campaign_position
        .and_then(|position| context.campaign().get(position))
        .map(|mission| mission.program_asset.to_ascii_lowercase())
        .unwrap_or_default();
    let mission_dir = program_asset
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_owned())
        .unwrap_or_default();
    let group_dir = format!("zbd/{}", world_id.key());

    let mut plan = MissionLaunchPlan {
        label: label.clone(),
        catalog_id,
        world_id,
        program_id,
        install_sha256: binding.install_sha256.clone(),
        mission_dir: mission_dir.clone(),
        group_dir: group_dir.clone(),
        surfaces: Vec::new(),
    };

    // One production animation join, shared by the three surfaces it answers
    // (`world_actors`, `mission_animations`, `camera_animations`): the same
    // `bind_mission_animation` the mission session calls, so the verdicts
    // below are read off the join rather than restated. A binding that
    // refuses is carried into those surfaces as the refusal it is.
    let animation = bind_mission_animation(install_root, &mission_dir);

    for surface in LaunchSurface::ALL {
        plan.surfaces.push(measure_surface(
            install_root,
            &found,
            &plan,
            &animation,
            surface,
        ));
    }
    Ok(plan)
}

/// What `--mission` can refuse with.
#[derive(Debug)]
pub enum MissionLaunchError {
    /// The label names no mission this build declares an identity for.
    UndeclaredMission(String),
    /// The plan could not be made.
    Plan(LaunchPlanError),
    /// The plan names surfaces no production consumer satisfies; every gap is
    /// spelled out. No scene was started.
    Blocked(Box<MissionLaunchPlan>),
    /// Every surface is satisfied but this build has no windowed mission
    /// composition to hand them to.
    NoRuntime,
}

impl fmt::Display for MissionLaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredMission(label) => write!(
                f,
                "mission {label:?} has no declared identity; only M01 is declared"
            ),
            Self::Plan(error) => write!(f, "{error}"),
            Self::Blocked(plan) => {
                write!(
                    f,
                    "{} cannot launch: {} of {} launch surfaces are not satisfied:",
                    plan.label.as_str(),
                    plan.gaps().count(),
                    plan.surfaces.len()
                )?;
                for gap in plan.gaps() {
                    write!(f, "\n  {}: {}", gap.surface.label(), gap.verdict.describe())?;
                }
                Ok(())
            }
            Self::NoRuntime => write!(
                f,
                "every launch surface is satisfied but the windowed mission \
                 composition is not built"
            ),
        }
    }
}

impl std::error::Error for MissionLaunchError {}

/// The mission labels and discovery titles this build declares (the work
/// orders' own identities; the binding confirms them against the install).
const DECLARED_MISSIONS: &[(&str, &str)] = &[("M01", "The Lost Treasure")];

/// Plans the requested mission and, when the closure is satisfied, would hand
/// it to the windowed composition. A plan with any unsatisfied surface is
/// refused with its diagnostics: nothing is spawned, drawn or played.
pub fn launch_mission(request: &crate::cli::MissionRequest) -> Result<(), MissionLaunchError> {
    let (label, title) = DECLARED_MISSIONS
        .iter()
        .find(|(label, _)| label.eq_ignore_ascii_case(&request.mission))
        .ok_or_else(|| MissionLaunchError::UndeclaredMission(request.mission.clone()))?;
    let label = MissionLabel::new(label)
        .map_err(|error| MissionLaunchError::UndeclaredMission(error.to_string()))?;
    let plan =
        plan_mission_launch(&request.cs_path, label, title).map_err(MissionLaunchError::Plan)?;
    if !plan.launchable() {
        return Err(MissionLaunchError::Blocked(Box::new(plan)));
    }
    Err(MissionLaunchError::NoRuntime)
}

/// Measures one surface: reads the archives it covers and judges the
/// result.
///
/// `animation` is the scope's own [`bind_mission_animation`] join (already
/// made once for the plan): the three surfaces it answers judge against what
/// that production consumer actually resolved, never against a restated
/// verdict.
fn measure_surface(
    install_root: &Path,
    found: &Discovery,
    plan: &MissionLaunchPlan,
    animation: &Result<MissionAnimationBinding, MissionAnimationError>,
    surface: LaunchSurface,
) -> SurfaceReport {
    match surface {
        LaunchSurface::WorldGeometry => {
            measure_geometry(install_root, found, &plan.group_dir, surface)
        }
        LaunchSurface::SharedAircraft => measure_aircraft(install_root, found, surface),
        LaunchSurface::WorldTextures => measure_textures(found, &plan.group_dir, surface),
        LaunchSurface::PlayerConfiguration => measure_player(install_root, plan, surface),
        LaunchSurface::MissionProgram
        | LaunchSurface::MissionObjectives
        | LaunchSurface::MissionEnvironment => {
            measure_mission_reader(install_root, found, plan, surface)
        }
        LaunchSurface::WorldActors => {
            measure_actor_readers(install_root, found, plan, animation, surface)
        }
        LaunchSurface::MissionAnimations | LaunchSurface::CameraAnimations => {
            measure_animation_carrier(install_root, found, plan, animation, surface)
        }
        LaunchSurface::MissionAudio => measure_audio(install_root, found, plan, surface),
    }
}

/// The group directory's `gamez.zbd`: read, node-decoded, and run through
/// the one production conversion (`world_scene_graph_from_gamez`), then
/// imported through [`crate::world::retail::read_world_container`] so the
/// verdict is read off the import's own [`WorldImportReport`]
/// ([`geometry_verdict`]). A refusal on either path is carried verbatim —
/// when `SceneGraph::build` refuses the container, the refusal *is* the
/// measured detail, not a reformulation.
fn measure_geometry(
    install_root: &Path,
    found: &Discovery,
    group_dir: &str,
    surface: LaunchSurface,
) -> SurfaceReport {
    let key = format!("{group_dir}/gamez.zbd");
    let Some(record) = manifest_record(found, &key) else {
        return SurfaceReport {
            surface,
            assets: Vec::new(),
            verdict: SurfaceVerdict::Unknown {
                detail: format!("no {key} in the discovered installation"),
            },
        };
    };
    let bytes = match read_member_bytes(install_root, record) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return asset_refusal(surface, found, record, &key, reason);
        }
    };
    let mut context = ParseContext::with_defaults(&key);
    let nodes = match read_gamez_nodes(&mut context, &bytes) {
        Ok(nodes) => nodes,
        Err(error) => {
            return asset_refusal(
                surface,
                found,
                record,
                &key,
                format!("the node array refuses: {error}"),
            );
        }
    };
    let container = match ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.{}", key.replace('/', ".")),
    ) {
        Ok(container) => container,
        Err(error) => {
            return asset_refusal(
                surface,
                found,
                record,
                &key,
                format!("the container id cannot be spelled: {error}"),
            );
        }
    };
    let adapter = SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source");
    let bindings = BindingMap::default();
    let verdict = match world_scene_graph_from_gamez(&container, &nodes, &[], &adapter, &bindings) {
        Ok(_) => {
            match crate::world::retail::read_world_container(
                install_root,
                group_dir.trim_start_matches("zbd/"),
                &WorldTextureLoad::project_default(),
            ) {
                Err(reason) => SurfaceVerdict::Unsupported {
                    mechanism: "original world import".to_owned(),
                    detail: reason.to_string(),
                },
                Ok(world) => {
                    // The measured coordinate source for a retail GameZ
                    // container (#436's image, #677's scale census): its
                    // `Origin::Installation` and identity axes are what make
                    // the report's axis class an observation instead of a
                    // designed convention.
                    let adapter =
                        SourceAdapter::new(CoordinateSource::retail_gamez(world.span().clone()));
                    let origin = Origin::Installation {
                        source: world.span().clone(),
                    };
                    match world.definition(origin, &adapter) {
                        Err(reason) => SurfaceVerdict::Unsupported {
                            mechanism: "original world import".to_owned(),
                            detail: reason.to_string(),
                        },
                        Ok(imported) => geometry_verdict(imported.report()),
                    }
                }
            }
        }
        Err(error) => {
            let mechanism = match &error {
                WorldSceneError::Hierarchy(_) => "world-hierarchy reconciliation",
                WorldSceneError::Build { .. } => "world-container scene conversion",
            };
            SurfaceVerdict::Unsupported {
                mechanism: mechanism.to_owned(),
                detail: error.to_string(),
            }
        }
    };
    SurfaceReport {
        surface,
        assets: vec![AssetReport {
            key,
            sha256: record.sha256.to_hex(),
            family: "gamez".to_owned(),
            members: Vec::new(),
            unreadable: None,
        }],
        verdict,
    }
}

/// The `world_geometry` verdict, read off the import's own
/// [`WorldImportReport`] rather than restated.
///
/// The vertex unit, the axis convention and every collision role the
/// container *states* were measured by #677, #716 and #727, so this surface
/// is satisfied exactly when the import applied the measured axis
/// convention **and** left no record without an answer. A grid-named
/// `fvol*` volume ([`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`]) and a
/// mesh-bearing unindexed record ([`UNINDEXED_ROLE_UNMEASURED`]) are both
/// records the container states nothing about: the launch would be placing
/// them by default, so they stay an open question.
fn geometry_verdict(report: &WorldImportReport) -> SurfaceVerdict {
    let fog = report.partition_records_fog_volume();
    let unindexed = report.objects_unindexed_unresolved();
    let meshless = report
        .partition_records()
        .saturating_sub(report.partition_records_with_mesh());
    let axis_measured = matches!(report.axis_class(), ClaimStatus::ObservedTool);
    let mut open: Vec<String> = Vec::new();
    if fog > 0 {
        open.push(format!(
            "the collision role of the {fog} grid-named `fvol*` volume records \
             ({GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED})"
        ));
    }
    if unindexed > 0 {
        open.push(format!(
            "the collision role of the {unindexed} unindexed geometry-bearing records \
             ({UNINDEXED_ROLE_UNMEASURED})"
        ));
    }
    if meshless > 0 {
        let noun = if meshless == 1 { "record" } else { "records" };
        let that = if meshless == 1 {
            "that stores"
        } else {
            "that store"
        };
        open.push(format!(
            "what the {meshless} grid {noun} {that} no mesh index drew"
        ));
    }
    if !axis_measured {
        open.push(format!(
            "the import applied the axis map `{}` as {:?}, not as the \
             installation-backed convention {} records",
            report.axis_map(),
            report.axis_class(),
            WORLD_AXIS_CONVENTION_MEASURED
        ));
    }
    if open.is_empty() {
        SurfaceVerdict::Satisfied {
            consumer: format!(
                "world::retail::read_world_container + import_world_container ({} partition \
                 cells, {} objects, axis `{}` under {})",
                report.partition_cells(),
                report.objects(),
                report.axis_map(),
                WORLD_AXIS_CONVENTION_MEASURED
            ),
        }
    } else {
        SurfaceVerdict::Unknown {
            detail: format!(
                "the container imports to a WorldDefinition ({} partition cells, {} \
                 objects) with the `{}` axis map applied as {:?} under {}, and every \
                 `fvol*` record classified as fog (#716); what no stage has answered \
                 is {} \
                 (docs/findings/2026-10-07-m01-lc-fvol-roles-and-axis-convention.md, \
                 docs/findings/2026-10-07-f18-grid-collision-origin.md)",
                report.partition_cells(),
                report.objects(),
                report.axis_map(),
                report.axis_class(),
                WORLD_AXIS_CONVENTION_MEASURED,
                open.join("; and ")
            ),
        }
    }
}

/// The shared aircraft container `ZBD/planes.zbd`, through the production
/// `scene_graph_from_gamez`.
fn measure_aircraft(
    install_root: &Path,
    found: &Discovery,
    surface: LaunchSurface,
) -> SurfaceReport {
    let key = "zbd/planes.zbd".to_owned();
    let Some(record) = manifest_record(found, &key) else {
        return SurfaceReport {
            surface,
            assets: Vec::new(),
            verdict: SurfaceVerdict::Unknown {
                detail: format!("no {key} in the discovered installation"),
            },
        };
    };
    let bytes = match read_member_bytes(install_root, record) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return asset_refusal(surface, found, record, &key, reason);
        }
    };
    let mut context = ParseContext::with_defaults(&key);
    let nodes = match read_gamez_nodes(&mut context, &bytes) {
        Ok(nodes) => nodes,
        Err(error) => {
            return asset_refusal(
                surface,
                found,
                record,
                &key,
                format!("the node array refuses: {error}"),
            );
        }
    };
    let container = match ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.{}", key.replace('/', ".")),
    ) {
        Ok(container) => container,
        Err(error) => {
            return asset_refusal(
                surface,
                found,
                record,
                &key,
                format!("the container id cannot be spelled: {error}"),
            );
        }
    };
    let adapter = SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source");
    let bindings = BindingMap::default();
    let verdict = match cs_content::scene::scene_graph_from_gamez(
        &container,
        &nodes,
        &[],
        &adapter,
        &bindings,
    ) {
        Ok(graph) => SurfaceVerdict::Satisfied {
            consumer: format!("scene_graph_from_gamez ({} nodes)", graph.len()),
        },
        Err(error) => SurfaceVerdict::Unsupported {
            mechanism: "aircraft-container scene conversion".to_owned(),
            detail: error.to_string(),
        },
    };
    SurfaceReport {
        surface,
        assets: vec![AssetReport {
            key,
            sha256: record.sha256.to_hex(),
            family: "gamez".to_owned(),
            members: Vec::new(),
            unreadable: None,
        }],
        verdict,
    }
}

/// The world group's texture archives: each `*texture*.zbd` in the group
/// directory, dispatched and fingerprinted. The decode-and-upload chain
/// (F08 readers, F17-B adapters) exists; what does not exist is a world
/// that binds them, which `WorldGeometry` reports.
fn measure_textures(found: &Discovery, group_dir: &str, surface: LaunchSurface) -> SurfaceReport {
    let assets: Vec<AssetReport> = found
        .manifest
        .files
        .iter()
        .filter(|record| {
            let key = record.relative_spelling.logical_key();
            key.starts_with(&format!("{group_dir}/"))
                && key.contains("texture")
                && key.ends_with(".zbd")
        })
        .map(|record| AssetReport {
            key: record.relative_spelling.logical_key(),
            sha256: record.sha256.to_hex(),
            family: "zbd".to_owned(),
            members: Vec::new(),
            unreadable: None,
        })
        .collect();
    let verdict = if assets.is_empty() {
        SurfaceVerdict::Unknown {
            detail: "the group directory holds no *texture*.zbd archives".to_owned(),
        }
    } else {
        SurfaceVerdict::Satisfied {
            consumer: "F08 texture decode and F17-B upload adapters".to_owned(),
        }
    };
    SurfaceReport {
        surface,
        assets,
        verdict,
    }
}

/// Which airframe and wingmate the mission assigns, from the original
/// `aiv.zrd` through `recover_retail_start_configuration`. The records
/// resolve; #715 then measured that no campaign mission data, no
/// installation file and no measured directive names the player's airframe
/// and that the pose's heading zero, handedness and frame relation were never
/// measured, so both stay recorded unknowns and a launch must not pick
/// either.
fn measure_player(
    install_root: &Path,
    plan: &MissionLaunchPlan,
    surface: LaunchSurface,
) -> SurfaceReport {
    let verdict = match crate::mission_start::recover_retail_start_configuration(
        install_root,
        &plan.mission_dir,
    ) {
        Err(error) => SurfaceVerdict::Unknown {
            detail: format!("the start configuration cannot be read: {error}"),
        },
        Ok(configuration) => {
            let mut open = Vec::new();
            if !matches!(configuration.airframe(), Resolved::Known(_)) {
                open.push("the player's airframe");
            }
            if !matches!(configuration.initial_pose(), Resolved::Known(_)) {
                open.push("the player's initial pose");
            }
            if open.is_empty() {
                SurfaceVerdict::Satisfied {
                    consumer: "mission_start::MissionStartConfiguration".to_owned(),
                }
            } else {
                let (verb, rest) = if open.len() == 1 {
                    ("is", open[0].to_string())
                } else {
                    ("are", open.join(" and "))
                };
                SurfaceVerdict::Unknown {
                    detail: format!(
                        "aiv.zrd records are read and the stored position binds to metres, \
                         but {rest} {verb} unmeasured: no campaign mission data, no \
                         installation file and no measured directive names the player's \
                         airframe, and the pose's heading zero, handedness and frame \
                         relation to the world grid were not measured either \
                         (docs/findings/2026-10-06-m01-lc-player-airframe-source.md)"
                    ),
                }
            }
        }
    };
    SurfaceReport {
        surface,
        assets: Vec::new(),
        verdict,
    }
}

/// The mission reader archive `zrdr.zbd`: every member listed and each
/// `.zrd` member decoded through the production document decoder, with the
/// refusal kept when it cannot. Three surfaces share this walk and differ
/// only in the mechanism they still lack:
///
/// * `MissionProgram` — the control members (`aiv.zrd`, `objectives.zrd`,
///   `targets.zrd`) and the rest of the archive as a runnable program.
/// * `MissionObjectives` — the authored objective records.
/// * `MissionEnvironment` — `weather.zrd` and environment records.
fn measure_mission_reader(
    install_root: &Path,
    found: &Discovery,
    plan: &MissionLaunchPlan,
    surface: LaunchSurface,
) -> SurfaceReport {
    let key = format!("{}/zrdr.zbd", plan.mission_dir);
    let assets = match read_reader_members(install_root, found, &key) {
        Ok(asset) => vec![asset],
        Err(report) => {
            return SurfaceReport {
                surface,
                assets: vec![report],
                verdict: SurfaceVerdict::Unknown {
                    detail: "the mission reader archive cannot be listed".to_owned(),
                },
            };
        }
    };
    let verdict = match surface {
        LaunchSurface::MissionProgram | LaunchSurface::MissionObjectives => {
            measure_control_program(install_root, plan)
        }
        _ => match crate::environment::retail::read_mission_weather(found, &plan.mission_dir) {
            Ok(_) => SurfaceVerdict::Satisfied {
                consumer: "environment::retail::read_mission_weather into EnvironmentSession"
                    .to_owned(),
            },
            Err(error) => SurfaceVerdict::Unsupported {
                mechanism: "mission weather binding".to_owned(),
                detail: error.to_string(),
            },
        },
    };
    SurfaceReport {
        surface,
        assets,
        verdict,
    }
}

/// The mission's control program (`objectives.zrd` directives) through the
/// production census: a launch runs only when every directive of the record
/// has an implemented disposition, and the unmet requirements are named.
fn measure_control_program(install_root: &Path, plan: &MissionLaunchPlan) -> SurfaceVerdict {
    let census = match crate::mission_control::survey_mission_control_programs(install_root) {
        Ok(census) => census,
        Err(error) => {
            return SurfaceVerdict::Unknown {
                detail: format!("the control census refuses: {error}"),
            };
        }
    };
    let Some(row) = census.row(&plan.mission_dir) else {
        return SurfaceVerdict::Unknown {
            detail: format!("the census has no row for {}", plan.mission_dir),
        };
    };
    match row.lowering() {
        None => SurfaceVerdict::Unsupported {
            mechanism: "mission control program".to_owned(),
            detail: "the reader archive declares no control member".to_owned(),
        },
        Some(lowering) if lowering.complete() => SurfaceVerdict::Satisfied {
            consumer: "mission_control::ControlLowering".to_owned(),
        },
        Some(lowering) => {
            let unmet: Vec<String> = lowering.unmet().map(|row| row.label()).collect();
            SurfaceVerdict::Unsupported {
                mechanism: "mission-directive semantics".to_owned(),
                detail: format!(
                    "unmet lowering requirements [{}]; directive meanings still unmeasured: {} \
                     (docs/findings/2026-10-04-m01-lc-mission-program.md)",
                    unmet.join(", "),
                    lowering.unmeasured_fields().join(", ")
                ),
            }
        }
    }
}

/// World-actor surfaces: the mission, world-group and shared (`zbd/zrdr.zbd`)
/// reader archives — every `.zrd` member decoded, every verdict kept — judged
/// against the one production join that carries member → actor for a scope.
///
/// #632 measured which member drives which actor, #678 joined that to the
/// world container and #718's [`crate::mission_animations::MissionAnimationPlayer`]
/// consumes it for a mission's startup rows, so "which member drives which
/// actor" is measured and consumed today. What still has no measured source
/// is the declared world-actor program's runtime half — a placed actor's
/// spawn and route — which is [`UnmeasuredFieldFamily::PlacementRecords`],
/// and this scope's own placements refuse to place under
/// [`PLACEMENT_FIELDS_CLAIM`].
fn measure_actor_readers(
    install_root: &Path,
    found: &Discovery,
    plan: &MissionLaunchPlan,
    animation: &Result<MissionAnimationBinding, MissionAnimationError>,
    surface: LaunchSurface,
) -> SurfaceReport {
    let mut assets = Vec::new();
    for key in [
        format!("{}/zrdr.zbd", plan.mission_dir),
        format!("{}/zrdr.zbd", plan.group_dir),
        "zbd/zrdr.zbd".to_owned(),
    ] {
        match read_reader_members(install_root, found, &key) {
            Ok(asset) => assets.push(asset),
            Err(report) => assets.push(report),
        }
    }
    let verdict = match animation {
        Err(error) => SurfaceVerdict::Unknown {
            detail: format!(
                "the scope's animation join refuses, so member → actor cannot be read: \
                 {error}"
            ),
        },
        Ok(binding) => {
            let placements = binding.placements().len();
            let targets = binding.world_targets().count();
            SurfaceVerdict::Unsupported {
                mechanism: "world-actor program semantics".to_owned(),
                detail: format!(
                    "the member → actor join resolves {targets} world names across {} \
                     startup rows and {placements} placement declarations of this scope \
                     (#632, #678, #718), but {} — {}; DeclaredWorldActorProgram has no \
                     measured source for a placed actor's spawn or route, and this \
                     scope's placements refuse to place under {PLACEMENT_FIELDS_CLAIM} \
                     (resolving tasks #574 and the field families #632 named)",
                    binding.startup().len(),
                    UnmeasuredFieldFamily::PlacementRecords.label(),
                    UnmeasuredFieldFamily::PlacementRecords.reason(),
                ),
            }
        }
    };
    SurfaceReport {
        surface,
        assets,
        verdict,
    }
}

/// The animation carriers (`mis_anim.zbd` in the mission directory,
/// `cam_anim.zbd` in the group directory): dispatched, fingerprinted and
/// reported, then judged against the production consumer that starts their
/// records for a mission.
///
/// The verdict is derived, never restated: the scope's own
/// [`bind_mission_animation`] join says which startup rows a record of
/// *this* carrier stores, and [`MissionAnimationPlayer`] is started with
/// exactly those rows. A carrier no row resolves a record in, or one whose
/// rows all refuse, names that instead of a fixed answer.
fn measure_animation_carrier(
    install_root: &Path,
    found: &Discovery,
    plan: &MissionLaunchPlan,
    animation: &Result<MissionAnimationBinding, MissionAnimationError>,
    surface: LaunchSurface,
) -> SurfaceReport {
    let (key, kind, mechanism) = match surface {
        LaunchSurface::MissionAnimations => (
            format!("{}/mis_anim.zbd", plan.mission_dir),
            CarrierKind::Mission,
            "mission-animation payload semantics",
        ),
        _ => (
            format!("{}/cam_anim.zbd", plan.group_dir),
            CarrierKind::Camera,
            "camera-animation payload semantics",
        ),
    };
    let assets = match read_archive_header(install_root, found, &key) {
        Ok(asset) => vec![asset],
        Err(report) => vec![report],
    };
    let missing = assets.iter().any(|asset| asset.unreadable.is_some());
    let verdict = if missing {
        SurfaceVerdict::Unknown {
            detail: format!("the carrier at {key} is not readable as a zbd"),
        }
    } else {
        match animation {
            Err(error) => SurfaceVerdict::Unknown {
                detail: format!(
                    "the scope's animation join refuses, so the records of {key} cannot \
                     be measured: {error}"
                ),
            },
            Ok(binding) => {
                let rows: Vec<StartupAnimation> = binding
                    .startup()
                    .iter()
                    .filter(
                        |row| matches!(row.record().bound(), Some(facts) if facts.carrier == kind),
                    )
                    .cloned()
                    .collect();
                if rows.is_empty() {
                    SurfaceVerdict::Unsupported {
                        mechanism: mechanism.to_owned(),
                        detail: format!(
                            "no startup row of {} resolves a record in {key}: the join \
                             found nothing this carrier stores",
                            plan.mission_dir
                        ),
                    }
                } else {
                    match started_rows(&rows) {
                        Err(reason) => SurfaceVerdict::Unsupported {
                            mechanism: mechanism.to_owned(),
                            detail: format!(
                                "the production consumer refused {} rows bound to \
                                 {key}: {reason}",
                                rows.len()
                            ),
                        },
                        Ok((0, refused)) => SurfaceVerdict::Unsupported {
                            mechanism: mechanism.to_owned(),
                            detail: format!(
                                "{} startup rows resolve a record in {key} and the \
                                     consumer started none of them ({refused} refused)",
                                rows.len()
                            ),
                        },
                        Ok((running, refused)) => SurfaceVerdict::Satisfied {
                            consumer: format!(
                                "mission_animations::MissionAnimationPlayer started \
                                 {running} of {} {key} rows through \
                                 animation::mission::bind_mission_animation ({refused} \
                                 refused)",
                                rows.len()
                            ),
                        },
                    }
                }
            }
        }
    };
    SurfaceReport {
        surface,
        assets,
        verdict,
    }
}

/// Starts one carrier's rows through the production consumer exactly as a
/// mission host would: one [`MissionAnimationPlayer`], each startup event's
/// rows offered under that event's own label.
///
/// The tick rate is the **host's** timeline (the plan claims no original
/// rate — `f20-anim.tick-rate-unmeasured` stands); nothing here is advanced
/// or rendered, the run only proves the consumer takes these rows.
///
/// # Errors
///
/// The player's own construction refusals, carried verbatim.
fn started_rows(rows: &[StartupAnimation]) -> Result<(usize, usize), String> {
    let session = SessionId::new(1).expect("one is a live session id");
    let mut player = MissionAnimationPlayer::new(session, 64).map_err(|error| error.to_string())?;
    let mut events: Vec<(String, Vec<StartupAnimation>)> = Vec::new();
    for row in rows {
        match events.iter_mut().find(|(event, _)| event == row.event()) {
            Some((_, group)) => group.push(row.clone()),
            None => events.push((row.event().to_owned(), vec![row.clone()])),
        }
    }
    for (event, group) in &events {
        let _ = player.start(event, Tick(0), group);
    }
    Ok((player.running_count(), player.refused_count()))
}

/// Sound archives inside the mission's scope: the mission directory, the
/// group directory, and `zbd/` itself. Playback machinery exists
/// (`AudioPlugin`, `RadioQueue`, `MusicDirector`) and #635's audible
/// `AudioDevice` backend consumes them, so the surface is satisfied exactly
/// when a sound-family archive is found in the scope.
fn measure_audio(
    install_root: &Path,
    found: &Discovery,
    plan: &MissionLaunchPlan,
    surface: LaunchSurface,
) -> SurfaceReport {
    let mut assets = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        let in_scope = key.starts_with(&format!("{}/", plan.mission_dir))
            || key.starts_with(&format!("{}/", plan.group_dir))
            || (key.starts_with("zbd/") && key.matches('/').count() == 1);
        if !(in_scope && key.ends_with(".zbd")) {
            continue;
        }
        if let Ok(asset) = read_archive_header(install_root, found, &key)
            && asset.family.eq_ignore_ascii_case("sound")
        {
            assets.push(asset);
        }
    }
    assets.sort_by(|left, right| left.key.cmp(&right.key));
    let verdict = if assets.is_empty() {
        SurfaceVerdict::Unknown {
            detail: "no sound-family archive was found in the mission scope".to_owned(),
        }
    } else {
        SurfaceVerdict::Satisfied {
            consumer: "audio::AudibleDevice over the decoded sound archives".to_owned(),
        }
    };
    SurfaceReport {
        surface,
        assets,
        verdict,
    }
}

/// Lists a reader archive and decodes each member: `.zrd` members through
/// `decode_zrd`, every other member kept opaque, and a member the reader
/// refuses kept as a refused row.
fn read_reader_members(
    install_root: &Path,
    found: &Discovery,
    key: &str,
) -> Result<AssetReport, AssetReport> {
    let Some(record) = manifest_record(found, key) else {
        return Err(AssetReport {
            key: key.to_owned(),
            sha256: String::new(),
            family: String::new(),
            members: Vec::new(),
            unreadable: Some("not present in the discovered manifest".to_owned()),
        });
    };
    let sha256 = record.sha256.to_hex();
    let bytes = read_member_bytes(install_root, record).map_err(|reason| AssetReport {
        key: key.to_owned(),
        sha256: sha256.clone(),
        family: String::new(),
        members: Vec::new(),
        unreadable: Some(reason),
    })?;
    let mut context = ParseContext::with_defaults(key);
    let decision =
        dispatch(ZbdProbe::new(key, &record.relative_spelling, &bytes)).map_err(|error| {
            AssetReport {
                key: key.to_owned(),
                sha256: sha256.clone(),
                family: String::new(),
                members: Vec::new(),
                unreadable: Some(format!("dispatch refused: {error}")),
            }
        })?;
    let family = format!("{:?}", decision.family());
    let index =
        read_version_one_index(&mut context, decision, &bytes).map_err(|error| AssetReport {
            key: key.to_owned(),
            sha256: sha256.clone(),
            family: family.clone(),
            members: Vec::new(),
            unreadable: Some(format!("index refused: {error}")),
        })?;
    let table = index.member_table();
    let archive =
        read_reader_archive(&mut context, &table, index.data()).map_err(|error| AssetReport {
            key: key.to_owned(),
            sha256: sha256.clone(),
            family: family.clone(),
            members: Vec::new(),
            unreadable: Some(format!("reader refused: {error}")),
        })?;
    let members = archive
        .entries()
        .map(|entry| {
            let name = String::from_utf8_lossy(entry.name()).into_owned();
            let verdict = if name.to_ascii_lowercase().ends_with(".zrd") {
                match decode_zrd(entry.content()) {
                    Ok(_) => MemberVerdict::Document,
                    Err(error) => MemberVerdict::Refused {
                        reason: error.to_string(),
                    },
                }
            } else {
                MemberVerdict::Opaque
            };
            MemberReport {
                name,
                bytes: entry.content().len() as u64,
                verdict,
            }
        })
        .collect();
    Ok(AssetReport {
        key: key.to_owned(),
        sha256,
        family,
        members,
        unreadable: None,
    })
}

/// How many bytes a classification probe needs: far past every documented
/// signature rule's `required_bytes`, and nowhere near a 128 MiB sound
/// archive's full read.
const HEADER_PROBE_BYTES: usize = 65_536;

/// Reads a container's header far enough to classify it: dispatch over a
/// bounded prefix of the bytes (the probe never needs the whole member for a
/// family decision).
fn read_archive_header(
    install_root: &Path,
    found: &Discovery,
    key: &str,
) -> Result<AssetReport, AssetReport> {
    let Some(record) = manifest_record(found, key) else {
        return Err(AssetReport {
            key: key.to_owned(),
            sha256: String::new(),
            family: String::new(),
            members: Vec::new(),
            unreadable: Some("not present in the discovered manifest".to_owned()),
        });
    };
    let sha256 = record.sha256.to_hex();
    let host: PathBuf = install_root.join(record.relative_spelling.as_str());
    let header = std::fs::File::open(&host)
        .and_then(|mut file| {
            use std::io::Read;
            let mut header = vec![0u8; HEADER_PROBE_BYTES];
            let read = file.read(&mut header)?;
            header.truncate(read);
            Ok(header)
        })
        .map_err(|reason| AssetReport {
            key: key.to_owned(),
            sha256: sha256.clone(),
            family: String::new(),
            members: Vec::new(),
            unreadable: Some(format!("cannot read {}: {reason}", host.display())),
        })?;
    let decision =
        dispatch(ZbdProbe::new(key, &record.relative_spelling, &header)).map_err(|error| {
            AssetReport {
                key: key.to_owned(),
                sha256: sha256.clone(),
                family: String::new(),
                members: Vec::new(),
                unreadable: Some(format!("dispatch refused: {error}")),
            }
        })?;
    Ok(AssetReport {
        key: key.to_owned(),
        sha256,
        family: format!("{:?}", decision.family()),
        members: Vec::new(),
        unreadable: None,
    })
}

/// A surface whose one archive could not be read: the asset row carries the
/// refusal verbatim and the verdict stays `Unknown` — unreadable bytes are
/// a gap to name, not a mechanism to file.
fn asset_refusal(
    surface: LaunchSurface,
    _found: &Discovery,
    record: &InstallFileRecord,
    key: &str,
    reason: String,
) -> SurfaceReport {
    SurfaceReport {
        surface,
        assets: vec![AssetReport {
            key: key.to_owned(),
            sha256: record.sha256.to_hex(),
            family: String::new(),
            members: Vec::new(),
            unreadable: Some(reason.clone()),
        }],
        verdict: SurfaceVerdict::Unknown { detail: reason },
    }
}

/// The manifest row for a logical key, if the installation declares it.
fn manifest_record<'a>(found: &'a Discovery, key: &str) -> Option<&'a InstallFileRecord> {
    found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
}

/// Reads one manifest row's container bytes.
fn read_member_bytes(install_root: &Path, record: &InstallFileRecord) -> Result<Vec<u8>, String> {
    let host: PathBuf = install_root.join(record.relative_spelling.as_str());
    std::fs::read(&host).map_err(|error| format!("cannot read {}: {error}", host.display()))
}
