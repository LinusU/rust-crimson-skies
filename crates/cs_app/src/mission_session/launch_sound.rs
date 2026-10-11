//! The one sound the M01 launch starts (VS-M01-RT-EVIDENCE-B, Rally #1313):
//! the player airframe's engine loop, measured from the original data.
//!
//! The chain is read, never assumed:
//!
//! 1. `pdevastator.engine_sound` through the production vehicle resolver
//!    names a sound group (`snd_devastatorengine`);
//! 2. `sounds.zrd` (member of `ZBD/zrdr.zbd`) maps that group to one archive
//!    member (`devastator.wav`);
//! 3. the member lives in both sound archives; which one the original opens
//!    is **unmeasured** (F14-D.7), so [`LAUNCH_SOUND_CONTAINER`] is a
//!    labelled choice.
//!
//! The bus (`engine`), the mode (`loop`) and the level (`1.0`) the mixer is
//! given are designed decisions, not measurements: the catalog record says
//! so in its provenance. The finding is
//! `docs/findings/2026-10-11-vs-m01-rt-evidence-audio-binding.md`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::app::App;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::{GlobalTransform, Query, ResMut, Resource, With, World};
use cs_sim::audio_events::{AudioDevice, Listener, SpatialPolicy};

use crate::audio::mixer::mix_session;
use crate::audio::{
    AudioBackendLog, AudioPlugin, AudioSession, AudioSpatial, CapabilityDeclaration,
    DeviceSampleLibrary, InMemorySamples, RefusingAudioDevice, SampleProbe, open_audible_device,
    sync_emitter_loops,
};
use crate::scene::SceneGenerations;

use cs_content::audio::{
    AudioAssetRecord, AudioBus, AudioCatalog, AudioDraft, AudioLevel, AudioPlayback, PlaybackMode,
};
use cs_content::catalog::baseline::install_file_key;
use cs_content::original_airframe::{OriginalDocuments, resolve_vehicle_record};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};

use super::content::SoundArchive;

/// The sound archive the launch feeds from: a labelled choice, because which
/// of `soundsl.zbd` / `soundsh.zbd` the original opens is unmeasured.
pub const LAUNCH_SOUND_CONTAINER: &str = "ZBD/soundsh.zbd";

/// The vehicle field naming the airframe's external engine sound group.
const ENGINE_SOUND_FIELD: &str = "engine_sound";

/// The reader archive holding the sound group table.
const SOUND_GROUPS_ARCHIVE: &str = "ZBD/zrdr.zbd";

/// The member of [`SOUND_GROUPS_ARCHIVE`] mapping groups to sound members.
const SOUND_GROUPS_MEMBER: &str = "sounds.zrd";

/// The measured launch sound and everything the composition needs to play it.
#[derive(Clone, Debug)]
pub struct LaunchSound {
    /// The player airframe record the field was read from.
    pub airframe: String,
    /// The sound group the airframe names.
    pub group: String,
    /// The archive member the group maps to.
    pub member: String,
    /// The archive the member is fed from, spelled as the installation does.
    pub container: String,
    /// The installation's digest of that archive.
    pub container_sha256: ContentHash,
    /// The archive on the host, inside the read-only installation.
    pub host: PathBuf,
    /// The stable content id of the member (F14-D.7 derivation).
    pub content: ContentId,
    /// The archive's byte length.
    pub bytes: u64,
}

/// Why the launch sound could not be measured.
#[derive(Debug)]
pub enum LaunchSoundError {
    /// A production reader refused.
    Read(String),
    /// The data does not name the sound the way the measurement requires.
    Unmeasured(String),
}

impl fmt::Display for LaunchSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "the launch sound's source refuses: {reason}"),
            Self::Unmeasured(reason) => write!(f, "the launch sound is not measured: {reason}"),
        }
    }
}

impl std::error::Error for LaunchSoundError {}

/// The content id of a member of the container spelled `container`.
///
/// # Errors
///
/// [`LaunchSoundError::Read`] when the id grammar refuses the spelling.
pub fn member_content_id(container: &str, member: &str) -> Result<ContentId, LaunchSoundError> {
    ContentId::from_source(
        ContentKind::Sound,
        &install_file_key(&format!("{container}/{member}")),
    )
    .map_err(|error| LaunchSoundError::Read(error.to_string()))
}

fn find_record<'a>(node: &'a ZrdValue, name: &str) -> Option<&'a ZrdValue> {
    let children = node.as_list()?;
    for pair in children.chunks(2) {
        if pair[0].as_text() == Some(name) && pair.len() == 2 {
            return Some(&pair[1]);
        }
    }
    children.iter().find_map(|child| find_record(child, name))
}

fn reader_member(install_root: &Path, container: &str, member: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(install_root.join(container))
        .map_err(|error| format!("cannot read {container}: {error}"))?;
    let spelling = cs_types::install::RelativePath::new(container).map_err(|e| e.to_string())?;
    let mut context = ParseContext::with_defaults("launch_sound");
    let decision =
        dispatch(ZbdProbe::new(container, &spelling, &bytes)).map_err(|e| e.to_string())?;
    let index =
        read_version_one_index(&mut context, decision, &bytes).map_err(|e| e.to_string())?;
    let table = index.member_table();
    let archive =
        read_reader_archive(&mut context, &table, index.data()).map_err(|e| e.to_string())?;
    archive
        .entries()
        .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
        .map(|entry| entry.content().to_vec())
        .ok_or_else(|| format!("{container} has no member {member}"))
}

/// Measures the launch sound of the player airframe record `airframe`.
///
/// # Errors
///
/// [`LaunchSoundError`] when the airframe names no engine sound, the group
/// is not a single member, or the chosen archive is not in `archives`.
pub fn measure_launch_sound(
    install_root: &Path,
    airframe: &str,
    archives: &[SoundArchive],
) -> Result<LaunchSound, LaunchSoundError> {
    let read = |reason: String| LaunchSoundError::Read(reason);
    let documents = OriginalDocuments::read(install_root).map_err(|e| read(e.to_string()))?;
    let resolved = resolve_vehicle_record(documents.vehicle(), airframe)
        .map_err(|e| read(format!("{airframe}: {e}")))?;
    let group = resolved
        .value(ENGINE_SOUND_FIELD)
        .and_then(|node| node.as_scalar_text())
        .ok_or_else(|| {
            LaunchSoundError::Unmeasured(format!(
                "{airframe} declares no {ENGINE_SOUND_FIELD} in its record chain"
            ))
        })?
        .to_owned();

    let sounds = decode_zrd(
        &reader_member(install_root, SOUND_GROUPS_ARCHIVE, SOUND_GROUPS_MEMBER).map_err(read)?,
    )
    .map_err(|e| read(format!("{SOUND_GROUPS_MEMBER}: {e}")))?;
    let member = find_record(&sounds, &group)
        .and_then(ZrdValue::as_text)
        .ok_or_else(|| {
            LaunchSoundError::Unmeasured(format!(
                "{SOUND_GROUPS_MEMBER} maps {group} to something other than one member"
            ))
        })?
        .to_owned();

    let archive = archives
        .iter()
        .find(|archive| archive.key.eq_ignore_ascii_case(LAUNCH_SOUND_CONTAINER))
        .ok_or_else(|| {
            LaunchSoundError::Unmeasured(format!(
                "{LAUNCH_SOUND_CONTAINER} is not among the mission's sound archives"
            ))
        })?;
    let container_sha256 =
        ContentHash::from_hex(&archive.sha256).map_err(|e| read(e.to_string()))?;
    let host = install_root.join(LAUNCH_SOUND_CONTAINER);
    let bytes = std::fs::metadata(&host)
        .map_err(|e| read(format!("cannot measure {}: {e}", host.display())))?
        .len();
    Ok(LaunchSound {
        airframe: airframe.to_owned(),
        content: member_content_id(LAUNCH_SOUND_CONTAINER, &member)?,
        group,
        member,
        container: LAUNCH_SOUND_CONTAINER.to_owned(),
        container_sha256,
        host,
        bytes,
    })
}

impl LaunchSound {
    /// The one-record declared catalog the mixer is given.
    ///
    /// The member's identity is measured; its bus, mode and level are
    /// designed, and the record's origin and provenance say so.
    ///
    /// # Errors
    ///
    /// [`LaunchSoundError::Read`] when a record part is refused.
    pub fn catalog(&self) -> Result<AudioCatalog, LaunchSoundError> {
        let refuse = |reason: String| LaunchSoundError::Read(reason);
        let claim = |id: &str| ClaimId::new(id).map_err(|e| refuse(e.to_string()));
        let designed = || -> Result<Provenance, LaunchSoundError> {
            Ok(Provenance::designed(claim(
                "vs-m01-rt-evidence-b.engine-loop-mix",
            )?))
        };
        let record = AudioAssetRecord::try_new(AudioDraft {
            id: self.content.clone(),
            origin: Origin::Designed,
            playback: AudioPlayback {
                bus: Resolved::Known(Known::new(AudioBus::Engine, designed()?)),
                level: Resolved::Known(Known::new(
                    AudioLevel::try_new(1.0).map_err(|e| refuse(e.to_string()))?,
                    designed()?,
                )),
                mode: Resolved::Known(Known::new(PlaybackMode::Loop, designed()?)),
            },
            decoded: Resolved::unknown(
                claim("vs-m01-rt-evidence-b.decoded-shape")?,
                "decoded when the load populates the sample library",
            )
            .map_err(|e| refuse(e.to_string()))?,
            provenance: designed()?,
        })
        .map_err(|e| refuse(e.to_string()))?;
        let mut catalog = AudioCatalog::new();
        catalog.insert(record).map_err(|e| refuse(e.to_string()))?;
        Ok(catalog)
    }
}

// ------------------------------------------------------------ the wiring --

/// Full gain inside this distance, in metres: the designed policy the
/// playtest's audio fixtures use. The original's attenuation law is
/// unmeasured, so this is a choice, not a measurement.
const LISTENER_NEAR_M: f64 = 10.0;
/// Silence beyond this distance, in metres (designed, see above).
const LISTENER_FAR_M: f64 = 100.0;

/// Resource: the tap on what the audible device's output stream pulled from
/// the launch sound, and the library the device reads from.
///
/// This is the **source boundary** (`SampleProbe`): it proves the decoded
/// member reached the device backend and was consumed, not that anything came
/// out of a speaker — that is the owner's `human_review`.
#[derive(Resource, Clone, Debug)]
pub struct LaunchAudioProbe {
    /// What the output stream pulled.
    pub probe: Arc<SampleProbe>,
    /// The library the device plays from; filled by the load's population
    /// pass with the decoded member.
    pub library: Arc<InMemorySamples>,
    /// The named refusal when the audible backend was refused.
    pub refusal: Option<cs_sim::audio_events::DeviceError>,
}

/// Where the launch container is staged inside the private cache: a
/// directory holding exactly `ZBD/soundsh.zbd`, so a one-file content mount
/// does not hash the whole installation (a directory mount hashes every file
/// below its root). The staged copy is verified against the installation's
/// own digest and lives in the Git-ignored private tree.
///
/// # Errors
///
/// [`LaunchSoundError::Read`] when the copy cannot be made or its digest is
/// not the installation's.
pub fn stage_container(
    cache_root: &Path,
    launch: &LaunchSound,
) -> Result<PathBuf, LaunchSoundError> {
    let refuse = |reason: String| LaunchSoundError::Read(reason);
    let root = cache_root.join("launch-sound");
    let target = root.join(&launch.container);
    let digest_of = |path: &Path| -> Option<ContentHash> {
        std::fs::read(path)
            .ok()
            .map(|bytes| cs_assets::install::sha256(&bytes))
    };
    if digest_of(&target) != Some(launch.container_sha256) {
        let parent = target.parent().unwrap_or(&root);
        std::fs::create_dir_all(parent)
            .map_err(|e| refuse(format!("cannot create {}: {e}", parent.display())))?;
        std::fs::copy(&launch.host, &target)
            .map_err(|e| refuse(format!("cannot stage {}: {e}", launch.host.display())))?;
        if digest_of(&target) != Some(launch.container_sha256) {
            return Err(refuse(format!(
                "{} does not match the installation's digest {}",
                launch.container,
                launch.container_sha256.to_hex()
            )));
        }
    }
    Ok(root)
}

/// Installs the audio stack for the launch: the declared one-record catalog,
/// the audible device (or the named refusal) with a probe attached before any
/// voice starts, the sample library the load fills, and the scene generation
/// the handoff reads.
///
/// The sample source (the content session the load ran under) is published
/// by the caller once the load has run.
///
/// # Errors
///
/// [`LaunchSoundError`] when the catalog record is refused.
pub fn install_launch_audio(
    app: &mut App,
    launch: &LaunchSound,
    start_m: [f32; 3],
) -> Result<(), LaunchSoundError> {
    let refuse = |reason: String| LaunchSoundError::Read(reason);
    let declaration = CapabilityDeclaration::from_environment();
    let library = Arc::new(InMemorySamples::new());
    let probe = Arc::new(SampleProbe::new());
    let mut refusal = None;
    let (device, backend): (Box<dyn AudioDevice>, AudioBackendLog) =
        match open_audible_device(library.clone(), &declaration) {
            Ok(device) => (
                Box::new(device.with_probe(probe.clone())),
                AudioBackendLog::audible(&declaration),
            ),
            Err(error) => {
                let log = AudioBackendLog::refused(&declaration, &error);
                let device = Box::new(RefusingAudioDevice::new(error.clone()));
                refusal = Some(error);
                (device, log)
            }
        };
    let policy = SpatialPolicy::try_new(LISTENER_NEAR_M, LISTENER_FAR_M)
        .map_err(|e| refuse(e.to_string()))?;
    let listener = Listener::try_new(start_m.map(f64::from), [1.0, 0.0, 0.0])
        .map_err(|e| refuse(e.to_string()))?;
    app.add_plugins(
        AudioPlugin::new(launch.catalog()?, AudioSpatial::new(policy, listener))
            .with_device(device),
    );
    // The plugin records a stand-in for a caller-given device and publishes no
    // library for it; this device is the audible one (or the named refusal),
    // so the log and the library say so.
    app.insert_resource(backend);
    app.insert_resource(DeviceSampleLibrary::new(library.clone()));
    app.insert_resource(LaunchAudioProbe {
        probe,
        library,
        refusal,
    });
    app.init_resource::<SceneGenerations>();
    app.add_systems(bevy::prelude::Update, follow_listener.before(mix_session));
    Ok(())
}

/// The listener rides with the player's body: the engine is the player's own,
/// so it is heard from where the body is. The right axis stays `+X` (designed;
/// the original's listener orientation is unmeasured).
fn follow_listener(
    mut spatial: ResMut<AudioSpatial>,
    body: Query<&GlobalTransform, With<super::compose::MissionPlayerBody>>,
) {
    if let Some(transform) = body.iter().next()
        && let Ok(listener) = Listener::try_new(
            transform.translation().as_dvec3().to_array(),
            [1.0, 0.0, 0.0],
        )
    {
        spatial.at(listener);
    }
}

/// Stops the launch loops at run end: the emitters' entities have left the
/// world (the caller despawned them), so the loops stop as `Despawned` — the
/// measured reason that is true — and one mix pass carries the stops to the
/// device.
pub fn stop_launch_audio(world: &mut World) {
    if !world.contains_resource::<AudioSession>() {
        return;
    }
    let _ = world.run_system_once(sync_emitter_loops);
    let _ = world.run_system_once(mix_session);
}

/// The refusal the audio backend carries, when the world asked for the
/// audible device and the machine refused it.
#[must_use]
pub fn backend_refusal(app: &App) -> Option<cs_sim::audio_events::DeviceError> {
    app.world()
        .get_resource::<LaunchAudioProbe>()?
        .refusal
        .clone()
}
