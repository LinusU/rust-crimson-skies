//! #1674 (`PARITY-M01-LC-MEASURE-CAM-TIMELINE`): how the launch cinematic's
//! scenes follow one another, and which camera variants can play.
//!
//! The finding is `docs/findings/2026-10-18-parity-m01-lc-cinematic-timeline.md`.
//! Everything here was read statically from the decrypted engine image
//! (`$CS_ENGINE_IMAGE`) and the animation records; no original executable ran,
//! so nothing is `verified_original`.
//!
//! ## What the scheduler does (static)
//!
//! The per-frame update of an animation instance (`0x4ecd20`) adds the frame's
//! `game_dt` (`[0x9ad744]`, seconds, at most 0.125 s) to the instance clock
//! (`[instance + 0xb0]`) and steps every sequence in state 0 or 1 (`0x4ecbb0`).
//! A sequence runs its events **in order**; the next event waits until a clock
//! reaches its `START_TIME`, and which clock is the **second byte of the
//! event's tag** (the `group`):
//!
//! | group | the event waits for | where |
//! | --- | --- | --- |
//! | 1 | the instance clock (absolute) | `0x4ecc3f`..`0x4ecc4d` |
//! | 2 | the sequence clock `[sequence + 0x24]` | `0x4ecc2c`..`0x4ecc37` |
//! | 3 | the time since the previous event ran, `[sequence + 0x28]` | `0x4ecc19`..`0x4ecc24` |
//!
//! `CALL_ANIMATION` (`0x4eb6b0`) starts the named child instance without
//! waiting for it (the wait bit, event word `+0x2e` bit `0x10`, is clear in
//! every `generic_intro` call); `CALL_SEQUENCE` (`0x4eb570`) turns a dormant
//! sequence (state 3, the `0x303` sequences) into a running one; `IF` with
//! condition word `0x2000` (`0x4ec1c8`) is true when the node named by the
//! event's `+0x10` entry of the record's `Nodes` table is bound and has bit
//! `0x4` of its flag word `+0x24` set, which is the bit `OBJECT_ACTIVE_STATE`
//! (`0x4cca30`) sets and clears.
//!
//! [`measure_intro_timeline`] replays that for `generic_intro` in continuous
//! time, i.e. with an update step of zero. The real engine fires an event on
//! the first update whose clock has reached the wait, so each event runs late
//! by a part of one frame, never early ([`MAX_UPDATE_DELTA_SECONDS`]).
//!
//! ## What is refused
//!
//! A group-2 wait, a group-3 wait on the first event of a sequence (the
//! engine accumulates no sequence time before the first event has run, so
//! such an event never fires; the data has none), an `IF` condition other
//! than `0x2000`, nested or chained `IF`s, events after a `CALL_SEQUENCE`
//! (the called sequence runs concurrently), and any opcode the replay does not
//! know. Those are errors, not guesses.

use std::fmt;

use cs_formats::zbd::{AnimationRecord, AnimationRecordTableKind};

use crate::animation::events::{RawEvent, decode_event_stream, walk_event_stream};

/// The animation the launch starts with (`startanims.zrd`'s `NEW_GAME_START`).
pub const LAUNCH_INTRO_ANIMATION: &str = "generic_intro";
/// The sequence of that record the replay starts from. It is one of the two
/// flag-0 (not dormant) sequences, so it runs when the instance starts.
pub const START_SEQUENCE: &str = "start_script";

/// The largest `game_dt` the frame clock hands out: `0x63b158` = 0.125 s, the
/// default clamp (`0x63b160` = 1) of F16-F
/// (`docs/findings/2026-10-06-f16-f-original-clock-pause-and-speed-up-policy.md`).
/// An event can therefore run up to this much after the time
/// [`measure_intro_timeline`] gives, once per wait.
pub const MAX_UPDATE_DELTA_SECONDS: f32 = 0.125;

/// `CALL_SEQUENCE`.
const OP_CALL_SEQUENCE: u8 = 22;
/// `CALL_ANIMATION`.
const OP_CALL_ANIMATION: u8 = 24;
/// `STOP_ANIMATION`.
const OP_STOP_ANIMATION: u8 = 25;
/// `IF`.
const OP_IF: u8 = 31;
/// `ELSE`.
const OP_ELSE: u8 = 32;
/// `ENDIF`.
const OP_ENDIF: u8 = 34;
/// `OBJECT_MOTION_SI_SCRIPT`.
const OP_MOTION_SI_SCRIPT: u8 = 12;

/// The `IF` condition bit that draws from the random table (`0x4ec09c`).
pub const CONDITION_RANDOM: u32 = 0x1;
/// The `IF` condition bit that tests a node's active flag (`0x4ec1c8`).
pub const CONDITION_NODE_ACTIVE: u32 = 0x2000;

/// A fact about the cinematic's timing or branching that no measurement
/// settles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineUnknown {
    /// Stable identifier.
    pub id: &'static str,
    /// What is not known.
    pub what: &'static str,
    /// The content that depends on it.
    pub affects: &'static str,
    /// Who resolves it, or what would.
    pub resolver: &'static str,
}

/// Every fact [`measure_intro_timeline`] does not claim.
pub const TIMELINE_UNKNOWNS: &[TimelineUnknown] = &[
    TimelineUnknown {
        id: "start-trigger",
        what: "what starts `generic_intro` (and so where the cinematic's t = 0 \
               lies relative to the Flight Check screen, the mission load and \
               the first rendered frame)",
        affects: "the wall-clock moment of every cut",
        resolver: "#1663",
    },
    TimelineUnknown {
        id: "active-flag-of-the-player-roots",
        what: "which of the eleven `player_<airframe>` scene roots has bit 0x4 \
               of its flag word set when the cinematic runs (the `IF` reads \
               that bit; the code that sets it for the chosen airframe is not \
               traced)",
        affects: "whether the first-person shot `gi_1stperson` plays; with \
                  none of the two roots active it does",
        resolver: "#1663",
    },
    TimelineUnknown {
        id: "m01-airframe",
        what: "which airframe the player flies in M01; the data states none \
               (`2026-10-06-m01-lc-player-airframe-source.md`)",
        affects: "the cinematic's length (40.15 s or 30.25 s)",
        resolver: "#1580 (owner-confirmed airframe) and #1663",
    },
    TimelineUnknown {
        id: "draw-index",
        what: "how many draws of the shared random table earlier consumers \
               took before the cinematic's first random `IF` (one cursor, \
               `[0x72836c]`, shared with the random-range event at `0x4e90aa`)",
        affects: "which camera variant plays, given the seed",
        resolver: "an original run; not resolvable statically",
    },
    TimelineUnknown {
        id: "frame-quantisation",
        what: "the frame deltas of an original run (the update step is the \
               variable `game_dt`, at most 0.125 s)",
        affects: "each cut by up to one frame late, per wait",
        resolver: "an original run",
    },
];

/// Why the timeline could not be replayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineError {
    /// The record has no sequence by that name.
    NoSequence(String),
    /// The event stream does not walk or decode.
    Stream(String),
    /// A measured-shape expectation failed: the replay refuses instead of
    /// guessing (see the module documentation).
    Unmeasured(String),
}

impl fmt::Display for TimelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSequence(name) => write!(f, "the record has no sequence `{name}`"),
            Self::Stream(reason) => write!(f, "the event stream cannot be read: {reason}"),
            Self::Unmeasured(reason) => write!(f, "not a measured shape: {reason}"),
        }
    }
}

impl std::error::Error for TimelineError {}

/// One animation the intro starts, and when it stops it.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSpan {
    /// The `CALL_ANIMATION` name.
    pub animation: String,
    /// The instance-clock time the call runs, in seconds.
    pub started_at: f64,
    /// The time its `STOP_ANIMATION` runs; `None` when nothing stops it.
    pub stopped_at: Option<f64>,
}

impl SceneSpan {
    /// `stopped_at - started_at`, when it is stopped.
    #[must_use]
    pub fn length(&self) -> Option<f64> {
        self.stopped_at.map(|stop| stop - self.started_at)
    }
}

/// The replayed timeline of the launch cinematic.
#[derive(Clone, Debug, PartialEq)]
pub struct IntroTimeline {
    /// Every animation the replay started, in start order.
    pub scenes: Vec<SceneSpan>,
    /// The sequences that ran, in the order they started.
    pub sequences: Vec<String>,
    /// When the last event ran, in seconds: the cinematic's length in
    /// continuous time.
    pub length: f64,
}

impl IntroTimeline {
    /// The span of the named animation.
    #[must_use]
    pub fn scene(&self, animation: &str) -> Option<&SceneSpan> {
        self.scenes.iter().find(|span| span.animation == animation)
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// The name an event spells at `+0xc` (`CALL_SEQUENCE`, `CALL_ANIMATION`,
/// `STOP_ANIMATION`).
fn spelled(bytes: &[u8], event: &RawEvent) -> Result<String, TimelineError> {
    let at = event.offset as usize;
    let body = bytes
        .get(at + 0xc..at + event.length as usize)
        .ok_or_else(|| TimelineError::Stream("an event without a name field".into()))?;
    let name = text(body);
    if name.is_empty() {
        return Err(TimelineError::Unmeasured(format!(
            "an unnamed call at {at:#x}"
        )));
    }
    Ok(name)
}

/// The parts of an animation record the replay reads: its sequences' event
/// streams, its `Nodes` names and its `IndexWords` entries. Built from a
/// record by [`RecordView::from_record`]; tests author one byte by byte.
#[derive(Clone, Debug, Default)]
pub struct RecordView<'a> {
    /// `(name, event stream)` of every sequence, in stored order.
    pub sequences: Vec<(String, &'a [u8])>,
    /// The `Nodes` table's names, by index.
    pub nodes: Vec<String>,
    /// The `IndexWords` table's entries, by index.
    pub index_words: Vec<u32>,
}

impl<'a> RecordView<'a> {
    /// Reads the view of a record.
    ///
    /// # Errors
    ///
    /// [`TimelineError::Unmeasured`] when an `IndexWords` entry is not four
    /// bytes.
    pub fn from_record(record: &AnimationRecord<'a>) -> Result<Self, TimelineError> {
        let names = |kind| {
            record
                .table(kind)
                .map(|t| (0..).map_while(|i| t.name(i)).map(text).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let index_words = match record.table(AnimationRecordTableKind::IndexWords) {
            Some(table) => (0..)
                .map_while(|i| table.entry(i))
                .map(|raw| {
                    raw.try_into().map(u32::from_le_bytes).map_err(|_| {
                        TimelineError::Unmeasured("an IndexWords entry is not four bytes".into())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        Ok(Self {
            sequences: record
                .sequences()
                .iter()
                .map(|s| (text(s.name()), s.events()))
                .collect(),
            nodes: names(AnimationRecordTableKind::Nodes),
            index_words,
        })
    }
}

struct Ctx<'r, 'a> {
    record: &'r RecordView<'a>,
    active_nodes: &'r [&'r str],
    scenes: Vec<SceneSpan>,
    sequences: Vec<String>,
    length: f64,
    depth: usize,
}

impl Ctx<'_, '_> {
    fn node_is_active(&self, index: u32) -> Result<bool, TimelineError> {
        let name = self
            .record
            .nodes
            .get(index as usize)
            .ok_or_else(|| TimelineError::Unmeasured(format!("no node {index}")))?;
        Ok(self.active_nodes.contains(&name.as_str()))
    }

    fn run(&mut self, name: &str, origin: f64) -> Result<(), TimelineError> {
        self.depth += 1;
        if self.depth > 8 {
            return Err(TimelineError::Unmeasured(
                "sequence calls nest deeper than the data does".into(),
            ));
        }
        let (_, bytes) = *self
            .record
            .sequences
            .iter()
            .find(|(n, _)| n == name)
            .ok_or_else(|| TimelineError::NoSequence(name.to_owned()))?;
        let walked = walk_event_stream(bytes).map_err(|e| TimelineError::Stream(e.to_string()))?;
        let decoded =
            decode_event_stream(bytes).map_err(|e| TimelineError::Stream(e.to_string()))?;
        self.sequences.push(name.to_owned());

        // The time the previous event of this sequence ran (the sequence's
        // origin before the first), and the IF state of the walk.
        let mut last = origin;
        let mut ran_any = false;
        let mut called_sequence = false;
        let mut branch: Option<bool> = None; // `Some(taken)` inside an IF.
        let mut in_else = false;
        for (event, decoded) in walked.iter().zip(&decoded) {
            match event.opcode {
                OP_IF => {
                    if branch.is_some() {
                        return Err(TimelineError::Unmeasured("nested IF".into()));
                    }
                    let at = event.offset as usize;
                    let condition = word(bytes, at + 0xc);
                    if condition != CONDITION_NODE_ACTIVE {
                        return Err(TimelineError::Unmeasured(format!(
                            "IF condition word {condition:#x}"
                        )));
                    }
                    branch = Some(self.node_is_active(word(bytes, at + 0x10))?);
                    in_else = false;
                    continue;
                }
                OP_ELSE => {
                    if branch.is_none() || in_else {
                        return Err(TimelineError::Unmeasured("ELSE outside an IF".into()));
                    }
                    in_else = true;
                    continue;
                }
                OP_ENDIF => {
                    if branch.take().is_none() {
                        return Err(TimelineError::Unmeasured("ENDIF outside an IF".into()));
                    }
                    in_else = false;
                    continue;
                }
                _ => {}
            }
            if branch == Some(in_else) {
                continue; // the branch not taken
            }
            if called_sequence {
                return Err(TimelineError::Unmeasured(
                    "an event after CALL_SEQUENCE (the callee runs concurrently)".into(),
                ));
            }
            let wait = f64::from(decoded.start_time);
            let ran_at = match event.group {
                1 => last.max(wait),
                3 => {
                    if !ran_any && wait != 0.0 {
                        return Err(TimelineError::Unmeasured(
                            "a group-3 wait on the first event never fires".into(),
                        ));
                    }
                    last + wait
                }
                other => {
                    return Err(TimelineError::Unmeasured(format!("a group-{other} wait")));
                }
            };
            last = ran_at;
            ran_any = true;
            self.length = self.length.max(ran_at);
            match event.opcode {
                OP_CALL_ANIMATION => {
                    let animation = spelled(bytes, event)?;
                    self.scenes.push(SceneSpan {
                        animation,
                        started_at: ran_at,
                        stopped_at: None,
                    });
                }
                OP_STOP_ANIMATION => {
                    let animation = spelled(bytes, event)?;
                    let span = self
                        .scenes
                        .iter_mut()
                        .rev()
                        .find(|s| s.animation == animation && s.stopped_at.is_none())
                        .ok_or_else(|| {
                            TimelineError::Unmeasured(format!(
                                "`{animation}` stopped before it started"
                            ))
                        })?;
                    span.stopped_at = Some(ran_at);
                }
                OP_CALL_SEQUENCE => {
                    called_sequence = true;
                    let callee = spelled(bytes, event)?;
                    self.run(&callee, ran_at)?;
                }
                // Pose, state and sound events take no time in this replay.
                _ => {}
            }
        }
        if branch.is_some() {
            return Err(TimelineError::Unmeasured("an IF without ENDIF".into()));
        }
        self.depth -= 1;
        Ok(())
    }
}

/// Replays `generic_intro`'s `start_script` in continuous time.
///
/// `active_nodes` names the nodes of the record's `Nodes` table that are bound
/// and active when the cinematic runs (for the launch, the player's
/// `player_<airframe>` scene root). A node that is not listed counts as not
/// bound or not active; both make `IF` condition `0x2000` false in the engine
/// (`0x4ec1d5`..`0x4ec1e9`).
///
/// # Errors
///
/// [`TimelineError`] when the record is not the measured shape.
pub fn measure_intro_timeline(
    record: &RecordView<'_>,
    active_nodes: &[&str],
) -> Result<IntroTimeline, TimelineError> {
    let mut ctx = Ctx {
        record,
        active_nodes,
        scenes: Vec::new(),
        sequences: Vec::new(),
        length: 0.0,
        depth: 0,
    };
    ctx.run(START_SEQUENCE, 0.0)?;
    Ok(IntroTimeline {
        scenes: ctx.scenes,
        sequences: ctx.sequences,
        length: ctx.length,
    })
}

/// One camera-variant chain: a scene that picks one of several tracks by
/// successive random `IF`s.
#[derive(Clone, Debug, PartialEq)]
pub struct VariantChain {
    /// The record (scene) the chain lives in.
    pub scene: String,
    /// The thresholds of the successive `IF`s: the variant `k` is chosen when
    /// the `k`-th draw is `<=` `thresholds[k]` and every earlier one was not.
    pub thresholds: Vec<f32>,
    /// The `IndexWords` table entries (track block numbers) of the variants,
    /// one more than `thresholds`: the last is the unconditional fallback.
    pub track_blocks: Vec<u32>,
}

impl VariantChain {
    /// The probability of each variant for uniform draws in `[0, 1)`, assuming
    /// each `IF` takes its own draw: `p_k = theta_k * prod_{j<k}(1 - theta_j)`,
    /// the last variant taking the remainder.
    #[must_use]
    pub fn probabilities(&self) -> Vec<f64> {
        let mut left = 1.0_f64;
        let mut out = Vec::with_capacity(self.track_blocks.len());
        for theta in &self.thresholds {
            let theta = f64::from(*theta);
            out.push(left * theta);
            left *= 1.0 - theta;
        }
        out.push(left);
        out
    }
}

/// Reads every camera-variant chain of one scene record.
///
/// A chain head is a sequence that holds a random `IF` and that no
/// `CALL_SEQUENCE` of the record calls. Each `IF` is followed by one
/// `OBJECT_MOTION_SI_SCRIPT` and an `ELSE` that holds either the next
/// `OBJECT_MOTION_SI_SCRIPT` of the same sequence or a `CALL_SEQUENCE` to the
/// next link.
///
/// # Errors
///
/// [`TimelineError`] when a chain is not that shape.
pub fn variant_chains(
    scene: &str,
    record: &RecordView<'_>,
) -> Result<Vec<VariantChain>, TimelineError> {
    struct Seq {
        name: String,
        events: Vec<(RawEvent, Vec<u8>)>,
    }
    let mut sequences = Vec::new();
    for (name, bytes) in &record.sequences {
        let bytes = *bytes;
        let walked = walk_event_stream(bytes).map_err(|e| TimelineError::Stream(e.to_string()))?;
        let events = walked
            .into_iter()
            .map(|e| {
                let at = e.offset as usize;
                (e, bytes[at..at + e.length as usize].to_vec())
            })
            .collect();
        sequences.push(Seq {
            name: name.clone(),
            events,
        });
    }
    let called: Vec<String> = sequences
        .iter()
        .flat_map(|s| &s.events)
        .filter(|(e, _)| e.opcode == OP_CALL_SEQUENCE)
        .map(|(e, b)| text(&b[0xc..e.length as usize]))
        .collect();
    let block_of = |event: &[u8]| -> Result<u32, TimelineError> {
        let entry = word(event, 0x14) as usize;
        record
            .index_words
            .get(entry)
            .copied()
            .ok_or_else(|| TimelineError::Unmeasured(format!("no IndexWords entry {entry}")))
    };

    let mut chains = Vec::new();
    for head in &sequences {
        let has_random = head
            .events
            .iter()
            .any(|(e, b)| e.opcode == OP_IF && word(b, 0xc) & CONDITION_RANDOM != 0);
        if !has_random || (!head.name.is_empty() && called.contains(&head.name)) {
            continue;
        }
        let mut chain = VariantChain {
            scene: scene.to_owned(),
            thresholds: Vec::new(),
            track_blocks: Vec::new(),
        };
        let mut link = head;
        loop {
            let mut events = link.events.iter().map(|(e, b)| (e.opcode, b.as_slice()));
            let mut next: Option<&Seq> = None;
            while let Some((opcode, body)) = events.next() {
                if opcode != OP_IF {
                    // Only state events (`OBJECT_ACTIVE_STATE`) precede the IF.
                    continue;
                }
                if word(body, 0xc) != CONDITION_RANDOM {
                    return Err(TimelineError::Unmeasured(format!(
                        "IF condition word {:#x}",
                        word(body, 0xc)
                    )));
                }
                chain.thresholds.push(f32::from_bits(word(body, 0x14)));
                let (op, hit) = events
                    .next()
                    .ok_or_else(|| TimelineError::Unmeasured("an IF without a body".into()))?;
                if op != OP_MOTION_SI_SCRIPT {
                    return Err(TimelineError::Unmeasured(format!("IF body opcode {op}")));
                }
                chain.track_blocks.push(block_of(hit)?);
                match events.next() {
                    Some((OP_ELSE, _)) => {}
                    _ => return Err(TimelineError::Unmeasured("an IF without ELSE".into())),
                }
                match events.next() {
                    Some((OP_CALL_SEQUENCE, body)) => {
                        let callee = text(&body[0xc..]);
                        next = Some(
                            sequences
                                .iter()
                                .find(|s| s.name == callee)
                                .ok_or(TimelineError::NoSequence(callee))?,
                        );
                    }
                    Some((OP_MOTION_SI_SCRIPT, body)) => {
                        chain.track_blocks.push(block_of(body)?);
                    }
                    _ => return Err(TimelineError::Unmeasured("an ELSE body".into())),
                }
                match events.next() {
                    Some((OP_ENDIF, _)) => {}
                    _ => return Err(TimelineError::Unmeasured("an IF without ENDIF".into())),
                }
                break;
            }
            match next {
                Some(callee) => {
                    link = callee;
                    // The callee either holds the next IF or, last in the chain,
                    // only the unconditional motion event.
                    let has_if = link.events.iter().any(|(e, _)| e.opcode == OP_IF);
                    if !has_if {
                        let (_, body) = link
                            .events
                            .iter()
                            .find(|(e, _)| e.opcode == OP_MOTION_SI_SCRIPT)
                            .ok_or_else(|| {
                                TimelineError::Unmeasured("a chain link without a track".into())
                            })?;
                        chain.track_blocks.push(block_of(body)?);
                        break;
                    }
                }
                None => break,
            }
        }
        if chain.track_blocks.len() != chain.thresholds.len() + 1 {
            return Err(TimelineError::Unmeasured(
                "a chain without exactly one fallback".into(),
            ));
        }
        chains.push(chain);
    }
    Ok(chains)
}
