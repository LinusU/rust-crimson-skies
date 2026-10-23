//! The M01 ambient loops the original holds beside the engines: the
//! `snd_planeshake` rattle (a keyed voice slot) and the `snd_waterfall`
//! `SOUND ... AT_NODE spew` loop (an animation range gate).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`. Every
//! rule here is a `static` reading of the engine image, recorded in
//! `docs/findings/2026-10-23-m01-audio-rattle-slot-and-sound-sequences.md`
//! (the slot helper `0x45e470`, its tick `0x45e410`, the rattle gate
//! `0x48d21a`, the animation range gate `0x4ecd20`) and
//! `docs/findings/2026-10-22-m01-audio-timeline-voice-laws.md` (the 3D
//! volume and pan law). Nothing is `verified_original`.
//!
//! * [`RattleSlot`] — the voice-slot state machine: a call starts the group
//!   only while the slot holds no handle, and stamps the expiry to the
//!   slot clock; the slot tick stops the voice once the clock is **strictly
//!   greater** than the expiry and then advances the clock.
//! * [`anim_range_gate`] — `min <= d^2 < max` on the squared distance from
//!   the animation's node to the camera.
//! * [`drive_ambient_voices`] — the fixed-tick system that applies both to a
//!   world holding an [`AmbientVoicePlan`] and keeps the device voices in
//!   [`AmbientVoices`], which the smoke voice trace reads.
//!
//! Unknowns kept by name in [`AmbientVoicePlan::refusals`] and the finding:
//! the 33,864 B loop has no member (#1861) and is not wired; the origin of
//! the airframe maximum speed `[+0x668]` is read as `fd_speed`
//! (the same designed reading the wingman law makes); the `SoundVolume`
//! float; whether a stepped-off animation keeps updating its node sound.

use bevy::prelude::{GlobalTransform, Resource, With, World};
use cs_sim::audio_events::{AudioBus, DeviceVoiceId, VoiceStart, VoiceStop, VoiceUpdate};
use cs_types::content::ContentId;

use super::measured_voice::{EngineGroupLaw, VoiceListenerPose, measured_mix};

/// The producer label the trace and refusals carry.
pub const AMBIENT_PRODUCER: &str = "ambient-voices";

/// The rattle voice the plan describes (`player.zrd` `rattle`).
#[derive(Clone, Debug, PartialEq)]
pub struct RattleSpec {
    /// The sound group (`rattle.sound`, e.g. `snd_planeshake`).
    pub group: String,
    /// The group's member as a content id.
    pub member: ContentId,
    /// The group's own `VOLUME`, or `1.0` when the group carries none.
    pub volume: f64,
    /// `speed_range[0]`: the fraction of the airframe maximum at which the
    /// rattle starts (`0x48d21a`; only this element is ever read).
    pub speed_floor: f64,
    /// The airframe maximum speed in m/s (`[+0x668]`, read as `fd_speed`).
    pub max_speed_m_s: f64,
}

/// The waterfall voice the plan describes (`SOUND snd_waterfall AT_NODE
/// spew`, gated by the animation's `EXECUTION_BY_RANGE`).
#[derive(Clone, Debug, PartialEq)]
pub struct WaterfallSpec {
    /// The sound group (`snd_waterfall`).
    pub group: String,
    /// The group's member as a content id.
    pub member: ContentId,
    /// The group's `RANGE` and `VOLUME` as the 3D law's terms.
    pub law: EngineGroupLaw,
    /// The squared-distance window `(min, max)` the animation steps in.
    pub window_sq: (f64, f64),
    /// The `spew` node positions in metres.
    pub nodes: Vec<[f64; 3]>,
}

/// Resource: the ambient voices a mission installs.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct AmbientVoicePlan {
    /// The rattle, when the data and the airframe mode bind it.
    pub rattle: Option<RattleSpec>,
    /// The waterfall, when the data binds it.
    pub waterfall: Option<WaterfallSpec>,
    /// Every voice the data could not bind, by name, with the reason.
    pub refusals: Vec<String>,
}

/// The keyed voice slot `0x45e470` / `0x45e410` implement, in fixed ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RattleSlot {
    clock: u64,
    expiry: u64,
    held: bool,
}

impl RattleSlot {
    /// One call of the slot helper with `ttl = 0`: returns whether the
    /// caller must start the group (the record holds no handle) and stamps
    /// the expiry to the current slot clock.
    pub fn call(&mut self) -> bool {
        let start = !self.held;
        self.held = true;
        self.expiry = self.clock;
        start
    }

    /// One slot tick: stops the voice when the clock is strictly greater
    /// than the expiry (returns `true`), then advances the clock.
    pub fn tick(&mut self) -> bool {
        let stop = self.held && self.clock > self.expiry;
        if stop {
            self.held = false;
        }
        self.clock += 1;
        stop
    }

    /// Whether the slot holds a voice.
    #[must_use]
    pub const fn is_held(&self) -> bool {
        self.held
    }
}

/// The rattle consumer's gate: the aircraft is the camera target and its
/// speed has reached `speed_floor` times the airframe maximum (`0x48d21a`).
#[must_use]
pub fn rattle_gate(camera_target: bool, speed: f64, speed_floor: f64, max_speed: f64) -> bool {
    camera_target && speed >= speed_floor * max_speed
}

/// The animation range gate of `0x4ecd20`: the animation steps only for
/// `min <= d^2 < max`.
#[must_use]
pub fn anim_range_gate(distance_sq: f64, window_sq: (f64, f64)) -> bool {
    window_sq.0 <= distance_sq && distance_sq < window_sq.1
}

/// One ambient voice the device took.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldAmbient {
    /// The group the voice plays.
    pub group: String,
    /// The member playing.
    pub member: ContentId,
    /// The device voice.
    pub voice: DeviceVoiceId,
    /// Linear gain last handed to the device.
    pub gain: f64,
    /// Pan last handed to the device.
    pub pan: f64,
    /// Rate ratio last handed to the device.
    pub pitch: f64,
}

/// Resource: the ambient voices' live state.
#[derive(Resource, Debug, Default)]
pub struct AmbientVoices {
    /// The rattle slot.
    pub slot: RattleSlot,
    /// The held rattle voice.
    pub rattle: Option<HeldAmbient>,
    /// The held waterfall voice (started once the range gate first opens).
    pub waterfall: Option<HeldAmbient>,
    /// The waterfall's start tick, once started.
    pub waterfall_started_at: Option<u64>,
    /// Device refusals by group name.
    pub refusals: Vec<String>,
    /// Fixed ticks this system has run.
    pub ticks: u64,
}

impl AmbientVoices {
    /// The voices currently sounding.
    pub fn held(&self) -> impl Iterator<Item = &HeldAmbient> {
        self.rattle.iter().chain(self.waterfall.iter())
    }
}

fn dist_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

/// Runs the ambient voices one fixed tick.
///
/// The rattle gate needs the player body's speed and a control hand-over:
/// while the launch cinematic plays, the camera target is not the aircraft,
/// so the gate is closed. The waterfall starts at the first tick the camera
/// (the player body) is inside the nearest `spew` node's window, and — the
/// loop being a looped group — then keeps sounding under the 3D law.
pub fn drive_ambient_voices(world: &mut World) {
    let Some(plan) = world.get_resource::<AmbientVoicePlan>().cloned() else {
        return;
    };
    world.init_resource::<AmbientVoices>();
    let body = {
        let mut query = world.query_filtered::<(
            &GlobalTransform,
            Option<&avian3d::prelude::LinearVelocity>,
        ), With<crate::mission_session::MissionPlayerBody>>();
        query.iter(world).next().map(|(transform, velocity)| {
            (
                transform.translation().as_dvec3().to_array(),
                velocity.map_or([0.0; 3], |v| v.0.as_dvec3().to_array()),
            )
        })
    };
    let Some((position, velocity)) = body else {
        return;
    };
    let in_control = !crate::mission_session::launch_cinematic_playing(world);
    let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();

    if let Some(rattle) = &plan.rattle {
        let gate = rattle_gate(in_control, speed, rattle.speed_floor, rattle.max_speed_m_s);
        let start = gate && world.resource_mut::<AmbientVoices>().slot.call();
        if start {
            let outcome = start_voice(world, &rattle.member, rattle.volume, 0.0, 1.0);
            let mut voices = world.resource_mut::<AmbientVoices>();
            match outcome {
                Ok(voice) => {
                    voices.rattle = Some(HeldAmbient {
                        group: rattle.group.clone(),
                        member: rattle.member.clone(),
                        voice,
                        gain: rattle.volume,
                        pan: 0.0,
                        pitch: 1.0,
                    });
                }
                Err(reason) => voices.refusals.push(format!("{}: {reason}", rattle.group)),
            }
        }
        let stop = world.resource_mut::<AmbientVoices>().slot.tick();
        if stop {
            let held = world.resource_mut::<AmbientVoices>().rattle.take();
            if let Some(held) = held {
                stop_voice(world, held.voice);
            }
        }
    }

    if let Some(waterfall) = &plan.waterfall {
        let nearest = waterfall
            .nodes
            .iter()
            .map(|node| (*node, dist_sq(*node, position)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let mut ticks = world.resource::<AmbientVoices>().ticks;
        // A refused start is attempted once, like the launch cues.
        let started = world
            .resource::<AmbientVoices>()
            .waterfall_started_at
            .is_some();
        if let Some((node, d2)) = nearest {
            if !started && anim_range_gate(d2, waterfall.window_sq) {
                let outcome = start_voice(world, &waterfall.member, waterfall.law.volume, 0.0, 1.0);
                let mut voices = world.resource_mut::<AmbientVoices>();
                voices.waterfall_started_at = Some(ticks);
                match outcome {
                    Ok(voice) => {
                        voices.waterfall = Some(HeldAmbient {
                            group: waterfall.group.clone(),
                            member: waterfall.member.clone(),
                            voice,
                            gain: 0.0,
                            pan: 0.0,
                            pitch: 1.0,
                        });
                    }
                    Err(reason) => voices
                        .refusals
                        .push(format!("{}: {reason}", waterfall.group)),
                }
            }
            // The held voice mixes under the 3D law every tick, at the node
            // nearest the listener.
            let pose = VoiceListenerPose {
                position,
                right: [1.0, 0.0, 0.0],
                velocity,
            };
            let mix = measured_mix(&waterfall.law, node, [0.0; 3], &pose);
            let update = world
                .resource_mut::<AmbientVoices>()
                .waterfall
                .as_mut()
                .map(|held| {
                    held.gain = mix.gain * waterfall.law.volume;
                    held.pan = mix.pan;
                    held.pitch = mix.doppler;
                    (held.voice, held.gain, held.pan, held.pitch)
                });
            if let Some((voice, gain, pan, pitch)) = update {
                update_voice(world, voice, VoiceUpdate { gain, pan, pitch });
            }
        }
        ticks += 1;
        world.resource_mut::<AmbientVoices>().ticks = ticks;
    } else {
        world.resource_mut::<AmbientVoices>().ticks += 1;
    }
}

fn start_voice(
    world: &mut World,
    member: &ContentId,
    gain: f64,
    pan: f64,
    pitch: f64,
) -> Result<DeviceVoiceId, String> {
    let start = VoiceStart {
        asset: member.clone(),
        bus: AudioBus::Impacts,
        gain,
        pan,
        pitch,
    };
    world
        .get_resource_mut::<super::AudioOutput>()
        .ok_or_else(|| "no audio device installed".to_owned())?
        .device_mut()
        .start_voice(start)
        .map_err(|error| error.to_string())
}

fn stop_voice(world: &mut World, voice: DeviceVoiceId) {
    if let Some(mut output) = world.get_resource_mut::<super::AudioOutput>() {
        let _ = output.device_mut().stop_voice(VoiceStop {
            voice,
            reason: None,
        });
    }
}

fn update_voice(world: &mut World, voice: DeviceVoiceId, update: VoiceUpdate) {
    if let Some(mut output) = world.get_resource_mut::<super::AudioOutput>() {
        let _ = output.device_mut().update_voice(voice, update);
    }
}
