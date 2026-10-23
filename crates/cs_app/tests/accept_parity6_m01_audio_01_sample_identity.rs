//! PARITY6-M01-AUDIO-TIMELINE.01 (#1861): which retail sound members decode to
//! the byte lengths the original's Drydock buffers have.
//!
//! Task test prefix: `accept_parity6_m01_audio_01_`. Finding:
//! `docs/findings/2026-10-22-m01-audio-timeline-sample-identity.md`.
//!
//! The retail scenarios read the installation named by `CS_GAME_DIR` and fail
//! loudly without it. They call the production header reader through
//! [`cs_app::audio::pcm_size::decoded_pcm_size`] and the production `.zrd`
//! decoder; the numbers pinned here are the finding's table.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_app::audio::pcm_size::{PcmSizeError, decoded_pcm_size};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    ZbdProbe, dispatch, read_reader_archive, read_sound_archive, read_version_one_index,
};

/// The Drydock buffer length no retail member decodes to.
const UNRESOLVED_BYTES: u64 = 33_864;

pub(crate) fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this test"),
    )
}

/// One archive member: name, format tag, channels, rate, decoded bytes.
pub(crate) type Row = (String, u16, u16, u32, u64);

pub(crate) fn archive_rows(root: &Path, container: &str) -> Vec<Row> {
    let bytes = std::fs::read(root.join(container)).expect("sound archive readable");
    let spelling = cs_types::install::RelativePath::new(container).expect("relative path");
    let mut context = ParseContext::with_defaults("parity6_audio_01");
    let decision = dispatch(ZbdProbe::new(container, &spelling, &bytes)).expect("dispatch");
    let index = read_version_one_index(&mut context, decision, &bytes).expect("index");
    let table = index.member_table();
    let archive = read_sound_archive(&mut context, &table, index.data()).expect("archive");
    archive
        .entries()
        .map(|entry| {
            let name = String::from_utf8_lossy(entry.name()).into_owned();
            let size = decoded_pcm_size(entry.content())
                .unwrap_or_else(|error| panic!("{container}/{name}: {error:?}"));
            (
                name,
                size.format_tag,
                size.channels,
                size.rate_hz,
                size.pcm_bytes,
            )
        })
        .collect()
}

/// Member names of `rows` that decode to `bytes`, sorted.
fn members_of(rows: &[Row], bytes: u64) -> Vec<String> {
    let mut names: Vec<String> = rows
        .iter()
        .filter(|row| row.4 == bytes)
        .map(|row| row.0.clone())
        .collect();
    names.sort();
    names
}

pub(crate) fn sounds_zrd(root: &Path) -> ZrdValue {
    let container = "ZBD/zrdr.zbd";
    let bytes = std::fs::read(root.join(container)).expect("zrdr.zbd readable");
    let spelling = cs_types::install::RelativePath::new(container).expect("relative path");
    let mut context = ParseContext::with_defaults("parity6_audio_01");
    let decision = dispatch(ZbdProbe::new(container, &spelling, &bytes)).expect("dispatch");
    let index = read_version_one_index(&mut context, decision, &bytes).expect("index");
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("archive");
    let entry = archive
        .entries()
        .find(|e| String::from_utf8_lossy(e.name()).eq_ignore_ascii_case("sounds.zrd"))
        .expect("sounds.zrd");
    decode_zrd(entry.content()).expect("sounds.zrd decodes")
}

/// `(group name, member file, flags)` of every sound record in `sounds.zrd`.
fn sound_records(node: &ZrdValue, out: &mut Vec<(String, String, Vec<String>)>) {
    let Some(children) = node.as_list() else {
        return;
    };
    let name = children.first().and_then(ZrdValue::as_text);
    let file = children.get(1).and_then(ZrdValue::as_text);
    if let (Some(name), Some(file)) = (name, file)
        && file.to_ascii_lowercase().ends_with(".wav")
    {
        let flags = children[2..]
            .iter()
            .filter_map(|c| c.as_text().map(str::to_owned))
            .collect();
        out.push((name.to_owned(), file.to_owned(), flags));
        return;
    }
    for child in children {
        sound_records(child, out);
    }
}

/// Hand-built RIFF/WAVE with a bare 16-byte `fmt ` and `data` bytes of zeros.
fn synthetic_wave(tag: u16, channels: u16, rate: u32, align: u16, bits: u16, data: u32) -> Vec<u8> {
    let mut fmt = Vec::new();
    fmt.extend(tag.to_le_bytes());
    fmt.extend(channels.to_le_bytes());
    fmt.extend(rate.to_le_bytes());
    fmt.extend((rate * u32::from(align)).to_le_bytes());
    fmt.extend(align.to_le_bytes());
    fmt.extend(bits.to_le_bytes());
    let mut body = b"WAVE".to_vec();
    body.extend(b"fmt ");
    body.extend((fmt.len() as u32).to_le_bytes());
    body.extend(fmt);
    body.extend(b"data");
    body.extend(data.to_le_bytes());
    body.extend(std::iter::repeat_n(0u8, data as usize));
    let mut wave = b"RIFF".to_vec();
    wave.extend((body.len() as u32).to_le_bytes());
    wave.extend(body);
    wave
}

#[test]
fn accept_parity6_m01_audio_01_decoded_length_rule() {
    // MS ADPCM mono, 512-byte blocks: (512 - 7) * 2 + 2 = 1012 samples/block.
    let ms = decoded_pcm_size(&synthetic_wave(2, 1, 22_050, 512, 4, 1024)).expect("ms");
    assert_eq!(ms.pcm_bytes, 2 * 1012 * 2);
    // A trailing partial block counts as a whole block (34,408 = 17 * 2024).
    let partial = decoded_pcm_size(&synthetic_wave(2, 1, 22_050, 512, 4, 8704)).expect("partial");
    assert_eq!(partial.pcm_bytes, 34_408);
    // MS ADPCM stereo: (1024 - 14) * 2 / 2 + 2 = 1012 samples/block/channel.
    let stereo = decoded_pcm_size(&synthetic_wave(2, 2, 22_050, 1024, 4, 1024)).expect("st");
    assert_eq!(stereo.pcm_bytes, 1012 * 2 * 2);
    // IMA mono, 512-byte blocks: (512 - 4) * 2 + 1 = 1017 samples/block.
    let ima = decoded_pcm_size(&synthetic_wave(17, 1, 11_025, 512, 4, 512)).expect("ima");
    assert_eq!(ima.pcm_bytes, 1017 * 2);
    // PCM keeps its length at 16 bit and doubles at 8 bit.
    assert_eq!(
        decoded_pcm_size(&synthetic_wave(1, 1, 11_025, 2, 16, 100))
            .expect("pcm16")
            .pcm_bytes,
        100
    );
    assert_eq!(
        decoded_pcm_size(&synthetic_wave(1, 1, 11_025, 1, 8, 100))
            .expect("pcm8")
            .pcm_bytes,
        200
    );
    // Anything else is refused by name rather than guessed.
    assert_eq!(
        decoded_pcm_size(&synthetic_wave(0x55, 1, 11_025, 1, 0, 8)),
        Err(PcmSizeError::Unsupported { format_tag: 0x55 })
    );
    assert!(matches!(
        decoded_pcm_size(b"not a wave file at all"),
        Err(PcmSizeError::Wave(_))
    ));
}

/// The decoded size → `soundsh.zbd` members table of the finding.
const CANDIDATES: &[(u64, &[&str])] = &[
    (
        287_408,
        &[
            "devastator.wav",
            "devastator_cp.wav",
            "hellhound.wav",
            "kestrel.wav",
            "missile_torpedo_hit.wav",
        ],
    ),
    (
        89_056,
        &[
            "Metalbend3.wav",
            "VO_c3-HW-M5_Zachary_14.wav",
            "introscene4.wav",
        ],
    ),
    (
        119_416,
        &[
            "VO_c1-HA-m3_Zachary_17.wav",
            "VO_c4-RM-m1_Zachary_54.wav",
            "VO_c4-RM-m3_Zachary_39.wav",
            "nujetwind.wav",
            "plane_shake.wav",
            "punching.wav",
        ],
    ),
    (
        34_408,
        &[
            "VO_c4-RM-m2_Zachary_20.wav",
            "hook_release.wav",
            "waterfall.wav",
        ],
    ),
    (
        267_168,
        &[
            "VO_c1-HA-M5_Zachary_23-02.wav",
            "VO_c2-NW-M5_Zachary_19-5.wav",
            "explosion_7.wav",
            "train_derail.wav",
        ],
    ),
    (
        22_264,
        &["ricco4.wav", "tny_exp_2.wav", "warning_shot2.wav"],
    ),
    (
        78_936,
        &[
            "VO_c5-MN-m1_Zachary_21.wav",
            "VO_c5-MN-m1_Zachary_4.wav",
            "VO_id1_GL-AllyDwn-B.wav",
            "exp_water_b.wav",
        ],
    ),
    (16_192, &["ricco3.wav", "warning_seeker.wav"]),
    (18_216, &["ricco2.wav", "warning_shot3.wav"]),
    (
        220_616,
        &[
            "VO_c3-HW-M5_Zachary_18.wav",
            "VO_c4-RM-m3_Zachary_10.wav",
            "VO_c4-RM-m4_Zachary_8.wav",
            "VO_c5-MN-m2_Zachary_26.wav",
            "enginelooped.wav",
            "exp_ground_a.wav",
            "exp_ground_b.wav",
            "exp_plane_1.wav",
            "exp_plane_3.wav",
        ],
    ),
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_01_members_by_decoded_size() {
    let root = game_dir();
    let high = archive_rows(&root, "ZBD/soundsh.zbd");
    let low = archive_rows(&root, "ZBD/soundsl.zbd");
    assert_eq!((high.len(), low.len()), (2521, 2520));
    for (bytes, expected) in CANDIDATES {
        assert_eq!(&members_of(&high, *bytes), expected, "soundsh {bytes}");
        // Every one of these is a 22,050 Hz MS ADPCM member, mono except the
        // stereo airframe loops checked below.
        for row in high.iter().filter(|row| row.4 == *bytes) {
            let channels = if *bytes == 287_408 && row.0 != "missile_torpedo_hit.wav" {
                2
            } else {
                1
            };
            assert_eq!((row.1, row.2, row.3), (2, channels, 22_050), "{}", row.0);
        }
        // The low-rate archive decodes none of them to the same length.
        assert!(members_of(&low, *bytes).is_empty(), "soundsl {bytes}");
    }
    // The airframe loops: stereo MS ADPCM, except the torpedo hit (mono).
    for name in [
        "devastator.wav",
        "devastator_cp.wav",
        "hellhound.wav",
        "kestrel.wav",
    ] {
        let row = high.iter().find(|row| row.0 == name).expect(name);
        assert_eq!(
            (row.1, row.2, row.3, row.4),
            (2, 2, 22_050, 287_408),
            "{name}"
        );
    }
    // The launch one-shot is the 11,025 Hz mono member, identical in both.
    for rows in [&high, &low] {
        assert_eq!(
            members_of(rows, 225_000),
            ["VO_c5-MN-m4_Miles_47.wav", "introscene3.wav"]
        );
        for row in rows.iter().filter(|row| row.4 == 225_000) {
            assert_eq!((row.1, row.2, row.3), (2, 1, 11_025), "{}", row.0);
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_01_looped_groups_narrow_the_candidates() {
    let root = game_dir();
    let high = archive_rows(&root, "ZBD/soundsh.zbd");
    let mut records = Vec::new();
    sound_records(&sounds_zrd(&root), &mut records);
    let looped = |bytes: u64| -> Vec<String> {
        let mut names: Vec<String> = members_of(&high, bytes)
            .into_iter()
            .filter(|name| {
                records.iter().any(|(_, file, flags)| {
                    file.eq_ignore_ascii_case(name) && flags.iter().any(|f| f == "LOOPED")
                })
            })
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        looped(287_408),
        [
            "devastator.wav",
            "devastator_cp.wav",
            "hellhound.wav",
            "kestrel.wav"
        ]
    );
    assert_eq!(looped(119_416), ["nujetwind.wav", "plane_shake.wav"]);
    assert_eq!(looped(34_408), ["waterfall.wav"]);
    assert_eq!(looped(220_616), ["enginelooped.wav"]);
    assert_eq!(looped(267_168), ["train_derail.wav"]);
    assert_eq!(looped(16_192), ["warning_seeker.wav"]);
    assert!(looped(225_000).is_empty());
    assert!(looped(89_056).is_empty());
    // The group names: pdevastator's engine group is the single member, and the
    // wind loop's group is `snd_jetwind1`.
    let group_of = |file: &str| {
        records
            .iter()
            .find(|(_, f, _)| f.eq_ignore_ascii_case(file))
            .map(|(g, _, _)| g.clone())
    };
    assert_eq!(
        group_of("devastator.wav").as_deref(),
        Some("snd_devastatorengine")
    );
    assert_eq!(group_of("nujetwind.wav").as_deref(), Some("snd_jetwind1"));
    assert_eq!(group_of("waterfall.wav").as_deref(), Some("snd_waterfall"));
    assert_eq!(group_of("introscene3.wav").as_deref(), Some("snd_scene3"));
}

/// Every `RIFF....WAVE` blob in any file of the installation, found by
/// scanning raw bytes and measured by the production helper.
fn scan_all_riff(root: &Path) -> BTreeMap<String, Vec<u64>> {
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
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut found: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for path in files {
        let bytes = std::fs::read(&path).expect("file readable");
        let relative = path
            .strip_prefix(root)
            .expect("inside root")
            .to_string_lossy()
            .replace('\\', "/");
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
            if let Ok(size) = decoded_pcm_size(&bytes[start..end]) {
                found
                    .entry(relative.clone())
                    .or_default()
                    .push(size.pcm_bytes);
            }
        }
    }
    found
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_01_33864_matches_nothing_in_the_installation() {
    let root = game_dir();
    let found = scan_all_riff(&root);
    let counts: BTreeMap<&str, usize> = found.iter().map(|(k, v)| (k.as_str(), v.len())).collect();
    // Only the two sound archives and two non-audio containers hold any WAVE.
    assert_eq!(counts["ZBD/soundsh.zbd"], 2521);
    assert_eq!(counts["ZBD/soundsl.zbd"], 2520);
    assert_eq!(counts["GOSDATA/ASSETS/crimson.rof"], 8);
    assert_eq!(counts["SETUPENU.DLL"], 3);
    assert_eq!(counts.len(), 4, "{counts:?}");
    for (file, sizes) in &found {
        assert!(
            !sizes.contains(&UNRESOLVED_BYTES),
            "{file} holds a member decoding to {UNRESOLVED_BYTES} bytes"
        );
    }
    // Nor does any member of either archive decode to the neighbouring
    // lengths a one-sample or one-block rounding would give.
    for rounded in [
        UNRESOLVED_BYTES - 2,
        UNRESOLVED_BYTES + 2,
        UNRESOLVED_BYTES / 2,
    ] {
        for sizes in found.values() {
            assert!(!sizes.contains(&rounded), "{rounded}");
        }
    }
}
