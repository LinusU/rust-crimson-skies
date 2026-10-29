//! Subtitles and the HUD scale as the live accessibility session asks for them
//! (F52-W2).
//!
//! [`HudPresentation::from_session`] reads the session's UI scale, subtitle
//! switch and colour filter once per frame; the HUD and menu draw from the
//! result. Subtitles are drawn only while the switch is on, at the scaled text
//! size of [`crate::accessibility::cues::scaled`]. A line with no text is
//! reported, never drawn as an empty box. The base text size is **designed**;
//! see `docs/findings/2026-10-27-f52-w2-presentation-output.md`.

use std::fmt;

use bevy::prelude::{
    App, BackgroundColor, Color, Commands, Component, GlobalZIndex, JustifyContent, Node, Plugin,
    PositionType, Query, Res, ResMut, Resource, Startup, Text, TextColor, TextFont, UiRect, Update,
    Val, Visibility, With, Without, default,
};
use bevy::text::FontSize;
use cs_content::settings::ColourFilter;

use crate::accessibility::cues::scaled;
use crate::accessibility::session::SettingsSession;
use crate::audio::AudioSession;
use crate::camera::effects::PresentedEffects;
use crate::render::presentation::LivePresentation;

/// One spoken line to subtitle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleLine {
    /// Who speaks.
    pub speaker: String,
    /// What they say.
    pub text: String,
}

/// A subtitle that could not be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleError {
    /// The speaker of the empty line.
    pub speaker: String,
}

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "subtitle from {:?} has no text", self.speaker)
    }
}

impl std::error::Error for SubtitleError {}

/// What the HUD, menu and subtitle layer draw this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HudPresentation {
    /// The UI scale in percent.
    pub ui_scale_percent: u16,
    /// The colour filter the final image is drawn through.
    pub colour_filter: ColourFilter,
    /// Text height in pixels at the UI scale.
    pub text_px: u32,
    /// The subtitle lines to draw; empty while subtitles are off.
    pub subtitles: Vec<SubtitleLine>,
    /// Lines that could not be drawn.
    pub errors: Vec<SubtitleError>,
}

impl HudPresentation {
    /// Reads `session` now and lays out `lines`.
    #[must_use]
    pub fn from_session(session: &SettingsSession, lines: &[SubtitleLine]) -> Self {
        let presentation = session.presentation();
        let mut subtitles = Vec::new();
        let mut errors = Vec::new();
        if presentation.subtitles {
            for line in lines {
                if line.text.trim().is_empty() {
                    errors.push(SubtitleError {
                        speaker: line.speaker.clone(),
                    });
                } else {
                    subtitles.push(line.clone());
                }
            }
        }
        Self {
            ui_scale_percent: presentation.ui_scale_percent,
            colour_filter: presentation.colour_filter,
            text_px: scaled(presentation).text_px,
            subtitles,
            errors,
        }
    }
}

// ------------------------------------------------------------ the wire ---
//
// F52-W2-wire: `drive_hud_presentation` is the production pass — one per
// rendered frame, after `advance_radio` and `present_game_events` — that folds
// the live session's settings and the radio queue's active line into
// [`PresentedHud`]. `SubtitleHudPlugin`'s widget is the only place those
// subtitles reach the screen, and [`PresentedEffects`]'s flash the only place
// the presented flash does, so removing the pass leaves the screen silent.

/// The strongest screen flash the presented intensity draws (designed):
/// a full-intensity flash is a bright veil, never an opaque white-out.
pub const FLASH_MAX_ALPHA: f32 = 0.65;
/// The base subtitle size the session's scale multiplies, in px (designed).
pub const SUBTITLE_BASE_PX: f32 = 18.0;

/// Resource: what the HUD layer draws this frame (F52-W2-wire).
///
/// `drive_hud_presentation` rewrites it every rendered frame: the laid-out
/// [`HudPresentation`] under the live settings (or the designed defaults
/// while no session is installed — the same defaults the session-less HUD
/// already had), plus the presented flash intensity the frame's effects
/// asked for. `errors` are recorded, never drawn as empty boxes.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct PresentedHud {
    /// The frame's laid-out HUD presentation.
    pub hud: HudPresentation,
    /// The presented flash intensity, `0.0..=1.0` after [`EffectFrame`]'s
    /// own validation; `0.0` while reduced flash is on, no session is
    /// installed or nothing flashed.
    ///
    /// [`EffectFrame`]: crate::camera::effects::EffectFrame
    pub flash: f32,
}

impl Default for PresentedHud {
    fn default() -> Self {
        // The session-less face: designed scale, no filter, no subtitles —
        // what a world with no accessibility session already drew.
        Self {
            hud: HudPresentation {
                ui_scale_percent: 100,
                colour_filter: ColourFilter::Off,
                text_px: SUBTITLE_BASE_PX as u32,
                subtitles: Vec::new(),
                errors: Vec::new(),
            },
            flash: 0.0,
        }
    }
}

/// Lays out the frame's HUD presentation under the live session (F52-W2-wire).
///
/// The subtitles are the radio queue's active line — the line `advance_radio`
/// is voicing this frame — so the text follows the same tick clock the audio
/// does and ends when the line does. The flash is [`PresentedEffects`]'s own,
/// already filtered by `EffectFrame::present`: reduced flash reaches this
/// resource as `0.0`, not as a drawn-then-hidden value.
///
/// Runs in `Update` after `present_game_events`, so the presented frame it
/// reads is this frame's own.
pub fn drive_hud_presentation(
    live: Option<Res<LivePresentation>>,
    session: Option<Res<AudioSession>>,
    effects: Option<Res<PresentedEffects>>,
    mut presented: ResMut<PresentedHud>,
) {
    let lines: Vec<SubtitleLine> = session
        .as_deref()
        .and_then(|session| session.radio.active())
        .and_then(|line| {
            line.subtitle.as_ref().map(|text| SubtitleLine {
                speaker: line.speaker.clone(),
                text: text.clone(),
            })
        })
        .into_iter()
        .collect();
    presented.hud = live.as_deref().map_or_else(
        || PresentedHud::default().hud,
        |live| HudPresentation::from_session(&live.0, &lines),
    );
    presented.flash = effects
        .as_deref()
        .and_then(|effects| effects.frame.as_ref())
        .map_or(0.0, |frame| frame.flash());
}

/// The widget: a bottom-centred subtitle line and a full-screen flash veil,
/// windowed compositions only (the headless face has no renderer; the
/// [`PresentedHud`] resource above is what the acceptance tests drive).
#[derive(Clone, Copy, Debug, Default)]
pub struct SubtitleHudPlugin;

impl Plugin for SubtitleHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_subtitle_hud)
            .add_systems(Update, draw_subtitle_hud);
    }
}

/// The full-screen veil `draw_subtitle_hud` fades to the presented flash.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FlashVeil;

/// The subtitle row `draw_subtitle_hud` writes the active line into.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SubtitleText;

fn spawn_subtitle_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        // Below the cinematic letterbox (`GlobalZIndex(1000)`), above the HUD.
        GlobalZIndex(900),
        Visibility::Hidden,
        FlashVeil,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(140.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                padding: UiRect::all(Val::Px(6.0)),
                ..default()
            },
            Visibility::Inherited,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(SUBTITLE_BASE_PX),
                    ..default()
                },
                TextColor(Color::WHITE),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                Visibility::Hidden,
                SubtitleText,
            ));
        });
}

type SubtitleWidgets<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text,
        &'static mut TextFont,
        &'static mut Visibility,
    ),
    (With<SubtitleText>, Without<FlashVeil>),
>;
type VeilWidgets<'w, 's> = Query<
    'w,
    's,
    (&'static mut BackgroundColor, &'static mut Visibility),
    (With<FlashVeil>, Without<SubtitleText>),
>;

/// Draws [`PresentedHud`]: the subtitle at the session-scaled text size while
/// the switch is on, and the veil at the presented flash intensity.
fn draw_subtitle_hud(
    presented: Res<PresentedHud>,
    mut subtitles: SubtitleWidgets,
    mut veils: VeilWidgets,
) {
    let subtitle = presented.hud.subtitles.first();
    for (mut text, mut font, mut visibility) in &mut subtitles {
        if let Some(line) = subtitle {
            text.0 = format!("{}: {}", line.speaker, line.text);
            font.font_size = FontSize::Px(presented.hud.text_px as f32);
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
    for (mut background, mut visibility) in &mut veils {
        if presented.flash > 0.0 {
            *background = BackgroundColor(Color::srgba(
                1.0,
                1.0,
                1.0,
                presented.flash * FLASH_MAX_ALPHA,
            ));
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}
