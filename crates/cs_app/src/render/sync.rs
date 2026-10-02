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
//!   per batch carrying the batch's mesh and the batch's material with its
//!   image bound ([`BatchDraw`]), and one child entity per row
//!   ([`BatchInstancePlacement`]) so a batch of *n* aircraft is *n* placed
//!   draws of one geometry rather than one draw of the first aircraft only.
//!   The image a painted batch binds is the composed livery variant its key
//!   carries — the paint in texels, resolved through
//!   [`crate::render::paint`] — not the unpainted canonical image.
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
//!    previous frame spawned and this one does not claim is despawned — with
//!    the per-instance entities below it, which a recursive despawn takes with
//!    it. Reloading a frame a hundred times leaves the live entity count
//!    unchanged.
//! 4. **Nothing is drawn from nothing.** A world with no asset store is
//!    refused ([`SyncError::NoAssetStore`]) rather than drawn with unbound
//!    textures, and every batch draws with the material its own render state
//!    produced — the additive class with
//!    [`AdditiveMaterial`](crate::render::additive::AdditiveMaterial), every
//!    other class with a `StandardMaterial` — so the additive pass is placed
//!    like any other instead of being counted as a gap.
//! 5. **Teardown ends the session.** [`teardown`] despawns every batch entity
//!    and drops the state; a second call is a no-op, and a frame synced after
//!    it is refused because there is no session ([`SyncError::NoSession`]).
//! 6. **What is drawn comes from the composed visibility verdict.** Each row's
//!    draw state is [`crate::render::visibility::row_draw`], which reads the one
//!    verdict F11-C and F20-C.03 compose out of the LOD/damage record and a
//!    playing clip's own visibility record, and is reported in
//!    [`FrameSync::visibility`]. This consumer ranks nothing of its own: it
//!    places the rows the verdict draws, withholds the rest before the first
//!    entity of that batch is written, and counts every row it decided about.
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
//! collider or a visibility rule (spec F17 non-negotiable 5). Rule 6 is the same
//! either way: the draw decision is the composed verdict, and a profile switch
//! cannot move a row
//! (`accept_f20_c_draw_a_profile_switch_never_reaches_the_visibility_rule`).
//!
//! The apply happens at the *end* of [`sync_frame`], not in
//! [`process_render_profile_request`], so it is counted with the frame it
//! presented. The cost is an ordering assumption a caller must meet: a profile
//! request served in the same frame as a sync reaches the renderer *after* that
//! frame, so the first frame after a profile switch is still presented under the
//! previous profile. There is no render app or schedule in the crate yet to
//! order the two, and putting the apply in both places would give it two
//! precedences with one of them untested.
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
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::prelude::{Component, Resource, World};
use bevy::image::Image;
use bevy::light::DirectionalLight;
use bevy::math::Vec3;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::render::view::Msaa;
use bevy::transform::prelude::Transform;
use bevy::window::{Window, WindowResolution};
use cs_assets::install::sha256;
use cs_types::evidence::ContentHash;

use crate::livery::ModelInstanceId;
use crate::render::additive::AdditiveMaterial;
use crate::render::batch::{BatchInstance, BatchedFrame, InstanceBatch, SubmittedDraw};
use crate::render::bevy_state::{DrawableMaterial, MaterialKind};
use crate::render::capture::SceneOutcome;
use crate::render::material::RenderPhase;
use crate::render::paint::{PaintSource, upload_paint};
use crate::render::plan::DrawItemKey;
use crate::render::profile::{
    Presentation, ProfileError, RenderProfile, bevy_tonemapping, msaa_for,
};
use crate::render::visibility::{VisibilityReport, row_draw};

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

    /// The image every instance in this batch samples, when the batch has
    /// one: the composed paint of the batch's variant when the paint is
    /// established ([`crate::render::paint`]), the surface's canonical image
    /// otherwise.
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

/// Component: one row of a batch, placed in the world.
///
/// A batch is *one draw of one geometry*, so the *n* instances it covers are
/// *n* entities that share the batch's mesh and material handles and differ in
/// their place. This is where the per-instance state stops being bookkeeping:
/// [`BatchDraw::instances`] says a batch covers three aircraft at three
/// positions, and one child entity per row puts each of them at its own
/// position. Without it a three-aircraft batch would draw a single quad at the
/// origin — one aircraft wearing three identities, which is exactly the failure
/// AC03 exists to prevent.
///
/// The row itself is carried, not inferred from the transform, so a consumer
/// can ask which aircraft an entity draws without trusting a position.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BatchInstancePlacement {
    row: BatchInstance,
}

impl BatchInstancePlacement {
    /// The row this placement came from.
    pub const fn row(&self) -> &BatchInstance {
        &self.row
    }

    /// The model instance this entity draws for.
    pub const fn instance(&self) -> ModelInstanceId {
        self.row.instance()
    }

    /// Where the row places this instance, in meters.
    pub const fn center_m(&self) -> [f32; 3] {
        self.row.center_m()
    }
}

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
    /// Per-instance entities the live batches carry. Equal to the frame's
    /// instance count when every row is placed, which is what makes "each
    /// aircraft is drawn at its own place" a reported fact rather than an
    /// assumption.
    pub placed: usize,
    /// Batches that bound a composed paint image: the per-instance paint
    /// reached the bound texels, not just the batch key.
    pub painted: usize,
    /// Draws the frame withheld, unchanged by the sync.
    pub withheld: usize,
    /// How far the applied presentation reached.
    pub presentation: PresentationReach,
    /// What the composed visibility verdict decided for every row the frame
    /// held, and how many of them were placed because of it.
    ///
    /// [`FrameSync::withheld`] is the *batcher*'s report, taken from a damage
    /// snapshot before this consumer ran; this is the render path's own, taken
    /// from the records the world holds at the moment of the sync. Both are
    /// reported because they answer different questions — what the frame
    /// planned, and what was drawn.
    pub visibility: VisibilityReport,
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
    /// a mesh, image or material handle cannot be bound.
    NoAssetStore {
        /// Which store is missing: `"Image"`, `"Mesh"`, `"StandardMaterial"`
        /// or `"AdditiveMaterial"`.
        kind: &'static str,
    },
    /// The applied profile's sample count is not one this world can express.
    Profile(ProfileError),
    /// A frame row names a submitted-draw index that is not in the list.
    NoSubmittedDraw {
        /// The index the row names.
        index: usize,
    },
    /// The submitted draw a frame row names is not the one the frame was built
    /// from, so the list is stale with respect to the frame and the batch would
    /// be bound to buffers the frame never recorded.
    StaleSubmittedDraw {
        /// Index into the submitted-draw list.
        index: usize,
        /// The draw item the row names.
        item: DrawItemKey,
        /// What the submitted draw turned out to be:
        /// [`stale_draw_codes::REFUSED`] when the surface is not an upload at
        /// all, [`stale_draw_codes::WRONG_ITEM`] when it is an upload of
        /// another draw item, or [`stale_draw_codes::UPLOAD_MISMATCH`] when it
        /// is the right draw item with resources the batch key does not digest.
        why: &'static str,
    },
    /// A batch's committed paint names a variant the paint source never
    /// composed, so the painted texels do not exist to bind. The frame is
    /// refused rather than drawn with the unpainted image, which would paint
    /// the aircraft a paint it never chose — the failure AC03 exists to
    /// prevent.
    PaintNotComposed {
        /// The batch resource key the missing variant belongs to.
        batch: ContentHash,
        /// The digest of the variant the batch key carries.
        variant: ContentHash,
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
            Self::StaleSubmittedDraw { .. } => "stale_submitted_draw",
            Self::PaintNotComposed { .. } => "paint_not_composed",
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
            Self::StaleSubmittedDraw { index, item, why } => write!(
                f,
                "submitted draw {index} ({item}) is not the one this frame was built from: {why}"
            ),
            Self::PaintNotComposed { batch, variant } => write!(
                f,
                "batch {batch} paints with variant {variant}, which the paint source never composed"
            ),
            Self::NoSession => write!(f, "no render session is open"),
        }
    }
}

/// The reason codes a [`SyncError::StaleSubmittedDraw`] carries.
pub mod stale_draw_codes {
    /// The submitted draw is a refusal, so it has no buffers to bind; the frame
    /// was built from an upload.
    pub const REFUSED: &str = "refused";
    /// The submitted draw is an upload of a *different* draw item than the row
    /// names, so the list pairs an item with another item's outcome.
    pub const WRONG_ITEM: &str = "wrong_draw_item";
    /// The submitted draw is an upload of the right draw item, but not of the
    /// geometry, render state or image the batch key digests, so binding it
    /// would draw the batch with another surface's resources.
    pub const UPLOAD_MISMATCH: &str = "upload_mismatch";
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
    /// A teardown ended the session. `released` is what it actually took
    /// with it, so a repeated teardown of a session that is already gone is
    /// visible as a no-op rather than reported as a failure.
    TornDown {
        /// The session that ended.
        session: RenderSession,
        /// What the teardown released.
        released: RenderTeardown,
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
/// applied. [`RenderProfileRequest::TearDown`] ends the session it names,
/// refuses any other ([`SyncError::ForeignSession`]) and is a reported no-op
/// when nothing is open, because a session is not foreign to itself. Every
/// outcome is appended to [`RenderProfileLog`], so a refusal is reported
/// rather than swallowed.
pub fn process_render_profile_request(world: &mut World) {
    let Some(request) = world.remove_resource::<RenderProfileRequest>() else {
        return;
    };
    let session = request.session();
    let open = world.get_resource::<RenderSessionState>().cloned();
    match request {
        RenderProfileRequest::TearDown { .. } => match open {
            Some(state) if state.session() == session => {
                let released = teardown(world);
                log_profile_event(world, ProfileEvent::TornDown { session, released });
            }
            Some(state) => log_profile_event(
                world,
                ProfileEvent::Refused {
                    reason: SyncError::ForeignSession {
                        runtime: state.session(),
                        session,
                    },
                },
            ),
            // Nothing is open: the request is a second teardown, which releases
            // nothing and is not a refusal.
            None => {
                let released = teardown(world);
                log_profile_event(world, ProfileEvent::TornDown { session, released });
            }
        },
        RenderProfileRequest::Set { profile, .. } => {
            let switches_session = open
                .as_ref()
                .is_some_and(|state| state.session() != session);
            if switches_session || open.is_none() {
                // Ends the previous session's work, or releases anything a
                // dropped state left behind, so a `Set` never orphans a live
                // batch entity by replacing the map it is tracked in.
                teardown(world);
                world.insert_resource(BatchEntities::default());
            }
            let applied = open
                .as_ref()
                .filter(|state| state.session() == session)
                .map_or(1, |state| state.applied().saturating_add(1));
            let state = RenderSessionState::new(session, profile, applied);
            let fingerprint = state.profile_fingerprint();
            world.insert_resource(state);
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
/// — a foreign session, a profile nobody applied, a world with no asset store,
/// a submitted-draw list the frame was not built from — leaves the live
/// entities exactly as they were and the caller can retry.
///
/// What it writes per batch: one entity with the batch's [`Mesh3d`] and one
/// [`MeshMaterial3d`] whose material is the batch's own, and one child entity
/// per row ([`BatchInstancePlacement`]) at that row's own place, sharing the
/// same mesh and material handles. Every class has a drawable material, so
/// every batch is placed: the additive class with
/// [`AdditiveMaterial`], every other class with a `StandardMaterial`, and the
/// component's own type is the material, so one batch entity never carries
/// both.
///
/// `paints` is where each batch's committed paint is resolved to composed
/// bytes ([`PaintSource`]): a textured batch with an established paint binds
/// the composed variant image in place of the surface's canonical one. A
/// batch that samples no image binds nothing for its paint — a material with
/// no texture slot has no texels to paint.
///
/// # Errors
///
/// [`SyncError`] before anything is written, [`SyncError::NoSubmittedDraw`]
/// or [`SyncError::StaleSubmittedDraw`] if a frame row names an index the
/// submitted-draw list does not have, or an outcome that is not the one the
/// frame was built from (both caller bugs, refused rather than skipped), and
/// [`SyncError::PaintNotComposed`] if a batch's committed paint names a
/// variant the paint source never composed — never silently replaced with
/// the unpainted image.
pub fn sync_frame(
    world: &mut World,
    draws: &[SubmittedDraw<'_>],
    frame: &BatchedFrame,
    session: RenderSession,
    paints: &dyn PaintSource,
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
    // Every store a batch can bind into, resolved before the first entity is
    // written. The additive class's store is here for the same reason the
    // other three are: every class has a drawable material now, so a frame
    // with an additive batch would otherwise be discovered — and half-written
    // — before the missing store was noticed.
    for (kind, present) in [
        ("Image", world.get_resource::<Assets<Image>>().is_some()),
        ("Mesh", world.get_resource::<Assets<Mesh>>().is_some()),
        (
            "StandardMaterial",
            world.get_resource::<Assets<StandardMaterial>>().is_some(),
        ),
        (
            "AdditiveMaterial",
            world.get_resource::<Assets<AdditiveMaterial>>().is_some(),
        ),
    ] {
        if !present {
            return Err(SyncError::NoAssetStore { kind });
        }
    }

    // Every row is resolved against the submitted-draw list before the first
    // entity is written, so a stale list cannot spawn a half-frame. Each row
    // must be the outcome the frame was built from, not merely an index that
    // exists: a batch's buffers come from the row the batcher read, and a
    // different upload at that index is a different surface's resources.
    let mut prepared = Vec::with_capacity(frame.batches().len());
    for batch in frame.batches() {
        for row in batch.instances() {
            let index = row.item_index();
            let Some(submitted) = draws.get(index) else {
                return Err(SyncError::NoSubmittedDraw { index });
            };
            let SceneOutcome::Uploaded(upload) = submitted.outcome else {
                return Err(SyncError::StaleSubmittedDraw {
                    index,
                    item: row.item().clone(),
                    why: stale_draw_codes::REFUSED,
                });
            };
            if submitted.outcome.key() != row.item() {
                return Err(SyncError::StaleSubmittedDraw {
                    index,
                    item: row.item().clone(),
                    why: stale_draw_codes::WRONG_ITEM,
                });
            }
            if !batch.key().matches(upload) {
                return Err(SyncError::StaleSubmittedDraw {
                    index,
                    item: row.item().clone(),
                    why: stale_draw_codes::UPLOAD_MISMATCH,
                });
            }
        }
        // One upload per batch: every row shares the batch's digests, so the
        // first row's buffers are the batch's buffers. Checked above, so this
        // cannot be a refusal.
        let first = batch
            .instances()
            .first()
            .ok_or(SyncError::NoSubmittedDraw { index: 0 })?;
        let digest = batch_key(batch);
        let SceneOutcome::Uploaded(upload) = draws[first.item_index()].outcome else {
            return Err(SyncError::StaleSubmittedDraw {
                index: first.item_index(),
                item: first.item().clone(),
                why: stale_draw_codes::REFUSED,
            });
        };
        // The batch's paint, resolved before anything is written. The paint
        // reaches the GPU in the texels of the composed variant
        // (`crate::render::paint`): a variant the paint source has no bytes
        // for is refused, never silently replaced with the unpainted image.
        // A batch that samples no image has no texture slot for a paint.
        let paint = match (batch.key().paint(), upload.image()) {
            (Some(variant), Some(_)) => Some(upload_paint(
                paints
                    .variant(&variant)
                    .ok_or(SyncError::PaintNotComposed {
                        batch: digest,
                        variant: variant.digest(),
                    })?,
                upload.state().address(),
            )),
            _ => None,
        };
        prepared.push((*digest.as_bytes(), digest, batch, upload, paint));
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
    // One texture per paint upload, not one per batch: two batches that
    // share a variant *and* addressing (different phases or non-consecutive
    // items) bind the same texels, so they bind the same handle. The key is
    // the upload's fingerprint — variant, extent, addressing and texels —
    // not the variant alone: a paint sampled under two different address
    // modes is two textures.
    let mut paint_handles = BTreeMap::<[u8; 32], Handle<Image>>::new();

    for (key, digest, batch, upload, paint) in prepared {
        // The composed visibility verdict decides what is drawn, once per row,
        // from the records the world holds right now. It is read before the
        // first entity of this batch is written, so a withheld row leaves
        // nothing behind: no placement, and no mesh or material added to a
        // store for a draw that is not drawn.
        let drawn_rows = drawn_rows(world, draws, batch, &mut report.visibility);
        if drawn_rows.is_empty() {
            // Every row of this batch is kept off the screen, so the batch has
            // no draw at all: it is not spawned, and the entity a previous frame
            // placed for it is released below with everything else this frame
            // no longer claims. `previous` is deliberately not touched here —
            // a released batch must go through the one release path, so the
            // count in `released` and the live map cannot disagree.
            continue;
        }
        let existing = reuse_batch(&mut previous, key, world, &mut report.released);
        let reused = existing.is_some();
        let (entity, mesh) = match existing {
            Some(found) => {
                report.reused += 1;
                found
            }
            None => {
                let mesh = world
                    .resource_mut::<Assets<Mesh>>()
                    .add(upload.geometry().mesh().clone());
                let entity = world.spawn_empty().id();
                world.entity_mut(entity).insert(Mesh3d(mesh.clone()));
                report.spawned += 1;
                (entity, mesh)
            }
        };
        // The batch's own image, bound once: every row in this batch samples
        // it. A batch with an established paint binds the composed variant in
        // place of the canonical image — the paint is in the texels now, not
        // only in the key — and a batch shared with another instance's paint
        // could not exist because the paint is part of the key. A reused
        // batch keeps the handle it already has, so the store does not grow
        // once per frame.
        let image = match (&paint, upload.image(), reused) {
            (Some(_), _, true) => world
                .get::<BatchDraw>(entity)
                .and_then(|draw| draw.image.clone()),
            (Some(paint), _, false) => Some(
                paint_handles
                    .entry(*paint.fingerprint().as_bytes())
                    .or_insert_with(|| {
                        world
                            .resource_mut::<Assets<Image>>()
                            .add(paint.image().clone())
                    })
                    .clone(),
            ),
            (None, Some(_), true) => world
                .get::<BatchDraw>(entity)
                .and_then(|draw| draw.image.clone()),
            (None, Some(source), false) => Some(
                world
                    .resource_mut::<Assets<Image>>()
                    .add(source.image().clone()),
            ),
            (None, None, _) => None,
        };
        report.painted += usize::from(paint.is_some());
        // The material is a function of the batch key — the key carries the
        // render state, and the render state carries the class, so it decides
        // the material kind. A reused batch already holds the one this call
        // would add; adding it again would leave an orphan in the store on
        // every frame of a stable frame, which is a leak no test that only
        // counts batches would see. The upload's material value is therefore
        // *borrowed* here and only built by [`add_material`] in the branch that
        // actually adds one: a frame that reuses every batch copies a handle per
        // batch, not a whole `StandardMaterial` per batch.
        let material = match stored_material(world, entity, upload.material().kind()) {
            Some(current) => current,
            None => add_material(world, upload.material(), image.clone()),
        };
        world.entity_mut(entity).insert(BatchDraw {
            key: digest,
            phase: batch.phase(),
            image,
            instances: batch.instances().to_vec(),
        });
        set_material(world, entity, material.clone());
        // One placed entity per row the composed verdict draws. The batch key
        // covers the rows, so a reused entity's placements are the same rows;
        // they are only rebuilt when their count no longer matches, which
        // catches placements removed behind this path's back.
        report.placed += place_rows(world, entity, mesh, material, &drawn_rows);
        live.insert(key, entity);
    }

    // Everything the previous frame spawned and this frame does not claim is
    // released, so a reload leaves no stale geometry behind.
    for entity in previous.values() {
        release_entity(*entity, world, &mut report.released);
    }
    world.insert_resource(BatchEntities(live));
    report.presentation = apply_presentation(world, presentation, msaa);
    Ok(report)
}

/// The rows of `batch` the composed visibility verdict draws, counted.
///
/// Each row's decision is the composed verdict of the entity the live scene
/// owns for that row's part ([`row_draw`]), so LOD, damage and a playing clip
/// are ranked in exactly one place and this function only obeys it. Every row
/// the batch holds is counted — including the ones that are placed because no
/// record exists — so the report of what the renderer drew covers the whole
/// frame rather than only the interesting rows.
fn drawn_rows<'a>(
    world: &World,
    draws: &[SubmittedDraw<'a>],
    batch: &'a InstanceBatch,
    report: &mut VisibilityReport,
) -> Vec<&'a BatchInstance> {
    batch
        .instances()
        .iter()
        .filter(|row| {
            // Every row's submitted draw was resolved against this list above,
            // so the index the row carries is one this list has.
            let part = draws[row.item_index()].part;
            let decision = row_draw(world, part);
            report.record(decision);
            decision.drawn()
        })
        .collect()
}

/// The material handle one batch and its placements draw with.
///
/// A `Handle` cannot name which material *type* it is, but the ECS component
/// does: `MeshMaterial3d<AdditiveMaterial>` and `MeshMaterial3d<StandardMaterial>`
/// are different components, and a batch draws with whichever one its class's
/// material is. Carrying the kind alongside the handle is what lets the
/// placements below share one argument and one code path for both classes.
#[derive(Clone, Debug)]
enum BatchMaterial {
    /// A `StandardMaterial`, for opaque, masked, blended and emissive surfaces.
    Standard(Handle<StandardMaterial>),
    /// The additive class's own material, for the `One`/`One` blend.
    Additive(Handle<AdditiveMaterial>),
}

/// Adds `material` to the world, with the batch's image bound to it.
///
/// The image is bound here rather than in the caller because both material
/// types have a differently named field for it, and because the two are the
/// only difference between the branches: everything else the caller already
/// decided.
///
/// The value is cloned out of `material` here, in the one branch that needs an
/// owned value to hand to a store, so a frame that reuses every batch does not
/// pay for a material per batch.
fn add_material(
    world: &mut World,
    material: &DrawableMaterial,
    image: Option<Handle<Image>>,
) -> BatchMaterial {
    match material {
        DrawableMaterial::Standard(standard) => {
            let mut standard = standard.as_ref().clone();
            standard.base_color_texture = image;
            BatchMaterial::Standard(
                world
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(standard),
            )
        }
        DrawableMaterial::Additive(additive) => {
            let mut additive = additive.clone();
            additive.base_color_texture = image;
            BatchMaterial::Additive(
                world
                    .resource_mut::<Assets<AdditiveMaterial>>()
                    .add(additive),
            )
        }
    }
}

/// The material `entity` already draws with, when it draws with `kind`'s.
///
/// A handle of the wrong kind is not this batch's material: the batch key
/// digests the render state, which carries the class, so an entity found under
/// the key is drawing the right class's material and `None` means it is
/// missing or damaged.
fn stored_material(world: &World, entity: Entity, kind: MaterialKind) -> Option<BatchMaterial> {
    match kind {
        MaterialKind::Standard => world
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .map(|current| BatchMaterial::Standard(current.0.clone())),
        MaterialKind::Additive => world
            .get::<MeshMaterial3d<AdditiveMaterial>>(entity)
            .map(|current| BatchMaterial::Additive(current.0.clone())),
    }
}

/// Puts `material` on `entity` as the component its kind calls for.
fn set_material(world: &mut World, entity: Entity, material: BatchMaterial) {
    match material {
        BatchMaterial::Standard(handle) => {
            world
                .entity_mut(entity)
                .insert(MeshMaterial3d::<StandardMaterial>(handle));
        }
        BatchMaterial::Additive(handle) => {
            world
                .entity_mut(entity)
                .insert(MeshMaterial3d::<AdditiveMaterial>(handle));
        }
    }
}

/// The batch entity a previous frame tracked under `key`, if it can still serve
/// as this draw, and the mesh handle it already holds.
///
/// The batch key is the draw's identity, so an entity found under it is the same
/// draw. One that no longer carries the draw's components is not it: it is
/// released and a fresh entity is spawned rather than repaired in place, and a
/// key whose entity is already gone is simply dropped from the map.
fn reuse_batch(
    previous: &mut BTreeMap<[u8; 32], Entity>,
    key: [u8; 32],
    world: &mut World,
    released: &mut usize,
) -> Option<(Entity, Handle<Mesh>)> {
    let entity = previous.remove(&key)?;
    if world.get_entity(entity).is_err() {
        return None;
    }
    let usable = world
        .get_entity(entity)
        .is_ok_and(|found| found.contains::<BatchDraw>() && found.contains::<Mesh3d>());
    if !usable {
        release_entity(entity, world, released);
        return None;
    }
    let mesh = world
        .get::<Mesh3d>(entity)
        .expect("the entity carries a mesh")
        .0
        .clone();
    Some((entity, mesh))
}

/// Despawns `entity` and its placements, counting it, if it is still alive.
fn release_entity(entity: Entity, world: &mut World, released: &mut usize) {
    if world.get_entity(entity).is_ok() {
        *released += 1;
        // Recursive: the per-instance entities go with the batch.
        world.entity_mut(entity).despawn();
    }
}

/// Puts one placed entity per row under `batch`, and returns how many rows are
/// now placed.
///
/// `rows` are the rows the composed visibility verdict drew, so a row the
/// verdict withholds gets no placement — and a placement this call does not
/// claim again (because the verdict changed between two frames) is despawned
/// with the others, which is how a node a clip shows again comes back without a
/// stale draw being left on screen.
///
/// Each placement carries the row it came from and a translation of the row's
/// own `center_m`, so the *n* instances of a batch are *n* draws at *n* places
/// of one geometry and one material.
///
/// Placements are reconciled by the row's draw-item index, not rebuilt. The
/// batch key covers *which* instances a batch draws, not where they are, so a
/// frame in which an aircraft moved reuses the entity — and that row's placement
/// keeps its identity and takes the row's new place. Rebuilding all of them
/// would respawn every draw of a moving scene once per tick; never touching them
/// would leave a moved aircraft drawn where it used to be. A placement the frame
/// no longer claims is despawned.
fn place_rows(
    world: &mut World,
    batch: Entity,
    mesh: Handle<Mesh>,
    material: BatchMaterial,
    rows: &[&BatchInstance],
) -> usize {
    // The placements this batch already has, by the draw item each one is.
    let mut existing = BTreeMap::new();
    for child in world
        .get::<Children>(batch)
        .into_iter()
        .flatten()
        .copied()
        .filter(|child| {
            world
                .get_entity(*child)
                .is_ok_and(|found| found.contains::<BatchInstancePlacement>())
        })
    {
        if let Some(placement) = world.get::<BatchInstancePlacement>(child) {
            existing.insert(placement.row().item_index(), child);
        }
    }
    for row in rows {
        // Two rows of one batch cannot share a draw-item index, so at most one
        // placement is reused per row.
        let placement = BatchInstancePlacement {
            row: (*row).clone(),
        };
        let transform = Transform::from_translation(Vec3::from(row.center_m()));
        let child = match existing.remove(&row.item_index()) {
            Some(child) => {
                world
                    .entity_mut(child)
                    .insert((transform, Mesh3d(mesh.clone()), placement));
                child
            }
            None => world
                .spawn((ChildOf(batch), transform, Mesh3d(mesh.clone()), placement))
                .id(),
        };
        // The material goes on after the spawn, by kind: the component's own
        // type *is* the material, so one fixed `spawn` bundle would push every
        // class through the `StandardMaterial` type.
        set_material(world, child, material.clone());
    }
    // Whatever is left is a placement this frame does not claim.
    for child in existing.into_values() {
        world.entity_mut(child).despawn();
    }
    rows.len()
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
/// The despawn is recursive, so the per-instance entities under each batch go
/// with it; a teardown that left them would strand geometry in the world with
/// nothing tracking it, which is the one thing rule 3 above forbids.
///
/// A no-op when nothing is live, so a repeated teardown, a teardown after a
/// refused request and a teardown at shutdown are all safe.
pub fn teardown(world: &mut World) -> RenderTeardown {
    let mut report = RenderTeardown::default();
    if let Some(entities) = world.remove_resource::<BatchEntities>() {
        for entity in entities.0.values() {
            release_entity(*entity, world, &mut report.entities);
        }
    }
    report.sessions = world.remove_resource::<RenderSessionState>().is_some();
    report
}
