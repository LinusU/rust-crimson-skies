//! Task #1838 `PARITY5-CABIN-INTRO.02`: the windowed movie presenter. Task test
//! prefix: `accept_parity5_cabin_intro_02_`.
//!
//! The synthetic tests build a tiny MPEG-1 program stream byte by byte (one
//! grey 16x16 frame, three frames of silent mono MP2) and drive the production
//! [`MoviePlayer`] and [`MoviePlayerPlugin`] with no sound device, so CI runs
//! them. The retail test plays the measured intro movie `chap1.mpg` on the
//! real output device and renders its first seconds on the real GPU; it needs
//! `CS_GAME_DIR` and the `gpu` and `audio` capabilities, is `#[ignore]`d in CI
//! and fails loudly without them. Nobody looked at or listened to the result
//! (`human_review`); nothing here is `verified_original`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::input::ButtonInput;
use bevy::prelude::{App, Image, KeyCode, Messages, MinimalPlugins, MouseButton};
use cs_app::audio::device::CapabilityDeclaration;
use cs_app::cinematics::movie_player::{
    AudioOutput, MovieEnd, MovieFinished, MovieOutcome, MoviePlayer, MoviePlayerPlugin, MoviePoll,
    MoviePresenter, skip_requested,
};
use cs_sim::cinematic_state::MediaFailure;
use cs_types::content::{ContentId, ContentKind};

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    fn put(&mut self, count: usize, value: u64) {
        for i in (0..count).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if value >> i & 1 != 0 {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
    }

    fn put_bits(&mut self, bits: &str) {
        for bit in bits.chars().filter(|c| !c.is_whitespace()) {
            self.put(1, u64::from(bit == '1'));
        }
    }

    fn start_code(&mut self, code: u8) {
        assert_eq!(self.bits % 8, 0);
        self.bytes.extend([0x00, 0x00, 0x01, code]);
        self.bits += 32;
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

fn video_elementary_stream() -> Vec<u8> {
    let mut w = BitWriter::default();

    // Sequence header: 16×16, square pixels, 25 fps, default matrices
    w.start_code(0xB3);
    w.put(12, 16);
    w.put(12, 16);
    w.put(4, 1);
    w.put(4, 3);
    w.put(18, 0x3FFFF);
    w.put(1, 1);
    w.put(10, 16);
    w.put_bits("0 0 0");

    // User data, so that the full header size is available
    w.start_code(0xB2);
    for _ in 0..140 {
        w.put(8, 0xAA);
    }

    // Intra picture
    w.start_code(0x00);
    w.put(10, 0); // temporal reference
    w.put(3, 1); // intra
    w.put(16, 0xFFFF); // vbv delay
    w.put(3, 0); // extra bit + align

    // One slice with one macroblock, whose six blocks only have a DC
    // coefficient equal to the predictor
    w.start_code(0x01);
    w.put(5, 1); // quantizer scale
    w.put(1, 0); // no extra information
    w.put_bits("1"); // address increment 1
    w.put_bits("1"); // intra
    for _ in 0..4 {
        w.put_bits("100 10"); // luma DC size 0, end of block
    }
    for _ in 0..2 {
        w.put_bits("00 10"); // chroma DC size 0, end of block
    }
    w.put(4, 0); // align

    w.start_code(0xB7); // sequence end
    w.finish()
}

fn audio_elementary_stream() -> Vec<u8> {
    let mut frames = Vec::new();
    for _ in 0..3 {
        // MPEG-1 layer II, no CRC, 32 kbit/s, 44.1 kHz, mono
        let mut frame = vec![0xFF, 0xFD, 0x10, 0xC0];
        // All bit allocations zero: silence
        frame.resize(144000 * 32 / 44100, 0);
        frames.extend(frame);
    }
    frames
}

fn pes_packet(stream_id: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 1).unwrap();
    let mut packet = vec![0x00, 0x00, 0x01, stream_id];
    packet.extend(length.to_be_bytes());
    packet.push(0x0F); // no PTS
    packet.extend(payload);
    packet
}

pub(super) fn program_stream() -> Vec<u8> {
    let mut w = BitWriter::default();

    // Pack header
    w.start_code(0xBA);
    w.put_bits("0010 000 1");
    w.put(15, 0);
    w.put(1, 1);
    w.put(15, 0);
    w.put_bits("1 1");
    w.put(22, 1);
    w.put(1, 1);

    // System header with one audio and one video stream
    w.start_code(0xBB);
    w.put(16, 6);
    w.put(24, 0x800001);
    w.put(6, 1);
    w.put(5, 0);
    w.put(5, 1);
    w.put(8, 0xFF);

    let mut stream = w.finish();
    stream.extend(pes_packet(0xE0, &video_elementary_stream()));
    stream.extend(pes_packet(0xC0, &audio_elementary_stream()));
    stream.extend([0x00, 0x00, 0x01, 0xB9]);
    stream
}

fn media() -> ContentId {
    ContentId::from_source(ContentKind::Video, "test.cinematic").expect("the video id is valid")
}

fn synthetic_file(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cs_parity5_cabin_intro_02_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, program_stream()).unwrap();
    path
}

fn open(name: &str) -> MoviePlayer {
    MoviePlayer::open(&synthetic_file(name), &media(), &AudioOutput::Silent).unwrap()
}

#[test]
fn accept_parity5_cabin_intro_02_a_movie_shows_its_frame_then_completes_on_the_audio_clock() {
    let mut player = open("complete.mpg");
    assert_eq!(player.total_audio_frames(), 3 * 1152);
    // Before any audio is played nothing is due past frame 0 and nothing ends.
    let frame = match player.poll_at(10).unwrap() {
        MoviePoll::Playing(Some(frame)) => frame,
        other => panic!("expected the first frame, got {other:?}"),
    };
    assert_eq!((frame.index, frame.pts_us), (0, 0));
    assert_eq!(frame.rgba.len(), 16 * 16 * 4);
    assert!(frame.rgba.chunks(4).all(|px| px == [130, 130, 130, 255]));
    assert_eq!(player.poll_at(10).unwrap(), MoviePoll::Playing(None));
    assert_eq!(
        player.poll_at(3 * 1152 / 2).unwrap(),
        MoviePoll::Playing(None)
    );
    assert_eq!(
        player.poll_at(3 * 1152).unwrap(),
        MoviePoll::Finished(MovieEnd::Completed)
    );
}

#[test]
fn accept_parity5_cabin_intro_02_skip_ends_the_movie_as_skipped_not_completed() {
    let mut player = open("skip.mpg");
    player.poll_at(5).unwrap();
    assert_eq!(player.skip().unwrap(), MovieEnd::Skipped);
    assert_eq!(
        player.poll_at(3 * 1152).unwrap(),
        MoviePoll::Finished(MovieEnd::Skipped),
        "a skipped movie stays skipped even when the audio clock then reaches the end"
    );
}

#[test]
fn accept_parity5_cabin_intro_02_missing_and_foreign_files_are_media_failures() {
    let missing = MoviePlayer::open(
        Path::new("/nonexistent/chap1.mpg"),
        &media(),
        &AudioOutput::Silent,
    )
    .unwrap_err();
    assert!(
        matches!(missing, MediaFailure::MissingMedia { .. }),
        "{missing:?}"
    );

    let dir =
        std::env::temp_dir().join(format!("cs_parity5_cabin_intro_02_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let foreign = dir.join("foreign.mpg");
    std::fs::write(&foreign, b"RIFF not an mpeg program stream at all").unwrap();
    let error = MoviePlayer::open(&foreign, &media(), &AudioOutput::Silent).unwrap_err();
    assert!(
        matches!(
            error,
            MediaFailure::MissingDecoder { .. } | MediaFailure::DecodeFailed { .. }
        ),
        "{error:?}"
    );
}

#[test]
fn accept_parity5_cabin_intro_02_a_missing_audio_capability_is_refused_by_name() {
    let error = MoviePlayer::open(
        &synthetic_file("nodevice.mpg"),
        &media(),
        &AudioOutput::Device(CapabilityDeclaration::parse("retail,gpu")),
    )
    .unwrap_err();
    match error {
        MediaFailure::DecodeFailed { detail } => assert!(detail.contains("audio"), "{detail}"),
        other => panic!("expected a named audio refusal, got {other:?}"),
    }
}

#[test]
fn accept_parity5_cabin_intro_02_escape_space_return_and_a_left_click_skip() {
    let press = |code: KeyCode| {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(code);
        keys
    };
    for code in [KeyCode::Escape, KeyCode::Space, KeyCode::Enter] {
        assert!(skip_requested(Some(&press(code)), None), "{code:?}");
    }
    assert!(!skip_requested(Some(&press(KeyCode::KeyA)), None));
    let mut mouse = ButtonInput::<MouseButton>::default();
    mouse.press(MouseButton::Left);
    assert!(skip_requested(None, Some(&mouse)));
    let mut right = ButtonInput::<MouseButton>::default();
    right.press(MouseButton::Right);
    assert!(!skip_requested(None, Some(&right)));
    assert!(!skip_requested(None, None));
}

fn presenter_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_plugins(MoviePlayerPlugin);
    app
}

fn finished(app: &mut App) -> Vec<MovieFinished> {
    app.world_mut()
        .resource_mut::<Messages<MovieFinished>>()
        .drain()
        .collect()
}

#[test]
fn accept_parity5_cabin_intro_02_the_plugin_reports_a_natural_end_as_one_completed_event() {
    let mut app = presenter_app();
    let player = open("plugin_end.mpg");
    app.world_mut()
        .resource_mut::<MoviePresenter>()
        .start(player);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut events = Vec::new();
    while events.is_empty() {
        assert!(Instant::now() < deadline, "the movie never ended");
        app.update();
        std::thread::sleep(Duration::from_millis(5));
        events.extend(finished(&mut app));
    }
    assert_eq!(
        events,
        vec![MovieFinished {
            media: media(),
            outcome: MovieOutcome::Completed
        }]
    );
    let presenter = app.world().resource::<MoviePresenter>();
    assert!(!presenter.is_playing());
    assert!(presenter.frames_shown() >= 1, "the frame was put on screen");
    app.update();
    assert!(finished(&mut app).is_empty(), "the end is reported once");
}

#[test]
fn accept_parity5_cabin_intro_02_the_plugin_reports_escape_and_a_click_as_skipped() {
    for escape in [true, false] {
        let mut app = presenter_app();
        let player = open("plugin_skip.mpg");
        app.world_mut()
            .resource_mut::<MoviePresenter>()
            .start(player);
        app.update();
        assert!(app.world().resource::<MoviePresenter>().is_playing());
        if escape {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        } else {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
        app.update();
        assert_eq!(
            finished(&mut app),
            vec![MovieFinished {
                media: media(),
                outcome: MovieOutcome::Skipped
            }],
            "escape: {escape}"
        );
        assert!(!app.world().resource::<MoviePresenter>().is_playing());
    }
}

/// The measured intro movie, played on the real device and rendered on the
/// real GPU for its first seconds; the screenshot is kept under
/// `private/evidence/PARITY5-CABIN-INTRO.02/`.
#[test]
#[ignore = "requires CS_GAME_DIR and the gpu and audio capabilities"]
fn accept_parity5_cabin_intro_02_retail_chap1_plays_with_audio_and_renders_its_first_seconds() {
    use bevy::app::PluginGroup;
    use bevy::prelude::{Assets, DefaultPlugins, Handle, On, Res, Resource, WindowPlugin};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
    use std::sync::Mutex;

    let game = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    );
    let declaration = CapabilityDeclaration::from_environment();
    let media = ContentId::from_source(ContentKind::Video, "chap1").unwrap();
    let player = MoviePlayer::open(
        &game.join("GOSDATA/ASSETS/GRAPHICS/MPG/chap1.mpg"),
        &media,
        &AudioOutput::Device(declaration),
    )
    .expect("chap1.mpg opens with the audio device");
    assert_eq!((player.info().width, player.info().height), (320, 240));

    #[derive(Resource, Default)]
    struct Readback(Mutex<Option<Vec<u8>>>);

    let mut app = App::new();
    app.init_resource::<Readback>();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..WindowPlugin::default()
            }),
    );
    app.add_plugins(MoviePlayerPlugin);
    app.finish();
    app.cleanup();
    let (width, height) = (800_u32, 600_u32);
    let mut target = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 0, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    target.texture_descriptor.usage = TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    let target: Handle<Image> = app.world_mut().resource_mut::<Assets<Image>>().add(target);
    {
        let mut presenter = app.world_mut().resource_mut::<MoviePresenter>();
        presenter.render_to(target.clone());
        presenter.start(player);
    }
    let out = PathBuf::from(
        std::env::var("CS_EVIDENCE_DIR")
            .unwrap_or_else(|_| "private/evidence/PARITY5-CABIN-INTRO.02".to_owned()),
    );
    std::fs::create_dir_all(&out).unwrap();
    let png = out.join("chap1_first_seconds.png");
    let path = png.clone();
    app.add_observer(
        move |captured: On<ScreenshotCaptured>, readback: Res<Readback>| {
            *readback.0.lock().unwrap() = captured.image.data.clone();
            save_to_disk(path.clone())(captured);
        },
    );

    // Play until the audio clock has passed 3 s (the movie is 130 s long), then
    // ask for the picture.
    let deadline = Instant::now() + Duration::from_secs(60);
    while app.world().resource::<MoviePresenter>().clock_us() < 3_000_000 {
        assert!(
            Instant::now() < deadline,
            "the audio clock never reached 3 s"
        );
        app.update();
        assert!(
            finished(&mut app).is_empty(),
            "the intro must still be playing after 3 s"
        );
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(app.world().resource::<MoviePresenter>().frames_shown() > 30);
    app.world_mut().spawn(Screenshot::image(target));
    for _ in 0..40 {
        app.update();
        if app
            .world()
            .resource::<Readback>()
            .0
            .lock()
            .unwrap()
            .is_some()
        {
            break;
        }
    }
    let data = app
        .world()
        .resource::<Readback>()
        .0
        .lock()
        .unwrap()
        .take()
        .expect("the renderer returned the frame");
    assert_eq!(data.len(), (width * height * 4) as usize);
    // The picture fills the 4:3 surface: it is not the clear colour and not
    // flat. (The measured file letterboxes itself with black rows.)
    let magenta = data
        .chunks(4)
        .filter(|px| px[0] > 250 && px[1] < 5 && px[2] > 250)
        .count();
    assert_eq!(
        magenta, 0,
        "the clear colour shows: the movie did not cover the target"
    );
    let lit = data
        .chunks(4)
        .filter(|px| px[0].max(px[1]).max(px[2]) > 40)
        .count();
    assert!(lit > 10_000, "the picture is dark: {lit} lit pixels");
    assert!(png.exists());
    // Escape ends the movie as Skipped on the real path as well.
    // The real input plugin owns `ButtonInput`; press through its message.
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: bevy::input::keyboard::Key::Escape,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: bevy::prelude::Entity::PLACEHOLDER,
        });
    app.update();
    assert_eq!(
        finished(&mut app).first().map(|f| f.outcome.clone()),
        Some(MovieOutcome::Skipped)
    );
}
