//! Acceptance scenario F31-A for the `cs-inspect routes` command: the declared
//! route-graph contract is rendered as a read-only JSON report.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-A`. Task test prefix: `accept_f31_a_`.
//!
//! These tests drive the production command
//! [`cs_inspect::routes::routes_command_result`] and the shipped
//! `cs-inspect` binary. Refusing bad input, writing `--out` atomically and
//! naming the synthetic source are the behaviors that fail if the renderer or
//! the dispatch is removed.
//!
//! The inspected record is newly authored synthetic fixture data, never
//! original game data.

use std::path::PathBuf;
use std::process::Command;

/// A disposable output directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f31-a-routes-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the fixture directory is created");
        Self(root)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The library entry point renders the declared synthetic route: its schema,
/// its synthetic source (never retail), every node and edge, and the arch
/// trigger volume, all from production code.
#[test]
fn accept_f31_a_routes_command_reports_the_declared_synthetic_route() {
    let run = cs_inspect::routes::routes_command_result(&[]);
    assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
    assert!(run.out.is_none(), "without --out the report goes to stdout");

    let summary = run.summary.expect("the command reports its counts");
    assert_eq!(summary.nodes, 5);
    assert_eq!(summary.edges, 4);
    assert_eq!(summary.mandatory, 4);
    assert!(
        !summary.retail,
        "the declared fixture is synthetic, never retail"
    );

    let report = run.report.expect("the command reports");
    assert!(report.contains("\"schema\":\"cs-inspect-routes/v1\""));
    assert!(report.contains("\"source\":\"synthetic-fixture\""));
    assert!(report.contains("\"retail\":false"));
    assert!(report.contains("\"id\":\"route/synthetic.arch\""));
    assert!(report.contains("\"origin\":\"synthetic_fixture\""));
    assert!(report.contains("\"node_count\":5"));
    assert!(report.contains("\"edge_count\":4"));
    assert!(report.contains("\"mandatory_count\":4"));
    assert!(
        report.contains("\"from\":\"arch\",\"to\":\"exit\""),
        "the declared edges are reported: {report}"
    );
    assert!(
        report.contains("\"id\":\"synthetic.arch.opening\""),
        "the arch trigger volume is reported: {report}"
    );
}

/// The shipped binary runs the same `routes` command, and a bad flag or a
/// missing value is invalid input with a nonzero exit code and no report.
#[test]
fn accept_f31_a_routes_binary_runs_and_refuses_bad_input() {
    let output = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
        .arg("routes")
        .output()
        .expect("the cs-inspect binary must run without a GPU or retail installation");
    assert_eq!(output.status.code(), Some(0), "routes must exit zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"schema\":\"cs-inspect-routes/v1\"")
            && stdout.contains("\"source\":\"synthetic-fixture\""),
        "the binary prints the route report, got: {stdout:?}"
    );

    for argv in [vec!["routes", "--out"], vec!["routes", "--nope"]] {
        let refused = Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
            .args(&argv)
            .output()
            .expect("the cs-inspect binary must run");
        assert_eq!(
            refused.status.code(),
            Some(2),
            "{argv:?} is invalid input, got {}",
            String::from_utf8_lossy(&refused.stderr)
        );
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("cs-inspect routes:"),
            "{argv:?} explains the refusal"
        );
    }
}

/// `--out` is written atomically and the report on disk equals the report the
/// command returned, so a caller can trust either one.
#[test]
fn accept_f31_a_routes_command_writes_out_atomically() {
    let temp = TempDir::new("out");
    let path = temp.0.join("routes.json");
    let run = cs_inspect::routes::routes_command_result(&[
        "--out".to_owned(),
        path.display().to_string(),
    ]);
    assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
    assert_eq!(run.out.as_deref(), Some(path.as_path()));
    let written = std::fs::read_to_string(&path).expect("the report is written");
    assert_eq!(Some(&written), run.report.as_ref());
    assert!(written.contains("\"schema\":\"cs-inspect-routes/v1\""));
    assert!(written.contains("\"retail\":false"));
}
