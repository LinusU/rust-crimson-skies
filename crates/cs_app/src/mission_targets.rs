//! M01's declared ground targets and the world group's authored damage set
//! (M01-BRIDGE-CONVOY-SPAWN, Rally #1566).
//!
//! `docs/findings/2026-10-17-m01-ground-units-locate.md` (#1549) measured
//! where M01 "The Lost Treasure" keeps its bridge convoy, its camps and its
//! wreck: they are ordinary records of `ZBD/C3/gamez.zbd`, listed by the
//! mission's own `targets.zrd`, and given a `healthy`/`destroyed` layout and a
//! destruction chain by the world group's `ZBD/C3/zrdr.zbd` animation members
//! (`fueltruck.zrd`, `ucamp_dest.zrd`, `shipwreck.zrd`, …). None of them was
//! bound to anything: the world import makes an object only for a record the
//! world node owns directly, and no consumer read the mission's targets or the
//! authored damage coupling.
//!
//! This module is that binding. It reads two production sources and joins
//! them:
//!
//! * **The mission's `targets.zrd`** (the reader archive's member, in the
//!   original's mission → group → shared mount order), through
//!   [`read_mission_targets`]. Each record is a list of `[key, value]` pairs —
//!   `description`, `nodes`, `help_label`, `category_label` and the two
//!   marker keys `objective` / `other_target` — exactly the pair shape
//!   `cs_content::scenario_control` measures for the instant-action readers.
//! * **The world group's damage set**, through [`read_damage_couplings`]:
//!   every `.zrd` member of the group's reader archive that decodes as an
//!   `ANIMATION_DEFINITIONS` record is read through the production
//!   [`read_animation_definition_member`], and every definition that states a
//!   damage-coupling field ([`DAMAGE_COUPLING_FIELDS`]) is kept with the node
//!   selectors it names.
//!
//! [`bind_mission_targets`] joins them against the world container: every
//! declared target's node chain and every damage definition's node selector is
//! resolved against the container's stored names through the production
//! [`WorldNodeNames`] rule, the node's **composed world position** is read
//! from the production scene graph ([`SceneNode::world_transform`] — the one
//! pose owner, `root ∘ … ∘ local`), and the node's authored damage layout (its
//! `healthy`/`destroyed` children) is read off the container hierarchy.
//!
//! # What is measured, and what stays unknown
//!
//! Every value this module binds is an installation read (`ObservedTool`):
//! the row text, the node names, the composed position and the
//! `healthy`/`destroyed` child names. What is **not** measured is the meaning
//! of the coupling itself: `HEALTH`, `DAMAGE_SEQUENCE`, `PROXIMITY_DAMAGE` and
//! `COPY_NODE_DATA` state an authored destruction chain whose thresholds and
//! sequence roles are F29's measurement
//! ([`UnmeasuredFieldFamily::DamageCoupling`](crate::animation::programs::UnmeasuredFieldFamily::DamageCoupling)),
//! and the node selector spellings the container cannot answer (a node path,
//! a wildcard with a narrowing suffix) stay refusals rather than counts. The
//! authored `HEALTH` **value** is therefore never turned into an integrity
//! pool: the damage node this module builds carries the labelled designed
//! single unit of [`DESIGNED_TARGET_INTEGRITY`] (the same shape
//! `mission_session::parts` uses for a zeppelin part), so a routed hit can
//! take a target out of play at all, and the measured replacement is the
//! F29/F20 damage model that does not exist yet.
//!
//! Nothing here is `verified_original`: the coupling is read from the
//! installation and its semantics are explicitly open.

use std::path::Path;

use crate::mission_world_actors::{AllegianceBinding, resolve_world_node_allegiance};
use cs_assets::install::Discovery;
use cs_content::coordinates::SourceAdapter;
use cs_content::scene::{BindingMap, SceneGraph, SceneNode};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_content::world::world_scene_graph_from_gamez;
use cs_formats::gamez::{GameZNodes, read_gamez_nodes};
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discovery::discover_container;
use cs_sim::damage::{DamageNode, DamageNodeKey, DamageNodeKind};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::InstallFileRecord;

use crate::animation::programs::{
    ObjectSelector, SelectorMatch, WorldNodeNames, read_animation_definition_member,
};

/// The reader member a mission's own target list lives in (#1549).
pub const TARGETS_MEMBER: &str = "targets.zrd";

/// The `ANIMATION_DEFINITION` fields that couple an authored destruction
/// chain to its node (#1492's measured family vocabulary).
pub const DAMAGE_COUPLING_FIELDS: [&str; 4] = [
    "HEALTH",
    "DAMAGE_SEQUENCE",
    "PROXIMITY_DAMAGE",
    "COPY_NODE_DATA",
];

/// The first `targets.zrd` marker key, verbatim.
pub const TARGET_OBJECTIVE_KEY: &str = "objective";

/// The second `targets.zrd` marker key, verbatim.
pub const TARGET_OTHER_TARGET_KEY: &str = "other_target";

/// The claim a `targets.zrd` row's own text is filed under: the member
/// decoded and the row's pairs read as spelled.
pub const TARGET_ROW_CLAIM: &str = "f34-world.mission-target-row";

/// The claim a bound target's composed world position is filed under: the
/// container node resolved and the scene graph's composed transform read.
pub const TARGET_POSITION_CLAIM: &str = "f34-world.mission-target-position";

/// The claim a target's authored `healthy`/`destroyed` layout is filed under.
pub const TARGET_LAYOUT_CLAIM: &str = "f34-world.mission-target-damage-layout";

/// The claim the authored damage coupling is read under, and its meaning
/// left open under: the field names are measured, the thresholds and the
/// sequences' roles are not.
pub const TARGET_COUPLING_CLAIM: &str = "f20-anim.damage-coupling-unmeasured";

/// The claim the designed integrity of a target's damage node is filed under.
pub const M01_TARGET_INTEGRITY_CLAIM: &str = "m01-bridge-convoy-spawn.designed-target-integrity";

/// The designed integrity pool of one target node: **one damage unit**.
///
/// Not a measurement. The authored `HEALTH` value is read
/// ([`DamageCouplingDefinition`]) but its unit and the damage model it feeds
/// are F29's unmeasured surface, and the workspace's own precedent
/// (`mission_session::parts::DESIGNED_PART_INTEGRITY`) is a labelled designed
/// single unit rather than an authored number used as a pool. A damage node
/// whose pool is [`Resolved::Unknown`] blocks every hit, so the destruction
/// path could never take a target out of play at all; one unit is the least a
/// pool can be while letting *any* damage destroy the target. Every target
/// gets the same value, so this is explicitly not a per-target tuning table.
pub const DESIGNED_TARGET_INTEGRITY: f64 = 1.0;

/// One `targets.zrd` record, as spelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetRow {
    /// The record's position in the member, in stored order.
    pub index: usize,
    /// The `description` message name, when the record states one.
    pub description: Option<String>,
    /// The `nodes` value: each element is one node chain, root-to-leaf as
    /// spelled. A single-element chain is a bare node name.
    pub chains: Vec<Vec<String>>,
    /// Whether the record carries the `objective` marker.
    pub objective: bool,
    /// Whether the record carries the `other_target` marker.
    pub other_target: bool,
    /// The `help_label` message name, when the record states one.
    pub help_label: Option<String>,
    /// The `category_label` message name, when the record states one.
    pub category_label: Option<String>,
}

impl TargetRow {
    /// The row's own provenance: its text is read from the member verbatim.
    #[must_use]
    pub fn provenance(&self) -> Provenance {
        Provenance::new(claim(TARGET_ROW_CLAIM), ClaimStatus::ObservedTool, None)
            .expect("an observed-tool provenance needs no source span")
    }
}

/// The scope's `targets.zrd`, read in the original's mount order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissionTargetTable {
    /// The records, in stored order.
    pub rows: Vec<TargetRow>,
    /// The logical key of the container the member decoded from, or `None`
    /// when no searched carrier held it.
    pub carrier: Option<String>,
    /// Why no table could be read, verbatim: a carrier that could hold the
    /// member could not be checked, or the member refused to decode. A
    /// refusal is never folded into an empty [`Self::rows`], so an unread
    /// member cannot pass for a mission that declares no target.
    pub refusal: Option<String>,
}

/// One `ANIMATION_DEFINITION` that states a damage-coupling field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DamageCouplingDefinition {
    /// The member it decoded from, as the archive's index spells it.
    pub member: String,
    /// The definition's `ANIMATION_NAME`, when it states one.
    pub animation_name: Option<String>,
    /// The stored node selectors the definition names, verbatim — a `NAME`
    /// entry, or a `NAME1` path's terminal step.
    pub selectors: Vec<String>,
    /// The damage-coupling field names the definition states, in
    /// [`DAMAGE_COUPLING_FIELDS`] order.
    pub fields: Vec<String>,
}

/// The group reader archive's damage set, read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageCouplingTable {
    /// Every definition stating a coupling field, in member then stored order.
    pub definitions: Vec<DamageCouplingDefinition>,
    /// The logical key of the archive the definitions came from.
    pub carrier: Option<String>,
    /// Why the archive could not be read whole, verbatim.
    pub refusal: Option<String>,
}

/// The damage coupling the world group states for one node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DamageCoupling {
    /// The coupling field names, in [`DAMAGE_COUPLING_FIELDS`] order.
    pub fields: Vec<String>,
    /// The animation members that state them, in read order.
    pub members: Vec<String>,
}

impl DamageCoupling {
    /// Whether the world group states any coupling for the node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// The state a bound target's allegiance holds until the resolver has run.
const NOT_RESOLVED: &str = "the node resolver has not run over this binding";

/// One node bound as a damageable target of the composition.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundTarget {
    /// The `targets.zrd` row that declared it, when one did.
    pub declared: Option<usize>,
    /// The node chain the target was declared by: the `targets.zrd` row's
    /// chain, or the single selector that matched.
    pub chain: Vec<String>,
    /// The container record's stored display name.
    pub node: String,
    /// The container record's array slot.
    pub index: u32,
    /// The record's authored name-path from the container root.
    pub path: String,
    /// The composed world position in metres: the scene graph's own
    /// `world_transform` translation (`root ∘ … ∘ local`).
    pub position_m: [f64; 3],
    /// The point the engine's target record holds for the node (`record+0x68`
    /// of the selected target): the composed world transform applied to the
    /// centre of the node's stored bounding box. Equal to
    /// [`Self::position_m`] when the node stores no box. See
    /// `docs/findings/2026-10-21-m01-objective-target-points.md`.
    pub target_point_m: [f64; 3],
    /// The node's child named `healthy`, when it has one.
    pub healthy: Option<String>,
    /// The node's child named `destroyed`, when it has one.
    pub destroyed: Option<String>,
    /// The damage coupling the world group states for this node.
    pub coupling: DamageCoupling,
    /// Whether the declaring row carried the `objective` marker.
    pub objective: bool,
    /// Whether the declaring row carried the `other_target` marker.
    pub other_target: bool,
    /// The declaring row's `description` message name (`MSG_OBJ_SITE1`).
    pub description: Option<String>,
    /// The declaring row's `help_label` message name (`MSG_OBJ_EXAMINE`).
    pub help_label: Option<String>,
    /// What the engine's node resolver (`0x4bf080` → `0x4bef90`) answers for
    /// the node's subtree (M01-TARGET-ROSTER, #1592): the measured team int
    /// and the source that settled it, or why none can be claimed.
    pub allegiance: AllegianceBinding,
}

impl BoundTarget {
    /// The composed position's provenance: the container read plus the
    /// production scene graph's composed transform.
    #[must_use]
    pub fn position_provenance(&self) -> Provenance {
        Provenance::new(
            claim(TARGET_POSITION_CLAIM),
            ClaimStatus::ObservedTool,
            None,
        )
        .expect("an observed-tool provenance needs no source span")
    }

    /// Whether the node carries the authored `healthy`/`destroyed` layout.
    #[must_use]
    pub const fn has_destroy_layout(&self) -> bool {
        self.healthy.is_some() || self.destroyed.is_some()
    }

    /// The damage node key this target is addressable by: its own node name.
    ///
    /// # Errors
    ///
    /// The node name is not a valid [`DamageNodeKey`] spelling.
    pub fn damage_key(&self) -> Result<DamageNodeKey, String> {
        DamageNodeKey::new(&self.node).map_err(|error| error.to_string())
    }
}

/// What [`bind_mission_targets`] bound, and every refusal by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MissionTargetBinding {
    /// The bound targets, in (declared row, then damage definition) order.
    pub targets: Vec<BoundTarget>,
    /// The `targets.zrd` row indices that bound at least one node, in row
    /// order.
    pub declared: Vec<usize>,
    /// Every selector the container could not answer and every unreadable
    /// source, verbatim.
    pub refusals: Vec<String>,
}

impl MissionTargetBinding {
    /// Whether nothing was bound at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The bound target with this container node name, when one was bound.
    #[must_use]
    pub fn target(&self, node: &str) -> Option<&BoundTarget> {
        self.targets.iter().find(|target| target.node == node)
    }

    /// Every bound target the mission's own `targets.zrd` declared.
    pub fn declared_targets(&self) -> impl Iterator<Item = &BoundTarget> {
        self.targets
            .iter()
            .filter(|target| target.declared.is_some())
    }

    /// Every bound target the world group's damage set names.
    pub fn damage_targets(&self) -> impl Iterator<Item = &BoundTarget> {
        self.targets
            .iter()
            .filter(|target| !target.coupling.is_empty())
    }
}

/// A claim id this module declares.
fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a claim id this module declares")
}

/// The scope's `targets.zrd`, in the original's mount order — mission, group,
/// shared.
///
/// The search mirrors the other scope reads of this tree
/// (`mission_session::parts::read_scope_turrets`): the first archive that
/// lists a member named `targets.zrd` wins, an archive missing from the
/// manifest or unreadable on disk is kept as a reason for the next one, and a
/// member that refuses to decode is a refusal rather than an empty table.
/// Only when every carrier was checked and none held the member is the
/// absence itself measured.
#[must_use]
pub fn read_mission_targets(
    install_root: &Path,
    mission_dir: &str,
    group_dir: &str,
) -> MissionTargetTable {
    match cs_assets::install::discover(install_root) {
        Ok(found) => mission_targets_from(&found, install_root, mission_dir, group_dir),
        Err(error) => MissionTargetTable {
            refusal: Some(format!("the installation cannot be discovered: {error}")),
            ..MissionTargetTable::default()
        },
    }
}

/// [`read_mission_targets`] over an already-discovered installation, so one
/// binding pass pays for the inventory scan once.
fn mission_targets_from(
    found: &Discovery,
    install_root: &Path,
    mission_dir: &str,
    group_dir: &str,
) -> MissionTargetTable {
    let archives = [
        format!("{mission_dir}/zrdr.zbd"),
        format!("{group_dir}/zrdr.zbd"),
        "zbd/zrdr.zbd".to_owned(),
    ];
    let mut unreadable: Option<String> = None;
    for archive in &archives {
        let Some(record) = manifest_record(found, archive) else {
            unreadable = unreadable.or(Some(format!("{archive} is not in the manifest")));
            continue;
        };
        let bytes = match read_container_bytes(install_root, record) {
            Ok(bytes) => bytes,
            Err(reason) => {
                unreadable = unreadable.or(Some(reason));
                continue;
            }
        };
        let discovery = discover_container(archive, &record.relative_spelling, &bytes);
        let Some(program) = discovery.programs().iter().find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(TARGETS_MEMBER))
        }) else {
            continue;
        };
        let decoded = match decode_zrd(program.bytes()) {
            Ok(decoded) => decoded,
            Err(error) => {
                return MissionTargetTable {
                    refusal: Some(format!("{archive}'s {TARGETS_MEMBER} refuses: {error}")),
                    ..MissionTargetTable::default()
                };
            }
        };
        return MissionTargetTable {
            rows: read_target_rows(&decoded),
            carrier: Some(archive.clone()),
            refusal: None,
        };
    }
    match unreadable {
        Some(reason) => MissionTargetTable {
            refusal: Some(reason),
            ..MissionTargetTable::default()
        },
        None => MissionTargetTable::default(),
    }
}

/// Reads one decoded `targets.zrd` document's records.
///
/// The measured shape is the pair walk `cs_content::scenario_control` uses for
/// the instant-action readers: the root is the list of records and each record
/// is a list of `[key, value]` pairs. A record that is not a pair list still
/// contributes a row — a target that cannot be read is not dropped — and an
/// unrecognized key is ignored rather than guessed into a field.
#[must_use]
pub fn read_target_rows(document: &ZrdValue) -> Vec<TargetRow> {
    let mut rows = Vec::new();
    for (index, record) in document.as_list().unwrap_or_default().iter().enumerate() {
        let mut row = TargetRow {
            index,
            description: None,
            chains: Vec::new(),
            objective: false,
            other_target: false,
            help_label: None,
            category_label: None,
        };
        for child in record.as_list().unwrap_or_default() {
            let Some(pair) = child.as_list() else {
                continue;
            };
            let Some(key) = pair.first().and_then(ZrdValue::as_text) else {
                continue;
            };
            match key {
                "description" => {
                    row.description = pair.get(1).and_then(ZrdValue::as_text).map(str::to_owned);
                }
                "help_label" => {
                    row.help_label = pair.get(1).and_then(ZrdValue::as_text).map(str::to_owned);
                }
                "category_label" => {
                    row.category_label = pair.get(1).and_then(ZrdValue::as_text).map(str::to_owned);
                }
                TARGET_OBJECTIVE_KEY => row.objective = true,
                TARGET_OTHER_TARGET_KEY => row.other_target = true,
                "nodes" => row.chains = read_target_chains(pair.get(1)),
                _ => {}
            }
        }
        rows.push(row);
    }
    rows
}

/// The node chains a `nodes` value declares: each element is one chain, either
/// a bare name or a list of names from root to leaf.
fn read_target_chains(value: Option<&ZrdValue>) -> Vec<Vec<String>> {
    let mut chains = Vec::new();
    for element in value.and_then(ZrdValue::as_list).unwrap_or_default() {
        match element {
            ZrdValue::Text(name) => chains.push(vec![name.clone()]),
            ZrdValue::List(steps) => {
                let chain: Vec<String> = steps
                    .iter()
                    .filter_map(|step| step.as_text().map(str::to_owned))
                    .collect();
                if !chain.is_empty() {
                    chains.push(chain);
                }
            }
            _ => {}
        }
    }
    chains
}

/// The world group's authored damage set, read from every `.zrd` member of its
/// reader archive.
///
/// A member that decodes as an `ANIMATION_DEFINITIONS` record is read through
/// the production [`read_animation_definition_member`]; every definition that
/// states a damage-coupling field is kept. A member whose bytes spell a
/// coupling field but which the production reader refuses is a **refusal**,
/// never a silent skip: a damage member nobody could read must not pass for a
/// world that declares no damage.
#[must_use]
pub fn read_damage_couplings(install_root: &Path, group_dir: &str) -> DamageCouplingTable {
    match cs_assets::install::discover(install_root) {
        Ok(found) => damage_couplings_from(&found, install_root, group_dir),
        Err(error) => DamageCouplingTable {
            refusal: Some(format!("the installation cannot be discovered: {error}")),
            ..DamageCouplingTable::default()
        },
    }
}

/// [`read_damage_couplings`] over an already-discovered installation.
fn damage_couplings_from(
    found: &Discovery,
    install_root: &Path,
    group_dir: &str,
) -> DamageCouplingTable {
    let archive = format!("{group_dir}/zrdr.zbd");
    let Some(record) = manifest_record(found, &archive) else {
        return DamageCouplingTable {
            refusal: Some(format!("{archive} is not in the manifest")),
            ..DamageCouplingTable::default()
        };
    };
    let bytes = match read_container_bytes(install_root, record) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return DamageCouplingTable {
                refusal: Some(reason),
                ..DamageCouplingTable::default()
            };
        }
    };
    let discovery = discover_container(&archive, &record.relative_spelling, &bytes);
    let mut definitions = Vec::new();
    let mut refusals = Vec::new();
    for program in discovery.programs() {
        let Some(name) = program.locator().member() else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".zrd") {
            continue;
        }
        let spells_coupling = DAMAGE_COUPLING_FIELDS
            .iter()
            .any(|field| contains_bytes(program.bytes(), field.as_bytes()));
        match read_animation_definition_member(name, program.bytes()) {
            Ok(member) => {
                for definition in member.definitions() {
                    let fields: Vec<String> = DAMAGE_COUPLING_FIELDS
                        .iter()
                        .filter(|field| {
                            definition
                                .uninterpreted_fields()
                                .iter()
                                .any(|stored| stored == *field)
                        })
                        .map(|field| (*field).to_owned())
                        .collect();
                    if fields.is_empty() {
                        continue;
                    }
                    let selectors = definition
                        .objects()
                        .node_names()
                        .map(|selector| selector.stored().to_owned())
                        .collect();
                    definitions.push(DamageCouplingDefinition {
                        member: name.to_owned(),
                        animation_name: definition.animation_name().map(str::to_owned),
                        selectors,
                        fields,
                    });
                }
            }
            Err(error) => {
                if spells_coupling {
                    refusals.push(format!(
                        "{archive}'s {name} spells a damage-coupling field but does not read as an \
                         animation-definition member: {error}"
                    ));
                }
            }
        }
    }
    DamageCouplingTable {
        definitions,
        carrier: Some(archive),
        refusal: if refusals.is_empty() {
            None
        } else {
            Some(refusals.join("; "))
        },
    }
}

/// Whether `haystack` holds `needle` as a byte substring.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Binds the mission's declared targets and the world group's damage set to the
/// mission's world container.
///
/// Every declared node chain and every damage definition's node selector is
/// resolved against the container's own stored names through
/// [`WorldNodeNames`], the composed world position is read from the production
/// scene graph, and the node's `healthy`/`destroyed` children are read off the
/// container hierarchy. A selector the node-name table cannot answer (a node
/// path, a wildcard with a narrowing suffix) is a refusal, never a guess; a
/// selector that answers zero names is a measured absence.
#[must_use]
pub fn bind_mission_targets(
    install_root: &Path,
    mission_dir: &str,
    group_dir: &str,
) -> MissionTargetBinding {
    let found = match cs_assets::install::discover(install_root) {
        Ok(found) => found,
        Err(error) => {
            return MissionTargetBinding {
                targets: Vec::new(),
                declared: Vec::new(),
                refusals: vec![format!("the installation cannot be discovered: {error}")],
            };
        }
    };
    bind_mission_targets_from(&found, install_root, mission_dir, group_dir)
}

/// [`bind_mission_targets`] over an already-discovered installation.
fn bind_mission_targets_from(
    found: &Discovery,
    install_root: &Path,
    mission_dir: &str,
    group_dir: &str,
) -> MissionTargetBinding {
    let mut refusals = Vec::new();
    let table = mission_targets_from(found, install_root, mission_dir, group_dir);
    if let Some(reason) = &table.refusal {
        refusals.push(format!(
            "the mission's {TARGETS_MEMBER} could not be read ({reason}), so no declared target \
             is bound"
        ));
    }
    let couplings = damage_couplings_from(found, install_root, group_dir);
    if let Some(reason) = &couplings.refusal {
        refusals.push(format!(
            "the world group's damage set could not be read whole ({reason})"
        ));
    }

    let (graph, nodes) = match read_world_scene_from(found, install_root, group_dir) {
        Ok(scene) => scene,
        Err(reason) => {
            refusals.push(format!(
                "the world container could not be read, so no target resolves: {reason}"
            ));
            return MissionTargetBinding {
                targets: Vec::new(),
                declared: Vec::new(),
                refusals,
            };
        }
    };
    let names = WorldNodeNames::from_gamez(group_dir, &nodes);
    let mut targets: Vec<BoundTarget> = Vec::new();
    let mut declared = Vec::new();

    for row in &table.rows {
        let mut bound_any = false;
        for chain in &row.chains {
            match resolve_chain(&graph, chain) {
                Ok(node) => {
                    bound_any = true;
                    let coupling = coupling_for(&couplings, node.name());
                    merge_target(
                        &mut targets,
                        &graph,
                        node,
                        chain.clone(),
                        Some(row.index),
                        row.objective,
                        row.other_target,
                        (row.description.clone(), row.help_label.clone()),
                        coupling,
                    );
                }
                Err(reason) => refusals.push(format!(
                    "{} row {}: the chain {} does not resolve: {reason}",
                    TARGETS_MEMBER,
                    row.index,
                    chain.join("/")
                )),
            }
        }
        if bound_any {
            declared.push(row.index);
        }
    }

    for definition in &couplings.definitions {
        for selector in &definition.selectors {
            let matched =
                match resolve_selector(&names, selector, &definition.member, &mut refusals) {
                    Some(matched) => matched,
                    None => continue,
                };
            for name in matched {
                let Some(node) = graph.nodes().iter().find(|node| node.name() == name) else {
                    refusals.push(format!(
                        "{}: the selector {selector:?} named {name}, which the container's names \
                         carry but the scene graph did not convert",
                        definition.member
                    ));
                    continue;
                };
                let coupling = DamageCoupling {
                    fields: definition.fields.clone(),
                    members: vec![definition.member.clone()],
                };
                merge_target(
                    &mut targets,
                    &graph,
                    node,
                    vec![name],
                    None,
                    false,
                    false,
                    (None, None),
                    coupling,
                );
            }
        }
    }

    for target in &mut targets {
        if let Some(node) = graph
            .nodes()
            .iter()
            .find(|node| node.index() == target.index)
            && let Some(raw) = nodes.get(target.index)
        {
            target.target_point_m = target_point(node, raw);
        }
    }

    let names_bound: Vec<&str> = targets.iter().map(|target| target.node.as_str()).collect();
    let resolved =
        resolve_world_node_allegiance(install_root, found, mission_dir, group_dir, &names_bound);
    for (target, (_, allegiance)) in targets.iter_mut().zip(resolved) {
        target.allegiance = allegiance;
    }

    MissionTargetBinding {
        targets,
        declared,
        refusals,
    }
}

/// The target record's point for one node: its composed transform applied to
/// the centre of its stored bounding box.
///
/// The record stores up to three boxes (`unk116`, `unk140`, `unk164`) and the
/// measured sites each fill exactly one of them; the first non-empty one is
/// used, and a node with none keeps its origin. Which box the engine reads
/// for a node holding several is unmeasured.
fn target_point(node: &SceneNode, raw: &cs_formats::gamez::RawNode) -> [f64; 3] {
    let info = &raw.info;
    let centre = [info.unk116, info.unk140, info.unk164]
        .into_iter()
        .find(|bounds| bounds.iter().flatten().any(|value| *value != 0.0))
        .map(|[min, max]| {
            std::array::from_fn(|axis| (f64::from(min[axis]) + f64::from(max[axis])) / 2.0)
        });
    match centre {
        Some(centre) => node.world_transform().apply(centre),
        None => node.world_transform().translation(),
    }
}

/// The coupling the world group states for the node named `node`.
fn coupling_for(couplings: &DamageCouplingTable, node: &str) -> DamageCoupling {
    let mut coupling = DamageCoupling::default();
    for definition in &couplings.definitions {
        let states_node = definition.selectors.iter().any(|selector| {
            ObjectSelector::parse(selector)
                .ok()
                .is_some_and(|parsed| selector_matches(&parsed, node))
        });
        if states_node && !coupling.members.contains(&definition.member) {
            coupling.members.push(definition.member.clone());
            for field in &definition.fields {
                if !coupling.fields.contains(field) {
                    coupling.fields.push(field.clone());
                }
            }
        }
    }
    coupling
}

/// Resolves one stored node selector against the container's names.
///
/// `None` is a refusal already pushed: a node path or a wildcard with a
/// narrowing suffix the corpus does not establish. `Some(empty)` is a measured
/// absence.
fn resolve_selector(
    names: &WorldNodeNames,
    selector: &str,
    member: &str,
    refusals: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Ok(parsed) = ObjectSelector::parse(selector) else {
        refusals.push(format!(
            "{member}: the selector {selector:?} is not a node selector the production reader \
             accepts"
        ));
        return None;
    };
    match names.resolve(&parsed) {
        SelectorMatch::Node { name, occurrences } => Some(if occurrences == 0 {
            Vec::new()
        } else {
            vec![name]
        }),
        SelectorMatch::Family {
            prefix,
            occurrences,
        } => {
            if occurrences == 0 {
                Some(Vec::new())
            } else {
                Some(
                    names
                        .node_names()
                        .filter(|name| name.starts_with(&prefix))
                        .map(str::to_owned)
                        .collect(),
                )
            }
        }
        SelectorMatch::Path { path } => {
            refusals.push(format!(
                "{member}: the selector {path:?} is a node path, and a node-name table has no \
                 hierarchy, so how many records it selects is unmeasured"
            ));
            None
        }
        SelectorMatch::UnmeasuredSuffix {
            stored,
            prefix,
            suffix,
        } => {
            refusals.push(format!(
                "{member}: the selector {stored:?} carries {suffix:?} after its wildcard prefix \
                 {prefix:?}, which the corpus does not establish, so it is not counted"
            ));
            None
        }
    }
}

/// Whether `selector` selects the node named `node`.
fn selector_matches(selector: &ObjectSelector, node: &str) -> bool {
    match selector.segment() {
        crate::animation::programs::SelectorSegment::Literal(name) => name == node,
        crate::animation::programs::SelectorSegment::PrefixWildcard(prefix) => {
            node.starts_with(prefix)
        }
    }
}

/// Merges one node into the target list, or adds the coupling/marker to the
/// row that already bound it.
#[allow(clippy::too_many_arguments)]
fn merge_target(
    targets: &mut Vec<BoundTarget>,
    graph: &SceneGraph,
    node: &SceneNode,
    chain: Vec<String>,
    declared: Option<usize>,
    objective: bool,
    other_target: bool,
    labels: (Option<String>, Option<String>),
    coupling: DamageCoupling,
) {
    if let Some(existing) = targets.iter_mut().find(|target| target.node == node.name()) {
        if existing.declared.is_none() {
            existing.declared = declared;
            existing.chain = chain;
        }
        existing.objective |= objective;
        existing.other_target |= other_target;
        if existing.declared == declared && existing.description.is_none() {
            (existing.description, existing.help_label) = labels;
        }
        for member in coupling.members {
            if existing.coupling.members.contains(&member) {
                continue;
            }
            existing.coupling.members.push(member);
        }
        for field in coupling.fields {
            if !existing.coupling.fields.contains(&field) {
                existing.coupling.fields.push(field);
            }
        }
        return;
    }
    let (healthy, destroyed) = damage_layout(graph, node);
    targets.push(BoundTarget {
        declared,
        chain,
        node: node.name().to_owned(),
        index: node.index(),
        path: node.path().to_owned(),
        position_m: node.world_transform().translation(),
        target_point_m: node.world_transform().translation(),
        healthy,
        destroyed,
        coupling,
        objective,
        other_target,
        description: labels.0,
        help_label: labels.1,
        allegiance: AllegianceBinding::Open(NOT_RESOLVED.to_owned()),
    });
}

/// The node's authored `healthy`/`destroyed` children, by stored name.
fn damage_layout(graph: &SceneGraph, node: &SceneNode) -> (Option<String>, Option<String>) {
    let mut healthy = None;
    let mut destroyed = None;
    for child in node.children() {
        let Some(child) = graph.node(child) else {
            continue;
        };
        match child.name() {
            "healthy" => healthy = Some(child.name().to_owned()),
            "destroyed" => destroyed = Some(child.name().to_owned()),
            _ => {}
        }
    }
    (healthy, destroyed)
}

/// Resolves a `targets.zrd` node chain against the converted container.
///
/// A single-element chain is a node name. A longer chain is a root-to-leaf
/// **subsequence** of the node's authored name-path (`a.b.c`), the shape the
/// scene graph assigns (`world1.piratezep.….rock_zeppelin`): the last step
/// must be the node's own name and the earlier steps must appear in order
/// before it. More than one node matching is ambiguous and refused rather than
/// guessed.
fn resolve_chain<'a>(graph: &'a SceneGraph, chain: &[String]) -> Result<&'a SceneNode, String> {
    let Some((last, prefix)) = chain.split_last() else {
        return Err("the chain is empty".to_owned());
    };
    let matches: Vec<&SceneNode> = graph
        .nodes()
        .iter()
        .filter(|node| node.name() == last && path_contains_chain(node.path(), prefix, last))
        .collect();
    match matches.as_slice() {
        [] => Err(format!(
            "no record of the container carries the name {last:?} under the chain's steps"
        )),
        [only] => Ok(only),
        many => Err(format!(
            "{last:?} resolves to {} records, so which one the target names is ambiguous",
            many.len()
        )),
    }
}

/// Whether the node's authored path (`a.b.c`) carries `chain` as a
/// root-to-leaf subsequence ending at `leaf`.
fn path_contains_chain(path: &str, chain: &[String], leaf: &str) -> bool {
    let steps: Vec<&str> = path.split('.').collect();
    if steps.last() != Some(&leaf) {
        return false;
    }
    let mut cursor = 0usize;
    for step in chain {
        let Some(found) = steps[cursor..]
            .iter()
            .position(|candidate| candidate == step)
        else {
            return false;
        };
        cursor += found + 1;
    }
    true
}

/// Every node of the world group's container with its composed world position
/// in metres (`root ∘ … ∘ local`), in container order — the join
/// [`bind_mission_targets`] makes for its own targets, for a consumer that
/// binds nodes by name pattern (the turret entries).
///
/// # Errors
///
/// The installation cannot be discovered or the container cannot be read.
pub fn world_node_positions(
    install_root: &Path,
    group_dir: &str,
) -> Result<Vec<(String, [f64; 3])>, String> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| format!("the installation cannot be discovered: {error}"))?;
    let (graph, _) = read_world_scene_from(&found, install_root, group_dir)?;
    Ok(graph
        .nodes()
        .iter()
        .map(|node| (node.name().to_owned(), node.world_transform().translation()))
        .collect())
}

/// Reads the world group's `gamez.zbd` through the production readers: the raw
/// node array plus the converted scene graph, exactly the join
/// `crate::mission_world_actors::read_world_scene` makes.
fn read_world_scene_from(
    found: &Discovery,
    install_root: &Path,
    group_dir: &str,
) -> Result<(SceneGraph, GameZNodes), String> {
    let key = format!("{group_dir}/gamez.zbd");
    let record = manifest_record(found, &key)
        .ok_or_else(|| format!("no {key} in the discovered installation"))?;
    let bytes = read_container_bytes(install_root, record)?;
    let mut context = ParseContext::with_defaults(&key);
    let nodes = read_gamez_nodes(&mut context, &bytes)
        .map_err(|error| format!("the node array refuses: {error}"))?;
    let container = ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.{}", key.replace('/', ".")),
    )
    .map_err(|error| format!("the container id cannot be spelled: {error}"))?;
    let adapter = SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source");
    let bindings = BindingMap::default();
    let graph = world_scene_graph_from_gamez(&container, &nodes, &[], &adapter, &bindings)
        .map(|scene| scene.graph().clone())
        .map_err(|error| error.to_string())?;
    Ok((graph, nodes))
}

/// One manifest record by its logical key.
fn manifest_record<'a>(found: &'a Discovery, key: &str) -> Option<&'a InstallFileRecord> {
    found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
}

/// Reads one container's bytes from the installation.
fn read_container_bytes(
    install_root: &Path,
    record: &InstallFileRecord,
) -> Result<Vec<u8>, String> {
    let host = install_root.join(record.relative_spelling.as_str());
    std::fs::read(&host).map_err(|error| format!("cannot read {}: {error}", host.display()))
}

/// The damage node one bound target registers: a single lethal
/// internal-structure node whose pool is the labelled designed unit.
///
/// The authored `HEALTH` value is not used: its unit and the damage model are
/// F29's unmeasured surface, and
/// [`UnmeasuredFieldFamily::DamageCoupling`](crate::animation::programs::UnmeasuredFieldFamily::DamageCoupling)
/// names it open. The node is lethal because the authored layout is a
/// destruction state (a `healthy`/`destroyed` pair, a `*_destroy*` animation),
/// so a destroyed target leaves play rather than silently staying.
///
/// # Errors
///
/// The target's node name is not a valid [`DamageNodeKey`] spelling.
pub fn target_damage_node(target: &BoundTarget) -> Result<DamageNode, String> {
    let key = target.damage_key()?;
    let integrity = Resolved::Known(Known::new(
        DESIGNED_TARGET_INTEGRITY,
        Provenance::designed(claim(M01_TARGET_INTEGRITY_CLAIM)),
    ));
    Ok(DamageNode::new(key, DamageNodeKind::InternalStructure, integrity).with_lethal(true))
}
