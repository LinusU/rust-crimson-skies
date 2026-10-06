//! Acceptance for task #692: what the 76 unexplained bytes of a version-one
//! ZBD index entry are (`docs/findings/2026-10-06-t692-zbd-version-one-index-entry-tail.md`).
//!
//! Task #341 measured the region on the 62 retail `zrdr.zbd` archives; this
//! task measured all 64 archives indexed by a version-one trailer (the 62
//! reader archives plus `ZBD/soundsh.zbd` and `ZBD/soundsl.zbd`, 6,334
//! entries) and audited the decrypted executable for readers of the fields.
//! The region always splits `u32 word` / `u8 name_again[64]` / `u64 stamp`;
//! the archiver documentation names the slots `flags`, `comment` and `time`,
//! the original engine never reads any of them, and the meaning of the word
//! stays unknown. `UnexplainedBytes` therefore keeps the verbatim bytes and
//! its `unknown` label while [`UnexplainedBytes::word`],
//! [`UnexplainedBytes::name_again`] and [`UnexplainedBytes::stamp`] expose
//! the measured subfields raw.
//!
//! The synthetic `accept_t692_` test authors every byte it reads. The retail
//! tests read the installation at `$CS_GAME_DIR` (never write it) and fail
//! loudly without it. `evidence_report_t692_writes_the_acceptance_report` is
//! the evidence harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance
//! test.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use cs_formats::ParseContext;
use cs_formats::zbd::{
    INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES, INDEX_UNEXPLAINED_NAME_BYTES,
    TRAILER_VERSION_ONE, WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_PCM, ZbdFamily,
    ZbdProbe, dispatch, read_sound_archive, read_version_one_index,
};
use cs_types::evidence::{ClaimStatus, SourceSpan};

use super::t340::{
    artifact, command_output, env_var, game_dir, git, iso_utc_now, jstr, locked_version,
    parse_suite, path, retail_zbd_files, short_name, workspace_path,
};

/// Provenance label carried by every synthetic result.
const CONTAINER: &str = "synthetic/t692_archive.zbd";

/// FILETIME ticks of 2000-08-26T08:00:54Z .. 2000-08-26T08:06:58Z: the
/// measured stamp window across the 62 reader archives (task #692 finding).
const READER_STAMP_MIN: u64 = 126_117_504_540_000_000;
const READER_STAMP_MAX: u64 = 126_117_508_180_000_000;
/// The one stamp `soundsh.zbd` stores on every entry (2000-08-26T08:57:14Z).
const SOUNDSH_STAMP: u64 = 126_117_538_340_000_000;
/// The one stamp `soundsl.zbd` stores on every entry (2000-08-26T09:01:20Z).
const SOUNDSL_STAMP: u64 = 126_117_540_800_000_000;

/// The word every reader entry carries, and two `soundsl` PCM members.
const WORD_TWO: u32 = 2;
/// The word `soundsh` and most of `soundsl` carry.
const WORD_SOUND: u32 = 62;
/// Measured bounds of `soundsl`'s unexplained increasing word series.
const SERIES_MIN: u32 = 29_688_754;
const SERIES_MAX: u32 = 30_092_802;
const SERIES_COUNT: usize = 555;

/// An authored version-one index entry: `start`, `length`, a NUL-padded
/// 64-byte name and 76 authored tail bytes.
fn authored_entry(start: u32, length: u32, name: &[u8], tail: &[u8; 76]) -> Vec<u8> {
    let mut entry = Vec::with_capacity(INDEX_ENTRY_BYTES as usize);
    entry.extend_from_slice(&start.to_le_bytes());
    entry.extend_from_slice(&length.to_le_bytes());
    let mut field = [0u8; INDEX_NAME_BYTES];
    field[..name.len()].copy_from_slice(name);
    entry.extend_from_slice(&field);
    entry.extend_from_slice(tail);
    entry
}

/// `data`, then the entries, then the version-one trailer.
fn authored_archive(data: &[u8], entries: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = data.to_vec();
    for entry in entries {
        bytes.extend_from_slice(entry);
    }
    bytes.extend_from_slice(&TRAILER_VERSION_ONE.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes
}

/// The tail the archiver documentation describes (`flags`, `comment`,
/// `time`) with authored values: word `2`, a copy of `name`, stamp `stamp`.
fn authored_tail(word: u32, comment: &[u8; 64], stamp: u64) -> [u8; 76] {
    let mut tail = [0u8; 76];
    tail[..4].copy_from_slice(&word.to_le_bytes());
    tail[4..68].copy_from_slice(comment);
    tail[68..].copy_from_slice(&stamp.to_le_bytes());
    tail
}

// --- Synthetic ---------------------------------------------------------------

/// The measured subfields read where the archive puts them, on authored
/// bytes, through the production parser.
#[test]
fn accept_t692_tail_accessors_read_the_measured_subfields() {
    let data = b"one member, sixteen b".to_vec();
    let name = b"one.wav";
    let mut name_field = [0u8; 64];
    name_field[..name.len()].copy_from_slice(name);
    // A comment byte pattern that is *not* the name, so a swapped field
    // could not pass: the accessors must read where the archive puts them.
    let mut comment = [0u8; 64];
    comment[..9].copy_from_slice(b"a comment");
    let stamp = 0x01c7_89ab_cdef_0123u64;
    let entries = vec![authored_entry(
        0,
        data.len() as u32,
        name,
        &authored_tail(0xA5A5_0002, &comment, stamp),
    )];
    let bytes = authored_archive(&data, &entries);
    let spelling = path("ZBD/soundsl.zbd");
    let probe = &bytes[..bytes.len().min(64)];
    let decided = dispatch(ZbdProbe::new(CONTAINER, &spelling, probe)).expect("sound dispatches");
    let mut context = ParseContext::with_defaults(CONTAINER);
    let index = read_version_one_index(&mut context, decided, &bytes).expect("the index reads");
    assert_eq!(index.len(), 1);

    let entry = index.entry(0).expect("one entry");
    let unexplained = entry.unexplained();
    // The whole region is still verbatim, located and labelled unknown.
    let record = data.len() as u64;
    assert_eq!(
        unexplained.span(),
        SourceSpan {
            offset: record + 72,
            length: INDEX_UNEXPLAINED_BYTES as u64
        }
    );
    assert_eq!(unexplained.evidence(), ClaimStatus::Unknown);
    // …and the three measured subfields read raw from their offsets.
    assert_eq!(unexplained.word(), 0xA5A5_0002);
    assert_eq!(unexplained.name_again(), &comment[..]);
    assert_eq!(unexplained.name_again().len(), INDEX_UNEXPLAINED_NAME_BYTES);
    assert_eq!(unexplained.stamp(), stamp);
    // The subfields partition the region: verbatim bytes still hold all 76.
    assert_eq!(unexplained.bytes().len(), INDEX_UNEXPLAINED_BYTES);
    let mut reassembled = unexplained.word().to_le_bytes().to_vec();
    reassembled.extend_from_slice(unexplained.name_again());
    reassembled.extend_from_slice(&unexplained.stamp().to_le_bytes());
    assert_eq!(reassembled, unexplained.bytes());
}

// --- Retail -------------------------------------------------------------------

/// One retail archive's index-tail measurements, all through production
/// code (`read_version_one_index` and, for sound, `read_sound_archive` +
/// `read_wave_header` inside `SoundEntry::wave`).
#[derive(Debug)]
struct TailMeasure {
    spelling: String,
    family: ZbdFamily,
    size_bytes: u64,
    members: usize,
    /// `word()` -> count, in index order of first appearance.
    word_counts: BTreeMap<u32, usize>,
    /// `word()` for every entry, in index order (needed for the series).
    word_order: Vec<u32>,
    /// Names of entries whose word is [`WORD_TWO`].
    word_two_names: Vec<String>,
    /// `word() == 0` count.
    zero_words: usize,
    /// `name_again() != name_field()` count.
    name_again_mismatches: usize,
    /// `stamp() == 0` count.
    zero_stamps: usize,
    stamp_min: u64,
    stamp_max: u64,
    stamp_distinct: usize,
    /// `(entry index, WAVE format tag)` for every readable sound member.
    codecs: Vec<(usize, u16)>,
}

/// Measures every retail archive that production dispatch routes to a
/// version-one trailer index (the sound and reader families).
fn measure_tails() -> Vec<TailMeasure> {
    let game_dir = game_dir();
    let mut found = Vec::new();
    for (spelling, host) in retail_zbd_files(&game_dir) {
        let relative = path(&spelling);
        let bytes = fs::read(&host).unwrap_or_else(|error| panic!("read {spelling}: {error}"));
        let probe = &bytes[..bytes.len().min(64)];
        let decided = dispatch(ZbdProbe::new(&spelling, &relative, probe))
            .unwrap_or_else(|error| panic!("{spelling} must dispatch: {error}"));
        if !matches!(decided.family(), ZbdFamily::Sound | ZbdFamily::Reader) {
            continue;
        }
        let mut context = ParseContext::with_defaults(spelling.clone());
        let index = read_version_one_index(&mut context, decided, &bytes)
            .unwrap_or_else(|error| panic!("{spelling}: {error}"));
        assert_eq!(index.len() as u64 as usize, index.entries().len());

        let mut measure = TailMeasure {
            family: decided.family(),
            size_bytes: bytes.len() as u64,
            members: index.len(),
            word_counts: BTreeMap::new(),
            word_order: Vec::new(),
            word_two_names: Vec::new(),
            zero_words: 0,
            name_again_mismatches: 0,
            zero_stamps: 0,
            stamp_min: u64::MAX,
            stamp_max: 0,
            stamp_distinct: 0,
            codecs: Vec::new(),
            spelling: spelling.clone(),
        };
        let mut stamps = BTreeSet::new();
        for entry in index.entries() {
            let unexplained = entry.unexplained();
            let (word, stamp) = (unexplained.word(), unexplained.stamp());
            *measure.word_counts.entry(word).or_default() += 1;
            measure.word_order.push(word);
            if word == 0 {
                measure.zero_words += 1;
            }
            if word == WORD_TWO {
                measure
                    .word_two_names
                    .push(String::from_utf8_lossy(entry.name()).into_owned());
            }
            if unexplained.name_again() != entry.name_field() {
                measure.name_again_mismatches += 1;
            }
            if stamp == 0 {
                measure.zero_stamps += 1;
            }
            stamps.insert(stamp);
            measure.stamp_min = measure.stamp_min.min(stamp);
            measure.stamp_max = measure.stamp_max.max(stamp);
        }
        measure.stamp_distinct = stamps.len();
        if decided.family() == ZbdFamily::Sound {
            let table = index.member_table();
            let sound = read_sound_archive(&mut context, &table, index.data())
                .unwrap_or_else(|error| panic!("{spelling}: {error}"));
            assert_eq!(sound.failures(), 0, "{spelling}: members inside the data");
            for entry in sound.entries() {
                let tag = entry
                    .wave()
                    .unwrap_or_else(|error| panic!("{spelling} member {}: {error}", entry.index()))
                    .format_tag();
                measure.codecs.push((entry.index(), tag));
            }
        }
        found.push(measure);
    }
    found
}

/// The measured shape holds on every entry of every trailer-indexed
/// archive: the 64-byte field after the word copies the name field byte
/// for byte, and the trailing u64 is a nonzero FILETIME in the measured
/// build-session window.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t692_retail_every_index_entry_tail_has_the_measured_shape() {
    let measures = measure_tails();
    let readers = measures
        .iter()
        .filter(|measure| measure.family == ZbdFamily::Reader)
        .count();
    let sounds = measures
        .iter()
        .filter(|measure| measure.family == ZbdFamily::Sound)
        .count();
    assert_eq!(readers, 62, "reader archives");
    assert_eq!(sounds, 2, "sound archives");
    let members: usize = measures.iter().map(|measure| measure.members).sum();
    assert_eq!(members, 6_334, "every measured index entry");

    for measure in &measures {
        // The 64-byte slot after the word is the name field again, byte
        // for byte including the padding — on every entry of every family.
        assert_eq!(
            measure.name_again_mismatches, 0,
            "{}: every name_again copies the name field",
            measure.spelling
        );
        // The stamp is set everywhere and sits inside the measured
        // build-session window of 2000-08-26 (task #692 finding).
        assert_eq!(
            measure.zero_stamps, 0,
            "{}: every stamp set",
            measure.spelling
        );
        assert!(
            measure.stamp_min >= READER_STAMP_MIN && measure.stamp_max <= SOUNDSL_STAMP,
            "{}: stamps {} .. {} inside the measured window",
            measure.spelling,
            measure.stamp_min,
            measure.stamp_max
        );
        // The word is never zero anywhere — but what its values mean is
        // not established and the parser does not claim one.
        assert_eq!(
            measure.zero_words, 0,
            "{}: every word set",
            measure.spelling
        );
    }
    for measure in &measures {
        let (expected_min, expected_max) = match measure.family {
            ZbdFamily::Reader => (READER_STAMP_MIN, READER_STAMP_MAX),
            ZbdFamily::Sound => (SOUNDSH_STAMP, SOUNDSL_STAMP),
            other => unreachable!("measure_tails keeps sound and reader: {other:?}"),
        };
        assert!(
            measure.stamp_min >= expected_min && measure.stamp_max <= expected_max,
            "{}: family stamp window",
            measure.spelling
        );
    }
}

/// The word's measured distribution: `2` on all 1,293 reader entries, `62`
/// on every `soundsh` entry and 1,963 `soundsl` entries, `2` again on the
/// two `soundsl` PCM members, and a 555-entry strictly-increasing series —
/// all of them IMA-ADPCM members — whose semantics stay unestablished.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t692_retail_word_distribution_is_family_and_codec_shaped() {
    let measures = measure_tails();
    let mut soundsl = None;
    let mut soundsh = None;
    for measure in &measures {
        let basename = measure
            .spelling
            .rsplit('/')
            .next()
            .expect("a spelling has a basename")
            .to_ascii_lowercase();
        match measure.family {
            ZbdFamily::Reader => {
                assert_eq!(
                    measure.word_counts,
                    BTreeMap::from([(WORD_TWO, measure.members)]),
                    "{}: every reader word is 2",
                    measure.spelling
                );
            }
            ZbdFamily::Sound if basename == "soundsh.zbd" => {
                assert_eq!(
                    measure.word_counts,
                    BTreeMap::from([(WORD_SOUND, measure.members)]),
                    "soundsh: every word is 62"
                );
                assert_eq!(measure.stamp_min, SOUNDSH_STAMP);
                assert_eq!(measure.stamp_max, SOUNDSH_STAMP);
                soundsh = Some(measure);
            }
            ZbdFamily::Sound if basename == "soundsl.zbd" => {
                assert_eq!(measure.stamp_min, SOUNDSL_STAMP);
                assert_eq!(measure.stamp_max, SOUNDSL_STAMP);
                soundsl = Some(measure);
            }
            other => panic!("{}: unexpected {other:?}", measure.spelling),
        }
    }
    let soundsh = soundsh.expect("soundsh.zbd was measured");
    let soundsl = soundsl.expect("soundsl.zbd was measured");

    // soundsl: 1,963 entries at 62, two at 2 (both PCM, both named), and a
    // 555-entry series of increasing values, all ≡ 2 (mod 4), that matches
    // nothing else measured — recorded as unknown, not as offsets.
    assert_eq!(soundsl.word_counts.get(&WORD_SOUND), Some(&1_963));
    assert_eq!(soundsl.word_counts.get(&WORD_TWO), Some(&2));
    assert_eq!(
        soundsl.word_two_names,
        ["train2.wav", "pilot_eject1.wav"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    );
    let series: Vec<u32> = soundsl
        .word_order
        .iter()
        .copied()
        .filter(|word| !matches!(*word, WORD_TWO | WORD_SOUND))
        .collect();
    assert_eq!(series.len(), SERIES_COUNT);
    assert_eq!(soundsl.word_counts.len(), 2 + SERIES_COUNT);
    assert_eq!(*series.first().expect("series"), SERIES_MIN);
    assert_eq!(*series.last().expect("series"), SERIES_MAX);
    for pair in series.windows(2) {
        assert!(pair[0] < pair[1], "soundsl series increases: {pair:?}");
    }
    for word in &series {
        assert_eq!(word % 4, 2, "soundsl series word {word} ≡ 2 (mod 4)");
        assert!(
            (SERIES_MIN..=SERIES_MAX).contains(word),
            "soundsl series word {word} inside the measured bounds"
        );
    }

    // The codec correlation the finding records: every series member is
    // IMA ADPCM and nothing outside the series is — the word tracks the
    // member's storage, but no semantic for it is established.
    let tag_at: BTreeMap<usize, u16> = soundsl.codecs.iter().copied().collect();
    for (position, word) in soundsl.word_order.iter().enumerate() {
        let tag = *tag_at
            .get(&position)
            .unwrap_or_else(|| panic!("soundsl member {position} was read"));
        match *word {
            WORD_TWO => assert_eq!(tag, WAVE_FORMAT_PCM, "soundsl word-2 member {position}"),
            WORD_SOUND => assert!(
                matches!(tag, WAVE_FORMAT_MS_ADPCM | WAVE_FORMAT_PCM),
                "soundsl word-62 member {position}: tag {tag:#x}"
            ),
            _ => assert_eq!(
                tag, WAVE_FORMAT_IMA_ADPCM,
                "soundsl series member {position}: tag {tag:#x}"
            ),
        }
    }
    for (_, tag) in &soundsh.codecs {
        assert!(
            matches!(*tag, WAVE_FORMAT_MS_ADPCM | WAVE_FORMAT_PCM),
            "soundsh member: tag {tag:#x}"
        );
    }
}

// --- Evidence harness ---------------------------------------------------------

/// Evidence-report harness for task #692 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. Run from the workspace root:
///
/// 1. ```sh
///    mkdir -p private/evidence/T692
///    cargo test --workspace --locked -- accept_t692_ --include-ignored \
///      2>&1 | tee private/evidence/T692/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T692 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t692_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_formats --test zbd -- evidence_report_t692 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T692/acceptance.json \
///      --artifact-root private/evidence/T692
///    ```
///    The `unknowns` array is deliberately non-empty — the word's semantics
///    and the `soundsl` series stay unestablished — so validate *without*
///    `--require-pass`: that flag rejects reports with unresolved issues.
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T692.json`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t692_writes_the_acceptance_report() {
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
    let suite = parse_suite(&log, "accept_t692_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_t692_` tests were recorded in {}",
        log_path.display()
    );
    for retail in [
        "accept_t692_retail_every_index_entry_tail_has_the_measured_shape",
        "accept_t692_retail_word_distribution_is_family_and_codec_shaped",
    ] {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| short_name(name) == retail)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail} did not run: run step 1 with --include-ignored"));
        assert_eq!(status, "pass", "{retail} must pass");
    }

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

    // Per-archive tail facts: spellings, sizes, counts, word histograms,
    // stamp bounds — never archive bytes.
    let rows: Vec<String> = measure_tails()
        .iter()
        .map(|measure| {
            let sha = hashes
                .get(&measure.spelling)
                .map_or_else(|| "null".to_owned(), |hash| jstr(hash));
            let histogram = measure
                .word_counts
                .iter()
                .map(|(word, count)| format!("[{word}, {count}]"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{{\"spelling\": {}, \"sha256\": {sha}, \"family\": {}, \"size_bytes\": {}, \
                 \"members\": {}, \"word_histogram\": [{}], \"zero_words\": {}, \
                 \"name_again_mismatches\": {}, \"zero_stamps\": {}, \"stamp_min\": {}, \
                 \"stamp_max\": {}, \"stamp_distinct\": {}}}",
                jstr(&measure.spelling),
                jstr(measure.family.as_str()),
                measure.size_bytes,
                measure.members,
                histogram,
                measure.zero_words,
                measure.name_again_mismatches,
                measure.zero_stamps,
                measure.stamp_min,
                measure.stamp_max,
                measure.stamp_distinct,
            )
        })
        .collect();
    let index_path = evidence_dir.join("zbd-index-tail.json");
    fs::write(
        &index_path,
        format!(
            "{{\n \"task_id\": \"T692\",\n \"candidate_tree\": {},\n \"install_sha256\": {},\n \
             \"archive_count\": {},\n \"archives\": [\n  {}\n ]\n}}\n",
            jstr(&candidate_tree),
            jstr(&install_sha256),
            rows.len(),
            rows.join(",\n  ")
        ),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", index_path.display()));

    let unknowns = [
        "What the u32 `flags` slot of a version-one index entry encodes is not established: \
         `2` on every reader entry, `62` on `soundsh` and most of `soundsl`, correlated with the \
         sound members' WAVE codecs, but the original engine never reads the field and no source \
         names the values. Affected content: every one of the 6,334 index entries in the 64 \
         retail archives indexed by a version-one trailer.",
        "The 555 increasing word values on `soundsl.zbd`'s IMA-ADPCM members (29,688,754 .. \
         30,092,802, all ≡ 2 mod 4) match no member offset, gap or record measured anywhere; the \
         archiver documentation's unzeroed-memory warning is the most consistent account, but the \
         values' meaning stays unknown. Affected content: those 555 soundsl members' flag words.",
        "Whether the `time` stamp is ever compared by the original engine is settled negatively \
         for the loose-override path (it compares the archive file's own last-write time), but \
         whether some unmeasured original path reads `flags`, `comment` or `time` cannot be \
         excluded by static analysis alone. Affected content: runtime semantics of all three \
         fields, which production therefore keeps labelled `unknown`.",
    ]
    .iter()
    .map(|unknown| jstr(unknown))
    .collect::<Vec<_>>()
    .join(", ");

    let artifacts = [artifact(&log_path, "log"), artifact(&index_path, "json")];
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T692\",\n\
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
         \x20\"unknowns\": [{}],\n\
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
        unknowns,
        jstr(
            "implementer: swe2-max-1 (Rally #692 implement claim); reviewer: recorded by the \
             reviewing agent — no review had happened when this report was generated",
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production index reader and sound reader plus read_wave_header over every retail \
             sound and reader archive (zbd-index-tail.json), rustc and Cargo.lock. The three \
             `unknowns` are the task's honest residue: the flag word's semantics, the soundsl \
             series' meaning, and any unmeasured original reader of the fields; so the report \
             must be validated WITHOUT --require-pass, which would reject unresolved issues. \
             The claim is `implemented`, never `verified_original` or `release_approved`",
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
