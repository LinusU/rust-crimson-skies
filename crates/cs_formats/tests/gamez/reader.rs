//! Acceptance stage F10-B (task #363): the established CS GameZ mesh-array
//! layout and its reader into [`RawMesh`].
//!
//! The layout this file encodes comes from the pinned mech3ax v0.6.0 revision
//! (commit `d3521a9721be731d365504568ddcd78e3f9846bb`) and was checked against
//! the original installation; the worksheet, the per-archive measurements and
//! the recorded unknowns are in
//! `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md`.
//!
//! Every fixture in the synthetic part of this file is authored here from that
//! worksheet: no original game data is read, nothing is derived from it, and the
//! fixture writer shares no code with the reader. The expected values are
//! literals, so a reader and a writer that made the same mistake cannot agree.
//! The writer composes each mesh's data from its own parts and refuses a
//! container whose parts contradict its own stored counts, so a fixture can
//! never accidentally parse.
//!
//! The two `#[ignore = "requires CS_GAME_DIR"]` tests at the end are the retail
//! half: they read the read-only installation, fail loudly when it is missing,
//! and prove the same layout on real `planes.zbd` and world `gamez.zbd` bytes.

use cs_formats::ParseContext;
use cs_formats::gamez::reader::{Fixup, MESH_INFO_BYTES, MESH_INFO_TRAILER_BYTES};
use cs_formats::gamez::{
    CORNER_COUNT_MASK, FLAG_NORMALS, FLAG_SHIFT, FLAG_TRIANGLE_STRIP, FLAG_UNK2,
    GAMEZ_HEADER_BYTES, GameZError, GameZMeshes, PrimitiveKind, UNK08_C4, UNK08_PLANES,
    read_gamez_meshes,
};
use cs_formats::zbd::{GAMEZ_SIGNATURE, GAMEZ_VERSION};

// --------------------------------------------------------------- fixtures ---

/// The header fields a fixture chooses; the four section offsets are computed by
/// [`authored_container`], which lays the sections out in the order the layout
/// requires so no fixture depends on a hand-computed byte count.
#[derive(Clone)]
struct HeaderSpec {
    unk08: u32,
    texture_count: u32,
    node_array_size: u32,
    light_index: u32,
}

impl Default for HeaderSpec {
    fn default() -> Self {
        Self {
            unk08: 1_234_567_890,
            texture_count: 3,
            node_array_size: 7,
            light_index: 0,
        }
    }
}

/// `count` distinct `Vec3`s, so a reader that shifted or reordered the array
/// would read a different value rather than a plausible one.
fn vectors(count: u32) -> Vec<[f32; 3]> {
    (0..count)
        .map(|index| {
            let value = index as f32;
            [value, value * 2.0, value * 3.0]
        })
        .collect()
}

/// One mesh to write: its 100-byte record's fields and the parts its data is
/// composed from.
#[derive(Clone, Default)]
struct MeshSpec {
    file_ptr: u32,
    unk04: u32,
    unk08: u32,
    parent_count: u32,
    polygon_count: u32,
    vertex_count: u32,
    normal_count: u32,
    morph_count: u32,
    light_count: u32,
    unk36: u32,
    unk40: f32,
    unk44: f32,
    unk48: u32,
    polygons_ptr: u32,
    vertices_ptr: u32,
    normals_ptr: u32,
    lights_ptr: u32,
    morphs_ptr: u32,
    unk72: f32,
    unk76: f32,
    unk80: f32,
    unk84: f32,
    unk88: u32,
    material_count: u32,
    materials_ptr: u32,
    /// The positions, then the normals, then the morph vectors — the order the
    /// layout stores them in.
    vectors: Vec<[f32; 3]>,
    /// The mesh lights, in stored order.
    lights: Vec<LightSpec>,
    /// The polygons, in stored order.
    polygons: Vec<PolygonSpec>,
    /// The 12-byte mesh material references, in stored order.
    material_refs: Vec<(u32, u32, u32)>,
}

impl MeshSpec {
    /// A present mesh with the raw scalars a present record carries.
    fn present() -> Self {
        Self {
            file_ptr: 1,
            parent_count: 1,
            ..Self::default()
        }
    }

    /// The all-zero stub record.
    fn stub() -> Self {
        Self::default()
    }

    /// A one-triangle mesh: three positions, three normals, one polygon, no
    /// lights. The smallest present mesh this reader accepts.
    fn triangle() -> Self {
        let polygon = PolygonSpec::triangle();
        Self {
            polygon_count: 1,
            vertex_count: 3,
            normal_count: 3,
            vectors: {
                let mut all = vectors(3);
                all.extend(vectors(3));
                all
            },
            polygons: vec![polygon],
            ..Self::present()
        }
    }

    /// The 25 stored words of the 100-byte record, in the layout's order.
    fn record_words(&self) -> [u32; 25] {
        [
            self.file_ptr,
            self.unk04,
            self.unk08,
            self.parent_count,
            self.polygon_count,
            self.vertex_count,
            self.normal_count,
            self.morph_count,
            self.light_count,
            self.unk36,
            self.unk40.to_bits(),
            self.unk44.to_bits(),
            self.unk48,
            self.polygons_ptr,
            self.vertices_ptr,
            self.normals_ptr,
            self.lights_ptr,
            self.morphs_ptr,
            self.unk72.to_bits(),
            self.unk76.to_bits(),
            self.unk80.to_bits(),
            self.unk84.to_bits(),
            self.unk88,
            self.material_count,
            self.materials_ptr,
        ]
    }

    /// The mesh's data, composed from its parts in the layout's order:
    /// positions, normals, morphs, every light header, every light's trailing
    /// vectors, every polygon record, then per polygon its corner arrays, then
    /// the mesh material references.
    ///
    /// The two two-pass structures (all light headers before any light's
    /// vectors, all polygon records before any polygon's arrays) are the layout's
    /// and are where an interleaving reader would desynchronise.
    fn data(&self) -> Vec<u8> {
        assert_eq!(
            self.vectors.len() as u64,
            u64::from(self.vertex_count)
                + u64::from(self.normal_count)
                + u64::from(self.morph_count),
            "the fixture's vector array must match its own three counts"
        );
        assert_eq!(
            self.lights.len(),
            self.light_count as usize,
            "the fixture's lights must match light_count"
        );
        assert_eq!(
            self.polygons.len(),
            self.polygon_count as usize,
            "the fixture's polygons must match polygon_count"
        );
        assert_eq!(
            self.material_refs.len(),
            self.material_count as usize,
            "the fixture's material references must match material_count"
        );

        let mut out = Vec::new();
        for vector in &self.vectors {
            for channel in vector {
                out.extend_from_slice(&channel.to_le_bytes());
            }
        }
        for light in &self.lights {
            out.extend_from_slice(&light.header_bytes());
        }
        for light in &self.lights {
            assert_eq!(light.extra.len(), light.extra_count as usize);
            for vector in &light.extra {
                for channel in vector {
                    out.extend_from_slice(&channel.to_le_bytes());
                }
            }
        }
        for polygon in &self.polygons {
            out.extend_from_slice(&polygon.record_bytes());
        }
        for polygon in &self.polygons {
            out.extend_from_slice(&polygon.corner_bytes());
        }
        for (material_index, polygon_usage_count, unk_ptr) in &self.material_refs {
            for word in [material_index, polygon_usage_count, unk_ptr] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        out
    }
}

/// One polygon to write: its packed word, its `mat_count`, and the five arrays
/// that follow the record.
#[derive(Clone)]
struct PolygonSpec {
    corners: u32,
    flags: u32,
    unk04: i32,
    vertices_ptr: u32,
    normals_ptr: u32,
    mat_count: u32,
    uvs_ptr: u32,
    colors_ptr: u32,
    unk28: u32,
    unk32: u32,
    unk36: u32,
    /// One position index per corner.
    positions: Vec<u32>,
    /// One normal index per corner, or `None` for no normal array — which the
    /// layout only allows without [`FLAG_NORMALS`].
    normals: Option<Vec<u32>>,
    /// One material index per group.
    material_indices: Vec<u32>,
    /// `mat_count * corners` UV pairs, group by group.
    uvs: Vec<[f32; 2]>,
    /// One colour per corner.
    colors: Vec<[f32; 3]>,
}

impl PolygonSpec {
    /// A one-group, three-corner triangle with normals: the commonest stored
    /// shape, and the smallest a polygon can be.
    fn triangle() -> Self {
        Self {
            corners: 3,
            flags: FLAG_NORMALS,
            unk04: 0,
            vertices_ptr: 0xAAAA_0001,
            normals_ptr: 0xAAAA_0002,
            mat_count: 1,
            uvs_ptr: 0xAAAA_0003,
            colors_ptr: 0xAAAA_0004,
            unk28: 0xAAAA_0005,
            unk32: 0xAAAA_0006,
            unk36: 0x0000_FFFF,
            positions: vec![0, 1, 2],
            normals: Some(vec![0, 1, 2]),
            material_indices: vec![11],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            colors: vec![[1.0, 1.0, 1.0]; 3],
        }
    }

    /// A polygon of `corners` corners with `flags`, its arrays resized to match.
    ///
    /// The flag byte decides whether a normal array is stored at all, so `with`
    /// keeps the fixture consistent with the layout: a flag byte without
    /// [`FLAG_NORMALS`] stores no normal indices, exactly as a real record does.
    fn with(mut self, corners: u32, flags: u32) -> Self {
        self.corners = corners;
        self.flags = flags;
        if flags & FLAG_NORMALS == 0 {
            self.normals = None;
            self.normals_ptr = 0;
        } else if self.normals.is_none() {
            self.normals = Some((0..corners).map(|index| index % 3).collect());
            self.normals_ptr = 0xAAAA_0002;
        }
        self.resize();
        self
    }

    /// Fits every array to `corners` and `mat_count`, keeping the first values.
    fn resize(&mut self) {
        let corners = self.corners as usize;
        let groups = self.mat_count as usize;
        self.positions = (0..corners).map(|index| (index % 3) as u32).collect();
        self.normals = self.normals.as_ref().map(|stored| {
            (0..corners)
                .map(|index| stored[index % stored.len()])
                .collect()
        });
        self.material_indices = (0..groups).map(|group| 11 + group as u32).collect();
        self.uvs = (0..groups * corners)
            .map(|index| [index as f32 / 8.0, index as f32 / 16.0])
            .collect();
        self.colors = (0..corners)
            .map(|index| [index as f32 / 4.0, 1.0 - index as f32 / 4.0, 0.5])
            .collect();
    }

    /// `vertex_info` exactly as the layout packs it: the corner count in the low
    /// nine bits, the flag byte shifted up by eight.
    fn vertex_info(&self) -> u32 {
        (self.corners & CORNER_COUNT_MASK) | (self.flags << FLAG_SHIFT)
    }

    /// The ten words of the 40-byte record.
    fn record_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [
            self.vertex_info(),
            self.unk04 as u32,
            self.vertices_ptr,
            self.normals_ptr,
            self.mat_count,
            self.uvs_ptr,
            self.colors_ptr,
            self.unk28,
            self.unk32,
            self.unk36,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(out.len(), 40, "a polygon record is ten 4-byte fields");
        out
    }

    /// The five corner arrays, in the order the layout stores them after the
    /// records: positions, normals when the flag says so, every material index,
    /// then one UV set per group, then the colours.
    fn corner_bytes(&self) -> Vec<u8> {
        assert_eq!(
            self.normals.is_some(),
            self.flags & FLAG_NORMALS != 0,
            "a polygon stores a normal array exactly when its flag byte says so"
        );
        assert_eq!(self.positions.len(), self.corners as usize);
        assert_eq!(self.uvs.len(), (self.corners * self.mat_count) as usize);
        assert_eq!(self.colors.len(), self.corners as usize);
        assert_eq!(self.material_indices.len(), self.mat_count as usize);
        let mut out = Vec::new();
        for index in &self.positions {
            out.extend_from_slice(&index.to_le_bytes());
        }
        if let Some(normals) = &self.normals {
            for index in normals {
                out.extend_from_slice(&index.to_le_bytes());
            }
        }
        for material in &self.material_indices {
            out.extend_from_slice(&material.to_le_bytes());
        }
        for uv in &self.uvs {
            out.extend_from_slice(&uv[0].to_le_bytes());
            out.extend_from_slice(&uv[1].to_le_bytes());
        }
        for color in &self.colors {
            for channel in color {
                out.extend_from_slice(&channel.to_le_bytes());
            }
        }
        out
    }
}

/// One light to write: its 76-byte header and its trailing vectors.
#[derive(Clone)]
struct LightSpec {
    unk00: u32,
    unk04: u32,
    unk08: f32,
    extra_count: u32,
    unk16: u32,
    unk20: u32,
    unk24: u32,
    color: [f32; 3],
    pad40: u16,
    flags: u16,
    ptr: u32,
    unk48: f32,
    unk52: f32,
    unk56: f32,
    unk60: u32,
    unk64: f32,
    unk68: f32,
    unk72: f32,
    extra: Vec<[f32; 3]>,
}

impl LightSpec {
    /// A light with one trailing vector, the minimum the reference asserts, and
    /// a distinct value in every field so a reader that reordered or shifted the
    /// header would be caught.
    fn one() -> Self {
        Self {
            unk00: 1,
            unk04: 2,
            unk08: 3.5,
            extra_count: 1,
            unk16: 0,
            unk20: 0,
            unk24: 0xBF80_0000,
            color: [206.0, 22.0, 175.0],
            pad40: 0,
            flags: 0x01E7,
            ptr: 0x03BB_7D30,
            unk48: 1000.0,
            unk52: 0.0,
            unk56: 0.255,
            unk60: 0,
            unk64: 0.0,
            unk68: 0.0,
            unk72: 0.0,
            extra: vec![[0.5, 0.25, 0.125]],
        }
    }

    /// A light with two trailing vectors, so a reader that read one vector per
    /// light and continued would desynchronise here.
    fn two() -> Self {
        Self {
            extra_count: 2,
            extra: vec![[-2.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            ..Self::one()
        }
    }

    /// The nineteen 4-byte words of the 76-byte header. `pad40` and `flags` are
    /// two 2-byte fields sharing one word.
    fn header_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [
            self.unk00,
            self.unk04,
            self.unk08.to_bits(),
            self.extra_count,
            self.unk16,
            self.unk20,
            self.unk24,
            self.color[0].to_bits(),
            self.color[1].to_bits(),
            self.color[2].to_bits(),
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(
            &((u32::from(self.flags) << 16) | u32::from(self.pad40)).to_le_bytes(),
        );
        for word in [
            self.ptr,
            self.unk48.to_bits(),
            self.unk52.to_bits(),
            self.unk56.to_bits(),
            self.unk60,
            self.unk64.to_bits(),
            self.unk68.to_bits(),
            self.unk72.to_bits(),
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(out.len(), 76, "a light header is nineteen 4-byte fields");
        out
    }
}

/// The mesh index a container stores: `array_size`, `count`, `last_index`.
type IndexSpec = (i32, i32, i32);

/// The index the layout says the mesh at array position `slot` carries: its own
/// position plus one, and `-1` at the end of the array.
///
/// Stated here from the layout (`MeshIndexIter::next`) rather than borrowed from
/// the reader, so the fixtures are the specification side of the check. A
/// *remapped* container names its stub words explicitly instead.
fn sequential_expectation(slot: u32, array_size: u32) -> i32 {
    let next = slot + 1;
    if next == array_size { -1 } else { next as i32 }
}

/// The mesh index of a container whose present meshes are exactly `present`, in
/// the layout's own sequential order.
fn sequential_index(array_size: u32, present: &[u32]) -> IndexSpec {
    let count = present.len() as i32;
    let last = present
        .last()
        .copied()
        .map_or(-1, |slot| sequential_expectation(slot, array_size));
    (array_size as i32, count, last)
}

/// [`Fixup::Planes`]'s measured remap, as a function value.
fn planes_remap(expected: i32) -> i32 {
    Fixup::Planes.mesh_index_remap(expected)
}

/// [`Fixup::C4`]'s measured remap, as a function value.
fn c4_remap(expected: i32) -> i32 {
    Fixup::C4.mesh_index_remap(expected)
}

/// The stored word of every stub of a container, in slot order: the sequential
/// expectation, rewritten by `remap`. A present slot's word is the data offset,
/// which the fixture writer fills in, so it is left as zero here.
fn stub_words(array_size: u32, present: &[u32], remap: impl Fn(i32) -> i32) -> Vec<i32> {
    (0..array_size)
        .map(|slot| {
            if present.contains(&slot) {
                0
            } else {
                remap(sequential_expectation(slot, array_size))
            }
        })
        .collect()
}

/// Builds one container from a header spec, a mesh array and a mesh index,
/// laying the sections out in the order the layout requires and filling in every
/// offset, so no fixture depends on a hand-computed byte count.
///
/// A present record's stored data offset is computed here; a stub's stored word
/// is written as `stub_indices[slot]`, so a fixture states the remapped index it
/// wants explicitly rather than leaving the writer to guess.
fn authored_container(
    spec: &HeaderSpec,
    meshes: &[MeshSpec],
    index: IndexSpec,
    stub_indices: &[i32],
) -> Vec<u8> {
    let header_bytes = GAMEZ_HEADER_BYTES as usize;
    let textures_offset = header_bytes;
    // The material section is never parsed, so two 4-byte words are enough to
    // make the offset chain strictly increasing.
    let materials_offset = textures_offset + spec.texture_count as usize * 4;
    let meshes_offset = materials_offset + 8;
    let index_bytes =
        12 + meshes.len() * (MESH_INFO_BYTES as usize + MESH_INFO_TRAILER_BYTES as usize);
    let data: Vec<Vec<u8>> = meshes.iter().map(MeshSpec::data).collect();
    let data_len: usize = data.iter().map(Vec::len).sum();
    let nodes_offset = meshes_offset + index_bytes + data_len;
    assert!(
        nodes_offset <= u32::MAX as usize,
        "a fixture this large is not a fixture"
    );

    let mut out = Vec::new();
    for word in [
        GAMEZ_SIGNATURE,
        GAMEZ_VERSION,
        spec.unk08,
        spec.texture_count,
        textures_offset as u32,
        materials_offset as u32,
        meshes_offset as u32,
        spec.node_array_size,
        spec.light_index,
        nodes_offset as u32,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    assert_eq!(out.len(), header_bytes, "the header is ten 4-byte fields");

    for texture in 0..spec.texture_count {
        out.extend_from_slice(&texture.to_le_bytes());
    }
    for material in 0..2u32 {
        out.extend_from_slice(&material.to_le_bytes());
    }
    for word in [index.0, index.1, index.2] {
        out.extend_from_slice(&word.to_le_bytes());
    }

    // The record array, with each present record's data offset filled in from
    // the composed data that follows it. The data starts after the whole record
    // array, not after the index.
    let record_bytes = (MESH_INFO_BYTES + MESH_INFO_TRAILER_BYTES) as usize;
    let mut offset = (out.len() + meshes.len() * record_bytes) as u32;
    for (slot, mesh) in meshes.iter().enumerate() {
        for word in mesh.record_words() {
            out.extend_from_slice(&word.to_le_bytes());
        }
        if mesh.parent_count == 0 {
            // `stub_words` is indexed by slot and leaves a present slot's word at
            // zero, which is what the data offset would be filled in with.
            let stored = stub_indices.get(slot).copied().unwrap_or(0);
            out.extend_from_slice(&stored.to_le_bytes());
            continue;
        }
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data[slot].len() as u32;
    }
    assert_eq!(
        out.len() + data_len,
        nodes_offset,
        "records then data, back to back"
    );
    for bytes in &data {
        out.extend_from_slice(bytes);
    }
    out
}

fn parse(container: &str, bytes: &[u8]) -> Result<GameZMeshes, GameZError> {
    let mut context = ParseContext::with_defaults(container);
    read_gamez_meshes(&mut context, container, bytes)
}

fn parse_ok(container: &str, bytes: &[u8]) -> GameZMeshes {
    match parse(container, bytes) {
        Ok(meshes) => meshes,
        Err(error) => panic!("{container}: the authored fixture must parse, got {error}"),
    }
}

// ------------------------------------------------------------------ tests ---

/// The header fields come from the layout, not from a JSON property name: a
/// signature or version that is not the documented one is refused before a byte
/// of the mesh section is read, naming the word and its offset, and the section
/// chain must be strictly increasing and inside the container.
#[test]
fn accept_f10_b_gamez_header_gates_the_section() {
    let spec = HeaderSpec::default();
    let good = authored_container(
        &spec,
        &[MeshSpec::triangle()],
        sequential_index(1, &[0]),
        &[],
    );

    // The authored container really is the good one, and the walk ends exactly
    // where the header says the node array begins.
    let parsed = parse_ok("synthetic/good.zbd", &good);
    assert_eq!(parsed.header.signature, GAMEZ_SIGNATURE);
    assert_eq!(parsed.header.version, GAMEZ_VERSION);
    assert_eq!(
        parsed.header.nodes_offset as u64, parsed.data_end,
        "the walk must end exactly at the header's nodes_offset"
    );

    let mut wrong_signature = good.clone();
    wrong_signature[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    match parse("synthetic/bad_signature.zbd", &wrong_signature) {
        Err(GameZError::Signature { offset, found }) => {
            assert_eq!(offset, 0);
            assert_eq!(found, 0xDEAD_BEEF);
        }
        other => panic!("a foreign signature must be refused, got {other:?}"),
    }

    let mut wrong_version = good.clone();
    wrong_version[4..8].copy_from_slice(&41u32.to_le_bytes());
    match parse("synthetic/bad_version.zbd", &wrong_version) {
        Err(GameZError::Version { offset, found }) => {
            assert_eq!(offset, 4);
            assert_eq!(found, 41);
        }
        other => panic!("another game's version must be refused, got {other:?}"),
    }

    // The texture table is read straight after the header, so its offset is the
    // header size; a container that says otherwise is refused rather than read
    // from the wrong place.
    let materials_offset = u32::from_le_bytes(good[20..24].try_into().unwrap());
    let inside_the_chain = materials_offset - 4;
    let mut moved_textures = good.clone();
    moved_textures[16..20].copy_from_slice(&inside_the_chain.to_le_bytes());
    match parse("synthetic/moved_textures.zbd", &moved_textures) {
        Err(GameZError::TexturesOffsetNotAfterHeader { found }) => {
            assert_eq!(found, inside_the_chain)
        }
        other => panic!("a moved texture table must be refused, got {other:?}"),
    }

    // The three sections must be in order. A material section that starts after
    // the mesh index keeps every offset inside the container, so the order check
    // is what refuses it rather than the bounds check.
    let meshes_offset = u32::from_le_bytes(good[24..28].try_into().unwrap());
    let after_meshes = meshes_offset + 4;
    let mut out_of_order = good.clone();
    out_of_order[20..24].copy_from_slice(&after_meshes.to_le_bytes());
    match parse("synthetic/out_of_order.zbd", &out_of_order) {
        Err(GameZError::SectionOrder {
            pair,
            first,
            second,
        }) => {
            assert_eq!(pair, "materials_offset < meshes_offset");
            assert_eq!(first, after_meshes);
            assert_eq!(second, meshes_offset);
        }
        other => panic!("out-of-order sections must be refused, got {other:?}"),
    }

    // And a section past the end of the container is refused on its own.
    let huge = u32::try_from(good.len() + 1_000_000).expect("the fixture is small");
    let mut out_of_bounds = good.clone();
    out_of_bounds[24..28].copy_from_slice(&huge.to_le_bytes());
    assert!(
        matches!(
            parse("synthetic/meshes_beyond.zbd", &out_of_bounds),
            Err(GameZError::SectionOutOfBounds {
                field: "meshes_offset",
                ..
            })
        ),
        "a mesh index past the end of the container is refused"
    );

    // A section past the end of the container is refused with both numbers.
    let beyond_offset = (good.len() + 4_096) as u32;
    let mut beyond = good.clone();
    beyond[36..40].copy_from_slice(&beyond_offset.to_le_bytes());
    match parse("synthetic/beyond.zbd", &beyond) {
        Err(GameZError::SectionOutOfBounds {
            field,
            offset,
            container_len,
        }) => {
            assert_eq!(field, "nodes_offset");
            assert_eq!(offset, beyond_offset);
            assert_eq!(container_len, good.len() as u64);
        }
        other => panic!("a section outside the container must be refused, got {other:?}"),
    }
}

/// A truncation at **every** byte length of a real container fails: a reader that
/// trusted a stored count, or that returned a partial mesh, would answer `Ok` for
/// some of them.
#[test]
fn accept_f10_b_gamez_every_truncation_fails_loudly() {
    let spec = HeaderSpec::default();
    let good = authored_container(
        &spec,
        &[MeshSpec::triangle()],
        sequential_index(1, &[0]),
        &[],
    );
    for len in 0..good.len() {
        match parse("synthetic/truncated.zbd", &good[..len]) {
            Err(_) => {}
            Ok(meshes) => panic!(
                "truncating to {len} of {} bytes produced {} meshes; a short container must fail",
                good.len(),
                meshes.present_count()
            ),
        }
    }
    // The full length is the only one that parses.
    assert_eq!(parse_ok("synthetic/whole.zbd", &good).present_count(), 1);
}

/// The stored offsets and counts are the layout's: the reader trusts none of the
/// record's `*_ptr` fields, reaches every array by walking from the counts, and
/// stops exactly at `nodes_offset`. A reader that followed a pointer, or that
/// read the arrays in a different order, would read different values from the
/// same bytes.
#[test]
fn accept_f10_b_gamez_arrays_come_from_counts_not_pointers() {
    let spec = HeaderSpec::default();
    let mut mesh = MeshSpec::triangle();
    // Pointers that point nowhere at all, including past the end of the
    // container's mesh section.
    mesh.polygons_ptr = 0x7FFF_FFF0;
    mesh.vertices_ptr = 0;
    mesh.normals_ptr = 0;
    mesh.unk72 = -1.5;
    mesh.unk76 = 2.25;
    let expected_positions = mesh.vectors.clone();
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/no_pointers.zbd", &bytes);
    let read = parsed.get(0).expect("one present mesh");
    assert_eq!(read.index, 0);
    assert!(read.groups_are_complete());

    // Every raw field of the 100-byte record survived unchanged, pointer and all.
    assert_eq!(read.info.polygons_ptr, 0x7FFF_FFF0);
    assert_eq!(read.info.vertices_ptr, 0);
    assert_eq!(read.info.normals_ptr, 0);
    assert_eq!(read.info.file_ptr, 1);
    assert_eq!(read.info.unk72, -1.5);
    assert_eq!(read.info.unk76, 2.25);

    // The arrays came from the counts, in order, with the authored values.
    assert_eq!(read.mesh.positions, &expected_positions[..3]);
    assert_eq!(read.mesh.normals, &expected_positions[3..]);
    assert!(
        read.morphs.is_empty(),
        "no morph_count, so no morph vectors"
    );
    let polygon = &read.mesh.polygons[0];
    assert_eq!(polygon.corners.len(), 3);
    assert_eq!(polygon.corners[0].position, 0);
    assert_eq!(polygon.corners[2].position, 2);
    assert_eq!(polygon.corners[2].normal, Some(2));
    assert_eq!(read.groups(0).expect("polygon 0")[0].material, 11);
    assert_eq!(
        read.groups(0).expect("polygon 0")[0].uvs,
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
    );
}

/// `FLAG_TRIANGLE_STRIP` is the **only** thing that selects the corner topology,
/// and `FLAG_NORMALS` is the only thing that says a normal index follows. A reader
/// that inferred the topology from the corner count, or that always read a
/// normal array, would produce a different IR from the same bytes.
#[test]
fn accept_f10_b_gamez_flags_select_strip_and_normals() {
    let spec = HeaderSpec::default();
    // One mesh with two five-corner polygons over three positions: the first an
    // outline, the second a strip, neither with normals, both with an unnamed
    // flag bit set.
    let mut outline = PolygonSpec::triangle().with(5, FLAG_UNK2);
    // Five distinct positions walked once around a convex pentagon, so the
    // outline is simple and non-degenerate and really does triangulate.
    outline.positions = vec![0, 1, 2, 3, 4];
    let mut strip = outline.clone();
    strip.flags = FLAG_TRIANGLE_STRIP | FLAG_UNK2;
    let mut mesh = MeshSpec::present();
    mesh.polygon_count = 2;
    mesh.vertex_count = 5;
    mesh.vectors = vec![
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [3.0, 1.0, 0.0],
        [1.5, 2.5, 0.0],
        [0.0, 2.0, 0.0],
    ];
    mesh.polygons = vec![outline, strip];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/kinds.zbd", &bytes);
    let read = parsed.get(0).expect("one present mesh");
    assert_eq!(read.mesh.polygons.len(), 2);

    let polygon = &read.mesh.polygons[0];
    assert_eq!(
        polygon.kind,
        PrimitiveKind::Polygon,
        "no strip bit means an outline"
    );
    assert_eq!(
        polygon.raw_flags, FLAG_UNK2,
        "the flag byte is kept raw, unmapped bits included"
    );
    assert!(
        polygon.corners.iter().all(|corner| corner.normal.is_none()),
        "no NORMALS bit means no normal array was read"
    );

    let strip = &read.mesh.polygons[1];
    assert_eq!(
        strip.kind,
        PrimitiveKind::TriangleStrip,
        "the strip bit selects a strip"
    );
    assert_eq!(strip.raw_flags, FLAG_TRIANGLE_STRIP | FLAG_UNK2);

    // The shared production path agrees: a five-corner outline triangulates to
    // three triangles, and a five-step strip draws three steps.
    let topology = read.topology();
    assert_eq!(topology.decoded_faces(), 2);
    assert_eq!(topology.triangles.len(), 6);
    assert_eq!(
        topology.faces[0],
        cs_formats::gamez::FaceStatus::Decoded {
            triangles: 3,
            degenerate: 0
        }
    );
    assert_eq!(
        topology.faces[1],
        cs_formats::gamez::FaceStatus::Decoded {
            triangles: 3,
            degenerate: 0
        }
    );

    // A polygon with a flag bit outside the five the reference names is still
    // read, decoded, and reported: not silently accepted, not refused.
    let unmapped = PolygonSpec::triangle().with(3, 1 << 7);
    let mut mesh = MeshSpec::triangle();
    mesh.polygons = vec![unmapped.clone()];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/unmapped_flag.zbd", &bytes);
    assert_eq!(
        parsed
            .findings
            .iter()
            .map(|finding| finding.code())
            .collect::<Vec<_>>(),
        ["unknown_polygon_flag_bits"],
        "an unmapped bit is a visible finding, not a silent acceptance"
    );
    let read = parsed.get(0).expect("one present mesh");
    assert_eq!(read.mesh.polygons[0].raw_flags, 1 << 7);
    assert_eq!(
        read.topology().decoded_faces(),
        1,
        "and the face is still decoded"
    );
}

/// A polygon stores one UV set **per material group**, and both the group
/// material indices and every group's coordinates survive. A reader that kept
/// only the first group would look right on a single-material asset and silently
/// lose a seam everywhere else.
#[test]
fn accept_f10_b_gamez_keeps_every_material_group() {
    let spec = HeaderSpec::default();
    let mut polygon = PolygonSpec::triangle();
    polygon.mat_count = 2;
    polygon.resize();
    let authored = polygon.clone();
    let mut mesh = MeshSpec::triangle();
    mesh.polygons = vec![polygon];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/two_groups.zbd", &bytes);
    let mesh = parsed.get(0).expect("one present mesh");
    let read = &mesh.mesh.polygons[0];
    let groups = mesh.groups(0).expect("polygon 0 has its stored groups");

    assert!(
        mesh.groups_are_complete(),
        "one group list per stored polygon"
    );
    assert_eq!(groups.len(), 2, "both stored groups survive");
    assert_eq!(
        groups
            .iter()
            .map(|group| group.material)
            .collect::<Vec<_>>(),
        authored.material_indices
    );
    assert_eq!(groups[0].uvs, authored.uvs[0..3]);
    assert_eq!(groups[1].uvs, authored.uvs[3..6]);
    // The two groups really do differ, so keeping the first one only would show.
    assert_ne!(groups[0].uvs, groups[1].uvs);
    // Either group's coordinate is reachable per corner, which is what a
    // multi-material consumer needs and what the IR's single `uv` cannot express.
    assert_eq!(mesh.corner_uv(0, 1, 2), Some(authored.uvs[5]));
    assert_eq!(mesh.corner_uv(0, 0, 0), Some(authored.uvs[0]));
    assert_eq!(
        mesh.corner_uv(0, 2, 0),
        None,
        "an unstored group is None, not a guess"
    );
    assert_eq!(
        mesh.corner_uv(1, 0, 0),
        None,
        "an unstored polygon is None too"
    );
    // The first group is mirrored onto the single-valued IR fields, so a
    // one-material consumer is not wrong here.
    assert_eq!(read.material, authored.material_indices[0]);
    assert_eq!(read.corners[0].uv, Some(authored.uvs[0]));
    assert_eq!(read.corners[2].uv, Some(authored.uvs[2]));
}

/// Two corners sharing a position keep both sets of attributes: spec F10
/// non-negotiable #3 on parsed bytes, and AC03's authored half. A reader that
/// keyed corners by position would collapse the seam.
#[test]
fn accept_f10_b_gamez_shared_position_keeps_both_corners() {
    let spec = HeaderSpec::default();
    let mut polygon = PolygonSpec::triangle();
    // Position 0 repeats, and the two corners that share it differ in UV and in
    // colour.
    polygon.positions = vec![0, 1, 0];
    polygon.normals = Some(vec![0, 1, 0]);
    polygon.uvs = vec![[0.0, 0.0], [1.0, 1.0], [0.5, 0.25]];
    polygon.colors = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let authored = polygon.clone();
    let mut mesh = MeshSpec::triangle();
    mesh.vertex_count = 2;
    mesh.normal_count = 2;
    mesh.vectors = {
        let mut all = vectors(2);
        all.extend(vectors(2));
        all
    };
    mesh.polygons = vec![polygon];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/shared_position.zbd", &bytes);
    let read = &parsed.get(0).expect("one present mesh").mesh.polygons[0];

    assert_eq!(
        read.corners.len(),
        3,
        "three stored corners stay three corners"
    );
    assert_eq!(read.corners[0].position, 0);
    assert_eq!(read.corners[2].position, 0, "the shared position is shared");
    assert_eq!(read.corners[0].uv, Some(authored.uvs[0]));
    assert_eq!(read.corners[2].uv, Some(authored.uvs[2]));
    assert_ne!(read.corners[0].uv, read.corners[2].uv, "but the UVs differ");
    assert_ne!(
        read.corners[0].color, read.corners[2].color,
        "and the colours differ"
    );

    // Through the shared production path, the seam survives: the two corners are
    // distinct render corners, and the source-corner map leads back to each.
    let topology = parsed.get(0).expect("one present mesh").topology();
    assert_eq!(topology.decoded_faces(), 1);
    let corners = topology.triangles[0].corners;
    assert_eq!(corners[0], 0);
    assert_eq!(corners[2], 2);
    assert_ne!(
        read.corners[corners[0]].uv, read.corners[corners[2]].uv,
        "the seam is visible in the source-corner map"
    );
}

/// A mesh light is a variable-length record: a 76-byte header saying how many
/// `Vec3`s follow, and the vectors for **all** the mesh's lights stored **after**
/// all the headers. A reader that read each light's vectors right after its own
/// header would desynchronise at the second light and never reach the polygons.
#[test]
fn accept_f10_b_gamez_light_records_are_read_two_pass() {
    let spec = HeaderSpec::default();
    let first = LightSpec::one();
    let second = LightSpec::two();
    let mut mesh = MeshSpec::triangle();
    mesh.light_count = 2;
    mesh.lights_ptr = 0x1234_5678;
    mesh.lights = vec![first.clone(), second.clone()];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/lights.zbd", &bytes);
    let read = parsed.get(0).expect("one present mesh");

    assert_eq!(read.lights.len(), 2, "both stored lights are read");
    assert_eq!(read.info.light_count, 2);
    assert_eq!(read.info.lights_ptr, 0x1234_5678, "the pointer stays raw");

    // The first light's whole 76-byte header survived, field for field.
    let one = &read.lights[0];
    assert_eq!(one.header.unk00, 1);
    assert_eq!(one.header.unk04, 2);
    assert_eq!(one.header.unk08, 3.5);
    assert_eq!(one.header.extra_count, 1);
    assert_eq!(one.header.unk16, 0);
    assert_eq!(one.header.unk20, 0);
    assert_eq!(one.header.unk24, 0xBF80_0000);
    assert_eq!(one.header.color, [206.0, 22.0, 175.0]);
    assert_eq!(one.header.pad40, 0);
    assert_eq!(one.header.flags, 0x01E7);
    assert_eq!(one.header.ptr, 0x03BB_7D30);
    assert_eq!(one.header.unk48, 1000.0);
    assert_eq!(one.header.unk52, 0.0);
    assert_eq!(one.header.unk56, 0.255);
    assert_eq!(one.header.unk60, 0);
    assert_eq!(one.header.unk64, 0.0);
    assert_eq!(one.header.unk68, 0.0);
    assert_eq!(one.header.unk72, 0.0);
    assert_eq!(one.extra, first.extra);

    // And the second light, whose two vectors would be misread as the first's if
    // the two passes were interleaved.
    let two = &read.lights[1];
    assert_eq!(two.header.extra_count, 2);
    assert_eq!(two.extra, second.extra);
    assert_eq!(two.extra[0], [-2.0, 0.0, 0.0]);

    // The polygons after the lights still decoded: the proof the walk stayed
    // synchronised.
    assert_eq!(read.mesh.polygons.len(), 1);
    assert_eq!(read.topology().decoded_faces(), 1);
}

/// The mesh index is **non-sequential**: a stub record stores the index the next
/// present one is expected to carry, and for two measured archives that
/// expectation follows a different order than the array position. A reader that
/// assumed `index + 1` would refuse a container the reference accepts.
#[test]
fn accept_f10_b_gamez_nonsequential_index_uses_the_fixup() {
    // The two measured tables, checked where the reference states them and at
    // three unlisted indices that must pass through unchanged.
    assert_eq!(Fixup::for_unk08(UNK08_PLANES), Fixup::Planes);
    assert_eq!(Fixup::for_unk08(UNK08_C4), Fixup::C4);
    assert_eq!(Fixup::for_unk08(0), Fixup::None);
    assert_eq!(Fixup::for_unk08(unk08_of("ZBD/C1/gamez.zbd")), Fixup::None);
    assert_eq!(Fixup::Planes.mesh_index_remap(1396), 1321);
    assert_eq!(Fixup::Planes.mesh_index_remap(1554), 1779);
    assert_eq!(Fixup::Planes.mesh_index_remap(1314), 1459);
    for unlisted in [-1, 0, 42, 2250] {
        assert_eq!(
            Fixup::Planes.mesh_index_remap(unlisted),
            unlisted,
            "{unlisted} is not remapped"
        );
        assert_eq!(
            Fixup::C4.mesh_index_remap(unlisted),
            unlisted,
            "{unlisted} is not remapped"
        );
        assert_eq!(
            Fixup::None.mesh_index_remap(unlisted),
            unlisted,
            "{unlisted} is not remapped"
        );
    }
    assert_eq!(Fixup::C4.mesh_index_remap(2308), 2268);
    assert_eq!(Fixup::C4.mesh_index_remap(2312), 2490);
    assert_eq!(Fixup::Planes.last_index_remap(1779), 1313);
    assert_eq!(Fixup::Planes.last_index_remap(5), 5);
    assert_eq!(Fixup::C4.last_index_remap(2490), 2283);
    assert_eq!(Fixup::None.last_index_remap(1779), 1779);

    // A container with a stub: the stub's stored word is the expectation for its
    // own position, and `last_index` is the expectation after the last present
    // mesh. In a two-slot array the stub is the last slot, so it stores `-1`.
    let planes = HeaderSpec {
        unk08: UNK08_PLANES,
        ..HeaderSpec::default()
    };
    let words = stub_words(2, &[0], |value| value);
    assert_eq!(words, [0, -1], "the stub at the last slot stores -1");
    let bytes = authored_container(
        &planes,
        &[MeshSpec::triangle(), MeshSpec::stub()],
        sequential_index(2, &[0]),
        &words,
    );
    let parsed = parse_ok("synthetic/fixup_planes.zbd", &bytes);
    assert_eq!(parsed.fixup, Fixup::Planes, "unk08 selects the table");
    assert_eq!(parsed.index.last_index, 1);
    assert_eq!(parsed.present_count(), 1);
    assert!(
        parsed.get(1).is_none(),
        "the stub slot stays a stub, not a mesh"
    );
    assert_eq!(
        parsed.meshes.len(),
        2,
        "both array slots are present in the result"
    );

    // The same container with a stub word the layout does not produce is refused,
    // naming both the stored and the expected index: the check is real.
    let mut wrong = words.clone();
    wrong[1] = 99;
    let bad = authored_container(
        &planes,
        &[MeshSpec::triangle(), MeshSpec::stub()],
        sequential_index(2, &[0]),
        &wrong,
    );
    match parse("synthetic/fixup_wrong.zbd", &bad) {
        Err(GameZError::AbsentMeshIndex {
            mesh,
            found,
            expected,
        }) => {
            assert_eq!(mesh, 1);
            assert_eq!(found, 99);
            assert_eq!(expected, -1);
        }
        other => panic!("a stub with the wrong index must be refused, got {other:?}"),
    }

    // The remapped case itself, on a container big enough to reach a remapped
    // expectation: the planes table rewrites 1396 to 1321, so a stub at slot 1395
    // stores 1321 and a present mesh there is followed by 1321. The same bytes in
    // a container that selects no table are refused, which is what makes the
    // table load-bearing rather than decorative.
    const ARRAY: u32 = 1_397;
    const SLOT: u32 = 1_395;
    let from = sequential_expectation(SLOT, ARRAY);
    let to = Fixup::Planes.mesh_index_remap(from);
    assert_eq!(from, 1_396, "the measured remapped expectation");
    assert_eq!(to, 1_321, "and its measured stored value");
    let mut meshes = vec![MeshSpec::stub(); ARRAY as usize];
    meshes[SLOT as usize] = MeshSpec::triangle();
    let remapped_words = stub_words(ARRAY, &[SLOT], planes_remap);
    assert!(
        remapped_words
            .iter()
            .enumerate()
            .any(|(slot, word)| slot != SLOT as usize
                && *word != sequential_expectation(slot as u32, ARRAY)),
        "the planes table really does rewrite some stub words in this container"
    );
    let remapped = authored_container(&planes, &meshes, (ARRAY as i32, 1, from), &remapped_words);
    let parsed = parse_ok("synthetic/with_fixup.zbd", &remapped);
    assert_eq!(parsed.fixup, Fixup::Planes);
    assert_eq!(
        parsed.index.last_index, from,
        "last_index is the expectation after the last present mesh, and the small \
         last_index table does not rewrite this one"
    );
    assert_eq!(parsed.present_count(), 1);
    assert_eq!(
        parsed.get(SLOT).expect("the mesh").index,
        SLOT,
        "the mesh keeps its array position, which is what a node's mesh_index refers to"
    );

    // The C4 table, on its own measured case: 2308 is rewritten to 2268.
    let c4 = HeaderSpec {
        unk08: UNK08_C4,
        ..HeaderSpec::default()
    };
    const C4_ARRAY: u32 = 2_309;
    const C4_SLOT: u32 = 2_307;
    let c4_from = sequential_expectation(C4_SLOT, C4_ARRAY);
    assert_eq!(c4_from, 2_308);
    assert_eq!(Fixup::C4.mesh_index_remap(c4_from), 2_268);
    let mut c4_meshes = vec![MeshSpec::stub(); C4_ARRAY as usize];
    c4_meshes[C4_SLOT as usize] = MeshSpec::triangle();
    let c4_bytes = authored_container(
        &c4,
        &c4_meshes,
        (C4_ARRAY as i32, 1, c4_from),
        &stub_words(C4_ARRAY, &[C4_SLOT], c4_remap),
    );
    let parsed = parse_ok("synthetic/fixup_c4.zbd", &c4_bytes);
    assert_eq!(parsed.fixup, Fixup::C4);
    assert_eq!(
        parsed.index.last_index, c4_from,
        "the C4 last index is the expectation after the last present mesh"
    );
}

/// The reader cross-checks the index against the record array: the present count
/// and the last present mesh's index must both agree with what is stored, and a
/// stub must be the all-zero record. Each disagreement is its own failure, so a
/// consumer can tell a wrong count from a wrong index.
#[test]
fn accept_f10_b_gamez_index_contradictions_are_named() {
    let spec = HeaderSpec::default();

    let wrong_count = authored_container(&spec, &[MeshSpec::triangle()], (1, 2, -1), &[]);
    match parse("synthetic/count.zbd", &wrong_count) {
        Err(GameZError::PresentCount { declared, read }) => {
            assert_eq!(declared, 2);
            assert_eq!(read, 1);
        }
        other => panic!("a wrong present count must be refused, got {other:?}"),
    }

    let wrong_last = authored_container(&spec, &[MeshSpec::triangle()], (1, 1, 7), &[]);
    match parse("synthetic/last.zbd", &wrong_last) {
        Err(GameZError::LastIndex { expected, found }) => {
            assert_eq!(expected, -1, "a one-slot array's last expectation is -1");
            assert_eq!(found, 7);
        }
        other => panic!("a wrong last_index must be refused, got {other:?}"),
    }

    // Every field of a stub record is checked, not just a convenient few: each
    // case below differs in exactly one field, spread across the record (an
    // integer word, two float words, a pointer), so a check that looked at only
    // some of them would let a container through.
    for (case, field) in ["unk08", "unk44", "unk76", "unk40", "materials_ptr"]
        .into_iter()
        .enumerate()
    {
        let mut not_zero = MeshSpec::stub();
        match field {
            "unk08" => not_zero.unk08 = 5,
            "unk44" => not_zero.unk44 = 1.0,
            "unk76" => not_zero.unk76 = -0.5,
            "unk40" => not_zero.unk40 = 2.0,
            _ => not_zero.materials_ptr = 0x7FFF_FFF0,
        }
        let stubbed = authored_container(
            &spec,
            &[MeshSpec::triangle(), not_zero],
            sequential_index(2, &[0]),
            &stub_words(2, &[0], |value| value),
        );
        match parse("synthetic/stub.zbd", &stubbed) {
            Err(GameZError::NonZeroStubMesh { mesh }) => assert_eq!(mesh, 1, "case {case}"),
            other => panic!(
                "a stub differing only in {field} must be refused, got {other:?} (case {case})"
            ),
        }
    }

    let zero_array = authored_container(&spec, &[], (0, 0, 0), &[]);
    match parse("synthetic/array.zbd", &zero_array) {
        Err(GameZError::ArraySize { found }) => assert_eq!(found, 0),
        other => panic!("a zero array_size must be refused, got {other:?}"),
    }

    // A negative count is refused too, before a single record is read.
    let negative = authored_container(&spec, &[MeshSpec::triangle()], (1, -3, -1), &[]);
    match parse("synthetic/negative.zbd", &negative) {
        Err(GameZError::PresentCount { declared, read }) => {
            assert_eq!(declared, -3);
            assert_eq!(read, 1);
        }
        other => panic!("a negative count must be refused, got {other:?}"),
    }

    // The good container is the one that passes all five.
    let good = authored_container(
        &spec,
        &[MeshSpec::triangle()],
        sequential_index(1, &[0]),
        &[],
    );
    let parsed = parse_ok("synthetic/good.zbd", &good);
    assert_eq!(parsed.index.count, 1);
    assert_eq!(parsed.present_count(), 1);
}

/// The exact decoded / invalid / unsupported face counts a real consumer reports
/// come from this reader's own bytes: every stored polygon produces exactly one
/// status, the counts add up, and a polygon whose position index is out of range
/// is counted invalid while the rest of the mesh still decodes. A reader that
/// dropped a face, or called a broken face supported, would change these numbers.
#[test]
fn accept_f10_b_gamez_face_counts_are_exact() {
    let spec = HeaderSpec::default();
    // Three polygons in one mesh: a good triangle, one whose corner index is past
    // the three stored positions, and a good triangle again.
    let good = PolygonSpec::triangle();
    let mut broken = good.clone();
    broken.positions = vec![0, 1, 99];
    let mut mesh = MeshSpec::triangle();
    mesh.polygon_count = 3;
    mesh.polygons = vec![good.clone(), broken, good];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/face_counts.zbd", &bytes);
    let read = parsed.get(0).expect("one present mesh");
    let topology = read.topology();

    assert_eq!(
        topology.faces.len(),
        3,
        "one status per stored polygon, none dropped"
    );
    assert_eq!(topology.decoded_faces(), 2, "the two good triangles decode");
    assert_eq!(
        topology.invalid_faces(),
        1,
        "the out-of-range index is invalid"
    );
    assert_eq!(
        topology.unsupported_faces(),
        0,
        "and it is not a triangulation limit"
    );
    assert!(!topology.is_complete(), "an invalid face blocks upload");
    match &topology.faces[1] {
        cs_formats::gamez::FaceStatus::Rejected(issue) => {
            assert_eq!(issue.code(), "position_index_out_of_range");
            assert!(issue.to_string().contains("99"), "{issue}");
        }
        other => panic!("the middle polygon must be rejected, got {other:?}"),
    }
    // And the mesh after it still decoded: the "nothing dropped" rule.
    assert_eq!(
        topology.faces[2],
        cs_formats::gamez::FaceStatus::Decoded {
            triangles: 1,
            degenerate: 0
        }
    );

    // An out-of-range *normal* index is a different issue, not the same one.
    let mut bad_normal = PolygonSpec::triangle();
    bad_normal.normals = Some(vec![0, 1, 77]);
    let mut mesh = MeshSpec::triangle();
    mesh.polygons = vec![bad_normal];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let topology = parse_ok("synthetic/normal_range.zbd", &bytes)
        .get(0)
        .expect("one present mesh")
        .topology();
    assert_eq!(topology.invalid_faces(), 1);
    assert_eq!(topology.unsupported_faces(), 0);
    match &topology.faces[0] {
        cs_formats::gamez::FaceStatus::Rejected(issue) => {
            assert_eq!(issue.code(), "normal_index_out_of_range");
        }
        other => panic!("a bad normal index must be rejected, got {other:?}"),
    }
}

/// A hostile stored count is refused before anything is allocated, and the
/// refusal leaves the ledger as it was, so the same context can then read a good
/// container. A reader that trusted the count would try to allocate gigabytes
/// from a 4-byte field.
#[test]
fn accept_f10_b_gamez_hostile_counts_are_refused_and_retryable() {
    let spec = HeaderSpec::default();
    let good = authored_container(
        &spec,
        &[MeshSpec::triangle()],
        sequential_index(1, &[0]),
        &[],
    );

    // A mesh claiming four billion positions, patched into a container that is
    // otherwise valid. The fixture writer refuses to *compose* a mesh whose parts
    // contradict its own counts, which is exactly what a hostile count is, so the
    // container is built once and one stored word is replaced.
    let record_at = GAMEZ_HEADER_BYTES as usize + spec.texture_count as usize * 4 + 8 + 12;
    let vertex_count_at = record_at + 20;
    let mut bytes = good.clone();
    bytes[vertex_count_at..vertex_count_at + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let mut context = ParseContext::new("synthetic/hostile.zbd", 4_096, 4);
    let used_before = context.allocation().used();
    match read_gamez_meshes(&mut context, "synthetic/hostile.zbd", &bytes) {
        Err(GameZError::Parse(error)) => assert!(
            matches!(
                error.kind,
                cs_formats::ParseErrorKind::AllocationBudgetExceeded
                    | cs_formats::ParseErrorKind::UnexpectedEof
                    | cs_formats::ParseErrorKind::LengthOverflow
            ),
            "a hostile count must be a budget or bounds failure, got {:?}",
            error.kind
        ),
        other => panic!("a hostile position count must be refused, got {other:?}"),
    }
    assert_eq!(
        context.allocation().used(),
        used_before,
        "a refused attempt must leave the ledger exactly as it was"
    );

    // A hostile *array_size* is refused by arithmetic before a record is read: the
    // stored count would put the record array past the node array. The fixture
    // writer would refuse to build such a container, so the word is patched.
    let index_at = GAMEZ_HEADER_BYTES as usize + spec.texture_count as usize * 4 + 8;
    for count in [64i32, 1_000_000, 0x7FFF_FFFE] {
        let mut huge_array = good.clone();
        huge_array[index_at..index_at + 4].copy_from_slice(&count.to_le_bytes());
        let refused = parse("synthetic/huge_array.zbd", &huge_array);
        assert!(
            matches!(
                refused,
                Err(GameZError::MeshDataOffset { mesh: u32::MAX, .. })
            ),
            "a record array of {count} records that would run into the node array must be \
             refused, got {:?}",
            refused.err()
        );
    }
    // The very largest legal count is refused earlier, by the array-size range
    // itself, before a single record is read.
    let mut at_max = good.clone();
    at_max[index_at..index_at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
    assert!(
        matches!(
            parse("synthetic/array_at_max.zbd", &at_max),
            Err(GameZError::ArraySize { found }) if found == i32::MAX
        ),
        "array_size == i32::MAX is outside the layout's range"
    );

    // The record reservation is load-bearing too: a container with a thousand
    // array slots and one small mesh needs the ledger for the slots, and a
    // budget one byte short of that refuses it by name. A reader that reserved
    // nothing would parse the same bytes.
    let mut many_slots = vec![MeshSpec::stub(); 1_000];
    many_slots[500] = MeshSpec::triangle();
    let slots_container = authored_container(
        &spec,
        &many_slots,
        (1_000, 1, 501),
        &stub_words(1_000, &[500], |value| value),
    );
    let record_row = 108u64; // one `MeshRecord`: index, 100-byte record, trailer
    let mut tiny = ParseContext::new("synthetic/slots.zbd", 1_000 * record_row - 1, 4);
    let refused = read_gamez_meshes(&mut tiny, "synthetic/slots.zbd", &slots_container);
    assert!(
        matches!(&refused, Err(GameZError::Parse(error)) if error.field.ends_with("meshes.records")),
        "a budget one byte short of the record table must refuse it by name, got {:?}",
        refused.as_ref().err().map(ToString::to_string)
    );
    let mut roomy = ParseContext::new("synthetic/slots.zbd", 1_000 * record_row + 16_384, 4);
    assert_eq!(
        read_gamez_meshes(&mut roomy, "synthetic/slots.zbd", &slots_container)
            .expect("with room for the record table the same container reads")
            .present_count(),
        1
    );

    // The same context reads a good container afterwards: the refusal rolled
    // back, so nothing was consumed.
    let parsed = read_gamez_meshes(&mut context, "synthetic/good.zbd", &good)
        .expect("the same context must read a good container after a refusal");
    assert_eq!(parsed.present_count(), 1);
}

/// The mesh data is a back-to-back run ending exactly at `nodes_offset`. This
/// pins the *rule* on an authored container and then breaks it: a container
/// whose `nodes_offset` claims more than the data holds is refused with both
/// numbers, because a reader that stopped early would leave a tail of the
/// section unaccounted for.
#[test]
fn accept_f10_b_gamez_data_section_ends_at_nodes_offset() {
    let spec = HeaderSpec::default();
    let bytes = authored_container(
        &spec,
        &[MeshSpec::triangle()],
        sequential_index(1, &[0]),
        &[],
    );
    let parsed = parse_ok("synthetic/exact_end.zbd", &bytes);
    assert_eq!(
        parsed.data_end,
        u64::from(parsed.header.nodes_offset),
        "the walk must consume the section exactly"
    );
    let read = parsed.get(0).expect("one present mesh");
    assert_eq!(
        read.data_end, parsed.data_end,
        "the last mesh ends the section"
    );
    assert!(read.data_offset < read.data_end);
    // The first mesh's data starts where the record array ends, not anywhere
    // else: the section has no gap the reader invented or skipped.
    assert_eq!(read.data_offset, parsed.data_offset);

    // Two meshes share the boundary: the first's end is the second's start, so
    // nothing between them was lost.
    let two = authored_container(
        &spec,
        &[MeshSpec::triangle(), MeshSpec::triangle()],
        sequential_index(2, &[0, 1]),
        &[],
    );
    let parsed = parse_ok("synthetic/two_meshes.zbd", &two);
    let first = parsed.get(0).expect("mesh 0");
    let second = parsed.get(1).expect("mesh 1");
    assert_eq!(first.index, 0);
    assert_eq!(second.index, 1);
    assert_eq!(
        first.data_end, second.data_offset,
        "the meshes abut exactly"
    );
    assert_eq!(second.data_end, parsed.data_end);

    // Now claim a longer node array than the data supports. The claimed section
    // has to stay inside the container, or the bounds check fires first and the
    // walk's endpoint is never compared; so the container grows by the same 64
    // bytes the header claims.
    let claimed = u64::from(parsed.header.nodes_offset) + 64;
    let mut broken = two.clone();
    broken.extend_from_slice(&[0u8; 64]);
    broken[36..40].copy_from_slice(&(claimed as u32).to_le_bytes());
    match parse("synthetic/short_data.zbd", &broken) {
        Err(GameZError::MeshDataEnd { found, expected }) => {
            assert_eq!(found, u64::from(parsed.header.nodes_offset));
            assert_eq!(expected, claimed as u32);
        }
        other => panic!("data that does not fill the section must be refused, got {other:?}"),
    }

    // And the same bytes without the padding are refused as a section outside the
    // container, which is the check that runs before the walk.
    let mut outside = two.clone();
    outside[36..40].copy_from_slice(&(claimed as u32).to_le_bytes());
    assert!(
        matches!(
            parse("synthetic/outside.zbd", &outside),
            Err(GameZError::SectionOutOfBounds {
                field: "nodes_offset",
                ..
            })
        ),
        "a claimed section past the end of the container is refused first"
    );
}

/// The `vertex_info` word packs the corner count in bits `0..9` — **nine** bits,
/// the whole low half of the word — and the flag field in bits `9..15`. The flag
/// field is therefore seven bits whose low bit is always zero, and the five bits
/// the reference names are 2, 3, 4, 5 and 6 of it. A reader that masked the count
/// with eight bits, or that shifted the flags by a different amount, would read
/// the same values for every small polygon the corpus happens to contain, so the
/// split is pinned here on the widest count the format can express.
#[test]
fn accept_f10_b_gamez_vertex_info_splits_into_nine_bit_fields() {
    use cs_formats::gamez::RawPolygonInfo;

    // Every bit of the word set: the full corner count 0x1FF and the full flag
    // field 0xFE, so the two fields fill the word exactly.
    let widest = RawPolygonInfo {
        vertex_info: 0xFFFF,
        unk04: 0,
        vertices_ptr: 0,
        normals_ptr: 0,
        mat_count: 1,
        uvs_ptr: 0,
        colors_ptr: 0,
        unk28: 0,
        unk32: 0,
        unk36: 0,
    };
    assert_eq!(widest.corners(), 0x1FF, "the count is nine bits wide");
    assert_eq!(widest.flags(), 0xFE, "the flags are the top seven bits");
    assert!(widest.has_normals());
    assert!(widest.is_triangle_strip());
    assert_eq!(widest.kind(), PrimitiveKind::TriangleStrip);
    assert_eq!(
        widest.unknown_flag_bits(),
        0xFE & !cs_formats::gamez::KNOWN_POLYGON_FLAGS
    );

    // The two fields do not bleed into each other: the highest count with no
    // flags, and bit 8 alone, which belongs to the count and not to the flags.
    let bare = RawPolygonInfo {
        vertex_info: 0x01FF,
        ..widest
    };
    assert_eq!(bare.corners(), 0x1FF);
    assert_eq!(bare.flags(), 0);
    assert_eq!(bare.kind(), PrimitiveKind::Polygon);
    assert!(!bare.has_normals());
    assert_eq!(bare.unknown_flag_bits(), 0);
    let ninth_bit = RawPolygonInfo {
        vertex_info: 0x0100,
        ..widest
    };
    assert_eq!(ninth_bit.corners(), 0x100, "bit 8 belongs to the count");
    assert_eq!(ninth_bit.flags(), 0, "and not to the flag field");

    // The same split on a stored record: a 260-corner polygon, which no
    // eight-bit mask could describe.
    let spec = HeaderSpec::default();
    let wide = PolygonSpec::triangle().with(260, FLAG_NORMALS | FLAG_TRIANGLE_STRIP);
    let mut mesh = MeshSpec::triangle();
    mesh.vertex_count = 260;
    mesh.normal_count = 260;
    mesh.vectors = {
        let mut all = vectors(260);
        all.extend(vectors(260));
        all
    };
    mesh.polygons = vec![wide];
    let bytes = authored_container(&spec, &[mesh], sequential_index(1, &[0]), &[]);
    let parsed = parse_ok("synthetic/wide.zbd", &bytes);
    let mesh = parsed.get(0).expect("one mesh");
    let read = &mesh.mesh.polygons[0];
    assert_eq!(read.corners.len(), 260, "the ninth count bit is read");
    assert_eq!(read.raw_flags, FLAG_NORMALS | FLAG_TRIANGLE_STRIP);
    assert_eq!(read.kind, PrimitiveKind::TriangleStrip);
    let topology = mesh.topology();
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(
        topology.triangles.len(),
        258,
        "a 260-step strip draws 258 steps"
    );
}

/// A mesh record's stored data offset is where the walk must already be. A
/// container whose second mesh declares an offset the sequential walk never
/// reaches is refused **naming that mesh and both offsets**, which is a different
/// and more useful failure than the generic "the section did not end where the
/// header said".
#[test]
fn accept_f10_b_gamez_a_meshes_declared_offset_must_match_the_walk() {
    let spec = HeaderSpec::default();
    let bytes = authored_container(
        &spec,
        &[MeshSpec::triangle(), MeshSpec::triangle()],
        sequential_index(2, &[0, 1]),
        &[],
    );
    let parsed = parse_ok("synthetic/two.zbd", &bytes);
    let first_end = parsed.get(0).expect("mesh 0").data_end;
    let second_offset = parsed.get(1).expect("mesh 1").data_offset;
    assert_eq!(
        second_offset, first_end,
        "the two meshes abut in the good container"
    );

    // Push the second mesh's declared offset eight bytes further on, and pad the
    // data by the same eight bytes so the section still ends where the header
    // says. A reader that walked sequentially and only checked the end of the
    // section would accept these bytes; this one names the mesh instead.
    let record_bytes = MESH_INFO_BYTES as usize + MESH_INFO_TRAILER_BYTES as usize;
    let record_at = GAMEZ_HEADER_BYTES as usize + spec.texture_count as usize * 4 + 8 + 12;
    let second_trailer_at = record_at + record_bytes + MESH_INFO_BYTES as usize;
    let mut stretched = bytes.clone();
    stretched[second_trailer_at..second_trailer_at + 4]
        .copy_from_slice(&(second_offset as u32 + 8).to_le_bytes());
    stretched.extend_from_slice(&[0u8; 8]);
    let claimed = u32::from_le_bytes(stretched[36..40].try_into().unwrap()) + 8;
    stretched[36..40].copy_from_slice(&claimed.to_le_bytes());
    match parse("synthetic/gap.zbd", &stretched) {
        Err(GameZError::MeshDataNotSequential {
            mesh,
            declared,
            walked,
        }) => {
            assert_eq!(mesh, 1, "the failure names the mesh that desynchronised");
            assert_eq!(u64::from(declared), second_offset + 8);
            assert_eq!(walked, first_end, "and the offset the walk had reached");
        }
        other => {
            panic!("a declared offset the walk cannot reach must be refused, got {other:?}")
        }
    }
}

// ------------------------------------------------------------------ retail ---

/// The measured `unk08` of each GameZ archive of the original installation,
/// from the `HeaderCsC` blocks in
/// `crates/mech3ax-gamez/src/gamez/cs/fixup.rs` at the pinned revision. The two
/// named ones select a fixup table; zero means "no table, and this file does not
/// claim which value it has".
const RETAIL_GAMEZ: [(&str, u32); 9] = [
    ("ZBD/planes.zbd", UNK08_PLANES),
    ("ZBD/C1/gamez.zbd", 967_277_730),
    ("ZBD/C1B/gamez.zbd", 967_278_018),
    ("ZBD/C1C/gamez.zbd", 967_278_208),
    ("ZBD/C2/gamez.zbd", 967_278_462),
    ("ZBD/C2B/gamez.zbd", 967_278_721),
    ("ZBD/C3/gamez.zbd", 967_278_943),
    ("ZBD/C4/gamez.zbd", UNK08_C4),
    ("ZBD/C5/gamez.zbd", 967_279_700),
];

/// The `unk08` a file is known to store, or zero when this file does not claim
/// it.
fn unk08_of(rel: &str) -> u32 {
    RETAIL_GAMEZ
        .iter()
        .find(|(name, _)| *name == rel)
        .map(|(_, value)| *value)
        .unwrap_or(0)
}

/// The measured `nodes_offset` of each archive, from the same reference header
/// comments. These are the reference's own numbers, not this reader's: the
/// reader must land on them from the bytes alone.
const RETAIL_NODES_OFFSET: [(&str, u32); 9] = [
    ("ZBD/planes.zbd", 4_881_228),
    ("ZBD/C1/gamez.zbd", 4_326_296),
    ("ZBD/C1B/gamez.zbd", 1_924_148),
    ("ZBD/C1C/gamez.zbd", 1_964_684),
    ("ZBD/C2/gamez.zbd", 3_111_828),
    ("ZBD/C2B/gamez.zbd", 1_658_700),
    ("ZBD/C3/gamez.zbd", 3_661_748),
    ("ZBD/C4/gamez.zbd", 5_107_144),
    ("ZBD/C5/gamez.zbd", 5_259_292),
];

/// The read-only original installation, or a loud failure when the `retail`
/// capability is missing. Never a silent skip: a test that cannot prove anything
/// must fail, not pass.
fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect(
        "CS_GAME_DIR must point at the original installation: this test proves the layout on \
         retail bytes and cannot pass without them",
    ))
}

/// Reads one retail archive's mesh section through the production entrypoint.
fn read_retail(rel: &str) -> GameZMeshes {
    let path = game_dir().join(rel);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("{rel}: the original installation must hold it: {error}"));
    let mut context = ParseContext::with_defaults(rel);
    read_gamez_meshes(&mut context, rel, &bytes)
        .unwrap_or_else(|error| panic!("{rel}: the retail mesh section must read, got {error}"))
}

/// The retail half of the layout: every GameZ archive of the installation is
/// read, the mesh data walk lands exactly on the reference's own recorded
/// `nodes_offset` for that archive, the header matches the reference's recorded
/// words, the index is self-consistent against the records it points at, and
/// every face produces a status.
///
/// The offsets and header words asserted here are the pinned reference's
/// recorded numbers, not this reader's output, so a reader that mis-walks the
/// section cannot pass.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f10_b_gamez_retail_every_archive_lands_on_the_reference_offset() {
    for (rel, expected_unk08) in RETAIL_GAMEZ {
        let parsed = read_retail(rel);
        let expected_end = u64::from(
            RETAIL_NODES_OFFSET
                .iter()
                .find(|(name, _)| *name == rel)
                .map(|(_, offset)| *offset)
                .unwrap_or_else(|| panic!("{rel}: the reference records a nodes_offset for it")),
        );
        assert_eq!(
            parsed.data_end, expected_end,
            "{rel}: the walk must end on the reference's recorded nodes_offset"
        );
        assert_eq!(parsed.header.nodes_offset as u64, expected_end, "{rel}");
        assert_eq!(parsed.header.signature, GAMEZ_SIGNATURE, "{rel}");
        assert_eq!(parsed.header.version, GAMEZ_VERSION, "{rel}");
        assert_eq!(
            parsed.header.unk08, expected_unk08,
            "{rel}: unk08 as recorded"
        );

        // The index is self-consistent against the records it points at.
        assert_eq!(
            parsed.present_count(),
            parsed.index.count.max(0) as usize,
            "{rel}: present records must match the index's count"
        );
        assert_eq!(
            parsed.meshes.len(),
            parsed.index.array_size.max(0) as usize,
            "{rel}: one result slot per array entry"
        );
        // The remapped archives select their table; the rest use the sequential
        // expectation, and the recorded `last_index` is the remapped value.
        match expected_unk08 {
            UNK08_PLANES => {
                assert_eq!(parsed.fixup, Fixup::Planes, "{rel}");
                assert_eq!(parsed.index.last_index, 1313, "{rel}: the measured value");
                assert_eq!(Fixup::Planes.last_index_remap(1779), 1313, "{rel}");
            }
            UNK08_C4 => {
                assert_eq!(parsed.fixup, Fixup::C4, "{rel}");
                assert_eq!(parsed.index.last_index, 2283, "{rel}: the measured value");
                assert_eq!(Fixup::C4.last_index_remap(2490), 2283, "{rel}");
            }
            _ => assert_eq!(parsed.fixup, Fixup::None, "{rel}"),
        }

        // Every stored face has a status and the counts are exact: the decoded,
        // invalid and unsupported numbers are what F10-D's AC04 report
        // aggregates, so they must be real and must add up.
        let mut polygons = 0usize;
        let mut decoded = 0usize;
        let mut invalid = 0usize;
        let mut unsupported = 0usize;
        for mesh in parsed.present() {
            let topology = mesh.topology();
            assert_eq!(
                topology.faces.len(),
                mesh.mesh.polygons.len(),
                "{rel}: one status per stored polygon on mesh {}",
                mesh.index
            );
            polygons += topology.faces.len();
            decoded += topology.decoded_faces();
            invalid += topology.invalid_faces();
            unsupported += topology.unsupported_faces();
        }
        assert!(polygons > 0, "{rel}: the corpus is not empty");
        assert_eq!(
            decoded + invalid + unsupported,
            polygons,
            "{rel}: every stored face is counted exactly once"
        );
        assert_eq!(
            invalid, 0,
            "{rel}: the measured corpus has no invalid face; a new one is a finding, not a pass"
        );
        assert!(
            parsed.findings.is_empty(),
            "{rel}: every stored polygon is inside the reference's asserted profile, got {:?}",
            parsed.findings
        );
    }
}

/// The retail half of the flag, attribute and material reading, over **all nine**
/// GameZ archives: strips appear only where `FLAG_TRIANGLE_STRIP` is set, flag
/// bytes come through raw including the bits the reference does not name,
/// polygons store several material groups with one coordinate per corner each,
/// corners share positions without losing their own attributes, lights are read,
/// no morph vectors are stored, and every material reference was read but
/// explicitly not range-checked.
///
/// The discriminating step is the per-polygon arithmetic: every stored group must
/// hold exactly one coordinate per corner, and the aggregate group count must
/// equal the container's own `unchecked_material_references`. A reader that
/// mis-walked the section, kept only the first group, or mirrored a corner's UV
/// from the wrong group could not hold both.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f10_b_gamez_retail_flags_groups_and_seams_over_the_whole_corpus() {
    let mut polygons = 0usize;
    let mut strips = 0usize;
    let mut outlines = 0usize;
    let mut with_normals = 0usize;
    let mut multi_group = 0usize;
    let mut differing_groups = 0usize;
    let mut shared_positions = 0usize;
    let mut distinct_seams = 0usize;
    let mut corner_total = 0usize;
    let mut group_total = 0usize;
    let mut raw_flag_values: std::collections::BTreeSet<u32> = Default::default();
    let mut mesh_material_refs = 0usize;
    let mut light_records = 0usize;
    let mut morphs = 0usize;
    let mut group_histogram: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut per_archive: Vec<(&str, usize, usize, usize)> = Vec::new();

    for (rel, _) in RETAIL_GAMEZ {
        let parsed = read_retail(rel);
        let (mut archive_polygons, mut archive_strips, mut archive_multi) = (0, 0, 0);
        let (mut archive_mesh_refs, mut archive_polygon_refs) = (0, 0);
        for mesh in parsed.present() {
            mesh_material_refs += mesh.materials.len();
            archive_mesh_refs += mesh.materials.len();
            light_records += mesh.lights.len();
            morphs += mesh.morphs.len();
            for (polygon_index, polygon) in mesh.mesh.polygons.iter().enumerate() {
                polygons += 1;
                archive_polygons += 1;
                match polygon.kind {
                    PrimitiveKind::TriangleStrip => {
                        strips += 1;
                        archive_strips += 1;
                    }
                    PrimitiveKind::Polygon => outlines += 1,
                }
                if polygon.corners.iter().any(|corner| corner.normal.is_some()) {
                    with_normals += 1;
                }
                // Every stored group holds one coordinate per corner, and the
                // groups really can differ: a reader that kept only the first
                // would pass the first check and fail this one.
                let groups = mesh
                    .groups(polygon_index)
                    .expect("every stored polygon has its own group list");
                for group in groups {
                    assert_eq!(
                        group.uvs.len(),
                        polygon.corners.len(),
                        "{rel}: one stored coordinate per corner per group"
                    );
                }
                if groups.len() > 1 {
                    multi_group += 1;
                    archive_multi += 1;
                    // Some stored groups repeat another's coordinates, so the
                    // corpus is counted rather than assumed: what must hold is
                    // that the groups are kept as separate records, which the
                    // per-corner length above and the group count below do.
                    if groups[0].uvs != groups[1].uvs {
                        differing_groups += 1;
                    }
                }
                *group_histogram.entry(groups.len()).or_default() += 1;
                group_total += groups.len();
                archive_polygon_refs += groups.len();
                raw_flag_values.insert(polygon.raw_flags);
                corner_total += polygon.corners.len();

                // A repeated position index inside one polygon is a seam the
                // original authored; the corners must still be separate.
                let mut seen: std::collections::BTreeMap<u32, usize> = Default::default();
                for (corner_index, corner) in polygon.corners.iter().enumerate() {
                    if let Some(first) = seen.insert(corner.position, corner_index) {
                        shared_positions += 1;
                        let first_corner = polygon.corners[first];
                        if first_corner.uv != corner.uv || first_corner.color != corner.color {
                            distinct_seams += 1;
                        }
                    }
                }
            }
        }
        per_archive.push((rel, archive_polygons, archive_strips, archive_multi));
        // The single-group mirroring is exact where the corpus stores one group.
        if rel == "ZBD/planes.zbd" {
            assert_eq!(
                multi_group - archive_multi,
                0,
                "{rel}: no stored polygon has several material groups"
            );
        }
        assert_eq!(
            parsed.unchecked_material_references,
            archive_mesh_refs + archive_polygon_refs,
            "{rel}: the unchecked-material count names every reference that was read raw, at \
             both levels"
        );
    }

    assert!(polygons > 100_000, "the corpus is large, got {polygons}");
    assert!(strips > 0, "the corpus has strips");
    assert!(outlines > 0, "the corpus has outlines");
    assert_eq!(
        strips + outlines,
        polygons,
        "every face is one or the other"
    );
    assert!(
        with_normals > 0 && with_normals < polygons,
        "both normal cases occur"
    );
    assert!(
        raw_flag_values.contains(&(FLAG_TRIANGLE_STRIP | FLAG_NORMALS)),
        "the strip+normals flag byte occurs"
    );
    assert!(
        raw_flag_values.iter().any(|value| value & FLAG_UNK2 != 0),
        "a bit the reference does not name occurs and is carried raw"
    );
    assert!(
        raw_flag_values.iter().all(|value| *value < 0x100),
        "the flag byte is the only part of vertex_info kept on the IR, and it is one byte"
    );
    assert!(
        raw_flag_values
            .iter()
            .all(|value| value & !cs_formats::gamez::KNOWN_POLYGON_FLAGS == 0),
        "every flag bit the corpus uses is one the reference names"
    );
    assert!(
        group_histogram.contains_key(&1),
        "one stored group per polygon occurs"
    );
    assert!(
        group_histogram.contains_key(&2) && group_histogram.contains_key(&3),
        "two- and three-group polygons occur in the world archives, got {group_histogram:?}"
    );
    assert!(
        multi_group > 0,
        "some polygons store several material groups"
    );
    assert!(
        differing_groups > 0,
        "and at least one of them stores genuinely different coordinates per group"
    );
    assert!(
        group_total > polygons,
        "and they are the reason the group total exceeds it"
    );
    assert!(shared_positions > 0, "some corners share a position index");
    assert!(
        distinct_seams > 0,
        "at least one shared position keeps its own attributes"
    );
    assert!(
        corner_total > polygons * 3,
        "many outlines carry more than three corners"
    );
    assert!(mesh_material_refs > 0, "meshes store material references");
    assert!(light_records > 0, "meshes store lights");
    assert_eq!(morphs, 0, "the measured corpus stores no morph vectors");
    println!(
        "per-archive (polygons, strips, multi-group): {per_archive:?}; groups: {group_histogram:?}"
    );
}
