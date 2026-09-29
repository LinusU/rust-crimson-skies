//! Transitive dependency closure and graph validation (F14-B).
//!
//! Spec F14 non-negotiable behavior 2 ("Compute the transitive closure of
//! every launchable mission/scenario. Verify resources, parsers,
//! instructions, native handlers, sockets, strings, media and gameplay
//! consumers.") and the `IDENTITY-CONTENT` "Dependency closure algorithm":
//! start from the selected mission/scenario id, traverse every statically
//! referenced edge plus conservative dynamic candidate sets, keep visited
//! ids, predecessor chains and per-edge provenance, and mark readiness true
//! only when every critical node is supported.
//!
//! [`Closure::compute`] walks the catalog's reached subgraph from its
//! declared roots and reports, deterministically:
//!
//! * every reached node with its **closure readiness** — the node's own
//!   rules ([`super::CatalogElement::readiness`], parse state, normalize
//!   state, runtime consumer) *and* every reached dependency's readiness,
//!   computed to a fixpoint so a reference cycle that contains an unsupported
//!   node is unready while an all-ready reference cycle is ready;
//! * the **predecessor chain** from a root to any reached node, so deleting a
//!   resource several edges deep still yields the mission-to-resource path
//!   ([`Closure::chain_to`]);
//! * **orphaned references**: a dependency target with no catalog row is an
//!   explicit [`UnresolvedReference`], never treated as "no dependency";
//! * **ownership cycles**: a cycle among [`DependencyKind::Ownership`] edges
//!   is invalid (`ClosureError::OwnershipCycle`), while a cycle of plain
//!   reference edges is a legitimate graph and is allowed;
//! * a **closure hash** over the sorted ids, their content fingerprints, the
//!   closure state and the declared compatibility options, plus a canonical
//!   [`Closure::to_json`] report. Both are byte-stable under any input
//!   enumeration order (spec F14 AC02).
//!
//! Readiness is not stored back into the catalog: parsing, normalization,
//! dependency validation and runtime readiness are separate states
//! (non-negotiable behavior 1), and this value is the dependency-validation
//! state. An element keeps its own `readiness` and `unsupported_reasons`
//! unchanged.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use cs_assets::install::Sha256;
use cs_types::content::{
    ContentId, ContentKind, DependencyKind, NormalizeState, Provenance, UnsupportedReason,
};
use cs_types::evidence::ContentHash;
use cs_types::install::ParseState;

use super::Catalog;

/// The declared compatibility options a closure is computed under.
///
/// The contract's closure hash is "from sorted ids/hashes and compatibility
/// options": the same rows computed under different options are a different
/// closure, so the options are part of the hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatibilityOptions {
    /// Follow [`DependencyKind::DynamicCandidate`] edges. The conservative
    /// default is `true`: a bounded candidate set is still a dependency, and
    /// not following it would let an unbounded lookup look like "no
    /// dependencies".
    pub follow_dynamic_candidates: bool,
    /// Require every reached node to name at least one runtime consumer.
    pub require_runtime_consumer: bool,
    /// Tolerate references to ids with no catalog row. The default is
    /// `false`: an orphaned reference keeps its parent unavailable until it is
    /// resolved explicitly.
    pub tolerate_unresolved_references: bool,
}

impl Default for CompatibilityOptions {
    fn default() -> Self {
        Self::strict()
    }
}

impl CompatibilityOptions {
    /// The option set with nothing relaxed: follow dynamic candidates,
    /// require a consumer and refuse to tolerate orphaned references.
    pub const fn strict() -> Self {
        Self {
            follow_dynamic_candidates: true,
            require_runtime_consumer: true,
            tolerate_unresolved_references: false,
        }
    }

    /// The stable text encoding used in the closure hash.
    pub fn encode(&self) -> String {
        format!(
            "dynamic={};consumer={};tolerate_unresolved={}",
            self.follow_dynamic_candidates,
            self.require_runtime_consumer,
            self.tolerate_unresolved_references
        )
    }
}

/// Why a closure could not be computed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClosureError {
    /// A declared root id has no catalog row.
    UnknownRoot {
        /// The unknown root.
        id: ContentId,
    },
    /// A declared root is not a launchable mission or scenario.
    NotLaunchableRoot {
        /// The declared root.
        id: ContentId,
        /// Its kind.
        kind: ContentKind,
    },
    /// The ownership/parent subgraph contains a cycle, which is invalid.
    OwnershipCycle {
        /// The cycle, starting and ending at the same id.
        cycle: Vec<ContentId>,
    },
}

impl fmt::Display for ClosureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRoot { id } => write!(f, "no catalog element has root id {id}"),
            Self::NotLaunchableRoot { id, kind } => {
                write!(f, "root {id} is a {kind}, not a launchable scenario")
            }
            Self::OwnershipCycle { cycle } => {
                let path = cycle
                    .iter()
                    .map(ContentId::as_str)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "ownership cycle is invalid: {path}")
            }
        }
    }
}

impl std::error::Error for ClosureError {}

/// One reference edge of the reached subgraph.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edge {
    from: ContentId,
    target: ContentId,
    kind: DependencyKind,
}

/// A reference whose target has no catalog row.
///
/// The parent chain still reports it: deleting a texture several edges deep
/// leaves an explicit orphan, not a silent absence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedReference {
    /// The element that references the missing target.
    pub from: ContentId,
    /// The missing target.
    pub target: ContentId,
    /// Whether the reference was read directly or discovered dynamically.
    pub kind: DependencyKind,
    /// Where the reference was read from.
    pub provenance: Provenance,
}

/// The closure state of one reached node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeStatus {
    /// Whether the node's own rules (readiness, parse, normalize, consumer)
    /// pass, ignoring its dependencies.
    pub own_ready: bool,
    /// Whether the node is ready once its dependencies are folded in.
    pub ready: bool,
    /// Why the node is not ready, in canonical order. Empty iff `ready`.
    pub reasons: Vec<UnsupportedReason>,
}

/// The computed transitive dependency closure of a catalog's roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closure {
    roots: Vec<ContentId>,
    options: CompatibilityOptions,
    nodes: BTreeMap<ContentId, NodeStatus>,
    predecessors: BTreeMap<ContentId, ContentId>,
    unresolved: Vec<UnresolvedReference>,
    hash: ContentHash,
}

impl Closure {
    /// Computes the closure of every root over `catalog`.
    ///
    /// # Errors
    ///
    /// [`ClosureError::UnknownRoot`], [`ClosureError::NotLaunchableRoot`] and
    /// [`ClosureError::OwnershipCycle`].
    pub fn compute(
        catalog: &Catalog,
        roots: &[ContentId],
        options: CompatibilityOptions,
    ) -> Result<Self, ClosureError> {
        let mut root_set: BTreeSet<ContentId> = BTreeSet::new();
        for root in roots {
            let Some(element) = catalog.get(root) else {
                return Err(ClosureError::UnknownRoot { id: root.clone() });
            };
            if !element.kind.is_launchable() {
                return Err(ClosureError::NotLaunchableRoot {
                    id: root.clone(),
                    kind: element.kind,
                });
            }
            root_set.insert(root.clone());
        }

        let mut edges: Vec<Edge> = Vec::new();
        let mut predecessors: BTreeMap<ContentId, ContentId> = BTreeMap::new();
        let mut reached: BTreeSet<ContentId> = root_set.clone();
        let mut unresolved: Vec<UnresolvedReference> = Vec::new();
        let mut queue: VecDeque<ContentId> = root_set.iter().cloned().collect();

        while let Some(from) = queue.pop_front() {
            let Some(element) = catalog.get(&from) else {
                continue;
            };
            // Canonical dependency order makes traversal and predecessor
            // selection independent of the order rows were built in.
            let mut dependencies: Vec<&cs_types::content::Dependency> =
                element.dependencies.iter().collect();
            dependencies
                .sort_by(|a, b| (a.target.as_str(), a.kind).cmp(&(b.target.as_str(), b.kind)));
            for dependency in dependencies {
                let target = dependency.target.clone();
                if dependency.kind == DependencyKind::DynamicCandidate
                    && !options.follow_dynamic_candidates
                {
                    continue;
                }
                if catalog.get(&target).is_none() {
                    if !unresolved.iter().any(|held| {
                        held.from == from && held.target == target && held.kind == dependency.kind
                    }) {
                        unresolved.push(UnresolvedReference {
                            from: from.clone(),
                            target: target.clone(),
                            kind: dependency.kind,
                            provenance: dependency.provenance.clone(),
                        });
                    }
                    if !reached.contains(&target) && !predecessors.contains_key(&target) {
                        predecessors.insert(target.clone(), from.clone());
                    }
                    edges.push(Edge {
                        from: from.clone(),
                        target,
                        kind: dependency.kind,
                    });
                    continue;
                }
                edges.push(Edge {
                    from: from.clone(),
                    target: target.clone(),
                    kind: dependency.kind,
                });
                if reached.insert(target.clone()) {
                    predecessors.insert(target.clone(), from.clone());
                    queue.push_back(target);
                }
            }
        }
        edges.sort_by(|a, b| {
            (a.from.as_str(), a.target.as_str(), a.kind).cmp(&(
                b.from.as_str(),
                b.target.as_str(),
                b.kind,
            ))
        });
        unresolved.sort_by(|a, b| {
            (a.from.as_str(), a.target.as_str(), a.kind).cmp(&(
                b.from.as_str(),
                b.target.as_str(),
                b.kind,
            ))
        });

        if let Some(cycle) = ownership_cycle(&edges, &reached) {
            return Err(ClosureError::OwnershipCycle { cycle });
        }

        // Own rules first, ignoring dependencies.
        let mut nodes: BTreeMap<ContentId, NodeStatus> = BTreeMap::new();
        for id in &reached {
            let element = catalog.get(id).expect("a reached id has a catalog row");
            let (own_ready, reasons) = own_status(element, options);
            nodes.insert(
                id.clone(),
                NodeStatus {
                    own_ready,
                    ready: own_ready,
                    reasons,
                },
            );
        }

        // Fold dependencies in to a fixpoint. Reference cycles are allowed, so
        // this must not recurse or assume a topological order.
        let mut ready: BTreeMap<ContentId, bool> = nodes
            .iter()
            .map(|(id, status)| (id.clone(), status.own_ready))
            .collect();
        loop {
            let mut changed = false;
            for id in nodes.keys() {
                if !ready[id] {
                    continue;
                }
                if !dependencies_ready(id, &edges, &ready, catalog, options) {
                    ready.insert(id.clone(), false);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        for (id, status) in nodes.iter_mut() {
            status.ready = ready[id];
            if !status.ready && status.own_ready {
                for edge in edges.iter().filter(|edge| &edge.from == id) {
                    let bad = match catalog.get(&edge.target) {
                        Some(_) => !ready[&edge.target],
                        None => !options.tolerate_unresolved_references,
                    };
                    if bad {
                        status
                            .reasons
                            .push(UnsupportedReason::UnsupportedDependency {
                                target: edge.target.clone(),
                            });
                    }
                }
                dedup_reasons(&mut status.reasons);
            }
        }

        let hash = closure_hash(catalog, &root_set, options, &nodes, &unresolved);
        Ok(Self {
            roots: root_set.into_iter().collect(),
            options,
            nodes,
            predecessors,
            unresolved,
            hash,
        })
    }

    /// The declared roots, in canonical id order.
    pub fn roots(&self) -> &[ContentId] {
        &self.roots
    }

    /// The compatibility options this closure was computed under.
    pub fn options(&self) -> CompatibilityOptions {
        self.options
    }

    /// The closure hash over sorted ids, fingerprints, state and options.
    pub fn hash(&self) -> ContentHash {
        self.hash
    }

    /// Every reached node id, in canonical order.
    pub fn node_ids(&self) -> Vec<&ContentId> {
        self.nodes.keys().collect()
    }

    /// Whether `id` was reached from a root.
    pub fn is_reached(&self, id: &ContentId) -> bool {
        self.nodes.contains_key(id)
    }

    /// The closure state of a reached node.
    pub fn status(&self, id: &ContentId) -> Option<&NodeStatus> {
        self.nodes.get(id)
    }

    /// Whether a reached node is ready after its dependencies are folded in.
    /// An unreached id is not ready.
    pub fn is_ready(&self, id: &ContentId) -> bool {
        self.nodes.get(id).is_some_and(|status| status.ready)
    }

    /// Why a reached node is not ready; empty for a ready node or an
    /// unreached id.
    pub fn reasons(&self, id: &ContentId) -> &[UnsupportedReason] {
        self.nodes
            .get(id)
            .map(|status| status.reasons.as_slice())
            .unwrap_or(&[])
    }

    /// The reached nodes that are not ready, in canonical id order.
    pub fn unavailable(&self) -> Vec<&ContentId> {
        self.nodes
            .iter()
            .filter(|(_, status)| !status.ready)
            .map(|(id, _)| id)
            .collect()
    }

    /// The explicit orphaned references, in canonical order.
    pub fn unresolved(&self) -> &[UnresolvedReference] {
        &self.unresolved
    }

    /// Whether the closure covers every root and dependency: no orphaned
    /// reference and every root ready.
    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty() && self.roots.iter().all(|root| self.is_ready(root))
    }

    /// The reference chain from a root to `target`, inclusive, or `None` when
    /// `target` was not reached and is not an orphaned reference target.
    ///
    /// The chain is the deterministic first-discovery predecessor path, so a
    /// resource several edges deep is reported as its
    /// mission-to-resource path.
    pub fn chain_to(&self, target: &ContentId) -> Option<Vec<ContentId>> {
        if self.roots.contains(target) {
            return Some(vec![target.clone()]);
        }
        if !self.predecessors.contains_key(target) {
            return None;
        }
        let mut chain = vec![target.clone()];
        let mut current = target;
        loop {
            let predecessor = self.predecessors.get(current)?;
            chain.push(predecessor.clone());
            if self.roots.contains(predecessor) {
                chain.reverse();
                return Some(chain);
            }
            current = predecessor;
        }
    }

    /// The canonical deterministic JSON report of this closure.
    ///
    /// Every array is sorted and every string is escaped, so two closures
    /// computed from the same rows in any enumeration order serialize
    /// byte-for-byte identically (spec F14 AC02).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\"schema\":\"cs-content-closure-v1\",");
        out.push_str(&format!(
            "\"hash\":{},\"options\":{{",
            json_string(&self.hash.to_hex())
        ));
        out.push_str(&format!(
            "\"follow_dynamic_candidates\":{},\"require_runtime_consumer\":{},\"tolerate_unresolved_references\":{}",
            self.options.follow_dynamic_candidates,
            self.options.require_runtime_consumer,
            self.options.tolerate_unresolved_references
        ));
        out.push_str("},\"roots\":[");
        for (index, root) in self.roots.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&json_string(root.as_str()));
        }
        out.push_str("],\"nodes\":[");
        for (index, (id, status)) in self.nodes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            out.push_str(&json_string(id.as_str()));
            out.push_str(&format!(
                ",\"own_ready\":{},\"ready\":{}",
                status.own_ready, status.ready
            ));
            out.push_str(",\"chain\":[");
            if let Some(chain) = self.chain_to(id) {
                for (chain_index, member) in chain.iter().enumerate() {
                    if chain_index > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_string(member.as_str()));
                }
            }
            out.push_str("],\"reasons\":[");
            for (reason_index, reason) in status.reasons.iter().enumerate() {
                if reason_index > 0 {
                    out.push(',');
                }
                out.push_str("{\"code\":");
                out.push_str(&json_string(reason.code()));
                out.push_str(",\"detail\":");
                out.push_str(&match reason.detail() {
                    Some(detail) => json_string(detail),
                    None => "null".to_owned(),
                });
                out.push('}');
            }
            out.push_str("]}");
        }
        out.push_str("],\"unresolved\":[");
        for (index, reference) in self.unresolved.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"from\":");
            out.push_str(&json_string(reference.from.as_str()));
            out.push_str(",\"target\":");
            out.push_str(&json_string(reference.target.as_str()));
            out.push_str(",\"kind\":");
            out.push_str(&json_string(reference.kind.label()));
            out.push_str(",\"claim_id\":");
            out.push_str(&json_string(reference.provenance.claim_id.as_str()));
            out.push_str(",\"evidence_class\":");
            out.push_str(&json_string(reference.provenance.class.label()));
            out.push_str(",\"source\":");
            out.push_str(&match &reference.provenance.source {
                Some(source) => format!(
                    "{{\"container\":{},\"member\":{},\"offset\":{},\"length\":{}}}",
                    json_string(source.container_path()),
                    match source.member_key() {
                        Some(member) => json_string(member),
                        None => "null".to_owned(),
                    },
                    source.offset(),
                    source.length()
                ),
                None => "null".to_owned(),
            });
            out.push('}');
        }
        out.push_str("]}");
        out
    }
}

/// The node's own rules, ignoring dependencies.
fn own_status(
    element: &cs_types::content::CatalogElement,
    options: CompatibilityOptions,
) -> (bool, Vec<UnsupportedReason>) {
    let mut reasons = element.unsupported_reasons.clone();
    match &element.parse_state {
        ParseState::Unparsed => reasons.push(UnsupportedReason::NotParsed),
        ParseState::Failed { diagnostic } => reasons.push(UnsupportedReason::ParseFailed {
            diagnostic: diagnostic.clone(),
        }),
        ParseState::Parsed => {}
    }
    match &element.normalize_state {
        NormalizeState::NotNormalized => reasons.push(UnsupportedReason::NotNormalized),
        NormalizeState::Failed { diagnostic } => reasons.push(UnsupportedReason::NormalizeFailed {
            diagnostic: diagnostic.clone(),
        }),
        NormalizeState::Normalized => {}
    }
    if options.require_runtime_consumer && element.runtime_consumers.is_empty() {
        reasons.push(UnsupportedReason::MissingRuntimeConsumer);
    }
    dedup_reasons(&mut reasons);
    let own_ready = element.readiness.is_ready() && reasons.is_empty();
    (own_ready, reasons)
}

/// Whether every dependency that `id` owns or references is ready.
fn dependencies_ready(
    id: &ContentId,
    edges: &[Edge],
    ready: &BTreeMap<ContentId, bool>,
    catalog: &Catalog,
    options: CompatibilityOptions,
) -> bool {
    for edge in edges.iter().filter(|edge| &edge.from == id) {
        match catalog.get(&edge.target) {
            Some(_) => {
                if !ready.get(&edge.target).copied().unwrap_or(false) {
                    return false;
                }
            }
            None => {
                if !options.tolerate_unresolved_references {
                    return false;
                }
            }
        }
    }
    true
}

/// Deduplicates identical unsupported reasons while preserving their order.
///
/// The comparison is by full value, not by `(code, detail)`: an
/// [`UnsupportedReason::UnsupportedDependency`] names its target in the
/// variant payload and carries no `detail`, so a key that ignored the payload
/// would collapse two different unsupported dependencies of one node into a
/// single reason and hide one of them.
fn dedup_reasons(reasons: &mut Vec<UnsupportedReason>) {
    let mut unique: Vec<UnsupportedReason> = Vec::with_capacity(reasons.len());
    for reason in reasons.drain(..) {
        if !unique.contains(&reason) {
            unique.push(reason);
        }
    }
    *reasons = unique;
}

/// Detects a cycle among ownership edges, deterministically and iteratively.
fn ownership_cycle(edges: &[Edge], reached: &BTreeSet<ContentId>) -> Option<Vec<ContentId>> {
    let mut adjacency: BTreeMap<&ContentId, BTreeSet<&ContentId>> = BTreeMap::new();
    for edge in edges {
        if edge.kind.is_ownership()
            && reached.contains(&edge.from)
            && reached.contains(&edge.target)
        {
            adjacency
                .entry(&edge.from)
                .or_default()
                .insert(&edge.target);
        }
    }
    // 0 = unvisited, 1 = on the current path, 2 = finished.
    let mut colour: BTreeMap<&ContentId, u8> = BTreeMap::new();
    let mut stack: Vec<&ContentId> = Vec::new();
    let mut cursor: BTreeMap<&ContentId, usize> = BTreeMap::new();
    for start in reached {
        if colour.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        stack.clear();
        cursor.clear();
        stack.push(start);
        colour.insert(start, 1);
        cursor.insert(start, 0);
        while let Some(&node) = stack.last() {
            let neighbours: Vec<&ContentId> = adjacency
                .get(node)
                .map(|set| set.iter().copied().collect())
                .unwrap_or_default();
            let position = cursor.get(node).copied().unwrap_or(0);
            if position < neighbours.len() {
                cursor.insert(node, position + 1);
                let next = neighbours[position];
                match colour.get(next).copied().unwrap_or(0) {
                    0 => {
                        colour.insert(next, 1);
                        cursor.insert(next, 0);
                        stack.push(next);
                    }
                    1 => {
                        let start_index = stack
                            .iter()
                            .position(|member| *member == next)
                            .expect("a grey node is on the current path");
                        let mut cycle: Vec<ContentId> = stack[start_index..]
                            .iter()
                            .map(|member| (*member).clone())
                            .collect();
                        cycle.push(next.clone());
                        return Some(cycle);
                    }
                    _ => {}
                }
            } else {
                colour.insert(node, 2);
                stack.pop();
            }
        }
    }
    None
}

/// Hashes the closure's sorted ids, fingerprints, state and options.
fn closure_hash(
    catalog: &Catalog,
    roots: &BTreeSet<ContentId>,
    options: CompatibilityOptions,
    nodes: &BTreeMap<ContentId, NodeStatus>,
    unresolved: &[UnresolvedReference],
) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(b"cs-content-closure-v1\n");
    hasher.update(options.encode().as_bytes());
    hasher.update(b"\nroots\n");
    for root in roots {
        hasher.update(root.as_str().as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"nodes\n");
    for (id, status) in nodes {
        hasher.update(id.as_str().as_bytes());
        hasher.update(if status.ready {
            b"\tready"
        } else {
            b"\tunready"
        });
        if let Some(element) = catalog.get(id)
            && let Some(fingerprint) = &element.fingerprint
        {
            hasher.update(b"\t");
            hasher.update(fingerprint.sha256.to_hex().as_bytes());
        }
        for reason in &status.reasons {
            hasher.update(b"\t");
            hasher.update(reason.to_string().as_bytes());
        }
        hasher.update(b"\n");
    }
    hasher.update(b"unresolved\n");
    for reference in unresolved {
        hasher.update(reference.from.as_str().as_bytes());
        hasher.update(b"\t");
        hasher.update(reference.target.as_str().as_bytes());
        hasher.update(b"\t");
        hasher.update(reference.kind.label().as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize()
}

/// Renders `value` as a JSON string with the mandatory escapes.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::content::{
        CatalogElement, ConsumerKind, ContentId, Dependency, DependencyKind, NormalizeState,
        Origin, Provenance, Readiness, RuntimeConsumer, UnsupportedReason,
    };
    use cs_types::evidence::ClaimId;
    use cs_types::install::ParseState;

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("test claim id is valid")
    }

    fn designed() -> Provenance {
        Provenance::designed(claim("f14_b.test.dependency"))
    }

    fn element(
        kind: ContentKind,
        key: &str,
        deps: &[(ContentId, DependencyKind)],
        readiness: Readiness,
    ) -> CatalogElement {
        let id = ContentId::from_source(kind, key).expect("test id is valid");
        CatalogElement {
            kind,
            id,
            display_name: Some(format!("Authored {key}")),
            origin: Origin::SyntheticFixture,
            dependencies: deps
                .iter()
                .map(|(target, kind)| Dependency {
                    target: target.clone(),
                    kind: *kind,
                    provenance: designed(),
                })
                .collect(),
            parse_state: ParseState::Parsed,
            normalize_state: NormalizeState::Normalized,
            runtime_consumers: vec![RuntimeConsumer {
                kind: ConsumerKind::Gameplay,
                provenance: designed(),
            }],
            readiness,
            unsupported_reasons: if readiness.is_ready() {
                Vec::new()
            } else {
                vec![UnsupportedReason::MissingParser]
            },
            fingerprint: None,
        }
    }

    fn id(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).expect("test id is valid")
    }

    /// AC02 (closure half): the closure hash and JSON are the same for the
    /// same rows in any input enumeration order.
    #[test]
    fn accept_f14_b_closure_is_stable_under_reordering() {
        let mission = id(ContentKind::Mission, "m01");
        let rows = vec![
            element(ContentKind::Image, "scout_body", &[], Readiness::Ready),
            element(
                ContentKind::Airframe,
                "scout",
                &[(id(ContentKind::Image, "scout_body"), DependencyKind::Static)],
                Readiness::Ready,
            ),
            element(
                ContentKind::Mission,
                "m01",
                &[(id(ContentKind::Airframe, "scout"), DependencyKind::Static)],
                Readiness::Ready,
            ),
        ];

        let build = |rows: &[CatalogElement]| {
            let mut catalog = Catalog::new();
            for row in rows {
                catalog.insert(row.clone()).expect("row inserts");
            }
            Closure::compute(
                &catalog,
                std::slice::from_ref(&mission),
                CompatibilityOptions::default(),
            )
            .expect("closure computes")
        };

        let forward = build(&rows);
        let reversed_rows: Vec<CatalogElement> = rows.iter().rev().cloned().collect();
        let reversed = build(&reversed_rows);
        let shuffled = build(&[rows[1].clone(), rows[2].clone(), rows[0].clone()]);

        assert_eq!(forward.hash(), reversed.hash());
        assert_eq!(forward.hash(), shuffled.hash());
        assert_eq!(forward.to_json(), reversed.to_json());
        assert_eq!(forward.to_json(), shuffled.to_json());
        assert_eq!(forward, reversed);
        assert_eq!(forward, shuffled);
    }

    /// The closure reports a mission-to-texture chain across several edges and
    /// marks the mission unavailable when the deep texture is unsupported.
    #[test]
    fn accept_f14_b_closure_propagates_unsupported_dependencies_along_the_chain() {
        let texture = id(ContentKind::Image, "wing_tex");
        let material = id(ContentKind::Material, "wing_mat");
        let airframe = id(ContentKind::Airframe, "scout");
        let mission = id(ContentKind::Mission, "m01");

        let mut catalog = Catalog::new();
        catalog
            .insert(element(
                ContentKind::Material,
                "wing_mat",
                &[(texture.clone(), DependencyKind::Static)],
                Readiness::Ready,
            ))
            .expect("material inserts");
        catalog
            .insert(element(
                ContentKind::Airframe,
                "scout",
                &[(material.clone(), DependencyKind::Static)],
                Readiness::Ready,
            ))
            .expect("airframe inserts");
        catalog
            .insert(element(
                ContentKind::Mission,
                "m01",
                &[(airframe.clone(), DependencyKind::Static)],
                Readiness::Ready,
            ))
            .expect("mission inserts");
        catalog
            .insert(element(
                ContentKind::Image,
                "wing_tex",
                &[],
                Readiness::Unavailable,
            ))
            .expect("unsupported texture inserts");

        let closure = Closure::compute(
            &catalog,
            std::slice::from_ref(&mission),
            CompatibilityOptions::default(),
        )
        .expect("closure computes");

        assert!(
            !closure.is_ready(&mission),
            "the unsupported leaf blocks the mission"
        );
        assert!(!closure.is_ready(&airframe));
        assert!(closure.is_reached(&texture));
        assert!(
            closure
                .reasons(&mission)
                .contains(&UnsupportedReason::UnsupportedDependency {
                    target: airframe.clone()
                })
        );
        assert_eq!(
            closure.chain_to(&texture),
            Some(vec![mission, airframe, material, texture])
        );
    }

    /// An orphaned reference is explicit and keeps its parent unavailable; an
    /// ownership cycle is invalid while a reference cycle is allowed.
    #[test]
    fn accept_f14_b_orphans_and_ownership_cycles_are_reported() {
        let missing = id(ContentKind::Image, "deleted_texture");
        let mission = id(ContentKind::Mission, "m01");
        let mut catalog = Catalog::new();
        catalog
            .insert(element(
                ContentKind::Mission,
                "m01",
                &[(missing.clone(), DependencyKind::Static)],
                Readiness::Ready,
            ))
            .expect("mission inserts");

        let closure = Closure::compute(
            &catalog,
            std::slice::from_ref(&mission),
            CompatibilityOptions::default(),
        )
        .expect("closure computes");
        assert_eq!(closure.unresolved().len(), 1);
        assert_eq!(closure.unresolved()[0].target, missing);
        assert!(!closure.is_ready(&mission));
        assert_eq!(
            closure.chain_to(&missing),
            Some(vec![mission.clone(), missing.clone()])
        );

        let parent = id(ContentKind::World, "c1");
        let child = id(ContentKind::SceneNode, "root_node");
        let mut cyclic = Catalog::new();
        cyclic
            .insert(element(
                ContentKind::World,
                "c1",
                &[(child.clone(), DependencyKind::Ownership)],
                Readiness::Ready,
            ))
            .expect("world inserts");
        cyclic
            .insert(element(
                ContentKind::SceneNode,
                "root_node",
                &[(parent.clone(), DependencyKind::Ownership)],
                Readiness::Ready,
            ))
            .expect("node inserts");
        cyclic
            .insert(element(
                ContentKind::Mission,
                "m01",
                &[(parent.clone(), DependencyKind::Static)],
                Readiness::Ready,
            ))
            .expect("mission inserts");
        assert!(matches!(
            Closure::compute(&cyclic, &[mission], CompatibilityOptions::default()),
            Err(ClosureError::OwnershipCycle { .. })
        ));
    }
}
