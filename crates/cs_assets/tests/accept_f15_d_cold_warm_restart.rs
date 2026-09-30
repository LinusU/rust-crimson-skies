//! Acceptance scenario F15-D: spec F15 **AC04** measured on the original
//! installation — *warm and cold loads produce equal content hashes and
//! gameplay state* — plus the `cold`/`warm`/`restart` cycle of this
//! stage's title.
//!
//! # What this file is
//!
//! The F15-A..C stages proved the load transaction and the private store
//! on synthetic fixtures. This stage runs the *same production path* over
//! the real content at `$CS_GAME_DIR` and requires capability `retail`:
//!
//! * the producer is [`SessionIo`] over a [`ContentSession`] mounted by
//!   the designed baseline layout ([`SessionBuilder::mount_directory`] with
//!   the world [`MountBuilder`], exactly the calls
//!   [`SessionBuilder::mount_installation`] makes per world group), so every
//!   byte is a resolved [`SourceSpan`] of the real installation;
//! * the conversion is the **real** ZBD texture reader
//!   ([`read_zbd_textures`]) and the real [`ZbdTexture::decode`] base level,
//!   so each cached entry is a genuine derived image decoded from original
//!   data — not a byte pass-through of a fixture;
//! * the consumer is the real [`LoadingSession`] over a real [`CacheStore`],
//!   delivering through the controlled [`ExpectedLoad`] handoff into a real
//!   [`World`].
//!
//! # The scenario
//!
//! [`accept_f15_d_retail_cold_and_warm_loads_deliver_equal_content_and_gameplay_state`]
//! is AC04. For **every** world group the installation holds it loads that
//! world's own texture archives twice over one private store: cold, with
//! an empty store, and warm, with the store re-opened as a fresh process
//! would. The warm pass must be served entirely from verified cache
//! entries, and the two passes must deliver the same per-item payload
//! digests, the same closure hash and the same gameplay state (the content
//! and asset key each attached entity presents).
//!
//! The remaining tests are the failure cases AC04 needs to mean anything —
//! each one breaks the cache in the way a plausible shortcut would and
//! requires the *same* delivered content to come out:
//!
//! * a real killed child process leaving a half-written scratch entry,
//!   recovered on the next open (this stage's `restart`);
//! * a corrupted committed payload, refused by integrity validation and
//!   rebuilt (F15 non-negotiable behavior 3);
//! * entries planted under a foreign installation hash, a foreign source
//!   span and a foreign converter version, which must be misses, never
//!   served bytes;
//! * a budget too small for the closure, where entries are evicted and
//!   items go uncached — a cache is an optimization, never the
//!   authoritative data source.
//!
//! # Sensitivity
//!
//! Every test removes the implementation and fails: `CacheStore::begin_read`
//! or `verify_entry` gone and the warm pass is not a cache hit (or serves
//! corrupt bytes), `commit` gone and nothing is warm at all, the
//! [`ReadyBundle::closure_hash`]/`LoadedItemBinding` fields gone and the
//! comparisons do not compile. Nothing here reads a recorded expectation
//! and repeats it: the expected values are the cold pass's own measured
//! output, and the retail inputs are hashed at run time from the mounted
//! installation.
//!
//! Retail tests are `#[ignore = "requires CS_GAME_DIR"]` because CI has no
//! original data; they fail loudly (never vacuously) when the variable is
//! unset. Nothing derived from the installation is written inside it and
//! nothing is committed to Git.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Instant;

use bevy::ecs::world::World;

use cs_app::assets::{CanonicalPayload, ConversionError};
use cs_app::loading::{
    Criticality, ItemRead, LoadItem, LoadRequest, LoadState, LoadTarget, LoadedItemBinding,
    LoadingSession, SessionIo,
};
use cs_assets::cache::{
    CacheBudget, CacheDirectory, CacheKey, CacheStore, ConversionOptions, ConverterVersion,
    DecoderId, IrVersion,
};
use cs_assets::install::{self, Discovery};
use cs_assets::vfs::{ContentSession, MountBuilder, SessionBuilder};
use cs_formats::{AllocationBudget, read_zbd_textures};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan, WorldGroup,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

// --------------------------------------------------------------- inputs ---

/// The original installation. Unset means this machine has no `retail`
/// capability and the retail tests below must not run at all.
fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// Installation discovery hashes every file, so the whole binary shares
/// one run. The manifest is what names the installation in every message
/// and in the evidence report.
fn discovery() -> &'static Discovery {
    static DISCOVERY: OnceLock<Discovery> = OnceLock::new();
    DISCOVERY.get_or_init(|| {
        install::discover(&game_dir()).expect("the original installation is discovered")
    })
}

/// The installation's own identity, so a reader can tell which bytes a
/// measurement was taken from.
fn install_hash() -> ContentHash {
    install::fingerprint(&discovery().manifest)
}

/// The installation's content-only digest: SHA-256 over the per-file
/// content digests alone, sorted by logical key. The evidence report
/// records it as the content fingerprint of the measured installation.
#[allow(dead_code)]
fn content_hash() -> ContentHash {
    install::content_fingerprint(&discovery().manifest)
}

/// How many base levels of each real texture archive the derived asset
/// covers. Bounded on purpose: the closure is the *world's own* texture
/// archives, and a prefix of each is enough to make every entry a real
/// decoded image while keeping one load's IO in the range a frame budget
/// can absorb. Recorded in
/// `docs/findings/2026-09-30-t64-f15-d-cold-warm-restart.md`.
const DERIVED_TEXTURES_PER_ARCHIVE: usize = 8;

// ------------------------------------------------------------- the world ---

/// One world group of the real installation, mounted the designed way.
struct RetailWorld {
    /// The group's discovered spelling, e.g. `ZBD/C1`.
    spelling: String,
    /// The world's own id.
    group: WorldGroup,
    /// A content session whose context selects this world.
    session: ContentSession,
    /// The world's own texture archives, in mount order: `texture.zbd`
    /// and its `rtexture*.zbd` tiers.
    texture_archives: Vec<String>,
}

/// Every world group the installation holds, each with its own mounted
/// session. Built once for the whole binary: discovery and mounting are
/// measured IO, and the sessions are immutable, so the tests share them.
fn worlds() -> &'static [RetailWorld] {
    static WORLDS: OnceLock<Vec<RetailWorld>> = OnceLock::new();
    WORLDS.get_or_init(|| {
        let root = game_dir();
        let found = discovery();
        let install = install_hash();
        found
            .diagnosis
            .world_groups
            .iter()
            .enumerate()
            .map(|(index, spelling)| {
                let group = WorldGroup::from_relative(spelling.clone());
                // The designed baseline layout of
                // `SessionBuilder::mount_installation`, one world group at
                // a time: the same `MountBuilder` shape, precedence,
                // namespace and container label that method builds.
                let context = ResolveContext::new(install).with_world_group(group.clone());
                let mut builder = SessionBuilder::new(context);
                let mount = MountBuilder::new(
                    MountId::new(&format!("world-{index}")).expect("a valid mount id"),
                    MountNamespace::new("world").expect("a valid namespace"),
                    PrecedenceClass::MissionWorld,
                    spelling.as_str(),
                )
                .with_world_group(group.clone())
                .retail();
                let mut dir = root.clone();
                dir.extend(spelling.as_str().split(['/', '\\']));
                builder
                    .mount_directory(mount, &dir)
                    .unwrap_or_else(|error| panic!("{spelling}: {error}"));
                let session = builder.open();
                assert!(
                    session.rejected().is_empty(),
                    "{spelling}: {:?}",
                    session.rejected()
                );
                // The world's own texture archives, taken from what the
                // mount actually holds rather than from a hardcoded list,
                // so a different installation is measured, not assumed.
                let mut texture_archives: Vec<String> = session
                    .mounts()
                    .flat_map(|mount| mount.members())
                    .map(|(_, member)| member.spelling().as_str().to_owned())
                    .filter(|spelling| is_texture_archive(spelling))
                    .collect();
                texture_archives.sort();
                assert!(
                    !texture_archives.is_empty(),
                    "{spelling}: the world's own texture archives"
                );
                RetailWorld {
                    spelling: spelling.as_str().to_owned(),
                    group,
                    session,
                    texture_archives,
                }
            })
            .collect()
    })
}

/// Whether `spelling` is one of the world's own texture archives: the
/// primary `texture.zbd` and the `rtexture*.zbd` tiers beside it. The
/// rule is the one F04-D measured and recorded in
/// `docs/findings/2026-09-29-t346-texture-archive-member-collisions.md`:
/// within one world the primary is the designed winner over its tiers.
fn is_texture_archive(spelling: &str) -> bool {
    let lower = spelling.to_ascii_lowercase();
    lower == "texture.zbd" || (lower.starts_with("rtexture") && lower.ends_with(".zbd"))
}

// ------------------------------------------------------- the conversion ---

/// The converter identity the derived texture entries are stored under.
/// Bumping either version is what must invalidate them.
fn texture_converter() -> ConverterVersion {
    ConverterVersion {
        decoder: DecoderId::new("zbd-texture-base-level").expect("a valid decoder id"),
        decoder_version: 1,
        ir: IrVersion(1),
    }
}

/// The real conversion: the production ZBD texture reader parses the
/// archive and the real [`cs_formats::ZbdTexture::decode`] produces each
/// base level. The derived form is those decoded base levels in archive
/// order, each preceded by its own name and extent, so a cache entry is
/// original image data decoded by the engine — not the source bytes
/// renamed.
fn convert_texture_archive(
    item: &LoadItem,
    payload: &CanonicalPayload,
) -> Result<Vec<u8>, ConversionError> {
    let label = item.key.path().as_str().to_owned();
    let mut budget = AllocationBudget::with_defaults(label.clone());
    let package = read_zbd_textures(&label, payload.bytes(), &mut budget).map_err(|error| {
        ConversionError::Failed {
            detail: format!("{label}: {error}"),
        }
    })?;
    let mut derived = Vec::new();
    for texture in package.textures().iter().take(DERIVED_TEXTURES_PER_ARCHIVE) {
        let mut level = AllocationBudget::with_defaults(label.clone());
        let image = texture
            .decode(&mut level)
            .map_err(|error| ConversionError::Failed {
                detail: format!("{label}#{}: {error}", texture.entry_index()),
            })?;
        let extent = image.extent();
        let name = texture.name().as_bytes();
        let _ = derived.try_reserve(12 + name.len() + image.texels().len());
        derived.extend_from_slice(&(name.len() as u32).to_le_bytes());
        derived.extend_from_slice(name);
        derived.extend_from_slice(&extent.width.to_le_bytes());
        derived.extend_from_slice(&extent.height.to_le_bytes());
        derived.extend_from_slice(image.texels());
    }
    Ok(derived)
}

// ------------------------------------------------------------ load items ---

/// The `SourceSpan` the session resolves `member` under — the input
/// identity a derived cache key records.
fn span_of(session: &ContentSession, member: &str) -> SourceSpan {
    let key = AssetKey::from_spelling("world", member, "default").expect("a valid asset key");
    session
        .resolve(&key)
        .unwrap_or_else(|error| panic!("{member} resolves: {error}"))
        .resolved()
        .span
        .clone()
}

/// A load item for one of the world's own texture archives, with the
/// derived cache key listing exactly the source span the session resolves
/// it under.
fn archive_item(world: &RetailWorld, member: &str) -> LoadItem {
    let span = span_of(&world.session, member);
    let key = AssetKey::from_spelling("world", member, "default").expect("a valid asset key");
    // The content id is the world's own logical spelling, so the same
    // content is the same id in every pass and every world. A content key
    // admits `.`, `_` and `-`, so the world's directory separator becomes
    // a `-`; that is an id spelling, not a path.
    let content = ContentId::from_source(
        ContentKind::Image,
        &format!("{}-{}", world.spelling.replace(['/', '\\'], "-"), member),
    )
    .expect("a valid content id");
    LoadItem::new(key, content, Criticality::GameplayCritical, span.length())
        .expect("a real member has bytes")
        .with_derived(
            CacheKey::new(
                install_hash(),
                &[span],
                texture_converter(),
                ConversionOptions::none(),
            )
            .expect("a key with an input"),
        )
}

/// Every load item of one world's texture closure.
fn closure(world: &RetailWorld) -> Vec<LoadItem> {
    world
        .texture_archives
        .iter()
        .map(|member| archive_item(world, member))
        .collect()
}

// ---------------------------------------------------------- what a load did ---

/// The gameplay state one delivered load leaves in a world: what each
/// attached entity presents, plus the per-key payload digest. It is
/// compared across passes, so it must not contain the load identity —
/// cold, warm and restarted loads are different transactions and must
/// still deliver the same state.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GameplayState {
    /// The closure hash the bundle was versioned with.
    closure_hash: ContentHash,
    /// Sorted `(content id, asset key)` pairs of the delivered bindings.
    bindings: Vec<(String, String)>,
    /// Sorted `(asset key, payload digest)` pairs.
    payloads: Vec<(String, ContentHash)>,
}

/// Everything one pass of the cycle measured.
struct Outcome {
    /// The gameplay state handed to the world.
    state: GameplayState,
    /// Per item, whether the bytes came out of the cache.
    hits: Vec<bool>,
    /// The item reads in item order, for the assertions that name a cause.
    reads: Vec<ItemRead>,
    /// Entries and bytes the store holds afterwards.
    usage_entries: u64,
    usage_bytes: u64,
    /// What opening the store recovered, if this pass opened it.
    recovery: cs_assets::cache::RecoveryReport,
}

/// Opens a private store over `cache_root`. The root is outside the
/// installation and [`CacheDirectory::open`] refuses one that is not, so
/// the source installation is never a write target.
fn open_store(cache_root: &Path, entries: u64, bytes: u64) -> CacheStore {
    CacheStore::open(
        CacheDirectory::open(cache_root, &game_dir())
            .expect("a cache root outside the installation"),
        CacheBudget::new(entries, bytes).expect("a nonzero budget"),
    )
    .expect("the private store opens")
}

/// Runs one full load of `world`'s closure over `store` and hands it to a
/// fresh world.
///
/// The load is pumped item by item, exactly as the windowed loop pumps
/// once per frame, so the returned [`Outcome`] describes the real
/// per-item path: which items the cache served, what each delivered and
/// what the controlled handoff left in the world.
fn run_load(world: &RetailWorld, store: CacheStore) -> Outcome {
    let items = closure(world);
    let expected_items = items.len();
    let request = LoadRequest {
        session: world.session.generation(),
        target: LoadTarget::world(world.group.clone()),
        items,
    };
    let mut load = LoadingSession::new(request, store);
    let recovery = *load.driver().store().recovery();
    let mut reads: Vec<ItemRead> = Vec::new();
    {
        let mut io = SessionIo::new(&world.session, convert_texture_archive);
        load.begin().expect("the load begins");
        while let Some(read) = load.pump(&mut io).expect("one bounded step of the load") {
            reads.push(read);
        }
    }
    assert_eq!(
        reads.len(),
        expected_items,
        "{}: every item settles exactly once",
        world.spelling
    );
    assert_eq!(
        load.state(),
        LoadState::Ready,
        "{}: {:?}",
        world.spelling,
        load.failures()
    );

    // The versioned output the simulation receives, read from the real
    // transaction before the session is torn down.
    let bundle = load
        .driver()
        .transaction()
        .ready_bundle()
        .expect("a Ready transaction has a bundle");
    let closure_hash = bundle.closure_hash();
    let mut payloads: Vec<(String, ContentHash)> = bundle
        .items()
        .iter()
        .map(|item| (item.key.path().as_str().to_owned(), item.payload_sha256))
        .collect();
    payloads.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        payloads.len(),
        expected_items,
        "{}: the bundle carries the whole closure",
        world.spelling
    );
    assert!(
        bundle.omitted().is_empty(),
        "{}: nothing was deferred: {:?}",
        world.spelling,
        bundle.omitted()
    );

    // The gameplay state: the content and asset key each attached entity
    // presents. Read back out of the world, not from the bundle, so this
    // is what a consumer of the handoff actually sees.
    let mut target_world = World::new();
    load.announce(&mut target_world);
    let entities = load
        .deliver(&mut target_world)
        .expect("the bundle attaches to the world that announced it");
    assert_eq!(entities.len(), expected_items);
    let mut bindings: Vec<(String, String)> = entities
        .iter()
        .map(|entity| {
            let binding = target_world
                .get::<LoadedItemBinding>(*entity)
                .expect("a delivered entity carries its binding");
            (
                binding.content.as_str().to_owned(),
                binding.key.path().as_str().to_owned(),
            )
        })
        .collect();
    bindings.sort();

    let hits: Vec<bool> = reads.iter().map(ItemRead::is_cache_hit).collect();
    let store_handle = load.close();
    let usage = store_handle.usage();
    Outcome {
        state: GameplayState {
            closure_hash,
            bindings,
            payloads,
        },
        hits,
        reads,
        usage_entries: usage.entries,
        usage_bytes: usage.bytes,
        recovery,
    }
}

// ------------------------------------------------------------- the tests ---

/// A private, empty cache root for one pass. `TempTree` removes it again,
/// so nothing derived from the installation outlives the test and nothing
/// is written inside the installation.
struct Sandbox {
    /// Kept alive so the private cache is removed again when the test
    /// ends; nothing derived from the installation outlives it.
    _tree: common::TempTree,
    root: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let tree = common::TempTree::new(label);
        // `CacheDirectory::open` requires the root to exist and to be
        // outside the installation; it refuses a root that is not.
        let root = tree.root().join("cache");
        fs::create_dir_all(&root).expect("the private cache root is created");
        Self { _tree: tree, root }
    }

    fn cache_root(&self) -> PathBuf {
        self.root.clone()
    }
}

/// Spec F15 **AC04** on the original installation, for every world group
/// it holds: the world's own texture archives are loaded twice over one
/// private store — cold against an empty store, then warm against the
/// store a fresh process would re-open — and the two passes must deliver
/// the same content hashes and the same gameplay state.
///
/// The warm pass is required to be served *entirely* from verified cache
/// entries. That is what makes this AC04 rather than a tautology: a store
/// that ignored its own index, or a driver that rebuilt silently, would
/// deliver equal hashes by rebuilding every time and would fail the hit
/// assertion.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f15_d_retail_cold_and_warm_loads_deliver_equal_content_and_gameplay_state() {
    let installation = install_hash().to_hex();
    assert_eq!(
        worlds().len(),
        8,
        "the reference installation {installation} holds eight world groups"
    );

    for world in worlds() {
        let sandbox = Sandbox::new("f15-d-cold-warm");
        let items = closure(world).len();
        assert!(
            items > 0,
            "{}: an empty closure proves nothing",
            world.spelling
        );

        // Cold: nothing cached, so every item is read from its own source
        // span, converted by the real ZBD texture path and published.
        let cold = run_load(world, open_store(&sandbox.cache_root(), 64, 8 << 20));
        assert!(
            !cold.hits.iter().any(|hit| *hit),
            "{}: a cold load must not be served from the cache",
            world.spelling
        );
        for read in &cold.reads {
            assert!(
                matches!(read, ItemRead::Rebuilt { .. }),
                "{}: cold item: {read:?}",
                world.spelling
            );
        }
        assert_eq!(
            cold.usage_entries, items as u64,
            "{}: every derived archive reached the store",
            world.spelling
        );
        assert!(
            cold.usage_bytes > 0,
            "{}: real bytes were stored",
            world.spelling
        );
        // The closure hash is a real digest of the delivered closure, not
        // a placeholder: it is reproducible from the payload digests the
        // bundle carries, so a stubbed zero would be visible.
        assert_ne!(
            cold.state.closure_hash.to_hex(),
            "0".repeat(64),
            "{}: the closure hash is measured, not empty",
            world.spelling
        );
        assert_eq!(
            cold.state.closure_hash.to_hex().len(),
            64,
            "{}: the closure hash is a canonical sha256",
            world.spelling
        );

        // Warm: the store re-opened, so its own recovery scan runs and
        // the whole closure must come out of verified entries.
        let warm = run_load(world, open_store(&sandbox.cache_root(), 64, 8 << 20));
        assert_eq!(
            warm.recovery.swept_staging, 0,
            "{}: a clean shutdown leaves no scratch behind",
            world.spelling
        );
        assert_eq!(
            warm.recovery.dropped_corrupt, 0,
            "{}: a clean shutdown leaves no corrupt entry behind",
            world.spelling
        );
        assert!(
            warm.hits.iter().all(|hit| *hit),
            "{}: a warm load must be served entirely from the cache: {:?}",
            world.spelling,
            warm.reads
        );

        // AC04: equal content hashes and equal gameplay state.
        assert_eq!(
            warm.state.payloads, cold.state.payloads,
            "{}: warm and cold deliver different content hashes",
            world.spelling
        );
        assert_eq!(
            warm.state.closure_hash, cold.state.closure_hash,
            "{}: warm and cold deliver different closure hashes",
            world.spelling
        );
        assert_eq!(
            warm.state.bindings, cold.state.bindings,
            "{}: warm and cold deliver different gameplay state",
            world.spelling
        );
        assert_eq!(
            warm.usage_entries, cold.usage_entries,
            "{}: a warm load must not republish what it read",
            world.spelling
        );
    }
}

/// This stage's `restart`: a real child process is killed while it holds
/// a half-written cache entry, and the next open of the same private store
/// recovers — sweeping the scratch, publishing nothing partial — and then
/// delivers exactly the content the uninterrupted cold pass delivered.
///
/// The child is a second run of this very test binary, driven through the
/// production [`CacheStore`] write path and terminated without unwinding,
/// so what it leaves on disk is what a `kill` during a cache write leaves.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f15_d_retail_restart_after_a_killed_cache_write_recovers_and_reproduces_the_same_state() {
    let world = &worlds()[0];
    let sandbox = Sandbox::new("f15-d-restart");
    let cache_root = sandbox.cache_root();

    // The uninterrupted reference: what this world delivers with no
    // interruption at all.
    let reference = run_load(world, open_store(&cache_root, 64, 8 << 20));
    assert_eq!(reference.usage_entries, closure(world).len() as u64);

    // A second, larger world load is started in a child process and that
    // child is killed while a cache write is in flight. The child's
    // scratch directory and its partial payload survive the kill because
    // nothing had a chance to clean them up.
    let killed = kill_a_child_during_a_cache_write(&cache_root);
    assert!(
        killed.scratch_before_kill > 0,
        "the child was killed with {} scratch write(s) in flight, so the \
         recovery below is measured against a real interruption",
        killed.scratch_before_kill
    );
    assert!(
        !killed.signal.is_empty(),
        "the child was terminated by signal {} rather than exiting cleanly",
        killed.signal
    );

    // The next open: the store sweeps the interrupted write, holds no
    // entry the kill left half-written, and the same load then delivers
    // the same content and the same gameplay state as the reference.
    let restarted = run_load(world, open_store(&cache_root, 64, 8 << 20));
    assert_eq!(
        restarted.recovery.swept_staging, 1,
        "exactly the one interrupted write is swept on the next open"
    );
    assert_eq!(
        restarted.recovery.dropped_corrupt, 0,
        "a swept write is not also counted as a corrupt entry"
    );
    assert_eq!(
        restarted.state.payloads, reference.state.payloads,
        "{}: a restart after a killed cache write changed the content",
        world.spelling
    );
    assert_eq!(
        restarted.state.closure_hash, reference.state.closure_hash,
        "{}: a restart after a killed cache write changed the closure hash",
        world.spelling
    );
    assert_eq!(
        restarted.state.bindings, reference.state.bindings,
        "{}: a restart after a killed cache write changed the gameplay state",
        world.spelling
    );
    assert_eq!(
        restarted.usage_entries, reference.usage_entries,
        "{}: the recovered store holds the same entries",
        world.spelling
    );
    assert_eq!(
        restarted.hits.iter().filter(|hit| **hit).count(),
        reference.usage_entries as usize,
        "{}: every entry the reference published is warm again after the restart",
        world.spelling
    );
}

/// F15 non-negotiable behavior 3 on real content: a committed entry whose
/// payload no longer hashes to what its record declares fails integrity
/// validation, is dropped and rebuilt, and the load delivers the same
/// bytes the cold pass did. Cache corruption cannot change campaign state.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f15_d_retail_corrupt_cache_entry_is_rebuilt_and_never_changes_the_delivered_content() {
    let world = &worlds()[0];
    let sandbox = Sandbox::new("f15-d-corrupt");
    let cache_root = sandbox.cache_root();
    let reference = run_load(world, open_store(&cache_root, 64, 8 << 20));

    // Flip one byte inside one published payload. The entry stays
    // committed and its declared length still matches, so only the digest
    // check can catch it.
    let victim = closure(world)[0]
        .derived
        .clone()
        .expect("a real member has a derived key");
    let payload = open_store(&cache_root, 64, 8 << 20)
        .entries_root()
        .join(victim.digest().to_hex())
        .join(cs_assets::cache::PAYLOAD_FILE);
    let mut bytes = fs::read(&payload).expect("the published payload reads");
    assert!(!bytes.is_empty(), "the victim entry has real bytes");
    bytes[0] ^= 0xFF;
    fs::write(&payload, &bytes).expect("the payload is corrupted");

    let recovered = run_load(world, open_store(&cache_root, 64, 8 << 20));
    // Exactly the corrupted entry is refused and rebuilt; every other
    // entry is untouched and still served from the cache. A store that
    // rebuilt everything, or that served the corrupt bytes, fails here.
    let refused: Vec<&ItemRead> = recovered
        .reads
        .iter()
        .filter(|read| {
            matches!(
                read,
                ItemRead::Rebuilt {
                    cause: cs_app::loading::RebuildCause::Refused { .. },
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        refused.len(),
        1,
        "{}: exactly the one corrupted entry is refused: {:?}",
        world.spelling,
        recovered.reads
    );
    let ItemRead::Rebuilt {
        cause: cs_app::loading::RebuildCause::Refused { code, detail },
        ..
    } = refused[0]
    else {
        unreachable!("the filter kept only refusals")
    };
    assert_eq!(*code, "digest_mismatch", "the refusal names the digest");
    assert!(
        !detail.is_empty(),
        "the refusal says what was wrong, in words"
    );
    assert_eq!(
        recovered.hits.iter().filter(|hit| **hit).count(),
        recovered.reads.len() - 1,
        "{}: the untouched entries are still warm",
        world.spelling
    );
    assert_eq!(
        recovered.state.payloads, reference.state.payloads,
        "cache corruption changed the delivered content"
    );
    assert_eq!(
        recovered.state.closure_hash, reference.state.closure_hash,
        "cache corruption changed the closure hash"
    );
    assert_eq!(
        recovered.state.bindings, reference.state.bindings,
        "cache corruption changed the gameplay state"
    );

    // The rebuild republished the entry, so the next pass is warm again and
    // still equal: corruption is recoverable, not sticky.
    let rewarmed = run_load(world, open_store(&cache_root, 64, 8 << 20));
    assert!(rewarmed.hits.iter().all(|hit| *hit));
    assert_eq!(rewarmed.state.payloads, reference.state.payloads);
}

/// A cache key is the installation hash, every source span hash, the
/// decoder/IR version and the conversion options. Entries planted under
/// any *other* identity — a different installation, a different source
/// span, a different converter version — must be misses, never served
/// bytes, even though each holds a well-formed committed payload.
///
/// Every planted payload differs from the real derived bytes, so an
/// identity component that were ignored would deliver a different content
/// hash and fail these assertions.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f15_d_retail_cache_never_serves_an_entry_under_another_identity() {
    let world = &worlds()[0];
    // The planted entries go into a store that has never held the real
    // closure, so the real load below is cold and every served byte would
    // have to have come from a planted entry.
    let sandbox = Sandbox::new("f15-d-identity");
    let cache_root = sandbox.cache_root();
    let reference_sandbox = Sandbox::new("f15-d-identity-reference");
    let reference = run_load(
        world,
        open_store(&reference_sandbox.cache_root(), 64, 8 << 20),
    );

    let mut store = open_store(&cache_root, 64, 8 << 20);
    let mut planted: Vec<(String, CacheKey)> = Vec::new();
    for (index, item) in closure(world).iter().enumerate() {
        let real = item
            .derived
            .clone()
            .expect("a real member has a derived key");
        let span = span_of(&world.session, item.key.path().as_str());
        // A payload that is plainly not the real derived image: if it were
        // ever served, the delivered digest would differ from the
        // reference.
        let foreign_payload: Vec<u8> = vec![0xA5; 4096 + index];

        // 1. Another installation: the same member, a different install
        //    hash, so every other component of the key is identical.
        let other_install_hash = ContentHash::from_bytes([0x5A; 32]);
        let other_install = CacheKey::new(
            other_install_hash,
            std::slice::from_ref(&span),
            texture_converter(),
            ConversionOptions::none(),
        )
        .expect("a key with an input");
        // 2. Another source span: the same installation and member, one
        //    byte further into the container, so only the input differs.
        let shifted_span = SourceSpan::new(
            span.install_sha256(),
            span.container_path(),
            span.member_key(),
            span.offset() + 1,
            span.length() - 1,
            span.member_sha256(),
        )
        .expect("a valid neighbouring span");
        let other_span = CacheKey::new(
            install_hash(),
            &[shifted_span],
            texture_converter(),
            ConversionOptions::none(),
        )
        .expect("a key with an input");
        // 3. Another converter version: the same inputs, a decoder that
        //    claims a different version of itself.
        let other_decoder = CacheKey::new(
            install_hash(),
            &[span],
            ConverterVersion {
                decoder_version: texture_converter().decoder_version + 1,
                ..texture_converter()
            },
            ConversionOptions::none(),
        )
        .expect("a key with an input");

        for (what, key) in [
            ("installation", other_install),
            ("source span", other_span),
            ("converter", other_decoder),
        ] {
            assert_ne!(
                key.digest(),
                real.digest(),
                "{what}: the planted key must differ from the real one"
            );
            let mut write = store
                .begin_write(&key, foreign_payload.len() as u64)
                .expect("the planted write begins");
            write
                .write_all(&foreign_payload)
                .expect("the planted payload is staged");
            write.seal().expect("the planted write is sealed");
            store.commit(write).expect("the planted entry is published");
            planted.push((format!("{what}:{}", item.key.path()), key));
        }
    }
    assert_eq!(planted.len(), closure(world).len() * 3);
    let planted_entries = store.usage().entries;
    drop(store);

    // The real load runs over the store that now also holds every planted
    // entry. Nothing planted may be served.
    let after = run_load(world, open_store(&cache_root, 64, 8 << 20));
    assert!(
        !after.hits.iter().any(|hit| *hit),
        "{}: an entry planted under another identity was served: {:?}",
        world.spelling,
        after.reads
    );
    assert_eq!(
        after.state.payloads, reference.state.payloads,
        "a planted entry changed the delivered content"
    );
    assert_eq!(
        after.state.closure_hash, reference.state.closure_hash,
        "a planted entry changed the closure hash"
    );
    assert_eq!(
        after.usage_entries,
        planted_entries + closure(world).len() as u64,
        "{}: the real closure republished beside the planted entries",
        world.spelling
    );
}

/// A derived cache is an optimization, never the authoritative data
/// source. With a budget too small for the whole closure the store evicts
/// and some items cannot be cached at all — and the delivered content and
/// gameplay state are still exactly the reference's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f15_d_retail_budget_eviction_never_changes_the_delivered_content() {
    let world = &worlds()[0];
    let sandbox = Sandbox::new("f15-d-budget");
    let reference_sandbox = Sandbox::new("f15-d-budget-reference");
    let reference = run_load(
        world,
        open_store(&reference_sandbox.cache_root(), 64, 8 << 20),
    );

    // One entry's worth of room in a *cold* store, so the closure really
    // cannot fit and the load has to go to its sources for most items.
    let one_entry_bytes = reference.usage_bytes / reference.usage_entries.max(1);
    let starved = run_load(
        world,
        open_store(&sandbox.cache_root(), 1, one_entry_bytes + 1),
    );

    assert!(
        starved.usage_entries < reference.usage_entries,
        "the starved store must actually be short of room: {} entries vs {}",
        starved.usage_entries,
        reference.usage_entries
    );
    assert!(
        starved.reads.iter().any(|read| !read.is_cache_hit()),
        "the starved load must actually miss the cache somewhere"
    );
    assert_eq!(
        starved.state.payloads, reference.state.payloads,
        "an evicted or uncached derived asset changed the delivered content"
    );
    assert_eq!(
        starved.state.closure_hash, reference.state.closure_hash,
        "an evicted or uncached derived asset changed the closure hash"
    );
    assert_eq!(
        starved.state.bindings, reference.state.bindings,
        "an evicted or uncached derived asset changed the gameplay state"
    );
}

// ------------------------------------------- the killed child, for restart ---

/// The environment the parent hands the child so the child knows which
/// private store to write into and that it is the child.
const CHILD_STORE_VAR: &str = "CS_F15_D_CHILD_STORE";
/// The test the parent re-runs in the child.
const CHILD_TEST: &str = "f15_d_child_killed_mid_cache_write_is_not_an_acceptance_test";

/// What the parent observed about the child it killed.
struct Killed {
    /// How many scratch write directories the child held when it died.
    scratch_before_kill: usize,
    /// The signal the child was terminated by, or empty if it exited.
    signal: String,
}

/// Counts the scratch directories of an interrupted write under `root`.
///
/// The name is the store's own [`cs_assets::cache::STAGING_DIR`]: the
/// parent reads the directory the child left *without* opening a store,
/// because opening one is what sweeps it.
fn staging_dirs(cache_root: &Path) -> usize {
    fs::read_dir(cache_root.join(cs_assets::cache::STAGING_DIR))
        .map(|entries| entries.count())
        .unwrap_or(0)
}

/// Runs a real cache write in a child process and kills that child while
/// the write is in flight.
///
/// The child is this same test binary, re-run on [`CHILD_TEST`] with the
/// store's path in the environment. It drives the production
/// [`CacheStore::begin_write`]/`write_all` path against the real derived
/// payload of the real installation and then terminates itself without
/// unwinding, so its scratch directory and partial payload are left
/// exactly as a `kill` during a cache write would leave them.
fn kill_a_child_during_a_cache_write(cache_root: &Path) -> Killed {
    let executable = std::env::current_exe().expect("this test binary's own path");
    let mut child = Command::new(executable)
        .arg("--ignored")
        .arg("--exact")
        .arg(CHILD_TEST)
        .arg("--nocapture")
        .env(CHILD_STORE_VAR, cache_root)
        .env("CS_GAME_DIR", game_dir())
        // The child's own harness output is not this test's evidence; the
        // parent reports what it measured about the kill itself.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the child test binary starts");

    // Wait for the child to be holding a write in flight. The child holds
    // its scratch directory from `begin_write` until it dies, so its
    // appearance is the signal that the write is under way.
    //
    // The window is generous because the child is a fresh process: it
    // discovers and hashes the whole installation itself before it derives
    // anything, which is tens of seconds of real IO.
    let deadline = Instant::now() + std::time::Duration::from_secs(600);
    let mut scratch_before_kill = 0;
    while Instant::now() < deadline {
        scratch_before_kill = staging_dirs(cache_root);
        if scratch_before_kill > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // SIGKILL: no unwinding, no destructors, no flush.
    #[cfg(unix)]
    let signal = {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(child.id().to_string())
            .status();
        String::from("KILL")
    };
    #[cfg(not(unix))]
    let signal = {
        let _ = child.kill();
        String::from("kill")
    };
    let status = child.wait().expect("the killed child is reaped");
    assert!(
        !status.success(),
        "the child was expected to die, not to exit successfully: {status}"
    );
    assert!(
        scratch_before_kill > 0,
        "the child never held a cache write in flight, so nothing was interrupted"
    );
    // The scratch the child left is still there: it had no chance to clean
    // it up, which is the whole point.
    assert_eq!(
        staging_dirs(cache_root),
        scratch_before_kill,
        "the kill left the interrupted write on disk"
    );
    Killed {
        scratch_before_kill,
        signal,
    }
}

/// The child half of the restart scenario.
///
/// It is deliberately **not** named `accept_f15_d_*`, so the task's
/// acceptance selection (`accept_f15_d_`) never discovers it: it is a
/// harness component that exists to be killed, not a claim of its own. All
/// of its assertion power lives in the parent, which measures what the
/// kill left behind and what the next open recovered.
///
/// Run on its own — which is what a bare `--include-ignored` on this binary
/// does — it has no store to write into and says so instead of pretending
/// to have been interrupted.
#[test]
#[ignore = "child process of accept_f15_d_retail_restart_after_a_killed_cache_write_recovers_and_reproduces_the_same_state; run through it"]
fn f15_d_child_killed_mid_cache_write_is_not_an_acceptance_test() {
    let Some(store_path) = std::env::var_os(CHILD_STORE_VAR) else {
        println!(
            "{CHILD_TEST}: not running as the child of the restart scenario \
             ({CHILD_STORE_VAR} is unset), so there is no cache write to \
             interrupt. Run accept_f15_d_retail_restart_after_a_killed_cache_write_\
             recovers_and_reproduces_the_same_state, which starts and kills it."
        );
        return;
    };
    let store_path = PathBuf::from(store_path);
    let world = &worlds()[0];
    let item = closure(world)[0].clone();
    let key = item
        .derived
        .clone()
        .expect("a real member has a derived key");

    // Derive the real payload once, from the real installation, so the
    // child stages real bytes rather than filler.
    let session_asset = world
        .session
        .resolve(&item.key)
        .expect("the archive resolves in the child too");
    let source = world
        .session
        .read_all(&session_asset)
        .expect("the archive reads in the child too");
    let payload =
        convert_texture_archive(&item, &CanonicalPayload::new(item.content.kind(), source))
            .expect("the real conversion runs in the child too");
    assert!(!payload.is_empty(), "the derived payload has real bytes");

    let store = open_store(&store_path, 64, 8 << 20);
    let mut write = store
        .begin_write(&key, payload.len() as u64)
        .expect("the child's cache write begins");
    // A partial write: the bytes on disk do not match the declared length
    // and the record still says `Writing`, so this is exactly the
    // "partially written entry" the next open has to refuse.
    write
        .write_all(&payload[..payload.len() / 2])
        .expect("the child stages part of the payload");
    // Hold the write in flight so the parent's kill lands inside it
    // rather than after it. The store is never sealed and never committed,
    // so nothing the child staged is ever servable.
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    while Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Only reached if the parent never killed the child, in which case
    // this is a harness failure rather than a pass.
    std::process::abort();
}
