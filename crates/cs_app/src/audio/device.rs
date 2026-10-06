//! The audible backend: the one [`AudioDevice`] in this workspace that opens
//! real output hardware and plays decoded original audio through it.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-D`'s playback half. Task #635. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`; capability and evidence rules from
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! Until this module existed the only production device was
//! [`cs_sim::audio_events::RecordingAudioDevice`], which records commands and
//! makes no sound, so nothing in the workspace could produce audible evidence
//! and every `audio` claim was unreachable. This module is the missing half:
//!
//! * [`PcmAudio`] is one decoded audio asset in the shape a device plays —
//!   interleaved samples normalized from the declared width onto `-1.0 ..= 1.0`,
//!   with the member's own channel count and rate.
//! * [`sound_member_pcm`] is the **runtime consumer of the F06-C decoder**: it
//!   turns one `cs_assets` sound-archive member into [`PcmAudio`] under that
//!   member's own WAVE declaration, refusing by name on any failure.
//! * [`SampleLibrary`] is where a load puts the samples a
//!   [`cs_sim::audio_events::VoiceStart`] names; [`InMemorySamples`] is the
//!   plain map behind it.
//! * [`AudibleDevice`] is the [`AudioDevice`]: one rodio output stream, one
//!   looping voice per started loop, gain/pitch/pan applied per update.
//! * [`open_audible_device`] is the **capability gate**: a machine that does not
//!   declare `audio` gets a named [`DeviceError`], never a silent mute, and
//!   [`audibility_exit_code`] maps that refusal onto the CLI evidence
//!   contract's "4 means missing capability".
//! * [`RefusingAudioDevice`] is what a world installs when the gate refuses:
//!   every command fails with the same named code, so the mixer's report names
//!   the reason instead of the world quietly going quiet.
//!
//! # What this does and does not prove
//!
//! [`SampleProbe`] records the sample frames the engine actually pulled out of
//! a decoded member and handed to the output stream, so a run on a machine
//! with the `audio` capability can say that real samples of real original audio
//! reached a real open stream. That is *not* proof that a person heard them:
//! F41 non-negotiable behavior 5 keeps decoded-PCM evidence and audible device
//! verification apart, and `human_review` remains an owner gate. Nothing in
//! this file claims a fidelity of the original mix — the bus faders, the
//! limiter and the loop seam are **not** measured, and are recorded as unknown
//! in `docs/findings/2026-10-05-m01-lc-audible-audio-device.md`.
//!
//! # What is designed, not measured
//!
//! * The constant-power pan law (`cos`/`sin` of the pan angle), the stereo
//!   upmix of a mono member, and the fact that `pitch` is rodio's playback-rate
//!   ratio are this project's choices. The original engine's mixing is
//!   unmeasured (F41 research boundary).
//! * Bus gains are **not** applied: a voice plays at
//!   `loop gain × engine gain × spatial gain` exactly as
//!   [`cs_sim::audio_events::AudioMixer`] computes it, because no original bus
//!   fader is known.
//! * There is no limiter. Two voices summing past `1.0` clip at the device,
//!   which is recorded as a known limitation rather than papered over.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZero;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bevy::ecs::resource::Resource;
use cs_content::replay::{CapabilityClass, DeclaredCapabilities, ReplayError};
use cs_formats::ParseContext;
use cs_formats::zbd::{DecodedSound, PcmLayout, SampleLayout};
use cs_sim::audio_events::{
    AudioDevice, DeviceError, DeviceVoiceId, VoiceStart, VoiceStop, VoiceUpdate,
};
use cs_types::content::ContentId;
use rodio::source::SeekError;
use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source};

use crate::cli::EXIT_RUNTIME_FAILURE;

/// The stable code a machine without the `audio` capability is refused with.
///
/// Named rather than silent on purpose: `docs/contracts/CLI-EVIDENCE.md`
/// requires a missing capability to be a nonzero failure, and an unnamed mute
/// cannot be told apart from a game that simply has nothing to say.
pub const CODE_CAPABILITY_ABSENT: &str = "audio_capability_absent";

/// The stable code a machine that declares `audio` but has no usable output
/// stream is refused with.
pub const CODE_NO_OUTPUT_DEVICE: &str = "no_output_device";

/// The stable code a command is refused because no decoded sample is registered
/// for the asset it names.
pub const CODE_SAMPLE_UNAVAILABLE: &str = "sample_unavailable";

/// The stable code a command is refused because the device is closed.
pub const CODE_DEVICE_CLOSED: &str = "device_closed";

/// The stable code a command is refused because the device does not own the
/// voice it names.
pub const CODE_UNKNOWN_VOICE: &str = "unknown_voice";

/// The stable code a start is refused because one of its mix values is outside
/// the domain a device can apply.
pub const CODE_INVALID_MIX: &str = "invalid_mix";

/// The exit code `docs/contracts/CLI-EVIDENCE.md` assigns to a missing
/// capability.
pub const EXIT_MISSING_CAPABILITY: u8 = 4;

/// An `f32` behind an atomic cell.
///
/// `std` has no `AtomicF32`, and a panicking pan/gain/loudness path is the wrong
/// time to discover that. The value is stored as its IEEE-754 bit pattern, and
/// both accessors go through the `f32` conversions, so every value this type can
/// hold is a value this module could have written — the same discipline the rest
/// of the workspace uses for `f64` (see `cs_sim::ai::combat`'s bit-writing
/// hasher).
#[derive(Debug)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    /// Raises the stored value to `magnitude` when `magnitude` is larger.
    fn raise_to(&self, magnitude: f32) {
        let mut current = self.load();
        while magnitude > current {
            match self.0.compare_exchange_weak(
                current.to_bits(),
                magnitude.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(seen) => current = f32::from_bits(seen),
            }
        }
    }
}

/// What this machine says it can really do.
///
/// A thin wrapper over [`DeclaredCapabilities`] that keeps the **raw**
/// `$CS_CAPABILITIES` list and the reason it could not be parsed, because a
/// malformed list must not read as an empty one: "this machine declares
/// nothing" and "this machine's declaration is a typo" are different facts and
/// a diagnostic has to be able to say which.
///
/// The declaration is a subtraction, never a widening: everything here can only
/// remove what the machine granted, never add to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilityDeclaration {
    /// The parsed classes.
    pub declared: DeclaredCapabilities,
    /// The raw list and the parser's reason, when the list did not parse.
    pub malformed: Option<String>,
}

impl CapabilityDeclaration {
    /// Parses the comma-separated `$CS_CAPABILITIES` spelling.
    ///
    /// An unset or empty list is an empty declaration, which is what an unset
    /// variable means. A blank element or an unknown name is **not** silently
    /// dropped: it lands in [`Self::malformed`] with an empty
    /// [`Self::declared`], so a typo removes capabilities loudly.
    #[must_use]
    pub fn parse(list: &str) -> Self {
        match DeclaredCapabilities::parse(list) {
            Ok(declared) => Self {
                declared,
                malformed: None,
            },
            Err(error) => Self {
                declared: DeclaredCapabilities::none(),
                malformed: Some(describe_parse_error(list, &error)),
            },
        }
    }

    /// Reads `$CS_CAPABILITIES` from this process's environment.
    #[must_use]
    pub fn from_environment() -> Self {
        match std::env::var("CS_CAPABILITIES") {
            Ok(list) => Self::parse(&list),
            // An unset variable is an empty declaration, not a malformed one.
            Err(_) => Self::default(),
        }
    }

    /// Whether this machine declares `class`.
    #[must_use]
    pub fn contains(&self, class: CapabilityClass) -> bool {
        self.declared.contains(class)
    }

    /// The canonical spelling of the declared classes.
    #[must_use]
    pub fn label(&self) -> String {
        self.declared.label()
    }
}

/// The raw list plus the parser's own reason, as one diagnostic string.
fn describe_parse_error(list: &str, error: &ReplayError) -> String {
    let reason = match error {
        ReplayError::Blank { field } => format!("blank {field}"),
        ReplayError::Syntax { field, reason } => format!("{reason} in {field}"),
        other => other.to_string(),
    };
    format!("$CS_CAPABILITIES={list:?} did not parse: {reason}")
}

/// The CLI exit code an audible-device failure maps to.
///
/// `docs/contracts/CLI-EVIDENCE.md`: 4 is a missing capability, and any other
/// nonzero status is a runtime failure. A refusal never maps to 0.
#[must_use]
pub fn audibility_exit_code(error: &DeviceError) -> u8 {
    if error.code == CODE_CAPABILITY_ABSENT {
        EXIT_MISSING_CAPABILITY
    } else {
        EXIT_RUNTIME_FAILURE
    }
}

// ------------------------------------------------------------- pcm samples --

/// One decoded audio asset in the shape a device plays.
///
/// `samples` is **interleaved** and already normalized to `-1.0 ..= 1.0` from
/// the declared width, so a device never has to know whether the original
/// member stored 8-bit unsigned, 16-bit signed or 32-bit signed PCM, or a
/// block-coded stream: the F06-C decoder produces `i32` values and this is the
/// one place that maps them onto the device's domain.
///
/// The channel count and rate are the **member's own declaration**, carried
/// unchanged: nothing here resamples or reinterleaves a member into a different
/// shape, and the upmix to stereo happens only inside a voice source, where it
/// is a playback decision rather than a claim about the asset.
#[derive(Clone, Debug, PartialEq)]
pub struct PcmAudio {
    channels: u16,
    rate_hz: u32,
    samples: Arc<Vec<f32>>,
}

impl PcmAudio {
    /// Assembles device-ready samples from values already in `-1.0 ..= 1.0`.
    ///
    /// # Errors
    ///
    /// [`PcmError::NoChannels`] for zero channels, [`PcmError::NoRate`] for a
    /// zero rate, [`PcmError::UnsupportedChannels`] for a channel count this
    /// device does not place (only 1 and 2 are observed in retail, see
    /// `docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md`), and
    /// [`PcmError::NoSamples`] for an asset with no frames at all — an empty
    /// loop has nothing to repeat.
    pub fn try_new(channels: u16, rate_hz: u32, samples: Vec<f32>) -> Result<Self, PcmError> {
        if channels == 0 {
            return Err(PcmError::NoChannels);
        }
        if rate_hz == 0 {
            return Err(PcmError::NoRate { rate_hz });
        }
        if !matches!(channels, 1 | 2) {
            return Err(PcmError::UnsupportedChannels { channels });
        }
        if samples.is_empty() {
            return Err(PcmError::NoSamples);
        }
        if !samples.iter().all(|value| value.is_finite()) {
            return Err(PcmError::NonFiniteSample);
        }
        Ok(Self {
            channels,
            rate_hz,
            samples: Arc::new(samples),
        })
    }

    /// The member's own declared channel count.
    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// The member's own declared sample rate, in hertz.
    #[must_use]
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// How many sample values the asset holds, across all channels.
    #[must_use]
    pub fn sample_count(&self) -> u64 {
        self.samples.len() as u64
    }

    /// How many whole frames the asset holds.
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.sample_count() / u64::from(self.channels)
    }

    /// How long the asset plays once, at its own declared rate.
    #[must_use]
    pub fn duration(&self) -> Duration {
        if self.rate_hz == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(self.frames() as f64 / f64::from(self.rate_hz))
    }

    /// The interleaved samples, in the device's domain.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// The same interleaved samples as a shared handle.
    ///
    /// A device voice holds the member's samples rather than copying them, so
    /// starting the same loop twice costs one handle, not one copy of a decoded
    /// member. The buffer is immutable once built, so sharing it is safe.
    #[must_use]
    pub fn shared_samples(audio: &Self) -> Arc<Vec<f32>> {
        Arc::clone(&audio.samples)
    }

    /// The samples of one decoded member, normalized under the format **its
    /// own** declaration names.
    ///
    /// This is the whole of the width handling in the audio path: F06-C's
    /// decoder widens every layout to `i32`, and the layout alone says how those
    /// `i32`s relate to full scale. 8-bit RIFF/WAVE PCM is *unsigned* with
    /// silence at 128, so it is shifted before scaling; the signed widths and
    /// both block codecs divide by their own full-scale value.
    ///
    /// # Errors
    ///
    /// [`PcmError`] for a shape this device cannot play, or a value the
    /// decoder produced that is not a finite number.
    pub fn from_decoded(decoded: &DecodedSound) -> Result<Self, PcmError> {
        let format = decoded.format();
        let samples = decoded
            .samples()
            .iter()
            .map(|value| normalize(format.layout(), *value))
            .collect();
        Self::try_new(format.channels(), format.rate_hz(), samples)
    }
}

/// Maps one decoded value onto `-1.0 ..= 1.0` for the layout that produced it.
fn normalize(layout: SampleLayout, value: i32) -> f32 {
    match layout {
        // RIFF/WAVE 8-bit PCM is unsigned with silence at 128; the shift is the
        // format's own convention, not a gain choice.
        SampleLayout::Pcm(PcmLayout::Unsigned8) => (value as f32 - 128.0) / 128.0,
        SampleLayout::Pcm(PcmLayout::Signed16Le) => value as f32 / 32_768.0,
        SampleLayout::Pcm(PcmLayout::Signed32Le) => value as f32 / 2_147_483_648.0,
        // Both block codecs widen to a clipped `i16` (see
        // `cs_formats::zbd::adpcm`), so the 16-bit full scale is the right one.
        SampleLayout::Adpcm(_) => value as f32 / 32_768.0,
    }
}

/// Why a decoded asset is not in a shape a device can play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmError {
    /// The member declared no channels.
    NoChannels,
    /// The member declared no sample rate.
    NoRate {
        /// The declared rate.
        rate_hz: u32,
    },
    /// The member declares a channel count this device does not place.
    UnsupportedChannels {
        /// The declared channel count.
        channels: u16,
    },
    /// The asset holds no frames, so there is nothing to loop.
    NoSamples,
    /// A decoded value was NaN or infinite.
    NonFiniteSample,
}

impl PcmError {
    /// The stable machine-readable code a report or evidence record carries.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoChannels => "pcm_no_channels",
            Self::NoRate { .. } => "pcm_no_rate",
            Self::UnsupportedChannels { .. } => "pcm_unsupported_channels",
            Self::NoSamples => "pcm_no_samples",
            Self::NonFiniteSample => "pcm_non_finite_sample",
        }
    }
}

impl fmt::Display for PcmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoChannels => f.write_str("the asset declares no channel"),
            Self::NoRate { rate_hz } => write!(f, "the asset declares a rate of {rate_hz} Hz"),
            Self::UnsupportedChannels { channels } => write!(
                f,
                "the asset declares {channels} channels; only mono and stereo are placed"
            ),
            Self::NoSamples => f.write_str("the asset holds no sample frames"),
            Self::NonFiniteSample => f.write_str("a decoded sample value is not finite"),
        }
    }
}

impl std::error::Error for PcmError {}

/// Turns one sound-archive member into device-ready samples.
///
/// This is the production consumer of the F06-C decode: the member's own bytes
/// are decoded under its own WAVE declaration and normalized, and anything the
/// decoder or this device refuses is returned as a [`DeviceError`] carrying the
/// **refusal's own stable code**, so an evidence record can name which step
/// failed rather than only that something did.
pub fn sound_member_pcm(
    asset: &cs_assets::zbd::SoundAsset<'_>,
    context: &mut ParseContext,
) -> Result<PcmAudio, DeviceError> {
    let name = String::from_utf8_lossy(asset.name()).into_owned();
    let decoded = asset.decode(context).map_err(|error| {
        DeviceError::new(
            error.code(),
            format!("sound member {name:?} did not decode under its own header: {error}"),
        )
    })?;
    PcmAudio::from_decoded(&decoded).map_err(|error| {
        DeviceError::new(
            error.code(),
            format!("sound member {name:?} decoded to an unplayable shape: {error}"),
        )
    })
}

// ------------------------------------------------------------ sample source --

/// Where a device finds the samples a [`VoiceStart`] names.
///
/// The device never decodes and never opens a file: a load hands it the assets
/// it actually delivered, and an asset the library does not hold is **refused
/// by name** ([`CODE_SAMPLE_UNAVAILABLE`]) rather than played as silence.
///
/// # Why both methods take `&self`
///
/// The library is shared: the plugin hands the same object to the device and
/// to the world, and the population pass of the delivered load
/// (`super::samples::populate`) fills it *after* the device holds it. So a
/// lookup cannot borrow the map — it returns an owned [`PcmAudio`] handle,
/// which shares the samples behind an `Arc` and costs no copy of the decoded
/// member — and filling goes through [`SampleLibrary::replace_samples`], which
/// replaces the map wholesale rather than mutating entries in place.
pub trait SampleLibrary: fmt::Debug + Send + Sync {
    /// The decoded asset `id` names, when this library holds it.
    ///
    /// An **owned handle**, not a borrow: the samples themselves stay behind a
    /// shared `Arc`, so the caller holds the same decoded member without
    /// holding the map it was found in (see the trait note above).
    fn pcm(&self, id: &ContentId) -> Option<PcmAudio>;

    /// Replaces every entry with `samples`: the whole population pass of one
    /// delivered load, in one call.
    ///
    /// Replacing rather than merging is what makes a reload observable: the
    /// previous load's samples are unreachable the moment this returns, so a
    /// voice naming an asset of the replaced load is refused by name
    /// ([`CODE_SAMPLE_UNAVAILABLE`]) instead of playing what the old closure
    /// delivered.
    fn replace_samples(&self, samples: BTreeMap<ContentId, PcmAudio>);
}

/// Resource: the [`SampleLibrary`] this world's audible backend plays from,
/// which is the same object the delivered load's population pass fills.
///
/// The plugin inserts it when — and only when — it built the world's device
/// from a library ([`AudioPlugin::audible`](super::AudioPlugin::audible)), so
/// the device and the loader can never hold two different libraries: a world
/// that mixes to the recording stand-in has no library and nothing to fill,
/// and says so by having no such resource rather than by filling one nothing
/// reads.
#[derive(Resource, Clone)]
pub struct DeviceSampleLibrary(Arc<dyn SampleLibrary>);

impl DeviceSampleLibrary {
    /// Shares `library` between the device and the load's population pass.
    #[must_use]
    pub fn new(library: Arc<dyn SampleLibrary>) -> Self {
        Self(library)
    }

    /// The library the device plays from.
    #[must_use]
    pub fn library(&self) -> &Arc<dyn SampleLibrary> {
        &self.0
    }
}

impl fmt::Debug for DeviceSampleLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceSampleLibrary")
            .field("library", &self.0)
            .finish()
    }
}

/// A [`SampleLibrary`] held in memory, keyed by audio content id.
///
/// This is what a delivered load fills: each audio content id the load's
/// catalog declares and the load's assets contain gets the samples decoded from
/// its own member, and nothing else is reachable ([`super::samples`]).
///
/// The entries live behind a mutex rather than in the field itself: this is the
/// object the load's population pass writes through a shared handle while the
/// device reads it, and [`SampleLibrary::replace_samples`] is how the writing
/// happens — wholesale, so a reload never leaves a mixture of two closures.
#[derive(Debug, Default)]
pub struct InMemorySamples {
    entries: Mutex<BTreeMap<ContentId, PcmAudio>>,
}

impl Clone for InMemorySamples {
    fn clone(&self) -> Self {
        Self {
            entries: Mutex::new(
                self.entries
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone(),
            ),
        }
    }
}

impl InMemorySamples {
    /// An empty library: every voice is refused by name.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the samples of one audio content id.
    pub fn insert(&mut self, id: ContentId, audio: PcmAudio) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, audio);
    }

    /// How many assets this library holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Whether this library holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SampleLibrary for InMemorySamples {
    fn pcm(&self, id: &ContentId) -> Option<PcmAudio> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    fn replace_samples(&self, samples: BTreeMap<ContentId, PcmAudio>) {
        *self.entries.lock().unwrap_or_else(PoisonError::into_inner) = samples;
    }
}

// -------------------------------------------------------------- the device --

/// What one voice is playing.
struct AudibleVoice {
    player: Player,
    /// The constant-power pan the source applies per frame. Shared with the
    /// source because a rodio source is moved into the audio thread's mixer and
    /// cannot be borrowed again afterwards.
    pan: Arc<AtomicF32>,
}

/// A looping stereo voice over one decoded asset.
///
/// The source loops forever (`current_span_len` is `None`) because every voice
/// the mixer starts is a loop: a one-shot cue is F41-C's radio and music path,
/// which is driven by tick-completion rather than by a voice's own end.
///
/// A mono asset is spread to stereo **here** rather than by the device's
/// channel converter, so the pan below is a real left/right placement instead of
/// a gain applied to a signal that has not been split into channels yet. The
/// spread happens one frame at a time in [`Iterator::next`] rather than into a
/// second buffer at construction: a decoded member is up to a million samples,
/// and copying (then doubling) it per started voice would put megabytes of
/// allocation on the audio thread's start path for no gain.
///
/// Public because it is the boundary a headless machine can still check: the
/// pan law, the channel interleave, the mono spread and the loop seam are all
/// observable by pulling samples from this iterator, with no output hardware at
/// all. That is what makes them testable in CI rather than only on a machine
/// with a sound card.
pub struct LoopingVoice {
    /// The member's own interleaved samples, shared with the library rather than
    /// copied per voice.
    samples: Arc<Vec<f32>>,
    /// Whether the member stored one value per frame, so each frame owes a right
    /// channel this source has not emitted yet.
    mono: bool,
    rate_hz: u32,
    /// Index of the next stored value.
    cursor: usize,
    /// Whether the value at `cursor` still owes its right channel.
    pending_right: bool,
    pan: Arc<AtomicF32>,
    probe: Option<Arc<SampleProbe>>,
}

impl LoopingVoice {
    /// A voice looping `audio`, placed at `pan`.
    ///
    /// # Panics
    ///
    /// Never: every field a [`PcmAudio`] holds was validated when it was built.
    #[must_use]
    pub fn new(audio: &PcmAudio, pan: f32) -> Self {
        Self {
            samples: PcmAudio::shared_samples(audio),
            mono: audio.channels() == 1,
            rate_hz: audio.rate_hz(),
            cursor: 0,
            pending_right: false,
            pan: Arc::new(AtomicF32::new(pan)),
            probe: None,
        }
    }

    /// Attaches a probe that records every value this source emits.
    #[must_use]
    pub fn with_probe(mut self, probe: Arc<SampleProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    /// Moves the placement without stopping the voice.
    ///
    /// This is the [`cs_sim::audio_events::AudioDevice::update_voice`] path: a
    /// rodio source is moved into the audio thread's mixer and cannot be
    /// borrowed afterwards, so the placement travels through the shared cell the
    /// source reads per frame.
    pub fn set_pan(&self, pan: f32) {
        self.pan.store(pan);
    }

    /// The shared cell this source reads its placement from.
    ///
    /// A voice keeps the handle so a later update can move the placement after
    /// the source has been moved into the audio thread's mixer.
    fn pan_handle(&self) -> Arc<AtomicF32> {
        Arc::clone(&self.pan)
    }

    /// The constant-power gains for the current pan, in `0.0 ..= 1.0`.
    #[must_use]
    pub fn gains(&self) -> (f32, f32) {
        let pan = f64::from(self.pan.load()).clamp(-1.0, 1.0);
        let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
        (angle.cos() as f32, angle.sin() as f32)
    }
}

impl Iterator for LoopingVoice {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.samples.is_empty() {
            return None;
        }
        let (left, right) = self.gains();
        let value = self.samples[self.cursor];
        // The channel is read off the cursor **before** it advances: an
        // interleaved stereo member stores left at an even index and right at an
        // odd one, so reading it afterwards swaps every frame.
        let is_right = if self.mono {
            self.pending_right
        } else {
            self.cursor % 2 == 1
        };
        if self.mono {
            self.pending_right = !self.pending_right;
            if !self.pending_right {
                self.cursor += 1;
                if self.cursor == self.samples.len() {
                    self.cursor = 0;
                }
            }
        } else {
            self.cursor += 1;
            if self.cursor == self.samples.len() {
                self.cursor = 0;
            }
        }
        let out = if is_right {
            value * right
        } else {
            value * left
        };
        if let Some(probe) = &self.probe {
            probe.record(out);
        }
        Some(out)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, None)
    }
}

impl Source for LoopingVoice {
    fn current_span_len(&self) -> Option<usize> {
        // Infinite: the asset repeats until the voice is stopped.
        None
    }

    fn channels(&self) -> ChannelCount {
        // A mono member is spread to stereo by `next`, so the channel count this
        // source reports is always two and never zero.
        NonZero::new(2).expect("two is nonzero")
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.rate_hz).expect("a decoded asset's rate was validated as nonzero")
    }

    fn total_duration(&self) -> Option<Duration> {
        // A looping voice has no end, which rodio spells `None`.
        None
    }

    fn try_seek(&mut self, _to: Duration) -> Result<(), SeekError> {
        // A voice loops one asset from its start; there is no position inside
        // it a caller could ask for.
        Err(SeekError::NotSupported {
            underlying_source: std::any::type_name::<Self>(),
        })
    }
}

/// What the device's own engine pulled out of the decoded assets.
///
/// A tap on the **source** boundary, not on the audio hardware: it counts the
/// frames the output stream pulled and the peak and energy of the values it
/// pulled, which is what makes "the member played" checkable on a machine with
/// the `audio` capability. It is deliberately *not* a capture of what came out
/// of a speaker — that is the owner's `human_review` gate, and this workspace
/// cannot substitute for it (F41 non-negotiable behavior 5).
#[derive(Debug)]
pub struct SampleProbe {
    pulled: AtomicU64,
    peak: AtomicF32,
    energy: AtomicU64,
}

impl Default for SampleProbe {
    fn default() -> Self {
        Self {
            pulled: AtomicU64::new(0),
            peak: AtomicF32::new(0.0),
            energy: AtomicU64::new(0),
        }
    }
}

impl SampleProbe {
    /// An empty probe.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many sample values the engine pulled out of a decoded asset.
    #[must_use]
    pub fn pulled(&self) -> u64 {
        self.pulled.load(Ordering::Relaxed)
    }

    /// The largest absolute sample value pulled so far.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.peak.load()
    }

    /// The sum of the squares of the pulled values, scaled by `2^24` so the
    /// accumulator stays an exact integer.
    #[must_use]
    pub fn energy(&self) -> f64 {
        self.energy.load(Ordering::Relaxed) as f64 / 16_777_216.0
    }

    /// Whether anything at all was pulled.
    #[must_use]
    pub fn played(&self) -> bool {
        self.pulled() > 0
    }

    fn record(&self, value: f32) {
        self.pulled.fetch_add(1, Ordering::Relaxed);
        let magnitude = value.abs();
        // A monotonic maximum: only a value above the stored one stores.
        self.peak.raise_to(magnitude);
        let squared = f64::from(magnitude) * f64::from(magnitude) * 16_777_216.0;
        self.energy.fetch_add(squared as u64, Ordering::Relaxed);
    }
}

/// The one [`AudioDevice`] that opens real output hardware.
///
/// Open it with [`open_audible_device`], which applies the capability gate
/// first: a machine that does not declare `audio` never reaches the hardware
/// call at all, and gets a named [`DeviceError`] instead.
///
/// [`Self::unopened`] builds one **without** touching hardware. That is what
/// lets a headless machine exercise every command refusal — the property that
/// matters when there is no device is that the refusal is named, not that
/// something is audible.
pub struct AudibleDevice {
    library: Arc<dyn SampleLibrary>,
    probe: Option<Arc<SampleProbe>>,
    sink: Mutex<Option<MixerDeviceSink>>,
    /// Whether this device may ask the platform for a stream at all.
    ///
    /// [`Self::unopened`] clears it and [`open_audible_device`] sets it. The
    /// difference is not a stub: it is the difference between a world that was
    /// granted an output and a world that was refused one, and the refused world
    /// must not silently succeed on a later pass.
    opens_hardware: bool,
    voices: BTreeMap<DeviceVoiceId, AudibleVoice>,
    next_voice: u64,
}

impl fmt::Debug for AudibleDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudibleDevice")
            .field("open", &self.sink.lock().is_ok_and(|sink| sink.is_some()))
            .field("voices", &self.voices.len())
            .field("probed", &self.probe.is_some())
            .finish()
    }
}

impl AudibleDevice {
    /// A device with no output stream and no permission to ask for one.
    ///
    /// This is what a caller installs when no output was granted — and it is
    /// how a headless machine exercises every command refusal without touching
    /// hardware. The property that matters when there is no device is that the
    /// refusal is **named**: [`AudioDevice::open`] refuses with
    /// [`CODE_DEVICE_CLOSED`] rather than succeeding on a later attempt.
    #[must_use]
    pub fn unopened(library: Arc<dyn SampleLibrary>) -> Self {
        Self {
            library,
            probe: None,
            sink: Mutex::new(None),
            opens_hardware: false,
            voices: BTreeMap::new(),
            next_voice: 0,
        }
    }

    /// Attaches a probe that records what the engine pulls, and returns the
    /// handle that reads it.
    ///
    /// The probe must be attached before a voice starts: a source captures the
    /// handle when it is built, and a probe attached later would only see the
    /// voices started after it.
    #[must_use]
    pub fn with_probe(mut self, probe: Arc<SampleProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    /// How many voices this device currently holds.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// The voice ids this device holds, in id order.
    pub fn voice_ids(&self) -> impl Iterator<Item = DeviceVoiceId> + '_ {
        self.voices.keys().copied()
    }

    /// How far into its asset the engine has pulled `voice`, or `None` when
    /// this device does not own it.
    ///
    /// A position that advances is the device's own statement that the output
    /// stream consumed frames from that voice, which is what separates "a voice
    /// was started" from "the member played".
    #[must_use]
    pub fn played_position(&self, voice: DeviceVoiceId) -> Option<Duration> {
        self.voices.get(&voice).map(|held| held.player.get_pos())
    }
}

/// Opens the default output device, if this machine may play sound.
///
/// The order of the two refusals is the contract:
///
/// 1. **Capability first.** A machine that does not declare `audio` is refused
///    with [`CODE_CAPABILITY_ABSENT`] before any hardware call, so a headless
///    build fails by name instead of finding a device it was never granted.
/// 2. **Hardware second.** A machine that declares `audio` and still cannot open
///    a stream is refused with [`CODE_NO_OUTPUT_DEVICE`] and rodio's own
///    explanation.
///
/// # Errors
///
/// [`DeviceError`] with one of those two codes.
pub fn open_audible_device(
    library: Arc<dyn SampleLibrary>,
    declaration: &CapabilityDeclaration,
) -> Result<AudibleDevice, DeviceError> {
    if !declaration.contains(CapabilityClass::Audio) {
        let detail = match &declaration.malformed {
            Some(malformed) => {
                format!("the audio capability was not declared: {malformed}; nothing may be played")
            }
            None => format!(
                "the audio capability was not declared (CS_CAPABILITIES={:?}); nothing may be played",
                declaration.label()
            ),
        };
        return Err(DeviceError::new(CODE_CAPABILITY_ABSENT, detail));
    }
    let sink = DeviceSinkBuilder::open_default_sink().map_err(|error| {
        DeviceError::new(
            CODE_NO_OUTPUT_DEVICE,
            format!(
                "the machine declares the audio capability but no output device opened: {error}"
            ),
        )
    })?;
    Ok(AudibleDevice {
        library,
        probe: None,
        sink: Mutex::new(Some(sink)),
        opens_hardware: true,
        voices: BTreeMap::new(),
        next_voice: 0,
    })
}

impl AudioDevice for AudibleDevice {
    fn is_open(&self) -> bool {
        self.sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    fn open(&mut self) -> Result<(), DeviceError> {
        if self.is_open() {
            return Ok(());
        }
        if !self.opens_hardware {
            return Err(DeviceError::new(
                CODE_DEVICE_CLOSED,
                "this output was never granted: no stream is open and none may be requested",
            ));
        }
        let sink = DeviceSinkBuilder::open_default_sink().map_err(|error| {
            DeviceError::new(
                CODE_NO_OUTPUT_DEVICE,
                format!("no output device opened: {error}"),
            )
        })?;
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
        Ok(())
    }

    fn close(&mut self) {
        for (_, voice) in std::mem::take(&mut self.voices) {
            voice.player.stop();
        }
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    fn start_voice(&mut self, start: VoiceStart) -> Result<DeviceVoiceId, DeviceError> {
        // The asset is resolved before the open state so an asset the load never
        // delivered is named as such even on a device that happens to be shut.
        let Some(audio) = self.library.pcm(&start.asset) else {
            return Err(DeviceError::new(
                CODE_SAMPLE_UNAVAILABLE,
                format!("no decoded samples are registered for {}", start.asset),
            ));
        };
        let (gain, pan, pitch) = validate_voice(&start)?;
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(sink) = sink.as_mut() else {
            return Err(DeviceError::new(
                CODE_DEVICE_CLOSED,
                format!("cannot start {} on a closed output", start.asset),
            ));
        };
        let voice = DeviceVoiceId(self.next_voice);
        let source = LoopingVoice::new(&audio, pan);
        let pan_handle = source.pan_handle();
        let source = match self.probe.clone() {
            Some(probe) => source.with_probe(probe),
            None => source,
        };
        let player = Player::connect_new(sink.mixer());
        player.set_volume(gain);
        player.set_speed(pitch);
        player.append(source);
        self.next_voice += 1;
        self.voices.insert(
            voice,
            AudibleVoice {
                player,
                pan: pan_handle,
            },
        );
        Ok(voice)
    }

    fn stop_voice(&mut self, stop: VoiceStop) -> Result<(), DeviceError> {
        let Some(voice) = self.voices.remove(&stop.voice) else {
            return Err(DeviceError::new(
                CODE_UNKNOWN_VOICE,
                format!("voice {} is not sounding on this device", stop.voice.0),
            ));
        };
        voice.player.stop();
        Ok(())
    }

    fn update_voice(
        &mut self,
        voice: DeviceVoiceId,
        update: VoiceUpdate,
    ) -> Result<(), DeviceError> {
        if !self.is_open() {
            return Err(DeviceError::new(
                CODE_DEVICE_CLOSED,
                format!("cannot update voice {} on a closed output", voice.0),
            ));
        }
        let Some(held) = self.voices.get(&voice) else {
            return Err(DeviceError::new(
                CODE_UNKNOWN_VOICE,
                format!("voice {} is not sounding on this device", voice.0),
            ));
        };
        let (gain, pan, pitch) = validate_update(&update)?;
        held.pan.store(pan);
        held.player.set_volume(gain);
        held.player.set_speed(pitch);
        Ok(())
    }
}

/// Validates a start's mix values and converts them to the device's domain.
fn validate_voice(start: &VoiceStart) -> Result<(f32, f32, f32), DeviceError> {
    validate_mix(start.gain, start.pan, start.pitch, "start")
}

/// Validates an update's mix values and converts them to the device's domain.
fn validate_update(update: &VoiceUpdate) -> Result<(f32, f32, f32), DeviceError> {
    validate_mix(update.gain, update.pan, update.pitch, "update")
}

/// The one place a mix value crosses into the device's domain.
fn validate_mix(
    gain: f64,
    pan: f64,
    pitch: f64,
    what: &str,
) -> Result<(f32, f32, f32), DeviceError> {
    let refuse = |reason: String| DeviceError::new(CODE_INVALID_MIX, format!("{what} {reason}"));
    if !gain.is_finite() || gain < 0.0 {
        return Err(refuse(format!("has a gain of {gain}")));
    }
    if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err(refuse(format!("has a pan of {pan}")));
    }
    if !pitch.is_finite() || pitch <= 0.0 {
        return Err(refuse(format!("has a pitch of {pitch}")));
    }
    Ok((gain as f32, pan as f32, pitch as f32))
}

// ------------------------------------------------------- refusing device --

/// A device that refuses every command with one named reason.
///
/// A world that asked for the audible backend on a machine that cannot play
/// installs this instead of falling back to
/// [`RecordingAudioDevice`](cs_sim::audio_events::RecordingAudioDevice). The
/// difference is the whole point: a recording device makes the mixer report
/// success while nothing is audible, so a capability failure would be
/// indistinguishable from a mission with no audio. This one reports
/// [`CODE_CAPABILITY_ABSENT`] — or whatever the gate said — on every open,
/// start, stop and update, so
/// [`cs_sim::audio_events::MixerReport::refusals`] names it and nothing goes
/// quietly quiet.
#[derive(Debug)]
pub struct RefusingAudioDevice {
    error: DeviceError,
}

impl RefusingAudioDevice {
    /// A device that refuses everything with `error`.
    #[must_use]
    pub fn new(error: DeviceError) -> Self {
        Self { error }
    }

    /// The one refusal this device reports.
    #[must_use]
    pub const fn error(&self) -> &DeviceError {
        &self.error
    }
}

impl AudioDevice for RefusingAudioDevice {
    fn is_open(&self) -> bool {
        false
    }

    fn open(&mut self) -> Result<(), DeviceError> {
        Err(self.error.clone())
    }

    fn close(&mut self) {}

    fn start_voice(&mut self, _start: VoiceStart) -> Result<DeviceVoiceId, DeviceError> {
        Err(self.error.clone())
    }

    fn stop_voice(&mut self, _stop: VoiceStop) -> Result<(), DeviceError> {
        Err(self.error.clone())
    }

    fn update_voice(
        &mut self,
        _voice: DeviceVoiceId,
        _update: VoiceUpdate,
    ) -> Result<(), DeviceError> {
        Err(self.error.clone())
    }
}

// ------------------------------------------------------------ the backend --

/// Which device a world mixes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioBackendKind {
    /// Real output hardware, open and playing.
    Audible,
    /// The audible backend was asked for and refused by name; every command
    /// reports that refusal.
    Refused,
    /// No audible backend was asked for: the recording stand-in is installed,
    /// which records commands and makes no sound.
    StandIn,
}

/// Resource: which backend this world mixes to, and why.
///
/// The answer to "is this world audible?" is a world resource rather than a
/// type, because a headless machine and an audio-capable one run the same code
/// and must report different facts. A world holding [`AudioBackendKind::StandIn`]
/// or [`AudioBackendKind::Refused`] is **not** silent by accident: the first is
/// a deliberate stand-in for a machine that never asked for sound, and the
/// second names the refusal in [`Self::refusal`] and reports it on every mixer
/// pass.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct AudioBackendLog {
    /// The device this world mixes to.
    pub kind: AudioBackendKind,
    /// The canonical spelling of the capability classes this machine declared.
    pub declared: String,
    /// The named refusal, when the audible backend was asked for and refused.
    pub refusal: Option<AudioBackendRefusal>,
}

impl AudioBackendLog {
    /// A world mixing to real output hardware.
    #[must_use]
    pub fn audible(declaration: &CapabilityDeclaration) -> Self {
        Self {
            kind: AudioBackendKind::Audible,
            declared: declaration.label(),
            refusal: None,
        }
    }

    /// A world whose audible backend was refused.
    #[must_use]
    pub fn refused(declaration: &CapabilityDeclaration, error: &DeviceError) -> Self {
        Self {
            kind: AudioBackendKind::Refused,
            declared: declaration.label(),
            refusal: Some(classify_refusal(declaration, error)),
        }
    }

    /// A world mixing to the recording stand-in.
    #[must_use]
    pub fn stand_in(declaration: &CapabilityDeclaration) -> Self {
        Self {
            kind: AudioBackendKind::StandIn,
            declared: declaration.label(),
            refusal: None,
        }
    }

    /// Whether this world is really playing sound.
    #[must_use]
    pub const fn is_audible(&self) -> bool {
        matches!(self.kind, AudioBackendKind::Audible)
    }
}

/// Why the audible backend was not installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioBackendRefusal {
    /// This machine does not declare the `audio` capability.
    CapabilityAbsent {
        /// What it declared instead.
        declared: String,
    },
    /// `$CS_CAPABILITIES` did not parse, so nothing may be treated as declared.
    CapabilityUnparsable {
        /// The raw list and why it did not parse.
        detail: String,
    },
    /// The capability was declared but no output device opened.
    Open {
        /// The refusal's stable code.
        code: &'static str,
        /// The refusal's detail.
        detail: String,
    },
}

/// Classifies why [`open_audible_device`] refused.
///
/// The declaration, not the error's wording, is what separates "this machine
/// declared no audio capability" from "this machine's declaration is a typo":
/// a report has to be able to say which, and reading it out of a message
/// string would make a diagnostic depend on prose.
#[must_use]
pub fn classify_refusal(
    declaration: &CapabilityDeclaration,
    error: &DeviceError,
) -> AudioBackendRefusal {
    if error.code != CODE_CAPABILITY_ABSENT {
        return AudioBackendRefusal::Open {
            code: error.code,
            detail: error.detail.clone(),
        };
    }
    match &declaration.malformed {
        Some(detail) => AudioBackendRefusal::CapabilityUnparsable {
            detail: format!("{detail}: {}", error.detail),
        },
        None => AudioBackendRefusal::CapabilityAbsent {
            declared: declaration.label(),
        },
    }
}

impl AudioBackendRefusal {
    /// The stable machine-readable code this refusal carries.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::CapabilityAbsent { .. } | Self::CapabilityUnparsable { .. } => {
                CODE_CAPABILITY_ABSENT
            }
            Self::Open { code, .. } => code,
        }
    }

    /// The CLI exit code this refusal maps to.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        audibility_exit_code(&DeviceError::new(self.code(), String::new()))
    }
}

impl fmt::Display for AudioBackendRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityAbsent { declared } => write!(
                f,
                "the {CODE_CAPABILITY_ABSENT} refusal: this machine declared {declared:?}"
            ),
            Self::CapabilityUnparsable { detail } => {
                write!(f, "the {CODE_CAPABILITY_ABSENT} refusal: {detail}")
            }
            Self::Open { code, detail } => write!(f, "{code}: {detail}"),
        }
    }
}
