//! The damage resolver: deterministic hit ordering, part/system
//! transitions, destruction and scoring (F29-A).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`DamageResolver`] is one session generation's authority over damage
//! state. It owns a [`DamageGraph`] per registered actor and resolves one
//! tick's [`HitEvent`] batch at a time (the schedule's resolution phase,
//! `docs/01-ARCHITECTURE.md`):
//!
//! * **Deterministic ordering.** The batch is resolved in ascending
//!   [`HitEventId`] order — session, tick, producer, sequence — so the same
//!   hits produce the same events no matter which order the producers
//!   appended them (F29 non-negotiable behavior 2).
//! * **Session confinement.** Every hit must carry the resolver's session
//!   generation and the resolved tick; a hit stamped for another session or
//!   tick is refused by name (`STATE-TRANSACTIONS`: "Results, previous
//!   targets and delayed callbacks are always generation-qualified"). A
//!   restarted session is a *new* resolver — per-part damage, lifecycle
//!   records and deferred events never carry over (non-negotiable 5).
//! * **Simultaneous lethals.** Each hit applies in order; when an actor's
//!   first lethal node depletes, the resolution remembers the blow and
//!   every attacker's contribution. After the batch, each newly destroyed
//!   actor emits exactly one [`DamageEventKind::Lifecycle`] destruction and
//!   exactly one [`DamageEventKind::KillAwarded`] computed under that
//!   actor's own declared [`AttributionRule`] — two same-tick lethal hits
//!   award a single kill (AC01), and actors of different subject kinds
//!   resolve side by side under their own declared rules.
//! * **Lifecycle separation.** Death is one [`LifecycleKind`] of five; the
//!   others (bailout, capture, despawn, mission removal) are recorded
//!   through [`DamageResolver::record_lifecycle`] by the systems that own
//!   them. Each kind fires at most once per actor, and a terminal
//!   transition closes the record: nothing can be recorded for a despawned
//!   or mission-removed actor again.
//!
//! The resolver never infers damage from a particle or material
//! (non-negotiable 1): it consumes typed hits only, and its events are what
//! damaged visuals consume.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::content::Resolved;

use super::events::{
    ActorId, AttributionRule, DamageEvent, DamageEventId, DamageEventKind, HitEvent, HitEventId,
    LifecycleKind, RefusalReason,
};
use super::graph::{DamageChannel, DamageGraph, DamageNodeKey, PartState};

/// The declared rules one actor's resolution runs under.
///
/// `attribution` is the graph's declared simultaneous-lethal policy — the
/// "declared attribution rule" of AC01. It is data the schema carries per
/// graph, so it registers with the actor (`register_actor`): an aircraft, a
/// world object and a capital ship share the identity discipline but resolve
/// under their own declared rules, and no actor resolves kills under an
/// unstated convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamagePolicy {
    /// Which attacker a same-tick multiple-lethal resolution credits.
    pub attribution: AttributionRule,
}

/// Why a resolver operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageError {
    /// A hit or registration carried a session generation other than the
    /// resolver's. State never leaks across generations.
    ForeignSession {
        /// The resolver's session.
        expected: u64,
        /// The session the input carried.
        found: u64,
    },
    /// A hit carried a tick other than the tick being resolved; a batch
    /// resolves one tick only.
    ForeignTick {
        /// The tick being resolved.
        expected: Tick,
        /// The tick the hit carried.
        found: Tick,
    },
    /// Two hits in one batch share an identity; ordering would be
    /// ambiguous.
    DuplicateHit {
        /// The duplicated hit id.
        id: HitEventId,
    },
    /// The actor was already registered.
    DuplicateActor {
        /// The actor that exists already.
        actor: ActorId,
    },
    /// The actor is not registered with this resolver.
    UnknownActor {
        /// The actor that was named.
        actor: ActorId,
    },
    /// The actor already recorded this lifecycle transition; each kind
    /// fires once per session.
    DuplicateLifecycle {
        /// The actor.
        actor: ActorId,
        /// The repeated transition.
        kind: LifecycleKind,
    },
    /// The actor's record is closed by a terminal transition; nothing can
    /// be recorded for it again.
    ActorClosed {
        /// The closed actor.
        actor: ActorId,
        /// The terminal transition that closed it.
        terminal: LifecycleKind,
    },
}

impl fmt::Display for DamageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "input belongs to session {found}, but this resolver owns session {expected}"
            ),
            Self::ForeignTick { expected, found } => write!(
                f,
                "hit belongs to tick {}, but the resolved tick is {}",
                found.0, expected.0
            ),
            Self::DuplicateHit { id } => write!(
                f,
                "hit {}:{}:{}:{} appears twice in one batch",
                id.session, id.tick.0, id.producer, id.sequence
            ),
            Self::DuplicateActor { actor } => write!(f, "{actor} is already registered"),
            Self::UnknownActor { actor } => write!(f, "{actor} is not registered"),
            Self::DuplicateLifecycle { actor, kind } => {
                write!(f, "{actor} already recorded lifecycle {kind}")
            }
            Self::ActorClosed { actor, terminal } => {
                write!(f, "{actor} is closed by terminal lifecycle {terminal}")
            }
        }
    }
}

impl std::error::Error for DamageError {}

/// One actor's mutable damage state inside a resolution.
#[derive(Clone, Debug)]
struct ActorDamage {
    graph: DamageGraph,
    /// The declared rules this actor resolves under — its own graph's
    /// rules, never an ambient session convention.
    policy: DamagePolicy,
    /// Remaining integrity of every *known* pool. Unresolved pools are
    /// absent — their state is `PartState::Unknown`, asserted nowhere else.
    remaining: BTreeMap<DamageNodeKey, f64>,
    /// The lifecycle transitions recorded for this actor, in record order
    /// of first occurrence. Each kind appears at most once.
    lifecycle: BTreeSet<LifecycleKind>,
    /// The terminal transition that closed the record, when one fired.
    terminal: Option<LifecycleKind>,
}

impl ActorDamage {
    fn new(graph: &DamageGraph, policy: DamagePolicy) -> Self {
        let remaining = graph
            .nodes()
            .filter_map(|node| match node.integrity() {
                Resolved::Known(known) => Some((node.key().clone(), known.value)),
                Resolved::Unknown { .. } => None,
            })
            .collect();
        Self {
            graph: graph.clone(),
            policy,
            remaining,
            lifecycle: BTreeSet::new(),
            terminal: None,
        }
    }

    /// The observable state of one node: `Unknown` for an unresolved pool,
    /// `Destroyed` at zero, `Intact` before any damage, `Damaged` between.
    fn part_state(&self, key: &DamageNodeKey) -> PartState {
        let Some(node) = self.graph.node(key) else {
            return PartState::Unknown;
        };
        match node.integrity() {
            Resolved::Unknown { .. } => PartState::Unknown,
            Resolved::Known(known) => {
                let remaining = self.remaining.get(key).copied().unwrap_or(0.0);
                if remaining <= 0.0 {
                    PartState::Destroyed
                } else if remaining < known.value {
                    PartState::Damaged
                } else {
                    PartState::Intact
                }
            }
        }
    }
}

/// What one [`DamageResolver::resolve`] produced: the tick's ordered
/// events.
#[derive(Clone, Debug, PartialEq)]
pub struct TickResolution {
    /// The tick these events were resolved at.
    pub tick: Tick,
    /// The events, in emission order — hit applications, part and system
    /// transitions in resolved-hit order, then the tick's destruction and
    /// scoring events.
    pub events: Vec<DamageEvent>,
}

/// Per-victim resolution scratch for one batch.
#[derive(Default)]
struct VictimScratch {
    /// The first hit, in resolved order, that depleted a lethal node —
    /// the causal blow.
    blow: Option<HitEventId>,
    /// Total damage each attacker applied to the victim this batch, with
    /// the index of the attacker's first contributing hit for tie-breaks.
    contributions: BTreeMap<ActorId, (f64, u32)>,
    /// The victim's own lifecycle was already `Destroyed` before this
    /// batch, so nothing new is owed.
    already_destroyed: bool,
}

/// The per-session damage authority. See the module docs.
#[derive(Clone, Debug)]
pub struct DamageResolver {
    session: u64,
    producer: u32,
    actors: BTreeMap<ActorId, ActorDamage>,
    sequence: u32,
}

impl DamageResolver {
    /// A resolver for session `session`, stamping events with producer
    /// serial `producer`.
    ///
    /// A restart or aircraft swap is a *new* resolver: prior part damage,
    /// lifecycle records and deferred events never carry into the next
    /// generation (F29 non-negotiable behavior 5;
    /// `STATE-TRANSACTIONS` session generations).
    #[must_use]
    pub fn new(session: u64, producer: u32) -> Self {
        Self {
            session,
            producer,
            actors: BTreeMap::new(),
            sequence: 0,
        }
    }

    /// The session generation this resolver owns.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The producer serial stamped into event ids.
    #[must_use]
    pub const fn producer(&self) -> u32 {
        self.producer
    }

    /// Registers an actor, its damage graph and the declared rules that
    /// graph resolves under for this session.
    ///
    /// `policy` is the actor's own declared rules — what
    /// `cs_content::damage::GraphRules` declares and the boundary lowers —
    /// so actors of different subject kinds resolve side by side under
    /// *their own* rules (F29 deliverable: "the same identity discipline
    /// but their own rules"), never under a session-wide convention.
    ///
    /// # Errors
    ///
    /// [`DamageError::ForeignSession`] when `actor` belongs to another
    /// session generation, [`DamageError::DuplicateActor`] when it is
    /// already registered.
    pub fn register_actor(
        &mut self,
        actor: ActorId,
        graph: DamageGraph,
        policy: DamagePolicy,
    ) -> Result<(), DamageError> {
        if actor.session != self.session {
            return Err(DamageError::ForeignSession {
                expected: self.session,
                found: actor.session,
            });
        }
        if self.actors.contains_key(&actor) {
            return Err(DamageError::DuplicateActor { actor });
        }
        self.actors.insert(actor, ActorDamage::new(&graph, policy));
        Ok(())
    }

    /// The declared rules a registered actor resolves under; `None` when
    /// the actor is unknown.
    #[must_use]
    pub fn policy(&self, actor: &ActorId) -> Option<DamagePolicy> {
        self.actors.get(actor).map(|state| state.policy)
    }

    /// The observable state of one of an actor's nodes; `None` when the
    /// actor or node is unknown.
    #[must_use]
    pub fn part_state(&self, actor: &ActorId, node: &DamageNodeKey) -> Option<PartState> {
        self.actors
            .get(actor)
            .filter(|state| state.graph.node(node).is_some())
            .map(|state| state.part_state(node))
    }

    /// A node's remaining integrity; `None` when the actor or node is
    /// unknown or its pool is unresolved.
    #[must_use]
    pub fn remaining_integrity(&self, actor: &ActorId, node: &DamageNodeKey) -> Option<f64> {
        self.actors
            .get(actor)
            .and_then(|state| state.remaining.get(node).copied())
    }

    /// The lifecycle transitions recorded for an actor, each at most
    /// once; `None` when the actor is unknown.
    #[must_use]
    pub fn lifecycle(&self, actor: &ActorId) -> Option<&BTreeSet<LifecycleKind>> {
        self.actors.get(actor).map(|state| &state.lifecycle)
    }

    /// Whether the actor has recorded [`LifecycleKind::Destroyed`].
    #[must_use]
    pub fn is_destroyed(&self, actor: &ActorId) -> bool {
        self.actors
            .get(actor)
            .is_some_and(|state| state.lifecycle.contains(&LifecycleKind::Destroyed))
    }

    /// Records a non-damage lifecycle transition — bailout, capture,
    /// despawn, mission removal, or a scripted destruction — through the
    /// same once-per-kind ledger damage uses.
    ///
    /// The returned event is stamped at tick `at` with the resolver's own
    /// identity, so session systems publish the same `EventId` shape
    /// damage events carry.
    ///
    /// # Errors
    ///
    /// [`DamageError::UnknownActor`], [`DamageError::ActorClosed`] when the
    /// record is already terminal, [`DamageError::DuplicateLifecycle`]
    /// when this kind was already recorded.
    pub fn record_lifecycle(
        &mut self,
        actor: ActorId,
        kind: LifecycleKind,
        at: Tick,
    ) -> Result<DamageEvent, DamageError> {
        let state = self
            .actors
            .get_mut(&actor)
            .ok_or(DamageError::UnknownActor { actor })?;
        if let Some(terminal) = state.terminal {
            return Err(DamageError::ActorClosed { actor, terminal });
        }
        if !state.lifecycle.insert(kind) {
            return Err(DamageError::DuplicateLifecycle { actor, kind });
        }
        if kind.is_terminal() {
            state.terminal = Some(kind);
        }
        let event = DamageEvent {
            id: self.next_event_id(at),
            kind: DamageEventKind::Lifecycle { actor, kind },
        };
        Ok(event)
    }

    /// Resolves one tick's hits deterministically.
    ///
    /// The batch is validated before anything is applied: every hit must
    /// carry this resolver's session and `tick`, and hit ids must be
    /// unique. The hits are then resolved in ascending [`HitEventId`]
    /// order — the declared ordering rule — so input order cannot change
    /// the outcome. Each hit routes on its [`DamageChannel`], applies to
    /// node pools along the `overflow` chain, emits a record per hop and
    /// contributes to its attacker's tally. Once the batch is applied,
    /// every actor whose first lethal node depleted emits one destruction
    /// and one kill award under *its own* declared [`AttributionRule`] — a
    /// single kill no matter how many hits were lethal (AC01).
    ///
    /// Hits that land afterward still apply part damage — a wreck can take
    /// further hits — but can never emit a second destruction or award.
    /// The same holds for a record closed by a terminal transition: the
    /// part damage lands, but a despawned or mission-removed actor records
    /// no lifecycle or scoring event again.
    ///
    /// # Errors
    ///
    /// [`DamageError::ForeignSession`], [`DamageError::ForeignTick`] or
    /// [`DamageError::DuplicateHit`]; nothing is applied on error.
    pub fn resolve(
        &mut self,
        tick: Tick,
        hits: &[HitEvent],
    ) -> Result<TickResolution, DamageError> {
        let mut ids = BTreeSet::new();
        for hit in hits {
            if hit.id.session != self.session {
                return Err(DamageError::ForeignSession {
                    expected: self.session,
                    found: hit.id.session,
                });
            }
            if hit.id.tick != tick {
                return Err(DamageError::ForeignTick {
                    expected: tick,
                    found: hit.id.tick,
                });
            }
            if !ids.insert(hit.id) {
                return Err(DamageError::DuplicateHit { id: hit.id });
            }
        }

        let mut ordered: Vec<&HitEvent> = hits.iter().collect();
        ordered.sort_by_key(|hit| hit.id);

        let mut kinds: Vec<DamageEventKind> = Vec::new();
        let mut victims: BTreeMap<ActorId, VictimScratch> = BTreeMap::new();
        for (index, hit) in ordered.iter().enumerate() {
            let index = index as u32;
            self.apply_hit(hit, index, &mut kinds, &mut victims);
        }

        // Terminal pass: each newly destroyed actor emits one destruction
        // and one award, in actor order — never interleaved with the hits
        // that caused them, and never twice.
        for (victim, scratch) in victims {
            if scratch.already_destroyed {
                continue;
            }
            let Some(blow) = scratch.blow else {
                continue;
            };
            let state = self
                .actors
                .get_mut(&victim)
                .expect("a resolved victim is registered");
            // A record closed by a terminal transition records nothing
            // again — no destruction and no award, though the hits above
            // still applied their part damage.
            if state.terminal.is_some() {
                continue;
            }
            if state.lifecycle.insert(LifecycleKind::Destroyed) {
                kinds.push(DamageEventKind::Lifecycle {
                    actor: victim,
                    kind: LifecycleKind::Destroyed,
                });
            }
            let credited = match state.policy.attribution {
                AttributionRule::FirstLethalHit => Self::blow_attacker(&blow, hits),
                AttributionRule::GreatestDamage => scratch
                    .contributions
                    .iter()
                    .max_by(|a, b| {
                        // More applied damage wins; the tie-break is the
                        // earliest contributing hit, then the lower actor
                        // id, so the rule is total and deterministic.
                        a.1.0
                            .partial_cmp(&b.1.0)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(b.1.1.cmp(&a.1.1))
                            .then(b.0.cmp(a.0))
                    })
                    .map(|(attacker, _)| *attacker),
            };
            kinds.push(DamageEventKind::KillAwarded {
                victim,
                credited,
                rule: state.policy.attribution,
                blow,
            });
        }

        let events = kinds
            .into_iter()
            .map(|kind| DamageEvent {
                id: self.next_event_id(tick),
                kind,
            })
            .collect();
        Ok(TickResolution { tick, events })
    }

    /// The attacker of the hit `blow` names, looked up in the batch.
    fn blow_attacker(blow: &HitEventId, hits: &[HitEvent]) -> Option<ActorId> {
        hits.iter()
            .find(|hit| &hit.id == blow)
            .and_then(|hit| hit.attacker)
    }

    fn next_event_id(&mut self, tick: Tick) -> DamageEventId {
        let id = DamageEventId {
            session: self.session,
            tick,
            producer: self.producer,
            sequence: self.sequence,
        };
        self.sequence += 1;
        id
    }

    /// Applies one hit to its target's graph, emitting the per-hop records.
    fn apply_hit(
        &mut self,
        hit: &HitEvent,
        order_index: u32,
        kinds: &mut Vec<DamageEventKind>,
        victims: &mut BTreeMap<ActorId, VictimScratch>,
    ) {
        let Some(state) = self.actors.get_mut(&hit.target) else {
            kinds.push(DamageEventKind::HitRefused {
                hit: hit.id,
                reason: RefusalReason::UnknownTargetActor,
            });
            return;
        };

        // Channel routing picks the entry node: an `Armor`-channel hit on a
        // guarded node enters through its armor guard; every other hit
        // enters at the named node.
        let Some(named) = state.graph.node(&hit.node) else {
            kinds.push(DamageEventKind::HitRefused {
                hit: hit.id,
                reason: RefusalReason::UnknownNode,
            });
            return;
        };
        let mut current = match hit.channel {
            DamageChannel::Armor => named.guarded_by().unwrap_or(&hit.node).clone(),
            DamageChannel::Internal => hit.node.clone(),
        };

        let mut remaining_damage = hit.damage;
        let mut total_applied = 0.0;
        loop {
            let node = state.graph.node(&current).expect("graph edges validate");
            let (overflow, disables, lethal) =
                (node.overflow().cloned(), node.disables(), node.is_lethal());
            match node.integrity() {
                Resolved::Unknown { claim_id, reason } => {
                    // An unresolved pool cannot absorb or be skipped: the
                    // hit blocks visibly with its claim and the chain ends.
                    kinds.push(DamageEventKind::HitBlocked {
                        hit: hit.id,
                        claim_id: claim_id.clone(),
                        reason: reason.clone(),
                    });
                    break;
                }
                Resolved::Known(_) => {}
            }

            let from = state.part_state(&current);
            let pool = state.remaining.entry(current.clone()).or_insert(0.0);
            let applied = remaining_damage.min(*pool);
            *pool -= applied;
            remaining_damage -= applied;
            total_applied += applied;
            let left = *pool;
            kinds.push(DamageEventKind::HitApplied {
                hit: hit.id,
                node: current.clone(),
                applied,
                remaining_integrity: left,
            });
            let to = state.part_state(&current);
            if to != from {
                kinds.push(DamageEventKind::PartTransition {
                    node: current.clone(),
                    from,
                    to,
                });
            }
            if to == PartState::Destroyed
                && from != PartState::Destroyed
                && let Some(system) = disables
            {
                kinds.push(DamageEventKind::SystemDisabled {
                    node: current.clone(),
                    system,
                });
            }
            if to == PartState::Destroyed && from != PartState::Destroyed && lethal {
                let scratch = victims.entry(hit.target).or_default();
                scratch.already_destroyed = state.lifecycle.contains(&LifecycleKind::Destroyed);
                if !scratch.already_destroyed && scratch.blow.is_none() {
                    scratch.blow = Some(hit.id);
                }
            }

            if remaining_damage <= 0.0 {
                break;
            }
            let Some(next) = overflow else {
                break;
            };
            current = next;
        }

        // The attacker's tally counts everything this hit applied anywhere
        // on the victim — the declared `GreatestDamage` input.
        if let (Some(attacker), true) = (hit.attacker, total_applied > 0.0) {
            let scratch = victims.entry(hit.target).or_default();
            let entry = scratch
                .contributions
                .entry(attacker)
                .or_insert((0.0, order_index));
            entry.0 += total_applied;
            entry.1 = entry.1.min(order_index);
        }
    }
}
