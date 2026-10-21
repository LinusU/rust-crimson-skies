//! The objective-target flag consumer (M01-EXAMINE-OBJECTIVE, #1754).
//!
//! `ADD_OBJECTIVE_TARGET` / `REMOVE_OBJECTIVE_TARGET` set and clear the
//! objective-target flag (`+0x4d`) of the object each spelled name resolves
//! to, at the completion of the objective that spells them
//! ([`DirectiveOperation::SetTargetFlag`]). M01's examine objectives
//! (`OBJECTIVE2`, `9` and `12`) are `TRAVELERS player APPROACHING <site> 200`
//! with a `REMOVE_OBJECTIVE_TARGET <site>`: the site stops being an objective
//! target the moment the player comes within the radius.
//!
//! This ledger applies each such emission exactly once, deduplicated by
//! execution key like [`crate::mission_ai::WakeEnemiesLedger`], to the
//! mission's declared ground targets: the [`MissionGroundTarget`] flag and the
//! targeting store's record flag change together.
//!
//! **The hand-off.** When the retired site is the selected target the owner
//! saw the selection move to the next objective site by itself (Site #1 to
//! Site #2, no key pressed; `docs/findings/2026-10-21-m01-examine-
//! objective.md`). Which site comes next is **designed**: the next declared
//! objective row still flagged, in `targets.zrd` order, which is the order the
//! owner's `E` cycle measured. The original's selection rule is not recovered.
//! The same designed rule opens the mission: with nothing selected the first
//! declared objective site is, as the owner's first capture shows.

use std::collections::BTreeSet;

use bevy::prelude::{Entity, World};
use cs_script::ir::{DirectiveOperation, Value};
use cs_script::runtime::{ExecutionKey, MissionState};
use cs_sim::damage::ActorId;
use cs_types::Tick;

use crate::mission_actors::MissionGroundTarget;
use crate::targeting::TargetingSession;

/// One objective-target flag change this ledger applied to a ground target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetFlagChange {
    /// The container node whose flag changed.
    pub node: String,
    /// The new flag value.
    pub objective: bool,
    /// The tick of the host step that applied it.
    pub at: Tick,
    /// The node the selection was handed to, when this change took the flag
    /// off the selected target and another objective site was still flagged.
    pub handed_to: Option<String>,
}

/// The mission host's `ADD/REMOVE_OBJECTIVE_TARGET` consumer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetFlagLedger {
    applied: BTreeSet<ExecutionKey>,
    changes: Vec<TargetFlagChange>,
    opened: bool,
}

impl TargetFlagLedger {
    /// Applies every objective-target flag emission of `state` this ledger has
    /// not applied yet, and opens the mission on the first objective site.
    pub fn consume(&mut self, world: &mut World, state: &MissionState, tick: Tick) {
        self.open_on_first_site(world);
        for emission in state.directives() {
            let DirectiveOperation::SetTargetFlag {
                objective: true,
                set,
            } = emission.operation
            else {
                continue;
            };
            if !self.applied.insert(emission.key.execution_key()) {
                continue;
            }
            let mut names = Vec::new();
            collect_names(&emission.args, &mut names);
            for name in names {
                if let Some(change) = set_site_flag(world, &name, set, tick) {
                    self.changes.push(change);
                }
            }
        }
    }

    /// Every flag change applied to a declared ground target, in order.
    #[must_use]
    pub fn changes(&self) -> &[TargetFlagChange] {
        &self.changes
    }

    /// With nothing selected, selects the first declared objective site, once.
    fn open_on_first_site(&mut self, world: &mut World) {
        if self.opened {
            return;
        }
        let first = flagged_sites(world).into_iter().next();
        let Some(mut targeting) = world.get_resource_mut::<TargetingSession>() else {
            return;
        };
        if targeting.selection().current().is_some() {
            self.opened = true;
        } else if let Some((_, actor, _)) = first
            && targeting.select_scripted(actor)
        {
            self.opened = true;
        }
    }
}

/// Every name an emission spells, whether bare or inside a one-name chain.
fn collect_names(args: &[Value], names: &mut Vec<String>) {
    for value in args {
        match value {
            Value::Str(name) if !name.is_empty() => names.push(name.clone()),
            Value::List(inner) => collect_names(inner, names),
            _ => {}
        }
    }
}

/// The ground targets that are flagged as objective sites, in declaration
/// order: `(declared index, damage actor, node)`.
fn flagged_sites(world: &mut World) -> Vec<(usize, ActorId, String)> {
    let mut query = world.query::<&MissionGroundTarget>();
    let mut sites: Vec<(usize, ActorId, String)> = query
        .iter(world)
        .filter(|target| target.objective)
        .filter_map(|target| Some((target.declared?, target.damage_actor, target.node.clone())))
        .collect();
    sites.sort();
    sites
}

/// Sets or clears the flag of the declared ground target named `node`, and
/// hands the selection on when the cleared target was the selected one.
fn set_site_flag(
    world: &mut World,
    node: &str,
    objective: bool,
    at: Tick,
) -> Option<TargetFlagChange> {
    let (entity, actor, declared) = {
        let mut query = world.query::<(Entity, &MissionGroundTarget)>();
        let (entity, target) = query.iter(world).find(|(_, target)| target.node == node)?;
        (entity, target.damage_actor, target.declared)
    };
    if let Some(mut target) = world.get_mut::<MissionGroundTarget>(entity) {
        if target.objective == objective {
            return None;
        }
        target.objective = objective;
    }
    let successor = flagged_sites(world)
        .into_iter()
        .find(|(index, _, _)| Some(*index) > declared)
        .map(|(_, actor, node)| (actor, node));
    let mut handed_to = None;
    if let Some(mut targeting) = world.get_resource_mut::<TargetingSession>() {
        // A target the store does not hold (not rostered yet) keeps the flag
        // on the component only; the roster pass stamps it from there.
        let _ = targeting.store_mut().set_objective(actor, objective);
        if !objective
            && targeting.selection().current() == Some(actor)
            && let Some((next, next_node)) = successor
            && targeting.select_scripted(next)
        {
            handed_to = Some(next_node);
        }
    }
    Some(TargetFlagChange {
        node: node.to_owned(),
        objective,
        at,
        handed_to,
    })
}
