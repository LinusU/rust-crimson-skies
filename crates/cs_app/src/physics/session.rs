//! The production physics session: producer/consumer wiring (F23-C).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-C`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! F23-A built the verified schedule adapter and F23-B the body/contact
//! runtime; this module is the object a game loop actually owns. Its role
//! matches [`InputSession`](crate::input::InputSession) on the input side:
//! one type holds the world, gates every producer call
//! ([`spawn`](PhysicsSession::spawn), [`submit`](PhysicsSession::submit),
//! [`set_mode`](PhysicsSession::set_mode),
//! [`despawn`](PhysicsSession::despawn)) and hands the consumer one
//! [`SessionFrame`] per render frame — the classified contact reports and
//! spawn corrections of *every* tick that frame ran, in tick order, plus the
//! request counters, so an authoritative event can never be lost between
//! the second and third tick of a multi-tick frame.
//!
//! Producer wiring, in one place:
//!
//! * **Body creation** goes through [`spawn_body`]. A spawned body that
//!   needs swept detection and is already moving carries
//!   [`SpawnPreflight`] — F23-B measured a first-tick tunneling hole in
//!   Avian's swept AABB, and the preflight shape-cast
//!   (`docs/findings/2026-09-30-f23-b-*.md`, limitation 1) is how a spawn
//!   inside one tick of an obstacle reaches the contact instead of passing
//!   through.
//! * **Control-mode transitions** go through [`set_body_mode`]: they write
//!   the `RigidBody` kind and nothing dynamic, so the kinematic → dynamic
//!   aircraft release continues from the scripted pose and velocity with no
//!   discontinuity (the stage's AC03 minimum scenario).
//! * **Force requests** are the one-tick [`ForceRequests`] queue; the
//!   adapter drains it before the step and reports dropped/woken requests
//!   in the frame outcome.
//! * **Presentation** reads [`Transform`] while the simulation reads
//!   [`Position`]: spawned simulated bodies carry `TransformInterpolation`,
//!   so the transform the renderer sees eases between fixed poses within a
//!   frame while the authoritative pose stays tick-quantized.
//!
//! Teardown is explicit — [`teardown`](PhysicsSession::teardown) drops the
//! world, every producer call then returns
//! [`PhysicsSessionError::Inactive`], and
//! [`restart`](PhysicsSession::restart) builds a fresh world with no stale
//! reports, requests or active pairs (the same contract
//! [`crate::run::run_synthetic`] documents: a second run cannot inherit the
//! first run's state).
//!
//! The session is also the single-clock authority F23-A deferred (limitation
//! 3): it installs `Time<Fixed>` at the declared rate, drives the world by
//! [`TimeUpdateStrategy::ManualDuration`], and is the only thing that
//! advances it.
//!
//! **Designed wiring, not original data.** The pump shape, the preflight
//! clamp rule and the release semantics are declared project behavior; what
//! the original's loop looked like is unknown.

use core::time::Duration;
use std::fmt;

use avian3d::prelude::{AngularVelocity, Gravity, LinearVelocity, Position, SubstepCount};
use bevy::{
    prelude::{App, Entity, Transform, Vec3, World},
    time::{Real, Time, TimeUpdateStrategy},
};

use super::adapter::{ForceRequest, ForceRequests, PhysicsAdapterPlugin, PhysicsTickLedger};
use super::body::{BodyError, BodyMode, BodySpec, BodyTransitionError, set_body_mode, spawn_body};
use super::contacts::{ContactReport, ContactReports, PhysicsBodiesPlugin};
use super::fixture::PhysicsSample;
use super::preflight::{SpawnPreflight, SpawnPreflightEvent, SpawnPreflightLog};

/// What one render frame's pump produced.
///
/// `reports` is the consumer-facing list of authoritative contact events —
/// every tick's classified contacts, in tick order, so a contact can never
/// be silently overwritten by the next tick of the same frame.
/// `spawn_events` is the same channel for the first-tick preflight
/// corrections. The request counters are the tick ledger's deltas across
/// the frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionFrame {
    /// The pump index of this frame.
    pub frame: u64,
    /// How many fixed ticks the frame ran.
    pub ticks_ran: u64,
    /// The tick the session is at after the frame.
    pub tick: u64,
    /// The authoritative contact events the frame produced.
    pub reports: Vec<ContactReport>,
    /// The spawn preflight corrections the frame resolved.
    pub spawn_events: Vec<SpawnPreflightEvent>,
    /// Force requests applied during the frame.
    pub applied_requests: u64,
    /// Force requests that reached no dynamic body during the frame.
    pub dropped_requests: u64,
    /// Force requests whose target had to be woken during the frame.
    pub woken_requests: u64,
}

/// The result of one [`PhysicsSession::spawn`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnOutcome {
    /// The spawned body.
    pub entity: Entity,
    /// Whether the first tick will preflight the spawn (the body needs
    /// swept detection and was already moving).
    pub preflight_pending: bool,
}

/// Why a session operation failed.
#[derive(Debug)]
pub enum PhysicsSessionError {
    /// The session was torn down. Call [`PhysicsSession::restart`] to re-arm
    /// it.
    Inactive,
    /// The spawn spec was rejected at the boundary; nothing spawned.
    Body(BodyError),
    /// The mode transition could not be applied to that entity.
    Transition(BodyTransitionError),
    /// The entity is not part of this world (despawned, or never was).
    UnknownBody(Entity),
}

impl fmt::Display for PhysicsSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inactive => write!(
                f,
                "the physics session is torn down; restart it before pumping frames"
            ),
            Self::Body(error) => write!(f, "{error}"),
            Self::Transition(error) => write!(f, "{error}"),
            Self::UnknownBody(entity) => write!(f, "{entity} is not a body in this world"),
        }
    }
}

impl std::error::Error for PhysicsSessionError {}

impl From<BodyError> for PhysicsSessionError {
    fn from(error: BodyError) -> Self {
        Self::Body(error)
    }
}

impl From<BodyTransitionError> for PhysicsSessionError {
    fn from(error: BodyTransitionError) -> Self {
        Self::Transition(error)
    }
}

/// Builds a [`PhysicsSession`].
pub struct PhysicsSessionBuilder {
    fixed_hz: u32,
    gravity: Vec3,
}

impl PhysicsSessionBuilder {
    /// Overrides the fixed simulation rate. Defaults to
    /// [`BASELINE_FIXED_HZ`](super::BASELINE_FIXED_HZ).
    pub fn fixed_hz(mut self, fixed_hz: u32) -> Self {
        self.fixed_hz = fixed_hz;
        self
    }

    /// Overrides Avian's global gravity. Defaults to zero: the flight model
    /// (F24) decides how gravity is applied, and until then the session
    /// keeps the fixture's honest "no hand-written gravity force and no
    /// Avian gravity" baseline (`docs/contracts/FLIGHT-PHYSICS.md`).
    pub fn gravity(mut self, gravity: Vec3) -> Self {
        self.gravity = gravity;
        self
    }

    /// Builds the session.
    ///
    /// # Panics
    ///
    /// Panics when `fixed_hz` is zero, the same boundary
    /// [`PhysicsAdapterPlugin`] enforces.
    pub fn build(self) -> PhysicsSession {
        assert!(
            self.fixed_hz > 0,
            "PhysicsSession fixed_hz must be greater than zero"
        );
        PhysicsSession {
            app: Some(Self::app(self.fixed_hz, self.gravity)),
            fixed_hz: self.fixed_hz,
            gravity: self.gravity,
            frames: 0,
            tick_seen: 0,
        }
    }

    fn app(fixed_hz: u32, gravity: Vec3) -> App {
        // `PhysicsPlugins::default()` needs Bevy's asset stack under Avian's
        // pinned feature set, so the session builds its world through the one
        // shared headless composition rather than spelling the tuple out.
        let mut app = crate::asset_stack::headless_app();
        app.add_plugins(PhysicsAdapterPlugin::new(fixed_hz));
        app.add_plugins(PhysicsBodiesPlugin);
        app.insert_resource(SubstepCount(1));
        app.insert_resource(Gravity(gravity));

        // The session is the single-clock authority (F23-A limitation 3):
        // `pump_frame` is the only thing that feeds the manual duration, so
        // nothing else can advance `Time<Fixed>`.
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));

        // Seed the real clock baseline so the first counted update produces
        // the full manual delta and one fixed step
        // (`docs/findings/2026-09-23-t334-first-frame-fixed-step.md`).
        let startup = app.world().resource::<Time<Real>>().startup();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .update_with_instant(startup);

        app.finish();
        app.cleanup();
        app
    }
}

/// The production owner of one physics world.
///
/// One session is one world: a gameplay-side producer submits bodies, force
/// requests and mode transitions, [`pump_frame`](Self::pump_frame) advances
/// the fixed clock by the render frame's span, and the consumer reads the
/// frame's [`SessionFrame`] and the presentation poses.
pub struct PhysicsSession {
    app: Option<App>,
    fixed_hz: u32,
    gravity: Vec3,
    frames: u64,
    tick_seen: u64,
}

impl PhysicsSession {
    /// Starts building a session. The declared rate defaults to
    /// [`BASELINE_FIXED_HZ`](super::BASELINE_FIXED_HZ).
    pub fn builder() -> PhysicsSessionBuilder {
        PhysicsSessionBuilder {
            fixed_hz: super::BASELINE_FIXED_HZ,
            gravity: Vec3::ZERO,
        }
    }

    /// A session at the declared rate with the default (zero) gravity.
    pub fn new(fixed_hz: u32) -> PhysicsSession {
        Self::builder().fixed_hz(fixed_hz).build()
    }

    /// Whether the session still owns a world.
    pub fn is_active(&self) -> bool {
        self.app.is_some()
    }

    /// The declared fixed rate.
    pub fn fixed_hz(&self) -> u32 {
        self.fixed_hz
    }

    /// The declared fixed timestep.
    pub fn timestep(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.fixed_hz as f64)
    }

    /// The tick the world's ledger is on.
    pub fn tick(&self) -> u64 {
        self.app
            .as_ref()
            .map_or(0, |app| app.world().resource::<PhysicsTickLedger>().ticks)
    }

    /// The adapter's tick ledger.
    pub fn ledger(&self) -> Option<PhysicsTickLedger> {
        self.app
            .as_ref()
            .map(|app| *app.world().resource::<PhysicsTickLedger>())
    }

    /// Read-only access to the world, while the session is active.
    pub fn world(&self) -> Option<&World> {
        self.app.as_ref().map(App::world)
    }

    /// Mutable access to the world, while the session is active — for the
    /// rare operations the typed API does not cover.
    pub fn world_mut(&mut self) -> Option<&mut World> {
        self.app.as_mut().map(App::world_mut)
    }

    fn require_active_mut(&mut self) -> Result<&mut App, PhysicsSessionError> {
        self.app.as_mut().ok_or(PhysicsSessionError::Inactive)
    }

    /// Spawns one body through the production creation path.
    ///
    /// A body on a swept-detection layer that is already moving additionally
    /// carries [`SpawnPreflight`], so its first tick shape-casts one tick of
    /// travel and clamps onto a solid hit rather than tunneling (the F23-B
    /// first-tick hole). Validation errors propagate before anything
    /// spawns; a torn-down session refuses.
    pub fn spawn(&mut self, spec: &BodySpec) -> Result<SpawnOutcome, PhysicsSessionError> {
        let app = self.require_active_mut()?;
        let entity = spawn_body(app.world_mut(), spec)?;
        let preflight_pending = spec.layer.requires_continuous_detection()
            && Vec3::from_array(spec.linear_velocity_m_s).length() > 0.0;
        if preflight_pending {
            app.world_mut().entity_mut(entity).insert(SpawnPreflight);
        }
        Ok(SpawnOutcome {
            entity,
            preflight_pending,
        })
    }

    /// Despawns a body of this session.
    ///
    /// # Errors
    ///
    /// [`PhysicsSessionError::UnknownBody`] when the entity is not in this
    /// world; [`PhysicsSessionError::Inactive`] when the session is torn
    /// down.
    pub fn despawn(&mut self, entity: Entity) -> Result<(), PhysicsSessionError> {
        let app = self.require_active_mut()?;
        let target = app
            .world_mut()
            .get_entity_mut(entity)
            .map_err(|_| PhysicsSessionError::UnknownBody(entity))?;
        target.despawn();
        Ok(())
    }

    /// Switches `entity` to `mode` without a pose or velocity discontinuity.
    ///
    /// The kinematic → dynamic release (AC03) is this call with
    /// [`BodyMode::Dynamic`]: the body continues from the pose and velocity
    /// its scripted trajectory ended on, including a static body's stored
    /// velocity — the transition never rewrites state behind the caller's
    /// back (designed rule; the gameplay zero-release question is the
    /// caller's to ask through [`Self::world_mut`] or a future tuning task).
    pub fn set_mode(
        &mut self,
        entity: Entity,
        mode: BodyMode,
    ) -> Result<BodyMode, PhysicsSessionError> {
        let app = self.require_active_mut()?;
        Ok(set_body_mode(app.world_mut(), entity, mode)?)
    }

    /// Queues a one-tick force/torque request for the next fixed tick.
    pub fn submit(&mut self, request: ForceRequest) -> Result<(), PhysicsSessionError> {
        let app = self.require_active_mut()?;
        app.world_mut()
            .resource_mut::<ForceRequests>()
            .submit(request);
        Ok(())
    }

    /// Reads a body's authoritative pose and velocities.
    pub fn pose(&self, entity: Entity) -> Option<PhysicsSample> {
        let app = self.app.as_ref()?;
        let body = app.world().get_entity(entity).ok()?;
        let position = body.get::<Position>()?;
        let velocity = body.get::<LinearVelocity>()?;
        let angular = body.get::<AngularVelocity>()?;
        Some(PhysicsSample {
            position_m: position.0.to_array(),
            linear_velocity_m_s: velocity.0.to_array(),
            angular_velocity_rad_s: angular.0.to_array(),
        })
    }

    /// Reads the presentation transform of a body — the eased `Transform`
    /// the renderer consumes, not the tick-quantized `Position`.
    pub fn presentation_translation(&self, entity: Entity) -> Option<Vec3> {
        let app = self.app.as_ref()?;
        app.world()
            .get::<Transform>(entity)
            .map(|transform| transform.translation)
    }

    /// Advances the world by one render frame's span and returns what the
    /// frame produced.
    ///
    /// The frame is pumped one fixed tick per world update, so the
    /// per-tick contact batch is drained between ticks and a report from an
    /// early tick of the frame can never be overwritten by a later one; the
    /// fractional remainder still updates the eased presentation transform.
    ///
    /// # Errors
    ///
    /// [`PhysicsSessionError::Inactive`] when the session was torn down.
    pub fn pump_frame(&mut self, frame: Duration) -> Result<SessionFrame, PhysicsSessionError> {
        let timestep = self.timestep();
        let whole_ticks = (frame.as_secs_f64() / timestep.as_secs_f64()).floor() as u64;
        let app = self.app.as_mut().ok_or(PhysicsSessionError::Inactive)?;

        let ledger_before = *app.world().resource::<PhysicsTickLedger>();
        let mut reports = Vec::new();

        for _ in 0..whole_ticks {
            *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
                TimeUpdateStrategy::ManualDuration(timestep);
            app.update();
            drain_tick(app, &mut self.tick_seen, &mut reports);
        }
        let rest = frame
            .checked_sub(timestep.saturating_mul(u32::try_from(whole_ticks).unwrap_or(u32::MAX)))
            .unwrap_or_default();
        if !rest.is_zero() {
            *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
                TimeUpdateStrategy::ManualDuration(rest);
            app.update();
            drain_tick(app, &mut self.tick_seen, &mut reports);
        }

        let ledger_after = *app.world().resource::<PhysicsTickLedger>();
        let spawn_events = app.world_mut().resource_mut::<SpawnPreflightLog>().take();

        self.frames += 1;
        Ok(SessionFrame {
            frame: self.frames,
            ticks_ran: ledger_after.ticks - ledger_before.ticks,
            tick: ledger_after.ticks,
            reports,
            spawn_events,
            applied_requests: ledger_after.total_applied_requests
                - ledger_before.total_applied_requests,
            dropped_requests: ledger_after.total_dropped_requests
                - ledger_before.total_dropped_requests,
            woken_requests: ledger_after.total_woken_requests - ledger_before.total_woken_requests,
        })
    }

    /// Advances the world by exactly `ticks` fixed steps — the convenience
    /// shape of [`pump_frame`](Self::pump_frame) for a whole-tick frame.
    pub fn step(&mut self, ticks: u64) -> Result<SessionFrame, PhysicsSessionError> {
        self.pump_frame(
            self.timestep()
                .saturating_mul(u32::try_from(ticks).unwrap_or(u32::MAX)),
        )
    }

    /// Tears the session down: drops the world. Every operation returns
    /// [`PhysicsSessionError::Inactive`] until [`restart`](Self::restart).
    pub fn teardown(&mut self) {
        self.app = None;
    }

    /// Rebuilds the world exactly as built: same rate, same gravity, tick 0,
    /// no reports, no queued requests and no retained active pairs.
    pub fn restart(&mut self) {
        self.app = Some(PhysicsSessionBuilder::app(self.fixed_hz, self.gravity));
        self.frames = 0;
        self.tick_seen = 0;
    }
}

/// Appends the reports of a just-finished tick to the frame's list.
/// A frame update that ran no fixed tick (`ledger.ticks` unchanged) leaves
/// the last batch in place, so it is never drained twice.
fn drain_tick(app: &App, tick_seen: &mut u64, reports: &mut Vec<ContactReport>) {
    let tick = app.world().resource::<PhysicsTickLedger>().ticks;
    if tick == *tick_seen {
        return;
    }
    *tick_seen = tick;
    reports.extend_from_slice(app.world().resource::<ContactReports>().reports());
}
