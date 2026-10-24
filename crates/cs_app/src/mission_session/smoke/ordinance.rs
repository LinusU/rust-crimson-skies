//! The Change Ammo detour of the Flight Check stage (task #2070): once the
//! Flight Check's own frames are in, the script presses Change Ammo for the
//! pilot, photographs the `OrdinanceLayout` screen, opens the first ammunition
//! list by a click, photographs that, and leaves by Escape (the list closes,
//! then the screen), after which the Flight Check goes on to Fly Mission.
//!
//! The frames are `frame_olNNNNN_ordinance_layout.png` and
//! `frame_olNNNNN_ordinance_list.png` in the capture directory. They are
//! evidence to look at, not a pass/fail check: the screen's boxes, frames and
//! text are stand-ins (`ordinance_screen.rs`).

use bevy::prelude::{Commands, Query, Res, ResMut, Window};
use cs_content::ui_layout::{AspectFit, Rect};

use super::super::flight_check::{FlightCheckPhase, PreMissionStage};
use super::{SHOT_GRACE_FRAMES, SmokeConfig, SmokeData, SmokeShots, request_screenshot};
use crate::ui::front_end::SECTION_SURFACE;

/// Frames on the screen before the first photograph.
const SETTLE: u32 = 6;
/// The frame the first ammunition box is clicked open.
const OPEN_LIST: u32 = 10;
/// The frame the open list is photographed.
const LIST_SHOT: u32 = 14;

/// The detour's progress.
#[derive(Clone, Debug, Default)]
pub(in super::super) struct OrdinanceRun {
    /// 0 not started, 1 opened, 2 done.
    pub(in super::super) step: u8,
    /// Frames the screen has been up.
    frame: u32,
}

pub(in super::super) fn smoke_ordinance(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    phase: Option<ResMut<FlightCheckPhase>>,
    shots: Res<SmokeShots>,
    windows: Query<&Window>,
    mut commands: Commands,
) {
    let Some(mut phase) = phase else { return };
    if phase.stage() != PreMissionStage::Ordinance {
        return;
    }
    data.ordinance.frame += 1;
    let at = data.ordinance.frame;
    let mut request = |label: &str, data: &mut SmokeData| {
        data.shots_requested += 1;
        data.requested.push(label.to_owned());
        request_screenshot(
            &mut commands,
            shots.clone(),
            config
                .request
                .capture_dir
                .join(format!("frame_ol{at:05}_{label}.png")),
            label.to_owned(),
            None,
            None,
            None,
        );
    };
    let arrived = |label: &str| {
        shots
            .0
            .lock()
            .is_ok_and(|shots| shots.iter().any(|shot| shot.label == label))
    };
    if at == SETTLE {
        request("ordinance_layout", &mut data);
    } else if at == OPEN_LIST {
        let Ok(window) = windows.single() else { return };
        let surface = (window.width() as u32, window.height() as u32);
        let Some(fit) = AspectFit::new(SECTION_SURFACE, surface) else {
            return;
        };
        let Some(stage) = phase.ordinance_stage() else {
            return;
        };
        let Some(closed) = stage.screen().ammo_box(0).map(|row| row.rect) else {
            return;
        };
        let mapped = fit.map_rect(Rect {
            x: closed.x.max(0) as u32,
            y: closed.y.max(0) as u32,
            width: closed.width,
            height: closed.height,
        });
        let response = phase.click(
            surface,
            mapped.x + mapped.width / 2,
            mapped.y + mapped.height / 2,
        );
        println!("mission flight check: smoke clicked the first ammunition box: {response:?}");
    } else if at == LIST_SHOT {
        request("ordinance_list", &mut data);
    } else if at > LIST_SHOT
        && (arrived("ordinance_list") && arrived("ordinance_layout")
            || at > LIST_SHOT + SHOT_GRACE_FRAMES)
    {
        // The list closes first, then the screen.
        let first = phase.press_back();
        let second = phase.press_back();
        println!(
            "mission flight check: smoke left the OrdinanceLayout screen: {first:?}, {second:?}"
        );
        data.ordinance.step = 2;
    }
}
