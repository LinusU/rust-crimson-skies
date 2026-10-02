//! The resting rule: a body in contact with static world geometry comes to rest
//! (F23 follow-up #428).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`.
//! Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Resolves the
//! measurement F18-C recorded in
//! `docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md`
//! ("a body resting against a collider is not released when the collider
//! moves"); the full resolution, with every measurement behind the two
//! declared thresholds here, is
//! `docs/findings/2026-10-02-t428-contact-restitution-rule-for-a-resting-body.md`.
//!
//! # The measurement that makes this rule necessary
//!
//! Measured on the pinned pair (`bevy 0.19.1`, `avian3d 0.7.0`,
//! `SubstepCount(2)`, 120 Hz, `Gravity::ZERO`), a 250 kg box arriving at a
//! static wall:
//!
//! | impact speed | velocity after the contact resolves | as a fraction |
//! | --- | --- | --- |
//! | 5 m/s | 0.156 m/s | 3.1 % |
//! | 15 m/s | 0.610 m/s | 4.1 % |
//! | 30 m/s | 0.981 m/s | 3.3 % |
//! | 60 m/s | 1.759 m/s | 2.9 % |
//! | 100 m/s | 2.830 m/s | 2.8 % |
//!
//! Three properties of that residual are what this rule is built on, and each
//! was measured rather than assumed:
//!
//! 1. **It is not restitution.** Binding `Restitution::new(0.0)` explicitly on
//!    the wall reproduced the numbers above bit for bit — the engine's default
//!    is already 0.0, and zero restitution is the *correct* rule here: a body
//!    must not rebound off a door panel. Binding `Friction::new(0.0)` made the
//!    residual *larger* (1.037 m/s), so friction is damping part of it rather
//!    than causing it.
//! 2. **It is not the substep budget.** 1, 2, 4 and 8 solver substeps measured
//!    0.933, 0.981, 1.219 and 0.820 m/s at the same 30 m/s impact — noise
//!    around one value, not a trend.
//! 3. **It never decays.** The contact resolves in a *single* tick (measured:
//!    `|Δv| = 30.39 m/s` on the resolving tick, then `|Δv| = 0` or `6e-8` —
//!    float noise — on every tick after it, for 960 ticks), and the velocity
//!    that survives is *constant to float precision* from then on.
//!
//! Property 3 is the whole rule. A velocity that does not change while the body
//! is touching geometry is not being produced by anything: with zero gravity
//! and no drag there is no other force in this world that could be producing
//! it, so what is left is the contact solver's own approximation error, and it
//! carries the body away from the wall it just hit at a constant rate for as
//! long as the world runs. Measured: the identical residual, to the last
//! printed digit, whether the wall is displaced, despawned, or left exactly
//! where it was — which is why F18-C could not attribute its 1.49 m/s creep to
//! the overlay that moved the panel.
//!
//! # What the rule is, precisely
//!
//! [`retire_contact_residual`] runs in `FixedPostUpdate` **after**
//! [`PhysicsSystems::StepSimulation`] — the same slot the contact reporter
//! reads, so it sees this tick's contacts and this tick's post-step velocity.
//! For every **dynamic** body in a *touching* contact with **immovable** world
//! geometry:
//!
//! * the per-tick change in its linear velocity is compared against
//!   [`RESTING_STILL_EPSILON_M_S`]. A change at or below it is "the contact is
//!   no longer acting on this body"; a change above it resets the run, which is
//!   what a body being pushed, dragged, thrust along or bounced does;
//! * [`RESTING_STILL_TICKS`] consecutive unchanged ticks retire the body's
//!   linear *and* angular velocity to zero and mark it [`RestingContact`];
//! * a marked body stops being at rest when its velocity keeps **changing** for
//!   [`RESTING_RELEASE_TICKS`] consecutive ticks — measured to be the signal
//!   that separates gameplay from the contact's own solver bias — or
//!   immediately if it is touching nothing (free motion is never a contact's
//!   tail). Otherwise it is held at zero. The rule holds a body only while
//!   nothing is acting on it, so it cannot fight gameplay, and it does not
//!   decelerate a body on its way out of resting, because a body that is moving
//!   is released rather than written to.
//!
//! It writes **velocity only**. The pose is never touched: a crashed body stops
//! where the contact left it, which is the whole point of the rule and the
//! reason it is not a teleport. Nothing here is conditional on an overlay —
//! the rule cannot see one, and a body that is at rest before a door opens
//! stays at rest after it, which is the fact F18-C's stage needed pinned.
//!
//! # Designed rule, not original data
//!
//! Whether the 2000 PC original settled a body against geometry this way, let
//! it sleep, or left it drifting is **unknown**; the thresholds below are
//! measured from this engine, not read from the original. A world with gravity
//! and drag would need a different rule (see the finding's limitations), and
//! the module names that rather than pretending the rule is universal.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::{
    AngularVelocity, Collisions, ContactPairFlags, LinearVelocity, PhysicsSystems, RigidBody,
    Sensor, Sleeping,
};
use bevy::{
    ecs::{
        schedule::IntoScheduleConfigs,
        system::{Query, SystemParam},
    },
    prelude::{
        App, Commands, Component, Entity, FixedPostUpdate, Plugin, Res, ResMut, Resource, Vec3,
        With,
    },
};

use super::adapter::PhysicsTickLedger;

/// The per-tick change in linear velocity below which the contact is no longer
/// acting on a body.
///
/// **Frozen from the measurement above.** The residual the contact leaves is
/// *exactly* constant: over 960 ticks after the resolving tick the measured
/// per-tick change was `0.0`, once `6e-8` of float noise, and never more. One
/// millimetre per second per tick is five orders of magnitude above that noise
/// and, at 120 Hz, an acceleration of 0.12 m/s² — below any force the game
/// applies (a flight model's forces are orders of magnitude larger, and even a
/// single millinewton on a 250 kg body is `4e-6 m/s²` per tick), so it
/// separates "nothing is acting on this body" from "something is" with a very
/// wide margin on both sides.
pub const RESTING_STILL_EPSILON_M_S: f32 = 1.0e-3;

/// Consecutive unchanged ticks the rule needs before it retires a body.
///
/// **Frozen from the measurement above, and the measurement picks four over
/// both neighbours.** The contact's velocity is constant from the tick it
/// resolves, so this count is not about waiting for the residual to settle — it
/// is about *where* the body is left, and the numbers move with it. Measured on
/// the pinned pair, resting against a wall (a body's leading face at rest,
/// against `CONTACT_FACE_TOLERANCE_M` = 1 mm):
///
/// | ticks | 30 m/s impact | 100 m/s impact |
/// | --- | --- | --- |
/// | 1 | 3.4 mm clear | 10.4 mm clear |
/// | 2 | 6.6 mm clear | 25.5 mm clear |
/// | **4** | **12.8 mm clear** | **55.7 mm clear** |
/// | 6 | 19.1 mm clear | **1.76 m past the wall** |
/// | 8 | 25.4 mm clear | **1.76 m past the wall** |
///
/// **The upper bound is what the measurement forbids**, and it is a hard one: at
/// 100 m/s a body covers 0.83 m per tick against the wall's 0.4 m thickness, so
/// by six ticks it is still moving when the count completes and the rule
/// retires it a metre and three-quarters *past* the wall it struck. Six and
/// above are wrong, not merely eager, and a body "at rest" 1.76 m inside
/// geometry is the same render/collision mismatch this whole class of work is
/// about.
///
/// **The lower bound is a judgement, and this says so rather than inventing a
/// measurement for it.** One, two and four all leave the body beside the wall at
/// both speeds, differing only in how much clearance — which is Avian's own
/// contact tolerance behaving as designed, not a property of this count — so
/// nothing in the measured behaviour separates them. Four is chosen because the
/// contact's transient is a single tick and a count of four is the smallest
/// that leaves three ticks of margin against it at 120 Hz, which is 1/30 s of
/// simulated time: short enough that "the body stopped" is not a perceptible
/// delay, long enough that no single solver impulse is mistaken for a settled
/// state. A reviewer who prefers one or two has the measurement to check that
/// choice against, and the pinned test's fast arm will tell them immediately if
/// the count is too *long*.
pub const RESTING_STILL_TICKS: u32 = 4;

/// Consecutive ticks of *changing* velocity that take a marked body out of rest.
///
/// **Frozen from the measurement the release signal needs, and the signal itself
/// is measured rather than assumed.** The obvious release test — "is the body's
/// speed above the epsilon?" — **cannot work here**: a genuinely resting body in
/// contact is handed a *non-zero* speed every tick by the solver's own
/// soft-constraint bias, 0.0638 m/s on the depot's door panel. A speed test
/// would release every resting body on its next tick and hand back exactly the
/// drift this rule exists to remove. (Measured: with a speed-based release the
/// depot probe cycled mark → drift → re-mark, and the `retired` counter reached
/// three for one body.)
///
/// Velocity **change** separates the two instead, because the bias *decays to a
/// constant* rather than staying constant: on the depot panel it falls
/// 0.0638 → 0.0036 m/s over **nineteen** consecutive ticks as the soft
/// constraint bleeds the initial overlap off, and only then stops changing at
/// all. Anything actually driving the body, by contrast, changes its velocity
/// every tick by `a·dt` — a 3 kN force on a 250 kg body at 120 Hz is `0.1 m/s`
/// per tick, a hundred times the epsilon.
///
/// **Nineteen is the number the constant is built from**, measured on the depot
/// panel across impacts from 5 to 100 m/s (worst run 19; the plain wall fixture
/// is 1, because its overlap is shallow). Twenty-four is that plus five ticks of
/// margin, because the length of the decay is a property of how deeply the pair
/// overlaps and this repository has not measured every geometry that could
/// overlap more deeply. The cost of the margin is measured and named: a marked
/// body that gameplay starts driving is released after 24 ticks, 1/5 s of
/// simulated time at 120 Hz.
pub const RESTING_RELEASE_TICKS: u32 = 24;

/// This dynamic body is at rest: the resting rule retired the velocity its
/// contact left behind, and nothing has moved it since.
///
/// The rule's own answer to "is this body still where it stopped", recorded as
/// a component rather than left to be re-derived from a velocity. It is
/// inserted when [`retire_contact_residual`] retires a body's drift, and
/// withdrawn by the same measurement that inserted it, read the other way round
/// — see the module docs.
///
/// It deliberately does **not** mean "this body is touching geometry right
/// now", and the measured reason is worth stating: on the pinned pair the
/// residual carries the body about 1.3 cm *away* from the surface it struck
/// before the rule stops it, which is further than Avian's own contact
/// tolerance, so the pair stops being a contact at all. A marker that came and
/// went with that pair would report nothing; what is durable is the fact that
/// the body's velocity was retired and nothing has since given it one.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestingContact {
    /// The tick ledger's count when the rule marked the body, or 0 when no
    /// ledger is installed.
    pub tick: u64,
}

impl RestingContact {
    /// The marker this rule inserts, stamped with `tick`.
    #[must_use]
    pub const fn at(tick: u64) -> Self {
        Self { tick }
    }
}

/// What the resting rule has done, counted.
///
/// Counters rather than a list: the rule's output is a *state* on the body
/// ([`RestingContact`]), and what a caller cannot read off the world is how
/// many residuals were retired — the number that says whether this world's
/// contacts are leaking velocity at all.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestingReports {
    /// Residuals retired: bodies whose non-zero linear or angular velocity this
    /// rule has zeroed.
    pub retired: u64,
    /// Bodies that stopped being at rest because a velocity above
    /// [`RESTING_STILL_EPSILON_M_S`] came back and stayed.
    pub released: u64,
    /// How many bodies carry [`RestingContact`] right now.
    pub resting: u64,
}

impl RestingReports {
    /// Whether the rule has retired anything this run.
    #[must_use]
    pub const fn retired_anything(&self) -> bool {
        self.retired > 0
    }
}

/// What the rule remembers about one body between ticks.
///
/// Private because it is bookkeeping, not a record: the state a caller reads is
/// [`RestingContact`], and everything here exists to reach the decision that
/// inserts it. It outlives the contact on purpose — see [`RestingContact`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct BodyState {
    /// Consecutive ticks whose velocity changed by at most
    /// [`RESTING_STILL_EPSILON_M_S`]. The run that retires the body.
    still_ticks: u32,
    /// Consecutive ticks whose velocity changed by *more* than
    /// [`RESTING_STILL_EPSILON_M_S`] since it was marked. The run that releases
    /// it. See [`RESTING_RELEASE_TICKS`] for why the signal is change and not
    /// speed.
    moving_ticks: u32,
    /// The velocity that run started from, and the one the release run is
    /// measured against.
    reference: Vec3,
    /// Whether the body currently carries [`RestingContact`].
    marked: bool,
}

/// Per-body bookkeeping, so the rule can compare a body against its own
/// previous tick.
///
/// Pruned every tick against the dynamic bodies that still exist, so a
/// despawned body cannot leak an entry: the same retention rule the contact
/// reporter applies to its active-pair set. An entry is *not* pruned for losing
/// its contact, because the marker's lifetime is longer than the contact's (see
/// [`RestingContact`]), and the map is therefore bounded by the number of live
/// bodies rather than by the number of contacts.
#[derive(Resource, Default, Debug)]
struct RestingState {
    bodies: BTreeMap<Entity, BodyState>,
}

/// Whether a collider is solid geometry rather than a trigger volume.
///
/// A [`Sensor`] is *a reported overlap and never an obstacle* (the contract's
/// own rule, and `cs_sim::collision::classify_contact`'s sensor/solid
/// boundary), so a body crossing a trigger volume is not resting against
/// anything. A bodyless collider is a standalone one — in this repository only
/// trigger volumes are spawned that way — and is treated as immovable, because
/// nothing can move it.
fn is_immovable(body: Option<Entity>, kinds: &Query<&RigidBody>) -> bool {
    match body {
        None => true,
        Some(entity) => kinds
            .get(entity)
            .is_ok_and(|kind| *kind == RigidBody::Static),
    }
}

/// The dynamic bodies touching immovable geometry this tick, in stable order.
///
/// One pass over the contact pairs rather than a query per body: the set is
/// small (a body resting against geometry is a rare, exceptional state) and a
/// `BTreeSet` keeps the pass deterministic, so two runs of the same world
/// retire bodies in the same order.
///
/// The [`ContactPairFlags::TOUCHING`] filter is load-bearing and the measured
/// case is narrow. A pair exists as soon as two colliders' AABBs overlap, which
/// is *before* the shapes touch: on the pinned pair, a 250 kg body flying at
/// 30 m/s into a wall is already in a non-touching pair at tick 9, at
/// `x = -0.500`, and the pair only starts touching on tick 10. Without the
/// filter a body would be treated as resting against a wall it had not reached
/// yet. The overlap window is short at these speeds because the pair is created
/// by the broad phase's inflated AABB and consumed by the next narrow phase,
/// which is why the flag is easy to leave out and hard to notice — the body it
/// would misjudge is one that is about to arrive.
fn touching_static_geometry(
    collisions: &Collisions,
    kinds: &Query<&RigidBody>,
    sensors: &Query<(), With<Sensor>>,
) -> BTreeSet<Entity> {
    let mut bodies = BTreeSet::new();
    for pair in collisions.iter() {
        if !pair.flags.contains(ContactPairFlags::TOUCHING) {
            continue;
        }
        // A sensor on either side makes the overlap a report, not a contact.
        if sensors.contains(pair.collider1) || sensors.contains(pair.collider2) {
            continue;
        }
        // Either side may be the geometry, so each is tried in both roles.
        for (immovable, other) in [(pair.body1, pair.body2), (pair.body2, pair.body1)] {
            if !is_immovable(immovable, kinds) {
                continue;
            }
            if let Some(body) = other
                && kinds
                    .get(body)
                    .is_ok_and(|kind| *kind == RigidBody::Dynamic)
            {
                bodies.insert(body);
            }
        }
    }
    bodies
}

/// One resting candidate: the two velocity components the rule is allowed to
/// write, plus the entity the bookkeeping is keyed by.
type RestingBody = (
    Entity,
    &'static mut LinearVelocity,
    &'static mut AngularVelocity,
);

/// The read-only half of the pass's queries, bundled so the system's own
/// parameter list stays inside the crate's arity limit.
///
/// `kinds` and `sensors` answer "is this immovable geometry" and `sleeping`
/// answers "has the engine already settled this body"; they are only ever read,
/// so grouping them says so in one place rather than at each use.
#[derive(SystemParam)]
struct QuietBodies<'w, 's> {
    kinds: Query<'w, 's, &'static RigidBody>,
    sensors: Query<'w, 's, (), With<Sensor>>,
    sleeping: Query<'w, 's, (), With<Sleeping>>,
}

/// Retires the contact residual of every body resting against world geometry.
///
/// The rule, in the order it runs:
///
/// 1. **Read.** The touching pairs name the bodies this tick, and the query
///    names the dynamic bodies that still exist. A body that has been
///    despawned loses its bookkeeping, so the map cannot outgrow the world.
/// 2. **Decide.** For a body the rule has not marked, a per-tick velocity change
///    at or below [`RESTING_STILL_EPSILON_M_S`] extends its unchanged run and
///    any larger change resets it, so anything acting on the body restarts the
///    count. A body it *has* marked is at rest for as long as it is still
///    touching the geometry that stopped it and its velocity is not *changing*;
///    it stops being at rest when either the contact goes or
///    [`RESTING_RELEASE_TICKS`] consecutive ticks of changing velocity say
///    something else is driving it.
/// 3. **Change.** [`RESTING_STILL_TICKS`] unchanged ticks retire the body's
///    linear and angular velocity and mark it [`RestingContact`]; a marked body
///    is held at zero on every tick it is still touching, which is what absorbs
///    the contact's own solver tail.
///
/// The mark's lifetime is the contact's, read from the other side. A marked
/// body that is no longer touching anything is unmarked and left alone, because
/// nothing is holding it there either — and that is the answer to the question
/// this task exists for: a body that came to rest against a panel that then
/// moves away is free, because it was never held by the panel in the first
/// place. See [`RestingContact`] for why the mark cannot instead be tied to the
/// body's velocity.
///
/// A marked body is **not** held against gameplay, and this is the half of the
/// rule that is easy to get wrong. Holding unconditionally is not a stronger
/// statement of "at rest"; it is a body the game can no longer move. Measured
/// with the hold unconditional: a 3 kN force into a wall left the marked body's
/// speed at exactly zero for sixty ticks, and a steady 0.5 m/s velocity write
/// never took effect at all, because the hold rewrote it every tick. The
/// release signal is therefore velocity **change**, not speed — see
/// [`RESTING_RELEASE_TICKS`], where the measurement that forces that choice is
/// recorded.
///
/// A **sleeping** body is never written to. Avian puts a body to sleep after a
/// settled period, so it is at rest by the engine's own statement, and writing
/// a velocity component is the one thing that would wake it again — the rule
/// must not keep a body awake that the engine has settled.
fn retire_contact_residual(
    collisions: Collisions,
    mut bodies: Query<RestingBody, With<RigidBody>>,
    quiet: QuietBodies,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut state: ResMut<RestingState>,
    mut reports: ResMut<RestingReports>,
    mut commands: Commands,
) {
    let tick = ledger.map_or(0, |ledger| ledger.ticks);
    let touching = touching_static_geometry(&collisions, &quiet.kinds, &quiet.sensors);

    // A despawned body must not leave its bookkeeping behind, and must not be
    // left counted: `resting` is a claim about bodies that exist, so an entry
    // pruned here is one fewer claim. Without the counter half of this, a body
    // that despawned while resting would be reported as at rest forever — a
    // count that only ever grows is not a count a caller can act on.
    let live: BTreeSet<Entity> = bodies.iter().map(|(entity, ..)| entity).collect();
    let dropped: Vec<Entity> = state
        .bodies
        .keys()
        .filter(|entity| !live.contains(entity))
        .copied()
        .collect();
    for entity in &dropped {
        if state.bodies.remove(entity).is_some_and(|body| body.marked) {
            reports.resting = reports.resting.saturating_sub(1);
        }
    }

    for (entity, mut linear, mut angular) in &mut bodies {
        let asleep = quiet.sleeping.contains(entity);
        let touching_now = touching.contains(&entity);

        // 2/3. A marked body: held at rest, released when it leaves the geometry
        // or when something starts driving it.
        if let Some(entry) = state.bodies.get_mut(&entity).filter(|e| e.marked) {
            if !touching_now {
                eprintln!("DEBUG release-on-contact-loss");
                // Nothing is touching it, so nothing is holding it there either.
                // The mark goes and the rule never touches the body again until
                // it is in contact and settles afresh.
                entry.marked = false;
                entry.still_ticks = 0;
                entry.moving_ticks = 0;
                entry.reference = linear.0;
                reports.released += 1;
                reports.resting = reports.resting.saturating_sub(1);
                commands.entity(entity).remove::<RestingContact>();
                continue;
            }

            // The contact is still there. Something may still be driving this
            // body, and the rule must not hold a body gameplay is acting on —
            // measured: with the hold unconditional, a 3 kN force into a wall
            // left a marked body's speed at exactly zero for sixty ticks, and a
            // steady tangential velocity wrote of 0.5 m/s never took effect at
            // all. So the release test is velocity **change**, not speed:
            // a resting body's post-step velocity is the solver's own constant
            // bias and does not change tick to tick, while anything driving the
            // body changes it every tick. See [`RESTING_RELEASE_TICKS`].
            let change = (linear.0 - entry.reference).length();
            if change > RESTING_STILL_EPSILON_M_S {
                entry.moving_ticks += 1;
            } else {
                entry.moving_ticks = 0;
            }
            entry.reference = linear.0;
            if entry.moving_ticks >= RESTING_RELEASE_TICKS {
                eprintln!("DEBUG release-on-drive moving_ticks={}", entry.moving_ticks);
                entry.marked = false;
                entry.still_ticks = 0;
                entry.moving_ticks = 0;
                entry.reference = linear.0;
                reports.released += 1;
                reports.resting = reports.resting.saturating_sub(1);
                commands.entity(entity).remove::<RestingContact>();
                continue;
            }

            // Still resting. Hold it at zero — including on the ticks where the
            // contact's own solver bias hands it a fresh velocity, which is
            // exactly the `entry.reference` this run is measured against.
            //
            // Idempotent, and never applied to a sleeping body: writing a
            // velocity component is the one thing that would wake a body
            // the engine has already settled.
            if !asleep && (linear.0 != Vec3::ZERO || angular.0 != Vec3::ZERO) {
                linear.0 = Vec3::ZERO;
                angular.0 = Vec3::ZERO;
            }
            continue;
        }

        // A body nobody is touching cannot be resting against anything, and a
        // sleeping one the rule has never marked has no residual of its own to
        // retire — the engine got there first.
        if !touching_now || asleep {
            continue;
        }

        let entry = state.bodies.entry(entity).or_insert(BodyState {
            still_ticks: 0,
            moving_ticks: 0,
            reference: linear.0,
            marked: false,
        });
        if (linear.0 - entry.reference).length() <= RESTING_STILL_EPSILON_M_S {
            entry.still_ticks += 1;
        } else {
            entry.still_ticks = 0;
            entry.reference = linear.0;
        }
        if entry.still_ticks < RESTING_STILL_TICKS {
            continue;
        }
        // Retire the drift. Velocity only: the pose the contact produced is
        // where the body stops.
        let was_moving = linear.0 != Vec3::ZERO || angular.0 != Vec3::ZERO;
        linear.0 = Vec3::ZERO;
        angular.0 = Vec3::ZERO;
        entry.marked = true;
        entry.still_ticks = 0;
        entry.moving_ticks = 0;
        entry.reference = linear.0;
        if was_moving {
            reports.retired += 1;
        }
        reports.resting += 1;
        commands.entity(entity).insert(RestingContact::at(tick));
    }
}

/// Installs the resting rule.
///
/// A plugin of its own rather than a private system inside
/// [`PhysicsBodiesPlugin`](super::PhysicsBodiesPlugin), because the rule is
/// needed by **two** compositions and neither owns the other: the physics
/// session and fixture (which install `PhysicsBodiesPlugin`) and the world
/// composition ([`crate::world::world_app`], which does not — it needs the
/// world contact log and the overlay pass, and `PhysicsBodiesPlugin` would also
/// bring the spawn preflight with it). Adding this plugin to a composition is
/// therefore the whole of the wiring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestingBodiesPlugin;

impl Plugin for RestingBodiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RestingState>()
            .init_resource::<RestingReports>()
            .add_systems(
                FixedPostUpdate,
                retire_contact_residual.after(PhysicsSystems::StepSimulation),
            );
    }
}

/// The resting rule's counters for a running world, or [`None`] when
/// [`RestingBodiesPlugin`] was never added.
#[must_use]
pub fn resting_reports(world: &bevy::prelude::World) -> Option<RestingReports> {
    world.get_resource::<RestingReports>().copied()
}
