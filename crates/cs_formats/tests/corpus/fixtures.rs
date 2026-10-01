//! Authored synthetic bytes for every declared corpus container, with the
//! span map that resolves the container's declared boundary kinds to real
//! offsets.
//!
//! Every byte here is authored in this file — no original game data. The
//! recipes mirror the format tests' own builders (`tests/rof.rs`,
//! `tests/interp.rs`, `tests/zbd/`, `tests/texture/`, `tests/gamez/`) so a
//! drift between the two reads the same way a spec violation does.

use cs_formats::zbd::{INTERP_SIGNATURE, INTERP_VERSION};
use cs_xtask::corpus::BoundaryKind;

use crate::spans::{CorpusFixture, frame, hard, slack};

const HEADER: BoundaryKind = BoundaryKind::FixedHeader;
const TABLE: BoundaryKind = BoundaryKind::VariableTable;
const TRAILER: BoundaryKind = BoundaryKind::Trailer;

fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// ROF (`src/rof.rs`): u32 entry_count, u32 names_length, then 24-byte
// records (start, decoded len, stored len, flags, name_len, id) and a
// NUL-terminated name table that must tile `names_length` exactly.
// ---------------------------------------------------------------------------

/// Two-entry directory block: the smallest table that is still a table.
pub fn rof_directory() -> CorpusFixture {
    let mut bytes = Vec::new();
    words(&mut bytes, &[2, 4]);
    for id in [1u32, 2] {
        words(&mut bytes, &[0, 0, 0, 0, 2, id]);
    }
    bytes.extend_from_slice(b"A\0B\0");
    CorpusFixture::new(
        bytes,
        &[hard(HEADER, 0..8), hard(TABLE, 8..56), hard(TABLE, 56..60)],
    )
}

/// The smallest whole archive `read_tree` accepts: a root block and one
/// file member whose stored extent sits right after the block.
pub fn rof_tree() -> CorpusFixture {
    let mut bytes = Vec::new();
    words(&mut bytes, &[1, 5]);
    words(&mut bytes, &[37, 6, 6, 0, 5, 7]);
    bytes.extend_from_slice(b"PLAN\0");
    bytes.extend_from_slice(b"flight");
    CorpusFixture::new(
        bytes,
        &[hard(HEADER, 0..8), hard(TABLE, 8..37), hard(TABLE, 37..43)],
    )
}

/// The same archive read through `read_member`: the member extent is the
/// load-bearing span; the trailing slack demonstrates the bounded-any
/// region the oracle also requires.
pub fn rof_member() -> CorpusFixture {
    let mut fixture = rof_tree();
    fixture.bytes.extend_from_slice(b"pad");
    fixture.spans = vec![hard(TABLE, 37..43), slack(43..46)];
    fixture
}

// ---------------------------------------------------------------------------
// ZBD (`src/zbd/`).
// ---------------------------------------------------------------------------

/// A signature-family header a probe dispatch accepts: INTERP signature +
/// version at the observed offsets, then bytes past the signature window
/// the dispatch never reads.
pub fn zbd_dispatch() -> CorpusFixture {
    let mut bytes = Vec::new();
    words(&mut bytes, &[INTERP_SIGNATURE, INTERP_VERSION, 1]);
    bytes.extend_from_slice(&[0xAA; 4]);
    CorpusFixture::new(bytes, &[hard(HEADER, 0..8), slack(8..16)])
}

/// A reader-archive container: member extents are described by the member
/// table input, so the bytes are pure member payload plus unreferenced
/// tail. The extent's span is the variable-length table boundary.
pub fn zbd_reader_archive() -> CorpusFixture {
    let mut bytes = b"readme-now".to_vec();
    bytes.extend_from_slice(&[0xAA; 4]);
    CorpusFixture::new(bytes, &[hard(TABLE, 0..10), slack(10..14)])
}

/// The same for the sound family.
pub fn zbd_sound_archive() -> CorpusFixture {
    let mut bytes = b"sound-data".to_vec();
    bytes.extend_from_slice(&[0xAA; 4]);
    CorpusFixture::new(bytes, &[hard(TABLE, 0..10), slack(10..14)])
}

/// A version-one trailer index over one member: `[member][count x 148-byte
/// entries][u32 version = 1][u32 count]`. The unexplained 76 bytes carry a
/// fill pattern so no truncated tail can re-read as `version = 1, count
/// fits` — that is what would let a cut parse a bogus index.
pub fn zbd_trailer_index() -> CorpusFixture {
    let mut bytes = b"M".to_vec();
    // entry: start, length, 64-byte name, 76 unexplained bytes.
    words(&mut bytes, &[0, 1]);
    let mut name = [0u8; 64];
    name[..3].copy_from_slice(b"doc");
    bytes.extend_from_slice(&name);
    bytes.extend_from_slice(&[0x5Au8; 76]);
    words(&mut bytes, &[1, 1]); // version one, one entry
    assert_eq!(bytes.len(), 157);
    CorpusFixture::new(
        bytes,
        &[
            hard(TABLE, 0..1),
            hard(TABLE, 1..149),
            hard(TRAILER, 149..157),
        ],
    )
}

/// The smallest RIFF/WAVE header `read_wave_header` accepts: `RIFF` +
/// `size == len - 8` + `WAVE`, a 16-byte `fmt ` chunk and an empty `data`
/// chunk.
pub fn zbd_wave_header() -> CorpusFixture {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    words(&mut bytes, &[36]);
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    words(&mut bytes, &[16]);
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    words(&mut bytes, &[8000, 16000]); // rate, byte rate
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits
    bytes.extend_from_slice(b"data");
    words(&mut bytes, &[0]);
    assert_eq!(bytes.len(), 44);
    CorpusFixture::new(bytes, &[hard(HEADER, 0..12), hard(TABLE, 12..44)])
}

// ---------------------------------------------------------------------------
// INTERP (`src/interp.rs`): 12-byte header, 128-byte index entries, then
// per-script line records ending in a zero u32.
// ---------------------------------------------------------------------------

/// One-script container with a zero-line script: header + index entry +
/// four-byte terminator.
pub fn interp_container() -> CorpusFixture {
    let mut bytes = Vec::new();
    words(&mut bytes, &[INTERP_SIGNATURE, INTERP_VERSION, 1]);
    let mut name = [0u8; 120];
    name[..4].copy_from_slice(b"corp");
    bytes.extend_from_slice(&name);
    words(&mut bytes, &[0x5EED, 140]); // timestamp, script_offset
    words(&mut bytes, &[0]); // zero-line terminator
    assert_eq!(bytes.len(), 144);
    CorpusFixture::new(
        bytes,
        &[
            hard(HEADER, 0..12),
            hard(TABLE, 12..140),
            hard(TABLE, 140..144),
        ],
    )
}

// ---------------------------------------------------------------------------
// BM (`src/bm.rs`): 4-byte header then one plane row per 1-byte layer, five
// planes for a 1x1 frame (base 3 + three masks + overlay 4). The trailing
// bytes are tolerated and kept as a diagnostic — the slack span.
// ---------------------------------------------------------------------------

pub fn bm_image() -> CorpusFixture {
    let mut bytes = vec![1u8, 0, 1, 0];
    bytes.extend_from_slice(&[0x01, 0x02, 0x03]); // base, big-endian channels
    bytes.extend_from_slice(&[0xF0, 0x0F, 0x3C]); // masks
    bytes.extend_from_slice(&[0x00, 0x12, 0x34, 0x56]); // overlay
    bytes.extend_from_slice(&[0xAA, 0xBB, 0xCC]); // tolerated tail
    assert_eq!(bytes.len(), 17);
    CorpusFixture::new(
        bytes,
        &[hard(HEADER, 0..4), hard(TABLE, 4..14), slack(14..17)],
    )
}

// ---------------------------------------------------------------------------
// Textures (`src/texture/`).
// ---------------------------------------------------------------------------

/// The smallest BMP in the supported subset: 1x1 4bpp BI_RGB with a
/// one-entry palette; `file_size` covers the whole 62 bytes.
pub fn texture_bmp() -> CorpusFixture {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"BM");
    words(&mut bytes, &[62]); // file size
    bytes.extend_from_slice(&[0; 4]); // reserved
    words(&mut bytes, &[58]); // pixel offset
    words(&mut bytes, &[40]); // info header size
    bytes.extend_from_slice(&1i32.to_le_bytes()); // width
    bytes.extend_from_slice(&1i32.to_le_bytes()); // height
    bytes.extend_from_slice(&1u16.to_le_bytes()); // planes
    bytes.extend_from_slice(&4u16.to_le_bytes()); // bits per pixel
    words(&mut bytes, &[0]); // BI_RGB
    words(&mut bytes, &[4]); // image size: stride * height
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    words(&mut bytes, &[1, 0]); // colors used, important
    bytes.extend_from_slice(&[0x20, 0x30, 0x40, 0]); // one BGR0 palette entry
    bytes.extend_from_slice(&[0; 4]); // one row: index 0 + zero padding
    assert_eq!(bytes.len(), 62);
    CorpusFixture::new(bytes, &[hard(HEADER, 0..54), hard(TABLE, 54..62)])
}

/// The smallest TGA: an 18-byte header and a 1x1 uncompressed true-color
/// pixel.
pub fn texture_tga() -> CorpusFixture {
    let mut bytes = vec![0u8, 0, 2]; // id length, no color map, true color
    bytes.extend_from_slice(&[0; 5]); // color map spec
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x origin
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y origin
    bytes.extend_from_slice(&1u16.to_le_bytes()); // width
    bytes.extend_from_slice(&1u16.to_le_bytes()); // height
    bytes.push(24); // pixel depth
    bytes.push(0); // descriptor
    bytes.extend_from_slice(&[0x10, 0x20, 0x30]); // B, G, R
    assert_eq!(bytes.len(), 21);
    CorpusFixture::new(bytes, &[hard(HEADER, 0..18), hard(TABLE, 18..21)])
}

/// One-texture ZBD texture package: 24-byte header, one 40-byte entry and
/// the contiguous texture data (16-byte info + 2-byte RGB565 level).
pub fn texture_zbd_package() -> CorpusFixture {
    let mut bytes = Vec::new();
    words(&mut bytes, &[0, 1, 0, 1, 0, 0]);
    let mut name = [0u8; 32];
    name[..1].copy_from_slice(b"a");
    bytes.extend_from_slice(&name);
    words(&mut bytes, &[64]);
    bytes.extend_from_slice(&(-1i32).to_le_bytes()); // no palette index
    words(&mut bytes, &[0x5]); // bytes-per-pixel 2 | no alpha
    bytes.extend_from_slice(&1u16.to_le_bytes()); // width
    bytes.extend_from_slice(&1u16.to_le_bytes()); // height
    words(&mut bytes, &[0]); // zero08
    bytes.extend_from_slice(&0u16.to_le_bytes()); // palette count
    bytes.extend_from_slice(&0u16.to_le_bytes()); // stretch
    bytes.extend_from_slice(&0x1234u16.to_le_bytes()); // one RGB565 texel
    assert_eq!(bytes.len(), 82);
    CorpusFixture::new(
        bytes,
        &[
            hard(HEADER, 0..24),
            hard(TABLE, 24..64),
            hard(TABLE, 64..82),
        ],
    )
}

// ---------------------------------------------------------------------------
// GameZ (`src/gamez/`): ten-word header, then textures / materials /
// meshes / nodes at the declared offsets.
// ---------------------------------------------------------------------------

/// The smallest mesh container: one stub mesh (array_size 1, count 0) whose
/// record is all-zero and whose trailer is the expected `-1` index. The
/// bytes after `nodes_offset` are the tolerated node array — slack.
pub fn gamez_meshes() -> CorpusFixture {
    const MESHES_OFFSET: u32 = 42;
    let nodes_offset = MESHES_OFFSET + 12 + 104;
    let mut bytes = Vec::new();
    words(
        &mut bytes,
        &[
            0x0297_1222, // GAMEZ_SIGNATURE
            42,          // GAMEZ_VERSION
            1_234_567_890,
            0,  // texture_count
            40, // textures_offset
            41, // materials_offset
            MESHES_OFFSET,
            7, // node_array_size
            0, // light_index
            nodes_offset,
        ],
    );
    bytes.extend_from_slice(&[0u8; (MESHES_OFFSET as usize) - 40]);
    for word in [1i32, 0, -1] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&[0u8; 100]);
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    bytes.extend_from_slice(&[0xAA; 4]); // tolerated node-array tail
    assert_eq!(bytes.len(), (nodes_offset + 4) as usize);
    let len = bytes.len();
    CorpusFixture::new(
        bytes,
        &[
            hard(HEADER, 0..40),
            hard(TABLE, 40..nodes_offset as usize),
            slack(nodes_offset as usize..len),
        ],
    )
}

/// The smallest materials container: one texture-name record, a full
/// 1000-slot material array holding zero present materials, then the same
/// stub mesh so the header's section offsets stay strictly increasing.
pub fn gamez_materials() -> CorpusFixture {
    const NG_MATERIAL_SLOTS: usize = 1000;
    const MATERIAL_SLOT_BYTES: usize = 44;

    let mut bytes = Vec::new();
    let textures_offset = 40usize;
    let materials_offset = textures_offset + 44;
    let meshes_offset = materials_offset + 16 + NG_MATERIAL_SLOTS * MATERIAL_SLOT_BYTES;
    let nodes_offset = meshes_offset + 12 + 104;

    words(
        &mut bytes,
        &[
            0x0297_1222,
            42,
            1_234_567_890,
            1, // texture_count
            textures_offset as u32,
            materials_offset as u32,
            meshes_offset as u32,
            7,
            0,
            nodes_offset as u32,
        ],
    );

    // TextureInfoNgC: three words, the 20-byte name, used/index/unk40.
    words(&mut bytes, &[0, 0, 0]);
    let mut name = [0u8; 20];
    name[..4].copy_from_slice(b"corp");
    bytes.extend_from_slice(&name);
    words(&mut bytes, &[2, 0]);
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    assert_eq!(bytes.len(), materials_offset);

    // MaterialInfo header for an empty array: (0, 0, 0, -1).
    for word in [0i32, 0, 0, -1] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    // Every slot is a free record (only MATERIAL_FLAG_FREE set at byte 1)
    // plus the free-list link pair.
    for index in 0..NG_MATERIAL_SLOTS {
        let mut record = [0u8; 40];
        record[1] = 1 << 5;
        bytes.extend_from_slice(&record);
        let link1: i16 = if index == 0 { -1 } else { (index - 1) as i16 };
        let link2: i16 = if index + 1 >= NG_MATERIAL_SLOTS {
            -1
        } else {
            (index + 1) as i16
        };
        bytes.extend_from_slice(&link1.to_le_bytes());
        bytes.extend_from_slice(&link2.to_le_bytes());
    }
    assert_eq!(bytes.len(), meshes_offset);

    // The stub mesh the mesh reader demands; materials does not read it,
    // but the header requires the section to exist.
    for word in [1i32, 0, -1] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&[0u8; 100]);
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    assert_eq!(bytes.len(), nodes_offset);
    bytes.extend_from_slice(&[0xAA; 4]);
    let len = bytes.len();

    CorpusFixture::new(
        bytes,
        &[
            hard(HEADER, 0..40),
            hard(TABLE, 40..meshes_offset),
            hard(TABLE, meshes_offset..nodes_offset),
            slack(nodes_offset..len),
        ],
    )
}

// ---------------------------------------------------------------------------
// PE (`src/pe_resources.rs`): DOS stub with `e_lfanew`, PE signature, COFF
// header, PE32 optional header and the section table.
// ---------------------------------------------------------------------------

/// Writes the DOS+PE header skeleton shared by the two PE fixtures and
/// returns the bytes positioned at the section table.
fn pe_skeleton(
    number_of_sections: u16,
    number_of_rva_and_sizes: u32,
    size_of_headers: u32,
    resource_directory: Option<(u32, u32)>,
) -> Vec<u8> {
    let mut bytes = vec![0u8; 0x80];
    bytes[0..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes.extend_from_slice(b"PE\0\0");
    // COFF header.
    bytes.extend_from_slice(&0x014cu16.to_le_bytes()); // machine: i386
    bytes.extend_from_slice(&number_of_sections.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&224u16.to_le_bytes()); // PE32 optional header
    bytes.extend_from_slice(&[0; 2]); // characteristics
    // PE32 optional header (224 bytes).
    let optional = bytes.len();
    bytes.extend_from_slice(&[0u8; 224]);
    bytes[optional..optional + 2].copy_from_slice(&0x010bu16.to_le_bytes());
    bytes[optional + 60..optional + 64].copy_from_slice(&size_of_headers.to_le_bytes());
    bytes[optional + 92..optional + 96].copy_from_slice(&number_of_rva_and_sizes.to_le_bytes());
    if let Some((rva, size)) = resource_directory {
        let dir = optional + 96 + 2 * 8;
        bytes[dir..dir + 4].copy_from_slice(&rva.to_le_bytes());
        bytes[dir + 4..dir + 8].copy_from_slice(&size.to_le_bytes());
    }
    bytes
}

/// A 40-byte section row.
fn pe_section_row(name: &[u8], rva: u32, virtual_size: u32, raw: Range32) -> Vec<u8> {
    let mut row = vec![0u8; 40];
    row[..name.len()].copy_from_slice(name);
    row[8..12].copy_from_slice(&virtual_size.to_le_bytes());
    row[12..16].copy_from_slice(&rva.to_le_bytes());
    row[16..20].copy_from_slice(&raw.size.to_le_bytes());
    row[20..24].copy_from_slice(&raw.pointer.to_le_bytes());
    row
}

struct Range32 {
    pointer: u32,
    size: u32,
}

/// The smallest PE the layout reader accepts: one `.text` section whose
/// raw bytes sit behind the headers; the bytes past the section table are
/// tolerated — slack.
pub fn pe_layout() -> CorpusFixture {
    let section_table = 0x80 + 4 + 20 + 224;
    let header_end = section_table + 40;
    let mut bytes = pe_skeleton(1, 0, header_end as u32, None);
    bytes.extend_from_slice(&pe_section_row(
        b".text\0\0\0",
        0x1000,
        16,
        Range32 {
            pointer: 0x200,
            size: 16,
        },
    ));
    assert_eq!(bytes.len(), header_end);
    bytes.resize(0x200, 0);
    bytes.extend_from_slice(&[0xCC; 16]); // .text raw bytes
    bytes.extend_from_slice(&[0xAA; 4]); // overlay slack
    let len = bytes.len();
    CorpusFixture::new(
        bytes,
        &[
            hard(HEADER, 0..section_table),
            hard(TABLE, section_table..header_end),
            slack(header_end..len),
        ],
    )
}

/// The smallest PE carrying one resource string block: `.rsrc` holds a
/// three-level directory (RT_STRING / block 1 / en-US) and one data entry
/// pointing at a 16-unit empty string table.
pub fn pe_resources() -> CorpusFixture {
    const RSRC_RVA: u32 = 0x2000;
    const RSRC_RAW: u32 = 0x200;

    // The resource tree inside the section: three 16-byte directory
    // headers + one 8-byte entry each, then a 16-byte data entry and the
    // 32-byte payload.
    let mut rsrc = Vec::new();
    for (id, target) in [
        (6u32, 24u32 | 0x8000_0000), // RT_STRING -> names dir
        (1, 48 | 0x8000_0000),       // block 1 -> languages dir
        (0x0409, 72),                // en-US -> data entry
    ] {
        rsrc.extend_from_slice(&[0; 8]); // characteristics, timestamp
        rsrc.extend_from_slice(&[0; 4]); // versions
        rsrc.extend_from_slice(&0u16.to_le_bytes()); // named entries
        rsrc.extend_from_slice(&1u16.to_le_bytes()); // id entries
        rsrc.extend_from_slice(&id.to_le_bytes());
        rsrc.extend_from_slice(&target.to_le_bytes());
    }
    let payload_rva = RSRC_RVA + 88;
    rsrc.extend_from_slice(&payload_rva.to_le_bytes());
    rsrc.extend_from_slice(&32u32.to_le_bytes());
    rsrc.extend_from_slice(&[0; 8]); // code page, reserved
    rsrc.extend_from_slice(&[0; 32]); // sixteen empty string units
    assert_eq!(rsrc.len(), 120);

    let header_end = 0x80 + 4 + 20 + 224 + 40;
    let mut bytes = pe_skeleton(1, 16, header_end as u32, Some((RSRC_RVA, 120)));
    bytes.extend_from_slice(&pe_section_row(
        b".rsrc\0\0\0",
        RSRC_RVA,
        120,
        Range32 {
            pointer: RSRC_RAW,
            size: 120,
        },
    ));
    assert_eq!(bytes.len(), header_end);
    bytes.resize(RSRC_RAW as usize, 0);
    bytes.extend_from_slice(&rsrc);
    let end = bytes.len();
    bytes.extend_from_slice(&[0xAA; 4]);
    let len = bytes.len();
    CorpusFixture::new(
        bytes,
        &[
            hard(HEADER, 0..header_end),
            hard(TABLE, RSRC_RAW as usize..end),
            slack(end..len),
        ],
    )
}

// ---------------------------------------------------------------------------
// Script programs (`src/script_raw/ledger.rs`): three 4-byte words, each
// naming an opcode the ledger knows.
// ---------------------------------------------------------------------------

/// Three known-opcode words. Frame spans mark each word's interior: a cut
/// inside a word is a truncated opcode; a cut on a boundary is a shorter
/// valid program.
pub fn script_program() -> CorpusFixture {
    let bytes: Vec<u8> = [0x10u32, 0x20, 0x30]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    CorpusFixture::new(bytes, &[frame(0..4), frame(4..8), frame(8..12)])
}

// ---------------------------------------------------------------------------
// Non-container surfaces (`NotApplicable`): the fixture is still authored
// and spanned, but nothing in it is required — every prefix is a bounded
// shorter input.
// ---------------------------------------------------------------------------

/// A small multi-line document.
pub fn text_lines() -> CorpusFixture {
    non_refusing(b"alpha\nbeta\ngamma\n")
}

/// A small keyed field list.
pub fn text_keyed_list() -> CorpusFixture {
    non_refusing(b"[SECTION]\r\nALPHA=1,2\r\nBETA=\"a;b\"\r\n")
}

/// A small resource-header document.
pub fn text_resource_header() -> CorpusFixture {
    non_refusing(b"#define ID_OK 200\r\n#define ID_CANCEL 201\r\n")
}

/// The INTERP container handed to discovery: it has bytes and a path but
/// discovery absorbs anything.
pub fn script_discovery() -> CorpusFixture {
    non_refusing(&interp_container().bytes)
}

/// The two sources of the inventory fixture.
pub fn script_inventory() -> CorpusFixture {
    non_refusing(&zbd_wave_header().bytes)
}

fn non_refusing(bytes: &[u8]) -> CorpusFixture {
    let len = bytes.len();
    CorpusFixture::new(bytes.to_vec(), &[slack(0..len)])
}
