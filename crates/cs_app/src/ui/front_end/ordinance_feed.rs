//! The `OrdinanceFeed` the OrdinanceLayout screen's `init` takes (task #2069,
//! follow-up to #2062), built the way the engine's natives build it from a
//! plane record: pure data, no Bevy.
//!
//! # What the engine does (static readings, `data/owner-static/engine-facts.toml`
//! `[ordinance_feed]`, not `verified_original`)
//!
//! * `E 2030` names gun position `n` by string `gun_name_base + gun` of the
//!   record's gun slot `n` (`+0x88`) and answers the held ammunition kind of
//!   the slot (`+0x98`): [`GunPosition`] `{ kind: gun, choice: ammo }`.
//! * `E 2027` lists the `ammo_rows` ammunition rows, string
//!   `ammo_row_base + i`.
//! * `E 2028` lists the rocket rows: the records of the twelve-record table
//!   that pass the [`ProgressGate`], string `rocket_row_base + record`.
//! * `E 2031` answers a rocket box's held row: `-1` when the plane lacks the
//!   box ([`PlaneRecord::has_rocket_box`]), otherwise [`RocketTable::row_of`]
//!   of the record's rocket id (`+0xa8`).
//! * `E 2035` loads a working copy of the live record, `E 2036` / `E 2037`
//!   store an ammunition kind / the record index of the chosen rocket row in
//!   it, `E 2034` copies it back: [`OrdinanceDraft`].
//!
//! # What is not known
//!
//! The gate's inputs (flag cells `0x647b5c`, `0x647b68`, `0x647b6c`, progress
//! cell `0x64b678`) are zero-initialised in the image and written by campaign
//! code nobody has read here, so the caller supplies a [`ProgressGate`]; which
//! of the twelve rockets a given campaign state lets through is therefore
//! known only as a function of it. The plane record's box counts (`+0x34`,
//! `+0x38`) are known for the two fresh-campaign records only; how another
//! plane's counts are set is unread.

use crate::engine_facts::{EngineFacts, FlightCheckPlaneSlot, OrdinanceFacts};

use super::ordinance_layout::{AMMO_BOXES, GunPosition, OrdinanceFeed, ROCKET_BOXES};

/// The gate's inputs: the three flag cells folded to "any is non-zero", and
/// the progress cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressGate {
    /// Any of the cells `0x647b5c`, `0x647b68`, `0x647b6c` is non-zero: every
    /// record passes.
    pub unlocked: bool,
    /// The cell `0x64b678`.
    pub progress: i32,
}

/// The rocket table under a gate (`0x40ff00`, `0x40ff50`, `0x40ffa0`).
#[derive(Clone, Copy, Debug)]
pub struct RocketTable<'a> {
    facts: &'a OrdinanceFacts,
    gate: ProgressGate,
}

impl<'a> RocketTable<'a> {
    /// The table the facts commit, read under `gate`.
    #[must_use]
    pub const fn new(facts: &'a OrdinanceFacts, gate: ProgressGate) -> Self {
        Self { facts, gate }
    }

    /// Whether table record `index` is a row: a flag is set, or its first
    /// dword is at most the progress cell plus one.
    #[must_use]
    pub fn passes(&self, index: usize) -> bool {
        self.facts.rocket_entries().get(index).is_some_and(|entry| {
            self.gate.unlocked || entry.first <= self.gate.progress.wrapping_add(1)
        })
    }

    /// The number of rows (`0x40ff00`).
    #[must_use]
    pub fn count(&self) -> usize {
        (0..self.facts.rocket_entries().len())
            .filter(|&index| self.passes(index))
            .count()
    }

    /// The table record row `n` shows (`0x40ff50`); the table length when
    /// `n` is past the last row, as the engine's loop ends there.
    #[must_use]
    pub fn nth(&self, n: usize) -> usize {
        (0..self.facts.rocket_entries().len())
            .filter(|&index| self.passes(index))
            .nth(n)
            .unwrap_or(self.facts.rocket_entries().len())
    }

    /// The row a rocket id sits at (`0x40ffa0`): the rows among records
    /// `0..=id`, minus one; `-1` for a negative id.
    #[must_use]
    pub fn row_of(&self, id: i32) -> i32 {
        let Ok(id) = usize::try_from(id) else {
            return -1;
        };
        let rows = (0..=id).filter(|&index| self.passes(index)).count();
        rows as i32 - 1
    }
}

/// The part of a plane record the screen reads and writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaneRecord {
    /// `+0x2c`.
    pub plane_id: u32,
    /// The gun slots (`+0x88`).
    pub guns: [u32; AMMO_BOXES],
    /// The ammunition kinds (`+0x98`).
    pub ammo: [u32; AMMO_BOXES],
    /// The rocket slots (`+0xa8`).
    pub rockets: [u32; ROCKET_BOXES],
    /// The dword at `+0x34`: rocket boxes 0 to 3 below it exist.
    pub low_boxes: u32,
    /// The dword at `+0x38`: rocket boxes 4 and up below it plus the bias
    /// exist.
    pub high_boxes: u32,
}

impl PlaneRecord {
    /// A fresh campaign's record for a Flight Check slot: the committed
    /// roster record the roster init copies in.
    #[must_use]
    pub fn fresh_campaign(facts: &EngineFacts, slot: FlightCheckPlaneSlot) -> Self {
        let row = facts.flight_check().default_for(slot);
        let feed = facts.ordinance();
        Self {
            plane_id: row.plane_id,
            guns: row.guns,
            ammo: row.ammo,
            rockets: row.rockets,
            low_boxes: feed.value("default_box_low_count") as u32,
            high_boxes: feed.value("default_box_high_count") as u32,
        }
    }

    /// Whether the plane has rocket box `slot` (`E 2031`'s test).
    #[must_use]
    pub fn has_rocket_box(&self, facts: &OrdinanceFacts, slot: usize) -> bool {
        let bias = facts.value("box_high_bias") as usize;
        if slot < 4 {
            slot < self.low_boxes as usize
        } else {
            slot < self.high_boxes as usize + bias
        }
    }
}

/// `E 2035` / `E 2036` / `E 2037` / `E 2034`: a working copy edited and
/// committed back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrdinanceDraft {
    live: PlaneRecord,
    working: PlaneRecord,
}

impl OrdinanceDraft {
    /// `E 2035`: the working copy starts as the live record.
    #[must_use]
    pub fn load(live: PlaneRecord) -> Self {
        Self {
            working: live.clone(),
            live,
        }
    }

    /// The record the screen is showing.
    #[must_use]
    pub const fn working(&self) -> &PlaneRecord {
        &self.working
    }

    /// The record as committed so far.
    #[must_use]
    pub const fn live(&self) -> &PlaneRecord {
        &self.live
    }

    /// `E 2036`: stores the chosen ammunition row as the slot's kind.
    /// Returns whether `slot` is a gun position.
    pub fn choose_ammo(&mut self, slot: usize, choice: usize) -> bool {
        let Some(kind) = self.working.ammo.get_mut(slot) else {
            return false;
        };
        *kind = choice as u32;
        true
    }

    /// `E 2037`: stores the table record the chosen row shows as the box's
    /// rocket. Returns whether `slot` is a rocket box.
    pub fn choose_rocket(&mut self, table: &RocketTable<'_>, slot: usize, row: usize) -> bool {
        let Some(rocket) = self.working.rockets.get_mut(slot) else {
            return false;
        };
        *rocket = table.nth(row) as u32;
        true
    }

    /// `E 2034`: copies the working record over the live one and answers it.
    pub fn commit(&mut self) -> &PlaneRecord {
        self.live = self.working.clone();
        &self.live
    }
}

/// The feed for `record`: `text` answers a string-table id (tag kept or not,
/// as the caller wants it shown); an id it lacks gives an empty line.
#[must_use]
pub fn ordinance_feed(
    facts: &EngineFacts,
    record: &PlaneRecord,
    gate: ProgressGate,
    text: &dyn Fn(u32) -> Option<String>,
) -> OrdinanceFeed {
    let feed = facts.ordinance();
    let line = |id: u64| text(id as u32).unwrap_or_default();
    let table = RocketTable::new(feed, gate);
    let guns = (0..AMMO_BOXES)
        .map(|slot| GunPosition {
            kind: record.guns[slot] as i32,
            name: line(feed.value("gun_name_base") + u64::from(record.guns[slot])),
            choice: record.ammo[slot] as i32,
        })
        .collect();
    let ammo_rows = (0..feed.value("ammo_rows"))
        .map(|row| line(feed.value("ammo_row_base") + row))
        .collect();
    let rocket_rows = (0..table.count())
        .map(|row| line(feed.value("rocket_row_base") + table.nth(row) as u64))
        .collect();
    let rockets = (0..ROCKET_BOXES)
        .map(|slot| {
            if !record.has_rocket_box(feed, slot) {
                return None;
            }
            usize::try_from(table.row_of(record.rockets[slot] as i32)).ok()
        })
        .collect();
    OrdinanceFeed {
        plane_info: None,
        guns,
        ammo_rows,
        rocket_rows,
        rockets,
    }
}
