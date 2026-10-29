//! The AI maneuver chooser (task #2204, `T452-REG-MANEUVER-CHOOSER`).
//!
//! Expresses what the image's chooser `0x4201a0` was measured to do
//! (`docs/findings/2026-10-29-t452-maneuver-chooser.md`, rows
//! `maneuver_chooser.*` and `tangler_ai.*` in
//! `data/owner-static/engine-facts.toml`; static reading only, nothing here is
//! `verified_original`):
//!
//! 1. the catalog is the seventeen `maneuvers.zrd` records in the engine's fixed
//!    slot order ([`MANEUVER_NAMES`]);
//! 2. per slot the gates run in the measured order: `autogyro_allowed`
//!    (`0x420202`), the skill level (`0x420227`), then the nitro gate
//!    (`0x4202ca`..`0x4202ea`): a `nitro` maneuver is offered only to an actor
//!    that carries the injector and is **not tangled**;
//! 3. the surviving candidates are weighted (`record weight + 1.0`, ×0.1 for the
//!    current maneuver, ×0.2 for the previous, ×6.0 when the actor prefers it,
//!    a negative weight replaced by 0.1) and one is drawn with
//!    `rand() / 32767.7 × Σ weights`;
//! 4. choosing a `nitro` maneuver asks for the injector (`0x4b2110(1)`), which a
//!    tangle refuses; when no candidate survives nothing changes.
//!
//! **Not decoded, so not expressed:** the flight content of a maneuver (its step
//! rows), the condition of the situational weight bonus at `0x4204da`, and the
//! cadence at which the engine calls the chooser. The bonus is an explicit input
//! of [`ChooserActor`], never computed here.

use cs_formats::zbd::ManeuverNode;

/// The seventeen maneuvers in the order the engine constructs its records
/// (`0x45f60c`..`0x45f718`); the index is the slot the chooser loops over.
pub const MANEUVER_NAMES: [&str; 17] = [
    "roll",
    "rudder_turn",
    "bank_turn",
    "climb",
    "dive",
    "jinking",
    "scissors",
    "rolling_scissors",
    "snap_roll",
    "immelman",
    "loop",
    "split_s",
    "lag_pursuit_roll",
    "high_yo_yo",
    "nitro_evade",
    "barrel_roll",
    "spiral_dive",
];

/// The skill level range the actor's `+0x958` is clamped to (`0x47ce80`).
pub const SKILL_LEVEL_MAX: i32 = 9;

/// `0x603598`: the draw scales `rand()` (0..=32767) by this before the weights.
const DRAW_SCALE: f64 = 1.0 / 32_767.7;
/// `0x6034fc`, the `natural_touch` weight factor and the previous-maneuver factor.
const NATURAL_TOUCH_FACTOR: f64 = 0.2;
const PREVIOUS_FACTOR: f64 = 0.2;
/// `0x6034a8`.
const CURRENT_FACTOR: f64 = 0.1;
/// `0x60369c`.
const PREFERRED_FACTOR: f64 = 6.0;
/// `0x6032dc`.
const WEIGHT_OFFSET: f64 = 1.0;
/// The constant `0x420614` stores for a negative weight (`0x3dcccccd`).
const NEGATIVE_WEIGHT_REPLACEMENT: f64 = 0.1;

/// One maneuver record, as the chooser reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ManeuverRecord {
    /// Record `+0`: the lowest skill level the maneuver is offered at.
    pub natural_touch: i32,
    /// Record `+4`: the `relative` key.
    pub relative: bool,
    /// Record `+5`: the `nitro` key.
    pub nitro: bool,
    /// Record `+6`: the `autogyro_allowed` key.
    pub autogyro_allowed: bool,
    /// Record `+8`: `bias + 0.2 × natural_touch` (the second term only when the
    /// node carries the key).
    pub weight: f32,
    /// How many `steps` rows the record carries. The rows are not decoded.
    pub step_rows: usize,
}

impl ManeuverRecord {
    /// Lowers a `maneuvers.zrd` node the way the constructor `0x45f3e0` does.
    pub fn from_node(node: &ManeuverNode<'_>) -> Self {
        let touch = if node.has_natural_touch {
            f64::from(node.natural_touch) * NATURAL_TOUCH_FACTOR
        } else {
            0.0
        };
        Self {
            natural_touch: node.natural_touch,
            relative: node.relative,
            nitro: node.nitro,
            autogyro_allowed: node.autogyro_allowed,
            weight: (f64::from(node.bias) + touch) as f32,
            step_rows: node.steps.len(),
        }
    }
}

/// Why a catalog could not be built from `maneuvers.zrd` nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// The file lacks one of the seventeen maneuvers the engine constructs.
    Missing(&'static str),
    /// The file holds a maneuver the engine has no slot for.
    Unknown(String),
    /// The file names a maneuver twice.
    Duplicate(String),
}

/// The maneuver records in slot order.
#[derive(Clone, Debug, PartialEq)]
pub struct ManeuverCatalog {
    records: Vec<ManeuverRecord>,
}

impl ManeuverCatalog {
    /// A catalog of already-lowered records; slot `i` is `records[i]`. Used for
    /// synthetic catalogs; the retail path is [`Self::from_nodes`].
    pub fn from_records(records: Vec<ManeuverRecord>) -> Self {
        Self { records }
    }

    /// Places the file's nodes into the engine's seventeen slots by name.
    ///
    /// # Errors
    ///
    /// [`CatalogError`] when a name is missing, unknown or repeated.
    pub fn from_nodes(nodes: &[ManeuverNode<'_>]) -> Result<Self, CatalogError> {
        let mut slots: Vec<Option<ManeuverRecord>> = vec![None; MANEUVER_NAMES.len()];
        for node in nodes {
            let slot = MANEUVER_NAMES
                .iter()
                .position(|name| *name == node.name)
                .ok_or_else(|| CatalogError::Unknown(node.name.to_string()))?;
            if slots[slot]
                .replace(ManeuverRecord::from_node(node))
                .is_some()
            {
                return Err(CatalogError::Duplicate(node.name.to_string()));
            }
        }
        let mut records = Vec::with_capacity(slots.len());
        for (slot, record) in slots.into_iter().enumerate() {
            records.push(record.ok_or(CatalogError::Missing(MANEUVER_NAMES[slot]))?);
        }
        Ok(Self { records })
    }

    /// The record in `slot`.
    pub fn record(&self, slot: usize) -> Option<&ManeuverRecord> {
        self.records.get(slot)
    }

    /// The number of slots.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the catalog has no slots.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// The actor state the chooser reads and writes.
#[derive(Clone, Debug, PartialEq)]
pub struct ChooserActor {
    skill_level: i32,
    /// `+0x946`: the aircraft carries the nitrous injector.
    pub has_injector: bool,
    /// `+0x2dc` flag 2: the actor is tangled.
    pub tangled: bool,
    /// `[+0x64]+0x21c` is non-zero: only `autogyro_allowed` maneuvers are offered.
    pub autogyro_restricted: bool,
    /// `+0x9b4`: the maneuver in progress.
    pub current: Option<usize>,
    /// `+0x9b8`: the one before it.
    pub previous: Option<usize>,
    /// `+0x994..+0x998`: slots the actor prefers.
    pub preferred: Vec<usize>,
    /// The weight bonus of `0x4204da`..`0x420504` applies. Its condition is not
    /// decoded; the caller says whether it holds.
    pub situational_bonus: bool,
}

impl ChooserActor {
    /// An actor at `skill_level` (clamped to `0..=9` as `0x47ce80` does) with no
    /// injector, no tangle and no history.
    pub fn new(skill_level: i32) -> Self {
        Self {
            skill_level: skill_level.clamp(0, SKILL_LEVEL_MAX),
            has_injector: false,
            tangled: false,
            autogyro_restricted: false,
            current: None,
            previous: None,
            preferred: Vec::new(),
            situational_bonus: false,
        }
    }

    /// `+0x958`.
    pub fn skill_level(&self) -> i32 {
        self.skill_level
    }
}

/// What the per-slot gates decided, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManeuverGate {
    /// The slot becomes a candidate.
    Offered,
    /// `autogyro_restricted` and the record lacks `autogyro_allowed` (`0x420221`).
    NotAutogyroAllowed,
    /// `natural_touch` is above the actor's skill level (`0x42023d`).
    SkillTooLow,
    /// A `nitro` maneuver and no injector (`0x4202dd`).
    NitroWithoutInjector,
    /// A `nitro` maneuver and the actor is tangled (`0x4202ea`).
    NitroWhileTangled,
}

/// Runs the gates for one record, in the measured order.
pub fn gate(record: &ManeuverRecord, actor: &ChooserActor) -> ManeuverGate {
    if actor.autogyro_restricted && !record.autogyro_allowed {
        return ManeuverGate::NotAutogyroAllowed;
    }
    if record.natural_touch > actor.skill_level {
        return ManeuverGate::SkillTooLow;
    }
    if record.nitro {
        if !actor.has_injector {
            return ManeuverGate::NitroWithoutInjector;
        }
        if actor.tangled {
            return ManeuverGate::NitroWhileTangled;
        }
    }
    ManeuverGate::Offered
}

/// A slot that passed the gates and its draw weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    /// The catalog slot.
    pub slot: usize,
    /// The weight of the draw.
    pub weight: f32,
}

fn weight_of(slot: usize, record: &ManeuverRecord, actor: &ChooserActor) -> f32 {
    let mut weight = f64::from(record.weight) + WEIGHT_OFFSET;
    if actor.situational_bonus {
        weight += WEIGHT_OFFSET;
    }
    if actor.current == Some(slot) {
        weight *= CURRENT_FACTOR;
    }
    if actor.previous == Some(slot) {
        weight *= PREVIOUS_FACTOR;
    }
    if actor.preferred.contains(&slot) {
        weight *= PREFERRED_FACTOR;
    }
    if weight < 0.0 {
        weight = NEGATIVE_WEIGHT_REPLACEMENT;
    }
    weight as f32
}

/// The tick's candidate list: every slot whose gates pass, in slot order.
pub fn candidates(catalog: &ManeuverCatalog, actor: &ChooserActor) -> Vec<Candidate> {
    catalog
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| gate(record, actor) == ManeuverGate::Offered)
        .map(|(slot, record)| Candidate {
            slot,
            weight: weight_of(slot, record, actor),
        })
        .collect()
}

/// What a choice did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManeuverChoice {
    /// The slot that became the actor's current maneuver.
    pub slot: usize,
    /// The chooser asked for the injector and the setter accepted it: the
    /// maneuver is `nitro` and the actor is not tangled (`0x4b2110` refuses a
    /// tangle).
    pub injector_enabled: bool,
}

fn adopt(catalog: &ManeuverCatalog, actor: &mut ChooserActor, slot: usize) -> ManeuverChoice {
    actor.previous = actor.current;
    actor.current = Some(slot);
    let nitro = catalog.records[slot].nitro;
    ManeuverChoice {
        slot,
        injector_enabled: nitro && !actor.tangled,
    }
}

/// The all-maneuver path: gate, weigh and draw with `roll`, the value the
/// engine's `rand()` returned (`0..=32767`).
///
/// Returns `None` and leaves the actor untouched when every maneuver is gated
/// out, as the image does (it keeps `+0x9b4` and `+0x9a0`).
pub fn choose(
    catalog: &ManeuverCatalog,
    actor: &mut ChooserActor,
    roll: u16,
) -> Option<ManeuverChoice> {
    let candidates = candidates(catalog, actor);
    let total: f64 = candidates.iter().map(|c| f64::from(c.weight)).sum();
    let mut remaining = f64::from(roll) * DRAW_SCALE * total;
    let mut winner = candidates.last()?.slot;
    for candidate in &candidates {
        remaining -= f64::from(candidate.weight);
        if remaining < 0.0 {
            winner = candidate.slot;
            break;
        }
    }
    Some(adopt(catalog, actor, winner))
}

/// The named-maneuver path (`0x43e10e`): no gate runs, so a tangled actor can be
/// forced into a `nitro` maneuver, though its injector still stays off.
/// Returns `None` for a slot outside the catalog.
pub fn choose_named(
    catalog: &ManeuverCatalog,
    actor: &mut ChooserActor,
    slot: usize,
) -> Option<ManeuverChoice> {
    (slot < catalog.len()).then(|| adopt(catalog, actor, slot))
}
