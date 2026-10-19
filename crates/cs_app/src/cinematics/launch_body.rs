//! #1685 (`PARITY-M01-ATMO-E5`): what writes the M01 player body before the
//! launch cinematic's callback 10, and what ends the cinematic state.
//!
//! The finding is `docs/findings/2026-10-19-m01-launch-body-pose.md`. It
//! continues [`super::launch_release`] (#1678), whose `body-state-at-intro-start`,
//! `node-detach-after-intro` and `cinematic-flag-clear` unknowns it settles as
//! far as the image and the retail container allow. Everything about the engine
//! is static (`$CS_ENGINE_IMAGE`, provenance `static`); no original run exists
//! and nothing is `verified_original`.
//!
//! What the retail container states is read, not assumed
//! ([`measure_intro_body_events`]): every event of the intro and of the
//! animations it transitively calls that moves the player node between
//! parents, fires one of the callbacks that touch the player body, or calls
//! another animation, with the sequence it sits in and its `START_TIME`.
//! What the engine does with them is [`BODY_ENGINE_FACTS`].

use cs_formats::zbd::{AnimationRecordSequenceKind, AnimationRecords};
use cs_types::install::RelativePath;

use super::launch_end_pose::{
    LaunchEndPoseError, PLAYER_NODE, half, node_name, record_named, text, word,
};
use super::launch_release::with_records;
use crate::animation::events::{decode_event_stream, walk_event_stream};

/// `OBJECT_ADD_CHILD`.
const OP_ADD_CHILD: u8 = 15;
/// `OBJECT_DELETE_CHILD`.
const OP_DELETE_CHILD: u8 = 16;
/// `CALL_ANIMATION`.
const OP_CALL_ANIMATION: u8 = 24;
/// `CALLBACK`.
const OP_CALLBACK: u8 = 35;

/// The callback ids whose player handler (`0x47e080`) writes the body, the
/// node, the cinematic flags or `[0x71c50c]`.
pub const BODY_CALLBACK_IDS: &[u32] = &[0, 10, 11, 12, 20, 951];

/// A statement about the engine, read from the image and not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyFact {
    /// Stable identifier.
    pub id: &'static str,
    /// Virtual address in the image (`file offset + 0x400000`).
    pub va: u32,
    /// What the code at `va` does, as read.
    pub claim: &'static str,
}

/// The static facts the finding rests on; provenance of every row is `static`.
/// `accept_parity_m01_atmo_e5_engine_facts_are_the_bytes_the_finding_read`
/// re-reads the instruction bytes behind each.
pub const BODY_ENGINE_FACTS: &[BodyFact] = &[
    BodyFact {
        id: "player-vehicle-pointer",
        va: 0x473060,
        claim: "`0x473060(veh)` stores its argument in `[0x71c298]`, the player vehicle every \
                other row means by `veh`. The only callers are `0x475397` (after the mission's \
                vehicle list is loaded: the first list entry), `0x47bc35` and `0x48003c`",
    },
    BodyFact {
        id: "vehicle-constructor-zeroes-pose",
        va: 0x4b0776,
        claim: "the vehicle constructor `0x4aff80` (called from the loader at `0x47c51d`) stores \
                the zero vector `0x75d1b8` in `veh+0x1f8` (euler) and `veh+0x204` (position), \
                and zero in `veh+0x150` (matrix) and `veh+0x1a4`: a constructed body has no \
                pose of its own",
    },
    BodyFact {
        id: "loader-places-the-node",
        va: 0x47c4c0,
        claim: "the loader `0x47c210` sets the player's scene node through `0x4d1d50` (position \
                from the record's `+0x18..+0x20`) and `0x4d1a30` (euler `(0, record+0x24 * \
                [0x6040e8], 0)`) before the vehicle exists",
    },
    BodyFact {
        id: "loader-copies-node-to-body",
        va: 0x47c77b,
        claim: "after constructing the vehicle the loader calls `0x476250(veh)`, which reads \
                the node's world position (`0x4cf200`) into `veh+0x204`, its stored euler \
                angles (`0x4d1b40`) into `veh+0x1f8..0x200`, builds the matrices \
                `veh+0x150`/`0x180` and the copy `veh+0x1a4`, and sets `veh+0x91d = 1`, \
                `veh+0x91e = 0`. This is the only code read that gives a new body its pose",
    },
    BodyFact {
        id: "mission-load-clears-then-sets-the-flags",
        va: 0x46563f,
        claim: "`0x4735b0` loads the aiv.zrd vehicle list, registers the first list entry as \
                the player (`0x475397`) and clears `veh+0x91d/0x91f/0x91e` (`0x4753a1`); its \
                caller then stores `veh+0x91d = 1`, `veh+0x91e = 0` (`0x46563f`/`0x465646`) \
                and, straight after, calls `0x46c370` with `StartAnims.zrd` and \
                `NEW_GAME_START` (`0x625e60`, `0x625e50`): the launch intro starts with \
                `veh+0x91d` set",
    },
    BodyFact {
        id: "pilot-record-is-not-the-body",
        va: 0x47b1fe,
        claim: "the stores at `0x47b1fe`/`0x47b2e8` (`[edi+0x204]`) belong to the pilot \
                attribute reader: the keys it passes to `0x57a090` are `dare_devil`, \
                `natural_touch`, `sixth_sense`, `stun_recovery`, \
                `preferred_engagement_altitude` (`0x628104`..`0x628160`). The constructor \
                `0x478d80` family writes the same offsets of that pilot struct",
    },
    BodyFact {
        id: "place-routine-intro-branch",
        va: 0x47f25f,
        claim: "`0x47f1f0(0, ...)` with `veh+0x91d` set: `veh+0x208 += [0x6032e4]` (100.0), \
                `veh+0x124 = 0x3ecccccd` (0.4), velocity `veh+0x924 = 0x53fb40(veh+0x150, \
                (0,0,1) at 0x6379d0) * [0x608040]` (-20.0 per component)",
    },
    BodyFact {
        id: "place-routine-writes-node-and-clears-flags",
        va: 0x47f5ea,
        claim: "the tail of `0x47f1f0` writes `veh+0x204..0x20c` to the node through `0x4d1d50` \
                and `veh+0x1f8..0x200` through `0x4d1a30`, then stores zero to \
                `veh+0x91d`, `0x91f`, `0x91e` (`0x47f625`..`0x47f631`)",
    },
    BodyFact {
        id: "callback-11-sets-the-flags",
        va: 0x47e1ae,
        claim: "the id-11 handler `0x47e19e` stores `veh+0x91d = 1`, `veh+0x91e = 0`, then \
                `veh+0x91e = 1`, and calls `0x455800(0)`",
    },
    BodyFact {
        id: "callback-12-starts-the-follow-up",
        va: 0x47e1ed,
        claim: "the id-12 handler `0x47e1ed` runs only while `veh+0x91d` is set and \
                `veh+0x91f` is clear; single-player it hides the node `veh+0x3a0` (`0x4cca30`) \
                and, if `[0x71bb6c]` is zero, starts the animation named by `[0x71bb68]` \
                through `0x4edda0`",
    },
    BodyFact {
        id: "callback-0-clears-the-intro-marker",
        va: 0x47e0bc,
        claim: "the id-0 handler stores zero to `[0x71c50c]` when its instance argument equals \
                the remembered intro instance; it touches no flag and no pose",
    },
    BodyFact {
        id: "release-handler-copies-node-to-body",
        va: 0x47e557,
        claim: "the id-951 handler reads the node's world position (`0x4cf200`) and rotation \
                (`0x4cf380`) into `veh+0x204`, `veh+0x150`, `veh+0x1a4`, detaches the node \
                (`0x4cd6d0`) if it has a parent of kind 5 and rewrites it (`0x4d1d50`, \
                `0x4d1a30`)",
    },
    BodyFact {
        id: "animation-tick-steps-the-reset-block",
        va: 0x4ed2c7,
        claim: "the per-frame tick `0x4ed2a0` adds the frame time to the instance clock \
                (`+0xb0`) and steps the RESET_SEQUENCE block `[instance+0xd0]` through \
                `0x4ecbb0`; when the block is finished it ends the instance through \
                `0x4ed190` (`0x4ed332`)",
    },
    BodyFact {
        id: "skip-key-path",
        va: 0x4a02ac,
        claim: "while `[0x71c50c]` is set, a key test (`0x536000`) clears it and restarts the \
                remembered instance through `0x4ed480(instance, 0)`",
    },
];

/// One thing the finding leaves open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyUnknown {
    /// Stable identifier.
    pub id: &'static str,
    /// What is not known.
    pub what: &'static str,
    /// The content that depends on it.
    pub affects: &'static str,
    /// What would resolve it.
    pub resolver: &'static str,
}

/// Every link the finding does not claim.
pub const BODY_UNKNOWNS: &[BodyUnknown] = &[
    BodyUnknown {
        id: "node-detach-after-intro",
        what: "what takes the player node out of the Pandora (`piratezep`) after the drop. The \
               four child events of the whole chain sit at START_TIME 0 in the reset block \
               and in `start_script`; nothing later moves the node. The only code read that \
               detaches a node and copies its world pose into the body is the id-951 \
               handler `0x47e557`, and the only 951 sites in C3/M01 are `texdrop` and \
               `pzep_launch_player`, which no data names in a `CALL_ANIMATION` and no \
               engine string names",
        affects: "the frame of the node after the intro, and the pose the first control \
                  frame starts from (the body keeps the callback-10 pose while the node \
                  keeps the last drop pose in the Pandora's frame)",
        resolver: "tracing what writes the node from the body each frame (`0x4b3050` and \
                   callers) against a node that still has a parent, or the owner's capture",
    },
    BodyUnknown {
        id: "cinematic-flag-clear",
        what: "which code clears `veh+0x91d/0x91e` after the intro. Callback 10 clears them \
               (`0x47f625`..`0x47f631`) and callback 11, which runs after it in the same \
               start, sets them again; callback 0 touches only `[0x71c50c]`. The other \
               clear sites (`0x4763df`, `0x47fecd`, `0x48049e`, `0x4805ad`, `0x493e4e`, \
               `0x497cf5`, `0x498209`, `0x49ca0a`, `0x4b05e5`, `0x4b1013`) were listed, not \
               traced to the intro's end",
        affects: "when control returns to the player",
        resolver: "tracing those writers' callers",
    },
    BodyUnknown {
        id: "airframe-mode",
        what: "the `mode` key (`tank`, `ship`, `jet`, `heli`, `wingman`, `plane` -> \
               `type+0xa4` = 2, 3, 0, 1, 4, 5) of the player's airframe record. The loader \
               snaps the node's height to the terrain (`0x47fad0`) only for 2 and 3; an \
               aircraft is not either, but the retail value was not read",
        affects: "whether the node's y differs from the stored 150 before the body copy",
        resolver: "reading the airframe record's `mode` for the M01 player airframe",
    },
    BodyUnknown {
        id: "network-gate-value",
        what: "the value of the config variable `Network` (`0x6237f4`, pointer \
               `[0x64f750]`, read by `0x440ad0`) in a single-player run; callback 10 places \
               the body only when it is zero",
        affects: "whether the placement runs at all in M01",
        resolver: "reading the default `0x440242` passes (`0x5bde90`) or the owner's run",
    },
    BodyUnknown {
        id: "velocity-integration",
        what: "how `veh+0x924` is integrated (per second, per frame) once the flight update \
               runs again",
        affects: "the unit of the -20.0 and the position a moment after control returns",
        resolver: "reading the integrator behind `0x4897c0`",
    },
    BodyUnknown {
        id: "no-run",
        what: "no original run exists on this machine",
        affects: "every claim; nothing is `verified_original`",
        resolver: "the owner's reference capture (REF-OWNER-FIRST-CAPTURE)",
    },
];

/// The body's state after callback 10's placement, as the engine computes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedBody {
    /// `veh+0x204..0x20c`: stored position with `+100.0` added to y.
    pub position: [f32; 3],
    /// `veh+0x1f8..0x200` as `(x, y, z)` angles: `(0, heading, 0)` in radians.
    pub euler: [f32; 3],
    /// `veh+0x124`.
    pub throttle: f32,
    /// `veh+0x924`: the body's `(0,0,1)` axis rotated by the orientation, times -20.0.
    pub velocity: [f32; 3],
}

/// The stored-position height the intro placement adds (`[0x6032e4]`).
pub const INTRO_RAISE: f32 = 100.0;
/// The intro placement's throttle (`0x3ecccccd`).
pub const INTRO_THROTTLE: f32 = 0.4;
/// The factor on the unit axis (`[0x608040]`).
pub const INTRO_SPEED_FACTOR: f32 = -20.0;

/// Replays `0x476250` (the loader's node-to-body copy) followed by the intro
/// branch of `0x47f1f0(0, 0)` for the stored `aiv.zrd` position and heading
/// (degrees), in the engine's order: the heading becomes radians through the
/// double `pi/180` (`0x6040e8`), the Euler triple `(0, heading, 0)` becomes the
/// quaternion of `0x53f610` and `(0,0,1)` is rotated by it with `0x53fb40`.
#[must_use]
pub fn place_player_at_intro_start(position: [f32; 3], heading_degrees: f32) -> PlacedBody {
    let heading = (f64::from(heading_degrees) * (std::f64::consts::PI / 180.0)) as f32;
    // 0x53f610(yaw, pitch, roll): half angles, then the four products.
    let half = |angle: f32| {
        let h = f64::from(angle) * 0.5;
        (h.sin() as f32, h.cos() as f32)
    };
    let (yaw, pitch, roll) = (heading, 0.0_f32, 0.0_f32);
    let ((sa, ca), (sb, cb), (sc, cc)) = (half(yaw), half(pitch), half(roll));
    let q = [
        cc * cb * ca + sc * sb * sa,
        sc * cb * sa + cc * sb * ca,
        cc * cb * sa - sc * sb * ca,
        sc * cb * ca - cc * sb * sa,
    ];
    // 0x53fb40(q, (0,0,1)): (2w^2 - 1) v + 2 (q.v) q + 2 w (q x v).
    let v = [0.0_f32, 0.0, 1.0];
    let (w, qv) = (q[0], [q[1], q[2], q[3]]);
    let dot = qv[0] * v[0] + qv[1] * v[1] + qv[2] * v[2];
    let cross = [
        qv[1] * v[2] - qv[2] * v[1],
        qv[2] * v[0] - qv[0] * v[2],
        qv[0] * v[1] - qv[1] * v[0],
    ];
    let k = 2.0 * w * w - 1.0;
    let axis: [f32; 3] = std::array::from_fn(|i| 2.0 * w * cross[i] + k * v[i] + 2.0 * dot * qv[i]);
    PlacedBody {
        position: [position[0], position[1] + INTRO_RAISE, position[2]],
        euler: [0.0, heading, 0.0],
        throttle: INTRO_THROTTLE,
        velocity: axis.map(|a| a * INTRO_SPEED_FACTOR),
    }
}

/// How often `name` occurs as bytes in a carrier. Used to show that no data
/// names an animation the chain never calls.
#[must_use]
pub fn name_occurrences(bytes: &[u8], name: &str) -> usize {
    let needle = name.as_bytes();
    if needle.is_empty() {
        return 0;
    }
    bytes.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Which part of the record an event sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// The `RESET_SEQUENCE` block (run by the tick `0x4ed2a0`).
    Reset,
    /// An ordinary sequence.
    Sequence,
}

/// What the event does to the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyEventKind {
    /// `CALLBACK` with an id of [`BODY_CALLBACK_IDS`].
    Callback(u32),
    /// `OBJECT_ADD_CHILD` naming the player node as the child.
    AddChild(String),
    /// `OBJECT_DELETE_CHILD` naming the player node as the child.
    DeleteChild(String),
    /// `CALL_ANIMATION`.
    Call(String),
}

/// One event of the launch chain that matters to the player's body.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyEvent {
    /// The record's `anim_name`.
    pub record: String,
    /// Reset block or ordinary sequence.
    pub block: BlockKind,
    /// The sequence's name.
    pub sequence: String,
    /// The tag's second byte (which clock the event waits for).
    pub group: u8,
    /// The event's `START_TIME`, seconds as the timeline finding reads it.
    pub start_time: f32,
    /// What it does.
    pub kind: BodyEventKind,
}

/// Every [`BodyEvent`] of `intro_animation` and the animations it
/// transitively calls that the container holds, in record order (intro
/// first, then first-call order) and stream order within a record.
///
/// # Errors
///
/// [`LaunchEndPoseError`] when the container does not read or a record is not
/// the measured shape.
pub fn measure_intro_body_events(
    path: &RelativePath,
    bytes: &[u8],
    intro_animation: &str,
) -> Result<Vec<BodyEvent>, LaunchEndPoseError> {
    with_records(path, bytes, |records| events_in(records, intro_animation))
}

fn events_in(
    records: &AnimationRecords<'_>,
    intro_animation: &str,
) -> Result<Vec<BodyEvent>, LaunchEndPoseError> {
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut queue = vec![intro_animation.to_owned()];
    while !queue.is_empty() {
        let name = queue.remove(0);
        if seen.contains(&name) || records.by_anim_name(name.as_bytes()).next().is_none() {
            continue;
        }
        seen.push(name.clone());
        let record = record_named(records, &name)?;
        for sequence in record.sequences() {
            let block = if sequence.kind() == AnimationRecordSequenceKind::Reset {
                BlockKind::Reset
            } else {
                BlockKind::Sequence
            };
            let stream = sequence.events();
            let stream_error = |error: crate::animation::events::EventStreamError| {
                LaunchEndPoseError::Chain(format!("event stream: {error}"))
            };
            let raw = walk_event_stream(stream).map_err(stream_error)?;
            let decoded = decode_event_stream(stream).map_err(stream_error)?;
            for (raw, decoded) in raw.iter().zip(&decoded) {
                let body =
                    &stream[raw.offset as usize..(raw.offset + u64::from(raw.length)) as usize];
                let kind = match raw.opcode {
                    OP_CALLBACK if body.len() >= 16 => {
                        let id = word(body, 0xc);
                        BODY_CALLBACK_IDS
                            .contains(&id)
                            .then_some(BodyEventKind::Callback(id))
                    }
                    OP_ADD_CHILD | OP_DELETE_CHILD => {
                        let player = node_name(record, i64::from(half(body, 0xe)))
                            .is_some_and(|child| child == PLAYER_NODE);
                        let parent =
                            node_name(record, i64::from(half(body, 0xc))).unwrap_or_default();
                        player.then_some(if raw.opcode == OP_ADD_CHILD {
                            BodyEventKind::AddChild(parent)
                        } else {
                            BodyEventKind::DeleteChild(parent)
                        })
                    }
                    OP_CALL_ANIMATION => decoded.spelled_name.clone().map(|called| {
                        queue.push(called.clone());
                        BodyEventKind::Call(called)
                    }),
                    _ => None,
                };
                if let Some(kind) = kind {
                    out.push(BodyEvent {
                        record: name.clone(),
                        block,
                        sequence: text(sequence.name()),
                        group: raw.group,
                        start_time: decoded.start_time,
                        kind,
                    });
                }
            }
        }
    }
    Ok(out)
}
