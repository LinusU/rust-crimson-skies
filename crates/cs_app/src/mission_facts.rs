//! The world-side [`MissionFacts`] writers: `members`, `groups`,
//! `generators` and `animations`, populated per tick from what the world
//! actually carries — the producer half `M01-LC-WORLD-FACTS` (#751) was
//! split off for.
//!
//! `MissionFacts` has six maps. `cs_sim` writes two of them —
//! [`crate::animation`]-side records aside, `ActorFactTable` writes `actors`
//! and `BlockLifecycleTable` writes `objectives` — and this module's
//! [`WorldFactTable`] writes the other four, the ones the lowered
//! world-reading conditions consume:
//!
//! * `members` — the exact [`MemberName`] chains `Condition::InactiveMembers`
//!   and `Condition::Travelers` spell, resolved against the mounted world;
//! * `groups` — the living member count `Condition::EnemyGroupDepletion`
//!   reads for its group id;
//! * `generators` — the pending-spawn count the same condition reads for its
//!   optional generator name;
//! * `animations` — the current state byte `Condition::AnimationStates`
//!   reads for an animation record's name.
//!
//! ## What the read set is
//!
//! [`MissionWorldReads::of`] walks one lowered [`MissionProgram`] and
//! collects every world-side operand — recursively, through `All`, `Any` and
//! `Not` — so the table reports exactly the keys the evaluator can read and
//! nothing else. A name the program never spells is never written: the map
//! stays the evaluator's fail-closed surface.
//!
//! ## What is measured and what is designed
//!
//! * Member resolution is the original's `member(name)` /
//!   `member::member(name)` two-step (finding B of the M01-LC findings): a
//!   chain's first element resolves an object in the world's flat name
//!   space, and each further element resolves one descendant of the node
//!   the previous element landed on — the member-of-member walk the record
//!   spells. Every step must land on **exactly one** node: a name nobody
//!   carries resolves `Missing`, and a name two nodes carry is ambiguous —
//!   which one the original picks is unmeasured, so the table reports no
//!   fact rather than choosing.
//! * Presence is the `+0x24` in-play bit (finding B): `InPlay` while the
//!   member and every ancestor on its mount are in play, `OutOfPlay` once
//!   it or an ancestor leaves play — a member cannot stay in play inside
//!   one that died, so the bit propagates down the mount.
//! * The group roster and the generator's pending count are **host
//!   declarations**: which members the original assigns to each AI group
//!   and how spawns enqueue is unmeasured (finding B's residual
//!   `DEDG_MEMBER_FIELD_REWRITES` names the neighbour gap). The table
//!   counts only what it resolved: a group whose roster resolves whole
//!   reports its living count, a group with an ambiguous member reports
//!   nothing — an unrecorded group is unknown, never empty — and a member
//!   that resolves `Missing` is not living.
//! * The animation name space is the carrier records' `anim_name`s — the
//!   same key space the `ANIM_STATE` operand spells (verified over M01's
//!   `mis_anim.zbd`: `hooked_to_klondike`, `wv_pickup_copilot`,
//!   `wv_drop_copilot` are all record names). The byte is the measured
//!   table the runtime documents (`DORMANT` 1, `RUNNING` 2, `EXECUTED` 3)
//!   and comes from [`MissionAnimationPlayer`]'s running and finished
//!   ledgers, keyed by `(carrier, record_index)`.
//!
//! ## How the fold composes
//!
//! [`WorldFactTable::facts`] returns a [`MissionFacts`] that carries only
//! these four maps; the caller folds it into the tick's fact map with
//! [`MissionFacts::absorb`] — after the actor and lifecycle tables' own
//! `facts()`, before `MissionSession::advance` — so the step reads one map
//! that every writer contributed to. Nothing is accumulated between calls:
//! what the evaluator reads on a tick is what the world held when `facts()`
//! was last taken.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cs_content::scene::{SceneGraph, SceneNodeId};
use cs_script::ir::{Condition, MemberName, MissionProgram, TravelersAnchor};
use cs_script::runtime::{MemberFact, MemberPresence, MissionFacts};

use crate::animation::survey::CarrierKind;
use crate::mission_animations::MissionAnimationPlayer;

/// The measured animation state byte for a record that exists but is not
/// running (`DORMANT`, finding C's `0x523820` state table).
const ANIMATION_DORMANT: u32 = 1;
/// The measured state byte for a record in the player's running ledger
/// (`RUNNING`).
const ANIMATION_RUNNING: u32 = 2;
/// The measured state byte for a record in the player's finished ledger
/// (`EXECUTED`).
const ANIMATION_EXECUTED: u32 = 3;

/// The world-side operands one lowered program's conditions read.
///
/// Built once per program by [`Self::of`] and handed to
/// [`WorldFactTable::facts`] every tick: the set is a property of the
/// record, not of the tick, so it is collected once and never re-walked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MissionWorldReads {
    /// Every `MemberName` chain spelled — `INACTIVE<n>` members, the
    /// `TRAVELERS` subject and its `Object` anchor, in the order the
    /// program first spells them (the set dedupes).
    members: BTreeSet<MemberName>,
    /// Every group id a `DEDG` site spells.
    groups: BTreeSet<i32>,
    /// Every generator name a `DEDG` site's optional child2 spells.
    generators: BTreeSet<String>,
    /// Every animation name an `ANIM_STATE` pair spells.
    animations: BTreeSet<String>,
}

impl MissionWorldReads {
    /// Collects the world-side operand set of `program` — every member
    /// chain, group id, generator name and animation name any lowered
    /// condition can read, through every `All`/`Any`/`Not` nesting.
    #[must_use]
    pub fn of(program: &MissionProgram) -> Self {
        let mut reads = Self::default();
        for objective in &program.objectives {
            reads.walk(&objective.condition);
        }
        reads
    }

    fn walk(&mut self, condition: &Condition) {
        match condition {
            Condition::InactiveMembers { members, .. } => {
                self.members.extend(members.iter().cloned());
            }
            Condition::EnemyGroupDepletion {
                group, generator, ..
            } => {
                self.groups.insert(*group);
                if let Some(generator) = generator {
                    self.generators.insert(generator.clone());
                }
            }
            Condition::Travelers {
                subject, anchor, ..
            } => {
                self.members.insert(subject.clone());
                if let TravelersAnchor::Object(anchor) = anchor {
                    self.members.insert(anchor.clone());
                }
            }
            Condition::AnimationStates { animations, .. } => {
                self.animations
                    .extend(animations.iter().map(|(name, _)| name.clone()));
            }
            Condition::All(inner) | Condition::Any(inner) => {
                for child in inner {
                    self.walk(child);
                }
            }
            Condition::Not(inner) => self.walk(inner),
            Condition::Const(_)
            | Condition::Compare { .. }
            | Condition::ActorIs { .. }
            | Condition::ObjectiveAwake { .. }
            | Condition::Unknown { .. } => {}
        }
    }

    /// The member chains the program spells.
    pub fn members(&self) -> impl Iterator<Item = &MemberName> {
        self.members.iter()
    }

    /// The group ids the program spells.
    pub fn groups(&self) -> impl Iterator<Item = i32> + '_ {
        self.groups.iter().copied()
    }

    /// The generator names the program spells.
    pub fn generators(&self) -> impl Iterator<Item = &String> {
        self.generators.iter()
    }

    /// The animation names the program spells.
    pub fn animations(&self) -> impl Iterator<Item = &String> {
        self.animations.iter()
    }
}

/// How one member chain resolved against the mounted world.
///
/// The three answers the resolver can honestly give: it landed on one node,
/// it landed on none, or it landed on several — the original's pick between
/// duplicates is unmeasured, so `Ambiguous` is reported rather than a choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberResolution {
    /// The chain landed on exactly one node.
    Resolved,
    /// Some chain element named nothing — the member is absent from the
    /// world.
    Missing,
    /// Some chain element named two or more nodes — which member the record
    /// meant is not knowable from the mount.
    Ambiguous,
}

/// The resolved fact for a chain — the [`MemberFact`] plus the resolution
/// verdict that produced it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MemberObservation {
    /// One node carries the chain; the fact is its presence and position.
    Resolved(MemberFact),
    /// No node carries the chain; the fact the writer records is
    /// [`MemberPresence::Missing`].
    Missing,
    /// The chain lands on several nodes; no fact is recorded.
    Ambiguous,
}

/// The resolution verdict, internal: the one case that differs from
/// [`MemberResolution`] is `Resolved` carrying the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedIndex {
    /// Exactly one node, at this mount slot.
    One(usize),
    /// No candidate.
    Missing,
    /// Two or more candidates.
    Ambiguous,
}

/// One mounted member: the world node or declared object a chain element
/// can land on.
#[derive(Clone, Debug, PartialEq)]
struct MemberNode {
    /// The authored name, exactly as the store or the host spelled it.
    name: String,
    /// The mount slot of the parent member, when this node has one.
    parent: Option<usize>,
    /// The mount slots of this node's children.
    children: Vec<usize>,
    /// The member's world position — the node's composed transform
    /// translation for a mounted node, the host's declaration for a member
    /// the static world does not carry.
    position: [f64; 3],
    /// The in-play bit this node carries: `+0x24` bit 4 while set. Cleared
    /// by the host's observation (`set_member_in_play`), never by the
    /// table.
    in_play: bool,
    /// Whether the member has left the world entirely — not the in-play
    /// bit: a removed member does not resolve at all, the measured
    /// distinction between an object that exists with its bit clear and no
    /// object.
    removed: bool,
}

/// The world-side fact writer: one mission's members, groups, generators
/// and animation states, observed and recorded under their exact spellings.
///
/// The world half is the mount: [`Self::mount`] lifts a converted
/// [`SceneGraph`] into member nodes — names, hierarchy and composed world
/// positions — and the host then moves, declares and retires members as the
/// simulation observes them. Nothing is populated without an observation:
/// a chain that resolves to nothing reads `Missing`, a name nobody declared
/// produces no `groups`/`generators`/`animations` key at all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldFactTable {
    /// The mounted member arena. Slot order is the graph's preorder for
    /// mounted nodes, then declared members in declaration order.
    members: Vec<MemberNode>,
    /// The declared AI-group rosters: group id → the member chains the host
    /// says belong to it. A group nobody declared is not here at all.
    groups: BTreeMap<i32, BTreeSet<MemberName>>,
    /// The declared generators' pending-spawn counts. A name nobody
    /// declared is not here, and the evaluator's measured read of a missing
    /// generator is a zero contribution — this map never writes the zero
    /// for a name it cannot see.
    generators: BTreeMap<String, u32>,
    /// The animation name space: every record name to the `(carrier,
    /// record_index)` keys that carry it, in mount order.
    animation_records: BTreeMap<String, Vec<(CarrierKind, usize)>>,
    /// The current state byte per animation name, recomputed by
    /// [`Self::observe_animations`] from the player's ledgers. A name whose
    /// records disagree about its state is not here — the byte is then
    /// unknown, not picked.
    animations: BTreeMap<String, u32>,
}

impl WorldFactTable {
    /// An empty table: no member resolves and no registry holds anything,
    /// so [`Self::facts`] reports every chain `Missing` and no group,
    /// generator or animation key — the same fail-closed surface an
    /// unpopulated `MissionFacts` gives, stated per key.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mounts every node of `graph` as a member: its authored name, its
    /// authored hierarchy and its composed world position, all in play —
    /// the state the world record itself declares. Positions and the
    /// in-play bits then move only through the host's observations.
    #[must_use]
    pub fn mount(graph: &SceneGraph) -> Self {
        let mut table = Self::new();
        let mut slots: HashMap<SceneNodeId, usize> = HashMap::with_capacity(graph.len());
        // `SceneGraph::nodes` is root-first preorder, so a node's parent is
        // always mounted before it.
        for node in graph.nodes() {
            let slot = table.members.len();
            let parent = node.parent().and_then(|id| slots.get(id).copied());
            if let Some(parent) = parent {
                table.members[parent].children.push(slot);
            }
            table.members.push(MemberNode {
                name: node.name().to_owned(),
                parent,
                children: Vec::new(),
                position: node.world_transform().translation(),
                in_play: true,
                removed: false,
            });
            slots.insert(node.id().clone(), slot);
        }
        table
    }

    /// Whether the node at `slot` has left the world — removed itself, or
    /// under a member that was.
    fn gone(&self, mut slot: usize) -> bool {
        loop {
            let node = &self.members[slot];
            if node.removed {
                return true;
            }
            match node.parent {
                Some(parent) => slot = parent,
                None => return false,
            }
        }
    }

    /// Whether the node at `slot` carries the in-play bit — itself and
    /// every ancestor still in the world and in play.
    fn in_play(&self, mut slot: usize) -> bool {
        loop {
            let node = &self.members[slot];
            if node.removed || !node.in_play {
                return false;
            }
            match node.parent {
                Some(parent) => slot = parent,
                None => return true,
            }
        }
    }

    /// The mount slots under `root` (the node itself excluded) whose name
    /// is `name`, skipping members that have left the world — a removed
    /// member's subtree leaves with it.
    fn descendants_named(&self, root: usize, name: &str) -> Vec<usize> {
        let mut hits = Vec::new();
        let mut pending: Vec<usize> = self.members[root].children.clone();
        while let Some(slot) = pending.pop() {
            if self.members[slot].removed {
                continue;
            }
            if self.members[slot].name == name {
                hits.push(slot);
            }
            pending.extend_from_slice(&self.members[slot].children);
        }
        hits
    }

    /// Resolves `chain` to its mount slot: the first element names one
    /// member in the world's flat name space — every mounted node and every
    /// declared member, not only roots — and each further element names one
    /// descendant of the node the element before it landed on. Every step
    /// must land on exactly one living candidate: none resolves `Missing`,
    /// several resolves `Ambiguous`.
    fn resolve_index(&self, chain: &[String]) -> ResolvedIndex {
        let Some((first, rest)) = chain.split_first() else {
            return ResolvedIndex::Missing;
        };
        let mut hits: Vec<usize> = self
            .members
            .iter()
            .enumerate()
            .filter(|(slot, node)| node.name == *first && !self.gone(*slot))
            .map(|(slot, _)| slot)
            .collect();
        for element in rest {
            if hits.len() != 1 {
                return match hits.len() {
                    0 => ResolvedIndex::Missing,
                    _ => ResolvedIndex::Ambiguous,
                };
            }
            hits = self.descendants_named(hits[0], element);
        }
        match hits.as_slice() {
            [slot] => ResolvedIndex::One(*slot),
            [] => ResolvedIndex::Missing,
            _ => ResolvedIndex::Ambiguous,
        }
    }

    /// How `chain` resolves against the mounted world right now.
    #[must_use]
    pub fn resolve(&self, chain: &[String]) -> MemberResolution {
        match self.resolve_index(chain) {
            ResolvedIndex::One(_) => MemberResolution::Resolved,
            ResolvedIndex::Missing => MemberResolution::Missing,
            ResolvedIndex::Ambiguous => MemberResolution::Ambiguous,
        }
    }

    /// The member `chain` names: its observed [`MemberFact`] when resolved,
    /// `Missing` when it is absent and `Ambiguous` when the mount cannot
    /// say which member was meant.
    #[must_use]
    pub fn member(&self, chain: &[String]) -> MemberObservation {
        match self.resolve_index(chain) {
            ResolvedIndex::One(slot) => MemberObservation::Resolved(MemberFact {
                presence: if self.in_play(slot) {
                    MemberPresence::InPlay
                } else {
                    MemberPresence::OutOfPlay
                },
                position: self.members[slot].position,
            }),
            ResolvedIndex::Missing => MemberObservation::Missing,
            ResolvedIndex::Ambiguous => MemberObservation::Ambiguous,
        }
    }

    /// Declares a member the mounted world does not carry — the player
    /// craft, a spawn — under its member-table name at `position`. The
    /// member mounts at top level, in play; `set_member_in_play` retires it
    /// and `move_member` tracks it. Declaring a name the mount already
    /// carries is not refused: the next resolution of it is honestly
    /// `Ambiguous`.
    pub fn declare_member(&mut self, name: impl Into<String>, position: [f64; 3]) {
        self.members.push(MemberNode {
            name: name.into(),
            parent: None,
            children: Vec::new(),
            position,
            in_play: true,
            removed: false,
        });
    }

    /// Declares a member under a mounted parent — a part of a spawned
    /// object — resolving `parent` the way the fact walk does. The child
    /// mounts at `position` in play when the parent resolved;
    /// [`MemberResolution`] reports why it did not.
    pub fn declare_child_member(
        &mut self,
        parent: &[String],
        name: impl Into<String>,
        position: [f64; 3],
    ) -> MemberResolution {
        let ResolvedIndex::One(slot) = self.resolve_index(parent) else {
            return self.resolve(parent);
        };
        let child = self.members.len();
        self.members.push(MemberNode {
            name: name.into(),
            parent: Some(slot),
            children: Vec::new(),
            position,
            in_play: true,
            removed: false,
        });
        self.members[slot].children.push(child);
        MemberResolution::Resolved
    }

    /// Records that `chain`'s member moved to `position` — the host's
    /// per-tick actor positions. Returns the resolution, so a move offered
    /// for a member that does not resolve records nothing and says so.
    pub fn move_member(&mut self, chain: &[String], position: [f64; 3]) -> MemberResolution {
        match self.resolve_index(chain) {
            ResolvedIndex::One(slot) => {
                self.members[slot].position = position;
                MemberResolution::Resolved
            }
            ResolvedIndex::Missing => MemberResolution::Missing,
            ResolvedIndex::Ambiguous => MemberResolution::Ambiguous,
        }
    }

    /// Writes the in-play bit the `INACTIVE`/`TRAVELERS` evaluators read:
    /// `false` when the member left play, `true` when it re-entered. The
    /// member stays resolvable either way — leaving play is not leaving the
    /// world (`remove_member` is that).
    pub fn set_member_in_play(&mut self, chain: &[String], in_play: bool) -> MemberResolution {
        match self.resolve_index(chain) {
            ResolvedIndex::One(slot) => {
                self.members[slot].in_play = in_play;
                MemberResolution::Resolved
            }
            ResolvedIndex::Missing => MemberResolution::Missing,
            ResolvedIndex::Ambiguous => MemberResolution::Ambiguous,
        }
    }

    /// Removes `chain`'s member from the world: it and its subtree no
    /// longer resolve at all — `Missing`, not `OutOfPlay`. This is the
    /// object-not-found half of the measured bit test, not a death mark.
    pub fn remove_member(&mut self, chain: &[String]) -> MemberResolution {
        match self.resolve_index(chain) {
            ResolvedIndex::One(slot) => {
                self.members[slot].removed = true;
                MemberResolution::Resolved
            }
            ResolvedIndex::Missing => MemberResolution::Missing,
            ResolvedIndex::Ambiguous => MemberResolution::Ambiguous,
        }
    }

    /// Declares the member roster of one AI group: the chains the host says
    /// the group consists of, each spelled as the world resolves it.
    /// Re-declaring replaces the roster — the host's latest observation of
    /// the membership is the one this table keeps.
    pub fn declare_group(
        &mut self,
        group: i32,
        members: impl IntoIterator<Item = MemberName>,
    ) {
        self.groups.insert(group, members.into_iter().collect());
    }

    /// Adds one member to a group's roster — a spawn the generator enrolled,
    /// for example. An undeclared group is created by the declaration: the
    /// enrollment is itself the observation that the roster exists.
    pub fn add_to_group(&mut self, group: i32, member: MemberName) {
        self.groups.entry(group).or_default().insert(member);
    }

    /// The roster a group was declared with, in chain order.
    pub fn group_roster(&self, group: i32) -> Option<&BTreeSet<MemberName>> {
        self.groups.get(&group)
    }

    /// Declares a generator's pending-spawn count — the number the `DEDG`
    /// site's optional child2 still owes the group. A name never declared
    /// writes no key: the evaluator's measured zero-on-miss supplies the
    /// contribution, so the map records only counts that were observed.
    pub fn set_generator_pending(&mut self, name: impl Into<String>, pending: u32) {
        self.generators.insert(name.into(), pending);
    }

    /// The pending count a declared generator carries, when it carries one.
    #[must_use]
    pub fn generator_pending(&self, name: &str) -> Option<u32> {
        self.generators.get(name).copied()
    }

    /// Mounts the animation name space of one carrier: every record's
    /// `anim_name`, keyed to the `(carrier, record_index)` identity the
    /// player's ledgers report. A name two records carry keeps both keys —
    /// and stays unwritten the moment they disagree about its state.
    ///
    /// Mounted names start at the measured `DORMANT` byte: the record
    /// exists and has not run, which is exactly what the original's state
    /// table encodes.
    pub fn mount_animation_records(&mut self, kind: CarrierKind, anim_names: &[Vec<u8>]) {
        for (index, name) in anim_names.iter().enumerate() {
            self.animation_records
                .entry(String::from_utf8_lossy(name).into_owned())
                .or_default()
                .push((kind, index));
        }
    }

    /// Recomputes every mounted animation name's state byte from the
    /// player's running and finished ledgers: `RUNNING` while a record
    /// runs, `EXECUTED` once it has, `DORMANT` before either. A name two
    /// records share is written only while they agree — which record the
    /// original's resolver would pick is unmeasured, so a disagreeing name
    /// reports no byte at all.
    pub fn observe_animations(&mut self, player: &MissionAnimationPlayer) {
        let running: BTreeSet<(CarrierKind, usize)> = player
            .running()
            .map(|record| (record.carrier(), record.record_index()))
            .collect();
        let finished: BTreeSet<(CarrierKind, usize)> = player
            .finished()
            .map(|record| (record.carrier(), record.record_index()))
            .collect();
        self.animations = self
            .animation_records
            .iter()
            .filter_map(|(name, keys)| {
                let states: BTreeSet<u32> = keys
                    .iter()
                    .map(|key| {
                        if running.contains(key) {
                            ANIMATION_RUNNING
                        } else if finished.contains(key) {
                            ANIMATION_EXECUTED
                        } else {
                            ANIMATION_DORMANT
                        }
                    })
                    .collect();
                match states.len() {
                    1 => Some((name.clone(), states.into_iter().next().expect("one state"))),
                    _ => None,
                }
            })
            .collect();
    }

    /// The state byte `name` currently reports, when its records agree on
    /// one.
    #[must_use]
    pub fn animation_state(&self, name: &str) -> Option<u32> {
        self.animations.get(name).copied()
    }

    /// The living count of `group`'s declared roster: resolved members that
    /// are still in play. `None` when the group was never declared or a
    /// declared member resolves ambiguously — in both cases the count is
    /// not known, and an unknown group is not an empty one.
    #[must_use]
    pub fn group_living(&self, group: i32) -> Option<u32> {
        let roster = self.groups.get(&group)?;
        let mut living = 0u32;
        for chain in roster {
            match self.resolve_index(chain) {
                ResolvedIndex::One(slot) => {
                    living += u32::from(self.in_play(slot));
                }
                // A member observed absent is not in play; the count is
                // still known.
                ResolvedIndex::Missing => {}
                // Which member the record meant is unknowable — the count
                // is then unknown, not short by one.
                ResolvedIndex::Ambiguous => return None,
            }
        }
        Some(living)
    }

    /// The [`MissionFacts`] for the next evaluation: this table's four maps,
    /// populated for exactly the keys `reads` collected — no key the
    /// program does not read, and no invented value for a key the world did
    /// not resolve.
    ///
    /// Fold it into the tick's map with [`MissionFacts::absorb`], after the
    /// actor- and lifecycle-fact tables' own `facts()` and before
    /// `MissionSession::advance`.
    #[must_use]
    pub fn facts(&self, reads: &MissionWorldReads) -> MissionFacts {
        let mut facts = MissionFacts::default();
        for chain in &reads.members {
            match self.member(chain) {
                MemberObservation::Resolved(fact) => {
                    facts.members.insert(chain.clone(), fact);
                }
                MemberObservation::Missing => {
                    facts.members.insert(
                        chain.clone(),
                        MemberFact {
                            presence: MemberPresence::Missing,
                            position: [0.0; 3],
                        },
                    );
                }
                MemberObservation::Ambiguous => {}
            }
        }
        for group in &reads.groups {
            if let Some(living) = self.group_living(*group) {
                facts.groups.insert(*group, living);
            }
        }
        for name in &reads.generators {
            if let Some(pending) = self.generators.get(name) {
                facts.generators.insert(name.clone(), *pending);
            }
        }
        for name in &reads.animations {
            if let Some(state) = self.animations.get(name) {
                facts.animations.insert(name.clone(), *state);
            }
        }
        facts
    }
}
