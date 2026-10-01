//! Normalization, catalog, blueprints, localization and saves.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependencies: [`cs_types`],
//! [`cs_formats`] and, where a normalization needs the filesystem,
//! [`cs_assets`]. It must never depend on Bevy or Avian.
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
//! image catalog they populate, and the handoff to the GPU upload boundary.
//!
//! [`livery`] is the F09 paint composition
//! (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`): the three
//! mask colors of a BM livery, the deterministic cache key of a composed
//! variant and the composed RGB8 image (stage F09-B), plus the
//! session-scoped [`livery::LiveryVariantStore`] that caches those variants
//! keyed by every input that distinguishes them (stage F09-C).
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
//! [`coordinates`] holds source coordinate conventions and their adapters
//! into canonical space (`specs/F16-coordinates-units-origin-management-and-
//! clocks.md`, stage F16-A): one validated declaration per source, and every
//! position, direction, normal, rotation, winding, distance and angle
//! conversion derived from it, so a format maps into the canonical
//! convention exactly once. The declared sources are designed declarations
//! with `Origin`/`Provenance`; which convention an original file uses is
//! unmeasured (F16-D) and is never asserted here.
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
//! [`scene::BindingMap`] semantic-binding records.
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
//! [`flight_tuning`] is the F24-A provenance-carrying tuning schema
//! (`specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`, stage
//! `### F24-A`): every numeric field the `cs_sim::flight` equations consume,
//! with its unit and approved range, and a declared synthetic airframe whose
//! values are each known with provenance or an explicit unknown — never a
//! silent zero. It carries no original coefficient; F24-C maps the record
//! into the model and F24-D calibrates it against reference traces.
//!
//! [`cameras`] is the F21-A declared camera contract
//! (`specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-A`):
//! the provenance-carrying [`cameras::DeclaredCameraModes`] set of
//! [`cameras::DeclaredCameraMode`] records (a designed
//! [`cameras::CameraModeKind`] vocabulary, each mode's
//! [`cameras::ProjectionPolicy`], its [`cameras::Magnification`] and its
//! target-tracking flag) and the [`cameras::AspectFraming`] rule. Every
//! load-bearing value is a `Resolved` and the original PC view list, field
//! of view, axis, clipping planes and magnification are unmeasured, so the
//! synthetic fixture is designed content and never an original measurement.
//! Its runtime counterpart (the lowered projection and the framing math) is
//! `cs_app::camera`.
//!
//! [`target_rules`] is the F30-A declared targeting schema
//! (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`): the provenance-carrying
//! [`target_rules::DeclaredTargetRules`] of a subject's faction set,
//! directed [`target_rules::DeclaredRelation`]s and the
//! [`target_rules::TargetRuleSet`] policy knobs — threat window, crosshair
//! cone and the separate lead-indicator/aim-assistance options — each
//! [`Resolved`] known with provenance or an explicit unknown, never a
//! silent default. Its runtime counterpart is `cs_sim::targeting`; the
//! conversion boundary is `cs_app::targeting`.
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
//! [`animation`] is the F20-A declared animation IR
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stage
//! `### F20-A`): the provenance-carrying [`animation::AnimationClip`] record
//! with its transform, visibility, material and attachment channels and the
//! [`animation::EventMarker`]s whose unknown effects block rather than skip
//! gameplay transitions. Its runtime counterpart is
//! `cs_sim::animated_object`; the conversion boundary is
//! `cs_app::animation`.
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
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats
//! [`cs_assets`]: cs_assets

pub mod airframe_roles;
pub mod animation;
pub mod audio;
pub mod cameras;
pub mod campaign_bindings;
pub mod catalog;
pub mod config;
pub mod coordinates;
pub mod damage;
pub mod environment;
pub mod flight_tuning;
pub mod livery;
pub mod loading;
pub mod mesh;
pub mod routes;
pub mod save;
pub mod scene;
pub mod target_rules;
pub mod textures;
pub mod world;
