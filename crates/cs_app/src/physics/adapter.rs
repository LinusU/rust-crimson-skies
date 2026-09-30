//! The verified Avian fixed-step schedule adapter (F23-A).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! This is the typed boundary between gameplay force/torque decisions and the
//! pinned Bevy 0.19 / Avian3d 0.7 integrator. It does exactly three things:
//!
//! * declare the fixed simulation rate and install `Time<Fixed>` with that
//!   timestep ([`PhysicsAdapterPlugin`]);
//! * drain a one-tick [`ForceRequests`] queue into Avian's real force
//!   accumulator in `FixedPostUpdate` **before** `PhysicsSystems::Prepare`, so
//!   a request is applied to exactly the tick that submitted it and never
//!   lingers across ticks, waking a sleeping target first so that tick still
//!   integrates it;
//! * record one [`PhysicsTickLedger`] entry per fixed tick boundary and one
//!   per integration, so a test can assert one integration per tick and a
//!   constant (never variable) dt, and so a request that reached no dynamic
//!   body is counted rather than silently lost.
//!
//! The schedule hooks are the ones measured in F00-B
//! (`docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`):
//! `PhysicsPlugins::default()` runs physics in `FixedPostUpdate`, chained
//! `PhysicsSystems::{First, Prepare, StepSimulation, Writeback, Last}`.
//!
//! **Designed baseline, not original data.** 120 Hz is the spec's designed
//! starting rate (`### F23-A`), and the force queue is project design. The
//! original game's tick rate and force application order are unknown until the
//! compatibility/convergence stages measure them (F23-D, F16-D).

use core::time::Duration;
use std::fmt;

use avian3d::prelude::{Forces, PhysicsSystems, RigidBody, Sleeping, WriteRigidBodyForces};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{
        App, Commands, Entity, FixedPostUpdate, Plugin, Query, Res, ResMut, Resource, Vec3, With,
    },
    time::{Fixed, Time},
};

/// The designed fixed simulation rate, in Hz (`### F23-A`: "initially 120 Hz").
pub const BASELINE_FIXED_HZ: u32 = 120;

/// A non-finite or otherwise unusable force request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceRequestError {
    /// A named field contained NaN or infinity.
    NonFinite { field: &'static str },
}

impl fmt::Display for ForceRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
        }
    }
}

impl std::error::Error for ForceRequestError {}

/// One world-space force and torque for a single body, for exactly one fixed
/// tick.
///
/// Values are SI (`force_n` in newtons, `torque_nm` in newton-metres) in the
/// canonical world frame (`docs/contracts/FLIGHT-PHYSICS.md`, "Coordinate
/// convention"). Construct with [`ForceRequest::new`], which rejects
/// non-finite input at the boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForceRequest {
    body: Entity,
    force_n: [f32; 3],
    torque_nm: [f32; 3],
}

impl ForceRequest {
    /// Builds a request, rejecting any non-finite component.
    pub fn new(
        body: Entity,
        force_n: [f32; 3],
        torque_nm: [f32; 3],
    ) -> Result<Self, ForceRequestError> {
        validate_components(FORCE_FIELDS, force_n)?;
        validate_components(TORQUE_FIELDS, torque_nm)?;
        Ok(Self {
            body,
            force_n,
            torque_nm,
        })
    }

    /// The target body.
    pub fn body(self) -> Entity {
        self.body
    }

    /// The world-space force, in newtons.
    pub fn force_n(self) -> [f32; 3] {
        self.force_n
    }

    /// The world-space torque, in newton-metres.
    pub fn torque_nm(self) -> [f32; 3] {
        self.torque_nm
    }
}

const FORCE_FIELDS: [&str; 3] = ["force_n[0]", "force_n[1]", "force_n[2]"];
const TORQUE_FIELDS: [&str; 3] = ["torque_nm[0]", "torque_nm[1]", "torque_nm[2]"];

fn validate_components(
    fields: [&'static str; 3],
    values: [f32; 3],
) -> Result<(), ForceRequestError> {
    for (field, value) in fields.into_iter().zip(values) {
        if !value.is_finite() {
            return Err(ForceRequestError::NonFinite { field });
        }
    }
    Ok(())
}

/// The one-tick queue the adapter drains.
///
/// A request lives for the tick it was submitted in and no longer: the adapter
/// drains the queue before the integration step and clears it, so a released
/// force never silently persists (contract: "Do not add both a hand-written
/// gravity force and Avian global gravity", generalised to every owner of a
/// body's force).
#[derive(Resource, Default)]
pub struct ForceRequests(Vec<ForceRequest>);

impl ForceRequests {
    /// Queues one request. It is applied exactly once, in the next fixed tick.
    pub fn submit(&mut self, request: ForceRequest) {
        self.0.push(request);
    }

    /// Number of requests waiting to be applied.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no request is queued.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Removes and returns every queued request.
    pub fn take(&mut self) -> Vec<ForceRequest> {
        core::mem::take(&mut self.0)
    }
}

/// One record of the adapter's view of the fixed schedule.
///
/// `ticks` counts fixed tick boundaries before the integration step and
/// `integrations` counts the physics steps observed after
/// `PhysicsSystems::StepSimulation`. Comparing them is the structural
/// "one integration per declared tick" assertion; `timestep_s` is sampled from
/// Avian's own `Time<Fixed>` so a hidden variable dt would show up as a
/// changing value.
///
/// A request that reached no dynamic body is never silently lost:
/// `dropped_requests` counts the ones this tick that were not applied (a
/// despawned entity, a static or kinematic body, a disabled body), and
/// `woken_requests` counts the ones this tick whose target was asleep and
/// therefore had to be woken before they could apply. The wake counters are
/// per *request*: two requests for the same sleeping body count twice, and
/// the single wake they share serves both.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicsTickLedger {
    /// Fixed tick boundaries crossed.
    pub ticks: u64,
    /// Integration steps observed after the physics step set.
    pub integrations: u64,
    /// The fixed timestep of the most recent tick, in seconds.
    pub timestep_s: f32,
    /// Requests applied during the most recent tick.
    pub applied_requests: u64,
    /// Requests applied since the adapter was built.
    pub total_applied_requests: u64,
    /// Requests that reached no dynamic body during the most recent tick.
    pub dropped_requests: u64,
    /// Requests that reached no dynamic body since the adapter was built.
    pub total_dropped_requests: u64,
    /// Requests whose target was sleeping during the most recent tick.
    pub woken_requests: u64,
    /// Requests whose target was sleeping since the adapter was built.
    pub total_woken_requests: u64,
}

/// Installs the fixed-rate clock and the force/tick adapter systems.
///
/// The plugin pins the fixed schedule the F00-B probe measured: force requests
/// are drained in `FixedPostUpdate` before `PhysicsSystems::Prepare`, and the
/// integration counter observes `PhysicsSystems::StepSimulation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicsAdapterPlugin {
    fixed_hz: u32,
}

impl PhysicsAdapterPlugin {
    /// A plugin running at `fixed_hz` fixed steps per second.
    ///
    /// # Panics
    ///
    /// Building the app panics when `fixed_hz` is zero: a zero rate would
    /// divide the timestep by zero instead of failing loudly at the boundary.
    pub const fn new(fixed_hz: u32) -> Self {
        Self { fixed_hz }
    }

    /// The declared fixed rate.
    pub const fn fixed_hz(&self) -> u32 {
        self.fixed_hz
    }

    /// The fixed timestep as a [`Duration`].
    pub fn timestep(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.fixed_hz as f64)
    }

    /// The fixed timestep in seconds, as the f32 the ECS sees.
    pub fn timestep_seconds(&self) -> f32 {
        self.timestep().as_secs_f32()
    }
}

impl Default for PhysicsAdapterPlugin {
    fn default() -> Self {
        Self::new(BASELINE_FIXED_HZ)
    }
}

impl Plugin for PhysicsAdapterPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            self.fixed_hz > 0,
            "PhysicsAdapterPlugin fixed_hz must be greater than zero"
        );

        app.insert_resource(Time::<Fixed>::from_seconds(self.timestep().as_secs_f64()));
        app.init_resource::<ForceRequests>();
        app.init_resource::<PhysicsTickLedger>();

        app.add_systems(
            FixedPostUpdate,
            (
                record_tick_boundary,
                wake_requested_bodies,
                apply_force_requests,
            )
                .chain()
                .before(PhysicsSystems::Prepare),
        );
        app.add_systems(
            FixedPostUpdate,
            record_integration.after(PhysicsSystems::StepSimulation),
        );
    }
}

/// Opens the tick: records the timestep and resets the per-tick counters.
fn record_tick_boundary(time: Res<Time<Fixed>>, mut ledger: ResMut<PhysicsTickLedger>) {
    ledger.ticks += 1;
    ledger.timestep_s = time.timestep().as_secs_f32();
    ledger.applied_requests = 0;
    ledger.dropped_requests = 0;
    ledger.woken_requests = 0;
}

/// Wakes every sleeping body that has a request waiting for this tick.
///
/// A sleeping body has no `SolverBody`, so applying a force to it alone would
/// park the acceleration in `VelocityIntegrationData` where the sleeping step
/// neither applies it nor clears it: the request would surface a tick later,
/// scaled a second time by the timestep (F23-A limitation 1, measured in
/// `docs/findings/2026-09-30-f23-b-body-creation-forces-sweeps-and-transitions.md`).
/// Removing `Sleeping` here rebuilds the solver body at the automatic sync
/// point the `.chain()` places before [`apply_force_requests`], so the request
/// applies to exactly the tick that submitted it, awake.
///
/// Bodies that are merely missing, static, kinematic or disabled are left
/// alone: enabling a disabled body or moving a static one is a gameplay
/// decision, not something a force queue may do behind the caller's back.
fn wake_requested_bodies(
    requests: Res<ForceRequests>,
    sleeping: Query<(), With<Sleeping>>,
    mut commands: Commands,
    mut ledger: ResMut<PhysicsTickLedger>,
) {
    for request in requests.0.iter() {
        let body = request.body();
        if sleeping.get(body).is_err() {
            continue;
        }
        commands.entity(body).remove::<Sleeping>();
        ledger.woken_requests += 1;
        ledger.total_woken_requests += 1;
    }
}

/// Drains the request queue into Avian's accumulator before integration.
///
/// Only a dynamic body can take a force, so a request for a kinematic or
/// static body — or for an entity that no longer exists — is dropped and
/// counted instead of being applied to something that would ignore it or
/// silently half-apply it. The queue is a gameplay-facing boundary: a stale
/// generation must not crash a tick, and it must not look like a hit either.
fn apply_force_requests(
    mut requests: ResMut<ForceRequests>,
    mut bodies: Query<(Forces, &RigidBody)>,
    mut ledger: ResMut<PhysicsTickLedger>,
) {
    for request in requests.take() {
        let Ok((mut forces, rigid_body)) = bodies.get_mut(request.body()) else {
            ledger.dropped_requests += 1;
            ledger.total_dropped_requests += 1;
            continue;
        };
        if !rigid_body.is_dynamic() {
            ledger.dropped_requests += 1;
            ledger.total_dropped_requests += 1;
            continue;
        }
        forces.apply_force(Vec3::from_array(request.force_n()));
        forces.apply_torque(Vec3::from_array(request.torque_nm()));
        ledger.applied_requests += 1;
        ledger.total_applied_requests += 1;
    }
}

/// Records one integration after the physics step set.
fn record_integration(mut ledger: ResMut<PhysicsTickLedger>) {
    ledger.integrations += 1;
}
