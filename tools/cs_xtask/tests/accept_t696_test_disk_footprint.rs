//! Task #696: the workspace's own share of the CI runner's disk.
//!
//! The `rust` CI job starts with 124 GB free after the owner's toolchain
//! cleanup and ends `cargo test --workspace` with 20 GB free on main at
//! 385160c2 (run 37394319899), so it writes about 104 GB; one commit earlier
//! (73f84b1c, run 37386273061) the same job ended with 450 MB free. The bytes
//! are the test binaries: 376 of them on this tree, and the engine-linked ones
//! measure 105 MB to 233 MB each on a local `dev` build. So whether the next
//! test file fits is decided by a measurement, and this suite pins the
//! measurement.
//!
//! What is pinned here:
//!
//! * the plan is read from the manifests and the filesystem, and every target
//!   in it is a file that exists — a target the plan invents is a failure, and
//!   so is a file the plan drops;
//! * a target's bytes come from the binary whose `.d` sidecar names that
//!   target's own source file, so two members with equally named test files
//!   stay apart;
//! * a target with no binary in the target directory measures as *unknown*,
//!   and a tree with nothing measured reports no marginal cost rather than
//!   zero, so "not built here" can never be read as "free";
//! * `test = false` on `[lib]` / `[[bin]]` keeps a harness out of the plan,
//!   the way `tools/cs_xtask/Cargo.toml` uses it (task #610).
//!
//! Nothing here weakens a check and nothing here is a gate: the report is a
//! measurement, and this suite fails when the measurement stops being one.
//! The CI numbers and the `.github/` options left to the owner are in
//! `docs/findings/2026-10-06-t696-ci-runner-disk-budget.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_xtask::bootstrap;
use cs_xtask::footprint::{self, ENGINE_LINKED_FLOOR, Footprint, TargetKind, TestTarget};
use cs_xtask::transient;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Scratch space unique to this test process, matching the pid-keyed layout
/// the sibling suites adopted so overlapping `cargo test` runs cannot delete
/// each other's fixtures.
fn scratch(name: &str) -> PathBuf {
    let dir = workspace_root().join(format!(
        "target/t696-footprint-fixtures/{}/{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the scratch directory must be creatable");
    dir
}

/// Writes a fake workspace: `members` as `[workspace] members`, one
/// `crates/<member>/Cargo.toml` and the test files named by `tests`.
fn fake_workspace(root: &Path, members: &[(&str, &str, &[&str])]) {
    let listed: Vec<String> = members
        .iter()
        .map(|(member, _, _)| format!("\"crates/{member}\""))
        .collect();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver = \"3\"\nmembers = [\n  {}\n]\n",
            listed.join(",\n  ")
        ),
    )
    .expect("the workspace manifest must be writable");
    for (member, manifest, tests) in members {
        let dir = root.join("crates").join(member);
        fs::create_dir_all(dir.join("src")).expect("the member src dir must be creatable");
        fs::write(dir.join("Cargo.toml"), manifest).expect("the member manifest must be writable");
        fs::write(dir.join("src").join("lib.rs"), "// crate\n").expect("src/lib.rs");
        for test in *tests {
            let path = dir.join("tests").join(test);
            fs::create_dir_all(path.parent().expect("a test path has a parent"))
                .expect("the tests dir must be creatable");
            fs::write(path, "// test\n").expect("the test file must be writable");
        }
    }
}

/// Writes a cargo `.d` sidecar and the binary it describes, so a target can be
/// measured from a target directory that was built somewhere else.
fn built_binary(deps: &Path, name: &str, source: &str, bytes: usize) {
    fs::create_dir_all(deps).expect("the deps dir must be creatable");
    // Cargo's stem is `<target name>-<hash>`; the hash is derived from the
    // source path here so two members with the same target name get the
    // distinct files real cargo produces.
    let hash: u64 = source.bytes().fold(1469598103934665603u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1099511628211)
    });
    let stem = format!("{name}-{hash:016x}");
    fs::write(
        deps.join(format!("{stem}.d")),
        format!("/somewhere/target/debug/deps/{stem}.d: {source}\n\n{source}:\n\n"),
    )
    .expect("the dep file must be writable");
    fs::write(deps.join(&stem), vec![0u8; bytes]).expect("the binary must be writable");
}

/// Every file cargo would link a test binary from, straight from the
/// filesystem: `tests/<name>.rs` and `tests/<name>/main.rs`.
fn integration_files(root: &Path) -> Vec<String> {
    let manifest = transient::read_to_string(&root.join("Cargo.toml"), transient::PATIENT)
        .expect("the workspace manifest must be readable");
    let mut found = Vec::new();
    for member in bootstrap::workspace_members(&manifest).expect("the members list") {
        let tests = root.join(&member).join("tests");
        if !transient::is_dir(&tests, transient::SCAN) {
            continue;
        }
        let mut names: Vec<String> = transient::read_dir(&tests, transient::SCAN)
            .expect("the tests dir must be readable")
            .flatten()
            .filter_map(|entry| {
                let file_type = transient::file_type(&entry, transient::SCAN).ok()?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if file_type.is_dir() {
                    transient::is_file(&tests.join(&name).join("main.rs"), transient::SCAN)
                        .then(|| format!("{member}/tests/{name}/main.rs"))
                } else {
                    name.ends_with(".rs")
                        .then(|| format!("{member}/tests/{name}"))
                }
            })
            .collect();
        found.append(&mut names);
    }
    found.sort();
    found
}

/// The measured plan of this workspace, which is what the CI job links: every
/// test file the tree really has, every enabled harness, and nothing else.
#[test]
fn accept_t696_the_workspaces_test_binaries_are_measured_not_guessed() {
    let root = workspace_root();
    let deps = footprint::default_deps_dir(&root);
    let report = footprint::measure_workspace(&root, &deps)
        .unwrap_or_else(|error| panic!("the workspace's test plan must be readable: {error}"));

    let planned: Vec<&str> = report
        .targets
        .iter()
        .map(|target| target.source.as_str())
        .collect();
    let on_disk = integration_files(&root);
    for source in &on_disk {
        assert!(
            planned.contains(&source.as_str()),
            "{} exists, so the plan must contain it: the plan is what CI links",
            source
        );
    }
    assert_eq!(
        planned.len(),
        on_disk.len() + report.unit_harness_count(),
        "the plan is the {} test files plus the enabled unit-test harnesses",
        on_disk.len()
    );

    // `tools/cs_xtask` sets `test = false` on both of its targets (task #610),
    // so its harnesses are not in the plan and cannot spend a runner byte.
    let manifest = fs::read_to_string(root.join("tools/cs_xtask/Cargo.toml"))
        .expect("cs_xtask's manifest must be readable");
    assert!(!footprint::lib_harness_is_linked(&manifest));
    assert!(!footprint::bin_harness_is_linked(&manifest, "src/main.rs"));
    for target in &report.targets {
        assert!(
            !target.source.starts_with("tools/cs_xtask/src/"),
            "cs_xtask's harnesses are turned off, so {} must not be in the plan",
            target.source
        );
    }

    // This target directory holds a build of the tree, so every target in the
    // plan is measured and the marginal cost is that measurement.
    assert_eq!(
        report.unmeasured().count(),
        0,
        "a target directory holding this build must measure every target in the plan"
    );
    let marginal = report
        .marginal_bytes()
        .expect("a measured engine-linked group must report a marginal cost");
    assert!(
        marginal >= ENGINE_LINKED_FLOOR,
        "the marginal cost {marginal} must come from the engine-linked group"
    );
    assert!(
        report.measured_bytes() > marginal,
        "the measured total must exceed one binary, or the plan is not measured"
    );
    let largest = report
        .largest()
        .expect("a measured tree has a largest binary");
    assert!(
        largest.bytes.unwrap_or_default() >= marginal,
        "the largest measured binary must not be below the median"
    );
}

/// The plan is this tree's real test files, one binary each, and it drops
/// nothing and invents nothing.
#[test]
fn accept_t696_the_plan_is_read_from_the_manifests_and_the_filesystem() {
    let root = scratch("plan");
    fake_workspace(
        &root,
        &[
            (
                "heavy",
                "[package]\nname = \"heavy\"\n\n[lib]\n",
                &["flight.rs", "physics/main.rs"],
            ),
            (
                "light",
                "[package]\nname = \"light\"\n\n[lib]\n",
                &["wire.rs"],
            ),
            (
                "silent",
                "[package]\nname = \"silent\"\n\n[lib]\ntest = false\n\n[[bin]]\nname = \"silent\"\npath = \"src/main.rs\"\ntest = false\n",
                &[],
            ),
        ],
    );
    fs::write(root.join("crates/silent/src/main.rs"), "fn main() {}\n").expect("src/main.rs");
    for member in ["heavy", "light"] {
        fs::write(
            root.join("crates").join(member).join("src/main.rs"),
            "fn main() {}\n",
        )
        .expect("src/main.rs");
    }

    let report = footprint::measure_workspace(&root, &root.join("no-such-deps")).expect("the plan");
    let sources: Vec<&str> = report.targets.iter().map(|t| t.source.as_str()).collect();
    assert_eq!(
        sources,
        vec![
            "crates/heavy/src/lib.rs",
            "crates/heavy/src/main.rs",
            "crates/heavy/tests/flight.rs",
            "crates/heavy/tests/physics/main.rs",
            "crates/light/src/lib.rs",
            "crates/light/src/main.rs",
            "crates/light/tests/wire.rs",
        ],
        "both test layouts count, every enabled harness counts and a `test = false` target does not"
    );
    assert_eq!(report.unit_harness_count(), 4);
    assert_eq!(
        report
            .targets
            .iter()
            .filter(|t| t.kind == TargetKind::Integration)
            .count(),
        3
    );
    for target in &report.targets {
        assert!(
            root.join(&target.source).is_file(),
            "{} is in the plan, so it must be a file on disk",
            target.source
        );
    }
}

/// A target's bytes are the binary of its own source file, never a same-named
/// neighbour's: the two members here both have `tests/wire.rs`.
#[test]
fn accept_t696_a_measurement_is_taken_from_the_binary_of_that_source() {
    let root = scratch("measure");
    fake_workspace(
        &root,
        &[
            ("one", "[package]\nname = \"one\"\n\n[lib]\n", &["wire.rs"]),
            ("two", "[package]\nname = \"two\"\n\n[lib]\n", &["wire.rs"]),
        ],
    );
    let deps = root.join("target/debug/deps");
    built_binary(&deps, "wire", "crates/one/tests/wire.rs", 111);
    built_binary(&deps, "wire", "crates/two/tests/wire.rs", 222);

    let report = footprint::measure_workspace(&root, &deps).expect("the plan");
    let measured: Vec<(&str, u64)> = report
        .targets
        .iter()
        .filter(|t| t.source.ends_with("tests/wire.rs"))
        .map(|t| {
            (
                t.source.as_str(),
                t.bytes
                    .expect("both wire.rs binaries exist in this target dir"),
            )
        })
        .collect();
    assert!(
        report
            .targets
            .iter()
            .filter(|t| t.source.ends_with("tests/wire.rs"))
            .all(|t| t.binaries == 1),
        "an integration test file is linked once, so its source produced one binary"
    );
    assert_eq!(
        measured,
        vec![
            ("crates/one/tests/wire.rs", 111),
            ("crates/two/tests/wire.rs", 222)
        ],
        "each member's wire.rs must measure its own binary, not the other member's"
    );
}

/// An unbuilt target is unknown, never zero: a target directory with nothing in
/// it reports no marginal cost and a zero total rather than a free plan.
#[test]
fn accept_t696_an_unbuilt_target_is_unknown_and_not_zero() {
    let root = scratch("unbuilt");
    fake_workspace(
        &root,
        &[("one", "[package]\nname = \"one\"\n\n[lib]\n", &["wire.rs"])],
    );
    let report = footprint::measure_workspace(&root, &root.join("empty")).expect("the plan");

    assert_eq!(report.planned(), 2, "the plan still holds both targets");
    assert_eq!(report.measured().count(), 0);
    assert_eq!(report.unmeasured().count(), 2);
    assert_eq!(
        report.measured_bytes(),
        0,
        "nothing measured, so nothing summed"
    );
    assert_eq!(
        report.marginal_bytes(),
        None,
        "with nothing measured the marginal cost is unknown, not zero"
    );
    assert_eq!(report.largest(), None);

    // One measured small binary still leaves the marginal cost unknown: it is
    // the engine-linked group that decides what a new test file costs.
    let deps = root.join("target/debug/deps");
    built_binary(&deps, "wire", "crates/one/tests/wire.rs", 4096);
    let report = footprint::measure_workspace(&root, &deps).expect("the plan");
    assert_eq!(report.measured().count(), 1);
    assert_eq!(report.small().len(), 1);
    assert!(report.engine_linked().is_empty());
    assert_eq!(report.marginal_bytes(), None);
    assert_eq!(report.measured_bytes(), 4096);
}

/// `test = false` is read from the target's own table only: a `test` key in
/// another table, or behind a comment, must not turn a harness off or on.
#[test]
fn accept_t696_a_disabled_harness_is_read_from_its_own_table() {
    let manifest = "\
[package]
name = \"cs_xtask\"

[lib]
# task #610: no unit tests in src/, so no empty harness in the plan.
test = false

[profile.dev]
debug = \"line-tables-only\"

[[bin]]
name = \"cs_xtask\"
test = false

[[bin]]
name = \"other\"
path = \"src/other.rs\"
test = true
";
    assert!(!footprint::lib_harness_is_linked(manifest));
    assert!(!footprint::bin_harness_is_linked(manifest, "src/main.rs"));
    assert!(
        footprint::bin_harness_is_linked(manifest, "src/other.rs"),
        "an explicit `test = true` keeps that harness in the plan"
    );
    assert!(
        footprint::lib_harness_is_linked("[package]\nname = \"x\"\n"),
        "a manifest that says nothing links the lib harness by default"
    );
    assert!(
        footprint::bin_harness_is_linked("[package]\nname = \"x\"\n", "src/main.rs"),
        "a member with no [[bin]] table links src/main.rs by default"
    );
}

/// The engine-linked split reports both group sizes and takes the marginal
/// cost from the measured engine-linked group only.
#[test]
fn accept_t696_the_engine_linked_split_reports_both_groups() {
    let heavy = |bytes: u64| TestTarget {
        member: "crates/cs_app".to_string(),
        name: "flight".to_string(),
        source: format!("crates/cs_app/tests/flight-{bytes}"),
        kind: TargetKind::Integration,
        bytes: Some(bytes),
        binaries: 1,
    };
    let report = Footprint {
        targets: vec![
            heavy(100_000_000),
            heavy(120_000_000),
            heavy(200_000_000),
            heavy(2_000_000),
            heavy(3_000_000),
        ],
    };

    assert_eq!(report.engine_linked().len(), 3);
    assert_eq!(report.small().len(), 2);
    assert_eq!(
        report.marginal_bytes(),
        Some(120_000_000),
        "the marginal cost is the median of the engine-linked group"
    );
    assert_eq!(report.largest().map(|t| t.bytes), Some(Some(200_000_000)));
    assert_eq!(report.measured_bytes(), 425_000_000);
    assert_eq!(
        report.by_member(),
        vec![footprint::MemberFootprint {
            member: "crates/cs_app".to_string(),
            planned: 5,
            measured: 5,
            measured_bytes: 425_000_000,
        }]
    );
}

/// A `src/main.rs` bin target is linked twice by `cargo test` — the plain
/// binary an integration test can exec, and its own test harness — so the
/// measurement reports both and keeps the larger size.
#[test]
fn accept_t696_a_bin_target_is_linked_twice_and_the_larger_size_is_kept() {
    let root = scratch("twice");
    fake_workspace(
        &root,
        &[("app", "[package]\nname = \"app\"\n\n[lib]\n", &["wire.rs"])],
    );
    fs::write(root.join("crates/app/src/main.rs"), "fn main() {}\n").expect("src/main.rs");
    let deps = root.join("target/debug/deps");
    fs::create_dir_all(&deps).expect("the deps dir must be creatable");
    // Two binaries from one `src/main.rs`: the plain 40-byte one and a
    // 120-byte harness.
    for (name, _bytes) in [("app-1111111111111111", 40), ("app-2222222222222222", 120)] {
        fs::write(
            deps.join(format!("{name}.d")),
            format!(".../{name}.d: crates/app/src/main.rs\n\ncrates/app/src/main.rs:\n"),
        )
        .expect("the dep file must be writable");
    }
    for (name, bytes) in [("app-1111111111111111", 40), ("app-2222222222222222", 120)] {
        fs::write(deps.join(name), vec![0u8; bytes]).expect("the binary must be writable");
    }
    // The lib harness's rule line also names `src/main.rs` as one of the module
    // sources its crate is made of, and it is the largest binary here. Matching
    // any dependency instead of the root would report 4096 for `src/main.rs`.
    fs::write(
        deps.join("app-3333333333333333.d"),
        ".../app-3333333333333333.d: crates/app/src/lib.rs crates/app/src/main.rs\n\n\
         crates/app/src/lib.rs:\ncrates/app/src/main.rs:\n",
    )
    .expect("the dep file must be writable");
    fs::write(deps.join("app-3333333333333333"), vec![0u8; 4096]).expect("the binary");

    let report = footprint::measure_workspace(&root, &deps).expect("the plan");
    let main = report
        .targets
        .iter()
        .find(|t| t.source == "crates/app/src/main.rs")
        .expect("src/main.rs is a harness target");
    assert_eq!(
        (main.bytes, main.binaries),
        (Some(120), 2),
        "both binaries of src/main.rs are counted, the larger size is kept, and the lib \
         harness's 4096-byte rule line naming src/main.rs does not leak into it"
    );
    let lib = report
        .targets
        .iter()
        .find(|t| t.source == "crates/app/src/lib.rs")
        .expect("src/lib.rs is a harness target");
    assert_eq!(
        (lib.bytes, lib.binaries),
        (Some(4096), 1),
        "the lib harness is its own binary, and its rule line also naming src/main.rs \
         must not move the bin's measurement"
    );
}

/// The command the report is reached through prints the measurement, and says
/// plainly that doc-test binaries are outside it.
#[test]
fn accept_t696_the_report_command_prints_the_measured_footprint() {
    let bin = env!("CARGO_BIN_EXE_cs_xtask");
    let output = transient::command_output(
        Command::new(bin)
            .arg("report-test-disk")
            .arg("--workspace-root")
            .arg(workspace_root()),
    )
    .expect("cs_xtask report-test-disk must run");
    assert!(
        output.status.success(),
        "the report is not a gate: it must exit 0"
    );

    let text = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "test binaries in the plan",
        "engine-linked",
        "measured in total",
        "doc-test binaries are not counted",
    ] {
        assert!(
            text.contains(expected),
            "the report must state {expected:?}:\n{text}"
        );
    }
    // A number that was measured, not a placeholder.
    assert!(
        !text.contains("marginal cost of another test file is unknown"),
        "this target directory holds a build, so the marginal cost must be measured:\n{text}"
    );
}
