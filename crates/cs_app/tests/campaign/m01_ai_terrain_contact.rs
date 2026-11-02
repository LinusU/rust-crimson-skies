//! M01-AI-TERRAIN-CONTACT-TESTS (#2293): the per-type contact tests the
//! world segment query `0x4c8f70` runs against the objects M01's hillside
//! is made of — the dispatch prologue `0x4c9a00`, the AABB test `0x4cbf80`,
//! the bounds provider `0x4cc010`, the per-type handlers `0x4c9fe0` /
//! `0x4c9adf` / `0x4c9e10` / `0x4ca140`, the triangle-exact mesh test
//! `0x55c9c0` with its polygon testers `0x55d6c0` / `0x55db90`, and what
//! the follower caller `0x41f810` reads of the `0x2c`-byte record.
//!
//! Task test prefix: `accept_m01_ai_terrain_contact_`. The decode and the
//! meaning of every window is in
//! `docs/findings/2026-11-02-ai-terrain-query-4c8f70.md` §13. Every
//! image-backed test is ignored without `CS_ENGINE_IMAGE` and fails loudly
//! when it is unset; the retail reading is ignored without `CS_GAME_DIR`
//! and fails loudly too. All claims are `static` or retail-file: the
//! original executable was **not** run.

use std::path::{Path, PathBuf};

use cs_content::coordinates::load_engine_image;
use cs_formats::ParseContext;
use cs_formats::gamez::nodes::{NodeKind, read_gamez_nodes};

use crate::campaign::m01_wingman_gains::bytes_at;

fn u32_at(bytes: &[u8], va: u64) -> u32 {
    u32::from_le_bytes(bytes_at(bytes, va, 4).try_into().expect("four bytes"))
}

fn f64_at(bytes: &[u8], va: u64) -> f64 {
    f64::from_le_bytes(bytes_at(bytes, va, 8).try_into().expect("eight bytes"))
}

/// The installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: this member needs the retail capability; run it with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The installation's spelling of one file, as in the sibling grid test.
fn install_file(root: &Path, relative: &str) -> Vec<u8> {
    let mut candidates = vec![root.join(relative.to_ascii_lowercase())];
    let upper = relative.to_ascii_uppercase();
    if upper != relative.to_ascii_lowercase() {
        candidates.push(root.join(upper));
    }
    candidates.push(root.join(relative));
    for path in &candidates {
        if path.is_file() {
            return std::fs::read(path).expect("the installation's file reads");
        }
    }
    panic!("none of {candidates:?} exists: this is not the measured installation layout");
}

/// (virtual address, bytes, what the window shows).
///
/// Every window is a place §13's reading rests on: the `0x4c9a00` arg2
/// gate and the `0x20` mask, the type-5 handler's AABB/transform/mesh
/// stages, the box overlap, the bounds provider's two forms, the mesh
/// test's group walk and tester dispatch, the record writes, the other
/// dispatch cases' call sites, the `[0x71e8b0]` setter and the caller's
/// count-byte read.
const WINDOWS: &[(u64, &[u8], &str)] = &[
    (
        0x4c9a9d,
        &[0xb3, 0x20, 0x3a, 0xcb, 0x72],
        "bl = 0x20 (the flag mask), doubling as the record-count cap",
    ),
    (
        0x4c9adf,
        &[
            0x8b, 0x0d, 0xf4, 0xb2, 0x63, 0x00, 0x8b, 0x44, 0x24, 0x18, 0x41, 0xbd, 0x01, 0x00,
            0x00, 0x00, 0x3b, 0xc5,
        ],
        "case 5: tally [0x63b2f4]; arg2 (entry ordinal + 1) vs 1",
    ),
    (
        0x4c9afc,
        &[0x84, 0xc3, 0x74, 0x28],
        "arg2 <= 1 and flag 0x20 clear -> exact path, no AABB",
    ),
    (
        0x4c9b00,
        &[0x56, 0xe8, 0x0a, 0x25, 0x00, 0x00],
        "call 0x4cc010 (bounds pre-reject)",
    ),
    (
        0x4c9b14,
        &[0x84, 0xc3, 0x74, 0x10],
        "post-AABB flag 0x20 -> record on the box alone",
    ),
    (
        0x4c9b18,
        &[0x56, 0xe8, 0x32, 0xe4, 0xff, 0xff],
        "call 0x4c7f50 (box-suffices record)",
    ),
    (
        0x4c9b28,
        &[0x8b, 0x7e, 0x38, 0x8b, 0x0f, 0xf6, 0xc1, 0x08],
        "edi = [obj+0x38] class data; [edi] bit 8 skips the transform",
    ),
    (
        0x4c9b32,
        &[0xa9, 0x00, 0x00, 0x08, 0x00],
        "[obj+0x24] bit 0x80000 selects the transform form",
    ),
    (
        0x4c9b60,
        &[0xa1, 0xdc, 0x79, 0x63, 0x00, 0x83, 0xc0, 0x04],
        "inline stack push: flag word at [0x6379dc]",
    ),
    (
        0x4c9b7b,
        &[0x83, 0xc7, 0x60, 0xa3, 0xe0, 0x79, 0x63, 0x00],
        "inline stack push: matrix pointer edi+0x60 at [0x6379e0]",
    ),
    (
        0x4c9b87,
        &[0x8d, 0x47, 0x60, 0x50, 0xe8, 0xe0, 0xf6, 0x06, 0x00],
        "0x539270(edi+0x60) push call",
    ),
    (
        0x4c9b90,
        &[
            0x6a, 0x03, 0x83, 0xc7, 0x30, 0x57, 0xe8, 0x25, 0x08, 0x07, 0x00,
        ],
        "0x53a3c0(edi+0x30, 3) fill call",
    ),
    (
        0x4c9b55,
        &[0xa8, 0x01, 0x75, 0x49, 0x83, 0xe0, 0xdf, 0x89, 0x07],
        "the [edi] 0x20 consume-once clear runs only when [edi] bit 0x01 is clear",
    ),
    (
        0x4c9ba2,
        &[0x8b, 0x46, 0x3c, 0x85, 0xc0, 0x74, 0x35],
        "[obj+0x3c] (resolved mesh record) required",
    ),
    (
        0x4c9ba9,
        &[0x8b, 0x8e, 0xc4, 0x00, 0x00, 0x00],
        "ecx = [obj+0xc4] (the file's unk196 detail word)",
    ),
    (
        0x4c9baf,
        &[0x3b, 0x0d, 0xb0, 0xe8, 0x71, 0x00, 0x7e, 0x27],
        "[obj+0xc4] <= [0x71e8b0] skips the exact test",
    ),
    (
        0x4c9bbd,
        &[0x68, 0xc4, 0xe8, 0x71, 0x00, 0x68, 0xb8, 0xe8, 0x71, 0x00],
        "&seg_end 0x71e8c4, &seg_start 0x71e8b8 pushed as args 3/4",
    ),
    (
        0x4c9bc9,
        &[0xe8, 0xf2, 0x2d, 0x09, 0x00],
        "call 0x55c9c0 (triangle-exact test)",
    ),
    (
        0x4c9bd5,
        &[0x56, 0xe8, 0x75, 0xe3, 0xff, 0xff],
        "call 0x4c7f50 after a hit",
    ),
    (
        0x4c9bf1,
        &[0x66, 0x8b, 0x46, 0x56],
        "children count word [obj+0x56]",
    ),
    (
        0x4c9c03,
        &[0x8b, 0x4e, 0x5c, 0x8b, 0x04, 0xb9],
        "child = [[obj+0x5c] + 4*i]",
    ),
    (
        0x4c9c30,
        &[0xe8, 0xcb, 0xfd, 0xff, 0xff],
        "recurse 0x4c9a00(child, child_count)",
    ),
    (
        0x4c9c55,
        &[0x85, 0xed, 0x74, 0x05, 0xe8, 0xf2, 0x78, 0xff, 0xff],
        "a pushed transform is popped by 0x4c1550",
    ),
    (
        0x4c9c5e,
        &[0x8b, 0x0d, 0xf0, 0xe9, 0x71, 0x00, 0x8a, 0x11, 0x32, 0xc0],
        "return = (count byte == 0)",
    ),
    (
        0x4c9c72,
        &[
            0xff, 0x05, 0xec, 0xb2, 0x63, 0x00, 0x8b, 0x56, 0x38, 0xd9, 0x42, 0x04, 0xdc, 0x1d,
            0xe8, 0x33, 0x60, 0x00,
        ],
        "case 6: tally [0x63b2ec]; [data+4] fcomp 5.0 gate",
    ),
    (
        0x4c9cd9,
        &[0xe8, 0x22, 0xfd, 0xff, 0xff],
        "case 6 recurses 0x4c9a00 on every child",
    ),
    (
        0x4c9d12,
        &[
            0xff, 0x05, 0xfc, 0xb2, 0x63, 0x00, 0x8b, 0x7e, 0x38, 0x83, 0x3f, 0x00,
        ],
        "case 7: tally [0x63b2fc]; [data] nonzero required",
    ),
    (
        0x4c9d60,
        &[0x8b, 0x47, 0x14, 0x8b, 0x4c, 0xc7, 0x20],
        "case 7 tests one child: [data + [data+0x14]*8 + 0x20]",
    ),
    (
        0x4c9d76,
        &[
            0x8b, 0x54, 0x24, 0x18, 0x52, 0x56, 0xe8, 0x8f, 0x00, 0x00, 0x00,
        ],
        "case 8 tail-calls 0x4c9e10",
    ),
    (
        0x4c9d89,
        &[
            0x8b, 0x44, 0x24, 0x18, 0x50, 0x56, 0xe8, 0x4c, 0x02, 0x00, 0x00,
        ],
        "case 1 tail-calls 0x4c9fe0",
    ),
    (
        0x4c9d9c,
        &[
            0x8b, 0x4c, 0x24, 0x18, 0x51, 0x56, 0xe8, 0x99, 0x03, 0x00, 0x00,
        ],
        "case 9 tail-calls 0x4ca140",
    ),
    (
        0x4c9dd7,
        &[0x8b, 0x44, 0x24, 0x18],
        "case 10 returns arg2, tests nothing",
    ),
    (
        0x4cbf80,
        &[
            0xa1, 0x5c, 0xb3, 0x63, 0x00, 0xd9, 0x05, 0xdc, 0xe8, 0x71, 0x00, 0x8b, 0x4c, 0x24,
            0x04, 0x40, 0xa3,
        ],
        "0x4cbf80: tally [0x63b35c]; seg max x vs [box]",
    ),
    (
        0x4cbfa1,
        &[0xd9, 0x05, 0xd0, 0xe8, 0x71, 0x00, 0xd8, 0x59, 0x0c],
        "seg min x vs [box+0xc] (strict)",
    ),
    (
        0x4cbfb4,
        &[0xd9, 0x05, 0xe0, 0xe8, 0x71, 0x00, 0xd8, 0x59, 0x04],
        "seg max y vs [box+4] (strict)",
    ),
    (
        0x4cbfc7,
        &[0xd9, 0x05, 0xd4, 0xe8, 0x71, 0x00, 0xd8, 0x59, 0x10],
        "seg min y vs [box+0x10] (strict)",
    ),
    (
        0x4cbfda,
        &[0xd9, 0x05, 0xe4, 0xe8, 0x71, 0x00, 0xd8, 0x59, 0x08],
        "seg max z vs [box+8] (strict)",
    ),
    (
        0x4cbfed,
        &[0xd9, 0x05, 0xd8, 0xe8, 0x71, 0x00, 0xd8, 0x59, 0x14],
        "seg min z vs [box+0x14] (strict)",
    ),
    (
        0x4cc010,
        &[0x8b, 0x15, 0x5c, 0xb3, 0x63, 0x00],
        "0x4cc010 shares the [0x63b35c] tally",
    ),
    (
        0x4cc03a,
        &[0xa8, 0x20, 0x75, 0x17],
        "flag 0x20 -> the eight-corner transformed box",
    ),
    (
        0x4cc044,
        &[0xe8, 0x67, 0x21, 0x00, 0x00],
        "call 0x4ce1b0 (the plain AABB provider)",
    ),
    (
        0x4cc05b,
        &[0xe8, 0x70, 0x19, 0x00, 0x00],
        "call 0x4cd9d0 (the transformed-corners provider)",
    ),
    (
        0x4cc093,
        &[0xba, 0x07, 0x00, 0x00, 0x00],
        "0x4cc010 merge loop over the returned box: seven counts",
    ),
    (
        0x4cc112,
        &[0x83, 0xc1, 0x0c, 0x4a, 0x75, 0x80],
        "merge loop: step 0xc per component pair, while count",
    ),
    (
        0x4cc11c,
        &[0xd9, 0x05, 0xdc, 0xe8, 0x71, 0x00, 0xd8, 0xd9, 0xdf, 0xe0],
        "the overlap is inlined: seg max x vs the merged box (strict)",
    ),
    (
        0x4cc1be,
        &[0xf6, 0x46, 0x24, 0x20, 0x74, 0x2c],
        "flag 0x20 objects take the box-contact call; others return 0",
    ),
    (
        0x4cc1da,
        &[0xe8, 0x91, 0x10, 0x09, 0x00, 0x83, 0xc4, 0x10, 0x85, 0xc0],
        "call 0x55d270 (segment vs box record); nonzero -> accepted",
    ),
    (
        0x55d270,
        &[
            0x83, 0xec, 0x08, 0x53, 0x55, 0x56, 0x8b, 0x74, 0x24, 0x18, 0x57, 0x8b, 0x7c, 0x24,
            0x20, 0xc7, 0x47, 0x20, 0x00, 0x00, 0x00, 0x00,
        ],
        "0x55d270: box writer head; rec+0x20 = 0 (a box face has no material)",
    ),
    (
        0x55c9c0,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x68, 0x53, 0x56, 0x57, 0x8b, 0x7d, 0x08, 0x8b, 0x47,
            0x10, 0x85, 0xc0,
        ],
        "0x55c9c0: edi = geo arg; [geo+0x10] group count required",
    ),
    (
        0x55c9d7,
        &[0xf6, 0x47, 0x08, 0x10],
        "[geo+8] bit 0x10 selects the scaled-vertex path",
    ),
    (
        0x55ca24,
        &[0x8b, 0x0d, 0xdc, 0x79, 0x63, 0x00, 0x83, 0x39, 0x00],
        "stacked-transform flag at [0x6379dc]",
    ),
    (
        0x55ca41,
        &[0xe8, 0x7a, 0x15, 0xfe, 0xff],
        "call 0x53dfc0 (inverse affine of the stack top)",
    ),
    (
        0x55cb5b,
        &[
            0x8b, 0x47, 0x58, 0x8d, 0x14, 0x80, 0x8b, 0x47, 0x34, 0x8d, 0x34, 0xd0,
        ],
        "group walk: [geo+0x34] + [geo+0x58] * 40",
    ),
    (
        0x55cb67,
        &[0x8b, 0x06, 0xf6, 0xc4, 0x20],
        "group word bit 0x2000 = indexed strip",
    ),
    (
        0x55cb72,
        &[0x25, 0xff, 0x03, 0x00, 0x00],
        "index count = group word & 0x3ff",
    ),
    (
        0x55cbd4,
        &[0x8b, 0x3d, 0x84, 0x5e, 0xa0, 0x00],
        "scratch triangle base 0xa05e84",
    ),
    (
        0x55cc3d,
        &[0xf6, 0x42, 0x01, 0x02],
        "[[group+0x14]] byte+1 bit 2 -> textured tester",
    ),
    (
        0x55cc9c,
        &[0xe8, 0xef, 0x0e, 0x00, 0x00],
        "call 0x55db90 (textured polygon test)",
    ),
    (
        0x55ccc4,
        &[0xe8, 0xf7, 0x09, 0x00, 0x00],
        "call 0x55d6c0 (plain polygon test)",
    ),
    (
        0x55cdc6,
        &[
            0x8b, 0x46, 0x14, 0x8b, 0x10, 0x8b, 0x4d, 0x0c, 0x89, 0x51, 0x20,
        ],
        "rec+0x20 = [[group+0x14]] (the surface word)",
    ),
    (
        0x55cdd1,
        &[0xa1, 0xdc, 0x79, 0x63, 0x00, 0x83, 0x38, 0x00],
        "the stack flag gates the world transform of the hit",
    ),
    (
        0x55ceac,
        &[0xe8, 0xdf, 0x57, 0xec, 0xff],
        "call 0x422690 (unit normal in place)",
    ),
    (
        0x55ceb6,
        &[0x5f, 0x5e, 0xb8, 0x01, 0x00, 0x00, 0x00],
        "hit: return 1",
    ),
    (0x55d15d, &[0x5f, 0x5e, 0x33, 0xc0, 0x5b], "miss: return 0"),
    (
        0x55d259,
        &[0x8b, 0x45, 0xec, 0x89, 0x47, 0x58],
        "[geo+0x58] = the hit group's index (cache)",
    ),
    (
        0x55d6c0,
        &[
            0x83, 0xec, 0x10, 0x8b, 0x4c, 0x24, 0x20, 0xa1, 0x64, 0xb3, 0x63, 0x00,
        ],
        "0x55d6c0: polygon tester; tally [0x63b364]",
    ),
    (
        0x55d717,
        &[0xd9, 0x1e],
        "fstp [esi] — the face normal lands at rec+0",
    ),
    (
        0x55d7d8,
        &[0xd9, 0x5e, 0x0c],
        "fstp [esi+0xc] — the contact point lands at rec+0xc",
    ),
    (
        0x55d84a,
        &[0xff, 0x24, 0xbd, 0x74, 0xdb, 0x55, 0x00],
        "six-way edge-walk jump through 0x55db74",
    ),
    (
        0x55db90,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x28],
        "0x55db90: the textured variant",
    ),
    (
        0x55dbbb,
        &[0xe8, 0xc0, 0x07, 0xfe, 0xff],
        "call 0x53e380 (polygon normal into rec)",
    ),
    (
        0x55de2e,
        &[0xe8, 0xfa, 0x05, 0xfe, 0xff],
        "call 0x53e42d (barycentric weights)",
    ),
    (
        0x4c9fe0,
        &[
            0x83, 0xec, 0x0c, 0x53, 0x56, 0x57, 0x8b, 0x7c, 0x24, 0x1c, 0xf6, 0x47, 0x24, 0x04,
        ],
        "type 1: object flag bit 4 gates the transform push",
    ),
    (
        0x4ca05d,
        &[0xe8, 0x5e, 0x29, 0x09, 0x00],
        "type 1 calls 0x55c9c0",
    ),
    (
        0x4ca06e,
        &[0x89, 0x78, 0x24],
        "type 1 writes rec+0x24 = object inline",
    ),
    (
        0x4c9e10,
        &[0x8b, 0x0d, 0xdc, 0xb2, 0x63, 0x00],
        "type 8: tally [0x63b2dc]",
    ),
    (
        0x4c9f02,
        &[0xe8, 0xb9, 0x2a, 0x09, 0x00],
        "type 8 calls 0x55c9c0",
    ),
    (
        0x4c9e5a,
        &[0xb9, 0x00, 0x00, 0x20, 0x00],
        "mov ecx, 0x200000 — the owner-chain flag tested at [node+0x24]",
    ),
    (
        0x4c9e68,
        &[0x85, 0xf6, 0x8b, 0xc6, 0x74, 0x1c],
        "owner walk: starts at the object",
    ),
    (
        0x4c9e73,
        &[0x8b, 0x50, 0x40],
        "owner word candidate [node+0x40]",
    ),
    (
        0x4c9e81,
        &[0x8b, 0x40, 0x58, 0x8b, 0x00],
        "ascend via parent_array[0] ([node+0x58])",
    ),
    (
        0x4ca140,
        &[0x8b, 0x0d, 0xe4, 0xb2, 0x63, 0x00],
        "type 9: tally [0x63b2e4]",
    ),
    (
        0x4ca1e6,
        &[0xe8, 0x85, 0xf0, 0x06, 0x00],
        "type 9 pushes edi+0x30 via 0x539270",
    ),
    (
        0x4ca244,
        &[0xe8, 0x77, 0x27, 0x09, 0x00],
        "type 9 calls 0x55c9c0",
    ),
    (
        0x4c7600,
        &[
            0x8b, 0x44, 0x24, 0x04, 0x85, 0xc0, 0x74, 0x0a, 0x8b, 0x44, 0x24, 0x08, 0xa3, 0xb0,
            0xe8, 0x71, 0x00,
        ],
        "[0x71e8b0] setter: arg1 nonzero -> arg2",
    ),
    (
        0x4c7612,
        &[0xc7, 0x05, 0xb0, 0xe8, 0x71, 0x00, 0xff, 0xff, 0xff, 0xff],
        "[0x71e8b0] default = -1 (the +0xc4 gate inert)",
    ),
    (
        0x41f821,
        &[
            0xd9, 0x86, 0x08, 0x02, 0x00, 0x00, 0xd8, 0x1d, 0xf0, 0xc3, 0x71, 0x00,
        ],
        "caller: vehicle y vs floor [0x71c3f0]",
    ),
    (
        0x41f838,
        &[
            0xd9, 0x86, 0x08, 0x02, 0x00, 0x00, 0xd8, 0x1d, 0xf4, 0xc3, 0x71, 0x00,
        ],
        "caller: vehicle y vs ceiling [0x71c3f4]",
    ),
    (
        0x41f95f,
        &[0x8a, 0x85, 0x80, 0x7b, 0xff, 0xff],
        "caller reads buf[0] (the count byte) — no record field",
    ),
    (
        0x41f98e,
        &[0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00],
        "the escape-climb latch [esi+0x358] = 3",
    ),
];

#[test]
fn accept_m01_ai_terrain_contact_windows_are_well_formed() {
    assert!(!WINDOWS.is_empty());
    for (va, expected, what) in WINDOWS {
        assert!(
            !expected.is_empty(),
            "{va:#x}: an empty window pins nothing ({what})"
        );
    }
    let mut vas: Vec<u64> = WINDOWS.iter().map(|(va, _, _)| *va).collect();
    vas.sort_unstable();
    vas.dedup();
    assert_eq!(
        vas.len(),
        WINDOWS.len(),
        "every window must name a distinct address"
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_ai_terrain_contact_windows_match_the_image() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    for (va, expected, what) in WINDOWS {
        assert_eq!(
            bytes_at(&image.bytes, *va, expected.len()),
            *expected,
            "{va:#x}: {what}"
        );
    }
}

/// The float constants §13 rests on: the polygon inside-test epsilon, the
/// type-6 LOD gate and the geometry scale compare, plus the record
/// capacity cap implied by the `0x20` mask (`0x4c9a9d`).
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_ai_terrain_contact_constants_match_the_image() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    assert_eq!(
        f64_at(bytes, 0x609180),
        -0.0001,
        "0x609180: the polygon inside-test epsilon"
    );
    assert_eq!(
        f64_at(bytes, 0x6033e8),
        5.0,
        "0x6033e8: the type-6 [data+4] gate"
    );
    assert_eq!(
        f64_at(bytes, 0x603398),
        0.0,
        "0x603398: the geometry scale compare"
    );

    // The second dispatch table entry the §13.8 table rests on: type 7
    // (index 6) reaches the selector handler 0x4c9d12.
    assert_eq!(
        u32_at(bytes, 0x4c9de0 + 6 * 4),
        0x4c9d12,
        "dispatch case 7 -> 0x4c9d12"
    );
}

/// §13.1: every member the query can meet in the hillside cells is node
/// type 5, the group `g28604` carries no own mesh (`mesh_index` -1), and
/// no member carries the "box suffices" flag `0x20` — so each reaches the
/// `0x55c9c0` triangle test.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_ai_terrain_contact_hillside_members_are_type5() {
    let root = game_dir();
    let bytes = install_file(&root, "ZBD/C3/gamez.zbd");
    let mut ctx = ParseContext::with_defaults("ZBD/C3/gamez.zbd");
    let nodes = read_gamez_nodes(&mut ctx, &bytes).expect("the container's node array reads");

    // (slot, name, mesh_index): the hillside members of §11's table plus
    // the group's mesh-carrying child.
    let members: &[(u32, &str, i32)] = &[
        (829, "g28596", 516),
        (871, "g28471", 559),
        (2874, "g28604", -1),
        (2882, "g28982", 762),
        (830, "g28598", 517),
        (881, "g28474", 562),
        (2873, "g28603", 761),
    ];
    for (slot, name, mesh) in members {
        let node = nodes
            .nodes
            .iter()
            .find(|node| node.index == *slot)
            .unwrap_or_else(|| panic!("slot {slot} exists"));
        assert_eq!(
            node.info.node_type, 5,
            "{name}: node_type must be 5 (Object3d), the §13.1 premise"
        );
        assert_eq!(
            node.info.mesh_index, *mesh,
            "{name}: mesh_index from the file"
        );
        assert_eq!(
            node.info.flags & 0x20,
            0,
            "{name}: the 0x20 box-suffices bit stays clear"
        );
        assert_eq!(
            node.info.flags & 0x14,
            0x14,
            "{name}: the 0x04|0x10 bits the dispatch requires"
        );
        assert!(
            matches!(&node.kind, NodeKind::Object3d(_)),
            "{name}: the data record is an Object3d"
        );
    }
}
