//! The CS GameZ texture-name table and material records, and how a material
//! names its texture (`specs/F10-gamez-mesh-topology-and-material-records.md`,
//! stage `### F10-C`, slice F10-C.02; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! # Provenance
//!
//! The layout below is **established**, not guessed. It was read from the pinned
//! legacy CS-capable reference, mech3ax **v0.6.0**, commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
//! `docs/research/SOURCES.md`), and checked byte-for-byte against every GameZ
//! archive of the original installation. The field worksheet with source file
//! and line references, the per-archive measurements and the remaining unknowns
//! are in `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`.
//!
//! | What | Source at that commit |
//! | --- | --- |
//! | 40-byte container header, the section chain, the order the sections are read in | `crates/mech3ax-gamez/src/gamez/cs/mod.rs` (`HeaderCsC`, `read_gamez`) |
//! | 44-byte texture-name record, the NUL-suffixed name encoding | `crates/mech3ax-gamez/src/textures/ng.rs` (`TextureInfoNgC`, `read_texture_infos`) |
//! | 16-byte material header, 40-byte material record, 28-byte cycle record, the five material flag bits | `crates/mech3ax-gamez/src/materials/mod.rs` (`MaterialInfoC`, `MaterialC`, `CycleInfoC`, `MaterialFlags`, `MatType::Ng`) |
//! | the two-pass material walk: present slots, then the zero slots, then the cycle data | `crates/mech3ax-gamez/src/materials/read_multi.rs` (`read_materials`, `assert_material_info`, `read_materials_zero`) |
//! | one material record, one zero slot, one cycle record, and the `material.index` to texture binding | `crates/mech3ax-gamez/src/materials/read_single.rs` (`read_material`, `read_material_zero`, `read_cycle`) |
//! | the C-suffix string decoding | `crates/mech3ax-common/src/string/mod.rs` (`str_from_c_suffix`, `from_ascii`) |
//!
//! No code was copied: mech3ax is EUPL-1.2 and is read as a reference only.
//!
//! # What this reads
//!
//! [`read_gamez_materials`] reads the two sections between the header and the
//! mesh index, `textures_offset..meshes_offset`, and checks that it stopped
//! exactly on [`GameZHeader::meshes_offset`]. That equality is the correctness
//! check: the material section's length depends on the stored `count` **and** on
//! which records set the cycled flag, so a reader that got either wrong, or that
//! mis-sized a record, would end somewhere else.
//!
//! It is deliberately separate from [`read_gamez_meshes`](super::read_gamez_meshes),
//! which reads the mesh section only and keeps material indices as raw
//! references. A caller that wants both reads the container once per entrypoint,
//! because each of the two readers proves its own section boundary on its own.
//!
//! # How a material names its texture
//!
//! A material record does **not** store a name. It stores a *number* at offset
//! 16 — the reference calls it `index`, and its own field comment says "ptr in
//! mechlib, texture index in gamez" — which is an index into the container's
//! texture-name table. [`GameZMaterials::texture_of`] performs exactly that
//! lookup, and [`GameZTextureName::name`] is that entry's stored name, decoded.
//!
//! Nothing here normalizes, folds, trims or re-spells that name. A name is
//! reported exactly as the table stores it, once the one encoding rule the layout
//! establishes — the stored `NUL` where the `.` of the extension was — has been
//! undone. Which archive a name is looked up in, and whether the lookup
//! succeeds, is the dependency audit's business, not this reader's.
//!
//! # What is kept raw
//!
//! Spec F10 non-negotiable #5: a field an older API calls `specular` is **not**
//! read as specularity. [`RawMaterialRecord::field32`] is that field. The pinned
//! reference names it `specular`; newer classification of the same field calls it
//! soil, and nothing read here decides between them, so it is carried as a raw
//! `f32` whose bits are untouched and whose meaning is `Unknown`.
//!
//! Every other `field*` below is raw for the same reason: the reference either
//! names it without explaining it, or asserts a range without saying what it
//! does. A stored value outside an assertion the reference makes is reported as a
//! [`MaterialFinding`] and the record is still read — such a value changes no
//! stored length, so refusing it would throw away a dependency the audit needs
//! to see.

use std::fmt;
use std::mem::size_of;

use cs_types::evidence::ClaimStatus;

use super::reader::GameZHeader;
use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader};

/// Error scope stamped onto failures raised while reading the texture-name
/// table and the material records.
pub const MATERIALS_ENTRYPOINT: &str = "gamez.materials";

/// Bytes of one texture-name record: `TextureInfoNgC`.
pub const TEXTURE_INFO_BYTES: u64 = 44;
/// Bytes of the name field inside one texture-name record: `Ascii<20>`.
pub const TEXTURE_NAME_BYTES: u64 = 20;
/// Bytes of the material section header: `MaterialInfoC`.
pub const MATERIAL_HEADER_BYTES: u64 = 16;
/// Bytes of one material record: `MaterialC`.
pub const MATERIAL_RECORD_BYTES: u64 = 40;
/// Bytes of the two link words that follow every material record: two `i16`.
pub const MATERIAL_LINK_BYTES: u64 = 4;
/// Bytes of one material slot: the record plus its two link words.
pub const MATERIAL_SLOT_BYTES: u64 = MATERIAL_RECORD_BYTES + MATERIAL_LINK_BYTES;
/// Bytes of one cycle record header: `CycleInfoC`.
pub const CYCLE_HEADER_BYTES: u64 = 28;
/// Bytes of one frame of cycle data: one texture index.
pub const CYCLE_FRAME_BYTES: u64 = 4;

/// Slots in the material array of a CS GameZ container: `MatType::Ng`.
///
/// The reference asserts `0 <= array_size <= 1000` but always *reads* and
/// *writes* exactly this many slots, so the array's byte length does not depend
/// on the stored `array_size`. The measured corpus stores 274–954 present
/// materials, so the zero region is never empty and is usually the larger half.
pub const NG_MATERIAL_SLOTS: u32 = 1000;

/// Flag bit 0 of a material record: the colour information comes from a texture,
/// and the material names one.
pub const MATERIAL_FLAG_TEXTURED: u8 = 1 << 0;
/// Flag bit 1: the reference calls this `UNKNOWN` and hands it on as
/// `TexturedMaterial::flag`. What it does is not established.
pub const MATERIAL_FLAG_UNKNOWN: u8 = 1 << 1;
/// Flag bit 2: the material has cycled texture frames, stored after the whole
/// material array. It is the one flag bit that changes a stored length.
pub const MATERIAL_FLAG_CYCLED: u8 = 1 << 2;
/// Flag bit 4: the reference asserts this is set for every present material in a
/// CS container and clear for every Recoil one.
pub const MATERIAL_FLAG_ALWAYS: u8 = 1 << 4;
/// Flag bit 5: the slot is free. The reference asserts the whole record is the
/// all-zero record in that case, with this bit the only one set.
pub const MATERIAL_FLAG_FREE: u8 = 1 << 5;
/// Every flag bit the reference's `MaterialFlags` names.
pub const KNOWN_MATERIAL_FLAGS: u8 = MATERIAL_FLAG_TEXTURED
    | MATERIAL_FLAG_UNKNOWN
    | MATERIAL_FLAG_CYCLED
    | MATERIAL_FLAG_ALWAYS
    | MATERIAL_FLAG_FREE;

/// The `used` word the reference names for a texture whose `field00` is null.
const TEXTURE_STATE_USED: u32 = 2;

/// Upper bound the reference puts on `texture_count`.
const MAX_TEXTURE_COUNT: u32 = 4096;

/// How one stored texture name was encoded in its 20-byte field.
///
/// The layout stores a name and its extension as two NUL-separated runs, with
/// the `.` of the extension replaced by the NUL that ends the first run. Undoing
/// that is the only decoding this reader does; what follows the name is reported
/// exactly as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureNameEncoding {
    /// The field held a name and then a NUL with nothing after it: the stored
    /// name is the first run, and the reference's rule is "no suffix".
    StemOnly,
    /// The field held a name, a NUL, a non-empty extension and another NUL. The
    /// stored name is `first + "." + extension`; an extension that itself begins
    /// with `.` is kept, because that is what the field holds.
    WithSuffix,
    /// The field was completely full: one NUL and no second one, so the name ran
    /// into the last byte. The reference restores the `.` and takes all 20 bytes.
    /// The measured corpus stores 95 such names across its nine archives, five of
    /// them in `planes.zbd` (`horizonindicator.tif`), so this is an ordinary case
    /// and not a defect.
    Unterminated,
}

impl TextureNameEncoding {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::StemOnly => "stem_only",
            Self::WithSuffix => "with_suffix",
            Self::Unterminated => "unterminated",
        }
    }
}

/// One entry of the container's texture-name table: `TextureInfoNgC`.
///
/// `name` is the decoded name; `stem` and `suffix` are the two stored runs it
/// came from. A consumer that needs the bytes as stored has `encoding` and can
/// re-derive them; a consumer that needs to look the texture up uses `name` and
/// must not alter it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameZTextureName {
    /// Index of this entry in the container's texture table. A material record's
    /// `texture_index` is a value of this space.
    pub index: u32,
    /// The stored name, decoded. Never case-folded, trimmed or re-spelled.
    pub name: String,
    /// The run of bytes before the stored NUL.
    pub stem: String,
    /// The run of bytes after it, when the field stored one.
    pub suffix: Option<String>,
    /// How the field was encoded.
    pub encoding: TextureNameEncoding,
    /// `unk00`: the reference's `Ptr`. Null when [`Self::field32`] is
    /// [`TEXTURE_STATE_USED`], non-null otherwise, and never followed.
    pub field00: u32,
    /// `used`: `2` when the texture is in use, `1` when it is being processed.
    /// Not interpreted here beyond the reference's own two-value assertion.
    pub field32: u32,
    /// `index`: the reference asserts it is zero.
    pub field36: u32,
    /// `unk40`: the reference asserts it is `-1`.
    pub field40: i32,
}

/// What a material record is, as far as the layout establishes.
///
/// The distinction is the reference's own: a record with the textured flag takes
/// all of its colour from a texture it names, a record without it is a flat
/// colour. Neither says what the renderer does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialKind {
    /// The textured flag is set: the material names a texture at
    /// [`RawMaterialRecord::texture_index`].
    Textured,
    /// The textured flag is clear: the material is a flat colour and names no
    /// texture.
    Colored,
}

impl MaterialKind {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Textured => "textured",
            Self::Colored => "colored",
        }
    }
}

/// The 40 bytes of one material record, field by field: `MaterialC`.
///
/// Each word is in its own field, exactly as stored, so a reader that
/// transposed two of them would be caught by a test that reads them back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawMaterialRecord {
    /// `alpha`: the reference asserts `0xFF` for a textured record and `0x00`
    /// for a zero slot.
    pub alpha: u8,
    /// `flags`: the five bits [`KNOWN_MATERIAL_FLAGS`] names, kept whole so a bit
    /// the reference does not name cannot be lost.
    pub flags: u8,
    /// `rgb`: the reference asserts `0x7FFF` for a textured record and `0x0000`
    /// for a zero slot or an untextured record.
    pub rgb: u16,
    /// `color`: three `f32` the reference asserts are `255.0` each for a textured
    /// record. Raw: the reference does not say this is a display colour or how
    /// it is consumed.
    pub color: [f32; 3],
    /// `index`: the **texture index** in a CS container ("ptr in mechlib,
    /// texture index in gamez"). The reference asserts zero for an untextured
    /// record and for a zero slot.
    pub texture_index: u32,
    /// `zero20`: the reference asserts `0.0`.
    pub field20: f32,
    /// `half24`: the reference asserts `0.5` for a present record and `0.0` for a
    /// zero slot.
    pub field24: f32,
    /// `half28`: the reference asserts `0.5` for a present record and `0.0` for a
    /// zero slot.
    pub field28: f32,
    /// The `f32` the pinned reference calls `specular` and that newer
    /// classification calls soil. **Kept raw and uninterpreted** (spec F10
    /// non-negotiable #5): its meaning is `Unknown`, nothing here reads it as
    /// specularity or as soil, and its bits are untouched.
    pub field32: f32,
    /// `cycle_ptr`: the reference's pointer to this material's cycle record,
    /// asserted zero unless the cycled flag is set. Raw and never followed.
    pub cycle_ptr: u32,
}

impl RawMaterialRecord {
    /// The record names a texture at [`Self::texture_index`].
    pub const fn kind(&self) -> MaterialKind {
        if self.flags & MATERIAL_FLAG_TEXTURED != 0 {
            MaterialKind::Textured
        } else {
            MaterialKind::Colored
        }
    }

    /// Flag bits outside the five the reference's `MaterialFlags` names.
    pub const fn unknown_flag_bits(&self) -> u8 {
        self.flags & !KNOWN_MATERIAL_FLAGS
    }

    /// The cycled flag is set, so cycle data is stored for this material after
    /// the material array.
    pub const fn is_cycled(&self) -> bool {
        self.flags & MATERIAL_FLAG_CYCLED != 0
    }
}

/// One present material record: its 40 stored bytes, its two link words and, for
/// a cycled material, the frame data stored after the material array.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMaterial {
    /// Array index of this material in the container's material table. This is
    /// the value a mesh's material reference and a polygon's material group
    /// carry.
    pub index: u32,
    /// The 40 stored bytes.
    pub record: RawMaterialRecord,
    /// The first of the two `i16` link words stored after the record. Its
    /// expected value is not the same rule the zero slots use; see
    /// [`GameZMaterials`].
    pub link1: i16,
    /// The second link word, raw.
    pub link2: i16,
    /// The cycled frame data, when the cycled flag is set. Raw: the reference
    /// names none of its fields.
    pub cycle: Option<RawCycle>,
}

impl RawMaterial {
    /// What the record is, as far as its flags establish.
    pub const fn kind(&self) -> MaterialKind {
        self.record.kind()
    }
}

/// One stored cycle record plus its frame list: `CycleInfoC` and its `count1`
/// texture indices.
///
/// The reference names none of these fields beyond their constraints, and the
/// constraint on `field12` differs per game ("in MW: 2.0..=16.0, in CS:
/// 0.0..=16.0"), so all of it is raw here. A frame's value is an index into the
/// container's texture-name table, exactly like a material's `texture_index`.
#[derive(Debug, Clone, PartialEq)]
pub struct RawCycle {
    /// `unk00`: the reference asserts it is non-zero.
    pub field00: u32,
    /// `unk04`: not interpreted.
    pub field04: u32,
    /// `unk08`: the reference asserts it is zero.
    pub field08: u32,
    /// `unk12`: the reference asserts `0.0 <= unk12 <= 16.0` for CS.
    pub field12: f32,
    /// `count1`: frames stored, and asserted equal to `count2`.
    pub count1: u32,
    /// `count2`: the reference asserts it equals `count1`.
    pub count2: u32,
    /// `data_ptr`: the reference asserts it is non-zero. Raw, never followed.
    pub data_ptr: u32,
    /// The stored frame texture indices, in stored order.
    pub textures: Vec<u32>,
}

/// The four `i32` at the start of the material section: `MaterialInfoC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialInfo {
    /// `array_size`: the reference asserts `0 <= array_size <= 1000`. It is not
    /// the number of slots the section stores, which is always
    /// [`NG_MATERIAL_SLOTS`].
    pub array_size: i32,
    /// `count`: present material records. `index_max` and `index_last` are
    /// cross-checked against it, and it is the count a material index is
    /// range-checked against.
    pub count: i32,
    /// `index_max`: the reference asserts it equals [`Self::count`].
    pub index_max: i32,
    /// `index_last`: the reference asserts it equals `count - 1`.
    pub index_last: i32,
}

/// Something the material section stores that the reference's assertions do not
/// allow.
///
/// A finding is **not** an error: none of these values changes a stored length,
/// so the record is read, its material and its texture reach the dependency
/// audit, and the finding says the archive is outside the reference's asserted
/// profile. The measured corpus produces no findings at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialFinding {
    /// A texture-name record has a field whose value the reference asserts.
    TextureRecordField {
        /// Index in the texture-name table.
        texture: u32,
        /// `"field04"`, `"field08"`, `"field32"`, `"field36"` or `"field40"`.
        field: &'static str,
        /// The value stored.
        found: u32,
    },
    /// A material record has flag bits the reference's `MaterialFlags` does not
    /// name, so the reference would refuse the container here.
    UnknownMaterialFlags {
        /// Index in the material table.
        material: u32,
        /// The unmapped flag bits.
        bits: u8,
    },
    /// A material record has a field whose value the reference asserts.
    MaterialField {
        /// Index in the material table.
        material: u32,
        /// The field, named as [`RawMaterialRecord`] names it, or `"always"` /
        /// `"free"` for the two flag bits the reference asserts on their own.
        field: &'static str,
        /// The value stored, as it is stored: an `f32` field is its bit pattern.
        found: u64,
    },
    /// A material slot's two link words are not the pair the reference expects
    /// for its position.
    MaterialLink {
        /// Index in the material table.
        material: u32,
        /// `"link1"` or `"link2"`.
        field: &'static str,
        /// The value stored.
        found: i16,
        /// The value the reference expects.
        expected: i16,
    },
    /// The cycled flag and `cycle_ptr` disagree: one is set and the other is not.
    /// The reference asserts the pointer is non-zero exactly when the flag is
    /// set, and a disagreement moves every byte after it, so this is the one
    /// finding that can move the section boundary — the walk then misses
    /// `meshes_offset` and the read fails rather than reporting a partial table.
    CyclePointerMismatch {
        /// Index in the material table.
        material: u32,
        /// The stored pointer.
        cycle_ptr: u32,
    },
    /// A cycle record has a field whose value the reference asserts.
    CycleField {
        /// Index of the material the cycle belongs to.
        material: u32,
        /// `"field00"`, `"field08"`, `"field12"`, `"count1"`, `"count2"` or
        /// `"data_ptr"`.
        field: &'static str,
        /// The value stored, as it is stored: an `f32` field is its bit pattern.
        found: u64,
    },
    /// A texture index stored in a material or in a cycle frame is outside the
    /// container's texture-name table. The reference asserts it is inside; the
    /// material is still read and still reported, with no texture name.
    TextureIndexOutOfRange {
        /// Index of the material (or cycle owner) the reference came through.
        material: u32,
        /// The stored index.
        index: u32,
        /// Entries in the texture-name table.
        available: u32,
    },
}

impl MaterialFinding {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TextureRecordField { .. } => "texture_record_field",
            Self::UnknownMaterialFlags { .. } => "unknown_material_flags",
            Self::MaterialField { .. } => "material_field",
            Self::MaterialLink { .. } => "material_link",
            Self::CyclePointerMismatch { .. } => "cycle_pointer_mismatch",
            Self::CycleField { .. } => "cycle_field",
            Self::TextureIndexOutOfRange { .. } => "texture_index_out_of_range",
        }
    }

    /// Material index the finding is about, or [`u32::MAX`] for a finding about
    /// a texture-name record, which has no material.
    pub const fn material(&self) -> u32 {
        match self {
            Self::TextureRecordField { .. } => u32::MAX,
            Self::UnknownMaterialFlags { material, .. }
            | Self::MaterialField { material, .. }
            | Self::MaterialLink { material, .. }
            | Self::CyclePointerMismatch { material, .. }
            | Self::CycleField { material, .. }
            | Self::TextureIndexOutOfRange { material, .. } => *material,
        }
    }

    /// The field the finding is about, named as the layout names it, or `""` for
    /// a finding that is about the record as a whole.
    #[must_use]
    pub const fn code_field(&self) -> &'static str {
        match self {
            Self::TextureRecordField { field, .. }
            | Self::MaterialField { field, .. }
            | Self::MaterialLink { field, .. }
            | Self::CycleField { field, .. } => field,
            Self::UnknownMaterialFlags { .. }
            | Self::CyclePointerMismatch { .. }
            | Self::TextureIndexOutOfRange { .. } => "",
        }
    }

    /// Evidence class of a finding: `ObservedTool` at best, never better, since
    /// a finding is by definition outside the documented profile.
    pub const fn evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }
}

impl fmt::Display for MaterialFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::TextureRecordField {
                texture,
                field,
                found,
            } => write!(f, "{code}: texture {texture} field {field} is {found}"),
            Self::UnknownMaterialFlags { material, bits } => {
                write!(f, "{code}: material {material} has flag bits 0x{bits:02X}")
            }
            Self::MaterialField {
                material,
                field,
                found,
            } => write!(f, "{code}: material {material} field {field} is {found}"),
            Self::MaterialLink {
                material,
                field,
                found,
                expected,
            } => write!(
                f,
                "{code}: material {material} {field} is {found}, expected {expected}"
            ),
            Self::CyclePointerMismatch {
                material,
                cycle_ptr,
            } => write!(
                f,
                "{code}: material {material} cycle pointer {cycle_ptr} disagrees with its cycled flag"
            ),
            Self::CycleField {
                material,
                field,
                found,
            } => write!(f, "{code}: material {material} cycle {field} is {found}"),
            Self::TextureIndexOutOfRange {
                material,
                index,
                available,
            } => write!(
                f,
                "{code}: material {material} names texture {index} of {available}"
            ),
        }
    }
}

/// The texture-name table and the material records of one CS GameZ container.
#[derive(Debug, Clone, PartialEq)]
pub struct GameZMaterials {
    /// The 40-byte container header, so a consumer never has to read it twice.
    pub header: GameZHeader,
    /// The texture-name table, in stored order. Its length is the header's
    /// `texture_count`.
    pub textures: Vec<GameZTextureName>,
    /// The four words at the start of the material section.
    pub info: MaterialInfo,
    /// The present material records, in stored order. Its length is
    /// [`MaterialInfo::count`], and it is the table a material index is
    /// range-checked against.
    pub materials: Vec<RawMaterial>,
    /// The all-zero slots stored after the present records. The reference always
    /// reads and writes [`NG_MATERIAL_SLOTS`] slots, so this is
    /// `NG_MATERIAL_SLOTS - count`; they are read and checked rather than
    /// skipped, because their length is what makes the cycle data's offset
    /// knowable.
    pub free_slots: u32,
    /// Everything this reader read that the reference's assertions do not allow,
    /// in stored order.
    pub findings: Vec<MaterialFinding>,
    /// Where the texture-name table started.
    pub textures_offset: u64,
    /// Where the material section started.
    pub materials_offset: u64,
    /// Where the material section ended, exclusive. Equal to
    /// [`GameZHeader::meshes_offset`], which is the check that the whole
    /// variable-length section was read and nothing was missed.
    pub data_end: u64,
}

impl GameZMaterials {
    /// The number of present material records: the value a stored material index
    /// must be below.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.materials.len() as u32
    }

    /// One texture-name entry by table index, or `None` when out of range.
    #[must_use]
    pub fn texture(&self, index: u32) -> Option<&GameZTextureName> {
        self.textures.get(index as usize)
    }

    /// One material record by table index, or `None` when out of range — which is
    /// the reference's `material_index < material_count` check, offered to a
    /// caller instead of being an assertion, so an out-of-range index is reported
    /// rather than clamped.
    #[must_use]
    pub fn material(&self, index: u32) -> Option<&RawMaterial> {
        self.materials.get(index as usize)
    }

    /// The texture a material names.
    ///
    /// `None` when the material is [`MaterialKind::Colored`] — it has no texture
    /// dependency at all — or when its stored index is outside the table, which
    /// the reference asserts cannot happen and which
    /// [`MaterialFinding::TextureIndexOutOfRange`] reports. The two are
    /// distinguishable: [`Self::names_a_texture`] says which.
    #[must_use]
    pub fn texture_of(&self, material: &RawMaterial) -> Option<&GameZTextureName> {
        if !self.names_a_texture(material) {
            return None;
        }
        self.texture(material.record.texture_index)
    }

    /// Whether a material names a texture at all, so a caller can tell an
    /// untextured material from a textured one whose index is out of range.
    #[must_use]
    pub fn names_a_texture(&self, material: &RawMaterial) -> bool {
        material.kind() == MaterialKind::Textured
    }

    /// Every table index that stores the same name as an earlier one, with all of
    /// its indices, in table order.
    ///
    /// The measured corpus contains one such group: `planes.zbd` stores the name
    /// `bldhwk_cowling..tif` 36 times. The pinned reference renames repeats on
    /// **write** (`name1.tif`, `name2.tif`, …) so that a round trip is
    /// unambiguous; that renaming is an extractor convention and is **not**
    /// reproduced here, so the duplicate stays visible to a consumer that has to
    /// decide what to do with it.
    #[must_use]
    pub fn duplicate_names(&self) -> Vec<(String, Vec<u32>)> {
        let mut groups: Vec<(String, Vec<u32>)> = Vec::new();
        for texture in &self.textures {
            match groups.iter_mut().find(|(name, _)| *name == texture.name) {
                Some((_, indices)) => indices.push(texture.index),
                None => groups.push((texture.name.clone(), vec![texture.index])),
            }
        }
        groups.retain(|(_, indices)| indices.len() > 1);
        groups
    }

    /// Evidence class of the layout this reader implements: documented in the
    /// pinned reference *and* measured against the original installation, which
    /// is `ObservedTool`. Never `VerifiedOriginal` here — that needs an original
    /// run, which has not happened.
    #[must_use]
    pub const fn layout_evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }
}

/// Why the texture-name table or the material records of a CS GameZ container
/// could not be read.
///
/// Each variant carries counts, offsets and field values only, never archive
/// bytes, and every offset is the absolute container offset the failure is
/// anchored at.
#[derive(Debug, Clone, PartialEq)]
pub enum GameZMaterialError {
    /// A read ran past the end of the section, or a stored count was refused by
    /// one of the parse's budgets: a [`ParseError`] already scoped as
    /// `gamez.materials.<field>`.
    Parse(ParseError),
    /// The container header is not a CS GameZ container, or its sections do not
    /// form the chain the layout requires. This is [`super::GameZError`], shared
    /// with the mesh reader so both entrypoints reject the same bytes for the
    /// same reason.
    ///
    /// It carries no container label, for the same reason the mesh reader's own
    /// validation variants do not: a header is refused on its own words, and the
    /// label travels with the [`Self::Parse`] failures a truncated read raises.
    Header(super::GameZError),
    /// `texture_count` is not below the reference's bound of 4096, so the
    /// texture table's byte extent is not established.
    TextureCount {
        /// The value found.
        found: u32,
    },
    /// The texture table's records do not fill
    /// `textures_offset..materials_offset`.
    TextureSectionEnd {
        /// Offset the walk reached.
        found: u64,
        /// Offset the header declares for the material records.
        expected: u32,
    },
    /// A texture name field holds no NUL at all, so the reference's decoding has
    /// nothing to terminate on.
    TextureNameUnterminated {
        /// Index in the texture-name table.
        texture: u32,
    },
    /// A texture name field holds a byte with the high bit set, which the
    /// reference's `from_ascii` refuses.
    TextureNameNotAscii {
        /// Index in the texture-name table.
        texture: u32,
        /// Byte position inside the 20-byte field.
        at: u8,
        /// The byte found.
        found: u8,
    },
    /// A terminated texture name has a non-zero byte after its terminator, which
    /// the reference requires to be padding.
    TextureNamePadding {
        /// Index in the texture-name table.
        texture: u32,
        /// Byte position inside the 20-byte field.
        at: u8,
        /// The byte found.
        found: u8,
    },
    /// `array_size` is outside `0..=1000`, or `count` is outside
    /// `0..=array_size`.
    MaterialCount {
        /// The word the failure is about.
        field: &'static str,
        /// Its value.
        found: i32,
    },
    /// `index_max` or `index_last` is not the value the reference asserts
    /// against `count`.
    MaterialIndex {
        /// The word the failure is about.
        field: &'static str,
        /// Its value.
        found: i32,
        /// The value the reference expects.
        expected: i32,
    },
    /// The material records and the cycle data did not end exactly on
    /// `meshes_offset`, so a record was read with the wrong length or one was
    /// skipped.
    MaterialSectionEnd {
        /// Offset the walk reached.
        found: u64,
        /// Offset the header declares for the mesh index.
        expected: u32,
    },
}

impl GameZMaterialError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
            Self::Header(_) => "header",
            Self::TextureCount { .. } => "texture_count",
            Self::TextureSectionEnd { .. } => "texture_section_end",
            Self::TextureNameUnterminated { .. } => "texture_name_unterminated",
            Self::TextureNameNotAscii { .. } => "texture_name_not_ascii",
            Self::TextureNamePadding { .. } => "texture_name_padding",
            Self::MaterialCount { .. } => "material_count",
            Self::MaterialIndex { .. } => "material_index",
            Self::MaterialSectionEnd { .. } => "material_section_end",
        }
    }

    /// The container label the failure came from, when the variant carries one
    /// from the checked reader.
    #[must_use]
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::Header(error) => error.container(),
            _ => "",
        }
    }

    /// The byte offset the failure is anchored at, when the variant names one.
    #[must_use]
    pub fn offset(&self) -> Option<u64> {
        match self {
            Self::Parse(error) => Some(error.offset),
            Self::Header(error) => error.offset(),
            Self::TextureSectionEnd { found, .. } | Self::MaterialSectionEnd { found, .. } => {
                Some(*found)
            }
            _ => None,
        }
    }
}

impl From<ParseError> for GameZMaterialError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for GameZMaterialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::Header(error) => write!(f, "{error}"),
            Self::TextureCount { found } => {
                write!(f, "at offset 12: texture count {found} is not below 4096")
            }
            Self::TextureSectionEnd { found, expected } => write!(
                f,
                "at offset {found}: the texture table ends here, the header declares the material \
                 records at {expected}"
            ),
            Self::TextureNameUnterminated { texture } => write!(
                f,
                "at offset {}: texture {texture} has no NUL in its 20-byte name field",
                u64::from(*texture) * TEXTURE_INFO_BYTES + TEXTURE_NAME_BYTES
            ),
            Self::TextureNameNotAscii { texture, at, found } => write!(
                f,
                "at offset {}: texture {texture} name byte {at} is 0x{found:02X}, not ASCII",
                u64::from(*texture) * TEXTURE_INFO_BYTES + TEXTURE_NAME_BYTES + u64::from(*at)
            ),
            Self::TextureNamePadding { texture, at, found } => write!(
                f,
                "at offset {}: texture {texture} name byte {at} is 0x{found:02X} after the \
                 terminator, not padding",
                u64::from(*texture) * TEXTURE_INFO_BYTES + TEXTURE_NAME_BYTES + u64::from(*at)
            ),
            Self::MaterialCount { field, found } => write!(
                f,
                "at offset {MATERIAL_HEADER_BYTES}: material {field} is {found}"
            ),
            Self::MaterialIndex {
                field,
                found,
                expected,
            } => write!(
                f,
                "at offset {}: material {field} is {found}, expected {expected}",
                MATERIAL_HEADER_BYTES + 8
            ),
            Self::MaterialSectionEnd { found, expected } => write!(
                f,
                "at offset {found}: the material section ends here, the header declares the mesh \
                 index at {expected}"
            ),
        }
    }
}

impl std::error::Error for GameZMaterialError {}

/// Reads the texture-name table and the material records of a CS GameZ container
/// into [`GameZMaterials`].
///
/// `bytes` is the whole container exactly as the installation stores it, the same
/// input [`read_gamez_meshes`](super::read_gamez_meshes) takes: the header is
/// read and validated here too, so either entrypoint can be used alone and both
/// reject the same bytes for the same reason. `_container` is the parse's own
/// label — [`ParseContext::new`] takes it and every reader error inherits it, so
/// this reader carries no second, possibly different, one.
///
/// The read runs through [`ParseContext::parse`] (spec F03), so a truncated
/// section, a hostile stored count and an over-large table are refused before
/// anything is allocated, every failure names an absolute offset, and a failed
/// attempt leaves the allocation ledger untouched so the same context can retry.
///
/// # Errors
///
/// * [`GameZMaterialError::Header`] for anything the container header itself
///   contradicts;
/// * [`GameZMaterialError::TextureCount`] when `texture_count` is not below
///   4096, and [`GameZMaterialError::TextureSectionEnd`] when the table's records
///   do not end on `materials_offset`;
/// * [`GameZMaterialError::TextureNameUnterminated`],
///   [`GameZMaterialError::TextureNameNotAscii`] and
///   [`GameZMaterialError::TextureNamePadding`] for a name field the reference's
///   decoding refuses;
/// * [`GameZMaterialError::MaterialCount`] and
///   [`GameZMaterialError::MaterialIndex`] when the section header contradicts
///   itself, and [`GameZMaterialError::MaterialSectionEnd`] when the records and
///   the cycle data do not end on `meshes_offset`.
pub fn read_gamez_materials(
    context: &mut ParseContext,
    _container: &str,
    bytes: &[u8],
) -> Result<GameZMaterials, GameZMaterialError> {
    match context.parse(
        MATERIALS_ENTRYPOINT,
        bytes,
        |reader, allocation, _recursion| match read_sections(reader, allocation, bytes.len() as u64)
        {
            Ok(value) => Ok(Ok(value)),
            Err(GameZMaterialError::Parse(error)) => Err(error),
            Err(domain) => Ok(Err(domain)),
        },
    ) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(domain)) => Err(domain),
        Err(error) => Err(GameZMaterialError::Parse(error)),
    }
}

/// One attempt inside the parse, in the same shape as the mesh reader's, so a
/// domain failure keeps its own variant and a reader failure keeps its own
/// `gamez.materials.<field>` scope while the ledger still rolls back.
fn read_sections(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    container_len: u64,
) -> Result<GameZMaterials, GameZMaterialError> {
    let header = super::reader::read_container_header(reader, container_len)
        .map_err(GameZMaterialError::Header)?;
    if header.texture_count >= MAX_TEXTURE_COUNT {
        return Err(GameZMaterialError::TextureCount {
            found: header.texture_count,
        });
    }
    let textures_offset = u64::from(header.textures_offset);
    let materials_offset = u64::from(header.materials_offset);
    let meshes_offset = u64::from(header.meshes_offset);

    let mut findings: Vec<MaterialFinding> = Vec::new();

    let mut table = reader
        .window(
            textures_offset,
            materials_offset - textures_offset,
            "texture table",
        )
        .map_err(GameZMaterialError::Parse)?;
    let textures = read_texture_table(&mut table, allocation, &header, &mut findings)?;
    let textures_end = table.position();
    if textures_end != materials_offset {
        return Err(GameZMaterialError::TextureSectionEnd {
            found: textures_end,
            expected: header.materials_offset,
        });
    }

    let mut section = reader
        .window(
            materials_offset,
            meshes_offset - materials_offset,
            "material records",
        )
        .map_err(GameZMaterialError::Parse)?;
    let (info, materials, free_slots) =
        read_material_table(&mut section, allocation, &header, &mut findings)?;
    let data_end = section.position();
    if data_end != meshes_offset {
        return Err(GameZMaterialError::MaterialSectionEnd {
            found: data_end,
            expected: header.meshes_offset,
        });
    }

    Ok(GameZMaterials {
        header,
        textures,
        info,
        materials,
        free_slots,
        findings,
        textures_offset,
        materials_offset,
        data_end,
    })
}

fn read_texture_table(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    header: &GameZHeader,
    findings: &mut Vec<MaterialFinding>,
) -> Result<Vec<GameZTextureName>, GameZMaterialError> {
    let count = usize::try_from(header.texture_count).expect("a u32 count fits a usize");
    // The stored table, the vector it becomes and the name bytes it owns: all
    // three are charged, so the ledger describes every buffer this parse hands
    // out and not only the one that could be large.
    allocation
        .reserve(
            "textures",
            reader.position(),
            u64::try_from(count).expect("usize is u64 on 64-bit") * TEXTURE_NAME_BYTES,
            1,
        )
        .map_err(GameZMaterialError::Parse)?;
    allocation
        .reserve(
            "textures.out",
            reader.position(),
            u64::try_from(count).expect("usize is u64 on 64-bit"),
            size_of::<GameZTextureName>() as u64,
        )
        .map_err(GameZMaterialError::Parse)?;

    let mut textures = Vec::with_capacity(count);
    for index in 0..header.texture_count {
        let field00 = reader.read_u32("texture.field00").map_err(material_parse)?;
        let field04 = reader.read_u32("texture.field04").map_err(material_parse)?;
        let field08 = reader.read_u32("texture.field08").map_err(material_parse)?;
        // The name is the record's **third** field, at offset 12, and `used`,
        // `index` and `unk40` follow it. Its length is fixed by the layout, so it
        // is read as one unit rather than field by field.
        let name_bytes = reader
            .read_bytes("texture.name", TEXTURE_NAME_BYTES as usize)
            .map_err(material_parse)?;
        let field: &[u8; TEXTURE_NAME_BYTES as usize] = name_bytes
            .try_into()
            .map_err(|_| GameZMaterialError::TextureNameUnterminated { texture: index })?;
        let (name, stem, suffix, encoding) = decode_name(field, index)?;
        let field32 = reader.read_u32("texture.used").map_err(material_parse)?;
        let field36 = reader.read_u32("texture.index").map_err(material_parse)?;
        let field40 = reader.read_i32("texture.unk40").map_err(material_parse)?;

        for (field, found) in [("field04", field04), ("field08", field08)] {
            if found != 0 {
                findings.push(MaterialFinding::TextureRecordField {
                    texture: index,
                    field,
                    found,
                });
            }
        }
        if field32 != TEXTURE_STATE_USED && field32 != 1 {
            findings.push(MaterialFinding::TextureRecordField {
                texture: index,
                field: "field32",
                found: field32,
            });
        }
        if field36 != 0 {
            findings.push(MaterialFinding::TextureRecordField {
                texture: index,
                field: "field36",
                found: field36,
            });
        }
        if field40 != -1 {
            findings.push(MaterialFinding::TextureRecordField {
                texture: index,
                field: "field40",
                found: field40 as u32,
            });
        }
        textures.push(GameZTextureName {
            index,
            name,
            stem,
            suffix,
            encoding,
            field00,
            field32,
            field36,
            field40,
        });
    }
    Ok(textures)
}

/// Decodes one 20-byte name field exactly as `str_from_c_suffix` does.
///
/// Three shapes occur in the measured corpus and all three are ordinary:
///
/// * `bldhwk_cowling\0.tif\0` — a name, the `.` stored as the NUL, an extension
///   that itself starts with `.`, then padding. The stored name is
///   `bldhwk_cowling..tif`, and the second dot is what the field holds.
/// * `lightmap\0\0…` — a name and nothing after it.
/// * `horizonindicator\0tif` — 20 bytes, completely full. The reference restores
///   the `.` and takes the whole field.
///
/// A field with no NUL at all is the one shape the reference refuses.
fn decode_name(
    field: &[u8; TEXTURE_NAME_BYTES as usize],
    texture: u32,
) -> Result<(String, String, Option<String>, TextureNameEncoding), GameZMaterialError> {
    if let Some((at, found)) = field
        .iter()
        .enumerate()
        .find(|(_, byte)| *byte & 0x80 != 0)
        .map(|(at, byte)| (at as u8, *byte))
    {
        return Err(GameZMaterialError::TextureNameNotAscii { texture, at, found });
    }
    let Some(first) = field.iter().position(|byte| *byte == 0) else {
        return Err(GameZMaterialError::TextureNameUnterminated { texture });
    };
    let stem = ascii(&field[..first]);

    match field[first + 1..].iter().position(|byte| *byte == 0) {
        // `(Some(zero1), None)`: the field is full, so the name ran into its last
        // byte. Restore the `.` and take all twenty bytes; there is no room for
        // padding, so there is no padding to check.
        None => {
            let mut owned = *field;
            owned[first] = b'.';
            let name = ascii(&owned);
            let suffix = name[first + 1..].to_owned();
            Ok((name, stem, Some(suffix), TextureNameEncoding::Unterminated))
        }
        Some(offset) => {
            let second = first + 1 + offset;
            if let Some((at, found)) = field[second + 1..]
                .iter()
                .enumerate()
                .find(|(_, byte)| **byte != 0)
                .map(|(at, byte)| ((second + 1 + at) as u8, *byte))
            {
                return Err(GameZMaterialError::TextureNamePadding { texture, at, found });
            }
            if second == first + 1 {
                // The byte after the terminator is a terminator too: no suffix.
                return Ok((stem.clone(), stem, None, TextureNameEncoding::StemOnly));
            }
            let suffix = ascii(&field[first + 1..second]);
            let name = format!("{stem}.{suffix}");
            Ok((name, stem, Some(suffix), TextureNameEncoding::WithSuffix))
        }
    }
}

/// The reference's `from_ascii`, which refuses only a byte with the high bit
/// set. The whole field was scanned once, before any run was taken out of it, so
/// these sub-ranges cannot contain one.
fn ascii(bytes: &[u8]) -> String {
    debug_assert!(
        bytes.iter().all(|byte| byte & 0x80 == 0),
        "the high-bit scan already refused this run"
    );
    // The reference's own check: every byte below 0x80, control bytes included.
    String::from_utf8_lossy(bytes).into_owned()
}

/// The name-field read failure, re-anchored at the field the reader refused.
fn material_parse(error: ParseError) -> GameZMaterialError {
    GameZMaterialError::Parse(error)
}

fn read_material_table(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    header: &GameZHeader,
    findings: &mut Vec<MaterialFinding>,
) -> Result<(MaterialInfo, Vec<RawMaterial>, u32), GameZMaterialError> {
    let array_size = reader
        .read_i32("material.array_size")
        .map_err(material_parse)?;
    let count = reader.read_i32("material.count").map_err(material_parse)?;
    let index_max = reader
        .read_i32("material.index_max")
        .map_err(material_parse)?;
    let index_last = reader
        .read_i32("material.index_last")
        .map_err(material_parse)?;
    if array_size < 0 || array_size > NG_MATERIAL_SLOTS as i32 {
        return Err(GameZMaterialError::MaterialCount {
            field: "array_size",
            found: array_size,
        });
    }
    if count < 0 || count > array_size {
        return Err(GameZMaterialError::MaterialCount {
            field: "count",
            found: count,
        });
    }
    for (field, found, expected) in [
        ("index_max", index_max, count),
        ("index_last", index_last, count - 1),
    ] {
        if found != expected {
            return Err(GameZMaterialError::MaterialIndex {
                field,
                found,
                expected,
            });
        }
    }
    let info = MaterialInfo {
        array_size,
        count,
        index_max,
        index_last,
    };
    let valid = count as u32;

    // One reservation for the whole 1000-slot array, so a hostile `count` cannot
    // turn the walk into a per-slot allocation.
    allocation
        .reserve(
            "materials",
            reader.position(),
            u64::from(NG_MATERIAL_SLOTS),
            MATERIAL_SLOT_BYTES,
        )
        .map_err(GameZMaterialError::Parse)?;
    allocation
        .reserve(
            "materials.out",
            reader.position(),
            u64::from(NG_MATERIAL_SLOTS),
            size_of::<RawMaterial>() as u64,
        )
        .map_err(GameZMaterialError::Parse)?;

    let mut materials: Vec<RawMaterial> = Vec::with_capacity(valid as usize);
    for index in 0..valid {
        let record = read_material_record(reader)?;
        check_material_fields(record, index, findings);
        let link1 = reader.read_i16("material.link1").map_err(material_parse)?;
        let link2 = reader.read_i16("material.link2").map_err(material_parse)?;
        // A present slot's first link word points at the *next* material and its
        // second at the previous one. The zero slots below store the pair the
        // other way round, and that is the reference's own reading of both.
        let expected1 = if index + 1 >= valid {
            -1
        } else {
            (index + 1) as i16
        };
        let expected2 = if index == 0 { -1 } else { (index - 1) as i16 };
        check_links(index, link1, link2, expected1, expected2, findings);
        materials.push(RawMaterial {
            index,
            record,
            link1,
            link2,
            cycle: None,
        });
    }

    // The zero slots. Their length is what makes the cycle data's offset
    // knowable, so they are walked rather than skipped, and each is required to
    // be the all-zero record the reference asserts.
    for index in valid..NG_MATERIAL_SLOTS {
        let record = read_material_record(reader)?;
        check_zero_slot(record, index, findings);
        let link1 = reader.read_i16("material.link1").map_err(material_parse)?;
        let link2 = reader.read_i16("material.link2").map_err(material_parse)?;
        let expected1 = if index == valid {
            -1
        } else {
            (index - 1) as i16
        };
        let expected2 = if index + 1 >= NG_MATERIAL_SLOTS {
            -1
        } else {
            (index + 1) as i16
        };
        check_links(index, link1, link2, expected1, expected2, findings);
    }
    let free_slots = NG_MATERIAL_SLOTS - valid;

    // The cycle data, in material order, after the whole array.
    for material in &mut materials {
        let index = material.index;
        let cycled = material.record.is_cycled();
        if cycled != (material.record.cycle_ptr != 0) {
            findings.push(MaterialFinding::CyclePointerMismatch {
                material: index,
                cycle_ptr: material.record.cycle_ptr,
            });
        }
        if !cycled {
            continue;
        }
        let cycle = read_cycle(reader, allocation, index, findings)?;
        material.cycle = Some(cycle);
    }

    for material in &materials {
        if material.record.kind() != MaterialKind::Textured {
            continue;
        }
        let index = material.record.texture_index;
        if index >= header.texture_count {
            findings.push(MaterialFinding::TextureIndexOutOfRange {
                material: material.index,
                index,
                available: header.texture_count,
            });
        }
        if let Some(cycle) = &material.cycle {
            for frame in &cycle.textures {
                if *frame >= header.texture_count {
                    findings.push(MaterialFinding::TextureIndexOutOfRange {
                        material: material.index,
                        index: *frame,
                        available: header.texture_count,
                    });
                }
            }
        }
    }

    Ok((info, materials, free_slots))
}

fn check_links(
    material: u32,
    link1: i16,
    link2: i16,
    expected1: i16,
    expected2: i16,
    findings: &mut Vec<MaterialFinding>,
) {
    for (field, found, expected) in [("link1", link1, expected1), ("link2", link2, expected2)] {
        if found != expected {
            findings.push(MaterialFinding::MaterialLink {
                material,
                field,
                found,
                expected,
            });
        }
    }
}

fn read_material_record(reader: &mut Reader<'_>) -> Result<RawMaterialRecord, GameZMaterialError> {
    Ok(RawMaterialRecord {
        alpha: reader.read_u8("material.alpha").map_err(material_parse)?,
        flags: reader.read_u8("material.flags").map_err(material_parse)?,
        rgb: reader.read_u16("material.rgb").map_err(material_parse)?,
        color: [
            reader
                .read_f32("material.color.r")
                .map_err(material_parse)?,
            reader
                .read_f32("material.color.g")
                .map_err(material_parse)?,
            reader
                .read_f32("material.color.b")
                .map_err(material_parse)?,
        ],
        texture_index: reader.read_u32("material.index").map_err(material_parse)?,
        field20: reader
            .read_f32("material.field20")
            .map_err(material_parse)?,
        field24: reader
            .read_f32("material.field24")
            .map_err(material_parse)?,
        field28: reader
            .read_f32("material.field28")
            .map_err(material_parse)?,
        field32: reader
            .read_f32("material.field32")
            .map_err(material_parse)?,
        cycle_ptr: reader
            .read_u32("material.cycle_ptr")
            .map_err(material_parse)?,
    })
}

/// Pushes one value assertion of a **present** material record.
///
/// None of these changes a stored length, so a record that breaks one is read and
/// reported rather than refused: a dependency the audit has to see is not thrown
/// away because one of its cosmetic fields is odd.
fn material_field(
    findings: &mut Vec<MaterialFinding>,
    material: u32,
    field: &'static str,
    broken: bool,
    found: u64,
) {
    if broken {
        findings.push(MaterialFinding::MaterialField {
            material,
            field,
            found,
        });
    }
}

fn check_material_fields(
    record: RawMaterialRecord,
    index: u32,
    findings: &mut Vec<MaterialFinding>,
) {
    let bits = record.unknown_flag_bits();
    if bits != 0 {
        findings.push(MaterialFinding::UnknownMaterialFlags {
            material: index,
            bits,
        });
    }
    material_field(
        findings,
        index,
        "always",
        record.flags & MATERIAL_FLAG_ALWAYS == 0,
        0,
    );
    material_field(
        findings,
        index,
        "free",
        record.flags & MATERIAL_FLAG_FREE != 0,
        0,
    );
    material_field(
        findings,
        index,
        "field20",
        record.field20.to_bits() != 0.0f32.to_bits(),
        u64::from(record.field20.to_bits()),
    );
    if record.kind() == MaterialKind::Textured {
        material_field(
            findings,
            index,
            "alpha",
            record.alpha != 0xFF,
            u64::from(record.alpha),
        );
        material_field(
            findings,
            index,
            "rgb",
            record.rgb != 0x7FFF,
            u64::from(record.rgb),
        );
        for (field, value) in [
            ("color.r", record.color[0]),
            ("color.g", record.color[1]),
            ("color.b", record.color[2]),
        ] {
            material_field(
                findings,
                index,
                field,
                value.to_bits() != 255.0f32.to_bits(),
                u64::from(value.to_bits()),
            );
        }
        for (field, value) in [("field24", record.field24), ("field28", record.field28)] {
            material_field(
                findings,
                index,
                field,
                value.to_bits() != 0.5f32.to_bits(),
                u64::from(value.to_bits()),
            );
        }
    } else {
        // The reference asserts four things about an untextured record and no
        // more: the unknown and cycled flags are clear (checked below), the `rgb`
        // word is zero, the texture index is zero and the cycle pointer is zero.
        // It asserts **nothing** about `alpha` or the colour, and the measured
        // corpus stores `0xFF` there, so asserting it would be inventing a rule.
        material_field(
            findings,
            index,
            "rgb",
            record.rgb != 0,
            u64::from(record.rgb),
        );
        material_field(
            findings,
            index,
            "texture_index",
            record.texture_index != 0,
            u64::from(record.texture_index),
        );
        material_field(findings, index, "cycled", record.is_cycled(), 0);
        material_field(
            findings,
            index,
            "unknown",
            record.flags & MATERIAL_FLAG_UNKNOWN != 0,
            0,
        );
    }
}

/// The all-zero record the reference asserts for every slot past `count`.
///
/// A zero slot is the one place where the record's own bytes are the claim: the
/// reference requires every field to be zero and the free flag to be the only
/// bit set. Breaking that is reported and changes no length, so the walk
/// continues and the section boundary still decides whether the container was
/// really read.
fn check_zero_slot(record: RawMaterialRecord, index: u32, findings: &mut Vec<MaterialFinding>) {
    material_field(
        findings,
        index,
        "free",
        record.flags != MATERIAL_FLAG_FREE,
        u64::from(record.flags),
    );
    material_field(
        findings,
        index,
        "alpha",
        record.alpha != 0,
        u64::from(record.alpha),
    );
    material_field(
        findings,
        index,
        "rgb",
        record.rgb != 0,
        u64::from(record.rgb),
    );
    for (field, value) in [
        ("color.r", record.color[0]),
        ("color.g", record.color[1]),
        ("color.b", record.color[2]),
    ] {
        material_field(
            findings,
            index,
            field,
            value.to_bits() != 0.0f32.to_bits(),
            u64::from(value.to_bits()),
        );
    }
    material_field(
        findings,
        index,
        "texture_index",
        record.texture_index != 0,
        u64::from(record.texture_index),
    );
    for (field, value) in [
        ("field20", record.field20),
        ("field24", record.field24),
        ("field28", record.field28),
        ("field32", record.field32),
    ] {
        material_field(
            findings,
            index,
            field,
            value.to_bits() != 0.0f32.to_bits(),
            u64::from(value.to_bits()),
        );
    }
    material_field(
        findings,
        index,
        "cycle_ptr",
        record.cycle_ptr != 0,
        u64::from(record.cycle_ptr),
    );
}

fn read_cycle(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    material: u32,
    findings: &mut Vec<MaterialFinding>,
) -> Result<RawCycle, GameZMaterialError> {
    let field00 = reader.read_u32("cycle.field00").map_err(material_parse)?;
    let field04 = reader.read_u32("cycle.field04").map_err(material_parse)?;
    let field08 = reader.read_u32("cycle.field08").map_err(material_parse)?;
    let field12 = reader.read_f32("cycle.field12").map_err(material_parse)?;
    let count1 = reader.read_u32("cycle.count1").map_err(material_parse)?;
    let count2 = reader.read_u32("cycle.count2").map_err(material_parse)?;
    let data_ptr = reader.read_u32("cycle.data_ptr").map_err(material_parse)?;

    material_field(
        findings,
        material,
        "field00",
        field00 == 0,
        u64::from(field00),
    );
    material_field(
        findings,
        material,
        "field08",
        field08 != 0,
        u64::from(field08),
    );
    material_field(
        findings,
        material,
        "data_ptr",
        data_ptr == 0,
        u64::from(data_ptr),
    );
    if !(0.0..=16.0).contains(&field12) {
        findings.push(MaterialFinding::CycleField {
            material,
            field: "field12",
            found: u64::from(field12.to_bits()),
        });
    }
    if count1 != count2 {
        findings.push(MaterialFinding::CycleField {
            material,
            field: "count2",
            found: u64::from(count2),
        });
    }

    allocation
        .reserve(
            "cycle.frames",
            reader.position(),
            u64::from(count1),
            CYCLE_FRAME_BYTES,
        )
        .map_err(GameZMaterialError::Parse)?;
    allocation
        .reserve(
            "cycle.frames.out",
            reader.position(),
            u64::from(count1),
            size_of::<u32>() as u64,
        )
        .map_err(GameZMaterialError::Parse)?;

    let mut textures = Vec::with_capacity(count1 as usize);
    for _ in 0..count1 {
        textures.push(reader.read_u32("cycle.texture").map_err(material_parse)?);
    }
    Ok(RawCycle {
        field00,
        field04,
        field08,
        field12,
        count1,
        count2,
        data_ptr,
        textures,
    })
}
