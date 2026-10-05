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
/// without them prints the helper's frame with no location, and this fails.
///
/// On macOS the one host condition under which this cannot judge the profile
/// is checked first: see [`missing_debug_map_objects`].
#[test]
fn accept_t430_a_panic_backtrace_names_the_file_and_line() {
    let exe = std::env::current_exe().expect("this test binary must have a path to re-run");
    let missing = missing_debug_map_objects(&exe);
    if !missing.is_empty() {
        eprintln!(
            "accept_t430_a_panic_backtrace_names_the_file_and_line NOT RUN on this \
host: {} names its line tables by debug-map (N_OSO) paths and {} of them were \
removed after the link, so no backtrace from it can name a line whatever the \
profile says (task #691, docs/findings/2026-10-06-t691-macos-backtrace-line-tables.md). \
Rebuild the binary to run it. Missing: {missing:?}",
            exe.display(),
            missing.len()
        );
        return;
    }
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
[profile.dev] debug = \"line-tables-only\" (cs_xtask verify-ci-budget); the \
helper's output was: {output}"
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

/// The objects of `exe`'s own crate that its Mach-O debug map names but that
/// no longer exist. Empty on every other host, and for a binary that keeps its
/// DWARF itself.
///
/// rustc's macOS default (`-C split-debuginfo=unpacked`, which cargo passes
/// explicitly) links no DWARF into the binary: its symbol table holds one
/// `N_OSO` entry per object, and the crate's own line tables stay in the
/// `<binary>.<cgu>.rcgu.o` files beside it in `target/debug/deps`. `std` opens
/// those paths only when it prints a backtrace. A deletion after the link —
/// this host's prune of agents' target directories — leaves every frame of
/// the crate without a location, which is a property of the host's files, not
/// of the profile this test guards.
fn missing_debug_map_objects(exe: &Path) -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let image = std::fs::read(exe).expect("this test binary must be readable");
    let own_prefix = format!(
        "{}.",
        exe.file_name()
            .expect("this test binary must have a file name")
            .to_string_lossy()
    );
    debug_map_objects(&image)
        .into_iter()
        .map(PathBuf::from)
        .filter(|object| {
            object.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with(&own_prefix) && name.ends_with(".o")
            })
        })
        .filter(|object| !object.exists())
        .collect()
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
/// Panics on anything else: a binary this cannot read is a broken check, and
/// must not look like a host condition.
fn debug_map_objects(image: &[u8]) -> Vec<String> {
    let u32_at = |offset: usize| {
        u32::from_le_bytes(
            image
                .get(offset..offset + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .expect("the Mach-O image must not be truncated"),
        )
    };
    assert_eq!(
        u32_at(0),
        MH_MAGIC_64,
        "the test binary must be thin Mach-O 64"
    );
    let command_count = u32_at(16);
    let mut command = 32;
    for _ in 0..command_count {
        let (kind, size) = (u32_at(command), u32_at(command + 4));
        if kind == LC_SYMTAB {
            let (symbols, count) = (u32_at(command + 8) as usize, u32_at(command + 12) as usize);
            let (strings, strings_size) =
                (u32_at(command + 16) as usize, u32_at(command + 20) as usize);
            let table = &image[strings..strings + strings_size];
            return (0..count)
                .map(|index| symbols + index * NLIST_64_SIZE)
                .filter(|entry| image[entry + 4] == N_OSO)
                .map(|entry| {
                    let name = &table[u32_at(entry) as usize..];
                    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                    String::from_utf8_lossy(&name[..end]).into_owned()
                })
                .collect();
        }
        command += size as usize;
    }
    Vec::new()
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
    assert_eq!(debug_map_objects(&image), ["/t/deps/x-1.a.rcgu.o"]);
}

/// On macOS the guard reads the real binary, and right after `cargo test`
/// built it every object its debug map names for this crate is still there,
/// so the backtrace test above runs rather than excusing itself.
#[cfg(target_os = "macos")]
#[test]
fn accept_t430_macos_guard_reads_this_binarys_debug_map() {
    let exe = std::env::current_exe().expect("this test binary must have a path");
    let image = std::fs::read(&exe).expect("this test binary must be readable");
    let objects = debug_map_objects(&image);
    assert!(
        objects.iter().any(|object| object.ends_with(".rcgu.o")),
        "an unpacked macOS build names its objects in the debug map: {objects:?}"
    );
}
