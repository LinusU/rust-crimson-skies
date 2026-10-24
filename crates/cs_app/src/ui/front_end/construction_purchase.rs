//! The Plane Construction hub's purchase panel (task #2053, part of #2036): the
//! `[@Purchase@]` section of `ASSETS/LAYOUT.CSV` (*CONSTRUCTION TOTALS*) with
//! the panel script `ASSETS/SCRIPTS/PURCHASE.SCRIPT` and the name check of
//! `PLANECONSTRUCTION.SCRIPT`'s message `20001` run over it, as the model a
//! presenter draws and a headless drive reads. The hub
//! ([`super::construction_hub`]) owns the page; Ready (`ScriptToExe =
//! Purchase`) opens it.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! Engine image `CS_ENGINE_IMAGE`, dispatcher `0x4093a0`; the numbers are
//! looked up in the byte table `0x40f788` and the jump table `0x40f5b4`.
//!
//! * **Layout.** A title, the column heads (*Item*, *Weight*, *Cost*), a row
//!   title for each of the airframe, engine, armor, guns and hardpoints, a
//!   *Totals* row, a problems line and the *Purchase Now* button, all
//!   absolute. Three columns: `TEXTLEFT`/`TEXTLEFTI` for the name, `TEXTCENT`
//!   for the weight and `TEXTRIGHT` for the cost, both right-justified `T`
//!   rows 50 wide. The airframe and engine each have a one-line row (`T`
//!   rows with no resource id, filled at run time); the armor, guns and
//!   hardpoints are `A` text lists of three columns (`SJ` controls with 4, 4
//!   and 2 rows: `@globals@AR`).
//! * **Script.** `gui_create` fills the rows with `E 2251` (airframe), `E 2252`
//!   (engine), `E 2253..2255` (armor name, weight, cost), `E 2256..2258`
//!   (guns), `E 2259..2261` (hardpoints) and `E 2262` (totals), shows the
//!   armor rows only if `E 2253(-1)` counts a zone, and asks `E 2264` for the
//!   problems line; a non-zero answer (no problem) activates *Purchase Now*
//!   (`mail 10018`), a zero answer deactivates it (`mail 10000`). A press
//!   (`mail 10003`) in a campaign (`$$CV$$` and `$$TGA$$` clear) asks `E 2210`
//!   for a free record; `-1` shows message box 204 (the hangar limit) and
//!   keeps the screen. Otherwise `mail 20001` to the construction screen is
//!   the name check: an empty name shows message box 203 and puts the focus
//!   back on the name box; a name goes to `E 2215(1, name)`. Then `E 2263`
//!   commits, `E 2107` words message box 1 (string 703, *This %1!s! has been
//!   purchased and delivered to your hangar*), and after it `mail 12004` leaves
//!   the construction screen as Cancel does.
//! * **Rows** (every figure is `%d` of string `1175`, the totals' cost `$%d`
//!   of string `1178`, the totals' weight `%d` of string `1179`):
//!   * `E 2251` (handler `0x40af39`): the name is string `3000 + airframe`,
//!     the weight the airframe row's second word and the cost its first word
//!     (the 44-byte rows at `0x619bb0`).
//!   * `E 2252` (`0x40afd8`): engine 6 is no engine: string `1171` (*No Engine
//!     Selected*) with empty figures, which the script then colours red
//!     (`0xffff0000`). Otherwise the name is string `3100 + 6 * airframe +
//!     engine`, the weight the airframe's engine base weight (`0x619d9c`, 12
//!     byte rows) plus the engine's (`0x619e1c`), the cost `0x4057c0`.
//!   * `E 2253..2255` (`0x40b0e4`): the armor words `rec + 0x74, 0x78, 0x7c,
//!     0x80` are the nose, tail, left wing and right wing zones; row `n` is
//!     the `n`th non-zero one. Name: string `1191 + zone` over the units;
//!     weight and cost `4 * units` each (`lea 4*ecx`; `20 * units / 5`).
//!   * `E 2256..2258` (`0x40b1f3`): row `n` is the `n`th occupied gun slot;
//!     name `(1) ` or `(2) ` (string `506`, 2 when the slot is paired)
//!     followed by string `3310 + kind` (`0x40fe40`); weight `0x4055c0` and
//!     cost `0x405700` of that slot.
//!   * `E 2259..2261` (`0x40b2b4`): the count is how many of `rec + 0x34,
//!     0x38` are non-zero; row `0` is string `1176` (*Left Wing: %d*) over
//!     `rec + 0x34`, any other row string `1177` over `rec + 0x38`; weight
//!     `480 *` and cost `410 *` that count. The rows are not skipped when a
//!     word is zero: with only the right word set the one row reads the left
//!     word. The page transcribes that.
//!   * `E 2262` (`0x40b368`): the weight `0x405550`, the cost `0x405680`.
//! * **The weight** (`0x405550`): the airframe's weight, plus an engine's
//!   weights unless engine 6, plus every gun's weight, plus four times the
//!   armor words and 120 times the hardpoint words. **The cost**
//!   (`0x405680`): the airframe's cost, `0x4057c0`, every gun's cost, four
//!   times the armor words and 410 times the hardpoint words. `0x4057c0` is 0
//!   for engine 6, else the airframe's engine base cost (`0x619d98`) with,
//!   by engine 0 to 5, the offsets -425, 0, 425, 5, 430 and 855 (the jump
//!   table at `0x405804`). A gun slot's weight and cost are the gun row's
//!   (`0x619e68`, 28 bytes) column chosen by the airframe's gun-slot mask
//!   (`0x619bc8`, as in [`super::construction_stats`]) times 1, or 2 for a
//!   pair.
//! * **Problems** (`E 2264`, `0x40b418`): the head is string `1182` (*CAN'T
//!   PURCHASE:*); the items, in this order and each only if it holds, are
//!   string `1226` (*INSUFFICIENT FUNDS*: the cash `[0x64b788]` is below the
//!   cost), `1227` (*OVERWEIGHT*: the airframe's capacity, `0x619bb8`, is
//!   below the weight) and `1183` (*NO ENGINE*). Items are joined by the
//!   user's list separator (`GetLocaleInfo`, `LOCALE_SLIST`, stored at
//!   `0x647baa` by `0x4112b0`) and a space. With no item the line is empty
//!   and the answer is 1 (go), else 0. (The head is string `1254` and the
//!   funds test is skipped while `[0x647b68]` or `[0x647b6c]` is set, the
//!   multiplayer modes; they are not modelled.)
//! * **The commit** (`E 2210`, `0x4111f0`; `E 2263`, `0x40b56c`). `E 2210`
//!   answers the first record whose first word is 0, but only if more than five
//!   records are empty or of kind 2; else `-1`. `E 2263` rewrites the plane's
//!   per-gun and per-hardpoint words, takes the cost (`0x405680`) off the cash,
//!   copies the working record into the free one with the first word set to 1
//!   and selects it (`[0x64b67c]`).
//!
//! # What is *not* known, and so not claimed
//!
//! * **The list separator** is the machine's; the page takes it as a
//!   parameter ([`DEFAULT_SEPARATOR`], the `en-US` value).
//! * **`mail 10021`**, the edit control's refusal check on a name, which the
//!   name check asks, is unread (`CTL.SCRIPT`'s handler depends on settings of
//!   the control this task does not read): the page accepts any non-empty
//!   name, as the Plane Name dialog's model does.
//! * **What `E 2263` writes** besides the cash and the record's first word:
//!   the words it sets at `rec + 0x98..` and `0xa8..` (the guns' and the
//!   hardpoints' state) and `0x406840` are transcribed neither in meaning nor
//!   in effect; the commit is [`commit_purchase`]'s, over the content model.
//! * **The multiplayer / Instant Action export** (`$$CV$$`, `$$TGA$$`, string
//!   1139 on the button): not modelled.
//! * **The text lists' pitch.** The `SJ` control draws its rows at a pitch
//!   the script does not state; the page uses the row's height.
//! * **The units** (pounds, dollars) of the figures beyond the strings'.
//! * **Where the plane and the cash come from in a real run** is #2052's feed.

use std::fmt;

use cs_sim::campaign::CampaignState;
use cs_sim::economy::CommitReceipt;

use super::construction_hub::{HubError, HubText, TabCaption};
use super::construction_stats::{AIRFRAMES, GUN_SLOTS, NO_ENGINE, NO_GUN, PlaneRecord};
use super::original::{OriginalLayout, Row, Section};
use super::previous_missions::{colour, split_tag};
use super::shell::ShellRect;
use crate::construction::{ConstructionContext, ConstructionError, ConstructionScreen};

/// The layout section of the panel.
pub const PURCHASE_SECTION: &str = "Purchase";
/// *Purchase Now*.
pub const PURCHASE_BUTTON: &str = "PUR_B_PURCHASE";
/// The problems line.
pub const PROBLEMS_ROW: &str = "PUR_T_PROBLEMS";
/// The list separator of an `en-US` machine (`LOCALE_SLIST`).
pub const DEFAULT_SEPARATOR: &str = ",";

/// Message box 203: the plane needs a name (the hub's edit box is empty).
pub const NAME_REQUIRED_STRING: u32 = 203;
/// Message box 204: the hangar is full (`E 2211`).
pub const HANGAR_FULL_STRING: u32 = 204;
/// The purchase box (`E 2107`): *This %1!s! has been purchased and delivered
/// to your hangar.*
pub const PURCHASED_STRING: u32 = 703;
/// The head of the problems line.
pub const PROBLEMS_HEAD_STRING: u32 = 1182;
/// *INSUFFICIENT FUNDS*.
pub const FUNDS_STRING: u32 = 1226;
/// *OVERWEIGHT*.
pub const OVERWEIGHT_STRING: u32 = 1227;
/// *NO ENGINE*.
pub const NO_ENGINE_STRING: u32 = 1183;
/// *No Engine Selected*, the engine row's name without an engine.
pub const NO_ENGINE_NAME_STRING: u32 = 1171;
/// The airframe's long name is string `3000 + airframe`.
pub const AIRFRAME_NAME_BASE: u32 = 3000;
/// The airframe's short name is string `3020 + airframe` (box 703).
pub const AIRFRAME_SHORT_BASE: u32 = 3020;
/// An engine's name is string `3100 + 6 * airframe + engine`.
pub const ENGINE_NAME_BASE: u32 = 3100;
/// The first armor zone's string (*Nose: %1!d! units*), then tail, left wing,
/// right wing.
pub const ARMOR_NAME_BASE: u32 = 1191;
/// A gun's name is string `3310 + kind`.
pub const GUN_NAME_BASE: u32 = 3310;
/// *(%1!d!) *, the gun row's count.
pub const GUN_COUNT_STRING: u32 = 506;
/// *Left Wing: %1!d!*; the right wing is the next string.
pub const HARDPOINT_NAME_BASE: u32 = 1176;
/// The totals' cost, *$%1!d!*.
pub const TOTAL_COST_STRING: u32 = 1178;
/// The red the engine row turns without an engine.
pub const NO_ENGINE_COLOUR: u32 = 0xFFFF_0000;

/// The 44-byte airframe rows at `0x619bb0`: cost, weight, capacity (words 0
/// to 2).
const AIRFRAME_TABLE: [[i32; 3]; AIRFRAMES] = [
    [6800, 1400, 4160],
    [4548, 3255, 9525],
    [1870, 5460, 15760],
    [5568, 2415, 6545],
    [3783, 3885, 11015],
    [4250, 3500, 10100],
    [2763, 4725, 14035],
    [5015, 2870, 7610],
    [2890, 4620, 13780],
    [5228, 2695, 7205],
    [2423, 5005, 14675],
];
/// The 12-byte rows at `0x619d98`: the airframe's engine base cost and base
/// weight (words 0 and 1).
const ENGINE_BASE: [[i32; 2]; AIRFRAMES] = [
    [850, 1000],
    [1700, 2000],
    [2550, 3000],
    [850, 1000],
    [1700, 2000],
    [1700, 2000],
    [2550, 3000],
    [850, 1000],
    [2550, 3000],
    [850, 1000],
    [2550, 3000],
];
/// The engines' weights at `0x619e1c` (engine 0 to 5).
const ENGINE_WEIGHT: [i32; 6] = [-500, 0, 500, 0, 500, 1000];
/// What `0x4057c0` adds to the base cost by engine 0 to 5 (the jump table at
/// `0x405804` and the immediates `0x1ae`, `0x1a9`).
const ENGINE_COST: [i32; 6] = [-425, 0, 425, 5, 430, 855];
/// The gun rows at `0x619e68`: the cost columns (`+0`, `+4`) and the weight
/// columns (`+8`, `+0xc`) by kind 0 to 4.
const GUN_COST: [[i32; 2]; 5] = [[240, 440], [320, 530], [410, 610], [490, 700], [580, 780]];
const GUN_WEIGHT: [[i32; 2]; 5] = [[280, 520], [380, 620], [480, 720], [580, 820], [680, 920]];
/// Armor: `4` per unit, weight and cost alike.
const ARMOR_UNIT: i32 = 4;
/// A hardpoint weighs 480 (`0x1e0`) and costs 410 (`0x1e0 - 0x46`).
const HARDPOINT_WEIGHT: i32 = 480;
const HARDPOINT_COST: i32 = 410;
/// A record is free for `E 2210` when this many are empty or kind 2 (the
/// answer needs more than [`FREE_RESERVE`]).
pub const FREE_RESERVE: usize = 5;

/// Why a figure could not be computed: the tables have no row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PurchaseError {
    /// The airframe has no row.
    Airframe(u32),
    /// The engine has no row.
    Engine(u32),
    /// The gun kind has no row.
    Gun(u32),
}

impl fmt::Display for PurchaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Airframe(n) => write!(f, "airframe {n} has no row in the purchase tables"),
            Self::Engine(n) => write!(f, "engine {n} has no row in the purchase tables"),
            Self::Gun(n) => write!(f, "gun kind {n} has no row in the purchase tables"),
        }
    }
}

impl std::error::Error for PurchaseError {}

fn airframe_row(record: &PlaneRecord) -> Result<usize, PurchaseError> {
    let index = record.airframe as usize;
    if index < AIRFRAMES {
        Ok(index)
    } else {
        Err(PurchaseError::Airframe(record.airframe))
    }
}

fn engine_row(record: &PlaneRecord) -> Result<Option<usize>, PurchaseError> {
    if record.engine == NO_ENGINE {
        return Ok(None);
    }
    let index = record.engine as usize;
    if index < ENGINE_WEIGHT.len() {
        Ok(Some(index))
    } else {
        Err(PurchaseError::Engine(record.engine))
    }
}

/// The airframe's weight capacity (`0x619bb8`).
///
/// # Errors
///
/// [`PurchaseError::Airframe`] for an airframe without a row.
pub fn capacity(record: &PlaneRecord) -> Result<i32, PurchaseError> {
    Ok(AIRFRAME_TABLE[airframe_row(record)?][2])
}

/// `0x4057c0`: the engine's cost, 0 without one.
///
/// # Errors
///
/// [`PurchaseError`] for an airframe or engine without a row.
pub fn engine_cost(record: &PlaneRecord) -> Result<i32, PurchaseError> {
    let af = airframe_row(record)?;
    Ok(engine_row(record)?.map_or(0, |engine| ENGINE_BASE[af][0] + ENGINE_COST[engine]))
}

/// The airframe's engine base weight plus the engine's, 0 without one.
fn engine_weight(record: &PlaneRecord) -> Result<i32, PurchaseError> {
    let af = airframe_row(record)?;
    Ok(engine_row(record)?.map_or(0, |engine| ENGINE_BASE[af][1] + ENGINE_WEIGHT[engine]))
}

/// The occupied gun slots, in slot order: `(slot, kind, multiplier)`.
fn occupied_guns(record: &PlaneRecord) -> Vec<(usize, usize, i32)> {
    record
        .guns
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind != NO_GUN)
        .map(|(slot, kind)| {
            (
                slot,
                *kind as usize,
                1 + i32::from(record.gun_pairs >> slot & 1),
            )
        })
        .collect()
}

/// One gun slot's figure from `table` (`0x4055c0`, `0x405700` for one slot).
fn gun_figure(
    record: &PlaneRecord,
    table: &[[i32; 2]; 5],
    (slot, kind, multiplier): (usize, usize, i32),
) -> Result<i32, PurchaseError> {
    let af = airframe_row(record)?;
    let row = table.get(kind).ok_or(PurchaseError::Gun(kind as u32))?;
    let column = usize::from(GUN_SLOTS[af] & (1 << slot) != 0);
    Ok(row[column] * multiplier)
}

fn guns_figure(record: &PlaneRecord, table: &[[i32; 2]; 5]) -> Result<i32, PurchaseError> {
    let mut total = 0;
    for gun in occupied_guns(record) {
        total += gun_figure(record, table, gun)?;
    }
    Ok(total)
}

/// The plane's weight (`0x405550`).
///
/// # Errors
///
/// [`PurchaseError`] for an airframe, engine or gun kind without a row.
pub fn plane_weight(record: &PlaneRecord) -> Result<i32, PurchaseError> {
    let af = airframe_row(record)?;
    let hardpoints: i32 = record.hardpoints.iter().sum();
    let armor: i32 = record.armor.iter().sum();
    Ok(AIRFRAME_TABLE[af][1]
        + engine_weight(record)?
        + guns_figure(record, &GUN_WEIGHT)?
        + ARMOR_UNIT * (armor + 120 * hardpoints))
}

/// The plane's cost (`0x405680`).
///
/// # Errors
///
/// [`PurchaseError`] for an airframe, engine or gun kind without a row.
pub fn plane_value(record: &PlaneRecord) -> Result<i32, PurchaseError> {
    let af = airframe_row(record)?;
    let hardpoints: i32 = record.hardpoints.iter().sum();
    let armor: i32 = record.armor.iter().sum();
    Ok(AIRFRAME_TABLE[af][0]
        + engine_cost(record)?
        + guns_figure(record, &GUN_COST)?
        + ARMOR_UNIT * armor
        + HARDPOINT_COST * hardpoints)
}

/// What stops a purchase (`E 2264`), in the order the line lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The cash is below the cost.
    InsufficientFunds,
    /// The weight is above the airframe's capacity.
    Overweight,
    /// The engine is 6.
    NoEngine,
}

/// The problems of `record` with `cash` on hand (`E 2264`).
///
/// # Errors
///
/// [`PurchaseError`] for a record the tables do not cover.
pub fn problems(record: &PlaneRecord, cash: i64) -> Result<Vec<Problem>, PurchaseError> {
    let mut out = Vec::new();
    if cash < i64::from(plane_value(record)?) {
        out.push(Problem::InsufficientFunds);
    }
    if capacity(record)? < plane_weight(record)? {
        out.push(Problem::Overweight);
    }
    if record.engine == NO_ENGINE {
        out.push(Problem::NoEngine);
    }
    Ok(out)
}

/// The armor zones that hold units, in zone order: `(zone, units)`.
fn armor_rows(record: &PlaneRecord) -> Vec<(usize, i32)> {
    record
        .armor
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, units)| *units != 0)
        .collect()
}

/// How many hardpoint rows there are (`E 2259(-1)`): the non-zero words.
fn hardpoint_count(record: &PlaneRecord) -> usize {
    record.hardpoints.iter().filter(|n| **n != 0).count()
}

/// What the page is handed: the plane being built and the cash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PurchaseFeed {
    /// The working plane's record.
    pub record: PlaneRecord,
    /// The cash on hand (`[0x64b788]`).
    pub cash: i64,
}

/// A row of the page: its place and colour.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    id: String,
    rect: ShellRect,
    color: u32,
    justify: u32,
    size: Option<u32>,
}

/// A fixed `T` row with its string.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fixed {
    slot: Slot,
    text: String,
}

/// A list's three columns: name, weight, cost.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Columns {
    name: Slot,
    weight: Slot,
    cost: Slot,
}

/// The finished lines of one list or row.
type Line = [String; 3];

#[derive(Clone, Debug, Default)]
struct Strings {
    head: Option<String>,
    funds: Option<String>,
    overweight: Option<String>,
    no_engine: Option<String>,
    no_engine_name: Option<String>,
    armor: Vec<Option<String>>,
    hardpoints: Vec<Option<String>>,
    gun_count: Option<String>,
    gun_names: Vec<Option<String>>,
    airframes: Vec<Option<String>>,
    airframe_short: Vec<Option<String>>,
    engines: Vec<Option<String>>,
    total_cost: Option<String>,
    required: Option<String>,
    full: Option<String>,
    purchased: Option<String>,
}

/// Why the panel could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurchasePageError {
    /// The layout has no `Purchase` section.
    NoSection,
    /// A row the script needs is missing.
    RowMissing(&'static str),
    /// A row's column cannot be read.
    Hub(HubError),
}

impl fmt::Display for PurchasePageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no [@{PURCHASE_SECTION}@] section"),
            Self::RowMissing(row) => write!(f, "the Purchase section has no {row} row"),
            Self::Hub(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PurchasePageError {}

impl From<HubError> for PurchasePageError {
    fn from(error: HubError) -> Self {
        Self::Hub(error)
    }
}

/// What a press of *Purchase Now* did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurchaseOutcome {
    /// Nothing happened: the button is inactive, or no plane was fed.
    Nothing,
    /// The hangar has no free record (`E 2210` answered `-1`): message box 204,
    /// the screen stays.
    HangarFull {
        /// The box text (string 204).
        text: Option<String>,
    },
    /// The name box is empty: message box 203, the focus goes back to the box.
    NameRequired {
        /// The box text (string 203).
        text: Option<String>,
    },
    /// The purchase may run: the caller commits it ([`commit_purchase`]),
    /// stores the plane in `slot` and calls [`PurchasePage::purchased`].
    Commit {
        /// The record the plane goes into (`E 2210`).
        slot: usize,
        /// The name the plane is stored under (`E 2215(1, name)`).
        name: String,
        /// The cost taken off the cash.
        value: i32,
    },
}

/// The purchase panel's page.
#[derive(Clone, Debug)]
pub struct PurchasePage {
    fixed: Vec<Fixed>,
    airframe: Columns,
    engine: Columns,
    armor: Columns,
    guns: Columns,
    hardpoints: Columns,
    totals: (Slot, Slot),
    problems: Slot,
    button: Option<TabCaption>,
    strings: Strings,
    separator: String,
    feed: Option<PurchaseFeed>,
}

fn slot_of(row: &Row, first: usize) -> Result<Slot, HubError> {
    let number = |index: usize| {
        row.fields
            .get(index)
            .and_then(|value| value.trim().parse::<i32>().ok())
    };
    let (Some(x), Some(y)) = (number(first), number(first + 1)) else {
        return Err(HubError::Column {
            row: row.id.clone(),
            column: "X/Y",
        });
    };
    let (Some(color), Some(justify)) = (
        row.fields.get(first + 5).and_then(|v| colour(v)),
        row.fields
            .get(first + 6)
            .and_then(|v| v.trim().parse::<u32>().ok()),
    ) else {
        return Err(HubError::Column {
            row: row.id.clone(),
            column: "Color/Justify",
        });
    };
    Ok(Slot {
        id: row.id.to_ascii_uppercase(),
        rect: ShellRect {
            x,
            y,
            width: number(first + 3).unwrap_or(0).max(0) as u32,
            height: number(first + 4).unwrap_or(0).max(0) as u32,
        },
        color,
        justify,
        size: None,
    })
}

fn find<'a>(section: &'a Section, id: &'static str) -> Result<&'a Row, PurchasePageError> {
    section
        .other
        .iter()
        .find(|row| row.id.eq_ignore_ascii_case(id))
        .ok_or(PurchasePageError::RowMissing(id))
}

/// A `T` row: `Resource, X, Y, Z, Width, Height, Color, Justify`.
fn text_slot(section: &Section, id: &'static str) -> Result<Slot, PurchasePageError> {
    Ok(slot_of(find(section, id)?, 1)?)
}

/// An `A` row: `X, Y, Z, Width, Height, Color, Justify, ..`.
fn list_slot(section: &Section, id: &'static str) -> Result<Slot, PurchasePageError> {
    Ok(slot_of(find(section, id)?, 0)?)
}

impl PurchasePage {
    /// Reads the page from the layout; `name` resolves an `IDS_*` name and
    /// `id` a string-table id, each with its `[FONT]` tag.
    ///
    /// # Errors
    ///
    /// [`PurchasePageError`] for a missing section or row or an unreadable
    /// column.
    pub fn from_layout(
        layout: &OriginalLayout,
        name: &dyn Fn(&str) -> Option<String>,
        id: &dyn Fn(u32) -> Option<String>,
    ) -> Result<Self, PurchasePageError> {
        let section = layout
            .section(PURCHASE_SECTION)
            .ok_or(PurchasePageError::NoSection)?;
        let mut fixed = Vec::new();
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
                continue;
            };
            let Some(raw) = name(res) else { continue };
            let (size, text) = split_tag(&raw);
            let mut slot = slot_of(row, 1)?;
            slot.size = size;
            fixed.push(Fixed {
                slot,
                text: text.to_owned(),
            });
        }
        let columns = |name: &'static str,
                       weight: &'static str,
                       cost: &'static str,
                       list: bool|
         -> Result<Columns, PurchasePageError> {
            let read = if list { list_slot } else { text_slot };
            Ok(Columns {
                name: read(section, name)?,
                weight: read(section, weight)?,
                cost: read(section, cost)?,
            })
        };
        let plain = |string: u32| id(string).map(|raw| split_tag(&raw).1.to_owned());
        let strings = Strings {
            head: id(PROBLEMS_HEAD_STRING),
            funds: plain(FUNDS_STRING),
            overweight: plain(OVERWEIGHT_STRING),
            no_engine: plain(NO_ENGINE_STRING),
            no_engine_name: plain(NO_ENGINE_NAME_STRING),
            armor: (0..4).map(|n| plain(ARMOR_NAME_BASE + n)).collect(),
            hardpoints: (0..2).map(|n| plain(HARDPOINT_NAME_BASE + n)).collect(),
            gun_count: plain(GUN_COUNT_STRING),
            gun_names: (0..5).map(|n| plain(GUN_NAME_BASE + n)).collect(),
            airframes: (0..AIRFRAMES as u32)
                .map(|n| plain(AIRFRAME_NAME_BASE + n))
                .collect(),
            airframe_short: (0..AIRFRAMES as u32)
                .map(|n| plain(AIRFRAME_SHORT_BASE + n))
                .collect(),
            engines: (0..(AIRFRAMES as u32 * 6))
                .map(|n| plain(ENGINE_NAME_BASE + n))
                .collect(),
            total_cost: plain(TOTAL_COST_STRING),
            required: plain(NAME_REQUIRED_STRING),
            full: plain(HANGAR_FULL_STRING),
            purchased: plain(PURCHASED_STRING),
        };
        let button = section
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(PURCHASE_BUTTON))
            .and_then(|button| {
                let res = button
                    .fields
                    .iter()
                    .find(|field| field.trim().starts_with("IDS_"))?;
                let raw = name(res.trim())?;
                let (size, text) = split_tag(&raw);
                let text = text.to_owned();
                let mut colors = [0xFF00_0000; 4];
                for (slot, index) in colors.iter_mut().zip(14..18) {
                    *slot = colour(button.fields.get(index)?)?;
                }
                Some(TabCaption {
                    button: button.id.to_ascii_uppercase(),
                    text,
                    size,
                    colors,
                })
            });
        Ok(Self {
            fixed,
            airframe: columns(
                "PUR_T_AIRFRAME",
                "PUR_T_AIRFRAMEWEIGHT",
                "PUR_T_AIRFRAMECOST",
                false,
            )?,
            engine: columns(
                "PUR_T_ENGINE",
                "PUR_T_ENGINEWEIGHT",
                "PUR_T_ENGINECOST",
                false,
            )?,
            armor: columns(
                "PUR_T_ARMORLIST",
                "PUR_T_ARMORWEIGHT",
                "PUR_T_ARMORCOST",
                true,
            )?,
            guns: columns("PUR_T_GUNLIST", "PUR_T_GUNWEIGHT", "PUR_T_GUNCOST", true)?,
            hardpoints: columns(
                "PUR_T_HARDPOINTLIST",
                "PUR_T_HARDPOINTWEIGHT",
                "PUR_T_HARDPOINTCOST",
                true,
            )?,
            totals: (
                text_slot(section, "PUR_T_TOTALSWEIGHT")?,
                text_slot(section, "PUR_T_TOTALSCOST")?,
            ),
            problems: text_slot(section, PROBLEMS_ROW)?,
            button,
            strings,
            separator: DEFAULT_SEPARATOR.to_owned(),
            feed: None,
        })
    }

    /// Uses the machine's list separator for the problems line.
    pub fn set_separator(&mut self, separator: &str) {
        separator.clone_into(&mut self.separator);
    }

    /// Hands the page the plane and the cash. `Err` when the tables have no
    /// row for the plane: the page then carries no figures.
    ///
    /// # Errors
    ///
    /// [`PurchaseError`] for a record the tables do not cover.
    pub fn set_feed(&mut self, feed: PurchaseFeed) -> Result<(), PurchaseError> {
        // Every figure is computed from the record: refuse one the tables do
        // not cover before the page draws anything of it.
        plane_value(&feed.record)?;
        plane_weight(&feed.record)?;
        capacity(&feed.record)?;
        self.feed = Some(feed);
        Ok(())
    }

    /// Takes the plane away (nothing is drawn but the fixed texts).
    pub fn clear_feed(&mut self) {
        self.feed = None;
    }

    /// The plane the page shows, when one was fed.
    #[must_use]
    pub const fn feed(&self) -> Option<&PurchaseFeed> {
        self.feed.as_ref()
    }

    /// The *Purchase Now* caption and its colours.
    #[must_use]
    pub const fn button(&self) -> Option<&TabCaption> {
        self.button.as_ref()
    }

    /// The problems that stop the purchase; none without a plane.
    #[must_use]
    pub fn problems(&self) -> Vec<Problem> {
        self.feed
            .and_then(|feed| problems(&feed.record, feed.cash).ok())
            .unwrap_or_default()
    }

    /// Whether *Purchase Now* is active: a plane is fed and `E 2264` found no
    /// problem. Without a plane the button stays inactive, since the script
    /// would have asked the engine about a record nobody supplied.
    #[must_use]
    pub fn button_enabled(&self) -> bool {
        self.feed.is_some() && self.problems().is_empty()
    }

    /// The problems line (`E 2264`), empty with none.
    #[must_use]
    pub fn problems_text(&self) -> String {
        let items: Vec<&str> = self
            .problems()
            .into_iter()
            .filter_map(|problem| match problem {
                Problem::InsufficientFunds => self.strings.funds.as_deref(),
                Problem::Overweight => self.strings.overweight.as_deref(),
                Problem::NoEngine => self.strings.no_engine.as_deref(),
            })
            .collect();
        if items.is_empty() {
            return String::new();
        }
        let head = self
            .strings
            .head
            .as_deref()
            .map_or("", |raw| split_tag(raw).1);
        format!("{head}{}", items.join(&format!("{} ", self.separator)))
    }

    /// The airframe row (`E 2251`): name, weight, cost.
    #[must_use]
    pub fn airframe_line(&self) -> Option<Line> {
        let record = &self.feed?.record;
        let af = airframe_row(record).ok()?;
        Some([
            self.strings.airframes.get(af)?.clone()?,
            AIRFRAME_TABLE[af][1].to_string(),
            AIRFRAME_TABLE[af][0].to_string(),
        ])
    }

    /// The engine row (`E 2252`); empty figures without an engine.
    #[must_use]
    pub fn engine_line(&self) -> Option<Line> {
        let record = &self.feed?.record;
        let af = airframe_row(record).ok()?;
        match engine_row(record).ok()? {
            None => Some([
                self.strings.no_engine_name.clone()?,
                String::new(),
                String::new(),
            ]),
            Some(engine) => Some([
                self.strings.engines.get(af * 6 + engine)?.clone()?,
                engine_weight(record).ok()?.to_string(),
                engine_cost(record).ok()?.to_string(),
            ]),
        }
    }

    /// The armor rows (`E 2253..2255`).
    #[must_use]
    pub fn armor_lines(&self) -> Vec<Line> {
        let Some(feed) = &self.feed else {
            return Vec::new();
        };
        armor_rows(&feed.record)
            .into_iter()
            .filter_map(|(zone, units)| {
                let template = self.strings.armor.get(zone)?.as_deref()?;
                Some([
                    fill(template, units),
                    (ARMOR_UNIT * units).to_string(),
                    (ARMOR_UNIT * units).to_string(),
                ])
            })
            .collect()
    }

    /// The gun rows (`E 2256..2258`).
    #[must_use]
    pub fn gun_lines(&self) -> Vec<Line> {
        let Some(feed) = &self.feed else {
            return Vec::new();
        };
        occupied_guns(&feed.record)
            .into_iter()
            .filter_map(|gun| {
                let (_, kind, multiplier) = gun;
                let count = fill(self.strings.gun_count.as_deref()?, multiplier);
                let name = self.strings.gun_names.get(kind)?.as_deref()?;
                Some([
                    format!("{count}{name}"),
                    gun_figure(&feed.record, &GUN_WEIGHT, gun).ok()?.to_string(),
                    gun_figure(&feed.record, &GUN_COST, gun).ok()?.to_string(),
                ])
            })
            .collect()
    }

    /// The hardpoint rows (`E 2259..2261`): row 0 reads the left word, any
    /// other row the right word, however many words are set.
    #[must_use]
    pub fn hardpoint_lines(&self) -> Vec<Line> {
        let Some(feed) = &self.feed else {
            return Vec::new();
        };
        (0..hardpoint_count(&feed.record))
            .filter_map(|row| {
                let count = feed.record.hardpoints[usize::from(row != 0)];
                let template = self
                    .strings
                    .hardpoints
                    .get(usize::from(row != 0))?
                    .as_deref()?;
                Some([
                    fill(template, count),
                    (HARDPOINT_WEIGHT * count).to_string(),
                    (HARDPOINT_COST * count).to_string(),
                ])
            })
            .collect()
    }

    /// The totals row (`E 2262`): weight and cost.
    #[must_use]
    pub fn totals_line(&self) -> Option<[String; 2]> {
        let record = &self.feed?.record;
        let cost = plane_value(record).ok()?;
        Some([
            plane_weight(record).ok()?.to_string(),
            fill(self.strings.total_cost.as_deref()?, cost),
        ])
    }

    /// Every text the page draws now: the fixed strings, and with a plane
    /// the rows, the totals and the problems line.
    #[must_use]
    pub fn texts(&self) -> Vec<HubText> {
        let mut out: Vec<HubText> = self
            .fixed
            .iter()
            .map(|fixed| hub_text(&fixed.slot, &fixed.text, fixed.slot.color))
            .collect();
        let columns = |out: &mut Vec<HubText>, columns: &Columns, line: &Line, row: usize, red| {
            for (slot, text) in [&columns.name, &columns.weight, &columns.cost]
                .into_iter()
                .zip(line)
            {
                let mut slot = slot.clone();
                // A list's rows stack at the row's height.
                slot.rect.y += (row as u32 * slot.rect.height) as i32;
                let color = if red { NO_ENGINE_COLOUR } else { slot.color };
                out.push(hub_text(&slot, text, color));
            }
        };
        if self.feed.is_none() {
            return out;
        }
        if let Some(line) = self.airframe_line() {
            columns(&mut out, &self.airframe, &line, 0, false);
        }
        if let Some(line) = self.engine_line() {
            // `equal$(NQA.BC, "")`: an empty weight turns the name red.
            columns(&mut out, &self.engine, &line, 0, line[1].is_empty());
        }
        for (list, lines) in [
            (&self.armor, self.armor_lines()),
            (&self.guns, self.gun_lines()),
            (&self.hardpoints, self.hardpoint_lines()),
        ] {
            for (row, line) in lines.iter().enumerate() {
                columns(&mut out, list, line, row, false);
            }
        }
        if let Some([weight, cost]) = self.totals_line() {
            out.push(hub_text(&self.totals.0, &weight, self.totals.0.color));
            out.push(hub_text(&self.totals.1, &cost, self.totals.1.color));
        }
        let problems = self.problems_text();
        if !problems.is_empty() {
            let mut slot = self.problems.clone();
            slot.size = self
                .strings
                .head
                .as_deref()
                .and_then(|raw| split_tag(raw).0);
            out.push(hub_text(&slot, &problems, slot.color));
        }
        out
    }

    /// A press of *Purchase Now* (`mail 10003` from the button). `name` is
    /// the hub's name box, `free_slot` the record `E 2210` found
    /// ([`super::construction_hangar::HangarPage::free_slot`]).
    pub fn press_purchase(&self, name: &str, free_slot: Option<usize>) -> PurchaseOutcome {
        let Some(feed) = &self.feed else {
            return PurchaseOutcome::Nothing;
        };
        if !self.button_enabled() {
            return PurchaseOutcome::Nothing;
        }
        let Some(slot) = free_slot else {
            return PurchaseOutcome::HangarFull {
                text: self.strings.full.clone(),
            };
        };
        if name.is_empty() {
            return PurchaseOutcome::NameRequired {
                text: self.strings.required.clone(),
            };
        }
        match plane_value(&feed.record) {
            Ok(value) => PurchaseOutcome::Commit {
                slot,
                name: name.to_owned(),
                value,
            },
            Err(_) => PurchaseOutcome::Nothing,
        }
    }

    /// The box that follows a commit (`E 2107`, string 703 over the plane's
    /// short name).
    #[must_use]
    pub fn purchased(&self) -> Option<String> {
        let feed = self.feed.as_ref()?;
        let short = self
            .strings
            .airframe_short
            .get(feed.record.airframe as usize)?
            .as_deref()?;
        Some(self.strings.purchased.as_deref()?.replace("%1!s!", short))
    }
}

fn fill(template: &str, value: i32) -> String {
    template.replace("%1!d!", &value.to_string())
}

fn hub_text(slot: &Slot, text: &str, color: u32) -> HubText {
    HubText {
        id: slot.id.clone(),
        text: text.to_owned(),
        size: slot.size,
        rect: slot.rect,
        color,
        justify: slot.justify,
    }
}

/// `E 2210` over `kinds` (each record's first word, `None` for an empty one):
/// the first empty record, but only when more than [`FREE_RESERVE`] records
/// are empty or of kind 2.
#[must_use]
pub fn free_slot(kinds: &[Option<u32>]) -> Option<usize> {
    let mut first = None;
    let mut free = 0_usize;
    for (slot, kind) in kinds.iter().enumerate() {
        match kind {
            None => {
                first.get_or_insert(slot);
                free += 1;
            }
            Some(super::construction_hangar::KIND_NOT_SOLD) => free += 1,
            Some(_) => {}
        }
    }
    first.filter(|_| free > FREE_RESERVE)
}

/// Hands the staged purchase to the construction screen's commit (`E 2263`):
/// the cost leaves the profile's cash and the plane is the profile's.
///
/// # Errors
///
/// [`ConstructionError`] when the commit is refused; the screen and `state`
/// are then unchanged.
pub fn commit_purchase(
    screen: &mut ConstructionScreen,
    ctx: &ConstructionContext<'_>,
    state: &mut CampaignState,
) -> Result<CommitReceipt, ConstructionError> {
    screen.commit(ctx, state)
}
