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
//!    unchanged — and, by rule 7, the store counts unchanged with it.
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
//!    This is also what makes rule 7 a gameplay-frequency rule rather than a rare
//!    one: a verdict that withholds *every* row of a batch releases that batch,
//!    and a verdict that starts drawing them again spawns it.
//! 7. **A released batch takes its assets back with it.** A batch entity
//!    *owns* the store entries this module added for it: the
//!    [`Assets<Mesh>`] entry the spawn uploaded, the material entry
//!    [`add_material`] created and the [`Assets<Image>`] entry that binds the
//!    batch's texture (the composed paint, or the canonical image). Its
//!    per-instance entities *borrow* the mesh and the material handles
//!    ([`BatchAssets`]). Releasing the batch despawns the entity and its
//!    placements first and only then hands each owned entry back to its store,
//!    and hands back only an entry no live batch and no live material entry
//!    still names ([`BatchAssetRefs`]), so one entry is returned once and a
//!    handle some live entity still draws with is left in place
//!    ([`FrameSync::reclaimed`]).
//!
//!    The image is the one entry that is named from **two** directions: it is
//!    shared — every spawn of the same paint fingerprint in one frame binds the
//!    one texture — and it is referenced from *inside* the material entry
//!    (`base_color_texture`), which Bevy's `Assets::remove` does not cascade.
//!    So an image goes back only when its owner count reaches zero *and* no
//!    material entry in either store still samples it, and the material of the
//!    batch being released is removed before that question is asked.
//!
//!    An entity that no longer carries the material component of its own kind
//!    is not reused at all: [`reuse_batch`] releases it through the same
//!    [`release_entity`] the stale batch goes through, and the frame spawns a
//!    fresh entity for the draw. A replacement entry is therefore always added
//!    by a spawn that records it, and a live owner record never names an entry
//!    its entity does not draw with.
//!
//!    Rules 3 and 7 together are what keep a frame path that releases and
//!    respawns a batch every other tick: rule 6's own release path. Without
//!    rule 7 that path would leave one mesh, one material and one image in
//!    their stores on each pass, the way a reused batch would leave one
//!    material per frame if [`add_material`] had no reuse guard either.
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

use bevy::asset::{AssetId, Assets, Handle};
use bevy::camera::visibility::Visibility;
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
///
/// `instances` is the **batch's** rows, which is not always the set that is on
/// screen: a row the composed visibility verdict withholds
/// ([`crate::render::visibility`]) keeps its per-instance state here and has
/// **no placement** under this entity. The record therefore answers "which
/// instances does this batch carry state for", and
/// [`FrameSync::visibility`] answers "which of them were drawn"; a row's
/// placement, not this list, is what the renderer draws.
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

    /// The batch's per-instance rows, in draw order.
    ///
    /// Every row the batcher produced, whether the composed visibility verdict
    /// drew it or not: this is the per-instance state record, and a withheld
    /// row's state is kept here even though it has no placement.
    pub fn instances(&self) -> &[BatchInstance] {
        &self.instances
    }

    /// The row of `instance`, when it is one of them.
    ///
    /// A row the composed visibility verdict withheld is still one of them —
    /// this answers which state the batch carries, not whether the row is on
    /// screen.
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

/// Component: the store entries this batch added, and is answerable for.
///
/// The ownership rule in one place. A batch entity **owns** the
/// [`Assets<Mesh>`] entry [`sync_frame`] added when it spawned the batch, the
/// material entry [`add_material`] created for it and the [`Assets<Image>`]
/// entry that binds its texture, and nothing else owns them: this module adds
/// all three and puts them on the entity that draws them. Its per-instance
/// entities **borrow** the mesh and material handles, and the batch's despawn
/// is recursive, so every borrow ends with the owner.
///
/// The image is recorded as an id beside the two, and it is `None` for a batch
/// that samples no image. It follows the same "recorded by the spawn that
/// added it" rule for a different reason from the other two: a shared image is
/// not the batch's alone (the same texture is bound by every spawn of that
/// paint fingerprint in a frame), so what the record gives the release path is
/// *one owner's name* to drop — [`BatchAssetRefs`] holds the count that says
/// whether any batch is left — plus the id to ask the material stores about.
///
/// Recorded on the entity rather than kept beside it because the release path
/// reads it *there*, before the despawn that takes it away: an entry is looked up
/// by the id its owner recorded, so a released batch hands back exactly the
/// entries it added and cannot hand back the same entry twice.
///
/// An entity found under a batch key that carries no such record owns nothing —
/// it was spawned by something else, or its record is gone — and a release leaves
/// the stores alone.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
struct BatchAssets {
    mesh: AssetId<Mesh>,
    material: OwnedMaterial,
    /// The image this spawn bound, when the batch samples one.
    image: Option<AssetId<Image>>,
}

/// The material store entry a batch owns.
///
/// Which store an id belongs to is a function of the batch's own render state,
/// and a `Handle` cannot say, so the id travels with its store: this is the
/// field that makes one release able to return both kinds of entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum OwnedMaterial {
    /// An `Assets<StandardMaterial>` entry, for the five non-additive classes.
    Standard(AssetId<StandardMaterial>),
    /// An `Assets<AdditiveMaterial>` entry, for the additive class.
    Additive(AssetId<AdditiveMaterial>),
}

/// Resource: how many live batch entities own each store entry this module
/// added.
///
/// Kept beside the entities because the two are changed together: an entry is
/// registered by the spawn that added it and dropped by the release that ended
/// its owner, so "no live batch names this entry any more" is a count that
/// reaches zero rather than a guess about what else in the world might still be
/// holding the handle. A release therefore drops one owner and returns an entry
/// only when that was the last one — which is what "a still-referenced handle is
/// not removed" means here.
///
/// One owner is the ordinary case for a mesh and a material, because a spawn
/// adds a fresh one of each for every batch it creates — but it is **not**
/// the case for an image: `paint_handles` hands one texture to every spawn of
/// the same paint fingerprint in a frame, and the canonical path could be made
/// to share the same way. The count is per id rather than per batch so that the
/// two-owner case is a decision the code makes rather than an assumption it
/// documents: the first release to end does not pull the entry out from under
/// the other live batch, and the image of a batch still drawing it is left in
/// its store.
///
/// The image count is necessary but not sufficient, and the difference is
/// exactly the case this task exists for: an image is also named from inside a
/// material entry (`base_color_texture`), which the owner count of *batches*
/// says nothing about. [`reclaim_store_entries`] therefore asks both questions
/// — no owner left **and** no material entry still sampling it — before an
/// image is removed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
struct BatchAssetRefs {
    meshes: BTreeMap<AssetId<Mesh>, usize>,
    materials: BTreeMap<OwnedMaterial, usize>,
    images: BTreeMap<AssetId<Image>, usize>,
}

impl BatchAssetRefs {
    /// Registers one new owner of the entries a spawn added.
    fn own(&mut self, owned: BatchAssets) {
        *self.meshes.entry(owned.mesh).or_default() += 1;
        *self.materials.entry(owned.material).or_default() += 1;
        if let Some(image) = owned.image {
            *self.images.entry(image).or_default() += 1;
        }
    }

    /// Drops one owner of a mesh entry and reports whether any owner is left.
    fn drop_mesh(&mut self, mesh: AssetId<Mesh>) -> bool {
        Self::drop_owner(&mut self.meshes, mesh)
    }

    /// Drops one owner of a material entry and reports whether any owner is left.
    fn drop_material(&mut self, material: OwnedMaterial) -> bool {
        Self::drop_owner(&mut self.materials, material)
    }

    /// Drops one owner of an image entry and reports whether any owner is left.
    ///
    /// Called only for a spawn that bound an image; the caller checks
    /// [`BatchAssets::image`] first, because an entry nobody registered is
    /// reported unowned and this map says nothing about a batch that samples
    /// no image at all.
    fn drop_image(&mut self, image: AssetId<Image>) -> bool {
        Self::drop_owner(&mut self.images, image)
    }

    /// Drops one owner and reports whether the entry is unowned.
    ///
    /// An entry nobody registered is reported unowned: nothing in this module
    /// names it, so a release that drops it removes one store entry and reports
    /// whether there was one to remove.
    fn drop_owner<K: Ord>(owners: &mut BTreeMap<K, usize>, key: K) -> bool {
        match owners.get_mut(&key) {
            Some(count) => {
                *count -= 1;
                if *count > 0 {
                    return false;
                }
                owners.remove(&key);
                true
            }
            None => true,
        }
    }
}

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

/// Store entries a sync or a teardown handed back to their stores.
///
/// Counted per store and only when a removal actually removed something, so a
/// second release of the same batch adds nothing here and the counts are the
/// number of entries that really left, not the number of release attempts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReclaimedAssets {
    /// `Assets<Mesh>` entries a released batch owned.
    pub meshes: usize,
    /// `Assets<StandardMaterial>` entries a released batch owned.
    pub materials: usize,
    /// `Assets<AdditiveMaterial>` entries a released batch owned.
    pub additive_materials: usize,
    /// `Assets<Image>` entries a released batch bound and that no live batch
    /// and no live material entry named any more.
    pub images: usize,
}

impl ReclaimedAssets {
    /// Folds another release's counts into this one's, so a sync or a teardown
    /// that releases several batches reports one total.
    fn absorb(&mut self, other: Self) {
        self.meshes += other.meshes;
        self.materials += other.materials;
        self.additive_materials += other.additive_materials;
        self.images += other.images;
    }
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
    /// Store entries the released batches owned and handed back: the other half
    /// of [`FrameSync::released`], and the count that keeps a frame path which
    /// releases and respawns a batch from growing a store every other tick —
    /// meshes, materials and the images the released batches bound.
    pub reclaimed: ReclaimedAssets,
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
    /// Store entries the despawned batches owned and handed back.
    pub reclaimed: ReclaimedAssets,
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
/// same mesh and material handles. The batch entity holds the handles and is
/// hidden; only the placements are drawn, so the entities a renderer draws are
/// exactly the placed rows. Every class has a drawable material, so
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
/// A batch this frame spawns records the mesh, material and image entries it
/// added ([`BatchAssets`]), and a batch it releases hands them back
/// ([`FrameSync::reclaimed`]) — the image only once no live batch owns it and
/// no material entry samples it. A frame that reuses every batch does neither,
/// so neither the reuse path nor the release path grows a store: one asset per
/// batch per frame would be a leak only a store count sees.
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
            // count in `released`, what that path hands back to the stores, and
            // the live map cannot disagree.
            continue;
        }
        let existing = reuse_batch(
            &mut previous,
            key,
            batch.material_kind(),
            world,
            &mut report.released,
            &mut report.reclaimed,
        );
        let reused = existing.is_some();
        // What a spawn added, recorded for the release that will end it. A
        // reused batch adds nothing and owns nothing new, so its own record —
        // already on the entity it reused — stays as it is.
        let mut added_mesh = None;
        let (entity, mesh) = match existing {
            Some(found) => {
                report.reused += 1;
                found
            }
            None => {
                let mesh = world
                    .resource_mut::<Assets<Mesh>>()
                    .add(upload.geometry().mesh().clone());
                added_mesh = Some(mesh.id());
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
        // batch, not a whole `StandardMaterial` per batch. The other half of
        // that same rule is [`release_entity`], which hands back what a spawn
        // added when the batch that owns it ends.
        let (material, added_material) =
            match stored_material(world, entity, upload.material().kind()) {
                Some(current) => (current, None),
                None => {
                    let (material, added) = add_material(world, upload.material(), image.clone());
                    (material, Some(added))
                }
            };
        // The owner record, for the batch that added the entries. Paired
        // rather than recorded field by field: a batch that owned half of what
        // it added would hand back one entry and leak the other, so an owner
        // record exists only when both the mesh and the material exist. The
        // image travels inside the same record because a spawn that bound one
        // is the spawn that added it — the `reused` branches above only read
        // the handle the entity already has — and a batch that samples no image
        // records `None`, which the release path leaves alone.
        if let Some(owned) = added_mesh
            .zip(added_material)
            .map(|(mesh, material)| BatchAssets {
                mesh,
                material,
                image: image.as_ref().map(|handle| handle.id()),
            })
        {
            world.entity_mut(entity).insert(owned);
            world
                .get_resource_or_insert_with(BatchAssetRefs::default)
                .own(owned);
        }
        // The batch entity holds the shared handles; it is not itself a draw.
        // Hidden on every pass, a reused entity included, so a batch can never
        // put one more copy of its geometry at the origin.
        world.entity_mut(entity).insert((
            BATCH_VISIBILITY,
            BatchDraw {
                key: digest,
                phase: batch.phase(),
                image,
                instances: batch.instances().to_vec(),
            },
        ));
        set_material(world, entity, material.clone());
        // One placed entity per row the composed verdict draws. The batch key
        // covers the rows, so a reused entity is the same batch; the
        // placements are reconciled by draw-item index, so a row the verdict
        // stops drawing loses its placement and a row it starts drawing gets
        // one, without respawning the ones that stayed.
        report.placed += place_rows(world, entity, mesh, material, &drawn_rows);
        live.insert(key, entity);
    }

    // Everything the previous frame spawned and this frame does not claim is
    // released, so a reload leaves no stale geometry behind — and no orphaned
    // mesh or material behind it either.
    for entity in previous.values() {
        let reclaimed = release_entity(*entity, world, &mut report.released);
        report.reclaimed.absorb(reclaimed);
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
/// pay for a material per batch. The id of the entry added is returned beside
/// the handle, because the caller is the one that knows what the batch owns and
/// the release that has to hand it back.
fn add_material(
    world: &mut World,
    material: &DrawableMaterial,
    image: Option<Handle<Image>>,
) -> (BatchMaterial, OwnedMaterial) {
    match material {
        DrawableMaterial::Standard(standard) => {
            let mut standard = standard.as_ref().clone();
            standard.base_color_texture = image;
            let handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(standard);
            let owned = OwnedMaterial::Standard(handle.id());
            (BatchMaterial::Standard(handle), owned)
        }
        DrawableMaterial::Additive(additive) => {
            let mut additive = additive.clone();
            additive.base_color_texture = image;
            let handle = world
                .resource_mut::<Assets<AdditiveMaterial>>()
                .add(additive);
            let owned = OwnedMaterial::Additive(handle.id());
            (BatchMaterial::Additive(handle), owned)
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
///
/// "The draw's components" is all of them, including the material of the batch's
/// own `kind`. The material is checked here for the reason [`BatchAssets`]
/// exists: this module *owns* the entry [`add_material`] adds, and a reused
/// entity that lost its material component would otherwise take a replacement
/// entry that no record names — an entry no release could hand back — while the
/// record it already carries kept naming the entry it no longer draws with.
/// Treating that entity as not usable routes it through [`release_entity`], so
/// the replacement is a spawn that records what it added and the stores do not
/// grow. `kind` is the batch's own class ([`InstanceBatch::material_kind`]),
/// which the key's state already covers, so an entity of the right kind is
/// checked for that kind's component.
///
/// A released entity here hands its own store entries back exactly like the
/// stale batch at the end of [`sync_frame`] does: both go through
/// [`release_entity`], so the repair path cannot be a second place a mesh or a
/// material is orphaned.
fn reuse_batch(
    previous: &mut BTreeMap<[u8; 32], Entity>,
    key: [u8; 32],
    kind: MaterialKind,
    world: &mut World,
    released: &mut usize,
    reclaimed: &mut ReclaimedAssets,
) -> Option<(Entity, Handle<Mesh>)> {
    let entity = previous.remove(&key)?;
    if world.get_entity(entity).is_err() {
        return None;
    }
    let usable = world
        .get_entity(entity)
        .is_ok_and(|found| found.contains::<BatchDraw>() && found.contains::<Mesh3d>())
        && stored_material(world, entity, kind).is_some();
    if !usable {
        reclaimed.absorb(release_entity(entity, world, released));
        return None;
    }
    let mesh = world
        .get::<Mesh3d>(entity)
        .expect("the entity carries a mesh")
        .0
        .clone();
    Some((entity, mesh))
}

/// Despawns `entity` and its placements, counting it, if it is still alive, and
/// returns the store entries it owned.
///
/// The order is the rule: the entries the entity owns are read first, then the
/// entity and every placement under it are despawned — which drops every
/// reference this module made to those entries, because a placement draws with
/// the batch's own handles — and only then is an unowned entry removed from its
/// store. Removing first would leave the placements holding a handle that no
/// longer resolves, which is a live entity drawing an asset that is gone.
///
/// Returns nothing for an entity that is already gone or owns no entries: the
/// entry is looked up by the id the owner recorded, so a second release of the
/// same batch finds no record and removes nothing.
fn release_entity(entity: Entity, world: &mut World, released: &mut usize) -> ReclaimedAssets {
    if world.get_entity(entity).is_err() {
        return ReclaimedAssets::default();
    }
    // Read before the despawn: the record goes with the entity.
    let owned = world.get::<BatchAssets>(entity).copied();
    *released += 1;
    // Recursive: the per-instance entities go with the batch.
    world.entity_mut(entity).despawn();
    owned.map_or_else(ReclaimedAssets::default, |owned| {
        reclaim_store_entries(world, owned)
    })
}

/// Hands a released batch's own entries back to the stores they came from.
///
/// [`BatchAssetRefs`] decides *whether* an entry goes back: this call drops one
/// owner of each, and an entry that still has an owner is left in place, because
/// a live batch is still drawing it. The removal is a lookup by the id the owner
/// recorded and is counted only when it removed something, so one entry is
/// returned once however many releases pass through here.
///
/// The image asks a second question, because it is the one entry named from
/// two directions: no live batch owns it **and** no material entry samples it
/// ([`material_binds_image`]). It is removed last, after this batch's own
/// material entry is out of the store — `Assets::remove` does not cascade, so
/// asking while the material is still there would keep every image forever.
///
/// A missing store is skipped rather than panicked on: a [`teardown`] of a world
/// that never had one is a no-op, the same way it despawns nothing when nothing
/// is live. [`sync_frame`] refuses a world without a store before it writes
/// anything ([`SyncError::NoAssetStore`]), so the only caller that can arrive
/// here without one is a teardown.
fn reclaim_store_entries(world: &mut World, owned: BatchAssets) -> ReclaimedAssets {
    // Scoped so the resource borrow ends before the stores are touched: the
    // counter and the assets are one transaction, not two overlapping ones.
    let (mesh_unowned, material_unowned, image_unowned) = {
        let mut refs = world.get_resource_or_insert_with(BatchAssetRefs::default);
        (
            refs.drop_mesh(owned.mesh),
            refs.drop_material(owned.material),
            owned.image.is_none_or(|image| refs.drop_image(image)),
        )
    };
    let mut reclaimed = ReclaimedAssets::default();
    if mesh_unowned && let Some(mut meshes) = world.get_resource_mut::<Assets<Mesh>>() {
        reclaimed.meshes = usize::from(meshes.remove(owned.mesh).is_some());
    }
    match owned.material {
        OwnedMaterial::Standard(id) => {
            if material_unowned
                && let Some(mut materials) = world.get_resource_mut::<Assets<StandardMaterial>>()
            {
                reclaimed.materials = usize::from(materials.remove(id).is_some());
            }
        }
        OwnedMaterial::Additive(id) => {
            if material_unowned
                && let Some(mut materials) = world.get_resource_mut::<Assets<AdditiveMaterial>>()
            {
                reclaimed.additive_materials = usize::from(materials.remove(id).is_some());
            }
        }
    }
    // Last, and only when both questions are answered: nobody owns it any
    // more, and — after this batch's material left the store above — no
    // material in either store still samples it. A batch whose surface samples
    // no image recorded `None` and never reaches this branch.
    if let Some(image) = owned.image
        && image_unowned
        && !material_binds_image(world, image)
        && let Some(mut images) = world.get_resource_mut::<Assets<Image>>()
    {
        reclaimed.images = usize::from(images.remove(image).is_some());
    }
    reclaimed
}

/// Whether any material entry in `world` still samples `image`.
///
/// The image a spawn added is referenced from *inside* a material entry
/// (`base_color_texture`), by the batch that added it and by any other batch
/// that bound the same texture — and Bevy's `Assets::remove` does not cascade,
/// so an image removed while a material still names it would leave that
/// material sampling a handle that no longer resolves. Both stores are read
/// because either class's material binds it, and every entry is read because
/// the question is about *the store*, not about this batch: the material of
/// the batch being released has to be removed before this is asked, which is
/// the order [`reclaim_store_entries`] holds.
fn material_binds_image(world: &World, image: AssetId<Image>) -> bool {
    let samples =
        |handle: Option<&Handle<Image>>| handle.is_some_and(|handle| handle.id() == image);
    world
        .get_resource::<Assets<StandardMaterial>>()
        .is_some_and(|materials| {
            materials
                .iter()
                .any(|(_, material)| samples(material.base_color_texture.as_ref()))
        })
        || world
            .get_resource::<Assets<AdditiveMaterial>>()
            .is_some_and(|materials| {
                materials
                    .iter()
                    .any(|(_, material)| samples(material.base_color_texture.as_ref()))
            })
}

/// The batch entity's visibility: never drawn.
///
/// A batch entity carries [`Mesh3d`] and its material because it is the
/// *owner* of the two handles its placements borrow ([`BatchAssets`]) and the
/// record the reuse path checks, not because it is a draw. An entity with a
/// mesh and Bevy's default visibility is rendered at its own transform, and a
/// batch entity's transform is the identity — so without this every batch
/// would draw one extra copy of its geometry at the world origin, on top of
/// its placed rows (Rally #506). The placements are the draws.
const BATCH_VISIBILITY: Visibility = Visibility::Hidden;

/// A placement's visibility: drawn, whatever its batch entity says.
///
/// `Visible` rather than the default `Inherited`, because the parent is
/// [`BATCH_VISIBILITY`] and a placement that inherited it would be hidden with
/// it. Whether a row is drawn at all is the composed visibility verdict's
/// decision (rule 6), made before a placement exists: a withheld row has no
/// placement to hide.
const PLACEMENT_VISIBILITY: Visibility = Visibility::Visible;

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
                world.entity_mut(child).insert((
                    transform,
                    PLACEMENT_VISIBILITY,
                    Mesh3d(mesh.clone()),
                    placement,
                ));
                child
            }
            None => world
                .spawn((
                    ChildOf(batch),
                    transform,
                    PLACEMENT_VISIBILITY,
                    Mesh3d(mesh.clone()),
                    placement,
                ))
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
/// nothing tracking it, which is the one thing rule 3 above forbids. Each
/// released entity also hands back the mesh, material and image entries it
/// added ([`RenderTeardown::reclaimed`]), so a session that ends leaves no
/// orphan in a store either.
///
/// A no-op when nothing is live, so a repeated teardown, a teardown after a
/// refused request and a teardown at shutdown are all safe. The owner counts in
/// [`BatchAssetRefs`] are left in place: they are the accounting for live
/// owners, every release above already dropped the one it held, and an owner that
/// a caller dropped without releasing would be a leak to report rather than a
/// count to reset behind.
pub fn teardown(world: &mut World) -> RenderTeardown {
    let mut report = RenderTeardown::default();
    if let Some(entities) = world.remove_resource::<BatchEntities>() {
        for entity in entities.0.values() {
            let reclaimed = release_entity(*entity, world, &mut report.entities);
            report.reclaimed.absorb(reclaimed);
        }
    }
    report.sessions = world.remove_resource::<RenderSessionState>().is_some();
    report
}

#[cfg(test)]
mod tests {
    use bevy::asset::Asset;

    use super::*;

    /// A store entry two live batches name is handed back by neither of them
    /// alone: the first release reports the entry as still owned and the second
    /// reports it free, so it leaves its store once and not before a live entity
    /// stopped drawing it.
    ///
    /// This is the branch of the release rule that a spawn cannot reach — a spawn
    /// adds a fresh mesh and a fresh material for every batch — so it is checked
    /// on the counter the release path itself uses, through the same
    /// [`BatchAssetRefs`] the production [`release_entity`] consults. A counter
    /// that reported "unowned" for the first of two releases would remove a
    /// handle a live entity still draws with.
    /// An asset id for the counter, never handed to an [`Assets`] store.
    ///
    /// Not [`AssetId::invalid`]: that one is documented as an id that must never
    /// be given to a store, and a fixture that leaned on it would be one edit
    /// away from doing exactly that.
    fn test_id<A: Asset>(nibble: u128) -> AssetId<A> {
        AssetId::Uuid {
            uuid: bevy::asset::uuid::Uuid::from_u128(nibble),
        }
    }

    #[test]
    fn accept_t512_a_store_entry_two_live_batches_name_is_handed_back_once() {
        let mesh = test_id::<Mesh>(1);
        let material = OwnedMaterial::Standard(test_id::<StandardMaterial>(2));
        let owned = BatchAssets {
            mesh,
            material,
            image: None,
        };
        let mut refs = BatchAssetRefs::default();

        refs.own(owned);
        refs.own(owned);
        assert_eq!(refs.meshes.get(&mesh), Some(&2));
        assert_eq!(refs.materials.get(&material), Some(&2));

        // The first owner to go leaves the entry alone: another live batch is
        // still drawing it.
        assert!(!refs.drop_mesh(mesh), "one owner of two is still an owner");
        assert!(
            !refs.drop_material(material),
            "one owner of two is still an owner"
        );

        // The second one frees it, and the entry stops being counted so a third
        // release finds nothing to drop.
        assert!(refs.drop_mesh(mesh));
        assert!(refs.drop_material(material));
        assert!(!refs.meshes.contains_key(&mesh));
        assert!(!refs.materials.contains_key(&material));
        assert!(refs.drop_mesh(mesh), "an entry nobody owns is unowned");
        assert!(refs.meshes.is_empty(), "and dropping it twice counts once");

        // The two material stores are counted apart, so dropping one standard
        // entry cannot free the additive store's.
        let other = OwnedMaterial::Standard(test_id::<StandardMaterial>(3));
        let additive = OwnedMaterial::Additive(test_id::<AdditiveMaterial>(4));
        refs.own(BatchAssets {
            mesh,
            material: additive,
            image: None,
        });
        refs.own(BatchAssets {
            mesh,
            material: other,
            image: None,
        });
        refs.own(BatchAssets {
            mesh,
            material: other,
            image: None,
        });
        assert!(
            !refs.drop_material(other),
            "one standard owner of two is still an owner"
        );
        assert_eq!(
            refs.materials.get(&additive),
            Some(&1),
            "dropping a standard entry did not touch the additive store's count"
        );
        assert!(refs.drop_material(other));
        assert!(refs.drop_material(additive));
        assert!(refs.materials.is_empty(), "both stores are empty again");
        assert_eq!(
            refs.meshes.get(&mesh),
            Some(&3),
            "the mesh owner count followed the three spawns"
        );
        assert!(!refs.drop_mesh(mesh), "two mesh owners are left");
        assert!(!refs.drop_mesh(mesh), "and then one");
        assert!(refs.drop_mesh(mesh));
        assert!(refs.meshes.is_empty());
    }

    /// An image a live material entry still samples stays in its store, even
    /// when no batch owns it any more — and it goes back once that material
    /// does.
    ///
    /// The owner count answers "does a live batch bind this texture"; the
    /// material stores answer a different question only they can answer, because
    /// the image is named from *inside* `base_color_texture` and Bevy's
    /// `Assets::remove` does not cascade. `sync_frame` cannot reach the case
    /// today — every spawn that binds an image records an owner for it, so the
    /// count and the store agree — which is why this is checked on the release
    /// path itself, against a material entry this module never added: the shape a
    /// change that shares one texture between a material and an unrecorded
    /// binding would produce, and the reason the removal asks both questions in
    /// the order it does (the batch's own material first, then the image).
    #[test]
    fn accept_t514_an_image_a_live_material_entry_still_samples_stays_in_its_store() {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

        /// A one-texel texture, big enough to be a valid image.
        fn texture() -> Image {
            Image::new(
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                vec![1, 2, 3, 4],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            )
        }

        let mut world = World::new();
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<StandardMaterial>::default());
        world.insert_resource(Assets::<AdditiveMaterial>::default());

        // The texture, a material *another* batch draws with that samples it,
        // and the batch's own material, which samples nothing.
        let image = world.resource_mut::<Assets<Image>>().add(texture());
        let foreign = {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            materials.add(StandardMaterial {
                base_color_texture: Some(image.clone()),
                ..StandardMaterial::default()
            })
        };
        let own = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        // A mesh id for the record, never stored: the mesh is not what this
        // case is about, and `AssetId::Uuid` ids remove as a lookup that misses.
        let owned = BatchAssets {
            mesh: test_id::<Mesh>(9),
            material: OwnedMaterial::Standard(own.id()),
            image: Some(image.id()),
        };
        world
            .get_resource_or_insert_with(BatchAssetRefs::default)
            .own(owned);

        // The last owner goes, so the count alone would call the texture free —
        // and the texture must stay, because a live material still samples it.
        // The store is checked first: it is the observable fact, the report is
        // its account.
        let first = reclaim_store_entries(&mut world, owned);
        assert_eq!(
            world.resource::<Assets<Image>>().len(),
            1,
            "the image a live material samples is still in its store"
        );
        assert_eq!(
            first,
            ReclaimedAssets {
                meshes: 0,
                materials: 1,
                additive_materials: 0,
                images: 0,
            },
            "the batch's own material went back; the sampled texture did not"
        );
        assert!(
            world
                .resource::<Assets<StandardMaterial>>()
                .get(foreign.id())
                .is_some(),
            "the material that samples it is untouched"
        );

        // Once that material is gone too, nothing names the texture and the
        // release hands it back — a second pass over the same record, because
        // the counter already reported the entry unowned.
        world
            .resource_mut::<Assets<StandardMaterial>>()
            .remove(foreign.id());
        let second = reclaim_store_entries(&mut world, owned);
        assert!(
            world.resource::<Assets<Image>>().is_empty(),
            "and the store gives it back exactly once"
        );
        assert_eq!(
            second,
            ReclaimedAssets {
                meshes: 0,
                materials: 0,
                additive_materials: 0,
                images: 1,
            },
            "no batch owns it and no material samples it any more"
        );
    }
}
