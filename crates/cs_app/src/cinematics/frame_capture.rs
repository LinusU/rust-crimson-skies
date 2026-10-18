//! A real GPU render of one composed cinematic surface, offscreen (F40-D4).
//!
//! The surface [`super::device::compose_letterboxed`] produced is uploaded as a
//! texture, drawn one-to-one onto a render target of the same size by the real
//! renderer (nearest sampling, no scaling) and read back. The readback is
//! compared with the surface that was uploaded, so the capture is evidence that
//! the renderer shows the letterboxed picture, not just that a PNG exists.
//!
//! Not claimed: this is an offscreen target, not the game window, and nobody
//! looked at it (`human_review`).

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
use cs_types::evidence::ContentHash;

use super::Viewport;

const WARMUP_FRAMES: u32 = 4;
const MAX_CAPTURE_UPDATES: u32 = 40;

/// What one GPU capture measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceCapture {
    /// The adapter the driver reported.
    pub adapter: String,
    /// Frame size.
    pub size: (u32, u32),
    /// Pixels whose RGB differs from the uploaded surface by more than 2/255
    /// in any channel.
    pub mismatched_pixels: usize,
    /// Pixels of the readback inside the viewport that are not black.
    pub picture_pixels_lit: usize,
    /// Pixels of the readback outside the viewport (the bars) that are not
    /// opaque black.
    pub bar_pixels_not_black: usize,
    /// SHA-256 of the written PNG.
    pub png_sha256: ContentHash,
    /// Size of the PNG.
    pub png_bytes: u64,
}

/// Why a capture failed.
#[derive(Debug)]
pub enum SurfaceCaptureError {
    /// The surface buffer is not `width * height * 4` bytes.
    BadSurface,
    /// The renderer returned no image in the update bound.
    NoScreenshot,
    /// The PNG could not be read back.
    Io(String),
}

impl fmt::Display for SurfaceCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSurface => f.write_str("the surface buffer has the wrong length"),
            Self::NoScreenshot => f.write_str("the renderer produced no captured image"),
            Self::Io(detail) => write!(f, "capture file: {detail}"),
        }
    }
}

impl std::error::Error for SurfaceCaptureError {}

#[derive(Resource, Default)]
struct Readback(Mutex<Option<(u32, u32, Vec<u8>)>>);

/// Renders `surface` (RGBA, `size`) on the real GPU and writes the frame's PNG.
///
/// # Errors
///
/// [`SurfaceCaptureError`]; a failure leaves no PNG behind.
pub fn capture_surface(
    surface: &[u8],
    size: (u32, u32),
    viewport: Viewport,
    png: &Path,
) -> Result<SurfaceCapture, SurfaceCaptureError> {
    if surface.len() != size.0 as usize * size.1 as usize * 4 {
        return Err(SurfaceCaptureError::BadSurface);
    }
    let mut app = App::new();
    app.init_resource::<Readback>();
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

    let extent = Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let mut source = Image::new(
        extent,
        TextureDimension::D2,
        surface.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    source.sampler = ImageSampler::nearest();
    let mut target = Image::new_fill(
        extent,
        TextureDimension::D2,
        &[255, 0, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    target.texture_descriptor.usage = TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    let (source, target): (Handle<Image>, Handle<Image>) = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        (images.add(source), images.add(target))
    };
    app.world_mut().spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgba(1.0, 0.0, 1.0, 1.0)),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        Transform::default(),
    ));
    app.world_mut().spawn((
        Sprite {
            image: source,
            custom_size: Some(Vec2::new(size.0 as f32, size.1 as f32)),
            ..default()
        },
        Transform::default(),
    ));

    let path = png.to_path_buf();
    app.add_observer(
        move |captured: On<ScreenshotCaptured>, readback: Res<Readback>| {
            if let Some(data) = &captured.image.data {
                *readback.0.lock().expect("readback mutex") = Some((
                    captured.image.width(),
                    captured.image.height(),
                    data.clone(),
                ));
            }
            save_to_disk(path.clone())(captured);
        },
    );
    for update in 0..MAX_CAPTURE_UPDATES {
        if update == WARMUP_FRAMES {
            app.world_mut().spawn(Screenshot::image(target.clone()));
        }
        app.update();
        if app
            .world()
            .resource::<Readback>()
            .0
            .lock()
            .expect("readback mutex")
            .is_some()
        {
            break;
        }
    }
    let Some((width, height, data)) = app
        .world()
        .resource::<Readback>()
        .0
        .lock()
        .expect("readback mutex")
        .take()
    else {
        let _ = std::fs::remove_file(png);
        return Err(SurfaceCaptureError::NoScreenshot);
    };
    if (width, height) != size || data.len() != surface.len() {
        let _ = std::fs::remove_file(png);
        return Err(SurfaceCaptureError::BadSurface);
    }
    let adapter = app
        .world()
        .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
        .map(|info| format!("{} ({:?})", info.0.name, info.0.backend))
        .unwrap_or_else(|| "no adapter reported".to_owned());

    let mut mismatched = 0;
    let mut lit = 0;
    let mut bars = 0;
    for (index, (got, want)) in data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(surface.as_chunks::<4>().0)
        .enumerate()
    {
        if got[..3]
            .iter()
            .zip(&want[..3])
            .any(|(a, b)| a.abs_diff(*b) > 2)
        {
            mismatched += 1;
        }
        let x = u32::try_from(index % size.0 as usize).unwrap_or(u32::MAX);
        let y = u32::try_from(index / size.0 as usize).unwrap_or(u32::MAX);
        let inside = x >= viewport.x
            && x < viewport.x + viewport.width
            && y >= viewport.y
            && y < viewport.y + viewport.height;
        let black = got[0] <= 2 && got[1] <= 2 && got[2] <= 2;
        if inside && !black {
            lit += 1;
        }
        if !inside && !black {
            bars += 1;
        }
    }
    let bytes = std::fs::read(png).map_err(|e| SurfaceCaptureError::Io(e.to_string()))?;
    Ok(SurfaceCapture {
        adapter,
        size,
        mismatched_pixels: mismatched,
        picture_pixels_lit: lit,
        bar_pixels_not_black: bars,
        png_sha256: sha256(&bytes),
        png_bytes: bytes.len() as u64,
    })
}
