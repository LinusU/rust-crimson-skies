//! The RIFF/WAVE header of one sound archive member (task #344).
//!
//! Task #340 found that the members of `ZBD/sounds*.zbd` are RIFF/WAVE files;
//! task #343 lists them from the archive trailer. [`read_wave_header`] reads
//! the header of one such member, and nothing more: it walks the chunk list
//! and reads the `fmt ` fields and the `cue ` point count. It does not decode
//! a single sample. The format sample decoding needs is F06-C's.
//!
//! The layout is the RIFF form of the *Multimedia Programming Interface and
//! Data Specifications 1.0* (IBM/Microsoft, August 1991), sections "RIFF
//! File Format" and "WAVE Form Type":
//!
//! * the member starts with `RIFF`, a u32 size and the form type `WAVE`. The
//!   size counts every byte after the size word, so a whole member is
//!   `size + 8` bytes;
//! * then chunks follow: a four-character id, a u32 size, the payload and a
//!   pad byte when the size is odd;
//! * `fmt ` carries u16 `wFormatTag`, u16 `nChannels`, u32 `nSamplesPerSec`,
//!   u32 `nAvgBytesPerSec`, u16 `nBlockAlign` and, as the first
//!   format-specific field, u16 `wBitsPerSample` (16 bytes). It comes before
//!   `data`;
//! * `cue ` carries u32 `dwCuePoints` and then 24 bytes per cue point.
//!
//! Format tags are named from RFC 2361 ("WAVE and AVI Codec Registries",
//! appendix A): `0x0001` PCM, `0x0002` Microsoft ADPCM and `0x0011`
//! Intel/IMA (DVI) ADPCM. These are the three tags the retail archives use
//! (task #344 findings). Any other tag is kept as a number and its name
//! stays unknown.
//!
//! Loop points are never read here. No retail member carries a `smpl` chunk,
//! and a `cue ` point is a single position, not a loop region. See
//! [`WaveHeader::loop_reason`].
//!
//! Every read is bounds-checked against the member slice and allocates
//! nothing, so a hostile member costs at most one pass over its chunk
//! headers. Offsets in errors and spans are relative to the member's first
//! byte.

use std::fmt;

use cs_types::evidence::SourceSpan;

/// Bytes of the `RIFF` header: id, size and form type.
pub const RIFF_HEADER_BYTES: usize = 12;

/// Bytes of a chunk header: id and size.
pub const CHUNK_HEADER_BYTES: usize = 8;

/// Bytes of the `fmt ` fields this reader takes, up to `wBitsPerSample`.
pub const FMT_BYTES: u32 = 16;

/// Bytes of the `cue ` point count.
pub const CUE_COUNT_BYTES: u32 = 4;

/// Bytes of one `cue ` point.
pub const CUE_POINT_BYTES: u32 = 24;

/// `wFormatTag` of PCM (RFC 2361 `WAVE_FORMAT_PCM`).
pub const WAVE_FORMAT_PCM: u16 = 0x0001;

/// `wFormatTag` of Microsoft ADPCM (RFC 2361 `WAVE_FORMAT_ADPCM`).
pub const WAVE_FORMAT_MS_ADPCM: u16 = 0x0002;

/// `wFormatTag` of Intel/IMA ADPCM (RFC 2361 `WAVE_FORMAT_DVI_ADPCM`).
pub const WAVE_FORMAT_IMA_ADPCM: u16 = 0x0011;

/// Why the format name of an unlisted tag is not known.
pub const UNNAMED_FORMAT_REASON: &str = "the WAVE format tag is not one of the RFC 2361 tags this \
     reader names (0x0001 PCM, 0x0002 Microsoft ADPCM, 0x0011 IMA ADPCM); the tag number is kept";

/// Why a member without a `smpl` chunk has no loop points.
pub const NO_LOOP_CHUNK_REASON: &str = "the member carries no `smpl` chunk, so its WAVE header \
     declares no loop region; whether the game loops it is decided elsewhere and is unknown";

/// Why a member's `smpl` chunk is not read.
pub const SMPL_NOT_READ_REASON: &str = "the member carries a `smpl` chunk, but no retail member \
     does, so this reader has no checked layout for it and reads no loop points";

/// The name of a WAVE format tag, when RFC 2361 names it and the retail
/// archives use it.
pub const fn format_name(tag: u16) -> Option<&'static str> {
    match tag {
        WAVE_FORMAT_PCM => Some("pcm"),
        WAVE_FORMAT_MS_ADPCM => Some("ms_adpcm"),
        WAVE_FORMAT_IMA_ADPCM => Some("ima_adpcm"),
        _ => None,
    }
}

/// What the RIFF/WAVE header of one member declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveHeader {
    format_tag: u16,
    channels: u16,
    rate_hz: u32,
    avg_bytes_per_sec: u32,
    block_align: u16,
    bits_per_sample: u16,
    fmt: SourceSpan,
    data: SourceSpan,
    cue_points: u32,
    smpl: Option<SourceSpan>,
    other_chunks: u32,
}

impl WaveHeader {
    /// `wFormatTag`.
    pub const fn format_tag(&self) -> u16 {
        self.format_tag
    }

    /// The format name, when [`format_name`] knows the tag.
    pub const fn format_name(&self) -> Option<&'static str> {
        format_name(self.format_tag)
    }

    /// `nChannels`.
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// `nSamplesPerSec`, in hertz.
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// `nAvgBytesPerSec`.
    pub const fn avg_bytes_per_sec(&self) -> u32 {
        self.avg_bytes_per_sec
    }

    /// `nBlockAlign`, in bytes.
    pub const fn block_align(&self) -> u16 {
        self.block_align
    }

    /// `wBitsPerSample`.
    pub const fn bits_per_sample(&self) -> u16 {
        self.bits_per_sample
    }

    /// The `fmt ` payload inside the member, format-specific bytes included.
    pub const fn fmt_span(&self) -> SourceSpan {
        self.fmt
    }

    /// The `data` payload inside the member.
    pub const fn data_span(&self) -> SourceSpan {
        self.data
    }

    /// `dwCuePoints` of the `cue ` chunk, or 0 when there is none.
    pub const fn cue_points(&self) -> u32 {
        self.cue_points
    }

    /// The `smpl` payload inside the member, when there is one. Its contents
    /// are not read.
    pub const fn smpl_span(&self) -> Option<SourceSpan> {
        self.smpl
    }

    /// Chunks other than `fmt `, `data`, `cue ` and `smpl`, skipped unread.
    pub const fn other_chunks(&self) -> u32 {
        self.other_chunks
    }

    /// Why no loop points are known for this member.
    pub const fn loop_reason(&self) -> &'static str {
        if self.smpl.is_some() {
            SMPL_NOT_READ_REASON
        } else {
            NO_LOOP_CHUNK_REASON
        }
    }
}

/// Why a member is not a RIFF/WAVE file this reader can take.
///
/// Carries ids, sizes and member-relative offsets, never sample bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveError {
    /// The member is shorter than the 12-byte `RIFF` header.
    TooShort {
        /// Bytes the member has.
        member_len: u64,
    },
    /// The first four bytes are not `RIFF`.
    NotRiff {
        /// The four bytes found.
        found: [u8; 4],
    },
    /// The form type is not `WAVE`.
    NotWave {
        /// The four bytes found at offset 8.
        found: [u8; 4],
    },
    /// The `RIFF` size plus 8 is not the member length.
    RiffSizeMismatch {
        /// The declared `RIFF` size.
        declared: u32,
        /// Bytes the member has.
        member_len: u64,
    },
    /// Fewer than 8 bytes remain for a chunk header.
    TruncatedChunkHeader {
        /// Offset of the chunk header.
        offset: u64,
        /// Bytes left in the member.
        remaining: u64,
    },
    /// A chunk's payload runs past the end of the member.
    ChunkOutOfBounds {
        /// The chunk id.
        id: [u8; 4],
        /// Offset of the chunk header.
        offset: u64,
        /// The declared payload size.
        size: u32,
        /// Payload bytes the member has after the chunk header.
        available: u64,
    },
    /// A chunk that may occur once occurs again.
    DuplicateChunk {
        /// The chunk id.
        id: [u8; 4],
        /// Offset of the second chunk header.
        offset: u64,
    },
    /// The `fmt ` payload is shorter than the 16 bytes read here.
    FmtTooShort {
        /// Offset of the `fmt ` chunk header.
        offset: u64,
        /// The declared payload size.
        size: u32,
    },
    /// The `cue ` payload is shorter than its declared cue points need.
    CueTooShort {
        /// Offset of the `cue ` chunk header.
        offset: u64,
        /// The declared payload size.
        size: u32,
        /// The declared cue point count, when the payload holds it.
        points: Option<u32>,
    },
    /// `data` comes before `fmt `.
    DataBeforeFmt {
        /// Offset of the `data` chunk header.
        offset: u64,
    },
    /// There is no `fmt ` chunk.
    MissingFmt,
    /// There is no `data` chunk.
    MissingData,
}

impl WaveError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TooShort { .. } => "wave_too_short",
            Self::NotRiff { .. } => "not_riff",
            Self::NotWave { .. } => "not_wave",
            Self::RiffSizeMismatch { .. } => "riff_size_mismatch",
            Self::TruncatedChunkHeader { .. } => "truncated_chunk_header",
            Self::ChunkOutOfBounds { .. } => "chunk_out_of_bounds",
            Self::DuplicateChunk { .. } => "duplicate_chunk",
            Self::FmtTooShort { .. } => "fmt_too_short",
            Self::CueTooShort { .. } => "cue_too_short",
            Self::DataBeforeFmt { .. } => "data_before_fmt",
            Self::MissingFmt => "missing_fmt",
            Self::MissingData => "missing_data",
        }
    }

    /// Why the member's descriptor fields are unknown, for
    /// [`crate::zbd::SoundField::Unknown`].
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::TooShort { .. } => "the member is shorter than a RIFF header",
            Self::NotRiff { .. } => "the member does not start with `RIFF`",
            Self::NotWave { .. } => "the member's RIFF form type is not `WAVE`",
            Self::RiffSizeMismatch { .. } => "the member's RIFF size does not match its length",
            Self::TruncatedChunkHeader { .. } => "the member ends inside a chunk header",
            Self::ChunkOutOfBounds { .. } => "a chunk of the member runs past its end",
            Self::DuplicateChunk { .. } => {
                "the member repeats a `fmt `, `data`, `cue ` or `smpl` chunk"
            }
            Self::FmtTooShort { .. } => "the member's `fmt ` chunk is shorter than 16 bytes",
            Self::CueTooShort { .. } => "the member's `cue ` chunk is shorter than its cue points",
            Self::DataBeforeFmt { .. } => "the member's `data` chunk comes before `fmt `",
            Self::MissingFmt => "the member has no `fmt ` chunk",
            Self::MissingData => "the member has no `data` chunk",
        }
    }
}

impl fmt::Display for WaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let id = |id: &[u8; 4]| id.escape_ascii().to_string();
        match self {
            Self::TooShort { member_len } => {
                write!(
                    f,
                    "{member_len} bytes, a RIFF header needs {RIFF_HEADER_BYTES}"
                )
            }
            Self::NotRiff { found } => write!(f, "starts with `{}`, not `RIFF`", id(found)),
            Self::NotWave { found } => write!(f, "RIFF form `{}`, not `WAVE`", id(found)),
            Self::RiffSizeMismatch {
                declared,
                member_len,
            } => write!(
                f,
                "RIFF size {declared} + 8 is not the member length {member_len}"
            ),
            Self::TruncatedChunkHeader { offset, remaining } => write!(
                f,
                "at offset {offset}: {remaining} bytes left, a chunk header needs \
                 {CHUNK_HEADER_BYTES}"
            ),
            Self::ChunkOutOfBounds {
                id: chunk,
                offset,
                size,
                available,
            } => write!(
                f,
                "at offset {offset}: chunk `{}` declares {size} bytes, {available} remain",
                id(chunk)
            ),
            Self::DuplicateChunk { id: chunk, offset } => {
                write!(f, "at offset {offset}: a second `{}` chunk", id(chunk))
            }
            Self::FmtTooShort { offset, size } => write!(
                f,
                "at offset {offset}: `fmt ` has {size} bytes, {FMT_BYTES} are read"
            ),
            Self::CueTooShort {
                offset,
                size,
                points,
            } => match points {
                Some(points) => write!(
                    f,
                    "at offset {offset}: `cue ` has {size} bytes, {points} cue points need more"
                ),
                None => write!(
                    f,
                    "at offset {offset}: `cue ` has {size} bytes, its count needs \
                     {CUE_COUNT_BYTES}"
                ),
            },
            Self::DataBeforeFmt { offset } => {
                write!(f, "at offset {offset}: `data` before `fmt `")
            }
            Self::MissingFmt => write!(f, "no `fmt ` chunk"),
            Self::MissingData => write!(f, "no `data` chunk"),
        }
    }
}

impl std::error::Error for WaveError {}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn id_at(bytes: &[u8], at: usize) -> [u8; 4] {
    [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
}

/// The fields of a `fmt ` payload that is at least [`FMT_BYTES`] long.
struct Fmt {
    format_tag: u16,
    channels: u16,
    rate_hz: u32,
    avg_bytes_per_sec: u32,
    block_align: u16,
    bits_per_sample: u16,
}

/// Reads the RIFF/WAVE header of one sound archive member.
///
/// `member` is the member's bytes exactly as the archive stores them.
///
/// # Errors
///
/// A [`WaveError`] naming the first thing that breaks the layout described
/// in the module documentation.
pub fn read_wave_header(member: &[u8]) -> Result<WaveHeader, WaveError> {
    let member_len = member.len() as u64;
    if member.len() < RIFF_HEADER_BYTES {
        return Err(WaveError::TooShort { member_len });
    }
    let riff = id_at(member, 0);
    if &riff != b"RIFF" {
        return Err(WaveError::NotRiff { found: riff });
    }
    let form = id_at(member, 8);
    if &form != b"WAVE" {
        return Err(WaveError::NotWave { found: form });
    }
    let declared = u32_at(member, 4);
    if u64::from(declared) + 8 != member_len {
        return Err(WaveError::RiffSizeMismatch {
            declared,
            member_len,
        });
    }

    let mut fmt: Option<(SourceSpan, Fmt)> = None;
    let mut data: Option<SourceSpan> = None;
    let mut cue: Option<u32> = None;
    let mut smpl: Option<SourceSpan> = None;
    let mut other_chunks = 0u32;

    let mut at = RIFF_HEADER_BYTES;
    while at < member.len() {
        let offset = at as u64;
        let remaining = member.len() - at;
        if remaining < CHUNK_HEADER_BYTES {
            return Err(WaveError::TruncatedChunkHeader {
                offset,
                remaining: remaining as u64,
            });
        }
        let id = id_at(member, at);
        let size = u32_at(member, at + 4);
        let available = (remaining - CHUNK_HEADER_BYTES) as u64;
        if u64::from(size) > available {
            return Err(WaveError::ChunkOutOfBounds {
                id,
                offset,
                size,
                available,
            });
        }
        let start = at + CHUNK_HEADER_BYTES;
        // `size <= available`, so the payload lies inside `member`.
        let payload = &member[start..start + size as usize];
        let span = SourceSpan {
            offset: start as u64,
            length: u64::from(size),
        };
        let duplicate = || WaveError::DuplicateChunk { id, offset };
        match &id {
            b"fmt " => {
                if fmt.is_some() {
                    return Err(duplicate());
                }
                if size < FMT_BYTES {
                    return Err(WaveError::FmtTooShort { offset, size });
                }
                fmt = Some((
                    span,
                    Fmt {
                        format_tag: u16_at(payload, 0),
                        channels: u16_at(payload, 2),
                        rate_hz: u32_at(payload, 4),
                        avg_bytes_per_sec: u32_at(payload, 8),
                        block_align: u16_at(payload, 12),
                        bits_per_sample: u16_at(payload, 14),
                    },
                ));
            }
            b"data" => {
                if data.is_some() {
                    return Err(duplicate());
                }
                if fmt.is_none() {
                    return Err(WaveError::DataBeforeFmt { offset });
                }
                data = Some(span);
            }
            b"cue " => {
                if cue.is_some() {
                    return Err(duplicate());
                }
                if size < CUE_COUNT_BYTES {
                    return Err(WaveError::CueTooShort {
                        offset,
                        size,
                        points: None,
                    });
                }
                let points = u32_at(payload, 0);
                let needed =
                    u64::from(CUE_COUNT_BYTES) + u64::from(points) * u64::from(CUE_POINT_BYTES);
                if needed > u64::from(size) {
                    return Err(WaveError::CueTooShort {
                        offset,
                        size,
                        points: Some(points),
                    });
                }
                cue = Some(points);
            }
            b"smpl" => {
                if smpl.is_some() {
                    return Err(duplicate());
                }
                smpl = Some(span);
            }
            _ => other_chunks = other_chunks.saturating_add(1),
        }
        // An odd payload is followed by a pad byte. A pad byte missing at the
        // very end of the member ends the walk; the payload itself was in
        // bounds.
        at = start
            .saturating_add(size as usize)
            .saturating_add(size as usize & 1);
    }

    let (fmt_span, fmt) = fmt.ok_or(WaveError::MissingFmt)?;
    let data = data.ok_or(WaveError::MissingData)?;
    Ok(WaveHeader {
        format_tag: fmt.format_tag,
        channels: fmt.channels,
        rate_hz: fmt.rate_hz,
        avg_bytes_per_sec: fmt.avg_bytes_per_sec,
        block_align: fmt.block_align,
        bits_per_sample: fmt.bits_per_sample,
        fmt: fmt_span,
        data,
        cue_points: cue.unwrap_or(0),
        smpl,
        other_chunks,
    })
}
