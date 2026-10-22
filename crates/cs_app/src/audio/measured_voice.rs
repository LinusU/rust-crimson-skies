//! The measured zSound 3D voice law and the launch cinematic's voices
//! (PARITY6-M01-AUDIO-TIMELINE.03, Rally #1863).
//!
//! Two halves share this module:
//!
//! * The **measured engine-voice law** — the `sounds.zrd` group's own
//!   `RANGE`/`VOLUME` fields carried on [`EngineGroupLaw`], mixed per the
//!   `.02` voice-law finding (`zsnd3d.rolloff`/`fade_zone`/`mute_beyond` for
//!   distance, `zsnd3d.pan_law` for bearing, `zsnd3d.doppler_factor` for
//!   rate, `engine.audible_gate`/`audible_limit` for the 2000 m existence
//!   bound):
//!   `docs/findings/2026-10-22-m01-audio-timeline-voice-laws.md`. Emitters
//!   carrying [`MeasuredVoice`] get this mix instead of the designed
//!   flat-zone policy.
//! * The **launch-cinematic voices** — the `SOUND` cues of `generic_intro`'s
//!   scene sequences, played as device one-shots on their measured scene
//!   anchors: `snd_scene1` at `gi_scene1`, `snd_scene2` at `gi_scene2`,
//!   `snd_scene3` at `gi_1stperson` and `snd_scene4` at `gi_playerdrop`
//!   (`docs/findings/2026-10-22-m01-audio-launch-voices.md`).
//!
//! What this does not know stays named: `snd_droplaunch` has no `sounds.zrd`
//! group record and is recorded as a plan refusal rather than guessed onto a
//! member; the runtime `SoundVolume` float and the listener-axis sign are
//! `.02` unknowns, so the group `VOLUME` and the unit-right-axis assumption
//! are the only base terms applied.

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cs_sim::audio_events::{AudioBus, DeviceVoiceId, SpatialPolicy, VoiceStart};
use cs_types::content::ContentId;

/// The launch cinematic's phase, read so the plan can run on its clock.
use crate::mission_session::launch_cinematic::LaunchCinematic;

/// The speed of sound the .02 Doppler law divides by, metres per second
/// (`zsnd3d.doppler_factor`, cited float 345.0).
pub const SPEED_OF_SOUND: f64 = 345.0;

/// The measured pan ceiling: the .02 law reaches `dot · 1600` hundredths-dB
/// on the listener axis, i.e. 0.16 of the device pan field's `±1` range.
pub const PAN_SCALE: f64 = 0.16;

/// The `.02` `engine.audible_limit`: an aircraft voice exists only while the
/// aircraft is the player or within this distance of the player aircraft.
pub const AUDIBLE_RANGE_M: f64 = 2000.0;

/// The `.02` fade runs from `dmax` to this multiple of it before the voice
/// mutes outright (`zsnd3d.mute_beyond`).
const FADE_UPPER: f64 = 1.1;

/// The projection law the measured pass places emitters under. The measured
/// mix's distance gain and bearing pan are carried on a *projected*
/// `EmitterMix.position_m`: the position where this policy's `spatialize`
/// returns exactly the measured gain and pan. The reference distance is the
/// range `d' = ref/gain` needs to stay inside for every measured gain down
/// to the fade floor (1/1024 ⇒ 10240 m), and the cutoff is far past it so
/// the mute edge is `gain == 0` rather than an early floor.
const PROJECTION_NEAR_M: f64 = 10.0;
const PROJECTION_FAR_M: f64 = 1_000_000.0;

/// Resource: the measured `sounds.zrd` `RANGE`/`VOLUME` of the engine group
/// the session's engine-role loops play (the `.02` `desc` terms).
///
/// Inserted by the launch-audio install when the measured group carries the
/// fields; absent elsewhere, and absent worlds mix the designed law as
/// before.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct EngineGroupLaw {
    /// The group's `RANGE` near edge in metres (`dmin`).
    pub range_min: f64,
    /// The group's `RANGE` far edge in metres (`dmax`).
    pub range_max: f64,
    /// The group's `VOLUME` field — the `.02` `desc.volume` factor (the
    /// runtime `SoundVolume` float stays a named unknown, so this is the
    /// whole measured base term).
    pub volume: f64,
}

/// A validated engine-group law.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineGroupLawError {
    /// The range is not `0 < min < max`, or the volume is not finite and
    /// positive.
    BadShape,
}

impl EngineGroupLaw {
    /// Validates the measured fields.
    ///
    /// # Errors
    ///
    /// [`EngineGroupLawError::BadShape`] for a non-finite or ordered-wrong
    /// field.
    pub fn try_new(
        range_min: f64,
        range_max: f64,
        volume: f64,
    ) -> Result<Self, EngineGroupLawError> {
        let ok = [range_min, range_max, volume].iter().all(|v| v.is_finite())
            && range_min > 0.0
            && range_max > range_min
            && volume > 0.0;
        if ok {
            Ok(Self {
                range_min,
                range_max,
                volume,
            })
        } else {
            Err(EngineGroupLawError::BadShape)
        }
    }
}

/// Marker: this emitter's loop mixes under the session's [`EngineGroupLaw`]
/// (the measured `.02` law), not the designed flat-zone policy. Attached by
/// [`super::spawn::bind_spawned_emitters`] to engine-role emitters when the
/// install carried the law.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeasuredVoice;

/// Resource: the listener pose kept as raw fields.
///
/// [`cs_sim::audio_events::Listener`] keeps its position and right axis
/// private (the mixer never needs them), but the measured law does: the
/// `.02` pan and Doppler terms are computed *here*, so the pose the device
/// hears is kept beside the `AudioSpatial` by the same system that moves
/// the listener.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct VoiceListenerPose {
    /// Listener position in metres.
    pub position: [f64; 3],
    /// The listener's right axis (unit). `.02` leaves which world axis `A`
    /// is unestablished; the designed `+X` stands, as it does for the
    /// `AudioSpatial` listener.
    pub right: [f64; 3],
    /// The listener's velocity for the Doppler term — the player body's
    /// `LinearVelocity` (zero while the cinematic holds it).
    pub velocity: [f64; 3],
}

/// One emitter's measured mix for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredMix {
    /// The `.02` distance gain (rolloff, fade, mute).
    pub gain: f64,
    /// The `.02` bearing pan in `-1 ..= 1` device units.
    pub pan: f64,
    /// The `.02` Doppler rate factor.
    pub doppler: f64,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}

/// The `.02` distance law: `x = (dmax − dmin)/8`, `y = d − dmin`, on the
/// engine's `10·log2` dB scale (`v = 1000·log2 gain` centi-dB, so a linear
/// gain is `2^(v/1000)`).
///
/// * `y ≤ x` → 1.0 (inside the near edge plus the first eighth),
/// * `x < y ≤ 8x` → `x/y` (the `1000·log2(x/y)` rolloff),
/// * `dmax < d < 1.1·dmax` → `2^(−3 − 7·t)`, `t = (d−dmax)/(0.1·dmax)`,
///   continuous at `dmax` (1/8) and ending at the −10000 floor,
/// * `d ≥ 1.1·dmax` → `2^−10` (the −10000 mute: the voice stays alive,
///   silent — never a stop, exactly the floor the fade lands on).
#[must_use]
pub fn distance_gain(law: &EngineGroupLaw, distance: f64) -> f64 {
    let (dmin, dmax) = (law.range_min, law.range_max);
    let x = (dmax - dmin) / 8.0;
    let y = distance - dmin;
    if y <= x {
        1.0
    } else if y <= 8.0 * x {
        x / y
    } else if distance >= FADE_UPPER * dmax {
        (-10.0f64).exp2()
    } else {
        let t = (distance - dmax) / ((FADE_UPPER - 1.0) * dmax);
        (-3.0 - 7.0 * t).exp2()
    }
}

/// The `.02` bearing pan: `dot(src − listener, right) / dist · PAN_SCALE`
/// (the image's `dot · 1600` hundredths-dB scaled onto the device's `±1`).
#[must_use]
pub fn bearing_pan(rel: [f64; 3], distance: f64, right: [f64; 3]) -> f64 {
    if distance <= f64::EPSILON {
        return 0.0;
    }
    (dot(rel, right) / distance * PAN_SCALE).clamp(-1.0, 1.0)
}

/// The `.02` Doppler factor: `1 − dot(v_src − v_listener, src − listener) /
/// dist / SPEED_OF_SOUND`. A coincidence has no factor (1.0); the original's
/// own rate clamp is a device concern, so the factor is unclamped here.
#[must_use]
pub fn doppler_factor(rel_pos: [f64; 3], rel_vel: [f64; 3]) -> f64 {
    let distance = norm(rel_pos);
    if distance <= f64::EPSILON {
        return 1.0;
    }
    1.0 - dot(rel_vel, rel_pos) / distance / SPEED_OF_SOUND
}

/// The whole measured mix for one emitter this frame.
#[must_use]
pub fn measured_mix(
    law: &EngineGroupLaw,
    emitter_pos: [f64; 3],
    emitter_vel: [f64; 3],
    pose: &VoiceListenerPose,
) -> MeasuredMix {
    let rel = [
        emitter_pos[0] - pose.position[0],
        emitter_pos[1] - pose.position[1],
        emitter_pos[2] - pose.position[2],
    ];
    let rel_vel = [
        emitter_vel[0] - pose.velocity[0],
        emitter_vel[1] - pose.velocity[1],
        emitter_vel[2] - pose.velocity[2],
    ];
    let distance = norm(rel);
    MeasuredMix {
        gain: distance_gain(law, distance),
        pan: bearing_pan(rel, distance, pose.right),
        doppler: doppler_factor(rel, rel_vel),
    }
}

/// The policy the measured pass spatializes under.
///
/// # Panics
///
/// Never: the constants are valid by inspection.
#[must_use]
pub fn projection_policy() -> SpatialPolicy {
    SpatialPolicy::try_new(PROJECTION_NEAR_M, PROJECTION_FAR_M)
        .expect("the projection constants form a valid policy")
}

/// The position an emitter must report so the [`projection_policy`]'s
/// `spatialize` returns exactly `mix`'s measured gain and pan, keeping the
/// true bearing for the axis `right` cannot explain.
///
/// `spatialize` reads a gain `ref/d'` from the distance `d'` it is placed
/// at, so the measured gain `g` maps to `d' = ref/g` (the mute maps onto the
/// cutoff). Pan reads the placed direction's component along `right`, so
/// the emitter is rotated within the plane `right` spans with the true
/// direction's perpendicular remainder — a source dead ahead of the
/// listener keeps its depth, and only the side component is rescaled to
/// the measured ±0.16.
#[must_use]
pub fn projected_position(
    pose: &VoiceListenerPose,
    emitter_pos: [f64; 3],
    mix: &MeasuredMix,
) -> [f64; 3] {
    let rel = [
        emitter_pos[0] - pose.position[0],
        emitter_pos[1] - pose.position[1],
        emitter_pos[2] - pose.position[2],
    ];
    let along = dot(rel, pose.right);
    let mut perp = [
        rel[0] - along * pose.right[0],
        rel[1] - along * pose.right[1],
        rel[2] - along * pose.right[2],
    ];
    let plen = norm(perp);
    if plen > 1e-9 {
        perp = [perp[0] / plen, perp[1] / plen, perp[2] / plen];
    } else {
        // The emitter lies exactly on the right axis: any perpendicular
        // completes the direction. Gram-Schmidt against the axis with the
        // smallest component keeps it unit and well-conditioned.
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let base = axes
            .iter()
            .min_by(|a, b| {
                dot(**a, pose.right)
                    .abs()
                    .total_cmp(&dot(**b, pose.right).abs())
            })
            .copied()
            .unwrap_or([0.0, 1.0, 0.0]);
        let b = dot(base, pose.right);
        let gs = [
            base[0] - b * pose.right[0],
            base[1] - b * pose.right[1],
            base[2] - b * pose.right[2],
        ];
        let gl = norm(gs).max(1e-9);
        perp = [gs[0] / gl, gs[1] / gl, gs[2] / gl];
    }
    let pan = mix.pan.clamp(-1.0, 1.0);
    let rest = (1.0 - pan * pan).max(0.0).sqrt();
    let dir = [
        pan * pose.right[0] + rest * perp[0],
        pan * pose.right[1] + rest * perp[1],
        pan * pose.right[2] + rest * perp[2],
    ];
    let placed = if mix.gain <= 0.0 {
        PROJECTION_FAR_M
    } else {
        PROJECTION_NEAR_M / mix.gain
    };
    [
        pose.position[0] + dir[0] * placed,
        pose.position[1] + dir[1] * placed,
        pose.position[2] + dir[2] * placed,
    ]
}

// ------------------------------------------------------ launch voices -----

/// One `SOUND` cue of the launch cinematic, scheduled on its sequence's own
/// anchor.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneCue {
    /// The `sounds.zrd` group the cue names (e.g. `snd_scene3`).
    pub group: String,
    /// The archive member the group maps to, as a content id.
    pub member: ContentId,
    /// The fixed tick of the cinematic phase on which the cue fires — the
    /// first update whose clock reaches the anchor (the `.02` wait law).
    pub at_tick: u64,
}

/// Resource: the measured launch-voice schedule.
///
/// Built by the launch-audio install from the cinematic's scene spans and
/// the `sounds.zrd` group measurements. Cues anchor at the start tick of
/// the scene animation whose sequence carries their `SOUND` item; a cue
/// the data cannot bind (a `SOUND` name with no group record — the launch's
/// `snd_droplaunch`) is kept in [`Self::refusals`] by name.
#[derive(Resource, Clone, Debug, Default)]
pub struct LaunchVoicePlan {
    /// The playable cues, in anchor order.
    pub cues: Vec<SceneCue>,
    /// Every `SOUND` name the data could not bind, with the reason.
    pub refusals: Vec<String>,
}

/// One scene voice the plan started on the device.
#[derive(Clone, Debug)]
pub struct HeldSceneVoice {
    /// The group the cue names.
    pub group: String,
    /// The member playing.
    pub member: ContentId,
    /// The device voice, when the device took the start.
    pub voice: Option<DeviceVoiceId>,
    /// Fixed ticks left on the member's decoded duration; the record is
    /// dropped when it reaches zero.
    pub remaining_ticks: u64,
}

/// Resource: the launch voices the plan started, and what it could not
/// start.
///
/// The smoke voice trace reads `held` — that is the whole point of keeping
/// device one-shots here: `AudioRouter::play_one_shot` is
/// accept-and-forget, so a scene voice stored only there never reaches a
/// second's set.
#[derive(Resource, Debug, Default)]
pub struct LaunchVoices {
    /// Every cue the device took a voice for, held for the member's decoded
    /// duration.
    pub held: Vec<HeldSceneVoice>,
    /// Every group already attempted, fired or refused — the dedup record
    /// (a refused cue is tried once, named, and not retried every tick).
    pub fired: Vec<String>,
    /// Per-cue device refusals, by group name.
    pub refusals: Vec<String>,
}

/// Fires the plan's cues on the cinematic's clock.
///
/// Runs in `FixedLast` after the phase's own `advance_phase`, so
/// `elapsed_ticks` is the phase's own tick count when the cue list is
/// walked: a cue fires on the first update whose clock reaches its anchor,
/// the `.02` wait law. The device voice is kept in [`LaunchVoices`] for its
/// decoded duration — the `AudioDevice` contract has no completion
/// callback, so the record's life is the member's measured length, not a
/// guess at one.
pub fn drive_launch_voices(world: &mut World) {
    let Some(plan) = world.get_resource::<LaunchVoicePlan>().cloned() else {
        return;
    };
    // The cues are the cinematic's own: a world with no phase installed — a
    // capture scenario or a no-teleport smoke, which never runs the intro —
    // has no cinematic clock for them to fire on, so nothing fires.
    let Some(phase) = world.get_resource::<LaunchCinematic>() else {
        return;
    };
    let elapsed = phase.elapsed_ticks();
    world.init_resource::<LaunchVoices>();
    for cue in &plan.cues {
        if elapsed < cue.at_tick {
            continue;
        }
        if world
            .resource::<LaunchVoices>()
            .fired
            .iter()
            .any(|group| group == &cue.group)
        {
            continue;
        }
        let start = VoiceStart {
            asset: cue.member.clone(),
            // The scene groups carry no SFX/3D tag in `sounds.zrd`; the bus
            // is a designed choice — ambient world sound rather than a
            // mislabelled engine or dialogue.
            bus: AudioBus::Environment,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        };
        let outcome = world
            .get_resource_mut::<super::AudioOutput>()
            .map(|mut output| output.device_mut().start_one_shot(start));
        // The held record's life is the member's decoded length: read it
        // from the device library that the load's own population pass
        // filled — never a guessed duration.
        let duration_ticks = world
            .get_resource::<super::device::DeviceSampleLibrary>()
            .and_then(|library| library.library().pcm(&cue.member))
            .map(|pcm| {
                crate::mission_session::launch_cinematic::ticks_for(
                    pcm.sample_count() as f64
                        / f64::from(pcm.channels())
                        / f64::from(pcm.rate_hz()),
                    crate::physics::BASELINE_FIXED_HZ,
                )
            })
            .unwrap_or(1);
        let mut voices = world.resource_mut::<LaunchVoices>();
        match outcome {
            Some(Ok(voice)) => voices.held.push(HeldSceneVoice {
                group: cue.group.clone(),
                member: cue.member.clone(),
                voice: Some(voice),
                remaining_ticks: duration_ticks,
            }),
            Some(Err(error)) => voices.refusals.push(format!("{}: {error}", cue.group)),
            None => voices
                .refusals
                .push(format!("{}: no audio device installed", cue.group)),
        }
        voices.fired.push(cue.group.clone());
    }
    // Run the held records down on the same clock that fired them; a voice
    // outliving the phase keeps sounding — the countdown, not the phase,
    // owns its record.
    let mut voices = world.resource_mut::<LaunchVoices>();
    for held in &mut voices.held {
        held.remaining_ticks = held.remaining_ticks.saturating_sub(1);
    }
    voices.held.retain(|held| held.remaining_ticks > 0);
}
