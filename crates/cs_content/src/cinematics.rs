//! The declared cinematic schema: provenance-carrying prerendered-video and
//! in-engine camera sequences with their semantic actions (F40-A).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **content half** of the cinematic contract — the
//! normalized record a content importer produces and the catalog consumes. Its
//! runtime counterpart is `cs_sim::cinematic_state`; the lowering boundary is
//! `cs_app::cinematics`. This crate cannot depend on `cs_sim`, so the declared
//! record keeps its own typed vocabulary and the boundary maps it field-wise.
//!
//! The record keeps the **presentation** (a prerendered video or an in-engine
//! camera track) apart from the **semantic actions** that change mission state
//! (spec non-negotiable behavior 1). Every load-bearing value — duration, frame
//! size, skippability, pause policy, master clock and failure recovery — is a
//! [`Resolved`], so an unmeasured value stays an explicit unknown with its
//! claim id instead of a silent default. Nothing here is measured original
//! data.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_formats::ParseContext;
use cs_formats::text::{LineKind, RecordKind, read_keyed_list};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::config::{
    ConfigEntry, RawField as ConfigRawField, RawValue as ConfigRawValue, RecordSchema,
};
use crate::stunts::{ZrdValue, decode_zrd};

/// What shows the scene.
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredPresentation {
    /// A prerendered video file.
    PrerenderedVideo {
        /// The video resource (a [`ContentKind::Video`]).
        media: ContentId,
        /// The container/codec label, as recorded by the format research.
        format: Resolved<String>,
    },
    /// An in-engine sequence driven by a camera track.
    InEngine {
        /// The camera track (a [`ContentKind::CameraTrack`]).
        camera_track: ContentId,
    },
}

/// Declared simulation pause policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredPausePolicy {
    /// The simulation is paused for the scene.
    SimulationPaused,
    /// The simulation keeps running.
    SimulationRuns,
}

/// Declared master clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredClock {
    /// The audio device clock.
    Audio,
    /// The simulation tick clock.
    Simulation,
}

/// Declared media-failure recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredRecovery {
    /// Apply the remaining semantic actions, then report the failure.
    ApplyRemainingSemantics,
    /// Apply nothing more and report the failure.
    Block,
}

/// Declared semantic effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredEffect {
    /// Raise an objective event (a [`ContentKind::Objective`]).
    ObjectiveEvent(ContentId),
    /// Return control to the aircraft the player flies at that moment.
    ReturnControlToPlayer,
}

/// One declared semantic action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredAction {
    /// The action's id within the cinematic.
    pub id: u32,
    /// The cinematic tick it fires at.
    pub at_tick: u64,
    /// What it does.
    pub effect: DeclaredEffect,
}

/// Why a declared cinematic was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CinematicSchemaError {
    /// The subject was neither a video nor a camera track.
    SubjectKind {
        /// The offending id.
        id: ContentId,
    },
    /// The presentation resource did not match the kind it must be.
    PresentationKind {
        /// The offending id.
        id: ContentId,
        /// The kind it must be.
        expected: ContentKind,
    },
    /// The known duration was zero.
    ZeroDuration,
    /// The known frame size had a zero side.
    ZeroFrameSize,
    /// The known format label was empty.
    EmptyFormat,
    /// Two actions shared an id.
    DuplicateAction {
        /// The shared id.
        id: u32,
    },
    /// An action fired after the known end of the scene.
    ActionAfterEnd {
        /// The action id.
        id: u32,
        /// Its tick.
        at_tick: u64,
        /// The known duration.
        duration_ticks: u64,
    },
    /// An objective event named content that is not an objective.
    NotAnObjective {
        /// The offending id.
        id: ContentId,
    },
}

impl fmt::Display for CinematicSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectKind { id } => {
                write!(f, "cinematic subject {id} is not a video or camera track")
            }
            Self::PresentationKind { id, expected } => {
                write!(f, "presentation resource {id} is not a {expected}")
            }
            Self::ZeroDuration => write!(f, "the cinematic duration must not be zero"),
            Self::ZeroFrameSize => write!(f, "the frame size must not have a zero side"),
            Self::EmptyFormat => write!(f, "the media format label must not be empty"),
            Self::DuplicateAction { id } => write!(f, "semantic action {id} is declared twice"),
            Self::ActionAfterEnd {
                id,
                at_tick,
                duration_ticks,
            } => write!(
                f,
                "semantic action {id} fires at tick {at_tick}, after the end at {duration_ticks}"
            ),
            Self::NotAnObjective { id } => write!(f, "objective event {id} is not an objective"),
        }
    }
}

impl std::error::Error for CinematicSchemaError {}

/// One declared cinematic.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCinematic {
    subject: ContentId,
    origin: Origin,
    provenance: Provenance,
    presentation: DeclaredPresentation,
    duration_ticks: Resolved<u64>,
    frame_size: Resolved<(u32, u32)>,
    skippable: Resolved<bool>,
    pause: Resolved<DeclaredPausePolicy>,
    clock: Resolved<DeclaredClock>,
    recovery: Resolved<DeclaredRecovery>,
    actions: Vec<DeclaredAction>,
}

impl DeclaredCinematic {
    /// Builds a declared cinematic, validating every known value.
    ///
    /// # Errors
    ///
    /// [`CinematicSchemaError`] naming the first invalid known value.
    #[expect(clippy::too_many_arguments, reason = "one field per declared record")]
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        provenance: Provenance,
        presentation: DeclaredPresentation,
        duration_ticks: Resolved<u64>,
        frame_size: Resolved<(u32, u32)>,
        skippable: Resolved<bool>,
        pause: Resolved<DeclaredPausePolicy>,
        clock: Resolved<DeclaredClock>,
        recovery: Resolved<DeclaredRecovery>,
        actions: Vec<DeclaredAction>,
    ) -> Result<Self, CinematicSchemaError> {
        if !matches!(
            subject.kind(),
            ContentKind::Video | ContentKind::CameraTrack
        ) {
            return Err(CinematicSchemaError::SubjectKind { id: subject });
        }
        match &presentation {
            DeclaredPresentation::PrerenderedVideo { media, format } => {
                require_kind(media, ContentKind::Video)?;
                if let Resolved::Known(known) = format
                    && known.value.trim().is_empty()
                {
                    return Err(CinematicSchemaError::EmptyFormat);
                }
            }
            DeclaredPresentation::InEngine { camera_track } => {
                require_kind(camera_track, ContentKind::CameraTrack)?;
            }
        }
        let known_duration = match &duration_ticks {
            Resolved::Known(known) if known.value == 0 => {
                return Err(CinematicSchemaError::ZeroDuration);
            }
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        };
        if let Resolved::Known(known) = &frame_size
            && (known.value.0 == 0 || known.value.1 == 0)
        {
            return Err(CinematicSchemaError::ZeroFrameSize);
        }
        let mut seen = BTreeSet::new();
        for action in &actions {
            if !seen.insert(action.id) {
                return Err(CinematicSchemaError::DuplicateAction { id: action.id });
            }
            if let Some(duration_ticks) = known_duration
                && action.at_tick > duration_ticks
            {
                return Err(CinematicSchemaError::ActionAfterEnd {
                    id: action.id,
                    at_tick: action.at_tick,
                    duration_ticks,
                });
            }
            if let DeclaredEffect::ObjectiveEvent(objective) = &action.effect
                && objective.kind() != ContentKind::Objective
            {
                return Err(CinematicSchemaError::NotAnObjective {
                    id: objective.clone(),
                });
            }
        }
        Ok(Self {
            subject,
            origin,
            provenance,
            presentation,
            duration_ticks,
            frame_size,
            skippable,
            pause,
            clock,
            recovery,
            actions,
        })
    }

    /// The catalog subject.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The record's provenance.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// What shows the scene.
    #[must_use]
    pub const fn presentation(&self) -> &DeclaredPresentation {
        &self.presentation
    }

    /// The scene length in cinematic ticks, or an explicit unknown.
    #[must_use]
    pub const fn duration_ticks(&self) -> &Resolved<u64> {
        &self.duration_ticks
    }

    /// The source frame size in pixels, or an explicit unknown.
    #[must_use]
    pub const fn frame_size(&self) -> &Resolved<(u32, u32)> {
        &self.frame_size
    }

    /// Whether the scene can be skipped, or an explicit unknown.
    #[must_use]
    pub const fn skippable(&self) -> &Resolved<bool> {
        &self.skippable
    }

    /// The pause policy, or an explicit unknown.
    #[must_use]
    pub const fn pause(&self) -> &Resolved<DeclaredPausePolicy> {
        &self.pause
    }

    /// The master clock, or an explicit unknown.
    #[must_use]
    pub const fn clock(&self) -> &Resolved<DeclaredClock> {
        &self.clock
    }

    /// The media-failure recovery, or an explicit unknown.
    #[must_use]
    pub const fn recovery(&self) -> &Resolved<DeclaredRecovery> {
        &self.recovery
    }

    /// The semantic actions.
    #[must_use]
    pub fn actions(&self) -> &[DeclaredAction] {
        &self.actions
    }
}

fn require_kind(id: &ContentId, expected: ContentKind) -> Result<(), CinematicSchemaError> {
    if id.kind() == expected {
        Ok(())
    } else {
        Err(CinematicSchemaError::PresentationKind {
            id: id.clone(),
            expected,
        })
    }
}

fn designed<T>(value: T, provenance: &Provenance) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance.clone()))
}

/// The designed synthetic cinematic: a 100-tick prerendered 4:3 scene whose
/// actions raise two objective events and return control on the final frame.
#[must_use]
pub fn declared_synthetic_cinematic() -> DeclaredCinematic {
    let provenance =
        Provenance::designed(ClaimId::new("f40a.synthetic").expect("the claim id is valid"));
    let objective = |key| {
        ContentId::from_source(ContentKind::Objective, key)
            .expect("the synthetic objective id is valid")
    };
    let video = ContentId::from_source(ContentKind::Video, "synthetic.f40a.intro")
        .expect("the synthetic video id is valid");
    DeclaredCinematic::try_new(
        video.clone(),
        Origin::SyntheticFixture,
        provenance.clone(),
        DeclaredPresentation::PrerenderedVideo {
            media: video,
            format: designed("synthetic".to_owned(), &provenance),
        },
        designed(100, &provenance),
        designed((640, 480), &provenance),
        designed(true, &provenance),
        designed(DeclaredPausePolicy::SimulationPaused, &provenance),
        designed(DeclaredClock::Audio, &provenance),
        designed(DeclaredRecovery::ApplyRemainingSemantics, &provenance),
        vec![
            DeclaredAction {
                id: 1,
                at_tick: 0,
                effect: DeclaredEffect::ObjectiveEvent(objective("synthetic.f40a.briefed")),
            },
            DeclaredAction {
                id: 2,
                at_tick: 50,
                effect: DeclaredEffect::ObjectiveEvent(objective("synthetic.f40a.midpoint")),
            },
            DeclaredAction {
                id: 3,
                at_tick: 100,
                effect: DeclaredEffect::ReturnControlToPlayer,
            },
        ],
    )
    .expect("the synthetic cinematic is valid")
}

/// Why a camera-carrier record leaves every load-bearing cinematic value
/// unknown (F40-D3): the camera cinematics of `cam_anim.zbd` are measured to
/// be orchestration records — animation references, scene indices and node
/// tables — and no measured field carries a duration in ticks, a pose, a
/// frame size, a skippability, a pause policy, a master clock or a failure
/// recovery (statement spans in the original's stored unit are #690's
/// measurement; the unit and the tick conversion are its named unknowns)
/// (`docs/findings/2026-10-13-f40-d3-cam-anim-camera-format.md`).
const CAMERA_SEQUENCE_UNKNOWN_REASON: &str = "the retail cam_anim.zbd camera-cinematic records \
     carry no measured value for this field: their event streams are decoded as opaque TLV \
     instructions, and no per-tick pose, tick-unit duration or policy is established (F40-D3)";

/// Declares one measured camera-cinematic record as an in-engine cinematic
/// whose every load-bearing value is an explicit unknown (F40-D3).
///
/// The subject and the presentation are the measured pair — a
/// [`ContentKind::CameraTrack`] resource presented in-engine — and everything
/// the original leaves unmeasured stays [`Resolved::Unknown`] under its own
/// `f40d3.*` claim, so lowering refuses the record by claim
/// (`cs_app::cinematics::lower_cinematic`) instead of playing invented
/// numbers. This constructor never guesses a duration, a pose keyframe or a
/// policy.
///
/// # Errors
///
/// [`CinematicSchemaError`] when `camera_track` is not a
/// [`ContentKind::CameraTrack`].
pub fn declared_camera_sequence(
    origin: Origin,
    provenance: Provenance,
    camera_track: ContentId,
) -> Result<DeclaredCinematic, CinematicSchemaError> {
    fn unknown<T>(claim: &str) -> Resolved<T> {
        Resolved::unknown(
            ClaimId::new(claim).expect("the f40d3 claim ids are valid"),
            CAMERA_SEQUENCE_UNKNOWN_REASON,
        )
        .expect("the reason is nonempty")
    }
    DeclaredCinematic::try_new(
        camera_track.clone(),
        origin,
        provenance,
        DeclaredPresentation::InEngine { camera_track },
        unknown("f40d3.duration"),
        unknown("f40d3.frame_size"),
        unknown("f40d3.skippable"),
        unknown("f40d3.pause_policy"),
        unknown("f40d3.clock"),
        unknown("f40d3.recovery"),
        Vec::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuild(
        edit: impl FnOnce(&mut DeclaredPresentation, &mut Resolved<u64>, &mut Vec<DeclaredAction>),
    ) -> Result<DeclaredCinematic, CinematicSchemaError> {
        let base = declared_synthetic_cinematic();
        let mut presentation = base.presentation.clone();
        let mut duration = base.duration_ticks.clone();
        let mut actions = base.actions.clone();
        edit(&mut presentation, &mut duration, &mut actions);
        DeclaredCinematic::try_new(
            base.subject.clone(),
            base.origin.clone(),
            base.provenance.clone(),
            presentation,
            duration,
            base.frame_size.clone(),
            base.skippable.clone(),
            base.pause.clone(),
            base.clock.clone(),
            base.recovery.clone(),
            actions,
        )
    }

    #[test]
    fn accept_f40_a_declared_cinematic_separates_presentation_from_semantics() {
        let cinematic = declared_synthetic_cinematic();
        assert!(matches!(
            cinematic.presentation(),
            DeclaredPresentation::PrerenderedVideo { .. }
        ));
        assert_eq!(cinematic.actions().len(), 3);
        assert_eq!(cinematic.origin(), &Origin::SyntheticFixture);
        assert!(cinematic.duration_ticks().is_known());
    }

    #[test]
    fn accept_f40_a_declared_cinematic_refuses_invalid_known_values() {
        let provenance = Provenance::designed(ClaimId::new("f40a.test").unwrap());
        assert_eq!(
            rebuild(|_, d, _| *d = Resolved::Known(Known::new(0, provenance.clone()))).unwrap_err(),
            CinematicSchemaError::ZeroDuration
        );
        assert!(matches!(
            rebuild(|_, _, a| a[0].at_tick = 101).unwrap_err(),
            CinematicSchemaError::ActionAfterEnd { .. }
        ));
        assert_eq!(
            rebuild(|_, _, a| a[1].id = 1).unwrap_err(),
            CinematicSchemaError::DuplicateAction { id: 1 }
        );
        let video = ContentId::from_source(ContentKind::Video, "x").unwrap();
        assert!(matches!(
            rebuild(|_, _, a| a[0].effect = DeclaredEffect::ObjectiveEvent(video.clone()))
                .unwrap_err(),
            CinematicSchemaError::NotAnObjective { .. }
        ));
        assert!(matches!(
            rebuild(|p, _, _| {
                *p = DeclaredPresentation::InEngine {
                    camera_track: video.clone(),
                };
            })
            .unwrap_err(),
            CinematicSchemaError::PresentationKind { .. }
        ));
    }

    #[test]
    fn accept_f40_a_unknown_values_stay_unknown_and_are_not_rejected() {
        let unknown = Resolved::<u64>::unknown(
            ClaimId::new("f40a.duration").unwrap(),
            "the original runtime is unmeasured",
        )
        .unwrap();
        let cinematic = rebuild(|_, d, a| {
            *d = unknown.clone();
            a[0].at_tick = 9999;
        })
        .unwrap();
        assert!(!cinematic.duration_ticks().is_known());
    }
}

// ------------------------------------------------------------- retail ----
//
// The measured trigger inventory of the ten retail prerendered videos
// (F40-D2). Everything here is filled **only** where original bytes state
// it (AGENTS.md rule 4): the measurement runs over decoded original
// members — the `fmv.zrd` member of `ZBD/zrdr.zbd` and the
// `ASSETS/LAYOUT.CSV` and `ASSETS/SCRIPTS/*.SCRIPT` members of
// `GOSDATA/ASSETS/crimson.rof` — and a value no named source states stays
// an explicit [`Resolved::Unknown`] carrying its claim id and reason. The
// narrative evidence trail (engine-image corroboration, symbol-map names,
// what stays unread) is `docs/findings/2026-10-13-f40-d2-cinematic-trigger-
// inventory.md`; this module holds the typed record, not the narrative.

/// Claim id of the measured `.mpg` container label.
///
/// The label is the original's own file-name extension, spelled by its own
/// records (`PLAYAVI "MSopen1.mpg"`, `MOVIE=M,CrimFlag.MPG,…`) and by the
/// engine's FMV path literal `Assets\Graphics\MPG`. It is a label, not a
/// codec claim: no byte read here states which codec the streams use (the
/// engine image's `zvid_dshow.cpp`/`AVIFileInit`/`ICDecompress` strings show
/// a DirectShow/AVIFile decode path, not a codec identity).
pub const CLAIM_RETAIL_FORMAT_MPG: &str = "f40.d2.retail.format.mpg";

/// Claim id of the measured skippability of the chapter-intro cinematic.
///
/// `CAMPAIGNINTRO.SCRIPT` mails its own `11006` done-message from a
/// `key_escape`/`key_space`/`key_return` case and from a left-click handler;
/// its `11006` handler resumes the paused passenger-cabin page
/// (`script_continue @passengercabin@`) and ends the intro page, so a skip
/// reaches the same measured semantic end as the movie control's natural
/// completion (which mails `11006` to the page from `gui_execute`).
pub const CLAIM_RETAIL_SKIP_CAMPAIGN_INTRO: &str = "f40.d2.retail.skippable.campaignintro";

/// Claim id of the measured skippability of the final-cinema cinematic.
///
/// `FINALCINEMA.SCRIPT` mails its own `11006` done-message from
/// `key_escape` and from a left-click handler; its `11006` handler runs the
/// scrapbook page and ends the cinema page — the same measured end the movie
/// control's natural completion reaches.
pub const CLAIM_RETAIL_SKIP_FINAL_CINEMA: &str = "f40.d2.retail.skippable.finalcinema";

/// Claim id of the explicit unknown on the skippability of an
/// engine-FMV-sequence video (`msopen1`, `zipper`, `chap0`).
///
/// These are played by the engine's FMV player (`D:\zipper\gamez\zFMV\
/// fmv_script.cpp`); no named script source of the installation handles
/// their playback, so whether a key or button skips them is engine state no
/// byte read here states.
pub const CLAIM_RETAIL_SKIP_ENGINE_FMV: &str = "f40.d2.retail.skippable.engine_fmv";

/// Claim id of the explicit unknown on the skippability of a menu-backdrop
/// video (`crimflag` behind the main-menu/save/load/preferences pages).
///
/// The pages state no end and no skip for the backdrop: with `# Loops` 0 the
/// movie control loops until the page it belongs to is replaced, which is
/// menu navigation, not a stated skip action.
pub const CLAIM_RETAIL_SKIP_MENU_BACKDROP: &str = "f40.d2.retail.skippable.menu_backdrop";

/// Claim id of the explicit unknown on a video whose triggers are measured
/// but whose sources state no skip input and no end path.
pub const CLAIM_RETAIL_SKIP_UNREAD: &str = "f40.d2.retail.skippable.unread";

/// Claim id of the explicit unknown on every retail scene's simulation pause
/// policy.
///
/// The named sources run in the UI shell; no byte read here states the
/// engine player's simulation-pause policy (the `framerate` settings the
/// control sends around `play`/`loop`/`stop` are interface frame-rate
/// measures, not a simulation-pause statement).
pub const CLAIM_RETAIL_PAUSE_UNMEASURED: &str = "f40.d2.retail.pause.unmeasured";

/// Claim id of the explicit unknown on every retail scene's master clock.
pub const CLAIM_RETAIL_CLOCK_UNMEASURED: &str = "f40.d2.retail.clock.unmeasured";

/// Claim id under which the measurement records that no named source states
/// an objective-event or aircraft-control semantic action for any of the ten
/// videos: every scene runs in the UI shell, and the semantic transitions
/// the sources do state are page-level (recorded as
/// [`RetailSemanticEnd`]), not mission-state actions. Every retail record's
/// [`RetailCinematic::actions`] is empty under this claim.
pub const CLAIM_RETAIL_ACTIONS_NONE: &str = "f40.d2.retail.actions.none";

/// The installation-relative spelling of the reader archive the FMV
/// definitions live in (the same archive `crate::catalog::baseline`
/// spells as its `FMV_ARCHIVE`).
pub const RETAIL_FMV_ARCHIVE: &str = "ZBD/zrdr.zbd";

/// The FMV definitions member (the same member `crate::catalog::baseline`
/// spells as its `FMV_MEMBER`).
pub const RETAIL_FMV_MEMBER: &str = "fmv.zrd";

/// The installation-relative spelling of the ROF container the layout and
/// script sources live in (the same container `crate::catalog::baseline`
/// spells as its `VIDEO_LAYOUT_CONTAINER`).
pub const RETAIL_SCRIPT_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The layout member the movie records are read from.
pub const RETAIL_LAYOUT_MEMBER: &str = "ASSETS/LAYOUT.CSV";

/// The UI script member that triggers the chapter-intro cinematic from the
/// passenger cabin.
pub const RETAIL_PASSENGER_CABIN_MEMBER: &str = "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT";

/// The UI script member that constructs and plays the chapter movie.
pub const RETAIL_CAMPAIGN_INTRO_MEMBER: &str = "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT";

/// The UI script member that plays the final cinema.
pub const RETAIL_FINAL_CINEMA_MEMBER: &str = "ASSETS/SCRIPTS/FINALCINEMA.SCRIPT";

/// The control member whose `AL` class is the movie player.
pub const RETAIL_CONTROL_MEMBER: &str = "ASSETS/SCRIPTS/CTL.SCRIPT";

/// The native callback id whose return builds the chapter movie's file name
/// (`callback($$E$$, 2151, FC)`). Its **meaning and value domain are
/// unread** — measured as a call shape only, exactly as the F38-B host-call
/// stage records native callback ids.
pub const RETAIL_CHAPTER_CALLBACK_ID: i64 = 2151;

/// One byte fragment of one decoded original source, located by the
/// measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredFragment {
    /// The source the fragment was searched in (container + member spelling).
    pub source: String,
    /// The fragment as searched (single-line original text).
    pub fragment: String,
    /// How many times the source contains it.
    pub occurrences: usize,
    /// The offset of the first occurrence — `None` when the source contains
    /// it zero times, the measured absence a decision rests on.
    pub first_offset: Option<usize>,
}

/// How the measured sources trigger one retail video's playback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetailTrigger {
    /// A `PLAYAVI` action of an `fmv.zrd` sequence names the video; the
    /// engine's FMV player runs the sequence. Which engine event invokes the
    /// sequence is **not** measured (the engine image registers the sequence
    /// names `INTRO`/`CHAP0` against `fmv.zrd` at startup; the invocation
    /// sites were not read).
    FmvSequence {
        /// The sequence's own name (`INTRO`, `CHAP0`).
        sequence: String,
        /// The archive the sequence was read from.
        archive: &'static str,
        /// The member the sequence was read from.
        member: &'static str,
    },
    /// A UI script page creates the movie control (`@ctl@AL`) bound to a
    /// layout key whose record's file field is this video.
    UiMoviePage {
        /// The page: the layout record's section name with its `@` delimiters
        /// removed — which the layout member documents as the script's base
        /// name (`;[@scriptname@] the base name of the script without
        /// extension`).
        page: String,
        /// The record's key (`MOVIE`, `CM_MOVIE`, `CF_MOVIE`).
        layout_key: String,
        /// The record's `# Loops` field, measured when it parses as an
        /// integer. `0` is the control's continuous-loop path.
        loops: Option<u32>,
        /// Whether the bound page creates its movie control inside an
        /// `if ($$KW$$)` block (`$$freturntogame$$` in the retail script
        /// symbol map), measured by brace-matched block scope.
        freturntogame_guarded: bool,
        /// Whether the bound page overrides the control's file-name field
        /// (`<obj>.BL = …`) before initializing it, which supersedes the
        /// record's own file name (the measured campaign-intro case: the
        /// record spells `CrimFlag.MPG`, the page plays the constructed
        /// `chap<FC>.mpg`).
        file_superseded_by_constructed_name: bool,
    },
    /// The chapter construction: `CAMPAIGNINTRO.SCRIPT` builds
    /// `"chap" conv$(FC) ".mpg"` from native callback
    /// [`RETAIL_CHAPTER_CALLBACK_ID`]'s return and `CTL.SCRIPT` resolves and
    /// plays it. Which chapter number belongs to which file is **unread**
    /// (the callback's return domain is engine state; see the F14-D
    /// chapter-binding finding).
    ChapterConstruction {
        /// The page that constructs and plays the name.
        page: &'static str,
        /// The control member that resolves and plays it.
        control_member: &'static str,
        /// The native callback id the construction reads.
        callback_id: i64,
    },
}

/// What the measured sources state runs at a retail scene's semantic end —
/// the page-level transition, kept apart from media presentation (F40
/// non-negotiable behavior 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetailSemanticEnd {
    /// The done handler resumes a named paused page
    /// (`script_continue @passengercabin@`): control hands back to that
    /// page.
    UiScriptResume {
        /// The resumed page's name.
        page: String,
    },
    /// The done handler runs a named script page (`script_run` of
    /// `scrapbook.script`).
    UiScriptRun {
        /// The run script's file name as the source spells it.
        script: String,
    },
    /// The movie control loops (`# Loops` 0) and the page states no end for
    /// the backdrop.
    LoopForever,
    /// The scene is played by the engine; no named script source states what
    /// follows.
    EngineUnread,
}

/// One measured retail video with everything its named sources state — and
/// explicit unknowns where they state nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailCinematic {
    video: String,
    triggers: Vec<RetailTrigger>,
    skippable: Resolved<bool>,
    pause: Resolved<DeclaredPausePolicy>,
    clock: Resolved<DeclaredClock>,
    format: Resolved<String>,
    semantic_end: RetailSemanticEnd,
    actions: Vec<DeclaredAction>,
}

impl RetailCinematic {
    /// The video's original spelling, as the first source that named it
    /// spells it.
    #[must_use]
    pub fn video(&self) -> &str {
        &self.video
    }

    /// Every measured trigger of this video, in source order.
    #[must_use]
    pub fn triggers(&self) -> &[RetailTrigger] {
        &self.triggers
    }

    /// Whether the scene can be skipped, or an explicit unknown with the
    /// claim id that owns the question.
    #[must_use]
    pub fn skippable(&self) -> &Resolved<bool> {
        &self.skippable
    }

    /// The simulation pause policy — an explicit unknown on every retail
    /// scene (no named source states one).
    #[must_use]
    pub fn pause(&self) -> &Resolved<DeclaredPausePolicy> {
        &self.pause
    }

    /// The master clock — an explicit unknown on every retail scene.
    #[must_use]
    pub fn clock(&self) -> &Resolved<DeclaredClock> {
        &self.clock
    }

    /// The measured container label (`.mpg`), or an explicit unknown.
    #[must_use]
    pub fn format(&self) -> &Resolved<String> {
        &self.format
    }

    /// The measured semantic end of the scene.
    #[must_use]
    pub fn semantic_end(&self) -> &RetailSemanticEnd {
        &self.semantic_end
    }

    /// The semantic actions in the [`DeclaredCinematic`] vocabulary.
    ///
    /// Measured empty for every retail scene: no named source states an
    /// objective-event or aircraft-control action for any of the ten videos
    /// ([`CLAIM_RETAIL_ACTIONS_NONE`]); the page transitions the sources do
    /// state are [`Self::semantic_end`].
    #[must_use]
    pub fn actions(&self) -> &[DeclaredAction] {
        &self.actions
    }
}

/// The measured trigger inventory of the retail video collection.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailCinematicInventory {
    cinematics: Vec<RetailCinematic>,
    fragments: Vec<MeasuredFragment>,
    playavi_not_text: usize,
    movie_record_without_mpg_name: usize,
}

impl RetailCinematicInventory {
    /// One record per named retail video, in measurement order: FMV names
    /// first, then layout names, then chapter-construction files.
    #[must_use]
    pub fn cinematics(&self) -> &[RetailCinematic] {
        &self.cinematics
    }

    /// Every fragment the measurement searched for, found or absent.
    #[must_use]
    pub fn fragments(&self) -> &[MeasuredFragment] {
        &self.fragments
    }

    /// How many `PLAYAVI` actions carried a non-text argument (no name can
    /// be read from them).
    #[must_use]
    pub fn playavi_not_text(&self) -> usize {
        self.playavi_not_text
    }

    /// How many layout movie records the schema classified but whose file
    /// field is not an `.mpg` name.
    #[must_use]
    pub fn movie_record_without_mpg_name(&self) -> usize {
        self.movie_record_without_mpg_name
    }

    /// The record whose video name matches `name` case-insensitively, or
    /// `None` when no measured record is that video.
    #[must_use]
    pub fn by_video(&self, name: &str) -> Option<&RetailCinematic> {
        self.cinematics
            .iter()
            .find(|cinematic| cinematic.video.eq_ignore_ascii_case(name))
    }
}

/// One UI script page that can create the movie control: its name (the
/// script's base name) and its decoded bytes.
#[derive(Clone, Copy, Debug)]
pub struct UiPageSource<'a> {
    /// The page's base name (`mainmenu`, `campaignintro`, …).
    pub page: &'a str,
    /// The decoded script member's bytes.
    pub script: &'a [u8],
}

/// The decoded original sources the retail trigger measurement reads.
///
/// Every byte slice is the **decoded** member (compressed ROF members are
/// decompressed by the production reader before they get here); nothing
/// here reads an installation path itself, so the measurement is one pure
/// function the retail acceptance test drives with production-read bytes
/// and the synthetic tests drive with authored fixtures.
#[derive(Clone, Copy, Debug)]
pub struct RetailCinematicSources<'a> {
    /// The decoded `fmv.zrd` member bytes.
    pub fmv_member: &'a [u8],
    /// The decoded `ASSETS/LAYOUT.CSV` member bytes.
    pub layout_member: &'a [u8],
    /// The UI pages that can create the movie control, in a stable order
    /// (retail: mainmenu, save, load, preferences, campaignintro,
    /// finalcinema).
    pub ui_pages: &'a [UiPageSource<'a>],
    /// The decoded `PASSENGERCABIN.SCRIPT` member bytes.
    pub passenger_cabin: &'a [u8],
    /// The decoded `CTL.SCRIPT` member bytes.
    pub control: &'a [u8],
    /// The installation's `.mpg` file names, any spelling, as discovered
    /// (retail: the ten files under `GOSDATA/ASSETS/GRAPHICS/MPG/`).
    pub mpg_files: &'a [&'a str],
}

/// Why the retail sources could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CinematicMeasurementError {
    /// The `fmv.zrd` bytes do not decode as a `.zrd` document.
    FmvDecode {
        /// The decoder's stable refusal code.
        code: &'static str,
        /// The decoder's error offset.
        offset: u64,
    },
    /// The layout member does not read as a keyed field list.
    LayoutParse {
        /// The parse error, rendered.
        message: String,
    },
}

impl fmt::Display for CinematicMeasurementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FmvDecode { code, offset } => write!(
                f,
                "{RETAIL_FMV_ARCHIVE} member {RETAIL_FMV_MEMBER} does not decode as a .zrd \
                 document: {code} at offset {offset}"
            ),
            Self::LayoutParse { message } => write!(
                f,
                "{RETAIL_SCRIPT_CONTAINER} member {RETAIL_LAYOUT_MEMBER} does not read as a \
                 keyed field list: {message}"
            ),
        }
    }
}

impl std::error::Error for CinematicMeasurementError {}

/// The substring search the fragment measurement uses (no regex, no
/// dependency: fragments are single lines of ASCII script text).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// One fragment search over one decoded source: what was searched for and
/// where (or whether) it was found.
struct FragmentSearch<'a> {
    source: String,
    bytes: &'a [u8],
    rows: Vec<(String, usize, Option<usize>)>,
}

impl<'a> FragmentSearch<'a> {
    fn new(source: String, bytes: &'a [u8]) -> Self {
        Self {
            source,
            bytes,
            rows: Vec::new(),
        }
    }

    /// Searches `fragment` (single-line original text) and records its
    /// occurrence count and first offset.
    fn require(&mut self, fragment: &str) {
        let needle = fragment.as_bytes();
        let mut occurrences = 0usize;
        let mut first = None;
        let mut from = 0usize;
        while from + needle.len() <= self.bytes.len() {
            let Some(hit) = find_bytes(&self.bytes[from..], needle) else {
                break;
            };
            let at = from + hit;
            first.get_or_insert(at);
            occurrences += 1;
            from = at + needle.len().max(1);
        }
        self.rows.push((fragment.to_owned(), occurrences, first));
    }

    /// Whether `fragment` was found at least once.
    fn found(&self, fragment: &str) -> bool {
        self.rows
            .iter()
            .any(|(name, occurrences, _)| name == fragment && *occurrences > 0)
    }

    fn finish(self, out: &mut Vec<MeasuredFragment>) {
        for (fragment, occurrences, first_offset) in self.rows {
            out.push(MeasuredFragment {
                source: self.source.clone(),
                fragment,
                occurrences,
                first_offset,
            });
        }
    }
}

/// The byte range of the brace-matched block that follows `marker` — the
/// measured body of one handler — as an offset range into `bytes`.
fn block_range_after(bytes: &[u8], marker: &str) -> Option<std::ops::Range<usize>> {
    let start = if marker.is_empty() {
        0
    } else {
        find_bytes(bytes, marker.as_bytes())? + marker.len()
    };
    let open = find_bytes(&bytes[start..], b"{")? + start;
    let mut depth = 0usize;
    for (at, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + 1..at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The substring after `marker`, limited to the brace-matched block that
/// follows it.
fn block_after<'a>(bytes: &'a [u8], marker: &str) -> Option<&'a [u8]> {
    block_range_after(bytes, marker).map(|range| &bytes[range])
}

/// Whether some `if ($$KW$$)` block in `script` encloses `offset` — the
/// measured freturntogame guard over a page's movie-control creation.
fn freturntogame_guarded(script: &[u8], offset: usize) -> bool {
    let mut from = 0usize;
    while let Some(hit) = find_bytes(&script[from..], b"if ($$KW$$)") {
        let at = from + hit;
        let guarded = block_range_after(&script[at..], "")
            .is_some_and(|range| range.start + at <= offset && offset < range.end + at);
        if guarded {
            return true;
        }
        from = at + 1;
    }
    false
}

/// The `"<obj>.YC = \"<key>\""` binding offset of a page, if the page binds
/// `key` (case-insensitively) to a control.
fn yc_binding_offset(script: &[u8], key: &str) -> Option<usize> {
    let marker = b".YC = \"";
    let mut from = 0usize;
    while let Some(hit) = find_bytes(&script[from..], marker) {
        let value_at = from + hit + marker.len();
        let relative_end = find_bytes(&script[value_at..], b"\"")?;
        let end = value_at + relative_end;
        let value = &script[value_at..end];
        if value.len() == key.len()
            && value
                .iter()
                .zip(key.bytes())
                .all(|(left, right)| left.eq_ignore_ascii_case(&right))
        {
            return Some(from + hit);
        }
        from = value_at + 1;
    }
    None
}

/// The `script_continue @<page>@` target of a block.
fn script_continue_target(block: &[u8]) -> Option<String> {
    let marker = b"script_continue @";
    let at = find_bytes(block, marker)? + marker.len();
    let end = find_bytes(&block[at..], b"@")? + at;
    std::str::from_utf8(&block[at..end]).ok().map(str::to_owned)
}

/// The `script_run "assets\\scripts\\" "<file>"` target of a block.
fn script_run_target(block: &[u8]) -> Option<String> {
    let marker = br#"script_run "assets\\scripts\\""#;
    let at = find_bytes(block, marker)? + marker.len();
    let open = find_bytes(&block[at..], b"\"")? + at;
    let end = find_bytes(&block[open + 1..], b"\"")? + open + 1;
    std::str::from_utf8(&block[open + 1..end])
        .ok()
        .map(str::to_owned)
}

/// Whether `name` spells the chapter construction's product shape
/// (`chap` + decimal digits + `.mpg`, ASCII case-folded) — the same measured
/// shape the F14-D chapter stage recorded.
fn is_chapter_video_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("chap") else {
        return false;
    };
    let Some(digits) = rest.strip_suffix(".mpg") else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// A `Resolved::Unknown` with a statically valid claim id and a reason.
fn unmeasured<T>(claim: &str, reason: &str) -> Resolved<T> {
    Resolved::<T>::unknown(
        ClaimId::new(claim).expect("the inventory's claim ids are valid"),
        reason,
    )
    .expect("the inventory's unknown reasons are non-empty")
}

/// A `Resolved::Known` measured by this tool run (`ObservedTool` — never
/// `verified_original`, which no agent may self-award).
fn measured<T>(claim: &str, value: T) -> Resolved<T> {
    let claim_id = ClaimId::new(claim).expect("the inventory's claim ids are valid");
    let provenance = Provenance::new(claim_id, ClaimStatus::ObservedTool, None)
        .expect("an ObservedTool provenance needs no source span");
    Resolved::Known(Known::new(value, provenance))
}

/// The measured end-path evidence of one UI page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PageEndEvidence {
    skip_key: bool,
    skip_click: bool,
    skip_mail: bool,
    done_handler: bool,
    done_resume: Option<String>,
    done_run: Option<String>,
}

impl PageEndEvidence {
    /// The page states a skip input path that reaches its done handler.
    const fn skippable(&self) -> bool {
        self.skip_key && self.skip_click && self.skip_mail && self.done_handler
    }

    /// The page states no done handler at all (a looping backdrop page).
    const fn backdrop_only(&self) -> bool {
        !self.done_handler
    }
}

/// Measures one page's end-path evidence and records its fragments.
fn measure_page_end(
    source: String,
    script: &[u8],
    fragments: &mut Vec<MeasuredFragment>,
) -> PageEndEvidence {
    let mut search = FragmentSearch::new(source, script);
    search.require("key_escape");
    search.require("if (mouse.left == button_pressed)");
    search.require("mail (11006, this)");
    search.require("if (11006 == getmessage())");
    let evidence = PageEndEvidence {
        skip_key: search.found("key_escape"),
        skip_click: search.found("if (mouse.left == button_pressed)"),
        skip_mail: search.found("mail (11006, this)"),
        done_handler: search.found("if (11006 == getmessage())"),
        done_resume: block_after(script, "if (11006 == getmessage())")
            .and_then(script_continue_target),
        done_run: block_after(script, "if (11006 == getmessage())").and_then(script_run_target),
    };
    search.finish(fragments);
    evidence
}

/// The layout movie records of one decoded member.
struct LayoutMovie {
    key: String,
    section: Option<String>,
    file: String,
    loops: Option<u32>,
}

/// Measures the layout movie records: schema-classified `M` records whose
/// file field states an `.mpg` name.
fn measure_layout_movies(
    layout_member: &[u8],
) -> Result<(Vec<LayoutMovie>, usize), CinematicMeasurementError> {
    let mut context = ParseContext::with_defaults(RETAIL_LAYOUT_MEMBER);
    let list = read_keyed_list(&mut context, layout_member).map_err(|error| {
        CinematicMeasurementError::LayoutParse {
            message: error.to_string(),
        }
    })?;
    let mut movies = Vec::new();
    let mut without_mpg_name = 0usize;
    let mut section: Option<String> = None;
    for (index, line) in list.lines().iter().enumerate() {
        match &line.kind {
            LineKind::Section { name } => {
                section = std::str::from_utf8(name).ok().map(str::to_owned);
            }
            LineKind::Entry(entry) => {
                let cs_formats::text::Fields::Split(fields) = &entry.fields else {
                    continue;
                };
                let config_entry = ConfigEntry {
                    line: (index + 1) as u64,
                    section: section.clone().map(|name| name.as_bytes().to_vec()),
                    key: entry.key.to_vec(),
                    value: ConfigRawValue::Fields(
                        fields
                            .iter()
                            .map(|field| ConfigRawField {
                                raw: field.raw.to_vec(),
                                quoted: field.quoted,
                            })
                            .collect(),
                    ),
                };
                if RecordSchema::for_entry(&config_entry)
                    != Some(RecordSchema::Layout(RecordKind::Movie))
                {
                    continue;
                }
                let ConfigRawValue::Fields(fields) = &config_entry.value else {
                    continue;
                };
                let Some(file) = fields
                    .get(1)
                    .map(ConfigRawField::value)
                    .and_then(|value| std::str::from_utf8(value).ok())
                else {
                    without_mpg_name += 1;
                    continue;
                };
                if !file.to_ascii_lowercase().ends_with(".mpg") {
                    without_mpg_name += 1;
                    continue;
                }
                let loops = fields
                    .get(7)
                    .map(ConfigRawField::value)
                    .and_then(|value| std::str::from_utf8(value).ok()?.parse().ok());
                movies.push(LayoutMovie {
                    key: String::from_utf8_lossy(&config_entry.key).into_owned(),
                    section: section.clone(),
                    file: file.to_owned(),
                    loops,
                });
            }
            _ => {}
        }
    }
    Ok((movies, without_mpg_name))
}

/// The `fmv.zrd` PLAYAVI names with their sequence labels.
///
/// The measured document shape (retail `fmv.zrd`, 717 bytes, decoded
/// exactly): the root is a list of blocks; each block's children are
/// (name, body) pairs — `IMAGE_PATH`/`["video"]`, `INTRO`/the action list,
/// `CHAP0`/its action list. A `PLAYAVI` action is a list whose first child
/// is the text tag and whose second child is the video's file name.
fn fmv_playavi_pairs(
    fmv_member: &[u8],
) -> Result<(Vec<(String, String)>, usize), CinematicMeasurementError> {
    let root = decode_zrd(fmv_member).map_err(|error| CinematicMeasurementError::FmvDecode {
        code: error.code(),
        offset: error.offset(),
    })?;
    let mut pairs = Vec::new();
    let mut not_text = 0usize;
    let Some(blocks) = root.as_list() else {
        return Ok((pairs, not_text));
    };
    for block in blocks {
        let Some(children) = block.as_list() else {
            continue;
        };
        for (index, child) in children.iter().enumerate() {
            let (Some(name), Some(body)) = (
                child.as_text(),
                children.get(index + 1).and_then(ZrdValue::as_list),
            ) else {
                continue;
            };
            for action in body {
                let Some(action) = action.as_list() else {
                    continue;
                };
                if action.first().and_then(ZrdValue::as_text) != Some("PLAYAVI") {
                    continue;
                }
                match action.get(1).and_then(ZrdValue::as_text) {
                    Some(video) => pairs.push((name.to_owned(), video.to_owned())),
                    None => not_text += 1,
                }
            }
        }
    }
    Ok((pairs, not_text))
}

/// Measures the retail trigger inventory from the decoded original sources.
///
/// # Errors
///
/// [`CinematicMeasurementError`] when the FMV member does not decode as a
/// `.zrd` document or the layout member does not read as a keyed field
/// list. A fragment that is absent is **not** an error: it is the measured
/// absence the corresponding value stays unknown by.
pub fn measure_retail_cinematics(
    sources: &RetailCinematicSources<'_>,
) -> Result<RetailCinematicInventory, CinematicMeasurementError> {
    let mut fragments = Vec::new();

    // The FMV sequences: which videos a PLAYAVI action names.
    let (fmv_pairs, playavi_not_text) = fmv_playavi_pairs(sources.fmv_member)?;

    // The layout movie records: which videos a record names.
    let (layout_movies, movie_record_without_mpg_name) =
        measure_layout_movies(sources.layout_member)?;

    // Per-page end-path evidence, measured once per page.
    let mut page_rows: Vec<(String, PageEndEvidence)> = Vec::new();
    for ui_page in sources.ui_pages {
        let source = format!(
            "{RETAIL_SCRIPT_CONTAINER} member ASSETS/SCRIPTS/{}.SCRIPT",
            ui_page.page.to_ascii_uppercase()
        );
        let evidence = measure_page_end(source, ui_page.script, &mut fragments);
        page_rows.push((ui_page.page.to_ascii_lowercase(), evidence));
    }
    let page_evidence = |page: &str| {
        page_rows
            .iter()
            .find(|(name, _)| name == &page.to_ascii_lowercase())
            .map(|(_, evidence)| evidence)
    };

    // The chapter construction chain: the intro page's construction, the
    // cabin's trigger and the control's resolution and playback.
    let campaign_intro = sources
        .ui_pages
        .iter()
        .find(|page| page.page.eq_ignore_ascii_case("campaignintro"));
    let mut construction = FragmentSearch::new(
        format!("{RETAIL_SCRIPT_CONTAINER} member {RETAIL_CAMPAIGN_INTRO_MEMBER}"),
        campaign_intro.map_or(&[], |page| page.script),
    );
    construction.require(r#"GC.BL = "chap" conv$(FC) ".mpg""#);
    construction.require(r#"GC.YC = "cm_movie""#);
    construction.require("callback($$E$$, 2151, FC)");
    let construction_measured = construction.found(r#"GC.BL = "chap" conv$(FC) ".mpg""#)
        && construction.found(r#"GC.YC = "cm_movie""#)
        && construction.found("callback($$E$$, 2151, FC)");
    construction.finish(&mut fragments);

    let mut cabin = FragmentSearch::new(
        format!("{RETAIL_SCRIPT_CONTAINER} member {RETAIL_PASSENGER_CABIN_MEMBER}"),
        sources.passenger_cabin,
    );
    cabin.require("callback($$E$$, 2151, FC)");
    cabin.require("if (FC != 0)");
    cabin.require(r#"script_run "assets\\scripts\\" "campaignintro.script", 0x1999"#);
    cabin.require("script_pause @passengercabin@, 2");
    cabin.require("if (@globals@XQ)");
    let cabin_measured = cabin.found("callback($$E$$, 2151, FC)")
        && cabin.found("if (FC != 0)")
        && cabin.found(r#"script_run "assets\\scripts\\" "campaignintro.script", 0x1999"#);
    cabin.finish(&mut fragments);

    let mut control = FragmentSearch::new(
        format!("{RETAIL_SCRIPT_CONTAINER} member {RETAIL_CONTROL_MEMBER}"),
        sources.control,
    );
    control.require("movie CL");
    control.require(r#"CL = "assets\\graphics\\mpg\\" BL"#);
    control.require(r#"if (equal$(BL,""))"#);
    control.require("loop CL");
    control.require("getstatus(CL)");
    control.require("mail (11006, parent)");
    control.require("framerate = 10");
    let control_measured = control.found("movie CL")
        && control.found(r#"CL = "assets\\graphics\\mpg\\" BL"#)
        && control.found("mail (11006, parent)");
    control.finish(&mut fragments);

    let chapter_chain_measured = construction_measured && cabin_measured && control_measured;

    // Assemble one record per video, in source order.
    let key = |video: &str| video.to_ascii_lowercase();
    let mut order: Vec<String> = Vec::new();
    let mut triggers: BTreeMap<String, Vec<RetailTrigger>> = BTreeMap::new();
    let mut skippable: BTreeMap<String, Resolved<bool>> = BTreeMap::new();
    let mut semantic_end: BTreeMap<String, RetailSemanticEnd> = BTreeMap::new();

    // 1. FMV sequence names.
    for (sequence, video) in &fmv_pairs {
        triggers
            .entry(key(video))
            .or_default()
            .push(RetailTrigger::FmvSequence {
                sequence: sequence.clone(),
                archive: RETAIL_FMV_ARCHIVE,
                member: RETAIL_FMV_MEMBER,
            });
        if !order.contains(&key(video)) {
            order.push(key(video));
        }
        skippable.entry(key(video)).or_insert_with(|| {
            unmeasured(
                CLAIM_RETAIL_SKIP_ENGINE_FMV,
                "the sequence is played by the engine's FMV player; no named script source \
                 handles its playback, so whether a key or button skips it is engine state no \
                 byte read here states",
            )
        });
        semantic_end
            .entry(key(video))
            .or_insert(RetailSemanticEnd::EngineUnread);
    }

    // 2. Layout movie records, bound to their pages by the layout member's
    //    documented section-to-script rule plus the measured YC binding.
    for movie in &layout_movies {
        // The layout member documents its sections as
        // `;[@scriptname@] the base name of the script without extension`,
        // so a section's script-page name is its own name with the `@`
        // delimiters the section syntax wraps it in removed.
        let page_name = movie
            .section
            .clone()
            .unwrap_or_default()
            .trim_matches('@')
            .to_owned();
        let bound_page = sources
            .ui_pages
            .iter()
            .find(|page| page.page.eq_ignore_ascii_case(&page_name));
        let page_script = bound_page.map_or(&[][..], |page| page.script);
        let file_superseded = find_bytes(page_script, b".BL = ").is_some();
        let guarded = yc_binding_offset(page_script, &movie.key)
            .is_some_and(|offset| freturntogame_guarded(page_script, offset));
        triggers
            .entry(key(&movie.file))
            .or_default()
            .push(RetailTrigger::UiMoviePage {
                page: page_name.clone(),
                layout_key: movie.key.clone(),
                loops: movie.loops,
                freturntogame_guarded: guarded,
                file_superseded_by_constructed_name: file_superseded,
            });
        if !order.contains(&key(&movie.file)) {
            order.push(key(&movie.file));
        }
        // Skippability and semantic end come from the bound page's measured
        // end path. A superseded record contributes neither: its file does
        // not play from that page.
        let Some(evidence) = page_evidence(&page_name) else {
            continue;
        };
        if !file_superseded {
            if evidence.skippable() {
                let claim = if page_name.eq_ignore_ascii_case("finalcinema") {
                    CLAIM_RETAIL_SKIP_FINAL_CINEMA
                } else {
                    CLAIM_RETAIL_SKIP_CAMPAIGN_INTRO
                };
                skippable
                    .entry(key(&movie.file))
                    .or_insert_with(|| measured(claim, true));
            } else if evidence.backdrop_only() {
                skippable.entry(key(&movie.file)).or_insert_with(|| {
                    unmeasured(
                        CLAIM_RETAIL_SKIP_MENU_BACKDROP,
                        "the page states no end and no skip for the backdrop: with # Loops 0 \
                         the movie control loops until the page is replaced, which is menu \
                         navigation, not a stated skip action",
                    )
                });
                if movie.loops == Some(0) {
                    semantic_end
                        .entry(key(&movie.file))
                        .or_insert(RetailSemanticEnd::LoopForever);
                }
            }
            if let Some(resume) = &evidence.done_resume {
                semantic_end
                    .entry(key(&movie.file))
                    .or_insert(RetailSemanticEnd::UiScriptResume {
                        page: resume.clone(),
                    });
            } else if let Some(run) = &evidence.done_run {
                semantic_end
                    .entry(key(&movie.file))
                    .or_insert(RetailSemanticEnd::UiScriptRun {
                        script: run.clone(),
                    });
            }
        }
    }

    // 3. The chapter construction's products: every inventoried
    //    `chap<digits>.mpg` file no earlier source named, when the whole
    //    construction chain measured.
    if chapter_chain_measured {
        let intro_evidence = page_evidence("campaignintro");
        for file in sources.mpg_files {
            if !is_chapter_video_name(file) || order.contains(&key(file)) {
                continue;
            }
            triggers
                .entry(key(file))
                .or_default()
                .push(RetailTrigger::ChapterConstruction {
                    page: "campaignintro",
                    control_member: RETAIL_CONTROL_MEMBER,
                    callback_id: RETAIL_CHAPTER_CALLBACK_ID,
                });
            order.push(key(file));
            if let Some(evidence) = intro_evidence {
                if evidence.skippable() {
                    skippable
                        .entry(key(file))
                        .or_insert_with(|| measured(CLAIM_RETAIL_SKIP_CAMPAIGN_INTRO, true));
                }
                if let Some(resume) = &evidence.done_resume {
                    semantic_end
                        .entry(key(file))
                        .or_insert(RetailSemanticEnd::UiScriptResume {
                            page: resume.clone(),
                        });
                }
            }
        }
    }

    // Records: the first-spelling name, the measured values, and explicit
    // unknowns everywhere else.
    let mut cinematics = Vec::new();
    for video_key in order {
        let video = fmv_pairs
            .iter()
            .find(|(_, name)| key(name) == video_key)
            .map(|(_, name)| name.clone())
            .or_else(|| {
                layout_movies
                    .iter()
                    .find(|movie| key(&movie.file) == video_key)
                    .map(|movie| movie.file.clone())
            })
            .unwrap_or_else(|| video_key.clone());
        cinematics.push(RetailCinematic {
            video,
            triggers: triggers.get(&video_key).cloned().unwrap_or_default(),
            skippable: skippable.get(&video_key).cloned().unwrap_or_else(|| {
                unmeasured(
                    CLAIM_RETAIL_SKIP_UNREAD,
                    "the measured triggers of this video carry no skip input and no end path",
                )
            }),
            pause: unmeasured(
                CLAIM_RETAIL_PAUSE_UNMEASURED,
                "the named sources run in the UI shell; no byte read here states the engine \
                 player's simulation-pause policy",
            ),
            clock: unmeasured(
                CLAIM_RETAIL_CLOCK_UNMEASURED,
                "no named source states a master clock for the engine's video player",
            ),
            format: measured(CLAIM_RETAIL_FORMAT_MPG, "mpg".to_owned()),
            semantic_end: semantic_end
                .get(&video_key)
                .cloned()
                .unwrap_or(RetailSemanticEnd::EngineUnread),
            actions: Vec::new(),
        });
    }

    Ok(RetailCinematicInventory {
        cinematics,
        fragments,
        playavi_not_text,
        movie_record_without_mpg_name,
    })
}
