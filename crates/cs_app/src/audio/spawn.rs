//! The spawn/bind path: what a spawned entity needs before it can sound
//! (F41-B follow-up #531).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`. The finding this closes:
//! `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`.
//!
//! F41-B and follow-up #445 built the whole **consumer** half —
//! [`sync_emitter_loops`](super::sync_emitter_loops) binds loops,
//! [`smooth_engine_voices`](super::smooth_engine_voices) advances engine
//! voices, [`mix_session`](super::mix_session) carries both to a device — but
//! every one of those systems keys off components nothing in production ever
//! inserted, so a delivered load installed a session and then mixed silence.
//! This module is the producer half: [`bind_spawned_emitters`] attaches
//! [`AudioEmitterBinding`] (and, on an aircraft,
//! [`EngineVoiceFollow`](super::EngineVoiceFollow)) to the entities a spawn
//! path already created.
//!
//! # Who becomes an emitter
//!
//! Two kinds of entity, and neither is invented content:
//!
//! * **Aircraft** — an entity carrying a flight authority the simulation
//!   already owns: [`FlightAircraft`](crate::physics::flight::FlightAircraft)
//!   (the designed F24 law) or
//!   [`PlaytestOriginalFlight`](crate::playtest::scene::PlaytestOriginalFlight)
//!   (the recovered original law a mission's player body spawns). It gets the
//!   [`AudioEmitterRole::Engine`] loop and an
//!   [`EngineVoiceFollow`](super::EngineVoiceFollow), so its level follows
//!   throttle at the fixed rate.
//! * **World emitters** — the delivered item entity of the installed load
//!   whose own [`cs_content::audio`] record is the world's
//!   [`AudioEmitterRole::Environment`] loop. The F15 handoff spawns that
//!   entity (`ReadyBundle::attach`), so the world's ambient loop exists
//!   exactly where the load put it, lives with the load's other delivered
//!   entities, and stops through the same despawn path when they go. An engine
//!   loop is claimed by an aircraft, music is the F41-C director's bus, and
//!   the remaining cues are one-shots — none of those are world emitters.
//!
//! # Which asset a binding names
//!
//! Content never comes from here: the id always names a declared
//! [`cs_content::audio`] record of the world's
//! [`DeclaredAudioCatalog`] — the same records
//! [`insert_audio_session`](super::insert_audio_session) lowers. The role is
//! resolved **delivered first**: a spec the installed load actually lowered
//! wins, because that is the record the session can play. When the load
//! delivered no record for the role, the binding still names the declared one,
//! so [`sync_emitter_loops`](super::sync_emitter_loops) refuses it by name as
//! [`LoopRefusal::UnknownAsset`](super::LoopRefusal::UnknownAsset) instead of
//! the emitter quietly missing from the mix. Nothing here pre-empts that
//! refusal, and nothing here lowers a record the load did not deliver into
//! something playable.
//!
//! # What is *not* done here
//!
//! * **No placement.** A binding carries no pose; where an emitter *is* is the
//!   spawn path's own `Transform` (an emitter without one is reported as
//!   unplaced by the mixer rather than guessed onto the listener).
//! * **No lifecycle.** Stopping a loop when the entity dies is
//!   [`sync_emitter_loops`](super::sync_emitter_loops)'s job; this system only
//!   ever *adds* bindings, and only to entities that do not have one yet, so
//!   an emitter entity keeps one session-qualified id for its whole life.
//! * **No content invention.** No loop region, no attenuation value and no
//!   bus is authored here; every one of those stays where it was measured or
//!   designed (see the finding above).

use bevy::ecs::entity::Entity;
use bevy::ecs::query::{Or, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cs_content::audio::is_audio_kind;
use cs_sim::audio_events::{AudioBus, AudioEmitterId, PlaybackMode};
use cs_types::content::ContentId;
use cs_types::net::{ActorAllocator, AllocError, SessionId};

use crate::loading::{LoadIdentity, LoadedItemBinding};
use crate::physics::flight::FlightAircraft;
use crate::playtest::scene::PlaytestOriginalFlight;

use super::AudioEmitterBinding;
use super::engine::EngineVoiceFollow;
use super::handoff::{AudioHandoffLog, DeclaredAudioCatalog};
use super::loops::AudioSession;
use super::lower::{lower_bus, lower_mode};

/// One of the two emitter roles this path produces.
///
/// A role names a bus *and* the loop playback form: only something that
/// loops until it stops is an emitter. Which record fills the role is content,
/// not code — see [`bind_spawned_emitters`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioEmitterRole {
    /// An aircraft's engine loop: the bus an engine note mixes on.
    Engine,
    /// The world's ambient loop (wind, weather, environment).
    Environment,
}

impl AudioEmitterRole {
    /// Every role, in a stable order.
    pub const ALL: &'static [AudioEmitterRole] = &[Self::Engine, Self::Environment];

    /// The bus this role mixes on.
    pub const fn bus(self) -> AudioBus {
        match self {
            Self::Engine => AudioBus::Engine,
            Self::Environment => AudioBus::Environment,
        }
    }

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Environment => "environment",
        }
    }
}

/// The role a declared record fills, if any: the engine and environment
/// **loops** are emitters, and every other declared cue is not (one-shots are
/// event-driven, music is the director's bus, radio and UI are queued).
fn role_of(bus: AudioBus, mode: PlaybackMode) -> Option<AudioEmitterRole> {
    match (bus, mode) {
        (AudioBus::Engine, PlaybackMode::Loop) => Some(AudioEmitterRole::Engine),
        (AudioBus::Environment, PlaybackMode::Loop) => Some(AudioEmitterRole::Environment),
        _ => None,
    }
}

/// The role `content`'s declared record fills, or `None` when the catalog
/// holds no record for it or the record cannot state a bus and a mode.
fn declared_role(
    catalog: Option<&DeclaredAudioCatalog>,
    content: &ContentId,
) -> Option<AudioEmitterRole> {
    let record = catalog?.catalog().get(content)?;
    let bus = lower_bus(record.playback().bus.clone().known()?);
    let mode = lower_mode(record.playback().mode.clone().known()?);
    role_of(bus, mode)
}

/// The asset a binding for `role` names: **delivered first**, then declared.
///
/// The delivered half asks the installed session which specs the load actually
/// lowered (in content-id order, so two candidates resolve the same way every
/// frame); the declared half names the record the role needs when the load
/// delivered nothing for it, so the loop system can refuse that exact asset by
/// name instead of the emitter silently playing nothing.
fn resolve_role(
    session: &AudioSession,
    catalog: Option<&DeclaredAudioCatalog>,
    role: AudioEmitterRole,
) -> Option<ContentId> {
    let delivered = session
        .specs()
        .filter(|spec| role_of(spec.bus(), spec.mode()) == Some(role))
        .map(|spec| spec.asset().clone())
        .min();
    if delivered.is_some() {
        return delivered;
    }
    let catalog = catalog?;
    catalog
        .catalog()
        .iter()
        .filter(|record| {
            let Some(bus) = record.playback().bus.clone().known() else {
                return false;
            };
            let Some(mode) = record.playback().mode.clone().known() else {
                return false;
            };
            role_of(lower_bus(bus), lower_mode(mode)) == Some(role)
        })
        .map(|record| record.id().clone())
        .min()
}

/// Resource: the emitter-id allocator of the session this world is running.
///
/// [`AudioEmitterId`] is [`cs_types::net::ActorId`], so an emitter id is
/// `session + serial` and the serial space belongs to a session. The allocator
/// is created here — by the bind path, from the installed session — and is
/// replaced whenever the session generation changes, so ids minted for a
/// replaced load can never name a live emitter of the new one. Serials are
/// monotonic and never recycled (`ActorAllocator`), which is what makes an id
/// *stable per emitter entity*: an entity is bound once and keeps its id, and
/// a despawned entity's serial is never handed to its replacement.
#[derive(Resource, Debug)]
pub struct AudioEmitterIds {
    allocator: ActorAllocator,
}

impl AudioEmitterIds {
    /// An allocator minting ids for `session`, starting at serial 1.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self {
            allocator: ActorAllocator::new(session),
        }
    }

    /// The session this allocator mints ids for.
    pub const fn session(&self) -> SessionId {
        self.allocator.session()
    }

    /// The serial the next allocation would issue.
    pub const fn next_serial(&self) -> u64 {
        self.allocator.next_serial()
    }

    /// Allocates the next session-qualified emitter id.
    ///
    /// # Errors
    ///
    /// [`AllocError::Exhausted`] at the top of the serial range; the bind path
    /// records it by name and stops binding rather than wrapping onto an id
    /// that was already issued.
    pub fn allocate(&mut self) -> Result<AudioEmitterId, AllocError> {
        self.allocator.allocate()
    }
}

/// One binding this path attached, kept so a test or a report can read what
/// the spawn path produced without having inserted any of it.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioBindRecord {
    /// The entity that became an emitter.
    pub entity: Entity,
    /// The session-qualified emitter id it was given.
    pub emitter: AudioEmitterId,
    /// The declared record the binding names.
    pub asset: ContentId,
    /// The bus the binding mixes on.
    pub bus: AudioBus,
    /// The role the entity filled.
    pub role: AudioEmitterRole,
}

/// Why a spawned entity could not become an emitter.
///
/// Refusals are recorded by name and never silently skipped: a world that
/// mixes nothing has to be able to say which role had no content. A refusal is
/// recorded **once per session** rather than once per frame — the bind path
/// runs every frame and would otherwise log the same gap until memory ran out.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioBindRefusal {
    /// No record fills this role: not the installed load lowered one, and not
    /// the declared catalog states one. Nothing could be named, so nothing was
    /// attached — and the gap is this role's, not a silence to infer.
    NoRoleAsset {
        /// The role nothing filled.
        role: AudioEmitterRole,
    },
    /// The serial space is exhausted, so no further emitter can be named in
    /// this session.
    SerialsExhausted {
        /// The role that was being bound when allocation failed.
        role: AudioEmitterRole,
    },
}

/// Resource: what [`bind_spawned_emitters`] attached, and what it refused.
///
/// Cleared whenever the session generation changes, so the record describes
/// the load this world is running now.
#[derive(Resource, Debug, Default)]
pub struct AudioBindLog {
    /// Every binding the path attached, in arrival order.
    pub bound: Vec<AudioBindRecord>,
    /// Every refusal, in arrival order, at most one per distinct refusal.
    pub refusals: Vec<AudioBindRefusal>,
}

impl AudioBindLog {
    /// Records `refusal` unless the identical refusal is already recorded.
    pub fn note(&mut self, refusal: AudioBindRefusal) {
        if !self.refusals.contains(&refusal) {
            self.refusals.push(refusal);
        }
    }
}

/// Attaches emitter bindings to the entities the world's spawn paths created.
///
/// Runs in `Update`, chained **before**
/// [`sync_emitter_loops`](super::sync_emitter_loops) (see
/// [`AudioPlugin`](super::AudioPlugin)), so a frame that spawns an aircraft
/// binds its loop and mixes it in that same frame.
///
/// Total and idempotent: a world with no [`AudioSession`] (nothing has been
/// loaded) binds nothing, an entity that already carries a binding — a test's
/// own, or one from a previous session — is left exactly as it is, and an
/// entity is allocated an id at most once. An exclusive system, like the
/// [`insert_audio_session`](super::insert_audio_session) handoff it follows:
/// both need the whole world, and neither defers anything.
pub fn bind_spawned_emitters(world: &mut World) {
    // The session is the authority: it carries the scene generation every
    // binding must stamp, the specs the load lowered, and the session id the
    // emitter ids are qualified by. No session means nothing has been loaded,
    // which is silence rather than a failure.
    let (generation, session_id, engine) = {
        let Some(session) = world.get_resource::<AudioSession>() else {
            return;
        };
        let engine = resolve_role(
            session,
            world.get_resource::<DeclaredAudioCatalog>(),
            AudioEmitterRole::Engine,
        );
        (session.generation, session.router.session(), engine)
    };

    // The allocator and the log belong to one session: a replaced load gets a
    // fresh serial space (so an id of the old session can never alias a live
    // emitter of the new one) and a fresh record of what was attached.
    let stale = match world.get_resource::<AudioEmitterIds>() {
        Some(ids) => ids.session() != session_id,
        None => true,
    };
    if stale {
        world.insert_resource(AudioEmitterIds::new(session_id));
        world.insert_resource(AudioBindLog::default());
    }

    if engine.is_none() {
        world
            .resource_mut::<AudioBindLog>()
            .note(AudioBindRefusal::NoRoleAsset {
                role: AudioEmitterRole::Engine,
            });
    }

    // Aircraft: every spawned body with a flight authority and no binding yet.
    // Both authorities the workspace spawns are included — the designed F24
    // law and the recovered original law a mission's player body carries —
    // because the propeller system already reads exactly these two as "the
    // engine of an aircraft" and an engine loop that ignored one of them would
    // leave a whole spawn path silent.
    let mut aircraft: Vec<Entity> = Vec::new();
    {
        let mut query = world.query_filtered::<Entity, (
            Without<AudioEmitterBinding>,
            Or<(With<FlightAircraft>, With<PlaytestOriginalFlight>)>,
        )>();
        aircraft.extend(query.iter(world));
    }

    // World emitters: the installed load's own delivered item entities whose
    // declared record is the environment loop. An entity of a *superseded*
    // load is skipped without a refusal — the handoff already names the
    // replaced load once (`AudioHandoffRefusal::ReplacedLoads`), and this
    // system would otherwise re-log it every frame for every stale item.
    let mut world_items: Vec<(Entity, ContentId)> = Vec::new();
    let installed: Option<LoadIdentity> = world
        .get_resource::<AudioHandoffLog>()
        .and_then(|log| log.installed.as_ref().map(|install| install.load));
    if let Some(installed) = installed {
        let mut query =
            world.query_filtered::<(Entity, &LoadedItemBinding), Without<AudioEmitterBinding>>();
        let catalog = world.get_resource::<DeclaredAudioCatalog>();
        for (entity, item) in query.iter(world) {
            if item.load != installed || !is_audio_kind(item.content.kind()) {
                continue;
            }
            if declared_role(catalog, &item.content) == Some(AudioEmitterRole::Environment) {
                world_items.push((entity, item.content.clone()));
            }
        }
    }

    if let Some(asset) = engine {
        for entity in aircraft {
            let emitter = match bind_target(world, AudioEmitterRole::Engine) {
                Ok(emitter) => emitter,
                Err(refusal) => {
                    world.resource_mut::<AudioBindLog>().note(refusal);
                    break;
                }
            };
            let binding = AudioEmitterBinding {
                emitter,
                bus: AudioEmitterRole::Engine.bus(),
                asset: asset.clone(),
                generation,
            };
            world
                .entity_mut(entity)
                .insert((binding.clone(), EngineVoiceFollow::default()));
            record_bind(world, entity, emitter, &binding, AudioEmitterRole::Engine);
        }
    }

    for (entity, asset) in world_items {
        let emitter = match bind_target(world, AudioEmitterRole::Environment) {
            Ok(emitter) => emitter,
            Err(refusal) => {
                world.resource_mut::<AudioBindLog>().note(refusal);
                break;
            }
        };
        let binding = AudioEmitterBinding {
            emitter,
            bus: AudioEmitterRole::Environment.bus(),
            asset: asset.clone(),
            generation,
        };
        world.entity_mut(entity).insert(binding.clone());
        record_bind(
            world,
            entity,
            emitter,
            &binding,
            AudioEmitterRole::Environment,
        );
    }
}

/// The emitter id the next binding of `role` takes.
///
/// # Errors
///
/// [`AudioBindRefusal::SerialsExhausted`] when the session's serial space is
/// spent; the caller records it once and stops binding, because every later
/// allocation would fail the same way.
fn bind_target(
    world: &mut World,
    role: AudioEmitterRole,
) -> Result<AudioEmitterId, AudioBindRefusal> {
    world
        .resource_mut::<AudioEmitterIds>()
        .allocate()
        .map_err(|_: AllocError| AudioBindRefusal::SerialsExhausted { role })
}

/// Appends one attachment to the bind log.
fn record_bind(
    world: &mut World,
    entity: Entity,
    emitter: AudioEmitterId,
    binding: &AudioEmitterBinding,
    role: AudioEmitterRole,
) {
    world
        .resource_mut::<AudioBindLog>()
        .bound
        .push(AudioBindRecord {
            entity,
            emitter,
            asset: binding.asset.clone(),
            bus: binding.bus,
            role,
        });
}
