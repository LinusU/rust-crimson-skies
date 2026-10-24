//! Task #2040: the pilot's memento picture drawn in the cabin's photo frame
//! (`PC_MEMENTO`, behind `PC_FRAME`) with the pane transform #2039 decoded.
//! Task test prefix: `accept_parity5_cabin_memento_frame_`.
//!
//! The synthetic tests run the production [`show_cabin_memento`],
//! [`StartFlow::sync_memento`] and [`MenuShell::view`] over layout text in
//! the original's format; the retail test reads the real section and is
//! ignored in CI. The picture on screen is the owner-command cabin smoke's
//! (`docs/PLAYTEST.md`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::ui::front_end::{
    CABIN_MEMENTO, CABIN_MEMENTO_PANE, CABIN_MEMENTO_POSITION, CabinHub, FRAME_PANE,
    FrontEndScreens, MEMENTOS, MenuShell, OriginalLayout, StartFlow, picture_art, script_member,
    show_cabin_memento,
};

/// `PC_MEMENTO` has no art in the layout (the script builds it): the shell
/// skips it, as in the retail section.
const LAYOUT: &str = "\
[@PassengerCabin@]
PC_BACKGROUND=P,bg.png,0,0,4,1,0,2,0
PC_MEMENTO=P,,169,325,2,1,0,0,0
PC_FRAME=P,frame.png,169,317,5,1,0,2,0
PC_B_NEXT=B,next.png,87,504,0,0,,0x1000,0
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "bg.png" => Some((800, 600)),
        "frame.png" => Some((93, 115)),
        "next.png" => Some((221, 168)),
        _ => None,
    }
}

fn pictures() -> BTreeMap<String, (u32, u32)> {
    MEMENTOS
        .iter()
        .map(|entry| (picture_art(entry.name).to_ascii_lowercase(), (362, 460)))
        .collect()
}

fn shell() -> MenuShell {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    let mut shell = MenuShell::build(&layout, &|art: &str| extents(art), &|_: &str| None);
    shell.open("PassengerCabin").expect("the cabin opens");
    shell
}

fn order(shell: &MenuShell) -> Vec<String> {
    shell.sections()["PASSENGERCABIN"]
        .panes
        .iter()
        .map(|pane| pane.id.clone())
        .collect()
}

/// **The picture sits behind the frame with the decoded transform.**
#[test]
fn accept_parity5_cabin_memento_frame_pane_is_behind_the_frame_with_the_decoded_transform() {
    let mut shell = shell();
    assert_eq!(order(&shell), ["PC_BACKGROUND", "PC_FRAME"]);
    let name = MEMENTOS[1].name;
    assert!(show_cabin_memento(&mut shell, name, &pictures()));
    assert_eq!(
        order(&shell),
        ["PC_BACKGROUND", CABIN_MEMENTO_PANE, FRAME_PANE]
    );
    let pane = &shell.sections()["PASSENGERCABIN"].panes[1];
    assert_eq!(pane.art, picture_art(name));
    assert_eq!(pane.transform, Some(CABIN_MEMENTO));
    assert_eq!(
        (pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height),
        (CABIN_MEMENTO_POSITION.0, CABIN_MEMENTO_POSITION.1, 362, 460)
    );
    // The same picture again changes nothing; another one swaps the art.
    assert!(!show_cabin_memento(&mut shell, name, &pictures()));
    assert!(show_cabin_memento(
        &mut shell,
        MEMENTOS[2].name,
        &pictures()
    ));
    assert_eq!(order(&shell).len(), 3);
    // A picture whose extent is unknown is not guessed.
    assert!(!show_cabin_memento(&mut shell, "Nope.jpg", &pictures()));
}

/// **The view keeps the transformed pane whole instead of clipping the
/// 362x460 picture at the render position to the surface.**
#[test]
fn accept_parity5_cabin_memento_frame_view_does_not_clip_the_transformed_pane() {
    let mut shell = shell();
    show_cabin_memento(&mut shell, MEMENTOS[1].name, &pictures());
    let view = shell.view((800, 600)).expect("a view");
    let pane = view
        .panes
        .iter()
        .find(|pane| pane.id == CABIN_MEMENTO_PANE)
        .expect("the picture is in the view");
    assert_eq!(pane.transform, Some(CABIN_MEMENTO));
    assert_eq!((pane.logical.width, pane.logical.height), (362, 460));
    assert_eq!(pane.texel_offset, (0, 0));
    let frame = view.panes.iter().position(|p| p.id == FRAME_PANE).unwrap();
    let picture = view
        .panes
        .iter()
        .position(|p| p.id == CABIN_MEMENTO_PANE)
        .unwrap();
    assert!(picture < frame, "drawn before (behind) the frame");
}

/// **The start flow puts the pilot's stored memento in the frame, and it is
/// the one the Change Memento screen would show.**
#[test]
fn accept_parity5_cabin_memento_frame_start_flow_shows_the_pilots_memento() {
    let mut flow = StartFlow::new();
    flow.set_memento_pictures(pictures());
    let mut shell = shell();
    assert!(flow.sync_memento(&mut shell));
    let pane = shell.sections()["PASSENGERCABIN"]
        .panes
        .iter()
        .find(|pane| pane.id == CABIN_MEMENTO_PANE)
        .expect("the picture is drawn");
    assert_eq!(pane.art, picture_art(flow.memento()));
    assert!(!flow.sync_memento(&mut shell), "stable once drawn");
}

/// **Retail: the real section's skipped `PC_MEMENTO` row becomes a drawn pane
/// in front of the background and behind `PC_FRAME`, for each of the 24
/// pictures the installation holds.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_memento_frame_retail_section_draws_behind_the_frame() {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let screens = FrontEndScreens::open(&dir).expect("the original screen container opens");
    let layout = screens.original_layout().expect("the layout member reads");
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    CabinHub::NEW_PILOT
        .apply(&mut shell, &|art: &str| screens.art_extent(art))
        .expect("the hub applies");
    let mut flow = StartFlow::new();
    let mut found = BTreeMap::new();
    for entry in &MEMENTOS {
        let art = picture_art(entry.name);
        if let Some(extent) = screens.art_extent(&art) {
            found.insert(art, extent);
        }
    }
    assert!(found.contains_key(&picture_art(flow.memento())));
    flow.set_memento_pictures(found);
    assert!(flow.sync_memento(&mut shell));
    let names = order(&shell);
    let picture = names
        .iter()
        .position(|id| id == CABIN_MEMENTO_PANE)
        .unwrap();
    let frame = names.iter().position(|id| id == FRAME_PANE).unwrap();
    assert_eq!(picture + 1, frame, "directly behind the frame: {names:?}");
}
