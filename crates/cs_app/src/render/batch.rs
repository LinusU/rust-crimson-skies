//! Instance batching: the draw plan grouped into the batches that share their
//! GPU resources, with every instance's paint and damage kept per instance
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-C`, AC03; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! ## Why batching can lose an instance
//!
//! F17 non-negotiable 4: "Instancing and batching retain per-instance livery
//! and damage state." Batching is where that is easy to lose: a batch is one
//! draw, so a batch that merges two aircraft painted differently, or two
//! aircraft with a different part destroyed, *renders one of them wrongly* —
//! the other aircraft's paint or its missing wing silently becomes the first
//! aircraft's. Nothing downstream can see the mistake, because the geometry,
//! the state and the image are genuinely shared.
//!
//! So the batch key is not just the shared resources. It is
//!
//! * the **phase**, so no batch straddles two passes;
//! * the **geometry**, **render state** and **image** digests F17-B's
//!   adapters produced, so nothing merges across a different buffer, a
//!   different blend/alpha/cull decision or a different texture;
//! * the **committed livery** digest, read from the F09-C
//!   [`crate::livery::ModelLivery`] the instance is bound to — two paints are
//!   two variants, so two batches;
//!
//! and one rule that is stronger than a key: an instance whose paint is
//! **not established** is never merged with anything, not even with another
//! unresolved instance. Two unresolved paints may be different, so merging them
//! would invent a match; the item is drawn in a batch of its own and the gap
//! is reported in [`BatchedFrame::limitations`].
//!
//! Damage is not a key at all, because a destroyed part is not drawn: the
//! item is withheld with [`withheld_codes::DESTROYED_PART`] and its identity
//! stays in the frame's report. Withholding a *draw* touches no collider —
//! gameplay and collision read the canonical scene, not this frame — which is
//! F17 non-negotiable 4's other half.
//!
//! ## Order is never traded for a batch
//!
//! Batches are built by walking the plan in draw order and merging only
//! *consecutive* entries with an equal key. Two items with the same key that
//! another item sits between are two batches: reordering them to merge would
//! change the order F17-A established and F17 non-negotiable 1 preserves.
//!
//! ## What stays unknown
//!
//! Two things this stage cannot know, both reported rather than assumed:
//!
//! * an item whose **part identity** is unresolved cannot be checked against
//!   damage, so it is drawn (an unestablished destruction is not a
//!   destruction) and reported as
//!   [`limitation_codes::UNRESOLVED_PART_IDENTITY`];
//! * an item whose **instance has no visual record** has no established
//!   paint, so it is drawn unbatched and reported as
//!   [`limitation_codes::UNBOUND_INSTANCE`].
//!
//! Every class has a drawable material, the additive one included
//! ([`crate::render::additive::AdditiveMaterial`]), so a batch always has one
//! and the frame records *which* kind each batch draws with. F17-B and F17-C
//! recorded the additive class as a material gap; that gap is closed in
//! `docs/findings/2026-09-30-f17-c-followup-additive-material.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_assets::install::sha256;
use cs_content::scene::SceneNodeId;
use cs_types::Tick;
use cs_types::evidence::ContentHash;

use crate::livery::{LiveryError, LiveryRuntime, LiverySession, ModelInstanceId, ModelLivery};
use crate::render::bevy_state::MaterialKind;
use crate::render::capture::{SceneOutcome, SurfaceRefusal, SurfaceUpload, scene_codes};
use crate::render::material::RenderPhase;
use crate::render::plan::{DrawItem, DrawItemKey, DrawPlan};
use crate::render::profile::{ProfileParity, RenderProfile};
use crate::scene::AirframeDamageState;

/// The visual state one model instance contributes to a frame: the paint it is
/// committed to and the parts of it that are destroyed.
///
/// The paint comes from the F09-C livery runtime, which composes a variant per
/// distinct source-and-paint pair and hands back a deterministic
/// [`cs_content::livery::LiveryVariantKey`]; its digest *is* the instance's
/// paint identity for batching. The damage comes from the F11-C
/// [`AirframeDamageState`], whose entries are stable part identities.
///
/// The per-instance split is this stage's design: F11-C records the damage of
/// the one live airframe, and a frame that draws several model instances needs
/// the same record per instance. Nothing here decides *whether* a part is
/// destroyed — that is the gameplay decision F29 owns and F11-C reflects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceVisual {
    instance: ModelInstanceId,
    livery: Option<ContentHash>,
    destroyed: BTreeSet<SceneNodeId>,
}

impl InstanceVisual {
    /// An instance committed to a paint: its livery digest is established.
    pub fn bound(instance: ModelInstanceId, livery: &ModelLivery) -> Self {
        Self {
            instance,
            livery: Some(livery.key().digest()),
            destroyed: BTreeSet::new(),
        }
    }

    /// An instance with **no established paint**. Its items are drawn, never
    /// merged with anything, and the gap is reported.
    pub fn unbound(instance: ModelInstanceId) -> Self {
        Self {
            instance,
            livery: None,
            destroyed: BTreeSet::new(),
        }
    }

    /// The same instance with the recorded damage of `damage` copied in.
    pub fn with_damage(mut self, damage: &AirframeDamageState) -> Self {
        self.destroyed = damage.destroyed().cloned().collect();
        self
    }

    /// The instance this state belongs to.
    pub const fn instance(&self) -> ModelInstanceId {
        self.instance
    }

    /// The committed paint's variant digest, `None` when the paint is not
    /// established.
    pub const fn livery(&self) -> Option<ContentHash> {
        self.livery
    }

    /// Whether this instance's `part` is recorded as destroyed.
    pub fn is_destroyed(&self, part: &SceneNodeId) -> bool {
        self.destroyed.contains(part)
    }

    /// How many of this instance's parts are recorded as destroyed.
    pub fn destroyed_count(&self) -> usize {
        self.destroyed.len()
    }
}

/// The per-frame set of model instances, by stable instance identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstanceVisuals(BTreeMap<ModelInstanceId, InstanceVisual>);

impl InstanceVisuals {
    /// An empty set.
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Binds `instance` to the paint it is committed to, reading the record
    /// out of the F09-C livery runtime.
    ///
    /// An instance the runtime has no binding for is recorded as
    /// [`InstanceVisual::unbound`] rather than skipped, so a forgotten
    /// binding shows up in the frame's limitations.
    ///
    /// # Errors
    ///
    /// [`LiveryError::ForeignSession`](crate::livery::LiveryError::ForeignSession)
    /// when `session` did not open `runtime`, and nothing is recorded: a paint
    /// bound for a finished session is never served, and the caller retries
    /// with the open session.
    pub fn bind(
        &mut self,
        runtime: &LiveryRuntime,
        session: LiverySession,
        instance: ModelInstanceId,
        damage: &AirframeDamageState,
    ) -> Result<(), LiveryError> {
        runtime.require_session(session)?;
        let visual = match runtime.livery(instance) {
            Some(livery) => InstanceVisual::bound(instance, livery),
            None => InstanceVisual::unbound(instance),
        };
        self.insert(visual.with_damage(damage));
        Ok(())
    }

    /// Records one instance's visual state, replacing any previous record for
    /// the same instance.
    pub fn insert(&mut self, visual: InstanceVisual) -> Option<InstanceVisual> {
        self.0.insert(visual.instance(), visual)
    }

    /// The record for `instance`, when the frame has one.
    pub fn get(&self, instance: ModelInstanceId) -> Option<&InstanceVisual> {
        self.0.get(&instance)
    }

    /// How many instances the frame has a record for.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the frame has no instance records.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Which part of its instance one submitted draw item draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartRef<'a> {
    /// The part identity the item draws. Damage is checked against it.
    Known(&'a SceneNodeId),
    /// The part identity is not established, with the reason code that says
    /// why. Damage cannot be checked, so the item is drawn and the gap is
    /// reported.
    Unresolved(&'static str),
}

impl PartRef<'_> {
    /// The part identity, when it is established.
    pub const fn known(&self) -> Option<&SceneNodeId> {
        match self {
            Self::Known(node) => Some(node),
            Self::Unresolved(_) => None,
        }
    }
}

/// What the renderer is handed for one submitted draw item: the item, the
/// outcome F17-B's adapters produced for it, and the instance and part it
/// draws.
#[derive(Clone, Copy, Debug)]
pub struct SubmittedDraw<'a> {
    /// The submitted item.
    pub item: &'a DrawItem,
    /// Its upload outcome: an upload or a refusal.
    pub outcome: &'a SceneOutcome,
    /// The model instance this item draws for.
    pub instance: ModelInstanceId,
    /// Which part of that instance it draws.
    pub part: PartRef<'a>,
}

/// The shared resources one batch draws with, and the per-instance identity
/// that must match for two items to share them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchKey {
    phase: RenderPhase,
    geometry: ContentHash,
    state: ContentHash,
    image: Option<ContentHash>,
    livery: Option<ContentHash>,
}

impl BatchKey {
    /// The pass this batch draws in.
    pub const fn phase(&self) -> RenderPhase {
        self.phase
    }

    /// The digest of the uploaded geometry every instance shares.
    pub const fn geometry(&self) -> ContentHash {
        self.geometry
    }

    /// The digest of the render state every instance shares.
    pub const fn state(&self) -> ContentHash {
        self.state
    }

    /// The digest of the canonical image every instance samples, when the
    /// batch has one.
    pub const fn image(&self) -> Option<ContentHash> {
        self.image
    }

    /// The committed paint every instance in this batch shares. `None` is an
    /// unresolved paint, and such a batch holds exactly one item — see the
    /// module docs.
    pub const fn livery(&self) -> Option<ContentHash> {
        self.livery
    }

    /// Whether `upload` is the surface this key was computed from.
    ///
    /// The three shared resources are what the key digests, so a consumer can
    /// check in one call that the upload it is about to bind is the one the
    /// batch recorded. A different surface's geometry, render state or image at
    /// the same draw index is a stale submitted-draw list, not a batch to draw:
    /// binding it would put one surface's buffers under another surface's
    /// identity and report a success.
    ///
    /// The paint is not checked here: it comes from the instance record, not
    /// from the surface upload.
    pub fn matches(&self, upload: &SurfaceUpload) -> bool {
        self.geometry == upload.geometry().fingerprint()
            && self.state == upload.state().fingerprint()
            && self.image == upload.image().map(|image| image.fingerprint())
    }
}

/// One instance's row inside a batch: its own identity, its own place and its
/// own paint, never folded into the batch's shared resources.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchInstance {
    item_index: usize,
    item: DrawItemKey,
    instance: ModelInstanceId,
    center_m: [f32; 3],
    depth_m_bits: u32,
}

impl BatchInstance {
    /// Index into the submitted-draw list, the row's stable identity for this
    /// frame.
    pub const fn item_index(&self) -> usize {
        self.item_index
    }

    /// The draw item this row draws.
    pub const fn item(&self) -> &DrawItemKey {
        &self.item
    }

    /// The model instance this row draws for.
    pub const fn instance(&self) -> ModelInstanceId {
        self.instance
    }

    /// The item's scene position in meters.
    pub const fn center_m(&self) -> [f32; 3] {
        self.center_m
    }

    /// The view depth the plan's sort used for this row.
    pub fn depth_m(&self) -> f32 {
        f32::from_bits(self.depth_m_bits)
    }
}

/// One batched draw: the resources every instance in it shares, and the rows
/// that share them.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceBatch {
    key: BatchKey,
    instances: Vec<BatchInstance>,
    material_kind: MaterialKind,
    mergeable: bool,
}

impl InstanceBatch {
    /// The shared resources and the paint identity.
    pub const fn key(&self) -> &BatchKey {
        &self.key
    }

    /// The pass this batch draws in.
    pub const fn phase(&self) -> RenderPhase {
        self.key.phase
    }

    /// The rows, in draw order.
    pub fn instances(&self) -> &[BatchInstance] {
        &self.instances
    }

    /// How many instances this one draw covers.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Whether this draw covers no instance, which cannot happen: a batch is
    /// only built from drawn items.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Whether this batch may be extended by the next item with the same key.
    ///
    /// `false` for a batch whose paint is not established: such a batch holds
    /// exactly one row, because two unresolved paints may be different and
    /// merging them would draw one of them with the other's.
    pub const fn mergeable(&self) -> bool {
        self.mergeable
    }

    /// Which drawable material this batch draws with.
    ///
    /// Every class has one, so this is *which* and never "none": the additive
    /// class draws with its own material
    /// ([`crate::render::additive::AdditiveMaterial`]) and every other class
    /// with a `StandardMaterial`.
    pub const fn material_kind(&self) -> MaterialKind {
        self.material_kind
    }

    /// The row of `instance` in this batch, when it is one of them.
    pub fn row(&self, instance: ModelInstanceId) -> Option<&BatchInstance> {
        self.instances.iter().find(|row| row.instance() == instance)
    }
}

/// The reason codes a withheld draw carries.
pub mod withheld_codes {
    /// The instance's damage record names this item's part as destroyed, so the
    /// part is not drawn. Its identity stays in the frame's report, and no
    /// collider is touched: collision reads the canonical scene.
    pub const DESTROYED_PART: &str = "destroyed_part";
    /// The item is not drawn because F17-B's adapters refused it; the
    /// refusal's own reason codes follow this one.
    pub const REFUSED: &str = "refused";
}

/// The reason codes a batching limitation carries.
pub mod limitation_codes {
    /// The item's part identity is not established, so its damage could not be
    /// checked. It is drawn, because an unestablished destruction is not a
    /// destruction.
    pub const UNRESOLVED_PART_IDENTITY: &str = "unresolved_part_identity";
    /// The item names a model instance the frame has no visual record for, so
    /// its paint is not established. It is drawn on its own, never merged with
    /// another instance.
    pub const UNBOUND_INSTANCE: &str = "unbound_instance";
}

/// A draw item that did not reach the renderer, and why.
///
/// Withheld is a *report*, not a deletion: the item keeps its key and its
/// instance, so a gameplay, collision or evidence consumer can see that a part
/// exists and is not being drawn — F17 non-negotiable 4 forbids a visual
/// decision that silently removes a gameplay object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithheldDraw {
    item: DrawItemKey,
    instance: ModelInstanceId,
    reasons: Vec<&'static str>,
}

impl WithheldDraw {
    /// The draw item that was not drawn.
    pub const fn item(&self) -> &DrawItemKey {
        &self.item
    }

    /// The instance it would have drawn for.
    pub const fn instance(&self) -> ModelInstanceId {
        self.instance
    }

    /// The reason codes, in order: [`withheld_codes::REFUSED`] first for a
    /// surface the adapters refused, then that refusal's own codes.
    pub fn reasons(&self) -> &[&'static str] {
        &self.reasons
    }
}

/// A way this frame's batching is known to be limited.
///
/// Every variant is a case where the batcher could not establish something
/// and said so instead of assuming it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchingLimitation {
    /// The item's part identity is not established, so its damage could not
    /// be checked. It was drawn.
    UnresolvedPartIdentity {
        /// The draw item.
        item: DrawItemKey,
        /// Why the part identity is not established.
        reason: &'static str,
    },
    /// The item names an instance with no visual record, so its paint is not
    /// established. It was drawn in a batch of its own.
    UnboundInstance {
        /// The draw item.
        item: DrawItemKey,
        /// The instance that has no record.
        instance: ModelInstanceId,
    },
}

impl BatchingLimitation {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnresolvedPartIdentity { .. } => limitation_codes::UNRESOLVED_PART_IDENTITY,
            Self::UnboundInstance { .. } => limitation_codes::UNBOUND_INSTANCE,
        }
    }
}

impl fmt::Display for BatchingLimitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnresolvedPartIdentity { item, reason } => {
                write!(f, "{item}: its part identity is unresolved ({reason})")
            }
            Self::UnboundInstance { item, instance } => {
                write!(f, "{item}: {instance} has no visual record")
            }
        }
    }
}

/// Why a frame could not be batched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchError {
    /// The submitted draws and the draw plan disagree about which items exist.
    SceneIncomplete {
        /// A stable code for the kind of disagreement, from
        /// [`crate::render::capture::scene_codes`].
        code: &'static str,
        /// The draw item it is about.
        key: DrawItemKey,
    },
    /// A submitted draw's key is not the plan entry's key at that index.
    KeyMismatch {
        /// Index into the submitted-draw list.
        index: usize,
        /// The key the draw item carries.
        drawn: DrawItemKey,
        /// The key the plan's entry at that index names.
        planned: DrawItemKey,
    },
    /// A submitted draw no plan entry reaches, so it is not this frame.
    UnusedDraw {
        /// Index into the submitted-draw list.
        index: usize,
        /// The key the draw item carries.
        key: DrawItemKey,
    },
}

impl BatchError {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SceneIncomplete { code, .. } => code,
            Self::KeyMismatch { .. } => "submitted_draw_key_mismatch",
            Self::UnusedDraw { .. } => "submitted_draw_without_plan_entry",
        }
    }
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SceneIncomplete { code, key } => {
                write!(
                    f,
                    "the submitted draws do not match the plan at {key}: {code}"
                )
            }
            Self::KeyMismatch {
                index,
                drawn,
                planned,
            } => write!(
                f,
                "submitted draw {index} is {drawn} but the plan's entry at that index is {planned}"
            ),
            Self::UnusedDraw { index, key } => {
                write!(f, "submitted draw {index} ({key}) is in no plan entry")
            }
        }
    }
}

impl std::error::Error for BatchError {}

/// One tick's frame, batched.
///
/// The frame is the ordered, per-instance record the ECS consumer syncs: the
/// batches in draw order, the draws that were withheld with their reasons, the
/// batching limitations, the tick, and the profile it was built under. Its
/// [`Self::fingerprint`] covers all of it, so a comparison can prove two
/// frames drew the same instances the same way under the same profile.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchedFrame {
    profile: ProfileParity,
    profile_fingerprint: ContentHash,
    tick: Tick,
    batches: Vec<InstanceBatch>,
    withheld: Vec<WithheldDraw>,
    limitations: Vec<BatchingLimitation>,
    fingerprint: ContentHash,
}

/// Batches one tick's submitted draws under `profile`.
///
/// `draws` is the scene in **submission order**: draw `i` belongs to the plan
/// entry with `item == i`, and the batcher refuses to proceed when the two
/// disagree ([`BatchError`]) — the same completeness contract F17-B's capture
/// holds, so a stale list cannot be batched into a frame that looks complete.
///
/// # Errors
///
/// [`BatchError`] when `draws` and `plan` do not describe the same items.
pub fn batch_frame(
    draws: &[SubmittedDraw<'_>],
    plan: &DrawPlan,
    visuals: &InstanceVisuals,
    profile: &RenderProfile,
    tick: Tick,
) -> Result<BatchedFrame, BatchError> {
    let mut reached = vec![false; draws.len()];
    let mut batches: Vec<InstanceBatch> = Vec::new();
    let mut withheld: Vec<WithheldDraw> = Vec::new();
    let mut limitations: Vec<BatchingLimitation> = Vec::new();
    // The batch being filled, flushed whenever the key changes. Nothing is
    // reordered: only consecutive equal keys merge.
    let mut open: Option<InstanceBatch> = None;

    for entry in plan.entries() {
        let index = entry.item;
        let submitted = draws.get(index).ok_or(BatchError::SceneIncomplete {
            code: scene_codes::MISSING_OUTCOME,
            key: entry.key.clone(),
        })?;
        if submitted.item.key() != &entry.key || submitted.outcome.key() != &entry.key {
            return Err(BatchError::KeyMismatch {
                index,
                drawn: submitted.item.key().clone(),
                planned: entry.key.clone(),
            });
        }
        reached[index] = true;

        let instance = submitted.instance;
        let part = submitted.part;
        let item = entry.key.clone();
        match submitted.outcome {
            SceneOutcome::Refused(SurfaceRefusal { .. }) => {
                flush(&mut open, &mut batches);
                let mut reasons = vec![withheld_codes::REFUSED];
                if let SceneOutcome::Refused(refusal) = submitted.outcome {
                    reasons.extend(refusal.reasons().iter().copied());
                }
                withheld.push(WithheldDraw {
                    item,
                    instance,
                    reasons,
                });
            }
            SceneOutcome::Uploaded(upload) => {
                // The one record lookup per row. An instance with no record and
                // an instance whose paint is not established are the same case
                // for batching — there is no paint to keep apart — and both are
                // reported rather than assumed.
                let visual = visuals.get(instance);
                let livery = visual.and_then(InstanceVisual::livery);
                if livery.is_none() {
                    limitations.push(BatchingLimitation::UnboundInstance {
                        item: item.clone(),
                        instance,
                    });
                }
                let damaged = match (visual, part) {
                    (Some(visual), _) => visual_damage(visual, part, &item, &mut limitations),
                    // No record means no damage record either: the same gap is
                    // already reported above.
                    (None, _) => false,
                };
                if damaged {
                    flush(&mut open, &mut batches);
                    withheld.push(WithheldDraw {
                        item,
                        instance,
                        reasons: vec![withheld_codes::DESTROYED_PART],
                    });
                    continue;
                }
                let key = BatchKey {
                    phase: entry.phase,
                    geometry: upload.geometry().fingerprint(),
                    state: upload.state().fingerprint(),
                    image: upload.image().map(|image| image.fingerprint()),
                    livery,
                };
                let row = BatchInstance {
                    item_index: index,
                    item,
                    instance,
                    center_m: submitted.item.center_m(),
                    depth_m_bits: entry.depth_m.to_bits(),
                };
                let material_kind = upload.material_kind();
                // A batch whose paint is not established holds one row and
                // never merges: two unresolved paints may be different, so
                // merging them would invent a match.
                let mergeable = livery.is_some();
                match open.as_mut() {
                    Some(batch)
                        if batch.key == key
                            && batch.material_kind == material_kind
                            && batch.mergeable =>
                    {
                        batch.instances.push(row);
                    }
                    _ => {
                        flush(&mut open, &mut batches);
                        open = Some(InstanceBatch {
                            key,
                            instances: vec![row],
                            material_kind,
                            mergeable,
                        });
                    }
                }
            }
        }
    }
    flush(&mut open, &mut batches);

    if let Some(index) = reached.iter().position(|reached| !reached) {
        return Err(BatchError::UnusedDraw {
            index,
            key: draws[index].item.key().clone(),
        });
    }

    // Ordered by draw item key, so the report of what did not draw is the same
    // list whichever order the plan happened to draw the rest in.
    withheld.sort_by(|a, b| a.item.cmp(&b.item));

    let mut frame = BatchedFrame {
        profile: profile.parity(),
        profile_fingerprint: profile.fingerprint(),
        tick,
        batches,
        withheld,
        limitations,
        fingerprint: ContentHash::from_bytes([0; 32]),
    };
    frame.fingerprint = frame_fingerprint(&frame);
    Ok(frame)
}

/// Whether `visual`'s damage record removes the part one row draws, and
/// reports the rows whose part identity could not be checked.
fn visual_damage(
    visual: &InstanceVisual,
    part: PartRef<'_>,
    item: &DrawItemKey,
    limitations: &mut Vec<BatchingLimitation>,
) -> bool {
    match part {
        PartRef::Known(node) => visual.is_destroyed(node),
        PartRef::Unresolved(reason) => {
            limitations.push(BatchingLimitation::UnresolvedPartIdentity {
                item: item.clone(),
                reason,
            });
            false
        }
    }
}

fn flush(open: &mut Option<InstanceBatch>, batches: &mut Vec<InstanceBatch>) {
    if let Some(batch) = open.take() {
        batches.push(batch);
    }
}

fn frame_fingerprint(frame: &BatchedFrame) -> ContentHash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"cs/render/batched_frame/v1\0");
    bytes.extend_from_slice(frame.profile.code().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(frame.profile_fingerprint.as_bytes());
    bytes.extend_from_slice(&frame.tick.0.to_le_bytes());
    bytes.extend_from_slice(&(frame.batches.len() as u32).to_le_bytes());
    for batch in &frame.batches {
        bytes.extend_from_slice(batch.key.phase.code().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(batch.key.geometry.as_bytes());
        bytes.extend_from_slice(batch.key.state.as_bytes());
        push_optional_hash(&mut bytes, batch.key.image);
        push_optional_hash(&mut bytes, batch.key.livery);
        // `material_kind` is deliberately not digested: it is a function of the
        // class, and `batch.key.state` above digests the render state, which
        // digests the class. A separate byte here would be redundant rather than
        // load-bearing — the acceptance selection
        // `accept_f17_c_additive_the_reported_material_kind_follows_the_state_class`
        // pins the reported kind to the class instead.
        bytes.push(u8::from(batch.mergeable));
        bytes.extend_from_slice(&(batch.instances.len() as u32).to_le_bytes());
        for row in &batch.instances {
            bytes.extend_from_slice(&(row.item_index as u32).to_le_bytes());
            bytes.extend_from_slice(row.item.as_str().as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(&row.instance.0.to_le_bytes());
            bytes.extend_from_slice(&row.depth_m_bits.to_le_bytes());
            for value in row.center_m {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
    }
    bytes.extend_from_slice(&(frame.withheld.len() as u32).to_le_bytes());
    for withheld in &frame.withheld {
        bytes.extend_from_slice(withheld.item.as_str().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&withheld.instance.0.to_le_bytes());
        bytes.push(0);
        for reason in &withheld.reasons {
            bytes.extend_from_slice(reason.as_bytes());
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(&(frame.limitations.len() as u32).to_le_bytes());
    for limitation in &frame.limitations {
        bytes.extend_from_slice(limitation.code().as_bytes());
        bytes.push(0);
        match limitation {
            BatchingLimitation::UnresolvedPartIdentity { item, reason } => {
                bytes.extend_from_slice(item.as_str().as_bytes());
                bytes.push(0);
                bytes.extend_from_slice(reason.as_bytes());
                bytes.push(0);
            }
            BatchingLimitation::UnboundInstance { item, instance } => {
                bytes.extend_from_slice(item.as_str().as_bytes());
                bytes.push(0);
                bytes.extend_from_slice(&instance.0.to_le_bytes());
            }
        }
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

impl BatchedFrame {
    /// What this frame may be used as evidence for, from the profile it was
    /// built under.
    pub const fn profile(&self) -> ProfileParity {
        self.profile
    }

    /// The digest of the profile this frame was built under.
    pub const fn profile_fingerprint(&self) -> ContentHash {
        self.profile_fingerprint
    }

    /// The tick this frame is.
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The batches, in draw order.
    pub fn batches(&self) -> &[InstanceBatch] {
        &self.batches
    }

    /// How many draws this frame is: the batches, each covering one or more
    /// instances.
    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    /// How many instance rows the batches cover in total.
    pub fn instance_count(&self) -> usize {
        self.batches.iter().map(InstanceBatch::len).sum()
    }

    /// The batch `instance` appears in, when the frame draws it at all.
    pub fn batch_of(&self, instance: ModelInstanceId) -> Option<&InstanceBatch> {
        self.batches
            .iter()
            .find(|batch| batch.row(instance).is_some())
    }

    /// The draws that did not reach the renderer, ordered by draw item key.
    pub fn withheld(&self) -> &[WithheldDraw] {
        &self.withheld
    }

    /// Whether `instance` has a row in this frame.
    pub fn draws(&self, instance: ModelInstanceId) -> bool {
        self.batch_of(instance).is_some()
    }

    /// The batching limitations, in plan order.
    pub fn limitations(&self) -> &[BatchingLimitation] {
        &self.limitations
    }

    /// A digest of everything above: the profile, the tick, every batch's
    /// shared resources and per-instance rows in draw order, the withheld
    /// draws with their reasons and the limitations.
    ///
    /// Two frames of the same tick and camera are equal exactly when this
    /// digest matches, so a comparison sees a changed paint, a destroyed part
    /// or a changed profile as a different frame.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }
}
