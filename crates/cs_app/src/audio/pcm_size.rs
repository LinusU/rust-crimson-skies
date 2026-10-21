//! The decoded 16-bit PCM byte length of one sound-archive member.
//!
//! The original's Drydock sound buffers are sized by the **decoded** length of
//! the member, not by its stored `data` chunk. This is the lookup that turns a
//! member's own RIFF/WAVE declaration into that length so a measured buffer
//! size can be matched against the archive members that decode to it
//! (`docs/findings/2026-10-22-m01-audio-timeline-sample-identity.md`).
//!
//! The rule is computed from the `fmt ` declaration alone; no sample is
//! decoded:
//!
//! * PCM, 16 bit: the `data` length; 8 bit: twice the `data` length;
//! * Microsoft ADPCM (tag 2): `ceil(data / blockAlign)` blocks of
//!   `(blockAlign - 7 * ch) * 2 / ch + 2` samples per channel;
//! * IMA ADPCM (tag 17): the same block count of
//!   `(blockAlign - 4 * ch) * 8 / (4 * ch) + 1` samples per channel.
//!
//! A trailing partial block is counted as a whole block. Anything else is
//! refused by name, never guessed.

use cs_formats::zbd::{WaveError, read_wave_header};

const TAG_PCM: u16 = 1;
const TAG_MS_ADPCM: u16 = 2;
const TAG_IMA_ADPCM: u16 = 17;

/// Why a member's decoded length is not known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmSizeError {
    /// The member is not a RIFF/WAVE file the reader takes.
    Wave(WaveError),
    /// The member declares a format tag, channel count, block size or bit
    /// depth whose decoded length is not defined here.
    Unsupported {
        /// `wFormatTag`.
        format_tag: u16,
    },
}

/// The decoded size facts of one member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedPcmSize {
    /// `wFormatTag`.
    pub format_tag: u16,
    /// `nChannels`.
    pub channels: u16,
    /// `nSamplesPerSec`.
    pub rate_hz: u32,
    /// Bytes of decoded 16-bit PCM, partial trailing block counted whole.
    pub pcm_bytes: u64,
}

/// Computes the decoded 16-bit PCM byte length of one archive member.
///
/// # Errors
///
/// [`PcmSizeError`] when the header is refused or the declared layout has no
/// decoded length rule here.
pub fn decoded_pcm_size(member: &[u8]) -> Result<DecodedPcmSize, PcmSizeError> {
    let header = read_wave_header(member).map_err(PcmSizeError::Wave)?;
    let tag = header.format_tag();
    let channels = u64::from(header.channels());
    let align = u64::from(header.block_align());
    let data = header.data_span().length;
    let unsupported = PcmSizeError::Unsupported { format_tag: tag };
    if channels == 0 {
        return Err(unsupported);
    }
    let blocks = |per_block_header: u64, extra_samples: u64| -> Result<u64, PcmSizeError> {
        let overhead = per_block_header * channels;
        if align <= overhead {
            return Err(unsupported);
        }
        let blocks = data.div_ceil(align);
        // Two 4-bit samples per payload byte, plus the samples the block
        // header carries.
        let samples = (align - overhead) * 2 / channels + extra_samples;
        Ok(blocks * samples * 2 * channels)
    };
    let pcm_bytes = match tag {
        TAG_PCM => match header.bits_per_sample() {
            16 => data,
            8 => data * 2,
            _ => return Err(unsupported),
        },
        TAG_MS_ADPCM => blocks(7, 2)?,
        TAG_IMA_ADPCM => blocks(4, 1)?,
        _ => return Err(unsupported),
    };
    Ok(DecodedPcmSize {
        format_tag: tag,
        channels: header.channels(),
        rate_hz: header.rate_hz(),
        pcm_bytes,
    })
}
