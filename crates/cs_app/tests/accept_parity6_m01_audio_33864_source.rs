//! Acceptance for Rally #2030 `PARITY6-M01-AUDIO-33864-SOURCE`: where the 33,864 B
//! M01 loop cannot come from (finding
//! `docs/findings/2026-10-24-m01-audio-33864-source.md`).
//!
//! The ignored image test re-reads every cited byte of the decrypted engine
//! image and every direct call of the sound-source constructors and the buffer
//! dispatcher in `.text`, so a site cannot be missing or invented. The ignored
//! retail test measures every WAVE blob of every container with the production
//! helper [`cs_app::audio::pcm_size::decoded_pcm_size`] and reads the audio
//! header of each `.mpg`. The source of the loop stays unknown (AGENTS rule 4).
//! Task test prefix: `accept_parity6_m01_audio_33864_`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cs_app::audio::pcm_size::decoded_pcm_size;

const IMAGE_BASE: usize = 0x40_0000;
const TEXT: std::ops::Range<usize> = 0x40_1000..0x60_24bd;
const UNRESOLVED_BYTES: u64 = 33_864;

/// `(va, bytes)`: facts of the finding, `static` provenance.
const CITED: &[(usize, &[u8])] = &[
    // zsnd_cd.cpp is the MCI CD-audio player: its source path and device words.
    (0x63_a5bc, b"D:\\zipper\\gamez\\zSound\\zsnd_cd.cpp\0"),
    (0x63_a5e0, b"cdaudio\0"),
    (0x63_a5e8, b"CDAudio\0"),
    (0x63_a5f8, b"MCIError"),
    // Wave source parser: the `data` chunk stores its pointer at +0x28 and its
    // declared size at +0x10 (`mov [ecx+0x28],eax; mov [ecx+0x10],esi`).
    (
        0x59_b108,
        &[
            0x8b, 0x51, 0x28, 0x85, 0xd2, 0x74, 0x07, 0x8b, 0x51, 0x10, 0x85, 0xd2, 0x75, 0x39,
            0x89, 0x41, 0x28,
        ],
    ),
    // After the ACM conversion: `mov [ebp+0x10],eax; mov [ebp+8],eax` where eax
    // was loaded from the acmStreamSize output slot `[esp+0x20]`.
    (0x59_b3a2, &[0x8b, 0x44, 0x24, 0x20, 0x8b, 0x54, 0x24, 0x18]),
    (0x59_b3aa, &[0x89, 0x45, 0x10, 0x89, 0x45, 0x08]),
    (0x59_b25e, &[0x8b, 0x4c, 0x24, 0x1c]),
    // The ACM thunks: acmStreamSize (0x5f8e14) and acmFormatSuggest (0x5f8e08).
    (0x5f_8e14, &[0xff, 0x25, 0xe8, 0x00, 0xa2, 0x00]),
    (0x5f_8e08, &[0xff, 0x25, 0xf4, 0x00, 0xa2, 0x00]),
    // Files whose name starts "mu" are not read by the file constructor.
    (0x59_b403, &[0x80, 0x38, 0x6d, 0x75, 0x06, 0x80, 0x78]),
    // FMV audio (fmv_stream.cpp): non-streaming size is AVIStreamLength times
    // dwSampleSize (`[esi+0x39c]`); the streaming ring is twice [esi+0x3fc].
    (
        0x5f_94b8,
        &[
            0x8b, 0x07, 0x50, 0xe8, 0xd2, 0x30, 0x00, 0x00, 0x0f, 0xaf, 0x86, 0x9c, 0x03, 0x00,
            0x00,
        ],
    ),
    (0x5f_947e, &[0xd1, 0xe0, 0x50]),
    (0x5f_9404, &[0x8b, 0x86, 0x94, 0x03, 0x00, 0x00]),
];

/// Direct callers (`E8 rel32`) of each routine of the sound-source path.
const CALLERS: &[(usize, &[usize])] = &[
    // dispatcher to the DirectSound buffer creators
    (0x59_b580, &[0x59_65b2, 0x59_6d1d, 0x59_6dd3, 0x59_bfd0]),
    // source from a name (flag 0: archive reader; flag 1: fopen)
    (0x59_afc0, &[0x59_6598, 0x59_6cf8, 0x59_6dae, 0x59_bfa1]),
    // source from a memory image
    (0x59_b010, &[0x5c_ff6c]),
    // fopen reader
    (0x59_b3f0, &[0x59_affd]),
    // archive reader
    (0x59_b4f0, &[0x59_6d11, 0x59_6dc7]),
    // RIFF walk and ACM decode
    (0x59_b090, &[0x59_b049, 0x59_b455, 0x59_b55e]),
    (0x59_b170, &[0x59_b05b, 0x59_b467, 0x59_b570]),
    // source from raw memory (name, data, length)
    (0x59_bf80, &[0x5c_ff7d, 0x5f_9483, 0x5f_954b]),
];

fn image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    std::fs::read(path).expect("read the engine image")
}

fn file_offset(va: usize) -> usize {
    // .text maps file 0x1000 -> va 0x401000; .rdata/.data from file 0x203000 -> va 0x603000.
    if TEXT.contains(&va) {
        va - 0x40_1000 + 0x1000
    } else {
        va - 0x60_3000 + 0x20_3000
    }
}

fn callers_of(image: &[u8], target: usize) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for va in TEXT.start..TEXT.end - 5 {
        let at = file_offset(va);
        if image[at] != 0xe8 {
            continue;
        }
        let rel = i32::from_le_bytes(image[at + 1..at + 5].try_into().unwrap());
        if (va as i64 + 5 + i64::from(rel)) as usize == target {
            out.insert(va);
        }
    }
    out
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity6_m01_audio_33864_cited_bytes_and_call_sites_match_the_engine_image() {
    let image = image();
    for (va, bytes) in CITED {
        let at = file_offset(*va);
        assert_eq!(&image[at..at + bytes.len()], *bytes, "bytes at {va:#x}");
    }
    for (target, sites) in CALLERS {
        let want: BTreeSet<usize> = sites.iter().copied().collect();
        assert_eq!(callers_of(&image, *target), want, "callers of {target:#x}");
    }
    // zsnd_cd.cpp (`.text` 0x598100..0x598e80) reaches none of the source
    // constructors, the dispatcher or the buffer creators.
    let cd = 0x59_8100..0x59_8e80;
    for (target, _) in CALLERS
        .iter()
        .chain(&[(0x59_b5e0, &[][..]), (0x59_b950, &[][..])])
    {
        assert!(
            callers_of(&image, *target)
                .iter()
                .all(|va| !cd.contains(va)),
            "zsnd_cd.cpp calls {target:#x}"
        );
    }
    assert_eq!(image_base_sanity(&image), IMAGE_BASE);
}

/// The PE image base, read from the optional header.
fn image_base_sanity(image: &[u8]) -> usize {
    let pe = u32::from_le_bytes(image[0x3c..0x40].try_into().unwrap()) as usize;
    u32::from_le_bytes(image[pe + 0x34..pe + 0x38].try_into().unwrap()) as usize
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("readable directory")
        .map(|entry| entry.expect("entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// The first MPEG audio frame header after the first audio PES packet.
fn mpeg_audio_header(bytes: &[u8]) -> u32 {
    let pes = bytes
        .windows(4)
        .position(|w| w == [0, 0, 1, 0xc0])
        .expect("an audio PES packet");
    let rest = &bytes[pes..(pes + 400_000).min(bytes.len())];
    // PES headers may carry 0xff stuffing, so take the first plausible
    // MPEG-1 Layer II header (bitrate index 1..=14, rate index not 3).
    let at = (0..rest.len() - 4)
        .find(|&i| {
            let h = u32::from_be_bytes(rest[i..i + 4].try_into().unwrap());
            h >> 20 == 0xfff
                && (h >> 19) & 3 == 3
                && (h >> 17) & 3 == 2
                && (1..=14).contains(&((h >> 12) & 15))
                && (h >> 10) & 3 != 3
        })
        .expect("an audio frame sync");
    u32::from_be_bytes(rest[at..at + 4].try_into().unwrap())
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_33864_no_blob_and_no_fmv_stream_has_this_length() {
    let root = PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    );
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut blobs = 0usize;
    let mut mpgs = 0usize;
    for path in files {
        let bytes = std::fs::read(&path).expect("file readable");
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mpg"))
        {
            // MPEG-1 (version bits 3), Layer II (2), 44.1 kHz (rate index 0):
            // the movies' audio is not the 11,025 Hz stream of the loop.
            let header = mpeg_audio_header(&bytes);
            assert_eq!((header >> 19) & 3, 3, "{path:?} MPEG-1");
            assert_eq!((header >> 17) & 3, 2, "{path:?} layer II");
            assert_eq!((header >> 10) & 3, 0, "{path:?} 44.1 kHz");
            mpgs += 1;
            continue;
        }
        let mut at = 0;
        while let Some(hit) = bytes[at..].windows(4).position(|w| w == b"RIFF") {
            let start = at + hit;
            at = start + 4;
            if start + 12 > bytes.len() || &bytes[start + 8..start + 12] != b"WAVE" {
                continue;
            }
            let declared = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap());
            let end = start + 8 + declared as usize;
            if end > bytes.len() {
                continue;
            }
            let member = &bytes[start..end];
            // Neither the decoded length nor half of it (8-bit data read as
            // 16-bit) is 33,864.
            if let Ok(size) = decoded_pcm_size(member) {
                assert_ne!(size.pcm_bytes, UNRESOLVED_BYTES, "{path:?} at {start:#x}");
                assert_ne!(
                    size.pcm_bytes / 2,
                    UNRESOLVED_BYTES,
                    "{path:?} at {start:#x}"
                );
                blobs += 1;
            }
        }
    }
    assert_eq!(mpgs, 10);
    assert!(blobs >= 5_049, "{blobs}");
}
