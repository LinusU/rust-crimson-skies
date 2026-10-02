//! Block decoding of the two ADPCM layouts the retail sound archives declare
//! (task #444).
//!
//! Task #344 read the RIFF/WAVE header of every member of `ZBD/sounds*.zbd` and
//! found that nearly all of them declare a compressed sample format
//! (`docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md`): 4,464
//! members carry `wFormatTag` `0x0002` (Microsoft ADPCM) and 555 carry
//! `0x0011` (Intel/IMA, "DVI" ADPCM), 5,019 of the archives' 5,041 members.
//! Stage F06-C decoded uncompressed PCM only and refused every compressed
//! member with the tag the member itself declares
//! ([`crate::zbd::sound_sample::SampleFormatError::UnsupportedFormat`]).
//! This module decodes the two block layouts those tags name, and nothing else.
//!
//! # What is read from the member and what is fixed by the format
//!
//! The block *geometry* (how many bytes a header takes, how many nibbles hold
//! how many samples) and the *codebook* (the step, index and adaptation tables
//! below) are properties of the two named formats, not of this installation.
//! Everything that varies per member is read from the member's own bytes:
//!
//! | Field | Read from |
//! | --- | --- |
//! | `nChannels`, `nBlockAlign` | the member's `fmt ` chunk |
//! | `wSamplesPerBlock` | the member's `fmt ` extension, the two bytes after its `cbSize` |
//! | MS ADPCM `aCoefs` | the member's own `fmt ` coefficient table |
//! | per-block predictor, step index, delta, history | the block's own bytes |
//!
//! The extension's fields are located by the offsets the two layouts document,
//! not by the value `cbSize` itself: a member whose `cbSize` contradicts the
//! ones measured in the retail corpus is still read from those offsets, and a
//! payload too short to hold them is refused
//! ([`AdpcmExtension::Short`]) rather than padded or read out of bounds.
//!
//! Nothing here is a tuning table of this project's own, and no loop point, no
//! pitch and no volume is decided here: a loop point is a header fact no retail
//! member carries ([`crate::zbd::wave::WaveHeader::loop_reason`]), and playback
//! is F41's business.
//!
//! # The two block layouts
//!
//! **IMA (DVI) ADPCM, `wFormatTag` `0x0011`, 4 bits per sample.** Every block is
//! `nBlockAlign` bytes: a four-byte header — a little-endian `i16` initial
//! predictor, a `u8` step index and one reserved byte — then the encoded
//! nibbles, **low nibble of each byte first**. A block therefore holds
//! `1 + 2 * (nBlockAlign - 4)` samples, which is what a member declaring
//! `wSamplesPerBlock` 505 at `nBlockAlign` 256 declares. Each nibble moves the
//! predictor by a quarter of the current step: `step / 8` plus `step / 4`, `step
//! / 2` or `step` for the three magnitude bits, subtracted when bit 3 is set,
//! and then moves the step index by the format's index table, clamped to
//! `0..=88`.
//!
//! **Microsoft ADPCM, `wFormatTag` `0x0002`, 4 bits per sample.** Every block is
//! `nBlockAlign` bytes and starts with seven bytes per channel: a `u8` index
//! into the member's own `aCoefs` table, a little-endian `i16` initial delta,
//! and the two `i16` history samples the block's first two output values are.
//! The header fields are stored grouped by field, not grouped by channel: every
//! channel's coefficient index, then every channel's delta, then every channel's
//! first history sample, then every channel's second. The block's two history
//! values are output in the order the layout stores them — the older sample
//! first — and the encoded nibbles follow, **high nibble of each byte first**:
//! for a mono block both nibbles are that channel's next two samples, and for a
//! two-channel block the high nibble is the first channel's next sample and the
//! low nibble the second channel's. Each nibble predicts
//!
//! ```text
//! sample = (sample1 * aCoefs[index].predictor + sample2 * aCoefs[index].difference) / 256
//!          + (nibble >= 8 ? nibble - 16 : nibble) * delta
//! ```
//!
//! with integer division that truncates towards zero, the result clamped to
//! `i16`, and then `delta = max(16, ADAPTATION_TABLE[nibble] * delta / 256)`.
//!
//! # Measured against an independent implementation
//!
//! The two block layouts above were checked against FFmpeg's `adpcm_ima_wav`
//! and `adpcm_ms` decoders — a separate implementation of the same two
//! documented formats — by decoding one member of every distinct retail shape
//! with both and comparing the sample sequences value for value. All seven
//! retail shapes agree bit for bit (6,091,417 samples across the seven members
//! compared). The measurement, its private artifacts and its limits are in
//! `docs/findings/2026-10-02-t444-ima-and-ms-adpcm-block-decoding.md`.
//!
//! One limit of that cross-check is worth stating here: FFmpeg's MS ADPCM
//! decoder applies its own copy of the coefficient table, not the member's, so
//! it confirms this decode only for a member that declares that same table.
//! The retail members all declare it (task #444 measured one table across all
//! 4,464 of them), and the decode below reads the member's own table, which is
//! what the format says to do.
//!
//! # What stays unknown
//!
//! * A member with **more than one channel of IMA ADPCM**: no retail member has
//!   one, so the way a multi-channel IMA block interleaves its channel data is
//!   not observed here and is refused rather than guessed
//!   ([`AdpcmError::UnsupportedChannels`]).
//! * A member with **more than two channels of MS ADPCM**: retail has mono and
//!   stereo only, and the layouts documented for wider blocks differ, so it is
//!   refused the same way.
//! * **Blocks carry no continuity.** The layout states each block's predictor
//!   and history in the block's own header, so a decoder must not carry state
//!   across blocks; the retail archives' blocks do not continue each other
//!   (measured: 111,688 of 112,929 IMA and 767,926 of 773,332 MS block
//!   boundaries re-state a value that differs from the previous block's last
//!   output), which is consistent with the layout and says nothing about how
//!   the original game plays the sound.
//! * What the **original executable** did with the decoded values (pitch,
//!   volume, spatialisation) is not decided by these bytes and is not decided
//!   here.
//! * The **trailing short block** case: no retail member has one (every retail
//!   `data` payload is a whole number of `nBlockAlign` blocks), so the sample
//!   count of a short final block follows the documented geometry and is
//!   covered by synthetic tests only.

use std::fmt;

use super::wave::{WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_MS_ADPCM};

/// Bytes of the common `fmt ` fields every tag carries, up to and including
/// `wBitsPerSample`. The ADPCM fields start after these.
pub const FMT_COMMON_BYTES: usize = 16;

/// Bytes of the `fmt ` extension both ADPCM tags start with: u16 `cbSize`.
pub const CB_SIZE_BYTES: usize = 2;

/// Bytes of the whole IMA ADPCM `fmt ` extension: `cbSize` and
/// `wSamplesPerBlock`.
pub const IMA_EXTENSION_BYTES: usize = 4;

/// Bytes of the fixed part of the MS ADPCM `fmt ` extension: `cbSize`,
/// `wSamplesPerBlock` and `wNumCoefs`.
pub const MS_EXTENSION_BYTES: usize = 6;

/// Bytes of one MS ADPCM coefficient pair: two `i16`, `aCoefs[i][0]` then
/// `aCoefs[i][1]`.
pub const MS_COEFFICIENT_BYTES: usize = 4;

/// Coefficient pairs the MS ADPCM `fmt ` extension may declare. Retail members
/// declare all seven, and the layout reserves no more.
pub const MS_COEFFICIENT_PAIRS: usize = 7;

/// Bytes of the IMA ADPCM block header: `i16` predictor, `u8` step index, one
/// reserved byte.
pub const IMA_BLOCK_HEADER_BYTES: u64 = 4;

/// Bytes of the MS ADPCM block header for one channel: `u8` coefficient index,
/// `i16` initial delta and the two `i16` history samples.
pub const MS_BLOCK_HEADER_BYTES_PER_CHANNEL: u64 = 7;

/// Entries in the ADPCM step codebook, [`STEP_TABLE`].
pub const STEP_TABLE_ENTRIES: usize = 89;

/// Highest step index the codebook has. A block's step index above it is a
/// corrupt header, not a value to clamp silently.
pub const MAX_STEP_INDEX: u8 = 88;

/// The ADPCM step codebook shared by both layouts: entry `n` is the quantisation
/// step the decoder uses at step index `n`.
///
/// This is the format's own table, not a table of this project's: it is what
/// `ff_adpcm_step_table` in FFmpeg's `libavcodec/adpcm_data.c` carries, which
/// that file documents as the ADPCM reference source's step table, and both
/// decoders were measured to agree value for value on every retail member (see
/// the module documentation).
pub const STEP_TABLE: [i16; STEP_TABLE_ENTRIES] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// How much a decoded nibble moves the step index, indexed by the nibble.
///
/// Nibbles 4 to 7 and 12 to 15 (the positive and negative magnitudes of a step
/// or more) raise the index; the rest lower it, and the low four nibbles are
/// the format's no-change-to-half-step group.
pub const INDEX_TABLE: [i8; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// How much MS ADPCM adapts its delta after a decoded nibble, indexed by the
/// nibble, in 256ths of the current delta.
pub const ADAPTATION_TABLE: [i16; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];

/// Smallest MS ADPCM delta the format allows. A delta below it is raised to it.
pub const MIN_MS_DELTA: i32 = 16;

/// Largest MS ADPCM delta this decoder keeps.
///
/// Beyond it two things are true at once: the `i16` predictor clip decides the
/// output on its own (a nonzero nibble moves the predictor by at least this
/// delta, which no `i16` can absorb), and `delta * nibble` would still have to
/// fit `i32`. The bound is the one FFmpeg's `adpcm_ms` decoder applies for the
/// same reason, to the same value, so a decode that reaches it agrees with that
/// implementation instead of drifting from it.
pub const MAX_MS_DELTA: i32 = i32::MAX / 768;

/// One MS ADPCM coefficient pair: the two weights the predictor applies to the
/// block's two history samples, divided by 256.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsAdpcmCoefficient {
    predictor: i16,
    difference: i16,
}

impl MsAdpcmCoefficient {
    /// The pair a member's `fmt ` table holds at one index.
    pub const fn new(predictor: i16, difference: i16) -> Self {
        Self {
            predictor,
            difference,
        }
    }

    /// `aCoefs[i][0]`, the weight of the newer history sample.
    pub const fn predictor(self) -> i16 {
        self.predictor
    }

    /// `aCoefs[i][1]`, the weight of the older history sample.
    pub const fn difference(self) -> i16 {
        self.difference
    }
}

/// The MS ADPCM coefficient table one member's own `fmt ` chunk declares.
///
/// The table is the member's, never this module's: a block's coefficient index
/// selects a pair from here, and an index the member's own table does not hold
/// is refused ([`AdpcmError::CoefficientOutOfRange`]) rather than replaced with
/// a pair from a table in this file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsAdpcmCoefficients {
    pairs: [MsAdpcmCoefficient; MS_COEFFICIENT_PAIRS],
    count: u16,
}

impl MsAdpcmCoefficients {
    /// A table of the first `count` pairs; `count` is clamped to the
    /// [`MS_COEFFICIENT_PAIRS`] the layout reserves.
    pub const fn new(pairs: [MsAdpcmCoefficient; MS_COEFFICIENT_PAIRS], count: u16) -> Self {
        let count = if count > MS_COEFFICIENT_PAIRS as u16 {
            MS_COEFFICIENT_PAIRS as u16
        } else {
            count
        };
        Self { pairs, count }
    }

    /// How many pairs the member declares (`wNumCoefs`).
    pub const fn count(self) -> u16 {
        self.count
    }

    /// The pair at `index`, when the member's own table holds it.
    pub fn get(self, index: u16) -> Option<MsAdpcmCoefficient> {
        if index >= self.count {
            return None;
        }
        self.pairs.get(usize::from(index)).copied()
    }

    /// The declared pairs, including any the member leaves unused.
    pub const fn pairs(&self) -> &[MsAdpcmCoefficient; MS_COEFFICIENT_PAIRS] {
        &self.pairs
    }
}

/// What one member's `fmt ` chunk says beyond the 16 common bytes.
///
/// A member that declares no ADPCM tag, or declares one and carries no extension
/// bytes, gets [`AdpcmExtension::Absent`]: the header still reads (task #344
/// keeps the tail inside `fmt_span` without interpreting it), and it is the
/// decode plan that refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdpcmExtension {
    /// IMA ADPCM: the member declares `wSamplesPerBlock`.
    Ima {
        /// The `wSamplesPerBlock` the member declares.
        samples_per_block: u16,
    },
    /// Microsoft ADPCM: the member declares `wSamplesPerBlock` and its own
    /// coefficient table.
    Ms {
        /// The `wSamplesPerBlock` the member declares.
        samples_per_block: u16,
        /// The `aCoefs` the member declares.
        coefficients: MsAdpcmCoefficients,
    },
    /// The member declares an ADPCM tag but its `fmt ` payload is shorter than
    /// the fields that tag documents, so no decode plan can be built from it.
    Short {
        /// The `wFormatTag` the member declares.
        tag: u16,
        /// Bytes the `fmt ` payload holds, common fields included.
        declared_len: u32,
        /// Bytes the tag's documented extension needs.
        needed: u32,
    },
    /// The member declares more MS ADPCM coefficient pairs than the layout
    /// reserves, so its table is one this module does not read.
    ///
    /// This is deliberately not [`Self::Short`]: the `fmt ` payload can be long
    /// enough for the pairs the member declares, and reporting a length
    /// shortfall that does not exist would be a false diagnosis.
    TooManyCoefficients {
        /// The `wFormatTag` the member declares.
        tag: u16,
        /// The `wNumCoefs` the member declares.
        declared: u16,
    },
    /// No ADPCM extension is declared or present.
    Absent,
}

impl AdpcmExtension {
    /// The layout this extension declares, or `None` when it declares none or
    /// is too short to declare one.
    pub const fn layout(&self) -> Option<AdpcmLayout> {
        match *self {
            Self::Ima { samples_per_block } => Some(AdpcmLayout::Ima { samples_per_block }),
            Self::Ms {
                samples_per_block,
                coefficients,
            } => Some(AdpcmLayout::Ms {
                samples_per_block,
                coefficients,
            }),
            Self::Short { .. } | Self::TooManyCoefficients { .. } | Self::Absent => None,
        }
    }
}

/// The block-coded sample layout one member's `fmt ` extension declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdpcmLayout {
    /// Intel/IMA (DVI) ADPCM, 4 bits per sample.
    Ima {
        /// The `wSamplesPerBlock` the member declares.
        samples_per_block: u16,
    },
    /// Microsoft ADPCM, 4 bits per sample, with the member's own coefficients.
    Ms {
        /// The `wSamplesPerBlock` the member declares.
        samples_per_block: u16,
        /// The `aCoefs` the member declares.
        coefficients: MsAdpcmCoefficients,
    },
}

impl fmt::Display for AdpcmLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ima { .. } => f.write_str("ima_adpcm"),
            Self::Ms { .. } => f.write_str("ms_adpcm"),
        }
    }
}

impl AdpcmLayout {
    /// The RFC 2361 name of the `wFormatTag` this layout decodes.
    pub const fn format_tag(self) -> u16 {
        match self {
            Self::Ima { .. } => WAVE_FORMAT_IMA_ADPCM,
            Self::Ms { .. } => WAVE_FORMAT_MS_ADPCM,
        }
    }

    /// The `wSamplesPerBlock` the member declares.
    pub const fn declared_samples_per_block(self) -> u16 {
        match self {
            Self::Ima { samples_per_block }
            | Self::Ms {
                samples_per_block, ..
            } => samples_per_block,
        }
    }

    /// Bytes of block header a block of `channels` declared channels starts
    /// with.
    pub const fn block_header_bytes(self, channels: u16) -> u64 {
        match self {
            Self::Ima { .. } => IMA_BLOCK_HEADER_BYTES,
            Self::Ms { .. } => channels as u64 * MS_BLOCK_HEADER_BYTES_PER_CHANNEL,
        }
    }

    /// How many sample values per channel the block of `bytes` bytes at
    /// `offset` inside the `data` payload holds.
    ///
    /// # Errors
    ///
    /// [`AdpcmError::UnsupportedChannels`] for a channel count this layout is
    /// not read for, [`AdpcmError::ShortBlock`] for a block that cannot hold
    /// its own header, and [`AdpcmError::PartialBlock`] for a block whose
    /// nibbles do not divide into whole per-channel groups.
    pub fn block_sample_count(
        self,
        channels: u16,
        offset: u64,
        bytes: u64,
    ) -> Result<u64, AdpcmError> {
        if !self.supports(channels) {
            return Err(AdpcmError::UnsupportedChannels {
                offset,
                channels,
                layout: self,
            });
        }
        let header = self.block_header_bytes(channels);
        if bytes < header {
            return Err(AdpcmError::ShortBlock {
                offset,
                bytes,
                header,
            });
        }
        let nibble_bytes = bytes - header;
        match self {
            // One initial predictor, then two samples per nibble byte.
            Self::Ima { .. } => Ok(1 + 2 * nibble_bytes),
            // Two history samples, then two samples per nibble byte, one byte
            // per channel group.
            Self::Ms { .. } => {
                let groups = u64::from(channels);
                if !nibble_bytes.is_multiple_of(groups) {
                    return Err(AdpcmError::PartialBlock {
                        offset,
                        bytes: nibble_bytes,
                        channels,
                    });
                }
                Ok(2 + 2 * (nibble_bytes / groups))
            }
        }
    }

    /// Whether this layout is read for `channels` declared channels.
    ///
    /// The channel counts are the ones the retail archives declare: IMA ADPCM
    /// members are mono and MS ADPCM members are mono or stereo. See the
    /// module documentation for what a wider block would need.
    pub const fn supports(self, channels: u16) -> bool {
        match self {
            Self::Ima { .. } => channels == 1,
            Self::Ms { .. } => channels == 1 || channels == 2,
        }
    }

    /// How many sample values the whole `data` payload of `payload_len` bytes
    /// holds, counted from the block geometry alone.
    ///
    /// This is the count [`Self::decode`] books against the parse's allocation
    /// budget *before* the buffer exists (spec F03), and it is checked against
    /// what the decode actually produces, so the two can never drift apart.
    ///
    /// # Errors
    ///
    /// As [`Self::block_sample_count`], for the block the payload ends in.
    pub fn sample_count(
        self,
        channels: u16,
        block_align: u64,
        payload_len: u64,
    ) -> Result<u64, AdpcmError> {
        if block_align == 0 || !self.supports(channels) {
            return Err(AdpcmError::UnsupportedChannels {
                offset: 0,
                channels,
                layout: self,
            });
        }
        let mut total = 0u64;
        let mut offset = 0u64;
        while offset < payload_len {
            let bytes = block_align.min(payload_len - offset);
            let per_channel = self.block_sample_count(channels, offset, bytes)?;
            total = total
                .checked_add(
                    per_channel
                        .checked_mul(u64::from(channels))
                        .ok_or(AdpcmError::CountOverflow { offset })?,
                )
                .ok_or(AdpcmError::CountOverflow { offset })?;
            offset += bytes;
        }
        Ok(total)
    }

    /// How many sample values the payload holds **for one channel of a full
    /// block**: the count the layout's geometry gives a `block_align`-byte
    /// block, which is what a member's `wSamplesPerBlock` declares.
    ///
    /// # Errors
    ///
    /// As [`Self::block_sample_count`].
    pub fn full_block_sample_count(
        self,
        channels: u16,
        block_align: u64,
    ) -> Result<u64, AdpcmError> {
        self.block_sample_count(channels, 0, block_align)
    }

    /// Decodes the `data` payload of one member into `out`, one value per
    /// sample, and returns the number of blocks it decoded.
    ///
    /// A block's values are appended in the order that block stores its data,
    /// which is **frame by frame and channel by channel**. An IMA block is one
    /// channel: its header's initial predictor is the first value and each byte
    /// holds the next two, low nibble first. An MS block opens with its two
    /// history values of every channel — the older one first, so the block's
    /// first two frames — and every byte after that is one frame, the first
    /// channel's next value in its high nibble and the second channel's in its
    /// low one, while a mono byte carries the channel's next two values, high
    /// one first. A block therefore holds `block_sample_count * channels`
    /// consecutive values, and a block-coded `DecodedSound`'s frames split that
    /// way.
    ///
    /// # Errors
    ///
    /// [`AdpcmError::UnsupportedChannels`], [`AdpcmError::ShortBlock`] or
    /// [`AdpcmError::PartialBlock`] for a block the layout cannot read,
    /// [`AdpcmError::StepIndexOutOfRange`] for an IMA block header whose step
    /// index is past the codebook, and
    /// [`AdpcmError::CoefficientOutOfRange`] for an MS block whose coefficient
    /// index the member's own table does not hold.
    pub fn decode(
        self,
        channels: u16,
        block_align: u64,
        payload: &[u8],
        out: &mut Vec<i32>,
    ) -> Result<u64, AdpcmError> {
        if block_align == 0 || !self.supports(channels) {
            return Err(AdpcmError::UnsupportedChannels {
                offset: 0,
                channels,
                layout: self,
            });
        }
        let align = usize::try_from(block_align).map_err(|_| AdpcmError::CountOverflow {
            offset: payload.len() as u64,
        })?;
        let mut blocks = 0u64;
        let mut offset = 0u64;
        while offset < payload.len() as u64 {
            let start =
                usize::try_from(offset).map_err(|_| AdpcmError::CountOverflow { offset })?;
            let end = start
                .checked_add(align)
                .ok_or(AdpcmError::CountOverflow { offset })?
                .min(payload.len());
            let block = &payload[start..end];
            match self {
                Self::Ima { .. } => decode_ima_block(block, offset, out)?,
                Self::Ms { coefficients, .. } => {
                    decode_ms_block(block, offset, self, channels, coefficients, out)?;
                }
            }
            blocks += 1;
            offset += block.len() as u64;
        }
        Ok(blocks)
    }
}

/// Why a compressed member's samples could not be decoded.
///
/// Every variant carries the block's byte offset and sizes, never sample bytes
/// (spec F03).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdpcmError {
    /// The block at `offset` is shorter than the header its own layout
    /// documents, so it cannot hold a predictor at all.
    ShortBlock {
        /// The block's offset inside the `data` payload.
        offset: u64,
        /// Bytes the block holds.
        bytes: u64,
        /// Bytes the layout's header needs.
        header: u64,
    },
    /// The block at `offset` does not hold whole per-channel groups of nibbles,
    /// so a declared channel would be left without its share of a byte.
    PartialBlock {
        /// The block's offset inside the `data` payload.
        offset: u64,
        /// Nibble bytes the block holds after its header.
        bytes: u64,
        /// The declared channel count.
        channels: u16,
    },
    /// A block names a coefficient index the member's own `aCoefs` table does
    /// not hold.
    CoefficientOutOfRange {
        /// The block's offset inside the `data` payload.
        offset: u64,
        /// The channel whose block header names the index.
        channel: u16,
        /// The index the block declares.
        index: u8,
        /// How many pairs the member's own table declares.
        declared: u16,
    },
    /// An IMA block header declares a step index past the codebook.
    StepIndexOutOfRange {
        /// The block's offset inside the `data` payload.
        offset: u64,
        /// The step index the block declares.
        step_index: u8,
    },
    /// The channel count this layout is not read for: no retail member declares
    /// it, and the layouts documented for wider blocks differ.
    UnsupportedChannels {
        /// The block's offset inside the `data` payload, 0 before the first.
        offset: u64,
        /// The declared channel count.
        channels: u16,
        /// The layout that was asked to read it.
        layout: AdpcmLayout,
    },
    /// The sample count of the payload does not fit a `u64`.
    CountOverflow {
        /// The block the count stopped at.
        offset: u64,
    },
}

impl AdpcmError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ShortBlock { .. } => "adpcm_short_block",
            Self::PartialBlock { .. } => "adpcm_partial_block",
            Self::CoefficientOutOfRange { .. } => "adpcm_coefficient_out_of_range",
            Self::StepIndexOutOfRange { .. } => "adpcm_step_index_out_of_range",
            Self::UnsupportedChannels { .. } => "adpcm_unsupported_channels",
            Self::CountOverflow { .. } => "adpcm_count_overflow",
        }
    }

    /// The block's offset inside the `data` payload.
    pub const fn offset(&self) -> u64 {
        match self {
            Self::ShortBlock { offset, .. }
            | Self::PartialBlock { offset, .. }
            | Self::CoefficientOutOfRange { offset, .. }
            | Self::StepIndexOutOfRange { offset, .. }
            | Self::UnsupportedChannels { offset, .. }
            | Self::CountOverflow { offset } => *offset,
        }
    }
}

impl fmt::Display for AdpcmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShortBlock {
                offset,
                bytes,
                header,
            } => write!(
                f,
                "at data offset {offset}: the block holds {bytes} bytes, its {layout} header needs \
                 {header}",
                layout = if *header == IMA_BLOCK_HEADER_BYTES {
                    "IMA ADPCM"
                } else {
                    "MS ADPCM"
                }
            ),
            Self::PartialBlock {
                offset,
                bytes,
                channels,
            } => write!(
                f,
                "at data offset {offset}: {bytes} nibble bytes are not a whole number of \
                 {channels}-channel groups"
            ),
            Self::CoefficientOutOfRange {
                offset,
                channel,
                index,
                declared,
            } => write!(
                f,
                "at data offset {offset}: channel {channel} names coefficient {index}, but the \
                 member's own table declares {declared} pairs"
            ),
            Self::StepIndexOutOfRange { offset, step_index } => write!(
                f,
                "at data offset {offset}: step index {step_index} is past the codebook's \
                 {STEP_TABLE_ENTRIES} entries"
            ),
            Self::UnsupportedChannels {
                offset,
                channels,
                layout,
            } => write!(
                f,
                "at data offset {offset}: {layout} is not read for {channels} channels; no retail \
                 member declares that shape"
            ),
            Self::CountOverflow { offset } => {
                write!(
                    f,
                    "at data offset {offset}: the sample count does not fit a u64"
                )
            }
        }
    }
}

impl std::error::Error for AdpcmError {}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn i16_at(bytes: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([bytes[at], bytes[at + 1]])
}

/// Clamps a predicted value to the `i16` range the format stores.
fn clip_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// Reads the `fmt ` extension an ADPCM member's own bytes declare.
///
/// `fmt` is the member's whole `fmt ` payload (the 16 common fields plus
/// whatever follows), exactly as located by
/// [`crate::zbd::wave::WaveHeader::fmt_span`]. A `wFormatTag` this module does
/// not decode, or a payload with no extension bytes, is
/// [`AdpcmExtension::Absent`]; a payload too short for its tag's documented
/// fields is [`AdpcmExtension::Short`], and a MS ADPCM payload declaring more
/// coefficient pairs than the layout reserves is
/// [`AdpcmExtension::TooManyCoefficients`]. No plan can be built from either.
pub fn read_adpcm_extension(tag: u16, fmt: &[u8]) -> AdpcmExtension {
    let extension = fmt.len().saturating_sub(FMT_COMMON_BYTES);
    let short = |needed: usize| AdpcmExtension::Short {
        tag,
        declared_len: u32::try_from(fmt.len()).unwrap_or(u32::MAX),
        needed: u32::try_from(FMT_COMMON_BYTES.saturating_add(needed)).unwrap_or(u32::MAX),
    };
    match tag {
        WAVE_FORMAT_IMA_ADPCM => {
            if extension < IMA_EXTENSION_BYTES {
                return if extension == 0 {
                    AdpcmExtension::Absent
                } else {
                    short(IMA_EXTENSION_BYTES)
                };
            }
            AdpcmExtension::Ima {
                samples_per_block: u16_at(fmt, FMT_COMMON_BYTES + CB_SIZE_BYTES),
            }
        }
        WAVE_FORMAT_MS_ADPCM => {
            if extension < MS_EXTENSION_BYTES {
                return if extension == 0 {
                    AdpcmExtension::Absent
                } else {
                    short(MS_EXTENSION_BYTES)
                };
            }
            let samples_per_block = u16_at(fmt, FMT_COMMON_BYTES + CB_SIZE_BYTES);
            let count = u16_at(fmt, FMT_COMMON_BYTES + CB_SIZE_BYTES + 2);
            if usize::from(count) > MS_COEFFICIENT_PAIRS {
                return AdpcmExtension::TooManyCoefficients {
                    tag,
                    declared: count,
                };
            }
            let table = FMT_COMMON_BYTES + MS_EXTENSION_BYTES;
            let needed = table + usize::from(count) * MS_COEFFICIENT_BYTES;
            if fmt.len() < needed {
                return short(needed - FMT_COMMON_BYTES);
            }
            // Only the pairs the member declares are read; the rest of the table
            // stays unused, so a member that declares fewer than the layout
            // reserves is read as it declares itself.
            let mut pairs = [MsAdpcmCoefficient::new(0, 0); MS_COEFFICIENT_PAIRS];
            for (index, pair) in pairs.iter_mut().take(count as usize).enumerate() {
                let at = table + index * MS_COEFFICIENT_BYTES;
                *pair = MsAdpcmCoefficient::new(i16_at(fmt, at), i16_at(fmt, at + 2));
            }
            AdpcmExtension::Ms {
                samples_per_block,
                coefficients: MsAdpcmCoefficients::new(pairs, count),
            }
        }
        _ => AdpcmExtension::Absent,
    }
}

/// One IMA ADPCM channel's decode state inside a block.
struct ImaChannel {
    predictor: i16,
    step_index: u8,
}

impl ImaChannel {
    fn expand(&mut self, nibble: u8) -> i16 {
        let step = i32::from(STEP_TABLE[usize::from(self.step_index)]);
        // The magnitude bits weigh a quarter step, a half step and a full step on
        // top of the eighth step every nibble moves at least.
        let mut diff = step >> 3;
        if nibble & 1 != 0 {
            diff += step >> 2;
        }
        if nibble & 2 != 0 {
            diff += step >> 1;
        }
        if nibble & 4 != 0 {
            diff += step;
        }
        let signed = i32::from(self.predictor) + if nibble & 8 != 0 { -diff } else { diff };
        self.predictor = clip_i16(signed);
        self.step_index = advance_step_index(self.step_index, nibble);
        self.predictor
    }
}

/// Moves the step index the format's index table says, clamped to the codebook.
fn advance_step_index(step_index: u8, nibble: u8) -> u8 {
    let moved = i32::from(step_index) + i32::from(INDEX_TABLE[usize::from(nibble)]);
    moved.clamp(0, i32::from(MAX_STEP_INDEX)) as u8
}

/// One MS ADPCM channel's decode state inside a block.
struct MsChannel {
    sample1: i16,
    sample2: i16,
    delta: i32,
    coefficient: MsAdpcmCoefficient,
}

impl MsChannel {
    fn expand(&mut self, nibble: u8) -> i16 {
        let predicted = (i32::from(self.sample1) * i32::from(self.coefficient.predictor())
            + i32::from(self.sample2) * i32::from(self.coefficient.difference()))
            / 256;
        // The nibble is a signed magnitude: bit 3 is its sign, so a nibble of 8
        // is -8, not +8. The subtraction is done in the signed type the sum is
        // computed in.
        let magnitude = if nibble & 8 != 0 {
            i32::from(nibble) - 16
        } else {
            i32::from(nibble)
        };
        let next = predicted + magnitude * self.delta;
        self.sample2 = self.sample1;
        self.sample1 = clip_i16(next);
        let adapted =
            (i64::from(ADAPTATION_TABLE[usize::from(nibble)]) * i64::from(self.delta)) >> 8;
        self.delta = adapted.clamp(i64::from(MIN_MS_DELTA), i64::from(MAX_MS_DELTA)) as i32;
        self.sample1
    }
}

/// Decodes one IMA ADPCM block: `i16` predictor, `u8` step index, one reserved
/// byte, then the nibbles low one first.
fn decode_ima_block(block: &[u8], offset: u64, out: &mut Vec<i32>) -> Result<(), AdpcmError> {
    if (block.len() as u64) < IMA_BLOCK_HEADER_BYTES {
        return Err(AdpcmError::ShortBlock {
            offset,
            bytes: block.len() as u64,
            header: IMA_BLOCK_HEADER_BYTES,
        });
    }
    let step_index = block[2];
    if usize::from(step_index) >= STEP_TABLE_ENTRIES {
        return Err(AdpcmError::StepIndexOutOfRange { offset, step_index });
    }
    let mut channel = ImaChannel {
        predictor: i16_at(block, 0),
        step_index,
    };
    out.push(i32::from(channel.predictor));
    for &byte in &block[IMA_BLOCK_HEADER_BYTES as usize..] {
        for nibble in [byte & 0x0F, byte >> 4] {
            out.push(i32::from(channel.expand(nibble)));
        }
    }
    Ok(())
}

/// Decodes one MS ADPCM block: seven header bytes per channel, grouped by field
/// across the channels, then the nibbles with the high one first.
fn decode_ms_block(
    block: &[u8],
    offset: u64,
    layout: AdpcmLayout,
    channels: u16,
    coefficients: MsAdpcmCoefficients,
    out: &mut Vec<i32>,
) -> Result<(), AdpcmError> {
    let count = usize::from(channels);
    if channels == 0 || channels > 2 {
        return Err(AdpcmError::UnsupportedChannels {
            offset,
            channels,
            layout,
        });
    }
    let header = count * MS_BLOCK_HEADER_BYTES_PER_CHANNEL as usize;
    if block.len() < header {
        return Err(AdpcmError::ShortBlock {
            offset,
            bytes: block.len() as u64,
            header: header as u64,
        });
    }
    let nibble_bytes = block.len() - header;
    if !nibble_bytes.is_multiple_of(count) {
        return Err(AdpcmError::PartialBlock {
            offset,
            bytes: nibble_bytes as u64,
            channels,
        });
    }

    let mut state = Vec::with_capacity(count);
    for channel in 0..count {
        let index = block[channel];
        let Some(coefficient) = coefficients.get(u16::from(index)) else {
            return Err(AdpcmError::CoefficientOutOfRange {
                offset,
                channel: channel as u16,
                index,
                declared: coefficients.count(),
            });
        };
        // The header fields are stored grouped by field across the channels:
        // every channel's coefficient index, then every channel's delta, then
        // every channel's newer history sample, then the older one.
        state.push(MsChannel {
            sample1: i16_at(block, count * 3 + channel * 2),
            sample2: i16_at(block, count * 5 + channel * 2),
            delta: i32::from(i16_at(block, count + channel * 2)),
            coefficient,
        });
    }
    // The two history values are the block's first two output frames, one value
    // per channel each, in the order the layout stores them: the older sample
    // of every channel, then the newer one.
    for channel in &state {
        out.push(i32::from(channel.sample2));
    }
    for channel in &state {
        out.push(i32::from(channel.sample1));
    }
    // Every remaining byte is one frame: the first channel's next value in its
    // high nibble and the second channel's in its low one. A mono byte carries
    // the channel's next two values, high one first.
    for &byte in &block[header..] {
        if count == 1 {
            out.push(i32::from(state[0].expand(byte >> 4)));
            out.push(i32::from(state[0].expand(byte & 0x0F)));
        } else {
            out.push(i32::from(state[0].expand(byte >> 4)));
            out.push(i32::from(state[1].expand(byte & 0x0F)));
        }
    }
    Ok(())
}
