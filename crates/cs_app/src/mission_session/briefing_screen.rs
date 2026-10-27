//! The windowed briefing map screen (task #1750
//! `PARITY5-FRONTEND-BRIEFING-MAP`): the original's `Briefing.zrd` script
//! drawn on its map picture while its voice-over plays, in front of the
//! Flight Check.
//!
//! * [`read_briefing`] reads everything from the installation: the dialog and
//!   the mission's script from `Briefing.zrd`, the script's bitmaps from
//!   `ZBD/rimage.zbd`, the voice-over member (named through `sounds.zrd`) from
//!   the sound archive, and the voice-over's cue points, which are the
//!   markers the script waits for.
//! * [`BriefingVoice`] plays the voice-over through the production audible
//!   device ([`crate::audio::AudibleDevice`]) and reports how far the output
//!   stream pulled it; the briefing's clock follows that position, so a
//!   marker falls where the sound is. With no device (a machine without the
//!   `audio` capability) the clock runs on the wall clock and
//!   [`BriefingVoice::refusal`] names why.
//! * [`BriefingScreenPlugin`] spawns the screen when the phase's stage is the
//!   briefing, animates it from the compiled timeline every frame and takes
//!   the pointer for the three buttons' rollover; the buttons themselves
//!   are pressed by [`super::flight_check::FlightCheckPhase`]'s own input
//!   system, which drives the front-end machine.
//!
//! The readings of the script's numbers and the stand-ins for the unread
//! fonts and cursor are listed in `crate::ui::front_end::briefing` and in
//! `docs/findings/2026-10-21-parity5-briefing-map.md`. Nothing here is
//! `verified_original`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::math::Rot2;
use bevy::prelude::{
    App, BackgroundColor, Color, Commands, Component, Display, Entity, GlobalZIndex, Image,
    ImageNode, Node, Plugin, PositionType, Query, Res, ResMut, Resource, Text, TextColor, TextFont,
    Update, Val, Vec2, Visibility, Window, With, Without, default,
};
use bevy::text::{FontSize, Justify, LineBreak, TextLayout};
use bevy::time::{Real, Time};
use bevy::ui::UiTransform;
use cs_content::briefing::{
    BRIEFING_ARCHIVE, BRIEFING_MEMBER, BriefingDocument, DialogButtonKind, SOUND_GROUPS_MEMBER,
    sound_name_for, voice_member_in, wave_cue_points,
};
use cs_content::campaign_bindings::SourceContext;
use cs_content::stunts::decode_zrd;
use cs_sim::audio_events::{
    AudioBus, AudioDevice, DeviceError, DeviceVoiceId, VoiceStart, VoiceStop,
};
use cs_types::content::ContentId;

use super::flight_check::{FlightCheckPhase, PreMissionStage, argb, to_image};
use super::launch_sound::{member_content_id, reader_member};
use crate::audio::{
    AudibleDevice, CapabilityDeclaration, InMemorySamples, PcmAudio, SampleLibrary, SampleProbe,
    open_audible_device,
};
use crate::ui::front_end::{
    Artwork, BriefingScene, BriefingSceneError, ElementKind, FrontEndScreens, PANEL_ELEMENT,
};

/// The sound archive the voice-over is read from: the launch sound's labelled
/// choice, because which of `soundsl.zbd` and `soundsh.zbd` the original opens
/// is unmeasured (F14-D.7). The two hold the same cue points in time.
pub const VOICE_CONTAINER: &str = "ZBD/soundsh.zbd";

/// The designed size of an objective line, in logical pixels.
const LIST_FONT_PX: f32 = 13.0;
/// The designed size of the panel's title, in logical pixels.
const TITLE_FONT_PX: f32 = 16.0;
/// The designed size of a button's label, in logical pixels.
const LABEL_FONT_PX: f32 = 14.0;
/// The thickness of a drawn line, in logical pixels.
const LINE_THICKNESS: f32 = 2.0;
/// The designed colours: normal, under the pointer and pressed. The record
/// names three fonts for these states; the fonts are unread, the colours are
/// read off the owner's capture (white; red under the pointer).
const LABEL_NORMAL: u32 = 0xFFFF_F4D6;
const LABEL_ROLLOVER: u32 = 0xFFD6_2820;
const LABEL_ACTIVATE: u32 = 0xFFA0_1414;
/// The designed colour of the objective text on the parchment.
const LIST_COLOR: u32 = 0xFF2A_1C0C;

// ------------------------------------------------------------- the reader --

/// Why the briefing could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum BriefingReadError {
    /// The campaign layout does not place the mission.
    Campaign(String),
    /// `Briefing.zrd` could not be read or decoded.
    Document(String),
    /// The record holds no script for the mission's campaign position.
    NoScript(String),
    /// `sounds.zrd` names no voice-over member for the script.
    NoVoice(String),
    /// The voice-over member could not be read or decoded.
    Voice(String),
    /// A bitmap the script draws could not be read.
    Art(String),
    /// The scene could not be built.
    Scene(BriefingSceneError),
}

impl fmt::Display for BriefingReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Campaign(reason) => write!(f, "the briefing cannot be placed: {reason}"),
            Self::Document(reason) => write!(f, "{BRIEFING_MEMBER} cannot be read: {reason}"),
            Self::NoScript(reason) => write!(f, "the mission has no briefing script: {reason}"),
            Self::NoVoice(reason) => write!(f, "the briefing has no voice-over: {reason}"),
            Self::Voice(reason) => write!(f, "the voice-over cannot be read: {reason}"),
            Self::Art(reason) => write!(f, "a briefing bitmap cannot be read: {reason}"),
            Self::Scene(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BriefingReadError {}

/// The decoded bitmaps the screen draws, by lower-cased name.
#[derive(Clone, Debug, Default, Resource)]
pub struct BriefingArtwork {
    images: BTreeMap<String, Artwork>,
}

impl BriefingArtwork {
    /// One bitmap, matched ASCII case-insensitively.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Artwork> {
        self.images.get(&name.to_ascii_lowercase())
    }

    /// How many bitmaps are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether no bitmap is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// The voice-over member the briefing plays.
#[derive(Clone, Debug)]
pub struct BriefingVoiceAsset {
    /// The member's name (`c1-HA-m1_briefing.wav`).
    pub member: String,
    /// The archive it was read from.
    pub container: String,
    /// The decoded samples.
    pub pcm: PcmAudio,
    /// The member's stable content id.
    pub content: ContentId,
    /// The `cue ` points as `(id, seconds)` in the chunk's own order.
    pub cues: Vec<(u32, f64)>,
}

/// Everything [`read_briefing`] read.
#[derive(Clone, Debug)]
pub struct BriefingSource {
    /// What the screen draws.
    pub scene: BriefingScene,
    /// The decoded bitmaps.
    pub artwork: BriefingArtwork,
    /// The voice-over.
    pub voice: BriefingVoiceAsset,
}

/// Reads the briefing of one mission.
///
/// `mission_dir` is the mission archive's logical key (`zbd/c3/m01`);
/// `strings` resolves the dialog's string ids (`MSG_BTN_REPLAY_BRIEFING`);
/// `objectives` are the mission's objective lines.
///
/// # Errors
///
/// [`BriefingReadError`] from the first stage that fails; nothing is replaced
/// by a stand-in.
pub fn read_briefing(
    install_root: &Path,
    mission_dir: &str,
    screens: &FrontEndScreens,
    strings: &dyn Fn(&str) -> Option<String>,
    objectives: Vec<String>,
) -> Result<BriefingSource, BriefingReadError> {
    let context = SourceContext::read(install_root)
        .map_err(|error| BriefingReadError::Campaign(error.to_string()))?;
    let wanted = format!("{}/", mission_dir.to_ascii_lowercase());
    let position = context
        .campaign()
        .iter()
        .find(|entry| {
            entry
                .program_asset
                .to_ascii_lowercase()
                .starts_with(&wanted)
        })
        .ok_or_else(|| {
            BriefingReadError::Campaign(format!("{mission_dir} is not in the campaign layout"))
        })?;
    let sound = sound_name_for(position.chapter, position.mission_number);

    let bytes = reader_member(install_root, BRIEFING_ARCHIVE, BRIEFING_MEMBER)
        .map_err(BriefingReadError::Document)?;
    let document = BriefingDocument::decode(&bytes)
        .map_err(|error| BriefingReadError::Document(error.to_string()))?;
    let script = document.script_for_sound(&sound).ok_or_else(|| {
        BriefingReadError::NoScript(format!(
            "{BRIEFING_MEMBER} has no script playing {sound} (campaign position chapter {} \
             mission {})",
            position.chapter, position.mission_number
        ))
    })?;

    let groups = decode_zrd(
        &reader_member(install_root, BRIEFING_ARCHIVE, SOUND_GROUPS_MEMBER)
            .map_err(BriefingReadError::Voice)?,
    )
    .map_err(|error| BriefingReadError::Voice(format!("{SOUND_GROUPS_MEMBER}: {error}")))?;
    let member = voice_member_in(&groups, &script.key, &sound).ok_or_else(|| {
        BriefingReadError::NoVoice(format!(
            "{SOUND_GROUPS_MEMBER} maps {} to no member named {sound}",
            script.key
        ))
    })?;
    let read = screens
        .sound_member(VOICE_CONTAINER, &member)
        .map_err(|error| BriefingReadError::Voice(error.to_string()))?;
    let cues = wave_cue_points(&read.bytes)
        .map_err(|error| BriefingReadError::Voice(error.to_string()))?;
    let markers = cues.marker_seconds();
    let rate = f64::from(cues.sample_rate.max(1));
    let voice = BriefingVoiceAsset {
        content: member_content_id(VOICE_CONTAINER, &read.name)
            .map_err(|error| BriefingReadError::Voice(error.to_string()))?,
        member: read.name,
        container: VOICE_CONTAINER.to_owned(),
        pcm: read.pcm,
        cues: cues
            .points
            .iter()
            .map(|point| (point.id, f64::from(point.position) / rate))
            .collect(),
    };

    let mut names: Vec<String> = script
        .bitmaps()
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let dialog = &document.dialog;
    for name in std::iter::once(&dialog.objectives.background_bitmap)
        .chain(dialog.buttons.iter().map(|button| &button.bitmap))
    {
        if !names.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            names.push(name.clone());
        }
    }
    let mut images = BTreeMap::new();
    for name in &names {
        let art = screens
            .ui_artwork(name)
            .map_err(|error| BriefingReadError::Art(error.to_string()))?;
        images.insert(name.to_ascii_lowercase(), art);
    }
    let artwork = BriefingArtwork { images };
    let scene = BriefingScene::build(
        dialog,
        script,
        &markers,
        &|name| artwork.get(name).map(|art| (art.width(), art.height())),
        strings,
        objectives,
    )
    .map_err(BriefingReadError::Scene)?;
    Ok(BriefingSource {
        scene,
        artwork,
        voice,
    })
}

// -------------------------------------------------------------- the voice --

/// The voice-over's player: one audible device, one voice at a time.
#[derive(Resource)]
pub struct BriefingVoice {
    asset: BriefingVoiceAsset,
    library: Arc<InMemorySamples>,
    probe: Arc<SampleProbe>,
    device: Option<AudibleDevice>,
    voice: Option<DeviceVoiceId>,
    refusal: Option<DeviceError>,
    generation: Option<u32>,
    starts: u32,
    volume: f64,
}

impl BriefingVoice {
    /// A player for `asset` at the script's `vol`.
    #[must_use]
    pub fn new(asset: BriefingVoiceAsset, volume: f32) -> Self {
        let library = Arc::new(InMemorySamples::new());
        library.insert_sample(asset.content.clone(), asset.pcm.clone());
        Self {
            asset,
            library,
            probe: Arc::new(SampleProbe::new()),
            device: None,
            voice: None,
            refusal: None,
            generation: None,
            starts: 0,
            volume: f64::from(volume),
        }
    }

    /// The voice-over member this player holds.
    #[must_use]
    pub fn asset(&self) -> &BriefingVoiceAsset {
        &self.asset
    }

    /// The tap on what the output stream pulled.
    #[must_use]
    pub fn probe(&self) -> &Arc<SampleProbe> {
        &self.probe
    }

    /// The named refusal, when the device would not open or start the voice.
    #[must_use]
    pub fn refusal(&self) -> Option<&DeviceError> {
        self.refusal.as_ref()
    }

    /// How many times a voice was started (the first play and every replay).
    #[must_use]
    pub const fn starts(&self) -> u32 {
        self.starts
    }

    /// Whether a voice is sounding.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.voice.is_some()
    }

    /// Starts the voice-over from its beginning, replacing a sounding one.
    pub fn start(&mut self) {
        self.stop();
        if self.device.is_none() {
            match open_audible_device(
                self.library.clone() as Arc<dyn SampleLibrary>,
                &CapabilityDeclaration::from_environment(),
            ) {
                Ok(device) => self.device = Some(device.with_probe(self.probe.clone())),
                Err(error) => {
                    self.refusal = Some(error);
                    return;
                }
            }
        }
        let Some(device) = self.device.as_mut() else {
            return;
        };
        let started = device.start_one_shot(VoiceStart {
            asset: self.asset.content.clone(),
            // The original's bus for the briefing voice is unread (F41); the
            // device applies no bus fader.
            bus: AudioBus::Radio,
            gain: self.volume,
            pan: 0.0,
            pitch: 1.0,
        });
        match started {
            Ok(voice) => {
                self.voice = Some(voice);
                self.starts += 1;
                self.refusal = None;
            }
            Err(error) => self.refusal = Some(error),
        }
    }

    /// Stops a sounding voice.
    pub fn stop(&mut self) {
        if let (Some(device), Some(voice)) = (self.device.as_mut(), self.voice.take()) {
            let _ = device.stop_voice(VoiceStop {
                voice,
                reason: None,
            });
        }
    }

    /// Stops the voice and releases the output stream.
    pub fn close(&mut self) {
        self.stop();
        if let Some(mut device) = self.device.take() {
            device.close();
        }
        self.generation = None;
    }

    /// How far the output stream pulled the voice, in seconds.
    #[must_use]
    pub fn played_seconds(&self) -> Option<f64> {
        let position = self.device.as_ref()?.played_position(self.voice?)?;
        Some(position.as_secs_f64())
    }
}

/// How the briefing's clock is driven.
#[derive(Clone, Copy, Debug, Resource)]
pub struct BriefingDriver {
    /// Follow the voice's played position when a voice is sounding.
    pub follow_voice: bool,
    /// Advance by this many seconds per frame instead of the wall clock (the
    /// owner-command smoke steps its frames at 1/60 s).
    pub fixed_step: Option<f64>,
}

impl Default for BriefingDriver {
    fn default() -> Self {
        Self {
            follow_voice: true,
            fixed_step: None,
        }
    }
}

// ------------------------------------------------------------ the screen --

/// Installs the briefing screen over a pending phase that has a briefing.
pub fn install_briefing_screen(app: &mut App, source: BriefingSource) {
    let volume = source.scene.sound.volume;
    app.insert_resource(source.artwork);
    app.insert_resource(BriefingVoice::new(source.voice, volume));
    app.init_resource::<BriefingDriver>();
    app.add_plugins(BriefingScreenPlugin);
}

/// The window's face of the briefing.
#[derive(Clone, Copy, Debug, Default)]
pub struct BriefingScreenPlugin;

impl Plugin for BriefingScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BriefingShown>().add_systems(
            Update,
            (
                sync_briefing_screen,
                drive_briefing,
                animate_pictures,
                animate_lines,
                animate_panel,
                place_static_parts,
                animate_buttons,
                show_briefing_notice,
            )
                .chain(),
        );
    }
}

/// Whether the briefing's parts are spawned.
#[derive(Resource, Default)]
struct BriefingShown(bool);

/// Everything the briefing spawns.
#[derive(Component)]
struct BriefingPart;

/// A picture element of the script.
#[derive(Component)]
struct PictureElement {
    id: String,
    size: Vec2,
}

/// A line element of the script.
#[derive(Component)]
struct LineElement {
    id: String,
}

/// A static part: its logical region.
#[derive(Component, Clone, Copy)]
struct Placed {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Which static part of the panel.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum PanelPart {
    Parchment,
    Title,
    List,
}

/// One objective row of the list.
#[derive(Component)]
struct ObjectiveRow {
    index: u32,
}

/// A button's picture.
#[derive(Component)]
struct ButtonArt {
    kind: DialogButtonKind,
}

/// A button's label.
#[derive(Component)]
struct ButtonLabel {
    kind: DialogButtonKind,
}

/// The notice line.
#[derive(Component)]
struct NoticeLine;

/// The mapping from the logical 800x600 space to window pixels.
#[derive(Clone, Copy)]
struct Fit {
    scale: f32,
    left: f32,
    top: f32,
}

impl Fit {
    fn of(window: &Window, size: (u32, u32)) -> Option<Self> {
        let surface = (window.width() as u32, window.height() as u32);
        let fit = cs_content::ui_layout::AspectFit::new(size, surface)?;
        let rect = fit.image_rect();
        Some(Self {
            scale: rect.width as f32 / size.0 as f32,
            left: rect.x as f32,
            top: rect.y as f32,
        })
    }

    fn px(&self, logical: f32) -> Val {
        Val::Px(logical * self.scale)
    }
}

const BASE_Z: i32 = 2100;

fn to_logical_color(rgba: u32, alpha: f32) -> Color {
    let [a, r, g, b] = rgba.to_be_bytes();
    Color::srgba_u8(r, g, b, (f32::from(a) * alpha.clamp(0.0, 1.0)) as u8)
}

fn absolute() -> Node {
    Node {
        position_type: PositionType::Absolute,
        ..default()
    }
}

#[allow(clippy::too_many_lines)]
fn spawn_briefing(
    commands: &mut Commands,
    phase: &FlightCheckPhase,
    artwork: &BriefingArtwork,
    images: &mut bevy::prelude::Assets<Image>,
) {
    let Some(stage) = phase.briefing() else {
        return;
    };
    let scene = stage.scene();
    let image_of = |name: &str, images: &mut bevy::prelude::Assets<Image>| {
        artwork.get(name).map(|art| {
            (
                images.add(to_image(art)),
                Vec2::new(art.width() as f32, art.height() as f32),
            )
        })
    };
    let full = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    };
    commands.spawn((
        full,
        BackgroundColor(Color::BLACK),
        GlobalZIndex(2000),
        BriefingPart,
    ));
    for image in &scene.background {
        if let Some((handle, size)) = image_of(&image.bitmap, images) {
            commands.spawn((
                absolute(),
                ImageNode::new(handle),
                Placed {
                    x: image.at.x as f32,
                    y: image.at.y as f32,
                    width: size.x,
                    height: size.y,
                },
                GlobalZIndex(2001),
                BriefingPart,
            ));
        }
    }
    // The objectives panel: parchment, title, list.
    let panel = &scene.panel;
    let panel_z = scene
        .timeline
        .tracks()
        .iter()
        .find(|track| track.id == PANEL_ELEMENT)
        .map_or(0, |track| track.z);
    if let Some((handle, size)) = image_of(&panel.background_bitmap, images) {
        let mut node = ImageNode::new(handle);
        node.color = Color::srgba(1.0, 1.0, 1.0, 0.0);
        commands.spawn((
            absolute(),
            node,
            Placed {
                x: panel.background_at.x as f32,
                y: panel.background_at.y as f32,
                width: size.x,
                height: size.y,
            },
            Visibility::Hidden,
            PanelPart::Parchment,
            GlobalZIndex(BASE_Z + 2 * panel_z),
            BriefingPart,
        ));
    }
    commands.spawn((
        absolute(),
        Text::new(scene.panel_title.clone()),
        TextFont {
            font_size: FontSize::Px(TITLE_FONT_PX),
            ..default()
        },
        TextColor(argb(LIST_COLOR)),
        Placed {
            x: panel.title_at.x as f32,
            y: panel.title_at.y as f32,
            width: panel.wordwrap.0 as f32,
            height: TITLE_FONT_PX * 1.4,
        },
        Visibility::Hidden,
        PanelPart::Title,
        GlobalZIndex(BASE_Z + 2 * panel_z + 1),
        BriefingPart,
    ));
    let mut list = absolute();
    list.flex_direction = bevy::prelude::FlexDirection::Column;
    commands
        .spawn((
            list,
            Placed {
                x: panel.list_at.x as f32,
                y: panel.list_at.y as f32,
                width: panel.wordwrap.0 as f32,
                height: panel.wordwrap.1 as f32,
            },
            Visibility::Hidden,
            PanelPart::List,
            GlobalZIndex(BASE_Z + 2 * panel_z + 1),
            BriefingPart,
        ))
        .with_children(|parent| {
            for (index, text) in scene.objectives.iter().enumerate() {
                parent.spawn((
                    Node {
                        display: Display::None,
                        margin: bevy::prelude::UiRect::bottom(Val::Px(panel.spacing as f32)),
                        ..default()
                    },
                    Text::new(text.clone()),
                    TextFont {
                        font_size: FontSize::Px(LIST_FONT_PX),
                        ..default()
                    },
                    TextLayout::new(Justify::Left, LineBreak::WordBoundary),
                    TextColor(argb(LIST_COLOR)),
                    ObjectiveRow {
                        index: index as u32,
                    },
                ));
            }
        });
    // The script's pictures and lines.
    for track in scene.timeline.tracks() {
        match &track.kind {
            ElementKind::Picture { bitmap } => {
                let Some((handle, size)) = image_of(bitmap, images) else {
                    continue;
                };
                let mut node = ImageNode::new(handle);
                node.color = Color::srgba(1.0, 1.0, 1.0, 0.0);
                commands.spawn((
                    absolute(),
                    node,
                    UiTransform::IDENTITY,
                    Visibility::Hidden,
                    PictureElement {
                        id: track.id.clone(),
                        size,
                    },
                    GlobalZIndex(BASE_Z + 2 * track.z),
                    BriefingPart,
                ));
            }
            ElementKind::Line { color, .. } => {
                commands.spawn((
                    absolute(),
                    BackgroundColor(Color::srgba_u8(color[0], color[1], color[2], 0)),
                    UiTransform::IDENTITY,
                    Visibility::Hidden,
                    LineElement {
                        id: track.id.clone(),
                    },
                    GlobalZIndex(BASE_Z + 2 * track.z),
                    BriefingPart,
                ));
            }
            ElementKind::Panel | ElementKind::Objective { .. } => {}
        }
    }
    // The three buttons.
    for button in &scene.buttons {
        let Some((handle, _)) = image_of(&button.bitmap, images) else {
            continue;
        };
        let rect = Placed {
            x: button.rect.x as f32,
            y: button.rect.y as f32,
            width: button.rect.width as f32,
            height: button.rect.height as f32,
        };
        commands.spawn((
            absolute(),
            ImageNode::new(handle),
            rect,
            ButtonArt { kind: button.kind },
            GlobalZIndex(2500),
            BriefingPart,
        ));
        // The label is centred on the record's offset, a row of the button's
        // own width.
        let centre = button.rect.x as f32 + button.label_offset.x as f32;
        let label = Placed {
            x: centre - button.rect.width as f32 / 2.0,
            y: (button.rect.y as i32 + button.label_offset.y) as f32,
            width: button.rect.width as f32,
            height: button.rect.height as f32,
        };
        let mut holder = absolute();
        holder.justify_content = bevy::prelude::JustifyContent::Center;
        commands
            .spawn((holder, label, GlobalZIndex(2501), BriefingPart))
            .with_children(|parent| {
                parent.spawn((
                    Text::new(button.label.clone()),
                    TextFont {
                        font_size: FontSize::Px(LABEL_FONT_PX),
                        ..default()
                    },
                    TextLayout::new(Justify::Center, LineBreak::NoWrap),
                    TextColor(argb(LABEL_NORMAL)),
                    ButtonLabel { kind: button.kind },
                ));
            });
    }
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            bottom: Val::Px(40.0),
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.85, 0.2)),
        GlobalZIndex(2600),
        NoticeLine,
        BriefingPart,
    ));
}

fn sync_briefing_screen(
    mut commands: Commands,
    phase: Res<FlightCheckPhase>,
    artwork: Res<BriefingArtwork>,
    mut images: ResMut<bevy::prelude::Assets<Image>>,
    mut shown: ResMut<BriefingShown>,
    parts: Query<Entity, With<BriefingPart>>,
) {
    let wanted = phase.stage() == PreMissionStage::Briefing && phase.briefing().is_some();
    if wanted && !shown.0 {
        spawn_briefing(&mut commands, &phase, &artwork, &mut images);
        shown.0 = true;
    } else if !wanted && shown.0 {
        for part in &parts {
            commands.entity(part).despawn();
        }
        shown.0 = false;
    }
}

/// Starts, restarts and stops the voice-over and advances the clock.
fn drive_briefing(
    mut phase: ResMut<FlightCheckPhase>,
    mut voice: ResMut<BriefingVoice>,
    driver: Res<BriefingDriver>,
    time: Res<Time<Real>>,
) {
    if phase.stage() != PreMissionStage::Briefing {
        if voice.device.is_some() {
            voice.close();
        }
        return;
    }
    let delta = driver
        .fixed_step
        .unwrap_or_else(|| f64::from(time.delta_secs()));
    let Some(briefing) = phase.briefing_mut() else {
        return;
    };
    let generation = briefing.playback().restarts();
    if voice.generation != Some(generation) {
        voice.generation = Some(generation);
        voice.start();
    }
    let followed = if driver.follow_voice {
        voice.played_seconds().filter(|seconds| *seconds > 0.0)
    } else {
        None
    };
    match followed {
        Some(seconds) => briefing.playback_mut().sync_to_voice(seconds),
        None => briefing.playback_mut().advance(delta),
    }
}

fn animate_pictures(
    phase: Res<FlightCheckPhase>,
    windows: Query<&Window>,
    mut pictures: Query<(
        &PictureElement,
        &mut Node,
        &mut Visibility,
        &mut UiTransform,
        &mut ImageNode,
    )>,
) {
    let (Some(briefing), Ok(window)) = (phase.briefing(), windows.single()) else {
        return;
    };
    let Some(fit) = Fit::of(window, briefing.scene().size) else {
        return;
    };
    let drawn = briefing.scene().timeline.frame(briefing.playback().time());
    for (element, mut node, mut visibility, mut transform, mut image) in &mut pictures {
        let Some(shown) = drawn.iter().find(|shown| shown.id == element.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        image.color = Color::srgba(1.0, 1.0, 1.0, shown.alpha);
        let (mut x, mut y) = shown.position;
        if shown.centered {
            x -= element.size.x / 2.0;
            y -= element.size.y / 2.0;
        }
        node.left = Val::Px(fit.left + x * fit.scale);
        node.top = Val::Px(fit.top + y * fit.scale);
        node.width = fit.px(element.size.x);
        node.height = fit.px(element.size.y);
        transform.rotation = Rot2::turn_fraction(shown.rotation_revs);
    }
}

fn animate_lines(
    phase: Res<FlightCheckPhase>,
    windows: Query<&Window>,
    mut lines: Query<(
        &LineElement,
        &mut Node,
        &mut Visibility,
        &mut UiTransform,
        &mut BackgroundColor,
    )>,
) {
    let (Some(briefing), Ok(window)) = (phase.briefing(), windows.single()) else {
        return;
    };
    let Some(fit) = Fit::of(window, briefing.scene().size) else {
        return;
    };
    let scene = briefing.scene();
    let drawn = scene.timeline.frame(briefing.playback().time());
    for (element, mut node, mut visibility, mut transform, mut color) in &mut lines {
        let Some(shown) = drawn.iter().find(|shown| shown.id == element.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let ElementKind::Line { points, color: rgb } = &shown.kind else {
            continue;
        };
        *visibility = Visibility::Inherited;
        let (x0, y0) = (points[0].x as f32, points[0].y as f32);
        let (x1, y1) = (points[1].x as f32, points[1].y as f32);
        let length = (x1 - x0).hypot(y1 - y0);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        node.left = Val::Px(fit.left + (cx - length / 2.0) * fit.scale);
        node.top = Val::Px(fit.top + (cy - LINE_THICKNESS / 2.0) * fit.scale);
        node.width = fit.px(length);
        node.height = fit.px(LINE_THICKNESS);
        transform.rotation = Rot2::radians((y1 - y0).atan2(x1 - x0));
        color.0 = Color::srgba_u8(rgb[0], rgb[1], rgb[2], (shown.alpha * 255.0) as u8);
    }
}

#[allow(clippy::type_complexity)]
fn animate_panel(
    phase: Res<FlightCheckPhase>,
    windows: Query<&Window>,
    mut parts: Query<
        (
            &PanelPart,
            &Placed,
            &mut Node,
            &mut Visibility,
            Option<&mut ImageNode>,
            Option<&mut TextColor>,
        ),
        Without<ObjectiveRow>,
    >,
    mut rows: Query<(&ObjectiveRow, &mut Node, &mut TextColor, &mut TextFont), With<ObjectiveRow>>,
) {
    let (Some(briefing), Ok(window)) = (phase.briefing(), windows.single()) else {
        return;
    };
    let scene = briefing.scene();
    let Some(fit) = Fit::of(window, scene.size) else {
        return;
    };
    let drawn = scene.timeline.frame(briefing.playback().time());
    let panel = drawn.iter().find(|shown| shown.id == PANEL_ELEMENT);
    let alpha = panel.map_or(0.0, |shown| shown.alpha);
    for (part, placed, mut node, mut visibility, image, text) in &mut parts {
        *visibility = if panel.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        node.left = Val::Px(fit.left + placed.x * fit.scale);
        node.top = Val::Px(fit.top + placed.y * fit.scale);
        node.width = fit.px(placed.width);
        node.height = fit.px(placed.height);
        if let Some(mut image) = image {
            image.color = Color::srgba(1.0, 1.0, 1.0, alpha);
        }
        if let Some(mut text) = text {
            text.0 = to_logical_color(LIST_COLOR, alpha);
        }
        let _ = part;
    }
    let lines = scene.objective_lines(&drawn);
    for (row, mut node, mut color, mut font) in &mut rows {
        let shown = lines.iter().any(|(index, _)| *index == row.index);
        node.display = if shown { Display::Flex } else { Display::None };
        node.width = fit.px(scene.panel.wordwrap.0 as f32);
        node.margin = bevy::prelude::UiRect::bottom(fit.px(scene.panel.spacing as f32));
        font.font_size = FontSize::Px(LIST_FONT_PX * fit.scale);
        color.0 = to_logical_color(LIST_COLOR, alpha);
    }
}

#[allow(clippy::type_complexity)]
fn place_static_parts(
    phase: Res<FlightCheckPhase>,
    windows: Query<&Window>,
    mut parts: Query<
        (
            &Placed,
            &mut Node,
            Option<&mut TextFont>,
            Option<&PanelPart>,
        ),
        With<BriefingPart>,
    >,
) {
    let (Some(briefing), Ok(window)) = (phase.briefing(), windows.single()) else {
        return;
    };
    let Some(fit) = Fit::of(window, briefing.scene().size) else {
        return;
    };
    for (placed, mut node, font, panel) in &mut parts {
        if panel.is_some() {
            // The panel's parts are placed by `animate_panel`.
            continue;
        }
        node.left = Val::Px(fit.left + placed.x * fit.scale);
        node.top = Val::Px(fit.top + placed.y * fit.scale);
        node.width = fit.px(placed.width);
        node.height = fit.px(placed.height);
        if let Some(mut font) = font {
            font.font_size = FontSize::Px(LABEL_FONT_PX * fit.scale);
        }
    }
}

/// The rollover: the label under the pointer is drawn in the rollover colour,
/// the pressed one in the activate colour.
fn animate_buttons(
    phase: Res<FlightCheckPhase>,
    windows: Query<&Window>,
    mouse: Res<bevy::prelude::ButtonInput<bevy::prelude::MouseButton>>,
    mut labels: Query<(&ButtonLabel, &mut TextColor, &mut TextFont)>,
    arts: Query<(&ButtonArt, &Placed)>,
) {
    let (Some(briefing), Ok(window)) = (phase.briefing(), windows.single()) else {
        return;
    };
    let Some(fit) = Fit::of(window, briefing.scene().size) else {
        return;
    };
    let hovered = window.cursor_position().and_then(|at| {
        arts.iter()
            .find(|(_, placed)| {
                let left = fit.left + placed.x * fit.scale;
                let top = fit.top + placed.y * fit.scale;
                at.x >= left
                    && at.x < left + placed.width * fit.scale
                    && at.y >= top
                    && at.y < top + placed.height * fit.scale
            })
            .map(|(art, _)| art.kind)
    });
    for (label, mut color, mut font) in &mut labels {
        font.font_size = FontSize::Px(LABEL_FONT_PX * fit.scale);
        color.0 = match hovered {
            Some(kind) if kind == label.kind && mouse.pressed(bevy::prelude::MouseButton::Left) => {
                argb(LABEL_ACTIVATE)
            }
            Some(kind) if kind == label.kind => argb(LABEL_ROLLOVER),
            _ => argb(LABEL_NORMAL),
        };
    }
}

fn show_briefing_notice(
    phase: Res<FlightCheckPhase>,
    mut lines: Query<&mut Text, With<NoticeLine>>,
) {
    let Ok(mut line) = lines.single_mut() else {
        return;
    };
    let wanted = phase
        .briefing()
        .and_then(|briefing| briefing.notice())
        .unwrap_or("");
    if line.0 != wanted {
        line.0 = wanted.to_owned();
    }
}

// ------------------------------------------------------------ the probe --

/// The element ids the timeline draws at `time` seconds, as the screen's
/// probe names them: every picture and line with opacity, `OBJECTIVESLIST`
/// for the panel, `OBJECTIVE:<n>` for each objective line shown on it.
#[must_use]
pub(super) fn expected_ids(scene: &BriefingScene, time: f64) -> Vec<String> {
    let drawn = scene.timeline.frame(time);
    let panel = drawn
        .iter()
        .any(|shown| shown.id == PANEL_ELEMENT && shown.alpha > 0.001);
    let mut ids: Vec<String> = drawn
        .iter()
        .filter_map(|shown| match &shown.kind {
            ElementKind::Picture { .. } | ElementKind::Line { .. } if shown.alpha > 0.001 => {
                Some(shown.id.clone())
            }
            ElementKind::Panel if shown.alpha > 0.001 => Some(PANEL_ELEMENT.to_owned()),
            ElementKind::Objective { index } if panel => Some(format!("OBJECTIVE:{index}")),
            _ => None,
        })
        .collect();
    ids.sort();
    ids
}

/// What the smoke reads off the drawn screen: the ids the entities show, and
/// the buttons' regions.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct BriefingProbe<'w, 's> {
    pictures: Query<
        'w,
        's,
        (
            &'static PictureElement,
            &'static Visibility,
            &'static ImageNode,
        ),
    >,
    lines: Query<
        'w,
        's,
        (
            &'static LineElement,
            &'static Visibility,
            &'static BackgroundColor,
        ),
    >,
    panel: Query<
        'w,
        's,
        (
            &'static PanelPart,
            &'static Visibility,
            Option<&'static ImageNode>,
        ),
    >,
    rows: Query<'w, 's, (&'static ObjectiveRow, &'static Node)>,
    buttons: Query<
        'w,
        's,
        (
            &'static ButtonArt,
            &'static bevy::prelude::InheritedVisibility,
            &'static bevy::ui::UiGlobalTransform,
            &'static bevy::ui::ComputedNode,
        ),
    >,
}

impl BriefingProbe<'_, '_> {
    /// The ids the entities show, sorted, in [`expected_ids`]' spelling.
    pub(super) fn drawn_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for (element, visibility, image) in &self.pictures {
            if *visibility != Visibility::Hidden && image.color.to_srgba().alpha > 0.001 {
                ids.push(element.id.clone());
            }
        }
        for (element, visibility, color) in &self.lines {
            if *visibility != Visibility::Hidden && color.0.to_srgba().alpha > 0.001 {
                ids.push(element.id.clone());
            }
        }
        let panel_shown = self.panel.iter().any(|(part, visibility, image)| {
            *part == PanelPart::Parchment
                && *visibility != Visibility::Hidden
                && image.is_some_and(|image| image.color.to_srgba().alpha > 0.001)
        });
        if panel_shown {
            ids.push(PANEL_ELEMENT.to_owned());
        }
        if panel_shown {
            for (row, node) in &self.rows {
                if node.display != Display::None {
                    ids.push(format!("OBJECTIVE:{}", row.index));
                }
            }
        }
        ids.sort();
        ids
    }

    /// Each button as `(name, visible, [x, y, width, height])` in
    /// framebuffer pixels.
    pub(super) fn buttons(&self) -> Vec<(String, bool, [u32; 4])> {
        self.buttons
            .iter()
            .map(|(art, visible, transform, node)| {
                let centre = transform.translation;
                let size = node.size;
                (
                    art.kind.key().to_owned(),
                    visible.get(),
                    [
                        (centre.x - 0.5 * size.x).max(0.0) as u32,
                        (centre.y - 0.5 * size.y).max(0.0) as u32,
                        size.x.max(1.0) as u32,
                        size.y.max(1.0) as u32,
                    ],
                )
            })
            .collect()
    }
}
