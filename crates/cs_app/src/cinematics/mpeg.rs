//! The MPEG-1/MP2 decoder behind [`FrameSource`] (F40-D1).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-D1`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! The retail cinematics are MPEG-1 program streams (MPEG-1 video 320x240,
//! MP2 audio at 44.1 kHz; measured in
//! `docs/findings/2026-10-13-f40-d1-mpeg-decoder-approval-required.md`). The
//! owner approved the `slopmpeg` crate as the decoder dependency; it demuxes
//! and decodes both streams, so this module only adapts it to the playback
//! boundary:
//!
//! * [`MpegVideoSource`] is a [`FrameSource`]: the frames come out in
//!   presentation order with their presentation time, and the pixels of the
//!   frame last delivered are [`MpegVideoSource::current_rgba`].
//! * [`MpegAudioSource`] delivers the interleaved stereo samples that drive the
//!   master [`MediaClock`](super::playback::MediaClock).
//! * A file that is not there, a file that is not an MPEG-1 program stream and
//!   a stream that breaks part-way are each a [`MediaFailure`] (AC04): the
//!   player fails with a useful error and never completes.
//!
//! The two sources open the file separately, so audio and video are decoded
//! at the pace each consumer asks for.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;
use std::time::Duration;

use cs_sim::cinematic_state::MediaFailure;
use cs_types::content::ContentId;
use slopmpeg::{Decoder, PixelFormat};

use super::playback::{DecodeError, DecodedFrame, FrameSource};

/// The label of the format [`MediaFailure::MissingDecoder`] names when a file
/// is not an MPEG-1 program stream.
pub const MPEG_PS_FORMAT: &str = "mpeg-1 program stream";

/// The measured properties of a video stream, from its sequence header.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MpegVideoInfo {
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// Frames per second.
    pub frame_rate: f64,
}

/// Why a program stream could not be opened, as the player's failure.
///
/// `media` names what was looked for. A missing file is
/// [`MediaFailure::MissingMedia`]; a source that is not an MPEG-1 program
/// stream is [`MediaFailure::MissingDecoder`] (nothing here decodes it);
/// anything else (an unreadable file, a header the decoder rejects) is
/// [`MediaFailure::DecodeFailed`] with the decoder's message.
#[must_use]
pub fn open_failure(error: &slopmpeg::Error, media: &ContentId) -> MediaFailure {
    match error {
        slopmpeg::Error::Io(e) if e.kind() == io::ErrorKind::NotFound => {
            MediaFailure::MissingMedia {
                media: media.clone(),
            }
        }
        slopmpeg::Error::MissingPackHeader | slopmpeg::Error::MissingSystemHeader => {
            MediaFailure::MissingDecoder {
                format: MPEG_PS_FORMAT.to_owned(),
            }
        }
        other => MediaFailure::DecodeFailed {
            detail: other.to_string(),
        },
    }
}

fn open_file(path: &Path, media: &ContentId) -> Result<BufReader<File>, MediaFailure> {
    match File::open(path) {
        Ok(file) => Ok(BufReader::new(file)),
        Err(e) => Err(open_failure(&slopmpeg::Error::Io(e), media)),
    }
}

fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

/// The video of one program stream as a [`FrameSource`].
#[derive(Debug)]
pub struct MpegVideoSource<R> {
    decoder: Decoder<R>,
    info: MpegVideoInfo,
    next_index: u64,
    rgba: Vec<u8>,
    have_frame: bool,
}

impl MpegVideoSource<BufReader<File>> {
    /// Opens the video of the file at `path`; `media` names it in a failure.
    ///
    /// # Errors
    ///
    /// The [`MediaFailure`] of [`open_failure`].
    pub fn open(path: &Path, media: &ContentId) -> Result<Self, MediaFailure> {
        Self::new(open_file(path, media)?, media)
    }
}

impl<R: Read> MpegVideoSource<R> {
    /// The video of the program stream `reader` reads.
    ///
    /// # Errors
    ///
    /// The [`MediaFailure`] of [`open_failure`]; a stream without a video
    /// stream is a [`MediaFailure::DecodeFailed`].
    pub fn new(reader: R, media: &ContentId) -> Result<Self, MediaFailure> {
        let mut decoder = Decoder::new(reader).map_err(|e| open_failure(&e, media))?;
        decoder.set_audio_enabled(false);
        let info = decoder
            .video_info()
            .ok_or_else(|| MediaFailure::DecodeFailed {
                detail: format!("{media} has no video stream"),
            })?;
        Ok(Self {
            decoder,
            info: MpegVideoInfo {
                width: u32::try_from(info.width).unwrap_or(u32::MAX),
                height: u32::try_from(info.height).unwrap_or(u32::MAX),
                frame_rate: info.framerate,
            },
            next_index: 0,
            rgba: Vec::new(),
            have_frame: false,
        })
    }

    /// The video stream's measured properties.
    #[must_use]
    pub const fn info(&self) -> MpegVideoInfo {
        self.info
    }

    /// The pixels of the frame the last [`FrameSource::next_frame`] returned,
    /// as `width * height` RGBA texels in row order; `None` before the first
    /// frame and once the stream has ended.
    #[must_use]
    pub fn current_rgba(&self) -> Option<&[u8]> {
        self.have_frame.then_some(self.rgba.as_slice())
    }
}

impl<R: Read + Seek> MpegVideoSource<R> {
    /// The stream's duration, `None` when it cannot be determined.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] if the source cannot be read.
    pub fn duration(&mut self) -> Result<Option<Duration>, DecodeError> {
        self.decoder
            .duration()
            .map_err(|e| DecodeError(e.to_string()))
    }
}

impl<R: Read> FrameSource for MpegVideoSource<R> {
    fn next_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        match self.decoder.decode_video() {
            Ok(Some(frame)) => {
                self.rgba = frame.to_pixels(PixelFormat::Rgba);
                self.have_frame = true;
                let index = self.next_index;
                self.next_index += 1;
                Ok(Some(DecodedFrame {
                    pts_us: micros(frame.time()),
                    index,
                }))
            }
            Ok(None) => {
                self.have_frame = false;
                Ok(None)
            }
            Err(e) => {
                self.have_frame = false;
                Err(DecodeError(e.to_string()))
            }
        }
    }
}

/// The audio of one program stream: interleaved stereo `f32` samples.
///
/// A mono stream (`crimflag.mpg`) is delivered as two identical channels.
#[derive(Debug)]
pub struct MpegAudioSource<R> {
    decoder: Decoder<R>,
    sample_rate: u32,
}

impl MpegAudioSource<BufReader<File>> {
    /// Opens the audio of the file at `path`; `media` names it in a failure.
    ///
    /// # Errors
    ///
    /// The [`MediaFailure`] of [`open_failure`].
    pub fn open(path: &Path, media: &ContentId) -> Result<Self, MediaFailure> {
        Self::new(open_file(path, media)?, media)
    }
}

impl<R: Read> MpegAudioSource<R> {
    /// The audio of the program stream `reader` reads.
    ///
    /// # Errors
    ///
    /// The [`MediaFailure`] of [`open_failure`]; a stream without an audio
    /// stream is a [`MediaFailure::DecodeFailed`].
    pub fn new(reader: R, media: &ContentId) -> Result<Self, MediaFailure> {
        let mut decoder = Decoder::new(reader).map_err(|e| open_failure(&e, media))?;
        decoder.set_video_enabled(false);
        let info = decoder
            .audio_info()
            .ok_or_else(|| MediaFailure::DecodeFailed {
                detail: format!("{media} has no audio stream"),
            })?;
        Ok(Self {
            decoder,
            sample_rate: info.sample_rate,
        })
    }

    /// Samples per second, per channel.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The next chunk of interleaved stereo samples (left, right, left, ...)
    /// in `-1.0..=1.0`; `Ok(None)` is the end of the stream. The chunk is
    /// valid until the next call.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] when decoding fails part-way.
    pub fn next_samples(&mut self) -> Result<Option<&[f32]>, DecodeError> {
        match self.decoder.decode_audio() {
            Ok(samples) => Ok(samples.map(slopmpeg::Samples::interleaved)),
            Err(e) => Err(DecodeError(e.to_string())),
        }
    }
}
