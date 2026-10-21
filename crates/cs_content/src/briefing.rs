//! The original mission briefing screen's script (task #1750
//! `PARITY5-FRONTEND-BRIEFING-MAP`).
//!
//! `Briefing.zrd`, a member of the install-wide reader `ZBD/zrdr.zbd`, holds
//! the whole briefing screen as data:
//!
//! * `BRIEFINGDIALOG` — the objectives panel (a parchment bitmap, a title
//!   string id, the list's position, word-wrap box and spacing) and the three
//!   buttons (`REPLAY`, `RETURNTOCABIN`, `FLIGHTCHECK`: position, bitmap, the
//!   string id of the label and its offset, the font names of the three
//!   states);
//! * one `brief_c<NN>` record per mission: the map picture, the voice-over cue
//!   (`PlaySound`) and a `SCRIPT` of timed commands that put pictures, pins
//!   and objective lines on the map.
//!
//! [`BriefingDocument::decode`] reads the member into typed records;
//! [`BriefingDocument::script_for_sound`] finds the script a mission plays by
//! the voice cue's name (`briefing_c1m1` for the first mission of the first
//! chapter), which the campaign position states
//! ([`crate::campaign_bindings::CampaignMission`]).
//!
//! # What is read, and what is not
//!
//! The command set is the twelve the retail member uses (`PlaySound`,
//! `WaitForMarker`, `Wait`, `Pict`, `Fade`, `Spin`, `Move`, `On`, `Off`,
//! `ToBack`, `Objective`, `Line`); the grammar of every one is read strictly
//! and a record that deviates is a [`BriefingError`] naming the script and the
//! command. A command outside the set is kept as
//! [`BriefingCommand::Unknown`] and listed by [`BriefingScript::unknowns`]: it
//! is recorded, never skipped. The *meaning* of a command's numbers — what the
//! `Spin` revolutions are relative to, whether the `Fade`/`Spin`/`Move`
//! commands run beside the script or hold it, what `startmarker` is — is not
//! stated by the data; those readings live in the player
//! (`cs_app::ui::front_end::briefing`) and are labelled there.
//!
//! [`wave_cue_points`] reads the `cue ` chunk of the voice-over member, which
//! is what `WaitForMarker` waits on.
//!
//! Nothing here is `verified_original`.

use std::fmt;

use crate::stunts::{ZrdValue, decode_zrd};

/// The reader archive holding the briefing record.
pub const BRIEFING_ARCHIVE: &str = "ZBD/zrdr.zbd";

/// The member of [`BRIEFING_ARCHIVE`] holding the briefing record.
pub const BRIEFING_MEMBER: &str = "Briefing.zrd";

/// The sound-group table's member (`sounds.zrd`): its key `brief_c61` maps to
/// the voice-over archive member.
pub const SOUND_GROUPS_MEMBER: &str = "sounds.zrd";

/// Why the briefing record could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BriefingError {
    /// The script or section the problem is in.
    pub scope: String,
    /// What is wrong.
    pub reason: String,
}

impl BriefingError {
    fn new(scope: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for BriefingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.scope, self.reason)
    }
}

impl std::error::Error for BriefingError {}

/// A point in the 800x600 briefing space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    /// Pixels from the left edge.
    pub x: i32,
    /// Pixels from the top edge.
    pub y: i32,
}

/// The objectives panel `BRIEFINGDIALOG` declares.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectivesPanel {
    /// The parchment bitmap the panel is drawn on.
    pub background_bitmap: String,
    /// Where the bitmap is drawn.
    pub background_at: Point,
    /// The string id of the panel's title.
    pub title_text: String,
    /// The title's font name.
    pub title_font: String,
    /// Where the title is drawn.
    pub title_at: Point,
    /// The list's font name.
    pub list_font: String,
    /// Where the list starts.
    pub list_at: Point,
    /// The list's word-wrap box (width, height).
    pub wordwrap: (i32, i32),
    /// The spacing between list entries.
    pub spacing: i32,
}

/// Which of the three buttons a dialog button is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DialogButtonKind {
    /// `REPLAY`: Replay Briefing.
    Replay,
    /// `RETURNTOCABIN`: Return to Cabin.
    ReturnToCabin,
    /// `FLIGHTCHECK`: Go to Flight Check.
    FlightCheck,
}

impl DialogButtonKind {
    /// Every button in the record's own order.
    pub const ALL: [Self; 3] = [Self::Replay, Self::ReturnToCabin, Self::FlightCheck];

    /// The key the record names the button by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Replay => "REPLAY",
            Self::ReturnToCabin => "RETURNTOCABIN",
            Self::FlightCheck => "FLIGHTCHECK",
        }
    }
}

/// One of the dialog's three buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogButton {
    /// Which button.
    pub kind: DialogButtonKind,
    /// Where it is drawn.
    pub at: Point,
    /// The button's bitmap name.
    pub bitmap: String,
    /// The string id of its label.
    pub label_text: String,
    /// The label's offset from the button's corner.
    pub label_offset: Point,
    /// The label's font at rest.
    pub normal_font: String,
    /// The label's font under the pointer.
    pub rollover_font: String,
    /// The label's font while pressed.
    pub activate_font: String,
}

/// The `BRIEFINGDIALOG` record.
#[derive(Clone, Debug, PartialEq)]
pub struct BriefingDialog {
    /// The objectives panel.
    pub objectives: ObjectivesPanel,
    /// The three buttons, in the record's order.
    pub buttons: Vec<DialogButton>,
}

/// The voice-over a script plays.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundCue {
    /// The sound's name (`briefing_c1m1`), a `sounds.zrd` entry of the
    /// script's own key.
    pub name: String,
    /// The `vol` value.
    pub volume: f32,
    /// The `markers` flag: the sound's cue points are reported to the script.
    pub markers: bool,
    /// The `startmarker` value.
    pub start_marker: u32,
}

/// One background picture the script names (`BACKGROUND_IMAGES`).
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundImage {
    /// The bitmap name.
    pub bitmap: String,
    /// Where it is drawn.
    pub at: Point,
}

/// One timed command of a briefing script.
#[derive(Clone, Debug, PartialEq)]
pub enum BriefingCommand {
    /// `PlaySound`: starts the voice-over.
    PlaySound(SoundCue),
    /// `WaitForMarker n`: holds the script until marker `n` is reached.
    WaitForMarker(u32),
    /// `Wait s`: holds the script for `s` seconds.
    Wait(f32),
    /// `Pict id bitmap at center`: creates a picture.
    Pict {
        /// The element's name.
        id: String,
        /// The bitmap name.
        bitmap: String,
        /// Where it is created.
        at: Point,
        /// The `center` flag: `at` is the picture's centre.
        center: bool,
    },
    /// `Fade id start end duration`: opacity from `start` to `end`.
    Fade {
        /// The element's name.
        id: String,
        /// The opacity at the start.
        start: f32,
        /// The opacity at the end.
        end: f32,
        /// Seconds.
        duration: f32,
    },
    /// `Spin id [startrevs] revs duration`.
    Spin {
        /// The element's name.
        id: String,
        /// `startrevs`, when the command states one.
        start_revs: Option<f32>,
        /// `revs`.
        revs: f32,
        /// Seconds.
        duration: f32,
    },
    /// `Move id path duration`: from the path's first point to its second.
    Move {
        /// The element's name.
        id: String,
        /// The two path points.
        path: [Point; 2],
        /// Seconds.
        duration: f32,
    },
    /// `On id`: shows the element.
    On(String),
    /// `Off id`: hides the element.
    Off(String),
    /// `ToBack id`: sends the element behind the others.
    ToBack(String),
    /// `Objective id index`: the element is the objective line `index`.
    Objective {
        /// The element's name.
        id: String,
        /// The zero-based objective index.
        index: u32,
    },
    /// `Line id points color`: a line between two points.
    Line {
        /// The element's name.
        id: String,
        /// The two end points.
        points: [Point; 2],
        /// The colour as red, green, blue.
        color: [u8; 3],
    },
    /// A command outside the twelve the retail member uses.
    Unknown {
        /// The command's spelling.
        name: String,
    },
}

/// One `brief_c<NN>` record.
#[derive(Clone, Debug, PartialEq)]
pub struct BriefingScript {
    /// The record's key (`brief_c61`).
    pub key: String,
    /// The `IMAGE_PATH` the record names, verbatim.
    pub image_path: String,
    /// `BACKGROUND_IMAGES`: the map picture(s).
    pub background: Vec<BackgroundImage>,
    /// The voice-over the script starts, when it has a `PlaySound`.
    pub sound: Option<SoundCue>,
    /// Every command in order, `PlaySound` included.
    pub commands: Vec<BriefingCommand>,
}

impl BriefingScript {
    /// The spellings of every command the reader does not know.
    #[must_use]
    pub fn unknowns(&self) -> Vec<&str> {
        self.commands
            .iter()
            .filter_map(|command| match command {
                BriefingCommand::Unknown { name } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The highest marker number a `WaitForMarker` names.
    #[must_use]
    pub fn highest_marker(&self) -> Option<u32> {
        self.commands
            .iter()
            .filter_map(|command| match command {
                BriefingCommand::WaitForMarker(number) => Some(*number),
                _ => None,
            })
            .max()
    }

    /// Every bitmap name the script and its background draw.
    #[must_use]
    pub fn bitmaps(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .background
            .iter()
            .map(|image| image.bitmap.as_str())
            .collect();
        for command in &self.commands {
            if let BriefingCommand::Pict { bitmap, .. } = command
                && !names.iter().any(|name| name.eq_ignore_ascii_case(bitmap))
            {
                names.push(bitmap);
            }
        }
        names
    }
}

/// The decoded `Briefing.zrd`.
#[derive(Clone, Debug, PartialEq)]
pub struct BriefingDocument {
    /// The dialog record.
    pub dialog: BriefingDialog,
    /// Every `brief_c<NN>` record, in the member's order.
    pub scripts: Vec<BriefingScript>,
}

impl BriefingDocument {
    /// Decodes the member's bytes.
    ///
    /// # Errors
    ///
    /// [`BriefingError`] when the bytes are not a `.zrd` tree, the dialog
    /// record is missing or malformed, or a script's command deviates from its
    /// grammar.
    pub fn decode(bytes: &[u8]) -> Result<Self, BriefingError> {
        let tree = decode_zrd(bytes).map_err(|error| {
            BriefingError::new(
                BRIEFING_MEMBER,
                format!(
                    "not a .zrd tree: {} at byte {}",
                    error.code(),
                    error.offset()
                ),
            )
        })?;
        let root = tree
            .as_list()
            .and_then(|children| children.first())
            .and_then(ZrdValue::as_list)
            .ok_or_else(|| BriefingError::new(BRIEFING_MEMBER, "the root is not a record"))?;
        let dialog_node = flat_field(root, "BRIEFINGDIALOG")
            .ok_or_else(|| BriefingError::new(BRIEFING_MEMBER, "no BRIEFINGDIALOG record"))?;
        let dialog = read_dialog(dialog_node)?;
        let mut scripts = Vec::new();
        let mut index = 0;
        while index + 1 < root.len() {
            if let Some(key) = root[index].as_text()
                && key.to_ascii_lowercase().starts_with("brief_c")
            {
                scripts.push(read_script(key, &root[index + 1])?);
            }
            index += 2;
        }
        Ok(Self { dialog, scripts })
    }

    /// The script that plays the voice cue `sound` (`briefing_c1m1`),
    /// compared ASCII case-insensitively.
    #[must_use]
    pub fn script_for_sound(&self, sound: &str) -> Option<&BriefingScript> {
        self.scripts.iter().find(|script| {
            script
                .sound
                .as_ref()
                .is_some_and(|cue| cue.name.eq_ignore_ascii_case(sound))
        })
    }
}

/// The voice cue name the campaign position `(chapter, mission)` plays:
/// `briefing_c<chapter>m<mission>`. The pattern is read from the retail
/// member (every one of its 24 scripts follows it), never assumed for a
/// position the member does not have: [`BriefingDocument::script_for_sound`]
/// answers `None` for those.
#[must_use]
pub fn sound_name_for(chapter: u32, mission: u32) -> String {
    format!("briefing_c{chapter}m{mission}")
}

// ------------------------------------------------------------- reading --

fn flat_field<'a>(children: &'a [ZrdValue], key: &str) -> Option<&'a ZrdValue> {
    let mut index = 0;
    while index + 1 < children.len() {
        if children[index].as_text() == Some(key) {
            return Some(&children[index + 1]);
        }
        index += 2;
    }
    None
}

fn number(node: &ZrdValue) -> Option<f32> {
    match node {
        ZrdValue::Int(value) => Some(i32::from_ne_bytes(value.to_ne_bytes()) as f32),
        ZrdValue::Float(value) => Some(*value),
        _ => None,
    }
}

fn integer(node: &ZrdValue) -> Option<i32> {
    match node {
        ZrdValue::Int(value) => Some(i32::from_ne_bytes(value.to_ne_bytes())),
        _ => None,
    }
}

fn point(node: &ZrdValue) -> Option<Point> {
    let pair = node.as_list()?;
    match pair {
        [x, y] => Some(Point {
            x: integer(x)?,
            y: integer(y)?,
        }),
        _ => None,
    }
}

fn first_text(node: &ZrdValue) -> Option<&str> {
    node.as_list()?.first()?.as_text()
}

fn first_number(node: &ZrdValue) -> Option<f32> {
    number(node.as_list()?.first()?)
}

fn required<'a>(
    scope: &str,
    children: &'a [ZrdValue],
    key: &str,
) -> Result<&'a ZrdValue, BriefingError> {
    flat_field(children, key).ok_or_else(|| BriefingError::new(scope, format!("no {key} field")))
}

fn list<'a>(scope: &str, node: &'a ZrdValue, what: &str) -> Result<&'a [ZrdValue], BriefingError> {
    node.as_list()
        .ok_or_else(|| BriefingError::new(scope, format!("{what} is not a list")))
}

fn text_of(scope: &str, node: &ZrdValue, what: &str) -> Result<String, BriefingError> {
    first_text(node)
        .map(str::to_owned)
        .ok_or_else(|| BriefingError::new(scope, format!("{what} is not a name")))
}

fn point_of(scope: &str, node: &ZrdValue, what: &str) -> Result<Point, BriefingError> {
    point(node).ok_or_else(|| BriefingError::new(scope, format!("{what} is not a point")))
}

fn read_dialog(node: &ZrdValue) -> Result<BriefingDialog, BriefingError> {
    let scope = "BRIEFINGDIALOG";
    let children = list(scope, node, "the record")?;
    let primitives = list(
        scope,
        required(scope, children, "PRIMITIVES")?,
        "PRIMITIVES",
    )?;
    let panel = list(
        scope,
        required(scope, primitives, "OBJECTIVESLIST")?,
        "OBJECTIVESLIST",
    )?;
    let background = list(scope, required(scope, panel, "BACKGROUND")?, "BACKGROUND")?;
    let title = list(scope, required(scope, panel, "TITLE")?, "TITLE")?;
    let entries = list(scope, required(scope, panel, "LIST")?, "LIST")?;
    let wordwrap = list(scope, required(scope, entries, "WORDWRAP")?, "WORDWRAP")?;
    let wrap = match wordwrap {
        [width, height] => integer(width).zip(integer(height)),
        _ => None,
    }
    .ok_or_else(|| BriefingError::new(scope, "WORDWRAP is not a width and a height"))?;
    let objectives = ObjectivesPanel {
        background_bitmap: text_of(scope, required(scope, background, "BITMAP")?, "BITMAP")?,
        background_at: point_of(scope, required(scope, background, "POSITION")?, "POSITION")?,
        title_text: text_of(scope, required(scope, title, "TEXT")?, "TITLE TEXT")?,
        title_font: text_of(scope, required(scope, title, "FONT")?, "TITLE FONT")?,
        title_at: point_of(scope, required(scope, title, "POSITION")?, "TITLE POSITION")?,
        list_font: text_of(scope, required(scope, entries, "FONT")?, "LIST FONT")?,
        list_at: point_of(
            scope,
            required(scope, entries, "POSITION")?,
            "LIST POSITION",
        )?,
        wordwrap: wrap,
        spacing: first_number(required(scope, entries, "SPACING")?)
            .map(|value| value as i32)
            .ok_or_else(|| BriefingError::new(scope, "SPACING is not a number"))?,
    };
    let buttons_node = list(scope, required(scope, children, "BUTTONS")?, "BUTTONS")?;
    let mut buttons = Vec::new();
    for kind in DialogButtonKind::ALL {
        let button_scope = format!("BRIEFINGDIALOG.{}", kind.key());
        let fields = list(
            &button_scope,
            required(&button_scope, buttons_node, kind.key())?,
            "the button",
        )?;
        let label = list(
            &button_scope,
            required(&button_scope, fields, "LABEL")?,
            "LABEL",
        )?;
        let label_text = label
            .first()
            .and_then(ZrdValue::as_text)
            .ok_or_else(|| BriefingError::new(&button_scope, "LABEL names no string id"))?
            .to_owned();
        let label_offset = flat_field(&label[1..], "offset")
            .ok_or_else(|| BriefingError::new(&button_scope, "LABEL has no offset"))
            .and_then(|node| point_of(&button_scope, node, "offset"))?;
        let font_of = |fields: &[ZrdValue], what: &str| -> Result<String, BriefingError> {
            let label = list(
                &button_scope,
                required(&button_scope, fields, "LABEL")?,
                what,
            )?;
            let font = flat_field(&label[1..], "font")
                .ok_or_else(|| BriefingError::new(&button_scope, format!("{what} has no font")))?;
            text_of(&button_scope, font, what)
        };
        let state = |key: &str| -> Result<&[ZrdValue], BriefingError> {
            list(&button_scope, required(&button_scope, fields, key)?, key)
        };
        buttons.push(DialogButton {
            kind,
            at: point_of(
                &button_scope,
                required(&button_scope, fields, "POSITION")?,
                "POSITION",
            )?,
            bitmap: text_of(
                &button_scope,
                required(&button_scope, fields, "BITMAP")?,
                "BITMAP",
            )?,
            label_text,
            label_offset,
            normal_font: font_of(fields, "LABEL")?,
            rollover_font: font_of(state("ROLLOVER")?, "ROLLOVER")?,
            activate_font: font_of(state("ACTIVATE")?, "ACTIVATE")?,
        });
    }
    Ok(BriefingDialog {
        objectives,
        buttons,
    })
}

fn read_script(key: &str, node: &ZrdValue) -> Result<BriefingScript, BriefingError> {
    let children = list(key, node, "the record")?;
    let image_path = text_of(key, required(key, children, "IMAGE_PATH")?, "IMAGE_PATH")?;
    let mut background = Vec::new();
    for entry in list(
        key,
        required(key, children, "BACKGROUND_IMAGES")?,
        "BACKGROUND_IMAGES",
    )? {
        let parts = list(key, entry, "a background image")?;
        match parts {
            [name, x, y] => background.push(BackgroundImage {
                bitmap: name
                    .as_text()
                    .ok_or_else(|| BriefingError::new(key, "a background image has no name"))?
                    .to_owned(),
                at: Point {
                    x: integer(x).ok_or_else(|| BriefingError::new(key, "bad background x"))?,
                    y: integer(y).ok_or_else(|| BriefingError::new(key, "bad background y"))?,
                },
            }),
            _ => {
                return Err(BriefingError::new(
                    key,
                    "a background image is not a name and a point",
                ));
            }
        }
    }
    let script = list(key, required(key, children, "SCRIPT")?, "SCRIPT")?;
    let mut commands = Vec::new();
    let mut sound = None;
    for pair in script.chunks(2) {
        let [name, args] = pair else {
            return Err(BriefingError::new(key, "the script ends inside a command"));
        };
        let name = name
            .as_text()
            .ok_or_else(|| BriefingError::new(key, "a command has no name"))?;
        let command = read_command(key, name, args)?;
        if let BriefingCommand::PlaySound(cue) = &command {
            sound = Some(cue.clone());
        }
        commands.push(command);
    }
    Ok(BriefingScript {
        key: key.to_owned(),
        image_path,
        background,
        sound,
        commands,
    })
}

/// A command's arguments: the optional leading element name and the keyed
/// values after it.
struct Args<'a> {
    id: Option<&'a str>,
    rest: &'a [ZrdValue],
}

impl<'a> Args<'a> {
    fn new(node: &'a ZrdValue) -> Option<Self> {
        let items = node.as_list()?;
        let (id, rest) = match items.first() {
            Some(first) if first.as_text().is_some() && items.len() % 2 == 1 => {
                (first.as_text(), &items[1..])
            }
            _ => (None, items),
        };
        Some(Self { id, rest })
    }

    fn field(&self, key: &str) -> Option<&'a ZrdValue> {
        flat_field(self.rest, key)
    }
}

fn read_command(
    script: &str,
    name: &str,
    node: &ZrdValue,
) -> Result<BriefingCommand, BriefingError> {
    let scope = format!("{script}.{name}");
    let bad = |what: &str| BriefingError::new(&scope, what.to_owned());
    let args = Args::new(node).ok_or_else(|| bad("the arguments are not a list"))?;
    let id = || {
        args.id
            .map(str::to_owned)
            .ok_or_else(|| bad("no element name"))
    };
    let value = |key: &str| {
        args.field(key)
            .and_then(first_number)
            .ok_or_else(|| bad(&format!("no numeric {key}")))
    };
    let lowered = name.to_ascii_lowercase();
    Ok(match lowered.as_str() {
        "playsound" => BriefingCommand::PlaySound(SoundCue {
            name: args
                .field("sound")
                .and_then(first_text)
                .ok_or_else(|| bad("no sound"))?
                .to_owned(),
            volume: value("vol")?,
            markers: args
                .field("markers")
                .and_then(first_text)
                .is_some_and(|flag| flag.eq_ignore_ascii_case("true")),
            start_marker: value("startmarker")? as u32,
        }),
        "waitformarker" => BriefingCommand::WaitForMarker(
            args.rest
                .first()
                .and_then(integer)
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| bad("no marker number"))?,
        ),
        "wait" => BriefingCommand::Wait(
            args.rest
                .first()
                .and_then(number)
                .ok_or_else(|| bad("no duration"))?,
        ),
        "pict" => BriefingCommand::Pict {
            id: id()?,
            bitmap: args
                .field("bitmap")
                .and_then(first_text)
                .ok_or_else(|| bad("no bitmap"))?
                .to_owned(),
            at: args
                .field("at")
                .and_then(point)
                .ok_or_else(|| bad("no position"))?,
            center: args
                .field("center")
                .and_then(first_text)
                .is_some_and(|flag| flag.eq_ignore_ascii_case("true")),
        },
        "fade" => BriefingCommand::Fade {
            id: id()?,
            start: value("start")?,
            end: value("end")?,
            duration: value("duration")?,
        },
        "spin" => BriefingCommand::Spin {
            id: id()?,
            start_revs: args.field("startrevs").and_then(first_number),
            revs: value("revs")?,
            duration: value("duration")?,
        },
        "move" => {
            let path = args
                .field("path")
                .and_then(ZrdValue::as_list)
                .ok_or_else(|| bad("no path"))?;
            let [from, to] = path else {
                return Err(bad("the path is not two points"));
            };
            BriefingCommand::Move {
                id: id()?,
                path: [
                    point(from).ok_or_else(|| bad("a path point is malformed"))?,
                    point(to).ok_or_else(|| bad("a path point is malformed"))?,
                ],
                duration: value("duration")?,
            }
        }
        "on" => BriefingCommand::On(id()?),
        "off" => BriefingCommand::Off(id()?),
        "toback" => BriefingCommand::ToBack(id()?),
        "objective" => BriefingCommand::Objective {
            id: id()?,
            index: value("index")? as u32,
        },
        "line" => {
            let points = args
                .field("points")
                .and_then(ZrdValue::as_list)
                .ok_or_else(|| bad("no points"))?;
            let [from, to] = points else {
                return Err(bad("the line is not two points"));
            };
            let color = args
                .field("color")
                .and_then(ZrdValue::as_list)
                .ok_or_else(|| bad("no colour"))?;
            let [red, green, blue] = color else {
                return Err(bad("the colour is not three components"));
            };
            let channel = |node: &ZrdValue| {
                integer(node)
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or_else(|| bad("a colour component is not 0..=255"))
            };
            BriefingCommand::Line {
                id: id()?,
                points: [
                    point(from).ok_or_else(|| bad("a line point is malformed"))?,
                    point(to).ok_or_else(|| bad("a line point is malformed"))?,
                ],
                color: [channel(red)?, channel(green)?, channel(blue)?],
            }
        }
        _ => BriefingCommand::Unknown {
            name: name.to_owned(),
        },
    })
}

// ---------------------------------------------------------- voice cues --

/// One `cue ` point of a RIFF/WAVE member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CuePoint {
    /// The cue point's identifier.
    pub id: u32,
    /// `dwPosition`: the point's position in sample frames of the member's own
    /// rate.
    pub position: u32,
}

/// The cue points and rate of one WAVE member.
#[derive(Clone, Debug, PartialEq)]
pub struct WaveCues {
    /// `nSamplesPerSec` of the `fmt ` chunk.
    pub sample_rate: u32,
    /// The cue points in the chunk's own order.
    pub points: Vec<CuePoint>,
}

impl WaveCues {
    /// The marker times in seconds, **sorted by position**: marker `n` is the
    /// `n`th earliest cue point.
    ///
    /// The retail `c1-HA-m1_briefing.wav` stores its eight cue ids in an order
    /// that is not their time order (id 6 is the earliest, at 14.4 s), so the
    /// two readings of "marker n" — the cue with id `n + 1`, or the `n`th in
    /// time — differ. Only the time reading lets a script that waits for
    /// markers 0, 1, 2, … in order be played to the end of the voice-over (the
    /// last marker, at 94.5 s of 98.7 s, is where the script's 4.5 s final
    /// move ends with the sound); the reading is a stated choice, not a
    /// measurement of the original's code.
    #[must_use]
    pub fn marker_seconds(&self) -> Vec<f64> {
        let mut seconds: Vec<f64> = self
            .points
            .iter()
            .map(|point| f64::from(point.position) / f64::from(self.sample_rate.max(1)))
            .collect();
        seconds.sort_by(f64::total_cmp);
        seconds
    }
}

/// Reads the `fmt ` rate and the `cue ` points of a RIFF/WAVE member.
///
/// # Errors
///
/// [`BriefingError`] when the bytes are not RIFF/WAVE, hold no `fmt ` chunk or
/// a chunk runs past the end.
pub fn wave_cue_points(bytes: &[u8]) -> Result<WaveCues, BriefingError> {
    let scope = "wave cue points";
    let fail = |reason: &str| BriefingError::new(scope, reason.to_owned());
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(fail("not a RIFF/WAVE member"));
    }
    let word = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    };
    let mut rate = None;
    let mut points = Vec::new();
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = word(at + 4).ok_or_else(|| fail("a chunk header is cut off"))? as usize;
        let body = at + 8;
        let end = body
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| fail("a chunk runs past the end"))?;
        match id {
            b"fmt " => {
                rate = Some(word(body + 4).ok_or_else(|| fail("the fmt chunk is cut off"))?);
            }
            b"cue " => {
                let count = word(body).ok_or_else(|| fail("the cue chunk is cut off"))? as usize;
                for index in 0..count {
                    let entry = body + 4 + index * 24;
                    let (Some(cue_id), Some(position)) = (word(entry), word(entry + 4)) else {
                        return Err(fail("a cue point is cut off"));
                    };
                    if entry + 24 > end {
                        return Err(fail("a cue point runs past its chunk"));
                    }
                    points.push(CuePoint {
                        id: cue_id,
                        position,
                    });
                }
            }
            _ => {}
        }
        at = end + (size & 1);
    }
    Ok(WaveCues {
        sample_rate: rate.ok_or_else(|| fail("no fmt chunk"))?,
        points,
    })
}

/// A field of the `sounds.zrd` group table: the voice-over member name
/// (`c1-HA-m1_briefing.wav`) a script key (`brief_c61`) maps to.
///
/// The measured shape is `"brief_c61", [["briefing_c1m1",
/// "c1-HA-m1_briefing.wav", "PURGEABLE"]]`: the group holds one entry whose
/// first text is the sound's name and whose second is the archive member.
#[must_use]
pub fn voice_member_in(sounds: &ZrdValue, script_key: &str, sound_name: &str) -> Option<String> {
    let group = find_group(sounds, script_key)?;
    for entry in group.as_list()? {
        let parts = entry.as_list()?;
        if parts.first().and_then(ZrdValue::as_text) == Some(sound_name) {
            return parts.get(1).and_then(ZrdValue::as_text).map(str::to_owned);
        }
    }
    None
}

fn find_group<'a>(node: &'a ZrdValue, key: &str) -> Option<&'a ZrdValue> {
    let children = node.as_list()?;
    if let Some(found) = flat_field(children, key) {
        return Some(found);
    }
    children.iter().find_map(|child| find_group(child, key))
}
