//! Model instances and the construction preview: wiring faction and custom
//! paints onto aircraft (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`,
//! stage `### F09-C`; contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! Stage F09-A read the BM planes ([`cs_formats::bm`]); F09-B composed them
//! into one RGB8 image and produced its deterministic variant key
//! ([`cs_content::livery::compose_livery`]); F09-C adds the F09-B module
//! left open — "session selection, per-instance variants, the cache map
//! itself and the construction preview":
//!
//! * **the producer — the content variant store.**
//!   [`cs_content::livery::LiveryVariantStore`] caches composed images keyed
//!   by every input that distinguishes them (source fingerprint, the three
//!   colors, the algorithm version). [`LiveryRuntime`] owns one store per
//!   session and never composes the same variant twice.
//! * **the consumer — model instances.** A [`ModelInstanceId`] identifies one
//!   aircraft in a scene; a [`ModelLivery`] is its committed paint. Two
//!   instances that share one source image but choose different faction
//!   colors resolve to different keys and different stored bytes, and binding
//!   or previewing one instance never mutates another (spec non-negotiable
//!   #5).
//! * **the construction preview.** [`LiveryRuntime::preview`] composes a
//!   *candidate* paint for an already-bound instance and hands back a
//!   [`ConstructionPreview`] without changing the committed livery;
//!   [`LiveryRuntime::commit`] applies it. A preview that is never committed
//!   leaves the instance exactly as it was.
//! * **teardown, retry and stale state.** A [`LiverySession`] stamps the
//!   runtime; every compose, preview and commit refuses a foreign session, so
//!   a livery bound for one mission is never served after a switch. A failed
//!   composition stores nothing and leaves the instance untouched, so the
//!   caller can retry with a budget that fits. [`LiveryRuntime::release`] and
//!   [`LiveryRuntime::evict_unreferenced`] drop one instance's binding and
//!   the variants no bound instance still references; [`LiveryRuntime::teardown`]
//!   ends the session and reports what it dropped.
//!
//! The paint colors themselves are caller data, never a baked catalog: the
//! authoritative faction palettes and valid combinations come from original
//! data (spec non-negotiable #4, F09-D). A [`PaintChoice::Faction`] pairs a
//! faction catalog id with the colors a caller supplies for it, so this stage
//! can be wired and tested without inventing a palette. Findings and the
//! recorded unknowns: `docs/findings/2026-09-29-f09-c-model-instances-and-construction-preview.md`.
//!
//! This module is Bevy-free data and lifecycle: a rendering stage (F17-B)
//! drives it from a system, and the GPU adapter consumes the
//! [`cs_content::livery::ComposedLivery`] the runtime resolves. It performs no
//! color-space conversion, no decal interpretation and no per-instance
//! mutation of a shared image.
//!
//! A worked bind-and-read example is the unit test `module_doc_example_binds_and_reads_an_image`
//! in this file. It is a unit test, not a doctest, because a doctest links its
//! own binary against the whole Bevy rlib set, which dies with an lld bus error on CI.

use std::collections::{HashMap, HashSet};
use std::fmt;

use cs_content::livery::{ComposedLivery, LiveryPaint, LiveryVariantKey, LiveryVariantStore};
use cs_formats::bm::{BmError, BmFile};
use cs_formats::io::AllocationBudget;
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;

/// Monotonic generation of one livery session.
///
/// A new mission, a mission retry or a world switch opens a new generation
/// (`docs/01-ARCHITECTURE.md`, "Application state and lifecycle"). A runtime
/// stamped with one generation refuses every call that names another, so a
/// livery bound for a finished mission can never be served after the switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LiverySession(pub u64);

impl fmt::Display for LiverySession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "livery-session#{}", self.0)
    }
}

/// Stable identity of one model instance: one aircraft in one scene.
///
/// This is a new-engine runtime identity, not a source numeric id and not a
/// catalog index; the binding it names is disposable and is replaced, not
/// recycled across sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModelInstanceId(pub u64);

impl fmt::Display for ModelInstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "model-instance#{}", self.0)
    }
}

/// Which faction or custom scheme a model instance paints with.
///
/// The colors are supplied by the caller ([`LiveryPaint`]); this stage bakes
/// in no palette, because which colors a faction really uses comes from
/// original data (spec non-negotiable #4, F09-D). [`Self::Faction`] carries a
/// faction catalog id (namespace `faction`, `ContentKind::Faction`) so the
/// choice is traceable to a catalog element once one exists.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PaintChoice {
    /// A faction's stock paint: the catalog id of the faction, and the colors
    /// a caller supplies for it.
    Faction {
        /// The faction catalog element.
        faction: ContentId,
        /// The three mask colors, in mask-plane order.
        paint: LiveryPaint,
    },
    /// A player-authored custom paint.
    Custom(LiveryPaint),
}

impl PaintChoice {
    /// A faction paint choice.
    pub fn faction(faction: ContentId, paint: LiveryPaint) -> Self {
        Self::Faction { faction, paint }
    }

    /// A custom paint choice.
    pub fn custom(paint: LiveryPaint) -> Self {
        Self::Custom(paint)
    }

    /// The three colors, in mask-plane order.
    pub fn paint(&self) -> &LiveryPaint {
        match self {
            Self::Faction { paint, .. } | Self::Custom(paint) => paint,
        }
    }

    /// The faction catalog id, for a faction choice.
    pub fn faction_id(&self) -> Option<&ContentId> {
        match self {
            Self::Faction { faction, .. } => Some(faction),
            Self::Custom(_) => None,
        }
    }

    /// Stable lowercase origin label: `"faction"` or `"custom"`.
    pub const fn origin(&self) -> &'static str {
        match self {
            Self::Faction { .. } => "faction",
            Self::Custom(_) => "custom",
        }
    }
}

/// One model instance's committed livery: its paint choice and the variant
/// key of the composed image it resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelLivery {
    instance: ModelInstanceId,
    choice: PaintChoice,
    key: LiveryVariantKey,
}

impl ModelLivery {
    /// The instance this livery belongs to.
    pub fn instance(&self) -> ModelInstanceId {
        self.instance
    }

    /// The committed paint choice.
    pub fn choice(&self) -> &PaintChoice {
        &self.choice
    }

    /// The key of the composed variant the instance resolves to.
    pub fn key(&self) -> &LiveryVariantKey {
        &self.key
    }

    /// The fingerprint of the source image this livery was composed from.
    pub fn source(&self) -> &ContentHash {
        self.key.source()
    }
}

/// A candidate paint composed for one model instance but not yet committed.
///
/// Produced by [`LiveryRuntime::preview`]; its variant is already stored, so
/// [`LiveryRuntime::composed`] can read the previewed bytes, but the
/// instance's committed [`ModelLivery`] is unchanged until
/// [`LiveryRuntime::commit`] is called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionPreview {
    session: LiverySession,
    instance: ModelInstanceId,
    choice: PaintChoice,
    key: LiveryVariantKey,
}

impl ConstructionPreview {
    /// The session that composed the preview.
    pub fn session(&self) -> LiverySession {
        self.session
    }

    /// The instance the preview is for.
    pub fn instance(&self) -> ModelInstanceId {
        self.instance
    }

    /// The candidate paint choice.
    pub fn choice(&self) -> &PaintChoice {
        &self.choice
    }

    /// The key of the candidate variant.
    pub fn key(&self) -> &LiveryVariantKey {
        &self.key
    }
}

/// What [`LiveryRuntime::teardown`] released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveryTeardown {
    /// Instance bindings dropped.
    pub instances: usize,
    /// Composed variants dropped.
    pub variants: usize,
}

/// Why a livery request could not be served.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveryError {
    /// The call named a session that did not open this runtime.
    ForeignSession {
        /// The session that opened the runtime.
        runtime: LiverySession,
        /// The session the caller named.
        session: LiverySession,
    },
    /// The model instance is not bound in this runtime.
    UnknownInstance {
        /// The instance asked for.
        instance: ModelInstanceId,
    },
    /// A preview's variant is not in this runtime's store, so it cannot be
    /// committed here (it came from another runtime, or its variant was
    /// evicted).
    UnknownVariant {
        /// The variant asked for.
        key: LiveryVariantKey,
    },
    /// Composition failed; nothing was stored or bound.
    Compose(BmError),
}

impl LiveryError {
    /// Stable lowercase identifier for logs and diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ForeignSession { .. } => "foreign_session",
            Self::UnknownInstance { .. } => "unknown_instance",
            Self::UnknownVariant { .. } => "unknown_variant",
            Self::Compose(error) => error.code(),
        }
    }
}

impl fmt::Display for LiveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { runtime, session } => write!(
                f,
                "the livery runtime belongs to {runtime} and cannot serve {session}"
            ),
            Self::UnknownInstance { instance } => {
                write!(f, "{instance} is not bound in this livery runtime")
            }
            Self::UnknownVariant { key } => write!(
                f,
                "no composed variant {:?} is stored in this livery runtime",
                key.digest()
            ),
            Self::Compose(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LiveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compose(error) => Some(error),
            Self::ForeignSession { .. }
            | Self::UnknownInstance { .. }
            | Self::UnknownVariant { .. } => None,
        }
    }
}

impl From<BmError> for LiveryError {
    fn from(error: BmError) -> Self {
        Self::Compose(error)
    }
}

/// One session's model-instance liveries and the composed-variant store they
/// resolve through.
///
/// The runtime is the F09-C consumer of [`cs_content::livery`]: it composes
/// each distinct variant once, binds model instances to variants and offers
/// the construction preview. It stores every variant it composes; a variant
/// is released only by [`Self::evict_unreferenced`], [`Self::teardown`] or a
/// fresh runtime, never by switching a paint.
#[derive(Debug)]
pub struct LiveryRuntime {
    session: LiverySession,
    variants: LiveryVariantStore,
    instances: HashMap<ModelInstanceId, ModelLivery>,
}

impl LiveryRuntime {
    /// A runtime for one session, with no bindings and no variants.
    pub fn new(session: LiverySession) -> Self {
        Self {
            session,
            variants: LiveryVariantStore::new(),
            instances: HashMap::new(),
        }
    }

    /// The session that opened this runtime.
    pub fn session(&self) -> LiverySession {
        self.session
    }

    /// How many model instances are bound.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// How many composed variants are stored.
    pub fn variant_count(&self) -> usize {
        self.variants.len()
    }

    /// Refuses a session that did not open this runtime.
    ///
    /// # Errors
    ///
    /// [`LiveryError::ForeignSession`] when the sessions differ.
    pub fn require_session(&self, session: LiverySession) -> Result<(), LiveryError> {
        if session == self.session {
            Ok(())
        } else {
            Err(LiveryError::ForeignSession {
                runtime: self.session,
                session,
            })
        }
    }

    /// The committed livery of `instance`, if it is bound.
    pub fn livery(&self, instance: ModelInstanceId) -> Option<&ModelLivery> {
        self.instances.get(&instance)
    }

    /// The stored variant under `key`, if this runtime composed it.
    pub fn composed(&self, key: &LiveryVariantKey) -> Option<&ComposedLivery> {
        self.variants.get(key)
    }

    /// The composed image the bound `instance` resolves to, if it is bound.
    pub fn image(&self, instance: ModelInstanceId) -> Option<&ComposedLivery> {
        let livery = self.instances.get(&instance)?;
        self.variants.get(livery.key())
    }

    /// Composes `file` with `choice` if needed and binds `instance` to it.
    ///
    /// A source image already composed with a different paint is a different
    /// variant: both stay stored and this call changes only `instance`. A
    /// composition that fails stores nothing and leaves any previous binding
    /// of `instance` exactly as it was, so the caller can retry with a budget
    /// that fits.
    ///
    /// # Errors
    ///
    /// [`LiveryError::ForeignSession`] for another session, and
    /// [`LiveryError::Compose`] when the composition fails.
    pub fn bind(
        &mut self,
        session: LiverySession,
        instance: ModelInstanceId,
        file: &BmFile<'_>,
        choice: PaintChoice,
        budget: &mut AllocationBudget,
    ) -> Result<&ModelLivery, LiveryError> {
        self.require_session(session)?;
        let key = *self.variants.compose(file, choice.paint(), budget)?.key();
        self.instances.insert(
            instance,
            ModelLivery {
                instance,
                choice,
                key,
            },
        );
        Ok(self
            .instances
            .get(&instance)
            .expect("the livery was just inserted"))
    }

    /// Composes a candidate paint for the bound `instance` and returns it as a
    /// preview, leaving the committed livery untouched.
    ///
    /// The candidate variant is stored, so the previewed bytes are readable
    /// through [`Self::composed`] before any commit.
    ///
    /// # Errors
    ///
    /// [`LiveryError::ForeignSession`] for another session,
    /// [`LiveryError::UnknownInstance`] when `instance` is not bound, and
    /// [`LiveryError::Compose`] when the candidate composition fails (the
    /// committed livery is untouched in every case).
    pub fn preview(
        &mut self,
        session: LiverySession,
        instance: ModelInstanceId,
        file: &BmFile<'_>,
        candidate: PaintChoice,
        budget: &mut AllocationBudget,
    ) -> Result<ConstructionPreview, LiveryError> {
        self.require_session(session)?;
        if !self.instances.contains_key(&instance) {
            return Err(LiveryError::UnknownInstance { instance });
        }
        let key = *self
            .variants
            .compose(file, candidate.paint(), budget)?
            .key();
        Ok(ConstructionPreview {
            session: self.session,
            instance,
            choice: candidate,
            key,
        })
    }

    /// Applies `preview` as the committed livery of its instance.
    ///
    /// # Errors
    ///
    /// [`LiveryError::ForeignSession`] when the preview or the call belongs to
    /// another session, [`LiveryError::UnknownInstance`] when the instance is
    /// no longer bound, and [`LiveryError::UnknownVariant`] when the preview's
    /// variant is not stored here.
    pub fn commit(
        &mut self,
        session: LiverySession,
        preview: &ConstructionPreview,
    ) -> Result<&ModelLivery, LiveryError> {
        self.require_session(session)?;
        if preview.session != self.session {
            return Err(LiveryError::ForeignSession {
                runtime: self.session,
                session: preview.session,
            });
        }
        if !self.instances.contains_key(&preview.instance) {
            return Err(LiveryError::UnknownInstance {
                instance: preview.instance,
            });
        }
        if self.variants.get(&preview.key).is_none() {
            return Err(LiveryError::UnknownVariant { key: preview.key });
        }
        self.instances.insert(
            preview.instance,
            ModelLivery {
                instance: preview.instance,
                choice: preview.choice.clone(),
                key: preview.key,
            },
        );
        Ok(self
            .instances
            .get(&preview.instance)
            .expect("the livery was just inserted"))
    }

    /// Unbinds `instance`, returning its livery. The variants it referenced
    /// stay stored until [`Self::evict_unreferenced`] or
    /// [`Self::teardown`].
    pub fn release(&mut self, instance: ModelInstanceId) -> Option<ModelLivery> {
        self.instances.remove(&instance)
    }

    /// Drops every stored variant no bound instance references and returns how
    /// many were dropped.
    pub fn evict_unreferenced(&mut self) -> usize {
        let referenced: HashSet<LiveryVariantKey> =
            self.instances.values().map(|livery| livery.key).collect();
        self.variants.retain(|key| referenced.contains(key))
    }

    /// Ends the session: drops every binding and every variant and reports the
    /// counts.
    pub fn teardown(&mut self) -> LiveryTeardown {
        let instances = self.instances.len();
        self.instances.clear();
        LiveryTeardown {
            instances,
            variants: self.variants.clear(),
        }
    }
}

/// Acceptance stage F09-C. Every fixture is newly authored synthetic bytes
/// built here; nothing is derived from original game data. These tests call
/// the production [`cs_content::livery`] path through [`LiveryRuntime`].
#[cfg(test)]
mod tests {
    use cs_content::livery::compose_livery;
    use cs_formats::bm::PaintColor;
    use cs_formats::{ParseContext, read_bm};
    use cs_types::content::ContentKind;

    use super::*;

    /// The bind-and-read example from the module docs, kept as a unit test.
    #[test]
    fn module_doc_example_binds_and_reads_an_image() {
        use cs_formats::io::AllocationBudget;
        use cs_types::content::{ContentId, ContentKind};

        // A 1x1 BM: header is height, then width; base, three masks, overlay.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u16.to_le_bytes()); // height
        bytes.extend_from_slice(&1u16.to_le_bytes()); // width
        bytes.extend_from_slice(&[10, 20, 30]); // base
        bytes.extend_from_slice(&[255, 255, 255]); // masks 1..3
        bytes.extend_from_slice(&[0, 0, 0, 0]); // overlay
        let mut context = ParseContext::with_defaults("synthetic/f09-c.bm");
        let file = read_bm(&mut context, &bytes).expect("the image is valid");

        let mut runtime = LiveryRuntime::new(LiverySession(1));
        let faction = ContentId::from_source(ContentKind::Faction, "red").expect("valid id");
        let red = LiveryPaint::new([
            PaintColor::new(255, 0, 0),
            PaintColor::WHITE,
            PaintColor::WHITE,
        ]);
        let mut budget = AllocationBudget::with_defaults("synthetic/f09-c.bm");
        let bound = runtime
            .bind(
                LiverySession(1),
                ModelInstanceId(1),
                &file,
                PaintChoice::faction(faction, red),
                &mut budget,
            )
            .expect("the paint fits")
            .clone();
        let image = runtime
            .image(ModelInstanceId(1))
            .expect("the instance is bound");
        assert_eq!(image.rgb().len(), 3);
        assert_eq!(bound.key().colors()[0], PaintColor::new(255, 0, 0));
    }

    const RED: PaintColor = PaintColor::new(255, 0, 0);
    const BLUE: PaintColor = PaintColor::new(0, 0, 255);
    const PAINT_X: PaintColor = PaintColor::new(200, 100, 50);

    /// One faction's paint: a distinct first-plane color so two factions
    /// over the same source really differ.
    const RED_PAINT: LiveryPaint = LiveryPaint::new([RED, PAINT_X, PaintColor::WHITE]);
    const BLUE_PAINT: LiveryPaint = LiveryPaint::new([BLUE, PAINT_X, PaintColor::WHITE]);

    const BASE: [[u8; 3]; 4] = [[10, 20, 30], [40, 50, 60], [70, 80, 90], [100, 110, 120]];

    /// A 2x2 BM, header height then width, planes in stored order. A full mask
    /// on every plane makes the paint colors change the composed bytes.
    fn build_masks(masks: [u8; 3]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u16.to_le_bytes()); // height
        bytes.extend_from_slice(&2u16.to_le_bytes()); // width
        for texel in BASE {
            bytes.extend_from_slice(&texel);
        }
        for mask in masks {
            bytes.extend_from_slice(&[mask; 4]);
        }
        bytes.extend_from_slice(&[0u8; 16]); // transparent overlay
        bytes
    }

    /// The shared source: one image both planes are built from.
    fn shared_source() -> Vec<u8> {
        build_masks([255, 255, 255])
    }

    fn parse(bytes: &[u8]) -> BmFile<'_> {
        let mut context = ParseContext::with_defaults("synthetic/f09-c.bm");
        read_bm(&mut context, bytes).expect("the synthetic image parses")
    }

    fn budget() -> AllocationBudget {
        AllocationBudget::with_defaults("synthetic/f09-c.bm")
    }

    fn faction(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Faction, key).expect("a valid faction id")
    }

    /// The independently computed bytes for a source and paint, so a runtime
    /// result can be checked against the production composition path rather
    /// than against the runtime's own output.
    fn expected_rgb(file: &BmFile<'_>, paint: &LiveryPaint) -> Vec<u8> {
        compose_livery(file, paint, &mut budget())
            .expect("the expected composition fits")
            .rgb()
            .to_vec()
    }

    /// AC03: two planes share one source image, choose different faction
    /// colors, and neither one's bytes or key change when the other is bound
    /// or repainted.
    #[test]
    fn accept_f09_c_two_planes_share_source_but_keep_their_faction_colors() {
        let bytes = shared_source();
        let file = parse(&bytes);
        let mut runtime = LiveryRuntime::new(LiverySession(7));
        let plane_a = ModelInstanceId(1);
        let plane_b = ModelInstanceId(2);

        let a = runtime
            .bind(
                LiverySession(7),
                plane_a,
                &file,
                PaintChoice::faction(faction("red"), RED_PAINT),
                &mut budget(),
            )
            .expect("red fits")
            .clone();

        let b = runtime
            .bind(
                LiverySession(7),
                plane_b,
                &file,
                PaintChoice::faction(faction("blue"), BLUE_PAINT),
                &mut budget(),
            )
            .expect("blue fits")
            .clone();

        // Same source, different paint: two variants, two images.
        assert_eq!(runtime.instance_count(), 2);
        assert_eq!(runtime.variant_count(), 2, "both faction variants are kept");
        assert_eq!(a.source(), b.source(), "the planes share one source image");
        assert_ne!(a.key(), b.key(), "different factions, different variants");
        assert_ne!(
            runtime.image(plane_a).expect("A").rgb(),
            runtime.image(plane_b).expect("B").rgb(),
            "the two faction colors must not cross-contaminate"
        );
        // Each plane's bytes are exactly the independent composition of that
        // plane's paint over the shared source.
        assert_eq!(
            runtime.image(plane_a).expect("A").rgb(),
            expected_rgb(&file, &RED_PAINT).as_slice()
        );
        assert_eq!(
            runtime.image(plane_b).expect("B").rgb(),
            expected_rgb(&file, &BLUE_PAINT).as_slice()
        );
        assert_eq!(a.choice().origin(), "faction");
        assert_eq!(a.choice().faction_id(), Some(&faction("red")));

        // Repainting A changes only A: B keeps its bytes and key.
        runtime
            .bind(
                LiverySession(7),
                plane_a,
                &file,
                PaintChoice::custom(BLUE_PAINT),
                &mut budget(),
            )
            .expect("the custom paint fits");
        assert_eq!(
            runtime.livery(plane_a).expect("A").choice().origin(),
            "custom"
        );
        assert_eq!(runtime.livery(plane_a).expect("A").key(), b.key());
        assert_eq!(
            runtime.image(plane_b).expect("B").rgb(),
            expected_rgb(&file, &BLUE_PAINT).as_slice(),
            "repainting A must not mutate B"
        );
        // A custom paint equal to a faction's colors reuses the same variant.
        assert_eq!(runtime.variant_count(), 2);
        assert_eq!(
            runtime.image(plane_a).expect("A").rgb(),
            expected_rgb(&file, &BLUE_PAINT).as_slice(),
            "A now resolves to the blue variant"
        );
    }

    /// The construction preview shows a candidate without changing the
    /// committed livery; committing applies it.
    #[test]
    fn accept_f09_c_construction_preview_does_not_touch_the_committed_livery() {
        let bytes = shared_source();
        let file = parse(&bytes);
        let mut runtime = LiveryRuntime::new(LiverySession(3));
        let plane = ModelInstanceId(10);
        let committed = runtime
            .bind(
                LiverySession(3),
                plane,
                &file,
                PaintChoice::faction(faction("red"), RED_PAINT),
                &mut budget(),
            )
            .expect("red fits")
            .clone();
        let red_rgb = runtime.image(plane).expect("bound").rgb().to_vec();

        let preview = runtime
            .preview(
                LiverySession(3),
                plane,
                &file,
                PaintChoice::faction(faction("blue"), BLUE_PAINT),
                &mut budget(),
            )
            .expect("the preview composes");
        assert_eq!(preview.instance(), plane);
        assert_eq!(preview.session(), LiverySession(3));
        assert_eq!(preview.choice().origin(), "faction");
        assert_ne!(preview.key(), committed.key());
        // The candidate's bytes are readable and differ from the committed
        // ones, but the committed livery is untouched.
        assert_eq!(
            runtime.composed(preview.key()).expect("stored").rgb(),
            expected_rgb(&file, &BLUE_PAINT).as_slice()
        );
        assert_eq!(runtime.livery(plane).expect("bound").key(), committed.key());
        assert_eq!(
            runtime.image(plane).expect("bound").rgb(),
            red_rgb.as_slice()
        );

        // Committing applies the candidate and keeps both variants.
        let committed = runtime
            .commit(LiverySession(3), &preview)
            .expect("commit")
            .clone();
        assert_eq!(committed.key(), preview.key());
        assert_eq!(
            runtime.image(plane).expect("bound").rgb(),
            expected_rgb(&file, &BLUE_PAINT).as_slice()
        );
        assert_eq!(runtime.variant_count(), 2, "the red variant is not evicted");
    }

    /// Teardown and eviction: releasing one plane drops only the variants no
    /// other plane references; teardown drops everything.
    #[test]
    fn accept_f09_c_release_and_teardown_drop_only_unreferenced_variants() {
        let bytes = shared_source();
        let file = parse(&bytes);
        let mut runtime = LiveryRuntime::new(LiverySession(4));
        let plane_a = ModelInstanceId(1);
        let plane_b = ModelInstanceId(2);
        runtime
            .bind(
                LiverySession(4),
                plane_a,
                &file,
                PaintChoice::custom(RED_PAINT),
                &mut budget(),
            )
            .expect("A fits");
        runtime
            .bind(
                LiverySession(4),
                plane_b,
                &file,
                PaintChoice::custom(BLUE_PAINT),
                &mut budget(),
            )
            .expect("B fits");
        assert_eq!(runtime.variant_count(), 2);

        let released = runtime.release(plane_b).expect("B was bound");
        assert_eq!(released.instance(), plane_b);
        assert!(runtime.livery(plane_b).is_none());
        assert_eq!(runtime.release(plane_b), None, "releasing twice is a no-op");
        assert_eq!(
            runtime.variant_count(),
            2,
            "a release does not evict on its own"
        );

        assert_eq!(runtime.evict_unreferenced(), 1);
        assert_eq!(runtime.variant_count(), 1, "only A's variant remains");
        assert!(runtime.image(plane_a).is_some());

        let teardown = runtime.teardown();
        assert_eq!(teardown.instances, 1);
        assert_eq!(teardown.variants, 1);
        assert_eq!(runtime.instance_count(), 0);
        assert_eq!(runtime.variant_count(), 0);
        assert!(runtime.livery(plane_a).is_none());
    }

    /// Error propagation and retry: a refused composition binds nothing and
    /// leaves the instance untouched, a foreign session is refused, and a
    /// retry with a fitting budget succeeds.
    #[test]
    fn accept_f09_c_failed_composition_leaves_no_stale_state_and_retry_succeeds() {
        let bytes = shared_source();
        let file = parse(&bytes);
        let mut runtime = LiveryRuntime::new(LiverySession(9));
        let plane = ModelInstanceId(1);

        // 2x2 RGB8 needs 12 bytes; 5 is refused.
        let mut tiny = AllocationBudget::new("synthetic/f09-c.bm", 5);
        let error = runtime
            .bind(
                LiverySession(9),
                plane,
                &file,
                PaintChoice::custom(RED_PAINT),
                &mut tiny,
            )
            .expect_err("the composition does not fit");
        assert_eq!(error.code(), "allocation_budget_exceeded");
        assert_eq!(tiny.used(), 0, "a refused reservation allocates nothing");
        assert!(runtime.livery(plane).is_none(), "nothing was bound");
        assert_eq!(runtime.instance_count(), 0);
        assert_eq!(runtime.variant_count(), 0, "nothing was stored");

        // Retry with a sufficient budget succeeds from a clean state.
        runtime
            .bind(
                LiverySession(9),
                plane,
                &file,
                PaintChoice::custom(RED_PAINT),
                &mut budget(),
            )
            .expect("the retry fits");
        assert_eq!(runtime.instance_count(), 1);
        assert_eq!(runtime.variant_count(), 1);

        // A foreign session is refused by every entry point and changes
        // nothing.
        let before = runtime.image(plane).expect("bound").rgb().to_vec();
        assert_eq!(
            runtime
                .bind(
                    LiverySession(99),
                    plane,
                    &file,
                    PaintChoice::custom(BLUE_PAINT),
                    &mut budget(),
                )
                .expect_err("another session is refused")
                .code(),
            "foreign_session"
        );
        assert_eq!(
            runtime
                .preview(
                    LiverySession(99),
                    plane,
                    &file,
                    PaintChoice::custom(BLUE_PAINT),
                    &mut budget(),
                )
                .expect_err("another session is refused")
                .code(),
            "foreign_session"
        );
        runtime
            .require_session(LiverySession(99))
            .expect_err("foreign session");
        assert_eq!(runtime.image(plane).expect("bound").rgb(), before);
        assert_eq!(runtime.variant_count(), 1);

        // An unknown instance cannot be previewed.
        assert_eq!(
            runtime
                .preview(
                    LiverySession(9),
                    ModelInstanceId(404),
                    &file,
                    PaintChoice::custom(BLUE_PAINT),
                    &mut budget(),
                )
                .expect_err("unknown instance")
                .code(),
            "unknown_instance"
        );
    }

    /// A preview is bound to the runtime's store: committing one whose variant
    /// this runtime never composed is refused, and a preview of an instance
    /// released in the meantime cannot be committed.
    #[test]
    fn accept_f09_c_commit_refuses_a_preview_without_its_variant_or_instance() {
        let bytes = shared_source();
        let file = parse(&bytes);
        let mut runtime = LiveryRuntime::new(LiverySession(5));
        let plane = ModelInstanceId(1);
        runtime
            .bind(
                LiverySession(5),
                plane,
                &file,
                PaintChoice::custom(RED_PAINT),
                &mut budget(),
            )
            .expect("red fits");
        let preview = runtime
            .preview(
                LiverySession(5),
                plane,
                &file,
                PaintChoice::custom(BLUE_PAINT),
                &mut budget(),
            )
            .expect("a preview composes");

        // A second runtime of the same session number did not compose the
        // previewed variant, so it refuses the preview instead of binding a
        // key it cannot resolve to bytes.
        let mut other = LiveryRuntime::new(LiverySession(5));
        other
            .bind(
                LiverySession(5),
                plane,
                &file,
                PaintChoice::custom(RED_PAINT),
                &mut budget(),
            )
            .expect("red fits");
        assert_eq!(
            other
                .commit(LiverySession(5), &preview)
                .expect_err("the variant is not in the other store")
                .code(),
            "unknown_variant"
        );

        // Releasing the instance before the commit refuses it.
        runtime.release(plane);
        assert_eq!(
            runtime
                .commit(LiverySession(5), &preview)
                .expect_err("the instance is gone")
                .code(),
            "unknown_instance"
        );
        // The refused commits bound nothing.
        assert_eq!(
            other.livery(plane).expect("still red").key().colors()[0],
            RED
        );
    }
}
