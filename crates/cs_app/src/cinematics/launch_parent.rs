//! #1679 (`PARITY-M01-ATMO-E4`): where the Pandora (`piratezep`) hangs in the
//! scene graph, and whether anything moves it during the launch cinematic.
//!
//! The finding is `docs/findings/2026-10-19-m01-launch-parent-node.md`. It
//! continues [`super::launch_end_pose`] (#1660, unknowns `parent-of-parent`
//! and `parent-motion`).
//!
//! * [`measure_ancestry`] walks the stored parent slots of `gamez.zbd`'s node
//!   array from a named node up to a root and reports every link with the
//!   transform it stores; it composes nothing.
//! * [`events_naming_node`] lists the animation events of a carrier that name
//!   a node through a layout measured here, and [`NodeRole::writes_transform`]
//!   says which of them could move it.
//! * [`ZEPPELIN_ENGINE_FACTS`] are the instruction-level facts about the
//!   zeppelin actor update (static, never run); [`PARENT_UNKNOWNS`] is what
//!   stays open.

use std::fmt;

use cs_formats::gamez::{GameZNodeError, GameZNodes, read_gamez_nodes};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    AnimationRecord, AnimationRecordTableKind, HeaderStatus, ZbdFamily, ZbdProbe, dispatch,
    family_record, read_animation_index,
};
use cs_types::install::RelativePath;

use super::launch_end_pose::{half, text, word};
use super::launch_release::EngineFact;
use crate::animation::events::walk_event_stream;

/// The node the cinematic parents the player to.
pub const PANDORA_NODE: &str = "piratezep";

/// One link of the walk from a node to a root.
#[derive(Clone, Debug, PartialEq)]
pub struct AncestryLink {
    /// Array slot.
    pub index: u32,
    /// Authored name.
    pub name: String,
    /// The node-kind label (`world`, `object3d`, ...).
    pub kind: &'static str,
    /// The stored translation, when the node is an object that stores a
    /// transform at all.
    pub translation: Option<[f32; 3]>,
    /// The stored euler triple, same condition.
    pub rotation: Option<[f32; 3]>,
    /// Whether this node lists the previous link of the walk among its children.
    pub lists_previous_as_child: bool,
}

/// The walk from a node to the root of its tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Ancestry {
    /// The node itself first, the root last.
    pub chain: Vec<AncestryLink>,
    /// How many nodes in the container carry the node's name.
    pub same_name_count: usize,
}

impl Ancestry {
    /// The root of the walk.
    #[must_use]
    pub fn root(&self) -> &AncestryLink {
        self.chain.last().expect("a chain has at least the node")
    }

    /// Whether the walk ends in a `World` node (the container the engine's
    /// `world1` is).
    #[must_use]
    pub fn ends_in_world_node(&self) -> bool {
        self.root().kind == "world"
    }

    /// Whether every ancestor above the node stores no transform of its own
    /// (a non-object node, or an identity record), so the node's local numbers
    /// are in the root's frame unchanged.
    #[must_use]
    pub fn ancestors_store_no_transform(&self) -> bool {
        self.chain.iter().skip(1).all(|link| {
            link.translation.is_none_or(|t| t == [0.0; 3])
                && link.rotation.is_none_or(|r| r == [0.0; 3])
        })
    }
}

/// Why the walk could not be made.
#[derive(Clone, Debug, PartialEq)]
pub enum LaunchParentError {
    /// The node array refused to decode.
    Nodes(String),
    /// No node carries the name.
    NotFound(String),
    /// A parent slot loops back.
    Cycle(String),
    /// A parent slot names no stored node.
    DanglingParent {
        /// The node whose slot dangles.
        node: String,
    },
}

impl fmt::Display for LaunchParentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nodes(e) => write!(f, "the node array refuses: {e}"),
            Self::NotFound(n) => write!(f, "no node named `{n}`"),
            Self::Cycle(n) => write!(f, "the parent slots of `{n}` form a cycle"),
            Self::DanglingParent { node } => write!(f, "`{node}` stores a dangling parent slot"),
        }
    }
}

impl std::error::Error for LaunchParentError {}

impl From<GameZNodeError> for LaunchParentError {
    fn from(error: GameZNodeError) -> Self {
        Self::Nodes(error.to_string())
    }
}

/// Reads `gamez_bytes` (a CS GameZ container, labelled `label`) and walks
/// `node_name` to its root.
///
/// # Errors
///
/// [`LaunchParentError`] when the container does not decode, the name is not
/// stored, or the parent slots do not lead to a root.
pub fn measure_ancestry(
    label: &str,
    gamez_bytes: &[u8],
    node_name: &str,
) -> Result<Ancestry, LaunchParentError> {
    let mut context = ParseContext::with_defaults(label);
    let nodes = read_gamez_nodes(&mut context, gamez_bytes)?;
    ancestry_in(&nodes, node_name)
}

/// The walk over an already decoded node array.
///
/// # Errors
///
/// As [`measure_ancestry`], minus the decode.
pub fn ancestry_in(nodes: &GameZNodes, node_name: &str) -> Result<Ancestry, LaunchParentError> {
    let same_name_count = nodes.nodes.iter().filter(|n| n.name == node_name).count();
    let start = nodes
        .nodes
        .iter()
        .find(|n| n.name == node_name)
        .ok_or_else(|| LaunchParentError::NotFound(node_name.to_owned()))?;
    let mut chain: Vec<AncestryLink> = Vec::new();
    let mut current = start;
    let mut previous: Option<u32> = None;
    loop {
        if chain.iter().any(|l| l.index == current.index) {
            return Err(LaunchParentError::Cycle(node_name.to_owned()));
        }
        let object = current.object3d();
        chain.push(AncestryLink {
            index: current.index,
            name: current.name.clone(),
            kind: current.kind.label(),
            translation: object.map(|o| o.translation),
            rotation: object.map(|o| o.rotation),
            lists_previous_as_child: previous.is_none_or(|p| current.children.contains(&p)),
        });
        previous = Some(current.index);
        match current.parent {
            None => break,
            Some(slot) => {
                current = nodes
                    .get(slot)
                    .ok_or_else(|| LaunchParentError::DanglingParent {
                        node: current.name.clone(),
                    })?;
            }
        }
    }
    Ok(Ancestry {
        chain,
        same_name_count,
    })
}

/// `OBJECT_ACTIVE_STATE`: the node is word `0x10`.
const OP_ACTIVE_STATE: u8 = 6;
/// `OBJECT_ROTATE_STATE`: the node is word `0x1c`.
const OP_ROTATE_STATE: u8 = 9;
/// `OBJECT_MOTION_SI_SCRIPT`: the node is word `0x10`.
const OP_MOTION_SI_SCRIPT: u8 = 12;
/// `OBJECT_ADD_CHILD`: parent half `0xc`, child half `0xe`.
const OP_ADD_CHILD: u8 = 15;
/// `OBJECT_DELETE_CHILD`: same layout.
const OP_DELETE_CHILD: u8 = 16;

/// How an event refers to a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRole {
    /// The event changes the node's active state.
    ActiveState,
    /// The event changes the node's rotation state.
    RotateState,
    /// The event drives the node's position and rotation from a motion track.
    Motion,
    /// The node is the parent of an added or removed child.
    ParentOfChild,
    /// The node is the child that is added or removed.
    ChildOfParent,
}

/// One event that names the node in its record's `Nodes` table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeEvent {
    /// The record's `anim_name`.
    pub record: String,
    /// The sequence's name (empty for an unnamed one).
    pub sequence: String,
    /// The opcode.
    pub opcode: u8,
    /// How the event refers to the node.
    pub role: NodeRole,
}

/// Every event of the container that names `node_name` through a layout
/// measured here (opcodes 6, 9, 12, 15, 16).
///
/// # Errors
///
/// A description of the first container or event stream that does not read.
pub fn events_naming_node(
    path: &RelativePath,
    bytes: &[u8],
    node_name: &str,
) -> Result<Vec<NodeEvent>, String> {
    let key = path.logical_key();
    let mut context = ParseContext::with_defaults(&key);
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    let decision = dispatch(ZbdProbe::new(&key, path, &bytes[..needed]))
        .map_err(|error| format!("{key}: dispatch refused ({})", error.code()))?;
    if !matches!(decision.header_status(), HeaderStatus::Validated { .. })
        || decision.family() != ZbdFamily::Animation
    {
        return Err(format!("{key}: not a validated animation container"));
    }
    let index = read_animation_index(&mut context, decision, bytes)
        .map_err(|error| format!("{key}: index ({})", error.code()))?;
    let payload = index
        .payload()
        .map_err(|error| format!("{key}: payload ({})", error.code()))?;
    let records = payload
        .records()
        .map_err(|error| format!("{key}: records ({})", error.code()))?;
    let mut out = Vec::new();
    for record in records.iter() {
        let Some(slot) = node_slot(record, node_name) else {
            continue;
        };
        for sequence in record.sequences() {
            let events = sequence.events();
            let raw =
                walk_event_stream(events).map_err(|error| format!("event stream: {error}"))?;
            for event in raw {
                let body = &events
                    [event.offset as usize..(event.offset + u64::from(event.length)) as usize];
                let word_is =
                    |offset: usize| body.len() >= offset + 4 && word(body, offset) as usize == slot;
                let half_is = |offset: usize| {
                    body.len() >= offset + 2 && usize::try_from(half(body, offset)) == Ok(slot)
                };
                let mut roles = Vec::new();
                match event.opcode {
                    OP_ACTIVE_STATE if word_is(0x10) => roles.push(NodeRole::ActiveState),
                    OP_ROTATE_STATE if word_is(0x1c) => roles.push(NodeRole::RotateState),
                    OP_MOTION_SI_SCRIPT if word_is(0x10) => roles.push(NodeRole::Motion),
                    OP_ADD_CHILD | OP_DELETE_CHILD => {
                        if half_is(0xc) {
                            roles.push(NodeRole::ParentOfChild);
                        }
                        if half_is(0xe) {
                            roles.push(NodeRole::ChildOfParent);
                        }
                    }
                    _ => {}
                }
                for role in roles {
                    out.push(NodeEvent {
                        record: text(record.anim_name()),
                        sequence: text(sequence.name()),
                        opcode: event.opcode,
                        role,
                    });
                }
            }
        }
    }
    Ok(out)
}

fn node_slot(record: &AnimationRecord<'_>, node_name: &str) -> Option<usize> {
    let table = record.table(AnimationRecordTableKind::Nodes)?;
    (0..table.count()).find(|&i| table.name(i).is_some_and(|n| n == node_name.as_bytes()))
}

impl NodeRole {
    /// Whether an event in this role writes the node's own position or
    /// rotation (a motion track, a rotation state), as opposed to changing its
    /// visibility or its list of children.
    #[must_use]
    pub const fn writes_transform(self) -> bool {
        matches!(self, Self::Motion | Self::RotateState)
    }
}

/// The static facts the zeppelin-motion half of the finding rests on.
/// Provenance of every row is `static` (instruction bytes of
/// `$CS_ENGINE_IMAGE`, image base `0x400000`); the acceptance test
/// `accept_parity_m01_atmo_e4_engine_facts_are_the_bytes_the_finding_read`
/// re-reads each.
pub const ZEPPELIN_ENGINE_FACTS: &[EngineFact] = &[
    EngineFact {
        id: "zeppelin-loader-slots",
        va: 0x4bda75,
        claim: "the `zeppelins.zrd` record loader `0x4bd8d0` stores `position` into the \
                zeppelin object at `+0x20..+0x28` (`0x4bda75`, `0x4bda7e`, `0x4bda8d`), \
                `yaw`/`pitch` at `+0x2c`/`+0x30`, `max_speed` at `+0x9c` (`0x4bdad6`) and \
                `max_accel` at `+0xa8` (`0x4bdaf4`)",
    },
    EngineFact {
        id: "zeppelin-constructor-starts-idle",
        va: 0x4bd483,
        claim: "the constructor `0x4bd460` clears the bytes `+0x4`, `+0x5`, `+0x6` \
                (`0x4bd483`, `0x4bd486`, `0x4bd489`, all `eax` = 0 from `0x4bd478`) and \
                the current speed `+0xa4` (`0x4bd52b`): a new zeppelin is not enabled and \
                has speed zero",
    },
    EngineFact {
        id: "zeppelin-enable-all",
        va: 0x4bd370,
        claim: "`0x4bd370` sets byte `+0x4` to 1 on every zeppelin of the manager at \
                `0x71df80`; `0x4bd390` sets it to 0. `0x4bd370` is called from `0x41f2e7`, \
                `0x496610` and `0x4966aa`; `0x4bd390` only from `0x41f256`",
    },
    EngineFact {
        id: "zeppelin-enable-callbacks",
        va: 0x47e510,
        claim: "callback id 0x392 (handler `0x47e503`, not multiplayer) calls `0x41f2e0` \
                (`0x47e510`), which enables every zeppelin; callback id 0x391 (handler \
                `0x47e4b8`) calls `0x41f250` (`0x47e4b8`), which disables them. M01's \
                intro `RESET_SEQUENCE` fires 0x392; `player_setup` (`LOAD_GAME_START`) \
                fires 0x391. Both functions have other callers, not read",
    },
    EngineFact {
        id: "zeppelin-update-needs-enable-byte",
        va: 0x4bfa16,
        claim: "the zeppelin per-frame update `0x4bf9d0` reads byte `+0x4` (`0x4bfa0f`) and \
                branches over all movement to `0x4bfc6f` when it is zero (`0x4bfa16`)",
    },
    EngineFact {
        id: "zeppelin-moves-by-speed",
        va: 0x4bf7af,
        claim: "`0x4bf710` adds `speed (+0xa4) × game_dt` along the heading to the \
                position `+0x20..+0x28` (`0x4bf7af`..`0x4bf7d9`) and then writes the node \
                (`0x4bf930` → `0x4bf9b0` → setter `0x4d1d50`). The speed is changed only \
                by `0x4bf240` (`0x4bf278`..`0x4bf2ab`), toward the target speed its \
                callers pass, at the rate `+0xac × game_dt`; the other writers are the \
                constructor and the save-game restore `0x4c0a90` (`0x4c0adc`)",
    },
    EngineFact {
        id: "zeppelin-update-only-in-world-update",
        va: 0x489c75,
        claim: "the manager update `0x4bd3b0` (which calls `0x4bf9d0` for each zeppelin) has \
                one caller, `0x489c75`, inside the world update `0x4897c0`",
    },
    EngineFact {
        id: "world-update-skipped-during-intro",
        va: 0x4897f3,
        claim: "`0x4897c0` branches to `0x489e30` (`0x4897f3`) when `[0x71c50c]` \
                (`0x4897dd`) is non-zero, past `0x489c75`; no branch outside \
                `0x4897c0`..`0x489e30` targets the span `0x4897f9`..`0x489e30`. \
                `[0x71c50c]` is set by callback 20 and cleared by callback 0 when the \
                same animation finishes (#1678): the zeppelins are not updated while the \
                intro runs",
    },
];

/// A link of the chain that static analysis does not settle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParentUnknown {
    /// Stable identifier.
    pub id: &'static str,
    /// What is not known.
    pub what: &'static str,
    /// The content that depends on it.
    pub affects: &'static str,
    /// What would resolve it.
    pub resolver: &'static str,
}

/// Every link this module's finding does not claim.
pub const PARENT_UNKNOWNS: &[ParentUnknown] = &[
    ParentUnknown {
        id: "first-frame-order",
        what: "whether a world update `0x4897c0` runs between the intro's \
               `RESET_SEQUENCE` (callback 0x392 enables the zeppelins) and \
               `callback_sequence` (callback 20 sets `[0x71c50c]`). Both run when the \
               intro starts; their order against the frame's world update was not traced. \
               At most that one update moves the Pandora, and only by the speed one \
               ramp step gives it (`+0xac × game_dt`) times `game_dt`",
        affects: "the Pandora's position by at most one frame's displacement",
        resolver: "a follow-up reading the animation start `0x4ed340` against the \
                   frame loop; or the owner's reference capture",
    },
    ParentUnknown {
        id: "pre-intro-frames",
        what: "whether any frame of the world update runs between mission load and the \
               intro's start. The zeppelins are not enabled before it (`+0x4` = 0 from \
               the constructor, and `player_setup` disables them), so the update \
               returns before moving them",
        affects: "nothing measured: the Pandora stays at its spawn pose",
        resolver: "none needed unless another enabler (`0x496610`, `0x4966aa`, not read) \
                   runs at load",
    },
    ParentUnknown {
        id: "other-enablers",
        what: "`0x496610` and `0x4966aa` also enable every zeppelin; the functions \
               around them were not read",
        affects: "whether the zeppelins are enabled before the intro starts",
        resolver: "a follow-up reading `0x4965d0`..`0x4966b0`",
    },
    ParentUnknown {
        id: "multiplayer-net-path",
        what: "in multiplayer (`0x440ad0` non-zero) `0x4bf9d0` calls the interpolation \
               `0x470550`, which also writes the zeppelin position and the node",
        affects: "multiplayer only; not M01 single player",
        resolver: "the multiplayer tasks",
    },
    ParentUnknown {
        id: "save-game-restore",
        what: "`0x4c0a90` restores speed `+0xa4` and the position from a saved state",
        affects: "a loaded game only; not a new campaign's intro",
        resolver: "the save-game tasks",
    },
    ParentUnknown {
        id: "post-intro-motion",
        what: "once the intro finishes (`[0x71c50c]` cleared) the zeppelin update runs \
               and the Pandora can move along its route (route table `[0x64f610]`, \
               `[0x64f614]`; target speed from `max_speed`); the route was not read",
        affects: "the Pandora's position after the intro, not during it",
        resolver: "a follow-up reading `0x4bf9d0`'s route selection",
    },
    ParentUnknown {
        id: "no-run",
        what: "no original executable run exists on this machine and no \
               `original-launch/` frame: every statement is static",
        affects: "every claim; nothing is `verified_original`",
        resolver: "the owner's reference capture (REF-OWNER-FIRST-CAPTURE)",
    },
];
