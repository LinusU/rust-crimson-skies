//! The `ammo` command of the INTERP command language (task #2173,
//! T452-R05-AMMO-SCRIPT).
//!
//! The decrypted engine stores `arg1` into slot `arg0` of the twelve-entry
//! loadout slot table (slots `0..4` gun, `4..12` rocket) for the two-argument
//! form, and fills the eight rocket slots with `arg0` for the one-argument
//! form (static, never `verified_original`; see
//! `docs/findings/2026-10-28-t452-r05-ammo-command-census.md`). This module
//! resolves a script line into that meaning and censuses a decoded container;
//! it decides nothing about what the engine does with the slot afterwards.
//!
//! The head is matched ASCII case-insensitively and the arguments are read as
//! decimal ASCII. How the engine's own argument reader (`0x5b7980`) spells a
//! number is not established, so any argument that is not plain decimal digits
//! is reported as [`AmmoLineIssue::UnreadableArgument`], never coerced.

use std::collections::{BTreeMap, BTreeSet};

use cs_formats::interp::{DecodedInterp, InterpLine};

/// The command name the engine compares against (string at `0x622ee0`).
pub const AMMO_COMMAND: &str = "ammo";

/// Slots `0..AMMO_GUN_SLOTS` are gun slots.
pub const AMMO_GUN_SLOTS: u32 = 4;

/// The slot table's entry count (`0x62ae28`): four gun and eight rocket slots.
pub const AMMO_SLOT_COUNT: u32 = 12;

/// What an `ammo` line asks for, when it is well formed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmmoAction {
    /// Two arguments: store `index` into `slot`.
    SetSlot {
        /// The slot argument (`arg0`).
        slot: u32,
        /// The weapon-record index argument (`arg1`).
        index: u32,
    },
    /// One argument: fill all eight rocket slots (`4..12`) with `index`.
    FillRockets {
        /// The weapon-record index argument (`arg0`).
        index: u32,
    },
}

impl AmmoAction {
    /// The record index the line installs.
    #[must_use]
    pub const fn index(self) -> u32 {
        match self {
            Self::SetSlot { index, .. } | Self::FillRockets { index } => index,
        }
    }

    /// The slots the line writes: one for the two-argument form, `4..12` for
    /// the one-argument form.
    #[must_use]
    pub fn slots(self) -> Vec<u32> {
        match self {
            Self::SetSlot { slot, .. } => vec![slot],
            Self::FillRockets { .. } => (AMMO_GUN_SLOTS..AMMO_SLOT_COUNT).collect(),
        }
    }
}

/// Why an `ammo` line is reported instead of resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmmoLineIssue {
    /// The line has neither one nor two arguments.
    UnsupportedArity {
        /// The argument count found.
        arguments: usize,
    },
    /// An argument is not plain decimal digits (or overflows `u32`).
    UnreadableArgument {
        /// The argument position.
        position: usize,
    },
    /// The two-argument form names a slot at or past [`AMMO_SLOT_COUNT`]; the
    /// engine does not range-check it.
    SlotOutOfRange {
        /// The slot argument.
        slot: u32,
    },
    /// The index names no `wep_` record of the supplied record set; the engine
    /// does not range-check it.
    IndexOutOfRange {
        /// The index argument.
        index: u32,
    },
}

/// One resolved `ammo` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmmoLine {
    /// The script's name bytes, lossily spelled.
    pub script: String,
    /// The line's position inside its script.
    pub line: usize,
    /// The meaning, when the arguments read.
    pub action: Option<AmmoAction>,
    /// Every reason the line is not a clean in-range command.
    pub issues: Vec<AmmoLineIssue>,
}

/// The `wep_` record name an index installs.
#[must_use]
pub fn ammo_record_name(index: u32) -> String {
    format!("wep_{index:02}")
}

fn decimal(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Resolves one line, or `None` when its head is not the `ammo` command.
/// `known_indices` is the set of record indices that exist.
#[must_use]
pub fn resolve_ammo_line(
    script: &str,
    position: usize,
    line: &InterpLine<'_>,
    known_indices: &BTreeSet<u32>,
) -> Option<AmmoLine> {
    let tokens = line.tokens();
    let head = tokens.first()?;
    if !head.bytes().eq_ignore_ascii_case(AMMO_COMMAND.as_bytes()) {
        return None;
    }
    let args = &tokens[1..];
    let mut issues = Vec::new();
    let mut values = Vec::new();
    for (offset, token) in args.iter().enumerate() {
        match decimal(token.bytes()) {
            Some(value) => values.push(value),
            None => issues.push(AmmoLineIssue::UnreadableArgument { position: offset }),
        }
    }
    let action = match (args.len(), values.as_slice()) {
        (1, [index]) => Some(AmmoAction::FillRockets { index: *index }),
        (2, [slot, index]) => Some(AmmoAction::SetSlot {
            slot: *slot,
            index: *index,
        }),
        (arguments, _) if !matches!(arguments, 1 | 2) => {
            issues.push(AmmoLineIssue::UnsupportedArity { arguments });
            None
        }
        _ => None,
    };
    if let Some(action) = action {
        if let AmmoAction::SetSlot { slot, .. } = action
            && slot >= AMMO_SLOT_COUNT
        {
            issues.push(AmmoLineIssue::SlotOutOfRange { slot });
        }
        if !known_indices.contains(&action.index()) {
            issues.push(AmmoLineIssue::IndexOutOfRange {
                index: action.index(),
            });
        }
    }
    Some(AmmoLine {
        script: script.to_owned(),
        line: position,
        action,
        issues,
    })
}

/// The census of one decoded container.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AmmoCensus {
    /// Scripts inspected.
    pub scripts: usize,
    /// Lines inspected.
    pub lines: usize,
    /// Every `ammo` line, in container order.
    pub commands: Vec<AmmoLine>,
}

impl AmmoCensus {
    /// Per script, the number of `ammo` lines.
    #[must_use]
    pub fn per_script(&self) -> BTreeMap<&str, usize> {
        let mut map = BTreeMap::new();
        for command in &self.commands {
            *map.entry(command.script.as_str()).or_insert(0) += 1;
        }
        map
    }

    /// The set of indices installed by resolved lines.
    #[must_use]
    pub fn installed_indices(&self) -> BTreeSet<u32> {
        self.commands
            .iter()
            .filter_map(|command| command.action.map(AmmoAction::index))
            .collect()
    }

    /// The slots a given index is installed into.
    #[must_use]
    pub fn slots_of(&self, index: u32) -> BTreeSet<u32> {
        self.commands
            .iter()
            .filter_map(|command| command.action)
            .filter(|action| action.index() == index)
            .flat_map(AmmoAction::slots)
            .collect()
    }

    /// The lines that carry at least one issue.
    #[must_use]
    pub fn reported(&self) -> Vec<&AmmoLine> {
        self.commands
            .iter()
            .filter(|command| !command.issues.is_empty())
            .collect()
    }
}

/// Walks every script of `decoded` for `ammo` lines.
#[must_use]
pub fn census_ammo_commands(
    decoded: &DecodedInterp<'_>,
    known_indices: &BTreeSet<u32>,
) -> AmmoCensus {
    let mut census = AmmoCensus::default();
    for script in decoded.scripts() {
        census.scripts += 1;
        let name = String::from_utf8_lossy(script.name()).into_owned();
        for (position, line) in script.lines().iter().enumerate() {
            census.lines += 1;
            if let Some(resolved) = resolve_ammo_line(&name, position, line, known_indices) {
                census.commands.push(resolved);
            }
        }
    }
    census
}

/// The installation's INTERP container, relative to the install root.
pub const AMMO_SCRIPT_CONTAINER: &str = "ZBD/interp.zbd";

/// Why the installation census could not run.
#[derive(Debug)]
pub enum AmmoCensusError {
    /// The container could not be read.
    Io(String),
    /// The container is not a valid INTERP container.
    Interp(String),
    /// The `wep_` records could not be read.
    Records(String),
}

impl std::fmt::Display for AmmoCensusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(detail) => write!(f, "{AMMO_SCRIPT_CONTAINER} cannot be read: {detail}"),
            Self::Interp(detail) => write!(f, "{AMMO_SCRIPT_CONTAINER} does not decode: {detail}"),
            Self::Records(detail) => write!(f, "the weapon records cannot be read: {detail}"),
        }
    }
}

impl std::error::Error for AmmoCensusError {}

/// The record indices (`wep_NN` with a decimal `NN`) of a record-name list.
#[must_use]
pub fn record_indices<'a>(records: impl IntoIterator<Item = &'a str>) -> BTreeSet<u32> {
    records
        .into_iter()
        .filter_map(|name| name.strip_prefix("wep_"))
        .filter_map(decimal_str)
        .collect()
}

fn decimal_str(text: &str) -> Option<u32> {
    decimal(text.as_bytes())
}

/// Reads the installation's INTERP container and every `wep_` record, and
/// censuses the `ammo` command over them.
///
/// # Errors
///
/// [`AmmoCensusError`] when the container or the records cannot be read.
pub fn read_original_ammo_census(
    install_root: &std::path::Path,
) -> Result<AmmoCensus, AmmoCensusError> {
    let bytes = std::fs::read(install_root.join(AMMO_SCRIPT_CONTAINER))
        .map_err(|error| AmmoCensusError::Io(error.to_string()))?;
    let mut context = cs_formats::ParseContext::with_defaults(AMMO_SCRIPT_CONTAINER);
    let decoded = cs_formats::interp::decode_interp(&mut context, &bytes)
        .map_err(|error| AmmoCensusError::Interp(error.to_string()))?;
    let weapons = crate::ordnance::read_original_weapon_census(install_root)
        .map_err(|error| AmmoCensusError::Records(error.to_string()))?;
    let known = record_indices(weapons.records.iter().map(|record| record.record.as_str()));
    Ok(census_ammo_commands(&decoded, &known))
}
