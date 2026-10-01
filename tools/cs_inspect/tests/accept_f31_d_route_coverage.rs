//! F31-D acceptance: `cs-inspect routes --coverage` audits the retail
//! installation's route-carrier coverage in every mission type (task #128).
//!
//! The synthetic tests run the shipped binary (`CARGO_BIN_EXE_cs-inspect`)
//! over newly authored fixture trees: they prove the production walk,
//! classification and carrier check, never anything about a retail
//! installation, and never touch `$CS_GAME_DIR`. The retail test is
//! `#[ignore = "requires CS_GAME_DIR"]` and is run by the implementing and
//! reviewing agents with `--include-ignored`; it asserts the installation's
//! campaign/Instant Action/multiplayer mission directories all carry the
//! observed AI-navigation carrier member.
//!
//! The audit is a **carrier coverage, not a route decode**: the report states
//! the original route encoding is unmeasured and the tests never assert a
//! decoded route.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::TempTree;

/// Builds a reader-family archive (F06/indexed by a version-one trailer) whose
/// members are `(name, body)` in declaration order: member data, then one
/// 148-byte index entry per member, then the version-one trailer.
///
/// This is the same shape the F13-B tests author; it is fixture data, not
/// original bytes.
fn reader_archive(members: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut extents = Vec::new();
    for (name, body) in members {
        extents.push((bytes.len() as u32, body.len() as u32, *name));
        bytes.extend_from_slice(body);
    }
    for (start, length, name) in extents {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let mut field = [0u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0u8; 76]);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// A mission archive that carries the observed AI-navigation carrier member.
fn carrier_archive() -> Vec<u8> {
    reader_archive(&[
        (b"aiv.zrd", b"\x0a\x00\x00\x00\x0b\x00\x00\x00"),
        (b"objectives.zrd", b"\x01\x00\x00\x00"),
    ])
}

/// A mission archive that hides the AI-navigation carrier member.
fn carrierless_archive() -> Vec<u8> {
    reader_archive(&[(b"objectives.zrd", b"\x01\x00\x00\x00")])
}

/// A three-mission fixture with one mission of each observed type, each
/// carrying the carrier member.
fn coverage_tree(label: &str) -> TempTree {
    let tree = TempTree::new(label);
    tree.write("ZBD/C1/M01/zrdr.zbd", &carrier_archive());
    tree.write("ZBD/C1/IA1/zrdr.zbd", &carrier_archive());
    tree.write("ZBD/C2B/MP2/zrdr.zbd", &carrier_archive());
    tree
}

/// The shipped binary under test.
fn cs_inspect() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cs-inspect"))
}

/// The original installation, as the environment declares it. Panics when
/// `CS_GAME_DIR` is unset, so an `--include-ignored` run without the retail
/// capability fails loudly instead of passing vacuously.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: F31-D's retail test needs the retail capability; run this \
             suite with `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

fn coverage_report(tree: &TempTree) -> String {
    let out = tree.root().join("coverage.json");
    let output = cs_inspect()
        .arg("routes")
        .arg("--coverage")
        .arg("--cs-path")
        .arg(tree.root())
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "routes --coverage exits zero on success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(&out).expect("the --out report exists")
}

// ------------------------------------------------------------- synthetic ---

/// The coverage report names every mission type, counts each and reports the
/// unmeasured route encoding rather than a decoded route.
#[test]
fn accept_f31_d_coverage_reports_every_mission_type() {
    let tree = coverage_tree("report");
    let report = coverage_report(&tree);

    assert!(
        report.contains("\"schema\":\"cs-inspect-routes-coverage/v1\""),
        "the report names its schema, got: {report}"
    );
    assert!(
        report.contains("\"retail\":true"),
        "a read installation is retail, got: {report}"
    );
    assert!(
        report.contains("\"carrier_member\":\"aiv.zrd\""),
        "the carrier member is named, got: {report}"
    );
    assert!(
        report.contains("\"mission_count\":3") && report.contains("\"covered\":3"),
        "all three missions are covered, got: {report}"
    );
    assert!(
        report.contains("\"coverage\":true"),
        "coverage holds, got: {report}"
    );
    for (mission_type, count) in [
        ("campaign", 1),
        ("instant_action", 1),
        ("multiplayer", 1),
        ("other", 0),
    ] {
        assert!(
            report.contains(&format!(
                "\"type\":\"{mission_type}\",\"mission_count\":{count},\"covered\":{count}"
            )),
            "{mission_type} is counted, got: {report}"
        );
    }
    assert!(
        report.contains("\"route_encoding\":{\"state\":\"unmeasured\""),
        "the report states the route encoding is unmeasured, got: {report}"
    );
    // The three mission rows are in canonical (group, mission) order:
    // group `C1` before `C2B`, and inside `C1` its `IA1` before `M01`.
    let c1_ia1 = report.find("\"mission\":\"IA1\"").expect("IA1 row");
    let c1_m01 = report.find("\"mission\":\"M01\"").expect("M01 row");
    let c2b_mp2 = report.find("\"mission\":\"MP2\"").expect("MP2 row");
    assert!(
        c1_ia1 < c1_m01 && c1_m01 < c2b_mp2,
        "the rows are in canonical (group, mission) order, got: {report}"
    );
}

/// A mission that hides the carrier member or its archive is a coverage
/// failure (exit 3) whose report stays visible and names the mission.
#[test]
fn accept_f31_d_coverage_fails_closed_on_a_missing_carrier_or_archive() {
    // A campaign mission without the carrier member.
    let hidden = TempTree::new("hidden-carrier");
    hidden.write("ZBD/C1/M01/zrdr.zbd", &carrierless_archive());
    let out = hidden.root().join("coverage.json");
    let output = cs_inspect()
        .arg("routes")
        .arg("--coverage")
        .arg("--cs-path")
        .arg(hidden.root())
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(3),
        "a hidden carrier member is exit 3, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("aiv.zrd"),
        "the failure names the missing carrier, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&out).expect("the coverage report is still written");
    assert!(
        report.contains("\"carrier_present\":false"),
        "the failing row is visible, got: {report}"
    );
    assert!(
        report.contains("\"coverage\":false"),
        "coverage does not pass, got: {report}"
    );

    // An Instant Action directory without its program archive.
    let missing = TempTree::new("missing-archive");
    missing.mkdir("ZBD/C1/IA1");
    let output = cs_inspect()
        .arg("routes")
        .arg("--coverage")
        .arg("--cs-path")
        .arg(missing.root())
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(3),
        "a missing archive is exit 3, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"program_present\":false") && stdout.contains("\"coverage\":false"),
        "the missing archive stays visible, got: {stdout}"
    );

    // An installation with no mission directory at all is not an empty pass.
    let empty = TempTree::new("empty");
    empty.mkdir("ZBD/C1");
    let output = cs_inspect()
        .arg("routes")
        .arg("--coverage")
        .arg("--cs-path")
        .arg(empty.root())
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(3),
        "an installation with no mission directory is exit 3, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Missing installation and malformed input are named, nonzero exits.
#[test]
fn accept_f31_d_coverage_refuses_missing_installation_and_bad_input() {
    // No --cs-path and no CS_GAME_DIR: the retail capability is missing.
    let output = cs_inspect()
        .arg("routes")
        .arg("--coverage")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(4),
        "no installation is exit 4, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("cs-inspect:"));

    // Malformed input and conflicting modes are exit 2.
    for args in [
        vec!["routes", "--coverage", "--bogus", "x"],
        vec!["routes", "--coverage", "--cs-path"],
        vec!["routes", "--coverage", "--follow"],
        vec!["routes", "--follow", "--cs-path", "/tmp"],
    ] {
        let output = cs_inspect()
            .args(&args)
            .env_remove("CS_GAME_DIR")
            .output()
            .expect("cs-inspect runs");
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} is exit 2 (invalid input), stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // A missing ZBD directory is a runtime failure that names the path.
    let no_zbd = TempTree::new("no-zbd");
    no_zbd.write("GOSDATA/readme.txt", b"x");
    let output = cs_inspect()
        .arg("routes")
        .arg("--coverage")
        .arg("--cs-path")
        .arg(no_zbd.root())
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        output.status.code(),
        Some(1),
        "a missing ZBD is a runtime failure, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(&no_zbd.root().join("ZBD").display().to_string()),
        "the failure names the path it happened at, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The command body is public production code: `--cs-path` wins over
/// `CS_GAME_DIR` and without `--out` the report is rendered in memory.
#[test]
fn accept_f31_d_command_result_wins_flag_over_environment() {
    let flagged = coverage_tree("flagged");
    let environ = coverage_tree("environ");
    environ.write("ZBD/C3/M07/zrdr.zbd", &carrier_archive());

    let run = cs_inspect::routes::routes_command_result_with_env(
        &[
            "--coverage".to_owned(),
            "--cs-path".to_owned(),
            flagged.root().display().to_string(),
        ],
        Some(environ.root().as_os_str().to_owned()),
    );
    assert_eq!(run.exit_code, 0, "diagnostics: {:?}", run.diagnostics);
    assert!(run.out.is_none(), "without --out no file is written");
    let summary = run.coverage.expect("the coverage summary is present");
    assert_eq!(summary.missions, 3);
    assert_eq!(summary.covered, 3);
    assert!(summary.covered());
    assert_eq!(summary.campaign, 1);
    assert_eq!(summary.instant_action, 1);
    assert_eq!(summary.multiplayer, 1);
    let report = run.report.expect("the report was rendered");
    assert!(
        !report.contains("\"mission\":\"M07\""),
        "CS_GAME_DIR must not contribute when --cs-path is given, got: {report}"
    );

    // Without --cs-path the environment selects the installation.
    let run = cs_inspect::routes::routes_command_result_with_env(
        &["--coverage".to_owned()],
        Some(environ.root().as_os_str().to_owned()),
    );
    assert_eq!(run.exit_code, 0, "diagnostics: {:?}", run.diagnostics);
    assert_eq!(run.coverage.expect("summary").missions, 4);
}

// ---------------------------------------------------------------- retail ---

/// The installation's campaign, Instant Action and multiplayer mission
/// directories all carry the observed AI-navigation carrier member, and the
/// shipped consumer reports that coverage.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f31_d_retail_covers_every_mission_type() {
    let install = game_dir();
    // The report is written outside the read-only installation and outside the
    // checkout: cs-inspect never writes inside `$CS_GAME_DIR`.
    let out = std::env::temp_dir().join(format!(
        "cs-f31-d-retail-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    let output = cs_inspect()
        .arg("routes")
        .arg("--coverage")
        .arg("--cs-path")
        .arg(&install)
        .arg("--out")
        .arg(&out)
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("cs-inspect runs");
    let report = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    assert_eq!(
        output.status.code(),
        Some(0),
        "coverage over the installation holds, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        report.contains("\"mission_count\":53") && report.contains("\"covered\":53"),
        "the installation declares 53 covered mission directories, got: {report}"
    );
    assert!(
        report.contains("\"type\":\"campaign\",\"mission_count\":24,\"covered\":24"),
        "24 campaign missions are covered, got: {report}"
    );
    assert!(
        report.contains("\"type\":\"instant_action\",\"mission_count\":8,\"covered\":8"),
        "8 Instant Action scenarios are covered, got: {report}"
    );
    assert!(
        report.contains("\"type\":\"multiplayer\",\"mission_count\":21,\"covered\":21"),
        "21 multiplayer scenarios are covered, got: {report}"
    );
    assert!(
        report.contains("\"coverage\":true"),
        "the installation's coverage passes, got: {report}"
    );
}
