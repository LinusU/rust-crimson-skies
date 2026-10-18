//! Test-free support for task #336's `__eh_frame` gate (compiled by every
//! `cs_app` test binary; it holds no `#[test]`, see `shared_test_sources` in
//! `Cargo.toml`). Each binary asserts its own `current_exe()` through
//! [`assert_image_keeps_unwind_table`], so the measured artifact is the real
//! per-binary link output with no path guessing.

#![cfg(target_os = "macos")]
#![allow(dead_code)]

use std::path::Path;

/// `size`/`addr` of one Mach-O section, found by walking 64-bit load
/// commands. Thin arm64 images are expected; fat images are searched for
/// their arm64 slice.
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

/// The 24-bit dwarf-offset field of a compact-unwind entry cannot address
/// `__eh_frame` beyond 16 MiB.
pub const COMPACT_UNWIND_DWARF_OFFSET_LIMIT: u64 = 16 * 1024 * 1024;

/// Asserts that the linked image at `image` keeps `__eh_frame` under the
/// compact-unwind encoding limit and carries a real `__unwind_info` table
/// (macOS arm64 unwinding has no working `__eh_frame` fallback).
pub fn assert_image_keeps_unwind_table(image: &Path) {
    let sections = macho::unwind_sections(image);
    assert!(
        sections.eh_frame_size < COMPACT_UNWIND_DWARF_OFFSET_LIMIT,
        "{}: __eh_frame is {} bytes; past 16 MiB the linker cannot encode \
         dwarf unwind offsets and unwinding through the tail functions aborts",
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
