//! The front-end shell: one `LAYOUT.CSV` section presented as it is written
//! (FE-MENU-1, task #1833).
//!
//! Spec context: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`
//! and `docs/findings/2026-10-15-f45-b-1-original-layout.md`. Shared contract:
//! `docs/contracts/UI-NETWORK.md` ("Authored images are aspect-fit with logical
//! hotspot coordinates transformed by the same scale/offset as the image").
//!
//! # What this is, and what it is not
//!
//! The designed front end ([`super::ScreenSession`]) presents screens a
//! validated [`super::ScreenDeck`] carries, and on this installation the deck
//! carries none: the state table's coverage rules refuse every original
//! section. The **shell** is the windowed presenter `cs --cs-path <retail>
//! --front-end` runs: it draws one `[@Name@]` section of the original
//! `ASSETS/LAYOUT.CSV` — every `P` row as a pane at its authored `X,Y` and
//! every `B` row as a button whose art is the F45-B.1-measured vertical
//! strip of [`super::BUTTON_STATE_FRAMES`] state frames — over the logical
//! 800x600 surface the front end is authored for, aspect-fit to the window.
//!
//! It is **generic over sections**: nothing here knows `MainMenu` from
//! `Campaign`. Pressing a button applies the destination the original data
//! itself states:
//!
//! * a non-empty `ScriptToExe` column navigates to that section when the
//!   layout holds one of the same name (`MM_B_CAMPAIGN` → `Campaign`);
//! * a button that starts no script looks its `case` arm up in the section's
//!   own screen script through [`super::ScreenHandlers`]: exactly
//!   `terminate` ends the run (`MM_B_QUIT`), and `activate(@name@)` followed
//!   only by `script_end "<this>.script"` navigates to `name` — the two
//!   statements `CAMPAIGN.SCRIPT` states for `CM_B_CANCEL`;
//! * everything else is [`ButtonEffect::Unhandled`]: the press is drawn and
//!   **recorded** with its verbatim handler, never guessed into a transition.
//!
//! # What stays unknown (recorded per rule 4, never hidden)
//!
//! * The original focus/tab order — no `LAYOUT.CSV` column carries it; focus
//!   is the declaration order, as F45-B.1 recorded.
//! * Which member is the original's front-end cursor — the Esc screen's own
//!   script names `daglove`/`dafinger` (in `ZBD/rimage.zbd`, unreachable from
//!   this member list) and the `ASSETS/GRAPHICS` pointer PNGs' consumer was
//!   never examined
//!   (`docs/findings/2026-10-21-pause-map-screen-data.md`, "not decided").
//!   `CURSOR_CANDIDATES` tries the named pointer art in order; the first
//!   that decodes is drawn **and recorded** — never asserted as the
//!   original's choice.
//! * When the original draws a button **disabled** — the enabled state is a
//!   runtime script effect (`mail(10000, …)`/`mail(10018, …)` in
//!   `CAMPAIGN.SCRIPT`), unread statically; every presented button is enabled,
//!   and the disabled frame is reachable only through
//!   [`MenuShell::set_enabled`], a designed affordance.
//! * `M` movie rows — parsed into [`ShellSection::movies`] for the presenter
//!   to play (FE-MENU-2); the *movie's own* audio, the `Z` draw order and
//!   whether a non-looping movie's last frame stays up are still unread.
//! * `T`/`E`/`L`/`D`/`S`/`W`/`Z` controls — counted on the section
//!   ([`ShellSection::other_rows`]), never drawn or faked.
//! * A `B` row's `ResID` label — kept on [`ShellButton::res_id`]; which
//!   string table answers an `IDS_*` name and how the text is drawn over
//!   the art are unread, and no label text is drawn (the `MainMenu`
//!   buttons write `0`: their labels are the strips' baked text).
//! * A `P` row's `NUMFRAMES` other than 1 — the shell draws frame 0 of the
//!   strip and keeps the count on [`ShellPane::frames`].
//! * `AlphaType=2` (colour-key) — which texel the original keys out is
//!   unread; the pane is drawn with its stored alpha and [`ShellPane::color_keyed`]
//!   records the claim that needs checking.
//! * `Z`-order other than declaration order — the `Z` column's draw order is
//!   unread; panes draw in declaration order.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::ui_layout::{AspectFit, Rect};

use super::original::{ArtExtents, OriginalLayout, Row, ScreenScripts};
use super::pane_transform::PaneTransform;
use super::{BUTTON_STATE_FRAMES, ScreenHandlers};

/// The logical surface every `LAYOUT.CSV` section is authored for: the
/// front-end's own 800x600 (F45-B.1, F45-D measured).
pub const SECTION_SURFACE: (u32, u32) = (800, 600);

/// The section the shell opens on: the original's entry screen.
pub const START_SECTION: &str = "MainMenu";

/// The graphics members tried as the pointer's cursor art, in order. Which
/// member the original front end maps onto the pointer is unread (see the
/// module docs); the first that decodes is drawn, and [`MenuShell::cursor`]/
/// the smoke report name which it was — or that none was found.
pub const CURSOR_CANDIDATES: &[&str] = &["PASSIVEPOINTERZ.PNG", "ACTIVEPOINTERZ.PNG"];

/// A signed rectangle: `LAYOUT.CSV` coordinates can be negative (a pane hung
/// off the surface's edge, `SB_P_GRIME` at `-1,-1`), which the unsigned
/// [`Rect`] cannot hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellRect {
    /// Left edge in logical pixels (may be negative).
    pub x: i32,
    /// Top edge in logical pixels (may be negative).
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl ShellRect {
    /// Whether the logical point lies inside (right/bottom exclusive).
    #[must_use]
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && i64::from(x) < i64::from(self.x) + i64::from(self.width)
            && i64::from(y) < i64::from(self.y) + i64::from(self.height)
    }

    /// The part of this rect inside a `surface` (an unsigned rect at the
    /// origin), and how many texels of the source the clipping cut off the
    /// left and top. `None` when nothing is visible.
    #[must_use]
    pub fn clip(&self, surface: (u32, u32)) -> Option<(Rect, (u32, u32))> {
        let left = self.x.max(0);
        let top = self.y.max(0);
        let right = (self.x + i32::try_from(self.width).unwrap_or(i32::MAX))
            .min(i32::try_from(surface.0).unwrap_or(i32::MAX));
        let bottom = (self.y + i32::try_from(self.height).unwrap_or(i32::MAX))
            .min(i32::try_from(surface.1).unwrap_or(i32::MAX));
        if right <= left || bottom <= top {
            return None;
        }
        let cut = (
            u32::try_from(left - self.x).unwrap_or(0),
            u32::try_from(top - self.y).unwrap_or(0),
        );
        Some((
            Rect {
                x: u32::try_from(left).unwrap_or(0),
                y: u32::try_from(top).unwrap_or(0),
                width: u32::try_from(right - left).unwrap_or(0),
                height: u32::try_from(bottom - top).unwrap_or(0),
            },
            cut,
        ))
    }
}

/// Which of the strip's frames a button is drawn with — the four colour sets
/// `[GLOBALVARS]` names (`DISABLED`, `ACTIVE`, `ROLLOVER`, `DEPRESSED`), in
/// the art's own top-to-bottom order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonVisual {
    /// Frame 0: the button cannot be pressed.
    Disabled,
    /// Frame 1: the button's resting state.
    Active,
    /// Frame 2: the pointer or focus is on the button.
    Rollover,
    /// Frame 3: the button is held down.
    Depressed,
}

impl ButtonVisual {
    /// The frame index in the art's strip.
    #[must_use]
    pub const fn frame(self) -> u32 {
        match self {
            Self::Disabled => 0,
            Self::Active => 1,
            Self::Rollover => 2,
            Self::Depressed => 3,
        }
    }
}

/// What pressing a button does, read from the layout row and the section's
/// own screen script — never assigned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ButtonEffect {
    /// Navigate to the named section (the `ScriptToExe` column, or an
    /// `activate(@name@)` + `script_end` handler). The name is the section's
    /// as written.
    Navigate(String),
    /// Hand back to `name` and end this section: the `activate(@name@)` +
    /// `script_end "<this>.script"` pair. The target is the still-open
    /// section that started this one, so the shell unwinds to it instead of
    /// stacking another copy; when it is not open it replaces this section.
    Return(String),
    /// End the run: the handler is exactly `terminate`.
    Terminate,
    /// The press has no effect the shell knows; recorded, never guessed.
    Unhandled(UnhandledEffect),
}

/// Why a button's press is unhandled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnhandledEffect {
    /// `ScriptToExe` names a script the layout has no section for. The script
    /// itself may exist (`MULTIPLAYERMAIN.SCRIPT` does); its layout does not.
    ScriptNotPresent {
        /// The name as the row writes it.
        script: String,
    },
    /// No `ScriptToExe` and no `case` arm in the section's script (or the
    /// script could not be read).
    NoHandler,
    /// A handler the shell does not run — native callbacks, `mail`, screen
    /// state. The statements verbatim, in order.
    HandlerNotShell {
        /// The arm's statements.
        statements: Vec<String>,
    },
}

/// One `P` row to draw: the row's art's first frame at its `X,Y`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellPane {
    /// The row id as written, upper-cased.
    pub id: String,
    /// The art name as the row writes it.
    pub art: String,
    /// One frame's extent at the row's position.
    pub rect: ShellRect,
    /// The row's `NUMFRAMES` column (1 for a still pane; the shell draws
    /// frame 0 and records any other count).
    pub frames: u32,
    /// The strip frame drawn: 0 unless a screen's script picks another (the
    /// cabin's map pins, [`super::cabin`]).
    pub frame: u32,
    /// The row's `IsRegion?` column — kept for the record; the shell does
    /// not hit-test panes.
    pub is_region: bool,
    /// `AlphaType` is `2` (colour-key): the keyed texel is unread, so the
    /// pane draws with its stored alpha and this flags the record.
    pub color_keyed: bool,
    /// The `scale`/`rotate`/`origin` the screen's script gave the pane, drawn
    /// as [`PaneTransform`] places it ([`super::cabin`]'s memento picture):
    /// `rect.x`/`rect.y` is then the `render` position and `rect`'s extent
    /// the picture's own. `None` draws the pane as the layout wrote it.
    pub transform: Option<PaneTransform>,
}

/// One `M` row to play: the screen's movie object, drawn under the panes.
/// The drawn rect needs the decoded video's extent, which the parser does
/// not have: [`ShellMovie::rect_at`] applies the row's `ScaleX`/`ScaleY`
/// once the decoder reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellMovie {
    /// The row id as written, upper-cased.
    pub id: String,
    /// The movie file name as the row writes it (`CrimFlag.MPG`).
    pub file: String,
    /// The row's `X`,`Y` in logical pixels (signed: may be negative).
    pub x: i32,
    /// Top edge in logical pixels.
    pub y: i32,
    /// The row's `Z` — kept for the record; movies draw under the panes in
    /// declaration order (the draw order the column carries is unread).
    pub z: u32,
    /// The row's `ScaleX`/`ScaleY`, percent of the video's own extent: `250`
    /// draws the measured 320x240 `crimflag.mpg` at the authored 800x600 the
    /// front end presents (the only reading that lands the flag the owner's
    /// frame shows; inferred, not `verified_original`).
    pub scale_x: u32,
    /// See `scale_x`.
    pub scale_y: u32,
    /// The row's `# Loops`: `0` loops continuously (the convention the file's
    /// own `W` sound rows document and the owner's frame shows), anything
    /// else is the play count (`CF_MOVIE` plays `Final.MPG` once).
    pub loops: u32,
    /// The row's `Region?` — kept for the record; movies are not hit-tested.
    pub region: bool,
}

impl ShellMovie {
    /// The drawn rect once the video's decoded extent is known: the row's
    /// `X`,`Y` plus the video extent at `ScaleX`/`ScaleY` percent.
    #[must_use]
    pub fn rect_at(&self, video: (u32, u32)) -> ShellRect {
        ShellRect {
            x: self.x,
            y: self.y,
            width: video.0.saturating_mul(self.scale_x).saturating_div(100),
            height: video.1.saturating_mul(self.scale_y).saturating_div(100),
        }
    }
}

/// One `B` row to draw: a button whose art is a vertical strip of
/// [`BUTTON_STATE_FRAMES`] equal state frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellButton {
    /// The row id as written, upper-cased.
    pub id: String,
    /// The art name as the row writes it.
    pub art: String,
    /// One frame's extent at the row's position.
    pub rect: ShellRect,
    /// The row's `ResID` column as written: `None` when it is empty or `0`
    /// — no string-table label, so the text is whatever the art strip
    /// bakes in (all six `MainMenu` buttons) — `Some` of the token
    /// otherwise (`IDS_*` names like `IA_B_FLY`'s `IDS_IA_B_FLYMISSION`,
    /// or the `!` the message-box rows write). How the original draws a
    /// `ResID` label — which string table answers it, which font and
    /// where over the art — is unread, and no label text is drawn here.
    pub res_id: Option<String>,
    /// The effect pressing it takes.
    pub effect: ButtonEffect,
    /// `Some(checked)` for a check-box button (the row's `Type` column is
    /// `1`): its art is [`CHECK_STATE_FRAMES`] frames — the four state frames
    /// unchecked, then the same four checked — and a press flips the state
    /// instead of running [`Self::effect`].
    pub check: Option<bool>,
    /// Whether it can be pressed. Always `true` when built from the layout —
    /// the original's enable state is a runtime script effect, unread; see
    /// the module docs and [`MenuShell::set_enabled`].
    pub enabled: bool,
}

/// Why a `P`/`B`/`M` row is not drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkippedReason {
    /// The row names no art.
    EmptyArt,
    /// A field the row needs holds a placeholder no variable defines.
    UnresolvedPlaceholder {
        /// The placeholder name.
        name: String,
    },
    /// `X` or `Y` is not a (signed) number.
    PositionUnread,
    /// The art's extent could not be read.
    ArtUnread {
        /// The art name.
        art: String,
    },
    /// A button's art is not a [`BUTTON_STATE_FRAMES`]-frame strip, or a
    /// pane's `NUMFRAMES` is zero — never guessed.
    FrameCountUnknown {
        /// The measured height (buttons) or the declared count (panes).
        height: u32,
    },
    /// An `M` row's field is empty or not the unsigned number the header's
    /// record layout declares (`ScaleX`, `ScaleY`, `# Loops`; `X`,`Y`,`Z`
    /// reuse [`SkippedReason::PositionUnread`]).
    MovieFieldUnread {
        /// The field's name in the header's record layout.
        field: &'static str,
    },
}

/// A row the shell does not draw, with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedRow {
    /// The row id as written.
    pub id: String,
    /// `P`, `B` or `M`.
    pub kind: String,
    /// Why it is not drawn.
    pub reason: SkippedReason,
}

/// One `LAYOUT.CSV` section as the shell presents it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellSection {
    /// The section name as written.
    pub name: String,
    /// The `M` rows in declaration order, drawn under the panes.
    pub movies: Vec<ShellMovie>,
    /// The `P` rows in declaration (draw) order.
    pub panes: Vec<ShellPane>,
    /// The `B` rows in declaration (focus) order.
    pub buttons: Vec<ShellButton>,
    /// The `P`/`B`/`M` rows not drawn, in declaration order with reasons.
    pub skipped: Vec<SkippedRow>,
    /// The row kinds present besides `P`/`B`/`M` (`T`, `E`, `A`, `D`, `L`,
    /// `S`, `W`, `Z`, …), counted and never drawn.
    pub other_rows: Vec<String>,
    /// Whether entering the section focuses its first enabled button (the
    /// default). A screen whose script focuses the control host rather than a
    /// button (`focus(@ctl@)`, the cabin) starts with no button focused.
    pub focus_on_entry: bool,
}

/// The outcome one applied input produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellOutcome {
    /// Nothing changed (a press on dead space, an up with nothing held).
    Nothing,
    /// A button's press had no effect the shell knows; recorded on the shell's
    /// [`MenuShell::unhandled`] log.
    Unhandled {
        /// The button's row id.
        id: String,
        /// Why the press is unhandled.
        effect: UnhandledEffect,
    },
    /// A check-box button flipped.
    Toggled {
        /// The button's row id.
        id: String,
        /// The state after the press.
        checked: bool,
    },
    /// Moved to another section.
    Entered {
        /// The section now on top.
        section: String,
    },
    /// A button handed control back to a section that was already open (or
    /// replaced this one): the shell unwound to `section` rather than
    /// opening a new copy of it.
    Returned {
        /// The section now on top.
        section: String,
        /// The whole stack before the move, so a refused move can be undone
        /// with [`MenuShell::reopen`].
        previous: Vec<String>,
    },
    /// Esc stepped back to the previous section.
    Back {
        /// The section now on top.
        section: String,
    },
    /// The run should end: a `terminate` button or Esc on the root section.
    Quit,
}

/// Why the shell itself cannot do something.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellError {
    /// The layout holds no section of this name.
    NoSuchSection {
        /// The asked-for name.
        name: String,
    },
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchSection { name } => write!(f, "the layout has no {name} section"),
        }
    }
}

impl std::error::Error for ShellError {}

/// What holds a button down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PressKind {
    /// The pointer's left button.
    Pointer,
    /// The activate key (Enter).
    Keyboard,
}

/// A drawn button in surface pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellButtonView {
    /// The button's index in the section's declaration order.
    pub index: usize,
    /// The row id.
    pub id: String,
    /// The art name as the row writes it.
    pub art: String,
    /// Its visible region in logical pixels (clipped to the surface).
    pub logical: Rect,
    /// Its region in surface pixels.
    pub rect: Rect,
    /// Where inside the art the visible region starts, in texels: the state
    /// frame's offset plus whatever an off-surface edge clipped.
    pub texel_offset: (u32, u32),
    /// The strip frame it draws.
    pub visual: ButtonVisual,
    /// Whether it holds keyboard focus.
    pub focused: bool,
}

/// A drawn pane in surface pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellPaneView {
    /// The pane's index in the section's declaration order.
    pub index: usize,
    /// The row id.
    pub id: String,
    /// The art name as the row writes it.
    pub art: String,
    /// Its visible region in logical pixels (clipped to the surface).
    pub logical: Rect,
    /// Its visible region in surface pixels.
    pub rect: Rect,
    /// Where inside the art's frame-0 the visible region starts, in texels:
    /// whatever an off-surface edge clipped.
    pub texel_offset: (u32, u32),
    /// The pane's [`ShellPane::transform`]; `logical` then stays the
    /// picture's own box at the render position.
    pub transform: Option<PaneTransform>,
}

/// The presented frame: the current section fitted to a surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellView {
    /// The section name.
    pub section: String,
    /// The aspect-fit the mapping used.
    pub fit: AspectFit,
    /// The visible panes in declaration order.
    pub panes: Vec<ShellPaneView>,
    /// The visible buttons in declaration order.
    pub buttons: Vec<ShellButtonView>,
    /// The pointer's logical position, when it is over the surface.
    pub pointer: Option<(i32, i32)>,
}

/// The shell: every section of a parsed `LAYOUT.CSV` presented, a navigation
/// stack and the focus/pointer state the presenter animates.
///
/// [`Self::build`] measures every section up front — panes and buttons and
/// the rows it cannot draw — so the whole layout's record exists, not just
/// the screens a session happens to reach.
#[derive(Clone, Debug)]
pub struct MenuShell {
    sections: BTreeMap<String, ShellSection>,
    /// The navigation stack of section names as written; the last is current.
    stack: Vec<String>,
    /// Focus index into the current section's buttons (the first enabled one
    /// on entry, matching the designed screens' entry rule).
    focus: Option<usize>,
    /// The hovered button index.
    hover: Option<usize>,
    /// The held button and what holds it.
    pressed: Option<(usize, PressKind)>,
    /// The last logical pointer position.
    pointer: Option<(i32, i32)>,
    /// The cursor art that was found, `(member name, extent)`.
    cursor: Option<(String, (u32, u32))>,
    /// Presses that had no known effect, in order: `(section, id, effect)`.
    unhandled: Vec<(String, String, UnhandledEffect)>,
    /// The row id of the button the last press applied.
    applied: Option<String>,
}

impl MenuShell {
    /// Presents every section of `layout`, measuring art through `extents`
    /// and reading screen scripts through `scripts`.
    ///
    /// The shell starts on no screen: [`Self::open`] names the section the
    /// window first shows.
    #[must_use]
    pub fn build(
        layout: &OriginalLayout,
        extents: &dyn ArtExtents,
        scripts: &dyn ScreenScripts,
    ) -> Self {
        let mut measured: BTreeMap<String, Option<(u32, u32)>> = BTreeMap::new();
        let mut measure = |art: &str| -> Option<(u32, u32)> {
            let key = art.to_ascii_lowercase();
            *measured.entry(key).or_insert_with(|| extents.extent(art))
        };
        let mut sections: BTreeMap<String, ShellSection> = layout
            .sections()
            .iter()
            .map(|section| {
                let handlers = scripts
                    .script(&section.name)
                    .map(|text| ScreenHandlers::parse(&text));
                (
                    section.name.to_ascii_uppercase(),
                    present_section(layout, section, &mut measure, handlers.as_ref()),
                )
            })
            .collect();
        // `PLANENAME.SCRIPT` centres its plate and sets every other control
        // relative to it (#1958).
        if let Some(dialog) =
            sections.get_mut(&super::plane_name::NAME_SECTION.to_ascii_uppercase())
        {
            let _ = super::plane_name::centre_dialog(dialog);
        }
        let cursor = CURSOR_CANDIDATES
            .iter()
            .find_map(|name| measure(name).map(|extent| ((*name).to_owned(), extent)));
        Self {
            sections,
            stack: Vec::new(),
            focus: None,
            hover: None,
            pressed: None,
            pointer: None,
            cursor,
            unhandled: Vec::new(),
            applied: None,
        }
    }

    /// Opens `section` as the shell's root: the navigation stack restarts
    /// there, focus lands on its first enabled button and the pointer state
    /// clears.
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchSection`] for a name the layout does not hold.
    pub fn open(&mut self, section: &str) -> Result<(), ShellError> {
        let Some(found) = self
            .sections
            .values()
            .find(|present| present.name.eq_ignore_ascii_case(section))
        else {
            return Err(ShellError::NoSuchSection {
                name: section.to_owned(),
            });
        };
        self.stack = vec![found.name.clone()];
        self.entered();
        Ok(())
    }

    /// Draws `from`'s `M` rows and the `P` rows named in `pane_ids` under
    /// `to`'s own rows: the layers a screen that only *hides* its predecessor
    /// leaves showing (`MAINMENU.SCRIPT` runs `deactivate(this)` on Campaign,
    /// and the owner's Drydock capture of the pilot page shows the flag movie
    /// and the logo behind it, not the button frame or the buttons).
    ///
    /// The carried panes come first in `to`'s draw order (they are the ground
    /// its own panes stand on); a row `to` already holds is not doubled. A
    /// name absent from either section changes nothing and returns `false`.
    pub fn carry_under(&mut self, to: &str, from: &str, pane_ids: &[&str]) -> bool {
        let key = |name: &str| name.to_ascii_uppercase();
        let Some(source) = self.sections.get(&key(from)) else {
            return false;
        };
        let movies = source.movies.clone();
        let panes: Vec<ShellPane> = pane_ids
            .iter()
            .filter_map(|id| {
                source
                    .panes
                    .iter()
                    .find(|pane| pane.id.eq_ignore_ascii_case(id))
            })
            .cloned()
            .collect();
        let Some(target) = self.sections.get_mut(&key(to)) else {
            return false;
        };
        for movie in movies {
            if !target.movies.iter().any(|held| held.id == movie.id) {
                target.movies.push(movie);
            }
        }
        let fresh: Vec<ShellPane> = panes
            .into_iter()
            .filter(|pane| !target.panes.iter().any(|held| held.id == pane.id))
            .collect();
        target.panes.splice(0..0, fresh);
        true
    }

    /// The named section for a screen's own set-up (the cabin's hub,
    /// [`super::cabin::CabinHub`]), matched ASCII case-insensitively.
    pub fn section_mut(&mut self, name: &str) -> Option<&mut ShellSection> {
        self.sections.get_mut(&name.to_ascii_uppercase())
    }

    /// The current section.
    #[must_use]
    pub fn section(&self) -> Option<&ShellSection> {
        self.stack
            .last()
            .and_then(|name| self.sections.get(&name.to_ascii_uppercase()))
    }

    /// The current section's name.
    #[must_use]
    pub fn section_name(&self) -> Option<&str> {
        self.stack.last().map(String::as_str)
    }

    /// Every section the layout holds, keyed by its upper-cased name.
    #[must_use]
    pub fn sections(&self) -> &BTreeMap<String, ShellSection> {
        &self.sections
    }

    /// The navigation stack depth (1 is the root).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The focused button index.
    #[must_use]
    pub fn focus(&self) -> Option<usize> {
        self.focus
    }

    /// The hovered button index.
    #[must_use]
    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    /// The cursor art found, `(member name, extent)`, or `None` — the
    /// recorded unknown the window falls back from.
    #[must_use]
    pub fn cursor(&self) -> Option<&(String, (u32, u32))> {
        self.cursor.as_ref()
    }

    /// Presses that had no known effect, `(section, button id, effect)`.
    #[must_use]
    pub fn unhandled(&self) -> &[(String, String, UnhandledEffect)] {
        &self.unhandled
    }

    /// Marks a button of the current section enabled or disabled: a designed
    /// affordance — the original's enable state is a runtime script effect,
    /// unread (see the module docs). `false` draws the strip's disabled
    /// frame and removes the button from focus and pointer hits.
    #[must_use]
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        let Some(name) = self.stack.last().cloned() else {
            return false;
        };
        let Some(section) = self.sections.get_mut(&name.to_ascii_uppercase()) else {
            return false;
        };
        let Some(index) = section
            .buttons
            .iter()
            .position(|button| button.id.eq_ignore_ascii_case(id))
        else {
            return false;
        };
        section.buttons[index].enabled = enabled;
        if !enabled {
            if self.focus == Some(index) {
                self.focus = section.buttons.iter().position(|button| button.enabled);
            }
            if self.hover == Some(index) {
                self.hover = None;
            }
            if self.pressed.is_some_and(|(held, _)| held == index) {
                self.pressed = None;
            }
        }
        true
    }

    /// Sets a check-box button's state (a page staging or discarding a
    /// change). `false` when no such check box is on the current section.
    pub fn set_checked(&mut self, id: &str, checked: bool) -> bool {
        let Some(name) = self.stack.last().cloned() else {
            return false;
        };
        self.sections
            .get_mut(&name.to_ascii_uppercase())
            .and_then(|section| {
                section
                    .buttons
                    .iter_mut()
                    .find(|button| button.id.eq_ignore_ascii_case(id) && button.check.is_some())
            })
            .map(|button| button.check = Some(checked))
            .is_some()
    }

    /// A check-box button's state on the current section.
    #[must_use]
    pub fn is_checked(&self, id: &str) -> Option<bool> {
        self.section()?
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(id))
            .and_then(|button| button.check)
    }

    /// The id of the button the pointer or key is holding down, if any.
    #[must_use]
    pub fn pressed_id(&self) -> Option<&str> {
        let (index, _) = self.pressed?;
        self.section()?
            .buttons
            .get(index)
            .map(|button| button.id.as_str())
    }

    /// Focuses the button `id` of the current section (`None` clears focus,
    /// for a page whose own control holds it). `false` when there is no
    /// enabled button of that name.
    pub fn set_focus_id(&mut self, id: Option<&str>) -> bool {
        let Some(id) = id else {
            self.focus = None;
            return true;
        };
        let found = self.section().and_then(|section| {
            section
                .buttons
                .iter()
                .position(|button| button.enabled && button.id.eq_ignore_ascii_case(id))
        });
        if found.is_some() {
            self.focus = found;
        }
        found.is_some()
    }

    /// Moves focus to the next (or previous) enabled button, wrapping in
    /// declaration order — the order focus visits, the original's tab order
    /// being unread.
    pub fn move_focus(&mut self, forward: bool) {
        let Some(section) = self.section() else {
            return;
        };
        let enabled: Vec<usize> = section
            .buttons
            .iter()
            .enumerate()
            .filter(|(_, button)| button.enabled)
            .map(|(index, _)| index)
            .collect();
        if enabled.is_empty() {
            self.focus = None;
            return;
        }
        let at = self
            .focus
            .and_then(|focus| enabled.iter().position(|index| *index == focus));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => enabled.len() - 1,
            (Some(at), true) => (at + 1) % enabled.len(),
            (Some(at), false) => (at + enabled.len() - 1) % enabled.len(),
        };
        self.focus = Some(enabled[next]);
    }

    /// Moves the pointer to a logical point and updates the hover.
    ///
    /// The hover is always the button under the pointer, but only actual
    /// motion selects it: the window re-applies the cached cursor position
    /// every frame, and a stationary pointer must not take the selection
    /// back from the keyboard.
    pub fn pointer_move(&mut self, x: i32, y: i32) {
        let moved = self.pointer != Some((x, y));
        self.pointer = Some((x, y));
        self.hover = self.button_at(x, y);
        if moved && self.hover.is_some() {
            self.focus = self.hover;
        }
    }

    /// The pointer left the surface.
    pub fn pointer_leave(&mut self) {
        self.pointer = None;
        self.hover = None;
    }

    /// The pointer's button went down: the hovered button becomes held.
    pub fn pointer_down(&mut self) {
        if let Some(index) = self.hover
            && self
                .section()
                .is_some_and(|section| section.buttons[index].enabled)
        {
            self.focus = Some(index);
            self.pressed = Some((index, PressKind::Pointer));
        }
    }

    /// The pointer's button came up: a held button still hovered applies.
    pub fn pointer_up(&mut self) -> ShellOutcome {
        match self.pressed.take() {
            Some((index, PressKind::Pointer)) if self.hover == Some(index) => self.apply(index),
            _ => ShellOutcome::Nothing,
        }
    }

    /// The activate key went down on the focused button.
    pub fn activate_down(&mut self) {
        if let Some(index) = self.focus
            && self
                .section()
                .is_some_and(|section| section.buttons[index].enabled)
        {
            self.pressed = Some((index, PressKind::Keyboard));
        }
    }

    /// The activate key came up: a button the key still holds applies.
    pub fn activate_up(&mut self) -> ShellOutcome {
        match self.pressed.take() {
            Some((index, PressKind::Keyboard)) => self.apply(index),
            _ => ShellOutcome::Nothing,
        }
    }

    /// Esc: drops a held button, steps one section back, or ends the run on
    /// the root section.
    pub fn escape(&mut self) -> ShellOutcome {
        if self.pressed.take().is_some() {
            return ShellOutcome::Nothing;
        }
        if self.stack.len() > 1 {
            self.stack.pop();
            self.entered();
            return ShellOutcome::Back {
                section: self.stack.last().expect("a root stays").clone(),
            };
        }
        ShellOutcome::Quit
    }

    /// The frame to draw: the current section's panes and buttons fitted to
    /// `surface`, with every button's strip state resolved.
    ///
    /// The fit is the contract's own [`AspectFit`]: the logical 800x600
    /// surface scaled and centred, hotspots mapped with the same transform.
    #[must_use]
    pub fn view(&self, surface: (u32, u32)) -> Option<ShellView> {
        let section = self.section()?;
        let fit = AspectFit::new(SECTION_SURFACE, surface)?;
        let mut panes = Vec::new();
        for (index, pane) in section.panes.iter().enumerate() {
            // A transformed pane's rect is the picture at the render
            // position, not where it lands: the presenter places it, so the
            // surface does not clip it here.
            let clipped = match (
                pane.transform,
                u32::try_from(pane.rect.x),
                u32::try_from(pane.rect.y),
            ) {
                (Some(_), Ok(x), Ok(y)) => Some((
                    Rect {
                        x,
                        y,
                        width: pane.rect.width,
                        height: pane.rect.height,
                    },
                    (0, 0),
                )),
                (Some(_), _, _) => None,
                (None, _, _) => pane.rect.clip(SECTION_SURFACE),
            };
            let Some((visible, texel_offset)) = clipped else {
                continue;
            };
            panes.push(ShellPaneView {
                index,
                id: pane.id.clone(),
                art: pane.art.clone(),
                logical: visible,
                rect: fit.map_rect(visible),
                texel_offset: (
                    texel_offset.0,
                    texel_offset.1 + pane.frame * pane.rect.height,
                ),
                transform: pane.transform,
            });
        }
        let buttons = section
            .buttons
            .iter()
            .enumerate()
            .filter_map(|(index, button)| {
                let (visible, cut) = button.rect.clip(SECTION_SURFACE)?;
                let visual = self.visual(index);
                Some(ShellButtonView {
                    index,
                    id: button.id.clone(),
                    art: button.art.clone(),
                    logical: visible,
                    rect: fit.map_rect(visible),
                    texel_offset: (
                        cut.0,
                        cut.1
                            + (visual.frame()
                                + if button.check == Some(true) {
                                    BUTTON_STATE_FRAMES
                                } else {
                                    0
                                })
                                * button.rect.height,
                    ),
                    visual,
                    focused: self.focus == Some(index),
                })
            })
            .collect();
        Some(ShellView {
            section: section.name.clone(),
            fit,
            panes,
            buttons,
            pointer: self.pointer,
        })
    }

    /// Maps a surface point back to logical pixels (the inverse of
    /// [`AspectFit::map_rect`]'s integer transform), for pointer input.
    /// `None` outside the fitted image or on a degenerate surface.
    #[must_use]
    pub fn unmap_point(surface: (u32, u32), x: u32, y: u32) -> Option<(i32, i32)> {
        let fit = AspectFit::new(SECTION_SURFACE, surface)?;
        let image = fit.image_rect();
        if !image.contains(x, y) {
            return None;
        }
        let logical = |surface_at: u32, base: u32, fitted: u32, span: u32| {
            let value = u64::from(surface_at - base) * u64::from(span) / u64::from(fitted.max(1));
            i32::try_from(value).unwrap_or(i32::MAX)
        };
        Some((
            logical(x, image.x, image.width, SECTION_SURFACE.0),
            logical(y, image.y, image.height, SECTION_SURFACE.1),
        ))
    }

    /// The strip state a button draws right now.
    #[must_use]
    pub fn visual(&self, index: usize) -> ButtonVisual {
        let enabled = self
            .section()
            .is_some_and(|section| section.buttons[index].enabled);
        if !enabled {
            return ButtonVisual::Disabled;
        }
        if self.pressed.is_some_and(|(held, _)| held == index) {
            return ButtonVisual::Depressed;
        }
        if self.focus == Some(index) {
            return ButtonVisual::Rollover;
        }
        ButtonVisual::Active
    }

    /// The button under a logical point: the later declaration wins an
    /// overlap, matching [`cs_content::ui_layout::ScreenLayout::hit_test`].
    #[must_use]
    fn button_at(&self, x: i32, y: i32) -> Option<usize> {
        self.section().and_then(|section| {
            section
                .buttons
                .iter()
                .enumerate()
                .rev()
                .find(|(_, button)| button.enabled && button.rect.contains(x, y))
                .map(|(index, _)| index)
        })
    }

    /// The row id of the button whose press the last outcome came from: what
    /// tells Accept from Cancel when both start the same screen.
    #[must_use]
    pub fn last_applied(&self) -> Option<&str> {
        self.applied.as_deref()
    }

    /// Applies one button's effect.
    fn apply(&mut self, index: usize) -> ShellOutcome {
        let Some(section) = self.section().cloned() else {
            return ShellOutcome::Nothing;
        };
        let Some(button) = section.buttons.get(index) else {
            return ShellOutcome::Nothing;
        };
        self.applied = Some(button.id.clone());
        if let Some(checked) = button.check {
            let id = button.id.clone();
            let checked = !checked;
            self.set_checked(&id, checked);
            return ShellOutcome::Toggled { id, checked };
        }
        match &button.effect {
            ButtonEffect::Navigate(name) => {
                let name = self.canonical(name);
                // A script start that names a section already open below
                // this one reactivates that parent: close what is above it.
                if let Some(parent) = self.stack[..self.stack.len() - 1]
                    .iter()
                    .rposition(|open| open.eq_ignore_ascii_case(&name))
                {
                    return self.unwind_to(parent + 1);
                }
                self.stack.push(name.clone());
                self.entered();
                ShellOutcome::Entered { section: name }
            }
            ButtonEffect::Return(name) => {
                let name = self.canonical(name);
                let open = self.stack[..self.stack.len() - 1]
                    .iter()
                    .rposition(|open| open.eq_ignore_ascii_case(&name));
                match open {
                    Some(parent) => self.unwind_to(parent + 1),
                    None => {
                        // Not open: this section ends and the target takes
                        // its place.
                        let previous = self.stack.clone();
                        self.stack.pop();
                        self.stack.push(name.clone());
                        self.entered();
                        ShellOutcome::Returned {
                            section: name,
                            previous,
                        }
                    }
                }
            }
            ButtonEffect::Terminate => ShellOutcome::Quit,
            ButtonEffect::Unhandled(effect) => {
                let effect = effect.clone();
                self.unhandled
                    .push((section.name.clone(), button.id.clone(), effect.clone()));
                ShellOutcome::Unhandled {
                    id: button.id.clone(),
                    effect,
                }
            }
        }
    }

    /// The section's own spelling of `name`, or `name` as written.
    fn canonical(&self, name: &str) -> String {
        self.sections
            .get(&name.to_ascii_uppercase())
            .map_or_else(|| name.to_owned(), |section| section.name.clone())
    }

    /// Closes every section above the first `keep`.
    fn unwind_to(&mut self, keep: usize) -> ShellOutcome {
        let previous = self.stack.clone();
        self.stack.truncate(keep);
        self.entered();
        ShellOutcome::Returned {
            section: self.stack.last().expect("a root stays").clone(),
            previous,
        }
    }

    /// Undoes a [`ShellOutcome::Returned`] the caller refused: the stack goes
    /// back to the `previous` one the outcome carried.
    pub fn reopen(&mut self, previous: &[String]) {
        self.stack = previous.to_vec();
        self.entered();
    }

    /// Resets the per-screen input state on entry: focus to the first
    /// enabled button, held released, hover recomputed under the kept pointer
    /// (which does not select it: only motion does).
    fn entered(&mut self) {
        self.focus = self.section().and_then(|section| {
            section
                .focus_on_entry
                .then(|| section.buttons.iter().position(|button| button.enabled))
                .flatten()
        });
        self.hover = self.pointer.and_then(|(x, y)| self.button_at(x, y));
        self.pressed = None;
    }
}

/// Presents one section: its `P` rows into panes, its `B` rows into buttons
/// and its `M` rows into movies, every row it cannot draw kept in `skipped`
/// with the reason.
fn present_section(
    layout: &OriginalLayout,
    section: &super::original::Section,
    measure: &mut dyn FnMut(&str) -> Option<(u32, u32)>,
    handlers: Option<&ScreenHandlers>,
) -> ShellSection {
    let mut movies = Vec::new();
    let mut panes = Vec::new();
    let mut buttons = Vec::new();
    let mut skipped = Vec::new();
    for pane in &section.panes {
        match present_pane(pane, measure) {
            Ok(present) => panes.push(present),
            Err(reason) => skipped.push(SkippedRow {
                id: pane.id.clone(),
                kind: pane.kind.clone(),
                reason,
            }),
        }
    }
    for button in &section.buttons {
        match present_button(layout, section, button, measure, handlers) {
            Ok(present) => buttons.push(present),
            Err(reason) => skipped.push(SkippedRow {
                id: button.id.clone(),
                kind: button.kind.clone(),
                reason,
            }),
        }
    }
    for row in &section.other {
        if !row.kind.eq_ignore_ascii_case("M") {
            continue;
        }
        match present_movie(row) {
            Ok(present) => movies.push(present),
            Err(reason) => skipped.push(SkippedRow {
                id: row.id.clone(),
                kind: row.kind.clone(),
                reason,
            }),
        }
    }
    let mut other_rows: Vec<String> = section
        .other
        .iter()
        .filter(|row| !row.kind.eq_ignore_ascii_case("M"))
        .map(|row| row.kind.clone())
        .collect();
    other_rows.sort();
    other_rows.dedup();
    ShellSection {
        name: section.name.clone(),
        movies,
        panes,
        buttons,
        skipped,
        other_rows,
        focus_on_entry: true,
    }
}

/// The numeric fields a row carries: `X` and `Y` as *signed* numbers —
/// negative positions are real data (a pane hung off an edge), not a parse
/// failure — while `Z`/`NUMFRAMES`/flags stay unsigned as the file writes
/// them.
fn signed(row: &Row, index: usize) -> Option<i32> {
    row.fields.get(index)?.trim().parse().ok()
}

/// Presents one `P` row: `ID=P,art,X,Y,Z,NUMFRAMES,IsRegion?,AlphaType,…`.
fn present_pane(
    row: &Row,
    measure: &mut dyn FnMut(&str) -> Option<(u32, u32)>,
) -> Result<ShellPane, SkippedReason> {
    if let Some(name) = row.unresolved.first() {
        return Err(SkippedReason::UnresolvedPlaceholder { name: name.clone() });
    }
    let art = row.fields.first().map_or("", String::as_str);
    if art.is_empty() {
        return Err(SkippedReason::EmptyArt);
    }
    let (Some(x), Some(y)) = (signed(row, 1), signed(row, 2)) else {
        return Err(SkippedReason::PositionUnread);
    };
    let frames = row
        .fields
        .get(4)
        .and_then(|field| field.trim().parse::<u32>().ok());
    let (width, height) = measure(art).ok_or_else(|| SkippedReason::ArtUnread {
        art: art.to_owned(),
    })?;
    let frames = match frames {
        Some(0) | None => {
            return Err(SkippedReason::FrameCountUnknown {
                height: frames.unwrap_or(0),
            });
        }
        Some(frames) => frames,
    };
    if height % frames != 0 || width == 0 {
        return Err(SkippedReason::FrameCountUnknown { height });
    }
    Ok(ShellPane {
        id: row.id.clone(),
        art: art.to_owned(),
        rect: ShellRect {
            x,
            y,
            width,
            height: height / frames,
        },
        frames,
        frame: 0,
        is_region: row.fields.get(5).is_some_and(|field| field.trim() == "1"),
        color_keyed: row.fields.get(6).is_some_and(|field| field.trim() == "2"),
        transform: None,
    })
}

/// The frames of a check-box button's art: the four state frames unchecked,
/// then the same four checked. Measured on `PF_B_CheckBoxSmall.png`
/// (45x232 = 8 frames of 29; the lower four carry the cross). The order of
/// the checked half repeating the unchecked half's state order is read off
/// the art, not off a script.
pub const CHECK_STATE_FRAMES: u32 = 2 * BUTTON_STATE_FRAMES;

/// The art whose strip is [`BUTTON_STATE_FRAMES`] frames plus a trailing
/// transparent row, measured on the installation (task #1840): the Previous
/// Missions strip is 201x129, every row of its four 32-row frames repeats at
/// period 32 and row 128 holds no opaque texel. `(art stem, height, frame
/// height)`; an art not named here must divide evenly.
const PADDED_BUTTON_STRIPS: &[(&str, u32, u32)] = &[("pc_b_previousmissions", 129, 32)];

/// One state frame's height of a button strip, or `None` when the strip is
/// not [`BUTTON_STATE_FRAMES`] equal frames (never guessed).
fn button_frame_height(art: &str, width: u32, height: u32) -> Option<u32> {
    let stem = art
        .rsplit_once('.')
        .map_or(art, |(stem, _)| stem)
        .to_ascii_lowercase();
    if height == 0 || width == 0 || stem.contains("8states") {
        return None;
    }
    if let Some((_, _, frame)) = PADDED_BUTTON_STRIPS
        .iter()
        .find(|(name, padded, _)| *name == stem && *padded == height)
    {
        return Some(*frame);
    }
    height
        .is_multiple_of(BUTTON_STATE_FRAMES)
        .then_some(height / BUTTON_STATE_FRAMES)
}

/// Presents one `B` row: `ID=B,art,X,Y,Z,ResID,ScriptToExe,…` — its region is
/// the art's own extent divided by [`BUTTON_STATE_FRAMES`], its effect the
/// destination the data states (see the module docs).
fn present_button(
    layout: &OriginalLayout,
    section: &super::original::Section,
    row: &Row,
    measure: &mut dyn FnMut(&str) -> Option<(u32, u32)>,
    handlers: Option<&ScreenHandlers>,
) -> Result<ShellButton, SkippedReason> {
    if let Some(name) = row.unresolved.first() {
        return Err(SkippedReason::UnresolvedPlaceholder { name: name.clone() });
    }
    let art = row.fields.first().map_or("", String::as_str);
    if art.is_empty() {
        return Err(SkippedReason::EmptyArt);
    }
    let (Some(x), Some(y)) = (signed(row, 1), signed(row, 2)) else {
        return Err(SkippedReason::PositionUnread);
    };
    let (width, height) = measure(art).ok_or_else(|| SkippedReason::ArtUnread {
        art: art.to_owned(),
    })?;
    // `ID=B,…` rows write `Type` (`0` normal, `1` check, `2` radio) and
    // `Checked?` after `Left,Top,Right,Bottom` (columns 12 and 13 once the
    // header's omitted `ResID` is accounted for, like `ScriptToExe` at 5).
    let kind = row.fields.get(12).map_or("", |field| field.trim());
    let check = (kind == "1").then(|| row.fields.get(13).is_some_and(|field| field.trim() == "1"));
    let frame_height = if check.is_some() {
        (height != 0 && height.is_multiple_of(CHECK_STATE_FRAMES))
            .then_some(height / CHECK_STATE_FRAMES)
    } else {
        button_frame_height(art, width, height)
    };
    let Some(frame_height) = frame_height else {
        return Err(SkippedReason::FrameCountUnknown { height });
    };
    let script = row.fields.get(5).map_or("", String::as_str);
    let effect = if !script.is_empty() {
        ButtonEffect::Navigate(script.to_owned())
    } else {
        handler_effect(&section.name, row, handlers)
    };
    let effect = match effect {
        ButtonEffect::Navigate(name) | ButtonEffect::Return(name)
            if layout.section(&name).is_none() =>
        {
            ButtonEffect::Unhandled(UnhandledEffect::ScriptNotPresent { script: name })
        }
        other => other,
    };
    let res_id = row
        .fields
        .get(4)
        .map(String::as_str)
        .map(str::trim)
        .filter(|field| !field.is_empty() && *field != "0")
        .map(str::to_owned);
    Ok(ShellButton {
        id: row.id.clone(),
        art: art.to_owned(),
        rect: ShellRect {
            x,
            y,
            width,
            height: frame_height,
        },
        res_id,
        effect,
        check,
        enabled: true,
    })
}

/// Presents one `M` row: `ID=M,file,X,Y,Z,ScaleX,ScaleY,# Loops,Region?` —
/// the header's `ID=M,X,Y,Z,ScaleX,ScaleY,# Loops,Region?,HelpID` omits the
/// file column the data rows write first (the same staleness F45-B.1
/// recorded on the button header). The drawn rect waits for the decoded
/// extent ([`ShellMovie::rect_at`]): the row carries the authored *scale*,
/// not pixels.
fn present_movie(row: &Row) -> Result<ShellMovie, SkippedReason> {
    if let Some(name) = row.unresolved.first() {
        return Err(SkippedReason::UnresolvedPlaceholder { name: name.clone() });
    }
    let file = row.fields.first().map_or("", String::as_str);
    if file.is_empty() {
        return Err(SkippedReason::MovieFieldUnread { field: "file" });
    }
    let (Some(x), Some(y), Some(z)) = (signed(row, 1), signed(row, 2), signed(row, 3)) else {
        return Err(SkippedReason::PositionUnread);
    };
    let unsigned = |index: usize, field: &'static str| {
        row.fields
            .get(index)
            .and_then(|field| field.trim().parse::<u32>().ok())
            .ok_or(SkippedReason::MovieFieldUnread { field })
    };
    Ok(ShellMovie {
        id: row.id.clone(),
        file: file.to_owned(),
        x,
        y,
        z: u32::try_from(z).map_err(|_| SkippedReason::PositionUnread)?,
        scale_x: unsigned(4, "ScaleX")?,
        scale_y: unsigned(5, "ScaleY")?,
        loops: unsigned(6, "# Loops")?,
        region: row.fields.get(7).is_some_and(|field| field.trim() == "1"),
    })
}

/// The effect of a button that starts no screen script, from the `case` arm
/// its own section's script states:
///
/// * exactly `terminate` — ends the run;
/// * `activate(@name@)` followed only by `script_end "<this>.script"` — the
///   section hands over to `name` and ends itself, the pair `CAMPAIGN.SCRIPT`
///   states for `CM_B_CANCEL`;
/// * anything else stays unhandled, verbatim.
fn handler_effect(section: &str, row: &Row, handlers: Option<&ScreenHandlers>) -> ButtonEffect {
    let Some(handlers) = handlers else {
        return ButtonEffect::Unhandled(UnhandledEffect::NoHandler);
    };
    let Some(statements) = handlers.handler(&row.id) else {
        return ButtonEffect::Unhandled(UnhandledEffect::NoHandler);
    };
    if statements == ["terminate"] {
        return ButtonEffect::Terminate;
    }
    if let Some(destination) = statements
        .first()
        .and_then(|statement| activate_target(statement))
    {
        let self_end = format!("script_end \"{}.script\"", section.to_ascii_lowercase());
        if statements[1..]
            .iter()
            .all(|statement| statement.eq_ignore_ascii_case(&self_end))
            && !statements[1..].is_empty()
        {
            return ButtonEffect::Return(destination);
        }
    }
    ButtonEffect::Unhandled(UnhandledEffect::HandlerNotShell {
        statements: statements.to_vec(),
    })
}

/// The `name` of `activate(@name@)`, exactly as written.
fn activate_target(statement: &str) -> Option<String> {
    let inner = statement
        .trim()
        .strip_prefix("activate(@")?
        .strip_suffix("@)")?;
    Some(inner.to_owned())
}
