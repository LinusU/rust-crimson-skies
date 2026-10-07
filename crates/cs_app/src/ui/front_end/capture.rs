//! A real GPU capture of one front-end screen, offscreen
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, stage
//! `### F45-D`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! # What this is for
//!
//! F45-D's required capabilities are `gpu` and `retail` and its minimum
//! scenario is *"capture and review all original front-end screens and
//! navigation paths"*. F45-A built the state table, F45-B the authored
//! presentation (`ScreenSession::view`) and F45-C the wiring; none of them
//! ever **drew** anything, so "a visible button must have a functioning state
//! transition" was checked on data, never on a frame. This module is the
//! drawing half: it takes a [`ScreenView`] — the artwork id, its logical size
//! and every hotspot already mapped by the image's own aspect-fit — plus the
//! decoded [`Artwork`] pixels, draws the frame on the real renderer and
//! writes a PNG with the measured facts about that frame attached.
//!
//! # What is **not** claimed
//!
//! * **Not the original's appearance.** The pixels are whatever the caller
//!   decoded: an authored fixture screen in a synthetic test, a decoded
//!   original texture in the retail test. No colour, scale or hotspot is
//!   invented here, and nothing in this module says which original artwork
//!   belongs to which screen — that binding is unread
//!   (`docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`,
//!   resolving task #742).
//! * **Not the original's focus order.** The focused button is drawn because
//!   [`ScreenView`] reports one; the order it was reached in is F45-B's
//!   authored declaration order.
//! * **Not a screenshot of the original executable.** This is our own
//!   renderer drawing our own view. What the original draws when it runs
//!   needs an original run (REF-OWNER-FIRST-CAPTURE), which no agent can
//!   produce.
//!
//! # The refusals
//!
//! Every way this can produce a file that is not evidence of a drawn screen
//! is a named error, never a written PNG:
//!
//! * [`ScreenCaptureError::EmptyArtwork`] / [`ScreenCaptureError::PixelCount`]
//!   — the artwork carries no pixels at all, so there is nothing to draw.
//! * [`ScreenCaptureError::SurfaceMismatch`] — the view was fitted against a
//!   surface other than this capture's own frame, so its hotspots would land
//!   on the wrong pixels.
//! * [`ScreenCaptureError::SizeMismatch`] — the decoded artwork's extent is
//!   not the logical image the view fitted, so the hotspots would be mapped
//!   onto a differently shaped picture.
//! * [`ScreenCaptureError::NoScreenshotCaptured`] — the renderer ran but
//!   produced no image.
//! * [`ScreenCaptureError::UniformFrame`] — every pixel came back as the
//!   background: nothing was drawn. This is the check that makes the PNG
//!   evidence rather than a decoration.
//! * [`ScreenCaptureError::Io`] — the PNG could not be written or read back.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use bevy::app::PluginGroup;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::image::{Image, ImageSampler};
use bevy::prelude::{
    App, Assets, Camera, Camera2d, Color, DefaultPlugins, Handle, On, Res, Resource, Sprite,
    Transform, Vec2, WindowPlugin, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use cs_assets::install::sha256;
use cs_content::ui_layout::{AspectFit, Rect};
use cs_types::evidence::ContentHash;

use super::screens::ScreenView;

/// The capture frame's width in pixels: the extent every front-end screen
/// background in `GOSDATA/ASSETS/crimson.rof` stores (measured `800x600`, see
/// [`super::retail`]), so an original screen maps onto the frame one-to-one.
/// A 640x480 authored screen — the F45 fixture's own size, and the stored
/// extent of `mainmenu` and `escapemenu` in `ZBD/rimage.zbd` — letterboxes by
/// exactly the same [`AspectFit`] a player's surface would.
pub const SCREEN_CAPTURE_WIDTH: u32 = 800;

/// The capture frame's height in pixels. As [`SCREEN_CAPTURE_WIDTH`].
pub const SCREEN_CAPTURE_HEIGHT: u32 = 600;

/// The surface every [`ScreenView`] a capture takes must have been made
/// against.
pub const SCREEN_CAPTURE_SURFACE: (u32, u32) = (SCREEN_CAPTURE_WIDTH, SCREEN_CAPTURE_HEIGHT);

/// The clear colour the capture renders onto: the frame's background.
///
/// Chosen dark and deliberately unequal to [`BUTTON_COLOR`] and
/// [`FOCUS_COLOR`], so a frame that drew nothing and a frame that drew
/// something are distinguishable by [`ScreenCapture::covered_pixels`].
const CLEAR_COLOR: [f32; 4] = [0.051, 0.063, 0.086, 1.0];

/// The colour an unfocused hotspot's region is drawn with.
const BUTTON_COLOR: [f32; 4] = [0.90, 0.45, 0.18, 0.55];

/// The colour the focused hotspot's region is drawn with: a different hue, so
/// "which button has focus" is visible in the frame rather than asserted.
const FOCUS_COLOR: [f32; 4] = [0.25, 0.75, 1.0, 0.75];

/// How many frames the capture waits before asking for the screenshot.
///
/// The sprite assets have to reach the renderer before a frame can draw them;
/// four frames is the same measured sufficiency the F51 text capture uses.
const WARMUP_FRAMES: u32 = 4;

/// The bound on how many updates one capture may drive.
///
/// The readback is asynchronous, so the bound turns a driver that never answers
/// into [`ScreenCaptureError::NoScreenshotCaptured`] rather than a hang.
const MAX_CAPTURE_UPDATES: u32 = 24;

/// One screen's decoded pixels, RGBA8, top row first.
///
/// The extent is the *logical* image the layout describes ([`ScreenView::image`]),
/// so a capture can check that the picture and the hotspots it is drawn with
/// describe the same image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artwork {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Artwork {
    /// Builds artwork from `width * height` RGBA8 pixels in row-major order.
    ///
    /// # Errors
    ///
    /// [`ScreenCaptureError::EmptyArtwork`] for a zero side,
    /// [`ScreenCaptureError::PixelCount`] when `rgba.len() != width * height * 4`.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, ScreenCaptureError> {
        if width == 0 || height == 0 {
            return Err(ScreenCaptureError::EmptyArtwork { width, height });
        }
        let expected = u64::from(width) * u64::from(height) * 4;
        let expected = usize::try_from(expected).unwrap_or(usize::MAX);
        if rgba.len() != expected {
            return Err(ScreenCaptureError::PixelCount {
                expected,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// The artwork's width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// The artwork's height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The artwork's extent, as [`super::screens::ScreenView::image`] spells it.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The raw RGBA8 bytes, row-major, top row first.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Whether every pixel is the same colour: a picture that would draw as a
    /// flat slab wherever it is put.
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .skip(1)
            .all(|pixel| *pixel == self.rgba[..4])
    }
}

/// One hotspot as the capture draws it: its region in **surface** pixels and
/// whether it holds focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedButton {
    /// The button's stable id, for a reader of the report.
    pub id: String,
    /// The region in capture-frame pixels.
    pub rect: Rect,
    /// Whether this is the focused button.
    pub focused: bool,
}

/// What one capture produced, all of it measured from the frame that came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenCapture {
    /// The label the capture was asked for (a screen or image name).
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
    /// How many hotspot regions were drawn on top of the artwork.
    pub buttons: usize,
    /// SHA-256 of the written PNG's bytes.
    pub png_sha256: ContentHash,
    /// How many bytes the PNG has.
    pub png_bytes: u64,
    /// Where the PNG was written.
    pub png: String,
}

impl ScreenCapture {
    /// Whether the frame drew anything at all. A capture that exists has
    /// already passed the uniform-frame gate, so this is a predicate over
    /// measured facts.
    #[must_use]
    pub const fn drew_screen(&self) -> bool {
        self.distinct_luminance > 1 && self.covered_pixels > 0
    }
}

/// Why a capture could not be produced.
#[derive(Debug)]
pub enum ScreenCaptureError {
    /// The artwork carries no pixels.
    EmptyArtwork {
        /// The refused extent.
        width: u32,
        /// The refused extent.
        height: u32,
    },
    /// The pixel buffer is not `width * height * 4` bytes long.
    PixelCount {
        /// Bytes the extent needs.
        expected: usize,
        /// Bytes supplied.
        actual: usize,
    },
    /// The view was fitted against another surface, so its hotspots do not
    /// belong to this frame.
    SurfaceMismatch {
        /// The surface the view was fitted against, as its own fit reports it.
        view_surface: (u32, u32),
    },
    /// The decoded artwork is not the image the view fitted.
    SizeMismatch {
        /// The logical image the view fitted.
        view_image: (u32, u32),
        /// The decoded artwork's extent.
        artwork: (u32, u32),
    },
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

impl fmt::Display for ScreenCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArtwork { width, height } => {
                write!(f, "the artwork {width}x{height} carries no pixels to draw")
            }
            Self::PixelCount { expected, actual } => write!(
                f,
                "the artwork supplies {actual} bytes where {expected} RGBA8 pixels are needed"
            ),
            Self::SurfaceMismatch { view_surface } => write!(
                f,
                "the view was fitted against a {}x{} surface, not this capture's {}x{} frame",
                view_surface.0, view_surface.1, SCREEN_CAPTURE_WIDTH, SCREEN_CAPTURE_HEIGHT
            ),
            Self::SizeMismatch {
                view_image,
                artwork,
            } => write!(
                f,
                "the view fitted a {}x{} image but the artwork decodes to {}x{}",
                view_image.0, view_image.1, artwork.0, artwork.1
            ),
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

impl std::error::Error for ScreenCaptureError {}

/// Captures the screen `view` presents, with `artwork` drawn behind its
/// hotspots.
///
/// `view` must have been made against [`SCREEN_CAPTURE_SURFACE`]: its fit and
/// its hotspot rectangles are the ones this frame draws, checked rather than
/// assumed.
///
/// # Errors
///
/// [`ScreenCaptureError::SurfaceMismatch`] when the view was fitted against
/// another surface, [`ScreenCaptureError::SizeMismatch`] when the artwork is
/// not the image that fit describes, then the same refusals as
/// [`capture_artwork`].
pub fn capture_screen(
    view: &ScreenView,
    artwork: &Artwork,
    png: &Path,
) -> Result<ScreenCapture, ScreenCaptureError> {
    let expected = AspectFit::new(view.image, SCREEN_CAPTURE_SURFACE).ok_or(
        ScreenCaptureError::SurfaceMismatch {
            view_surface: (0, 0),
        },
    )?;
    if view.fit != expected {
        return Err(ScreenCaptureError::SurfaceMismatch {
            view_surface: SCREEN_CAPTURE_SURFACE,
        });
    }
    if artwork.extent() != view.image {
        return Err(ScreenCaptureError::SizeMismatch {
            view_image: view.image,
            artwork: artwork.extent(),
        });
    }
    let buttons: Vec<CapturedButton> = view
        .buttons
        .iter()
        .map(|button| CapturedButton {
            id: button.id.to_string(),
            rect: button.rect,
            focused: button.focused,
        })
        .collect();
    capture_artwork(&format!("{:?}", view.screen), artwork, &buttons, png)
}

/// Captures `artwork` fitted into the frame, with `buttons` drawn on top.
///
/// This is the path a *retail* capture takes: the original artwork with no
/// deck behind it, because no original hotspot layout is decoded anywhere in
/// this repository (task #742).
///
/// # Errors
///
/// [`ScreenCaptureError::EmptyArtwork`]/[`ScreenCaptureError::PixelCount`]
/// (from [`Artwork::new`]'s checks, re-checked here), then
/// [`ScreenCaptureError::NoScreenshotCaptured`],
/// [`ScreenCaptureError::UniformFrame`] or [`ScreenCaptureError::Io`].
/// Every refusal leaves no PNG behind.
pub fn capture_artwork(
    label: &str,
    artwork: &Artwork,
    buttons: &[CapturedButton],
    png: &Path,
) -> Result<ScreenCapture, ScreenCaptureError> {
    if artwork.width() == 0 || artwork.height() == 0 {
        return Err(ScreenCaptureError::EmptyArtwork {
            width: artwork.width(),
            height: artwork.height(),
        });
    }
    let expected_len = u64::from(artwork.width()) * u64::from(artwork.height()) * 4;
    if u64::try_from(artwork.rgba().len()).unwrap_or(u64::MAX) != expected_len {
        return Err(ScreenCaptureError::PixelCount {
            expected: usize::try_from(expected_len).unwrap_or(usize::MAX),
            actual: artwork.rgba().len(),
        });
    }
    let fit = AspectFit::new(artwork.extent(), SCREEN_CAPTURE_SURFACE).ok_or(
        ScreenCaptureError::EmptyArtwork {
            width: artwork.width(),
            height: artwork.height(),
        },
    )?;

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

    let target = capture_image();
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(target);
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

    // The artwork, fitted exactly as [`AspectFit`] fits it for a player.
    let image_rect = fit.image_rect();
    let texture = artwork_image(artwork);
    let texture = app.world_mut().resource_mut::<Assets<Image>>().add(texture);
    app.world_mut().spawn((
        Sprite {
            image: texture,
            custom_size: Some(Vec2::new(image_rect.width as f32, image_rect.height as f32)),
            ..default()
        },
        Transform::from_xyz(
            surface_x(image_rect.x + image_rect.width / 2),
            surface_y(image_rect.y + image_rect.height / 2),
            0.0,
        ),
    ));

    // The hotspots, over the artwork: a focused one in its own colour.
    for button in buttons {
        let color = if button.focused {
            FOCUS_COLOR
        } else {
            BUTTON_COLOR
        };
        app.world_mut().spawn((
            Sprite::from_color(
                Color::srgba(color[0], color[1], color[2], color[3]),
                Vec2::new(button.rect.width as f32, button.rect.height as f32),
            ),
            Transform::from_xyz(
                surface_x(button.rect.x + button.rect.width / 2),
                surface_y(button.rect.y + button.rect.height / 2),
                1.0,
            ),
        ));
    }

    drive_capture(&mut app, CaptureTarget { image: handle }, png)?;

    // Every refusal from here on removes the PNG the renderer already wrote, so
    // a refused capture cannot leave a file that reads like a good one.
    let facts = {
        let recorded = app.world().resource::<CapturedFrame>();
        let guard = recorded.0.lock().map_err(|_| {
            discard_capture(png);
            ScreenCaptureError::NoScreenshotCaptured {
                updates: MAX_CAPTURE_UPDATES,
            }
        })?;
        match *guard {
            Some(facts) => facts,
            None => {
                discard_capture(png);
                return Err(ScreenCaptureError::NoScreenshotCaptured {
                    updates: MAX_CAPTURE_UPDATES,
                });
            }
        }
    };
    if facts.distinct_luminance <= 1 {
        discard_capture(png);
        return Err(ScreenCaptureError::UniformFrame {
            distinct_luminance: facts.distinct_luminance,
            covered_pixels: facts.covered_pixels,
            total_pixels: facts.width as usize * facts.height as usize,
        });
    }

    let bytes = std::fs::read(png).map_err(|error| ScreenCaptureError::Io {
        path: png.display().to_string(),
        reason: error.to_string(),
    })?;
    Ok(ScreenCapture {
        label: label.to_owned(),
        adapter: adapter_name(&app),
        width: facts.width,
        height: facts.height,
        distinct_luminance: facts.distinct_luminance,
        covered_pixels: facts.covered_pixels,
        covered_permille: covered_permille(facts.covered_pixels, facts.width, facts.height),
        buttons: buttons.len(),
        png_sha256: sha256(&bytes),
        png_bytes: bytes.len() as u64,
        png: png.display().to_string(),
    })
}

/// A surface pixel's x in the camera's world space (origin at frame centre).
const fn surface_x(x: u32) -> f32 {
    x as f32 - SCREEN_CAPTURE_WIDTH as f32 * 0.5
}

/// A surface pixel's y in the camera's world space: the surface's y grows
/// downward and the world's grows upward, so the frame's top edge is the
/// world's positive y.
const fn surface_y(y: u32) -> f32 {
    SCREEN_CAPTURE_HEIGHT as f32 * 0.5 - y as f32
}

/// The artwork as a Bevy texture: the decoded RGBA8 plane, row-major from the
/// top, which is the canonical orientation [`AspectFit`] and every hotspot
/// were written against.
fn artwork_image(artwork: &Artwork) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: artwork.width(),
            height: artwork.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        artwork.rgba().to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING;
    image
}

/// The render target the frame is drawn into and read back from.
fn capture_image() -> Image {
    let size = Extent3d {
        width: SCREEN_CAPTURE_WIDTH,
        height: SCREEN_CAPTURE_HEIGHT,
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

/// Drives the app until the screenshot comes back or the updates run out.
fn drive_capture(
    app: &mut App,
    target: CaptureTarget,
    png: &Path,
) -> Result<(), ScreenCaptureError> {
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
    Err(ScreenCaptureError::NoScreenshotCaptured {
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
