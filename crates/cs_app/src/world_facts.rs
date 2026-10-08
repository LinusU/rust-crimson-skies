//! The world-side half of [`cs_script::runtime::MissionFacts`]: the writer for
//! `members`, `groups`, `generators` and `animations`.
//!
//! The simulation owns the other two maps — [`cs_sim::mission::ActorFactTable`]
//! writes `actors` and [`cs_sim::mission::BlockLifecycleTable`] writes
//! `objectives` — and both document that the world-side maps are somebody
//! else's to fold in through [`MissionFacts::absorb`]. This module is that
//! somebody: the host that observes the world.
//!
//! # What the record spells, and what has to answer
//!
//! The lowering (`M01-LC-DIRECTIVE-LOWERING`) emits four conditions that read
//! world state rather than program state:
//!
//! | Condition | Map it reads | Operand the record spells |
//! | --- | --- | --- |
//! | [`cs_script::ir::Condition::InactiveMembers`] | `members` | a node/part/part-state **name chain** per listed member |
//! | [`cs_script::ir::Condition::Travelers`] | `members` | a subject chain and, for an object anchor, an anchor chain |
//! | [`cs_script::ir::Condition::EnemyGroupDepletion`] | `groups`, `generators` | a **group id** and an optional generator name |
//! | [`cs_script::ir::Condition::AnimationStates`] | `animations` | an **animation name** per listed pair |
//!
//! None of those maps had a writer when the lowering landed, so
//! [`cs_script::runtime::MissionState::holds`] answered `false` for every key they read and the 24
//! of M01's 58 blocks carrying one of these conditions could evaluate but never
//! complete (finding `2026-10-07-m01-lc-directive-lowering-integration.md`).
//! This module builds the writers.
//!
//! # Resolution first, observation second
//!
//! Two different questions have to be answered before a row is written, and
//! they are deliberately kept apart:
//!
//! 1. **Does the operand name anything in the observed world?** That is
//!    resolution, and it is [`MemberResolver`]'s job for the member chains — a
//!    chain that names nothing is recorded as [`MemberPresence::Missing`] and
//!    never given a presence, a position or a count. The contract's own rule
//!    applies — "Validate names/ids/ranges" and measure rather than assume
//!    (`docs/contracts/SCRIPT-MISSION.md`, "Program security").
//! 2. **What does the world say about it this tick?** That is observation, and
//!    it is [`WorldObservation`]'s job: the host's per-tick report of the
//!    in-play state of the members it is holding, the living count of the
//!    groups it tracks, the pending spawns a generator owes and the state byte
//!    of an animation it is running.
//!
//! A resolved name nobody observed this tick gets **no row at all**, which is
//! the same fail-closed read every other unpopulated map gives
//! ([`cs_script::runtime::MissionState::holds`] answers `false` for a key the
//! map does not hold). Nothing here defaults: no presence, no position, no
//! count.
//!
//! # The resolver is the admission ticket
//!
//! [`WorldFactTable::facts`] answers nothing at all when the table has no
//! resolver ([`WorldFactTable::drop_resolver`]): every spelled chain is
//! recorded [`MemberPresence::Missing`] and no count is reported, whatever the
//! observation holds. The resolver is what ties an operand to the observed
//! world; without it there is no way to tell a chain the record spelled from a
//! chain nothing could look up, so the writer refuses to name anything.
//!
//! # What is measured and what is not
//!
//! The chain *shape* is measured: an `INACTIVE<n>` site resolves its first
//! string through the original's name resolver and each later string through a
//! chained member lookup on the previous result, so the arguments are one
//! hierarchy (finding
//! `2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`).
//! [`MemberResolver`] implements exactly that over the installation's own node
//! hierarchy: element 0 must name exactly one node, each later element must
//! name exactly one **descendant** of the node the previous element matched,
//! and anything else — no match, or more than one — is unresolved rather than a
//! guess.
//!
//! Two things stay unknown and are carried as such rather than filled in:
//!
//! * **The in-play bit's producers** (`+0x24` bit 4) are vehicle/zeppelin
//!   spawn and despawn code outside the measured bound
//!   ([`cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`]), so this module never
//!   derives presence — it only reports what the host observed.
//! * **A node's world position** is per-node-type data-section content this
//!   tree does not decode, so the resolver reports *existence* only. A position
//!   arrives with the observation, which is where a live world actually has
//!   one.
//!
//! See `docs/findings/2026-10-08-m01-lc-world-facts.md` for the measurements
//! behind the resolution rule and for which of M01's blocks can now complete.

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::{MemberName, MissionProgram, TravelersAnchor};
use cs_script::runtime::{MemberFact, MemberPresence, MissionFacts};
use cs_sim::mission::{BlockLifecycleTable, MissionSession};

use crate::world::retail::RetailWorldContainer;

/// One chain, as the record spelled it, made from string literals.
///
/// A small helper for callers building a [`MemberName`] out of the record's own
/// spellings; it exists so no caller has to decide between `Vec<String>` and
/// something borrowable.
#[must_use]
pub fn member_chain<const N: usize>(parts: [&str; N]) -> MemberName {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// Resolves an original member chain onto the installation's node hierarchy.
///
/// A [`MemberName`] is `[base]` or `[base, part, part-state]` — the
/// node/part/part-state triple F39-E4 measured — and the original resolves it
/// as **one hierarchy**: the first string through its name resolver, each later
/// string through a chained lookup on the previous result (finding B). This
/// resolver is that rule over data this tree actually reads.
///
/// # The rule, and why it is fail-closed
///
/// * the first element must match **exactly one** node in the hierarchy;
/// * every later element must match exactly one node **inside the subtree** of
///   the node the previous element matched;
/// * zero matches or more than one match is *unresolved*.
///
/// The last point is the important one. M01's container spells `ctur1` under
/// five different zeppelins, so a flat name lookup would answer for the wrong
/// ship; the hierarchy is what makes the chain unambiguous, and a chain the
/// hierarchy cannot pin to a single node is answered `None` rather than with an
/// arbitrary candidate.
///
/// The resolver reports **existence only**. It does not carry a world position:
/// a node's transform lives in per-node-type data-section content this tree
/// does not decode, so producing one here would be an invented number.
#[derive(Clone, Debug)]
pub struct MemberResolver {
    /// Every node's authored name, by hierarchy slot.
    names: Vec<String>,
    /// `ancestors[slot]` holds that node's ancestors, root first — the path a
    /// chain element has to be inside to be the next element's match.
    ancestors: Vec<Vec<u32>>,
}

impl MemberResolver {
    /// Reads a resolver out of one world container's own node array.
    #[must_use]
    pub fn from_container(container: &RetailWorldContainer) -> Self {
        Self::from_hierarchy(
            container
                .nodes()
                .nodes
                .iter()
                .map(|node| (node.name.clone(), node.parent)),
        )
    }

    /// Reads a resolver out of a `(name, parent-slot)` hierarchy.
    ///
    /// The parent slot of a root is `None`; a parent slot out of range is
    /// treated as a root rather than as a chain to walk, so a malformed
    /// hierarchy cannot make a lookup answer for a node it did not reach.
    #[must_use]
    pub fn from_hierarchy<I>(nodes: I) -> Self
    where
        I: IntoIterator<Item = (String, Option<u32>)>,
    {
        let stored: Vec<(String, Option<u32>)> = nodes.into_iter().collect();
        let count = stored.len() as u32;
        let names: Vec<String> = stored.iter().map(|(name, _)| name.clone()).collect();
        // A parent must be a real slot; a slot naming itself is dropped to a
        // root as well, so a malformed record cannot walk in a circle.
        let parents: Vec<Option<u32>> = stored
            .iter()
            .enumerate()
            .map(|(slot, (_, parent))| {
                parent.filter(|parent| *parent < count && *parent as usize != slot)
            })
            .collect();
        // Ancestors, root first: `path[slot]` is the chain a chain element has
        // to sit inside to be the next element's match. The parent slots are
        // not assumed to come before their children in stored order, so the
        // walk carries its own cycle guard instead.
        let mut ancestors: Vec<Vec<u32>> = Vec::with_capacity(stored.len());
        for slot in 0..stored.len() {
            let mut path: Vec<u32> = Vec::new();
            let mut cursor = parents[slot];
            while let Some(parent) = cursor {
                if path.contains(&parent) {
                    break;
                }
                path.push(parent);
                cursor = parents[parent as usize];
            }
            path.reverse();
            ancestors.push(path);
        }
        Self { names, ancestors }
    }

    /// How many nodes this resolver's hierarchy holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the hierarchy holds no node at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The slot the chain resolves to, or `None` when it does not resolve.
    ///
    /// `None` covers both "nothing is called that" and "more than one thing is
    /// called that": neither may be answered with a chosen candidate, because
    /// the caller would then be measuring the wrong object and reporting it as
    /// a world fact.
    #[must_use]
    pub fn resolve(&self, chain: &MemberName) -> Option<u32> {
        let first = chain.first()?;
        let mut current: Vec<u32> = self
            .names
            .iter()
            .enumerate()
            .filter(|(_, name)| *name == first)
            .map(|(slot, _)| slot as u32)
            .collect();
        current.sort_unstable();
        for step in chain.iter().skip(1) {
            let mut next: Vec<u32> = Vec::new();
            for slot in &current {
                for (candidate, name) in self.names.iter().enumerate() {
                    if name == step && self.ancestors[candidate].contains(slot) {
                        next.push(candidate as u32);
                    }
                }
            }
            next.sort_unstable();
            next.dedup();
            current = next;
            if current.is_empty() {
                return None;
            }
        }
        if current.len() != 1 {
            return None;
        }
        current.first().copied()
    }
}

/// What the live world reported about one member chain this tick.
///
/// The in-play bit's producers are outside the measured bound
/// ([`cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`]), so this struct is where
/// that answer enters: the writer reports it, never derives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemberObservation {
    /// Whether the live world still holds this member in play.
    pub in_play: bool,
    /// The member's world position as the live world reports it, for the
    /// radius comparisons [`cs_script::ir::Condition::Travelers`] runs.
    pub position: [f64; 3],
}

impl MemberObservation {
    /// The world still holds this member in play, at this position.
    #[must_use]
    pub const fn held_in_play(position: [f64; 3]) -> Self {
        Self {
            in_play: true,
            position,
        }
    }

    /// The world knows this member and no longer holds it in play, at this
    /// position.
    #[must_use]
    pub const fn no_longer_in_play(position: [f64; 3]) -> Self {
        Self {
            in_play: false,
            position,
        }
    }

    /// The presence this observation spells: `InPlay` or `OutOfPlay`.
    const fn presence(self) -> MemberPresence {
        if self.in_play {
            MemberPresence::InPlay
        } else {
            MemberPresence::OutOfPlay
        }
    }
}

/// One tick's report from the live world.
///
/// This is the whole input the world-side writer takes, and it holds exactly
/// what a host can actually observe: a member's in-play state and position, a
/// group's living count, a generator's pending-spawn count and an animation's
/// state byte. An entry the host did not report is an entry nobody observed —
/// the map then misses the key and the condition reading it answers `false`.
///
/// The default is the empty report: a world that said nothing at all. It is a
/// valid value and it is fail-closed, which is the point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldObservation {
    /// Chains the live world reported, keyed exactly as the record spells them.
    members: BTreeMap<MemberName, MemberObservation>,
    /// Group id → how many of its members are still in play.
    groups: BTreeMap<i32, u32>,
    /// Generator name → how many spawns it still owes.
    generators: BTreeMap<String, u32>,
    /// Animation name → its current state byte (`UNDEFINED` 0 …
    /// `INVALID_AND_RUNNING` 6).
    animations: BTreeMap<String, u32>,
}

impl WorldObservation {
    /// An empty report: the world said nothing, so nothing is observed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports what the world holds for one member chain.
    #[must_use]
    pub fn member(mut self, chain: MemberName, observation: MemberObservation) -> Self {
        self.members.insert(chain, observation);
        self
    }

    /// Reports how many members of one group are still in play.
    #[must_use]
    pub fn group(mut self, id: i32, living: u32) -> Self {
        self.groups.insert(id, living);
        self
    }

    /// Reports how many spawns one named generator still owes.
    #[must_use]
    pub fn generator(mut self, name: impl Into<String>, pending: u32) -> Self {
        self.generators.insert(name.into(), pending);
        self
    }

    /// Reports one animation's current state byte.
    #[must_use]
    pub fn animation(mut self, name: impl Into<String>, state: u32) -> Self {
        self.animations.insert(name.into(), state);
        self
    }

    /// The member chains this report carries.
    #[must_use]
    pub fn members(&self) -> &BTreeMap<MemberName, MemberObservation> {
        &self.members
    }

    /// The group ids this report carries, with their living counts.
    #[must_use]
    pub fn groups(&self) -> &BTreeMap<i32, u32> {
        &self.groups
    }

    /// The generator names this report carries, with their pending counts.
    #[must_use]
    pub fn generators(&self) -> &BTreeMap<String, u32> {
        &self.generators
    }

    /// The animation names this report carries, with their state bytes.
    #[must_use]
    pub fn animations(&self) -> &BTreeMap<String, u32> {
        &self.animations
    }
}

/// Every world-side operand one program's conditions spell, collected once so
/// the writer and the tests read the same set.
///
/// The set is keyed exactly as the record spelled the operand — the chain
/// itself, the group id as the site's own int, the generator and animation
/// names verbatim — because [`MissionFacts`]' maps are keyed that way and a
/// normalized key would be a second spelling nobody measured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldOperands {
    /// Every member chain any condition reads: `InactiveMembers`' listed
    /// chains, `Travelers`' subject and its object anchor.
    pub members: BTreeSet<MemberName>,
    /// Every group id any `EnemyGroupDepletion` names.
    pub groups: BTreeSet<i32>,
    /// Every generator name any `EnemyGroupDepletion` names — M01 spells none,
    /// so M01's set is empty and its `DEDG` blocks read no pending spawns.
    pub generators: BTreeSet<String>,
    /// Every animation name any `AnimationStates` names.
    pub animations: BTreeSet<String>,
}

impl WorldOperands {
    /// Collects the world-side operands every objective condition spells.
    ///
    /// This walks the same condition tree the evaluator walks, so the maps are
    /// populated for exactly the keys the tick will ask about and for nothing
    /// else.
    #[must_use]
    pub fn of(program: &MissionProgram) -> Self {
        let mut operands = Self::default();
        for objective in &program.objectives {
            Self::collect(&objective.condition, &mut operands);
        }
        operands
    }

    /// Walks one condition tree, collecting the operands of its world-shaped
    /// arms.
    fn collect(condition: &cs_script::ir::Condition, operands: &mut Self) {
        use cs_script::ir::Condition;
        match condition {
            Condition::InactiveMembers { members, .. } => {
                operands.members.extend(members.iter().cloned());
            }
            Condition::Travelers {
                subject, anchor, ..
            } => {
                operands.members.insert(subject.clone());
                if let TravelersAnchor::Object(chain) = anchor {
                    operands.members.insert(chain.clone());
                }
            }
            Condition::EnemyGroupDepletion {
                group, generator, ..
            } => {
                operands.groups.insert(*group);
                if let Some(name) = generator {
                    operands.generators.insert(name.clone());
                }
            }
            Condition::AnimationStates { animations, .. } => {
                operands
                    .animations
                    .extend(animations.iter().map(|(name, _)| name.clone()));
            }
            Condition::All(inner) | Condition::Any(inner) => {
                for child in inner {
                    Self::collect(child, operands);
                }
            }
            Condition::Not(inner) => Self::collect(inner, operands),
            // The variants that spell no world operand, listed one by one the
            // way [`cs_script::runtime::MissionState::holds`] lists them. No
            // catch-all on purpose: a later world-shaped variant must be a
            // compile error here rather than a map that silently stays empty
            // while the condition reading it answers `false` forever.
            Condition::Const(_)
            | Condition::Compare { .. }
            | Condition::ActorIs { .. }
            | Condition::ObjectiveAwake { .. }
            | Condition::Unknown { .. } => {}
        }
    }

    /// The operand count as one `(members, groups, generators, animations)`
    /// tuple, for pinning a mission's shape in a test.
    #[must_use]
    pub fn tuple(&self) -> (usize, usize, usize, usize) {
        (
            self.members.len(),
            self.groups.len(),
            self.generators.len(),
            self.animations.len(),
        )
    }
}

/// The production writer for the world-side half of [`MissionFacts`].
///
/// One table per mission tick: the host builds it with the mission's
/// [`MemberResolver`], hands it what the world observed
/// ([`WorldFactTable::observe`]), and asks for the facts
/// ([`WorldFactTable::facts`]) to fold in through
/// [`compose_mission_facts`] before the tick that evaluates them.
///
/// The table never holds a fact of its own. It holds a *resolver* (can these
/// operands be tied to the observed world at all?) and an *observation* (what
/// did the world say?), and [`Self::facts`] is where the two meet — which is
/// why [`Self::drop_resolver`] turns the whole table off instead of only the
/// member rows.
#[derive(Clone, Debug)]
pub struct WorldFactTable {
    /// The chain resolver, or `None` once it has been removed — see
    /// [`Self::drop_resolver`].
    resolver: Option<MemberResolver>,
    /// What the world reported this tick.
    observation: WorldObservation,
}

impl WorldFactTable {
    /// A table that resolves chains against `resolver` and answers from
    /// whatever is observed.
    #[must_use]
    pub fn new(resolver: MemberResolver) -> Self {
        Self {
            resolver: Some(resolver),
            observation: WorldObservation::new(),
        }
    }

    /// Replaces what the world reported this tick.
    pub fn observe(&mut self, observation: WorldObservation) {
        self.observation = observation;
    }

    /// The report the table currently holds.
    #[must_use]
    pub fn observation(&self) -> &WorldObservation {
        &self.observation
    }

    /// Removes the resolver, turning the table's answers fail-closed.
    ///
    /// With no resolver nothing can be tied to the observed world, so
    /// [`Self::facts`] records every spelled chain as
    /// [`MemberPresence::Missing`] and reports no group, generator or
    /// animation row at all — whatever the observation holds. It is the
    /// strongest form of the fail-closed read: an absent resolver defaults
    /// nothing.
    pub fn drop_resolver(&mut self) {
        self.resolver = None;
    }

    /// Whether this table still resolves chains.
    #[must_use]
    pub fn has_resolver(&self) -> bool {
        self.resolver.is_some()
    }

    /// The [`MissionFacts`] this table observed, carrying **only** the four
    /// world-side maps.
    ///
    /// Row rules, in order:
    ///
    /// * **No resolver** → every spelled chain becomes
    ///   [`MemberPresence::Missing`] with the zeroed position, and no count of
    ///   any kind is reported.
    /// * **Chain observed** → the observed presence and position, whether or
    ///   not the static hierarchy knows the name (a live object the world holds
    ///   but the authored hierarchy does not — the player, for instance — is
    ///   exactly this case, and the world is what says it is there).
    /// * **Chain the hierarchy resolves, unobserved** → *no row*: the name is
    ///   real but its state this tick is unknown, so the key is absent and
    ///   [`cs_script::runtime::MissionState::holds`] answers `false`.
    /// * **Chain nothing resolves** → [`MemberPresence::Missing`] with the
    ///   zeroed position: recorded absent, never invented.
    /// * **A group, generator or animation** → the reported count or state
    ///   byte, and *no row at all* when nothing reported one.
    ///
    /// The argument is [`WorldOperands`], the operands the program will
    /// actually ask about, so a fact nobody reads is not carried either.
    #[must_use]
    pub fn facts(&self, operands: &WorldOperands) -> MissionFacts {
        let mut facts = MissionFacts::default();
        let Some(resolver) = self.resolver.as_ref() else {
            for chain in &operands.members {
                facts.members.insert(chain.clone(), missing_member());
            }
            return facts;
        };
        for chain in &operands.members {
            if let Some(observed) = self.observation.members.get(chain) {
                facts.members.insert(
                    chain.clone(),
                    MemberFact {
                        presence: observed.presence(),
                        position: observed.position,
                    },
                );
            } else if resolver.resolve(chain).is_none() {
                facts.members.insert(chain.clone(), missing_member());
            }
        }
        for group in &operands.groups {
            if let Some(living) = self.observation.groups.get(group) {
                facts.groups.insert(*group, *living);
            }
        }
        for name in &operands.generators {
            if let Some(pending) = self.observation.generators.get(name) {
                facts.generators.insert(name.clone(), *pending);
            }
        }
        for name in &operands.animations {
            if let Some(state) = self.observation.animations.get(name) {
                facts.animations.insert(name.clone(), *state);
            }
        }
        facts
    }
}

/// The row an unresolved chain gets: recorded absent, position zeroed.
///
/// The zeroed position is the original's own record for an anchor that never
/// resolves (finding B), and it cannot be read by anything that would act on it
/// — [`cs_script::ir::Condition::InactiveMembers`] counts only [`MemberPresence::OutOfPlay`]
/// and [`cs_script::ir::Condition::Travelers`] refuses a subject that is not
/// [`MemberPresence::InPlay`] before it ever looks at a position.
const fn missing_member() -> MemberFact {
    MemberFact {
        presence: MemberPresence::Missing,
        position: [0.0; 3],
    }
}

/// Folds every fact writer's table into the one [`MissionFacts`] a tick reads.
///
/// This is the fold the two `cs_sim` tables document as the caller's job: the
/// actor-fact table owns `actors`, the block-lifecycle table owns `objectives`
/// and the world-side table owns the other four, and each builds its own
/// [`MissionFacts`] for [`MissionFacts::absorb`] to combine. Doing it here, in
/// one function, is what makes "the maps were folded before the tick that
/// evaluated them" a property a caller gets by construction rather than by
/// remembering.
///
/// `session` contributes its actor observations, `blocks` the numbered blocks'
/// lifecycle states and `world` the four world-side maps, keyed exactly as the
/// record spelled them.
#[must_use]
pub fn compose_mission_facts(
    session: &MissionSession,
    blocks: &BlockLifecycleTable,
    world: &WorldFactTable,
    operands: &WorldOperands,
) -> MissionFacts {
    let mut facts = session.actor_facts().facts();
    facts.absorb(blocks.facts());
    facts.absorb(world.facts(operands));
    facts
}
