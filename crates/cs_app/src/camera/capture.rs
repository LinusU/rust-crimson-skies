//! The deterministic capture request and the override report (F21-C).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-C`, non-negotiable behavior 5: "Screenshot CLI accepts a
//! reproducible world pose, mission id, tick and deterministic settings;
//! report all overrides." Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! This module owns the **camera half** of that request. F21's own runtime
//! (`--screenshot`) is not implemented — `cs_app::cli` still rejects every flag
//! outside `--synthetic --headless` — so this is the record a flag parser would
//! build and a session would consume, not a command line: parsing is `cli`'s
//! job and drawing the image is the renderer's, and both live outside this
//! stage's owner paths. What this stage fixes is the *shape* of the request and
//! the discipline around it, so a future CLI cannot quietly invent a pose, a
//! mission, a tick or a setting the capture did not name.
//!
//! # The inputs, and which of them cannot be defaulted
//!
//! | input | type | why |
//! | --- | --- | --- |
//! | mission id | [`CaptureRequest::mission`], a `mission`-kind [`ContentId`] | a capture of "the game" is not reproducible; the mission is the world, the rules and the content set |
//! | tick | [`CaptureRequest::tick`] | the same camera pose at two ticks is two different frames |
//! | world pose | [`CaptureRequest::pose`], `Option<CameraPose>` | a pinned pose is the only way to reproduce a shot the live camera would have moved away from |
//! | deterministic settings | [`CaptureRequest::settings`], `Option<ComparisonSettings>` | F17 refuses a capture whose settings are not the fixed comparison set, and so does this boundary ([`CaptureError::SettingsNotFixed`]) |
//! | rig and aspect | [`CaptureRequest::rig`], [`CaptureRequest::aspect`] | the same pose through the spyglass at 21:9 is a different picture |
//!
//! # Every override is reported, never dropped
//!
//! [`CaptureRequest::apply`] returns a [`CaptureReport`] listing one
//! [`CaptureOverride`] per thing the engine did differently from simply
//! drawing the session's camera at that tick: the rig it switched to and back
//! from, the pose it pinned, the aspect it framed at, the settings it froze,
//! the smoothing it bypassed, the magnification it folded into the frustum,
//! and the `f64 → f32` narrowing of the pinned projection. A capture that
//! pinned nothing still reports the two things the engine always had to state
//! (the magnification and the narrowing), so "no overrides" and "not reported"
//! can never look the same.
//!
//! # The one projection owner, and the narrowing kept visible
//!
//! A [`PinnedProjection`] is *derived* from the frame's own
//! [`LoweredProjection`] at the capture's aspect — it is never a second,
//! independently authored frustum. The declared policy is `f64` and the record
//! F17 pins is `f32`, so the narrowing is real and lossy; the report keeps both
//! ends of it ([`Narrowing::exact`] and [`Narrowing::pinned`]) instead of hiding
//! it behind a cast, and a value that does not survive the narrowing refuses
//! ([`ProjectionPinError::Unrepresentable`]) rather than pinning an infinity.
//!
//! Folding the magnification into the frustum is a decision, and it is reported
//! as one: see [`CaptureOverride::Magnification`] and [`pin`].

use std::fmt;

use cs_content::cameras::{AspectRatio, Magnification};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

use crate::render::capture::{ComparisonSettings, Projection, ProjectionError};

use super::pose::CameraPose;
use super::projection::LoweredProjection;
use super::rig::ViewRig;

/// Why a capture request was refused.
///
/// Every variant is a refusal *before* anything is applied, so a caller that
/// gets one back has a session that is exactly as it was and can retry the
/// corrected request.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureError {
    /// The request named a content id that is not a mission.
    NotAMission {
        /// What it named.
        id: ContentId,
    },
    /// The request carried comparison settings that are not the fixed set.
    ///
    /// F17 non-negotiable 3 pins exposure, tonemapping and gamma during a
    /// comparison and its `capture` refuses anything else by name. A capture
    /// request carrying an unpinned set would compare frames rendered
    /// differently, so it refuses here rather than passing it on.
    SettingsNotFixed {
        /// The exposure that was refused, in bits, so the report is exact.
        exposure_bits: u32,
    },
    /// The request asked for a rig the session's mode set does not declare.
    UndeclaredRig {
        /// The rig the request asked for.
        rig: ViewRig,
        /// The mode kind that rig needs.
        kind: &'static str,
    },
    /// A running scripted camera covers the requested tick, and the request
    /// pins a view.
    ///
    /// One camera has one authority. A scripted camera that names the pose it
    /// draws and a capture that names the view it draws through are both claims
    /// on the same frame, so the session refuses the capture and the producer
    /// releases the script (or retires the capture) first. A request that pins
    /// no view coexists with a script, because it overrides only what the
    /// request names.
    ScriptCoversTick {
        /// The camera that is driving.
        camera: ContentId,
        /// The tick the capture asked for.
        tick: Tick,
    },
    /// A capture is already installed.
    ///
    /// A pending capture is a promise about one frame. Replacing it silently
    /// would drop a request the producer still believes in, so the session
    /// refuses and the producer clears it.
    AlreadyPending {
        /// The tick the new request asked for.
        requested: Tick,
        /// The tick already pending.
        pending: Tick,
    },
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAMission { id } => write!(
                f,
                "a capture names the mission it is of, and {id} is a {} id",
                id.kind()
            ),
            Self::SettingsNotFixed { exposure_bits } => write!(
                f,
                "a capture must carry the fixed comparison settings, and this set has \
                 exposure {} which is not the fixed exposure",
                f32::from_bits(*exposure_bits)
            ),
            Self::UndeclaredRig { rig, kind } => write!(
                f,
                "the capture asked for the {rig} view and this mode set declares no {kind} mode"
            ),
            Self::ScriptCoversTick { camera, tick } => write!(
                f,
                "{camera} is driving the camera at tick {}, so a capture cannot pin a view \
                 there; release the script first",
                tick.0
            ),
            Self::AlreadyPending { requested, pending } => write!(
                f,
                "a capture for tick {} is already pending, so the one for tick {} was not \
                 installed",
                pending.0, requested.0
            ),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Why a lowered projection could not be pinned into a capture record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProjectionPinError {
    /// The value is finite as `f64` but has no `f32` near it: a clipping plane
    /// beyond `f32::MAX`, or a near plane below the smallest positive `f32`.
    /// The capture would pin an infinity or a zero, so it refuses.
    Unrepresentable {
        /// Which field it was: `"vertical_fov"`, `"aspect"`, `"near_m"` or
        /// `"far_m"`.
        field: &'static str,
        /// The `f64` value that has no `f32` near it.
        exact: f64,
    },
    /// The `f32` record rejected the narrowed values.
    Refused(ProjectionError),
}

impl fmt::Display for ProjectionPinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrepresentable { field, exact } => write!(
                f,
                "the capture projection's {field} ({exact}) has no f32 near it, so pinning it \
                 would not reproduce the declared frustum"
            ),
            Self::Refused(error) => write!(f, "the pinned projection was refused: {error}"),
        }
    }
}

impl std::error::Error for ProjectionPinError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unrepresentable { .. } => None,
            Self::Refused(error) => Some(error),
        }
    }
}

/// One field's `f64 → f32` narrowing, with both ends kept.
///
/// The declared policy is `f64` because the framing math is `f64`; the record a
/// comparison capture pins is `f32` because that is F17's record. The cast is
/// therefore unavoidable, and the honest thing is to make the loss inspectable:
/// `exact` is what the policy declares, `pinned` is what the capture carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Narrowing {
    /// Which field this is.
    pub field: &'static str,
    /// The value the lowered policy declares, in `f64`.
    pub exact: f64,
    /// The value the capture record pins, in `f32`.
    pub pinned: f32,
}

impl Narrowing {
    /// How far the pinned value is from the exact one.
    ///
    /// Zero when the value survived the cast exactly; a non-zero result is the
    /// rounding a comparison capture has baked in, and it is part of the report
    /// rather than an accident of the cast.
    #[must_use]
    pub fn drift(self) -> f64 {
        f64::from(self.pinned) - self.exact
    }
}

/// How a capture applied a mode's magnification to the pinned frustum.
///
/// The mechanism is **engine design**, not an original measurement: the
/// original's spyglass may have narrowed its field of view, scaled the
/// projection or drawn a separate pass, and nothing measured says which
/// (F21-D). The capture therefore *states* which mechanism it used instead of
/// presenting one as the original's behaviour, and it reports the factor, so a
/// consumer can tell a 4x capture from a declared-but-unmagnified one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagnificationPin {
    /// The factor the mode declares.
    pub factor: f64,
    /// Whether the factor narrowed the vertical field of view, which is what
    /// [`pin`] does.
    pub folded_into_fov: bool,
}

/// The projection a capture pins, derived from the frame's own policy.
///
/// It is a *derived* record: the only inputs are the frame's
/// [`LoweredProjection`], the aspect the capture frames at and the
/// magnification the frame's mode declares. Nothing here is authored
/// independently, so a capture cannot pin a frustum the mode never declared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinnedProjection {
    /// The record F17's `capture` consumes.
    pinned: Projection,
    /// The vertical field of view the policy declares at the capture aspect
    /// after the declared framing rule, before the magnification is folded in.
    declared_vertical_fov: f64,
    /// The same field of view as the capture carries it.
    vertical_fov: Narrowing,
    /// The aspect, narrowed.
    aspect: Narrowing,
    /// The near plane, narrowed.
    near_m: Narrowing,
    /// The far plane, narrowed.
    far_m: Narrowing,
    /// The factor the magnification folded in, and how.
    magnification: MagnificationPin,
}

impl PinnedProjection {
    /// The `f32` record a capture carries.
    #[must_use]
    pub const fn pinned(&self) -> &Projection {
        &self.pinned
    }

    /// The vertical field of view the declared policy resolves to at the
    /// capture aspect, in `f64`, before any magnification.
    #[must_use]
    pub const fn declared_vertical_fov(&self) -> f64 {
        self.declared_vertical_fov
    }

    /// The field of view the capture actually pins, with both ends of the
    /// narrowing.
    ///
    /// This is the declared policy's own value at the capture aspect **after**
    /// the magnification has been folded in, in `f64`, next to the `f32` the
    /// record carries. [`Self::declared_vertical_fov`] is the value before the
    /// fold, and a consumer comparing a magnified capture against the declared
    /// policy needs this one.
    #[must_use]
    pub const fn pinned_vertical_fov(&self) -> Narrowing {
        self.vertical_fov
    }

    /// Every narrowing this pin performed, in a stable order: field of view,
    /// aspect, near plane, far plane.
    #[must_use]
    pub const fn narrowings(&self) -> [Narrowing; 4] {
        [self.vertical_fov, self.aspect, self.near_m, self.far_m]
    }

    /// The largest absolute drift any of the four narrowings introduced.
    ///
    /// A capture tool can compare this against zero to say whether its pinned
    /// frustum is bit-identical to the declared one, or against a tolerance to
    /// say how far it is not.
    #[must_use]
    pub fn max_drift(&self) -> f64 {
        self.narrowings()
            .iter()
            .map(|narrowing| narrowing.drift().abs())
            .fold(0.0_f64, f64::max)
    }

    /// How the magnification was applied to the pinned frustum.
    #[must_use]
    pub const fn magnification(&self) -> MagnificationPin {
        self.magnification
    }
}

/// Derives the projection a capture pins from `projection` at `aspect`.
///
/// The declared framing rule is applied **first** — the policy resolves to its
/// vertical field of view at `aspect` in `f64` — and the magnification is then
/// folded in by dividing the half-angle *tangent* by the factor. Dividing the
/// tangent rather than the angle is what keeps the composed frustum exact for
/// any factor: `tan(v/2)/f` is the tangent of the narrowed half-angle, so the
/// pinned horizontal field of view follows from the pinned vertical one at the
/// same aspect without an approximation. A factor of one narrows nothing.
///
/// # Errors
///
/// [`ProjectionPinError::Unrepresentable`] for a value with no `f32` near it
/// (an infinity, or a value that rounds to zero), and
/// [`ProjectionPinError::Refused`] when the `f32` record rejects the narrowed
/// values — a near plane that rounded up past the far plane, a field of view
/// that rounded to zero.
pub fn pin(
    projection: LoweredProjection,
    aspect: AspectRatio,
    magnification: Magnification,
) -> Result<PinnedProjection, ProjectionPinError> {
    let factor = magnification.value();
    let declared = projection.vertical_fov_at(aspect).0;
    let narrowed = if factor == 1.0 {
        declared
    } else {
        2.0 * ((declared * 0.5).tan() / factor).atan()
    };
    let fields = [
        ("vertical_fov", narrowed),
        ("aspect", aspect.value()),
        ("near_m", projection.near_m().0),
        ("far_m", projection.far_m().0),
    ];
    let mut narrowed_values = [0.0_f32; 4];
    for (index, (field, value)) in fields.iter().enumerate() {
        let as_f32 = *value as f32;
        // A finite `f64` beyond `f32::MAX` becomes an infinity, and a value
        // below the smallest positive `f32` becomes zero. Either one pins a
        // frustum that is not the declared one, so both refuse by name.
        if !as_f32.is_finite() || (*value != 0.0 && as_f32 == 0.0) {
            return Err(ProjectionPinError::Unrepresentable {
                field,
                exact: *value,
            });
        }
        narrowed_values[index] = as_f32;
    }
    let record = Projection::new(
        narrowed_values[0],
        narrowed_values[1],
        narrowed_values[2],
        narrowed_values[3],
    )
    .map_err(ProjectionPinError::Refused)?;
    Ok(PinnedProjection {
        pinned: record,
        declared_vertical_fov: declared,
        vertical_fov: Narrowing {
            field: "vertical_fov",
            exact: narrowed,
            pinned: narrowed_values[0],
        },
        aspect: Narrowing {
            field: "aspect",
            exact: fields[1].1,
            pinned: narrowed_values[1],
        },
        near_m: Narrowing {
            field: "near_m",
            exact: fields[2].1,
            pinned: narrowed_values[2],
        },
        far_m: Narrowing {
            field: "far_m",
            exact: fields[3].1,
            pinned: narrowed_values[3],
        },
        magnification: MagnificationPin {
            factor,
            folded_into_fov: factor != 1.0,
        },
    })
}

/// The camera a capture request is applied to.
///
/// The session builds this from the frame it produced, so the report is built
/// out of what the frame actually used rather than out of what the request
/// asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureTarget {
    /// The rig the frame drew through, or `None` when a scripted camera drew
    /// it — a scripted camera is not one of the player's views, and reporting a
    /// view for it would name something the frame never used.
    pub rig: Option<ViewRig>,
    /// The pose the frame drew from, after any pinned pose was applied.
    pub pose: CameraPose,
    /// The frame's own lowered projection.
    pub projection: LoweredProjection,
    /// The aspect the frame was framed at, after any override.
    pub aspect: AspectRatio,
    /// The magnification the frame's mode declares.
    pub magnification: Magnification,
}

/// What one override changed, as the report states it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CaptureOverride {
    /// The rig the capture drew through, and the rig the session went back to
    /// after the capture frame.
    ///
    /// `requested` *is* the rig the capture used: the request is checked
    /// against the mode set when it is installed, so the switch cannot fail on
    /// an undeclared mode later. The pair is kept because a report that named
    /// only the view it drew through could not answer "what is the camera back
    /// on now?", and a teardown is as much an override as the frame is.
    Rig {
        /// What the request asked for, and what the capture used.
        requested: ViewRig,
        /// The rig in force again once the capture frame was over.
        restored: ViewRig,
    },
    /// The world pose the capture drew from.
    ///
    /// A pinned pose is held exactly — the smoother is bypassed — so `requested`
    /// and `effective` are the same pose, and saying so is the report's job
    /// rather than the reader's assumption.
    WorldPose {
        /// The pose the request pinned.
        requested: CameraPose,
        /// The pose the capture drew from.
        effective: CameraPose,
    },
    /// The viewport aspect the capture was framed at.
    Aspect {
        /// What the request asked for.
        requested: AspectRatio,
        /// What the capture used.
        effective: AspectRatio,
    },
    /// The comparison settings the capture froze.
    Settings {
        /// The fixed set the capture pinned.
        settings: ComparisonSettings,
    },
    /// The camera smoothing the capture bypassed.
    ///
    /// A pinned pose is held exactly, so the frame-rate independent follow has
    /// nothing to do; a capture that kept smoothing would depend on the wall
    /// time between frames and would not reproduce.
    Smoothing {
        /// Whether the smoother was bypassed for this capture.
        bypassed: bool,
    },
    /// How the mode's magnification reached the pinned frustum.
    Magnification {
        /// The factor the mode declares.
        factor: f64,
        /// Whether it was folded into the vertical field of view.
        folded_into_fov: bool,
    },
    /// The `f64 → f32` narrowing the pinned projection carries.
    Projection {
        /// The largest absolute drift any field took.
        max_drift: f64,
    },
}

impl CaptureOverride {
    /// The stable label of what was overridden, for a report a human reads.
    #[must_use]
    pub const fn field(&self) -> &'static str {
        match self {
            Self::Rig { .. } => "rig",
            Self::WorldPose { .. } => "world_pose",
            Self::Aspect { .. } => "aspect",
            Self::Settings { .. } => "settings",
            Self::Smoothing { .. } => "smoothing",
            Self::Magnification { .. } => "magnification",
            Self::Projection { .. } => "projection",
        }
    }
}

impl fmt::Display for CaptureOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rig {
                requested,
                restored,
            } => write!(f, "rig {requested} (restored to {restored} afterwards)"),
            Self::WorldPose { effective, .. } => write!(f, "world pose at {effective:?}"),
            Self::Aspect {
                requested,
                effective,
            } => write!(f, "aspect {requested} (framed at {effective})"),
            Self::Settings { settings } => write!(
                f,
                "settings exposure {} gamma {} tonemap {} msaa {} shadows {}",
                settings.exposure(),
                settings.gamma(),
                settings.tonemap().code(),
                settings.msaa_samples(),
                settings.shadows()
            ),
            Self::Smoothing { bypassed } => write!(f, "smoothing bypassed: {bypassed}"),
            Self::Magnification {
                factor,
                folded_into_fov,
            } => write!(
                f,
                "magnification {factor}x (folded into the vertical field of view: \
                 {folded_into_fov})"
            ),
            Self::Projection { max_drift } => {
                write!(f, "projection narrowed to f32 (largest drift {max_drift})")
            }
        }
    }
}

/// A reproducible capture request: the mission, the tick and every override
/// the caller pinned.
///
/// A request is a **value**. It is built by whoever parses flags (not
/// implemented in this stage) and consumed by
/// [`CameraSession`](super::session::CameraSession), which applies it to
/// exactly the frame at [`Self::tick`] and then retires it.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRequest {
    mission: ContentId,
    tick: Tick,
    rig: Option<ViewRig>,
    pose: Option<CameraPose>,
    aspect: Option<AspectRatio>,
    settings: Option<ComparisonSettings>,
}

impl CaptureRequest {
    /// A capture of `mission` at `tick`, with nothing else pinned.
    ///
    /// # Errors
    ///
    /// [`CaptureError::NotAMission`] when `mission` is not a `mission`-kind id.
    pub fn new(mission: ContentId, tick: Tick) -> Result<Self, CaptureError> {
        Self::build(mission, tick, None, None, None, None)
    }

    /// A capture that also pins the rig it draws through.
    ///
    /// # Errors
    ///
    /// As [`Self::build`].
    pub fn with_rig(
        mission: ContentId,
        tick: Tick,
        rig: ViewRig,
        settings: Option<ComparisonSettings>,
    ) -> Result<Self, CaptureError> {
        Self::build(mission, tick, Some(rig), None, None, settings)
    }

    /// Builds a request, validating every field at once.
    ///
    /// # Errors
    ///
    /// [`CaptureError::NotAMission`] for an id of another kind,
    /// [`CaptureError::SettingsNotFixed`] for an unpinned comparison set, and
    /// [`CaptureError::UndeclaredRig`] for a rig the session's own mode set
    /// does not declare — which is a property of the session, so the session
    /// re-checks it when the request is installed.
    pub fn build(
        mission: ContentId,
        tick: Tick,
        rig: Option<ViewRig>,
        pose: Option<CameraPose>,
        aspect: Option<AspectRatio>,
        settings: Option<ComparisonSettings>,
    ) -> Result<Self, CaptureError> {
        if mission.kind() != ContentKind::Mission {
            return Err(CaptureError::NotAMission { id: mission });
        }
        if let Some(settings) = settings
            && !settings.is_fixed()
        {
            return Err(CaptureError::SettingsNotFixed {
                exposure_bits: settings.exposure().to_bits(),
            });
        }
        Ok(Self {
            mission,
            tick,
            rig,
            pose,
            aspect,
            settings,
        })
    }

    /// The mission the capture is of.
    #[must_use]
    pub const fn mission(&self) -> &ContentId {
        &self.mission
    }

    /// The tick the capture is reproducible at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The rig the capture asks for, when it asks for one.
    #[must_use]
    pub const fn rig(&self) -> Option<ViewRig> {
        self.rig
    }

    /// The world pose the capture pins, when it pins one.
    #[must_use]
    pub const fn pose(&self) -> Option<CameraPose> {
        self.pose
    }

    /// The aspect the capture frames at, when it pins one.
    #[must_use]
    pub const fn aspect(&self) -> Option<AspectRatio> {
        self.aspect
    }

    /// The fixed comparison settings the capture pins, when it pins any.
    #[must_use]
    pub const fn settings(&self) -> Option<ComparisonSettings> {
        self.settings
    }

    /// Reports every override this request applied to `target`.
    ///
    /// The report is built from what the frame used, not from what was asked,
    /// so a request the session could not fully honour says so. The order is
    /// stable and is the order a report prints: rig, world pose, aspect,
    /// settings, smoothing, magnification, projection. The last two are always
    /// present: the engine always had to state how it applied the mode's
    /// magnification and how far the `f32` record drifted from the declared
    /// `f64` policy.
    ///
    /// `restored` is the rig the session goes back to once the capture frame is
    /// over; the report names it so a teardown is as visible as the override.
    ///
    /// # Errors
    ///
    /// [`ProjectionPinError`] when the frame's own projection cannot be pinned
    /// into an `f32` record at the capture's aspect. The session treats that as
    /// a retryable refusal and keeps the request installed.
    pub fn apply(
        &self,
        target: &CaptureTarget,
        restored: ViewRig,
    ) -> Result<CaptureReport, ProjectionPinError> {
        let aspect = self.aspect.unwrap_or(target.aspect);
        let pinned = pin(target.projection, aspect, target.magnification)?;
        let mut overrides = Vec::new();
        // First everything the *request* pinned, in the order a reader wants
        // it: which view, which pose, which viewport, which settings.
        if let Some(requested) = self.rig {
            overrides.push(CaptureOverride::Rig {
                requested,
                restored,
            });
        }
        if let Some(requested) = self.pose {
            overrides.push(CaptureOverride::WorldPose {
                requested,
                effective: target.pose,
            });
        }
        if let Some(requested) = self.aspect {
            overrides.push(CaptureOverride::Aspect {
                requested,
                effective: aspect,
            });
        }
        if let Some(settings) = self.settings {
            overrides.push(CaptureOverride::Settings { settings });
        }
        // Then the three the engine always had to state: whether the live
        // camera's smoothing stood, how the mode's magnification reached the
        // frustum, and how far the `f32` record drifted from the declared
        // `f64` policy. A capture that pinned nothing still reports all three,
        // so "no overrides" and "not reported" can never look the same.
        overrides.push(CaptureOverride::Smoothing {
            bypassed: self.pose.is_some(),
        });
        overrides.push(CaptureOverride::Magnification {
            factor: pinned.magnification().factor,
            folded_into_fov: pinned.magnification().folded_into_fov,
        });
        overrides.push(CaptureOverride::Projection {
            max_drift: pinned.max_drift(),
        });
        Ok(CaptureReport {
            request: self.clone(),
            mission: self.mission.clone(),
            tick: self.tick,
            rig: target.rig,
            pose: target.pose,
            aspect,
            pinned,
            overrides,
        })
    }
}

/// What one applied capture did, and everything it changed.
///
/// A report is what non-negotiable behavior 5 asks for: the overrides are
/// listed, not summarised, and the pinned projection travels with it so a
/// consumer never has to re-derive the frustum the capture used.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureReport {
    /// The request that produced this report.
    request: CaptureRequest,
    /// The mission, copied out so a report is self-describing.
    mission: ContentId,
    /// The tick the capture was taken at.
    tick: Tick,
    /// The rig the capture drew through, when a player view drew it.
    rig: Option<ViewRig>,
    /// The pose the capture drew from.
    pose: CameraPose,
    /// The aspect the capture was framed at, after the override.
    aspect: AspectRatio,
    /// The projection the capture pinned.
    pinned: PinnedProjection,
    /// Every override, in report order.
    overrides: Vec<CaptureOverride>,
}

impl CaptureReport {
    /// The request this report answers.
    #[must_use]
    pub const fn request(&self) -> &CaptureRequest {
        &self.request
    }

    /// The mission the capture is of.
    #[must_use]
    pub const fn mission(&self) -> &ContentId {
        &self.mission
    }

    /// The tick the capture was taken at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The rig the capture drew through, when a player view drew it.
    #[must_use]
    pub const fn rig(&self) -> Option<ViewRig> {
        self.rig
    }

    /// The pose the capture drew from.
    #[must_use]
    pub const fn pose(&self) -> CameraPose {
        self.pose
    }

    /// The aspect the capture was framed at.
    #[must_use]
    pub const fn aspect(&self) -> AspectRatio {
        self.aspect
    }

    /// The projection the capture pinned.
    #[must_use]
    pub const fn pinned(&self) -> &PinnedProjection {
        &self.pinned
    }

    /// Every override, in report order.
    #[must_use]
    pub fn overrides(&self) -> &[CaptureOverride] {
        &self.overrides
    }

    /// The override recorded for `field`, when there is one.
    #[must_use]
    pub fn override_of(&self, field: &str) -> Option<&CaptureOverride> {
        self.overrides.iter().find(|entry| entry.field() == field)
    }
}
