//! The `handling` command: expose the declared handling probe and
//! reference-envelope schema as a read-only inspection report (F26-A).
//!
//! ```text
//! cs-inspect handling [--out <file>]
//! ```
//!
//! The report renders the production schema of [`cs_sim::probes`] — the probe
//! vocabulary, the declared [`ReferenceEnvelope`] with its recorded input,
//! initial state, difficulty, loadout, timing uncertainty, units, tolerance and
//! held-out entry — and the two comparison outcomes AC01 turns on. It is the
//! schema-level counterpart of the `routes` command: a real consumer of the
//! production types, not a second implementation.
//!
//! Every value comes from the declared synthetic fixture
//! ([`Origin::SyntheticFixture`]), so the report names its synthetic source and
//! is never retail-ready. It makes **no** original-fidelity claim: no original
//! installation is read, no probe is flown, and the assessment's
//! `supports_original_fidelity` is false by construction. Running headless
//! probes against real reference traces is F26-B/F26-D.
//!
//! ```text
//! exit 0  the report was produced
//! exit 2  invalid input (unknown flag, missing value)
//! exit 1  a runtime failure building the report or writing --out
//! ```

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use cs_sim::probes::{
    EnvelopeEntry, HandlingAssessment, ProbeInitialState, ProbeInputStep, ProbeKind,
    ReferenceEnvelope, TimingUncertainty, VerdictStatus, synthetic_covering_assessment,
    synthetic_out_of_envelope_assessment, synthetic_reference_envelope,
};

use crate::catalog::{json_string, report_run, write_atomic};

/// The report schema version this consumer writes.
pub const HANDLING_REPORT_VERSION: &str = "cs-inspect-handling/v1";

/// The `source` label of the declared synthetic handling report; it can never
/// be `installation`.
pub const SYNTHETIC_HANDLING_SOURCE_LABEL: &str = "synthetic-fixture";

/// Parsed `handling` arguments.
#[derive(Debug, Default)]
struct HandlingArgs {
    /// The `--out` report path; `None` writes the report to stdout.
    out: Option<PathBuf>,
}

/// Everything one `handling` run produced.
#[derive(Debug)]
pub struct HandlingRun {
    /// The `CLI-EVIDENCE` exit code.
    pub exit_code: u8,
    /// The JSON report, when it was produced.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
    /// The counts the report declares, when it was produced.
    pub summary: Option<HandlingSummary>,
}

/// The counts one `handling` report declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandlingSummary {
    /// How many maneuvers the probe vocabulary declares.
    pub probe_kinds: usize,
    /// How many entries the envelope declares.
    pub entries: usize,
    /// How many entries are held out of the fit.
    pub held_out: usize,
    /// Whether the covering candidate passed.
    pub covering_passes: bool,
    /// Whether the out-of-envelope candidate passed (it must not).
    pub out_of_envelope_passes: bool,
}

impl HandlingRun {
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

fn parse_handling_args(args: &[String]) -> Result<HandlingArgs, String> {
    let mut parsed = HandlingArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        if flag != "--out" {
            return Err(format!(
                "cs-inspect handling: unsupported argument {flag:?}; expected --out <file> \
                 (retail handling probing arrives with F26-C/D)"
            ));
        }
        let Some(value) = cursor.next() else {
            return Err(format!("cs-inspect handling: {flag} needs a value"));
        };
        parsed.out = Some(PathBuf::from(value));
    }
    Ok(parsed)
}

/// Runs the `handling` command and returns its exit code.
///
/// `--out` is written atomically and its final path is reported on stderr;
/// without `--out` the JSON report goes to stdout. A failure is never returned
/// as success.
pub fn handling_command(args: &[String]) -> ExitCode {
    let run = handling_command_result(args);
    report_run(
        "handling",
        &run.diagnostics,
        run.report.as_deref(),
        run.out.as_deref(),
    );
    ExitCode::from(run.exit_code)
}

/// The body of [`handling_command`], separate so a failure carries its named
/// exit code and the report can be inspected without touching stdout.
pub fn handling_command_result(args: &[String]) -> HandlingRun {
    let parsed = match parse_handling_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return HandlingRun::failed(2, message),
    };

    let (report, summary) = match build_handling_report() {
        Ok(report) => {
            let summary = report.summary;
            (report.report, summary)
        }
        Err(message) => return HandlingRun::failed(1, message),
    };

    match parsed.out {
        Some(out) => match write_atomic(&out, &report) {
            Ok(()) => HandlingRun {
                exit_code: 0,
                report: Some(report),
                out: Some(out),
                diagnostics: Vec::new(),
                summary: Some(summary),
            },
            Err(error) => HandlingRun {
                exit_code: 1,
                report: Some(report),
                out: None,
                diagnostics: vec![format!(
                    "cs-inspect handling: cannot write report to {}: {error}",
                    out.display()
                )],
                summary: Some(summary),
            },
        },
        None => HandlingRun {
            exit_code: 0,
            report: Some(report),
            out: None,
            diagnostics: Vec::new(),
            summary: Some(summary),
        },
    }
}

/// The rendered report and its declared counts.
struct HandlingReport {
    report: String,
    summary: HandlingSummary,
}

/// Renders the declared synthetic handling schema and both comparison outcomes.
///
/// # Errors
///
/// The `String` diagnostic when the declared fixture fails its own validation,
/// which would be a defect in the fixture rather than in the caller.
fn build_handling_report() -> Result<HandlingReport, String> {
    let envelope = synthetic_reference_envelope();
    let covering = synthetic_covering_assessment().map_err(|error| {
        format!("the declared synthetic covering candidate is invalid: {error}")
    })?;
    let out_of_envelope = synthetic_out_of_envelope_assessment().map_err(|error| {
        format!("the declared synthetic out-of-envelope candidate is invalid: {error}")
    })?;

    let summary = HandlingSummary {
        probe_kinds: ProbeKind::ALL.len(),
        entries: envelope.entries.len(),
        held_out: envelope
            .entries
            .iter()
            .filter(|entry| entry.held_out)
            .count(),
        covering_passes: covering.passes(),
        out_of_envelope_passes: out_of_envelope.passes(),
    };
    let report = render_handling_report(&envelope, &covering, &out_of_envelope);
    Ok(HandlingReport { report, summary })
}

/// Renders one handling report.
fn render_handling_report(
    envelope: &ReferenceEnvelope,
    covering: &HandlingAssessment,
    out_of_envelope: &HandlingAssessment,
) -> String {
    let vocabulary = ProbeKind::ALL
        .iter()
        .map(|maneuver| {
            let quantity = maneuver.quantity();
            format!(
                "{{\"maneuver\":{},\"quantity\":{},\"unit\":{}}}",
                json_string(maneuver.label()),
                json_string(quantity.label()),
                json_string(quantity.unit()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    let missing = envelope
        .missing_maneuvers()
        .iter()
        .map(|maneuver| json_string(maneuver.label()))
        .collect::<Vec<_>>()
        .join(",");

    let held_out = envelope
        .entries
        .iter()
        .filter(|entry| entry.held_out)
        .map(|entry| json_string(entry.maneuver.label()))
        .collect::<Vec<_>>()
        .join(",");

    let entries = envelope
        .entries
        .iter()
        .map(render_entry)
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"schema\":{},\"source\":{},\"retail\":{},\"probe_kinds\":[{}],\
         \"envelope\":{{\"airframe\":{},\"origin\":{},\"model_kind\":{},\"difficulty\":{},\
         \"loadout\":{},\"timing_uncertainty\":{},\"entry_count\":{},\
         \"held_out\":[{}],\"missing_maneuvers\":[{}],\"entries\":[{}]}},\
         \"assessments\":[{{\"candidate\":\"covering\",\"passes\":{},\
         \"supports_original_fidelity\":{},\"failures\":{},\"unavailable\":{},\"verdicts\":[{}]}},\
         {{\"candidate\":\"acceleration_tuned_turn_outside\",\"passes\":{},\
         \"supports_original_fidelity\":{},\"failures\":{},\"unavailable\":{},\"verdicts\":[{}]}}],\
         \"note\":{}}}",
        json_string(HANDLING_REPORT_VERSION),
        json_string(SYNTHETIC_HANDLING_SOURCE_LABEL),
        envelope.origin.is_original(),
        vocabulary,
        json_string(&envelope.airframe_id),
        json_string(envelope.origin.label()),
        json_string(envelope.model_kind.label()),
        json_string(&envelope.difficulty),
        json_string(&envelope.loadout),
        render_timing(&envelope.timing_uncertainty),
        envelope.entries.len(),
        held_out,
        missing,
        entries,
        covering.passes(),
        covering.supports_original_fidelity_claim(),
        covering.failures().len(),
        covering.unavailable().len(),
        render_verdicts(covering),
        out_of_envelope.passes(),
        out_of_envelope.supports_original_fidelity_claim(),
        out_of_envelope.failures().len(),
        out_of_envelope.unavailable().len(),
        render_verdicts(out_of_envelope),
        json_string(
            "declared synthetic schema: no original installation was read, no probe was flown, \
             and no original-fidelity claim is made"
        ),
    )
}

fn render_timing(timing: &TimingUncertainty) -> String {
    format!(
        "{{\"plus_minus_s\":{},\"method\":{}}}",
        render_f64(timing.plus_minus_s),
        json_string(&timing.method),
    )
}

fn render_entry(entry: &EnvelopeEntry) -> String {
    let window = entry.accepted_window();
    let input = entry
        .input
        .iter()
        .map(render_input_step)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"maneuver\":{},\"quantity\":{},\"unit\":{},\"reference\":{},\
         \"tolerance\":{{\"plus_minus\":{},\"rationale\":{}}},\"accepted_window\":[{},{}],\
         \"held_out\":{},\"initial_state\":{},\"input\":[{}]}}",
        json_string(entry.maneuver.label()),
        json_string(entry.quantity.label()),
        json_string(&entry.unit),
        render_f64(entry.reference),
        render_f64(entry.tolerance.plus_minus),
        json_string(&entry.tolerance.rationale),
        render_f64(window[0]),
        render_f64(window[1]),
        entry.held_out,
        render_initial_state(&entry.initial_state),
        input,
    )
}

fn render_initial_state(state: &ProbeInitialState) -> String {
    format!(
        "{{\"airspeed_mps\":{},\"vertical_speed_mps\":{},\"altitude_m\":{},\
         \"engine_spool\":{},\"boost_available\":{}}}",
        render_f64(state.airspeed_mps),
        render_f64(state.vertical_speed_mps),
        render_f64(state.altitude_m),
        render_f64(state.engine_spool),
        state.boost_available,
    )
}

fn render_input_step(step: &ProbeInputStep) -> String {
    format!(
        "{{\"at_s\":{},\"pitch\":{},\"roll\":{},\"yaw\":{},\"throttle\":{},\"boost\":{}}}",
        render_f64(step.at_s),
        render_f64(step.input.pitch),
        render_f64(step.input.roll),
        render_f64(step.input.yaw),
        render_f64(step.input.throttle),
        step.input.boost,
    )
}

fn render_verdicts(assessment: &HandlingAssessment) -> String {
    assessment
        .verdicts
        .iter()
        .map(|verdict| {
            let measured = match verdict.measured {
                Some(value) => render_f64(value),
                None => "null".to_owned(),
            };
            let status = match verdict.status {
                VerdictStatus::WithinEnvelope => "\"within_envelope\"".to_owned(),
                VerdictStatus::OutOfEnvelope { deviation } => format!(
                    "{{\"out_of_envelope\":{{\"deviation\":{}}}}}",
                    render_f64(deviation)
                ),
                VerdictStatus::NoMeasurement => "\"no_measurement\"".to_owned(),
            };
            format!(
                "{{\"maneuver\":{},\"quantity\":{},\"held_out\":{},\"reference\":{},\
                 \"measured\":{},\"status\":{}}}",
                json_string(verdict.maneuver.label()),
                json_string(verdict.quantity.label()),
                verdict.held_out,
                render_f64(verdict.reference),
                measured,
                status,
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// A canonical finite value: JSON has no NaN or infinity, and the schema
/// refuses non-finite values, so this never emits one.
fn render_f64(value: f64) -> String {
    let mut text = String::new();
    let _ = write!(text, "{value}");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report names its synthetic source, is never retail, lists every
    /// probe kind and the held-out turn, and shows the two comparison outcomes
    /// — including that the out-of-envelope candidate does not pass.
    #[test]
    fn accept_f26_a_handling_report_names_synthetic_source_and_shows_both_outcomes() {
        let render = build_handling_report().expect("the declared fixture is valid");
        let report = render.report;
        assert!(report.contains(&format!("\"schema\":\"{HANDLING_REPORT_VERSION}\"")));
        assert!(report.contains(&format!("\"source\":\"{SYNTHETIC_HANDLING_SOURCE_LABEL}\"")));
        assert!(report.contains("\"retail\":false"));

        assert_eq!(render.summary.probe_kinds, 10);
        assert_eq!(render.summary.entries, 10);
        assert_eq!(render.summary.held_out, 1);
        assert!(render.summary.covering_passes);
        assert!(
            !render.summary.out_of_envelope_passes,
            "the out-of-envelope candidate must be reported as not passing"
        );

        for maneuver in ProbeKind::ALL {
            assert!(
                report.contains(&format!("\"maneuver\":\"{}\"", maneuver.label())),
                "{} is listed: {report}",
                maneuver.label()
            );
        }
        assert!(report.contains("\"held_out\":[\"turn\"]"));
        assert!(report.contains("\"missing_maneuvers\":[]"));
        assert!(report.contains("\"candidate\":\"covering\",\"passes\":true"));
        assert!(
            report.contains("\"candidate\":\"acceleration_tuned_turn_outside\",\"passes\":false")
        );
        assert!(report.contains("\"status\":{\"out_of_envelope\":{\"deviation\":40}}"));
        assert!(report.contains("\"supports_original_fidelity\":false"));
    }

    /// The command writes `--out` atomically and is byte-stable; a bad flag or
    /// a missing value is invalid input and reports nothing.
    #[test]
    fn accept_f26_a_handling_command_writes_out_and_refuses_bad_input() {
        let temp = std::env::temp_dir().join(format!("cs-f26-a-handling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).expect("the fixture directory is created");
        let path = temp.join("handling.json");

        let written = handling_command_result(&["--out".to_owned(), path.display().to_string()]);
        assert_eq!(written.exit_code, 0);
        assert_eq!(written.out.as_deref(), Some(path.as_path()));
        let on_disk = std::fs::read_to_string(&path).expect("the report is written");
        assert_eq!(Some(&on_disk), written.report.as_ref());

        let stdout = handling_command_result(&[]);
        assert_eq!(stdout.exit_code, 0);
        assert_eq!(
            on_disk,
            stdout.report.expect("the report is stable"),
            "the same schema serializes byte-for-byte identically"
        );

        assert_eq!(handling_command_result(&["--out".to_owned()]).exit_code, 2);
        assert_eq!(handling_command_result(&["--nope".to_owned()]).exit_code, 2);
        assert_eq!(
            handling_command_result(&["--cs-path".to_owned(), "/nope".to_owned()]).exit_code,
            2,
            "retail reading is not available in this stage"
        );

        let _ = std::fs::remove_dir_all(&temp);
    }
}
