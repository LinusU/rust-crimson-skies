//! Bevy composition, the Avian physics adapter, rendering, input, audio and UI.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. The workspace bootstrap ships
//! the typed, asset-free [`synthetic`] development scene, the `cs` binary
//! entry point, its [`cli`] request parser and the [`run`] entry point that
//! executes a fixed-tick headless synthetic smoke. Rendering, input, audio, UI
//! and retail mission composition arrive with later F00+ tasks.
//!
//! [`origin`] is the world-origin frame of `specs/F16-coordinates-units-
//! origin-management-and-clocks.md`: an f64 `WorldOrigin` per epoch, its f32
//! local frame, the typed distinction between a rebase (world identity and
//! swept continuity survive) and a teleport, and (F16-B) the atomic
//! `OriginShift` transaction that converts a whole set of `SpatialAnchor`s
//! into a new frame. F16-C binds those anchors into a `SpatialWorld` and
//! drives them from the `cs_sim` frame clock through `FixedTickDriver`, so a
//! render frame's wall time only ever adds whole fixed ticks and equal input
//! at 30, 60 or 144 FPS yields the same state.
//!
//! [`livery`] is the F09-C consumer
//! (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`, stage
//! `### F09-C`): per-model-instance faction and custom paints resolved
//! through a session-scoped composed-variant store, plus the construction
//! preview, teardown and retry the rendering stages (F17) drive.
//!
//! [`scene`] and [`airframe_visual`] are the F11-A ECS binding records
//! (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`,
//! `### F11-A`): a generation-stamped [`scene::SceneNodeBinding`] tying an
//! entity to its canonical scene node, the [`scene::NodeVisualTransform`]
//! conversion of the one composed transform into a full-affine
//! `GlobalTransform`, and [`airframe_visual::AirframeVisual`] — the
//! airframe → scene-root reference by content id, never by array position.
//! Stage `### F11-B` (the same module) adds the hierarchy import —
//! [`scene::import_scene`] and [`scene::import_airframe`] spawn that
//! hierarchy from a converted scene graph — and the presentation-only rule
//! [`scene::select_lod_presentation`], which recomputes only
//! [`scene::NodePresentation`] from the supplied [`scene::LodDistance`] and
//! the [`scene::NodeDisabled`] markers, so a destroyed node and everything
//! under it stay disabled across an LOD transition (AC02). Stage `### F11-C`
//! wires both into the running app: the [`scene::AirframeSceneRequest`]
//! producer hand-off served by [`scene::process_airframe_scene_request`]
//! (load, reload with generation ownership, teardown, and a refusal reported
//! in the [`scene::AirframeSceneLog`] instead of being swallowed), the
//! evidence-backed [`scene::PartBinding`]s a load binds from
//! `cs_content::scene::PartSocket`, and [`scene::apply_airframe_damage`],
//! which reflects the recorded [`scene::AirframeDamageState`] onto the
//! [`scene::NodeDisabled`] markers, so loading and unloading the same
//! airframe a hundred times leaves the live entity count unchanged (AC03).
//!
//! [`input`] is the F22-A application boundary
//! (`specs/F22-input-bindings-devices-and-control-ownership.md`,
//! `### F22-A`): the active action map and input context
//! ([`input::InputBindings`]) and the per-render-frame collector
//! ([`input::InputCollector`]) that resolves physical sources into the
//! ticked `cs_types::input::InputFrame` the simulation buffers. The device
//! adapters and axis calibration are F22-B.
//!
//! [`animation`] is the F20-A application boundary
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stage
//! `### F20-A`): [`animation::lower::lower_clip`] lowers a declared
//! `cs_content::animation::AnimationClip` into the
//! `cs_sim::animated_object::AnimatedClip` the fixed-tick evaluator plays,
//! [`animation::presentation::interpolated_pose`] is the render-side
//! fractional-alpha sampler that changes presentation only, and
//! [`animation::AnimatedNodeBinding`] is the generation-stamped ECS record
//! tying an entity to one animated node.
//!
//! [`physics`] is the F23-A Avian boundary
//! (`specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! `### F23-A`): the fixed-rate schedule adapter, the one-tick force/torque
//! request queue and the tick/integration ledger, plus a minimal synthetic
//! fixture. It is the only place the pinned Avian force accumulator is driven;
//! body creation, sweeps and kinematic transitions are F23-B.
//!
//! [`loading`] is the F15-A load-transaction contract
//! (`specs/F15-asynchronous-asset-loading-and-private-cache.md`, stage
//! `### F15-A`): the cancellable `Requested → Loading → Validating →
//! Ready|Failed` transaction, session/serial-stamped IO tickets whose late
//! completions are discarded, and the versioned [`loading::ReadyBundle`]
//! handoff that spawns its entities only under the world's expected load
//! identity. [`assets`] is the same stage's conversion boundary: the typed
//! `CanonicalAsset` input and `ConvertedAsset` output records that keep
//! canonical-to-Bevy conversion inside this crate.
//!
//! [`render`] is the F17-A rendering contract
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-A`): the declared-not-derived material classification
//! ([`render::material`]), the phase-ordered, depth-sorted
//! [`render::plan::DrawPlan`], and the [`render::golden`] synthetic test
//! scene — overlapping glass, an alpha-cut fence, an additive sprite and
//! per-corner colors. The Bevy adapters and profiles are F17-B/C.

pub mod airframe_visual;
pub mod animation;
pub mod assets;
pub mod cli;
pub mod input;
pub mod livery;
pub mod loading;
pub mod origin;
pub mod physics;
pub mod render;
pub mod run;
pub mod scene;
pub mod synthetic;
