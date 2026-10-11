//! The weapon-effect presentation consumer: what actually sounds and draws the
//! effects an accepted fire event emitted
//! (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, non-negotiable
//! 5; task F27-C-FX #539). The finding this closes:
//! `docs/findings/2026-10-11-f27-c-fx-weapon-effect-presentation.md`.
//!
//! F27-C wired the weapon pipeline up to the edge of the presentation side:
//! every *accepted* [`FireEvent`](cs_sim::weapons::FireEvent) becomes one
//! [`WeaponEffect`] carrying that shot's own declared muzzle-effect and
//! shot-sound catalog ids, its mount, its projectile and the muzzle's world
//! position, recorded once in the session's log — and then nothing played or
//! drew it. This module is that consumer, and the log is its **only** source:
//!
//! * [`present_weapon_effects`] drains [`WeaponSession::drain_effects`] exactly
//!   once per call and hands every drained record to both halves, so an effect
//!   that was never emitted can never be played or drawn, a denied input and a
//!   duplicate packet present nothing, and a drained batch cannot be presented
//!   twice (a second call over an unchanged session sees an empty log).
//! * [`trigger_shot_sound`] is the audio half: the declared shot-sound id is
//!   resolved through the installed load's own lowered audio specs (the same
//!   [`AudioAssetSpec`](cs_sim::audio_events::AudioAssetSpec)s
//!   [`insert_audio_session`](crate::audio::insert_audio_session) installed), a
//!   name the load did not deliver is refused by name instead of playing a
//!   guessed clip, and the accepted cue is placed against the world's listener
//!   with the **emitted muzzle position** — never the listener's own position
//!   as a default.
//! * [`spawn_muzzle_effect`] is the visual half: one [`MuzzleEffectSpawn`]
//!   entity per accepted shot, carrying the gun's declared muzzle-effect
//!   catalog id and the emitted pose, stamped with the caller's scene
//!   generation (the `STATE-TRANSACTIONS` session-generation discipline, the
//!   same rule [`crate::debris::SpawnedDebris`] and
//!   [`crate::weapons::WeaponRoundMirror`] follow). The entity is a record for
//!   the render path to read, not a rendering: no pipeline yet resolves a
//!   `hardpoint_equipment` id to a drawable asset, so nothing here hardcodes a
//!   mesh and nothing claims any id matches the original.
//!
//! The two halves are independent: an undelivered sound asset refuses the
//! audio half by name and the muzzle entity still spawns, because a missing
//! audio record is not a reason to erase the flash.
//!
//! Like [`step_weapon_session`](crate::weapons::step_weapon_session), this is a
//! function, not a registered system: `WeaponSession` lives with its caller,
//! and the pass needs the weapon session, the audio session and the world
//! together. Whoever owns the session's tick calls it once per tick after the
//! step. One-shot *device* playback is F41's follow-up space — this stage stops
//! at the accepted, placed cue the router holds, exactly where the loop
//! systems stood before their mixer consumer.

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use bevy::prelude::{Entity, Transform, World};
use cs_sim::audio_events::{
    AudioEventError, AudioEventId, OneShotEvent, OneShotOutcome, SpatialError, SpatialMix,
    spatialize,
};
use cs_sim::damage::{ActorId, DamageNodeKey};
use cs_sim::weapons::ProjectileId;
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

use crate::audio::{AudioSession, AudioSpatial};
use crate::scene::SceneGeneration;
use crate::weapons::{WeaponEffect, WeaponSession};

/// The producer serial shot-sound one-shots this consumer creates are stamped
/// with.
///
/// [`AudioEventId`](cs_sim::audio_events::AudioEventId)s are deduplicated per
/// `(session, producer)` by the router's one-shot ledger, so weapon shots need
/// their own producer space, separate from the loop bindings'
/// [`LOOP_PRODUCER`](crate::audio::LOOP_PRODUCER).
pub const WEAPON_SHOT_PRODUCER: u32 = 1;

/// Resource: the weapon-shot producer's one-shot sequence state of the
/// installed session.
///
/// The router's ledger accepts an event id at most once per `(session,
/// producer)`, so the sequences this consumer stamps must be strictly
/// increasing for as long as one audio session plays shot cues. The counter is
/// therefore session presentation state held beside the sessions, exactly as
/// [`crate::audio::AudioEmitterIds`] holds the emitter serial space: a fresh
/// session gets a fresh counter, and a counter that outlives its session only
/// issues *higher* sequences into a fresh router's empty ledger, which is
/// always accepted. A sequence consumed by a cue that was later refused leaves
/// a harmless gap; the ledger only rejects non-increasing ids.
#[derive(Resource, Clone, Debug, Default)]
pub struct WeaponShotSequences {
    next: u32,
}

impl WeaponShotSequences {
    /// A counter for a freshly installed session, starting at sequence zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The next sequence to stamp, refusing to wrap onto an id already issued.
    ///
    /// `None` at the top of the `u32` range: every further cue would have to
    /// repeat a sequence the router already accepted, so none is issued and
    /// the caller reports the refusal by name rather than wrapping
    /// ([`WeaponEffectRefusal::SequencesExhausted`]).
    fn take(&mut self) -> Option<u32> {
        let sequence = self.next;
        self.next = sequence.checked_add(1)?;
        Some(sequence)
    }
}

/// Where one accepted shot's cue sits relative to the listener.
///
/// Every cue states exactly what its placement is: the spatial law's own
/// result, or the report that no listener exists or that the position could
/// not be placed. A cue is never *defaulted* onto the listener's own position
/// — the rule [`mix_session`](crate::audio::mix_session) follows for an
/// emitter with no `GlobalTransform`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpatialPlacement {
    /// The muzzle position was placed: distance gain and stereo pan under the
    /// world's designed attenuation policy.
    Placed(SpatialMix),
    /// The world carries no [`AudioSpatial`], so nothing places cues; the cue
    /// is reported as unplaced rather than centred on the listener.
    NoListener,
    /// The spatial law refused the muzzle position.
    Unplaced {
        /// Why it could not be placed.
        error: SpatialError,
    },
}

/// One accepted shot's accepted audio cue.
///
/// The cue exists only for a drained [`WeaponEffect`]: its [`OneShotEvent`] is
/// the asset, bus and gain of the installed load's own lowered spec for the
/// gun's declared shot-sound id, and its id is this consumer's producer stamped
/// with the tick the shot was accepted on. The whole event is kept — not a
/// transcription of its fields — so the later device pass reads exactly what
/// the router accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponShotCue {
    /// The cue the router accepted.
    pub event: OneShotEvent,
    /// The round the cue sounds for.
    pub projectile: ProjectileId,
    /// The muzzle's emitted world position, the placement input.
    pub origin: WorldPosition,
    /// What the spatial law answered for this cue.
    pub spatial: SpatialPlacement,
}

/// Component: one accepted shot's muzzle-effect spawn.
///
/// The pass spawns one of these per drained [`WeaponEffect`], carrying the
/// gun's declared muzzle-effect catalog id and the emitted muzzle pose, under
/// the scene generation the caller named. It carries **no** mesh, sprite,
/// light or lifetime: which drawable asset `effect` resolves to and how long a
/// flash lasts are the render path's decisions, and this record is all the
/// presentation consumer owns (the same designed boundary
/// [`crate::debris::SpawnedDebris`] states). The generation stamp is what a
/// reload uses to identify a leftover by mismatch rather than by a surviving
/// pointer.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct MuzzleEffectSpawn {
    /// The accepted shot this muzzle effect presents.
    pub projectile: ProjectileId,
    /// Who fired it.
    pub shooter: ActorId,
    /// Which mount fired it.
    pub mount: DamageNodeKey,
    /// The gun's declared muzzle-effect catalog id.
    pub effect: ContentId,
    /// The tick the shot was accepted on.
    pub at: Tick,
    /// The scene generation the spawn was stamped under.
    pub generation: SceneGeneration,
}

/// Why one accepted shot's cue could not be presented.
///
/// Every refusal is a returned record naming the shot and the reason, never a
/// silence to infer: the audio half refuses by name so a world that plays no
/// gun fire can say which declared asset was missing.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponEffectRefusal {
    /// The installed load lowered no audio spec for the gun's declared
    /// shot-sound id, so nothing playable names that asset. The cue is not
    /// built and nothing is played — never a guessed clip
    /// ([`crate::audio::LoopRefusal::UnknownAsset`]'s rule for loops).
    UnknownAsset {
        /// The refused shot.
        projectile: ProjectileId,
        /// The declared shot-sound id the load did not deliver.
        sound: ContentId,
    },
    /// The delivered spec could not form a one-shot cue (for example it
    /// declares loop playback).
    ShotSpec {
        /// The refused shot.
        projectile: ProjectileId,
        /// Why the spec refused.
        error: AudioEventError,
    },
    /// The sequence space is spent, so no further cue can be named in this
    /// session without repeating an id the router already accepted.
    SequencesExhausted {
        /// The shot that could not be stamped.
        projectile: ProjectileId,
    },
    /// The spatial law refused the muzzle position, so the cue is not placed
    /// and does not play: a shot at a guessed position is worse than a shot
    /// reported silent.
    Unplaced {
        /// The refused shot.
        projectile: ProjectileId,
        /// Why the position could not be placed.
        error: SpatialError,
    },
    /// The router had already accepted this event id, so nothing may play
    /// again.
    SuppressedDuplicate {
        /// The suppressed event.
        id: AudioEventId,
    },
    /// The cue belongs to another session generation than the router's.
    RefusedForeignSession {
        /// The refused event.
        id: AudioEventId,
        /// The router's session.
        session: SessionId,
    },
}

/// What one [`present_weapon_effects`] pass produced and what it refused.
///
/// Kept in the return value rather than a resource: like
/// [`WeaponTick`](crate::weapons::WeaponTick), the pass is a function and the
/// caller decides where its report lives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponEffectReport {
    /// The cues the router accepted, in drain order.
    pub played: Vec<WeaponShotCue>,
    /// The muzzle-effect entities this pass spawned, in drain order.
    pub spawned: Vec<Entity>,
    /// Everything the pass could not present, in arrival order.
    pub refusals: Vec<WeaponEffectRefusal>,
}

/// Presents every effect the weapon session has emitted since the last drain:
/// one shot-sound cue through the audio session and one muzzle-effect spawn.
///
/// The drain is the **only** source — this pass reads nothing else to decide
/// what sounds or spawns, which is F27 non-negotiable 5 ("consumption, sound
/// and muzzle effects derive from accepted fire events") made structural: a
/// denied input, a refused whole tick or a duplicate packet emitted no effect,
/// so this pass presents nothing for it.
///
/// `spatial` is the world's listener and attenuation policy, if it has one;
/// `generation` stamps the spawned entities so a reload identifies leftovers by
/// mismatch. The two halves are independent per record: an audio refusal is
/// reported and the muzzle effect still spawns.
#[must_use]
pub fn present_weapon_effects(
    world: &mut World,
    weapon: &mut WeaponSession,
    audio: &mut AudioSession,
    sequences: &mut WeaponShotSequences,
    spatial: Option<&AudioSpatial>,
    generation: SceneGeneration,
) -> WeaponEffectReport {
    let mut report = WeaponEffectReport::default();
    for effect in weapon.drain_effects() {
        match trigger_shot_sound(&effect, audio, sequences, spatial) {
            Ok(cue) => report.played.push(cue),
            Err(refusal) => report.refusals.push(refusal),
        }
        report
            .spawned
            .push(spawn_muzzle_effect(world, &effect, generation));
    }
    report
}

/// The audio half: one accepted shot's declared sound, resolved and placed.
///
/// The declared shot-sound id is resolved through the installed load's lowered
/// specs — never a hardcoded clip — and the cue is placed with the effect's
/// **own emitted muzzle position** against the listener. A world with no
/// [`AudioSpatial`] records [`SpatialPlacement::NoListener`] on the cue rather
/// than defaulting the shot onto the listener.
///
/// # Errors
///
/// [`WeaponEffectRefusal::UnknownAsset`] when the load delivered no spec for
/// the declared id, [`WeaponEffectRefusal::ShotSpec`] when the spec cannot
/// form a one-shot, [`WeaponEffectRefusal::SequencesExhausted`] at the top of
/// the sequence range, [`WeaponEffectRefusal::Unplaced`] when the spatial law
/// refuses the muzzle position, and the router's own
/// [`OneShotOutcome`] refusals by name.
pub fn trigger_shot_sound(
    effect: &WeaponEffect,
    audio: &mut AudioSession,
    sequences: &mut WeaponShotSequences,
    spatial: Option<&AudioSpatial>,
) -> Result<WeaponShotCue, WeaponEffectRefusal> {
    let Some(spec) = audio.spec(&effect.sound) else {
        return Err(WeaponEffectRefusal::UnknownAsset {
            projectile: effect.projectile,
            sound: effect.sound.clone(),
        });
    };
    let sequence = sequences
        .take()
        .ok_or(WeaponEffectRefusal::SequencesExhausted {
            projectile: effect.projectile,
        })?;
    let event = spec
        .one_shot_event(AudioEventId {
            session: audio.router.session(),
            tick: effect.at,
            producer: WEAPON_SHOT_PRODUCER,
            sequence,
        })
        .map_err(|error| WeaponEffectRefusal::ShotSpec {
            projectile: effect.projectile,
            error,
        })?;
    let spatial = match spatial {
        None => SpatialPlacement::NoListener,
        Some(config) => {
            match spatialize(&config.policy, &config.listener, effect.origin.to_array()) {
                Ok(mix) => SpatialPlacement::Placed(mix),
                Err(error) => {
                    // `WeaponEffect::origin` is a `WorldPosition`, which is
                    // finite by construction, so today this arm cannot fire
                    // from a drained record — but an unplaceable cue is
                    // refused by name rather than played at a guessed
                    // position if one ever arrives.
                    return Err(WeaponEffectRefusal::Unplaced {
                        projectile: effect.projectile,
                        error,
                    });
                }
            }
        }
    };
    match audio.router.play_one_shot(&event) {
        OneShotOutcome::Accepted { .. } => Ok(WeaponShotCue {
            event,
            projectile: effect.projectile,
            origin: effect.origin,
            spatial,
        }),
        OneShotOutcome::SuppressedDuplicate { id } => {
            Err(WeaponEffectRefusal::SuppressedDuplicate { id })
        }
        OneShotOutcome::RefusedForeignSession { id, session } => {
            Err(WeaponEffectRefusal::RefusedForeignSession { id, session })
        }
    }
}

/// The visual half: one accepted shot's muzzle-effect spawn.
///
/// The entity carries the gun's declared muzzle-effect catalog id exactly as
/// the accepted event named it — no mesh, no sprite, no invented asset — with
/// its `Transform` written from the emitted muzzle origin and its identity
/// stamped with `generation`.
///
/// The spawned entity, so a caller or a test can report what was placed.
pub fn spawn_muzzle_effect(
    world: &mut World,
    effect: &WeaponEffect,
    generation: SceneGeneration,
) -> Entity {
    world
        .spawn((
            MuzzleEffectSpawn {
                projectile: effect.projectile,
                shooter: effect.shooter,
                mount: effect.mount.clone(),
                effect: effect.effect.clone(),
                at: effect.at,
                generation,
            },
            muzzle_transform(effect.origin),
        ))
        .id()
}

/// Despawns every muzzle-effect spawn, returning how many went.
///
/// The teardown entry a reload, restart or aircraft swap calls: it is total
/// (a second call returns zero) and needs no state from this module, so a
/// torn-down world cannot keep a stale flash that outlives its load.
pub fn release_muzzle_effects(world: &mut World) -> usize {
    let spawns: Vec<Entity> = world
        .iter_entities()
        .filter(|entity_ref| entity_ref.contains::<MuzzleEffectSpawn>())
        .map(|entity_ref| entity_ref.id())
        .collect();
    let count = spawns.len();
    for entity in spawns {
        world.entity_mut(entity).despawn();
    }
    count
}

/// The spawn [`Transform`] for an emitted muzzle position.
///
/// Position only: no orientation is measured for a muzzle flash in the
/// original data, so the spawn carries none rather than a default rotation
/// that would read as a claim. The f32 conversion is the ECS's own precision —
/// the authoritative position stays f64 on the effect record, and every
/// spawn's pose comes from it (the same rule
/// [`crate::weapons::sync_round_mirrors`] states for round mirrors).
fn muzzle_transform(position: WorldPosition) -> Transform {
    let [x, y, z] = position.to_array();
    Transform::from_translation(bevy::prelude::Vec3::new(x as f32, y as f32, z as f32))
}
