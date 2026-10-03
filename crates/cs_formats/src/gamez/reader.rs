//! The CS GameZ mesh-array layout and its reader into [`RawMesh`].
//!
//! # Provenance
//!
//! The layout below is **established**, not guessed. It was read from the
//! pinned legacy CS-capable reference, mech3ax **v0.6.0**, commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
//! `docs/research/SOURCES.md`), and checked byte-for-byte against every
//! GameZ archive of the original installation. The field worksheet with
//! source file and line references, the per-archive measurements and the
//! remaining unknowns are in
//! `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md`.
//!
//! Source files this module implements, at that commit:
//!
//! | What | Source |
//! | --- | --- |
//! | 40-byte container header | `crates/mech3ax-gamez/src/gamez/cs/mod.rs` (`HeaderCsC`) |
//! | section order, signature and version | `crates/mech3ax-gamez/src/gamez/common.rs` (`SIGNATURE`, `VERSION_CS`) |
//! | mesh index and its non-sequential fixups | `crates/mech3ax-gamez/src/gamez/common.rs` (`MeshesInfoC`, `MeshIndexIter`, `read_meshes_info_nonseq`), `crates/mech3ax-gamez/src/gamez/cs/meshes.rs` (`read_meshes`) |
//! | two measured mesh-index remap tables | `crates/mech3ax-gamez/src/gamez/cs/fixup.rs` (`Fixup`) |
//! | 100-byte mesh record, 40-byte polygon record, flag bits, per-corner arrays, 12-byte mesh material reference | `crates/mech3ax-gamez/src/mesh/ng.rs` (`MeshNgC`, `PolygonNgC`, `PolygonBitFlags`, `read_polygons`, `read_mesh_material_infos`) and `crates/mech3ax-api-types/src/gamez/mesh/ng.rs` (`MeshMaterialInfo`) |
//! | 76-byte mesh light record and its trailing vectors | `crates/mech3ax-gamez/src/mesh/common.rs` (`LightC`, `read_lights`) |
//! | node to mesh association | `crates/mech3ax-nodes/src/cs/node.rs` (`NodeCsC.mesh_index`, offset 60 of the 208-byte record) |
//!
//! No code was copied: mech3ax is EUPL-1.2 and is read as a reference only.
//! Every field name here starts with `unk_` where the reference itself assigns
//! no meaning, and stays raw (spec F10 non-negotiable #5: a field the
//! reference calls `specular` is not read as specularity — here not even the
//! material record is parsed; that is F10-C.02).
//!
//! # What this reads, and what it deliberately does not
//!
//! [`read_gamez_meshes`] reads the container header and the **mesh section
//! only**: the mesh index, the array of 100-byte mesh records, and every
//! stored mesh's data. It stops exactly at [`GameZHeader::nodes_offset`] and
//! checks that it got there, which is what proves the whole variable-length
//! section was read and nothing was skipped.
//!
//! The other three sections are not read here, and their offsets are used only
//! to bound this one:
//!
//! * the texture-name table, `textures_offset..materials_offset`;
//! * the material records, `materials_offset..meshes_offset` — F10-C.02 audits
//!   those and their texture dependencies;
//! * the node array, `nodes_offset..end` — F11-A owns the node records.
//!
//! Because the material records are not parsed, a material index stays a **raw
//! reference**: this module proves the index was read from inside the file and
//! stores it, but it does not claim the index resolves and does not range-check
//! it against a material count it has not read. That check belongs to F10-C.02
//! with the table in hand. [`GameZMeshes::unchecked_material_references`] states
//! the gap as a count, so no consumer can mistake an unchecked raw index for a
//! resolved material.
//!
//! # What is kept raw
//!
//! The stored polygon record packs its corner count and its flag bits into one
//! `u32` (`vertex_info`), and this reader splits it exactly as the reference
//! does: corners are the nine bits `0..9`, flags are the seven bits `9..15`, and
//! the flag field's low bit is always zero. Only two flag bits
//! are given a meaning, and only because the layout establishes it:
//! [`FLAG_TRIANGLE_STRIP`] selects the corner topology (a strip, decoded by
//! [`decode_strip`]) and [`FLAG_NORMALS`] says the polygon stores a per-corner
//! normal index. Every other bit is carried in [`RawPolygon::raw_flags`]
//! untouched.
//!
//! A polygon with several stored material groups keeps **all** of them, in
//! [`GameZMesh::material_groups`] — one entry per stored polygon, in polygon
//! order. [`GameZMesh::groups`] and [`GameZMesh::corner_uv`] pair them with the
//! IR polygons. They are deliberately *not* a field on [`RawPolygon`]: the IR is
//! stage F10-A's published contract and F10-C.01 has merged a consumer of it, so
//! this task does not add a field to it. [`RawPolygon::material`] and each
//! corner's [`RawCorner::uv`] mirror the **first** group, which is exact for a
//! single-group asset and is a single-material *view* of the rest, not the whole
//! of it: the group list is what a consumer that needs every authored
//! coordinate and every authored material index reads.

use std::fmt;
use std::mem::size_of;

use cs_types::evidence::ClaimStatus;

use super::mesh::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader};
use crate::zbd::{GAMEZ_SIGNATURE, GAMEZ_VERSION};

/// Error scope stamped onto failures raised while reading the mesh section.
pub const MESHES_ENTRYPOINT: &str = "gamez.meshes";

/// Bytes of the CS GameZ container header ([`GameZHeader`]).
///
/// `HeaderCsC` in `crates/mech3ax-gamez/src/gamez/cs/mod.rs` is declared
/// `#[repr(C)]` with ten `u32` fields and `static_assert_size!(HeaderCsC, 40)`.
pub const GAMEZ_HEADER_BYTES: u64 = 40;

/// Bytes of the mesh index ([`MeshIndex`]): `MeshesInfoC`, three `i32`.
pub const MESH_INDEX_BYTES: u64 = 12;

/// Bytes of one stored mesh record ([`RawMeshInfo`]): `MeshNgC`, twenty-five
/// 4-byte fields, `static_assert_size!(MeshNgC, 100)`.
pub const MESH_INFO_BYTES: u64 = 100;

/// Bytes of the 4-byte word that follows every mesh record: the mesh data
/// offset for a present mesh, the expected index for an absent one.
pub const MESH_INFO_TRAILER_BYTES: u64 = 4;

/// Bytes of one stored polygon record ([`RawPolygonInfo`]): `PolygonNgC`, ten
/// 4-byte fields, `static_assert_size!(PolygonNgC, 40)`.
pub const POLYGON_INFO_BYTES: u64 = 40;

/// Bytes of one stored mesh material reference ([`RawMeshMaterialInfo`]).
pub const MESH_MATERIAL_INFO_BYTES: u64 = 12;

/// Bytes of one stored mesh light header ([`RawMeshLightHeader`]): `LightC`,
/// `static_assert_size!(LightC, 76)`.
pub const MESH_LIGHT_HEADER_BYTES: u64 = 76;

/// Bytes of one stored `Vec3`: a position, a normal, a morph vector or a light
/// extra.
pub const VEC3_BYTES: u64 = 12;

/// Largest corner count the polygon record's `vertex_info` can express, and the
/// bound every corner array is reserved against.
pub const MAX_POLYGON_CORNERS: u32 = 0x1FF;

/// Mask of the corner-count bits of `vertex_info` (`vertex_info & 0x1FF`): the
/// count field is nine bits wide, bits `0..9`.
pub const CORNER_COUNT_MASK: u32 = 0x1FF;

/// Shift that moves the flag byte of `vertex_info` down to bit 0
/// (`(vertex_info & 0xFE00) >> 8`).
pub const FLAG_SHIFT: u32 = 8;

/// Mask of the flag bits inside `vertex_info`, before the shift: bits `9..15`.
///
/// Seven bits, so the flag field's low bit is always zero after the shift and the
/// five bits the reference names are 2, 3, 4, 5 and 6 of
/// [`RawPolygonInfo::flags`]. The largest value a flag field can hold is `0xFE`.
pub const FLAG_MASK: u32 = 0xFE00;

/// `PolygonBitFlags::UNK2`. Retained so a caller can see the bit occurred; the
/// reference names it `UNK2` and nothing more, so it is not interpreted.
pub const FLAG_UNK2: u32 = 1 << 2;

/// `PolygonBitFlags::UNK3`. The reference notes it is absent from `mechlib`.
pub const FLAG_UNK3: u32 = 1 << 3;

/// `PolygonBitFlags::NORMALS` in `crates/mech3ax-gamez/src/mesh/ng.rs`.
///
/// Set: the polygon stores one normal index per corner, right after its
/// position indices, and every [`RawCorner::normal`] is `Some`.
pub const FLAG_NORMALS: u32 = 1 << 4;

/// `PolygonBitFlags::TRI_STRIP` in `crates/mech3ax-gamez/src/mesh/ng.rs`.
///
/// Set: the corners are a triangle strip ([`PrimitiveKind::TriangleStrip`]).
/// Clear: the corners are one outline ([`PrimitiveKind::Polygon`]). This is the
/// **only** established selector between the two, which is what F10-A listed as
/// unknown.
pub const FLAG_TRIANGLE_STRIP: u32 = 1 << 5;

/// `PolygonBitFlags::UNK6`. The reference notes it is absent from `mechlib`.
pub const FLAG_UNK6: u32 = 1 << 6;

/// Every flag bit the reference's `PolygonBitFlags` names, within the seven the
/// flag field can hold.
pub const KNOWN_POLYGON_FLAGS: u32 =
    FLAG_UNK2 | FLAG_UNK3 | FLAG_NORMALS | FLAG_TRIANGLE_STRIP | FLAG_UNK6;

/// Largest value a flag field can hold: `0xFE`, the flag bits' full width.
pub const MAX_POLYGON_FLAGS: u32 = FLAG_MASK >> FLAG_SHIFT;

/// The `unk08` header value that identifies the `planes.zbd` variant and
/// selects [`Fixup::Planes`].
pub const UNK08_PLANES: u32 = 967_277_477;

/// The `unk08` header value that identifies the C4 `gamez.zbd` variant and
/// selects [`Fixup::C4`].
pub const UNK08_C4: u32 = 967_279_328;

/// Which stored mesh-index fixup table applies to an archive.
///
/// The mesh index is **non-sequential**: an absent mesh record stores the index
/// the next present one is expected to carry, and that expectation follows a
/// different order than the array position. The reference carries two measured
/// remap tables and matches them against the whole header (`Fixup::read`). An
/// archive matching neither is [`Fixup::None`] and is read with the sequential
/// expectation `index + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixup {
    /// No `unk08` matched a fixup table: the sequential expectation is used.
    None,
    /// The `planes.zbd` header: twelve remapped indices.
    Planes,
    /// The C4 `gamez.zbd` header: fifty-four remapped indices.
    C4,
}

impl Fixup {
    /// The fixup a header selects, by its `unk08` word alone.
    ///
    /// The reference compares the *whole* header struct, but `unk08` alone
    /// separates the two tables in the measured corpus (nine archives); the
    /// other nine fields are validated separately by this reader.
    pub const fn for_unk08(unk08: u32) -> Self {
        match unk08 {
            UNK08_PLANES => Self::Planes,
            UNK08_C4 => Self::C4,
            _ => Self::None,
        }
    }

    /// The measured `(expected, stored)` pairs of the `planes.zbd` table, in
    /// the reference's own order.
    const PLANES_TABLE: [(i32, i32); 12] = [
        (1396, 1321),
        (1398, 1316),
        (1554, 1779),
        (1558, 1396),
        (1559, 1553),
        (1681, 1502),
        (1317, 1558),
        (1460, 1680),
        (1322, 1557),
        (1503, 1391),
        (1314, 1459),
        (1392, 1395),
    ];

    /// The measured `(expected, stored)` pairs of the C4 table, in the
    /// reference's own order.
    const C4_TABLE: [(i32, i32); 54] = [
        (2308, 2268),
        (2371, 2285),
        (2283, 2281),
        (2290, 2307),
        (2367, 2359),
        (2266, 2368),
        (2289, 2286),
        (2288, 2409),
        (2282, 2366),
        (2360, 2296),
        (2375, 2370),
        (2403, 2309),
        (2278, 2304),
        (2408, 2402),
        (2409, 2403),
        (2270, 2288),
        (2300, 2270),
        (2305, 2276),
        (2306, 2404),
        (2285, 2290),
        (2302, 2273),
        (2279, 2282),
        (2292, 2294),
        (2311, 2272),
        (2304, 2275),
        (2293, 2291),
        (2276, 2277),
        (2296, 2293),
        (2314, 2311),
        (2405, 2292),
        (2309, 2406),
        (2307, 2300),
        (2369, 2405),
        (2277, 2280),
        (2299, 2306),
        (2271, 2301),
        (2281, 2278),
        (2297, 2374),
        (2312, 2490),
        (2404, 2407),
        (2269, 2308),
        (2265, 2303),
        (2273, 2313),
        (2291, 2289),
        (2286, 2269),
        (2294, 2263),
        (2301, 2302),
        (2272, 2310),
        (2303, 2299),
        (2310, 2305),
        (2406, 2408),
        (2407, 2265),
        (2274, 2271),
        (2410, 2297),
    ];

    fn table(self) -> &'static [(i32, i32)] {
        match self {
            Self::None => &[],
            Self::Planes => &Self::PLANES_TABLE,
            Self::C4 => &Self::C4_TABLE,
        }
    }

    /// The stored index an absent mesh record at array position `index` must
    /// carry under this fixup.
    ///
    /// `expected` is the reference's own value: `index + 1`, except at the end
    /// of the array where it is `-1` (`MeshIndexIter::next`).
    pub fn mesh_index_remap(self, expected: i32) -> i32 {
        self.table()
            .iter()
            .find(|(from, _)| *from == expected)
            .map_or(expected, |(_, to)| *to)
    }

    /// The stored `last_index` the last present mesh must be followed by.
    pub fn last_index_remap(self, last_index: i32) -> i32 {
        match self {
            Self::None => last_index,
            Self::Planes if last_index == 1779 => 1313,
            Self::C4 if last_index == 2490 => 2283,
            _ => last_index,
        }
    }

    /// Short name of the variant, for diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Planes => "planes",
            Self::C4 => "c4",
        }
    }
}

/// The 40-byte CS GameZ container header.
///
/// `HeaderCsC` in `crates/mech3ax-gamez/src/gamez/cs/mod.rs`. Every field is
/// stored as the reference stores it; `unk08` and `light_index` have no
/// established meaning beyond fixup selection and the node array, and the four
/// section offsets are byte offsets from the start of the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameZHeader {
    /// `signature`: [`GAMEZ_SIGNATURE`] for a CS GameZ archive.
    pub signature: u32,
    /// `version`: [`GAMEZ_VERSION`] (42) for a CS GameZ archive.
    pub version: u32,
    /// `unk08`: selects the mesh-index fixup (see [`Fixup::for_unk08`]); its
    /// own meaning is not established.
    pub unk08: u32,
    /// `texture_count`: entries of the texture-name table, which this reader
    /// does not parse.
    pub texture_count: u32,
    /// `textures_offset`: start of the texture-name table. The reference
    /// asserts it equals [`GAMEZ_HEADER_BYTES`], because the table is read
    /// straight after the header.
    pub textures_offset: u32,
    /// `materials_offset`: start of the material records (F10-C.02).
    pub materials_offset: u32,
    /// `meshes_offset`: start of the mesh index, the section this reader
    /// starts from.
    pub meshes_offset: u32,
    /// `node_array_size`: entries in the node array (F11-A), not read here.
    pub node_array_size: u32,
    /// `light_index`: the node index of the container's light node. The
    /// reference asserts it equals that node's index; the node array is not
    /// read here, so the value is only carried.
    pub light_index: u32,
    /// `nodes_offset`: start of the node array and the **exclusive end of the
    /// mesh section**: this reader stops here and checks it arrived exactly.
    pub nodes_offset: u32,
}

/// The mesh index: `MeshesInfoC` in
/// `crates/mech3ax-gamez/src/gamez/common.rs`, three `i32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshIndex {
    /// `array_size`: entries in the mesh record array, present or not.
    pub array_size: i32,
    /// `count`: present mesh records, cross-checked against the number of
    /// records whose `parent_count` is non-zero.
    pub count: i32,
    /// `last_index`: the expected index after the **last present** mesh, which
    /// for a remapped archive is not `count`.
    pub last_index: i32,
}

/// One 100-byte stored mesh record, plus the 4-byte word that follows it.
///
/// Every field is stored as the reference stores it. `polygon_count`,
/// `vertex_count`, `normal_count` and `material_count` are the stored counts the
/// data section is read with; the remaining fields are raw, and the `*_ptr`
/// values are the reference's `Ptr` fields — raw `u32`s it only compares against
/// zero, whose runtime meaning is not established.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawMeshInfo {
    /// `file_ptr`: the reference asserts it is `0` or `1` (`bool_c!`).
    pub file_ptr: u32,
    /// `unk04`: the reference asserts it is in `[0, 1, 2]`.
    pub unk04: u32,
    /// `unk08`: not interpreted.
    pub unk08: u32,
    /// `parent_count`: non-zero marks a **present** mesh record. A zero record
    /// is an all-zero stub whose following word is the expected index of the
    /// next present mesh rather than a data offset.
    pub parent_count: u32,
    /// `polygon_count`: stored polygon records in this mesh.
    pub polygon_count: u32,
    /// `vertex_count`: stored positions in this mesh.
    pub vertex_count: u32,
    /// `normal_count`: stored normals in this mesh.
    pub normal_count: u32,
    /// `morph_count`: stored morph vectors in this mesh.
    pub morph_count: u32,
    /// `light_count`: stored mesh light records in this mesh.
    pub light_count: u32,
    /// `unk36`: the reference asserts it is zero.
    pub unk36: u32,
    /// `unk40`: not interpreted.
    pub unk40: f32,
    /// `unk44`: not interpreted.
    pub unk44: f32,
    /// `unk48`: the reference asserts it is zero.
    pub unk48: u32,
    /// `polygons_ptr`: the reference's `Ptr` to the polygon records. Raw.
    pub polygons_ptr: u32,
    /// `vertices_ptr`: the reference's `Ptr` to the positions. Raw.
    pub vertices_ptr: u32,
    /// `normals_ptr`: the reference's `Ptr` to the normals. Raw.
    pub normals_ptr: u32,
    /// `lights_ptr`: the reference's `Ptr` to the mesh lights. Raw.
    pub lights_ptr: u32,
    /// `morphs_ptr`: the reference's `Ptr` to the morph vectors. Raw.
    pub morphs_ptr: u32,
    /// `unk72`: not interpreted.
    pub unk72: f32,
    /// `unk76`: not interpreted.
    pub unk76: f32,
    /// `unk80`: not interpreted.
    pub unk80: f32,
    /// `unk84`: not interpreted.
    pub unk84: f32,
    /// `unk88`: the reference asserts it is zero.
    pub unk88: u32,
    /// `material_count`: stored mesh material references in this mesh.
    pub material_count: u32,
    /// `materials_ptr`: the reference's `Ptr` to those references. Raw.
    pub materials_ptr: u32,
}

impl RawMeshInfo {
    /// The record is a present mesh rather than an all-zero stub.
    ///
    /// The reference's test is `parent_count > 0`; a zero record is asserted to
    /// be entirely zero, and this reader checks that
    /// ([`GameZError::NonZeroStubMesh`]).
    pub const fn is_present(&self) -> bool {
        self.parent_count > 0
    }
}

/// The all-zero mesh record: a stub slot. [`assert_mesh_info_zero`] in the
/// reference requires every field to be zero for one.
const STUB_MESH_INFO: RawMeshInfo = RawMeshInfo {
    file_ptr: 0,
    unk04: 0,
    unk08: 0,
    parent_count: 0,
    polygon_count: 0,
    vertex_count: 0,
    normal_count: 0,
    morph_count: 0,
    light_count: 0,
    unk36: 0,
    unk40: 0.0,
    unk44: 0.0,
    unk48: 0,
    polygons_ptr: 0,
    vertices_ptr: 0,
    normals_ptr: 0,
    lights_ptr: 0,
    morphs_ptr: 0,
    unk72: 0.0,
    unk76: 0.0,
    unk80: 0.0,
    unk84: 0.0,
    unk88: 0,
    material_count: 0,
    materials_ptr: 0,
};

/// One stored material group of a polygon: a material reference and that group's
/// texture coordinate for every corner.
///
/// The CS GameZ polygon record stores one UV set *per material group*
/// (`PolygonMaterialNg` in the pinned reference), so a polygon with two groups
/// carries two UVs per corner. The group is where that pairing lives: a UV is
/// only meaningful together with the material it was stored for (spec F10
/// non-negotiable #3: per-corner attributes are never merged).
///
/// These live on [`GameZMesh::material_groups`], one entry per stored polygon in
/// polygon order, rather than on [`RawPolygon`]. The IR is stage F10-A's
/// published contract and other crates already build against it, so this task
/// does not add a field to it; [`GameZMesh::groups`] pairs an IR polygon with its
/// stored groups, and [`GameZMesh::groups_are_complete`] states the invariant the
/// parallel array depends on.
///
/// The reader never merges, orders or dedupes groups: `mat_count` is the stored
/// count and the list is exactly that long, in stored order.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMaterialGroup {
    /// Stored material index, unchanged. The material record layout is not
    /// established here; this is a reference, not a resolved material.
    pub material: u32,
    /// One texture coordinate per corner of the polygon, in stored order.
    pub uvs: Vec<[f32; 2]>,
}

/// One 40-byte stored polygon record header: `PolygonNgC` in
/// `crates/mech3ax-gamez/src/mesh/ng.rs`.
///
/// `vertex_info` is the packed `u32` the reference splits into
/// `verts_in_poly = vertex_info & 0x1FF` and `verts_bits = (vertex_info & 0xFE00) >> 8`.
/// Both are kept here, plus the split values, so no consumer has to redo the
/// masking and no bit is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawPolygonInfo {
    /// `vertex_info`: the packed corner-count and flag word, unchanged.
    pub vertex_info: u32,
    /// `unk04`: the reference asserts `-50 <= unk04 <= 50`. Its meaning is
    /// unknown, and the measured corpus stays inside that range, but this
    /// reader does not enforce it: a value outside it changes no stored length,
    /// so enforcing it would refuse a face the layout can still read exactly.
    pub unk04: i32,
    /// `vertices_ptr`: the reference's `Ptr` to the position indices. Raw.
    pub vertices_ptr: u32,
    /// `normals_ptr`: the reference's `Ptr` to the normal indices, which it
    /// asserts is non-zero exactly when [`FLAG_NORMALS`] is set. Raw.
    pub normals_ptr: u32,
    /// `mat_count`: stored material groups. The reference asserts `> 0`.
    pub mat_count: u32,
    /// `uvs_ptr`: the reference's `Ptr` to the UV coordinates. Raw.
    pub uvs_ptr: u32,
    /// `colors_ptr`: the reference's `Ptr` to the corner colors. Raw.
    pub colors_ptr: u32,
    /// `unk28`: the reference asserts it is non-zero. Raw.
    pub unk28: u32,
    /// `unk32`: the reference asserts it is non-zero. Raw.
    pub unk32: u32,
    /// `unk36`: the reference asserts it is `<= 0xFFFF`. Raw.
    pub unk36: u32,
}

impl RawPolygonInfo {
    /// `verts_in_poly`: the stored corner count (`vertex_info & 0x1FF`).
    pub const fn corners(&self) -> u32 {
        self.vertex_info & CORNER_COUNT_MASK
    }

    /// `verts_bits`: the stored flag bits (`(vertex_info & 0xFE00) >> 8`).
    pub const fn flags(&self) -> u32 {
        (self.vertex_info & FLAG_MASK) >> FLAG_SHIFT
    }

    /// The polygon stores one normal index per corner.
    pub const fn has_normals(&self) -> bool {
        self.flags() & FLAG_NORMALS != 0
    }

    /// The corners are a triangle strip rather than one outline.
    pub const fn is_triangle_strip(&self) -> bool {
        self.flags() & FLAG_TRIANGLE_STRIP != 0
    }

    /// The corner topology the established flag selects.
    pub const fn kind(&self) -> PrimitiveKind {
        if self.is_triangle_strip() {
            PrimitiveKind::TriangleStrip
        } else {
            PrimitiveKind::Polygon
        }
    }

    /// Flag bits outside the five the reference names.
    ///
    /// The reference rejects a polygon whose flag byte has a bit outside
    /// `PolygonBitFlags` (`from_bits`). This reader keeps such a polygon
    /// instead: the extra bit changes no stored length, so the face is still
    /// readable, and the count is reported as a [`ParseFinding`] so it cannot
    /// pass unnoticed.
    pub const fn unknown_flag_bits(&self) -> u32 {
        self.flags() & !KNOWN_POLYGON_FLAGS
    }
}

/// One 12-byte stored mesh material reference: `MeshMaterialInfo` in
/// `crates/mech3ax-api-types/src/gamez/mesh/ng.rs`.
///
/// The reference asserts `material_index < material_count`, where
/// `material_count` comes from the material section it read. This reader does
/// not parse that section, so the index stays a **raw reference** and no
/// material-range failure is possible here; F10-C.02 makes that check with the
/// table in hand. `polygon_usage_count` and `unk_ptr` are the reference's second
/// and third fields, which it names and does not otherwise use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawMeshMaterialInfo {
    /// `material_index`: index into the container's material records. Raw.
    pub material_index: u32,
    /// `polygon_usage_count`: read by the reference, not interpreted here.
    pub polygon_usage_count: u32,
    /// `unk_ptr`: not interpreted.
    pub unk_ptr: u32,
}

/// One 76-byte stored mesh light header: `LightC` in
/// `crates/mech3ax-gamez/src/mesh/common.rs`.
///
/// A light is a variable-length record, so this reader must read the header to
/// know how many trailing `Vec3`s follow; it keeps the whole header raw rather
/// than reading only `extra_count` and skipping the rest. `extra_count` is the
/// number of `Vec3`s stored for this light, and the reference reads all the
/// light headers of a mesh before any light's extras, which is the order
/// [`read_mesh_lights`] follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawMeshLightHeader {
    /// `unk00`: the reference asserts it is in `[0, 1]`.
    pub unk00: u32,
    /// `unk04`: the reference asserts it is in `[0, 1, 2]`.
    pub unk04: u32,
    /// `unk08`: the reference asserts `0.0 <= unk08 <= 5.0`.
    pub unk08: f32,
    /// `extra_count`: `Vec3`s stored for this light.
    pub extra_count: u32,
    /// `unk16`: the reference asserts it is zero.
    pub unk16: u32,
    /// `unk20`: the reference asserts it is zero.
    pub unk20: u32,
    /// `unk24`: raw. The reference's assertion on it is commented out and
    /// records several values in the measured corpus, so it is not constrained.
    pub unk24: u32,
    /// `color`: three `f32` the reference asserts are in `0.0..=255.0`. Raw: the
    /// reference does not say this is a display colour or how it is consumed.
    pub color: [f32; 3],
    /// `pad40`: the reference asserts it is zero.
    pub pad40: u16,
    /// `flags`: the reference's `Bits<u16>`, whose bits it names `LightFlags`.
    /// Raw.
    pub flags: u16,
    /// `ptr`: the reference asserts it is non-zero. Raw.
    pub ptr: u32,
    /// `unk48`: the reference asserts `0.0 <= unk48 <= 2000.0`. Raw.
    pub unk48: f32,
    /// `unk52`: the reference asserts `0.0 <= unk52 <= 3500.0`. Raw.
    pub unk52: f32,
    /// `unk56`: the reference asserts `0.0 <= unk56 <= 5.1`. Raw.
    pub unk56: f32,
    /// `unk60`: the reference asserts it is in `[0, 1]`. Raw.
    pub unk60: u32,
    /// `unk64`: the reference asserts `0.0 <= unk64 <= 5000.0`. Raw.
    pub unk64: f32,
    /// `unk68`: the reference asserts `0.0 <= unk68 <= 4000.0`. Raw.
    pub unk68: f32,
    /// `unk72`: the reference asserts `0.0 <= unk72 <= 10000.0`. Raw.
    pub unk72: f32,
}

/// One stored mesh light: its raw header and the `Vec3`s that follow it.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMeshLight {
    /// The 76-byte header, raw.
    pub header: RawMeshLightHeader,
    /// `extra_count` `Vec3`s in stored order. Raw: their meaning is not
    /// established, and they are kept so the next record's offset is not the
    /// only thing that could be checked.
    pub extra: Vec<[f32; 3]>,
}

/// One stored mesh: the parsed [`RawMesh`] plus the raw record it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct GameZMesh {
    /// Array index of this mesh in the container's mesh array. This is the
    /// value a scene node's `mesh_index` refers to (`NodeCsC.mesh_index`, offset
    /// 60 of the 208-byte node record).
    pub index: u32,
    /// The 100-byte record, raw.
    pub info: RawMeshInfo,
    /// The stored arrays and polygons, parsed.
    pub mesh: RawMesh,
    /// The 40-byte record of every stored polygon, raw, in stored order.
    ///
    /// Parallel to [`GameZMesh::mesh`]'s polygons and the same length. These are
    /// kept rather than dropped for the same reason [`GameZMesh::info`] is: the
    /// `unk` words and the four `Ptr` fields have no established meaning (F10
    /// non-negotiable #5) and the pointers are never followed, so a consumer
    /// auditing a face — F10-C.02's material binding, F10-D's private-corpus pass
    /// — has to be able to see what was stored rather than trust that the reader
    /// saw it. The IR's [`RawPolygon`] keeps the flag byte, the kind and the
    /// corners; everything else the record held is here.
    pub polygon_records: Vec<RawPolygonInfo>,
    /// The stored mesh lights, in stored order.
    pub lights: Vec<RawMeshLight>,
    /// The stored morph vectors, in stored order.
    ///
    /// `morph_count` is zero in every measured retail archive; the array is
    /// read from the same offset arithmetic regardless, so a variant with morphs
    /// is not a layout change.
    pub morphs: Vec<[f32; 3]>,
    /// The 12-byte mesh material references, in stored order.
    pub materials: Vec<RawMeshMaterialInfo>,
    /// Every stored material group of every stored polygon, in polygon order.
    ///
    /// Parallel to the polygons of [`GameZMesh::mesh`] and the same length, which
    /// [`GameZMesh::groups_are_complete`] states and
    /// [`GameZMesh::groups`] relies on. A polygon's groups live here rather than
    /// on [`RawPolygon`] because the IR is a contract other crates already build
    /// against and this task does not add a field to it.
    ///
    /// **This list, not [`RawPolygon::material`], is the authority on how many
    /// groups a stored polygon has.** The IR field mirrors group `0` only.
    /// Measured on the installation: 127 728 stored polygons keep one group,
    /// 999 keep two and 7 keep three, and all of the multi-group ones are in the
    /// world archives — `ZBD/planes.zbd` stores none. See
    /// `docs/findings/2026-09-29-f10-e-material-groups-into-the-render-mesh.md`.
    pub material_groups: Vec<Vec<RawMaterialGroup>>,
    /// Where this mesh's data started, in bytes from the container start.
    pub data_offset: u64,
    /// Where this mesh's data ended, exclusive. The next present mesh's
    /// `data_offset` equals this, which is what makes the section walkable
    /// without trusting any stored pointer.
    pub data_end: u64,
}

impl GameZMesh {
    /// The topology report for this mesh: the exact decoded, invalid and
    /// unsupported face counts over its own polygons.
    pub fn topology(&self) -> super::mesh::MeshTopology {
        self.mesh.topology()
    }

    /// The stored material groups of one polygon, in stored order.
    ///
    /// `None` when `polygon` is out of range. An empty slice means the stored
    /// record declared no group at all, which the reader reports as a
    /// [`ParseFinding::PolygonWithoutMaterial`] rather than inventing one.
    pub fn groups(&self, polygon: usize) -> Option<&[RawMaterialGroup]> {
        self.material_groups.get(polygon).map(Vec::as_slice)
    }

    /// One corner's texture coordinate for one stored material group, or `None`
    /// when any of the three indices is out of range.
    ///
    /// [`RawCorner::uv`] on the IR carries material group `0` only, so this is
    /// the lookup a consumer of a **multi-group** polygon needs: a stored
    /// `mat_count` of two or three means two or three authored coordinates per
    /// corner, and only the first one is on the IR.
    pub fn corner_uv(&self, polygon: usize, group: usize, corner: usize) -> Option<[f32; 2]> {
        self.groups(polygon)?.get(group)?.uvs.get(corner).copied()
    }

    /// The raw 40-byte record of one stored polygon, or `None` when `polygon` is
    /// out of range.
    ///
    /// The IR's [`RawPolygon`] keeps the flag byte, the corner topology and the
    /// corners; this is the rest of what the record stored, kept so an auditing
    /// consumer can see the `unk` words and the never-followed pointers.
    ///
    /// Index-aligned with [`GameZMesh::mesh`]'s polygons, in stored order.
    pub fn record(&self, polygon: usize) -> Option<&RawPolygonInfo> {
        self.polygon_records.get(polygon)
    }

    /// Whether every stored polygon has its own group list: the invariant
    /// [`GameZMesh::groups`] depends on.
    pub fn groups_are_complete(&self) -> bool {
        self.material_groups.len() == self.mesh.polygons.len()
    }
}

/// Something the layout can read but the reference asserts does not occur.
///
/// A finding is **not** an error: the stored lengths are still well defined, so
/// the face is read and reaches [`RawMesh::topology`]. The finding exists so a
/// caller can see that the archive is outside the reference's asserted profile
/// instead of believing it was inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseFinding {
    /// A polygon has flag bits the reference's `PolygonBitFlags` does not name,
    /// so the reference would refuse the container here.
    UnknownPolygonFlagBits {
        /// Mesh array index.
        mesh: u32,
        /// Polygon index in that mesh.
        polygon: u32,
        /// The unmapped flag bits.
        bits: u32,
    },
    /// A strip polygon does not set [`FLAG_NORMALS`], which the reference
    /// asserts (`"has normals when tri strip"`).
    StripWithoutNormals {
        /// Mesh array index.
        mesh: u32,
        /// Polygon index in that mesh.
        polygon: u32,
    },
    /// A polygon stored no material group (`mat_count == 0`), which the
    /// reference asserts against. The face is read with no group.
    PolygonWithoutMaterial {
        /// Mesh array index.
        mesh: u32,
        /// Polygon index in that mesh.
        polygon: u32,
    },
    /// A polygon stored fewer than three corners, which the reference asserts
    /// against. The face reaches [`RawMesh::topology`] and is counted as
    /// invalid there, with its corners intact.
    PolygonTooFewCorners {
        /// Mesh array index.
        mesh: u32,
        /// Polygon index in that mesh.
        polygon: u32,
        /// Corners stored.
        corners: u32,
    },
}

impl ParseFinding {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownPolygonFlagBits { .. } => "unknown_polygon_flag_bits",
            Self::StripWithoutNormals { .. } => "strip_without_normals",
            Self::PolygonWithoutMaterial { .. } => "polygon_without_material",
            Self::PolygonTooFewCorners { .. } => "polygon_too_few_corners",
        }
    }

    /// Mesh array index the finding is about.
    pub const fn mesh(&self) -> u32 {
        match self {
            Self::UnknownPolygonFlagBits { mesh, .. }
            | Self::StripWithoutNormals { mesh, .. }
            | Self::PolygonWithoutMaterial { mesh, .. }
            | Self::PolygonTooFewCorners { mesh, .. } => *mesh,
        }
    }

    /// Evidence class of a finding: `ObservedTool` at best, never better, since
    /// a finding is by definition something the documented profile did not
    /// cover.
    pub const fn evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }
}

impl fmt::Display for ParseFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::UnknownPolygonFlagBits { polygon, bits, .. } => {
                write!(f, "{code}: polygon {polygon} has flag bits 0x{bits:02X}")
            }
            Self::StripWithoutNormals { polygon, .. } => {
                write!(f, "{code}: polygon {polygon} is a strip without normals")
            }
            Self::PolygonWithoutMaterial { polygon, .. } => {
                write!(f, "{code}: polygon {polygon} stored no material group")
            }
            Self::PolygonTooFewCorners {
                polygon, corners, ..
            } => {
                write!(f, "{code}: polygon {polygon} stores {corners} corners")
            }
        }
    }
}

/// The parsed mesh section of one CS GameZ container.
#[derive(Debug, Clone, PartialEq)]
pub struct GameZMeshes {
    /// The 40-byte container header.
    pub header: GameZHeader,
    /// The mesh index.
    pub index: MeshIndex,
    /// Which stored fixup table this container's `unk08` selected.
    pub fixup: Fixup,
    /// One entry per array slot, in stored order, `None` for an all-zero stub.
    /// The array index is the mesh index a node's `mesh_index` refers to.
    pub meshes: Vec<Option<GameZMesh>>,
    /// Everything this reader read that the reference's assertions do not
    /// allow, in stored order. An empty list means every stored polygon was
    /// inside the reference's asserted profile, which is the strongest claim
    /// this reader makes about a container.
    pub findings: Vec<ParseFinding>,
    /// Every stored material reference in the mesh section, kept raw and
    /// **not** range-checked: the material section belongs to F10-C.02, so this
    /// reader has no material count to check an index against. Zero means no
    /// mesh stored a material reference at all.
    ///
    /// A container-level fact rather than a per-mesh [`ParseFinding`]: it holds
    /// for every mesh at once, so one entry per mesh would drown the findings
    /// that really are per-face.
    pub unchecked_material_references: usize,
    /// Where the first present mesh's data started, in bytes from the container
    /// start.
    pub data_offset: u64,
    /// Where the mesh data ended, exclusive. Equal to
    /// [`GameZHeader::nodes_offset`], which is the check that the whole
    /// variable-length section was read and nothing was missed.
    pub data_end: u64,
}

impl GameZMeshes {
    /// The present meshes, in array order.
    pub fn present(&self) -> impl Iterator<Item = &GameZMesh> {
        self.meshes.iter().flatten()
    }

    /// Present mesh records, as the reference cross-checks them against
    /// [`MeshIndex::count`].
    pub fn present_count(&self) -> usize {
        self.present().count()
    }

    /// Array slots this section has, absent stubs included. This is the bound a
    /// scene node's `mesh_index` is range-checked against, so it is the array
    /// size and never the number of meshes that are present in it.
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.meshes.len()
    }

    /// One present mesh by array index — the lookup a scene node's
    /// `mesh_index` performs.
    pub fn get(&self, mesh_index: u32) -> Option<&GameZMesh> {
        self.meshes.get(mesh_index as usize)?.as_ref()
    }

    /// Pairs one container's node array with this mesh section and reports
    /// every node whose stored `mesh_index` names no present mesh.
    ///
    /// The node section is an argument rather than a field because the node
    /// reader does not hold the mesh array either: this is the seam that has
    /// both, and it keeps the check out of the node reader, where the index
    /// stays raw.
    #[must_use]
    pub fn node_bindings(
        &self,
        nodes: &super::nodes::GameZNodes,
    ) -> super::bindings::NodeMeshBindings {
        super::bindings::NodeMeshBindings::of(nodes, self)
    }

    /// Every mesh's topology report, in array order, with the present meshes
    /// only: the exact decoded, invalid and unsupported face counts F10-D's AC04
    /// report aggregates.
    pub fn topologies(&self) -> impl Iterator<Item = (u32, super::mesh::MeshTopology)> {
        self.present()
            .map(|mesh| (mesh.index, mesh.mesh.topology()))
    }

    /// The exact face census of this container: what the records declare, what
    /// was stored, and every face that reaches no drawable triangle, in mesh
    /// then polygon order. This is F10-D's AC04 report over one container.
    pub fn face_census(&self) -> super::census::FaceCensus {
        super::census::FaceCensus::of(self)
    }

    /// Evidence class of the layout this reader implements: documented in the
    /// pinned reference *and* measured against the original installation, which
    /// is `ObservedTool`. Never `VerifiedOriginal` here — that needs an
    /// original run, which has not happened.
    pub const fn layout_evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }
}

/// Why the mesh section of a CS GameZ container could not be read.
///
/// Each variant carries counts and offsets only, never archive bytes, and
/// every offset is the absolute container offset the failure is anchored at.
#[derive(Debug, Clone, PartialEq)]
pub enum GameZError {
    /// A read ran past the end of the container, or a stored count was refused
    /// by one of the parse's budgets: a [`ParseError`] already scoped as
    /// `gamez.meshes.<field>`.
    Parse(ParseError),
    /// `signature` is not [`GAMEZ_SIGNATURE`].
    Signature {
        /// Offset of the signature word.
        offset: u64,
        /// The value found.
        found: u32,
    },
    /// `version` is not [`GAMEZ_VERSION`].
    Version {
        /// Offset of the version word.
        offset: u64,
        /// The value found.
        found: u32,
    },
    /// A section offset is past the end of the container.
    SectionOutOfBounds {
        /// Which field.
        field: &'static str,
        /// Its value.
        offset: u32,
        /// Bytes the container has.
        container_len: u64,
    },
    /// Two section offsets are not in the order the layout requires.
    SectionOrder {
        /// The reference's own relation name.
        pair: &'static str,
        /// The first value.
        first: u32,
        /// The second value.
        second: u32,
    },
    /// `textures_offset` is not [`GAMEZ_HEADER_BYTES`], which the reference
    /// asserts because the texture table is read straight after the header.
    TexturesOffsetNotAfterHeader {
        /// The value found.
        found: u32,
    },
    /// A mesh record's stored data offset is outside the mesh section, before
    /// the previous mesh's, or the record array itself does not fit before the
    /// node array.
    MeshDataOffset {
        /// Mesh array index, or [`u32::MAX`] for the record array itself.
        mesh: u32,
        /// The stored offset.
        offset: u32,
        /// The lowest offset the layout allows.
        previous: u32,
        /// The exclusive end of the mesh section.
        end: u32,
    },
    /// A mesh record's stored data offset is not the offset the walk reached,
    /// so the data section is not where the index says it is.
    MeshDataNotSequential {
        /// Mesh array index.
        mesh: u32,
        /// Offset the index declares.
        declared: u32,
        /// Offset the sequential walk reached.
        walked: u64,
    },
    /// `array_size` is outside `1..=i32::MAX - 1`.
    ArraySize {
        /// The value found.
        found: i32,
    },
    /// An absent mesh record's stored index is not the expected index for its
    /// array position under the selected fixup.
    AbsentMeshIndex {
        /// Mesh array index.
        mesh: u32,
        /// The index stored.
        found: i32,
        /// The index expected.
        expected: i32,
    },
    /// A stub mesh record is not entirely zero, which the reference asserts
    /// (`assert_mesh_info_zero`).
    NonZeroStubMesh {
        /// Mesh array index.
        mesh: u32,
    },
    /// The number of present mesh records is not [`MeshIndex::count`].
    PresentCount {
        /// The count the index declares.
        declared: i32,
        /// Present records actually read.
        read: usize,
    },
    /// The stored `last_index` is not the remapped expected index after the last
    /// present mesh.
    LastIndex {
        /// The index the last present mesh's position implies, after the fixup.
        expected: i32,
        /// The value stored.
        found: i32,
    },
    /// The mesh data did not end exactly at `nodes_offset`, so a record was read
    /// with the wrong length or one was skipped.
    MeshDataEnd {
        /// Offset the walk reached.
        found: u64,
        /// Offset the header declares for the node array.
        expected: u32,
    },
}

impl GameZError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
            Self::Signature { .. } => "signature",
            Self::Version { .. } => "version",
            Self::SectionOutOfBounds { .. } => "section_out_of_bounds",
            Self::SectionOrder { .. } => "section_order",
            Self::TexturesOffsetNotAfterHeader { .. } => "textures_offset_not_after_header",
            Self::MeshDataOffset { .. } => "mesh_data_offset",
            Self::MeshDataNotSequential { .. } => "mesh_data_not_sequential",
            Self::ArraySize { .. } => "array_size",
            Self::AbsentMeshIndex { .. } => "absent_mesh_index",
            Self::NonZeroStubMesh { .. } => "non_zero_stub_mesh",
            Self::PresentCount { .. } => "present_count",
            Self::LastIndex { .. } => "last_index",
            Self::MeshDataEnd { .. } => "mesh_data_end",
        }
    }

    /// The container label the failure came from, when the variant carries one
    /// from the checked reader. The validation variants name no container
    /// because they are produced inside the parse whose reader holds it.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            _ => "",
        }
    }

    /// The byte offset the failure is anchored at, when the variant names one.
    pub fn offset(&self) -> Option<u64> {
        match self {
            Self::Signature { offset, .. } | Self::Version { offset, .. } => Some(*offset),
            Self::SectionOutOfBounds { offset, .. } => Some(u64::from(*offset)),
            Self::MeshDataNotSequential { walked, .. } => Some(*walked),
            Self::MeshDataEnd { found, .. } => Some(*found),
            Self::Parse(error) => Some(error.offset),
            _ => None,
        }
    }
}

impl From<ParseError> for GameZError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for GameZError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::Signature { offset, found } => write!(
                f,
                "at offset {offset}: GameZ signature is 0x{found:08X}, expected 0x{GAMEZ_SIGNATURE:08X}"
            ),
            Self::Version { offset, found } => write!(
                f,
                "at offset {offset}: GameZ version is {found}, expected {GAMEZ_VERSION} for Crimson Skies"
            ),
            Self::SectionOutOfBounds {
                field,
                offset,
                container_len,
            } => write!(
                f,
                "section {field} at {offset} is outside the {container_len}-byte container"
            ),
            Self::SectionOrder {
                pair,
                first,
                second,
            } => {
                write!(
                    f,
                    "section offsets are out of order: {pair} is {first} then {second}"
                )
            }
            Self::TexturesOffsetNotAfterHeader { found } => write!(
                f,
                "textures_offset is {found}, expected {GAMEZ_HEADER_BYTES} (the texture table is read straight after the header)"
            ),
            Self::MeshDataOffset {
                mesh,
                offset,
                previous,
                end,
            } => write!(
                f,
                "mesh {mesh} data offset {offset} is outside the mesh section [{previous}, {end})"
            ),
            Self::MeshDataNotSequential {
                mesh,
                declared,
                walked,
            } => write!(
                f,
                "mesh {mesh} declares data offset {declared} but the walk reached {walked}"
            ),
            Self::ArraySize { found } => {
                write!(f, "mesh array_size is {found}, expected 1..=i32::MAX-1")
            }
            Self::AbsentMeshIndex {
                mesh,
                found,
                expected,
            } => write!(
                f,
                "absent mesh {mesh} stores index {found}, expected {expected} for its position"
            ),
            Self::NonZeroStubMesh { mesh } => {
                write!(f, "absent mesh {mesh} is not an all-zero stub record")
            }
            Self::PresentCount { declared, read } => write!(
                f,
                "mesh index declares {declared} present meshes, but {read} present records were read"
            ),
            Self::LastIndex { expected, found } => write!(
                f,
                "mesh index stores last_index {found}, expected {expected} after the last present mesh"
            ),
            Self::MeshDataEnd { found, expected } => write!(
                f,
                "mesh data ended at {found}, expected the header's nodes_offset {expected}"
            ),
        }
    }
}

impl std::error::Error for GameZError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

/// One entry of the mesh record array, before the data section is walked.
#[derive(Debug, Clone, Copy)]
struct MeshRecord {
    /// Array index of this slot.
    index: u32,
    /// The 100-byte record, raw.
    info: RawMeshInfo,
    /// The 4-byte word after it: the data offset for a present record, or the
    /// expected index of the next present one for a stub.
    trailer: u32,
}

/// The expected index an absent mesh record at array position `index` carries
/// before any fixup: `index + 1`, and `-1` at the end of the array.
///
/// `MeshIndexIter::next` in `crates/mech3ax-gamez/src/gamez/common.rs`.
const fn expected_index(index: u32, array_size: u32) -> i32 {
    let next = index + 1;
    if next == array_size { -1 } else { next as i32 }
}

/// Reinterprets the stored 4-byte word as the `i32` the reference reads it as.
///
/// The word after a stub mesh record is an `i32` index, and the same word after
/// a present record is a `u32` offset. Rust has no `u32` to `i32` conversion, so
/// this is a bit reinterpretation: no value is narrowed, and a stored index
/// above `i32::MAX` becomes negative exactly as the reference's own `read_i32`
/// would see it.
const fn reinterpret_i32(word: u32) -> i32 {
    word as i32
}

/// Reads the header and mesh section of a CS GameZ container into
/// [`GameZMeshes`].
///
/// `bytes` is the whole container exactly as the installation stores it: a
/// `planes.zbd` or `gamez.zbd` file whose first word is the GameZ signature.
/// `container` is a provenance label (a relative spelling, never a path that
/// gets joined) used by every reader error.
///
/// The read runs through [`ParseContext::parse`] (spec F03), so a truncated
/// container, a hostile stored count and an over-large mesh are refused before
/// anything is allocated, every failure names the container and an absolute
/// offset, and a failed attempt leaves the allocation ledger untouched so the
/// same context can retry the bytes honestly.
///
/// # Errors
///
/// * [`GameZError::Signature`] / [`GameZError::Version`] when the first two
///   words are not a CS GameZ container;
/// * [`GameZError::SectionOutOfBounds`] / [`GameZError::SectionOrder`] /
///   [`GameZError::TexturesOffsetNotAfterHeader`] when the header's offsets do
///   not describe a section chain inside the container;
/// * [`GameZError::ArraySize`], [`GameZError::AbsentMeshIndex`],
///   [`GameZError::NonZeroStubMesh`], [`GameZError::PresentCount`] or
///   [`GameZError::LastIndex`] when the mesh index contradicts itself;
/// * [`GameZError::MeshDataOffset`] / [`GameZError::MeshDataNotSequential`] /
///   [`GameZError::MeshDataEnd`] when the stored mesh data is not a back-to-back
///   run of exactly the declared meshes between the record array and
///   `nodes_offset`;
/// * [`GameZError::Parse`] for truncation and for a stored count one of the
///   budgets refuses.
///
pub fn read_gamez_meshes(
    context: &mut ParseContext,
    _container: &str,
    bytes: &[u8],
) -> Result<GameZMeshes, GameZError> {
    // The container label is the parse's own: `ParseContext::new` takes it, and
    // every reader error inherits it. A reader must not carry a second,
    // possibly different one.
    scoped(context, bytes, |reader, allocation| {
        read_meshes(reader, allocation, bytes.len() as u64)
    })
}

/// Runs one attempt inside the parse, so a reader failure keeps its own scope
/// (`gamez.meshes.<field>`) and a domain failure keeps its own variant, while
/// either one still rolls the allocation ledger back and stamps the entrypoint.
fn scoped<T>(
    context: &mut ParseContext,
    bytes: &[u8],
    attempt: impl FnOnce(&mut Reader<'_>, &mut AllocationBudget) -> Result<T, GameZError>,
) -> Result<T, GameZError> {
    match context.parse(
        MESHES_ENTRYPOINT,
        bytes,
        |reader, allocation, _recursion| match attempt(reader, allocation) {
            Ok(value) => Ok(Ok(value)),
            Err(GameZError::Parse(error)) => Err(error),
            Err(domain) => Ok(Err(domain)),
        },
    ) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(domain)) => Err(domain),
        Err(error) => Err(GameZError::Parse(error)),
    }
}

fn read_meshes(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    container_len: u64,
) -> Result<GameZMeshes, GameZError> {
    let header = read_container_header(reader, container_len)?;
    let (index, records) = read_mesh_index(reader, allocation, &header, container_len)?;
    let fixup = Fixup::for_unk08(header.unk08);
    check_mesh_index(&index, &records, fixup, &header, reader.position())?;
    let data_offset = reader.position();
    let (meshes, findings) = read_mesh_data(reader, allocation, &records)?;
    let data_end = reader.position();
    if data_end != u64::from(header.nodes_offset) {
        return Err(GameZError::MeshDataEnd {
            found: data_end,
            expected: header.nodes_offset,
        });
    }
    let unchecked_material_references = meshes
        .iter()
        .flatten()
        .map(|mesh| mesh.materials.len() + mesh.material_groups.iter().flatten().count())
        .sum();
    Ok(GameZMeshes {
        header,
        index,
        fixup,
        meshes,
        findings,
        unchecked_material_references,
        data_offset,
        data_end,
    })
}

/// Reads and validates the 40-byte container header.
///
/// Shared with the material-record reader so both entrypoints reject the same
/// bytes for the same reason: it is the header, not either section, that says
/// which container this is and where its sections are.
pub(crate) fn read_container_header(
    reader: &mut Reader<'_>,
    container_len: u64,
) -> Result<GameZHeader, GameZError> {
    let signature = reader.read_u32("header.signature")?;
    let version = reader.read_u32("header.version")?;
    if signature != GAMEZ_SIGNATURE {
        return Err(GameZError::Signature {
            offset: 0,
            found: signature,
        });
    }
    if version != GAMEZ_VERSION {
        return Err(GameZError::Version {
            offset: 4,
            found: version,
        });
    }
    let header = GameZHeader {
        signature,
        version,
        unk08: reader.read_u32("header.unk08")?,
        texture_count: reader.read_u32("header.texture_count")?,
        textures_offset: reader.read_u32("header.textures_offset")?,
        materials_offset: reader.read_u32("header.materials_offset")?,
        meshes_offset: reader.read_u32("header.meshes_offset")?,
        node_array_size: reader.read_u32("header.node_array_size")?,
        light_index: reader.read_u32("header.light_index")?,
        nodes_offset: reader.read_u32("header.nodes_offset")?,
    };
    check_sections(&header, container_len)?;
    Ok(header)
}

/// The section chain the reference asserts.
///
/// `read.offset == header.textures_offset` right after the header read is the
/// narrowest of them (the texture table is read first, so its offset is the
/// header size), and the three strict inequalities are the other assertions in
/// `read_gamez`. `meshes_offset` and `nodes_offset` are additionally required to
/// be inside the container here, so every later bound is real.
fn check_sections(header: &GameZHeader, container_len: u64) -> Result<(), GameZError> {
    for (field, offset) in [
        ("nodes_offset", header.nodes_offset),
        ("meshes_offset", header.meshes_offset),
        ("materials_offset", header.materials_offset),
        ("textures_offset", header.textures_offset),
    ] {
        if u64::from(offset) > container_len {
            return Err(GameZError::SectionOutOfBounds {
                field,
                offset,
                container_len,
            });
        }
    }
    for (pair, first, second) in [
        (
            "textures_offset < materials_offset",
            header.textures_offset,
            header.materials_offset,
        ),
        (
            "materials_offset < meshes_offset",
            header.materials_offset,
            header.meshes_offset,
        ),
        (
            "meshes_offset < nodes_offset",
            header.meshes_offset,
            header.nodes_offset,
        ),
    ] {
        if first >= second {
            return Err(GameZError::SectionOrder {
                pair,
                first,
                second,
            });
        }
    }
    if u64::from(header.textures_offset) != GAMEZ_HEADER_BYTES {
        return Err(GameZError::TexturesOffsetNotAfterHeader {
            found: header.textures_offset,
        });
    }
    Ok(())
}

fn read_mesh_index(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    header: &GameZHeader,
    container_len: u64,
) -> Result<(MeshIndex, Vec<MeshRecord>), GameZError> {
    // The reader sits at the end of the header; the mesh index is at
    // `meshes_offset`, and the gap is the texture and material sections this
    // reader does not parse.
    let at = reader.position();
    debug_assert_eq!(at, GAMEZ_HEADER_BYTES);
    if u64::from(header.meshes_offset) < at {
        return Err(GameZError::MeshDataOffset {
            mesh: u32::MAX,
            offset: header.meshes_offset,
            previous: at as u32,
            end: header.nodes_offset,
        });
    }
    reader.skip("meshes.seek", (header.meshes_offset - at as u32) as usize)?;
    let array_size = reader.read_i32("meshes.array_size")?;
    let count = reader.read_i32("meshes.count")?;
    let last_index = reader.read_i32("meshes.last_index")?;
    if !(1..=i32::MAX - 1).contains(&array_size) {
        return Err(GameZError::ArraySize { found: array_size });
    }
    let index = MeshIndex {
        array_size,
        count,
        last_index,
    };

    // The record array must fit before the node array, or a hostile
    // `array_size` would walk into it. This is checked arithmetic, no read.
    let slots = array_size as u64;
    let records_bytes = slots
        .checked_mul(MESH_INFO_BYTES + MESH_INFO_TRAILER_BYTES)
        .ok_or(GameZError::ArraySize { found: array_size })?;
    let records_end = reader.checked_extent("meshes.records", reader.position(), records_bytes)?;
    if records_end > u64::from(header.nodes_offset) || records_end > container_len {
        return Err(GameZError::MeshDataOffset {
            mesh: u32::MAX,
            offset: records_end as u32,
            previous: header.meshes_offset,
            end: header.nodes_offset,
        });
    }
    allocation.reserve(
        "meshes.records",
        reader.position(),
        slots,
        size_of::<MeshRecord>() as u64,
    )?;

    let mut records = Vec::with_capacity(array_size as usize);
    for mesh in 0..array_size as u32 {
        let info = read_mesh_info(reader)?;
        let trailer = reader.read_u32("mesh.info.trailer")?;
        if !info.is_present() && info != STUB_MESH_INFO {
            return Err(GameZError::NonZeroStubMesh { mesh });
        }
        records.push(MeshRecord {
            index: mesh,
            info,
            trailer,
        });
    }
    Ok((index, records))
}

fn read_mesh_info(reader: &mut Reader<'_>) -> Result<RawMeshInfo, GameZError> {
    Ok(RawMeshInfo {
        file_ptr: reader.read_u32("mesh.file_ptr")?,
        unk04: reader.read_u32("mesh.unk04")?,
        unk08: reader.read_u32("mesh.unk08")?,
        parent_count: reader.read_u32("mesh.parent_count")?,
        polygon_count: reader.read_u32("mesh.polygon_count")?,
        vertex_count: reader.read_u32("mesh.vertex_count")?,
        normal_count: reader.read_u32("mesh.normal_count")?,
        morph_count: reader.read_u32("mesh.morph_count")?,
        light_count: reader.read_u32("mesh.light_count")?,
        unk36: reader.read_u32("mesh.unk36")?,
        unk40: reader.read_f32("mesh.unk40")?,
        unk44: reader.read_f32("mesh.unk44")?,
        unk48: reader.read_u32("mesh.unk48")?,
        polygons_ptr: reader.read_u32("mesh.polygons_ptr")?,
        vertices_ptr: reader.read_u32("mesh.vertices_ptr")?,
        normals_ptr: reader.read_u32("mesh.normals_ptr")?,
        lights_ptr: reader.read_u32("mesh.lights_ptr")?,
        morphs_ptr: reader.read_u32("mesh.morphs_ptr")?,
        unk72: reader.read_f32("mesh.unk72")?,
        unk76: reader.read_f32("mesh.unk76")?,
        unk80: reader.read_f32("mesh.unk80")?,
        unk84: reader.read_f32("mesh.unk84")?,
        unk88: reader.read_u32("mesh.unk88")?,
        material_count: reader.read_u32("mesh.material_count")?,
        materials_ptr: reader.read_u32("mesh.materials_ptr")?,
    })
}

/// Cross-checks the mesh index against the record array: the reference's
/// `absent mesh index`, `mesh count` and `mesh last index` assertions, plus the
/// non-decreasing data offsets it checks while reading the array.
fn check_mesh_index(
    index: &MeshIndex,
    records: &[MeshRecord],
    fixup: Fixup,
    header: &GameZHeader,
    records_end: u64,
) -> Result<(), GameZError> {
    let array_size = index.array_size as u32;
    let mut present = 0usize;
    let mut last_expected = -1i32;
    let mut previous = records_end as u32;
    for record in records {
        if record.info.is_present() {
            present += 1;
            last_expected = expected_index(record.index, array_size);
            // The reference asserts `prev_offset <= mesh_offset <=
            // nodes_offset` while reading the array, starting from the offset
            // just after the index.
            //
            // Only the **upper** half is checked here. The lower half is
            // subsumed by the sequential walk in `read_mesh_data`: a declared
            // offset below the previous record's is not where the walk stands,
            // so it is refused there as [`GameZError::MeshDataNotSequential`]
            // with both offsets. Keeping the redundant comparison would only
            // duplicate one check in two places.
            //
            // Recorded deviation from the reference: its assertion is inclusive
            // at `nodes_offset`, this bound is not. So a *present* mesh record
            // with no data at all, sitting last in the array, is refused here and
            // accepted there. No measured archive contains one (all 17 139
            // present records across the nine GameZ containers store data), so
            // the corpus cannot say which reading is right. The strict reading
            // is kept and the deviation is recorded rather than resolved by
            // guessing; see
            // `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md`.
            if record.trailer >= header.nodes_offset {
                return Err(GameZError::MeshDataOffset {
                    mesh: record.index,
                    offset: record.trailer,
                    previous,
                    end: header.nodes_offset,
                });
            }
            previous = record.trailer;
        } else {
            let expected = fixup.mesh_index_remap(expected_index(record.index, array_size));
            let stored = reinterpret_i32(record.trailer);
            if stored != expected {
                return Err(GameZError::AbsentMeshIndex {
                    mesh: record.index,
                    found: stored,
                    expected,
                });
            }
        }
    }
    if present != index.count.max(0) as usize {
        return Err(GameZError::PresentCount {
            declared: index.count,
            read: present,
        });
    }
    let expected = fixup.last_index_remap(last_expected);
    if expected != index.last_index {
        return Err(GameZError::LastIndex {
            expected,
            found: index.last_index,
        });
    }
    Ok(())
}

type DataResult = (Vec<Option<GameZMesh>>, Vec<ParseFinding>);

fn read_mesh_data(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    records: &[MeshRecord],
) -> Result<DataResult, GameZError> {
    let mut meshes: Vec<Option<GameZMesh>> = Vec::with_capacity(records.len());
    let mut findings = Vec::new();
    for record in records {
        if !record.info.is_present() {
            meshes.push(None);
            continue;
        }
        // The data section is a back-to-back run: the walk must already be at
        // this mesh's declared offset. The reference asserts exactly this
        // (`assert_that!("mesh offset", read.offset == mesh_offset, ...)`).
        if reader.position() != u64::from(record.trailer) {
            return Err(GameZError::MeshDataNotSequential {
                mesh: record.index,
                declared: record.trailer,
                walked: reader.position(),
            });
        }
        let mesh = read_one_mesh(reader, allocation, record, &mut findings)?;
        meshes.push(Some(mesh));
    }
    Ok((meshes, findings))
}

fn read_one_mesh(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    record: &MeshRecord,
    findings: &mut Vec<ParseFinding>,
) -> Result<GameZMesh, GameZError> {
    let info = record.info;
    let positions = read_vec3s(reader, allocation, info.vertex_count, "mesh.positions")?;
    let normals = read_vec3s(reader, allocation, info.normal_count, "mesh.normals")?;
    let morphs = read_vec3s(reader, allocation, info.morph_count, "mesh.morphs")?;
    let lights = read_mesh_lights(reader, allocation, info.light_count)?;
    let PolygonData {
        records: polygon_records,
        polygons,
        material_groups,
    } = read_mesh_polygons(
        reader,
        allocation,
        record.index,
        info.polygon_count,
        findings,
    )?;
    let materials = read_mesh_materials(reader, allocation, info.material_count)?;
    let data_end = reader.position();
    Ok(GameZMesh {
        index: record.index,
        info,
        mesh: RawMesh {
            positions,
            normals,
            polygons,
        },
        polygon_records,
        lights,
        morphs,
        materials,
        material_groups,
        data_offset: u64::from(record.trailer),
        data_end,
    })
}

fn read_vec3s(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    count: u32,
    field: &'static str,
) -> Result<Vec<[f32; 3]>, GameZError> {
    allocation.reserve(field, reader.position(), u64::from(count), VEC3_BYTES)?;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        out.push([
            reader.read_f32(field)?,
            reader.read_f32(field)?,
            reader.read_f32(field)?,
        ]);
    }
    Ok(out)
}

/// `read_lights` in the reference: every light header first, then every light's
/// trailing `Vec3`s, in the same order.
fn read_mesh_lights(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    count: u32,
) -> Result<Vec<RawMeshLight>, GameZError> {
    allocation.reserve(
        "mesh.lights",
        reader.position(),
        u64::from(count),
        size_of::<RawMeshLight>() as u64,
    )?;
    let mut lights = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let header = read_mesh_light_header(reader)?;
        lights.push(RawMeshLight {
            header,
            extra: Vec::new(),
        });
    }
    for light in &mut lights {
        let count = light.header.extra_count;
        allocation.reserve(
            "mesh.light_extra",
            reader.position(),
            u64::from(count),
            VEC3_BYTES,
        )?;
        light.extra.reserve(count as usize);
        for _ in 0..count {
            light.extra.push([
                reader.read_f32("mesh.light_extra.x")?,
                reader.read_f32("mesh.light_extra.y")?,
                reader.read_f32("mesh.light_extra.z")?,
            ]);
        }
    }
    Ok(lights)
}

fn read_mesh_light_header(reader: &mut Reader<'_>) -> Result<RawMeshLightHeader, GameZError> {
    Ok(RawMeshLightHeader {
        unk00: reader.read_u32("light.unk00")?,
        unk04: reader.read_u32("light.unk04")?,
        unk08: reader.read_f32("light.unk08")?,
        extra_count: reader.read_u32("light.extra_count")?,
        unk16: reader.read_u32("light.unk16")?,
        unk20: reader.read_u32("light.unk20")?,
        unk24: reader.read_u32("light.unk24")?,
        color: [
            reader.read_f32("light.color.r")?,
            reader.read_f32("light.color.g")?,
            reader.read_f32("light.color.b")?,
        ],
        pad40: reader.read_u16("light.pad40")?,
        flags: reader.read_u16("light.flags")?,
        ptr: reader.read_u32("light.ptr")?,
        unk48: reader.read_f32("light.unk48")?,
        unk52: reader.read_f32("light.unk52")?,
        unk56: reader.read_f32("light.unk56")?,
        unk60: reader.read_u32("light.unk60")?,
        unk64: reader.read_f32("light.unk64")?,
        unk68: reader.read_f32("light.unk68")?,
        unk72: reader.read_f32("light.unk72")?,
    })
}

/// `read_polygons` in the reference: every polygon record first, then, per
/// polygon in order, its position indices, its normal indices when the
/// [`FLAG_NORMALS`] bit is set, its `mat_count` material indices, one UV set per
/// material group, and finally its corner colors.
fn read_mesh_polygons(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    mesh: u32,
    count: u32,
    findings: &mut Vec<ParseFinding>,
) -> Result<PolygonData, GameZError> {
    allocation.reserve(
        "polygon.records",
        reader.position(),
        u64::from(count),
        size_of::<RawPolygonInfo>() as u64,
    )?;
    let mut infos = Vec::with_capacity(count as usize);
    for _ in 0..count {
        infos.push(read_polygon_info(reader)?);
    }
    allocation.reserve(
        "polygon.materials",
        reader.position(),
        u64::from(count),
        size_of::<RawPolygon>() as u64,
    )?;
    let mut polygons = Vec::with_capacity(count as usize);
    let mut material_groups: Vec<Vec<RawMaterialGroup>> = Vec::with_capacity(count as usize);
    for (polygon, info) in infos.iter().enumerate() {
        let polygon_index = polygon as u32;
        let corners = info.corners();
        let unknown_bits = info.unknown_flag_bits();
        if unknown_bits != 0 {
            findings.push(ParseFinding::UnknownPolygonFlagBits {
                mesh,
                polygon: polygon_index,
                bits: unknown_bits,
            });
        }
        if info.is_triangle_strip() && !info.has_normals() {
            findings.push(ParseFinding::StripWithoutNormals {
                mesh,
                polygon: polygon_index,
            });
        }
        if info.mat_count == 0 {
            findings.push(ParseFinding::PolygonWithoutMaterial {
                mesh,
                polygon: polygon_index,
            });
        }
        if corners < 3 {
            findings.push(ParseFinding::PolygonTooFewCorners {
                mesh,
                polygon: polygon_index,
                corners,
            });
        }
        let (stored, groups) = read_polygon(reader, allocation, info, corners)?;
        polygons.push(stored);
        material_groups.push(groups);
    }
    debug_assert_eq!(polygons.len(), material_groups.len());
    Ok(PolygonData {
        records: infos,
        polygons,
        material_groups,
    })
}

/// The three per-polygon results, all in stored order and all the same length.
struct PolygonData {
    /// The raw 40-byte records.
    records: Vec<RawPolygonInfo>,
    /// The parsed IR polygons.
    polygons: Vec<RawPolygon>,
    /// Every stored material group, one entry per polygon.
    material_groups: Vec<Vec<RawMaterialGroup>>,
}

fn read_polygon_info(reader: &mut Reader<'_>) -> Result<RawPolygonInfo, GameZError> {
    Ok(RawPolygonInfo {
        vertex_info: reader.read_u32("polygon.vertex_info")?,
        unk04: reader.read_i32("polygon.unk04")?,
        vertices_ptr: reader.read_u32("polygon.vertices_ptr")?,
        normals_ptr: reader.read_u32("polygon.normals_ptr")?,
        mat_count: reader.read_u32("polygon.mat_count")?,
        uvs_ptr: reader.read_u32("polygon.uvs_ptr")?,
        colors_ptr: reader.read_u32("polygon.colors_ptr")?,
        unk28: reader.read_u32("polygon.unk28")?,
        unk32: reader.read_u32("polygon.unk32")?,
        unk36: reader.read_u32("polygon.unk36")?,
    })
}

fn read_polygon(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    info: &RawPolygonInfo,
    corners: u32,
) -> Result<(RawPolygon, Vec<RawMaterialGroup>), GameZError> {
    // `corners` is `vertex_info & 0x1FF`, so this bound is the format's own and
    // not an invented limit: no stored count can ask for more.
    debug_assert!(corners <= MAX_POLYGON_CORNERS);
    let positions = read_u32s(reader, allocation, corners, "polygon.positions")?;
    let normals = if info.has_normals() {
        Some(read_u32s(reader, allocation, corners, "polygon.normals")?)
    } else {
        None
    };
    let groups_count = info.mat_count;
    allocation.reserve(
        "polygon.material_indices",
        reader.position(),
        u64::from(groups_count),
        4,
    )?;
    let mut group_materials = Vec::with_capacity(groups_count as usize);
    for _ in 0..groups_count {
        group_materials.push(reader.read_u32("polygon.material_index")?);
    }
    allocation.reserve(
        "polygon.material_groups",
        reader.position(),
        u64::from(groups_count),
        size_of::<RawMaterialGroup>() as u64,
    )?;
    let mut materials = Vec::with_capacity(groups_count as usize);
    for &material in &group_materials {
        allocation.reserve("polygon.uvs", reader.position(), u64::from(corners), 8)?;
        let mut uvs = Vec::with_capacity(corners as usize);
        for _ in 0..corners {
            uvs.push([
                reader.read_f32("polygon.uv.u")?,
                reader.read_f32("polygon.uv.v")?,
            ]);
        }
        materials.push(RawMaterialGroup { material, uvs });
    }
    allocation.reserve(
        "polygon.colors",
        reader.position(),
        u64::from(corners),
        VEC3_BYTES,
    )?;
    let mut colors = Vec::with_capacity(corners as usize);
    for _ in 0..corners {
        colors.push([
            reader.read_f32("polygon.color.r")?,
            reader.read_f32("polygon.color.g")?,
            reader.read_f32("polygon.color.b")?,
        ]);
    }
    let mut corners_out = Vec::with_capacity(corners as usize);
    for corner in 0..corners as usize {
        let normal = normals.as_ref().map(|indices| indices[corner]);
        let uv = materials.first().map(|group| group.uvs[corner]);
        corners_out.push(RawCorner {
            position: positions[corner],
            normal,
            uv,
            color: Some(colors[corner]),
        });
    }
    Ok((
        RawPolygon {
            kind: info.kind(),
            raw_flags: info.flags(),
            // The first stored group, mirrored onto the IR's single-valued field,
            // so a consumer of a one-group polygon is never wrong.
            material: materials.first().map_or(0, |group| group.material),
            corners: corners_out,
        },
        materials,
    ))
}

fn read_u32s(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    count: u32,
    field: &str,
) -> Result<Vec<u32>, GameZError> {
    allocation.reserve(field, reader.position(), u64::from(count), 4)?;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        out.push(reader.read_u32(field)?);
    }
    Ok(out)
}

fn read_mesh_materials(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    count: u32,
) -> Result<Vec<RawMeshMaterialInfo>, GameZError> {
    allocation.reserve(
        "mesh.materials",
        reader.position(),
        u64::from(count),
        size_of::<RawMeshMaterialInfo>() as u64,
    )?;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        out.push(RawMeshMaterialInfo {
            material_index: reader.read_u32("mesh_material.material_index")?,
            polygon_usage_count: reader.read_u32("mesh_material.polygon_usage_count")?,
            unk_ptr: reader.read_u32("mesh_material.unk_ptr")?,
        });
    }
    Ok(out)
}
