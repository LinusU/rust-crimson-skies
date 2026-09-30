//! The fixed-tick animation playback: verified transform, material and
//! attachment tracks (F20-B).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-B`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F20-A defined the declared IR
//! ([`cs_content::animation`](cs_content::animation)) and the fixed-tick
//! evaluator ([`cs_sim::animated_object`](cs_sim::animated_object)) but left
//! them unplugged: nothing played a clip inside a session and no consumer saw
//! a track's value. This module is that production path:
//!
//! * [`play_animation`] takes a validated declared
//!   [`AnimationClip`](cs_content::animation::AnimationClip), lowers it
//!   through [`lower::lower_clip`] and starts **one live instance** of it in
//!   the [`AnimationPlayback`] resource — refused when the world has no
//!   session, when that `animation_track` already plays, or when the clip
//!   does not survive the boundary;
//! * [`advance_animation`] is the fixed-tick entry: once per committed
//!   session tick it advances every playing instance, publishes the markers
//!   it crossed into the [`AnimationLog`], and applies the three track kinds
//!   this stage owns to the entities bound to them;
//! * [`stop_animation`] ends an instance (the applied components stay until
//!   their owner tears them down — spawn/despawn wiring is F20-C).
//!
//! # Verified application
//!
//! A track value reaches an entity only when the binding verifies:
//!
//! * the entity's [`AnimatedNodeBinding`] names a **playing** clip,
//! * and the **scene generation** that clip was started under — a binding
//!   stamped by a superseded scene load is never driven (F11/F20 session
//!   generation ownership), and
//! * the binding's node is one the clip actually drives, with an aspect that
//!   has a reached key.
//!
//! Every other entity is left exactly as it is. Nothing is guessed for a
//! missing entity: the state is derived from the clip position every tick, so
//! application is idempotent — running two advances over the same span writes
//! the same components, and a loop pass that repeats a pose writes it again
//! without re-firing anything (F20 non-negotiable behavior 3).
//!
//! # Unknowns block one track, not the node
//!
//! A material or attachment reference that is [`Resolved::Unknown`] never
//! becomes a component: the previously applied value stays and the refusal is
//! published once as a [`BlockedTrack`] carrying the unknown's claim id and
//! reason (F20 non-negotiable behavior 2 — an unknown retains its locator and
//! blocks the transition it gates). The report is a property of the playing
//! clip, so it is published whether or not an entity happens to be bound to
//! that node — the same way a blocked marker is — while the binding decides
//! only whether a *value* is applied. The *other* tracks of the same node keep
//! applying, and marker effects keep firing, so one undecoded reference
//! cannot silently stop a whole clip — it is visible in the log instead.
//!
//! # What this stage does not do
//!
//! The transform track is published as [`NodeAnimatedPose`], one component
//! per node: the pose a render and a collision consumer both read, so neither
//! can diverge (the F20-A `mesh_pose`/`collider_pose` rule carried into the
//! ECS). Recomposing descendants' world poses from it, reparenting entities,
//! inherited detach velocity, visibility/`NodePresentation` ordering with LOD
//! selection, mission-marker consumption and the schedule placement of the
//! advance are F20-C's wiring — see
//! `docs/findings/2026-09-30-f20-b-transform-material-attachment-tracks.md`.

use std::collections::{BTreeMap, HashSet};

use bevy::ecs::component::Component;
use bevy::ecs::world::World;
use bevy::prelude::{Entity, Resource};
use cs_content::animation::AnimationClip;
use cs_sim::animated_object::{
    AnimatedNodeState, AnimatedObject, AnimationEvent, AttachmentState, BlockedMarker, PoseSample,
};
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

use super::AnimatedNodeBinding;
use super::lower::{LowerError, lower_clip};

// --------------------------------------------------------------- tracks ---

/// Component: the pose the playing clip's transform track applied to this
/// node.
///
/// This is the node's one animated pose — the F20-A rule carried into the
/// ECS: [`Self::mesh`] and [`Self::collider`] are two names for the same
/// stored value, so a transform track cannot reach a render consumer without
/// reaching the collision consumer in the same tick (AC01's coherence, stated
/// structurally). An entity without the component has not been driven by a
/// transform track; the component is written only for a verified binding.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct NodeAnimatedPose(pub PoseSample);

impl NodeAnimatedPose {
    /// The pose the mesh presents.
    #[must_use]
    pub const fn mesh(&self) -> PoseSample {
        self.0
    }

    /// The pose the collider evaluates — the same value as [`Self::mesh`].
    #[must_use]
    pub const fn collider(&self) -> PoseSample {
        self.0
    }

    /// The stored pose.
    #[must_use]
    pub const fn pose(&self) -> PoseSample {
        self.0
    }
}

/// Component: the material the playing clip's material track applied to this
/// node.
///
/// Only a **known**, kind-validated `material` id ever reaches this
/// component: a [`Resolved::Unknown`] material is blocked and reported in the
/// [`AnimationLog`] instead, so a consumer never sees a material nobody
/// resolved and never guesses one.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct NodeAnimatedMaterial(pub ContentId);

impl NodeAnimatedMaterial {
    /// The applied material (`material` kind, validated when the clip was
    /// assembled).
    #[must_use]
    pub fn material(&self) -> &ContentId {
        &self.0
    }
}

/// Component: the attachment the playing clip's attachment track applied to
/// this node.
///
/// The record is the explicit parent change with its authored
/// [`PosePolicy`](cs_sim::animated_object::PosePolicy) — `parent: None` is a
/// detach, `Some(known)` an attachment under that node. Reparenting the ECS
/// hierarchy and recomputing the world/local pose belongs to F20-C; an
/// unknown parent never reaches this component (it is blocked and reported).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct NodeAnimatedAttachment(pub AttachmentState);

impl NodeAnimatedAttachment {
    /// The applied attachment state.
    #[must_use]
    pub fn attachment(&self) -> &AttachmentState {
        &self.0
    }
}

/// Which `Resolved` track kind an application refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrackKind {
    /// A material-swap key that resolved to nothing.
    Material,
    /// An attachment key whose parent resolved to nothing.
    Attachment,
}

impl TrackKind {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Material => "material",
            Self::Attachment => "attachment",
        }
    }
}

/// A track application that was blocked by an unknown reference.
///
/// Published once per (clip, node, track) per playing instance, from the
/// clip's evaluated state alone — an entity does not have to be bound for a
/// gap in the clip to be visible. An unknown that stays unresolved does not
/// append one entry per tick, and the same clip played again reports its own
/// gap again (the `AirframeSceneLog` "report the gap, not the frame" rule).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockedTrack {
    /// The playing clip the track belongs to.
    pub clip: ContentId,
    /// The node whose track was refused.
    pub node: ContentId,
    /// Which track kind was refused.
    pub track: TrackKind,
    /// The claim the unknown is recorded under.
    pub claim_id: ClaimId,
    /// Why the reference is unknown.
    pub reason: String,
}

/// Something the advance refused instead of performing.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationRefusal {
    /// The clip head was held: the session tick asked for a position behind
    /// the one the instance already reached, so it did not move — no marker
    /// re-fired and no track changed (F20 non-negotiable behavior 5: a
    /// reversed or restarted session never re-offers a one-shot marker).
    ///
    /// `from` is the clip time the instance sits at and `to` the one the
    /// session tick asked for. Reported once per occurrence, not once per
    /// tick while the session stays behind.
    Held {
        /// The clip whose head was held.
        clip: ContentId,
        /// The clip time the instance sits at.
        from: u64,
        /// The clip time the session tick asked for.
        to: u64,
    },
}

// ------------------------------------------------------------------ log ---

/// Resource: the append-only record of what the playback published.
///
/// This is the consumer seam of this stage: marker events (gameplay and
/// presentation), markers blocked by an unknown effect, tracks blocked by an
/// unknown reference and the advances that were refused. It grows with the
/// number of *published entries* — never one per tick — and is meant to be
/// drained by the layer that consumes them (the mission marker consumer is
/// F20-C's wiring), so [`Self::drain`] is how that consumer takes its batch.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct AnimationLog {
    events: Vec<AnimationEvent>,
    blocked_markers: Vec<BlockedMarker>,
    blocked_tracks: Vec<BlockedTrack>,
    refused: Vec<AnimationRefusal>,
}

impl AnimationLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The fired marker events, oldest first.
    #[must_use]
    pub fn events(&self) -> &[AnimationEvent] {
        &self.events
    }

    /// The marker crossings blocked by an unknown effect.
    #[must_use]
    pub fn blocked_markers(&self) -> &[BlockedMarker] {
        &self.blocked_markers
    }

    /// The track applications blocked by an unknown reference.
    #[must_use]
    pub fn blocked_tracks(&self) -> &[BlockedTrack] {
        &self.blocked_tracks
    }

    /// The advances that were refused instead of performed.
    #[must_use]
    pub fn refusals(&self) -> &[AnimationRefusal] {
        &self.refused
    }

    /// How many entries the log holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
            + self.blocked_markers.len()
            + self.blocked_tracks.len()
            + self.refused.len()
    }

    /// Whether nothing has been published since the last drain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes everything published so far, leaving the log empty.
    ///
    /// The returned log is the consumer's batch; draining is what keeps a
    /// long session from accumulating presentation cues that were already
    /// handed over.
    #[must_use]
    pub fn drain(&mut self) -> Self {
        std::mem::take(self)
    }
}

// ------------------------------------------------------------ playback ----

/// One live instance of a playing clip: the evaluator plus the identity its
/// events are stamped with.
#[derive(Debug)]
struct PlayingClip {
    /// The scene generation the instance serves; only bindings stamped with
    /// it are driven.
    generation: SceneGeneration,
    /// The session tick the clip started at; clip time is the ticks since.
    started_at: Tick,
    /// The producer serial this instance's event ids carry.
    producer: u32,
    /// The fixed-tick evaluator, held across advances so its per-activation
    /// dedup state survives every loop pass.
    object: AnimatedObject,
    /// The (node, track) applications already reported as blocked.
    blocked_tracks: HashSet<(ContentId, TrackKind)>,
    /// Whether a held (refused) advance is already on record.
    held_reported: bool,
}

/// Resource: the session's animation playback — its session id, the next
/// producer serial, and the live instance of every playing `animation_track`.
///
/// One live instance per clip id at this stage: the spawn wiring that runs
/// several instances of one track (one propeller per aircraft) is F20-C's,
/// and [`Self::play`] refuses a second instance instead of silently
/// replacing the first. The producer inserts this resource once per session
/// (`AnimationPlayback::new(session)`); [`play_animation`] refuses to start
/// anything without it, so an animation can never play in no session at all.
#[derive(Resource, Debug)]
pub struct AnimationPlayback {
    session: u64,
    next_producer: u32,
    playing: BTreeMap<ContentId, PlayingClip>,
}

impl AnimationPlayback {
    /// A playback for `session`: its events are stamped with that session
    /// generation, so a restarted session can never collide with the
    /// previous one's ids (`IDENTITY-CONTENT` session generations).
    #[must_use]
    pub fn new(session: u64) -> Self {
        Self {
            session,
            next_producer: 0,
            playing: BTreeMap::new(),
        }
    }

    /// The session generation every event id carries.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// Whether this `animation_track` currently plays.
    #[must_use]
    pub fn is_playing(&self, clip: &ContentId) -> bool {
        self.playing.contains_key(clip)
    }

    /// The playing clips, in stable id order.
    pub fn playing(&self) -> impl Iterator<Item = &ContentId> + '_ {
        self.playing.keys()
    }

    /// The clip time one playing instance has reached; `None` when the track
    /// is not playing.
    #[must_use]
    pub fn time(&self, clip: &ContentId) -> Option<u64> {
        self.playing.get(clip).map(|playing| playing.object.time())
    }

    /// The scene generation a playing instance serves.
    #[must_use]
    pub fn generation(&self, clip: &ContentId) -> Option<SceneGeneration> {
        self.playing.get(clip).map(|playing| playing.generation)
    }

    /// The producer serial a playing instance stamps its event ids with.
    #[must_use]
    pub fn producer(&self, clip: &ContentId) -> Option<u32> {
        self.playing.get(clip).map(|playing| playing.producer)
    }

    /// Whether a [`LoopMode::Once`](cs_sim::animated_object::LoopMode) track
    /// has reached its end; `None` when the track is not playing.
    #[must_use]
    pub fn is_finished(&self, clip: &ContentId) -> Option<bool> {
        self.playing
            .get(clip)
            .map(|playing| playing.object.is_finished())
    }

    /// How many clips are playing.
    #[must_use]
    pub fn len(&self) -> usize {
        self.playing.len()
    }

    /// Whether no clip is playing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.playing.is_empty()
    }

    /// Starts one instance of an already validated runtime clip.
    fn start(
        &mut self,
        clip: &AnimationClip,
        generation: SceneGeneration,
        at: Tick,
    ) -> Result<(), AnimationPlayError> {
        let runtime = lower_clip(clip).map_err(AnimationPlayError::Lower)?;
        let id = runtime.id().clone();
        if self.playing.contains_key(&id) {
            return Err(AnimationPlayError::AlreadyPlaying { clip: id });
        }
        // The serial space stops one short of `u32::MAX`, so a wrap can
        // never hand out an id an earlier event of this session already
        // fired with.
        if self.next_producer == u32::MAX {
            return Err(AnimationPlayError::ProducerExhausted);
        }
        let producer = self.next_producer;
        self.next_producer += 1;
        let object = AnimatedObject::new(runtime, self.session, producer);
        self.playing.insert(
            id,
            PlayingClip {
                generation,
                started_at: at,
                producer,
                object,
                blocked_tracks: HashSet::new(),
                held_reported: false,
            },
        );
        Ok(())
    }
}

/// Why a declared clip was not started.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationPlayError {
    /// The world carries no [`AnimationPlayback`]: there is no session to
    /// play in, so nothing was started.
    NoSession,
    /// That `animation_track` already has a live instance in this session.
    /// A second instance is refused rather than silently replacing the
    /// first; multi-instance spawn wiring is F20-C.
    AlreadyPlaying {
        /// The track that is already playing.
        clip: ContentId,
    },
    /// The declared clip did not survive the lowering boundary. Unreachable
    /// for a clip that passed [`AnimationClip::try_new`] (the runtime
    /// re-checks the same invariants), kept so a future drift between the
    /// two boundaries is refused here instead of reaching the evaluator.
    Lower(LowerError),
    /// The session's producer serials are exhausted; start a new session.
    ProducerExhausted,
}

impl std::fmt::Display for AnimationPlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSession => write!(f, "the world holds no animation playback to play in"),
            Self::AlreadyPlaying { clip } => write!(f, "the track {clip} is already playing"),
            Self::Lower(source) => write!(f, "the declared clip did not lower: {source}"),
            Self::ProducerExhausted => {
                write!(f, "no producer serial is left in this session")
            }
        }
    }
}

impl std::error::Error for AnimationPlayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lower(source) => Some(source),
            _ => None,
        }
    }
}

// ------------------------------------------------------- entry points -----

/// Starts one declared clip in the world's [`AnimationPlayback`].
///
/// The clip is lowered and re-validated at the boundary
/// ([`lower::lower_clip`]), so a playback can only ever drive the track
/// kinds, tick ranges and marker keys that both records accept. The
/// instance's event ids are stamped with the playback's session and a fresh
/// producer serial, and its clip time starts at `at`.
///
/// # Errors
///
/// [`AnimationPlayError::NoSession`] when the world holds no
/// [`AnimationPlayback`], [`AnimationPlayError::AlreadyPlaying`] when the
/// track already plays, [`AnimationPlayError::Lower`] when the declared clip
/// does not survive the boundary, [`AnimationPlayError::ProducerExhausted`]
/// when no producer serial is left.
pub fn play_animation(
    world: &mut World,
    clip: &AnimationClip,
    generation: SceneGeneration,
    at: Tick,
) -> Result<(), AnimationPlayError> {
    let Some(mut playback) = world.remove_resource::<AnimationPlayback>() else {
        return Err(AnimationPlayError::NoSession);
    };
    let started = playback.start(clip, generation, at);
    world.insert_resource(playback);
    started
}

/// Stops the instance of one `animation_track`, reporting whether it played.
///
/// The components the tracks already applied stay on their entities: who owns
/// them after a stop (teardown, a destroyed node, a reloaded scene) is F20-C's
/// spawn/despawn wiring, and this stage never despawns an entity.
pub fn stop_animation(world: &mut World, clip: &ContentId) -> bool {
    let Some(mut playback) = world.remove_resource::<AnimationPlayback>() else {
        return false;
    };
    let stopped = playback.playing.remove(clip).is_some();
    world.insert_resource(playback);
    stopped
}

/// Advances every playing clip to the session tick `at`, publishes what it
/// crossed and applies its tracks to the bound entities.
///
/// The caller is the fixed-tick driver: it calls this **once per committed
/// session tick**, in the F16-C order, with the tick stamped into every event
/// id. Clip time is the ticks since the instance started
/// (`at - started_at`), so the evaluator's head only ever moves forward: a
/// session tick that goes backwards holds the instance where it is and
/// publishes [`AnimationRefusal::Held`] instead of replaying anything (F20
/// non-negotiable behavior 5). An instance whose own start tick has not
/// arrived yet is passed over entirely — it offers no marker, applies no
/// state and reports no hold, so a clip scheduled to start at tick `N` is
/// silent at `N - 1`.
///
/// With no [`AnimationPlayback`] resource the call does nothing — there is
/// no session to advance.
pub fn advance_animation(world: &mut World, at: Tick) {
    let Some(mut playback) = world.remove_resource::<AnimationPlayback>() else {
        return;
    };

    // 1. Advance every instance and collect the markers it crossed. The
    //    evaluator keeps the per-activation dedup, so a looping clip keeps
    //    its one-shot gameplay event across passes.
    let mut events: Vec<AnimationEvent> = Vec::new();
    let mut blocked_markers: Vec<BlockedMarker> = Vec::new();
    let mut refused: Vec<AnimationRefusal> = Vec::new();
    for (id, playing) in playback.playing.iter_mut() {
        if at.0 < playing.started_at.0 {
            // The instance's own start tick has not arrived: it has not
            // played a single tick yet, so no marker is offered, no state is
            // applied and there is no head to hold (a clip scheduled to
            // start at tick N must not emit anything at tick N - 1 —
            // non-negotiable behavior 1, markers fire at their authored
            // tick of the fixed-tick simulation).
            continue;
        }
        let target = at.0 - playing.started_at.0;
        if target < playing.object.time() {
            if !playing.held_reported {
                playing.held_reported = true;
                refused.push(AnimationRefusal::Held {
                    clip: id.clone(),
                    from: playing.object.time(),
                    to: target,
                });
            }
            continue;
        }
        playing.held_reported = false;
        let outcome = playing.object.advance_to(target, at).expect(
            "the clip time was checked to move forward, and only a backwards head is refused",
        );
        events.extend(outcome.events);
        blocked_markers.extend(outcome.blocked);
    }

    // 2. Snapshot the evaluated state of every instance that has started —
    //    one coherent record per driven node, owned locally so the world's
    //    borrow ends before anything is written — and publish the unknown
    //    references its tracks carry. A gap in the clip's own content is
    //    reported whether or not an entity happens to be bound to that node
    //    (the same way a blocked marker is, without consulting the world);
    //    the binding decides only whether a *value* is applied, never
    //    whether a gap is visible.
    let mut states: BTreeMap<ContentId, (SceneGeneration, BTreeMap<ContentId, AnimatedNodeState>)> =
        BTreeMap::new();
    let mut blocked_tracks: Vec<BlockedTrack> = Vec::new();
    for (id, playing) in playback.playing.iter_mut() {
        if at.0 < playing.started_at.0 {
            // Not started yet: the instance contributes no state, so its
            // tracks apply to nothing and its gaps are not offered either.
            continue;
        }
        let node_states = playing.object.states();
        for (node, state) in &node_states {
            match state.material() {
                Some(Resolved::Unknown { claim_id, reason }) => publish_blocked(
                    playing,
                    &mut blocked_tracks,
                    id,
                    node,
                    TrackKind::Material,
                    claim_id,
                    reason,
                ),
                Some(Resolved::Known(_)) | None => {}
            }
            if let Some(AttachmentState {
                parent: Some(Resolved::Unknown { claim_id, reason }),
                ..
            }) = state.attachment()
            {
                publish_blocked(
                    playing,
                    &mut blocked_tracks,
                    id,
                    node,
                    TrackKind::Attachment,
                    claim_id,
                    reason,
                );
            }
        }
        states.insert(id.clone(), (playing.generation, node_states));
    }

    // 3. The candidate bindings, collected from the world before any write.
    let mut query = world.query::<(Entity, &AnimatedNodeBinding)>();
    let bindings: Vec<(Entity, AnimatedNodeBinding)> = query
        .iter(world)
        .map(|(entity, binding)| (entity, binding.clone()))
        .collect();

    // 4. Decide the writes: a track value reaches an entity only through a
    //    verified binding, and an unknown reference is never written.
    let mut writes: Vec<(Entity, NodeWrite)> = Vec::new();
    for (entity, binding) in bindings {
        let Some((generation, node_states)) = states.get(&binding.clip) else {
            // That track is not playing: the entity keeps its state.
            continue;
        };
        if *generation != binding.generation {
            // A binding stamped by another scene load is never driven.
            continue;
        }
        let Some(state) = node_states.get(&binding.node) else {
            // This clip does not drive that node.
            continue;
        };
        if let Some(pose) = state.pose().copied() {
            writes.push((entity, NodeWrite::Pose(pose)));
        }
        if let Some(Resolved::Known(known)) = state.material() {
            writes.push((entity, NodeWrite::Material(known.value.clone())));
        }
        if let Some(attachment) = state.attachment() {
            match &attachment.parent {
                // `None` is a detach — a definite transition, not an
                // unknown — and a known parent is kind-validated at clip
                // assembly, so both apply. An unknown parent was published
                // above and is never written.
                None | Some(Resolved::Known(_)) => {
                    writes.push((entity, NodeWrite::Attachment(attachment.clone())));
                }
                Some(Resolved::Unknown { .. }) => {}
            }
        }
    }

    // 5. Publish the playback, apply the writes idempotently, and append
    //    everything that was published to the log.
    world.insert_resource(playback);
    for (entity, write) in writes {
        apply_write(world, entity, write);
    }
    if !events.is_empty()
        || !blocked_markers.is_empty()
        || !blocked_tracks.is_empty()
        || !refused.is_empty()
    {
        let mut log = world.remove_resource::<AnimationLog>().unwrap_or_default();
        log.events.extend(events);
        log.blocked_markers.extend(blocked_markers);
        log.blocked_tracks.extend(blocked_tracks);
        log.refused.extend(refused);
        world.insert_resource(log);
    }
}

/// Records one unknown track of a playing instance, the first time the
/// evaluated state of that `(node, track)` reaches it — the once-per-instance
/// publication the [`BlockedTrack`] doc promises, independent of any binding.
fn publish_blocked(
    playing: &mut PlayingClip,
    published: &mut Vec<BlockedTrack>,
    clip: &ContentId,
    node: &ContentId,
    track: TrackKind,
    claim_id: &ClaimId,
    reason: &str,
) {
    if playing.blocked_tracks.insert((node.to_owned(), track)) {
        published.push(BlockedTrack {
            clip: clip.to_owned(),
            node: node.to_owned(),
            track,
            claim_id: claim_id.to_owned(),
            reason: reason.to_owned(),
        });
    }
}

/// One track value on its way to one verified entity.
#[derive(Clone, Debug, PartialEq)]
enum NodeWrite {
    /// The transform track's pose.
    Pose(PoseSample),
    /// The material track's resolved material.
    Material(ContentId),
    /// The attachment track's parent change.
    Attachment(AttachmentState),
}

/// Writes one track value onto its entity, only when it changed — running
/// the same advance twice writes the same component twice, and a component
/// that already holds the value is left untouched (idempotence).
fn apply_write(world: &mut World, entity: Entity, write: NodeWrite) {
    match write {
        NodeWrite::Pose(pose) => {
            insert_changed::<NodeAnimatedPose>(world, entity, NodeAnimatedPose(pose))
        }
        NodeWrite::Material(material) => {
            insert_changed::<NodeAnimatedMaterial>(world, entity, NodeAnimatedMaterial(material))
        }
        NodeWrite::Attachment(attachment) => insert_changed::<NodeAnimatedAttachment>(
            world,
            entity,
            NodeAnimatedAttachment(attachment),
        ),
    }
}

fn insert_changed<T: Component + PartialEq>(world: &mut World, entity: Entity, value: T) {
    if world.get::<T>(entity) != Some(&value) {
        world.entity_mut(entity).insert(value);
    }
}
