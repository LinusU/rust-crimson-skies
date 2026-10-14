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
//! [`CueEmission`](cs_content::objectives_engine::CueEmission) for each one
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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use cs_content::catalog::baseline::install_file_key;
use cs_content::objectives_engine::{CueEmission, CueKind, EngineObjectives};
use cs_content::stunts::ZrdValue;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    SampleFormat, ZbdProbe, decode_sound_sample, dispatch, read_sound_archive,
    read_version_one_index,
};
use cs_sim::audio_events::{AudioEventId, RadioLine, RadioLineError};
use cs_types::content::{ContentId, ContentKind};

use super::launch_sound::{LAUNCH_SOUND_CONTAINER, reader_member};
use crate::audio::AudioSession;
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
    /// is never queued with a guessed one.
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
    /// One voice member reached the audio session's radio queue.
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
    /// The group is a music state request: it reaches no radio line here, and
    /// this build holds no measured music asset to request either.
    Music {
        /// The music group the cue named.
        group: String,
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
    /// ([`ContentKind::Dialogue`] over F14-D.7's `install_file_key`
    /// spelling).
    pub asset: ContentId,
    /// The decoded length in fixed simulation ticks, at least one.
    pub duration_ticks: u64,
}

/// One resolved sound group: the members it plays, whether it is a music
/// state request, and the leaves the table could not resolve.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedGroup {
    /// The archive members the group reaches, in the spelled order.
    pub members: Vec<String>,
    /// Whether the group spelled the `MUSIC` leaf.
    pub music: bool,
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
    /// What refused to prepare, keyed by the group or member name it
    /// refused: an expansion that would not resolve, a member the container
    /// does not hold, a member that would not decode. A cue naming one of
    /// these answers with the refusal here rather than with a line — and
    /// never with silence.
    failures: BTreeMap<String, RadioRefusal>,
    /// The install-relative spelling the members were read from.
    container: String,
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
    #[must_use]
    pub fn groups(&self) -> impl Iterator<Item = &str> {
        self.groups.keys().map(String::as_str)
    }

    /// The members this radio prepared, in member order.
    #[must_use]
    pub fn prepared(&self) -> impl Iterator<Item = &PreparedLine> {
        self.lines.values()
    }

    /// What refused to prepare, keyed by the group or member it refused.
    #[must_use]
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

        let durations = decode_durations(install_root, &container, &members, &mut failures)?;
        let mut lines = BTreeMap::new();
        for member in members {
            let Some(duration_ticks) = durations.get(&member).copied() else {
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
            lines.insert(
                member.clone(),
                PreparedLine {
                    member,
                    speaker,
                    asset,
                    duration_ticks,
                },
            );
        }

        Ok(Self {
            groups: resolved,
            lines,
            failures,
            container,
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
        // state's tracks, and this build holds no measured music asset.
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

/// The group → child table of `sounds.zrd`.
#[derive(Debug, Default, Clone)]
struct SoundGroupTable {
    /// `SETS` rows: group name → archive member.
    members: BTreeMap<String, String>,
    /// `SOUND_GROUPS` records: group name → its spelled children.
    children: BTreeMap<String, Vec<String>>,
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
        let record = cs_content::stunts::objective_record(&document);
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
                            match child {
                                ZrdValue::Text(text) => children.push(text.clone()),
                                // A child the record spelled as a one-element
                                // list is the same name; any other shape
                                // reaches the refusal path by never
                                // resolving.
                                ZrdValue::List(inner) => {
                                    if let Some(text) = inner.first().and_then(as_text) {
                                        children.push(text.to_owned());
                                    }
                                }
                                _ => {}
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

/// The dialogue content id of an archive member: F14-D.7's
/// `install_file_key` derivation over [`ContentKind::Dialogue`], because the
/// member is a spoken line.
///
/// # Errors
///
/// [`RadioError`] when the id grammar refuses the spelling.
fn dialogue_content_id(container: &str, member: &str) -> Result<ContentId, RadioError> {
    ContentId::from_source(
        ContentKind::Dialogue,
        &install_file_key(&format!("{container}/{member}")),
    )
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

/// The fixed-tick length of every member, decoded through `cs_formats`' own
/// sample decoder and counted in frames.
///
/// The container is read once and each member is decoded exactly once: the
/// line's duration is the decoded value count divided by the declared channel
/// count and rate — never a guessed constant.
fn decode_durations(
    install_root: &Path,
    container: &str,
    members: &BTreeSet<String>,
    failures: &mut BTreeMap<String, RadioRefusal>,
) -> Result<BTreeMap<String, u64>, RadioError> {
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

    let mut durations = BTreeMap::new();
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
        let decoded = match decode_sound_sample(&mut context, content, &format) {
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
        let seconds = decoded.sample_count() as f64 / f64::from(channels) / f64::from(rate_hz);
        let ticks = (seconds * f64::from(BASELINE_FIXED_HZ)).round() as u64;
        durations.insert(name.into_owned(), ticks.max(1));
    }
    for member in members {
        if !durations.contains_key(member) && !failures.contains_key(member) {
            failures.insert(
                member.clone(),
                RadioRefusal::MemberAbsent {
                    member: member.clone(),
                },
            );
        }
    }
    Ok(durations)
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
        assert!(
            id.key().contains("VO_c2-NW-m1_Jack_7.wav"),
            "the id must name the member it plays, got {id}"
        );
    }

    #[test]
    fn accept_m01_play_radio_a_group_expands_through_sets_and_children() {
        let table = SoundGroupTable {
            members: [
                ("snd_a".to_owned(), "VO_x_a_1.wav".to_owned()),
                ("snd_b".to_owned(), "VO_x_b_2.wav".to_owned()),
            ]
            .into_iter()
            .collect(),
            children: [
                (
                    "snd_pair".to_owned(),
                    vec!["snd_a".to_owned(), "snd_b".to_owned()],
                ),
                (
                    "snd_music".to_owned(),
                    vec![MUSIC_LEAF.to_owned(), "DYNAMIC_WEIGHTS".to_owned()],
                ),
                ("snd_leaf".to_owned(), vec!["VO_x_c_3.wav".to_owned()]),
            ]
            .into_iter()
            .collect(),
        };

        let pair = table.expand("snd_pair").expect("both children resolve");
        assert_eq!(pair.members, ["VO_x_a_1.wav", "VO_x_b_2.wav"]);
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
