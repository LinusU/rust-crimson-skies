//! Acceptance task PARITY5 P5-CONS-C (#2044): the Plane Construction hub's
//! Armor and Paint panels. Task test prefix:
//! `accept_parity5_cabin_plane_construction_armor_paint_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real `[@Armor@]`/`[@Paint@]` rows, `V#` variables and global variables,
//!   invented art extents and strings), through the production panels, the
//!   production hub page, the production [`ConstructionScreen`] and the
//!   headless presenter; they run in CI and prove nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows and strings.
//!   `#[ignore]`d because CI has no installation.

use bevy::app::App;
use bevy::input::InputSystems;
use bevy::prelude::{IntoScheduleConfigs, PreUpdate, Vec2};

use cs_app::construction::{ConstructionContext, ConstructionScreen};
use cs_app::ui::front_end::construction_panels::{
    ARMOR_SECTION, ArmorFeed, ArmorOption, ArmorPanel, DECAL_VARIANTS, PAINT_SECTION, PaintFeed,
    PaintPanel, PanelEdit, PatternOption, apply_edits,
};
use cs_app::ui::front_end::{
    HUB_SECTION, HubOutcome, HubPage, HubPanel, MenuShell, MissionEnd, OriginalLayout,
    PlaneNamePage, SECTION_SURFACE, ShellResource, StartFlow, headless_app_construction,
};
use cs_content::construction::{
    DecalPlacement, PaintSelection, SYNTHETIC_HEAVY_PLATE_KEY, SYNTHETIC_PLATE_KEY,
    declared_synthetic_blueprint,
};
use cs_types::content::{ContentId, ContentKind, DamageNodeKey};

use super::parity5_cabin_plane_construction_hub::{
    LAYOUT as HUB_LAYOUT, TestInput, draft, drive, extents as hub_extents, hub, ids, inject,
    names as hub_names, open_hub, press_button, section, state,
};

/// The real global variables the two panels' rows name.
const GLOBALS: &str = "\
[GLOBALVARS]
G17=STDITEMH,15
G18=PX_ITEMW,225
G19=PX_TEXTH,15
G20=PX_TEXTW,100
G21=PX_SLIDER,PX_B_ScrollBar.png
G22=PX_DOWN,PX_B_ScrollDown.png
G23=PX_UP,PX_B_ScrollUP.png
G24=PX_TITLEW,320
G27=GN_DROPUP,GN_B_ListboxarrowSMALLup.png
G28=GN_DROPDOWN,GN_B_ListboxarrowSMALLdown.png
";

/// The real `[@Armor@]` section.
const ARMOR: &str = "\
	[@Armor@]
    V1=GX,422
    V2=V2,490

	AR_P_LINE0		=P,PX_Rule.png,422,156,5,1,0,0,0
	AR_P_LINE1		=P,PX_Rule.png,422,203,5,1,0,0,0
    AR_P_LINE2      =P,PX_Rule.png,422,250,5,1,0,0,0

    AR_T_TITLE      =T,IDS_AR_TITLE,426,62,0,<PX_TITLEW>,22,0xff261E47,0
	AR_T_NOSETITLE	=T,IDS_AR_NOSE_TITLE,<GX>,110,0,0,0,0xFF000000,0
	AR_T_TAILTITLE	=T,IDS_AR_TAIL_TITLE,<GX>,159,0,0,0,0xFF000000,0
	AR_T_LEFTTITLE	=T,IDS_AR_LEFT_TITLE,<GX>,205,0,0,0,0xFF000000,0
	AR_T_RIGHTTITLE	=T,IDS_AR_RIGHT_TITLE,<GX>,252,0,0,0,0xFF000000,0
    AR_T_ARMORNAME  =T,,<GX>,312,0,335,18,0xFF000000,0

    AR_S_ARMORDESC  =S,0xFF554C67,0xFFE6DABE,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,412,334,0,325,165,,0xFF000000

    AR_D_POINT0  =D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,133,100,<PX_ITEMW>,<STDITEMH>,13
    AR_D_POINT1  =D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,180,90,<PX_ITEMW>,<STDITEMH>,13
    AR_D_POINT2  =D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,228,80,<PX_ITEMW>,<STDITEMH>,13
    AR_D_POINT3  =D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,275,70,<PX_ITEMW>,<STDITEMH>,13

";

/// The real `[@Paint@]` section.
const PAINT: &str = "\
	[@Paint@]
    V1=GX,422
    V2=V2,445
    V3=V3,640
    V4=V4,430

    PT_P_DECALS=P,PX_P_Decals.tga,0,0,0,50,0,2,1

    PT_P_LINE0=P,PX_Rule.png,422,146,5,1,0,0,0
	PT_P_LINE1=P,PX_Rule.png,422,258,5,1,0,0,0
    PT_P_LINE2=P,PX_Rule.png,422,370,5,1,0,0,0

    PT_T_TITLE          =T,IDS_PT_TITLE,426,62,2,<PX_TITLEW>,22,0xff261E47,0
	PT_T_TITLEPATTERN	=T,IDS_PT_PATTERN_TITLE,<GX>,100,2,0,0,0xFF000000,0
	PT_T_TITLECOLOR		=T,IDS_PT_COLOR_TITLE,<GX>,148,20,0,0,0xFF000000,0
	PT_T_TITLESHADE		=T,IDS_PT_SHADE_TITLE,642,148,2,0,0,0xFF000000,0
	PT_T_TITLEDECAL		=T,IDS_PT_DECAL_TITLE,<GX>,258,2,0,0,0xFF000000,0
	PT_T_NUMBERSC0		=T,IDS_GN_1P,<V4>,170,2,<PX_TEXTW>,<PX_TEXTH>,0xFF000000,0
	PT_T_NUMBERSC1		=T,IDS_GN_2P,<V4>,200,2,<PX_TEXTW>,<PX_TEXTH>,0xFF000000,0
	PT_T_NUMBERSC2		=T,IDS_GN_3P,<V4>,232,2,<PX_TEXTW>,<PX_TEXTH>,0xFF000000,0
    PT_T_NUMBERSD0      =T,IDS_PT_DECALNOSE,425,275,2,<PX_TEXTW>,<PX_TEXTH>,0xFF000000,0
    PT_T_NUMBERSD1      =T,IDS_PT_DECALTAIL,545,275,2,<PX_TEXTW>,<PX_TEXTH>,0xFF000000,0
    PT_T_NUMBERSD2      =T,IDS_PT_DECALWING,665,275,2,<PX_TEXTW>,<PX_TEXTH>,0xFF000000,0
    PT_T_PAINTNAME      =T,,<GX>,375,2,335,18,0xFF000000,0

    PT_S_PAINTDESC=S,0xFF554C67,0xFFE6DABE,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,412,398,2,325,105,,0xFF000000

    PT_D_PATTERN=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,120,0,<PX_ITEMW>,<STDITEMH>,12
    PT_D_COLORS0=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,167,30,134,<STDITEMH>,18
    PT_D_COLORS1=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,198,20,134,<STDITEMH>,18
    PT_D_COLORS2=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,228,10,134,<STDITEMH>,18
    PT_D_SHADES0=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,167,30,89,<STDITEMH>,10
    PT_D_SHADES1=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,198,20,89,<STDITEMH>,10
    PT_D_SHADES2=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,228,10,89,<STDITEMH>,10
    PT_D_DECALS0=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,420,291,8,87,73,2
    PT_D_DECALS1=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,540,291,8,87,73,2
    PT_D_DECALS2=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,660,291,8,87,73,2

";

/// The hub fixture's layout with the two panels' real rows in place of their
/// stub sections, and the global variables in front.
fn layout_text() -> String {
    let at = |marker: &str| {
        HUB_LAYOUT
            .find(marker)
            .expect("the hub fixture has the section")
    };
    let (armor, guns) = (at("\t[@Armor@]"), at("\t[@Guns@]"));
    let (paint, purchase) = (at("\t[@Paint@]"), at("\t[@Purchase@]"));
    format!(
        "{GLOBALS}{}{ARMOR}{}{PAINT}{}",
        &HUB_LAYOUT[..armor],
        &HUB_LAYOUT[guns..paint],
        &HUB_LAYOUT[purchase..]
    )
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(&layout_text()).expect("the fixture parses")
}

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "px_rule.png" => Some((300, 2)),
        _ => hub_extents(art),
    }
}

fn names(name: &str) -> Option<String> {
    Some(
        match name {
            "IDS_AR_TITLE" => "[TREB9B]ARMOR",
            "IDS_AR_NOSE_TITLE" => "[TREB9B]NOSE",
            "IDS_AR_TAIL_TITLE" => "[TREB9B]TAIL",
            "IDS_AR_LEFT_TITLE" => "[TREB9B]LEFT",
            "IDS_AR_RIGHT_TITLE" => "[TREB9B]RIGHT",
            "IDS_PT_TITLE" => "[TREB9B]PAINT",
            "IDS_PT_PATTERN_TITLE" => "[TREB9B]PATTERN",
            "IDS_PT_COLOR_TITLE" => "[TREB9B]COLOR",
            "IDS_PT_SHADE_TITLE" => "[TREB9B]SHADE",
            "IDS_PT_DECAL_TITLE" => "[TREB9B]DECALS",
            "IDS_GN_1P" => "[TREB9B]1.",
            "IDS_GN_2P" => "[TREB9B]2.",
            "IDS_GN_3P" => "[TREB9B]3.",
            "IDS_PT_DECALNOSE" => "[TREB9B]NOSE",
            "IDS_PT_DECALTAIL" => "[TREB9B]TAIL",
            "IDS_PT_DECALWING" => "[TREB9B]WING",
            _ => return hub_names(name),
        }
        .to_owned(),
    )
}

fn hub_page() -> HubPage {
    HubPage::from_layout(&layout(), &names, &ids).expect("the fixture has the rows")
}

fn armor_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Armor, key).expect("armor id")
}

fn mask_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::PaintMask, key).expect("mask id")
}

fn zone(key: &str) -> DamageNodeKey {
    DamageNodeKey::new(key).expect("zone key")
}

fn armor_feed() -> ArmorFeed {
    ArmorFeed {
        options: vec![
            ArmorOption {
                id: armor_id(SYNTHETIC_PLATE_KEY),
                name: "Light plate".to_owned(),
                description: "Thin and cheap.".to_owned(),
            },
            ArmorOption {
                id: armor_id(SYNTHETIC_HEAVY_PLATE_KEY),
                name: "Heavy plate".to_owned(),
                description: "Thick and dear.".to_owned(),
            },
        ],
        zones: [
            zone("armor_zone_nose"),
            zone("armor_zone_tail"),
            zone("armor_zone_left"),
            zone("armor_zone_right"),
        ],
        fitted: Vec::new(),
    }
}

fn paint_feed() -> PaintFeed {
    PaintFeed {
        patterns: vec![
            PatternOption {
                mask: mask_id("fixture.pattern_a"),
                name: "Stripes".to_owned(),
                description: "Two stripes.".to_owned(),
            },
            PatternOption {
                mask: mask_id("fixture.pattern_b"),
                name: "Plain".to_owned(),
                description: "Nothing.".to_owned(),
            },
        ],
        colours: vec![0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF],
        shades: vec![
            vec![0xFF80_0000, 0xFFC0_0000],
            vec![0xFF00_8000, 0xFF00_C000, 0xFF00_FF80],
            vec![0xFF00_0080],
        ],
        decals: vec![mask_id("fixture.decal_a"), mask_id("fixture.decal_b")],
        current: PaintSelection::default(),
    }
}

fn armor_panel() -> ArmorPanel {
    ArmorPanel::from_layout(&layout(), &names).expect("the fixture has the armor rows")
}

fn paint_panel() -> PaintPanel {
    PaintPanel::from_layout(&layout(), &names).expect("the fixture has the paint rows")
}

/// The middle of a rect, as a point.
fn centre(rect: cs_app::ui::front_end::ShellRect) -> (i32, i32) {
    (
        rect.x + rect.width as i32 / 2,
        rect.y + rect.height as i32 / 2,
    )
}

// -------------------------------------------------------------- the armor ---

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_a_the_armor_panel_reads_the_real_rows() {
    let panel = armor_panel();
    let lists = panel.lists();
    assert_eq!(lists.len(), 4);
    let rects: Vec<(i32, i32, u32, u32, u32)> = lists
        .iter()
        .map(|list| {
            let row = list.row();
            (
                row.rect.x,
                row.rect.y,
                row.rect.width,
                row.rect.height,
                row.displayed,
            )
        })
        .collect();
    assert_eq!(
        rects,
        [
            (490, 133, 225, 15, 13),
            (490, 180, 225, 15, 13),
            (490, 228, 225, 15, 13),
            (490, 275, 225, 15, 13),
        ]
    );
    let texts = panel.texts();
    let text = |id: &str| {
        texts
            .iter()
            .find(|text| text.id == id)
            .map(|t| t.text.as_str())
    };
    assert_eq!(text("AR_T_TITLE"), Some("ARMOR"));
    assert_eq!(text("AR_T_NOSETITLE"), Some("NOSE"));
    assert_eq!(text("AR_T_TAILTITLE"), Some("TAIL"));
    assert_eq!(text("AR_T_LEFTTITLE"), Some("LEFT"));
    assert_eq!(text("AR_T_RIGHTTITLE"), Some("RIGHT"));
    // The name is filled at run time: nothing before a pick or a hover.
    assert_eq!(text("AR_T_ARMORNAME"), None);
    let desc = panel.desc_box();
    assert_eq!(
        (desc.rect.x, desc.rect.y, desc.rect.width, desc.rect.height),
        (412, 334, 325, 165)
    );
    assert_eq!(
        (desc.border, desc.back, desc.color),
        (0xFF55_4C67, 0xFFE6_DABE, 0xFF00_0000)
    );
}

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_b_a_layout_without_a_panels_rows_is_refused_by_name()
 {
    let bare = OriginalLayout::parse("[@Armor@]\nAR_P_X=P,x.png,0,0,0,1,0,0,0\n").expect("parses");
    let error = ArmorPanel::from_layout(&bare, &names).expect_err("no rows");
    assert!(
        error.to_string().contains("Armor has no"),
        "the refusal names the missing row: {error}"
    );
    let none = OriginalLayout::parse("[@Other@]\nX=P,x.png,0,0,0,1,0,0,0\n").expect("parses");
    assert!(PaintPanel::from_layout(&none, &names).is_err());
    // A hub whose layout has no panel rows keeps the marked notice for the tab.
    let page = HubPage::from_layout(&none, &names, &ids);
    assert!(page.is_err(), "the hub itself needs its section");
}

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_c_a_pick_fits_the_zone_and_left_and_right_mirror()
 {
    let mut panel = armor_panel();
    panel.set_feed(armor_feed());
    // A list opens on its box and picks on an item.
    let nose = panel.lists()[0].row().rect;
    assert_eq!(panel.press_at(centre(nose).0, centre(nose).1), Some(vec![]));
    assert!(panel.lists()[0].is_open());
    let item = panel.lists()[0].item_rect(1, 0);
    assert_eq!(
        panel.press_at(centre(item).0, centre(item).1),
        Some(vec![0])
    );
    assert!(!panel.lists()[0].is_open());
    assert_eq!(panel.zone_choice(0), Some(1));
    for other in 1..4 {
        assert_eq!(panel.zone_choice(other), None);
    }
    // Left (2) and right (3) mirror each other, as the script's mailbox does.
    let left = panel.lists()[2].row().rect;
    panel.press_at(centre(left).0, centre(left).1);
    let item = panel.lists()[2].item_rect(0, 0);
    let changed = panel.press_at(centre(item).0, centre(item).1);
    assert_eq!(changed, Some(vec![2, 3]));
    assert_eq!(
        (panel.zone_choice(2), panel.zone_choice(3)),
        (Some(0), Some(0))
    );
    // …and right answers left.
    let right = panel.lists()[3].row().rect;
    panel.press_at(centre(right).0, centre(right).1);
    let item = panel.lists()[3].item_rect(1, 0);
    panel.press_at(centre(item).0, centre(item).1);
    assert_eq!(
        (panel.zone_choice(2), panel.zone_choice(3)),
        (Some(1), Some(1))
    );
    assert_eq!(panel.zone_choice(1), None, "tail has no mirror");
    let fitments = panel.fitments().expect("armor ids");
    let zones: Vec<&str> = fitments.iter().map(|f| f.zone().as_str()).collect();
    assert_eq!(
        zones,
        ["armor_zone_nose", "armor_zone_left", "armor_zone_right"]
    );
    assert!(
        fitments
            .iter()
            .all(|f| f.armor() == &armor_id(SYNTHETIC_HEAVY_PLATE_KEY))
    );
}

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_d_hovering_an_item_names_it_without_picking_it()
 {
    let mut panel = armor_panel();
    panel.set_feed(armor_feed());
    let tail = panel.lists()[1].row().rect;
    panel.press_at(centre(tail).0, centre(tail).1);
    let item = panel.lists()[1].item_rect(1, 0);
    assert!(panel.hover_at(Some(centre(item))));
    let named = |panel: &ArmorPanel| {
        panel
            .texts()
            .into_iter()
            .find(|text| text.id == "AR_T_ARMORNAME")
            .map(|text| text.text)
    };
    assert_eq!(named(&panel).as_deref(), Some("Heavy plate"));
    assert_eq!(panel.description(), Some("Thick and dear."));
    assert_eq!(panel.zone_choice(1), None, "a hover picks nothing");
    // The pointer leaves the list: the name goes with the hover.
    assert!(panel.hover_at(None));
    assert_eq!(named(&panel), None);
    // A press off the open list closes it and picks nothing.
    assert_eq!(panel.press_at(5, 5), None);
    assert!(!panel.lists()[1].is_open());
    assert_eq!(panel.zone_choice(1), None);
    // A pick shows the armor's name until another is picked or hovered.
    panel.press_at(centre(tail).0, centre(tail).1);
    let first = panel.lists()[1].item_rect(0, 0);
    panel.press_at(centre(first).0, centre(first).1);
    assert_eq!(named(&panel).as_deref(), Some("Light plate"));
    assert_eq!(panel.description(), Some("Thin and cheap."));
}

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_e_the_armor_panel_edits_the_production_draft()
 {
    let draft = draft();
    let ctx = ConstructionContext {
        rules: &draft.rules,
        policy: &draft.policy,
        book: &draft.book,
        graph: &draft.graph,
    };
    let mut screen = ConstructionScreen::open(&draft.state, declared_synthetic_blueprint(), vec![]);
    let before = screen.view(&ctx, &draft.state);
    assert!(!before.dirty);
    let mut page = hub_page();
    assert_eq!(page.press("PX_B_ARMOR"), HubOutcome::Shown(HubPanel::Armor));
    assert!(page.set_armor_feed(armor_feed()));
    // Heavy plates on both sides and the nose.
    for zone_index in [0usize, 2] {
        let panel = page.armor().expect("built");
        let rect = panel.lists()[zone_index].row().rect;
        assert!(page.press_at(centre(rect).0, centre(rect).1));
        let item = page.armor().expect("built").lists()[zone_index].item_rect(1, 0);
        assert!(page.press_at(centre(item).0, centre(item).1));
    }
    let edits = page.take_edits();
    assert_eq!(edits.len(), 2);
    let PanelEdit::Armor(last) = edits[1].clone() else {
        panic!("an armor edit: {edits:?}");
    };
    assert_eq!(last.len(), 3, "nose, left and right");
    assert!(page.take_edits().is_empty(), "the queue drains");
    let refused = apply_edits(&mut screen, edits);
    assert!(refused.is_empty(), "{refused:?}");
    let after = screen.view(&ctx, &draft.state);
    assert!(after.dirty, "the draft differs from the saved plane");
    assert_eq!(after.blueprint.armor(), last.as_slice());
    assert_ne!(
        after.blueprint.armor(),
        before.blueprint.armor(),
        "the panel replaced the fitments"
    );
}

// -------------------------------------------------------------- the paint ---

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_f_the_paint_panel_reads_the_real_rows() {
    let panel = paint_panel();
    let lists: Vec<_> = panel.lists().collect();
    assert_eq!(lists.len(), 10, "pattern, 3 colours, 3 shades, 3 decals");
    let at = |index: usize| {
        let row = lists[index].row();
        (
            row.id.as_str(),
            row.rect.x,
            row.rect.y,
            row.rect.width,
            row.rect.height,
            row.displayed,
        )
    };
    assert_eq!(at(0), ("PT_D_PATTERN", 445, 120, 225, 15, 12));
    assert_eq!(at(1), ("PT_D_COLORS0", 445, 167, 134, 15, 18));
    assert_eq!(at(4), ("PT_D_SHADES0", 640, 167, 89, 15, 10));
    assert_eq!(at(7), ("PT_D_DECALS0", 420, 291, 87, 73, 2));
    assert_eq!(at(9), ("PT_D_DECALS2", 660, 291, 87, 73, 2));
    assert!(lists[7..].iter().all(|list| list.cells() == DECAL_VARIANTS));
    assert!(lists[..7].iter().all(|list| list.cells() == 1));
    let texts = panel.texts();
    let text = |id: &str| {
        texts
            .iter()
            .find(|text| text.id == id)
            .map(|t| t.text.as_str())
    };
    assert_eq!(text("PT_T_TITLE"), Some("PAINT"));
    assert_eq!(text("PT_T_TITLEPATTERN"), Some("PATTERN"));
    assert_eq!(text("PT_T_TITLESHADE"), Some("SHADE"));
    assert_eq!(text("PT_T_NUMBERSC1"), Some("2."));
    assert_eq!(text("PT_T_NUMBERSD2"), Some("WING"));
    assert_eq!(text("PT_T_PAINTNAME"), None, "filled at run time");
}

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_g_picks_make_the_selection_and_a_colour_refreshes_its_shades()
 {
    let mut panel = paint_panel();
    panel.set_feed(paint_feed());
    let click = |panel: &mut PaintPanel, rect: cs_app::ui::front_end::ShellRect| {
        let (x, y) = centre(rect);
        panel.press_at(x, y)
    };
    // No shade list until a colour is picked.
    assert!(panel.lists().nth(4).expect("shade 0").items().is_empty());
    // The pattern.
    let pattern = panel.lists().next().expect("pattern").row().rect;
    assert_eq!(click(&mut panel, pattern), Some(false));
    let item = panel.lists().next().expect("pattern").item_rect(1, 0);
    assert_eq!(click(&mut panel, item), Some(true));
    assert_eq!(panel.choice().pattern, Some(1));
    // Colour 1 of slot 0 refreshes slot 0's shades to that colour's.
    let colour = panel.lists().nth(1).expect("colour 0").row().rect;
    click(&mut panel, colour);
    let item = panel.lists().nth(1).expect("colour 0").item_rect(1, 0);
    assert_eq!(click(&mut panel, item), Some(true));
    let shades = panel.lists().nth(4).expect("shade 0");
    assert_eq!(shades.items().len(), 3);
    assert_eq!(shades.items()[2].swatch, Some(0xFF00_FF80));
    // A shade, then another colour with fewer shades drops it.
    let shade = shades.row().rect;
    click(&mut panel, shade);
    let item = panel.lists().nth(4).expect("shade 0").item_rect(2, 0);
    click(&mut panel, item);
    assert_eq!(panel.choice().shades[0], Some(2));
    let colour = panel.lists().nth(1).expect("colour 0").row().rect;
    click(&mut panel, colour);
    let item = panel.lists().nth(1).expect("colour 0").item_rect(2, 0);
    click(&mut panel, item);
    assert_eq!(panel.lists().nth(4).expect("shade 0").items().len(), 1);
    assert_eq!(panel.choice().shades[0], None, "the held shade is gone");
    // A decal's variant is the cell picked.
    let decal = panel.lists().nth(7).expect("decal 0").row().rect;
    click(&mut panel, decal);
    let cell = panel.lists().nth(7).expect("decal 0").item_rect(1, 3);
    assert_eq!(click(&mut panel, cell), Some(true));
    assert_eq!(panel.choice().decals[0], Some((1, 3)));
    let selection = panel.selection().expect("a valid selection");
    assert_eq!(selection.masks(), [mask_id("fixture.pattern_b")]);
    let placement =
        DecalPlacement::try_new(mask_id("fixture.decal_b"), mask_id("fixture.pattern_b"))
            .expect("placement");
    assert_eq!(selection.decals(), [placement]);
    // The same decal in a second slot is the schema's duplicate.
    let decal = panel.lists().nth(8).expect("decal 1").row().rect;
    click(&mut panel, decal);
    let cell = panel.lists().nth(8).expect("decal 1").item_rect(1, 0);
    click(&mut panel, cell);
    assert!(panel.selection().is_err());
}

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_h_the_paint_panel_edits_the_production_draft()
 {
    let draft = draft();
    let ctx = ConstructionContext {
        rules: &draft.rules,
        policy: &draft.policy,
        book: &draft.book,
        graph: &draft.graph,
    };
    let mut screen = ConstructionScreen::open(&draft.state, declared_synthetic_blueprint(), vec![]);
    let mut page = hub_page();
    assert_eq!(page.press("PX_B_PAINT"), HubOutcome::Shown(HubPanel::Paint));
    assert!(page.set_paint_feed(paint_feed()));
    let pattern = page
        .paint()
        .expect("built")
        .lists()
        .next()
        .expect("list")
        .row()
        .rect;
    assert!(page.press_at(centre(pattern).0, centre(pattern).1));
    let item = page
        .paint()
        .expect("built")
        .lists()
        .next()
        .expect("list")
        .item_rect(0, 0);
    assert!(page.press_at(centre(item).0, centre(item).1));
    // The same pick twice in two decal slots: the second is refused, whole.
    for slot in [7usize, 8] {
        let rect = page
            .paint()
            .expect("built")
            .lists()
            .nth(slot)
            .expect("list")
            .row()
            .rect;
        page.press_at(centre(rect).0, centre(rect).1);
        let cell = page
            .paint()
            .expect("built")
            .lists()
            .nth(slot)
            .expect("list")
            .item_rect(0, 1);
        page.press_at(centre(cell).0, centre(cell).1);
    }
    let edits = page.take_edits();
    assert_eq!(edits.len(), 3);
    assert!(matches!(&edits[0], PanelEdit::Paint(p) if p.decals().is_empty()));
    assert!(matches!(&edits[1], PanelEdit::Paint(p) if p.decals().len() == 1));
    assert!(matches!(&edits[2], PanelEdit::Refused(_)));
    let PanelEdit::Paint(wanted) = edits[1].clone() else {
        panic!("a paint edit");
    };
    let refused = apply_edits(&mut screen, edits);
    assert_eq!(refused.len(), 1, "{refused:?}");
    let view = screen.view(&ctx, &draft.state);
    assert_eq!(view.blueprint.paint(), &wanted);
    assert!(view.dirty);
}

// ---------------------------------------------------------- the hub window ---

fn cabin_app() -> App {
    let layout = layout();
    let mut shell = MenuShell::build(&layout, &extents, &|_: &str| None);
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    let name = PlaneNamePage::from_layout(&layout, &names, &ids, &extents).expect("the name rows");
    let mut app = headless_app_construction(shell, Some(flow), name, hub_page());
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

fn click(app: &mut App, at: (i32, i32)) {
    drive(app, |queue| {
        queue.cursor = Some(Vec2::new(at.0 as f32, at.1 as f32));
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

#[test]
fn accept_parity5_cabin_plane_construction_armor_paint_i_the_tab_shows_the_panel_with_no_notice_and_the_lists_take_clicks()
 {
    assert_eq!(SECTION_SURFACE, (800, 600));
    let mut app = cabin_app();
    open_hub(&mut app, "Ace");
    // Armor: no notice, the tab held down, the panel's rules drawn.
    press_button(&mut app, "PX_B_ARMOR");
    assert_eq!(section(&app), HUB_SECTION);
    assert_eq!(hub(&app).selected(), HubPanel::Armor);
    assert!(
        state(&app).flow().expect("a flow").notice().is_none(),
        "a built panel puts up no notice"
    );
    assert_eq!(
        state(&app).hub_outcomes(),
        [HubOutcome::Shown(HubPanel::Armor)]
    );
    app.update();
    let panes = |app: &App| -> Vec<String> {
        state(app)
            .shell
            .section()
            .expect("a section")
            .panes
            .iter()
            .map(|pane| pane.id.clone())
            .collect()
    };
    assert!(
        panes(&app).contains(&"AR_P_LINE0".to_owned()),
        "{:?}",
        panes(&app)
    );
    assert!(!panes(&app).contains(&"PT_P_LINE0".to_owned()));
    // A click on a zone list's box opens it; a click on an item picks.
    assert!(
        app.world_mut()
            .resource_mut::<ShellResource>()
            .set_hub_armor_feed(armor_feed())
    );
    let nose = hub(&app).armor().expect("built").lists()[0].row().rect;
    click(&mut app, centre(nose));
    assert!(hub(&app).armor().expect("built").lists()[0].is_open());
    let item = hub(&app).armor().expect("built").lists()[0].item_rect(1, 0);
    click(&mut app, centre(item));
    assert_eq!(hub(&app).armor().expect("built").zone_choice(0), Some(1));
    let edits = app
        .world_mut()
        .resource_mut::<ShellResource>()
        .take_hub_edits();
    assert!(
        matches!(edits.as_slice(), [PanelEdit::Armor(f)] if f.len() == 1),
        "{edits:?}"
    );
    // Paint swaps the panel's rules; Guns, still unbuilt, keeps its notice.
    press_button(&mut app, "PX_B_PAINT");
    app.update();
    assert!(panes(&app).contains(&"PT_P_LINE0".to_owned()));
    assert!(!panes(&app).contains(&"AR_P_LINE0".to_owned()));
    assert!(
        !panes(&app).contains(&"PT_P_DECALS".to_owned()),
        "a source sheet"
    );
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    press_button(&mut app, "PX_B_GUNS");
    assert_eq!(
        state(&app)
            .flow()
            .expect("a flow")
            .notice()
            .map(|entry| entry.button),
        Some("PX_B_GUNS")
    );
    // Opening the hub afresh closes the lists and returns to the Airframe tab.
    assert!(panes(&app).iter().all(|id| !id.starts_with("PT_P_")));
}

// ------------------------------------------------------------------ retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_armor_paint_retail_the_original_rows_read_with_the_installations_text()
 {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let real = screens.original_layout().expect("the real layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let resolve = |name: &str| strings.resolve_tagged(name);
    let armor = ArmorPanel::from_layout(&real, &resolve).expect("the real armor rows read");
    let rects: Vec<(i32, i32, u32, u32, u32)> = armor
        .lists()
        .iter()
        .map(|list| {
            let row = list.row();
            (
                row.rect.x,
                row.rect.y,
                row.rect.width,
                row.rect.height,
                row.displayed,
            )
        })
        .collect();
    assert_eq!(
        rects,
        [
            (490, 133, 225, 15, 13),
            (490, 180, 225, 15, 13),
            (490, 228, 225, 15, 13),
            (490, 275, 225, 15, 13),
        ]
    );
    let texts = armor.texts();
    for id in [
        "AR_T_TITLE",
        "AR_T_NOSETITLE",
        "AR_T_TAILTITLE",
        "AR_T_LEFTTITLE",
        "AR_T_RIGHTTITLE",
    ] {
        let text = texts
            .iter()
            .find(|text| text.id == id)
            .unwrap_or_else(|| panic!("{id} reads"));
        assert!(!text.text.is_empty(), "{id} has the installation's string");
    }
    let paint = PaintPanel::from_layout(&real, &resolve).expect("the real paint rows read");
    assert_eq!(paint.lists().count(), 10);
    assert_eq!(
        paint
            .lists()
            .filter(|list| list.cells() == DECAL_VARIANTS)
            .count(),
        3
    );
    let texts = paint.texts();
    for id in ["PT_T_TITLE", "PT_T_TITLEPATTERN", "PT_T_NUMBERSD0"] {
        assert!(texts.iter().any(|text| text.id == id), "{id} reads");
    }
    // The hub built over the real layout has both panels.
    let page = HubPage::from_layout(
        &real,
        &|name: &str| strings.resolve_tagged(name),
        &|id: u32| strings.resolve_id_tagged(id),
    )
    .expect("the real hub rows read");
    assert!(page.armor().is_some() && page.paint().is_some());
    // The presented panel sections keep their rules.
    let shell = MenuShell::build(&real, &|art: &str| screens.art_extent(art), &|_: &str| None);
    for (name, rule) in [(ARMOR_SECTION, "AR_P_LINE0"), (PAINT_SECTION, "PT_P_LINE0")] {
        let section = shell
            .sections()
            .get(&name.to_ascii_uppercase())
            .expect("the section");
        assert!(
            section.panes.iter().any(|pane| pane.id == rule),
            "{name} presents {rule}"
        );
    }
}
