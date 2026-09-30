//! The render session: the profile producer hand-off, the batched frame's ECS
//! consumer, teardown and retry
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-C`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! F17-B produced a [`crate::render::capture::FrameCapture`] — a value, not a
//! draw call — and named the missing half: "binding a texture handle needs an
//! `Assets<Image>` and is the consumer's job", and "the material with its own
//! blend state is F17-C's wiring". This module is that consumer, and it is
//! where the two ends meet:
//!
//! * **the producer** is a profile request ([`RenderProfileRequest`]), served
//!   once per run by [`process_render_profile_request`], which publishes the
//!   [`RenderSessionState`] the frame path reads;
//! * **the consumer** is [`sync_frame`], which takes a [`BatchedFrame`] and
//!   its [`SubmittedDraw`]s and makes the ECS hold what it names: one entity
//!   per batch carrying the batch's mesh, the batch's material with its image
//!   bound, and the per-instance rows the batch keeps ([`BatchDraw`]).
//!
//! # What the rules make structural
//!
//! 1. **A frame is synced under the profile it was built with.** A frame whose
//!    profile is not the applied one is refused
//!    ([`SyncError::ProfileMismatch`]) before a single entity is touched, so
//!    the ECS can never hold a frame drawn under settings nobody applied — and
//!    applying that profile and syncing again succeeds, which is the retry.
//! 2. **A session owns its work.** Every call takes a [`RenderSession`]; a
//!    session that did not open the world is refused
//!    ([`SyncError::ForeignSession`]) and changes nothing, so a frame built
//!    for a finished mission is never drawn after a switch.
//! 3. **No stale geometry.** Entities are keyed by the batch's own resource
//!    key: a batch that is still in the frame is reused, and every entity the
//!    previous frame spawned and this one does not claim is despawned.
//!    Reloading a frame a hundred times leaves the live entity count unchanged.
//! 4. **Nothing is drawn from nothing.** A world with no image store is
//!    refused ([`SyncError::NoImageStore`]) rather than drawn with unbound
//!    textures, and a batch whose material gap is still open is *not* spawned:
//!    it is counted in [`FrameSync::unmaterialed`], so the additive pass is
//!    visible instead of silently missing.
//! 5. **Teardown ends the session.** [`teardown`] despawns every batch entity
//!    and drops the state; a second call is a no-op, and a frame synced after
//!    it is refused because there is no session ([`SyncError::NoSession`]).
//!
//! # What the presentation reaches, and what it does not
//!
//! [`Presentation`] is applied to the entities that own the decision:
//! [`Msaa`] and Bevy's `Tonemapping` on the cameras that carry them,
//! [`DirectionalLight::shadow_maps_enabled`] on every directional light, and
//! the window's resolution when the profile sets one. Each is counted in
//! [`FrameSync::presentation`], including a count that is **zero** — a profile
//! that reached no camera is a reportable fact, not a success. Nothing else in
//! the world is touched: an enhancement cannot reach a material, a sort, a
//! collider or a visibility rule (spec F17 non-negotiable 5).
//!
//! # The asset load identity is still open
//!
//! The image bound here is the one F17-B's adapter produced, added to whatever
//! [`Assets<Image>`] the world already has. It is **not** stamped with an
//! installation span, a derived cache key or a load transaction, because this
//! stage has no load identity to stamp it with: F15's
//! [`crate::assets::ConvertedAsset`] cache is the owner of that
//! (`docs/findings/2026-09-30-f17-c-profiles-and-instance-batching.md`).
//!
//! # Designed, not original
//!
//! Which Bevy component carries each presentation decision is an engine fact.
//! What the original renderer used for any of them is unknown, and nothing
//! here claims otherwise.

use std::collections::BTreeMap;
use std::fmt;

use bevy::asset::{Assets, Handle};
use bevy::core_pipeline::tonemapping::Tonemapping as BevyTonemapping;
use bevy::ecs::entity::Entity;
use bevy::ecs::prelude::{Component, Resource, World};
use bevy::image::Image;
use bevy::light::DirectionalLight;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::render::view::Msaa;
use bevy::window::{Window, WindowResolution};
use cs_assets::install::sha256;
use cs_types::evidence::ContentHash;

use crate::livery::ModelInstanceId;
use crate::render::batch::{BatchInstance, BatchedFrame, InstanceBatch, SubmittedDraw};
use crate::render::capture::SceneOutcome;
use crate::render::material::RenderPhase;
use crate::render::profile::{
    Presentation, ProfileError, RenderProfile, bevy_tonemapping, msaa_for,
};

/// The generation of one render session.
///
/// A new mission, a mission retry or a world switch opens a new generation
/// (`docs/01-ARCHITECTURE.md`, "Application state and lifecycle"), the same
/// rule the F09-C livery session follows. Every call that touches a session's
/// work names its generation, so work built for a finished session is refused
/// rather than served.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Resource)]
pub struct RenderSession(pub u64);

impl fmt::Display for RenderSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "render-session#{}", self.0)
    }
}

/// The profile a session renders under, and how many times one was applied.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct RenderSessionState {
    session: RenderSession,
    profile: RenderProfile,
    applied: u64,
}

impl RenderSessionState {
    /// A session that has applied `profile` `applied` times.
    pub const fn new(session: RenderSession, profile: RenderProfile, applied: u64) -> Self {
        Self {
            session,
            profile,
            applied,
        }
    }

    /// The session that opened this state.
    pub const fn session(&self) -> RenderSession {
        self.session
    }

    /// The applied profile.
    pub const fn profile(&self) -> &RenderProfile {
        &self.profile
    }

    /// The digest of the applied profile.
    pub fn profile_fingerprint(&self) -> ContentHash {
        self.profile.fingerprint()
    }

    /// How many profiles were applied, which rises across a retry.
    pub const fn applied(&self) -> u64 {
        self.applied
    }

    /// The presentation the applied profile resolves to.
    pub fn presentation(&self) -> Presentation {
        self.profile.presentation()
    }

    /// Refuses a session that did not open this state.
    ///
    /// # Errors
    ///
    /// [`SyncError::ForeignSession`] when the sessions differ.
    pub fn require_session(&self, session: RenderSession) -> Result<(), SyncError> {
        if session == self.session {
            Ok(())
        } else {
            Err(SyncError::ForeignSession {
                runtime: self.session,
                session,
            })
        }
    }
}

/// Producer → consumer hand-off: what a session's renderer should be doing.
///
/// A request, never a mutation: the caller inserts it and
/// [`process_render_profile_request`] serves it once. That is what makes
/// "apply a profile, then sync a frame built under it" an order the schedule
/// cannot get wrong by accident — the frame path refuses a profile nobody
/// applied ([`SyncError::ProfileMismatch`]).
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub enum RenderProfileRequest {
    /// Render this session under this profile. A session already open under
    /// another profile switches to it, keeping its entities; a *different*
    /// session ends the open one first.
    Set {
        /// The session to render.
        session: RenderSession,
        /// The profile to render it under.
        profile: RenderProfile,
    },
    /// End this session: release every batch entity and drop the state.
    TearDown {
        /// The session to end.
        session: RenderSession,
    },
}

impl RenderProfileRequest {
    /// A request to render `session` under `profile`.
    pub const fn set(session: RenderSession, profile: RenderProfile) -> Self {
        Self::Set { session, profile }
    }

    /// A request to end `session`.
    pub const fn tear_down(session: RenderSession) -> Self {
        Self::TearDown { session }
    }

    /// The session the request names.
    pub const fn session(&self) -> RenderSession {
        match self {
            Self::Set { session, .. } | Self::TearDown { session } => *session,
        }
    }

    /// The requested profile, when the request sets one.
    pub const fn profile(&self) -> Option<&RenderProfile> {
        match self {
            Self::Set { profile, .. } => Some(profile),
            Self::TearDown { .. } => None,
        }
    }
}

/// Component: the batch an entity draws, and the per-instance rows it keeps.
///
/// `key` is the batch's own resource identity ([`batch_key`]), so the frame
/// path finds this entity again without trusting a position or a name.
/// `instances` is the point of the type: a batched draw keeps one row per
/// instance, with that instance's identity, paint and place, and the rows are
/// not folded into the shared material.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BatchDraw {
    key: ContentHash,
    phase: RenderPhase,
    image: Option<Handle<Image>>,
    instances: Vec<BatchInstance>,
}

impl BatchDraw {
    /// The batch resource key this entity draws.
    pub const fn key(&self) -> ContentHash {
        self.key
    }

    /// The pass this entity draws in.
    pub const fn phase(&self) -> RenderPhase {
        self.phase
    }

    /// The image every instance in this batch samples, when the batch has one.
    pub fn image(&self) -> Option<&Handle<Image>> {
        self.image.as_ref()
    }

    /// The per-instance rows, in draw order.
    pub fn instances(&self) -> &[BatchInstance] {
        &self.instances
    }

    /// The row of `instance`, when it is one of them.
    pub fn row(&self, instance: ModelInstanceId) -> Option<&BatchInstance> {
        self.instances.iter().find(|row| row.instance() == instance)
    }
}

/// Resource: the entities the live frame owns, by batch key.
///
/// Keyed by the digest's raw bytes because [`ContentHash`] is `Eq` and `Hash`
/// but not `Ord`, and the release order below is a stable one.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
struct BatchEntities(BTreeMap<[u8; 32], Entity>);

/// How far the applied presentation reached.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentationReach {
    /// Camera entities whose antialiasing or tone curve was set.
    pub cameras: usize,
    /// Directional lights whose shadow setting was set.
    pub lights: usize,
    /// Windows whose resolution was set. Zero unless the profile sets one.
    pub windows: usize,
}

/// What [`sync_frame`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameSync {
    /// Batches spawned as new entities.
    pub spawned: usize,
    /// Batches whose entity was already there and was updated in place.
    pub reused: usize,
    /// Entities the previous frame spawned and this one does not claim.
    pub released: usize,
    /// Batches that have no drawable material, so nothing was spawned for
    /// them.
    pub unmaterialed: usize,
    /// Draws the frame withheld, unchanged by the sync.
    pub withheld: usize,
    /// How far the applied presentation reached.
    pub presentation: PresentationReach,
}

/// What a teardown released.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderTeardown {
    /// Batch entities despawned.
    pub entities: usize,
    /// Whether a session state was dropped.
    pub sessions: bool,
}

/// Why a frame could not be synced, or a request could not be served.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncError {
    /// The call named a session that did not open this world.
    ForeignSession {
        /// The session that is open.
        runtime: RenderSession,
        /// The session the caller named.
        session: RenderSession,
    },
    /// The frame was built under a profile nobody applied, or under a
    /// different one than the applied profile. Nothing was touched.
    ProfileMismatch {
        /// The digest of the applied profile.
        applied: ContentHash,
        /// The digest of the profile the frame was built under.
        frame: ContentHash,
    },
    /// The world has no asset store for one of the resources a batch needs, so
    /// a mesh or image handle cannot be bound.
    NoAssetStore {
        /// Which store is missing: `"image"` or `"mesh"`.
        kind: &'static str,
    },
    /// The applied profile's sample count is not one this world can express.
    Profile(ProfileError),
    /// A frame row names a submitted-draw index that is not in the list.
    NoSubmittedDraw {
        /// The index the row names.
        index: usize,
    },
    /// No session state is present, so there is no profile to sync under.
    NoSession,
}

impl SyncError {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ForeignSession { .. } => "foreign_session",
            Self::ProfileMismatch { .. } => "profile_mismatch",
            Self::NoAssetStore { .. } => "no_asset_store",
            Self::Profile(error) => error.code(),
            Self::NoSubmittedDraw { .. } => "no_submitted_draw",
            Self::NoSession => "no_render_session",
        }
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { runtime, session } => {
                write!(
                    f,
                    "the render world belongs to {runtime} and cannot serve {session}"
                )
            }
            Self::ProfileMismatch { applied, frame } => write!(
                f,
                "the frame was built under profile {} and the applied profile is {applied}",
                frame
            ),
            Self::NoAssetStore { kind } => write!(
                f,
                "the world has no Assets<{kind}>, so a handle cannot be bound"
            ),
            Self::Profile(error) => write!(f, "{error}"),
            Self::NoSubmittedDraw { index } => {
                write!(f, "the frame's row {index} is in no submitted draw")
            }
            Self::NoSession => write!(f, "no render session is open"),
        }
    }
}

impl std::error::Error for SyncError {}

/// What happened to one profile request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileEvent {
    /// The request applied: this is the session's profile now.
    Applied {
        /// The session it applied to.
        session: RenderSession,
        /// The digest of the applied profile.
        profile: ContentHash,
    },
    /// The request was refused and nothing changed, so it can be retried.
    Refused {
        /// Why it was refused.
        reason: SyncError,
    },
    /// A teardown ended the session.
    TornDown {
        /// The session that ended.
        session: RenderSession,
    },
}

/// Resource: the profile events this world served, oldest first.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderProfileLog(pub Vec<ProfileEvent>);

impl RenderProfileLog {
    /// The events, oldest first.
    pub fn events(&self) -> &[ProfileEvent] {
        &self.0
    }

    /// The last event, when there is one.
    pub fn last(&self) -> Option<&ProfileEvent> {
        self.0.last()
    }

    /// How many events the log holds.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every refusal in the log, in order.
    pub fn refusals(&self) -> impl Iterator<Item = &SyncError> {
        self.0.iter().filter_map(|event| match event {
            ProfileEvent::Refused { reason } => Some(reason),
            _ => None,
        })
    }
}

fn log_profile_event(world: &mut World, event: ProfileEvent) {
    let mut log = world
        .remove_resource::<RenderProfileLog>()
        .unwrap_or_default();
    log.0.push(event);
    world.insert_resource(log);
}

/// Serves the queued [`RenderProfileRequest`] once.
///
/// An exclusive system because the whole apply is one transaction: the session
/// state and the batch entities it owns are read and written together, and
/// nothing may observe a profile applied to half a world.
///
/// [`RenderProfileRequest::Set`] for a **new** session ends the previous one
/// first — its batch entities are despawned — so a mission switch leaves
/// nothing of the old one drawn. A `Set` for the session already open replaces
/// only the profile, which is how a fidelity/enhanced switch is applied
/// without a teardown, and the state records how many times a profile was
/// applied. [`RenderProfileRequest::TearDown`] ends the session it names and
/// refuses any other ([`SyncError::ForeignSession`]). Every outcome is appended
/// to [`RenderProfileLog`], so a refusal is reported rather than swallowed.
pub fn process_render_profile_request(world: &mut World) {
    let Some(request) = world.remove_resource::<RenderProfileRequest>() else {
        return;
    };
    let session = request.session();
    let open = world.get_resource::<RenderSessionState>().cloned();
    match request {
        RenderProfileRequest::TearDown { .. } => match open {
            Some(state) if state.session() == session => {
                let _released = teardown(world);
                log_profile_event(world, ProfileEvent::TornDown { session });
            }
            _ => log_profile_event(
                world,
                ProfileEvent::Refused {
                    reason: SyncError::ForeignSession {
                        runtime: open.map_or(session, |state| state.session()),
                        session,
                    },
                },
            ),
        },
        RenderProfileRequest::Set { profile, .. } => {
            let switches_session = open
                .as_ref()
                .is_some_and(|state| state.session() != session);
            if switches_session {
                let _released = teardown(world);
            }
            let applied = open
                .as_ref()
                .filter(|state| state.session() == session)
                .map_or(1, |state| state.applied().saturating_add(1));
            let state = RenderSessionState::new(session, profile, applied);
            let fingerprint = state.profile_fingerprint();
            world.insert_resource(state);
            if open.is_none() || switches_session {
                world.insert_resource(BatchEntities::default());
            }
            log_profile_event(
                world,
                ProfileEvent::Applied {
                    session,
                    profile: fingerprint,
                },
            );
        }
    }
}

/// The stable identity of one batched draw.
///
/// A draw is its shared resources *and* the instance rows it draws: the same
/// pass, buffers, render state, image and committed paint, with the same
/// instances in it. Two batches therefore never collide — in particular two
/// batches whose paint is unresolved and which are identical in every
/// resource still differ, because they draw different instances — and the
/// entity this key names is exactly the draw, so a batch that changed which
/// instances it covers is a different draw and is respawned rather than
/// quietly redrawn with the old rows.
pub fn batch_key(batch: &InstanceBatch) -> ContentHash {
    let key = batch.key();
    let mut bytes = Vec::with_capacity(2 + 96 + 1 + 32);
    bytes.extend_from_slice(b"cs/render/batch/v1\0");
    bytes.extend_from_slice(key.phase().code().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(key.geometry().as_bytes());
    bytes.extend_from_slice(key.state().as_bytes());
    push_optional_hash(&mut bytes, key.image());
    push_optional_hash(&mut bytes, key.livery());
    bytes.push(u8::from(batch.mergeable()));
    bytes.extend_from_slice(&(batch.instances().len() as u32).to_le_bytes());
    for row in batch.instances() {
        bytes.extend_from_slice(&(row.item_index() as u32).to_le_bytes());
        bytes.extend_from_slice(row.item().as_str().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&row.instance().0.to_le_bytes());
    }
    sha256(&bytes)
}

fn push_optional_hash(bytes: &mut Vec<u8>, hash: Option<ContentHash>) {
    match hash {
        None => bytes.push(0),
        Some(hash) => {
            bytes.push(1);
            bytes.extend_from_slice(hash.as_bytes());
        }
    }
}

/// Makes the ECS hold what `frame` names.
///
/// The whole frame is prepared before a single entity is written, so a refusal
/// — a foreign session, a profile nobody applied, a world with no image store
/// — leaves the live entities exactly as they were and the caller can retry.
///
/// # Errors
///
/// [`SyncError`] before anything is written, and [`SyncError::NoSubmittedDraw`]
/// if a frame row names an index the submitted-draw list does not have, which
/// is a caller bug and is refused rather than skipped.
pub fn sync_frame(
    world: &mut World,
    draws: &[SubmittedDraw<'_>],
    frame: &BatchedFrame,
    session: RenderSession,
) -> Result<FrameSync, SyncError> {
    let state = world
        .get_resource::<RenderSessionState>()
        .ok_or(SyncError::NoSession)?;
    state.require_session(session)?;
    let applied = state.profile_fingerprint();
    if applied != frame.profile_fingerprint() {
        return Err(SyncError::ProfileMismatch {
            applied,
            frame: frame.profile_fingerprint(),
        });
    }
    // The presentation is resolved before anything is touched, so a sample
    // count the world cannot express is refused here rather than half-applied
    // below.
    let presentation = state.presentation();
    let msaa = msaa_for(presentation.msaa_samples()).map_err(SyncError::Profile)?;
    for (kind, present) in [
        ("Image", world.get_resource::<Assets<Image>>().is_some()),
        ("Mesh", world.get_resource::<Assets<Mesh>>().is_some()),
        (
            "StandardMaterial",
            world.get_resource::<Assets<StandardMaterial>>().is_some(),
        ),
    ] {
        if !present {
            return Err(SyncError::NoAssetStore { kind });
        }
    }

    // Every row is resolved against the submitted-draw list before the first
    // entity is written, so a stale list cannot spawn a half-frame.
    let mut prepared = Vec::with_capacity(frame.batches().len());
    for batch in frame.batches() {
        for row in batch.instances() {
            if row.item_index() >= draws.len() {
                return Err(SyncError::NoSubmittedDraw {
                    index: row.item_index(),
                });
            }
        }
        // One upload per batch: every row shares the batch's digests, so the
        // first row's buffers are the batch's buffers.
        let first = batch
            .instances()
            .first()
            .ok_or(SyncError::NoSubmittedDraw { index: 0 })?;
        let digest = batch_key(batch);
        prepared.push((*digest.as_bytes(), digest, batch, draws[first.item_index()]));
    }

    let stale = world
        .get_resource::<BatchEntities>()
        .cloned()
        .unwrap_or_default();
    let mut previous = stale.0.clone();
    let mut report = FrameSync {
        withheld: frame.withheld().len(),
        ..FrameSync::default()
    };
    let mut live = BTreeMap::new();

    for (key, digest, batch, submitted) in prepared {
        let SceneOutcome::Uploaded(upload) = submitted.outcome else {
            // A refused surface is not a batch: the frame already reported it.
            previous.remove(&key);
            continue;
        };
        // No drawable material yet: the batch is reported, not faked with
        // another class's blend, and no entity is left behind for it.
        let Some(base) = upload.standard_material() else {
            report.unmaterialed += 1;
            previous.remove(&key);
            continue;
        };
        let mut material = base.clone();
        let existing = previous.remove(&key).filter(|entity| {
            world
                .get_entity(*entity)
                .is_ok_and(|found| found.contains::<BatchDraw>())
        });
        let reused = existing.is_some();
        let entity = match existing {
            Some(entity) => {
                report.reused += 1;
                entity
            }
            None => {
                let mesh = world
                    .resource_mut::<Assets<Mesh>>()
                    .add(upload.geometry().mesh().clone());
                let entity = world.spawn_empty().id();
                world.entity_mut(entity).insert(Mesh3d(mesh));
                report.spawned += 1;
                entity
            }
        };
        // The batch's own image, bound once: every row in this batch samples
        // it, and a batch shared with another instance's paint could not exist
        // because the image is part of the key. A reused batch keeps the handle
        // it already has, so the store does not grow once per frame.
        let image = match (upload.image(), reused) {
            (None, _) => None,
            (Some(_), true) => world
                .get::<BatchDraw>(entity)
                .and_then(|draw| draw.image.clone()),
            (Some(source), false) => Some(
                world
                    .resource_mut::<Assets<Image>>()
                    .add(source.image().clone()),
            ),
        };
        if let Some(handle) = image.clone() {
            material.base_color_texture = Some(handle);
        }
        let handle = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(material);
        world.entity_mut(entity).insert((
            MeshMaterial3d::<StandardMaterial>(handle),
            BatchDraw {
                key: digest,
                phase: batch.phase(),
                image,
                instances: batch.instances().to_vec(),
            },
        ));
        live.insert(key, entity);
    }

    // Everything the previous frame spawned and this frame does not claim is
    // released, so a reload leaves no stale geometry behind.
    for entity in previous.values() {
        if world.get_entity(*entity).is_ok() {
            report.released += 1;
            world.entity_mut(*entity).despawn();
        }
    }
    world.insert_resource(BatchEntities(live));
    report.presentation = apply_presentation(world, presentation, msaa);
    Ok(report)
}

/// Applies `presentation` to the entities that own the decision.
///
/// Counts what it reached, including zero: a profile that reached no camera is
/// a fact the caller can see, not a silent success.
fn apply_presentation(
    world: &mut World,
    presentation: Presentation,
    msaa: Msaa,
) -> PresentationReach {
    let mut reach = PresentationReach::default();
    let curve = bevy_tonemapping(presentation.tonemap());
    let resolution = presentation.render_resolution();
    let entities = world
        .iter_entities()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    for entity in entities {
        let mut entity = world.entity_mut(entity);
        let mut camera = false;
        if let Some(mut samples) = entity.get_mut::<Msaa>() {
            *samples = msaa;
            camera = true;
        }
        if let Some(mut current) = entity.get_mut::<BevyTonemapping>() {
            *current = curve;
            camera = true;
        }
        if camera {
            reach.cameras += 1;
        }
        if let Some(mut light) = entity.get_mut::<DirectionalLight>() {
            light.shadow_maps_enabled = presentation.shadows();
            reach.lights += 1;
        }
        if let Some(resolution) = resolution
            && let Some(mut window) = entity.get_mut::<Window>()
        {
            window.resolution = WindowResolution::new(resolution.width, resolution.height);
            reach.windows += 1;
        }
    }
    reach
}

/// Ends the render session: despawns every batch entity and drops the state.
///
/// A no-op when nothing is live, so a repeated teardown, a teardown after a
/// refused request and a teardown at shutdown are all safe.
pub fn teardown(world: &mut World) -> RenderTeardown {
    let mut report = RenderTeardown::default();
    if let Some(entities) = world.remove_resource::<BatchEntities>() {
        for entity in entities.0.values() {
            if world.get_entity(*entity).is_ok() {
                report.entities += 1;
                world.entity_mut(*entity).despawn();
            }
        }
    }
    report.sessions = world.remove_resource::<RenderSessionState>().is_some();
    report
}
