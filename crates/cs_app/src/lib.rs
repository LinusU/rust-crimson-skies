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
//! The same load also gives a scene node's **collider** a spawn path (F20-C):
//! a node whose authored [`cs_content::scene::CollisionRole::Collider`] is
//! opted into [`physics::NodeColliderPresence`] and carries an Avian collider
//! on its own entity, built from the geometry a load declares in the
//! [`scene::SceneCollisionGeometry`] resource and placed through the one
//! affine decision the world path uses — and a node whose geometry nobody
//! declared is reported by name in [`scene::SceneEvent::Loaded`] instead of
//! being fitted with a guessed shape.
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
//! [`animation`] is the F20-A/F20-B application boundary
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stages
//! `### F20-A` and `### F20-B`): [`animation::lower::lower_clip`] lowers a
//! declared `cs_content::animation::AnimationClip` into the
//! `cs_sim::animated_object::AnimatedClip` the fixed-tick evaluator plays,
//! [`animation::presentation::interpolated_pose`] is the render-side
//! fractional-alpha sampler that changes presentation only, and
//! [`animation::AnimatedNodeBinding`] is the generation-stamped ECS record
//! tying an entity to one animated node of one playing
//! [`animation::AnimationInstance`]. Stage `### F20-B` adds
//! [`animation::playback`]: [`animation::play_animation`] starts one
//! lowered instance of a declared clip in the [`animation::AnimationPlayback`]
//! resource, [`animation::advance_animation`] is the fixed-tick entry that
//! advances every instance, publishes its markers into the
//! [`animation::AnimationLog`] and applies the transform, visibility, material
//! and attachment tracks to the entities whose binding verifies — a playing
//! instance, the live scene generation and a node the clip actually drives —
//! while an unknown material or parent is blocked and reported instead of
//! applied, and [`animation::stop_animation`] ends one instance and releases
//! what that instance applied, for its own entities only.
//! Stage `### F20-C` adds [`animation::attachment`], the consumer half of
//! that record: [`animation::apply_attachment_transitions`] (run at the end
//! of every fixed-tick advance) turns it into the parent change itself —
//! `ChildOf` inserted or removed with the authored pose policy, the world
//! pose of the node's descendants recomposed behind it, and the detached
//! node given the world velocity its parent had at that tick, exactly once
//! per change — while [`animation::release_attachments_before_despawn`]
//! releases an animated attachment before its parent is despawned
//! (non-negotiable behavior 4, AC03). Stage `### F20-C.02` adds
//! [`animation::schedule`], the producer wiring: the session driver's
//! [`animation::CommittedSessionTick`] stamp is the only clock input, and
//! [`animation::advance_animation_on_session_tick`] — installed by
//! [`animation::AnimationSchedulePlugin`] in `FixedPostUpdate` after the
//! physics step — advances the playback once per **committed tick change**,
//! nothing without the stamp and nothing for a repeated one, while
//! [`animation::release_superseded_instances`] releases what the instances of
//! a superseded scene generation had applied. Stage `### F20-C.03` adds
//! [`animation::visibility`], the consumer half of the visibility channel and
//! the ownership decision against the two systems that also decide whether a
//! node is drawn: [`animation::NodeAnimatedVisibility`] is the clip's
//! evaluated visibility applied on the same verified path as the other
//! channels, and [`animation::composed_visibility_verdict`] returns the
//! single [`animation::VisibilityVerdict`] a render or collision consumer
//! reads — composed on read out of that record, the node's own
//! [`scene::NodeDisabled`] marker and F11-C's
//! [`scene::NodePresentation`], with damage outranking LOD, LOD's cull
//! outranking the clip's reason, and a clip-hidden node carrying no collider
//! whatever the draw verdict says. Nothing in the animation path writes
//! [`scene::NodePresentation`] or [`scene::NodeDisabled`], so neither the LOD
//! pass nor an animation pass can silently lose the other's decision, and a
//! destroyed node is never re-drawn by a loop pass or a distance change — not
//! even in the frame between damage's marker and that frame's LOD pass
//! (non-negotiable behavior 3). The F20-C integration step adds the missing
//! producers: [`animation::bind_animated_node`] is the spawn-side entry that
//! writes the generation-stamped [`animation::AnimatedNodeBinding`] and starts
//! the `(track, instance)` it names, and [`animation::AnimationPlugin`] is the
//! one-stop production composition — its [`animation::commit_session_tick`]
//! copies the F23-A physics ledger's committed tick into
//! [`animation::CommittedSessionTick`] and it installs the fixed-tick advance,
//! so a [`physics::PhysicsSession`] that adds the plugin through its
//! `configure` seam drives the whole path. The mission-marker consumer of
//! [`animation::AnimationLog`] is [`mission_markers`]: it drains the log
//! through the seam F20-C named, resolves each fired gameplay marker through
//! the mission host's **declared** cue → signal table
//! ([`mission_markers::MissionMarkerBindings`], which refuses an unbound cue
//! rather than inventing a symbol) and raises it into a real
//! [`objectives::ObjectiveSession`] step, exactly once per marker activation
//! ([`mission_markers::MarkerActivation`]) so a loop pass, a skip or a
//! re-drained batch cannot duplicate a mission event (F20 non-negotiable
//! behavior 5). It is task #507's slice; no production mission host drives it
//! yet (the animation composition is filed as #509).
//! `### F20-C.04` gives the verdict's collider half its reader in
//! [`physics::collider`], so the clip's half of the verdict now reaches the
//! simulation and the animation path still writes nothing but its own record.
//!
//! Task #718 (`M01-LC-ACTOR-ANIM-CONSUMERS`) adds [`mission_animations`], the
//! mission session's other animation consumer. It is the consumer the member →
//! actor binding (#632), the join (#678) and the event grammar (#690) were
//! measured for and never had: [`mission_animations::MissionAnimationPlayer`]
//! starts one mission scope's own `mis_anim.zbd` / `cam_anim.zbd` startup
//! rows, advances them once per committed tick and reports the statements they
//! reach — with the archive and member that declares each one, and with the
//! object selectors that name the world records those members drive — while
//! every row the join refused keeps its refusal and is never started;
//! [`mission_animations::step_mission_animations`] is the composed tick that
//! runs that record half beside the marker half above, so one call carries the
//! animation log into [`objectives::ObjectiveSession`] and advances the
//! mission's own animation records for the same committed tick.
//!
//! [`camera`] is the F21-A/F21-B/F21-C camera boundary
//! (`specs/F21-cameras-cockpit-views-and-spyglass.md`, stages `### F21-A`,
//! `### F21-B` and `### F21-C`):
//! [`camera::lower_projection`], which lowers a declared
//! `cs_content::cameras::ProjectionPolicy` into the
//! [`camera::LoweredProjection`] a renderer consumes — normalizing the
//! authored field of view to the vertical axis at its reference aspect,
//! refusing every `Resolved::Unknown` and refusing a stretch framing rule —
//! and the framing math that reports the vertical and horizontal field of
//! view at any aspect and the normalized viewport coordinate of a
//! world-space point; [`camera::CameraPose`] and [`camera::CameraBasis`],
//! the read-only copy of the authoritative pose and its derived axes;
//! [`camera::lower_camera_modes`], which lowers a declared
//! `cs_content::cameras::DeclaredCameraModes` set into the
//! [`camera::LoweredCameraModes`] a session's camera path consumes, keeping
//! each mode's placement and its `cs_types::content::Origin` so a cockpit
//! binding's provenance survives lowering; stage `### F21-B`:
//! [`camera::CameraRig`], the cockpit, chase, free-look and spyglass rigs that
//! turn those records into the [`camera::RigFrame`] a renderer draws,
//! [`camera::PoseSmoother`], whose law is frame-rate independent and which
//! reseats only on a teleport or an aircraft swap, and the spyglass rules
//! that drop a destroyed or switched target in the same frame instead of
//! leaving a stale magnified actor behind; and stage `### F21-C`, which owns
//! the runs rather than the rigs: [`camera::ScriptCameraRequest`], the typed
//! bounded request a producer hands the camera (no timeline — F40 owns those),
//! [`camera::CaptureRequest`], the deterministic capture's mission, tick,
//! world pose, aspect, view and fixed comparison settings with the override
//! report non-negotiable behavior 5 asks for, [`camera::pin`], which derives
//! the projection a capture carries from the frame's own lowered policy at the
//! capture aspect and keeps the `f64 → f32` narrowing visible instead of
//! growing a second projection owner, and [`camera::CameraSession`], which
//! owns the player's rig, at most one scripted camera and at most one pending
//! capture, and produces one [`camera::SessionFrame`] per render frame naming
//! the authority that drew it — which is where AC03, *swap aircraft during a
//! scripted capture and verify the camera binds to the new player body*, is
//! decided.
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
//! [`physics`] is the F23-A Avian boundary
//! (`specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! `### F23-A`): the fixed-rate schedule adapter, the one-tick force/torque
//! request queue and the tick/integration ledger, plus a minimal synthetic
//! fixture. It is the only place the pinned Avian force accumulator is driven;
//! body creation, sweeps and kinematic transitions are F23-B. Stage
//! `### F20-C.04` adds [`physics::collider`], the collision-side consumer of
//! the animation layer's visibility verdict:
//! [`physics::NodeColliderPresence`] is the record this layer owns for a node's
//! collider, [`physics::apply_collider_presence`] merges the composed
//! [`animation::ColliderVerdict`] into it and projects it onto Avian's
//! `ColliderDisabled` marker, and a damage removal recorded there is terminal
//! for the pass, so no clip verdict can re-enable a collider the damage side
//! removed.
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
//! runtime rules a sweep filters candidates with and refuses each unknown
//! (penetration, ricochet and ammo switching are *deferred* to F27-D and named
//! as such by `cs_content::weapons::InteractionRules::deferred`, so no runtime
//! code pretends to apply them);
//! [`weapons::lower_ammunition`]; the [`weapons::MountPoseBinding`] and
//! generation-stamped [`weapons::WeaponActorBinding`] ECS records that keep
//! a mount's live hierarchy pose session- and generation-qualified; and the
//! F27-C application consumer [`weapons::resolve_swept_damage`], which is the
//! production caller of `cs_sim::weapons::GunHitRouter` — it turns one
//! accepted shot's swept contacts into `cs_sim::damage::HitEvent`s carrying
//! the gun definition's own per-channel damage amounts and hands them to the
//! session's authoritative `DamageResolver`, so a gun's declared damage
//! reaches the graph through the one authority that owns it. The boundary
//! decision this implements is recorded in
//! `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`.
//! Stage `### F27-C`'s application half (task #119) is the same module:
//! [`weapons::WeaponSession`] is one session generation's authority over the
//! cadence, the router, the lowered interaction rules, the live rounds and the
//! effects, [`weapons::step_weapon_session`] is the single per-tick step that
//! runs the selection, the fire, the accepted-shot effects, the swept damage
//! and the retirement of spent rounds, and [`weapons::sync_round_mirrors`]
//! reconciles the ECS mirror of the authoritative rounds — each round's
//! `Transform` written from the simulation's own position and carrying no
//! collider, so Avian never becomes a second contact authority. The design
//! record is `docs/findings/2026-10-02-f27-c-weapon-session-wiring.md`.
//!
//! [`mission_control`] is the measured mission control program (task
//! `M01-LC-MISSION-PROGRAM`, #630):
//! [`mission_control::survey_mission_control_programs`], which reads every
//! mission-scoped reader archive of the installation, finds each mission's
//! control member by the *rule* that the member is the one whose decoded record
//! declares numbered `OBJECTIVE<N>` blocks — decoding every member so the rule has
//! candidates to choose between — and measures the directive vocabulary that
//! member spells. It reports which directives the engine can honour and names
//! every one it cannot, and it keeps the campaign gate closed
//! ([`mission_control::RetailControlCensus::campaign_ready`]) while a single
//! mission declares a directive with no measured effect. Nothing here claims what
//! any original spelling *does*: no original executable has been run.
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

//! [`damage`] is the F29-A damage boundary
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`): [`damage::lower_graph`], which lowers a declared
//! `cs_content::damage::DeclaredDamageGraph` into the
//! `cs_sim::damage::DamageGraph` the session resolver registers with every
//! `Resolved::Unknown` carried through; [`damage::lower_policy`], which
//! lowers the graph's declared lethal-attribution rule and refuses an
//! unknown one outright; the generation-stamped
//! [`damage::DamageActorBinding`] ECS record tying an entity to its
//! session-qualified actor and damage-graph subject; and the damage-to-collider
//! bridge ([`damage::DamageZoneBinding`], [`damage::apply_damage_events`],
//! [`damage::repair_damage_zone`]) that decides a destroyed zone's collider and
//! calls the F20-C.04 collision seam.
//!
//! [`debris`] is the F29-C.2 debris consumer
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C`): [`debris::PartDebrisBinding`], the spawn path's record of
//! which authored debris object one part's destruction presents, and
//! [`debris::apply_debris_state`], the state-driven pass that spawns exactly
//! one [`debris::SpawnedDebris`] instance for every destroyed part with a
//! resolved binding, despawns it again when the authority stops calling the
//! part destroyed, refuses an unresolved binding by name and invents nothing
//! for a part that has none — with [`debris::release_debris`] as the
//! reload/restart/swap teardown entry. The debris asset and its presentation
//! belong to another stage; this module owns the decision and the record.
//!
//! [`targeting`] is the F30-A/F30-B targeting boundary
//! (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stages `### F30-A`/`### F30-B`): [`targeting::lower_rules`], which
//! lowers a declared `cs_content::target_rules::DeclaredTargetRules` into
//! the `cs_sim::targeting::TargetPolicy` and `AllegianceTable` a session
//! opens with — refusing every `Resolved::Unknown` rather than guessing a
//! relation, window or assistance flag — and the generation-stamped
//! [`targeting::TargetableBinding`] ECS record tying an entity to its
//! session-qualified targeting actor and rules subject.
//!
//! F30-B adds the production path: [`targeting::lower_selection_actions`]
//! lowers the declared action table into the `cs_sim::targeting`
//! command-edge table,
//! [`targeting::TargetingSession`] owns one session's store, selection and
//! last phase record, and the three producer entries
//! [`targeting::sync_targetable_roster`] (roster and poses from bound
//! entities), [`targeting::apply_selection_edges`] (command edges to
//! selections) and [`targeting::apply_target_damage`] (the damage system's
//! applied hits and lifecycle transitions to threat state and eligibility)
//! drive it.
//!
//! F30-C adds the consumer half:
//! [`targeting::apply_target_consumers`] derives the three views a session
//! publishes — [`targeting::HudTargetReadout`] (the reticle and the threat
//! list the HUD draws),
//! [`targeting::SpyglassReadout`] (the target the spyglass magnifies) and
//! [`targeting::GuidanceReadout`] (what aid the weapon path may offer) — from
//! one phase record at the tick the consumers render, publishing them through
//! the [`targeting::TargetConsumers`] resource, and
//! [`targeting::teardown_target_consumers`] drops them at the end of a
//! session generation. Each view carries its evidence and its refusal: a
//! destroyed or withdrawn target clears before any consumer describes it, and
//! no view confers combat authority.
//!
//!
//! [`ai`] is the F31-C mission-ECS binding
//! (`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`): [`ai::bind_route`], which projects a resolved
//! `cs_content::routes::ResolvedRoute` into the runtime
//! `cs_sim::ai::navigation::RouteGraph` while keeping the authored node id ->
//! runtime node id map, carrying the declared route termination across (a
//! `Loop` record binds as a route that re-arms its marker sequence, not as one
//! refused by name) and refusing an unbound moving anchor by name; the
//! [`ai::MovingAnchor`] component whose
//! live Avian transform is sampled into the route frame every fixed tick; the
//! [`ai::RoutePursuit`] component an AI aircraft carries; the session resource
//! [`ai::AiNavigation`] that owns one `NavigationSet` and reconciles its roster
//! with the live pursuit entities; and [`ai::AiNavigationPlugin`], whose
//! fixed-tick systems decide one bounded command per aircraft and write it into
//! the same `FlightAircraft` boundary the player input session uses.
//!
//! [`roster`] is the F33-A/F33-B/F33-C pilot-roster boundary
//! (`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stages `### F33-A`, `### F33-B` and `### F33-C`): [`roster::lower_roster`],
//! which lowers a declared `cs_content::pilots::DeclaredRoster` into the
//! [`roster::LoweredRoster`] a session registers actors and wingmate
//! assignments from — refusing an unknown pilot voice (never a random line)
//! and an unknown survivability (never a silent mortal); [`roster::open_roster`],
//! which opens a session's `AlliesRoster` from that lowered roster and the
//! player's briefing plan, so a retry rebuilds the authored wingmate set
//! rather than carrying the failed world's; [`roster::apply_roster_lifecycle`]
//! / [`roster::apply_ally_lifecycle`], which feed the authoritative F29
//! damage lifecycle into the identity record as an
//! [`cs_sim::allies::AllyEvent`] mission callback plus an authored-voice
//! [`roster::DialogueCue`] and ground an ally that may no longer act in the
//! weapon firing gate (AC03); and the generation-stamped
//! [`roster::RosterBinding`] ECS record tying an entity to its
//! session-qualified actor and roster subject.
//!
//! [`capital`] is the F35-A capital-ship boundary, extended by F35-B and
//! F35-C (`specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stages `### F35-A` to `### F35-C`):
//! [`capital::lower_capital_ship`], which lowers a declared
//! `cs_content::capital::DeclaredCapitalShip` into the
//! `cs_sim::capital::CapitalShip` aggregate with every subsystem, engine,
//! bay (each launch bay with the socket and capacity F35-C consumes), turret,
//! anchor, cargo pool and trajectory mapped field-wise and every
//! `Resolved::Unknown` carried through; the boundary refuses an unknown
//! initial owner outright, because guns, targeting and docking eligibility
//! cannot switch coherently under a guessed owner; and the
//! generation-stamped [`capital::CapitalActorBinding`] ECS record.
//!
//! [`interaction`] is the F36-A interaction boundary
//! (`specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`, stage
//! `### F36-A`): [`interaction::lower_interaction`], which lowers a declared
//! `cs_content::interaction::DeclaredInteraction` into the
//! `cs_sim::interaction` kind, authorization, swept eligibility envelope and
//! transfer policy; the boundary refuses an unknown authorization or an
//! unknown eligibility field rather than inventing a value; and the
//! generation-stamped [`interaction::InteractionActorBinding`] ECS record.
//!
//! [`objectives`] is the F39 trigger consumer
//! (`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! which owns trigger semantics): [`objectives::SpawnTickTriggerPlugin`] and
//! [`objectives::TriggerCrossings`], the one place the swept spawn preflight's
//! sensor record becomes a gameplay [`objectives::TriggerCrossing`] — a trigger
//! a body crossed inside the tick it spawned in, which the engine reports
//! nothing for because a sensor must not stop or delay a spawn (F23-C) and a
//! freshly spawned body is not yet in the broad phase (F23-B). The crossing is
//! decided from the body's own swept motion, delivered once per
//! `(actor, volume)` pair on the tick it happened, and the delivery is a pure
//! read: it cannot move a body. The decision, its measurement and the
//! boundaries of the claim are in
//! `docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md`.
//! Stage `### F39-C` adds the mission-program wiring (the same module):
//! [`objectives::lower_program`], which lowers a validated
//! `cs_content::objectives::DeclaredObjectiveProgram` into the
//! [`objectives::LoweredObjectives`] a session launches — every
//! `Resolved::Unknown` and every non-gameplay timer domain refused by name —
//! and [`objectives::ObjectiveSession`], the producer→runtime→consumer path
//! that feeds the `cs_sim::objectives` runtime each tick's `TickInput` and
//! dispatches the ordered event stream to the spawn world, the dialogue cue
//! queue and the objective display, plus the [`objectives::ObjectiveSession::retry`]
//! teardown/retry contract that returns what the old generation still owned
//! (live wave actors, undrained cues, armed deadlines) so nothing survives
//! into the new one.
//!
//! [`cinematics`] is the F40-A cinematic boundary
//! (`specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`, stage
//! `### F40-A`): [`cinematics::lower_cinematic`], which lowers a declared
//! `cs_content::cinematics::DeclaredCinematic` into a
//! `cs_sim::cinematic_state::CinematicScript` and refuses an unknown value
//! rather than inventing one; [`cinematics::begin`], which turns missing media
//! into a `Failed` player rather than a completion; and
//! [`cinematics::fit_letterboxed`], which fits a frame without stretching it.
//!
//! [`audio`] is the F41-A audio boundary
//! (`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`): [`audio::lower_record`] and [`audio::lower_catalog`], which
//! lower a declared `cs_content::audio::AudioAssetRecord` into the
//! `cs_sim::audio_events::AudioAssetSpec` the router schedules, refusing an
//! unknown bus, level or playback mode by claim instead of inventing one; and
//! the generation-stamped [`audio::AudioEmitterBinding`] tying an entity to its
//! session-qualified emitter, bus and asset. F41-B adds [`audio::sync_emitter_loops`],
//! which starts, swaps and stops loops as those bindings spawn and despawn;
//! [`audio::smooth_engine_voices`], which smooths engine pitch and volume from
//! the flight model's fixed-tick throttle spool; [`audio::mix_session`] and the
//! [`audio::device_lost`] / [`audio::device_restored`] pair, the consumer that
//! carries the session's outcomes and the spatial law to an output device while
//! simulation stays independent of it; and [`audio::AudioPlugin`], whose
//! [`audio::insert_audio_session`] lets the F15 loading handoff own the session
//! a delivered load implies. Loop regions stay unknown and the attenuation law
//! is designed rather than measured; see
//! `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`.
//!
//! [`ui::front_end`] is the F45-A front-end state table
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, stage
//! `### F45-A`): every screen from install selection to results, each action's
//! transition, the resources it acquires and releases, the draft/confirm rule
//! for Back, and [`ui::front_end::FrontEnd`], which requests domain
//! transactions and never edits campaign or profile fields itself.
//!
//! [`ui::hud`] is the F46 instrument projection
//! (`specs/F46-hud-instruments-mission-map-and-pause.md`, stages `### F46-A`
//! and `### F46-B`): [`ui::hud::attitude`] turns an attitude quaternion into
//! horizon and heading, [`ui::hud::Hud`] converts an SI aircraft sample into
//! gauge values under a `cs_content::hud::HudPolicy`, keeps the low-altitude
//! warning, and refuses a sample of any session or aircraft it is not bound
//! to — and [`ui::hud::Hud::frame`] projects the whole HUD (instruments, gun
//! gauge, airframe panel, launcher cluster and target display) from the
//! session's own weapon, ordnance, damage and target authorities, refusing a
//! source stamped for another session generation.
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
//!
//! [`accessibility`] is the F52-A boundary
//! (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`, stage
//! `### F52-A`): keyboard-only and controller-only front-end navigation, a
//! remap session that cannot strand a device, colour-independent objective
//! cues, reduced shake/flash that keeps required notifications, and atomic
//! settings persistence with a safe-defaults startup. The settings types are
//! `cs_content::settings`.
//!
//! [`network`] is the F57-A networked-aircraft boundary
//! (`specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`,
//! stage `### F57-A`): [`network::physics`] is the client-side ingest path that
//! turns one decoded [`cs_net::snapshot::Snapshot`] plus the session's live
//! world-origin epoch into the [`network::physics::RemoteMirror`] of remote
//! aircraft, refusing a foreign epoch, an out-of-order tick, an ended
//! generation and an uninterpretable record by name — and, because sequenced
//! snapshots are droppable, never treating an actor's absence from a snapshot as
//! a despawn, so a lossy or reordered link leaves neither a duplicate
//! destruction nor a permanent ghost aircraft. The server-side authority for the
//! same schema (generations, the once-per-generation destruction gate, input
//! acknowledgment) is [`cs_sim::net_state`]; the schema, quantizers and declared
//! error budgets are [`cs_net::snapshot`]. Interpolation and bounded local
//! prediction are F57-B, reconciliation wiring F57-C.
//!
//! [`capture`] is the F59-B runtime half of replay, capture and acceptance
//! evidence
//! (`specs/F59-replays-captures-probes-and-acceptance-evidence.md`, stage
//! `### F59-B`): [`capture::identity`] computes the engine/content/rules
//! digests a `cs_content::replay::ReplayRecord`'s build fingerprint is built
//! from out of the content a run actually loaded, [`capture::state`] is the
//! only producer of per-tick state hashes in the runtime (a pose read back from
//! Avian plus the forces the tick's own law computed — never a hash of the
//! input), [`capture::replay`] drives the production flight world from a
//! recorded `cs_types::input::CommandStream` and reports AC01's envelope
//! comparison beside AC02's compatibility verdict, and [`capture::render`] is
//! the single boundary between a capture record's exact
//! `cs_content::replay::RenderConfig` and the renderer's `f32`
//! [`render::capture::ComparisonSettings`]. The record schema itself is F59-A's
//! `cs_content::replay`; the commands that drive this path and write evidence
//! are F59-C and F59-D.

pub mod accessibility;
pub mod ai;
pub mod airframe_visual;
pub mod animation;
pub mod asset_stack;
pub mod assets;
pub mod audio;
pub mod camera;
pub mod campaign;
pub mod capital;
pub mod capture;
pub mod cinematics;
pub mod cli;
pub mod construction;
pub mod control_lowering;
pub mod damage;
pub mod debris;
pub mod diagnostics;
pub mod environment;
pub mod input;
pub mod interaction;
pub mod livery;
pub mod loading;
pub mod mission_animations;
pub mod mission_control;
pub mod mission_markers;
pub mod mission_start;
pub mod network;
pub mod objectives;
pub mod ordnance;
pub mod origin;
pub mod physics;
pub mod playtest;
pub mod playtest_retail;
pub mod playtest_textures;
pub mod profile;
pub mod render;
pub mod roster;
pub mod run;
pub mod scene;
pub mod stunts;
pub mod synthetic;
pub mod targeting;
pub mod text;
pub mod ui {
    //! Screens and their state machines.
    //!
    //! [`instant_action`] is the F49-A Instant Action selection and
    //! scenario-lowering boundary
    //! (`specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
    //! `### F49-A`): a [`instant_action::ScenarioSelection`] naming content ids
    //! only, lowered through the declared
    //! `cs_content::instant_action` schema into the
    //! [`instant_action::LoweredScenario`] a session spawns from. Every
    //! `Resolved::Unknown` refuses there rather than becoming a default, and
    //! the type has no campaign cash, ownership or objective field, so an
    //! Instant Action run cannot write campaign progression (F49
    //! non-negotiable 3).
    //!
    //! [`mods`] is the F53 mod screen and the host half of a mod mount
    //! (`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
    //! stages `### F53-B` and `### F53-C`): [`mods::ModsView`] projects the
    //! host's `cs_content::mods::ModSelection` and its mount outcome into
    //! rows and refusal notices, [`mods::lobby_compatibility`] folds the
    //! mounted set's compatibility signature into the `cs_net` handshake
    //! record so a tuning mod can never silently share a lobby with stock
    //! content (F53 AC03), and [`mods::mount_selection`] attaches the
    //! bounded validator a sandboxed mission payload has to pass before
    //! `cs_content::mods::mount_mods` will enable it (F53 non-negotiable 2).

    pub mod front_end;
    pub mod hud;
    pub mod instant_action;
    pub mod lobby;
    pub mod mods;
    pub mod scrapbook;
}
pub mod weapons;
pub mod world;
pub mod world_actors;
