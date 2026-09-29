//! Acceptance task F10-C.02: the CS GameZ texture-name table and material
//! records, and how a material names its texture
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, section
//! `### F10-C`).
//!
//! The layout this file encodes comes from the pinned mech3ax v0.6.0 revision
//! (commit `d3521a9721be731d365504568ddcd78e3f9846bb`), read from
//! `crates/mech3ax-gamez/src/textures/ng.rs`,
//! `crates/mech3ax-gamez/src/materials/{mod,read_multi,read_single}.rs` and
//! `crates/mech3ax-gamez/src/gamez/cs/mod.rs`. The worksheet, the per-archive
//! measurements and the recorded unknowns are in
//! `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`.
//!
//! Every synthetic fixture here is authored from that worksheet: no original game
//! data is read, nothing is derived from it, and the fixture writer shares no
//! code with the reader. The expected values are literals, so a reader and a
//! writer that made the same mistake cannot agree. The writer composes each
//! section from its own parts and **refuses** a container whose parts contradict
//! its own stored offsets, so a fixture cannot accidentally parse.
//!
//! The `#[ignore = "requires CS_GAME_DIR"]` test at the end is the retail half:
//! it reads the read-only installation, fails loudly when it is missing, and
//! proves the layout on all nine real GameZ archives.

use cs_formats::ParseContext;
use cs_formats::gamez::materials::{
    MATERIAL_FLAG_ALWAYS, MATERIAL_FLAG_CYCLED, MATERIAL_FLAG_FREE, MATERIAL_FLAG_TEXTURED,
    MATERIAL_FLAG_UNKNOWN, NG_MATERIAL_SLOTS,
};
use cs_formats::gamez::{
    CYCLE_FRAME_BYTES, CYCLE_HEADER_BYTES, GAMEZ_HEADER_BYTES, GameZMaterialError, GameZMaterials,
    MATERIAL_RECORD_BYTES, MATERIAL_SLOT_BYTES, MaterialFinding, MaterialKind, TEXTURE_INFO_BYTES,
    TextureNameEncoding, read_gamez_materials,
};
use cs_formats::zbd::{GAMEZ_SIGNATURE, GAMEZ_VERSION};
use cs_types::evidence::ClaimStatus;

// --------------------------------------------------------------- fixtures ---

/// The 20 bytes a name field stores, written the way the layout stores them.
///
/// The three shapes below are the three the measured corpus contains; nothing
/// else is a legal encoding, and `decode_name` has to tell them apart.
#[derive(Clone)]
struct NameField([u8; 20]);

impl NameField {
    /// `stem`, then the `.` of the extension stored as the NUL that ends the
    /// stem, then the extension, then a second NUL and zero padding.
    fn with_suffix(stem: &str, suffix: &str) -> Self {
        let mut out = [0u8; 20];
        let stem = stem.as_bytes();
        let suffix = suffix.as_bytes();
        assert!(
            stem.len() + 1 + suffix.len() < 20,
            "the field holds 20 bytes"
        );
        out[..stem.len()].copy_from_slice(stem);
        out[stem.len()] = 0;
        out[stem.len() + 1..stem.len() + 1 + suffix.len()].copy_from_slice(suffix);
        out[stem.len() + 1 + suffix.len()] = 0;
        Self(out)
    }

    /// `stem` and nothing after its terminator.
    fn stem_only(stem: &str) -> Self {
        Self::with_suffix(stem, "")
    }

    /// A completely full field: one NUL, where the `.` of the extension was, and
    /// no second terminator. The measured corpus has 95 of these across its nine
    /// GameZ archives, 5 of them in `planes.zbd` alone.
    fn full(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), 20, "a full field is exactly 20 bytes");
        let mut out = [0u8; 20];
        out.copy_from_slice(bytes);
        Self(out)
    }

    /// The same, with one byte replaced, for the refusal variants.
    fn with(self, at: usize, byte: u8) -> Self {
        let mut out = self.0;
        out[at] = byte;
        Self(out)
    }
}

/// One stored texture-name record.
#[derive(Clone)]
struct NameSpec {
    name: NameField,
    field00: u32,
    field32: u32,
    field36: u32,
    field40: i32,
}

impl NameSpec {
    /// A record inside the reference's asserted profile: null pointer, in use,
    /// index zero, `unk40` of -1.
    fn used(name: NameField) -> Self {
        Self {
            name,
            field00: 0,
            field32: 2,
            field36: 0,
            field40: -1,
        }
    }

    /// A record whose pointer is non-null and whose state is the other value the
    /// reference allows.
    fn processing(name: NameField, pointer: u32) -> Self {
        Self {
            name,
            field00: pointer,
            field32: 1,
            field36: 0,
            field40: -1,
        }
    }

    fn record_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // `TextureInfoNgC`: the name is the **third** field, at offset 12, and
        // `used`, `index` and `unk40` follow it. Writing the words in any other
        // order would produce a record the reader must refuse.
        for word in [self.field00, 0, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&self.name.0);
        for word in [self.field32, self.field36] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&self.field40.to_le_bytes());
        assert_eq!(out.len() as u64, TEXTURE_INFO_BYTES, "six words and a name");
        out
    }
}

/// The ten words of one material record, every one of them settable, so two
/// fixtures can differ in every field.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MaterialSpec {
    alpha: u8,
    flags: u8,
    rgb: u16,
    color: [f32; 3],
    texture_index: u32,
    field20: f32,
    field24: f32,
    field28: f32,
    field32: f32,
    cycle_ptr: u32,
}

impl MaterialSpec {
    /// A textured material naming `texture`, inside the reference's asserted
    /// profile. `field32` is the word the reference calls `specular` and newer
    /// classification calls soil; a fixture sets it to a value nothing here
    /// interprets, so a reader that read it as anything would be caught.
    fn textured(texture: u32, field32: f32) -> Self {
        Self {
            alpha: 0xFF,
            flags: MATERIAL_FLAG_TEXTURED | MATERIAL_FLAG_ALWAYS,
            rgb: 0x7FFF,
            color: [255.0, 255.0, 255.0],
            texture_index: texture,
            field20: 0.0,
            field24: 0.5,
            field28: 0.5,
            field32,
            cycle_ptr: 0,
        }
    }

    /// An untextured material: a flat colour that names nothing.
    fn colored(color: [f32; 3], field32: f32) -> Self {
        Self {
            alpha: 0x10,
            flags: MATERIAL_FLAG_ALWAYS,
            rgb: 0,
            color,
            texture_index: 0,
            field20: 0.0,
            field24: 0.5,
            field28: 0.5,
            field32,
            cycle_ptr: 0,
        }
    }

    /// The same material with the cycled flag and a non-null cycle pointer.
    fn cycled(mut self, pointer: u32) -> Self {
        self.flags |= MATERIAL_FLAG_CYCLED;
        self.cycle_ptr = pointer;
        self
    }

    fn record_bytes(&self) -> Vec<u8> {
        let mut out = vec![self.alpha, self.flags];
        out.extend_from_slice(&self.rgb.to_le_bytes());
        for value in self.color {
            out.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        for word in [
            self.texture_index,
            self.field20.to_bits(),
            self.field24.to_bits(),
            self.field28.to_bits(),
            self.field32.to_bits(),
            self.cycle_ptr,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(
            out.len() as u64,
            MATERIAL_RECORD_BYTES,
            "a material record is ten words"
        );
        out
    }
}

/// The all-zero record the reference asserts for every slot past `count`: only
/// the free flag set, every other byte zero.
fn zero_record_bytes() -> Vec<u8> {
    let mut out = vec![0u8; MATERIAL_RECORD_BYTES as usize];
    out[1] = MATERIAL_FLAG_FREE;
    out
}

/// One stored cycle record: the 28-byte header plus its frame indices.
#[derive(Clone)]
struct CycleSpec {
    field00: u32,
    field04: u32,
    field08: u32,
    field12: f32,
    count1: u32,
    count2: u32,
    data_ptr: u32,
    frames: Vec<u32>,
}

impl CycleSpec {
    /// A cycle inside the reference's asserted profile.
    fn of(frames: Vec<u32>) -> Self {
        let count = frames.len() as u32;
        Self {
            field00: 1,
            field04: 7,
            field08: 0,
            field12: 4.0,
            count1: count,
            count2: count,
            data_ptr: 0x00AB_CDEF,
            frames,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [self.field00, self.field04, self.field08] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&self.field12.to_bits().to_le_bytes());
        for word in [self.count1, self.count2, self.data_ptr] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(
            out.len() as u64,
            CYCLE_HEADER_BYTES,
            "a cycle header is seven words"
        );
        for frame in &self.frames {
            out.extend_from_slice(&frame.to_le_bytes());
        }
        assert_eq!(
            out.len() as u64,
            CYCLE_HEADER_BYTES + CYCLE_FRAME_BYTES * self.frames.len() as u64,
            "then one word per frame"
        );
        out
    }
}

/// The material section a fixture stores, described in the layout's own terms.
#[derive(Clone, Default)]
struct MaterialSection {
    /// The four header words; `None` is what the layout requires.
    array_size: Option<i32>,
    count: Option<i32>,
    index_max: Option<i32>,
    index_last: Option<i32>,
    /// The present materials, in stored order.
    materials: Vec<MaterialSpec>,
    /// The two link words of every present slot, in stored order. `None` is what
    /// the layout requires.
    links: Option<Vec<(i16, i16)>>,
    /// One zero slot to store as something other than the all-zero record, for
    /// the finding test. `(slot index, record)`.
    dirty_zero_slot: Option<(u32, MaterialSpec)>,
    /// The cycle data, stored after the array in material order.
    cycles: Vec<CycleSpec>,
}

impl MaterialSection {
    fn of(materials: Vec<MaterialSpec>) -> Self {
        Self {
            materials,
            ..Self::default()
        }
    }

    /// The four words, with the layout's own constraints filled in.
    fn header_words(&self) -> [i32; 4] {
        let count = self.materials.len() as i32;
        [
            self.array_size.unwrap_or(count),
            self.count.unwrap_or(count),
            self.index_max.unwrap_or(count),
            self.index_last.unwrap_or(count - 1),
        ]
    }

    /// The two link words a present slot at `index` stores.
    fn present_links(&self, index: u32, count: u32) -> (i16, i16) {
        if let Some(links) = &self.links {
            return links[index as usize];
        }
        (
            if index + 1 >= count {
                -1
            } else {
                (index + 1) as i16
            },
            if index == 0 { -1 } else { (index - 1) as i16 },
        )
    }

    /// The two link words a zero slot at `index` stores. The pair runs the other
    /// way round from a present slot's, which is the reference's own reading.
    fn zero_links(&self, index: u32, count: u32) -> (i16, i16) {
        (
            if index == count {
                -1
            } else {
                (index - 1) as i16
            },
            if index + 1 >= NG_MATERIAL_SLOTS {
                -1
            } else {
                (index + 1) as i16
            },
        )
    }

    fn bytes(&self) -> Vec<u8> {
        let count = self.materials.len() as u32;
        let mut out = Vec::new();
        for word in self.header_words() {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for (index, material) in self.materials.iter().enumerate() {
            out.extend_from_slice(&material.record_bytes());
            let (link1, link2) = self.present_links(index as u32, count);
            out.extend_from_slice(&link1.to_le_bytes());
            out.extend_from_slice(&link2.to_le_bytes());
        }
        for index in count..NG_MATERIAL_SLOTS {
            let record = match &self.dirty_zero_slot {
                Some((slot, spec)) if *slot == index => spec.record_bytes(),
                _ => zero_record_bytes(),
            };
            out.extend_from_slice(&record);
            let (link1, link2) = self.zero_links(index, count);
            out.extend_from_slice(&link1.to_le_bytes());
            out.extend_from_slice(&link2.to_le_bytes());
        }
        for cycle in &self.cycles {
            out.extend_from_slice(&cycle.bytes());
        }
        assert_eq!(
            out.len() as u64,
            16 + MATERIAL_SLOT_BYTES * u64::from(NG_MATERIAL_SLOTS)
                + self
                    .cycles
                    .iter()
                    .map(|cycle| {
                        CYCLE_HEADER_BYTES + CYCLE_FRAME_BYTES * cycle.frames.len() as u64
                    })
                    .sum::<u64>(),
            "a material section is a header, 1000 slots and the cycle data"
        );
        out
    }
}

/// Builds one whole container: header, texture table, material section, then the
/// smallest mesh section that satisfies the mesh reader (one all-zero stub).
///
/// The offsets are computed here from the sections' own lengths, so no fixture
/// depends on a hand-counted byte total and a wrong count cannot hide.
fn container(textures: &[NameSpec], section: &MaterialSection) -> Vec<u8> {
    let table: Vec<u8> = textures.iter().flat_map(NameSpec::record_bytes).collect();
    let materials = section.bytes();
    let textures_offset = GAMEZ_HEADER_BYTES as usize;
    let materials_offset = textures_offset + table.len();
    let meshes_offset = materials_offset + materials.len();
    // One stub mesh: the index is (1, 0, -1) and the record is the all-zero
    // record followed by the expected index of the next present mesh, which is
    // -1 at the end of a one-slot array.
    let record_array = 100 + 4;
    let nodes_offset = meshes_offset + 12 + record_array;

    let mut out = Vec::new();
    for word in [
        GAMEZ_SIGNATURE,
        GAMEZ_VERSION,
        1_234_567_890,
        textures.len() as u32,
        textures_offset as u32,
        materials_offset as u32,
        meshes_offset as u32,
        7,
        0,
        nodes_offset as u32,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    assert_eq!(
        out.len(),
        textures_offset,
        "the header is ten 4-byte fields"
    );

    out.extend_from_slice(&table);
    assert_eq!(
        out.len(),
        materials_offset,
        "the texture table then the materials"
    );
    out.extend_from_slice(&materials);
    assert_eq!(
        out.len(),
        meshes_offset,
        "the material section then the mesh index"
    );

    for word in [1i32, 0, -1] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&[0u8; 100]);
    out.extend_from_slice(&(-1i32).to_le_bytes());
    assert_eq!(out.len(), nodes_offset, "one stub mesh, no data");
    out
}

/// The three names every fixture starts from, one of each encoding shape.
fn three_names() -> Vec<NameSpec> {
    vec![
        NameSpec::used(NameField::with_suffix("Sky1", "tif")),
        NameSpec::used(NameField::stem_only("lightmap")),
        NameSpec::used(NameField::full(b"horizonindicator\x00tif")),
    ]
}

fn parse(bytes: &[u8]) -> Result<GameZMaterials, GameZMaterialError> {
    let mut context = ParseContext::with_defaults("fixture");
    read_gamez_materials(&mut context, "fixture", bytes)
}

fn parse_ok(bytes: &[u8]) -> GameZMaterials {
    match parse(bytes) {
        Ok(materials) => materials,
        Err(error) => panic!("the authored fixture must parse, got {error}"),
    }
}

// ------------------------------------------------------------------ tests ---

/// A material record stores an **index** into the container's texture table, and
/// the three name encodings the layout has decode to the stored names. Nothing
/// is folded or trimmed: `Sky1.tif` stays `Sky1.tif`.
#[test]
fn accept_f10_c_02_a_material_names_its_texture_by_table_index() {
    let bytes = container(
        &three_names(),
        &MaterialSection::of(vec![
            MaterialSpec::textured(0, 0.25),
            MaterialSpec::textured(1, 0.5),
            MaterialSpec::textured(2, 0.75),
            MaterialSpec::colored([1.0, 0.0, 0.0], 0.125),
        ]),
    );
    let materials = parse_ok(&bytes);

    assert_eq!(materials.textures.len(), 3);
    let names: Vec<&str> = materials.textures.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Sky1.tif", "lightmap", "horizonindicator.tif"]);
    assert_eq!(materials.textures[0].stem, "Sky1");
    assert_eq!(materials.textures[0].suffix.as_deref(), Some("tif"));
    assert_eq!(
        materials.textures[0].encoding,
        TextureNameEncoding::WithSuffix
    );
    assert_eq!(materials.textures[1].suffix, None);
    assert_eq!(
        materials.textures[1].encoding,
        TextureNameEncoding::StemOnly
    );
    // A full field restores the `.` and takes all twenty bytes.
    assert_eq!(
        materials.textures[2].encoding,
        TextureNameEncoding::Unterminated
    );
    assert_eq!(materials.textures[2].stem, "horizonindicator");
    assert_eq!(materials.textures[2].suffix.as_deref(), Some("tif"));

    // The binding: each material's stored index, nothing else, picks the name.
    for (index, expected) in [
        (0u32, "Sky1.tif"),
        (1, "lightmap"),
        (2, "horizonindicator.tif"),
    ] {
        let material = materials.material(index).expect("material is in range");
        assert_eq!(material.kind(), MaterialKind::Textured);
        assert_eq!(
            materials
                .texture_of(material)
                .expect("a textured material names a texture")
                .name,
            expected
        );
    }

    // The untextured one names nothing at all, which is not a failure.
    let coloured = materials.material(3).expect("material is in range");
    assert_eq!(coloured.kind(), MaterialKind::Colored);
    assert!(materials.texture_of(coloured).is_none());
    assert!(
        !materials.names_a_texture(coloured),
        "an untextured material names nothing, which is not a failure"
    );
    assert_eq!(coloured.record.texture_index, 0);
    assert_eq!(coloured.record.color, [1.0, 0.0, 0.0]);
}

/// The ten words of a material record and the 44 bytes of a texture record
/// survive in their own slots. Two fixtures that differ in every field are read
/// back field by field, so a reader that transposed two words, or that dropped
/// the word the reference calls `specular` and newer classification calls soil,
/// fails here.
#[test]
fn accept_f10_c_02_record_words_survive_in_their_own_slots() {
    let mut first = MaterialSpec::textured(1, 0.0625);
    first.alpha = 0x01;
    first.flags = MATERIAL_FLAG_TEXTURED | MATERIAL_FLAG_ALWAYS | MATERIAL_FLAG_UNKNOWN;
    first.rgb = 0x1234;
    first.color = [1.5, 2.5, 3.5];
    first.field20 = 4.5;
    first.field24 = 5.5;
    first.field28 = 6.5;
    first.field32 = 7.5;
    let mut second = MaterialSpec::textured(2, 8.5);
    second.alpha = 0x02;
    second.flags = MATERIAL_FLAG_TEXTURED | MATERIAL_FLAG_ALWAYS;
    second.rgb = 0x5678;
    second.color = [9.5, 8.5, 7.5];
    second.field20 = 6.5;
    second.field24 = 4.5;
    second.field28 = 2.5;
    second.field32 = 1.5;
    let bytes = container(
        &[
            NameSpec::used(NameField::with_suffix("a", "tif")),
            NameSpec::processing(NameField::with_suffix("b", "TIF"), 0xDEAD_BEEF),
            NameSpec::used(NameField::stem_only("c")),
        ],
        &MaterialSection::of(vec![first, second]),
    );
    let materials = parse_ok(&bytes);

    let read = |index: u32| -> MaterialSpec {
        let record = materials.material(index).expect("material").record;
        MaterialSpec {
            alpha: record.alpha,
            flags: record.flags,
            rgb: record.rgb,
            color: record.color,
            texture_index: record.texture_index,
            field20: record.field20,
            field24: record.field24,
            field28: record.field28,
            field32: record.field32,
            cycle_ptr: record.cycle_ptr,
        }
    };
    assert_eq!(read(0), first, "material 0 keeps every word");
    assert_eq!(read(1), second, "material 1 keeps every word");
    // The two records differ in every field, so a transposition is visible.
    assert_ne!(read(0), read(1));

    // The texture records too: both words the reference does not interpret, the
    // `used` word, the index and the -1 all survive.
    let texture = &materials.textures[0];
    assert_eq!(texture.field00, 0);
    assert_eq!(texture.field32, 2);
    assert_eq!(texture.field36, 0);
    assert_eq!(texture.field40, -1);
    assert_eq!(texture.name, "a.tif");
    let texture = &materials.textures[1];
    assert_eq!(texture.field00, 0xDEAD_BEEF);
    assert_eq!(texture.field32, 1);
    assert_eq!(
        texture.name, "b.TIF",
        "the extension's case is stored, not folded"
    );
    // The two records deliberately leave the reference's asserted profile, so the
    // findings name exactly those fields — the assertion list, in the order the
    // reader checks them, with the unmapped flag bit on the first record.
    let flagged: Vec<String> = materials
        .findings
        .iter()
        .map(|finding| format!("{}:{}", finding.material(), finding.code_field()))
        .collect();
    assert_eq!(
        flagged,
        vec![
            "0:field20",
            "0:alpha",
            "0:rgb",
            "0:color.r",
            "0:color.g",
            "0:color.b",
            "0:field24",
            "0:field28",
            "1:field20",
            "1:alpha",
            "1:rgb",
            "1:color.r",
            "1:color.g",
            "1:color.b",
            "1:field24",
            "1:field28",
        ],
        "only the fields the fixture left outside the profile are reported"
    );
}

/// The array is exactly 1000 slots however few are present, the zero region's
/// records are the all-zero record, and the walk ends on `meshes_offset`. A
/// reader that sized the array from `array_size`, or that skipped the zero
/// region, would end somewhere else and be refused with both numbers.
#[test]
fn accept_f10_c_02_the_array_is_a_thousand_slots_and_the_walk_ends_on_meshes_offset() {
    let bytes = container(
        &three_names(),
        &MaterialSection::of(vec![MaterialSpec::textured(0, 0.0)]),
    );
    let materials = parse_ok(&bytes);

    assert_eq!(materials.info.array_size, 1);
    assert_eq!(materials.info.count, 1);
    assert_eq!(materials.info.index_max, 1);
    assert_eq!(materials.info.index_last, 0);
    assert_eq!(materials.count(), 1);
    assert_eq!(
        materials.free_slots,
        NG_MATERIAL_SLOTS - 1,
        "the other 999 slots are read, not skipped"
    );
    assert_eq!(
        materials.data_end,
        u64::from(materials.header.meshes_offset)
    );
    assert_eq!(
        materials.data_end,
        materials.materials_offset + 16 + MATERIAL_SLOT_BYTES * 1000,
        "a header and a thousand slots, exactly"
    );
    assert_eq!(materials.findings, Vec::new());

    // One more present material does **not** move the boundary: the array is
    // always a thousand slots, so a present material is a zero slot becoming a
    // present one. A reader that sized the array from `count` would end earlier.
    let two = container(
        &three_names(),
        &MaterialSection::of(vec![
            MaterialSpec::textured(0, 0.0),
            MaterialSpec::textured(1, 0.0),
        ]),
    );
    let two = parse_ok(&two);
    assert_eq!(two.free_slots, NG_MATERIAL_SLOTS - 2);
    assert_eq!(
        two.data_end, materials.data_end,
        "a thousand slots is a thousand slots, present or not"
    );
}

/// The cycle data is stored after the whole array, in material order, one record
/// per cycled material, and a container that stores one still lands on
/// `meshes_offset`. A reader that read the cycle data per material instead of
/// after the array, or that used `count1` where the record stores `count2`, would
/// miss the boundary.
#[test]
fn accept_f10_c_02_cycle_data_is_stored_after_the_whole_array() {
    let section = MaterialSection {
        materials: vec![
            MaterialSpec::textured(0, 0.0).cycled(0x1111_1111),
            MaterialSpec::textured(1, 0.0),
            MaterialSpec::textured(2, 0.0).cycled(0x2222_2222),
        ],
        cycles: vec![CycleSpec::of(vec![0, 1, 2]), CycleSpec::of(vec![2])],
        ..MaterialSection::default()
    };
    let bytes = container(&three_names(), &section);
    let materials = parse_ok(&bytes);

    assert_eq!(
        materials.data_end,
        u64::from(materials.header.meshes_offset)
    );
    let cycle = materials
        .material(0)
        .expect("material 0")
        .cycle
        .as_ref()
        .expect("cycled");
    assert_eq!(cycle.textures, [0, 1, 2]);
    assert_eq!(cycle.count1, 3);
    assert_eq!(cycle.count2, 3);
    assert_eq!(cycle.field00, 1);
    assert_eq!(cycle.field04, 7);
    assert_eq!(cycle.field08, 0);
    assert_eq!(cycle.field12, 4.0);
    assert_eq!(cycle.data_ptr, 0x00AB_CDEF);
    // The uncycled material stores no cycle record at all, and the reader does
    // not invent one.
    assert!(materials.material(1).expect("material 1").cycle.is_none());
    assert_eq!(
        materials
            .material(2)
            .expect("material 2")
            .cycle
            .as_ref()
            .expect("cycled")
            .textures,
        [2]
    );
    // The two words either side of the record's own ten are in their own slots:
    // a cycled material's cycle pointer keeps the value the fixture gave it.
    assert_eq!(
        materials.material(0).expect("material 0").record.cycle_ptr,
        0x1111_1111
    );
    assert_eq!(
        materials.material(2).expect("material 2").record.cycle_ptr,
        0x2222_2222
    );
    assert_eq!(
        materials.material(1).expect("material 1").record.cycle_ptr,
        0
    );
    assert_eq!(materials.findings, Vec::new());
}

/// The present slots' link words run forwards then backwards; the zero slots'
/// run backwards then forwards, and `-1` terminates each run. Both rules are the
/// reference's own, and a reader that used one rule for both halves of the array
/// would produce link findings on the whole zero region.
#[test]
fn accept_f10_c_02_link_words_run_in_opposite_directions_in_the_two_halves() {
    let count = 3u32;
    let links: Vec<(i16, i16)> = (0..count)
        .map(|index| {
            (
                if index + 1 >= count {
                    -1
                } else {
                    (index + 1) as i16
                },
                if index == 0 { -1 } else { (index - 1) as i16 },
            )
        })
        .collect();
    assert_eq!(links, [(1, -1), (2, 0), (-1, 1)]);
    let section = MaterialSection {
        materials: (0..count).map(|_| MaterialSpec::textured(0, 0.0)).collect(),
        links: Some(links),
        ..MaterialSection::default()
    };
    let materials = parse_ok(&container(&three_names(), &section));
    assert_eq!(
        materials.findings,
        Vec::new(),
        "the expected pairs are no finding"
    );
    assert_eq!(
        (
            materials.material(0).expect("0").link1,
            materials.material(0).expect("0").link2
        ),
        (1, -1)
    );
    assert_eq!(
        (
            materials.material(2).expect("2").link1,
            materials.material(2).expect("2").link2
        ),
        (-1, 1)
    );
    // The zero region starts at `count`, so its first link word is -1 and its
    // second is `count + 1`; that is the same pair the present slots would use,
    // swapped.
    assert_eq!(materials.free_slots, NG_MATERIAL_SLOTS - count);
}

/// A stored value outside an assertion the reference makes is **reported**, not
/// refused and not silently accepted: the record still reaches the caller with
/// its raw words, the finding names the exact slot and field, and the section
/// still ends on `meshes_offset`.
#[test]
fn accept_f10_c_02_out_of_profile_values_are_findings_not_failures() {
    // A textured material whose `alpha`, `rgb`, colour and half-fields are not
    // the values the reference asserts, with an unmapped flag bit and both link
    // words wrong.
    let mut odd = MaterialSpec::textured(0, 0.5);
    odd.alpha = 0x00;
    odd.flags |= 0x40; // a bit the reference's MaterialFlags does not name
    odd.rgb = 0x0000;
    odd.color = [0.5, 0.5, 0.5];
    odd.field24 = 0.25;
    odd.field28 = 0.125;
    let section = MaterialSection {
        materials: vec![odd],
        links: Some(vec![(7, 9)]),
        ..MaterialSection::default()
    };
    let materials = parse_ok(&container(&three_names(), &section));

    // The record is still there, with its own raw words.
    let record = materials.material(0).expect("the record is read").record;
    assert_eq!(record.alpha, 0x00);
    assert_eq!(record.rgb, 0x0000);
    assert_eq!(record.color, [0.5, 0.5, 0.5]);
    assert_eq!(record.field24, 0.25);
    assert_eq!(
        materials.data_end,
        u64::from(materials.header.meshes_offset)
    );

    let codes: Vec<&str> = materials
        .findings
        .iter()
        .map(MaterialFinding::code)
        .collect();
    for expected in ["unknown_material_flags", "material_field", "material_link"] {
        assert!(codes.contains(&expected), "{expected} in {codes:?}");
    }
    assert!(
        materials.findings.iter().any(|finding| matches!(
            finding,
            MaterialFinding::MaterialField { field: "alpha", .. }
        )),
        "the alpha finding names its field: {codes:?}"
    );
    assert!(
        materials.findings.iter().any(|finding| matches!(
            finding,
            MaterialFinding::MaterialLink {
                field: "link1",
                found: 7,
                expected: -1,
                ..
            }
        )),
        "the link finding names both numbers: {codes:?}"
    );
    assert!(
        materials
            .findings
            .iter()
            .all(|finding| finding.material() == 0),
        "every finding names the slot it is about"
    );
    assert_eq!(
        materials.findings[0].evidence(),
        ClaimStatus::ObservedTool,
        "a finding is ObservedTool at best"
    );

    // A zero slot that is not the all-zero record is a finding too, and it does
    // not stop the walk: the zero region's length is fixed either way.
    let section = MaterialSection {
        materials: vec![MaterialSpec::textured(0, 0.0)],
        dirty_zero_slot: Some((5, odd)),
        ..MaterialSection::default()
    };
    let dirty = parse_ok(&container(&three_names(), &section));
    assert!(
        dirty
            .findings
            .iter()
            .any(|finding| matches!(finding, MaterialFinding::MaterialField { material: 5, .. }))
    );
    assert_eq!(dirty.data_end, u64::from(dirty.header.meshes_offset));
}

/// A name the container stores more than once stays visible, with every table
/// index, instead of being renamed or silently collapsed. The pinned reference
/// renames repeats on **write**; that is an extractor convention and this reader
/// does not reproduce it.
#[test]
fn accept_f10_c_02_duplicate_container_names_are_reported_with_every_index() {
    let repeated = NameField::with_suffix("bldhwk_cowling.", "tif");
    let bytes = container(
        &[
            NameSpec::used(NameField::with_suffix("a", "tif")),
            NameSpec::used(repeated.clone()),
            NameSpec::used(repeated.clone()),
            NameSpec::used(repeated),
        ],
        &MaterialSection::of(vec![
            MaterialSpec::textured(1, 0.0),
            MaterialSpec::textured(3, 0.0),
        ]),
    );
    let materials = parse_ok(&bytes);
    assert_eq!(
        materials.duplicate_names(),
        vec![("bldhwk_cowling..tif".to_owned(), vec![1, 2, 3])],
        "the name is stored as the field holds it, dot and all"
    );
    // Both materials still resolve to the same stored name, and neither is
    // rewritten to a numbered variant.
    for index in [0u32, 1] {
        let name = materials
            .texture_of(materials.material(index).expect("material"))
            .expect("a texture")
            .name
            .clone();
        assert_eq!(name, "bldhwk_cowling..tif");
    }
}

/// Every refusal the layout's own constraints imply, each with its own variant
/// and its own numbers: a count the section header contradicts, a name field with
/// no NUL, one with a high bit, one with junk after its terminator, and a
/// container whose sections do not tile.
#[test]
fn accept_f10_c_02_contradictions_are_named_with_both_numbers() {
    // The section header: `index_max` and `index_last` are cross-checked
    // against `count`, and the failure names both numbers.
    for (field, wrong, expected) in [("index_max", 9i32, 1), ("index_last", 9, 0)] {
        let mut section = MaterialSection::of(vec![MaterialSpec::textured(0, 0.0)]);
        match field {
            "index_max" => section.index_max = Some(wrong),
            _ => section.index_last = Some(wrong),
        }
        let bytes = container(&three_names(), &section);
        let error = parse(&bytes).expect_err("the header contradicts itself");
        assert!(
            matches!(
                &error,
                GameZMaterialError::MaterialIndex { field: got, found, expected: want }
                if *got == field && *found == wrong && *want == expected
            ),
            "{field}: {error}"
        );
    }
    // `count` above `array_size`.
    let mut section = MaterialSection::of(vec![MaterialSpec::textured(0, 0.0)]);
    section.array_size = Some(0);
    let error = parse(&container(&three_names(), &section)).expect_err("count exceeds array_size");
    assert!(
        matches!(
            error,
            GameZMaterialError::MaterialCount {
                field: "count",
                found: 1
            }
        ),
        "{error}"
    );

    // A name field with no NUL at all.
    let name = NameField([b'a'; 20]);
    let bytes = container(
        &[NameSpec::used(name)],
        &MaterialSection::of(vec![MaterialSpec::textured(0, 0.0)]),
    );
    let error = parse(&bytes).expect_err("a name field with no terminator");
    assert!(
        matches!(
            error,
            GameZMaterialError::TextureNameUnterminated { texture: 0 }
        ),
        "{error}"
    );

    // A name field with a byte outside ASCII.
    let name = NameField::stem_only("x").with(1, 0xFF);
    let bytes = container(
        &[NameSpec::used(name)],
        &MaterialSection::of(vec![MaterialSpec::textured(0, 0.0)]),
    );
    let error = parse(&bytes).expect_err("a name field outside ASCII");
    assert!(
        matches!(
            error,
            GameZMaterialError::TextureNameNotAscii {
                texture: 0,
                at: 1,
                found: 0xFF
            }
        ),
        "{error}"
    );

    // A name field with junk where the padding must be.
    let name = NameField::stem_only("x").with(3, 0x41);
    let bytes = container(
        &[NameSpec::used(name)],
        &MaterialSection::of(vec![MaterialSpec::textured(0, 0.0)]),
    );
    let error = parse(&bytes).expect_err("a name field with junk padding");
    assert!(
        matches!(
            error,
            GameZMaterialError::TextureNamePadding {
                texture: 0,
                at: 3,
                found: 0x41
            }
        ),
        "{error}"
    );

    // `texture_count` at the reference's own bound is refused, because the
    // table's byte extent is then not established.
    let mut bytes = container(&three_names(), &MaterialSection::default());
    bytes[12..16].copy_from_slice(&4096u32.to_le_bytes());
    let error = parse(&bytes).expect_err("texture_count is not below 4096");
    assert!(
        matches!(error, GameZMaterialError::TextureCount { found: 4096 }),
        "{error}"
    );

    // A container that is not a CS GameZ container at all is refused by the
    // shared header check, with the mesh reader's own variant.
    let error = parse(b"not a gamez container at all, really").expect_err("not a container");
    assert_eq!(error.code(), "header");
}

/// The section boundary is the correctness check, and it fails loudly: a material
/// index that names a table the container does not have, a cycle record whose
/// frame count disagrees with its own header, and a container that is one byte
/// short all end with the offset the walk reached and the offset the header
/// declares.
#[test]
fn accept_f10_c_02_the_section_boundary_fails_loudly_and_is_retryable() {
    // A material naming a texture the container's table does not have: the
    // record is read and reported, and the section still ends where it should.
    let section = MaterialSection::of(vec![MaterialSpec::textured(9, 0.0)]);
    let materials = parse_ok(&container(&three_names(), &section));
    assert_eq!(materials.count(), 1);
    assert_eq!(materials.material(0).expect("read").record.texture_index, 9);
    assert!(
        materials.findings.iter().any(|finding| matches!(
            finding,
            MaterialFinding::TextureIndexOutOfRange {
                material: 0,
                index: 9,
                available: 3
            }
        )),
        "{:?}",
        materials.findings
    );
    assert!(
        materials
            .texture_of(materials.material(0).expect("read"))
            .is_none()
    );

    // A container truncated anywhere at all is refused, loudly, and names the
    // container. Truncating the tail also pulls `nodes_offset` past the end, so
    // the shared header check is what catches the shortest cuts: the failure is
    // still a failure, and nothing is parsed from a partial section.
    let bytes = container(
        &three_names(),
        &MaterialSection::of(vec![MaterialSpec::textured(0, 0.0)]),
    );
    for cut in [
        bytes.len() - 1,
        bytes.len() - 100,
        materials_offset(&bytes) + 4,
        materials_offset(&bytes) + 1000,
    ] {
        let error = parse(&bytes[..cut]).expect_err("a truncated section");
        assert!(
            matches!(
                error.code(),
                "parse" | "header" | "material_section_end" | "texture_section_end"
            ),
            "cut {cut}: {error}"
        );
        assert!(
            !error.to_string().is_empty(),
            "cut {cut}: the failure says why"
        );
        assert!(
            error.container() == "fixture" || error.offset().is_some(),
            "cut {cut}: the failure names the container or the offset: {error}"
        );
    }

    // A failed attempt leaves the ledger as it was, so the same context can read
    // the good bytes honestly afterwards.
    let mut context = ParseContext::with_defaults("fixture");
    let short = &bytes[..bytes.len() - 1];
    assert!(read_gamez_materials(&mut context, "fixture", short).is_err());
    let after =
        read_gamez_materials(&mut context, "fixture", &bytes).expect("the same context retries");
    assert_eq!(after.count(), 1);
}

fn materials_offset(bytes: &[u8]) -> usize {
    u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize
}

/// The retail half: all nine GameZ archives of the original installation. The
/// layout is proved by the material section ending **exactly** on the mesh index
/// the header declares, by every stored material index naming a table the
/// container has, and by the recorded counts.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f10_c_02_retail_every_archive_lands_on_its_meshes_offset() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the installation");
    let archives = [
        ("ZBD/planes.zbd", 298, 954),
        ("ZBD/C1/gamez.zbd", 565, 570),
        ("ZBD/C1B/gamez.zbd", 359, 320),
        ("ZBD/C1C/gamez.zbd", 279, 284),
        ("ZBD/C2/gamez.zbd", 498, 492),
        ("ZBD/C2B/gamez.zbd", 285, 274),
        ("ZBD/C3/gamez.zbd", 465, 483),
        ("ZBD/C4/gamez.zbd", 654, 685),
        ("ZBD/C5/gamez.zbd", 582, 607),
    ];
    let mut total_materials = 0usize;
    let mut total_textures = 0usize;
    for (relative, textures, materials) in archives {
        let textures = textures as u64;
        let materials = materials as u32;
        let path = std::path::Path::new(&game_dir).join(relative);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{relative}: the installation must hold it: {error}"));
        let mut context = ParseContext::with_defaults(relative);
        let parsed = read_gamez_materials(&mut context, relative, &bytes)
            .unwrap_or_else(|error| panic!("{relative}: the material section must read: {error}"));

        // The discriminating check: the walk ends on the mesh index, not near it.
        assert_eq!(
            parsed.data_end,
            u64::from(parsed.header.meshes_offset),
            "{relative}: the material section must end on meshes_offset"
        );
        assert_eq!(
            parsed.materials_offset,
            u64::from(parsed.header.materials_offset)
        );
        // The texture table's records fill the space before it exactly.
        assert_eq!(
            parsed.materials_offset,
            parsed.textures_offset + textures * TEXTURE_INFO_BYTES,
            "{relative}: textures_offset + 44 * texture_count == materials_offset"
        );
        assert_eq!(parsed.textures.len(), textures as usize, "{relative}");
        assert_eq!(parsed.count(), materials, "{relative}");
        assert_eq!(
            parsed.free_slots,
            NG_MATERIAL_SLOTS - materials,
            "{relative}"
        );
        assert_eq!(parsed.info.count as u32, parsed.count(), "{relative}");
        // Every stored material index is inside the container's own table, and
        // every present record is inside the reference's asserted profile.
        assert!(
            parsed.findings.is_empty(),
            "{relative}: {:?}",
            parsed.findings
        );
        for material in &parsed.materials {
            assert!(
                material.record.texture_index < parsed.textures.len() as u32,
                "{relative}: material {} names texture {}",
                material.index,
                material.record.texture_index
            );
        }
        // Every name decodes, and no decoding invented a name the field does not
        // hold: the stored stem is always a prefix of the stored name.
        for texture in &parsed.textures {
            assert!(
                !texture.name.is_empty(),
                "{relative}: texture {}",
                texture.index
            );
            assert!(
                texture.name.starts_with(&texture.stem),
                "{relative}: texture {} `{}` does not start with its stem `{}`",
                texture.index,
                texture.name,
                texture.stem
            );
        }
        assert_eq!(
            parsed.layout_evidence(),
            ClaimStatus::ObservedTool,
            "documented and measured, never original-verified"
        );
        total_materials += parsed.count() as usize;
        total_textures += parsed.textures.len();
    }
    assert_eq!(
        total_textures, 3985,
        "the measured corpus's texture-name entries"
    );
    assert_eq!(
        total_materials, 4669,
        "the measured corpus's present materials"
    );
}
