//! The Preferences → Audio page's model (task #1996): the `[@Audio@]`
//! section's checkbox `AP_B_MUSIC`, its three `Z` sliders, its `D` sound
//! quality drop-down and its labels, with the Accept / Cancel semantics of
//! `ASSETS/SCRIPTS/AUDIO.SCRIPT`.
//!
//! The panes and buttons draw through the [`super::MenuShell`] like every
//! other section; this page reads the rows the shell only counts
//! ([`super::widgets`]) and owns their behaviour. Nothing here renders: the
//! windowed presenter draws what [`AudioPage`] holds
//! ([`super::audio_view`]) and a headless drive reads the same model.
//!
//! # What the original data says (static readings, not `verified_original`)
//!
//! * **Rows.** `[@Audio@]` holds `AP_BACKGROUND` (a 566x366 pane whose five
//!   empty boxes on the left are the title plates the labels sit on),
//!   `AP_B_CANCELCHANGES` / `AP_B_ACCEPTCHANGES` (both `ScriptToExe`
//!   `Preferences`), `AP_B_MUSIC` (a `Type 1` check box,
//!   `PF_B_CheckBoxSmall.png`), eleven `T` rows, the drop-down
//!   `AP_D_SQuality` and the sliders `AP_S_MVOLUME`, `AP_S_EVOLUME`,
//!   `AP_S_VVOLUME` (1..=100, authored `CurrVal` 50).
//! * **Labels.** `IDS_AP_*` names, answered by the installation's
//!   `RESOURCE.H` ids and `langui.dll`: *AUDIO*, *In-Game Music*, *Music
//!   Volume*, *Effects Volume*, *Voice Volume*, *Sound Quality* and one
//!   description line each.
//! * **Script** (`AUDIO.SCRIPT`). `callback($$E$$, 2129, 0, …)` *reads* the
//!   five values (quality, the three volumes, music on) into the screen;
//!   the check box and sliders apply **live** (`setvolume(3|5, music)`,
//!   `setvolume(4|1, effects)`, `setvolume(2, voice)`, the music loop stops
//!   at off); **Cancel** runs `callback($$E$$, 2110)` and re-reads the stored
//!   values, putting the music back; **Accept** runs `callback($$E$$, 2129,
//!   1, …)` (*writes* the five values) and `callback($$A$$, 14)`. A changed
//!   quality while `$$HB$$` is set raises message 712 (*Crimson Skies must
//!   quit and restart for sound quality changes to take effect. Quit now?*).
//!   Both buttons then leave for `Preferences` through the layout's
//!   `ScriptToExe`.
//!
//! This page does the same: every change is **staged** and applied at once to
//! the front end's own music and cues ([`AudioValues::music_gain`] etc.),
//! Cancel restores what was last committed, and Accept commits and writes.
//!
//! # What is *not* known, and so not claimed
//!
//! * **Where the original keeps the values.** `callback 2129`'s storage is the
//!   engine's configuration (the registry keys `MusicOn`, `MusicVolume`,
//!   `SfxVolume`, `VoiceVolume`, `SoundLOD`,
//!   `docs/findings/2026-10-18-f52-original-options.md`), not read here. The
//!   values persist in **this project's own file**, [`AudioSettingsFile`].
//! * **The unset defaults.** What the original shows with nothing stored is
//!   unread. This page starts from the layout's authored slider value (50),
//!   the music **on** (the front end plays its music today) and quality
//!   **High**: designed defaults, recorded as such.
//! * **The drop-down's items.** `AB.TJ = 2132` and `@shareditems@XTA` supply
//!   them at run time. The page offers `IDS_LOW` and `IDS_HIGH` (strings 106
//!   and 107, which the README's *Sound Quality High/Low* option and the
//!   description *Low may improve performance* agree with), index 0 = Low,
//!   1 = High: an inference, and the index → engine `SoundLOD` mapping is
//!   unread.
//! * **How a volume maps to gain.** `setvolume(channel, 0..=100)` is an engine
//!   call; the page applies `value / 100` linearly — a design choice.
//! * **Voice volume and effects in a mission.** The front end plays no voice;
//!   the committed values are stored for the mission mixer (F52-W2, #781), and
//!   no mission consumes them yet.
//! * **Quality takes effect after a restart** only in the original; here the
//!   committed quality is stored and the page says so ([`AudioAccept`]); no
//!   quit prompt is drawn (the message box presenter is not built) and the
//!   menu keeps running.
//! * **Esc.** The script has no Esc handler; Esc here is Cancel, a design
//!   addition (the pilot page does the same).

use std::fmt;
use std::path::{Path, PathBuf};

use super::original::OriginalLayout;
use super::shell::ShellRect;
use super::widgets::{
    DropdownRow, SliderRow, TextRow, WidgetError, dropdown_rows, slider_rows, text_rows,
};

/// The layout section this page belongs to.
pub const AUDIO_SECTION: &str = "Audio";

/// The check box's row id (upper-cased, as the layout stores ids).
pub const MUSIC_BUTTON: &str = "AP_B_MUSIC";
/// The Accept button's row id.
pub const ACCEPT_BUTTON: &str = "AP_B_ACCEPTCHANGES";
/// The Cancel button's row id.
pub const CANCEL_BUTTON: &str = "AP_B_CANCELCHANGES";
/// The music volume slider's row id (`AUDIO.SCRIPT`: `FA`).
pub const MUSIC_SLIDER: &str = "AP_S_MVOLUME";
/// The effects volume slider's row id (`AUDIO.SCRIPT`: `GA`).
pub const EFFECTS_SLIDER: &str = "AP_S_EVOLUME";
/// The voice volume slider's row id (`AUDIO.SCRIPT`: `HA`).
pub const VOICE_SLIDER: &str = "AP_S_VVOLUME";
/// The sound quality drop-down's row id.
pub const QUALITY_DROPDOWN: &str = "AP_D_SQUALITY";

/// The name of the file the values persist in, inside the profile directory.
pub const SETTINGS_FILE_NAME: &str = "audio-settings.txt";

/// One step of a keyboard adjustment of a slider.
pub const KEY_STEP: i32 = 5;

/// The sound quality choices, in drop-down order (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundQuality {
    /// `IDS_LOW`.
    Low,
    /// `IDS_HIGH`.
    High,
}

impl SoundQuality {
    /// Both, in list order.
    pub const ALL: [Self; 2] = [Self::Low, Self::High];

    /// The `IDS_*` name of the item's label.
    #[must_use]
    pub const fn label_name(self) -> &'static str {
        match self {
            Self::Low => "IDS_LOW",
            Self::High => "IDS_HIGH",
        }
    }

    const fn word(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::High => "high",
        }
    }
}

/// The five values of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioValues {
    /// The in-game music check box.
    pub music_on: bool,
    /// Music volume, 1..=100.
    pub music_volume: i32,
    /// Effects volume, 1..=100.
    pub effects_volume: i32,
    /// Voice volume, 1..=100.
    pub voice_volume: i32,
    /// Sound quality.
    pub quality: SoundQuality,
}

impl AudioValues {
    /// The linear gain the front end's music plays at: silent when the check
    /// box is off, else `volume / 100`.
    #[must_use]
    pub fn music_gain(&self) -> f64 {
        if self.music_on {
            f64::from(self.music_volume) / 100.0
        } else {
            0.0
        }
    }

    /// The linear gain the front end's cues play at.
    #[must_use]
    pub fn effects_gain(&self) -> f64 {
        f64::from(self.effects_volume) / 100.0
    }
}

/// The project's own file for the committed values, one `Key=value` per line
/// under the original's value names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSettingsFile {
    path: PathBuf,
}

impl AudioSettingsFile {
    /// The file `SETTINGS_FILE_NAME` inside `directory`.
    #[must_use]
    pub fn in_directory(directory: &Path) -> Self {
        Self {
            path: directory.join(SETTINGS_FILE_NAME),
        }
    }

    /// The file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored values over `defaults`: a missing file or a missing or
    /// unusable line leaves the default (never an invented value).
    #[must_use]
    pub fn load(&self, defaults: AudioValues) -> AudioValues {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return defaults;
        };
        let mut values = defaults;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let volume = || value.parse::<i32>().ok().filter(|v| (1..=100).contains(v));
            match key {
                "MusicOn" => match value {
                    "0" => values.music_on = false,
                    "1" => values.music_on = true,
                    _ => {}
                },
                "MusicVolume" => values.music_volume = volume().unwrap_or(values.music_volume),
                "SfxVolume" => values.effects_volume = volume().unwrap_or(values.effects_volume),
                "VoiceVolume" => values.voice_volume = volume().unwrap_or(values.voice_volume),
                "SoundQuality" => {
                    if let Some(quality) = SoundQuality::ALL
                        .into_iter()
                        .find(|quality| quality.word() == value)
                    {
                        values.quality = quality;
                    }
                }
                _ => {}
            }
        }
        values
    }

    /// Writes `values` whole, through a temporary file so a failed write
    /// never leaves half of one.
    ///
    /// # Errors
    ///
    /// The I/O error text.
    pub fn save(&self, values: &AudioValues) -> Result<(), String> {
        let text = format!(
            "MusicOn={}\nMusicVolume={}\nSfxVolume={}\nVoiceVolume={}\nSoundQuality={}\n",
            u8::from(values.music_on),
            values.music_volume,
            values.effects_volume,
            values.voice_volume,
            values.quality.word(),
        );
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let temporary = self.path.with_extension("tmp");
        std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

/// The controls the page's keyboard focus visits, in drawn order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioControl {
    /// `AP_B_MUSIC`.
    Music,
    /// `AP_S_MVOLUME`.
    MusicVolume,
    /// `AP_S_EVOLUME`.
    EffectsVolume,
    /// `AP_S_VVOLUME`.
    VoiceVolume,
    /// `AP_D_SQuality`.
    Quality,
    /// `AP_B_CANCELCHANGES`.
    Cancel,
    /// `AP_B_ACCEPTCHANGES`.
    Accept,
}

impl AudioControl {
    /// All, in focus order.
    pub const ALL: [Self; 7] = [
        Self::Music,
        Self::MusicVolume,
        Self::EffectsVolume,
        Self::VoiceVolume,
        Self::Quality,
        Self::Cancel,
        Self::Accept,
    ];

    /// The shell button this control is, if it is one.
    #[must_use]
    pub const fn button(self) -> Option<&'static str> {
        match self {
            Self::Music => Some(MUSIC_BUTTON),
            Self::Cancel => Some(CANCEL_BUTTON),
            Self::Accept => Some(ACCEPT_BUTTON),
            _ => None,
        }
    }
}

/// One label ready to draw: the resolved text, its box and style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioLabel {
    /// The `T` row's id.
    pub id: String,
    /// The text, `[FONT]` tag stripped.
    pub text: String,
    /// The tag's pixel size when it names one (`COUR12B` → 12, `TREB9B` → 9):
    /// a reading of the face names, not of the font files (absent).
    pub size: Option<u32>,
    /// The box.
    pub rect: ShellRect,
    /// `0xAARRGGBB`.
    pub color: u32,
    /// 0 left, 1 centre, 2 right.
    pub justify: u32,
}

/// What an Accept did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioAccept {
    /// The values now committed.
    pub committed: AudioValues,
    /// The quality differs from the previous commit: the original asks to
    /// restart for it (message 712).
    pub quality_changed: bool,
    /// The write's refusal, when the file could not be written.
    pub save_error: Option<String>,
}

/// Why the page cannot be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioPageError {
    /// A widget row is unreadable.
    Widget(WidgetError),
    /// The section lacks a row the page needs.
    MissingRow(&'static str),
}

impl fmt::Display for AudioPageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Widget(error) => error.fmt(f),
            Self::MissingRow(id) => write!(f, "the Audio section has no {id} row"),
        }
    }
}

impl std::error::Error for AudioPageError {}

impl From<WidgetError> for AudioPageError {
    fn from(error: WidgetError) -> Self {
        Self::Widget(error)
    }
}

/// The page's live state.
#[derive(Clone, Debug)]
pub struct AudioPage {
    labels: Vec<AudioLabel>,
    sliders: Vec<SliderRow>,
    dropdown: DropdownRow,
    /// `(quality, resolved label)`; empty when the strings are not answered.
    items: Vec<(SoundQuality, String)>,
    committed: AudioValues,
    staged: AudioValues,
    focus: Option<AudioControl>,
    open: bool,
    drag: Option<AudioControl>,
    store: Option<AudioSettingsFile>,
    dirty: bool,
    accepts: Vec<AudioAccept>,
}

/// The designed defaults (module docs): the layout's authored `CurrVal`.
fn defaults(sliders: &[SliderRow]) -> AudioValues {
    let initial = |id: &str| {
        sliders
            .iter()
            .find(|slider| slider.id == id)
            .map_or(50, |slider| slider.initial)
    };
    AudioValues {
        music_on: true,
        music_volume: initial(MUSIC_SLIDER),
        effects_volume: initial(EFFECTS_SLIDER),
        voice_volume: initial(VOICE_SLIDER),
        quality: SoundQuality::High,
    }
}

fn split_tag(raw: &str) -> (Option<u32>, &str) {
    let Some(rest) = raw.strip_prefix('[') else {
        return (None, raw);
    };
    let Some((tag, text)) = rest.split_once(']') else {
        return (None, raw);
    };
    let digits: String = tag.chars().filter(char::is_ascii_digit).collect();
    (digits.parse().ok(), text)
}

impl AudioPage {
    /// Reads the `[@Audio@]` section's rows. `strings` answers an `IDS_*`
    /// name with the installation's text, `[FONT]` tag included; `measure`
    /// the extent of an art name; `store` is where Accept writes (and where
    /// the starting values are read from).
    ///
    /// # Errors
    ///
    /// [`AudioPageError`] for an unreadable row or a missing one.
    pub fn from_layout(
        layout: &OriginalLayout,
        strings: &dyn Fn(&str) -> Option<String>,
        measure: &dyn Fn(&str) -> Option<(u32, u32)>,
        store: Option<AudioSettingsFile>,
    ) -> Result<Self, AudioPageError> {
        let texts: Vec<TextRow> = text_rows(layout, AUDIO_SECTION)?;
        let sliders = slider_rows(layout, AUDIO_SECTION, measure)?;
        for id in [MUSIC_SLIDER, EFFECTS_SLIDER, VOICE_SLIDER] {
            if !sliders.iter().any(|slider| slider.id == id) {
                return Err(AudioPageError::MissingRow(id));
            }
        }
        let dropdown = dropdown_rows(layout, AUDIO_SECTION)?
            .into_iter()
            .find(|row| row.id == QUALITY_DROPDOWN)
            .ok_or(AudioPageError::MissingRow(QUALITY_DROPDOWN))?;
        let labels = texts
            .into_iter()
            .filter_map(|row| {
                let raw = strings(&row.name)?;
                let (size, text) = split_tag(&raw);
                Some(AudioLabel {
                    id: row.id,
                    text: text.to_owned(),
                    size,
                    rect: row.rect,
                    color: row.color,
                    justify: row.justify,
                })
            })
            .collect();
        let items: Vec<(SoundQuality, String)> = SoundQuality::ALL
            .into_iter()
            .filter_map(|quality| {
                let raw = strings(quality.label_name())?;
                Some((quality, split_tag(&raw).1.to_owned()))
            })
            .collect();
        let starting = defaults(&sliders);
        let committed = store
            .as_ref()
            .map_or(starting, |store| store.load(starting));
        Ok(Self {
            labels,
            sliders,
            dropdown,
            items: if items.len() == SoundQuality::ALL.len() {
                items
            } else {
                Vec::new()
            },
            committed,
            staged: committed,
            focus: None,
            open: false,
            drag: None,
            store,
            dirty: true,
            accepts: Vec::new(),
        })
    }

    /// The labels that resolved (an unanswered `IDS_*` name draws nothing).
    #[must_use]
    pub fn labels(&self) -> &[AudioLabel] {
        &self.labels
    }

    /// The slider rows.
    #[must_use]
    pub fn sliders(&self) -> &[SliderRow] {
        &self.sliders
    }

    /// The drop-down row.
    #[must_use]
    pub fn dropdown(&self) -> &DropdownRow {
        &self.dropdown
    }

    /// The drop-down's items `(quality, label)`; empty when the labels are
    /// unanswered, in which case the drop-down is inert.
    #[must_use]
    pub fn items(&self) -> &[(SoundQuality, String)] {
        &self.items
    }

    /// Whether the drop-down's list is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// The values on screen (staged, and applied live).
    #[must_use]
    pub const fn staged(&self) -> AudioValues {
        self.staged
    }

    /// The values last committed (or loaded).
    #[must_use]
    pub const fn committed(&self) -> AudioValues {
        self.committed
    }

    /// The focused control.
    #[must_use]
    pub const fn focus(&self) -> Option<AudioControl> {
        self.focus
    }

    /// What every Accept did, in order.
    #[must_use]
    pub fn accepts(&self) -> &[AudioAccept] {
        &self.accepts
    }

    /// Whether the drawn state changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// The rect of the list's `index`-th row when open: below the box, one box
    /// height each.
    #[must_use]
    pub fn item_rect(&self, index: usize) -> ShellRect {
        let box_rect = self.dropdown.rect;
        ShellRect {
            x: box_rect.x,
            y: box_rect.y + ((index as u32 + 1) * box_rect.height) as i32,
            width: box_rect.width,
            height: box_rect.height,
        }
    }

    fn slider_index(&self, control: AudioControl) -> Option<usize> {
        let id = match control {
            AudioControl::MusicVolume => MUSIC_SLIDER,
            AudioControl::EffectsVolume => EFFECTS_SLIDER,
            AudioControl::VoiceVolume => VOICE_SLIDER,
            _ => return None,
        };
        self.sliders.iter().position(|slider| slider.id == id)
    }

    /// The slider control row index of `control`.
    #[must_use]
    pub fn slider_of(&self, control: AudioControl) -> Option<&SliderRow> {
        self.slider_index(control).map(|index| &self.sliders[index])
    }

    /// The value of a volume control on screen.
    #[must_use]
    pub const fn volume(&self, control: AudioControl) -> Option<i32> {
        match control {
            AudioControl::MusicVolume => Some(self.staged.music_volume),
            AudioControl::EffectsVolume => Some(self.staged.effects_volume),
            AudioControl::VoiceVolume => Some(self.staged.voice_volume),
            _ => None,
        }
    }

    /// Stages a check box state.
    pub fn set_music_on(&mut self, on: bool) {
        if self.staged.music_on != on {
            self.staged.music_on = on;
            self.dirty = true;
        }
    }

    /// Stages a volume (clamped to the slider's range). `false` for a
    /// control that is not a volume.
    pub fn set_volume(&mut self, control: AudioControl, value: i32) -> bool {
        let Some(index) = self.slider_index(control) else {
            return false;
        };
        let value = value.clamp(self.sliders[index].min, self.sliders[index].max);
        let slot = match control {
            AudioControl::MusicVolume => &mut self.staged.music_volume,
            AudioControl::EffectsVolume => &mut self.staged.effects_volume,
            _ => &mut self.staged.voice_volume,
        };
        if *slot != value {
            *slot = value;
            self.dirty = true;
        }
        true
    }

    /// Stages a quality choice.
    pub fn set_quality(&mut self, quality: SoundQuality) {
        if self.staged.quality != quality {
            self.staged.quality = quality;
            self.dirty = true;
        }
    }

    /// The slider control whose slot the logical point is on.
    #[must_use]
    pub fn slider_at(&self, x: i32, y: i32) -> Option<AudioControl> {
        [
            AudioControl::MusicVolume,
            AudioControl::EffectsVolume,
            AudioControl::VoiceVolume,
        ]
        .into_iter()
        .find(|control| {
            self.slider_of(*control)
                .is_some_and(|slider| slider.hit_rect().contains(x, y))
        })
    }

    /// A press at a logical point on a slider grabs it and sets the value;
    /// returns whether the point was on one.
    pub fn begin_drag(&mut self, x: i32, y: i32) -> bool {
        let Some(control) = self.slider_at(x, y) else {
            return false;
        };
        self.drag = Some(control);
        self.focus = Some(control);
        self.open = false;
        self.drag_to(x);
        true
    }

    /// The held slider follows the pointer to logical `x`.
    pub fn drag_to(&mut self, x: i32) {
        if let Some(control) = self.drag
            && let Some(slider) = self.slider_of(control)
        {
            let value = slider.value_at(x);
            self.set_volume(control, value);
        }
    }

    /// The pointer's button came up.
    pub fn end_drag(&mut self) -> bool {
        self.drag.take().is_some()
    }

    /// Whether a slider is held.
    #[must_use]
    pub const fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// A press at a logical point on the drop-down: opens or closes the list,
    /// or picks the item under the point. Returns whether the press was the
    /// drop-down's (so the shell must not also see it).
    pub fn press_dropdown(&mut self, x: i32, y: i32) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if self.open {
            let picked = (0..self.items.len()).find(|index| self.item_rect(*index).contains(x, y));
            self.open = false;
            self.dirty = true;
            if let Some(index) = picked {
                let quality = self.items[index].0;
                self.set_quality(quality);
                return true;
            }
            return self.dropdown.rect.contains(x, y);
        }
        if self.dropdown.rect.contains(x, y) {
            self.open = true;
            self.focus = Some(AudioControl::Quality);
            self.dirty = true;
            return true;
        }
        false
    }

    /// Moves the page's focus through [`AudioControl::ALL`].
    pub fn move_focus(&mut self, forward: bool) -> AudioControl {
        let count = AudioControl::ALL.len();
        let at = self.focus.and_then(|focus| {
            AudioControl::ALL
                .iter()
                .position(|control| *control == focus)
        });
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(at), true) => (at + 1) % count,
            (Some(at), false) => (at + count - 1) % count,
        };
        self.focus = Some(AudioControl::ALL[next]);
        self.open = false;
        self.dirty = true;
        AudioControl::ALL[next]
    }

    /// Puts the focus on `control` (the shell's own focus landed on a button).
    pub fn set_focus(&mut self, control: Option<AudioControl>) {
        if self.focus != control {
            self.focus = control;
            self.dirty = true;
        }
    }

    /// Left / Right on the focused control: a slider moves by [`KEY_STEP`],
    /// the quality steps through its items. Returns whether anything moved.
    pub fn adjust(&mut self, direction: i32) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        if let Some(current) = self.volume(focus) {
            let before = self.staged;
            self.set_volume(focus, current + direction * KEY_STEP);
            return before != self.staged;
        }
        if focus == AudioControl::Quality && !self.items.is_empty() {
            let at = self
                .items
                .iter()
                .position(|(quality, _)| *quality == self.staged.quality)
                .unwrap_or(0) as i32;
            let next = (at + direction).clamp(0, self.items.len() as i32 - 1) as usize;
            let before = self.staged;
            self.set_quality(self.items[next].0);
            return before != self.staged;
        }
        false
    }

    /// Closes the drop-down's list; whether it was open.
    pub fn close_list(&mut self) -> bool {
        let was = self.open;
        if was {
            self.open = false;
            self.dirty = true;
        }
        was
    }

    /// Enter on the focused quality drop-down opens or closes its list.
    pub fn activate(&mut self) -> bool {
        if self.focus == Some(AudioControl::Quality) && !self.items.is_empty() {
            self.open = !self.open;
            self.dirty = true;
            return true;
        }
        false
    }

    /// Accept: commits the staged values and writes them.
    pub fn accept(&mut self) -> AudioAccept {
        let quality_changed = self.staged.quality != self.committed.quality;
        self.committed = self.staged;
        let save_error = self
            .store
            .as_ref()
            .and_then(|store| store.save(&self.committed).err());
        self.finish();
        let done = AudioAccept {
            committed: self.committed,
            quality_changed,
            save_error,
        };
        self.accepts.push(done.clone());
        done
    }

    /// Cancel: drops every staged change.
    pub fn cancel(&mut self) {
        self.staged = self.committed;
        self.finish();
    }

    fn finish(&mut self) {
        self.open = false;
        self.drag = None;
        self.focus = None;
        self.dirty = true;
    }
}
