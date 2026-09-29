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
//! [`livery`] is the F09-B paint composition
//! (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`,
//! `### F09-B`): the three mask colors of a BM livery, the deterministic
//! cache key of a composed variant and the composed RGB8 image.
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
//! hash and JSON report).
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
//! [`mesh`] is the F10-C.01 render mesh
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`,
//! `### F10-C`): the Bevy-free canonical render mesh built from F10-B's raw
//! GameZ mesh IR and its topology. One render vertex per distinct
//! `(position index, normal index, uv, color, material)` tuple, compared
//! bit-exactly, so a shared position with different per-corner UVs keeps the
//! authored seam; every render vertex and triangle keeps its source corner
//! or step; an incomplete topology is refused naming each rejected face's
//! `FaceIssue` code; and triangles are grouped by their raw material index,
//! never interpreting polygon flags.
//!
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats
//! [`cs_assets`]: cs_assets

pub mod campaign_bindings;
pub mod catalog;
pub mod config;
pub mod coordinates;
pub mod livery;
pub mod loading;
pub mod mesh;
pub mod textures;
