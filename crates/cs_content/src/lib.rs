//! Normalization, catalog, blueprints, localization and saves.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependencies: [`cs_types`],
//! [`cs_formats`] and, where a normalization needs the filesystem,
//! [`cs_assets`]. It must never depend on Bevy or Avian.
//!
//! The stage paragraphs below stay in ascending feature-sheet order; the
//! insertion rule and its rationale are `docs/architecture/crate-module-docs.md`.
//!
//! [`loading`] is the F07-C work: the loading-plan adapter
//! (`specs/F07-interp-loading-script-container.md`, `### F07-C`). It reads a
//! container `cs_formats::decode_interp` validated, classifies its lines with
//! `cs_formats::plan_interp_loading` and resolves the registered loading
//! commands through a content session, producing the world's dependency closure
//! and the lines that fail it. It ships no command registrations: which
//! commands load resources is F07-D's measurement, so until then every line is
//! unclassified and every world's plan fails with the line's source offset and
//! the world it affects rather than reporting a loaded state.
//!
//! [`textures`] is the F08-C wiring
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`,
//! `### F08-C`): texture archives resolved through a content session, the
//! image catalog they populate, the handoff to the GPU upload boundary, and
//! the measured rule that picks which of a world's `texture.zbd` /
//! `rtexture*.zbd` tiers a world load opens, and the measured fold a requested
//! texture name is searched under
//! ([`textures::folded_texture_name`]).
//!
//! [`detail`] is the detail-settings half of that stage (task #688): the
//! surface that owns the `TextureMemory_HW`/`TextureMemory_SW` members and the
//! video panel's texture dropdown with their measured conversions, authored as
//! [`ClaimStatus::Designed`](cs_types::evidence::ClaimStatus) until the
//! `detail.zrd` member is bound by a measured task.
//!
//! [`livery`] is the F09 paint composition
//! (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`): the three
//! mask colors of a BM livery, the deterministic cache key of a composed
//! variant and the composed RGB8 image (stage F09-B), plus the
//! session-scoped [`livery::LiveryVariantStore`] that caches those variants
//! keyed by every input that distinguishes them (stage F09-C).
//!
//! [`mesh`] is the F10-C render mesh
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`,
//! `### F10-C`): the Bevy-free canonical render mesh built from F10-B's raw
//! GameZ mesh IR and its topology, the material/texture dependency audit, the
//! F10-C.03 container-to-upload wiring, and F10-C.04's
//! [`mesh::measure_bindings`], which *measures* which texture archive a
//! container's materials bind to under five name readings and reports the
//! weakest decision those numbers support — without touching the audit's own
//! exact-name rule, and without resolving a tie it cannot settle. Its candidates
//! are the caller's, so a measurement's search space is part of its evidence.
//! One render vertex per distinct
//! `(position index, normal index, uv, color, material)` tuple, compared
//! bit-exactly, so a shared position with different per-corner UVs keeps the
//! authored seam; every render vertex and triangle keeps its source corner
//! or step; an incomplete topology is refused naming each rejected face's
//! `FaceIssue` code; and triangles are grouped by their raw material index,
//! never interpreting polygon flags. Its F10-C.03 half is the wiring: a
//! GameZ container opened through a [`cs_assets::vfs::ContentSession`] and
//! [`cs_assets::zbd::ZbdContainer`], read by both production section readers
//! and cross-checked against each other's header, producing a
//! [`mesh::RenderMeshRecord`] catalog row per stored mesh — failed meshes and
//! failed containers included, each keeping the reader's own container, field
//! and byte offset — and a [`mesh::MeshUpload`] payload that owns its data and
//! survives the close of the session that read it. The consumer boundary is
//! where this stops: the canonical-mesh-to-Bevy adapter is F17-B's.
//!
//! [`scene`] is the F11-A scene contract
//! (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`,
//! `### F11-A`): the typed [`scene::ParsedNode`] input a node-array reader
//! produces, the [`scene::SceneGraph`] conversion into stable
//! [`scene::SceneNodeId`] records (rejecting cycles, dangling parents and
//! ambiguous roots), canonical [`scene::CanonicalTransform`] composition that
//! preserves nested transforms and negative scale, and the evidence-backed
//! [`scene::BindingMap`] semantic-binding records. Task #392 adds the half that
//! turns a real container into those records:
//! [`scene::parsed_nodes_from_gamez`] maps
//! `cs_formats::gamez::read_gamez_nodes`'s output onto [`scene::ParsedNode`]
//! without inventing anything the store has not earned — an authored name
//! crosses over verbatim, an object record keeps the stored matrix exactly when
//! it disagrees with its own euler triple, a LOD near bound is resolved from
//! the square the record stores, and a `mesh_index` the supplied
//! [`scene::MeshSlot`] catalog cannot answer stays an explicit
//! `Resolved::Unknown` — and [`scene::scene_graph_from_gamez`] runs the
//! hierarchy build on top. The store's records, the typed records and the
//! canonical graph stay three separate steps, so a container that reads but
//! does not convert is reported as exactly that
//! ([`scene::GameZSceneError::Build`]). The layout and its measurements are in
//! `docs/findings/2026-10-02-gamez-node-array-layout.md`.
//!
//! [`config`] holds lossless configuration documents with provenance and
//! key accounting (`specs/F12-text-configuration-strings-and-pe-
//! resources.md`, stage F12-A), the typed, checked conversion of a
//! declared field into a tuning constant (stage F12-B) and the F12-C
//! consumers: a value becomes one only against a [`config::FieldSpec`] that
//! declares its width, signedness and approved range — a negative,
//! overflowing or non-finite value never does — [`config::resolve_tunings`]
//! resolves a list of declared fields against a document, and
//! [`config::StringCatalog`] resolves localizable string ids and languages
//! through the PE resource reader.
//!
//! [`catalog`] holds the canonical content catalog and its declared
//! launchable baseline (`specs/F14-canonical-content-catalog-and-dependency-
//! closure.md`, stages F14-A/F14-B): stable-id elements in canonical order,
//! duplicate identities refused, and the unsupported-mission count that
//! keeps an unavailable mission in the denominator. F14-B adds the
//! [`catalog::normalize`] quantity normalizer (canonical units, approved
//! ranges and explicit refusals) and the [`catalog::closure`] transitive
//! dependency walk (per-edge provenance, propagated unsupported
//! dependencies, orphaned references, ownership cycles and a deterministic
//! hash and JSON report). Its F14-D stage adds
//! [`catalog::baseline`]: the complete private baseline inventory read from
//! the original installation — one row per inventoried file, one row per
//! campaign mission program and one declared launchable row per campaign
//! mission directory, so the coverage denominator comes from the
//! installation instead of a filtered list of supported rows.
//!
//! [`mission_control`] is the measured mission control program (task
//! `M01-LC-MISSION-PROGRAM`, #630): which reader-archive member carries a
//! mission's control program — decided by
//! [`mission_control::control_member`], the rule that the member is the one whose
//! decoded record declares numbered `OBJECTIVE<N>` blocks, not the one with the
//! longest or the most suggestive name — every directive key its blocks spell
//! with that key's measured argument shape, and a
//! [`mission_control::DirectiveDisposition`] per key: the one mission-IR action
//! a key can reach, the measured effect a stage A/B/C/D finding supplies
//! ([`mission_control::measured_directive`], residual unknowns included), or a
//! refusal with a named [`mission_control::UnmeasuredReason`].
//! [`mission_control::ControlLowering`] then accounts, requirement by
//! requirement, for what `cs_script::bindings::lower_program` would still need
//! before any of it could become a `MissionProgram`. Measured is not support:
//! a key whose effect is known still has no host binding, and no original
//! executable has been run.
//!
//! [`coordinates`] holds source coordinate conventions and their adapters
//! into canonical space (`specs/F16-coordinates-units-origin-management-and-
//! clocks.md`, stage F16-A): one validated declaration per source, and every
//! position, direction, normal, rotation, winding, distance and angle
//! conversion derived from it, so a format maps into the canonical
//! convention exactly once. The declared sources are designed declarations
//! with `Origin`/`Provenance`; which convention an original file uses is
//! unmeasured (F16-D) and is never asserted here.
//!
//! [`world`] is the F18-A world contract
//! (`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! `### F18-A`): one authored [`world::WorldDefinition`] holding its sectors,
//! its object instances and its declared boundary, and one concrete
//! [`world::WorldInstance`] load record carrying the variant, the object
//! population and the initial damage a mission applies — so reusing a world
//! for another mission is a record, never a leftover (F18 non-negotiable
//! behavior 5). Each instance keeps the render mesh reference, the authored
//! transform, the collision role, the collision shape and the gameplay
//! surface role as **separate roles over one record**, and every role arrives
//! as [`Resolved`], so an unevidenced role stays an explicit unknown instead
//! of a guessed default. The records are Bevy-free inputs for F18-B's import
//! and static-collision generation; no collider, file or renderer is touched
//! here.
//!
//! [`environment`] is the F19-A environment contract
//! (`specs/F19-sky-atmosphere-weather-and-visibility.md`, stage `### F19-A`):
//! one authored [`environment::EnvironmentDefinition`] that separates sky
//! art, sky orientation, fog, lighting, cloud layers, precipitation, wind
//! and gameplay visibility into their own records, a renderer default that
//! is tagged `designed`, a gameplay visibility that is never derived from
//! fog, and an [`environment::EnvironmentTimeline`] of weather changes at
//! whole simulation ticks. Which time domain those ticks belong to is
//! decided in `cs_sim::visibility`, because this crate must not depend on
//! `cs_sim`. No sky is rendered and no file is opened here; F19-B builds the
//! sky/fog/light and weather effects from these records.
//!
//! [`weather`] is the mission `weather.zrd` reader and its binding to an
//! [`environment::EnvironmentDefinition`] (task #636): a strict typed reader
//! of the decoded `.zrd` tree that keeps the file's own number spellings, and
//! a binder that makes only the precipitation kind known and names every
//! other field it leaves unbound, because the original's world-unit scale and
//! angle conventions are unmeasured.
//!
//! [`animation`] is the F20-A declared animation IR
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stage
//! `### F20-A`): the provenance-carrying [`animation::AnimationClip`] record
//! with its transform, visibility, material and attachment channels and the
//! [`animation::EventMarker`]s whose unknown effects block rather than skip
//! gameplay transitions. Its runtime counterpart is
//! `cs_sim::animated_object`; the conversion boundary is
//! `cs_app::animation`.
//!
//! [`cameras`] is the F21-A/F21-B declared camera contract
//! (`specs/F21-cameras-cockpit-views-and-spyglass.md`, stages `### F21-A` and
//! `### F21-B`):
//! the provenance-carrying [`cameras::DeclaredCameraModes`] set of
//! [`cameras::DeclaredCameraMode`] records (a designed
//! [`cameras::CameraModeKind`] vocabulary, each mode's
//! [`cameras::ProjectionPolicy`], its [`cameras::Magnification`], its
//! target-tracking flag, its [`cameras::DeclaredPlacement`] — a
//! [`cameras::CockpitViewpoint`] bound to a named
//! [`cameras::CockpitBindingSource`] or a [`cameras::BodyOffset`] — and its
//! [`cameras::LookLimits`]) and the [`cameras::AspectFraming`] rule. A
//! cockpit mode may only declare a bound viewpoint, so an aircraft with no
//! verified cockpit binding declares no cockpit view at all. Every
//! load-bearing value is a `Resolved` and the original PC view list, field
//! of view, axis, clipping planes, magnification, cockpit bindings and look
//! range are unmeasured, so the synthetic fixture is designed content and
//! never an original measurement. Its runtime counterpart (the lowered
//! records, the projection math and the rigs) is `cs_app::camera`.
//!
//! [`flight_tuning`] is the F24-A provenance-carrying tuning schema
//! (`specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`, stage
//! `### F24-A`): every numeric field the `cs_sim::flight` equations consume,
//! with its unit and approved range, and a declared synthetic airframe whose
//! values are each known with provenance or an explicit unknown — never a
//! silent zero. It carries no original coefficient; F24-C maps the record
//! into the model and F24-D calibrates it against reference traces.
//!
//! [`airframe_roles`] is the F25-A role record
//! (`specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stage `### F25-A`): what one airframe *is* — its control-law label, its
//! roster presence versus ordinary menu availability, and its explicit
//! controllability, launch and weapon constraints — plus the pure
//! [`airframe_roles::AirframeRoles::resolve_launch`] that decides which
//! airframe a session launches, so a mission's forced assignment overrides the
//! player's hangar selection for that session without writing to the owned
//! loadout. Its declared roster is synthetic and its rotor mapping is an
//! explicit unknown; F25-C wires it into the runtime launch path.
//!
//! [`weapons`] is the F27-A declared weapon schema
//! (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-A`): the provenance-carrying
//! [`weapons::DeclaredGunDefinition`] whose `mount` is the same
//! `crate::damage::DamageNodeKey` the declared damage graph disables and
//! whose every ballistic parameter — caliber, ammunition, rate, muzzle
//! velocity, lifetime, spread, per-channel damage, inheritance rule,
//! effect and sound — is a separate [`weapons`] `Resolved` value, never a
//! silent default; the [`weapons::AmmunitionId`] that keeps ammunition an
//! opaque `ammo` catalog id instead of an invented enum; the
//! [`weapons::InteractionRules`] whose self-hit, friendly-fire, penetration,
//! ricochet and ammo-switching options each refuse to lower while unknown,
//! and whose [`weapons::InteractionOption`] /
//! [`weapons::InteractionRules::deferred`] make "declared but not applied"
//! a queryable fact of the schema: self-hit and friendly fire are applied by
//! `cs_sim::weapons::WeaponRules::admit_candidates`, while penetration,
//! ricochet and ammo switching are deferred to F27-D with their reasons, so
//! the gap is visible to an audit without reading a findings file;
//! and the [`weapons::DeclaredLoadout`] whose gun/ammunition pairings are
//! the rows F27-D's ammunition audit walks. Its runtime counterpart is
//! `cs_sim::weapons`; the conversion boundary is `cs_app::weapons`.
//!
//! [`ordnance`] is the F28-A declared ordnance schema
//! (`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`, stage
//! `### F28-A`): the provenance-carrying
//! [`ordnance::DeclaredOrdnance`] whose launch geometry, stack capacity and
//! mass, arming, fuse, guidance, lost-target rule, lifetime, area effect,
//! per-channel damage, status effects and media are each a separate
//! [`ordnance`] `Resolved` value, never a silent default; the
//! [`ordnance::DeclaredOrdnanceFamily`] vocabulary whose six names are the
//! sheet's discovery leads rather than a catalogue, with the family required
//! to agree with the declared [`ordnance::DeclaredOrdnanceDetails`]; the
//! [`ordnance::DeclaredNitro`] booster, kept separate from the projectile
//! record because a booster has no fuse, no lifetime and no blast; and the
//! [`ordnance::DeclaredEquipmentRules`] that loadout validation and an
//! import both read, so an unsupported component cannot reach a session by
//! way of an import. Its runtime counterpart is `cs_sim::weapons::ordnance`;
//! the conversion boundary is `cs_app::ordnance`.
//!
//! [`damage`] is the F29-A declared damage schema
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`): the provenance-carrying [`damage::DeclaredDamageGraph`]
//! of armor zones, internal structure, engines and weapon mounts with
//! [`Resolved`] integrity pools, [`scene::SceneNodeId`] part bindings, per-subject
//! declared rules (aircraft, world object and capital ship share the
//! identity discipline but keep their own rules) and the lethal
//! [`damage::AttributionRule`] a session resolves kills under — every value
//! known with provenance or an explicit unknown, never a silent default.
//! Its runtime counterpart is `cs_sim::damage`; the conversion boundary is
//! `cs_app::damage`.
//!
//! [`target_rules`] is the F30-A/F30-B/F30-C declared targeting schema
//! (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stages `### F30-A`/`### F30-B`/`### F30-C`): the provenance-carrying
//! [`target_rules::DeclaredTargetRules`] of a subject's faction set,
//! directed [`target_rules::DeclaredRelation`]s and the
//! [`target_rules::TargetRuleSet`] policy knobs — threat window, crosshair
//! cone and the separate lead-indicator/aim-assistance options — each
//! [`Resolved`] known with provenance or an explicit unknown, never a
//! silent default, plus F30-B's
//! [`target_rules::DeclaredSelectionActions`], the command-edge table an IA
//! preset declares and the runtime binding lowers from. F30-C adds no record
//! here: the guidance consumer reads these same two options, and the
//! [`Provenance`] each carries is the evidence classification F30
//! non-negotiable 3 requires. Its runtime counterpart is `cs_sim::targeting`;
//! the conversion boundary and the HUD/spyglass/guidance views are
//! `cs_app::targeting`.
//!
//! [`routes`] is the F31-A declared route graph
//! (`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-A`): the provenance-carrying [`routes::RouteDefinition`] with its
//! stable [`routes::RouteNodeId`]s and authored sequences, its
//! [`routes::TriggerVolume`]s, its [`routes::ReferenceFrame`] (world or a
//! moving carrier/train/escort) and its adjacency-only [`routes::RouteEdge`]s,
//! so an authored route can never encode a shortcut past a mandatory marker.
//! Its runtime counterpart is `cs_sim::ai::navigation`; the conversion
//! boundary is F31-C. The original route encoding is undecoded, so every value
//! here is designed or an explicit unknown, never an original route.
//!
//! [`ai`] is the F32-A declared combat-AI contract
//! (`specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-A`): the declared [`ai::DeclaredCombatRole`] vocabulary, the
//! [`ai::SkillKnobs`] and [`ai::PriorityPolicy`] a role runs under — every
//! value a [`cs_types::content::Resolved`] with its own provenance, never a
//! silent default — the *closed* [`ai::SkillKnob`] vocabulary a variant or a
//! difficulty tier may move (deliberately no damage, health or
//! simulation-rate knob, so an ace is a behavior variant and difficulty
//! cannot fake itself with a faster simulation), the data-driven
//! [`ai::DeclaredAceProfile`], the [`ai::DeclaredFormation`] with one
//! [`ai::RecoveryPolicy`] per recovery trigger, and the
//! [`ai::DifficultyProfile`]s. Its runtime counterpart is
//! `cs_sim::ai::combat`; the conversion boundary is `cs_app::ai::combat`
//! (F32-B roles and priority, F32-C aces, difficulty and formations).
//!
//! [`pilots`] is the F33-A declared pilot/aircraft/faction roster
//! (`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stage `### F33-A`): the provenance-carrying [`pilots::DeclaredRoster`] of
//! a subject's [`pilots::DeclaredPilot`]s (identity and voice), its
//! [`pilots::DeclaredWingmate`] slots (pilot, airframe, loadout and
//! [`pilots::DeclaredSurvivability`]) and its [`pilots::DeclaredNeutralTraffic`].
//! Pilot, aircraft and faction are separate catalog namespaces, so a faction
//! change can never rename a vehicle (AC01); neutral traffic is authored per
//! mission, so an omitted list is empty, never a population default (AC04).
//! Its runtime counterpart is `cs_sim::allies`; the conversion boundary is
//! `cs_app::roster`.
//!
//! [`capital`] is the F35-A declared capital-ship schema
//! (`specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`, stage
//! `### F35-A`): the provenance-carrying [`capital::DeclaredCapitalShip`]
//! with its authored trajectory, engines, gas/structural sections, turrets,
//! weapon bays, launch bays with socket transforms, docking anchors, cargo
//! and ownership. Identity is the [`capital::CapitalSubsystemKey`]; the
//! shared [`capital::DeclaredSubsystem`] list pairs each part's kind with
//! the behavior its destruction changes, and every load-bearing value is a
//! [`Resolved`], so an unmeasured thrust, socket or owner stays an explicit
//! unknown. Its runtime counterpart is `cs_sim::capital`; the lowering
//! boundary is `cs_app::capital`.
//!
//! [`interaction`] is the F36-A declared docking/pickup/boarding/plane-swap
//! schema (`specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`): the provenance-carrying [`interaction::DeclaredInteraction`]
//! naming its [`interaction::DeclaredInteractionKind`], the objective that
//! authorizes it, its [`interaction::DeclaredEligibility`] envelope and its
//! [`interaction::DeclaredTransferPolicy`], every load-bearing value a
//! [`cs_types::content::Resolved`]. Its runtime counterpart is
//! `cs_sim::interaction`; the conversion boundary is `cs_app::interaction`.
//!
//! [`objectives`] is the F39-C declared objective-program schema
//! (`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-C`): the provenance-carrying
//! [`objectives::DeclaredObjectiveProgram`] of one mission — its
//! [`objectives::DeclaredObjective`]s with initial states and
//! [`objectives::DeclaredRevealRule`]s, its [`objectives::DeclaredCondition`]s
//! (a roster, one [`objectives::DeclaredCountKind`], a required count and a
//! [`objectives::DeclaredCountReaction`]), its [`objectives::DeclaredTimer`]s
//! with declared starts, domains and single [`objectives::DeclaredTimerAction`]s,
//! its [`objectives::DeclaredTrigger`] volumes and its
//! [`objectives::DeclaredSpawnGroup`] bindings — plus the declared terminal
//! [`objectives::DeclaredPrecedence`] kept a `Resolved` because the original
//! rule is unmeasured. The schema is closed: a declaration may only name a
//! declaration of the same program, so a dangling objective, condition, timer
//! or spawn-group reference is refused at declaration rather than waiting to
//! be caught in use. Its runtime counterpart is `cs_sim::objectives`; the
//! conversion boundary is `cs_app::objectives`.
//!
//! [`cinematics`] is the F40-A declared cutscene/video schema
//! (`specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`, stage
//! `### F40-A`): the provenance-carrying [`cinematics::DeclaredCinematic`]
//! keeping its [`cinematics::DeclaredPresentation`] (prerendered video or
//! in-engine camera track) apart from its [`cinematics::DeclaredAction`]
//! semantic actions, every load-bearing value a
//! [`cs_types::content::Resolved`]. Its runtime counterpart is
//! `cs_sim::cinematic_state`; the conversion boundary is `cs_app::cinematics`.
//!
//! [`audio`] is the F41-A declared audio catalog
//! (`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`): the provenance-carrying [`audio::AudioAssetRecord`] binding an
//! audio id to its [`audio::AudioPlayback`] metadata (one of the seven
//! [`audio::AudioBus`]es, a validated [`audio::AudioLevel`] and a
//! [`audio::PlaybackMode`]) and to a [`audio::DecodedPcm`] reference, with the
//! [`audio::AudioCatalog`] that refuses a duplicate id. Its runtime counterpart
//! is `cs_sim::audio_events`; the conversion boundary is `cs_app::audio`. The
//! original audio pipeline is undecoded, so every value here is designed or an
//! explicit unknown, never an original measurement.
//!
//! [`construction`] is the F44-A construction record
//! (`specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-A`): the [`construction::AircraftBlueprint`] input every
//! construction path shares, the per-airframe
//! [`construction::ConstructionRules`] limit profile, the
//! [`construction::PriceBook`] of declared component masses and prices, and the
//! **exact integer** budget arithmetic — [`construction::WeightUnits`] game
//! weight units and [`construction::MoneyMinor`] minor currency, with no float
//! anywhere — that returns a [`construction::BlueprintAssessment`] whose
//! verdicts compare integers only. Non-negotiable 1's observed four gun
//! positions and eight rocket hardpoints are per-profile `Resolved` data rather
//! than constants, and an unmeasured price, limit or gun-position cost is a
//! named refusal instead of a loadout that quietly fits. The validator's
//! constraint rules, the transactional purchase/sell draft and the preview are
//! F44-B's and F44-C's.
//!
//! [`ui_layout`] is the F45-A authored-screen layout
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, stage
//! `### F45-A`): a validated [`ui_layout::ScreenLayout`] of logical hotspots and
//! the integer [`ui_layout::AspectFit`] that maps image and hotspots with one
//! scale and offset. The state table that consumes it is
//! `cs_app::ui::front_end`.
//!
//! [`hud`] is the F46-A display-unit policy (`specs/F46-hud-instruments-mission-map-and-pause.md`,
//! stage `### F46-A`): a [`hud::HudPolicy`] naming each gauge's unit, the speed
//! it reads, the altitude datum and the low-altitude thresholds, every choice
//! tagged designed or original-verified. The instrument projection that uses it
//! is `cs_app::ui::hud`.
//!
//! [`instant_action`] is the F49-A declared preset and custom-scenario schema
//! (`specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-A`): the [`instant_action::InstantActionCatalog`] with its presets
//! and the closed [`instant_action::ScenarioOptions`] a custom scenario may
//! select, the shared parameter set a preset and a
//! [`instant_action::CustomScenarioRequest`] both carry, and
//! [`instant_action::InstantActionCatalog::validate_custom`]'s full list of
//! actionable problems for an impossible roster. The lowering boundary to the
//! simulation is `cs_app::ui::instant_action`.
//!
//! [`campaign_bindings`] holds the engine-independent mission binding and
//! campaign coverage records (`specs/F50-per-mission-compatibility-and-
//! full-campaign-closure.md`, stage F50-A): the seven required content
//! categories the F50 owner ruling preserves, one explicit unresolved
//! dependency row per required subsystem, the frozen campaign denominator
//! read from `missions/bindings/campaign-inventory.tsv`, and the coverage
//! totals plus closure reports that keep a missing, unknown or unsupported
//! child counted instead of ready. Its M01-A stage adds the first
//! source-derived binding: `SourceContext` reads the original installation's
//! fingerprint, campaign directory layout and localized string table, and
//! `SourceBinding` resolves the five critical dependencies of the mission
//! sheets' data-binding checklist while keeping every unbound checklist
//! entry in its `unknowns`. It claims no gameplay success and no
//! `verified_original` state; running missions stays with the runtime stages.
//!
//! [`localization`] is the F51-A declared localization contract
//! (`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-A`): the opaque, validated [`localization::LocaleId`] and
//! the *explicit* [`localization::LocaleChain`] a resolution walks (never a
//! defaulted language list, because the original supported locales are
//! unmeasured), a [`localization::TextCatalog`] of locale-bound rows keyed by
//! the locale-independent [`localization::TextId`] that reports which locale
//! and which chain depth answered — and names every locale it tried when none
//! did — the declared control-markup grammar
//! ([`localization::MarkupGrammar`] and [`localization::parse_markup`]) that
//! keeps a refused control as literal text plus a [`localization::MarkupIssue`]
//! instead of interpreting a resource string as executable markup, and the font
//! provenance ([`localization::FontFace`], [`localization::FontSource`]) that
//! can only hold an original private font or a licensed fallback with verified
//! permission, plus the [`localization::GlyphCoverage`] that *counts* the
//! characters a font cannot render. Its application boundary is
//! `cs_app::text`.
//!
//! [`settings`] is the F52-A accessibility settings and fidelity boundary
//! (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`, stage
//! `### F52-A`): [`settings::Presentation`] that cannot reach the simulation,
//! the separate [`settings::ModernProfile`] of gameplay assists, the
//! [`settings::FidelityLabel`] replay metadata records and the strict
//! persisted text form. The application side is `cs_app::accessibility`.
//!
//! [`mods`] is the F53 mod manifest, mount-plan and mount contract
//! (`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stages `### F53-A` and `### F53-B`): the typed [`mods::ModManifest`] a
//! manifest source produces, the [`mods::ContentOverride`]s a mod claims
//! with the two
//! policies that classify them (an override's gameplay effect and which
//! validator its payload must pass are both computed from the target's
//! content kind, never asserted by the author), and the deterministic
//! [`mods::ModPlan`] over a set of mods — a visible load order, a
//! [`mods::PrecedenceReport`] naming the winner and every shadowed claim of
//! each contested content id, a gameplay/cosmetic verdict and a
//! [`mods::PlanProblem`] list for every way a set can be refused. The load
//! order is the same [`cs_types::asset_id::ModStack`] a resolve context opts
//! into, so "the later mod wins" is one rule rather than two.
//! [`mods::mount_mods`] is the F53-B half that touches bytes: it plans
//! first, so a cycle or any other plan problem is refused before a root is
//! opened, then walks each mod's root through [`cs_assets::mods::ModRoot`],
//! re-validates every declared source at the join, refuses a payload only a
//! symbolic link would satisfy, re-checks the byte budgets against the
//! **measured** sizes, gates mission and script payloads behind the host's
//! bounded [`mods::ProgramValidator`] (no validator, no mount) and returns
//! a [`mods::MountedMods`] whose compatibility signature covers the
//! resolved content bytes. F53-C is the selection UI and export tooling,
//! F53-D the reproducibility evidence.
//!
//! [`multiplayer`] is the F56-A original multiplayer catalog
//! (`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-A`): [`multiplayer::discover_modes`] reads the modes the localized
//! string table names and their briefings, [`multiplayer::discover_slots`]
//! inventories the `MP<n>` scenario slots of every world group and binds each
//! to the [`multiplayer::ScenarioMode`] family its own decoded `targets.zrd`
//! names, and what the installation does not state (every per-mode rule, and
//! which deathmatch variant a [`multiplayer::ScenarioMode::Deathmatch`] slot
//! launches under) stays an explicit
//! [`cs_types::content::Resolved::Unknown`].
//!
//! [`replay`] is the F59-A replay, capture and evidence schema
//! (`specs/F59-replays-captures-probes-and-acceptance-evidence.md`, stage
//! `### F59-A`): the [`replay::ReplayRecord`] that carries the engine, content
//! and rules digests, the initial state, the recorded command stream, the
//! seeds, the tick rate and the pinned authored choices; the
//! [`replay::StateEnvelope`] of promised per-tick state hashes with the
//! comparison AC01 asks for; the [`replay::CompatibilityVerdict`] that rejects
//! a cross-build replay (or records it as explicitly best-effort) and refuses a
//! replay whose content changed (AC02); the [`replay::CaptureRecord`] that
//! pins the exact camera, tick, render configuration and build hash (AC03); and
//! the [`replay::EvidenceBundle`] whose capability table turns a missing device
//! into a blocked claim rather than a silent pass (AC04). It simulates,
//! renders and opens nothing: the capture path is `cs_app::capture` (F59-B) and
//! the commands are `cs-inspect`/`cs_xtask` (F59-C).
//!
//! [`legacy_import`] is the F64-A legacy-import contract
//! (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, stage
//! `### F64-A`): [`legacy_import::plan_import`] turns one read-only legacy
//! source into an [`legacy_import::ImportPlan`] or an
//! [`legacy_import::ImportRefusal`], classifying the result as full, partial
//! or unsupported and retaining the source fingerprint and the unresolved rows
//! as a [`legacy_import::MigrationReport`]. Legacy ids resolve through
//! [`legacy_import::LegacyIdMap`] content identities, never through list
//! positions; undeclared bytes stay unresolved rather than guessed; and only a
//! measured layout is admitted by default. It is a pure function over borrowed
//! bytes with no write path, so a hostile or oversized profile cannot touch the
//! source or a new save. Nothing here is original-verified: every legacy layout
//! is still `Unknown` and F64-B supplies the measured one.
//!
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats
//! [`cs_assets`]: cs_assets

pub mod ai;
pub mod airframe_roles;
pub mod animation;
pub mod audio;
pub mod cameras;
pub mod campaign;
pub mod campaign_bindings;
pub mod capital;
pub mod catalog;
pub mod cinematics;
pub mod config;
pub mod construction;
pub mod coordinates;
pub mod damage;
pub mod detail;
pub mod environment;
pub mod flight_tuning;
pub mod hud;
pub mod instant_action;
pub mod interaction;
pub mod legacy_import;
pub mod livery;
pub mod loading;
pub mod localization;
pub mod mesh;
pub mod mission_control;
pub mod mods;
pub mod multiplayer;
pub mod objectives;
pub mod ordnance;
pub mod original_airframe;
pub mod pilots;
pub mod replay;
pub mod routes;
pub mod save;
pub mod scene;
pub mod scrapbook;
pub mod settings;
pub mod stunts;
pub mod target_rules;
pub mod textures;
pub mod ui_layout;
pub mod weapons;
pub mod weather;
pub mod world;
pub mod world_actors;
