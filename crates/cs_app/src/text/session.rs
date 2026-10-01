//! The localization runtime menus, HUD and subtitles share (F51-C).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-C` (non-negotiable behaviors 4 and 5). Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! F51-B built [`layout_localized_text`], the one production call a screen
//! makes: resolve an id, parse its markup, lay it out and surface every
//! diagnostic. What it deliberately did **not** own is the runtime that call
//! needs — the selected locale, the decoded catalog, the loaded fonts — and the
//! three consumers F51's stage C names: the menus, the HUD and the subtitles.
//! [`TextSession`] is that runtime.
//!
//! * It owns exactly one [`LocaleChain`], one [`TextCatalog`], one
//!   [`MarkupGrammar`] and one [`FontSet`], so menus, HUD and subtitles cannot
//!   each resolve the same id differently.
//! * [`TextSession::switch_locale`] is the locale teardown/retry: the offered
//!   catalog is checked before it is installed, and a catalog that answers
//!   nothing for its chain is refused with the previous locale still in place, so
//!   a half-loaded locale cannot blank every label. The caller retries once the
//!   content is repaired.
//! * [`TextSession::retry_font`] is the same for a font a measurer could not
//!   read on a previous attempt.
//! * [`TextSession::menu_text`], [`TextSession::hud_text`] and
//!   [`TextSession::subtitle_text`] bind the resolved text to the *locale-free*
//!   identity the consumer keys on: a menu element id, a HUD field id, or a
//!   speaker plus the exact cue id and its tick window. Switching locale changes
//!   the text and nothing else, which is F51 non-negotiable behavior 5 —
//!   "changing locale cannot change save ids, mission identity, numeric parsing
//!   or network protocol values".
//! * A miss stays a named [`TextSessionError::Missing`] and never an empty
//!   label, and a subtitle that is too short or too late is refused before it is
//!   bound rather than silently truncated.
//!
//! # What is designed and what is unknown
//!
//! The original supported-locale list, control-markup spelling, font format and
//! text metrics all remain unmeasured; the chain, grammar and measurer stay
//! caller-declared, and the unknowns are recorded in
//! `docs/findings/2026-10-01-f51-c-menus-hud-subtitles-and-font-loading.md`.
//! No font is parsed here and no glyph is rasterized: the session carries the
//! measured numbers a real decode (F51-D) will supply.

use std::fmt;

use bevy::math::Rect;
use cs_content::localization::{
    LocaleChain, LocaleId, MarkupGrammar, SubstitutionTable, TextCatalog, TextId,
};
use cs_types::content::ContentId;

use super::fonts::{FontLoadError, FontMeasurer, FontSet};
use super::layout::{LayoutError, RequiredControl};
use super::screen::{ScreenText, ScreenTextError, ScreenTextRequest, layout_localized_text};

/// Why a text session operation failed.
#[derive(Clone, Debug, PartialEq)]
pub enum TextSessionError {
    /// The session's default font, or a font a request named, is not loaded.
    /// Nothing is laid out, so no text is drawn with unmeasured metrics.
    MissingFont {
        /// The font that is not loaded.
        id: ContentId,
    },
    /// No locale in the session's chain answered the id, so there is no text to
    /// show. `tried` names every locale walked, in order.
    Missing {
        /// The requested string id.
        id: TextId,
        /// Every locale that was tried.
        tried: Vec<LocaleId>,
    },
    /// The resolved string's panel has nowhere to put the text.
    Layout(LayoutError),
    /// A locale switch was refused because the offered catalog answers no id for
    /// its chain. The previous catalog stays installed, so the caller can retry.
    NoContentForLocale {
        /// The chain the offered catalog was checked against.
        chain: Vec<LocaleId>,
    },
    /// A subtitle named no speaker.
    EmptySpeaker,
    /// A subtitle declared zero duration, so it would never be visible.
    ZeroDuration,
    /// A subtitle's tick window overflowed: the sum of the start tick and the
    /// duration is not representable.
    TickOverflow {
        /// The declared start tick.
        start_tick: u64,
        /// The declared duration.
        duration_ticks: u64,
    },
}

impl fmt::Display for TextSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFont { id } => write!(f, "font {id} is not loaded"),
            Self::Missing { id, tried } => {
                let labels: Vec<&str> = tried.iter().map(LocaleId::as_str).collect();
                write!(f, "no locale answered {id} (tried {})", labels.join(", "))
            }
            Self::Layout(error) => write!(f, "{error}"),
            Self::NoContentForLocale { chain } => {
                let labels: Vec<&str> = chain.iter().map(LocaleId::as_str).collect();
                write!(
                    f,
                    "the offered catalog answers nothing for locale chain {}",
                    labels.join(", ")
                )
            }
            Self::EmptySpeaker => f.write_str("a subtitle must name a speaker"),
            Self::ZeroDuration => f.write_str("a subtitle must last at least one tick"),
            Self::TickOverflow {
                start_tick,
                duration_ticks,
            } => write!(
                f,
                "a subtitle starting at tick {start_tick} for {duration_ticks} ticks overflows the tick range"
            ),
        }
    }
}

impl std::error::Error for TextSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::MissingFont { .. }
            | Self::Missing { .. }
            | Self::NoContentForLocale { .. }
            | Self::EmptySpeaker
            | Self::ZeroDuration
            | Self::TickOverflow { .. } => None,
        }
    }
}

impl From<ScreenTextError> for TextSessionError {
    fn from(error: ScreenTextError) -> Self {
        match error {
            ScreenTextError::Missing { id, tried } => Self::Missing { id, tried },
            ScreenTextError::Layout(error) => Self::Layout(error),
        }
    }
}

/// The one localization runtime menus, HUD and subtitles share.
///
/// A session is built from already-decoded pieces (the F51-B bridge turns F12
/// rows into a [`TextCatalog`]) and already-loaded fonts, and it is the only
/// thing that decides which locale and which font a screen reads. It owns no
/// game state and writes nothing: it is presentation-only, which is what keeps a
/// locale change away from save ids and unlocks.
#[derive(Clone, Debug)]
pub struct TextSession {
    chain: LocaleChain,
    catalog: TextCatalog,
    grammar: MarkupGrammar,
    fonts: FontSet,
    default_font: ContentId,
}

impl TextSession {
    /// Builds a session from its parts.
    ///
    /// # Errors
    ///
    /// [`TextSessionError::MissingFont`] when `default_font` is not in `fonts`.
    /// A session that cannot draw its own default text is not a usable session.
    pub fn try_new(
        chain: LocaleChain,
        catalog: TextCatalog,
        grammar: MarkupGrammar,
        fonts: FontSet,
        default_font: ContentId,
    ) -> Result<Self, TextSessionError> {
        if !fonts.contains(&default_font) {
            return Err(TextSessionError::MissingFont { id: default_font });
        }
        Ok(Self {
            chain,
            catalog,
            grammar,
            fonts,
            default_font,
        })
    }

    /// The locale chain in force, the selected locale first.
    #[must_use]
    pub fn chain(&self) -> &LocaleChain {
        &self.chain
    }

    /// The decoded strings in force.
    #[must_use]
    pub fn catalog(&self) -> &TextCatalog {
        &self.catalog
    }

    /// The declared markup grammar every string is validated against.
    #[must_use]
    pub fn grammar(&self) -> &MarkupGrammar {
        &self.grammar
    }

    /// The loaded fonts.
    #[must_use]
    pub fn fonts(&self) -> &FontSet {
        &self.fonts
    }

    /// The font a request that names none is laid out with.
    #[must_use]
    pub fn default_font(&self) -> &ContentId {
        &self.default_font
    }

    /// Switches the session to a new locale chain and catalog.
    ///
    /// The offered catalog is **checked before it is installed**: a catalog that
    /// answers not one id for its chain is refused with
    /// [`TextSessionError::NoContentForLocale`], and the previous locale stays
    /// fully in force. That is the teardown/retry rule this stage owns — a locale
    /// switch that cannot complete must leave the running screen readable, not
    /// blank, and the caller retries the same call once the missing content is
    /// loaded.
    ///
    /// # Errors
    ///
    /// [`TextSessionError::NoContentForLocale`].
    pub fn switch_locale(
        &mut self,
        chain: LocaleChain,
        catalog: TextCatalog,
    ) -> Result<(), TextSessionError> {
        if catalog.audit(&chain).resolved == 0 {
            return Err(TextSessionError::NoContentForLocale {
                chain: chain.locales().to_vec(),
            });
        }
        self.chain = chain;
        self.catalog = catalog;
        Ok(())
    }

    /// Retries loading one font face after a previous attempt failed.
    ///
    /// Fonts are locale-independent, so a locale switch never unloads them; a
    /// font that could not be measured fails where it is used, and this is the
    /// retry that repairs it once the caller's measurer can read it.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Unmeasured`].
    pub fn retry_font(
        &mut self,
        face: &cs_content::localization::FontFace,
        measurer: &dyn FontMeasurer,
    ) -> Result<(), FontLoadError> {
        self.fonts.reload(face, measurer)
    }

    /// Changes the font a request that names none is laid out with.
    ///
    /// # Errors
    ///
    /// [`TextSessionError::MissingFont`] when `id` is not loaded.
    pub fn set_default_font(&mut self, id: ContentId) -> Result<(), TextSessionError> {
        if !self.fonts.contains(&id) {
            return Err(TextSessionError::MissingFont { id });
        }
        self.default_font = id;
        Ok(())
    }

    /// Resolves, parses and lays one string out with the session's locale,
    /// grammar and a font.
    fn layout(
        &self,
        id: &TextId,
        panel: Rect,
        required: &[RequiredControl],
        substitutions: &SubstitutionTable,
        font: Option<&ContentId>,
    ) -> Result<ScreenText, TextSessionError> {
        let font_id = font.unwrap_or(&self.default_font);
        let metrics = self
            .fonts
            .metrics(font_id)
            .ok_or_else(|| TextSessionError::MissingFont {
                id: font_id.clone(),
            })?;
        let screen = layout_localized_text(&ScreenTextRequest {
            catalog: &self.catalog,
            id,
            chain: &self.chain,
            grammar: &self.grammar,
            metrics,
            substitutions,
            panel,
            required,
        })?;
        Ok(screen)
    }

    /// Lays out one menu row's label, bound to its locale-free element id.
    ///
    /// # Errors
    ///
    /// [`TextSessionError::MissingFont`], [`TextSessionError::Missing`] or
    /// [`TextSessionError::Layout`].
    pub fn menu_text(&self, request: &MenuTextRequest<'_>) -> Result<MenuText, TextSessionError> {
        let text = self.layout(
            request.label,
            request.panel,
            request.required,
            request.substitutions,
            request.font,
        )?;
        Ok(MenuText {
            element: request.element.clone(),
            text,
        })
    }

    /// Lays out one HUD readout's label, bound to its locale-free field id.
    ///
    /// # Errors
    ///
    /// [`TextSessionError::MissingFont`], [`TextSessionError::Missing`] or
    /// [`TextSessionError::Layout`].
    pub fn hud_text(&self, request: &HudTextRequest<'_>) -> Result<HudText, TextSessionError> {
        let text = self.layout(
            request.label,
            request.panel,
            request.required,
            request.substitutions,
            request.font,
        )?;
        Ok(HudText {
            field: request.field.clone(),
            text,
        })
    }

    /// Lays out one dialogue cue, bound to its exact speaker, cue id and tick
    /// window (F51 non-negotiable behavior 4).
    ///
    /// A cue with no speaker or no duration is refused before it is bound, so a
    /// subtitle that could never be shown is an error at the producer rather than
    /// an invisible line on screen.
    ///
    /// # Errors
    ///
    /// [`TextSessionError::EmptySpeaker`], [`TextSessionError::ZeroDuration`],
    /// [`TextSessionError::TickOverflow`], [`TextSessionError::MissingFont`],
    /// [`TextSessionError::Missing`] or [`TextSessionError::Layout`].
    pub fn subtitle_text(
        &self,
        request: &SubtitleRequest<'_>,
    ) -> Result<SubtitleText, TextSessionError> {
        if request.speaker.trim().is_empty() {
            return Err(TextSessionError::EmptySpeaker);
        }
        if request.duration_ticks == 0 {
            return Err(TextSessionError::ZeroDuration);
        }
        let end_tick = request
            .start_tick
            .checked_add(request.duration_ticks)
            .ok_or(TextSessionError::TickOverflow {
                start_tick: request.start_tick,
                duration_ticks: request.duration_ticks,
            })?;
        let text = self.layout(
            request.cue,
            request.panel,
            request.required,
            request.substitutions,
            request.font,
        )?;
        Ok(SubtitleText {
            speaker: request.speaker.to_owned(),
            cue: request.cue.clone(),
            start_tick: request.start_tick,
            end_tick,
            text,
        })
    }
}

/// Everything one menu row's label needs.
#[derive(Clone, Debug)]
pub struct MenuTextRequest<'a> {
    /// The menu element the text belongs to: a `ui_resource` id that is stable
    /// across locales, so it can key focus, painting and unlock state.
    pub element: &'a ContentId,
    /// The string to resolve.
    pub label: &'a TextId,
    /// The panel the label lives in.
    pub panel: Rect,
    /// The controls inside the panel the label must never cover.
    pub required: &'a [RequiredControl],
    /// The substitution values the screen supplies.
    pub substitutions: &'a SubstitutionTable,
    /// The font to draw with, or the session's default.
    pub font: Option<&'a ContentId>,
}

/// One menu row's localized label, bound to its element.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuText {
    element: ContentId,
    text: ScreenText,
}

impl MenuText {
    /// The locale-free element the label belongs to.
    #[must_use]
    pub fn element(&self) -> &ContentId {
        &self.element
    }

    /// The resolved, parsed and laid-out label.
    #[must_use]
    pub fn text(&self) -> &ScreenText {
        &self.text
    }
}

/// Everything one HUD readout's label needs.
#[derive(Clone, Debug)]
pub struct HudTextRequest<'a> {
    /// The HUD field the text belongs to: a `ui_resource` id that is stable
    /// across locales.
    pub field: &'a ContentId,
    /// The string to resolve.
    pub label: &'a TextId,
    /// The panel the label lives in.
    pub panel: Rect,
    /// The controls inside the panel the label must never cover.
    pub required: &'a [RequiredControl],
    /// The substitution values the screen supplies.
    pub substitutions: &'a SubstitutionTable,
    /// The font to draw with, or the session's default.
    pub font: Option<&'a ContentId>,
}

/// One HUD readout's localized label, bound to its field.
#[derive(Clone, Debug, PartialEq)]
pub struct HudText {
    field: ContentId,
    text: ScreenText,
}

impl HudText {
    /// The locale-free field the label belongs to.
    #[must_use]
    pub fn field(&self) -> &ContentId {
        &self.field
    }

    /// The resolved, parsed and laid-out label.
    #[must_use]
    pub fn text(&self) -> &ScreenText {
        &self.text
    }
}

/// Everything one dialogue subtitle cue needs.
#[derive(Clone, Debug)]
pub struct SubtitleRequest<'a> {
    /// Who speaks. Matched against the radio line's speaker, so two speakers
    /// saying the same cue are two different subtitles.
    pub speaker: &'a str,
    /// The exact cue string id.
    pub cue: &'a TextId,
    /// The simulation tick the cue becomes visible.
    pub start_tick: u64,
    /// How many simulation ticks it stays visible. Nonzero: completion is
    /// measured in ticks, never by an audio callback (F41).
    pub duration_ticks: u64,
    /// The panel the subtitle lives in.
    pub panel: Rect,
    /// The controls inside the panel the subtitle must never cover.
    pub required: &'a [RequiredControl],
    /// The substitution values the screen supplies.
    pub substitutions: &'a SubstitutionTable,
    /// The font to draw with, or the session's default.
    pub font: Option<&'a ContentId>,
}

/// One dialogue subtitle, bound to its exact speaker, cue id and tick window.
#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleText {
    speaker: String,
    cue: TextId,
    start_tick: u64,
    end_tick: u64,
    text: ScreenText,
}

impl SubtitleText {
    /// Who speaks.
    #[must_use]
    pub fn speaker(&self) -> &str {
        &self.speaker
    }

    /// The exact cue string id.
    #[must_use]
    pub fn cue(&self) -> &TextId {
        &self.cue
    }

    /// The tick the cue becomes visible.
    #[must_use]
    pub fn start_tick(&self) -> u64 {
        self.start_tick
    }

    /// The tick the cue stops being visible (exclusive).
    #[must_use]
    pub fn end_tick(&self) -> u64 {
        self.end_tick
    }

    /// Whether the cue is visible at `tick`.
    #[must_use]
    pub fn is_visible_at(&self, tick: u64) -> bool {
        (self.start_tick..self.end_tick).contains(&tick)
    }

    /// The resolved, parsed and laid-out cue text.
    #[must_use]
    pub fn text(&self) -> &ScreenText {
        &self.text
    }
}
