//! Acceptance scenario F15-E: spec F15 **AC04** ("warm and cold loads
//! produce equal content hashes and gameplay state") measured over a
//! **mission dependency closure**, not a hand-picked asset list.
//!
//! # What this file extends
//!
//! F15-D measured AC04 over each world group's own texture archives — a
//! closure F15-D's own evidence report names as *not* a mission's dependency
//! closure (`docs/findings/2026-09-30-t64-f15-d-cold-warm-restart.md`). This
//! stage runs the same production load path over the closure a **campaign
//! mission** really has, resolved through the production dependency-closure
//! algorithm of `docs/contracts/IDENTITY-CONTENT.md`:
//!
//! * the closure of every launchable campaign mission is computed by the
//!   production catalog builder and closure walker
//!   (`cs_content::catalog::retail_baseline` + `Closure::compute`, the very
//!   code the production `cs-inspect closure` CLI runs), and
//!   [`accept_f15_e_mission_closures_resolve_through_the_production_closure_and_cli`]
//!   cross-checks that closure against the CLI's own report for the
//!   reference mission, so the closure is measured, never hand-picked;
//! * every reached closure node is resolved through the mounted original
//!   installation and its resolved span is compared with the catalog row's
//!   own origin span, so the bytes loaded are the bytes the closure names;
//! * the conversion is the real reader-archive structural decode
//!   ([`common::derive_reader_archive_listing`], the F06 dispatch →
//!   version-one trailer index → reader-archive reader pipeline), and the
//!   consumer is the real [`LoadingSession`] over a real [`CacheStore`],
//!   delivering into a real [`World`], exactly as in F15-D.
//!
//! # Naming what the closure reaches but the stage cannot serve
//!
//! [`accept_f15_e_retail_mission_closure_cold_warm_restart_deliver_equal_content_and_gameplay_state`]
//! prints, per mission and per closure node, whether the node's bytes were
//! delivered and at which level, or why the node is not served — and asserts
//! the accounting is total (every node is either served or named), so no
//! closure member is skipped silently (task F15-E AC3). A closure node with
//! no runtime consumer is reported as a **finding**, never as a pass: the
//! closure's readiness is measured by the catalog and nothing here awards
//! it.
//!
//! # Sensitivity
//!
//! Every test removes the implementation and fails: the closure walker gone
//! and the CLI cross-check does not match; the session's resolution gone and
//! a closure node's span is not served; `CacheStore::begin_read` /
//! `verify_entry` gone and the warm pass is not a cache hit; `commit` gone
//! and nothing is warm; the [`ReadyBundle::closure_hash`] /
//! [`LoadedItemBinding`] fields gone and the comparisons do not compile; the
//! reader-archive pipeline gone and the conversion fails, so no load ever
//! reaches `Ready`; a closure walker that stopped expanding the mission's
//! edges fails the degenerate-closure guard, so the stage can never silently
//! shrink to measuring the launchable root alone. The expected values are
//! each pass's own measured output,
//! and every retail input is hashed at run time from the mounted
//! installation.
//!
//! Retail tests are `#[ignore = "requires CS_GAME_DIR"]` because CI has no
//! original data; they fail loudly (never vacuously) when the variable is
//! unset. Nothing is written inside the installation and nothing derived
//! from it is committed.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use bevy::ecs::world::World;

use cs_app::assets::{CanonicalPayload, ConversionError};
use cs_app::loading::{
    Criticality, ItemRead, LoadItem, LoadRequest, LoadState, LoadTarget, LoadedItemBinding,
    LoadingSession, SessionIo,
};
use cs_assets::cache::{CacheBudget, CacheDirectory, CacheKey, CacheStore, ConversionOptions};
use cs_assets::vfs::{ContentSession, INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::catalog::baseline::{Baseline, retail_baseline};
use cs_content::catalog::closure::{Closure, CompatibilityOptions};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan,
};
use cs_types::content::{ContentId, ContentKind, Origin};

// --------------------------------------------------------------- inputs ---

/// The original installation. Unset means this machine has no `retail`
/// capability and the retail tests below must not run at all.
fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The production baseline of the installation: every catalog row, the
/// declared launchable roots and the measured fingerprints. Built once for
/// the whole binary; building it discovers and hashes the installation.
fn baseline() -> &'static Baseline {
    static BASELINE: OnceLock<Baseline> = OnceLock::new();
    BASELINE.get_or_init(|| {
        retail_baseline(&game_dir()).expect("the original installation builds its baseline")
    })
}

/// The installation's own identity, as the baseline measured it.
fn install_hash() -> cs_types::evidence::ContentHash {
    cs_types::evidence::ContentHash::from_hex(&baseline().install_sha256)
        .expect("the baseline's installation fingerprint is a canonical sha256")
}

/// Every launchable **campaign mission** of this installation, in the
/// baseline's declared order.
fn mission_roots() -> Vec<ContentId> {
    baseline()
        .roots
        .iter()
        .filter(|id| id.kind() == cs_types::content::ContentKind::Mission)
        .cloned()
        .collect()
}

/// One content session holding the whole installation under the designed
/// baseline's install mount (`SessionBuilder::mount_installation`'s install
/// half: [`PrecedenceClass::Shared`], namespace [`INSTALL_NAMESPACE`], every
/// file keyed by its own installation-relative spelling). A closure node's
/// bytes are then resolved by the span the catalog row itself names — no
/// filename guessing, one key, one file.
fn session() -> &'static ContentSession {
    static SESSION: OnceLock<ContentSession> = OnceLock::new();
    SESSION.get_or_init(|| {
        let mount = MountBuilder::new(
            MountId::new("install").expect("a valid mount id"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
            PrecedenceClass::Shared,
            ".",
        )
        .retail();
        let mut builder = SessionBuilder::new(ResolveContext::new(install_hash()));
        builder
            .mount_directory(mount, &game_dir())
            .expect("the installation mounts");
        let session = builder.open();
        assert!(
            session.rejected().is_empty(),
            "the installation holds entries the mount refused: {:?}",
            session.rejected()
        );
        session
    })
}

// ------------------------------------------------- the mission closures ---

/// One campaign mission's dependency closure over the original installation,
/// with the load items the closure resolves to and the accounting of every
/// reached node.
struct MissionClosure {
    /// The launchable root, e.g. `mission/ch1-m01`.
    root: ContentId,
    /// The computed closure itself, the production walker's own output.
    closure: Closure,
    /// One load item per **unique source span** the closure reaches: three
    /// closure nodes (mission, program, install file) can name the same
    /// bytes, and the load serves those bytes once under the install-file
    /// row's identity — the deduplication is measured, and the per-node
    /// accounting below records which node each item stands for.
    items: Vec<LoadItem>,
    /// Per closure node: its id, its kind, and either the index of the load
    /// item that serves its bytes or the reason the node is not served.
    accounting: Vec<NodeAccounting>,
}

/// How one reached closure node was accounted for.
struct NodeAccounting {
    /// The catalog id of the node.
    id: String,
    /// The node's content kind.
    kind: String,
    /// The item index serving the node's bytes, when the node is served.
    served_by: Option<usize>,
    /// Why the node is not served, in words, when it is not.
    unserved: Option<String>,
    /// Whether the catalog itself calls this node ready.
    ready: bool,
}

/// The `SourceSpan` identity a deduplication groups spans by.
fn span_identity(span: &SourceSpan) -> (String, u64, u64) {
    (
        span.container_path().to_ascii_lowercase(),
        span.offset(),
        span.length(),
    )
}

/// The load item for one unique closure span, derived under the real
/// reader-archive listing converter and the span the session really serves.
///
/// `key_path` is the catalog row's own container path — the installation
/// spelling the closure names — and the span put into the cache key is the
/// **session's own resolution** of that path, which the caller has already
/// checked agrees with the row on member, offset and length — so the key
/// names the bytes the closure declared, identity included.
fn closure_item(node_id: &ContentId, key_path: &str, span: &SourceSpan) -> LoadItem {
    let key = AssetKey::from_spelling(INSTALL_NAMESPACE, key_path, "default")
        .expect("a catalog container path is a valid asset key");
    LoadItem::new(
        key,
        node_id.clone(),
        Criticality::GameplayCritical,
        span.length(),
    )
    .expect("a real closure member has bytes")
    .with_derived(
        CacheKey::new(
            install_hash(),
            std::slice::from_ref(span),
            common::derived_reader_listing_converter(),
            ConversionOptions::none(),
        )
        .expect("a key with an input"),
    )
}

/// The nodes sharing one unique source span, keyed by the span identity.
type SpanGroup<'a> = ((String, u64, u64), Vec<&'a ContentId>);

/// Computes every campaign mission's closure and resolves each reached node
/// to load items, once for the whole binary.
fn mission_closures() -> &'static Vec<MissionClosure> {
    static CLOSURES: OnceLock<Vec<MissionClosure>> = OnceLock::new();
    CLOSURES.get_or_init(|| {
        let catalog = &baseline().catalog;
        mission_roots()
            .into_iter()
            .map(|root| {
                let closure = Closure::compute(
                    catalog,
                    std::slice::from_ref(&root),
                    CompatibilityOptions::default(),
                )
                .unwrap_or_else(|error| panic!("{root}: the closure computes: {error}"));

                // One group of nodes per unique span, in node-id order.
                let mut groups: Vec<SpanGroup<'_>> = Vec::new();
                for node in closure.node_ids() {
                    let element = catalog
                        .get(node)
                        .unwrap_or_else(|| panic!("{node}: a reached node has a catalog row"));
                    let Origin::Installation { source } = &element.origin else {
                        continue;
                    };
                    let identity = span_identity(source);
                    match groups.iter_mut().find(|(seen, _)| *seen == identity) {
                        Some((_, members)) => members.push(node),
                        None => groups.push((identity, vec![node])),
                    }
                }

                let mut items: Vec<LoadItem> = Vec::new();
                let mut accounting: Vec<NodeAccounting> = Vec::new();
                let mut served: Vec<NodeAccounting> = Vec::new();
                let mut unserved: Vec<NodeAccounting> = Vec::new();
                for node in closure.node_ids() {
                    let element = catalog.get(node).expect("a reached node has a catalog row");
                    let record = NodeAccounting {
                        id: node.as_str().to_owned(),
                        kind: element.kind.label().to_owned(),
                        served_by: None,
                        unserved: None,
                        ready: closure.is_ready(node),
                    };
                    match &element.origin {
                        Origin::Installation { .. } => served.push(record),
                        origin => {
                            let mut record = record;
                            record.unserved = Some(format!(
                                "origin {} names no installation bytes",
                                origin.label()
                            ));
                            unserved.push(record);
                        }
                    }
                }
                for (identity, members) in &groups {
                    // The span every member of the group declared; they are
                    // equal on (container, offset, length) by construction.
                    let first = catalog.get(members[0]).expect("a reached node has a row");
                    let Origin::Installation { source } = &first.origin else {
                        unreachable!("the grouping only holds installation origins")
                    };
                    // Resolve the span through the mounted installation and
                    // check it is the bytes the closure named. The mount is
                    // the designed install mount (container label `.`), so
                    // the member key — not the container label — carries the
                    // installation-relative path the catalog row names.
                    let key = AssetKey::from_spelling(
                        INSTALL_NAMESPACE,
                        source.container_path(),
                        "default",
                    )
                    .expect("a catalog container path is a valid asset key");
                    let resolved = session()
                        .resolve(&key)
                        .unwrap_or_else(|error| panic!("{identity:?}: the span resolves: {error}"))
                        .resolved()
                        .span
                        .clone();
                    assert_eq!(
                        resolved
                            .member_key()
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                        source.container_path().to_ascii_lowercase(),
                        "{}: the closure names {} but the session resolved {}[{}]",
                        members[0],
                        source.container_path(),
                        resolved.container_path(),
                        resolved.member_key().unwrap_or_default()
                    );
                    assert_eq!(
                        resolved.offset(),
                        source.offset(),
                        "{}: the resolved offset differs from the closure's",
                        members[0]
                    );
                    assert_eq!(
                        resolved.length(),
                        source.length(),
                        "{}: the resolved length differs from the closure's",
                        members[0]
                    );

                    // The install-file row is the row that *is* the bytes;
                    // its identity heads the item. Without one, the first
                    // node in sorted order does.
                    let head = members
                        .iter()
                        .find(|id| {
                            catalog
                                .get(id)
                                .map(|element| {
                                    element.kind == cs_types::content::ContentKind::InstallFile
                                })
                                .unwrap_or(false)
                        })
                        .copied()
                        .unwrap_or(members[0]);
                    let head = ContentId::parse(head.as_str()).expect("a catalog id parses");
                    let index = items.len();
                    items.push(closure_item(&head, key.path().as_str(), &resolved));
                    for record in &mut served {
                        if members.iter().any(|member| member.as_str() == record.id) {
                            record.served_by = Some(index);
                        }
                    }
                }
                accounting.extend(unserved);
                accounting.extend(served);
                accounting.sort_by(|left, right| left.id.cmp(&right.id));

                MissionClosure {
                    root,
                    closure,
                    items,
                    accounting,
                }
            })
            .collect()
    })
}

// --------------------------------------------------------- what a load did ---

/// The gameplay state one delivered load leaves behind, compared across
/// passes exactly as in F15-D: it must not contain the load identity.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GameplayState {
    /// The closure hash the bundle was versioned with.
    closure_hash: cs_types::evidence::ContentHash,
    /// Sorted `(content id, asset key)` pairs of the delivered bindings.
    bindings: Vec<(String, String)>,
    /// Sorted `(asset key, payload digest)` pairs.
    payloads: Vec<(String, cs_types::evidence::ContentHash)>,
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
    /// What opening the store recovered, if this pass opened it.
    recovery: cs_assets::cache::RecoveryReport,
}

/// The real conversion for this stage: the production reader-archive
/// structural decode, shared with the evidence harness through `common`.
fn convert_archive(
    item: &LoadItem,
    payload: &CanonicalPayload,
) -> Result<Vec<u8>, ConversionError> {
    common::derive_reader_archive_listing(item.key.path().as_str(), payload.bytes())
        .map_err(|detail| ConversionError::Failed { detail })
}

/// Opens a private store over `cache_root`, outside the installation.
fn open_store(cache_root: &Path, entries: u64, bytes: u64) -> CacheStore {
    CacheStore::open(
        CacheDirectory::open(cache_root, &game_dir())
            .expect("a cache root outside the installation"),
        CacheBudget::new(entries, bytes).expect("a nonzero budget"),
    )
    .expect("the private store opens")
}

/// Runs one full load of one mission's closure over `store` and hands it to
/// a fresh world, pumped item by item exactly as the windowed loop pumps.
fn run_load(mission: &MissionClosure, store: CacheStore) -> Outcome {
    let expected_items = mission.items.len();
    assert!(expected_items > 0, "an empty closure proves nothing");
    let request = LoadRequest {
        session: session().generation(),
        target: LoadTarget::shared(),
        items: mission.items.clone(),
    };
    let mut load = LoadingSession::new(request, store);
    let recovery = *load.driver().store().recovery();
    let mut reads: Vec<ItemRead> = Vec::new();
    {
        let mut io = SessionIo::new(session(), convert_archive);
        load.begin().expect("the load begins");
        while let Some(read) = load.pump(&mut io).expect("one bounded step of the load") {
            reads.push(read);
        }
    }
    assert_eq!(
        reads.len(),
        expected_items,
        "{}: every item settles exactly once",
        mission.root
    );
    assert_eq!(
        load.state(),
        LoadState::Ready,
        "{}: {:?}",
        mission.root,
        load.failures()
    );

    let bundle = load
        .driver()
        .transaction()
        .ready_bundle()
        .expect("a Ready transaction has a bundle");
    let closure_hash = bundle.closure_hash();
    let mut payloads: Vec<(String, cs_types::evidence::ContentHash)> = bundle
        .items()
        .iter()
        .map(|item| (item.key.path().as_str().to_owned(), item.payload_sha256))
        .collect();
    payloads.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        payloads.len(),
        expected_items,
        "{}: the bundle carries the whole closure",
        mission.root
    );
    assert!(
        bundle.omitted().is_empty(),
        "{}: nothing was deferred: {:?}",
        mission.root,
        bundle.omitted()
    );

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
        recovery,
    }
}

// ------------------------------------------------------------- the tests ---

/// A private, empty cache root for one pass, removed again afterwards.
struct Sandbox {
    /// Kept alive so the private cache is removed again when the test ends.
    _tree: common::TempTree,
    root: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let tree = common::TempTree::new(label);
        let root = tree.root().join("cache");
        fs::create_dir_all(&root).expect("the private cache root is created");
        Self { _tree: tree, root }
    }

    fn cache_root(&self) -> PathBuf {
        self.root.clone()
    }
}

/// The production `cs-inspect` binary, located next to this test binary
/// (`target/debug/deps/<test>` → `target/debug/cs-inspect`), exactly as the
/// campaign evidence sequence locates it.
fn cs_inspect_binary() -> PathBuf {
    let mut binary = std::env::current_exe().expect("this test binary has a path");
    binary.pop(); // `deps/`
    binary.pop(); // the target profile directory
    let binary = binary.join("cs-inspect");
    assert!(
        binary.is_file(),
        "{} is missing: run `cargo build -p cs_inspect --locked` first",
        binary.display()
    );
    binary
}

/// A fast smoke check of this stage's conversion on the reference archive,
/// runnable without the (slow) catalog baseline: the production reader
/// pipeline must decode the mission program archive's member index. Not
/// named `accept_f15_e_*` — it is a regression guard for the derived form,
/// not one of the stage's acceptance scenarios.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn f15_e_derive_reader_listing_reads_the_reference_mission_program_archive() {
    let bytes = fs::read(game_dir().join("ZBD/C1C/M01/zrdr.zbd"))
        .expect("the reference mission's reader archive reads");
    let derived = common::derive_reader_archive_listing("ZBD/C1C/M01/zrdr.zbd", &bytes)
        .expect("the production reader pipeline decodes the member index");
    assert!(
        derived.len() > 4,
        "the derived listing carries the member count and more"
    );
    let count = u32::from_le_bytes(
        derived[0..4]
            .try_into()
            .expect("the member count is four bytes"),
    );
    assert!(
        count > 0,
        "the reference mission's reader archive declares members"
    );
    println!("F15-E DERIVE: zrdr.zbd lists {count} member(s)");
}

/// AC1 and AC6, the resolution half: every campaign mission's closure is
/// computed by the production algorithm, every reached node's bytes resolve
/// through the mounted installation to the exact span the closure names, and
/// the CLI's own report for the reference mission agrees node for node and
/// hash for hash with the in-process computation — so the load items the
/// next test serves are the closure's, not a hand-picked list.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f15_e_mission_closures_resolve_through_the_production_closure_and_cli() {
    let closures = mission_closures();
    let roots = mission_roots();
    assert_eq!(
        closures.len(),
        roots.len(),
        "every launchable campaign mission has a closure"
    );
    assert!(
        !closures.is_empty(),
        "the baseline declares campaign missions; an empty denominator proves nothing"
    );

    for mission in closures {
        assert!(
            !mission.items.is_empty(),
            "{}: the closure reaches at least one servable span",
            mission.root
        );
        // AC1 names a **mission dependency closure**, and a degenerate one
        // would make this stage vacuous: if the production walker ever
        // stopped expanding the mission's edges, the closure would shrink to
        // the launchable root alone, every assertion here would still hold,
        // and AC04 would be measured over a single row instead of a closure.
        // The production catalog reaches mission -> program -> install-file
        // for every campaign mission today (recorded in the F15-E findings
        // doc), so pin that as a lower bound — never an exact count, which
        // F50-B/M01-B will legitimately grow once mission->content edges land.
        let kinds: Vec<ContentKind> = mission
            .closure
            .node_ids()
            .iter()
            .filter_map(|id| baseline().catalog.get(id).map(|element| element.kind))
            .collect();
        for required in [
            ContentKind::Mission,
            ContentKind::Script,
            ContentKind::InstallFile,
        ] {
            assert!(
                kinds.contains(&required),
                "{}: the closure must reach a {required:?} node, else AC04 is measured over a \
                 degenerate closure (reached kinds: {kinds:?})",
                mission.root,
            );
        }
        // AC3's total accounting: every reached node is either served by an
        // item or explicitly named with a reason.
        let nodes = mission.closure.node_ids().len();
        let accounted: Vec<&NodeAccounting> = mission.accounting.iter().collect();
        assert_eq!(
            accounted.len(),
            nodes,
            "{}: every reached node is accounted for exactly once",
            mission.root
        );
        for record in &accounted {
            match record.served_by {
                Some(index) => assert!(
                    index < mission.items.len(),
                    "{}: {} names an item that exists",
                    mission.root,
                    record.id
                ),
                None => assert!(
                    record.unserved.is_some(),
                    "{}: {} is not served and must say why",
                    mission.root,
                    record.id
                ),
            }
            if record.unserved.is_some() {
                println!(
                    "F15-E NOT SERVED: {} node {} ({}) — {}",
                    mission.root,
                    record.id,
                    record.kind,
                    record.unserved.as_deref().unwrap_or_default()
                );
            }
            if !record.ready {
                println!(
                    "F15-E FINDING (no runtime consumer / not ready): {} node {} ({})",
                    mission.root, record.id, record.kind
                );
            }
        }
        println!(
            "F15-E CLOSURE: {} hash {} nodes {} items {}",
            mission.root,
            mission.closure.hash().to_hex(),
            nodes,
            mission.items.len()
        );
    }

    // The CLI cross-check: the production `cs-inspect closure` report for
    // the reference mission must carry the same hash and the same node ids
    // the in-process computation above used to build the load items.
    let reference = ContentId::parse("mission/ch1-m01").expect("the reference mission id parses");
    let mine = closures
        .iter()
        .find(|mission| mission.root == reference)
        .expect("the reference mission is a launchable root");
    let tree = common::TempTree::new("f15-e-closure-cli");
    let out = tree.root().join("closure.json");
    let run = Command::new(cs_inspect_binary())
        .args([
            "closure",
            "--cs-path",
            game_dir().to_str().expect("a UTF-8 installation path"),
            "--mission",
            reference.as_str(),
            "--out",
            out.to_str().expect("a UTF-8 temporary path"),
        ])
        .output()
        .expect("cs-inspect runs");
    assert_eq!(
        run.status.code(),
        Some(0),
        "cs-inspect closure exited with {:?}: {}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    let report = fs::read_to_string(&out).expect("the CLI wrote its report");
    assert!(
        report.contains(&format!("\"hash\":\"{}\"", mine.closure.hash().to_hex())),
        "the CLI report disagrees with the in-process closure hash {}",
        mine.closure.hash().to_hex()
    );
    for node in mine.closure.node_ids() {
        assert!(
            report.contains(&format!("\"id\":\"{}\"", node.as_str())),
            "the CLI report does not reach {}",
            node.as_str()
        );
    }
    assert_eq!(
        report.matches("\"own_ready\":").count(),
        mine.closure.node_ids().len(),
        "the CLI report holds exactly the reached nodes"
    );
    println!(
        "F15-E CLI CROSS-CHECK: {} agrees with cs-inspect closure, hash {}",
        reference,
        mine.closure.hash().to_hex()
    );
}

/// AC04 and the cold/warm/restart cycle of this stage's title, over **every**
/// campaign mission's dependency closure: each mission's closure is loaded
/// three times over one private store — cold against an empty store, warm
/// against the re-opened store, and again after that store is closed and
/// re-opened as a fresh process would — and all three passes must deliver
/// the same per-item payload digests, the same [`ReadyBundle::closure_hash`]
/// and the same gameplay state.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f15_e_retail_mission_closure_cold_warm_restart_deliver_equal_content_and_gameplay_state()
{
    let installation = install_hash().to_hex();
    let closures = mission_closures();
    assert!(
        !closures.is_empty(),
        "the reference installation {installation} declares campaign missions"
    );

    for mission in closures {
        let sandbox = Sandbox::new("f15-e-cycle");
        let items = mission.items.len();

        // Cold: nothing cached, every item read from its own source span and
        // converted by the real reader-archive decode.
        let cold = run_load(mission, open_store(&sandbox.cache_root(), 64, 8 << 20));
        assert!(
            !cold.hits.iter().any(|hit| *hit),
            "{}: a cold load must not be served from the cache",
            mission.root
        );
        for read in &cold.reads {
            assert!(
                matches!(read, ItemRead::Rebuilt { .. }),
                "{}: cold item: {read:?}",
                mission.root
            );
        }
        assert_eq!(
            cold.usage_entries, items as u64,
            "{}: every derived closure member reached the store",
            mission.root
        );
        assert_ne!(
            cold.state.closure_hash.to_hex(),
            "0".repeat(64),
            "{}: the closure hash is measured, not empty",
            mission.root
        );

        // Warm: the store re-opened, its recovery scan run, the whole
        // closure served from verified entries.
        let warm = run_load(mission, open_store(&sandbox.cache_root(), 64, 8 << 20));
        assert_eq!(
            warm.recovery.swept_staging, 0,
            "{}: a clean shutdown leaves no scratch behind",
            mission.root
        );
        assert_eq!(
            warm.recovery.dropped_corrupt, 0,
            "{}: a clean shutdown leaves no corrupt entry behind",
            mission.root
        );
        assert!(
            warm.hits.iter().all(|hit| *hit),
            "{}: a warm load must be served entirely from the cache: {:?}",
            mission.root,
            warm.reads
        );

        // Restart: the store closed and re-opened once more, exactly as a
        // fresh process re-opening the private cache would.
        let restarted = run_load(mission, open_store(&sandbox.cache_root(), 64, 8 << 20));
        assert_eq!(
            restarted.recovery.swept_staging, 0,
            "{}: the second clean shutdown leaves no scratch behind",
            mission.root
        );
        assert!(
            restarted.hits.iter().all(|hit| *hit),
            "{}: a restarted load must be served entirely from the cache: {:?}",
            mission.root,
            restarted.reads
        );

        // AC04: equal content hashes and equal gameplay state, pass over
        // pass over pass.
        assert_eq!(
            warm.state.payloads, cold.state.payloads,
            "{}: warm and cold deliver different content hashes",
            mission.root
        );
        assert_eq!(
            warm.state.closure_hash, cold.state.closure_hash,
            "{}: warm and cold deliver different closure hashes",
            mission.root
        );
        assert_eq!(
            warm.state.bindings, cold.state.bindings,
            "{}: warm and cold deliver different gameplay state",
            mission.root
        );
        assert_eq!(
            restarted.state.payloads, cold.state.payloads,
            "{}: a restart changed the delivered content",
            mission.root
        );
        assert_eq!(
            restarted.state.closure_hash, cold.state.closure_hash,
            "{}: a restart changed the closure hash",
            mission.root
        );
        assert_eq!(
            restarted.state.bindings, cold.state.bindings,
            "{}: a restart changed the gameplay state",
            mission.root
        );
        assert_eq!(
            warm.usage_entries, cold.usage_entries,
            "{}: a warm load must not republish what it read",
            mission.root
        );

        println!(
            "F15-E CYCLE: {} closure {} delivered {} item(s), cold==warm==restart, payload \
             digests [{}]",
            mission.root,
            mission.closure.hash().to_hex(),
            items,
            cold.state
                .payloads
                .iter()
                .map(|(key, digest)| format!("{key}={}", digest.to_hex()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
