//! #1678 (`PARITY-M01-ATMO-E3`): what places the player's body when the M01
//! launch drop animation stops.
//!
//! The finding is `docs/findings/2026-10-18-m01-launch-release.md`. It
//! continues [`super::launch_end_pose`] (#1660), which measured the pose the
//! drop animation leaves on the `player` *node*. This module answers, from the
//! same retail container and from static analysis of the owner-supplied image,
//! whether that node pose ever reaches the player's physics body, and records
//! every link that stays unknown.
//!
//! What the data states is read, not assumed ([`measure_launch_release`]):
//!
//! * the launch chain: the intro record and every animation it (transitively)
//!   calls that the container holds, each with its flag word, activation byte,
//!   `RESET_TIME` and the `CALLBACK` ids it fires;
//! * the intro's `RESET_SEQUENCE` events in order;
//! * whether the drop animation's `STOP_ANIMATION` finishes it at once
//!   (`RESET_TIME < 0`);
//! * whether any record of the chain carries `AUTO_RESET_NODE_STATES`;
//! * whether the chain fires the one callback that copies a node's world pose
//!   into the player's body ([`RELEASE_CALLBACK_ID`]).
//!
//! What the engine does with those values is [`ENGINE_FACTS`] (addresses in
//! the image, provenance `static`); the acceptance tests re-read the
//! instruction bytes they rest on. Nothing here is `verified_original` and no
//! original run exists: the first control frame is **not** claimed, see
//! [`RELEASE_UNKNOWNS`].

use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    AnimationRecord, AnimationRecordSequenceKind, AnimationRecordTableKind, AnimationRecords,
    HeaderStatus, ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index,
};
use cs_types::install::RelativePath;

use super::launch_end_pose::{
    DROP_SEQUENCE, LaunchEndPoseError, PLAYER_NODE, half, node_name, record_named, text, word,
};
use crate::animation::events::{decode_event_stream, walk_event_stream};

/// The record flag word bit `AUTO_RESET_NODE_STATES ON` sets (parser
/// `0x51e775`). Runtime: instance `+0x9c` is the record's `+156` word.
pub const AUTO_RESET_NODE_STATES: u32 = 0x0004_0000;
/// The callback id whose handler (`0x47e557`) copies the player node's world
/// pose into the player's body state, detaches the node from its parent and
/// derives the velocity.
pub const RELEASE_CALLBACK_ID: u32 = 951;
/// The callback id (fired by the intro's `RESET_SEQUENCE`) whose handler calls
/// the engine's "place the player" routine `0x47f1f0`.
pub const PLACE_PLAYER_CALLBACK_ID: u32 = 10;

/// `OBJECT_ADD_CHILD`.
const OP_ADD_CHILD: u8 = 15;
/// `OBJECT_DELETE_CHILD`.
const OP_DELETE_CHILD: u8 = 16;
/// `CALL_ANIMATION`.
const OP_CALL_ANIMATION: u8 = 24;
/// `CALLBACK`.
const OP_CALLBACK: u8 = 35;

/// A statement about the engine, read from the image and not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineFact {
    /// Stable identifier.
    pub id: &'static str,
    /// Virtual address in the image (`file offset + 0x400000`).
    pub va: u32,
    /// What the code at `va` does, as read.
    pub claim: &'static str,
}

/// The static facts the finding rests on. Provenance of every row is
/// `static` (instruction bytes read from `$CS_ENGINE_IMAGE`); the acceptance
/// test `accept_parity_m01_atmo_e3_engine_facts_are_the_bytes_the_finding_read`
/// re-reads each.
pub const ENGINE_FACTS: &[EngineFact] = &[
    EngineFact {
        id: "record-flags-are-instance-0x9c",
        va: 0x4ed09e,
        claim: "`0x4ed090` tests `[instance+0x9c] & 0x40000` (AUTO_RESET_NODE_STATES); the \
                instance is the record copied verbatim, so `+0x9c` is the record's flag word \
                at `+156`, `+0xa0` its status byte, `+0xa1` its activation byte, `+0xac` its \
                `RESET_TIME`, `+0xd0` its `RESET_SEQUENCE` block and `+0xd4` its \
                `DAMAGE_SEQUENCE` block",
    },
    EngineFact {
        id: "stop-animation-executor",
        va: 0x4ee297,
        claim: "opcode 25 (`STOP_ANIMATION`) is registered to `0x4ebc80`, which finds the \
                called child by name and passes its instance to `0x4ebbb0`",
    },
    EngineFact {
        id: "stop-is-immediate-for-negative-reset-time",
        va: 0x4ebbe1,
        claim: "`0x4ebbb0` compares the instance's `RESET_TIME` (`+0xac`) with 0.0; below it \
                the instance is finished at once through `0x4ed190` (the call at `0x4ebc44`)",
    },
    EngineFact {
        id: "finish-writes-no-node-pose",
        va: 0x4ed190,
        claim: "`0x4ed190` stops the sound/particle/light objects (`0x4eba60`, `0x4ebae0`, \
                `0x4ebb30`), stops the child animations (`0x4ebbb0`), cancels the timer and \
                sets the state byte; it calls neither the position setter `0x4d1d50` nor the \
                rotation setter `0x4d1a30`",
    },
    EngineFact {
        id: "node-restore-is-start-only-and-flag-gated",
        va: 0x4ed424,
        claim: "`0x4ed090` (the only code that writes saved node positions and rotations \
                back: `0x4d1d50`, `0x4d1a30`, `0x4d18d0`) is called only from `0x4ed340` \
                (start), and returns at once unless flag `0x40000` is set",
    },
    EngineFact {
        id: "reset-sequence-runs-at-start",
        va: 0x4ed42c,
        claim: "`0x4ed340` rewinds (`0x4ebfa0`) and steps (`0x4ecbb0`) the block at \
                `[instance+0xd0]` (the record's `RESET_SEQUENCE`), then schedules the \
                per-frame tick `0x4ed2a0`, which keeps stepping it. The ordinary sequences \
                (`+0xcc`) whose initial state is 0 run beside it",
    },
    EngineFact {
        id: "damage-sequence-runs-from-hit-callbacks-only",
        va: 0x4e71e6,
        claim: "`0x4e71e0` rewinds and steps `[instance+0xd4]` (the `DAMAGE_SEQUENCE`); its \
                two callers `0x4e7150` and `0x4e7220` are registered at `0x523287`..`0x5232e3` \
                only for activation bytes 0, 1 and 2 (weapon/collide hits)",
    },
    EngineFact {
        id: "callback-executor",
        va: 0x4ee2f6,
        claim: "opcode 35 (`CALLBACK`) calls the instance callback `[instance+0x74]` \
                (instance, `[+0x78]`, id), installed by `0x4ee160`",
    },
    EngineFact {
        id: "player-callback-handler-table",
        va: 0x47f160,
        claim: "the vehicle callback handler `0x47e080` dispatches ids 0..13 through the \
                dword table `0x47f160` (id 10 -> `0x47e11e`), id 20 -> `0x47e2e1` (remembers \
                the running intro animation in `[0x71c50c]`) and ids from 0x392 through the \
                byte table `0x47f1b8` and dword table `0x47f198` (id 951 -> `0x47e557`)",
    },
    EngineFact {
        id: "place-player-from-callback-10",
        va: 0x47e15f,
        claim: "the id-10 handler calls `0x47f1f0(0, 0)` unless `*[0x64f750]` (read by \
                `0x440ad0`) is non-zero",
    },
    EngineFact {
        id: "place-player-routine",
        va: 0x47f1f0,
        claim: "`0x47f1f0` writes the player body: position `veh+0x204`, euler `veh+0x1f8`, \
                matrices `veh+0x150`/`0x180`, copy `veh+0x1a4`, velocity `veh+0x924`, \
                throttle `veh+0x124`, then the node through `0x4d1d50`/`0x4d1a30` and clears \
                `veh+0x91d/0x91e/0x91f`. With `veh+0x91d` set it adds `[0x6032e4]` (100.0) to \
                `veh+0x208`, sets the throttle to 0.4 and the velocity to the body's \
                `(0,0,1)` axis times `[0x608040]` (-20.0); with it clear, throttle 0 and the \
                velocity of the zero vector `0x75d1b8`",
    },
    EngineFact {
        id: "release-callback-handler",
        va: 0x47e557,
        claim: "the id-951 handler reads the player node's world position (`0x4cf200`) and \
                rotation (`0x4cf380`), stores them in the body (`veh+0x204`, `+0x150`, \
                `+0x1a4`), detaches the node from a parent (`0x4cd6d0`), writes it back \
                (`0x4d1d50`, `0x4d1a30`) and sets `veh+0x924` from `veh+0x198..0x1a0` times \
                `[0x60803c]`. It is the only code read that copies a node pose into the body",
    },
    EngineFact {
        id: "flight-update-skipped-during-intro",
        va: 0x4897dd,
        claim: "the player's per-frame update `0x4897c0` reads `[0x71c50c]` (set by callback \
                20, cleared by callback 0 when the same animation finishes) and branches to \
                `0x489e30` (shadow/level-of-detail work only) while it is non-zero: the \
                flight integration does not run during the intro",
    },
];

/// What a callback id does in the player handler `0x47e080` (static).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackMeaning {
    /// The id as the `CALLBACK` event states it.
    pub id: u32,
    /// The handler's address.
    pub handler_va: u32,
    /// What the handler does, as read.
    pub meaning: &'static str,
}

/// The ids the launch chain fires, plus [`RELEASE_CALLBACK_ID`].
pub const CALLBACK_MEANINGS: &[CallbackMeaning] = &[
    CallbackMeaning {
        id: 0,
        handler_va: 0x47e0ab,
        meaning: "the instance finished: clears `[0x71c50c]` if it is this animation",
    },
    CallbackMeaning {
        id: 1,
        handler_va: 0x47e0cb,
        meaning: "flags a pending refresh (`[0x71d290] = 1` when the animation is the \
                  remembered `[0x71bb6c]`) and calls `0x42e5c0`",
    },
    CallbackMeaning {
        id: 2,
        handler_va: 0x47e8fb,
        meaning: "calls `0x42e5a0`; not read further",
    },
    CallbackMeaning {
        id: 10,
        handler_va: 0x47e11e,
        meaning: "shows the HUD pieces and, outside the multiplayer case, places the \
                  player body: `0x47f1f0(0, 0)`",
    },
    CallbackMeaning {
        id: 11,
        handler_va: 0x47e19e,
        meaning: "puts the player in the cinematic state: `veh+0x91d = 1`, `veh+0x91e = 1`",
    },
    CallbackMeaning {
        id: 20,
        handler_va: 0x47e2e1,
        meaning: "remembers the running animation in `[0x71c50c]`; the player's flight \
                  update does not integrate while it is set",
    },
    CallbackMeaning {
        id: 0x29b,
        handler_va: 0x47e342,
        meaning: "`[0x621378] = 1`",
    },
    CallbackMeaning {
        id: 0x391,
        handler_va: 0x47e4b8,
        meaning: "HUD on (`0x41f250`, `0x4a95f0`, `0x4516e0`, `0x453660`)",
    },
    CallbackMeaning {
        id: 0x392,
        handler_va: 0x47e503,
        meaning: "HUD off (`0x41f2e0`, `0x4a9610`, `0x4516e0`, `0x453660`)",
    },
    CallbackMeaning {
        id: RELEASE_CALLBACK_ID,
        handler_va: 0x47e557,
        meaning: "release: copy the player node's world pose into the body, detach the \
                  node, derive the velocity",
    },
];

/// A link of the chain that static analysis does not settle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseUnknown {
    /// Stable identifier.
    pub id: &'static str,
    /// What is not known.
    pub what: &'static str,
    /// The content that depends on it.
    pub affects: &'static str,
    /// What would resolve it.
    pub resolver: &'static str,
}

/// Every link [`measure_launch_release`] does not claim.
pub const RELEASE_UNKNOWNS: &[ReleaseUnknown] = &[
    ReleaseUnknown {
        id: "body-state-at-intro-start",
        what: "what the player body's position (`veh+0x204`) and euler angles (`veh+0x1f8`) \
               hold when callback 10 runs `0x47f1f0` at the start of the intro. The \
               spawn loader `0x47c210` writes the stored aiv.zrd position and heading to \
               the *node* only; the constructor `0x4aff80` and the player subclass were not \
               read, so the stored record reaching the body is a candidate, not a finding",
        affects: "the first control frame's position and heading",
        resolver: "a follow-up reading `0x4aff80` and the player vehicle's initialiser",
    },
    ReleaseUnknown {
        id: "node-detach-after-intro",
        what: "what removes the player node from the Pandora (`OBJECT_ADD_CHILD` in \
               `start_script`) after the intro ends. The RESET_SEQUENCE that re-adds it to \
               world1 runs at animation start; nothing read re-runs it at the natural end \
               (only the skip key path `0x4a02ac` restarts the intro through `0x4ed480`), \
               and the release handler (callback 951) is not fired by the chain",
        affects: "whether the first control frame's node pose is in the world frame, and \
                  the first frames' visible position",
        resolver: "a follow-up reading what runs when `[0x71c50c]` clears (`0x47e0ab`) and \
                  the vehicle's node write-back",
    },
    ReleaseUnknown {
        id: "cinematic-flag-clear",
        what: "which code clears `veh+0x91d/0x91e` after the intro. `0x47f1f0` clears them \
               at its tail, but the intro's callback 11 sets them again after the \
               RESET_SEQUENCE's callback 10 has run; the writers that remain \
               (`0x4753a1`, `0x4763df`, `0x47fecd`, `0x48049e`, `0x4805ad`, ...) were not \
               traced to the intro's end",
        affects: "when control returns, and which `0x47f1f0` branch a later call takes",
        resolver: "a follow-up tracing those writers",
    },
    ReleaseUnknown {
        id: "multiplayer-gate",
        what: "whether `*[0x64f750]` (read by `0x440ad0`, the gate of the id-10 placement) \
               is zero in single-player",
        affects: "whether the placement at the start of the intro runs at all in M01",
        resolver: "a follow-up reading the writers of `[0x64f750]`",
    },
    ReleaseUnknown {
        id: "velocity-direction",
        what: "the sign and axis convention of `0x53fb40(matrix, (0,0,1))` times -20.0, and \
               the units of the velocity vector at `veh+0x924`",
        affects: "the first control frame's velocity vector (magnitude 20.0 engine units is \
                  read; direction and unit are not)",
        resolver: "#1673 (rotation helpers)",
    },
    ReleaseUnknown {
        id: "unreached-start-state",
        what: "`0x71bb44..0x71bb60`, the start pose and speed the `arg != 0` branch of \
               `0x47f1f0` reads, are not written by any direct store in the image; their \
               source (a loaded record, or zero) is unread",
        affects: "the branch taken for `arg != 0` (callers `0x4440bf`) only; not the intro's \
                  `arg == 0` call",
        resolver: "a follow-up",
    },
    ReleaseUnknown {
        id: "no-run",
        what: "no original executable run exists on this machine and no `original-launch/` \
               frame: every statement here is static",
        affects: "every claim; nothing is `verified_original`",
        resolver: "the owner's reference capture (REF-OWNER-FIRST-CAPTURE)",
    },
];

/// One animation of the launch chain.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainAnimation {
    /// The record's `anim_name`.
    pub name: String,
    /// The record's flag word (`+156`).
    pub flags: u32,
    /// The activation byte (`+161`): 0 weapon hit, 1 collide hit, 2 either,
    /// 3 `ON_CALL`, 4 `ON_STARTUP` (parser `0x51e2d0`..`0x51e390`).
    pub activation: u8,
    /// `RESET_TIME` (`+172`), seconds; negative means "no reset".
    pub reset_time_s: f32,
    /// `flags & AUTO_RESET_NODE_STATES`.
    pub auto_reset_node_states: bool,
    /// The names in the record's `Objects` table (count byte `+217`): the
    /// entries `0x4ed090` saves and restores (92-byte stride `0x5c`).
    pub objects: Vec<String>,
    /// The `CALLBACK` ids its sequences fire, in stream order.
    pub callback_ids: Vec<u32>,
    /// The `CALL_ANIMATION` targets its sequences name, in stream order.
    pub calls: Vec<String>,
}

/// One step of the intro's `RESET_SEQUENCE`, in stream order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResetStep {
    /// `OBJECT_DELETE_CHILD`.
    DeleteChild {
        /// Parent node name.
        parent: String,
        /// Child node name.
        child: String,
    },
    /// `OBJECT_ADD_CHILD`.
    AddChild {
        /// Parent node name.
        parent: String,
        /// Child node name.
        child: String,
    },
    /// `CALLBACK`.
    Callback {
        /// The id.
        id: u32,
    },
    /// `CALL_ANIMATION`.
    CallAnimation {
        /// The animation named.
        name: String,
    },
    /// Any other opcode (visibility, rotation state, ...).
    Other {
        /// The opcode.
        opcode: u8,
    },
}

/// Where a `CALLBACK` event sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackSite {
    /// The record's `anim_name`.
    pub record: String,
    /// The sequence's name (empty for an unnamed one).
    pub sequence: String,
    /// The event's byte offset in the sequence's stream.
    pub offset: u64,
    /// The record's activation byte.
    pub activation: u8,
}

/// What the retail data states about the launch chain's release.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchRelease {
    /// The intro record.
    pub intro: String,
    /// The intro's `RESET_SEQUENCE`, in stream order.
    pub reset_sequence: Vec<ResetStep>,
    /// The intro, then every animation it transitively calls that the
    /// container holds, in first-call order.
    pub chain: Vec<ChainAnimation>,
    /// `CALL_ANIMATION` targets of the chain that the container does not hold.
    pub external_calls: Vec<String>,
    /// The animation `drop_planes` calls.
    pub drop_animation: String,
    /// `true` when the drop animation's `RESET_TIME` is negative, i.e. its
    /// `STOP_ANIMATION` finishes it at once (`0x4ebbb0` -> `0x4ed190`).
    pub drop_stop_is_immediate: bool,
    /// `true` when any record of the chain carries `AUTO_RESET_NODE_STATES`:
    /// the only way the engine writes saved poses back. The propeller, light
    /// and screen-effect records of the chain do.
    pub any_auto_reset_node_states: bool,
    /// `true` when such a record lists the `player` node in its `Objects`
    /// table, i.e. when a start of it could write the player node's saved
    /// pose back. Measured `false` for M01's launch chain.
    pub auto_reset_restores_player: bool,
    /// Every callback id the chain fires, sorted, without repeats.
    pub callback_ids: Vec<u32>,
    /// `true` when the chain fires [`RELEASE_CALLBACK_ID`].
    pub release_callback_in_chain: bool,
    /// `true` when the intro's `RESET_SEQUENCE` fires
    /// [`PLACE_PLAYER_CALLBACK_ID`] (the placement of the body at the start).
    pub reset_places_player: bool,
}

/// Opens the container and runs `read` on its records (which borrow the
/// parse context, so they cannot be returned).
pub(super) fn with_records<T>(
    path: &RelativePath,
    bytes: &[u8],
    read: impl FnOnce(&AnimationRecords<'_>) -> Result<T, LaunchEndPoseError>,
) -> Result<T, LaunchEndPoseError> {
    let carrier = LaunchEndPoseError::Carrier;
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
    read(&records)
}

/// Every `CALLBACK` event of `bytes` whose id is `id`, in record order.
///
/// # Errors
///
/// [`LaunchEndPoseError::Carrier`] when the container does not read.
pub fn callback_sites(
    path: &RelativePath,
    bytes: &[u8],
    id: u32,
) -> Result<Vec<CallbackSite>, LaunchEndPoseError> {
    with_records(path, bytes, |records| {
        let mut sites = Vec::new();
        for record in records.iter() {
            for sequence in record.sequences() {
                let events = sequence.events();
                let raw = walk_event_stream(events)
                    .map_err(|error| LaunchEndPoseError::Chain(format!("event stream: {error}")))?;
                for event in raw {
                    if event.opcode != OP_CALLBACK {
                        continue;
                    }
                    let body = &events
                        [event.offset as usize..(event.offset + u64::from(event.length)) as usize];
                    if body.len() >= 16 && word(body, 0xc) == id {
                        sites.push(CallbackSite {
                            record: text(record.anim_name()),
                            sequence: text(sequence.name()),
                            offset: event.offset,
                            activation: record.activation(),
                        });
                    }
                }
            }
        }
        Ok(sites)
    })
}

/// One event of a record's sequences, reset block included.
type KindedEvent<'a> = (AnimationRecordSequenceKind, &'a [u8], u8);

/// Every event of every sequence of `record`, as
/// `(sequence kind, event bytes, opcode)`.
fn all_events<'a>(
    record: &AnimationRecord<'a>,
) -> Result<Vec<KindedEvent<'a>>, LaunchEndPoseError> {
    let mut out = Vec::new();
    for sequence in record.sequences() {
        let events = sequence.events();
        let raw = walk_event_stream(events)
            .map_err(|error| LaunchEndPoseError::Chain(format!("event stream: {error}")))?;
        for event in raw {
            out.push((
                sequence.kind(),
                &events[event.offset as usize..(event.offset + u64::from(event.length)) as usize],
                event.opcode,
            ));
        }
    }
    Ok(out)
}

fn object_names(record: &AnimationRecord<'_>) -> Vec<String> {
    record
        .table(AnimationRecordTableKind::Objects)
        .map(|table| {
            (0..table.count())
                .filter_map(|i| table.name(i))
                .map(text)
                .collect()
        })
        .unwrap_or_default()
}

fn chain_animation(record: &AnimationRecord<'_>) -> Result<ChainAnimation, LaunchEndPoseError> {
    let mut callback_ids = Vec::new();
    let mut calls = Vec::new();
    for sequence in record.sequences() {
        let decoded = decode_event_stream(sequence.events())
            .map_err(|error| LaunchEndPoseError::Chain(format!("event decode: {error}")))?;
        let raw = walk_event_stream(sequence.events())
            .map_err(|error| LaunchEndPoseError::Chain(format!("event stream: {error}")))?;
        for (event, raw) in decoded.iter().zip(raw.iter()) {
            match event.opcode {
                OP_CALL_ANIMATION => {
                    if let Some(name) = &event.spelled_name {
                        calls.push(name.clone());
                    }
                }
                OP_CALLBACK => {
                    let body = &sequence.events()
                        [raw.offset as usize..(raw.offset + u64::from(raw.length)) as usize];
                    if body.len() >= 16 {
                        callback_ids.push(word(body, 0xc));
                    }
                }
                _ => {}
            }
        }
    }
    let flags = record.flags();
    Ok(ChainAnimation {
        name: text(record.anim_name()),
        flags,
        activation: record.activation(),
        reset_time_s: record.reset_time(),
        auto_reset_node_states: flags & AUTO_RESET_NODE_STATES != 0,
        objects: object_names(record),
        callback_ids,
        calls,
    })
}

/// Reads the launch chain of `intro_animation` in the camera carrier `bytes`
/// and states what it does and does not do to the player's body.
///
/// # Errors
///
/// [`LaunchEndPoseError`] naming the first link that is absent or not the
/// measured shape.
pub fn measure_launch_release(
    path: &RelativePath,
    bytes: &[u8],
    intro_animation: &str,
) -> Result<LaunchRelease, LaunchEndPoseError> {
    with_records(path, bytes, |records| measure_in(records, intro_animation))
}

fn measure_in(
    records: &AnimationRecords<'_>,
    intro_animation: &str,
) -> Result<LaunchRelease, LaunchEndPoseError> {
    let intro = record_named(records, intro_animation)?;

    // The intro's RESET_SEQUENCE, in order.
    let mut reset_sequence = Vec::new();
    for (kind, event, opcode) in all_events(intro)? {
        if kind != AnimationRecordSequenceKind::Reset {
            continue;
        }
        let nodes = |event: &[u8]| {
            let parent = node_name(intro, i64::from(half(event, 0xc)));
            let child = node_name(intro, i64::from(half(event, 0xe)));
            parent.zip(child)
        };
        reset_sequence.push(match opcode {
            OP_ADD_CHILD | OP_DELETE_CHILD => {
                let (parent, child) = nodes(event).ok_or_else(|| {
                    LaunchEndPoseError::Chain(
                        "a reset-sequence child event names a node outside the table".to_owned(),
                    )
                })?;
                if opcode == OP_ADD_CHILD {
                    ResetStep::AddChild { parent, child }
                } else {
                    ResetStep::DeleteChild { parent, child }
                }
            }
            OP_CALLBACK if event.len() >= 16 => ResetStep::Callback {
                id: word(event, 0xc),
            },
            OP_CALL_ANIMATION => {
                let name = text(&event[12..]);
                ResetStep::CallAnimation {
                    name: name.split('\0').next().unwrap_or_default().to_owned(),
                }
            }
            other => ResetStep::Other { opcode: other },
        });
    }

    // The drop animation: the one `drop_planes` calls.
    let mut drops = Vec::new();
    for sequence in intro.sequences() {
        if sequence.kind() == AnimationRecordSequenceKind::Reset
            || text(sequence.name()) != DROP_SEQUENCE
        {
            continue;
        }
        let decoded = decode_event_stream(sequence.events())
            .map_err(|error| LaunchEndPoseError::Chain(format!("{DROP_SEQUENCE}: {error}")))?;
        for event in decoded {
            if let (OP_CALL_ANIMATION, Some(name)) = (event.opcode, event.spelled_name) {
                drops.push(name);
            }
        }
    }
    let [drop_animation] = drops.as_slice() else {
        return Err(LaunchEndPoseError::Chain(format!(
            "`{DROP_SEQUENCE}` must call exactly one animation; it calls {}",
            drops.len()
        )));
    };

    // The chain: breadth-first over CALL_ANIMATION targets the container holds.
    let mut chain: Vec<ChainAnimation> = Vec::new();
    let mut external_calls: Vec<String> = Vec::new();
    let mut queue = vec![intro_animation.to_owned()];
    while !queue.is_empty() {
        let name = queue.remove(0);
        if chain.iter().any(|seen| seen.name == name) {
            continue;
        }
        if records.by_anim_name(name.as_bytes()).next().is_none() {
            if !external_calls.contains(&name) {
                external_calls.push(name);
            }
            continue;
        }
        let record = record_named(records, &name)?;
        let animation = chain_animation(record)?;
        queue.extend(animation.calls.iter().cloned());
        chain.push(animation);
    }

    let drop_record = chain
        .iter()
        .find(|animation| &animation.name == drop_animation)
        .ok_or_else(|| {
            LaunchEndPoseError::Chain(format!("`{drop_animation}` is not in the container"))
        })?;
    let mut callback_ids: Vec<u32> = chain
        .iter()
        .flat_map(|animation| animation.callback_ids.iter().copied())
        .collect();
    callback_ids.sort_unstable();
    callback_ids.dedup();

    Ok(LaunchRelease {
        intro: intro_animation.to_owned(),
        reset_places_player: reset_sequence.contains(&ResetStep::Callback {
            id: PLACE_PLAYER_CALLBACK_ID,
        }),
        reset_sequence,
        drop_animation: drop_animation.clone(),
        drop_stop_is_immediate: drop_record.reset_time_s < 0.0,
        any_auto_reset_node_states: chain
            .iter()
            .any(|animation| animation.auto_reset_node_states),
        auto_reset_restores_player: chain.iter().any(|animation| {
            animation.auto_reset_node_states
                && animation.objects.iter().any(|name| name == PLAYER_NODE)
        }),
        release_callback_in_chain: callback_ids.contains(&RELEASE_CALLBACK_ID),
        callback_ids,
        chain,
        external_calls,
    })
}
