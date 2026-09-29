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
//! closure.md`, stage F14-A): stable-id elements in canonical order,
//! duplicate identities refused, and the unsupported-mission count that
//! keeps an unavailable mission in the denominator.
//!
//! [`campaign_bindings`] holds the engine-independent mission binding and
//! campaign coverage records (`specs/F50-per-mission-compatibility-and-
//! full-campaign-closure.md`, stage F50-A): the seven required content
//! categories the F50 owner ruling preserves, one explicit unresolved
//! dependency row per required subsystem, the frozen campaign denominator
//! read from `missions/bindings/campaign-inventory.tsv`, and the coverage
//! totals plus closure reports that keep a missing, unknown or unsupported
//! child counted instead of ready. It reads no original data and claims no
//! gameplay success; binding real missions is F50-B and the per-mission
//! tasks.
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
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats
//! [`cs_assets`]: cs_assets

pub mod campaign_bindings;
pub mod catalog;
pub mod config;
pub mod coordinates;
pub mod livery;
pub mod loading;
pub mod textures;
