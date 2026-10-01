//! A real GPU capture of one laid-out localized string block, offscreen
//! (`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-D`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! # What this is for
//!
//! F51-D's required capabilities are `gpu` and `retail`, and the stage is the
//! localization/overflow audit. [`crate::text::audit`] measures what the strings
//! and media are; this module is the `gpu` half: it takes the **real line
//! geometry** the production [`layout_text`](crate::text::layout::layout_text)
//! produced for a locale, draws one filled quad per painted line on the real
//! renderer and writes a PNG with the measured facts about that frame attached.
//!
//! # What is **not** claimed
//!
//! * **Not glyph rendering.** The original font is a bitmap whose
//!   cell-to-character mapping is unmeasured, so this capture draws the line
//!   **boxes** the layout placed, not the original's glyphs. It is evidence that
//!   the laid-out geometry is drawable and has extent; it is not evidence about
//!   how the original drew text.
//! * **Not the original's appearance.** The colours are chosen here, and no
//!   stored material or texture is read. This is a geometry witness.
//! * **Not a font or layout measurement.** The line boxes come from the caller's
//!   [`TextMetrics`](crate::text::metrics::TextMetrics); the capture adds no
//!   metric of its own.
//!
//! # The refusals
//!
//! Every way this can produce a file that is not evidence of a drawn block is a
//! named error, never a written PNG:
//!
//! * [`TextCaptureError::NoVisibleLines`] — the layout painted no line, so a
//!   frame would be empty however well it was framed.
//! * [`TextCaptureError::NoScreenshotCaptured`] — the renderer ran but produced
//!   no image.
//! * [`TextCaptureError::UniformFrame`] — the frame came back and every pixel is
//!   the clear colour: nothing was drawn. This is what makes the PNG evidence
//!   rather than a decoration.
//! * [`TextCaptureError::Io`] — the PNG could not be written or read back.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use bevy::app::PluginGroup;
use bevy::camera::ClearColorConfig;
use bevy::camera::RenderTarget;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::{
    App, Assets, Camera, Camera2d, Color, DefaultPlugins, Handle, On, Res, Resource, Sprite,
    Transform, Vec2, WindowPlugin, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use cs_assets::install::sha256;
use cs_types::evidence::ContentHash;

use super::layout::TextLayout;

/// The capture frame's width in pixels. Matches the AC01 fixture panel so the
/// panel maps onto the frame one-to-one.
pub const TEXT_CAPTURE_WIDTH: u32 = 640;

/// The capture frame's height in pixels. As [`TEXT_CAPTURE_WIDTH`].
pub const TEXT_CAPTURE_HEIGHT: u32 = 480;

/// The clear colour the capture renders onto: the frame's background.
///
/// Chosen dark and deliberately unequal to [`LINE_COLOR`], so a frame that drew
/// nothing and a frame that drew something are distinguishable by
/// [`TextCapture::covered_pixels`].
const CLEAR_COLOR: [f32; 4] = [0.043, 0.055, 0.075, 1.0];

/// The colour one laid-out line's box is drawn with.
const LINE_COLOR: [f32; 4] = [0.78, 0.82, 0.7, 1.0];

/// How many frames the capture waits before asking for the screenshot.
///
/// The sprite assets have to reach the renderer before a frame can draw them;
/// one `RenderApp` pass is the minimum, and four frames is the same measured
/// sufficiency the F18-D geometry capture uses.
const WARMUP_FRAMES: u32 = 4;

/// The bound on how many updates one capture may drive.
///
/// The readback is asynchronous, so the bound turns a driver that never answers
/// into [`TextCaptureError::NoScreenshotCaptured`] rather than a hang.
const MAX_CAPTURE_UPDATES: u32 = 24;

/// One laid-out line's box, in capture-frame world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextBox {
    /// The box centre, in the camera's world space (origin at frame centre).
    pub center: [f32; 2],
    /// The box's width and height.
    pub size: [f32; 2],
    /// The box's fill colour.
    pub color: [f32; 4],
}

/// The line boxes a `layout` paints inside `panel`, mapped to the capture frame.
///
/// Only lines with a non-empty [`painted_rect`](TextLayout::painted_rect) **and**
/// a non-zero measured [`text_width`](crate::text::layout::LaidOutLine::text_width)
/// become a box: a scrolled line outside the viewport paints nothing, an empty
/// line has no text to show, and a zero-area box is not something to draw. Each
/// box is the line's measured text extent — the conservative band-wide
/// [`painted_rect`](TextLayout::painted_rect) is clipped to the line's own
/// advance width — so two locales with different text draw different frames.
/// The mapping is a uniform scale from the panel's own rectangle onto the frame,
/// so a capture of a 640x480 panel is one-to-one and a panel of another size is
/// fitted rather than clipped.
#[must_use]
pub fn text_boxes(layout: &TextLayout, panel: bevy::math::Rect) -> Vec<TextBox> {
    if panel.width() <= 0.0 || panel.height() <= 0.0 {
        return Vec::new();
    }
    let scale_x = TEXT_CAPTURE_WIDTH as f32 / panel.width();
    let scale_y = TEXT_CAPTURE_HEIGHT as f32 / panel.height();
    let mut boxes = Vec::new();
    for index in 0..layout.lines().len() {
        let Some(rect) = layout.painted_rect(index) else {
            continue;
        };
        let width = layout.lines()[index].text_width().min(rect.width());
        if width <= 0.0 || rect.height() <= 0.0 {
            continue;
        }
        let center = bevy::math::Vec2::new(rect.min.x + width * 0.5, rect.center().y);
        let world_x = (center.x - panel.min.x) * scale_x - TEXT_CAPTURE_WIDTH as f32 * 0.5;
        // The panel's y grows downward and the frame's world y grows upward, so
        // the mapping is flipped: the panel's top edge lands at the frame's top.
        let world_y = TEXT_CAPTURE_HEIGHT as f32 * 0.5 - (center.y - panel.min.y) * scale_y;
        boxes.push(TextBox {
            center: [world_x, world_y],
            size: [width * scale_x, rect.height() * scale_y],
            color: LINE_COLOR,
        });
    }
    boxes
}

/// What one capture produced, all of it measured from the frame that came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCapture {
    /// The label the capture was asked for (a locale label in practice).
    pub label: String,
    /// The adapter the renderer actually selected, as the driver reported it.
    pub adapter: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// How many distinct luminance levels the frame holds. One means the whole
    /// frame is the background.
    pub distinct_luminance: usize,
    /// Pixels that differ from [`CLEAR_COLOR`].
    pub covered_pixels: usize,
    /// [`Self::covered_pixels`] over the frame's pixel count.
    pub covered_permille: u32,
    /// How many line boxes were drawn.
    pub lines: usize,
    /// SHA-256 of the written PNG's bytes.
    pub png_sha256: ContentHash,
    /// How many bytes the PNG has.
    pub png_bytes: u64,
    /// Where the PNG was written.
    pub png: String,
}

impl TextCapture {
    /// Whether the frame drew anything at all. A capture that exists has already
    /// passed the uniform-frame gate, so this is a predicate over measured facts.
    #[must_use]
    pub const fn drew_lines(&self) -> bool {
        self.distinct_luminance > 1 && self.covered_pixels > 0
    }
}

/// Why a capture could not be produced.
#[derive(Debug)]
pub enum TextCaptureError {
    /// The layout painted no line, so there is nothing to draw.
    NoVisibleLines,
    /// The renderer ran but produced no image.
    NoScreenshotCaptured {
        /// How many updates the capture drove before giving up.
        updates: u32,
    },
    /// The frame came back and every pixel is the background: nothing was drawn.
    UniformFrame {
        /// How many distinct luminance levels the frame held. Always one.
        distinct_luminance: usize,
        /// Pixels that differ from the background, and how many pixels there are.
        covered_pixels: usize,
        /// The frame's pixel count.
        total_pixels: usize,
    },
    /// The PNG could not be written or read back.
    Io {
        /// The path involved.
        path: String,
        /// The operating system's message, verbatim.
        reason: String,
    },
}

impl fmt::Display for TextCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVisibleLines => {
                f.write_str("the layout painted no visible line, so a frame would be empty")
            }
            Self::NoScreenshotCaptured { updates } => write!(
                f,
                "the renderer ran {updates} updates and produced no captured image"
            ),
            Self::UniformFrame {
                distinct_luminance,
                covered_pixels,
                total_pixels,
            } => write!(
                f,
                "the frame held {distinct_luminance} distinct luminance level and {covered_pixels} \
                 of {total_pixels} pixels off the background, i.e. nothing was drawn"
            ),
            Self::Io { path, reason } => {
                write!(f, "the capture image {path} could not be used: {reason}")
            }
        }
    }
}

impl std::error::Error for TextCaptureError {}

/// The frame the capture app renders into, handed to the driver loop.
#[derive(Resource, Clone)]
struct CaptureTarget {
    image: Handle<Image>,
}

/// What the observer recorded about the frame that came back.
#[derive(Resource, Default)]
struct CapturedFrame(Mutex<Option<FrameFacts>>);

/// The measured facts about one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameFacts {
    width: u32,
    height: u32,
    distinct_luminance: usize,
    covered_pixels: usize,
}

/// Renders `boxes` on the real GPU and writes the frame's PNG.
///
/// `png`'s parent directory must exist; the file is written by the renderer's
/// own screenshot path and then read back for its digest, so the digest is of
/// the file on disk.
///
/// # Errors
///
/// [`TextCaptureError::NoVisibleLines`] for an empty box list,
/// [`TextCaptureError::NoScreenshotCaptured`],
/// [`TextCaptureError::UniformFrame`] or [`TextCaptureError::Io`]. Every refusal
/// leaves no PNG behind.
pub fn capture_text_boxes(
    label: &str,
    boxes: &[TextBox],
    png: &Path,
) -> Result<TextCapture, TextCaptureError> {
    if boxes.is_empty() {
        return Err(TextCaptureError::NoVisibleLines);
    }

    let mut app = App::new();
    app.init_resource::<CapturedFrame>();
    // No window: the frame is rendered into an image asset and read back, so the
    // capture needs an adapter and not a display.
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..WindowPlugin::default()
            }),
    );
    app.finish();
    app.cleanup();

    let image = capture_image();
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.world_mut().spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgba(
                CLEAR_COLOR[0],
                CLEAR_COLOR[1],
                CLEAR_COLOR[2],
                CLEAR_COLOR[3],
            )),
            ..default()
        },
        RenderTarget::Image(handle.clone().into()),
        Transform::default(),
    ));
    for line in boxes {
        app.world_mut().spawn((
            Sprite::from_color(
                Color::srgba(line.color[0], line.color[1], line.color[2], line.color[3]),
                Vec2::new(line.size[0], line.size[1]),
            ),
            Transform::from_xyz(line.center[0], line.center[1], 0.0),
        ));
    }

    drive_capture(&mut app, CaptureTarget { image: handle }, png)?;

    // Every refusal from here on removes the PNG the renderer already wrote, so
    // a refused capture cannot leave a file that reads like a good one.
    let facts = {
        let recorded = app.world().resource::<CapturedFrame>();
        let guard = recorded.0.lock().map_err(|_| {
            discard_capture(png);
            TextCaptureError::NoScreenshotCaptured {
                updates: MAX_CAPTURE_UPDATES,
            }
        })?;
        match *guard {
            Some(facts) => facts,
            None => {
                discard_capture(png);
                return Err(TextCaptureError::NoScreenshotCaptured {
                    updates: MAX_CAPTURE_UPDATES,
                });
            }
        }
    };
    if facts.distinct_luminance <= 1 {
        discard_capture(png);
        return Err(TextCaptureError::UniformFrame {
            distinct_luminance: facts.distinct_luminance,
            covered_pixels: facts.covered_pixels,
            total_pixels: facts.width as usize * facts.height as usize,
        });
    }

    let bytes = std::fs::read(png).map_err(|error| TextCaptureError::Io {
        path: png.display().to_string(),
        reason: error.to_string(),
    })?;
    Ok(TextCapture {
        label: label.to_owned(),
        adapter: adapter_name(&app),
        width: facts.width,
        height: facts.height,
        distinct_luminance: facts.distinct_luminance,
        covered_pixels: facts.covered_pixels,
        covered_permille: covered_permille(facts.covered_pixels, facts.width, facts.height),
        lines: boxes.len(),
        png_sha256: sha256(&bytes),
        png_bytes: bytes.len() as u64,
        png: png.display().to_string(),
    })
}

/// The render target the frame is drawn into and read back from.
fn capture_image() -> Image {
    let size = Extent3d {
        width: TEXT_CAPTURE_WIDTH,
        height: TEXT_CAPTURE_HEIGHT,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[
            (CLEAR_COLOR[0] * 255.0) as u8,
            (CLEAR_COLOR[1] * 255.0) as u8,
            (CLEAR_COLOR[2] * 255.0) as u8,
            255,
        ],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

/// Drives the app until the screenshot comes back or the updates run out.
fn drive_capture(app: &mut App, target: CaptureTarget, png: &Path) -> Result<(), TextCaptureError> {
    let png = png.to_path_buf();
    let observer = move |captured: On<ScreenshotCaptured>, frame: Res<CapturedFrame>| {
        if captured.image.data.is_some() {
            let image = captured.image.clone();
            *frame.0.lock().expect("the capture frame mutex") = Some(measure(&image));
        }
        save_to_disk(png.clone())(captured);
    };
    app.add_observer(observer);
    for update in 0..MAX_CAPTURE_UPDATES {
        if update == WARMUP_FRAMES {
            app.world_mut()
                .spawn(Screenshot::image(target.image.clone()));
        }
        app.update();
        if app
            .world()
            .resource::<CapturedFrame>()
            .0
            .lock()
            .expect("the capture frame mutex")
            .is_some()
        {
            return Ok(());
        }
    }
    Err(TextCaptureError::NoScreenshotCaptured {
        updates: MAX_CAPTURE_UPDATES,
    })
}

/// Removes a capture the renderer already wrote, so no refusal leaves a file.
fn discard_capture(png: &Path) {
    let _ = std::fs::remove_file(png);
}

/// The measured facts of one frame.
fn measure(image: &Image) -> FrameFacts {
    let data = image.data.as_ref().expect("a captured frame has data");
    let clear = [
        (CLEAR_COLOR[0] * 255.0).round() as u8,
        (CLEAR_COLOR[1] * 255.0).round() as u8,
        (CLEAR_COLOR[2] * 255.0).round() as u8,
    ];
    let mut levels: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
    let mut covered = 0_usize;
    for pixel in data.as_chunks::<4>().0 {
        let luminance = ((29 * u32::from(pixel[0])
            + 150 * u32::from(pixel[1])
            + 77 * u32::from(pixel[2]))
            >> 8) as u8;
        levels.insert(luminance);
        if pixel[0] != clear[0] || pixel[1] != clear[1] || pixel[2] != clear[2] {
            covered += 1;
        }
    }
    FrameFacts {
        width: image.width(),
        height: image.height(),
        distinct_luminance: levels.len(),
        covered_pixels: covered,
    }
}

/// Coverage as a permille of the frame, rounded down.
fn covered_permille(covered: usize, width: u32, height: u32) -> u32 {
    let total = u64::from(width) * u64::from(height);
    if total == 0 {
        return 0;
    }
    ((covered as u128 * 1000) / total as u128) as u32
}

/// The adapter the driver reported, as `<name> (<backend>)`.
fn adapter_name(app: &App) -> String {
    app.world()
        .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
        .map(|info| {
            let info = &*info.0;
            format!("{} ({:?})", info.name, info.backend)
        })
        .unwrap_or_else(|| "no adapter reported".to_owned())
}
