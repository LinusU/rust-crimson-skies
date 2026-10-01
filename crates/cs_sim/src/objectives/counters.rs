//! Per-category actor counters.

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::ActorId;

use crate::damage::LifecycleKind;

/// Why an actor stopped counting as present. Distinct on purpose: an actor
/// removed by a cinematic is not a kill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CountKind {
    Destroyed,
    Disabled,
    Captured,
    Escaped,
    Despawned,
}

impl CountKind {
    /// The category a damage lifecycle transition counts toward, or `None`
    /// when it counts toward none (a pilot bailing out is not a kill, and
    /// mission removal is not any of the five).
    #[must_use]
    pub const fn from_lifecycle(kind: LifecycleKind) -> Option<Self> {
        match kind {
            LifecycleKind::Destroyed => Some(Self::Destroyed),
            LifecycleKind::OwnershipCaptured => Some(Self::Captured),
            LifecycleKind::Despawned => Some(Self::Despawned),
            LifecycleKind::PilotBailout | LifecycleKind::MissionRemoved => None,
        }
    }
}

/// Which actors are in which category. An actor is counted once per
/// category; recording it twice is a no-op.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorCounters {
    sets: BTreeMap<CountKind, BTreeSet<ActorId>>,
}

impl ActorCounters {
    /// Records `actor` in `kind`; returns whether it was newly counted.
    pub fn record(&mut self, kind: CountKind, actor: ActorId) -> bool {
        self.sets.entry(kind).or_default().insert(actor)
    }

    /// The count of one category only.
    #[must_use]
    pub fn count(&self, kind: CountKind) -> usize {
        self.sets.get(&kind).map_or(0, BTreeSet::len)
    }

    /// Whether `actor` is in `kind`.
    #[must_use]
    pub fn contains(&self, kind: CountKind, actor: ActorId) -> bool {
        self.sets.get(&kind).is_some_and(|s| s.contains(&actor))
    }
}
