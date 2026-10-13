//! The measured event-stream layout inside a camera carrier's animation
//! records (F40-D3).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-D`. The finding that records the measurement:
//! `docs/findings/2026-10-13-f40-d3-cam-anim-camera-format.md`.
//!
//! # What was measured
//!
//! `ZBD/<group>/cam_anim.zbd` is the world group's **camera carrier**: the
//! same animation-container format as `mis_anim.zbd` (signature `0x08170616`,
//! version 53, the front index and the 272-byte records of task #650) —
//! its records are animation definitions, not a camera-keyframe table. What
//! task #650 left as raw bytes, each sequence block's **event stream**, is
//! measured here as a TLV sequence:
//!
//! ```text
//! event: u32 type_id; u32 total_bytes; u8 payload[total_bytes - 8]
//! ```
//!
//! * every event starts with the 8-byte header and occupies exactly
//!   `total_bytes` bytes; the next event starts there, so a stream is one
//!   flat sequence of events with no padding and no trailer;
//! * `total_bytes` is at least 12 and at most 1156 in every retail event
//!   measured (see [`MEASURED_MIN_EVENT_BYTES`]);
//! * each measured type id stores one fixed `total_bytes` across every
//!   container it occurs in — with one measured exception, `0x12f`, which
//!   stores 548 or 1156 ([`measured_event_total_size`]);
//! * the payload bytes are **not decoded**: no field inside a payload has a
//!   measured meaning, so this module keeps them verbatim and never reads a
//!   pose, a tick or a name out of one.
//!
//! The census behind this (2026-10-13, this repository's own tool, read-only
//! against `$CS_GAME_DIR`): **all 61 retail animation carriers — 56 994
//! sequence blocks, 242 391 events — every stream tiled exactly by its
//! events, no size below 12, no overrun, no trailing bytes, 59 distinct type
//! ids of which 58 store one fixed size**. The 56 994 blocks are exactly the
//! count task #650 walked, which cross-checks the two derivations.
//!
//! # Camera cinematics, and what is *not* measured
//!
//! The in-engine camera sequences are the records whose `object_name` **and**
//! `root_name` are `camera1` ([`CAMERA_OBJECT_NAME`]): `generic_intro`,
//! `gi_scene1`, `player_setup` and `speed_cue` in all eight carriers
//! (`speed_cue2` additionally in `c1b` and `c2b`), and mission carriers carry
//! their own (`mission_intro_animation`/`scene1` in `c4/m01`). They
//! *orchestrate* — their events reference other animation names, scene
//! indices and node tables — but **no measured field carries a camera pose
//! keyframe**: no per-tick position, orientation or field-of-view table
//! exists anywhere this reader measures. So no
//! [`super::timeline::CameraTimeline`] can be built from this data today;
//! the candidates and their rejection are in the finding, and the content
//! half marks the affected fields `Resolved::Unknown`
//! ([`cs_content::cinematics::declared_camera_sequence`]) instead of
//! inventing keyframes.
//!
//! # Fail-closed
//!
//! The stream walk refuses a truncated header, an event whose `total_bytes`
//! is below the measured minimum, and an event that would run past the end of
//! its stream ([`CameraAnimStreamError`]) — each naming the offset, the type
//! id and the arithmetic, never a payload byte. The carrier survey refuses a
//! container that does not dispatch to the animation family, before a byte of
//! the index is read ([`CameraCarrierError`]).

use std::fmt;

use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    AnimationRecord, AnimationRecordSequence, AnimationRecordSequenceKind, HeaderStatus, ZbdFamily,
    ZbdProbe, dispatch, family_record, read_animation_index,
};
use cs_types::install::RelativePath;

/// Bytes of an event's fixed header: the type id and the event's total byte
/// length, both `u32`.
pub const CAMERA_EVENT_HEADER_BYTES: usize = 8;

/// The smallest `total_bytes` any retail event stores (the `0x120`/`0x122`
/// events: a header and one word). An event that declares fewer bytes is
/// refused by name rather than walked.
pub const MEASURED_MIN_EVENT_BYTES: u32 = 12;

/// The largest `total_bytes` any retail event stores (`0x12f`'s 1156-byte
/// shape). Recorded so a census can state the measured range without
/// re-walking the installation.
pub const MEASURED_MAX_EVENT_BYTES: u32 = 1156;

/// The object **and** root name of the camera-cinematic records, measured in
/// all eight retail camera carriers and in the mission carriers that ship
/// one.
pub const CAMERA_OBJECT_NAME: &[u8] = b"camera1";

/// The fixed `total_bytes` of one measured event type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredEventSize {
    /// One stored size in every container the type occurs in.
    Fixed(u32),
    /// Two stored sizes, both measured (`0x12f`: 548 and 1156); nothing
    /// measured tells the two shapes apart, so both are kept.
    OneOf(u32, u32),
}

impl MeasuredEventSize {
    /// Whether `total_bytes` is one of this type's measured sizes.
    #[must_use]
    pub const fn contains(self, total_bytes: u32) -> bool {
        match self {
            Self::Fixed(size) => size == total_bytes,
            Self::OneOf(first, second) => first == total_bytes || second == total_bytes,
        }
    }
}

/// The fixed `total_bytes` measured for `type_id` across **all 61 retail
/// animation carriers**, or `None` for a type this repository has not
/// measured.
///
/// Every type id and size below is this repository's own measurement over the
/// retail containers (`ClaimStatus::ObservedTool`); the semantics of a type
/// are **not** measured, so the table never interprets one. A type outside
/// this table is reported by the survey ([`CameraCarrierSurvey::unmeasured_types`])
/// rather than assumed to fit the walk.
#[must_use]
pub const fn measured_event_total_size(type_id: u32) -> Option<MeasuredEventSize> {
    let size = match type_id {
        0x101 | 0x201 | 0x301 => 28,
        0x102 => 72,
        0x104 | 0x304 => 152,
        0x105 | 0x205 | 0x305 => 112,
        0x106 | 0x306 => 20,
        0x107 => 32,
        0x108 => 28,
        0x109 | 0x309 => 32,
        0x10a => 332,
        0x10b | 0x20b | 0x30b => 144,
        0x10c | 0x30c => 76,
        0x10d => 24,
        0x10e | 0x20e | 0x30e => 36,
        0x10f | 0x110 => 16,
        0x111 => 20,
        0x114 => 48,
        0x116 | 0x117 | 0x119 | 0x11a | 0x11b => 48,
        0x118 | 0x11c => 80,
        0x11e | 0x21e | 0x31e => 20,
        0x11f | 0x121 => 28,
        0x120 | 0x122 => 12,
        0x123 => 16,
        0x124 => 64,
        0x129 => 36,
        0x12a | 0x22a | 0x32a => 596,
        0x12e => 188,
        0x12f => return Some(MeasuredEventSize::OneOf(548, 1156)),
        0x216 | 0x217 | 0x219 => 48,
        0x218 => 80,
        0x316 | 0x317 | 0x319 => 48,
        0x318 => 80,
        _ => return None,
    };
    Some(MeasuredEventSize::Fixed(size))
}

/// One TLV event of a sequence stream: its type id and its payload bytes
/// (everything after the 8-byte header), verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraAnimEvent<'a> {
    type_id: u32,
    total_bytes: u32,
    payload: &'a [u8],
}

impl<'a> CameraAnimEvent<'a> {
    /// The event's type id (`0x101`..`0x32a` in retail).
    pub const fn type_id(&self) -> u32 {
        self.type_id
    }

    /// The event's declared total byte length, header included.
    pub const fn total_bytes(&self) -> u32 {
        self.total_bytes
    }

    /// The payload bytes after the header, verbatim and never interpreted.
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// The fixed size measured for this event's type, when this repository
    /// has measured one.
    pub const fn measured_size(&self) -> Option<MeasuredEventSize> {
        measured_event_total_size(self.type_id)
    }

    /// Whether the event's stored size matches its type's measured size.
    ///
    /// `false` for a type with no measurement: an unmeasured type is never
    /// treated as conforming.
    pub const fn matches_measured_size(&self) -> bool {
        match self.measured_size() {
            Some(size) => size.contains(self.total_bytes),
            None => false,
        }
    }
}

/// An [`CameraAnimEvent`] with its payload bytes owned, so a survey result
/// borrows nothing from the container it was walked from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedCameraAnimEvent {
    type_id: u32,
    total_bytes: u32,
    payload: Vec<u8>,
}

impl OwnedCameraAnimEvent {
    /// Copies a parsed event's bytes.
    #[must_use]
    pub fn from_parsed(event: &CameraAnimEvent<'_>) -> Self {
        Self {
            type_id: event.type_id,
            total_bytes: event.total_bytes,
            payload: event.payload.to_vec(),
        }
    }

    /// The event's type id.
    pub const fn type_id(&self) -> u32 {
        self.type_id
    }

    /// The event's declared total byte length, header included.
    pub const fn total_bytes(&self) -> u32 {
        self.total_bytes
    }

    /// The owned payload bytes, verbatim and never interpreted.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The fixed size measured for this event's type, when measured.
    pub const fn measured_size(&self) -> Option<MeasuredEventSize> {
        measured_event_total_size(self.type_id)
    }

    /// Whether the event's stored size matches its type's measured size.
    pub const fn matches_measured_size(&self) -> bool {
        match self.measured_size() {
            Some(size) => size.contains(self.total_bytes),
            None => false,
        }
    }
}

/// Why a sequence event stream could not be walked.
///
/// Carries offsets, type ids and lengths only, never payload bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraAnimStreamError {
    /// Fewer than the 8 header bytes remain at this offset.
    TruncatedHeader {
        /// Where the event would start, counted from the stream start.
        offset: u64,
        /// Bytes the header needs.
        needed: u64,
        /// Bytes the stream has from there.
        available: u64,
    },
    /// The event declares fewer total bytes than any retail event stores.
    SizeBelowMeasuredMinimum {
        /// Where the event starts.
        offset: u64,
        /// The event's type id.
        type_id: u32,
        /// The declared total bytes.
        total_bytes: u32,
    },
    /// The event would run past the end of its stream.
    EventOverrunsStream {
        /// Where the event starts.
        offset: u64,
        /// The event's type id.
        type_id: u32,
        /// The declared total bytes.
        total_bytes: u32,
        /// Bytes the stream has from the offset.
        available: u64,
    },
}

impl CameraAnimStreamError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TruncatedHeader { .. } => "truncated_header",
            Self::SizeBelowMeasuredMinimum { .. } => "size_below_measured_minimum",
            Self::EventOverrunsStream { .. } => "event_overruns_stream",
        }
    }
}

impl fmt::Display for CameraAnimStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader {
                offset,
                needed,
                available,
            } => write!(
                f,
                "the event at stream offset {offset} needs {needed} header bytes, the stream has \
                 {available}"
            ),
            Self::SizeBelowMeasuredMinimum {
                offset,
                type_id,
                total_bytes,
            } => write!(
                f,
                "the event 0x{type_id:x} at stream offset {offset} declares {total_bytes} bytes, \
                 below the {MEASURED_MIN_EVENT_BYTES} bytes every retail event stores"
            ),
            Self::EventOverrunsStream {
                offset,
                type_id,
                total_bytes,
                available,
            } => write!(
                f,
                "the event 0x{type_id:x} at stream offset {offset} declares {total_bytes} bytes, \
                 the stream has {available}"
            ),
        }
    }
}

impl std::error::Error for CameraAnimStreamError {}

/// Walks one sequence block's event stream into its TLV events.
///
/// The stream is one flat sequence of `{ u32 type_id; u32 total_bytes;
/// payload[total_bytes - 8] }` events; this walk reads the header of each and
/// keeps the payload verbatim. It reads no byte it was not told to read, and
/// it stops at the first refusal.
///
/// # Errors
///
/// [`CameraAnimStreamError`] for a truncated header, a declared size below
/// [`MEASURED_MIN_EVENT_BYTES`] or an event running past the stream's end.
pub fn parse_camera_anim_stream(
    stream: &[u8],
) -> Result<Vec<CameraAnimEvent<'_>>, CameraAnimStreamError> {
    let mut events = Vec::new();
    let mut offset = 0usize;
    while offset < stream.len() {
        let available = (stream.len() - offset) as u64;
        let end_of_header = stream.len().min(offset + CAMERA_EVENT_HEADER_BYTES);
        let header = &stream[offset..end_of_header];
        if header.len() < CAMERA_EVENT_HEADER_BYTES {
            return Err(CameraAnimStreamError::TruncatedHeader {
                offset: offset as u64,
                needed: CAMERA_EVENT_HEADER_BYTES as u64,
                available,
            });
        }
        let word = |at: usize| {
            u32::from_le_bytes(
                header[at..at + 4]
                    .try_into()
                    .expect("four bytes of an event header"),
            )
        };
        let type_id = word(0);
        let total_bytes = word(4);
        if total_bytes < MEASURED_MIN_EVENT_BYTES {
            return Err(CameraAnimStreamError::SizeBelowMeasuredMinimum {
                offset: offset as u64,
                type_id,
                total_bytes,
            });
        }
        let end = offset + total_bytes as usize;
        if end > stream.len() {
            return Err(CameraAnimStreamError::EventOverrunsStream {
                offset: offset as u64,
                type_id,
                total_bytes,
                available,
            });
        }
        events.push(CameraAnimEvent {
            type_id,
            total_bytes,
            payload: &stream[offset + CAMERA_EVENT_HEADER_BYTES..end],
        });
        offset = end;
    }
    Ok(events)
}

/// Whether a record is one of the camera cinematics: its `object_name` and
/// its `root_name` are both [`CAMERA_OBJECT_NAME`], the measured shape of
/// every camera-rooted retail record.
#[must_use]
pub fn is_camera_cinematic(record: &AnimationRecord<'_>) -> bool {
    record.object_name() == CAMERA_OBJECT_NAME && record.root_name() == CAMERA_OBJECT_NAME
}

/// One parsed sequence block of a camera-cinematic record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraSequenceBlock {
    /// Reset, damage or ordinary — the record's own classification.
    pub kind: AnimationRecordSequenceKind,
    /// The block's parsed TLV events, in stored order.
    pub events: Vec<OwnedCameraAnimEvent>,
}

/// One camera-cinematic record, with every one of its sequence blocks'
/// event streams parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraSequence {
    /// The record's position in the carrier's record list.
    pub record_index: usize,
    /// The animation identity (`anim_name`), verbatim.
    pub anim_name: Vec<u8>,
    /// The object the animation moves (`camera1` by classification).
    pub object_name: Vec<u8>,
    /// The animation's root object (`camera1` by classification).
    pub root_name: Vec<u8>,
    /// The record's flag word, meaning not measured bit by bit.
    pub flags: u32,
    /// Every sequence block, in on-disk order: reset, damage, then ordinary.
    pub blocks: Vec<CameraSequenceBlock>,
}

/// What a camera carrier's camera cinematics look like, measured by walking
/// the carrier's own bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraCarrierSurvey {
    container: String,
    version: u32,
    total_records: usize,
    sequences: Vec<CameraSequence>,
    streams_parsed: u64,
    events_parsed: u64,
    unmeasured_types: Vec<u32>,
    size_mismatches: Vec<(u32, u32)>,
}

impl CameraCarrierSurvey {
    /// The container's provenance label.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The animation-family version word the container declares.
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// How many animation records the carrier holds in total.
    pub const fn total_records(&self) -> usize {
        self.total_records
    }

    /// The camera-cinematic records, in carrier order.
    pub fn sequences(&self) -> &[CameraSequence] {
        &self.sequences
    }

    /// How many sequence blocks were walked across the camera cinematics.
    pub const fn streams_parsed(&self) -> u64 {
        self.streams_parsed
    }

    /// How many TLV events were walked across the camera cinematics.
    pub const fn events_parsed(&self) -> u64 {
        self.events_parsed
    }

    /// The event type ids that occurred with no measured size (deduplicated,
    /// ascending). Empty on every retail carrier measured.
    pub fn unmeasured_types(&self) -> &[u32] {
        &self.unmeasured_types
    }

    /// The `(type_id, total_bytes)` events whose stored size did not match
    /// their type's measured size (deduplicated). Empty on every retail
    /// carrier measured.
    pub fn size_mismatches(&self) -> &[(u32, u32)] {
        &self.size_mismatches
    }
}

/// Why a camera carrier could not be surveyed.
///
/// Carries names, codes and lengths only, never container bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraCarrierError {
    /// Two-key dispatch refused the container's bytes.
    DispatchRefused {
        /// The container's provenance label.
        container: String,
        /// The refusal's stable code.
        code: &'static str,
    },
    /// Dispatch identified another family; its records are not this
    /// format's.
    WrongFamily {
        /// The container's provenance label.
        container: String,
        /// The family dispatch decided.
        family: ZbdFamily,
    },
    /// Dispatch answered `Unvalidated` — not bytes this survey walks.
    UnvalidatedHeader {
        /// The container's provenance label.
        container: String,
    },
    /// The index, the payload header or the record walk refused.
    CarrierRefused {
        /// The container's provenance label.
        container: String,
        /// The refusal's stable code.
        code: &'static str,
    },
    /// A sequence block's event stream did not parse.
    Stream {
        /// The container's provenance label.
        container: String,
        /// The record's animation name, verbatim.
        anim_name: Vec<u8>,
        /// Which block of that record, by position in its block list.
        block: usize,
        /// The stream walk's refusal.
        source: CameraAnimStreamError,
    },
}

impl CameraCarrierError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DispatchRefused { .. } => "dispatch_refused",
            Self::WrongFamily { .. } => "wrong_family",
            Self::UnvalidatedHeader { .. } => "unvalidated_header",
            Self::CarrierRefused { .. } => "carrier_refused",
            Self::Stream { .. } => "stream",
        }
    }
}

impl fmt::Display for CameraCarrierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DispatchRefused { container, code } => {
                write!(f, "{container}: dispatch refused the container ({code})")
            }
            Self::WrongFamily { container, family } => write!(
                f,
                "{container}: the `{}` family is not a camera carrier",
                family.as_str()
            ),
            Self::UnvalidatedHeader { container } => {
                write!(f, "{container}: the header validated no family")
            }
            Self::CarrierRefused { container, code } => {
                write!(f, "{container}: the carrier refused to be read ({code})")
            }
            Self::Stream {
                container,
                anim_name,
                block,
                source,
            } => write!(
                f,
                "{container}: record {} block {block}: {source}",
                String::from_utf8_lossy(anim_name)
            ),
        }
    }
}

impl std::error::Error for CameraCarrierError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Stream { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Surveys one camera carrier's own bytes: dispatches the container, walks
/// its animation records, and parses every event stream of every
/// camera-cinematic record ([`is_camera_cinematic`]).
///
/// `path` is the carrier's installation-relative spelling; the bytes come
/// from the caller, so a synthetic installation and the original one take
/// exactly this path.
///
/// # Errors
///
/// [`CameraCarrierError`] naming the refusal: dispatch, family, index,
/// payload, record walk or a stream that does not tile.
pub fn survey_camera_carrier(
    path: &RelativePath,
    bytes: &[u8],
) -> Result<CameraCarrierSurvey, CameraCarrierError> {
    let key = path.logical_key();
    let container = key.clone();
    let mut context = ParseContext::with_defaults(&key);
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    let decision = dispatch(ZbdProbe::new(&key, path, &bytes[..needed])).map_err(|error| {
        CameraCarrierError::DispatchRefused {
            container: container.clone(),
            code: error.code(),
        }
    })?;
    let version = match decision.header_status() {
        HeaderStatus::Validated { version, .. } => version,
        HeaderStatus::Unvalidated { .. } => {
            return Err(CameraCarrierError::UnvalidatedHeader { container });
        }
    };
    if decision.family() != ZbdFamily::Animation {
        return Err(CameraCarrierError::WrongFamily {
            container,
            family: decision.family(),
        });
    }
    let index = read_animation_index(&mut context, decision, bytes).map_err(|error| {
        CameraCarrierError::CarrierRefused {
            container: container.clone(),
            code: error.code(),
        }
    })?;
    let payload = index
        .payload()
        .map_err(|error| CameraCarrierError::CarrierRefused {
            container: container.clone(),
            code: error.code(),
        })?;
    let records = payload
        .records()
        .map_err(|error| CameraCarrierError::CarrierRefused {
            container: container.clone(),
            code: error.code(),
        })?;

    let mut sequences = Vec::new();
    let mut streams_parsed = 0u64;
    let mut events_parsed = 0u64;
    let mut unmeasured_types: Vec<u32> = Vec::new();
    let mut size_mismatches: Vec<(u32, u32)> = Vec::new();
    for record in records.iter() {
        if !is_camera_cinematic(record) {
            continue;
        }
        let anim_name = record.anim_name().to_vec();
        let mut blocks = Vec::new();
        for (block_index, block) in record.sequences().iter().enumerate() {
            let events = parse_camera_anim_stream(block.events()).map_err(|source| {
                CameraCarrierError::Stream {
                    container: container.clone(),
                    anim_name: anim_name.clone(),
                    block: block_index,
                    source,
                }
            })?;
            streams_parsed += 1;
            events_parsed += events.len() as u64;
            for event in &events {
                match event.measured_size() {
                    None => {
                        if !unmeasured_types.contains(&event.type_id()) {
                            unmeasured_types.push(event.type_id());
                        }
                    }
                    Some(size) if !size.contains(event.total_bytes()) => {
                        let pair = (event.type_id(), event.total_bytes());
                        if !size_mismatches.contains(&pair) {
                            size_mismatches.push(pair);
                        }
                    }
                    Some(_) => {}
                }
            }
            blocks.push(CameraSequenceBlock {
                kind: block.kind(),
                events: events
                    .iter()
                    .map(OwnedCameraAnimEvent::from_parsed)
                    .collect(),
            });
        }
        sequences.push(CameraSequence {
            record_index: record.index(),
            anim_name,
            object_name: record.object_name().to_vec(),
            root_name: record.root_name().to_vec(),
            flags: record.flags(),
            blocks,
        });
    }
    unmeasured_types.sort_unstable();
    size_mismatches.sort_unstable();
    Ok(CameraCarrierSurvey {
        container,
        version,
        total_records: records.len(),
        sequences,
        streams_parsed,
        events_parsed,
        unmeasured_types,
        size_mismatches,
    })
}

/// The kind of a sequence block, re-exported from the family reader so a
/// survey caller names the block exactly as the container stores it.
pub use cs_formats::zbd::AnimationRecordSequenceKind as SequenceBlockKind;

/// The events of a record's sequence block, for callers that walk blocks
/// directly: the raw stream a [`CameraSequenceBlock`] parses.
pub const fn sequence_block_stream<'a>(block: &'a AnimationRecordSequence<'_>) -> &'a [u8] {
    block.events()
}
