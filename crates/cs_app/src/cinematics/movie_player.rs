//! The windowed movie presenter (PARITY5-CABIN-INTRO.02, task #1838).
//!
//! Findings: `docs/findings/2026-10-21-parity5-cabin-intro-measurement.md`
//! (the post-pilot intro is `chap1.mpg`; the original skips on Escape, Space,
//! Return or a left click and ends the same way as a natural end).
//!
//! Two halves:
//!
//! * [`MoviePlayer`] is the engine-free half. It opens an MPEG-1 program
//!   stream, decodes the MP2 audio, plays it through the real output device
//!   ([`AudibleDevice`], behind the `audio` capability gate) and slaves the
//!   decoded video to **how far the device has pulled that audio** (the
//!   [`MediaClock`], non-negotiable behavior 3). [`MoviePlayer::poll`] hands
//!   back the pixels of a newly due frame, or the end.
//! * [`MoviePlayerPlugin`] is the Bevy half: a [`MoviePresenter`] resource
//!   holds the running player, a 2D camera and a sprite show the current frame
//!   fitted into the window at its own aspect ratio ([`fit_letterboxed`], never
//!   stretched, bars are the clear colour), [`skip_requested`] reads the skip
//!   input, and the end is reported as a typed [`MovieFinished`] message.
//!
//! A movie ends in exactly one of three ways, never silently:
//! [`MovieOutcome::Completed`] (played to the end), [`MovieOutcome::Skipped`]
//! (the player asked) or [`MovieOutcome::Failed`] with the [`MediaFailure`]
//! (missing file, not MPEG-1, a stream that breaks part-way, an output that
//! stopped pulling). A failure is never a completion.
//!
//! The retail pause policy, tick rate and skippability of the cabin intro are
//! engine state the finding records as unread; the plan the player runs is the
//! same designed playback plan as [`super::device`]'s, made skippable.
//! [`AudioOutput::Silent`] exists for machines and tests without an output
//! device: it has **no sound** and is never evidence that audio plays.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use bevy::camera::RenderTarget;
use bevy::image::{Image, ImageSampler};
use bevy::input::ButtonInput;
use bevy::prelude::{
    App, Assets, Camera, Camera2d, Commands, Component, Entity, Handle, KeyCode, Message,
    MessageWriter, MouseButton, Plugin, Query, Res, ResMut, Resource, Sprite, Transform, Update,
    Vec2, Window, With, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use cs_sim::audio_events::{AudioBus, AudioDevice, DeviceVoiceId, VoiceStart, VoiceStop};
use cs_sim::cinematic_state::{
    CinematicScript, CinematicState, CompletionKind, FailureRecovery, MasterClock, MediaFailure,
    PausePolicy, SYNTHETIC_PLAYER,
};
use cs_types::content::ContentId;

use super::device::{
    DEVICE_SLACK, END_STALL, END_TAIL_DIVISOR, PROBE_DRIFT_TOLERANCE_US, PROBE_TICKS_PER_SECOND,
    SharedFrames, TeeSource, frames_of,
};
use super::mpeg::{MPEG_PS_FORMAT, MpegAudioSource, MpegVideoInfo, MpegVideoSource};
use super::playback::{MediaClock, VideoPlayback};
use super::session::{CinematicSession, MissionHandback};
use super::{CinematicPlan, MediaAvailability, PresentationPlan, fit_letterboxed};
use crate::audio::device::{
    AudibleDevice, CapabilityDeclaration, InMemorySamples, PcmAudio, open_audible_device,
};

/// Where the movie's sound goes.
#[derive(Clone, Debug)]
pub enum AudioOutput {
    /// The real output device, opened only if the declaration lists `audio`.
    Device(CapabilityDeclaration),
    /// No sound at all; the clock follows wall time. For machines and tests
    /// without an output device. Not evidence that audio plays.
    Silent,
}

/// One newly due frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentedFrame {
    /// The frame's index in decode order.
    pub index: u64,
    /// Its presentation time in microseconds.
    pub pts_us: u64,
    /// The audio master clock when it was shown, in microseconds.
    pub clock_us: u64,
    /// Its pixels: `width * height` RGBA texels in row order.
    pub rgba: Vec<u8>,
}

/// What one [`MoviePlayer::poll`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoviePoll {
    /// Still playing; a frame is present when a new one came due.
    Playing(Option<PresentedFrame>),
    /// The movie is over, played to the end or skipped.
    Finished(MovieEnd),
}

/// How a movie that did not fail ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovieEnd {
    /// It played to the last audio frame.
    Completed,
    /// The player skipped it.
    Skipped,
}

/// How a movie ended, as [`MovieFinished`] reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MovieOutcome {
    /// It played to the end.
    Completed,
    /// The player skipped it.
    Skipped,
    /// It could not be played (or stopped part-way): never a completion.
    Failed(MediaFailure),
}

impl From<MovieEnd> for MovieOutcome {
    fn from(end: MovieEnd) -> Self {
        match end {
            MovieEnd::Completed => Self::Completed,
            MovieEnd::Skipped => Self::Skipped,
        }
    }
}

fn failure(detail: impl std::fmt::Display) -> MediaFailure {
    MediaFailure::DecodeFailed {
        detail: detail.to_string(),
    }
}

/// One movie playing against its audio.
pub struct MoviePlayer {
    media: ContentId,
    info: MpegVideoInfo,
    rate: u32,
    total_frames: u64,
    session: CinematicSession<TeeSource<BufReader<File>>>,
    sink: MissionHandback,
    frames: SharedFrames,
    device: Option<(AudibleDevice, DeviceVoiceId)>,
    last_index: Option<u64>,
    last_played: u64,
    last_advance: Instant,
    started: Instant,
    audio_len: Duration,
    finished: Option<MovieEnd>,
}

impl std::fmt::Debug for MoviePlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoviePlayer")
            .field("media", &self.media)
            .field("state", self.session.state())
            .finish_non_exhaustive()
    }
}

impl MoviePlayer {
    /// Opens the program stream at `path` and starts it. `media` names it in a
    /// failure.
    ///
    /// # Errors
    ///
    /// The [`MediaFailure`]: a missing file is `MissingMedia`, a file that is
    /// not an MPEG-1 program stream `MissingDecoder`, an undecodable stream,
    /// an unusable audio track or an output device that cannot be opened
    /// `DecodeFailed` with the reason.
    pub fn open(
        path: &Path,
        media: &ContentId,
        output: &AudioOutput,
    ) -> Result<Self, MediaFailure> {
        let video = MpegVideoSource::open(path, media)?;
        let info = video.info();
        let mut audio_source = MpegAudioSource::open(path, media)?;
        let rate = audio_source.sample_rate();
        let mut samples = Vec::new();
        while let Some(chunk) = audio_source.next_samples().map_err(failure)? {
            samples.extend_from_slice(chunk);
        }
        let pcm = PcmAudio::try_new(2, rate, samples).map_err(|e| failure(format!("{e:?}")))?;
        let total_frames = pcm.frames();

        let device = match output {
            AudioOutput::Silent => None,
            AudioOutput::Device(declaration) => {
                let mut library = InMemorySamples::new();
                library.insert(media.clone(), pcm);
                let mut device = open_audible_device(Arc::new(library), declaration)
                    .map_err(|e| failure(format!("audio device: {} ({})", e.detail, e.code)))?;
                let voice = device
                    .start_one_shot(VoiceStart {
                        asset: media.clone(),
                        bus: AudioBus::Music,
                        gain: 1.0,
                        pan: 0.0,
                        pitch: 1.0,
                    })
                    .map_err(|e| failure(format!("audio device: {} ({})", e.detail, e.code)))?;
                Some((device, voice))
            }
        };

        let frames: SharedFrames = Arc::<Mutex<_>>::default();
        let tee = TeeSource {
            inner: video,
            frames: Arc::clone(&frames),
        };
        let duration_ticks = total_frames * u64::from(PROBE_TICKS_PER_SECOND) / u64::from(rate);
        let script = CinematicScript::try_new(
            media.clone(),
            duration_ticks.max(1),
            true,
            PausePolicy::SimulationRuns,
            MasterClock::Audio,
            FailureRecovery::Block,
            Vec::new(),
        )
        .map_err(|e| failure(format!("{e:?}")))?;
        let plan = CinematicPlan {
            script,
            presentation: PresentationPlan::Video {
                media: media.clone(),
                format: MPEG_PS_FORMAT.to_owned(),
                frame_size: (info.width, info.height),
            },
        };
        let clock = MediaClock::new(rate).map_err(failure)?;
        let mut sink = MissionHandback::new(SYNTHETIC_PLAYER);
        let session = CinematicSession::begin(
            plan,
            &MediaAvailability::Present,
            Some(VideoPlayback::new(tee, PROBE_DRIFT_TOLERANCE_US)),
            clock,
            PROBE_TICKS_PER_SECOND,
            SYNTHETIC_PLAYER,
            &mut sink,
        )
        .map_err(failure)?;
        Ok(Self {
            media: media.clone(),
            info,
            rate,
            total_frames,
            session,
            sink,
            frames,
            device,
            last_index: None,
            last_played: 0,
            last_advance: Instant::now(),
            started: Instant::now(),
            audio_len: Duration::from_micros(total_frames * 1_000_000 / u64::from(rate)),
            finished: None,
        })
    }

    /// The movie being played.
    #[must_use]
    pub const fn media(&self) -> &ContentId {
        &self.media
    }

    /// The video stream's measured properties.
    #[must_use]
    pub const fn info(&self) -> MpegVideoInfo {
        self.info
    }

    /// Audio frames (per channel) the file decodes to.
    #[must_use]
    pub const fn total_audio_frames(&self) -> u64 {
        self.total_frames
    }

    /// The audio master clock's position in microseconds.
    #[must_use]
    pub fn clock_us(&self) -> u64 {
        self.session.clock().position_us()
    }

    /// Audio frames the master clock counts as played.
    #[must_use]
    pub fn played_audio_frames(&self) -> u64 {
        self.session.clock().played_samples()
    }

    /// The session's state.
    #[must_use]
    pub const fn state(&self) -> &CinematicState {
        self.session.state()
    }

    /// The audio frames the output has played, with the end-of-audio rule: the
    /// output reports the position it last pulled a whole buffer to, so a
    /// position that stops within the tail of the audio is the end of it.
    fn device_played(&mut self) -> u64 {
        let Some((device, voice)) = &self.device else {
            let wall = frames_of(self.started.elapsed(), self.rate);
            return wall.min(self.total_frames);
        };
        let position = device.played_position(*voice).unwrap_or_default();
        let mut played = frames_of(position, self.rate).min(self.total_frames);
        if played != self.last_played {
            self.last_played = played;
            self.last_advance = Instant::now();
        } else if played > 0
            && played + u64::from(self.rate / END_TAIL_DIVISOR) >= self.total_frames
            && self.last_advance.elapsed() >= END_STALL
        {
            played = self.total_frames;
        }
        played
    }

    /// Advances to what the output device (or, silent, the wall clock) has
    /// played and reports the newly due frame or the end.
    ///
    /// # Errors
    ///
    /// The [`MediaFailure`] of a stream that broke part-way or an output that
    /// stopped pulling; the movie is over and was not completed.
    pub fn poll(&mut self) -> Result<MoviePoll, MediaFailure> {
        let played = self.device_played();
        if self.started.elapsed() > self.audio_len + DEVICE_SLACK
            && played < self.total_frames
            && !self.session.state().is_terminal()
        {
            self.stop_audio();
            return Err(failure(format!(
                "the output stopped after {played} of {} audio frames",
                self.total_frames
            )));
        }
        self.poll_at(played)
    }

    /// As [`Self::poll`] with the audio frames played given by the caller.
    ///
    /// # Errors
    ///
    /// As [`Self::poll`].
    pub fn poll_at(&mut self, played_frames: u64) -> Result<MoviePoll, MediaFailure> {
        if let Some(end) = self.finished {
            return Ok(MoviePoll::Finished(end));
        }
        let delta = played_frames
            .min(self.total_frames)
            .saturating_sub(self.session.clock().played_samples());
        if delta > 0
            && let Err(error) = self.session.step(delta, &mut self.sink)
        {
            self.stop_audio();
            return Err(failure(error));
        }
        match self.session.state() {
            CinematicState::Failed { reason, .. } => {
                let reason = reason.clone();
                self.stop_audio();
                return Err(reason);
            }
            CinematicState::Completed(kind) => {
                let end = match kind {
                    CompletionKind::Played => MovieEnd::Completed,
                    CompletionKind::Skipped => MovieEnd::Skipped,
                };
                self.finish(end);
                return Ok(MoviePoll::Finished(end));
            }
            CinematicState::Canceled => {
                self.stop_audio();
                return Err(failure("the cinematic was canceled"));
            }
            _ => {}
        }
        let clock_us = self.session.clock().position_us();
        let Some(shown) = self.session.video().and_then(VideoPlayback::shown) else {
            return Ok(MoviePoll::Playing(None));
        };
        if self.last_index == Some(shown.index) {
            return Ok(MoviePoll::Playing(None));
        }
        let (index, pts_us) = (shown.index, shown.pts_us);
        self.last_index = Some(index);
        let rgba = self
            .frames
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, rgba)| rgba.clone());
        Ok(MoviePoll::Playing(rgba.map(|rgba| PresentedFrame {
            index,
            pts_us,
            clock_us,
            rgba,
        })))
    }

    /// Skips the movie: the player asked, so it ends now, silently stopped.
    ///
    /// # Errors
    ///
    /// The [`MediaFailure`] if the session refuses the skip (the movie had
    /// already ended).
    pub fn skip(&mut self) -> Result<MovieEnd, MediaFailure> {
        if let Some(end) = self.finished {
            return Ok(end);
        }
        self.session.skip(&mut self.sink).map_err(failure)?;
        self.finish(MovieEnd::Skipped);
        Ok(MovieEnd::Skipped)
    }

    fn finish(&mut self, end: MovieEnd) {
        self.finished = Some(end);
        self.stop_audio();
    }

    fn stop_audio(&mut self) {
        if let Some((mut device, voice)) = self.device.take() {
            let _ = device.stop_voice(VoiceStop {
                voice,
                reason: None,
            });
        }
    }
}

impl Drop for MoviePlayer {
    fn drop(&mut self) {
        self.stop_audio();
    }
}

/// Whether the player asked to skip this frame: Escape, Space or Return
/// pressed, or the left mouse button pressed. (The original's `gui_char`
/// handles `key_escape`, `key_space`, `key_return` and a left press.)
#[must_use]
pub fn skip_requested(
    keys: Option<&ButtonInput<KeyCode>>,
    mouse: Option<&ButtonInput<MouseButton>>,
) -> bool {
    keys.is_some_and(|keys| {
        keys.any_just_pressed([KeyCode::Escape, KeyCode::Space, KeyCode::Enter])
    }) || mouse.is_some_and(|mouse| mouse.just_pressed(MouseButton::Left))
}

/// The movie is over. Sent exactly once per started movie.
#[derive(Clone, Debug, Message, PartialEq, Eq)]
pub struct MovieFinished {
    /// The movie.
    pub media: ContentId,
    /// How it ended.
    pub outcome: MovieOutcome,
}

#[derive(Component)]
struct MovieScreen;

/// The running movie and what shows it.
#[derive(Default, Resource)]
pub struct MoviePresenter {
    player: Option<MoviePlayer>,
    image: Option<Handle<Image>>,
    frame_size: (u32, u32),
    target: Option<Handle<Image>>,
    last_clock_us: u64,
    frames_shown: u64,
}

impl MoviePresenter {
    /// Draws into `target` instead of the primary window (an offscreen
    /// capture); set before [`Self::start`].
    pub fn render_to(&mut self, target: Handle<Image>) {
        self.target = Some(target);
    }

    /// Starts `player`, replacing any movie already running without a report.
    pub fn start(&mut self, player: MoviePlayer) {
        let info = player.info();
        self.frame_size = (info.width, info.height);
        self.player = Some(player);
        self.image = None;
        self.last_clock_us = 0;
        self.frames_shown = 0;
    }

    /// Whether a movie is running.
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.player.is_some()
    }

    /// The audio master clock of the running movie, in microseconds; the last
    /// value once it has ended.
    #[must_use]
    pub fn clock_us(&self) -> u64 {
        self.player
            .as_ref()
            .map_or(self.last_clock_us, MoviePlayer::clock_us)
    }

    /// Distinct frames put on screen since the last [`Self::start`].
    #[must_use]
    pub const fn frames_shown(&self) -> u64 {
        self.frames_shown
    }
}

/// Adds the [`MoviePresenter`], its systems and the [`MovieFinished`] message.
/// The app needs `Assets<Image>` (`DefaultPlugins` has it).
#[derive(Clone, Copy, Debug, Default)]
pub struct MoviePlayerPlugin;

impl Plugin for MoviePlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoviePresenter>()
            .add_message::<MovieFinished>()
            .add_systems(Update, drive_movie);
    }
}

fn blank_frame(size: (u32, u32)) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn drive_movie(
    mut commands: Commands,
    mut presenter: ResMut<MoviePresenter>,
    mut images: ResMut<Assets<Image>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut sprites: Query<(&mut Sprite, &mut Transform), With<MovieScreen>>,
    screen: Query<Entity, With<MovieScreen>>,
    mut finished: MessageWriter<MovieFinished>,
) {
    let presenter = &mut *presenter;
    let Some(player) = presenter.player.as_mut() else {
        for entity in &screen {
            commands.entity(entity).despawn();
        }
        presenter.image = None;
        return;
    };
    if presenter.image.is_none() {
        let handle = images.add(blank_frame(presenter.frame_size));
        let mut camera = commands.spawn((
            MovieScreen,
            Camera2d,
            Camera {
                order: 100,
                ..default()
            },
        ));
        if let Some(target) = &presenter.target {
            camera.insert(RenderTarget::Image(target.clone().into()));
        }
        commands.spawn((
            MovieScreen,
            Sprite {
                image: handle.clone(),
                custom_size: Some(Vec2::new(
                    presenter.frame_size.0 as f32,
                    presenter.frame_size.1 as f32,
                )),
                ..default()
            },
            Transform::default(),
        ));
        presenter.image = Some(handle);
    }

    let outcome = if skip_requested(keys.as_deref(), mouse.as_deref()) {
        match player.skip() {
            Ok(end) => Some(MovieOutcome::from(end)),
            Err(failure) => Some(MovieOutcome::Failed(failure)),
        }
    } else {
        match player.poll() {
            Ok(MoviePoll::Playing(Some(frame))) => {
                if let Some(mut image) = presenter.image.as_ref().and_then(|h| images.get_mut(h)) {
                    image.data = Some(frame.rgba);
                    presenter.frames_shown += 1;
                }
                None
            }
            Ok(MoviePoll::Playing(None)) => None,
            Ok(MoviePoll::Finished(end)) => Some(MovieOutcome::from(end)),
            Err(failure) => Some(MovieOutcome::Failed(failure)),
        }
    };

    let surface = presenter
        .target
        .as_ref()
        .and_then(|t| images.get(t))
        .map(|t| (t.width(), t.height()))
        .or_else(|| {
            windows
                .single()
                .ok()
                .map(|w| (w.width() as u32, w.height() as u32))
        });
    if let Some(surface) = surface
        && let Ok(viewport) = fit_letterboxed(presenter.frame_size, surface)
    {
        for (mut sprite, mut transform) in &mut sprites {
            sprite.custom_size = Some(Vec2::new(viewport.width as f32, viewport.height as f32));
            *transform = Transform::default();
        }
    }

    if let Some(outcome) = outcome {
        presenter.last_clock_us = player.clock_us();
        finished.write(MovieFinished {
            media: player.media().clone(),
            outcome,
        });
        presenter.player = None;
        presenter.image = None;
        for entity in &screen {
            commands.entity(entity).despawn();
        }
    }
}
