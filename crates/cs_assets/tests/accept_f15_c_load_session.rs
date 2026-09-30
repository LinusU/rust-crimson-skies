//! Acceptance scenario F15-C: the wired load — the real producer
//! (`SessionIo` resolving and reading through a `ContentSession`), the
//! [`LoadingScreen`] render-model a UI draws between pumps, cancellation
//! and teardown, retry as a fresh transaction over the same store, the
//! controlled [`ExpectedLoad`] handoff into a Bevy [`World`], and spec
//! F15 AC03's per-input invalidation at the session boundary.
//!
//! These tests exercise production code only — `cs_app::loading`'s
//! [`LoadingSession`], [`SessionIo`], [`SourceFault`] and [`ExpectedLoad`]
//! plus the real `cs_formats` BM reader and composition as the wired
//! livery conversion. The fixture installation is synthetic; it proves
//! nothing about retail content. Removing `LoadingSession` or `SessionIo`
//! makes the file fail to build; removing the identity or invalidation
//! checks makes the scenarios themselves fail.

mod common;

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

use bevy::ecs::world::World;

use cs_app::assets::{CanonicalPayload, ConversionError};
use cs_app::loading::{
    Criticality, DriverError, ExpectedLoad, HandoffError, ItemRead, LoadIo, LoadItem, LoadRequest,
    LoadState, LoadTarget, LoadedItemBinding, LoadingSession, RebuildCause, RecoveryPath,
    RetryError, SessionIo, SourceFault,
};
use cs_assets::cache::{
    CacheBudget, CacheDirectory, CacheKey, CacheLookup, CacheStore, ConversionOptions,
    ConverterVersion, DecoderId, IrVersion, SourceSpanHash,
};
use cs_assets::vfs::{ContentSession, MountBuilder, ReadCancel, ReadProgress, SessionBuilder};
use cs_formats::{AllocationBudget, PaintColor, ParseContext, read_bm};
use cs_types::asset_id::{MountId, MountNamespace, PrecedenceClass, SourceSpan, WorldGroup};
use cs_types::content::ContentKind;
use cs_types::evidence::ContentHash;

use common::{TempTree, fixed_hash, synthetic_content, synthetic_context, synthetic_key};

fn install() -> ContentHash {
    fixed_hash(0x5A)
}

/// A tiny valid BM image in the observed subset (F09-A's layout): `height`
/// and `width` as little-endian u16, then base RGB, one byte per mask
/// plane and one RGBA overlay texel — 4 + 10 bytes at 1x1.
fn bm_1x1(base: [u8; 3], masks: [u8; 3], overlay: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&base);
    bytes.extend_from_slice(&masks);
    bytes.extend_from_slice(&overlay);
    bytes
}

const PAINT: [PaintColor; 3] = [
    PaintColor::new(0xCC, 0x22, 0x11),
    PaintColor::new(0x22, 0x66, 0xCC),
    PaintColor::new(0xEE, 0xAA, 0x33),
];

/// The wired conversion for `.bm` members: the real BM reader and the
/// observed composition algorithm, so the changed-source scenario covers
/// a genuine derived asset rather than a nominal transform. Other
/// members pass their bytes through.
fn convert_member(item: &LoadItem, payload: &CanonicalPayload) -> Result<Vec<u8>, ConversionError> {
    if !item.key.path().as_str().ends_with(".bm") {
        return Ok(payload.bytes().to_vec());
    }
    let mut context = ParseContext::with_defaults(item.key.to_string());
    let file = read_bm(&mut context, payload.bytes()).map_err(|error| ConversionError::Failed {
        detail: error.to_string(),
    })?;
    let mut budget = AllocationBudget::with_defaults(item.key.to_string());
    let image = file
        .compose(PAINT, &mut budget)
        .map_err(|error| ConversionError::Failed {
            detail: error.to_string(),
        })?;
    Ok(image.rgb().to_vec())
}

fn livery_converter() -> ConverterVersion {
    ConverterVersion {
        decoder: DecoderId::new("bm-livery").expect("valid decoder id"),
        decoder_version: 1,
        ir: IrVersion(1),
    }
}

/// The span `session` resolves `member` under — the input identity a
/// derived cache key records.
fn span_of(session: &ContentSession, member: &str) -> SourceSpan {
    session
        .resolve(&synthetic_key("world", member))
        .expect("the member resolves")
        .resolved()
        .span
        .clone()
}

/// A load item whose derived cache key lists exactly the source span the
/// session resolves the member under — AC03's per-input granularity.
fn member_item(
    session: &ContentSession,
    member: &str,
    content: &str,
    criticality: Criticality,
) -> LoadItem {
    let key = synthetic_key("world", member);
    let span = span_of(session, member);
    LoadItem::new(
        key,
        synthetic_content(ContentKind::Image, content),
        criticality,
        span.length(),
    )
    .expect("a real member has bytes")
    .with_derived(
        CacheKey::new(
            install(),
            &[span],
            livery_converter(),
            ConversionOptions::none(),
        )
        .expect("a key with an input"),
    )
}

/// A load item for a member no mount holds — a declared dependency that
/// must fail as `not_found`, never be skipped.
fn missing_item(member: &str, content: &str) -> LoadItem {
    LoadItem::new(
        synthetic_key("world", member),
        synthetic_content(ContentKind::Image, content),
        Criticality::GameplayCritical,
        64,
    )
    .expect("nonzero work units")
}

fn request(session: &ContentSession, items: Vec<LoadItem>) -> LoadRequest {
    LoadRequest {
        session: session.generation(),
        target: LoadTarget::world(WorldGroup::new("zbd/c1").expect("valid world spelling")),
        items,
    }
}

/// How many entities in `world` carry any [`LoadedItemBinding`].
fn bound_entities(world: &mut World) -> usize {
    world.query::<&LoadedItemBinding>().iter(world).count()
}

/// A fixture installation with a world-group directory holding two livery
/// sources, `hull.bm` and `trim.bm`. The bytes are newly authored fixture
/// data — they prove nothing about the retail files, and the original
/// installation is never touched.
struct Fixture {
    _tree: TempTree,
    install_root: PathBuf,
    cache_root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let tree = TempTree::new(label);
        let install_root = tree.root().join("install");
        let cache_root = tree.root().join("cache");
        let world_dir = install_root.join("zbd").join("c1");
        fs::create_dir_all(&world_dir).expect("the fixture installation exists");
        fs::create_dir_all(&cache_root).expect("the fixture cache exists");
        fs::write(
            world_dir.join("hull.bm"),
            bm_1x1([0x80, 0x40, 0x20], [0, 0, 0], [0, 0, 0, 0]),
        )
        .expect("the hull livery is written");
        fs::write(
            world_dir.join("trim.bm"),
            bm_1x1([0x11, 0x99, 0x44], [0, 0, 0], [0, 0, 0, 0]),
        )
        .expect("the trim livery is written");
        Self {
            _tree: tree,
            install_root,
            cache_root,
        }
    }

    /// A content session mounting the fixture's world group — fresh each
    /// call: a new session generation and freshly hashed members, what a
    /// world switch or a restart remount looks like.
    fn session(&self) -> ContentSession {
        let mut builder = SessionBuilder::new(synthetic_context(install(), "zbd/c1"));
        let mount = MountBuilder::new(
            MountId::new("world-0").expect("valid mount id"),
            MountNamespace::new("world").expect("valid namespace"),
            PrecedenceClass::MissionWorld,
            "zbd/c1",
        )
        .with_world_group(WorldGroup::new("zbd/c1").expect("valid world spelling"))
        .retail();
        builder
            .mount_directory(mount, &self.install_root.join("zbd").join("c1"))
            .expect("the world directory mounts");
        builder.open()
    }

    /// A private store rooted at the fixture's cache directory, outside
    /// the fixture installation — each call re-opens it, so what a load
    /// committed survives the way it would across a world switch.
    fn store(&self) -> CacheStore {
        CacheStore::open(
            CacheDirectory::open(&self.cache_root, &self.install_root)
                .expect("a cache root outside the installation"),
            CacheBudget::new(8, 8 << 20).expect("a nonzero budget"),
        )
        .expect("the store opens")
    }

    /// XORs one byte of an on-disk member — the producer-side "this
    /// source changed" the remount and the invalidation observe.
    fn edit(&self, member: &str, index: usize, xor: u8) {
        let path = self.install_root.join("zbd").join("c1").join(member);
        let mut bytes = fs::read(&path).expect("the member reads");
        bytes[index] ^= xor;
        fs::write(&path, bytes).expect("the member is rewritten");
    }
}

/// A fixture producer for the retry path: its source read serves `bytes`
/// until `allowed` reads have run, then faults — so a test controls
/// exactly which item's read fails.
struct FixtureIo {
    reads: Cell<u32>,
    allowed: Cell<u32>,
    bytes: Vec<u8>,
}

impl LoadIo for FixtureIo {
    fn read_source(
        &mut self,
        _index: usize,
        _item: &LoadItem,
        _cancel: &ReadCancel,
        progress: &mut dyn FnMut(ReadProgress),
    ) -> Result<Vec<u8>, SourceFault> {
        let reads = self.reads.get() + 1;
        self.reads.set(reads);
        if reads > self.allowed.get() {
            return Err(SourceFault::new(
                "source_read",
                "the fixture IO fault",
                RecoveryPath::Retry,
            ));
        }
        progress(ReadProgress {
            read: self.bytes.len() as u64,
            total: self.bytes.len() as u64,
        });
        Ok(self.bytes.clone())
    }

    fn convert(
        &mut self,
        _index: usize,
        _item: &LoadItem,
        payload: &CanonicalPayload,
    ) -> Result<Vec<u8>, ConversionError> {
        Ok(payload.bytes().to_vec())
    }
}

/// Spec F15 AC03, wired end to end: a load caches each item's derived
/// asset under a key listing its own source span; editing one livery
/// source invalidates only the entries derived from it — the untouched
/// livery is served from the cache and only the changed one is rebuilt.
#[test]
fn accept_f15_c_changed_livery_source_invalidates_only_its_derived_assets() {
    let fixture = Fixture::new("f15-c-livery-invalidation");

    // First load: both liveries resolve through the session, are composed
    // by the real BM path and cached under keys that record their own
    // source spans.
    let session_a = fixture.session();
    let old_hull_span = span_of(&session_a, "hull.bm");
    let old_trim_span = span_of(&session_a, "trim.bm");
    let hull_item_a = member_item(
        &session_a,
        "hull.bm",
        "hull-livery",
        Criticality::GameplayCritical,
    );
    let trim_item_a = member_item(
        &session_a,
        "trim.bm",
        "trim-livery",
        Criticality::GameplayCritical,
    );
    let hull_key_a = hull_item_a.derived.clone().expect("a derived key");
    let trim_key = trim_item_a.derived.clone().expect("a derived key");
    let mut load_a = LoadingSession::new(
        request(&session_a, vec![hull_item_a, trim_item_a]),
        fixture.store(),
    );
    let mut io_a = SessionIo::new(&session_a, convert_member);
    load_a.begin().expect("the first load begins");
    let read = load_a
        .pump(&mut io_a)
        .expect("the hull item runs")
        .expect("an item settled");
    let ItemRead::Rebuilt {
        payload_sha256: hull_hash_a,
        ..
    } = read
    else {
        panic!("{read:?}");
    };
    let read = load_a
        .pump(&mut io_a)
        .expect("the trim item runs")
        .expect("an item settled");
    assert!(matches!(read, ItemRead::Rebuilt { .. }), "{read:?}");
    assert!(load_a.pump(&mut io_a).expect("validation runs").is_none());
    assert_eq!(load_a.state(), LoadState::Ready);

    let mut world = World::new();
    load_a.announce(&mut world);
    let entities = load_a.deliver(&mut world).expect("the bundle attaches");
    assert_eq!(entities.len(), 2);
    let store = load_a.close();
    assert_eq!(store.usage().entries, 2, "both derived assets are cached");

    // One livery source is edited on disk. The next session remounts and
    // rehashes it, so its span — and therefore the cache-key input —
    // changes; the old span hash is what the stale entries still record.
    fixture.edit("hull.bm", 4, 0xFF); // one byte of the base plane
    let session_b = fixture.session();
    let new_hull_span = span_of(&session_b, "hull.bm");
    assert_ne!(
        SourceSpanHash::of(&new_hull_span),
        SourceSpanHash::of(&old_hull_span),
        "the edited source hashes differently"
    );
    assert_eq!(
        SourceSpanHash::of(&span_of(&session_b, "trim.bm")),
        SourceSpanHash::of(&old_trim_span),
        "the untouched source keeps its span hash"
    );

    let hull_item_b = member_item(
        &session_b,
        "hull.bm",
        "hull-livery",
        Criticality::GameplayCritical,
    );
    let trim_item_b = member_item(
        &session_b,
        "trim.bm",
        "trim-livery",
        Criticality::GameplayCritical,
    );
    let hull_key_b = hull_item_b.derived.clone().expect("a derived key");
    assert_ne!(
        hull_key_b.digest(),
        hull_key_a.digest(),
        "a changed input rekeys the derived asset"
    );
    assert_eq!(
        trim_item_b
            .derived
            .as_ref()
            .expect("a derived key")
            .digest(),
        trim_key.digest(),
        "an unchanged input keeps its key"
    );

    // Invalidating the previous source's span hash drops only the derived
    // assets built on it: the trim's entry survives.
    let mut load_b =
        LoadingSession::new(request(&session_b, vec![hull_item_b, trim_item_b]), store);
    let report = load_b
        .invalidate_source(SourceSpanHash::of(&old_hull_span))
        .expect("the invalidation runs");
    assert_eq!(
        report.dropped,
        vec![hull_key_a.digest()],
        "only the entry derived from the edited source fell"
    );
    assert_eq!(report.entries, 1);

    // The re-load: the untouched livery is a warm hit, the changed one
    // rebuilds and republishes — nothing else moves.
    let mut io_b = SessionIo::new(&session_b, convert_member);
    load_b.begin().expect("the reload begins");
    let read = load_b
        .pump(&mut io_b)
        .expect("the hull item runs")
        .expect("an item settled");
    let ItemRead::Rebuilt {
        cause,
        payload_sha256: hull_hash_b,
        ..
    } = read
    else {
        panic!("{read:?}");
    };
    assert_eq!(cause, RebuildCause::NoEntry);
    assert_ne!(
        hull_hash_b, hull_hash_a,
        "the composed livery really changed"
    );
    let read = load_b
        .pump(&mut io_b)
        .expect("the trim item runs")
        .expect("an item settled");
    assert!(matches!(read, ItemRead::CacheHit { .. }), "{read:?}");
    assert!(load_b.pump(&mut io_b).expect("validation runs").is_none());
    assert_eq!(load_b.state(), LoadState::Ready);

    // The store holds exactly the surviving and the rebuilt entry.
    assert_eq!(load_b.driver().store().usage().entries, 2);
    assert!(matches!(
        load_b.driver().store().begin_read(&trim_key),
        Ok(CacheLookup::Hit(_))
    ));
    assert!(matches!(
        load_b.driver().store().begin_read(&hull_key_b),
        Ok(CacheLookup::Hit(_))
    ));
    assert!(
        matches!(
            load_b.driver().store().begin_read(&hull_key_a),
            Ok(CacheLookup::Miss)
        ),
        "the stale entry is gone, not merely unreachable"
    );

    let mut world_b = World::new();
    load_b.announce(&mut world_b);
    let entities = load_b
        .deliver(&mut world_b)
        .expect("the reload's bundle attaches");
    assert_eq!(entities.len(), 2);
    let _store = load_b.close();
}

/// Cancellation is teardown: the switch the UI throws stops the bounded
/// step before the source is ever read, the transaction ends `Cancelled`
/// — not `Failed` — the screen reports it, no bundle exists, and the
/// store comes back clean. A direct `cancel` on a live load ends it the
/// same way.
#[test]
fn accept_f15_c_cancel_stops_the_load_and_the_teardown_is_clean() {
    let fixture = Fixture::new("f15-c-cancel");
    let session = fixture.session();
    let mut load = LoadingSession::new(
        request(
            &session,
            vec![
                member_item(
                    &session,
                    "hull.bm",
                    "hull-livery",
                    Criticality::GameplayCritical,
                ),
                member_item(
                    &session,
                    "trim.bm",
                    "trim-livery",
                    Criticality::GameplayCritical,
                ),
            ],
        ),
        fixture.store(),
    );
    let mut io = SessionIo::new(&session, convert_member);

    // A pump before `begin` is a transition error, not an implicit start.
    assert!(matches!(
        load.pump(&mut io),
        Err(DriverError::Transition(_))
    ));
    load.begin().expect("the load begins");

    // The cancel switch is thrown mid-load — as the UI's button or a
    // world switch does — and the next bounded step reports the
    // cancellation instead of reading the source.
    load.cancel_handle().cancel();
    let read = load
        .pump(&mut io)
        .expect("the cancelled step settles")
        .expect("the item was issued");
    let ItemRead::Failed { failure } = &read else {
        panic!("{read:?}");
    };
    assert_eq!(failure.code, "read_cancelled");
    assert_eq!(load.state(), LoadState::Cancelled);
    assert!(
        load.pump(&mut io).expect("nothing left to pump").is_none(),
        "a terminal load has nothing to pump"
    );
    let screen = load.screen();
    assert_eq!(screen.state, LoadState::Cancelled);
    assert!(!screen.cancellable);
    assert!(
        screen.failures.is_empty(),
        "a cancelled load's unfinished reads are cancelled work, not failures"
    );
    assert!(matches!(
        load.deliver(&mut World::new()),
        Err(HandoffError::NotReady {
            state: LoadState::Cancelled
        })
    ));
    assert!(
        matches!(
            load.retry(),
            Err(RetryError::NotFailed {
                state: LoadState::Cancelled
            })
        ),
        "a cancelled load is a switch-away, not a retry"
    );

    // A direct cancel on a live load ends it at once.
    let session2 = fixture.session();
    let mut load2 = LoadingSession::new(
        request(
            &session2,
            vec![member_item(
                &session2,
                "hull.bm",
                "hull-livery",
                Criticality::GameplayCritical,
            )],
        ),
        fixture.store(),
    );
    load2.begin().expect("the second load begins");
    let report = load2.cancel().expect("a live load cancels");
    assert_eq!(report.detached, 0, "no read was in flight between pumps");
    assert_eq!(load2.state(), LoadState::Cancelled);
    let store = load2.close();
    assert_eq!(
        store.usage().entries,
        0,
        "a cancelled load publishes nothing"
    );
}

/// A failed load retries as a fresh transaction: the retry carries a new
/// serial on the same request and the same store — so entries the failed
/// attempt published still serve a warm re-read — and reaches `Ready`
/// once the source fault clears. Only `Failed` loads may retry.
#[test]
fn accept_f15_c_retry_is_a_fresh_transaction_over_the_same_store() {
    let fixture = Fixture::new("f15-c-retry");
    let session = fixture.session();
    let mut io = FixtureIo {
        reads: Cell::new(0),
        allowed: Cell::new(1),
        bytes: b"fixture payload".to_vec(),
    };

    let mut load = LoadingSession::new(
        request(
            &session,
            vec![
                member_item(
                    &session,
                    "hull.bm",
                    "hull-livery",
                    Criticality::GameplayCritical,
                ),
                member_item(
                    &session,
                    "trim.bm",
                    "trim-livery",
                    Criticality::GameplayCritical,
                ),
            ],
        ),
        fixture.store(),
    );
    let first_identity = load.identity();

    // The trim item's source read faults once: the hull item is already
    // read and published; the load fails before its critical closure is
    // complete.
    load.run(&mut io)
        .expect("the first attempt runs to failure");
    assert_eq!(load.state(), LoadState::Failed);
    assert_eq!(load.failures().len(), 1);
    assert_eq!(load.failures()[0].code, "source_read");
    assert_eq!(load.failures()[0].recovery, RecoveryPath::Retry);
    assert_eq!(io.reads.get(), 2, "both items were attempted");
    io.allowed.set(16);

    // Only a failed load retries — a live one refuses.
    let request_items = || {
        request(
            &session,
            vec![member_item(
                &session,
                "hull.bm",
                "hull-livery",
                Criticality::GameplayCritical,
            )],
        )
    };
    assert!(matches!(
        LoadingSession::new(request_items(), fixture.store()).retry(),
        Err(RetryError::NotFailed {
            state: LoadState::Requested
        })
    ));

    // The retry is a fresh transaction — a new serial, so nothing the
    // failed attempt produced can land in it — over the same store, so
    // the hull item is a warm hit and only the trim source is read again.
    let mut retry = load.retry().expect("a failed load retries");
    assert_ne!(
        retry.identity().serial,
        first_identity.serial,
        "the retry is a fresh transaction, not the terminal one replayed"
    );
    assert_eq!(
        retry.identity().session,
        first_identity.session,
        "the retry re-enters the same content session"
    );
    retry.begin().expect("the retry begins");
    let read = retry
        .pump(&mut io)
        .expect("the hull item runs")
        .expect("an item settled");
    assert!(
        matches!(read, ItemRead::CacheHit { .. }),
        "the failed attempt's published entry serves the retry: {read:?}"
    );
    assert_eq!(
        io.reads.get(),
        2,
        "a warm retry does not re-read the source"
    );
    let read = retry
        .pump(&mut io)
        .expect("the trim item runs")
        .expect("an item settled");
    assert!(matches!(read, ItemRead::Rebuilt { .. }), "{read:?}");
    assert_eq!(io.reads.get(), 3);
    assert!(retry.pump(&mut io).expect("validation runs").is_none());
    assert_eq!(retry.state(), LoadState::Ready);

    let mut world = World::new();
    retry.announce(&mut world);
    let entities = retry.deliver(&mut world).expect("the retry attaches");
    assert_eq!(entities.len(), 2);

    // A ready load has nothing to retry.
    assert!(matches!(
        retry.retry(),
        Err(RetryError::NotFailed {
            state: LoadState::Ready
        })
    ));
}

/// A dependency nothing resolves is a named failure with a recovery path
/// — the loading screen shows it, not a bar stalled at a guessed percent —
/// and the load fails without ever going interactive.
#[test]
fn accept_f15_c_missing_dependency_names_the_failure_and_recovery() {
    let fixture = Fixture::new("f15-c-missing");
    let session = fixture.session();
    let mut load = LoadingSession::new(
        request(
            &session,
            vec![
                member_item(
                    &session,
                    "hull.bm",
                    "hull-livery",
                    Criticality::GameplayCritical,
                ),
                missing_item("missing.bm", "missing-livery"),
            ],
        ),
        fixture.store(),
    );
    let mut io = SessionIo::new(&session, convert_member);
    load.run(&mut io)
        .expect("the load runs to its recorded failure");
    assert_eq!(load.state(), LoadState::Failed);
    let failures = load.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code, "not_found");
    assert_eq!(failures[0].recovery, RecoveryPath::MissingDependency);
    assert_eq!(
        failures[0].key,
        synthetic_key("world", "missing.bm"),
        "the failure names the dependency nothing resolved"
    );
    let screen = format!("{}", load.screen());
    assert!(screen.contains("not_found"), "{screen}");
    assert!(screen.contains("missing_dependency"), "{screen}");
    assert!(!load.driver().transaction().is_world_interactive());
    assert!(matches!(
        load.deliver(&mut World::new()),
        Err(HandoffError::NotReady {
            state: LoadState::Failed
        })
    ));
    let _store = load.close();
}

/// A converter refusing a canonical payload is its own failure class: the
/// member resolves and reads fine, but the wired conversion answers
/// `ConversionError::Failed` — recorded as `conversion` with `Abort`
/// recovery, never disguised as a retryable read fault, and a
/// gameplay-critical refusal fails the load before it can go interactive.
#[test]
fn accept_f15_c_conversion_failure_is_named_and_aborts() {
    let fixture = Fixture::new("f15-c-conversion");
    // Two bytes are shorter than the BM header: the member mounts and
    // reads, but `read_bm` refuses it, so the failure lands in the
    // conversion arm, not the source read.
    fs::write(
        fixture
            .install_root
            .join("zbd")
            .join("c1")
            .join("broken.bm"),
        [0u8; 2],
    )
    .expect("the broken livery is written");
    let session = fixture.session();
    let mut load = LoadingSession::new(
        request(
            &session,
            vec![member_item(
                &session,
                "broken.bm",
                "broken-livery",
                Criticality::GameplayCritical,
            )],
        ),
        fixture.store(),
    );
    let mut io = SessionIo::new(&session, convert_member);
    load.run(&mut io)
        .expect("the load runs to its recorded failure");
    assert_eq!(load.state(), LoadState::Failed);
    let failures = load.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code, "conversion");
    assert_eq!(failures[0].recovery, RecoveryPath::Abort);
    assert_eq!(
        failures[0].key,
        synthetic_key("world", "broken.bm"),
        "the failure names the item the converter refused"
    );
    let screen = format!("{}", load.screen());
    assert!(screen.contains("conversion"), "{screen}");
    assert!(screen.contains("abort"), "{screen}");
    assert!(!load.driver().transaction().is_world_interactive());
    assert!(matches!(
        load.deliver(&mut World::new()),
        Err(HandoffError::NotReady {
            state: LoadState::Failed
        })
    ));
    let _store = load.close();
}

/// The simulation handoff attaches only to the world that announced the
/// load, only once, and only when the load is ready: an unannounced world,
/// a world expecting a different load and a repeat delivery are all
/// refused — and the refused bundle spawns nothing.
#[test]
fn accept_f15_c_handoff_attaches_only_to_the_announced_world() {
    let fixture = Fixture::new("f15-c-handoff");
    let session_a = fixture.session();
    let mut load_a = LoadingSession::new(
        request(
            &session_a,
            vec![member_item(
                &session_a,
                "hull.bm",
                "hull-livery",
                Criticality::GameplayCritical,
            )],
        ),
        fixture.store(),
    );
    let mut world = World::new();

    // Nothing to hand off until the load is ready — the state is named.
    assert!(matches!(
        load_a.deliver(&mut world),
        Err(HandoffError::NotReady {
            state: LoadState::Requested
        })
    ));
    let mut io_a = SessionIo::new(&session_a, convert_member);
    load_a.run(&mut io_a).expect("the first load runs");
    assert_eq!(load_a.state(), LoadState::Ready);

    // A ready bundle still does not attach to a world that announced
    // nothing.
    assert!(matches!(
        load_a.deliver(&mut world),
        Err(HandoffError::Unannounced { .. })
    ));
    assert_eq!(bound_entities(&mut world), 0);

    // A world waiting for load_a refuses load_b's bundle as foreign.
    load_a.announce(&mut world);
    let session_b = fixture.session();
    let mut load_b = LoadingSession::new(
        request(
            &session_b,
            vec![member_item(
                &session_b,
                "hull.bm",
                "hull-livery",
                Criticality::GameplayCritical,
            )],
        ),
        fixture.store(),
    );
    let mut io_b = SessionIo::new(&session_b, convert_member);
    load_b.run(&mut io_b).expect("the second load runs");
    assert!(matches!(
        load_b.deliver(&mut world),
        Err(HandoffError::Foreign { bundle, expected })
            if bundle == load_b.identity() && expected == load_a.identity()
    ));
    assert_eq!(
        bound_entities(&mut world),
        0,
        "a bundle of a load the world is not waiting for spawns nothing"
    );

    // The announced load attaches — exactly once, at this boundary.
    let entities = load_a
        .deliver(&mut world)
        .expect("the expected bundle attaches");
    assert_eq!(entities.len(), 1);
    assert_eq!(bound_entities(&mut world), 1);
    assert!(
        world.get_resource::<ExpectedLoad>().is_none(),
        "the handoff consumes the world's expectation"
    );
    assert!(matches!(
        load_a.deliver(&mut world),
        Err(HandoffError::Unannounced { .. })
    ));
    let _store_a = load_a.close();
    let _store_b = load_b.close();
}

/// The loading screen reports measured work, not a guess: progress in the
/// items' declared work units, the bounded step's own byte counts, the
/// item in flight — and it offers cancel only while the load is live.
#[test]
fn accept_f15_c_screen_reports_measured_progress() {
    let fixture = Fixture::new("f15-c-screen");
    let session = fixture.session();
    let mut load = LoadingSession::new(
        request(
            &session,
            vec![
                member_item(
                    &session,
                    "hull.bm",
                    "hull-livery",
                    Criticality::GameplayCritical,
                ),
                member_item(
                    &session,
                    "trim.bm",
                    "trim-livery",
                    Criticality::GameplayCritical,
                ),
            ],
        ),
        fixture.store(),
    );
    let mut io = SessionIo::new(&session, convert_member);
    load.begin().expect("the load begins");

    let screen = load.screen();
    assert_eq!(screen.state, LoadState::Loading);
    assert_eq!(screen.progress.completed_units, 0);
    assert_eq!(screen.progress.total_units, 28, "two 14-byte BM members");
    assert!(screen.item.is_none());
    assert!(screen.cancellable);
    assert!(screen.failures.is_empty());

    let read = load
        .pump(&mut io)
        .expect("the hull item runs")
        .expect("an item settled");
    assert!(matches!(read, ItemRead::Rebuilt { .. }), "{read:?}");
    let screen = load.screen();
    assert_eq!(screen.progress.completed_units, 14);
    assert_eq!(screen.progress.items_ready, 1);
    assert_eq!(
        screen.item.as_ref().expect("an item is in flight"),
        &synthetic_key("world", "hull.bm"),
        "the screen names the item the load is reading"
    );
    // The last bounded step reported was the staged payload write: the
    // composed RGB payload, one complete chunk.
    assert_eq!(screen.io.total, 3);
    assert_eq!(screen.io.read, 3);

    load.run(&mut io).expect("the rest of the load runs");
    let screen = load.screen();
    assert_eq!(screen.state, LoadState::Ready);
    assert_eq!(screen.progress.items_ready, 2);
    assert_eq!(screen.progress.completed_units, screen.progress.total_units);
    assert!(!screen.cancellable);
    let line = format!("{screen}");
    assert!(line.contains("ready"), "{line}");
    assert!(line.contains("28/28 work units"), "{line}");
    let _store = load.close();
}
