use std::collections::BTreeSet;

use cs_app::accessibility::cues::{ObjectiveStatus, Shape};
use cs_app::accessibility::objective_page::{from_view, objective_page, status_of};
use cs_app::input::SessionMode;
use cs_app::objectives::{DisplayedObjective, ObjectiveSession, lower_program};
use cs_app::ui::hud::{AircraftSample, HudSession, MissionPage, MissionSources, PageView};
use cs_content::hud::HudPolicy;
use cs_content::objectives::declared_synthetic_objectives;
use cs_content::settings::{ColourFilter, MAX_UI_SCALE_PERCENT, Presentation};
use cs_script::ir::SymbolId;
use cs_script::runtime::SessionGeneration;
use cs_sim::objectives::state::ObjectiveState;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{ActorId, SessionId};
use cs_types::space::Quaternion;

const STATES: [ObjectiveState; 6] = [
    ObjectiveState::Pending,
    ObjectiveState::Active,
    ObjectiveState::Succeeded,
    ObjectiveState::Failed,
    ObjectiveState::Optional,
    ObjectiveState::Superseded,
];

fn rows() -> Vec<DisplayedObjective> {
    STATES
        .iter()
        .enumerate()
        .map(|(i, state)| DisplayedObjective {
            symbol: SymbolId(i as u32 + 1),
            content: ContentId::from_source(ContentKind::Objective, &format!("synthetic.row{i}"))
                .unwrap(),
            state: *state,
            revealed: true,
        })
        .collect()
}

fn large_monochrome() -> Presentation {
    let mut p = Presentation::designed();
    p.ui_scale_percent = MAX_UI_SCALE_PERCENT;
    p.colour_filter = ColourFilter::Monochrome;
    p
}

#[test]
fn accept_f52_b_objectives_read_by_shape_and_text_with_colour_off_at_large_scale() {
    let page = objective_page(&rows(), &large_monochrome(), 10_000);
    assert_eq!(page.lines.len(), STATES.len());
    for line in &page.lines {
        assert_eq!(
            line.cue.colour, None,
            "the filter removes every colour role"
        );
    }
    let shapes: BTreeSet<_> = page
        .lines
        .iter()
        .map(|l| format!("{:?}", l.cue.shape))
        .collect();
    let texts: BTreeSet<_> = page.lines.iter().map(|l| l.cue.text_key).collect();
    assert_eq!(shapes.len(), STATES.len(), "every state has its own shape");
    assert_eq!(texts.len(), STATES.len(), "every state has its own word");
    // The line order is the display's order, unchanged by the settings.
    let statuses: Vec<_> = page.lines.iter().map(|l| l.status).collect();
    assert_eq!(statuses, ObjectiveStatus::ALL.to_vec());
    assert_eq!(page.lines[2].cue.shape, Shape::Check);
    assert_eq!(page.lines[3].cue.shape, Shape::Cross);
}

#[test]
fn accept_f52_b_large_scale_grows_the_lines_and_never_drops_one() {
    let small = objective_page(&rows(), &Presentation::designed(), 200);
    let large = objective_page(&rows(), &large_monochrome(), 200);
    assert_eq!(small.max_scroll_px(), 0, "six small lines fit 200 px");
    assert_eq!(large.lines.len(), small.lines.len());
    assert_eq!(large.metrics.text_px, small.metrics.text_px * 3);
    assert_eq!(large.content_px(), small.content_px() * 3);
    // At 300 % the page no longer fits and scrolls; every line is reachable
    // and fully visible at its offset, without any line being cut.
    assert!(large.max_scroll_px() > 0);
    assert!(large.fully_visible(0).len() < large.lines.len());
    let mut seen = BTreeSet::new();
    for index in 0..large.lines.len() {
        let offset = large
            .scroll_to_show(index, 0)
            .expect("each line fits alone");
        assert!(large.fully_visible(offset).contains(&index));
        seen.insert(index);
    }
    assert_eq!(seen.len(), STATES.len());
    // Scrolling back up to an earlier line also works.
    let last = large.scroll_to_show(5, 0).unwrap();
    assert_eq!(large.scroll_to_show(0, last), Some(0));
    // A viewport shorter than one line cannot show it.
    let tiny = objective_page(&rows(), &large_monochrome(), 10);
    assert_eq!(tiny.scroll_to_show(0, 0), None);
}

#[test]
fn accept_f52_b_hidden_has_no_cue_and_the_colour_filter_leaves_status_alone() {
    assert_eq!(status_of(ObjectiveState::Hidden), None);
    let mut hidden = rows();
    hidden[0].state = ObjectiveState::Hidden;
    assert_eq!(
        objective_page(&hidden, &Presentation::designed(), 1000)
            .lines
            .len(),
        STATES.len() - 1
    );
    let plain = objective_page(&rows(), &Presentation::designed(), 1000);
    for filter in ColourFilter::ALL {
        let mut p = Presentation::designed();
        p.colour_filter = filter;
        let filtered = objective_page(&rows(), &p, 1000);
        for (a, b) in plain.lines.iter().zip(&filtered.lines) {
            assert_eq!(
                (a.status, a.cue.shape, a.cue.text_key),
                (b.status, b.cue.shape, b.cue.text_key)
            );
        }
    }
}

#[test]
fn accept_f52_b_the_hud_sessions_objectives_page_is_read_through_the_cues() {
    let objectives = ObjectiveSession::launch(
        lower_program(&declared_synthetic_objectives()).unwrap(),
        SessionGeneration(1),
    )
    .unwrap();
    let session = SessionId::new(7).unwrap();
    let actor = ActorId { session, serial: 1 };
    let mut hud = HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).unwrap();
    hud.bind(session, actor).unwrap();
    hud.open(MissionPage::Objectives);
    let sample = AircraftSample {
        session,
        actor,
        attitude: Quaternion::IDENTITY,
        velocity_mps: [0.0, 0.0, -50.0],
        wind_mps: [0.0; 3],
        height_m: 500.0,
        ground_height_m: Some(0.0),
    };
    let view = hud
        .view(
            &sample,
            &MissionSources {
                objectives: Some(objectives.display()),
                ..MissionSources::default()
            },
        )
        .unwrap();
    let page = from_view(&view, &large_monochrome(), 100).expect("the objectives page");
    assert_eq!(
        page.lines.len(),
        1,
        "only the born-revealed primary is listed"
    );
    assert_eq!(page.lines[0].status, ObjectiveStatus::Active);
    assert_eq!(page.lines[0].cue.colour, None);
    assert_eq!(
        page.lines[0].content,
        objectives.display().visible()[0].content
    );
    // Another page produces no objective page.
    hud.open(MissionPage::Recon);
    let other = hud.view(&sample, &MissionSources::default()).unwrap();
    assert!(!matches!(other, PageView::Objectives(_)));
    assert!(from_view(&other, &large_monochrome(), 100).is_none());
}
