//! Acceptance for task #344: the RIFF/WAVE header of every sound archive
//! member, read by production code (`cs_formats::zbd::read_wave_header`) and
//! reported as the entry's `SoundDescriptor`
//! (`docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md`).
//!
//! The synthetic `accept_t344_*` tests author every byte here: WAVE members
//! laid out as the RIFF specification (IBM/Microsoft 1991) describes, packed
//! into archives with the version-one trailer (task #343). The retail test
//! reads the installation at `$CS_GAME_DIR` (never writes it) and fails
//! loudly without it. `evidence_report_t344_writes_the_acceptance_report` is
//! the evidence harness (`docs/contracts/CLI-EVIDENCE.md`), not an
//! acceptance test.

use std::collections::BTreeMap;
use std::fs;

use cs_formats::ParseContext;
use cs_formats::zbd::{
    ContainerStatus, NO_LOOP_CHUNK_REASON, SAMPLES_NOT_DECODED_REASON, SMPL_NOT_READ_REASON,
    SoundArchive, SoundField, UNNAMED_FORMAT_REASON, VersionOneIndex, WAVE_FORMAT_IMA_ADPCM,
    WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_PCM, WaveError, ZbdFamily, ZbdProbe, dispatch,
    read_sound_archive, read_version_one_index, read_wave_header,
};
use cs_types::evidence::SourceSpan;

use super::t340::{
    artifact, command_output, env_var, game_dir, git, iso_utc_now, jstr, locked_version,
    parse_suite, path, retail_zbd_files, short_name, workspace_path,
};

/// Provenance label carried by every synthetic result.
const CONTAINER: &str = "synthetic/t344_sounds.zbd";

/// One authored chunk: id and payload (padded on output when odd).
fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

/// A `fmt ` payload: the 16 common bytes, then `extra` format-specific bytes.
fn fmt(tag: u16, channels: u16, rate: u32, block_align: u16, bits: u16, extra: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    let avg = rate * u32::from(block_align);
    bytes.extend_from_slice(&avg.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&bits.to_le_bytes());
    bytes.extend_from_slice(extra);
    bytes
}

/// A `cue ` payload with `points` authored cue points.
fn cue(points: u32) -> Vec<u8> {
    let mut bytes = points.to_le_bytes().to_vec();
    for point in 0..points {
        for word in [point + 1, point * 100] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(b"data");
        for word in [0u32, 0, point * 100] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes
}

/// `RIFF`, the size, `WAVE` and `chunks`.
fn wave(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&body);
    bytes
}

/// A 16-bit mono PCM member at 22050 Hz with four authored samples.
fn pcm_member() -> Vec<u8> {
    wave(&[
        chunk(b"fmt ", &fmt(WAVE_FORMAT_PCM, 1, 22_050, 2, 16, &[])),
        chunk(b"data", &[1, 0, 2, 0, 3, 0, 4, 0]),
    ])
}

/// `members` back to back, then a version-one index naming them and the
/// trailer (task #343's layout).
fn archive(members: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut index = Vec::new();
    for (name, member) in members {
        index.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        index.extend_from_slice(&(member.len() as u32).to_le_bytes());
        let mut field = [0u8; 64];
        field[..name.len()].copy_from_slice(name);
        index.extend_from_slice(&field);
        index.extend_from_slice(&[0u8; 76]);
        bytes.extend_from_slice(member);
    }
    bytes.extend_from_slice(&index);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// Reads `bytes` as `ZBD/soundsl.zbd` through dispatch, the trailer index
/// and the sound reader, and hands the archive to `check`.
fn with_sound_archive(bytes: &[u8], check: impl FnOnce(&SoundArchive<'_>)) {
    let spelling = path("ZBD/soundsl.zbd");
    let probe = &bytes[..bytes.len().min(64)];
    let decided = dispatch(ZbdProbe::new(CONTAINER, &spelling, probe)).expect("dispatches");
    assert_eq!(decided.family(), ZbdFamily::Sound);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let index: VersionOneIndex<'_> =
        read_version_one_index(&mut context, decided, bytes).expect("the index reads");
    let table = index.member_table();
    let sound = read_sound_archive(&mut context, &table, index.data()).expect("sound reads");
    check(&sound);
}

fn span(offset: u64, length: u64) -> SourceSpan {
    SourceSpan { offset, length }
}

// --- Synthetic ----------------------------------------------------------------

#[test]
fn accept_t344_a_pcm_member_fills_its_descriptor_from_the_fmt_chunk() {
    let bytes = archive(&[(b"gun.wav", pcm_member())]);
    with_sound_archive(&bytes, |sound| {
        let entry = sound.entry(0).expect("readable");
        let header = entry.wave().expect("the member is a WAVE file");
        assert_eq!(header.format_tag(), WAVE_FORMAT_PCM);
        assert_eq!(header.format_name(), Some("pcm"));
        assert_eq!(header.channels(), 1);
        assert_eq!(header.rate_hz(), 22_050);
        assert_eq!(header.avg_bytes_per_sec(), 44_100);
        assert_eq!(header.block_align(), 2);
        assert_eq!(header.bits_per_sample(), 16);
        // `RIFF` header (12) + `fmt ` header (8): the payload is at 20.
        assert_eq!(header.fmt_span(), span(20, 16));
        assert_eq!(header.data_span(), span(44, 8));
        let data = header.data_span();
        assert_eq!(
            &entry.content()[data.offset as usize..][..data.length as usize],
            &[1, 0, 2, 0, 3, 0, 4, 0]
        );
        assert_eq!(header.cue_points(), 0);
        assert_eq!(header.smpl_span(), None);
        assert_eq!(header.other_chunks(), 0);

        let descriptor = entry.descriptor();
        assert_eq!(descriptor.format(), SoundField::Known("pcm"));
        assert_eq!(descriptor.format_tag(), SoundField::Known(1));
        assert_eq!(descriptor.channels(), SoundField::Known(1));
        assert_eq!(descriptor.rate_hz(), SoundField::Known(22_050));
        assert_eq!(descriptor.bits_per_sample(), SoundField::Known(16));
        assert_eq!(descriptor.block_align(), SoundField::Known(2));
        assert_eq!(descriptor.cue_points(), SoundField::Known(0));
        // No `smpl` chunk: no loop is declared, and none is invented.
        assert_eq!(
            descriptor.loop_points(),
            SoundField::Unknown {
                reason: NO_LOOP_CHUNK_REASON
            }
        );

        // Reading the header decodes nothing: the entry stays unsupported.
        let unsupported = sound.unsupported_records();
        assert_eq!(unsupported.len(), 1);
        assert_eq!(unsupported[0].reason(), SAMPLES_NOT_DECODED_REASON);
        assert_eq!(sound.wave_failures().count(), 0);
        assert_eq!(sound.status(), ContainerStatus::Clean);
    });
}

#[test]
fn accept_t344_adpcm_tags_are_named_and_unlisted_tags_stay_unknown() {
    // Extended `fmt ` payloads, as ADPCM members carry them: the 16 common
    // bytes plus format-specific bytes this reader keeps inside `fmt_span`.
    let ms = wave(&[
        chunk(
            b"fmt ",
            &fmt(WAVE_FORMAT_MS_ADPCM, 2, 22_050, 1024, 4, &[32, 0, 0xF4, 1]),
        ),
        chunk(b"data", &[0; 6]),
    ]);
    let ima = wave(&[
        chunk(
            b"fmt ",
            &fmt(WAVE_FORMAT_IMA_ADPCM, 1, 11_025, 256, 4, &[2, 0, 0xF9, 1]),
        ),
        chunk(b"data", &[0; 4]),
    ]);
    let unlisted = wave(&[
        chunk(b"fmt ", &fmt(0x0055, 1, 44_100, 1, 0, &[])),
        chunk(b"data", &[0; 2]),
    ]);
    let bytes = archive(&[(b"ms.wav", ms), (b"ima.wav", ima), (b"mp3.wav", unlisted)]);
    with_sound_archive(&bytes, |sound| {
        let descriptors: Vec<_> = sound.entries().map(|entry| entry.descriptor()).collect();
        assert_eq!(descriptors[0].format(), SoundField::Known("ms_adpcm"));
        assert_eq!(descriptors[0].channels(), SoundField::Known(2));
        assert_eq!(descriptors[0].block_align(), SoundField::Known(1024));
        assert_eq!(descriptors[0].bits_per_sample(), SoundField::Known(4));
        assert_eq!(
            sound.entry(0).expect("ms").wave().expect("wave").fmt_span(),
            span(20, 20)
        );
        assert_eq!(descriptors[1].format(), SoundField::Known("ima_adpcm"));
        assert_eq!(descriptors[1].rate_hz(), SoundField::Known(11_025));
        // A tag outside the named set keeps its number; its name is unknown.
        assert_eq!(
            descriptors[2].format(),
            SoundField::Unknown {
                reason: UNNAMED_FORMAT_REASON
            }
        );
        assert_eq!(descriptors[2].format_tag(), SoundField::Known(0x0055));
        assert_eq!(descriptors[2].rate_hz(), SoundField::Known(44_100));
    });
}

#[test]
fn accept_t344_cue_points_are_counted_and_never_read_as_loops() {
    let cued = wave(&[
        chunk(b"fmt ", &fmt(WAVE_FORMAT_PCM, 1, 11_025, 1, 8, &[])),
        chunk(b"cue ", &cue(2)),
        // An odd-sized chunk this reader skips, with its pad byte.
        chunk(b"note", b"odd"),
        chunk(b"data", &[0x80; 5]),
    ]);
    let sampled = wave(&[
        chunk(b"fmt ", &fmt(WAVE_FORMAT_PCM, 1, 11_025, 1, 8, &[])),
        chunk(b"smpl", &[0; 36]),
        chunk(b"data", &[0x80; 2]),
    ]);
    let header = read_wave_header(&cued).expect("cued member reads");
    assert_eq!(header.cue_points(), 2);
    assert_eq!(header.other_chunks(), 1);
    // `fmt ` 8+16, `cue ` 8+52, `note` 8+3+pad: `data` payload at 12+96+8.
    assert_eq!(header.data_span(), span(116, 5));

    let bytes = archive(&[(b"cued.wav", cued), (b"sampled.wav", sampled)]);
    with_sound_archive(&bytes, |sound| {
        let cued = sound.entry(0).expect("cued").descriptor();
        assert_eq!(cued.cue_points(), SoundField::Known(2));
        assert_eq!(
            cued.loop_points(),
            SoundField::Unknown {
                reason: NO_LOOP_CHUNK_REASON
            }
        );
        let sampled = sound.entry(1).expect("sampled");
        assert_eq!(
            sampled.wave().expect("sampled member reads").smpl_span(),
            Some(span(44, 36))
        );
        assert_eq!(
            sampled.descriptor().loop_points(),
            SoundField::Unknown {
                reason: SMPL_NOT_READ_REASON
            }
        );
    });
}

#[test]
fn accept_t344_non_wave_members_fail_clearly_beside_their_siblings() {
    let bytes = archive(&[
        (b"good.wav", pcm_member()),
        (b"text.wav", b"not a sound file".to_vec()),
        (b"avi.wav", b"RIFF\x04\x00\x00\x00AVI ".to_vec()),
        (b"tiny.wav", b"RIFF".to_vec()),
        (b"also-good.wav", pcm_member()),
    ]);
    with_sound_archive(&bytes, |sound| {
        // Every member is in bounds, so the bounds status stays clean…
        assert_eq!(sound.status(), ContainerStatus::Clean);
        assert_eq!(sound.len(), 5);
        // …and the three that are not WAVE files are named with their error.
        let failures: Vec<(usize, WaveError)> = sound
            .wave_failures()
            .map(|(entry, error)| (entry.index(), error))
            .collect();
        assert_eq!(
            failures,
            [
                (1, WaveError::NotRiff { found: *b"not " }),
                (2, WaveError::NotWave { found: *b"AVI " }),
                (3, WaveError::TooShort { member_len: 4 }),
            ]
        );
        for (index, error) in &failures {
            let descriptor = sound.entry(*index).expect("readable").descriptor();
            assert_eq!(
                descriptor.rate_hz(),
                SoundField::Unknown {
                    reason: error.reason()
                }
            );
            assert_eq!(descriptor.format().reason(), Some(error.reason()));
        }
        assert_eq!(failures[0].1.code(), "not_riff");
        assert_eq!(
            failures[1].1.to_string(),
            "RIFF form `AVI `, not `WAVE`",
            "the error names what it found"
        );
        // The siblings still read.
        for index in [0, 4] {
            let descriptor = sound.entry(index).expect("readable").descriptor();
            assert_eq!(descriptor.rate_hz(), SoundField::Known(22_050));
        }
        let reasons: Vec<&str> = sound
            .unsupported_records()
            .iter()
            .map(|record| record.reason())
            .collect();
        assert_eq!(
            reasons,
            [
                SAMPLES_NOT_DECODED_REASON,
                WaveError::NotRiff { found: *b"not " }.reason(),
                WaveError::NotWave { found: *b"AVI " }.reason(),
                WaveError::TooShort { member_len: 4 }.reason(),
                SAMPLES_NOT_DECODED_REASON,
            ]
        );
    });
}

#[test]
fn accept_t344_malformed_chunk_lists_are_refused_with_their_offsets() {
    let fmt_chunk = chunk(b"fmt ", &fmt(WAVE_FORMAT_PCM, 1, 8_000, 1, 8, &[]));
    let data_chunk = chunk(b"data", &[0; 2]);

    // A RIFF size that does not match the member length.
    let mut resized = wave(&[fmt_chunk.clone(), data_chunk.clone()]);
    resized[4] += 2;
    assert_eq!(
        read_wave_header(&resized),
        Err(WaveError::RiffSizeMismatch {
            declared: 40,
            member_len: 46
        })
    );

    // A chunk claiming more bytes than remain, up to u32::MAX.
    for size in [3u32, u32::MAX] {
        let mut member = wave(&[fmt_chunk.clone(), data_chunk.clone()]);
        member[40..44].copy_from_slice(&size.to_le_bytes());
        assert_eq!(
            read_wave_header(&member),
            Err(WaveError::ChunkOutOfBounds {
                id: *b"data",
                offset: 36,
                size,
                available: 2
            }),
            "{size}"
        );
    }

    // Three stray bytes after the last chunk: too few for a chunk header.
    let mut stray = wave(&[fmt_chunk.clone(), data_chunk.clone()]);
    stray.extend_from_slice(b"xyz");
    stray[4] += 3;
    assert_eq!(
        read_wave_header(&stray),
        Err(WaveError::TruncatedChunkHeader {
            offset: 46,
            remaining: 3
        })
    );

    let cases: [(Vec<Vec<u8>>, WaveError); 7] = [
        (
            vec![data_chunk.clone()],
            WaveError::DataBeforeFmt { offset: 12 },
        ),
        (vec![fmt_chunk.clone()], WaveError::MissingData),
        (vec![chunk(b"LIST", b"info")], WaveError::MissingFmt),
        (
            vec![fmt_chunk.clone(), fmt_chunk.clone(), data_chunk.clone()],
            WaveError::DuplicateChunk {
                id: *b"fmt ",
                offset: 36,
            },
        ),
        (
            vec![chunk(b"fmt ", &[1, 0, 1, 0]), data_chunk.clone()],
            WaveError::FmtTooShort {
                offset: 12,
                size: 4,
            },
        ),
        (
            vec![
                fmt_chunk.clone(),
                chunk(b"cue ", &[1, 0]),
                data_chunk.clone(),
            ],
            WaveError::CueTooShort {
                offset: 36,
                size: 2,
                points: None,
            },
        ),
        (
            // Three cue points declared, one present.
            vec![
                fmt_chunk.clone(),
                chunk(b"cue ", &{
                    let mut payload = cue(1);
                    payload[0] = 3;
                    payload
                }),
                data_chunk.clone(),
            ],
            WaveError::CueTooShort {
                offset: 36,
                size: 28,
                points: Some(3),
            },
        ),
    ];
    for (chunks, expected) in cases {
        let member = wave(&chunks);
        assert_eq!(read_wave_header(&member), Err(expected), "{expected}");
    }
}

// --- Retail -------------------------------------------------------------------

/// One `fmt ` shape: tag, channels, rate, bits per sample, block align.
type Shape = (u16, u16, u32, u16, u16);

/// What production code established about one retail sound archive.
#[derive(Debug, Default)]
struct RetailSounds {
    spelling: String,
    members: usize,
    wave_failures: usize,
    shapes: BTreeMap<Shape, usize>,
    cued_members: usize,
    cue_points: u64,
    smpl_members: usize,
    other_chunks: u64,
    unknown_formats: usize,
    known_loops: usize,
    independent_mismatches: usize,
}

/// `fmt ` fields read independently of the production parser: the retail
/// members start with `fmt ` at offset 12 (checked here).
fn independent_shape(content: &[u8]) -> Option<Shape> {
    if content.get(12..16)? != b"fmt " {
        return None;
    }
    let word = |at: usize| u16::from_le_bytes([content[at], content[at + 1]]);
    let rate = u32::from_le_bytes(content.get(24..28)?.try_into().ok()?);
    Some((word(20), word(22), rate, word(34), word(32)))
}

fn retail_sounds() -> Vec<RetailSounds> {
    let game_dir = game_dir();
    let mut found = Vec::new();
    for (spelling, host) in retail_zbd_files(&game_dir) {
        let relative = path(&spelling);
        let bytes = fs::read(&host).unwrap_or_else(|error| panic!("read {spelling}: {error}"));
        let probe = &bytes[..bytes.len().min(64)];
        let decided = dispatch(ZbdProbe::new(&spelling, &relative, probe))
            .unwrap_or_else(|error| panic!("{spelling} must dispatch: {error}"));
        if decided.family() != ZbdFamily::Sound {
            continue;
        }
        let mut context = ParseContext::with_defaults(spelling.clone());
        let index = read_version_one_index(&mut context, decided, &bytes)
            .unwrap_or_else(|error| panic!("{spelling}: {error}"));
        let table = index.member_table();
        let sound = read_sound_archive(&mut context, &table, index.data())
            .unwrap_or_else(|error| panic!("{spelling}: {error}"));
        assert_eq!(sound.failures(), 0, "{spelling}: members in bounds");

        let mut report = RetailSounds {
            members: sound.len(),
            wave_failures: sound.wave_failures().count(),
            ..RetailSounds::default()
        };
        for entry in sound.entries() {
            let Ok(header) = entry.wave() else { continue };
            let descriptor = entry.descriptor();
            let shape = (
                header.format_tag(),
                header.channels(),
                header.rate_hz(),
                header.bits_per_sample(),
                header.block_align(),
            );
            *report.shapes.entry(shape).or_default() += 1;
            if independent_shape(entry.content()) != Some(shape) {
                report.independent_mismatches += 1;
            }
            if header.cue_points() > 0 {
                report.cued_members += 1;
            }
            report.cue_points += u64::from(header.cue_points());
            report.smpl_members += usize::from(header.smpl_span().is_some());
            report.other_chunks += u64::from(header.other_chunks());
            report.unknown_formats += usize::from(!descriptor.format().is_known());
            report.known_loops += usize::from(descriptor.loop_points().is_known());
            assert_eq!(descriptor.rate_hz(), SoundField::Known(header.rate_hz()));
            assert_eq!(descriptor.channels(), SoundField::Known(header.channels()));
        }
        report.spelling = spelling;
        found.push(report);
    }
    found
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t344_retail_every_sound_member_has_a_wave_descriptor() {
    let sounds = retail_sounds();
    assert_eq!(sounds.len(), 2, "the two sound archives task #340 counted");
    for sound in &sounds {
        let spelling = &sound.spelling;
        assert!(sound.members > 0, "{spelling}");
        assert_eq!(sound.wave_failures, 0, "{spelling}: every member is WAVE");
        assert_eq!(
            sound.independent_mismatches, 0,
            "{spelling}: production fmt fields match an independent read"
        );
        // Observed: only the three named tags, so every format is named.
        assert_eq!(sound.unknown_formats, 0, "{spelling}");
        for (tag, ..) in sound.shapes.keys() {
            assert!(
                [WAVE_FORMAT_PCM, WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_IMA_ADPCM].contains(tag),
                "{spelling}: tag {tag:#06x}"
            );
        }
        // Observed: no `smpl` chunk anywhere, so no loop point is known.
        assert_eq!(sound.smpl_members, 0, "{spelling}");
        assert_eq!(sound.known_loops, 0, "{spelling}");
        assert_eq!(sound.other_chunks, 0, "{spelling}: only fmt, cue and data");
        assert_eq!(
            sound.cued_members, 35,
            "{spelling}: members with cue points"
        );
        println!(
            "{spelling}: {} members, shapes {:?}, {} cued members, {} cue points",
            sound.members, sound.shapes, sound.cued_members, sound.cue_points
        );
    }
}

// --- Evidence harness ---------------------------------------------------------

/// Evidence-report harness for task #344 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. Run from the workspace root:
///
/// 1. ```sh
///    mkdir -p private/evidence/T344
///    cargo test --workspace --locked -- accept_t344_ --include-ignored \
///      2>&1 | tee private/evidence/T344/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T344 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t344_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_formats --test zbd -- evidence_report_t344 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T344/acceptance.json \
///      --artifact-root private/evidence/T344 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T344.json`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t344_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = game_dir();

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
    let suite = parse_suite(&log, "accept_t344_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_t344_` tests were recorded in {}",
        log_path.display()
    );
    let retail = "accept_t344_retail_every_sound_member_has_a_wave_descriptor";
    let status = suite
        .assertions
        .iter()
        .find(|(name, _)| short_name(name) == retail)
        .map(|(_, status)| *status)
        .unwrap_or_else(|| panic!("{retail} did not run: run step 1 with --include-ignored"));
    assert_eq!(status, "pass", "{retail} must pass");

    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery reads the original installation");
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let content_sha256 = cs_assets::install::content_fingerprint(&found.manifest).to_hex();
    let hashes: BTreeMap<String, String> = found
        .manifest
        .files
        .iter()
        .map(|record| {
            (
                record.relative_spelling.as_str().to_owned(),
                record.sha256.to_hex(),
            )
        })
        .collect();

    // Per-archive header facts: counts and fmt shapes, no original bytes.
    let rows: Vec<String> = retail_sounds()
        .iter()
        .map(|sound| {
            let sha = hashes
                .get(&sound.spelling)
                .map_or_else(|| "null".to_owned(), |hash| jstr(hash));
            let shapes: Vec<String> = sound
                .shapes
                .iter()
                .map(|((tag, channels, rate, bits, align), members)| {
                    format!(
                        "{{\"format_tag\": {tag}, \"format\": {}, \"channels\": {channels}, \
                         \"rate_hz\": {rate}, \"bits_per_sample\": {bits}, \
                         \"block_align\": {align}, \"members\": {members}}}",
                        cs_formats::zbd::format_name(*tag).map_or_else(|| "null".to_owned(), jstr)
                    )
                })
                .collect();
            format!(
                "{{\"spelling\": {}, \"sha256\": {sha}, \"members\": {}, \"wave_failures\": {}, \
                 \"independent_fmt_mismatches\": {}, \"cued_members\": {}, \"cue_points\": {}, \
                 \"smpl_members\": {}, \"other_chunks\": {}, \"unnamed_formats\": {}, \
                 \"known_loop_points\": {}, \"fmt_shapes\": [{}]}}",
                jstr(&sound.spelling),
                sound.members,
                sound.wave_failures,
                sound.independent_mismatches,
                sound.cued_members,
                sound.cue_points,
                sound.smpl_members,
                sound.other_chunks,
                sound.unknown_formats,
                sound.known_loops,
                shapes.join(", "),
            )
        })
        .collect();
    let headers_path = evidence_dir.join("zbd-sound-wave-headers.json");
    fs::write(
        &headers_path,
        format!(
            "{{\n \"task_id\": \"T344\",\n \"candidate_tree\": {},\n \"install_sha256\": {},\n \
             \"archive_count\": {},\n \"archives\": [\n  {}\n ]\n}}\n",
            jstr(&candidate_tree),
            jstr(&install_sha256),
            rows.len(),
            rows.join(",\n  ")
        ),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", headers_path.display()));

    let artifacts = [artifact(&log_path, "log"), artifact(&headers_path, "json")];
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T344\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        jstr(&command_output("rustc", &["--version"])),
        jstr(&locked_version("bevy")),
        jstr(&locked_version("avian3d")),
        jstr(&iso_utc_now()),
        argv.iter()
            .map(|arg| jstr(arg))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.passed + suite.failed + suite.ignored,
        suite.passed + suite.failed,
        suite.passed,
        suite.failed,
        suite.ignored,
        suite
            .assertions
            .iter()
            .map(|(name, status)| format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(short_name(name))
            ))
            .collect::<Vec<_>>()
            .join(", "),
        artifacts
            .iter()
            .map(|(name, digest, kind)| format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(
            "claude-1 (implementing agent, self-check; the Rally reviewer regenerates this \
             report on the rebased commit)"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production index reader, sound reader and WAVE header reader over every retail \
             sound archive member (zbd-sound-wave-headers.json, with an independent read of \
             each fmt chunk), rustc and Cargo.lock; validated with tools/validate_evidence.py \
             --require-pass. Loop points stay unknown for every member (no retail member \
             carries a `smpl` chunk) and `cue ` points are counted, not interpreted, as the \
             task requires; both are recorded as unknown in the task #344 findings"
        ),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must not validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}
