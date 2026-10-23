//! The session phase of the M01 launch cinematic (task #1663
//! `PARITY-M01-LC-PHASE`, slice ii of #1642).
//!
//! The phase runs after Fly Mission and before player control. While it runs
//! the player has no control (the original law is not stepped, the weapon keys
//! publish nothing and the body is held where the launch left it), the cockpit
//! HUD is held back and the letterbox bars are on. When its last tick has run,
//! control and the HUD are handed to the player with the body exactly where it
//! stood: the composed start pose (`docs/findings/2026-10-19-m01-composed-start-pose.md`).
//!
//! **Measured.** The length is `generic_intro`'s, replayed from the data by
//! [`crate::cinematics::intro_timeline::measure_intro_timeline`]: 40.15 s, or
//! 30.25 s when the player's scene root is `player_balmoral` or
//! `player_warhawk` (`docs/findings/2026-10-18-parity-m01-lc-cinematic-timeline.md`).
//! The original steps it in seconds with a variable frame delta; this
//! composition counts integer fixed ticks at
//! [`crate::physics::BASELINE_FIXED_HZ`], rounding the measured length **up**
//! to a whole tick ([`ticks_for`]), so the hand-over is never early.
//!
//! **Not measured** (and not decided here): what the camera shows (#1665), the
//! bars' slide-in and exit (#1664), what the original does with the world's
//! actors during the cinematic, and the velocity the original hands over (the
//! body keeps the velocity the composition declared, `velocity-integration`
//! in `docs/findings/2026-10-19-m01-launch-body-pose.md`). Nothing here is
//! `verified_original`.

use std::fmt;
use std::path::Path;

use avian3d::prelude::LinearVelocity;
use bevy::app::{App, FixedFirst, FixedLast, Update};
use bevy::math::Vec3;
use bevy::prelude::{
    Component, DistanceFog, Entity, FogFalloff, Local, Query, ResMut, Resource, With, World,
};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index};
use cs_types::install::RelativePath;

use super::compose::MissionPlayerBody;
use super::letterbox::LaunchLetterbox;
use crate::cinematics::intro_timeline::{RecordView, measure_intro_timeline};
use crate::cinematics::launch_end_pose::LAUNCH_INTRO_ANIMATION;

/// The camera carrier that holds the launch animations, relative to the
/// installation.
pub const LAUNCH_CAMERA_CARRIER: &str = "ZBD/C3/cam_anim.zbd";

/// Why the cinematic's length could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchCinematicError(pub String);

impl fmt::Display for LaunchCinematicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the launch cinematic's length is not measured: {}",
            self.0
        )
    }
}

impl std::error::Error for LaunchCinematicError {}

/// Marks a UI node whose visibility its own system owns because the launch
/// cinematic decides it (the cockpit HUD's safe area, the letterbox bars). The
/// smoke's probes hide and restore every other node; a restore must leave
/// these alone, or it shows the bars after hand-over and the HUD during the
/// phase (#1704).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CinematicOwnedVisibility;

/// The measured length of the launch cinematic for one player airframe.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchCinematicLength {
    /// The player's scene root the replay used (`player_<airframe>`).
    pub scene_root: String,
    /// The length in seconds of game time.
    pub seconds: f64,
    /// The scene spans the replay produced, in start order: the launch
    /// voices' `SOUND` cues anchor at the starts of the scenes whose
    /// sequences carry them (#1863).
    pub scenes: Vec<crate::cinematics::intro_timeline::SceneSpan>,
}

impl LaunchCinematicLength {
    /// The length in fixed ticks at `hz`.
    #[must_use]
    pub fn ticks(&self, hz: u32) -> u64 {
        ticks_for(self.seconds, hz)
    }
}

/// `seconds` of game time as whole fixed ticks at `hz`, rounded up.
#[must_use]
pub fn ticks_for(seconds: f64, hz: u32) -> u64 {
    // The product is a small non-negative number (tens of seconds times a
    // rate), so the conversion cannot truncate meaningfully.
    (seconds * f64::from(hz) - 1e-9).ceil().max(0.0) as u64
}

/// Replays `generic_intro` in the camera carrier `bytes` for a player whose
/// scene root is `scene_root` and returns its length.
///
/// # Errors
///
/// [`LaunchCinematicError`] when the carrier does not parse, the record is
/// absent or ambiguous, or the replay refuses.
pub fn measure_launch_cinematic(
    bytes: &[u8],
    scene_root: &str,
) -> Result<LaunchCinematicLength, LaunchCinematicError> {
    let refuse = |what: String| LaunchCinematicError(format!("{LAUNCH_CAMERA_CARRIER}: {what}"));
    let path = RelativePath::new(&LAUNCH_CAMERA_CARRIER.to_ascii_lowercase())
        .map_err(|error| refuse(format!("path: {error}")))?;
    let key = path.logical_key();
    let mut context = ParseContext::with_defaults(&key);
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    let decision = dispatch(ZbdProbe::new(&key, &path, &bytes[..needed]))
        .map_err(|error| refuse(format!("dispatch refused ({})", error.code())))?;
    let index = read_animation_index(&mut context, decision, bytes)
        .map_err(|error| refuse(format!("index ({})", error.code())))?;
    let payload = index
        .payload()
        .map_err(|error| refuse(format!("payload ({})", error.code())))?;
    let records = payload
        .records()
        .map_err(|error| refuse(format!("records ({})", error.code())))?;
    let mut found = records.by_anim_name(LAUNCH_INTRO_ANIMATION.as_bytes());
    let (Some(record), None) = (found.next(), found.next()) else {
        return Err(refuse(format!(
            "`{LAUNCH_INTRO_ANIMATION}` must name exactly one record"
        )));
    };
    let view = RecordView::from_record(record).map_err(|error| refuse(error.to_string()))?;
    let timeline =
        measure_intro_timeline(&view, &[scene_root]).map_err(|error| refuse(error.to_string()))?;
    Ok(LaunchCinematicLength {
        scene_root: scene_root.to_owned(),
        seconds: timeline.length,
        scenes: timeline.scenes,
    })
}

/// Reads [`LAUNCH_CAMERA_CARRIER`] from the installation and measures it.
///
/// # Errors
///
/// [`LaunchCinematicError`] when the file cannot be read or measured.
pub fn read_launch_cinematic(
    install_root: &Path,
    scene_root: &str,
) -> Result<LaunchCinematicLength, LaunchCinematicError> {
    let bytes = std::fs::read(install_root.join(LAUNCH_CAMERA_CARRIER)).map_err(|error| {
        LaunchCinematicError(format!("cannot read {LAUNCH_CAMERA_CARRIER}: {error}"))
    })?;
    measure_launch_cinematic(&bytes, scene_root)
}

/// The session's cinematic phase: game state, counted in fixed ticks.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct LaunchCinematic {
    total_ticks: u64,
    elapsed_ticks: u64,
    held_velocity: Option<Vec3>,
}

impl LaunchCinematic {
    /// A phase of `total_ticks` fixed ticks.
    ///
    /// # Errors
    ///
    /// [`LaunchCinematicError`] for a zero length: a phase that never plays is
    /// a missing measurement, not a short cinematic.
    pub fn new(total_ticks: u64) -> Result<Self, LaunchCinematicError> {
        if total_ticks == 0 {
            return Err(LaunchCinematicError("the length is zero ticks".to_owned()));
        }
        Ok(Self {
            total_ticks,
            elapsed_ticks: 0,
            held_velocity: None,
        })
    }

    /// Whether the cinematic still holds the player's control and the HUD.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.elapsed_ticks < self.total_ticks
    }

    /// Fixed ticks that have run inside the phase.
    #[must_use]
    pub fn elapsed_ticks(&self) -> u64 {
        self.elapsed_ticks
    }

    /// The phase's length in fixed ticks.
    #[must_use]
    pub fn total_ticks(&self) -> u64 {
        self.total_ticks
    }

    /// Completes one fixed tick of the phase. Returns `true` on the tick that
    /// ends it.
    pub fn complete_tick(&mut self) -> bool {
        if !self.is_playing() {
            return false;
        }
        self.elapsed_ticks += 1;
        !self.is_playing()
    }
}

/// Whether the world's launch cinematic is still playing. A world without the
/// phase is never held.
#[must_use]
pub fn launch_cinematic_playing(world: &World) -> bool {
    world
        .get_resource::<LaunchCinematic>()
        .is_some_and(LaunchCinematic::is_playing)
}

/// Run condition of the systems that must not run while the player has no
/// control.
#[must_use]
pub fn player_has_control(phase: Option<bevy::prelude::Res<LaunchCinematic>>) -> bool {
    !phase.is_some_and(|phase| phase.is_playing())
}

/// Installs the phase: it holds the player's body for `total_ticks` fixed
/// ticks, switches the letterbox bars on for as long, then hands over.
///
/// # Errors
///
/// [`LaunchCinematicError`] for a zero length.
pub fn install_launch_cinematic(
    app: &mut App,
    total_ticks: u64,
) -> Result<(), LaunchCinematicError> {
    app.insert_resource(LaunchCinematic::new(total_ticks)?);
    app.add_systems(FixedFirst, hold_body)
        .add_systems(FixedLast, advance_phase)
        .add_systems(Update, (sync_letterbox, sync_fog));
    Ok(())
}

/// Keeps the body still while the phase plays: the original's flight update
/// does not integrate during the intro
/// (`docs/findings/2026-10-18-m01-launch-release.md`), so the velocity the body
/// carries is put aside and handed back at the end.
fn hold_body(
    mut phase: ResMut<LaunchCinematic>,
    mut bodies: Query<&mut LinearVelocity, With<MissionPlayerBody>>,
) {
    if !phase.is_playing() {
        return;
    }
    for mut velocity in &mut bodies {
        if phase.held_velocity.is_none() {
            phase.held_velocity = Some(velocity.0);
        }
        velocity.0 = Vec3::ZERO;
    }
}

pub(crate) fn advance_phase(
    mut phase: ResMut<LaunchCinematic>,
    mut bodies: Query<&mut LinearVelocity, With<MissionPlayerBody>>,
) {
    if phase.complete_tick()
        && let Some(held) = phase.held_velocity.take()
    {
        for mut velocity in &mut bodies {
            velocity.0 = held;
        }
    }
}

fn sync_letterbox(
    phase: bevy::prelude::Res<LaunchCinematic>,
    bars: Option<ResMut<LaunchLetterbox>>,
) {
    if let Some(mut bars) = bars
        && bars.is_active() != phase.is_playing()
    {
        bars.set_active(phase.is_playing());
    }
}

/// The falloff a [`DistanceFog`] is parked on while the cinematic plays: far
/// beyond anything drawn, the same stand-in the smoke's probes use for "no
/// fog" (`apply_probe` in `smoke.rs`). It has no `PartialEq`, so the parked
/// state is tested on the fields.
const CINEMATIC_FALLOFF: FogFalloff = FogFalloff::Linear {
    start: 1.0e9,
    end: 2.0e9,
};

fn is_parked(falloff: &FogFalloff) -> bool {
    matches!(falloff, FogFalloff::Linear { start, end }
        if *start == 1.0e9 && *end == 2.0e9)
}

/// The cinematic-mode half of the original's camera hand-over, parked for as
/// long as the phase plays. Entering cinematic mode writes fog **mode 0** —
/// "no fog test at all", per the mode read at `0x551e3b` — to the world
/// object's state block through setter `0x4da800` (call `0x45067c` inside
/// entry `0x450550`) and multiplies the camera far clip by 4.0
/// (`cinematic_enter_far_clip_factor`, `0x4506a6`); the exit `0x450e30`
/// writes **mode 1** (`0x450ee1`) and multiplies the clip by 0.25
/// (`cinematic_leave_far_clip_factor`, `0x450f0b`, the exact inverse) and
/// clears the cinematic flag `0x654120`. Fog range, colour and altitude are
/// never touched — only whether the renderer tests them
/// (`docs/findings/2026-10-23-m01-fog-update-static.md`, section 2).
///
/// That the launch takes this path is an inference, not a trace: the flag is
/// stored only by the pair (`0x450562` sets it, `0x450f9b` clears it), and
/// the per-frame camera dispatch steps the cinematic camera `0x450ad0`
/// exactly while it is set (`0x489cec`), so the intro's driven camera means
/// entry ran and the return of control means the exit ran — mode 1 plus a
/// quarter of the cinematic far clip — before the first controllable frame.
/// The clip half has no visible effect here: nothing drawn sits beyond the
/// composed camera's 40 km far plane, so scaling it fourfold both ways
/// changes no pixel; the fog step is the portable half. The five-second
/// clearing the owner's frames show after hand-over is not a fog change —
/// nothing in the image interpolates the parameters — it is the aircraft
/// flying through the constant zone fog (`FOG_RANGES 1000 4500`).
fn sync_fog(
    phase: Option<bevy::prelude::Res<LaunchCinematic>>,
    mut fogs: Query<(Entity, &mut DistanceFog)>,
    mut parked: Local<Vec<(Entity, FogFalloff)>>,
) {
    if phase.is_some_and(|phase| phase.is_playing()) {
        for (entity, mut fog) in &mut fogs {
            if is_parked(&fog.falloff) {
                continue;
            }
            if parked.iter().all(|&(kept, _)| kept != entity) {
                parked.push((entity, fog.falloff.clone()));
            }
            fog.falloff = CINEMATIC_FALLOFF.clone();
        }
    } else {
        for (entity, falloff) in parked.drain(..) {
            if let Ok((_, mut fog)) = fogs.get_mut(entity) {
                fog.falloff = falloff;
            }
        }
    }
}
