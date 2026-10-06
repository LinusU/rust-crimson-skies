//! Task #430: keep the CI job's build footprint inside the runner's disk.
//!
//! The `rust` CI job died on different runners at the same step: linking the
//! merged `cs_app` doctest failed with `collect2: fatal error: ld terminated
//! with signal 7 [Bus error]` inside rust-lld's writer threads. The
//! measurement behind it (run 36736183776) is that the root filesystem which
//! also holds `/tmp` — where rustdoc links the doctest — had 1.91 GiB free of
//! 144 GiB, 99% used, while `cargo test` ran. A memory-mapped output file the
//! filesystem cannot extend faults with `SIGBUS`; the same tree passed on
//! runners that had more headroom with the identical restored cache.
//!
//! `.github/` is owner-maintained, so the part this workspace owns is the
//! profile CI builds under: rustc's default `debug = 2` puts type, variable
//! and name DWARF in every dependency object (`libbevy_pbr.rlib` is 169.5 MB
//! of which 102.7 MB is `__debug_*`), and that is the footprint that spends
//! the budget. The fix is one line in the workspace manifest,
//! `[profile.dev] debug = "line-tables-only"`, and [`cs_xtask::budget`] is the
//! gate that keeps it there.
//!
//! Evidence, measurements and the `.github/` options left to the owner are in
//! `docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`.

use std::path::{Path, PathBuf};

use cs_xtask::budget::{self, BudgetError};
use cs_xtask::transient;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root must exist")
}

/// The workspace this task changed: reverting `[profile.dev] debug` to
/// rustc's default — deleting the line or setting it to `2` — must fail this
/// gate, so the CI job cannot silently go back to the footprint that ran the
/// runner out of disk.
#[test]
fn accept_t430_this_workspace_keeps_full_dwarf_out_of_the_ci_profiles() {
    if let Err(error) = budget::verify_workspace(&workspace_root()) {
        panic!(
            "{error}\n\
             (gate: cs_xtask verify-ci-budget)"
        );
    }
}

/// The manifest as CI crashed with it: no `[profile.dev] debug` at all, so
/// rustc's default applies to every dependency object. This is the exact
/// revert, and the gate has to name it rather than pass it.
#[test]
fn accept_t430_the_unreduced_profile_ci_crashed_with_is_rejected() {
    let manifest = "\
[workspace]
members = []

[profile.dev.package.\"*\"]
opt-level = 3
";
    assert_eq!(
        budget::profile_debug(manifest, "dev"),
        None,
        "the recorded pre-fix manifest states no [profile.dev] debug level"
    );
    assert_eq!(
        budget::verify_manifest("Cargo.toml", manifest),
        Err(BudgetError::UnstatedDebug {
            file: "Cargo.toml".to_string(),
        })
    );
}

/// A full-DWARF level is rejected in every table CI links under, including
/// the dependency tables where the bytes actually are.
#[test]
fn accept_t430_full_dwarf_is_rejected_wherever_ci_states_it() {
    // `true` is cargo's boolean spelling of `2`, not a reduced level: the same
    // scratch crate built with `debug = true` and with `debug = 2` produced a
    // byte-identical binary. A trailing comment must not smuggle a level past
    // the gate either.
    for (value, recorded) in [
        ("2", "2"),
        ("\"2\"", "\"2\""),
        ("true", "true"),
        ("\"full\"", "\"full\""),
        (" 2 ", "2"),
        ("2 # full type information, needed for the debugger", "2"),
    ] {
        let manifest = format!("[profile.dev]\ndebug = {value}\n");
        assert_eq!(
            budget::verify_manifest("Cargo.toml", &manifest),
            Err(BudgetError::FullDwarf {
                file: "Cargo.toml".to_string(),
                profile: "profile.dev".to_string(),
                value: recorded.to_string(),
            }),
            "debug = {value} must not pass: it is full DWARF"
        );
    }

    // The dependency tables are part of the same profile and hold the bulk.
    let dependency_override = "\
[profile.dev]
debug = \"line-tables-only\"

[profile.dev.package.\"*\"]
debug = 2
opt-level = 3
";
    assert_eq!(
        budget::verify_manifest("Cargo.toml", dependency_override),
        Err(BudgetError::FullDwarf {
            file: "Cargo.toml".to_string(),
            profile: "profile.dev.package.\"*\"".to_string(),
            value: "2".to_string(),
        })
    );

    // And so is a profile that inherits from `dev` and overrides it back.
    let inherited = "\
[profile.dev]
debug = \"line-tables-only\"

[profile.test]
debug = 2
";
    assert_eq!(
        budget::verify_manifest("Cargo.toml", inherited),
        Err(BudgetError::FullDwarf {
            file: "Cargo.toml".to_string(),
            profile: "profile.test".to_string(),
            value: "2".to_string(),
        })
    );
}

/// Every reduced level cargo accepts passes, so the gate does not force one
/// spelling: line numbers in a backtrace are what has to survive, not the
/// particular string.
#[test]
fn accept_t430_reduced_debug_levels_pass_the_budget() {
    for value in [
        "0",
        "false",
        "\"none\"",
        "1",
        "\"limited\"",
        "\"line-tables-only\"",
        "\"line-directives-only\"",
    ] {
        let manifest = format!(
            "[profile.dev]\ndebug = {value}\n\n[profile.dev.package.\"*\"]\nopt-level = 3\n"
        );
        assert_eq!(
            budget::verify_manifest("Cargo.toml", &manifest),
            Ok(()),
            "debug = {value} must stay inside the CI disk budget"
        );
    }
}

/// The committed manifest keeps `file:line` for a panic backtrace: the
/// setting this task relies on is the one that still emits line tables, not
/// one that trades the whole debug section away for a few more megabytes.
#[test]
fn accept_t430_the_workspace_keeps_backtrace_line_numbers() {
    let manifest = std::fs::read_to_string(workspace_root().join(budget::MANIFEST_PATH))
        .expect("the workspace manifest must be readable");
    assert_eq!(
        budget::profile_debug(&manifest, "dev"),
        Some("\"line-tables-only\""),
        "[profile.dev] debug must be \"line-tables-only\": a panic backtrace \
still needs file:line, and full DWARF is what filled the CI runner's disk"
    );
}

/// Name of the helper process that panics on purpose, for
/// [`accept_t430_a_panic_backtrace_names_the_file_and_line`]. It is not an
/// acceptance test itself: it returns immediately unless the parent asks for
/// it, so it carries no `accept_t430_` prefix and nothing selects it by name.
const PANIC_HELPER: &str = "cs_xtask_t430_panic_helper";

/// Environment variable that turns this test binary into the panicking child.
const PANIC_HELPER_ENV: &str = "CS_XTASK_T430_PANIC_HELPER";

/// The process that panics when [`PANIC_HELPER_ENV`] is set, and does nothing
/// at all otherwise. It is the only test in the binary that does.
#[test]
fn cs_xtask_t430_panic_helper() {
    if std::env::var_os(PANIC_HELPER_ENV).is_none() {
        return;
    }
    panic!("cs_xtask task #430: this panic exists to be located in a backtrace");
}

/// The setting above is only worth its megabytes if the binary really keeps
/// `file:line`, so this checks the artifact instead of the manifest string: it
/// runs [`PANIC_HELPER`] in a child process with `RUST_BACKTRACE=1` and
/// requires a backtrace *frame* to name a line of this file.
///
/// Only the frames are read, never the header the panic hook prints for the
/// panic's own location: that location is a string the compiler wrote into the
/// binary, so it would pass even with the line program thrown away. Frames in
/// `std` do not count either: they resolve from the toolchain's prebuilt
/// rlibs whatever this workspace's profile says, so only a frame in this file
/// shows that the workspace's own code kept its line tables. A profile
/// without them prints the helper's frame with no location, and this fails,
/// including when an environment variable overrides the committed profile
/// (`CARGO_PROFILE_DEV_DEBUG=0` reproduces exactly that; see
/// [`line_table_diagnosis`] for what the failure then reports).
#[test]
fn accept_t430_a_panic_backtrace_names_the_file_and_line() {
    let exe = std::env::current_exe().expect("this test binary must have a path to re-run");
    let child = transient::command_output(
        std::process::Command::new(&exe)
            .args(["--exact", PANIC_HELPER, "--nocapture"])
            .env(PANIC_HELPER_ENV, "1")
            .env("RUST_BACKTRACE", "1"),
    )
    .expect("the panic helper must be runnable");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(
        !child.status.success(),
        "the helper process must have panicked, so there is a backtrace to read: {output}"
    );
    let frames = output
        .split_once("stack backtrace:")
        .map_or("", |(_, frames)| frames);
    assert!(
        names_source_location(frames, THIS_FILE),
        "no backtrace frame named a line of {THIS_FILE}, so the test binaries \
CI builds cannot locate a failing test. The committed setting is \
[profile.dev] debug = \"line-tables-only\" (cs_xtask verify-ci-budget).\n\
{}\nThe helper's output was: {output}",
        line_table_diagnosis(&exe)
    );
}

/// The file name the helper's own backtrace frames must point into.
const THIS_FILE: &str = "accept_t430_ci_disk_budget.rs";

/// Whether any whitespace-separated token in `text` is a `.../file:12` style
/// location in `file`, which is what a backtrace frame looks like when the line
/// program survived. The column that may follow the line number is ignored.
fn names_source_location(text: &str, file: &str) -> bool {
    let needle = format!("{file}:");
    text.split_ascii_whitespace().any(|token| {
        token.split_once(&needle).is_some_and(|(dir, position)| {
            (dir.is_empty() || dir.ends_with('/'))
                && position.starts_with(|c: char| c.is_ascii_digit())
        })
    })
}

#[test]
fn accept_t430_only_this_files_frames_count_as_located() {
    let std_only = "0: core::panicking::panic_fmt\n at /rustc/x/library/core/src/panicking.rs:80:14\n\
2: accept_t430_ci_disk_budget::cs_xtask_t430_panic_helper\n";
    assert!(!names_source_location(std_only, THIS_FILE));
    let located = "2: cs_xtask_t430_panic_helper\n at ./tools/cs_xtask/tests/accept_t430_ci_disk_budget.rs:194:5\n";
    assert!(names_source_location(located, THIS_FILE));
    assert!(!names_source_location(
        "at ./tests/not_accept_t430_ci_disk_budget.rs:194:5",
        THIS_FILE
    ));
}

/// What the build that produced this binary was actually configured with, so
/// the next reader of this failure does not have to re-derive it. Two things
/// decide whether a backtrace can name a line, and both are reported:
///
/// * the environment, which overrides the committed profile. A single
///   `CARGO_PROFILE_DEV_DEBUG=0` reproduces this exact failure on this host:
///   measured with it set, the child's frames carry no location at all and
///   this test fails.
/// * on macOS, where the binary's own line tables do not live in the binary.
///   rustc's macOS default (`-C split-debuginfo=unpacked`, which cargo passes
///   explicitly) links no DWARF in: the symbol table holds one `N_OSO`
///   debug-map entry per object, naming this crate's `*.rcgu.o` files in
///   `target/debug/deps`, and `std` opens those paths only when it prints a
///   backtrace. So on that platform the report says how many of them are gone
///   rather than assuming the profile is at fault. Task #691 measured all of
///   this; `docs/findings/2026-10-06-t691-macos-backtrace-line-tables.md` has
///   the evidence and the option that was rejected on cost grounds.
fn line_table_diagnosis(exe: &Path) -> String {
    let mut lines = vec![String::from("How the binary that just failed was built:")];
    lines.extend(debug_info_overrides());
    lines.extend(macos_line_table_state(exe));
    lines.join("\n")
}

/// The environment settings that can take the line tables out of a build that
/// otherwise obeys the committed profile. Cargo reads these over the manifest,
/// so a value here wins over `[profile.dev] debug` for every build in the
/// environment that exports it.
fn debug_info_overrides() -> Vec<String> {
    debug_info_overrides_from(|name| std::env::var_os(name))
}

/// [`debug_info_overrides`] over a caller-supplied lookup, so the rule can be
/// checked against settings this process did not have to be built with.
fn debug_info_overrides_from(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Vec<String> {
    let mut notes = Vec::new();
    for name in ["CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"] {
        if let Some(value) = get(name) {
            notes.push(format!(
                "{name}={value:?} is set and cargo's environment overrides the \
committed [profile.dev] debug, so the binaries carry that level"
            ));
        }
    }
    for name in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
        if let Some(value) = get(name) {
            notes.push(format!(
                "{name}={value:?} is set; a -C debuginfo=0 or -C strip in it \
overrides the profile's debug level"
            ));
        }
    }
    if notes.is_empty() {
        notes.push(String::from(
            "no CARGO_PROFILE_DEV_DEBUG, CARGO_PROFILE_TEST_DEBUG, RUSTFLAGS or \
CARGO_ENCODED_RUSTFLAGS is set, so the committed [profile.dev] \
debug = \"line-tables-only\" was what this binary was built with",
        ));
    }
    notes
}

/// Where this binary keeps the line tables of its own crate, on the platform
/// where it does not keep them in the binary. Empty everywhere else, where the
/// committed profile is the only thing that decides.
#[cfg(target_os = "macos")]
fn macos_line_table_state(exe: &Path) -> Vec<String> {
    let directory = exe.parent().unwrap_or_else(|| Path::new("."));
    let image = match std::fs::read(exe) {
        Ok(image) => image,
        Err(error) => return vec![format!("this binary could not be read: {error}")],
    };
    match debug_map_objects(&image) {
        Ok(entries) => describe_own_objects(&own_crate_objects(&entries, directory)),
        Err(reason) => vec![format!(
            "this binary's debug map could not be read: {reason}"
        )],
    }
}

/// Every other platform links the line tables into the binary, so there is
/// nothing to look up: the debug level is the whole story.
#[cfg(not(target_os = "macos"))]
fn macos_line_table_state(_exe: &Path) -> Vec<String> {
    Vec::new()
}

/// The report for the objects a macOS binary points its line tables at: how
/// many there were, which of them are gone, and what follows from that. Every
/// object that is not there is named, so a reader never has to guess which one
/// was dropped.
fn describe_own_objects(objects: &[PathBuf]) -> Vec<String> {
    if objects.is_empty() {
        return vec![String::from(
            "this binary's debug map names no object of its own crate, so it can \
locate a frame only through DWARF inside the binary itself: a .dSYM, or a \
platform that links it in. rustc's macOS default \
(-C split-debuginfo=unpacked) links none and writes no such entry, which is \
what an overridden debug level leaves behind",
        )];
    }
    let gone: Vec<&PathBuf> = objects.iter().filter(|object| !object.exists()).collect();
    let mut notes = vec![format!(
        "this binary keeps the line tables of its own crate outside itself \
(rustc's macOS default -C split-debuginfo=unpacked): its debug map names {} \
object(s) of this crate, of which {} no longer exist",
        objects.len(),
        gone.len()
    )];
    for object in &gone {
        notes.push(format!("  gone: {}", object.display()));
    }
    if gone.is_empty() {
        notes.push(String::from(
            "  all of them are still there, so the line tables are missing from the \
objects themselves: that is the debug level this binary was built with, not a \
file that went missing",
        ));
    }
    notes
}

/// `MH_MAGIC_64`, the little-endian magic of a thin 64-bit Mach-O image.
const MH_MAGIC_64: u32 = 0xfeed_facf;
/// `LC_SYMTAB`, the load command that locates the symbol and string tables.
const LC_SYMTAB: u32 = 0x2;
/// `N_OSO`, the stab type of a debug-map entry naming an object file.
const N_OSO: u8 = 0x66;
/// Size of a `struct nlist_64`.
const NLIST_64_SIZE: usize = 16;

/// The `N_OSO` paths of a thin 64-bit Mach-O image, in symbol-table order.
/// `Err` names what could not be read, so a binary this cannot parse is
/// reported rather than taken to have no objects at all.
fn debug_map_objects(image: &[u8]) -> Result<Vec<String>, String> {
    let u32_at = |offset: usize| {
        image
            .get(offset..offset + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| format!("the Mach-O image ends before offset {offset}"))
    };
    if u32_at(0)? != MH_MAGIC_64 {
        return Err(String::from("the image is not a thin 64-bit Mach-O binary"));
    }
    let mut command = 32;
    for _ in 0..u32_at(16)? {
        let kind = u32_at(command)?;
        let size = u32_at(command + 4)? as usize;
        if size == 0 {
            return Err(String::from("a load command claims to be empty"));
        }
        if kind == LC_SYMTAB {
            let (symbols, count) = (
                u32_at(command + 8)? as usize,
                u32_at(command + 12)? as usize,
            );
            let strings = u32_at(command + 16)? as usize;
            let strings_size = u32_at(command + 20)? as usize;
            let table = image
                .get(strings..strings + strings_size)
                .ok_or_else(|| format!("the string table at {strings} is outside the image"))?;
            let mut objects = Vec::new();
            for index in 0..count {
                let entry = symbols + index * NLIST_64_SIZE;
                if image.get(entry + 4) != Some(&N_OSO) {
                    continue;
                }
                let name = table.get(u32_at(entry)? as usize..).ok_or_else(|| {
                    String::from("a debug-map entry points outside the string table")
                })?;
                let end = name
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len());
                objects.push(String::from_utf8_lossy(&name[..end]).into_owned());
            }
            return Ok(objects);
        }
        command += size;
    }
    Ok(Vec::new())
}

/// The objects of `exe`'s own crate among the debug-map entries `of` it.
///
/// Two things are not this crate's objects and must never be counted as such:
/// the `archive(member)` references into a dependency's rlib, which are not
/// paths at all, and any loose path outside `directory`, which belongs to
/// something else. The binary's own file name is not usable as the prefix
/// either: rustc truncates a long crate name in the object file name
/// (`f2a9df64c40d13f7-yte_edit_fingerprint….rcgu.o`), so the directory and
/// the `.rcgu.o` suffix are what identify them.
fn own_crate_objects(entries: &[String], directory: &Path) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|entry| !entry.contains('('))
        .map(PathBuf::from)
        .filter(|object| {
            object.to_string_lossy().ends_with(".rcgu.o") && object.parent() == Some(directory)
        })
        .collect()
}

/// The reader finds `N_OSO` entries and nothing else: a minimal image with one
/// load command, one ordinary symbol and one debug-map entry.
#[test]
fn accept_t430_debug_map_reader_lists_only_object_paths() {
    let strings = b"\0_main\0/t/deps/x-1.a.rcgu.o\0";
    let (symbols, strings_at) = (32 + 24, 32 + 24 + 2 * NLIST_64_SIZE);
    let mut image = Vec::new();
    for word in [MH_MAGIC_64, 0x0100_000c, 0, 2, 1, 24, 0, 0] {
        image.extend_from_slice(&word.to_le_bytes());
    }
    for word in [
        LC_SYMTAB,
        24,
        symbols as u32,
        2,
        strings_at as u32,
        strings.len() as u32,
    ] {
        image.extend_from_slice(&word.to_le_bytes());
    }
    for (name, kind) in [(1_u32, 0x0f_u8), (7, N_OSO)] {
        image.extend_from_slice(&name.to_le_bytes());
        image.extend_from_slice(&[kind, 0, 0, 0]);
        image.extend_from_slice(&0_u64.to_le_bytes());
    }
    image.extend_from_slice(strings);
    assert_eq!(
        debug_map_objects(&image),
        Ok(vec!["/t/deps/x-1.a.rcgu.o".to_string()])
    );
    assert_eq!(
        debug_map_objects(&[]),
        Err(String::from("the Mach-O image ends before offset 0"))
    );
    assert_eq!(
        debug_map_objects(&0x0100_000c_u32.to_le_bytes()),
        Err(String::from("the image is not a thin 64-bit Mach-O binary"))
    );
}

/// Only this crate's own loose objects are counted: a dependency's rlib member
/// is an `archive(member)` reference and not a path at all, a rustup path is
/// another toolchain's, and a `.d` file is not an object. Every test binary's
/// own CGUs are the loose `.rcgu.o` files in its own directory, which is why
/// the name of the binary cannot be the discriminator.
#[test]
fn accept_t430_own_objects_exclude_dependency_members_and_other_directories() {
    let entries = [
        "/t/debug/deps/x-1.a.cgu.0.rcgu.o".to_string(),
        "/t/debug/deps/x-1.b.cgu.0.rcgu.o".to_string(),
        "/t/debug/deps/libfoo.rlib(foo-abc123.foo.cgu.0.rcgu.o)".to_string(),
        "/rustup/toolchains/1/lib/rustlib/a/lib/libstd.rlib(std.cgu.0.rcgu.o)".to_string(),
        "/other/dir/z-9.c.cgu.0.rcgu.o".to_string(),
        "/t/debug/deps/x-1.a.d".to_string(),
    ];
    assert_eq!(
        own_crate_objects(&entries, Path::new("/t/debug/deps")),
        [
            PathBuf::from("/t/debug/deps/x-1.a.cgu.0.rcgu.o"),
            PathBuf::from("/t/debug/deps/x-1.b.cgu.0.rcgu.o"),
        ]
    );
    assert!(own_crate_objects(&entries, Path::new("/t/debug/deps")).len() < entries.len());
    assert!(own_crate_objects(&[], Path::new("/t/debug/deps")).is_empty());
}

/// The report names every object that is not there, and says so plainly when
/// they are all there, so a reader can tell a missing file from a debug level.
#[test]
fn accept_t430_line_table_report_names_the_objects_that_are_gone() {
    let all_there = describe_own_objects(&[PathBuf::from("/")]);
    assert!(
        all_there[0].contains("1 object(s)") && all_there[0].contains("0 no longer exist"),
        "{all_there:?}"
    );
    assert!(all_there[1].contains("still there"), "{all_there:?}");

    let one_gone = describe_own_objects(&[
        PathBuf::from("/"),
        PathBuf::from("/t/debug/deps/gone.cgu.0.rcgu.o"),
    ]);
    assert!(one_gone[0].contains("1 no longer exist"), "{one_gone:?}");
    assert!(
        one_gone[1].contains("gone: /t/debug/deps/gone.cgu.0.rcgu.o"),
        "{one_gone:?}"
    );
    assert_eq!(one_gone.len(), 2, "{one_gone:?}");

    let none_named = describe_own_objects(&[]);
    assert_eq!(none_named.len(), 1, "{none_named:?}");
    assert!(
        none_named[0].contains("names no object of its own crate"),
        "{none_named:?}"
    );
}

/// An overriding debug level is named in the failure, because it reproduces
/// this failure exactly and is invisible in the manifest, and a clean
/// environment is reported as clean rather than left blank.
#[test]
fn accept_t430_line_table_diagnosis_names_the_environment() {
    let clean = debug_info_overrides_from(|_| None);
    assert_eq!(clean.len(), 1, "{clean:?}");
    assert!(clean[0].contains("no CARGO_PROFILE_DEV_DEBUG"), "{clean:?}");

    let overridden =
        debug_info_overrides_from(|name| (name == "CARGO_PROFILE_DEV_DEBUG").then(|| "0".into()));
    assert_eq!(overridden.len(), 1, "{overridden:?}");
    assert!(
        overridden[0].contains("CARGO_PROFILE_DEV_DEBUG=\"0\"")
            && overridden[0].contains("overrides the committed"),
        "{overridden:?}"
    );

    let both = debug_info_overrides_from(|name| match name {
        "CARGO_PROFILE_DEV_DEBUG" => Some("none".into()),
        "CARGO_PROFILE_TEST_DEBUG" => Some("0".into()),
        "RUSTFLAGS" => Some("-C debuginfo=0".into()),
        _ => None,
    });
    assert_eq!(both.len(), 3, "{both:?}");
}

/// On macOS the report is produced from this very binary, and it accounts for
/// every object the reader finds for this crate: one that is neither present
/// nor named would be a report that hides its own evidence.
#[cfg(target_os = "macos")]
#[test]
fn accept_t430_macos_report_accounts_for_every_object_it_finds() {
    let exe = std::env::current_exe().expect("this test binary must have a path");
    let image = std::fs::read(&exe).expect("this test binary must be readable");
    let entries = debug_map_objects(&image).expect("this test binary's debug map must be readable");
    let own = own_crate_objects(&entries, exe.parent().expect("a test binary has a parent"));
    let report = describe_own_objects(&own).join("\n");
    if own.is_empty() {
        assert!(
            report.contains("names no object of its own crate"),
            "{report}"
        );
    } else {
        assert!(
            report.contains(&format!("names {} object(s)", own.len())),
            "{report}"
        );
    }
    for object in &own {
        assert!(
            object.exists() || report.contains(&object.display().to_string()),
            "every object the debug map names is either there or named in the report: \
{report}"
        );
    }
}
