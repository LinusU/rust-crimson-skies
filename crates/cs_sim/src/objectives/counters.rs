//! Per-category actor counters.
//!
//! # Which of the five have a producer (F39-E4)
//!
//! The five categories are a **designed vocabulary** (F39 non-negotiable
//! behavior 2 asks counters to distinguish destroyed, disabled, captured,
//! escaped and despawned actors), and only three of them have an engine
//! producer today: [`CountKind::from_lifecycle`] maps the damage lifecycle
//! transitions, so `Destroyed`, `Captured` and `Despawned` are reported by
//! [`crate::damage::DamageResolver`].
//!
//! `Disabled` and `Escaped` are **not**, and F39-E4 measured why
//! (`docs/findings/2026-10-04-f39-e4-count-category-producers.md`): over the
//! owner's installation the original's own objective records declare no
//! actor-count category for either, and the compiled mission program that would
//! carry a counter is still undecoded (F13-B/C, F38), so nothing measurable
//! produces them. They are therefore **reachable only from a caller that
//! reports them**, which is what [`Self::needs_declared_reporter`] says in
//! queryable form. A caller that invents a producer to make the count reachable
//! would be reporting a category the original never declares, so the declared
//! schema refuses an original record that counts one
//! (`cs_content::objectives::ObjectivesSchemaError::UnmeasuredCountCategory`).
//!
//! What the original *does* name is a label, not a category: 13 measured
//! `targets.zrd` records carry the objective kinds `MSG_OBJ_DISABLE` (5) and
//! `MSG_OBJ_DISABLEENG` (8), beside 107 `MSG_OBJ_DESTROY` records. A localized
//! label and a counted lifecycle transition are different things, so the
//! measurement does not become a producer here.

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::ActorId;

use crate::damage::LifecycleKind;

/// Why an actor stopped counting as present. Distinct on purpose: an actor
/// removed by a cinematic is not a kill.
///
/// `Disabled` and `Escaped` are designed categories with no measured producer
/// (F39-E4): see the module docs and [`Self::needs_declared_reporter`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CountKind {
    Destroyed,
    Disabled,
    Captured,
    Escaped,
    Despawned,
}

impl CountKind {
    /// Every category, in declaration order.
    ///
    /// The enumeration a consumer walks instead of matching on the type, so a
    /// new variant cannot be added without every consumer seeing it.
    pub const ALL: &'static [CountKind] = &[
        Self::Destroyed,
        Self::Disabled,
        Self::Captured,
        Self::Escaped,
        Self::Despawned,
    ];

    /// The stable label used in reports and evidence (`"destroyed"`, …).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Destroyed => "destroyed",
            Self::Disabled => "disabled",
            Self::Captured => "captured",
            Self::Escaped => "escaped",
            Self::Despawned => "despawned",
        }
    }

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

    /// Whether this category has **no** producer in the engine and can only be
    /// counted by a caller that reports it.
    ///
    /// The inverse of [`Self::from_lifecycle`] being `Some(_)` for the other
    /// three, and the one place F39-E4's measured answer is given in queryable
    /// form: `Disabled` and `Escaped` are the two categories the original's own
    /// objective records never declare and whose transitions no subsystem
    /// reports. `Disabled` is *not* gone — it still exists in the world, which
    /// is why a session's teardown keeps it — but nothing produces the report.
    #[must_use]
    pub const fn needs_declared_reporter(self) -> bool {
        matches!(self, Self::Disabled | Self::Escaped)
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
