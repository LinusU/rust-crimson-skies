//! Decoded-frame playback against the master media clock (F40-B).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-B`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! No codec is assumed: the original video format is unrecovered (see
//! `docs/findings/2026-10-05-f40-b-playback-and-camera-timelines.md`), so
//! decoding sits behind [`FrameSource`]. This module owns what is independent
//! of the codec: which decoded frame is due on the master clock
//! ([`VideoPlayback::frame_due`]), pause/resume that stops the clock rather
//! than the decoder ([`MediaClock`]) and the measured audio/video drift against
//! an explicit tolerance ([`DriftReport`]). Audio is the master clock
//! (non-negotiable behavior 3); video follows it, never the reverse.

use std::fmt;

/// Audio samples per second the media clock counts in (the clock unit).
pub type SampleRate = u32;

/// The master media clock, counted in audio samples actually played.
///
/// The audio device reports samples; while paused it is told to stop, so
/// [`MediaClock::position`] holds. Video asks the clock, so a pause can never
/// leave video running ahead of audio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaClock {
    rate: SampleRate,
    played_samples: u64,
    paused: bool,
}

/// A zero sample rate was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZeroRate;

impl fmt::Display for ZeroRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the media clock sample rate must not be zero")
    }
}

impl std::error::Error for ZeroRate {}

impl MediaClock {
    /// A running clock at position zero.
    ///
    /// # Errors
    ///
    /// [`ZeroRate`] for a zero rate.
    pub const fn new(rate: SampleRate) -> Result<Self, ZeroRate> {
        if rate == 0 {
            return Err(ZeroRate);
        }
        Ok(Self {
            rate,
            played_samples: 0,
            paused: false,
        })
    }

    /// The sample rate.
    #[must_use]
    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    /// Samples played so far.
    #[must_use]
    pub const fn played_samples(&self) -> u64 {
        self.played_samples
    }

    /// Whether the clock is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// The position in microseconds.
    #[must_use]
    pub fn position_us(&self) -> u64 {
        let micros = u128::from(self.played_samples) * 1_000_000 / u128::from(self.rate);
        u64::try_from(micros).unwrap_or(u64::MAX)
    }

    /// The audio device played `samples`. Ignored while paused, as a paused
    /// device plays none; this also absorbs a late device callback.
    pub const fn audio_played(&mut self, samples: u64) {
        if !self.paused {
            self.played_samples = self.played_samples.saturating_add(samples);
        }
    }

    /// Stops the clock.
    pub const fn pause(&mut self) {
        self.paused = true;
    }

    /// Restarts the clock from where it stopped.
    pub const fn resume(&mut self) {
        self.paused = false;
    }
}

/// One decoded frame, as a [`FrameSource`] delivers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedFrame {
    /// Presentation time in microseconds from the start of the video.
    pub pts_us: u64,
    /// The frame's index in decode order.
    pub index: u64,
}

/// Why a source could not deliver a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "decode failed: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

/// A decoder. Frames come in non-decreasing presentation order; `Ok(None)` is
/// the end of the stream.
pub trait FrameSource {
    /// The next frame.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] when decoding fails part-way.
    fn next_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError>;
}

/// What the presenter should do this refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameDue {
    /// Show this frame (the newest one whose time has come; older ones that
    /// were also due were dropped to catch up).
    Show {
        /// The frame.
        frame: DecodedFrame,
        /// Earlier due frames skipped to catch up.
        dropped: u64,
    },
    /// Keep the frame already on screen.
    Hold,
    /// The stream ended.
    Ended,
}

/// The measured drift of shown video against the audio master clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftReport {
    /// Signed drift in microseconds: shown frame pts minus clock position.
    /// Negative means video is behind audio.
    pub drift_us: i64,
    /// The approved tolerance in microseconds.
    pub tolerance_us: u64,
}

impl DriftReport {
    /// Whether the drift is within tolerance.
    #[must_use]
    pub const fn within_tolerance(&self) -> bool {
        self.drift_us.unsigned_abs() <= self.tolerance_us
    }
}

/// Video playback slaved to a [`MediaClock`].
pub struct VideoPlayback<S: FrameSource> {
    source: S,
    pending: Option<DecodedFrame>,
    shown: Option<DecodedFrame>,
    ended: bool,
    tolerance_us: u64,
}

impl<S: FrameSource> VideoPlayback<S> {
    /// Playback of `source` with an explicit drift tolerance. The tolerance is
    /// a caller-supplied design value: the original's is unmeasured.
    pub const fn new(source: S, tolerance_us: u64) -> Self {
        Self {
            source,
            pending: None,
            shown: None,
            ended: false,
            tolerance_us,
        }
    }

    /// The frame on screen, if any.
    #[must_use]
    pub const fn shown(&self) -> Option<&DecodedFrame> {
        self.shown.as_ref()
    }

    /// Chooses what to present at the clock's current position. While the
    /// clock is paused its position holds, so this holds the shown frame.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] from the source; the caller reports it as a media
    /// failure, never a completion.
    pub fn frame_due(&mut self, clock: &MediaClock) -> Result<FrameDue, DecodeError> {
        let now = clock.position_us();
        let mut newest: Option<DecodedFrame> = None;
        let mut due = 0_u64;
        loop {
            if self.pending.is_none() && !self.ended {
                match self.source.next_frame()? {
                    Some(frame) => self.pending = Some(frame),
                    None => self.ended = true,
                }
            }
            match &self.pending {
                Some(frame) if frame.pts_us <= now => {
                    newest = self.pending.take();
                    due += 1;
                }
                _ => break,
            }
        }
        if let Some(frame) = newest {
            self.shown = Some(frame.clone());
            return Ok(FrameDue::Show {
                frame,
                dropped: due - 1,
            });
        }
        if self.ended && self.pending.is_none() {
            return Ok(FrameDue::Ended);
        }
        Ok(FrameDue::Hold)
    }

    /// Drift of the shown frame against the clock; `None` before the first
    /// frame is shown.
    #[must_use]
    pub fn drift(&self, clock: &MediaClock) -> Option<DriftReport> {
        let shown = self.shown.as_ref()?;
        let drift_us = i64::try_from(shown.pts_us).unwrap_or(i64::MAX)
            - i64::try_from(clock.position_us()).unwrap_or(i64::MAX);
        Some(DriftReport {
            drift_us,
            tolerance_us: self.tolerance_us,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cinematics::{MediaAvailability, begin, lower_cinematic};
    use cs_content::cinematics::declared_synthetic_cinematic;
    use cs_sim::cinematic_state::{CinematicState, MediaFailure, SYNTHETIC_PLAYER};

    const RATE: SampleRate = 48_000;
    /// 25 fps, in microseconds.
    const FRAME_US: u64 = 40_000;

    struct Frames {
        next: u64,
        count: u64,
        fail_at: Option<u64>,
    }

    impl FrameSource for Frames {
        fn next_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
            if self.fail_at == Some(self.next) {
                return Err(DecodeError("corrupt packet".into()));
            }
            if self.next >= self.count {
                return Ok(None);
            }
            let frame = DecodedFrame {
                pts_us: self.next * FRAME_US,
                index: self.next,
            };
            self.next += 1;
            Ok(Some(frame))
        }
    }

    fn playback(count: u64, fail_at: Option<u64>) -> VideoPlayback<Frames> {
        VideoPlayback::new(
            Frames {
                next: 0,
                count,
                fail_at,
            },
            FRAME_US,
        )
    }

    fn samples_for_us(us: u64) -> u64 {
        us * u64::from(RATE) / 1_000_000
    }

    #[test]
    fn accept_f40_b_pause_and_resume_keep_video_within_the_drift_tolerance() {
        let mut clock = MediaClock::new(RATE).unwrap();
        let mut video = playback(100, None);
        for _ in 0..25 {
            clock.audio_played(samples_for_us(FRAME_US));
            video.frame_due(&clock).unwrap();
        }
        let before = video.shown().unwrap().clone();
        assert!(video.drift(&clock).unwrap().within_tolerance());

        // Pause: the device plays nothing; a late callback must not move the
        // clock, and the presenter must hold rather than run ahead.
        clock.pause();
        for _ in 0..50 {
            clock.audio_played(samples_for_us(FRAME_US));
            assert_eq!(video.frame_due(&clock).unwrap(), FrameDue::Hold);
        }
        assert_eq!(video.shown().unwrap(), &before);

        clock.resume();
        for _ in 0..25 {
            clock.audio_played(samples_for_us(FRAME_US));
            let due = video.frame_due(&clock).unwrap();
            assert!(matches!(due, FrameDue::Show { dropped: 0, .. }), "{due:?}");
            let report = video.drift(&clock).unwrap();
            assert!(report.within_tolerance(), "{report:?}");
        }
        assert_eq!(video.shown().unwrap().index, before.index + 25);
    }

    #[test]
    fn accept_f40_b_a_stalled_decoder_is_reported_as_drift_beyond_tolerance() {
        let mut clock = MediaClock::new(RATE).unwrap();
        let mut video = playback(100, None);
        clock.audio_played(samples_for_us(FRAME_US));
        video.frame_due(&clock).unwrap();
        // Audio plays on, the video is not asked: drift must show it.
        clock.audio_played(samples_for_us(10 * FRAME_US));
        let report = video.drift(&clock).unwrap();
        assert!(report.drift_us < 0);
        assert!(!report.within_tolerance());
        // Catching up drops the late frames and lands back in tolerance.
        match video.frame_due(&clock).unwrap() {
            FrameDue::Show { dropped, .. } => assert!(dropped >= 9),
            other => panic!("expected Show, got {other:?}"),
        }
        assert!(video.drift(&clock).unwrap().within_tolerance());
    }

    #[test]
    fn accept_f40_b_stream_end_and_decode_failure_are_distinct_outcomes() {
        let mut clock = MediaClock::new(RATE).unwrap();
        let mut video = playback(2, None);
        clock.audio_played(samples_for_us(10 * FRAME_US));
        assert!(matches!(
            video.frame_due(&clock).unwrap(),
            FrameDue::Show { dropped: 1, .. }
        ));
        assert_eq!(video.frame_due(&clock).unwrap(), FrameDue::Ended);

        // A decode failure feeds the F40-A player as a media failure: never a
        // completion.
        let plan = lower_cinematic(&declared_synthetic_cinematic()).unwrap();
        let mut player = begin(&plan, &MediaAvailability::Present, SYNTHETIC_PLAYER).unwrap();
        let mut broken = playback(10, Some(3));
        let mut clock = MediaClock::new(RATE).unwrap();
        clock.audio_played(samples_for_us(10 * FRAME_US));
        let err = broken.frame_due(&clock).unwrap_err();
        player
            .media_failed(MediaFailure::DecodeFailed {
                detail: err.to_string(),
            })
            .unwrap();
        assert!(matches!(
            player.state(),
            CinematicState::Failed {
                reason: MediaFailure::DecodeFailed { .. },
                ..
            }
        ));
        assert!(player.semantic_end_reached());
    }

    #[test]
    fn accept_f40_b_clock_rejects_a_zero_rate_and_reports_microseconds() {
        assert_eq!(MediaClock::new(0), Err(ZeroRate));
        let mut clock = MediaClock::new(RATE).unwrap();
        clock.audio_played(u64::from(RATE));
        assert_eq!(clock.position_us(), 1_000_000);
    }
}
