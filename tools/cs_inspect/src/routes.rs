//! The `routes` command: expose the declared route graph contract as a
//! read-only inspection report (F31-A) and, with `--follow`, run the
//! production navigation follower over the projected route (F31-C).
//!
//! ```text
//! cs-inspect routes [--follow] [--out <file>]
//! cs-inspect routes --coverage [--cs-path <dir>] [--out <file>]
//! ```
//!
//! Without `--follow` the report renders the provenance-carrying declared route
//! record [`cs_content::routes::declared_synthetic_arch_route`] — the authored
//! id, origin, reference frame, termination, clearance and every node with its
//! stable id, authored sequence, mandatory flag, resolved position and
//! resolved trigger volume — plus the declared edges. It is the content half
//! of the F31 route contract; the typed runtime consumer
//! (`cs_sim::ai::navigation`) is not linked into `cs-inspect`, so this command
//! makes no claim about follow behavior.
//!
//! With `--follow` this command is the F31-C **conversion boundary**: it
//! resolves the declared record ([`ResolvedRoute`]), projects it into the
//! runtime [`RouteGraph`] through [`project_route`] and drives the production
//! [`NavigationSet`] with [`cs_sim::ai::navigation::follow_route`] — a
//! displaced actor rejoining before its next mandatory marker, the teardown of
//! the actor and a retry in a fresh session generation. An unknown position,
//! arrival radius or clearance, an unsupported loop termination and an unbound
//! moving anchor are propagated as named errors, never defaulted.
//!
//! Every value the record carries is newly authored project design
//! ([`Origin::SyntheticFixture`]); the report names its synthetic source and
//! is never retail-ready, so a synthetic route can never be mistaken for an
//! original mission route. Unknown positions, trigger volumes and clearances
//! are reported as explicit unknowns with their claim id and reason, never as
//! a silent zero. No original route is parsed: the original encoding is still
//! unmeasured (F13), so an original-data route claim belongs to F31-D.
//!
//! With `--coverage` the command is the F31-D **retail route-coverage audit**
//! (task #128): it walks every mission directory `ZBD/<group>/<mission>` of the
//! installation, classifies each by mission type
//! ([`cs_content::routes::classify_mission_type`]: campaign `M##`, Instant
//! Action `IA#`, multiplayer `MP#`) and checks that the mission's reader
//! archive dispatches and locates the observed AI-navigation control carrier
//! member (`aiv.zrd`). Coverage fails closed (exit 3) when any mission of any
//! type is missing its archive or that carrier. The audit is a **carrier
//! coverage**, not a route decode: the original route encoding, node layout,
//! units and trigger rule remain unmeasured, and the report says so
//! (`"route_encoding":{"state":"unmeasured"}`), so nothing here is a claim about
//! the original game's routes. `--cs-path` wins over `CS_GAME_DIR`; with neither,
//! the retail capability is missing (exit 4).
//!
//! ```text
//! exit 0  the report was produced (and, with --coverage, coverage holds)
//! exit 2  invalid input (unknown flag, missing value, conflicting mode)
//! exit 3  with --coverage: a mission type has no mission, or a mission hides
//!         its archive or its AI-navigation carrier member
//! exit 4  with --coverage: no installation selected (no --cs-path, no CS_GAME_DIR)
//! exit 1  a runtime failure building a report or writing --out
//! ```

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install::sha256;
use cs_content::routes::{
    AnchorKind, MissionType, ReferenceFrame, ResolvedRoute, RouteDefinition, RouteNode,
    RouteTermination, TriggerShape, TriggerVolume, classify_mission_type,
    declared_synthetic_arch_route,
};
use cs_formats::script_raw::{ProgramKind, discover_container};
use cs_formats::zbd::ZbdFamily;
use cs_sim::ai::navigation::{
    FollowPlan, NavState, NavigationCadence, NavigationError, NavigationSet, Navigator,
    PursuitRequest, ReferenceFrameSample, RouteFrame, RouteGraph, RouteGraphError,
    RouteNode as NavRouteNode, RouteNodeId, SYNTHETIC_PURSUIT_SEED, SYNTHETIC_PURSUIT_SESSION,
    follow_route, heading_from_direction, synthetic_maneuver_envelope, synthetic_pursuit_actor,
    synthetic_pursuit_start,
};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, Provenance, Resolved};
use cs_types::install::RelativePath;

use crate::catalog::{json_string, report_run, write_atomic};

/// The report schema version this consumer writes.
pub const ROUTES_REPORT_VERSION: &str = "cs-inspect-routes/v1";

/// The report schema version the F31-C `--follow` consumer writes.
pub const ROUTES_FOLLOW_REPORT_VERSION: &str = "cs-inspect-routes-follow/v1";

/// The report schema version the F31-D `--coverage` consumer writes.
pub const ROUTES_COVERAGE_REPORT_VERSION: &str = "cs-inspect-routes-coverage/v1";

/// The observed mission AI-navigation control member the F31-D coverage audit
/// requires in every mission directory (F13-B observation: a name rule, not a
/// decode).
pub const ROUTE_CARRIER_MEMBER: &str = "aiv.zrd";

/// The claim id the F31-D coverage report cites for the still-unmeasured
/// original route encoding.
pub const ROUTE_ENCODING_CLAIM_ID: &str = "f31d.route_encoding";

/// The `source` label of the declared synthetic route report; it can never be
/// `installation`.
pub const SYNTHETIC_ROUTES_SOURCE_LABEL: &str = "synthetic-fixture";

/// Parsed `routes` arguments.
#[derive(Debug, Default)]
struct RoutesArgs {
    /// The `--out` report path; `None` writes the report to stdout.
    out: Option<PathBuf>,
    /// Run the production follower over the projected route instead of
    /// rendering the declared record (F31-C).
    follow: bool,
    /// Run the F31-D retail route-coverage audit over an installation.
    coverage: bool,
    /// The installation `--coverage` reads; wins over `CS_GAME_DIR`.
    cs_path: Option<PathBuf>,
}

/// Everything one `routes` run produced.
#[derive(Debug)]
pub struct RoutesRun {
    /// The `CLI-EVIDENCE` exit code.
    pub exit_code: u8,
    /// The JSON report, when it was produced.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
    /// The counts the record report declares, when it was produced.
    pub summary: Option<RoutesSummary>,
    /// The counts the `--follow` report declares, when it was produced.
    pub follow: Option<FollowSummary>,
    /// The counts the `--coverage` report declares, when it was produced.
    pub coverage: Option<RouteCoverageSummary>,
}

/// The counts one `routes --coverage` report declares (F31-D).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteCoverageSummary {
    /// How many mission directories the installation declares.
    pub missions: usize,
    /// How many of them carry the AI-navigation control carrier member.
    pub covered: usize,
    /// How many mission directories are campaign `M##` missions.
    pub campaign: usize,
    /// How many mission directories are Instant Action `IA#` scenarios.
    pub instant_action: usize,
    /// How many mission directories are multiplayer `MP#` scenarios.
    pub multiplayer: usize,
    /// How many mission directories are none of the three above.
    pub other: usize,
}

impl RouteCoverageSummary {
    /// Whether coverage passes: the installation declares at least one mission
    /// directory and every one of them carries the carrier member.
    ///
    /// An installation with no mission directory is *not* covered: an empty
    /// denominator must never read as a pass.
    #[must_use]
    pub fn covered(&self) -> bool {
        self.missions > 0 && self.covered == self.missions
    }

    /// The count for one mission type label.
    #[must_use]
    pub fn count(&self, mission_type: MissionType) -> usize {
        match mission_type {
            MissionType::Campaign => self.campaign,
            MissionType::InstantAction => self.instant_action,
            MissionType::Multiplayer => self.multiplayer,
            MissionType::Other => self.other,
        }
    }
}

/// The counts one `routes` record report declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutesSummary {
    /// How many nodes the route declares.
    pub nodes: usize,
    /// How many edges the route declares.
    pub edges: usize,
    /// How many nodes are mandatory markers.
    pub mandatory: usize,
    /// Whether the route claims original installation data.
    pub retail: bool,
}

/// The counts one `routes --follow` report declares (F31-C).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FollowSummary {
    /// How many nodes the projected runtime route has.
    pub nodes: usize,
    /// The tick the displaced follower reached its first mandatory marker on,
    /// or `None` when it never did.
    pub rejoin_tick: Option<u64>,
    /// Whether the displaced follower rejoined before the next mandatory
    /// marker.
    pub rejoined: bool,
    /// Whether the follower's whole actor roster was torn down.
    pub torn_down: bool,
    /// Whether a retry in a fresh session generation started from scratch.
    pub retry_reset: bool,
    /// Whether a stale command generation was refused rather than followed.
    pub stale_generation_refused: bool,
}

impl RoutesRun {
    fn failed(exit_code: u8, message: String) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![message],
            summary: None,
            follow: None,
            coverage: None,
        }
    }
}

fn parse_routes_args(args: &[String]) -> Result<RoutesArgs, String> {
    let mut parsed = RoutesArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        match flag {
            "--follow" => {
                parsed.follow = true;
                continue;
            }
            "--coverage" => {
                parsed.coverage = true;
                continue;
            }
            "--out" | "--cs-path" => {
                let Some(value) = cursor.next() else {
                    return Err(format!("cs-inspect routes: {flag} needs a value"));
                };
                if flag == "--out" {
                    parsed.out = Some(PathBuf::from(value));
                } else {
                    parsed.cs_path = Some(PathBuf::from(value));
                }
                continue;
            }
            other => {
                return Err(format!(
                    "cs-inspect routes: unsupported argument {other:?}; expected --follow, \
                     --coverage, --cs-path <dir> or --out <file>"
                ));
            }
        }
    }
    if parsed.follow && parsed.coverage {
        return Err(
            "cs-inspect routes: --follow and --coverage are different reports; pass one".to_owned(),
        );
    }
    if parsed.follow && parsed.cs_path.is_some() {
        return Err(
            "cs-inspect routes: --cs-path reads an installation and belongs to --coverage; the \
             declared record and --follow read no installation"
                .to_owned(),
        );
    }
    Ok(parsed)
}

/// Runs the `routes` command and returns its exit code.
///
/// `--out` is written atomically and its final path is reported on stderr;
/// without `--out` the JSON report goes to stdout. A failure is never returned
/// as success.
pub fn routes_command(args: &[String]) -> ExitCode {
    let run = routes_command_result_with_env(args, std::env::var_os("CS_GAME_DIR"));
    report_run(
        "routes",
        &run.diagnostics,
        run.report.as_deref(),
        run.out.as_deref(),
    );
    ExitCode::from(run.exit_code)
}

/// The body of [`routes_command`] reading the environment's `CS_GAME_DIR`,
/// separate so a failure carries its named exit code and the report can be
/// inspected without touching stdout.
pub fn routes_command_result(args: &[String]) -> RoutesRun {
    routes_command_result_with_env(args, std::env::var_os("CS_GAME_DIR"))
}

/// The body of [`routes_command`] with the environment's installation passed
/// in, so a test can drive `--coverage` without touching the process
/// environment.
pub fn routes_command_result_with_env(args: &[String], env_cs_path: Option<OsString>) -> RoutesRun {
    let parsed = match parse_routes_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return RoutesRun::failed(2, message),
    };

    if parsed.follow {
        return finish_follow_run(parsed.out);
    }

    if parsed.coverage {
        return finish_coverage_run(parsed.cs_path, env_cs_path, parsed.out);
    }

    let route = declared_synthetic_arch_route();
    let summary = RoutesSummary {
        nodes: route.nodes().len(),
        edges: route.edges().len(),
        mandatory: route.mandatory_node_count(),
        retail: route.origin().is_original(),
    };
    let report = routes_report(&route);
    match parsed.out {
        Some(out) => match write_atomic(&out, &report) {
            Ok(()) => RoutesRun {
                exit_code: 0,
                report: Some(report),
                out: Some(out),
                diagnostics: Vec::new(),
                summary: Some(summary),
                follow: None,
                coverage: None,
            },
            Err(error) => RoutesRun {
                exit_code: 1,
                report: Some(report),
                out: None,
                diagnostics: vec![format!(
                    "cs-inspect routes: cannot write report to {}: {error}",
                    out.display()
                )],
                summary: Some(summary),
                follow: None,
                coverage: None,
            },
        },
        None => RoutesRun {
            exit_code: 0,
            report: Some(report),
            out: None,
            diagnostics: Vec::new(),
            summary: Some(summary),
            follow: None,
            coverage: None,
        },
    }
}

/// Builds the F31-C `--follow` report and writes it like the record report.
fn finish_follow_run(out: Option<PathBuf>) -> RoutesRun {
    let built = match build_follow_report() {
        Ok(built) => built,
        Err(message) => return RoutesRun::failed(1, message),
    };
    let BuiltFollowReport { report, summary } = built;
    match out {
        Some(out) => match write_atomic(&out, &report) {
            Ok(()) => RoutesRun {
                exit_code: 0,
                report: Some(report),
                out: Some(out),
                diagnostics: Vec::new(),
                summary: None,
                follow: Some(summary),
                coverage: None,
            },
            Err(error) => RoutesRun {
                exit_code: 1,
                report: Some(report),
                out: None,
                diagnostics: vec![format!(
                    "cs-inspect routes: cannot write report to {}: {error}",
                    out.display()
                )],
                summary: None,
                follow: Some(summary),
                coverage: None,
            },
        },
        None => RoutesRun {
            exit_code: 0,
            report: Some(report),
            out: None,
            diagnostics: Vec::new(),
            summary: None,
            follow: Some(summary),
            coverage: None,
        },
    }
}

// ------------------------------------------------- F31-D: coverage audit ----

/// One mission directory the F31-D audit found, with the carrier check.
struct CoverageMission {
    mission_type: MissionType,
    group: String,
    mission: String,
    program_asset: String,
    program_present: bool,
    program_sha256: Option<String>,
    carrier_present: bool,
    mission_programs: usize,
    finding: Option<String>,
}

/// Runs the F31-D `--coverage` audit and writes the report.
fn finish_coverage_run(
    explicit: Option<PathBuf>,
    env_cs_path: Option<OsString>,
    out: Option<PathBuf>,
) -> RoutesRun {
    let cs_path = explicit.or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return RoutesRun::failed(
            4,
            "cs-inspect routes: no installation selected: pass --cs-path <dir> or set \
             CS_GAME_DIR"
                .to_owned(),
        );
    };

    let missions = match scan_mission_directories(&cs_path) {
        Ok(missions) => missions,
        Err(CoverageWalk::Runtime(message)) => return RoutesRun::failed(1, message),
    };

    let mut summary = RouteCoverageSummary::default();
    for mission in &missions {
        summary.missions += 1;
        *match mission.mission_type {
            MissionType::Campaign => &mut summary.campaign,
            MissionType::InstantAction => &mut summary.instant_action,
            MissionType::Multiplayer => &mut summary.multiplayer,
            MissionType::Other => &mut summary.other,
        } += 1;
        if mission.carrier_present {
            summary.covered += 1;
        }
    }

    let report = coverage_report(&cs_path, &missions);
    let coverage = summary.covered();
    let exit_code = if coverage { 0 } else { 3 };
    let diagnostics = if coverage {
        Vec::new()
    } else if missions.is_empty() {
        vec![format!(
            "cs-inspect routes: {} declares no ZBD/<group>/<mission> mission directory",
            cs_path.display()
        )]
    } else {
        missions
            .iter()
            .filter_map(|mission| mission.finding.clone())
            .collect()
    };

    match out {
        Some(out) => match write_atomic(&out, &report) {
            Ok(()) => RoutesRun {
                exit_code,
                report: Some(report),
                out: Some(out),
                diagnostics,
                summary: None,
                follow: None,
                coverage: Some(summary),
            },
            Err(error) => RoutesRun {
                exit_code: 1,
                report: Some(report),
                out: None,
                diagnostics: vec![format!(
                    "cs-inspect routes: cannot write report to {}: {error}",
                    out.display()
                )],
                summary: None,
                follow: None,
                coverage: Some(summary),
            },
        },
        None => RoutesRun {
            exit_code,
            report: Some(report),
            out: None,
            diagnostics,
            summary: None,
            follow: None,
            coverage: Some(summary),
        },
    }
}

/// Why the F31-D mission walk could not run at all. A mission that fails the
/// coverage check is *not* an error: it is a covered-or-not row and a failure
/// that is reported, not raised.
enum CoverageWalk {
    /// The installation could not be walked (no ZBD directory).
    Runtime(String),
}

/// Walks `ZBD/<group>/<mission>` and checks each mission's AI-navigation
/// carrier member, in canonical `(group, mission)` order.
///
/// Each mission is scoped exactly as F13-B's `mission_scope`: a directory two
/// levels below `ZBD`, holding a `zrdr.zbd` reader archive. The archive is
/// dispatched through the production F13-B `discover_container`, and the
/// carrier is present when a located mission program names the observed
/// [`ROUTE_CARRIER_MEMBER`]. Every failure becomes an explicit row, never a
/// dropped mission.
fn scan_mission_directories(install_root: &Path) -> Result<Vec<CoverageMission>, CoverageWalk> {
    let zbd = find_child_dir(install_root, "ZBD").ok_or_else(|| {
        CoverageWalk::Runtime(format!(
            "cs-inspect routes: {} is not a container directory the audit can walk",
            install_root.join("ZBD").display()
        ))
    })?;
    let zbd_name = zbd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ZBD".to_owned());

    let mut groups: Vec<(String, PathBuf)> = read_dir_named(&zbd);
    groups.sort_by(|left, right| left.0.cmp(&right.0));

    let mut missions = Vec::new();
    for (group, group_path) in groups {
        let mut mission_dirs: Vec<(String, PathBuf)> = read_dir_named(&group_path);
        mission_dirs.sort_by(|left, right| left.0.cmp(&right.0));
        for (mission, mission_path) in mission_dirs {
            missions.push(check_mission(&zbd_name, &group, &mission, &mission_path));
        }
    }
    Ok(missions)
}

/// Checks one mission directory for its `zrdr.zbd` and the AI-navigation
/// carrier member.
fn check_mission(zbd_name: &str, group: &str, mission: &str, dir: &Path) -> CoverageMission {
    let mission_type = classify_mission_type(mission);
    let program_asset = format!("{zbd_name}/{group}/{mission}/zrdr.zbd");
    let host = dir.join("zrdr.zbd");
    let mut row = CoverageMission {
        mission_type,
        group: group.to_owned(),
        mission: mission.to_owned(),
        program_asset: program_asset.clone(),
        program_present: false,
        program_sha256: None,
        carrier_present: false,
        mission_programs: 0,
        finding: None,
    };

    let bytes = match fs::read(&host) {
        Ok(bytes) => bytes,
        Err(_) => {
            row.finding = Some(format!(
                "{}: mission {group}/{mission} has no readable program archive {program_asset}",
                mission_type.label()
            ));
            return row;
        }
    };
    row.program_present = true;
    row.program_sha256 = Some(sha256(&bytes).to_hex());

    let Ok(relative) = RelativePath::new(&program_asset) else {
        row.finding = Some(format!(
            "{}: mission {group}/{mission} path {program_asset} is not a relative path",
            mission_type.label()
        ));
        return row;
    };
    let discovery = discover_container(&program_asset, &relative, &bytes);
    if discovery.family() != Some(ZbdFamily::Reader) {
        row.finding = Some(format!(
            "{}: mission {group}/{mission} archive {program_asset} is not a reader archive",
            mission_type.label()
        ));
        return row;
    }
    row.mission_programs = discovery
        .programs()
        .iter()
        .filter(|program| program.kind() == ProgramKind::Mission)
        .count();
    row.carrier_present = discovery
        .programs()
        .iter()
        .any(|program| program.locator().member() == Some(ROUTE_CARRIER_MEMBER));
    if !row.carrier_present {
        row.finding = Some(format!(
            "{}: mission {group}/{mission} archive {program_asset} has no {ROUTE_CARRIER_MEMBER} \
             AI-navigation carrier member",
            mission_type.label()
        ));
    }
    row
}

/// The directory entries of a directory as `(name, path)` for every child
/// directory. A directory that cannot be read yields no children: the caller
/// reports an empty mission set rather than crashing.
fn read_dir_named(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .collect()
}

/// The child directory of `root` whose name matches `wanted` without regard to
/// case.
fn find_child_dir(root: &Path, wanted: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    entries.flatten().find_map(|entry| {
        let path = entry.path();
        (path.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(wanted))
        .then_some(path)
    })
}

/// Renders the F31-D retail route-coverage report.
///
/// The rows are in canonical `(group, mission)` order; per-type summaries are
/// emitted in the fixed campaign/Instant Action/multiplayer/other order so the
/// bytes are stable. The report is byte-stable, names the installation it read
/// and states the unmeasured encoding explicitly: a carrier present in every
/// mission is never presented as a decoded route.
fn coverage_report(install_root: &Path, missions: &[CoverageMission]) -> String {
    let mut by_type: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    for mission in missions {
        let entry = by_type
            .entry(mission.mission_type.label())
            .or_insert((0, 0));
        entry.0 += 1;
        if mission.carrier_present {
            entry.1 += 1;
        }
    }
    let type_rows = [
        MissionType::Campaign,
        MissionType::InstantAction,
        MissionType::Multiplayer,
        MissionType::Other,
    ]
    .iter()
    .map(|mission_type| {
        let (count, covered) = by_type.get(mission_type.label()).copied().unwrap_or((0, 0));
        format!(
            "{{\"type\":{},\"mission_count\":{count},\"covered\":{covered}}}",
            json_string(mission_type.label()),
        )
    })
    .collect::<Vec<_>>()
    .join(",");

    let mission_rows = missions
        .iter()
        .map(|mission| {
            let digest = match &mission.program_sha256 {
                Some(digest) => json_string(digest),
                None => "null".to_owned(),
            };
            format!(
                "{{\"type\":{},\"group\":{},\"mission\":{},\"program_asset\":{},\
                 \"program_present\":{},\"program_sha256\":{},\"carrier_present\":{},\
                 \"mission_programs\":{}}}",
                json_string(mission.mission_type.label()),
                json_string(&mission.group),
                json_string(&mission.mission),
                json_string(&mission.program_asset),
                mission.program_present,
                digest,
                mission.carrier_present,
                mission.mission_programs,
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    let covered = missions.iter().filter(|m| m.carrier_present).count();
    format!(
        "{{\"schema\":{},\"source\":{},\"retail\":true,\
         \"route_encoding\":{{\"state\":\"unmeasured\",\"claim_id\":{},\
         \"reason\":\"the original route encoding, node layout, units and trigger rule are \
         unmeasured (F13 locates mission programs, not routes); this audit checks the \
         AI-navigation carrier member, not decoded routes\"}},\
         \"carrier_member\":{},\"mission_count\":{},\"covered\":{},\"coverage\":{},\
         \"types\":[{type_rows}],\"missions\":[{mission_rows}]}}",
        json_string(ROUTES_COVERAGE_REPORT_VERSION),
        json_string(&install_root.display().to_string()),
        json_string(ROUTE_ENCODING_CLAIM_ID),
        json_string(ROUTE_CARRIER_MEMBER),
        missions.len(),
        covered,
        covered == missions.len() && !missions.is_empty(),
    )
}

// ------------------------------------------------- F31-C: projection ------

/// Why a resolved declared route could not become a runtime [`RouteGraph`]
/// (F31-C).
#[derive(Clone, Debug, PartialEq)]
pub enum RouteProjectionError {
    /// A moving route named an anchor that the caller did not bind to a
    /// runtime actor id.
    UnboundAnchor {
        /// The authored anchor content id.
        anchor: String,
    },
    /// The route asks for a loop, which the runtime follower cannot express.
    UnsupportedTermination {
        /// The authored termination.
        termination: &'static str,
    },
    /// The projected graph failed the runtime's own validation.
    Graph(RouteGraphError),
}

impl std::fmt::Display for RouteProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnboundAnchor { anchor } => write!(
                f,
                "moving route anchor {anchor:?} is not bound to a runtime actor"
            ),
            Self::UnsupportedTermination { termination } => write!(
                f,
                "the runtime follower cannot express route termination {termination:?}"
            ),
            Self::Graph(error) => write!(f, "the projected route graph is invalid: {error}"),
        }
    }
}

impl std::error::Error for RouteProjectionError {}

/// The runtime id a moving route's authored anchor is bound to.
///
/// Binding the content anchor ([`ContentId`]) to the runtime actor id is the
/// F31-C wiring step: the producer record names the authored anchor, while the
/// runtime follower addresses the anchor's session actor. A moving route whose
/// anchor is not in the binding table is refused, never silently addressed at
/// an invented id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorBinding {
    /// The authored anchor content id.
    pub anchor: ContentId,
    /// The runtime actor id the anchor is bound to.
    pub runtime_id: u64,
}

/// Projects a resolved declared route into the runtime follower's
/// [`RouteGraph`] (F31-C).
///
/// The authored sequence becomes the runtime node key (the content record
/// guarantees sequences are strictly increasing and unique, and the follower's
/// progress is list-ordered), so a projected node can never be renamed by a
/// container reorder. Node positions and arrival radii are the known values the
/// producer resolved; the world/moving reference frame is carried across.
///
/// # Errors
///
/// [`RouteProjectionError::UnboundAnchor`] for a moving route whose anchor has
/// no binding, [`RouteProjectionError::UnsupportedTermination`] for a loop the
/// runtime cannot express, and [`RouteProjectionError::Graph`] when the
/// projected graph fails the runtime's validation.
pub fn project_route(
    route: &ResolvedRoute,
    anchors: &[AnchorBinding],
) -> Result<RouteGraph, RouteProjectionError> {
    let frame = match route.frame() {
        ReferenceFrame::World => RouteFrame::World,
        ReferenceFrame::Moving(anchor) => {
            let runtime_id = anchors
                .iter()
                .find(|binding| binding.anchor == anchor.anchor)
                .map(|binding| binding.runtime_id)
                .ok_or_else(|| RouteProjectionError::UnboundAnchor {
                    anchor: anchor.anchor.as_str().to_owned(),
                })?;
            RouteFrame::Moving { anchor: runtime_id }
        }
    };
    if route.termination() == RouteTermination::Loop {
        return Err(RouteProjectionError::UnsupportedTermination {
            termination: "loop",
        });
    }
    let nodes = route
        .nodes()
        .iter()
        .map(|node| NavRouteNode {
            id: RouteNodeId(node.sequence),
            sequence: node.sequence,
            mandatory: node.mandatory,
            position_m: node.position_m.value,
            arrival_radius_m: node.arrival_radius_m.value,
        })
        .collect();
    RouteGraph::try_new(frame, route.clearance_m().value, nodes)
        .map_err(RouteProjectionError::Graph)
}

// ------------------------------------------------- F31-C: follow probe -----

/// The designed fixed step of the `routes --follow` probe, in seconds.
pub const SYNTHETIC_FOLLOW_DT_S: f64 = 1.0 / 60.0;

/// The maximum ticks the `routes --follow` displaced-rejoin probe runs before
/// it reports a non-rejoin. Bounded so a wedged follower fails diagnostically.
pub const FOLLOW_MAX_TICKS: usize = 20_000;

/// The rendered `--follow` report and its declared counts.
struct BuiltFollowReport {
    report: String,
    summary: FollowSummary,
}

/// Runs the F31-C producer-to-consumer wiring end to end over the declared
/// synthetic route: resolve the record, project it, drive the production set
/// with a displaced actor to its first mandatory marker, tear the roster down
/// and retry in a fresh session generation.
///
/// # Errors
///
/// A `String` diagnostic naming the refused resolution, projection or decision
/// — a failure is never rendered as a successful report.
fn build_follow_report() -> Result<BuiltFollowReport, String> {
    let declared = declared_synthetic_arch_route();
    let resolved = declared.resolve().map_err(|error| {
        format!(
            "cs-inspect routes: the declared route cannot be resolved for the follower: {error}"
        )
    })?;
    let graph = project_route(&resolved, &[]).map_err(|error| {
        format!(
            "cs-inspect routes: cannot project the declared route into the runtime graph: {error}"
        )
    })?;

    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .map_err(|error| {
        format!("cs-inspect routes: the declared synthetic envelope is invalid: {error}")
    })?;
    let actor = synthetic_pursuit_actor(1);
    let mut set = NavigationSet::new(SYNTHETIC_PURSUIT_SESSION, SYNTHETIC_PURSUIT_SEED, navigator);
    // The synthetic spawn point is the route's first node, which is not a
    // marker, so the actor resumes past it exactly as a mission spawn does.
    set.register_resuming(actor, 1)
        .map_err(|error| format!("cs-inspect routes: cannot register the probe actor: {error}"))?;

    // Displaced off the route and pointed away from it; a follower with no
    // remembered route memory would drive away from the marker instead of
    // rejoining.
    let displaced = NavState {
        position_m: [40.0, 0.0, -40.0],
        heading_rad: heading_from_direction(0.0, 1.0),
        speed_mps: 40.0,
        climb_mps: 0.0,
    };
    let outcome = follow_route(
        &mut set,
        actor,
        FollowPlan {
            route: &graph,
            blockers: &[],
            dt_s: SYNTHETIC_FOLLOW_DT_S,
            start: displaced,
            max_ticks: FOLLOW_MAX_TICKS,
        },
        |_| ReferenceFrameSample::IDENTITY,
    )
    .map_err(|error| format!("cs-inspect routes: the follower refused its request: {error}"))?;

    // The first mandatory marker of the route and the tick the follower first
    // carried its progress past it.
    let mandatory_index = graph.nodes.iter().position(|node| node.mandatory);
    let rejoin_tick = mandatory_index.and_then(|index| {
        outcome
            .decisions
            .iter()
            .find(|decision| decision.decision.progress.reached() == index + 1)
            .map(|decision| decision.decision.tick.0)
    });
    let first_mandatory_sequence = mandatory_index.map(|index| graph.nodes[index].sequence);

    // Teardown: an actor despawn removes its pursuit state, and a retry in a
    // fresh session generation starts from an empty roster rather than
    // inheriting the torn-down actor's progress.
    let removed = set.unregister(actor);
    let torn_down = removed && set.is_empty();
    let retry_session = SYNTHETIC_PURSUIT_SESSION + 1;
    let retry_actor = ActorId {
        session: retry_session,
        serial: 1,
    };
    let mut retry = NavigationSet::new(retry_session, SYNTHETIC_PURSUIT_SEED, *set.navigator());
    retry
        .register(retry_actor)
        .map_err(|error| format!("cs-inspect routes: cannot register the retry actor: {error}"))?;
    let retry_reset = retry
        .state(retry_actor)
        .is_some_and(|state| state.progress().reached() == 0);

    // Error propagation: a command carrying the previous session generation is
    // refused, never silently followed by the retry set.
    let stale = PursuitRequest {
        actor: retry_actor,
        tick: Tick(0),
        generation: SYNTHETIC_PURSUIT_SESSION,
        state: synthetic_pursuit_start(),
        route: &graph,
        frame: ReferenceFrameSample::IDENTITY,
        blockers: &[],
        dt_s: SYNTHETIC_FOLLOW_DT_S,
    };
    let stale_generation_refused = matches!(
        retry.decide(&stale),
        Err(NavigationError::ForeignSession { expected, found })
            if expected == retry_session && found == SYNTHETIC_PURSUIT_SESSION
    );

    let rejoined = rejoin_tick.is_some();
    let report = format!(
        "{{\"schema\":{},\"source\":{},\"retail\":{},\
         \"route\":{{\"id\":{},\"frame\":{},\"clearance_m\":{},\"node_count\":{},\
         \"mandatory_count\":{}}},\
         \"follow\":{{\"displaced_start_m\":[{},{},{}],\"ticks\":{},\"reached\":{},\
         \"complete\":{},\"blocked\":{},\"first_mandatory_sequence\":{},\"rejoin_tick\":{},\
         \"rejoined\":{}}},\
         \"lifecycle\":{{\"torn_down\":{},\"retry_session\":{},\"retry_reset\":{},\
         \"stale_generation_refused\":{}}}}}",
        json_string(ROUTES_FOLLOW_REPORT_VERSION),
        json_string(SYNTHETIC_ROUTES_SOURCE_LABEL),
        resolved.origin().is_original(),
        json_string(resolved.id().as_str()),
        render_frame(resolved.frame()),
        render_f64(resolved.clearance_m().value),
        graph.node_count(),
        graph.nodes.iter().filter(|node| node.mandatory).count(),
        render_f64(displaced.position_m[0]),
        render_f64(displaced.position_m[1]),
        render_f64(displaced.position_m[2]),
        outcome.decisions.len(),
        outcome.progress.reached(),
        outcome.complete,
        outcome.blocked_at.is_some(),
        first_mandatory_sequence.map_or_else(|| "null".to_owned(), |value| value.to_string()),
        rejoin_tick.map_or_else(|| "null".to_owned(), |value| value.to_string()),
        rejoined,
        torn_down,
        retry_session,
        retry_reset,
        stale_generation_refused,
    );
    Ok(BuiltFollowReport {
        report,
        summary: FollowSummary {
            nodes: graph.node_count(),
            rejoin_tick,
            rejoined,
            torn_down,
            retry_reset,
            stale_generation_refused,
        },
    })
}

/// Renders the deterministic JSON route-graph report for one declared route.
///
/// The node and edge rows keep the record's authored order; the report does
/// not reorder them, so the canonical order stays a property of the record
/// rather than of this renderer. Every string is escaped by [`json_string`],
/// so an authored key can never break out of its field.
pub fn routes_report(route: &RouteDefinition) -> String {
    let nodes = route
        .nodes()
        .iter()
        .map(render_node)
        .collect::<Vec<_>>()
        .join(",");
    let edges = route
        .edges()
        .iter()
        .map(|edge| {
            format!(
                "{{\"from\":{},\"to\":{}}}",
                json_string(edge.from.as_str()),
                json_string(edge.to.as_str()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"schema\":{},\"source\":{},\"retail\":{},\"route\":{{\"id\":{},\"origin\":{},\
         \"frame\":{},\"termination\":{},\"clearance_m\":{},\"node_count\":{},\
         \"edge_count\":{},\"mandatory_count\":{},\"nodes\":[{}],\"edges\":[{}]}}}}",
        json_string(ROUTES_REPORT_VERSION),
        json_string(SYNTHETIC_ROUTES_SOURCE_LABEL),
        route.origin().is_original(),
        json_string(route.id().as_str()),
        json_string(route.origin().label()),
        render_frame(route.frame()),
        json_string(termination_label(route.termination())),
        render_resolved_f64(route.clearance_m()),
        route.nodes().len(),
        route.edges().len(),
        route.mandatory_node_count(),
        nodes,
        edges,
    )
}

fn render_node(node: &RouteNode) -> String {
    format!(
        "{{\"id\":{},\"sequence\":{},\"mandatory\":{},\"position_m\":{},\
         \"arrival_radius_m\":{},\"trigger\":{}}}",
        json_string(node.id.as_str()),
        node.sequence,
        node.mandatory,
        render_resolved_position(&node.position_m),
        render_resolved_f64(&node.arrival_radius_m),
        render_resolved_trigger(&node.trigger),
    )
}

fn termination_label(termination: RouteTermination) -> &'static str {
    match termination {
        RouteTermination::End => "end",
        RouteTermination::Loop => "loop",
    }
}

fn anchor_kind_label(kind: AnchorKind) -> &'static str {
    match kind {
        AnchorKind::Carrier => "carrier",
        AnchorKind::Train => "train",
        AnchorKind::Escort => "escort",
        AnchorKind::Other => "other",
    }
}

fn render_frame(frame: &ReferenceFrame) -> String {
    match frame {
        ReferenceFrame::World => "\"world\"".to_owned(),
        ReferenceFrame::Moving(anchor) => format!(
            "{{\"moving\":{{\"anchor\":{},\"kind\":{}}}}}",
            json_string(anchor.anchor.as_str()),
            json_string(anchor_kind_label(anchor.kind)),
        ),
    }
}

fn render_resolved_f64(value: &Resolved<f64>) -> String {
    match value {
        Resolved::Known(known) => format!(
            "{{\"state\":\"known\",\"value\":{},\"provenance\":{}}}",
            render_f64(known.value),
            render_provenance(&known.provenance),
        ),
        Resolved::Unknown { claim_id, reason } => format!(
            "{{\"state\":\"unknown\",\"claim_id\":{},\"reason\":{}}}",
            json_string(claim_id.as_str()),
            json_string(reason),
        ),
    }
}

fn render_resolved_position(value: &Resolved<[f64; 3]>) -> String {
    match value {
        Resolved::Known(known) => format!(
            "{{\"state\":\"known\",\"value\":[{},{},{}],\"provenance\":{}}}",
            render_f64(known.value[0]),
            render_f64(known.value[1]),
            render_f64(known.value[2]),
            render_provenance(&known.provenance),
        ),
        Resolved::Unknown { claim_id, reason } => format!(
            "{{\"state\":\"unknown\",\"claim_id\":{},\"reason\":{}}}",
            json_string(claim_id.as_str()),
            json_string(reason),
        ),
    }
}

/// Renders the three-valued trigger resolution: a known volume, a known
/// absence (`null`) or an explicit unknown.
fn render_resolved_trigger(value: &Resolved<Option<TriggerVolume>>) -> String {
    match value {
        Resolved::Known(known) => {
            let volume = match &known.value {
                Some(volume) => render_volume(volume),
                None => "null".to_owned(),
            };
            format!(
                "{{\"state\":\"known\",\"value\":{},\"provenance\":{}}}",
                volume,
                render_provenance(&known.provenance),
            )
        }
        Resolved::Unknown { claim_id, reason } => format!(
            "{{\"state\":\"unknown\",\"claim_id\":{},\"reason\":{}}}",
            json_string(claim_id.as_str()),
            json_string(reason),
        ),
    }
}

fn render_volume(volume: &TriggerVolume) -> String {
    let shape = match volume.shape {
        TriggerShape::Sphere { radius_m } => {
            format!(
                "{{\"kind\":\"sphere\",\"radius_m\":{}}}",
                render_f64(radius_m)
            )
        }
        TriggerShape::AxisAlignedBox { half_extents_m } => format!(
            "{{\"kind\":\"axis_aligned_box\",\"half_extents_m\":[{},{},{}]}}",
            render_f64(half_extents_m[0]),
            render_f64(half_extents_m[1]),
            render_f64(half_extents_m[2]),
        ),
    };
    format!(
        "{{\"id\":{},\"shape\":{}}}",
        json_string(volume.id.as_str()),
        shape,
    )
}

fn render_provenance(provenance: &Provenance) -> String {
    let source = match &provenance.source {
        Some(span) => json_string(&span.to_string()),
        None => "null".to_owned(),
    };
    format!(
        "{{\"claim_id\":{},\"class\":{},\"source\":{}}}",
        json_string(provenance.claim_id.as_str()),
        json_string(provenance.class.label()),
        source,
    )
}

/// A canonical finite meter value: JSON has no NaN or infinity, and the route
/// record already refuses non-finite known values, so this never emits one.
fn render_f64(value: f64) -> String {
    let mut text = String::new();
    let _ = write!(text, "{value}");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report names its synthetic source, is never retail and carries
    /// every node and edge of the declared fixture in authored order.
    #[test]
    fn accept_f31_a_routes_report_names_synthetic_source_and_lists_every_node() {
        let report = routes_report(&declared_synthetic_arch_route());
        assert!(report.contains(&format!("\"schema\":\"{ROUTES_REPORT_VERSION}\"")));
        assert!(report.contains(&format!("\"source\":\"{SYNTHETIC_ROUTES_SOURCE_LABEL}\"")));
        assert!(report.contains("\"retail\":false"));
        assert!(report.contains("\"id\":\"route/synthetic.arch\""));
        assert!(report.contains("\"origin\":\"synthetic_fixture\""));
        assert!(report.contains("\"frame\":\"world\""));
        assert!(report.contains("\"termination\":\"end\""));
        assert!(report.contains("\"clearance_m\":{\"state\":\"known\",\"value\":2"));
        assert!(report.contains("\"node_count\":5"));
        assert!(report.contains("\"edge_count\":4"));
        assert!(report.contains("\"mandatory_count\":4"));

        for id in ["start", "funnel", "arch", "exit", "goal"] {
            assert!(
                report.contains(&format!("\"id\":\"{id}\"")),
                "node {id} is listed: {report}"
            );
        }
        assert!(report.contains("\"from\":\"start\",\"to\":\"funnel\""));
        assert!(report.contains("\"from\":\"exit\",\"to\":\"goal\""));

        // The arch marker carries its authored trigger volume; a plain
        // waypoint carries an explicit known absence.
        assert!(
            report.contains(
                "\"id\":\"synthetic.arch.opening\",\"shape\":{\"kind\":\"sphere\",\"radius_m\":5"
            ),
            "the arch trigger volume is reported: {report}"
        );
        assert!(report.contains("{\"state\":\"known\",\"value\":null"));
    }

    /// The command writes `--out` atomically and is byte-stable; a bad flag or
    /// a missing value is invalid input and reports nothing.
    #[test]
    fn accept_f31_a_routes_command_writes_out_and_refuses_bad_input() {
        let temp = std::env::temp_dir().join(format!("cs-f31-a-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).expect("the fixture directory is created");
        let path = temp.join("routes.json");

        let written = routes_command_result(&["--out".to_owned(), path.display().to_string()]);
        assert_eq!(written.exit_code, 0);
        assert_eq!(written.out.as_deref(), Some(path.as_path()));
        let on_disk = std::fs::read_to_string(&path).expect("the report is written");
        assert_eq!(Some(&on_disk), written.report.as_ref());

        let stdout = routes_command_result(&[]);
        assert_eq!(stdout.exit_code, 0);
        assert_eq!(
            on_disk,
            stdout.report.expect("the report is stable"),
            "the same record serializes byte-for-byte identically"
        );

        assert_eq!(routes_command_result(&["--out".to_owned()]).exit_code, 2);
        assert_eq!(routes_command_result(&["--nope".to_owned()]).exit_code, 2);

        let _ = std::fs::remove_dir_all(&temp);
    }

    /// The coverage summary never passes an empty denominator, and its report
    /// states the unmeasured encoding and every per-type row.
    #[test]
    fn accept_f31_d_coverage_summary_never_passes_an_empty_denominator() {
        let empty = RouteCoverageSummary::default();
        assert_eq!(empty.missions, 0);
        assert!(!empty.covered(), "an empty installation is not a pass");

        let report = coverage_report(Path::new("/install"), &[]);
        assert!(report.contains("\"mission_count\":0"));
        assert!(report.contains("\"coverage\":false"));
        assert!(report.contains("\"route_encoding\":{\"state\":\"unmeasured\""));
        assert!(report.contains("\"carrier_member\":\"aiv.zrd\""));
        for mission_type in ["campaign", "instant_action", "multiplayer", "other"] {
            assert!(
                report.contains(&format!(
                    "\"type\":\"{mission_type}\",\"mission_count\":0,\"covered\":0"
                )),
                "every type row is present: {report}"
            );
        }

        let summary = RouteCoverageSummary {
            missions: 2,
            covered: 1,
            campaign: 1,
            instant_action: 1,
            multiplayer: 0,
            other: 0,
        };
        assert!(!summary.covered(), "one uncovered mission fails coverage");
        assert_eq!(summary.count(MissionType::Campaign), 1);
        assert_eq!(summary.count(MissionType::Multiplayer), 0);
    }
}
