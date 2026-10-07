//! The original-asset screens of the front end (F45-B).
//!
//! Spec: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
//! stage `### F45-B`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Use an
//! explicit state table for every screen and Back/Cancel path. A UI action
//! requests a domain transaction, it does not directly edit campaign cash,
//! ownership or objective fields", "Authored images are aspect-fit with
//! logical hotspot coordinates transformed by the same scale/offset as the
//! image").
//!
//! F45-A built the state table ([`super::TABLE`]) and its machine
//! ([`super::FrontEnd`]) plus the authored-screen layout
//! ([`cs_content::ui_layout::ScreenLayout`]), all synthetic. This module puts
//! the two together for the screens a player reaches before flying:
//!
//! * [`ScreenAssets`] — one screen's artwork id and its hotspots, converted
//!   once into parsed [`Button`]s in **declaration order**, which is the order
//!   keyboard and controller focus visits them.
//! * [`ScreenDeck`] — the screens the application actually carries. A deck is
//!   validated against the state table: every authored hotspot must request an
//!   action this screen has a transition for (the F45-A [`super::check_layout`]
//!   rules), the art must offer an escape, and no designed button of the
//!   screen may be hidden from navigation.
//! * [`ScreenSession`] — the front end presented through a deck: pointer
//!   hit-testing through the *same* aspect-fit transform as the image, focus
//!   in the authored order, and every action applied by the machine's own
//!   table, so a click is the same transaction as the equivalent key press.
//!
//! A screen the deck does not carry is **reported**
//! ([`ScreenSessionError::ScreenNotInDeck`]) and the machine stays coherent on
//! it — never replaced with a placeholder screen. Which original artwork and
//! which original hotspot coordinates belong to which screen is *not* read
//! here: no original screen layout is decoded anywhere in this repository, so
//! the deck's data is whatever a loader supplies, and the unknowns are
//! recorded in `docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`
//! instead of guessed.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::ui_layout::{AspectFit, Rect, ScreenLayout};
use cs_types::content::{ContentId, ContentKind};

use super::{
    Action, ActionSource, ConstructionDraft, FrontEnd, LayoutProblem, Loadout, Outcome, Refusal,
    Screen, find, rows_on,
};

/// One authored button: its stable `ui-resource` id, the front-end action it
/// requests and its region in logical image coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    /// The `ui-resource` id of the button.
    pub id: ContentId,
    /// The action pressing it requests of the state table.
    pub action: Action,
    /// The region in logical image coordinates.
    pub rect: Rect,
}

/// Why a screen's assets were refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenAssetError {
    /// The artwork id is not an `image` content id, so no screen could be
    /// drawn from it.
    ArtNotImage {
        /// The offending id.
        art: ContentId,
    },
    /// A hotspot is not a working button; the same three rules
    /// [`super::check_layout`] states, checked while the hotspots are parsed.
    Layout {
        /// Which rule it broke.
        problem: LayoutProblem,
    },
}

impl fmt::Display for ScreenAssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArtNotImage { art } => {
                write!(f, "the screen artwork {art} is not an image")
            }
            Self::Layout { problem } => write!(f, "{problem:?}"),
        }
    }
}

impl std::error::Error for ScreenAssetError {}

/// One screen's authored front end: the artwork to draw and the buttons on it.
///
/// Built from a validated [`ScreenLayout`]; the hotspots are parsed into
/// actions once, so a deck can never hold a button whose key the state table
/// does not know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenAssets {
    art: ContentId,
    image: (u32, u32),
    buttons: Vec<Button>,
}

impl ScreenAssets {
    /// Parses the layout's hotspots into buttons in declaration order.
    ///
    /// # Errors
    ///
    /// [`ScreenAssetError::ArtNotImage`] when `art` is not an image id, or
    /// [`ScreenAssetError::Layout`] when a hotspot names an unknown action
    /// key or an application-side result rather than a button.
    pub fn new(art: ContentId, layout: &ScreenLayout) -> Result<Self, ScreenAssetError> {
        if art.kind() != ContentKind::Image {
            return Err(ScreenAssetError::ArtNotImage { art });
        }
        let image = layout.image();
        let mut buttons = Vec::with_capacity(layout.hotspots().len());
        for hotspot in layout.hotspots() {
            let name = hotspot.id.as_str().to_owned();
            let Some(action) = Action::from_key(&hotspot.action) else {
                return Err(ScreenAssetError::Layout {
                    problem: LayoutProblem::UnknownAction {
                        hotspot: name,
                        key: hotspot.action.clone(),
                    },
                });
            };
            if action.source() != ActionSource::User {
                return Err(ScreenAssetError::Layout {
                    problem: LayoutProblem::NotAButton {
                        hotspot: name,
                        action,
                    },
                });
            }
            buttons.push(Button {
                id: hotspot.id.clone(),
                action,
                rect: hotspot.rect,
            });
        }
        Ok(Self {
            art,
            image,
            buttons,
        })
    }

    /// The artwork id of the screen.
    #[must_use]
    pub fn art(&self) -> &ContentId {
        &self.art
    }

    /// The logical size of the authored image.
    #[must_use]
    pub fn image(&self) -> (u32, u32) {
        self.image
    }

    /// The buttons in authored (focus) order.
    #[must_use]
    pub fn buttons(&self) -> &[Button] {
        &self.buttons
    }

    /// The button focus lands on when the screen is entered.
    #[must_use]
    pub fn first_action(&self) -> Option<Action> {
        self.buttons.first().map(|button| button.action)
    }
}

/// Why a deck was refused; no deck is built, so no screen can be shown from
/// data that would hide or break a button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckError {
    /// Two entries name the same screen.
    DuplicateScreen {
        /// The repeated screen.
        screen: Screen,
    },
    /// A hotspot of this screen has no transition on it: pressing it would
    /// do nothing.
    Layout {
        /// The screen.
        screen: Screen,
        /// Which rule it broke.
        problem: LayoutProblem,
    },
    /// The artwork offers no escape button, so the screen could not be left.
    NoEscapeButton {
        /// The screen.
        screen: Screen,
    },
    /// The artwork hides a designed button of the screen from navigation.
    HiddenAction {
        /// The screen.
        screen: Screen,
        /// The button with no hotspot.
        action: Action,
    },
}

impl fmt::Display for DeckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateScreen { screen } => {
                write!(f, "{screen:?} is carried twice")
            }
            Self::Layout { screen, problem } => write!(f, "{screen:?}: {problem:?}"),
            Self::NoEscapeButton { screen } => {
                write!(
                    f,
                    "{screen:?}: the artwork offers no Back, Cancel, Quit or Pause button"
                )
            }
            Self::HiddenAction { screen, action } => write!(
                f,
                "{screen:?}: the artwork hides the designed button {}",
                action.key()
            ),
        }
    }
}

impl std::error::Error for DeckError {}

/// The screens the application carries, each with its authored artwork and
/// hotspots, validated against the state table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenDeck {
    assets: BTreeMap<Screen, ScreenAssets>,
}

impl ScreenDeck {
    /// Validates and builds a deck.
    ///
    /// The rules, per screen: no duplicate screen; every hotspot requests an
    /// action this screen has a transition for (the F45-A
    /// [`super::check_layout`] rules, which [`ScreenAssets::new`] already
    /// applied for the keys); the art offers an escape action; and every
    /// designed button of the screen (its user rows, in table order) has a
    /// hotspot, so navigation can reach it.
    ///
    /// # Errors
    ///
    /// The first [`DeckError`].
    pub fn new(entries: Vec<(Screen, ScreenAssets)>) -> Result<Self, DeckError> {
        let mut assets = BTreeMap::new();
        for (screen, screen_assets) in entries {
            if assets.contains_key(&screen) {
                return Err(DeckError::DuplicateScreen { screen });
            }
            for button in screen_assets.buttons() {
                if find(screen, button.action).is_none() {
                    return Err(DeckError::Layout {
                        screen,
                        problem: LayoutProblem::NoTransition {
                            hotspot: button.id.as_str().to_owned(),
                            action: button.action,
                        },
                    });
                }
            }
            if !screen_assets
                .buttons()
                .iter()
                .any(|button| button.action.is_escape())
            {
                return Err(DeckError::NoEscapeButton { screen });
            }
            for row in rows_on(screen) {
                if row.action.source() == ActionSource::User
                    && !screen_assets
                        .buttons()
                        .iter()
                        .any(|button| button.action == row.action)
                {
                    return Err(DeckError::HiddenAction {
                        screen,
                        action: row.action,
                    });
                }
            }
            assets.insert(screen, screen_assets);
        }
        Ok(Self { assets })
    }

    /// The assets of a screen, when the deck carries them.
    #[must_use]
    pub fn assets(&self, screen: Screen) -> Option<&ScreenAssets> {
        self.assets.get(&screen)
    }

    /// The carried screens, in [`Screen::ALL`] order.
    pub fn screens(&self) -> impl Iterator<Item = Screen> {
        Screen::ALL
            .into_iter()
            .filter(|screen| self.assets.contains_key(screen))
    }
}

/// Why a deck-driven interaction could not happen. A refusal changes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenSessionError {
    /// The current screen's assets are not in the deck: reported, never
    /// replaced with a placeholder screen.
    ScreenNotInDeck {
        /// The screen with no authored assets.
        screen: Screen,
    },
    /// The surface has a zero side, so nothing can be fitted to it.
    DegenerateSurface {
        /// The surface.
        surface: (u32, u32),
    },
    /// The pointer hit no button: outside the fitted image, or on no hotspot.
    NoButtonAt {
        /// The surface point.
        x: u32,
        /// The surface point.
        y: u32,
    },
    /// The state table refused the action; the machine is unchanged.
    Refused(Refusal),
}

impl fmt::Display for ScreenSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScreenNotInDeck { screen } => {
                write!(f, "{screen:?} carries no authored screen")
            }
            Self::DegenerateSurface { surface } => {
                write!(f, "a {}x{} surface fits nothing", surface.0, surface.1)
            }
            Self::NoButtonAt { x, y } => write!(f, "no button at ({x}, {y})"),
            Self::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ScreenSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Refused(why) => Some(why),
            _ => None,
        }
    }
}

/// One button as it appears on a surface: the same scale and offset the image
/// itself is fitted with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonView {
    /// The `ui-resource` id of the button.
    pub id: ContentId,
    /// The action pressing it requests.
    pub action: Action,
    /// The region in surface pixels.
    pub rect: Rect,
    /// Whether this button holds focus.
    pub focused: bool,
}

/// The authored presentation of the current screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenView {
    /// The screen.
    pub screen: Screen,
    /// The artwork to draw.
    pub art: ContentId,
    /// The logical size of the authored image.
    pub image: (u32, u32),
    /// The fit of that image into the surface.
    pub fit: AspectFit,
    /// The buttons in authored (focus) order, in surface pixels.
    pub buttons: Vec<ButtonView>,
}

/// The front end presented through a [`ScreenDeck`].
///
/// The deck supplies the art, the hotspots and the focus order; the machine
/// still applies every action through its own table, so a click, a key press
/// and the equivalent system result are the same transition with the same
/// requests. Focus lands on the authored first button when a screen is
/// entered; a screen the deck does not carry falls back to the table's own
/// focus order and says so when it is drawn ([`ScreenSessionError::ScreenNotInDeck`]).
#[derive(Clone, Debug)]
pub struct ScreenSession {
    front_end: FrontEnd,
    deck: ScreenDeck,
}

impl ScreenSession {
    /// A session on [`Screen::START`], with that screen's authored first
    /// button focused when the deck carries it.
    #[must_use]
    pub fn new(deck: ScreenDeck) -> Self {
        let mut front_end = FrontEnd::new();
        if let Some(action) = deck
            .assets(Screen::START)
            .and_then(ScreenAssets::first_action)
        {
            // The deck was validated against this same table, so the action
            // exists on the screen the machine starts on.
            let _ = front_end.set_focus(action);
        }
        Self { front_end, deck }
    }

    /// The machine behind the presentation.
    #[must_use]
    pub fn front_end(&self) -> &FrontEnd {
        &self.front_end
    }

    /// The authored screens this session presents.
    #[must_use]
    pub fn deck(&self) -> &ScreenDeck {
        &self.deck
    }

    /// The authored presentation of the current screen: the artwork and its
    /// buttons mapped with the image's own scale and offset.
    ///
    /// # Errors
    ///
    /// [`ScreenSessionError::ScreenNotInDeck`] when the deck carries no
    /// assets for the screen (never a placeholder), or
    /// [`ScreenSessionError::DegenerateSurface`] for a zero-sided surface.
    pub fn view(&self, surface: (u32, u32)) -> Result<ScreenView, ScreenSessionError> {
        let screen = self.front_end.screen();
        let assets = self
            .deck
            .assets(screen)
            .ok_or(ScreenSessionError::ScreenNotInDeck { screen })?;
        let fit = AspectFit::new(assets.image(), surface)
            .ok_or(ScreenSessionError::DegenerateSurface { surface })?;
        let focus = self.front_end.focus();
        let buttons = assets
            .buttons()
            .iter()
            .map(|button| ButtonView {
                id: button.id.clone(),
                action: button.action,
                rect: fit.map_rect(button.rect),
                focused: focus == Some(button.action),
            })
            .collect();
        Ok(ScreenView {
            screen,
            art: assets.art().clone(),
            image: assets.image(),
            fit,
            buttons,
        })
    }

    /// Presses the authored button under a surface point. A point outside the
    /// fitted image or on no hotspot hits nothing and changes nothing; a later
    /// button wins an overlap, exactly like [`ScreenLayout::hit_test`].
    ///
    /// # Errors
    ///
    /// As [`Self::view`], [`ScreenSessionError::NoButtonAt`] for a point on no
    /// button, or [`ScreenSessionError::Refused`] when the table refuses the
    /// action (the machine is unchanged).
    pub fn click(
        &mut self,
        surface: (u32, u32),
        x: u32,
        y: u32,
    ) -> Result<Outcome, ScreenSessionError> {
        let screen = self.front_end.screen();
        let assets = self
            .deck
            .assets(screen)
            .ok_or(ScreenSessionError::ScreenNotInDeck { screen })?;
        let fit = AspectFit::new(assets.image(), surface)
            .ok_or(ScreenSessionError::DegenerateSurface { surface })?;
        let action = assets
            .buttons()
            .iter()
            .rev()
            .find(|button| fit.map_rect(button.rect).contains(x, y))
            .map(|button| button.action)
            .ok_or(ScreenSessionError::NoButtonAt { x, y })?;
        self.press(action)
    }

    /// Applies an action by key: system results (a finished load, a mission
    /// ending) and buttons take the same path through the table.
    ///
    /// # Errors
    ///
    /// [`ScreenSessionError::Refused`]; the machine is unchanged.
    pub fn press(&mut self, action: Action) -> Result<Outcome, ScreenSessionError> {
        let outcome = self
            .front_end
            .apply(action)
            .map_err(ScreenSessionError::Refused)?;
        self.entered(&outcome)?;
        Ok(outcome)
    }

    /// Activates the focused button.
    ///
    /// # Errors
    ///
    /// As [`Self::press`], including [`Refusal::NothingFocused`].
    pub fn activate(&mut self) -> Result<Outcome, ScreenSessionError> {
        let outcome = self
            .front_end
            .activate_focus()
            .map_err(ScreenSessionError::Refused)?;
        self.entered(&outcome)?;
        Ok(outcome)
    }

    /// Answers an open discard prompt by dropping the draft and carrying on.
    ///
    /// # Errors
    ///
    /// [`ScreenSessionError::Refused`]; with no prompt open nothing changes.
    pub fn confirm_discard(&mut self) -> Result<Outcome, ScreenSessionError> {
        let outcome = self
            .front_end
            .confirm_discard()
            .map_err(ScreenSessionError::Refused)?;
        self.entered(&outcome)?;
        Ok(outcome)
    }

    /// Answers an open discard prompt by keeping the draft.
    ///
    /// # Errors
    ///
    /// [`ScreenSessionError::Refused`]; with no prompt open nothing changes.
    pub fn keep_editing(&mut self) -> Result<(), ScreenSessionError> {
        self.front_end
            .keep_editing()
            .map_err(ScreenSessionError::Refused)
    }

    /// Opens the construction draft: the screen's own domain-side input, kept
    /// out of the presentation so no caller can reach around the machine.
    ///
    /// # Errors
    ///
    /// [`Refusal::WrongScreen`] off the construction screen, or
    /// [`Refusal::ConfirmationPending`] while a discard prompt is open.
    pub fn open_construction(&mut self, draft: ConstructionDraft) -> Result<(), Refusal> {
        self.front_end.open_construction(draft)
    }

    /// Replaces the edited components of the open construction draft.
    ///
    /// # Errors
    ///
    /// As [`Self::open_construction`], and [`Refusal::NoDraft`] with none open.
    pub fn edit_construction(&mut self, components: Vec<ContentId>) -> Result<(), Refusal> {
        self.front_end.edit_construction(components)
    }

    /// Replaces the flight-check selection.
    ///
    /// # Errors
    ///
    /// [`Refusal::WrongScreen`] off the flight check, or
    /// [`Refusal::ConfirmationPending`] while a discard prompt is open.
    pub fn select_loadout(&mut self, loadout: Loadout) -> Result<(), Refusal> {
        self.front_end.select_loadout(loadout)
    }

    /// Declares whether the mission needs a wingmate: the mission data's say,
    /// passed straight through to the machine.
    pub fn set_wingmate_required(&mut self, required: bool) {
        self.front_end.set_wingmate_required(required);
    }

    /// Moves focus to the next (or previous) authored button, wrapping. A
    /// screen the deck does not carry falls back to the table's own order.
    ///
    /// # Errors
    ///
    /// [`ScreenSessionError::Refused`] when the focused action is not a
    /// button of the current screen.
    pub fn move_focus(&mut self, forward: bool) -> Result<(), ScreenSessionError> {
        let Some(assets) = self.deck.assets(self.front_end.screen()) else {
            self.front_end.move_focus(forward);
            return Ok(());
        };
        let order: Vec<Action> = assets
            .buttons()
            .iter()
            .map(|button| button.action)
            .collect();
        if order.is_empty() {
            // Unreachable: a decked screen always carries its escape button.
            self.front_end.move_focus(forward);
            return Ok(());
        }
        let at = self
            .front_end
            .focus()
            .and_then(|focus| order.iter().position(|action| *action == focus));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => order.len() - 1,
            (Some(at), true) => (at + 1) % order.len(),
            (Some(at), false) => (at + order.len() - 1) % order.len(),
        };
        let action = order[next];
        self.front_end
            .set_focus(action)
            .map_err(ScreenSessionError::Refused)
    }

    /// After an accepted action: entering a screen focuses its authored first
    /// button, and the machine's own reset stands in when the deck carries no
    /// assets for it. A prompt or an exit did not enter a screen, so focus is
    /// left alone.
    fn entered(&mut self, outcome: &Outcome) -> Result<(), ScreenSessionError> {
        if outcome.from == outcome.to {
            return Ok(());
        }
        if let Some(action) = self
            .deck
            .assets(outcome.to)
            .and_then(ScreenAssets::first_action)
        {
            self.front_end
                .set_focus(action)
                .map_err(ScreenSessionError::Refused)?;
        }
        Ok(())
    }
}
