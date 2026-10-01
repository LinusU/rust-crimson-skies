//! Explicit support/cargo dependency graph.

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::ActorId;

/// Geometry and collision are one state: they change together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Intact geometry with its collider.
    Intact,
    /// Destroyed geometry and no collider.
    Destroyed,
}

/// A refused edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphError {
    UnknownActor(ActorId),
    DuplicateActor(ActorId),
    /// The edge would make an actor transitively support itself.
    Cycle {
        supporter: ActorId,
        dependent: ActorId,
    },
}

/// Who depends on whom: `dependent` is lost when `supporter` is destroyed.
/// Edges are declared by actor id; no name is ever inspected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SupportGraph {
    presence: BTreeMap<ActorId, Presence>,
    dependents: BTreeMap<ActorId, BTreeSet<ActorId>>,
}

impl SupportGraph {
    /// # Errors
    ///
    /// [`GraphError::DuplicateActor`].
    pub fn add_actor(&mut self, actor: ActorId) -> Result<(), GraphError> {
        if self.presence.insert(actor, Presence::Intact).is_some() {
            return Err(GraphError::DuplicateActor(actor));
        }
        Ok(())
    }

    /// Declares that `dependent` rests on / is carried by `supporter`.
    ///
    /// # Errors
    ///
    /// [`GraphError::UnknownActor`] or [`GraphError::Cycle`].
    pub fn add_support(
        &mut self,
        supporter: ActorId,
        dependent: ActorId,
    ) -> Result<(), GraphError> {
        for a in [supporter, dependent] {
            if !self.presence.contains_key(&a) {
                return Err(GraphError::UnknownActor(a));
            }
        }
        if supporter == dependent || self.reaches(dependent, supporter) {
            return Err(GraphError::Cycle {
                supporter,
                dependent,
            });
        }
        self.dependents
            .entry(supporter)
            .or_default()
            .insert(dependent);
        Ok(())
    }

    fn reaches(&self, from: ActorId, to: ActorId) -> bool {
        let mut stack = vec![from];
        let mut seen = BTreeSet::new();
        while let Some(a) = stack.pop() {
            if a == to {
                return true;
            }
            if seen.insert(a) {
                stack.extend(self.dependents.get(&a).into_iter().flatten().copied());
            }
        }
        false
    }

    #[must_use]
    pub fn presence(&self, actor: ActorId) -> Option<Presence> {
        self.presence.get(&actor).copied()
    }

    /// Destroys `actor` and everything that transitively depends on it, in
    /// breadth order with ties broken by id. Returns the newly destroyed
    /// actors; each had its geometry and collider removed in this one call.
    /// Destroying an already destroyed actor returns an empty list.
    ///
    /// # Errors
    ///
    /// [`GraphError::UnknownActor`].
    pub fn destroy(&mut self, actor: ActorId) -> Result<Vec<ActorId>, GraphError> {
        match self.presence.get(&actor) {
            None => return Err(GraphError::UnknownActor(actor)),
            Some(Presence::Destroyed) => return Ok(Vec::new()),
            Some(Presence::Intact) => {}
        }
        let mut out = Vec::new();
        let mut queue = std::collections::VecDeque::from([actor]);
        while let Some(a) = queue.pop_front() {
            if self.presence.insert(a, Presence::Destroyed) == Some(Presence::Intact) {
                out.push(a);
                queue.extend(self.dependents.get(&a).into_iter().flatten().copied());
            }
        }
        Ok(out)
    }
}
