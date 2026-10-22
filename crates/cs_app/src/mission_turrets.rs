//! The mission's turret entries and their live byte (FLAK-B1-TURRET-WAKE,
//! #1880).
//!
//! The original keeps one registry **entry** per turret node, built from the
//! first `ai.zrd` `TURRET` record whose `NODES` pattern names the node
//! (`0x4a9890`, earliest record wins). Each entry carries a **live byte**
//! (`entry+0x6e`); the turret's own update returns at once while it is clear
//! (`docs/findings/2026-10-21-flak-a-turret-update-law.md`, `gate.live_byte`).
//! The byte starts as the record's `ACTIVATED` (absent: live; `0`: asleep) and
//! only three places write it afterwards: `WAKEUP_TURRETS`' name matcher, the
//! `WAKEUP_ZEP_TURRETS` recursion and the save-state restore
//! (`docs/findings/2026-10-21-m01-flak-and-site-defences.md`).
//!
//! This module owns the entries and that byte. **Nothing here fires**: the
//! update law is FLAK-B2 (#1881).
//!
//! * [`TurretSpec`] — every field of one record the update reads, read from the
//!   decoded `ai.zrd`. A field the record does not state stays `None`; the
//!   original's default is a named accessor, not a stored guess. Keys this
//!   reader does not interpret are listed in [`TurretSpec::other_keys`].
//! * [`TurretEntries`] — one entry per world node a record's one-element
//!   `NODES` pattern matches (positions from the world container's composed
//!   scene graph) and one per zeppelin part chain a two-element `NODES` entry
//!   declares. A zeppelin part is only an entry when the mission program names
//!   its chain (the part plan's own source); a part nothing names is unbound,
//!   which the unknowns of the finding record.
//! * [`TurretWakeLedger`] — the mission host's `WAKEUP_TURRETS` /
//!   `WAKEUP_ZEP_TURRETS` consumer: each emission is applied exactly once,
//!   deduplicated by execution key like
//!   [`crate::mission_ai::WakeEnemiesLedger`], and a name that wakes nothing
//!   is a named [`TurretWakeRefusal`], never swallowed.

use std::collections::BTreeSet;
use std::path::Path;

use cs_content::stunts::{ZrdValue, zrd_flat_fields};
use cs_script::ir::{DirectiveOperation, MemberName};
use cs_script::runtime::{ExecutionKey, MissionState};
use cs_types::Tick;

use crate::mission_ai::wake_enemy_names;
use crate::mission_session::{
    MissionHostSeed, part_state_chains, pattern_matches, subject_node_name,
};
use crate::world_facts::WorldOperands;

/// The original's default `TEAM` of a record that states none (`0x441b80`
/// stores `0 + 2`).
pub const DEFAULT_TEAM: u32 = 2;
/// The original's default `DETECTION_RANGE`, metres (`0x4a9bf7`).
pub const DEFAULT_DETECTION_RANGE_M: f64 = 200.0;

/// Why a `WAKEUP_TURRETS` / `WAKEUP_ZEP_TURRETS` site woke nothing: its
/// arguments are not the top-level text list the measured sites spell.
pub const TURRET_WAKE_ARGUMENTS_REASON: &str = "the turret wake site's arguments are not the top-level text list the measured \
     installation spells, so no name was resolved and nothing was woken";
/// Why a `WAKEUP_TURRETS` name woke nothing.
pub const TURRET_WAKE_NO_MATCH_REASON: &str = "no turret entry of this mission matches the WAKEUP_TURRETS name pattern (`*` is one \
     digit); the original's matcher does nothing for it either, and the name is kept so the \
     miss is visible";
/// Why a `WAKEUP_ZEP_TURRETS` name woke nothing.
pub const ZEPPELIN_WAKE_NO_MATCH_REASON: &str = "no zeppelin turret entry of this mission lies under this WAKEUP_ZEP_TURRETS node; only \
     part chains the mission program names are bound as entries";

/// Every key this reader interprets into a [`TurretSpec`] field or that the
/// part binding already reads from the record
/// ([`crate::mission_session::TurretRecord`]).
const KNOWN_KEYS: [&str; 18] = [
    "NODES",
    "ACTIVATED",
    "TEAM",
    "DETECTION_RANGE",
    "SHOOT_UP_ONLY",
    "FIRE_RATE",
    "FIRE_LIMITS",
    "INACCURACY",
    "ATTACK_INTERVAL",
    "BORED_INTERVAL",
    "PITCH",
    "YAW",
    "EFFECT",
    "AMMO",
    "WEAPON",
    "TITLE",
    "PARTS",
    "HEALTH",
];

/// One `ai.zrd` `TURRET` record, with the fields the turret update reads.
///
/// Numeric list fields keep the numbers the record states, in order; how a
/// pair such as `FIRE_RATE` is drawn from is the update's concern (FLAK-B2).
#[derive(Clone, Debug, PartialEq)]
pub struct TurretSpec {
    /// The record's position in the `TURRET` list.
    pub index: usize,
    /// Every `NODES` entry, each an ancestor-to-descendant pattern chain.
    pub nodes: Vec<Vec<String>>,
    /// `ACTIVATED`'s first value as stated; `None` when the key is absent.
    pub activated: Option<bool>,
    /// `TEAM` as stated.
    pub team: Option<u32>,
    /// `DETECTION_RANGE` as stated, metres.
    pub detection_range_m: Option<f64>,
    /// `SHOOT_UP_ONLY` as stated.
    pub shoot_up_only: Option<bool>,
    /// `FIRE_RATE` numbers, seconds.
    pub fire_rate: Option<Vec<f64>>,
    /// `FIRE_LIMITS` numbers.
    pub fire_limits: Option<Vec<f64>>,
    /// `INACCURACY` as stated.
    pub inaccuracy: Option<f64>,
    /// `ATTACK_INTERVAL` numbers, seconds.
    pub attack_interval: Option<Vec<f64>>,
    /// `BORED_INTERVAL` numbers, seconds.
    pub bored_interval: Option<Vec<f64>>,
    /// `PITCH` numbers (degrees in the record).
    pub pitch: Option<Vec<f64>>,
    /// `YAW` numbers (degrees in the record).
    pub yaw: Option<Vec<f64>>,
    /// `EFFECT` node name.
    pub effect: Option<String>,
    /// `AMMO` as stated.
    pub ammo: Option<f64>,
    /// The `WEAPON`'s `NAME`.
    pub weapon: Option<String>,
    /// Every key of the record this reader does not interpret, in record
    /// order — unknown stays unknown.
    pub other_keys: Vec<String>,
}

impl TurretSpec {
    /// The live byte the record starts its entries with: `ACTIVATED`, default
    /// live (the constructor's `1`, `0x4a9b14`).
    #[must_use]
    pub fn starts_live(&self) -> bool {
        self.activated.unwrap_or(true)
    }

    /// The team id: stated, else [`DEFAULT_TEAM`].
    #[must_use]
    pub fn team_id(&self) -> u32 {
        self.team.unwrap_or(DEFAULT_TEAM)
    }

    /// The detection range: stated, else [`DEFAULT_DETECTION_RANGE_M`].
    #[must_use]
    pub fn detection_range(&self) -> f64 {
        self.detection_range_m.unwrap_or(DEFAULT_DETECTION_RANGE_M)
    }
}

fn number(value: &ZrdValue) -> Option<f64> {
    match value {
        ZrdValue::Int(value) => Some(f64::from(*value)),
        ZrdValue::Float(value) => Some(f64::from(*value)),
        _ => None,
    }
}

/// The numbers of a field value: the list's numeric children, or the bare
/// number. `None` when any child is not a number (not the measured shape).
fn numbers(value: &ZrdValue) -> Option<Vec<f64>> {
    match value {
        ZrdValue::List(children) => children.iter().map(number).collect(),
        other => number(other).map(|value| vec![value]),
    }
}

fn first_number(value: &ZrdValue) -> Option<f64> {
    numbers(value)?.first().copied()
}

fn first_text(value: &ZrdValue) -> Option<String> {
    match value {
        ZrdValue::Text(text) => Some(text.clone()),
        ZrdValue::List(children) => children.first()?.as_text().map(str::to_owned),
        _ => None,
    }
}

/// The `TURRET` list of a decoded `ai.zrd` with every field the update reads,
/// in record order. A record without `NODES` is skipped, as the original
/// skips it (and [`crate::mission_session::read_turret_records`] does); the
/// indexes are the list positions both readers share.
#[must_use]
pub fn read_turret_specs(ai_zrd: &ZrdValue) -> Vec<TurretSpec> {
    let Some(entries) = ai_zrd.as_list() else {
        return Vec::new();
    };
    let list = entries.iter().find_map(|entry| {
        let kids = entry.as_list()?;
        (kids.first()?.as_text() == Some("TURRET")).then(|| kids.get(1)?.as_list())?
    });
    let mut specs = Vec::new();
    for (index, record) in list.unwrap_or_default().iter().enumerate() {
        let fields = zrd_flat_fields(record);
        let field = |name: &str| fields.iter().find(|(key, _)| *key == name).map(|(_, v)| *v);
        let Some(nodes) = field("NODES").and_then(ZrdValue::as_list) else {
            continue;
        };
        let nodes = nodes
            .iter()
            .filter_map(|element| match element {
                ZrdValue::Text(text) => Some(vec![text.clone()]),
                ZrdValue::List(chain) => chain
                    .iter()
                    .map(|part| part.as_text().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .filter(|chain| !chain.is_empty()),
                _ => None,
            })
            .collect();
        let weapon = field("WEAPON").and_then(ZrdValue::as_list).and_then(|w| {
            zrd_flat_fields(&ZrdValue::List(w.to_vec()))
                .into_iter()
                .find(|(key, _)| *key == "NAME")
                .and_then(|(_, value)| first_text(value))
        });
        let flag = |name: &str| field(name).and_then(first_number).map(|value| value != 0.0);
        let list_of = |name: &str| field(name).and_then(numbers);
        specs.push(TurretSpec {
            index,
            nodes,
            activated: flag("ACTIVATED"),
            team: field("TEAM")
                .and_then(first_number)
                .filter(|value| *value >= 0.0 && value.fract() == 0.0)
                .map(|value| value as u32),
            detection_range_m: field("DETECTION_RANGE").and_then(first_number),
            shoot_up_only: flag("SHOOT_UP_ONLY"),
            fire_rate: list_of("FIRE_RATE"),
            fire_limits: list_of("FIRE_LIMITS"),
            inaccuracy: field("INACCURACY").and_then(first_number),
            attack_interval: list_of("ATTACK_INTERVAL"),
            bored_interval: list_of("BORED_INTERVAL"),
            pitch: list_of("PITCH"),
            yaw: list_of("YAW"),
            effect: field("EFFECT").and_then(first_text),
            ammo: field("AMMO").and_then(first_number),
            weapon,
            other_keys: fields
                .iter()
                .map(|(key, _)| (*key).to_owned())
                .filter(|key| !KNOWN_KEYS.contains(&key.as_str()))
                .collect(),
        });
    }
    specs
}

/// What kind of node an entry mounts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurretMount {
    /// A node of the world container (`t_truck01`, `aagun30`).
    World,
    /// A part of a zeppelin the mission program names.
    Zeppelin,
}

/// One turret registry entry.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretEntry {
    /// The entry's name: the world node's stored name, or the zeppelin part.
    pub name: String,
    /// The chain naming it: `[node]` for a world node, `[zeppelin, part, …]`.
    pub chain: Vec<String>,
    /// The world or zeppelin mount.
    pub mount: TurretMount,
    /// The [`TurretSpec::index`] of the record that built it.
    pub record: usize,
    /// The composed world position, metres, for a world node; a zeppelin
    /// part's moves with its aircraft and is not bound here.
    pub position_m: Option<[f64; 3]>,
    /// The team id the entry carries.
    pub team: u32,
    /// The live byte: the update runs only while it is set.
    pub live: bool,
}

/// The mission's turret entries as bound at load, and every refusal to bind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurretEntries {
    /// The entries, world nodes in container order then zeppelin parts.
    pub entries: Vec<TurretEntry>,
    /// Every source that could not be read or bound, verbatim.
    pub refusals: Vec<String>,
    /// Every `WEAPON` record that could not be read or classified, verbatim.
    pub weapon_refusals: Vec<String>,
    /// The `TURRET` records the entries were bound over, for the fire.
    pub specs: Vec<TurretSpec>,
    /// The `WEAPON` records the specs name, read from `weapons.zrd`
    /// (FLAK-B3, #1882); empty for a stage with no installation.
    pub weapons:
        std::collections::BTreeMap<String, crate::mission_session::turret_fire::TurretWeaponRecord>,
}

impl TurretEntries {
    /// Builds the entries: one per world node a one-element `NODES` pattern
    /// matches (earliest record wins) and one per zeppelin part chain a
    /// two-element entry declares.
    ///
    /// `world_nodes` is the container's nodes with composed positions, or the
    /// reason they could not be read.
    #[must_use]
    pub fn bind(
        specs: &[TurretSpec],
        world_nodes: Result<Vec<(String, [f64; 3])>, String>,
        zeppelin_chains: &[MemberName],
    ) -> Self {
        let mut entries = Vec::new();
        let mut refusals = Vec::new();
        match world_nodes {
            Ok(nodes) => {
                for (name, position) in nodes {
                    let found = specs.iter().find(|spec| {
                        spec.nodes.iter().any(|chain| {
                            matches!(chain.as_slice(), [pattern] if pattern_matches(pattern, &name))
                        })
                    });
                    if let Some(spec) = found {
                        entries.push(TurretEntry {
                            chain: vec![name.clone()],
                            name,
                            mount: TurretMount::World,
                            record: spec.index,
                            position_m: Some(position),
                            team: spec.team_id(),
                            live: spec.starts_live(),
                        });
                    }
                }
            }
            Err(reason) => refusals.push(format!(
                "the world container could not be read, so no world turret entry is bound: \
                 {reason}"
            )),
        }
        for chain in zeppelin_chains {
            let [actor, part, ..] = chain.as_slice() else {
                continue;
            };
            let found = specs.iter().find(|spec| {
                spec.nodes.iter().any(|nodes| {
                    matches!(nodes.as_slice(), [mount, node]
                        if pattern_matches(mount, actor) && pattern_matches(node, part))
                })
            });
            if let Some(spec) = found {
                entries.push(TurretEntry {
                    name: part.clone(),
                    chain: chain.to_vec(),
                    mount: TurretMount::Zeppelin,
                    record: spec.index,
                    position_m: None,
                    team: spec.team_id(),
                    live: spec.starts_live(),
                });
            }
        }
        Self {
            entries,
            refusals,
            weapon_refusals: Vec::new(),
            specs: specs.to_vec(),
            weapons: std::collections::BTreeMap::new(),
        }
    }

    /// Reads the world container's positions through the production binder and
    /// binds the entries over them.
    #[must_use]
    pub fn read(
        install_root: &Path,
        group_dir: &str,
        specs: &[TurretSpec],
        zeppelin_chains: &[MemberName],
    ) -> Self {
        Self::bind(
            specs,
            crate::mission_targets::world_node_positions(install_root, group_dir),
            zeppelin_chains,
        )
    }

    /// The entry named `name` (a world node name).
    #[must_use]
    pub fn entry(&self, name: &str) -> Option<&TurretEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }
}

/// One wake the ledger applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurretWake {
    /// The name the site spelled, verbatim.
    pub name: String,
    /// The entries whose live byte this wake newly set, in entry order.
    pub woken: Vec<String>,
    /// How many matching entries were already live.
    pub already_live: usize,
    /// The committed tick the directive was consumed on.
    pub at: Tick,
}

/// Why a turret wake name woke nothing, named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurretWakeRefusal {
    /// The name the site spelled; empty when the arguments were not the
    /// measured shape.
    pub name: String,
    /// Which operation spelled it.
    pub operation: DirectiveOperation,
    /// Why nothing was woken.
    pub reason: &'static str,
    /// The committed tick the directive was consumed on.
    pub at: Tick,
}

/// The mission host's `WAKEUP_TURRETS` / `WAKEUP_ZEP_TURRETS` consumer, and
/// the live entries it writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurretWakeLedger {
    entries: Vec<TurretEntry>,
    applied: BTreeSet<ExecutionKey>,
    wakes: Vec<TurretWake>,
    refused: Vec<TurretWakeRefusal>,
}

impl TurretWakeLedger {
    /// A ledger over the entries as bound at load, nothing applied.
    #[must_use]
    pub fn new(entries: &TurretEntries) -> Self {
        Self {
            entries: entries.entries.clone(),
            ..Self::default()
        }
    }

    /// Applies every turret wake emission of `state` not already applied.
    ///
    /// `WAKEUP_TURRETS` sets the live byte of every world entry whose name
    /// matches the pattern (`*` is one digit). `WAKEUP_ZEP_TURRETS` sets it for
    /// every entry of the named zeppelin node and below. Nothing is applied
    /// twice, and a site or name that wakes nothing is a refusal.
    pub fn consume(&mut self, state: &MissionState, tick: Tick) {
        for emission in state.directives() {
            let zeppelin = match emission.operation {
                DirectiveOperation::WakeTurrets => false,
                DirectiveOperation::WakeZeppelinTurrets => true,
                _ => continue,
            };
            if !self.applied.insert(emission.key.execution_key()) {
                continue;
            }
            let Some(names) = wake_enemy_names(&emission.args) else {
                self.refused.push(TurretWakeRefusal {
                    name: String::new(),
                    operation: emission.operation,
                    reason: TURRET_WAKE_ARGUMENTS_REASON,
                    at: tick,
                });
                continue;
            };
            for name in names {
                self.wake(&name, zeppelin, emission.operation, tick);
            }
        }
    }

    fn wake(&mut self, name: &str, zeppelin: bool, operation: DirectiveOperation, tick: Tick) {
        let (mut woken, mut already_live, mut matched) = (Vec::new(), 0, false);
        for entry in &mut self.entries {
            let hit = if zeppelin {
                entry.mount == TurretMount::Zeppelin
                    && entry.chain.first().is_some_and(|actor| actor == name)
            } else {
                entry.mount == TurretMount::World && pattern_matches(name, &entry.name)
            };
            if !hit {
                continue;
            }
            matched = true;
            if entry.live {
                already_live += 1;
            } else {
                entry.live = true;
                woken.push(entry.name.clone());
            }
        }
        if matched {
            self.wakes.push(TurretWake {
                name: name.to_owned(),
                woken,
                already_live,
                at: tick,
            });
        } else {
            self.refused.push(TurretWakeRefusal {
                name: name.to_owned(),
                operation,
                reason: if zeppelin {
                    ZEPPELIN_WAKE_NO_MATCH_REASON
                } else {
                    TURRET_WAKE_NO_MATCH_REASON
                },
                at: tick,
            });
        }
    }

    /// The entries with their current live bytes.
    #[must_use]
    pub fn entries(&self) -> &[TurretEntry] {
        &self.entries
    }

    /// The entry named `name` (a world node name).
    #[must_use]
    pub fn entry(&self, name: &str) -> Option<&TurretEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// How many wake emissions have been applied.
    #[must_use]
    pub fn applied(&self) -> usize {
        self.applied.len()
    }

    /// Every wake that matched at least one entry, in consumption order.
    #[must_use]
    pub fn wakes(&self) -> &[TurretWake] {
        &self.wakes
    }

    /// Every name that woke nothing and why, in consumption order.
    #[must_use]
    pub fn refused(&self) -> &[TurretWakeRefusal] {
        &self.refused
    }
}

/// Every part chain of every bound zeppelin the mission program names — the
/// part plan's own source ([`part_state_chains`]) — for binding zeppelin
/// turret entries.
#[must_use]
pub fn zeppelin_part_chains(seed: &MissionHostSeed) -> Vec<MemberName> {
    let operands = WorldOperands::of(&seed.control);
    let Some(actors) = seed.world_actors.as_ref() else {
        return Vec::new();
    };
    actors
        .subjects
        .values()
        .filter_map(subject_node_name)
        .flat_map(|name| part_state_chains(&seed.resolver, &operands, name))
        .map(|(chain, _)| chain)
        .collect()
}
