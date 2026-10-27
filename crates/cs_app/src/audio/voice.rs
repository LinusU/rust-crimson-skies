//! The radio voice members' publish step: the members a prepare step decoded
//! reach the device's [`SampleLibrary`] under the exact id a
//! [`RadioLine`](cs_sim::audio_events::RadioLine) names (Rally #1465,
//! `M01-RADIO-SAMPLE-IDENTITY`).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stages
//! `### F41-B`/`### F41-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! # The gap this closes
//!
//! #1438 (`M01-PLAY-RADIO`) derives each spoken member's id as
//! [`ContentKind::Dialogue`] over F14-D.7's
//! [`install_file_key`]
//! spelling of `"<container>/<member>"` and names that id on its `RadioLine`.
//! The device only plays what its library holds, and the loader's population
//! pass (`super::samples::populate`) decodes a member only when the declared
//! catalog carries its id **and** the derived id is the [`ContentKind::Sound`]
//! one. No container family states a cue class (F14-D.7), so the declared
//! catalog holds no dialogue row and a `Dialogue` id over the same key is
//! refused `sample_member_absent` — the owner therefore cannot hear M01's
//! radio.
//!
//! # The rule this implements (designed, not measured)
//!
//! The owner decided (task #1465, note of 2026-10-15):
//!
//! * a spoken voice member is [`ContentKind::Dialogue`], the kind #1438 already
//!   mints — **one identity per member**, the one the `RadioLine` already
//!   names, and no new id spelling;
//! * route **(b)**: the radio prepare step, which already decodes the voice
//!   members it queues, publishes exactly those decoded members into the
//!   device's [`SampleLibrary`] under that dialogue id, at prepare time,
//!   through the library's insert path — [`SampleLibrary::insert_sample`].
//!
//! The installation states no cue class, so this rule is a project convention
//! and is labelled **designed**. [`voice_member_id`] derives it afresh (the id
//! `RadioLine::asset` carries), [`DecodedVoiceMember`] pairs it with the PCM a
//! prepare step decoded, and [`publish_voice_members`] inserts each one.
//!
//! # Ordering with the population pass
//!
//! [`SampleLibrary::replace_samples`] replaces the library **wholesale** for
//! one load, which is what makes a reload observable. [`publish_voice_members`]
//! therefore runs **after** the population pass of the same load — a publish
//! before it would be discarded by the replace. Both belong to one load: on a
//! reload the new load's population pass replaces the library and its own
//! prepare step publishes into the fresh library, so the replaced load's lines
//! are unreachable exactly like its other samples.
//!
//! # What this does not claim
//!
//! Nothing here is `verified_original`: no original executable ran, no member
//! was decoded from the installation by this module (the prepare step decodes
//! it), and no person heard anything. The owner's listening test stays the
//! F41-D gate. `ContentSampleSource` still derives [`ContentKind::Sound`] ids
//! for sound members and is unchanged; F14-D.7 is unchanged.

use std::fmt;

use cs_content::catalog::baseline::install_file_key;
use cs_types::content::{ContentId, ContentKind};

use super::device::{PcmAudio, SampleLibrary};

/// The exact content id a spoken voice member of `container` is known by.
///
/// The identity is [`ContentKind::Dialogue`] over F14-D.7's
/// [`install_file_key`] spelling of `"<container>/<member>"` — the one
/// [`RadioLine::asset`](cs_sim::audio_events::RadioLine) already names. This
/// derives the id the same way the radio prepare step does; it does not mint a
/// second spelling.
///
/// # Errors
///
/// [`VoiceMemberError::Identity`] when the id grammar refuses the derived key.
pub fn voice_member_id(container: &str, member: &str) -> Result<ContentId, VoiceMemberError> {
    let spelling = format!("{container}/{member}");
    ContentId::from_source(ContentKind::Dialogue, &install_file_key(&spelling)).map_err(|error| {
        VoiceMemberError::Identity {
            container: container.to_owned(),
            member: member.to_owned(),
            reason: error.to_string(),
        }
    })
}

/// Why a radio voice member could not be identified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceMemberError {
    /// The member's own spelling derives no id: the F14-D.7 key grammar
    /// refuses it, so no line can ever name it.
    Identity {
        /// The install-relative container the member was read from.
        container: String,
        /// The member's own name.
        member: String,
        /// The id grammar's own reason, unchanged.
        reason: String,
    },
}

impl fmt::Display for VoiceMemberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity {
                container,
                member,
                reason,
            } => write!(
                f,
                "voice member {member:?} of {container:?} derives no content id: {reason}"
            ),
        }
    }
}

impl std::error::Error for VoiceMemberError {}

/// One voice member a prepare step decoded, ready for the device: its identity
/// (the id a `RadioLine` names) and the samples to publish under it.
///
/// The id is computed **once**, at construction, from the member's own
/// spelling, so no publish path can pair one member's samples with another
/// member's id.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedVoiceMember {
    id: ContentId,
    container: String,
    member: String,
    pcm: PcmAudio,
}

impl DecodedVoiceMember {
    /// Pairs one decoded member with its identity.
    ///
    /// `container` is the install-relative spelling the member was read from
    /// (e.g. `ZBD/soundsh.zbd`) and `member` the member's own name; `pcm` is
    /// the member already decoded, because the prepare step that owns the
    /// decode is the one that publishes.
    ///
    /// # Errors
    ///
    /// [`VoiceMemberError::Identity`] when the member's spelling derives no id.
    pub fn new(container: &str, member: &str, pcm: PcmAudio) -> Result<Self, VoiceMemberError> {
        let id = voice_member_id(container, member)?;
        Ok(Self {
            id,
            container: container.to_owned(),
            member: member.to_owned(),
            pcm,
        })
    }

    /// The dialogue id a `RadioLine` names for this member.
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    /// The install-relative container the member was read from.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The member's own name.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// The member's decoded samples.
    #[must_use]
    pub const fn pcm(&self) -> &PcmAudio {
        &self.pcm
    }
}

/// What one publish pass did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoicePublication {
    /// How many voice members reached the library under their dialogue id.
    pub published: usize,
}

/// Publishes each decoded voice member into `library` under the exact dialogue
/// id a `RadioLine` names, through [`SampleLibrary::insert_sample`].
///
/// Runs after the same load's population pass (see the module note on
/// ordering). Nothing is refused here: every member's id was validated when it
/// was built ([`DecodedVoiceMember::new`]), so this cannot put samples under an
/// id no prepare step named.
pub fn publish_voice_members(
    library: &dyn SampleLibrary,
    members: &[DecodedVoiceMember],
) -> VoicePublication {
    for member in members {
        library.insert_sample(member.id.clone(), member.pcm.clone());
    }
    VoicePublication {
        published: members.len(),
    }
}
