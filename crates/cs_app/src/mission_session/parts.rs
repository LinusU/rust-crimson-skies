//! The live part-state objects of a mission's bound world actors
//! (M01-PLAY-TRIGGERS-PARTS, Rally #1481).
//!
//! # The defect this module repairs
//!
//! [`super::observation::mission_world_observation`] (M01-PLAY-TRIGGERS-OBSERVE,
//! #1480) writes a row for every chain the **composed world** can see: the
//! player's body, each bound world actor under its measured subject name and
//! the living count of every group the composition really spawned members for.
//! M01's 39 part-state chains — `["workersvoyagezep", "reng1", "healthy"]`,
//! `["piratezep", "reng32", "healthy"]`, … — are **not** among them: the world
//! import makes an object only for an `object3d` record the world node owns
//! directly, and an engine's `healthy` state node is a grandchild of the
//! zeppelin's own object. So those chains had no entity anywhere, the
//! `INACTIVE` ladders that count them could never complete and the win/loss
//! chain they feed could not start.
//!
//! This module gives each of those chains a **live part-state object**: one ECS
//! entity, spawned beside the bound world actor that owns it, carrying the chain
//! exactly as the record spells it and the damage identity the part is
//! addressable by. The observer then reports each one's presence, and the ladder
//! that reads it advances.
//!
//! # What the installation measures, and what it does not
//!
//! * **The chains resolve.** `MemberResolver` pins each `[actor, part,
//!   part-state]` chain to exactly one node of the C1C hierarchy: the leaf is the
//!   part's `healthy` state node, a child of the part node the chain's middle
//!   element names (`reng1`…`reng9`/`leng1`…`leng9` under `workersvoyagezep`,
//!   `reng11`…`leng32` under `piratezep`).
//! * **The part names are the record's own.** M01's `zeppelins.zrd` records
//!   declare `engines` — `workersvoyagezep`'s 18 and `piratezep`'s 12 node names
//!   — which are exactly the middle elements of the 18 and 12 chains
//!   `OBJECTIVE6/8/9/55` and `OBJECTIVE36`–`39` count. The `healthy` state node
//!   is the part's **intact** subtree (its siblings are `spin`/`counterspin`/
//!   `propstill`/`supports`); the wreck each part becomes is a separate
//!   root-level record (`wvreng_destroyed.flt`, `pzleng_destroyed.flt`, …), the
//!   same authored destruction-state shape `planes.zbd` gives an airframe's
//!   `healthy`/`destroyed` siblings.
//! * **The in-play bit is the node's own active flag.** The `INACTIVE` evaluator
//!   counts the listed nodes whose `+0x24` bit 4 is **clear**, and that bit is
//!   written by `gwNodeSetActive` (VA `0x4cca30`) — measured in #1360 — whose
//!   producer set (spawn, despawn, damage, ownership, scripts) is untraced
//!   ([`cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`]).
//! * **No per-engine integrity pool is stated anywhere.** The record states
//!   floats for its **gasbags** (`150.0` for all nine of `workersvoyagezep`'s,
//!   `120.0` for `piratezep`'s six) and, on the instant-action records,
//!   `cannon_health` rows, but nothing for an engine node. So what *destroys* an
//!   engine — the writer behind the bit — is not recoverable from data this tree
//!   reads, and this module does not invent one: it makes the parts
//!   **addressable** (one named damage node each, the seam the F27/F29 damage
//!   authority already routes a hit through) and reports the pool it applies.
//!
//! # The designed value, and why there is one
//!
//! A damage node whose pool is `Resolved::Unknown` **blocks** every hit
//! (`DamageResolver::resolve`, the `HitBlocked` arm), so a mission whose parts
//! have no measured pool cannot show a part leaving play at all. This slice
//! gives each part a labelled **designed** single-unit pool
//! ([`DESIGNED_PART_INTEGRITY`], claim [`M01_PART_STATE_CLAIM`]) so the
//! destruction path (#1437's landed weapon session, #1436's AI fire, and the
//! damage authority they both publish into) can actually take a part out of
//! play — exactly the shape M01-PLAY-WEAPONS (#1437) used for its designed
//! loadout. It is **not** a claim about the original's engine hit points, which
//! stay unmeasured and are named as a residue in
//! `docs/findings/2026-10-15-m01-play-triggers-part-state-presence.md`; F27-E's
//! measured damage table replaces it.
//!
//! # What a turret node is (M01-PLAY-TRIGGERS-TURRET-CLASS, Rally #1491)
//!
//! Nine of the 39 chains — `utur1`…`utur4`, `ctur1`…`ctur4`, `maingun9` under
//! `workersvoyagezep` — name nodes no `zeppelins.zrd` key declares. They are
//! declared by the **other** record table the original loads at mission start:
//! the shared `ai.zrd`'s `TURRET` list, whose `NODES` entries are `[mount, part]`
//! patterns (`*` is one digit — [`pattern_matches`]). `workersvoyagezep` is
//! named by record 27 (`maingun*`, `MSG_TUR_MAIN`), record 28 (`ctur*`,
//! `MSG_TUR_BELLY`) and record 31 (`utur*`, `MSG_TUR_TOP`), each with its own
//! `WEAPON` (name, ammunition, fire rate, detection range), `PARTS`
//! (`turret`/`gun`/`firepoint`), pitch and yaw limits — a **weapon mount**, so
//! [`turret_part_node`] gives the node [`DamageNodeKind::WeaponMount`] **and**
//! the F29 system a weapon mount's destruction disables,
//! [`SystemKind::Weapon`] (AC03). Records 28 and 31 also state `HEALTH`
//! (`10.0`); record 27 states none.
//!
//! The `disables` edge is read **per node**: the F29 consumer
//! (`cs_app::damage::apply_damage_state`) disables the mount the destroyed
//! node's edge names, by its own [`DamageNodeKey`], so a destroyed turret stops
//! firing exactly its own mount and never the zeppelin's other eight. The
//! actor-level system-state query is a report and gates nothing.
//!
//! What stays open: whether the **original** counts a turret's `healthy` state
//! the same way as an engine's, and whether it clears the `+0x24` bit 4 the
//! `INACTIVE` read counts (its producers are untraced); and the unit of
//! `HEALTH`. The classification itself **is** consumed (M01-PLAY-TRIGGERS-
//! TURRET-WIRE, #1499): the stage reads the scope's table once
//! ([`read_scope_turrets`]) and the spawn host builds every part node through
//! [`classed_part_node`]. See
//! `docs/findings/2026-10-16-m01-play-triggers-turret-class.md`.
//!
//! # What a fired round sweeps (M01-PLAY-PARTS-GEOMETRY, Rally #1492)
//!
//! Making a part *addressable* is not the same as letting a gun hit it. The
//! F27 gun step sweeps [`PartSweptBox`] entities through
//! [`part_sweep_candidates`], and until this slice M01's parts carried no box,
//! no [`NodeVisualTransform`] and no velocity — so nothing in the composed run
//! could destroy a part *geometrically*, only the damage authority could.
//! [`spawn_part_state`] now gives every part the whole geometric face: a box at
//! the owner's measured bind pose with the designed extents
//! [`DESIGNED_PART_HALF_EXTENTS_M`], the pose itself, and the zero motion a
//! `Held` world actor has.
//!
//! The box is **designed and labelled**: a part node's `object3d` record is one
//! the world import makes no object for, and no reader in this tree decodes its
//! authored transform or stored box, so the value a measurement would replace
//! it with is carried as an explicit unknown beside it
//! ([`MissionPartSweep`], [`M01_PART_GEOMETRY_UNKNOWN_CLAIM`]).
//!
//! # What is observed, and the fail-closed rule
//!
//! A part entity's presence is read from the session's damage authority, never
//! defaulted:
//!
//! | Part state | Row |
//! | --- | --- |
//! | `Intact` / `Damaged` | the chain is **in play**, at the owning actor's pose |
//! | `Destroyed` | the chain is **out of play** — the `INACTIVE` read |
//! | `Unknown` pool, or no registered actor | **no row at all** |
//!
//! The last line is the fail-closed half: an unmeasured pool is not a statement
//! that the part is in play, so the chain stays unobserved and the ladder that
//! counts it stays where it is.

use std::collections::BTreeMap;
use std::path::Path;

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{Component, Entity, GlobalTransform, Resource, Transform, Vec3, World};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::script_raw::discovery::discover_container;
use cs_script::ir::{ActorId as WorldActorId, MemberName};
use cs_sim::damage::{
    DamageNode, DamageNodeKey, DamageNodeKind, DamageResolver, PartState, SystemKind,
};
use cs_types::content::{ContentId, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::ActorId;

use crate::scene::{NodeVisualTransform, SceneGeneration};
use crate::weapons::PartSweptBox;
use crate::world_facts::{MemberResolver, WorldOperands};

use super::compose::MissionStage;
use super::observation::subject_node_name;

/// The claim every value this slice designed is filed under.
pub const M01_PART_STATE_CLAIM: &str = "m01-play-triggers-parts.designed-part-integrity";

/// The claim the designed sweep-box geometry of a part is filed under
/// (M01-PLAY-PARTS-GEOMETRY).
pub const M01_PART_GEOMETRY_CLAIM: &str = "m01-play-parts-geometry.designed-part-box";

/// The claim the unmeasured container geometry of a part is filed under.
pub const M01_PART_GEOMETRY_UNKNOWN_CLAIM: &str = "m01-play-parts-geometry.node-box-unmeasured";

/// The designed half extents of one part's swept box, in metres.
///
/// Not a measurement. A part node — `reng1`, `utur1`, … — is an `object3d`
/// record of the container hierarchy the world import makes no object for (it
/// objectifies only a record the **world node owns directly**), and no reader
/// in this tree decodes a per-node authored transform or stored box off such a
/// record: the resolver reports existence only
/// ([`crate::world_facts::MemberResolver`]), so the part's box centre and
/// extents have no measured source. This is therefore a labelled designed box
/// under [`M01_PART_GEOMETRY_CLAIM`], the same shape
/// [`DESIGNED_PART_INTEGRITY`] is, and the unmeasured source it stands in for
/// is carried beside it in [`MissionPartSweep::unmeasured`]. It is one value
/// for every part, so it is explicitly not a per-part tuning table; the
/// container record decode (F18/F35) or a measured original hit replaces it.
pub const DESIGNED_PART_HALF_EXTENTS_M: [f64; 3] = [4.0, 4.0, 4.0];

/// The designed integrity pool of one named part: **one damage unit**.
///
/// Not a measurement and not a table: the original states no engine pool
/// (finding `2026-10-15-m01-play-triggers-part-state-presence.md`), and a
/// single unit is the least a pool can be while still letting *any* damage
/// destroy the part — which is what the destruction path has to be able to do
/// before the `INACTIVE` ladders can advance. Every part gets the same value,
/// so this is explicitly not a per-part tuning table.
pub const DESIGNED_PART_INTEGRITY: f64 = 1.0;

/// Component: one live part-state object of a bound world actor.
///
/// The entity *is* the engine's part-state node in this composition: the chain
/// it was spawned for, the actor that owns it and the damage node a hit
/// addresses it by. Its pose is the owning actor's own bind-time pose — the
/// per-node authored transform of a container record the world import does not
/// make an object for is not decoded, so no offset is invented.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MissionPartState {
    /// The member chain, exactly as the record spells it.
    pub chain: MemberName,
    /// The part's damage node on the owning actor's graph — the chain's part
    /// element, which is the node name the actor's own record declares under
    /// `engines`.
    pub node: DamageNodeKey,
    /// The bound world actor's measured subject.
    pub subject: ContentId,
    /// The session damage actor the part's node lives on.
    pub actor: ActorId,
    /// The scene generation the entity spawned under.
    pub generation: SceneGeneration,
}

/// Component: where a part's swept-box geometry came from
/// (M01-PLAY-PARTS-GEOMETRY).
///
/// The box the sweep actually reads lives on [`PartSweptBox`]; this record
/// says **where it came from**, so a consumer can tell the designed value the
/// mission runs under from a measured one. The container's own per-node box is
/// the value that would replace it, and it is an explicit
/// [`Resolved::Unknown`] here — never silently absent.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MissionPartSweep {
    /// The half extents the sweep uses, in metres — the value of
    /// [`PartSweptBox::half_extents_m`].
    pub half_extents_m: [f64; 3],
    /// Where `half_extents_m` came from: [`M01_PART_GEOMETRY_CLAIM`],
    /// `designed`.
    pub provenance: Provenance,
    /// Why the container's own per-node box has no measured source, under
    /// [`M01_PART_GEOMETRY_UNKNOWN_CLAIM`].
    pub unmeasured: Resolved<[f64; 3]>,
}

/// One part-state chain that was given a live entity.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundPart {
    /// The chain, as spelled.
    pub chain: MemberName,
    /// The part's damage node.
    pub node: DamageNodeKey,
    /// The world actor that owns it.
    pub actor: ActorId,
    /// The spawned entity.
    pub entity: Entity,
}

/// Resource: what the part-state binding did — the parts it bound, the damage
/// nodes they added to their actors' graphs and every refusal by name.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionPartReport {
    /// The parts that were bound, in (actor, chain) order.
    pub parts: Vec<BoundPart>,
    /// The part damage nodes this binding added, per session damage actor, in
    /// stable order. This is exactly what the owning actor's graph carries
    /// beyond the designed airframe node.
    pub nodes: BTreeMap<ActorId, Vec<DamageNodeKey>>,
    /// Every chain that could not be bound, with why, verbatim.
    pub refusals: Vec<String>,
}

impl MissionPartReport {
    /// The chain of every bound part of one actor, in stable order.
    #[must_use]
    pub fn chains_of(&self, actor: ActorId) -> Vec<&MemberName> {
        self.parts
            .iter()
            .filter(|part| part.actor == actor)
            .map(|part| &part.chain)
            .collect()
    }

    /// Whether nothing was bound at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// The part a member chain names, as a damage node key — `None` for a chain
/// that is not a part-state chain.
///
/// A chain is `[actor]` or `[actor, part, …]` (the measured one-, two- and
/// three-element arities); only the second and later elements name a part, so a
/// one-element chain — M01's bare `["piratezep"]`, its `TRAVELERS` subject and
/// anchor — names the actor itself and is never a part.
#[must_use]
pub fn part_state_node(chain: &MemberName) -> Option<DamageNodeKey> {
    let part = chain.get(1)?;
    DamageNodeKey::new(part).ok()
}

/// The designed damage node of a part no `TURRET` record declares.
///
/// The class is [`DamageNodeKind::InternalStructure`]: the record's own
/// `engines` key declares M01's 18 `workersvoyagezep` engine nodes, and a
/// class read from a name alone would be a guess for anything else. No
/// `disables` edge is declared either: the zeppelin's motion is `Held` and the
/// original's part→system rule is unmeasured, so binding a propulsion system
/// here would be an invented rule. A part the `ai.zrd` `TURRET` table **does**
/// declare never reaches this node — [`classed_part_node`] chooses
/// [`turret_part_node`] for it.
#[must_use]
pub fn designed_part_node(key: &DamageNodeKey) -> DamageNode {
    DamageNode::new(
        key.clone(),
        DamageNodeKind::InternalStructure,
        Resolved::Known(Known::new(
            DESIGNED_PART_INTEGRITY,
            Provenance::designed(claim()),
        )),
    )
}

/// One `ai.zrd` `TURRET` record, as far as a part node needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretRecord {
    /// The record's position in the member's `TURRET` list — its turret type.
    pub index: usize,
    /// Every `NODES` entry, each an ancestor-to-descendant pattern chain.
    pub nodes: Vec<Vec<String>>,
    /// The `TITLE` message name.
    pub title: Option<String>,
    /// The `WEAPON`'s `NAME`.
    pub weapon: Option<String>,
    /// The record's `PARTS` node names.
    pub parts: Vec<String>,
    /// The record's `HEALTH`, in the unit the record states (unmeasured).
    pub health: Option<f64>,
}

/// The `TURRET` list of a decoded `ai.zrd`, in record order.
///
/// A record without a `NODES` key is skipped — the original asserts "Turret
/// entry #%d specifies no NODES" and skips it too — and so is a malformed
/// shape; neither is guessed into a record.
#[must_use]
pub fn read_turret_records(ai_zrd: &ZrdValue) -> Vec<TurretRecord> {
    let Some(entries) = ai_zrd.as_list() else {
        return Vec::new();
    };
    let list = entries.iter().find_map(|entry| {
        let kids = entry.as_list()?;
        (kids.first()?.as_text() == Some("TURRET")).then(|| kids.get(1)?.as_list())?
    });
    let mut records = Vec::new();
    for (index, record) in list.unwrap_or_default().iter().enumerate() {
        let fields = cs_content::stunts::zrd_flat_fields(record);
        let field = |name: &str| fields.iter().find(|(key, _)| *key == name).map(|(_, v)| *v);
        let Some(nodes) = field("NODES").and_then(ZrdValue::as_list) else {
            continue;
        };
        let nodes: Vec<Vec<String>> = nodes
            .iter()
            .filter_map(|element| match element {
                ZrdValue::Text(text) => Some(vec![text.clone()]),
                ZrdValue::List(chain) => chain
                    .iter()
                    .map(|part| part.as_text().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .filter(|chain| !chain.is_empty()),
                _ => None,
            })
            .collect();
        let first_text =
            |value: Option<&ZrdValue>| value?.as_list()?.first()?.as_text().map(str::to_owned);
        let weapon = field("WEAPON").and_then(|weapon| {
            let inner = zrd_pairs(weapon.as_list()?);
            first_text(inner.iter().find(|(k, _)| *k == "NAME").map(|(_, v)| *v))
        });
        let parts = field("PARTS")
            .and_then(ZrdValue::as_list)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.as_text().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let health = field("HEALTH")
            .and_then(ZrdValue::as_list)
            .and_then(|values| match values.first()? {
                ZrdValue::Float(value) => Some(f64::from(*value)),
                _ => None,
            });
        records.push(TurretRecord {
            index,
            nodes,
            title: first_text(field("TITLE")),
            weapon,
            parts,
            health,
        });
    }
    records
}

/// The scope's `ai.zrd` `TURRET` table, as the composition carries it from
/// stage build to the spawn host (M01-PLAY-TRIGGERS-TURRET-WIRE, #1499).
///
/// The original decodes `ai.zrd` at mission start (`0x465596`, before
/// `zeppelins.zrd` at `0x4655c0`) and builds the `0x71d910` binding table its
/// turret and allegiance paths consult, taking the member from the **first**
/// scope archive that holds it in the mission → group → shared mount order
/// ([`read_scope_turrets`]; the same order `crate::mission_world_actors`
/// applies for allegiance, #1155).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurretTable {
    /// The `TURRET` list in record order — [`read_turret_records`]'s own
    /// records, empty when no carrier holds the member.
    pub records: Vec<TurretRecord>,
    /// The logical key of the container the member decoded from
    /// (`zbd/zrdr.zbd`), or `None` when no searched carrier held it.
    pub carrier: Option<String>,
    /// Why no table could be read, verbatim: a carrier that could hold the
    /// member could not be checked, or the member refused to decode. A
    /// refusal is carried instead of being folded into `records` so that no
    /// reader can mistake an **unread** table for a measured absence — the
    /// spawn host reports it beside the parts it then leaves unclassed.
    pub refusal: Option<String>,
}

/// Reads the scope's `ai.zrd` in the original's mount order — mission, group,
/// shared — and answers its `TURRET` list, or the reason none can be claimed.
///
/// The carrier search mirrors `crate::mission_world_actors`' allegiance
/// reader: the first archive that lists a member named `ai.zrd` wins, an
/// archive missing from the manifest or unreadable on disk is kept as a
/// reason for the next one, and a member that refuses to decode is a refusal
/// rather than an empty table. When no carrier holds the member the answer is
/// a **measured absence**: the original's loader finds nothing either and the
/// table it feeds stays empty, so every part keeps its unclassed node.
///
/// The manifest lookup is what makes this a production read: an installation
/// spells its containers with their own case (`ZBD/C1C/ZRDR.ZBD`), so the
/// host path comes from the discovered record and never from the logical key
/// the search matched.
#[must_use]
pub fn read_scope_turrets(install_root: &Path, mission_dir: &str, group_dir: &str) -> TurretTable {
    let archives = [
        format!("{mission_dir}/zrdr.zbd"),
        format!("{group_dir}/zrdr.zbd"),
        "zbd/zrdr.zbd".to_owned(),
    ];
    let refused = |reason: String| TurretTable {
        refusal: Some(reason),
        ..TurretTable::default()
    };
    let found = match cs_assets::install::discover(install_root) {
        Ok(found) => found,
        Err(error) => return refused(format!("the installation cannot be discovered: {error}")),
    };
    let mut unreadable: Option<String> = None;
    for archive in &archives {
        let Some(record) = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == *archive)
        else {
            unreadable = unreadable.or(Some(format!("{archive} is not in the manifest")));
            continue;
        };
        let host = install_root.join(record.relative_spelling.as_str());
        let bytes = match std::fs::read(&host) {
            Ok(bytes) => bytes,
            Err(error) => {
                unreadable =
                    unreadable.or(Some(format!("cannot read {}: {error}", host.display())));
                continue;
            }
        };
        let discovery = discover_container(archive, &record.relative_spelling, &bytes);
        let Some(program) = discovery.programs().iter().find(|program| {
            program.locator().member().is_some_and(|name| {
                name.eq_ignore_ascii_case(crate::mission_world_actors::TURRET_MEMBER)
            })
        }) else {
            continue;
        };
        let decoded = match decode_zrd(program.bytes()) {
            Ok(decoded) => decoded,
            Err(error) => {
                return refused(format!(
                    "{archive}'s {} refuses: {error}",
                    crate::mission_world_actors::TURRET_MEMBER
                ));
            }
        };
        return TurretTable {
            records: read_turret_records(&decoded),
            carrier: Some(archive.clone()),
            refusal: None,
        };
    }
    // A carrier that could not be checked may hold the member, so the read is
    // a refusal; only when every carrier was checked and none held it is the
    // absence itself measured.
    match unreadable {
        Some(reason) => refused(reason),
        None => TurretTable::default(),
    }
}

/// Key/value pairs of a flat `[key, value, key, value, …]` child list.
fn zrd_pairs(children: &[ZrdValue]) -> Vec<(&str, &ZrdValue)> {
    children
        .chunks(2)
        .filter_map(|pair| Some((pair.first()?.as_text()?, pair.get(1)?)))
        .collect()
}

/// `0x4abea0`'s pattern rule (measured in #1155): `*` matches exactly one digit,
/// every other byte is literal and the pattern consumes the whole name.
#[must_use]
pub fn pattern_matches(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.as_bytes(), name.as_bytes());
    pattern.len() == name.len()
        && pattern.iter().zip(name).all(|(&p, &n)| {
            if p == b'*' {
                n.is_ascii_digit()
            } else {
                p == n
            }
        })
}

/// The `TURRET` record that declares `part` of the actor named `actor`: the
/// earliest record with a two-element `NODES` entry `[mount, part]` whose two
/// patterns match (the original's earliest-record-wins binding, `0x4a9890`).
///
/// Longer or one-element entries (a nested part chain, a bare mount) are not
/// part-of-actor declarations and never match here.
#[must_use]
pub fn turret_record_for<'a>(
    records: &'a [TurretRecord],
    actor: &str,
    part: &str,
) -> Option<&'a TurretRecord> {
    records.iter().find(|record| {
        record.nodes.iter().any(|chain| {
            matches!(chain.as_slice(), [mount, node]
                if pattern_matches(mount, actor) && pattern_matches(node, part))
        })
    })
}

/// The damage node of a part a `TURRET` record declares: a
/// [`DamageNodeKind::WeaponMount`] with the same designed pool as every part
/// and the F29 system its destruction disables, [`SystemKind::Weapon`] (AC03).
#[must_use]
pub fn turret_part_node(key: &DamageNodeKey) -> DamageNode {
    DamageNode::new(
        key.clone(),
        DamageNodeKind::WeaponMount,
        Resolved::Known(Known::new(
            DESIGNED_PART_INTEGRITY,
            Provenance::designed(claim()),
        )),
    )
    .with_disables(SystemKind::Weapon)
}

/// The designed node of a part of `actor`: a weapon mount when the `TURRET`
/// table declares it, otherwise [`designed_part_node`]'s unclassed one.
#[must_use]
pub fn classed_part_node(records: &[TurretRecord], actor: &str, key: &DamageNodeKey) -> DamageNode {
    match turret_record_for(records, actor, key.as_str()) {
        Some(_) => turret_part_node(key),
        None => designed_part_node(key),
    }
}

/// The claim id [`M01_PART_STATE_CLAIM`] parsed.
fn claim() -> ClaimId {
    ClaimId::new(M01_PART_STATE_CLAIM).expect("a claim id this module declares")
}

/// The claim id [`M01_PART_GEOMETRY_CLAIM`] parsed.
fn geometry_claim() -> ClaimId {
    ClaimId::new(M01_PART_GEOMETRY_CLAIM).expect("a claim id this module declares")
}

/// The designed swept-box geometry of one part, with the unmeasured container
/// box it stands in for.
///
/// `node` is the part's damage node, so the recorded unknown names the exact
/// record whose authored box was not decoded rather than a bare "unmeasured".
#[must_use]
pub fn part_sweep_geometry(node: &DamageNodeKey) -> MissionPartSweep {
    let reason = format!(
        "node {node} is an `object3d` record of the container hierarchy the world import makes \
         no object for (it objectifies only a record the world node owns directly) and no reader \
         in this tree decodes that record's authored transform or stored box, so the part's box \
         centre and extents have no measured source; the designed half extents \
         {DESIGNED_PART_HALF_EXTENTS_M:?} stand in for it until the container node decode \
         (F18/F35) or a measured original hit replaces them"
    );
    MissionPartSweep {
        half_extents_m: DESIGNED_PART_HALF_EXTENTS_M,
        provenance: Provenance::designed(geometry_claim()),
        unmeasured: Resolved::<[f64; 3]>::unknown(
            ClaimId::new(M01_PART_GEOMETRY_UNKNOWN_CLAIM).expect("a claim id this module declares"),
            &reason,
        )
        .expect("the reason is non-empty"),
    }
}

/// The part-state chains of one world actor, in (chain, node) order.
///
/// A chain qualifies when it resolves against the container hierarchy, has a
/// part element and names `subject_name` — the node name the bound actor's
/// measured subject spells ([`subject_node_name`]) — as its actor.
#[must_use]
pub fn part_state_chains(
    resolver: &MemberResolver,
    operands: &WorldOperands,
    subject_name: &str,
) -> Vec<(MemberName, DamageNodeKey)> {
    let mut planned = Vec::new();
    for chain in &operands.members {
        if chain.first().map(String::as_str) != Some(subject_name) {
            continue;
        }
        let Some(node) = part_state_node(chain) else {
            continue;
        };
        if resolver.resolve(chain).is_none() {
            continue;
        }
        planned.push((chain.clone(), node));
    }
    planned
}

/// Every part plan of a stage, keyed by the **world actor id** the program gave
/// the actor: what each bound actor's parts are, and which damage nodes its
/// graph must carry.
///
/// The plan is read from the stage's own records — the mission program's world
/// operands through the container resolver over the very container the stage
/// imported — so the composition binds exactly the chains the record spells and
/// nothing else.
#[must_use]
pub fn plan_part_states(
    stage: &MissionStage,
) -> BTreeMap<WorldActorId, Vec<(MemberName, DamageNodeKey)>> {
    let operands = WorldOperands::of(&stage.host.control);
    let mut plan = BTreeMap::new();
    let Some(actors) = stage.host.world_actors.as_ref() else {
        return plan;
    };
    for (actor, subject) in &actors.subjects {
        let Some(name) = subject_node_name(subject) else {
            continue;
        };
        let chains = part_state_chains(&stage.host.resolver, &operands, name);
        if !chains.is_empty() {
            plan.insert(*actor, chains);
        }
    }
    plan
}

/// Reports what the session's damage authority says about one part, or `None`
/// when it says nothing this observer may report.
///
/// `Some(true)` is in play, `Some(false)` is out of play, and `None` is the
/// fail-closed read: an unregistered actor or an unresolved pool is not a
/// statement about presence.
#[must_use]
pub fn part_in_play(resolver: &DamageResolver, part: &MissionPartState) -> Option<bool> {
    match resolver.part_state(&part.actor, &part.node) {
        Some(PartState::Destroyed) => Some(false),
        Some(PartState::Intact | PartState::Damaged) => Some(true),
        Some(PartState::Unknown) | None => None,
    }
}

/// Spawns the live entity of one part-state chain beside its bound world actor.
///
/// The entity is the part's whole runtime face (M01-PLAY-PARTS-GEOMETRY): the
/// chain and its damage identity ([`MissionPartState`]), and the **swept-box
/// geometry** a fired round sweeps the part by
/// ([`PartSweptBox`] + [`NodeVisualTransform`] + [`LinearVelocity`]).
///
/// * the **centre** is `pose` — the owning actor's session pose at bind time,
///   the one measured placement the composition has for the part (the
///   per-node authored transform has no decoded source; see
///   [`part_sweep_geometry`]);
/// * the **box** is [`DESIGNED_PART_HALF_EXTENTS_M`], recorded with the
///   unmeasured container box it stands in for;
/// * the **motion** is zero: M01's zeppelins declare [`DeclaredMotion::Held`]
///   (no route, no keyframes, no velocity), so the actor's own velocity is
///   zero and `part_sweep_candidates` reconstructs the previous centre from
///   it exactly as it does for any part.
///
/// [`DeclaredMotion::Held`]: cs_content::world_actors::DeclaredMotion::Held
#[must_use]
pub fn spawn_part_state(
    world: &mut World,
    actor: ActorId,
    subject: ContentId,
    generation: SceneGeneration,
    chain: MemberName,
    node: DamageNodeKey,
    pose: [f64; 3],
) -> Entity {
    let translation = Vec3::new(pose[0] as f32, pose[1] as f32, pose[2] as f32);
    let transform = Transform::from_translation(translation);
    let sweep = part_sweep_geometry(&node);
    world
        .spawn((
            transform,
            NodeVisualTransform(GlobalTransform::from_translation(translation)),
            LinearVelocity(Vec3::ZERO),
            PartSweptBox::new(actor, node.clone(), sweep.half_extents_m),
            MissionPartState {
                chain,
                node,
                subject,
                actor,
                generation,
            },
            sweep,
        ))
        .id()
}

/// Every part entity the world holds, in spawn order, for teardown and for the
/// observer.
#[must_use]
pub fn part_entities(world: &mut World) -> Vec<Entity> {
    use bevy::prelude::With;
    let mut query = world.query_filtered::<Entity, With<MissionPartState>>();
    query.iter(world).collect()
}

/// The two outcomes a scope read can have when the installation is not retail
/// data: a carrier nobody could open is a **refusal**, carriers that were
/// opened and hold no member are a **measured absence**. The distinction is
/// what keeps an unread table from passing for "the original declares no
/// turret here" (M01-PLAY-TRIGGERS-TURRET-WIRE, #1499).
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// No root, so no carrier can even be looked up: the read answers why, and
    /// never an empty table that a consumer would read as a measured absence.
    #[test]
    fn accept_m01_play_triggers_turret_wire_an_uncheckable_installation_is_a_refusal() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-installation");
        assert!(!root.exists(), "the root really is not an installation");

        let table = read_scope_turrets(&root, "zbd/x/m01", "zbd/x");

        assert!(table.records.is_empty());
        assert_eq!(table.carrier, None);
        let reason = table
            .refusal
            .expect("a root no carrier can be checked under is a refusal, not an absence");
        assert!(reason.contains("cannot be discovered"), "{reason}");
    }

    /// Every carrier of the mount order exists and none holds an `ai.zrd`:
    /// the search itself completed, so the absence is the measurement.
    #[test]
    fn accept_m01_play_triggers_turret_wire_carriers_without_the_member_are_a_measured_absence() {
        let root =
            std::env::temp_dir().join(format!("cs_turret_wire_absence_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for archive in ["zbd/x/m01/zrdr.zbd", "zbd/x/zrdr.zbd", "zbd/zrdr.zbd"] {
            let path = root.join(archive);
            fs::create_dir_all(path.parent().expect("the carrier has a directory"))
                .expect("the carrier directory is created");
            fs::write(&path, b"not a container this tree ever shipped")
                .expect("the carrier is written");
        }

        let table = read_scope_turrets(&root, "zbd/x/m01", "zbd/x");
        let _ = fs::remove_dir_all(&root);

        assert!(table.records.is_empty(), "no carrier held the member");
        assert_eq!(table.carrier, None);
        assert_eq!(
            table.refusal, None,
            "every carrier was opened and checked: this is an absence, not a refusal"
        );
    }
}
