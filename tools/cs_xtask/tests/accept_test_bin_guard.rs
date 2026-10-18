//! Task #1542: one integration-test binary per crate, and the guard that keeps
//! it honest.
//!
//! A crate that sets `autotests = false` and declares one `[[test]]` target
//! links and runs exactly that one binary, so a `.rs` file under `tests/` that
//! no `mod` declaration reaches is silently neither compiled nor run. These
//! tests drive the production guard (`cs_xtask::test_layout`) against this
//! workspace and against throwaway fixture workspaces, and the `test-layout`
//! command end to end:
//!
//! * the guard passes on this workspace and names `crates/cs_types` as
//!   migrated, so the check is not vacuous;
//! * a temporary `crates/cs_types/tests/stray.rs` really makes it fail —
//!   naming the stray file and the `tests/main.rs` to declare it in — and the
//!   file is removed again, because `cargo test --workspace` runs this test;
//! * a fixture's stray top-level file and a former directory-style target
//!   (`tests/campaign/main.rs`) are both found, while a declared directory
//!   module (`mod declared;` -> `tests/declared/mod.rs`) is reached;
//! * a member that has not migrated is left alone;
//! * a member that sets `autotests = false` without exactly one `[[test]]`
//!   target, or whose target names a file that is not there, is reported;
//! * `#[path]`, inline `mod outer { … }` scopes, comments and string literals
//!   resolve the way rustc resolves them, so a declared file is not called
//!   stray and a file mentioned only in prose is not called declared.
//!
//! Removing the guard — or making it return a clean report unconditionally —
//! fails every one of them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use cs_xtask::test_layout::{self, LayoutFinding, LayoutReport};
use cs_xtask::transient;

/// Held by every test that scans this workspace's real `tests/` directories,
/// and for the whole of the test that temporarily adds `tests/stray.rs` to one,
/// so a scan can never see that file. Without it the two run on parallel test
/// threads and the scan reports the temporary file as a real stray.
static REAL_TREE: Mutex<()> = Mutex::new(());

/// Takes [`REAL_TREE`]; a test that panicked while holding it does not poison
/// the others.
pub(crate) fn real_tree_lock() -> MutexGuard<'static, ()> {
    REAL_TREE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Scratch space unique to this test process, inside the gitignored `target/`,
/// matching the pid-keyed layout of the sibling suites so overlapping runs
/// cannot delete each other's fixtures.
fn scratch(name: &str) -> PathBuf {
    let dir = workspace_root().join(format!(
        "target/test-bin-guard-fixtures/{}/{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the scratch directory must be creatable");
    dir
}

/// A throwaway workspace with one member named by `manifest`, whose files
/// (`("tests/main.rs", "…")` …) are written under it.
fn fixture(name: &str, member: &str, manifest: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = scratch(name);
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nresolver = \"3\"\nmembers = [\n  \"{member}\",\n]\n"),
    )
    .expect("the fixture workspace manifest must be writable");
    let dir = root.join(member);
    fs::create_dir_all(dir.join("src")).expect("the member src dir must be creatable");
    fs::write(dir.join("src/lib.rs"), "// crate\n").expect("the member lib must be writable");
    fs::write(dir.join("Cargo.toml"), manifest).expect("the member manifest must be writable");
    for (path, contents) in files {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().expect("a fixture file has a parent"))
            .expect("the fixture directory must be creatable");
        fs::write(&file, contents).expect("the fixture file must be writable");
    }
    root
}

/// A member that has migrated: one test binary, `tests/main.rs`.
const MIGRATED: &str = "\
[package]
name = \"guard-fixture\"
version = \"0.0.0\"
edition = \"2024\"
autotests = false

[[test]]
name = \"guard_fixture_tests\"
path = \"tests/main.rs\"
";

/// Whether the report names `needle` in any finding's message.
fn names_finding(report: &LayoutReport, needle: &str) -> bool {
    report.lines().iter().any(|line| line.contains(needle))
}

/// The stray files the report names, sorted.
fn stray_files(report: &LayoutReport) -> Vec<String> {
    let mut files: Vec<String> = report
        .findings
        .iter()
        .filter_map(|finding| match finding {
            LayoutFinding::StrayTestFile { file, .. } => Some(file.clone()),
            _ => None,
        })
        .collect();
    files.sort();
    files
}

/// A file that removes itself when the test ends, pass or panic, so a failed
/// run cannot leave a stray test source behind for the next one.
struct TemporaryFile {
    path: PathBuf,
}

impl TemporaryFile {
    fn create(path: &Path, contents: &str) -> Self {
        // A run killed outright can leave the file behind; clearing it first
        // keeps that from turning into a mystery finding for the next agent.
        let _ = fs::remove_file(path);
        fs::write(path, contents).expect("the temporary file must be writable");
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// The guard's real job, on the real tree: this workspace is clean, a migrated
/// crate is found (so the check is not vacuous), adding `tests/stray.rs` to
/// that crate makes it fail naming the file and the `main.rs` to add it to,
/// and removing the file makes it pass again.
///
/// This is the test CI's `cargo test --workspace` uses to enforce the
/// convention, and it is deliberately one test: the temporary file must be the
/// only one in the tree while it exists.
#[test]
fn accept_test_bin_guard_fails_on_a_temporary_stray_file_and_passes_on_this_workspace() {
    let _tree = real_tree_lock();
    let root = workspace_root();

    let report =
        test_layout::check_workspace(&root).expect("this workspace must be readable by the guard");
    assert!(
        report.is_clean(),
        "this workspace's test layout must be clean, got {:?}",
        report.lines()
    );
    assert!(
        report.members >= 10,
        "the guard must check every workspace member, saw {}",
        report.members
    );
    let migrated = report
        .migrated
        .iter()
        .find(|migrated| migrated.member == "crates/cs_types")
        .expect("cs_types must be a migrated crate, or this test proves nothing");
    assert_eq!(
        migrated.main, "crates/cs_types/tests/main.rs",
        "the migrated crate's single test binary is its declared [[test]] target"
    );
    assert_eq!(
        migrated.reachable.len(),
        7,
        "tests/main.rs plus its six former targets must all be reached, got {:?}",
        migrated.reachable
    );

    let stray = root.join("crates/cs_types/tests/stray.rs");
    let guard = TemporaryFile::create(&stray, "// a test source nothing declares\n");

    let report = test_layout::check_workspace(&root)
        .expect("this workspace must stay readable with the temporary file");
    assert!(
        !report.is_clean(),
        "an undeclared tests/*.rs in a migrated crate must fail the guard, got {:?}",
        report.lines()
    );
    assert_eq!(
        stray_files(&report),
        vec!["crates/cs_types/tests/stray.rs".to_string()],
        "the guard must name the stray file and nothing else, got {:?}",
        report.findings
    );
    assert!(
        names_finding(&report, "crates/cs_types/tests/main.rs"),
        "the finding must name the main.rs to declare it in, got {:?}",
        report.lines()
    );

    drop(guard);
    assert!(
        !stray.exists(),
        "the temporary stray file must be gone before the next check"
    );
    let report = test_layout::check_workspace(&root)
        .expect("this workspace must be readable after the temporary file is removed");
    assert!(
        report.is_clean(),
        "removing the stray file must make the guard pass again, got {:?}",
        report.lines()
    );
}

/// The failure the convention exists for: a migrated crate's former target is
/// no longer declared, and a file module in a directory that is not declared
/// either. A declared directory module (`mod declared;` -> `tests/declared/
/// mod.rs`) and a file reached through `#[path]` are *not* findings.
#[test]
fn accept_test_bin_guard_finds_every_unreached_test_source() {
    let root = fixture(
        "strays",
        "crates/migrated",
        MIGRATED,
        &[
            (
                "tests/main.rs",
                "mod declared;\n#[path = \"elsewhere/redirected.rs\"]\nmod renamed;\n",
            ),
            (
                "tests/declared/mod.rs",
                "// reached as a directory module\n",
            ),
            (
                "tests/elsewhere/redirected.rs",
                "// reached through #[path]\n",
            ),
            ("tests/stray.rs", "// not declared\n"),
            (
                "tests/campaign/main.rs",
                "// a former directory-style target\n",
            ),
            ("tests/not_a_module.txt", "// not Rust, not a test source\n"),
        ],
    );

    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert_eq!(
        stray_files(&report),
        vec![
            "crates/migrated/tests/campaign/main.rs".to_string(),
            "crates/migrated/tests/stray.rs".to_string(),
        ],
        "exactly the unreached sources must be reported, got {:?}",
        report.findings
    );
    assert!(
        names_finding(&report, "crates/migrated/tests/main.rs"),
        "each finding must name the main.rs to add the module to, got {:?}",
        report.lines()
    );
    assert!(
        report
            .migrated
            .iter()
            .any(|migrated| migrated.member == "crates/migrated"),
        "a crate with strays is still a migrated crate, got {:?}",
        report.migrated
    );
}

/// `#[path]` on a file module declared by a plain (non-`mod.rs`) file is
/// relative to that file's own directory, as rustc resolves it — not to the
/// `<dir>/<file stem>` directory its inline blocks use. `cs_app`'s
/// `tests/accept_f29_c_damage_consumers.rs` shares `tests/damage/*.rs` this way
/// (task #1543).
#[test]
fn accept_test_bin_guard_resolves_path_attributes_beside_a_plain_file() {
    let root = fixture(
        "path-beside-plain-file",
        "crates/migrated",
        MIGRATED,
        &[
            ("tests/main.rs", "mod consumer;\n"),
            (
                "tests/consumer.rs",
                "#[path = \"shared/helper.rs\"]\nmod helper;\n",
            ),
            ("tests/shared/helper.rs", "// reached through #[path]\n"),
            (
                "tests/consumer/shared/helper.rs",
                "// rustc never looks here\n",
            ),
        ],
    );

    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert_eq!(
        stray_files(&report),
        vec!["crates/migrated/tests/consumer/shared/helper.rs".to_string()],
        "only the file rustc never reaches is stray, got {:?}",
        report.findings
    );
}

/// A member that has not migrated keeps its one-binary-per-file layout and is
/// left alone: auto-discovered files are each their own target, not strays.
#[test]
fn accept_test_bin_guard_leaves_unmigrated_crates_alone() {
    let manifest = "\
[package]
name = \"guard-fixture\"
version = \"0.0.0\"
edition = \"2024\"
";
    let root = fixture(
        "unmigrated",
        "crates/plain",
        manifest,
        &[
            ("tests/accept_one.rs", "// its own binary\n"),
            ("tests/accept_two.rs", "// its own binary\n"),
            ("tests/campaign/main.rs", "// its own binary\n"),
        ],
    );

    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert!(
        report.is_clean(),
        "a crate without autotests = false must not be reported, got {:?}",
        report.lines()
    );
    assert!(
        report.migrated.is_empty(),
        "nothing has migrated in this fixture, got {:?}",
        report.migrated
    );
}

/// `autotests = false` with no single `[[test]]` target: every test source
/// would silently stop running (no target at all), or the one-binary promise
/// is false (two targets).
#[test]
fn accept_test_bin_guard_reports_a_missing_or_multiple_test_target() {
    let no_target = "\
[package]
name = \"guard-fixture\"
version = \"0.0.0\"
edition = \"2024\"
autotests = false
";
    let root = fixture(
        "no-target",
        "crates/plain",
        no_target,
        &[("tests/accept_one.rs", "// would never run\n")],
    );
    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert!(
        matches!(
            report.findings.as_slice(),
            [LayoutFinding::TestTargetCount { member, targets: 0, sources: 1, .. }]
                if member == "crates/plain"
        ),
        "a migrated crate with no [[test]] target must be reported, got {:?}",
        report.findings
    );

    let two_targets = "\
[package]
name = \"guard-fixture\"
version = \"0.0.0\"
edition = \"2024\"
autotests = false

[[test]]
name = \"one\"
path = \"tests/main.rs\"

[[test]]
name = \"two\"
path = \"tests/main.rs\"
";
    let root = fixture(
        "two-targets",
        "crates/twice",
        two_targets,
        &[("tests/main.rs", "// one binary, per the convention\n")],
    );
    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert!(
        matches!(
            report.findings.as_slice(),
            [LayoutFinding::TestTargetCount { targets: 2, .. }]
        ),
        "two [[test]] targets contradict the convention, got {:?}",
        report.findings
    );

    let missing = "\
[package]
name = \"guard-fixture\"
version = \"0.0.0\"
edition = \"2024\"
autotests = false

[[test]]
name = \"guard_fixture_tests\"
path = \"tests/main.rs\"
";
    let root = fixture(
        "missing-target",
        "crates/gone",
        missing,
        &[("tests/accept_one.rs", "// present but unreferenced\n")],
    );
    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert!(
        matches!(
            report.findings.as_slice(),
            [LayoutFinding::MissingTestTarget { member, main }]
                if member == "crates/gone" && main == "crates/gone/tests/main.rs"
        ),
        "a declared target whose file is gone must be reported, got {:?}",
        report.findings
    );
    assert!(
        names_finding(&report, "crates/gone/tests/main.rs"),
        "the finding must name the missing target, got {:?}",
        report.lines()
    );
}

/// A crate may declare more than one `[[test]]` target only through an
/// explicit `[package.metadata.cs_test_layout] test_targets = <n>` allowance
/// in its own manifest (task #1543: cs_app cannot link one binary under
/// macOS's 16 MiB `__eh_frame` limit). The guard then requires exactly that
/// many targets, every file reached, and no file reached by two targets.
#[test]
fn accept_test_bin_guard_allows_a_declared_number_of_targets_and_no_file_twice() {
    let manifest = |allowance: &str, targets: &[(&str, &str)]| {
        let mut text = String::from(
            "[package]\nname = \"guard-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\nautotests = false\n",
        );
        text.push_str(allowance);
        for (name, path) in targets {
            text.push_str(&format!(
                "\n[[test]]\nname = \"{name}\"\npath = \"{path}\"\n"
            ));
        }
        text
    };
    let allowance = "\n[package.metadata.cs_test_layout]\ntest_targets = 2\n";
    let both = [("a", "tests/a/main.rs"), ("b", "tests/b/main.rs")];

    // Two targets, each file in exactly one of them: clean, two binaries.
    let clean = fixture(
        "allowance-clean",
        "crates/split",
        &manifest(allowance, &both),
        &[
            ("tests/a/main.rs", "mod one;\n"),
            ("tests/a/one.rs", "// a\n"),
            ("tests/b/main.rs", "mod two;\n"),
            ("tests/b/two.rs", "// b\n"),
        ],
    );
    let report = test_layout::check_workspace(&clean).expect("the fixture must be readable");
    assert!(report.is_clean(), "{:?}", report.lines());
    assert_eq!(report.binaries(), 2);
    assert_eq!(report.migrated[0].mains.len(), 2);

    // A file declared by both targets is compiled and run twice.
    let twice = fixture(
        "allowance-twice",
        "crates/split",
        &manifest(allowance, &both),
        &[
            (
                "tests/a/main.rs",
                "#[path = \"../shared.rs\"]\nmod shared;\n",
            ),
            (
                "tests/b/main.rs",
                "#[path = \"../shared.rs\"]\nmod shared;\n",
            ),
            ("tests/shared.rs", "// both\n"),
        ],
    );
    let report = test_layout::check_workspace(&twice).expect("the fixture must be readable");
    assert!(
        matches!(
            report.findings.as_slice(),
            [LayoutFinding::DoublyReachedTestFile { file, mains, .. }]
                if file == "crates/split/tests/shared.rs" && mains.len() == 2
        ),
        "a file reached from two targets must be reported, got {:?}",
        report.findings
    );

    // A listed shared helper file with no tests may be reached by both
    // targets; one that holds a `#[test]` may not, or both would run it.
    let shared_allowance = "\n[package.metadata.cs_test_layout]\ntest_targets = 2\nshared_test_sources = [\"tests/common.rs\"]\n";
    let shared_files = |common: &'static str| {
        [
            (
                "tests/a/main.rs",
                "#[path = \"../common.rs\"]\nmod common;\n",
            ),
            (
                "tests/b/main.rs",
                "#[path = \"../common.rs\"]\nmod common;\n",
            ),
            ("tests/common.rs", common),
        ]
    };
    let root = fixture(
        "allowance-shared",
        "crates/split",
        &manifest(shared_allowance, &both),
        &shared_files("pub fn helper() {}\n"),
    );
    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert!(report.is_clean(), "{:?}", report.lines());
    let root = fixture(
        "allowance-shared-with-test",
        "crates/split",
        &manifest(shared_allowance, &both),
        &shared_files("#[test]\nfn runs_twice() {}\n"),
    );
    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert!(
        matches!(
            report.findings.as_slice(),
            [LayoutFinding::SharedTestSourceHasTests { file, .. }]
                if file == "crates/split/tests/common.rs"
        ),
        "a shared file holding a test must be reported, got {:?}",
        report.findings
    );

    // A file no target reaches is still stray.
    let stray = fixture(
        "allowance-stray",
        "crates/split",
        &manifest(allowance, &both),
        &[
            ("tests/a/main.rs", "// a\n"),
            ("tests/b/main.rs", "// b\n"),
            ("tests/lost.rs", "// nobody\n"),
        ],
    );
    let report = test_layout::check_workspace(&stray).expect("the fixture must be readable");
    assert_eq!(stray_files(&report), ["crates/split/tests/lost.rs"]);

    // The allowance is exact: fewer or more targets than allowed is reported,
    // and without an allowance two targets stay a violation.
    for (name, allowance, targets) in [
        ("allowance-short", allowance, &both[..1]),
        ("allowance-none", "", &both[..]),
    ] {
        let root = fixture(
            name,
            "crates/split",
            &manifest(allowance, targets),
            &[("tests/a/main.rs", "// a\n"), ("tests/b/main.rs", "// b\n")],
        );
        let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
        assert!(
            matches!(
                report.findings.as_slice(),
                [LayoutFinding::TestTargetCount { .. }]
            ),
            "{name}: the target count must match the allowance, got {:?}",
            report.findings
        );
    }
}

/// The module rules are rustc's: an inline module's file modules hang one
/// directory deeper, a `mod` inside a comment or a string is not a
/// declaration, and a module file reached from a nested file is reached.
#[test]
fn accept_test_bin_guard_resolves_modules_the_way_rustc_does() {
    let root = fixture(
        "module-rules",
        "crates/migrated",
        MIGRATED,
        &[
            (
                "tests/main.rs",
                "//! `mod prose;` in a doc comment declares nothing.\n\
                 mod outer {\n    mod nested;\n}\n\
                 mod real;\n\
                 let fixture_manifest = \"autotests = false\\nmod quoted;\\n\";\n\
                 /* mod block_comment; */\n",
            ),
            ("tests/outer/nested.rs", "// inline module scope\n"),
            ("tests/real.rs", "mod deeper;\n"),
            ("tests/real/deeper.rs", "// nested module file\n"),
            ("tests/prose.rs", "// only mentioned in a comment\n"),
            ("tests/quoted.rs", "// only mentioned in a string\n"),
            (
                "tests/block_comment.rs",
                "// only mentioned in a block comment\n",
            ),
        ],
    );

    let report = test_layout::check_workspace(&root).expect("the fixture must be readable");
    assert_eq!(
        stray_files(&report),
        vec![
            "crates/migrated/tests/block_comment.rs".to_string(),
            "crates/migrated/tests/prose.rs".to_string(),
            "crates/migrated/tests/quoted.rs".to_string(),
        ],
        "only files no declaration reaches may be reported, got {:?}",
        report.findings
    );
}

/// The command agents run: `cs_xtask test-layout` reports the checked workspace
/// and exits 0 when it is clean, exits 1 naming the stray file when it is not,
/// and treats an unusable workspace root as a gate failure rather than a
/// silent pass.
#[test]
fn accept_test_bin_guard_command_reports_the_layout() {
    let root = workspace_root();
    let bin = env!("CARGO_BIN_EXE_cs_xtask");

    let clean = fixture(
        "command-clean",
        "crates/migrated",
        MIGRATED,
        &[
            ("tests/main.rs", "mod accept_one;\n"),
            ("tests/accept_one.rs", "// reached\n"),
        ],
    );
    let output = transient::command_output(
        Command::new(bin)
            .args(["test-layout", "--workspace-root"])
            .arg(&clean),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        output.status.code(),
        Some(0),
        "a clean layout must pass; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("crates/migrated links one test binary from crates/migrated/tests/main.rs"),
        "the command must report the migrated crate, got: {stdout:?}"
    );
    assert!(
        stdout.contains("no `.rs` file under a migrated crate's tests/ is unreached"),
        "the command must say what it checked, got: {stdout:?}"
    );

    let dirty = fixture(
        "command-dirty",
        "crates/migrated",
        MIGRATED,
        &[
            ("tests/main.rs", "// nothing declared\n"),
            ("tests/stray.rs", "// unreached\n"),
        ],
    );
    let output = transient::command_output(
        Command::new(bin)
            .args(["test-layout", "--workspace-root"])
            .arg(&dirty),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        output.status.code(),
        Some(1),
        "a stray test source must fail the gate"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("crates/migrated/tests/stray.rs")
            && stderr.contains("crates/migrated/tests/main.rs"),
        "the failure must name the stray file and the main.rs, got: {stderr:?}"
    );

    let bad_root = transient::command_output(
        Command::new(bin)
            .args(["test-layout", "--workspace-root"])
            .arg(root.join("target/not-a-workspace")),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        bad_root.status.code(),
        Some(1),
        "an unusable workspace root must be a gate failure, not a usage error"
    );
    assert!(
        String::from_utf8_lossy(&bad_root.stderr).contains("not a workspace root"),
        "the failure must explain the root, got: {}",
        String::from_utf8_lossy(&bad_root.stderr)
    );

    let unknown_flag = transient::command_output(
        Command::new(bin)
            .args(["test-layout", "--prefix", "accept_test_bin_guard_"])
            .arg("--workspace-root")
            .arg(&root),
    )
    .expect("the cs_xtask binary must run");
    assert_eq!(
        unknown_flag.status.code(),
        Some(2),
        "an option belonging to another subcommand must be a usage error"
    );
    assert!(
        String::from_utf8_lossy(&unknown_flag.stderr).contains("unknown option"),
        "the usage error must name the option, got: {}",
        String::from_utf8_lossy(&unknown_flag.stderr)
    );
}
