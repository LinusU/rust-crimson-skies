//! The mission world-actor spawn adapter (M01-LC-WORLD-ACTOR-SPAWN, #772).
//!
//! The `zeppelins.zrd` member of a mission scope's reader archive (#574's
//! measured carrier grammar) is lowered into a
//! [`DeclaredWorldActorProgram`] through production code: each record's
//! `node` name is joined to the scope's world container through the same
//! [`world_scene_graph_from_gamez`] path `world_geometry` measures, the
//! record's `position` is bound as the actor's spawn position in metres,
//! and the program is handed to [`lower_world_actors`] and
//! [`WorldActorSession::launch`] — the same consumer a mission session
//! uses. Nothing a record does not state is invented: every field the
//! measurement cannot source arrives as [`Resolved::Unknown`] with its own
//! claim id, the lowering's refusals are kept verbatim, and the report
//! names them all.
//!
//! What the measurements support, and where each claim stops:
//!
//! * **The carrier and its field shapes are decoded** (#574). All 26 key
//!   *meanings* stay `KeyMeaning::Unknown` — the grammar is measured; what
//!   each field does to the original is not, except where the executable
//!   measurement below narrows it.
//! * **The spawn position binds under the measured write order (#814).**
//!   The owner-supplied decrypted image reads the record's named `position`
//!   (three floats) into the spawned object's position slots
//!   `obj+0x20`..`obj+0x28` (`0x4bda64`..`0x4bdac1`, #770), and the stored
//!   unit is the metre with the identity axis map (#436's owner note,
//!   #677's census). At the end of the same load routine the object hands
//!   the triple to `Object3d`'s position setter for the node its `node` key
//!   resolved (`0x4becf8` → `0x4bf930` → `0x4bf9b0` → `0x4d1d50`), which
//!   stores it at `class+0x54`..`+0x5c`. `placezeps.zrd`'s
//!   `OBJECT_TRANSLATE_STATE` writes the *same* slots of the *same* node
//!   through the *same* setter (`0x4e8de0` → `0x4d1d50`), and its
//!   `ON_STARTUP` states run from the animation player's first tick after
//!   the mission start — after the spawn write — so each record's
//!   [`SpawnedZeppelinActor::position`] carries the source the original
//!   applies last and [`SpawnedZeppelinActor::position_residue`] names the
//!   one it overwrites. `position_m` is [`Resolved::Known`] under
//!   [`SPAWN_POSE_CLAIM`]. M01's three records sit inside `c1c`'s measured
//!   node extent — the frame the value claims is corroborated by
//!   containment, not asserted.
//! * **The spawn attitude binds under the measured compose and the measured
//!   write order (#792).** `yaw`/`pitch` are degrees→radians into
//!   `obj+0x2c`/`obj+0x30` (measured at `0x4bda64`); at the end of the same
//!   load routine the object hands both to `Object3d::SetRotation` for the
//!   node its `node` key resolved (`0x4becf8` → `0x4bf930` → `0x4bf950` →
//!   `0x4d1a30`), which stores them at `class+0x18` (pitch) and `class+0x1c`
//!   (yaw) with `class+0x20` clear — and #770 measured the matrix build over
//!   those slots to be `M = Ry(r1)·Rx(r0)·Rz(r2)`, so the node's orientation
//!   is `Ry(yaw)·Rx(pitch)`, right-handed, in #436's identity-mapped metre
//!   frame. `placezeps.zrd`'s `OBJECT_ROTATE_STATE` writes the *same* slots
//!   of the *same* node through the *same* setter (`0x4e8cf5` → `0x4d1a30`),
//!   and its `ON_STARTUP` states run from the animation player's first tick
//!   after the mission start — after the spawn write — so each record's
//!   [`SpawnedZeppelinActor::attitude`] carries the source the original
//!   applies last and [`SpawnedZeppelinActor::attitude_residue`] names the
//!   one it overwrites. `orientation` is [`Resolved::Known`] under
//!   [`SPAWN_ATTITUDE_CLAIM`].
//! * **The faction binds the measured allegiance (#1155).** A record that
//!   states no `team` is *not* allegiance-less: while the record loads, the
//!   original stores the outcome of its scene-graph resolver at `zep+0xe0`
//!   (`0x4bf080` → `0x4bef90`, called from `0x4bd96a` and stored at
//!   `zep+0xe0` by `0x4bd977`) — a staged
//!   `targets.zrd` record for a marked node first, then the `ai.zrd` turret
//!   binding table (`0x4ac170` → `0x71d910`, loaded before
//!   `zeppelins.zrd`), then the children depth-first, first nonzero wins,
//!   else `0`. A recognized `team` spelling then overrides it
//!   (`enemy`→2, `ally`→1, `neutral`→0). `faction` is [`Resolved::Known`]
//!   under [`ALLEGIANCE_RESOLVED_CLAIM`] with the vocabulary's own spelling
//!   (`neutral`/`ally`/`enemy`), or [`Resolved::Unknown`] under
//!   [`ALLEGIANCE_OPEN_CLAIM`] when the measured path cannot settle — a
//!   gated marked node, an unreadable carrier, an unmodelled `NODES`
//!   element. M01's three team-less records resolve to `piratezep` → 1
//!   (`ally`), `workersvoyagezep` → 2 (`enemy`) and `blackswanzep` → 2
//!   (`enemy`); `docs/findings/2026-10-09-m01-lc-zeppelin-allegiance.md`
//!   records every measured address.
//! * **The motion is `Held` by shape, not by claim.** The carrier states no
//!   route polyline, keyframe schedule, velocity or carrier — a fixed pose
//!   at spawn is the only [`DeclaredMotion`] variant whose fields the record
//!   can fill. Whether the original then drives the actor (`targets`,
//!   `deactivated`, the undecoded mission program) is outside this
//!   program's authority and unmeasured; [`MOTION_RESIDUE`] names it
//!   instead of asserting it.
//! * **Sockets, pickups, support edges and gate transitions** have no
//!   carrier fields at all, so the program declares none. The session tick
//!   rate is the host's designed cadence ([`SESSION_RATE_CLAIM`]), never an
//!   original measurement.
//!
//! Nothing here is `verified_original`: measured fields are
//! [`ClaimStatus::ObservedTool`], the session rate is
//! [`ClaimStatus::Designed`], and every refusal is [`ClaimStatus::Unknown`].
//! `docs/findings/2026-10-08-m01-lc-world-actor-spawn.md` records the claim
//! boundary and the residues verbatim.

use std::collections::HashMap;
use std::path::Path;

use cs_assets::install::Discovery;
use cs_content::coordinates::SourceAdapter;
use cs_content::objectives::ProgramActor;
use cs_content::scene::BindingMap;
use cs_content::stunts::{ZrdValue, decode_zrd, zrd_field, zrd_flat_fields};
use cs_content::world::world_scene_graph_from_gamez;
use cs_content::world_actors::{
    DeclaredMotion, DeclaredWorldActor, DeclaredWorldActorKind, DeclaredWorldActorParts,
    DeclaredWorldActorProgram,
};
use cs_formats::gamez::{GameZNodes, RawNode, read_gamez_nodes};
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::placezeps::{PLACEZEPS_MEMBER, StateKind, read_placezeps_member};
use cs_formats::zbd::zeppelins::read_zeppelins_member;
use cs_script::runtime::SessionGeneration;
use cs_sim::world_actors::Quat;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::install::InstallFileRecord;

use crate::world_actors::{
    LoweredWorldActors, WorldActorLaunchError, WorldActorLowerError, WorldActorSession,
    lower_world_actors,
};

/// The reader member that carries a scope's placed world actors (#574).
pub const ZEPPELIN_MEMBER: &str = "zeppelins.zrd";

/// The session cadence the launch path steps a lowered program at — the
/// same 64 Hz [`crate::synthetic::TICK_HZ`] states and
/// `MissionAnimationPlayer::new` is handed in `mission_launch.rs`. It is a
/// designed contract of the reimplementation, not an original measurement.
pub const SESSION_TICKS_PER_SECOND: u32 = 64;

/// The claim the spawn position binds under (#814).
///
/// `0x4bda64`..`0x4bdac1` of the owner-supplied image reads the record's
/// named `position` into the spawned object's `obj+0x20`..`obj+0x28` (#770);
/// the stored unit is the metre with the identity axis map (#436, #677).
/// Two writers then reach the node's `Object3d` position slots
/// `class+0x54`..`+0x5c` through the same setter `0x4d1d50`: the spawn's
/// apply (`0x4becf8` → `0x4bf930` → `0x4bf9b0`, inside the mission-start
/// call stack) and `placezeps.zrd`'s startup `OBJECT_TRANSLATE_STATE`
/// (`0x4e8de0`, dispatched from the animation instance's first tick — the
/// later write on #792's measured order). The claim is the value that
/// order leaves on the node: the startup translate's absolute triple for
/// a node the member states one for (provenanced from the statement's
/// `STATE` span), the record's own `position` for a node it does not —
/// and the losing source stays named on the row
/// ([`SpawnedZeppelinActor::position_residue`]). The carrier's value still
/// lives at `obj+0x20`..`obj+0x28` and is handed back to the node only if
/// the object's drive byte is set again, which nothing on a single-player
/// launch does (#792 §3).
///
/// `docs/findings/2026-10-09-m01-lc-zeppelin-placement-position.md` records
/// the re-verified addresses and the values M01 binds.
pub const SPAWN_POSE_CLAIM: &str = "f34-world.zeppelin-spawn-pose";

/// The claim the composed spawn attitude binds under (#792).
///
/// Two measurements close it:
///
/// * **the compose** — the record's `yaw`/`pitch` reach the node as an
///   `Object3d` rotation triple through `0x4becf8` → `0x4bf930` → `0x4bf950`
///   → `0x4d1a30` (`class+0x18` = pitch, `class+0x1c` = yaw, `class+0x20` =
///   0), and #770 measured the build over those slots (`0x53bf40`, inverse
///   `0x53df30`) to be `M = Ry(r1)·Rx(r0)·Rz(r2)` — so `Ry(yaw)·Rx(pitch)`,
///   right-handed, in the identity-mapped metre frame;
/// * **the write order** — `placezeps.zrd`'s `OBJECT_ROTATE_STATE` writes the
///   same slots of the same node through the same setter (`0x4e8cf5` →
///   `0x4d1a30`), once, from the animation instance's first tick (`0x4ecd20`,
///   registered at `0x4eda92`, dispatching at `0x4ecc7f`), while the mission
///   start clears the zeppelin object's drive byte before the spawn
///   (`0x4648e0`/`0x4655f0` → `0x41f250` → `0x4bd390`) and nothing on a
///   single-player launch sets it again — so the startup state is what the
///   node keeps, and the carrier's spawn attitude is the residue.
///
/// `docs/findings/2026-10-09-m01-lc-zeppelin-attitude.md` records every
/// address and the ordering it establishes.
pub const SPAWN_ATTITUDE_CLAIM: &str = "f34-world.zeppelin-attitude-compose";

/// The claim an attitude stays open under (#792): the startup carrier
/// `placezeps.zrd` could not be read, so which source the original applies
/// last cannot be settled for this scope and no orientation is guessed.
pub const ATTITUDE_PRECEDENCE_UNKNOWN_CLAIM: &str =
    "f34-world.zeppelin-attitude-precedence-unknown";

/// Why an attitude stays [`Resolved::Unknown`] when the startup carrier
/// refuses: the spawn half is measured, the ordering over it is not.
pub const ATTITUDE_PRECEDENCE_UNKNOWN_REASON_PREFIX: &str = "the spawn attitude composes under a measured rule, but `placezeps.zrd`, the startup \
     placement that writes the same Object3d rotation slots after it, could not be read, so \
     which source the original applies last is open: ";

/// The claim a position stays open under (#814): which value the original
/// applies last could not be settled for this scope — the startup carrier
/// `placezeps.zrd` could not be read, or its translate for the node carries
/// a key the placement grammar leaves unmodelled (`RELATIVE`, `AT_NODE`,
/// …), so the applied position is not the bare `STATE` triple and none is
/// guessed.
pub const POSITION_PRECEDENCE_UNKNOWN_CLAIM: &str =
    "f34-world.zeppelin-position-precedence-unknown";

/// Why a position stays [`Resolved::Unknown`] when the startup carrier
/// refuses: the spawn half is measured, the ordering over it is not.
pub const POSITION_PRECEDENCE_UNKNOWN_REASON_PREFIX: &str = "the spawn position converts under a measured rule, but `placezeps.zrd`, the startup \
     placement that writes the same Object3d position slots after it, could not be read, so \
     which source the original applies last is open: ";

/// The member whose `TURRET` records the original's allegiance loader binds
/// (#1155): `0x4ac170` decodes it at mission start (`0x465596` — before
/// `zeppelins.zrd` at `0x4655c0`) and its `NODES` entries build the
/// `0x71d910` table `0x4bef90` consults.
pub const TURRET_MEMBER: &str = "ai.zrd";

/// The member whose `nodes` elements stage the `+0x8d` allegiance records
/// `0x4bef90` checks before the turret table (#1155): `0x4a2e00` builds one
/// only for a node whose stored `unk040` carries the mark bit *and* which a
/// `nodes` element matches.
pub const TARGETS_MEMBER: &str = "targets.zrd";

/// Bit 31 of a stored node's `unk040` word — the mark `0x4a2e00` requires
/// before it stages a `+0x8d` allegiance record for a `targets.zrd`-matched
/// node. The word's low 2-bit lanes carry the per-side ints the record's
/// `+8` is extracted from.
const ALLEGIANCE_MARKED: u32 = 0x8000_0000;

/// The claim a faction binds under when the measured allegiance path
/// resolves it (#1155).
///
/// The original computes a team int per record — the `0x4bef90` resolver's
/// result written at `0x4bd977`, then a recognized `team` spelling's
/// override (`0x4bd987`..`0x4bda5e`) — and the bound faction is the
/// vocabulary's own spelling: `neutral`/`ally`/`enemy` for `0`/`1`/`2`.
pub const ALLEGIANCE_RESOLVED_CLAIM: &str = "f34-world.zeppelin-allegiance";

/// The claim an allegiance stays [`Resolved::Unknown`] under when the
/// measured path cannot settle it (#1155) — never a guessed faction.
pub const ALLEGIANCE_OPEN_CLAIM: &str = "f34-world.zeppelin-allegiance-open";

/// The measured side vocabulary of the loader (`0x62b6c4`/`0x62b6cc`/
/// `0x62b6d4` → 2/1/0): the `team` key's override spellings and the
/// resolver ints' faction spellings — the same three words name both.
fn team_key(team: i64) -> Option<&'static str> {
    match team {
        0 => Some("neutral"),
        1 => Some("ally"),
        2 => Some("enemy"),
        _ => None,
    }
}

/// `0x4abea0`'s pattern rule, measured over the image: `*` matches exactly
/// one digit (`0`–`9`), every other byte is literal, and the pattern must
/// consume the whole name — `ctur*` matches `ctur1`, never `ctur01`.
fn wildcard_name(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.as_bytes(), name.as_bytes());
    if pattern.len() != name.len() {
        return false;
    }
    pattern.iter().zip(name).all(|(&p, &n)| {
        if p == b'*' {
            n.is_ascii_digit()
        } else {
            p == n
        }
    })
}

/// Which measured source settled a record's allegiance (#1155).
#[derive(Clone, Debug, PartialEq)]
pub enum AllegianceSource {
    /// The record's own `team` key: the loader's recognized spellings
    /// override the resolved value outright (`0x4bd987`..`0x4bda5e`).
    TeamSpelling,
    /// An `ai.zrd` `TURRET` record's `NODES` entry bound this node — the
    /// `0x71d910` record `0x4bef90` reads unconditionally at its second
    /// step, carrying the record's `TEAM` int (the parse at `0x4aa5b7`
    /// stores it verbatim via `0x453740`; absent it, `0x4aa5ee` calls
    /// `0x45c260(0, …)`, whose `lea ecx, [eax+2]` makes the default read
    /// as `2`).
    TurretBinding {
        /// The `TURRET` record's position in the member — its turret type.
        turret_record: usize,
        /// The bound node's name inside the zeppelin's subtree.
        bound_node: String,
    },
    /// The resolver's terminal zero (`0x4bef90`'s last step): no `+0x8d`
    /// record gated in, no turret binding anywhere under the node — the
    /// original leaves the object at `0`, `neutral`.
    NoBinding,
}

/// What the measured allegiance path settled for one record (#1155) — the
/// team int the original stores at `zep+0xe0`, or why none can be claimed.
#[derive(Clone, Debug, PartialEq)]
pub enum AllegianceBinding {
    /// The team int the original leaves on the spawned object.
    Measured {
        /// The team int: `0` neutral, `1` ally, `2` enemy under the loader's
        /// `0x62b6c4` vocabulary. An int outside `0..=2` is inside the
        /// engine's write but outside the measured vocabulary, so
        /// `declare_actor` keeps the faction open rather than guess one.
        team: i64,
        /// Which measured source produced it.
        source: AllegianceSource,
    },
    /// The measured path cannot settle an allegiance here — the reason is
    /// verbatim and no faction is guessed.
    Open(String),
}

/// How one scope-member read went, for the allegiance carriers.
enum MemberRead {
    /// The member decoded through [`decode_zrd`]; its archive span is
    /// carried for provenance.
    Decoded(ZrdValue, SourceSpan),
    /// No searched archive holds the member — a measured absence: the
    /// original's loader finds nothing and the table it feeds stays empty.
    Absent,
    /// An archive was unreadable or the member refused to decode; verbatim.
    Refused(String),
}

/// Reads the first `member` found across the scope's archives in the
/// original's mount order — mission, group, shared.
fn read_scope_member(
    install_root: &Path,
    found: &Discovery,
    archives: &[String],
    member: &str,
) -> MemberRead {
    let fingerprint = cs_assets::install::fingerprint(&found.manifest);
    let mut unreadable: Option<String> = None;
    for archive in archives {
        let Some(record) = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == *archive)
        else {
            unreadable = unreadable.or(Some(format!("{archive} is not in the manifest")));
            continue;
        };
        let bytes = match read_container_bytes(install_root, record) {
            Ok(bytes) => bytes,
            Err(reason) => {
                unreadable = unreadable.or(Some(reason));
                continue;
            }
        };
        let discovery = discover_container(archive, &record.relative_spelling, &bytes);
        let Some(program) = discovery.programs().iter().find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(member))
        }) else {
            continue;
        };
        let decoded = match decode_zrd(program.bytes()) {
            Ok(decoded) => decoded,
            Err(error) => {
                return MemberRead::Refused(format!("{archive}'s {member} refuses: {error}"));
            }
        };
        let span = program.locator().span();
        match SourceSpan::new(
            fingerprint,
            archive,
            Some(member),
            span.offset,
            span.len,
            None,
        ) {
            Ok(source) => return MemberRead::Decoded(decoded, source),
            Err(error) => {
                return MemberRead::Refused(format!("{archive}'s {member} span refuses: {error}"));
            }
        }
    }
    match unreadable {
        // A carrier that could not be checked may hold the member, so the
        // read is a refusal, never an absence.
        Some(reason) => MemberRead::Refused(reason),
        None => MemberRead::Absent,
    }
}

/// How the `targets.zrd` gate carrier read (#1155): the elements a `+0x8d`
/// record can be staged by.
enum TargetsRead {
    /// `targets.zrd` decoded: the `nodes` element paths across every record,
    /// plus a count of elements outside the measured `text`/`[text, …]`
    /// shapes — which cannot be evaluated against a marked node.
    Decoded {
        /// Bottom-up paths: an element's last pattern against the node
        /// itself, the rest against its ancestors in order (`0x4a2400`).
        paths: Vec<Vec<String>>,
        /// Elements outside the measured shapes.
        unmodelled: usize,
    },
    /// No archive carries `targets.zrd`: nothing stages a `+0x8d` record —
    /// the gate is provably inert.
    Absent,
    /// The member refused: whether a `+0x8d` record would gate is open.
    Refused(String),
}

/// The allegiance inputs one scope's records resolve against (#1155): the
/// `ai.zrd` turret bindings applied to the world container's node array,
/// and the `targets.zrd` `nodes` gate for the `+0x8d` records `0x4bef90`
/// consults first.
struct AllegianceLookup<'a> {
    /// The container's nodes by stored index — the resolver's walk domain
    /// and the mount lookup's space (`0x4d11e0` iterates every node).
    nodes: HashMap<u32, &'a RawNode>,
    /// `0x71d910`: node index → the team its first turret binding carries —
    /// `0x4a9890` returns the list's first match, so the earliest bound
    /// record wins a shared node.
    turret: HashMap<u32, (i64, usize)>,
    /// How `ai.zrd` read — the binding table's carrier.
    ai_refused: Option<String>,
    /// How `targets.zrd` read — the `+0x8d` gate's carrier.
    targets: TargetsRead,
    /// The span `ai.zrd` decoded from, when it did — the provenance a
    /// resolver-bound faction files under.
    ai_span: Option<SourceSpan>,
    /// `NODES` elements outside the measured shapes: with any present the
    /// binding table is incomplete and no resolution over it is claimed.
    unmodelled: Vec<String>,
}

impl AllegianceLookup<'_> {
    /// `0x4bf080` → `0x4bef90` for one joined node: a gated `+0x8d` record,
    /// then a `0x71d910` turret binding, then the children depth-first —
    /// first nonzero wins, else `0`.
    fn resolve(&self, start: &RawNode) -> Result<(i64, Option<AllegianceSource>), String> {
        if let Some(reason) = &self.ai_refused {
            return Err(format!(
                "{TURRET_MEMBER} cannot supply the turret binding table ({reason}), so whether a \
                 record binds `{}`'s subtree is unmeasurable and no allegiance is claimed",
                start.name
            ));
        }
        if let Some(element) = self.unmodelled.first() {
            return Err(format!(
                "{TURRET_MEMBER} carries a `NODES` element outside the measured shapes \
                 ({element}): the binding table is incomplete and no allegiance over it is \
                 claimed"
            ));
        }
        self.visit(start)
    }

    /// One node of the resolver walk: the `+0x8d` check, the unconditional
    /// turret lookup, then the children — `0x4bef90`'s own order.
    fn visit(&self, node: &RawNode) -> Result<(i64, Option<AllegianceSource>), String> {
        // Step 1: a `+0x8d` record exists only for a marked node a
        // `targets.zrd` element matches — and when one does, its team lane is
        // read off a caller-stack slot `0x4a2e00` never writes (the shift at
        // `0x4a2e45` sources `[esp+0x1c]` unset), so the outcome is not
        // measurable and nothing downstream is claimed.
        if node.info.unk040 & ALLEGIANCE_MARKED != 0 {
            match &self.targets {
                TargetsRead::Decoded { paths, unmodelled } => {
                    for path in paths {
                        if self.path_matches(path, node) {
                            return Err(format!(
                                "`{}` is allegiance-marked (unk040 {:#010x}) and a `nodes` \
                                 element of {TARGETS_MEMBER} matches it, so `0x4a2e00` stages a \
                                 `+0x8d` record for it — whose team lane reads an uninitialized \
                                 caller-stack slot and is not measurable",
                                node.name, node.info.unk040,
                            ));
                        }
                        if path
                            .last()
                            .is_some_and(|leaf| wildcard_name(leaf, &node.name))
                        {
                            return Err(format!(
                                "`{}` is allegiance-marked (unk040 {:#010x}) and a `nodes` \
                                 element of {TARGETS_MEMBER} names it at the leaf without its \
                                 ancestor chain matching — whether `0x4a2400`'s walk would gate \
                                 it is not measurable",
                                node.name, node.info.unk040,
                            ));
                        }
                    }
                    if *unmodelled > 0 {
                        return Err(format!(
                            "`{}` is allegiance-marked (unk040 {:#010x}) and {TARGETS_MEMBER} \
                             carries {unmodelled} `nodes` elements outside the measured shapes, \
                             so whether one gates it is unmeasurable",
                            node.name, node.info.unk040,
                        ));
                    }
                }
                TargetsRead::Refused(reason) => {
                    return Err(format!(
                        "`{}` is allegiance-marked (unk040 {:#010x}) and {TARGETS_MEMBER} \
                         cannot be evaluated ({reason}), so whether a `+0x8d` record gates it \
                         is unmeasurable",
                        node.name, node.info.unk040,
                    ));
                }
                TargetsRead::Absent => {}
            }
        }
        // Step 2: a `0x71d910` turret binding carries its team verbatim —
        // even `0`, which this node still reports to its caller.
        if let Some(&(team, turret_record)) = self.turret.get(&node.index) {
            return Ok((
                team,
                Some(AllegianceSource::TurretBinding {
                    turret_record,
                    bound_node: node.name.clone(),
                }),
            ));
        }
        // Step 3: the children, depth-first in stored order — the first
        // nonzero answer wins.
        for &child in &node.children {
            let Some(node) = self.nodes.get(&child) else {
                return Err(format!(
                    "child index {child} of `{}` is outside the stored node array",
                    node.name
                ));
            };
            let (team, source) = self.visit(node)?;
            if team != 0 {
                return Ok((team, source));
            }
        }
        // Step 4: nothing bound under this node — the resolver's terminal
        // zero.
        Ok((0, None))
    }

    /// Whether one `targets.zrd` `nodes` element matches a node — the
    /// `0x4a2400` bottom-up chain: the element's last pattern against the
    /// node, the one before against its parent, and so on upward, each under
    /// `0x4abea0`'s wildcard rule.
    fn path_matches(&self, path: &[String], node: &RawNode) -> bool {
        let mut current = Some(node);
        for pattern in path.iter().rev() {
            let Some(node) = current else {
                return false;
            };
            if !wildcard_name(pattern, &node.name) {
                return false;
            }
            current = node
                .parent
                .and_then(|parent| self.nodes.get(&parent))
                .copied();
        }
        true
    }
}

/// Builds the scope's allegiance lookup (#1155): applies `ai.zrd`'s `TURRET`
/// records to the container's nodes the way `0x4ac170` → `0x4a9df0` does,
/// and collects `targets.zrd`'s `nodes` gate elements.
fn build_allegiance_lookup<'a>(
    install_root: &Path,
    found: &Discovery,
    archives: &[String],
    nodes: &'a GameZNodes,
) -> AllegianceLookup<'a> {
    let by_index: HashMap<u32, &RawNode> =
        nodes.nodes.iter().map(|node| (node.index, node)).collect();
    let mut lookup = AllegianceLookup {
        nodes: by_index,
        turret: HashMap::new(),
        ai_refused: None,
        targets: TargetsRead::Absent,
        ai_span: None,
        unmodelled: Vec::new(),
    };
    match read_scope_member(install_root, found, archives, TURRET_MEMBER) {
        MemberRead::Decoded(root, span) => {
            apply_turret_records(&root, &mut lookup);
            lookup.ai_span = Some(span);
        }
        MemberRead::Absent => {}
        MemberRead::Refused(reason) => lookup.ai_refused = Some(reason),
    }
    match read_scope_member(install_root, found, archives, TARGETS_MEMBER) {
        MemberRead::Decoded(root, _) => {
            // Every record's `nodes` list contributes its elements; the
            // record bodies are `[key, value]` pairs, which `zrd_field`
            // reads.
            let mut paths = Vec::new();
            let mut unmodelled = 0;
            for record in root.as_list().unwrap_or(&[]) {
                let Some(nodes) = zrd_field(record, "nodes").and_then(ZrdValue::as_list) else {
                    continue;
                };
                for element in nodes {
                    match nodes_path(element) {
                        Some(path) => paths.push(path),
                        None => unmodelled += 1,
                    }
                }
            }
            lookup.targets = TargetsRead::Decoded { paths, unmodelled };
        }
        MemberRead::Absent => {}
        MemberRead::Refused(reason) => lookup.targets = TargetsRead::Refused(reason),
    }
    lookup
}

/// One `nodes`/`NODES` element as a path, when its shape is measured: a bare
/// text is the one-element path that binds the node itself, and a list of
/// texts is the ancestor-to-descendant chain — anything else is outside the
/// measured shapes.
fn nodes_path(element: &ZrdValue) -> Option<Vec<String>> {
    if let Some(text) = element.as_text() {
        return Some(vec![text.to_owned()]);
    }
    let children = element.as_list()?;
    let path: Option<Vec<String>> = children
        .iter()
        .map(|child| child.as_text().map(str::to_owned))
        .collect();
    path.filter(|path| !path.is_empty())
}

/// Applies the decoded `ai.zrd` `TURRET` table to the node array — the
/// `0x4ac170` → `0x4a9df0` side of the loader. Each `NODES` entry is a
/// chain `[mount, part…]`: the mount pattern is matched over every node
/// (`0x4d11e0` iterates the container), and each matched mount's subtree is
/// walked for the part chain (`0x4abee0`/`0x4ac0c4`). The node the chain
/// ends on carries the record's `TEAM` int — absent `TEAM` reads the
/// loader's `0x45c260` default of `2` — and the earliest record to bind a
/// node wins it.
fn apply_turret_records(root: &ZrdValue, lookup: &mut AllegianceLookup<'_>) {
    let Some(records) = zrd_field(root, "TURRET").and_then(ZrdValue::as_list) else {
        return;
    };
    for (turret_record, record) in records.iter().enumerate() {
        let fields = zrd_flat_fields(record);
        // `TEAM` is stored verbatim (`0x4aa5b7` → `0x453740`); absent it
        // the parse's `0x4aa5ee` → `0x45c260` writes `0 + 2`. Any other
        // shape leaves the record unmodelled.
        let team = match fields.iter().find(|(key, _)| *key == "TEAM") {
            None => 2,
            Some((_, value)) => match team_int(value) {
                Some(team) => team,
                None => {
                    lookup.unmodelled.push(format!(
                        "TURRET record {turret_record}'s TEAM is not an int"
                    ));
                    continue;
                }
            },
        };
        let Some(nodes) = fields
            .iter()
            .find(|(key, _)| *key == "NODES")
            .and_then(|(_, value)| value.as_list())
        else {
            // `0x4ac264` asserts "Turret entry #%d specifies no NODES" and
            // skips the record — the measured skip.
            continue;
        };
        for element in nodes {
            let Some(path) = nodes_path(element) else {
                lookup.unmodelled.push(format!(
                    "TURRET record {turret_record} carries a non-text `NODES` element"
                ));
                continue;
            };
            let (mount, parts) = path.split_first().expect("a nonempty path");
            let mounts: Vec<u32> = lookup
                .nodes
                .values()
                .filter(|node| wildcard_name(mount, &node.name))
                .map(|node| node.index)
                .collect();
            for index in mounts {
                lookup.bind_chain(index, parts, team, turret_record);
            }
        }
    }
}

/// A `TEAM` value's int: stored bare or wrapped in a one-element list — the
/// shapes the corpus states — and nothing else.
fn team_int(value: &ZrdValue) -> Option<i64> {
    match value {
        ZrdValue::Int(team) => Some(i64::from(*team)),
        ZrdValue::List(children) if children.len() == 1 => children[0].as_int().map(i64::from),
        _ => None,
    }
}

impl AllegianceLookup<'_> {
    /// `0x4abee0`/`0x4ac0c4`'s part-chain walk: under `node`, every
    /// descendant matching the next pattern advances the chain; the node the
    /// chain empties on is bound — so a one-element entry binds its mount.
    fn bind_chain(&mut self, index: u32, parts: &[String], team: i64, turret_record: usize) {
        let Some((pattern, rest)) = parts.split_first() else {
            // `0x4a9890` returns the list's first match, so the earliest
            // record to reach a node keeps it.
            self.turret.entry(index).or_insert((team, turret_record));
            return;
        };
        let node = self.nodes[&index];
        for &child in &node.children {
            let next = if wildcard_name(pattern, &self.nodes[&child].name) {
                rest
            } else {
                parts
            };
            self.bind_chain(child, next, team, turret_record);
        }
    }
}

/// What the measured allegiance path settles for one record (#1155): a
/// recognized `team` spelling settles it without consulting the resolver
/// (`enemy`→2, `ally`→1, `neutral`→0 — `0x4bd987`..`0x4bda5e`), an
/// unrecognized spelling keeps the resolved value (`0x4bda1d`), and the
/// resolver itself is `0x4bef90`'s walk over the joined node's subtree.
fn allegiance_for(
    record: &cs_formats::zbd::zeppelins::ZeppelinRecord,
    scene: Option<&GameZNodes>,
    lookup: Option<&AllegianceLookup<'_>>,
) -> AllegianceBinding {
    if let Some(team) = record.team().and_then(|spelling| match spelling {
        "ally" => Some(1),
        "enemy" => Some(2),
        "neutral" => Some(0),
        _ => None,
    }) {
        return AllegianceBinding::Measured {
            team,
            source: AllegianceSource::TeamSpelling,
        };
    }
    let (Some(nodes), Some(lookup)) = (scene, lookup) else {
        return AllegianceBinding::Open(
            "the world container's node array could not be read, so `0x4bef90` has no subtree to \
             walk — no allegiance is claimed"
                .to_owned(),
        );
    };
    let matches: Vec<&RawNode> = nodes
        .nodes
        .iter()
        .filter(|node| node.name == record.node())
        .collect();
    let [node] = matches.as_slice() else {
        return AllegianceBinding::Open(format!(
            "`{}` names {} nodes of the world container — the resolver's walk has no single \
             subtree, so no allegiance is claimed",
            record.node(),
            matches.len()
        ));
    };
    match lookup.resolve(node) {
        Ok((team, source)) => AllegianceBinding::Measured {
            team,
            source: source.unwrap_or(AllegianceSource::NoBinding),
        },
        Err(reason) => AllegianceBinding::Open(reason),
    }
}

/// The claim the subject join is reported under: a record's `node` name
/// resolved through the world container's canonical scene graph.
pub const NODE_JOIN_CLAIM: &str = "f34-world.zeppelin-node-join";

/// The claim the session cadence binds under: a designed contract value —
/// the launch host's stepping rate, not an original measurement.
pub const SESSION_RATE_CLAIM: &str = "f34-world.session-rate-designed";

/// The residue the `Held` motion shape deliberately does not claim.
pub const MOTION_RESIDUE: &str = "the carrier states a spawn pose and tuning floats \
    (max_speed/max_accel/accel_*), never a route polyline, keyframe schedule, velocity or carrier: \
    `Held` is the only DeclaredMotion shape the record's own fields can fill — the program asserts \
    no self-driven motion, and whether the original then drives the actor (targets, deactivated, \
    the undecoded mission program) is unmeasured and outside this program's authority";

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a claim id this module declares")
}

/// What became of one record's `node` name in the world container's
/// canonical scene graph.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeJoin {
    /// Exactly one stored node carries the name; its stable scene-node id is
    /// the actor's subject.
    Single(ContentId),
    /// The container holds no node of that name.
    Absent,
    /// More than one stored node carries the name; which one the record
    /// binds is unmeasured, so no subject is picked.
    Ambiguous(usize),
}

/// The attitude source the original's own write order leaves on the node
/// last (#792).
///
/// Both carriers write one node's `Object3d` rotation triple through the
/// same setter, `0x4d1a30` (`class+0x18` = `r0`, `class+0x1c` = `r1`,
/// `class+0x20` = `r2`):
///
/// * the spawn writes it while the record loads (`0x4becf8` → `0x4bf930` →
///   `0x4bf950`), from the object's `obj+0x2c`/`obj+0x30`;
/// * `placezeps.zrd`'s `OBJECT_ROTATE_STATE` writes it from the animation
///   instance's first tick (`0x4e8cf5` → `0x4d1a30`, dispatched at
///   `0x4ecc7f`), which runs from the callback the start registers at
///   `0x4eda92` — after the mission start has loaded the records.
///
/// Both halves are stored in radians, exactly as the original stores them.
#[derive(Clone, Debug, PartialEq)]
pub enum AttitudeSource {
    /// `placezeps.zrd`'s `OBJECT_ROTATE_STATE` for this `NAME`, in the
    /// stored order — the original's `r0`, `r1`, `r2` of the `Object3d`
    /// triple — and the later of the two writes.
    StartupPlacement {
        /// The three numbers the rotate parser leaves at event
        /// `+0x10`/`+0x14`/`+0x18` (radians, #791).
        rotation_radians: [f32; 3],
    },
    /// The `zeppelins.zrd` record's own `yaw`/`pitch`, converted and
    /// clamped exactly as the load converts and clamps them
    /// (`0x4bda9b`/`0x4bdab2`, clamp `0x4bdbc6`). This is what the original
    /// applies when the startup carrier states no rotation for the node.
    CarrierSpawn {
        /// The stored `yaw`, degrees→radians (`obj+0x2c`).
        yaw_radians: f32,
        /// The stored `pitch`, degrees→radians (`obj+0x30`) and clamped to
        /// the record's `min_pitch`/`max_pitch`.
        pitch_radians: f32,
    },
}

impl AttitudeSource {
    /// The unit quaternion `[x, y, z, w]` this source leaves on the node,
    /// composed by [`object3d_orientation`].
    #[must_use]
    pub fn orientation(&self) -> [f64; 4] {
        match *self {
            Self::StartupPlacement { rotation_radians } => {
                let [r0, r1, r2] = rotation_radians.map(f64::from);
                object3d_orientation(r0, r1, r2)
            }
            Self::CarrierSpawn {
                yaw_radians,
                pitch_radians,
            } => object3d_orientation(f64::from(pitch_radians), f64::from(yaw_radians), 0.0),
        }
    }

    /// The claim the composed attitude binds under, and the byte span the
    /// value comes from: the startup carrier's `STATE` list for a startup
    /// placement, the record's own member for a spawn attitude.
    #[must_use]
    pub const fn claim(&self) -> &'static str {
        SPAWN_ATTITUDE_CLAIM
    }
}

/// What settles a record's attitude, or why it stays open (#792).
#[derive(Clone, Debug, PartialEq)]
pub enum AttitudeBinding {
    /// The measured source the original applies last.
    Measured(AttitudeSource),
    /// The startup carrier could not be read, so which source applies last
    /// cannot be settled for this scope: the reason is verbatim and no
    /// orientation is bound.
    Open(String),
}

/// The position source the original's own write order leaves on the node
/// last (#814).
///
/// Both carriers write one node's `Object3d` position triple through the
/// same setter, `0x4d1d50` (`class+0x54` = x, `class+0x58` = y,
/// `class+0x5c` = z):
///
/// * the spawn writes it while the record loads (`0x4becf8` → `0x4bf930` →
///   `0x4bf9b0`), from the object's `obj+0x20`..`obj+0x28`;
/// * `placezeps.zrd`'s `OBJECT_TRANSLATE_STATE` writes it from the
///   animation instance's first tick (`0x4e8de0` → `0x4d1d50`, dispatched
///   at `0x4ecc7f`), which runs after the mission start has loaded the
///   records.
///
/// Both halves are metres in the stored order — the translate executor
/// feeds the event's `+0x10`/`+0x14`/`+0x18` straight to the setter's x,
/// y, z with no axis swap (#791).
#[derive(Clone, Debug, PartialEq)]
pub enum PositionSource {
    /// `placezeps.zrd`'s `OBJECT_TRANSLATE_STATE` for this `NAME` — the
    /// later of the two writes. Only a statement carrying no keys the
    /// grammar leaves unmodelled reaches this variant: `RELATIVE` and
    /// `AT_NODE` change what `0x4e8de0` applies, so a flagged statement's
    /// bare triple is not the applied position.
    StartupPlacement {
        /// The three numbers the translate executor hands `0x4d1d50`, in
        /// stored order — the absolute triple, since the statement
        /// states no `RELATIVE`/`AT_NODE`.
        position_m: [f32; 3],
        /// The statement's `STATE` list, spanned inside the reader
        /// archive: the provenance the bound value is filed under.
        state_span: SourceSpan,
    },
    /// The `zeppelins.zrd` record's own `position` — what the original
    /// applies when the startup carrier states no translate for the node.
    CarrierSpawn {
        /// The stored `position` triple (`obj+0x20`..`obj+0x28`), metres.
        position_m: [f32; 3],
    },
}

/// What settles a record's position, or why it stays open (#814).
#[derive(Clone, Debug, PartialEq)]
pub enum PositionBinding {
    /// The measured source the original applies last.
    Measured(PositionSource),
    /// The ordering or the applied value could not be settled for this
    /// scope: the reason is verbatim and no position is bound.
    Open(String),
}

/// The measured compose of an `Object3d` rotation triple into a world
/// orientation (#770 §12.2): `M = Ry(r1)·Rx(r0)·Rz(r2)` over the right-handed
/// matrices, the build the image performs at `0x53bf40` and inverts at
/// `0x53df30` (`pitch = asin(−M7)`, `yaw = atan2(M6, M8)`,
/// `roll = atan2(M1, M4)`), with `0x49f8ec`'s compass counter-rotation fixing
/// the sign. `r0`/`r1`/`r2` are radians in the node's own slots
/// (`class+0x18`/`+0x1c`/`+0x20`); the result is the unit quaternion
/// `[x, y, z, w]` the world-actor runtime consumes, in #436's identity-mapped
/// metre frame (`yaw = 0`, `pitch = 0` ⇒ identity).
#[must_use]
pub fn object3d_orientation(r0: f64, r1: f64, r2: f64) -> [f64; 4] {
    let axis = |angle: f64, axis: [f64; 4]| {
        let (sin, cos) = (angle * 0.5).sin_cos();
        [axis[0] * sin, axis[1] * sin, axis[2] * sin, cos]
    };
    // `M = Ry(r1)·Rx(r0)·Rz(r2)` is the quaternion product `qy ⊗ qx ⊗ qz`.
    Quat(axis(r1, [0.0, 1.0, 0.0, 0.0]))
        .compose(Quat(axis(r0, [1.0, 0.0, 0.0, 0.0])))
        .compose(Quat(axis(r2, [0.0, 0.0, 1.0, 0.0])))
        .0
}

/// One `zeppelins.zrd` record's declared outcome: the stored pose, the
/// subject join, and the declared actor when the join produced a subject.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnedZeppelinActor {
    /// The record's position in the member, header aside.
    pub index: usize,
    /// The `node` spelling: the world node the record binds.
    pub node: String,
    /// The stored position exactly as the record states it, in the
    /// original's stored units (metres, #436).
    pub stored_position: [f32; 3],
    /// The stored `yaw` degrees.
    pub stored_yaw: f32,
    /// The stored `pitch` degrees.
    pub stored_pitch: f32,
    /// The source the original applies last for this node's attitude
    /// (#792), or why it stays open.
    pub attitude: AttitudeBinding,
    /// The source the original applies last for this node's position
    /// (#814), or why it stays open.
    pub position: PositionBinding,
    /// The `team` spelling, verbatim, when the record states one — a
    /// recognized spelling is the loader's override source
    /// ([`AllegianceSource::TeamSpelling`]), an unrecognized one keeps the
    /// resolver's value.
    pub team: Option<String>,
    /// What the measured allegiance path settled for the record's node
    /// (#1155): the team int the original stores at `zep+0xe0`, or why none
    /// is claimed.
    pub allegiance: AllegianceBinding,
    /// The `deactivated` integer, verbatim, when the record states one —
    /// its meaning is `KeyMeaning::Unknown`.
    pub deactivated: Option<u32>,
    /// The `node` name joined to the world container's scene graph.
    pub subject: NodeJoin,
    /// The declared actor, when the join produced a subject. A record whose
    /// `node` names no single world node is reported, never defaulted onto
    /// an invented identity.
    pub declared: Option<ProgramActor>,
}

impl SpawnedZeppelinActor {
    /// The attitude source the original applies last, when the ordering is
    /// settled for this record.
    #[must_use]
    pub fn attitude_source(&self) -> Option<&AttitudeSource> {
        match &self.attitude {
            AttitudeBinding::Measured(source) => Some(source),
            AttitudeBinding::Open(_) => None,
        }
    }

    /// The source this record's attitude *overwrites*, named as the residue
    /// (#792) so the losing carrier is never silently dropped: the record's
    /// own spawn attitude when a startup rotation applies last, the startup
    /// placement's silence (and its translate, which does apply) when the
    /// spawn attitude is the last rotation, and the open question verbatim
    /// when the ordering could not be settled.
    #[must_use]
    pub fn attitude_residue(&self) -> String {
        match &self.attitude {
            AttitudeBinding::Measured(AttitudeSource::StartupPlacement { .. }) => format!(
                "the record's own spawn attitude (yaw {}°, pitch {}° of {ZEPPELIN_MEMBER}) is the \
                 residue: it is written to the same Object3d rotation slots at the spawn \
                 (0x4becf8 → 0x4bf930 → 0x4bf950 → 0x4d1a30) and overwritten by the startup \
                 state on the animation instance's first tick — the carrier's value stays the \
                 zeppelin object's own attitude (obj+0x2c/obj+0x30)",
                self.stored_yaw, self.stored_pitch,
            ),
            AttitudeBinding::Measured(AttitudeSource::CarrierSpawn { .. }) => format!(
                "{PLACEZEPS_MEMBER} states no OBJECT_ROTATE_STATE for `{}` (#791), so no startup \
                 rotation overwrites the spawn attitude and the carrier's value is what the node \
                 keeps; the same node's startup OBJECT_TRANSLATE_STATE does apply, to the node's \
                 position, after this spawn write",
                self.node
            ),
            AttitudeBinding::Open(reason) => reason.clone(),
        }
    }

    /// The position source the original applies last, when the ordering is
    /// settled for this record.
    #[must_use]
    pub fn position_source(&self) -> Option<&PositionSource> {
        match &self.position {
            PositionBinding::Measured(source) => Some(source),
            PositionBinding::Open(_) => None,
        }
    }

    /// The source this record's position *overwrites*, named as the
    /// residue (#814) so the losing carrier is never silently dropped: the
    /// record's own spawn position when a startup translate applies last,
    /// the startup carrier's silence when the spawn position is the last
    /// write, and the open question verbatim when it could not be settled.
    #[must_use]
    pub fn position_residue(&self) -> String {
        match &self.position {
            PositionBinding::Measured(PositionSource::StartupPlacement { .. }) => format!(
                "the record's own spawn position ({:?} of {ZEPPELIN_MEMBER}) is the residue: it \
                 is written to the same Object3d position slots at the spawn (0x4becf8 → \
                 0x4bf930 → 0x4bf9b0 → 0x4d1d50, class+0x54..+0x5c) and overwritten by the \
                 startup translate on the animation instance's first tick — the carrier's value \
                 stays the zeppelin object's own position (obj+0x20..obj+0x28), handed back to \
                 the node only if the object's drive byte is set again",
                self.stored_position,
            ),
            PositionBinding::Measured(PositionSource::CarrierSpawn { .. }) => format!(
                "{PLACEZEPS_MEMBER} states no OBJECT_TRANSLATE_STATE for `{}` (#791), so no \
                 startup translate overwrites the spawn position and the carrier's value is what \
                 the node keeps",
                self.node
            ),
            PositionBinding::Open(reason) => reason.clone(),
        }
    }
}

/// How the carrier member read went.
#[derive(Clone, Debug, PartialEq)]
pub enum CarrierRead {
    /// The member decoded; this many records.
    Decoded(usize),
    /// The scope's reader archive holds no `zeppelins.zrd` member.
    Absent,
    /// The archive or member could not be read; the reason is verbatim.
    Unreadable(String),
    /// [`read_zeppelins_member`] refused the bytes; the error is verbatim.
    Refused(String),
}

/// The `ACTIVATION` spelling the original starts at mission start:
/// `0x51e390` stores `4` for the keyword `ON_STARTUP` into a definition's
/// activation byte `+0xa1`, and `0x522fd0` starts every definition whose
/// byte is `4` (`0x52309b`, #792 §2). No other spelling is measured into
/// the startup ordering.
const ON_STARTUP_ACTIVATION: &str = "ON_STARTUP";

/// One `placezeps.zrd` state statement the startup ordering measures over
/// (#792's rotations, #814's translates): the `NAME` it addresses, the
/// `ACTIVATION` spelling of the definition carrying it, the numbers the
/// executor hands the `Object3d` setter, the `STATE` list's own byte span
/// for a bound value's provenance, and the statement keys the placement
/// grammar leaves unmodelled. Only an `ON_STARTUP` definition's statement
/// is a measured startup write — a statement under any other activation
/// settles no pose — and a flagged statement does not apply its bare
/// triple either.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupStatement {
    /// Which state statement this is.
    pub kind: StateKind,
    /// The `NAME` the statement addresses.
    pub node: String,
    /// The `ACTIVATION` spelling of the definition carrying the statement,
    /// verbatim.
    pub activation: String,
    /// The three numbers as the executable stores them in the event —
    /// radians for a rotate, the triple as parsed for a translate (#791).
    pub values: [f32; 3],
    /// The statement's `STATE` list, spanned inside the reader archive.
    pub state_span: SourceSpan,
    /// Keys of the statement outside `NAME` and `STATE`, verbatim.
    pub unmodelled: Vec<String>,
}

/// How the scope's startup placements read, for the pose ordering (#792's
/// attitude half, #814's position half): `placezeps.zrd` is the carrier
/// whose `ON_STARTUP` states the original applies to the node *after* the
/// spawn write.
#[derive(Clone, Debug)]
pub enum StartupRead {
    /// The member decoded: every `OBJECT_ROTATE_STATE` and
    /// `OBJECT_TRANSLATE_STATE` it states, in stored order, each with its
    /// definition's `ACTIVATION` spelling — only `ON_STARTUP` writes are
    /// the startup ordering this enum measures.
    Decoded(Vec<StartupStatement>),
    /// The reader archive holds no `placezeps.zrd` member: this scope
    /// starts no placements, so nothing overwrites the spawn pose.
    Absent,
    /// The member refused to decode; the ordering stays open verbatim
    /// ([`ATTITUDE_PRECEDENCE_UNKNOWN_CLAIM`],
    /// [`POSITION_PRECEDENCE_UNKNOWN_CLAIM`]).
    Refused(String),
}

/// How the world-container read for the subject join went.
#[derive(Clone, Debug)]
pub enum SceneJoin {
    /// `{group_dir}/gamez.zbd` converted to a canonical scene graph.
    Ready,
    /// The container read refused; the reason is verbatim.
    Refused(String),
}

/// One still-open field of a declared actor or of the program: the
/// production path's own "what is still open" list.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenField {
    /// The actor the field belongs to, or `None` for a program-level field.
    pub actor: Option<ProgramActor>,
    /// The declared field name.
    pub field: &'static str,
    /// The claim the unknown is filed under.
    pub claim_id: ClaimId,
    /// Why the field is unknown, verbatim from the declaration.
    pub reason: String,
}

/// The outcome of binding a scope's `zeppelins.zrd` carrier into a declared
/// world-actor program and running it through the production lowering and
/// session — the path the `world_actors` launch surface judges.
#[derive(Debug)]
pub struct MissionWorldActors {
    /// The reader archive the carrier lives in
    /// (`zbd/<group>/<mission>/zrdr.zbd`).
    archive: String,
    /// How the carrier read went.
    carrier: CarrierRead,
    /// How the world-container read for the subject join went.
    scene: SceneJoin,
    /// One row per decoded record, in member order.
    rows: Vec<SpawnedZeppelinActor>,
    /// The declared program, when at least one record joined a subject.
    program: Option<DeclaredWorldActorProgram>,
    /// Every `Resolved::Unknown` the declared program carries — the exact
    /// list of what is still open, in declaration order. This is wider than
    /// [`Self::lower_error`], which stops at the first refusal.
    open_fields: Vec<OpenField>,
    /// [`lower_world_actors`]'s verbatim verdict, when a program was
    /// declared.
    lower_error: Option<WorldActorLowerError>,
    /// The lowered program, when the lowering accepted it.
    lowered: Option<LoweredWorldActors>,
    /// The launched session — `Some` only when the program lowered and
    /// [`WorldActorSession::launch`] accepted it.
    session: Option<WorldActorSession>,
    /// The launch refusal, verbatim, when lowering passed but registration
    /// did not.
    launch_error: Option<WorldActorLaunchError>,
}

impl MissionWorldActors {
    /// The reader archive the carrier was read from.
    #[must_use]
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// How the carrier read went.
    #[must_use]
    pub const fn carrier(&self) -> &CarrierRead {
        &self.carrier
    }

    /// How the world-container read for the subject join went.
    #[must_use]
    pub const fn scene(&self) -> &SceneJoin {
        &self.scene
    }

    /// One row per decoded record, in member order.
    #[must_use]
    pub fn rows(&self) -> &[SpawnedZeppelinActor] {
        &self.rows
    }

    /// The declared program, when one was assembled.
    #[must_use]
    pub const fn program(&self) -> Option<&DeclaredWorldActorProgram> {
        self.program.as_ref()
    }

    /// Every still-open field of the declared program, in declaration
    /// order: the launch surface's "what is still open" list.
    #[must_use]
    pub fn open_fields(&self) -> &[OpenField] {
        &self.open_fields
    }

    /// The production lowering's verbatim refusal, when one was raised.
    #[must_use]
    pub const fn lower_error(&self) -> Option<&WorldActorLowerError> {
        self.lower_error.as_ref()
    }

    /// The lowered program, when the lowering accepted it.
    #[must_use]
    pub const fn lowered(&self) -> Option<&LoweredWorldActors> {
        self.lowered.as_ref()
    }

    /// The launched session, when the program lowered and registered.
    #[must_use]
    pub const fn session(&self) -> Option<&WorldActorSession> {
        self.session.as_ref()
    }

    /// The launch refusal, when lowering passed but registration refused.
    #[must_use]
    pub const fn launch_error(&self) -> Option<&WorldActorLaunchError> {
        self.launch_error.as_ref()
    }

    /// Whether the declared program lowered and launched a session.
    #[must_use]
    pub const fn is_satisfied(&self) -> bool {
        self.session.is_some()
    }
}

/// Reads a scope's `zeppelins.zrd` carrier, declares every record whose
/// `node` joins a single world node into a [`DeclaredWorldActorProgram`],
/// then lowers and launches it through the production session path.
///
/// Nothing is invented: `position_m` and `orientation` bind measured
/// under [`SPAWN_POSE_CLAIM`] and [`SPAWN_ATTITUDE_CLAIM`] from the source
/// the original applies last, `faction` binds the measured allegiance
/// under [`ALLEGIANCE_RESOLVED_CLAIM`] or stays [`Resolved::Unknown`]
/// under [`ALLEGIANCE_OPEN_CLAIM`], and `open_fields` names every open
/// field regardless of which refusal the lowering hits first.
///
/// `session_ticks_per_second` is the host's designed session cadence —
/// [`SESSION_TICKS_PER_SECOND`] is what the launch surface passes.
#[must_use]
pub fn bind_mission_world_actors(
    install_root: &Path,
    found: &Discovery,
    mission_dir: &str,
    group_dir: &str,
    mission_subject: &ContentId,
    session_ticks_per_second: u32,
) -> MissionWorldActors {
    let archive = format!("{mission_dir}/zrdr.zbd");
    let (member, member_span) = match read_carrier_member(install_root, found, &archive) {
        Ok(read) => read,
        Err(carrier) => {
            return MissionWorldActors {
                archive,
                carrier,
                scene: SceneJoin::Refused(
                    "not reached: the carrier member was not read".to_owned(),
                ),
                rows: Vec::new(),
                program: None,
                open_fields: Vec::new(),
                lower_error: None,
                lowered: None,
                session: None,
                launch_error: None,
            };
        }
    };
    let carrier = CarrierRead::Decoded(member.records().len());

    // The startup half of the pose ordering (#792's attitude, #814's
    // position): the scope's own `placezeps.zrd` states the placements the
    // animation player applies to the same node after this spawn write. A
    // refusal keeps every record's pose open rather than picking a source.
    let (startup, startup_span) = match read_startup_member(install_root, found, &archive) {
        Ok(read) => read,
        Err(reason) => (StartupRead::Refused(reason), None),
    };

    // The subject join: the world container's canonical scene graph, the
    // same conversion the `world_geometry` surface runs. A refused graph
    // means no subject can be resolved — every record is reported with its
    // join outcome rather than placed on a guessed node. The raw node array
    // comes back alongside it for the allegiance resolver (#1155), which
    // reads `ai.zrd`'s turret bindings and `targets.zrd`'s gate through the
    // scope's archives in the original's mission/group/shared mount order.
    let scene = read_world_scene(install_root, found, group_dir);
    let archives = [
        archive.clone(),
        format!("{group_dir}/zrdr.zbd"),
        "zbd/zrdr.zbd".to_owned(),
    ];
    let allegiance = scene
        .as_ref()
        .ok()
        .map(|(_, nodes)| build_allegiance_lookup(install_root, found, &archives, nodes));
    let rows: Vec<SpawnedZeppelinActor> = member
        .records()
        .iter()
        .enumerate()
        .map(|(index, record)| {
            declare_row(
                index,
                record,
                scene.as_ref().ok(),
                &startup,
                allegiance.as_ref(),
            )
        })
        .collect();
    let allegiance_span = allegiance
        .as_ref()
        .and_then(|lookup| lookup.ai_span.clone());

    let program = assemble_program(
        mission_subject,
        &member_span,
        startup_span.as_ref(),
        allegiance_span.as_ref(),
        session_ticks_per_second,
        &rows,
    );
    let open_fields = program
        .as_ref()
        .map(collect_open_fields)
        .unwrap_or_default();

    let (lowered, lower_error, session, launch_error) = match &program {
        None => (None, None, None, None),
        Some(program) => match lower_world_actors(program) {
            Err(error) => (None, Some(error), None, None),
            Ok(lowered) => match WorldActorSession::launch(lowered.clone(), SessionGeneration(1)) {
                Ok(session) => (Some(lowered), None, Some(session), None),
                Err(error) => (Some(lowered), None, None, Some(error)),
            },
        },
    };

    MissionWorldActors {
        archive,
        carrier,
        scene: match scene {
            Ok(_) => SceneJoin::Ready,
            Err(reason) => SceneJoin::Refused(reason),
        },
        rows,
        program,
        open_fields,
        lower_error,
        lowered,
        session,
        launch_error,
    }
}

/// Reads the `zeppelins.zrd` member out of the scope's reader archive.
///
/// `Ok` is the decoded member and its source span inside the container;
/// `Err` is the [`CarrierRead`] state to report instead — an absent member
/// is `Absent`, not an error, because a scope with no carrier declares no
/// world actors.
fn read_carrier_member(
    install_root: &Path,
    found: &Discovery,
    archive: &str,
) -> Result<(cs_formats::zbd::zeppelins::ZeppelinMember, SourceSpan), CarrierRead> {
    let Some(record) = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == archive)
    else {
        return Err(CarrierRead::Unreadable(format!(
            "{archive} is not in the discovered manifest"
        )));
    };
    let bytes = read_container_bytes(install_root, record).map_err(CarrierRead::Unreadable)?;
    let discovery = discover_container(archive, &record.relative_spelling, &bytes);
    let Some(member) = discovery.programs().iter().find(|program| {
        program
            .locator()
            .member()
            .is_some_and(|name| name.eq_ignore_ascii_case(ZEPPELIN_MEMBER))
    }) else {
        return Err(CarrierRead::Absent);
    };
    let decoded = read_zeppelins_member(member.bytes())
        .map_err(|error| CarrierRead::Refused(error.to_string()))?;
    let span = member.locator().span();
    let source = SourceSpan::new(
        cs_assets::install::fingerprint(&found.manifest),
        archive,
        Some(ZEPPELIN_MEMBER),
        span.offset,
        span.len,
        None,
    )
    .map_err(|error| CarrierRead::Unreadable(format!("the member's span refuses: {error}")))?;
    Ok((decoded, source))
}

/// Reads the scope's `placezeps.zrd` member: the startup state statements
/// the mission states, and the member's own byte span for their provenance.
///
/// `Absent` is a *measured* absence — a scope whose startup carrier states
/// no placement leaves the spawn pose as the last writer. A refusal is
/// never read as an absence: it comes back as [`StartupRead::Refused`] so
/// every record's attitude and position stay open under their precedence
/// claims.
fn read_startup_member(
    install_root: &Path,
    found: &Discovery,
    archive: &str,
) -> Result<(StartupRead, Option<SourceSpan>), String> {
    let Some(record) = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == archive)
    else {
        return Err(format!("{archive} is not in the discovered manifest"));
    };
    let bytes = read_container_bytes(install_root, record)?;
    let discovery = discover_container(archive, &record.relative_spelling, &bytes);
    let Some(member) = discovery.programs().iter().find(|program| {
        program
            .locator()
            .member()
            .is_some_and(|name| name.eq_ignore_ascii_case(PLACEZEPS_MEMBER))
    }) else {
        return Ok((StartupRead::Absent, None));
    };
    let decoded = read_placezeps_member(member.bytes()).map_err(|error| error.to_string())?;
    let span = member.locator().span();
    let fingerprint = cs_assets::install::fingerprint(&found.manifest);
    let source = SourceSpan::new(
        fingerprint,
        archive,
        Some(PLACEZEPS_MEMBER),
        span.offset,
        span.len,
        None,
    )
    .map_err(|error| format!("the member's span refuses: {error}"))?;
    // Every state statement of the member, in stored order: the `NAME` it
    // addresses, the `ACTIVATION` spelling of the definition carrying it
    // (only `ON_STARTUP` is measured into the startup ordering — a statement
    // under any other spelling settles no pose), the numbers the parser
    // leaves in the event (radians for a rotate, the triple as stored for a
    // translate, #791) — which the executors hand to `0x4d1a30`/`0x4d1d50`
    // (#792, #814) — the `STATE` list's own archive span for a bound
    // value's provenance, and the keys the grammar leaves unmodelled:
    // `RELATIVE`/`AT_NODE` change what an executor applies, so a flagged
    // statement settles no pose.
    let statements = decoded
        .definitions()
        .iter()
        .flat_map(|definition| {
            let activation = definition.activation();
            let sequence = definition.sequence();
            [sequence.translate(), sequence.rotate()]
                .into_iter()
                .flatten()
                .map(move |statement| (activation, statement))
        })
        .map(|(activation, statement)| {
            let range = statement.state_range();
            StartupStatement {
                kind: statement.kind(),
                node: statement.node().to_owned(),
                activation: activation.to_owned(),
                values: statement.parsed(),
                state_span: SourceSpan::new(
                    fingerprint,
                    archive,
                    Some(PLACEZEPS_MEMBER),
                    span.offset + range.start,
                    range.end - range.start,
                    None,
                )
                .expect("a STATE range inside the member spans"),
                unmodelled: statement
                    .unknown_fields()
                    .iter()
                    .map(|field| field.key.clone())
                    .collect(),
            }
        })
        .collect();
    Ok((StartupRead::Decoded(statements), Some(source)))
}

/// Reads `{group_dir}/gamez.zbd` and converts it through the production
/// `world_scene_graph_from_gamez` path — the same adapter and empty binding
/// map `measure_geometry` uses, so the subject join is the launch surface's
/// own measurement. The raw node array comes back alongside it: the
/// allegiance resolver's walk domain (#1155), which the converted graph
/// does not carry.
fn read_world_scene(
    install_root: &Path,
    found: &Discovery,
    group_dir: &str,
) -> Result<(cs_content::scene::SceneGraph, GameZNodes), String> {
    let key = format!("{group_dir}/gamez.zbd");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
        .ok_or_else(|| format!("no {key} in the discovered installation"))?;
    let bytes = read_container_bytes(install_root, record)?;
    let mut context = ParseContext::with_defaults(&key);
    let nodes = read_gamez_nodes(&mut context, &bytes)
        .map_err(|error| format!("the node array refuses: {error}"))?;
    let container = ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.{}", key.replace('/', ".")),
    )
    .map_err(|error| format!("the container id cannot be spelled: {error}"))?;
    let adapter = SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source");
    let bindings = BindingMap::default();
    let graph = world_scene_graph_from_gamez(&container, &nodes, &[], &adapter, &bindings)
        .map(|scene| scene.graph().clone())
        .map_err(|error| format!("{error}"))?;
    Ok((graph, nodes))
}

fn read_container_bytes(
    install_root: &Path,
    record: &InstallFileRecord,
) -> Result<Vec<u8>, String> {
    let host = install_root.join(record.relative_spelling.as_str());
    std::fs::read(&host).map_err(|error| format!("cannot read {}: {error}", host.display()))
}

/// The record's own spawn attitude, converted and clamped exactly as the
/// load converts and clamps it: degrees→radians through
/// [`crate::mission_start::stored_heading_radians`] (the image's
/// `0x6040e8`, `0x4bda9b`/`0x4bdab2`), `pitch` then clamped to the record's
/// `min_pitch`/`max_pitch` the way `0x4bdbc6` clamps it before the spawn
/// hands the pair to `SetRotation` (`0x4bf950` clamps again on the way).
fn spawn_attitude(record: &cs_formats::zbd::zeppelins::ZeppelinRecord) -> AttitudeSource {
    use crate::mission_start::stored_heading_radians;
    let min = stored_heading_radians(record.min_pitch());
    let max = stored_heading_radians(record.max_pitch());
    let pitch = stored_heading_radians(record.pitch());
    AttitudeSource::CarrierSpawn {
        yaw_radians: stored_heading_radians(record.yaw()),
        // A record that states an inverted range leaves the pitch alone
        // rather than panicking; M01's three records state a sane one.
        pitch_radians: if min <= max {
            pitch.clamp(min, max)
        } else {
            pitch
        },
    }
}

/// The statements of `kind` writing `node`, split at the measured
/// boundary: the writes an `ON_STARTUP` definition makes — the only
/// activation `0x522fd0` starts at mission start (`+0xa1 == 4`,
/// [`ON_STARTUP_ACTIVATION`]) — and the writes under any other spelling,
/// which the startup ordering does not cover.
fn startup_writes<'a>(
    statements: &'a [StartupStatement],
    kind: StateKind,
    node: &str,
) -> (Vec<&'a StartupStatement>, Vec<&'a StartupStatement>) {
    statements
        .iter()
        .filter(|statement| statement.kind == kind && statement.node == node)
        .partition(|statement| statement.activation == ON_STARTUP_ACTIVATION)
}

/// The source the original applies last for a node's attitude (#792): the
/// startup carrier's `OBJECT_ROTATE_STATE` for the `node` when exactly one
/// `ON_STARTUP` definition states one *without* keys the grammar leaves
/// unmodelled — the later of the two writes to the same `Object3d`
/// rotation slots — and the record's own spawn attitude otherwise. A
/// refused startup carrier settles nothing; neither does a statement under
/// an activation outside `ON_STARTUP`, a flagged rotate — `AT_NODE`, the
/// second flag and `RELATIVE` each change what `0x4e8b80` applies — or
/// several startup rotations for one node, whose order among themselves is
/// not measured.
fn attitude_for(node: &str, spawn: AttitudeSource, startup: &StartupRead) -> AttitudeBinding {
    match startup {
        StartupRead::Refused(reason) => AttitudeBinding::Open(format!(
            "{ATTITUDE_PRECEDENCE_UNKNOWN_REASON_PREFIX}{reason}"
        )),
        StartupRead::Absent => AttitudeBinding::Measured(spawn),
        StartupRead::Decoded(statements) => {
            let (writes, deferred) = startup_writes(statements, StateKind::Rotate, node);
            if let [first, ..] = deferred.as_slice() {
                return AttitudeBinding::Open(format!(
                    "the startup carrier states an OBJECT_ROTATE_STATE for `{node}` under the \
                     activation `{}` — only `ON_STARTUP` definitions are measured to start at \
                     mission start (`+0xa1 == 4`, `0x522fd0`), so whether that write lands inside \
                     the startup ordering is open and no attitude is guessed",
                    first.activation
                ));
            }
            match writes.as_slice() {
                [] => AttitudeBinding::Measured(spawn),
                [statement] if statement.unmodelled.is_empty() => {
                    AttitudeBinding::Measured(AttitudeSource::StartupPlacement {
                        rotation_radians: statement.values,
                    })
                }
                [statement] => AttitudeBinding::Open(format!(
                    "the startup carrier states an OBJECT_ROTATE_STATE for `{node}`, but the \
                     statement carries keys the placement grammar leaves unmodelled ({}) — \
                     `AT_NODE`, the second flag and `RELATIVE` each change what `0x4e8b80` \
                     applies, so a flagged rotate's bare STATE triple is not the applied \
                     attitude and none is guessed",
                    statement.unmodelled.join(", ")
                )),
                many => AttitudeBinding::Open(format!(
                    "the startup carrier states {} OBJECT_ROTATE_STATEs for `{node}` — the \
                     measured order settles every spawn write before every startup write, not \
                     one startup write against another, so which lands last is open and no \
                     attitude is guessed",
                    many.len()
                )),
            }
        }
    }
}

/// The source the original applies last for a node's position (#814): the
/// startup carrier's `OBJECT_TRANSLATE_STATE` for the `node` when exactly
/// one `ON_STARTUP` definition states one *without* keys the grammar
/// leaves unmodelled — the later of the two writes to the same `Object3d`
/// position slots — and the record's own spawn position otherwise. A
/// refused startup carrier settles nothing; neither does a statement under
/// an activation outside `ON_STARTUP`, a flagged translate — `RELATIVE`
/// and `AT_NODE` change what `0x4e8de0` applies — or several startup
/// translates for one node, whose order among themselves is not measured.
fn position_for(node: &str, carrier_position: [f32; 3], startup: &StartupRead) -> PositionBinding {
    match startup {
        StartupRead::Refused(reason) => PositionBinding::Open(format!(
            "{POSITION_PRECEDENCE_UNKNOWN_REASON_PREFIX}{reason}"
        )),
        StartupRead::Absent => PositionBinding::Measured(PositionSource::CarrierSpawn {
            position_m: carrier_position,
        }),
        StartupRead::Decoded(statements) => {
            let (writes, deferred) = startup_writes(statements, StateKind::Translate, node);
            if let [first, ..] = deferred.as_slice() {
                return PositionBinding::Open(format!(
                    "the startup carrier states an OBJECT_TRANSLATE_STATE for `{node}` under the \
                     activation `{}` — only `ON_STARTUP` definitions are measured to start at \
                     mission start (`+0xa1 == 4`, `0x522fd0`), so whether that write lands inside \
                     the startup ordering is open and no position is guessed",
                    first.activation
                ));
            }
            match writes.as_slice() {
                [] => PositionBinding::Measured(PositionSource::CarrierSpawn {
                    position_m: carrier_position,
                }),
                [statement] if statement.unmodelled.is_empty() => {
                    PositionBinding::Measured(PositionSource::StartupPlacement {
                        position_m: statement.values,
                        state_span: statement.state_span.clone(),
                    })
                }
                [statement] => PositionBinding::Open(format!(
                    "the startup carrier states an OBJECT_TRANSLATE_STATE for `{node}`, but the \
                     statement carries keys the placement grammar leaves unmodelled ({}) — the \
                     write order over the node's position slots is measured (#792), the applied \
                     position of a flagged translate is not its bare STATE triple, and none is \
                     guessed",
                    statement.unmodelled.join(", ")
                )),
                many => PositionBinding::Open(format!(
                    "the startup carrier states {} OBJECT_TRANSLATE_STATEs for `{node}` — the \
                     measured order settles every spawn write before every startup write, not \
                     one startup write against another, so which lands last is open and no \
                     position is guessed",
                    many.len()
                )),
            }
        }
    }
}

/// One record's declared outcome: the stored pose kept verbatim, the `node`
/// name joined against the canonical scene graph, the measured allegiance
/// (#1155), and the [`DeclaredWorldActor`] when the join names a single
/// node.
fn declare_row(
    index: usize,
    record: &cs_formats::zbd::zeppelins::ZeppelinRecord,
    scene: Option<&(cs_content::scene::SceneGraph, GameZNodes)>,
    startup: &StartupRead,
    allegiance: Option<&AllegianceLookup<'_>>,
) -> SpawnedZeppelinActor {
    let subject = match scene {
        None => NodeJoin::Absent,
        Some((graph, _)) => {
            let matches: Vec<&cs_content::scene::SceneNode> = graph
                .nodes()
                .iter()
                .filter(|node| node.name() == record.node())
                .collect();
            match matches.as_slice() {
                [node] => NodeJoin::Single(node.id().as_content_id().clone()),
                [] => NodeJoin::Absent,
                _ => NodeJoin::Ambiguous(matches.len()),
            }
        }
    };
    let declared = matches!(&subject, NodeJoin::Single(_)).then(|| ProgramActor(index as u32));
    SpawnedZeppelinActor {
        index,
        node: record.node().to_owned(),
        stored_position: record.position(),
        stored_yaw: record.yaw(),
        stored_pitch: record.pitch(),
        attitude: attitude_for(record.node(), spawn_attitude(record), startup),
        position: position_for(record.node(), record.position(), startup),
        team: record.team().map(str::to_owned),
        allegiance: allegiance_for(record, scene.map(|(_, nodes)| nodes), allegiance),
        deactivated: record.deactivated(),
        subject,
        declared,
    }
}

/// Declares one actor per row whose subject joined, with the measured pose
/// bound and every unmeasured field an explicit unknown.
///
/// `startup_span` is the byte span of the scope's `placezeps.zrd` when it
/// read: a startup-sourced attitude is provenanced from *that* member and a
/// startup-sourced position from the statement's own `STATE` span it
/// carries, while either spawn-sourced half is provenanced from the
/// carrier's own `span` (#792, #814). `allegiance_span` is the scope's
/// `ai.zrd` span when it decoded — a resolver-bound faction is provenanced
/// from the turret table it resolved through; a `team`-keyed faction is
/// provenanced from the carrier's own record (#1155).
fn declare_actor(
    row: &SpawnedZeppelinActor,
    actor: ProgramActor,
    span: &SourceSpan,
    startup_span: Option<&SourceSpan>,
    allegiance_span: Option<&SourceSpan>,
) -> Option<DeclaredWorldActor> {
    let NodeJoin::Single(subject) = &row.subject else {
        return None;
    };
    let position_m = match &row.position {
        PositionBinding::Measured(source) => {
            let (value, source_span) = match source {
                PositionSource::StartupPlacement {
                    position_m,
                    state_span,
                } => (*position_m, state_span.clone()),
                PositionSource::CarrierSpawn { position_m } => (*position_m, span.clone()),
            };
            let provenance = Provenance::new(
                claim(SPAWN_POSE_CLAIM),
                cs_types::evidence::ClaimStatus::ObservedTool,
                Some(source_span),
            )
            .expect("observed provenance with a source span");
            Resolved::Known(Known::new(value.map(f64::from), provenance))
        }
        PositionBinding::Open(reason) => {
            Resolved::unknown(claim(POSITION_PRECEDENCE_UNKNOWN_CLAIM), reason)
                .expect("a reason is stated")
        }
    };
    let orientation = match &row.attitude {
        AttitudeBinding::Measured(source) => {
            let source_span = match source {
                AttitudeSource::StartupPlacement { .. } => {
                    startup_span.cloned().unwrap_or_else(|| span.clone())
                }
                AttitudeSource::CarrierSpawn { .. } => span.clone(),
            };
            let provenance = Provenance::new(
                claim(SPAWN_ATTITUDE_CLAIM),
                cs_types::evidence::ClaimStatus::ObservedTool,
                Some(source_span),
            )
            .expect("observed provenance with a source span");
            Resolved::Known(Known::new(source.orientation(), provenance))
        }
        AttitudeBinding::Open(reason) => {
            Resolved::unknown(claim(ATTITUDE_PRECEDENCE_UNKNOWN_CLAIM), reason)
                .expect("a reason is stated")
        }
    };
    let faction = match &row.allegiance {
        AllegianceBinding::Measured { team, source } => match team_key(*team) {
            Some(spelling) => {
                let source_span = match source {
                    AllegianceSource::TeamSpelling => span.clone(),
                    AllegianceSource::TurretBinding { .. } | AllegianceSource::NoBinding => {
                        allegiance_span.cloned().unwrap_or_else(|| span.clone())
                    }
                };
                let provenance = Provenance::new(
                    claim(ALLEGIANCE_RESOLVED_CLAIM),
                    cs_types::evidence::ClaimStatus::ObservedTool,
                    Some(source_span),
                )
                .expect("observed provenance with a source span");
                Resolved::Known(Known::new(
                    ContentId::from_source(ContentKind::Faction, spelling)
                        .expect("a measured vocabulary spelling is a valid content key"),
                    provenance,
                ))
            }
            // The int is inside the engine's write but outside the measured
            // `neutral`/`ally`/`enemy` vocabulary — no faction is guessed.
            None => {
                let reason = format!(
                    "the measured allegiance path resolved team {team} for `{}`, an int outside \
                     the `neutral`/`ally`/`enemy` vocabulary (`0x62b6c4`): no faction spelling \
                     is measured for it and none is invented",
                    row.node
                );
                Resolved::unknown(claim(ALLEGIANCE_OPEN_CLAIM), &reason)
                    .expect("a reason is stated")
            }
        },
        AllegianceBinding::Open(reason) => {
            Resolved::unknown(claim(ALLEGIANCE_OPEN_CLAIM), reason).expect("a reason is stated")
        }
    };
    Some(DeclaredWorldActor {
        actor,
        subject: subject.clone(),
        kind: DeclaredWorldActorKind::Airship,
        faction,
        objective: None,
        motion: DeclaredMotion::Held {
            position_m,
            orientation,
        },
        sockets: Vec::new(),
    })
}

/// Assembles the declared program out of the rows whose subjects joined.
///
/// The program's subject is the mission's catalog identity, its origin the
/// carrier member's own bytes, and its tick rate the host's designed
/// session cadence — never an original measurement. `None` when no record
/// declared.
fn assemble_program(
    mission_subject: &ContentId,
    member_span: &SourceSpan,
    startup_span: Option<&SourceSpan>,
    allegiance_span: Option<&SourceSpan>,
    session_ticks_per_second: u32,
    rows: &[SpawnedZeppelinActor],
) -> Option<DeclaredWorldActorProgram> {
    let actors: Vec<DeclaredWorldActor> = rows
        .iter()
        .filter_map(|row| {
            row.declared.and_then(|actor| {
                declare_actor(row, actor, member_span, startup_span, allegiance_span)
            })
        })
        .collect();
    if actors.is_empty() {
        return None;
    }
    DeclaredWorldActorProgram::try_new(
        mission_subject.clone(),
        Origin::Installation {
            source: member_span.clone(),
        },
        Provenance::new(
            claim(SPAWN_POSE_CLAIM),
            cs_types::evidence::ClaimStatus::ObservedTool,
            Some(member_span.clone()),
        )
        .expect("observed provenance with a source span"),
        DeclaredWorldActorParts {
            ticks_per_second: Resolved::Known(Known::new(
                session_ticks_per_second,
                Provenance::designed(claim(SESSION_RATE_CLAIM)),
            )),
            actors,
            support: Vec::new(),
            pickups: Vec::new(),
            transitions: Vec::new(),
        },
    )
    .ok()
}

/// Every `Resolved::Unknown` a declared program carries, in declaration
/// order — the production path's own list of what is still open, wider than
/// the single refusal `lower_world_actors` reports first.
fn collect_open_fields(program: &DeclaredWorldActorProgram) -> Vec<OpenField> {
    let mut open = Vec::new();
    if let Resolved::Unknown { claim_id, reason } = program.ticks_per_second() {
        open.push(OpenField {
            actor: None,
            field: "ticks_per_second",
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        });
    }
    fn collect<T>(
        field: &'static str,
        actor: ProgramActor,
        resolved: &Resolved<T>,
        open: &mut Vec<OpenField>,
    ) {
        if let Resolved::Unknown { claim_id, reason } = resolved {
            open.push(OpenField {
                actor: Some(actor),
                field,
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    }
    for actor in program.actors() {
        collect("faction", actor.actor, &actor.faction, &mut open);
        match &actor.motion {
            DeclaredMotion::Held {
                position_m,
                orientation,
            } => {
                collect("position_m", actor.actor, position_m, &mut open);
                collect("orientation", actor.actor, orientation, &mut open);
            }
            DeclaredMotion::Path(path) => {
                for key in &path.keyframes {
                    collect("position_m", actor.actor, &key.position_m, &mut open);
                    collect("orientation", actor.actor, &key.orientation, &mut open);
                }
                collect(
                    "ticks_per_second",
                    actor.actor,
                    &path.ticks_per_second,
                    &mut open,
                );
            }
            DeclaredMotion::Route(route) => {
                collect("points", actor.actor, &route.points, &mut open);
                collect("speed_m_s", actor.actor, &route.speed_m_s, &mut open);
                collect(
                    "start_progress_m",
                    actor.actor,
                    &route.start_progress_m,
                    &mut open,
                );
            }
            DeclaredMotion::Free {
                position_m,
                velocity_m_s,
                orientation,
            } => {
                collect("position_m", actor.actor, position_m, &mut open);
                collect("velocity_m_s", actor.actor, velocity_m_s, &mut open);
                collect("orientation", actor.actor, orientation, &mut open);
            }
            DeclaredMotion::Carried { .. } => {}
        }
        for socket in &actor.sockets {
            collect("offset_m", actor.actor, &socket.offset_m, &mut open);
        }
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_sim::world_actors::runtime::WorldActorKind;
    use cs_types::content::Known;
    use cs_types::evidence::ClaimStatus;

    fn test_claim(name: &str) -> ClaimId {
        ClaimId::new(&format!("f34c.test.{name}")).expect("valid test claim")
    }

    fn designed<T>(value: T) -> Resolved<T> {
        Resolved::Known(Known::new(
            value,
            Provenance::designed(test_claim("designed")),
        ))
    }

    fn source_span() -> SourceSpan {
        SourceSpan::new(
            cs_types::evidence::ContentHash::from_hex(&"a".repeat(64)).expect("test hash"),
            "zbd/c1c/m01/zrdr.zbd",
            Some(ZEPPELIN_MEMBER),
            0,
            4,
            None,
        )
        .expect("test span")
    }

    fn row(index: usize, node: &str, subject: NodeJoin) -> SpawnedZeppelinActor {
        SpawnedZeppelinActor {
            index,
            node: node.to_owned(),
            stored_position: [-3678.6, 1460.0, -11985.3],
            stored_yaw: 180.0,
            stored_pitch: 0.0,
            attitude: AttitudeBinding::Measured(AttitudeSource::CarrierSpawn {
                yaw_radians: crate::mission_start::stored_heading_radians(180.0),
                pitch_radians: 0.0,
            }),
            position: PositionBinding::Measured(PositionSource::CarrierSpawn {
                position_m: [-3678.6, 1460.0, -11985.3],
            }),
            team: Some("enemy".to_owned()),
            allegiance: AllegianceBinding::Measured {
                team: 2,
                source: AllegianceSource::TeamSpelling,
            },
            deactivated: None,
            declared: matches!(&subject, NodeJoin::Single(_)).then(|| ProgramActor(index as u32)),
            subject,
        }
    }

    fn scene_subject(name: &str) -> NodeJoin {
        NodeJoin::Single(
            ContentId::from_source(ContentKind::SceneNode, &format!("container.test.{name}"))
                .expect("test node id"),
        )
    }

    fn mission() -> ContentId {
        ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("test mission id")
    }

    /// **A record whose `node` joins a world node declares an actor with the
    /// measured spawn pose, attitude and allegiance bound, and every still
    /// unsettled field refused by name.**
    /// The production declaration is what the launch surface reads: position
    /// metres under the spawn-pose claim, the composed attitude under the
    /// compose claim, and `faction` from the measured allegiance path — a
    /// record whose allegiance that path cannot settle keeps the field an
    /// explicit unknown under the open claim (#1155), and the session path
    /// still refuses it — the measurement is carried, never faked.
    #[test]
    fn accept_vs_m01_runtime_spawn_declaration_binds_only_the_measured_fields() {
        let mut unsettled = row(
            1,
            "workersvoyagezep",
            scene_subject("world1.workersvoyagezep"),
        );
        unsettled.team = None;
        unsettled.allegiance = AllegianceBinding::Open(
            "the fixture's subtree carries an unmodelled `NODES` element, so no allegiance \
             over the binding table is claimed"
                .to_owned(),
        );
        let rows = vec![
            row(0, "piratezep", scene_subject("world1.piratezep")),
            unsettled,
            // A record whose node names nothing declares no actor.
            row(2, "ghostzep", NodeJoin::Absent),
        ];
        let program = assemble_program(
            &mission(),
            &source_span(),
            None,
            None,
            SESSION_TICKS_PER_SECOND,
            &rows,
        )
        .expect("the joined records assemble a program");

        assert_eq!(
            program.actors().len(),
            2,
            "the unresolved node stays undeclared"
        );
        let actor = &program.actors()[0];
        assert_eq!(actor.kind, DeclaredWorldActorKind::Airship);
        let DeclaredMotion::Held {
            position_m,
            orientation,
        } = &actor.motion
        else {
            panic!("the spawn declares a held pose");
        };
        let Resolved::Known(known) = position_m else {
            panic!("the stored position binds measured");
        };
        let expected: [f64; 3] = rows[0].stored_position.map(f64::from);
        assert_eq!(
            known.value, expected,
            "the stored metres arrive unscaled and un-invented"
        );
        assert_eq!(
            known.provenance.class,
            ClaimStatus::ObservedTool,
            "the spawn pose is claimed at observed_tool, never stronger"
        );
        let Resolved::Known(allegiance) = &actor.faction else {
            panic!("the measured allegiance binds a faction");
        };
        assert_eq!(
            allegiance.value,
            ContentId::from_source(ContentKind::Faction, "enemy").expect("faction id"),
            "the row's `team` spelling is the loader's own override source (enemy → 2)"
        );
        assert_eq!(
            allegiance.provenance.claim_id.as_str(),
            ALLEGIANCE_RESOLVED_CLAIM
        );
        assert_eq!(
            allegiance.provenance.class,
            ClaimStatus::ObservedTool,
            "the faction is claimed at observed_tool, never stronger"
        );
        // The composed attitude binds measured under the compose claim, at
        // `observed_tool`, from the source the original applies last — the
        // record's own spawn attitude here, because the fixture's row states
        // no startup rotation.
        let Resolved::Known(attitude) = orientation else {
            panic!("the composed attitude binds measured");
        };
        assert_eq!(attitude.provenance.claim_id.as_str(), SPAWN_ATTITUDE_CLAIM);
        assert_eq!(attitude.provenance.class, ClaimStatus::ObservedTool);
        assert_eq!(
            attitude.value,
            object3d_orientation(
                0.0,
                f64::from(crate::mission_start::stored_heading_radians(180.0)),
                0.0
            ),
            "the spawn attitude composes as Ry(yaw)·Rx(pitch) with the roll slot clear"
        );
        assert_eq!(
            rows[0].attitude_source(),
            Some(&AttitudeSource::CarrierSpawn {
                yaw_radians: crate::mission_start::stored_heading_radians(180.0),
                pitch_radians: 0.0,
            }),
            "the row carries the source the original applies last"
        );
        assert!(
            rows[0].attitude_residue().contains("OBJECT_ROTATE_STATE"),
            "the source this one does not take is still named: {}",
            rows[0].attitude_residue()
        );

        // The resolver's terminal zero is a measured allegiance too: a
        // subtree with no binding resolves to team 0 (`0x4a3f80`), the
        // `neutral` spelling — never an unknown, never a guessed side.
        let mut bare = row(3, "blackswanzep", scene_subject("world1.blackswanzep"));
        bare.team = None;
        bare.allegiance = AllegianceBinding::Measured {
            team: 0,
            source: AllegianceSource::NoBinding,
        };
        let bare =
            declare_actor(&bare, ProgramActor(3), &source_span(), None, None).expect("declares");
        let Resolved::Known(neutral) = &bare.faction else {
            panic!("the resolver's zero binds `neutral`");
        };
        assert_eq!(
            neutral.value,
            ContentId::from_source(ContentKind::Faction, "neutral").expect("faction id"),
        );
        assert_eq!(
            neutral.provenance.claim_id.as_str(),
            ALLEGIANCE_RESOLVED_CLAIM
        );

        // The open-field list names every gap the lowering would hit, not
        // just the first: here, the one actor whose allegiance the measured
        // path cannot settle. The pose and attitude are measured (#792,
        // #814), so they are not part of the list.
        let open = collect_open_fields(&program);
        assert_eq!(open.len(), 1, "one actor x an unsettled allegiance");
        assert!(
            open.iter().all(|field| {
                field.field == "faction" && field.claim_id.as_str() == ALLEGIANCE_OPEN_CLAIM
            }),
            "the open fields are exactly the unsettled ones: {open:?}"
        );

        // The production lowering refuses the first unknown by name.
        let error = lower_world_actors(&program).expect_err("an unsettled allegiance refuses");
        assert!(
            matches!(error, WorldActorLowerError::UnknownValue { field, .. } if field == "faction"),
            "the first refusal names the faction: {error}"
        );
        assert!(error.to_string().contains(ALLEGIANCE_OPEN_CLAIM));
    }

    /// **When the fields are measured the same production path launches a
    /// session and the actor stands at its declared spawn.**
    /// The declared actor keeps the measured allegiance — the field #793
    /// left open and #1155 settled from the loader's own resolver — so the
    /// lowering, the launch and the anchor pose all run for real with the
    /// *measured* faction, attitude and pose still on the actor.
    #[test]
    fn accept_vs_m01_runtime_a_measured_program_launches_and_places_the_actor() {
        let row = row(0, "piratezep", scene_subject("world1.piratezep"));
        let expected_orientation = row
            .attitude_source()
            .expect("the fixture's attitude is settled")
            .orientation();
        let actor = declare_actor(&row, ProgramActor(0), &source_span(), None, None)
            .expect("the joined record declares");
        let Resolved::Known(allegiance) = &actor.faction else {
            panic!("the measured allegiance binds a faction");
        };
        assert_eq!(
            allegiance.value,
            ContentId::from_source(ContentKind::Faction, "enemy").expect("faction id"),
            "the launch runs with the measured allegiance, never a designed one"
        );
        assert_eq!(
            allegiance.provenance.claim_id.as_str(),
            ALLEGIANCE_RESOLVED_CLAIM
        );
        let program = DeclaredWorldActorProgram::try_new(
            mission(),
            Origin::SyntheticFixture,
            Provenance::designed(test_claim("program")),
            DeclaredWorldActorParts {
                ticks_per_second: designed(SESSION_TICKS_PER_SECOND),
                actors: vec![actor],
                support: Vec::new(),
                pickups: Vec::new(),
                transitions: Vec::new(),
            },
        )
        .expect("the fully-known program assembles");
        let lowered = lower_world_actors(&program).expect("the measured program lowers");
        assert_eq!(lowered.actors.len(), 1);
        assert_eq!(lowered.actors[0].kind, WorldActorKind::Airship);
        assert_eq!(
            lowered.actors[0].faction.key(),
            "enemy",
            "the session carries the measured allegiance, never a designed one"
        );
        let session = WorldActorSession::launch(lowered, SessionGeneration(1))
            .expect("the lowered program launches");
        let pose = session
            .set()
            .pose(cs_script::ir::ActorId(0))
            .expect("the actor's canonical pose is the session's read");
        assert_eq!(
            pose.position_m,
            [-3678.6_f32, 1460.0, -11985.3].map(f64::from),
            "the session places the actor at its measured spawn"
        );
        assert_eq!(
            pose.orientation.0, expected_orientation,
            "the session carries the composed attitude of the source that applies last"
        );
    }

    /// **An allegiance the measured path cannot settle — or one outside the
    /// loader's vocabulary — stays refused by name (#1155).**
    /// Measured absence is not a blanket pass: the open verdict refuses the
    /// lowering exactly as the retired unmapped-spelling verdict did, and a
    /// team int outside the measured `neutral`/`ally`/`enemy` vocabulary is
    /// never guessed into a faction.
    #[test]
    fn accept_m01_lc_zeppelin_allegiance_an_unsettled_allegiance_stays_refused() {
        let mut open = row(0, "piratezep", scene_subject("world1.piratezep"));
        open.team = None;
        open.allegiance = AllegianceBinding::Open(
            "`ctur1` is allegiance-marked and a `nodes` element of targets.zrd gates it, \
             whose team lane is not measurable"
                .to_owned(),
        );
        let rows = [open];
        let program = assemble_program(
            &mission(),
            &source_span(),
            None,
            None,
            SESSION_TICKS_PER_SECOND,
            &rows,
        )
        .expect("the joined record assembles");
        let open_fields = collect_open_fields(&program);
        assert_eq!(open_fields.len(), 1, "the unsettled faction is the gap");
        assert_eq!(open_fields[0].field, "faction");
        assert_eq!(
            open_fields[0].claim_id.as_str(),
            ALLEGIANCE_OPEN_CLAIM,
            "the gap is filed under the open allegiance claim"
        );
        let error = lower_world_actors(&program).expect_err("an unsettled allegiance refuses");
        assert!(
            matches!(error, WorldActorLowerError::UnknownValue { field, .. } if field == "faction"),
            "the refusal names the faction: {error}"
        );
        assert!(error.to_string().contains(ALLEGIANCE_OPEN_CLAIM));

        // A team int inside the engine's write but outside the measured
        // vocabulary binds no guessed faction either.
        let mut odd = row(
            1,
            "workersvoyagezep",
            scene_subject("world1.workersvoyagezep"),
        );
        odd.allegiance = AllegianceBinding::Measured {
            team: 7,
            source: AllegianceSource::TurretBinding {
                turret_record: 3,
                bound_node: "utur1".to_owned(),
            },
        };
        let odd = declare_actor(&odd, ProgramActor(1), &source_span(), None, None)
            .expect("the joined record declares");
        let Resolved::Unknown { claim_id, reason } = &odd.faction else {
            panic!("team 7 is outside the vocabulary: {:?}", odd.faction);
        };
        assert_eq!(claim_id.as_str(), ALLEGIANCE_OPEN_CLAIM);
        assert!(reason.contains("team 7"), "{reason}");
    }

    /// **The position binds the source the original applies last, and an
    /// unsettled one stays open by name (#814).**
    /// A startup placement binds the `STATE` triple under the spawn-pose
    /// claim provenanced from the statement's own span; an open binding is
    /// declared `Resolved::Unknown` under the precedence claim, verbatim.
    #[test]
    fn accept_m01_lc_zeppelin_placement_position_binding_picks_the_last_writer_or_stays_open() {
        let span = source_span();
        let state_span = SourceSpan::new(
            cs_types::evidence::ContentHash::from_hex(&"a".repeat(64)).expect("test hash"),
            "zbd/c1c/m01/zrdr.zbd",
            Some(PLACEZEPS_MEMBER),
            49_213 + 446,
            32,
            None,
        )
        .expect("the STATE range spans");

        // A startup translate that applies last binds its absolute triple,
        // provenanced from the STATE list's own bytes — never the carrier's
        // span.
        let mut startup = row(0, "piratezep", scene_subject("world1.piratezep"));
        startup.position = PositionBinding::Measured(PositionSource::StartupPlacement {
            position_m: [-3584.0, 1360.0, -8704.0],
            state_span: state_span.clone(),
        });
        let actor = declare_actor(&startup, ProgramActor(0), &span, None, None).expect("declares");
        let DeclaredMotion::Held { position_m, .. } = &actor.motion else {
            panic!("a held pose");
        };
        let Resolved::Known(known) = position_m else {
            panic!("the applied position binds measured");
        };
        assert_eq!(known.value, [-3584.0_f32, 1360.0, -8704.0].map(f64::from));
        assert_eq!(known.provenance.claim_id.as_str(), SPAWN_POSE_CLAIM);
        assert_eq!(known.provenance.class, ClaimStatus::ObservedTool);
        assert_eq!(
            known.provenance.source.as_ref().map(SourceSpan::offset),
            Some(state_span.offset()),
            "the startup value is provenanced from the STATE span"
        );
        assert!(
            startup.position_residue().contains("spawn position"),
            "the residue names the spawn position it overwrites: {}",
            startup.position_residue()
        );

        // A carrier spawn binds the record's own triple, provenanced from
        // the carrier's member, with the startup carrier's silence named.
        let carrier = row(1, "blackswanzep", scene_subject("world1.blackswanzep"));
        let actor = declare_actor(&carrier, ProgramActor(1), &span, None, None).expect("declares");
        let DeclaredMotion::Held { position_m, .. } = &actor.motion else {
            panic!("a held pose");
        };
        let Resolved::Known(known) = position_m else {
            panic!("the carrier's own position binds measured");
        };
        assert_eq!(
            known.value,
            carrier.stored_position.map(f64::from),
            "the spawn position is what the node keeps"
        );
        assert_eq!(
            known.provenance.source.as_ref().map(SourceSpan::offset),
            Some(span.offset())
        );
        assert!(
            carrier
                .position_residue()
                .contains("OBJECT_TRANSLATE_STATE"),
            "the residue names the startup carrier's silence: {}",
            carrier.position_residue()
        );

        // An unsettled ordering declares no position: the field stays an
        // explicit unknown under the precedence claim, verbatim.
        let mut open = row(
            2,
            "workersvoyagezep",
            scene_subject("world1.workersvoyagezep"),
        );
        open.position = PositionBinding::Open("the member refused: truncated".to_owned());
        let actor = declare_actor(&open, ProgramActor(2), &span, None, None).expect("declares");
        let DeclaredMotion::Held { position_m, .. } = &actor.motion else {
            panic!("a held pose");
        };
        let Resolved::Unknown { claim_id, reason } = position_m else {
            panic!("an open position binds nothing");
        };
        assert_eq!(claim_id.as_str(), POSITION_PRECEDENCE_UNKNOWN_CLAIM);
        assert_eq!(reason, "the member refused: truncated");
    }

    fn startup_statement(
        kind: StateKind,
        node: &str,
        activation: &str,
        unmodelled: &[&str],
    ) -> StartupStatement {
        StartupStatement {
            kind,
            node: node.to_owned(),
            activation: activation.to_owned(),
            values: [-3584.0, 1360.0, -8704.0],
            state_span: SourceSpan::new(
                cs_types::evidence::ContentHash::from_hex(&"a".repeat(64)).expect("test hash"),
                "zbd/c1c/m01/zrdr.zbd",
                Some(PLACEZEPS_MEMBER),
                49_213 + 446,
                32,
                None,
            )
            .expect("the STATE range spans"),
            unmodelled: unmodelled.iter().map(|key| (*key).to_owned()).collect(),
        }
    }

    /// **A flagged, deferred or competing startup write settles no pose —
    /// it stays open by name (#814).**
    /// The measured order settles every spawn write before every
    /// `ON_STARTUP` write and nothing else: a statement under another
    /// activation, a statement whose flags change what the executor
    /// applies, and a second startup write to the same node each leave the
    /// binding `Open` rather than guess a value the original did not write.
    #[test]
    fn accept_m01_lc_zeppelin_placement_position_unsettled_writers_stay_open() {
        let spawn = || AttitudeSource::CarrierSpawn {
            yaw_radians: 0.0,
            pitch_radians: 0.0,
        };
        let carrier = [-3678.6, 1460.0, -11985.3];

        // One clean ON_STARTUP translate binds its triple.
        let startup = StartupRead::Decoded(vec![startup_statement(
            StateKind::Translate,
            "piratezep",
            "ON_STARTUP",
            &[],
        )]);
        let PositionBinding::Measured(PositionSource::StartupPlacement { position_m, .. }) =
            position_for("piratezep", carrier, &startup)
        else {
            panic!("a clean startup translate binds");
        };
        assert_eq!(position_m, [-3584.0, 1360.0, -8704.0]);

        // A node no statement addresses keeps the carrier's own value.
        let PositionBinding::Measured(PositionSource::CarrierSpawn { position_m }) =
            position_for("blackswanzep", carrier, &startup)
        else {
            panic!("an unaddressed node keeps the spawn position");
        };
        assert_eq!(position_m, carrier);

        // A flagged translate does not apply its bare STATE triple.
        let startup = StartupRead::Decoded(vec![startup_statement(
            StateKind::Translate,
            "piratezep",
            "ON_STARTUP",
            &["RELATIVE"],
        )]);
        let PositionBinding::Open(reason) = position_for("piratezep", carrier, &startup) else {
            panic!("a flagged translate settles nothing");
        };
        assert!(reason.contains("RELATIVE"), "{reason}");

        // A statement under an activation the ordering does not measure is
        // not a startup write, whatever it states.
        let startup = StartupRead::Decoded(vec![startup_statement(
            StateKind::Translate,
            "piratezep",
            "OBJECTIVE13",
            &[],
        )]);
        let PositionBinding::Open(reason) = position_for("piratezep", carrier, &startup) else {
            panic!("a deferred translate settles nothing");
        };
        assert!(reason.contains("OBJECTIVE13"), "{reason}");

        // Two startup writes to one node have no measured order between
        // them; neither is picked.
        let startup = StartupRead::Decoded(vec![
            startup_statement(StateKind::Translate, "piratezep", "ON_STARTUP", &[]),
            startup_statement(StateKind::Translate, "piratezep", "ON_STARTUP", &[]),
        ]);
        let PositionBinding::Open(reason) = position_for("piratezep", carrier, &startup) else {
            panic!("competing startup writes settle nothing");
        };
        assert!(reason.contains("2 OBJECT_TRANSLATE_STATEs"), "{reason}");

        // The rotate half follows the same rule: `0x4e8b80` reads the same
        // flag word, so a flagged rotate settles no attitude either.
        let startup = StartupRead::Decoded(vec![startup_statement(
            StateKind::Rotate,
            "piratezep",
            "ON_STARTUP",
            &["AT_NODE"],
        )]);
        let AttitudeBinding::Open(reason) = attitude_for("piratezep", spawn(), &startup) else {
            panic!("a flagged rotate settles nothing");
        };
        assert!(reason.contains("AT_NODE"), "{reason}");
    }
}
