//! Resolve each stock-livery prefix to the airframe it names, measured from
//! the original scene/airframe records (spec `F09`, stage `F09-PREFIX`, Rally
//! #386).
//!
//! F09-D measured eleven livery prefixes (`AGYRO`, `BAL`, `BLO`, `BRI`, `DEV`,
//! `FIR`, `FUR`, `HEL`, `KES`, `PEA`, `WAR`) but only as the `<PREFIX>` half of
//! a `GRAPHICS/<FACTION>/<PREFIX>_<PART>.bm` spelling; which airframe each
//! prefix names is a separate claim about the original data. This module
//! establishes it from one chain inside a GameZ container (the shared aircraft
//! archive `ZBD/planes.zbd` in the measured installation):
//!
//! ```text
//! candidate airframe root node -> subtree nodes -> mesh_index -> mesh record
//!   -> material reference -> material record -> texture-name table entry
//!   -> stored name starting with "<prefix>_" (ASCII-case-folded)
//! ```
//!
//! Every link is a production reader's own measured fact: the node array
//! ([`cs_formats::gamez::read_gamez_nodes`]), the mesh section
//! ([`cs_formats::gamez::read_gamez_meshes`]) and the material section with
//! its texture-name table ([`cs_formats::gamez::read_gamez_materials`]).
//! Nothing here guesses from a model name or from name similarity, which is
//! exactly what the F09-D limitation forbade.
//!
//! # Caller data, engine ships no idiom
//!
//! Both inputs are caller data, exactly as `cs_content::scene`'s roster
//! discovery treats its declaring-script idiom: the **candidate airframes**
//! ([`AirframeCandidate`], e.g. the rows `discover_airframe_roster` found) and
//! the **prefix list** (e.g. the prefixes `StockLiveryCatalog::discover`
//! measured over the stock-livery library). A wrong idiom produces no rows and
//! no mappings, which is falsifiable; a shipped table would be a claim with no
//! provenance.
//!
//! # The matching rule
//!
//! A container texture name matches a prefix when its ASCII-case-folded
//! spelling starts with the folded prefix followed by `_` — the same
//! `<PREFIX>_<PART>` separator the stock-livery spelling uses
//! ([`cs_content::livery::parse_livery_spelling`]). The fold is a designed
//! bridging decision: the stock library stores its prefixes uppercase while
//! the measured container stores its texture names lowercase. The separator is
//! what keeps `kestrel…`-style names from matching `kes`, and no two prefixes
//! of the measured list fold into a collision (a caller list that does is
//! refused as [`PrefixAirframeError::DuplicatePrefix`]).
//!
//! # The resolution rule
//!
//! For one prefix, every candidate whose subtree references at least one
//! matching texture is *naming evidence*. The prefix resolves to the unique
//! candidate whose set of distinct matched names is a **superset of every
//! other naming candidate's set** — i.e. the only airframe whose evidence is
//! not contained in anyone else's. Two candidates with equal or incomparable
//! evidence are [`Resolved::Unknown`] with a reason listing both; a prefix no
//! candidate names is `Unknown` with its own reason. Nothing is ever resolved
//! by position, by count-heuristics or by name similarity.
//!
//! A resolved mapping carries [`Provenance`] with class
//! [`ClaimStatus::ObservedTool`] and a [`SourceSpan`] over the container's own
//! texture-name record — the bytes that literally spell `<prefix>_…` — plus
//! the installation and container content hashes the caller measured.
//! Unresolvable links inside the walk (a mesh index past the mesh array, an
//! absent mesh slot, a material index past the material table) are counted in
//! the per-candidate evidence rather than silently skipped.

use std::collections::BTreeSet;

use cs_formats::gamez::{GameZMaterials, GameZMeshes, GameZNodes};
use cs_types::asset_id::{SourceSpan, SourceSpanError};
use cs_types::content::{ContentId, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

/// The claim a **resolved** prefix-to-airframe mapping carries.
pub const CLAIM_PREFIX_AIRFRAME: &str = "f09.prefix.airframe";

/// The claim an explicitly unknown mapping carries, whether the prefix is
/// absent from every candidate or ambiguous between two of them.
pub const CLAIM_PREFIX_UNRESOLVED: &str = "f09.prefix.airframe-unresolved";

/// One candidate airframe and the node-array record whose subtree carries its
/// geometry.
///
/// The identity is the caller's (e.g. the roster discovery's
/// `airframe/<root>` catalog element); the root is located by its array slot
/// and cross-checked against the stored node name so a stale transcription is
/// refused instead of silently resolving against the wrong subtree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirframeCandidate {
    airframe: ContentId,
    root_index: u32,
    root_name: String,
}

impl AirframeCandidate {
    /// Pairs the caller's airframe identity with the node-array slot whose
    /// subtree carries its geometry, and the stored name that slot must hold.
    #[must_use]
    pub fn new(airframe: ContentId, root_index: u32, root_name: impl Into<String>) -> Self {
        Self {
            airframe,
            root_index,
            root_name: root_name.into(),
        }
    }

    /// The airframe identity this candidate resolves prefixes to.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The node-array slot the subtree walk starts from.
    #[must_use]
    pub const fn root_index(&self) -> u32 {
        self.root_index
    }

    /// The stored node name the slot must hold.
    #[must_use]
    pub fn root_name(&self) -> &str {
        &self.root_name
    }
}

/// The measured naming evidence of one candidate for one prefix.
///
/// `distinct_names` counts distinct matched texture names; the containment
/// rule runs over those names, not over raw reference counts, so a texture
/// referenced by many meshes cannot outweigh exclusive evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixRootEvidence {
    airframe: ContentId,
    root_index: u32,
    root_name: String,
    distinct_names: usize,
    referencing_meshes: usize,
    unresolved_meshes: usize,
    unresolved_materials: usize,
}

impl PrefixRootEvidence {
    /// The naming candidate.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The node-array slot its subtree was walked from.
    #[must_use]
    pub const fn root_index(&self) -> u32 {
        self.root_index
    }

    /// The stored node name of that slot.
    #[must_use]
    pub fn root_name(&self) -> &str {
        &self.root_name
    }

    /// Distinct matched texture names the subtree references.
    #[must_use]
    pub const fn distinct_names(&self) -> usize {
        self.distinct_names
    }

    /// Mesh slots in the subtree that reference at least one matched texture.
    #[must_use]
    pub const fn referencing_meshes(&self) -> usize {
        self.referencing_meshes
    }

    /// Mesh associations in the subtree that named no present mesh (past the
    /// mesh array, or an all-zero slot), plus child slots past the node array
    /// — a link whose textures cannot be read at all. Their textures cannot
    /// be counted, so a non-zero value is a measured gap in the evidence, not
    /// a pass.
    #[must_use]
    pub const fn unresolved_meshes(&self) -> usize {
        self.unresolved_meshes
    }

    /// Material references in resolved meshes that named no material record.
    #[must_use]
    pub const fn unresolved_materials(&self) -> usize {
        self.unresolved_materials
    }
}

/// One prefix's verdict together with the evidence it was decided on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixAirframeMapping {
    prefix: String,
    mapping: Resolved<ContentId>,
    naming: Vec<PrefixRootEvidence>,
}

impl PrefixAirframeMapping {
    /// The caller's prefix spelling.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The airframe this prefix names, or the explicit unknown with its
    /// reason.
    #[must_use]
    pub fn mapping(&self) -> &Resolved<ContentId> {
        &self.mapping
    }

    /// Every candidate that names this prefix, in caller order. Empty when no
    /// candidate names it at all.
    #[must_use]
    pub fn naming(&self) -> &[PrefixRootEvidence] {
        &self.naming
    }
}

/// The full measured prefix-to-airframe map of one container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixAirframeMap {
    container: String,
    mappings: Vec<PrefixAirframeMapping>,
}

impl PrefixAirframeMap {
    /// The container the evidence was measured in, as the caller spelled it.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// One verdict per caller prefix, in caller order.
    #[must_use]
    pub fn mappings(&self) -> &[PrefixAirframeMapping] {
        &self.mappings
    }

    /// The verdict of one prefix, matched ASCII-case-insensitively against
    /// the caller's spelling.
    #[must_use]
    pub fn mapping(&self, prefix: &str) -> Option<&PrefixAirframeMapping> {
        self.mappings
            .iter()
            .find(|mapping| mapping.prefix.eq_ignore_ascii_case(prefix))
    }
}

/// Why a prefix-to-airframe resolution was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixAirframeError {
    /// A candidate names a node-array slot the container does not hold.
    CandidateNodeMissing {
        /// The identity the caller paired with the slot.
        airframe: String,
        /// The refused slot.
        root_index: u32,
        /// The slots the container does hold.
        node_count: usize,
    },
    /// A candidate's slot holds a node whose stored name differs from the one
    /// the caller declared, so the candidate is stale or misaddressed.
    CandidateRootNameMismatch {
        /// The identity whose candidate disagrees.
        airframe: String,
        /// The slot that disagreed.
        root_index: u32,
        /// The stored name the caller declared.
        expected: String,
        /// The stored name the container holds.
        actual: String,
    },
    /// Two candidates were given the same node-array slot, which would count
    /// one subtree as two airframes.
    DuplicateCandidateRoot {
        /// The doubly claimed slot.
        root_index: u32,
    },
    /// Two prefixes fold to the same spelling, so their matches cannot be
    /// told apart.
    DuplicatePrefix {
        /// The later spelling that folded onto an earlier one.
        prefix: String,
    },
    /// A prefix was empty or whitespace-only, so `<prefix>_` names no
    /// separator rule at all.
    EmptyPrefix,
    /// A source span over the measured bytes could not be built.
    Span {
        /// The container the span was anchored in.
        path: String,
        /// Why the span was refused.
        source: SourceSpanError,
    },
    /// The mapping's provenance could not be built.
    Provenance {
        /// The claim that was being built.
        claim: String,
        /// Why it was refused.
        reason: String,
    },
}

impl std::fmt::Display for PrefixAirframeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CandidateNodeMissing {
                airframe,
                root_index,
                node_count,
            } => write!(
                f,
                "{airframe} names node-array slot {root_index}, which the container does not hold \
                 ({node_count} slots)"
            ),
            Self::CandidateRootNameMismatch {
                airframe,
                root_index,
                expected,
                actual,
            } => write!(
                f,
                "{airframe} names slot {root_index} as {expected:?}, but the container stores \
                 {actual:?} there"
            ),
            Self::DuplicateCandidateRoot { root_index } => write!(
                f,
                "two candidates claim node-array slot {root_index}; one subtree is one airframe"
            ),
            Self::DuplicatePrefix { prefix } => write!(
                f,
                "prefix {prefix:?} folds onto an earlier prefix; their matches cannot be told \
                 apart"
            ),
            Self::EmptyPrefix => write!(f, "a prefix must not be empty"),
            Self::Span { path, source } => write!(f, "span over {path}: {source}"),
            Self::Provenance { claim, reason } => write!(f, "provenance for {claim}: {reason}"),
        }
    }
}

impl std::error::Error for PrefixAirframeError {}

/// The folded spelling a prefix matches texture names with: lowercased plus
/// the stock-livery separator.
fn match_head(prefix: &str) -> String {
    format!("{}_", prefix.to_ascii_lowercase())
}

/// Resolve every given prefix to the airframe whose subtree names it, measured
/// over one container's three GameZ sections.
///
/// `install_sha256` and `container_sha256` are the hashes the caller measured
/// for the installation and for the container's own bytes; they travel into
/// every resolved mapping's [`SourceSpan`], whose offset and length locate the
/// matched texture-name record that spells the prefix. See the module docs for
/// the matching and resolution rules.
///
/// # Errors
///
/// [`PrefixAirframeError`] when the caller data is inconsistent: a candidate
/// slot the container does not hold, a slot whose stored name disagrees with
/// the candidate, two candidates on one slot, an empty prefix or two prefixes
/// that fold together. A container that simply does not name a prefix is not
/// an error: that mapping is [`Resolved::Unknown`] with its reason.
#[allow(clippy::too_many_arguments)]
pub fn resolve_prefix_airframes(
    container: &str,
    install_sha256: ContentHash,
    container_sha256: ContentHash,
    nodes: &GameZNodes,
    meshes: &GameZMeshes,
    materials: &GameZMaterials,
    candidates: &[AirframeCandidate],
    prefixes: &[String],
) -> Result<PrefixAirframeMap, PrefixAirframeError> {
    // Caller-data validation: an inconsistent input is refused, never repaired.
    let mut seen_roots: BTreeSet<u32> = BTreeSet::new();
    for candidate in candidates {
        let Some(node) = nodes.get(candidate.root_index) else {
            return Err(PrefixAirframeError::CandidateNodeMissing {
                airframe: candidate.airframe.as_str().to_owned(),
                root_index: candidate.root_index,
                node_count: nodes.nodes.len(),
            });
        };
        if node.name != candidate.root_name {
            return Err(PrefixAirframeError::CandidateRootNameMismatch {
                airframe: candidate.airframe.as_str().to_owned(),
                root_index: candidate.root_index,
                expected: candidate.root_name.clone(),
                actual: node.name.clone(),
            });
        }
        if !seen_roots.insert(candidate.root_index) {
            return Err(PrefixAirframeError::DuplicateCandidateRoot {
                root_index: candidate.root_index,
            });
        }
    }
    let mut seen_prefixes: BTreeSet<String> = BTreeSet::new();
    for prefix in prefixes {
        if prefix.trim().is_empty() {
            return Err(PrefixAirframeError::EmptyPrefix);
        }
        if !seen_prefixes.insert(prefix.to_ascii_lowercase()) {
            return Err(PrefixAirframeError::DuplicatePrefix {
                prefix: prefix.clone(),
            });
        }
    }

    let mut mappings = Vec::with_capacity(prefixes.len());
    for prefix in prefixes {
        let head = match_head(prefix);
        // One walk per candidate: the subtree's distinct matched names, the
        // mesh slots that reference them, and the links that did not resolve.
        let mut per_candidate: Vec<Observation> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let mut stack = vec![candidate.root_index];
            let mut visited: BTreeSet<u32> = BTreeSet::new();
            let mut names: BTreeSet<String> = BTreeSet::new();
            let mut referencing_meshes = 0usize;
            let mut unresolved_meshes = 0usize;
            let mut unresolved_materials = 0usize;
            // The matched texture record with the smallest offset: the span a
            // resolved mapping points at.
            let mut anchor: Option<(u64, u64)> = None;
            while let Some(index) = stack.pop() {
                // A cycle in the child links cannot loop the walk; the visited
                // set is the guard, and nothing is counted twice.
                if !visited.insert(index) {
                    continue;
                }
                let Some(node) = nodes.get(index) else {
                    // A child slot past the array is a container defect; the
                    // walk reports it through the unresolved count of the
                    // node that held it, never by panicking.
                    unresolved_meshes += 1;
                    continue;
                };
                for child in &node.children {
                    stack.push(*child);
                }
                if node.mesh_index() < 0 {
                    continue;
                }
                let Some(mesh) = meshes.get(node.mesh_index() as u32) else {
                    unresolved_meshes += 1;
                    continue;
                };
                let mut names_this_mesh = false;
                for reference in &mesh.materials {
                    let Some(material) = materials.material(reference.material_index) else {
                        unresolved_materials += 1;
                        continue;
                    };
                    let Some(texture) = materials.texture_of(material) else {
                        continue;
                    };
                    if !texture.name.to_ascii_lowercase().starts_with(&head) {
                        continue;
                    }
                    names.insert(texture.name.to_ascii_lowercase());
                    names_this_mesh = true;
                    let span = (texture.data_offset, texture.data_end - texture.data_offset);
                    if anchor.is_none_or(|current| span.0 < current.0) {
                        anchor = Some(span);
                    }
                }
                if names_this_mesh {
                    referencing_meshes += 1;
                }
            }
            per_candidate.push(Observation {
                candidate,
                names,
                referencing_meshes,
                unresolved_meshes,
                unresolved_materials,
                anchor,
            });
        }

        let naming: Vec<&Observation> = per_candidate
            .iter()
            .filter(|observation| !observation.names.is_empty())
            .collect();

        let mapping = if naming.is_empty() {
            Resolved::unknown(
                ClaimId::new(CLAIM_PREFIX_UNRESOLVED).map_err(|error| {
                    PrefixAirframeError::Provenance {
                        claim: CLAIM_PREFIX_UNRESOLVED.to_owned(),
                        reason: error.to_string(),
                    }
                })?,
                &format!(
                    "no candidate airframe's subtree in {container} references a texture whose \
                     stored name starts with {head:?}"
                ),
            )
            .map_err(|error| PrefixAirframeError::Provenance {
                claim: CLAIM_PREFIX_UNRESOLVED.to_owned(),
                reason: error.to_string(),
            })?
        } else {
            // The unique candidate whose evidence contains every other naming
            // candidate's evidence. Equal or incomparable sets leave no
            // unique such candidate, and that is an explicit unknown.
            let winners: Vec<&Observation> = naming
                .iter()
                .copied()
                .filter(|observation| {
                    naming
                        .iter()
                        .all(|other| other.names.is_subset(&observation.names))
                })
                .collect();
            match winners.as_slice() {
                [winner] => {
                    let offset = winner
                        .anchor
                        .expect("a naming candidate has a matched record")
                        .0;
                    let length = winner
                        .anchor
                        .expect("a naming candidate has a matched record")
                        .1;
                    let span = SourceSpan::new(
                        install_sha256,
                        container,
                        None,
                        offset,
                        length,
                        Some(container_sha256),
                    )
                    .map_err(|source| PrefixAirframeError::Span {
                        path: container.to_owned(),
                        source,
                    })?;
                    let provenance = Provenance::new(
                        ClaimId::new(CLAIM_PREFIX_AIRFRAME).map_err(|error| {
                            PrefixAirframeError::Provenance {
                                claim: CLAIM_PREFIX_AIRFRAME.to_owned(),
                                reason: error.to_string(),
                            }
                        })?,
                        ClaimStatus::ObservedTool,
                        Some(span),
                    )
                    .map_err(|error| PrefixAirframeError::Provenance {
                        claim: CLAIM_PREFIX_AIRFRAME.to_owned(),
                        reason: error.to_string(),
                    })?;
                    Resolved::Known(Known::new(winner.candidate.airframe.clone(), provenance))
                }
                _ => {
                    let detail = naming
                        .iter()
                        .map(|observation| {
                            let exclusive = observation
                                .names
                                .iter()
                                .filter(|name| {
                                    naming.iter().all(|other| {
                                        std::ptr::eq(*other, *observation)
                                            || !other.names.contains(*name)
                                    })
                                })
                                .count();
                            format!(
                                "{} names {} distinct name(s), {} of them no other candidate \
                                 names",
                                observation.candidate.airframe.as_str(),
                                observation.names.len(),
                                exclusive
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ");
                    Resolved::unknown(
                        ClaimId::new(CLAIM_PREFIX_UNRESOLVED).map_err(|error| {
                            PrefixAirframeError::Provenance {
                                claim: CLAIM_PREFIX_UNRESOLVED.to_owned(),
                                reason: error.to_string(),
                            }
                        })?,
                        &format!(
                            "prefix {prefix:?} has naming evidence in {container} that no single \
                             candidate contains: {detail}"
                        ),
                    )
                    .map_err(|error| PrefixAirframeError::Provenance {
                        claim: CLAIM_PREFIX_UNRESOLVED.to_owned(),
                        reason: error.to_string(),
                    })?
                }
            }
        };

        let evidence = naming
            .iter()
            .map(|observation| PrefixRootEvidence {
                airframe: observation.candidate.airframe.clone(),
                root_index: observation.candidate.root_index,
                root_name: observation.candidate.root_name.clone(),
                distinct_names: observation.names.len(),
                referencing_meshes: observation.referencing_meshes,
                unresolved_meshes: observation.unresolved_meshes,
                unresolved_materials: observation.unresolved_materials,
            })
            .collect();
        mappings.push(PrefixAirframeMapping {
            prefix: prefix.clone(),
            mapping,
            naming: evidence,
        });
    }

    Ok(PrefixAirframeMap {
        container: container.to_owned(),
        mappings,
    })
}

/// One candidate's measured evidence for one prefix, as the walk gathered it.
struct Observation<'a> {
    /// The candidate whose subtree was walked.
    candidate: &'a AirframeCandidate,
    /// The distinct matched texture names, folded.
    names: BTreeSet<String>,
    /// Mesh slots in the subtree referencing at least one matched name.
    referencing_meshes: usize,
    /// Mesh associations that named no present mesh.
    unresolved_meshes: usize,
    /// Material references that named no material record.
    unresolved_materials: usize,
    /// The matched texture record with the smallest offset: `(offset, length)`.
    anchor: Option<(u64, u64)>,
}

/// Convenience access for callers that hold the mappings as a map and want
/// the resolved airframe of one prefix.
impl PrefixAirframeMap {
    /// The resolved airframe of one prefix, or `None` when the prefix was not
    /// in the caller's list.
    #[must_use]
    pub fn resolved_airframe(&self, prefix: &str) -> Option<&Resolved<ContentId>> {
        self.mapping(prefix).map(PrefixAirframeMapping::mapping)
    }

    /// The number of prefixes this map was resolved for.
    #[must_use]
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    /// Whether the map holds no prefixes at all — an empty caller list, which
    /// is a search that was never asked, not an answer of "none".
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }
}
