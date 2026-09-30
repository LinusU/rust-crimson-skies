//! Rendering: material classification, the ordered draw plan, the golden
//! synthetic render-test scene and the canonical-to-Bevy adapters
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stages `### F17-A` and `### F17-B`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! F17-A fixed the Bevy-free contract:
//!
//! * [`material`] is the classification boundary. A surface's render
//!   class — [`material::MaterialClass`] opaque, masked, blended, additive
//!   or emissive — is never derived from raw stored bytes (the GameZ
//!   record does not establish one); it is *declared* with an evidence
//!   status and checked for consistency against the coverage, alpha-test
//!   and surface facts the content pipeline established. Facts that
//!   establish nothing produce [`material::Classification::Unclassified`]
//!   with every reason, never a default.
//! * [`plan`] is the ordering boundary. [`plan::DrawPlan::build`] groups
//!   classified [`plan::DrawItem`]s into the fixed [`material::RenderPhase`]
//!   order, keeps authored submission order inside the opaque and masked
//!   phases, sorts translucent and additive items back-to-front against
//!   one [`plan::SceneView`], and reports equal-depth pairs in
//!   [`plan::DrawPlan::limitations`] instead of hiding them.
//! * [`golden`] is the minimum synthetic fixture (the sheet's AC01):
//!   overlapping glass, an alpha-cut fence, an additive sprite and a
//!   per-corner-colored quad, submitted scrambled so only a correct plan
//!   orders them.
//!
//! F17-B adds the production path, the only place in the workspace where a
//! canonical IR becomes a Bevy asset — which is what makes spec F15's "only
//! `cs_app` converts canonical assets into Bevy assets" structural rather
//! than a promise:
//!
//! * [`bevy_mesh::upload_group`] turns one `cs_content::mesh::RenderGroup`
//!   into compacted Bevy vertex/index buffers, bit-exact, with no attribute
//!   fabricated and no triangle dropped.
//! * [`bevy_image::upload_image`] turns one
//!   `cs_formats::texture::DecodedImage` into a Bevy texture whose format
//!   is chosen from the stored color space, so the GPU corrects the stored
//!   values exactly once (spec F17 non-negotiable 3). A 16-bit packed image
//!   is widened to 8-bit channels and its coverage is composed on the CPU
//!   first, both by the decided policy in [`rgb565`].
//! * [`bevy_state::render_state`] reads the render state out of a
//!   [`material::ClassifiedMaterial`] and refuses the two facts nothing
//!   measured (two-sidedness, addressing).
//! * [`capture::capture`] records one tick's frame — settings, camera, tick,
//!   every surface's digests in draw order, every refusal with its reason —
//!   so the same camera and tick captured twice can be compared (AC02)
//!   without a GPU.
//!
//! Every adapter refuses rather than defaults: an unestablished fact becomes
//! a reason code on a refusal the capture reports, never a value invented
//! here.
//!
//! F17-C adds the two ends F17-B left open — the profile a frame renders under
//! and the consumer that draws it:
//!
//! * [`profile::RenderProfile`] is the fidelity baseline plus the enhanced
//!   options, each switchable on its own. A profile with any enhancement on
//!   reports [`profile::ProfileParity::DesignedImprovement`] and is refused as
//!   comparison evidence, and an enhancement reaches only
//!   [`profile::Presentation`] — never a material, a sort, a collider or a
//!   visibility rule.
//! * [`batch::batch_frame`] groups the plan's draws into the batches that
//!   share their GPU resources, keeping every instance's committed livery and
//!   damage state per instance (AC03): a different paint is a different
//!   batch, an unresolved paint is never merged, and a destroyed part is
//!   withheld with its reason instead of being drawn or silently dropped.
//! * [`sync::process_render_profile_request`] is the profile producer's
//!   hand-off and [`sync::sync_frame`] the ECS consumer that turns a batched
//!   frame into entities, binds each batch's image, applies the presentation
//!   to the camera, light and window that own it, releases what the frame no
//!   longer claims, and refuses a frame whose profile nobody applied so the
//!   caller can apply it and retry.
//!
//! # Acceptance tests
//!
//! The `accept_f17_c_` selection in `crates/cs_app/tests/render/profiles.rs`
//! covers the sheet's AC03 — two instances with different paint and damage
//! staying visually independent after batching — plus the profile rules: every
//! enhancement independently switchable, the two forbidden options refused, an
//! enhanced profile refused as comparison evidence, no enhancement changing a
//! draw decision, and the consumer's teardown, refusal and retry paths.

pub mod batch;
pub mod bevy_image;
pub mod bevy_mesh;
pub mod bevy_state;
pub mod capture;
pub mod golden;
pub mod material;
pub mod plan;
pub mod profile;
pub mod rgb565;
pub mod sync;
