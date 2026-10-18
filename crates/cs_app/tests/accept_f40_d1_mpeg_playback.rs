//! Acceptance tests of the MPEG-1/MP2 decoder behind `FrameSource` (F40-D1).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-D1`.
//!
//! The synthetic tests decode a program stream built byte by byte in this file
//! (one grey 16x16 intra frame, three frames of silent mono MP2), so CI runs
//! them without the original data. The retail test decodes the ten original
//! cinematics from `$CS_GAME_DIR` through the same production sources and
//! compares frame count, size, frame rate and audio length with the measured
//! baseline of `docs/findings/2026-10-13-f40-d1-mpeg-decoder-approval-required.md`
//! (ffprobe 8.1.1, an independent decoder). It needs `CS_GAME_DIR`; run it
//! with `--include-ignored`.

use std::io::{self, Cursor, Read};
use std::path::PathBuf;

use cs_app::cinematics::mpeg::{MPEG_PS_FORMAT, MpegAudioSource, MpegVideoSource, open_failure};
use cs_app::cinematics::playback::{FrameDue, FrameSource, MediaClock, VideoPlayback};
use cs_app::cinematics::{MediaAvailability, begin, lower_cinematic};
use cs_content::cinematics::declared_synthetic_cinematic;
use cs_sim::cinematic_state::{CinematicState, MediaFailure, SYNTHETIC_PLAYER};
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

fn program_stream() -> Vec<u8> {
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

#[test]
fn accept_f40_d1_synthetic_stream_decodes_video_and_audio_through_the_sources() {
    let mut video = MpegVideoSource::new(Cursor::new(program_stream()), &media()).unwrap();
    let info = video.info();
    assert_eq!((info.width, info.height), (16, 16));
    assert!((info.frame_rate - 25.0).abs() < f64::EPSILON);
    assert!(video.current_rgba().is_none(), "no frame before the first");

    let frame = video.next_frame().unwrap().expect("one frame");
    assert_eq!((frame.index, frame.pts_us), (0, 0));
    let rgba = video.current_rgba().unwrap();
    assert_eq!(rgba.len(), 16 * 16 * 4);
    assert!(rgba.chunks(4).all(|px| px == [130, 130, 130, 255]));
    assert!(video.next_frame().unwrap().is_none());
    assert!(video.current_rgba().is_none());

    let mut audio = MpegAudioSource::new(Cursor::new(program_stream()), &media()).unwrap();
    assert_eq!(audio.sample_rate(), 44_100);
    let mut chunks = 0;
    while let Some(samples) = audio.next_samples().unwrap() {
        assert_eq!(samples.len(), 2 * 1152, "stereo, 1152 samples per frame");
        assert!(samples.iter().all(|&s| s == 0.0));
        chunks += 1;
    }
    assert_eq!(chunks, 3);
}

#[test]
fn accept_f40_d1_decoded_frames_are_presented_against_the_master_clock() {
    let source = MpegVideoSource::new(Cursor::new(program_stream()), &media()).unwrap();
    let mut playback = VideoPlayback::new(source, 40_000);
    let mut clock = MediaClock::new(44_100).unwrap();
    clock.audio_played(10);
    match playback.frame_due(&clock).unwrap() {
        FrameDue::Show { frame, dropped } => assert_eq!((frame.index, dropped), (0, 0)),
        other => panic!("expected the frame, got {other:?}"),
    }
    assert_eq!(playback.frame_due(&clock).unwrap(), FrameDue::Ended);
}

#[test]
fn accept_f40_d1_missing_file_is_a_media_failure_and_never_a_completion() {
    let missing = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("f40-d1-absent.mpg");
    let _ = std::fs::remove_file(&missing);
    let failure = MpegVideoSource::open(&missing, &media()).unwrap_err();
    assert_eq!(failure, MediaFailure::MissingMedia { media: media() });
    let failure = MpegAudioSource::open(&missing, &media()).unwrap_err();
    assert_eq!(failure, MediaFailure::MissingMedia { media: media() });

    // The failure lands in the player as a Failed state with a semantic end
    // that is not a completion.
    let plan = lower_cinematic(&declared_synthetic_cinematic()).unwrap();
    let mut player = begin(&plan, &MediaAvailability::Present, SYNTHETIC_PLAYER).unwrap();
    player.media_failed(failure).unwrap();
    assert!(matches!(
        player.state(),
        CinematicState::Failed {
            reason: MediaFailure::MissingMedia { .. },
            ..
        }
    ));
}

#[test]
fn accept_f40_d1_a_file_that_is_not_an_mpeg_program_stream_names_the_missing_decoder() {
    let failure =
        MpegVideoSource::new(Cursor::new(b"RIFF....AVI LIST".to_vec()), &media()).unwrap_err();
    assert_eq!(
        failure,
        MediaFailure::MissingDecoder {
            format: MPEG_PS_FORMAT.to_owned()
        }
    );
    let failure = MpegAudioSource::new(Cursor::new(Vec::new()), &media()).unwrap_err();
    assert!(
        matches!(failure, MediaFailure::MissingDecoder { .. }),
        "{failure:?}"
    );
}

/// A reader whose source breaks after `good` bytes.
#[derive(Debug)]
struct Breaks {
    inner: Cursor<Vec<u8>>,
    good: u64,
}

impl Read for Breaks {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.inner.position() >= self.good {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "disk gone"));
        }
        self.inner.read(buf)
    }
}

#[test]
fn accept_f40_d1_a_read_failure_is_a_decode_failure_and_open_errors_map_to_failures() {
    // Unreadable from the first byte: opening reports the decoder's message.
    let failure = MpegVideoSource::new(
        Breaks {
            inner: Cursor::new(program_stream()),
            good: 0,
        },
        &media(),
    )
    .unwrap_err();
    assert!(
        matches!(&failure, MediaFailure::DecodeFailed { detail } if detail.contains("disk gone")),
        "{failure:?}"
    );

    let denied = slopmpeg::Error::Io(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
    assert!(matches!(
        open_failure(&denied, &media()),
        MediaFailure::DecodeFailed { .. }
    ));
    let absent = slopmpeg::Error::Io(io::Error::new(io::ErrorKind::NotFound, "absent"));
    assert_eq!(
        open_failure(&absent, &media()),
        MediaFailure::MissingMedia { media: media() }
    );
    assert!(matches!(
        open_failure(&slopmpeg::Error::MissingVideoHeader, &media()),
        MediaFailure::DecodeFailed { .. }
    ));
}

/// The measured baseline of the ten retail cinematics: file, coded video
/// pictures, sequence-header frame rate, duration in seconds.
///
/// The picture counts are read straight from the bitstream (the number of
/// picture start codes in each file's video packets, I+P+B), independently of
/// the decoder under test. ffprobe 8.1.1 reports one packet more in nine
/// of the ten files (`docs/findings/2026-10-13-f40-d1-mpeg-decoder-approval-required.md`;
/// msopen1 agrees); that extra packet is not a coded picture. The durations are ffprobe's.
const RETAIL: [(&str, u64, f64, f64); 10] = [
    ("chap0", 4349, 30.0, 144.953_467),
    ("chap1", 3927, 30.0, 130.821_222),
    ("chap2", 3699, 30.0, 123.271_833),
    ("chap3", 2785, 30.0, 92.734_689),
    ("chap4", 4125, 30.0, 137.483_333),
    ("chap5", 2875, 30.0, 95.816_667),
    ("crimflag", 241, 30.0, 8.016_667),
    ("final", 3331, 30.0, 111.020_400),
    ("msopen1", 405, 29.97, 13.531_422),
    ("zipper", 609, 30.0, 20.283_333),
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f40_d1_retail_cinematics_match_the_measured_baseline() {
    let root = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    for (name, frames, frame_rate, seconds) in RETAIL {
        let path = root
            .join("GOSDATA/ASSETS/GRAPHICS/MPG")
            .join(format!("{name}.mpg"));
        let id = ContentId::from_source(ContentKind::Video, name).unwrap();

        let mut video = MpegVideoSource::open(&path, &id).unwrap_or_else(|f| panic!("{name}: {f}"));
        let info = video.info();
        assert_eq!((info.width, info.height), (320, 240), "{name} size");
        assert!((info.frame_rate - frame_rate).abs() < 0.001, "{name} rate");
        let duration = video.duration().unwrap().expect("a duration").as_secs_f64();
        // The decoder's duration is the video stream's last timestamp; ffprobe's
        // is the container's, which includes the audio tail.
        assert!(
            (duration - seconds).abs() < 0.5,
            "{name} duration {duration}"
        );

        let mut count = 0_u64;
        let mut last_pts = 0_u64;
        while let Some(frame) = video.next_frame().unwrap() {
            assert_eq!(frame.index, count, "{name} index");
            assert!(frame.pts_us >= last_pts, "{name} pts order at {count}");
            last_pts = frame.pts_us;
            assert_eq!(video.current_rgba().unwrap().len(), 320 * 240 * 4);
            count += 1;
        }
        assert_eq!(count, frames, "{name} frame count");
        let by_count = count as f64 / frame_rate;
        assert!(
            (by_count - seconds).abs() < 0.2,
            "{name} {count} frames last {by_count}s"
        );
        let last_s = last_pts as f64 / 1_000_000.0;
        assert!((last_s - seconds).abs() < 0.2, "{name} last pts {last_s}");

        let mut audio = MpegAudioSource::open(&path, &id).unwrap_or_else(|f| panic!("{name}: {f}"));
        assert_eq!(audio.sample_rate(), 44_100, "{name} audio rate");
        let mut samples = 0_u64;
        while let Some(chunk) = audio.next_samples().unwrap() {
            assert_eq!(chunk.len(), 2 * 1152);
            samples += 1152;
        }
        let audio_s = samples as f64 / 44_100.0;
        assert!((audio_s - seconds).abs() < 0.2, "{name} audio {audio_s}s");
    }
}
