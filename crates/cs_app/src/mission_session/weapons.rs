//! M01's playable weapons (M01-PLAY-WEAPONS, #1437).
//!
//! M01's Devastator carries guns and rockets; nothing opened a weapon session
//! for the running mission, so the player could not shoot. This module is that
//! session: the **designed** M01 loadout, the cockpit publication that turns
//! the input boundary's delivered edges into fire orders, and the per-tick
//! schedules that resolve them through the production F27 (guns) and F28
//! (rockets) paths.
//!
//! # The loadout is designed, and says so
//!
//! The original's Devastator ammunition and rocket ordnance were not
//! recovered: there is no production reader that returns M01's declared
//! ammunition or ordnance ([`crate::mission_launch`]'s
//! `player_configuration` surface binds the airframe and the start pose and
//! nothing else). Which weapon *records* the fresh-campaign Devastator's gun
//! slots carry **is** measured — `wep_50`, `wep_40`, `wep_30`
//! (`[gun_loadout]` in `data/owner-static/engine-facts.toml`, #1628) — and
//! the guns' shot sound now names the member the selected record's measured
//! `LOOPED_SOUND_NAME` chain answers. Every other value here is
//! [`Origin::Designed`] with a designed [`Provenance`] claim
//! (`m01-play-weapons.designed-loadout`): a playable loadout that is
//! **never** presented as original. When F27-E/F28-D measure the rest of the
//! original's loadout, these records are what gets replaced.
//!
//! # What this module owns, and what it does not
//!
//! It owns the *player's* session: the [`WeaponSession`], the
//! [`OrdnanceSession`], the shared [`SessionDamageResolver`], the
//! [`TargetingSession`] and the publication of one tick's
//! [`WeaponTickInput`]. It owns the player's **rig** — the
//! [`MountPoseBinding`] nodes every gun and launcher fires from, hung under
//! the live player body.
//!
//! It does **not** spawn mission targets. A victim is an entity the spawn
//! host binds ([`crate::damage::DamageActorBinding`], a
//! [`crate::weapons::PartSweptBox`], a
//! [`crate::debris::PartDebrisBinding`]) and registers with the shared
//! resolver; M01's own actors are another task's. A world with no bound
//! victim fires into a world that resolves nothing, which is the honest
//! state and is reported rather than papered over.

use std::collections::BTreeMap;

use bevy::prelude::*;

use cs_content::damage::DamageNodeKey;
use cs_content::ordnance::DeclaredInheritanceRule as OrdnanceInheritanceRule;
use cs_content::ordnance::{
    DeclaredArmingRule, DeclaredEquipmentRules, DeclaredFuseRule, DeclaredGuidanceRule,
    DeclaredHardpointKind, DeclaredLaunchGeometry, DeclaredOrdnance, DeclaredOrdnanceDetails,
    DeclaredOrdnanceFamily, DeclaredOrdnanceMedia, DeclaredProjectile, DeclaredProximityFuse,
    DeclaredStackLoad,
};
use cs_content::target_rules::{
    DeclaredAllegiance, DeclaredRelation, DeclaredTargetRules, TargetRuleSet,
};
use cs_content::weapons::{
    AmmunitionId, DeclaredAmmunition, DeclaredCaliber, DeclaredFriendlyFireRule,
    DeclaredGunDefinition, DeclaredGunMountKind, DeclaredGunRate, DeclaredInheritanceRule,
    DeclaredSelfHitRule, DeclaredSpreadCone, DeclaredWeaponDamage, InteractionRules,
};
use cs_sim::damage::{ActorId, DamageEventKind, DamageResolver, SystemKind};
use cs_sim::targeting::{Allegiance, TargetClass, TargetRecord};
use cs_sim::time::TickRate;
use cs_sim::weapons::{
    GunBank, OrdnanceId, OrdnanceRegistry, ProjectileId, StatusEffectTarget, TargetPath,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::{Radians, WorldPosition};

use super::burst_puffs::{PendingOrdnanceBursts, PuffBurst};
use super::weapon_counts::{OriginalGunRecord, OriginalWeaponCounts};
use crate::damage::{DamageActorBinding, PendingDamageEvents};
use crate::input::platform::PlatformInput;
use crate::mission_actors::{MissionGroundTarget, SCRIPTED_DAMAGE_NODE};
use crate::ordnance::{
    OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId, OrdnanceLauncherBinding, OrdnanceOrder,
    OrdnanceSession, OrdnanceStep, lower_ordnance, step_ordnance_session,
};
use crate::physics::PhysicsTickLedger;
use crate::scene::{NodeVisualTransform, SceneGeneration};
use crate::score::SessionScoreLedger;
use crate::targeting::{TargetingSession, lower_rules};
use crate::weapons::{
    CockpitInput, MountPoseBinding, SessionDamageResolver, WeaponActorBinding, WeaponSession,
    WeaponTickInput, WeaponTickInputs,
};

/// The claim every designed M01 weapon record carries: the loadout is project
/// design, not a measurement of the original.
pub const MISSION_WEAPON_CLAIM: &str = "m01-play-weapons.designed-loadout";

/// The claim the numbers read from the selected gun's `weapons.zrd` record
/// bind under (Rally #1742).
pub const MISSION_GUN_RECORD_CLAIM: &str = "m01-guns-and-rockets.selected-gun-record";

/// The claim the designed calm-air fallback binds under, when the mission's
/// authored wind is an explicit unknown.
pub const MISSION_WIND_CLAIM: &str = "m01-play-weapons.designed-calm-air";

/// The player's actor serial within the mission session.
///
/// **Reserved at the top of the serial space.** The mission's scripted-actor
/// spawner ([`crate::mission_actors`], M01-PLAY-ACTORS #1435) allocates one
/// session actor per spawned aircraft and zeppelin from an
/// [`ActorAllocator`](cs_types::net::ActorAllocator) that issues serials from
/// `1` upward for the served session. A small player serial would therefore be
/// handed to the first spawned actor as well, giving the player and that actor
/// one identity: the F27 `SelfHitRule::Excluded` would read the player's own
/// shot on it as a self-hit, and the rocket's target search would skip it as
/// the shooter. `u64::MAX` is the one serial the allocator can never issue —
/// [`ActorAllocator::allocate`](cs_types::net::ActorAllocator::allocate)
/// refuses the id whose successor would overflow — so the player owns a serial
/// no session actor ever shares.
pub const MISSION_PLAYER_SERIAL: u64 = u64::MAX;

/// The gun path's router producer serial.
pub const MISSION_ROUTER_PRODUCER: u32 = 201;
/// The rocket path's producer serial.
pub const MISSION_ORDNANCE_PRODUCER: u32 = 202;
/// The shared damage authority's producer serial.
pub const MISSION_DAMAGE_PRODUCER: u32 = 203;
/// The local cockpit channel's producer serial.
pub const MISSION_COCKPIT_PRODUCER: u32 = 204;

/// The nose gun's damage-node mount.
pub const MISSION_GUN_MOUNT_NOSE: &str = "gun_mount_1";
/// The left wing gun's damage-node mount.
pub const MISSION_GUN_MOUNT_WING_LEFT: &str = "gun_mount_2";
/// The right wing gun's damage-node mount.
pub const MISSION_GUN_MOUNT_WING_RIGHT: &str = "gun_mount_3";
/// The rocket launcher's damage-node mount.
pub const MISSION_ROCKET_MOUNT: &str = "ordnance_mount_1";

/// The designed player faction.
pub const MISSION_FACTION_PLAYER: &str = "designed.m01.player";
/// The designed hostile faction.
pub const MISSION_FACTION_HOSTILE: &str = "designed.m01.hostile";
/// The designed neutral faction.
pub const MISSION_FACTION_NEUTRAL: &str = "designed.m01.neutral";

/// The mounted position of the nose gun, in metres, relative to the player
/// body. Designed: fire group 0 of the original has two firepoints and none is
/// a nose point, so this third gun has no measured muzzle (recorded unknown in
/// the #2113 finding).
const NOSE_MOUNT_OFFSET_M: [f32; 3] = [0.0, -0.4, -4.0];
/// The left wing gun's muzzle: the stored position of `firepoint7` of the
/// player Devastator (`player_pfighter`), the first slot of fire group 0 (task
/// #2113, `docs/findings/2026-10-25-f27-c-fx-orig-muzzle-mounts.md`).
/// Measured from `planes.zbd`; not `verified_original`.
pub const WING_LEFT_OFFSET_M: [f32; 3] = [-1.165_008_1, -0.620_576_6, -1.199_481_9];
/// The right wing gun's muzzle: `firepoint8`, the second slot of fire group 0.
/// Measured from `planes.zbd`.
pub const WING_RIGHT_OFFSET_M: [f32; 3] = [1.204_927_2, -0.584_683_8, -1.199_481_9];
/// The rocket launcher's designed offset.
const ROCKET_OFFSET_M: [f32; 3] = [0.0, -1.0, -2.0];

fn claim() -> ClaimId {
    ClaimId::new(MISSION_WEAPON_CLAIM).expect("the claim id is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn designed_faction(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Faction, key).expect("the faction id is valid")
}

/// The mission's designed gun-loadout catalog ids, read by the rig and the
/// audit rather than spelled twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionWeaponIds {
    /// The ammunition type the guns carry.
    pub ammunition: ContentId,
    /// The loadout catalog subject the guns and rockets belong to.
    pub loadout: ContentId,
}

/// The Devastator's designed M01 gun loadout.
///
/// Three guns — a nose cannon and one gun per wing — all firing the one
/// designed ammunition type. Every number is project design.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionLoadout {
    /// The declared guns, in mount order.
    pub guns: Vec<DeclaredGunDefinition>,
    /// The bank selected at open: every gun.
    pub bank: GunBank,
    /// The catalog ids the loadout names.
    pub ids: MissionWeaponIds,
}

/// Builds the designed gun for `mount` of `kind`.
fn designed_gun(mount: &str, kind: DeclaredGunMountKind, key: &str) -> DeclaredGunDefinition {
    gun_with(mount, kind, key, &GunNumbers::designed())
}

/// The numbers one declared gun carries, and how they are labelled.
struct GunNumbers {
    ticks_between_shots: u32,
    muzzle_velocity_mps: f64,
    lifetime_ticks: u64,
    armor: f64,
    internal: f64,
    /// Whether the numbers were read from the installation's gun record.
    measured: bool,
}

impl GunNumbers {
    const fn designed() -> Self {
        Self {
            ticks_between_shots: MISSION_TICKS_BETWEEN_SHOTS,
            muzzle_velocity_mps: MISSION_MUZZLE_VELOCITY_MPS,
            lifetime_ticks: MISSION_LIFETIME_TICKS,
            armor: MISSION_ARMOR_DAMAGE,
            internal: MISSION_INTERNAL_DAMAGE,
            measured: false,
        }
    }

    /// The selected gun's record, at `ticks_per_second` fixed ticks a second.
    fn from_record(record: &OriginalGunRecord, ticks_per_second: u32) -> Self {
        let hz = f64::from(ticks_per_second);
        Self {
            ticks_between_shots: ((hz / record.fire_rate_per_s).round() as u32).max(1),
            muzzle_velocity_mps: record.velocity_mps,
            lifetime_ticks: ((record.range_m / record.velocity_mps * hz).ceil() as u64).max(1),
            armor: record.armor_damage,
            internal: record.health_damage,
            measured: true,
        }
    }

    /// A value of this gun, labelled by where it came from.
    fn value<T>(&self, value: T) -> Resolved<T> {
        if !self.measured {
            return known(value);
        }
        let provenance = Provenance::new(
            ClaimId::new(MISSION_GUN_RECORD_CLAIM).expect("the claim id is valid"),
            ClaimStatus::ObservedTool,
            None,
        )
        .expect("an observed-tool provenance needs no source");
        Resolved::Known(Known::new(value, provenance))
    }
}

fn gun_with(
    mount: &str,
    kind: DeclaredGunMountKind,
    key: &str,
    numbers: &GunNumbers,
) -> DeclaredGunDefinition {
    DeclaredGunDefinition::try_new(
        ContentId::from_source(ContentKind::Weapon, key).expect("the gun id is valid"),
        Origin::Designed,
        cs_content::damage::DamageNodeKey::new(mount).expect("the mount key is valid"),
        kind,
        None,
        known(DeclaredCaliber::try_new(MISSION_CALIBER).expect("the caliber text is valid")),
        known(mission_ammunition()),
        numbers.value(DeclaredGunRate {
            ticks_between_shots: numbers.ticks_between_shots,
        }),
        numbers.value(numbers.muzzle_velocity_mps),
        numbers.value(numbers.lifetime_ticks),
        DeclaredSpreadCone {
            half_angle: known(Radians(MISSION_SPREAD_HALF_ANGLE_RAD)),
        },
        DeclaredWeaponDamage {
            armor: numbers.value(numbers.armor),
            internal: numbers.value(numbers.internal),
        },
        known(DeclaredInheritanceRule::Full),
        known(
            ContentId::from_source(ContentKind::HardpointEquipment, MISSION_MUZZLE_EFFECT)
                .expect("the effect id is valid"),
        ),
        // The measured member the player's selected M01 gun record loops
        // (#1628): `gun_sound` measures and delivers exactly this id beside
        // the launch sound. The designed wrapper's provenance stays honest —
        // which member the guns *name* is our binding; what the member *is*
        // is measured.
        known(super::gun_sound::measured_gun_sound_id()),
        designed_interaction_rules(),
        Provenance::designed(claim()),
    )
    .expect("the designed M01 gun is valid")
}

/// The designed ammunition id the loadout carries.
#[must_use]
pub fn mission_ammunition_id() -> ContentId {
    ContentId::from_source(ContentKind::Ammo, MISSION_AMMUNITION_KEY).expect("the ammo id is valid")
}

/// The designed ammunition id in the `ammo` namespace.
#[must_use]
pub fn mission_ammunition() -> AmmunitionId {
    AmmunitionId::try_new(mission_ammunition_id()).expect("the designed ammo id is in ammo")
}

/// The one designed ammunition type: the same damage profile the guns carry.
///
/// F27 non-negotiable 1 forbids deriving a gun's damage from a multiplier
/// table, so the ammunition record carries its own designed profile and the
/// guns repeat it rather than scaling one from the other.
#[must_use]
pub fn designed_ammunition() -> DeclaredAmmunition {
    DeclaredAmmunition::try_new(
        mission_ammunition(),
        Origin::Designed,
        known(DeclaredCaliber::try_new(MISSION_CALIBER).expect("the caliber text is valid")),
        DeclaredWeaponDamage {
            armor: known(MISSION_ARMOR_DAMAGE),
            internal: known(MISSION_INTERNAL_DAMAGE),
        },
        designed_interaction_rules(),
        Provenance::designed(claim()),
    )
    .expect("the designed M01 ammunition is valid")
}

/// The designed interaction rules every M01 gun runs under.
///
/// Self-hit excluded and friendly fire limited to declared hostile or neutral
/// relations (an ally is never hit): design, not measurement (F27
/// non-negotiable 4). The original's hit path compares no team
/// (`2026-10-25-m01-hit-dispatch-team-trace.md`, #2099), so neutral world
/// targets such as `hydrogentank1..4` are not refused by relation.
#[must_use]
pub fn designed_interaction_rules() -> InteractionRules {
    InteractionRules {
        self_hit: known(DeclaredSelfHitRule::Excluded),
        friendly_fire: known(DeclaredFriendlyFireRule::NonFriendly),
        penetration: known(false),
        ricochet: known(false),
        ammo_switching: known(false),
    }
}

/// The Devastator's designed M01 gun loadout.
#[must_use]
pub fn designed_loadout() -> MissionLoadout {
    let guns = vec![
        designed_gun(
            MISSION_GUN_MOUNT_NOSE,
            DeclaredGunMountKind::Nose,
            MISSION_GUN_NOSE_KEY,
        ),
        designed_gun(
            MISSION_GUN_MOUNT_WING_LEFT,
            DeclaredGunMountKind::WingLeft,
            MISSION_GUN_WING_LEFT_KEY,
        ),
        designed_gun(
            MISSION_GUN_MOUNT_WING_RIGHT,
            DeclaredGunMountKind::WingRight,
            MISSION_GUN_WING_RIGHT_KEY,
        ),
    ];
    let bank = GunBank::try_new([
        cs_content::damage::DamageNodeKey::new(MISSION_GUN_MOUNT_NOSE)
            .expect("the nose mount key is valid"),
        cs_content::damage::DamageNodeKey::new(MISSION_GUN_MOUNT_WING_LEFT)
            .expect("the wing mount key is valid"),
        cs_content::damage::DamageNodeKey::new(MISSION_GUN_MOUNT_WING_RIGHT)
            .expect("the wing mount key is valid"),
    ])
    .expect("the designed bank names its mounts");
    MissionLoadout {
        guns,
        bank,
        ids: MissionWeaponIds {
            ammunition: mission_ammunition_id(),
            loadout: ContentId::from_source(ContentKind::Loadout, MISSION_LOADOUT_KEY)
                .expect("the loadout id is valid"),
        },
    }
}

/// The player's M01 gun loadout when the selected gun's record is read
/// (Rally #1742): **one** gun — the engine's current-gun selector keeps the
/// first slot, `wep_50` — firing at that record's own rate, speed, range and
/// damage. The original's HUD counts one gun's `AMMO_LIMIT` down (2000 to 1980
/// over a short hold in the owner's capture), so the two wing guns the
/// designed loadout adds are not part of it. The mount, the caliber text and
/// the spread stay designed.
#[must_use]
pub fn selected_gun_loadout(record: &OriginalGunRecord, ticks_per_second: u32) -> MissionLoadout {
    let numbers = GunNumbers::from_record(record, ticks_per_second);
    let designed = designed_loadout();
    let mount = DamageNodeKey::new(MISSION_GUN_MOUNT_NOSE).expect("the nose mount key is valid");
    MissionLoadout {
        guns: vec![gun_with(
            MISSION_GUN_MOUNT_NOSE,
            DeclaredGunMountKind::Nose,
            MISSION_GUN_NOSE_KEY,
            &numbers,
        )],
        bank: GunBank::try_new([mount]).expect("the bank names its mount"),
        ids: designed.ids,
    }
}

/// The designed rocket's catalog content id.
#[must_use]
pub fn rocket_content_id() -> ContentId {
    ContentId::from_source(ContentKind::Weapon, MISSION_ROCKET_KEY)
        .expect("the rocket content id is valid")
}

/// The Devastator's designed M01 rocket: a proximity-fused flak rocket.
///
/// The family, fuse, arming, lifetime and blast are all project design. The
/// original's rocket records exist only as raw `weapons.zrd` blast numbers
/// (`cs_content::ordnance::read_original_rocket_blasts`), which F28-D has not
/// yet bound to an airframe loadout, so nothing here may claim to be the
/// original's.
#[must_use]
pub fn designed_rocket() -> DeclaredOrdnance {
    designed_rocket_with_capacity(MISSION_ROCKET_CAPACITY)
}

/// [`designed_rocket`] with its stack capacity replaced, for the capacity the
/// original's data states (PARITY2-M01-HUD-VALUES, #1732).
#[must_use]
pub fn designed_rocket_with_capacity(capacity_units: u64) -> DeclaredOrdnance {
    let projectile = DeclaredProjectile {
        launch: DeclaredLaunchGeometry {
            mount: DamageNodeKey::new(MISSION_ROCKET_MOUNT)
                .expect("the launcher mount key is valid"),
            hardpoint: known(DeclaredHardpointKind::Underslung),
            launch_speed_mps: known(MISSION_ROCKET_LAUNCH_SPEED_MPS),
            inheritance: known(OrdnanceInheritanceRule::Full),
            release_delay_ticks: known(0),
        },
        stack: DeclaredStackLoad {
            capacity_units: known(capacity_units),
            unit_mass_kg: known(MISSION_ROCKET_MASS_KG),
        },
        arming: DeclaredArmingRule::AfterTicks(known(MISSION_ROCKET_ARM_TICKS)),
        fuse: DeclaredFuseRule::Proximity(DeclaredProximityFuse {
            trigger_radius_m: known(MISSION_ROCKET_TRIGGER_RADIUS_M),
        }),
        guidance: DeclaredGuidanceRule::Unguided,
        lifetime_ticks: known(MISSION_ROCKET_LIFETIME_TICKS),
        area_effect: None,
        armor_damage: known(MISSION_ROCKET_ARMOR_DAMAGE),
        internal_damage: known(MISSION_ROCKET_INTERNAL_DAMAGE),
        status: Vec::new(),
        media: DeclaredOrdnanceMedia {
            visual: known(
                ContentId::from_source(ContentKind::HardpointEquipment, MISSION_ROCKET_VISUAL)
                    .expect("the rocket visual id is valid"),
            ),
            sound: known(
                ContentId::from_source(ContentKind::Sound, MISSION_ROCKET_SOUND)
                    .expect("the rocket sound id is valid"),
            ),
            particles: None,
        },
        equipment_rules: DeclaredEquipmentRules::default(),
    };
    DeclaredOrdnance::try_new(
        rocket_content_id(),
        Origin::Designed,
        DeclaredOrdnanceFamily::ProximityFlak,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        Provenance::designed(claim()),
    )
    .expect("the designed M01 rocket is valid")
}

/// The designed targeting rules the mission's session opens under.
///
/// Two factions, one directed hostile relation each way and a neutral pair:
/// project design, because M01's own relations are not recovered. The
/// selection table the session binds is the **measured** one — the original's
/// `Q`/`W`/`E`/`R`/`T` command set in
/// [`super::targeting_input::mission_selection_binding`]
/// (PARITY2-M01-TARGET-INDICATORS, #1727).
#[must_use]
pub fn designed_target_rules() -> DeclaredTargetRules {
    let player = designed_faction(MISSION_FACTION_PLAYER);
    let hostile = designed_faction(MISSION_FACTION_HOSTILE);
    let neutral = designed_faction(MISSION_FACTION_NEUTRAL);
    let relation = |from: &ContentId, to: &ContentId, allegiance| DeclaredRelation {
        from: from.clone(),
        to: to.clone(),
        allegiance: known(allegiance),
    };
    DeclaredTargetRules::try_new(
        ContentId::from_source(ContentKind::Mission, MISSION_TARGET_RULES_SUBJECT)
            .expect("the rules subject is valid"),
        Origin::Designed,
        vec![player.clone(), hostile.clone(), neutral.clone()],
        vec![
            relation(&player, &hostile, DeclaredAllegiance::Hostile),
            relation(&hostile, &player, DeclaredAllegiance::Hostile),
            relation(&player, &neutral, DeclaredAllegiance::Neutral),
            relation(&neutral, &player, DeclaredAllegiance::Neutral),
            relation(&neutral, &hostile, DeclaredAllegiance::Hostile),
            relation(&hostile, &neutral, DeclaredAllegiance::Hostile),
        ],
        TargetRuleSet {
            threat_window: known(MISSION_THREAT_WINDOW_TICKS),
            crosshair_cone: known(Radians(std::f64::consts::PI / 18.0)),
            lead_indicator: known(true),
            aim_assistance: known(false),
        },
        Provenance::designed(claim()),
    )
    .expect("the designed M01 target rules are valid")
}

/// The player's live weapon session: the generation, the actor, the scene
/// generation the rig's mounts are stamped with, and the rocket the launcher
/// fires.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct MissionWeapons {
    loadout: MissionLoadout,
    session: SessionId,
    player: ActorId,
    generation: SceneGeneration,
    rocket: OrdnanceId,
}

impl MissionWeapons {
    /// The session generation this session is confined to.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The player's actor, the shooter every minted order names.
    #[must_use]
    pub const fn player(&self) -> ActorId {
        self.player
    }

    /// The scene generation live entities are stamped with.
    #[must_use]
    pub const fn generation(&self) -> SceneGeneration {
        self.generation
    }

    /// The rocket component the launcher fires.
    #[must_use]
    pub const fn rocket(&self) -> &OrdnanceId {
        &self.rocket
    }
}

/// Every session and authority M01's weapons are opened as one unit.
///
/// [`install`](Self::install) puts them on the world the way a running
/// session's open or restart does: the resources the F27/F29 schedules
/// already expect, plus the marker resource the publisher and the rig read.
pub struct MissionWeaponSessions {
    /// The marker the publisher and the rig read.
    pub mission: MissionWeapons,
    /// The F27 gun session.
    pub weapons: WeaponSession,
    /// The F28 ordnance session.
    pub ordnance: OrdnanceSession,
    /// The shared damage authority, the one every producer routes into.
    pub damage: DamageResolver,
    /// The F27/F30 declared allegiance authority.
    pub targeting: TargetingSession,
}

/// Why M01's weapon session could not be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionWeaponError {
    /// The M27/M28 session refused to open.
    Weapons(String),
    /// The declared targeting rules refused to lower.
    Targeting(String),
    /// No session was installed, so the rig or the publisher has nothing to
    /// attach to.
    NotInstalled,
    /// The player entity named for the rig does not exist.
    NoPlayer,
}

impl std::fmt::Display for MissionWeaponError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Weapons(reason) => write!(f, "the M01 weapon session refused: {reason}"),
            Self::Targeting(reason) => write!(f, "the M01 targeting rules refused: {reason}"),
            Self::NotInstalled => f.write_str("no M01 weapon session is installed"),
            Self::NoPlayer => f.write_str("no live player entity exists for the weapon rig"),
        }
    }
}

impl std::error::Error for MissionWeaponError {}

/// Opens M01's player weapon session for `session`.
///
/// # Errors
///
/// [`MissionWeaponError`] when the session generation is zero, the declared
/// loadout refuses to register, or the designed target rules refuse to lower.
pub fn open_mission_weapons(
    session: SessionId,
    rate: TickRate,
    generation: SceneGeneration,
) -> Result<MissionWeaponSessions, MissionWeaponError> {
    open_mission_weapons_with(session, rate, generation, None)
}

/// [`open_mission_weapons`] with the starting counts the original's data
/// states: every gun starts with `gun_rounds` (the selected record's
/// `AMMO_LIMIT`) and the rocket stack holds `rocket_stack`. `None` keeps the
/// designed counts.
///
/// # Errors
///
/// As [`open_mission_weapons`].
pub fn open_mission_weapons_with(
    session: SessionId,
    rate: TickRate,
    generation: SceneGeneration,
    counts: Option<OriginalWeaponCounts>,
) -> Result<MissionWeaponSessions, MissionWeaponError> {
    let loadout = counts.map_or_else(designed_loadout, |c| {
        selected_gun_loadout(&c.gun, rate.ticks_per_second())
    });
    let player = ActorId {
        session,
        serial: MISSION_PLAYER_SERIAL,
    };
    let mut weapons = WeaponSession::new(session.get(), Tick(0), MISSION_ROUTER_PRODUCER)
        .map_err(|error| MissionWeaponError::Weapons(error.to_string()))?;
    weapons
        .register(
            player,
            &loadout.guns,
            loadout.bank.clone(),
            counts.map_or(MISSION_STARTING_ROUNDS, |c| c.gun_rounds),
        )
        .map_err(|error| MissionWeaponError::Weapons(error.to_string()))?;

    // The rocket path: the designed component is lowered into the supported
    // catalogue and registered for the player, exactly as a measured one
    // would be.
    let rocket =
        designed_rocket_with_capacity(counts.map_or(MISSION_ROCKET_CAPACITY, |c| c.rocket_stack));
    let mut catalogue = OrdnanceRegistry::new();
    let component =
        lower_ordnance(&rocket).map_err(|error| MissionWeaponError::Weapons(error.to_string()))?;
    catalogue
        .register(component)
        .map_err(|error| MissionWeaponError::Weapons(error.to_string()))?;
    let rocket_id = OrdnanceId::try_new(rocket.ordnance().clone())
        .map_err(|error| MissionWeaponError::Weapons(error.to_string()))?;
    let mut ordnance = OrdnanceSession::new(
        session.get(),
        Tick(0),
        rate,
        MISSION_ORDNANCE_PRODUCER,
        catalogue,
    )
    .map_err(|error| MissionWeaponError::Weapons(error.to_string()))?;
    ordnance
        .register(player, std::slice::from_ref(&rocket))
        .map_err(|error| MissionWeaponError::Weapons(error.to_string()))?;

    let rules = designed_target_rules();
    let lowered =
        lower_rules(&rules).map_err(|error| MissionWeaponError::Targeting(error.to_string()))?;
    let mut targeting = TargetingSession::new(
        session,
        lowered,
        super::targeting_input::mission_selection_binding(),
        rules.subject().clone(),
        generation,
    );
    targeting
        .store_mut()
        .register(TargetRecord {
            actor: player,
            faction: designed_faction(MISSION_FACTION_PLAYER),
            class: TargetClass::Aircraft,
            objective: false,
            revealed: true,
            phase_eligible: true,
            position: WorldPosition::try_new([0.0, 0.0, 0.0])
                .expect("the designed origin is finite"),
        })
        .map_err(|error| MissionWeaponError::Targeting(error.to_string()))?;

    Ok(MissionWeaponSessions {
        mission: MissionWeapons {
            loadout,
            session,
            player,
            generation,
            rocket: rocket_id,
        },
        weapons,
        ordnance,
        damage: DamageResolver::new(session, MISSION_DAMAGE_PRODUCER),
        targeting,
    })
}

impl MissionWeaponSessions {
    /// Installs every session and authority on `world`.
    ///
    /// The score ledger is **not** opened here: the mission host already owns
    /// it for the session it launches. When one is installed, the player is
    /// admitted to it so a kill attributed to the player can score; a world
    /// with no ledger scores nothing, which is the damage schedule's own
    /// statement and not this function's to paper over.
    ///
    /// # Errors
    ///
    /// [`MissionWeaponError::NoPlayer`] is never returned; that variant
    /// belongs to the rig.
    pub fn install(self, world: &mut World) -> Result<(), MissionWeaponError> {
        let player = self.mission.player;
        let session = self.mission.session;
        // The mission's actor spawner (M01-PLAY-ACTORS, #1435) runs at Startup
        // and registers its damage actors with the session's shared resolver.
        // This open runs after it, so a resolver already serving this session
        // is **kept**: replacing it would drop every registration the actors
        // committed. A resolver of another generation (a torn-down run) is
        // replaced, exactly as the weapons session contract spells.
        let reuse = world
            .get_resource::<SessionDamageResolver>()
            .is_some_and(|resolver| resolver.session() == session.get());
        world.insert_resource(self.mission);
        world.insert_resource(self.weapons);
        world.insert_resource(MissionOrdnanceSession(self.ordnance));
        if !reuse {
            world.insert_resource(SessionDamageResolver::new(self.damage));
        }
        world.insert_resource(self.targeting);
        world.init_resource::<WeaponTickInputs>();
        world.init_resource::<MissionOrdnanceOrders>();
        world.init_resource::<MissionCockpitActions>();
        world.init_resource::<MissionWeaponReport>();
        // A generation a load never minted is generation zero; the rig and
        // every live read must still agree on one, so the counter is present.
        world.init_resource::<crate::scene::SceneGenerations>();
        if let Some(mut ledger) = world.get_resource_mut::<SessionScoreLedger>()
            && ledger.session() == session
        {
            let _ = admit_actor(&mut ledger, player);
        }
        Ok(())
    }
}

/// Admits `actor` to the session's score ledger, the spawn host's own duty.
///
/// Idempotent in effect: a second admission of the same actor is refused by
/// the ledger and reported as `false` rather than panicking, so a restart or
/// a repeated pass is a no-op.
pub fn admit_actor(ledger: &mut SessionScoreLedger, actor: ActorId) -> bool {
    let state = cs_sim::net_state::NetActorState::spawn(
        actor,
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("the designed origin is finite"),
        cs_types::space::Quaternion::IDENTITY,
        MISSION_DESIGNED_HULL,
    );
    ledger.admit(state).is_ok()
}

/// One gun or launcher mount of the player's rig.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RigMount {
    mount: &'static str,
    offset_m: [f32; 3],
}

/// Every mount the player's rig hangs on the body.
const RIG_MOUNTS: &[RigMount] = &[
    RigMount {
        mount: MISSION_GUN_MOUNT_NOSE,
        offset_m: NOSE_MOUNT_OFFSET_M,
    },
    RigMount {
        mount: MISSION_GUN_MOUNT_WING_LEFT,
        offset_m: WING_LEFT_OFFSET_M,
    },
    RigMount {
        mount: MISSION_GUN_MOUNT_WING_RIGHT,
        offset_m: WING_RIGHT_OFFSET_M,
    },
    RigMount {
        mount: MISSION_ROCKET_MOUNT,
        offset_m: ROCKET_OFFSET_M,
    },
];

/// How many of [`RIG_MOUNTS`] are guns: the player's rocket launcher is
/// always the last entry, and no AI aircraft carries one (`attach_gun_rig`).
const GUN_RIG_MOUNTS: usize = RIG_MOUNTS.len() - 1;

/// Attaches the player's weapon rig to `player`: the [`WeaponActorBinding`]
/// that marks the live hierarchy and one [`MountPoseBinding`] node per gun.
///
/// The mounts hang under the body, so `live_mount_transforms` reads a pose
/// that tracks the animated airframe and the inherited airframe velocity from
/// the same live hierarchy — F27 non-negotiable 2. The two wing offsets are the
/// measured `firepoint7`/`firepoint8` points; the nose and rocket offsets are
/// designed.
///
/// # Errors
///
/// [`MissionWeaponError::NotInstalled`] when no session was installed,
/// [`MissionWeaponError::NoPlayer`] when `player` does not exist.
pub fn attach_player_rig(world: &mut World, player: Entity) -> Result<usize, MissionWeaponError> {
    let mission = world
        .get_resource::<MissionWeapons>()
        .cloned()
        .ok_or(MissionWeaponError::NotInstalled)?;
    let loadout = mission.loadout.clone();
    if world.get_entity(player).is_err() {
        return Err(MissionWeaponError::NoPlayer);
    }
    world.entity_mut(player).insert((
        WeaponActorBinding {
            actor: mission.player,
            guns: loadout.guns.iter().map(|gun| gun.gun().clone()).collect(),
            loadout: loadout.ids.loadout.clone(),
            generation: mission.generation,
        },
        OrdnanceLauncherBinding {
            actor: mission.player,
            ordnance: vec![rocket_content_id()],
            loadout: loadout.ids.loadout.clone(),
            generation: mission.generation,
        },
    ));
    Ok(hang_rig_mounts(
        world,
        player,
        RIG_MOUNTS,
        mission.generation,
    ))
}

/// Attaches the mission's **gun** rig to a live actor entity that is not the
/// player's seat: the same designed mounts the player's rig hangs and the
/// same [`WeaponActorBinding`] shape, but no rocket launcher, because an AI
/// aircraft fires guns only (M01-PLAY-AI, #1436).
///
/// The caller names the actor, so an AI aircraft's muzzles are the mission's
/// own designed hardpoints under its own body — the same poses
/// [`compose_mission_mount_poses`] composes every fixed tick.
///
/// # Errors
///
/// [`MissionWeaponError::NotInstalled`] when no session was installed,
/// [`MissionWeaponError::NoPlayer`] when `root` does not exist.
pub fn attach_gun_rig(
    world: &mut World,
    root: Entity,
    actor: ActorId,
) -> Result<usize, MissionWeaponError> {
    let mission = world
        .get_resource::<MissionWeapons>()
        .cloned()
        .ok_or(MissionWeaponError::NotInstalled)?;
    if world.get_entity(root).is_err() {
        return Err(MissionWeaponError::NoPlayer);
    }
    let loadout = designed_loadout();
    world.entity_mut(root).insert(WeaponActorBinding {
        actor,
        guns: loadout.guns.iter().map(|gun| gun.gun().clone()).collect(),
        loadout: loadout.ids.loadout.clone(),
        generation: mission.generation,
    });
    // The gun mounts only: the rocket mount belongs to the player's
    // launcher, which no AI aircraft carries.
    Ok(hang_rig_mounts(
        world,
        root,
        &RIG_MOUNTS[..GUN_RIG_MOUNTS],
        mission.generation,
    ))
}

/// Hangs one [`MountPoseBinding`] node per rig entry under `root`.
fn hang_rig_mounts(
    world: &mut World,
    root: Entity,
    mounts: &[RigMount],
    generation: crate::scene::SceneGeneration,
) -> usize {
    for rig in mounts {
        let offset = Vec3::new(rig.offset_m[0], rig.offset_m[1], rig.offset_m[2]);
        world.spawn((
            MountPoseBinding {
                mount: DamageNodeKey::new(rig.mount).expect("the rig mount key is valid"),
                generation,
            },
            NodeVisualTransform(GlobalTransform::from_translation(offset)),
            Transform::from_translation(offset),
            ChildOf(root),
        ));
    }
    mounts.len()
}

/// The mission's weapon report: what the publisher decided this tick, and what
/// it could not.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionWeaponReport {
    /// Fixed ticks the publisher has published for.
    pub ticks: u64,
    /// The committed tick the last publication served.
    pub at: Option<Tick>,
    /// The wind the last record carried, and where it came from.
    pub wind: Option<[f64; 3]>,
    /// The record carried a designed calm-air fallback because the mission's
    /// authored wind is an explicit unknown.
    pub designed_calm_air: bool,
    /// The player's delivered flight edges published in the last record.
    pub actions: Vec<Action>,
    /// The rockets that could not be launched, by name.
    pub rocket_refusals: Vec<String>,
    /// How many rocket launches have been refused, repeats included. The HUD
    /// notice counts from this, so a second refusal of the same kind is shown
    /// again once the first line has expired (M01-WINDOW-FIRE, #1536).
    pub rocket_refusal_events: u64,
    /// The selection pass's last refusal — an unregistered observer, or an
    /// under-reticule edge in a frame with no crosshair ray — by name, when
    /// one happened (PARITY2-M01-TARGET-INDICATORS, #1727).
    pub targeting_refusal: Option<String>,
    /// Why the publisher could not run at all.
    pub refused: Option<String>,
}

impl MissionWeaponReport {
    fn begin_tick(&mut self) {
        self.ticks += 1;
        self.at = None;
        self.wind = None;
        self.designed_calm_air = false;
        self.actions.clear();
        self.targeting_refusal = None;
        // `rocket_refusals` deliberately accumulates: a refusal is a fact
        // about the world (no hostile target) that repeats every tick it
        // holds, and a report that cleared it would make the reason
        // unobservable from outside the tick that produced it.
        self.refused = None;
    }
}

/// The wind the mission's weapon publication carries.
///
/// The mission's own environment authority is the source; a record that
/// authors no wind is an explicit unknown, and the session then publishes a
/// **designed calm air** carrying [`MISSION_WIND_CLAIM`] rather than a
/// silently invented one. `unknown` is true when that fallback was used, so
/// the mission's report says which wind a flight actually resolved under.
pub struct MissionWind {
    /// The wind velocity in world metres per second.
    pub velocity_m_s: [f64; 3],
    /// The velocity is the designed calm-air fallback, not the mission's own
    /// authored wind.
    pub designed: bool,
    /// The claim the fallback binds under, when it was used.
    pub claim: Option<ClaimId>,
}

/// Resolves the wind the next published record carries.
///
/// The mission's own environment authority is the source; a record that
/// authors no wind is an explicit unknown, and the session then publishes a
/// **designed calm air** carrying [`MISSION_WIND_CLAIM`] rather than a
/// silently invented one — the F27 contract leaves that policy to the
/// session, and this is the policy, labelled.
#[must_use]
pub fn mission_wind(world: &World) -> MissionWind {
    if let Some(host) = world.get_resource::<super::MissionHost>()
        && let Ok(wind) = host.environment().wind()
    {
        return MissionWind {
            velocity_m_s: wind.velocity_m_s(),
            designed: false,
            claim: None,
        };
    }
    MissionWind {
        velocity_m_s: [0.0, 0.0, 0.0],
        designed: true,
        claim: Some(ClaimId::new(MISSION_WIND_CLAIM).expect("the claim id is valid")),
    }
}

/// Builds one tick's published record from the tick, the player and the
/// delivered cockpit actions.
///
/// The production publisher and the acceptance test build the record through
/// this one function, so a test drives exactly what a session runs.
#[must_use]
pub fn mission_tick_input(
    session: SessionId,
    at: Tick,
    player: ActorId,
    wind: [f64; 3],
    actions: Vec<Action>,
) -> WeaponTickInput {
    WeaponTickInput {
        session: session.get(),
        at,
        wind_velocity_m_s: Some(wind),
        orders: Vec::new(),
        cockpit: Some(CockpitInput {
            shooter: player,
            producer: MISSION_COCKPIT_PRODUCER,
            actions,
        }),
    }
}

/// Publishes one committed tick's weapon input, the way a running session
/// drives the F27 schedule.
///
/// Runs in `FixedPostUpdate` **before**
/// [`drive_weapon_session`](crate::weapons::drive_weapon_session), so the
/// record it pushes serves the tick the ledger has just committed. A world
/// with no installed session, no committed tick or no delivered edge
/// publishes nothing and says so.
pub fn publish_mission_weapon_input(world: &mut World) {
    ensure_mission_weapons(world);
    let Some(mission) = world.get_resource::<MissionWeapons>().cloned() else {
        return;
    };
    let mut report = world
        .remove_resource::<MissionWeaponReport>()
        .unwrap_or_default();
    report.begin_tick();

    let Some(at) = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| Tick(ledger.ticks))
    else {
        report.refused = Some("no committed fixed tick exists yet".to_owned());
        world.insert_resource(report);
        return;
    };
    let wind = mission_wind(world);
    let mut actions: Vec<Action> = world
        .get_resource_mut::<MissionCockpitActions>()
        .map(|mut pending| std::mem::take(&mut pending.actions))
        .unwrap_or_default();
    // M01-WINDOW-FIRE (#1536): holding the guns key keeps firing for as long
    // as it is held, at the rate the loadout's guns declare. The input
    // boundary delivers one press edge per hold (F22's rule), so the repeat is
    // minted here: one edge per [`MISSION_TICKS_BETWEEN_SHOTS`], the very
    // rate the gun records declare and the F27 step's own per-mount cooldown
    // enforces. A press edge always fires at once; the cadence only decides
    // the repeats, so a player who taps faster than the gun fires gets the
    // F27's own named cooldown refusal rather than a hidden queue.
    // The launch cinematic leaves the player no control (#1663): whatever the
    // keys did during it fires nothing.
    let controlled = !super::launch_cinematic::launch_cinematic_playing(world);
    if !controlled {
        actions.clear();
    }
    // PARITY2-M01-TARGET-INDICATORS (#1727): the frame's measured target
    // commands act on the one selected target through the session's own
    // command-edge table. They run on the tick this publisher serves, with
    // the camera's crosshair ray for the under-reticule select — and on a
    // cleared action list (no control, the launch cinematic) they change no
    // selection, exactly like the weapons they share the queue with.
    if controlled {
        // The records the selection reads are the roster's own; the
        // roster passes stamp them at their bind poses, so the tick's live
        // poses are written first (PARITY2-M01-TARGET-INDICATORS, #1727).
        super::targeting_input::sync_mission_target_poses(world);
        let crosshair = super::targeting_input::mission_crosshair(world, mission.player);
        if let Err(error) = crate::targeting::apply_selection_edges(
            world,
            mission.player,
            &actions,
            cs_sim::targeting::SelectionFrame {
                now: at,
                crosshair: crosshair.as_ref(),
            },
        ) {
            report.targeting_refusal = Some(format!("{error}"));
        }
    }
    let pressed = actions.iter().copied().any(is_fire_guns);
    if controlled && !pressed && guns_key_held(world) && gun_hold_due(world, at) {
        actions.push(Action::Flight(FlightCommand::FirePrimary));
    }
    if actions.iter().copied().any(is_fire_guns) {
        world.insert_resource(MissionGunHold { last: Some(at) });
    }

    // The fire-rockets edges mint launch orders for the ordnance driver.
    let mut orders = MissionOrdnanceOrders::default();
    let mut sequence = 0u32;
    for _ in actions.iter().copied().filter(|a| is_fire_rockets(*a)) {
        match rocket_launch(world, &mission, at, sequence) {
            Ok(order) => {
                orders.orders.push(order);
                sequence += 1;
            }
            Err(reason) => {
                report.rocket_refusal_events += 1;
                if !report.rocket_refusals.contains(&reason) {
                    report.rocket_refusals.push(reason);
                }
            }
        }
    }
    if !orders.orders.is_empty() {
        world.insert_resource(orders);
    }

    let mut input = mission_tick_input(
        mission.session,
        at,
        mission.player,
        wind.velocity_m_s,
        actions.clone(),
    );
    // M01-PLAY-AI (#1436): an attacker's fire decisions join **this** tick's
    // record. A tick's inputs are consumed exactly once, so the AI cannot
    // publish a second record for the same tick — it rides the mission's own
    // publication.
    crate::mission_ai::append_ai_fire_orders(world, &mut input);
    // FLAK-B3 (#1882): a woken turret's shots ride the same record.
    super::turret_fire::append_turret_fire_orders(world, &mut input);
    world
        .get_resource_mut::<WeaponTickInputs>()
        .expect("the installed session initialises the weapon input queue")
        .push(input);
    report.at = Some(at);
    report.wind = Some(wind.velocity_m_s);
    report.designed_calm_air = wind.designed;
    report.actions = actions;
    world.insert_resource(report);
}

/// Resource wrapper for the mission's F28 ordnance session.
///
/// [`OrdnanceSession`] is not itself an ECS [`Resource`]; the mission holds it
/// behind this wrapper so the ordnance driver can take it out, step it and put
/// it back, exactly as the weapon schedule does with [`WeaponSession`].
#[derive(Resource)]
pub struct MissionOrdnanceSession(pub OrdnanceSession);

/// The delivered cockpit edges of the current rendered frame.
///
/// The input boundary delivers a press edge once per rendered frame, while the
/// publisher runs once per committed fixed tick — two or more per frame at the
/// baseline rate. A press must fire once, so the collector copies the frame's
/// flight edges here and the first fixed tick of the frame takes them; later
/// ticks of the same frame see none.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionCockpitActions {
    /// The frame's delivered flight edges, in boundary order.
    pub actions: Vec<Action>,
}

/// Copies the rendered frame's delivered flight edges for the fixed ticks.
///
/// The edges are **appended** to whatever is pending: the boundary delivers a
/// press once per rendered frame while the publisher runs once per committed
/// fixed tick, and a frame that ran no fixed tick must carry its press into
/// the next one rather than dropping it.
pub fn collect_mission_cockpit_actions(world: &mut World) {
    let delivered: Vec<Action> = world
        .get_resource::<PlatformInput>()
        .map(|platform| {
            platform
                .report()
                .delivered
                .iter()
                .copied()
                .filter(|action| matches!(action, Action::Flight(_)))
                .collect()
        })
        .unwrap_or_default();
    if delivered.is_empty() {
        return;
    }
    let mut pending = world
        .remove_resource::<MissionCockpitActions>()
        .unwrap_or_default();
    pending.actions.extend(delivered);
    world.insert_resource(pending);
}

/// The mission's hold-to-fire bookkeeping for the guns (M01-WINDOW-FIRE,
/// #1536).
///
/// The original fires the guns repeatedly while the trigger is held; the F22
/// input boundary delivers one press edge per hold, so the publisher mints the
/// repeats and this is the bookkeeping that spaces them.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct MissionGunHold {
    /// The tick the guns last fired on, `None` before the first shot.
    pub last: Option<Tick>,
}

/// Whether the guns key is currently held down.
///
/// The hold is the input session's own state ([`HeldEdge`]s, F22-B's
/// level-triggered device report), so a key held through a pause, a focus loss
/// or a control handover is *not* reported as held: the session released it
/// there, exactly as it released the press.
///
/// [`HeldEdge`]: cs_app::input::HeldEdge
#[must_use]
pub fn guns_key_held(world: &World) -> bool {
    world
        .get_resource::<PlatformInput>()
        .is_some_and(|platform| {
            platform
                .session()
                .collector()
                .devices()
                .held_edges()
                .iter()
                .any(|held| is_fire_guns(held.action()))
        })
}

/// Whether a hold-to-fire repeat is due at `at`.
///
/// The cadence is [`MISSION_TICKS_BETWEEN_SHOTS`] — the rate the loadout's
/// own gun records declare ([`designed_gun`]), which is also the rate the F27
/// step's per-mount cooldown enforces — so a repeat is minted exactly when the
/// last fired tick has aged out. A press edge does not consult this: a press
/// fires at once and is refused by name if the gun is still cooling down.
#[must_use]
pub fn gun_hold_due(world: &World, at: Tick) -> bool {
    world.get_resource::<MissionGunHold>().is_none_or(|hold| {
        hold.last.is_none_or(|last| {
            at.0.saturating_sub(last.0) >= u64::from(MISSION_TICKS_BETWEEN_SHOTS)
        })
    })
}

/// The pending rocket launches one tick's cockpit minted.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionOrdnanceOrders {
    /// The launch orders, in mint order.
    pub orders: Vec<OrdnanceOrder>,
}

/// Resource: what the session's committed ordnance ticks' rockets did.
///
/// The instrumentation the convoy-rocket path was missing: a launch that
/// left the rail, the selected target its engagement named, every fuse
/// trigger with the victim it actually reached and the node the hits were
/// routed to, what the damage authority applied or refused, and the
/// victim's integrity afterwards. A rocket that reached nothing is
/// reported as nothing — an expiry or a routing refusal — never as silent
/// phantom damage.
///
/// A rendered frame can run several ordnance ticks and a launch is ticks
/// apart from the detonation it causes, so the record accumulates: each
/// committed tick appends its launches, detonations, refusals and expiries
/// and moves `at` forward.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionOrdnanceReport {
    /// The last committed ordnance tick the report folded in, when a step ran.
    pub at: Option<Tick>,
    /// The launches ordered so far: the selected target and the node each
    /// engagement named, in mint order.
    pub launches: Vec<MissionOrdnanceLaunch>,
    /// Launch orders the ordnance step refused, verbatim.
    pub orders_refused: Vec<String>,
    /// The items the step accepted, in launch order.
    pub projectiles: Vec<ProjectileId>,
    /// One entry per detonation routed so far.
    pub detonations: Vec<MissionOrdnanceDetonation>,
    /// Triggered items whose routing refused, verbatim.
    pub routing_refused: Vec<String>,
    /// Items whose lifetime ended without a trigger.
    pub expired: Vec<ProjectileId>,
}

/// One minted rocket launch's designated engagement.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionOrdnanceLaunch {
    /// The selected target the engagement named.
    pub target: ActorId,
    /// The victim's own registered node the hits were aimed at.
    pub node: DamageNodeKey,
}

/// One detonation's victim-facing outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionOrdnanceDetonation {
    /// The item that detonated.
    pub projectile: ProjectileId,
    /// The fuse cause — `impact`, `proximity` or `timed`.
    pub trigger: &'static str,
    /// The actor the routed hits named, when the routing produced any.
    pub victim: Option<ActorId>,
    /// The damage node the hits named on the victim.
    pub node: Option<DamageNodeKey>,
    /// How many hits the damage authority applied.
    pub applied: usize,
    /// The hits the authority refused or could not resolve, each with its
    /// reason — an unknown node, an unknown actor or an unresolved pool.
    pub refused: Vec<String>,
    /// The victim node's integrity after the resolution, when the pool is
    /// resolved.
    pub remaining_integrity: Option<f64>,
    /// Whether the resolution recorded the victim's `Destroyed` lifecycle.
    pub destroyed: bool,
}

/// One rocket launch aimed at the player's selected target.
///
/// The designated victim is the selection the mission's targeting session
/// already holds (F30-C): the player picks the truck, the engagement names
/// it. The original's lock and launch gesture are unmeasured, so this is
/// labelled project design, not a reproduction — but a roster scan that
/// fires at the lowest-serial hostile is what made rockets hit the wrong
/// truck, and it is gone. The node the engagement names is the victim's own
/// registered one, read back from the damage authority — a bound ground
/// target's `targets.zrd` node (`t_truck01`), a scripted actor's
/// `airframe` — so `"hull"` can no longer be aimed at an actor that never
/// registered one. A selected target that is stale, unrostered, not
/// hostile or node-less is refused by name.
fn rocket_launch(
    world: &mut World,
    mission: &MissionWeapons,
    at: Tick,
    sequence: u32,
) -> Result<OrdnanceOrder, String> {
    let (target, hostile) = {
        let targeting = world
            .get_resource::<TargetingSession>()
            .ok_or_else(|| "no targeting session is installed".to_owned())?;
        let target = targeting
            .selection()
            .current()
            .ok_or_else(|| "no target is selected".to_owned())?;
        if target == mission.player {
            return Err("the selected target is the player".to_owned());
        }
        let store = targeting.store();
        let shooter = store
            .record(&mission.player)
            .ok_or_else(|| "the player is not rostered in the targeting store".to_owned())?;
        let record = store
            .record(&target)
            .ok_or_else(|| format!("the selected target {target} is not rostered"))?;
        (
            target,
            store.allegiance(&shooter.faction, &record.faction) == Some(Allegiance::Hostile),
        )
    };
    if !hostile {
        return Err(format!(
            "the selected target {target} is not hostile to the player"
        ));
    }
    let node = victim_damage_node(world, mission, target).ok_or_else(|| {
        format!("the selected target {target} registers no damage node this session")
    })?;
    Ok(OrdnanceOrder::Launch(OrdnanceLaunch {
        id: OrdnanceLaunchId {
            session: mission.session.get(),
            tick: at,
            producer: MISSION_ORDNANCE_PRODUCER,
            sequence,
        },
        shooter: mission.player,
        ordnance: mission.rocket.clone(),
        target: Some(target),
        engagement: OrdnanceEngagement {
            damage_target: target,
            node,
            status_recipient: StatusEffectTarget::actor_system(target, SystemKind::Propulsion),
        },
    }))
}

/// The damage node one victim actor's hits name: a bound ground target's
/// own `targets.zrd` node, a scripted actor's `airframe` frame node, else
/// the first node its registered graph declares. `None` when the actor
/// registers no graph this session — a stale or unbound victim — because
/// there is then no honest node to aim at.
fn victim_damage_node(
    world: &mut World,
    mission: &MissionWeapons,
    actor: ActorId,
) -> Option<DamageNodeKey> {
    {
        let mut query = world.query::<&MissionGroundTarget>();
        for target in query.iter(world) {
            if target.generation == mission.generation && target.damage_actor == actor {
                return DamageNodeKey::new(target.node.as_str()).ok();
            }
        }
    }
    let scripted = DamageNodeKey::new(SCRIPTED_DAMAGE_NODE).ok()?;
    let resolver = world.get_resource::<SessionDamageResolver>()?.resolver();
    let graph = resolver.graph(&actor)?;
    if graph.nodes().any(|node| *node.key() == scripted) {
        return Some(scripted);
    }
    graph.nodes().next().map(|node| node.key().clone())
}

/// The victim→node table one ordnance step routes fuse victims through:
/// every live damage-bound actor of the generation mapped to its own
/// registered node, so a proximity detonation's damage lands on the actor
/// the rocket actually reached rather than the one the launch designated.
fn mission_victim_nodes(
    world: &mut World,
    mission: &MissionWeapons,
) -> BTreeMap<ActorId, DamageNodeKey> {
    let actors: Vec<ActorId> = {
        let mut query = world.query::<&DamageActorBinding>();
        let mut actors: Vec<ActorId> = query
            .iter(world)
            .filter(|binding| {
                binding.generation == mission.generation && binding.actor != mission.player
            })
            .map(|binding| binding.actor)
            .collect();
        actors.sort();
        actors.dedup();
        actors
    };
    let mut nodes = BTreeMap::new();
    for actor in actors {
        if let Some(node) = victim_damage_node(world, mission, actor) {
            nodes.insert(actor, node);
        }
    }
    nodes
}

/// The eligible targets' swept paths for the rockets' proximity test.
///
/// One path per live damage-bound actor: its centre now and, reconstructed
/// from the body velocity the weapon path already uses, its centre at the
/// start of the tick.
fn rocket_target_paths(world: &mut World, mission: &MissionWeapons, dt_s: f64) -> Vec<TargetPath> {
    let targets: Vec<(Entity, ActorId)> = {
        let mut query = world.query::<(Entity, &DamageActorBinding)>();
        query
            .iter(world)
            .filter(|(_, binding)| {
                binding.generation == mission.generation && binding.actor != mission.player
            })
            .map(|(entity, binding)| (entity, binding.actor))
            .collect()
    };
    let mut paths = Vec::new();
    for (entity, actor) in targets {
        let Some(transform) = world.get::<GlobalTransform>(entity) else {
            continue;
        };
        let current = transform.translation();
        let velocity = world
            .get::<avian3d::prelude::LinearVelocity>(entity)
            .map(|velocity| velocity.0)
            .unwrap_or(Vec3::ZERO);
        let previous = current - velocity * dt_s as f32;
        let path = TargetPath::try_new(
            actor,
            [
                f64::from(previous.x),
                f64::from(previous.y),
                f64::from(previous.z),
            ],
            [
                f64::from(current.x),
                f64::from(current.y),
                f64::from(current.z),
            ],
        );
        if let Ok(path) = path {
            paths.push(path);
        }
    }
    paths
}

/// Drives one committed fixed tick of M01's rocket path.
///
/// Runs in `FixedPostUpdate` after the gun step and before
/// [`drive_damage_schedule`](crate::damage::drive_damage_schedule), so a
/// rocket that detonates this tick hands its resolved batch to the same tick's
/// damage consumers — colliders, debris and the score ledger.
pub fn drive_mission_ordnance(world: &mut World) {
    let Some(mission) = world.get_resource::<MissionWeapons>().cloned() else {
        return;
    };
    if world.get_resource::<MissionOrdnanceSession>().is_none() {
        return;
    }
    let Some(at) = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| Tick(ledger.ticks))
    else {
        return;
    };
    let Some(dt_s) = world
        .get_resource::<Time<Fixed>>()
        .map(|time| time.timestep().as_secs_f64())
    else {
        return;
    };
    let orders = world
        .remove_resource::<MissionOrdnanceOrders>()
        .map(|orders| orders.orders)
        .unwrap_or_default();
    let paths = rocket_target_paths(world, &mission, dt_s);
    let victim_nodes = mission_victim_nodes(world, &mission);
    let wind = mission_wind(world).velocity_m_s;
    let MissionOrdnanceSession(mut session) = world
        .remove_resource::<MissionOrdnanceSession>()
        .expect("the session was checked present above");
    let mut damage = world
        .remove_resource::<SessionDamageResolver>()
        .expect("the installed session leaves the shared damage authority");
    let tick = step_ordnance_session(
        world,
        &mut session,
        damage.resolver_mut(),
        &orders,
        &OrdnanceStep {
            at,
            dt_s,
            wind_velocity_m_s: wind,
            generation: mission.generation,
            guidance: None,
            targets: &paths,
            impacts: &[],
            blast_recipients: None,
            victim_nodes: Some(&victim_nodes),
        },
    );
    // The per-rocket truth the mission reports: fired, selected target,
    // detonation victim and node, applied or refused hits and the victim's
    // integrity afterwards.
    {
        let resolver = damage.resolver();
        let mut report = world
            .remove_resource::<MissionOrdnanceReport>()
            .unwrap_or_default();
        report.at = Some(tick.at);
        report
            .launches
            .extend(orders.iter().filter_map(|order| match order {
                OrdnanceOrder::Launch(launch) => Some(MissionOrdnanceLaunch {
                    target: launch.engagement.damage_target,
                    node: launch.engagement.node.clone(),
                }),
                _ => None,
            }));
        report
            .orders_refused
            .extend(tick.orders_refused.iter().map(ToString::to_string));
        report.projectiles.extend(tick.launched.iter().copied());
        report
            .routing_refused
            .extend(tick.routing_refused.iter().map(ToString::to_string));
        report.expired.extend(tick.expired.iter().copied());
        for detonation in &tick.detonations {
            let victim = detonation.hits.first().map(|hit| hit.target);
            let node = detonation.hits.first().map(|hit| hit.node.clone());
            let mut entry = MissionOrdnanceDetonation {
                projectile: detonation.projectile,
                trigger: detonation.trigger.label(),
                victim,
                node: node.clone(),
                applied: 0,
                refused: Vec::new(),
                remaining_integrity: None,
                destroyed: false,
            };
            if let Some(resolution) = &detonation.damage {
                for event in &resolution.events {
                    match &event.kind {
                        DamageEventKind::HitApplied { .. } => entry.applied += 1,
                        DamageEventKind::HitBlocked { reason, .. } => {
                            entry.refused.push(format!("blocked: {reason}"));
                        }
                        DamageEventKind::HitRefused { reason, .. } => {
                            entry.refused.push(format!("refused: {}", reason.label()));
                        }
                        DamageEventKind::Lifecycle {
                            kind: cs_sim::damage::LifecycleKind::Destroyed,
                            ..
                        } => entry.destroyed = true,
                        _ => {}
                    }
                }
            }
            if let Some(source) = &detonation.damage_refused {
                entry.refused.push(format!("batch refused: {source}"));
            }
            if let (Some(victim), Some(node)) = (victim, node.as_ref()) {
                entry.remaining_integrity = resolver.remaining_integrity(&victim, node);
            }
            report.detonations.push(entry);
        }
        world.insert_resource(report);
    }
    // A detonation's resolved batch goes to the same damage consumers the gun
    // path's rounds go to, so a rocket kill scores and wrecks exactly once.
    world.init_resource::<PendingDamageEvents>();
    {
        let mut pending = world.resource_mut::<PendingDamageEvents>();
        for detonation in &tick.detonations {
            if let Some(resolution) = &detonation.damage
                && !resolution.events.is_empty()
            {
                pending.push(detonation.hits.clone(), resolution.events.clone());
            }
        }
    }
    // The burst puffs' source events (PARITY2-DMG-FX-B, #1806): every item
    // that detonated, at the position the runtime recorded for it.
    {
        let mut bursts: Vec<PuffBurst> = tick
            .detonations
            .iter()
            .filter_map(|detonation| {
                session
                    .runtime()
                    .get(&detonation.projectile)
                    .map(|item| item.current())
            })
            .chain(tick.guidance_blasts.iter().map(|blast| blast.position))
            .map(|position| {
                let [x, y, z] = position.to_array();
                PuffBurst {
                    at: tick.at,
                    position: bevy::prelude::Vec3::new(x as f32, y as f32, z as f32),
                }
            })
            .collect();
        world.init_resource::<PendingOrdnanceBursts>();
        world
            .resource_mut::<PendingOrdnanceBursts>()
            .0
            .append(&mut bursts);
    }
    world.insert_resource(MissionOrdnanceSession(session));
    world.insert_resource(damage);
}

/// The command the fire-rockets binding carries.
pub const MISSION_FIRE_ROCKETS: FlightCommand = FlightCommand::FireSecondary;

/// Whether an action is a fire-guns edge.
#[must_use]
pub fn is_fire_guns(action: Action) -> bool {
    action == Action::Flight(FlightCommand::FirePrimary)
}

/// Whether an action is a fire-rockets edge.
#[must_use]
pub fn is_fire_rockets(action: Action) -> bool {
    action == Action::Flight(MISSION_FIRE_ROCKETS)
}

/// Opens the session on first use and re-opens it after a restart.
///
/// `MissionHost::restart` mints a fresh session generation and rebuilds the
/// player body; a session serving the old generation is foreign to the new
/// one. This compares the installed session to the host's served generation
/// and re-opens on a mismatch, so a restart never leaves the player holding a
/// dead generation's guns.
///
/// The arming pass [`crate::mission_ai`] runs in `FixedPreUpdate` calls this
/// too: the restart that replaces the session lands in `PostUpdate`, one
/// schedule stage after the pass — without the same staleness check ahead of
/// registering, the first tick after a restart would arm against the dead
/// generation and record a refusal the same tick resolves.
pub fn ensure_mission_weapons(world: &mut World) {
    let served = world
        .get_resource::<super::MissionHost>()
        .map(|host| host.served());
    let installed = world.get_resource::<MissionWeapons>().map(|m| m.session);
    if served.is_some() && served != installed {
        open_composed_mission_weapons(world);
    }
}

/// Composes the player's rig mounts' [`NodeVisualTransform`] poses from the
/// live body.
///
/// [`live_mount_transforms`](crate::weapons::live_mount_transforms) reads a
/// mount's `NodeVisualTransform` — the one pose owner F27 non-negotiable 2
/// names — and that component is **not** what Bevy's transform propagation
/// writes (it writes `GlobalTransform`). The rig's owner therefore composes
/// it here, from the body's live global transform and each mount's local
/// offset, so a muzzle tracks the animated airframe instead of a fixed origin.
///
/// Only the mission rig's own mount keys are touched: any other
/// [`MountPoseBinding`] in a composed mission belongs to another stage's
/// hierarchy and is left alone.
pub fn compose_mission_mount_poses(
    mut mounts: Query<(
        &MountPoseBinding,
        &ChildOf,
        &Transform,
        &mut NodeVisualTransform,
    )>,
    globals: Query<&GlobalTransform>,
) {
    for (binding, parent, local, mut node) in &mut mounts {
        if !RIG_MOUNTS
            .iter()
            .any(|rig| binding.mount == rig_mount_key(rig.mount))
        {
            continue;
        }
        if let Ok(parent_global) = globals.get(parent.parent()) {
            node.0 = parent_global.mul_transform(*local);
        }
    }
}

/// The damage-node key of one rig mount.
fn rig_mount_key(mount: &str) -> DamageNodeKey {
    DamageNodeKey::new(mount).expect("the rig mount key is valid")
}

/// Installs M01's weapon session on a composed mission once the player exists.
///
/// The composition already installed [`crate::weapons::WeaponSchedulePlugin`]
/// and [`crate::damage::DamageSchedulePlugin`]; this leaves the session
/// resources those schedules serve, and the rig the mounts hang on. Refusals
/// are named in [`MissionWeaponReport::refused`] rather than panicking.
pub fn open_composed_mission_weapons(world: &mut World) {
    let Some(session) = world
        .get_resource::<super::MissionHost>()
        .map(|host| host.served())
    else {
        return;
    };
    world.init_resource::<crate::scene::SceneGenerations>();
    let generation = world
        .get_resource::<crate::scene::SceneGenerations>()
        .map(crate::scene::SceneGenerations::latest)
        .unwrap_or(SceneGeneration(1));
    let rate = TickRate::new(crate::mission_world_actors::SESSION_TICKS_PER_SECOND)
        .expect("the composition's tick rate is nonzero");
    let counts = world
        .get_resource::<super::MissionStage>()
        .and_then(|stage| stage.weapon_counts);
    match open_mission_weapons_with(session, rate, generation, counts) {
        Ok(sessions) => {
            let sessions_player = sessions.mission.player();
            let _ = sessions.install(world);
            let player = world
                .get_resource::<super::MissionComposition>()
                .map(|composition| composition.player);
            match player {
                Some(player) => {
                    let _ = attach_player_rig(world, player);
                    let actor = sessions_player;
                    match super::player_victim::attach_player_victim(
                        world, player, actor, generation,
                    ) {
                        Ok(victim) => world.insert_resource(victim),
                        Err(reason) => {
                            if let Some(mut report) =
                                world.get_resource_mut::<MissionWeaponReport>()
                            {
                                report.refused = Some(reason);
                            }
                        }
                    }
                }
                None => {
                    if let Some(mut report) = world.get_resource_mut::<MissionWeaponReport>() {
                        report.refused =
                            Some("the player body had not spawned when the rig opened".to_owned());
                    }
                }
            }
        }
        Err(error) => {
            let mut report = world
                .remove_resource::<MissionWeaponReport>()
                .unwrap_or_default();
            report.refused = Some(error.to_string());
            world.insert_resource(report);
        }
    }
}

/// The designed hull value every admitted actor starts with.
///
/// The score ledger refuses an actor nobody admitted; the pool value itself
/// is the engine's designed network hull, not a measurement.
const MISSION_DESIGNED_HULL: u32 = 400;

// The designed loadout's own numbers. Every one is project design.
const MISSION_CALIBER: &str = "designed .50 caliber";
const MISSION_AMMUNITION_KEY: &str = "m01-play-weapons.designed-ammo";
const MISSION_GUN_NOSE_KEY: &str = "m01-play-weapons.designed-nose-gun";
const MISSION_GUN_WING_LEFT_KEY: &str = "m01-play-weapons.designed-left-gun";
const MISSION_GUN_WING_RIGHT_KEY: &str = "m01-play-weapons.designed-right-gun";
const MISSION_LOADOUT_KEY: &str = "m01-play-weapons.designed-loadout";
/// Designed rounds the mission's guns start with, per its own designed
/// loadout. The value is project design, not a measurement, and is kept here
/// rather than borrowed from a synthetic fixture constant.
const MISSION_STARTING_ROUNDS: u64 = 250;
const MISSION_TARGET_RULES_SUBJECT: &str = "m01-play-weapons.designed-target-rules";
const MISSION_MUZZLE_EFFECT: &str = "m01-play-weapons.designed-muzzle";
const MISSION_ROCKET_KEY: &str = "m01-play-weapons.designed-rocket";
const MISSION_ROCKET_VISUAL: &str = "m01-play-weapons.designed-rocket-visual";
const MISSION_ROCKET_SOUND: &str = "m01-play-weapons.designed-rocket-sound";
/// The damage node a launched rocket's hits name on its victim.
///
/// Designed: the original's part names per airframe are not recovered, so a
/// rocket aims at one named part and the resolver refuses an unknown node by
/// name rather than guessing a different one.
pub const MISSION_TARGET_NODE: &str = "hull";
/// Designed ticks between shots.
const MISSION_TICKS_BETWEEN_SHOTS: u32 = 4;
/// Designed muzzle velocity, in metres per second.
const MISSION_MUZZLE_VELOCITY_MPS: f64 = 640.0;
/// Designed round lifetime, in ticks.
const MISSION_LIFETIME_TICKS: u64 = 90;
/// Designed spread cone half-angle, in radians.
const MISSION_SPREAD_HALF_ANGLE_RAD: f64 = 0.004;
/// Designed armor-channel damage per round.
const MISSION_ARMOR_DAMAGE: f64 = 12.0;
/// Designed internal-channel damage per round.
const MISSION_INTERNAL_DAMAGE: f64 = 12.0;
/// Designed threat window, in ticks.
const MISSION_THREAT_WINDOW_TICKS: u64 = 120;
/// Designed rocket launch speed, in metres per second.
const MISSION_ROCKET_LAUNCH_SPEED_MPS: f64 = 210.0;
/// Designed rocket stack capacity, in units.
const MISSION_ROCKET_CAPACITY: u64 = 6;
/// Designed rocket unit mass, in kilograms.
const MISSION_ROCKET_MASS_KG: f64 = 2.4;
/// Designed ticks before the rocket's fuse arms.
const MISSION_ROCKET_ARM_TICKS: u64 = 3;
/// Designed proximity trigger radius, in metres.
const MISSION_ROCKET_TRIGGER_RADIUS_M: f64 = 12.0;
/// Designed rocket lifetime, in ticks.
const MISSION_ROCKET_LIFETIME_TICKS: u64 = 240;
/// Designed rocket armor-channel damage.
const MISSION_ROCKET_ARMOR_DAMAGE: f64 = 28.0;
/// Designed rocket internal-channel damage.
const MISSION_ROCKET_INTERNAL_DAMAGE: f64 = 20.0;

/// Wires M01's weapons into a composed mission.
///
/// The composition already installs [`crate::weapons::WeaponSchedulePlugin`]
/// and [`crate::damage::DamageSchedulePlugin`]; this plugin adds the one
/// session those schedules serve (once the player body exists) and the two
/// per-tick entries the schedulers do not own:
///
/// * [`publish_mission_weapon_input`] runs before
///   [`drive_weapon_session`](crate::weapons::drive_weapon_session) so the
///   record it pushes serves the tick the ledger has just committed;
/// * [`drive_mission_ordnance`] runs after the gun step and before
///   [`drive_damage_schedule`](crate::damage::drive_damage_schedule), so a
///   rocket's batch reaches the same tick's damage consumers.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionWeaponsPlugin;

impl Plugin for MissionWeaponsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MissionWeaponReport>()
            .init_resource::<MissionOrdnanceOrders>()
            .init_resource::<MissionCockpitActions>()
            .init_resource::<MissionGunHold>()
            .add_systems(PostStartup, open_composed_mission_weapons)
            .add_systems(
                PreUpdate,
                // The collector must read the report the pump of *this* frame
                // wrote. Both live in `PreUpdate` and neither state survives
                // without the other being read, so the order is stated rather
                // than left to the executor's tie-break (M01-WINDOW-FIRE,
                // #1536).
                collect_mission_cockpit_actions.after(crate::input::platform::pump_platform_input),
            )
            .add_systems(
                PreUpdate,
                // The measured targeting keys (PARITY2-M01-TARGET-INDICATORS,
                // #1727) mint their edges behind the delivered ones, in the
                // same frame the pump read.
                super::targeting_input::mint_mission_target_keys
                    .after(collect_mission_cockpit_actions)
                    .run_if(not(
                        resource_exists::<super::original_keys::MissionControls>,
                    )),
            )
            .add_systems(
                PreUpdate,
                // The original layout (PARITY2-ORIGINAL-KEYBINDINGS, #1733)
                // mints every modifier-sensitive key of its table instead.
                super::original_keys::mint_original_chord_keys
                    .after(collect_mission_cockpit_actions)
                    .run_if(resource_exists::<super::original_keys::MissionControls>),
            )
            .add_systems(
                FixedPostUpdate,
                publish_mission_weapon_input
                    .after(compose_mission_mount_poses)
                    .after(avian3d::prelude::PhysicsSystems::StepSimulation)
                    .before(crate::weapons::drive_weapon_session)
                    .before(crate::damage::drive_damage_schedule),
            )
            .add_systems(
                FixedPostUpdate,
                super::player_victim::compose_player_hull_box
                    .after(avian3d::prelude::PhysicsSystems::StepSimulation)
                    .before(crate::weapons::drive_weapon_session),
            )
            // The flak cues' one-shot producer counter (FLAK-C4/C5): the
            // turret step plays `snd_enemyflak`/`snd_missile_flak` through it.
            .init_resource::<super::flak_sound::FlakSoundSequences>()
            // The player's bullet-hit ricochets (#1865).
            .init_resource::<super::hit_sound::HitSoundState>()
            .add_systems(
                FixedPostUpdate,
                super::hit_sound::cue_player_hits
                    .after(crate::weapons::drive_weapon_session)
                    .before(crate::damage::drive_damage_schedule),
            )
            .add_systems(
                FixedPostUpdate,
                (
                    super::turret_fire::build_mission_turrets,
                    super::turret_fire::step_mission_turrets
                        .run_if(resource_exists::<super::turret_fire::MissionTurretFire>),
                )
                    .chain()
                    .after(avian3d::prelude::PhysicsSystems::StepSimulation)
                    .before(compose_mission_mount_poses),
            )
            .add_systems(
                FixedPostUpdate,
                compose_mission_mount_poses
                    .after(avian3d::prelude::PhysicsSystems::StepSimulation)
                    .before(crate::weapons::drive_weapon_session),
            )
            .add_systems(
                FixedPostUpdate,
                super::debrief::tally_player_gun
                    .after(crate::weapons::drive_weapon_session)
                    .before(crate::damage::drive_damage_schedule),
            )
            .add_systems(
                FixedPostUpdate,
                drive_mission_ordnance
                    .after(crate::weapons::drive_weapon_session)
                    .before(crate::damage::drive_damage_schedule),
            );
    }
}
