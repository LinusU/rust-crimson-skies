//! The front end's music and click/rollover cues (FE-MENU-4).
//!
//! Spec: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
//! stages `### F45-B`/`### F45-D`; task FE-MENU-4.
//!
//! What the installation states, read from `ASSETS/SCRIPTS/GLOBALS.SCRIPT`
//! (the script every front-end screen shares):
//!
//! * six `sound` declarations in its `gui_create`, in this order:
//!   `mouseclick.wav`, `mouseover.wav`, `mouseclick.wav`, `mouseover.wav`,
//!   `entertext.wav`, `entertext_error.wav`, all under `assets\sounds\`
//!   (members of the screen container, `ASSETS/SOUNDS/*.WAV`);
//! * one `@ctl@SK` object — the music control — given `"music_splash.wav"`
//!   and a few numeric properties, then `initialize`d in the same
//!   `gui_create`, i.e. when the front end starts, not on any one screen.
//!   The passenger cabin mails it `11003`/`11004` (pause/continue).
//!
//! [`MenuSoundTable::parse`] reads those statements; nothing here is typed in.
//!
//! # What is **not** claimed
//!
//! * **Which control plays which declared sound, and when.** The scripts only
//!   declare the sounds; the playing is the executable's button and edit
//!   control classes. The cue each file serves is read from its **name**
//!   ([`MenuCue::file`]): `mouseover` on a button entering its rollover
//!   state, `mouseclick` on a press, `entertext` on an accepted character,
//!   `entertext_error` on a refused one. That is a reading of names, not a
//!   measured trigger: whether click sounds on press or on release, and what
//!   the second click/over pair (`FR`/`GR`) is for, stay unknown.
//! * **The music's loop count and level.** The control's numeric properties
//!   are kept verbatim ([`MenuMusic::properties`]) and unread; the music
//!   loops (a design choice: a menu track that ended into silence would be
//!   the odd reading) at unit gain. The screen scripts compute the level
//!   from engine settings `$$PR$$`/`$$CA$$` that are not mapped here.
//! * **Which copy of `music_splash.wav` the control opens.** The name carries
//!   no directory; the screen container holds one (2 312 360 bytes) and
//!   `ZBD/soundsh.zbd` another. The container's copy is played, the archive
//!   is the fallback.
//! * **Pause/continue on the cabin** — the cabin is not presented yet.

use std::collections::BTreeMap;

use cs_content::catalog::baseline::install_file_key;
use cs_formats::ParseContext;
use cs_formats::zbd::{SampleFormat, decode_sound_sample};
use cs_sim::audio_events::{
    AudioBus, AudioDevice, DeviceError, DeviceVoiceId, VoiceStart, VoiceStop,
};
use cs_types::content::{ContentId, ContentKind};

use super::original::script_member;
use super::retail::FrontEndScreens;
use crate::audio::{
    CapabilityDeclaration, FALLBACK_MUSIC_CONTAINER, InMemorySamples, PREFERRED_MUSIC_CONTAINER,
    PcmAudio, open_audible_device,
};

/// The section whose script declares the shared sounds.
pub const GLOBALS_SECTION: &str = "GLOBALS";

/// The screen container's directory the `sound` declarations name,
/// normalised (the script writes `assets\\sounds\\`).
pub const SOUND_DIRECTORY: &str = "ASSETS/SOUNDS";

/// The class the script gives the music control (`object RR = @ctl@SK`).
const MUSIC_CLASS: &str = "SK";

/// The level a cue or the music plays at; the data states none.
const UNIT_GAIN: f64 = 1.0;

/// A UI feedback cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MenuCue {
    /// A button enters its rollover state (pointer or focus).
    Rollover,
    /// A button is pressed.
    Press,
    /// The edit box accepts a typed character.
    TextAccepted,
    /// The edit box refuses a typed character (it is full).
    TextRejected,
}

impl MenuCue {
    /// Every cue, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Rollover,
        Self::Press,
        Self::TextAccepted,
        Self::TextRejected,
    ];

    /// The declared sound file this cue is read from (by name; see the
    /// module's "not claimed").
    #[must_use]
    pub const fn file(self) -> &'static str {
        match self {
            Self::Rollover => "mouseover.wav",
            Self::Press => "mouseclick.wav",
            Self::TextAccepted => "entertext.wav",
            Self::TextRejected => "entertext_error.wav",
        }
    }

    /// A stable label for reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rollover => "rollover",
            Self::Press => "press",
            Self::TextAccepted => "text_accepted",
            Self::TextRejected => "text_rejected",
        }
    }
}

/// One `sound X = "dir" "file"` declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredSound {
    /// The script variable (`DR`).
    pub variable: String,
    /// The file name as written (`mouseclick.wav`).
    pub file: String,
}

/// The music control as the script configures it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuMusic {
    /// The script variable the control is held in (`RR`).
    pub variable: String,
    /// The track file name as written (`music_splash.wav`).
    pub file: String,
    /// The control's other assignments, `(property, right-hand side)`,
    /// verbatim and unread.
    pub properties: Vec<(String, String)>,
}

/// Why `GLOBALS.SCRIPT` yielded no sound table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuSoundError {
    /// The script is not in the screen container.
    ScriptMissing,
    /// The script declares no `sound` at all.
    NoSounds,
    /// The script configures no music control with a track.
    NoMusic,
    /// A `sound` line does not have the two quoted halves.
    Malformed {
        /// The line, trimmed.
        line: String,
    },
}

impl std::fmt::Display for MenuSoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScriptMissing => write!(f, "{GLOBALS_SECTION}.SCRIPT is not in the container"),
            Self::NoSounds => f.write_str("the script declares no `sound`"),
            Self::NoMusic => f.write_str("the script configures no music track"),
            Self::Malformed { line } => write!(f, "unreadable sound declaration `{line}`"),
        }
    }
}

impl std::error::Error for MenuSoundError {}

/// The sound declarations of `GLOBALS.SCRIPT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSoundTable {
    /// The `sound` declarations in script order.
    pub sounds: Vec<DeclaredSound>,
    /// The music control.
    pub music: MenuMusic,
}

impl MenuSoundTable {
    /// Reads the table from the script's text.
    ///
    /// # Errors
    ///
    /// [`MenuSoundError`] when there is no `sound`, a declaration is
    /// unreadable, or no music track is configured.
    pub fn parse(text: &str) -> Result<Self, MenuSoundError> {
        let mut sounds = Vec::new();
        let mut music_variable: Option<String> = None;
        let mut assignments: Vec<(String, String, String)> = Vec::new();
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("sound ") {
                let (variable, value) =
                    rest.split_once('=')
                        .ok_or_else(|| MenuSoundError::Malformed {
                            line: line.to_owned(),
                        })?;
                let quoted: Vec<&str> = value.split('"').skip(1).step_by(2).collect();
                let [_directory, file] = quoted.as_slice() else {
                    return Err(MenuSoundError::Malformed {
                        line: line.to_owned(),
                    });
                };
                sounds.push(DeclaredSound {
                    variable: variable.trim().to_owned(),
                    file: (*file).to_owned(),
                });
            } else if let Some(rest) = line.strip_prefix("object ")
                && let Some((variable, class)) = rest.split_once('=')
                && class.trim() == format!("@ctl@{MUSIC_CLASS}")
            {
                music_variable = Some(variable.trim().to_owned());
            } else if let Some((target, value)) = line.split_once('=')
                && let Some((variable, property)) = target.trim().split_once('.')
            {
                assignments.push((
                    variable.to_owned(),
                    property.trim().to_owned(),
                    value.trim().to_owned(),
                ));
            }
        }
        if sounds.is_empty() {
            return Err(MenuSoundError::NoSounds);
        }
        let variable = music_variable.ok_or(MenuSoundError::NoMusic)?;
        let mut file = None;
        let mut properties = Vec::new();
        for (owner, property, value) in assignments {
            if owner != variable {
                continue;
            }
            match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                Some(name) if file.is_none() && name.to_ascii_lowercase().ends_with(".wav") => {
                    file = Some(name.to_owned());
                }
                _ => properties.push((property, value)),
            }
        }
        let file = file.ok_or(MenuSoundError::NoMusic)?;
        Ok(Self {
            sounds,
            music: MenuMusic {
                variable,
                file,
                properties,
            },
        })
    }

    /// Reads the table from the installation's `GLOBALS.SCRIPT`.
    ///
    /// # Errors
    ///
    /// [`MenuSoundError::ScriptMissing`] or the parse's own error.
    pub fn from_screens(screens: &FrontEndScreens) -> Result<Self, MenuSoundError> {
        let text = screens
            .script_text(&script_member(GLOBALS_SECTION))
            .ok_or(MenuSoundError::ScriptMissing)?;
        Self::parse(&text)
    }

    /// The declaration serving `cue`, when the script declares its file.
    #[must_use]
    pub fn declared(&self, cue: MenuCue) -> Option<&DeclaredSound> {
        self.sounds
            .iter()
            .find(|sound| sound.file.eq_ignore_ascii_case(cue.file()))
    }
}

/// The content id of a screen-container sound file.
///
/// # Errors
///
/// The id grammar's refusal.
pub fn cue_sample_id(file: &str) -> Result<ContentId, String> {
    ContentId::from_source(
        ContentKind::Sound,
        &install_file_key(&format!("{SOUND_DIRECTORY}/{}", file.to_ascii_uppercase())),
    )
    .map_err(|error| error.to_string())
}

/// The content id of the menu music track.
///
/// # Errors
///
/// The id grammar's refusal.
pub fn music_sample_id(file: &str) -> Result<ContentId, String> {
    ContentId::from_source(
        ContentKind::Music,
        &install_file_key(&format!("{SOUND_DIRECTORY}/{}", file.to_ascii_uppercase())),
    )
    .map_err(|error| error.to_string())
}

/// Decodes one stored RIFF/WAVE member with the production decoder.
///
/// # Errors
///
/// A message naming the step that refused.
pub fn decode_wave(name: &str, bytes: &[u8]) -> Result<PcmAudio, String> {
    let format =
        SampleFormat::from_member(bytes).map_err(|error| format!("{name}: header: {error}"))?;
    let mut context = ParseContext::with_defaults(name.to_owned());
    let decoded = decode_sound_sample(&mut context, bytes, &format)
        .map_err(|error| format!("{name}: decode: {error}"))?;
    PcmAudio::from_decoded(&decoded).map_err(|error| format!("{name}: samples: {error}"))
}

/// The decoded samples of a table, keyed by the ids [`MenuAudio`] plays.
#[derive(Debug, Default)]
pub struct MenuSamples {
    /// The decoded members.
    pub library: InMemorySamples,
    /// What could not be loaded, as `file: reason`.
    pub failures: Vec<String>,
    /// Where the music came from (`rof` or the ZBD container).
    pub music_source: Option<String>,
}

impl MenuSamples {
    /// Decodes every cue file and the music track the table names.
    #[must_use]
    pub fn load(screens: &FrontEndScreens, table: &MenuSoundTable) -> Self {
        let mut samples = Self::default();
        for cue in MenuCue::ALL {
            let Some(declared) = table.declared(cue) else {
                samples
                    .failures
                    .push(format!("{}: not declared by the script", cue.file()));
                continue;
            };
            let member = format!("{SOUND_DIRECTORY}/{}", declared.file);
            let loaded = screens
                .rof_member_bytes(&member)
                .ok_or_else(|| format!("{member}: not in the screen container"))
                .and_then(|bytes| decode_wave(&member, &bytes))
                .and_then(|pcm| Ok((cue_sample_id(&declared.file)?, pcm)));
            match loaded {
                Ok((id, pcm)) => samples.library.insert(id, pcm),
                Err(reason) => samples.failures.push(reason),
            }
        }
        let file = &table.music.file;
        let member = format!("{SOUND_DIRECTORY}/{file}");
        let from_rof = screens
            .rof_member_bytes(&member)
            .ok_or_else(|| format!("{member}: not in the screen container"))
            .and_then(|bytes| decode_wave(&member, &bytes))
            .map(|pcm| (pcm, "crimson.rof".to_owned()));
        let loaded = from_rof.or_else(|rof_reason| {
            [PREFERRED_MUSIC_CONTAINER, FALLBACK_MUSIC_CONTAINER]
                .into_iter()
                .find_map(|container| {
                    screens
                        .sound_member(container, file)
                        .ok()
                        .map(|member| (member.pcm, container.to_owned()))
                })
                .ok_or(rof_reason)
        });
        match loaded.and_then(|(pcm, source)| Ok((music_sample_id(file)?, pcm, source))) {
            Ok((id, pcm, source)) => {
                samples.library.insert(id, pcm);
                samples.music_source = Some(source);
            }
            Err(reason) => samples.failures.push(reason),
        }
        samples
    }
}

/// One thing the menu audio asked of its device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAudioEvent {
    /// The music loop started.
    MusicStarted,
    /// A cue played once.
    Cue(MenuCue),
    /// The device refused a command; the refusal's own code.
    Refused {
        /// What was being asked (`music`, or a cue label).
        what: String,
        /// The device error's stable code.
        code: &'static str,
    },
}

/// The front end's audio: the music loop and the UI cues, over any
/// [`AudioDevice`].
#[derive(Debug)]
pub struct MenuAudio {
    device: Box<dyn AudioDevice>,
    music: ContentId,
    cues: BTreeMap<MenuCue, ContentId>,
    music_voice: Option<DeviceVoiceId>,
    events: Vec<MenuAudioEvent>,
}

impl MenuAudio {
    /// Audio over `device` for `table`'s files, opening the device.
    ///
    /// # Errors
    ///
    /// The device's refusal to open, or an id the grammar refuses.
    pub fn new(
        mut device: Box<dyn AudioDevice>,
        table: &MenuSoundTable,
    ) -> Result<Self, DeviceError> {
        let id_error = |reason: String| DeviceError::new("menu_audio_identity", reason);
        let music = music_sample_id(&table.music.file).map_err(id_error)?;
        let mut cues = BTreeMap::new();
        for cue in MenuCue::ALL {
            if let Some(declared) = table.declared(cue) {
                cues.insert(cue, cue_sample_id(&declared.file).map_err(id_error)?);
            }
        }
        if !device.is_open() {
            device.open()?;
        }
        Ok(Self {
            device,
            music,
            cues,
            music_voice: None,
            events: Vec::new(),
        })
    }

    /// The real output, over the samples `samples` holds, when this machine
    /// declares `audio` and has an output device.
    ///
    /// # Errors
    ///
    /// The named refusal ([`CapabilityDeclaration`] gate or no device).
    pub fn audible(
        samples: MenuSamples,
        table: &MenuSoundTable,
        declaration: &CapabilityDeclaration,
    ) -> Result<Self, DeviceError> {
        let device = open_audible_device(std::sync::Arc::new(samples.library), declaration)?;
        Self::new(Box::new(device), table)
    }

    /// Starts the music loop; a no-op once it plays.
    pub fn start_music(&mut self) {
        if self.music_voice.is_some() {
            return;
        }
        let start = VoiceStart {
            asset: self.music.clone(),
            bus: AudioBus::Music,
            gain: UNIT_GAIN,
            pan: 0.0,
            pitch: 1.0,
        };
        match self.device.start_voice(start) {
            Ok(voice) => {
                self.music_voice = Some(voice);
                self.events.push(MenuAudioEvent::MusicStarted);
            }
            Err(error) => self.events.push(MenuAudioEvent::Refused {
                what: "music".to_owned(),
                code: error.code,
            }),
        }
    }

    /// Stops the music loop.
    pub fn stop_music(&mut self) {
        if let Some(voice) = self.music_voice.take() {
            let _ = self.device.stop_voice(VoiceStop {
                voice,
                reason: None,
            });
        }
    }

    /// Plays `cue` once.
    pub fn cue(&mut self, cue: MenuCue) {
        let Some(asset) = self.cues.get(&cue) else {
            self.events.push(MenuAudioEvent::Refused {
                what: cue.label().to_owned(),
                code: "cue_not_declared",
            });
            return;
        };
        let start = VoiceStart {
            asset: asset.clone(),
            bus: AudioBus::Ui,
            gain: UNIT_GAIN,
            pan: 0.0,
            pitch: 1.0,
        };
        match self.device.start_one_shot(start) {
            Ok(_) => self.events.push(MenuAudioEvent::Cue(cue)),
            Err(error) => self.events.push(MenuAudioEvent::Refused {
                what: cue.label().to_owned(),
                code: error.code,
            }),
        }
    }

    /// Whether the music loop is sounding.
    #[must_use]
    pub fn music_playing(&self) -> bool {
        self.music_voice.is_some()
    }

    /// Everything asked so far, in order.
    #[must_use]
    pub fn events(&self) -> &[MenuAudioEvent] {
        &self.events
    }

    /// How many times `cue` played.
    #[must_use]
    pub fn cue_count(&self, cue: MenuCue) -> usize {
        self.events
            .iter()
            .filter(|event| **event == MenuAudioEvent::Cue(cue))
            .count()
    }
}
