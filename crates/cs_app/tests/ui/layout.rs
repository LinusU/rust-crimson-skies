use cs_app::ui::front_end::{Action, LayoutProblem, Screen, check_layout};
use cs_content::ui_layout::{AspectFit, Hotspot, LayoutError, Rect, ScreenLayout};
use cs_types::content::ContentKind;

use super::id;

fn hotspot(key: &str, action: &str, rect: Rect) -> Hotspot {
    Hotspot {
        id: id(ContentKind::UiResource, key),
        action: action.to_owned(),
        rect,
    }
}

const fn rect(x: u32, y: u32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn accept_f45_a_hotspots_scale_and_offset_exactly_like_the_image() {
    // A 640x480 authored image on a 1920x1080 surface: height-limited,
    // 1440x1080, centred with a 240 pixel bar on each side.
    let fit = AspectFit::new((640, 480), (1920, 1080)).expect("fit");
    assert_eq!(fit.image_rect(), rect(240, 0, 1440, 1080));
    // The whole image maps to the fitted rectangle, so a hotspot covering it
    // does too.
    assert_eq!(fit.map_rect(rect(0, 0, 640, 480)), fit.image_rect());
    // 64x48 logical is exactly one tenth: 144x108 at the image's offset.
    assert_eq!(fit.map_rect(rect(64, 48, 64, 48)), rect(384, 108, 144, 108));

    // Width-limited: 640x480 on 800x800 leaves bars above and below.
    let wide = AspectFit::new((640, 480), (800, 800)).expect("fit");
    assert_eq!(wide.image_rect(), rect(0, 100, 800, 600));

    assert_eq!(AspectFit::new((0, 480), (800, 600)), None);
    assert_eq!(AspectFit::new((640, 480), (800, 0)), None);
}

#[test]
fn accept_f45_a_a_click_in_the_bars_hits_nothing_and_a_click_on_a_button_hits_it() {
    let layout = ScreenLayout::new(
        (640, 480),
        vec![
            hotspot("new-profile", "new-profile", rect(0, 0, 320, 240)),
            hotspot("quit", "quit", rect(320, 0, 320, 240)),
        ],
    )
    .expect("layout");
    let fit = AspectFit::new(layout.image(), (1920, 1080)).expect("fit");
    assert_eq!(layout.hit_test(&fit, 10, 10), None, "left bar");
    assert_eq!(layout.hit_test(&fit, 1900, 10), None, "right bar");
    let new_profile = layout.hit_test(&fit, 250, 10).expect("hit");
    assert_eq!(new_profile.action, "new-profile");
    // The shared edge belongs to the second hotspot, never to both or neither.
    let edge = fit.map_rect(rect(320, 0, 320, 240)).x;
    assert_eq!(
        layout.hit_test(&fit, edge - 1, 10).expect("hit").action,
        "new-profile"
    );
    assert_eq!(layout.hit_test(&fit, edge, 10).expect("hit").action, "quit");
}

#[test]
fn accept_f45_a_layout_validation_refuses_each_malformed_hotspot() {
    let ok = hotspot("a", "back", rect(0, 0, 10, 10));
    assert_eq!(
        ScreenLayout::new((0, 10), vec![]),
        Err(LayoutError::EmptyImage)
    );
    assert!(matches!(
        ScreenLayout::new((100, 100), vec![ok.clone(), ok.clone()]),
        Err(LayoutError::DuplicateHotspot { .. })
    ));
    assert!(matches!(
        ScreenLayout::new((100, 100), vec![hotspot("a", "back", rect(95, 0, 10, 10))]),
        Err(LayoutError::OutOfBounds { .. })
    ));
    assert!(matches!(
        ScreenLayout::new((100, 100), vec![hotspot("a", "back", rect(0, 0, 0, 10))]),
        Err(LayoutError::OutOfBounds { .. })
    ));
    assert!(matches!(
        ScreenLayout::new((100, 100), vec![hotspot("a", "", rect(0, 0, 10, 10))]),
        Err(LayoutError::EmptyAction { .. })
    ));
    let wrong_kind = Hotspot {
        id: id(ContentKind::Image, "a"),
        ..ok
    };
    assert!(matches!(
        ScreenLayout::new((100, 100), vec![wrong_kind]),
        Err(LayoutError::WrongKind { .. })
    ));
}

#[test]
fn accept_f45_a_a_visible_button_must_have_a_transition_on_its_screen() {
    let layout = ScreenLayout::new(
        (640, 480),
        vec![
            hotspot("briefing", "open-briefing", rect(0, 0, 100, 100)),
            hotspot("scrapbook", "open-scrapbook", rect(100, 0, 100, 100)),
            hotspot("back", "back", rect(200, 0, 100, 100)),
        ],
    )
    .expect("layout");
    assert_eq!(check_layout(Screen::Cabin, &layout), vec![]);

    // The same layout on the recon screen: two buttons go nowhere.
    let problems = check_layout(Screen::Recon, &layout);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems.iter().all(|p| matches!(
        p,
        LayoutProblem::NoTransition {
            action: Action::OpenBriefing | Action::OpenScrapbook,
            ..
        }
    )));

    let odd = ScreenLayout::new(
        (640, 480),
        vec![
            hotspot("ghost", "no-such-action", rect(0, 0, 10, 10)),
            hotspot("loaded", "load-succeeded", rect(10, 0, 10, 10)),
        ],
    )
    .expect("layout");
    let problems = check_layout(Screen::Loading, &odd);
    assert!(matches!(problems[0], LayoutProblem::UnknownAction { .. }));
    assert!(matches!(
        problems[1],
        LayoutProblem::NotAButton {
            action: Action::LoadSucceeded,
            ..
        }
    ));
}
