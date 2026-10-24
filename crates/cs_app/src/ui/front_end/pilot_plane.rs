//! The start flow's per-pilot working plane and cash (#2075, part of #2052).
//!
//! A new pilot's working plane begins as the engine's 204-byte template and
//! the cash as the reset routine's write (`0x4113b0`); the Plane Name
//! dialog's OK then runs `E 2212`, which empties the loadout when the
//! default-loadout box is unchecked. Every number comes from
//! `data/owner-static/engine-facts.toml` (`static`, #2074). Words the table
//! leaves unknown (the name, the three derived colours, the trailing pointer)
//! are 0 here and are read by nothing.
//!
//! [`PilotPlane::hub_feed`] and [`PilotPlane::purchase_feed`] give the hub the
//! figures of the working plane through the engine's own tables
//! ([`super::construction_purchase`]): dollars and pounds, as the tables hold
//! them.

use super::construction_hangar::{SAVED_SLOTS, SavedPlane};
use super::construction_hub::{HubFeed, HubPage};
use super::construction_purchase::{
    PurchaseError, PurchaseFeed, capacity, plane_value, plane_weight,
};
use super::construction_stats::{AIRFRAMES, PlaneRecord};
use super::previous_missions::split_tag;
use crate::engine_facts::{NewPilotStartFacts, engine_facts};

/// The first airframe-name string (`3000 + airframe`).
const AIRFRAME_STRING: u32 = 3000;
/// The first short airframe-name string (`3020 + airframe`).
const AIRFRAME_SHORT_STRING: u32 = 3020;

/// The strings the hub names an airframe with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AirframeNames {
    /// String `3000 + n`, `[FONT]` tag stripped.
    pub long: Option<String>,
    /// String `3020 + n`, `[FONT]` tag stripped.
    pub short: Option<String>,
}

/// Reads the airframes' names through `id`, an installation string lookup.
pub fn airframe_names(id: &dyn Fn(u32) -> Option<String>) -> Vec<AirframeNames> {
    let plain = |number: u32| id(number).map(|raw| split_tag(&raw).1.to_owned());
    (0..AIRFRAMES as u32)
        .map(|n| AirframeNames {
            long: plain(AIRFRAME_STRING + n),
            short: plain(AIRFRAME_SHORT_STRING + n),
        })
        .collect()
}

/// A word of the record that does not hold a value the tables take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadWord {
    /// The record offset.
    pub offset: u32,
    /// The word.
    pub value: i32,
}

/// One pilot's working plane and cash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PilotPlane {
    cash: i64,
    words: Vec<i32>,
    /// The saved-plane records (`0x64b78c`, 25 of them); a record whose first
    /// word is 0 is empty.
    saved: Vec<SavedRecord>,
    /// The selected record (`[0x64b67c]`), 0 after the reset.
    selected: usize,
}

/// One saved-plane record: its name (`+4`) and its dwords.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SavedRecord {
    name: String,
    words: Vec<i32>,
}

impl PilotPlane {
    /// A new pilot: the template record, emptied when `default_loadout` is
    /// false (the Plane Name box unchecked), and the starting cash, which
    /// depends on the profile variable `fAllowAll` (`allow_all`).
    #[must_use]
    pub fn from_facts(facts: &NewPilotStartFacts, allow_all: bool, default_loadout: bool) -> Self {
        let mut words = vec![0_i32; facts.value("record_dwords") as usize];
        for word in facts.template_words() {
            words[(word.offset / 4) as usize] = word.value;
        }
        // The reset copies the template over record 0 before `E 2212` empties
        // the working plane, so record 0 keeps the stock loadout.
        let record_words = words.len();
        let mut saved = vec![
            SavedRecord {
                name: String::new(),
                words: vec![0; record_words],
            };
            SAVED_SLOTS
        ];
        saved[0].words.clone_from(&words);
        if !default_loadout {
            facts.apply_empty_loadout(&mut words);
        }
        // `allow_all` also fills records 2..12 with the eleven airframe
        // templates (`0x41160b`); the table does not carry those, so they stay
        // empty (recorded as unknown in the findings).
        Self {
            cash: facts.starting_cash(allow_all),
            words,
            saved,
            selected: 0,
        }
    }

    /// [`Self::from_facts`] over the committed table; `None` (reported) when
    /// the table does not parse.
    #[must_use]
    pub fn new_pilot(allow_all: bool, default_loadout: bool) -> Option<Self> {
        match engine_facts() {
            Ok(facts) => Some(Self::from_facts(
                facts.new_pilot(),
                allow_all,
                default_loadout,
            )),
            Err(error) => {
                eprintln!("cs: no new-pilot start state: {error}");
                None
            }
        }
    }

    /// Stores `words` (a 204-byte record) as saved plane `slot` under `name`.
    ///
    /// # Panics
    ///
    /// When `slot` is not below [`SAVED_SLOTS`] or the record has the wrong
    /// length.
    pub fn save_plane(&mut self, slot: usize, name: &str, words: Vec<i32>) {
        assert_eq!(words.len(), self.words.len(), "a saved record is 51 dwords");
        self.saved[slot] = SavedRecord {
            name: name.to_owned(),
            words,
        };
    }

    /// Empties saved plane `slot` (a sale: first word 0).
    pub fn clear_saved(&mut self, slot: usize) {
        self.saved[slot].words[0] = 0;
    }

    /// The working plane's record, to save under a slot.
    #[must_use]
    pub fn working_words(&self) -> &[i32] {
        &self.words
    }

    /// The selected saved record (`[0x64b67c]`).
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Selects a saved record.
    pub fn select(&mut self, slot: usize) {
        self.selected = slot.min(SAVED_SLOTS - 1);
    }

    /// The occupied records (first word non-zero).
    #[must_use]
    pub fn occupied(&self) -> usize {
        self.saved.iter().filter(|r| r.words[0] != 0).count()
    }

    /// `E 2018` on the construction screen (engine state 13 or 17): the
    /// occupied records less one.
    #[must_use]
    pub fn saved_count(&self) -> u32 {
        let occupied = self.occupied() as u32;
        occupied - u32::from(occupied > 0)
    }

    /// The hangar list's planes, one entry per slot: kind (first word), name,
    /// airframe and value through the engine's tables. A record the tables
    /// cannot read is reported and left out of the list.
    #[must_use]
    pub fn hangar_planes(&self) -> Vec<Option<SavedPlane>> {
        self.saved
            .iter()
            .map(|saved| {
                let kind = u32::try_from(saved.words[0]).ok().filter(|&k| k != 0)?;
                let record = match record_of(&saved.words) {
                    Ok(record) => record,
                    Err(error) => {
                        eprintln!("cs: a saved plane holds {error:?}");
                        return None;
                    }
                };
                let value = match plane_value(&record) {
                    Ok(value) => value.max(0) as u64,
                    Err(error) => {
                        eprintln!("cs: a saved plane is not priced: {error:?}");
                        return None;
                    }
                };
                Some(SavedPlane {
                    kind,
                    name: saved.name.clone(),
                    airframe: record.airframe,
                    value,
                })
            })
            .collect()
    }

    /// The cash in dollars (`[0x64b788]`).
    #[must_use]
    pub fn cash(&self) -> i64 {
        self.cash
    }

    /// Sets the cash (a purchase, a sale, a payout).
    pub fn set_cash(&mut self, cash: i64) {
        self.cash = cash;
    }

    /// One dword of the record by byte offset.
    #[must_use]
    pub fn word(&self, offset: u32) -> i32 {
        self.words[(offset / 4) as usize]
    }

    /// Sets the airframe (`+0x2c`).
    pub fn set_airframe(&mut self, airframe: u32) {
        self.words[0x2c / 4] = airframe as i32;
    }

    /// Sets the engine (`+0x30`).
    pub fn set_engine(&mut self, engine: u32) {
        self.words[0x30 / 4] = engine as i32;
    }

    /// The fields of the record the statistics and prices read.
    ///
    /// # Errors
    ///
    /// [`BadWord`] for a negative airframe, engine or gun word.
    pub fn record(&self) -> Result<PlaneRecord, BadWord> {
        record_of(&self.words)
    }

    /// The numbers the hub draws for this plane. A figure the tables cannot
    /// answer stays unknown (and is reported), never zero.
    #[must_use]
    pub fn hub_feed(&self, names: &[AirframeNames]) -> HubFeed {
        let mut feed = HubFeed {
            cash: Some(self.cash.max(0) as u64),
            // The working plane is the saved copy right after `E 2212`.
            dirty: false,
            saved_planes: Some(self.saved_count()),
            ..HubFeed::default()
        };
        let record = match self.record() {
            Ok(record) => record,
            Err(error) => {
                eprintln!("cs: the working plane holds {error:?}");
                return feed;
            }
        };
        let figure = |value: Result<i32, PurchaseError>| match value {
            Ok(value) => Some(value.max(0) as u64),
            Err(error) => {
                eprintln!("cs: the working plane is not priced: {error:?}");
                None
            }
        };
        feed.plane_cost = figure(plane_value(&record));
        feed.weight_capacity = figure(capacity(&record));
        feed.current_weight = figure(plane_weight(&record));
        if let Some(name) = names.get(record.airframe as usize) {
            feed.airframe = name.long.clone();
            feed.airframe_short = name.short.clone();
        }
        feed.plane = Some(record);
        feed
    }

    /// Hands the hub the working plane's figures, then the hangar's list and
    /// Sell's count from the saved records (without hangar rows the feed's own
    /// count stands). Call again after the record or the cash changes.
    pub fn feed_hub(&self, page: &mut HubPage) {
        page.set_feed(self.hub_feed(page.airframe_names()));
        page.set_saved_planes(self.hangar_planes(), self.selected, self.cash.max(0) as u64);
    }

    /// The purchase panel's feed, `None` for a record the tables cannot read.
    #[must_use]
    pub fn purchase_feed(&self) -> Option<PurchaseFeed> {
        Some(PurchaseFeed {
            record: self.record().ok()?,
            cash: self.cash,
        })
    }
}

/// The fields of a 51-dword record the statistics and prices read.
fn record_of(words: &[i32]) -> Result<PlaneRecord, BadWord> {
    let word = |offset: u32| words[(offset / 4) as usize];
    let unsigned = |offset: u32| {
        let value = word(offset);
        u32::try_from(value).map_err(|_| BadWord { offset, value })
    };
    let mut guns = [0; 4];
    for (slot, gun) in guns.iter_mut().enumerate() {
        *gun = unsigned(0x88 + 4 * slot as u32)?;
    }
    Ok(PlaneRecord {
        airframe: unsigned(0x2c)?,
        engine: unsigned(0x30)?,
        armor: [0x74, 0x78, 0x7c, 0x80].map(word),
        hardpoints: [word(0x34), word(0x38)],
        guns,
        // The nibble of `+0x84`; bit `slot` doubles that slot's gun
        // (`0x4055c0`).
        gun_pairs: (word(0x84) & 0xf) as u8,
    })
}
