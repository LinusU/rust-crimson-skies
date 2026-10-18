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
//!
//! Follow-up stage F45-C.5 (#1525) reads the original screen scripts and
//! measures what the flight check's own controls state: its ammunition
//! controls start the aircraft's ordnance screen
//! (`ASSETS/LAYOUT.CSV`'s `[FlightCheck]` `ScriptToExe`), the plane slot each
//! edits comes from `FLIGHTCHECK.SCRIPT`'s own `@globals@ZQ` state, and the
//! flight check's own section holds **no** selector row — so the original
//! states no per-mission list of aircraft or loadouts. Its tests are
//! `accept_f45_c_5_*` below and the flow gates the ammunition projection on
//! the measurement. See
//! `docs/findings/2026-10-18-f45-c-5-flight-check-options-and-ammunition.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::loading::{Criticality, LoadItem, LoadTarget};
use cs_app::roster::{LoweredRoster, lower_roster};
use cs_app::ui::front_end::{
    Action, FlightCheckSelection, FlightCheckSlot, FlowError, FlowSetup, FrontEndFlow,
    FrontEndScreens, LoadPlan, Loadout, OriginalLayout, Screen, script_member,
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

// ---- F45-C.5 (#1525): the measured producer of the flight check's slots ----

/// A JSON string literal for the evidence artifact, so no measured id can
/// break out of its string.
fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            control if control.is_control() => out.push(' '),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// A synthetic original layout **in the original's own format**: a flight
/// check whose ammunition controls start an ordnance screen (a section with
/// selector rows) and whose plane control starts the plane screen. The names
/// are invented; the row kinds and columns are the originals'.
const FLIGHT_CHECK_LAYOUT: &str = "\
[@FlightCheck@]
FC_B_CHANGEAMMO =B,FC_B_PaperButton.Png,274,132,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1
FC_B_CHANGEAMMOW=B,FC_B_PaperButton.Png,274,350,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1
FC_B_CHANGEPLANE=B,FC_B_PaperButton.Png,128,132,0,IDS_FC_B_CHANGEPLANE,PlaneSelection,0x1000,1

[@OrdinanceLayout@]
OL_D_AMMO0 =D,slider,up,down,dropup,dropdown,136,120,155,148,20,5
OL_D_ROCKETS0=D,slider,up,down,dropup,dropdown,136,320,120,148,20,12

[@PlaneSelection@]
PS_D_PILOTPLANE=D,slider,up,down,dropup,dropdown,138,132,0,271,20,13
";

/// The same flight check without the wingmate ammunition control: the original
/// shape that offers the wingmate slot no equipment.
const FLIGHT_CHECK_LAYOUT_NO_WINGMATE: &str = "\
[@FlightCheck@]
FC_B_CHANGEAMMO =B,FC_B_PaperButton.Png,274,132,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1

[@OrdinanceLayout@]
OL_D_AMMO0 =D,slider,up,down,dropup,dropdown,136,120,155,148,20,5
";

/// A synthetic screen script in the original's *shape* (`object X = @ctl@BE`
/// then `X.YC = "row id"`, arms in `gui_mailbox`). `FC_B_CHANGEAMMO` selects
/// the pilot's plane (`@globals@ZQ = -1`) and `FC_B_CHANGEAMMOW` the
/// wingmate's (`-2`), exactly as `FLIGHTCHECK.SCRIPT` does.
const FLIGHT_CHECK_SCRIPT: &str = "\
main
{
gui_create
{
object OP = @ctl@BE
OP.YC = \"fc_b_changeammo\"
object PP = @ctl@BE
PP.YC = \"fc_b_changeammow\"
}
gui_mailbox
{
switch (getmessage())
{
case 10003:
switch (sender)
{
case OP:
@globals@ZQ = -1
break
case PP:
@globals@ZQ = -2
break
}
}
}
}
";

/// The production measurement of one synthetic layout.
fn measured(layout_text: &str) -> FlightCheckSelection {
    let layout = OriginalLayout::parse(layout_text).expect("the fixture layout parses");
    let scripts =
        |section: &str| (section == "FlightCheck").then(|| FLIGHT_CHECK_SCRIPT.to_owned());
    layout
        .flight_check_selection(&scripts)
        .expect("the fixture flight check is measured")
}

impl Fixture {
    /// A flow whose application read the original flight check and supplies
    /// the measurement (F45-C.5).
    fn measured_flow(&self, slots: usize, measured: FlightCheckSelection) -> FrontEndFlow {
        let mut flow = FrontEndFlow::new(
            preflight_deck(),
            self.setup(slots).flight_check_selection(measured),
        );
        flow.set_content_session(self.session());
        flow.set_cache_store(self.store());
        flow.set_load_plan(load_plan());
        flow
    }
}

/// The production reader measures the flight check's own controls, the plane
/// slot each edits and the selector rows of the sections they start — and
/// measures that the flight check's own section holds **no** selector row, so
/// the original states no per-mission list of aircraft or loadouts. Remove the
/// reader (or its `@globals@ZQ` reading) and this test fails.
#[test]
fn accept_f45_c_5_the_reader_measures_the_controls_their_slots_and_their_selectors() {
    let first = measured(FLIGHT_CHECK_LAYOUT);
    let controls: Vec<_> = first
        .controls
        .iter()
        .map(|control| (control.id.as_str(), control.starts.as_str(), control.slot))
        .collect();
    assert_eq!(
        controls,
        vec![
            (
                "FC_B_CHANGEAMMO",
                "OrdinanceLayout",
                Some(FlightCheckSlot::Player)
            ),
            (
                "FC_B_CHANGEAMMOW",
                "OrdinanceLayout",
                Some(FlightCheckSlot::Wingmate)
            ),
            ("FC_B_CHANGEPLANE", "PlaneSelection", None),
        ],
        "the control, the screen it starts and the slot its arm selects"
    );
    assert!(
        first.selectors.is_empty(),
        "the flight check's own section holds no selector row: no option list is in the data"
    );
    assert_eq!(
        first.destinations,
        vec![
            (
                "OrdinanceLayout".to_owned(),
                vec!["OL_D_AMMO0".to_owned(), "OL_D_ROCKETS0".to_owned()]
            ),
            (
                "PlaneSelection".to_owned(),
                vec!["PS_D_PILOTPLANE".to_owned()]
            ),
        ],
        "the option surfaces live in the screens the buttons start"
    );
    assert!(first.edits(FlightCheckSlot::Player));
    assert!(first.edits(FlightCheckSlot::Wingmate));

    // An original whose flight check offers the wingmate slot no control that
    // starts a screen with selectors edits no wingmate equipment.
    let none = measured(FLIGHT_CHECK_LAYOUT_NO_WINGMATE);
    assert!(none.edits(FlightCheckSlot::Player));
    assert!(
        !none.edits(FlightCheckSlot::Wingmate),
        "no wingmate control: the wingmate's equipment is not editable"
    );
}

/// The measured producer gates the projection: with the measurement supplied
/// and a wingmate control reaching an option surface, the committed ammunition
/// is still that wingmate's equipment.
#[test]
fn accept_f45_c_5_a_measured_producer_with_a_wingmate_control_commits_the_equipment() {
    let fx = Fixture::new("measured");
    let mut flow = fx.measured_flow(1, measured(FLIGHT_CHECK_LAYOUT));
    let choice = loadout();
    to_flight_check(&mut flow, &choice);

    flow.press(Action::Launch)
        .expect("the measured original edits the wingmate's equipment");
    assert_eq!(flow.domain().committed_loadout(), Some(&choice));
    let domain = flow.domain();
    let assignment = domain
        .roster()
        .expect("the roster is opened")
        .wingmate(cs_sim::allies::WingmateSlot(SLOT))
        .expect("the authored slot is assigned");
    assert_eq!(assignment.loadout, id(ContentKind::Ammo, AMMUNITION));
    assert_eq!(
        assignment.aircraft.as_content(),
        &id(ContentKind::Airframe, WINGMATE_AIRFRAME)
    );
}

/// The gate itself: with the measurement supplied and no wingmate control
/// reaching an option surface, the flow refuses to make the ammunition that
/// wingmate's equipment before the machine moves — nothing is committed, no
/// load starts and the flight check stays on screen.
#[test]
fn accept_f45_c_5_a_measured_producer_without_a_wingmate_control_refuses_the_equipment() {
    let fx = Fixture::new("no-wingmate-control");
    let mut flow = fx.measured_flow(1, measured(FLIGHT_CHECK_LAYOUT_NO_WINGMATE));
    to_flight_check(&mut flow, &loadout());

    let refused = flow
        .press(Action::Launch)
        .expect_err("the measured original edits no wingmate equipment");
    assert!(
        matches!(refused, FlowError::NoWingmateEquipment),
        "the refusal is named: {refused}"
    );
    assert_eq!(flow.front_end().screen(), Screen::FlightCheck);
    assert_eq!(flow.load().attempts(), 0, "no load was started");
    assert!(flow.domain().committed_loadout().is_none());
    assert!(flow.domain().launch().is_none());
    assert!(flow.domain().roster().is_none());
}

/// **The retail half.** What the installation's own front-end data states for
/// the flight check's ammunition: the layout gives `FC_B_CHANGEAMMO` /
/// `FC_B_CHANGEAMMOW` the screen `OrdinanceLayout`, the screen script selects
/// the plane slot each edits, and the ordnance screen holds the plane's gun and
/// rocket selector rows. The flight check's own section holds no selector row,
/// so the original states no per-mission option list.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f45_c_5_retail_the_flight_checks_ammunition_is_the_planes_ordnance_screen() {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    let screens = FrontEndScreens::open(&dir).expect("the original screen container opens");
    let (text, layout_sha) = screens.layout_text().expect("the layout member reads");
    let layout = OriginalLayout::parse(&text).expect("the original layout parses");
    let scripts = |section: &str| screens.script_text(&script_member(section));
    let measured = layout
        .flight_check_selection(&scripts)
        .expect("the original flight check is measured");

    for (id, starts, slot) in [
        (
            "FC_B_CHANGEAMMO",
            "OrdinanceLayout",
            FlightCheckSlot::Player,
        ),
        (
            "FC_B_CHANGEAMMOW",
            "OrdinanceLayout",
            FlightCheckSlot::Wingmate,
        ),
        (
            "FC_B_CHANGEPLANE",
            "PlaneSelection",
            FlightCheckSlot::Player,
        ),
        (
            "FC_B_CHANGEPLANEW",
            "PlaneSelection",
            FlightCheckSlot::Wingmate,
        ),
    ] {
        let control = measured
            .controls
            .iter()
            .find(|control| control.id == id)
            .unwrap_or_else(|| panic!("{id} is one of the flight check's controls"));
        assert_eq!(control.starts, starts, "{id} starts {starts}");
        assert_eq!(control.slot, Some(slot), "{id} edits {slot:?}");
    }

    assert!(
        measured.selectors.is_empty(),
        "the flight check's own section holds no selector row"
    );
    assert!(measured.edits(FlightCheckSlot::Wingmate));

    let (_, ordnance) = measured
        .destinations
        .iter()
        .find(|(starts, _)| starts == "OrdinanceLayout")
        .expect("the ordnance screen is reached");
    assert_eq!(
        ordnance.len(),
        12,
        "the plane's ordnance: four gun and eight rocket selectors"
    );
    assert_eq!(
        ordnance
            .iter()
            .filter(|id| id.starts_with("OL_D_AMMO"))
            .count(),
        4,
        "four gun selectors"
    );
    assert_eq!(
        ordnance
            .iter()
            .filter(|id| id.starts_with("OL_D_ROCKETS"))
            .count(),
        8,
        "eight rocket selectors"
    );

    if let Ok(dir) = std::env::var("CS_EVIDENCE_DIR") {
        let dir = PathBuf::from(dir);
        let dir = if dir.is_absolute() {
            dir
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("workspace root")
                .join(dir)
        };
        fs::create_dir_all(&dir).expect("the evidence directory is created");
        let controls: Vec<String> = measured
            .controls
            .iter()
            .map(|control| {
                format!(
                    "{{\"id\": {}, \"starts\": {}, \"slot\": {}}}",
                    json_str(&control.id),
                    json_str(&control.starts),
                    json_str(match control.slot {
                        Some(FlightCheckSlot::Player) => "player",
                        Some(FlightCheckSlot::Wingmate) => "wingmate",
                        None => "none",
                    })
                )
            })
            .collect();
        let destinations: Vec<String> = measured
            .destinations
            .iter()
            .map(|(starts, selectors)| {
                let ids: Vec<String> = selectors.iter().map(|id| json_str(id)).collect();
                format!(
                    "{{\"starts\": {}, \"selectors\": [{}]}}",
                    json_str(starts),
                    ids.join(", ")
                )
            })
            .collect();
        let document = format!(
            "{{\n  \"member\": \"GOSDATA/ASSETS/crimson.rof::ASSETS/LAYOUT.CSV\",\n  \
             \"script\": \"ASSETS/SCRIPTS/FLIGHTCHECK.SCRIPT\",\n  \
             \"install_sha256\": {},\n  \"layout_sha256\": {},\n  \
             \"controls\": [{}],\n  \"flight_check_selectors\": [],\n  \"destinations\": [{}]\n}}\n",
            json_str(screens.install_sha256()),
            json_str(&layout_sha),
            controls.join(", "),
            destinations.join(", ")
        );
        fs::write(dir.join("flight-check-selection.json"), document)
            .expect("the artifact is written");
    }
}
