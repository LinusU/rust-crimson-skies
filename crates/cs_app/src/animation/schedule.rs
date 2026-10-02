//! The producer wiring: where the animation advance runs, and what tears an
//! instance down (F20-C.02).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`, non-negotiable behaviors 1, 3 and 4. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F20-B's `advance_animation` was a callable entry with no caller, and
//! F20-C.01's findings recorded the consequence: *no in-game tick reaches the
//! animation path*. This module is that caller. It contains
//!
//! * [`CommittedSessionTick`] — the stamp the **session driver** writes when
//!   it commits a fixed tick, and the only clock input the animation path
//!   reads;
//! * [`advance_animation_on_session_tick`] — the fixed-tick system, which
//!   advances the playback once per **committed tick change** and nothing at
//!   all without the stamp or for a repeated one;
//! * [`AnimationSchedulePlugin`] — installs that system in `FixedPostUpdate`
//!   after the physics step;
//! * [`release_superseded_instances`] — the teardown rule for the scene
//!   loads that supersede a generation an instance was serving.
//!
//! # One clock, one stamp
//!
//! `CommittedSessionTick` is a stamp, **not** a clock. Nothing in this module
//! ever advances it, and its only writer is the session driver that commits a
//! fixed tick (F20-A finding 5: "`advance_to` is the only clock face … the
//! caller supplies the session `Tick`"). No second clock authority is
//! introduced: `Time<Fixed>` belongs to the physics adapter and the session
//! driver, and neither this module nor [`advance_animation`] reads it.
//!
//! A world with no `CommittedSessionTick` advances **nothing** — no marker, no
//! applied component, no pass at all. That is the same "no session, no
//! animation" rule `play_animation` already enforces, and it is why the
//! presence of the stamp is checked rather than defaulted.
//!
//! # Once per committed tick change
//!
//! The system forwards the stamp to [`advance_animation`] only when it
//! **differs** from the tick the playback was last advanced to
//! ([`AnimationPlayback::advanced_through`], written by `advance_animation`
//! itself, so a direct call and this system agree on what "already advanced"
//! means):
//!
//! * **no stamp** — no session, no animation: nothing runs;
//! * **a repeated tick** — not a new tick: no pass runs, nothing is published
//!   and no component is written. A driver that commits the same tick twice,
//!   or a render frame that pumps no new fixed tick, is not a second pass;
//! * **a backwards tick** — forwarded, so the F20-B hold rule can report:
//!   every instance is held where it is, nothing is re-applied and
//!   [`AnimationRefusal::Held`] is published once per occurrence
//!   (non-negotiable behavior 5). The forwarded tick becomes the new
//!   `advanced_through`, so an immediate repeat of it is a repeat and is not
//!   forwarded again — the hold cannot be reported twice for one occurrence;
//! * **a forward tick** — the ordinary case: one pass, one set of published
//!   markers, one idempotent application of the tracks.
//!
//! # Where in the fixed loop
//!
//! The system runs in `FixedPostUpdate` **after**
//! [`PhysicsSystems::StepSimulation`](avian3d::prelude::PhysicsSystems) — the
//! F23-A adapter's measured hook, and the same place the contact reporter and
//! the spawn-tick crossing delivery sit. A marker therefore fires on the tick
//! whose physics has already produced the poses it refers to, and the detach
//! velocity the attachment consumer inherits is the one the step produced.
//! **The original order is unmeasured** (F20-A's unknowns: whether marker
//! firing ran before or after physics); this is a designed placement, and
//! F20-D's probe is where it gets measured.
//!
//! # Teardown for a superseded scene
//!
//! [`release_superseded_instances`] is a directly callable entry, not a
//! system: the scene load path must call it in the step in which it despawns
//! the superseded scene, together with
//! [`release_attachments_before_despawn`], and a fixed-tick system could not
//! be ordered against a despawn that happens in `Update`.

use avian3d::prelude::PhysicsSystems;
use bevy::app::{App, Plugin};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::World;
use bevy::prelude::{FixedPostUpdate, Resource};
use cs_types::Tick;

use crate::physics::PhysicsTickLedger;
use crate::scene::SceneGenerations;

use super::playback::{AnimationPlayback, InstanceKey, advance_animation, release_instance};

/// Resource: the newest session tick the fixed-tick driver has committed.
///
/// The animation path reads it and nothing else: it is written by the session
/// driver that commits a fixed tick, and never by anything in
/// `cs_app::animation`. It is a **stamp, not a clock** — a world that has none
/// advances no animation at all, exactly like a world with no
/// [`AnimationPlayback`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommittedSessionTick(pub Tick);

impl CommittedSessionTick {
    /// The committed tick.
    #[must_use]
    pub const fn new(tick: Tick) -> Self {
        Self(tick)
    }

    /// The committed tick.
    #[must_use]
    pub const fn tick(self) -> Tick {
        self.0
    }
}

/// Advances the animation playback once per committed session tick.
///
/// The fixed-tick entry the session loop runs: it reads
/// [`CommittedSessionTick`], and forwards a *changed* tick to
/// [`advance_animation`]. With no stamp — no session tick has been committed
/// into this world — nothing runs at all. With a tick the playback has already
/// been advanced to, nothing runs either, so a repeated stamp is not a second
/// pass. A tick that went backwards **is** forwarded, so the existing
/// [`AnimationRefusal::Held`](super::AnimationRefusal::Held) rule still
/// reports the hold once per occurrence while every instance stays where it
/// is.
///
/// Directly callable, and the same code the plugin installs, so a test drives
/// exactly what a session runs.
pub fn advance_animation_on_session_tick(world: &mut World) {
    let Some(committed) = world.get_resource::<CommittedSessionTick>().copied() else {
        // No session tick: no session, no animation.
        return;
    };
    let already = world
        .get_resource::<AnimationPlayback>()
        .is_some_and(|playback| playback.advanced_through() == Some(committed.0));
    if already {
        // The same tick committed twice is not a new tick.
        return;
    }
    advance_animation(world, committed.0);
}

/// Installs the fixed-tick animation advance.
///
/// Add it to the world that runs a session's fixed loop — the production
/// [`PhysicsSession`](crate::physics::PhysicsSession), whose
/// [`configure`](crate::physics::PhysicsSessionBuilder::configure) seam is
/// where an app composes one. The **session driver** is the other half and is
/// not installed here: something must commit a fixed tick by writing
/// [`CommittedSessionTick`], and that is the driver's job, not the animation
/// path's. A world that installs this plugin without such a driver runs
/// `FixedPostUpdate` and advances nothing.
///
/// The system is ordered **after** the physics step, so an animation marker
/// fires on the tick whose physics has already run (see the module doc).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimationSchedulePlugin;

impl Plugin for AnimationSchedulePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedPostUpdate,
            advance_animation_on_session_tick.after(PhysicsSystems::StepSimulation),
        );
    }
}

/// The session driver's production writer: commits the physics adapter's
/// fixed-tick count into [`CommittedSessionTick`].
///
/// [`CommittedSessionTick`] is a **stamp**: something outside the animation
/// path has to write the session tick it is simulating, and nothing did. The
/// world's authoritative count of committed fixed ticks is the F23-A physics
/// adapter's [`PhysicsTickLedger`] — the F23-C [`PhysicsSession`] pumps the
/// world and that ledger counts every fixed step it ran — so this system reads
/// that ledger and nothing else. It is not a second clock authority: it never
/// reads `Time<Fixed>`, never advances time, and copies a counter the session
/// already owns.
///
/// A world with **no** [`PhysicsTickLedger`] commits nothing, exactly like a
/// world with no session: the schedule then advances nothing, which is the
/// "no session, no animation" rule `play_animation` and
/// [`advance_animation_on_session_tick`] already enforce. A world that has the
/// ledger but never steps it also commits nothing new.
///
/// Installed by [`AnimationPlugin`], ordered after
/// [`PhysicsSystems::StepSimulation`](avian3d::prelude::PhysicsSystems) and
/// before [`advance_animation_on_session_tick`], so the advance in the same
/// fixed tick reads the tick that step committed.
///
/// [`PhysicsSession`]: crate::physics::PhysicsSession
pub fn commit_session_tick(world: &mut World) {
    let Some(ticks) = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| ledger.ticks)
    else {
        // No physics session: no committed tick, no animation.
        return;
    };
    match world.get_resource_mut::<CommittedSessionTick>() {
        Some(mut committed) => committed.0 = Tick(ticks),
        None => {
            world.insert_resource(CommittedSessionTick::new(Tick(ticks)));
        }
    }
}

/// The one-stop production animation plugin: the session tick driver plus the
/// fixed-tick schedule.
///
/// Add it once to a world that runs a session's fixed loop — the production
/// [`PhysicsSession`](crate::physics::PhysicsSession) composes one through its
/// [`configure`](crate::physics::PhysicsSessionBuilder::configure) seam, the
/// same seam [`FlightForcesPlugin`](crate::physics::FlightForcesPlugin) uses.
/// It installs:
///
/// * [`commit_session_tick`] in `FixedPostUpdate`, after the physics step and
///   before the advance, so the session's committed tick reaches
///   [`CommittedSessionTick`]; and
/// * [`AnimationSchedulePlugin`], the fixed-tick advance itself.
///
/// Add it once: adding it twice (or adding [`AnimationSchedulePlugin`]
/// alongside it) would install the advance twice. A caller that wants only the
/// schedule, without the driver, keeps using [`AnimationSchedulePlugin`]
/// directly — but then nothing commits a tick and the schedule advances
/// nothing, which is the honest state of a world with no session driver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AnimationSchedulePlugin);
        app.add_systems(
            FixedPostUpdate,
            commit_session_tick
                .after(PhysicsSystems::StepSimulation)
                .before(advance_animation_on_session_tick),
        );
    }
}

/// Stops every live instance a superseded scene load left behind and releases
/// what each of them applied; returns the instances it tore down, in stable
/// `(track, instance)` order.
///
/// An instance is superseded when the scene generation it serves is not the
/// one [`SceneGenerations::latest`] names — the load path's own monotone
/// counter, so a reload that succeeded leaves every instance of the previous
/// generation stale (F11/F20 session-generation ownership). The teardown is
/// the same one [`stop_animation`](super::stop_animation) performs per
/// instance: the animation-managed hierarchy links are released first, so a
/// despawn that follows cannot take an animated node with its parent
/// (non-negotiable behavior 4), and then the applied values and the bindings
/// go. An instance of the live generation is untouched.
///
/// A world with **no** [`SceneGenerations`] resource releases nothing and
/// reports nothing: without the load path's counter there is no evidence that
/// any generation was superseded, and tearing every instance down on the
/// absence of evidence would be a guess.
///
/// This is an entry, not a system: **call it in the same step in which the
/// superseded scene's entities are despawned**, together with
/// [`release_attachments_before_despawn`](super::attachment::release_attachments_before_despawn)
/// for the links the animation never applied. Nothing in this crate calls it
/// yet — the spawn/despawn wiring is F20-C's
/// (`docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md`).
pub fn release_superseded_instances(world: &mut World) -> Vec<InstanceKey> {
    let Some(mut playback) = world.remove_resource::<AnimationPlayback>() else {
        return Vec::new();
    };
    let Some(latest) = world
        .get_resource::<SceneGenerations>()
        .map(SceneGenerations::latest)
    else {
        world.insert_resource(playback);
        return Vec::new();
    };
    let superseded = playback.superseded(latest);
    for key in &superseded {
        playback.remove(key);
    }
    world.insert_resource(playback);

    for key in &superseded {
        release_instance(world, &key.clip, key.instance);
    }
    superseded
}
