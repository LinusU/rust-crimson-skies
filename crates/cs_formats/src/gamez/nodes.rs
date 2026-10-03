//! The CS GameZ node array and its reader.
//!
//! # Provenance
//!
//! The layout below is **established, not guessed**. It was read from the
//! pinned legacy CS-capable reference, mech3ax **v0.6.0**, commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
//! `docs/research/SOURCES.md`), and then **measured against every GameZ
//! archive of the original installation** — nine archives, 56 620 stored node
//! records — with the per-archive numbers in
//! `docs/findings/2026-10-02-gamez-node-array-layout.md`. That evidence class
//! is `ObservedTool`: documented in a reference tool and reproduced from the
//! original bytes. No code was copied; mech3ax is EUPL-1.2 and is read as a
//! reference only.
//!
//! | What | Source |
//! | --- | --- |
//! | the 40-byte header and the `node_array_size` / `light_index` / `nodes_offset` words | `crates/mech3ax-gamez/src/gamez/cs/mod.rs` (`HeaderCsC`), already read by [`super::reader`] |
//! | the two-pass node array: `array_size` × (`NodeCsC` 208 bytes + one 4-byte node index), then the per-kind data records | `crates/mech3ax-gamez/src/gamez/cs/nodes.rs` (`read_nodes`, `write_nodes`) |
//! | the 208-byte `NodeCsC` info record and its field offsets | `crates/mech3ax-nodes/src/cs/node.rs:86-117` (`NodeCsC`, `assert_node`, `read_node_info`) |
//! | the node-type tags | `crates/mech3ax-nodes/src/types.rs` (`NodeType`) |
//! | the 144-byte `Object3dCsC` object record | `crates/mech3ax-nodes/src/cs/object3d/data.rs` |
//! | the 92-byte `LodCsC` LOD record | `crates/mech3ax-nodes/src/cs/lod/data.rs` |
//! | the 204-byte `WorldCsC` record and its 88-byte `PartitionCsC` grid | `crates/mech3ax-nodes/src/cs/world/data.rs` |
//! | the 28/248/488/256-byte display, window, camera and light records | `crates/mech3ax-nodes/src/node_data/display.rs`, `crates/mech3ax-nodes/src/cs/{window,camera,light}/data.rs` |
//! | `euler_to_matrix` | `crates/mech3ax-nodes/src/math.rs` |
//!
//! # The shape of the array
//!
//! A CS GameZ node array is **two passes over two sections**, and the
//! distinction matters because a reader that treats it as one flat array of
//! fixed-size records reads nonsense:
//!
//! 1. the **info array**, `nodes_offset .. nodes_offset + 212 · node_array_size`:
//!    one [`RawNodeInfo`] (208 bytes) immediately followed by one 4-byte
//!    `node_index` word, interleaved, per node;
//! 2. the **data section**, which starts exactly where the info array ends and
//!    holds one kind-specific record per node **in node order**, each followed
//!    by its parent slot and its child slots.
//!
//! `node_array_size` is a **count of records, not a byte length**: the node
//! array is not `nodes_offset .. nodes_offset + node_array_size`. The data
//! section is variable-length, and its total size is the distance from the
//! first record to the end of the container.
//!
//! Each info record stores an absolute `data_ptr`. This reader **does not
//! follow it**: it walks the data section sequentially, exactly as the
//! reference does, and *checks* that the `data_ptr` it walked to equals the
//! pointer the record stored ([`GameZNodeError::DataOffset`]). That turns a
//! stored pointer into a cross-check instead of a second, competing way to
//! address the same bytes, and it is what proves no record was skipped. The
//! same walk ends exactly at the end of the container in all nine measured
//! archives.
//!
//! # What is read and what is not
//!
//! A node's *own* record is read for every kind, because the record's length is
//! what advances the walk. What is *interpreted* is much narrower:
//!
//! * an object node's `flags`, euler `rotation`, per-axis `scale`, stored
//!   `matrix` and `translation` — the authored transform;
//! * a LOD node's `level` and its `range_near_sq` / `range_far` /
//!   `range_far_sq`;
//! * a world node's two partition-grid counts, which are what make its
//!   variable-length block knowable at all.
//!
//! Everything else stays raw, on two levels. [`RawNodeInfo`] is the whole
//! 208-byte record, every unmeasured word included, so a later stage re-derives
//! from the record instead of trusting a guess; and [`RawNode::data_offset`] +
//! [`RawNode::data_bytes`] address the node's own record bytes, so a stage that
//! needs the world's fog and partitions, the camera's FOV or the light's range
//! reads them from the same bytes this reader measured rather than asking it to
//! have guessed. A finding is **not** an error here: a record the reference
//! would have refused is still read, with its stored lengths intact, and the
//! deviation is reported ([`NodeFinding`]).
//!
//! The CS node flag bits are unmeasured — every bit is `UNK*` in the pinned
//! reference — so `flags` is carried raw and nothing here interprets it. A
//! node's `mesh_index` is carried raw too: the node reader does not hold the
//! mesh section, so it cannot range-check a non-negative index, and it says so
//! through [`GameZNodes::mesh_index_bounds`] instead of pretending to. The
//! check itself is [`super::bindings::NodeMeshBindings::of`], which is handed
//! this section **and** the mesh array and reports every node whose index names
//! no present mesh.

use std::fmt;
use std::mem::size_of;

use cs_types::evidence::ClaimStatus;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader};

use super::reader::{GameZHeader, read_container_header};

/// Error scope stamped onto failures raised while reading the node array.
pub const NODES_ENTRYPOINT: &str = "gamez.nodes";

/// Bytes of one stored node's whole info slot: the 36-byte name field followed
/// by [`RawNodeInfo`], `static_assert_size!(NodeCsC, 208)` in the pinned
/// reference.
///
/// This is the size of the **slot**, not of [`RawNodeInfo`]: the name is read as
/// a bounded C string and kept as a `String`, so the record struct holds only
/// the 172 bytes that follow it. Every field name on [`RawNodeInfo`] is the
/// offset the reference gives that field in the whole `NodeCsC`, so a field
/// named `unk040` is the record's **first** field even though the reference
/// spells it 40. The two sizes are tied together by a compile-time assertion in
/// the same module, and by `NODE_TYPE_OFFSET`, which is what a diagnostic needs.
pub const NODE_INFO_BYTES: u64 = 208;

/// Bytes of the 4-byte word that follows every node info record: the node's
/// own `node_index`.
pub const NODE_INDEX_BYTES: u64 = 4;

/// Offset of the `node_type` word **inside one 212-byte info slot**: the
/// 36-byte name field, then eleven 4-byte words (`flags`, `unk040`, `unk044`,
/// `zone_id` and the tag itself).
///
/// The offset is counted from the start of the slot rather than from the start
/// of [`RawNodeInfo`], because every field name in this module is the offset the
/// pinned reference gives it in the whole `NodeCsC`, which includes the name.
pub const NODE_TYPE_OFFSET: u64 = 52;

/// Bytes of one node's entry in the info array: the info record and its
/// `node_index` word, interleaved. This is the stride of the info array, not
/// the size of the whole node array.
pub const NODE_SLOT_BYTES: u64 = NODE_INFO_BYTES + NODE_INDEX_BYTES;

/// Bytes of the stored node name field, NUL-terminated inside its own bound.
pub const NODE_NAME_BYTES: u64 = 36;

/// `NodeType::Camera`.
pub const NODE_TYPE_CAMERA: u32 = 1;
/// `NodeType::World`.
pub const NODE_TYPE_WORLD: u32 = 2;
/// `NodeType::Window`.
pub const NODE_TYPE_WINDOW: u32 = 3;
/// `NodeType::Display`.
pub const NODE_TYPE_DISPLAY: u32 = 4;
/// `NodeType::Object3d`.
pub const NODE_TYPE_OBJECT3D: u32 = 5;
/// `NodeType::LoD`.
pub const NODE_TYPE_LOD: u32 = 6;
/// `NodeType::Light`.
pub const NODE_TYPE_LIGHT: u32 = 9;

/// `NodeType::Empty`, the one tag the reference asserts never occurs: a CS
/// container holds no empty node.
pub const NODE_TYPE_EMPTY: u32 = 0;

/// Mask of the top byte of the trailing `node_index` word
/// (`NODE_INDEX_TOP_MASK` in the reference).
pub const NODE_INDEX_TOP_MASK: u32 = 0xFF00_0000;

/// The value the reference asserts that top byte carries
/// (`NODE_INDEX_TOP`).
pub const NODE_INDEX_TOP: u32 = 0x0200_0000;

/// Mask of the low three bytes of the `node_index` word, the part the
/// reference treats as the node's own index (`NODE_INDEX_BOT_MASK`).
pub const NODE_INDEX_BOT_MASK: u32 = 0x00FF_FFFF;

/// The `node_index` the reference reserves for "no index"
/// (`NODE_INDEX_INVALID`). One record per measured archive carries it.
pub const NODE_INDEX_INVALID: u32 = 0x00FF_FFFF;

/// Bytes of one object node's own record: `Object3dCsC`,
/// `static_assert_size!(Object3dCsC, 144)`.
pub const OBJECT3D_DATA_BYTES: u64 = 144;

/// `Object3dCsC.flags` for a record that stores a transform.
pub const OBJECT3D_FLAGS_TRANSFORMED: u32 = 32;

/// `Object3dCsC.flags` for a record that stores no transform: the reference
/// asserts the rotation, translation and matrix are then exactly identity.
pub const OBJECT3D_FLAGS_IDENTITY: u32 = 40;

/// Bytes of one LOD node's own record: `LodCsC`,
/// `static_assert_size!(LodCsC, 92)`.
pub const LOD_DATA_BYTES: u64 = 92;

/// Bytes of one world node's own fixed record: `WorldCsC`,
/// `static_assert_size!(WorldCsC, 204)`.
pub const WORLD_DATA_BYTES: u64 = 204;

/// Bytes of one `PartitionCsC` grid cell: `static_assert_size!(PartitionCsC, 88)`.
pub const WORLD_PARTITION_BYTES: u64 = 88;

/// Bytes of one `PartitionValue`: a `u32` node index plus two `f32` bounds,
/// `static_assert_size!(PartitionValue, 12)`.
pub const WORLD_PARTITION_VALUE_BYTES: u64 = 12;

/// Bytes of one display node's own record: `DisplayC`,
/// `static_assert_size!(DisplayC, 28)`.
pub const DISPLAY_DATA_BYTES: u64 = 28;

/// Bytes of one window node's own record: `WindowCsC`,
/// `static_assert_size!(WindowCsC, 248)`.
pub const WINDOW_DATA_BYTES: u64 = 248;

/// Bytes of one camera node's own record: `CameraC`,
/// `static_assert_size!(CameraC, 488)`.
pub const CAMERA_DATA_BYTES: u64 = 488;

/// Bytes of one light node's own record: `LightCsC`,
/// `static_assert_size!(LightCsC, 256)`, plus the one `u32` parent word the
/// reference reads after it.
pub const LIGHT_DATA_BYTES: u64 = 256;

/// The largest difference, per matrix entry, at which a stored `matrix` is
/// taken to agree with the one its own euler triple derives.
///
/// The measured corpus is bimodal: over 18 400 transformed object records the
/// two agree to within `2.8e-8` (f32 rounding of the same expression) or
/// differ by more than `1e-2`. A tolerance between the two populations
/// separates them exactly; it is a **measurement**, not a claim about the
/// original engine, and a disagreement is reported through
/// [`RawObject3dData::matrix_disagrees`] rather than silently preferring one
/// representation.
pub const MATRIX_AGREEMENT_TOLERANCE: f32 = 1.0e-4;

/// One stored node's info record, decoded field by field: the 172 bytes of
/// `NodeCsC` that follow its 36-byte name field.
///
/// Every word is kept, including the ones no measured meaning attaches to:
/// `unk040`, `unk044`, `environment_data`, `action_priority`,
/// `action_callback`, `area_partition`, `unk112`, the three bounding boxes,
/// `unk196` and the words the reference asserts are zero. F11-A's typed input
/// ([`cs_content::scene::ParsedNode`]) deliberately drops them; keeping them
/// here is what lets a later stage re-derive them from the record instead of
/// from a guess (F11-A finding, "Unmeasured stored fields are not carried by
/// the typed input").
///
/// **Each field's name is the offset the pinned reference gives it in the whole
/// 208-byte `NodeCsC`, which includes the name.** So this struct is 172 bytes,
/// [`NODE_INFO_BYTES`] is the 208-byte slot the name shares, and `unk040` is the
/// struct's first field rather than its fortieth. A stage that re-derives a
/// field's position from its name has to add [`NODE_NAME_BYTES`] first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawNodeInfo {
    /// `flags`: the CS node flag word. Every bit is `UNK*` in the reference,
    /// so nothing interprets it; the reference asserts bits 19 and 24 are
    /// always set.
    pub flags: u32,
    /// `unk040`: not interpreted. The reference asserts a container holds only
    /// a measured set of values.
    pub unk040: u32,
    /// `unk044`: not interpreted.
    pub unk044: u32,
    /// `zone_id`: the stored damage/zone id. `ZONE_DEFAULT` (255) means no
    /// zone; the field's domain beyond that is unmeasured and it is carried
    /// raw.
    pub zone_id: u32,
    /// `node_type`: the kind tag, one of the [`NODE_TYPE_*`] constants.
    pub node_type: u32,
    /// `data_ptr`: this node's own record's absolute container offset. The
    /// reference asserts it is non-zero; this reader walks the data section
    /// sequentially instead and checks the two agree.
    pub data_ptr: u32,
    /// `mesh_index`: the index into the container's mesh array, stored signed
    /// with `-1` meaning "no mesh". Only a non-negative value is a mesh
    /// association.
    pub mesh_index: i32,
    /// `environment_data`: the reference asserts it is zero.
    pub environment_data: u32,
    /// `action_priority`: the reference asserts it is one.
    pub action_priority: u32,
    /// `action_callback`: the reference asserts it is zero.
    pub action_callback: u32,
    /// `area_partition`: the reference's `AreaPartitionPm` (`x`, `y`,
    /// `virtual_x`, `virtual_y` as `i16`), with `(-1, -1, 0, 0)` meaning
    /// "default". Unmeasured, kept raw.
    pub area_partition: [i16; 4],
    /// `parent_count`: the reference reads it as a **boolean** — a node has a
    /// parent or it does not — and asserts nothing above one.
    pub parent_count: u16,
    /// `children_count`: how many child slots follow this node's record.
    pub children_count: u16,
    /// `parent_array_ptr`: the reference's `Ptr` to the parent slot. A runtime
    /// address, not a file offset; kept raw.
    pub parent_array_ptr: u32,
    /// `children_array_ptr`: the reference's `Ptr` to the child slots. Raw.
    pub children_array_ptr: u32,
    /// `unk096`: the reference asserts it is zero.
    pub unk096: u32,
    /// `unk100`: the reference asserts it is zero.
    pub unk100: u32,
    /// `unk104`: the reference asserts it is zero.
    pub unk104: u32,
    /// `unk108`: the reference asserts it is zero.
    pub unk108: u32,
    /// `unk112`: the reference asserts `0`, `1` or `2` and never interprets it.
    pub unk112: u32,
    /// `unk116`: a 24-byte bounding box (two `Vec3`s). Unmeasured.
    pub unk116: [[f32; 3]; 2],
    /// `unk140`: a 24-byte bounding box. Unmeasured.
    pub unk140: [[f32; 3]; 2],
    /// `unk164`: a 24-byte bounding box. Unmeasured.
    pub unk164: [[f32; 3]; 2],
    /// `unk188`: the reference asserts it is zero.
    pub unk188: u32,
    /// `unk192`: the reference asserts it is zero.
    pub unk192: u32,
    /// `unk196`: the reference asserts `0` for a world/display/window/camera/
    /// light and `160` for an object or LOD node, and never interprets it.
    pub unk196: u32,
    /// `unk200`: the reference asserts it is zero.
    pub unk200: u32,
    /// `unk204`: the reference asserts it is zero.
    pub unk204: u32,
}

impl RawNodeInfo {
    /// Whether the record declares a parent slot, which the reference reads as
    /// `parent_count != 0`.
    #[must_use]
    pub const fn has_parent(&self) -> bool {
        self.parent_count != 0
    }
}

/// The slot is the record plus its name, and the walk reads them in that order.
/// If either size drifts, every record after the first lands at the wrong
/// offset, so the arithmetic is pinned at compile time rather than discovered
/// nine archives later.
const _: () = {
    assert!(
        size_of::<RawNodeInfo>() as u64 + NODE_NAME_BYTES == NODE_INFO_BYTES,
        "the info record and its name field must fill the 208-byte slot"
    );
    assert!(
        NODE_TYPE_OFFSET < NODE_INFO_BYTES,
        "node_type is a word inside the slot"
    );
};

/// One object node's own 144-byte record: the authored transform, and the
/// fields around it the reference pins to zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawObject3dData {
    /// `flags`: [`OBJECT3D_FLAGS_TRANSFORMED`] or
    /// [`OBJECT3D_FLAGS_IDENTITY`]; the reference asserts no other value.
    pub flags: u32,
    /// `rotation`: the stored euler triple, in the source's angle unit.
    pub rotation: [f32; 3],
    /// `scale`: the stored per-axis scale. Every measured record stores
    /// exactly `1.0`, so the composition order of scale and rotation is
    /// unobservable in the corpus.
    pub scale: [f32; 3],
    /// `matrix`: the stored 3×3, row-major, source axes. It takes precedence
    /// over the euler triple whenever the two disagree.
    pub matrix: [[f32; 3]; 3],
    /// `translation`: the stored translation, in source units.
    pub translation: [f32; 3],
}

impl RawObject3dData {
    /// Whether the record stores no transform at all
    /// ([`OBJECT3D_FLAGS_IDENTITY`]), which is the reference's signal that the
    /// authored transform is the identity.
    #[must_use]
    pub const fn stores_identity(&self) -> bool {
        self.flags == OBJECT3D_FLAGS_IDENTITY
    }

    /// The 3×3 this record's euler triple derives, in the reference's
    /// convention: `Rz·Ry·Rx` over **negated** angles, composing the **raw
    /// stored numbers** (`euler_to_matrix` in
    /// `crates/mech3ax-nodes/src/math.rs`).
    ///
    /// Observed-tool evidence: the convention is the pinned reference's rule, and
    /// this reader has not verified it against the original executable.
    ///
    /// The raw numbers are what the reference composes, and the reference also
    /// asserts each component lies in `[-π, π]`, so the store's unit is radians.
    /// This function therefore composes the **raw** numbers and nothing else: it
    /// is the reference's own check, useful for reporting what a container
    /// holds, and a conversion whose declared source uses a different angle unit
    /// has to make its own precedence decision in *that* unit rather than reuse
    /// this one.
    #[must_use]
    pub fn euler_matrix(&self) -> [[f32; 3]; 3] {
        let [x, y, z] = self.rotation.map(|angle| -angle);
        let (sin_x, cos_x) = x.sin_cos();
        let (sin_y, cos_y) = y.sin_cos();
        let (sin_z, cos_z) = z.sin_cos();
        [
            [
                cos_y * cos_z,
                sin_x * sin_y * cos_z - cos_x * sin_z,
                cos_x * sin_y * cos_z + sin_x * sin_z,
            ],
            [
                cos_y * sin_z,
                sin_x * sin_y * sin_z + cos_x * cos_z,
                cos_x * sin_y * sin_z - sin_x * cos_z,
            ],
            [-sin_y, sin_x * cos_y, cos_x * cos_y],
        ]
    }

    /// Whether the stored `matrix` differs from the one the record's own euler
    /// triple derives by more than [`MATRIX_AGREEMENT_TOLERANCE`] in any
    /// entry.
    ///
    /// A `true` here is why [`RawObject3dData::matrix`] has to win over the
    /// euler triple downstream: the two are different transforms and the
    /// stored one is what the file holds.
    #[must_use]
    pub fn matrix_disagrees(&self) -> bool {
        let derived = self.euler_matrix();
        (0..3).any(|row| {
            (0..3).any(|col| {
                (self.matrix[row][col] - derived[row][col]).abs() > MATRIX_AGREEMENT_TOLERANCE
            })
        })
    }
}

/// One LOD node's own 92-byte record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawLodData {
    /// `level`: the reference reads it as a boolean and never interprets which
    /// value means what.
    pub level: u32,
    /// `range_near_sq`: the **near bound stored squared**. The near distance is
    /// its square root; the record does not store the distance.
    pub range_near_sq: f32,
    /// `range_far`: the far distance, stored once.
    pub range_far: f32,
    /// `range_far_sq`: the far bound stored a second time, squared. The
    /// reference asserts it equals `range_far * range_far`; whether that
    /// consistency check holds in the corpus is a finding, not a guarantee.
    pub range_far_sq: f32,
    /// `unk64`: unmeasured. The reference asserts `unk68` is its square.
    pub unk64: f32,
    /// `unk68`: unmeasured.
    pub unk68: f32,
    /// `unk72`: unmeasured. The reference asserts `unk76` is its square.
    pub unk72: f32,
    /// `unk76`: unmeasured.
    pub unk76: f32,
}

impl RawLodData {
    /// The near distance, resolved from [`Self::range_near_sq`].
    ///
    /// A negative stored square has no real root; `None` says so instead of
    /// producing a NaN a caller would have to recognise.
    #[must_use]
    pub fn range_min(&self) -> Option<f32> {
        if self.range_near_sq < 0.0 {
            None
        } else {
            Some(self.range_near_sq.sqrt())
        }
    }

    /// Whether the record's own `range_far_sq` is the square of its
    /// `range_far`, within [`MATRIX_AGREEMENT_TOLERANCE`].
    #[must_use]
    pub fn far_square_is_consistent(&self) -> bool {
        let expected = self.range_far * self.range_far;
        (self.range_far_sq - expected).abs() <= MATRIX_AGREEMENT_TOLERANCE * expected.abs().max(1.0)
    }
}

/// The size-determining part of one world node's own record.
///
/// The world's record is the only **variable-length** node data: after its
/// 204-byte header and one `u32`, it stores a `partition_x_count` ×
/// `partition_y_count` grid of 88-byte partition records, each followed by its
/// own `count` × 12-byte values, and only then the child slots. Those two
/// counts are what make the block knowable, so they are what is decoded here.
///
/// The world's *content* — its area, fog, partitions and per-partition node
/// lists — is F18's subject and is **not** interpreted. It stays addressable:
/// [`RawNode::data_offset`] and [`RawNode::data_bytes`] name the exact bytes,
/// so a stage that owns the world record re-derives its fields from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawWorldData {
    /// `virt_partition_x_count`: grid cells along the first axis.
    pub partition_x_count: u32,
    /// `virt_partition_y_count`: grid cells along the second axis.
    pub partition_y_count: u32,
    /// How many bytes the whole partition block occupies, cells and their
    /// values together.
    pub partition_bytes: u64,
    /// How many `PartitionValue` records the block holds in total.
    pub partition_values: u64,
}

/// The kind a stored node declares, with the part of its own record this stage
/// interprets.
///
/// The variants mirror the reference's `NodeType` tags one for one. A variant
/// with no payload (camera, window, display, light) means exactly that: the
/// record is read — its length is what advances the walk and its bytes stay
/// addressable — but none of its fields has a measured meaning this stage
/// needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeKind {
    /// `NodeType::World`: the shared world container.
    World(RawWorldData),
    /// `NodeType::Camera`.
    Camera,
    /// `NodeType::Window`.
    Window,
    /// `NodeType::Display`.
    Display,
    /// `NodeType::Light`.
    Light,
    /// `NodeType::Object3d`: an object node with the authored transform.
    Object3d(RawObject3dData),
    /// `NodeType::LoD`: one LOD variant selector.
    Lod(RawLodData),
}

impl NodeKind {
    /// The stored `node_type` tag this kind came from.
    #[must_use]
    pub const fn tag(self) -> u32 {
        match self {
            Self::World(_) => NODE_TYPE_WORLD,
            Self::Camera => NODE_TYPE_CAMERA,
            Self::Window => NODE_TYPE_WINDOW,
            Self::Display => NODE_TYPE_DISPLAY,
            Self::Light => NODE_TYPE_LIGHT,
            Self::Object3d(_) => NODE_TYPE_OBJECT3D,
            Self::Lod(_) => NODE_TYPE_LOD,
        }
    }

    /// The short label used in findings and diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::World(_) => "world",
            Self::Camera => "camera",
            Self::Window => "window",
            Self::Display => "display",
            Self::Light => "light",
            Self::Object3d(_) => "object3d",
            Self::Lod(_) => "lod",
        }
    }

    /// The kind a stored tag names.
    ///
    /// # Errors
    ///
    /// `None` for a tag that is not one of the seven the CS layout defines —
    /// including [`NODE_TYPE_EMPTY`], which the reference asserts never occurs
    /// in a CS container.
    #[must_use]
    pub const fn from_tag(tag: u32) -> Option<Self> {
        Some(match tag {
            NODE_TYPE_WORLD => Self::World(RawWorldData {
                partition_x_count: 0,
                partition_y_count: 0,
                partition_bytes: 0,
                partition_values: 0,
            }),
            NODE_TYPE_CAMERA => Self::Camera,
            NODE_TYPE_WINDOW => Self::Window,
            NODE_TYPE_DISPLAY => Self::Display,
            NODE_TYPE_LIGHT => Self::Light,
            NODE_TYPE_OBJECT3D => Self::Object3d(RawObject3dData {
                flags: 0,
                rotation: [0.0; 3],
                scale: [0.0; 3],
                matrix: [[0.0; 3]; 3],
                translation: [0.0; 3],
            }),
            NODE_TYPE_LOD => Self::Lod(RawLodData {
                level: 0,
                range_near_sq: 0.0,
                range_far: 0.0,
                range_far_sq: 0.0,
                unk64: 0.0,
                unk68: 0.0,
                unk72: 0.0,
                unk76: 0.0,
            }),
            _ => return None,
        })
    }
}

/// One stored node record, decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct RawNode {
    /// The node's slot in the info array, in stored order. This is the array
    /// position every parent and child slot refers to.
    pub index: u32,
    /// The authored display name: the stored 36-byte name field up to its first
    /// `NUL`.
    ///
    /// The bytes **after** that `NUL` are not padding: in every measured
    /// record they hold a shifted remainder of the reference's
    /// `Default_node_name` write buffer, so a name is *only* the prefix and a
    /// reader that checks the padding would refuse every real record. The
    /// prefix is required to be ASCII, which is the reference's own rule.
    pub name: String,
    /// The trailing 4-byte `node_index` word, unchanged. Its top byte is
    /// asserted to be [`NODE_INDEX_TOP`] and its low three bytes are the
    /// node's own index in the engine's id space — a **different** space from
    /// [`Self::index`], and not what parent/child slots refer to.
    pub node_index: u32,
    /// The whole 208-byte info record, unmeasured words included.
    pub info: RawNodeInfo,
    /// The kind tag's interpreted payload.
    pub kind: NodeKind,
    /// Absolute container offset of this node's own data record.
    pub data_offset: u32,
    /// How many bytes this node's own data record occupies, including its
    /// parent and child slots.
    pub data_bytes: u64,
    /// The parent slot, when the record has one. An index into the info array.
    pub parent: Option<u32>,
    /// The child slots, in stored order. Indices into the info array.
    pub children: Vec<u32>,
}

impl RawNode {
    /// The stored node flag word, uninterpreted.
    #[must_use]
    pub const fn flags(&self) -> u32 {
        self.info.flags
    }

    /// The stored zone id, uninterpreted.
    #[must_use]
    pub const fn zone_id(&self) -> u32 {
        self.info.zone_id
    }

    /// The stored `mesh_index`, signed: a non-negative value is the mesh-array
    /// slot this node associates and `-1` means "no mesh".
    #[must_use]
    pub const fn mesh_index(&self) -> i32 {
        self.info.mesh_index
    }

    /// The node's own `node_index`, with the top byte masked off.
    #[must_use]
    pub const fn engine_index(&self) -> u32 {
        self.node_index & NODE_INDEX_BOT_MASK
    }

    /// The object record's authored transform, when this is an object node.
    #[must_use]
    pub const fn object3d(&self) -> Option<RawObject3dData> {
        match self.kind {
            NodeKind::Object3d(data) => Some(data),
            _ => None,
        }
    }

    /// The LOD record, when this is a LOD node.
    #[must_use]
    pub const fn lod(&self) -> Option<RawLodData> {
        match self.kind {
            NodeKind::Lod(data) => Some(data),
            _ => None,
        }
    }

    /// Whether this node declares no parent, which makes it a root of the
    /// stored forest.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}

/// Something the node array can read but the reference asserts does not occur.
///
/// A finding is **not** an error: the record's stored lengths are still well
/// defined, so the node is read with everything it stores. The finding exists
/// so a caller can see that the container is outside the reference's asserted
/// profile instead of believing it was inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeFinding {
    /// The trailing `node_index` word's top byte is not
    /// [`NODE_INDEX_TOP`], which the reference asserts. Nothing this stage
    /// reads depends on that byte, so the node is still read.
    NodeIndexTopBits {
        /// Node array index.
        node: u32,
        /// The word as stored.
        found: u32,
    },
    /// An object record's `flags` is neither [`OBJECT3D_FLAGS_TRANSFORMED`] nor
    /// [`OBJECT3D_FLAGS_IDENTITY`], which the reference asserts.
    ObjectFlags {
        /// Node array index.
        node: u32,
        /// The stored value.
        found: u32,
    },
    /// An object record stores a `matrix` that its own euler triple does not
    /// derive, beyond [`MATRIX_AGREEMENT_TOLERANCE`]. Both are kept: the
    /// stored matrix is what the file holds and takes precedence.
    ObjectMatrixDisagrees {
        /// Node array index.
        node: u32,
    },
    /// A `flags == 40` object record is **not** exactly the identity transform,
    /// which the reference asserts.
    ObjectIdentityNotIdentity {
        /// Node array index.
        node: u32,
    },
    /// A LOD record's `range_far_sq` is not the square of its `range_far`,
    /// which the reference asserts.
    LodFarSquare {
        /// Node array index.
        node: u32,
    },
    /// A LOD record's `range_near_sq` is negative, so it has no real square
    /// root and the near distance does not exist.
    LodNearSquareNegative {
        /// Node array index.
        node: u32,
    },
    /// A LOD record's `level` is neither `0` nor `1`, which the reference reads
    /// as a boolean.
    LodLevel {
        /// Node array index.
        node: u32,
        /// The stored value.
        found: u32,
    },
    /// An object or LOD record stores `unk196 != 160`, which the reference
    /// asserts for both kinds.
    NodeField196 {
        /// Node array index.
        node: u32,
        /// The stored value.
        found: u32,
    },
    /// A record's `parent_count` is above one, which the reference reads as a
    /// boolean and would refuse.
    ParentCount {
        /// Node array index.
        node: u32,
        /// The stored value.
        found: u16,
    },
    /// A world record's own `children_count` field is not one, which the
    /// reference asserts. It is a different field from the info record's
    /// `children_count`, which is what the child slots follow.
    WorldChildrenCount {
        /// Node array index.
        node: u32,
        /// The stored value.
        found: u32,
    },
    /// A node's `mesh_index` is negative but not `-1`, so it names no mesh and
    /// is not the documented "no mesh" sentinel either.
    MeshIndexSentinel {
        /// Node array index.
        node: u32,
        /// The stored value.
        found: i32,
    },
}

impl NodeFinding {
    /// Stable lowercase identifier for logs and structured diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NodeIndexTopBits { .. } => "node_index_top_bits",
            Self::ObjectFlags { .. } => "object_flags",
            Self::ObjectMatrixDisagrees { .. } => "object_matrix_disagrees",
            Self::ObjectIdentityNotIdentity { .. } => "object_identity_not_identity",
            Self::LodFarSquare { .. } => "lod_far_square",
            Self::LodNearSquareNegative { .. } => "lod_near_square_negative",
            Self::LodLevel { .. } => "lod_level",
            Self::NodeField196 { .. } => "node_field_196",
            Self::ParentCount { .. } => "parent_count",
            Self::WorldChildrenCount { .. } => "world_children_count",
            Self::MeshIndexSentinel { .. } => "mesh_index_sentinel",
        }
    }

    /// Node array index the finding is about.
    #[must_use]
    pub const fn node(&self) -> u32 {
        match self {
            Self::NodeIndexTopBits { node, .. }
            | Self::ObjectFlags { node, .. }
            | Self::ObjectMatrixDisagrees { node }
            | Self::ObjectIdentityNotIdentity { node }
            | Self::LodFarSquare { node }
            | Self::LodNearSquareNegative { node }
            | Self::LodLevel { node, .. }
            | Self::NodeField196 { node, .. }
            | Self::ParentCount { node, .. }
            | Self::WorldChildrenCount { node, .. }
            | Self::MeshIndexSentinel { node, .. } => *node,
        }
    }

    /// Evidence class of a finding: `ObservedTool` at best, never better, since
    /// a finding is by definition something the documented profile did not
    /// cover.
    #[must_use]
    pub const fn evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }
}

impl fmt::Display for NodeFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::NodeIndexTopBits { found, .. } => write!(
                f,
                "{code}: node_index is 0x{found:08X}, whose top byte is not 0x{NODE_INDEX_TOP:08X}"
            ),
            Self::ObjectFlags { found, .. } => {
                write!(f, "{code}: object record stores flags {found}")
            }
            Self::ObjectMatrixDisagrees { .. } => {
                write!(
                    f,
                    "{code}: the stored matrix differs from the stored euler triple"
                )
            }
            Self::ObjectIdentityNotIdentity { .. } => {
                write!(
                    f,
                    "{code}: an identity object record is not the identity transform"
                )
            }
            Self::LodFarSquare { .. } => {
                write!(f, "{code}: range_far_sq is not the square of range_far")
            }
            Self::LodNearSquareNegative { .. } => {
                write!(
                    f,
                    "{code}: range_near_sq is negative, so it has no real root"
                )
            }
            Self::LodLevel { found, .. } => {
                write!(
                    f,
                    "{code}: LOD level stores {found}, which is not a boolean"
                )
            }
            Self::NodeField196 { found, .. } => {
                write!(f, "{code}: record stores field 196 as {found}")
            }
            Self::ParentCount { found, .. } => {
                write!(
                    f,
                    "{code}: parent_count stores {found}, which is read as a boolean"
                )
            }
            Self::WorldChildrenCount { found, .. } => {
                write!(
                    f,
                    "{code}: the world record's own children count stores {found}"
                )
            }
            Self::MeshIndexSentinel { found, .. } => {
                write!(
                    f,
                    "{code}: mesh_index stores {found}, which is neither -1 nor an index"
                )
            }
        }
    }
}

/// The smallest and largest non-negative `mesh_index` any node in this
/// container stores, plus how many nodes store one at all.
///
/// The node reader does not hold the mesh section, so it cannot range-check an
/// index; this is the honest substitute: it states which slots the nodes name
/// so a caller holding the mesh array can check them itself. The check itself is
/// [`super::bindings::NodeMeshBindings::of`], which is handed this section **and**
/// the mesh array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshIndexBounds {
    /// How many nodes store a non-negative `mesh_index`.
    pub bound: usize,
    /// The smallest non-negative index stored, or `None` when none is.
    pub min: Option<u32>,
    /// The largest non-negative index stored, or `None` when none is.
    pub max: Option<u32>,
}

/// The decoded node array of one CS GameZ container.
#[derive(Debug, Clone, PartialEq)]
pub struct GameZNodes {
    /// The 40-byte container header, read by this reader so the node array is
    /// located and gated by the same words every other section is.
    pub header: GameZHeader,
    /// One record per array slot, in stored order.
    pub nodes: Vec<RawNode>,
    /// Where the info array starts: the header's `nodes_offset`.
    pub info_offset: u64,
    /// Where the info array ends, exclusive: `info_offset + 212 · node_array_size`.
    pub info_end: u64,
    /// Where the data section starts. Equal to [`Self::info_end`], which is the
    /// check that the whole fixed-stride info array was read and nothing was
    /// skipped.
    pub data_offset: u64,
    /// Where the data section ended, exclusive. Equal to the container length,
    /// which is the check that the whole variable-length section was read.
    pub data_end: u64,
    /// Everything this reader read that the reference's assertions do not
    /// allow, in stored order. An empty list means every stored record was
    /// inside the reference's asserted profile, which is the strongest claim
    /// this reader makes about a container.
    pub findings: Vec<NodeFinding>,
}

impl GameZNodes {
    /// One node by array index — the lookup a parent or child slot performs.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&RawNode> {
        self.nodes.get(index as usize)
    }

    /// The nodes that declare no parent, in stored order: the roots of the
    /// stored forest.
    pub fn roots(&self) -> impl Iterator<Item = &RawNode> {
        self.nodes.iter().filter(|node| node.is_root())
    }

    /// Every node of one kind, in stored order.
    pub fn of_kind(&self, kind: u32) -> impl Iterator<Item = &RawNode> {
        self.nodes
            .iter()
            .filter(move |node| node.kind.tag() == kind)
    }

    /// Which mesh-array slots the nodes of this container name.
    #[must_use]
    pub fn mesh_index_bounds(&self) -> MeshIndexBounds {
        let mut bounds = MeshIndexBounds {
            bound: 0,
            min: None,
            max: None,
        };
        for node in &self.nodes {
            let Ok(index) = u32::try_from(node.mesh_index()) else {
                continue;
            };
            bounds.bound += 1;
            bounds.min = Some(bounds.min.map_or(index, |min: u32| min.min(index)));
            bounds.max = Some(bounds.max.map_or(index, |max: u32| max.max(index)));
        }
        bounds
    }

    /// Evidence class of the layout this reader implements: documented in the
    /// pinned reference *and* measured against the original installation, which
    /// is `ObservedTool`. Never `VerifiedOriginal` here — that needs an
    /// original run, which has not happened.
    #[must_use]
    pub const fn layout_evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }
}

/// Why the node array of a CS GameZ container could not be read.
///
/// Each variant carries counts, offsets and node indices only, never archive
/// bytes, and every offset is the absolute container offset the failure is
/// anchored at.
#[derive(Debug, Clone, PartialEq)]
pub enum GameZNodeError {
    /// A read ran past the end of the container, a stored count was refused by
    /// one of the parse's budgets, or a name field was not decodable: a
    /// [`ParseError`] already scoped as `gamez.nodes.<field>`.
    Parse(ParseError),
    /// The shared 40-byte container header was refused, so the node array
    /// cannot be located. The inner error is the mesh section's own
    /// [`GameZError`](super::reader::GameZError), verbatim.
    Header(super::reader::GameZError),
    /// `nodes_offset` is past the end of the container, so the node array
    /// cannot start inside it.
    NodesOffsetOutOfBounds {
        /// The header's `nodes_offset`.
        offset: u32,
        /// Bytes the container has.
        container_len: u64,
    },
    /// `node_array_size` is zero: the array would be empty, and an empty
    /// section is a container this layout does not describe.
    NodeArrayEmpty,
    /// The info array does not fit before the end of the container.
    NodeArrayOutOfBounds {
        /// Where the info array would end.
        end: u64,
        /// Bytes the container has.
        container_len: u64,
    },
    /// A record's `node_type` is not one of the seven the CS layout defines.
    NodeType {
        /// Node array index.
        node: u32,
        /// Offset of the `node_type` word.
        offset: u64,
        /// The value found.
        found: u32,
    },
    /// A record's stored `data_ptr` is not the offset the sequential walk
    /// reached, so the data section is not where the record says it is.
    DataOffset {
        /// Node array index.
        node: u32,
        /// Offset the record declares.
        declared: u32,
        /// Offset the sequential walk reached.
        walked: u64,
    },
    /// A record's parent slot is not an index into the node array.
    ParentSlot {
        /// Node array index.
        node: u32,
        /// The stored value.
        found: u32,
        /// How many nodes the container holds.
        count: u32,
    },
    /// A record's child slot is not an index into the node array.
    ChildSlot {
        /// Node array index.
        node: u32,
        /// Position of the child in the stored child list.
        position: u16,
        /// The stored value.
        found: u32,
        /// How many nodes the container holds.
        count: u32,
    },
    /// A world record's partition grid is larger than the remaining data
    /// section, so its variable-length block cannot be where the record says.
    PartitionGrid {
        /// Node array index.
        node: u32,
        /// `partition_x_count * partition_y_count`, which overflowed or does not
        /// fit.
        cells: u64,
        /// How many bytes the data section has left.
        available: u64,
    },
    /// The data section did not end exactly at the end of the container, so a
    /// record was read with the wrong length or one was skipped.
    DataEnd {
        /// Offset the walk reached.
        found: u64,
        /// Bytes the container has.
        expected: u64,
    },
}

impl GameZNodeError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
            Self::Header(_) => "header",
            Self::NodesOffsetOutOfBounds { .. } => "nodes_offset_out_of_bounds",
            Self::NodeArrayEmpty => "node_array_empty",
            Self::NodeArrayOutOfBounds { .. } => "node_array_out_of_bounds",
            Self::NodeType { .. } => "node_type",
            Self::DataOffset { .. } => "data_offset",
            Self::ParentSlot { .. } => "parent_slot",
            Self::ChildSlot { .. } => "child_slot",
            Self::PartitionGrid { .. } => "partition_grid",
            Self::DataEnd { .. } => "data_end",
        }
    }

    /// The container label the failure came from, when the variant carries one
    /// from the checked reader. The validation variants name no container
    /// because they are produced inside the parse whose reader holds it.
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
            Self::NodesOffsetOutOfBounds { offset, .. } => Some(u64::from(*offset)),
            Self::NodeType { offset, .. } => Some(*offset),
            Self::DataOffset { walked, .. } => Some(*walked),
            Self::DataEnd { found, .. } => Some(*found),
            Self::Parse(error) => Some(error.offset),
            Self::Header(error) => error.offset(),
            _ => None,
        }
    }
}

impl From<ParseError> for GameZNodeError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<super::reader::GameZError> for GameZNodeError {
    /// The 40-byte container header is read by the reader the mesh and
    /// material sections share, so its refusal crosses over whole rather than
    /// being re-spelled here: a signature or version failure is one condition
    /// with one set of numbers whichever section asked.
    fn from(error: super::reader::GameZError) -> Self {
        Self::Header(error)
    }
}

impl fmt::Display for GameZNodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::Header(error) => write!(f, "{error}"),
            Self::NodesOffsetOutOfBounds {
                offset,
                container_len,
            } => write!(
                f,
                "nodes_offset is {offset}, outside the {container_len}-byte container"
            ),
            Self::NodeArrayEmpty => write!(f, "the container declares an empty node array"),
            Self::NodeArrayOutOfBounds { end, container_len } => write!(
                f,
                "the node info array would end at {end}, outside the {container_len}-byte container"
            ),
            Self::NodeType {
                node,
                offset,
                found,
            } => write!(
                f,
                "at offset {offset}: node {node} declares node_type {found}, which is not a CS node type"
            ),
            Self::DataOffset {
                node,
                declared,
                walked,
            } => write!(
                f,
                "node {node} declares data offset {declared} but the walk reached {walked}"
            ),
            Self::ParentSlot { node, found, count } => write!(
                f,
                "node {node} stores parent slot {found}, outside the {count} stored nodes"
            ),
            Self::ChildSlot {
                node,
                position,
                found,
                count,
            } => write!(
                f,
                "node {node} child {position} stores slot {found}, outside the {count} stored nodes"
            ),
            Self::PartitionGrid {
                node,
                cells,
                available,
            } => write!(
                f,
                "node {node} declares a {cells}-cell partition grid, which does not fit in the \
                 {available} bytes the data section has left"
            ),
            Self::DataEnd { found, expected } => write!(
                f,
                "the node data section ended at {found}, expected the container's {expected} bytes"
            ),
        }
    }
}

impl std::error::Error for GameZNodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Header(error) => Some(error),
            _ => None,
        }
    }
}

/// Reads the node array of a CS GameZ container.
///
/// The reader re-reads the 40-byte container header, so the node array is
/// located and gated by exactly the same words the mesh and material sections
/// are, and then reads the two passes the layout defines: the fixed-stride info
/// array, and the variable-length data section in node order.
///
/// # Errors
///
/// [`GameZNodeError`] in every case: `Parse` for truncation, a refused budget
/// and an undecodable name; `NodesOffsetOutOfBounds` / `NodeArrayEmpty` /
/// `NodeArrayOutOfBounds` when the array cannot be where the header says;
/// `NodeType` for a record whose kind tag is not a CS node type; `DataOffset`
/// when a record's stored pointer is not the offset the walk reached;
/// `ParentSlot` / `ChildSlot` for a hierarchy link that is not an index into
/// this array; `PartitionGrid` for a world grid that cannot fit; and `DataEnd`
/// when the data section did not end at the container's end.
pub fn read_gamez_nodes(
    context: &mut ParseContext,
    bytes: &[u8],
) -> Result<GameZNodes, GameZNodeError> {
    // The container label is the parse's own: `ParseContext::new` takes it, and
    // every reader error inherits it. A reader must not carry a second,
    // possibly different one.
    match context.parse(
        NODES_ENTRYPOINT,
        bytes,
        |reader, allocation, _recursion| match read_nodes(reader, allocation, bytes.len() as u64) {
            Ok(value) => Ok(Ok(value)),
            Err(GameZNodeError::Parse(error)) => Err(error),
            Err(domain) => Ok(Err(domain)),
        },
    ) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(domain)) => Err(domain),
        Err(error) => Err(GameZNodeError::Parse(error)),
    }
}

/// The info array's declared byte length, or a refusal of a count that cannot
/// produce one.
fn info_array_bytes(count: u32) -> Result<u64, GameZNodeError> {
    if count == 0 {
        return Err(GameZNodeError::NodeArrayEmpty);
    }
    u64::from(count)
        .checked_mul(NODE_SLOT_BYTES)
        .ok_or(GameZNodeError::NodeArrayEmpty)
}

/// One entry of the info array, before the data section is walked.
#[derive(Debug, Clone)]
struct NodeSlot {
    /// Array index of this slot.
    index: u32,
    /// The authored name, decoded.
    name: String,
    /// The 208-byte record.
    info: RawNodeInfo,
    /// The 4-byte word after it.
    node_index: u32,
}

fn read_nodes(
    reader: &mut Reader<'_>,
    allocation: &mut AllocationBudget,
    container_len: u64,
) -> Result<GameZNodes, GameZNodeError> {
    let header = read_container_header(reader, container_len)?;
    let info_offset = u64::from(header.nodes_offset);
    if info_offset >= container_len {
        return Err(GameZNodeError::NodesOffsetOutOfBounds {
            offset: header.nodes_offset,
            container_len,
        });
    }
    let info_bytes = info_array_bytes(header.node_array_size)?;
    let info_end = reader.checked_extent("node.info_array", info_offset, info_bytes)?;
    if info_end > container_len {
        return Err(GameZNodeError::NodeArrayOutOfBounds {
            end: info_end,
            container_len,
        });
    }
    allocation.reserve(
        "node.info_array",
        info_offset,
        u64::from(header.node_array_size),
        size_of::<NodeSlot>() as u64,
    )?;

    // Pass one: the fixed-stride info array, opened as its own window so the
    // mesh and material sections behind it can never be read as a node.
    let mut window = reader.window(info_offset, info_bytes, "node.info_array")?;
    let mut slots = Vec::with_capacity(header.node_array_size as usize);
    for index in 0..header.node_array_size {
        let name = window
            .read_bounded_cstr("node.name", NODE_NAME_BYTES as usize)?
            .to_owned();
        let info = read_node_info(&mut window)?;
        let node_index = window.read_u32("node.index")?;
        if NodeKind::from_tag(info.node_type).is_none() {
            return Err(GameZNodeError::NodeType {
                node: index,
                offset: info_offset + u64::from(index) * NODE_SLOT_BYTES + NODE_TYPE_OFFSET,
                found: info.node_type,
            });
        }
        slots.push(NodeSlot {
            index,
            name,
            info,
            node_index,
        });
    }

    // Pass two: the data section, which begins exactly where the info array
    // ends. That equality is the check that pass one consumed the whole array
    // and nothing else.
    let data_offset = window.position();
    if data_offset != info_end {
        return Err(GameZNodeError::DataEnd {
            found: data_offset,
            expected: info_end,
        });
    }
    let mut data = reader.window(data_offset, container_len - data_offset, "node.data")?;

    let count = header.node_array_size;
    let mut findings = Vec::new();
    let mut nodes = Vec::with_capacity(count as usize);
    for slot in &slots {
        let walked = data.position();
        if u64::from(slot.info.data_ptr) != walked {
            return Err(GameZNodeError::DataOffset {
                node: slot.index,
                declared: slot.info.data_ptr,
                walked,
            });
        }
        let record = read_node_data(&mut data, slot, count, &mut findings)?;
        nodes.push(record);
    }
    let data_end = data.position();
    if data_end != container_len {
        return Err(GameZNodeError::DataEnd {
            found: data_end,
            expected: container_len,
        });
    }

    Ok(GameZNodes {
        header,
        nodes,
        info_offset,
        info_end,
        data_offset,
        data_end,
        findings,
    })
}

/// Reads the 208-byte info record behind a name that was already consumed.
fn read_node_info(reader: &mut Reader<'_>) -> Result<RawNodeInfo, ParseError> {
    Ok(RawNodeInfo {
        flags: reader.read_u32("node.flags")?,
        unk040: reader.read_u32("node.unk040")?,
        unk044: reader.read_u32("node.unk044")?,
        zone_id: reader.read_u32("node.zone_id")?,
        node_type: reader.read_u32("node.node_type")?,
        data_ptr: reader.read_u32("node.data_ptr")?,
        mesh_index: reader.read_i32("node.mesh_index")?,
        environment_data: reader.read_u32("node.environment_data")?,
        action_priority: reader.read_u32("node.action_priority")?,
        action_callback: reader.read_u32("node.action_callback")?,
        area_partition: [
            reader.read_i16("node.area_partition.x")?,
            reader.read_i16("node.area_partition.y")?,
            reader.read_i16("node.area_partition.virtual_x")?,
            reader.read_i16("node.area_partition.virtual_y")?,
        ],
        parent_count: reader.read_u16("node.parent_count")?,
        children_count: reader.read_u16("node.children_count")?,
        parent_array_ptr: reader.read_u32("node.parent_array_ptr")?,
        children_array_ptr: reader.read_u32("node.children_array_ptr")?,
        unk096: reader.read_u32("node.unk096")?,
        unk100: reader.read_u32("node.unk100")?,
        unk104: reader.read_u32("node.unk104")?,
        unk108: reader.read_u32("node.unk108")?,
        unk112: reader.read_u32("node.unk112")?,
        unk116: [read_vec3(reader)?, read_vec3(reader)?],
        unk140: [read_vec3(reader)?, read_vec3(reader)?],
        unk164: [read_vec3(reader)?, read_vec3(reader)?],
        unk188: reader.read_u32("node.unk188")?,
        unk192: reader.read_u32("node.unk192")?,
        unk196: reader.read_u32("node.unk196")?,
        unk200: reader.read_u32("node.unk200")?,
        unk204: reader.read_u32("node.unk204")?,
    })
}

fn read_vec3(reader: &mut Reader<'_>) -> Result<[f32; 3], ParseError> {
    Ok([
        reader.read_f32("vec3.x")?,
        reader.read_f32("vec3.y")?,
        reader.read_f32("vec3.z")?,
    ])
}

/// Reads one node's own data record, its parent slot and its child slots.
fn read_node_data(
    data: &mut Reader<'_>,
    slot: &NodeSlot,
    count: u32,
    findings: &mut Vec<NodeFinding>,
) -> Result<RawNode, GameZNodeError> {
    let index = slot.index;
    if slot.node_index & NODE_INDEX_TOP_MASK != NODE_INDEX_TOP {
        findings.push(NodeFinding::NodeIndexTopBits {
            node: index,
            found: slot.node_index,
        });
    }
    if slot.info.mesh_index < -1 {
        findings.push(NodeFinding::MeshIndexSentinel {
            node: index,
            found: slot.info.mesh_index,
        });
    }
    if slot.info.parent_count > 1 {
        findings.push(NodeFinding::ParentCount {
            node: index,
            found: slot.info.parent_count,
        });
    }

    let start = data.position();
    // The parent **word**'s presence is `parent_count != 0` for most kinds, and
    // unconditional for the two kinds the reference reads it without consulting
    // the boolean: a LOD variant cannot stand alone (and the reference asserts
    // the boolean is set for it), and a light node's word is read "as a result
    // of parent_count" even though the reference asserts the boolean is clear
    // for it. The word still has to be consumed for a light node, or the walk
    // desynchronises.
    let kind_tag = slot.info.node_type;
    let has_parent_word =
        slot.info.has_parent() || matches!(kind_tag, NODE_TYPE_LOD | NODE_TYPE_LIGHT);
    let kind = match kind_tag {
        NODE_TYPE_OBJECT3D => {
            let object = read_object3d(data)?;
            check_object3d(index, &object, findings);
            NodeKind::Object3d(object)
        }
        NODE_TYPE_LOD => {
            let lod = read_lod(data)?;
            check_lod(index, &lod, findings);
            NodeKind::Lod(lod)
        }
        NODE_TYPE_WORLD => {
            let world = read_world(data, index, findings)?;
            NodeKind::World(world)
        }
        NODE_TYPE_CAMERA => {
            data.skip("node.camera", CAMERA_DATA_BYTES as usize)?;
            NodeKind::Camera
        }
        NODE_TYPE_WINDOW => {
            data.skip("node.window", WINDOW_DATA_BYTES as usize)?;
            NodeKind::Window
        }
        NODE_TYPE_DISPLAY => {
            data.skip("node.display", DISPLAY_DATA_BYTES as usize)?;
            NodeKind::Display
        }
        NODE_TYPE_LIGHT => {
            data.skip("node.light", LIGHT_DATA_BYTES as usize)?;
            NodeKind::Light
        }
        other => {
            return Err(GameZNodeError::NodeType {
                node: index,
                offset: start,
                found: other,
            });
        }
    };
    if matches!(kind, NodeKind::Object3d(_) | NodeKind::Lod(_)) && slot.info.unk196 != 160 {
        findings.push(NodeFinding::NodeField196 {
            node: index,
            found: slot.info.unk196,
        });
    }

    // The **declared** linkage is the record's own `parent_count` boolean,
    // which is what the reference's own model uses to decide whether a node has
    // a parent. A light node's word is still range-checked when it is read, so
    // a stored index that names no node is reported rather than dropped.
    let parent = if has_parent_word {
        let stored = data.read_u32("node.parent")?;
        if stored >= count {
            return Err(GameZNodeError::ParentSlot {
                node: index,
                found: stored,
                count,
            });
        }
        slot.info.has_parent().then_some(stored)
    } else {
        None
    };
    let children = read_children(data, index, slot.info.children_count, count)?;

    Ok(RawNode {
        index,
        name: slot.name.clone(),
        node_index: slot.node_index,
        info: slot.info,
        kind,
        data_offset: start as u32,
        data_bytes: data.position() - start,
        parent,
        children,
    })
}

fn read_children(
    data: &mut Reader<'_>,
    node: u32,
    children_count: u16,
    count: u32,
) -> Result<Vec<u32>, GameZNodeError> {
    if children_count == 0 {
        return Ok(Vec::new());
    }
    let mut children = Vec::with_capacity(children_count as usize);
    for position in 0..children_count {
        let stored = data.read_u32("node.child")?;
        if stored >= count {
            return Err(GameZNodeError::ChildSlot {
                node,
                position,
                found: stored,
                count,
            });
        }
        children.push(stored);
    }
    Ok(children)
}

fn read_object3d(data: &mut Reader<'_>) -> Result<RawObject3dData, ParseError> {
    let flags = data.read_u32("object.flags")?;
    // `opacity` and the four words after it: the reference asserts each is zero
    // and none has a measured meaning, so they are stepped over rather than
    // carried.
    data.skip("object.unmeasured", 20)?;
    let rotation = read_vec3(data)?;
    let scale = read_vec3(data)?;
    let matrix = [read_vec3(data)?, read_vec3(data)?, read_vec3(data)?];
    let translation = read_vec3(data)?;
    // The trailing 48 zero bytes the reference asserts are part of the record,
    // so they are stepped over: the record is 144 bytes, not 96.
    data.skip("object.unmeasured_tail", 48)?;
    Ok(RawObject3dData {
        flags,
        rotation,
        scale,
        matrix,
        translation,
    })
}

fn read_lod(data: &mut Reader<'_>) -> Result<RawLodData, ParseError> {
    let level = data.read_u32("lod.level")?;
    let range_near_sq = data.read_f32("lod.range_near_sq")?;
    let range_far = data.read_f32("lod.range_far")?;
    let range_far_sq = data.read_f32("lod.range_far_sq")?;
    // The 48 bytes the reference asserts are zero, then the two squared pairs
    // it carries without interpreting.
    data.skip("lod.unmeasured", 48)?;
    let record = RawLodData {
        level,
        range_near_sq,
        range_far,
        range_far_sq,
        unk64: data.read_f32("lod.unk64")?,
        unk68: data.read_f32("lod.unk68")?,
        unk72: data.read_f32("lod.unk72")?,
        unk76: data.read_f32("lod.unk76")?,
    };
    // `one80`, `unk84` and `unk88`: three words the reference asserts and never
    // interprets. They are part of the 92-byte record, so they are stepped
    // over rather than carried.
    data.skip("lod.unmeasured_tail", 12)?;
    Ok(record)
}

fn read_world(
    data: &mut Reader<'_>,
    node: u32,
    findings: &mut Vec<NodeFinding>,
) -> Result<RawWorldData, GameZNodeError> {
    // The two grid counts sit inside the fixed 204-byte header, and they are
    // the only fields of it this stage needs: everything else in the world
    // record is F18's subject, so the header is stepped over rather than
    // decoded field by field.
    data.skip("world.header", 152)?;
    let partition_x_count = data.read_u32("world.partition_x_count")?;
    let partition_y_count = data.read_u32("world.partition_y_count")?;
    // The reference asserts this record's *own* children count field is one.
    // It is a different field from the info record's `children_count`, which is
    // what the child slots follow, so it is checked as a finding and not used
    // to size anything.
    data.skip("world.header", 16)?;
    let own_children = data.read_u32("world.children_count")?;
    if own_children != 1 {
        findings.push(NodeFinding::WorldChildrenCount {
            node,
            found: own_children,
        });
    }
    // The rest of the 204-byte header, then the one word the reference reads as
    // the world's single child value, whose meaning is unmeasured. The
    // partition grid starts after it.
    data.skip("world.header_tail", 24)?;
    data.skip("world.child_value", 4)?;

    let cells = u64::from(partition_x_count).saturating_mul(u64::from(partition_y_count));
    let available = data.remaining() as u64;
    if cells
        .checked_mul(WORLD_PARTITION_BYTES)
        .is_none_or(|block| block > available)
    {
        return Err(GameZNodeError::PartitionGrid {
            node,
            cells,
            available,
        });
    }
    let mut partition_bytes = 0u64;
    let mut partition_values = 0u64;
    for _ in 0..cells {
        // The per-cell `count` is the eighth `u16` of the 88-byte record, not
        // its first word, so the cell is opened rather than assumed. Its values
        // follow the cell, so a cell whose own count does not fit what is left
        // of the data section is refused with the grid's own reason instead of
        // surfacing as a bare truncation.
        data.skip("world.partition", 58)?;
        let count = u64::from(data.read_u16("world.partition.count")?);
        data.skip("world.partition", (WORLD_PARTITION_BYTES - 60) as usize)?;
        let values = count
            .checked_mul(WORLD_PARTITION_VALUE_BYTES)
            .filter(|values| *values <= data.remaining() as u64)
            .ok_or(GameZNodeError::PartitionGrid {
                node,
                cells,
                available,
            })?;
        data.skip("world.partition.values", values as usize)?;
        partition_bytes += WORLD_PARTITION_BYTES + values;
        partition_values += count;
    }
    Ok(RawWorldData {
        partition_x_count,
        partition_y_count,
        partition_bytes,
        partition_values,
    })
}

fn check_object3d(node: u32, object: &RawObject3dData, findings: &mut Vec<NodeFinding>) {
    if object.flags != OBJECT3D_FLAGS_TRANSFORMED && object.flags != OBJECT3D_FLAGS_IDENTITY {
        findings.push(NodeFinding::ObjectFlags {
            node,
            found: object.flags,
        });
    }
    if object.stores_identity() {
        let identity = object.rotation == [0.0; 3]
            && object.translation == [0.0; 3]
            && object.matrix == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        if !identity {
            findings.push(NodeFinding::ObjectIdentityNotIdentity { node });
        }
        return;
    }
    if object.matrix_disagrees() {
        findings.push(NodeFinding::ObjectMatrixDisagrees { node });
    }
}

fn check_lod(node: u32, lod: &RawLodData, findings: &mut Vec<NodeFinding>) {
    if lod.level > 1 {
        findings.push(NodeFinding::LodLevel {
            node,
            found: lod.level,
        });
    }
    if lod.range_near_sq < 0.0 {
        findings.push(NodeFinding::LodNearSquareNegative { node });
    }
    if !lod.far_square_is_consistent() {
        findings.push(NodeFinding::LodFarSquare { node });
    }
}
