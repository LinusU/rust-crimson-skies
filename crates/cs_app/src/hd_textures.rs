//! The optional HD texture pack (`--hd <dir>`).
//!
//! A replacement pack in the layout of the community upscale
//! <https://github.com/phly95/crimson-texture-upscale> (release v2.0.0):
//! `crimson_hd_chosen_world_textures.zip` and
//! `crimson_hd_chosen_ui_textures.zip`, kept unextracted in one directory.
//! Extracted folders in the same directory work too. Each entry's top folder
//! names the kind of art, and the rest of its path names what it replaces:
//!
//! | entry | replaces |
//! | --- | --- |
//! | `world_textures/<name>.png` | world texture `<name>` in every world group without its own entry |
//! | `world_textures/<GROUP>/<tier>/<name>.png` | world texture `<name>` in `<GROUP>` only (the tier folder is ignored) |
//! | `ui_textures/rimage/<name>.png` | the `rimage.zbd` image `<name>` |
//! | `ui_textures/rof/<path>.png` | the ROF member `<path>`, whatever its extension |
//!
//! Names match case- and extension-insensitively, as the pack's README asks.
//!
//! A replacement is a presentation enhancement, never original data. The
//! original is still resolved first, so a name the original cannot reach is
//! never drawn from the pack; only its pixels are swapped, and the
//! replacement keeps the original's coverage class (an opaque original stays
//! opaque, a keyed one takes its coverage from the replacement's alpha
//! channel). With the pack installed, every engine texture built through
//! `crate::playtest_textures::to_bevy_image` also gets a mip chain
//! ([`add_mips`]): an 8× texture without one shimmers at any distance.
//!
//! The pack is process-wide ([`install`] once from `main`, [`pack`]
//! everywhere else), so the many places that open a world's texture archive
//! need no new parameter. Without `--hd` nothing here runs.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use bevy::image::Image;
use cs_formats::AllocationBudget;
use cs_formats::texture::{
    AlphaSource, ColorSpace, DecodedImage, DescriptorParts, Extent, ImageDescriptor, PixelFormat,
    RowOrder, decode_base_level,
};
use image::RgbaImage;
use image::imageops::FilterType;
use zip::ZipArchive;

/// The command-line flag that installs the pack.
pub const HD_FLAG: &str = "--hd";

/// Top folder of the world-texture entries.
const WORLD_TOP: &str = "world_textures";
/// Top folder of the UI entries.
const UI_TOP: &str = "ui_textures";
/// Largest edge a replacement drawn through the texture decoder may keep: the texture decoder's own
/// limit. A larger replacement is downscaled to fit.
const MAX_REPLACEMENT_EDGE: u32 = cs_formats::texture::MAX_DIMENSION;

static PACK: OnceLock<HdPack> = OnceLock::new();

/// Why the pack could not be opened.
#[derive(Debug)]
pub enum HdPackError {
    /// The directory or one of its files could not be read.
    Io { path: PathBuf, reason: String },
    /// A zip in the directory is not a readable archive.
    Zip { path: PathBuf, reason: String },
    /// The directory holds no `world_textures/` or `ui_textures/` entry.
    Empty { path: PathBuf },
    /// [`install`] was called twice.
    AlreadyInstalled,
}

impl fmt::Display for HdPackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, reason } => write!(f, "HD pack {}: {reason}", path.display()),
            Self::Zip { path, reason } => {
                write!(
                    f,
                    "HD pack {}: not a readable zip: {reason}",
                    path.display()
                )
            }
            Self::Empty { path } => write!(
                f,
                "HD pack {}: no {WORLD_TOP}/ or {UI_TOP}/ entries (expected the \
                 crimson_hd_chosen_*_textures.zip files, unextracted or extracted)",
                path.display()
            ),
            Self::AlreadyInstalled => write!(f, "the HD pack is already installed"),
        }
    }
}

impl std::error::Error for HdPackError {}

/// Where one entry's bytes live.
#[derive(Clone, Debug)]
enum Source {
    /// Entry `name` of the pack's zip number `archive`.
    Zip { archive: usize, name: String },
    /// A loose file of an extracted pack.
    File(PathBuf),
}

/// An opened pack: an index of every entry, its bytes read on demand.
pub struct HdPack {
    dir: PathBuf,
    archives: Vec<Mutex<ZipArchive<File>>>,
    /// `(group, name)`: the group-specific entries.
    world_grouped: HashMap<(String, String), Source>,
    /// `name`: the entries shared by every group without its own.
    world_shared: HashMap<String, Source>,
    /// `rimage/<name>` or `rof/<path>`, lower case, extension dropped.
    ui: HashMap<String, Source>,
}

impl fmt::Debug for HdPack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HdPack")
            .field("dir", &self.dir)
            .field("world_entries", &self.world_entries())
            .field("ui_entries", &self.ui.len())
            .finish_non_exhaustive()
    }
}

/// Opens `dir` and makes it the process-wide pack.
///
/// # Errors
///
/// What [`HdPack::open`] refuses, or [`HdPackError::AlreadyInstalled`].
pub fn install(dir: &Path) -> Result<&'static HdPack, HdPackError> {
    let opened = HdPack::open(dir)?;
    PACK.set(opened)
        .map_err(|_| HdPackError::AlreadyInstalled)?;
    Ok(PACK.get().expect("just installed"))
}

/// The installed pack, if `--hd` was given.
#[must_use]
pub fn pack() -> Option<&'static HdPack> {
    PACK.get()
}

/// Removes every `--hd <dir>` pair from `args` and returns the last
/// directory named. The flag is process-wide, so it is taken off before the
/// run-mode parser sees the vector.
///
/// # Errors
///
/// A message when `--hd` is the last argument.
pub fn take_flag(args: &mut Vec<String>) -> Result<Option<PathBuf>, String> {
    let mut dir = None;
    while let Some(at) = args.iter().position(|arg| arg == HD_FLAG) {
        if at + 1 >= args.len() {
            return Err(format!(
                "{HD_FLAG} expects the directory holding the HD pack"
            ));
        }
        let value = args.remove(at + 1);
        args.remove(at);
        dir = Some(PathBuf::from(value));
    }
    Ok(dir)
}

/// Appends `--hd <dir>` to a command line this process hands over to a new
/// copy of itself, so the pack survives the start flow's mission hand-over.
pub fn forward_flag(args: &mut Vec<String>) {
    if let Some(pack) = pack() {
        args.push(HD_FLAG.to_owned());
        args.push(pack.dir.display().to_string());
    }
}

impl HdPack {
    /// Indexes every `*.zip` in `dir` and every loose file under its
    /// `world_textures/` and `ui_textures/` folders.
    ///
    /// # Errors
    ///
    /// [`HdPackError::Io`] / [`HdPackError::Zip`] when something cannot be
    /// read, [`HdPackError::Empty`] when nothing in `dir` is pack art.
    pub fn open(dir: &Path) -> Result<Self, HdPackError> {
        let io = |path: &Path, error: std::io::Error| HdPackError::Io {
            path: path.to_owned(),
            reason: error.to_string(),
        };
        let mut pack = Self {
            dir: dir.to_owned(),
            archives: Vec::new(),
            world_grouped: HashMap::new(),
            world_shared: HashMap::new(),
            ui: HashMap::new(),
        };
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|error| io(dir, error))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        entries.sort();
        for path in entries {
            let is_zip = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
            if is_zip {
                let file = File::open(&path).map_err(|error| io(&path, error))?;
                let archive = ZipArchive::new(file).map_err(|error| HdPackError::Zip {
                    path: path.clone(),
                    reason: error.to_string(),
                })?;
                let index = pack.archives.len();
                let names: Vec<String> = archive
                    .file_names()
                    .filter_map(|name| name.ok().map(std::borrow::Cow::into_owned))
                    .collect();
                for name in names {
                    if !name.ends_with('/') {
                        pack.add(
                            &name,
                            Source::Zip {
                                archive: index,
                                name: name.clone(),
                            },
                        );
                    }
                }
                pack.archives.push(Mutex::new(archive));
            } else if path.is_dir() {
                let mut files = Vec::new();
                walk(&path, &mut files).map_err(|error| io(&path, error))?;
                for file in files {
                    let Ok(relative) = file.strip_prefix(dir) else {
                        continue;
                    };
                    let spelling = relative.to_string_lossy().replace('\\', "/");
                    pack.add(&spelling, Source::File(file.clone()));
                }
            }
        }
        if pack.world_entries() == 0 && pack.ui.is_empty() {
            return Err(HdPackError::Empty {
                path: dir.to_owned(),
            });
        }
        Ok(pack)
    }

    fn add(&mut self, spelling: &str, source: Source) {
        let mut parts = spelling.split('/').filter(|part| !part.is_empty());
        let Some(top) = parts.next() else {
            return;
        };
        let rest: Vec<&str> = parts.collect();
        let Some((file, folders)) = rest.split_last() else {
            return;
        };
        let stem = match file.rfind('.') {
            Some(dot) if dot > 0 => &file[..dot],
            _ => file,
        }
        .to_ascii_lowercase();
        if top.eq_ignore_ascii_case(WORLD_TOP) {
            match folders {
                [] => {
                    self.world_shared.insert(stem, source);
                }
                [group, ..] => {
                    self.world_grouped
                        .insert((group.to_ascii_lowercase(), stem), source);
                }
            }
        } else if top.eq_ignore_ascii_case(UI_TOP) {
            let mut key: Vec<String> = folders.iter().map(|f| f.to_ascii_lowercase()).collect();
            key.push(stem);
            self.ui.insert(key.join("/"), source);
        }
    }

    /// The directory the pack was opened from.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// How many world-texture entries the pack indexes.
    #[must_use]
    pub fn world_entries(&self) -> usize {
        self.world_grouped.len() + self.world_shared.len()
    }

    /// How many UI entries the pack indexes.
    #[must_use]
    pub fn ui_entries(&self) -> usize {
        self.ui.len()
    }

    fn read(&self, source: &Source) -> Option<Vec<u8>> {
        match source {
            Source::File(path) => std::fs::read(path).ok(),
            Source::Zip { archive, name } => {
                let mut archive = self.archives.get(*archive)?.lock().ok()?;
                let mut entry = archive.by_name(name).ok()?;
                let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).ok()?);
                entry.read_to_end(&mut bytes).ok()?;
                Some(bytes)
            }
        }
    }

    fn decode(&self, source: &Source, label: &str) -> Option<RgbaImage> {
        let bytes = self.read(source)?;
        match image::load_from_memory(&bytes) {
            Ok(decoded) => Some(decoded.into_rgba8()),
            Err(error) => {
                eprintln!(
                    "cs: HD pack entry {label} was not decoded ({error}); the original is drawn"
                );
                None
            }
        }
    }

    /// The replacement for world texture `name` (the projected texture name,
    /// e.g. `cliff01`) in world group `group` (e.g. `c1c`).
    #[must_use]
    pub fn world_texture(&self, group: &str, name: &str) -> Option<RgbaImage> {
        let name = name.to_ascii_lowercase();
        let source = self
            .world_grouped
            .get(&(group.to_ascii_lowercase(), name.clone()))
            .or_else(|| self.world_shared.get(&name))?;
        self.decode(source, &format!("{WORLD_TOP} {group}/{name}"))
    }

    /// The replacement for a UI image. `key` is `rimage/<name>` for an
    /// `rimage.zbd` image or `rof/<member path>` for a ROF member; case,
    /// backslashes and the extension do not matter.
    #[must_use]
    pub fn ui_image(&self, key: &str) -> Option<RgbaImage> {
        let key = ui_key(key);
        let source = self.ui.get(&key)?;
        self.decode(source, &format!("{UI_TOP} {key}"))
    }

    /// Whether the pack holds a replacement for UI image `key` (see
    /// [`Self::ui_image`]), without decoding it.
    #[must_use]
    pub fn has_ui_image(&self, key: &str) -> bool {
        self.ui.contains_key(&ui_key(key))
    }

    /// `original` with its pixels swapped for the pack's replacement of world
    /// texture `name` in `group`, or `None` when the pack has none (or it
    /// cannot be decoded). The replacement keeps the original's coverage
    /// class: opaque stays opaque, anything keyed reads the replacement's
    /// alpha channel.
    #[must_use]
    pub fn world_decoded(
        &self,
        group: &str,
        name: &str,
        original: &DecodedImage,
    ) -> Option<DecodedImage> {
        let rgba = self.world_texture(group, name)?;
        replacement_decoded(rgba, &format!("{WORLD_TOP}/{group}/{name}"), original)
    }

    /// As [`Self::world_decoded`], for the UI image `key` (see
    /// [`Self::ui_image`]), for UI art drawn through the texture path.
    #[must_use]
    pub fn ui_decoded(&self, key: &str, original: &DecodedImage) -> Option<DecodedImage> {
        let rgba = self.ui_image(key)?;
        replacement_decoded(rgba, &format!("{UI_TOP}/{}", ui_key(key)), original)
    }
}

/// `rgba` as a decoded image with `original`'s coverage class and alpha test.
fn replacement_decoded(
    mut rgba: RgbaImage,
    label: &str,
    original: &DecodedImage,
) -> Option<DecodedImage> {
    if rgba.width() > MAX_REPLACEMENT_EDGE || rgba.height() > MAX_REPLACEMENT_EDGE {
        let scale = f64::from(MAX_REPLACEMENT_EDGE) / f64::from(rgba.width().max(rgba.height()));
        let width = scaled(rgba.width(), scale);
        let height = scaled(rgba.height(), scale);
        rgba = image::imageops::resize(&rgba, width, height, FilterType::Triangle);
    }
    let alpha_source = match original.alpha_source() {
        AlphaSource::Opaque => AlphaSource::Opaque,
        _ => AlphaSource::Channel,
    };
    let descriptor = ImageDescriptor::new(DescriptorParts {
        extent: Extent {
            width: rgba.width(),
            height: rgba.height(),
        },
        format: PixelFormat::Rgba8,
        row_order: RowOrder::TopDown,
        palette: None,
        mips: Vec::new(),
        alpha_source,
        alpha_test: original.alpha_test(),
        color_space: ColorSpace::Srgb,
    })
    .ok()?;
    let mut budget = AllocationBudget::with_defaults(label);
    decode_base_level(label, &descriptor, rgba.as_raw(), &mut budget).ok()
}

fn scaled(edge: u32, scale: f64) -> u32 {
    // Rounded and kept at least one texel; the edge is at most `u32::MAX`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let value = (f64::from(edge) * scale).round() as u32;
    value.max(1)
}

fn ui_key(key: &str) -> String {
    let key = key.replace('\\', "/").to_ascii_lowercase();
    let (folders, file) = key.rsplit_once('/').unwrap_or(("", key.as_str()));
    let stem = match file.rfind('.') {
        Some(dot) if dot > 0 => &file[..dot],
        _ => file,
    };
    if folders.is_empty() {
        stem.to_owned()
    } else {
        format!("{folders}/{stem}")
    }
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Gives a single-level 2D RGBA8 `image` a full mip chain, each level a
/// triangle-filtered halving of the one above, and samples it trilinearly.
/// Anything else is left as it is.
pub fn add_mips(image: &mut Image) {
    use bevy::image::{ImageFilterMode, ImageSampler};
    use bevy::render::render_resource::{TextureDimension, TextureFormat};

    let descriptor = &image.texture_descriptor;
    let rgba8 = matches!(
        descriptor.format,
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
    );
    if !rgba8
        || descriptor.dimension != TextureDimension::D2
        || descriptor.mip_level_count != 1
        || descriptor.size.depth_or_array_layers != 1
    {
        return;
    }
    let (width, height) = (descriptor.size.width, descriptor.size.height);
    let Some(data) = image.data.take() else {
        return;
    };
    let Some(mut level) = RgbaImage::from_raw(width, height, data.clone()) else {
        image.data = Some(data);
        return;
    };
    let mut chain = data;
    let mut levels = 1;
    while level.width() > 1 || level.height() > 1 {
        let next_width = (level.width() / 2).max(1);
        let next_height = (level.height() / 2).max(1);
        level = image::imageops::resize(&level, next_width, next_height, FilterType::Triangle);
        chain.extend_from_slice(level.as_raw());
        levels += 1;
    }
    image.data = Some(chain);
    image.texture_descriptor.mip_level_count = levels;
    if let ImageSampler::Descriptor(sampler) = &mut image.sampler {
        sampler.mipmap_filter = ImageFilterMode::Linear;
        sampler.anisotropy_clamp = 16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_flag_is_taken_off_the_command_line() {
        let mut args: Vec<String> = ["--cs-path", "/game", "--hd", "/hd", "--mission", "M01"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(take_flag(&mut args), Ok(Some(PathBuf::from("/hd"))));
        assert_eq!(args, ["--cs-path", "/game", "--mission", "M01"]);
        let mut dangling = vec!["--hd".to_owned()];
        assert!(take_flag(&mut dangling).is_err());
    }

    #[test]
    fn hd_entries_index_by_kind_group_and_folded_name() {
        let mut pack = HdPack {
            dir: PathBuf::new(),
            archives: Vec::new(),
            world_grouped: HashMap::new(),
            world_shared: HashMap::new(),
            ui: HashMap::new(),
        };
        let file = |name: &str| Source::File(PathBuf::from(name));
        pack.add("world_textures/Cloud1.png", file("shared"));
        pack.add("world_textures/C1/rtexture15/cloud1.png", file("c1"));
        pack.add("ui_textures/rimage/Arrow.png", file("arrow"));
        pack.add(
            "ui_textures/rof/ASSETS/GRAPHICS/MM_BACKGROUND.PNG",
            file("mm"),
        );
        pack.add("README.txt", file("ignored"));
        assert_eq!(pack.world_entries(), 2);
        assert!(
            pack.world_grouped
                .contains_key(&("c1".to_owned(), "cloud1".to_owned()))
        );
        assert!(pack.world_shared.contains_key("cloud1"));
        assert!(pack.has_ui_image("rimage/arrow.tif"));
        assert!(pack.has_ui_image("rof/assets\\graphics\\mm_background.tga"));
        assert!(!pack.has_ui_image("rof/assets/graphics/other.tga"));
    }
}
