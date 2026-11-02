//! M01-AI-TERRAIN-GRID-BUILDER (#2290): the builder of the world area
//! partition `[holder+0x38]` behind `0x4c8f70`, the mission-script commands
//! that set it, the retail data that carries it, and whether M01's hillside
//! is in it.
//!
//! Task test prefix: `accept_m01_ai_terrain_grid_`. The decode is in
//! `docs/findings/2026-11-02-ai-terrain-query-4c8f70.md` §9-§12, which
//! supersedes that file's earlier "builder unknown". Every claim is `static`
//! or retail-file: the original executable was **not** run, so nothing here
//! is `verified_original`.
//!
//! Image windows (`CS_ENGINE_IMAGE`) and retail readings (`CS_GAME_DIR`) are
//! `#[ignore]`-ed for CI, which has neither; both fail loudly when the
//! variable is unset rather than passing.

use std::path::{Path, PathBuf};

use cs_content::coordinates::{load_engine_image, original_image_digest};
use cs_formats::ParseContext;
use cs_formats::gamez::nodes::{NODE_TYPE_WORLD, NodeKind, RawNode, read_gamez_nodes};
use cs_formats::gamez::read_gamez_meshes;
use cs_formats::interp::decode_interp;

use crate::campaign::m01_wingman_gains::bytes_at;

fn u32_at(bytes: &[u8], va: u64) -> u32 {
    u32::from_le_bytes(bytes_at(bytes, va, 4).try_into().expect("four bytes"))
}

fn f32_at(bytes: &[u8], va: u64) -> f32 {
    f32::from_le_bytes(bytes_at(bytes, va, 4).try_into().expect("four bytes"))
}

/// The little-endian word at a *file* offset of a retail container (the
/// image helpers above take `VA = offset + 0x400000`, retail data does not).
fn le_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn le_f32(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
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

/// The installation's spelling of one file. The measured tree is
/// `ZBD/interp.zbd` and `ZBD/C3/gamez.zbd`; a case-insensitive filesystem
/// (macOS) resolves either spelling, so the test asks for the file rather
/// than asserting one case, and fails loudly when neither exists.
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
/// Every window is a place §9-§10's reading rests on: the builder's allocation
/// and field stores, the seven script-command handlers, the node inserter and
/// the cell-range arithmetic, the file reader, the dispatcher's call sites,
/// the command-name strings, and the floor/ceiling writer.
const WINDOWS: &[(u64, &[u8], &str)] = &[
    (
        0x4daa20,
        &[
            0x55, 0x8b, 0xec, 0x51, 0x8b, 0x45, 0x08, 0x57, 0x8b, 0x78, 0x38, 0x8b, 0x8f, 0xa0,
        ],
        "builder head: the world node argument and its class data",
    ),
    (
        0x4daa3b,
        &[
            0x83, 0xc4, 0x04, 0xd9, 0x45, 0x0c, 0x8b, 0x45, 0x0c, 0xd8, 0x0d, 0x2c, 0x80, 0x60,
            0x00, 0x8b, 0x4d, 0x10, 0x89, 0x47, 0x74, 0x89,
        ],
        "WorldPartition: cell size x/z into +0x74/+0x78",
    ),
    (
        0x4daa8b,
        &[
            0xd9, 0x45, 0x0c, 0xd8, 0x0d, 0xe0, 0x32, 0x60, 0x00, 0xd9, 0x5f, 0x7c, 0xd9, 0x45,
            0x10, 0xd8, 0x0d, 0xe0, 0x32, 0x60, 0x00, 0xd9, 0x9f, 0x80,
        ],
        "half cell into +0x7c, then cell_size * 0.5 into +0x80",
    ),
    (
        0x4daaf1,
        &[
            0xd9, 0x47, 0x3c, 0xd8, 0x77, 0x74, 0xe8, 0x64, 0xc3, 0x11, 0x00, 0x8b, 0xc8, 0x89,
            0x4d, 0x0c, 0xdb, 0x45, 0x0c, 0x89, 0x8f, 0x98, 0x00, 0x00, 0x00, 0xd8, 0x4f, 0x74,
            0xd8, 0x5f, 0x3c, 0xdf, 0xe0, 0xf6, 0xc4, 0x01, 0x74, 0x07, 0x41, 0x89,
        ],
        "cols = ceil([+0x3c] / [+0x74]) into +0x98, bumped when the float product is short",
    ),
    (
        0x4dab1e,
        &[
            0xd9, 0x47, 0x40, 0xd8, 0x77, 0x78, 0xe8, 0x37, 0xc3, 0x11, 0x00, 0x8b, 0xc8, 0x89,
            0x4d, 0x0c, 0xdb, 0x45, 0x0c, 0x89, 0x8f, 0x9c, 0x00, 0x00, 0x00, 0xd8, 0x4f, 0x78,
            0xd8, 0x5f, 0x40, 0xdf, 0xe0, 0xf6, 0xc4, 0x41,
        ],
        "rows = ceil([+0x40] / [+0x78]) into +0x9c",
    ),
    (
        0x4dab51,
        &[
            0x81, 0xf9, 0xff, 0x7f, 0x00, 0x00, 0x0f, 0x8f, 0x1f, 0x01, 0x00, 0x00, 0x8b, 0x87,
            0x9c, 0x00, 0x00, 0x00, 0x3d, 0xff, 0x7f, 0x00, 0x00, 0x0f,
        ],
        "columns and rows refused above 0x7fff (message 0x62ddf4)",
    ),
    (
        0x4dab6e,
        &[
            0x53, 0x56, 0x6a, 0x04, 0x50, 0xff, 0x15, 0xb4, 0x01, 0xa2, 0x00, 0x89, 0x87, 0xa0,
        ],
        "row table = calloc(rows, 4) into +0xa0",
    ),
    (
        0x4dab90,
        &[
            0x8b, 0x97, 0x98, 0x00, 0x00, 0x00, 0x6a, 0x58, 0x52, 0xff, 0x15, 0xb4, 0x01, 0xa2,
            0x00, 0x89, 0x06, 0x8b, 0x87, 0x9c, 0x00, 0x00,
        ],
        "each row = calloc(cols, 0x58), stored through the row table",
    ),
    (
        0x4dabe4,
        &[
            0xdb, 0x45, 0x10, 0xd9, 0x5d, 0xfc, 0xdb, 0x45, 0x08, 0x8b, 0x16, 0x81, 0xca, 0x00,
            0x01, 0x00, 0x00, 0x8d, 0x46, 0x10, 0xd8, 0x4f, 0x74, 0xd8, 0x47, 0x34, 0xd9, 0x5e,
            0x08, 0xd9, 0x45, 0xfc,
        ],
        "cell: flags |= 0x100, min x = origin_x + col*size_x",
    ),
    (
        0x4dac35,
        &[0xe8, 0x16, 0xdf, 0xff, 0xff],
        "call 0x4d8b50: derive the cell's own AABB",
    ),
    (
        0x4dac3a,
        &[0xc7, 0x46, 0x04, 0xff, 0xff, 0xff, 0xff, 0x8b],
        "cell +0x04 = -1",
    ),
    (
        0x4dac7c,
        &[
            0x8b, 0x97, 0x9c, 0x00, 0x00, 0x00, 0x52, 0x51, 0x68, 0xf4, 0xdd, 0x62, 0x00, 0x68,
            0x01, 0x0d, 0x00, 0x00, 0x68, 0xd0, 0xdd, 0x62, 0x00, 0x68,
        ],
        "too-many-partitions path pushes the message at 0x62ddf4",
    ),
    (
        0x4da960,
        &[
            0x8b, 0x44, 0x24, 0x04, 0xd9, 0x44, 0x24, 0x08, 0x8b, 0x40, 0x38, 0xd8, 0x40, 0x3c,
            0x8b, 0x4c,
        ],
        "WorldOrigin: origin x/z stored, far x = origin + width",
    ),
    (
        0x4da9c0,
        &[
            0x8b, 0x44, 0x24, 0x04, 0xd9, 0x44, 0x24, 0x08, 0x8b, 0x40, 0x38, 0xd8, 0x40, 0x34,
            0x8b, 0x4c,
        ],
        "WorldExtents: width stored at +0x3c, far x = origin + width",
    ),
    (
        0x4dadf0,
        &[
            0x8b, 0x44, 0x24, 0x04, 0x8b, 0x40, 0x38, 0xd9, 0x40, 0x34, 0x8b, 0x4c, 0x24, 0x08,
            0xd8, 0x64,
        ],
        "WorldPartitionInclusionTolerance: the two padding words",
    ),
    (
        0x4dae40,
        &[
            0x8b, 0x44, 0x24, 0x04, 0x56, 0x8b, 0x70, 0x38, 0x8b, 0x44, 0x24, 0x0c, 0x3d, 0xff,
            0x00, 0x00,
        ],
        "WorldPartitionMaxDECFeatureCount: compared with 0xff before the byte store",
    ),
    (
        0x4da150,
        &[
            0x8b, 0x44, 0x24, 0x08, 0x85, 0xc0, 0x56, 0x57, 0x8b, 0x7c, 0x24, 0x0c, 0x8b, 0x77,
            0x38, 0x89, 0x46, 0x60, 0x74, 0x17, 0x57, 0xe8, 0x06, 0xff, 0xff, 0xff, 0x8b, 0x46,
            0x68, 0x8b, 0x4e, 0x64, 0x50, 0x51, 0x57, 0xe8, 0x08, 0x00, 0x00, 0x00,
        ],
        "WorldSetVirtualPartition: +0x60 = flag, then 0x4da070 and 0x4da180",
    ),
    (
        0x4da0d3,
        &[0xbd, 0x00, 0x80, 0x00, 0x00, 0x8b, 0x8f, 0x98],
        "virtual partition: ebp = 0x8000",
    ),
    (
        0x4da103,
        &[0x09, 0x68, 0x24],
        "virtual partition: or dword [obj+0x24], 0x8000 for every cell object",
    ),
    (
        0x4da180,
        &[
            0x8b, 0x44, 0x24, 0x04, 0x8b, 0x40, 0x38, 0x8b, 0x4c, 0x24, 0x08, 0x8b, 0x54, 0x24,
            0x0c, 0x56, 0x8b, 0xb0, 0x98, 0x00, 0x00, 0x00, 0x2b, 0xf1,
        ],
        "map size: +0x64 = x, +0x6c = cols - x (0x4da180 head)",
    ),
    (
        0x4dae80,
        &[
            0x51, 0x53, 0x55, 0x8b, 0x6c, 0x24, 0x10, 0x8b, 0x4d, 0x38, 0x56, 0x57, 0x8b, 0x7c,
            0x24, 0x24, 0x66, 0x8b, 0x07, 0x66, 0x3b, 0x47, 0x04, 0xc7, 0x44, 0x24, 0x10, 0x00,
            0x00, 0x00, 0x00, 0x89, 0x4c, 0x24, 0x18, 0x0f, 0x85, 0xf0, 0x00, 0x00,
        ],
        "inserter: world -> class data, then the cell-range words at [arg]",
    ),
    (
        0x4daf00,
        &[
            0x89, 0x74, 0x03, 0xf4, 0x8b, 0x4f, 0x3c, 0x8b, 0x44, 0x24, 0x28, 0x8b, 0x50, 0x04,
            0x89, 0x54, 0x19, 0xf8, 0x8b, 0x50, 0x10, 0x8b, 0x4f, 0x3c, 0x8b, 0x44, 0x24, 0x2c,
            0x89, 0x54, 0x19, 0xfc, 0x66, 0xff, 0x47, 0x3a,
        ],
        "entry[0] = node, entry[1..2] = the box's two y words, [cell+0x3a]++",
    ),
    (
        0x4da500,
        &[
            0x8b, 0x44, 0x24, 0x04, 0x53, 0x8b, 0x5c, 0x24, 0x0c, 0x56, 0x8b, 0x70, 0x38, 0xd9,
            0x86, 0x90, 0x00, 0x00, 0x00, 0x57, 0xd8, 0x03, 0x83, 0xec, 0x08, 0xd8, 0x66, 0x34,
            0xd8, 0x8e, 0x84, 0x00,
        ],
        "cell range: (box.min + [+0x90] - origin) * [+0x84]",
    ),
    (
        0x4db0c0,
        &[
            0x83, 0xec, 0x20, 0x53, 0x56, 0x57, 0x8b, 0x7c, 0x24, 0x34, 0x8b, 0x5f, 0x24, 0xf6,
            0xc3, 0x80,
        ],
        "insert node: flags bit 0x80 jumps to the whole-grid default",
    ),
    (
        0x4db1d5,
        &[
            0x8b, 0x74, 0x24, 0x30, 0x83, 0xc8, 0xff, 0x66, 0x89, 0x44, 0x24, 0x0e, 0x66, 0x89,
            0x44, 0x24, 0x0c, 0x8b, 0x46, 0x38, 0x66, 0x8b, 0x90, 0x98, 0x00, 0x00, 0x00, 0x66,
            0x89, 0x54, 0x24, 0x10,
        ],
        "whole-grid default: -1 .. [+0x98]/[+0x9c]",
    ),
    (
        0x4dacb0,
        &[
            0x8b, 0x44, 0x24, 0x04, 0x83, 0xec, 0x0c, 0x53, 0x55, 0x8b, 0x68, 0x38, 0x8b, 0x85,
            0xa0, 0x00, 0x00, 0x00, 0x33, 0xdb, 0x3b, 0xc3,
        ],
        "destroy: row table at +0xa0",
    ),
    (
        0x4e2f4a,
        &[
            0x68, 0xcc, 0x00, 0x00, 0x00, 0xc7, 0x44, 0x24, 0x14, 0x01, 0x00, 0x00, 0x00, 0xff,
            0xd5,
        ],
        "gClsReadNode world case: malloc 0xcc and read the header",
    ),
    (
        0x4e304a,
        &[
            0x8b, 0x46, 0x38, 0x8b, 0x88, 0x9c, 0x00, 0x00, 0x00, 0x6a, 0x04, 0x51, 0xff, 0x15,
            0xb4, 0x01, 0xa2, 0x00, 0x8b, 0x56, 0x38, 0x89,
        ],
        "reader: row table = calloc(rows, 4) into +0xa0",
    ),
    (
        0x4e30de,
        &[
            0x53, 0x6a, 0x01, 0x6a, 0x58, 0x57, 0xff, 0x15, 0xf8, 0x01, 0xa2, 0x00, 0x83, 0xc4,
            0x10, 0x83, 0xf8, 0x01, 0x0f, 0x85, 0x1d, 0x01, 0x00, 0x00, 0x66, 0x83, 0x7f, 0x3a,
            0x00, 0x7e, 0x26, 0x0f, 0xbf, 0x47, 0x3a, 0x8d, 0x04, 0x40, 0xc1, 0xe0,
        ],
        "reader: read a 0x58-byte cell, then its [cell+0x3a] entries",
    ),
    (
        0x4e3101,
        &[
            0x8d, 0x04, 0x40, 0xc1, 0xe0, 0x02, 0x50, 0xff, 0xd5, 0x8b, 0x4c, 0x24, 0x24, 0x0f,
            0xbf, 0x57, 0x3a, 0x51, 0x53, 0x52, 0x50, 0x89, 0x47, 0x3c, 0xe8, 0x42, 0x02, 0x00,
            0x00, 0x83, 0xc4, 0x14,
        ],
        "reader: entry array = count*12 at [cell+0x3c], then 0x4e3360",
    ),
    (
        0x4e3360,
        &[
            0x56, 0x57, 0x8b, 0x7c, 0x24, 0x10, 0x85, 0xff, 0x74, 0x6a, 0x8b, 0x44, 0x24, 0x14,
            0x8b, 0x74, 0x24, 0x0c, 0x50, 0x8d, 0x0c,
        ],
        "entry reader: 3 words (12 bytes) per entry",
    ),
    (
        0x5bc5f8,
        &[
            0x8b, 0x85, 0xb8, 0x00, 0x00, 0x00, 0x51, 0x52, 0x50, 0xe8, 0x1a, 0xe4, 0xf1, 0xff,
        ],
        "WorldPartition -> call 0x4daa20",
    ),
    (
        0x5bc454,
        &[
            0x8b, 0x85, 0xb8, 0x00, 0x00, 0x00, 0x51, 0x52, 0x50, 0xe8, 0x5e, 0xe5, 0xf1, 0xff,
        ],
        "WorldExtents -> call 0x4da9c0",
    ),
    (
        0x5bc4a3,
        &[
            0x8b, 0x85, 0xb8, 0x00, 0x00, 0x00, 0x51, 0x52, 0x50, 0xe8, 0xaf, 0xe4, 0xf1, 0xff,
        ],
        "WorldOrigin -> call 0x4da960",
    ),
    (
        0x5bc4f2,
        &[
            0x8b, 0x85, 0xb8, 0x00, 0x00, 0x00, 0x51, 0x52, 0x50, 0xe8, 0xf0, 0xe8, 0xf1, 0xff,
        ],
        "WorldPartitionInclusionTolerance -> call 0x4dadf0",
    ),
    (
        0x5bc52f,
        &[
            0x50, 0x51, 0xe8, 0x0a, 0xe9, 0xf1, 0xff, 0x83, 0xc4, 0x08, 0x5f, 0x5e, 0x5d,
        ],
        "WorldPartitionMaxDECFeatureCount -> call 0x4dae40",
    ),
    (
        0x5bca6b,
        &[
            0x8b, 0x95, 0xb8, 0x00, 0x00, 0x00, 0x50, 0x56, 0x52, 0xe8, 0x07, 0xd7, 0xf1, 0xff,
        ],
        "WorldSetVirtualPartitionMapSize -> call 0x4da180",
    ),
    (
        0x5bcaa7,
        &[
            0x50, 0x8b, 0x85, 0xb8, 0x00, 0x00, 0x00, 0x50, 0xe8, 0x9c, 0xd6, 0xf1, 0xff,
        ],
        "WorldSetVirtualPartition -> call 0x4da150",
    ),
    (
        0x5bc5a8,
        &[
            0x8b, 0x85, 0xb8, 0x00, 0x00, 0x00, 0x51, 0x52, 0x56, 0x50, 0xe8, 0xd9, 0xf1, 0xf1,
            0xff,
        ],
        "WorldPartitionSetActive -> call 0x4db790",
    ),
    (
        0x474151,
        &[
            0xc7, 0x05, 0xf4, 0xc3, 0x71, 0x00, 0x00, 0x00, 0xfa, 0x45, 0xc7, 0x05, 0xf0, 0xc3,
            0x71, 0x00, 0x00, 0x00, 0xa0, 0x41,
        ],
        "mission setup: [0x71c3f4] = 8000.0, then [0x71c3f0] = 20.0",
    ),
    (
        0x62ddf4,
        &[
            0x45, 0x72, 0x72, 0x6f, 0x72, 0x3a, 0x20, 0x74, 0x6f, 0x6f, 0x20, 0x6d, 0x61, 0x6e,
            0x79, 0x20, 0x77, 0x6f, 0x72, 0x6c, 0x64, 0x20, 0x61, 0x72, 0x65, 0x61, 0x20, 0x70,
            0x61, 0x72, 0x74, 0x69, 0x74, 0x69, 0x6f, 0x6e, 0x73, 0x3a, 0x20, 0x25, 0x64, 0x20,
            0x25, 0x64,
        ],
        "too-many-partitions message text",
    ),
    (
        0x62ddd0,
        &[
            0x44, 0x3a, 0x5c, 0x7a, 0x69, 0x70, 0x70, 0x65, 0x72, 0x5c, 0x67, 0x61, 0x6d, 0x65,
            0x7a, 0x5c, 0x7a, 0x63, 0x6c, 0x61, 0x73, 0x73, 0x5c, 0x63, 0x6c, 0x73, 0x5f, 0x77,
        ],
        "the builder's source: zclass\\cls_world.c",
    ),
    (
        0x63e2b8,
        &[
            0x57, 0x6f, 0x72, 0x6c, 0x64, 0x50, 0x61, 0x72, 0x74, 0x69, 0x74, 0x69, 0x6f, 0x6e,
            0x00, 0x00, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x50, 0x61, 0x72, 0x74, 0x69, 0x74, 0x69,
            0x6f, 0x6e, 0x53, 0x65, 0x74, 0x41, 0x63, 0x74, 0x69, 0x76, 0x65, 0x00, 0x57, 0x6f,
            0x72, 0x6c, 0x64, 0x50, 0x61, 0x72,
        ],
        "the command names WorldPartition and WorldPartitionSetActive",
    ),
    (
        0x63e2e0,
        &[
            0x57, 0x6f, 0x72, 0x6c, 0x64, 0x50, 0x61, 0x72, 0x74, 0x69, 0x74, 0x69, 0x6f, 0x6e,
            0x4d, 0x61, 0x78, 0x44, 0x45, 0x43, 0x46, 0x65, 0x61, 0x74, 0x75, 0x72, 0x65, 0x43,
            0x6f, 0x75, 0x6e, 0x74, 0x00, 0x00, 0x00, 0x00, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x50,
            0x61, 0x72, 0x74, 0x69, 0x74, 0x69, 0x6f, 0x6e, 0x49, 0x6e, 0x63, 0x6c, 0x75, 0x73,
            0x69, 0x6f, 0x6e, 0x54, 0x6f, 0x6c, 0x65, 0x72,
        ],
        "WorldPartitionMaxDECFeatureCount and WorldPartitionInclusionTolerance",
    ),
    (
        0x63e328,
        &[
            0x57, 0x6f, 0x72, 0x6c, 0x64, 0x4f, 0x72, 0x69, 0x67, 0x69, 0x6e, 0x00, 0x57, 0x6f,
            0x72, 0x6c, 0x64, 0x45, 0x78, 0x74, 0x65, 0x6e, 0x74, 0x73, 0x00, 0x00, 0x00, 0x00,
            0x57, 0x6f, 0x72, 0x6c, 0x64, 0x41, 0x64, 0x64, 0x4c, 0x69, 0x67, 0x68,
        ],
        "WorldOrigin, WorldExtents, WorldAddLight",
    ),
    (
        0x63e154,
        &[
            0x57, 0x6f, 0x72, 0x6c, 0x64, 0x53, 0x65, 0x74, 0x56, 0x69, 0x72, 0x74, 0x75, 0x61,
            0x6c, 0x50, 0x61, 0x72, 0x74, 0x69, 0x74, 0x69, 0x6f, 0x6e, 0x00, 0x00, 0x00, 0x00,
            0x57, 0x6f, 0x72, 0x6c, 0x64, 0x53, 0x65, 0x74, 0x56, 0x69, 0x72, 0x74, 0x75, 0x61,
            0x6c, 0x50,
        ],
        "WorldSetVirtualPartition and WorldSetVirtualPartitionMapSize",
    ),
];

/// No window is empty, odd-length or a duplicate: a typo in a citation would
/// otherwise compare fewer bytes than the finding claims. Runs on CI, where
/// the image is absent.
#[test]
fn accept_m01_ai_terrain_grid_windows_are_well_formed() {
    let mut seen = std::collections::BTreeSet::new();
    for (va, bytes, what) in WINDOWS {
        assert!(!bytes.is_empty(), "{what}");
        assert!(seen.insert(*va), "{what}: duplicate window {va:#x}");
    }
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_ai_terrain_grid_windows_match_the_image() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(image.digest, original_image_digest());
    for (va, expected, what) in WINDOWS {
        assert_eq!(
            bytes_at(&image.bytes, *va, expected.len()),
            *expected,
            "{va:#x}: {what}"
        );
    }
}

/// The builder's own constants, and the two words the floor/ceiling writer
/// stores: a reader that trusted the immediates in a window would not notice
/// a drifted constant elsewhere in the image.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_ai_terrain_grid_constants_are_the_image_words() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;
    assert_eq!(f32_at(bytes, 0x60802c), 0.125, "default x tolerance factor");
    assert_eq!(
        f32_at(bytes, 0x608dc8),
        -0.125,
        "default z tolerance factor"
    );
    assert_eq!(f32_at(bytes, 0x6034f8), -0.5, "half-diagonal factor");
    assert_eq!(f32_at(bytes, 0x6032e0), 0.5, "half-cell factor");
    assert_eq!(f32_at(bytes, 0x6032dc), 1.0, "one / cell size");
    // `[0x71c3f4]` = 8000.0 and `[0x71c3f0]` = 20.0, as the immediates are
    // read by `0x474151`/`0x47415b`.
    assert_eq!(u32_at(bytes, 0x474153), 0x71c3f4, "the ceiling's global");
    assert_eq!(u32_at(bytes, 0x474157), 0x45fa0000, "ceiling 8000.0");
    assert_eq!(u32_at(bytes, 0x47415d), 0x71c3f0, "the floor's global");
    assert_eq!(u32_at(bytes, 0x474161), 0x41a00000, "floor 20.0");
    assert_eq!(f32::from_bits(0x45fa0000), 8000.0);
    assert_eq!(f32::from_bits(0x41a00000), 20.0);
}

/// `support\c3\load.gw` in `ZBD/interp.zbd` is where C3's partition
/// parameters come from: the six commands and their exact tokens and
/// offsets, read through the production decoding reader.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_ai_terrain_grid_m01_script_sets_the_partition() {
    let root = game_dir();
    let bytes = install_file(&root, "ZBD/interp.zbd");
    let file = decode_interp(&mut ParseContext::with_defaults("ZBD/interp.zbd"), &bytes)
        .expect("the installation's interp container decodes");

    let load = file
        .scripts()
        .iter()
        .find(|script| script.name() == b"support\\c3\\load.gw")
        .expect("support\\c3\\load.gw is in the container");

    // The measured lines: (data offset, size, tokens).
    let want: &[(u64, u32, &[&[u8]])] = &[
        (
            112847,
            25,
            &[b"WorldOrigin".as_slice(), b"-16384.0", b"0.0"],
        ),
        (
            112880,
            30,
            &[b"WorldExtents".as_slice(), b"16384.0", b"-16384.0"],
        ),
        (
            112918,
            26,
            &[b"WorldPartition".as_slice(), b"1024", b"-1024"],
        ),
        (
            112952,
            41,
            &[
                b"WorldPartitionInclusionTolerance".as_slice(),
                b"3.0",
                b"3.0",
            ],
        ),
        (113100, 28, &[b"WorldSetVirtualPartition".as_slice(), b"on"]),
        (
            113136,
            36,
            &[b"WorldSetVirtualPartitionMapSize".as_slice(), b"1", b"1"],
        ),
    ];
    for (offset, size, tokens) in want {
        let line = load
            .lines()
            .iter()
            .find(|line| line.data_offset() == *offset)
            .unwrap_or_else(|| panic!("no line at data offset {offset}"));
        assert_eq!(line.size(), *size, "line at {offset} size");
        let got: Vec<&[u8]> = line.tokens().iter().map(|token| token.bytes()).collect();
        assert_eq!(got, *tokens, "line at {offset} tokens");
    }

    // M01's own script only moves the class "active" flag; it does not change
    // membership, and its rectangle does not contain cell (4, 12).
    let m01 = file
        .scripts()
        .iter()
        .find(|script| script.name() == b"support\\c3\\m01.gw")
        .expect("support\\c3\\m01.gw is in the container");
    let active = m01
        .lines()
        .iter()
        .find(|line| line.data_offset() == 120833)
        .expect("the WorldPartitionSetActive line");
    let tokens: Vec<&[u8]> = active.tokens().iter().map(|token| token.bytes()).collect();
    assert_eq!(
        tokens,
        [
            b"WorldPartitionSetActive".as_slice(),
            b"off",
            b"-15360",
            b"-7168",
            b"-8192",
            b"-14336"
        ]
    );
}

fn world_node(nodes: &[RawNode]) -> &RawNode {
    nodes
        .iter()
        .find(|node| matches!(node.kind, NodeKind::World(_)))
        .expect("the container holds one world node")
}

/// The C3 world record carries the whole grid: dimensions, origin, cell size,
/// the virtual-partition words, all 256 cells and their 439 entries, and the
/// hillside cell holds the hillside while the shipwreck ring and cliffs are in
/// no cell at all.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_ai_terrain_grid_c3_world_record_carries_the_grid() {
    let root = game_dir();
    let bytes = install_file(&root, "ZBD/C3/gamez.zbd");
    let mut ctx = ParseContext::with_defaults("ZBD/C3/gamez.zbd");
    let nodes = read_gamez_nodes(&mut ctx, &bytes).expect("the container's node array reads");
    let mut mesh_ctx = ParseContext::with_defaults("ZBD/C3/gamez.zbd");
    let meshes = read_gamez_meshes(&mut mesh_ctx, "ZBD/C3/gamez.zbd", &bytes)
        .expect("the container's mesh array reads");

    let world = world_node(&nodes.nodes);
    assert_eq!(world.name, "world1");
    assert_eq!(world.info.node_type, NODE_TYPE_WORLD);
    assert_eq!(world.index, 0);
    assert_eq!(world.data_offset, 4_808_244);
    assert_eq!(world.data_bytes, 28_060);
    assert_eq!(world.info.children_count, 14);
    let NodeKind::World(grid) = world.kind else {
        unreachable!("world_node returned a world node");
    };
    assert_eq!(grid.partition_x_count, 16);
    assert_eq!(grid.partition_y_count, 16);
    assert_eq!(grid.partition_bytes, 27_796);
    assert_eq!(grid.partition_values, 439);

    let record = usize::try_from(world.data_offset).expect("a file offset");
    let header = &bytes[record..record + 0xcc];
    assert_eq!(le_f32(header, 0x34), -16384.0, "origin x");
    assert_eq!(le_f32(header, 0x38), 0.0, "origin z");
    assert_eq!(le_f32(header, 0x3c), 16384.0, "width");
    assert_eq!(
        le_f32(header, 0x40),
        -16384.0,
        "depth (negative: rows run to -z)"
    );
    assert_eq!(le_f32(header, 0x44), 0.0, "far x");
    assert_eq!(le_f32(header, 0x48), -16384.0, "far z");
    assert_eq!(le_f32(header, 0x74), 1024.0, "cell size x");
    assert_eq!(le_f32(header, 0x78), -1024.0, "cell size z");
    assert_eq!(le_f32(header, 0x84), 1.0 / 1024.0, "inverse cell size x");
    assert_eq!(le_f32(header, 0x88), -1.0 / 1024.0, "inverse cell size z");
    assert_eq!(
        le_f32(header, 0x8c),
        -768.0,
        "half diagonal (fast inverse sqrt)"
    );
    assert_eq!(le_f32(header, 0x90), 3.0, "inclusion tolerance x");
    assert_eq!(le_f32(header, 0x94), 3.0, "inclusion tolerance z");
    assert_eq!(le_u32(header, 0x60), 1, "virtual partition on");
    assert_eq!(le_u32(header, 0x64), 1, "virtual map size x");
    assert_eq!(le_u32(header, 0x68), 1, "virtual map size z");
    assert_eq!(le_u32(header, 0x6c), 15, "cols - map size x");
    assert_eq!(le_u32(header, 0x70), 15, "rows - map size z");
    assert_eq!(header[0x5c], 16, "max DEC feature count");
    assert_eq!(le_u32(header, 0x98), 16, "columns");
    assert_eq!(le_u32(header, 0x9c), 16, "rows");
    // The builder's arithmetic and the script agree with the stored words.
    assert_eq!(le_f32(header, 0x3c) / le_f32(header, 0x74), 16.0);
    assert_eq!(le_f32(header, 0x40) / le_f32(header, 0x78), 16.0);

    // Walk the block exactly as `gClsReadNode` does.
    let origin_x = le_f32(header, 0x34);
    let origin_z = le_f32(header, 0x38);
    let size_x = le_f32(header, 0x74);
    let size_z = le_f32(header, 0x78);
    let mut at = usize::try_from(world.data_offset).expect("offset") + 0xcc + 4;
    // One cell's listed `(node index, subtree min y, subtree max y)` entries.
    type CellEntries = Vec<(u32, f32, f32)>;
    let mut cells: std::collections::BTreeMap<(usize, usize), CellEntries> =
        std::collections::BTreeMap::new();
    let mut cell_count = 0_usize;
    for row in 0..16_usize {
        for col in 0..16_usize {
            let cell = &bytes[at..at + 0x58];
            assert_eq!(
                le_u32(cell, 0x00),
                0x100,
                "cell flags: the builder's marker"
            );
            assert_eq!(
                le_f32(cell, 0x08),
                origin_x + col as f32 * size_x,
                "cell ({row}, {col}) min x"
            );
            assert_eq!(
                le_f32(cell, 0x0c),
                origin_z + row as f32 * size_z,
                "cell ({row}, {col}) min z"
            );
            assert_eq!(
                le_f32(cell, 0x1c),
                origin_x + (col as f32 + 1.0) * size_x,
                "cell ({row}, {col}) max x"
            );
            assert_eq!(
                le_f32(cell, 0x18),
                origin_z + (row as f32 + 1.0) * size_z,
                "cell ({row}, {col}) max z"
            );
            let count = i16::from_le_bytes([cell[0x3a], cell[0x3b]]);
            assert!(count >= 0, "entry count is an int16 at +0x3a");
            at += 0x58;
            let mut entries = Vec::new();
            for i in 0..count as usize {
                let entry = &bytes[at + i * 12..at + i * 12 + 12];
                let node = le_u32(entry, 0);
                let min_y = f32::from_bits(le_u32(entry, 4));
                let max_y = f32::from_bits(le_u32(entry, 8));
                entries.push((node, min_y, max_y));
            }
            at += count as usize * 12;
            cell_count += 1;
            cells.insert((row, col), entries);
        }
    }
    assert_eq!(cell_count, 256);
    assert_eq!(
        at,
        usize::try_from(world.data_offset).expect("offset") + 0xcc + 4 + 27_796,
        "the cell walk ends exactly on the child slots"
    );

    let total: usize = cells.values().map(Vec::len).sum();
    assert_eq!(total, 439, "one entry per listed node");
    // Every entry names a node, and that node's own AreaPartitionPm names the
    // cell it is listed in: (col, row, virtual col, virtual row).
    let mut flagged_8000 = 0_usize;
    for ((row, col), entries) in &cells {
        for (slot, _, _) in entries {
            let node = nodes
                .nodes
                .iter()
                .find(|node| node.index == *slot)
                .unwrap_or_else(|| panic!("entry {slot} names no node"));
            assert_eq!(
                node.info.area_partition,
                [*col as i16, *row as i16, *col as i16, *row as i16],
                "node {slot} AreaPartitionPm names the cell it is listed in"
            );
            assert_eq!(
                node.info.flags & 0x14,
                0x14,
                "node {slot} carries the query's two required bits (4 and 0x10)"
            );
            if node.info.flags & 0x8000 != 0 {
                flagged_8000 += 1;
            }
        }
    }
    assert_eq!(
        flagged_8000, 430,
        "430 of the 439 cell members carry the virtual-partition bit"
    );

    // No node outside the grid carries 0x8000.
    let listed: std::collections::BTreeSet<u32> =
        cells.values().flatten().map(|(slot, _, _)| *slot).collect();
    for node in &nodes.nodes {
        if !listed.contains(&node.index) {
            assert_eq!(
                node.info.flags & 0x8000,
                0,
                "non-member {} carries the virtual-partition bit",
                node.index
            );
        }
    }

    // The hillside cell and its neighbour.
    let hillside = &cells[&(4, 12)];
    assert_eq!(
        hillside
            .iter()
            .map(|(slot, _, _)| *slot)
            .collect::<Vec<_>>(),
        vec![829, 871, 2874],
        "cell (4, 12): two flat tiles and the hillside group"
    );
    let (_, min_y, max_y) = hillside[2];
    assert!((min_y - -3.5746138).abs() < 0.001, "hillside min y {min_y}");
    assert!((max_y - 180.47455).abs() < 0.001, "hillside max y {max_y}");

    let ring = &cells[&(5, 12)];
    assert_eq!(
        ring.iter().map(|(slot, _, _)| *slot).collect::<Vec<_>>(),
        vec![830, 881, 2873],
        "cell (5, 12) is the next hill tile"
    );

    // The entry's two words are the subtree's y range: check against code
    // that reads the node's own mesh rather than the cell record.
    let node = |slot: u32| {
        nodes
            .nodes
            .iter()
            .find(|node| node.index == slot)
            .expect("a node")
    };
    let mesh_range = |slot: u32| -> (f32, f32) {
        let mesh_index = u32::try_from(node(slot).info.mesh_index).expect("a mesh");
        let mesh = meshes.get(mesh_index).expect("the node's mesh");
        mesh.mesh
            .positions
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p[1]), hi.max(p[1]))
            })
    };
    let (lo, hi) = mesh_range(2873);
    assert!(
        (lo - ring[2].1).abs() < 0.001 && (hi - ring[2].2).abs() < 0.001,
        "slot 2873's mesh y range {lo}..{hi} is the entry's y range"
    );
    // `g28604` is a group; its listed range is the range of its child mesh.
    let child = node(2874).children[0];
    let (lo, hi) = node(2874)
        .children
        .iter()
        .filter(|slot| node(**slot).info.mesh_index >= 0)
        .map(|slot| mesh_range(*slot))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), (a, b)| {
            (lo.min(a), hi.max(b))
        });
    assert!(
        (lo - hillside[2].1).abs() < 0.001 && (hi - hillside[2].2).abs() < 0.001,
        "slot 2874's listed range {min_y}..{max_y} is its child mesh's {lo}..{hi} (child {child})"
    );

    // The shipwreck's mountain ring and the east cliffs are drawn but are in
    // no cell: no AreaPartitionPm, and no virtual-partition bit.
    for slot in [2994_u32, 2965] {
        let node = nodes
            .nodes
            .iter()
            .find(|node| node.index == slot)
            .expect("the terrain node");
        assert!(
            !listed.contains(&slot),
            "slot {slot} ({}) must not be in the grid",
            node.name
        );
        assert_eq!(
            node.info.area_partition,
            [-1, -1, 0, 0],
            "slot {slot} declares the default partition"
        );
        assert_eq!(
            node.info.flags & 0x8000,
            0,
            "slot {slot} carries no virtual-partition bit"
        );
        assert!(
            node.info.mesh_index >= 0,
            "slot {slot} is a mesh node, so its absence is not a mesh artefact"
        );
    }
}
