//! Acceptance for task #343: the version-one member index at the end of a
//! sound or reader archive, read by production code
//! (`cs_formats::zbd::read_version_one_index`) and fed to the F06-B readers
//! (`docs/findings/2026-09-28-t343-zbd-version-one-member-index.md`).
//!
//! The synthetic `accept_t343_*` tests author every byte here: the archives
//! are laid out as the pinned mech3ax v0.6.0 source describes (task #340
//! findings), with authored names and member bytes. The retail test reads the
//! installation at `$CS_GAME_DIR` (never writes it) and fails loudly without
//! it. `evidence_report_t343_writes_the_acceptance_report` is the evidence
//! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use cs_formats::zbd::{
    ContainerStatus, EntryAnomaly, INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_ROW_BYTES,
    INDEX_UNEXPLAINED_BYTES, IndexError, MEMBER_EXTENT_BYTES, MemberError, MemberStatus,
    TRAILER_BYTES, UNEXPLAINED_REASON, VersionOneIndex, ZbdDispatch, ZbdFamily, ZbdProbe, dispatch,
    read_reader_archive, read_sound_archive, read_version_one_index,
};
use cs_formats::{ParseContext, ParseErrorKind};
use cs_types::evidence::{ClaimStatus, SourceSpan};
use cs_types::install::RelativePath;

use super::t340::{
    artifact, command_output, env_var, game_dir, git, iso_utc_now, jstr, locked_version,
    parse_suite, path, retail_zbd_files, short_name, workspace_path,
};

/// Provenance label carried by every synthetic result and error.
const CONTAINER: &str = "synthetic/t343_archive.zbd";

/// One authored index entry.
#[derive(Clone)]
struct Entry {
    start: u32,
    length: u32,
    name: [u8; INDEX_NAME_BYTES],
    unexplained: [u8; INDEX_UNEXPLAINED_BYTES],
}

impl Entry {
    /// An entry with a NUL-padded `name` and a recognizable unexplained tail.
    fn new(start: u32, length: u32, name: &[u8], tag: u8) -> Self {
        let mut field = [0u8; INDEX_NAME_BYTES];
        field[..name.len()].copy_from_slice(name);
        let mut unexplained = [0u8; INDEX_UNEXPLAINED_BYTES];
        for (index, byte) in unexplained.iter_mut().enumerate() {
            *byte = tag.wrapping_add(index as u8);
        }
        Self {
            start,
            length,
            name: field,
            unexplained,
        }
    }
}

/// `data`, then the entries, then the version-one trailer.
fn archive(data: &[u8], entries: &[Entry], version: u32, count: u32) -> Vec<u8> {
    let mut bytes = data.to_vec();
    for entry in entries {
        bytes.extend_from_slice(&entry.start.to_le_bytes());
        bytes.extend_from_slice(&entry.length.to_le_bytes());
        bytes.extend_from_slice(&entry.name);
        bytes.extend_from_slice(&entry.unexplained);
    }
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes
}

/// An archive whose index lists every entry it carries.
fn indexed(data: &[u8], entries: &[Entry]) -> Vec<u8> {
    archive(data, entries, 1, entries.len() as u32)
}

/// Three authored WAVE-like members of 16, 12 and 20 bytes, back to back.
fn sound_data() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF\x08\x00\x00\x00WAVEone!");
    data.extend_from_slice(b"RIFF\x04\x00\x00\x00WAVE");
    data.extend_from_slice(b"RIFF\x0c\x00\x00\x00WAVEthree!!!");
    data
}

fn sound_entries() -> Vec<Entry> {
    vec![
        Entry::new(0, 16, b"engine.wav", 0x10),
        Entry::new(16, 12, b"gun.wav", 0x40),
        // A duplicate name, kept as its own member.
        Entry::new(28, 20, b"engine.wav", 0x70),
    ]
}

fn dispatch_at<'a>(spelling: &'a RelativePath, bytes: &'a [u8]) -> ZbdDispatch<'a> {
    let probe = &bytes[..bytes.len().min(64)];
    dispatch(ZbdProbe::new(CONTAINER, spelling, probe))
        .unwrap_or_else(|error| panic!("{spelling:?} dispatches: {error}"))
}

fn read_index<'a>(
    context: &mut ParseContext,
    spelling: &'a RelativePath,
    bytes: &'a [u8],
) -> Result<VersionOneIndex<'a>, IndexError> {
    read_version_one_index(context, dispatch_at(spelling, bytes), bytes)
}

fn span(offset: u64, length: u64) -> SourceSpan {
    SourceSpan { offset, length }
}

// --- Synthetic ----------------------------------------------------------------

#[test]
fn accept_t343_sound_members_come_from_the_trailer() {
    let data = sound_data();
    let entries = sound_entries();
    let bytes = indexed(&data, &entries);
    let spelling = path("ZBD/soundsl.zbd");
    let mut context = ParseContext::with_defaults(CONTAINER);
    let index = read_index(&mut context, &spelling, &bytes).expect("the index reads");

    assert_eq!(index.family(), ZbdFamily::Sound);
    assert_eq!(index.version(), 1);
    assert_eq!(index.len(), 3);
    assert_eq!(index.table_start(), data.len() as u64);
    assert_eq!(index.data(), &data[..]);
    assert_eq!(
        index.index_span(),
        span(data.len() as u64, 3 * INDEX_ENTRY_BYTES + TRAILER_BYTES)
    );

    let names: Vec<&[u8]> = index.entries().iter().map(|entry| entry.name()).collect();
    assert_eq!(names, [&b"engine.wav"[..], b"gun.wav", b"engine.wav"]);
    for (position, entry) in index.entries().iter().enumerate() {
        let authored = &entries[position];
        assert_eq!(entry.index(), position);
        assert_eq!(
            entry.span(),
            span(u64::from(authored.start), u64::from(authored.length))
        );
        assert_eq!(entry.name_field(), &authored.name[..]);
        assert!(entry.is_conforming(), "{position}");
        let record = data.len() as u64 + position as u64 * INDEX_ENTRY_BYTES;
        assert_eq!(entry.record_span(), span(record, INDEX_ENTRY_BYTES));
        // The unexplained bytes are verbatim, located and labelled unknown.
        let unexplained = entry.unexplained();
        assert_eq!(unexplained.bytes(), &authored.unexplained[..]);
        assert_eq!(unexplained.span(), span(record + 72, 76));
        let at = unexplained.span().offset as usize;
        assert_eq!(&bytes[at..at + 76], unexplained.bytes());
        assert_eq!(unexplained.evidence(), ClaimStatus::Unknown);
        assert_eq!(unexplained.reason(), UNEXPLAINED_REASON);
    }
    // The index carries no numeric ids.
    assert!(index.extents().iter().all(|extent| extent.id().is_none()));

    let table = index.member_table();
    let sound = read_sound_archive(&mut context, &table, index.data()).expect("sound reads");
    assert_eq!(sound.status(), ContainerStatus::Clean);
    assert_eq!(sound.len(), 3);
    assert_eq!(sound.container(), CONTAINER);
    let contents: Vec<&[u8]> = sound.entries().map(|entry| entry.content()).collect();
    assert_eq!(contents, [&data[0..16], &data[16..28], &data[28..48]]);
    assert_eq!(sound.entry(2).expect("third").name(), b"engine.wav");
    // Members cover the data region exactly; the index covers the rest.
    assert_eq!(sound.consumed_ranges(), [span(0, data.len() as u64)]);
    assert!(sound.uncovered_ranges().is_empty());
}

#[test]
fn accept_t343_reader_archives_read_their_trailer_at_every_level() {
    let data = b"first reader entrysecond".to_vec();
    let entries = vec![
        Entry::new(0, 18, b"a.zrd", 1),
        Entry::new(18, 6, b"a.zrd", 2),
    ];
    let bytes = indexed(&data, &entries);
    for spelling in ["ZBD/zrdr.zbd", "ZBD/c1/zrdr.zbd", "ZBD/m01/zrdr.zbd"] {
        let spelling = path(spelling);
        let mut context = ParseContext::with_defaults(CONTAINER);
        let index = read_index(&mut context, &spelling, &bytes).expect("the index reads");
        assert_eq!(index.family(), ZbdFamily::Reader, "{spelling:?}");
        let table = index.member_table();
        let reader = read_reader_archive(&mut context, &table, index.data()).expect("reader reads");
        assert_eq!(reader.status(), ContainerStatus::Clean);
        let entries: Vec<(&[u8], &[u8])> = reader
            .entries()
            .map(|entry| (entry.name(), entry.content()))
            .collect();
        assert_eq!(
            entries,
            [
                (&b"a.zrd"[..], &b"first reader entry"[..]),
                (b"a.zrd", b"second")
            ]
        );
        // The sound reader refuses the reader index.
        assert!(read_sound_archive(&mut context, &table, index.data()).is_err());
    }
}

#[test]
fn accept_t343_families_without_a_trailer_are_refused() {
    let bytes = indexed(&sound_data(), &sound_entries());
    // A texture archive: routed by role, not indexed by a trailer.
    let spelling = path("ZBD/c1/texture.zbd");
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_index(&mut context, &spelling, &bytes).expect_err("no trailer index");
    assert_eq!(error.code(), "not_indexed_by_trailer");
    assert_eq!(
        error,
        IndexError::NotIndexedByTrailer {
            container: CONTAINER.to_owned(),
            family: ZbdFamily::Texture,
        }
    );
    assert_eq!(context.allocation().used(), 0);
}

#[test]
fn accept_t343_other_trailer_versions_are_refused() {
    let spelling = path("ZBD/soundsh.zbd");
    for version in [0, 2, u32::MAX] {
        let bytes = archive(&sound_data(), &sound_entries(), version, 3);
        let mut context = ParseContext::with_defaults(CONTAINER);
        let error = read_index(&mut context, &spelling, &bytes).expect_err("version");
        assert_eq!(
            error,
            IndexError::UnsupportedVersion {
                container: CONTAINER.to_owned(),
                offset: bytes.len() as u64 - 8,
                version,
            }
        );
        assert_eq!(context.allocation().used(), 0);
    }
}

#[test]
fn accept_t343_truncated_and_oversized_indexes_fail_before_allocating() {
    let spelling = path("ZBD/soundsl.zbd");
    // Shorter than the trailer.
    for length in 0..8 {
        let bytes = vec![1u8; length];
        let mut context = ParseContext::with_defaults(CONTAINER);
        let Err(IndexError::Parse(error)) = read_index(&mut context, &spelling, &bytes) else {
            panic!("a {length}-byte archive has no trailer");
        };
        assert_eq!(error.kind, ParseErrorKind::UnexpectedEof, "{length}");
        assert!(error.field.starts_with("zbd.trailer."), "{}", error.field);
        assert_eq!(context.allocation().used(), 0);
    }
    // A count whose entries do not fit, up to u32::MAX.
    let data = sound_data();
    for count in [4, 1_000, u32::MAX] {
        let bytes = archive(&data, &sound_entries(), 1, count);
        let mut context = ParseContext::with_defaults(CONTAINER);
        let error = read_index(&mut context, &spelling, &bytes).expect_err("count");
        assert_eq!(
            error,
            IndexError::IndexOutOfBounds {
                container: CONTAINER.to_owned(),
                offset: bytes.len() as u64 - 4,
                count,
                needed: u64::from(count) * INDEX_ENTRY_BYTES + TRAILER_BYTES,
                container_len: bytes.len() as u64,
            }
        );
        assert_eq!(context.allocation().used(), 0);
    }
    // An empty index: the whole archive is the trailer.
    let bytes = indexed(&[], &[]);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let index = read_index(&mut context, &spelling, &bytes).expect("empty index");
    assert!(index.is_empty());
    assert!(index.data().is_empty());
    assert_eq!(index.index_span(), span(0, 8));
}

#[test]
fn accept_t343_the_index_is_charged_exactly_against_the_budget() {
    let bytes = indexed(&sound_data(), &sound_entries());
    let spelling = path("ZBD/soundsl.zbd");
    let charge = 3 * (INDEX_ROW_BYTES + MEMBER_EXTENT_BYTES);

    let mut short = ParseContext::new(CONTAINER, charge - 1, 4);
    let Err(IndexError::Parse(error)) = read_index(&mut short, &spelling, &bytes) else {
        panic!("one byte short of the charge is refused");
    };
    assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
    assert!(error.field.starts_with("zbd.trailer."), "{}", error.field);
    assert_eq!(
        short.allocation().used(),
        0,
        "a refused index is rolled back"
    );

    let mut exact = ParseContext::new(CONTAINER, charge, 4);
    let index = read_index(&mut exact, &spelling, &bytes).expect("the exact charge fits");
    assert_eq!(index.len(), 3);
    assert_eq!(exact.allocation().used(), charge);
}

#[test]
fn accept_t343_entry_anomalies_are_recorded_without_hiding_siblings() {
    let data = sound_data();
    let mut unterminated = Entry::new(16, 12, b"", 0);
    unterminated.name = [b'n'; INDEX_NAME_BYTES];
    let mut padded = Entry::new(28, 20, b"pad.wav", 0);
    padded.name[40] = 0x7F;
    let entries = vec![
        Entry::new(0, 16, b"fine.wav", 0),
        Entry::new(16, 0, b"empty.wav", 0),
        unterminated,
        padded,
        Entry::new(0, 16, b"caf\xE9.wav", 0),
    ];
    let bytes = indexed(&data, &entries);
    let spelling = path("ZBD/soundsl.zbd");
    let mut context = ParseContext::with_defaults(CONTAINER);
    let index = read_index(&mut context, &spelling, &bytes).expect("the index reads");

    let anomalies: Vec<Vec<EntryAnomaly>> = index
        .entries()
        .iter()
        .map(|entry| entry.anomalies().collect())
        .collect();
    assert_eq!(
        anomalies,
        [
            vec![],
            vec![EntryAnomaly::EmptyExtent],
            vec![EntryAnomaly::UnterminatedName],
            vec![EntryAnomaly::NonZeroNamePadding],
            vec![EntryAnomaly::NonAsciiName],
        ]
    );
    assert_eq!(index.anomalous_entries().count(), 4);
    // Names stay verbatim: the whole field without a NUL, raw non-ASCII bytes.
    assert_eq!(index.entry(2).expect("entry").name(), &[b'n'; 64][..]);
    assert_eq!(index.entry(3).expect("entry").name(), b"pad.wav");
    assert_eq!(index.entry(4).expect("entry").name(), b"caf\xE9.wav");

    // Every entry still reaches the reader.
    let table = index.member_table();
    let sound = read_sound_archive(&mut context, &table, index.data()).expect("sound reads");
    assert_eq!(sound.len(), 5);
    assert_eq!(sound.status(), ContainerStatus::Clean);
    assert_eq!(sound.entry(1).expect("empty member").content(), b"");
}

#[test]
fn accept_t343_a_member_reaching_into_the_index_fails_on_its_own_row() {
    let data = sound_data();
    let entries = vec![
        Entry::new(0, 16, b"fine.wav", 0),
        // Ends 4 bytes past the data region, inside the first index entry.
        Entry::new(40, 12, b"spill.wav", 0),
        Entry::new(16, 12, b"also-fine.wav", 0),
    ];
    let bytes = indexed(&data, &entries);
    let spelling = path("ZBD/soundsl.zbd");
    let mut context = ParseContext::with_defaults(CONTAINER);
    let index = read_index(&mut context, &spelling, &bytes).expect("the index reads");
    let table = index.member_table();
    let sound = read_sound_archive(&mut context, &table, index.data()).expect("sound reads");

    assert_eq!(sound.status(), ContainerStatus::Failed { failures: 1 });
    let row = sound.listing().row(1).expect("row");
    assert_eq!(
        row.status(),
        MemberStatus::Failed(MemberError::OutOfBounds {
            index: 1,
            offset: 40,
            length: 12,
            container_len: 48,
            end: 52,
        })
    );
    assert!(sound.entry(1).is_none());
    assert_eq!(sound.entry(2).expect("sibling").content(), &data[16..28]);
    // The bytes no readable member claimed are reported.
    assert_eq!(sound.uncovered_ranges(), [span(28, 20)]);
}

// --- Retail -------------------------------------------------------------------

/// What production code established about one retail sound/reader archive.
#[derive(Debug)]
struct RetailIndex {
    spelling: String,
    family: ZbdFamily,
    size_bytes: u64,
    members: usize,
    table_start: u64,
    duplicate_names: usize,
    anomalies: usize,
    failures: usize,
    uncovered_bytes: u64,
    overlapping_members: usize,
    ordered: bool,
}

/// Reads every retail sound and reader archive through the production
/// index reader and family readers.
fn retail_indexes() -> Vec<RetailIndex> {
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
        // The count word, read independently of the production parser.
        let declared = u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().expect("four"));
        assert_eq!(index.len(), declared as usize, "{spelling}: every member");

        let table = index.member_table();
        let (failures, uncovered, rows) = match decided.family() {
            ZbdFamily::Sound => {
                let sound = read_sound_archive(&mut context, &table, index.data())
                    .unwrap_or_else(|error| panic!("{spelling}: {error}"));
                for entry in sound.entries() {
                    let content = entry.content();
                    assert!(
                        content.len() >= 12
                            && &content[0..4] == b"RIFF"
                            && &content[8..12] == b"WAVE",
                        "{spelling}: member {}",
                        entry.index()
                    );
                }
                let rows: Vec<SourceSpan> =
                    sound.listing().rows().iter().map(|r| r.span()).collect();
                (sound.failures(), sound.uncovered_ranges(), rows)
            }
            _ => {
                let reader = read_reader_archive(&mut context, &table, index.data())
                    .unwrap_or_else(|error| panic!("{spelling}: {error}"));
                let rows: Vec<SourceSpan> =
                    reader.listing().rows().iter().map(|r| r.span()).collect();
                (reader.failures(), reader.uncovered_ranges(), rows)
            }
        };
        let mut sorted = rows.clone();
        sorted.sort_by_key(|span| (span.offset, span.length));
        let overlapping = sorted
            .windows(2)
            .filter(|pair| pair[1].offset < pair[0].offset + pair[0].length)
            .count();
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();
        for entry in index.entries() {
            if !seen.insert(entry.name()) {
                duplicates.insert(entry.name());
            }
        }
        found.push(RetailIndex {
            family: decided.family(),
            size_bytes: bytes.len() as u64,
            members: index.len(),
            table_start: index.table_start(),
            duplicate_names: duplicates.len(),
            anomalies: index.anomalous_entries().count(),
            failures,
            uncovered_bytes: uncovered.iter().map(|span| span.length).sum(),
            overlapping_members: overlapping,
            ordered: rows == sorted,
            spelling,
        });
    }
    found
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t343_retail_every_sound_and_reader_archive_lists_all_members() {
    let indexes = retail_indexes();
    let count = |family| {
        indexes
            .iter()
            .filter(|index| index.family == family)
            .count()
    };
    // The archives task #340 counted.
    assert_eq!(count(ZbdFamily::Sound), 2, "sound archives");
    assert_eq!(count(ZbdFamily::Reader), 62, "reader archives");
    for index in &indexes {
        assert!(index.members > 0, "{}", index.spelling);
        assert_eq!(index.anomalies, 0, "{}: entry anomalies", index.spelling);
        assert_eq!(
            index.failures, 0,
            "{}: members inside the data",
            index.spelling
        );
        // Observed: members tile the data region with no gap and no overlap,
        // in index order, so nothing before the index is left unclaimed.
        assert_eq!(index.uncovered_bytes, 0, "{}: uncovered", index.spelling);
        assert_eq!(index.overlapping_members, 0, "{}: overlap", index.spelling);
        assert!(
            index.ordered,
            "{}: index order is offset order",
            index.spelling
        );
    }
    let members: usize = indexes.iter().map(|index| index.members).sum();
    let duplicates: usize = indexes.iter().map(|index| index.duplicate_names).sum();
    println!(
        "{} archives, {members} members, {duplicates} duplicated names",
        indexes.len()
    );
}

// --- Evidence harness ---------------------------------------------------------

/// Evidence-report harness for task #343 (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`). Not an acceptance test: it fails
/// loudly when its inputs are missing. Run from the workspace root:
///
/// 1. ```sh
///    mkdir -p private/evidence/T343
///    cargo test --workspace --locked -- accept_t343_ --include-ignored \
///      2>&1 | tee private/evidence/T343/cargo-test.log
///    ```
///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/T343 \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t343_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_formats --test zbd -- evidence_report_t343 --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py private/evidence/T343/acceptance.json \
///      --artifact-root private/evidence/T343 --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T343.json`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_t343_writes_the_acceptance_report() {
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
    let suite = parse_suite(&log, "accept_t343_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_t343_` tests were recorded in {}",
        log_path.display()
    );
    let retail = "accept_t343_retail_every_sound_and_reader_archive_lists_all_members";
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

    // Per-archive index facts: spellings, sizes, counts — no original bytes.
    let rows: Vec<String> = retail_indexes()
        .iter()
        .map(|index| {
            let sha = hashes
                .get(&index.spelling)
                .map_or_else(|| "null".to_owned(), |hash| jstr(hash));
            format!(
                "{{\"spelling\": {}, \"sha256\": {sha}, \"family\": {}, \"size_bytes\": {}, \
                 \"members\": {}, \"table_start\": {}, \"duplicate_names\": {}, \
                 \"entry_anomalies\": {}, \"member_failures\": {}, \"uncovered_bytes\": {}, \
                 \"overlapping_members\": {}, \"index_order_is_offset_order\": {}}}",
                jstr(&index.spelling),
                jstr(index.family.as_str()),
                index.size_bytes,
                index.members,
                index.table_start,
                index.duplicate_names,
                index.anomalies,
                index.failures,
                index.uncovered_bytes,
                index.overlapping_members,
                index.ordered,
            )
        })
        .collect();
    let index_path = evidence_dir.join("zbd-member-index.json");
    fs::write(
        &index_path,
        format!(
            "{{\n \"task_id\": \"T343\",\n \"candidate_tree\": {},\n \"install_sha256\": {},\n \
             \"archive_count\": {},\n \"archives\": [\n  {}\n ]\n}}\n",
            jstr(&candidate_tree),
            jstr(&install_sha256),
            rows.len(),
            rows.join(",\n  ")
        ),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", index_path.display()));

    let artifacts = [artifact(&log_path, "log"), artifact(&index_path, "json")];
    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T343\",\n\
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
            "implementer: claude-1/claude-1 (Rally #343, implement claim of \
            2026-09-28T17:11:16Z, handed over at 17:20:59Z); reviewer: claude-1/claude-1 again, \
            on the review claim of 2026-09-28T17:21:15Z, which merged it at 17:24:48Z. The same \
            agent instance is on both sides, so this review is not independent and is not \
            independent original-reference evidence; the review claim started fifteen seconds \
            after the hand-over, so the activity log cannot prove a fresh context and none is \
            claimed. No agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production index reader and family readers over every retail sound and reader \
             archive (zbd-member-index.json), rustc and Cargo.lock; validated with \
             tools/validate_evidence.py --require-pass. The 76 unexplained bytes of each \
             index entry are retained verbatim as unknown, as the task requires; their \
             meaning is recorded as unknown in the task #343 findings"
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
