//! Read-only installation mounts, discovery, IO and derived caches.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. [`install`] defines the typed
//! inventory path (discovered host files to validated manifest) for F02,
//! including the F02-B path that walks a real installation and hashes bytes
//! (`discover`, `discover_with_cache`, streaming `Sha256`, `fingerprint`,
//! `content_fingerprint`, `AnalysisCache`), and [`vfs`] is the
//! context-aware virtual filesystem for F04: mount namespaces, precedence
//! and `resolve(context, key)`. [`zbd`] is the F06-C wiring: a ZBD container
//! resolved through a content session is dispatched, its own trailer member
//! index is read, and its sound members become sound assets with the samples
//! their own WAVE headers declare; its F06-D corpus audit gives every
//! container and member a row. [`rof`] is the F05-C bridge: a ROF container
//! walked by the `cs_formats` reader becomes one mount of file members,
//! whose bytes are read and explicitly exported through the bounded
//! decoder. [`cache`] is the F15-A contract layer of the private
//! derived-asset cache: the [`cache::CacheKey`] identity (installation,
//! source spans, decoder/IR version, conversion options), the stored-entry
//! integrity gate and the private-location/budget bounds. [`mods`] is the
//! F53-B root join: one mod's host directory walked, indexed and hashed as
//! a mod-precedence, mod-scoped mount ([`mods::ModRoot`]), a declared source
//! resolved against that root with its spelling re-validated at the join and
//! a symbolic link never followed, and the member read back against the
//! digest the walk recorded. Allowed
//! dependencies:
//! [`cs_types`] and [`cs_formats`]. The original installation at
//! `$CS_GAME_DIR` is read-only and nothing derived from it is committed to
//! Git.
//!
//! [`install`]: install
//! [`mods`]: mods
//! [`vfs`]: vfs
//! [`zbd`]: zbd
//! [`rof`]: rof
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats

pub mod cache;
pub mod install;
pub mod mods;
pub mod rof;
pub mod vfs;
pub mod zbd;
