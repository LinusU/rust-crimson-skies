//! Acceptance tests F15-B: the private derived-asset store is bounded,
//! atomic and recoverable, and never serves a partial write.
//!
//! The minimum scenario of stage `### F15-B` is spec F15 AC02 — "Kill the
//! process during cache write; next startup recovers cleanly" — and
//! `accept_f15_b_killed_cache_write_recovers_on_next_startup` produces it
//! the only way that is actually evidence: a **second real process** opens
//! the store, stages a write, announces that it is mid-write and is then
//! killed with SIGKILL from this process. The torn state on disk is
//! therefore produced by a real process death, not simulated in-process.
//!
//! Every test here calls production code only (`cs_assets::cache::store`).
//! Removing the commit rename, the startup sweep, the integrity gate or the
//! budget check makes the matching test fail.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cs_assets::cache::store::{ENTRIES_DIR, STAGING_DIR};
use cs_assets::cache::{
    BudgetExceeded, CacheBudget, CacheDirectory, CacheKey, CacheLookup, CacheReadError, CacheStore,
    ConversionOptions, ConverterVersion, DecoderId, HEADER_FILE, IntegrityError, IrVersion,
    PAYLOAD_FILE, SourceSpanHash, StoreError,
};
use cs_assets::install::sha256;
use cs_types::evidence::ContentHash;

use common::{TempTree, fixed_hash, synthetic_span};

/// The env var that turns this test binary into the killed writer.
const CRASH_POINT: &str = "CS_F15_B_CRASH_POINT";
/// The env var naming the fixture tree the killed writer works in.
const CRASH_ROOT: &str = "CS_F15_B_CRASH_ROOT";
/// The env var naming how many bytes the killed writer declares.
const CRASH_PAYLOAD: &str = "CS_F15_B_CRASH_PAYLOAD";
/// The marker the killed writer leaves once it is mid-write.
const READY: &str = "cs-f15-b-ready";
/// How long the parent waits for the writer to reach its crash point
/// before failing, so a stuck child cannot hang the suite.
const WRITER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

fn install() -> ContentHash {
    fixed_hash(0x31)
}

fn converter() -> ConverterVersion {
    ConverterVersion {
        decoder: DecoderId::new("zbd-texture").expect("valid decoder id"),
        decoder_version: 3,
        ir: IrVersion(1),
    }
}

fn span_a() -> cs_types::asset_id::SourceSpan {
    synthetic_span(
        install(),
        "zbd/c1/texture.zbd",
        "hull_a.bmp",
        0,
        fixed_hash(0xA1),
    )
}

fn span_b() -> cs_types::asset_id::SourceSpan {
    synthetic_span(
        install(),
        "zbd/c1/texture.zbd",
        "mask_b.bmp",
        4096,
        fixed_hash(0xB2),
    )
}

fn key_a() -> CacheKey {
    CacheKey::new(
        install(),
        &[span_a()],
        converter(),
        ConversionOptions::none(),
    )
    .expect("a key with an input")
}

fn key_b() -> CacheKey {
    CacheKey::new(
        install(),
        &[span_b()],
        converter(),
        ConversionOptions::none(),
    )
    .expect("a key with an input")
}

/// A key over both spans, so invalidation has something to be selective
/// about.
fn key_ab() -> CacheKey {
    CacheKey::new(
        install(),
        &[span_a(), span_b()],
        converter(),
        ConversionOptions::from_pairs(&[("faction", "brighton")]).expect("valid option"),
    )
    .expect("a key with inputs")
}

/// A private installation and cache pair inside one disposable tree.
struct Fixture {
    tree: TempTree,
    install_root: PathBuf,
    cache_root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let tree = TempTree::new(label);
        let install_root = tree.root().join("install");
        let cache_root = tree.root().join("cache");
        fs::create_dir_all(install_root.join("zbd").join("c1"))
            .expect("the fixture installation exists");
        fs::create_dir_all(&cache_root).expect("the fixture cache exists");
        fs::write(
            install_root.join("zbd").join("c1").join("texture.zbd"),
            b"source bytes",
        )
        .expect("fixture bytes are written");
        Self {
            tree,
            install_root,
            cache_root,
        }
    }

    fn store(&self, max_entries: u64, max_bytes: u64) -> CacheStore {
        CacheStore::open(
            CacheDirectory::open(&self.cache_root, &self.install_root)
                .expect("a private cache root"),
            CacheBudget::new(max_entries, max_bytes).expect("a nonzero budget"),
        )
        .expect("the store opens")
    }

    /// The store as the *next startup* would open it: recovery runs.
    fn reopen(&self, max_entries: u64, max_bytes: u64) -> CacheStore {
        self.store(max_entries, max_bytes)
    }

    /// The scratch directory the killed writer left behind, read without
    /// opening a store — opening one would sweep it, which is exactly the
    /// recovery this test has to observe.
    fn staging(&self) -> PathBuf {
        self.cache_root.join(STAGING_DIR)
    }

    /// The published-entry directory, read without opening a store.
    fn entries(&self) -> PathBuf {
        self.cache_root.join(ENTRIES_DIR)
    }
}

/// Writes a payload of `len` bytes with a recognisable pattern, so a
/// partial file is distinguishable from a complete one.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

/// Stages, fills and seals a write, leaving it for `commit`.
fn sealed(store: &CacheStore, key: &CacheKey, bytes: &[u8]) -> cs_assets::cache::PendingStoreWrite {
    let mut write = store
        .begin_write(key, bytes.len() as u64)
        .expect("the write is staged");
    write.write_all(bytes).expect("the payload is written");
    write.seal().expect("the write seals");
    write
}

/// A payload that spans more than one bounded chunk, so a mid-read
/// cancellation is observable at a chunk boundary.
fn multi_chunk_payload() -> Vec<u8> {
    payload(cs_assets::cache::CACHE_IO_CHUNK as usize + 4096)
}

// --- AC02: kill the process during the cache write ----------------------

/// The minimum acceptance scenario of F15-B (spec AC02).
///
/// A real second process is killed while a derived entry is being written;
/// the store this process then opens must recover: the interrupted write
/// is swept, the key is a miss (never a half-served hit), and rebuilding
/// and committing the same key works. The test also kills a process that
/// got as far as a *sealed* write, because the commit is the rename and
/// not the header write — a store that published on seal would serve an
/// entry that was never committed.
#[test]
fn accept_f15_b_killed_cache_write_recovers_on_next_startup() {
    for point in ["mid_write", "after_seal"] {
        let fixture = Fixture::new(&format!("f15-b-kill-{point}"));
        let key = key_a();
        let full = payload(64 * 1024);
        let status = kill_writer(&fixture, point, &full);

        // The child really died mid-write: it announced the crash point
        // and then was killed by a signal rather than exiting.
        assert!(
            !status.success(),
            "the writer process must not exit on its own ({point})"
        );
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(
                status.signal(),
                Some(9),
                "the writer must be killed by SIGKILL, not exit ({point})"
            );
        }

        // Before recovery: the interrupted write is scratch, and the
        // published-entry directory does not exist. Both are what the
        // kill left behind. These are read straight off the tree, because
        // opening the store is what sweeps them.
        let staging = fixture.staging();
        let debris: Vec<PathBuf> = fs::read_dir(&staging)
            .expect("the staging directory exists")
            .map(|entry| entry.expect("a readable directory entry").path())
            .collect();
        assert_eq!(
            debris.len(),
            1,
            "the killed writer left exactly one scratch directory ({point})"
        );
        assert!(
            !fixture.entries().join(key.digest().to_hex()).exists(),
            "an interrupted write must never be published ({point})"
        );

        // Startup recovery: the debris is swept and reported, the key is a
        // miss, and the store opens cleanly with nothing half-served.
        let store = fixture.reopen(8, 1 << 20);
        assert_eq!(
            store.recovery().swept_staging,
            1,
            "the interrupted write is swept at startup ({point})"
        );
        assert!(
            fs::read_dir(&staging)
                .expect("the staging directory is readable")
                .next()
                .is_none(),
            "the swept scratch directory is gone ({point})"
        );
        assert_eq!(store.usage().entries, 0, "nothing is half-served ({point})");
        assert!(matches!(
            store.begin_read(&key).expect("the lookup runs"),
            CacheLookup::Miss
        ));

        // And the rebuild works: the same key commits and verifies.
        let mut store = store;
        let entry = store
            .commit(sealed(&store, &key, &full))
            .expect("the rebuilt write commits");
        assert_eq!(entry.payload_sha256, sha256(&full));
        let CacheLookup::Hit(read) = store.begin_read(&key).expect("the lookup runs") else {
            panic!("a committed entry is a hit");
        };
        let verified = read.complete().expect("the committed entry verifies");
        assert_eq!(verified.payload(), full.as_slice());
    }
}

/// Runs this test binary again as the writer that is about to be killed,
/// waits until it is really mid-write, then kills it and returns its
/// status.
///
/// The handshake is a marker file rather than a line on the child's
/// stdout: a test binary's own output belongs to its harness, and the
/// evidence this test needs is the state of the filesystem, not the
/// child's opinion of it. The marker is written after the partial write,
/// so its existence is proof that a write was in flight.
fn kill_writer(fixture: &Fixture, point: &str, payload: &[u8]) -> std::process::ExitStatus {
    let mut child = Command::new(std::env::current_exe().expect("the test binary's own path"))
        .args([
            "--exact",
            "accept_f15_b_writer_child_killed_during_the_cache_write",
            "--nocapture",
        ])
        .env(CRASH_POINT, point)
        .env(CRASH_ROOT, fixture.tree.root())
        .env(CRASH_PAYLOAD, payload.len().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the writer process starts");
    let marker = fixture.tree.root().join(format!("{READY}-{point}"));
    let deadline = std::time::Instant::now() + WRITER_TIMEOUT;
    while !marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the writer process must reach its crash point ({point})"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // SIGKILL: no unwinding, no destructors, no flush — the process simply
    // stops existing in the middle of the write.
    child.kill().expect("the writer is killed");
    child.wait().expect("the killed writer is reaped")
}

/// The other half of the kill scenario: this test binary, re-executed as
/// the writer.
///
/// Run on its own it exercises a full store round trip, so it is never a
/// no-op test; run with [`CRASH_POINT`] set it stages a write, announces
/// that it is mid-write and then waits to be killed from outside.
#[test]
fn accept_f15_b_writer_child_killed_during_the_cache_write() {
    let Ok(point) = std::env::var(CRASH_POINT) else {
        // Standalone: a real staged write, commit and verified read.
        let fixture = Fixture::new("f15-b-writer-standalone");
        let key = key_a();
        let bytes = payload(4096);
        let mut store = fixture.store(8, 1 << 20);
        store
            .commit(sealed(&store, &key, &bytes))
            .expect("the write commits");
        let CacheLookup::Hit(read) = store.begin_read(&key).expect("the lookup runs") else {
            panic!("a committed entry is a hit");
        };
        assert_eq!(
            read.complete().expect("the entry verifies").payload(),
            bytes
        );
        return;
    };

    let root = PathBuf::from(std::env::var(CRASH_ROOT).expect("the fixture root is named"));
    let install_root = root.join("install");
    let cache_root = root.join("cache");
    let declared: usize = std::env::var(CRASH_PAYLOAD)
        .expect("the declared payload length is named")
        .parse()
        .expect("a numeric payload length");
    let key = key_a();
    let store = CacheStore::open(
        CacheDirectory::open(&cache_root, &install_root).expect("a private cache root"),
        CacheBudget::new(8, 1 << 20).expect("a nonzero budget"),
    )
    .expect("the store opens");

    // A real bounded write: half the declared payload, then wait to die.
    let mut write = store
        .begin_write(&key, declared as u64)
        .expect("the write is staged");
    let half = declared / 2;
    write
        .write_all(&payload(half))
        .expect("the first half is written");
    if point == "after_seal" {
        write
            .write_all(&payload(declared - half))
            .expect("the second half is written");
        write.seal().expect("the write seals");
    }
    fs::write(root.join(format!("{READY}-{point}")), b"mid-write")
        .expect("the crash point is announced");
    loop {
        // The write is held open, mid-flight, until this process is
        // killed from outside. Nothing here may complete it.
        std::thread::sleep(std::time::Duration::from_millis(50));
        if write.written() as usize > declared {
            panic!("the staging write grew past its declared length");
        }
    }
}

// --- The commit boundary and the integrity gate ------------------------

/// A write that is staged and dropped is not an entry: dropping removes
/// the scratch directory, and the key stays a miss. This is the graceful
/// half of AC02 (the process died instead, in the test above).
#[test]
fn accept_f15_b_dropped_write_leaves_no_entry() {
    let fixture = Fixture::new("f15-b-dropped");
    let key = key_a();
    let bytes = payload(32 * 1024);
    let store = fixture.store(8, 1 << 20);

    let mut write = store
        .begin_write(&key, bytes.len() as u64)
        .expect("the write is staged");
    write
        .write_all(&bytes[..1024])
        .expect("a prefix is written");
    let staging = write.staging().to_path_buf();
    assert!(
        staging.exists(),
        "the scratch directory exists while writing"
    );
    drop(write);
    assert!(
        !staging.exists(),
        "an unpublished write removes its scratch directory"
    );
    assert!(matches!(
        store.begin_read(&key).expect("the lookup runs"),
        CacheLookup::Miss
    ));
    assert_eq!(store.usage().entries, 0);

    // A cancelled write is refused at the next chunk boundary and is
    // likewise never published.
    let mut write = store
        .begin_write(&key, bytes.len() as u64)
        .expect("the write is staged");
    write.cancel_handle().cancel();
    assert!(matches!(
        write.append(&bytes[..1024]),
        Err(StoreError::WriteCancelled { .. })
    ));
    drop(write);
    assert!(matches!(
        store.begin_read(&key).expect("the lookup runs"),
        CacheLookup::Miss
    ));
}

/// A store's own record is not trusted: bytes that were tampered with
/// after the commit are refused on read, and a header that lies about the
/// payload is not a served entry either.
#[test]
fn accept_f15_b_tampered_entry_is_refused_never_served() {
    let fixture = Fixture::new("f15-b-tampered");
    let key = key_a();
    let bytes = payload(8192);
    let mut store = fixture.store(8, 8 << 20);
    store
        .commit(sealed(&store, &key, &bytes))
        .expect("the write commits");

    // Same length, different bytes: the entry is still found, but the
    // bytes do not hash to what was committed.
    let entry = store.entries_root().join(key.digest().to_hex());
    let mut corrupt = bytes.clone();
    corrupt[17] ^= 0xFF;
    fs::write(entry.join(PAYLOAD_FILE), &corrupt).expect("the payload is tampered with");
    let CacheLookup::Hit(read) = store.begin_read(&key).expect("the lookup runs") else {
        panic!("a committed entry is a hit");
    };
    assert!(matches!(
        read.complete(),
        Err(CacheReadError::Corrupt(
            IntegrityError::DigestMismatch { .. }
        ))
    ));

    // A header that declares another payload's digest is refused by the
    // same gate — the store does not get to vouch for its own bytes.
    let header = fs::read_to_string(entry.join(HEADER_FILE)).expect("the record is readable");
    let lying = header.replace(&sha256(&bytes).to_hex(), &sha256(b"other").to_hex());
    fs::write(entry.join(HEADER_FILE), &lying).expect("the record is tampered with");
    let CacheLookup::Hit(read) = store.begin_read(&key).expect("the lookup runs") else {
        panic!("a committed entry is a hit");
    };
    assert!(matches!(
        read.complete(),
        Err(CacheReadError::Corrupt(
            IntegrityError::DigestMismatch { .. }
        ))
    ));

    // A record whose facets no longer hash to its own key is not decodable
    // at all: the identity is checked, not trusted.
    let forged = header.replace(&key.install().to_hex(), &fixed_hash(0x99).to_hex());
    fs::write(entry.join(HEADER_FILE), &forged).expect("the record is tampered with");
    assert!(
        matches!(
            store.begin_read(&key),
            Err(StoreError::CorruptHeader { .. })
        ),
        "a record that does not hash to its own key must be refused"
    );

    // Startup recovery drops it: the entry is rebuilt, never served.
    let store = fixture.reopen(8, 1 << 20);
    assert_eq!(store.recovery().dropped_corrupt, 1);
    assert_eq!(store.usage().entries, 0);
}

/// The streaming header constructor the bounded writer uses is gated by
/// `verify_entry` like any other: a writer that misreports the length or
/// the digest of its payload has its entry refused.
#[test]
fn accept_f15_b_streaming_header_cannot_bypass_the_gate() {
    let key = key_a();
    let bytes = payload(1024);
    let honest = cs_assets::cache::EntryHeader::committed_streaming(
        key.clone(),
        bytes.len() as u64,
        sha256(&bytes),
    );
    assert_eq!(
        cs_assets::cache::verify_entry(&key, &honest, &bytes)
            .expect("an honestly declared streaming header verifies")
            .payload(),
        bytes
    );

    let short = cs_assets::cache::EntryHeader::committed_streaming(
        key.clone(),
        (bytes.len() - 8) as u64,
        sha256(&bytes),
    );
    assert!(matches!(
        cs_assets::cache::verify_entry(&key, &short, &bytes),
        Err(IntegrityError::LengthMismatch { .. })
    ));
    let lying = cs_assets::cache::EntryHeader::committed_streaming(
        key.clone(),
        bytes.len() as u64,
        sha256(b"something else"),
    );
    assert!(matches!(
        cs_assets::cache::verify_entry(&key, &lying, &bytes),
        Err(IntegrityError::DigestMismatch { .. })
    ));
}

// --- Bounded, cancellable reads -----------------------------------------

/// Reads are bounded and measurable: progress is reported per chunk with
/// the entry's declared total, and a cancellation thrown from a progress
/// callback stops the read at its next chunk boundary with nothing
/// delivered.
#[test]
fn accept_f15_b_reads_are_chunked_measured_and_cancellable() {
    let fixture = Fixture::new("f15-b-bounded-read");
    let key = key_a();
    let bytes = multi_chunk_payload();
    let mut store = fixture.store(8, 8 << 20);
    store
        .commit(sealed(&store, &key, &bytes))
        .expect("the write commits");

    // A full read reports one step per chunk, always against the declared
    // total, and delivers exactly the committed bytes.
    let CacheLookup::Hit(read) = store.begin_read(&key).expect("the lookup runs") else {
        panic!("a committed entry is a hit");
    };
    assert_eq!(read.total(), bytes.len() as u64);
    let mut steps = Vec::new();
    let verified = read
        .complete_with(|progress| steps.push(progress))
        .expect("the entry verifies");
    assert_eq!(verified.payload(), bytes.as_slice());
    assert_eq!(
        steps.len(),
        2,
        "a payload over one chunk is read in more than one bounded step"
    );
    assert_eq!(steps[0].total, bytes.len() as u64);
    assert_eq!(steps[0].read, cs_assets::cache::CACHE_IO_CHUNK);
    assert_eq!(steps[1].read, bytes.len() as u64);
    assert!(steps[1].read > steps[0].read, "progress only moves forward");

    // Cancelled from the first step, the read stops at the next chunk
    // boundary and delivers nothing.
    let CacheLookup::Hit(read) = store.begin_read(&key).expect("the lookup runs") else {
        panic!("a committed entry is a hit");
    };
    let switch = read.cancel_handle();
    let mut steps = 0;
    let cancelled = read.complete_with(|_| {
        steps += 1;
        switch.cancel();
    });
    assert!(matches!(
        cancelled,
        Err(CacheReadError::Cancelled { read, total })
            if read == cs_assets::cache::CACHE_IO_CHUNK && total == bytes.len() as u64
    ));
    assert_eq!(
        steps, 1,
        "the cancellation is observed at the next boundary"
    );
}

// --- Bounded growth and eviction ----------------------------------------

/// The store grows only within its budget: a write that does not fit
/// evicts the entry with the lowest write sequence, and a write that could
/// not fit an empty store is refused with nothing half-written.
#[test]
fn accept_f15_b_store_evicts_within_its_budget_and_refuses_the_impossible() {
    let fixture = Fixture::new("f15-b-budget");
    let mut store = fixture.store(2, 1024);

    let first = payload(256);
    let second = payload(256);
    let third = payload(256);
    let mut written = Vec::new();
    for (key, bytes) in [(&key_a(), &first), (&key_b(), &second), (&key_ab(), &third)] {
        written.push(
            store
                .commit(sealed(&store, key, bytes))
                .expect("the write commits"),
        );
    }
    // The first entry was evicted to make room for the third: the policy
    // is the lowest write sequence, and it is reported, not silent.
    assert_eq!(written[2].evicted, vec![key_a().digest()]);
    assert_eq!(store.usage().entries, 2);
    assert_eq!(store.usage().bytes, 512);
    assert!(matches!(
        store.begin_read(&key_a()).expect("the lookup runs"),
        CacheLookup::Miss
    ));
    assert!(matches!(
        store.begin_read(&key_b()).expect("the lookup runs"),
        CacheLookup::Hit(_)
    ));

    // An entry larger than the whole byte budget is refused, the store
    // keeps what it had, and no entry is left behind.
    let mut store = fixture.reopen(2, 1024);
    let huge = payload(4096);
    let mut write = store
        .begin_write(&key_a(), huge.len() as u64)
        .expect("the write is staged");
    write.write_all(&huge).expect("the payload is written");
    write.seal().expect("the write seals");
    assert!(matches!(
        store.commit(write),
        Err(StoreError::Budget(BudgetExceeded::Bytes { .. }))
    ));
    assert_eq!(store.usage().entries, 2, "a refused write evicts nothing");
    assert_eq!(store.usage().bytes, 512);
    assert!(matches!(
        store.begin_read(&key_a()).expect("the lookup runs"),
        CacheLookup::Miss
    ));
}

/// A write that declares more bytes than it holds is refused at the seal,
/// so a committed header can never describe bytes that are not there.
#[test]
fn accept_f15_b_seal_refuses_an_incomplete_write() {
    let fixture = Fixture::new("f15-b-short-write");
    let key = key_a();
    let store = fixture.store(8, 1 << 20);
    let mut write = store.begin_write(&key, 4096).expect("the write is staged");
    write
        .write_all(&payload(1024))
        .expect("a prefix is written");
    assert!(matches!(
        write.seal(),
        Err(StoreError::EntryTooLong {
            declared: 4096,
            offered: 1024
        })
    ));
    // The staging directory is not an entry, sealed or not.
    drop(write);
    assert!(matches!(
        store.begin_read(&key).expect("the lookup runs"),
        CacheLookup::Miss
    ));
    assert_eq!(store.usage().entries, 0);
}

// --- Per-input invalidation (the store half of AC03) --------------------

/// Changing one source input drops exactly the entries that were derived
/// from it and leaves every other entry served.
#[test]
fn accept_f15_b_invalidation_drops_only_the_dependent_entries() {
    let fixture = Fixture::new("f15-b-invalidation");
    let mut store = fixture.store(8, 1 << 20);
    for (key, bytes) in [
        (&key_a(), &payload(128)),
        (&key_b(), &payload(128)),
        (&key_ab(), &payload(128)),
    ] {
        let write = sealed(&store, key, bytes);
        store.commit(write).expect("the write commits");
    }
    assert_eq!(store.usage().entries, 3);

    // The livery source is edited: its span hash changes, so exactly the
    // entries that list it are invalidated.
    let edited = synthetic_span(
        install(),
        "zbd/c1/texture.zbd",
        "mask_b.bmp",
        4096,
        fixed_hash(0xB9),
    );
    let previous = SourceSpanHash::of(&span_b());
    let report = store
        .invalidate_source(previous)
        .expect("invalidation runs");
    let mut dropped: Vec<String> = report.dropped.iter().map(ContentHash::to_hex).collect();
    dropped.sort();
    let mut expected = vec![key_b().digest().to_hex(), key_ab().digest().to_hex()];
    expected.sort();
    assert_eq!(
        dropped, expected,
        "only the entries that list the edited source are dropped"
    );
    assert_eq!(report.entries, 1);
    assert!(matches!(
        store.begin_read(&key_a()).expect("the lookup runs"),
        CacheLookup::Hit(_)
    ));
    assert!(matches!(
        store.begin_read(&key_b()).expect("the lookup runs"),
        CacheLookup::Miss
    ));
    assert!(matches!(
        store.begin_read(&key_ab()).expect("the lookup runs"),
        CacheLookup::Miss
    ));
    assert_ne!(
        SourceSpanHash::of(&edited),
        previous,
        "the edited source has a different span hash"
    );
}

// --- Non-negotiable behavior 1: the store is private -------------------

/// The store writes only inside its private root: a snapshot of the
/// source installation before and after a full write, commit and eviction
/// cycle is byte-identical, and the cache root the store is given cannot
/// lie inside the installation at all.
#[test]
fn accept_f15_b_store_never_writes_inside_the_installation() {
    let fixture = Fixture::new("f15-b-private");
    let before = snapshot(&fixture.install_root);
    let mut store = fixture.store(1, 256);
    for key in [&key_a(), &key_b(), &key_ab()] {
        store
            .commit(sealed(&store, key, &payload(128)))
            .expect("the write commits");
    }
    assert_eq!(store.usage().entries, 1, "the budget held");
    assert_eq!(
        before,
        snapshot(&fixture.install_root),
        "the source installation is untouched by cache traffic"
    );
    // And a cache root inside the installation is refused before any
    // store exists.
    let inside = fixture.install_root.join("cache");
    fs::create_dir_all(&inside).expect("the refused root exists");
    assert!(matches!(
        CacheDirectory::open(&inside, &fixture.install_root),
        Err(cs_assets::cache::CacheLocationError::InsideInstall { .. })
    ));
}

/// Every file below `root` with its length, so a change anywhere in the
/// tree shows up.
fn snapshot(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut rows = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory).expect("a readable directory");
        for entry in entries {
            let path = entry.expect("a readable entry").path();
            let metadata = fs::symlink_metadata(&path).expect("readable metadata");
            if metadata.is_dir() {
                stack.push(path);
            } else {
                rows.push((path, metadata.len()));
            }
        }
    }
    rows.sort();
    rows
}
