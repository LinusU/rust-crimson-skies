//! The cabin's Plane Construction hub (task #2042, part of #2036): the
//! `[@PlaneConstruction@]` section of `ASSETS/LAYOUT.CSV` with the screen script
//! `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` runs over it, as the model a
//! presenter draws and a headless drive reads. The Plane Name dialog's OK opens
//! it ([`super::plane_name`]).
//!
//! The panes and buttons draw through the [`super::MenuShell`] like every other
//! section; this page owns what the shell only counts: the plane-name edit box,
//! the captions and statistics texts, the selected tab, the cancel prompt and
//! the numbers the draft hands in ([`HubFeed`]). Nothing here renders.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! * **Layout.** Three buttons below the panel area (`PX_B_CANCEL`,
//!   `PX_B_READY`, `PX_B_SELL`), six tab buttons (`PX_B_AIRFRAME`, `_ENGINE`,
//!   `_ARMOR`, `_GUNS`, `_HARDPOINTS`, `_PAINT`, every one the 127-wide
//!   `PX_Tab.png` with its `IDS_PX_B_*` caption), the backdrop
//!   `PX_BackGround.jpg`, a plane picture pane (`PX_P_PLANE0`, whose art the
//!   engine supplies), four small-icon/plane slots and 20 bar-graph panes, and
//!   the `T` rows. All rows are absolute: the script sets no `relative`.
//! * **Tabs.** The script sends the selected tab `mail(10000, ..)` and the
//!   previous one `mail(10018, ..)`; the airframe tab is selected on open
//!   (`VMA = KNA; mail(10000, KNA)`) and its panel script
//!   (`airframe.script`) started. The tab strips carry four colours per button
//!   (the layout header names them `ColorDisabled, ColorActive,
//!   ColorRollover, ColorDepressed`); a selected tab draws as `Depressed`.
//! * **Plane name.** The edit box `PX_E_NAME` (211-wide by the script's
//!   `SNA.WB = 302 - R`, starting right of the `PLANE NAME:` caption,
//!   `R = QNA.location.x + QNA.WB`, 16 characters) holds the name entered in
//!   the dialog before it.
//! * **Texts.** `PX_T_PLANECOST` is string 1036 over the plane's cost,
//!   `PX_T_AIRFRAME` string 1029 over the airframe's name, `PX_T_WEIGHTCAPACITY`
//!   1030 and `PX_T_CURRENTWEIGHT` 1031 (1032, *Pending*, when the engine flag
//!   `[0x647ba4]` is set). `PX_T_CASH` is the cash with a `$` in front.
//! * **Colours.** The cost line turns red when the cost is more than the cash
//!   (`E 2213` answers `cost <= cash`, handler `0x40add0`, `setle`); the
//!   current-weight line turns red when the weight is more than the airframe's
//!   capacity and the flag `[0x647ba4]` is clear (`E 2214`, handler
//!   `0x40ae33`, compare at `0x40af15`).
//! * **Cancel.** `E 2221` (handler `0x40b6b3`) compares the working plane record
//!   at `0x64cb78..` with a saved copy at `0x6480fc..` field by field and
//!   answers non-zero when they differ. A difference opens message box 4 with
//!   the text `E 2265` builds (handler `0x40b3d4`: string 1228, *Are you sure
//!   you want to cancel building this %1!s!?*, over the airframe's short name,
//!   string `3020 + airframe`); an answer of 3 leaves. No difference leaves
//!   at once. Leaving goes back to whoever opened the dialog: `$$CV$$` Instant
//!   Action, `$$TGA$$` the multiplayer menu, else the cabin.
//! * **Sell** is active only when `E 2018` answers 3 or more (handler
//!   `0x4097ea`, a count it reads from the engine); it runs `hangar.script`
//!   ([`super::construction_hangar`]).
//! * **Default loadout.** `PLANECONSTRUCTION.SCRIPT` asks the Plane Name
//!   dialog's check box (`mail(10008, @planename@OOA)`) and hands the answer to
//!   `E 2212` (handler `0x40aacc`): [`StartingLoadout`].
//! * **The four picture panes** (`px_p_plane0..3`, `E 2243`) are
//!   [`super::construction_hangar::SlotIcons`].
//!
//! # What is *not* known, and so not claimed
//!
//! * **The statistics** (task #2051, [`super::construction_stats`]):
//!   `PX_T_TOPSPEED`, `_AGILITY`, `_ARMOR` and `_OFFENSE` and the 20 bar
//!   graphs follow a [`PlaneRecord`] in the feed; without one nothing is drawn
//!   for them. What a draft's choices map to in the record is task #2052's.
//!   The airframe's blueprint on `PX_P_PLANE0` is drawn with them.
//! * **`E 2230`** (handler `0x40d473`) and **`E 2507`** (the `A` callback
//!   class) are unread.
//! * **The cash and cost figures' unit.** The strings say pounds (*lbs.*) and
//!   dollars; the draft's own weight and price units are fed through
//!   unchanged and their equivalence to the engine's is unmeasured.
//! * **The *gimme* cheat** the script's `gui_char` implements is not modelled.
//! * **The tab panels, Ready's purchase panel and the Sell hangar** are other
//!   tasks; their buttons put up a marked not-implemented notice.
//! * **Message box 4's look** is the message-box presenter's, which is not
//!   built: the page shows the string with the keys that answer it.
//! * **Fonts.** The text uses the engine's default face at the size the
//!   `[FONT]` tag names; the caption faces and the cash row's face (the
//!   `callback($$NB$$, 91, ..)` prefix) are stand-ins.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::construction::BudgetQuantity;

use cs_types::content::ContentId;

use super::construction_armor_paint::{
    ArmorFeed, ArmorPanel, DescBox, DropList, PaintFeed, PaintPanel, PanelEdit,
};
use super::construction_hangar::{HangarPage, SavedPlane, SlotIcons};
use super::construction_panels::{ChoiceKind, ChoiceOption, ChoicePanel, ChoicePress};
use super::construction_purchase::{
    PURCHASE_BUTTON, PURCHASE_SECTION, PurchaseError, PurchaseFeed, PurchaseOutcome, PurchasePage,
};
use super::construction_slots::{SlotKind, SlotOption, SlotPanel, SlotPress};
use super::construction_stats::{self, PlaneRecord};
use super::original::{OriginalLayout, Section};
use super::pilot_page::EditBox;
use super::previous_missions::{colour, split_tag};
use super::shell::{ShellPane, ShellRect, ShellSection};
use super::start_flow::NotImplemented;
use crate::construction::ConstructionView;

/// The layout section this page belongs to.
pub const HUB_SECTION: &str = "PlaneConstruction";
/// The plane-name edit box row.
pub const EDIT_ROW: &str = "PX_E_NAME";
/// Cancel.
pub const CANCEL_BUTTON: &str = "PX_B_CANCEL";
/// Ready.
pub const READY_BUTTON: &str = "PX_B_READY";
/// Sell.
pub const SELL_BUTTON: &str = "PX_B_SELL";
/// The string of the cancel prompt: *Are you sure you want to cancel building
/// this %1!s!?*.
pub const CANCEL_PROMPT_STRING: u32 = 1228;
/// Message box 4's answer that leaves (`@globals@OR.VR == 3`).
pub const LEAVE_ANSWER: u32 = 3;
/// The colour the cost and weight lines turn when over their limit.
pub const OVER_COLOUR: u32 = 0xFFFF_0000;
/// The `Sell` button needs this many planes (`E 2018` answers `3 > count`
/// to deactivate it).
pub const SELL_MIN_PLANES: u32 = 3;

const PLANE_NAME_RIGHT: i32 = 302;
const PLANE_NAME_LABEL: &str = "PX_T_PLANENAME";
const COST_LABEL: &str = "PX_T_PLANECOST";
const AIRFRAME_LABEL: &str = "PX_T_AIRFRAME";
const CAPACITY_LABEL: &str = "PX_T_WEIGHTCAPACITY";
const WEIGHT_LABEL: &str = "PX_T_CURRENTWEIGHT";
const CASH_ROW: &str = "PX_T_CASH";
/// The weight line's text while the engine flag `[0x647ba4]` is set.
pub const WEIGHT_PENDING_STRING: u32 = 1032;
/// The size of the cash row's text: its title's, the row has no tag of its own.
const CASH_SIZE: u32 = 14;

/// The `T` rows with no resource id the engine fills from tables this project
/// has not read ([`HubPage::unread_rows`]).
const STATISTIC_ROWS: [&str; 4] = [
    "PX_T_TOPSPEED",
    "PX_T_AGILITY",
    "PX_T_ARMOR",
    "PX_T_OFFENSE",
];

/// Panes the engine shows from the plane record: the statistics' bar graphs
/// (`E 2015(25, 0x40, ..)`), the small icons and the plane slots. The shell
/// holds them back until a record is fed ([`prepare_section`]).
const RUNTIME_PANES: [&str; 2] = ["PX_P_SMALLICONS", "PX_P_PLANE"];

/// The six tab buttons: the panel each selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    /// `PX_B_AIRFRAME`, `AIRFRAME.SCRIPT`.
    Airframe,
    /// `PX_B_ENGINE`, `ENGINE.SCRIPT`.
    Engine,
    /// `PX_B_ARMOR`, `ARMOR.SCRIPT`.
    Armor,
    /// `PX_B_GUNS`, `GUNS.SCRIPT`.
    Guns,
    /// `PX_B_HARDPOINTS`, `ORDINANCELAYOUT.SCRIPT`.
    Hardpoints,
    /// `PX_B_PAINT`, the paint panel.
    Paint,
}

impl Panel {
    /// The tabs in the order the layout declares them.
    pub const ALL: [Self; 6] = [
        Self::Airframe,
        Self::Engine,
        Self::Armor,
        Self::Guns,
        Self::Hardpoints,
        Self::Paint,
    ];

    /// The tab's button row id.
    #[must_use]
    pub const fn button(self) -> &'static str {
        match self {
            Self::Airframe => "PX_B_AIRFRAME",
            Self::Engine => "PX_B_ENGINE",
            Self::Armor => "PX_B_ARMOR",
            Self::Guns => "PX_B_GUNS",
            Self::Hardpoints => "PX_B_HARDPOINTS",
            Self::Paint => "PX_B_PAINT",
        }
    }

    /// The panel for a button row id.
    #[must_use]
    pub fn of_button(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|panel| panel.button().eq_ignore_ascii_case(id))
    }

    /// The marked notice a press puts up until the panel is built, and the
    /// Rally task that builds it.
    #[must_use]
    pub const fn not_built(self) -> (NotImplemented, u32) {
        match self {
            Self::Airframe => (
                NotImplemented {
                    section: HUB_SECTION,
                    button: "PX_B_AIRFRAME",
                    what: "the Airframe panel",
                },
                2043,
            ),
            Self::Engine => (
                NotImplemented {
                    section: HUB_SECTION,
                    button: "PX_B_ENGINE",
                    what: "the Engine panel",
                },
                2043,
            ),
            Self::Armor => (
                NotImplemented {
                    section: HUB_SECTION,
                    button: "PX_B_ARMOR",
                    what: "the Armor panel",
                },
                2044,
            ),
            Self::Guns => (
                NotImplemented {
                    section: HUB_SECTION,
                    button: "PX_B_GUNS",
                    what: "the Guns panel",
                },
                2045,
            ),
            Self::Hardpoints => (
                NotImplemented {
                    section: HUB_SECTION,
                    button: "PX_B_HARDPOINTS",
                    what: "the Hardpoints panel",
                },
                2045,
            ),
            Self::Paint => (
                NotImplemented {
                    section: HUB_SECTION,
                    button: "PX_B_PAINT",
                    what: "the Paint panel",
                },
                2044,
            ),
        }
    }
}

/// What Ready opens: the *Construction totals* purchase panel (the button's
/// `ScriptToExe` is `Purchase`, task #2053). Built when the layout has the
/// `Purchase` rows; the marked notice stands in for a layout that lacks them.
pub const READY_NOT_BUILT: NotImplemented = NotImplemented {
    section: HUB_SECTION,
    button: "PX_B_READY",
    what: "the purchase panel",
};

/// What the Plane Name dialog's OK stands on when the installation could not
/// supply the hub's rows (rule 5: a missing screen blocks, it never passes).
pub const HUB_UNAVAILABLE: NotImplemented = NotImplemented {
    section: HUB_SECTION,
    button: "PN_B_OK",
    what: "Plane Construction",
};

/// What Sell opens: `hangar.script` (task #2046).
pub const SELL_NOT_BUILT: NotImplemented = NotImplemented {
    section: HUB_SECTION,
    button: "PX_B_SELL",
    what: "the Sell hangar's window",
};

/// What a new plane starts with (`E 2212`, the Plane Name dialog's check box).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingLoadout {
    /// The airframe's template record, weapons included (box checked).
    Stock,
    /// The template with the gun and hardpoint fields cleared (box unchecked).
    Empty,
}

/// What the draft hands the hub. Every number is optional: a figure nobody
/// supplied is drawn as nothing, never as zero.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HubFeed {
    /// The cash on hand.
    pub cash: Option<u64>,
    /// The plane's cost.
    pub plane_cost: Option<u64>,
    /// The airframe's weight capacity.
    pub weight_capacity: Option<u64>,
    /// The plane's current weight.
    pub current_weight: Option<u64>,
    /// Whether the weight line shows *Pending* (the engine flag `[0x647ba4]`).
    pub weight_pending: bool,
    /// The airframe's name (string `3000 + airframe`).
    pub airframe: Option<String>,
    /// The airframe's short name (string `3020 + airframe`), which the cancel
    /// prompt names.
    pub airframe_short: Option<String>,
    /// Whether the working plane differs from the saved one (`E 2221`).
    pub dirty: bool,
    /// How many planes the pilot has saved (`E 2018`); Sell needs
    /// [`SELL_MIN_PLANES`].
    pub saved_planes: Option<u32>,
    /// The working plane record the statistics read (`E 2015(25, ..)`);
    /// nothing is drawn for them without one. The draft does not name the
    /// engine's fields, so [`Self::from_view`] leaves it to the caller
    /// (task #2052).
    pub plane: Option<PlaneRecord>,
}

impl HubFeed {
    /// The feed the draft view gives: the cost meter's use is the plane's
    /// cost, the mass meter's limit the weight capacity and its use the
    /// current weight, and the view's dirty flag `E 2221`'s answer. A view
    /// whose verdict could not be measured has no meters, so those figures
    /// stay unknown.
    #[must_use]
    pub fn from_view(view: &ConstructionView, cash: Option<u64>) -> Self {
        let meter = |quantity| view.meters.iter().find(|meter| meter.quantity == quantity);
        Self {
            cash,
            plane_cost: meter(BudgetQuantity::Cost).map(|meter| meter.used),
            weight_capacity: meter(BudgetQuantity::Mass).map(|meter| meter.limit),
            current_weight: meter(BudgetQuantity::Mass).map(|meter| meter.used),
            dirty: view.dirty,
            ..Self::default()
        }
    }
}

/// Why the page could not be read from the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubError {
    /// The layout has no `PlaneConstruction` section.
    NoSection,
    /// A row the page needs is not in the section.
    RowMissing(&'static str),
    /// A column is not the number the row layout states.
    Column {
        /// The row.
        row: String,
        /// The column.
        column: &'static str,
    },
}

impl fmt::Display for HubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no {HUB_SECTION} section"),
            Self::RowMissing(id) => write!(f, "{HUB_SECTION} has no {id} row"),
            Self::Column { row, column } => write!(f, "{row}: the {column} column is unreadable"),
        }
    }
}

impl std::error::Error for HubError {}

/// A `T` row of the hub: its string and box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubLabel {
    /// The row id (upper-cased).
    pub id: String,
    /// The string, `[FONT]` tag stripped, `%1!d!`/`%1!s!` still in it.
    pub template: String,
    /// The `[FONT]` tag's size.
    pub size: Option<u32>,
    /// The box in surface pixels; a zero width or height is the row's
    /// auto-size.
    pub rect: ShellRect,
    /// `0xAARRGGBB`.
    pub color: u32,
    /// 0 left, 1 centre, 2 right.
    pub justify: u32,
}

/// A text the page draws now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubText {
    /// The row id.
    pub id: String,
    /// The finished text.
    pub text: String,
    /// The size the tag names.
    pub size: Option<u32>,
    /// The box.
    pub rect: ShellRect,
    /// `0xAARRGGBB`, red when over a limit.
    pub color: u32,
    /// 0 left, 1 centre, 2 right.
    pub justify: u32,
}

/// A tab's caption and its four colours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabCaption {
    /// The button id (upper-cased).
    pub button: String,
    /// The text.
    pub text: String,
    /// The `[FONT]` tag's size.
    pub size: Option<u32>,
    /// `[Disabled, Active, Rollover, Depressed]`, as the layout header names
    /// the columns.
    pub colors: [u32; 4],
}

/// What a press on the page did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubOutcome {
    /// The press is not the page's.
    Nothing,
    /// A tab was selected. An Airframe or Engine tab whose panel is built
    /// ([`HubPage::panel_built`]) needs nothing more; any other panel is not
    /// built.
    Panel(Panel),
    /// An entry of the Airframe or Engine panel's list was chosen
    /// (`mail 10015`); [`super::construction_panels::apply_choice`] puts it
    /// into the draft.
    Choose {
        /// The panel.
        panel: Panel,
        /// The content chosen.
        id: ContentId,
    },
    /// An entry of the Guns or Hardpoints panel's list was chosen (`mail
    /// 10015`); [`super::construction_slots::apply_slot_choice`] puts it into
    /// the draft. `id` is `None` for the empty position.
    ChooseSlot {
        /// The panel.
        panel: Panel,
        /// The position in the panel.
        slot: usize,
        /// The content chosen.
        id: Option<ContentId>,
    },
    /// A press opened or closed a panel's list.
    List,
    /// Ready: its panel is not built.
    Ready,
    /// Ready opened the purchase panel ([`HubPage::purchase`]).
    Purchase,
    /// A message box is up ([`HubPage::message`]): the name is missing (203),
    /// the hangar is full (204) or the plane was bought (703).
    Message,
    /// *Purchase Now* may run: the caller commits the draft
    /// ([`super::construction_purchase::commit_purchase`]), stores the plane
    /// in `slot`, and tells the page ([`HubPage::finish_purchase`]).
    Commit {
        /// The record the plane goes into.
        slot: usize,
        /// The plane's name.
        name: String,
        /// The cost taken off the cash.
        value: i32,
    },
    /// Sell: the hangar is not built.
    Sell,
    /// Cancel with unsaved changes: the prompt is up ([`HubPage::prompt`]).
    ConfirmLeave,
    /// The prompt was answered no.
    Stay,
    /// Cancel with nothing to lose, or the prompt answered yes: back out.
    Leave,
}

/// The Plane Construction hub page.
#[derive(Clone, Debug)]
pub struct HubPage {
    edit: EditBox,
    text: String,
    labels: Vec<HubLabel>,
    cash: Option<HubLabel>,
    tabs: Vec<TabCaption>,
    pending_weight: Option<String>,
    prompt_template: Option<String>,
    prompt: Option<String>,
    selected: Panel,
    feed: HubFeed,
    unread: Vec<String>,
    stats: StatsRows,
    airframe: Option<ChoicePanel>,
    engine: Option<ChoicePanel>,
    guns: Option<SlotPanel>,
    hardpoints: Option<SlotPanel>,
    dirty: bool,
    armor: Option<ArmorPanel>,
    paint: Option<PaintPanel>,
    edits: Vec<PanelEdit>,
    icons: Option<SlotIcons>,
    default_loadout: bool,
    hangar: Option<HangarPage>,
    purchase: Option<PurchasePage>,
    purchase_open: bool,
    message: Option<Message>,
}

/// An OK-only message box over the hub.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Message {
    text: String,
    /// Closing the box of a finished purchase leaves the screen
    /// (`mail 12004`).
    leaves: bool,
}

/// A stand-in for the width the engine measures a caption at: the face is not
/// in the installation, so this is the glyph count times 0.6 of the size.
#[must_use]
pub fn stand_in_width(text: &str, size: u32) -> u32 {
    (text.chars().count() as f32 * size as f32 * 0.6).ceil() as u32
}

impl HubPage {
    /// Reads the `[@PlaneConstruction@]` rows. `name` answers an `IDS_*` name
    /// and `id` a string-table id with the installation's text, `[FONT]` tag
    /// included.
    ///
    /// # Errors
    ///
    /// [`HubError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        name: &dyn Fn(&str) -> Option<String>,
        id: &dyn Fn(u32) -> Option<String>,
    ) -> Result<Self, HubError> {
        let section = layout.section(HUB_SECTION).ok_or(HubError::NoSection)?;
        let labels = read_labels(section, name)?;
        let edit_row = section
            .other
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case(EDIT_ROW))
            .ok_or(HubError::RowMissing(EDIT_ROW))?;
        let field = |index: usize, column: &'static str| {
            edit_row
                .fields
                .get(index)
                .and_then(|value| value.trim().parse::<i32>().ok())
                .ok_or_else(|| HubError::Column {
                    row: EDIT_ROW.to_owned(),
                    column,
                })
        };
        let colour_at = |index: usize, column: &'static str| {
            edit_row
                .fields
                .get(index)
                .and_then(|value| colour(value))
                .ok_or_else(|| HubError::Column {
                    row: EDIT_ROW.to_owned(),
                    column,
                })
        };
        // `R = QNA.location.x + QNA.WB; SNA.WB = 302 - R; SNA.location.x = R`.
        let title = labels
            .iter()
            .find(|label| label.id.eq_ignore_ascii_case(PLANE_NAME_LABEL))
            .ok_or(HubError::RowMissing(PLANE_NAME_LABEL))?;
        let title_width = if title.rect.width > 0 {
            title.rect.width
        } else {
            stand_in_width(
                &title.template,
                title
                    .size
                    .unwrap_or(super::previous_missions_view::DEFAULT_SIZE as u32),
            )
        };
        let left = title.rect.x + title_width as i32;
        let edit = EditBox {
            font: edit_row.fields.first().cloned().unwrap_or_default(),
            rect: ShellRect {
                x: left,
                y: field(2, "Y")?,
                width: (PLANE_NAME_RIGHT - left).max(0) as u32,
                height: u32::try_from(field(5, "Height")?).unwrap_or(0),
            },
            max_chars: usize::try_from(field(6, "MaxChars")?).unwrap_or(0),
            text_color: colour_at(7, "TextColor")?,
            frame_color: edit_row.fields.get(8).and_then(|value| colour(value)),
            cursor_color: colour_at(9, "CursorColor")?,
        };
        let cash = section
            .other
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case(CASH_ROW))
            .and_then(|row| {
                Some(HubLabel {
                    id: CASH_ROW.to_owned(),
                    template: String::new(),
                    size: Some(CASH_SIZE),
                    rect: row_rect(row).ok()?,
                    color: row.fields.get(6).and_then(|value| colour(value))?,
                    justify: row.fields.get(7)?.trim().parse().ok()?,
                })
            });
        let tabs = read_tabs(section, name);
        let unread = STATISTIC_ROWS
            .iter()
            .filter(|row| {
                section
                    .other
                    .iter()
                    .any(|other| other.id.eq_ignore_ascii_case(row))
            })
            .map(|row| (*row).to_owned())
            .collect();
        Ok(Self {
            edit,
            text: String::new(),
            labels,
            cash,
            tabs,
            pending_weight: id(WEIGHT_PENDING_STRING).map(|raw| split_tag(&raw).1.to_owned()),
            prompt_template: id(CANCEL_PROMPT_STRING).map(|raw| split_tag(&raw).1.to_owned()),
            prompt: None,
            selected: Panel::Airframe,
            feed: HubFeed::default(),
            unread,
            stats: StatsRows::from_layout(section, id),
            // A layout without a panel's rows leaves that tab on the marked
            // notice (rule 5: a missing screen blocks, it never passes).
            airframe: ChoicePanel::from_layout(layout, ChoiceKind::Airframe, name).ok(),
            engine: ChoicePanel::from_layout(layout, ChoiceKind::Engine, name).ok(),
            guns: SlotPanel::from_layout(layout, SlotKind::Guns, name).ok(),
            hardpoints: SlotPanel::from_layout(layout, SlotKind::HardPoints, name).ok(),
            dirty: true,
            // A layout without a panel's rows leaves its tab on the marked
            // notice.
            armor: ArmorPanel::from_layout(layout, name).ok(),
            paint: PaintPanel::from_layout(layout, name).ok(),
            edits: Vec::new(),
            // A layout without the four picture panes draws none.
            icons: SlotIcons::from_section(section),
            // The check box starts checked (`PN_B_DEFAULT`, `1,1`).
            default_loadout: true,
            // A layout without the Hangar rows leaves Sell on the marked notice.
            hangar: HangarPage::from_layout(layout, name, id).ok(),
            // A layout without the Purchase rows leaves Ready on the marked
            // notice.
            purchase: PurchasePage::from_layout(layout, name, id).ok(),
            purchase_open: false,
            message: None,
        })
    }

    /// The Sell hangar's page, when the layout supplied its rows.
    #[must_use]
    pub const fn hangar(&self) -> Option<&HangarPage> {
        self.hangar.as_ref()
    }

    /// The Sell hangar's page, to press its buttons.
    pub const fn hangar_mut(&mut self) -> Option<&mut HangarPage> {
        self.hangar.as_mut()
    }

    /// The purchase panel, when the layout supplied its rows.
    #[must_use]
    pub const fn purchase(&self) -> Option<&PurchasePage> {
        self.purchase.as_ref()
    }

    /// Whether the purchase panel is the one on screen (Ready was pressed
    /// and no tab since).
    #[must_use]
    pub const fn purchase_open(&self) -> bool {
        self.purchase_open && self.purchase.is_some()
    }

    /// Hands the purchase panel the plane and the cash. `Ok(false)` when the
    /// layout gave the panel no rows.
    ///
    /// # Errors
    ///
    /// [`PurchaseError`] for a plane the tables do not cover.
    pub fn set_purchase_feed(&mut self, feed: PurchaseFeed) -> Result<bool, PurchaseError> {
        let Some(purchase) = &mut self.purchase else {
            return Ok(false);
        };
        purchase.set_feed(feed)?;
        self.dirty = true;
        Ok(true)
    }

    /// The message box that is up, if one is.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_ref().map(|message| message.text.as_str())
    }

    /// The tab held down: none while the purchase panel is up (Ready's latch
    /// is released by the panel's own `mail 10017`, `PLANECONSTRUCTION.SCRIPT`
    /// `case 10017`, and the tab before it by Ready's press).
    #[must_use]
    pub fn latched(&self) -> Option<&'static str> {
        if self.purchase_open() {
            None
        } else {
            Some(self.selected.button())
        }
    }

    /// Whether *Purchase Now* is active (`E 2264`).
    #[must_use]
    pub fn purchase_enabled(&self) -> bool {
        self.purchase_open()
            && self
                .purchase
                .as_ref()
                .is_some_and(PurchasePage::button_enabled)
    }

    /// Tells the page the commit ran: the purchase box (`E 2107`) is up, and
    /// closing it leaves the screen. Nothing happens without a purchase panel.
    pub fn finish_purchase(&mut self) -> HubOutcome {
        let Some(text) = self.purchase.as_ref().and_then(PurchasePage::purchased) else {
            return HubOutcome::Nothing;
        };
        self.message = Some(Message { text, leaves: true });
        self.dirty = true;
        HubOutcome::Message
    }

    /// Closes the message box (its OK): after a purchase this leaves the
    /// screen, otherwise it keeps building.
    pub fn dismiss_message(&mut self) -> HubOutcome {
        let Some(message) = self.message.take() else {
            return HubOutcome::Nothing;
        };
        self.dirty = true;
        if message.leaves {
            HubOutcome::Leave
        } else {
            HubOutcome::Stay
        }
    }

    /// Hands the hangar the pilot's saved planes (see
    /// [`HangarPage::set_planes`]) and takes the hub's Sell count from them,
    /// as `E 2018` counts. Does nothing without the hangar's rows.
    pub fn set_saved_planes(
        &mut self,
        planes: Vec<Option<SavedPlane>>,
        selected: usize,
        cash: u64,
    ) {
        let Some(hangar) = self.hangar.as_mut() else {
            return;
        };
        // The hub is the construction screen: engine state 13 or 17.
        hangar.set_planes(planes, true, selected, cash);
        self.feed.saved_planes = Some(hangar.saved_count());
        self.dirty = true;
    }

    /// Takes Sell's count from the hangar again after a sale emptied a record
    /// (`E 2018`).
    pub fn hangar_changed(&mut self) {
        if let Some(hangar) = self.hangar.as_ref() {
            self.feed.saved_planes = Some(hangar.saved_count());
        }
        self.dirty = true;
    }

    /// The four saved-plane picture panes (`PX_P_PLANE0..3`), when the layout
    /// has all four.
    #[must_use]
    pub const fn slot_icons(&self) -> Option<&SlotIcons> {
        self.icons.as_ref()
    }

    /// Whether the Plane Name dialog's *Default loadout* box was checked
    /// (`PN_B_DEFAULT`): the script passes it to `E 2212` (handler `0x40aacc`),
    /// which fills the plane being built from the airframe's template record
    /// (`0x619f58 + 204 * airframe`); with the box unchecked it then clears the
    /// gun and hardpoint fields to the empty position. Which loadout each
    /// airframe's template holds is unread, so a caller applies
    /// [`StartingLoadout::Stock`] only with a template it has.
    #[must_use]
    pub const fn starting_loadout(&self) -> StartingLoadout {
        if self.default_loadout {
            StartingLoadout::Stock
        } else {
            StartingLoadout::Empty
        }
    }

    /// Records the dialog's check box (carried by `NameOutcome::Construct`).
    pub fn set_default_loadout(&mut self, checked: bool) {
        self.default_loadout = checked;
    }

    /// The Airframe or Engine panel, when the layout supplied its rows.
    #[must_use]
    pub fn choice_panel(&self, panel: Panel) -> Option<&ChoicePanel> {
        match panel {
            Panel::Airframe => self.airframe.as_ref(),
            Panel::Engine => self.engine.as_ref(),
            _ => None,
        }
    }

    fn choice_panel_mut(&mut self, panel: Panel) -> Option<&mut ChoicePanel> {
        match panel {
            Panel::Airframe => self.airframe.as_mut(),
            Panel::Engine => self.engine.as_mut(),
            _ => None,
        }
    }

    /// The Guns or Hardpoints panel, when the layout supplied its rows.
    #[must_use]
    pub fn slot_panel(&self, panel: Panel) -> Option<&SlotPanel> {
        match panel {
            Panel::Guns => self.guns.as_ref(),
            Panel::Hardpoints => self.hardpoints.as_ref(),
            _ => None,
        }
    }

    fn slot_panel_mut(&mut self, panel: Panel) -> Option<&mut SlotPanel> {
        match panel {
            Panel::Guns => self.guns.as_mut(),
            Panel::Hardpoints => self.hardpoints.as_mut(),
            _ => None,
        }
    }

    /// Whether `panel` is built (task #2043: Airframe and Engine; #2044:
    /// Armor and Paint; #2045: Guns and Hardpoints; each when its rows are in
    /// the layout).
    #[must_use]
    pub fn panel_built(&self, panel: Panel) -> bool {
        self.choice_panel(panel).is_some()
            || self.slot_panel(panel).is_some()
            || match panel {
                Panel::Armor => self.armor.is_some(),
                Panel::Paint => self.paint.is_some(),
                _ => false,
            }
    }

    /// Hands a Guns or Hardpoints panel its entries and what the draft holds
    /// in each position. Does nothing for a panel that is not built.
    pub fn set_slot_options(
        &mut self,
        panel: Panel,
        options: Vec<SlotOption>,
        held: &[Option<ContentId>],
    ) {
        if let Some(slots) = self.slot_panel_mut(panel) {
            slots.set_options(options, held);
            self.dirty = true;
        }
    }

    /// The draft changed: the Guns or Hardpoints panel's choices follow it.
    pub fn follow_slots(&mut self, panel: Panel, held: &[Option<ContentId>]) {
        if let Some(slots) = self.slot_panel_mut(panel) {
            slots.follow(held);
        }
    }

    /// Hands a panel its entries and the one the draft holds. Does nothing
    /// for a panel that is not built.
    pub fn set_choices(
        &mut self,
        panel: Panel,
        options: Vec<ChoiceOption>,
        selected: Option<&ContentId>,
    ) {
        if let Some(choice) = self.choice_panel_mut(panel) {
            choice.set_options(options, selected);
            self.dirty = true;
        }
    }

    /// The draft changed: the panel's choice follows it.
    pub fn follow_choice(&mut self, panel: Panel, selected: Option<&ContentId>) {
        if let Some(choice) = self.choice_panel_mut(panel) {
            choice.follow(selected);
        }
    }

    /// The pointer moved: the open list's rolled-over entry follows it.
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) {
        let selected = self.selected;
        let changed = match selected {
            Panel::Armor => self
                .armor
                .as_mut()
                .is_some_and(|armor| armor.hover_at(point)),
            Panel::Paint => self
                .paint
                .as_mut()
                .is_some_and(|paint| paint.hover_at(point)),
            _ => {
                if let Some(choice) = self.choice_panel_mut(selected) {
                    choice.hover_at(point);
                }
                if let Some(slots) = self.slot_panel_mut(selected) {
                    slots.hover_at(point);
                }
                false
            }
        };
        self.dirty |= changed;
    }

    /// A pointer press at a logical point, offered to the selected panel's
    /// drop-down before the shell sees it. [`HubOutcome::Nothing`] means the
    /// press is not the panel's.
    pub fn press_at(&mut self, x: i32, y: i32) -> HubOutcome {
        if self.prompt.is_some() || self.message.is_some() || self.purchase_open() {
            return HubOutcome::Nothing;
        }
        let selected = self.selected;
        let took = match selected {
            Panel::Armor => self.armor.as_mut().and_then(|armor| {
                let changed = armor.press_at(x, y)?;
                if !changed.is_empty() {
                    self.edits.push(match armor.fitments() {
                        Ok(fitments) => PanelEdit::Armor(fitments),
                        Err(error) => PanelEdit::Refused(format!("{error:?}")),
                    });
                }
                Some(())
            }),
            Panel::Paint => self.paint.as_mut().and_then(|paint| {
                if paint.press_at(x, y)? {
                    self.edits.push(match paint.selection() {
                        Ok(selection) => PanelEdit::Paint(selection),
                        Err(error) => PanelEdit::Refused(format!("{error:?}")),
                    });
                }
                Some(())
            }),
            _ => {
                if let Some(slots) = self.slot_panel_mut(selected) {
                    return match slots.press_at(x, y) {
                        SlotPress::Nothing => HubOutcome::Nothing,
                        SlotPress::Toggled => HubOutcome::List,
                        SlotPress::Chose { slot, id } => HubOutcome::ChooseSlot {
                            panel: selected,
                            slot,
                            id,
                        },
                    };
                }
                let Some(choice) = self.choice_panel_mut(selected) else {
                    return HubOutcome::Nothing;
                };
                return match choice.press_at(x, y) {
                    ChoicePress::Nothing => HubOutcome::Nothing,
                    ChoicePress::Toggled => HubOutcome::List,
                    ChoicePress::Chose(id) => HubOutcome::Choose {
                        panel: selected,
                        id,
                    },
                };
            }
        };
        if took.is_some() {
            self.dirty = true;
            HubOutcome::List
        } else {
            HubOutcome::Nothing
        }
    }

    /// Closes the selected panel's open list (Esc); whether one was open.
    pub fn close_list(&mut self) -> bool {
        let was_open = self.list_open();
        match self.selected {
            Panel::Armor => {
                if let Some(armor) = &mut self.armor {
                    armor.close();
                }
            }
            Panel::Paint => {
                if let Some(paint) = &mut self.paint {
                    paint.close();
                }
            }
            selected => {
                if let Some(slots) = self.slot_panel_mut(selected) {
                    return slots.close_list();
                }
                return self
                    .choice_panel_mut(selected)
                    .is_some_and(ChoicePanel::close_list);
            }
        }
        was_open
    }

    /// Whether the selected panel's list is open.
    #[must_use]
    pub fn list_open(&self) -> bool {
        match self.selected {
            Panel::Armor => self
                .armor
                .as_ref()
                .is_some_and(|armor| armor.lists().iter().any(DropList::is_open)),
            Panel::Paint => self
                .paint
                .as_ref()
                .is_some_and(|paint| paint.lists().any(DropList::is_open)),
            selected => {
                self.choice_panel(selected)
                    .is_some_and(ChoicePanel::is_open)
                    || self
                        .slot_panel(selected)
                        .is_some_and(|slots| slots.open_slot().is_some())
            }
        }
    }

    /// The plane-name edit box.
    #[must_use]
    pub fn edit(&self) -> &EditBox {
        &self.edit
    }

    /// The plane's name.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The tabs' captions.
    #[must_use]
    pub fn tabs(&self) -> &[TabCaption] {
        &self.tabs
    }

    /// The selected tab.
    #[must_use]
    pub fn selected(&self) -> Panel {
        self.selected
    }

    /// The numbers the page draws.
    #[must_use]
    pub fn feed(&self) -> &HubFeed {
        &self.feed
    }

    /// The statistics rows nobody has fed a plane record: nothing is drawn
    /// for them.
    #[must_use]
    pub fn unread_rows(&self) -> &[String] {
        if self.feed.plane.is_some() {
            &[]
        } else {
            &self.unread
        }
    }

    /// The four levels (speed, armor, agility, offense) the fed plane
    /// record gives, `None` without one or when its tables have no row.
    #[must_use]
    pub fn stat_levels(&self) -> Option<[u8; construction_stats::STATS]> {
        construction_stats::levels(&self.feed.plane?).ok()
    }

    /// The airframe's blueprint pane (`E 2243` with its flag clear), at
    /// the `PX_P_PLANE0` row's position; `None` without a fed airframe the
    /// tables cover or without its measured art
    /// ([`Self::set_blueprint_extents`]).
    #[must_use]
    pub(super) fn blueprint(&self) -> Option<ShellPane> {
        let plane = self.feed.plane?;
        if plane.airframe as usize >= construction_stats::AIRFRAMES {
            return None;
        }
        let art = super::construction_hangar::blueprint_art(plane.airframe);
        let extent = *self.stats.extents.get(&art.to_ascii_lowercase())?;
        Some(blueprint_pane(art, extent, self.stats.plane_at?))
    }

    /// Hands the page the measured extents of the blueprint arts, by
    /// lower-cased name.
    pub fn set_blueprint_extents(&mut self, extents: BTreeMap<String, (u32, u32)>) {
        self.stats.extents = extents;
    }

    /// The cancel prompt's text, while it is up.
    #[must_use]
    pub fn prompt(&self) -> Option<&str> {
        self.prompt.as_deref()
    }

    /// Whether Sell can be pressed (`E 2018` answers 3 or more).
    #[must_use]
    pub fn sell_enabled(&self) -> bool {
        self.feed
            .saved_planes
            .is_some_and(|planes| planes >= SELL_MIN_PLANES)
    }

    pub(super) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Whether the model changed since the last call; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        let mut changed = std::mem::take(&mut self.dirty);
        for panel in [self.airframe.as_mut(), self.engine.as_mut()]
            .into_iter()
            .flatten()
        {
            changed |= panel.take_dirty();
        }
        for panel in [self.guns.as_mut(), self.hardpoints.as_mut()]
            .into_iter()
            .flatten()
        {
            changed |= panel.take_dirty();
        }
        changed
    }

    /// Opens the page afresh with the plane's `name` (the dialog before it
    /// stored it with `E 2215`): the airframe tab selected, no prompt.
    pub fn open(&mut self, name: &str) {
        self.text = name.chars().take(self.edit.max_chars).collect();
        self.selected = Panel::Airframe;
        self.purchase_open = false;
        self.message = None;
        self.prompt = None;
        self.dirty = true;
        if let Some(armor) = &mut self.armor {
            armor.close();
        }
        if let Some(paint) = &mut self.paint {
            paint.close();
        }
    }

    /// The armor panel, when the layout supplied its rows.
    #[must_use]
    pub fn armor(&self) -> Option<&ArmorPanel> {
        self.armor.as_ref()
    }

    /// The paint panel, when the layout supplied its rows.
    #[must_use]
    pub fn paint(&self) -> Option<&PaintPanel> {
        self.paint.as_ref()
    }

    /// The layout section whose panes the selected panel draws, when it is
    /// built.
    #[must_use]
    pub fn panel_section(&self) -> Option<&'static str> {
        if self.purchase_open() {
            return Some(PURCHASE_SECTION);
        }
        match self.selected {
            Panel::Armor if self.armor.is_some() => {
                Some(super::construction_armor_paint::ARMOR_SECTION)
            }
            Panel::Paint if self.paint.is_some() => {
                Some(super::construction_armor_paint::PAINT_SECTION)
            }
            _ => None,
        }
    }

    /// Hands the armor panel its lists and what the draft fits.
    pub fn set_armor_feed(&mut self, feed: ArmorFeed) -> bool {
        match &mut self.armor {
            Some(armor) => {
                armor.set_feed(feed);
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    /// Hands the paint panel its lists and what the draft carries.
    pub fn set_paint_feed(&mut self, feed: PaintFeed) -> bool {
        match &mut self.paint {
            Some(paint) => {
                paint.set_feed(feed);
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    /// The selected panel's drop-down lists (none for a tab with no panel).
    #[must_use]
    pub fn panel_lists(&self) -> Vec<&DropList> {
        match self.selected {
            Panel::Armor => self
                .armor
                .as_ref()
                .map(|armor| armor.lists().iter().collect())
                .unwrap_or_default(),
            Panel::Paint => self
                .paint
                .as_ref()
                .map(|paint| paint.lists().collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The selected panel's description box and the text in it.
    #[must_use]
    pub fn panel_description(&self) -> Option<(DescBox, &str)> {
        match self.selected {
            Panel::Armor => {
                let armor = self.armor.as_ref()?;
                Some((armor.desc_box(), armor.description()?))
            }
            Panel::Paint => {
                let paint = self.paint.as_ref()?;
                Some((paint.desc_box(), paint.description()?))
            }
            _ => None,
        }
    }

    /// The edits the panels made to the draft since the last call, in order;
    /// [`super::construction_armor_paint::apply_edits`] hands them to the
    /// construction screen.
    pub fn take_edits(&mut self) -> Vec<PanelEdit> {
        std::mem::take(&mut self.edits)
    }

    /// Replaces the numbers the page draws.
    pub fn set_feed(&mut self, feed: HubFeed) {
        if self.feed != feed {
            self.feed = feed;
            self.dirty = true;
        }
    }

    /// Typed text: printable characters up to the row's `MaxChars`.
    pub fn type_text(&mut self, text: &str) {
        if self.prompt.is_some() || self.message.is_some() {
            return;
        }
        for character in text.chars().filter(|c| !c.is_control()) {
            if self.text.chars().count() < self.edit.max_chars {
                self.text.push(character);
            }
        }
        self.dirty = true;
    }

    /// Backspace.
    pub fn backspace(&mut self) {
        if self.prompt.is_none() && self.message.is_none() {
            self.text.pop();
            self.dirty = true;
        }
    }

    /// The texts to draw: the `T` rows whose figures are known, with the cost
    /// and weight lines red over their limits.
    #[must_use]
    pub fn texts(&self) -> Vec<HubText> {
        let feed = &self.feed;
        let mut out = Vec::new();
        for label in &self.labels {
            let mut color = label.color;
            let text = match label.id.as_str() {
                COST_LABEL => {
                    let Some(cost) = feed.plane_cost else {
                        continue;
                    };
                    if feed.cash.is_some_and(|cash| cost > cash) {
                        color = OVER_COLOUR;
                    }
                    fill(&label.template, &cost.to_string())
                }
                AIRFRAME_LABEL => {
                    let Some(name) = &feed.airframe else { continue };
                    fill(&label.template, name)
                }
                CAPACITY_LABEL => {
                    let Some(capacity) = feed.weight_capacity else {
                        continue;
                    };
                    fill(&label.template, &capacity.to_string())
                }
                WEIGHT_LABEL => {
                    if feed.weight_pending
                        || self
                            .airframe
                            .as_ref()
                            .is_some_and(ChoicePanel::weight_pending)
                    {
                        // String 1032 replaces 1031; it has no figure.
                        match &self.pending_weight {
                            Some(text) => text.clone(),
                            None => continue,
                        }
                    } else {
                        let Some(weight) = feed.current_weight else {
                            continue;
                        };
                        if feed
                            .weight_capacity
                            .is_some_and(|capacity| weight > capacity)
                        {
                            color = OVER_COLOUR;
                        }
                        fill(&label.template, &weight.to_string())
                    }
                }
                _ => label.template.clone(),
            };
            out.push(HubText {
                id: label.id.clone(),
                text,
                size: label.size,
                rect: label.rect,
                color,
                justify: label.justify,
            });
        }
        out.extend(self.stats.texts(self.stat_levels()));
        if self.purchase_open() {
            out.extend(self.purchase.iter().flat_map(PurchasePage::texts));
        }
        match self.selected {
            _ if self.purchase_open() => {}
            Panel::Armor => out.extend(self.armor.iter().flat_map(ArmorPanel::texts)),
            Panel::Paint => out.extend(self.paint.iter().flat_map(PaintPanel::texts)),
            _ => {}
        }
        if let (Some(label), Some(cash)) = (&self.cash, feed.cash) {
            out.push(HubText {
                id: label.id.clone(),
                text: format!("${cash}"),
                size: label.size,
                rect: label.rect,
                color: label.color,
                justify: label.justify,
            });
        }
        out
    }

    /// A press on one of the page's buttons.
    pub fn press(&mut self, button: &str) -> HubOutcome {
        if self.prompt.is_some() || self.message.is_some() {
            return HubOutcome::Nothing;
        }
        if button.eq_ignore_ascii_case(CANCEL_BUTTON) {
            if self.feed.dirty {
                let name = self.feed.airframe_short.clone().unwrap_or_default();
                self.prompt = Some(
                    self.prompt_template
                        .as_deref()
                        .map_or_else(String::new, |template| fill(template, &name)),
                );
                self.dirty = true;
                HubOutcome::ConfirmLeave
            } else {
                HubOutcome::Leave
            }
        } else if button.eq_ignore_ascii_case(READY_BUTTON) {
            if self.purchase.is_some() {
                self.close_list();
                self.purchase_open = true;
                self.dirty = true;
                HubOutcome::Purchase
            } else {
                HubOutcome::Ready
            }
        } else if button.eq_ignore_ascii_case(PURCHASE_BUTTON) {
            self.press_purchase()
        } else if button.eq_ignore_ascii_case(SELL_BUTTON) {
            if self.sell_enabled() {
                HubOutcome::Sell
            } else {
                HubOutcome::Nothing
            }
        } else if let Some(panel) = Panel::of_button(button) {
            self.close_list();
            self.selected = panel;
            self.purchase_open = false;
            self.dirty = true;
            HubOutcome::Panel(panel)
        } else {
            HubOutcome::Nothing
        }
    }

    /// *Purchase Now*: the name check of `mail 20001` and `E 2210`.
    fn press_purchase(&mut self) -> HubOutcome {
        if !self.purchase_open() {
            return HubOutcome::Nothing;
        }
        // Without the hangar's records `E 2210` has nothing to ask.
        let Some(hangar) = &self.hangar else {
            return HubOutcome::Nothing;
        };
        let free = hangar.free_slot();
        let Some(purchase) = &self.purchase else {
            return HubOutcome::Nothing;
        };
        match purchase.press_purchase(&self.text, free) {
            PurchaseOutcome::Nothing => HubOutcome::Nothing,
            PurchaseOutcome::HangarFull { text } | PurchaseOutcome::NameRequired { text } => {
                match text {
                    Some(text) => {
                        self.message = Some(Message {
                            text,
                            leaves: false,
                        });
                        self.dirty = true;
                        HubOutcome::Message
                    }
                    // A string table without the box's text cannot show it.
                    None => HubOutcome::Nothing,
                }
            }
            PurchaseOutcome::Commit { slot, name, value } => {
                HubOutcome::Commit { slot, name, value }
            }
        }
    }

    /// Answers the cancel prompt: `answer` is the message box's result, 3
    /// meaning leave (`@globals@OR.VR == 3`). With no prompt up it does
    /// nothing.
    pub fn answer_prompt(&mut self, answer: u32) -> HubOutcome {
        if self.prompt.take().is_none() {
            return HubOutcome::Nothing;
        }
        self.dirty = true;
        if answer == LEAVE_ANSWER {
            HubOutcome::Leave
        } else {
            HubOutcome::Stay
        }
    }
}

/// Puts `value` into the string's `%1!d!` / `%1!s!` hole.
fn fill(template: &str, value: &str) -> String {
    template.replace("%1!d!", value).replace("%1!s!", value)
}

/// The box a `T` row states: `X, Y, Width, Height` at columns 1, 2, 4, 5.
pub(super) fn row_rect(row: &super::original::Row) -> Result<ShellRect, HubError> {
    let number = |index: usize| {
        row.fields
            .get(index)
            .and_then(|value| value.trim().parse::<i32>().ok())
    };
    let (Some(x), Some(y)) = (number(1), number(2)) else {
        return Err(HubError::Column {
            row: row.id.clone(),
            column: "X/Y",
        });
    };
    // A width or height the layout leaves as a `<PLACEHOLDER>` no variable
    // defines reads as the row's auto-size, 0.
    Ok(ShellRect {
        x,
        y,
        width: number(4).unwrap_or(0).max(0) as u32,
        height: number(5).unwrap_or(0).max(0) as u32,
    })
}

/// The `T` rows with a resource id `name` answers.
pub(super) fn read_labels(
    section: &Section,
    name: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<HubLabel>, HubError> {
    let mut labels = Vec::new();
    for row in section
        .other
        .iter()
        .filter(|row| row.kind.eq_ignore_ascii_case("T"))
    {
        let Some(res) = row
            .fields
            .first()
            .map(|field| field.trim())
            .filter(|field| !field.is_empty())
        else {
            // The row with no resource id is filled at run time.
            continue;
        };
        let Some(raw) = name(res) else { continue };
        let rect = row_rect(row)?;
        let (Some(color), Some(justify)) = (
            row.fields.get(6).and_then(|value| colour(value)),
            row.fields
                .get(7)
                .and_then(|value| value.trim().parse::<u32>().ok()),
        ) else {
            return Err(HubError::Column {
                row: row.id.clone(),
                column: "Color/Justify",
            });
        };
        let (size, text) = split_tag(&raw);
        labels.push(HubLabel {
            id: row.id.to_ascii_uppercase(),
            template: text.to_owned(),
            size,
            rect,
            color,
            justify,
        });
    }
    Ok(labels)
}

/// The six tabs' captions with their four colours (columns 14 to 17 of a
/// `B` row, which the header names `ColorDisabled .. ColorDepressed`).
fn read_tabs(section: &Section, name: &dyn Fn(&str) -> Option<String>) -> Vec<TabCaption> {
    Panel::ALL
        .into_iter()
        .filter_map(|panel| {
            let button = section
                .buttons
                .iter()
                .find(|button| button.id.eq_ignore_ascii_case(panel.button()))?;
            let res = button
                .fields
                .iter()
                .find(|field| field.trim().starts_with("IDS_"))?;
            let raw = name(res.trim())?;
            let (size, text) = split_tag(&raw);
            let mut colors = [0xFF00_0000; 4];
            for (slot, index) in colors.iter_mut().zip(14..18) {
                *slot = colour(button.fields.get(index)?)?;
            }
            Some(TabCaption {
                button: button.id.to_ascii_uppercase(),
                text: text.to_owned(),
                size,
                colors,
            })
        })
        .collect()
}

/// The statistics' rows: where each text sits and the strings that fill it,
/// and where the blueprint goes (task #2051).
#[derive(Clone, Debug, Default)]
struct StatsRows {
    /// The text rows by statistic.
    rows: [Option<HubLabel>; construction_stats::STATS],
    /// The head strings by statistic, `[FONT]` tag stripped.
    heads: [Option<String>; construction_stats::STATS],
    /// The five rating strings, `[FONT]` tag stripped.
    ratings: Vec<Option<String>>,
    /// The `PX_P_PLANE0` row's position.
    plane_at: Option<(i32, i32)>,
    /// The blueprint arts' measured extents, by lower-cased name.
    extents: BTreeMap<String, (u32, u32)>,
}

impl StatsRows {
    fn from_layout(section: &Section, id: &dyn Fn(u32) -> Option<String>) -> Self {
        let string = |n: u32| id(n).map(|raw| split_tag(&raw).1.to_owned());
        let rows = construction_stats::TEXT_ROWS.map(|name| {
            let row = section
                .other
                .iter()
                .find(|row| row.id.eq_ignore_ascii_case(name))?;
            Some(HubLabel {
                id: name.to_owned(),
                template: String::new(),
                size: None,
                rect: row_rect(row).ok()?,
                color: row.fields.get(6).and_then(|value| colour(value))?,
                justify: row.fields.get(7)?.trim().parse().ok()?,
            })
        });
        let plane_at = section
            .panes
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case("PX_P_PLANE0"))
            .filter(|row| row.unresolved.is_empty())
            .and_then(|row| {
                let number = |index: usize| row.fields.get(index)?.trim().parse::<i32>().ok();
                Some((number(1)?, number(2)?))
            });
        Self {
            rows,
            heads: construction_stats::HEAD_STRINGS.map(id),
            ratings: (0..construction_stats::BARS as u32)
                .map(|level| string(construction_stats::RATING_STRING + level))
                .collect(),
            plane_at,
            extents: BTreeMap::new(),
        }
    }

    /// The four lines for `levels`; nothing without them or without the
    /// installation's strings.
    fn texts(&self, levels: Option<[u8; construction_stats::STATS]>) -> Vec<HubText> {
        let Some(levels) = levels else {
            return Vec::new();
        };
        (0..construction_stats::STATS)
            .filter_map(|stat| {
                let row = self.rows[stat].as_ref()?;
                let (size, head) = split_tag(self.heads[stat].as_deref()?);
                let rating = self.ratings.get(usize::from(levels[stat]))?.as_deref()?;
                Some(HubText {
                    id: row.id.clone(),
                    text: construction_stats::caption(head, rating),
                    size,
                    rect: row.rect,
                    color: row.color,
                    justify: row.justify,
                })
            })
            .collect()
    }
}

/// Takes out of the presented `PlaneConstruction` section the panes the
/// engine shows from the plane record (the bar graphs, the small icons and
/// the plane slots) and holds them in [`ShellSection::held_panes`], where the
/// hub lights them from a fed record.
pub(super) fn prepare_section(section: &mut ShellSection) {
    let (held, shown) = std::mem::take(&mut section.panes)
        .into_iter()
        .partition(|pane| is_stats_pane(&pane.id));
    section.panes = shown;
    section.held_panes = held;
}

/// Whether `id` names a pane the hub shows from the plane record: a bar of
/// one of the four groups, the small icons or one of the plane slots.
fn is_stats_pane(id: &str) -> bool {
    let id = id.to_ascii_uppercase();
    id == RUNTIME_PANES[0]
        || id.starts_with(RUNTIME_PANES[1])
        || construction_stats::BAR_GROUPS
            .iter()
            .any(|group| id.starts_with(group))
}

/// The bar panes of `pool` the levels light: bar `r` of a group when
/// `r <= level`, as the script's `12000` handler activates them. The small
/// icons and the three other plane slots stay hidden: the script shows them
/// only from state the hub does not have.
#[must_use]
pub(super) fn lit_bars(
    pool: &[ShellPane],
    levels: [u8; construction_stats::STATS],
) -> Vec<ShellPane> {
    let mut out = Vec::new();
    for (group, level) in construction_stats::BAR_GROUPS.iter().zip(levels) {
        out.extend(
            pool.iter()
                .filter(|pane| {
                    pane.id
                        .to_ascii_uppercase()
                        .strip_prefix(group)
                        .and_then(|bar| bar.parse::<u8>().ok())
                        .is_some_and(|bar| bar <= level)
                })
                .cloned(),
        );
    }
    out
}

/// The blueprint pane: the airframe's art at the `PX_P_PLANE0` position,
/// whose extent the caller measured.
#[must_use]
pub(super) fn blueprint_pane(art: String, extent: (u32, u32), at: (i32, i32)) -> ShellPane {
    ShellPane {
        id: "PX_P_PLANE0".to_owned(),
        art,
        rect: ShellRect {
            x: at.0,
            y: at.1,
            width: extent.0,
            height: extent.1,
        },
        frames: 1,
        frame: 0,
        is_region: false,
        color_keyed: false,
        transform: None,
    }
}
