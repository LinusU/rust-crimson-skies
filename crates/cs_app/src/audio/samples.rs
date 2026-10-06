//! The population pass: the delivered load's audio members decoded into the
//! device's [`SampleLibrary`] (task #652, `M01-LC-AUDIO-LOAD`).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-D` (real audible playback) on top of `### F41-A`'s declared
//! catalog; the closure itself is spec
//! `specs/F15-asynchronous-asset-loading-and-private-cache.md`. Shared
//! contracts: `docs/contracts/IDENTITY-CONTENT.md` (stable content ids) and
//! `docs/contracts/CLI-EVIDENCE.md` (a missing capability or an undecodable
//! member is named, never silent).
//!
//! # The hop this closes
//!
//! [`super::handoff::insert_audio_session`] already turns a delivered closure
//! into an [`AudioSession`](super::AudioSession) — which *routing specs* the
//! load declared — and the device, plugin and mixer are real (task #635). What
//! nothing did was fill the library the audible device reads: a `VoiceStart`
//! naming a delivered asset was refused with `sample_unavailable` because no
//! loader had put its samples anywhere. `populate` is that half of the same
//! pass: the same [`LoadedItemBinding`](crate::loading::LoadedItemBinding)
//! entities, the same declared catalog, the same install, one call later.
//!
//! # What it populates
//!
//! For the newest delivered load only, and only for members that are
//!
//! * delivered by that closure (a [`LoadedItemBinding`] whose kind is an audio
//!   kind),
//! * **declared** by the world's [`AudioCatalog`] — an undeclared id is already
//!   refused by name in the same pass and is not decoded behind the catalog's
//!   back, and
//! * present in the container the binding's own [`AssetKey`] addresses, under
//!   the derived content id the F14-D.7 baseline derives for that member
//!   (container spelling plus declared name).
//!
//! The result replaces the library **wholesale**
//! ([`SampleLibrary::replace_samples`](super::device::SampleLibrary::replace_samples)), so a reload leaves the previous
//! closure's samples unreachable: they are not merged, not stale-addressable
//! and not played.
//!
//! # What it cannot populate (named, not skipped)
//!
//! * **No sample source in the world.** [`AudioSampleSource`] is what gives the
//!   pass bytes to decode; without it every delivered member is refused as
//!   [`AudioHandoffRefusal::NoSampleSource`] and the library is emptied rather
//!   than left holding the replaced load's samples. A world with no library at
//!   all (the recording stand-in) has nothing to fill and is not refused.
//! * **A source from another content session.** The source's
//!   [`SessionGeneration`] must be the load's own; otherwise the pass would
//!   decode one session's content into another session's library. Refused as
//!   [`AudioHandoffRefusal::ForeignSampleSource`].
//! * **A member whose own header this workspace does not decode.** Its
//!   [`DeviceError`] code is the refusal's own
//!   ([`AudioHandoffRefusal::SampleUndecodable`]), carried unchanged from
//!   [`sound_member_pcm`] — a member refused by F06-C is absent from the
//!   library and named by that code, never dropped in silence.
//! * **A container the VFS will not resolve, an index that does not read, a
//!   member the container does not declare.** Each is a `DeviceError` with the
//!   reader's own code ([`CODE_SAMPLE_ABSENT`] for a member the container does
//!   not name).
//! * **`music` and `dialogue` content that no sound container declares.** Only
//!   the ZBD sound family is decoded here; see
//!   `docs/findings/2026-10-06-m01-lc-audio-load.md`, which records this
//!   loader's coverage.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use bevy::ecs::resource::Resource;
use cs_assets::vfs::{ContentSession, SessionGeneration};
use cs_assets::zbd::{SoundAsset, ZbdContainer};
use cs_content::audio::AudioCatalog;
use cs_content::catalog::baseline::install_file_key;
use cs_formats::ParseContext;
use cs_sim::audio_events::DeviceError;
use cs_types::asset_id::AssetKey;
use cs_types::content::{ContentId, ContentKind};

use super::device::{DeviceSampleLibrary, PcmAudio, sound_member_pcm};
use super::handoff::{AudioHandoffLog, AudioHandoffRefusal};
use crate::loading::LoadIdentity;

/// The stable code a delivered member is refused with when the container its
/// key addresses declares no member under that derived content id.
///
/// Named rather than omitted: a member that is simply not there and a member
/// that failed to decode are different facts, and a report has to be able to
/// say which happened.
pub const CODE_SAMPLE_ABSENT: &str = "sample_member_absent";

// ------------------------------------------------------------- the source --

/// Where the population pass gets the bytes of the members a load delivered.
///
/// The device never opens a file (see [`super::device::SampleLibrary`]), and
/// the F15 closure itself carries digests rather than payloads, so decoding
/// needs one thing the load path already has: a [`ContentSession`] to resolve
/// and read the delivered items through. [`ContentSampleSource`] is the
/// production implementation of this trait; a world publishes whichever source
/// its load was issued under as [`AudioSampleSource`].
///
/// The generation is part of the contract, not a detail: the pass refuses a
/// source whose [`SessionGeneration`] is not the delivered load's, so content
/// from a replaced session can never be decoded into a live library.
pub trait SampleSource: fmt::Debug + Send + Sync {
    /// The content session generation this source reads through.
    fn generation(&self) -> SessionGeneration;

    /// Decodes each delivered member `wanted` names.
    ///
    /// One result per requested content id — a member that could not be
    /// decoded is an `Err` carrying the refusal's own code, never an omitted
    /// entry, so the caller can name it. Entries may come back in any order.
    fn decode(
        &self,
        wanted: &[(ContentId, AssetKey)],
    ) -> Vec<(ContentId, Result<PcmAudio, DeviceError>)>;
}

/// Resource: the sample source this world's delivered load was issued under.
///
/// Inserted alongside the load announcement (before
/// [`ReadyBundle::attach`](crate::loading::ReadyBundle::attach)), because the
/// population pass runs once, with the same install that installs the audio
/// session — a source published after the first frame the load is attached is
/// a source that arrives too late, and the pass says so by refusing rather
/// than by retrying every frame.
#[derive(Resource, Clone, Debug)]
pub struct AudioSampleSource(Arc<dyn SampleSource>);

impl AudioSampleSource {
    /// Publishes `source` as the one the population pass decodes through.
    #[must_use]
    pub fn new(source: Arc<dyn SampleSource>) -> Self {
        Self(source)
    }

    /// Publishes a source reading through `session`.
    ///
    /// The production constructor: the session the load was issued under is
    /// the same one that read the closure, so what gets decoded is what was
    /// delivered.
    #[must_use]
    pub fn of(session: ContentSession) -> Self {
        Self::new(Arc::new(ContentSampleSource::new(session)))
    }

    /// The published source.
    #[must_use]
    pub fn source(&self) -> &Arc<dyn SampleSource> {
        &self.0
    }
}

/// The production [`SampleSource`]: members decoded through the content
/// session the load read through.
///
/// One container is opened and indexed once per container, however many of its
/// members the closure delivered: a retail sound archive declares 2520 members
/// and a mission may deliver a handful of them, so re-indexing per member
/// would make the install's cost the archive's size times the member count.
#[derive(Debug)]
pub struct ContentSampleSource {
    session: ContentSession,
}

impl ContentSampleSource {
    /// A source decoding through `session`.
    #[must_use]
    pub fn new(session: ContentSession) -> Self {
        Self { session }
    }

    /// The session this source reads through.
    #[must_use]
    pub const fn session(&self) -> &ContentSession {
        &self.session
    }

    /// Decodes the members of one container, all of which that container's
    /// key addresses.
    fn decode_container(
        &self,
        key: &AssetKey,
        wanted: &[ContentId],
    ) -> Vec<(ContentId, Result<PcmAudio, DeviceError>)> {
        let spelling = key.path().as_str();
        // Every failure below refuses *every* member that asked for this
        // container: the container did not open, so none of them can be
        // decoded, and saying so once per member keeps the caller's report
        // complete instead of leaving ids it cannot explain.
        let refuse_all = |code: &'static str, detail: String| {
            wanted
                .iter()
                .map(|content| (content.clone(), Err(DeviceError::new(code, detail.clone()))))
                .collect()
        };
        let container = match ZbdContainer::open(&self.session, key) {
            Ok(container) => container,
            Err(error) => {
                return refuse_all(
                    error.code(),
                    format!("sound container {spelling:?} did not open: {error}"),
                );
            }
        };
        let mut parse = ParseContext::with_defaults(container.label());
        let index = match container.index(&mut parse) {
            Ok(index) => index,
            Err(error) => {
                return refuse_all(
                    error.code(),
                    format!("sound container {spelling:?} did not index: {error}"),
                );
            }
        };
        let table = index.member_table();
        let assets = match container.sound_assets(&mut parse, &index, &table) {
            Ok(assets) => assets,
            Err(error) => {
                return refuse_all(
                    error.code(),
                    format!("sound container {spelling:?} is not a sound archive: {error}"),
                );
            }
        };

        // The identity of a member is the one F14-D.7 derives for it: the
        // container's spelling plus the name its own index declares. Nothing
        // here re-derives identity from an offset or a digest, so a library
        // entry and a catalog row can never name two different members.
        let mut members: BTreeMap<ContentId, &SoundAsset<'_>> = BTreeMap::new();
        for asset in assets.entries() {
            let name = String::from_utf8_lossy(asset.name()).into_owned();
            let Ok(id) = ContentId::from_source(
                ContentKind::Sound,
                &install_file_key(&format!("{spelling}/{name}")),
            ) else {
                // A name the id grammar refuses has no identity to be looked
                // up under, so no delivered id can ever name it: it is not a
                // candidate, and a candidate that is missing is named below.
                continue;
            };
            members.insert(id, asset);
        }

        wanted
            .iter()
            .map(|content| {
                let Some(asset) = members.get(content) else {
                    return (
                        content.clone(),
                        Err(DeviceError::new(
                            CODE_SAMPLE_ABSENT,
                            format!(
                                "sound container {spelling:?} declares no member whose derived \
                                 content id is {content}"
                            ),
                        )),
                    );
                };
                // The refusal's own code, unchanged: `sound_member_pcm` names
                // the step that failed (the member's header decode, or the
                // shape it decoded to).
                let pcm = sound_member_pcm(asset, &mut parse);
                (content.clone(), pcm)
            })
            .collect()
    }
}

impl SampleSource for ContentSampleSource {
    fn generation(&self) -> SessionGeneration {
        self.session.generation()
    }

    fn decode(
        &self,
        wanted: &[(ContentId, AssetKey)],
    ) -> Vec<(ContentId, Result<PcmAudio, DeviceError>)> {
        let mut groups: BTreeMap<AssetKey, Vec<ContentId>> = BTreeMap::new();
        for (content, key) in wanted {
            groups.entry(key.clone()).or_default().push(content.clone());
        }
        let mut decoded = Vec::with_capacity(wanted.len());
        for (key, members) in groups {
            decoded.extend(self.decode_container(&key, &members));
        }
        decoded
    }
}

// ------------------------------------------------------- the population ----

/// What one population pass did: how many delivered members reached the
/// library, and how many were refused (each by name, in the log).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SamplePopulation {
    /// Members decoded into the library.
    pub(crate) samples: usize,
    /// Members that did not, each named in the handoff log.
    pub(crate) refused: usize,
}

/// Fills `library` with the audio members the delivered closure of `load`
/// carries, in one call, and records every member that could not be decoded.
///
/// This is called from [`super::handoff::insert_audio_session`] — the same
/// pass that installs the session — and is total: every path either fills the
/// library with exactly this load's members or empties it, so the previous
/// load's samples are never left reachable, and every member that is not in
/// the library is named in `log`.
pub(crate) fn populate(
    library: Option<&DeviceSampleLibrary>,
    source: Option<&AudioSampleSource>,
    delivered: &[(ContentId, AssetKey)],
    catalog: &AudioCatalog,
    load: LoadIdentity,
    log: &mut AudioHandoffLog,
) -> SamplePopulation {
    let Some(library) = library else {
        // No audible library in this world: its device is the recording
        // stand-in or a caller's own, and there is nothing to fill. Saying so
        // would add a refusal to every headless world for a thing that is not
        // missing there.
        return SamplePopulation::default();
    };
    let library = library.library();

    // Declared *and* delivered: an undeclared id is already refused by name in
    // this same pass, and decoding it would put samples into the library for
    // an asset no session can route.
    let wanted: Vec<(ContentId, AssetKey)> = delivered
        .iter()
        .filter(|(content, _)| catalog.contains(content))
        .cloned()
        .collect();

    if wanted.is_empty() {
        // A reload that delivered no audio empties the library: the replaced
        // closure's samples must not outlive it either.
        library.replace_samples(BTreeMap::new());
        return SamplePopulation::default();
    }

    let Some(source) = source else {
        library.replace_samples(BTreeMap::new());
        log.refusals.push(AudioHandoffRefusal::NoSampleSource);
        return SamplePopulation {
            samples: 0,
            refused: wanted.len(),
        };
    };
    let source = source.source();

    if source.generation() != load.session {
        // The pass would be decoding one content session's bytes into another
        // session's library. Refused, and the library is emptied so the
        // replaced load's samples are not mistaken for this load's.
        library.replace_samples(BTreeMap::new());
        log.refusals.push(AudioHandoffRefusal::ForeignSampleSource {
            load,
            generation: source.generation(),
        });
        return SamplePopulation {
            samples: 0,
            refused: wanted.len(),
        };
    }

    let mut results: BTreeMap<ContentId, Result<PcmAudio, DeviceError>> =
        source.decode(&wanted).into_iter().collect();
    let mut samples = BTreeMap::new();
    let mut refused = 0;
    for (content, _) in &wanted {
        match results.remove(content) {
            Some(Ok(pcm)) => {
                samples.insert(content.clone(), pcm);
            }
            Some(Err(error)) => {
                refused += 1;
                log.refusals.push(AudioHandoffRefusal::SampleUndecodable {
                    content: content.clone(),
                    code: error.code,
                    detail: error.detail,
                });
            }
            None => {
                // A source that reports nothing for a member would otherwise
                // drop it silently, which is exactly what this pass exists to
                // prevent.
                refused += 1;
                log.refusals.push(AudioHandoffRefusal::SampleUndecodable {
                    content: content.clone(),
                    code: CODE_SAMPLE_ABSENT,
                    detail: "the sample source returned no result for this member".to_owned(),
                });
            }
        }
    }
    let samples_count = samples.len();
    library.replace_samples(samples);
    SamplePopulation {
        samples: samples_count,
        refused,
    }
}
