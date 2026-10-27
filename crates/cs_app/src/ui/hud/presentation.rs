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

use cs_content::settings::ColourFilter;

use crate::accessibility::cues::scaled;
use crate::accessibility::session::SettingsSession;

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
