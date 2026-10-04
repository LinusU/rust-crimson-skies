//! ZBD family inventory, dispatch and the reader/sound container subset
//! (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`, stages
//! `### F06-A` and `### F06-B`).
//!
//! "ZBD" is a family label, not one file layout: the sound, reader,
//! texture, interp, GameZ and animation containers are distinct families
//! with distinct readers. No installation bytes were read for these stages
//! (ordinary build/test capability only):
//!
//! * [`family`] is the inventory: one row per family with its reader slot,
//!   its header rule and the observed archive names that identify it.
//! * [`header`] holds the documented header rules — today exactly the
//!   INTERP signature/version — and an explicit "undocumented" rule for
//!   every family whose layout is unknown.
//! * [`role`] resolves the installation role of a path from the observed
//!   naming conventions under the `zbd/` content root.
//! * [`dispatch`] combines the two keys into a [`dispatch::ZbdDispatch`]
//!   or a machine-matchable [`dispatch::ZbdDispatchError`], never a silent
//!   fallback to another parser.
//! * [`archive`] is the bounded container layer the family readers share:
//!   declared members, checked extents, consumed and uncovered ranges, the
//!   strict status, the [`archive::RoutableFamily`] refusal that keeps a
//!   dispatched family from being named by a caller, and the
//!   [`archive::FamilyMismatch`] gate a reader puts in front of bytes of
//!   another family (stage F06-B).
//! * [`reader_archive`] and [`sound_archive`] are the two readers this
//!   stage implements (F06-B "reader and sound container subset with
//!   bounds"). They hand out each member's verbatim bytes with its source
//!   span and its evidence, and they decode nothing. Reader entries report
//!   the recorded unknown (stage F06-B); sound entries report what their
//!   RIFF/WAVE header declares (task #344).
//! * [`trailer`] reads the version-one member index at the end of a sound or
//!   reader archive into the member extents those readers list (task #343).
//! * [`anim`] is the animation family's **own** front index (task #633):
//!   `cam_anim.zbd`/`mis_anim.zbd` carry no trailer, so their two declared
//!   tables — the sibling containers the data refers to and the
//!   animation-definition sources their payload carries — are read here, and
//!   the fixed block at the front of the payload with them. The animation
//!   records themselves stay undecoded, for the reason the module states.
//! * [`wave`] reads the RIFF/WAVE header of one sound member into the
//!   descriptor [`sound_archive`] reports (task #344).
//! * [`adpcm`] reads the `fmt ` extension the two ADPCM tags carry and decodes
//!   their blocks: the layouts `wFormatTag` `0x0011` (IMA) and `0x0002`
//!   (Microsoft) name, with the per-member values the member itself declares
//!   (task #444).
//! * [`sound_sample`] decodes a member's `data` payload under the format its
//!   own WAVE header declares (stage F06-C). [`sound_sample::SampleFormat::from_header`]
//!   is that stage's plan and decodes uncompressed PCM;
//!   [`sound_sample::SampleFormat::from_member`] is task #444's plan and also
//!   decodes the two block codecs, whose geometry and codebook live in
//!   [`adpcm`]. A compressed member is never approximated and never passed
//!   through as if it were PCM.
//!
//! The evidence behind every rule, the design decisions and the recorded
//! unknowns are written down in
//! `docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md` and
//! `docs/findings/2026-09-28-f06-b-reader-and-sound-container-bounds.md`;
//! the task #340 header signatures and archive names in
//! `docs/findings/2026-09-28-t340-zbd-family-headers-and-archive-names.md`;
//! the task #343 member index in
//! `docs/findings/2026-09-28-t343-zbd-version-one-member-index.md`;
//! the task #344 WAVE headers in
//! `docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md`; and
//! stage F06-C's sample decode and VFS wiring in
//! `docs/findings/2026-09-28-f06-c-vfs-members-and-audio-assets.md`.
//! The fixtures exercised by `crates/cs_formats/tests/zbd/` are newly
//! authored synthetic bytes; nothing here is derived from original game
//! data.

pub mod adpcm;
pub mod anim;
pub mod archive;
pub mod dispatch;
pub mod family;
pub mod header;
pub mod reader_archive;
pub mod role;
pub mod sound_archive;
pub mod sound_sample;
pub mod trailer;
pub mod wave;

pub use adpcm::{
    ADAPTATION_TABLE, AdpcmError, AdpcmExtension, AdpcmLayout, IMA_BLOCK_HEADER_BYTES,
    IMA_EXTENSION_BYTES, INDEX_TABLE, MAX_MS_DELTA, MAX_STEP_INDEX, MIN_MS_DELTA,
    MS_BLOCK_HEADER_BYTES_PER_CHANNEL, MS_COEFFICIENT_BYTES, MS_COEFFICIENT_PAIRS,
    MS_EXTENSION_BYTES, MsAdpcmCoefficient, MsAdpcmCoefficients, STEP_TABLE, STEP_TABLE_ENTRIES,
    read_adpcm_extension,
};
pub use anim::{
    ANIM_COUNTS_OFFSET, ANIM_ENTRYPOINT, ANIM_EXTERNAL_PATH_BYTES, ANIM_EXTERNAL_ROW_BYTES,
    ANIM_FLAG_WORD_OFFSET, ANIM_GRAVITY_OFFSET, ANIM_MEMBER_PATH_BYTES, ANIM_MEMBER_ROW_BYTES,
    ANIM_ONE_WORD_OFFSET, ANIM_PAYLOAD_HEADER_BYTES, ANIM_RECORD_AREA_OFFSET,
    ANIM_RECORD_COUNT_OFFSET, ANIM_RECORD_FIXED_BYTES, ANIM_ROW_BYTES, ANIM_SEQUENCE_INFO_BYTES,
    AnimationIndex, AnimationIndexError, AnimationPayload, AnimationPayloadHeader, AnimationRecord,
    AnimationRecordCounts, AnimationRecordError, AnimationRecordPointers, AnimationRecordSequence,
    AnimationRecordSequenceKind, AnimationRecordTable, AnimationRecordTableKind, AnimationRecords,
    AnimationRow, AnimationRowAnomaly, POINTERS_UNRESOLVED_REASON, RECORDS_NOT_DECODED_REASON,
    indexed_by_animation_header, read_animation_index, stamp_evidence,
};
pub use archive::{
    ArchiveListing, CONTAINER_ENTRYPOINT, ContainerError, ContainerStatus, FamilyMismatch,
    FamilyOrigin, MEMBER_ROW_BYTES, MemberError, MemberExtent, MemberRow, MemberStatus,
    MemberTable, RoutableFamily, SOURCE_SPAN_BYTES, UnsupportedRecord, list_members,
    require_family, undocumented_reason,
};
pub use dispatch::{
    DispatchBasis, HeaderStatus, RoleStatus, ZbdDispatch, ZbdDispatchError, ZbdProbe, dispatch,
};
pub use family::{ZBD_FAMILY_INVENTORY, ZbdFamily, ZbdFamilyRecord, ZbdReaderId, family_record};
pub use header::{
    ANIMATION_SIGNATURE, ANIMATION_SIGNATURE_OFFSET, ANIMATION_VERSION, ANIMATION_VERSION_OFFSET,
    GAMEZ_SIGNATURE, GAMEZ_SIGNATURE_OFFSET, GAMEZ_VERSION, GAMEZ_VERSION_OFFSET, HeaderProbe,
    HeaderRule, INTERP_SIGNATURE, INTERP_SIGNATURE_OFFSET, INTERP_VERSION, INTERP_VERSION_OFFSET,
    SignatureRule,
};
pub use reader_archive::{
    EncodingEvidence, ReaderArchive, ReaderEntry, ReaderError, read_reader_archive,
};
pub use role::{
    CONTENT_ROOT, OUTSIDE_CONTENT_ROOT, RoleLevel, RolePattern, RoleRule, UNOBSERVED_NAME, ZbdRole,
    role_for_path,
};
pub use sound_archive::{
    SAMPLES_NOT_DECODED_REASON, SoundArchive, SoundDescriptor, SoundEntry, SoundError, SoundField,
    read_sound_archive,
};
pub use sound_sample::{
    DecodedSound, PcmLayout, SAMPLE_ENTRYPOINT, SAMPLE_VALUE_BYTES, SampleError, SampleFormat,
    SampleFormatError, SampleLayout, decode_payload, decode_sound_sample,
};
pub use trailer::{
    EntryAnomaly, INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_ROW_BYTES, INDEX_UNEXPLAINED_BYTES,
    IndexEntry, IndexError, MEMBER_EXTENT_BYTES, TRAILER_BYTES, TRAILER_ENTRYPOINT,
    TRAILER_VERSION_ONE, UNEXPLAINED_REASON, UnexplainedBytes, VersionOneIndex, indexed_by_trailer,
    read_version_one_index,
};
pub use wave::{
    CHUNK_HEADER_BYTES, CUE_COUNT_BYTES, CUE_POINT_BYTES, FMT_BYTES, NO_LOOP_CHUNK_REASON,
    RIFF_HEADER_BYTES, SMPL_NOT_READ_REASON, UNNAMED_FORMAT_REASON, WAVE_FORMAT_IMA_ADPCM,
    WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_PCM, WaveError, WaveHeader, format_name, read_wave_header,
};
