//! Acceptance tests F15-B: the production path from a load transaction to
//! the private cache — bounded reads, atomic writes, and the integrity
//! re-verification before a world may go interactive.
//!
//! These tests drive `cs_app::loading::LoadDriver` over a real
//! `cs_assets::cache::CacheStore` on a real filesystem, with a real
//! content-session generation. They cover the half of spec F15 AC04 that
//! needs no original data — warm and cold loads delivering the same
//! content hashes — and the load-level halves of behaviors 1, 2, 3 and 5:
//! a cache that cannot hold or even be read an entry does not fail a
//! load, a cancelled read stops the load instead of serving it, an entry
//! that changed under the load fails the load rather than making a world
//! interactive, and a failure names its dependency and recovery path.

mod common;

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

use cs_app::assets::CanonicalPayload;
use cs_app::loading::{
    Criticality, DriverError, HandoffError, ItemRead, LoadDriver, LoadItem, LoadRequest, LoadState,
    LoadTarget, LoadTransaction, RebuildCause, RecoveryPath, SourceFault, StepProgress,
    UncachedReason,
};
use cs_assets::cache::store::ENTRIES_DIR;
use cs_assets::cache::{
    BudgetExceeded, CacheBudget, CacheDirectory, CacheKey, CacheLookup, CacheStore,
    ConversionOptions, ConverterVersion, DecoderId, HEADER_FILE, IrVersion, PAYLOAD_FILE,
    StoreError,
};
use cs_assets::vfs::{ReadError, SessionBuilder};
use cs_types::asset_id::WorldGroup;
use cs_types::content::ContentKind;
use cs_types::evidence::ContentHash;

use common::{TempTree, fixed_hash, synthetic_content, synthetic_context, synthetic_key};

fn install() -> ContentHash {
    fixed_hash(0x5A)
}

fn converter() -> ConverterVersion {
    ConverterVersion {
        decoder: DecoderId::new("zbd-texture").expect("valid decoder id"),
        decoder_version: 3,
        ir: IrVersion(1),
    }
}

fn span_a() -> cs_types::asset_id::SourceSpan {
    common::synthetic_span(
        install(),
        "zbd/c1/texture.zbd",
        "hull_a.bmp",
        0,
        fixed_hash(0xA1),
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

/// The canonical source bytes and the derived form the fake converter
/// produces from them: a pure function of its input, so a warm and a cold
/// load deliver identical bytes.
fn source_bytes() -> Vec<u8> {
    b"canonical source bytes of the fixture".to_vec()
}

fn convert(payload: &CanonicalPayload) -> Vec<u8> {
    let mut derived = payload.bytes().to_vec();
    derived.extend_from_slice(b"::derived");
    derived
}

fn derived_of_source() -> Vec<u8> {
    convert(&CanonicalPayload::new(ContentKind::Image, source_bytes()))
}

/// One load's world: the private store, a content session, and a request
/// with one cached gameplay-critical item and one deferred item that is
/// used without a derived entry.
struct World {
    /// Held so the fixture tree lives exactly as long as the test.
    _tree: TempTree,
    install_root: PathBuf,
    cache_root: PathBuf,
    session: cs_assets::vfs::ContentSession,
}

impl World {
    fn new(label: &str) -> Self {
        let tree = TempTree::new(label);
        let install_root = tree.root().join("install");
        let cache_root = tree.root().join("cache");
        fs::create_dir_all(install_root.join("zbd").join("c1"))
            .expect("the fixture installation exists");
        fs::create_dir_all(&cache_root).expect("the fixture cache exists");
        fs::write(
            install_root.join("zbd").join("c1").join("texture.zbd"),
            source_bytes(),
        )
        .expect("the fixture source is written");
        let session = SessionBuilder::new(synthetic_context(install(), "zbd/c1")).open();
        Self {
            _tree: tree,
            install_root,
            cache_root,
            session,
        }
    }

    fn store(&self, max_entries: u64, max_bytes: u64) -> CacheStore {
        CacheStore::open(
            CacheDirectory::open(&self.cache_root, &self.install_root)
                .expect("a cache root outside the installation"),
            CacheBudget::new(max_entries, max_bytes).expect("a nonzero budget"),
        )
        .expect("the store opens")
    }

    /// Publishes `derived` under `key` directly, so a test can arrange a
    /// warm store without running a load first.
    fn warm(&self, key: &CacheKey, derived: &[u8], max_bytes: u64) -> CacheStore {
        let mut store = self.store(8, max_bytes);
        let mut write = store
            .begin_write(key, derived.len() as u64)
            .expect("the write is staged");
        write.write_all(derived).expect("the payload is written");
        write.seal().expect("the write seals");
        store.commit(write).expect("the write commits");
        store
    }

    /// A fresh transaction, as a world switch would issue: a new serial
    /// for the same session generation.
    fn request(&self, name: &str, key: &CacheKey) -> LoadTransaction {
        LoadTransaction::issue(LoadRequest {
            session: self.session.generation(),
            target: LoadTarget::world(WorldGroup::new("zbd/c1").expect("valid world spelling")),
            items: vec![
                LoadItem::new(
                    synthetic_key("world", "texture.zbd"),
                    synthetic_content(ContentKind::Image, name),
                    Criticality::GameplayCritical,
                    4096,
                )
                .expect("nonzero work units")
                .with_derived(key.clone()),
                LoadItem::new(
                    synthetic_key("world", "hud.dss"),
                    synthetic_content(ContentKind::Image, "hud-deferred"),
                    Criticality::Deferred,
                    512,
                )
                .expect("nonzero work units"),
            ],
        })
    }

    /// The stored payload of one entry, for tampering.
    fn payload_of(&self, key: &CacheKey) -> PathBuf {
        self.cache_root
            .join(ENTRIES_DIR)
            .join(key.digest().to_hex())
            .join(PAYLOAD_FILE)
    }

    /// The entry directory of one key.
    fn entry_of(&self, key: &CacheKey) -> PathBuf {
        self.cache_root
            .join(ENTRIES_DIR)
            .join(key.digest().to_hex())
    }
}

/// Reads and converts the fixture's source item, counting the source read
/// so a test can prove a warm load never touched it.
fn from_source<'a>(
    reads: &'a Cell<u32>,
) -> impl FnOnce() -> Result<Vec<u8>, SourceFault> + use<'a> {
    move || {
        reads.set(reads.get() + 1);
        Ok(source_bytes())
    }
}

/// A source read that must never happen, with the reason in the panic.
fn no_source() -> impl FnOnce() -> Result<Vec<u8>, SourceFault> {
    || panic!("a warm load must not read the source")
}

/// A conversion that must never happen, with the reason in the panic.
fn no_convert() -> impl FnOnce(&CanonicalPayload) -> Result<Vec<u8>, cs_app::assets::ConversionError>
{
    |_| panic!("a warm load must not convert again")
}

fn convert_default(payload: &CanonicalPayload) -> Result<Vec<u8>, cs_app::assets::ConversionError> {
    Ok(convert(payload))
}

fn no_progress() -> impl FnMut(StepProgress) {
    |_step| {}
}

/// Warms and cold loads deliver the same content: the closure hash of a
/// load served from the store equals the closure hash of the load that
/// built every entry itself (spec F15 AC04, the half that needs no
/// original data), and the warm load neither reads the source nor
/// converts again.
#[test]
fn accept_f15_b_warm_and_cold_loads_deliver_equal_content() {
    let world = World::new("f15-b-warm-cold");
    let key = key_a();
    let reads = Cell::new(0u32);

    // Cold: nothing stored, so the source is read, converted, and the
    // derived form is committed atomically.
    let mut cold = LoadDriver::new(world.request("hull-a", &key), world.store(8, 8 << 20));
    cold.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let cold_read = cold
        .load_item(0, &mut progress, from_source(&reads), convert_default)
        .expect("the item is settled");
    assert!(
        matches!(cold_read, ItemRead::Rebuilt { .. }),
        "{cold_read:?}"
    );
    assert_eq!(reads.get(), 1, "a cold load reads its source once");
    cold.load_item(1, &mut progress, from_source(&reads), convert_default)
        .expect("the deferred item is settled");
    cold.validate_delivered().expect("the cold load validates");
    assert!(cold.transaction().is_world_interactive());

    // Warm: a new transaction over a new handle on the same store, with
    // sources that would fail the test if they were read at all.
    let warm_store = world.store(8, 8 << 20);
    assert_eq!(warm_store.usage().entries, 1, "the derived form is cached");
    let mut warm = LoadDriver::new(world.request("hull-a", &key), warm_store);
    warm.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let warm_read = warm
        .load_item(0, &mut progress, no_source(), no_convert())
        .expect("the item is settled");
    assert!(warm_read.is_cache_hit(), "{warm_read:?}");
    assert_eq!(
        warm_read.payload_sha256(),
        cold_read.payload_sha256(),
        "the warm and cold loads deliver the same derived bytes"
    );
    warm.load_item(
        1,
        &mut progress,
        from_source(&Cell::new(0)),
        convert_default,
    )
    .expect("the deferred item is settled");
    warm.validate_delivered().expect("the warm load validates");
    assert!(warm.transaction().is_world_interactive());
    assert_eq!(
        cold.transaction()
            .ready_bundle()
            .expect("a ready bundle")
            .closure_hash(),
        warm.transaction()
            .ready_bundle()
            .expect("a ready bundle")
            .closure_hash(),
        "cache warmth must not change what a load delivers"
    );
}

/// A stored entry that no longer hashes to what the load delivered fails
/// the load at validation, with the item and the recovery path named: a
/// corrupt cache cannot make a world interactive (behavior 3).
#[test]
fn accept_f15_b_validation_refuses_a_cache_entry_that_changed() {
    let world = World::new("f15-b-validation-refusal");
    let key = key_a();
    let store = world.warm(&key, &derived_of_source(), 8 << 20);

    // A warm load delivers from the store, and the store's bytes are
    // replaced with other bytes of the same length while the load runs.
    let mut driver = LoadDriver::new(world.request("hull-a", &key), store);
    driver.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let read = driver
        .load_item(0, &mut progress, no_source(), no_convert())
        .expect("the item is settled");
    assert!(read.is_cache_hit(), "{read:?}");
    driver
        .load_item(
            1,
            &mut progress,
            from_source(&Cell::new(0)),
            convert_default,
        )
        .expect("the deferred item is settled");
    let mut corrupt = fs::read(world.payload_of(&key)).expect("the stored payload is readable");
    corrupt[7] ^= 0xFF;
    fs::write(world.payload_of(&key), &corrupt).expect("the payload is tampered with");

    let error = driver
        .validate_delivered()
        .expect_err("a changed entry must be refused");
    let DriverError::Integrity { index, state, .. } = error else {
        panic!("{error}");
    };
    assert_eq!(index, 0, "the refusal names the offending item");
    assert_eq!(state, LoadState::Failed);
    assert_eq!(
        driver.transaction().state(),
        LoadState::Failed,
        "cache corruption fails the load instead of the world going interactive"
    );
    let failure = driver
        .transaction()
        .failures()
        .last()
        .expect("the refusal is on the record");
    assert_eq!(failure.code, "cache_integrity");
    assert_eq!(failure.recovery, RecoveryPath::RebuildDerived);
    assert!(matches!(
        driver.transaction().ready_bundle(),
        Err(HandoffError::NotReady {
            state: LoadState::Failed
        })
    ));
    assert!(!driver.transaction().is_world_interactive());
}

/// A stored entry the bounded read cannot verify is dropped and rebuilt
/// from the sources in the same load — refused, never served (behavior 3).
#[test]
fn accept_f15_b_refused_entry_is_rebuilt_not_served() {
    let world = World::new("f15-b-refused-rebuild");
    let key = key_a();
    let store = world.warm(&key, &derived_of_source(), 8 << 20);
    let mut corrupt = fs::read(world.payload_of(&key)).expect("the stored payload is readable");
    corrupt[3] ^= 0xFF;
    fs::write(world.payload_of(&key), &corrupt).expect("the payload is tampered with");

    let reads = Cell::new(0u32);
    let mut driver = LoadDriver::new(world.request("hull-a", &key), store);
    driver.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let read = driver
        .load_item(0, &mut progress, from_source(&reads), convert_default)
        .expect("the item is settled");
    let ItemRead::Rebuilt {
        payload_sha256,
        cause,
    } = &read
    else {
        panic!("{read:?}");
    };
    assert_eq!(
        *cause,
        RebuildCause::Refused {
            code: "digest_mismatch",
            detail: cause_detail(cause),
        },
        "the refusal is named, not swallowed"
    );
    assert_eq!(
        reads.get(),
        1,
        "the refused entry is rebuilt from the source"
    );
    assert_eq!(
        *payload_sha256,
        cs_assets::install::sha256(&derived_of_source()),
        "the load delivers the bytes it just built"
    );
    assert_eq!(
        fs::read(world.payload_of(&key)).expect("the replacement is stored"),
        derived_of_source(),
        "the rebuild replaced the corrupt entry"
    );
}

/// The detail half of a [`RebuildCause::Refused`], whatever it is.
fn cause_detail(cause: &RebuildCause) -> String {
    match cause {
        RebuildCause::Refused { detail, .. } => detail.clone(),
        other => panic!("{other:?}"),
    }
}

/// A bounded read that is cancelled stops the load: nothing is delivered,
/// the transaction is cancelled rather than failed, and no ready bundle
/// exists for a world to attach (behavior 2).
#[test]
fn accept_f15_b_cancelled_cache_read_stops_the_load() {
    let world = World::new("f15-b-cancelled");
    let key = key_a();
    // Three bounded chunks: the switch is thrown while the first chunk is
    // being reported, the driver passes it to the read, and the read stops
    // at the boundary after the next one — never at the end of the last
    // chunk, which would have delivered the whole payload.
    let derived = vec![7u8; 3 * cs_assets::cache::CACHE_IO_CHUNK as usize + 4096];
    let store = world.warm(&key, &derived, 8 << 20);

    let mut driver = LoadDriver::new(world.request("hull-a", &key), store);
    driver.transaction_mut().begin().expect("the load begins");
    let switch = driver.cancel_handle();
    let steps = Cell::new(0u32);
    let mut progress = |_step: StepProgress| {
        steps.set(steps.get() + 1);
        // In the real world this is another thread; from the progress
        // callback it is the same observation point.
        switch.cancel();
    };
    let read = driver
        .load_item(
            0,
            &mut progress,
            || panic!("a cancelled read must not fall back to the source"),
            no_convert(),
        )
        .expect("the item is settled");
    assert!(matches!(read, ItemRead::Failed { .. }), "{read:?}");
    assert_eq!(
        steps.get(),
        2,
        "the read stops at the chunk boundary after the one in flight"
    );
    assert!(
        driver.is_cancelled(),
        "the driver's own switch recorded the cancellation"
    );
    assert_eq!(
        driver.transaction().state(),
        LoadState::Cancelled,
        "a cancelled read cancels the load; it does not fail it"
    );
    assert!(matches!(
        driver.transaction().ready_bundle(),
        Err(HandoffError::NotReady {
            state: LoadState::Cancelled
        })
    ));
}

/// A cache that cannot hold the entry, or cannot be read at all, does not
/// fail the load: the derived bytes are delivered and the reason is
/// reported (behavior 1 — the cache is an optimization, never the
/// authoritative data source).
#[test]
fn accept_f15_b_uncacheable_item_is_delivered_not_failed() {
    let world = World::new("f15-b-uncached");
    let key = key_a();
    // One byte of budget: the entry cannot be admitted even into an empty
    // store, and the store keeps what it had.
    let mut driver = LoadDriver::new(world.request("hull-a", &key), world.store(8, 1));
    driver.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let read = driver
        .load_item(
            0,
            &mut progress,
            from_source(&Cell::new(0)),
            convert_default,
        )
        .expect("the item is settled");
    let ItemRead::Uncached {
        payload_sha256,
        cause,
        reason,
    } = &read
    else {
        panic!("{read:?}");
    };
    assert_eq!(*cause, RebuildCause::NoEntry);
    assert!(matches!(
        reason,
        UncachedReason::Budget(BudgetExceeded::Bytes { .. })
    ));
    assert_eq!(
        *payload_sha256,
        cs_assets::install::sha256(&derived_of_source()),
        "the delivered bytes are the ones the load just built"
    );
    assert_eq!(
        driver.store().usage().entries,
        0,
        "a refused write evicts nothing and caches nothing"
    );

    // The second item declares no derived key: it is used without a stored
    // entry and is delivered the same way.
    let read = driver
        .load_item(
            1,
            &mut progress,
            from_source(&Cell::new(0)),
            convert_default,
        )
        .expect("the item is settled");
    assert!(
        matches!(
            read,
            ItemRead::Rebuilt {
                cause: RebuildCause::NotCacheable,
                ..
            }
        ),
        "{read:?}"
    );

    driver.validate_delivered().expect("the load validates");
    assert!(driver.transaction().is_world_interactive());
}

/// A store that cannot be read is a fault of the cache, not of the
/// content: the item still loads from its sources, the reason is on the
/// record, and recovery leaves what the store did not write alone.
#[test]
fn accept_f15_b_unreadable_cache_still_loads_from_the_source() {
    let world = World::new("f15-b-unreadable");
    let key = key_a();
    // A file where an entry directory belongs: the record cannot be read.
    let entry = world.entry_of(&key);
    fs::create_dir_all(entry.parent().expect("entries has a parent"))
        .expect("the entries directory exists");
    fs::write(&entry, b"not a directory").expect("the obstacle is written");

    let mut driver = LoadDriver::new(world.request("hull-a", &key), world.store(8, 8 << 20));
    driver.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let reads = Cell::new(0u32);
    let read = driver
        .load_item(0, &mut progress, from_source(&reads), convert_default)
        .expect("the item is settled");
    let ItemRead::Uncached { cause, reason, .. } = &read else {
        panic!("{read:?}");
    };
    assert!(
        matches!(cause, RebuildCause::StoreUnavailable { .. }),
        "{cause:?}"
    );
    assert!(matches!(
        reason,
        UncachedReason::Store {
            code: "cache_io",
            ..
        }
    ));
    assert_eq!(reads.get(), 1, "the item came from its sources");
    driver
        .load_item(1, &mut progress, from_source(&reads), convert_default)
        .expect("the deferred item is settled");
    driver.validate_delivered().expect("the load validates");
    assert!(
        driver.transaction().is_world_interactive(),
        "an unreadable cache never fails a load that can read its sources"
    );
    assert!(matches!(
        driver.store().begin_read(&key),
        Err(StoreError::Io { .. })
    ));
    // The obstacle is not the store's own bookkeeping, so recovery counts
    // it and leaves it: a private cache never deletes what it did not
    // write.
    assert_eq!(world.store(8, 8 << 20).recovery().kept_unknown, 1);
    assert!(fs::metadata(&entry).is_ok(), "the obstacle is still there");
}

/// A store whose read side reports a fault is still written to: the item
/// is rebuilt from its sources, the derived form is published, and the
/// report says what really happened — rebuilt, with the read fault named
/// as the cause, not "not cached" for an entry the store now holds.
#[test]
fn accept_f15_b_rebuild_replaces_an_entry_the_store_could_not_read() {
    let world = World::new("f15-b-unreadable-rebuilt");
    let key = key_a();
    let store = world.warm(&key, &derived_of_source(), 8 << 20);
    // The record is not decodable, which the store reports as a fault of
    // its own bookkeeping rather than as a refusal of these bytes.
    fs::write(
        world.entry_of(&key).join(HEADER_FILE),
        b"not a cache record\n",
    )
    .expect("the record is destroyed");

    let mut driver = LoadDriver::new(world.request("hull-a", &key), store);
    driver.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let reads = Cell::new(0u32);
    let read = driver
        .load_item(0, &mut progress, from_source(&reads), convert_default)
        .expect("the item is settled");
    let ItemRead::Rebuilt {
        payload_sha256,
        cause,
    } = &read
    else {
        panic!("the rebuild was published, so it is not `Uncached`: {read:?}");
    };
    assert_eq!(reads.get(), 1, "the item came from its sources");
    assert!(
        matches!(
            cause,
            RebuildCause::StoreUnavailable {
                code: "corrupt_header",
                ..
            }
        ),
        "the read fault is named: {cause:?}"
    );
    assert_eq!(
        *payload_sha256,
        cs_assets::install::sha256(&derived_of_source())
    );
    // The rebuilt entry is the one the store serves from now on.
    assert!(
        matches!(driver.store().begin_read(&key), Ok(CacheLookup::Hit(_))),
        "the publish replaced the record the store could not read"
    );
    driver
        .load_item(1, &mut progress, from_source(&reads), convert_default)
        .expect("the deferred item is settled");
    driver.validate_delivered().expect("the load validates");
    assert!(driver.transaction().is_world_interactive());
}

/// A gameplay-critical failure ends the load and names the missing
/// dependency; a deferred failure is recorded and reported while the load
/// finishes, with the failed item left out of the bundle rather than
/// hidden (behaviors 4 and 5).
#[test]
fn accept_f15_b_failures_name_the_dependency_and_the_recovery_path() {
    let world = World::new("f15-b-failures");
    let key = key_a();

    // The gameplay-critical item's source cannot be read: the load fails
    // and says why, so the world never goes interactive.
    let mut critical = LoadDriver::new(world.request("hull-a", &key), world.store(8, 8 << 20));
    critical.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    let read = critical
        .load_item(
            0,
            &mut progress,
            || {
                Err(ReadError::NoBacking {
                    mount: "world-0".to_owned(),
                }
                .into())
            },
            no_convert(),
        )
        .expect("the item is settled");
    let ItemRead::Failed { failure } = &read else {
        panic!("{read:?}");
    };
    assert_eq!(failure.code, "source_read");
    assert!(
        failure.detail.contains("no host bytes"),
        "{}",
        failure.detail
    );
    assert_eq!(failure.recovery, RecoveryPath::Retry);
    assert_eq!(
        failure.key,
        synthetic_key("world", "texture.zbd"),
        "the failure names the item that failed"
    );
    assert_eq!(
        critical.transaction().state(),
        LoadState::Failed,
        "a world without its critical closure may not go interactive"
    );
    assert!(!critical.transaction().critical_closure_ready());
    assert!(!critical.transaction().is_world_interactive());

    // A deferred item's failure is recorded, reported and omitted; the
    // load still finishes.
    let mut deferred = LoadDriver::new(world.request("hull-b", &key), world.store(8, 8 << 20));
    deferred.transaction_mut().begin().expect("the load begins");
    let read = deferred
        .load_item(
            0,
            &mut progress,
            from_source(&Cell::new(0)),
            convert_default,
        )
        .expect("the critical item is settled");
    assert!(matches!(read, ItemRead::Rebuilt { .. }), "{read:?}");
    let read = deferred
        .load_item(
            1,
            &mut progress,
            || {
                Err(ReadError::OutOfRange {
                    start: 4096,
                    length: 16,
                    member_length: 8,
                }
                .into())
            },
            no_convert(),
        )
        .expect("the deferred item is settled");
    let ItemRead::Failed { failure } = &read else {
        panic!("{read:?}");
    };
    assert_eq!(failure.code, "source_read");
    assert_eq!(
        failure.key,
        synthetic_key("world", "hud.dss"),
        "the deferred failure names the item that failed, not another one"
    );
    assert!(failure.detail.contains("leaves the"), "{}", failure.detail);
    assert_eq!(failure.recovery, RecoveryPath::Retry);
    deferred.validate_delivered().expect("the load validates");
    assert!(deferred.transaction().is_world_interactive());
    let bundle = deferred
        .transaction()
        .ready_bundle()
        .expect("a ready bundle");
    assert_eq!(
        bundle.omitted(),
        &[synthetic_key("world", "hud.dss")],
        "the deferred failure is reported on the bundle, not hidden"
    );
    assert_eq!(bundle.items().len(), 1, "only the delivered item is bound");
}

/// An item the driver already settled is never read a second time: a
/// retry is a new transaction, so two reads can never race for one slot.
#[test]
fn accept_f15_b_an_item_is_read_once_per_driver() {
    let world = World::new("f15-b-once");
    let key = key_a();
    let mut driver = LoadDriver::new(world.request("hull-a", &key), world.store(8, 8 << 20));
    driver.transaction_mut().begin().expect("the load begins");
    let mut progress = no_progress();
    driver
        .load_item(
            0,
            &mut progress,
            from_source(&Cell::new(0)),
            convert_default,
        )
        .expect("the item is settled");
    let error = driver
        .load_item(
            0,
            &mut progress,
            from_source(&Cell::new(0)),
            convert_default,
        )
        .expect_err("a settled item is not read again");
    let DriverError::NotAccepted {
        index,
        state,
        error: reason,
    } = &error
    else {
        panic!("{error}");
    };
    assert_eq!(*index, 0);
    assert_eq!(
        *reason,
        cs_app::loading::IssueError::ItemBusy { index: 0 },
        "the second read is refused as a busy item, not read again"
    );
    assert_eq!(*state, LoadState::Loading, "the second item is still open");
    let error = driver
        .load_item(
            7,
            &mut progress,
            from_source(&Cell::new(0)),
            convert_default,
        )
        .expect_err("an index outside the closure is refused");
    assert!(matches!(
        error,
        DriverError::NotAccepted {
            index: 7,
            error: cs_app::loading::IssueError::UnknownItem { index: 7 },
            ..
        }
    ));
}

/// The store a warm load is served from is the store the driver hands
/// back, so a caller can inspect or invalidate it after the load.
#[test]
fn accept_f15_b_driver_hands_back_its_transaction_and_store() {
    let world = World::new("f15-b-parts");
    let key = key_a();
    let store = world.warm(&key, &derived_of_source(), 8 << 20);
    let identity = {
        let driver = LoadDriver::new(world.request("hull-a", &key), store);
        assert_eq!(driver.transaction().state(), LoadState::Requested);
        assert_eq!(driver.store().usage().entries, 1);
        driver.transaction().identity()
    };
    let driver = LoadDriver::new(world.request("hull-b", &key), world.store(8, 8 << 20));
    assert_ne!(
        driver.transaction().identity(),
        identity,
        "a successor load never shares the serial of the load it replaced"
    );
    let (transaction, store) = driver.into_parts();
    assert_eq!(transaction.state(), LoadState::Requested);
    assert_eq!(store.usage().entries, 1);
    assert!(matches!(
        store.begin_read(&key).expect("the lookup runs"),
        CacheLookup::Hit(_)
    ));
}
