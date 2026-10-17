//! Acceptance stage F45-C.2: the flight check's wingmate and ammunition slots
//! reach their consumer (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
//! section `### F45-C`; follow-up task #748). Task test prefix:
//! `accept_f45_c_2_`. Shared contracts: `docs/contracts/UI-NETWORK.md` and
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! F45-C committed the player's, the wingmate's and the ammunition selection
//! as one `Request::CommitLoadout`, but only the player's airframe reached a
//! consumer (`cs_content::airframe_roles::AirframeRoles::resolve_launch`,
//! F25-A). This stage gives the other two slots the consumer the repository
//! really has: the F33 briefing/roster boundary,
//! [`cs_app::roster::open_roster`], which turns the player's briefing
//! selection into the session's `AlliesRoster` wingmate assignments. The
//! `wingmate` slot supplies the authored slot's **aircraft**, the
//! `ammunition` slot the **equipment** the briefing commits, and the pilot,
//! voice and survivability stay the mission's own.
//!
//! Every value here is authored fixture data written under the system
//! temporary directory: the mission's authored roster is the F33-B synthetic
//! fixture lowered through the production `cs_app::roster::lower_roster`, the
//! campaign graph is built directly, and the content session mounts a
//! directory of written bytes. Nothing about the original front end's option
//! set is read or claimed: which aircraft and loadouts a retail mission
//! offers is unmeasured (F45-B.1 read the original layout's buttons and no
//! option data, F45-D measured artwork, F27-D measured the *construction*
//! loadout surface), so the finding records it as unknown and the flow never
//! invents a list.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::loading::{Criticality, LoadItem, LoadTarget};
use cs_app::roster::{LoweredRoster, lower_roster};
use cs_app::ui::front_end::{
    Action, FlowError, FlowSetup, FrontEndFlow, LoadPlan, Loadout, Screen,
};
use cs_assets::cache::{CacheBudget, CacheDirectory, CacheStore};
use cs_assets::vfs::{ContentSession, MountBuilder, SessionBuilder};
use cs_content::airframe_roles::declared_synthetic_roles;
use cs_content::pilots::{DeclaredRoster, declared_synthetic_roster};
use cs_sim::allies::{BriefingError, SurvivabilityPolicy};
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, CampaignRunId, DifficultyId, Outcome as MissionResult, Reward,
    RuntimeEdge, RuntimeNode, RuntimeNodeKind,
};
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

use super::screens::preflight_deck;

/// The world group the fixture content session mounts.
const WORLD: &str = "zbd/c1";
/// The wingmate-slot index the single-slot fixture mission authors.
const SLOT: u32 = 1;
/// The airframe the fixture flight check selects for the wingmate.
const WINGMATE_AIRFRAME: &str = "fixture.wingmate-plane";
/// The ammunition the fixture flight check selects.
const AMMUNITION: &str = "fixture.standard-rounds";

// --- the fixture installation, population and campaign ---------------------

/// A unique, disposable directory under the system temporary directory.
struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f45-c-2-{label}-{}-{}",
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

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid fixture id")
}

fn node_key(key: &str) -> CampaignNodeKey {
    CampaignNodeKey::new(key).expect("a valid fixture node key")
}

/// `m01 --Victory--> ending`, with no roster gates: this stage never reads
/// the campaign's own availability, it only needs a run to exist.
fn graph() -> CampaignGraph {
    CampaignGraph::try_new(
        vec![
            RuntimeNode {
                id: node_key("m01"),
                kind: RuntimeNodeKind::Mission {
                    mission: id(ContentKind::Mission, "m01"),
                },
                edges: vec![RuntimeEdge {
                    on: MissionResult::Succeeded,
                    to: node_key("ending"),
                    grant: Reward::default(),
                }],
            },
            RuntimeNode {
                id: node_key("ending"),
                kind: RuntimeNodeKind::Ending,
                edges: Vec::new(),
            },
        ],
        node_key("m01"),
        Vec::new(),
    )
    .expect("the fixture campaign graph is valid")
}

/// The mission's authored, lowered wingmate roster with `slots` wingmate
/// assignments, lowered through the production boundary.
fn mission_roster(slots: usize) -> LoweredRoster {
    let full = declared_synthetic_roster();
    let wingmates = full.wingmates()[..slots].to_vec();
    let pilots = full
        .pilots()
        .iter()
        .filter(|pilot| wingmates.iter().any(|w| w.pilot() == pilot.id()))
        .cloned()
        .collect();
    let declared = DeclaredRoster::try_new(
        full.subject().clone(),
        full.origin().clone(),
        full.player_faction().clone(),
        pilots,
        wingmates,
        Vec::new(),
        full.provenance().clone(),
    )
    .expect("the trimmed fixture roster is valid");
    lower_roster(&declared).expect("the fixture roster lowers")
}

/// The install/cache/profile roots of one fixture.
struct Fixture {
    tree: TempTree,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let tree = TempTree::new(label);
        let world = tree.path().join("install").join("zbd").join("c1");
        fs::create_dir_all(&world).expect("the world directory exists");
        fs::write(world.join("hull.bm"), b"fixture hull bytes").expect("a member is written");
        Self { tree }
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

    /// A setup over a fresh population with the mission's authored wingmate
    /// roster the flight check commits through.
    fn setup(&self, slots: usize) -> FlowSetup {
        FlowSetup::new(
            self.tree.path().join("profiles"),
            graph(),
            CampaignRunId::new("run.f45c2").expect("valid run id"),
            DifficultyId::new("standard").expect("valid difficulty"),
        )
        .sandbox()
        .profile_name("Pilot")
        .roles(declared_synthetic_roles())
        .roster(mission_roster(slots))
    }

    /// A flow with the authored preflight deck, this fixture's domain inputs
    /// and a one-item load plan so a launch can enter the loading screen.
    fn flow(&self, slots: usize) -> FrontEndFlow {
        let mut flow = FrontEndFlow::new(preflight_deck(), self.setup(slots));
        flow.set_content_session(self.session());
        flow.set_cache_store(self.store());
        flow.set_load_plan(load_plan());
        flow
    }
}

fn install_hash() -> ContentHash {
    ContentHash::from_bytes([0x7C; 32])
}

fn load_plan() -> LoadPlan {
    LoadPlan {
        target: LoadTarget::world(WorldGroup::new(WORLD).expect("the fixture world spelling")),
        items: vec![
            LoadItem::new(
                AssetKey::from_spelling("world", "hull.bm", "default").expect("a fixture key"),
                id(ContentKind::Image, "fixture.hull"),
                Criticality::GameplayCritical,
                64,
            )
            .expect("a declared work unit"),
        ],
    }
}

/// The fixture flight check's selection: the authored player plane, the
/// fixture wingmate airframe and the fixture ammunition.
fn loadout() -> Loadout {
    Loadout {
        player: Some(id(ContentKind::Airframe, "fixture.synthetic-fixed-wing")),
        wingmate: Some(id(ContentKind::Airframe, WINGMATE_AIRFRAME)),
        ammunition: Some(id(ContentKind::Ammo, AMMUNITION)),
    }
}

/// Walks a flow from the install selection to the flight check with `choice`
/// selected, creating the profile as it goes.
fn to_flight_check(flow: &mut FrontEndFlow, choice: &Loadout) {
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::NewProfile)
        .expect("a profile is created");
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

// --- the consumer ----------------------------------------------------------

/// The committed wingmate and ammunition reach the session's identity roster:
/// the flight check's `wingmate` airframe becomes the authored wingmate
/// slot's aircraft and the `ammunition` becomes the equipment the briefing
/// commits, while the pilot, voice and survivability stay the mission's own.
/// The player's airframe still resolves through F25's launch, so the one
/// atomic `CommitLoadout` really did reach both of its consumers.
#[test]
fn accept_f45_c_2_the_committed_wingmate_and_ammunition_reach_the_session_roster() {
    let fx = Fixture::new("roster");
    let mut flow = fx.flow(1);
    let choice = loadout();
    to_flight_check(&mut flow, &choice);

    flow.press(Action::Launch).expect("the mission launches");
    assert_eq!(flow.front_end().screen(), Screen::Loading);
    assert_eq!(
        flow.domain().committed_loadout(),
        Some(&choice),
        "the whole selection was committed atomically"
    );
    let domain = flow.domain();
    let launch = domain.launch().expect("the launch resolved");
    assert_eq!(
        launch.airframe,
        id(ContentKind::Airframe, "fixture.synthetic-fixed-wing")
    );

    let roster = domain
        .roster()
        .expect("the committed slots opened the session's identity roster");
    assert_eq!(roster.session(), 1, "the roster belongs to the session");
    assert_eq!(
        roster
            .player_faction()
            .expect("the fixture mission declares a player faction")
            .as_content(),
        &id(ContentKind::Faction, "synthetic.nathan")
    );
    assert_eq!(
        roster.wingmates().len(),
        1,
        "exactly the authored slot is assigned"
    );

    let slot = cs_sim::allies::WingmateSlot(SLOT);
    let assignment = roster
        .wingmate(slot)
        .expect("the authored slot is assigned");
    assert_eq!(
        assignment.aircraft.as_content(),
        &id(ContentKind::Airframe, WINGMATE_AIRFRAME),
        "the flight check's wingmate airframe is the slot's aircraft"
    );
    assert_eq!(
        assignment.loadout,
        id(ContentKind::Ammo, AMMUNITION),
        "the flight check's ammunition is the equipment the briefing commits"
    );
    assert_eq!(
        assignment.pilot.as_content(),
        &id(ContentKind::Pilot, "synthetic.betty"),
        "the pilot is the mission's authored one, not the flight check's"
    );
    assert_eq!(
        assignment.voice,
        id(ContentKind::Voice, "synthetic.betty"),
        "the voice is the authored one"
    );
    assert_eq!(assignment.survivability, SurvivabilityPolicy::Mortal);
}

/// A selection the mission authors no slot for is refused before the machine
/// moves: nothing is committed, no load starts and the flight check is still
/// on screen. A mission that authors a different number of slots than the
/// flight check can address is the one thing the flow will not guess.
#[test]
fn accept_f45_c_2_a_selection_the_mission_authors_no_slot_for_is_refused() {
    // No authored wingmate slot at all: a selected wingmate has no authored
    // pilot, voice or survivability to be carried by.
    let fx = Fixture::new("no-slot");
    let mut flow = fx.flow(0);
    let choice = loadout();
    to_flight_check(&mut flow, &choice);

    let refused = flow
        .press(Action::Launch)
        .expect_err("the mission authors no wingmate slot");
    assert!(
        matches!(refused, FlowError::NoWingmateSlot),
        "the missing slot is named: {refused}"
    );
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    assert_eq!(flow.load().attempts(), 0, "no load was started");
    assert!(
        flow.domain().committed_loadout().is_none(),
        "nothing was committed"
    );
    assert!(flow.domain().launch().is_none());
    assert!(
        flow.domain().roster().is_none(),
        "no roster was opened from a refused commit"
    );

    // Two authored slots and one flight-check slot: which authored slot the
    // selection fills is unmeasured, so it is refused rather than picked.
    let fx = Fixture::new("two-slots");
    let mut flow = fx.flow(2);
    to_flight_check(&mut flow, &choice);
    let refused = flow
        .press(Action::Launch)
        .expect_err("the mission authors two wingmate slots");
    match refused {
        FlowError::AmbiguousWingmateSlot { authored } => assert_eq!(authored, 2),
        other => panic!("wrong refusal: {other}"),
    }
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    assert_eq!(flow.load().attempts(), 0);
    assert!(flow.domain().roster().is_none());

    // A roster rule refusal survives to the caller by name: the flow reports
    // the F33 briefing boundary's own error type rather than flattening it
    // into a string.
    let refusal = FlowError::from(BriefingError::UnknownWingmateSlot {
        slot: cs_sim::allies::WingmateSlot(SLOT),
    });
    assert!(
        matches!(refusal, FlowError::Briefing(_)),
        "the briefing boundary's refusal is its own variant: {refusal}"
    );
}

/// A mission that needs no wingmate commits none: its authored assignment
/// flies exactly the equipment the mission authored, and the roster is still
/// opened so the session has its authored wingmates.
#[test]
fn accept_f45_c_2_a_loadout_without_a_wingmate_keeps_the_authored_equipment() {
    let fx = Fixture::new("no-wingmate");
    let mut flow = fx.flow(1);
    let choice = Loadout {
        player: Some(id(ContentKind::Airframe, "fixture.synthetic-fixed-wing")),
        wingmate: None,
        ammunition: Some(id(ContentKind::Ammo, AMMUNITION)),
    };
    to_flight_check(&mut flow, &choice);

    flow.press(Action::Launch)
        .expect("a mission that needs no wingmate launches");
    let domain = flow.domain();
    let roster = domain.roster().expect("the roster is opened");
    let assignment = roster
        .wingmate(cs_sim::allies::WingmateSlot(SLOT))
        .expect("the authored slot is assigned");
    assert_eq!(
        assignment.aircraft.as_content(),
        &id(ContentKind::Airframe, "synthetic.devastator"),
        "no selection means the mission's authored airframe flies"
    );
    assert_eq!(
        assignment.loadout,
        id(ContentKind::Loadout, "synthetic.escort"),
        "and the authored equipment, not the flight check's ammunition"
    );
}

/// The identity roster belongs to the campaign the session is in: closing the
/// profile drops it with the rest of that campaign's domain state, so the
/// next profile cannot read the previous session's wingmates.
#[test]
fn accept_f45_c_2_closing_the_profile_drops_the_roster_with_the_session() {
    let fx = Fixture::new("teardown");
    let mut flow = fx.flow(1);
    to_flight_check(&mut flow, &loadout());
    flow.press(Action::Launch).expect("the mission launches");
    assert!(flow.domain().roster().is_some());

    // Leave the way the player came: cancel the load, back out of the flight
    // check (answering the discard prompt), the cabin, then the profile.
    flow.press(Action::Cancel).expect("the load is cancelled");
    flow.press(Action::Back).expect("a discard prompt opens");
    flow.confirm_discard().expect("the draft is dropped");
    assert_eq!(flow.front_end().screen(), Screen::Briefing);
    flow.press(Action::Back).expect("the briefing is left");
    flow.press(Action::Back).expect("the cabin is left");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert!(
        flow.domain().profile().is_none(),
        "the profile session ended"
    );
    assert!(
        flow.domain().roster().is_none(),
        "the closed campaign's roster is not reported as the next one's"
    );
}
