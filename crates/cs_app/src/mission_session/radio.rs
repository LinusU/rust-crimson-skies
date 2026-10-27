//! M01's radio dialogue: the objective program's sound-group cues resolved to
//! voice lines and queued in the audio session (Rally #1438,
//! `M01-PLAY-RADIO`).
//!
//! # What fires a line, measured
//!
//! The mission program spells two sound keys per objective block
//! (`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`,
//! static code evidence from the owner's executable):
//!
//! * `WAKEUP_SOUND_GROUP` — played through the *woken* channel when the block
//!   wakes (`0x469ce0` → `0x46cc70`), and
//! * `COMPLETED_SOUND_GROUP` — played through the *completed* channel when the
//!   block completes (`0x46a95b` → `0x46cc50`).
//!
//! [`EngineObjectives`](cs_content::objectives_engine) models both
//! transitions and emits a
//! [`CueEmission`] for each one
//! (including the mission-level `RoleJingle`, whose group the record does not
//! name). This module is the consumer: it resolves the spelled group name into
//! the voice files the installation stores and offers them to [`AudioSession`]'s
//! radio queue.
//!
//! # Where a name resolves, measured from the installation
//!
//! `sounds.zrd` (a member of `ZBD/zrdr.zbd`) is the group table, read here
//! through the same reader-archive path the launch sound uses. Two sections
//! carry the spellings this consumer honours:
//!
//! * **`SETS`** — rows `[<group name>, <member>.wav, …]`; the second element
//!   is the archive member the name plays.
//! * **`SOUND_GROUPS`** — records `[<group name>, <child>…]`; a child is a
//!   group name (or a one-element list holding one), and a group expands
//!   through the sets until it reaches members.
//!
//! Two leaves are classified, never guessed: a child spelled `MUSIC` is a
//! **music state request** (the executable's own seven `music_*_sg` names map
//! to `MUSIC`), and a `<name>.wav` child is an archive member. A music group
//! plays no radio line at all — its children are that state's tracks, not
//! spoken lines.
//!
//! # What is measured, and what stays designed
//!
//! * **Measured:** the group names a mission's program spells, the set/group
//!   table they resolve through, the member each set row names, each member's
//!   decoded length (decoded through `cs_formats`' own sample decoder and
//!   counted in frames), and the `VO_<mission>_<speaker>_<index>` spelling of
//!   the voice members the archive stores.
//! * **Designed, not measured:** the radio *priority* and interruptibility
//!   every line is offered with (F41-C records the original's queue rules as
//!   unmeasured), and the fact that a line's `speaker` is displayed as the
//!   `<speaker>` token its own file name spells. No original executable ran
//!   here, so nothing in this module is `verified_original`.
//! * **Absent, not invented:** no subtitle text. The installation's reader
//!   records carry no prose for a voice line (the longest string in the root
//!   reader is a path), and no key maps a line to text, so every line is
//!   queued without a caption rather than with a guessed one.
//!
//! # The publish step (M01-RADIO-SAMPLE-WIRE, #1466)
//!
//! A queued line names a [`ContentKind::Dialogue`](cs_types::content::ContentKind)
//! id, but the device only plays what its [`DeviceSampleLibrary`] holds, and
//! the load's population pass decodes declared *sound* members only. So the
//! prepare step keeps the PCM it already decoded for each member it queues
//! ([`MissionRadio::voices`]) and [`publish_prepared_voices`] hands exactly
//! those members to the device library under the id the line names, through
//! [`crate::audio::publish_voice_members`]. It runs **after** the same load's
//! population pass, which is what installs the audio session and fills the
//! library wholesale — see [`install_radio_voice_publication`] and
//! `crates/cs_app/src/audio/voice.rs`. The identity rule and route (b) are the
//! owner's designed decision (task #1465 note of 2026-10-15): the installation
//! states no cue class (F14-D.7), so nothing here is `verified_original`, and
//! this is not evidence that a person hears anything — the F41-D listening
//! test stays the owner's gate.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use bevy::app::App;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{PreUpdate, Resource, World};
use cs_content::objectives_engine::{CueEmission, CueKind, EngineObjectives};
use cs_content::stunts::ZrdValue;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    SampleFormat, ZbdProbe, decode_sound_sample, dispatch, read_sound_archive,
    read_version_one_index,
};
use cs_sim::audio_events::{AudioEventId, RadioLine, RadioLineError};
use cs_types::content::ContentId;

use super::host::MissionHost;
use super::launch_sound::{LAUNCH_SOUND_CONTAINER, reader_member};
use crate::audio::{
    AudioHandoffLog, AudioSession, DecodedMusicMember, DecodedVoiceMember, DeviceSampleLibrary,
    PcmAudio, insert_audio_session, publish_voice_members,
};
use crate::loading::LoadIdentity;
use crate::physics::BASELINE_FIXED_HZ;

/// The reader archive `sounds.zrd` is a member of.
pub const SOUND_GROUPS_ARCHIVE: &str = "ZBD/zrdr.zbd";

/// The member holding the group → member table.
pub const SOUND_GROUPS_MEMBER: &str = "sounds.zrd";

/// The archive every voice member this module plays is read from — the same
/// **labelled choice** the launch sound makes, because which of
/// `soundsL`/`soundsM` the original opens is unmeasured (F14-D.7).
pub const RADIO_SOUND_CONTAINER: &str = LAUNCH_SOUND_CONTAINER;

/// The leaf `SOUND_GROUPS` spells for a music state request (the executable's
/// own `music_*_sg` names resolve to it).
const MUSIC_LEAF: &str = "MUSIC";

/// The section of `sounds.zrd` holding `[<name>, <member>.wav, …]` rows.
const SETS_KEY: &str = "SETS";

/// The section of `sounds.zrd` holding `[<name>, <child>…]` records.
const SOUND_GROUPS_KEY: &str = "SOUND_GROUPS";

/// The file-name prefix the installation spells on its spoken lines
/// (`VO_c2-NW-m1_Jack_7.wav`, 2 299 members of the two sound archives).
const VOICE_PREFIX: &str = "VO_";

/// The radio priority every mission line is offered with.
///
/// **Designed:** `docs/findings/2026-10-01-f41-c-radio-queue-music-and-device-loss.md`
/// records the original's radio priority and interrupt rules as unmeasured, so
/// one level for every line keeps the queue in emission order instead of
/// inventing a hierarchy.
const RADIO_PRIORITY: u8 = 1;

/// Whether a mission line may be cut off by a strictly higher priority line.
/// **Designed**, for the same reason as [`RADIO_PRIORITY`].
const RADIO_INTERRUPTIBLE: bool = true;

/// Why the group table or one of its members would not read.
#[derive(Debug)]
pub enum RadioError {
    /// A production reader refused the source bytes.
    Read(String),
    /// The table or a member it names does not spell what the measurement
    /// requires.
    Unmeasured(String),
}

impl std::fmt::Display for RadioError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(reason) => write!(formatter, "the radio source refuses: {reason}"),
            Self::Unmeasured(reason) => {
                write!(formatter, "the radio source is not measured: {reason}")
            }
        }
    }
}

impl std::error::Error for RadioError {}

/// Why one cue's group did not become radio lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadioRefusal {
    /// The mission-level role jingle: `CueKind::RoleJingle` carries no group,
    /// because the record does not name the mission-level group it plays.
    UnnamedRoleJingle,
    /// No prepared group answers this name — the cue named a group this
    /// mission's declarations never spelled.
    GroupNotPrepared {
        /// The group the cue named.
        group: String,
    },
    /// The group was declared, but its expansion refused: a child is a name
    /// nothing in `sounds.zrd` declares.
    GroupUnresolved {
        /// The group whose expansion refused.
        group: String,
        /// The table's own reason.
        reason: String,
    },
    /// The member the set row names is not in the container the labelled
    /// choice reads.
    MemberAbsent {
        /// The member nothing declares.
        member: String,
    },
    /// The member's spelling would not become a content id, so the line has
    /// no identity to be looked up under.
    Identity {
        /// The member whose spelling refused.
        member: String,
        /// The id grammar's own reason.
        reason: String,
    },
    /// The group expands, but to no voice member at all.
    NoVoiceMember {
        /// The group that resolved to nothing playable here.
        group: String,
    },
    /// The group spelled `MUSIC`, but is not one of the executable's seven
    /// built-in `music_*_sg` states: no measured table exists to select a
    /// track from, and no track is guessed (Rally #1463, `M01-PLAY-MUSIC`).
    MusicStateUnknown {
        /// The music group that is no built-in state.
        group: String,
    },
    /// The track the music selector picked is in neither archive of the
    /// labelled container choice (`ZBD/soundsh.zbd` preferred,
    /// `ZBD/soundsl.zbd` otherwise), so no identity can be minted for it.
    MusicMemberAbsent {
        /// The member neither archive declares.
        member: String,
    },
    /// The picked track member's spelling would not become a content id, so
    /// the request has no identity to be looked up under.
    MusicIdentity {
        /// The member whose spelling refused.
        member: String,
        /// The id grammar's own reason.
        reason: String,
    },
    /// A child leaf is neither `MUSIC`, a member, nor another group this
    /// table holds: the record spelled a word this consumer does not
    /// resolve, and it is named rather than dropped.
    UnresolvedLeaf {
        /// The group being expanded.
        group: String,
        /// The leaf that resolved to nothing.
        leaf: String,
    },
    /// The member is not a spoken line under the archive's own `VO_`
    /// spelling, so there is no line (and no speaker) to queue.
    NotAVoiceMember {
        /// The member the set row named.
        member: String,
    },
    /// The member's name does not spell `VO_<mission>_<speaker>_<index>`, so
    /// no speaker can be read from it.
    NoSpeaker {
        /// The member whose spelling refused.
        member: String,
    },
    /// The member would not decode under its own WAVE declaration, so its
    /// length — and therefore the line's duration — is unknown and the line
    /// is never queued with a guessed one. Also the member's own decode yields
    /// no shape the device can hold (no frames at all, or a channel count the
    /// device cannot place): the line could never be delivered, so it is
    /// refused by name rather than queued as an asset no device can ever play
    /// (M01-RADIO-SAMPLE-WIRE, #1466).
    Undecodable {
        /// The member that would not decode.
        member: String,
        /// The decoder's own reason.
        reason: String,
    },
    /// The member decoded to a shape with no rate or channel count to count
    /// its frames against.
    NoDuration {
        /// The member whose length could not be counted.
        member: String,
    },
    /// [`RadioLine::try_new`] refused the built line, quoting its own typed
    /// reason.
    Line {
        /// The member the line was built from.
        member: String,
        /// The line builder's own message.
        reason: String,
    },
}

impl std::fmt::Display for RadioRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnnamedRoleJingle => write!(
                formatter,
                "the role jingle's group is mission-level and the record does not name it"
            ),
            Self::GroupNotPrepared { group } => {
                write!(formatter, "no prepared sound group answers {group:?}")
            }
            Self::GroupUnresolved { group, reason } => {
                write!(
                    formatter,
                    "sound group {group:?} does not resolve: {reason}"
                )
            }
            Self::MemberAbsent { member } => {
                write!(
                    formatter,
                    "{RADIO_SOUND_CONTAINER} declares no member {member}"
                )
            }
            Self::Identity { member, reason } => {
                write!(formatter, "{member:?} has no dialogue identity: {reason}")
            }
            Self::NoVoiceMember { group } => {
                write!(
                    formatter,
                    "sound group {group:?} resolves to no voice member"
                )
            }
            Self::MusicStateUnknown { group } => {
                write!(
                    formatter,
                    "sound group {group:?} spells MUSIC but is no built-in music_*_sg state"
                )
            }
            Self::MusicMemberAbsent { member } => {
                write!(
                    formatter,
                    "neither {} nor {} declares member {member}",
                    crate::audio::PREFERRED_MUSIC_CONTAINER,
                    crate::audio::FALLBACK_MUSIC_CONTAINER,
                )
            }
            Self::MusicIdentity { member, reason } => {
                write!(formatter, "{member:?} has no music identity: {reason}")
            }
            Self::UnresolvedLeaf { group, leaf } => write!(
                formatter,
                "sound group {group:?} has a leaf {leaf:?} that is neither MUSIC, a member nor a group"
            ),
            Self::NotAVoiceMember { member } => write!(
                formatter,
                "{member:?} is not a {VOICE_PREFIX} member, so it is not a spoken line"
            ),
            Self::NoSpeaker { member } => write!(
                formatter,
                "{member:?} does not spell {VOICE_PREFIX}<mission>_<speaker>_<index>"
            ),
            Self::Undecodable { member, reason } => {
                write!(formatter, "{member:?} does not decode: {reason}")
            }
            Self::NoDuration { member } => write!(
                formatter,
                "{member:?} declares no rate or channel count to count its length against"
            ),
            Self::Line { member, reason } => write!(formatter, "{member:?}: {reason}"),
        }
    }
}

/// What one cue produced, in the order the host offered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadioLineOutcome {
    /// One voice member was offered to the audio session's radio queue. The
    /// queue's own verdict — a full pending queue, a duplicate id, a foreign
    /// session — arrives as a `RadioEvent` on the session and is named by the
    /// mixer's radio report, never dropped.
    Queued {
        /// The group the cue named.
        group: String,
        /// The archive member the line plays.
        member: String,
        /// The speaker the member's own name spells.
        speaker: String,
        /// The line's measured length, in fixed simulation ticks.
        duration_ticks: u64,
    },
    /// The group is a music state request: [`MissionRadio::plan`] answers a
    /// cue with this classification. It is what a plan is, never what an
    /// offer produced — [`offer_cues`] turns it into [`Self::MusicRequested`]
    /// or a refusal, so this variant does not appear in offered outcomes.
    Music {
        /// The music group the cue named.
        group: String,
    },
    /// The music state request selected one track and asked the audio
    /// session's [`MusicDirector`](cs_sim::audio_events::MusicDirector) for
    /// it: the owner-ruled track-level identity under the measured selection
    /// rule (Rally #1463, `M01-PLAY-MUSIC`).
    MusicRequested {
        /// The music group the cue named.
        group: String,
        /// The slot the state's measured rule picked.
        slot: usize,
        /// The track member that slot names in the state's built-in table.
        member: String,
        /// The [`ContentKind::Music`](cs_types::content::ContentKind) id the
        /// request carried — `crate::audio::music_member_id` over the
        /// labelled container's member.
        asset: ContentId,
        /// What the session's music director answered.
        outcome: cs_sim::audio_events::MusicOutcome,
    },
    /// The cue reached no audio session, so nothing was queued for it.
    NoAudioSession {
        /// The group the cue named, when it spelled one.
        group: Option<String>,
    },
    /// The cue's group did not become lines, quoting the typed refusal.
    Refused {
        /// The group the cue named, when it spelled one.
        group: Option<String>,
        /// Why nothing was queued.
        refusal: RadioRefusal,
    },
}

/// One voice member as the prepare step measured it: identity, speaker and
/// length, all read from the installation before any window exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedLine {
    /// The archive member this line plays.
    pub member: String,
    /// The speaker, read from the member's own
    /// `VO_<mission>_<speaker>_<index>` spelling.
    pub speaker: String,
    /// The dialogue content id the member derives
    /// ([`cs_types::content::ContentKind::Dialogue`] over F14-D.7's
    /// `install_file_key` spelling).
    pub asset: ContentId,
    /// The decoded length in fixed simulation ticks, at least one.
    pub duration_ticks: u64,
}

/// One resolved sound group: the members it plays, whether it is a music
/// state request, the `DYNAMIC_WEIGHTS` factor a music group spells, and the
/// leaves the table could not resolve.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedGroup {
    /// The archive members the group reaches, in the spelled order.
    pub members: Vec<String>,
    /// Whether the group spelled the `MUSIC` leaf.
    pub music: bool,
    /// The `DYNAMIC_WEIGHTS` factor the group spells beside `MUSIC` — the
    /// post-draw decay the measured selector multiplies the chosen weight by
    /// (`docs/findings/2026-10-16-m01-play-music-state-requests.md` §3), when
    /// the record spells one.
    pub dynamic_weights: Option<f32>,
    /// The children that resolved to nothing, in the spelled order — named
    /// when the cue fires, never silently dropped.
    pub unresolved: Vec<String>,
}

/// The mission's sound-group cues, resolved once at content preparation.
///
/// Everything here is read before a window or an app exists: the group table
/// from `sounds.zrd`, each requested group's expansion, and each reached
/// member's decoded length from the sound archive the labelled container
/// choice names.
#[derive(Debug, Default, Clone)]
pub struct MissionRadio {
    /// The groups this mission's declarations name, resolved.
    groups: BTreeMap<String, ResolvedGroup>,
    /// The members those groups reach, prepared.
    lines: BTreeMap<String, PreparedLine>,
    /// The PCM the prepare step decoded for each prepared member, in member
    /// order: exactly what [`publish_prepared_voices`] hands to the device's
    /// [`DeviceSampleLibrary`] under the id the member's line names.
    voices: Vec<DecodedVoiceMember>,
    /// Every track a prepared music group's table can select, by the id its
    /// cue names and the archive it lives in. Nothing is decoded here: the
    /// mixer decodes a track the first time a music state request starts it
    /// ([`MissionRadio::music_loader`], Rally #1551).
    music_tracks: Vec<MusicTrackSource>,
    /// The installation the tracks are decoded from on demand.
    install_root: PathBuf,
    /// What refused to prepare, keyed by the group or member name it
    /// refused: an expansion that would not resolve, a member the container
    /// does not hold, a member that would not decode. A cue naming one of
    /// these answers with the refusal here rather than with a line — and
    /// never with silence.
    failures: BTreeMap<String, RadioRefusal>,
    /// The install-relative spelling the members were read from.
    container: String,
    /// For every track a prepared music group's built-in table can select,
    /// the archive that member lives in under the labelled container choice:
    /// [`crate::audio::PREFERRED_MUSIC_CONTAINER`] when it declares the
    /// member, [`crate::audio::FALLBACK_MUSIC_CONTAINER`] otherwise. A table
    /// member neither archive declares is absent — its pick is refused at
    /// cue time ([`RadioRefusal::MusicMemberAbsent`]), never minted under a
    /// guessed container (Rally #1463, `M01-PLAY-MUSIC`).
    music_containers: BTreeMap<String, String>,
}

impl MissionRadio {
    /// A radio that answers no cue: the composition for a stage that carries
    /// no audio (the synthetic acceptance stages).
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// The install-relative container the members are read from.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The groups this radio prepared, in name order.
    pub fn groups(&self) -> impl Iterator<Item = &str> {
        self.groups.keys().map(String::as_str)
    }

    /// The expansion one prepared group resolved to, when this radio read it:
    /// the members it reaches in the spelled order, whether it spelled the
    /// `MUSIC` leaf (a **music state request**, not an ordinary cue), and the
    /// children nothing declared.
    ///
    /// This reads the same [`ResolvedGroup`] [`Self::plan`] answers a cue
    /// from; it exists so a consumer can ask *what* a music state request
    /// reaches without replaying the table.
    #[must_use]
    pub fn resolved(&self, group: &str) -> Option<&ResolvedGroup> {
        self.groups.get(group)
    }

    /// The archive `member` lives in under the labelled container choice —
    /// `ZBD/soundsh.zbd` preferred, `ZBD/soundsl.zbd` otherwise — when the
    /// prepare step found the member there.
    ///
    /// Every track a prepared music group's built-in table can select is
    /// listed ([`Self::read`] enumerates them); `None` names a track neither
    /// archive declares, which the cue that selects it answers with
    /// [`RadioRefusal::MusicMemberAbsent`].
    #[must_use]
    pub fn music_container(&self, member: &str) -> Option<&str> {
        self.music_containers.get(member).map(String::as_str)
    }

    /// What this radio's resolved music groups arm a
    /// [`crate::audio::MusicSelector`] with: each group's child count — the
    /// resolved member count, the domain the measured draw walks — and the
    /// `DYNAMIC_WEIGHTS` factor it spells, when it spells one (the measured
    /// `[group+8]` flag: unspelled groups skip the whole post-draw block).
    #[must_use]
    pub fn music_arms(&self) -> Vec<crate::audio::MusicArm> {
        self.groups
            .iter()
            .filter(|(_, resolved)| resolved.music)
            .map(|(group, resolved)| crate::audio::MusicArm {
                group: group.clone(),
                children: resolved.members.len(),
                factor: resolved.dynamic_weights,
            })
            .collect()
    }

    /// The members this radio prepared, in member order.
    pub fn prepared(&self) -> impl Iterator<Item = &PreparedLine> {
        self.lines.values()
    }

    /// The PCM the prepare step decoded for each prepared member, in member
    /// order: exactly the members [`publish_prepared_voices`] publishes into
    /// the device library, each under the dialogue id the line that names it
    /// carries.
    #[must_use]
    pub fn voices(&self) -> &[DecodedVoiceMember] {
        &self.voices
    }

    /// The decoded music tracks, in container then member order: exactly the
    /// tracks [`publish_prepared_voices`] publishes under their music ids.
    #[must_use]
    pub fn music_tracks(&self) -> &[MusicTrackSource] {
        &self.music_tracks
    }

    /// Decodes the music track `id` names from the container the prepare step
    /// found it in, at this moment.
    ///
    /// # Errors
    ///
    /// [`RadioRefusal`] when `id` names no selectable track, or the track
    /// will not decode.
    pub fn decode_music(&self, id: &ContentId) -> Result<DecodedMusicMember, RadioRefusal> {
        let source = self
            .music_tracks
            .iter()
            .find(|track| &track.id == id)
            .ok_or_else(|| RadioRefusal::MusicMemberAbsent {
                member: id.to_string(),
            })?;
        let member = source.member.clone();
        let mut failures = BTreeMap::new();
        let wanted = BTreeSet::from([member.clone()]);
        let mut decoded = decode_members(
            &self.install_root,
            &source.container,
            &wanted,
            &mut failures,
        )
        .map_err(|error| RadioRefusal::Undecodable {
            member: member.clone(),
            reason: error.to_string(),
        })?;
        let Some(DecodedMember { pcm, .. }) = decoded.remove(&member) else {
            return Err(failures
                .remove(&member)
                .unwrap_or(RadioRefusal::MusicMemberAbsent { member }));
        };
        DecodedMusicMember::new(&source.container, &member, pcm).map_err(|error| {
            RadioRefusal::MusicIdentity {
                member,
                reason: error.to_string(),
            }
        })
    }

    /// The loader the mixer decodes requested tracks through: a copy of this
    /// radio's track table and installation path.
    #[must_use]
    pub fn music_loader(&self) -> crate::audio::LazyMusicTracks {
        crate::audio::LazyMusicTracks::new(std::sync::Arc::new(self.clone()))
    }

    /// What refused to prepare, keyed by the group or member it refused.
    pub fn failures(&self) -> impl Iterator<Item = (&str, &RadioRefusal)> {
        self.failures
            .iter()
            .map(|(name, refusal)| (name.as_str(), refusal))
    }

    /// Reads `groups` out of the installation: the table from `sounds.zrd`,
    /// each group's expansion, and each reached member's decoded length.
    ///
    /// `groups` are the sound-group names the mission's own declarations
    /// spell — the only names its cues can ever name — so nothing else is
    /// resolved and no other mission's voices are read.
    ///
    /// # Errors
    ///
    /// [`RadioError`] naming the refusing source: a table that does not read,
    /// a group that reaches a name nothing declares, or a member that would
    /// not decode (its length is the line's duration, so an undecodable
    /// member is never queued with a guessed one).
    pub fn read(
        install_root: &Path,
        groups: impl IntoIterator<Item = String>,
    ) -> Result<Self, RadioError> {
        let table = SoundGroupTable::read(install_root)?;
        let container = RADIO_SOUND_CONTAINER.to_owned();

        let mut resolved: BTreeMap<String, ResolvedGroup> = BTreeMap::new();
        let mut members: BTreeSet<String> = BTreeSet::new();
        let mut failures: BTreeMap<String, RadioRefusal> = BTreeMap::new();
        for group in groups {
            match table.expand(&group) {
                Ok(expansion) => {
                    if !expansion.music {
                        members.extend(expansion.members.iter().cloned());
                    }
                    resolved.insert(group, expansion);
                }
                Err(error) => {
                    // The record spells a name the table cannot honour: the
                    // group stays out of the resolution and answers its cue
                    // with this refusal instead of a line.
                    let reason = error.to_string();
                    failures.insert(
                        group.clone(),
                        RadioRefusal::GroupUnresolved { group, reason },
                    );
                }
            }
        }

        // The music half of the same prepare step: every track a resolved
        // music group's built-in table can select gets its container under
        // the labelled choice — `soundsh` preferred, `soundsl` otherwise —
        // and a track in neither is recorded as absent so the cue that
        // selects it is refused by name rather than minted under a guessed
        // container (the owner ruling's container rule, #1463).
        let mut music_containers = BTreeMap::new();
        let mut music_tracks_out = Vec::new();
        let music_tracks: BTreeSet<String> = resolved
            .iter()
            .filter(|(_, expansion)| expansion.music)
            .filter_map(|(group, _)| crate::audio::music_state_spec(group))
            .flat_map(|spec| spec.tracks.iter().map(|track| (*track).to_owned()))
            .collect();
        if !music_tracks.is_empty() {
            let preferred =
                sound_archive_members(install_root, crate::audio::PREFERRED_MUSIC_CONTAINER)?;
            let fallback =
                sound_archive_members(install_root, crate::audio::FALLBACK_MUSIC_CONTAINER)?;
            for track in music_tracks {
                if preferred.contains(&track) {
                    music_containers
                        .insert(track, crate::audio::PREFERRED_MUSIC_CONTAINER.to_owned());
                } else if fallback.contains(&track) {
                    music_containers
                        .insert(track, crate::audio::FALLBACK_MUSIC_CONTAINER.to_owned());
                } else {
                    failures.insert(
                        track.clone(),
                        RadioRefusal::MusicMemberAbsent { member: track },
                    );
                }
            }
        }

        // Each selectable track gets its music id here, from its spelling; its
        // samples are decoded when a request first starts it (Rally #1551).
        for (track, archive) in &music_containers {
            match crate::audio::music_member_id(archive, track) {
                Ok(id) => music_tracks_out.push(MusicTrackSource {
                    id,
                    container: archive.clone(),
                    member: track.clone(),
                }),
                Err(error) => {
                    failures.insert(
                        track.clone(),
                        RadioRefusal::MusicIdentity {
                            member: track.clone(),
                            reason: error.to_string(),
                        },
                    );
                }
            }
        }

        let decoded_members = decode_members(install_root, &container, &members, &mut failures)?;
        let mut lines = BTreeMap::new();
        let mut voices = Vec::new();
        for member in members {
            let Some(DecodedMember {
                pcm,
                duration_ticks,
            }) = decoded_members.get(&member).cloned()
            else {
                continue;
            };
            let speaker = match speaker_of(&member) {
                Ok(speaker) => speaker,
                Err(refusal) => {
                    failures.insert(member.clone(), refusal);
                    continue;
                }
            };
            let asset = match dialogue_content_id(&container, &member) {
                Ok(asset) => asset,
                Err(error) => {
                    failures.insert(
                        member.clone(),
                        RadioRefusal::Identity {
                            member: member.clone(),
                            reason: error.to_string(),
                        },
                    );
                    continue;
                }
            };
            // The voice carries the same dialogue id `asset` spells — both go
            // through [`crate::audio::voice_member_id`] — so the line a cue
            // queues and the samples the publish step inserts name one
            // identity.
            let voice = match DecodedVoiceMember::new(&container, &member, pcm) {
                Ok(voice) => voice,
                Err(error) => {
                    failures.insert(
                        member.clone(),
                        RadioRefusal::Identity {
                            member: member.clone(),
                            reason: error.to_string(),
                        },
                    );
                    continue;
                }
            };
            lines.insert(
                member.clone(),
                PreparedLine {
                    member,
                    speaker,
                    asset,
                    duration_ticks,
                },
            );
            voices.push(voice);
        }

        Ok(Self {
            groups: resolved,
            lines,
            voices,
            music_tracks: music_tracks_out,
            install_root: install_root.to_path_buf(),
            failures,
            container,
            music_containers,
        })
    }

    /// A radio prepared from already-decoded synthetic members: the
    /// synthetic-content seam, the counterpart of [`MissionRadio::empty`].
    ///
    /// [`MissionRadio::read`] is what a mission's own declarations drive; this
    /// one exists so a composition over synthetic content can carry prepared
    /// voice members and drive [`publish_prepared_voices`] end to end without
    /// an installation. Every value is derived from what is handed in exactly
    /// as [`MissionRadio::read`] derives it — the length from the PCM's own
    /// frames and rate, the speaker from the member's own spelling, the
    /// identity through [`crate::audio::voice_member_id`] — so it states no
    /// retail fact. It resolves no group, so it answers every cue through
    /// [`RadioRefusal::GroupNotPrepared`]; that is the point: this seam
    /// exercises the publish path, not the table.
    ///
    /// # Errors
    ///
    /// [`RadioRefusal`] naming the member whose spelling yields no speaker
    /// ([`RadioRefusal::NotAVoiceMember`] / [`RadioRefusal::NoSpeaker`]) or no
    /// dialogue id ([`RadioRefusal::Identity`]).
    pub fn from_synthetic_members(
        container: &str,
        members: impl IntoIterator<Item = (String, PcmAudio)>,
    ) -> Result<Self, RadioRefusal> {
        let mut lines = BTreeMap::new();
        let mut voices = Vec::new();
        for (member, pcm) in members {
            let speaker = speaker_of(&member)?;
            let id = crate::audio::voice_member_id(container, &member).map_err(|error| {
                RadioRefusal::Identity {
                    member: member.clone(),
                    reason: error.to_string(),
                }
            })?;
            let duration_ticks = duration_ticks(&pcm);
            let voice = DecodedVoiceMember::new(container, &member, pcm).map_err(|error| {
                RadioRefusal::Identity {
                    member: member.clone(),
                    reason: error.to_string(),
                }
            })?;
            lines.insert(
                member.clone(),
                PreparedLine {
                    member,
                    speaker,
                    asset: id,
                    duration_ticks,
                },
            );
            voices.push(voice);
        }
        Ok(Self {
            groups: BTreeMap::new(),
            lines,
            voices,
            music_tracks: Vec::new(),
            install_root: PathBuf::new(),
            failures: BTreeMap::new(),
            container: container.to_owned(),
            music_containers: BTreeMap::new(),
        })
    }

    /// Turns one cue into the lines it queues, the music request it is, or
    /// the typed refusal it becomes.
    #[must_use]
    pub fn plan(&self, cue: &CueEmission) -> Vec<RadioLineOutcome> {
        if matches!(cue.kind, CueKind::RoleJingle) || cue.group.is_none() {
            return vec![RadioLineOutcome::Refused {
                group: None,
                refusal: RadioRefusal::UnnamedRoleJingle,
            }];
        }
        let group = cue.group.clone().expect("checked above");
        if let Some(refusal) = self.failures.get(&group) {
            return vec![RadioLineOutcome::Refused {
                group: Some(group),
                refusal: refusal.clone(),
            }];
        }
        let Some(resolved) = self.groups.get(&group) else {
            return vec![RadioLineOutcome::Refused {
                group: Some(group.clone()),
                refusal: RadioRefusal::GroupNotPrepared { group },
            }];
        };
        // A music state request plays no radio line: its children are that
        // state's tracks, and `offer_cues` turns this classification into the
        // `MusicDirector` request or the typed refusal it becomes.
        if resolved.music {
            return vec![RadioLineOutcome::Music { group }];
        }
        let mut outcomes = Vec::new();
        for leaf in &resolved.unresolved {
            outcomes.push(RadioLineOutcome::Refused {
                group: Some(group.clone()),
                refusal: RadioRefusal::UnresolvedLeaf {
                    group: group.clone(),
                    leaf: leaf.clone(),
                },
            });
        }
        if resolved.members.is_empty() {
            outcomes.push(RadioLineOutcome::Refused {
                group: Some(group.clone()),
                refusal: RadioRefusal::NoVoiceMember { group },
            });
            return outcomes;
        }
        for member in &resolved.members {
            let Some(line) = self.lines.get(member) else {
                let refusal = self.failures.get(member).cloned().unwrap_or_else(|| {
                    RadioRefusal::NotAVoiceMember {
                        member: member.clone(),
                    }
                });
                outcomes.push(RadioLineOutcome::Refused {
                    group: Some(group.clone()),
                    refusal,
                });
                continue;
            };
            outcomes.push(RadioLineOutcome::Queued {
                group: group.clone(),
                member: line.member.clone(),
                speaker: line.speaker.clone(),
                duration_ticks: line.duration_ticks,
            });
        }
        outcomes
    }

    /// Builds the [`RadioLine`] a queued plan names and offers it to `audio`,
    /// which stamps the line with its own event id.
    ///
    /// # Errors
    ///
    /// [`RadioRefusal::Line`] when the line builder refuses the shape — the
    /// audio side's own typed reason, unchanged.
    pub fn enqueue(
        &self,
        outcome: &RadioLineOutcome,
        audio: &mut AudioSession,
    ) -> Result<(), RadioRefusal> {
        let RadioLineOutcome::Queued {
            member, speaker, ..
        } = outcome
        else {
            return Ok(());
        };
        let line = self
            .lines
            .get(member)
            .ok_or_else(|| RadioRefusal::NotAVoiceMember {
                member: member.clone(),
            })?;
        let id: AudioEventId = audio.mint_radio_id();
        RadioLine::try_new(
            id,
            speaker,
            line.asset.clone(),
            RADIO_PRIORITY,
            RADIO_INTERRUPTIBLE,
            None,
            line.duration_ticks,
        )
        .map(|built| audio.enqueue_radio(built))
        .map_err(|error: RadioLineError| RadioRefusal::Line {
            member: member.clone(),
            reason: error.to_string(),
        })
    }
}

/// Offers `cues` to `audio` through `radio`, answering with what each one
/// became, in emission order.
///
/// This is the host's whole sound step ([`super::MissionHost::step`] stage 7)
/// factored out so the acceptance suite drives the same code the composition
/// drives: a world with no audio session (`audio` is `None`) answers every
/// cue by naming it, never by dropping it, and a line the line builder
/// refuses is reported with that refusal instead of a queued line.
///
/// A music state request reaches the session's
/// [`MusicDirector`](cs_sim::audio_events::MusicDirector): `music` — the
/// mission seed's selector — picks the track by the state's measured rule
/// ([`crate::audio::MusicSelector::select`]), the labelled container choice
/// names its archive, and the request carries the owner-ruled
/// [`crate::audio::music_member_id`] identity. A group the selector cannot
/// serve (no measured state, a track in neither archive, a member whose
/// spelling mints no id) is refused by name, never by a guessed track.
#[must_use]
pub fn offer_cues(
    cues: &[CueEmission],
    radio: &MissionRadio,
    music: &mut crate::audio::MusicSelector,
    audio: Option<&mut AudioSession>,
) -> Vec<RadioLineOutcome> {
    if cues.is_empty() {
        return Vec::new();
    }
    let Some(audio) = audio else {
        return cues
            .iter()
            .map(|cue| RadioLineOutcome::NoAudioSession {
                group: cue.group.clone(),
            })
            .collect();
    };
    let audio = &mut *audio;
    let mut outcomes = Vec::new();
    for cue in cues {
        for plan in radio.plan(cue) {
            if let RadioLineOutcome::Music { group } = &plan {
                outcomes.push(request_music(group, radio, music, audio));
                continue;
            }
            match radio.enqueue(&plan, audio) {
                Ok(()) => outcomes.push(plan),
                Err(refusal) => outcomes.push(RadioLineOutcome::Refused {
                    group: cue.group.clone(),
                    refusal,
                }),
            }
        }
    }
    outcomes
}

/// Serves one music state request: the selector's measured pick, the labelled
/// container choice's archive, the owner-ruled track identity, and the audio
/// session's `MusicDirector` verdict — or the typed refusal of a request the
/// chain cannot honour.
fn request_music(
    group: &str,
    radio: &MissionRadio,
    music: &mut crate::audio::MusicSelector,
    audio: &mut AudioSession,
) -> RadioLineOutcome {
    let refused = |refusal: RadioRefusal| RadioLineOutcome::Refused {
        group: Some(group.to_owned()),
        refusal,
    };
    let Some(pick) = music.select(group) else {
        return refused(RadioRefusal::MusicStateUnknown {
            group: group.to_owned(),
        });
    };
    let Some(container) = radio.music_container(&pick.member) else {
        return refused(RadioRefusal::MusicMemberAbsent {
            member: pick.member.clone(),
        });
    };
    let asset = match crate::audio::music_member_id(container, &pick.member) {
        Ok(asset) => asset,
        Err(error) => {
            return refused(RadioRefusal::MusicIdentity {
                member: pick.member.clone(),
                reason: error.to_string(),
            });
        }
    };
    let id = audio.mint_music_id();
    let outcome = audio.request_music(cs_sim::audio_events::MusicCue {
        id,
        asset: asset.clone(),
    });
    RadioLineOutcome::MusicRequested {
        group: group.to_owned(),
        slot: pick.slot,
        member: pick.member,
        asset,
        outcome,
    }
}

// ------------------------------------------------------------ the publish --

/// Resource: the load whose prepared voice members the publish step has
/// already handed to the device's sample library.
///
/// One publish per load: the prepare step's members are the same on every
/// frame, so re-inserting them frame after frame would only take the
/// library's lock again for a value that is already there. A reload's own
/// install is a different [`LoadIdentity`], so its prepare step publishes
/// into the fresh library.
#[derive(Resource, Debug, Default)]
pub struct VoicePublicationWatch {
    /// The load the last publish ran for, when one has run.
    load: Option<LoadIdentity>,
}

/// Publishes this mission's prepared voice members into the world's device
/// sample library, once per load (M01-RADIO-SAMPLE-WIRE, #1466).
///
/// # Ordering (load-bearing)
///
/// The same load's population pass
/// ([`crate::audio::insert_audio_session`]) fills the library with
/// [`SampleLibrary::replace_samples`](crate::audio::SampleLibrary::replace_samples)
/// — **wholesale** — so a publish that ran before it would be discarded. This
/// system therefore waits for that pass's own recorded install
/// ([`AudioHandoffLog`]) and is installed
/// [`after`](bevy::ecs::schedule::IntoScheduleConfigs::after) it, so it can
/// only run once the pass has run.
///
/// # What it does not do
///
/// A world with no device library (a stage with no audible launch sound), no
/// audio install yet, or a radio that prepared no voice member is left
/// exactly as it is. Nothing is refused here: the members' ids were validated
/// when they were built, so the refusal path is unchanged — an id no prepare
/// step published is still refused `sample_unavailable` by name when a line
/// names it.
pub fn publish_prepared_voices(world: &mut World) {
    let Some(installed) = world
        .get_resource::<AudioHandoffLog>()
        .and_then(|log| log.installed.as_ref())
        .map(|install| install.load)
    else {
        return;
    };
    // Already published for this load: nothing below is even looked at, so the
    // steady state costs one resource read per frame.
    if world
        .get_resource::<VoicePublicationWatch>()
        .is_some_and(|watch| watch.load == Some(installed))
    {
        return;
    }
    let Some(library) = world.get_resource::<DeviceSampleLibrary>().cloned() else {
        return;
    };
    let Some((members, lazy_music)) = world.get_resource::<MissionHost>().map(|host| {
        (
            host.radio().voices().to_vec(),
            (!host.radio().music_tracks().is_empty()).then(|| host.radio().music_loader()),
        )
    }) else {
        return;
    };
    if members.is_empty() && lazy_music.is_none() {
        return;
    }
    let mut watch = world.get_resource_or_insert_with(VoicePublicationWatch::default);
    watch.load = Some(installed);
    let _published = publish_voice_members(library.library().as_ref(), &members);
    // The music tracks are not published here: the mixer decodes the one a
    // request starts, through this loader (Rally #1551).
    if let Some(lazy_music) = lazy_music {
        world.insert_resource(lazy_music);
    }
}

/// Installs [`publish_prepared_voices`] in the composition's `PreUpdate`,
/// after the load's population pass ([`AudioHandoffLog`]'s own
/// [`insert_audio_session`]) — the ordering the module note on the publish
/// step explains. A world whose composition installs no audio plugin simply
/// has no such pass and no publish.
pub fn install_radio_voice_publication(app: &mut App) {
    app.init_resource::<VoicePublicationWatch>();
    app.add_systems(
        PreUpdate,
        publish_prepared_voices.after(insert_audio_session),
    );
}

/// One selectable music track, known by name and not yet decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicTrackSource {
    /// The music id a cue names ([`crate::audio::music_member_id`]).
    pub id: ContentId,
    /// The install-relative archive the track lives in.
    pub container: String,
    /// The track's own member name.
    pub member: String,
}

impl crate::audio::MusicTrackLoader for MissionRadio {
    fn load(&self, id: &ContentId) -> Result<DecodedMusicMember, String> {
        self.decode_music(id).map_err(|refusal| refusal.to_string())
    }
}

/// The group → child table of `sounds.zrd`.
#[derive(Debug, Default, Clone)]
struct SoundGroupTable {
    /// `SETS` rows: group name → archive member.
    members: BTreeMap<String, String>,
    /// `SOUND_GROUPS` records: group name → its spelled children.
    children: BTreeMap<String, Vec<String>>,
    /// `SOUND_GROUPS` records: group name → the float its `DYNAMIC_WEIGHTS`
    /// property spells, for the groups that carry one (the music states).
    /// The property itself resolves to no member — it stays an
    /// [`ResolvedGroup::unresolved`] leaf — but its spelled value is the
    /// measured post-draw decay factor the music selector reproduces.
    weight_factors: BTreeMap<String, f32>,
}

impl SoundGroupTable {
    /// Reads the table out of the installation's root reader archive.
    ///
    /// # Errors
    ///
    /// [`RadioError`] when a production reader refuses or the member holds no
    /// section this consumer honours.
    fn read(install_root: &Path) -> Result<Self, RadioError> {
        let bytes = reader_member(install_root, SOUND_GROUPS_ARCHIVE, SOUND_GROUPS_MEMBER)
            .map_err(RadioError::Read)?;
        let document = cs_content::stunts::decode_zrd(&bytes)
            .map_err(|error| RadioError::Read(format!("{SOUND_GROUPS_MEMBER}: {error}")))?;
        Self::from_document(&document)
    }

    /// The table one decoded `sounds.zrd` document holds, whatever asked for
    /// it: the `SETS` rows become members, the `SOUND_GROUPS` records become
    /// children.
    ///
    /// # Errors
    ///
    /// [`RadioError::Unmeasured`] when the document declares neither section.
    fn from_document(document: &ZrdValue) -> Result<Self, RadioError> {
        let record = cs_content::stunts::objective_record(document);
        let fields = cs_content::stunts::zrd_flat_fields(record);

        let mut table = Self::default();
        for (key, value) in &fields {
            match *key {
                SETS_KEY => {
                    for (_set, entries) in cs_content::stunts::zrd_flat_fields(value) {
                        let Some(rows) = entries.as_list() else {
                            continue;
                        };
                        for row in rows {
                            let Some(parts) = row.as_list() else {
                                continue;
                            };
                            let (Some(name), Some(member)) = (
                                parts.first().and_then(as_text),
                                parts.get(1).and_then(as_text),
                            ) else {
                                continue;
                            };
                            table
                                .members
                                .entry(name.to_owned())
                                .or_insert_with(|| member.to_owned());
                        }
                    }
                }
                SOUND_GROUPS_KEY => {
                    let Some(records) = value.as_list() else {
                        continue;
                    };
                    for record in records {
                        let Some(parts) = record.as_list() else {
                            continue;
                        };
                        let Some(name) = parts.first().and_then(as_text) else {
                            continue;
                        };
                        let children = table.children.entry(name.to_owned()).or_default();
                        for child in &parts[1..] {
                            collect_child_names(child, children);
                            if let Some(factor) = spelled_weight_factor(child) {
                                table
                                    .weight_factors
                                    .entry(name.to_owned())
                                    .or_insert(factor);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if table.members.is_empty() && table.children.is_empty() {
            return Err(RadioError::Unmeasured(format!(
                "{SOUND_GROUPS_MEMBER} declares neither {SETS_KEY} nor {SOUND_GROUPS_KEY}"
            )));
        }
        Ok(table)
    }

    /// Expands `group` through the children records until every leaf is a set
    /// member or the `MUSIC` marker, in the order the record spelled.
    ///
    /// A name reached twice is expanded once, so a group that (transitively)
    /// contains itself resolves to what it reaches rather than looping.
    ///
    /// # Errors
    ///
    /// [`RadioError::Unmeasured`] when a child is a name nothing declares —
    /// the record spells something this table cannot honour.
    fn expand(&self, group: &str) -> Result<ResolvedGroup, RadioError> {
        let mut resolved = ResolvedGroup::default();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(group.to_owned());
        while let Some(name) = queue.pop_front() {
            if !seen.insert(name.clone()) {
                continue;
            }
            // A group that spells `DYNAMIC_WEIGHTS` carries its decay
            // factor beside the marker: the first such group reached
            // supplies it (the music records spell it on their own child).
            resolved.dynamic_weights = resolved
                .dynamic_weights
                .or_else(|| self.weight_factors.get(&name).copied());
            if let Some(member) = self.members.get(&name) {
                if !resolved.members.contains(member) {
                    resolved.members.push(member.clone());
                }
                continue;
            }
            let Some(children) = self.children.get(&name) else {
                return Err(RadioError::Unmeasured(format!(
                    "sound group {group:?} reaches {name:?}, which {SOUND_GROUPS_MEMBER} declares \
                     neither as a set member nor as a group"
                )));
            };
            for child in children {
                if child == MUSIC_LEAF {
                    resolved.music = true;
                    continue;
                }
                if child.to_ascii_lowercase().ends_with(".wav") {
                    if !resolved.members.contains(child) {
                        resolved.members.push(child.clone());
                    }
                    continue;
                }
                if self.members.contains_key(child) || self.children.contains_key(child) {
                    queue.push_back(child.clone());
                } else if !resolved.unresolved.contains(child) {
                    // A word the record spells beside a group (for example
                    // `DYNAMIC_WEIGHTS`) resolves to nothing: named here
                    // rather than dropped.
                    resolved.unresolved.push(child.clone());
                }
            }
        }
        Ok(resolved)
    }
}

/// A `ZrdValue`'s text, when it is one.
fn as_text(value: &ZrdValue) -> Option<&str> {
    value.as_text()
}

/// The names one `SOUND_GROUPS` child holds, collected whole.
///
/// The record spells a child as a bare name (`"snd_a"`), as a list of names
/// (`["snd_a", ["snd_b"]]`, the spelling `snd_NW1Start` uses for its second
/// line), or as a name beside a property (`["MUSIC", "DYNAMIC_WEIGHTS", 0.5,
/// ["snd_prebattle1"]]`). Every text leaf is a name candidate: a leaf nothing
/// declares reaches [`RadioRefusal::UnresolvedLeaf`] when the cue fires,
/// rather than being dropped because a list wrapped it.
fn collect_child_names(child: &ZrdValue, out: &mut Vec<String>) {
    match child {
        ZrdValue::Text(text) => out.push(text.clone()),
        ZrdValue::List(items) => {
            for item in items {
                collect_child_names(item, out);
            }
        }
        ZrdValue::Int(_) | ZrdValue::Float(_) => {}
    }
}

/// The `DYNAMIC_WEIGHTS` factor a `SOUND_GROUPS` child spells, when it does.
///
/// The property lives inside a child list like `["MUSIC", "DYNAMIC_WEIGHTS",
/// 0.5, ["snd_prebattle1"]]`: the factor is the first numeric leaf after the
/// property's own name. Nested children are searched the same way
/// [`collect_child_names`] descends.
fn spelled_weight_factor(child: &ZrdValue) -> Option<f32> {
    let items = child.as_list()?;
    for (index, item) in items.iter().enumerate() {
        if let ZrdValue::Text(text) = item
            && text.eq_ignore_ascii_case("DYNAMIC_WEIGHTS")
        {
            return items[index + 1..].iter().find_map(|value| match value {
                ZrdValue::Float(factor) => Some(*factor),
                ZrdValue::Int(int) => Some(*int as f32),
                ZrdValue::Text(_) | ZrdValue::List(_) => None,
            });
        }
        if let Some(factor) = spelled_weight_factor(item) {
            return Some(factor);
        }
    }
    None
}

/// The dialogue content id of an archive member — F14-D.7's
/// `install_file_key` derivation over
/// [`cs_types::content::ContentKind::Dialogue`], because the member is a
/// spoken line.
///
/// This is [`crate::audio::voice_member_id`], the one derivation #1465
/// owns: the id a `RadioLine` names here and the id the publish step puts
/// the member's samples under must be the same code path, so this wrapper
/// exists only to keep this module's typed error.
///
/// # Errors
///
/// [`RadioError`] when the id grammar refuses the spelling.
pub fn dialogue_content_id(container: &str, member: &str) -> Result<ContentId, RadioError> {
    crate::audio::voice_member_id(container, member)
        .map_err(|error| RadioError::Read(error.to_string()))
}

/// The speaker a voice member's own name spells.
///
/// The archive spells its spoken lines `VO_<mission>_<speaker>_<index>.wav`
/// (for example `VO_c2-NW-m1_Jack_7.wav`); the `<speaker>` token is the
/// installation's own naming, read here rather than invented.
///
/// # Errors
///
/// [`RadioRefusal::NotAVoiceMember`] when the name carries no `VO_` prefix,
/// [`RadioRefusal::NoSpeaker`] when the rest does not spell
/// `<mission>_<speaker>_<index>`.
fn speaker_of(member: &str) -> Result<String, RadioRefusal> {
    let stem = member.strip_suffix(".wav").unwrap_or(member);
    let stem = strip_voice_prefix(stem).ok_or_else(|| RadioRefusal::NotAVoiceMember {
        member: member.to_owned(),
    })?;
    // The mission token carries no underscore in every spelled line
    // (`c2-NW-m1`, `id1`), so the first underscore ends it.
    let (_, tail) = stem
        .split_once('_')
        .ok_or_else(|| RadioRefusal::NoSpeaker {
            member: member.to_owned(),
        })?;
    let Some((head, index)) = tail.rsplit_once('_') else {
        return Err(RadioRefusal::NoSpeaker {
            member: member.to_owned(),
        });
    };
    // The trailing index is digits, optionally in two hyphenated groups
    // (`43`, `31-1`).
    let digits = index.replace('-', "");
    if head.is_empty()
        || digits.is_empty()
        || !digits.chars().all(|character| character.is_ascii_digit())
    {
        return Err(RadioRefusal::NoSpeaker {
            member: member.to_owned(),
        });
    }
    Ok(head.to_owned())
}

/// The `VO_` prefix a spoken line's name carries, matched without regard to
/// case the way the archive's own consumer compares names.
fn strip_voice_prefix(stem: &str) -> Option<&str> {
    let (head, tail) = stem.split_at(VOICE_PREFIX.len().min(stem.len()));
    if head.eq_ignore_ascii_case(VOICE_PREFIX) {
        Some(tail)
    } else {
        None
    }
}

/// The member names one sound archive declares.
///
/// This is the labelled container choice's presence probe for the music
/// tracks (`ZBD/soundsh.zbd` preferred, `ZBD/soundsl.zbd` otherwise — the
/// owner ruling's container rule): a name listing only, through the same
/// production archive readers the voice decode uses, and no member is
/// decoded — a music track's id is minted from its name, not its samples.
///
/// # Errors
///
/// [`RadioError::Read`] when a production reader refuses the archive.
fn sound_archive_members(
    install_root: &Path,
    container: &str,
) -> Result<BTreeSet<String>, RadioError> {
    let path = install_root.join(container);
    let bytes = std::fs::read(&path)
        .map_err(|error| RadioError::Read(format!("cannot read {container}: {error}")))?;
    let spelling = cs_types::install::RelativePath::new(container)
        .map_err(|error| RadioError::Read(error.to_string()))?;
    let mut context = ParseContext::with_defaults("mission_radio");
    let decision = dispatch(ZbdProbe::new(container, &spelling, &bytes))
        .map_err(|error| RadioError::Read(error.to_string()))?;
    let index = read_version_one_index(&mut context, decision, &bytes)
        .map_err(|error| RadioError::Read(error.to_string()))?;
    let table = index.member_table();
    let archive = read_sound_archive(&mut context, &table, index.data())
        .map_err(|error| RadioError::Read(error.to_string()))?;
    Ok(archive
        .entries()
        .map(|entry| String::from_utf8_lossy(entry.name()).into_owned())
        .collect())
}

/// The fixed-tick length of every member, decoded through `cs_formats`' own
/// sample decoder and counted in frames.
///
/// The container is read once and each member is decoded exactly once: the
/// line's duration is the decoded value count divided by the declared channel
/// count and rate — never a guessed constant — and the device-ready samples
/// come from the same decode, so the publish step hands the device exactly the
/// PCM this prepare step measured.
fn decode_members(
    install_root: &Path,
    container: &str,
    members: &BTreeSet<String>,
    failures: &mut BTreeMap<String, RadioRefusal>,
) -> Result<BTreeMap<String, DecodedMember>, RadioError> {
    if members.is_empty() {
        return Ok(BTreeMap::new());
    }
    let path = install_root.join(container);
    let bytes = std::fs::read(&path)
        .map_err(|error| RadioError::Read(format!("cannot read {container}: {error}")))?;
    let spelling = cs_types::install::RelativePath::new(container)
        .map_err(|error| RadioError::Read(error.to_string()))?;
    let mut context = ParseContext::with_defaults("mission_radio");
    let decision = dispatch(ZbdProbe::new(container, &spelling, &bytes))
        .map_err(|error| RadioError::Read(error.to_string()))?;
    let index = read_version_one_index(&mut context, decision, &bytes)
        .map_err(|error| RadioError::Read(error.to_string()))?;
    let table = index.member_table();
    let archive = read_sound_archive(&mut context, &table, index.data())
        .map_err(|error| RadioError::Read(error.to_string()))?;

    let mut decoded_members = BTreeMap::new();
    for entry in archive.entries() {
        let name = String::from_utf8_lossy(entry.name());
        if !members.contains(name.as_ref()) {
            continue;
        }
        let member = name.clone().into_owned();
        let content = entry.content();
        let format = match SampleFormat::from_member(content) {
            Ok(format) => format,
            Err(error) => {
                // The member's own header refuses: its length — the line's
                // duration — stays unknown, and the cue that names it is
                // refused by name rather than queued with a guessed one.
                failures.insert(
                    member.clone(),
                    RadioRefusal::Undecodable {
                        member,
                        reason: format!("the WAVE header refuses: {error}"),
                    },
                );
                continue;
            }
        };
        // A fresh context per member: the parse context's allocation budget is
        // per decode, not per archive, so the long music tracks (megabytes of
        // samples each) do not starve the members read after them.
        let mut member_context = ParseContext::with_defaults("mission_radio");
        let decoded = match decode_sound_sample(&mut member_context, content, &format) {
            Ok(decoded) => decoded,
            Err(error) => {
                failures.insert(
                    member.clone(),
                    RadioRefusal::Undecodable {
                        member,
                        reason: format!("the sample decode refuses: {error}"),
                    },
                );
                continue;
            }
        };
        let channels = format.channels();
        let rate_hz = format.rate_hz();
        if channels == 0 || rate_hz == 0 {
            failures.insert(member.clone(), RadioRefusal::NoDuration { member });
            continue;
        }
        // The device-ready shape of the same decode. A member the decoder
        // produced values for but the device cannot hold (no samples at all,
        // or a channel count it cannot place) is refused by name: the line
        // could never be delivered, so queueing it as something the device
        // cannot play would be a silent lie.
        let pcm = match PcmAudio::from_decoded(&decoded) {
            Ok(pcm) => pcm,
            Err(error) => {
                failures.insert(
                    member.clone(),
                    RadioRefusal::Undecodable {
                        member,
                        reason: format!("the decoded member is not a playable shape: {error}"),
                    },
                );
                continue;
            }
        };
        decoded_members.insert(
            name.into_owned(),
            DecodedMember {
                duration_ticks: duration_ticks(&pcm),
                pcm,
            },
        );
    }
    for member in members {
        if !decoded_members.contains_key(member) && !failures.contains_key(member) {
            failures.insert(
                member.clone(),
                RadioRefusal::MemberAbsent {
                    member: member.clone(),
                },
            );
        }
    }
    Ok(decoded_members)
}

/// One member's decode: the device-ready samples and the line length in fixed
/// simulation ticks.
#[derive(Debug, Clone, PartialEq)]
struct DecodedMember {
    /// The samples a device plays for this member.
    pcm: PcmAudio,
    /// The member's decoded length in fixed simulation ticks, at least one.
    duration_ticks: u64,
}

/// The fixed-tick length of one decoded member: its own frame count over its
/// own declared rate, at the baseline fixed rate, never below one tick — the
/// duration the line that plays it carries.
fn duration_ticks(pcm: &PcmAudio) -> u64 {
    let seconds = pcm.frames() as f64 / f64::from(pcm.rate_hz());
    ((seconds * f64::from(BASELINE_FIXED_HZ)).round() as u64).max(1)
}

/// The sound-group names an engine objective model can ever cue.
///
/// Every cue the model emits names one of these two declarations, so this is
/// the complete set a mission's cues reach — a group outside it cannot be
/// named by this mission's program.
#[must_use]
pub fn declared_sound_groups(objectives: &EngineObjectives) -> BTreeSet<String> {
    let mut groups = BTreeSet::new();
    for index in 0usize.. {
        let Some(objective) = objectives.get(index) else {
            break;
        };
        if let Some(group) = &objective.declaration.wakeup_sound_group {
            groups.insert(group.clone());
        }
        if let Some(group) = &objective.declaration.completed_sound_group {
            groups.insert(group.clone());
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use cs_content::catalog::baseline::install_file_key;
    use cs_types::content::ContentKind;

    use super::*;

    #[test]
    fn accept_m01_play_radio_the_speaker_is_read_from_the_members_own_spelling() {
        assert_eq!(
            speaker_of("VO_c2-NW-m1_WorkersVoyage_1.wav").expect("the measured spelling"),
            "WorkersVoyage"
        );
        assert_eq!(
            speaker_of("VO_c2-NW-m1_Jack_31-1.wav").expect("the hyphenated index spelling"),
            "Jack"
        );
        assert_eq!(
            speaker_of("VO_c3-HW-m1_BigJohn_43-5.wav").expect("the two-group index spelling"),
            "BigJohn"
        );
        assert!(matches!(
            speaker_of("devastator.wav"),
            Err(RadioRefusal::NotAVoiceMember { .. })
        ));
        assert!(matches!(
            speaker_of("VO_id1.wav"),
            Err(RadioRefusal::NoSpeaker { .. })
        ));
    }

    #[test]
    fn accept_m01_play_radio_a_dialogue_id_is_derived_from_the_member_spelling() {
        let id = dialogue_content_id(RADIO_SOUND_CONTAINER, "VO_c2-NW-m1_Jack_7.wav")
            .expect("the archive spells the member");
        assert_eq!(id.kind(), ContentKind::Dialogue);
        // The id grammar encodes the source key (it escapes the separators and
        // lowercases), so the member's own spelling is compared through the
        // production derivation rather than by hand.
        let expected = install_file_key(&format!("{RADIO_SOUND_CONTAINER}/VO_c2-NW-m1_Jack_7.wav"));
        assert_eq!(
            id.key(),
            expected.as_str(),
            "the id must embed the container and the member the line plays, got {id}"
        );
        assert!(
            expected.contains("soundsh") && expected.ends_with("jack_5f_7.wav"),
            "the derived key spells the labelled container and this member: {expected}"
        );
    }

    #[test]
    fn accept_m01_play_radio_a_group_expands_through_sets_and_children() {
        // The measured `sounds.zrd` shape: one record holding both sections,
        // `SETS` rows of `[<name>, <member>.wav]`, and a `SOUND_GROUPS` child
        // spelled the way `snd_NW1Start` spells its second line — a bare name
        // beside a nested one-element list.
        let row = |name: &str, member: &str| {
            ZrdValue::List(vec![
                ZrdValue::Text(name.to_owned()),
                ZrdValue::Text(member.to_owned()),
            ])
        };
        let document = ZrdValue::List(vec![ZrdValue::List(vec![
            ZrdValue::Text(SETS_KEY.to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Text("c1cm01".to_owned()),
                ZrdValue::List(vec![
                    row("snd_a", "VO_x_a_1.wav"),
                    row("snd_b", "VO_x_b_2.wav"),
                ]),
            ]),
            ZrdValue::Text(SOUND_GROUPS_KEY.to_owned()),
            ZrdValue::List(vec![
                ZrdValue::List(vec![
                    ZrdValue::Text("snd_pair".to_owned()),
                    ZrdValue::List(vec![
                        ZrdValue::Text("snd_a".to_owned()),
                        ZrdValue::List(vec![ZrdValue::Text("snd_b".to_owned())]),
                    ]),
                ]),
                ZrdValue::List(vec![
                    ZrdValue::Text("snd_music".to_owned()),
                    ZrdValue::Text(MUSIC_LEAF.to_owned()),
                    ZrdValue::Text("DYNAMIC_WEIGHTS".to_owned()),
                ]),
                ZrdValue::List(vec![
                    ZrdValue::Text("snd_leaf".to_owned()),
                    ZrdValue::Text("VO_x_c_3.wav".to_owned()),
                ]),
            ]),
        ])]);
        let table =
            SoundGroupTable::from_document(&document).expect("the record declares both sections");

        let pair = table.expand("snd_pair").expect("both children resolve");
        assert_eq!(
            pair.members,
            ["VO_x_a_1.wav", "VO_x_b_2.wav"],
            "a child list keeps every name it spells, however deeply the record nests them"
        );
        assert!(!pair.music);
        assert!(pair.unresolved.is_empty());

        let music = table.expand("snd_music").expect("the leaves classify");
        assert!(music.music);
        assert!(music.members.is_empty());
        assert_eq!(music.unresolved, ["DYNAMIC_WEIGHTS"]);

        let leaf = table
            .expand("snd_leaf")
            .expect("a member child resolves directly");
        assert_eq!(leaf.members, ["VO_x_c_3.wav"]);

        assert!(
            table.expand("snd_unknown").is_err(),
            "a name nothing declares must be refused, never dropped"
        );
    }
}
