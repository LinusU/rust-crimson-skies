//! The record kinds of the keyed field list members and the field lists
//! their own comments document (task #371, keyed `F12-I`).
//!
//! `ASSETS/LAYOUT.CSV` opens with comment lines that name the field list of
//! each record kind and `ASSETS/SCRAPBOOK.CSV`'s second comment line names
//! its own list. Those comments are a **documented source for the field
//! lists and nothing else**: they say nothing about a field's type, unit,
//! signedness or range. This module transcribes the documented lists —
//! names, order, which fields the comments bracket as optional and the
//! comments' own parenthetical notes — as inert metadata. It reads no
//! bytes, converts no value and decides no type; the typed schema that a
//! consumer accounts against is `cs_content::config`'s
//! [`RecordFieldSpec`](https://docs.rs/cs_content), which cites these lists.
//!
//! Every record of `LAYOUT.CSV` begins with a one-letter field naming its
//! kind, and ten of the eleven documented kinds appear (the sound object
//! does not). `SCRAPBOOK.CSV` has no record letter: its entries begin with
//! a number and it documents a single "Mission_Spread_Item" list. Whether
//! the original tells a `V`/`G` variable definition from an object record
//! by the key or by the first field not being a record letter is **not**
//! established from the data — the two rules agree on every observed line —
//! and is recorded as unknown in
//! `docs/findings/2026-09-29-f12-i-record-kind-schemas.md`.
//!
//! Only field-name identifiers and structural notes are reproduced here;
//! no line, prose or value of the original comments is committed (task #371
//! finding, "What is and is not committed").

/// One record kind of `ASSETS/LAYOUT.CSV`, identified by its record letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// `B` — button.
    Button,
    /// `P` — pane.
    Pane,
    /// `T` — text.
    Text,
    /// `E` — edit box.
    EditBox,
    /// `M` — movie.
    Movie,
    /// `A` — text list.
    TextList,
    /// `S` — scrolling text.
    ScrollingText,
    /// `D` — dropdown list.
    Dropdown,
    /// `L` — listbox.
    Listbox,
    /// `Z` — slider.
    Slider,
    /// `W` — sound object. Documented but **absent** from the shipped
    /// `LAYOUT.CSV`; no field of it was ever observed.
    SoundObject,
}

impl RecordKind {
    /// Every documented kind, in the order the comments name them.
    pub const ALL: [Self; 11] = [
        Self::Button,
        Self::Pane,
        Self::Text,
        Self::EditBox,
        Self::Movie,
        Self::TextList,
        Self::ScrollingText,
        Self::Dropdown,
        Self::Listbox,
        Self::Slider,
        Self::SoundObject,
    ];

    /// The record letter that selects this kind in a `LAYOUT.CSV` record.
    pub const fn letter(self) -> u8 {
        match self {
            Self::Button => b'B',
            Self::Pane => b'P',
            Self::Text => b'T',
            Self::EditBox => b'E',
            Self::Movie => b'M',
            Self::TextList => b'A',
            Self::ScrollingText => b'S',
            Self::Dropdown => b'D',
            Self::Listbox => b'L',
            Self::Slider => b'Z',
            Self::SoundObject => b'W',
        }
    }

    /// The kind a record letter selects, or `None` when no documented kind
    /// uses that byte.
    ///
    /// The comparison is exact: the shipped member spells every record
    /// letter in upper case, and no lower-case record letter exists to say
    /// what the original does with one.
    pub const fn from_letter(byte: u8) -> Option<Self> {
        match byte {
            b'B' => Some(Self::Button),
            b'P' => Some(Self::Pane),
            b'T' => Some(Self::Text),
            b'E' => Some(Self::EditBox),
            b'M' => Some(Self::Movie),
            b'A' => Some(Self::TextList),
            b'S' => Some(Self::ScrollingText),
            b'D' => Some(Self::Dropdown),
            b'L' => Some(Self::Listbox),
            b'Z' => Some(Self::Slider),
            b'W' => Some(Self::SoundObject),
            _ => None,
        }
    }

    /// The kind's name as the member's own comment names it.
    pub const fn documented_name(self) -> &'static str {
        match self {
            Self::Button => "Button",
            Self::Pane => "Pane",
            Self::Text => "Text",
            Self::EditBox => "EditBox",
            Self::Movie => "Movie",
            Self::TextList => "TextList",
            Self::ScrollingText => "Scrolling Text",
            Self::Dropdown => "Dropdown List",
            Self::Listbox => "Listbox",
            Self::Slider => "Slider",
            Self::SoundObject => "Sound Object",
        }
    }
}

/// One field a record kind's comment names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentedField {
    /// The field name as the comment spells it. A parenthetical instead of a
    /// name, e.g. the button-type enum, is kept as written and explained by
    /// [`Self::note`].
    pub name: &'static str,
    /// Whether the comment brackets the field as optional (`[a,b,c]`).
    pub optional: bool,
    /// The comment's own parenthetical about the field, empty when it gives
    /// none. Never a type: the comments do not state types.
    pub note: &'static str,
}

const fn field(name: &'static str, optional: bool, note: &'static str) -> DocumentedField {
    DocumentedField {
        name,
        optional,
        note,
    }
}

/// `B`'s documented field list, including the four optional colours.
static BUTTON_FIELDS: [DocumentedField; 22] = [
    field("ID", false, "the record letter B is this field's value"),
    field("ArtPath", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("TabOrder", false, ""),
    field("ResID", false, ""),
    field("HelpID", false, ""),
    field("ScriptToExe", false, ""),
    field("ScriptPri", false, ""),
    field("EndScript?", false, "'?' means a bool"),
    field("Left", false, ""),
    field("Top", false, ""),
    field("Right", false, ""),
    field("Bottom", false, ""),
    field(
        "(0=Normal,1=Check,2=Radio)",
        false,
        "unnamed button-type value enum",
    ),
    field("Checked?", false, "'?' means a bool"),
    field(
        "ColorDisabled",
        true,
        "colors should only be specified if ResID is specified",
    ),
    field(
        "ColorActive",
        true,
        "colors should only be specified if ResID is specified",
    ),
    field(
        "ColorRollover",
        true,
        "colors should only be specified if ResID is specified",
    ),
    field(
        "ColorDepressed",
        true,
        "colors should only be specified if ResID is specified",
    ),
    field(
        "Group",
        false,
        "documented as radio button group, not working",
    ),
];

/// `P`'s documented field list.
static PANE_FIELDS: [DocumentedField; 10] = [
    field("ID", false, ""),
    field("ArtPath", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("NUMFRAMES", false, ""),
    field("IsRegion?", false, "'?' means a bool"),
    field("AlphaType", false, "Solid=0, Alpha=1, ColorKey=2"),
    field("Volatile?", false, "'?' means a bool"),
    field("HelpID", false, ""),
];

/// `T`'s documented field list.
static TEXT_FIELDS: [DocumentedField; 9] = [
    field("ID", false, ""),
    field("ResID", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("Width", false, "0 means auto-size"),
    field("Height", false, "0 means auto-size"),
    field("Color", false, ""),
    field("Justify", false, "0=left, 1=center, 2=right, 3=all"),
];

/// `E`'s documented field list.
static EDIT_BOX_FIELDS: [DocumentedField; 13] = [
    field("ID", false, ""),
    field("FontID", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("Width", false, ""),
    field("Height", false, "0 means auto-size"),
    field("MaxChars", false, ""),
    field("HelpID", false, ""),
    field("TabOrder", false, ""),
    field("TexTCOLOR", false, "0 means use default"),
    field("FrameColor", false, "0 means use default"),
    field("CursorColor", false, "0 means use default"),
];

/// `M`'s documented field list.
static MOVIE_FIELDS: [DocumentedField; 9] = [
    field("ID", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("ScaleX", false, ""),
    field("ScaleY", false, ""),
    field("# Loops", false, ""),
    field("Region?", false, "'?' means a bool"),
    field("HelpID", false, ""),
];

/// `A`'s documented field list.
static TEXT_LIST_FIELDS: [DocumentedField; 10] = [
    field("ID", false, ""),
    field("HelpID", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("Width", false, ""),
    field("Height", false, ""),
    field("TexTCOLOR", false, ""),
    field("Justify", false, "0=left, 1=center, 2=right, 3=all"),
    field("ItemSpacing", false, ""),
];

/// `S`'s documented field list.
static SCROLLING_TEXT_FIELDS: [DocumentedField; 14] = [
    field("ID", false, ""),
    field("BorderColor", false, ""),
    field("BackColor", false, ""),
    field("Slider", false, ""),
    field("UpArrow", false, ""),
    field("DownArrow", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("Width", false, ""),
    field("Height", false, ""),
    field("TabOrder", false, ""),
    field("ResID", false, ""),
    field("Color", false, ""),
];

/// `D`'s documented field list.
static DROPDOWN_FIELDS: [DocumentedField; 14] = [
    field("ID", false, ""),
    field("Slider", false, ""),
    field("UpArrow", false, ""),
    field("DownArrow", false, ""),
    field("DropUp", false, ""),
    field("DropDown", false, ""),
    field("ScriptPointer", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("Width", false, ""),
    field("Height", false, ""),
    field("TotalDisplayed", false, ""),
    field("TabOrder", false, ""),
];

/// `L`'s documented field list.
static LISTBOX_FIELDS: [DocumentedField; 12] = [
    field("ID", false, ""),
    field("Slider", false, ""),
    field("UpArrow", false, ""),
    field("DownArrow", false, ""),
    field("ScriptPointer", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("Width", false, ""),
    field("Height", false, ""),
    field("TotalDisplayed", false, ""),
    field("TabOrder", false, ""),
];

/// `Z`'s documented field list.
static SLIDER_FIELDS: [DocumentedField; 13] = [
    field("ID", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Z", false, ""),
    field("MinVal", false, ""),
    field("MaxVal", false, ""),
    field("CurrVal", false, ""),
    field("RegionFile", false, ""),
    field("SliderFile", false, ""),
    field("Left", false, ""),
    field("Top", false, ""),
    field("Right", false, ""),
    field("Bottom", false, ""),
];

/// `W`'s documented field list. The kind does not appear in the shipped
/// member, so every field of it stays unobserved.
static SOUND_OBJECT_FIELDS: [DocumentedField; 6] = [
    field("ID", false, ""),
    field("WAVFileName", false, ""),
    field("Channel", false, ""),
    field("Volume", false, ""),
    field("LoopCount (0=continuous)", false, "0=continuous"),
    field("Autostart?", false, "'?' means a bool"),
];

/// The one documented field list of `ASSETS/SCRAPBOOK.CSV`, named
/// `Mission_Spread_Item` by the member's own comment.
///
/// It has **sixteen** fields; the task record and the survey prose that call
/// it "seventeen" count the bracketed `Left,Top,Right,Bottom` group as four
/// names rather than one quoted field. The member's data is decisive: every
/// one of its 461 entries has sixteen fields, the eleventh a quoted
/// `"a,b,c,d"`. The discrepancy is recorded in
/// `docs/findings/2026-09-29-f12-i-record-kind-schemas.md`.
static SCRAPBOOK_FIELDS: [DocumentedField; 16] = [
    field("Objective", false, ""),
    field("ResourceID", false, ""),
    field("ImageName", false, ""),
    field("ImageType", false, ""),
    field("X", false, ""),
    field("Y", false, ""),
    field("Alpha", false, ""),
    field("Width", false, ""),
    field("Height", false, ""),
    field("DrawOrder", false, ""),
    field(
        "Left,Top,Right,Bottom",
        false,
        "one quoted field holding four comma-separated numbers",
    ),
    field("Zoom", false, ""),
    field("ZoomX", false, ""),
    field("ZoomY", false, ""),
    field("TitleResID", false, ""),
    field("TextResID", false, ""),
];

/// The number of fields the comment's bracket marks optional, per kind. Only
/// the button's four colours are bracketed.
pub const fn optional_field_count(kind: RecordKind) -> usize {
    match kind {
        RecordKind::Button => 4,
        _ => 0,
    }
}

/// The documented field list of a `LAYOUT.CSV` record kind, in the order the
/// member's comment names the fields.
pub fn documented_fields(kind: RecordKind) -> &'static [DocumentedField] {
    match kind {
        RecordKind::Button => &BUTTON_FIELDS,
        RecordKind::Pane => &PANE_FIELDS,
        RecordKind::Text => &TEXT_FIELDS,
        RecordKind::EditBox => &EDIT_BOX_FIELDS,
        RecordKind::Movie => &MOVIE_FIELDS,
        RecordKind::TextList => &TEXT_LIST_FIELDS,
        RecordKind::ScrollingText => &SCROLLING_TEXT_FIELDS,
        RecordKind::Dropdown => &DROPDOWN_FIELDS,
        RecordKind::Listbox => &LISTBOX_FIELDS,
        RecordKind::Slider => &SLIDER_FIELDS,
        RecordKind::SoundObject => &SOUND_OBJECT_FIELDS,
    }
}

/// The documented `Mission_Spread_Item` field list of `SCRAPBOOK.CSV`.
pub fn documented_scrapbook_fields() -> &'static [DocumentedField] {
    &SCRAPBOOK_FIELDS
}

/// The member's own note that a `?` suffix on a field name means the field
/// is a boolean.
pub const BOOL_MARKER_NOTE: &str = "'?' means a bool";

/// The member's own note that a button's colours are only meaningful when
/// the button also names a `ResID`.
pub const BUTTON_COLOR_NOTE: &str = "colors should only be specified if ResID is specified";
