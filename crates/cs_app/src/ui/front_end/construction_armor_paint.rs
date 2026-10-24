//! The Plane Construction hub's Armor and Paint panels (task #2044, part of
//! #2036): the `[@Armor@]` and `[@Paint@]` sections of `ASSETS/LAYOUT.CSV` with
//! the scripts `ASSETS/SCRIPTS/ARMOR.SCRIPT` and `PAINT.SCRIPT` run over them,
//! as the model a presenter draws and a headless drive reads. They are drawn
//! inside the hub ([`super::construction_hub`]) when their tab is selected.
//!
//! The panels edit the draft through [`PanelEdit`]s the host hands to the
//! production [`ConstructionScreen`] ([`apply_edits`]): the armor panel's four
//! zones become [`ArmorFitment`]s, the paint panel's pattern and decals a
//! [`PaintSelection`].
//!
//! # What the scripts say (static readings, not `verified_original`)
//!
//! * **Armor.** Four drop-downs `AR_D_POINT0..3` under the titles *nose*,
//!   *tail*, *left* and *right* (strings 1044 to 1047), each filled by
//!   `callback($$E$$, 2246, ..)`, a rule under every title, the panel title, an
//!   armor name (`AR_T_ARMORNAME`, `E 2220(3, ..)`) and a scrolling description
//!   (`AR_S_ARMORDESC`, `E 2225`). A pick in a list stores the choice for that
//!   zone (`E 2247(1, zone, item)`); **the left and right zones mirror each
//!   other**: the mailbox's `default` arm answers zone 2 with zone 3 and zone 3
//!   with zone 2 and copies the choice there. Moving over an item of an open
//!   list (message 10013) shows that item's name and description without
//!   changing the dropdown.
//! * **Paint.** One pattern drop-down (`PT_D_PATTERN`, `E 2238`), three colour
//!   drop-downs (`PT_D_COLORS0..2`, `E 2237`) each with a shade drop-down beside
//!   it (`PT_D_SHADES0..2`, `E 2236`; a colour pick refreshes its shade list),
//!   and three decal drop-downs (`PT_D_DECALS0..2`, `E 2239`) whose rows are
//!   five variants of a decal side by side (`for (BMA = 0; BMA < 5; ..)` over
//!   the `PX_P_Decals.tga` sheet, five frames per decal). The decals sit under
//!   the captions *nose*, *tail* and *wing* (`IDS_PT_DECALNOSE` ..).
//! * The layout's header names a `D` row's columns
//!   `Slider, UpArrow, DownArrow, DropUp, DropDown, ScriptPointer, X, Y, Z,
//!   Width, Height, TotalDisplayed`; the data rows omit `ScriptPointer`.
//!
//! # What is *not* known, and so not claimed
//!
//! * **The lists' contents.** The armor items, patterns, colours, shades and
//!   decals come from the engine's tables (`E 2246`, `2229`, `2232` to `2239`,
//!   `@shareditems@`), which no file holds. The panels draw the lists the
//!   caller feeds ([`ArmorFeed`], [`PaintFeed`]) and nothing for a list nobody
//!   fed. Affected: every entry of both panels. Resolving task: #2052.
//! * **Which damage-graph node a zone is.** [`ArmorFeed::zones`] is declared by
//!   the profile (the finding of #183): the four zones have no node key in any
//!   file.
//! * **What the name and description show before a first pick or hover.**
//!   `E 2220(3, ..)` and `E 2225` fill them at set-up from engine state; the
//!   handlers are unread, so nothing is drawn until an item is picked or
//!   hovered. That the pair describes the last picked or hovered item is read
//!   from the script's shape, not from the handlers.
//! * **Paint beyond pattern and decals.** The colour, shade and decal-variant
//!   choices have no field in [`PaintSelection`] (masks and decal placements
//!   over a mask), so they are kept by the panel ([`PaintChoice`]) and reach no
//!   blueprint. The pattern is read as the blueprint's one mask and a decal as
//!   a placement over it; that is inferred. Affected: the paint panel's
//!   colours, shades and variants. Resolving task: #2052.
//! * **Scrolling.** The lists show at most their `TotalDisplayed` items and
//!   the slider/arrows (`PX_SLIDER`, `PX_UP`, `PX_DOWN`) and the description's
//!   scroll are not built: items beyond the visible ones and text beyond the
//!   box are unreachable.
//! * **The decal pictures** (the `PX_P_Decals.tga` sheet) are not drawn: a
//!   decal row is its five outlined cells.
//! * **The lists' own look** (`GN_DROPUP`, `GN_DROPDOWN` art, the open list's
//!   placement below the closed box, one box height per item) is a stand-in
//!   shared with the Audio page; only the decal lists' offset (`x - 13`,
//!   `y + height + 11`) is read from the script (`10025`).

use cs_content::construction::{
    ArmorFitment, ConstructionSchemaError, DecalPlacement, PaintSelection,
};
use cs_types::content::{ContentId, DamageNodeKey};

use super::construction_hub::{HubLabel, HubText, read_labels, row_rect};
use super::original::{OriginalLayout, Row, Section};
use super::previous_missions::colour;
use super::shell::ShellRect;
use super::widgets::{DropdownRow, WidgetError, dropdown_rows};
use crate::construction::ConstructionScreen;

/// The armor panel's layout section.
pub const ARMOR_SECTION: &str = "Armor";
/// The paint panel's layout section.
pub const PAINT_SECTION: &str = "Paint";
/// The armor zones: nose, tail, left, right.
pub const ARMOR_ZONES: usize = 4;
/// The paint panel's colour (and shade) slots.
pub const PAINT_SLOTS: usize = 3;
/// The variants of one decal (`for (BMA = 0; BMA < 5; ..)`).
pub const DECAL_VARIANTS: u32 = 5;
/// The armor zones that mirror each other (`case 2: AA = 3`, `case 3: AA = 2`).
pub const MIRRORED_ZONES: (usize, usize) = (2, 3);

/// The decal lists open this far left of the closed box (`ZKA = x - 13`).
const DECAL_LIST_DX: i32 = -13;
/// …and this far below its bottom edge (`location.y + XB + 11`).
const DECAL_LIST_DY: i32 = 11;

const ARMOR_NAME_ROW: &str = "AR_T_ARMORNAME";
const ARMOR_DESC_ROW: &str = "AR_S_ARMORDESC";
const PAINT_NAME_ROW: &str = "PT_T_PAINTNAME";
const PAINT_DESC_ROW: &str = "PT_S_PAINTDESC";
const ARMOR_LISTS: [&str; ARMOR_ZONES] =
    ["AR_D_POINT0", "AR_D_POINT1", "AR_D_POINT2", "AR_D_POINT3"];
const PATTERN_LIST: &str = "PT_D_PATTERN";
const COLOR_LISTS: [&str; PAINT_SLOTS] = ["PT_D_COLORS0", "PT_D_COLORS1", "PT_D_COLORS2"];
const SHADE_LISTS: [&str; PAINT_SLOTS] = ["PT_D_SHADES0", "PT_D_SHADES1", "PT_D_SHADES2"];
const DECAL_LISTS: [&str; PAINT_SLOTS] = ["PT_D_DECALS0", "PT_D_DECALS1", "PT_D_DECALS2"];
/// The decal sheet pane: the script deactivates it, it is only a source.
pub(super) const DECAL_SHEET_PANE: &str = "PT_P_DECALS";

/// Why a panel could not be read from the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelError {
    /// The layout has no such section.
    NoSection(&'static str),
    /// A row the panel needs is not in the section.
    RowMissing {
        /// The section.
        section: &'static str,
        /// The row.
        id: &'static str,
    },
    /// A row's column is not readable.
    Widget(WidgetError),
}

impl std::fmt::Display for PanelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSection(name) => write!(f, "the layout has no {name} section"),
            Self::RowMissing { section, id } => write!(f, "{section} has no {id} row"),
            Self::Widget(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PanelError {}

impl From<WidgetError> for PanelError {
    fn from(error: WidgetError) -> Self {
        Self::Widget(error)
    }
}

/// One entry of a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// The entry's text; empty for a swatch or a decal.
    pub label: String,
    /// `0xAARRGGBB` of a colour or shade swatch.
    pub swatch: Option<u32>,
}

/// What a press did to a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListPress {
    /// The press is not this list's.
    Outside,
    /// The closed box was pressed: the list opened.
    Opened,
    /// The open list was dismissed without a pick.
    Closed,
    /// An item (and, for a decal list, a variant cell) was picked.
    Picked {
        /// The item's index.
        item: usize,
        /// The variant cell (always 0 for a plain list).
        cell: u32,
    },
}

/// A drop-down list: the closed box a `D` row states and the items an open
/// list offers.
#[derive(Clone, Debug)]
pub struct DropList {
    row: DropdownRow,
    cells: u32,
    items: Vec<ListItem>,
    selected: Option<(usize, u32)>,
    open: bool,
    hover: Option<(usize, u32)>,
}

impl DropList {
    fn new(row: DropdownRow, cells: u32) -> Self {
        Self {
            row,
            cells,
            items: Vec::new(),
            selected: None,
            open: false,
            hover: None,
        }
    }

    /// The `D` row.
    #[must_use]
    pub fn row(&self) -> &DropdownRow {
        &self.row
    }

    /// The items fed to the list.
    #[must_use]
    pub fn items(&self) -> &[ListItem] {
        &self.items
    }

    /// The picked item and its variant cell.
    #[must_use]
    pub fn selected(&self) -> Option<(usize, u32)> {
        self.selected
    }

    /// Whether the list is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The item (and cell) under the pointer while the list is open.
    #[must_use]
    pub fn hover(&self) -> Option<(usize, u32)> {
        self.hover
    }

    /// The cells of one decal row (1 for a plain list).
    #[must_use]
    pub fn cells(&self) -> u32 {
        self.cells
    }

    /// How many items an open list shows (`TotalDisplayed`, no scrolling).
    #[must_use]
    pub fn shown(&self) -> usize {
        self.items.len().min(self.row.displayed as usize)
    }

    /// The rect of an open list's cell.
    #[must_use]
    pub fn item_rect(&self, item: usize, cell: u32) -> ShellRect {
        let box_rect = self.row.rect;
        let (dx, dy) = if self.cells > 1 {
            (DECAL_LIST_DX, DECAL_LIST_DY)
        } else {
            (0, 0)
        };
        ShellRect {
            x: box_rect.x + dx + (cell * box_rect.width) as i32,
            y: box_rect.y + dy + ((item as u32 + 1) * box_rect.height) as i32,
            width: box_rect.width,
            height: box_rect.height,
        }
    }

    fn set_items(&mut self, items: Vec<ListItem>) {
        self.items = items;
        self.open = false;
        self.hover = None;
        if self
            .selected
            .is_some_and(|(item, _)| item >= self.items.len())
        {
            self.selected = None;
        }
    }

    fn select(&mut self, selection: Option<(usize, u32)>) {
        self.selected =
            selection.filter(|(item, cell)| *item < self.items.len() && *cell < self.cells);
    }

    fn close(&mut self) {
        self.open = false;
        self.hover = None;
    }

    fn cell_at(&self, x: i32, y: i32) -> Option<(usize, u32)> {
        (0..self.shown()).find_map(|item| {
            (0..self.cells).find_map(|cell| {
                self.item_rect(item, cell)
                    .contains(x, y)
                    .then_some((item, cell))
            })
        })
    }

    fn press(&mut self, x: i32, y: i32) -> ListPress {
        if self.open {
            let picked = self.cell_at(x, y);
            self.close();
            return match picked {
                Some((item, cell)) => {
                    self.selected = Some((item, cell));
                    ListPress::Picked { item, cell }
                }
                None if self.row.rect.contains(x, y) => ListPress::Closed,
                None => ListPress::Outside,
            };
        }
        if !self.items.is_empty() && self.row.rect.contains(x, y) {
            self.open = true;
            return ListPress::Opened;
        }
        ListPress::Outside
    }

    fn hover_at(&mut self, point: Option<(i32, i32)>) -> bool {
        let hover = match point {
            Some((x, y)) if self.open => self.cell_at(x, y),
            _ => None,
        };
        std::mem::replace(&mut self.hover, hover) != hover
    }
}

/// The scrolling description's box (`S` row: border, back, text colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescBox {
    /// The box.
    pub rect: ShellRect,
    /// `BorderColor`.
    pub border: u32,
    /// `BackColor`.
    pub back: u32,
    /// `Color`, the text's.
    pub color: u32,
}

/// What both panels share: their captions, the run-time name row and the
/// description box.
#[derive(Clone, Debug)]
struct Common {
    labels: Vec<HubLabel>,
    name: HubLabel,
    desc: DescBox,
}

fn find<'a>(section: &'a Section, id: &str) -> Option<&'a Row> {
    section
        .other
        .iter()
        .find(|row| row.id.eq_ignore_ascii_case(id))
}

fn read_common(
    layout: &OriginalLayout,
    section_name: &'static str,
    name_row: &'static str,
    desc_row: &'static str,
    names: &dyn Fn(&str) -> Option<String>,
) -> Result<Common, PanelError> {
    let section = layout
        .section(section_name)
        .ok_or(PanelError::NoSection(section_name))?;
    let labels = read_labels(section, names).map_err(|_| {
        PanelError::Widget(WidgetError::Column {
            row: section_name.to_owned(),
            column: "T row",
        })
    })?;
    let missing = |id| PanelError::RowMissing {
        section: section_name,
        id,
    };
    let row = find(section, name_row).ok_or_else(|| missing(name_row))?;
    let column = |row: &Row, index: usize, column: &'static str| {
        row.fields
            .get(index)
            .and_then(|field| colour(field))
            .ok_or_else(|| {
                PanelError::Widget(WidgetError::Column {
                    row: row.id.clone(),
                    column,
                })
            })
    };
    let name = HubLabel {
        id: name_row.to_owned(),
        template: String::new(),
        size: None,
        rect: row_rect(row).map_err(|_| {
            PanelError::Widget(WidgetError::Column {
                row: row.id.clone(),
                column: "X/Y",
            })
        })?,
        color: column(row, 6, "Color")?,
        justify: 0,
    };
    let row = find(section, desc_row).ok_or_else(|| missing(desc_row))?;
    let number = |index: usize, column: &'static str| {
        row.fields
            .get(index)
            .and_then(|field| field.trim().parse::<i32>().ok())
            .ok_or_else(|| {
                PanelError::Widget(WidgetError::Column {
                    row: row.id.clone(),
                    column,
                })
            })
    };
    // `ID=S,BorderColor,BackColor,Slider,UpArrow,DownArrow,X,Y,Z,Width,
    // Height,TabOrder,[ResID,]Color`: the data rows omit the `ResID`.
    let desc = DescBox {
        rect: ShellRect {
            x: number(5, "X")?,
            y: number(6, "Y")?,
            width: number(8, "Width")?.max(0) as u32,
            height: number(9, "Height")?.max(0) as u32,
        },
        border: column(row, 0, "BorderColor")?,
        back: column(row, 1, "BackColor")?,
        color: column(row, 11, "Color")?,
    };
    Ok(Common { labels, name, desc })
}

fn read_lists(
    layout: &OriginalLayout,
    section: &'static str,
    ids: &[&'static str],
    cells: u32,
) -> Result<Vec<DropList>, PanelError> {
    let rows = dropdown_rows(layout, section)?;
    ids.iter()
        .map(|id| {
            rows.iter()
                .find(|row| row.id.eq_ignore_ascii_case(id))
                .cloned()
                .map(|row| DropList::new(row, cells))
                .ok_or(PanelError::RowMissing { section, id })
        })
        .collect()
}

fn common_texts(common: &Common, shown: Option<&str>) -> Vec<HubText> {
    let mut out: Vec<HubText> = common
        .labels
        .iter()
        .map(|label| HubText {
            id: label.id.clone(),
            text: label.template.clone(),
            size: label.size,
            rect: label.rect,
            color: label.color,
            justify: label.justify,
        })
        .collect();
    if let Some(name) = shown {
        out.push(HubText {
            id: common.name.id.clone(),
            text: name.to_owned(),
            size: common.name.size,
            rect: common.name.rect,
            color: common.name.color,
            justify: common.name.justify,
        });
    }
    out
}

// ------------------------------------------------------------------ armor ---

/// One armor the engine offers (an item of the zone lists).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmorOption {
    /// The armor component's catalog id.
    pub id: ContentId,
    /// The list text (`E 2246`) and `E 2220(3, ..)`'s name.
    pub name: String,
    /// `E 2225`'s description.
    pub description: String,
}

/// What the draft and the profile hand the armor panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmorFeed {
    /// The armors, in list order.
    pub options: Vec<ArmorOption>,
    /// The damage-graph node of nose, tail, left and right, declared by the
    /// profile.
    pub zones: [DamageNodeKey; ARMOR_ZONES],
    /// What the draft already fits.
    pub fitted: Vec<ArmorFitment>,
}

/// The armor panel.
#[derive(Clone, Debug)]
pub struct ArmorPanel {
    common: Common,
    lists: Vec<DropList>,
    feed: Option<ArmorFeed>,
    shown: Option<usize>,
}

impl ArmorPanel {
    /// Reads the `[@Armor@]` rows.
    ///
    /// # Errors
    ///
    /// [`PanelError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        names: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, PanelError> {
        let common = read_common(layout, ARMOR_SECTION, ARMOR_NAME_ROW, ARMOR_DESC_ROW, names)?;
        // The four zone titles are the script's `ar_t_*title` controls.
        for (_, _, label) in cs_content::construction::ORIGINAL_ARMOR_ZONES {
            if !common
                .labels
                .iter()
                .any(|found| found.id.eq_ignore_ascii_case(label))
            {
                return Err(PanelError::RowMissing {
                    section: ARMOR_SECTION,
                    id: label,
                });
            }
        }
        Ok(Self {
            lists: read_lists(layout, ARMOR_SECTION, &ARMOR_LISTS, 1)?,
            common,
            feed: None,
            shown: None,
        })
    }

    /// The four zone lists: nose, tail, left, right.
    #[must_use]
    pub fn lists(&self) -> &[DropList] {
        &self.lists
    }

    /// The description box.
    #[must_use]
    pub fn desc_box(&self) -> DescBox {
        self.common.desc
    }

    /// The armor picked or hovered last, by option index.
    fn current(&self) -> Option<&ArmorOption> {
        let feed = self.feed.as_ref()?;
        let hovered = self
            .lists
            .iter()
            .find_map(|list| list.hover().map(|(item, _)| item));
        feed.options.get(hovered.or(self.shown)?)
    }

    /// The description to scroll in the box: the armor picked or hovered last.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.current().map(|option| option.description.as_str())
    }

    /// The option each zone holds, by index.
    #[must_use]
    pub fn zone_choice(&self, zone: usize) -> Option<usize> {
        self.lists
            .get(zone)
            .and_then(DropList::selected)
            .map(|(item, _)| item)
    }

    /// Hands the panel the armors, the zones' nodes and what the draft fits.
    pub fn set_feed(&mut self, feed: ArmorFeed) {
        let items: Vec<ListItem> = feed
            .options
            .iter()
            .map(|option| ListItem {
                label: option.name.clone(),
                swatch: None,
            })
            .collect();
        for (zone, list) in self.lists.iter_mut().enumerate() {
            list.set_items(items.clone());
            let held = feed
                .fitted
                .iter()
                .find(|fitment| fitment.zone() == &feed.zones[zone])
                .and_then(|fitment| {
                    feed.options
                        .iter()
                        .position(|option| &option.id == fitment.armor())
                });
            list.select(held.map(|item| (item, 0)));
        }
        self.feed = Some(feed);
        self.shown = None;
    }

    /// Picks `option` for `zone`, as the script's mailbox does: the left and
    /// right zones mirror each other. Returns the zones that changed.
    pub fn choose(&mut self, zone: usize, option: usize) -> Vec<usize> {
        let mut changed = Vec::new();
        let mirrored = match zone {
            z if z == MIRRORED_ZONES.0 => Some(MIRRORED_ZONES.1),
            z if z == MIRRORED_ZONES.1 => Some(MIRRORED_ZONES.0),
            _ => None,
        };
        for target in std::iter::once(zone).chain(mirrored) {
            if let Some(list) = self.lists.get_mut(target) {
                let before = list.selected();
                list.select(Some((option, 0)));
                // The zone picked in is always a change (its list stored the
                // pick before this ran); the mirror only when it differed.
                if target == zone || list.selected() != before {
                    changed.push(target);
                }
            }
        }
        self.shown = Some(option);
        changed
    }

    /// A press at a logical point. `None` when no list took it; otherwise the
    /// zones whose armor changed (empty for an open or a dismissal).
    pub fn press_at(&mut self, x: i32, y: i32) -> Option<Vec<usize>> {
        for zone in 0..self.lists.len() {
            if !self.lists[zone].is_open() {
                continue;
            }
            match self.lists[zone].press(x, y) {
                ListPress::Picked { item, .. } => return Some(self.choose(zone, item)),
                ListPress::Closed => return Some(Vec::new()),
                ListPress::Outside | ListPress::Opened => {}
            }
        }
        for zone in 0..self.lists.len() {
            if self.lists[zone].press(x, y) == ListPress::Opened {
                return Some(Vec::new());
            }
        }
        None
    }

    /// Moves the pointer; whether anything shown changed.
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) -> bool {
        let mut changed = false;
        for list in &mut self.lists {
            changed |= list.hover_at(point);
        }
        changed
    }

    /// The fitments the panel's choices make, one per zone that has one.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] when an option is not an armor id.
    pub fn fitments(&self) -> Result<Vec<ArmorFitment>, ConstructionSchemaError> {
        let Some(feed) = &self.feed else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (zone, key) in feed.zones.iter().enumerate() {
            if let Some(option) = self
                .zone_choice(zone)
                .and_then(|item| feed.options.get(item))
            {
                out.push(ArmorFitment::try_new(key.clone(), option.id.clone())?);
            }
        }
        Ok(out)
    }

    /// The texts to draw: the titles, and the armor's name once one was picked
    /// or hovered.
    #[must_use]
    pub fn texts(&self) -> Vec<HubText> {
        common_texts(
            &self.common,
            self.current().map(|option| option.name.as_str()),
        )
    }

    pub(super) fn close(&mut self) {
        for list in &mut self.lists {
            list.close();
        }
    }
}

// ------------------------------------------------------------------ paint ---

/// One pattern (an item of `PT_D_PATTERN`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternOption {
    /// The paint mask the pattern is.
    pub mask: ContentId,
    /// The list text and `E 2220(6, ..)`'s name.
    pub name: String,
    /// The description.
    pub description: String,
}

/// What the draft and the profile hand the paint panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintFeed {
    /// The patterns, in list order.
    pub patterns: Vec<PatternOption>,
    /// The colours' swatches (`E 2229`).
    pub colours: Vec<u32>,
    /// The shades' swatches of each colour (`E 2236`), indexed like `colours`.
    pub shades: Vec<Vec<u32>>,
    /// The decals, in list order: each a paint-mask id in the catalog.
    pub decals: Vec<ContentId>,
    /// What the draft already carries.
    pub current: PaintSelection,
}

/// What the paint panel's lists hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaintChoice {
    /// The pattern.
    pub pattern: Option<usize>,
    /// The colour of each slot.
    pub colours: [Option<usize>; PAINT_SLOTS],
    /// The shade of each slot.
    pub shades: [Option<usize>; PAINT_SLOTS],
    /// The decal and variant of each slot.
    pub decals: [Option<(usize, u32)>; PAINT_SLOTS],
}

/// The paint panel.
#[derive(Clone, Debug)]
pub struct PaintPanel {
    common: Common,
    pattern: DropList,
    colours: Vec<DropList>,
    shades: Vec<DropList>,
    decals: Vec<DropList>,
    feed: Option<PaintFeed>,
    shown: Option<usize>,
}

impl PaintPanel {
    /// Reads the `[@Paint@]` rows.
    ///
    /// # Errors
    ///
    /// [`PanelError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        names: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, PanelError> {
        let common = read_common(layout, PAINT_SECTION, PAINT_NAME_ROW, PAINT_DESC_ROW, names)?;
        let mut pattern = read_lists(layout, PAINT_SECTION, &[PATTERN_LIST], 1)?;
        Ok(Self {
            common,
            pattern: pattern.remove(0),
            colours: read_lists(layout, PAINT_SECTION, &COLOR_LISTS, 1)?,
            shades: read_lists(layout, PAINT_SECTION, &SHADE_LISTS, 1)?,
            decals: read_lists(layout, PAINT_SECTION, &DECAL_LISTS, DECAL_VARIANTS)?,
            feed: None,
            shown: None,
        })
    }

    /// Every list: pattern, colours, shades, decals.
    pub fn lists(&self) -> impl Iterator<Item = &DropList> {
        std::iter::once(&self.pattern)
            .chain(&self.colours)
            .chain(&self.shades)
            .chain(&self.decals)
    }

    fn lists_mut(&mut self) -> impl Iterator<Item = &mut DropList> {
        std::iter::once(&mut self.pattern)
            .chain(&mut self.colours)
            .chain(&mut self.shades)
            .chain(&mut self.decals)
    }

    /// The description box.
    #[must_use]
    pub fn desc_box(&self) -> DescBox {
        self.common.desc
    }

    fn current(&self) -> Option<&PatternOption> {
        let feed = self.feed.as_ref()?;
        let hovered = self.pattern.hover().map(|(item, _)| item);
        feed.patterns.get(hovered.or(self.shown)?)
    }

    /// The pattern's description, for the box.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.current().map(|pattern| pattern.description.as_str())
    }

    /// What the lists hold.
    #[must_use]
    pub fn choice(&self) -> PaintChoice {
        let slot = |lists: &[DropList]| -> [Option<(usize, u32)>; PAINT_SLOTS] {
            std::array::from_fn(|index| lists.get(index).and_then(DropList::selected))
        };
        let plain = |lists: &[DropList]| slot(lists).map(|held| held.map(|(item, _)| item));
        PaintChoice {
            pattern: self.pattern.selected().map(|(item, _)| item),
            colours: plain(&self.colours),
            shades: plain(&self.shades),
            decals: slot(&self.decals),
        }
    }

    /// Hands the panel the lists' contents and what the draft carries: the
    /// draft's mask picks the pattern and its decal placements the decal
    /// rows (the variant is not in a [`PaintSelection`], so it starts at the
    /// first).
    pub fn set_feed(&mut self, feed: PaintFeed) {
        self.pattern.set_items(
            feed.patterns
                .iter()
                .map(|pattern| ListItem {
                    label: pattern.name.clone(),
                    swatch: None,
                })
                .collect(),
        );
        let swatches = |colours: &[u32]| -> Vec<ListItem> {
            colours
                .iter()
                .map(|colour| ListItem {
                    label: String::new(),
                    swatch: Some(*colour),
                })
                .collect()
        };
        for list in &mut self.colours {
            list.set_items(swatches(&feed.colours));
        }
        let decals: Vec<ListItem> = feed
            .decals
            .iter()
            .map(|_| ListItem {
                label: String::new(),
                swatch: None,
            })
            .collect();
        for list in &mut self.decals {
            list.set_items(decals.clone());
        }
        let held = feed.current.masks().first().and_then(|mask| {
            feed.patterns
                .iter()
                .position(|pattern| &pattern.mask == mask)
        });
        self.pattern.select(held.map(|item| (item, 0)));
        for (slot, placement) in feed.current.decals().iter().take(PAINT_SLOTS).enumerate() {
            let held = feed
                .decals
                .iter()
                .position(|decal| decal == placement.decal());
            self.decals[slot].select(held.map(|item| (item, 0)));
        }
        for list in &mut self.shades {
            list.set_items(Vec::new());
        }
        self.feed = Some(feed);
        self.shown = None;
        for slot in 0..PAINT_SLOTS {
            self.refresh_shades(slot);
        }
    }

    /// A colour pick refreshes its shade list (`E 2236(0, ..)`, then
    /// `initialize(MLA[R])`); the shade held stays when the new list has it.
    fn refresh_shades(&mut self, slot: usize) {
        let swatches = self
            .feed
            .as_ref()
            .zip(self.colours[slot].selected())
            .and_then(|(feed, (colour, _))| feed.shades.get(colour))
            .map(|shades| {
                shades
                    .iter()
                    .map(|shade| ListItem {
                        label: String::new(),
                        swatch: Some(*shade),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let held = self.shades[slot].selected();
        self.shades[slot].set_items(swatches);
        self.shades[slot].select(held);
    }

    /// A press at a logical point. `None` when no list took it; otherwise
    /// whether a choice changed.
    pub fn press_at(&mut self, x: i32, y: i32) -> Option<bool> {
        let count = 1 + self.colours.len() + self.shades.len() + self.decals.len();
        // An open list takes the press first.
        for index in 0..count {
            let press = {
                let list = self.nth_mut(index);
                if !list.is_open() {
                    continue;
                }
                list.press(x, y)
            };
            match press {
                ListPress::Picked { item, .. } => {
                    self.picked(index, item);
                    return Some(true);
                }
                ListPress::Closed => return Some(false),
                ListPress::Outside | ListPress::Opened => {}
            }
        }
        for index in 0..count {
            if self.nth_mut(index).press(x, y) == ListPress::Opened {
                return Some(false);
            }
        }
        None
    }

    fn nth_mut(&mut self, index: usize) -> &mut DropList {
        let colours = self.colours.len();
        let shades = self.shades.len();
        match index {
            0 => &mut self.pattern,
            i if i <= colours => &mut self.colours[i - 1],
            i if i <= colours + shades => &mut self.shades[i - 1 - colours],
            i => &mut self.decals[i - 1 - colours - shades],
        }
    }

    fn picked(&mut self, index: usize, item: usize) {
        if index == 0 {
            self.shown = Some(item);
        } else if index <= self.colours.len() {
            self.refresh_shades(index - 1);
        }
    }

    /// Moves the pointer; whether anything shown changed.
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) -> bool {
        let mut changed = false;
        for list in self.lists_mut() {
            changed |= list.hover_at(point);
        }
        changed
    }

    /// The paint the panel's pattern and decals make: the pattern is the
    /// blueprint's mask and each decal a placement over it. With no pattern
    /// there is no mask to place a decal over, so none is made.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] for a mask or decal that is not a
    /// `paint_mask` id, or the same decal in two slots.
    pub fn selection(&self) -> Result<PaintSelection, ConstructionSchemaError> {
        let Some(feed) = &self.feed else {
            return Ok(PaintSelection::default());
        };
        let choice = self.choice();
        let Some(mask) = choice
            .pattern
            .and_then(|item| feed.patterns.get(item))
            .map(|pattern| pattern.mask.clone())
        else {
            return Ok(PaintSelection::default());
        };
        let mut placements = Vec::new();
        for held in choice.decals.into_iter().flatten() {
            if let Some(decal) = feed.decals.get(held.0) {
                placements.push(DecalPlacement::try_new(decal.clone(), mask.clone())?);
            }
        }
        PaintSelection::try_new(vec![mask], placements)
    }

    /// The texts to draw: the titles and the pattern's name once one was
    /// picked or hovered.
    #[must_use]
    pub fn texts(&self) -> Vec<HubText> {
        common_texts(
            &self.common,
            self.current().map(|pattern| pattern.name.as_str()),
        )
    }

    pub(super) fn close(&mut self) {
        for list in self.lists_mut() {
            list.close();
        }
    }
}

// ------------------------------------------------------------------ edits ---

/// What a panel changed in the draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelEdit {
    /// The armor panel's fitments.
    Armor(Vec<ArmorFitment>),
    /// The paint panel's selection.
    Paint(PaintSelection),
    /// A choice the draft refused to take (the schema error's text).
    Refused(String),
}

/// Hands the panels' edits to the draft; returns the refusals' texts.
pub fn apply_edits(screen: &mut ConstructionScreen, edits: Vec<PanelEdit>) -> Vec<String> {
    let mut refused = Vec::new();
    for edit in edits {
        match edit {
            PanelEdit::Armor(fitments) => {
                if let Err(error) = screen.fit_armor(fitments) {
                    refused.push(format!("{error:?}"));
                }
            }
            PanelEdit::Paint(paint) => screen.set_paint(paint),
            PanelEdit::Refused(why) => refused.push(why),
        }
    }
    refused
}
