//! Task #336: keep `__eh_frame` under the Apple linker's compact-unwind
//! encoding limit so every function keeps a valid unwind entry.
//!
//! Measured cause (`docs/findings/2026-09-23-t336-eh-frame-unwind-table.md`):
//! compact-unwind entries reference DWARF FDEs through a 24-bit offset, so
//! `__eh_frame` beyond 16 MiB cannot be encoded. With the dependency graph at
//! `opt-level = 0` the `cs` binary linked 17,061,116 bytes of `__eh_frame` and
//! 5,733 functions got entries whose dwarf offset wrapped to 0 — the CIE, not
//! an FDE — while macOS arm64 unwinding cannot fall back to `__eh_frame` at
//! all (a panic in a compact-unwind-free binary aborts with `END_OF_STACK`).
//! Unwinding through any of those frames therefore aborts instead of
//! unwinding. The workspace keeps `__eh_frame` below the cap by building
//! dependencies at `opt-level = 3` in the dev profile
//! (`[profile.dev.package."*"]` in the workspace `Cargo.toml`), which shrank
//! the section to ~6.2 MiB and removed the `ld: __eh_frame section too large`
//! warning.
//!
//! Observable failure if the override is removed: the dev-profile binaries
//! grow `__eh_frame` past 16 MiB again and the Mach-O assertion below fails.

use std::fs;
use std::path::{Path, PathBuf};

/// Workspace root: `crates/cs_app` is two levels below it.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root must exist")
}

/// The fix lives in exactly one place: `[profile.dev.package."*"]` must keep
/// dependencies optimized so their `__eh_frame` contribution stays small.
/// Removing or weakening the override must fail this gate (the assertion is
/// about the setting, not about its current byte-count side effect).
#[test]
fn accept_t336_dev_profile_keeps_dependencies_optimized() {
    let manifest = fs::read_to_string(workspace_root().join("Cargo.toml"))
        .expect("the workspace Cargo.toml must be readable");

    let mut in_override = false;
    let mut opt_level = None;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_override = matches!(
                line,
                "[profile.dev.package.*]" | "[profile.dev.package.\"*\"]"
            );
            continue;
        }
        if in_override && line.starts_with("opt-level") {
            opt_level = line
                .split_once('=')
                .map(|(_, value)| value.trim().trim_matches('"').to_string());
        }
    }

    assert_eq!(
        opt_level.as_deref(),
        Some("3"),
        "workspace Cargo.toml must keep [profile.dev.package.*] opt-level = 3 \
         so __eh_frame stays under the 16 MiB compact-unwind encoding limit"
    );
}

/// A panic unwound through several frames must run every destructor and be
/// catchable: exception handling in the linked artifact has to work, not
/// merely exist. On macOS this exercises the compact-unwind path end to end.
#[test]
fn accept_t336_panic_unwinds_and_runs_destructors() {
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[inline(never)]
    fn deepest() {
        let _guard = Guard;
        panic!("accept_t336 unwind probe");
    }
    #[inline(never)]
    fn middle() {
        let _guard = Guard;
        deepest();
    }
    #[inline(never)]
    fn outer() {
        let _guard = Guard;
        middle();
    }

    let result = panic::catch_unwind(AssertUnwindSafe(outer));
    assert!(
        result.is_err(),
        "the panic must unwind to catch_unwind, not abort"
    );
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        3,
        "every frame's destructor must run while unwinding"
    );
}

/// The warning's precondition asserted on the linked artifacts themselves:
/// both the shipped `cs` binary and this suite's test binary must keep
/// `__eh_frame` under the 16 MiB that the compact-unwind dwarf-offset field
/// can encode, and must carry a real `__unwind_info` table (macOS arm64
/// unwinding has no working `__eh_frame` fallback — see the findings file).
/// Suites b–f assert their own binary in `accept_t336_suite_<x>_image.rs`.
#[cfg(target_os = "macos")]
#[test]
fn accept_t336_linked_images_keep_eh_frame_under_the_encoding_limit() {
    for image in [
        PathBuf::from(env!("CARGO_BIN_EXE_cs")),
        std::env::current_exe().expect("the test binary path must resolve"),
    ] {
        crate::eh_frame_gate::assert_image_keeps_unwind_table(&image);
    }
}
