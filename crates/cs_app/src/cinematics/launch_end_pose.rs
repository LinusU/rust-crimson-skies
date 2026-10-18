//! #1660 (`PARITY-M01-ATMO-E2`): the player's pose at the end of the launch
//! cinematic, measured from `cam_anim.zbd`.
//!
//! The finding is `docs/findings/2026-10-18-m01-launch-end-pose.md`. This
//! module is the production reader behind it. It follows the chain the data
//! states and refuses where the data does not state one:
//!
//! 1. `generic_intro` (the `NEW_GAME_START` row of `startanims.zrd`) adds the
//!    `player` node as a child of a parent node (`OBJECT_ADD_CHILD`, opcode
//!    15). The parent is decoded from the event, not named here.
//! 2. `generic_intro`'s `drop_planes` sequence calls the animation that drops
//!    the player (`CALL_ANIMATION`) and stops it at a stated time
//!    (`STOP_ANIMATION`). Both are decoded.
//! 3. The dropping animation's `OBJECT_MOTION_SI_SCRIPT` event (opcode 12) for
//!    the `player` node names a track through the record's `IndexWords` table.
//!    The track is evaluated at the stop time with
//!    [`cs_formats::zbd::anim_tracks`].
//! 4. The track's numbers are the `player` node's own position in its
//!    **parent's** frame (the event's flag bit 0 is clear: absolute, not a base
//!    plus an offset). The parent's measured world pose
//!    ([`ParentPose`]) turns that into a world position.
//!
//! What is not measured is returned as [`LAUNCH_END_POSE_UNKNOWNS`], never
//! filled in: the rotation convention of the track's quaternion, the parent
//! node's own parent, the clock that starts the drop, what the engine does with
//! the player once the animation stops, and the velocity it leaves behind.
//! Nothing here is `verified_original`.

use std::fmt;

use cs_formats::io::ParseContext;
use cs_formats::zbd::anim_tracks::{AnimationTracks, TrackBlock, read_animation_tracks};
use cs_formats::zbd::{
    AnimationRecord, AnimationRecordSequenceKind, AnimationRecordTableKind, HeaderStatus,
    ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index,
};
use cs_types::install::RelativePath;

use crate::animation::events::{decode_event_stream, walk_event_stream};
use crate::mission_start::stored_heading_radians;
use crate::mission_world_actors::{SpawnedZeppelinActor, object3d_orientation};

/// The animation `startanims.zrd`'s `NEW_GAME_START` row starts the launch
/// with. M01's row names it (`ZBD/C3/M01/zrdr.zbd`, `startanims.zrd`); the
/// reader takes it as an argument so a mission that names another still goes
/// through the same decode.
pub const LAUNCH_INTRO_ANIMATION: &str = "generic_intro";
/// The node the cinematic moves and the player drives.
pub const PLAYER_NODE: &str = "player";
/// The sequence of the intro that calls and stops the player's drop.
pub const DROP_SEQUENCE: &str = "drop_planes";

/// `OBJECT_MOTION_SI_SCRIPT` (`0x4ea7d0`).
const OP_MOTION_SI_SCRIPT: u8 = 12;
/// `OBJECT_ADD_CHILD` (`0x4eab20`).
const OP_ADD_CHILD: u8 = 15;
/// `CALL_ANIMATION`.
const OP_CALL_ANIMATION: u8 = 24;
/// `STOP_ANIMATION`.
const OP_STOP_ANIMATION: u8 = 25;

/// A link of the chain that no measurement settles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchPoseUnknown {
    /// Stable identifier.
    pub id: &'static str,
    /// What is not known.
    pub what: &'static str,
    /// The content that depends on it.
    pub affects: &'static str,
    /// Who resolves it, or what would.
    pub resolver: &'static str,
}

/// Every link [`measure_launch_end_pose`] does not claim.
pub const LAUNCH_END_POSE_UNKNOWNS: &[LaunchPoseUnknown] = &[
    LaunchPoseUnknown {
        id: "rotation-convention",
        what: "how the engine turns the track's quaternion (and its rotation \
               polynomial) into the node's rotation triple, and so the sign of \
               the pitch",
        affects: "the player's attitude at the end of the drop (magnitude is \
                  measured, sign is not)",
        resolver: "#1673 (helpers 0x53fbf0, 0x53f920, 0x53fa40, 0x53df30)",
    },
    LaunchPoseUnknown {
        id: "parent-of-parent",
        what: "the parent node's own parent in the scene graph; its stored \
               position is applied through the Object3d position setter, so it \
               is in that parent's frame",
        affects: "whether the composed position is a world1 position",
        resolver: "#1679 (the parent's place in the world container's node tree)",
    },
    LaunchPoseUnknown {
        id: "parent-motion",
        what: "whether the parent node moves between mission start and the \
               stop time (zeppelin engines, `max_speed`)",
        affects: "the composed world position and any carried velocity",
        resolver: "#1679",
    },
    LaunchPoseUnknown {
        id: "intro-clock",
        what: "when, on the intro's clock, `drop_planes` starts (every call \
               states START_TIME 0), and whether the stop time is relative to \
               the drop animation's own clock",
        affects: "the cinematic's total length; not the pose (the stop time and \
                  the track share one clock by structure)",
        resolver: "#1674",
    },
    LaunchPoseUnknown {
        id: "branch-choice",
        what: "which airframe takes which branch of `check_balmoral` / \
               `check_warhawk` (condition word 0x2000, operands 5 and 6); both \
               branches end in the same drop",
        affects: "whether a first-person shot precedes the drop",
        resolver: "#1674",
    },
    LaunchPoseUnknown {
        id: "release-rule",
        what: "what the engine does with the player when the drop animation \
               stops: the intro's RESET_SEQUENCE puts the player under the \
               world root, and neither the executors of OBJECT_ADD_CHILD / \
               OBJECT_DELETE_CHILD (0x4cd610, 0x4cd850) nor anything read here \
               rewrites the child's local numbers, so a re-parent alone \
               would keep them; when the reset runs and what places the \
               physics body is not measured",
        affects: "whether the first control frame is this pose or the stored \
                  aiv.zrd player record",
        resolver: "#1678",
    },
    LaunchPoseUnknown {
        id: "release-velocity",
        what: "the velocity the physics body starts with: the track's last \
               segment is a spline that eases to rest, so its derivative at the \
               stop time and its mean rate disagree, and no field states a \
               hand-off velocity",
        affects: "the first control frame's speed and direction",
        resolver: "#1678",
    },
    LaunchPoseUnknown {
        id: "field-of-view-and-camera",
        what: "the camera of the first control frame is the player cockpit \
               camera, not this cinematic's `camera1`",
        affects: "nothing in this pose",
        resolver: "#1665 / #1652",
    },
];

/// Why the chain could not be followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchEndPoseError {
    /// The container did not dispatch, index or walk.
    Carrier(String),
    /// A record, table entry or event the chain needs is missing, or is not
    /// the measured shape.
    Chain(String),
    /// The chain is measured but the track cannot answer at the stop time.
    Track(String),
    /// The parent's world pose was not supplied.
    NoParentPose(String),
}

impl fmt::Display for LaunchEndPoseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Carrier(reason) => write!(f, "the camera carrier cannot be read: {reason}"),
            Self::Chain(reason) => {
                write!(f, "the launch chain is not the measured shape: {reason}")
            }
            Self::Track(reason) => write!(f, "the motion track cannot answer: {reason}"),
            Self::NoParentPose(node) => {
                write!(
                    f,
                    "no measured world pose was supplied for the parent node `{node}`"
                )
            }
        }
    }
}

impl std::error::Error for LaunchEndPoseError {}

/// The measured world pose of the node the player is parented to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentPose<'a> {
    /// The node's name.
    pub node: &'a str,
    /// Its position as the bind leaves it, metres.
    pub position_m: [f32; 3],
    /// Its orientation `[x, y, z, w]`, composed by [`object3d_orientation`].
    pub orientation: [f64; 4],
}

impl<'a> ParentPose<'a> {
    /// The pose of a bound `zeppelins.zrd` record, from the sources
    /// `bind_mission_world_actors` measured (#792, #814). `None` when either
    /// source stayed open.
    #[must_use]
    pub fn from_zeppelin(actor: &'a SpawnedZeppelinActor) -> Option<Self> {
        use crate::mission_world_actors::PositionSource;
        let position_m = match actor.position_source()? {
            PositionSource::StartupPlacement { position_m, .. }
            | PositionSource::CarrierSpawn { position_m } => *position_m,
        };
        Some(Self {
            node: &actor.node,
            position_m,
            orientation: actor.attitude_source()?.orientation(),
        })
    }

    /// A pose from a stored position and the record's yaw and pitch degrees,
    /// composed the way the spawn composes them (pitch about X, yaw about Y).
    #[must_use]
    pub fn from_stored(
        node: &'a str,
        position_m: [f32; 3],
        yaw_degrees: f32,
        pitch_degrees: f32,
    ) -> Self {
        let pitch = f64::from(stored_heading_radians(pitch_degrees));
        let yaw = f64::from(stored_heading_radians(yaw_degrees));
        Self {
            node,
            position_m,
            orientation: object3d_orientation(pitch, yaw, 0.0),
        }
    }
}

/// Which event or track a value was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSite {
    /// The record's `anim_name`.
    pub record: String,
    /// The sequence's name (empty for an unnamed sequence).
    pub sequence: String,
    /// The event's byte offset in the sequence's stream.
    pub offset: u64,
}

/// The `player` node's own pose in its parent's frame at the stop time.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalPose {
    /// The track's source path as the member table spells it.
    pub track_path: String,
    /// Index of the track block in the container's tail.
    pub block_index: usize,
    /// Index of the segment the stop time falls in, and its bounds.
    pub segment_index: usize,
    /// Segment start, seconds.
    pub segment_start_s: f32,
    /// Segment end, seconds.
    pub segment_end_s: f32,
    /// The position the cubics give at the stop time, metres.
    pub position_m: [f32; 3],
    /// The cubics' derivative at the stop time, metres per second.
    pub cubic_velocity_m_s: [f32; 3],
    /// Metres per second over the preceding 0.1 s of the same track: the rate
    /// the cubic reaches before it eases to rest.
    pub preceding_rate_m_s: [f32; 3],
    /// The rotation part's first four words, `(w, x, y, z)`.
    pub segment_start_quaternion_wxyz: [f32; 4],
    /// The rotation polynomial's three channels at the stop time.
    pub rotation_vector: [f32; 3],
    /// `2·atan2(|xyz|, w)` of the segment's start quaternion, degrees: the
    /// size of the rotation. Its axis is [`Self::segment_start_quaternion_wxyz`]'s
    /// vector part; the sign convention is [`LAUNCH_END_POSE_UNKNOWNS`]'s
    /// `rotation-convention`.
    pub rotation_angle_degrees: f32,
}

/// The measured end of the launch cinematic.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchEndPose {
    /// The node the player is a child of while the cinematic runs.
    pub parent_node: String,
    /// The `OBJECT_ADD_CHILD` event that says so.
    pub parent_event: EventSite,
    /// The animation that drops the player.
    pub drop_animation: String,
    /// The time its `STOP_ANIMATION` states, seconds.
    pub stop_time_s: f32,
    /// The `OBJECT_MOTION_SI_SCRIPT` event that names the track.
    pub motion_event: EventSite,
    /// The player's pose in the parent's frame.
    pub local: LocalPose,
    /// `local.position_m` carried through the parent's pose: the player's
    /// position in the parent's own parent's frame, metres. A world position
    /// only if the `parent-of-parent` unknown resolves to the world root.
    pub composed_position_m: [f64; 3],
    /// The same for the direction of `local.cubic_velocity_m_s` (the parent's
    /// own motion is not added: `parent-motion`).
    pub composed_cubic_velocity_m_s: [f64; 3],
    /// The track's yaw and roll in the parent's frame are the parent's:
    /// the segment's quaternion has no Y or Z component when this is `true`.
    pub rotation_is_pure_pitch: bool,
}

fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [qx, qy, qz, qw] = q;
    // v' = v + 2w (u × v) + 2 u × (u × v), u = (qx, qy, qz).
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = [qx, qy, qz];
    let t = cross(u, v).map(|c| 2.0 * c);
    let ut = cross(u, t);
    [
        v[0] + qw * t[0] + ut[0],
        v[1] + qw * t[1] + ut[1],
        v[2] + qw * t[2] + ut[2],
    ]
}

pub(super) fn word(event: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(event[offset..offset + 4].try_into().expect("four bytes"))
}

pub(super) fn half(event: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(event[offset..offset + 2].try_into().expect("two bytes"))
}

pub(super) fn text(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

pub(super) fn node_name(record: &AnimationRecord<'_>, index: i64) -> Option<String> {
    let table = record.table(AnimationRecordTableKind::Nodes)?;
    let index = usize::try_from(index).ok()?;
    table.name(index).map(text)
}

pub(super) fn record_named<'r, 'a>(
    records: &'r cs_formats::zbd::AnimationRecords<'a>,
    name: &'r str,
) -> Result<&'r AnimationRecord<'a>, LaunchEndPoseError> {
    let mut found = records.by_anim_name(name.as_bytes());
    let first = found
        .next()
        .ok_or_else(|| LaunchEndPoseError::Chain(format!("no animation record `{name}`")))?;
    if found.next().is_some() {
        return Err(LaunchEndPoseError::Chain(format!(
            "`{name}` names more than one record"
        )));
    }
    Ok(first)
}

/// One event of an ordinary sequence: its bytes, header included.
pub(super) struct SequenceEvent<'a> {
    pub(super) sequence: String,
    pub(super) offset: u64,
    pub(super) opcode: u8,
    pub(super) bytes: &'a [u8],
}

/// Every event of a record's non-reset sequences.
pub(super) fn ordinary_events<'a>(
    record: &AnimationRecord<'a>,
) -> Result<Vec<SequenceEvent<'a>>, LaunchEndPoseError> {
    let mut out = Vec::new();
    for sequence in record.sequences() {
        if sequence.kind() == AnimationRecordSequenceKind::Reset {
            continue;
        }
        let events = sequence.events();
        let raw = walk_event_stream(events)
            .map_err(|error| LaunchEndPoseError::Chain(format!("event stream: {error}")))?;
        for event in raw {
            out.push(SequenceEvent {
                sequence: text(sequence.name()),
                offset: event.offset,
                opcode: event.opcode,
                bytes: &events
                    [event.offset as usize..(event.offset + u64::from(event.length)) as usize],
            });
        }
    }
    Ok(out)
}

fn locate_block<'t, 'a>(
    tracks: &'t AnimationTracks<'a>,
    index: usize,
) -> Result<&'t TrackBlock<'a>, LaunchEndPoseError> {
    tracks
        .block(index)
        .ok_or_else(|| LaunchEndPoseError::Chain(format!("no track block {index}")))
}

/// Follows the chain in the camera carrier `bytes` and measures the player's
/// pose at the end of the drop.
///
/// `parent` supplies the measured world pose of the node the chain parents the
/// player to; the reader checks its name against the decoded parent.
///
/// # Errors
///
/// [`LaunchEndPoseError`] naming the first link that is absent or not the
/// measured shape.
pub fn measure_launch_end_pose(
    path: &RelativePath,
    bytes: &[u8],
    intro_animation: &str,
    parent: &[ParentPose<'_>],
) -> Result<LaunchEndPose, LaunchEndPoseError> {
    let carrier = |what: String| LaunchEndPoseError::Carrier(what);
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
    let zan_rows: Vec<String> = index
        .members()
        .iter()
        .filter(|row| row.path().to_ascii_lowercase().ends_with(b".zan"))
        .map(|row| text(row.path()))
        .collect();
    let tracks = read_animation_tracks(records.trailing(), zan_rows.len())
        .map_err(|error| carrier(format!("{key}: motion tracks ({})", error.code())))?;

    // 1. The parent of the player while the intro runs.
    let intro = record_named(&records, intro_animation)?;
    let mut parents = Vec::new();
    for found in ordinary_events(intro)? {
        if found.opcode != OP_ADD_CHILD {
            continue;
        }
        let event = found.bytes;
        let (Some(parent_name), Some(child_name)) = (
            node_name(intro, i64::from(half(event, 0xc))),
            node_name(intro, i64::from(half(event, 0xe))),
        ) else {
            return Err(LaunchEndPoseError::Chain(
                "an OBJECT_ADD_CHILD names a node outside the record's table".to_owned(),
            ));
        };
        if child_name == PLAYER_NODE {
            parents.push((parent_name, found.sequence, found.offset));
        }
    }
    let [(parent_node, parent_sequence, parent_offset)] = parents.as_slice() else {
        return Err(LaunchEndPoseError::Chain(format!(
            "`{intro_animation}` must add `{PLAYER_NODE}` as a child exactly once; it does so {} times",
            parents.len()
        )));
    };

    // 2. The drop animation and its stop time.
    let mut calls = Vec::new();
    let mut stops = Vec::new();
    for sequence in intro.sequences() {
        if sequence.kind() == AnimationRecordSequenceKind::Reset
            || text(sequence.name()) != DROP_SEQUENCE
        {
            continue;
        }
        let decoded = decode_event_stream(sequence.events())
            .map_err(|error| LaunchEndPoseError::Chain(format!("{DROP_SEQUENCE}: {error}")))?;
        for event in decoded {
            match (event.opcode, event.spelled_name.clone()) {
                (OP_CALL_ANIMATION, Some(name)) => calls.push(name),
                (OP_STOP_ANIMATION, Some(name)) => stops.push((name, event.start_time)),
                _ => {}
            }
        }
    }
    let [drop_animation] = calls.as_slice() else {
        return Err(LaunchEndPoseError::Chain(format!(
            "`{DROP_SEQUENCE}` must call exactly one animation; it calls {}",
            calls.len()
        )));
    };
    let matching: Vec<f32> = stops
        .iter()
        .filter(|(name, _)| name == drop_animation)
        .map(|(_, time)| *time)
        .collect();
    let [stop_time_s] = matching.as_slice() else {
        return Err(LaunchEndPoseError::Chain(format!(
            "`{DROP_SEQUENCE}` must stop `{drop_animation}` exactly once; it does so {} times",
            matching.len()
        )));
    };
    let stop_time_s = *stop_time_s;

    // 3. The drop animation's motion event for the player.
    let drop = record_named(&records, drop_animation)?;
    let mut motions = Vec::new();
    for found in ordinary_events(drop)? {
        if found.opcode != OP_MOTION_SI_SCRIPT {
            continue;
        }
        let event = found.bytes;
        if node_name(drop, i64::from(word(event, 0x10))).as_deref() != Some(PLAYER_NODE) {
            continue;
        }
        if word(event, 0xc) != 0 {
            return Err(LaunchEndPoseError::Chain(
                "the player's motion event is relative (flag word not zero), so its track is a \
                 base plus an offset the reader does not model"
                    .to_owned(),
            ));
        }
        let slot = word(event, 0x14) as usize;
        let words = drop
            .table(AnimationRecordTableKind::IndexWords)
            .and_then(|table| table.entry(slot))
            .ok_or_else(|| LaunchEndPoseError::Chain(format!("IndexWords has no entry {slot}")))?;
        motions.push((found.sequence, found.offset, word(words, 0) as usize));
    }
    let [(motion_sequence, motion_offset, block_index)] = motions.as_slice() else {
        return Err(LaunchEndPoseError::Chain(format!(
            "`{drop_animation}` must move `{PLAYER_NODE}` with exactly one motion event; it has {}",
            motions.len()
        )));
    };
    let block = locate_block(&tracks, *block_index)?;
    if text(block.object_name()) != PLAYER_NODE {
        return Err(LaunchEndPoseError::Chain(format!(
            "track block {block_index} moves `{}`, not `{PLAYER_NODE}`",
            text(block.object_name())
        )));
    }
    if block.mode() != 1 {
        return Err(LaunchEndPoseError::Track(format!(
            "track block {block_index} has mode {}; only the cubic mode is read",
            block.mode()
        )));
    }

    // 4. Evaluate at the stop time.
    let segments = block.segments();
    let last = segments.len().saturating_sub(1);
    let (segment_index, segment) = segments
        .iter()
        .enumerate()
        .find(|(index, s)| {
            s.start() <= stop_time_s
                && (stop_time_s < s.end() || (*index == last && stop_time_s <= s.end()))
        })
        .ok_or_else(|| {
            LaunchEndPoseError::Track(format!(
                "the stop time {stop_time_s} s is outside the track's {} s",
                block.duration()
            ))
        })?;
    let (Some(position), Some(rotation)) = (segment.position(), segment.rotation()) else {
        return Err(LaunchEndPoseError::Track(format!(
            "segment {segment_index} carries no position or no rotation part (flags {}); \
             what an absent part does is not measured",
            segment.flags()
        )));
    };
    let u = stop_time_s - segment.start();
    let at = |c: usize, u: f32| position.cubic_at(c, u);
    let local_position = [at(0, u), at(1, u), at(2, u)];
    let cubic_velocity = [0, 1, 2].map(|c| {
        let [_, a1, a2, a3] = position.cubic(c);
        a1 + u * (2.0 * a2 + 3.0 * u * a3)
    });
    // The preceding rate: position at the stop time against 0.1 s earlier.
    let earlier = (stop_time_s - 0.1).max(0.0);
    let earlier_position = {
        let (_, s) = segments
            .iter()
            .enumerate()
            .find(|(_, s)| s.start() <= earlier && earlier < s.end())
            .ok_or_else(|| LaunchEndPoseError::Track("no segment 0.1 s earlier".to_owned()))?;
        let part = s.position().ok_or_else(|| {
            LaunchEndPoseError::Track("no position part 0.1 s earlier".to_owned())
        })?;
        let u = earlier - s.start();
        [0, 1, 2].map(|c| part.cubic_at(c, u))
    };
    let dt = stop_time_s - earlier;
    let preceding_rate = [0, 1, 2].map(|c| (local_position[c] - earlier_position[c]) / dt);
    let q = rotation.start_quaternion();
    let vector_norm = (q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let rotation_angle_degrees = 2.0 * vector_norm.atan2(q[0]).to_degrees();
    let rotation_vector = [0, 1, 2].map(|c| rotation.cubic_at(c, u));

    // 5. Carry it through the parent's pose.
    let parent_pose = parent
        .iter()
        .find(|pose| pose.node == parent_node)
        .ok_or_else(|| LaunchEndPoseError::NoParentPose(parent_node.clone()))?;
    let carried = rotate(parent_pose.orientation, local_position.map(f64::from));
    let composed_position_m = [0, 1, 2].map(|c| f64::from(parent_pose.position_m[c]) + carried[c]);
    let composed_cubic_velocity_m_s =
        rotate(parent_pose.orientation, cubic_velocity.map(f64::from));

    Ok(LaunchEndPose {
        parent_node: parent_node.clone(),
        parent_event: EventSite {
            record: intro_animation.to_owned(),
            sequence: parent_sequence.clone(),
            offset: *parent_offset,
        },
        drop_animation: drop_animation.clone(),
        stop_time_s,
        motion_event: EventSite {
            record: drop_animation.clone(),
            sequence: motion_sequence.clone(),
            offset: *motion_offset,
        },
        local: LocalPose {
            track_path: zan_rows
                .get(*block_index)
                .cloned()
                .unwrap_or_else(|| text(block.path())),
            block_index: *block_index,
            segment_index,
            segment_start_s: segment.start(),
            segment_end_s: segment.end(),
            position_m: local_position,
            cubic_velocity_m_s: cubic_velocity,
            preceding_rate_m_s: preceding_rate,
            segment_start_quaternion_wxyz: q,
            rotation_vector,
            rotation_angle_degrees,
        },
        composed_position_m,
        composed_cubic_velocity_m_s,
        rotation_is_pure_pitch: q[2].abs() < 1e-6 && q[3].abs() < 1e-6,
    })
}
