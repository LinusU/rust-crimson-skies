//! The workspace's own share of the CI runner's disk: what `cargo test
//! --workspace` asks the runner to link (task #696).
//!
//! [`crate::budget`] guards the *profile* the CI job links under, because the
//! per-object cost of full DWARF is the workspace's to choose. The other half
//! of the same budget is the *number of binaries*: every `tests/*.rs` file and
//! every `src/lib.rs` / `src/main.rs` unit-test harness is a separate
//! executable the job links and execs, and the engine-linked ones are large.
//! Measured on main at 385160c2 (run 37394319899, `rust` job), the job starts
//! with 124 GB free after the owner's toolchain cleanup and ends `cargo test`
//! with 20 GB free, so it writes about 104 GB; one commit earlier
//! (73f84b1c, run 37386273061) the same job ended with 450 MB free. The
//! per-binary cost is therefore the number that decides whether the next test
//! file fits, and the only way to learn it is to measure a built tree.
//!
//! This module is that measurement, and it is deliberately a *report* rather
//! than a gate:
//!
//! * It counts the test binaries in the plan from the manifests and the
//!   filesystem, honouring `test = false` on a target's own table the way
//!   `tools/cs_xtask/Cargo.toml` uses it (task #610), so a crate that has no
//!   unit tests does not spend a runner byte on an empty harness.
//! * It measures each built binary by reading the `.d` sidecar cargo writes
//!   next to it, which names the source file that produced it. Nothing is
//!   matched on the binary's name, so two crates with equally named test files
//!   stay apart, and an absent binary is reported as *unknown* rather than as
//!   zero bytes.
//! * It splits the measured binaries into engine-linked and small ones by a
//!   floor ([`ENGINE_LINKED_FLOOR`]) and prints both group sizes, so a tree
//!   whose sizes do not fall into the two groups is visible in the output
//!   instead of silently moving the median.
//!
//! What it does not count is stated rather than guessed: `cargo test` also
//! links one binary per doc-test code block, and counting those needs rustdoc's
//! own parse. The plan this module returns is the set of *test harness*
//! binaries, so the total it reports is a floor for the runner's write, never
//! an upper bound.
//!
//! The measurements behind the numbers quoted here are in
//! `docs/findings/2026-10-06-t696-ci-runner-disk-budget.md`.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::bootstrap;
use crate::transient;

/// Smallest measured binary treated as engine-linked.
///
/// Measured on this workspace (macOS aarch64, `dev` profile, full DWARF): a
/// test binary that links the Bevy graph is 104.8 MB to 233.3 MB, and one that
/// does not is 1.1 MB to 9.0 MB. Nothing lands between those groups, so
/// [`ENGINE_LINKED_FLOOR`] separates them on the measured tree.
pub const ENGINE_LINKED_FLOOR: u64 = 50_000_000;

/// Which test target a measured binary belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetKind {
    /// One `tests/<name>.rs` or `tests/<name>/main.rs` file: cargo links one
    /// executable per file.
    Integration,
    /// The `src/lib.rs` or `src/main.rs` unit-test harness, unless the target's
    /// own manifest table sets `test = false`.
    UnitHarness,
}

impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integration => write!(f, "integration"),
            Self::UnitHarness => write!(f, "unit harness"),
        }
    }
}

/// One executable `cargo test --workspace` links and runs for this workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestTarget {
    /// Workspace member that owns the target, e.g. `crates/cs_app`.
    pub member: String,
    /// Name to report the target under: the test file's stem, or the harness
    /// source file.
    pub name: String,
    /// Source file that defines the target, relative to the workspace root.
    /// This is also what the measurement matches on.
    pub source: String,
    /// Integration test or unit-test harness.
    pub kind: TargetKind,
    /// Size of the largest built binary this target's source produced, or
    /// `None` when this target dir holds none. `None` means *not measured
    /// here*, never *zero bytes*.
    pub bytes: Option<u64>,
    /// How many executables this target's source produced. It is 1 for an
    /// integration test and for a `src/lib.rs` harness; a `src/main.rs` bin
    /// target is linked twice by `cargo test` — once as the plain binary an
    /// integration test can exec, once as its own test harness — so this
    /// reports the largest of them rather than pretending there was only one.
    pub binaries: usize,
}

impl TestTarget {
    /// Whether this target's binary was measured at or above
    /// [`ENGINE_LINKED_FLOOR`].
    pub fn is_engine_linked(&self) -> bool {
        self.bytes.is_some_and(|bytes| bytes >= ENGINE_LINKED_FLOOR)
    }
}

/// The plan and its measurements, per member and in total.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footprint {
    /// Every test binary in the plan, ordered by member then source path.
    pub targets: Vec<TestTarget>,
}

impl Footprint {
    /// Number of test binaries in the plan, measured or not.
    pub fn planned(&self) -> usize {
        self.targets.len()
    }

    /// How many of the planned binaries are unit-test harnesses.
    pub fn unit_harness_count(&self) -> usize {
        self.targets
            .iter()
            .filter(|t| t.kind == TargetKind::UnitHarness)
            .count()
    }

    /// Targets whose binary was measured in this target dir.
    pub fn measured(&self) -> impl Iterator<Item = &TestTarget> {
        self.targets.iter().filter(|t| t.bytes.is_some())
    }

    /// Targets this target dir holds no binary for. A target that has never
    /// been built here is listed, never dropped.
    pub fn unmeasured(&self) -> impl Iterator<Item = &TestTarget> {
        self.targets.iter().filter(|t| t.bytes.is_none())
    }

    /// Measured binaries at or above [`ENGINE_LINKED_FLOOR`], smallest first.
    pub fn engine_linked(&self) -> Vec<&TestTarget> {
        let mut heavy: Vec<&TestTarget> =
            self.measured().filter(|t| t.is_engine_linked()).collect();
        heavy.sort_by_key(|t| t.bytes);
        heavy
    }

    /// Measured binaries below [`ENGINE_LINKED_FLOOR`], smallest first.
    pub fn small(&self) -> Vec<&TestTarget> {
        let mut small: Vec<&TestTarget> =
            self.measured().filter(|t| !t.is_engine_linked()).collect();
        small.sort_by_key(|t| t.bytes);
        small
    }

    /// Sum of every measured binary. Reports only the binaries it measured;
    /// [`unmeasured`](Self::unmeasured) carries the rest, so this is a floor
    /// for the tree and never a total for an unbuilt target.
    pub fn measured_bytes(&self) -> u64 {
        self.measured().map(|t| t.bytes.unwrap_or_default()).sum()
    }

    /// What one more engine-linked test file costs: the median of the measured
    /// engine-linked binaries. `None` when nothing engine-linked has been
    /// measured, which is reported as unknown rather than as zero.
    pub fn marginal_bytes(&self) -> Option<u64> {
        let heavy = self.engine_linked();
        if heavy.is_empty() {
            return None;
        }
        Some(heavy[heavy.len() / 2].bytes.unwrap_or_default())
    }

    /// The largest measured binary, or `None` when nothing was measured.
    pub fn largest(&self) -> Option<&TestTarget> {
        self.measured().max_by_key(|t| t.bytes)
    }

    /// Per-member totals, in member order.
    pub fn by_member(&self) -> Vec<MemberFootprint> {
        let mut members: Vec<&str> = self.targets.iter().map(|t| t.member.as_str()).collect();
        members.sort_unstable();
        members.dedup();
        members
            .into_iter()
            .map(|member| {
                let targets: Vec<&TestTarget> =
                    self.targets.iter().filter(|t| t.member == member).collect();
                MemberFootprint {
                    member: member.to_string(),
                    planned: targets.len(),
                    measured: targets.iter().filter(|t| t.bytes.is_some()).count(),
                    measured_bytes: targets.iter().filter_map(|t| t.bytes).sum(),
                }
            })
            .collect()
    }
}

/// One member's share of the plan and of the measured bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberFootprint {
    /// Workspace member path, e.g. `crates/cs_app`.
    pub member: String,
    /// Test binaries this member contributes to the plan.
    pub planned: usize,
    /// How many of them this target dir holds a binary for.
    pub measured: usize,
    /// Sum of those binaries' measured sizes.
    pub measured_bytes: u64,
}

/// Why the footprint cannot be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FootprintError {
    /// A manifest or a `tests` directory could not be read.
    Io { path: String },
    /// A member's manifest could not be read or understood.
    Members(bootstrap::BootstrapError),
}

impl fmt::Display for FootprintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path } => write!(f, "cannot read {path}"),
            Self::Members(error) => write!(f, "workspace members check failed: {error}"),
        }
    }
}

impl std::error::Error for FootprintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Members(error) => Some(error),
            Self::Io { .. } => None,
        }
    }
}

/// The integration test targets under `member`'s `tests` directory.
///
/// Both layouts cargo accepts count: `tests/<name>.rs` and
/// `tests/<name>/main.rs`. The result is sorted by source path, and a
/// directory without `main.rs` contributes nothing because cargo builds no
/// target for it.
pub fn integration_test_targets(
    workspace_root: &Path,
    member: &str,
) -> Result<Vec<TestTarget>, FootprintError> {
    let tests = workspace_root.join(member).join("tests");
    if !transient::is_dir(&tests, transient::SCAN) {
        return Ok(Vec::new());
    }
    let entries = transient::read_dir(&tests, transient::SCAN).map_err(|_| FootprintError::Io {
        path: tests.display().to_string(),
    })?;

    let mut found: Vec<TestTarget> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| FootprintError::Io {
            path: tests.display().to_string(),
        })?;
        let file_type =
            transient::file_type(&entry, transient::SCAN).map_err(|_| FootprintError::Io {
                path: entry.path().display().to_string(),
            })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            let main = tests.join(&name).join("main.rs");
            if transient::is_file(&main, transient::SCAN) {
                found.push(target(
                    member,
                    &name,
                    format!("{member}/tests/{name}/main.rs"),
                    TargetKind::Integration,
                ));
            }
            continue;
        }
        let Some(stem) = name.strip_suffix(".rs") else {
            continue;
        };
        found.push(target(
            member,
            stem,
            format!("{member}/tests/{name}"),
            TargetKind::Integration,
        ));
    }
    found.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(found)
}

/// The unit-test harness targets of `member`, minus the ones its manifest
/// turns off.
///
/// `test = false` in `[lib]` or in an `[[bin]]` table is how `cs_xtask` keeps
/// two empty harnesses out of the plan (task #610); a harness that is turned
/// off is not part of the plan and so not part of the runner's write.
pub fn unit_harness_targets(
    workspace_root: &Path,
    member: &str,
    manifest: &str,
) -> Result<Vec<TestTarget>, FootprintError> {
    let mut found = Vec::new();
    if transient::is_file(
        &workspace_root.join(member).join("src").join("lib.rs"),
        transient::SCAN,
    ) && lib_harness_is_linked(manifest)
    {
        found.push(target(
            member,
            "src/lib.rs",
            format!("{member}/src/lib.rs"),
            TargetKind::UnitHarness,
        ));
    }
    if transient::is_file(
        &workspace_root.join(member).join("src").join("main.rs"),
        transient::SCAN,
    ) && bin_harness_is_linked(manifest, "src/main.rs")
    {
        found.push(target(
            member,
            "src/main.rs",
            format!("{member}/src/main.rs"),
            TargetKind::UnitHarness,
        ));
    }
    Ok(found)
}

/// Whether `[lib]` leaves its unit-test harness in the plan.
pub fn lib_harness_is_linked(manifest: &str) -> bool {
    table_flag(manifest, "[lib]", "test").unwrap_or(true)
}

/// One `[[bin]]` table, as far as the harness decision needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BinTable {
    path: Option<String>,
    test: Option<bool>,
}

/// Every `[[bin]]` table in `manifest`, in order.
fn bin_tables(manifest: &str) -> Vec<BinTable> {
    let mut tables: Vec<BinTable> = Vec::new();
    let mut inside = false;
    for raw in manifest.lines() {
        let line = strip_comment(raw.trim());
        if line == "[[bin]]" {
            tables.push(BinTable {
                path: None,
                test: None,
            });
            inside = true;
            continue;
        }
        if line.starts_with('[') {
            inside = false;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(table) = tables.last_mut() else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "path" => table.path = Some(value.to_string()),
            "test" => table.test = Some(value == "true"),
            _ => {}
        }
    }
    tables
}

/// Whether the `[[bin]]` table whose `path` is `bin_path` leaves its
/// unit-test harness in the plan.
///
/// A table that states no `path` is `src/main.rs`, which is what cargo assumes,
/// and a member with no `[[bin]]` table at all links that harness by default.
pub fn bin_harness_is_linked(manifest: &str, bin_path: &str) -> bool {
    bin_tables(manifest)
        .into_iter()
        .find(|table| table.path.as_deref().unwrap_or("src/main.rs") == bin_path)
        .and_then(|table| table.test)
        .unwrap_or(true)
}

/// The boolean value of `key` in `header`, if the table states one.
fn table_flag(manifest: &str, header: &str, key: &str) -> Option<bool> {
    let mut inside = false;
    for raw in manifest.lines() {
        let line = strip_comment(raw.trim());
        if line.starts_with('[') {
            inside = line == header;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((found, value)) = line.split_once('=') else {
            continue;
        };
        if found.trim() == key {
            return Some(unquote(value.trim()) == "true");
        }
    }
    None
}

fn target(member: &str, name: &str, source: String, kind: TargetKind) -> TestTarget {
    TestTarget {
        member: member.to_string(),
        name: name.to_string(),
        source,
        kind,
        bytes: None,
        binaries: 0,
    }
}

/// Measures the binary `target` produced in `deps_dir`, if it is there.
///
/// The match is made on the `.d` sidecar cargo writes beside every binary,
/// which names the source file that produced it, so two members with equally
/// named test files stay apart and a stale binary from an older build of the
/// same file is still the one that describes the file. Returns `None` when the
/// target dir holds no binary for the target: an unmeasured target is unknown,
/// not zero.
pub fn measure(target: &TestTarget, deps_dir: &Path) -> Option<Measured> {
    let entries = transient::read_dir(deps_dir, transient::SCAN).ok()?;
    let mut measured: Option<Measured> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".d") else {
            continue;
        };
        let binary = deps_dir.join(stem);
        if !transient::is_file(&binary, transient::SCAN) {
            continue;
        }
        let Ok(text) = transient::read_to_string(&deps_dir.join(&name), transient::SCAN) else {
            continue;
        };
        if !dep_file_root(&text).is_some_and(|token| root_is(token, &target.source)) {
            continue;
        }
        if let Ok(metadata) = transient::metadata(&binary, transient::SCAN) {
            // A dir entry list is unordered and an older build of the same
            // source can sit beside a newer one, so every binary of this source
            // counts and the largest is the one a new build would reproduce.
            measured = Some(match measured {
                None => Measured {
                    bytes: metadata.len(),
                    binaries: 1,
                },
                Some(previous) => Measured {
                    bytes: previous.bytes.max(metadata.len()),
                    binaries: previous.binaries + 1,
                },
            });
        }
    }
    measured
}

/// What a target dir holds for one test target: its largest binary and how
/// many binaries that source produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measured {
    /// Size of the largest binary the source produced.
    pub bytes: u64,
    /// How many executables the source produced.
    pub binaries: usize,
}

/// The source file a cargo `.d` file was built from: the first dependency on
/// its `Makefile`-style rule line.
///
/// The *root* dependency is what identifies a target. Every dependency is not:
/// a unit-test harness's rule line also names the module sources its crate is
/// made of, so `tools/cs_inspect`'s `src/lib.rs` rule line names
/// `src/main.rs` too and a match on any dependency would measure the lib
/// harness when asked for the bin's.
///
/// Cargo writes the path relative to the workspace root for a target built
/// inside it and absolute for one built from elsewhere, so an absolute tail is
/// accepted as the same root.
fn dep_file_root(text: &str) -> Option<&str> {
    let rule = text.split_once(':')?.1;
    rule.split_whitespace().next()
}

/// Whether a root dependency `token` from a `.d` file is `source`, the
/// workspace-relative path.
///
/// Cargo writes the root relative to the workspace root for a target built
/// inside it and absolute for one built from elsewhere, so an absolute tail is
/// the same root. Windows separators are folded because the same tree can be
/// read on either.
fn root_is(token: &str, source: &str) -> bool {
    let token = token.replace('\\', "/");
    token == source || token.ends_with(&format!("/{source}"))
}

/// The whole workspace's test-binary plan, with the binaries `deps_dir` holds
/// measured.
///
/// `deps_dir` is a target directory's `debug/deps` directory. A workspace
/// member that cannot be read is an error; a target that is not built there is
/// reported unmeasured.
pub fn measure_workspace(
    workspace_root: &Path,
    deps_dir: &Path,
) -> Result<Footprint, FootprintError> {
    let manifest_path = workspace_root.join(budget_manifest());
    let manifest = transient::read_to_string(&manifest_path, transient::PATIENT).map_err(|_| {
        FootprintError::Io {
            path: manifest_path.display().to_string(),
        }
    })?;
    let members = bootstrap::workspace_members(&manifest).map_err(FootprintError::Members)?;

    let mut targets = Vec::new();
    for member in &members {
        let member_manifest = workspace_root.join(member).join("Cargo.toml");
        let text =
            transient::read_to_string(&member_manifest, transient::PATIENT).map_err(|_| {
                FootprintError::Io {
                    path: member_manifest.display().to_string(),
                }
            })?;
        let mut found = integration_test_targets(workspace_root, member)?;
        found.extend(unit_harness_targets(workspace_root, member, &text)?);
        targets.extend(found);
    }
    targets.sort_by(|a, b| a.source.cmp(&b.source));
    for target in &mut targets {
        if let Some(measured) = measure(target, deps_dir) {
            target.bytes = Some(measured.bytes);
            target.binaries = measured.binaries;
        }
    }
    Ok(Footprint { targets })
}

/// The workspace manifest's path relative to the workspace root. Shared with
/// [`crate::budget`] so both tools read the same file.
fn budget_manifest() -> &'static str {
    crate::budget::MANIFEST_PATH
}

/// Drops a trailing TOML comment from an already-trimmed line.
fn strip_comment(line: &str) -> &str {
    line.split_once('#')
        .map_or(line, |(head, _)| head)
        .trim_end()
}

/// Strips one layer of TOML string quoting.
fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim()
}

/// The target directory a local build of `workspace_root` used, honouring
/// `CARGO_TARGET_DIR` and falling back to `<root>/target`. Returns
/// `<target dir>/debug/deps`, where cargo puts the test binaries.
pub fn default_deps_dir(workspace_root: &Path) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| workspace_root.join("target"));
    target.join("debug").join("deps")
}
