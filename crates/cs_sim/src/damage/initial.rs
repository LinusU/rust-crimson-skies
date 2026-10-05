//! Authored initial damage (F29, task #515).
//!
//! A mission can start part of the world damaged
//! (`cs_content::world::WorldInstance::initially_damaged`). That record says
//! *which objects* start damaged and nothing else: no amount, and no map from
//! a world object to a damage node. This module is the resolver-side half of
//! applying such authoring: [`InitialDamage`] is one authored, provenance-
//! carrying statement "this node starts with `amount` of its pool gone", and
//! [`InitialDamageReport`] says what was applied and what was **not**, with
//! the reason, instead of guessing.
//!
//! Nothing here invents the amount or the object-to-node mapping; the caller
//! supplies both from authored data (see `cs_app::damage`), and a missing
//! piece is reported as unresolved. A pool is never started at or below zero:
//! whether the original game had a legal pre-destroyed state is unknown, so
//! such a statement is refused as [`InitialDamageRefusal::PreDestroyed`].

use cs_types::content::Provenance;

use super::graph::DamageNodeKey;

/// One authored statement that a node starts damaged.
#[derive(Clone, Debug, PartialEq)]
pub struct InitialDamage {
    /// The node the damage is authored against.
    pub node: DamageNodeKey,
    /// The integrity removed from the node's pool before the session starts.
    pub amount: f64,
    /// Where the statement came from.
    pub provenance: Provenance,
}

/// Why one authored statement was not applied.
#[derive(Clone, Debug, PartialEq)]
pub enum InitialDamageRefusal {
    /// The actor's graph has no such node.
    UnknownNode,
    /// The node's pool is an explicit unknown, so no remainder can be stated.
    UnresolvedPool,
    /// The amount is not finite or not positive.
    InvalidAmount,
    /// The amount equals or exceeds the pool; a pre-destroyed start is not a
    /// legal state this engine can claim.
    PreDestroyed {
        /// The pool the amount was authored against.
        pool: f64,
    },
    /// An earlier statement in the same batch already named this node.
    DuplicateNode,
}

/// One authored statement that was applied.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedInitialDamage {
    /// The statement, as authored.
    pub damage: InitialDamage,
    /// The node's remaining integrity after it.
    pub remaining: f64,
}

/// One authored statement that was refused, with the reason.
#[derive(Clone, Debug, PartialEq)]
pub struct UnresolvedInitialDamage {
    /// The statement, as authored.
    pub damage: InitialDamage,
    /// Why it was not applied.
    pub refusal: InitialDamageRefusal,
}

/// What [`super::DamageResolver::apply_initial_damage`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InitialDamageReport {
    /// The statements applied, in input order.
    pub applied: Vec<AppliedInitialDamage>,
    /// The statements left unapplied, in input order.
    pub unresolved: Vec<UnresolvedInitialDamage>,
}
