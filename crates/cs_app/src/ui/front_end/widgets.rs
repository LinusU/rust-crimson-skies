//! The settings-page widget rows of `ASSETS/LAYOUT.CSV` (task #1996): the
//! `T` text, `Z` slider and `D` dropdown rows a Preferences submenu
//! (`[@Audio@]`, `[@Video@]`, `[@GameOptions@]`, `[@ControlsPrefs@]`) is made
//! of beside its `P` panes and `B` buttons.
//!
//! The generic [`super::MenuShell`] draws only `P`/`B`/`M` rows and counts
//! every other kind; the page models ([`super::AudioPage`]) read the rows they
//! own through these parsers, so the next submenu reuses the same geometry
//! reading instead of a second one.
//!
//! # Record layouts (static readings, not `verified_original`)
//!
//! The header comments of `LAYOUT.CSV` (every column after the id and kind):
//!
//! * `ID=T,ResID,X,Y,Z,Width,Height,Color,Justify` — "Width or Height = 0
//!   means auto-size"; the data rows agree (`AP_T_TITLE=T,IDS_AP_TITLE,122,
//!   228,50,200,28,0XFFC0BAAD,1`). `Justify` is 0 left, 1 centre (the title
//!   rows) — 2 right is not written by any `Audio` row and is read as the
//!   obvious continuation, unmeasured.
//! * `ID=Z,X,Y,Z,MinVal,MaxVal,CurrVal,RegionFile,SliderFile,Left,Top,Right,
//!   Bottom` — the slot art (`PF_B_SliderSlot.png`, 171x3) and the knob art
//!   (`PF_B_Slider.png`, 43x21). **`Left`/`Top`/`Right`/`Bottom`
//!   (`0,-10,1,-10`) are unread**: this reading uses `Left` and `Top` as the
//!   knob's offset from the slot's top-left (`-10` centres a 21-row knob on a
//!   3-row slot to within a row) and ignores `Right`/`Bottom`; the original's
//!   exact hit region is recorded as unknown.
//! * `ID=D,Slider,UpArrow,DownArrow,DropUp,DropDown,ScriptPointer,X,Y,Z,Width,
//!   Height,TotalDisplayed,TabOrder` — the data rows omit `ScriptPointer`
//!   (the same header staleness the `B`, `E` and `L` headers have), so `X` is
//!   column 5.

use super::original::{OriginalLayout, Row, Section};
use super::shell::ShellRect;

/// Why a widget row could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetError {
    /// The layout has no section of this name.
    NoSection(String),
    /// A column holds a placeholder no variable defines.
    Unresolved {
        /// The row id.
        row: String,
        /// The placeholder.
        name: String,
    },
    /// A column is missing or not the number the record layout states.
    Column {
        /// The row id.
        row: String,
        /// The column's name in the header's record layout.
        column: &'static str,
    },
    /// A slider's art could not be measured.
    ArtUnread {
        /// The row id.
        row: String,
        /// The art name.
        art: String,
    },
}

impl std::fmt::Display for WidgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSection(name) => write!(f, "the layout has no {name} section"),
            Self::Unresolved { row, name } => write!(f, "{row}: placeholder <{name}> is undefined"),
            Self::Column { row, column } => write!(f, "{row}: the {column} column is unreadable"),
            Self::ArtUnread { row, art } => write!(f, "{row}: the art {art} cannot be measured"),
        }
    }
}

impl std::error::Error for WidgetError {}

/// One `T` row: a string-table name drawn in a box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRow {
    /// The row id, upper-cased.
    pub id: String,
    /// The `IDS_*` name the row writes.
    pub name: String,
    /// The box in logical pixels (`Width`/`Height` 0 = auto-size, kept as 0).
    pub rect: ShellRect,
    /// `0xAARRGGBB`.
    pub color: u32,
    /// 0 left, 1 centre, 2 right.
    pub justify: u32,
}

/// One `Z` row: a horizontal slider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliderRow {
    /// The row id, upper-cased.
    pub id: String,
    /// The slot's top-left in logical pixels.
    pub x: i32,
    /// The slot's top-left in logical pixels.
    pub y: i32,
    /// Smallest value.
    pub min: i32,
    /// Largest value.
    pub max: i32,
    /// The authored `CurrVal`: the value before anything is stored.
    pub initial: i32,
    /// The slot's art name.
    pub slot_art: String,
    /// The knob's art name.
    pub knob_art: String,
    /// The slot's extent.
    pub slot: (u32, u32),
    /// The knob's extent.
    pub knob: (u32, u32),
    /// The knob's offset from the slot's top-left (`Left`, `Top`).
    pub knob_offset: (i32, i32),
}

impl SliderRow {
    /// The knob's top-left for `value`, in logical pixels: it travels the
    /// slot's width less its own, so it never leaves the slot.
    #[must_use]
    pub fn knob_at(&self, value: i32) -> (i32, i32) {
        let span = i64::from((self.max - self.min).max(1));
        let travel = i64::from(self.slot.0.saturating_sub(self.knob.0));
        let along = i64::from(value.clamp(self.min, self.max) - self.min) * travel / span;
        (
            self.x + self.knob_offset.0 + along as i32,
            self.y + self.knob_offset.1,
        )
    }

    /// The region a press grabs: the slot's width over the knob's height.
    #[must_use]
    pub fn hit_rect(&self) -> ShellRect {
        ShellRect {
            x: self.x,
            y: self.y + self.knob_offset.1,
            width: self.slot.0,
            height: self.knob.1,
        }
    }

    /// The value a pointer at logical `x` means: the knob's centre follows the
    /// pointer along the slot's travel, rounded to the nearest whole value.
    #[must_use]
    pub fn value_at(&self, x: i32) -> i32 {
        let travel = i64::from(self.slot.0.saturating_sub(self.knob.0)).max(1);
        let along =
            (i64::from(x) - i64::from(self.x) - i64::from(self.knob.0 / 2)).clamp(0, travel);
        let span = i64::from((self.max - self.min).max(1));
        let value = i64::from(self.min) + (along * span + travel / 2) / travel;
        value.clamp(i64::from(self.min), i64::from(self.max)) as i32
    }
}

/// One `D` row: a drop-down list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropdownRow {
    /// The row id, upper-cased.
    pub id: String,
    /// The closed box.
    pub rect: ShellRect,
    /// The row's `TotalDisplayed`.
    pub displayed: u32,
    /// The arrow art that closes the box's right edge (`DropDown`).
    pub arrow_art: String,
}

fn number(row: &Row, index: usize, column: &'static str) -> Result<i32, WidgetError> {
    row.fields
        .get(index)
        .and_then(|field| field.trim().parse::<i32>().ok())
        .ok_or_else(|| WidgetError::Column {
            row: row.id.clone(),
            column,
        })
}

fn colour(row: &Row, index: usize) -> Result<u32, WidgetError> {
    row.fields
        .get(index)
        .map(|field| field.trim())
        .and_then(|field| {
            field
                .strip_prefix("0x")
                .or_else(|| field.strip_prefix("0X"))
        })
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .ok_or_else(|| WidgetError::Column {
            row: row.id.clone(),
            column: "Color",
        })
}

fn resolved(row: &Row) -> Result<(), WidgetError> {
    match row.unresolved.first() {
        Some(name) => Err(WidgetError::Unresolved {
            row: row.id.clone(),
            name: name.clone(),
        }),
        None => Ok(()),
    }
}

fn section<'a>(layout: &'a OriginalLayout, name: &str) -> Result<&'a Section, WidgetError> {
    layout
        .section(name)
        .ok_or_else(|| WidgetError::NoSection(name.to_owned()))
}

/// Every `T` row of `name`, in declaration order.
///
/// # Errors
///
/// [`WidgetError`] for a missing section or a row that is not readable; a row
/// is never skipped silently.
pub fn text_rows(layout: &OriginalLayout, name: &str) -> Result<Vec<TextRow>, WidgetError> {
    section(layout, name)?
        .other
        .iter()
        .filter(|row| row.kind.eq_ignore_ascii_case("T"))
        .map(|row| {
            resolved(row)?;
            let id_name = row
                .fields
                .first()
                .map(|field| field.trim().to_owned())
                .filter(|field| !field.is_empty())
                .ok_or_else(|| WidgetError::Column {
                    row: row.id.clone(),
                    column: "ResID",
                })?;
            let unsigned = |index, column| number(row, index, column).map(|n| n.max(0) as u32);
            Ok(TextRow {
                id: row.id.clone(),
                name: id_name,
                rect: ShellRect {
                    x: number(row, 1, "X")?,
                    y: number(row, 2, "Y")?,
                    width: unsigned(4, "Width")?,
                    height: unsigned(5, "Height")?,
                },
                color: colour(row, 6)?,
                justify: unsigned(7, "Justify")?,
            })
        })
        .collect()
}

/// Every `Z` row of `name`, with its art measured through `measure`.
///
/// # Errors
///
/// [`WidgetError`] as for [`text_rows`].
pub fn slider_rows(
    layout: &OriginalLayout,
    name: &str,
    measure: &dyn Fn(&str) -> Option<(u32, u32)>,
) -> Result<Vec<SliderRow>, WidgetError> {
    section(layout, name)?
        .other
        .iter()
        .filter(|row| row.kind.eq_ignore_ascii_case("Z"))
        .map(|row| {
            resolved(row)?;
            let art = |index: usize, column| {
                row.fields
                    .get(index)
                    .map(|field: &String| field.trim().to_owned())
                    .filter(|field| !field.is_empty())
                    .ok_or_else(|| WidgetError::Column {
                        row: row.id.clone(),
                        column,
                    })
            };
            let slot_art = art(6, "RegionFile")?;
            let knob_art = art(7, "SliderFile")?;
            let extent = |art: &str| {
                measure(art).ok_or_else(|| WidgetError::ArtUnread {
                    row: row.id.clone(),
                    art: art.to_owned(),
                })
            };
            Ok(SliderRow {
                id: row.id.clone(),
                x: number(row, 0, "X")?,
                y: number(row, 1, "Y")?,
                min: number(row, 3, "MinVal")?,
                max: number(row, 4, "MaxVal")?,
                initial: number(row, 5, "CurrVal")?,
                slot: extent(&slot_art)?,
                knob: extent(&knob_art)?,
                slot_art,
                knob_art,
                knob_offset: (number(row, 8, "Left")?, number(row, 9, "Top")?),
            })
        })
        .collect()
}

/// Every `D` row of `name`.
///
/// # Errors
///
/// [`WidgetError`] as for [`text_rows`].
pub fn dropdown_rows(layout: &OriginalLayout, name: &str) -> Result<Vec<DropdownRow>, WidgetError> {
    section(layout, name)?
        .other
        .iter()
        .filter(|row| row.kind.eq_ignore_ascii_case("D"))
        .map(|row| {
            resolved(row)?;
            let unsigned = |index, column| number(row, index, column).map(|n| n.max(0) as u32);
            Ok(DropdownRow {
                id: row.id.clone(),
                rect: ShellRect {
                    x: number(row, 5, "X")?,
                    y: number(row, 6, "Y")?,
                    width: unsigned(8, "Width")?,
                    height: unsigned(9, "Height")?,
                },
                displayed: unsigned(10, "TotalDisplayed")?,
                arrow_art: row
                    .fields
                    .get(4)
                    .map_or_else(String::new, |f| f.trim().to_owned()),
            })
        })
        .collect()
}
