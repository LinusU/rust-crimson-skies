//! The synthetic content-catalog fixture (F14-A) and the catalog, closure and
//! readiness inspection commands (F14-C).
//!
//! [`synthetic_catalog_fixture`] builds a small authored catalog through the
//! canonical [`Catalog`] constructor: ready and unsupported elements, a
//! ready launchable mission and an unsupported one, and a non-launchable
//! unsupported resource. Every row is `Origin::SyntheticFixture`, so the
//! fixture proves nothing about a retail installation and can never be
//! mistaken for a retail catalog entry (spec F14 AC04). It exists so tests
//! and the [`catalog_command`]/[`closure_command`] inspection commands
//! (F14-C) have a real, validated input without touching the owner's
//! installation at `$CS_GAME_DIR`.
//!
//! ```text
//! cs-inspect catalog [--out <file>]
//! cs-inspect closure --mission <catalog-id> [--strict] [--out <file>]
//! ```
//!
//! F14-C is the integration stage: it wires the catalog and closure
//! production paths in `cs_content::catalog` into the two `CLI-EVIDENCE`
//! consumers. [`catalog_report`] renders every catalog row with its parse,
//! normalize and readiness state and the baseline accounting, and
//! [`closure_run`] computes the transitive closure of one declared
//! mission/scenario root from the same rows, reporting the predecessor chain
//! of every reached node (including a resource deleted several edges deep),
//! the orphaned references, and — under `--strict` — failing when the
//! closure is not complete. Both reports name their source and are
//! `Origin::SyntheticFixture`, so a synthetic row is never presented as a
//! retail catalog entry; reading the owner's installation to build the
//! complete private baseline inventory is F14-D (required capability
//! `retail`).
//!
//! Naming, ids, digests and reasons here are newly authored fixture data,
//! not original content.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_content::catalog::Catalog;
use cs_content::catalog::closure::{Closure, ClosureError, CompatibilityOptions};
use cs_types::content::{
    CatalogElement, ConsumerKind, ContentId, ContentKind, Dependency, DependencyKind,
    NormalizeState, Origin, Provenance, Readiness, RuntimeConsumer, UnsupportedReason,
};
use cs_types::evidence::ClaimId;
use cs_types::install::ParseState;

/// A validated fixture claim id.
fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("synthetic fixture claim id is valid")
}

/// Designed provenance for one fixture claim.
fn designed(id: &str) -> Provenance {
    Provenance::designed(claim(id))
}

/// A validated synthetic element id.
fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("synthetic fixture id is valid")
}

/// A ready synthetic element of `id`'s kind, depending on `deps`.
fn ready_element(element_id: ContentId, display: &str, deps: &[&ContentId]) -> CatalogElement {
    CatalogElement {
        kind: element_id.kind(),
        id: element_id,
        display_name: Some(display.to_owned()),
        origin: Origin::SyntheticFixture,
        dependencies: deps
            .iter()
            .map(|target| Dependency {
                target: (*target).clone(),
                kind: DependencyKind::Static,
                provenance: designed("f14.synthetic.dependency"),
            })
            .collect(),
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: designed("f14.synthetic.consumer"),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
    }
}

/// Builds the minimal synthetic content catalog.
///
/// The catalog holds a ready launchable mission and an unsupported
/// launchable mission, so its `unsupported_count` is one and it is not
/// fully ready; the unsupported non-launchable resource stays visible
/// beside them (collections cannot exclude failed entries). Every row is a
/// synthetic fixture.
pub fn synthetic_catalog_fixture() -> Catalog {
    let mut catalog = Catalog::new();

    let world = id(ContentKind::World, "c1");
    let airframe = id(ContentKind::Airframe, "scout");
    let image = id(ContentKind::Image, "scout_body");
    let sound = id(ContentKind::Sound, "engine_loop");
    let ready_mission = id(ContentKind::Mission, "m01");
    let unsupported_mission = id(ContentKind::Mission, "m02");
    let missing_texture = id(ContentKind::Image, "missing_texture");

    catalog
        .insert(ready_element(world.clone(), "Synthetic World C1", &[]))
        .expect("synthetic world inserts");
    catalog
        .insert(ready_element(
            airframe.clone(),
            "Synthetic Scout",
            &[&image],
        ))
        .expect("synthetic airframe inserts");
    catalog
        .insert(ready_element(image.clone(), "Synthetic Scout Body", &[]))
        .expect("synthetic image inserts");
    catalog
        .insert(ready_element(sound.clone(), "Synthetic Engine Loop", &[]))
        .expect("synthetic sound inserts");
    catalog
        .insert(ready_element(
            ready_mission.clone(),
            "Synthetic Mission M01",
            &[&world, &airframe],
        ))
        .expect("synthetic ready mission inserts");

    let mut unsupported = ready_element(
        unsupported_mission.clone(),
        "Synthetic Mission M02",
        &[&world],
    );
    unsupported.readiness = Readiness::Unavailable;
    unsupported.parse_state = ParseState::Failed {
        diagnostic: "synthetic mission has no parser in this workspace stage".to_owned(),
    };
    unsupported.unsupported_reasons = vec![UnsupportedReason::ParseFailed {
        diagnostic: "synthetic mission has no parser in this workspace stage".to_owned(),
    }];
    catalog
        .insert(unsupported)
        .expect("synthetic unsupported mission inserts");

    let mut missing = ready_element(missing_texture, "Synthetic Missing Texture", &[]);
    missing.readiness = Readiness::Unavailable;
    missing.unsupported_reasons = vec![UnsupportedReason::MissingParser];
    catalog
        .insert(missing)
        .expect("synthetic unsupported image inserts");

    catalog
        .declare_launchable(&ready_mission)
        .expect("the ready mission is launchable");
    catalog
        .declare_launchable(&unsupported_mission)
        .expect("the unsupported mission is still launchable and counted");

    catalog
}

/// The report format version of the `catalog` command.
pub const CATALOG_REPORT_VERSION: &str = "cs-inspect-catalog/1";

/// The report format version of the `closure` command.
pub const CLOSURE_REPORT_VERSION: &str = "cs-inspect-closure/1";

/// The source label both commands report: the validated synthetic fixture.
///
/// Reading the owner's installation to build the retail catalog is F14-D;
/// naming the source keeps a synthetic row from being read as retail
/// (spec F14 AC04).
pub const SYNTHETIC_SOURCE_LABEL: &str = "synthetic-fixture";

/// The authored synthetic catalog the inspection commands consume.
///
/// It is the F14-A [`synthetic_catalog_fixture`] with one deep ready branch
/// added: `mission/m01` reaches `airframe/scout`, which reaches
/// `material/scout_skin`, which reaches `image/scout_paint` several edges
/// down. Every row is `Origin::SyntheticFixture`, and the baseline still
/// holds one ready and one unsupported launchable mission, so the commands
/// can demonstrate both a complete and an incomplete closure without the
/// owner's installation.
pub fn inspection_catalog_fixture() -> Catalog {
    let mut catalog = Catalog::new();

    let world = id(ContentKind::World, "c1");
    let hull = id(ContentKind::Mesh, "scout_hull");
    let body = id(ContentKind::Image, "scout_body");
    let paint = id(ContentKind::Image, "scout_paint");
    let skin = id(ContentKind::Material, "scout_skin");
    let airframe = id(ContentKind::Airframe, "scout");
    let sound = id(ContentKind::Sound, "engine_loop");
    let ready_mission = id(ContentKind::Mission, "m01");
    let unsupported_mission = id(ContentKind::Mission, "m02");

    catalog
        .insert(ready_element(world.clone(), "Synthetic World C1", &[]))
        .expect("synthetic world inserts");
    catalog
        .insert(ready_element(hull.clone(), "Synthetic Scout Hull", &[]))
        .expect("synthetic hull inserts");
    catalog
        .insert(ready_element(body.clone(), "Synthetic Scout Body", &[]))
        .expect("synthetic body inserts");
    catalog
        .insert(ready_element(paint.clone(), "Synthetic Scout Paint", &[]))
        .expect("synthetic paint inserts");
    catalog
        .insert(ready_element(
            skin.clone(),
            "Synthetic Scout Skin",
            &[&paint],
        ))
        .expect("synthetic skin inserts");
    catalog
        .insert(ready_element(
            airframe.clone(),
            "Synthetic Scout",
            &[&hull, &skin, &body],
        ))
        .expect("synthetic airframe inserts");
    catalog
        .insert(ready_element(sound.clone(), "Synthetic Engine Loop", &[]))
        .expect("synthetic sound inserts");
    catalog
        .insert(ready_element(
            ready_mission.clone(),
            "Synthetic Mission M01",
            &[&world, &airframe, &sound],
        ))
        .expect("synthetic ready mission inserts");

    let mut unsupported = ready_element(
        unsupported_mission.clone(),
        "Synthetic Mission M02",
        &[&world],
    );
    unsupported.readiness = Readiness::Unavailable;
    unsupported.parse_state = ParseState::Failed {
        diagnostic: "synthetic mission has no parser in this workspace stage".to_owned(),
    };
    unsupported.unsupported_reasons = vec![UnsupportedReason::ParseFailed {
        diagnostic: "synthetic mission has no parser in this workspace stage".to_owned(),
    }];
    catalog
        .insert(unsupported)
        .expect("synthetic unsupported mission inserts");

    catalog
        .declare_launchable(&ready_mission)
        .expect("the ready mission is launchable");
    catalog
        .declare_launchable(&unsupported_mission)
        .expect("the unsupported mission is still launchable and counted");

    catalog
}

/// The readiness accounting one `catalog` run reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSummary {
    /// Every row, including the failed ones.
    pub rows: usize,
    /// Rows whose own readiness is `Ready`.
    pub ready: usize,
    /// Rows whose own readiness is `Unavailable`.
    pub unavailable: usize,
    /// Declared launchable missions/scenarios (the readiness denominator).
    pub launchable: usize,
    /// Declared launchable rows that are not ready.
    pub unsupported_launchable: usize,
    /// Declared launchable rows with an installation origin.
    pub original_launchable: usize,
    /// Declared launchable rows that are not installation data.
    pub synthetic_launchable: usize,
    /// Whether every declared launchable row is ready.
    pub is_fully_ready: bool,
    /// Whether every declared launchable row is original and ready.
    pub is_retail_ready: bool,
    /// Whether any row claims an installation origin.
    pub has_original_rows: bool,
}

/// The counts of one `catalog` run, read from the production catalog API.
pub fn catalog_summary(catalog: &Catalog) -> CatalogSummary {
    let ready = catalog
        .elements()
        .filter(|element| element.is_ready())
        .count();
    CatalogSummary {
        rows: catalog.len(),
        ready,
        unavailable: catalog.len() - ready,
        launchable: catalog.launchable_count(),
        unsupported_launchable: catalog.unsupported_count(),
        original_launchable: catalog.original_launchable_count(),
        synthetic_launchable: catalog.synthetic_launchable_count(),
        is_fully_ready: catalog.is_fully_ready(),
        is_retail_ready: catalog.is_retail_ready(),
        has_original_rows: catalog
            .elements()
            .any(|element| element.origin.is_original()),
    }
}

/// Renders the canonical JSON catalog report of `catalog`.
///
/// Every row stays in the report, including the failed and unavailable ones
/// (`IDENTITY-CONTENT`: "Collections cannot exclude failed entries"), and
/// every array is in canonical id order, so the report is byte-stable for
/// the same rows in any insertion order.
pub fn catalog_report(catalog: &Catalog, source: &str) -> String {
    let summary = catalog_summary(catalog);
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"schema\":{},\"source\":{},\"retail\":{},\"rows\":{},\"ready\":{},\
         \"unavailable\":{},\"launchable\":{},\"unsupported_launchable\":{},\
         \"original_launchable\":{},\"synthetic_launchable\":{},\"is_fully_ready\":{},\
         \"is_retail_ready\":{},\"elements\":[",
        json_string(CATALOG_REPORT_VERSION),
        json_string(source),
        summary.has_original_rows,
        summary.rows,
        summary.ready,
        summary.unavailable,
        summary.launchable,
        summary.unsupported_launchable,
        summary.original_launchable,
        summary.synthetic_launchable,
        summary.is_fully_ready,
        summary.is_retail_ready,
    );
    for (index, element) in catalog.elements().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&element_json(element));
    }
    out.push_str("]}");
    out
}

/// Renders one catalog row.
fn element_json(element: &CatalogElement) -> String {
    let consumers: Vec<String> = element
        .runtime_consumers
        .iter()
        .map(|consumer| json_string(consumer.kind.label()))
        .collect();
    let mut dependencies: Vec<String> = element
        .dependencies
        .iter()
        .map(|dependency| {
            format!(
                "{{\"target\":{},\"kind\":{}}}",
                json_string(dependency.target.as_str()),
                json_string(dependency.kind.label())
            )
        })
        .collect();
    dependencies.sort();
    let reasons: Vec<String> = element
        .unsupported_reasons
        .iter()
        .map(|reason| {
            format!(
                "{{\"code\":{},\"detail\":{}}}",
                json_string(reason.code()),
                match reason.detail() {
                    Some(detail) => json_string(detail),
                    None => "null".to_owned(),
                }
            )
        })
        .collect();
    format!(
        "{{\"id\":{},\"kind\":{},\"display_name\":{},\"origin\":{},\"parse_state\":{},\
         \"normalize_state\":{},\"consumers\":[{}],\"dependencies\":[{}],\"readiness\":{},\
         \"reasons\":[{}],\"fingerprint\":{}}}",
        json_string(element.id.as_str()),
        json_string(element.kind.label()),
        match &element.display_name {
            Some(name) => json_string(name),
            None => "null".to_owned(),
        },
        json_string(element.origin.label()),
        json_string(parse_state_label(&element.parse_state)),
        json_string(normalize_state_label(&element.normalize_state)),
        consumers.join(","),
        dependencies.join(","),
        json_string(element.readiness.label()),
        reasons.join(","),
        match &element.fingerprint {
            Some(fingerprint) => json_string(&fingerprint.sha256.to_hex()),
            None => "null".to_owned(),
        },
    )
}

/// The stable label of a parse state.
fn parse_state_label(state: &ParseState) -> &'static str {
    match state {
        ParseState::Unparsed => "unparsed",
        ParseState::Parsed => "parsed",
        ParseState::Failed { .. } => "failed",
    }
}

/// The stable label of a normalize state.
fn normalize_state_label(state: &NormalizeState) -> &'static str {
    match state {
        NormalizeState::NotNormalized => "not_normalized",
        NormalizeState::Normalized => "normalized",
        NormalizeState::Failed { .. } => "failed",
    }
}

/// The closure counts one `closure` run reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosureSummary {
    /// The declared roots.
    pub roots: usize,
    /// The reached nodes.
    pub reached: usize,
    /// The reached nodes whose closure readiness is true.
    pub ready: usize,
    /// The reached nodes that are not ready.
    pub unavailable: usize,
    /// The explicit orphaned references.
    pub unresolved: usize,
    /// Whether every root is ready and no reference is orphaned.
    pub complete: bool,
}

/// Everything one `closure` run produced.
#[derive(Debug)]
pub struct ClosureRun {
    /// The `CLI-EVIDENCE` exit code.
    pub exit_code: u8,
    /// The JSON report, when the closure computed.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
    /// The counts, when the closure computed.
    pub summary: Option<ClosureSummary>,
}

impl ClosureRun {
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

/// Computes the closure of `roots` over `catalog` and renders its report.
///
/// This is the production body both the CLI and the acceptance tests drive:
/// an unknown or non-launchable root is invalid content (exit 2), an
/// ownership cycle is a failed validation (exit 3), and a computed closure
/// exits 3 only under `strict` when it is not complete.
pub fn closure_run(
    catalog: &Catalog,
    roots: &[ContentId],
    strict: bool,
    source: &str,
) -> ClosureRun {
    let closure = match Closure::compute(catalog, roots, CompatibilityOptions::default()) {
        Ok(closure) => closure,
        Err(error) => {
            let exit_code = match error {
                ClosureError::UnknownRoot { .. } | ClosureError::NotLaunchableRoot { .. } => 2,
                ClosureError::OwnershipCycle { .. } => 3,
            };
            return ClosureRun::failed(exit_code, format!("cs-inspect closure: {error}"));
        }
    };

    let reached = closure.node_ids();
    let summary = ClosureSummary {
        roots: closure.roots().len(),
        reached: reached.len(),
        ready: reached.iter().filter(|id| closure.is_ready(id)).count(),
        unavailable: closure.unavailable().len(),
        unresolved: closure.unresolved().len(),
        complete: closure.is_complete(),
    };
    let report = closure_report_json(&closure, strict, source);
    let exit_code = if strict && !summary.complete { 3 } else { 0 };
    ClosureRun {
        exit_code,
        report: Some(report),
        out: None,
        diagnostics: Vec::new(),
        summary: Some(summary),
    }
}

/// Renders the canonical JSON closure report.
///
/// The nested `closure` object is the production `Closure::to_json` payload,
/// so this consumer cannot drift from the F14-B report. `unresolved_chains`
/// adds the consumer's own integration: for every orphaned reference it
/// renders the full predecessor chain from the root, so a texture deleted
/// several edges deep is reported as its complete `mission -> … -> texture`
/// path even though the orphan has no node row of its own.
fn closure_report_json(closure: &Closure, strict: bool, source: &str) -> String {
    let mut chains: Vec<String> = closure
        .unresolved()
        .iter()
        .map(|reference| {
            let chain = closure
                .chain_to(&reference.target)
                .unwrap_or_default()
                .iter()
                .map(|id| json_string(id.as_str()))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"from\":{},\"target\":{},\"chain\":[{}]}}",
                json_string(reference.from.as_str()),
                json_string(reference.target.as_str()),
                chain,
            )
        })
        .collect();
    chains.sort();
    format!(
        "{{\"schema\":{},\"source\":{},\"strict\":{},\"complete\":{},\
         \"unresolved_chains\":[{}],\"closure\":{}}}",
        json_string(CLOSURE_REPORT_VERSION),
        json_string(source),
        strict,
        closure.is_complete(),
        chains.join(","),
        closure.to_json(),
    )
}

/// Parsed `catalog` arguments.
#[derive(Debug, Default)]
struct CatalogArgs {
    out: Option<PathBuf>,
}

/// Parsed `closure` arguments.
#[derive(Debug, Default)]
struct ClosureArgs {
    mission: Option<String>,
    strict: bool,
    out: Option<PathBuf>,
}

fn parse_catalog_args(args: &[String]) -> Result<CatalogArgs, String> {
    let mut parsed = CatalogArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        match arg.as_str() {
            "--out" => {
                let Some(value) = cursor.next() else {
                    return Err("cs-inspect catalog: --out needs a value".to_owned());
                };
                parsed.out = Some(PathBuf::from(value));
            }
            other => {
                return Err(format!(
                    "cs-inspect catalog: unsupported argument {other:?}; expected --out <file>"
                ));
            }
        }
    }
    Ok(parsed)
}

fn parse_closure_args(args: &[String]) -> Result<ClosureArgs, String> {
    let mut parsed = ClosureArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        match arg.as_str() {
            "--strict" => parsed.strict = true,
            flag @ ("--mission" | "--out") => {
                let Some(value) = cursor.next() else {
                    return Err(format!("cs-inspect closure: {flag} needs a value"));
                };
                if flag == "--mission" {
                    parsed.mission = Some(value.clone());
                } else {
                    parsed.out = Some(PathBuf::from(value));
                }
            }
            other => {
                return Err(format!(
                    "cs-inspect closure: unsupported argument {other:?}; expected --mission \
                     <catalog-id>, --strict, --out <file>"
                ));
            }
        }
    }
    Ok(parsed)
}

/// Everything one `catalog` run produced.
#[derive(Debug)]
pub struct CatalogRun {
    /// The `CLI-EVIDENCE` exit code.
    pub exit_code: u8,
    /// The JSON report, when the command ran.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
    /// The counts, when the command ran.
    pub summary: Option<CatalogSummary>,
}

impl CatalogRun {
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

/// Runs the `catalog` command and returns its exit code.
pub fn catalog_command(args: &[String]) -> ExitCode {
    let run = catalog_command_result(args);
    report_run(
        "catalog",
        &run.diagnostics,
        run.report.as_deref(),
        run.out.as_deref(),
    );
    ExitCode::from(run.exit_code)
}

/// The body of [`catalog_command`].
pub fn catalog_command_result(args: &[String]) -> CatalogRun {
    let parsed = match parse_catalog_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return CatalogRun::failed(2, message),
    };
    let catalog = inspection_catalog_fixture();
    let summary = catalog_summary(&catalog);
    let report = catalog_report(&catalog, SYNTHETIC_SOURCE_LABEL);
    let out = match &parsed.out {
        Some(out) => match write_atomic(out, &report) {
            Ok(()) => Some(out.clone()),
            Err(error) => {
                return CatalogRun {
                    exit_code: 1,
                    report: Some(report),
                    out: None,
                    diagnostics: vec![format!(
                        "cs-inspect catalog: cannot write report to {}: {error}",
                        out.display()
                    )],
                    summary: Some(summary),
                };
            }
        },
        None => None,
    };
    CatalogRun {
        exit_code: 0,
        report: Some(report),
        out,
        diagnostics: Vec::new(),
        summary: Some(summary),
    }
}

/// Runs the `closure` command and returns its exit code.
pub fn closure_command(args: &[String]) -> ExitCode {
    let run = closure_command_result(args);
    report_run(
        "closure",
        &run.diagnostics,
        run.report.as_deref(),
        run.out.as_deref(),
    );
    ExitCode::from(run.exit_code)
}

/// The body of [`closure_command`].
pub fn closure_command_result(args: &[String]) -> ClosureRun {
    let parsed = match parse_closure_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return ClosureRun::failed(2, message),
    };
    let Some(mission) = &parsed.mission else {
        return ClosureRun::failed(
            2,
            "cs-inspect closure: --mission <catalog-id> is required".to_owned(),
        );
    };
    let root = match ContentId::parse(mission) {
        Ok(id) => id,
        Err(error) => {
            return ClosureRun::failed(
                2,
                format!("cs-inspect closure: --mission {mission:?} is not a catalog id: {error}"),
            );
        }
    };
    let catalog = inspection_catalog_fixture();
    let mut run = closure_run(
        &catalog,
        std::slice::from_ref(&root),
        parsed.strict,
        SYNTHETIC_SOURCE_LABEL,
    );
    if let (Some(report), Some(out)) = (run.report.as_deref(), &parsed.out) {
        match write_atomic(out, report) {
            Ok(()) => run.out = Some(out.clone()),
            Err(error) => {
                run.exit_code = 1;
                run.out = None;
                run.diagnostics.push(format!(
                    "cs-inspect closure: cannot write report to {}: {error}",
                    out.display()
                ));
            }
        }
    }
    run
}

/// Prints diagnostics to stderr and the report to stdout when it was not
/// written to `--out`.
fn report_run(command: &str, diagnostics: &[String], report: Option<&str>, out: Option<&Path>) {
    for line in diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (report, out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote {command} report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
}

/// Writes `report` to `out` atomically via a sibling temporary file.
fn write_atomic(out: &Path, report: &str) -> io::Result<()> {
    let mut temp_name = out.as_os_str().to_owned();
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp_name);
    let result = fs::write(&temp, report).and_then(|()| fs::rename(&temp, out));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// A JSON string literal.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC01 through the fixture: the unsupported launchable mission raises
    /// the unsupported count and prevents full readiness, and every row —
    /// including the failed ones — stays in the collection.
    #[test]
    fn accept_f14_a_fixture_keeps_unsupported_rows_and_counts_them() {
        let catalog = synthetic_catalog_fixture();

        assert_eq!(catalog.len(), 7, "every fixture row stays in the catalog");
        assert_eq!(catalog.launchable_count(), 2);
        assert_eq!(
            catalog.unsupported_count(),
            1,
            "the unsupported launchable mission is counted, never dropped"
        );
        assert!(!catalog.is_fully_ready());

        let unsupported = catalog.unsupported_launchables();
        assert_eq!(unsupported.len(), 1);
        assert_eq!(unsupported[0].id.as_str(), "mission/m02");
        assert_eq!(unsupported[0].unsupported_codes(), vec!["parse_failed"]);

        let failed = catalog
            .elements()
            .filter(|element| !element.is_ready())
            .collect::<Vec<_>>();
        assert_eq!(
            failed.len(),
            2,
            "the unsupported non-launchable resource stays visible too"
        );
    }

    /// AC04 through the fixture: every launchable row is a synthetic
    /// fixture, so none can be mistaken for a retail catalog entry.
    #[test]
    fn accept_f14_a_fixture_is_synthetic_and_never_retail() {
        let catalog = synthetic_catalog_fixture();

        assert_eq!(catalog.original_launchable_count(), 0);
        assert_eq!(catalog.synthetic_launchable_count(), 2);
        assert!(
            !catalog.is_retail_ready(),
            "a synthetic catalog is never retail-ready"
        );
        assert!(
            catalog
                .elements()
                .all(|element| !element.origin.is_original()),
            "no fixture row claims an installation origin"
        );
    }

    /// Builds the deep chain `mission/m01 -> airframe/scout ->
    /// material/scout_skin -> image/scout_paint`, inserting the texture row
    /// only when `include_texture` is true. With it false the texture is
    /// *deleted*: the material still references it, so the closure must
    /// report the orphan and keep the mission unavailable.
    fn deep_chain_catalog(include_texture: bool) -> Catalog {
        let world = id(ContentKind::World, "c1");
        let paint = id(ContentKind::Image, "scout_paint");
        let skin = id(ContentKind::Material, "scout_skin");
        let airframe = id(ContentKind::Airframe, "scout");
        let mission = id(ContentKind::Mission, "m01");

        let mut catalog = Catalog::new();
        catalog
            .insert(ready_element(world, "Synthetic World C1", &[]))
            .expect("world inserts");
        if include_texture {
            catalog
                .insert(ready_element(paint.clone(), "Synthetic Scout Paint", &[]))
                .expect("texture inserts");
        }
        catalog
            .insert(ready_element(
                skin.clone(),
                "Synthetic Scout Skin",
                &[&paint],
            ))
            .expect("material inserts");
        catalog
            .insert(ready_element(airframe.clone(), "Synthetic Scout", &[&skin]))
            .expect("airframe inserts");
        catalog
            .insert(ready_element(
                mission.clone(),
                "Synthetic Mission M01",
                &[&airframe],
            ))
            .expect("mission inserts");
        catalog
            .declare_launchable(&mission)
            .expect("the mission is launchable");
        catalog
    }

    /// A disposable output directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "cs-f14-c-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("the fixture directory is created");
            Self(root)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// The `catalog` command reports every row with its readiness and names
    /// the source, and a synthetic row is never presented as retail.
    #[test]
    fn accept_f14_c_catalog_command_reports_readiness_and_synthetic_origin() {
        let run = catalog_command_result(&[]);
        assert_eq!(run.exit_code, 0);
        let summary = run.summary.expect("the command ran");
        assert_eq!(summary.rows, 9, "every fixture row stays in the catalog");
        assert_eq!(summary.ready, 8);
        assert_eq!(summary.unavailable, 1);
        assert_eq!(summary.launchable, 2);
        assert_eq!(
            summary.unsupported_launchable, 1,
            "the unsupported launchable mission is counted, never dropped"
        );
        assert_eq!(summary.original_launchable, 0);
        assert_eq!(summary.synthetic_launchable, 2);
        assert!(!summary.is_fully_ready);
        assert!(
            !summary.is_retail_ready,
            "a synthetic catalog is never retail-ready"
        );

        let report = run.report.expect("the command reports");
        assert!(report.contains(&format!("\"schema\":\"{CATALOG_REPORT_VERSION}\"")));
        assert!(report.contains("\"source\":\"synthetic-fixture\""));
        assert!(report.contains("\"retail\":false"));
        assert!(
            report.contains("\"origin\":\"synthetic_fixture\""),
            "every row names its synthetic origin"
        );
        assert!(
            !report.contains("\"origin\":\"installation\""),
            "no row claims an installation origin"
        );
        assert!(
            report.contains("\"id\":\"image/scout_paint\""),
            "the deep texture row is in the report"
        );
        assert!(
            report.contains("\"id\":\"mission/m02\"")
                && report.contains("\"readiness\":\"unavailable\""),
            "the unsupported launchable mission stays visible and unavailable"
        );
    }

    /// The `catalog` command writes `--out` atomically and is byte-stable for
    /// the same rows; a bad flag or a missing value is invalid input.
    #[test]
    fn accept_f14_c_catalog_command_writes_out_and_is_byte_stable() {
        let temp = TempDir::new("catalog-out");
        let path = temp.0.join("catalog.json");
        let first = catalog_command_result(&args(&["--out", path.to_str().expect("UTF-8")]));
        assert_eq!(first.exit_code, 0);
        assert_eq!(first.out.as_deref(), Some(path.as_path()));
        let written = fs::read_to_string(&path).expect("the report is written");
        assert_eq!(Some(&written), first.report.as_ref());

        let second = catalog_command_result(&[]);
        assert_eq!(
            written,
            second.report.expect("the report is stable"),
            "the same rows serialize byte-for-byte identically"
        );

        assert_eq!(catalog_command_result(&args(&["--out"])).exit_code, 2);
        assert_eq!(
            catalog_command_result(&args(&["--nope"])).exit_code,
            2,
            "an unknown flag is invalid input"
        );
    }

    /// AC03 minimum scenario: a texture several edges deep is deleted, and
    /// the closure reports the whole `mission -> texture` chain while the
    /// mission stays unavailable. Removing the orphan detection or the chain
    /// recording makes this test fail.
    #[test]
    fn accept_f14_c_closure_command_reports_the_deep_mission_to_texture_chain() {
        let catalog = deep_chain_catalog(false);
        let mission = id(ContentKind::Mission, "m01");
        let texture = id(ContentKind::Image, "scout_paint");

        let strict = closure_run(
            &catalog,
            std::slice::from_ref(&mission),
            true,
            SYNTHETIC_SOURCE_LABEL,
        );
        assert_eq!(
            strict.exit_code, 3,
            "a deleted deep texture makes the closure incomplete under --strict"
        );
        let summary = strict.summary.expect("the closure computed");
        assert_eq!(summary.unresolved, 1, "the orphan is explicit");
        assert_eq!(summary.ready, 0);
        assert!(!summary.complete);

        let report = strict.report.expect("the closure reports");
        assert!(
            report.contains(
                "\"chain\":[\"mission/m01\",\"airframe/scout\",\"material/scout_skin\",\
                 \"image/scout_paint\"]"
            ),
            "the mission-to-texture chain is reported in full: {report}"
        );
        assert!(
            report.contains("\"target\":\"image/scout_paint\""),
            "the deleted texture is named as an orphaned reference"
        );

        // Without --strict the same incomplete closure computes and exits 0,
        // so the closure data is still the evidence of the failure.
        let relaxed = closure_run(&catalog, &[mission], false, SYNTHETIC_SOURCE_LABEL);
        assert_eq!(relaxed.exit_code, 0);
        let relaxed_report = relaxed.report.expect("the closure reports");
        assert!(relaxed_report.contains("\"strict\":false"));
        assert!(
            relaxed_report.contains(
                "\"chain\":[\"mission/m01\",\"airframe/scout\",\"material/scout_skin\",\
                 \"image/scout_paint\"]"
            ),
            "the same incomplete chain is reported without --strict"
        );

        // The same chain is complete when the texture row is present, so the
        // failure above is the deletion alone.
        let complete = closure_run(
            &deep_chain_catalog(true),
            &[id(ContentKind::Mission, "m01")],
            true,
            SYNTHETIC_SOURCE_LABEL,
        );
        assert_eq!(complete.exit_code, 0);
        assert!(complete.summary.expect("computed").complete);
        assert!(
            complete
                .report
                .expect("reports")
                .contains(&format!("\"id\":\"{}\"", texture.as_str()))
        );
    }

    /// The `closure` command wired to the fixture: a reachable all-ready
    /// mission is a complete strict closure, and the unsupported mission is
    /// refused under `--strict`.
    #[test]
    fn accept_f14_c_closure_command_is_strict_over_the_fixture() {
        let complete = closure_command_result(&args(&["--mission", "mission/m01", "--strict"]));
        assert_eq!(complete.exit_code, 0);
        assert!(complete.summary.expect("computed").complete);

        let incomplete = closure_command_result(&args(&["--mission", "mission/m02", "--strict"]));
        assert_eq!(
            incomplete.exit_code, 3,
            "the unsupported mission fails a strict closure"
        );
        assert!(!incomplete.summary.expect("computed").complete);
        assert_eq!(incomplete.out, None);
    }

    /// The `closure` command refuses a missing, unknown, malformed or
    /// non-launchable root as invalid content, and never reports success.
    #[test]
    fn accept_f14_c_closure_command_refuses_unknown_and_non_launchable_roots() {
        for argv in [
            vec!["--strict"],                            // missing --mission
            vec!["--mission", "mission/m99"],            // unknown root
            vec!["--mission", "image/scout_paint"],      // non-launchable root
            vec!["--mission", "m01"],                    // malformed id
            vec!["--mission", "mission/m01", "--bogus"], // unknown flag
            vec!["--mission"],                           // missing value
        ] {
            let run = closure_command_result(&args(&argv));
            assert_eq!(run.exit_code, 2, "{argv:?} is invalid input");
            assert!(run.report.is_none(), "{argv:?} reports nothing");
            assert!(run.summary.is_none());
            assert!(!run.diagnostics.is_empty(), "{argv:?} explains the refusal");
        }
    }

    /// The `closure` command writes `--out` atomically.
    #[test]
    fn accept_f14_c_closure_command_writes_out() {
        let temp = TempDir::new("closure-out");
        let path = temp.0.join("closure.json");
        let run = closure_command_result(&args(&[
            "--mission",
            "mission/m01",
            "--out",
            path.to_str().expect("UTF-8"),
        ]));
        assert_eq!(run.exit_code, 0);
        assert_eq!(run.out.as_deref(), Some(path.as_path()));
        let written = fs::read_to_string(&path).expect("the report is written");
        assert_eq!(Some(&written), run.report.as_ref());
        assert!(written.contains(&format!("\"schema\":\"{CLOSURE_REPORT_VERSION}\"")));
        assert!(written.contains("\"complete\":true"));
    }
}
