//! Acceptance stage F45-C: the construction, flight check, loading and return
//! flows wired into their real producers and consumers
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, section
//! `### F45-C`). Task test prefix: `accept_f45_c_`. Shared contracts:
//! `docs/contracts/UI-NETWORK.md` and `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The minimum scenario is AC03 — **retry a failed load after repairing its
//! dependency without restarting the app** — driven through
//! [`FrontEndFlow`]: the load is the real `crate::loading::LoadingSession`
//! pumped through the production `SessionIo` over a content session mounted on
//! a fixture installation, its missing dependency fails the load, and the same
//! process loads it again once the file is there.
//!
//! Everything else here is that same wiring seen from the other three flows:
//! a construction commit that reaches the F44 economy *and* the profile save
//! (and a refusal that reaches neither the screen nor the save), a flight-check
//! commit that resolves the launch through the F25 roster and refuses an
//! airframe the hangar may not select, and a return walk whose outcome is
//! applied and saved through F43's campaign run, whose abort changes nothing,
//! and which reopens the profile in the same process.
//!
//! All data is authored fixture data written under the system temporary
//! directory: no original file, price, coordinate or screen is read, and
//! nothing here proves anything about retail content (F45-D's
//! `retail,gpu` stage captures that).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::campaign::{campaign_profile_id, lower_campaign, write_snapshot};
use cs_app::construction::ConstructionScreen;
use cs_app::loading::{Criticality, LoadItem, LoadTarget};
use cs_app::profile::ProfileSession;
use cs_app::ui::front_end::{
    Action, AudioScope, ConstructionDraft, Effect, FlowError, FlowSetup, FrontEndFlow,
    InputContext, LoadError, LoadPlan, LoadVerdict, Loadout, Resource, ResourceLedger,
    ResourceProblem, Screen, ScreenSessionError,
};
use cs_assets::cache::{CacheBudget, CacheDirectory, CacheStore};
use cs_assets::vfs::{ContentSession, MountBuilder, SessionBuilder};
use cs_content::airframe_roles::{LaunchAssignmentError, declared_synthetic_roles};
use cs_content::campaign::{
    CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId, EdgeCondition,
    NodeKind, RewardSpec, RosterEntry,
};
use cs_content::construction::{
    AircraftBlueprint, ArmorFitment, SYNTHETIC_HEAVY_PLATE_KEY, SYNTHETIC_OTHER_AIRFRAME_KEY,
    declared_synthetic_blueprint, declared_synthetic_price_book, synthetic_armor_fitments,
    synthetic_boundary_rules, synthetic_policy,
};
use cs_content::save::settings::SettingCatalog;
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, EventKey,
    MissionOutcome, Outcome as MissionResult, OutcomeAuthority, OutcomeId,
    ProfileId as CampaignProfileId, SessionGeneration, SymbolId,
};
use cs_types::Tick;
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};
use cs_types::profile::{ProfileId as ProfileSlot, ProfileKind};

use super::screens::{SURFACE, button_point, preflight_deck};

/// The synthetic budget one win grants: far above the synthetic prices, so a
/// commit's affordability is never the thing under test.
const GRANT: u64 = 1_000_000;
/// The world group the fixture content session mounts.
const WORLD: &str = "zbd/c1";

// --- fixture -------------------------------------------------------------

/// A unique, disposable directory under the system temporary directory.
struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f45-c-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f45c.fixture"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed()))
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node(key: &str) -> CampaignNodeId {
    CampaignNodeId::new(key).expect("valid node id")
}

/// `m01 --Victory(+grant)--> ending`, with every synthetic component rostered
/// from `m01` — the fixture shape F44-C's construction tests use, re-authored
/// here so this file reads on its own.
fn granted_graph(grant: u64) -> CampaignGraph {
    let policy = synthetic_policy();
    let mut draft = CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m01"),
                kind: NodeKind::Mission {
                    mission: known(id(ContentKind::Mission, "m01")),
                },
                edges: vec![CampaignEdge {
                    on: EdgeCondition::Victory,
                    to: node("ending"),
                    grant: Some(RewardSpec {
                        currency: known(grant),
                        unlocks: Vec::new(),
                    }),
                    provenance: designed(),
                }],
                provenance: designed(),
            },
            CampaignNode {
                id: node("ending"),
                kind: NodeKind::Ending,
                edges: Vec::new(),
                provenance: designed(),
            },
        ],
        entry: node("m01"),
        roster: Vec::new(),
        provenance: designed(),
    };
    for item in policy.available() {
        draft.roster.push(RosterEntry {
            item: item.clone(),
            available_from: node("m01"),
            provenance: designed(),
        });
    }
    let definition = CampaignDefinition::try_new(draft).expect("the fixture campaign is valid");
    lower_campaign(&definition).expect("the fixture campaign lowers")
}

/// The campaign identity the flow derives for this fixture's sandbox slot —
/// the production function, so the tests can never drift from what `open`
/// computes (#749).
fn campaign_profile(slot: ProfileSlot) -> CampaignProfileId {
    campaign_profile_id(ProfileKind::Synthetic, slot)
}

fn run_id() -> CampaignRunId {
    CampaignRunId::new("run.f45c").expect("valid run id")
}

fn difficulty() -> DifficultyId {
    DifficultyId::new("standard").expect("valid difficulty")
}

/// The outcome record a finished `m01` produces. `sequence` distinguishes two
/// records of the same node so a replay is not deduplicated by accident.
fn mission_record(slot: ProfileSlot, outcome: MissionResult, sequence: u32) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: campaign_profile(slot),
            run: run_id(),
            session: SessionGeneration(1),
            terminal_event: EventKey {
                session: SessionGeneration(1),
                tick: Tick(600),
                source: SymbolId(7),
                sequence,
            },
        },
        node: CampaignNodeKey::new("m01").expect("valid node key"),
        outcome,
        score: 900,
        authority: OutcomeAuthority::Authorized,
    }
}

/// Opens the fixture population and creates the profile the flow will
/// continue, writing a campaign snapshot into it when `grant` is `Some`.
/// Closing the session here is what leaves the population claim free for the
/// flow's own session: the profile exists on disk before the flow runs.
fn seeded_profile(base: &Path, graph: &CampaignGraph, grant: Option<u64>) -> ProfileSlot {
    let catalog = SettingCatalog::empty();
    let mut session = ProfileSession::open_sandbox(base, &catalog).expect("the sandbox opens");
    let id = session.create("Pilot").expect("the pilot is created");
    if let Some(grant) = grant {
        let mut state = CampaignState::begin(campaign_profile(id), run_id(), difficulty(), graph);
        state
            .apply_outcome(graph, &mission_record(id, MissionResult::Succeeded, 0))
            .expect("the first victory commits");
        assert_eq!(state.currency(), grant, "the fixture won its first mission");
        session
            .commit_with(|document| {
                write_snapshot(document, &state.snapshot()).expect("the campaign encodes");
                Ok(())
            })
            .expect("the campaign is stored");
    }
    session.finish().expect("the profile session ends");
    id
}

/// The install/cache/profile roots of one fixture, plus the flow they build.
struct Fixture {
    tree: TempTree,
    graph: CampaignGraph,
    profile: ProfileSlot,
}

impl Fixture {
    /// A fixture whose installation carries both members the mission
    /// declares.
    fn new(label: &str, grant: Option<u64>) -> Self {
        let fixture = Self::bare(label, grant);
        fixture.repair();
        fixture
    }

    /// A fixture whose mission dependency `mission.zrd` is **not** carried by
    /// the installation: a load over it fails until [`Self::repair`].
    fn missing_dependency(label: &str, grant: Option<u64>) -> Self {
        Self::bare(label, grant)
    }

    fn bare(label: &str, grant: Option<u64>) -> Self {
        let tree = TempTree::new(label);
        let graph = granted_graph(GRANT);
        let profile = seeded_profile(&tree.path().join("profiles"), &graph, grant);
        let world = tree.path().join("install").join("zbd").join("c1");
        fs::create_dir_all(&world).expect("the world directory exists");
        fs::write(world.join("hull.bm"), b"fixture hull bytes").expect("a member is written");
        Self {
            tree,
            graph,
            profile,
        }
    }

    fn install(&self) -> PathBuf {
        self.tree.path().join("install")
    }

    fn cache(&self) -> PathBuf {
        self.tree.path().join("cache")
    }

    fn world(&self) -> PathBuf {
        self.install().join("zbd").join("c1")
    }

    /// Creates the mission's missing dependency — the repair AC03 asks for.
    fn repair(&self) {
        fs::write(self.world().join("mission.zrd"), b"fixture mission bytes")
            .expect("the dependency is written");
    }

    fn session(&self) -> ContentSession {
        let mut builder = SessionBuilder::new(
            ResolveContext::new(install_hash())
                .with_world_group(WorldGroup::new(WORLD).expect("the fixture world spelling")),
        );
        let mount = MountBuilder::new(
            MountId::new("world-0").expect("a mount id"),
            MountNamespace::new("world").expect("a namespace"),
            PrecedenceClass::MissionWorld,
            WORLD,
        )
        .with_world_group(WorldGroup::new(WORLD).expect("the fixture world spelling"));
        builder
            .mount_directory(mount, &self.world())
            .expect("the world directory mounts");
        builder.open()
    }

    fn store(&self) -> CacheStore {
        fs::create_dir_all(self.cache()).expect("the cache root exists");
        CacheStore::open(
            CacheDirectory::open(&self.cache(), &self.install())
                .expect("a cache outside the install"),
            CacheBudget::new(8, 8 << 20).expect("a nonzero budget"),
        )
        .expect("the store opens")
    }

    fn setup(&self) -> FlowSetup {
        FlowSetup::new(
            self.tree.path().join("profiles"),
            self.graph.clone(),
            run_id(),
            difficulty(),
        )
        .sandbox()
        .profile_name("Pilot")
        .roles(declared_synthetic_roles())
        .construction(
            synthetic_boundary_rules(),
            synthetic_policy(),
            declared_synthetic_price_book(),
        )
    }

    /// A flow with the authored preflight deck, this fixture's domain inputs
    /// and a load plan over the two members the mission declares.
    fn flow(&self) -> FrontEndFlow {
        let mut flow = FrontEndFlow::new(preflight_deck(), self.setup());
        flow.set_content_session(self.session());
        flow.set_cache_store(self.store());
        flow.set_load_plan(load_plan());
        flow
    }
}

fn install_hash() -> ContentHash {
    ContentHash::from_bytes([0x5A; 32])
}

fn member(name: &str) -> AssetKey {
    AssetKey::from_spelling("world", name, "default").expect("the fixture key is valid")
}

/// The mission's declared closure: one member the installation carries and one
/// it does not (until it is repaired).
fn load_plan() -> LoadPlan {
    LoadPlan {
        target: LoadTarget::world(WorldGroup::new(WORLD).expect("the fixture world spelling")),
        items: vec![
            load_item("hull.bm", "fixture.hull"),
            load_item("mission.zrd", "fixture.mission"),
        ],
    }
}

fn load_item(member_name: &str, content: &str) -> LoadItem {
    LoadItem::new(
        member(member_name),
        id(ContentKind::Image, content),
        Criticality::GameplayCritical,
        64,
    )
    .expect("a declared work unit")
}

fn loadout(player: &str) -> Loadout {
    Loadout {
        player: Some(id(ContentKind::Airframe, player)),
        wingmate: Some(id(ContentKind::Airframe, "fixture.synthetic-wingman")),
        ammunition: Some(id(ContentKind::Ammo, "standard")),
    }
}

fn pilotable() -> &'static str {
    "fixture.synthetic-fixed-wing"
}

/// Walks a flow from the install selection to the flight check, with `loadout`
/// selected and the profile opened as the fixture's existing pilot.
fn to_flight_check(flow: &mut FrontEndFlow, choice: &Loadout) {
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens");
    flow.press(Action::OpenBriefing)
        .expect("the briefing opens");
    flow.press(Action::ContinueToFlightCheck)
        .expect("the flight check opens");
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    flow.select_loadout(choice.clone())
        .expect("the selection is taken");
}

fn walk_to_cabin(flow: &mut FrontEndFlow) {
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
}

fn currency(flow: &FrontEndFlow) -> u64 {
    flow.domain()
        .campaign()
        .expect("a campaign is open")
        .state()
        .currency()
}

fn revision(flow: &FrontEndFlow) -> u64 {
    flow.domain()
        .campaign()
        .expect("a campaign is open")
        .state()
        .revision()
}

// --- AC03: the minimum scenario ------------------------------------------

/// **AC03, the stage's minimum scenario.** The mission declares a dependency
/// the installation does not carry: the load runs through the real content
/// resolver, fails with the missing dependency named, and the machine is back
/// on the flight check with the draft intact, the world released and the
/// campaign untouched (STATE-TRANSACTIONS: *a failed load does not consume
/// campaign money or progress*). The dependency is then repaired and the very
/// same flow — same profile session, same machine, no restart — loads it and
/// reaches the flight.
#[test]
fn accept_f45_c_a_failed_load_is_retried_after_repairing_its_dependency_without_restarting() {
    let fx = Fixture::missing_dependency("retry", Some(GRANT));
    let mut flow = fx.flow();
    let choice = loadout(pilotable());
    to_flight_check(&mut flow, &choice);

    let before_currency = currency(&flow);
    let before_revision = revision(&flow);

    // Launching commits the selection and starts the load.
    flow.press(Action::Launch).expect("the mission launches");
    assert_eq!(flow.front_end().screen(), Screen::Loading);
    assert_eq!(flow.load().attempts(), 1, "the first attempt is running");
    assert!(
        flow.ledger().holds_world(),
        "the loading screen holds the world"
    );

    let verdict = flow.drive_load().expect("the load runs");
    let LoadVerdict::Failed { reason } = verdict else {
        panic!("the missing dependency must fail the load, got {verdict:?}");
    };
    assert!(
        reason.contains("mission.zrd"),
        "the failure names the missing dependency: {reason}"
    );
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    assert_eq!(
        flow.front_end().loadout(),
        &choice,
        "the selection survives the failure"
    );
    let failure = flow
        .front_end()
        .last_failure()
        .expect("the failure is shown");
    assert!(
        failure.reason.contains("mission.zrd"),
        "the screen shows what to fix: {}",
        failure.reason
    );
    assert!(
        !flow.ledger().holds_world(),
        "the half-loaded world is released"
    );
    assert!(!flow.load().in_flight(), "no load is left running");
    assert_eq!(
        currency(&flow),
        before_currency,
        "a failed load spent no campaign money"
    );
    assert_eq!(
        revision(&flow),
        before_revision,
        "a failed load wrote no campaign progress"
    );
    assert!(
        flow.domain().profile().is_some(),
        "the profile session is still open — nothing restarted"
    );

    // Repair the dependency and load again, in the same process.
    fx.repair();
    flow.set_content_session(fx.session());

    flow.press(Action::Launch)
        .expect("the mission launches again");
    assert_eq!(flow.front_end().screen(), Screen::Loading);
    assert_eq!(
        flow.load().attempts(),
        2,
        "the retry is a second attempt, not a replay of the first"
    );
    let verdict = flow.drive_load().expect("the retry runs");
    assert_eq!(verdict, LoadVerdict::Ready, "the repaired dependency loads");
    assert_eq!(flow.front_end().screen(), Screen::Flight);
    assert!(flow.ledger().holds_world());
    assert_eq!(
        currency(&flow),
        before_currency,
        "loading still costs nothing"
    );
}

// --- teardown -------------------------------------------------------------

/// Cancelling a load that is still working tears it down the way leaving the
/// loading screen must: the transaction is cancelled and closed (the private
/// cache comes back for the next attempt), the world is released, the
/// selection is kept and the campaign does not move.
#[test]
fn accept_f45_c_cancelling_a_running_load_tears_it_down_and_keeps_the_selection() {
    let fx = Fixture::new("cancel", Some(GRANT));
    let mut flow = fx.flow();
    let choice = loadout(pilotable());
    to_flight_check(&mut flow, &choice);

    let before = currency(&flow);
    let before_revision = revision(&flow);

    flow.press(Action::Launch).expect("the mission launches");
    let running = flow.pump_load().expect("one bounded step");
    assert!(
        matches!(running, LoadVerdict::Running(_)),
        "the load is still working: {running:?}"
    );
    assert!(flow.load().in_flight());

    flow.press(Action::Cancel).expect("the load is cancelled");
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    assert!(!flow.load().in_flight(), "the attempt is torn down");
    assert_eq!(flow.load().state(), None, "and holds no transaction");
    assert!(!flow.ledger().holds_world());
    assert_eq!(flow.front_end().loadout(), &choice, "the selection is kept");
    assert_eq!(currency(&flow), before);
    assert_eq!(revision(&flow), before_revision);
    assert!(
        flow.domain().profile().is_some(),
        "cancelling a load closes no profile"
    );

    // The cache came back with the teardown, so the next attempt can start.
    flow.press(Action::Launch)
        .expect("the mission launches again");
    assert_eq!(flow.load().attempts(), 2);
    let verdict = flow.drive_load().expect("the second attempt runs");
    assert_eq!(verdict, LoadVerdict::Ready);
    assert_eq!(flow.front_end().screen(), Screen::Flight);
}

// --- the construction flow ------------------------------------------------

/// The construction screen's commit is the F44 economy transaction and the
/// profile save, not a front-end-local edit: the currency moves, the profile
/// revision advances, and reopening the profile in the same process shows the
/// committed state — so it really reached the disk.
#[test]
fn accept_f45_c_a_construction_commit_reaches_the_economy_and_the_profile_save() {
    let fx = Fixture::new("construction", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);
    assert_eq!(
        flow.domain().profile_id(),
        Some(fx.profile),
        "the flow opened the fixture's own existing profile"
    );

    let blueprint_id = id(ContentKind::Blueprint, "custom");
    let screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        declared_synthetic_blueprint(),
        Vec::new(),
    );
    flow.attach_construction(blueprint_id.clone(), screen)
        .expect("the screen is attached");
    flow.press(Action::OpenConstruction)
        .expect("the construction screen opens");
    flow.open_construction(ConstructionDraft {
        blueprint: blueprint_id,
        saved: Vec::new(),
        components: Vec::new(),
    })
    .expect("the draft is open");

    let before = currency(&flow);
    let before_revision = revision(&flow);
    assert_eq!(before, GRANT);

    flow.press(Action::CommitConstruction)
        .expect("the blueprint commits");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    assert_eq!(flow.front_end().construction(), None, "the draft is spent");
    assert!(
        flow.domain()
            .construction()
            .and_then(|s| s.committed())
            .is_some(),
        "the domain screen committed"
    );
    let spent = currency(&flow);
    assert!(spent < before, "the components were paid for: {spent}");
    assert!(
        revision(&flow) > before_revision,
        "the profile revision moved"
    );

    // The profile was closed and reopened from disk by the same process, so
    // the committed state is what a fresh session reads back.
    flow.press(Action::Back).expect("the cabin is left");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert!(flow.domain().profile().is_none(), "the session ended");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens again");
    assert_eq!(currency(&flow), spent, "the commit is on disk");
    assert!(
        revision(&flow) > before_revision,
        "and the revision that stored it came back with it"
    );
}

/// A commit the validator refuses writes nothing, keeps the screen where it is
/// and leaves the campaign byte-for-byte unchanged — the plan-before-press
/// order, seen from the domain side.
#[test]
fn accept_f45_c_a_refused_construction_commit_changes_nothing_and_stays_on_the_screen() {
    let fx = Fixture::new("construction-refused", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);

    let mut armor = synthetic_armor_fitments();
    armor[0] = ArmorFitment::try_new(
        armor[0].zone().clone(),
        id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY),
    )
    .expect("a valid fitment");
    let overweight = declared_synthetic_blueprint()
        .with_armor(armor)
        .expect("the blueprint is well formed");

    let blueprint_id = id(ContentKind::Blueprint, "overweight");
    let screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        overweight,
        Vec::new(),
    );
    flow.attach_construction(blueprint_id.clone(), screen)
        .expect("the screen is attached");
    flow.press(Action::OpenConstruction)
        .expect("the construction screen opens");
    flow.open_construction(ConstructionDraft {
        blueprint: blueprint_id,
        saved: Vec::new(),
        components: Vec::new(),
    })
    .expect("the draft is open");

    let before = currency(&flow);
    let before_revision = revision(&flow);
    let refused = flow
        .press(Action::CommitConstruction)
        .expect_err("the overweight blueprint cannot commit");
    assert!(
        matches!(refused, FlowError::Construction(_)),
        "the economy refused: {refused}"
    );
    assert_eq!(
        flow.front_end().screen(),
        Screen::Construction,
        "a refused commit does not move the screen"
    );
    assert!(
        flow.domain().construction().is_some(),
        "the screen is still open for the player to fix"
    );
    assert_eq!(currency(&flow), before);
    assert_eq!(revision(&flow), before_revision);
}

/// The state machine's draft and the domain's screen are two views of one
/// blueprint; a commit that names a blueprint the open screen is not editing
/// is a wiring mistake and is reported as one, before anything moves.
#[test]
fn accept_f45_c_a_commit_for_another_blueprint_is_refused_before_the_screen_moves() {
    let fx = Fixture::new("construction-mismatch", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);

    let screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        declared_synthetic_blueprint(),
        Vec::new(),
    );
    flow.attach_construction(id(ContentKind::Blueprint, "the-open-one"), screen)
        .expect("the screen is attached");
    flow.press(Action::OpenConstruction)
        .expect("the construction screen opens");
    flow.open_construction(ConstructionDraft {
        blueprint: id(ContentKind::Blueprint, "another-one"),
        saved: Vec::new(),
        components: Vec::new(),
    })
    .expect("the draft is open");

    let before = revision(&flow);
    let refused = flow
        .press(Action::CommitConstruction)
        .expect_err("the drafts have come apart");
    assert!(
        matches!(refused, FlowError::ConstructionMismatch { .. }),
        "the mismatch is named: {refused}"
    );
    assert_eq!(flow.front_end().screen(), Screen::Construction);
    assert_eq!(revision(&flow), before);
}

// --- F45-C.1: the machine's draft is the attached screen's projection ------

/// The flat id list a blueprint carries, in `AircraftBlueprint::components`
/// order — the canonical projection the machine's draft is filled from.
fn fitted(blueprint: &AircraftBlueprint) -> Vec<ContentId> {
    blueprint
        .components()
        .into_iter()
        .map(|(_, component)| component.clone())
        .collect()
}

/// Attaches `blueprint` under `blueprint_id`, moves the machine onto the
/// construction screen and opens its draft.
fn open_projected_draft(flow: &mut FrontEndFlow, blueprint: AircraftBlueprint, name: &str) {
    let screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        blueprint,
        Vec::new(),
    );
    let blueprint_id = id(ContentKind::Blueprint, name);
    flow.attach_construction(blueprint_id.clone(), screen)
        .expect("a clean screen attaches");
    flow.press(Action::OpenConstruction)
        .expect("the construction screen opens");
    flow.open_construction(ConstructionDraft {
        blueprint: blueprint_id,
        saved: Vec::new(),
        components: Vec::new(),
    })
    .expect("the draft is open");
}

/// The declared `saved`/`components` are F45-B's machine-level input; with a
/// screen attached they are a view the flow fills from the screen itself.
/// Whatever they claim, an opened draft against the attached screen's
/// blueprint id comes out projected: `saved` is the committed fit and
/// `components` the current one, so a freshly opened screen is clean.
#[test]
fn accept_f45_c_1_the_open_draft_is_projected_from_the_attached_screen() {
    let fx = Fixture::new("projection", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);

    let committed = fitted(&declared_synthetic_blueprint());
    assert_eq!(committed.len(), 13, "the fixture blueprint's full fit");
    let screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        declared_synthetic_blueprint(),
        Vec::new(),
    );
    let blueprint_id = id(ContentKind::Blueprint, "projected");
    flow.attach_construction(blueprint_id.clone(), screen)
        .expect("a clean screen attaches");
    flow.press(Action::OpenConstruction)
        .expect("the construction screen opens");

    // Claims that do not match the screen at all: the projection replaces
    // them rather than trusting a second view.
    flow.open_construction(ConstructionDraft {
        blueprint: blueprint_id,
        saved: vec![id(ContentKind::Armor, "declared.nonsense")],
        components: Vec::new(),
    })
    .expect("the draft is open");

    let draft = flow.front_end().construction().expect("the draft is open");
    assert_eq!(
        draft.saved, committed,
        "the base is the screen's committed fit"
    );
    assert_eq!(draft.components, committed);
    assert!(!draft.is_dirty(), "a fresh screen projects a clean draft");

    // A clean draft leaves without asking.
    let outcome = flow.press(Action::Cancel).expect("the screen closes");
    assert!(
        !outcome.effects.contains(&Effect::AskDiscard),
        "nothing asked to discard: {:?}",
        outcome.effects
    );
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
}

/// Editing goes through the attached screen's own operations — there is no
/// flat-list edit, because a flat id cannot name the slot it belongs to —
/// and the machine's draft follows it: `components` is re-projected, `saved`
/// stays at the committed base, and the dirty state the discard prompt reads
/// is the real draft's.
#[test]
fn accept_f45_c_1_a_screen_edit_marks_the_machine_draft_dirty() {
    let fx = Fixture::new("edit", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);
    let committed = fitted(&declared_synthetic_blueprint());
    open_projected_draft(&mut flow, declared_synthetic_blueprint(), "edited");

    let swapped = id(ContentKind::Airframe, SYNTHETIC_OTHER_AIRFRAME_KEY);
    flow.edit_construction(|screen| {
        screen
            .set_airframe(swapped.clone())
            .expect("an airframe id fits the slot")
    })
    .expect("the screen is attached");

    // The edit reached the domain screen, not a machine-side copy.
    assert!(
        flow.domain()
            .construction()
            .expect("the screen is attached")
            .is_dirty(),
        "the real screen's own flag moved"
    );
    let mut expected = committed.clone();
    expected[0] = swapped;
    let draft = flow.front_end().construction().expect("the draft is open");
    assert_eq!(
        draft.components, expected,
        "the airframe slot leads the projection"
    );
    assert_eq!(draft.saved, committed, "the committed base did not move");
    assert!(draft.is_dirty());

    // The discard prompt reads that real dirty state.
    let outcome = flow.press(Action::Cancel).expect("leaving asks");
    assert!(
        outcome.effects.contains(&Effect::AskDiscard),
        "the discard prompt fired: {:?}",
        outcome.effects
    );
    assert_eq!(flow.front_end().screen(), Screen::Construction);
    flow.confirm_discard().expect("the discard confirms");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
}

/// A staged sale is pending draft state the blueprint alone cannot show:
/// `components` carries it after the fitments, so `saved != components`
/// catches a sale exactly like a fitment swap.
#[test]
fn accept_f45_c_1_a_staged_sale_is_part_of_the_draft_projection() {
    let fx = Fixture::new("sale", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);
    let committed = fitted(&declared_synthetic_blueprint());
    open_projected_draft(&mut flow, declared_synthetic_blueprint(), "seller");

    let sold = id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY);
    flow.edit_construction(|screen| screen.queue_sale(sold.clone()))
        .expect("the screen is attached");
    assert_eq!(
        flow.domain()
            .construction()
            .expect("the screen is attached")
            .staged_sales(),
        std::slice::from_ref(&sold),
        "the sale was queued on the real screen"
    );

    let mut expected = committed.clone();
    expected.push(sold);
    let draft = flow.front_end().construction().expect("the draft is open");
    assert_eq!(
        draft.components, expected,
        "the staged sale follows the fitments"
    );
    assert_eq!(draft.saved, committed);
    assert!(draft.is_dirty(), "a queued sale is unsaved work");
}

/// A commit re-bases the projection: the committed blueprint becomes the
/// draft's `saved`, so the next opening of the same screen projects clean
/// and a cancel leaves without asking — the machine's dirty state tracks
/// what the screen really committed, not the attach-time base.
#[test]
fn accept_f45_c_1_a_commit_rebases_the_projection() {
    let fx = Fixture::new("rebase", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);
    let committed = fitted(&declared_synthetic_blueprint());
    open_projected_draft(&mut flow, declared_synthetic_blueprint(), "rebased");

    // Drop the four missiles through the screen: the draft is dirty, then
    // the commit buys the remaining fit and re-bases.
    flow.edit_construction(|screen| screen.fit_ordnance(Vec::new()).expect("an empty rack fits"))
        .expect("the screen is attached");
    let draft = flow.front_end().construction().expect("the draft is open");
    assert_eq!(
        draft.components.len(),
        9,
        "the four missiles left the draft"
    );
    assert_eq!(
        draft.components.len() + 4,
        committed.len(),
        "what left is exactly the ordnance"
    );

    flow.press(Action::CommitConstruction)
        .expect("the edited blueprint commits");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    assert!(
        !flow
            .domain()
            .construction()
            .expect("the screen stays attached")
            .is_dirty(),
        "the commit rebased the screen too"
    );

    // Reopening projects against the committed fit, not the attach-time one.
    flow.press(Action::OpenConstruction)
        .expect("the construction screen reopens");
    flow.open_construction(ConstructionDraft {
        blueprint: id(ContentKind::Blueprint, "rebased"),
        saved: Vec::new(),
        components: Vec::new(),
    })
    .expect("the draft is open");
    let draft = flow.front_end().construction().expect("the draft is open");
    assert_eq!(
        draft.saved, draft.components,
        "saved and current agree again"
    );
    assert_eq!(draft.components.len(), 9, "the committed fit is the base");
    assert!(!draft.is_dirty(), "a just-committed screen is clean");
    let outcome = flow.press(Action::Cancel).expect("the screen closes");
    assert!(!outcome.effects.contains(&Effect::AskDiscard));
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
}

/// A screen that was edited before the flow ever saw it has no committed
/// base to project `saved` from — the session keeps no copy of the blueprint
/// it was opened on. The flow refuses the attach rather than inventing one:
/// nothing is stored and nothing moves.
#[test]
fn accept_f45_c_1_a_screen_edited_before_attach_cannot_be_projected() {
    let fx = Fixture::new("dirty-attach", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);

    let mut screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        declared_synthetic_blueprint(),
        Vec::new(),
    );
    screen.queue_sale(id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY));
    assert!(screen.is_dirty(), "the screen was edited before the attach");

    let before = revision(&flow);
    let refused = flow
        .attach_construction(id(ContentKind::Blueprint, "too-late"), screen)
        .expect_err("an edited screen has no base to project");
    assert!(
        matches!(refused, FlowError::ConstructionScreenDirty),
        "the refusal is named: {refused}"
    );
    assert!(
        flow.domain().construction().is_none(),
        "the refused screen was not kept"
    );
    assert_eq!(revision(&flow), before, "nothing committed");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
}

/// A draft that names a blueprint other than the attached screen's is a
/// second view the projection must not "repair": its declared lists pass
/// through untouched so the commit's own mismatch check still sees the two
/// have come apart.
#[test]
fn accept_f45_c_1_a_draft_naming_another_blueprint_is_not_reprojected() {
    let fx = Fixture::new("foreign-draft", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);

    let screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        declared_synthetic_blueprint(),
        Vec::new(),
    );
    flow.attach_construction(id(ContentKind::Blueprint, "the-open-one"), screen)
        .expect("a clean screen attaches");
    flow.press(Action::OpenConstruction)
        .expect("the construction screen opens");

    let declared_saved = vec![id(ContentKind::Armor, "declared.saved")];
    let declared_components = vec![id(ContentKind::Weapon, "declared.part")];
    flow.open_construction(ConstructionDraft {
        blueprint: id(ContentKind::Blueprint, "another-one"),
        saved: declared_saved.clone(),
        components: declared_components.clone(),
    })
    .expect("the draft is open");

    let draft = flow.front_end().construction().expect("the draft is open");
    assert_eq!(
        draft.saved, declared_saved,
        "a foreign draft keeps its declared base"
    );
    assert_eq!(draft.components, declared_components);

    let before = revision(&flow);
    let refused = flow
        .press(Action::CommitConstruction)
        .expect_err("the drafts have come apart");
    assert!(
        matches!(refused, FlowError::ConstructionMismatch { .. }),
        "the mismatch is still caught: {refused}"
    );
    assert_eq!(revision(&flow), before);
}

// --- the flight check flow ------------------------------------------------

/// The flight check commits player, wingmate and ammunition as **one**
/// request and resolves the player's aircraft through the roster the launch
/// resolver really uses; an airframe the hangar may not select is refused
/// while the player is still on the flight check, with no load started and
/// nothing committed.
#[test]
fn accept_f45_c_the_flight_check_commits_one_loadout_and_resolves_the_launch() {
    let fx = Fixture::new("flight-check", Some(GRANT));
    let mut flow = fx.flow();
    let choice = loadout(pilotable());
    to_flight_check(&mut flow, &choice);

    // The mission-only airframe is in the roster and pilotable, but the
    // hangar cannot select it, so the launch resolver refuses it.
    let mut mission_only = choice.clone();
    mission_only.player = Some(id(ContentKind::Airframe, "fixture.synthetic-autogyro"));
    flow.select_loadout(mission_only)
        .expect("the selection is taken");
    let refused = flow
        .press(Action::Launch)
        .expect_err("the hangar may not select this airframe");
    match refused {
        FlowError::Launch(LaunchAssignmentError::NotHangarSelectable { .. }) => {}
        other => panic!("wrong refusal: {other}"),
    }
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    assert_eq!(flow.load().attempts(), 0, "no load was started");
    assert!(
        flow.domain().committed_loadout().is_none(),
        "nothing committed"
    );
    assert!(flow.domain().launch().is_none());

    // An airframe the roster does not carry at all is refused the same way.
    let mut unknown = choice.clone();
    unknown.player = Some(id(ContentKind::Airframe, "fixture.no-such-plane"));
    flow.select_loadout(unknown)
        .expect("the selection is taken");
    let refused = flow
        .press(Action::Launch)
        .expect_err("an unknown airframe cannot launch");
    assert!(
        matches!(
            refused,
            FlowError::Launch(LaunchAssignmentError::UnknownAirframe { .. })
        ),
        "wrong refusal: {refused}"
    );
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);

    // The real selection goes through: one request, one launch, pressed on
    // the authored flight-check artwork rather than through the machine.
    flow.select_loadout(choice.clone())
        .expect("the selection is taken");
    let (x, y) = button_point(flow.session(), Action::Launch);
    flow.click(SURFACE, x, y)
        .expect("the authored launch button");
    assert_eq!(flow.front_end().screen(), Screen::Loading);
    assert_eq!(
        flow.domain().committed_loadout(),
        Some(&choice),
        "the whole loadout was committed atomically"
    );
    let domain = flow.domain();
    let launch = domain.launch().expect("the launch resolved");
    assert_eq!(launch.airframe, id(ContentKind::Airframe, pilotable()));
    assert!(
        !launch.is_forced(),
        "nothing forced this session's aircraft"
    );
    assert_eq!(
        currency(&flow),
        GRANT,
        "committing a loadout spends nothing"
    );
}

// --- the return flow ------------------------------------------------------

/// The whole return walk: an abort changes nothing, the results screen's
/// outcome is F43's campaign transaction (applied *and* saved, with a
/// mismatch between the screen's verdict and the mission's record refused),
/// leaving the cabin closes the profile, and the same process opens it again
/// with everything it wrote.
#[test]
fn accept_f45_c_the_return_flow_saves_the_outcome_and_reopens_the_profile_without_a_restart() {
    let fx = Fixture::new("return", None);
    let mut flow = fx.flow();
    to_flight_check(&mut flow, &loadout(pilotable()));

    flow.press(Action::Launch).expect("the mission launches");
    let verdict = flow.drive_load().expect("the load runs");
    assert_eq!(verdict, LoadVerdict::Ready);
    assert_eq!(flow.front_end().screen(), Screen::Flight);
    let before = currency(&flow);
    let before_revision = revision(&flow);

    // Abandoning from the pause screen is the campaign's *not* happening.
    flow.press(Action::Pause).expect("the flight pauses");
    flow.press(Action::AbortMission)
        .expect("the mission is abandoned");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    assert!(!flow.ledger().holds_world(), "the world was released");
    assert_eq!(currency(&flow), before, "an abort spent nothing");
    assert_eq!(revision(&flow), before_revision, "an abort wrote nothing");
    assert!(
        flow.domain().profile().is_some(),
        "the cabin is still inside the profile"
    );

    // Fly it again and reach the results screen.
    flow.press(Action::OpenBriefing)
        .expect("the briefing opens");
    flow.press(Action::ContinueToFlightCheck)
        .expect("the flight check opens");
    flow.select_loadout(loadout(pilotable()))
        .expect("the selection is taken");
    flow.press(Action::Launch).expect("the mission launches");
    assert_eq!(
        flow.drive_load().expect("the load runs"),
        LoadVerdict::Ready
    );
    assert_eq!(flow.front_end().screen(), Screen::Flight);

    // The screen says success, the mission's own record says failure: neither
    // is applied and the machine stays where it was.
    flow.set_mission_outcome(mission_record(fx.profile, MissionResult::Failed, 1));
    let refused = flow
        .press(Action::MissionSucceeded)
        .expect_err("the two verdicts disagree");
    assert!(
        matches!(refused, FlowError::OutcomeMismatch { .. }),
        "the mismatch is named: {refused}"
    );
    assert_eq!(flow.front_end().screen(), Screen::Flight);
    assert_eq!(currency(&flow), before);
    assert!(
        flow.domain().pending_outcome().is_some(),
        "the record is kept"
    );

    // The record the mission really produced applies and saves.
    flow.set_mission_outcome(mission_record(fx.profile, MissionResult::Succeeded, 2));
    flow.press(Action::MissionSucceeded)
        .expect("the outcome applies");
    assert_eq!(flow.front_end().screen(), Screen::Results);
    assert!(flow.domain().applied_outcome().is_some());
    assert_eq!(currency(&flow), before + GRANT, "the grant was paid");
    assert!(revision(&flow) > before_revision, "the outcome was saved");
    let after_outcome = revision(&flow);

    // Back out the way the player came: the cabin's own escape closes the
    // profile, and opening it again in this same process finds everything.
    flow.press(Action::ReturnToCabin)
        .expect("back to the cabin");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    flow.press(Action::Back).expect("the cabin is left");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert!(
        flow.domain().teardown().is_some(),
        "the session was released"
    );
    assert!(flow.domain().profile().is_none());

    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens again");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    assert_eq!(currency(&flow), before + GRANT, "the outcome survived");
    assert_eq!(revision(&flow), after_outcome, "and so did its revision");
    assert!(
        flow.domain().applied_outcome().is_none(),
        "the closed profile's applied outcome is not reported as this one's"
    );

    // The whole walk held exactly one input context at every step and left no
    // world behind; the cabin's escape closes the profile once more and the
    // menu's own quit button leaves the application.
    assert!(!flow.ledger().holds_world());
    flow.press(Action::Back).expect("the cabin is left");
    assert!(flow.domain().teardown().is_some());
    flow.press(Action::Quit).expect("the application leaves");
    assert!(flow.exiting());
    assert_eq!(&flow.ledger().resources(), flow.front_end().held());
}

// --- the campaign's profile identity is the slot's own (F45-C.3) ----------

/// The boundary mapping itself: it is a pure function of the slot's own
/// identity — the same slot derives the same string every time, a different
/// slot a different one, and the population label keeps `synthetic.1` apart
/// from `production.1`.
#[test]
fn accept_f45_c_3_the_campaign_identity_is_derived_from_the_slot() {
    let slot = ProfileSlot::new(1).expect("nonzero");
    let identity = campaign_profile_id(ProfileKind::Synthetic, slot);
    assert_eq!(
        identity,
        campaign_profile_id(ProfileKind::Synthetic, slot),
        "the mapping is a pure function of the slot"
    );
    assert_eq!(
        identity.as_str(),
        "synthetic.1",
        "the documented `{{population}}.{{slot}}` shape"
    );
    assert_ne!(
        identity,
        campaign_profile_id(
            ProfileKind::Synthetic,
            ProfileSlot::new(2).expect("nonzero")
        ),
        "another slot derives another identity"
    );
    assert_ne!(
        identity,
        campaign_profile_id(ProfileKind::Production, slot),
        "another population derives another identity"
    );
}

/// The flow records the *selected slot's* identity — not a string the
/// caller invented — and a close/reopen in the same process derives the
/// same one, so `CampaignRun::open` resumes the run it wrote rather than
/// reporting `WrongRun`.
#[test]
fn accept_f45_c_3_the_flows_campaign_identity_is_stable_across_a_reopen() {
    let fx = Fixture::new("identity", None);
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);
    let first = flow
        .domain()
        .campaign()
        .expect("a campaign is open")
        .state()
        .snapshot()
        .profile;
    assert_eq!(
        fx.profile,
        ProfileSlot::new(1).expect("nonzero"),
        "the fresh sandbox seeded slot 1"
    );
    assert_eq!(
        first.as_str(),
        "synthetic.1",
        "the run records the selected slot's own identity — `synthetic.1` — \
         not a string the caller invented"
    );

    // Leave the cabin (the profile closes) and continue the same profile:
    // the reopened run must name the same campaign identity.
    flow.press(Action::Back).expect("the cabin is left");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens again");
    assert_eq!(flow.front_end().screen(), Screen::Cabin);
    assert_eq!(
        flow.domain()
            .campaign()
            .expect("a campaign is open")
            .state()
            .snapshot()
            .profile,
        first,
        "the reopen derived the same identity, so the saved run matched"
    );
}

/// A deleted slot's identity can never be issued again: the population's
/// high-water mark does not move on delete, so the next created profile
/// derives a different campaign identity and can never inherit the deleted
/// one's campaign (contract, "Persistence").
#[test]
fn accept_f45_c_3_a_deleted_profiles_identity_is_never_recycled() {
    let tree = TempTree::new("identity-recycle");
    let catalog = SettingCatalog::empty();
    let mut session = ProfileSession::open_sandbox(&tree.path().join("profiles"), &catalog)
        .expect("the sandbox opens");
    let first = session.create("Pilot").expect("the pilot is created");
    session.delete(first).expect("the profile deletes");
    let next = session.create("Pilot").expect("a new pilot is created");
    assert_ne!(first, next, "the slot number is never reissued");
    assert_ne!(
        campaign_profile_id(ProfileKind::Synthetic, first),
        campaign_profile_id(ProfileKind::Synthetic, next),
        "the deleted slot's campaign identity is never recycled"
    );
    session.finish().expect("the session ends");
}

// --- the run owns every campaign write (F45-C.4) ---------------------------

/// A construction commit and a results-screen outcome commit in one profile
/// session both persist through the run's own revision-checked save: the
/// second commits on top of the first with no re-open between them — its
/// base-revision check can only pass because the run already holds the
/// construction commit — and a profile reopened from disk carries both.
#[test]
fn accept_f45_c_4_a_construction_commit_and_an_outcome_commit_share_the_runs_revision_line() {
    let fx = Fixture::new("construction-then-outcome", Some(GRANT));
    let mut flow = fx.flow();
    walk_to_cabin(&mut flow);

    // The construction commit.
    let blueprint_id = id(ContentKind::Blueprint, "custom");
    let screen = ConstructionScreen::open(
        flow.domain().campaign().expect("a campaign").state(),
        declared_synthetic_blueprint(),
        Vec::new(),
    );
    flow.attach_construction(blueprint_id.clone(), screen)
        .expect("the screen is attached");
    flow.press(Action::OpenConstruction)
        .expect("the construction screen opens");
    flow.open_construction(ConstructionDraft {
        blueprint: blueprint_id,
        saved: Vec::new(),
        components: Vec::new(),
    })
    .expect("the draft is open");
    let before_construction = revision(&flow);
    flow.press(Action::CommitConstruction)
        .expect("the blueprint commits");
    let spent = currency(&flow);
    assert!(spent < GRANT, "the components were paid for: {spent}");
    let after_construction = revision(&flow);
    assert!(
        after_construction > before_construction,
        "the construction commit persisted one revision"
    );

    // The outcome commit, in the same profile session and on the same run.
    flow.press(Action::OpenBriefing)
        .expect("the briefing opens");
    flow.press(Action::ContinueToFlightCheck)
        .expect("the flight check opens");
    flow.select_loadout(loadout(pilotable()))
        .expect("the selection is taken");
    flow.press(Action::Launch).expect("the mission launches");
    assert_eq!(
        flow.drive_load().expect("the load runs"),
        LoadVerdict::Ready
    );
    // `m01`'s victory grant was consumed by the fixture's seed, so this
    // second victory is a same-kind replay: it repays nothing, but it still
    // commits — the applied set grows and the revision line advances — which
    // is exactly the write a stale-base run could not have made.
    flow.set_mission_outcome(mission_record(fx.profile, MissionResult::Succeeded, 2));
    flow.press(Action::MissionSucceeded)
        .expect("the outcome applies over the construction commit");
    assert_eq!(flow.front_end().screen(), Screen::Results);
    assert_eq!(currency(&flow), spent, "a replayed victory repays no grant");
    let after_outcome = revision(&flow);
    assert!(
        after_outcome > after_construction,
        "the outcome commit persisted the next revision on the same base"
    );

    // Reopen the profile in the same process: the stored revision is the
    // outcome's own — both writes reached disk in one revision line, and a
    // run that had lost the construction commit could not have produced it.
    flow.press(Action::ReturnToCabin)
        .expect("back to the cabin");
    flow.press(Action::Back).expect("the cabin is left");
    flow.press(Action::ContinueProfile)
        .expect("the profile continues");
    flow.press(Action::ConfirmProfile)
        .expect("the profile opens again");
    assert_eq!(currency(&flow), spent, "the construction spend is on disk");
    assert_eq!(revision(&flow), after_outcome);
}

// --- refusals the table itself reports ------------------------------------

/// A flow reports the machine's own refusals unchanged: an action the screen
/// does not offer is refused by the plan, before the domain is even asked.
#[test]
fn accept_f45_c_a_refused_action_reaches_no_domain_at_all() {
    let fx = Fixture::new("refused", Some(GRANT));
    let mut flow = fx.flow();
    let refused = flow
        .press(Action::Launch)
        .expect_err("the install selection does not launch missions");
    match refused {
        FlowError::Screen(ScreenSessionError::Refused(_)) => {}
        other => panic!("wrong refusal: {other}"),
    }
    assert_eq!(flow.front_end().screen(), Screen::InstallSelect);
    assert!(flow.domain().profile().is_none());
    assert_eq!(flow.load().attempts(), 0);

    // Planning reports the same thing without touching the machine: quitting
    // the install selection asks the domain for nothing at all.
    let plan = flow.plan(Action::Quit).expect("a plan exists");
    assert_eq!(plan.from, Screen::InstallSelect);
    assert_eq!(plan.to, Screen::InstallSelect);
    assert_eq!(plan.request, None);
    assert_eq!(flow.front_end().screen(), Screen::InstallSelect);
    assert_eq!(
        &flow.ledger().resources(),
        flow.front_end().held(),
        "a refused action changed nothing the application holds"
    );
}

/// Every resource the flow ever acquired was released again: the ledger's
/// held set equals the machine's own held set at every point of the walks
/// above, which is what keeps "returning to the menu cannot leave the old
/// world simulating" a checked property rather than a comment.
#[test]
fn accept_f45_c_the_ledger_and_the_machine_agree_on_every_held_resource() {
    let fx = Fixture::new("ledger", Some(GRANT));
    let mut flow = fx.flow();
    to_flight_check(&mut flow, &loadout(pilotable()));
    assert_eq!(
        &flow.ledger().resources(),
        flow.front_end().held(),
        "the consumer and the machine agree on the flight check"
    );

    flow.press(Action::Launch).expect("the mission launches");
    flow.drive_load().expect("the load runs");
    assert_eq!(&flow.ledger().resources(), flow.front_end().held());
    assert_eq!(
        flow.ledger().input(),
        Some(cs_app::ui::front_end::InputContext::Flight),
        "the flight context is bound once"
    );

    flow.press(Action::Pause).expect("the flight pauses");
    flow.press(Action::AbortMission)
        .expect("the mission is abandoned");
    flow.press(Action::Back).expect("the cabin is left");
    assert_eq!(&flow.ledger().resources(), flow.front_end().held());
    assert!(!flow.ledger().holds_world());
    assert_eq!(
        flow.ledger().input(),
        Some(cs_app::ui::front_end::InputContext::Menu),
        "exactly one input context is bound, the menu's"
    );
    assert!(!flow.front_end().held().contains(&Resource::World));
}

// --- the refusal that keeps a load from starting at all ------------------

/// The loading screen must be able to start its load **before** the machine
/// moves: a launch with no declared closure, no content session or no private
/// cache is refused while the player is still on the flight check — no load
/// attempt, nothing committed, no world acquired, campaign untouched — and
/// once the missing input is supplied the same flow launches in the same
/// process. This is the other half of AC03: the failure that must not happen
/// at all rather than the one that is recovered from.
#[test]
fn accept_f45_c_a_launch_without_a_usable_load_plan_is_refused_before_the_machine_moves() {
    let fx = Fixture::new("no-plan", Some(GRANT));
    let choice = loadout(pilotable());

    // Nothing was supplied: the mission declares no dependency closure.
    {
        let mut flow = FrontEndFlow::new(preflight_deck(), fx.setup());
        to_flight_check(&mut flow, &choice);
        let before_currency = currency(&flow);
        let before_revision = revision(&flow);
        let refused = flow
            .press(Action::Launch)
            .expect_err("there is no load plan to enter Loading with");
        assert!(
            matches!(refused, FlowError::Load(LoadError::NoPlan)),
            "the missing plan is named: {refused}"
        );
        assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
        assert_eq!(flow.load().attempts(), 0, "no load was started");
        assert!(
            flow.domain().committed_loadout().is_none(),
            "nothing committed"
        );
        assert!(flow.domain().launch().is_none());
        assert!(!flow.ledger().holds_world(), "no world was acquired");
        assert_eq!(currency(&flow), before_currency, "no money moved");
        assert_eq!(revision(&flow), before_revision, "no progress written");
        assert_eq!(
            &flow.ledger().resources(),
            flow.front_end().held(),
            "the refused launch left the two views agreeing"
        );
    }

    // A plan and a cache, but no content session to resolve the reads through.
    {
        let mut flow = FrontEndFlow::new(preflight_deck(), fx.setup());
        flow.set_load_plan(load_plan());
        flow.set_cache_store(fx.store());
        to_flight_check(&mut flow, &choice);
        let refused = flow
            .press(Action::Launch)
            .expect_err("there is no content session to read through");
        assert!(
            matches!(refused, FlowError::Load(LoadError::NoContentSession)),
            "the missing content session is named: {refused}"
        );
        assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
        assert_eq!(flow.load().attempts(), 0);
    }

    // A plan and a content session, but no private cache to load over. The
    // cache is the only thing missing, so supplying it is all it takes.
    let mut flow = FrontEndFlow::new(preflight_deck(), fx.setup());
    flow.set_load_plan(load_plan());
    flow.set_content_session(fx.session());
    to_flight_check(&mut flow, &choice);
    let refused = flow
        .press(Action::Launch)
        .expect_err("there is no cache to load over");
    assert!(
        matches!(refused, FlowError::Load(LoadError::NoCacheStore)),
        "the missing cache is named: {refused}"
    );
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    assert_eq!(flow.load().attempts(), 0);
    assert!(flow.domain().committed_loadout().is_none());

    flow.set_cache_store(fx.store());
    flow.press(Action::Launch)
        .expect("the mission launches once the load can start");
    assert_eq!(flow.front_end().screen(), Screen::Loading);
    assert_eq!(
        flow.load().attempts(),
        1,
        "the first attempt starts only now"
    );
    assert_eq!(
        flow.drive_load().expect("the load runs"),
        LoadVerdict::Ready
    );
    assert_eq!(flow.front_end().screen(), Screen::Flight);
}

// --- the ledger's own refusal paths ---------------------------------------

/// The ledger is fed the effect stream, so an impossible pair is an error
/// instead of a silently lost teardown: releasing what nobody holds, taking a
/// resource twice and binding a second input context while the first is still
/// bound (the trigger on a button and a gun at once) are all refused, while
/// the release-then-acquire switch a real transition emits is taken.
#[test]
fn accept_f45_c_the_ledger_refuses_an_impossible_resource_stream() {
    let mut ledger = ResourceLedger::default();

    let refused = ledger
        .apply(&Effect::Release(Resource::World))
        .expect_err("nothing holds the world");
    assert_eq!(refused, ResourceProblem::ReleasedUnknown(Resource::World));
    assert!(ledger.resources().is_empty(), "the refusal changed nothing");

    ledger
        .apply(&Effect::Acquire(Resource::Input(InputContext::Menu)))
        .expect("the menu context binds");
    assert_eq!(ledger.input(), Some(InputContext::Menu));

    let refused = ledger
        .apply(&Effect::Acquire(Resource::Input(InputContext::Menu)))
        .expect_err("the same context cannot bind twice");
    assert_eq!(
        refused,
        ResourceProblem::InputBoundTwice {
            held: InputContext::Menu,
            offered: InputContext::Menu,
        }
    );

    let refused = ledger
        .apply(&Effect::Acquire(Resource::Input(InputContext::Flight)))
        .expect_err("the flight context cannot bind while the menu's is bound");
    assert_eq!(
        refused,
        ResourceProblem::InputBoundTwice {
            held: InputContext::Menu,
            offered: InputContext::Flight,
        }
    );
    assert_eq!(
        ledger.input(),
        Some(InputContext::Menu),
        "the refused binding left the first one held"
    );

    ledger
        .apply(&Effect::Acquire(Resource::Audio(AudioScope::FrontEnd)))
        .expect("the front-end audio scope is taken");
    let refused = ledger
        .apply(&Effect::Acquire(Resource::Audio(AudioScope::FrontEnd)))
        .expect_err("the same scope cannot be taken twice");
    assert_eq!(
        refused,
        ResourceProblem::AcquiredTwice(Resource::Audio(AudioScope::FrontEnd))
    );

    ledger
        .apply(&Effect::Acquire(Resource::World))
        .expect("the world is acquired");
    let refused = ledger
        .apply(&Effect::Acquire(Resource::World))
        .expect_err("the world cannot be acquired twice");
    assert_eq!(refused, ResourceProblem::AcquiredTwice(Resource::World));

    // What a real transition emits: release the held scope, then acquire the
    // next one; the input context switches the same way.
    ledger
        .apply(&Effect::Release(Resource::Audio(AudioScope::FrontEnd)))
        .expect("the old scope is released");
    ledger
        .apply(&Effect::Acquire(Resource::Audio(AudioScope::Flight)))
        .expect("the new scope is acquired");
    assert_eq!(ledger.audio(), Some(AudioScope::Flight));
    ledger
        .apply(&Effect::Release(Resource::Input(InputContext::Menu)))
        .expect("the menu context is released");
    ledger
        .apply(&Effect::Acquire(Resource::Input(InputContext::Flight)))
        .expect("the flight context binds once the menu's is gone");
    assert_eq!(ledger.input(), Some(InputContext::Flight));

    ledger
        .apply_all([
            &Effect::Release(Resource::Audio(AudioScope::Flight)),
            &Effect::Release(Resource::World),
            &Effect::Release(Resource::Input(InputContext::Flight)),
        ])
        .expect("the teardown releases everything that was held");
    assert!(ledger.resources().is_empty());
    assert_eq!(ledger.input(), None);
    assert_eq!(ledger.audio(), None);
    assert!(!ledger.holds_world());
}
