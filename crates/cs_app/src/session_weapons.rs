//! The production session host's publisher for the scheduled weapon step
//! (F27-C-PUBLISH, task #1299).
//!
//! [`WeaponSchedulePlugin`](crate::weapons::WeaponSchedulePlugin) steps the
//! merged weapon path once per committed fixed tick, but it consumes
//! [`WeaponTickInputs`] it does not produce: a session that publishes nothing
//! reports `missing_inputs` and never fires. This module is the publisher for
//! a [`PhysicsSession`](crate::physics::PhysicsSession) host that is not the
//! mission composition (which publishes through
//! `mission_session::MissionWeaponsPlugin`):
//!
//! * [`SessionWeaponHost`] is the host's configuration — the session's own
//!   [`EnvironmentSession`] (the wind authority) and the local seat's firing
//!   channel. A world without it publishes nothing, so a world that composes
//!   its own publisher is never served twice.
//! * [`install_session_weapons`] installs one generation's [`WeaponSession`],
//!   [`SessionDamageResolver`] and [`TargetingSession`] together, replacing the
//!   previous generation's, and refuses a set that disagrees on the generation.
//! * [`publish_session_weapon_input`] builds the tick's [`WeaponTickInput`]:
//!   the environment's measured wind (an unmeasured wind is published as
//!   `None`, never still air), the authored [`WeaponOrder`]s queued with
//!   [`SessionWeaponOrders`], and the local seat's delivered actions.
//! * [`collect_session_cockpit_actions`] copies the flight actions the F22
//!   input boundary delivered on the frame into [`SessionCockpitActions`] for
//!   the fixed ticks to take.

use bevy::app::{App, Plugin, PreUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{FixedPostUpdate, World};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::input::Action;

use crate::environment::EnvironmentSession;
use crate::input::platform::{PlatformInput, pump_platform_input};
use crate::physics::PhysicsTickLedger;
use crate::targeting::TargetingSession;
use crate::weapons::{
    CockpitInput, SessionDamageResolver, WeaponOrder, WeaponSession, WeaponTickInput,
    WeaponTickInputs, drive_weapon_session,
};

/// The local seat's firing channel: who it flies and the producer serial the
/// session allocated to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionLocalSeat {
    /// The actor the seat flies.
    pub shooter: ActorId,
    /// The session-allocated producer serial the minted fire intents carry.
    pub producer: u32,
}

/// Resource: the host's publishing configuration. Its presence is what turns
/// the publisher on for a world.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct SessionWeaponHost {
    /// The session's environment, the wind authority.
    pub environment: EnvironmentSession,
    /// The local seat, when one flies this session.
    pub local_seat: Option<SessionLocalSeat>,
}

/// Resource: orders the session authored (network replays, scripted orders,
/// bank switches), published with the next tick's record.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SessionWeaponOrders {
    orders: Vec<WeaponOrder>,
}

impl SessionWeaponOrders {
    /// Queues one order for the next published tick.
    pub fn push(&mut self, order: WeaponOrder) {
        self.orders.push(order);
    }

    /// How many orders are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// Whether no order is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }
}

/// Resource: the delivered flight actions waiting for a fixed tick.
///
/// The boundary delivers per rendered frame and the publisher runs per
/// committed tick, so the first tick after a delivery takes the actions and
/// later ticks see none — a press fires once.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SessionCockpitActions {
    actions: Vec<Action>,
}

impl SessionCockpitActions {
    /// Appends actions the input boundary delivered.
    pub fn extend(&mut self, delivered: impl IntoIterator<Item = Action>) {
        self.actions.extend(delivered);
    }

    /// The actions waiting.
    #[must_use]
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }
}

/// Resource: what the last publication did, for diagnostics and tests.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SessionWeaponPublishReport {
    /// The tick last published for.
    pub at: Option<Tick>,
    /// Whether that record carried a measured wind.
    pub wind_measured: bool,
    /// Orders and cockpit actions the record carried.
    pub orders: usize,
    /// Cockpit actions the record carried.
    pub actions: usize,
    /// Why the last run published nothing, when it did not.
    pub skipped: Option<SessionWeaponSkip>,
}

/// Why a run published no record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionWeaponSkip {
    /// No [`WeaponSession`] is installed: nothing to serve.
    NoSession,
    /// The physics ledger is absent.
    NoTimeline,
}

/// Why [`install_session_weapons`] refused a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionInstallRefusal {
    /// The three authorities do not share one session generation.
    GenerationMismatch {
        /// The weapon session's generation.
        weapons: u64,
        /// The damage resolver's generation.
        damage: u64,
        /// The targeting session's generation.
        targeting: u64,
    },
}

impl std::fmt::Display for SessionInstallRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GenerationMismatch {
                weapons,
                damage,
                targeting,
            } => write!(
                f,
                "weapon session {weapons}, damage resolver {damage} and targeting {targeting} \
                 are not one generation"
            ),
        }
    }
}

impl std::error::Error for SessionInstallRefusal {}

/// Installs one generation's weapon authorities, replacing the previous
/// generation's. Pending orders and delivered actions of the old generation
/// are dropped; the schedule's own teardown releases the old rounds and
/// pending records.
///
/// # Errors
///
/// [`SessionInstallRefusal::GenerationMismatch`] when the three disagree;
/// nothing is installed.
pub fn install_session_weapons(
    world: &mut World,
    weapons: WeaponSession,
    damage: SessionDamageResolver,
    targeting: TargetingSession,
) -> Result<(), SessionInstallRefusal> {
    let (w, d, t) = (
        weapons.session(),
        damage.session(),
        targeting.session().get(),
    );
    if w != d || w != t {
        return Err(SessionInstallRefusal::GenerationMismatch {
            weapons: w,
            damage: d,
            targeting: t,
        });
    }
    world.insert_resource(weapons);
    world.insert_resource(damage);
    world.insert_resource(targeting);
    world.insert_resource(SessionWeaponOrders::default());
    world.insert_resource(SessionCockpitActions::default());
    Ok(())
}

/// Copies the frame's delivered flight actions for the fixed ticks.
pub fn collect_session_cockpit_actions(world: &mut World) {
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
    world
        .get_resource_or_insert_with(SessionCockpitActions::default)
        .extend(delivered);
}

/// Publishes the committed tick's [`WeaponTickInput`].
///
/// Runs before [`drive_weapon_session`] so the record serves the tick the
/// ledger has just committed. A world without [`SessionWeaponHost`] is not
/// this publisher's and is left alone.
pub fn publish_session_weapon_input(world: &mut World) {
    let Some(host) = world.get_resource::<SessionWeaponHost>() else {
        return;
    };
    let wind_velocity_m_s = host.environment.wind().ok().map(|wind| wind.velocity_m_s());
    let seat = host.local_seat;
    let mut report = SessionWeaponPublishReport::default();
    let Some(session) = world
        .get_resource::<WeaponSession>()
        .map(WeaponSession::session)
    else {
        report.skipped = Some(SessionWeaponSkip::NoSession);
        world.insert_resource(report);
        return;
    };
    let Some(at) = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| Tick(ledger.ticks))
    else {
        report.skipped = Some(SessionWeaponSkip::NoTimeline);
        world.insert_resource(report);
        return;
    };
    let orders = world
        .get_resource_mut::<SessionWeaponOrders>()
        .map(|mut pending| std::mem::take(&mut pending.orders))
        .unwrap_or_default();
    let delivered = world
        .get_resource_mut::<SessionCockpitActions>()
        .map(|mut pending| std::mem::take(&mut pending.actions))
        .unwrap_or_default();
    let cockpit = seat.map(|seat| CockpitInput {
        shooter: seat.shooter,
        producer: seat.producer,
        actions: delivered,
    });
    report.at = Some(at);
    report.wind_measured = wind_velocity_m_s.is_some();
    report.orders = orders.len();
    report.actions = cockpit.as_ref().map_or(0, |c| c.actions.len());
    world
        .get_resource_or_insert_with(WeaponTickInputs::default)
        .push(WeaponTickInput {
            session,
            at,
            wind_velocity_m_s,
            orders,
            cockpit,
        });
    world.insert_resource(report);
}

/// Installs the publisher: the frame's action collector and the per-tick
/// publication ahead of the weapon schedule.
#[derive(Clone, Copy, Debug, Default)]
pub struct SessionWeaponPublishPlugin;

impl Plugin for SessionWeaponPublishPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SessionWeaponPublishReport>()
            .add_systems(
                PreUpdate,
                collect_session_cockpit_actions.after(pump_platform_input),
            )
            .add_systems(
                FixedPostUpdate,
                publish_session_weapon_input
                    .after(avian3d::prelude::PhysicsSystems::StepSimulation)
                    .before(drive_weapon_session),
            );
    }
}
