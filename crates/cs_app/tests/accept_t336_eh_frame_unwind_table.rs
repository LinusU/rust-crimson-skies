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

/// `size`/`addr` of one Mach-O section, found by walking 64-bit load
/// commands. Thin arm64 images are expected; fat images are searched for
/// their arm64 slice.
#[cfg(target_os = "macos")]
mod macho {
    use std::fs;
    use std::path::Path;

    pub struct UnwindSections {
        pub eh_frame_size: u64,
        pub unwind_info_size: u64,
    }

    const LC_SEGMENT_64: u32 = 0x19;
    const CPU_TYPE_ARM64: u32 = 0x0100_000C;
    const MH_MAGIC_64: u32 = 0xFEED_FACF;
    const FAT_MAGIC: u32 = 0xCAFE_BABE;
    const FAT_MAGIC_64: u32 = 0xCAFE_BABF;

    fn u32le(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    fn u32be(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    fn u64be(bytes: &[u8], at: usize) -> u64 {
        u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap())
    }
    fn u64le(bytes: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
    }

    /// Offset of the arm64 thin image inside `bytes` (0 for a thin file).
    fn arm64_slice_offset(bytes: &[u8]) -> usize {
        if u32le(bytes, 0) == MH_MAGIC_64 {
            return 0;
        }
        let magic = u32be(bytes, 0);
        assert!(
            magic == FAT_MAGIC || magic == FAT_MAGIC_64,
            "expected a Mach-O image, got magic {magic:#010x}"
        );
        let entry_size = if magic == FAT_MAGIC_64 { 32 } else { 20 };
        let nfat = u32be(bytes, 4) as usize;
        for i in 0..nfat {
            let at = 8 + i * entry_size;
            if u32be(bytes, at) == CPU_TYPE_ARM64 {
                // fat_arch_64 stores the slice offset as u64, fat_arch as u32.
                let offset = if magic == FAT_MAGIC_64 {
                    u64be(bytes, at + 8) as usize
                } else {
                    u32be(bytes, at + 8) as usize
                };
                assert_eq!(
                    u32le(bytes, offset),
                    MH_MAGIC_64,
                    "the fat arm64 slice must be a 64-bit Mach-O"
                );
                return offset;
            }
        }
        panic!("no arm64 slice in fat Mach-O");
    }

    /// Reads `__eh_frame`/`__unwind_info` sizes out of the linked image at
    /// `path` — the real artifact the linker produced, not a fixture.
    pub fn unwind_sections(path: &Path) -> UnwindSections {
        let bytes = fs::read(path).expect("the linked image must be readable");
        let base = arm64_slice_offset(&bytes);
        let ncmds = u32le(&bytes, base + 16) as usize;

        let mut eh_frame_size = None;
        let mut unwind_info_size = None;
        let mut at = base + 32; // sizeof(mach_header_64)
        for _ in 0..ncmds {
            let cmd = u32le(&bytes, at);
            let cmdsize = u32le(&bytes, at + 4) as usize;
            if cmd == LC_SEGMENT_64 {
                let nsects = u32le(&bytes, at + 64) as usize;
                let mut sect = at + 72; // sizeof(segment_command_64)
                for _ in 0..nsects {
                    let name_end = bytes[sect..sect + 16]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(16);
                    let name = &bytes[sect..sect + name_end];
                    let size = u64le(&bytes, sect + 40);
                    match name {
                        b"__eh_frame" => eh_frame_size = Some(size),
                        b"__unwind_info" => unwind_info_size = Some(size),
                        _ => {}
                    }
                    sect += 80; // sizeof(section_64)
                }
            }
            at += cmdsize;
        }

        UnwindSections {
            eh_frame_size: eh_frame_size.expect("__eh_frame must be present"),
            unwind_info_size: unwind_info_size.expect("__unwind_info must be present"),
        }
    }
}

/// The warning's precondition asserted on the linked artifacts themselves:
/// both the shipped `cs` binary and this test binary must keep `__eh_frame`
/// under the 16 MiB that the compact-unwind dwarf-offset field can encode,
/// and must carry a real `__unwind_info` table (macOS arm64 unwinding has no
/// working `__eh_frame` fallback — see the findings file).
#[cfg(target_os = "macos")]
#[test]
fn accept_t336_linked_images_keep_eh_frame_under_the_encoding_limit() {
    const COMPACT_UNWIND_DWARF_OFFSET_LIMIT: u64 = 16 * 1024 * 1024;

    for image in [
        PathBuf::from(env!("CARGO_BIN_EXE_cs")),
        std::env::current_exe().expect("the test binary path must resolve"),
    ] {
        let sections = macho::unwind_sections(&image);
        assert!(
            sections.eh_frame_size < COMPACT_UNWIND_DWARF_OFFSET_LIMIT,
            "{}: __eh_frame is {} bytes; past 16 MiB the linker cannot encode \
             dwarf unwind offsets and unwinding through the tail functions \
             aborts",
            image.display(),
            sections.eh_frame_size
        );
        assert!(
            sections.unwind_info_size > 0,
            "{}: __unwind_info must exist — without compact unwind a panic \
             cannot unwind at all on this platform",
            image.display()
        );
    }
}
