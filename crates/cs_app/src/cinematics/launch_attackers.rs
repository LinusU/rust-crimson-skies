//! #1680 (`M01-OUTCOME-A-ATTACKER`): which hostile aircraft can engage the M01
//! player in the first ~70 s, and what activates them.
//!
//! The finding is `docs/findings/2026-10-19-m01-launch-attackers.md`. This
//! module is the production reader behind it, in two halves:
//!
//! * [`survey_cinematic_fighters`] follows the launch cinematic's
//!   `CALL_ANIMATION` closure in `cam_anim.zbd` and reports which records
//!   carry the `piratefighter` scene node and which event opcodes the whole
//!   closure uses. It decides nothing about hostility: it reports what the
//!   records state.
//! * [`ObjectivesWakeGraph`] reads a mission's `objectives.zrd` into the
//!   wake-relevant facts of every objective (`BEGIN_DORMANT`, the
//!   `WAKE_OBJECTIVE_WHEN_I_COMPLETE` / `NAP_OBJECTIVE_WHEN_I_COMPLETE` edges,
//!   the `WAKEUP_*` sites, the `TRAVELERS` condition) so a reader can ask which
//!   objectives wake on their own and which are reachable from which.
//!
//! The measured meaning of the directives is
//! `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
//! (`BEGIN_DORMANT`, lifecycle) and `...-directive-c-...` (`WAKEUP_*`). Nothing
//! here is `verified_original`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_content::stunts::ZrdValue;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    AnimationRecord, AnimationRecordSequenceKind, AnimationRecordTableKind, HeaderStatus,
    ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index,
};
use cs_types::install::RelativePath;

use crate::animation::events::{EventClass, decode_event_stream};

/// The scene node the launch animations place for the pirate fighters.
pub const PIRATE_FIGHTER_NODE: &str = "piratefighter";

/// `CALL_ANIMATION`.
const OP_CALL_ANIMATION: u8 = 24;

/// Why a reader refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttackerReadError {
    /// The camera carrier is not the measured shape.
    Carrier(String),
    /// A record the chain needs is absent or ambiguous.
    Record(String),
    /// `objectives.zrd` is not the measured shape.
    Objectives(String),
}

impl fmt::Display for AttackerReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Carrier(what) => write!(f, "camera carrier: {what}"),
            Self::Record(what) => write!(f, "animation record: {what}"),
            Self::Objectives(what) => write!(f, "objectives program: {what}"),
        }
    }
}

impl std::error::Error for AttackerReadError {}

/// How often one opcode occurs in the closure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpcodeCount {
    /// The installation's statement spelling.
    pub statement: &'static str,
    /// This project's class of it.
    pub class: EventClass,
    /// Occurrences over every sequence (reset and damage included).
    pub count: usize,
}

/// What the launch cinematic's animation closure states about the fighters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CinematicFighterSurvey {
    /// The records reached from the intro through `CALL_ANIMATION`, the intro
    /// first, in discovery order.
    pub reached: Vec<String>,
    /// The subset of [`Self::reached`] whose `Nodes` table names
    /// [`PIRATE_FIGHTER_NODE`].
    pub with_fighter_node: Vec<String>,
    /// Called names with no record in this carrier (they live in another
    /// container, or nowhere).
    pub called_elsewhere: Vec<String>,
    /// Every opcode of every reached record, counted.
    pub opcodes: BTreeMap<u8, OpcodeCount>,
}

impl CinematicFighterSurvey {
    /// Whether any reached record uses an opcode of `class`.
    #[must_use]
    pub fn uses_class(&self, class: EventClass) -> bool {
        self.opcodes.values().any(|o| o.class == class)
    }
}

fn text(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

fn names_node(record: &AnimationRecord<'_>, wanted: &str) -> bool {
    record
        .table(AnimationRecordTableKind::Nodes)
        .is_some_and(|table| (0..table.count()).any(|i| table.name(i) == Some(wanted.as_bytes())))
}

/// Surveys the closure of `CALL_ANIMATION` events reachable from
/// `intro_animation` in the camera carrier `bytes`.
///
/// # Errors
///
/// [`AttackerReadError`] naming the first link that is absent or not the
/// measured shape.
pub fn survey_cinematic_fighters(
    path: &RelativePath,
    bytes: &[u8],
    intro_animation: &str,
) -> Result<CinematicFighterSurvey, AttackerReadError> {
    let carrier = |what: String| AttackerReadError::Carrier(what);
    let key = path.logical_key();
    let mut context = ParseContext::with_defaults(&key);
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    let decision = dispatch(ZbdProbe::new(&key, path, &bytes[..needed]))
        .map_err(|error| carrier(format!("{key}: dispatch refused ({})", error.code())))?;
    if !matches!(decision.header_status(), HeaderStatus::Validated { .. })
        || decision.family() != ZbdFamily::Animation
    {
        return Err(carrier(format!(
            "{key}: not a validated animation container"
        )));
    }
    let index = read_animation_index(&mut context, decision, bytes)
        .map_err(|error| carrier(format!("{key}: index ({})", error.code())))?;
    let payload = index
        .payload()
        .map_err(|error| carrier(format!("{key}: payload ({})", error.code())))?;
    let records = payload
        .records()
        .map_err(|error| carrier(format!("{key}: records ({})", error.code())))?;

    let mut reached: Vec<String> = Vec::new();
    let mut queue = vec![intro_animation.to_owned()];
    let mut called_elsewhere: BTreeSet<String> = BTreeSet::new();
    let mut with_fighter_node = Vec::new();
    let mut opcodes: BTreeMap<u8, OpcodeCount> = BTreeMap::new();
    while let Some(name) = queue.pop() {
        if reached.contains(&name) {
            continue;
        }
        let mut found = records.by_anim_name(name.as_bytes());
        let Some(record) = found.next() else {
            if name == intro_animation {
                return Err(AttackerReadError::Record(format!(
                    "no animation record `{name}`"
                )));
            }
            called_elsewhere.insert(name.clone());
            continue;
        };
        if found.next().is_some() {
            return Err(AttackerReadError::Record(format!(
                "`{name}` names more than one record"
            )));
        }
        reached.push(name.clone());
        if names_node(record, PIRATE_FIGHTER_NODE) {
            with_fighter_node.push(name.clone());
        }
        let mut called = Vec::new();
        for sequence in record.sequences() {
            let decoded = decode_event_stream(sequence.events()).map_err(|error| {
                AttackerReadError::Record(format!("{name}/{}: {error}", text(sequence.name())))
            })?;
            for event in decoded {
                opcodes
                    .entry(event.opcode)
                    .or_insert(OpcodeCount {
                        statement: event.statement,
                        class: event.class,
                        count: 0,
                    })
                    .count += 1;
                if event.opcode == OP_CALL_ANIMATION
                    && sequence.kind() != AnimationRecordSequenceKind::Damage
                    && let Some(callee) = event.spelled_name
                {
                    called.push(callee);
                }
            }
        }
        // Depth-first in stated order: later calls are pushed first.
        queue.extend(called.into_iter().rev());
    }
    Ok(CinematicFighterSurvey {
        reached,
        with_fighter_node,
        called_elsewhere: called_elsewhere.into_iter().collect(),
        opcodes,
    })
}

/// What a `TRAVELERS` condition measures the player against.
#[derive(Clone, Debug, PartialEq)]
pub enum TravelTarget {
    /// A named world node.
    Node(String),
    /// A literal point, metres.
    Point([f64; 3]),
}

/// An objective's `TRAVELERS ['player', 'APPROACHING', target, radius, n]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Travelers {
    /// What the player approaches.
    pub target: TravelTarget,
    /// The stated radius.
    pub radius: f32,
}

/// The wake-relevant facts of one objective block.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ObjectiveFacts {
    /// The block's number (`OBJECTIVE<n>`).
    pub number: u32,
    /// Whether the block spells `BEGIN_DORMANT`.
    pub begins_dormant: bool,
    /// `BEGIN_DORMANT`'s first float: a value `>= 0` is the mission-clock
    /// second the block wakes itself at; `< 0` waits for an external wake.
    pub dormant_arg: Option<f32>,
    /// `WAKE_OBJECTIVE_WHEN_I_COMPLETE` targets.
    pub wakes: Vec<u32>,
    /// `NAP_OBJECTIVE_WHEN_I_COMPLETE` `(objective, seconds)` pairs.
    pub naps: Vec<(u32, f32)>,
    /// `WAKEUP_ENEMIES` names.
    pub wakeup_enemies: Vec<String>,
    /// `WAKEUP_ZEP_TURRETS` names.
    pub wakeup_zep_turrets: Vec<String>,
    /// `WAKEUP_TURRETS` names.
    pub wakeup_turrets: Vec<String>,
    /// The block's `TRAVELERS` condition, when it has one.
    pub travelers: Option<Travelers>,
    /// Whether the block spells an `ANIM_STATE` condition.
    pub has_anim_state: bool,
}

impl ObjectiveFacts {
    /// The mission-clock second the block wakes itself at, when it does.
    #[must_use]
    pub fn timed_wake_s(&self) -> Option<f32> {
        self.dormant_arg.filter(|seconds| *seconds >= 0.0)
    }

    /// Whether the block is awake from the first tick: no `BEGIN_DORMANT`.
    #[must_use]
    pub fn awake_at_start(&self) -> bool {
        !self.begins_dormant
    }

    /// Every name any `WAKEUP_*` site of the block lists.
    pub fn woken_names(&self) -> impl Iterator<Item = &str> {
        self.wakeup_enemies
            .iter()
            .chain(&self.wakeup_zep_turrets)
            .chain(&self.wakeup_turrets)
            .map(String::as_str)
    }
}

fn floats_and_ints(list: &[ZrdValue]) -> Vec<f64> {
    list.iter()
        .filter_map(|v| match v {
            ZrdValue::Int(n) => Some(f64::from(*n)),
            ZrdValue::Float(x) => Some(f64::from(*x)),
            _ => None,
        })
        .collect()
}

fn names(list: &[ZrdValue]) -> Vec<String> {
    list.iter()
        .filter_map(|v| v.as_text().map(str::to_owned))
        .collect()
}

fn read_block(number: u32, body: &[ZrdValue]) -> ObjectiveFacts {
    let mut facts = ObjectiveFacts {
        number,
        ..ObjectiveFacts::default()
    };
    let mut i = 0;
    while i < body.len() {
        let Some(key) = body[i].as_text() else {
            i += 1;
            continue;
        };
        let args = body.get(i + 1).and_then(ZrdValue::as_list);
        i += if args.is_some() { 2 } else { 1 };
        let args = args.unwrap_or_default();
        match key {
            "BEGIN_DORMANT" => {
                facts.begins_dormant = true;
                facts.dormant_arg = floats_and_ints(args).first().map(|v| *v as f32);
            }
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE" => facts
                .wakes
                .extend(floats_and_ints(args).into_iter().map(|v| v as u32)),
            "NAP_OBJECTIVE_WHEN_I_COMPLETE" => {
                let numbers = floats_and_ints(args);
                facts.naps.extend(
                    numbers
                        .chunks(2)
                        .filter(|c| c.len() == 2)
                        .map(|c| (c[0] as u32, c[1] as f32)),
                );
            }
            "WAKEUP_ENEMIES" => facts.wakeup_enemies.extend(names(args)),
            "WAKEUP_ZEP_TURRETS" => facts.wakeup_zep_turrets.extend(names(args)),
            "WAKEUP_TURRETS" => facts.wakeup_turrets.extend(names(args)),
            "ANIM_STATE" => facts.has_anim_state = true,
            "TRAVELERS" => {
                if let [who, mode, target, radius, ..] = args
                    && who.as_text() == Some("player")
                    && mode.as_text() == Some("APPROACHING")
                {
                    let target = match target {
                        ZrdValue::Text(node) => Some(TravelTarget::Node(node.clone())),
                        ZrdValue::List(point) => match floats_and_ints(point)[..] {
                            [x, y, z] => Some(TravelTarget::Point([x, y, z])),
                            _ => None,
                        },
                        _ => None,
                    };
                    let radius = floats_and_ints(std::slice::from_ref(radius))
                        .first()
                        .copied();
                    if let (Some(target), Some(radius)) = (target, radius) {
                        facts.travelers = Some(Travelers {
                            target,
                            radius: radius as f32,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    facts
}

/// The wake-relevant facts of every objective of one `objectives.zrd`.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectivesWakeGraph {
    objectives: Vec<ObjectiveFacts>,
}

impl ObjectivesWakeGraph {
    /// Reads the decoded `objectives.zrd` document.
    ///
    /// # Errors
    ///
    /// [`AttackerReadError::Objectives`] when the document is not a list or
    /// holds no `OBJECTIVE<n>` block.
    pub fn read(document: &ZrdValue) -> Result<Self, AttackerReadError> {
        let top = document
            .as_list()
            .ok_or_else(|| AttackerReadError::Objectives("the document is not a list".into()))?;
        // A decoded member is one list holding the flat `name, body, ...` run.
        let top = match top {
            [only] => only.as_list().unwrap_or(top),
            _ => top,
        };
        let mut objectives = Vec::new();
        for pair in top.windows(2) {
            let (Some(key), Some(body)) = (pair[0].as_text(), pair[1].as_list()) else {
                continue;
            };
            if let Some(number) = key
                .strip_prefix("OBJECTIVE")
                .and_then(|n| n.parse::<u32>().ok())
            {
                objectives.push(read_block(number, body));
            }
        }
        if objectives.is_empty() {
            return Err(AttackerReadError::Objectives(
                "no OBJECTIVE<n> block".into(),
            ));
        }
        Ok(Self { objectives })
    }

    /// Every block, in document order.
    #[must_use]
    pub fn objectives(&self) -> &[ObjectiveFacts] {
        &self.objectives
    }

    /// Block `number`.
    #[must_use]
    pub fn get(&self, number: u32) -> Option<&ObjectiveFacts> {
        self.objectives.iter().find(|o| o.number == number)
    }

    /// The blocks that spell a `WAKEUP_ENEMIES`, `WAKEUP_ZEP_TURRETS` or
    /// `WAKEUP_TURRETS` site naming `name` (a literal match).
    #[must_use]
    pub fn blocks_waking(&self, name: &str) -> Vec<u32> {
        self.objectives
            .iter()
            .filter(|o| o.woken_names().any(|n| n == name))
            .map(|o| o.number)
            .collect()
    }

    /// Every block from which `target` is reachable through
    /// `WAKE_OBJECTIVE_WHEN_I_COMPLETE` and `NAP_OBJECTIVE_WHEN_I_COMPLETE`
    /// edges (the transitive predecessors, `target` excluded unless it is on a
    /// cycle).
    #[must_use]
    pub fn ancestors(&self, target: u32) -> BTreeSet<u32> {
        let mut found = BTreeSet::new();
        let mut frontier = vec![target];
        while let Some(node) = frontier.pop() {
            for o in &self.objectives {
                let edge = o.wakes.contains(&node) || o.naps.iter().any(|(n, _)| *n == node);
                if edge && found.insert(o.number) {
                    frontier.push(o.number);
                }
            }
        }
        found
    }
}
