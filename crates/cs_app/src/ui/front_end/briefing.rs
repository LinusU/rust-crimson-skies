//! The briefing map screen's model (task #1750
//! `PARITY5-FRONTEND-BRIEFING-MAP`): the original's `Briefing.zrd` script
//! compiled into a timeline, and the scene the window draws.
//!
//! No renderer and no audio device lives here:
//!
//! * [`Timeline::compile`] walks a [`BriefingScript`]'s commands once, holding
//!   a cursor in seconds, and records what each named element does when.
//!   `WaitForMarker n` moves the cursor to the voice-over's `n`th marker
//!   (the `cue ` points of the sound member, [`WaveCues::marker_seconds`]),
//!   `Wait s` advances it by `s`; every other command stamps the element it
//!   names with the cursor's time.
//! * [`Timeline::frame`] answers, for a time in seconds, which elements are
//!   drawn and with which opacity, rotation and position.
//! * [`BriefingPlayback`] holds the clock: it runs, is [restarted](BriefingPlayback::restart)
//!   by Replay Briefing, and is [synchronised](BriefingPlayback::sync_to_voice)
//!   to the voice-over's played position when a device plays it.
//! * [`BriefingScene`] is what is drawn: the map, the objectives panel, the
//!   objectives' text and the three buttons at the dialog record's positions.
//!
//! # Readings the data does not state
//!
//! The script names numbers, not their meaning; these readings are choices,
//! each listed in [`BriefingScene::unknowns`] and in
//! `docs/findings/2026-10-21-parity5-briefing-map.md`:
//!
//! * **`Fade`, `Spin` and `Move` run beside the script**: each starts at the
//!   cursor's time and the script continues at once; only `Wait` and
//!   `WaitForMarker` hold it. The retail member pairs a `Fade` of 0.25 s, a
//!   `Spin` of 0.5 s and a `Move` of 0.5 s on one picture before the next
//!   marker, which reads as one arrival, not as 1.25 s of three steps.
//! * **`Spin` turns from `startrevs` to `revs` revolutions** (clockwise
//!   positive, a missing `startrevs` meaning the rotation the element has),
//!   and the element stays at `revs` afterwards. The retail pictures' paper
//!   rests at +0.05 turn (18°), which is how the owner's capture of the
//!   treasure map hangs.
//! * **`Move` runs linearly** from the path's first point to its second and
//!   stays at the second; before it starts the element is at its `Pict`
//!   position. **`center`** makes the position the picture's centre.
//! * **Marker `n` is the `n`th earliest cue point**, not the cue with id
//!   `n + 1` ([`WaveCues::marker_seconds`] gives the reasoning).
//! * An element is hidden until its `On`, hidden again by `Off`; with no
//!   `Fade` it is fully opaque; elements stack in creation order and
//!   `ToBack` sends one behind every other.
//! * Fonts are named by the record (`ObjList`, `BtnLabelNormal`, …) and
//!   unread; text uses the engine's default font at designed sizes and
//!   colours, and the cursor bitmaps (`daglove`, `dafinger`) are not drawn.
//!
//! Nothing here is `verified_original`.

use std::fmt;

use cs_content::briefing::{
    BackgroundImage, BriefingCommand, BriefingDialog, BriefingScript, DialogButtonKind,
    ObjectivesPanel, Point, SoundCue,
};
use cs_content::ui_layout::Rect;

/// The designed height of an objective line, in logical pixels (the record's
/// `ObjList` font is unread).
pub const DESIGNED_LINE_HEIGHT: f32 = 14.0;

/// The id the script gives the objectives panel (`Fade OBJECTIVESLIST …`).
pub const PANEL_ELEMENT: &str = "OBJECTIVESLIST";

/// What a drawn element is.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementKind {
    /// A bitmap from the UI archive.
    Picture {
        /// The bitmap name as the script spells it.
        bitmap: String,
    },
    /// A line between two points.
    Line {
        /// The end points.
        points: [Point; 2],
        /// The colour.
        color: [u8; 3],
    },
    /// The objectives panel (parchment, title and list).
    Panel,
    /// One objective line of the panel's list.
    Objective {
        /// The zero-based objective index.
        index: u32,
    },
}

/// One timed change of an element's opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FadeAnim {
    from: f64,
    start: f32,
    end: f32,
    duration: f32,
}

/// One timed change of an element's rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SpinAnim {
    from: f64,
    start: Option<f32>,
    end: f32,
    duration: f32,
}

/// One timed move of an element.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MoveAnim {
    from: f64,
    path: [Point; 2],
    duration: f32,
}

/// Everything the script says about one named element.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// The element's name.
    pub id: String,
    /// What it is.
    pub kind: ElementKind,
    /// Where it is created.
    pub at: Point,
    /// Whether `at` is its centre.
    pub centered: bool,
    /// The script time it was created.
    pub created: f64,
    /// Its stacking order: creation order, lowered by `ToBack`.
    pub z: i32,
    shown: Vec<(f64, bool)>,
    fades: Vec<FadeAnim>,
    spins: Vec<SpinAnim>,
    moves: Vec<MoveAnim>,
}

/// One element as drawn at one time.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnElement {
    /// The element's name.
    pub id: String,
    /// What it is.
    pub kind: ElementKind,
    /// Opacity, `0.0..=1.0`.
    pub alpha: f32,
    /// Rotation in revolutions, clockwise positive.
    pub rotation_revs: f32,
    /// The position in the logical space.
    pub position: (f32, f32),
    /// Whether the position is the element's centre.
    pub centered: bool,
    /// Stacking order, ascending.
    pub z: i32,
}

/// Why a script cannot be compiled.
#[derive(Clone, Debug, PartialEq)]
pub enum TimelineError {
    /// The script holds a command the reader does not know.
    UnknownCommand(String),
    /// A command names an element no `Pict`, `Objective` or `Line` created.
    UnknownElement {
        /// The command.
        command: &'static str,
        /// The element.
        id: String,
    },
    /// The script waits for a marker the voice-over does not have.
    MissingMarker {
        /// The marker the script waits for.
        wanted: u32,
        /// How many markers the voice-over has.
        have: usize,
    },
    /// An element name was created twice.
    DuplicateElement(String),
}

impl fmt::Display for TimelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(name) => write!(f, "the script holds the unknown command {name}"),
            Self::UnknownElement { command, id } => {
                write!(f, "{command} names {id}, which nothing created")
            }
            Self::MissingMarker { wanted, have } => write!(
                f,
                "the script waits for marker {wanted} but the voice-over has {have} markers"
            ),
            Self::DuplicateElement(id) => write!(f, "{id} is created twice"),
        }
    }
}

impl std::error::Error for TimelineError {}

/// The compiled script.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    tracks: Vec<Track>,
    end: f64,
    sound_at: Option<f64>,
}

fn progress(now: f64, from: f64, duration: f32) -> f32 {
    if duration <= 0.0 {
        return if now >= from { 1.0 } else { 0.0 };
    }
    (((now - from) / f64::from(duration)).clamp(0.0, 1.0)) as f32
}

impl Timeline {
    /// Compiles a script against the voice-over's marker times (seconds,
    /// sorted).
    ///
    /// # Errors
    ///
    /// [`TimelineError`] for an unknown command, an element nothing created,
    /// a marker the voice-over lacks or an element created twice.
    pub fn compile(script: &BriefingScript, markers: &[f64]) -> Result<Self, TimelineError> {
        let mut tracks = vec![Track {
            id: PANEL_ELEMENT.to_owned(),
            kind: ElementKind::Panel,
            at: Point { x: 0, y: 0 },
            centered: false,
            created: 0.0,
            z: 0,
            shown: Vec::new(),
            fades: Vec::new(),
            spins: Vec::new(),
            moves: Vec::new(),
        }];
        let mut cursor = 0.0_f64;
        let mut end = 0.0_f64;
        let mut sound_at = None;
        let mut next_z = 1;
        let mut back_z = 0;
        for command in &script.commands {
            match command {
                BriefingCommand::PlaySound(_) => sound_at = Some(cursor),
                BriefingCommand::WaitForMarker(number) => {
                    let marker =
                        markers
                            .get(*number as usize)
                            .ok_or(TimelineError::MissingMarker {
                                wanted: *number,
                                have: markers.len(),
                            })?;
                    cursor = cursor.max(*marker);
                }
                BriefingCommand::Wait(seconds) => cursor += f64::from(*seconds),
                BriefingCommand::Pict {
                    id,
                    bitmap,
                    at,
                    center,
                } => {
                    create(
                        &mut tracks,
                        id,
                        ElementKind::Picture {
                            bitmap: bitmap.clone(),
                        },
                        *at,
                        *center,
                        cursor,
                        &mut next_z,
                    )?;
                }
                BriefingCommand::Objective { id, index } => {
                    create(
                        &mut tracks,
                        id,
                        ElementKind::Objective { index: *index },
                        Point { x: 0, y: 0 },
                        false,
                        cursor,
                        &mut next_z,
                    )?;
                }
                BriefingCommand::Line { id, points, color } => {
                    create(
                        &mut tracks,
                        id,
                        ElementKind::Line {
                            points: *points,
                            color: *color,
                        },
                        points[0],
                        false,
                        cursor,
                        &mut next_z,
                    )?;
                }
                BriefingCommand::Fade {
                    id,
                    start,
                    end: to,
                    duration,
                } => {
                    track_mut(&mut tracks, "Fade", id)?.fades.push(FadeAnim {
                        from: cursor,
                        start: *start,
                        end: *to,
                        duration: *duration,
                    });
                    end = end.max(cursor + f64::from(*duration));
                }
                BriefingCommand::Spin {
                    id,
                    start_revs,
                    revs,
                    duration,
                } => {
                    track_mut(&mut tracks, "Spin", id)?.spins.push(SpinAnim {
                        from: cursor,
                        start: *start_revs,
                        end: *revs,
                        duration: *duration,
                    });
                    end = end.max(cursor + f64::from(*duration));
                }
                BriefingCommand::Move { id, path, duration } => {
                    track_mut(&mut tracks, "Move", id)?.moves.push(MoveAnim {
                        from: cursor,
                        path: *path,
                        duration: *duration,
                    });
                    end = end.max(cursor + f64::from(*duration));
                }
                BriefingCommand::On(id) => {
                    track_mut(&mut tracks, "On", id)?.shown.push((cursor, true))
                }
                BriefingCommand::Off(id) => {
                    track_mut(&mut tracks, "Off", id)?
                        .shown
                        .push((cursor, false));
                }
                BriefingCommand::ToBack(id) => {
                    back_z -= 1;
                    track_mut(&mut tracks, "ToBack", id)?.z = back_z;
                }
                BriefingCommand::Unknown { name } => {
                    return Err(TimelineError::UnknownCommand(name.clone()));
                }
            }
            end = end.max(cursor);
        }
        Ok(Self {
            tracks,
            end,
            sound_at,
        })
    }

    /// The script time of its last command or animation end, in seconds.
    #[must_use]
    pub fn end(&self) -> f64 {
        self.end
    }

    /// When the script starts its voice-over, in seconds of script time.
    #[must_use]
    pub fn sound_at(&self) -> Option<f64> {
        self.sound_at
    }

    /// Every element the script names.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// The elements drawn at `time` seconds, in stacking order.
    #[must_use]
    pub fn frame(&self, time: f64) -> Vec<DrawnElement> {
        let mut drawn: Vec<DrawnElement> = self
            .tracks
            .iter()
            .filter(|track| track.created <= time)
            .filter(|track| {
                track
                    .shown
                    .iter()
                    .rev()
                    .find(|(at, _)| *at <= time)
                    .is_some_and(|(_, on)| *on)
            })
            .map(|track| {
                let alpha = track
                    .fades
                    .iter()
                    .rev()
                    .find(|fade| fade.from <= time)
                    .map_or(1.0, |fade| {
                        fade.start
                            + (fade.end - fade.start) * progress(time, fade.from, fade.duration)
                    });
                let mut rotation = 0.0_f32;
                for spin in track.spins.iter().filter(|spin| spin.from <= time) {
                    let start = spin.start.unwrap_or(rotation);
                    rotation =
                        start + (spin.end - start) * progress(time, spin.from, spin.duration);
                }
                let mut position = (track.at.x as f32, track.at.y as f32);
                for step in track.moves.iter().filter(|step| step.from <= time) {
                    let along = progress(time, step.from, step.duration);
                    position = (
                        step.path[0].x as f32 + (step.path[1].x - step.path[0].x) as f32 * along,
                        step.path[0].y as f32 + (step.path[1].y - step.path[0].y) as f32 * along,
                    );
                }
                DrawnElement {
                    id: track.id.clone(),
                    kind: track.kind.clone(),
                    alpha: alpha.clamp(0.0, 1.0),
                    rotation_revs: rotation,
                    position,
                    centered: track.centered,
                    z: track.z,
                }
            })
            .collect();
        drawn.sort_by_key(|element| element.z);
        drawn
    }
}

fn create(
    tracks: &mut Vec<Track>,
    id: &str,
    kind: ElementKind,
    at: Point,
    centered: bool,
    created: f64,
    next_z: &mut i32,
) -> Result<(), TimelineError> {
    if tracks.iter().any(|track| track.id == id) {
        return Err(TimelineError::DuplicateElement(id.to_owned()));
    }
    tracks.push(Track {
        id: id.to_owned(),
        kind,
        at,
        centered,
        created,
        z: *next_z,
        shown: Vec::new(),
        fades: Vec::new(),
        spins: Vec::new(),
        moves: Vec::new(),
    });
    *next_z += 1;
    Ok(())
}

fn track_mut<'a>(
    tracks: &'a mut [Track],
    command: &'static str,
    id: &str,
) -> Result<&'a mut Track, TimelineError> {
    tracks
        .iter_mut()
        .find(|track| track.id == id)
        .ok_or_else(|| TimelineError::UnknownElement {
            command,
            id: id.to_owned(),
        })
}

/// The briefing's clock.
///
/// It runs from the moment the screen opens (or Replay restarts it). When a
/// device plays the voice-over, [`BriefingPlayback::sync_to_voice`] sets the
/// clock to the voice's played position, so the markers fall where the sound
/// is, not where a wall clock guesses.
#[derive(Clone, Debug, PartialEq)]
pub struct BriefingPlayback {
    time: f64,
    restarts: u32,
}

impl Default for BriefingPlayback {
    fn default() -> Self {
        Self::new()
    }
}

impl BriefingPlayback {
    /// A clock at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            time: 0.0,
            restarts: 0,
        }
    }

    /// The clock, in seconds.
    #[must_use]
    pub const fn time(&self) -> f64 {
        self.time
    }

    /// How often Replay Briefing restarted the clock.
    #[must_use]
    pub const fn restarts(&self) -> u32 {
        self.restarts
    }

    /// Advances the clock by `seconds` (never backwards).
    pub fn advance(&mut self, seconds: f64) {
        self.time += seconds.max(0.0);
    }

    /// Sets the clock to the voice-over's played position.
    pub fn sync_to_voice(&mut self, played_seconds: f64) {
        self.time = played_seconds.max(0.0);
    }

    /// Replay Briefing: the clock returns to zero.
    pub fn restart(&mut self) {
        self.time = 0.0;
        self.restarts += 1;
    }
}

/// A value the screen cannot state and how it is drawn instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneUnknown {
    /// What is unknown.
    pub what: String,
    /// How the screen draws it meanwhile.
    pub drawn_as: String,
}

/// One of the three buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct BriefingButton {
    /// Which button.
    pub kind: DialogButtonKind,
    /// The button's bitmap name.
    pub bitmap: String,
    /// The region in logical pixels.
    pub rect: Rect,
    /// The label, resolved from the record's string id.
    pub label: String,
    /// The label's offset from the button's corner.
    pub label_offset: Point,
}

/// Why a scene cannot be built.
#[derive(Clone, Debug, PartialEq)]
pub enum BriefingSceneError {
    /// The script could not be compiled.
    Timeline(TimelineError),
    /// The script has no `PlaySound`, so there is no voice-over to play.
    NoSound,
    /// The script names no background picture.
    NoBackground,
    /// A bitmap's extent could not be read.
    ArtUnread(String),
    /// A button's position is negative.
    NegativePosition(DialogButtonKind),
}

impl fmt::Display for BriefingSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeline(error) => write!(f, "the briefing script cannot play: {error}"),
            Self::NoSound => f.write_str("the briefing script plays no voice-over"),
            Self::NoBackground => f.write_str("the briefing script names no map picture"),
            Self::ArtUnread(name) => write!(f, "the bitmap {name} cannot be read"),
            Self::NegativePosition(kind) => {
                write!(f, "the {} button's position is negative", kind.key())
            }
        }
    }
}

impl std::error::Error for BriefingSceneError {}

/// What the briefing screen draws.
#[derive(Clone, Debug, PartialEq)]
pub struct BriefingScene {
    /// The script's key (`brief_c61`).
    pub script_key: String,
    /// The logical screen: the map picture's extent.
    pub size: (u32, u32),
    /// The map picture(s).
    pub background: Vec<BackgroundImage>,
    /// The objectives panel the dialog record states.
    pub panel: ObjectivesPanel,
    /// The panel's title, resolved from the record's string id.
    pub panel_title: String,
    /// The mission's objective lines, by objective index.
    pub objectives: Vec<String>,
    /// The three buttons, in the record's order.
    pub buttons: Vec<BriefingButton>,
    /// The compiled script.
    pub timeline: Timeline,
    /// The voice-over cue.
    pub sound: SoundCue,
    /// The marker times the timeline was compiled against, in seconds.
    pub markers: Vec<f64>,
    /// What the data does not state and the screen draws a designed stand-in for.
    pub unknowns: Vec<SceneUnknown>,
}

impl BriefingScene {
    /// Builds the scene.
    ///
    /// `extent` maps a bitmap name to its size; `strings` maps a string id
    /// (`MSG_BTN_REPLAY_BRIEFING`) to its text; `objectives` is the mission's
    /// objective lines; `markers` the voice-over's marker times.
    ///
    /// # Errors
    ///
    /// [`BriefingSceneError`] when the script cannot be compiled, plays no
    /// sound, names no map or a bitmap cannot be read.
    pub fn build(
        dialog: &BriefingDialog,
        script: &BriefingScript,
        markers: &[f64],
        extent: &dyn Fn(&str) -> Option<(u32, u32)>,
        strings: &dyn Fn(&str) -> Option<String>,
        objectives: Vec<String>,
    ) -> Result<Self, BriefingSceneError> {
        let timeline = Timeline::compile(script, markers).map_err(BriefingSceneError::Timeline)?;
        let sound = script.sound.clone().ok_or(BriefingSceneError::NoSound)?;
        let first = script
            .background
            .first()
            .ok_or(BriefingSceneError::NoBackground)?;
        let size = extent(&first.bitmap)
            .ok_or_else(|| BriefingSceneError::ArtUnread(first.bitmap.clone()))?;
        let mut unknowns = Vec::new();
        let mut buttons = Vec::new();
        for button in &dialog.buttons {
            let (width, height) = extent(&button.bitmap)
                .ok_or_else(|| BriefingSceneError::ArtUnread(button.bitmap.clone()))?;
            let (Ok(x), Ok(y)) = (u32::try_from(button.at.x), u32::try_from(button.at.y)) else {
                return Err(BriefingSceneError::NegativePosition(button.kind));
            };
            let label = strings(&button.label_text).unwrap_or_else(|| {
                unknowns.push(SceneUnknown {
                    what: format!(
                        "the string {} is not in the string table",
                        button.label_text
                    ),
                    drawn_as: button.label_text.clone(),
                });
                button.label_text.clone()
            });
            buttons.push(BriefingButton {
                kind: button.kind,
                bitmap: button.bitmap.clone(),
                rect: Rect {
                    x,
                    y,
                    width,
                    height,
                },
                label,
                label_offset: button.label_offset,
            });
        }
        let panel_title = strings(&dialog.objectives.title_text).unwrap_or_else(|| {
            unknowns.push(SceneUnknown {
                what: format!(
                    "the string {} is not in the string table",
                    dialog.objectives.title_text
                ),
                drawn_as: dialog.objectives.title_text.clone(),
            });
            dialog.objectives.title_text.clone()
        });
        for (what, drawn_as) in [
            (
                "the fonts the record names (ObjList, ObjListTitle, BtnLabelNormal, BtnLabelRollover, BtnLabelActivate)",
                "the engine's default font at designed sizes and colours",
            ),
            (
                "the cursor bitmaps (daglove, dafinger)",
                "the system pointer",
            ),
            (
                "whether Fade, Spin and Move hold the script or run beside it",
                "run beside it",
            ),
            (
                "what Spin's startrevs and revs are relative to",
                "turn from startrevs to revs and stay",
            ),
            (
                "which cue point marker n is when the cue ids are not in time order",
                "the nth earliest cue point",
            ),
            (
                "the briefing's state after Return to Briefing from the Flight Check",
                "the briefing restarts",
            ),
        ] {
            unknowns.push(SceneUnknown {
                what: what.to_owned(),
                drawn_as: drawn_as.to_owned(),
            });
        }
        Ok(Self {
            script_key: script.key.clone(),
            size,
            background: script.background.clone(),
            panel: dialog.objectives.clone(),
            panel_title,
            objectives,
            buttons,
            timeline,
            sound,
            markers: markers.to_vec(),
            unknowns,
        })
    }

    /// Every bitmap the scene draws, once each, in first-use order.
    #[must_use]
    pub fn bitmaps(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut add = |name: &str| {
            if !names.iter().any(|known| known.eq_ignore_ascii_case(name)) {
                names.push(name.to_owned());
            }
        };
        for image in &self.background {
            add(&image.bitmap);
        }
        add(&self.panel.background_bitmap);
        for track in self.timeline.tracks() {
            if let ElementKind::Picture { bitmap } = &track.kind {
                add(bitmap);
            }
        }
        for button in &self.buttons {
            add(&button.bitmap);
        }
        names
    }

    /// The button of a kind.
    #[must_use]
    pub fn button(&self, kind: DialogButtonKind) -> Option<&BriefingButton> {
        self.buttons.iter().find(|button| button.kind == kind)
    }

    /// The objective lines drawn at `time`: those whose `Objective` element is
    /// shown, in objective-index order, as `(index, text)`.
    #[must_use]
    pub fn objective_lines(&self, drawn: &[DrawnElement]) -> Vec<(u32, String)> {
        let mut lines: Vec<(u32, String)> = drawn
            .iter()
            .filter_map(|element| match element.kind {
                ElementKind::Objective { index } => Some((
                    index,
                    self.objectives
                        .get(index as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("unknown (objective {})", index + 1)),
                )),
                _ => None,
            })
            .collect();
        lines.sort_by_key(|(index, _)| *index);
        lines
    }
}
