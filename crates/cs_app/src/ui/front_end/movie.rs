//! The front-end shell's `M`-row movies, decoded and scheduled (FE-MENU-2).
//!
//! Spec: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
//! stage `### F45-B`/`### F45-D`; task FE-MENU-2.
//!
//! `LAYOUT.CSV`'s `MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0` row makes the
//! main menu's flag wave a looping MPEG-1 program stream under the panes —
//! 320x240 video scaled 250% to the authored 800x600 surface. The shell
//! records the row ([`super::shell::ShellMovie`]); this module owns what the
//! window needs to *play* it:
//!
//! * [`MovieSource`] is the video half of the cinematics playback boundary:
//!   the production [`MpegVideoSource`] fills it, tests fill it with a
//!   scripted source — no decoder is assumed here either.
//! * [`MovieBackdrop`] turns one `ShellMovie` plus an `open` closure into a
//!   repeating source: frames are delivered on their decoded presentation
//!   times, and the row's `# Loops` (`0` = continuously, the convention the
//!   file's `W` rows document) replays the stream from the start.
//!
//! # What is **not** claimed
//!
//! * **The movie's own audio** — `crimflag.mpg` carries a mono MP2 track the
//!   owner frame's capture cannot separate from the menu's music; which cue
//!   the original plays is FE-MENU-4's audio work, recorded here as not
//!   claimed rather than guessed.
//! * **What the original does at a non-looping movie's end** — the last
//!   delivered frame stays up (the honest hold); whether the original holds
//!   or clears is unread.
//! * **The `Z` column's draw order** — movies draw under the panes in
//!   declaration order, matching the shell's recorded `Z` position.

use std::io::Read;
use std::time::Duration;

use cs_content::catalog::baseline::install_file_key;
use cs_sim::cinematic_state::MediaFailure;
use cs_types::content::{ContentId, ContentKind};

use super::retail::MOVIE_DIRECTORY;
use super::shell::{ShellMovie, ShellRect};
use crate::cinematics::mpeg::MpegVideoSource;
use crate::cinematics::playback::{DecodedFrame, FrameSource};

/// The content id a front-end movie file names in a [`MediaFailure`].
///
/// `file` is the `M` row's file name as written (`CrimFlag.MPG`); the id is
/// the installation-relative path under `GOSDATA/ASSETS/GRAPHICS/MPG`,
/// escaped through the catalog's own [`install_file_key`] — the same key
/// the install-file inventory row carries, so the id is that row's
/// identity.
#[must_use]
pub fn movie_media(file: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::InstallFile,
        &install_file_key(&format!("{MOVIE_DIRECTORY}/{file}")),
    )
    .expect("an install_file_key output is inside the id grammar")
}

/// One decoded-movie source: the video half of the cinematics
/// [`FrameSource`] boundary, plus what a backdrop needs that a pure frame
/// stream does not carry — the last decoded frame's pixels and the stream's
/// measured extent and rate.
pub trait MovieSource: FrameSource {
    /// The pixels of the frame the last `next_frame` returned, `width *
    /// height` RGBA8 in row order; `None` before the first frame and once
    /// the stream has ended.
    fn pixels(&self) -> Option<&[u8]>;
    /// The video's decoded extent.
    fn extent(&self) -> (u32, u32);
    /// The stream's measured frame rate.
    fn frame_rate(&self) -> f64;
}

impl<R: Read> MovieSource for MpegVideoSource<R> {
    fn pixels(&self) -> Option<&[u8]> {
        self.current_rgba()
    }

    fn extent(&self) -> (u32, u32) {
        let info = self.info();
        (info.width, info.height)
    }

    fn frame_rate(&self) -> f64 {
        self.info().frame_rate
    }
}

/// One section movie playing in a loop: the `M` row, a live decoder and the
/// frame currently on the surface.
///
/// The player decodes one frame ahead (`pending`) and delivers it when its
/// presentation time — rebased to the first frame's, so an MPEG stream's
/// nonzero container timestamps cannot stall playback — comes due on the
/// elapsed clock the caller supplies. At the stream's end the `open` closure
/// reopens it, the clock restarts and `played` counts the playthroughs, until
/// `# Loops` is spent (`0` never is). A decode error or a failed reopen is
/// recorded in [`Self::failure`] and the last delivered frame stays up; the
/// backdrop never fabricates a replacement.
///
/// `O` opens a *fresh* source at the start of the stream — the loop
/// restart. Holding the closure rather than the path keeps the player over
/// any `Read` the caller supplies (a file, a container read, test bytes).
pub struct MovieBackdrop<S, O> {
    movie: ShellMovie,
    open: O,
    source: S,
    /// Elapsed-time origin of the current playthrough.
    cycle_start: Duration,
    /// The current playthrough's first frame pts: decoded pts rebased to 0.
    pts_base: u64,
    /// The decoded-ahead frame waiting for its presentation time.
    pending: Option<DecodedFrame>,
    /// The frame on the surface (RGBA8, `extent` sized).
    current: Vec<u8>,
    /// Frames delivered to the surface, all playthroughs.
    delivered: u64,
    /// Playthroughs that ran to the stream's end.
    played: u32,
    /// The stream ended and `# Loops` was spent.
    ended: bool,
    /// The recorded failure, once decode or a reopen failed.
    failure: Option<String>,
    /// The video's decoded extent.
    extent: (u32, u32),
    /// The stream's measured frame rate.
    frame_rate: f64,
}

impl<S: MovieSource, O> MovieBackdrop<S, O> {
    /// The row this backdrop plays.
    #[must_use]
    pub const fn movie(&self) -> &ShellMovie {
        &self.movie
    }

    /// The video's decoded extent.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.extent
    }

    /// The stream's measured frame rate.
    #[must_use]
    pub const fn frame_rate(&self) -> f64 {
        self.frame_rate
    }

    /// The drawn rect: the row's `X`,`Y` and the video extent at the row's
    /// `ScaleX`/`ScaleY` percent, in logical pixels.
    #[must_use]
    pub fn rect(&self) -> ShellRect {
        self.movie.rect_at(self.extent)
    }

    /// The frame on the surface, `width * height` RGBA8; empty only between
    /// [`Self::open`] and the first [`Self::update`] (or after a failure
    /// before any frame decoded).
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.current
    }

    /// Frames delivered to the surface over every playthrough.
    #[must_use]
    pub const fn delivered(&self) -> u64 {
        self.delivered
    }

    /// Playthroughs that ran to the stream's end.
    #[must_use]
    pub const fn played(&self) -> u32 {
        self.played
    }

    /// `# Loops` spent: the stream ended and the row asked for no replay.
    /// The last delivered frame stays up.
    #[must_use]
    pub const fn ended(&self) -> bool {
        self.ended
    }

    /// The recorded failure, once decoding or a reopen failed.
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Copies the pending frame onto the surface.
    fn deliver(&mut self) {
        if let Some(pixels) = self.source.pixels() {
            self.current.clear();
            self.current.extend_from_slice(pixels);
            self.delivered += 1;
        }
    }
}

impl<S: MovieSource, O: FnMut() -> Result<S, MediaFailure>> MovieBackdrop<S, O> {
    /// Opens the movie's first source and reads the first frame — the frame
    /// a `ShellMovie` needs to draw at all and the one `open` fails on when
    /// the stream holds no video.
    ///
    /// # Errors
    ///
    /// The `open` closure's [`MediaFailure`], or
    /// [`MediaFailure::DecodeFailed`] when the first frame will not decode.
    pub fn open(movie: &ShellMovie, mut open: O, now: Duration) -> Result<Self, MediaFailure> {
        let mut source = open()?;
        let extent = source.extent();
        let frame_rate = source.frame_rate();
        let first = source
            .next_frame()
            .map_err(|error| MediaFailure::DecodeFailed {
                detail: error.0.clone(),
            })?
            .ok_or_else(|| MediaFailure::DecodeFailed {
                detail: format!("{} has no video frames", movie.file),
            })?;
        Ok(Self {
            movie: movie.clone(),
            open,
            source,
            cycle_start: now,
            pts_base: first.pts_us,
            pending: Some(first),
            current: Vec::new(),
            delivered: 0,
            played: 0,
            ended: false,
            failure: None,
            extent,
            frame_rate,
        })
    }

    /// Advances playback to `now`: every pending frame whose rebased
    /// presentation time has come due is decoded past and the newest lands
    /// on the surface. Returns `true` when the surface pixels changed.
    pub fn update(&mut self, now: Duration) -> bool {
        if self.ended || self.failure.is_some() {
            return false;
        }
        let elapsed_us = micros(now.saturating_sub(self.cycle_start));
        let mut changed = false;
        loop {
            if self.pending.is_none() && !self.fill(now) {
                break;
            }
            let due = self
                .pending
                .as_ref()
                .is_some_and(|frame| frame.pts_us.saturating_sub(self.pts_base) <= elapsed_us);
            if !due {
                break;
            }
            self.pending = None;
            self.deliver();
            changed = true;
        }
        changed
    }

    /// Refills `pending` from the source, looping to a reopened source at
    /// the stream's end while `# Loops` allows. `false` when there is
    /// nothing more to decode (loops spent or a recorded failure).
    fn fill(&mut self, now: Duration) -> bool {
        match self.source.next_frame() {
            Ok(Some(frame)) => {
                self.pending = Some(frame);
                true
            }
            Err(error) => {
                self.failure = Some(format!("decode failed: {}", error.0));
                false
            }
            Ok(None) => {
                self.played = self.played.saturating_add(1);
                if self.movie.loops != 0 && self.played >= self.movie.loops {
                    self.ended = true;
                    return false;
                }
                match (self.open)() {
                    Err(failure) => {
                        self.failure = Some(failure.to_string());
                        false
                    }
                    Ok(mut source) => match source.next_frame() {
                        Ok(Some(first)) => {
                            self.source = source;
                            self.cycle_start = now;
                            self.pts_base = first.pts_us;
                            self.pending = Some(first);
                            true
                        }
                        Ok(None) => {
                            self.failure =
                                Some(format!("{} reopened with no video frames", self.movie.file));
                            false
                        }
                        Err(error) => {
                            self.failure = Some(format!("decode failed: {}", error.0));
                            false
                        }
                    },
                }
            }
        }
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}
