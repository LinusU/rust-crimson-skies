//! The drawn propeller's spin, driven by the simulated engine (task #710,
//! `PLAYTEST-PROP-SPIN`).
//!
//! Owner playtest feedback 2026-10-06: "the propeller doesn't spin". This is
//! the presentation system that closes it, and it is deliberately **only** a
//! presentation system: it reads the flight body's authoritative engine state
//! ([`FlightAircraft::engine`], the spool the F24 fixed-wing advances each
//! fixed tick from the held throttle) and writes the **propeller child's own
//! local [`Transform`]**. It never writes the flight body's pose — the body
//! belongs to the physics path alone (AGENTS rule 7, one pose owner).
//!
//! Three inputs, each from its own place, none invented here:
//!
//! | input | where it comes from |
//! | --- | --- |
//! | the hub axis and pivot | [`measure_propeller_hub`](crate::playtest_retail::measure_propeller_hub): measured from the drawn disc's own triangles |
//! | the throttle / engine state | the production flight path: [`FlightAircraft::engine`] |
//! | the pause | [`PlaytestState::paused`], the same session policy the fixed clock obeys |
//!
//! and one designed value, the rate curve, which carries its own claim
//! ([`PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED`]) because no airframe of
//! `ZBD/planes.zbd` stores an rpm and no original run has ever been watched.
//!
//! What is **unmeasured** and stays a declared development choice:
//!
//! * which propeller mesh the original shows at which speed (`staticprop1`,
//!   `prop1`/`prop1b`, `prop2`/`prop2b`, `nitroprop1` are six stored states and
//!   the node flag bits that would say when are unmeasured), so exactly the one
//!   drawn disc is spun and the five others stay undrawn;
//! * which way it turns — [`PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED`] in
//!   `playtest_retail`;
//! * how fast — [`PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED`] below.
//!
//! The startup `playtest sources` line and the smoke `report.json` carry all of
//! it through [`propeller_spin_json`].

use bevy::math::{Quat, Vec3};
use bevy::prelude::{ChildOf, Component, Query, Res, Time, Transform};
use bevy::time::Virtual;
use cs_sim::flight::EngineState;

use super::PlaytestState;
use crate::physics::FlightAircraft;
use crate::playtest_retail::{
    PLAYTEST_PROP_HUB_MEASUREMENT, PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED, PropellerHub,
    PropellerSpinSpec, json_escape,
};

/// The claim id the **rate curve** is filed under: revolutions per second
/// against throttle spool are a designed development value, not a measurement.
///
/// No airframe record of `ZBD/planes.zbd` stores an rpm (the declared engine
/// curve is idle thrust, maximum thrust and a throttle response rate), and no
/// original run has been watched, so the curve below is authored for this
/// playtest and labelled as such everywhere it appears.
pub const PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED: &str =
    "playtest-retail.propeller-spin-rate-is-designed";

/// The spin rule, verbatim, as the startup `playtest sources` line and the
/// smoke `report.json` record it.
pub const PLAYTEST_PROP_SPIN_RULE: &str = "the drawn propeller disc turns about the hub axis \
     measured from its own triangles, at the rate curve recorded here, read from the flight body's \
     engine state every frame; a stopped engine stops it, a pause freezes it, and only the \
     propeller's own local transform is written - never the flight body's pose";

/// Which propeller mesh is drawn and spun, and which are not, verbatim.
pub const PLAYTEST_PROP_SPIN_MESH_RULE: &str = "staticprop1 only: prop1, prop1b, prop2, prop2b and nitroprop1 stay undrawn, because which \
     stored propeller state the original shows at which speed is unmeasured";

/// The disc's rate at **idle** with the engine running, in revolutions per
/// second. Designed: slow enough to read as a turning disc rather than a strobe.
pub const PROP_SPIN_IDLE_REV_PER_S: f64 = 1.0;

/// The disc's rate at **full throttle**, in revolutions per second. Designed:
/// six times idle, and well under the 30 rev/s at which a 60 fps frame would
/// alias the disc backwards instead of showing it turning.
pub const PROP_SPIN_FULL_REV_PER_S: f64 = 6.0;

/// How many revolutions the recorded angle is wrapped at, so a long session
/// keeps f32 precision on the fraction that [`PropellerSpin::transform`]
/// actually draws. Wrapping loses nothing: a turn and a turn plus a whole
/// number of revolutions are the same pose.
const REVOLUTION_WRAP: f32 = 1024.0;

/// The disc's designed rate for one engine state: **zero when the engine is
/// not running**, otherwise a linear curve from [`PROP_SPIN_IDLE_REV_PER_S`]
/// at zero spool to [`PROP_SPIN_FULL_REV_PER_S`] at full spool.
///
/// The spool is what the flight model advances each fixed tick towards the
/// held throttle command, so the disc spools up and down with the engine
/// rather than jumping with the key.
#[must_use]
pub fn propeller_spin_rate_rev_per_s(engine: EngineState) -> f64 {
    if !engine.running {
        return 0.0;
    }
    let spool = engine.spool.clamp(0.0, 1.0);
    PROP_SPIN_IDLE_REV_PER_S + (PROP_SPIN_FULL_REV_PER_S - PROP_SPIN_IDLE_REV_PER_S) * spool
}

/// One drawn propeller's spin state: where its hub is in the body's frame and
/// how far it has turned.
///
/// The component carries everything the system needs so the system itself
/// keeps no state (AGENTS rule 7: no game state hidden in UI code): the
/// placement the part was spawned at ([`Self::base`]), the **measured** hub
/// axis and pivot carried into the body's frame, and the accumulated
/// revolutions. Nothing about it is authoritative for the flight body.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PropellerSpin {
    /// The part's placement under the body, with no spin applied.
    base: Transform,
    /// The measured disc normal, unit length, in the body's frame.
    axis: Vec3,
    /// The measured hub point, in the body's frame: the one point that never
    /// moves while the disc turns.
    pivot: Vec3,
    /// Accumulated revolutions, wrapped at [`REVOLUTION_WRAP`].
    revolutions: f32,
}

impl PropellerSpin {
    /// The spin state of a disc whose hub was measured in the mesh's own frame
    /// and which is drawn at `base` under its body.
    ///
    /// Both measured values are carried through `base` as an **affine** map —
    /// the pivot as a point, the axis as the image of a unit step from it — so
    /// a placement with a scale still spins about the line its own geometry
    /// describes. A placement that collapses that step (zero scale) keeps the
    /// measured unit axis rather than producing a NaN.
    #[must_use]
    pub fn from_hub(hub: &PropellerHub, base: Transform) -> Self {
        let pivot = base.transform_point(Vec3::from(hub.pivot));
        let step = base.transform_point(Vec3::from(hub.pivot) + Vec3::from(hub.axis)) - pivot;
        let axis = if step.is_finite() && step.length_squared() > f32::EPSILON {
            step.normalize()
        } else {
            Vec3::from(hub.axis)
        };
        Self {
            base,
            axis,
            pivot,
            revolutions: 0.0,
        }
    }

    /// The part's placement without the spin: what the transform returns to at
    /// zero revolutions.
    #[must_use]
    pub const fn base(&self) -> Transform {
        self.base
    }

    /// The measured hub axis, in the body's frame.
    #[must_use]
    pub const fn axis(&self) -> Vec3 {
        self.axis
    }

    /// The measured hub point, in the body's frame.
    #[must_use]
    pub const fn pivot(&self) -> Vec3 {
        self.pivot
    }

    /// Accumulated whole and partial revolutions, wrapped at
    /// [`REVOLUTION_WRAP`]: the monotonic reading a test or a HUD observes.
    #[must_use]
    pub const fn revolutions(&self) -> f32 {
        self.revolutions
    }

    /// The turn drawn this revolution, in radians, in `[0, 2π)`.
    #[must_use]
    pub fn rotation_radians(&self) -> f32 {
        self.revolutions.fract() * std::f32::consts::TAU
    }

    /// Advances the disc by `turns` revolutions. Non-finite and non-positive
    /// steps change nothing, so a stalled or stopped engine holds its angle
    /// exactly.
    pub fn advance(&mut self, turns: f32) {
        if !turns.is_finite() || turns <= 0.0 {
            return;
        }
        self.revolutions = (self.revolutions + turns) % REVOLUTION_WRAP;
    }

    /// The part's local transform with the spin applied: a rotation of
    /// [`Self::rotation_radians`] about the measured axis **through the
    /// measured pivot**, so the hub stays put and the blades turn around it.
    #[must_use]
    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.pivot)
            * Transform::from_rotation(Quat::from_axis_angle(self.axis, self.rotation_radians()))
            * Transform::from_translation(-self.pivot)
            * self.base
    }
}

/// Turns every drawn propeller whose parent is a flight body, once per frame.
///
/// Registered in `PlaytestPlugin` **after** `sync_pause`, so a pause that
/// begins in a frame freezes the disc in that same frame: the session's pause
/// is the one thing that decides, exactly as it does for the fixed clock. The
/// advance itself is the virtual clock's own delta, so a paused clock moves
/// nothing whatever this system is asked to do.
pub fn spin_propellers(
    time: Res<Time<Virtual>>,
    state: Res<PlaytestState>,
    engines: Query<&FlightAircraft>,
    mut spinners: Query<(&mut PropellerSpin, &ChildOf, &mut Transform)>,
) {
    if state.paused {
        return;
    }
    let dt_s = time.delta_secs_f64();
    if dt_s <= 0.0 || !dt_s.is_finite() {
        return;
    }
    for (mut spin, child, mut transform) in &mut spinners {
        let Ok(engine) = engines.get(child.parent()) else {
            // Not a flight body's child: nothing authoritative to read, so
            // nothing is spun.
            continue;
        };
        let rate = propeller_spin_rate_rev_per_s(engine.engine());
        if rate <= 0.0 {
            // Engine off: the angle holds, and the transform is not written.
            continue;
        }
        spin.advance((rate * dt_s) as f32);
        *transform = spin.transform();
    }
}

/// The `propeller_spin` object of the startup `playtest sources` line and of
/// the smoke `report.json`: the rule, the rate curve **with its claim**, the
/// spin sense claim, the mesh actually drawn and spun, and the hub measured
/// from that mesh's own geometry.
///
/// `None` — the drawn set holds no measurable propeller — is reported as an
/// empty `shown` list and a `null` hub rather than as a rule that ran.
#[must_use]
pub fn propeller_spin_json(spec: Option<&PropellerSpinSpec>) -> String {
    let shown = spec.map_or_else(
        || "[]".to_owned(),
        |spec| format!("[\"{}\"]", json_escape(&spec.node_name)),
    );
    let hub = spec.map_or_else(
        || "null".to_owned(),
        |spec| {
            let [ax, ay, az] = spec.hub.axis;
            let [px, py, pz] = spec.hub.pivot;
            format!(
                "{{\"measured\":\"{}\",\"axis\":[{ax:?},{ay:?},{az:?}],\"pivot\":[{px:?},{py:?},\
                 {pz:?}],\"radius_m\":{:?},\"thickness_m\":{:?}}}",
                json_escape(PLAYTEST_PROP_HUB_MEASUREMENT),
                spec.hub.radius_m,
                spec.hub.thickness_m,
            )
        },
    );
    format!(
        "\"propeller_spin\":{{\"shown\":{shown},\"mesh_rule\":\"{}\",\"rule\":\"{}\",\
         \"rate_claim\":\"{PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED}\",\"sense_claim\":\"{}\",\
         \"rate_rev_per_s\":{{\"idle\":{PROP_SPIN_IDLE_REV_PER_S:?},\
         \"full\":{PROP_SPIN_FULL_REV_PER_S:?}}},\"hub\":{hub}}}",
        json_escape(PLAYTEST_PROP_SPIN_MESH_RULE),
        json_escape(PLAYTEST_PROP_SPIN_RULE),
        json_escape(PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED),
    )
}
