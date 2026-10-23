//! The Esc pause screen: the original's chart, flag pins, objectives note,
//! memento photo and four buttons (PARITY3-PAUSE-MAP-SCREEN.02-A, #1873).
//!
//! `ZBD/zrdr.zbd#escape.zrd` describes the screen (`docs/findings/
//! 2026-10-21-pause-map-screen-data.md`): the mission's `loading_*` entry
//! names the chart, its clip rectangle, the photo and an `ESC_SCRIPT` that
//! lays one flag-pin picture per objective at authored screen pixels; the
//! shared `LOADINGDIALOG` names the note and the buttons. Every bitmap is a
//! texture of `ZBD/rimage.zbd`.
//!
//! * [`read_pause_map`] reads one mission's entry into a [`PauseMapScene`] and
//!   the bitmaps it draws. A mission whose entry is missing or carries no
//!   chart is refused, never replaced by a stand-in.
//! * [`PauseMapPlugin`] draws the scene while the player's Esc pause holds
//!   the session (`playtest::meta_controls` is the one Esc path; the fixed
//!   clock freezes with the session's pause, `playtest::sync_pause`). The
//!   **Resume** button resumes the session. **Restart** closes the screen,
//!   resumes the clock and raises the composed host's
//!   [`MissionHostRestartRequest`](super::host::MissionHostRestartRequest)
//!   (PARITY3-PAUSE-MAP-SCREEN.02-C, #1875), so the host's one restart path
//!   rebuilds the authored initial state under a fresh session generation.
//!   **Quit** abandons the mission: it writes `AppExit::Success` (the
//!   composed `cs --mission` window has no front-end machine to return to,
//!   so leaving ends the run, as F10 does) and reports the press.
//!   **Preferences** opens the mission window's `Preferences` section
//!   ([`super::preferences_overlay`], PARITY3-PAUSE-MAP-SCREEN.02-E.02,
//!   #1923): the press writes [`OpenPreferencesOverlay`] and while the
//!   overlay is up the screen's parts are despawned — its buttons dead —
//!   until the overlay's Return to Game closes it and the map is drawn
//!   again. The session stays paused and the fixed clock frozen either
//!   way; a composition without the overlay only reports the press.
//!
//! The live parts (PARITY3-PAUSE-MAP-SCREEN.02-B, #1874) come from
//! [`PauseMapLive`], which [`PauseMapPlugin`] fills from the mission host's
//! objective session and the player body: each pin shows the record's bitmap
//! until its objective is revealed and then its numbered flag
//! ([`pin_flag_bitmap`]), the note lists the revealed objectives with a check
//! mark on completed ones, and the player icon sits at the world position
//! through the candidate [`world_to_chart_pixel`].
//!
//! What is a stand-in, recorded in
//! `docs/findings/2026-10-22-pause-map-presenter.md` and
//! `docs/findings/2026-10-22-pause-map-live-objectives.md`: the faces `Andy
//! Bold` and `Trebuchet MS Bold` are not in the installation, so the note
//! title, list and the button labels use Bevy's default face (colours and
//! offsets are the record's); the "?" to number rule, the check mark's
//! placement and the `WORLD` mapping are readings, not decodes. Nothing here
//! is `verified_original`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use avian3d::prelude::{Position, Rotation};
use bevy::app::AppExit;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, BackgroundColor, ButtonInput, ChildOf, Color, Commands, Component, Entity, FlexDirection,
    GlobalZIndex, Handle, Has, Image, ImageNode, Local, Message, MessageReader, MessageWriter,
    MouseButton, Node, Overflow, Plugin, PositionType, Quat, Query, Res, ResMut, Resource, Rot2,
    Text, TextColor, TextFont, UiTransform, Update, Val, Vec3, Window, With, Without, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{FontSize, Justify, LineBreak, TextLayout};
use bevy::window::CursorOptions;
use cs_content::briefing::{BRIEFING_ARCHIVE, BRIEFING_MEMBER, BriefingDocument, Point};
use cs_content::campaign_bindings::SourceContext;
use cs_content::config::MessageCatalog;
use cs_content::stunts::{ZrdValue, decode_zrd, zrd_field};

use super::compose::MissionPlayerBody;
use super::host::{MissionHost, MissionHostRestartRequest};
use super::launch_sound::reader_member;
use super::lifecycles::ObjectiveLifecycles;
use super::preferences_overlay::{OpenPreferencesOverlay, PreferencesOverlay};
use crate::input::platform::PlatformInput;
use crate::input::session::PauseReason;
use crate::ui::front_end::{Artwork, FrontEndScreens};
use crate::ui::hud::{MissionMessages, ObjectiveHud};

/// The reader archive that holds the Esc scripts.
pub const ESCAPE_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member that describes the Esc screen.
pub const ESCAPE_MEMBER: &str = "escape.zrd";
/// The screen's logical size: the designed 800x600 space the record's pixels
/// are authored in.
pub const SCREEN_SIZE: (u32, u32) = (800, 600);

/// The label colours of the three states, from `fonts.zrd`'s
/// `BtnEscapeNormal`, `BtnEscapeRollover` and `BtnEscapeActivate` records.
const LABEL_NORMAL: [u8; 3] = [10, 10, 10];
const LABEL_ROLLOVER: [u8; 3] = [105, 100, 85];
const LABEL_ACTIVATE: [u8; 3] = [180, 175, 151];
/// The note title's colour, from `fonts.zrd`'s `ObjListTitle`.
const TITLE_COLOR: [u8; 3] = [16, 16, 16];
/// The designed sizes of the stand-in face (the record's heights are the
/// unavailable faces' own).
const TITLE_FONT_PX: f32 = 16.0;
const LIST_FONT_PX: f32 = 14.0;
/// The note list's text colour, from `fonts.zrd`'s `ObjList`.
const LIST_COLOR: [u8; 3] = [16, 16, 16];
/// The highest number a numbered flag carries (`pin1`..`pin8`).
const MAX_FLAG_NUMBER: u32 = 8;
/// The check mark's width: it is drawn left of the list's column.
const CHECK_MARK_WIDTH: f32 = 25.0;
const LABEL_FONT_PX: f32 = 14.0;

/// The layers, back to front.
const Z_BACKDROP: i32 = 3000;
const Z_BACKGROUND: i32 = 3001;
const Z_CHART: i32 = 3002;
const Z_SHADOW: i32 = 3003;
const Z_MEMENTO: i32 = 3004;
const Z_NOTE: i32 = 3005;
const Z_PINS: i32 = 3006;
const Z_BUTTON: i32 = 3008;
const Z_CURSOR: i32 = 3020;

// ------------------------------------------------------------ the scene --

/// Why the Esc screen could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseMapError(pub String);

impl fmt::Display for PauseMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the pause map cannot be read: {}", self.0)
    }
}

impl std::error::Error for PauseMapError {}

fn fail<T>(reason: impl Into<String>) -> Result<T, PauseMapError> {
    Err(PauseMapError(reason.into()))
}

/// The four buttons of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PauseButtonKind {
    /// `RESUME_MISSION_BTN`.
    Resume,
    /// `RESTART_MISSION_BTN`.
    Restart,
    /// `CONFIGURE_BTN`.
    Preferences,
    /// `MAINMENU_BTN`.
    Quit,
}

impl PauseButtonKind {
    /// Every button in the record's order.
    pub const ALL: [Self; 4] = [Self::Resume, Self::Restart, Self::Preferences, Self::Quit];

    /// The key the record names the button by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Resume => "RESUME_MISSION_BTN",
            Self::Restart => "RESTART_MISSION_BTN",
            Self::Preferences => "CONFIGURE_BTN",
            Self::Quit => "MAINMENU_BTN",
        }
    }
}

/// The chart: a bitmap drawn at a position and clipped to a rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct PauseChart {
    /// The bitmap (`HA-m1MAP`).
    pub bitmap: String,
    /// Where the bitmap's corner is drawn.
    pub at: Point,
    /// The clip rectangle's top-left corner.
    pub clip_top_left: Point,
    /// The clip rectangle's bottom-right corner.
    pub clip_bottom_right: Point,
    /// The `WORLD` rectangle `[left, top, right, bottom]`, kept as authored.
    /// Its mapping onto the chart is unread (the player icon needs it).
    pub world: [f32; 4],
}

/// One picture of the `ESC_SCRIPT`.
#[derive(Clone, Debug, PartialEq)]
pub struct PausePicture {
    /// The picture's id (`OBJPIN1`, `MOMENTOSHAD`).
    pub id: String,
    /// The bitmap.
    pub bitmap: String,
    /// The authored position.
    pub at: Point,
    /// Whether the position is the picture's centre.
    pub centered: bool,
}

/// The objectives note: parchment and title (the list is a later task).
#[derive(Clone, Debug, PartialEq)]
pub struct PauseNote {
    /// The parchment bitmap.
    pub bitmap: String,
    /// Where it is drawn.
    pub at: Point,
    /// The title's string id (`MSG_BRF_DLG_OBJECTIVES`).
    pub title_text: String,
    /// The resolved title.
    pub title: String,
    /// Where the title is drawn.
    pub title_at: Point,
    /// The check mark bitmap (`obj_check1`) of a completed objective.
    pub check_mark: String,
    /// Where the list's column starts (`LIST POSITION`).
    pub list_at: Point,
    /// The column's word-wrap width and height (`LIST WORDWRAP`).
    pub list_wrap: (i32, i32),
    /// The space between two entries (`LIST SPACING`).
    pub list_spacing: i32,
}

/// One button.
#[derive(Clone, Debug, PartialEq)]
pub struct PauseButton {
    /// Which button.
    pub kind: PauseButtonKind,
    /// Its corner.
    pub at: Point,
    /// The bitmaps at rest, under the pointer and pressed.
    pub bitmaps: [String; 3],
    /// The label's string id (`MSG_BTN_RESUME`).
    pub label_text: String,
    /// The resolved label.
    pub label: String,
    /// The label's offset from the button's corner.
    pub label_offset: Point,
}

/// Everything the Esc screen draws, as the record states it.
#[derive(Clone, Debug, PartialEq)]
pub struct PauseMapScene {
    /// The script entry (`loading_c61`).
    pub key: String,
    /// The `BACKGROUND_IMAGES` (`loadframe`).
    pub background: Vec<(String, Point)>,
    /// The chart.
    pub chart: PauseChart,
    /// The memento photo's bitmap and position.
    pub memento: (String, Point),
    /// The script's pictures in order: the photo's shadow and one pin per
    /// objective.
    pub pictures: Vec<PausePicture>,
    /// The objectives note.
    pub note: PauseNote,
    /// The four buttons.
    pub buttons: Vec<PauseButton>,
    /// The cursor bitmap and its rollover bitmap.
    pub cursor: (String, String),
    /// The player aircraft icon's bitmap (`OWNSHIP`, `singledev`).
    pub own_ship: String,
    /// The script's `Objective OBJn index i` rows: the row id and the
    /// objective index the note's `n`-th line shows.
    pub objectives: Vec<(String, usize)>,
    /// Script commands this reading does not interpret, by name.
    pub unread_commands: Vec<String>,
}

impl PauseMapScene {
    /// Every bitmap name the scene draws.
    #[must_use]
    pub fn bitmaps(&self) -> Vec<String> {
        let mut names: Vec<String> = self.background.iter().map(|(n, _)| n.clone()).collect();
        names.push(self.chart.bitmap.clone());
        names.push(self.memento.0.clone());
        names.extend(self.pictures.iter().map(|p| p.bitmap.clone()));
        names.push(self.note.bitmap.clone());
        for button in &self.buttons {
            names.extend(button.bitmaps.iter().cloned());
        }
        names.push(self.cursor.0.clone());
        names.push(self.cursor.1.clone());
        names.push(self.own_ship.clone());
        names.push(self.note.check_mark.clone());
        names.extend((1..=MAX_FLAG_NUMBER).filter_map(numbered_flag));
        let mut seen = Vec::<String>::new();
        names.retain(|name| {
            let lower = name.to_ascii_lowercase();
            if seen.contains(&lower) {
                false
            } else {
                seen.push(lower);
                true
            }
        });
        names
    }

    /// The row index the pin `pin` (`OBJPIN2`) stands for: the `index` of the
    /// script's `Objective OBJ2 index i`.
    #[must_use]
    pub fn pin_objective(&self, pin: &PausePicture) -> Option<usize> {
        let number = pin.id.strip_prefix("OBJPIN")?;
        let wanted = format!("OBJ{number}");
        self.objectives
            .iter()
            .find(|(id, _)| *id == wanted)
            .map(|(_, index)| *index)
    }

    /// The pins: the pictures named `OBJPIN<n>`, in script order.
    pub fn pins(&self) -> impl Iterator<Item = &PausePicture> {
        self.pictures
            .iter()
            .filter(|picture| picture.id.starts_with("OBJPIN"))
    }
}

/// What [`read_pause_map`] read.
#[derive(Clone, Debug, Resource)]
pub struct PauseMapSource {
    /// What the screen draws.
    pub scene: PauseMapScene,
    /// The decoded bitmaps by lower-cased name.
    pub artwork: BTreeMap<String, Artwork>,
}

// ---------------------------------------------------------- the parsing --

fn field<'a>(node: &'a ZrdValue, key: &str, within: &str) -> Result<&'a ZrdValue, PauseMapError> {
    match zrd_field(node, key) {
        Some(value) => Ok(value),
        None => fail(format!("{within} has no {key}")),
    }
}

fn text(node: &ZrdValue, what: &str) -> Result<String, PauseMapError> {
    match node
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdValue::as_text)
    {
        Some(value) => Ok(value.to_owned()),
        None => fail(format!("{what} is not a text")),
    }
}

fn ints(node: &ZrdValue, what: &str) -> Result<Vec<i32>, PauseMapError> {
    let Some(children) = node.as_list() else {
        return fail(format!("{what} is not a list"));
    };
    children
        .iter()
        .map(|value| match value {
            ZrdValue::Int(int) => Ok(i32::from_ne_bytes(int.to_ne_bytes())),
            _ => Err(PauseMapError(format!("{what} holds a non-integer"))),
        })
        .collect()
}

fn floats(node: &ZrdValue, what: &str) -> Result<Vec<f32>, PauseMapError> {
    let Some(children) = node.as_list() else {
        return fail(format!("{what} is not a list"));
    };
    children
        .iter()
        .map(|value| match value {
            ZrdValue::Float(float) => Ok(*float),
            ZrdValue::Int(int) => Ok(i32::from_ne_bytes(int.to_ne_bytes()) as f32),
            _ => Err(PauseMapError(format!("{what} holds a non-number"))),
        })
        .collect()
}

fn point(node: &ZrdValue, what: &str) -> Result<Point, PauseMapError> {
    match ints(node, what)?.as_slice() {
        [x, y] => Ok(Point { x: *x, y: *y }),
        _ => fail(format!("{what} is not a pair")),
    }
}

/// The value after the text `key` among a command's flat arguments
/// (`["OBJPIN1" "bitmap" ["pin6"] "at" [506 149]]`).
fn argument<'a>(command: &'a ZrdValue, key: &str) -> Option<&'a ZrdValue> {
    let children = command.as_list()?;
    let at = children
        .iter()
        .position(|child| child.as_text() == Some(key))?;
    children.get(at + 1)
}

/// A button's three states from its record.
fn read_button(
    kind: PauseButtonKind,
    record: &ZrdValue,
    strings: &dyn Fn(&str) -> Option<String>,
) -> Result<PauseButton, PauseMapError> {
    let what = kind.key();
    let label = field(record, "LABEL", what)?;
    let label_text = label
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdValue::as_text)
        .map(str::to_owned);
    let Some(label_text) = label_text else {
        return fail(format!("{what} LABEL names no string"));
    };
    let Some(offset) = argument(label, "offset") else {
        return fail(format!("{what} LABEL has no offset"));
    };
    let Some(resolved) = strings(&label_text) else {
        return fail(format!(
            "the string {label_text} of {what} does not resolve"
        ));
    };
    let rest = text(field(record, "BITMAP", what)?, what)?;
    let rollover = text(
        field(field(record, "ROLLOVER", what)?, "BITMAP", what)?,
        what,
    )?;
    let activate = text(
        field(field(record, "ACTIVATE", what)?, "BITMAP", what)?,
        what,
    )?;
    Ok(PauseButton {
        kind,
        at: point(field(record, "POSITION", what)?, what)?,
        bitmaps: [rest, rollover, activate],
        label_text,
        label: resolved,
        label_offset: point(offset, what)?,
    })
}

impl PauseMapScene {
    /// Reads the entry `key` of a decoded `escape.zrd` document (the reader's
    /// one wrapping list already removed).
    ///
    /// # Errors
    ///
    /// [`PauseMapError`] naming the first record the entry or the shared
    /// dialog lacks, or a string that does not resolve.
    pub fn from_document(
        document: &ZrdValue,
        key: &str,
        strings: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, PauseMapError> {
        let Some(entry) = zrd_field(document, key) else {
            return fail(format!("{ESCAPE_MEMBER} has no entry {key}"));
        };
        let primitives = field(entry, "PRIMITIVES", key)?;
        let Some(map) = zrd_field(primitives, "MAP") else {
            return fail(format!("{key} carries no chart (no MAP primitive)"));
        };
        let clip = field(map, "CLIP", "MAP")?;
        let world = field(map, "WORLD", "MAP")?;
        let world_corner = |name: &str| -> Result<[f32; 2], PauseMapError> {
            match floats(field(world, name, "WORLD")?, "WORLD")?.as_slice() {
                [x, z] => Ok([*x, *z]),
                _ => fail("WORLD corner is not a pair"),
            }
        };
        let (top_left, bottom_right) = (world_corner("topleft")?, world_corner("bottomright")?);
        let chart = PauseChart {
            bitmap: text(field(map, "BITMAP", "MAP")?, "MAP BITMAP")?,
            at: point(field(map, "POSITION", "MAP")?, "MAP POSITION")?,
            clip_top_left: point(field(clip, "topleft", "CLIP")?, "CLIP")?,
            clip_bottom_right: point(field(clip, "bottomright", "CLIP")?, "CLIP")?,
            world: [top_left[0], top_left[1], bottom_right[0], bottom_right[1]],
        };
        let memento_record = field(primitives, "MEMENTO", key)?;
        let memento = (
            text(field(memento_record, "BITMAP", "MEMENTO")?, "MEMENTO")?,
            point(field(memento_record, "POSITION", "MEMENTO")?, "MEMENTO")?,
        );

        let mut background = Vec::new();
        for row in field(entry, "BACKGROUND_IMAGES", key)?
            .as_list()
            .into_iter()
            .flatten()
        {
            let Some([name, x, y]) = row.as_list().and_then(|c| c.get(..3)) else {
                return fail("a BACKGROUND_IMAGES row is not (name x y)");
            };
            let at = point(&ZrdValue::List(vec![x.clone(), y.clone()]), "background")?;
            let Some(name) = name.as_text() else {
                return fail("a BACKGROUND_IMAGES name is not a text");
            };
            background.push((name.to_owned(), at));
        }

        let mut pictures = Vec::new();
        let mut objectives = Vec::new();
        let mut unread_commands = Vec::new();
        let script = field(entry, "ESC_SCRIPT", key)?;
        for pair in script
            .as_list()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .chunks(2)
        {
            let [command, args] = pair else {
                return fail("ESC_SCRIPT ends in a command without arguments");
            };
            match command.as_text() {
                Some("Pict") => {
                    let Some(id) = args
                        .as_list()
                        .and_then(|children| children.first())
                        .and_then(ZrdValue::as_text)
                    else {
                        return fail("a Pict names no id");
                    };
                    let Some(bitmap) = argument(args, "bitmap") else {
                        return fail(format!("Pict {id} has no bitmap"));
                    };
                    let Some(at) = argument(args, "at") else {
                        return fail(format!("Pict {id} has no position"));
                    };
                    let centered = argument(args, "center")
                        .and_then(|value| value.as_list())
                        .and_then(|children| children.first())
                        .and_then(ZrdValue::as_text)
                        == Some("true");
                    pictures.push(PausePicture {
                        id: id.to_owned(),
                        bitmap: text(bitmap, id)?,
                        at: point(at, id)?,
                        centered,
                    });
                }
                Some("Objective") => {
                    let id = args
                        .as_list()
                        .and_then(|children| children.first())
                        .and_then(ZrdValue::as_text);
                    let (Some(id), Some(index)) = (id, argument(args, "index")) else {
                        return fail("an Objective names no id or index");
                    };
                    let index = match ints(index, id)?.as_slice() {
                        [index] if *index >= 0 => *index as usize,
                        _ => return fail(format!("Objective {id} has no index")),
                    };
                    objectives.push((id.to_owned(), index));
                }
                // The visibility and stacking commands are read as: everything
                // is on, the note is behind the pictures.
                Some("ToBack" | "On") => {}
                Some(other) => unread_commands.push(other.to_owned()),
                None => unread_commands.push("<not a text>".to_owned()),
            }
        }

        let dialog = field(document, "LOADINGDIALOG", ESCAPE_MEMBER)?;
        let list = field(
            field(dialog, "PRIMITIVES", "LOADINGDIALOG")?,
            "OBJECTIVESLIST",
            "PRIMITIVES",
        )?;
        let background_record = field(list, "BACKGROUND", "OBJECTIVESLIST")?;
        let title_record = field(list, "TITLE", "OBJECTIVESLIST")?;
        let list_record = field(list, "LIST", "OBJECTIVESLIST")?;
        let title_text = text(field(title_record, "TEXT", "TITLE")?, "TITLE")?;
        let Some(title) = strings(&title_text) else {
            return fail(format!("the string {title_text} does not resolve"));
        };
        let note = PauseNote {
            bitmap: text(
                field(background_record, "BITMAP", "BACKGROUND")?,
                "BACKGROUND",
            )?,
            at: point(
                field(background_record, "POSITION", "BACKGROUND")?,
                "BACKGROUND",
            )?,
            title_text,
            title,
            title_at: point(field(title_record, "POSITION", "TITLE")?, "TITLE")?,
            check_mark: text(
                field(
                    field(list, "CHECKMARK", "OBJECTIVESLIST")?,
                    "BITMAP",
                    "CHECKMARK",
                )?,
                "CHECKMARK",
            )?,
            list_at: point(field(list_record, "POSITION", "LIST")?, "LIST")?,
            list_wrap: match ints(field(list_record, "WORDWRAP", "LIST")?, "WORDWRAP")?.as_slice() {
                [width, height] => (*width, *height),
                _ => return fail("LIST WORDWRAP is not a pair"),
            },
            list_spacing: match ints(field(list_record, "SPACING", "LIST")?, "SPACING")?.as_slice()
            {
                [spacing] => *spacing,
                _ => return fail("LIST SPACING is not one number"),
            },
        };
        let own_ship = text(
            field(
                field(
                    field(dialog, "PRIMITIVES", "LOADINGDIALOG")?,
                    "OWNSHIP",
                    "PRIMITIVES",
                )?,
                "BITMAP",
                "OWNSHIP",
            )?,
            "OWNSHIP",
        )?;
        let buttons_record = field(dialog, "BUTTONS", "LOADINGDIALOG")?;
        let buttons = PauseButtonKind::ALL
            .iter()
            .map(|kind| {
                read_button(
                    *kind,
                    field(buttons_record, kind.key(), "BUTTONS")?,
                    strings,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cursor = field(
            field(document, "default", ESCAPE_MEMBER)?,
            "CURSOR",
            "default",
        )?;
        let cursor = (
            text(field(cursor, "BITMAP", "CURSOR")?, "CURSOR")?,
            text(
                field(field(cursor, "ROLLOVER", "CURSOR")?, "BITMAP", "ROLLOVER")?,
                "ROLLOVER",
            )?,
        );
        Ok(Self {
            key: key.to_owned(),
            background,
            chart,
            memento,
            pictures,
            note,
            buttons,
            cursor,
            own_ship,
            objectives,
            unread_commands,
        })
    }
}

/// The `escape.zrd` entry of the mission placed at `mission_dir`
/// (`zbd/c3/m01`): the briefing script's key with `brief_` read as
/// `loading_` (`brief_c61` / `loading_c61`, both chapter 1 mission 1).
fn escape_key(install_root: &Path, mission_dir: &str) -> Result<String, PauseMapError> {
    let context = SourceContext::read(install_root)
        .map_err(|error| PauseMapError(format!("the campaign layout: {error}")))?;
    let wanted = format!("{}/", mission_dir.to_ascii_lowercase());
    let Some(position) = context.campaign().iter().find(|entry| {
        entry
            .program_asset
            .to_ascii_lowercase()
            .starts_with(&wanted)
    }) else {
        return fail(format!("{mission_dir} is not in the campaign layout"));
    };
    let sound = cs_content::briefing::sound_name_for(position.chapter, position.mission_number);
    let bytes =
        reader_member(install_root, BRIEFING_ARCHIVE, BRIEFING_MEMBER).map_err(PauseMapError)?;
    let document = BriefingDocument::decode(&bytes)
        .map_err(|error| PauseMapError(format!("{BRIEFING_MEMBER}: {error}")))?;
    let Some(script) = document.script_for_sound(&sound) else {
        return fail(format!("{BRIEFING_MEMBER} has no script playing {sound}"));
    };
    match script.key.strip_prefix("brief_") {
        Some(rest) => Ok(format!("loading_{rest}")),
        None => fail(format!("the briefing key {} is not brief_*", script.key)),
    }
}

/// Reads the Esc screen of one mission.
///
/// `mission_dir` is the mission archive's logical key (`zbd/c3/m01`);
/// `strings` resolves the record's string ids (`MSG_BTN_RESUME`).
///
/// # Errors
///
/// [`PauseMapError`] from the first stage that fails.
pub fn read_pause_map(
    install_root: &Path,
    mission_dir: &str,
    screens: &FrontEndScreens,
    strings: &dyn Fn(&str) -> Option<String>,
) -> Result<PauseMapSource, PauseMapError> {
    let key = escape_key(install_root, mission_dir)?;
    let bytes =
        reader_member(install_root, ESCAPE_ARCHIVE, ESCAPE_MEMBER).map_err(PauseMapError)?;
    let tree =
        decode_zrd(&bytes).map_err(|error| PauseMapError(format!("{ESCAPE_MEMBER}: {error}")))?;
    // The reader wraps the document in one more list.
    let Some(document) = tree.as_list().and_then(|children| children.first()) else {
        return fail(format!("{ESCAPE_MEMBER} holds no document"));
    };
    let scene = PauseMapScene::from_document(document, &key, strings)?;
    let mut artwork = BTreeMap::new();
    for name in scene.bitmaps() {
        let art = screens
            .ui_artwork(&name)
            .map_err(|error| PauseMapError(format!("the bitmap {name}: {error}")))?;
        artwork.insert(name.to_ascii_lowercase(), art);
    }
    Ok(PauseMapSource { scene, artwork })
}

// ----------------------------------------------------------- live state --

/// One objective of the note: the identity rows the objective session declares
/// with a message (M01: `PRIMARY 1`..`PRIMARY 4`), in (role, ordinal) order.
/// The script's `Objective OBJn index i` and pin `OBJPINn` name row `i`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseObjectiveRow {
    /// The `IDENTITY` ordinal: the number its flag carries.
    pub ordinal: u32,
    /// The resolved objective text (the message key when it does not resolve).
    pub text: String,
    /// The session has woken it (or it is the one the HUD shows next).
    pub revealed: bool,
    /// The session marks it completed.
    pub completed: bool,
}

/// Where the player is, in the world's own axes, and which way the nose
/// points on the chart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerFix {
    /// World `x`.
    pub x: f32,
    /// World `z`.
    pub z: f32,
    /// Clockwise from the chart's top, radians: `0` is the nose along
    /// `-z` (`cs_sim::flight::BODY_FORWARD`).
    pub heading: f32,
}

/// What the screen shows of the running mission. The presenter reads only this
/// resource; [`read_pause_map_live`] fills it from the mission host and the
/// player body every frame.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct PauseMapLive {
    /// The objective rows.
    pub rows: Vec<PauseObjectiveRow>,
    /// The player, once a body exists.
    pub player: Option<PlayerFix>,
}

impl PauseObjectiveRow {
    /// The note's rows of the objective session `hud` projects: every
    /// identity with a role and a message element, in (role, ordinal) order.
    /// A row is *revealed* when the session has woken it, or when it is the
    /// objective the HUD shows while every one still sleeps
    /// ([`ObjectiveHud::displayed`]): the original's rule is native and
    /// unread, this is the reading the findings record
    /// (`2026-10-22-pause-map-live-objectives.md`).
    #[must_use]
    pub fn rows_of(hud: &ObjectiveHud, messages: Option<&MessageCatalog>) -> Vec<Self> {
        let shown = hud.displayed().map(|row| row.objective);
        let mut rows: Vec<_> = hud
            .rows()
            .iter()
            .filter(|row| row.role_code != 0 && row.message.is_some())
            .collect();
        rows.sort_by_key(|row| (row.role_code, row.ordinal, row.objective));
        rows.into_iter()
            .filter_map(|row| {
                Some(Self {
                    ordinal: row.ordinal,
                    text: ObjectiveHud::row_text(row, messages)?,
                    revealed: row.revealed || shown == Some(row.objective),
                    completed: row.completed,
                })
            })
            .collect()
    }
}

/// The numbered flag texture of objective number `number`: `pin1`..`pin8`,
/// with `pin6_1` for 6 because `pin6` is the "?" flag. `None` beyond 8.
#[must_use]
pub fn numbered_flag(number: u32) -> Option<String> {
    match number {
        6 => Some("pin6_1".to_owned()),
        1..=MAX_FLAG_NUMBER => Some(format!("pin{number}")),
        _ => None,
    }
}

/// The "?" -> number rule of one pin: **the single named reading**. The
/// script names one bitmap per pin (`authored`: the "?" flag for three of
/// M01's four, the "4" flag for the fourth). While the pin's objective is
/// not revealed the pin keeps that authored bitmap; once the session reveals
/// it, the pin shows the numbered flag of its `IDENTITY` ordinal. Whether the
/// original switches on a woken objective, on the HUD's choice or on
/// something else is native and not in the data; provenance `static`,
/// unverified.
#[must_use]
pub fn pin_flag_bitmap(authored: &str, row: Option<&PauseObjectiveRow>) -> String {
    match row {
        Some(row) if row.revealed => {
            numbered_flag(row.ordinal).unwrap_or_else(|| authored.to_owned())
        }
        _ => authored.to_owned(),
    }
}

/// The candidate chart mapping: the `WORLD` rectangle is the chart's clip
/// rectangle, world `x` runs left to right and world `z` runs **down** (the
/// record's second `WORLD` component is `-z`, descending top to bottom).
/// Candidate only: no engine decode and no owner capture fits it
/// (`2026-10-22-pause-map-live-objectives.md`). `None` for a degenerate
/// rectangle.
#[must_use]
pub fn world_to_chart_pixel(chart: &PauseChart, x: f32, z: f32) -> Option<(f32, f32)> {
    let [left, top, right, bottom] = chart.world;
    let (width, height) = (right - left, bottom - top);
    if width == 0.0 || height == 0.0 {
        return None;
    }
    let u = (x - left) / width;
    let v = (-z - top) / height;
    let clip_width = (chart.clip_bottom_right.x - chart.clip_top_left.x) as f32;
    let clip_height = (chart.clip_bottom_right.y - chart.clip_top_left.y) as f32;
    Some((
        chart.clip_top_left.x as f32 + u * clip_width,
        chart.clip_top_left.y as f32 + v * clip_height,
    ))
}

/// The player's chart fix from the body's pose.
#[must_use]
pub fn player_fix(position: Vec3, rotation: Quat) -> PlayerFix {
    let [fx, _, fz] = cs_sim::flight::BODY_FORWARD;
    let nose = rotation * Vec3::new(fx as f32, 0.0, fz as f32);
    PlayerFix {
        x: position.x,
        z: position.z,
        heading: nose.x.atan2(-nose.z),
    }
}

/// Fills [`PauseMapLive`]: the rows from the mission host's engine objective
/// model and the fix from the player body. A source that is absent leaves its
/// half untouched (no host, or a host on the declared model, has no rows).
fn read_pause_map_live(
    host: Option<Res<MissionHost>>,
    messages: Option<Res<MissionMessages>>,
    bodies: Query<(&Position, &Rotation), With<MissionPlayerBody>>,
    mut live: ResMut<PauseMapLive>,
) {
    let mut next = live.clone();
    if let Some(host) = host
        && let ObjectiveLifecycles::Engine(engine) = host.lifecycles()
    {
        let hud = ObjectiveHud::from_engine(engine);
        next.rows = PauseObjectiveRow::rows_of(
            &hud,
            messages.as_deref().and_then(MissionMessages::catalog),
        );
    }
    if let Ok((position, rotation)) = bodies.single() {
        next.player = Some(player_fix(position.0, rotation.0));
    }
    if *live != next {
        *live = next;
    }
}

// ------------------------------------------------------- the presenter --

/// A button of the screen was pressed. Resume has already resumed the
/// session when this is written, Quit has asked the app to exit, Restart
/// is acted on by [`restart_on_press`] and Preferences has already written
/// [`OpenPreferencesOverlay`].
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseMapPress(pub PauseButtonKind);

/// Whether the screen's parts are spawned.
#[derive(Resource, Default)]
pub struct PauseMapShown(pub bool);

/// Everything the screen spawns (public so a test can count it). Marked as a
/// help/pause panel so the smoke's probes treat it like the keymap overlay.
#[derive(Component)]
pub struct PauseMapPart;

/// A part's logical region in the 800x600 space, relative to its parent.
#[derive(Component, Clone, Copy)]
struct Placed {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// A part the live state decides (pins, the note's list, the player icon):
/// rebuilt whenever [`PauseMapLive`] changes.
#[derive(Component)]
struct LivePart;

/// The clip container of the chart, the icon's parent.
#[derive(Component)]
struct ChartClip;

/// The space between a column's entries, in logical pixels.
#[derive(Component, Clone, Copy)]
struct RowGap(f32);

/// The decoded bitmaps' handles and sizes by lower-cased name, while the
/// screen is up.
#[derive(Resource, Default)]
struct PauseMapImages(BTreeMap<String, (Handle<Image>, f32, f32)>);

/// A text part's designed size in logical pixels.
#[derive(Component, Clone, Copy)]
struct FontPx(f32);

/// A button's picture: the three states' images.
#[derive(Component)]
struct ButtonArt {
    kind: PauseButtonKind,
    states: [Handle<Image>; 3],
}

/// A button's label.
#[derive(Component)]
struct ButtonLabel {
    kind: PauseButtonKind,
}

/// The brass cursor.
#[derive(Component)]
struct PauseCursor {
    normal: Handle<Image>,
    rollover: Handle<Image>,
}

/// The mapping from the logical space to window pixels.
#[derive(Clone, Copy)]
struct Fit {
    scale: f32,
    left: f32,
    top: f32,
}

impl Fit {
    fn of(window: &Window) -> Option<Self> {
        let surface = (window.width() as u32, window.height() as u32);
        let fit = cs_content::ui_layout::AspectFit::new(SCREEN_SIZE, surface)?;
        let rect = fit.image_rect();
        Some(Self {
            scale: rect.width as f32 / SCREEN_SIZE.0 as f32,
            left: rect.x as f32,
            top: rect.y as f32,
        })
    }

    fn logical(&self, at: bevy::prelude::Vec2) -> bevy::prelude::Vec2 {
        bevy::prelude::Vec2::new(
            (at.x - self.left) / self.scale,
            (at.y - self.top) / self.scale,
        )
    }
}

/// Whether the player's Esc pause is holding the session.
fn pause_is_open(platform: &PlatformInput) -> bool {
    platform.session().is_paused()
        && platform.session().pause_reason() == Some(PauseReason::PlayerRequest)
}

fn to_image(art: &Artwork) -> Image {
    Image::new(
        Extent3d {
            width: art.width(),
            height: art.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        art.rgba().to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn rgb(color: [u8; 3]) -> Color {
    Color::srgb_u8(color[0], color[1], color[2])
}

fn absolute() -> Node {
    Node {
        position_type: PositionType::Absolute,
        ..default()
    }
}

fn spawn_pause_map(
    commands: &mut Commands,
    source: &PauseMapSource,
    images: &mut bevy::prelude::Assets<Image>,
) -> BTreeMap<String, (Handle<Image>, f32, f32)> {
    let scene = &source.scene;
    let handles: BTreeMap<String, (Handle<Image>, f32, f32)> = source
        .artwork
        .iter()
        .map(|(name, art)| {
            (
                name.clone(),
                (
                    images.add(to_image(art)),
                    art.width() as f32,
                    art.height() as f32,
                ),
            )
        })
        .collect();
    let handle_of = |name: &str| handles.get(&name.to_ascii_lowercase()).cloned();
    let image_part = |commands: &mut Commands, name: &str, region: (f32, f32), z: i32| {
        let (handle, width, height) = handle_of(name)?;
        Some(
            commands
                .spawn((
                    absolute(),
                    ImageNode::new(handle),
                    Placed {
                        x: region.0,
                        y: region.1,
                        width,
                        height,
                    },
                    GlobalZIndex(z),
                    PauseMapPart,
                    super::original_keys::ControlsHelpPanel,
                ))
                .id(),
        )
    };

    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..absolute()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(Z_BACKDROP),
        PauseMapPart,
        super::original_keys::ControlsHelpPanel,
    ));
    for (name, at) in &scene.background {
        image_part(commands, name, (at.x as f32, at.y as f32), Z_BACKGROUND);
    }
    // The chart is clipped: a container at the clip rectangle holds the
    // bitmap at its position relative to the container's corner.
    let chart = &scene.chart;
    let clip = Placed {
        x: chart.clip_top_left.x as f32,
        y: chart.clip_top_left.y as f32,
        width: (chart.clip_bottom_right.x - chart.clip_top_left.x) as f32,
        height: (chart.clip_bottom_right.y - chart.clip_top_left.y) as f32,
    };
    if let Some((handle, width, height)) = handle_of(&chart.bitmap) {
        commands
            .spawn((
                Node {
                    overflow: Overflow::clip(),
                    ..absolute()
                },
                clip,
                ChartClip,
                GlobalZIndex(Z_CHART),
                PauseMapPart,
                super::original_keys::ControlsHelpPanel,
            ))
            .with_children(|parent| {
                parent.spawn((
                    absolute(),
                    ImageNode::new(handle),
                    Placed {
                        x: (chart.at.x - chart.clip_top_left.x) as f32,
                        y: (chart.at.y - chart.clip_top_left.y) as f32,
                        width,
                        height,
                    },
                    PauseMapPart,
                ));
            });
    }
    // The photo's shadow, the photo, the note.
    for picture in scene
        .pictures
        .iter()
        .filter(|p| !p.id.starts_with("OBJPIN"))
    {
        place_picture(commands, picture, Z_SHADOW, &handle_of);
    }
    image_part(
        commands,
        &scene.memento.0,
        (scene.memento.1.x as f32, scene.memento.1.y as f32),
        Z_MEMENTO,
    );
    image_part(
        commands,
        &scene.note.bitmap,
        (scene.note.at.x as f32, scene.note.at.y as f32),
        Z_NOTE,
    );
    commands.spawn((
        absolute(),
        Text::new(scene.note.title.clone()),
        TextFont {
            font_size: FontSize::Px(TITLE_FONT_PX),
            ..default()
        },
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
        TextColor(rgb(TITLE_COLOR)),
        FontPx(TITLE_FONT_PX),
        Placed {
            x: scene.note.title_at.x as f32,
            y: scene.note.title_at.y as f32,
            width: 190.0,
            height: TITLE_FONT_PX * 1.4,
        },
        GlobalZIndex(Z_NOTE + 1),
        PauseMapPart,
        super::original_keys::ControlsHelpPanel,
    ));
    for button in &scene.buttons {
        let states = button
            .bitmaps
            .each_ref()
            .map(|name| handle_of(name).map(|(handle, _, _)| handle));
        let [Some(rest), Some(rollover), Some(activate)] = states else {
            continue;
        };
        let size = handle_of(&button.bitmaps[0]).map_or((132.0, 28.0), |h| (h.1, h.2));
        commands.spawn((
            absolute(),
            ImageNode::new(rest.clone()),
            Placed {
                x: button.at.x as f32,
                y: button.at.y as f32,
                width: size.0,
                height: size.1,
            },
            ButtonArt {
                kind: button.kind,
                states: [rest, rollover, activate],
            },
            GlobalZIndex(Z_BUTTON),
            PauseMapPart,
            super::original_keys::ControlsHelpPanel,
        ));
        // The label is centred on the record's offset in a row of the
        // button's own width.
        let centre = button.at.x as f32 + button.label_offset.x as f32;
        commands.spawn((
            absolute(),
            Text::new(button.label.clone()),
            TextFont {
                font_size: FontSize::Px(LABEL_FONT_PX),
                ..default()
            },
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
            TextColor(rgb(LABEL_NORMAL)),
            FontPx(LABEL_FONT_PX),
            Placed {
                x: centre - size.0 / 2.0,
                y: (button.at.y + button.label_offset.y) as f32,
                width: size.0,
                height: size.1,
            },
            ButtonLabel { kind: button.kind },
            GlobalZIndex(Z_BUTTON + 1),
            PauseMapPart,
            super::original_keys::ControlsHelpPanel,
        ));
    }
    if let (Some((normal, ..)), Some((rollover, ..))) =
        (handle_of(&scene.cursor.0), handle_of(&scene.cursor.1))
    {
        commands.spawn((
            absolute(),
            ImageNode::new(normal.clone()),
            PauseCursor { normal, rollover },
            GlobalZIndex(Z_CURSOR),
            PauseMapPart,
            super::original_keys::ControlsHelpPanel,
        ));
    }
    handles
}

fn place_picture(
    commands: &mut Commands,
    picture: &PausePicture,
    z: i32,
    handle_of: &impl Fn(&str) -> Option<(Handle<Image>, f32, f32)>,
) -> Option<Entity> {
    let (handle, width, height) = handle_of(&picture.bitmap)?;
    let (mut x, mut y) = (picture.at.x as f32, picture.at.y as f32);
    if picture.centered {
        x -= width / 2.0;
        y -= height / 2.0;
    }
    Some(
        commands
            .spawn((
                absolute(),
                ImageNode::new(handle),
                Placed {
                    x,
                    y,
                    width,
                    height,
                },
                GlobalZIndex(z),
                PauseMapPart,
                super::original_keys::ControlsHelpPanel,
            ))
            .id(),
    )
}

/// Spawns what the live state decides: one flag pin per objective, the
/// note's list and the player icon. Every part carries [`LivePart`].
fn spawn_live(
    commands: &mut Commands,
    scene: &PauseMapScene,
    images: &PauseMapImages,
    live: &PauseMapLive,
    clip: Option<Entity>,
) {
    let handle_of = |name: &str| images.0.get(&name.to_ascii_lowercase()).cloned();
    for pin in scene.pins() {
        let row = scene
            .pin_objective(pin)
            .and_then(|index| live.rows.get(index));
        let shown = PausePicture {
            bitmap: pin_flag_bitmap(&pin.bitmap, row),
            ..pin.clone()
        };
        if let Some(entity) = place_picture(commands, &shown, Z_PINS, &handle_of) {
            commands.entity(entity).insert(LivePart);
        }
    }

    let revealed: Vec<_> = live.rows.iter().filter(|row| row.revealed).collect();
    if !revealed.is_empty() {
        let note = &scene.note;
        let check = handle_of(&note.check_mark);
        commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                    ..absolute()
                },
                Placed {
                    x: note.list_at.x as f32,
                    y: note.list_at.y as f32,
                    width: note.list_wrap.0 as f32,
                    height: note.list_wrap.1 as f32,
                },
                RowGap(note.list_spacing as f32),
                GlobalZIndex(Z_NOTE + 1),
                PauseMapPart,
                LivePart,
                super::original_keys::ControlsHelpPanel,
            ))
            .with_children(|list| {
                for row in revealed {
                    list.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            ..default()
                        },
                        PauseMapPart,
                    ))
                    .with_children(|line| {
                        if let (true, Some((handle, width, height))) = (row.completed, &check) {
                            line.spawn((
                                absolute(),
                                ImageNode::new(handle.clone()),
                                Placed {
                                    x: -CHECK_MARK_WIDTH,
                                    y: 0.0,
                                    width: *width,
                                    height: *height,
                                },
                                PauseMapPart,
                            ));
                        }
                        line.spawn((
                            Text::new(row.text.clone()),
                            TextFont {
                                font_size: FontSize::Px(LIST_FONT_PX),
                                ..default()
                            },
                            TextLayout::new(Justify::Left, LineBreak::WordBoundary),
                            TextColor(rgb(LIST_COLOR)),
                            FontPx(LIST_FONT_PX),
                            PauseMapPart,
                        ));
                    });
                }
            });
    }

    // The player icon sits inside the chart's clip container, so the chart's
    // edge clips it.
    if let (Some(fix), Some(clip), Some((handle, width, height))) =
        (live.player, clip, handle_of(&scene.own_ship))
        && let Some((px, py)) = world_to_chart_pixel(&scene.chart, fix.x, fix.z)
    {
        let origin = scene.chart.clip_top_left;
        commands.entity(clip).with_children(|parent| {
            parent.spawn((
                absolute(),
                ImageNode::new(handle),
                Placed {
                    x: px - origin.x as f32 - width / 2.0,
                    y: py - origin.y as f32 - height / 2.0,
                    width,
                    height,
                },
                UiTransform::from_rotation(Rot2::radians(fix.heading)),
                PauseMapPart,
                LivePart,
            ));
        });
    }
}

/// Builds the live parts when the screen opens and again whenever the live
/// state changes while it is up.
#[allow(clippy::too_many_arguments)]
fn refresh_live(
    mut commands: Commands,
    source: Res<PauseMapSource>,
    images: Res<PauseMapImages>,
    live: Res<PauseMapLive>,
    shown: Res<PauseMapShown>,
    old: Query<Entity, With<LivePart>>,
    clips: Query<Entity, With<ChartClip>>,
    mut built: Local<Option<PauseMapLive>>,
) {
    if !shown.0 {
        *built = None;
        return;
    }
    if built.as_ref() == Some(&*live) {
        return;
    }
    for part in &old {
        commands.entity(part).despawn();
    }
    spawn_live(
        &mut commands,
        &source.scene,
        &images,
        &live,
        clips.iter().next(),
    );
    *built = Some(live.clone());
}

/// Spawns the screen while the player's Esc pause holds the session and
/// removes it when the pause ends; hides the system pointer while it is up.
#[allow(clippy::too_many_arguments)]
fn sync_pause_map(
    mut commands: Commands,
    source: Res<PauseMapSource>,
    platform: Option<Res<PlatformInput>>,
    pending: Option<Res<super::flight_check::FlightCheckPhase>>,
    overlay: Option<Res<PreferencesOverlay>>,
    mut images: ResMut<bevy::prelude::Assets<Image>>,
    mut shown: ResMut<PauseMapShown>,
    parts: Query<Entity, With<PauseMapPart>>,
    mut cursors: Query<&mut CursorOptions>,
) {
    // A pre-mission screen (briefing, Flight Check) is not the mission, and
    // while the preferences overlay the Preferences button opened is up the
    // map is hidden under it: no part drawn, no button live.
    let covered = overlay.is_some_and(|overlay| overlay.open);
    let wanted = platform.is_some_and(|platform| pause_is_open(&platform))
        && !pending.is_some_and(|phase| phase.is_pending())
        && !covered;
    if wanted && !shown.0 {
        let handles = spawn_pause_map(&mut commands, &source, &mut images);
        commands.insert_resource(PauseMapImages(handles));
        shown.0 = true;
    } else if !wanted && shown.0 {
        for part in &parts {
            commands.entity(part).despawn();
        }
        shown.0 = false;
    }
    for mut cursor in &mut cursors {
        if cursor.visible == shown.0 {
            cursor.visible = !shown.0;
        }
    }
}

/// Lays every part out in window pixels: a top-level part sits in the fitted
/// 800x600 rectangle, a nested one (the chart inside its clip) in its parent.
#[allow(clippy::type_complexity)]
fn place_pause_map(
    windows: Query<&Window>,
    mut parts: Query<
        (
            &Placed,
            &mut Node,
            Option<&FontPx>,
            Option<&mut TextFont>,
            Option<&RowGap>,
            Has<ChildOf>,
        ),
        With<PauseMapPart>,
    >,
    mut unplaced_text: Query<(&FontPx, &mut TextFont), (With<PauseMapPart>, Without<Placed>)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(fit) = Fit::of(window) else {
        return;
    };
    for (placed, mut node, size, font, gap, nested) in &mut parts {
        let (left, top) = if nested {
            (0.0, 0.0)
        } else {
            (fit.left, fit.top)
        };
        node.left = Val::Px(left + placed.x * fit.scale);
        node.top = Val::Px(top + placed.y * fit.scale);
        node.width = Val::Px(placed.width * fit.scale);
        node.height = Val::Px(placed.height * fit.scale);
        if let Some(gap) = gap {
            node.row_gap = Val::Px(gap.0 * fit.scale);
        }
        if let (Some(size), Some(mut font)) = (size, font) {
            font.font_size = FontSize::Px(size.0 * fit.scale);
        }
    }
    for (size, mut font) in &mut unplaced_text {
        font.font_size = FontSize::Px(size.0 * fit.scale);
    }
}

/// Rollover, press and the brass cursor.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive_pause_map(
    shown: Res<PauseMapShown>,
    overlay: Option<Res<PreferencesOverlay>>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut platform: ResMut<PlatformInput>,
    mut presses: MessageWriter<PauseMapPress>,
    mut opens: MessageWriter<OpenPreferencesOverlay>,
    mut exit: MessageWriter<AppExit>,
    mut commands: bevy::prelude::Commands,
    return_to_front_end: Option<Res<super::ReturnToFrontEnd>>,
    left_cell: Option<Res<super::MissionLeftCell>>,
    mut arts: Query<(&ButtonArt, &mut ImageNode, &Placed)>,
    mut labels: Query<(&ButtonLabel, &mut TextColor)>,
    mut cursors: Query<
        (&PauseCursor, &mut ImageNode, &mut Node),
        bevy::prelude::Without<ButtonArt>,
    >,
) {
    // While the overlay covers the screen the buttons are dead — nothing
    // hovered, no press (the frame the overlay opened on can still hold the
    // map's parts until `sync_pause_map` despawns them).
    if !shown.0 || overlay.is_some_and(|overlay| overlay.open) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(fit) = Fit::of(window) else {
        return;
    };
    let pointer = window.cursor_position();
    let hovered = pointer.and_then(|at| {
        let logical = fit.logical(at);
        arts.iter()
            .find(|(_, _, placed)| {
                logical.x >= placed.x
                    && logical.x < placed.x + placed.width
                    && logical.y >= placed.y
                    && logical.y < placed.y + placed.height
            })
            .map(|(art, _, _)| art.kind)
    });
    let held = mouse.pressed(MouseButton::Left);
    for (art, mut node, _) in &mut arts {
        let state = match hovered {
            Some(kind) if kind == art.kind && held => 2,
            Some(kind) if kind == art.kind => 1,
            _ => 0,
        };
        if node.image != art.states[state] {
            node.image = art.states[state].clone();
        }
    }
    for (label, mut color) in &mut labels {
        color.0 = rgb(match hovered {
            Some(kind) if kind == label.kind && held => LABEL_ACTIVATE,
            Some(kind) if kind == label.kind => LABEL_ROLLOVER,
            _ => LABEL_NORMAL,
        });
    }
    for (cursor, mut image, mut node) in &mut cursors {
        let wanted = if hovered.is_some() {
            &cursor.rollover
        } else {
            &cursor.normal
        };
        if image.image != *wanted {
            image.image = wanted.clone();
        }
        match pointer {
            Some(at) => {
                node.display = bevy::prelude::Display::Flex;
                node.left = Val::Px(at.x);
                node.top = Val::Px(at.y);
                node.width = Val::Px(40.0 * fit.scale);
                node.height = Val::Px(40.0 * fit.scale);
            }
            None => node.display = bevy::prelude::Display::None,
        }
    }
    if mouse.just_pressed(MouseButton::Left)
        && let Some(kind) = hovered
    {
        if kind == PauseButtonKind::Resume {
            platform.session_mut().resume();
        }
        if kind == PauseButtonKind::Preferences {
            opens.write(OpenPreferencesOverlay);
        }
        if kind == PauseButtonKind::Quit {
            // `cs --mission` composes no front-end machine, so there is no
            // main menu to return to: leaving the mission ends the window
            // like the developer F10 does, and `run_windowed` tears the
            // composition down on the way out. The player abandoned the
            // flight, which is not a mission failure, so the exit is clean.
            // A run the start flow handed over also records that the player
            // is going back to the cabin (#1831).
            if return_to_front_end.is_some() {
                commands.insert_resource(super::MissionLeft(super::MissionEnd::Abandoned));
                if let Some(cell) = &left_cell {
                    cell.record(super::MissionEnd::Abandoned);
                }
            }
            exit.write(AppExit::Success);
        }
        presses.write(PauseMapPress(kind));
    }
}

/// **Restart**: closes the pause and hands the composed host a
/// [`MissionHostRestartRequest`], which `mission_host_restart` (`PostUpdate`)
/// consumes in the same frame. A composition with no [`MissionHost`] has no
/// session to restart, so the press changes nothing there.
fn restart_on_press(
    mut presses: MessageReader<PauseMapPress>,
    host: Option<Res<MissionHost>>,
    mut platform: ResMut<PlatformInput>,
    mut commands: Commands,
) {
    let restart = presses
        .read()
        .any(|press| press.0 == PauseButtonKind::Restart);
    if restart && host.is_some() {
        platform.session_mut().resume();
        commands.insert_resource(MissionHostRestartRequest);
    }
}

/// The mesh HUD (dials and compass) is drawn by its own camera over the UI, so
/// the screen hides it while it is open and gives it back when it closes —
/// and while the preferences overlay covers it (#1923): the flight
/// instruments belong to no menu page.
fn hide_hud_art(
    shown: Res<PauseMapShown>,
    overlay: Option<Res<PreferencesOverlay>>,
    mut was: bevy::prelude::Local<bool>,
    mut roots: Query<&mut bevy::prelude::Visibility, With<super::hud_art::HudArtRoot>>,
) {
    let up = shown.0 || overlay.is_some_and(|overlay| overlay.open);
    if up == *was {
        return;
    }
    *was = up;
    for mut visibility in &mut roots {
        *visibility = if up {
            bevy::prelude::Visibility::Hidden
        } else {
            bevy::prelude::Visibility::Inherited
        };
    }
}

/// Installs the pause map over a mission composition.
pub fn install_pause_map(app: &mut App, source: PauseMapSource) {
    app.insert_resource(source);
    app.add_plugins(PauseMapPlugin);
}

/// The window's face of the Esc pause.
#[derive(Clone, Copy, Debug, Default)]
pub struct PauseMapPlugin;

impl Plugin for PauseMapPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AppExit>()
            .init_resource::<PauseMapShown>()
            .init_resource::<PauseMapLive>()
            .init_resource::<PauseMapImages>()
            .add_message::<PauseMapPress>()
            // Registered here too so `drive_pause_map` has a writer whether
            // or not the overlay's plugin is installed.
            .add_message::<OpenPreferencesOverlay>()
            .add_systems(
                Update,
                (
                    read_pause_map_live,
                    sync_pause_map,
                    refresh_live,
                    place_pause_map,
                    drive_pause_map,
                    restart_on_press,
                    hide_hud_art,
                )
                    .chain(),
            );
    }
}
