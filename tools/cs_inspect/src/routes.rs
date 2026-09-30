//! The `routes` command: expose the declared route graph contract as a
//! read-only inspection report (F31-A).
//!
//! ```text
//! cs-inspect routes [--out <file>]
//! ```
//!
//! The report renders the provenance-carrying declared route record
//! [`cs_content::routes::declared_synthetic_arch_route`] — the authored id,
//! origin, reference frame, termination, clearance and every node with its
//! stable id, authored sequence, mandatory flag, resolved position and
//! resolved trigger volume — plus the declared edges. It is the content half
//! of the F31 route contract; the typed runtime consumer
//! (`cs_sim::ai::navigation`) is not linked into `cs-inspect`, so this command
//! makes no claim about follow behavior.
//!
//! Every value the record carries is newly authored project design
//! ([`Origin::SyntheticFixture`]); the report names its synthetic source and
//! is never retail-ready, so a synthetic route can never be mistaken for an
//! original mission route. Unknown positions, trigger volumes and clearances
//! are reported as explicit unknowns with their claim id and reason, never as
//! a silent zero.
//!
//! ```text
//! exit 0  the report was produced
//! exit 2  invalid input (unknown flag, missing value)
//! exit 1  a runtime failure writing --out
//! ```

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use cs_content::routes::{
    AnchorKind, ReferenceFrame, RouteDefinition, RouteNode, RouteTermination, TriggerShape,
    TriggerVolume, declared_synthetic_arch_route,
};
use cs_types::content::{Provenance, Resolved};

use crate::catalog::{json_string, report_run, write_atomic};

/// The report schema version this consumer writes.
pub const ROUTES_REPORT_VERSION: &str = "cs-inspect-routes/v1";

/// The `source` label of the declared synthetic route report; it can never be
/// `installation`.
pub const SYNTHETIC_ROUTES_SOURCE_LABEL: &str = "synthetic-fixture";

/// Parsed `routes` arguments.
#[derive(Debug, Default)]
struct RoutesArgs {
    /// The `--out` report path; `None` writes the report to stdout.
    out: Option<PathBuf>,
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
    /// The counts the report declares, when it was produced.
    pub summary: Option<RoutesSummary>,
}

/// The counts one `routes` report declares.
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

impl RoutesRun {
    fn failed(exit_code: u8, message: String) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![message],
            summary: None,
        }
    }
}

fn parse_routes_args(args: &[String]) -> Result<RoutesArgs, String> {
    let mut parsed = RoutesArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        if flag != "--out" {
            return Err(format!(
                "cs-inspect routes: unsupported argument {flag:?}; expected --out <file>"
            ));
        }
        let Some(value) = cursor.next() else {
            return Err(format!("cs-inspect routes: {flag} needs a value"));
        };
        parsed.out = Some(PathBuf::from(value));
    }
    Ok(parsed)
}

/// Runs the `routes` command and returns its exit code.
///
/// `--out` is written atomically and its final path is reported on stderr;
/// without `--out` the JSON report goes to stdout. A failure is never returned
/// as success.
pub fn routes_command(args: &[String]) -> ExitCode {
    let run = routes_command_result(args);
    report_run(
        "routes",
        &run.diagnostics,
        run.report.as_deref(),
        run.out.as_deref(),
    );
    ExitCode::from(run.exit_code)
}

/// The body of [`routes_command`], separate so a failure carries its named
/// exit code and the report can be inspected without touching stdout.
pub fn routes_command_result(args: &[String]) -> RoutesRun {
    let parsed = match parse_routes_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return RoutesRun::failed(2, message),
    };

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
            },
        },
        None => RoutesRun {
            exit_code: 0,
            report: Some(report),
            out: None,
            diagnostics: Vec::new(),
            summary: Some(summary),
        },
    }
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
        "{{\"id\":{},\"sequence\":{},\"mandatory\":{},\"position_m\":{},\"trigger\":{}}}",
        json_string(node.id.as_str()),
        node.sequence,
        node.mandatory,
        render_resolved_position(&node.position_m),
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
}
