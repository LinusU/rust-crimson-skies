//! Bevy composition, the Avian physics adapter, rendering, input, audio and UI.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. The workspace bootstrap ships
//! the typed, asset-free [`synthetic`] development scene, the `cs` binary
//! entry point, its [`cli`] request parser and the [`run`] entry point that
//! executes a fixed-tick headless synthetic smoke. Rendering, input, audio, UI
//! and retail mission composition arrive with later F00+ tasks.
//!
//! [`asset_stack`] is the Bevy asset stack every headless world in this crate
//! runs on and the one production path from a converted canonical mesh to a
//! mesh-derived collider: Avian's default `collider-from-mesh` feature makes
//! `PhysicsPlugins::default()` depend on `Assets<Mesh>` and
//! `AssetEvent<Mesh>`, so [`asset_stack::headless_app`] is where the base
//! plugin set is written down once and [`asset_stack::spawn_static_mesh_collider`]
//! is where an uploaded [`bevy::mesh::Mesh`] becomes collision.
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
//! [`input`] is the F22-A/F22-B/F22-C application boundary
//! (`specs/F22-input-bindings-devices-and-control-ownership.md`):
//! the active action map and input context ([`input::InputBindings`]) and the
//! per-render-frame collector ([`input::InputCollector`]) that resolves
//! physical sources into the ticked `cs_types::input::InputFrame` the
//! simulation buffers, plus the stage's device adapters
//! ([`input::DeviceAdapters`], [`input::DeviceEvent`]): one event per device
//! per render frame, calibrated per [`cs_types::input::DeviceId`] identity and
//! [`cs_types::input::AxisChannel`], with device removal releasing the held
//! buttons, neutralizing the axes the device drove and reporting a
//! [`input::DeviceLoss`]. Stage `### F22-C` adds the loop that owns both ends:
//! [`input::InputSession`] holds a collector *and* the simulation's
//! [`cs_sim::control::ControlBuffer`], [`cs_sim::control::ControlGate`] and
//! [`cs_sim::control::ThrottleSteps`], and is the only thing that changes the
//! session's context, focus, pause or control authority, so a menu, a text
//! field, a pause screen and the simulation cannot disagree about who owns the
//! devices. It routes every [`cs_types::input::Action::Ui`] out of the flight
//! path as an [`input::UiRequest`] and performs none of them, pauses only where
//! the [`input::SessionMode`] allows it, runs no input boundary while paused,
//! and releases the whole local input path — holds, queued edges, held axes —
//! on a focus loss, a pause, a control handover and a teardown. It also records
//! the [`cs_types::input::CommandStream`] the consumer executed at each input
//! boundary, which [`input::CommandReplay`] feeds back through the same pump at
//! any display rate (AC03).
//!
//! [`animation`] is the F20-A/F20-B application boundary
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stages
//! `### F20-A` and `### F20-B`): [`animation::lower::lower_clip`] lowers a
//! declared `cs_content::animation::AnimationClip` into the
//! `cs_sim::animated_object::AnimatedClip` the fixed-tick evaluator plays,
//! [`animation::presentation::interpolated_pose`] is the render-side
//! fractional-alpha sampler that changes presentation only, and
//! [`animation::AnimatedNodeBinding`] is the generation-stamped ECS record
//! tying an entity to one animated node. Stage `### F20-B` adds
//! [`animation::playback`]: [`animation::play_animation`] starts one
//! lowered instance of a declared clip in the [`animation::AnimationPlayback`]
//! resource, [`animation::advance_animation`] is the fixed-tick entry that
//! advances every instance, publishes its markers into the
//! [`animation::AnimationLog`] and applies the transform, material and
//! attachment tracks to the entities whose binding verifies — a playing
//! clip, the live scene generation and a node the clip actually drives —
//! while an unknown material or parent is blocked and reported instead of
//! applied, and [`animation::stop_animation`] ends an instance.
//! Stage `### F20-C` adds [`animation::attachment`], the consumer half of
//! that record: [`animation::apply_attachment_transitions`] (run at the end
//! of every fixed-tick advance) turns it into the parent change itself —
//! `ChildOf` inserted or removed with the authored pose policy, the world
//! pose of the node's descendants recomposed behind it, and the detached
//! node given the world velocity its parent had at that tick, exactly once
//! per change — while [`animation::release_attachments_before_despawn`]
//! releases an animated attachment before its parent is despawned
//! (non-negotiable behavior 4, AC03).
//!
//! [`physics`] is the F23-A Avian boundary
//! (`specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! `### F23-A`): the fixed-rate schedule adapter, the one-tick force/torque
//! request queue and the tick/integration ledger, plus a minimal synthetic
//! fixture. It is the only place the pinned Avian force accumulator is driven;
//! body creation, sweeps and kinematic transitions are F23-B.
//!
//! [`world`] is the F18 world boundary
//! (`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A` and `### F18-B`): [`world::spawn_world`] turns a
//! validated `cs_content::world::WorldDefinition` into a presented entity and a
//! collider built from *one* authored transform and one
//! [`world::WorldObjectBinding`] — for `WorldCollisionShape::FromMesh` from
//! *one* uploaded asset handle, whose `TrimeshFromMesh` collider keeps every
//! stored triangle — [`world::WorldContacts`] records which authored object, in
//! which sector, under which surface rule an actor reached,
//! [`world::load_world`] and its sector calls are the one-world-at-a-time load
//! transaction whose per-object condition survives an unload, and
//! [`world::arch_world`] / [`world::harbor_world`] are the synthetic arch the
//! acceptance tests sweep a body through and the mesh-authored harbor world
//! they stream. Mission overlays and visibility-driven streaming are F18-C; the
//! simplification policy for retail geometry and the original's own unit scale
//! are unmeasured (see
//! `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`).
//!
//! [`loading`] is the F15 load pipeline
//! (`specs/F15-asynchronous-asset-loading-and-private-cache.md`): the
//! F15-A cancellable `Requested → Loading → Validating → Ready|Failed`
//! transaction with session/serial-stamped IO tickets whose late
//! completions are discarded; the F15-B [`loading::LoadDriver`] that runs
//! each item's bounded read — a verified private-cache hit, or a source
//! read, conversion and atomic publish — and re-verifies cached payloads
//! before `Ready`; and the F15-C wiring: the [`loading::LoadIo`] producer
//! seam and [`loading::SessionIo`] content-session producer, the
//! [`loading::LoadingScreen`] render-model, the [`loading::LoadingSession`]
//! pump/cancel/retry/invalidate lifecycle and the [`loading::ExpectedLoad`]
//! handoff that spawns a [`loading::ReadyBundle`]'s entities only in the
//! world that announced exactly that load. [`assets`] is the conversion
//! boundary those stages share: the typed `CanonicalAsset` input and
//! `ConvertedAsset` output records that keep canonical-to-Bevy conversion
//! inside this crate.
//!
//! [`render`] is the F17 rendering contract
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stages `### F17-A`, `### F17-B` and `### F17-C`): the declared-not-derived
//! material classification ([`render::material`]), the phase-ordered,
//! depth-sorted [`render::plan::DrawPlan`], and the [`render::golden`]
//! synthetic test scene — overlapping glass, an alpha-cut fence, an additive
//! sprite and per-corner colors; plus the canonical-to-Bevy adapters
//! ([`render::bevy_mesh`], [`render::bevy_image`], [`render::bevy_state`]),
//! the comparison frame capture ([`render::capture`]), the faithful profile and
//! its independently switchable enhanced options ([`render::profile`]), the
//! instance batching that keeps every aircraft's committed paint and damage
//! state per instance ([`render::batch`]), the session that applies a profile
//! and syncs a batched frame into the ECS ([`render::sync`]), and the additive
//! class's own material and WGSL shader ([`render::additive`]) — the `One`/`One`
//! blend a `StandardMaterial` cannot reach, which every class now has a
//! drawable form of.
//!
//! [`audio`] is the F41-A audio boundary
//! (`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`): [`audio::lower_record`] and [`audio::lower_catalog`], which
//! lower a declared `cs_content::audio::AudioAssetRecord` into the
//! `cs_sim::audio_events::AudioAssetSpec` the router schedules, refusing an
//! unknown bus, level or playback mode by claim instead of inventing one; and
//! the generation-stamped [`audio::AudioEmitterBinding`] tying an entity to its
//! session-qualified emitter, bus and asset. F41-B adds [`audio::sync_emitter_loops`],
//! which starts, swaps and stops loops as those bindings spawn and despawn.
//!
//! [`damage`] is the F29-A damage boundary
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`): [`damage::lower_graph`], which lowers a declared
//! `cs_content::damage::DeclaredDamageGraph` into the
//! `cs_sim::damage::DamageGraph` the session resolver registers with every
//! `Resolved::Unknown` carried through; [`damage::lower_policy`], which
//! lowers the graph's declared lethal-attribution rule and refuses an
//! unknown one outright; and the generation-stamped
//! [`damage::DamageActorBinding`] ECS record tying an entity to its
//! session-qualified actor and damage-graph subject.
//!
//! [`capital`] is the F35-A capital-ship boundary
//! (`specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`, stage
//! `### F35-A`): [`capital::lower_capital_ship`], which lowers a declared
//! `cs_content::capital::DeclaredCapitalShip` into the
//! `cs_sim::capital::CapitalShip` aggregate with every subsystem, engine,
//! bay, turret, anchor, cargo pool and trajectory mapped field-wise and every
//! `Resolved::Unknown` carried through; the boundary refuses an unknown
//! initial owner outright, because guns, targeting and docking eligibility
//! cannot switch coherently under a guessed owner; and the generation-stamped
//! [`capital::CapitalActorBinding`] ECS record.
//!
//! [`targeting`] is the F30-A targeting boundary
//! (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`): [`targeting::lower_rules`], which lowers a declared
//! `cs_content::target_rules::DeclaredTargetRules` into the
//! `cs_sim::targeting::TargetPolicy` and `AllegianceTable` a session opens
//! with — refusing every `Resolved::Unknown` rather than guessing a
//! relation, window or assistance flag — and the generation-stamped
//! [`targeting::TargetableBinding`] ECS record tying an entity to its
//! session-qualified targeting actor and rules subject.
//!
//! [`weapons`] is the F27-A weapon boundary
//! (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-A`): [`weapons::lower_gun`], which lowers a declared
//! `cs_content::weapons::DeclaredGunDefinition` into the
//! `cs_sim::weapons::GunDefinition` a fire resolver registers — refusing
//! every unresolved field by name, because a session must not fire a gun
//! whose muzzle velocity, cadence, spread or damage was invented;
//! [`weapons::lower_rules`], which lowers the declared self-hit,
//! friendly-fire, penetration, ricochet and ammo-switching options into the
//! runtime rules a sweep filters candidates with and refuses each unknown;
//! [`weapons::lower_ammunition`]; the [`weapons::MountPoseBinding`] and
//! generation-stamped [`weapons::WeaponActorBinding`] ECS records that keep
//! a mount's live hierarchy pose session- and generation-qualified.
//!
//! [`ordnance`] is the F28-A ordnance boundary
//! (`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`, stage
//! `### F28-A`): [`ordnance::lower_ordnance`], which lowers a declared
//! `cs_content::ordnance::DeclaredOrdnance` into the
//! `cs_sim::weapons::ordnance::OrdnanceComponent` a session registers —
//! refusing every unresolved field by name, because a session must not fly a
//! rocket whose trigger radius, arming delay, blast radius, damage or
//! lost-target rule was invented;
//! [`ordnance::lower_equipment_rules`], the shared equipment-compatibility
//! rule the shop and an import both read, refusing each unresolved option;
//! and the generation-stamped
//! [`ordnance::OrdnanceLauncherBinding`] ECS record that keeps a launcher's
//! live hierarchy pose session- and generation-qualified.

//! [`environment`] is the F19-A/F19-B environment boundary
//! (`specs/F19-sky-atmosphere-weather-and-visibility.md`, stages
//! `### F19-A` and `### F19-B`): [`environment::SkyFrame`], the one record
//! that centres a sky on the camera's **world** position while carrying the
//! authored sky orientation and sun direction unchanged, so a world rebase
//! cannot rotate or pop the sky (AC01);
//! [`environment::EnvironmentClock`], which runs a definition's authored
//! weather timeline on authoritative-gameplay time; and the synthetic
//! [`environment::clear_sky_environment`] /
//! [`environment::storm_environment`] fixtures the acceptance tests drive.
//! Stage `### F19-B` adds the *effects* of those records:
//! [`environment::AuthoritativeWind`], the one wind field flight and
//! projectiles read (`v_air = v_world - wind_world`, refused when the wind
//! is unknown) together with [`environment::ProjectileMotion`], so a wind
//! change moves aircraft airspeed and projectile-relative velocity
//! consistently (AC02) — through `cs_sim::environment`'s one conversion, which
//! the flight models call too (task #434 `F19-WIND-CONVERSION-OWNER`);
//! [`environment::EnvironmentEffects`] and the
//! [`environment::SkyEffect`] / [`environment::FogEffect`] /
//! [`environment::LightEffect`] it gathers, which are what a frame may be
//! drawn from — a missing sky texture stays a diagnostic, fog fades but never
//! becomes a sight range, and an unknown sun or ambient produces no rig; and
//! [`environment::CosmeticField`], the decorative precipitation drawn only
//! from the run's cosmetic weather stream. No renderer draws a sky from these
//! records yet: F19-C wires them into their real producer and consumer.
//!
//! [`camera`] is the F21-A camera boundary
//! (`specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-A`):
//! [`camera::lower_projection`], which lowers a declared
//! `cs_content::cameras::ProjectionPolicy` into the
//! [`camera::LoweredProjection`] a renderer consumes — normalizing the
//! authored field of view to the vertical axis at its reference aspect,
//! refusing every `Resolved::Unknown` and refusing a stretch framing rule —
//! and the framing math that reports the vertical and horizontal field of
//! view at any aspect and the normalized viewport coordinate of a
//! world-space point; [`camera::CameraPose`] and [`camera::CameraBasis`],
//! the read-only copy of the authoritative pose and its derived axes; and
//! [`camera::lower_camera_modes`], which lowers a declared
//! `cs_content::cameras::DeclaredCameraModes` set into the
//! [`camera::LoweredCameraModes`] a session's camera path consumes.
//!
//!
//! [`ai`] is the F31-C mission-ECS binding
//! (`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`): [`ai::bind_route`], which projects a resolved
//! `cs_content::routes::ResolvedRoute` into the runtime
//! `cs_sim::ai::navigation::RouteGraph` while keeping the authored node id ->
//! runtime node id map and refusing an unbound moving anchor or an
//! unsupported termination by name; the [`ai::MovingAnchor`] component whose
//! live Avian transform is sampled into the route frame every fixed tick; the
//! [`ai::RoutePursuit`] component an AI aircraft carries; the session resource
//! [`ai::AiNavigation`] that owns one `NavigationSet` and reconciles its roster
//! with the live pursuit entities; and [`ai::AiNavigationPlugin`], whose
//! fixed-tick systems decide one bounded command per aircraft and write it into
//! the same `FlightAircraft` boundary the player input session uses.
//!
//! [`text`] is the F51-A text boundary
//! (`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`, stage
//! `### F51-A`): [`text::TextMetrics`] — the typed measurement input where
//! F51-B's font parsing will land, so the layout never opens a font file — and
//! [`text::layout_text`], which lays a validated
//! `cs_content::localization::MarkupDocument` out inside a panel's largest
//! **free band** (the strip no required control occupies) and reports whether
//! the result fits or has to scroll, so a long translation never paints over
//! the buttons a screen must keep readable (AC01). Every absent glyph, refused
//! markup control and unresolved substitution comes back as a
//! [`text::LayoutDiagnostic`] rather than being applied silently. Nothing here
//! draws: F51-C wires it into the real menus, HUD and subtitles.

pub mod ai;
pub mod airframe_visual;
pub mod animation;
pub mod asset_stack;
pub mod assets;
pub mod audio;
pub mod camera;
pub mod campaign;
pub mod capital;
pub mod cli;
pub mod damage;
pub mod environment;
pub mod input;
pub mod livery;
pub mod loading;
pub mod ordnance;
pub mod origin;
pub mod physics;
pub mod profile;
pub mod render;
pub mod run;
pub mod scene;
pub mod synthetic;
pub mod targeting;
pub mod text;
pub mod weapons;
pub mod world;
