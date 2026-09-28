//! Acceptance stages F05-A and F05-B: raw ROF directory structs, synthetic
//! boundary fixtures, the directory walk and bounded member reads
//! (`specs/F05-rof-directory-trees-and-compressed-members.md`, sections
//! `### F05-A` and `### F05-B`).
//!
//! Every byte in this file is authored here (except
//! `fixtures/synthetic/flat-uncompressed.rof`, whose bytes are authored by
//! `tools/make_synthetic_fixtures.py` and read only for cross-checking, and
//! the zlib streams noted where they appear): newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access.
//!
//! The F05-A fixtures build a directory *tree* out of separately encoded
//! blocks — a root block plus one block per directory record — so the
//! acceptance scenario (two directories, duplicate basenames, stable ids)
//! is observable while traversal itself (following a record's `start`, cycle
//! detection, bounded depth) stays with F05-B: here the test passes each
//! block's own bytes to the production entrypoint. F05-B walks that same
//! tree through `read_tree` and reads its members through `read_member`.

use cs_formats::{
    AllocationBudget, DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, ParseContext,
    ParseErrorKind, RECORD_BYTES, RofDirectory, RofError, RofFlags, RofLimits, RofMember,
    RofRawHeader, RofRawRecord, RofTree, RofTreeDirectory, read_directory, read_member, read_tree,
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

// ---------------------------------------------------------------------------
// F05-B: directory traversal and bounded member reads
// ---------------------------------------------------------------------------

/// The payload of the compressed fixtures: 204 bytes, four repetitions of
/// one authored line.
const COMPRESSED_PAYLOAD: &[u8] = &[
    0x43, 0x72, 0x69, 0x6d, 0x73, 0x6f, 0x6e, 0x20, 0x53, 0x6b, 0x69, 0x65, 0x73, 0x20, 0x73, 0x79,
    0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63, 0x20, 0x63, 0x6f, 0x6d, 0x70, 0x72, 0x65, 0x73, 0x73,
    0x65, 0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62, 0x65, 0x72, 0x20, 0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61,
    0x64, 0x2e, 0x0a, 0x43, 0x72, 0x69, 0x6d, 0x73, 0x6f, 0x6e, 0x20, 0x53, 0x6b, 0x69, 0x65, 0x73,
    0x20, 0x73, 0x79, 0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63, 0x20, 0x63, 0x6f, 0x6d, 0x70, 0x72,
    0x65, 0x73, 0x73, 0x65, 0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62, 0x65, 0x72, 0x20, 0x70, 0x61, 0x79,
    0x6c, 0x6f, 0x61, 0x64, 0x2e, 0x0a, 0x43, 0x72, 0x69, 0x6d, 0x73, 0x6f, 0x6e, 0x20, 0x53, 0x6b,
    0x69, 0x65, 0x73, 0x20, 0x73, 0x79, 0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63, 0x20, 0x63, 0x6f,
    0x6d, 0x70, 0x72, 0x65, 0x73, 0x73, 0x65, 0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62, 0x65, 0x72, 0x20,
    0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x2e, 0x0a, 0x43, 0x72, 0x69, 0x6d, 0x73, 0x6f, 0x6e,
    0x20, 0x53, 0x6b, 0x69, 0x65, 0x73, 0x20, 0x73, 0x79, 0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63,
    0x20, 0x63, 0x6f, 0x6d, 0x70, 0x72, 0x65, 0x73, 0x73, 0x65, 0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62,
    0x65, 0x72, 0x20, 0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x2e, 0x0a,
];

/// [`COMPRESSED_PAYLOAD`] as a zlib stream (62 bytes), produced by
/// `python3 -c "import zlib; ..."` (CPython 3.14, zlib 1.2.12) — an
/// implementation that shares no code with the decoder under test, because
/// "a fixture whose writer and reader share the same wrong assumption is
/// not independent validation" (`docs/research/FORMAT-NOTES.md`).
const COMPRESSED_STREAM: &[u8] = &[
    0x78, 0x9c, 0x73, 0x2e, 0xca, 0xcc, 0x2d, 0xce, 0xcf, 0x53, 0x08, 0xce, 0xce, 0x4c, 0x2d, 0x56,
    0x28, 0xae, 0xcc, 0x2b, 0xc9, 0x48, 0x2d, 0xc9, 0x4c, 0x56, 0x48, 0xce, 0xcf, 0x2d, 0x28, 0x4a,
    0x2d, 0x2e, 0x4e, 0x4d, 0x51, 0xc8, 0x4d, 0xcd, 0x4d, 0x4a, 0x2d, 0x52, 0x28, 0x48, 0xac, 0xcc,
    0xc9, 0x4f, 0x4c, 0xd1, 0xe3, 0x72, 0x1e, 0xac, 0x5a, 0x00, 0xd7, 0xca, 0x4c, 0x91,
];

/// 128 KiB of zero bytes as a zlib stream (149 bytes): a stored extent far
/// smaller than what it decodes to, i.e. an expansion bomb. Same provenance
/// as [`COMPRESSED_STREAM`].
const BOMB_STREAM: &[u8] = &[
    0x78, 0xda, 0xed, 0xc1, 0x31, 0x01, 0x00, 0x00, 0x00, 0xc2, 0xa0, 0xf5, 0x4f, 0xed, 0x61, 0x0d,
    0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x6e, 0x00, 0x1e, 0x00, 0x01,
];

/// A container with a root block listing one file entry, then `payload`
/// right after the block. Both length words are parameters: the compressed
/// fixtures are exactly the case where they differ.
fn single_member_file(
    name: &'static str,
    flags: u32,
    raw_length: u32,
    raw_length_on_disk: u32,
    id: u32,
    payload: &[u8],
) -> Vec<u8> {
    let names = name_table(&[name]);
    let block_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len();
    let records = [RawRecord {
        start: block_len as u32,
        raw_length,
        raw_length_on_disk,
        flags,
        name_length: names.len() as u32,
        id,
    }];
    let mut bytes = valid_block(&records, &names);
    assert_eq!(bytes.len(), block_len, "the payload starts after the block");
    bytes.extend_from_slice(payload);
    bytes
}

/// A root block with a single directory entry pointing at `start`.
fn root_directory_pointing_at(start: u32) -> Vec<u8> {
    let names = name_table(&["SUB"]);
    valid_block(&[RawRecord::directory("SUB", 1, start)], &names)
}

/// A root block whose single file entry claims 1000 bytes that the
/// container does not hold.
fn outside_file_member() -> Vec<u8> {
    single_member_file("FAR.DAT", 0, 1000, 1000, 5, b"")
}

/// A root block whose single file entry declares an unknown flag bit.
fn unknown_flag_member() -> Vec<u8> {
    single_member_file("X.DAT", 0x8, 0, 0, 5, b"")
}

/// Two file entries claiming extents that share 30 bytes.
fn overlapping_members() -> Vec<u8> {
    let names = name_table(&["A.DAT", "B.DAT"]);
    let records = [
        RawRecord::file("A.DAT", 1).at(200, 50),
        RawRecord::file("B.DAT", 2).at(220, 50),
    ];
    let mut bytes = valid_block(&records, &names);
    bytes.resize(300, 0);
    bytes
}

/// A chain of `levels` directory blocks, each pointing at the next one; the
/// last block is empty. Blocks are 34 bytes (`8 + 24 + "D\0"`), so block
/// `level` starts at `level * 34`.
fn directory_chain(levels: u32) -> Vec<u8> {
    let names = name_table(&["D"]);
    let width = (DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len()) as u32;
    let mut bytes = Vec::new();
    for level in 0..levels {
        let next = (level + 1) * width;
        if level + 1 == levels {
            bytes.extend_from_slice(&valid_block(&[], &[]));
        } else {
            bytes.extend_from_slice(&valid_block(&[RawRecord::directory("D", 1, next)], &names));
        }
    }
    bytes
}

/// The member of `tree` whose root-relative path is `path`.
fn member_of<'a, 'b>(tree: &'b RofTree<'a>, path: &[&[u8]]) -> &'b RofMember<'a> {
    tree.members()
        .iter()
        .find(|member| member.path == path)
        .expect("the fixture lists this member")
}

/// **AC01 through the production walk:** the synthetic uncompressed tree has
/// two directories, duplicate basenames and stable ids — this time found by
/// `read_tree` following the records, not by the test.
#[test]
fn accept_f05_b_traversal_finds_two_directories_duplicate_basenames_and_stable_ids() {
    let fixture = tree();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &fixture.bytes).expect("the authored tree must traverse");

    // Every block: root first, then depth-first, each with its own path.
    assert_eq!(walked.directories().len(), 3);
    assert_eq!(walked.root().offset, 0);
    assert_eq!(walked.root().path, Vec::<&[u8]>::new());
    assert_eq!(walked.root().directory.block_len(), fixture.root.len);
    assert_eq!(walked.root().directory.header().entry_count, 3);
    let directory_paths: Vec<Vec<&[u8]>> = walked
        .directories()
        .iter()
        .map(|directory| directory.path.clone())
        .collect();
    assert_eq!(
        directory_paths,
        vec![vec![], vec![b"MIS".as_slice()], vec![b"MAP".as_slice()]]
    );
    assert_eq!(walked.directories()[1].offset as usize, fixture.mis.offset);
    assert_eq!(walked.directories()[2].offset as usize, fixture.map.offset);

    // Every member, depth-first, with the id its record declares.
    let listed: Vec<(Vec<&[u8]>, u32)> = walked
        .members()
        .iter()
        .map(|member| (member.path.clone(), member.record.id))
        .collect();
    // Depth-first: MIS's entries, then MAP's, then the root's own file
    // (the root lists MIS, MAP and index.txt in that order).
    assert_eq!(
        listed,
        vec![
            (vec![b"MIS".as_slice(), b"readme.txt".as_slice()], 11),
            (vec![b"MIS".as_slice(), b"brief.dat".as_slice()], 12),
            (vec![b"MAP".as_slice(), b"readme.txt".as_slice()], 21),
            (vec![b"MAP".as_slice(), b"tiles.dat".as_slice()], 22),
            (vec![b"index.txt".as_slice()], 3),
        ]
    );

    // Duplicate basename under two directories, stable ids keeping them
    // apart: the same name, two members, two ids.
    let readmes: Vec<&RofMember<'_>> = walked
        .members()
        .iter()
        .filter(|member| member.path.last().copied() == Some(b"readme.txt".as_slice()))
        .collect();
    assert_eq!(readmes.len(), 2);
    assert_eq!((readmes[0].record.id, readmes[1].record.id), (11, 21));

    // Both declared extents end inside the container and match the record.
    let file_len = fixture.bytes.len() as u64;
    for member in walked.members() {
        assert!(member.length_end <= file_len, "{member:?}");
        assert!(member.length_on_disk_end <= file_len, "{member:?}");
        assert_eq!(
            member.length_end - member.start,
            u64::from(member.record.raw_length)
        );
        assert_eq!(
            member.length_on_disk_end - member.start,
            u64::from(member.record.raw_length_on_disk)
        );
    }

    // Stable ids and paths: the same bytes walked again yield the same
    // listing.
    let copy = fixture.bytes.clone();
    let mut second = ParseContext::with_defaults(CONTAINER);
    let reread = read_tree(&mut second, &copy).expect("the copy must traverse");
    let ids: Vec<u32> = reread
        .members()
        .iter()
        .map(|member| member.record.id)
        .collect();
    assert_eq!(ids, vec![11, 12, 21, 22, 3]);
    let listed_again: Vec<Vec<&[u8]>> = reread
        .members()
        .iter()
        .map(|member| member.path.clone())
        .collect();
    assert_eq!(
        listed_again,
        listed.into_iter().map(|(path, _)| path).collect::<Vec<_>>()
    );
}

/// Every payload of the authored tree, and the committed Python-authored
/// fixture, read back byte for byte through `read_member` — with the stored
/// and decoded lengths reported for each (AC04's byte-identity against a
/// reference tool needs retail data and stays with F05-D; this pins the
/// synthetic path).
#[test]
fn accept_f05_b_uncompressed_member_reads_are_byte_identical() {
    let fixture = tree();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &fixture.bytes).expect("the authored tree must traverse");

    for (path, expected) in [
        (vec![b"index.txt".as_slice()], ROOT_INDEX),
        (
            vec![b"MIS".as_slice(), b"readme.txt".as_slice()],
            MIS_README,
        ),
        (vec![b"MIS".as_slice(), b"brief.dat".as_slice()], MIS_BRIEF),
        (
            vec![b"MAP".as_slice(), b"readme.txt".as_slice()],
            MAP_README,
        ),
        (vec![b"MAP".as_slice(), b"tiles.dat".as_slice()], MAP_TILES),
    ] {
        let member = member_of(&walked, &path);
        let read = read_member(&context, &fixture.bytes, member, &RofLimits::default())
            .expect("the extent is inside the container");
        assert_eq!(read.data.as_slice(), expected, "member {path:?}");
        assert_eq!(read.stored_len, expected.len() as u64);
        assert_eq!(read.decoded_len, expected.len() as u64);
        assert_eq!(read.trailing_len, 0);
    }

    // The committed fixture through the same production path: its payload
    // and its empty member read exactly as the generator wrote them.
    const BYTES: &[u8] = include_bytes!("../../../fixtures/synthetic/flat-uncompressed.rof");
    let mut context = ParseContext::with_defaults("fixtures/synthetic/flat-uncompressed.rof");
    let walked = read_tree(&mut context, BYTES).expect("the shared fixture must traverse");
    assert_eq!(walked.directories().len(), 1);
    assert_eq!(walked.members().len(), 2);

    let hello = read_member(
        &context,
        BYTES,
        member_of(&walked, &[b"HELLO.TXT".as_slice()]),
        &RofLimits::default(),
    )
    .expect("HELLO.TXT is inside the file");
    assert_eq!(hello.data.as_slice(), &BYTES[76..110]);
    assert_eq!(
        (hello.stored_len, hello.decoded_len, hello.trailing_len),
        (34, 34, 0)
    );

    // The zero-length member reads zero bytes instead of failing.
    let empty = read_member(
        &context,
        BYTES,
        member_of(&walked, &[b"EMPTY.DAT".as_slice()]),
        &RofLimits::default(),
    )
    .expect("an empty member is a valid read");
    assert!(empty.data.is_empty());
    assert_eq!(
        (empty.stored_len, empty.decoded_len, empty.trailing_len),
        (0, 0, 0)
    );
}

/// **AC02 / the minimum acceptance scenario:** compressed data where stored
/// and decoded lengths differ, and the selected profile explains both —
/// stored = the record's `raw_length` extent (the field the reference
/// extractor hands to zlib [S05]), decoded = what the bounded decoder
/// produces (no record field states it), `raw_length_on_disk` validated but
/// never read (its meaning is the F05-D research blocker).
#[test]
fn accept_f05_b_compressed_member_stored_and_decoded_lengths_differ() {
    let stream_len = COMPRESSED_STREAM.len() as u32; // 62 bytes stored
    let payload_len = COMPRESSED_PAYLOAD.len() as u32; // 204 bytes decoded
    assert_ne!(
        stream_len, payload_len,
        "the fixture is asymmetric by design"
    );
    // The second length word carries a different value: inside the
    // container, but neither the stored nor the decoded length.
    let other_length = stream_len / 2 + 1; // 32

    let bytes = single_member_file(
        "packed.bin",
        FLAG_COMPRESSED,
        stream_len,
        other_length,
        77,
        COMPRESSED_STREAM,
    );
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &bytes).expect("the compressed member must traverse");
    assert_eq!(walked.members().len(), 1);
    let member = &walked.members()[0];
    assert_eq!(member.path, [b"packed.bin".as_slice()]);
    assert_eq!(member.record.flags, RofFlags(FLAG_COMPRESSED));

    // Profile part one: the stored extent is `raw_length`, and the other
    // declared word is a different, in-bounds value.
    assert_eq!(member.record.raw_length, stream_len);
    assert_eq!(member.record.raw_length_on_disk, other_length);
    assert_ne!(member.record.raw_length, member.record.raw_length_on_disk);
    assert_eq!(
        member.length_end,
        bytes.len() as u64,
        "the extent ends at the end of the file"
    );
    assert_eq!(
        member.length_on_disk_end,
        member.start + u64::from(other_length)
    );
    assert!(member.length_on_disk_end <= bytes.len() as u64);

    // Profile part two: the two lengths the profile reports, and neither
    // half comes from the wrong field.
    let read = read_member(&context, &bytes, member, &RofLimits::default())
        .expect("the stream must decode");
    assert_eq!(
        read.stored_len,
        u64::from(stream_len),
        "stored = raw_length"
    );
    assert_eq!(
        read.decoded_len,
        u64::from(payload_len),
        "decoded = the decoder's output"
    );
    assert_ne!(
        read.stored_len, read.decoded_len,
        "AC02: stored and decoded differ"
    );
    assert!(read.decoded_len > read.stored_len);
    assert_ne!(
        read.decoded_len,
        u64::from(member.record.raw_length_on_disk),
        "no record field states the decoded length"
    );
    assert_eq!(read.data.as_slice(), COMPRESSED_PAYLOAD);
    assert_eq!(read.trailing_len, 0);

    // The profile is load-bearing: reading the *other* word as the extent
    // hands the decoder 32 of the 62 stream bytes and fails instead of
    // quietly returning something else.
    let flipped = RofMember {
        path: member.path.clone(),
        record: RofRawRecord {
            raw_length: other_length,
            raw_length_on_disk: stream_len,
            ..member.record
        },
        start: member.start,
        length_end: member.start + u64::from(other_length),
        length_on_disk_end: member.length_end,
    };
    let error = read_member(&context, &bytes, &flipped, &RofLimits::default())
        .expect_err("the wrong length word must not decode");
    assert_eq!(error.code(), "decode_failure");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(error.offset(), member.start);

    // And the word the reader ignores changes nothing: three containers
    // that differ only in `raw_length_on_disk` read byte-identically.
    for other in [1u32, 17, stream_len] {
        let bytes = single_member_file(
            "packed.bin",
            FLAG_COMPRESSED,
            stream_len,
            other,
            77,
            COMPRESSED_STREAM,
        );
        let mut context = ParseContext::with_defaults(CONTAINER);
        let walked = read_tree(&mut context, &bytes).expect("the fixture must traverse");
        let read = read_member(
            &context,
            &bytes,
            &walked.members()[0],
            &RofLimits::default(),
        )
        .expect("the stream must decode");
        assert_eq!(
            read.data.as_slice(),
            COMPRESSED_PAYLOAD,
            "raw_length_on_disk = {other}"
        );
        assert_eq!(read.stored_len, u64::from(stream_len));
        assert_eq!(read.decoded_len, u64::from(payload_len));
        assert_eq!(read.trailing_len, 0);
    }
}

/// Exact boundaries of a compressed extent (spec F05, non-negotiable #4):
/// bytes after the end of the stream are reported as trailing data, and a
/// stream that is cut short or corrupted fails instead of decoding halfway.
#[test]
fn accept_f05_b_compressed_extent_records_trailing_data_and_refuses_bad_streams() {
    const TRAILER: &[u8] = b"TAIL";
    let stream_len = COMPRESSED_STREAM.len();

    // The declared extent is longer than the stream: 4 trailing bytes.
    let mut payload = COMPRESSED_STREAM.to_vec();
    payload.extend_from_slice(TRAILER);
    let bytes = single_member_file(
        "packed.bin",
        FLAG_COMPRESSED,
        payload.len() as u32,
        payload.len() as u32,
        77,
        &payload,
    );
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &bytes).expect("the longer extent must traverse");
    let read = read_member(
        &context,
        &bytes,
        &walked.members()[0],
        &RofLimits::default(),
    )
    .expect("the stream decodes");
    assert_eq!(read.stored_len, (stream_len + TRAILER.len()) as u64);
    assert_eq!(
        read.trailing_len,
        TRAILER.len() as u64,
        "trailing data is reported"
    );
    assert_eq!(read.decoded_len, COMPRESSED_PAYLOAD.len() as u64);
    assert_eq!(read.data.as_slice(), COMPRESSED_PAYLOAD);

    // A stream cut short of its data: refused, with no partial output.
    let short = &COMPRESSED_STREAM[..stream_len - 3];
    let bytes = single_member_file(
        "packed.bin",
        FLAG_COMPRESSED,
        short.len() as u32,
        short.len() as u32,
        77,
        short,
    );
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &bytes).expect("a short extent still traverses");
    let error = read_member(
        &context,
        &bytes,
        &walked.members()[0],
        &RofLimits::default(),
    )
    .expect_err("a truncated stream must not decode");
    assert_eq!(error.code(), "decode_failure");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(error.offset(), walked.members()[0].start);

    // A corrupted byte: refused as well, by deflate or by adler32.
    let mut corrupt = COMPRESSED_STREAM.to_vec();
    let corrupted = corrupt.len() / 2;
    corrupt[corrupted] ^= 0xff;
    let bytes = single_member_file(
        "packed.bin",
        FLAG_COMPRESSED,
        corrupt.len() as u32,
        corrupt.len() as u32,
        77,
        &corrupt,
    );
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &bytes).expect("the corrupt extent still traverses");
    let error = read_member(
        &context,
        &bytes,
        &walked.members()[0],
        &RofLimits::default(),
    )
    .expect_err("a corrupt stream must not decode");
    assert_eq!(error.code(), "decode_failure");

    // An entry whose flags have no observed meaning is not read at all —
    // even when its extent is perfectly valid (non-negotiable #5).
    let member = RofMember {
        path: vec![b"packed.bin".as_slice()],
        record: RofRawRecord {
            flags: RofFlags(0x10),
            ..walked.members()[0].record
        },
        start: walked.members()[0].start,
        length_end: walked.members()[0].length_end,
        length_on_disk_end: walked.members()[0].length_on_disk_end,
    };
    let error = read_member(&context, &bytes, &member, &RofLimits::default())
        .expect_err("unknown flags are refused before the span is read");
    assert_eq!(error.code(), "unsupported_layout");
}

/// **AC03 (expansion bomb):** 149 stored bytes that decode to 128 KiB are
/// refused at the configured ceiling — reported, not expanded — while the
/// same member reads fine under the designed default.
#[test]
fn accept_f05_b_expansion_bomb_fails_at_the_configured_ceiling() {
    let bytes = single_member_file(
        "bomb.bin",
        FLAG_COMPRESSED,
        BOMB_STREAM.len() as u32,
        BOMB_STREAM.len() as u32,
        91,
        BOMB_STREAM,
    );
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &bytes).expect("a small stored extent traverses");
    let member = &walked.members()[0];
    // The only charge on this ledger is the tree's own booking above.
    let booked = context.allocation().used();
    assert!(booked > 0, "the traversal books the tree it returns");

    // Under the designed default the member decodes: the bomb is only a
    // bomb against a smaller ceiling, not an invalid stream.
    let read = read_member(&context, &bytes, member, &RofLimits::default())
        .expect("128 KiB is inside the 64 MiB default");
    assert_eq!(read.stored_len, BOMB_STREAM.len() as u64);
    assert_eq!(read.decoded_len, 128 * 1024);
    assert!(
        read.stored_len * 100 < read.decoded_len,
        "stored {} bytes, decoded {} bytes",
        read.stored_len,
        read.decoded_len
    );
    assert!(read.data.iter().all(|byte| *byte == 0));

    // Against a 4 KiB ceiling it stops at the ceiling and reports what it
    // would have produced (never more than one decode chunk past it).
    let limits = RofLimits::new(4096);
    let error = read_member(&context, &bytes, member, &limits)
        .expect_err("an expansion bomb must fail instead of expanding");
    assert_eq!(error.code(), "expansion_bomb");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(error.offset(), member.start);
    match &error {
        RofError::ExpansionBomb {
            limit, observed, ..
        } => {
            assert_eq!(*limit, 4096);
            assert!(
                *observed > *limit,
                "observed {observed} must pass the limit"
            );
            assert!(
                *observed <= *limit + 8 * 1024,
                "never more than one chunk past the limit, observed {observed}"
            );
        }
        other => panic!("expected an expansion bomb, got {other:?}"),
    }

    // A ceiling of zero refuses any read that would produce a byte.
    let error = read_member(&context, &bytes, member, &RofLimits::new(0))
        .expect_err("even the first chunk is refused");
    assert_eq!(error.code(), "expansion_bomb");

    // Neither the successful read nor either refusal touches the ledger:
    // a member read books nothing, the traversal's booking is unchanged.
    assert_eq!(
        context.allocation().used(),
        booked,
        "member reads book nothing"
    );
}

/// **AC03 (cycle):** a directory that points back at a block already open
/// on the path from the root fails at the repeated block — and a block
/// reached from two parents fails too, because no source documents sharing.
#[test]
fn accept_f05_b_directory_cycles_and_shared_blocks_are_refused() {
    // A directory pointing at its own block.
    let bytes = root_directory_pointing_at(0);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error =
        read_tree(&mut context, &bytes).expect_err("a self-referential directory must be refused");
    assert_eq!(error.code(), "cycle");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(error.offset(), 0, "the cycle closes at the repeated block");
    match &error {
        RofError::Cycle { depth, .. } => assert_eq!(*depth, 1),
        other => panic!("expected a cycle, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // root -> SUB -> back to the root: found on the way down, with the
    // whole path reported, not after the recursion limit was exhausted.
    let root_names = name_table(&["SUB"]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let sub_names = name_table(&["UP"]);
    let mut bytes = valid_block(
        &[RawRecord::directory("SUB", 1, root_len as u32)],
        &root_names,
    );
    bytes.extend_from_slice(&valid_block(
        &[RawRecord::directory("UP", 2, 0)],
        &sub_names,
    ));
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error =
        read_tree(&mut context, &bytes).expect_err("the loop back to the root must be refused");
    assert_eq!(error.code(), "cycle");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(error.offset(), 0, "the cycle closes at the root block");
    match &error {
        RofError::Cycle { depth, .. } => assert_eq!(*depth, 2, "root and SUB were open"),
        other => panic!("expected a cycle, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // One block under two parents: not a cycle, but sharing nobody
    // documented, so it is refused as an unsupported layout instead of
    // being traversed (and therefore listed) twice.
    let names = name_table(&["SUB", "SUB2"]);
    let root_len = DIRECTORY_HEADER_BYTES + 2 * RECORD_BYTES + names.len();
    let records = [
        RawRecord::directory("SUB", 1, root_len as u32),
        RawRecord::directory("SUB2", 2, root_len as u32),
    ];
    let mut bytes = valid_block(&records, &names);
    bytes.extend_from_slice(&valid_block(&[], &[]));
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error =
        read_tree(&mut context, &bytes).expect_err("a block under two parents must be refused");
    assert_eq!(error.code(), "unsupported_layout");
    assert_eq!(
        error.offset(),
        root_len as u64,
        "the shared block is where the second parent reaches it"
    );
    assert!(
        error.to_string().contains("reuses bytes"),
        "the work bound names the block that reused another's bytes: {error}"
    );
    assert!(
        error.to_string().contains("overlap"),
        "the refusal names the shared span: {error}"
    );
    assert_eq!(context.allocation().used(), 0);
}

/// **AC03 (bounded depth):** nesting is charged against the parse's
/// recursion budget, so a chain of directories cannot walk the reader off
/// the stack, and the refusal names the absolute offset of the block that
/// did not fit (spec F05, non-negotiable #3).
#[test]
fn accept_f05_b_bounded_depth_refuses_to_descend_forever() {
    let bytes = directory_chain(4);

    // The default budget (32 levels) takes the whole chain...
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &bytes)
        .expect("four levels are inside the designed recursion budget");
    assert_eq!(walked.directories().len(), 4);
    assert_eq!(walked.directories()[3].offset, 102, "the empty leaf block");
    assert_eq!(walked.members().len(), 0, "the chain holds no files");

    // ...a budget of two refuses the third level, at its own offset.
    let mut context = ParseContext::new(CONTAINER, AllocationBudget::DEFAULT_LIMIT, 2);
    let error = read_tree(&mut context, &bytes).expect_err("the third level must be refused");
    match &error {
        RofError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::RecursionDepthExceeded);
            assert_eq!(error.field, "rof.tree.directory");
            assert_eq!(error.container, CONTAINER);
            assert_eq!(error.offset, 68, "block 0 -> 34 -> 68: the third block");
        }
        other => panic!("expected a structural depth failure, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);
}

/// **AC03 (outside-file pointer):** a declared extent that reaches past the
/// end of the container fails before anything is read from it — a member
/// length, the second length word, and a directory pointer.
#[test]
fn accept_f05_b_outside_file_pointers_fail_before_any_read() {
    // 1. A member whose `raw_length` reaches past the end.
    let bytes = outside_file_member();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("a member past the end of the file must be refused");
    assert_eq!(error.code(), "extent_out_of_bounds");
    assert_eq!(error.container(), CONTAINER);
    match &error {
        RofError::ExtentOutOfBounds {
            offset,
            start,
            length,
            file_len,
            ..
        } => {
            assert_eq!(*start, *offset, "the extent's own start is reported");
            assert_eq!(*length, 1000);
            assert_eq!(*file_len, bytes.len() as u64);
        }
        other => panic!("expected an out-of-bounds extent, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // 2. `raw_length` fits, `raw_length_on_disk` does not: the second word
    //    is validated as an extent too, whatever it ends up meaning.
    let names = name_table(&["HALF.DAT"]);
    let block_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len();
    let records = [RawRecord {
        start: block_len as u32,
        raw_length: 4,
        raw_length_on_disk: 1000,
        flags: 0,
        name_length: names.len() as u32,
        id: 6,
    }];
    let mut bytes = valid_block(&records, &names);
    bytes.extend_from_slice(b"abcd");
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("the on-disk length must be inside the file as well");
    assert_eq!(error.code(), "extent_out_of_bounds");
    match &error {
        RofError::ExtentOutOfBounds { length, .. } => assert_eq!(*length, 1000),
        other => panic!("expected an out-of-bounds extent, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // 3. A directory pointer past the end: the walk refuses the block
    //    before reading a single byte of it.
    let bytes = root_directory_pointing_at(5000);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("a directory past the end of the file must be refused");
    assert_eq!(error.code(), "extent_out_of_bounds");
    match &error {
        RofError::ExtentOutOfBounds {
            offset,
            length,
            file_len,
            ..
        } => {
            assert_eq!(*offset, 5000);
            assert_eq!(
                *length, DIRECTORY_HEADER_BYTES as u64,
                "a block needs its header"
            );
            assert_eq!(*file_len, bytes.len() as u64);
        }
        other => panic!("expected an out-of-bounds extent, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // 4. A directory record's length words are established as well,
    //    although no source says what they mean for a directory: a word
    //    that reaches past the end fails before the block is descended
    //    into, reported at the extent's own start like a file entry's.
    let root_names = name_table(&["SUB"]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let records = [RawRecord {
        raw_length: 1000,
        ..RawRecord::directory("SUB", 1, root_len as u32)
    }];
    let mut bytes = valid_block(&records, &root_names);
    bytes.extend_from_slice(&valid_block(&[], &[]));
    let container_len = bytes.len() as u64;
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("a directory record length past the end must be refused");
    assert_eq!(error.code(), "extent_out_of_bounds");
    match &error {
        RofError::ExtentOutOfBounds {
            offset,
            start,
            length,
            file_len,
            ..
        } => {
            assert_eq!(*offset, root_len as u64, "the extent's own start");
            assert_eq!(*start, root_len as u64);
            assert_eq!(*length, 1000);
            assert_eq!(*file_len, container_len);
        }
        other => panic!("expected an out-of-bounds extent, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // 5. The second word of a directory record likewise: the check is on
    //    both words of every record, not only on `raw_length`.
    let records = [RawRecord {
        raw_length_on_disk: 4000,
        ..RawRecord::directory("SUB", 1, root_len as u32)
    }];
    let mut bytes = valid_block(&records, &root_names);
    bytes.extend_from_slice(&valid_block(&[], &[]));
    let container_len = bytes.len() as u64;
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("a directory record on-disk length past the end must be refused");
    assert_eq!(error.code(), "extent_out_of_bounds");
    match &error {
        RofError::ExtentOutOfBounds {
            offset,
            length,
            file_len,
            ..
        } => {
            assert_eq!(*offset, root_len as u64);
            assert_eq!(*length, 4000);
            assert_eq!(*file_len, container_len);
        }
        other => panic!("expected an out-of-bounds extent, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // 6. `read_member` checks again, so a hand-built member cannot read
    //    outside the container either.
    let bytes = single_member_file("OK.DAT", 0, 4, 4, 1, b"abcd");
    let context = ParseContext::with_defaults(CONTAINER);
    let member = RofMember {
        path: vec![b"OK.DAT".as_slice()],
        record: RofRawRecord {
            start: 1_000_000,
            raw_length: 4,
            raw_length_on_disk: 4,
            flags: RofFlags(0),
            name_length: 7,
            id: 1,
        },
        start: 1_000_000,
        length_end: 1_000_004,
        length_on_disk_end: 1_000_004,
    };
    let error = read_member(&context, &bytes, &member, &RofLimits::default())
        .expect_err("a hand-built member past the end must be refused");
    assert_eq!(error.code(), "extent_out_of_bounds");
    assert_eq!(error.offset(), 1_000_000);
}

/// **AC03 (invalid name table):** a nested block whose header disagrees
/// with its records is refused at the *absolute* offset of the name table,
/// with the walk's own error scope on top.
#[test]
fn accept_f05_b_invalid_name_table_fails_a_nested_block() {
    let root_names = name_table(&["SUB"]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len(); // 36
    let sub_records = [RawRecord::file("AB", 1)];
    let mut over = name_table(&["AB"]); // records describe 3 bytes
    over.push(0); // header declares 4
    let sub_block = block(
        sub_records.len() as u32,
        over.len() as u32,
        &sub_records,
        &over,
    );

    let mut bytes = valid_block(
        &[RawRecord::directory("SUB", 1, root_len as u32)],
        &root_names,
    );
    bytes.extend_from_slice(&sub_block);

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("the nested name table disagrees with its records");
    assert_eq!(error.code(), "name_table_length");
    assert_eq!(error.container(), CONTAINER);
    // The name table of the nested block starts at its offset plus the
    // header and the single record: 36 + 8 + 24.
    assert_eq!(
        error.offset(),
        (root_len + DIRECTORY_HEADER_BYTES + RECORD_BYTES) as u64
    );
    match &error {
        RofError::NameTableLength {
            declared,
            described,
            ..
        } => {
            assert_eq!(*declared, 4);
            assert_eq!(*described, 3);
        }
        other => panic!("expected a name-table failure, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);
}

/// **The `TREE_ENTRYPOINT` scope contract:** a structural failure raised
/// inside a nested block reports `rof.tree.directory.<field>` at that
/// block's absolute offset, while the same failure in the root block keeps
/// `rof.tree.<field>` — so the field alone says which block refused, at
/// any offset.
#[test]
fn accept_f05_b_nested_structural_failures_carry_the_directory_scope() {
    // The root block, then a nested block that declares one record and
    // then ends: its record table does not exist.
    let root_names = name_table(&["SUB"]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let mut bytes = valid_block(
        &[RawRecord::directory("SUB", 1, root_len as u32)],
        &root_names,
    );
    bytes.extend_from_slice(&1u32.to_le_bytes()); // entry_count
    bytes.extend_from_slice(&0u32.to_le_bytes()); // names_length

    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("a nested block without its record table must be refused");
    assert_eq!(error.code(), "parse");
    match &error {
        RofError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(error.field, "rof.tree.directory.records");
            assert_eq!(error.container, CONTAINER);
            assert_eq!(
                error.offset,
                (root_len + DIRECTORY_HEADER_BYTES) as u64,
                "the absolute offset of the missing table"
            );
        }
        other => panic!("expected a structural failure, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);

    // The root block's own structural failures keep the root scope, so
    // the two are told apart by the field alone.
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes[..DIRECTORY_HEADER_BYTES])
        .expect_err("a root header without its record table must be refused");
    match &error {
        RofError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(error.field, "rof.tree.records");
            assert_eq!(error.container, CONTAINER);
            assert_eq!(error.offset, DIRECTORY_HEADER_BYTES as u64);
        }
        other => panic!("expected a structural failure, got {other:?}"),
    }
    assert_eq!(context.allocation().used(), 0);
}

/// **Non-negotiable #5:** flags this reader cannot explain, an unobserved
/// flag combination and overlapping extents all surface
/// `UnsupportedLayout` instead of having a span extracted from them.
#[test]
fn accept_f05_b_unexplained_flags_and_overlaps_surface_unsupported_layout() {
    // 1. A bit with no observed meaning.
    let bytes = unknown_flag_member();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error =
        read_tree(&mut context, &bytes).expect_err("an unexplained flag bit must be refused");
    assert_eq!(error.code(), "unsupported_layout");
    assert_eq!(error.container(), CONTAINER);
    assert_eq!(
        error.offset(),
        DIRECTORY_HEADER_BYTES as u64,
        "the record's own offset"
    );
    assert!(
        error.to_string().contains("0x00000008"),
        "the refusal names the flags word: {error}"
    );
    assert_eq!(context.allocation().used(), 0);

    // 2. The directory+compressed combination: both bits observed, the
    //    combination not.
    let names = name_table(&["SUB"]);
    let records = [RawRecord {
        flags: FLAG_DIRECTORY | FLAG_COMPRESSED,
        ..RawRecord::directory("SUB", 1, 36)
    }];
    let mut bytes = valid_block(&records, &names);
    bytes.extend_from_slice(&[0u8; 8]);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes)
        .expect_err("a directory that is also compressed must be refused");
    assert_eq!(error.code(), "unsupported_layout");
    assert!(
        error.to_string().contains("directory+compressed"),
        "the refusal names the combination: {error}"
    );
    assert_eq!(context.allocation().used(), 0);

    // 3. Two members sharing bytes: no source documents the sharing, so
    //    neither span is extracted.
    let bytes = overlapping_members();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read_tree(&mut context, &bytes).expect_err("overlapping extents must be refused");
    assert_eq!(error.code(), "unsupported_layout");
    assert_eq!(
        error.offset(),
        220,
        "the second extent starts inside the first"
    );
    assert!(
        error.to_string().contains("overlap"),
        "the refusal names the overlap: {error}"
    );
    assert_eq!(context.allocation().used(), 0);
}

/// The allocation ledger: exactly what a successful walk books, nothing at
/// all for any refusal, and one reservation instead of one per block.
#[test]
fn accept_f05_b_bookings_and_refusals_leave_the_ledger_exact() {
    let fixture = tree();

    // What a successful walk books — computed from the returned tree, so
    // the expectation tracks the layout instead of a magic number: every
    // record table, both node arrays, and the path slice headers.
    let mut context = ParseContext::with_defaults(CONTAINER);
    let walked = read_tree(&mut context, &fixture.bytes).expect("the tree must traverse");
    let path_slices = walked
        .directories()
        .iter()
        .map(|directory| directory.path.len() as u64)
        .sum::<u64>()
        + walked
            .members()
            .iter()
            .map(|member| member.path.len() as u64)
            .sum::<u64>();
    let booked = (3 + 2 + 2) as u64 * RECORD_BYTES as u64
        + walked.directories().len() as u64 * std::mem::size_of::<RofTreeDirectory>() as u64
        + walked.members().len() as u64 * std::mem::size_of::<RofMember>() as u64
        + path_slices * std::mem::size_of::<&[u8]>() as u64;
    assert_eq!(
        context.allocation().used(),
        booked,
        "one reservation for the whole tree"
    );
    assert!(
        booked >= (3 + 2 + 2) as u64 * RECORD_BYTES as u64,
        "the record tables are booked at least, as read_directory books them"
    );

    // One byte less than the tree needs: refused before a record table
    // exists, and nothing is charged.
    let mut refused = ParseContext::new(CONTAINER, booked - 1, 8);
    let error = read_tree(&mut refused, &fixture.bytes)
        .expect_err("a tree one byte over budget must be refused");
    match &error {
        RofError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::AllocationBudgetExceeded);
            assert_eq!(error.field, "rof.tree.records");
            assert_eq!(error.container, CONTAINER);
            assert!(
                error.observed.contains(&booked.to_string()),
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

    // Exactly the booked amount: accepted, charged once.
    let mut accepted = ParseContext::new(CONTAINER, booked, 8);
    read_tree(&mut accepted, &fixture.bytes).expect("the tree fits exactly");
    assert_eq!(accepted.allocation().used(), booked);

    // Every refusal leaves the ledger exactly as it found it — on a fresh
    // context (nothing charged) and on one that has already booked a tree
    // (the earlier charge untouched).
    let refusals: [(Vec<u8>, &'static str); 4] = [
        (root_directory_pointing_at(0), "cycle"),
        (outside_file_member(), "extent_out_of_bounds"),
        (unknown_flag_member(), "unsupported_layout"),
        (overlapping_members(), "unsupported_layout"),
    ];
    for (bytes, code) in refusals {
        let mut fresh = ParseContext::with_defaults(CONTAINER);
        let error = read_tree(&mut fresh, &bytes).expect_err("the fixture must be refused");
        assert_eq!(error.code(), code, "{error}");
        assert_eq!(fresh.allocation().used(), 0, "{code}: nothing was booked");

        let error = read_tree(&mut context, &bytes).expect_err("still refused");
        assert_eq!(error.code(), code, "{error}");
        assert_eq!(context.allocation().used(), booked, "{code}: unchanged");
    }

    // A member read books nothing: its decoded buffer is the caller's and
    // is bounded per read instead of against the parse's budget.
    let member = walked.members()[0].clone();
    read_member(&context, &fixture.bytes, &member, &RofLimits::default())
        .expect("the member reads");
    let error = read_member(&context, &fixture.bytes, &member, &RofLimits::new(4))
        .expect_err("a four-byte ceiling refuses a twelve-byte member");
    assert_eq!(error.code(), "expansion_bomb");
    assert_eq!(
        context.allocation().used(),
        booked,
        "member reads book nothing"
    );
}
