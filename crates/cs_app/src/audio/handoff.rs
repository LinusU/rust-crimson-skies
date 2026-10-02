//! The loading handoff's audio half: the one place an [`AudioSession`] is
//! created (F41-B follow-up #445).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`. F41-B's finding recorded that nothing inserted the session, so
//! the loop systems had no resource and were never registered:
//! `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`.
//!
//! # Why the handoff owns it
//!
//! An audio session is session- and generation-scoped state, and the F15 load
//! is what defines both:
//!
//! * the **session generation** is the content session the delivered closure
//!   came from, so a loop id from a replaced session can never alias a live
//!   one;
//! * the **set of playable specs** is the delivered audio content of *that*
//!   load, lowered through the real
//!   [`lower_record`](super::lower::lower_record). A declared record the load
//!   did not deliver is not in the session, so an emitter binding naming it is
//!   refused by name ([`super::LoopRefusal::UnknownAsset`]) instead of
//!   scheduling an asset that is not there.
//!
//! [`insert_audio_session`] therefore reads the F15 handoff's own output — the
//! [`LoadedItemBinding`] entities
//! [`ReadyBundle::attach`](crate::loading::ReadyBundle::attach) spawns — and
//! installs the session that load owns. Nothing else creates one, so a world
//! that never loaded has no audio at all, and a reload gets a fresh session for
//! the new generation rather than inheriting the previous load's loops.
//!
//! # What it refuses
//!
//! Every refusal is recorded by name in [`AudioHandoffLog`], never swallowed:
//! delivered audio content with no declared record, a declared record that
//! cannot be lowered, a world whose delivered items come from more than one
//! load, or a world with no [`SceneGenerations`] counter to stamp the session's
//! scene generation from. The last one is deliberately strict: without the load
//! path's own generation counter there is no evidence of a scene generation,
//! and inventing one would make every real binding look stale.

use std::sync::Arc;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cs_content::audio::{AudioCatalog, is_audio_kind};
use cs_sim::audio_events::{AudioAssetSpec, AudioEmitterId, AudioRouter};
use cs_types::content::ContentId;

use crate::loading::{LoadIdentity, LoadedItemBinding};
use crate::scene::{SceneGeneration, SceneGenerations};

use super::loops::AudioSession;
use super::lower::{AudioLowerError, lower_record};
use super::mixer::{AudioMixing, AudioOutput};

/// Resource: the declared audio catalog an installed session lowers its specs
/// from.
///
/// The declared catalog is the content layer's record; the session holds only
/// the lowered runtime specs, exactly as elsewhere in the workspace. Inserting
/// this resource is therefore the content side's half of the wiring, and
/// [`insert_audio_session`] refuses to install without it.
#[derive(Resource, Clone, Debug)]
pub struct DeclaredAudioCatalog(Arc<AudioCatalog>);

impl DeclaredAudioCatalog {
    /// Wraps `catalog` for the session to lower from.
    #[must_use]
    pub fn new(catalog: AudioCatalog) -> Self {
        Self(Arc::new(catalog))
    }

    /// The declared catalog.
    #[must_use]
    pub fn catalog(&self) -> &AudioCatalog {
        &self.0
    }
}

/// The load whose audio session is installed, and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioInstall {
    /// The load that owned the session.
    pub load: LoadIdentity,
    /// The content session generation the session's router is bound to.
    pub session: u64,
    /// The scene generation the session's emitter bindings must carry.
    pub generation: SceneGeneration,
    /// How many delivered audio records lowered into a playable spec.
    pub specs: usize,
    /// How many delivered audio records did not.
    pub refused: usize,
}

/// Why a delivered load could not install an audio session, or a delivered
/// audio record could not be lowered.
///
/// A load that is **already** installed is not a refusal: this system runs every
/// frame, so recording "already installed" would add one entry per frame to a
/// log nobody wants. [`AudioHandoffLog::installs`] and
/// [`AudioHandoffLog::installed`] are the record of what the world holds.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioHandoffRefusal {
    /// The world has no declared audio catalog, so nothing could be lowered.
    NoCatalog,
    /// The world has no [`SceneGenerations`] counter, so there is no evidence
    /// of the scene generation a binding must carry.
    NoSceneGeneration,
    /// The world holds delivered items from more than one load; the newest
    /// owns the session and the older items belong to a replaced load.
    ReplacedLoads {
        /// The load that owns the session.
        installed: LoadIdentity,
        /// The superseded delivered load still holding entities.
        superseded: LoadIdentity,
    },
    /// A delivered audio content id has no declared record.
    UndeclaredAsset {
        /// The content the load delivered.
        content: ContentId,
    },
    /// A delivered declared record cannot be lowered into a routing spec.
    NotLowerable {
        /// The refused record.
        error: AudioLowerError,
    },
}

/// Resource: what the loading handoff installed, and everything it refused.
#[derive(Resource, Clone, Debug, Default)]
pub struct AudioHandoffLog {
    /// The session currently installed, if any.
    pub installed: Option<AudioInstall>,
    /// Every refusal, in arrival order.
    pub refusals: Vec<AudioHandoffRefusal>,
    /// How many sessions this world has installed; more than one means a
    /// reload happened and the previous load's session is gone.
    pub installs: u32,
    /// The emitters whose voices the last install stopped, because the mixer it
    /// replaced still had them sounding.
    pub released: Vec<AudioEmitterId>,
}

/// Which of two loads came later: the content session generation first, then
/// the process-wide serial within it.
///
/// [`LoadIdentity`] itself is `Eq` but deliberately unordered, so the
/// comparison the handoff needs is spelled out here instead of being inferred
/// from a derive it does not have.
fn load_key(load: LoadIdentity) -> (u64, u64) {
    (load.session.get(), load.serial.get())
}

/// Installs the [`AudioSession`] a delivered load owns.
///
/// Runs in `PreUpdate` (see [`AudioPlugin`](super::AudioPlugin)), which is after
/// the load's controlled handoff has attached its bundle and before the frame's
/// loop systems run, so the session exists before anything reads it and a
/// reload's fresh session is never one frame behind.
///
/// Total and idempotent: a world with no delivered items, or one already
/// carrying this load's session, is left exactly as it is. Every refusal is
/// recorded by name in the [`AudioHandoffLog`].
pub fn insert_audio_session(world: &mut World) {
    // Three passes over the delivered bindings, and none of them clones a
    // binding. A `LoadedItemBinding` owns a `ContentId` (its id and key) and an
    // `AssetKey` (its relative path and a cached logical key), so collecting the
    // world's delivered items into a `Vec` every frame would allocate four
    // strings per delivered mesh, texture and sound in the level to look at the
    // handful that are audio — and this runs in `PreUpdate`, in every frame of
    // every mission. `LoadIdentity` is `Copy`, so the passes that only need to
    // know *which* load owns this world cost nothing.
    let mut query = world.query::<&LoadedItemBinding>();
    let mut newest: Option<LoadIdentity> = None;
    for binding in query.iter(world) {
        newest = Some(match newest {
            Some(current) if load_key(current) >= load_key(binding.load) => current,
            _ => binding.load,
        });
    }
    let Some(newest) = newest else {
        // Nothing was delivered into this world: it has no audio, which is not
        // a failure.
        return;
    };
    // Only once the owner is known can the loads it replaced be named: one
    // pass cannot tell a superseded load from the newest one it had not reached
    // yet.
    let mut superseded: Option<LoadIdentity> = None;
    for binding in query.iter(world) {
        if binding.load == newest {
            continue;
        }
        superseded = Some(match superseded {
            Some(current) if load_key(current) <= load_key(binding.load) => current,
            _ => binding.load,
        });
    }
    let mut log = world
        .remove_resource::<AudioHandoffLog>()
        .unwrap_or_default();
    if log
        .installed
        .as_ref()
        .is_some_and(|install| install.load == newest)
    {
        // Already the installed session: this frame changes nothing, and saying
        // so on every frame would grow the log without telling anyone anything.
        world.insert_resource(log);
        return;
    }
    if let Some(superseded) = superseded {
        // Two delivered closures live in one world. The newest load owns the
        // session; the older entities are named here rather than silently
        // folded into it.
        log.refusals.push(AudioHandoffRefusal::ReplacedLoads {
            installed: newest,
            superseded,
        });
    }
    let Some(generation) = world
        .get_resource::<SceneGenerations>()
        .map(SceneGenerations::latest)
    else {
        log.refusals.push(AudioHandoffRefusal::NoSceneGeneration);
        world.insert_resource(log);
        return;
    };
    let Some(catalog) = world.get_resource::<DeclaredAudioCatalog>().cloned() else {
        log.refusals.push(AudioHandoffRefusal::NoCatalog);
        world.insert_resource(log);
        return;
    };

    let mut specs: Vec<AudioAssetSpec> = Vec::new();
    let mut lowered = 0;
    let mut refused = 0;
    for binding in query.iter(world) {
        if binding.load != newest || !is_audio_kind(binding.content.kind()) {
            continue;
        }
        let Some(record) = catalog.catalog().get(&binding.content) else {
            refused += 1;
            log.refusals.push(AudioHandoffRefusal::UndeclaredAsset {
                content: binding.content.clone(),
            });
            continue;
        };
        match lower_record(record) {
            Ok(lowered_asset) => {
                specs.push(lowered_asset.spec);
                lowered += 1;
            }
            Err(error) => {
                refused += 1;
                log.refusals
                    .push(AudioHandoffRefusal::NotLowerable { error });
            }
        }
    }

    world.insert_resource(AudioSession::new(
        AudioRouter::new(newest.session.get()),
        generation,
        specs,
    ));
    // The mixer belongs to the same session as the loops it carries, so a
    // reload replaces both together. The replaced mixer releases its voices
    // first: a session swapped out from under the device would otherwise leave
    // the previous load audible with nothing left to stop it.
    let mut released = Vec::new();
    if let Some(mut mixing) = world.remove_resource::<AudioMixing>()
        && let Some(mut output) = world.remove_resource::<AudioOutput>()
    {
        released = mixing.mixer_mut().release(output.device_mut()).stopped;
        world.insert_resource(output);
        world.insert_resource(mixing);
    }
    world.insert_resource(AudioMixing::new(newest.session.get()));
    log.installs += 1;
    log.released = released;
    log.installed = Some(AudioInstall {
        load: newest,
        session: newest.session.get(),
        generation,
        specs: lowered,
        refused,
    });
    world.insert_resource(log);
}
