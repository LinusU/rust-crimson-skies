//! Rendering: material classification, the ordered draw plan and the
//! golden synthetic render-test scene
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-A`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! This stage fixes the typed contract and its fixture, not a runtime:
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
//! Nothing here touches Bevy's render world: the Bevy mesh/image adapters
//! are F17-B, the faithful/enhanced profiles F17-C. The module is
//! deliberately Bevy-free data, so the contract tests run headless.

pub mod golden;
pub mod material;
pub mod plan;
