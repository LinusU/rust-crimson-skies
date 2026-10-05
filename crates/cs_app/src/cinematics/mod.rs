//! The cinematic lowering boundary (F40-A).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module sits between the declared cinematic schema
//! ([`cs_content::cinematics`]) and the runtime contract
//! ([`cs_sim::cinematic_state`]), which cannot see each other. It contains:
//!
//! * [`lower_cinematic`] — a validated
//!   [`cs_content::cinematics::DeclaredCinematic`] becomes a
//!   [`CinematicPlan`]: the runtime script plus what the presentation needs.
//!   Every unresolved value (duration, skippability, pause policy, clock,
//!   recovery, a video's format and frame size) is refused by claim; the
//!   boundary never invents one, and never assumes a codec.
//! * [`begin`] — starts a player and turns a missing file or decoder into a
//!   `Failed` state that is never a completion (non-negotiable behavior 5).
//! * [`fit_letterboxed`] — fits a frame into a surface at its own aspect
//!   ratio, centred, never stretched (non-negotiable behavior 4).
//!
//! Decoded playback against the master clock is [`playback`] and authored
//! camera timelines are [`timeline`] (F40-B); the skip/pause wiring is F40-C.

pub mod playback;
pub mod timeline;

use std::fmt;

use cs_content::cinematics::{
    DeclaredCinematic, DeclaredClock, DeclaredEffect, DeclaredPausePolicy, DeclaredPresentation,
    DeclaredRecovery,
};
use cs_sim::cinematic_state::{
    CinematicPlayer, CinematicScript, FailureRecovery, MasterClock, MediaFailure, PausePolicy,
    PlayerError, ScriptError, SemanticAction, SemanticActionId, SemanticEffect,
};
use cs_sim::damage::ActorId;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

/// Why a declared cinematic could not be lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CinematicLowerError {
    /// A required value is `Resolved::Unknown`.
    Unknown {
        /// The declared field name.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// The runtime refused the assembled script.
    Script(ScriptError),
}

impl fmt::Display for CinematicLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "cinematic field {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::Script(source) => write!(f, "the runtime refused the script: {source}"),
        }
    }
}

impl std::error::Error for CinematicLowerError {}

/// What the presentation layer must show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresentationPlan {
    /// A prerendered video; the decoder is chosen by `format`.
    Video {
        /// The video resource.
        media: ContentId,
        /// The recorded container/codec label.
        format: String,
        /// The source frame size in pixels.
        frame_size: (u32, u32),
    },
    /// An in-engine camera sequence.
    Camera {
        /// The camera track.
        track: ContentId,
    },
}

/// The lowered runtime contract of one cinematic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CinematicPlan {
    /// The semantic script.
    pub script: CinematicScript,
    /// The presentation, kept apart from the script.
    pub presentation: PresentationPlan,
}

fn required<T: Clone>(value: &Resolved<T>, field: &'static str) -> Result<T, CinematicLowerError> {
    match value {
        Resolved::Known(known) => Ok(known.value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(CinematicLowerError::Unknown {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// Lowers a declared cinematic into the runtime contract.
///
/// # Errors
///
/// [`CinematicLowerError`] naming the first unresolved field or the runtime
/// script refusal.
pub fn lower_cinematic(declared: &DeclaredCinematic) -> Result<CinematicPlan, CinematicLowerError> {
    let presentation = match declared.presentation() {
        DeclaredPresentation::PrerenderedVideo { media, format } => PresentationPlan::Video {
            media: media.clone(),
            format: required(format, "format")?,
            frame_size: required(declared.frame_size(), "frame_size")?,
        },
        DeclaredPresentation::InEngine { camera_track } => PresentationPlan::Camera {
            track: camera_track.clone(),
        },
    };
    let pause = match required(declared.pause(), "pause")? {
        DeclaredPausePolicy::SimulationPaused => PausePolicy::SimulationPaused,
        DeclaredPausePolicy::SimulationRuns => PausePolicy::SimulationRuns,
    };
    let clock = match required(declared.clock(), "clock")? {
        DeclaredClock::Audio => MasterClock::Audio,
        DeclaredClock::Simulation => MasterClock::Simulation,
    };
    let recovery = match required(declared.recovery(), "recovery")? {
        DeclaredRecovery::ApplyRemainingSemantics => FailureRecovery::ApplyRemainingSemantics,
        DeclaredRecovery::Block => FailureRecovery::Block,
    };
    let actions = declared
        .actions()
        .iter()
        .map(|action| SemanticAction {
            id: SemanticActionId(action.id),
            at_tick: action.at_tick,
            effect: match &action.effect {
                DeclaredEffect::ObjectiveEvent(id) => SemanticEffect::ObjectiveEvent(id.clone()),
                DeclaredEffect::ReturnControlToPlayer => SemanticEffect::ReturnControlToPlayer,
            },
        })
        .collect();
    let script = CinematicScript::try_new(
        declared.subject().clone(),
        required(declared.duration_ticks(), "duration_ticks")?,
        required(declared.skippable(), "skippable")?,
        pause,
        clock,
        recovery,
        actions,
    )
    .map_err(CinematicLowerError::Script)?;
    Ok(CinematicPlan {
        script,
        presentation,
    })
}

/// What the loader found for the presentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaAvailability {
    /// The media file and a decoder are available.
    Present,
    /// The media file is not in the installation.
    MissingFile,
    /// The file exists but no decoder handles its format.
    NoDecoder,
}

/// Starts the player for `plan`. Anything but [`MediaAvailability::Present`]
/// for a video fails the player on the spot with a useful error, so a missing
/// copyrighted file is reported and never a black screen that "completes".
///
/// # Errors
///
/// [`PlayerError`] if the freshly built player refuses to start or fail.
pub fn begin(
    plan: &CinematicPlan,
    availability: &MediaAvailability,
    player_actor: ActorId,
) -> Result<CinematicPlayer, PlayerError> {
    let mut player = CinematicPlayer::new(plan.script.clone(), player_actor);
    player.start()?;
    if let PresentationPlan::Video { media, format, .. } = &plan.presentation {
        match availability {
            MediaAvailability::Present => {}
            MediaAvailability::MissingFile => {
                player.media_failed(MediaFailure::MissingMedia {
                    media: media.clone(),
                })?;
            }
            MediaAvailability::NoDecoder => {
                player.media_failed(MediaFailure::MissingDecoder {
                    format: format.clone(),
                })?;
            }
        }
    }
    Ok(player)
}

/// A pixel rectangle inside the output surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// A zero-sized frame or surface was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyExtent;

impl fmt::Display for EmptyExtent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the frame and the surface must both be non-empty")
    }
}

impl std::error::Error for EmptyExtent {}

/// Fits `frame` into `surface` at the frame's own aspect ratio, centred, with
/// bars on the unused axis. The result is never stretched, however large the
/// surface.
///
/// # Errors
///
/// [`EmptyExtent`] when either extent has a zero side.
pub fn fit_letterboxed(frame: (u32, u32), surface: (u32, u32)) -> Result<Viewport, EmptyExtent> {
    let (fw, fh) = (u64::from(frame.0), u64::from(frame.1));
    let (sw, sh) = (u64::from(surface.0), u64::from(surface.1));
    if fw == 0 || fh == 0 || sw == 0 || sh == 0 {
        return Err(EmptyExtent);
    }
    let (width, height) = if sw * fh <= sh * fw {
        (sw, fh * sw / fw)
    } else {
        (fw * sh / fh, sh)
    };
    // Both are bounded by the surface, so they fit in u32.
    let narrow = |v: u64| u32::try_from(v).expect("bounded by the surface");
    Ok(Viewport {
        x: narrow((sw - width) / 2),
        y: narrow((sh - height) / 2),
        width: narrow(width),
        height: narrow(height),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_content::cinematics::declared_synthetic_cinematic;
    use cs_sim::cinematic_state::{
        CinematicState, SYNTHETIC_PLAYER, SYNTHETIC_SWAPPED_PLAYER, synthetic_cinematic_id,
    };
    use cs_types::content::{Known, Provenance};

    fn plan() -> CinematicPlan {
        lower_cinematic(&declared_synthetic_cinematic()).unwrap()
    }

    #[test]
    fn accept_f40_a_lowering_maps_the_declared_cinematic_field_wise() {
        let plan = plan();
        assert_eq!(plan.script.duration_ticks(), 100);
        assert!(plan.script.skippable());
        assert_eq!(plan.script.pause(), PausePolicy::SimulationPaused);
        assert_eq!(plan.script.clock(), MasterClock::Audio);
        assert_eq!(plan.script.actions().len(), 3);
        assert_eq!(
            plan.presentation,
            PresentationPlan::Video {
                media: synthetic_cinematic_id(),
                format: "synthetic".into(),
                frame_size: (640, 480)
            }
        );
    }

    #[test]
    fn accept_f40_a_lowering_refuses_unknown_values_by_claim() {
        let base = declared_synthetic_cinematic();
        let provenance = Provenance::designed(ClaimId::new("f40a.test").unwrap());
        let unknown_bool =
            Resolved::<bool>::unknown(ClaimId::new("f40a.skip").unwrap(), "unmeasured").unwrap();
        let declared = DeclaredCinematic::try_new(
            base.subject().clone(),
            base.origin().clone(),
            provenance.clone(),
            base.presentation().clone(),
            base.duration_ticks().clone(),
            base.frame_size().clone(),
            unknown_bool,
            base.pause().clone(),
            base.clock().clone(),
            base.recovery().clone(),
            base.actions().to_vec(),
        )
        .unwrap();
        match lower_cinematic(&declared).unwrap_err() {
            CinematicLowerError::Unknown {
                field, claim_id, ..
            } => {
                assert_eq!(field, "skippable");
                assert_eq!(claim_id.as_str(), "f40a.skip");
            }
            other => panic!("unexpected {other:?}"),
        }

        let known_format = Resolved::Known(Known::new("x".to_owned(), provenance));
        assert!(known_format.is_known());
    }

    #[test]
    fn accept_f40_a_missing_media_or_decoder_fails_with_a_useful_error_and_no_completion() {
        let plan = plan();
        let missing = begin(&plan, &MediaAvailability::MissingFile, SYNTHETIC_PLAYER).unwrap();
        match missing.state() {
            CinematicState::Failed { reason, .. } => {
                assert_eq!(
                    reason,
                    &MediaFailure::MissingMedia {
                        media: synthetic_cinematic_id()
                    }
                );
                assert!(reason.to_string().contains("missing media"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        let no_decoder = begin(&plan, &MediaAvailability::NoDecoder, SYNTHETIC_PLAYER).unwrap();
        assert!(matches!(
            no_decoder.state(),
            CinematicState::Failed {
                reason: MediaFailure::MissingDecoder { .. },
                ..
            }
        ));
        let ok = begin(&plan, &MediaAvailability::Present, SYNTHETIC_PLAYER).unwrap();
        assert_eq!(ok.state(), &CinematicState::Playing { elapsed: 0 });
    }

    #[test]
    fn accept_f40_a_skip_after_aircraft_swap_returns_control_through_the_boundary() {
        let mut player = begin(&plan(), &MediaAvailability::Present, SYNTHETIC_PLAYER).unwrap();
        player.advance(30).unwrap();
        player.set_player_actor(SYNTHETIC_SWAPPED_PLAYER);
        player.request_skip().unwrap();
        player.settle_skip().unwrap();
        assert!(player.semantic_end_reached());
        assert_eq!(player.applied().len(), 3);
    }

    #[test]
    fn accept_f40_a_letterbox_preserves_aspect_and_centres() {
        // 4:3 into 16:9: pillarboxed.
        assert_eq!(
            fit_letterboxed((640, 480), (1920, 1080)).unwrap(),
            Viewport {
                x: 240,
                y: 0,
                width: 1440,
                height: 1080
            }
        );
        // 16:9 into 4:3: letterboxed.
        assert_eq!(
            fit_letterboxed((1280, 720), (800, 600)).unwrap(),
            Viewport {
                x: 0,
                y: 75,
                width: 800,
                height: 450
            }
        );
        // Exact fit.
        let exact = fit_letterboxed((640, 480), (1280, 960)).unwrap();
        assert_eq!(
            (exact.x, exact.y, exact.width, exact.height),
            (0, 0, 1280, 960)
        );
        assert_eq!(fit_letterboxed((0, 480), (10, 10)), Err(EmptyExtent));
    }
}
