//! Original cinematic playback on the real audio device clock (F40-D4).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-D`. Findings: `docs/findings/2026-10-18-f40-d4-device-playback.md`.
//!
//! [`play_on_device`] joins the three production halves that F40-B/C/D1 left
//! apart:
//!
//! * the decoder ([`MpegAudioSource`], [`MpegVideoSource`]);
//! * the real output stream ([`AudibleDevice`], opened behind the `audio`
//!   capability gate): the decoded MP2 audio is one voice, and **how far the
//!   output stream has pulled that voice** is the master clock — samples
//!   actually played, not wall time;
//! * the [`CinematicSession`], stepped with the samples the device played, so
//!   video follows audio ([`VideoPlayback`]) and a failure is never a
//!   completion.
//!
//! Each frame the session shows is composed into the output surface at its own
//! aspect ratio ([`compose_letterboxed`], the pixels of
//! [`super::fit_letterboxed`]'s viewport, bars opaque black, never stretched)
//! and handed to a [`FramePresenter`]. The GPU presenter is
//! [`super::frame_capture`].
//!
//! ### Not retail semantics
//!
//! The retail pause policy, master clock and skippability are measured
//! unknowns (F40-D2), so no retail plan can be lowered. The plan this driver
//! runs is a **playback-probe plan**: its length is the decoded audio length,
//! it is not skippable and has no actions. It is a designed scaffold around
//! the real media, not a statement about the original's scene rules.

use std::collections::VecDeque;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cs_sim::audio_events::{AudioBus, AudioDevice, DeviceError, VoiceStart};
use cs_sim::cinematic_state::{
    CinematicScript, CinematicState, FailureRecovery, MasterClock, MediaFailure, PausePolicy,
    SYNTHETIC_PLAYER,
};
use cs_types::content::ContentId;

use super::mpeg::{MPEG_PS_FORMAT, MpegAudioSource, MpegVideoSource};
use super::playback::{DecodeError, DecodedFrame, FrameSource, MediaClock, VideoPlayback};
use super::session::{CinematicSession, MissionHandback, SessionError};
use super::{
    CinematicPlan, EmptyExtent, MediaAvailability, PresentationPlan, Viewport, fit_letterboxed,
};
use crate::audio::device::{CapabilityDeclaration, InMemorySamples, PcmAudio, open_audible_device};

/// Cinematic ticks per second of the probe plan (one per millisecond).
pub const PROBE_TICKS_PER_SECOND: u32 = 1000;

/// The drift tolerance of the probe: 100 ms, a designed value (the original's
/// is unmeasured).
pub const PROBE_DRIFT_TOLERANCE_US: u64 = 100_000;

/// How long the loop sleeps between polls of the device position.
const POLL: Duration = Duration::from_millis(4);

/// The output position stops short of the audio's end by up to a buffer; a
/// position within `1/END_TAIL_DIVISOR` s of the end that stops advancing for
/// [`END_STALL`] is the end of the audio.
pub(super) const END_TAIL_DIVISOR: u32 = 20;
pub(super) const END_STALL: Duration = Duration::from_millis(250);

/// How long past the audio length the device may take before the run is a
/// failure (an output that stopped pulling is not a completion).
pub(super) const DEVICE_SLACK: Duration = Duration::from_secs(10);

/// Why a frame could not be composed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposeError {
    /// A zero-sized frame or surface.
    Empty,
    /// The pixel buffer is not `width * height * 4` bytes.
    WrongLength {
        /// Bytes expected.
        expected: usize,
        /// Bytes given.
        got: usize,
    },
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the frame and the surface must both be non-empty"),
            Self::WrongLength { expected, got } => {
                write!(f, "the frame holds {got} bytes, {expected} were expected")
            }
        }
    }
}

impl std::error::Error for ComposeError {}

impl From<EmptyExtent> for ComposeError {
    fn from(_: EmptyExtent) -> Self {
        Self::Empty
    }
}

/// Composes an RGBA `frame` of `frame_size` into a `surface_size` RGBA buffer:
/// the frame is scaled (nearest) into the [`fit_letterboxed`] viewport at its
/// own aspect ratio and the rest is opaque black. Also returns the viewport.
///
/// # Errors
///
/// [`ComposeError`] for an empty extent or a frame buffer of the wrong length.
pub fn compose_letterboxed(
    frame: &[u8],
    frame_size: (u32, u32),
    surface_size: (u32, u32),
) -> Result<(Vec<u8>, Viewport), ComposeError> {
    let viewport = fit_letterboxed(frame_size, surface_size)?;
    let expected = frame_size.0 as usize * frame_size.1 as usize * 4;
    if frame.len() != expected {
        return Err(ComposeError::WrongLength {
            expected,
            got: frame.len(),
        });
    }
    let (sw, sh) = (surface_size.0 as usize, surface_size.1 as usize);
    let mut out = vec![0_u8; sw * sh * 4];
    for pixel in out.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    let (fw, fh) = (frame_size.0 as usize, frame_size.1 as usize);
    let (vw, vh) = (viewport.width as usize, viewport.height as usize);
    let columns: Vec<usize> = (0..vw).map(|col| (col * fw / vw) * 4).collect();
    for row in 0..vh {
        let src_row = &frame[(row * fh / vh) * fw * 4..][..fw * 4];
        let dst_start = ((viewport.y as usize + row) * sw + viewport.x as usize) * 4;
        for (dst, src) in out[dst_start..dst_start + vw * 4]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(&columns)
        {
            dst.copy_from_slice(&src_row[*src..*src + 4]);
        }
    }
    Ok((out, viewport))
}

/// One frame the session showed, composed for the output surface.
#[derive(Debug)]
pub struct ShownFrame<'a> {
    /// The frame's index in decode order.
    pub index: u64,
    /// Its presentation time in microseconds.
    pub pts_us: u64,
    /// The audio master clock when it was shown, in microseconds.
    pub clock_us: u64,
    /// The surface size.
    pub surface_size: (u32, u32),
    /// Where the picture sits inside the surface.
    pub viewport: Viewport,
    /// The composed surface, RGBA.
    pub surface: &'a [u8],
}

/// The consumer of composed frames.
pub trait FramePresenter {
    /// Shows `frame`. An `Err` stops playback and is reported, never ignored.
    ///
    /// # Errors
    ///
    /// A message naming what the presenter could not do.
    fn present(&mut self, frame: &ShownFrame<'_>) -> Result<(), String>;
}

/// Why device playback did not finish.
#[derive(Debug)]
pub enum DevicePlaybackError {
    /// The media could not be opened or decoded: a useful error, no completion.
    Media(MediaFailure),
    /// The output stream could not be opened or refused the voice.
    Device(DeviceError),
    /// The audio could not be turned into device samples.
    Audio(String),
    /// The session refused a step.
    Session(SessionError),
    /// The presenter failed.
    Present(String),
    /// The surface was empty or the frame inconsistent.
    Compose(ComposeError),
    /// The output stopped pulling samples before the audio ended.
    DeviceStalled {
        /// Frames the device had played.
        played_frames: u64,
        /// Frames the audio holds.
        total_frames: u64,
    },
}

impl fmt::Display for DevicePlaybackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Media(failure) => write!(f, "the cinematic media failed: {failure:?}"),
            Self::Device(error) => write!(f, "audio device: {} ({})", error.detail, error.code),
            Self::Audio(detail) => write!(f, "cinematic audio unusable: {detail}"),
            Self::Session(error) => write!(f, "cinematic session: {error:?}"),
            Self::Present(detail) => write!(f, "frame presenter: {detail}"),
            Self::Compose(error) => write!(f, "frame composition: {error}"),
            Self::DeviceStalled {
                played_frames,
                total_frames,
            } => write!(
                f,
                "the output stopped after {played_frames} of {total_frames} audio frames"
            ),
        }
    }
}

impl std::error::Error for DevicePlaybackError {}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackEnd {
    /// The device played all the audio and the session completed.
    Completed,
    /// The caller's time limit stopped the run first. Not a completion.
    TimeLimit,
}

/// What one device playback measured.
#[derive(Clone, Debug, PartialEq)]
pub struct DevicePlaybackReport {
    /// The media played.
    pub media: ContentId,
    /// How it ended.
    pub end: PlaybackEnd,
    /// The session's final state.
    pub state: CinematicState,
    /// Source video size.
    pub source_size: (u32, u32),
    /// Video frame rate from the sequence header.
    pub frame_rate: f64,
    /// Audio sample rate (frames per second).
    pub sample_rate: u32,
    /// Audio frames the file decodes to.
    pub total_audio_frames: u64,
    /// Audio frames the output stream had pulled when the run ended.
    pub played_audio_frames: u64,
    /// Distinct frames handed to the presenter.
    pub frames_presented: u64,
    /// The largest `|frame pts - audio clock|` observed, microseconds.
    pub max_abs_drift_us: u64,
    /// Whether every observed drift was within [`PROBE_DRIFT_TOLERANCE_US`].
    pub drift_within_tolerance: bool,
    /// The surface size.
    pub surface_size: (u32, u32),
    /// Where the picture sat inside it.
    pub viewport: Viewport,
    /// Wall-clock length of the run.
    pub wall: Duration,
}

pub(super) type SharedFrames = Arc<Mutex<VecDeque<(u64, Vec<u8>)>>>;

/// A [`FrameSource`] that keeps the pixels of the frames it has delivered, so
/// the pixels of the *shown* frame are still available when the decoder has
/// already looked one frame ahead.
pub(super) struct TeeSource<R> {
    pub(super) inner: MpegVideoSource<R>,
    pub(super) frames: SharedFrames,
}

impl<R: std::io::Read> FrameSource for TeeSource<R> {
    fn next_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        let frame = self.inner.next_frame()?;
        if let (Some(frame), Some(rgba)) = (&frame, self.inner.current_rgba()) {
            let mut frames = self.frames.lock().unwrap_or_else(PoisonError::into_inner);
            frames.push_back((frame.index, rgba.to_vec()));
            while frames.len() > 8 {
                frames.pop_front();
            }
        }
        Ok(frame)
    }
}

/// Plays the MPEG program stream at `path` through the real output device and
/// hands each shown frame, letterboxed into `surface_size`, to `presenter`.
///
/// `limit` stops the run early with [`PlaybackEnd::TimeLimit`] (never a
/// completion). `declaration` is the `$CS_CAPABILITIES` declaration: without
/// `audio` the device is refused by name before any hardware call.
///
/// # Errors
///
/// [`DevicePlaybackError`]; a missing file or a file that is not MPEG-1 is
/// [`DevicePlaybackError::Media`].
pub fn play_on_device(
    path: &Path,
    media: &ContentId,
    declaration: &CapabilityDeclaration,
    surface_size: (u32, u32),
    limit: Option<Duration>,
    presenter: &mut impl FramePresenter,
) -> Result<DevicePlaybackReport, DevicePlaybackError> {
    let video = MpegVideoSource::open(path, media).map_err(DevicePlaybackError::Media)?;
    let info = video.info();
    let mut audio_source =
        MpegAudioSource::open(path, media).map_err(DevicePlaybackError::Media)?;
    let rate = audio_source.sample_rate();
    let mut samples = Vec::new();
    while let Some(chunk) = audio_source
        .next_samples()
        .map_err(|e| DevicePlaybackError::Media(MediaFailure::DecodeFailed { detail: e.0 }))?
    {
        samples.extend_from_slice(chunk);
    }
    let pcm = PcmAudio::try_new(2, rate, samples)
        .map_err(|e| DevicePlaybackError::Audio(format!("{e:?}")))?;
    let total_frames = pcm.frames();

    let mut library = InMemorySamples::new();
    library.insert(media.clone(), pcm);
    let mut device =
        open_audible_device(Arc::new(library), declaration).map_err(DevicePlaybackError::Device)?;

    let frames: SharedFrames = Arc::default();
    let tee = TeeSource {
        inner: video,
        frames: Arc::clone(&frames),
    };
    let duration_ticks = total_frames * u64::from(PROBE_TICKS_PER_SECOND) / u64::from(rate);
    let script = CinematicScript::try_new(
        media.clone(),
        duration_ticks.max(1),
        false,
        PausePolicy::SimulationRuns,
        MasterClock::Audio,
        FailureRecovery::Block,
        Vec::new(),
    )
    .map_err(|e| DevicePlaybackError::Audio(format!("{e:?}")))?;
    let plan = CinematicPlan {
        script,
        presentation: PresentationPlan::Video {
            media: media.clone(),
            format: MPEG_PS_FORMAT.to_owned(),
            frame_size: (info.width, info.height),
        },
    };
    let clock = MediaClock::new(rate).map_err(|e| DevicePlaybackError::Audio(e.to_string()))?;
    let mut sink = MissionHandback::new(SYNTHETIC_PLAYER);
    let mut session = CinematicSession::begin(
        plan,
        &MediaAvailability::Present,
        Some(VideoPlayback::new(tee, PROBE_DRIFT_TOLERANCE_US)),
        clock,
        PROBE_TICKS_PER_SECOND,
        SYNTHETIC_PLAYER,
        &mut sink,
    )
    .map_err(DevicePlaybackError::Session)?;

    let voice = device
        .start_one_shot(VoiceStart {
            asset: media.clone(),
            bus: AudioBus::Music,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        })
        .map_err(DevicePlaybackError::Device)?;

    let started = Instant::now();
    let audio_len = Duration::from_micros(total_frames * 1_000_000 / u64::from(rate));
    let mut presented = 0_u64;
    let mut last_index = None;
    let mut max_drift = 0_u64;
    let mut viewport = fit_letterboxed((info.width, info.height), surface_size)
        .map_err(|e| DevicePlaybackError::Compose(e.into()))?;
    let mut last_played = 0_u64;
    let mut last_advance = Instant::now();
    let end = loop {
        std::thread::sleep(POLL);
        let position = device.played_position(voice).unwrap_or_default();
        let mut played = frames_of(position, rate).min(total_frames);
        if played != last_played {
            last_played = played;
            last_advance = Instant::now();
        } else if played > 0
            && played + u64::from(rate / END_TAIL_DIVISOR) >= total_frames
            && last_advance.elapsed() >= END_STALL
        {
            // The output reports the position it last pulled a whole buffer
            // to, so the final partial buffer never shows in it: a position
            // that stops within the tail of the audio is the end of the audio.
            played = total_frames;
        }
        let delta = played.saturating_sub(session.clock().played_samples());
        if delta > 0 {
            session
                .step(delta, &mut sink)
                .map_err(DevicePlaybackError::Session)?;
        }
        if let CinematicState::Failed { reason, .. } = session.state() {
            return Err(DevicePlaybackError::Media(reason.clone()));
        }
        if let Some(video) = session.video() {
            if let Some(report) = video.drift(session.clock()) {
                max_drift = max_drift.max(report.drift_us.unsigned_abs());
            }
            if let Some(shown) = video.shown()
                && last_index != Some(shown.index)
            {
                last_index = Some(shown.index);
                let pixels = frames
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .iter()
                    .find(|(index, _)| *index == shown.index)
                    .map(|(_, rgba)| rgba.clone());
                if let Some(pixels) = pixels {
                    let (surface, placed) =
                        compose_letterboxed(&pixels, (info.width, info.height), surface_size)
                            .map_err(DevicePlaybackError::Compose)?;
                    viewport = placed;
                    presenter
                        .present(&ShownFrame {
                            index: shown.index,
                            pts_us: shown.pts_us,
                            clock_us: session.clock().position_us(),
                            surface_size,
                            viewport,
                            surface: &surface,
                        })
                        .map_err(DevicePlaybackError::Present)?;
                    presented += 1;
                }
            }
        }
        if session.state().is_terminal() {
            break PlaybackEnd::Completed;
        }
        let elapsed = started.elapsed();
        if limit.is_some_and(|limit| elapsed >= limit) {
            break PlaybackEnd::TimeLimit;
        }
        if elapsed > audio_len + DEVICE_SLACK {
            return Err(DevicePlaybackError::DeviceStalled {
                played_frames: session.clock().played_samples(),
                total_frames,
            });
        }
    };
    let played_audio_frames = session.clock().played_samples();
    let state = session.state().clone();
    let _ = device.stop_voice(cs_sim::audio_events::VoiceStop {
        voice,
        reason: None,
    });
    Ok(DevicePlaybackReport {
        media: media.clone(),
        end,
        state,
        source_size: (info.width, info.height),
        frame_rate: info.frame_rate,
        sample_rate: rate,
        total_audio_frames: total_frames,
        played_audio_frames,
        frames_presented: presented,
        max_abs_drift_us: max_drift,
        drift_within_tolerance: max_drift <= PROBE_DRIFT_TOLERANCE_US,
        surface_size,
        viewport,
        wall: started.elapsed(),
    })
}

pub(super) fn frames_of(position: Duration, rate: u32) -> u64 {
    u64::try_from(position.as_micros() * u128::from(rate) / 1_000_000).unwrap_or(u64::MAX)
}
