//! The whole HUD for one frame (F46-B): instruments plus the gauge clusters
//! and the target display, derived from the session's own authorities.
//!
//! [`Hud::frame`] is the production path a render tick calls once per bound
//! aircraft. It reads **no** gameplay state of its own — every row below is
//! borrowed out of an authority the session already owns, so the HUD can
//! never disagree with them:
//!
//! * the F27-C [`WeaponSession`] supplies the gun gauge: the registered
//!   arsenal's mounts with their live rounds, the selected bank and the
//!   damage-disabled mounts;
//! * the F28-C [`OrdnanceSession`] supplies the launcher cluster: the
//!   registered components a hardpoint can still launch (the session audit's
//!   own rows);
//! * the F29 [`DamageResolver`] supplies the airframe panel: every damage
//!   node of the actor's graph with its live [`PartState`] — `Unknown` is
//!   reported, never guessed — and the declared scene binding the presenter
//!   maps an indicator by;
//! * the F30-C [`TargetConsumers`] resource supplies the target display:
//!   the published [`HudTargetReadout`], shown only while it is bound to
//!   this HUD's `(session, observer)`.
//!
//! Stale-state discipline (F46 non-negotiable 5): an authority stamped for a
//! different session generation is refused as
//! [`HudError::ForeignAuthority`], never read as an empty gauge. An
//! authority that is not installed, a session whose actor is not registered
//! and consumer views bound to a different observer produce no rows — the
//! frame field is `None`, so the presenter draws nothing and nothing is
//! invented. `docs/findings/2026-10-07-f46-b-hud-gauges-and-target-display.md`
//! records the decisions and the unknowns.

use std::collections::BTreeSet;

use cs_sim::damage::{DamageNodeKey, DamageNodeKind, DamageResolver, PartState};
use cs_sim::weapons::{AmmunitionId, GunMountKind, OrdnanceFamily, OrdnanceId};
use cs_types::content::{ContentId, Resolved};
use cs_types::net::{ActorId, SessionId};

use super::{AircraftSample, Hud, HudError, Instruments};
use crate::ordnance::{OrdnanceSession, session_ordnance_audit};
use crate::targeting::{HudTargetReadout, TargetConsumers};
use crate::weapons::WeaponSession;

/// The session authorities one HUD frame is projected from.
///
/// Each `Option` declares whether the authority exists in the calling
/// session at all: `None` produces no rows for that cluster, while a source
/// stamped for a different session generation is refused inside
/// [`Hud::frame`] — an absent source and a stale source are different
/// statements, and only one of them is a display state.
#[derive(Clone, Copy, Default)]
pub struct HudSources<'a> {
    /// The F27-C weapon session the bound actor's guns live in.
    pub weapons: Option<&'a WeaponSession>,
    /// The F28-C ordnance session the bound actor's launchers live in.
    pub ordnance: Option<&'a OrdnanceSession>,
    /// The F29 damage resolver the bound actor's airframe graph lives in.
    pub damage: Option<&'a DamageResolver>,
    /// The F30-C consumer views as `apply_target_consumers` last published
    /// them.
    pub targets: Option<&'a TargetConsumers>,
}

/// One mounted gun as the gauge shows it.
///
/// This is the weapon authority's own state: the row exists because the gun
/// is registered, and `selected`, `rounds` and `disabled` are read, never
/// inferred — a dry mount is a gauge row with `rounds: 0`, a destroyed mount
/// a row with `disabled: true`, and neither changes the bank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountedGun {
    /// The damage node the gun is mounted on.
    pub mount: DamageNodeKey,
    /// The declared mount kind (nose, wing, tail, gondola).
    pub kind: GunMountKind,
    /// The ammunition type this mount loads.
    pub ammunition: AmmunitionId,
    /// Rounds remaining on this mount.
    pub rounds: u64,
    /// Whether the mount belongs to the selected bank.
    pub selected: bool,
    /// Whether a destroyed weapon-mount node has disabled this mount.
    pub disabled: bool,
}

/// The weapon gauge cluster: the `gungauge` indicators and the ammunition
/// fields.
///
/// `empty` is the gauge state AC02 names — a bank is selected and every
/// selected mount is out of rounds. It is a **display** state: it never
/// selects, unselects or refills anything, the same rule
/// [`cs_sim::weapons::WeaponState::select`] itself keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeaponGauge {
    /// One row per mounted gun, in stable mount order.
    pub guns: Vec<MountedGun>,
    /// Rounds remaining across the selected bank's mounts.
    pub selected_rounds: u64,
    /// The ammunition types the selected mounts load, deduplicated — usually
    /// one, but a mixed bank reports every type rather than inventing a
    /// winner for the type field.
    pub ammunition: Vec<AmmunitionId>,
    /// Whether a bank is selected at all.
    pub has_selection: bool,
    /// A bank is selected and every selected mount has no rounds left.
    pub empty: bool,
}

/// One airframe part as the damage panel shows it.
///
/// `state` is the resolver's own [`PartState`] — a part whose integrity is
/// unresolved reports `Unknown`, never a guessed intact or destroyed —
/// and `scene_binding` is the node's declared presentation binding, carried
/// verbatim so the presenter can match the measured indicator names
/// (`rightwingdamage` and siblings) without the HUD owning a zone table.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageZone {
    /// The damage node the row describes.
    pub node: DamageNodeKey,
    /// The part kind.
    pub kind: DamageNodeKind,
    /// The live observable state.
    pub state: PartState,
    /// The scene node the graph binds this part to, as declared; `Unknown`
    /// stays unresolved.
    pub scene_binding: Option<Resolved<ContentId>>,
}

/// One launchable hardpoint as the ordnance gauge shows it.
///
/// No authoritative shots-remaining count exists: launching a component
/// never decrements its declared stack budget, so the row reports the
/// component mounted on the hardpoint and nothing more. The count a
/// `mgindicator` row would need is an authority gap, recorded in the F46-B
/// findings — it is not invented here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountedOrdnance {
    /// The registered component on the hardpoint.
    pub ordnance: OrdnanceId,
    /// The damage-node mount it launches from.
    pub mount: DamageNodeKey,
    /// The behavior family.
    pub family: OrdnanceFamily,
}

/// The complete HUD view for one frame.
///
/// Every `Option` is an **absent** authority or record: `None` means the
/// presenter draws nothing for that cluster, which is different from a
/// cluster that is present and empty. A frame can never describe another
/// aircraft: [`Hud::frame`] derives it for the bound `(session, actor)`
/// only and refuses foreign-generation sources outright.
#[derive(Clone, Debug, PartialEq)]
pub struct HudFrame {
    /// The flight-instrument projection.
    pub instruments: Instruments,
    /// The gun gauge, `None` when no weapon session was supplied or the bound
    /// actor is not registered in it.
    pub weapons: Option<WeaponGauge>,
    /// The airframe damage panel, `None` when no damage resolver was
    /// supplied or the bound actor is not registered in it.
    pub airframe: Option<Vec<DamageZone>>,
    /// The launcher rows, `None` when no ordnance session was supplied, the
    /// session is closed or the actor has no launchable components.
    pub ordnance: Option<Vec<MountedOrdnance>>,
    /// The target display, `None` when no consumer views were supplied or
    /// they are bound to another `(session, observer)` — the previous
    /// aircraft's target is never drawn.
    pub target: Option<HudTargetReadout>,
}

impl Hud {
    /// Projects the whole frame for `sample` from `sources`.
    ///
    /// The kinematic half is [`Hud::project`]'s own bound check; every
    /// supplied authority is then read for the *bound* actor only. A
    /// source stamped for another session generation is refused with
    /// [`HudError::ForeignAuthority`] — reading it as an empty gauge would
    /// show the previous generation's aircraft, which is exactly the stale
    /// state the binding exists to prevent.
    ///
    /// The weapon gauge is derived from the live `WeaponState`: a bank that
    /// fired its last round reads `empty` on this frame while its mounts
    /// stay `selected` — the projection observes the selection, it never
    /// owns it.
    ///
    /// # Errors
    ///
    /// Every error [`Hud::project`] can return, plus
    /// [`HudError::ForeignAuthority`]; on an error nothing is projected.
    pub fn frame(
        &mut self,
        sample: &AircraftSample,
        sources: &HudSources<'_>,
    ) -> Result<HudFrame, HudError> {
        let instruments = self.project(sample)?;
        let (_, actor) = self.bound().expect("project proved the binding");
        Ok(HudFrame {
            instruments,
            weapons: weapon_gauge(sources.weapons, sample.session, actor)?,
            airframe: airframe_zones(sources.damage, sample.session, actor)?,
            ordnance: ordnance_rows(sources.ordnance, sample.session, actor)?,
            target: target_view(sources.targets, actor).cloned(),
        })
    }
}

/// The weapon gauge's gather: the actor's registered guns with their live
/// state, or `None` when the actor has no arsenal in this session.
fn weapon_gauge(
    weapons: Option<&WeaponSession>,
    bound: SessionId,
    actor: ActorId,
) -> Result<Option<WeaponGauge>, HudError> {
    let Some(weapons) = weapons else {
        return Ok(None);
    };
    if weapons.session() != bound.get() {
        return Err(HudError::ForeignAuthority {
            source: "weapons",
            expected: bound,
            found: weapons.session(),
        });
    }
    let Some(state) = weapons.state(&actor) else {
        return Ok(None);
    };
    let mut guns = Vec::new();
    for definition in weapons.cadence().resolver().definitions(&actor) {
        let mount = definition.mount();
        guns.push(MountedGun {
            mount: mount.clone(),
            kind: definition.kind(),
            ammunition: definition.ammunition().clone(),
            rounds: state.ammunition(mount),
            selected: state.selected().contains(mount),
            disabled: state.is_disabled(mount),
        });
    }
    let selected_rounds = guns
        .iter()
        .filter(|gun| gun.selected)
        .map(|gun| gun.rounds)
        .sum();
    let ammunition = guns
        .iter()
        .filter(|gun| gun.selected)
        .map(|gun| gun.ammunition.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let has_selection = !state.selected().is_empty();
    Ok(Some(WeaponGauge {
        guns,
        selected_rounds,
        ammunition,
        has_selection,
        empty: has_selection && selected_rounds == 0,
    }))
}

/// The damage panel's gather: every node of the actor's graph with its live
/// part state, or `None` when the resolver does not describe the actor.
fn airframe_zones(
    damage: Option<&DamageResolver>,
    bound: SessionId,
    actor: ActorId,
) -> Result<Option<Vec<DamageZone>>, HudError> {
    let Some(damage) = damage else {
        return Ok(None);
    };
    if damage.session() != bound {
        return Err(HudError::ForeignAuthority {
            source: "damage",
            expected: bound,
            found: damage.session().get(),
        });
    }
    let Some(graph) = damage.graph(&actor) else {
        return Ok(None);
    };
    Ok(Some(
        graph
            .nodes()
            .map(|node| DamageZone {
                node: node.key().clone(),
                kind: node.kind(),
                state: damage
                    .part_state(&actor, node.key())
                    .unwrap_or(PartState::Unknown),
                scene_binding: node.scene_binding().cloned(),
            })
            .collect(),
    ))
}

/// The launcher cluster's gather: the actor's launchable components, or
/// `None` when the session is closed or the actor has none.
fn ordnance_rows(
    ordnance: Option<&OrdnanceSession>,
    bound: SessionId,
    actor: ActorId,
) -> Result<Option<Vec<MountedOrdnance>>, HudError> {
    let Some(ordnance) = ordnance else {
        return Ok(None);
    };
    if ordnance.session() != bound.get() {
        return Err(HudError::ForeignAuthority {
            source: "ordnance",
            expected: bound,
            found: ordnance.session(),
        });
    }
    if ordnance.is_closed() {
        return Ok(None);
    }
    let audit = session_ordnance_audit(ordnance);
    let rows: Vec<MountedOrdnance> = audit
        .rows()
        .iter()
        .filter(|row| row.shooter() == actor && row.is_launchable())
        .map(|row| MountedOrdnance {
            ordnance: row.ordnance().clone(),
            mount: row
                .launcher()
                .expect("a launchable component always names a mount")
                .clone(),
            family: row.family(),
        })
        .collect();
    Ok((!rows.is_empty()).then_some(rows))
}

/// The target display's gather: the published HUD readout, only while the
/// consumers are bound to this observer — another aircraft's views are
/// never drawn.
fn target_view(targets: Option<&TargetConsumers>, actor: ActorId) -> Option<&HudTargetReadout> {
    let consumers = targets?;
    match consumers.bound() {
        Some(bound) if bound.observer == actor => consumers.hud(),
        _ => None,
    }
}
