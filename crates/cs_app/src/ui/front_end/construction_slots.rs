//! The Guns and Hardpoints panels of the Plane Construction hub (task #2045,
//! part of #2036): the `[@Guns@]` and `[@HardPoints@]` sections of
//! `ASSETS/LAYOUT.CSV` with the panel scripts `ASSETS/SCRIPTS/GUNS.SCRIPT` and
//! `HARDPOINTS.SCRIPT` run over them, as the model a presenter draws and a
//! headless drive reads. The hub ([`super::construction_hub`]) owns both.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! * **Guns.** `GUNS.SCRIPT` builds four gun positions (`int AS[4]`,
//!   `object DS[4]`, `object ES[4]`, every loop `R < 4`), each a title text
//!   (`GN_T_GUNTITLE0..3`, no resource id: `E 2250` fills it) over one
//!   drop-down (`GN_D_GUN0..3`, 305 wide, 11 rows displayed, all absolute),
//!   three rules (`GN_P_LINE0..2`), the panel title (`IDS_GN_TITLE`), one name
//!   line (`GN_T_GUNNAME`) and a scrolling description (`GN_S_GUNDESC`).
//! * **Hardpoints.** `HARDPOINTS.SCRIPT` builds two points (`object DT[2]`,
//!   `int AT[2]`), a title text each (`IDS_HP_POINT1_TITLE`, `_POINT2_`) over a
//!   drop-down (`HP_D_POINT0..1`, 5 rows displayed), one rule, the panel title
//!   (`IDS_HP_TITLE`), a name line (`HP_T_HARDPOINTNAME`) and a description
//!   (`HP_S_HARDPOINTDESC`). **The hub has no rocket panel**: the eight
//!   rocket drop-downs (`object RKA[8]`) belong to `ORDINANCELAYOUT.SCRIPT`,
//!   the Instant Action loadout screen (section `OrdinanceLayout`), which
//!   reads `@globals@ZQ` for the pilot or the wingmate; it is not built here.
//! * **Messages.** Both scripts take the drop-down's `10015` (an entry chosen:
//!   `AS[BS] = sender.QG`, then the store call), `10013` (an entry rolled over:
//!   the store call with `sender.RM`, the highlighted row) and `10001` (the
//!   box's init: re-applies the held value), each followed by `mail(12001,
//!   @planeconstruction@)` to refresh the hub's costs and statistics. The
//!   guns' name line and description follow the slot that last sent a message
//!   (`BS`, found by `ES[BS] == sender`).
//! * **Guns' entries** (`E 2248`, handler `0x40be1b`, the list's text
//!   callback): eleven entries, indices 0 to 4 the five gun kinds named by
//!   strings `3310 + kind`, indices 5 to 9 the same kinds with the format
//!   string 506 (`0x1fa`) put in front (read as the *pair* form), index 10 the
//!   string `3310 + 5`, the empty position. The slot store is `E 2249`
//!   (handler `0x40bea2`): get answers `kind + 5` when bit `slot` of the byte
//!   `[0x64cbfc]` is set, 10 when the kind is 5 (empty); set takes an index,
//!   sets or clears that bit (index 5 to 9 sets it, 10 and 0 to 4 clear it) and
//!   stores the kind (`index - 5`, or 5 for index 10) at
//!   `[0x64cc00 + 4 * slot]`. The name line is string `3310 + kind` of the
//!   focused slot (`E 2220` mode 4).
//! * **Hardpoints' entries** (`E 2244`, handler `0x40b81d`): five entries,
//!   strings 1165 (index 0), 1168 (index 1) and a format of string 1169 over
//!   the index for the others. The store is `E 2245` (handler `0x40ad0f`):
//!   get/set of `[0x64cbac]` (point 0) or `[0x64cbb0]` (point 1). Entry 0 is
//!   read as the empty point, which is an inference.
//!
//! # What is *not* known, and so not claimed
//!
//! * **Which draft mount a position is.** `GunFitment` is keyed by a
//!   damage-graph mount and `OrdnanceFitment` by a hardpoint node; the engine
//!   keeps plain slot numbers. The caller supplies the mount of each slot
//!   ([`apply_slot_choice`]); this module invents none.
//! * **Which content id is entry `n`** and what a *pair* means for the
//!   draft's `positions`: the caller's list and the positions it passes.
//! * **The gun titles** (`E 2250`), **the hardpoint name line** (`E 2220` mode
//!   5) and **the descriptions** (`E 2226`, `E 2227`): their sources are
//!   unread, so those texts are not drawn.
//! * **The hardpoint entry strings' words** beyond the ids above, and the
//!   `ZTA` shared-items control the drop-downs use: the list is drawn as a
//!   stand-in box.
//! * **Message box behaviour, the statistics bars and the costs** the `12001`
//!   mail refreshes: #2051.

use std::fmt;

use cs_content::construction::ConstructionSchemaError;
use cs_content::construction::{GunFitment, OrdnanceFitment};
use cs_content::damage::DamageNodeKey;
use cs_types::content::{ContentId, Resolved};

use super::original::{OriginalLayout, Row, Section};
use super::previous_missions::{colour, split_tag};
use super::shell::ShellRect;
use super::widgets::{DropdownRow, WidgetError, dropdown_rows};
use crate::construction::ConstructionScreen;

/// Which of the two panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotKind {
    /// `GUNS.SCRIPT`, the `Guns` section.
    Guns,
    /// `HARDPOINTS.SCRIPT`, the `HardPoints` section.
    HardPoints,
}

impl SlotKind {
    /// The layout section.
    #[must_use]
    pub const fn section(self) -> &'static str {
        match self {
            Self::Guns => "Guns",
            Self::HardPoints => "HardPoints",
        }
    }

    /// How many positions the script builds (`R < 4`, `R < 2`): the same
    /// numbers `cs_content` measured on the other screens.
    #[must_use]
    pub const fn slots(self) -> usize {
        match self {
            Self::Guns => cs_content::construction::ORIGINAL_GUN_SLOTS as usize,
            Self::HardPoints => cs_content::construction::ORIGINAL_HARDPOINT_POINTS as usize,
        }
    }

    /// The panel's title row.
    #[must_use]
    pub const fn title_row(self) -> &'static str {
        match self {
            Self::Guns => "GN_T_TITLE",
            Self::HardPoints => "HP_T_TITLE",
        }
    }

    /// The name line the script fills at run time.
    #[must_use]
    pub const fn name_row(self) -> &'static str {
        match self {
            Self::Guns => "GN_T_GUNNAME",
            Self::HardPoints => "HP_T_HARDPOINTNAME",
        }
    }

    /// The scrolling description box.
    #[must_use]
    pub const fn description_row(self) -> &'static str {
        match self {
            Self::Guns => "GN_S_GUNDESC",
            Self::HardPoints => "HP_S_HARDPOINTDESC",
        }
    }

    fn slot_title_row(self, slot: usize) -> String {
        match self {
            Self::Guns => format!("GN_T_GUNTITLE{slot}"),
            Self::HardPoints => format!("HP_T_POINTTITLE{slot}"),
        }
    }

    fn dropdown_row(self, slot: usize) -> String {
        match self {
            Self::Guns => format!("GN_D_GUN{slot}"),
            Self::HardPoints => format!("HP_D_POINT{slot}"),
        }
    }
}

/// Why a panel could not be read from the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotError {
    /// The layout has no such section.
    NoSection(&'static str),
    /// A row the panel needs is not in the section.
    RowMissing(String),
    /// A column is not the number the row layout states.
    Column {
        /// The row.
        row: String,
    },
    /// The drop-down rows are unreadable.
    Dropdown(WidgetError),
}

impl fmt::Display for SlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection(name) => write!(f, "the layout has no {name} section"),
            Self::RowMissing(id) => write!(f, "the panel has no {id} row"),
            Self::Column { row } => write!(f, "{row}: a column is unreadable"),
            Self::Dropdown(error) => write!(f, "the drop-down is unreadable: {error:?}"),
        }
    }
}

impl std::error::Error for SlotError {}

/// One entry of a position's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotOption {
    /// The content the entry stands for; `None` is the empty position.
    pub id: Option<ContentId>,
    /// The entry's text.
    pub name: String,
}

/// A text of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotText {
    /// The row id.
    pub id: String,
    /// The finished text.
    pub text: String,
    /// The `[FONT]` tag's size.
    pub size: Option<u32>,
    /// The box.
    pub rect: ShellRect,
    /// `0xAARRGGBB`.
    pub color: u32,
    /// 0 left, 1 centre, 2 right.
    pub justify: u32,
}

/// What a press at a point did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotPress {
    /// The press is not the panel's.
    Nothing,
    /// A list opened or closed without a choice.
    Toggled,
    /// An entry was chosen (`mail 10015`).
    Chose {
        /// The position.
        slot: usize,
        /// The entry's content, `None` for the empty position.
        id: Option<ContentId>,
    },
}

/// The Guns or Hardpoints panel.
#[derive(Clone, Debug)]
pub struct SlotPanel {
    kind: SlotKind,
    title: SlotText,
    slot_titles: Vec<Option<SlotText>>,
    name: SlotText,
    description: ShellRect,
    dropdowns: Vec<DropdownRow>,
    options: Vec<SlotOption>,
    selected: Vec<Option<usize>>,
    focus: usize,
    open: Option<usize>,
    rolled: Option<usize>,
    dirty: bool,
}

impl SlotPanel {
    /// Reads the panel's rows. `name` answers an `IDS_*` name with the
    /// installation's text, `[FONT]` tag included.
    ///
    /// # Errors
    ///
    /// [`SlotError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        kind: SlotKind,
        name: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, SlotError> {
        let section = layout
            .section(kind.section())
            .ok_or(SlotError::NoSection(kind.section()))?;
        let title = text_row(section, kind.title_row(), Some(name))?;
        let name_line = text_row(section, kind.name_row(), None)?;
        let description = rect_row(section, kind.description_row())?;
        let rows = dropdown_rows(layout, kind.section()).map_err(SlotError::Dropdown)?;
        let mut dropdowns = Vec::new();
        let mut slot_titles = Vec::new();
        for slot in 0..kind.slots() {
            let id = kind.dropdown_row(slot);
            dropdowns.push(
                rows.iter()
                    .find(|row| row.id.eq_ignore_ascii_case(&id))
                    .cloned()
                    .ok_or(SlotError::RowMissing(id))?,
            );
            let title_id = kind.slot_title_row(slot);
            let text = text_row(section, &title_id, Some(name))?;
            // A gun position's title has no resource id (`E 2250` fills it,
            // unread): nothing to draw.
            slot_titles.push((!text.text.is_empty()).then_some(text));
        }
        Ok(Self {
            kind,
            title,
            slot_titles,
            name: name_line,
            description,
            dropdowns,
            options: Vec::new(),
            selected: vec![None; kind.slots()],
            focus: 0,
            open: None,
            rolled: None,
            dirty: true,
        })
    }

    /// Which panel this is.
    #[must_use]
    pub const fn kind(&self) -> SlotKind {
        self.kind
    }

    /// The closed box of a position.
    #[must_use]
    pub fn dropdown(&self, slot: usize) -> Option<&DropdownRow> {
        self.dropdowns.get(slot)
    }

    /// The scrolling description box. Nothing is drawn in it: its text comes
    /// from `E 2226`/`E 2227`, unread.
    #[must_use]
    pub const fn description_rect(&self) -> ShellRect {
        self.description
    }

    /// The entries every position's list shows.
    #[must_use]
    pub fn options(&self) -> &[SlotOption] {
        &self.options
    }

    /// The chosen entry of a position.
    #[must_use]
    pub fn selected(&self, slot: usize) -> Option<usize> {
        self.selected.get(slot).copied().flatten()
    }

    /// The position the name line follows (`BS`).
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// The position whose list is open.
    #[must_use]
    pub const fn open_slot(&self) -> Option<usize> {
        self.open
    }

    /// The entry the pointer is over while a list is open.
    #[must_use]
    pub const fn rolled(&self) -> Option<usize> {
        self.rolled
    }

    /// Whether the model changed since the last call; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Hands the panel its entries and what the draft holds in each position
    /// (an id not among the entries, and `None`, leave no choice unless the
    /// list has an empty entry for `None`).
    pub fn set_options(&mut self, options: Vec<SlotOption>, held: &[Option<ContentId>]) {
        self.options = options;
        self.open = None;
        self.rolled = None;
        self.follow(held);
        self.dirty = true;
    }

    /// Follows the draft: each position's choice is what the draft holds.
    pub fn follow(&mut self, held: &[Option<ContentId>]) {
        for slot in 0..self.selected.len() {
            let at = self
                .options
                .iter()
                .position(|option| option.id.as_ref() == held.get(slot).and_then(Option::as_ref));
            if at != self.selected[slot] {
                self.selected[slot] = at;
                self.dirty = true;
            }
        }
    }

    /// The rect of an open list's `index`-th row: below the box, one box
    /// height each.
    #[must_use]
    pub fn item_rect(&self, slot: usize, index: usize) -> Option<ShellRect> {
        let box_rect = self.dropdowns.get(slot)?.rect;
        Some(ShellRect {
            x: box_rect.x,
            y: box_rect.y + ((index as u32 + 1) * box_rect.height) as i32,
            width: box_rect.width,
            height: box_rect.height,
        })
    }

    /// The pointer moved to a logical point: while a list is open the entry
    /// under it is rolled over (`mail 10013`).
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) {
        let rolled = self.open.and_then(|slot| {
            let (x, y) = point?;
            (0..self.options.len()).find(|index| {
                self.item_rect(slot, *index)
                    .is_some_and(|r| r.contains(x, y))
            })
        });
        if rolled != self.rolled {
            self.rolled = rolled;
            self.dirty = true;
        }
    }

    /// A press at a logical point: opens or closes a list, or chooses the
    /// entry under the point. A press elsewhere closes an open list and is
    /// not the panel's.
    pub fn press_at(&mut self, x: i32, y: i32) -> SlotPress {
        if self.options.is_empty() {
            return SlotPress::Nothing;
        }
        if let Some(slot) = self.open {
            let picked = (0..self.options.len()).find(|index| {
                self.item_rect(slot, *index)
                    .is_some_and(|r| r.contains(x, y))
            });
            self.open = None;
            self.rolled = None;
            self.dirty = true;
            if let Some(index) = picked {
                self.selected[slot] = Some(index);
                self.focus = slot;
                return SlotPress::Chose {
                    slot,
                    id: self.options[index].id.clone(),
                };
            }
            return if self.dropdowns[slot].rect.contains(x, y) {
                SlotPress::Toggled
            } else {
                SlotPress::Nothing
            };
        }
        if let Some(slot) = self
            .dropdowns
            .iter()
            .position(|dropdown| dropdown.rect.contains(x, y))
        {
            self.open = Some(slot);
            self.focus = slot;
            self.dirty = true;
            return SlotPress::Toggled;
        }
        SlotPress::Nothing
    }

    /// Closes the open list; whether one was open.
    pub fn close_list(&mut self) -> bool {
        let was = self.open.is_some();
        if was {
            self.open = None;
            self.rolled = None;
            self.dirty = true;
        }
        was
    }

    /// The texts to draw: the panel title, each position's title when the
    /// layout names one, and for the guns the focused position's chosen
    /// entry on the name line (`E 2220` mode 4). The hardpoints' name line
    /// is unread and stays empty.
    #[must_use]
    pub fn texts(&self) -> Vec<SlotText> {
        let mut out = vec![self.title.clone()];
        out.extend(self.slot_titles.iter().flatten().cloned());
        if self.kind == SlotKind::Guns
            && let Some(option) = self
                .selected(self.focus)
                .and_then(|index| self.options.get(index))
        {
            out.push(SlotText {
                text: option.name.clone(),
                ..self.name.clone()
            });
        }
        out
    }
}

/// Why a choice could not be put into the draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotApplyError {
    /// The caller supplied no mount for the position.
    NoMount(usize),
    /// The draft refused the edit (wrong kind, repeated mount); unchanged.
    Schema(ConstructionSchemaError),
}

impl fmt::Display for SlotApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMount(slot) => write!(f, "position {slot} has no mount"),
            Self::Schema(error) => write!(f, "the draft refused the edit: {error:?}"),
        }
    }
}

impl std::error::Error for SlotApplyError {}

/// Applies a chosen entry to the draft. `mounts[slot]` is the damage-graph
/// node of the position (the caller's knowledge, see the module docs). A gun
/// keeps the `positions` of the fitment it replaces, or takes `positions`
/// when the mount was empty; `None` removes the fitment.
///
/// # Errors
///
/// [`SlotApplyError`]; the draft is unchanged.
pub fn apply_slot_choice(
    kind: SlotKind,
    slot: usize,
    id: Option<ContentId>,
    mounts: &[DamageNodeKey],
    positions: Resolved<u32>,
    screen: &mut ConstructionScreen,
) -> Result<(), SlotApplyError> {
    let mount = mounts.get(slot).ok_or(SlotApplyError::NoMount(slot))?;
    let blueprint = screen.export();
    match kind {
        SlotKind::Guns => {
            let mut guns: Vec<GunFitment> = Vec::new();
            for fitted in blueprint.guns() {
                if fitted.mount() != mount {
                    guns.push(fitted.clone());
                } else if let Some(id) = &id {
                    guns.push(
                        GunFitment::try_new(id.clone(), mount.clone(), fitted.positions().clone())
                            .map_err(SlotApplyError::Schema)?,
                    );
                }
            }
            if let Some(id) = id
                && !blueprint
                    .guns()
                    .iter()
                    .any(|fitted| fitted.mount() == mount)
            {
                guns.push(
                    GunFitment::try_new(id, mount.clone(), positions)
                        .map_err(SlotApplyError::Schema)?,
                );
            }
            screen.fit_guns(guns).map_err(SlotApplyError::Schema)
        }
        SlotKind::HardPoints => {
            let mut ordnance: Vec<OrdnanceFitment> = blueprint
                .ordnance()
                .iter()
                .filter(|fitted| fitted.hardpoint() != mount)
                .cloned()
                .collect();
            if let Some(id) = id {
                ordnance.push(
                    OrdnanceFitment::try_new(mount.clone(), id).map_err(SlotApplyError::Schema)?,
                );
            }
            screen
                .fit_ordnance(ordnance)
                .map_err(SlotApplyError::Schema)
        }
    }
}

/// What the draft holds in each position, for [`SlotPanel::set_options`] and
/// [`SlotPanel::follow`].
#[must_use]
pub fn held_in_positions(
    kind: SlotKind,
    mounts: &[DamageNodeKey],
    screen: &ConstructionScreen,
) -> Vec<Option<ContentId>> {
    let blueprint = screen.export();
    mounts
        .iter()
        .map(|mount| match kind {
            SlotKind::Guns => blueprint
                .guns()
                .iter()
                .find(|fitted| fitted.mount() == mount)
                .map(|fitted| fitted.gun().clone()),
            SlotKind::HardPoints => blueprint
                .ordnance()
                .iter()
                .find(|fitted| fitted.hardpoint() == mount)
                .map(|fitted| fitted.ordnance().clone()),
        })
        .collect()
}

type NameFn<'a> = &'a dyn Fn(&str) -> Option<String>;

fn find<'a>(section: &'a Section, id: &str) -> Result<&'a Row, SlotError> {
    section
        .other
        .iter()
        .find(|row| row.id.eq_ignore_ascii_case(id))
        .ok_or_else(|| SlotError::RowMissing(id.to_owned()))
}

fn rect_row(section: &Section, id: &str) -> Result<ShellRect, SlotError> {
    let row = find(section, id)?;
    let number = |index: usize| {
        row.fields
            .get(index)
            .and_then(|value| value.trim().parse::<i32>().ok())
            .ok_or_else(|| SlotError::Column { row: id.to_owned() })
    };
    // `S,Frame,Ground,Slider,Up,Down,X,Y,0,Width,Height,..`.
    Ok(ShellRect {
        x: number(5)?,
        y: number(6)?,
        width: number(8)?.max(0) as u32,
        height: number(9)?.max(0) as u32,
    })
}

fn text_row(section: &Section, id: &str, name: Option<NameFn<'_>>) -> Result<SlotText, SlotError> {
    let row = find(section, id)?;
    let bad = || SlotError::Column { row: id.to_owned() };
    let number = |index: usize| {
        row.fields
            .get(index)
            .and_then(|value| value.trim().parse::<i32>().ok())
    };
    let (Some(x), Some(y)) = (number(1), number(2)) else {
        return Err(bad());
    };
    let color = row
        .fields
        .get(6)
        .and_then(|value| colour(value))
        .ok_or_else(bad)?;
    let justify = row
        .fields
        .get(7)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .ok_or_else(bad)?;
    let (size, text) = match (name, row.fields.first().map(|field| field.trim())) {
        (Some(name), Some(res)) if !res.is_empty() => {
            let raw = name(res).ok_or_else(|| SlotError::RowMissing(res.to_owned()))?;
            let (size, text) = split_tag(&raw);
            (size, text.to_owned())
        }
        _ => (None, String::new()),
    };
    Ok(SlotText {
        id: id.to_ascii_uppercase(),
        text,
        size,
        rect: ShellRect {
            x,
            y,
            width: number(4).unwrap_or(0).max(0) as u32,
            height: number(5).unwrap_or(0).max(0) as u32,
        },
        color,
        justify,
    })
}
