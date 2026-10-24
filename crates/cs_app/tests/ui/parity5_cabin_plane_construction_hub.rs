//! Acceptance task PARITY5 P5-CONS-A (#2042): the Plane Construction hub the
//! Plane Name dialog's OK opens. Task test prefix:
//! `accept_parity5_cabin_plane_construction_hub_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real column layout and the real `V#` variables, invented art extents and
//!   strings). They prove the page's rules and the route from the cabin
//!   through the dialog to the hub and back, using the production start flow,
//!   the production [`ConstructionScreen`] feeding the figures and the
//!   headless presenter; they run in CI and prove nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows, the installation's
//!   own strings and the hub's placement. `#[ignore]`d because CI has no
//!   installation.

use bevy::app::App;
use bevy::ecs::message::MessageWriter;
use bevy::input::ButtonInput;
use bevy::input::ButtonState;
use bevy::input::InputSystems;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::construction::{ConstructionContext, ConstructionScreen};
use cs_app::ui::front_end::construction_hub::{
    CANCEL_BUTTON, CANCEL_PROMPT_STRING, LEAVE_ANSWER, OVER_COLOUR, READY_BUTTON, SELL_BUTTON,
    SELL_MIN_PLANES, WEIGHT_PENDING_STRING, stand_in_width,
};
use cs_app::ui::front_end::plane_name::OK_BUTTON;
use cs_app::ui::front_end::{
    HUB_SECTION, HubFeed, HubOutcome, HubPage, HubPanel, MenuShell, MissionEnd, NAME_SECTION,
    OriginalLayout, PlaneNamePage, READY_NOT_BUILT, SECTION_SURFACE, SELL_NOT_BUILT, Screen,
    ShellResource, StartFlow, headless_app_construction,
};
use cs_content::construction::{
    ConstructionPolicy, ConstructionRules, PriceBook, declared_synthetic_blueprint,
    declared_synthetic_price_book, synthetic_boundary_rules, synthetic_policy,
};
use cs_sim::campaign::{CampaignGraph, CampaignState};

/// The real section's rows (the `V#` variables, panes, buttons, edit box and
/// texts as the installation writes them) with invented art, and a cabin and
/// a name dialog to come from.
const LAYOUT: &str = "\
[@PassengerCabin@]
PC_BACKGROUND=P,bg.png,0,0,4,1,0,2,0
PC_B_NEWMISSION=B,btn.png,0,0,0,0,,0x1000,0
PC_B_PLANEX=B,btn.png,0,40,0,0,PlaneName,0x1000,0

[@PlaneName@]
PX_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
PN_P_BACKGROUND=P,plate.png,0,0,2,1,0,1,0
PN_B_OK=B,okcancel.png,74,130,0,IDS_OK,,,,,,,,,0x41A5A500,0xFFFFFF92,0xFFFFFF00,0x37277A00
PN_B_CANCEL=B,okcancel.png,161,130,0,IDS_CANCEL,,,,,,,,,0x41A5A500,0xFFFFFF92,0xFFFFFF00,0x37277A00
PN_B_DEFAULT=B,check.png,27,83,0,0,,,,,,,,1,1,
PN_E_NAME=E,FONT_PN_EDIT,23,40,0,211,16,7,0xFFFF0020,,0xFFEF0010
PN_T_TITLE=T,IDS_PN_TITLE,30,20,0,205,18,0xFF000000,0
PN_T_DEFAULT=T,IDS_PN_DEFAULTLOADOUT,49,82,0,191,36,0xFF000000,0

[@PlaneConstruction@]
V1=GX,524
V2=V2,PX_Tab.png
V3=V3,564
V4=V4,0xFFffffff
V5=V5,10
V6=PLANEX,16
V7=PLANEY,44
V8=BG_Y1,458
V9=BG_Y2,494
V10=BG1_X1,18
V11=BG1_X2,52
V12=BG1_X3,86
V13=BG1_X4,120
V14=BG1_X5,154
V15=BG2_X1,200
V16=BG2_X2,233
V17=BG2_X3,266
V18=BG2_X4,299
V19=BG2_X5,332
V20=BG_Z,3
PX_P_BACKGROUND=P,PX_BackGround.jpg,0,0,1,1,0,0,0
PX_P_SMALLICONS =P,PX_P_BlueprintSmall.tga,130,100,0,11,0,1,1
PX_P_PLANE0=P,,<PLANEX>,<PLANEY>,5,1,0,1,0
PX_P_PLANE1=P,,<PLANEX>,<PLANEY>,2,1,0,1,0
PX_P_OS0=P,PX_BarGraph.png,<BG1_X1>,<BG_Y1>,<BG_Z>,1,0,1,0
PX_P_AD4=P,PX_BarGraph.png,<BG2_X5>,<BG_Y1>,<BG_Z>,1,0,1,0
PX_P_SI2=P,PX_BarGraph.png,<BG1_X3>,<BG_Y2>,<BG_Z>,1,0,1,0
PX_P_OC0=P,PX_BarGraph.png,<BG2_X1>,<BG_Y2>,<BG_Z>,1,0,1,0
PX_B_Cancel =B,PX_B_CancelPurchase.png,605,<V3>,0,0,,0x1000,,,,,,,
PX_B_Ready  =B,PX_B_ReadyToPurchase.png,400,<V3>,0,0,Purchase,0x1100,0,,,,,,
PX_B_Sell   =B,PX_B_SellPlanes.png,100,<V3>,0,0,,,,,,,,,
PX_B_AIRFRAME   =B,<V2>,23,<GX>,0,IDS_PX_B_AIRFRAME,Airframe,0x1100,0,0,<V5>,0,0,0,0,0xFF37277a,<V4>,<V4>,0xFF37277a
PX_B_ENGINE     =B,<V2>,150,<GX>,0,IDS_PX_B_ENGINE,Engine,0x1100,0,0,<V5>,0,0,0,0,0xFF37277a,<V4>,<V4>,0xFF37277a
PX_B_ARMOR      =B,<V2>,279,<GX>,0,IDS_PX_B_ARMOR,Armor,0x1100,0,0,<V5>,0,0,0,0,0xFF37277a,<V4>,<V4>,0xFF37277a
PX_B_GUNS       =B,<V2>,406,<GX>,0,IDS_PX_B_GUNS,Guns,0x1100,0,0,<V5>,0,0,0,0,0xFF37277a,<V4>,<V4>,0xFF37277a
PX_B_HARDPOINTS =B,<V2>,534,<GX>,0,IDS_PX_B_HARDPOINTS,HardPoints,0x1100,0,0,<V5>,0,0,0,0,0xFF37277a,<V4>,<V4>,0xFF37277a
PX_B_PAINT      =B,<V2>,662,<GX>,0,IDS_PX_B_PAINT,Paint,0x1100,0,0,<V5>,0,0,0,0,0xFF37277a,<V4>,<V4>,0xFF37277a
PX_E_NAME   =E,FONT_PX_EDIT,0,12,0,0,17,16,0xFFFFFFFF,0xFF73699C,0xFFEF0010
PX_T_CASHTITLE      =T,IDS_PX_CASH_TITLE,615,0,0,120,25,0xFF000000,1
PX_T_CASH           =T,,615,25,0,120,55,0xFF000000,1
PX_T_PLANENAME      =T,IDS_PX_PLANENAME_TITLE,16,10,0,0,24,<V4>,0
PX_T_PLANECOST		=T,IDS_PX_PLANECOST_TITLE,385,12,0,0,0,<V4>,0
PX_T_AIRFRAME       =T,IDS_PX_AIRFRAME_TITLE,14,386,0,360,<PX_TEXTH>,<V4>,0
PX_T_WEIGHTCAPACITY =T,IDS_PX_WEIGHTCAPACITY_TITLE,14,402,0,175,36,<V4>,0
PX_T_CURRENTWEIGHT  =T,IDS_PX_CURRENTWEIGHT_TITLE,198,402,0,175,36,<V4>,0
PX_T_TOPSPEED	=T,,14,439,0,175,22,<V4>,0
PX_T_AGILITY	=T,,198,438,0,175,50,<V4>,0
PX_T_ARMOR		=T,,14,474,0,175,22,<V4>,0
PX_T_OFFENSE	=T,,198,474,0,175,22,<V4>,0

	[@AirFrame@]
AF_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
	[@Engine@]
EN_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
	[@Armor@]
AR_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
	[@Guns@]
GU_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
	[@HardPoints@]
HP_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
	[@Paint@]
PA_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
	[@Purchase@]
PU_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
";

/// The plate is 300x200 on the 800x600 surface.
fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "bg.png" | "px_background.jpg" => Some((800, 600)),
        "plate.png" => Some((300, 200)),
        "btn.png" | "okcancel.png" => Some((80, 128)),
        "check.png" => Some((20, 232)),
        "px_p_blueprintsmall.tga" => Some((130, 1100)),
        "px_bargraph.png" => Some((30, 20)),
        // Four 32-row frames.
        "px_b_cancelpurchase.png" | "px_b_readytopurchase.png" | "px_b_sellplanes.png" => {
            Some((150, 128))
        }
        // Four 25-row frames.
        "px_tab.png" => Some((127, 100)),
        _ => None,
    }
}

fn names(name: &str) -> Option<String> {
    Some(
        match name {
            "IDS_OK" => "[TREB9B]OK",
            "IDS_CANCEL" => "[TREB9B]Cancel",
            "IDS_PN_TITLE" => "[AB14I]Name your plane",
            "IDS_PN_DEFAULTLOADOUT" => "[TREB9B]Default loadout",
            "IDS_PX_B_AIRFRAME" => "[TREB10]Airframe",
            "IDS_PX_B_ENGINE" => "[TREB10]Engine",
            "IDS_PX_B_ARMOR" => "[TREB10]Armor",
            "IDS_PX_B_GUNS" => "[TREB10]Guns",
            "IDS_PX_B_HARDPOINTS" => "[TREB10]Hardpoints",
            "IDS_PX_B_PAINT" => "[TREB10]Paint",
            "IDS_PX_CASH_TITLE" => "[AB14I]$$$ on Hand:",
            "IDS_PX_PLANENAME_TITLE" => "[CSB11I]PLANE NAME:",
            "IDS_PX_PLANECOST_TITLE" => "[CSB11I]PLANE COST:  $%1!d!",
            "IDS_PX_AIRFRAME_TITLE" => "[COUR9]AIRFRAME: %1!s!",
            "IDS_PX_WEIGHTCAPACITY_TITLE" => "[CSB11I]WEIGHT CAPACITY:\n %1!d! lbs.",
            "IDS_PX_CURRENTWEIGHT_TITLE" => "[CSB11I]CURRENT WEIGHT:\n %1!d! lbs.",
            _ => return None,
        }
        .to_owned(),
    )
}

fn ids(id: u32) -> Option<String> {
    match id {
        203 => Some("[TREB9B]Please enter a name.".to_owned()),
        CANCEL_PROMPT_STRING => {
            Some("[COUR9]Are you sure you want to cancel building this %1!s!?".to_owned())
        }
        WEIGHT_PENDING_STRING => Some("[CSB11I]CURRENT WEIGHT:\n Pending".to_owned()),
        _ => None,
    }
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(LAYOUT).expect("the fixture parses")
}

fn hub_page() -> HubPage {
    HubPage::from_layout(&layout(), &names, &ids).expect("the fixture has the rows")
}

fn name_page() -> PlaneNamePage {
    PlaneNamePage::from_layout(&layout(), &names, &ids, &extents).expect("the fixture has the rows")
}

fn shell() -> MenuShell {
    MenuShell::build(&layout(), &extents, &|_: &str| None)
}

fn feed() -> HubFeed {
    HubFeed {
        cash: Some(5_000),
        plane_cost: Some(4_000),
        weight_capacity: Some(900),
        current_weight: Some(800),
        weight_pending: false,
        airframe: Some("Ford Hoplite".to_owned()),
        airframe_short: Some("Hoplite".to_owned()),
        dirty: false,
        saved_planes: Some(1),
    }
}

fn text_of<'a>(page: &'a HubPage, id: &str) -> Option<(&'a str, u32)> {
    // Borrow-free copy of the finished texts for the assertion.
    let texts = Box::leak(Box::new(page.texts()));
    texts
        .iter()
        .find(|text| text.id == id)
        .map(|text| (text.text.as_str(), text.color))
}

// ------------------------------------------------------- the synthetic half ---

#[test]
fn accept_parity5_cabin_plane_construction_hub_a_the_page_reads_the_real_rows() {
    let page = hub_page();
    let edit = page.edit();
    assert_eq!(edit.font, "FONT_PX_EDIT");
    // `R = QNA.location.x + QNA.WB; SNA.WB = 302 - R; SNA.location.x = R`.
    let left = 16 + stand_in_width("PLANE NAME:", 11) as i32;
    assert_eq!(
        (edit.rect.x, edit.rect.y, edit.rect.width, edit.rect.height),
        (left, 12, (302 - left) as u32, 17)
    );
    assert_eq!(edit.max_chars, 16);
    assert_eq!(edit.text_color, 0xFFFF_FFFF);
    assert_eq!(edit.frame_color, Some(0xFF73_699C));
    assert_eq!(edit.cursor_color, 0xFFEF_0010);
    let tabs: Vec<(&str, &str, [u32; 4])> = page
        .tabs()
        .iter()
        .map(|tab| (tab.button.as_str(), tab.text.as_str(), tab.colors))
        .collect();
    let colours = [0xFF37_277A, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFF37_277A];
    assert_eq!(
        tabs,
        [
            ("PX_B_AIRFRAME", "Airframe", colours),
            ("PX_B_ENGINE", "Engine", colours),
            ("PX_B_ARMOR", "Armor", colours),
            ("PX_B_GUNS", "Guns", colours),
            ("PX_B_HARDPOINTS", "Hardpoints", colours),
            ("PX_B_PAINT", "Paint", colours),
        ]
    );
    // The four statistics are filled by the engine from tables nobody read.
    assert_eq!(
        page.unread_rows(),
        [
            "PX_T_TOPSPEED",
            "PX_T_AGILITY",
            "PX_T_ARMOR",
            "PX_T_OFFENSE"
        ]
    );
    assert_eq!(page.selected(), HubPanel::Airframe);
    // Nothing is drawn for a figure nobody supplied.
    let ids: Vec<String> = page.texts().into_iter().map(|text| text.id).collect();
    assert_eq!(ids, ["PX_T_CASHTITLE", "PX_T_PLANENAME"]);
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_b_a_layout_without_its_rows_is_refused_by_name() {
    let no_edit = LAYOUT.replace("PX_E_NAME", "PX_E_OTHER");
    let layout = OriginalLayout::parse(&no_edit).expect("parses");
    let error = HubPage::from_layout(&layout, &names, &ids).expect_err("no edit row");
    assert!(error.to_string().contains("PX_E_NAME"), "{error}");
    let none = OriginalLayout::parse("[@Other@]\nA=P,a.png,0,0,0,1,0,0,0\n").expect("parses");
    let error = HubPage::from_layout(&none, &names, &ids).expect_err("no section");
    assert!(error.to_string().contains("PlaneConstruction"), "{error}");
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_c_the_texts_follow_the_feed_and_turn_red_over_a_limit()
 {
    let mut page = hub_page();
    page.set_feed(feed());
    assert_eq!(
        text_of(&page, "PX_T_PLANECOST"),
        Some(("PLANE COST:  $4000", 0xFFFF_FFFF))
    );
    assert_eq!(
        text_of(&page, "PX_T_AIRFRAME"),
        Some(("AIRFRAME: Ford Hoplite", 0xFFFF_FFFF))
    );
    assert_eq!(
        text_of(&page, "PX_T_WEIGHTCAPACITY"),
        Some(("WEIGHT CAPACITY:\n 900 lbs.", 0xFFFF_FFFF))
    );
    assert_eq!(
        text_of(&page, "PX_T_CURRENTWEIGHT"),
        Some(("CURRENT WEIGHT:\n 800 lbs.", 0xFFFF_FFFF))
    );
    assert_eq!(text_of(&page, "PX_T_CASH"), Some(("$5000", 0xFF00_0000)));
    // E 2213 answers `cost <= cash`: one more than the cash is red.
    page.set_feed(HubFeed {
        plane_cost: Some(5_001),
        ..feed()
    });
    assert_eq!(
        text_of(&page, "PX_T_PLANECOST").map(|t| t.1),
        Some(OVER_COLOUR)
    );
    // Exactly the cash is not.
    page.set_feed(HubFeed {
        plane_cost: Some(5_000),
        ..feed()
    });
    assert_eq!(
        text_of(&page, "PX_T_PLANECOST").map(|t| t.1),
        Some(0xFFFF_FFFF)
    );
    // E 2214: weight over the capacity is red, at the capacity it is not.
    page.set_feed(HubFeed {
        current_weight: Some(901),
        ..feed()
    });
    assert_eq!(
        text_of(&page, "PX_T_CURRENTWEIGHT").map(|t| t.1),
        Some(OVER_COLOUR)
    );
    page.set_feed(HubFeed {
        current_weight: Some(900),
        ..feed()
    });
    assert_eq!(
        text_of(&page, "PX_T_CURRENTWEIGHT").map(|t| t.1),
        Some(0xFFFF_FFFF)
    );
    // The pending flag swaps in string 1032, which has no figure and is never red.
    page.set_feed(HubFeed {
        current_weight: Some(901),
        weight_pending: true,
        ..feed()
    });
    assert_eq!(
        text_of(&page, "PX_T_CURRENTWEIGHT"),
        Some(("CURRENT WEIGHT:\n Pending", 0xFFFF_FFFF))
    );
}

/// The synthetic campaign the production construction screen draws from.
struct Draft {
    graph: CampaignGraph,
    rules: ConstructionRules,
    policy: ConstructionPolicy,
    book: PriceBook,
    state: CampaignState,
}

fn draft() -> Draft {
    use cs_content::campaign::{
        CampaignDefinition, CampaignDraft, CampaignEdge, CampaignNode, CampaignNodeId,
        EdgeCondition, NodeKind, RewardSpec, RosterEntry,
    };
    use cs_sim::campaign::{
        CampaignNodeKey, CampaignRunId, DifficultyId, EventKey, MissionOutcome, Outcome,
        OutcomeAuthority, OutcomeId, ProfileId, SessionGeneration, SymbolId,
    };
    use cs_types::Tick;
    use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
    use cs_types::evidence::ClaimId;
    let designed = || Provenance::designed(ClaimId::new("p5consa.test").expect("claim"));
    let node = |key: &str| CampaignNodeId::new(key).expect("node id");
    let policy = synthetic_policy();
    let mut campaign = CampaignDraft {
        nodes: vec![
            CampaignNode {
                id: node("m01"),
                kind: NodeKind::Mission {
                    mission: Resolved::Known(Known::new(
                        ContentId::from_source(ContentKind::Mission, "m01").expect("id"),
                        designed(),
                    )),
                },
                edges: vec![CampaignEdge {
                    on: EdgeCondition::Victory,
                    to: node("ending"),
                    grant: Some(RewardSpec {
                        currency: Resolved::Known(Known::new(1_000_000_u64, designed())),
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
        campaign.roster.push(RosterEntry {
            item: item.clone(),
            available_from: node("m01"),
            provenance: designed(),
        });
    }
    let definition = CampaignDefinition::try_new(campaign).expect("valid campaign");
    let graph = cs_app::campaign::lower_campaign(&definition).expect("lowers");
    let profile = ProfileId::new("pilot.test").expect("profile");
    let run = CampaignRunId::new("run.one").expect("run");
    let mut state = CampaignState::begin(
        profile.clone(),
        run.clone(),
        DifficultyId::new("standard").expect("difficulty"),
        &graph,
    );
    let outcome = MissionOutcome {
        id: OutcomeId {
            profile,
            run,
            session: SessionGeneration(1),
            terminal_event: EventKey {
                session: SessionGeneration(1),
                tick: Tick(10),
                source: SymbolId(7),
                sequence: 1,
            },
        },
        node: CampaignNodeKey::new("m01").expect("node key"),
        outcome: Outcome::Succeeded,
        score: 1,
        authority: OutcomeAuthority::Authorized,
    };
    state.apply_outcome(&graph, &outcome).expect("m01 is won");
    Draft {
        graph,
        rules: synthetic_boundary_rules(),
        policy,
        book: declared_synthetic_price_book(),
        state,
    }
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_d_the_feed_is_the_draft_views_numbers() {
    let draft = draft();
    let ctx = ConstructionContext {
        rules: &draft.rules,
        policy: &draft.policy,
        book: &draft.book,
        graph: &draft.graph,
    };
    let mut screen = ConstructionScreen::open(&draft.state, declared_synthetic_blueprint(), vec![]);
    let view = screen.view(&ctx, &draft.state);
    assert!(!view.meters.is_empty(), "the synthetic rules are measured");
    let feed = HubFeed::from_view(&view, Some(77));
    use cs_content::construction::BudgetQuantity;
    let meter = |quantity| {
        view.meters
            .iter()
            .find(|meter| meter.quantity == quantity)
            .expect("measured")
    };
    assert_eq!(feed.cash, Some(77));
    assert_eq!(feed.plane_cost, Some(meter(BudgetQuantity::Cost).used));
    assert_eq!(
        feed.weight_capacity,
        Some(meter(BudgetQuantity::Mass).limit)
    );
    assert_eq!(feed.current_weight, Some(meter(BudgetQuantity::Mass).used));
    assert!(!feed.dirty);
    // An edited draft differs from the saved plane: E 2221's answer.
    let mut armor = cs_content::construction::synthetic_armor_fitments();
    armor[0] = cs_content::construction::ArmorFitment::try_new(
        armor[0].zone().clone(),
        cs_types::content::ContentId::from_source(
            cs_types::content::ContentKind::Armor,
            cs_content::construction::SYNTHETIC_HEAVY_PLATE_KEY,
        )
        .expect("id"),
    )
    .expect("armor");
    screen.edit(
        declared_synthetic_blueprint()
            .with_armor(armor)
            .expect("blueprint"),
    );
    let edited = screen.view(&ctx, &draft.state);
    let edited_feed = HubFeed::from_view(&edited, None);
    assert!(edited_feed.dirty);
    assert_ne!(edited_feed.current_weight, feed.current_weight);
    let mut page = hub_page();
    page.set_feed(feed);
    let (cost, _) = text_of(&page, "PX_T_PLANECOST").expect("the cost line");
    assert!(cost.starts_with("PLANE COST:  $"), "{cost}");
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_e_tabs_select_and_ready_and_sell_are_marked() {
    let mut page = hub_page();
    page.open("Ace");
    assert_eq!(page.text(), "Ace");
    for panel in HubPanel::ALL {
        assert_eq!(
            page.press(panel.button()),
            HubOutcome::Panel(panel),
            "{panel:?}"
        );
        assert_eq!(page.selected(), panel);
    }
    assert_eq!(page.press(READY_BUTTON), HubOutcome::Ready);
    // Sell needs three saved planes (`E 2018` answers `3 > count` to deactivate).
    assert!(!page.sell_enabled());
    assert_eq!(page.press(SELL_BUTTON), HubOutcome::Nothing);
    page.set_feed(HubFeed {
        saved_planes: Some(SELL_MIN_PLANES),
        ..feed()
    });
    assert!(page.sell_enabled());
    assert_eq!(page.press(SELL_BUTTON), HubOutcome::Sell);
    assert_eq!(page.press("PX_B_OTHER"), HubOutcome::Nothing);
    // Typing stops at the row's MaxChars.
    page.open("");
    page.type_text("A very long plane name indeed");
    assert_eq!(page.text().chars().count(), 16);
    page.backspace();
    assert_eq!(page.text().chars().count(), 15);
    assert!(READY_NOT_BUILT.what.contains("purchase"));
    assert!(SELL_NOT_BUILT.what.contains("hangar"));
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_f_cancel_asks_only_when_the_draft_differs() {
    let mut page = hub_page();
    page.open("Ace");
    page.set_feed(feed());
    // E 2221 clear: leave at once.
    assert_eq!(page.press(CANCEL_BUTTON), HubOutcome::Leave);
    assert_eq!(page.prompt(), None);
    // E 2221 set: message box 4 with string 1228 over the airframe's short name.
    page.set_feed(HubFeed {
        dirty: true,
        ..feed()
    });
    assert_eq!(page.press(CANCEL_BUTTON), HubOutcome::ConfirmLeave);
    assert_eq!(
        page.prompt(),
        Some("Are you sure you want to cancel building this Hoplite?")
    );
    // The prompt owns the page: other presses and typing do nothing.
    assert_eq!(page.press("PX_B_GUNS"), HubOutcome::Nothing);
    page.type_text("zz");
    assert_eq!(page.text(), "Ace");
    // Any answer but 3 keeps building; 3 leaves.
    assert_eq!(page.answer_prompt(0), HubOutcome::Stay);
    assert_eq!(page.prompt(), None);
    assert_eq!(page.answer_prompt(LEAVE_ANSWER), HubOutcome::Nothing);
    assert_eq!(page.press(CANCEL_BUTTON), HubOutcome::ConfirmLeave);
    assert_eq!(page.answer_prompt(LEAVE_ANSWER), HubOutcome::Leave);
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_g_the_shell_presents_no_pane_the_engine_alone_fills()
{
    let shell = shell();
    let section = shell
        .sections()
        .get(&HUB_SECTION.to_ascii_uppercase())
        .expect("the section");
    let panes: Vec<&str> = section.panes.iter().map(|pane| pane.id.as_str()).collect();
    assert_eq!(
        panes,
        ["PX_P_BACKGROUND"],
        "bars, icons and slots are not drawn"
    );
    let buttons: Vec<&str> = section
        .buttons
        .iter()
        .map(|button| button.id.as_str())
        .collect();
    assert_eq!(
        buttons,
        [
            "PX_B_CANCEL",
            "PX_B_READY",
            "PX_B_SELL",
            "PX_B_AIRFRAME",
            "PX_B_ENGINE",
            "PX_B_ARMOR",
            "PX_B_GUNS",
            "PX_B_HARDPOINTS",
            "PX_B_PAINT",
        ]
    );
}

// -------------------------------------------------- the headless presenter ---

#[derive(Resource, Default)]
struct TestInput {
    cursor: Option<Vec2>,
    keys: Vec<(KeyCode, bool)>,
    mouse: Vec<bool>,
    typed: Vec<KeyboardInput>,
}

fn inject(
    keys: ResMut<ButtonInput<KeyCode>>,
    mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: MessageWriter<CursorMoved>,
    mut typed: MessageWriter<KeyboardInput>,
    mut queue: ResMut<TestInput>,
    window: bevy::prelude::Query<Entity, With<PrimaryWindow>>,
) {
    let mut keys = keys;
    keys.clear();
    for (key, down) in queue.keys.drain(..) {
        if down {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
    let mut mouse = mouse;
    mouse.clear();
    for down in queue.mouse.drain(..) {
        if down {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }
    for event in queue.typed.drain(..) {
        typed.write(event);
    }
    if let Some(position) = queue.cursor.take() {
        cursor.write(CursorMoved {
            window: window.single().unwrap_or(Entity::PLACEHOLDER),
            position,
            delta: None,
        });
    }
}

fn drive(app: &mut App, batch: impl FnOnce(&mut TestInput)) {
    batch(&mut app.world_mut().resource_mut::<TestInput>());
    app.update();
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        let typed = character.to_string();
        drive(app, |queue| {
            queue.typed.push(KeyboardInput {
                key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                logical_key: Key::Character(typed.as_str().into()),
                state: ButtonState::Pressed,
                text: Some(typed.as_str().into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        });
    }
}

fn tap(app: &mut App, key: KeyCode) {
    drive(app, |queue| queue.keys.push((key, true)));
    drive(app, |queue| queue.keys.push((key, false)));
}

fn press_button(app: &mut App, id: &str) {
    let (x, y) = {
        let state = app.world().resource::<ShellResource>();
        let view = state.shell.view(SECTION_SURFACE).expect("a view");
        let button = view
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(id))
            .unwrap_or_else(|| panic!("the section shows {id}"));
        (
            button.rect.x as f32 + button.rect.width as f32 * 0.5,
            button.rect.y as f32 + button.rect.height as f32 * 0.5,
        )
    };
    drive(app, |queue| {
        queue.cursor = Some(Vec2::new(x, y));
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

fn state(app: &App) -> &ShellResource {
    app.world().resource::<ShellResource>()
}

fn hub(app: &App) -> &HubPage {
    state(app)
        .construction_hub()
        .expect("the app has the hub page")
}

fn section(app: &App) -> String {
    state(app)
        .shell
        .section_name()
        .unwrap_or_default()
        .to_owned()
}

fn screen(app: &App) -> Screen {
    state(app).flow().expect("a flow").screen()
}

/// The cabin of a start flow that came back from a flight, with both pages.
fn cabin_app() -> App {
    let mut shell = shell();
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    assert_eq!(flow.screen(), Screen::Cabin);
    let mut app = headless_app_construction(shell, Some(flow), name_page(), hub_page());
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

/// The cabin → the Plane Name dialog → a typed name → OK → the hub.
fn open_hub(app: &mut App, name: &str) {
    press_button(app, "PC_B_PLANEX");
    assert_eq!(section(app), NAME_SECTION);
    type_text(app, name);
    press_button(app, OK_BUTTON);
    assert_eq!(section(app), HUB_SECTION);
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_h_ok_with_a_name_opens_the_hub_and_cancel_returns_to_the_cabin()
 {
    let mut app = cabin_app();
    open_hub(&mut app, "Ace");
    // The machine stays on the construction screen; the hub is that screen.
    assert_eq!(screen(&app), Screen::Construction);
    let flow = state(&app).flow().expect("a flow");
    assert_eq!(flow.section(), Some(HUB_SECTION));
    assert!(flow.hub_open());
    // No notice is over the hub, but the Airframe tab's missing panel is counted.
    assert!(flow.notice().is_none());
    let seen: Vec<&str> = flow
        .not_implemented_seen()
        .iter()
        .map(|entry| entry.button)
        .collect();
    assert_eq!(seen, ["PX_B_AIRFRAME"]);
    assert!(flow.refusals().is_empty(), "{:?}", flow.refusals());
    // The name the dialog took is in the hub's edit box.
    assert_eq!(hub(&app).text(), "Ace");
    assert_eq!(hub(&app).selected(), HubPanel::Airframe);
    // Cancel with an unmodified draft goes back to the cabin.
    press_button(&mut app, CANCEL_BUTTON);
    assert_eq!(section(&app), "PassengerCabin");
    assert_eq!(screen(&app), Screen::Cabin);
    assert!(!state(&app).flow().expect("a flow").hub_open());
    assert_eq!(state(&app).hub_outcomes(), [HubOutcome::Leave]);
    // The dialog opens again, not the hub.
    press_button(&mut app, "PC_B_PLANEX");
    assert_eq!(section(&app), NAME_SECTION);
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_i_a_tab_puts_up_the_marked_notice_and_is_held_down()
{
    let mut app = cabin_app();
    open_hub(&mut app, "Ace");
    press_button(&mut app, "PX_B_GUNS");
    // The layout nests a `[@Guns@]` section the button's `ScriptToExe` names;
    // the press stays on the hub all the same.
    assert_eq!(section(&app), HUB_SECTION);
    assert_eq!(hub(&app).selected(), HubPanel::Guns);
    let flow = state(&app).flow().expect("a flow");
    assert_eq!(
        flow.notice().map(|entry| entry.button),
        Some("PX_B_GUNS"),
        "the missing Guns panel is marked"
    );
    // The selected tab draws as depressed once the frame is synced.
    let index = {
        let section = state(&app).shell.section().expect("a section");
        section
            .buttons
            .iter()
            .position(|button| button.id == "PX_B_GUNS")
            .expect("the tab")
    };
    app.update();
    assert_eq!(
        state(&app).shell.visual(index),
        cs_app::ui::front_end::ButtonVisual::Depressed
    );
    // Typing is ignored under the notice; Enter closes it.
    type_text(&mut app, "zz");
    assert_eq!(hub(&app).text(), "Ace");
    tap(&mut app, KeyCode::Enter);
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    // Ready and Sell are marked too; Sell is inactive without three planes.
    press_button(&mut app, READY_BUTTON);
    assert_eq!(section(&app), HUB_SECTION);
    assert_eq!(
        state(&app).flow().expect("a flow").notice(),
        Some(&READY_NOT_BUILT)
    );
    tap(&mut app, KeyCode::Enter);
    press_button(&mut app, SELL_BUTTON);
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    let outcomes = state(&app).hub_outcomes();
    assert_eq!(
        outcomes,
        [HubOutcome::Panel(HubPanel::Guns), HubOutcome::Ready]
    );
}

#[test]
fn accept_parity5_cabin_plane_construction_hub_j_cancel_over_unsaved_changes_asks_and_y_n_answer() {
    let mut app = cabin_app();
    open_hub(&mut app, "Ace");
    assert!(state_mut(&mut app).set_hub_feed(HubFeed {
        dirty: true,
        ..feed()
    }));
    press_button(&mut app, CANCEL_BUTTON);
    assert_eq!(section(&app), HUB_SECTION, "the prompt keeps the screen");
    assert!(hub(&app).prompt().is_some_and(|p| p.ends_with("Hoplite?")));
    // N keeps building.
    type_text(&mut app, "n");
    assert_eq!(hub(&app).prompt(), None);
    assert_eq!(section(&app), HUB_SECTION);
    // Cancel again; Esc also answers no; Y leaves.
    press_button(&mut app, CANCEL_BUTTON);
    tap(&mut app, KeyCode::Escape);
    assert_eq!(hub(&app).prompt(), None);
    assert_eq!(section(&app), HUB_SECTION);
    press_button(&mut app, CANCEL_BUTTON);
    type_text(&mut app, "y");
    assert_eq!(section(&app), "PassengerCabin");
    assert_eq!(screen(&app), Screen::Cabin);
    assert_eq!(
        state(&app).hub_outcomes(),
        [
            HubOutcome::ConfirmLeave,
            HubOutcome::Stay,
            HubOutcome::ConfirmLeave,
            HubOutcome::Stay,
            HubOutcome::ConfirmLeave,
            HubOutcome::Leave,
        ]
    );
}

fn state_mut(app: &mut App) -> bevy::prelude::Mut<'_, ShellResource> {
    app.world_mut().resource_mut::<ShellResource>()
}

// ------------------------------------------------------------ the retail half ---

/// **Retail.** The real `[@PlaneConstruction@]` rows read, the installation's
/// own text answers the captions, the texts and the cancel prompt, and the
/// hub's panes and buttons are presented from the real art.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_hub_retail_the_original_rows_present_with_the_installations_text()
 {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let layout = screens.original_layout().expect("the real layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let mut page = HubPage::from_layout(
        &layout,
        &|name: &str| strings.resolve_tagged(name),
        &|id: u32| strings.resolve_id_tagged(id),
    )
    .expect("the real hub rows read");
    let tabs: Vec<&str> = page.tabs().iter().map(|tab| tab.text.as_str()).collect();
    assert_eq!(tabs.len(), 6, "every tab has the installation's caption");
    assert_eq!(tabs[0], "Airframe");
    assert!(tabs.iter().all(|tab| !tab.is_empty()));
    let edit = page.edit();
    assert_eq!(
        (edit.max_chars, edit.rect.y, edit.rect.height),
        (16, 12, 17)
    );
    assert_eq!(edit.frame_color, Some(0xFF73_699C));
    page.set_feed(HubFeed {
        dirty: true,
        ..feed()
    });
    let texts = page.texts();
    let cost = texts
        .iter()
        .find(|text| text.id == "PX_T_PLANECOST")
        .expect("the cost line");
    assert_eq!(cost.text, "PLANE COST:  $4000");
    let title = texts
        .iter()
        .find(|text| text.id == "PX_T_CASHTITLE")
        .expect("the cash title");
    assert_eq!(title.text, "$$$ on Hand:");
    assert_eq!(page.press(CANCEL_BUTTON), HubOutcome::ConfirmLeave);
    assert_eq!(
        page.prompt(),
        Some("Are you sure you want to cancel building this Hoplite?")
    );
    // The presented section keeps the backdrop and the nine buttons, and no
    // bar graph.
    let shell = MenuShell::build(&layout, &|art: &str| screens.art_extent(art), &|_: &str| {
        None
    });
    let section = shell
        .sections()
        .get(&HUB_SECTION.to_ascii_uppercase())
        .expect("the section");
    assert_eq!(section.buttons.len(), 9);
    assert!(
        section
            .panes
            .iter()
            .any(|pane| pane.id == "PX_P_BACKGROUND")
    );
    assert!(section.panes.iter().all(|pane| !pane.id.contains("_OS")));
}
