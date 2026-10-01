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

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

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
