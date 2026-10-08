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
//! * **The spawn position binds.** The owner-supplied decrypted image reads
//!   the record's named `position` (three floats) into the spawned object's
//!   position slots `obj+0x20`..`obj+0x28` (`0x4bda64`..`0x4bdac1`, #770),
//!   and the stored unit is the metre with the identity axis map (#436's
//!   owner note, #677's census): each [`SpawnedZeppelinActor`] declares
//!   `position_m` [`Resolved::Known`] under [`SPAWN_POSE_CLAIM`]. M01's
//!   three records sit inside `c1c`'s measured node extent (`piratezep`
//!   ~517 m from the player start) — the frame the value claims is
//!   corroborated by containment, not asserted. The residue stays named:
//!   `placezeps.zrd`'s startup placements state *different* positions for
//!   the same nodes, and which mechanism the original applies last is not
//!   measured — the same class of residue `mission_start`'s `initial_pose`
//!   carries for the mission program.
//! * **The spawn attitude does not bind.** `yaw`/`pitch` are
//!   degrees→radians into `obj+0x2c`/`obj+0x30` (measured at `0x4bda64`),
//!   but how those two slots compose into the zeppelin object's world
//!   orientation was never traced — the `M = Ry·Rx·Rz` compose #770
//!   measured lives in `Object3d::SetRotation` at `class+0x1c`, a different
//!   object family. `orientation` stays [`Resolved::Unknown`] under
//!   [`ATTITUDE_UNKNOWN_CLAIM`].
//! * **The faction does not bind.** `team` spellings `ally`/`enemy` are the
//!   measured vocabulary, but nothing maps a spelling to a faction
//!   [`ContentId`] of this program — `faction` stays [`Resolved::Unknown`]
//!   under [`FACTION_UNKNOWN_CLAIM`].
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

use std::path::Path;

use cs_assets::install::Discovery;
use cs_content::coordinates::SourceAdapter;
use cs_content::objectives::ProgramActor;
use cs_content::scene::BindingMap;
use cs_content::world::world_scene_graph_from_gamez;
use cs_content::world_actors::{
    DeclaredMotion, DeclaredWorldActor, DeclaredWorldActorKind, DeclaredWorldActorParts,
    DeclaredWorldActorProgram,
};
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::zeppelins::read_zeppelins_member;
use cs_script::runtime::SessionGeneration;
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

/// The claim the measured spawn pose binds under.
///
/// `0x4bda64`..`0x4bdac1` of the owner-supplied image reads the record's
/// named `position` into the spawned object's `obj+0x20`..`obj+0x28` (#770);
/// the stored unit is the metre with the identity axis map (#436, #677).
/// The claim is the record's own value through that measured conversion —
/// the same shape `MissionStartConfiguration::initial_pose` binds. The
/// residue the claim does *not* cover: `placezeps.zrd`'s startup placements
/// state different positions for the same nodes, and which mechanism the
/// original applies last is unmeasured.
pub const SPAWN_POSE_CLAIM: &str = "f34-world.zeppelin-spawn-pose";

/// The claim the spawn attitude's refusal is filed under.
///
/// `yaw`/`pitch` are degrees→radians into the zeppelin object's `obj+0x2c`/
/// `obj+0x30` (measured at `0x4bda64`), but how those two slots compose
/// into a world orientation was never traced for this object family — the
/// `M = Ry·Rx·Rz` compose #770 measured is `Object3d`'s `class+0x1c`, a
/// different layout.
pub const ATTITUDE_UNKNOWN_CLAIM: &str = "f34-world.zeppelin-attitude-unmeasured";

/// Why the spawn attitude stays [`Resolved::Unknown`].
pub const ATTITUDE_UNKNOWN_REASON: &str = "the record's yaw/pitch are measured as \
    degrees→radians written to the spawned object's +0x2c/+0x30 slots (0x4bda64, #770), but the \
    composition of those slots into a world orientation was never traced for the zeppelin object \
    family — the M = Ry·Rx·Rz compose measured for spawn headings is Object3d::SetRotation's \
    class+0x1c layout, a different object — and placezeps.zrd's startup records state a different \
    yaw for workersvoyagezep (180 vs 220), so which source the original's pose lands on is open";

/// The claim the faction refusal is filed under.
///
/// `team` spellings `ally`/`enemy` are #574's measured vocabulary, but no
/// measured source maps a spelling to a faction [`ContentId`] of this
/// program's namespace.
pub const FACTION_UNKNOWN_CLAIM: &str = "f34-world.zeppelin-faction-unmeasured";

/// Why a record's faction stays [`Resolved::Unknown`].
pub const FACTION_UNKNOWN_REASON: &str = "the record may state a `team` spelling \
    (`ally`/`enemy` is the measured vocabulary), but no measured source maps a spelling to a \
    faction identity — no faction catalog joins it, and `net.zrd`, the member that may hold the \
    net→faction table, has no decoder";

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
    /// The `team` spelling, verbatim, when the record states one — its
    /// mapping is unmeasured ([`FACTION_UNKNOWN_CLAIM`]).
    pub team: Option<String>,
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
/// Nothing is invented: `position_m` binds measured under
/// [`SPAWN_POSE_CLAIM`], `orientation` and `faction` arrive
/// [`Resolved::Unknown`] under their own claims, and `open_fields` names
/// every open field regardless of which refusal the lowering hits first.
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

    // The subject join: the world container's canonical scene graph, the
    // same conversion the `world_geometry` surface runs. A refused graph
    // means no subject can be resolved — every record is reported with its
    // join outcome rather than placed on a guessed node.
    let scene = read_world_scene(install_root, found, group_dir);
    let rows: Vec<SpawnedZeppelinActor> = member
        .records()
        .iter()
        .enumerate()
        .map(|(index, record)| declare_row(index, record, scene.as_ref().ok()))
        .collect();

    let program = assemble_program(
        mission_subject,
        &member_span,
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

/// Reads `{group_dir}/gamez.zbd` and converts it through the production
/// `world_scene_graph_from_gamez` path — the same adapter and empty binding
/// map `measure_geometry` uses, so the subject join is the launch surface's
/// own measurement.
fn read_world_scene(
    install_root: &Path,
    found: &Discovery,
    group_dir: &str,
) -> Result<cs_content::scene::SceneGraph, String> {
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
    world_scene_graph_from_gamez(&container, &nodes, &[], &adapter, &bindings)
        .map(|scene| scene.graph().clone())
        .map_err(|error| format!("{error}"))
}

fn read_container_bytes(
    install_root: &Path,
    record: &InstallFileRecord,
) -> Result<Vec<u8>, String> {
    let host = install_root.join(record.relative_spelling.as_str());
    std::fs::read(&host).map_err(|error| format!("cannot read {}: {error}", host.display()))
}

/// One record's declared outcome: the stored pose kept verbatim, the `node`
/// name joined against the canonical scene graph, and the
/// [`DeclaredWorldActor`] when the join names a single node.
fn declare_row(
    index: usize,
    record: &cs_formats::zbd::zeppelins::ZeppelinRecord,
    scene: Option<&cs_content::scene::SceneGraph>,
) -> SpawnedZeppelinActor {
    let subject = match scene {
        None => NodeJoin::Absent,
        Some(graph) => {
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
        team: record.team().map(str::to_owned),
        deactivated: record.deactivated(),
        subject,
        declared,
    }
}

/// Declares one actor per row whose subject joined, with the measured pose
/// bound and every unmeasured field an explicit unknown.
fn declare_actor(
    row: &SpawnedZeppelinActor,
    actor: ProgramActor,
    span: &SourceSpan,
) -> Option<DeclaredWorldActor> {
    let NodeJoin::Single(subject) = &row.subject else {
        return None;
    };
    let pose_provenance = Provenance::new(
        claim(SPAWN_POSE_CLAIM),
        cs_types::evidence::ClaimStatus::ObservedTool,
        Some(span.clone()),
    )
    .expect("observed provenance with a source span");
    let [x, y, z] = row.stored_position;
    Some(DeclaredWorldActor {
        actor,
        subject: subject.clone(),
        kind: DeclaredWorldActorKind::Airship,
        faction: Resolved::unknown(claim(FACTION_UNKNOWN_CLAIM), FACTION_UNKNOWN_REASON)
            .expect("a reason is stated"),
        objective: None,
        motion: DeclaredMotion::Held {
            position_m: Resolved::Known(Known::new(
                [f64::from(x), f64::from(y), f64::from(z)],
                pose_provenance,
            )),
            orientation: Resolved::unknown(claim(ATTITUDE_UNKNOWN_CLAIM), ATTITUDE_UNKNOWN_REASON)
                .expect("a reason is stated"),
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
    session_ticks_per_second: u32,
    rows: &[SpawnedZeppelinActor],
) -> Option<DeclaredWorldActorProgram> {
    let actors: Vec<DeclaredWorldActor> = rows
        .iter()
        .filter_map(|row| {
            row.declared
                .and_then(|actor| declare_actor(row, actor, member_span))
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
            team: Some("enemy".to_owned()),
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
    /// measured spawn pose bound and every unmeasured field refused by name.**
    /// The production declaration is what the launch surface reads: position
    /// metres under the spawn-pose claim, `orientation` and `faction` as
    /// explicit unknowns, and the session path still refuses — the
    /// measurement is carried, never faked.
    #[test]
    fn accept_vs_m01_runtime_spawn_declaration_binds_only_the_measured_fields() {
        let rows = vec![
            row(0, "piratezep", scene_subject("world1.piratezep")),
            row(
                1,
                "workersvoyagezep",
                scene_subject("world1.workersvoyagezep"),
            ),
            // A record whose node names nothing declares no actor.
            row(2, "ghostzep", NodeJoin::Absent),
        ];
        let program = assemble_program(&mission(), &source_span(), SESSION_TICKS_PER_SECOND, &rows)
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
        for (resolved, claim) in [
            (format!("{orientation:?}"), ATTITUDE_UNKNOWN_CLAIM),
            (format!("{:?}", actor.faction), FACTION_UNKNOWN_CLAIM),
        ] {
            assert!(resolved.contains(claim), "the field is filed under {claim}");
        }

        // The open-field list names every gap the lowering would hit, not
        // just the first: orientation and faction for each declared actor.
        let open = collect_open_fields(&program);
        assert_eq!(open.len(), 4, "two actors x (orientation + faction)");
        assert!(
            open.iter()
                .all(|field| matches!(field.field, "orientation" | "faction")),
            "the open fields are exactly the unmeasured ones: {open:?}"
        );

        // The production lowering refuses the first unknown by name.
        let error = lower_world_actors(&program).expect_err("the unmeasured fields refuse");
        assert!(
            matches!(error, WorldActorLowerError::UnknownValue { field, .. } if field == "orientation"),
            "the first refusal names the attitude: {error}"
        );
        assert!(error.to_string().contains(ATTITUDE_UNKNOWN_CLAIM));
    }

    /// **When the fields are measured the same production path launches a
    /// session and the actor stands at its declared spawn.**
    /// The declared actor is rebuilt here with a designed faction and a
    /// designed identity orientation — standing in for future measurements —
    /// so the lowering, the launch and the anchor pose all run for real.
    #[test]
    fn accept_vs_m01_runtime_a_measured_program_launches_and_places_the_actor() {
        let mut actor = declare_actor(
            &row(0, "piratezep", scene_subject("world1.piratezep")),
            ProgramActor(0),
            &source_span(),
        )
        .expect("the joined record declares");
        actor.faction = designed(
            ContentId::from_source(ContentKind::Faction, "synthetic.raiders").expect("faction id"),
        );
        let DeclaredMotion::Held { position_m, .. } = actor.motion.clone() else {
            panic!("the spawn declares a held pose");
        };
        actor.motion = DeclaredMotion::Held {
            position_m,
            orientation: designed([0.0, 0.0, 0.0, 1.0]),
        };
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
    }
}
