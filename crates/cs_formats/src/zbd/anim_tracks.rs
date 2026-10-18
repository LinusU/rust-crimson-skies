//! The motion tracks in the region after the last animation record
//! (task #1662, `PARITY-M01-LC-MEASURE-CAM`).
//!
//! [`AnimationRecords::trailing`](super::AnimationRecords::trailing) was left
//! unwalked by task #650: 31 of the 61 animation containers carry 29 690 ..
//! 1 361 762 bytes after the last record. This module reads them. They are the
//! compiled `.zan` motion tracks the container's member table names: the
//! camera path of the launch cinematic, the cockpit-pilot ejection, the
//! hook-up of a zeppelin, and so on. Finding and decoding is in
//! `docs/findings/2026-10-18-parity-m01-lc-cinematic-camera-pose-source.md`.
//!
//! # The layout (measured, [`ClaimStatus::ObservedTool`])
//!
//! ```text
//! region := descriptor[n]  block[n]                      n = the .zan member rows
//! descriptor (28 bytes): u32 ptr, u32 ptr, u32 type, u32 mode,
//!                        u32 segment_count, u32 data_bytes, u32 ptr
//! block:      path NUL  object-name NUL  segment[segment_count]
//! segment:    u32 flags, f32 start, f32 end, then one 76-byte part per
//!             set flag bit, in the order bit 0, bit 1, bit 2
//! part:       19 x f32
//! ```
//!
//! * `n` is the number of `.zan` rows in the container's member table:
//!   descriptor *k*, and the *k*-th block, belong to the *k*-th such row, and
//!   the block's path repeats the row's path. The caller passes `n`.
//! * the three `ptr` words are in-memory addresses of the build that wrote the
//!   file (`0x03e.....`, `0x045.....`, `0x047.....`); no rule maps one back to
//!   a position and none is read.
//! * `data_bytes` is exactly the byte length of the walked segments, and the
//!   blocks tile the region exactly. Both hold in all **1 090** blocks of the
//!   31 containers (**76 845** segments); this reader refuses a region where
//!   they do not.
//! * flag bit 0 is a *position* part, bit 1 a *rotation* part, bit 2 a *scale*
//!   part. Retail sets flags 0..=7 and nothing above.
//!
//! # What a part holds (static analysis of the engine image)
//!
//! The engine's evaluator is the function at `0x4ea3b0` (the motion
//! executor for animation-event opcode 12, `OBJECT_MOTION_SI_SCRIPT`, at
//! `0x4ea7d0`), and it reads a part as
//!
//! ```text
//! words 0..3   position: the start position; rotation: the start orientation
//!              as a quaternion (w, x, y, z), unit length
//! word  3      position: not read by the evaluator
//! words 4..7   mode 0 only: a rate, multiplied by the local time u
//! words 7..19  mode != 0: three channels of four words (a0, a1, a2, a3), each
//!              evaluated as a0 + u (a1 + u (a2 + u a3))
//! ```
//!
//! where `u = (time - segment start)`. The time unit is not measured (the
//! 14 `generic_intro` tracks end at 15.9333, 12.4375, 9.95 and 1.9667, which
//! sit just above the 15.9, 12.4, 9.9 and 1.95 the intro's stop events state,
//! so seconds is the reading the data supports). Position
//! channels are x, y, z of the node position. Rotation channels are the
//! components of a rotation vector which the engine converts to a quaternion
//! and composes with the segment's start quaternion; how that composition is
//! ordered and how the result becomes the node's Euler angles are not decoded
//! here ([`RotationPart`] exposes the raw words only).
//!
//! Measured over all 76 845 segments: every rotation quaternion has unit
//! length to 1.3e-7, and for contiguous segments with a position part the
//! cubic of one segment evaluated at its length equals the next segment's start
//! position to 3.1e-5 in the 14 `generic_intro` tracks.
//!
//! The in-memory address of the descriptor table, the meaning of `type`, and
//! the frame of reference of the positions are **not** measured.

use std::fmt;

use cs_types::evidence::ClaimStatus;

/// Bytes of one descriptor row.
pub const TRACK_DESCRIPTOR_BYTES: usize = 28;
/// Bytes of one segment's header: flags, start time, end time.
pub const TRACK_SEGMENT_HEADER_BYTES: usize = 12;
/// Floats in one segment part.
pub const TRACK_PART_FLOATS: usize = 19;
/// Bytes of one segment part.
pub const TRACK_PART_BYTES: usize = TRACK_PART_FLOATS * 4;
/// The flag bits a segment may set: position, rotation, scale.
pub const TRACK_FLAGS_MASK: u32 = 0b111;

/// The evidence class of the layout in this module.
pub const fn tracks_evidence() -> ClaimStatus {
    ClaimStatus::ObservedTool
}

/// One 19-float part of a segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackPart {
    words: [f32; TRACK_PART_FLOATS],
}

impl TrackPart {
    fn read(bytes: &[u8]) -> Self {
        let mut words = [0.0f32; TRACK_PART_FLOATS];
        for (index, slot) in words.iter_mut().enumerate() {
            *slot = float_at(bytes, index * 4);
        }
        Self { words }
    }

    /// All 19 words, verbatim.
    pub const fn words(&self) -> &[f32; TRACK_PART_FLOATS] {
        &self.words
    }

    /// Words 0..3: the start position of a position part, the start
    /// orientation (w, x, y, z) of a rotation part is words 0..4.
    pub fn start_vector(&self) -> [f32; 3] {
        [self.words[0], self.words[1], self.words[2]]
    }

    /// Words 0..4: a rotation part's start quaternion (w, x, y, z).
    pub fn start_quaternion(&self) -> [f32; 4] {
        [self.words[0], self.words[1], self.words[2], self.words[3]]
    }

    /// Words 4..7: the rate the engine multiplies by the local time when the
    /// block's mode word is zero.
    pub fn linear_rate(&self) -> [f32; 3] {
        [self.words[4], self.words[5], self.words[6]]
    }

    /// Channel `channel` (0..3) of the cubic: `[a0, a1, a2, a3]`.
    ///
    /// # Panics
    ///
    /// When `channel` is 3 or more.
    pub fn cubic(&self, channel: usize) -> [f32; 4] {
        assert!(channel < 3, "a part has three cubic channels");
        let at = 7 + 4 * channel;
        [
            self.words[at],
            self.words[at + 1],
            self.words[at + 2],
            self.words[at + 3],
        ]
    }

    /// Channel `channel` of the cubic at local time `u`, in the engine's
    /// evaluation order `a0 + u (a1 + u (a2 + u a3))`.
    pub fn cubic_at(&self, channel: usize, u: f32) -> f32 {
        let [a0, a1, a2, a3] = self.cubic(channel);
        a0 + u * (a1 + u * (a2 + u * a3))
    }
}

/// One segment of a track: a time span and the parts that move in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackSegment {
    flags: u32,
    start: f32,
    end: f32,
    position: Option<TrackPart>,
    rotation: Option<TrackPart>,
    scale: Option<TrackPart>,
}

/// A rotation part. The same 19 words as any part; named so a consumer cannot
/// mistake it for a position.
pub type RotationPart = TrackPart;

impl TrackSegment {
    /// The flag word, verbatim.
    pub const fn flags(&self) -> u32 {
        self.flags
    }

    /// Segment start time.
    pub const fn start(&self) -> f32 {
        self.start
    }

    /// Segment end time.
    pub const fn end(&self) -> f32 {
        self.end
    }

    /// The position part, when flag bit 0 is set.
    pub const fn position(&self) -> Option<&TrackPart> {
        self.position.as_ref()
    }

    /// The rotation part, when flag bit 1 is set.
    pub const fn rotation(&self) -> Option<&RotationPart> {
        self.rotation.as_ref()
    }

    /// The scale part, when flag bit 2 is set.
    pub const fn scale(&self) -> Option<&TrackPart> {
        self.scale.as_ref()
    }
}

/// One track: the motion of one object by one `.zan` source.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackBlock<'a> {
    index: usize,
    type_word: u32,
    mode: u32,
    path: &'a [u8],
    object_name: &'a [u8],
    segments: Vec<TrackSegment>,
}

impl<'a> TrackBlock<'a> {
    /// Position among the region's blocks, which is the position of its `.zan`
    /// member row among the `.zan` rows.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The descriptor's type word, verbatim. Its meaning is not measured.
    pub const fn type_word(&self) -> u32 {
        self.type_word
    }

    /// The descriptor's mode word, `1` in 1 075 retail blocks and `0` in 15.
    /// The engine reads a position or rotation part as a cubic when it is
    /// non-zero and as a linear rate when it is zero.
    pub const fn mode(&self) -> u32 {
        self.mode
    }

    /// The source path as stored (separators as written).
    pub const fn path(&self) -> &'a [u8] {
        self.path
    }

    /// The name of the object the track moves.
    pub const fn object_name(&self) -> &'a [u8] {
        self.object_name
    }

    /// The segments, in stored order.
    pub fn segments(&self) -> &[TrackSegment] {
        &self.segments
    }

    /// The end time of the last segment, or `0.0` for an empty block.
    pub fn duration(&self) -> f32 {
        self.segments.last().map_or(0.0, TrackSegment::end)
    }
}

/// Every track of one container.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationTracks<'a> {
    blocks: Vec<TrackBlock<'a>>,
}

impl<'a> AnimationTracks<'a> {
    /// The blocks, in descriptor order.
    pub fn blocks(&self) -> &[TrackBlock<'a>] {
        &self.blocks
    }

    /// Block `index`.
    pub fn block(&self, index: usize) -> Option<&TrackBlock<'a>> {
        self.blocks.get(index)
    }

    /// Total segments over all blocks.
    pub fn segment_count(&self) -> usize {
        self.blocks.iter().map(|block| block.segments.len()).sum()
    }
}

/// Why the region could not be read as tracks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackError {
    /// The region is shorter than `n` descriptor rows.
    DescriptorsTruncated {
        /// Descriptor rows requested.
        rows: usize,
        /// Bytes the region has.
        available: usize,
    },
    /// A block's path or object name has no NUL before the region ends.
    NameUnterminated {
        /// The block.
        block: usize,
        /// Which name.
        part: &'static str,
    },
    /// A block's data runs past the region.
    DataTruncated {
        /// The block.
        block: usize,
        /// Bytes the descriptor declares.
        declared: u32,
        /// Bytes left in the region.
        available: usize,
    },
    /// A segment header or part runs past its block's data.
    SegmentTruncated {
        /// The block.
        block: usize,
        /// The segment.
        segment: usize,
    },
    /// A segment sets a flag bit above the scale bit.
    UnknownFlags {
        /// The block.
        block: usize,
        /// The segment.
        segment: usize,
        /// The flag word.
        flags: u32,
    },
    /// The segments do not number what the descriptor declares, or do not
    /// consume exactly the declared bytes.
    SegmentsDisagree {
        /// The block.
        block: usize,
        /// Segments the descriptor declares.
        declared: u32,
        /// Segments walked before the data ended.
        walked: usize,
    },
    /// Bytes remain after the last block.
    TrailingBytes {
        /// How many.
        bytes: usize,
    },
}

impl TrackError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DescriptorsTruncated { .. } => "track_descriptors_truncated",
            Self::NameUnterminated { .. } => "track_name_unterminated",
            Self::DataTruncated { .. } => "track_data_truncated",
            Self::SegmentTruncated { .. } => "track_segment_truncated",
            Self::UnknownFlags { .. } => "track_unknown_flags",
            Self::SegmentsDisagree { .. } => "track_segments_disagree",
            Self::TrailingBytes { .. } => "track_trailing_bytes",
        }
    }
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DescriptorsTruncated { rows, available } => write!(
                f,
                "{rows} track descriptors need {} bytes, the region has {available}",
                rows * TRACK_DESCRIPTOR_BYTES
            ),
            Self::NameUnterminated { block, part } => {
                write!(f, "track block {block}: the {part} has no terminating NUL")
            }
            Self::DataTruncated {
                block,
                declared,
                available,
            } => write!(
                f,
                "track block {block} declares {declared} data bytes, the region has {available}"
            ),
            Self::SegmentTruncated { block, segment } => write!(
                f,
                "track block {block}: segment {segment} runs past the block's data"
            ),
            Self::UnknownFlags {
                block,
                segment,
                flags,
            } => write!(
                f,
                "track block {block}: segment {segment} sets flags {flags:#x}, only bits 0..=2 are measured"
            ),
            Self::SegmentsDisagree {
                block,
                declared,
                walked,
            } => write!(
                f,
                "track block {block} declares {declared} segments, its data holds {walked}"
            ),
            Self::TrailingBytes { bytes } => {
                write!(f, "{bytes} bytes remain after the last track block")
            }
        }
    }
}

impl std::error::Error for TrackError {}

fn word_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes of a word"))
}

fn float_at(bytes: &[u8], at: usize) -> f32 {
    f32::from_bits(word_at(bytes, at))
}

fn take_name<'a>(
    region: &'a [u8],
    at: &mut usize,
    block: usize,
    part: &'static str,
) -> Result<&'a [u8], TrackError> {
    let rest = &region[*at..];
    let length = rest
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(TrackError::NameUnterminated { block, part })?;
    *at += length + 1;
    Ok(&rest[..length])
}

fn walk_segments(
    data: &[u8],
    block: usize,
    declared: u32,
) -> Result<Vec<TrackSegment>, TrackError> {
    let mut segments = Vec::new();
    let mut at = 0usize;
    while at < data.len() {
        let segment = segments.len();
        if data.len() - at < TRACK_SEGMENT_HEADER_BYTES {
            return Err(TrackError::SegmentTruncated { block, segment });
        }
        let flags = word_at(data, at);
        if flags & !TRACK_FLAGS_MASK != 0 {
            return Err(TrackError::UnknownFlags {
                block,
                segment,
                flags,
            });
        }
        let start = float_at(data, at + 4);
        let end = float_at(data, at + 8);
        at += TRACK_SEGMENT_HEADER_BYTES;
        let mut part = |bit: u32| -> Result<Option<TrackPart>, TrackError> {
            if flags & bit == 0 {
                return Ok(None);
            }
            let bytes = data
                .get(at..at + TRACK_PART_BYTES)
                .ok_or(TrackError::SegmentTruncated { block, segment })?;
            at += TRACK_PART_BYTES;
            Ok(Some(TrackPart::read(bytes)))
        };
        let position = part(1)?;
        let rotation = part(2)?;
        let scale = part(4)?;
        segments.push(TrackSegment {
            flags,
            start,
            end,
            position,
            rotation,
            scale,
        });
    }
    if segments.len() as u64 != u64::from(declared) {
        return Err(TrackError::SegmentsDisagree {
            block,
            declared,
            walked: segments.len(),
        });
    }
    Ok(segments)
}

/// Read the region after an animation container's last record as `rows`
/// tracks, where `rows` is the number of `.zan` rows in the container's member
/// table. Refuses a region that does not tile exactly.
pub fn read_animation_tracks(
    region: &[u8],
    rows: usize,
) -> Result<AnimationTracks<'_>, TrackError> {
    let table_bytes =
        rows.checked_mul(TRACK_DESCRIPTOR_BYTES)
            .ok_or(TrackError::DescriptorsTruncated {
                rows,
                available: region.len(),
            })?;
    if region.len() < table_bytes {
        return Err(TrackError::DescriptorsTruncated {
            rows,
            available: region.len(),
        });
    }
    let mut at = table_bytes;
    let mut blocks = Vec::with_capacity(rows);
    for index in 0..rows {
        let row = &region[index * TRACK_DESCRIPTOR_BYTES..][..TRACK_DESCRIPTOR_BYTES];
        let type_word = word_at(row, 8);
        let mode = word_at(row, 12);
        let declared_segments = word_at(row, 16);
        let declared_bytes = word_at(row, 20);
        let path = take_name(region, &mut at, index, "path")?;
        let object_name = take_name(region, &mut at, index, "object name")?;
        let end = at
            .checked_add(declared_bytes as usize)
            .filter(|end| *end <= region.len())
            .ok_or(TrackError::DataTruncated {
                block: index,
                declared: declared_bytes,
                available: region.len() - at,
            })?;
        let segments = walk_segments(&region[at..end], index, declared_segments)?;
        at = end;
        blocks.push(TrackBlock {
            index,
            type_word,
            mode,
            path,
            object_name,
            segments,
        });
    }
    if at != region.len() {
        return Err(TrackError::TrailingBytes {
            bytes: region.len() - at,
        });
    }
    Ok(AnimationTracks { blocks })
}
