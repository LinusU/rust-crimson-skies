//! Acceptance stage F05-A: raw ROF directory structs and synthetic
//! boundary fixtures (`specs/F05-rof-directory-trees-and-compressed-members.md`,
//! section `### F05-A`).
//!
//! Every byte in this file is authored here (except
//! `fixtures/synthetic/flat-uncompressed.rof`, whose bytes are authored by
//! `tools/make_synthetic_fixtures.py` and read only for cross-checking):
//! newly authored synthetic content, no original game data, no
//! `CS_GAME_DIR` access.
//!
//! The fixtures build a directory *tree* out of separately encoded blocks
//! — a root block plus one block per directory record — so the acceptance
//! scenario (two directories, duplicate basenames, stable ids) is observable
//! while traversal itself (following a record's `start`, cycle detection,
//! bounded depth) stays with F05-B: here the test passes each block's own
//! bytes to the production entrypoint.

use cs_formats::{
    DIRECTORY_HEADER_BYTES, FLAG_DIRECTORY, ParseContext, ParseErrorKind, RECORD_BYTES,
    RofDirectory, RofError, RofFlags, RofRawHeader, read_directory,
};

/// Provenance label carried by every error these tests assert on.
const CONTAINER: &str = "synthetic/f05_a_tree.rof";

/// One record exactly as authored: the six on-disk fields, verbatim.
#[derive(Clone, Copy)]
struct RawRecord {
    start: u32,
    raw_length: u32,
    raw_length_on_disk: u32,
    flags: u32,
    name_length: u32,
    id: u32,
}

impl RawRecord {
    /// A file entry called `name` with a distinct id: `name_length` counts
    /// the name *and* its NUL, as the shared synthetic fixture authors it.
    fn file(name: &'static str, id: u32) -> Self {
        Self {
            start: 0,
            raw_length: 0,
            raw_length_on_disk: 0,
            flags: 0,
            name_length: name.len() as u32 + 1,
            id,
        }
    }

    /// A directory entry: observed flag bit 1, `start` the absolute offset
    /// of the nested block. Its two length fields are left at zero because
    /// no source establishes what they mean for a directory (recorded as an
    /// unknown in `docs/findings/`).
    fn directory(name: &'static str, id: u32, start: u32) -> Self {
        Self {
            flags: FLAG_DIRECTORY,
            start,
            ..Self::file(name, id)
        }
    }

    /// Points the entry at `start` with `length` payload bytes on both
    /// length fields: uncompressed entries author them equal, exactly as
    /// `tools/make_synthetic_fixtures.py` does.
    fn at(mut self, start: u32, length: u32) -> Self {
        self.start = start;
        self.raw_length = length;
        self.raw_length_on_disk = length;
        self
    }
}

/// The name table for `names`: every name followed by its NUL.
fn name_table(names: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for name in names {
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table
}

/// Encodes one directory block. Header fields are parameters (not derived
/// from `records`/`names`) so a test can author a header that disagrees
/// with the tables it points at.
fn block(entry_count: u32, names_length: u32, records: &[RawRecord], names: &[u8]) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(DIRECTORY_HEADER_BYTES + records.len() * RECORD_BYTES + names.len());
    bytes.extend_from_slice(&entry_count.to_le_bytes());
    bytes.extend_from_slice(&names_length.to_le_bytes());
    for record in records {
        bytes.extend_from_slice(&record.start.to_le_bytes());
        bytes.extend_from_slice(&record.raw_length.to_le_bytes());
        bytes.extend_from_slice(&record.raw_length_on_disk.to_le_bytes());
        bytes.extend_from_slice(&record.flags.to_le_bytes());
        bytes.extend_from_slice(&record.name_length.to_le_bytes());
        bytes.extend_from_slice(&record.id.to_le_bytes());
    }
    bytes.extend_from_slice(names);
    bytes
}

/// A block whose header agrees with its records and its name table.
fn valid_block(records: &[RawRecord], names: &[u8]) -> Vec<u8> {
    block(records.len() as u32, names.len() as u32, records, names)
}

/// One block inside [`Tree`]: where it starts in the file and how many
/// bytes it occupies.
#[derive(Clone, Copy)]
struct Block {
    offset: usize,
    len: usize,
}

/// The authored tree: root block, two directory blocks, five payloads.
struct Tree {
    bytes: Vec<u8>,
    root: Block,
    mis: Block,
    map: Block,
}

const ROOT_INDEX: &[u8] = b"root index\n";
const MIS_README: &[u8] = b"shared readme (MIS)\n";
const MIS_BRIEF: &[u8] = b"brief\n";
const MAP_README: &[u8] = b"shared readme (MAP)\n";
const MAP_TILES: &[u8] = b"tiles\n";

/// Places one payload at `cursor` and advances past it.
fn place(cursor: &mut u32, payload: &[u8]) -> (u32, u32) {
    let start = *cursor;
    *cursor += payload.len() as u32;
    (start, payload.len() as u32)
}

/// Builds the fixture tree in block order: `[root][MIS][MAP][payloads]`.
///
/// The root names three entries — the two directories `MIS` and `MAP`
/// plus one file. `MIS` and `MAP` each hold a file called `readme.txt`, so
/// the tree has a duplicate basename under two different directories, and
/// each copy carries its own authored id.
fn tree() -> Tree {
    let root_names = name_table(&["MIS", "MAP", "index.txt"]);
    let mis_names = name_table(&["readme.txt", "brief.dat"]);
    let map_names = name_table(&["readme.txt", "tiles.dat"]);

    let root_len = DIRECTORY_HEADER_BYTES + 3 * RECORD_BYTES + root_names.len();
    let mis_len = DIRECTORY_HEADER_BYTES + 2 * RECORD_BYTES + mis_names.len();
    let map_len = DIRECTORY_HEADER_BYTES + 2 * RECORD_BYTES + map_names.len();

    let mis_offset = root_len;
    let map_offset = root_len + mis_len;
    let mut cursor = (root_len + mis_len + map_len) as u32;

    let (root_index_start, root_index_len) = place(&mut cursor, ROOT_INDEX);
    let (mis_readme_start, mis_readme_len) = place(&mut cursor, MIS_README);
    let (mis_brief_start, mis_brief_len) = place(&mut cursor, MIS_BRIEF);
    let (map_readme_start, map_readme_len) = place(&mut cursor, MAP_README);
    let (map_tiles_start, map_tiles_len) = place(&mut cursor, MAP_TILES);

    let root_records = [
        RawRecord::directory("MIS", 1, mis_offset as u32),
        RawRecord::directory("MAP", 2, map_offset as u32),
        RawRecord::file("index.txt", 3).at(root_index_start, root_index_len),
    ];
    let mis_records = [
        RawRecord::file("readme.txt", 11).at(mis_readme_start, mis_readme_len),
        RawRecord::file("brief.dat", 12).at(mis_brief_start, mis_brief_len),
    ];
    let map_records = [
        RawRecord::file("readme.txt", 21).at(map_readme_start, map_readme_len),
        RawRecord::file("tiles.dat", 22).at(map_tiles_start, map_tiles_len),
    ];

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&valid_block(&root_records, &root_names));
    bytes.extend_from_slice(&valid_block(&mis_records, &mis_names));
    bytes.extend_from_slice(&valid_block(&map_records, &map_names));
    for payload in [ROOT_INDEX, MIS_README, MIS_BRIEF, MAP_README, MAP_TILES] {
        bytes.extend_from_slice(payload);
    }
    assert_eq!(
        bytes.len(),
        cursor as usize,
        "every payload was placed once"
    );

    Tree {
        bytes,
        root: Block {
            offset: 0,
            len: root_len,
        },
        mis: Block {
            offset: mis_offset,
            len: mis_len,
        },
        map: Block {
            offset: map_offset,
            len: map_len,
        },
    }
}

/// The names of `directory`, in block order.
fn names(directory: &RofDirectory<'_>) -> Vec<Vec<u8>> {
    directory
        .entries()
        .map(|entry| entry.name.to_vec())
        .collect()
}

/// The ids of `directory`, in block order.
fn ids(directory: &RofDirectory<'_>) -> Vec<u32> {
    directory.entries().map(|entry| entry.record.id).collect()
}

/// **AC01 / minimum acceptance scenario:** a synthetic uncompressed tree
/// has two directories, duplicate basenames and stable ids.
///
/// The two directory blocks are read with the same production entrypoint
/// the root uses, at the offsets the fixture authored for them.
#[test]
fn accept_f05_a_tree_with_two_directories_duplicate_basenames_and_stable_ids() {
    let tree = tree();
    let mut context = ParseContext::with_defaults(CONTAINER);

    let root =
        read_directory(&mut context, &tree.bytes).expect("the authored root block must parse");

    // Header and block boundary, asserted from the authored constants:
    // 8 header bytes + 3 * 24 record bytes + 18 name bytes = 98.
    assert_eq!(
        root.header(),
        RofRawHeader {
            entry_count: 3,
            names_length: 18,
        }
    );
    assert_eq!(root.block_len(), 98);
    assert_eq!(root.len(), 3);
    assert_eq!(
        names(&root),
        vec![b"MIS".to_vec(), b"MAP".to_vec(), b"index.txt".to_vec()]
    );
    assert_eq!(root.name_bytes(1), Some(b"MAP".as_slice()));

    // Exactly two directory entries, with the offsets of their blocks.
    let directories: Vec<_> = root
        .entries()
        .filter(|entry| entry.record.flags.is_directory())
        .collect();
    assert_eq!(directories.len(), 2, "the root names two directories");
    assert_eq!(directories[0].name, b"MIS");
    assert_eq!(directories[0].record.id, 1);
    assert_eq!(directories[0].record.start as usize, tree.mis.offset);
    assert_eq!(directories[1].name, b"MAP");
    assert_eq!(directories[1].record.id, 2);
    assert_eq!(directories[1].record.start as usize, tree.map.offset);

    // The non-directory entry is a file, not a directory.
    assert!(!root.record(2).expect("third record").flags.is_directory());

    // Each directory block parses from its record's `start` and ends
    // exactly where its own payload bytes begin.
    let mis = read_directory(&mut context, &tree.bytes[tree.mis.offset..])
        .expect("the authored MIS block must parse");
    let map = read_directory(&mut context, &tree.bytes[tree.map.offset..])
        .expect("the authored MAP block must parse");
    assert_eq!(mis.block_len(), 77, "8 header + 2 * 24 records + 21 names");
    assert_eq!(map.block_len(), 77);
    assert_eq!(mis.header().entry_count, 2);
    assert_eq!(map.header().entry_count, 2);
    assert_eq!(
        names(&mis),
        vec![b"readme.txt".to_vec(), b"brief.dat".to_vec()]
    );
    assert_eq!(
        names(&map),
        vec![b"readme.txt".to_vec(), b"tiles.dat".to_vec()]
    );

    // Duplicate basename: the same name exists in both directories.
    let mis_readme = mis
        .entries()
        .find(|entry| entry.name == b"readme.txt")
        .expect("MIS holds readme.txt");
    let map_readme = map
        .entries()
        .find(|entry| entry.name == b"readme.txt")
        .expect("MAP holds readme.txt");
    assert_eq!(mis_readme.name, map_readme.name);
    assert_ne!(
        mis_readme.record.id, map_readme.record.id,
        "two entries may share a basename; their ids keep them apart"
    );
    assert_eq!((mis_readme.record.id, map_readme.record.id), (11, 21));

    // Stable ids: a second read of the same bytes, and a read of an
    // owned copy elsewhere in memory, yield exactly the authored ids.
    assert_eq!(ids(&root), vec![1, 2, 3]);
    assert_eq!(ids(&mis), vec![11, 12]);
    assert_eq!(ids(&map), vec![21, 22]);

    let copy = tree.bytes.clone();
    let mut second = ParseContext::with_defaults(CONTAINER);
    let reread = read_directory(&mut second, &copy).expect("the copy must parse");
    assert_eq!(ids(&reread), ids(&root));
    assert_eq!(names(&reread), names(&root));
    let mis_again = read_directory(&mut second, &copy[tree.mis.offset..])
        .expect("the copied MIS block must parse");
    assert_eq!(ids(&mis_again), ids(&mis));
}

/// The committed shared fixture is authored by
/// `tools/make_synthetic_fixtures.py`, independently of this crate. The
/// assertions below are written from that generator's documented values
/// (`fixtures/synthetic/expected.json`), never copied from our reader's
/// output — a fixture whose writer and reader share one assumption proves
/// nothing (`docs/research/FORMAT-NOTES.md`).
#[test]
fn accept_f05_a_shared_flat_fixture_matches_independent_assertions() {
    const BYTES: &[u8] = include_bytes!("../../../fixtures/synthetic/flat-uncompressed.rof");
    // 76-byte block + the 34-byte payload it points at.
    assert_eq!(BYTES.len(), 110);

    let mut context = ParseContext::with_defaults("fixtures/synthetic/flat-uncompressed.rof");
    let directory =
        read_directory(&mut context, BYTES).expect("the shared synthetic ROF fixture must parse");

    assert_eq!(
        directory.header(),
        RofRawHeader {
            entry_count: 2,
            names_length: 20,
        }
    );
    assert_eq!(directory.len(), 2);
    // 8 header + 2 * 24 record bytes + 20 name bytes: the block ends where
    // the payload begins, and the trailing payload is not part of it.
    assert_eq!(directory.block_len(), 76);
    assert_eq!(
        names(&directory),
        vec![b"HELLO.TXT".to_vec(), b"EMPTY.DAT".to_vec()]
    );
    assert_eq!(ids(&directory), vec![101, 102]);

    let first = directory.record(0).expect("first record");
    assert_eq!(first.start, 76);
    assert_eq!(first.raw_length, 34);
    assert_eq!(first.raw_length_on_disk, 34);
    assert_eq!(first.flags, RofFlags(0));
    assert_eq!(first.name_length, 10, "the name plus its NUL");
    assert_eq!(first.id, 101);

    // The zero-length entry: an empty member is parsed, not rejected.
    let second = directory.record(1).expect("second record");
    assert_eq!(second.start, 110);
    assert_eq!(second.raw_length, 0);
    assert_eq!(second.raw_length_on_disk, 0);
    assert_eq!(second.id, 102);

    // The bytes after the block are the authored payload, byte for byte.
    assert_eq!(&BYTES[76..110], b"Newly authored synthetic archive.\n");
}

/// Spec F05 non-negotiable #4: the two length fields are preserved as two
/// independent raw values and are never reinterpreted from their names,
/// and unknown flag bits survive verbatim so a later stage can surface
/// `UnsupportedLayout` before it extracts a span (#5).
#[test]
fn accept_f05_a_raw_fields_are_preserved_verbatim() {
    let records = [RawRecord {
        start: 4096,
        raw_length: 7,
        raw_length_on_disk: 11,
        flags: 0x8,
        name_length: 2,
        id: 0xDEAD_BEEF,
    }];
    let bytes = valid_block(&records, b"A\0");

    let mut context = ParseContext::with_defaults(CONTAINER);
    let directory = read_directory(&mut context, &bytes)
        .expect("raw fidelity is not a validity rule: the block parses");

    let record = directory.record(0).expect("the single record");
    assert_eq!(record.start, 4096, "extents are recorded, not checked here");
    assert_eq!(record.raw_length, 7);
    assert_eq!(record.raw_length_on_disk, 11);
    assert_ne!(
        record.raw_length, record.raw_length_on_disk,
        "the two length fields must not be collapsed into one"
    );
    assert_eq!(record.flags, RofFlags(0x8));
    assert_eq!(record.flags.bits(), 0x8);
    assert_eq!(record.flags.unknown_bits(), 0x8);
    assert!(record.flags.has_unknown_bits());
    assert!(!record.flags.is_directory());
    assert!(!record.flags.is_compressed());
    assert_eq!(record.name_length, 2);
    assert_eq!(record.id, 0xDEAD_BEEF);
    assert_eq!(directory.name_bytes(0), Some(b"A".as_slice()));

    // The observed bits keep their meaning on a normal directory record.
    let directory_flags = RofFlags(FLAG_DIRECTORY);
    assert!(directory_flags.is_directory());
    assert!(!directory_flags.has_unknown_bits());

    // The allocation model charges `entry_count * RECORD_BYTES`, which is
    // only honest while the decoded record is exactly the six on-disk
    // words.
    assert_eq!(
        std::mem::size_of::<cs_formats::RofRawRecord>(),
        RECORD_BYTES
    );
}

/// Spec F05 non-negotiable #2: the declared lengths of the name table must
/// agree with the records that point into it — in both directions.
#[test]
fn accept_f05_a_name_table_declared_length_mismatch_is_rejected() {
    let records = [RawRecord::file("AB", 1), RawRecord::file("CD", 2)];
    let table = name_table(&["AB", "CD"]); // records describe 6 bytes

    // Header declares one byte more than the records describe.
    let mut over = table.clone();
    over.push(0);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_directory(&mut context, &block(2, over.len() as u32, &records, &over))
        .expect_err("a name table larger than the records describe must be rejected");
    assert_eq!(error.code(), "name_table_length");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(
        error.offset(),
        DIRECTORY_HEADER_BYTES as u64 + 2 * RECORD_BYTES as u64
    );
    match &error {
        RofError::NameTableLength {
            declared,
            described,
            ..
        } => {
            assert_eq!(*declared, 7);
            assert_eq!(*described, 6);
        }
        other => panic!("expected NameTableLength, got {other:?}"),
    }

    // Header declares one byte less than the records describe.
    let error = read_directory(&mut context, &block(2, 5, &records, &table[..5]))
        .expect_err("a name table shorter than the records describe must be rejected");
    match &error {
        RofError::NameTableLength {
            declared,
            described,
            ..
        } => {
            assert_eq!(*declared, 5);
            assert_eq!(*described, 6);
        }
        other => panic!("expected NameTableLength, got {other:?}"),
    }
}

/// Spec F05 non-negotiable #2: a declared name must end in its NUL.
#[test]
fn accept_f05_a_name_without_terminator_is_rejected() {
    let records = [RawRecord {
        name_length: 5,
        ..RawRecord::file("abcdX", 1)
    }];
    let bytes = block(1, 5, &records, b"abcdX");

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_directory(&mut context, &bytes)
        .expect_err("a name with no terminator must be rejected");
    assert_eq!(error.code(), "unterminated_name");
    assert_eq!(error.container(), CONTAINER);
    // The name table starts after the header and the single record.
    assert_eq!(
        error.offset(),
        (DIRECTORY_HEADER_BYTES + RECORD_BYTES) as u64
    );
    assert!(error.to_string().contains("0x00"), "display: {error}");
}

/// Spec F05 non-negotiable #2: a record with no room for its terminator is
/// rejected rather than read as an empty name.
#[test]
fn accept_f05_a_zero_length_name_is_rejected() {
    let records = [RawRecord {
        name_length: 0,
        ..RawRecord::file("A", 1)
    }];
    let bytes = block(1, 0, &records, b"");

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_directory(&mut context, &bytes)
        .expect_err("a record declaring no name bytes must be rejected");
    assert_eq!(error.code(), "empty_name");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(
        error.offset(),
        (DIRECTORY_HEADER_BYTES + RECORD_BYTES) as u64
    );
}

/// Spec F05 non-negotiable #2: NUL splitting and the records must agree on
/// how many names exist, so an interior NUL is a rejection, not a silent
/// rename.
#[test]
fn accept_f05_a_name_with_interior_nul_is_rejected() {
    let records = [RawRecord {
        name_length: 5,
        ..RawRecord::file("AB", 1)
    }];
    let bytes = block(1, 5, &records, b"AB\0C\0");

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_directory(&mut context, &bytes)
        .expect_err("a name holding an interior NUL must be rejected");
    assert_eq!(error.code(), "interior_nul");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(
        error.offset(),
        (DIRECTORY_HEADER_BYTES + RECORD_BYTES) as u64
    );
}

/// Boundary fixture: **every** truncation of a block is rejected, and a
/// rejected attempt never leaves a charge on this parse's allocation
/// ledger (so the same context can retry those bytes honestly).
#[test]
fn accept_f05_a_every_truncation_of_a_block_is_rejected() {
    let tree = tree();
    let mut context = ParseContext::with_defaults(CONTAINER);

    for len in 0..tree.root.len {
        let error = match read_directory(&mut context, &tree.bytes[..len]) {
            Err(error) => error,
            Ok(_) => panic!("a block truncated to {len} bytes must not parse"),
        };
        assert!(
            matches!(error, RofError::Parse(_)),
            "truncation to {len} bytes must fail structurally, got {error:?}"
        );
    }
    assert_eq!(
        context.allocation().used(),
        0,
        "rejected attempts must not charge the allocation ledger"
    );

    // The two anchors the reader reports for the shortest truncations.
    let error = read_directory(&mut context, &tree.bytes[..0]).expect_err("empty input must fail");
    match &error {
        RofError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(error.field, "rof.directory.header.entry_count");
            assert_eq!(error.container, CONTAINER);
            assert_eq!(error.offset, 0);
        }
        other => panic!("expected a parse failure, got {other:?}"),
    }
    let error = read_directory(&mut context, &tree.bytes[..DIRECTORY_HEADER_BYTES])
        .expect_err("a header without records must fail");
    match &error {
        RofError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(error.field, "rof.directory.records");
            assert_eq!(error.offset, DIRECTORY_HEADER_BYTES as u64);
        }
        other => panic!("expected a parse failure, got {other:?}"),
    }

    // The untruncated block parses, and only now is its decoded table
    // charged: three records of 24 bytes.
    let directory =
        read_directory(&mut context, &tree.bytes).expect("the untruncated block parses");
    assert_eq!(
        context.allocation().used(),
        (3 * RECORD_BYTES) as u64,
        "a successful read books exactly the decoded record table"
    );
    assert_eq!(directory.block_len(), tree.root.len);
}

/// The allocation budget refuses a record table that does not fit the
/// parse's configured limit, exactly at the boundary: 48 bytes of budget
/// refuses the 72-byte table of the three-entry root block, and 72 bytes
/// accepts it. Nothing is charged by the refusal.
#[test]
fn accept_f05_a_record_table_beyond_the_allocation_budget_is_refused() {
    let tree = tree();

    let mut refused = ParseContext::new(CONTAINER, 2 * RECORD_BYTES as u64, 8);
    let error = read_directory(&mut refused, &tree.bytes)
        .expect_err("72 bytes of records cannot fit a 48-byte budget");
    match &error {
        RofError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
            assert_eq!(error.field, "rof.directory.records");
            assert_eq!(error.container, CONTAINER);
            assert!(
                error.observed.contains("72"),
                "observed: {}",
                error.observed
            );
        }
        other => panic!("expected a budget refusal, got {other:?}"),
    }
    assert_eq!(
        refused.allocation().used(),
        0,
        "a refused reservation is never charged"
    );

    let mut accepted = ParseContext::new(CONTAINER, 3 * RECORD_BYTES as u64, 8);
    read_directory(&mut accepted, &tree.bytes)
        .expect("72 bytes of records fit a 72-byte budget exactly");
    assert_eq!(accepted.allocation().used(), 3 * RECORD_BYTES as u64);
}

/// A hostile `entry_count` is refused by checked arithmetic before any
/// table is built or charged: the record table it claims cannot be read,
/// and the parse reports the field and the absolute offset instead of
/// allocating `u32::MAX * 24` bytes.
#[test]
fn accept_f05_a_hostile_entry_count_is_refused_by_bounds_checks() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // No record table follows the header.

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_directory(&mut context, &bytes)
        .expect_err("u32::MAX records cannot fit 8 bytes of input");
    match &error {
        RofError::Parse(error) => {
            // `UnexpectedEof` where `u32::MAX * 24` fits the target's
            // `usize`, `LengthOverflow` where the product itself does not.
            assert!(
                matches!(
                    error.kind,
                    ParseErrorKind::UnexpectedEof | ParseErrorKind::LengthOverflow
                ),
                "unexpected kind {:?}",
                error.kind
            );
            assert_eq!(error.field, "rof.directory.records");
            assert_eq!(error.container, CONTAINER);
            assert_eq!(error.offset, DIRECTORY_HEADER_BYTES as u64);
        }
        other => panic!("expected a structural failure, got {other:?}"),
    }
    assert_eq!(
        context.allocation().used(),
        0,
        "the hostile count must not reach the allocation ledger"
    );
}
