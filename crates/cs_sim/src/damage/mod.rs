//! Damage graphs, deterministic hit resolution and lifecycle events (F29).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Stage **F29-A** defines the typed contract, the resolver and a minimal
//! synthetic fixture — not the armor/disablement implementation (F29-B) or
//! the visual, debris, scoring and bailout wiring (F29-C). The module is
//! split so each part is one owner:
//!
//! * [`graph`] is the per-actor damage model: [`DamageNode`]s for armor
//!   zones, internal structure, engines and weapon mounts keyed by stable
//!   [`DamageNodeKey`]s, the [`DamageChannel`] a hit routes on and the
//!   `guarded_by`/`overflow` edges — armor and internal pools are distinct
//!   and no multiplier is ever invented.
//! * [`events`] is the immutable I/O: the [`HitEvent`] input, the ordered
//!   [`DamageEvent`] output vocabulary (applied hops, part and system
//!   transitions, lifecycle, the single kill award, refusals and blocks),
//!   the five distinct [`LifecycleKind`]s and the declared
//!   [`AttributionRule`].
//! * [`resolver`] is the per-session [`DamageResolver`]: session-qualified
//!   actor registration, the deterministic same-tick ordering, the
//!   simultaneous-lethal policy that awards one kill (AC01), and the
//!   once-per-kind lifecycle ledger that keeps death, bailout, capture,
//!   despawn and mission removal separate.
//! * [`synthetic`] is the declared synthetic airframe graph the acceptance
//!   tests drive.
//!
//! The declared, provenance-carrying schema that records where each graph
//! and rule came from lives in `cs_content::damage`; the lowering boundary
//! and the ECS binding records are `cs_app::damage`. What the original
//! 2000 PC game's damage model, armor behavior and kill attribution were
//! is unrecovered (`F29` "Research boundary"); nothing here claims to
//! reproduce it — see
//! `docs/findings/2026-09-30-f29-a-damage-graphs-hit-ordering-lifecycle.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`), so every record here is built from those
//! types: no Bevy, no renderer, no file access.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod events;
pub mod graph;
pub mod resolver;
pub mod synthetic;

pub use events::{
    ActorId, AttributionRule, DamageEvent, DamageEventId, DamageEventKind, HitEvent, HitEventError,
    HitEventId, LifecycleKind, RefusalReason,
};
pub use graph::{
    DamageChannel, DamageGraph, DamageGraphError, DamageNode, DamageNodeKey, DamageNodeKind,
    MAX_NODE_KEY_LEN, NodeKeyError, PartState, SystemKind,
};
pub use resolver::{DamageError, DamagePolicy, DamageResolver, TickResolution};
pub use synthetic::{
    SYNTHETIC_AIRFRAME_KEY, SYNTHETIC_ARMOR_INTEGRITY, SYNTHETIC_ARMOR_NODE,
    SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE, SYNTHETIC_HULL_INTEGRITY,
    SYNTHETIC_HULL_NODE, SYNTHETIC_MOUNT_INTEGRITY, SYNTHETIC_MOUNT_NODE, synthetic_airframe_graph,
};
