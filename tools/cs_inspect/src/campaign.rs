//! The `campaign` command: expose the retail campaign directory layout as a
//! read-only inspection report (F14-E, task #373).
//!
//! ```text
//! cs-inspect campaign [--cs-path <dir>] [--out <file>]
//! ```
//!
//! The report lists every `ZBD/<chapter><variant>/<mission>` directory the
//! installation declares, with its chapter, mission number, world group,
//! program archive path and presence and — for a present archive — the
//! SHA-256 of its whole bytes. No original bytes are copied into the report;
//! names and hashes only.
//!
//! The walk is the production derivation
//! [`cs_content::campaign_bindings::campaign_layout`], which shares
//! [`cs_content::campaign_bindings`]'s `scan_campaign` with the per-mission
//! binding stages (`SourceContext`) and with F14-D's retail baseline
//! inventory, so the layout cannot be derived several different ways.
//!
//! This report makes no readiness, playability or original-behaviour claim:
//! it describes the installation's directory layout and the digests of the
//! archives stored in it. `--cs-path` wins over `CS_GAME_DIR`; with neither
//! the retail capability is missing (exit 4, `docs/contracts/CLI-EVIDENCE.md`).
//!
//! ```text
//! exit 0  the report was produced
//! exit 2  invalid input (unknown flag, missing value)
//! exit 3  the installation declares no campaign mission directory
//! exit 4  no installation selected (no --cs-path and no CS_GAME_DIR)
//! exit 1  a runtime failure walking the layout or writing --out
//! ```

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_content::campaign_bindings::{CampaignLayoutEntry, SourceBindingError, campaign_layout};

use crate::catalog::{json_string, report_run, write_atomic};

/// The report schema version this consumer writes.
pub const CAMPAIGN_REPORT_VERSION: &str = "cs-inspect-campaign/v1";

/// Parsed `campaign` arguments.
#[derive(Debug, Default)]
struct CampaignArgs {
    /// The explicit `--cs-path`, which wins over `CS_GAME_DIR`.
    cs_path: Option<PathBuf>,
    /// The `--out` report path; `None` writes the report to stdout.
    out: Option<PathBuf>,
}

/// Everything one `campaign` run produced.
#[derive(Debug)]
pub struct CampaignRun {
    /// The `CLI-EVIDENCE` exit code.
    pub exit_code: u8,
    /// The JSON report, when the layout was read.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
    /// How many campaign missions the report lists, when it lists any.
    pub mission_count: Option<usize>,
}

impl CampaignRun {
    fn failed(exit_code: u8, message: String) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![message],
            mission_count: None,
        }
    }
}

fn parse_campaign_args(args: &[String]) -> Result<CampaignArgs, String> {
    let mut parsed = CampaignArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        if flag != "--cs-path" && flag != "--out" {
            return Err(format!(
                "cs-inspect campaign: unsupported argument {flag:?}; expected --cs-path <dir> \
                 and/or --out <file>"
            ));
        }
        let Some(value) = cursor.next() else {
            return Err(format!("cs-inspect campaign: {flag} needs a value"));
        };
        if flag == "--cs-path" {
            parsed.cs_path = Some(PathBuf::from(value));
        } else {
            parsed.out = Some(PathBuf::from(value));
        }
    }
    Ok(parsed)
}

/// Runs the `campaign` command and returns its exit code.
///
/// `--cs-path` wins over `CS_GAME_DIR` (`docs/contracts/CLI-EVIDENCE.md`).
/// `--out` is written atomically and its final path is reported on stderr;
/// without `--out` the JSON report goes to stdout. A failure is never
/// returned as success.
pub fn campaign_command(args: &[String]) -> ExitCode {
    let run = campaign_command_result(args, std::env::var_os("CS_GAME_DIR"));
    report_run(
        "campaign",
        &run.diagnostics,
        run.report.as_deref(),
        run.out.as_deref(),
    );
    ExitCode::from(run.exit_code)
}

/// The body of [`campaign_command`], separate so the environment can be
/// injected by tests and so a failure carries its named exit code.
pub fn campaign_command_result(args: &[String], env_cs_path: Option<OsString>) -> CampaignRun {
    let parsed = match parse_campaign_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return CampaignRun::failed(2, message),
    };
    let cs_path = parsed.cs_path.or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return CampaignRun::failed(
            4,
            "cs-inspect campaign: no installation selected: pass --cs-path <dir> or set \
             CS_GAME_DIR"
                .to_owned(),
        );
    };

    let layout = match campaign_layout(&cs_path) {
        Ok(layout) => layout,
        Err(error) => {
            let exit_code = match error {
                SourceBindingError::NoCampaign => 3,
                _ => 1,
            };
            return CampaignRun::failed(exit_code, format!("cs-inspect campaign: {error}"));
        }
    };

    let mission_count = layout.len();
    let report = campaign_report(&cs_path, &layout);
    match parsed.out {
        Some(out) => match write_atomic(&out, &report) {
            Ok(()) => CampaignRun {
                exit_code: 0,
                report: Some(report),
                out: Some(out),
                diagnostics: Vec::new(),
                mission_count: Some(mission_count),
            },
            Err(error) => CampaignRun {
                exit_code: 1,
                report: Some(report),
                out: None,
                diagnostics: vec![format!(
                    "cs-inspect campaign: cannot write report to {}: {error}",
                    out.display()
                )],
                mission_count: Some(mission_count),
            },
        },
        None => CampaignRun {
            exit_code: 0,
            report: Some(report),
            out: None,
            diagnostics: Vec::new(),
            mission_count: Some(mission_count),
        },
    }
}

/// One chapter of the report: its number, how many missions it declares and
/// the world group directories those missions live in.
struct ChapterSummary {
    chapter: u32,
    mission_count: usize,
    world_groups: Vec<String>,
}

/// Renders the deterministic JSON campaign-layout report.
///
/// The mission rows are listed in the order the production derivation
/// produced them, which is `(chapter, mission number)` order; the report
/// does not reorder them, so the canonical order stays a property of the
/// shared walk rather than of this renderer. The chapter summary is
/// accumulated per chapter number and emitted in ascending chapter order, so
/// it counts every mission of a chapter exactly once whatever order the
/// rows arrive in. Every string is escaped by [`json_string`], so a path can
/// never break out of its field.
pub fn campaign_report(install_root: &Path, layout: &[CampaignLayoutEntry]) -> String {
    let mut grouped: BTreeMap<u32, ChapterSummary> = BTreeMap::new();
    for entry in layout {
        let summary = grouped
            .entry(entry.mission.chapter)
            .or_insert_with(|| ChapterSummary {
                chapter: entry.mission.chapter,
                mission_count: 0,
                world_groups: Vec::new(),
            });
        summary.mission_count += 1;
        if !summary.world_groups.contains(&entry.mission.world_group) {
            summary.world_groups.push(entry.mission.world_group.clone());
        }
    }
    // Canonical order inside a chapter, so the report does not depend on
    // which mission in the chapter happens to be walked first.
    for summary in grouped.values_mut() {
        summary.world_groups.sort();
    }
    let chapters: Vec<&ChapterSummary> = grouped.values().collect();

    let mut chapters_json = String::new();
    for (index, summary) in chapters.iter().enumerate() {
        if index > 0 {
            chapters_json.push(',');
        }
        let groups = summary
            .world_groups
            .iter()
            .map(|group| json_string(group))
            .collect::<Vec<_>>()
            .join(",");
        let _ = write!(
            chapters_json,
            "{{\"chapter\":{},\"mission_count\":{},\"world_groups\":[{}]}}",
            summary.chapter, summary.mission_count, groups,
        );
    }

    let mut missions_json = String::new();
    for (index, entry) in layout.iter().enumerate() {
        if index > 0 {
            missions_json.push(',');
        }
        let digest = match &entry.program_sha256 {
            Some(digest) => json_string(digest),
            None => "null".to_owned(),
        };
        let _ = write!(
            missions_json,
            "{{\"chapter\":{},\"mission_number\":{},\"world_group\":{},\"program_asset\":{},\
             \"program_present\":{},\"program_sha256\":{}}}",
            entry.mission.chapter,
            entry.mission.mission_number,
            json_string(&entry.mission.world_group),
            json_string(&entry.mission.program_asset),
            entry.mission.program_present,
            digest,
        );
    }

    format!(
        "{{\"schema\":{},\"source\":{},\"mission_count\":{},\"chapters\":[{}],\
         \"missions\":[{}]}}",
        json_string(CAMPAIGN_REPORT_VERSION),
        json_string(&install_root.display().to_string()),
        layout.len(),
        chapters_json,
        missions_json,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_groups_missions_by_chapter_and_sorts_the_world_groups() {
        let layout = vec![
            entry(1, 1, "c1c", true),
            entry(1, 2, "c1", true),
            entry(2, 1, "c2", false),
        ];
        let report = campaign_report(Path::new("/install"), &layout);
        assert!(
            report.contains("\"mission_count\":3"),
            "every mission is counted, got: {report}"
        );
        assert!(
            report.contains("\"chapter\":1,\"mission_count\":2,\"world_groups\":[\"c1\",\"c1c\"]"),
            "chapter 1 keeps both world groups in canonical order, got: {report}"
        );
        assert!(
            report.contains("\"chapter\":2,\"mission_count\":1,\"world_groups\":[\"c2\"]"),
            "chapter 2 stays separate, got: {report}"
        );
        // A present archive carries its digest; an absent one is explicit.
        assert!(
            report.contains(&format!("\"program_sha256\":\"{}\"", "a".repeat(64))),
            "a present archive carries a digest, got: {report}"
        );
        assert!(
            report.contains("\"program_sha256\":null"),
            "an absent archive is explicit, got: {report}"
        );
    }

    /// Chapter rows are accumulated per chapter number, so missions of one
    /// chapter are counted and grouped together even when they are not
    /// adjacent in the layout.
    #[test]
    fn report_counts_a_chapter_that_is_not_walked_contiguously() {
        let layout = vec![
            entry(2, 1, "c2", true),
            entry(1, 1, "c1", true),
            entry(2, 2, "c2b", true),
            entry(1, 2, "c1b", true),
        ];
        let report = campaign_report(Path::new("/install"), &layout);
        assert!(
            report.contains("\"chapter\":1,\"mission_count\":2,\"world_groups\":[\"c1\",\"c1b\"]"),
            "chapter 1 is one row of two missions, got: {report}"
        );
        assert!(
            report.contains("\"chapter\":2,\"mission_count\":2,\"world_groups\":[\"c2\",\"c2b\"]"),
            "chapter 2 is one row of two missions, got: {report}"
        );
        assert_eq!(
            report.matches("\"chapter\":").count(),
            6,
            "two chapter rows and four mission rows, nothing split or repeated, got: {report}"
        );
        let chapters = report
            .split_once("\"chapters\":[")
            .and_then(|(_, tail)| tail.split_once("],\"missions\":["))
            .map(|(head, _)| head)
            .expect("the report separates its chapter rows from its mission rows");
        assert_eq!(
            chapters.matches("\"chapter\":").count(),
            2,
            "each chapter is one summary row, got: {chapters}"
        );
    }

    /// A mission entry for the report tests: the digest is authored, since
    /// this only exercises the renderer.
    fn entry(chapter: u32, number: u32, group: &str, present: bool) -> CampaignLayoutEntry {
        CampaignLayoutEntry {
            mission: cs_content::campaign_bindings::CampaignMission {
                chapter,
                mission_number: number,
                world_group: group.to_owned(),
                program_asset: format!("ZBD/{group}/M{number:02}/zrdr.zbd"),
                program_present: present,
            },
            program_sha256: present.then(|| "a".repeat(64)),
        }
    }
}
