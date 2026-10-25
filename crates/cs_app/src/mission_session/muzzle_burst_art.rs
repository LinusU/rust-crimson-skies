//! The measured muzzle-burst art, loaded from the owner's retail files
//! (task #2105, `F27-C-FX-ORIG-DRAW-B`).
//!
//! `docs/findings/2026-10-24-f27-c-fx-orig-muzzle-effect-binding.md` measured
//! that a weapon row binds its muzzle flash **by name**: `weapons.zrd`
//! `BALLISTICS > wep_NN > FIRE > ANIMATION` names an `ANIMATION_DEFINITION` of
//! `muzzle_burst.zrd`, whose `NAME` is also a root node of the world's
//! `gamez.zbd` with two children — `mb1` (one quad) and `mb_spinflame`
//! (three quads) — textured `{slug,dum,ap,mag}_muzzle1/2.tif`.
//!
//! [`load_muzzle_burst_art`] reads exactly that chain and returns the plain
//! data a later task draws ([`MuzzleBurstArt`]). Nothing here draws, and
//! nothing is committed: every byte is read from `$CS_GAME_DIR` at load.
//!
//! A missing binding, definition, node, polygon or texture is refused **by
//! name** ([`MuzzleBurstArtError`]); there is no fallback to the designed
//! stand-in of [`super::weapon_present::resolve_muzzle_drawable`].
//!
//! Still unmeasured (see the finding): which axis of a gun mount the node's +z
//! is, and the blend behaviour of material flags 17. This module records
//! neither.

use std::fmt;
use std::path::Path;

use cs_content::stunts::decode_zrd;
use cs_formats::ParseContext;
use cs_formats::gamez::{read_gamez_materials, read_gamez_meshes, read_gamez_nodes};
use cs_formats::texture::{DecodedImage, read_zbd_textures};
use cs_formats::zbd::{EffectEvent, ReaderNode, read_animation_definitions, read_reader_nodes};

use super::gun_sound::{READER_ARCHIVE, WEAPONS_MEMBER, first_text, value_of};
use super::launch_sound::reader_member;

/// The reader-archive member that defines the `muzzle_burst*` animations.
pub const MUZZLE_BURST_MEMBER: &str = "muzzle_burst.zrd";
/// The gun-kind bursts whose two-node art the finding measured.
pub const GUN_BURST_NAMES: [&str; 4] = [
    "muzzle_burst_slug",
    "muzzle_burst_dum",
    "muzzle_burst_ap",
    "muzzle_burst_mag",
];
/// The child node holding the single quad.
pub const FLASH_NODE: &str = "mb1";
/// The child node holding the three rolled quads.
pub const SPIN_NODE: &str = "mb_spinflame";

/// One textured quad of a burst node, in the node's local space.
#[derive(Clone, Debug, PartialEq)]
pub struct BurstQuad {
    /// The four corners in stored order, as stored.
    pub corners: [[f32; 3]; 4],
    /// The stored texture coordinate of each corner (material group 0).
    pub uvs: [[f32; 2]; 4],
    /// The texture the quad's material names, e.g. `slug_muzzle1.tif`.
    pub texture: String,
}

/// One burst child node's geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct BurstNodeArt {
    /// The quads, in stored polygon order.
    pub quads: Vec<BurstQuad>,
}

/// One decoded burst texture.
#[derive(Clone, Debug, PartialEq)]
pub struct BurstTexture {
    /// The stored name, e.g. `slug_muzzle1.tif`.
    pub name: String,
    /// The decoded image.
    pub image: DecodedImage,
}

/// The art one weapon row's muzzle burst needs.
#[derive(Clone, Debug, PartialEq)]
pub struct MuzzleBurstArt {
    /// The weapon record, e.g. `wep_50`.
    pub weapon: String,
    /// Its `FIRE > ANIMATION` name and the gamez root node of that name,
    /// e.g. `muzzle_burst_slug`.
    pub burst: String,
    /// The world group the nodes and textures were read from, e.g. `C1`.
    pub world_group: String,
    /// The texture archive the textures came from (installation-relative).
    pub texture_archive: String,
    /// `mb1`: one quad.
    pub flash: BurstNodeArt,
    /// `mb_spinflame`: three quads 60 degrees apart about z.
    pub spin: BurstNodeArt,
    /// The roll about z, in degrees, the definition's `OBJECT_ROTATE_STATE`
    /// events turn `mb_spinflame` to; one is chosen per shot, each with
    /// `RANDOM_WEIGHT` 0.333 as measured.
    pub spin_rolls_deg: Vec<f32>,
    /// The decoded `*_muzzle1.tif` and `*_muzzle2.tif` textures.
    pub textures: Vec<BurstTexture>,
}

/// Why the burst art could not be loaded. Every variant names what is missing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MuzzleBurstArtError {
    /// A file or archive could not be read or decoded.
    Read(String),
    /// The weapon record has no `FIRE > ANIMATION` binding.
    NoBinding {
        /// The weapon record.
        weapon: String,
    },
    /// The bound animation is not one of the four measured gun bursts.
    NotAGunBurst {
        /// The weapon record.
        weapon: String,
        /// The animation it names.
        animation: String,
    },
    /// The animation name is not defined exactly once in `muzzle_burst.zrd`.
    Definition {
        /// The animation name.
        animation: String,
        /// How many definitions carry it.
        found: usize,
    },
    /// The definition does not state the three spin rolls.
    SpinRolls {
        /// The animation name.
        animation: String,
    },
    /// A scene node, its mesh, a polygon or a material is missing or not the
    /// measured shape.
    Node {
        /// The node path, e.g. `muzzle_burst_slug/mb1`.
        node: String,
        /// What is wrong.
        reason: String,
    },
    /// A texture is not in the world's texture archive.
    Texture {
        /// The texture name.
        name: String,
        /// What is wrong.
        reason: String,
    },
}

impl fmt::Display for MuzzleBurstArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "muzzle burst art unreadable: {reason}"),
            Self::NoBinding { weapon } => {
                write!(f, "{weapon} has no FIRE ANIMATION muzzle_burst binding")
            }
            Self::NotAGunBurst { weapon, animation } => write!(
                f,
                "{weapon} binds {animation}, which is not a measured gun burst"
            ),
            Self::Definition { animation, found } => write!(
                f,
                "{animation} is defined {found} times in {MUZZLE_BURST_MEMBER}, expected once"
            ),
            Self::SpinRolls { animation } => {
                write!(f, "{animation} states no OBJECT_ROTATE_STATE spin rolls")
            }
            Self::Node { node, reason } => write!(f, "gamez node {node}: {reason}"),
            Self::Texture { name, reason } => write!(f, "texture {name}: {reason}"),
        }
    }
}

impl std::error::Error for MuzzleBurstArtError {}

/// The `FIRE > ANIMATION` name `weapon` binds in a decoded `weapons.zrd`.
///
/// # Errors
///
/// [`MuzzleBurstArtError::NoBinding`] when the record, its `FIRE` block or the
/// `ANIMATION` key is absent.
pub fn fire_animation_of(
    weapons: &cs_content::stunts::ZrdValue,
    weapon: &str,
) -> Result<String, MuzzleBurstArtError> {
    let missing = || MuzzleBurstArtError::NoBinding {
        weapon: weapon.to_owned(),
    };
    let ballistics = value_of(weapons, "BALLISTICS").ok_or_else(missing)?;
    let record = value_of(ballistics, weapon).ok_or_else(missing)?;
    let fire = value_of(record, "FIRE").ok_or_else(missing)?;
    let animation = value_of(fire, "ANIMATION").ok_or_else(missing)?;
    first_text(animation).map(str::to_owned).ok_or_else(missing)
}

fn read_error(what: &str, error: impl fmt::Display) -> MuzzleBurstArtError {
    MuzzleBurstArtError::Read(format!("{what}: {error}"))
}

/// The `OBJECT_ROTATE_STATE` roll angles of the burst's definition, in order.
fn spin_rolls(bytes: &[u8], animation: &str) -> Result<Vec<f32>, MuzzleBurstArtError> {
    let nodes = read_reader_nodes(bytes).map_err(|e| read_error(MUZZLE_BURST_MEMBER, e))?;
    let defs =
        read_animation_definitions(&nodes).map_err(|e| read_error(MUZZLE_BURST_MEMBER, e))?;
    let matching: Vec<_> = defs.iter().filter(|d| d.name == animation).collect();
    let [definition] = matching.as_slice() else {
        return Err(MuzzleBurstArtError::Definition {
            animation: animation.to_owned(),
            found: matching.len(),
        });
    };
    let mut rolls = Vec::new();
    for sequence in &definition.sequences {
        for event in &sequence.events {
            if let EffectEvent::Other { keyword, value } = event
                && *keyword == "OBJECT_ROTATE_STATE"
            {
                let state: Vec<f32> = value
                    .field("STATE")
                    .and_then(ReaderNode::as_list)
                    .map(|list| list.iter().filter_map(ReaderNode::as_number).collect())
                    .unwrap_or_default();
                if let [0.0, 0.0, roll] = state[..] {
                    rolls.push(roll);
                }
            }
        }
    }
    if rolls.is_empty() {
        return Err(MuzzleBurstArtError::SpinRolls {
            animation: animation.to_owned(),
        });
    }
    Ok(rolls)
}

fn host_file(
    install_root: &Path,
    found: &cs_assets::install::Discovery,
    logical: &str,
) -> Result<(String, Vec<u8>), MuzzleBurstArtError> {
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == logical)
        .ok_or_else(|| MuzzleBurstArtError::Read(format!("{logical} is not in the manifest")))?;
    let spelling = record.relative_spelling.as_str().to_owned();
    let bytes = std::fs::read(install_root.join(&spelling))
        .map_err(|error| read_error(&spelling, error))?;
    Ok((spelling, bytes))
}

/// The highest-numbered `rtexture<N>.zbd` of `group_lower`, else `texture.zbd`
/// (the playtest's declared archive choice, see
/// `crate::playtest_textures::PLAYTEST_TEXTURE_ARCHIVE_IS_DESIGNED`).
fn texture_archive_of(found: &cs_assets::install::Discovery, group_lower: &str) -> Option<String> {
    let prefix = format!("zbd/{group_lower}/");
    let mut tiers: Vec<(u32, String)> = Vec::new();
    let mut primary = None;
    for record in &found.manifest.files {
        let logical = record.relative_spelling.logical_key();
        let Some(name) = logical.strip_prefix(&prefix) else {
            continue;
        };
        if name == "texture.zbd" {
            primary = Some(logical.to_owned());
        } else if let Some(number) = name
            .strip_prefix("rtexture")
            .and_then(|rest| rest.strip_suffix(".zbd"))
            .and_then(|digits| digits.parse::<u32>().ok())
        {
            tiers.push((number, logical.to_owned()));
        }
    }
    tiers.sort();
    tiers.pop().map(|(_, logical)| logical).or(primary)
}

/// Loads the muzzle-burst art `weapon`'s row binds, from the world group
/// `world_group` (e.g. `C1`) of the installation at `install_root`.
///
/// # Errors
///
/// [`MuzzleBurstArtError`], naming the first binding, definition, node,
/// polygon or texture that is missing or not the measured shape.
pub fn load_muzzle_burst_art(
    install_root: &Path,
    world_group: &str,
    weapon: &str,
) -> Result<MuzzleBurstArt, MuzzleBurstArtError> {
    let weapons_bytes = reader_member(install_root, READER_ARCHIVE, WEAPONS_MEMBER)
        .map_err(MuzzleBurstArtError::Read)?;
    let weapons = decode_zrd(&weapons_bytes).map_err(|e| read_error(WEAPONS_MEMBER, e))?;
    let burst = fire_animation_of(&weapons, weapon)?;
    if !GUN_BURST_NAMES.contains(&burst.as_str()) {
        return Err(MuzzleBurstArtError::NotAGunBurst {
            weapon: weapon.to_owned(),
            animation: burst,
        });
    }
    let definitions = reader_member(install_root, READER_ARCHIVE, MUZZLE_BURST_MEMBER)
        .map_err(MuzzleBurstArtError::Read)?;
    let spin_rolls_deg = spin_rolls(&definitions, &burst)?;

    let found = cs_assets::install::discover(install_root)
        .map_err(|e| read_error("the installation cannot be discovered", e))?;
    let lower = world_group.to_ascii_lowercase();
    let gamez_path = format!("zbd/{lower}/gamez.zbd");
    let (gamez_label, gamez) = host_file(install_root, &found, &gamez_path)?;
    let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(&gamez_label), &gamez)
        .map_err(|e| read_error(&gamez_label, e))?;
    let meshes = read_gamez_meshes(
        &mut ParseContext::with_defaults(&gamez_label),
        &gamez_label,
        &gamez,
    )
    .map_err(|e| read_error(&gamez_label, e))?;
    let materials = read_gamez_materials(
        &mut ParseContext::with_defaults(&gamez_label),
        &gamez_label,
        &gamez,
    )
    .map_err(|e| read_error(&gamez_label, e))?;

    let root = nodes
        .nodes
        .iter()
        .find(|node| node.name == burst)
        .ok_or_else(|| MuzzleBurstArtError::Node {
            node: burst.clone(),
            reason: format!("no such node in {gamez_label}"),
        })?;
    let child_art = |label: &str, quads: usize| -> Result<BurstNodeArt, MuzzleBurstArtError> {
        let path = format!("{burst}/{label}");
        let fail = |reason: String| MuzzleBurstArtError::Node {
            node: path.clone(),
            reason,
        };
        let node = root
            .children
            .iter()
            .filter_map(|child| nodes.get(*child))
            .find(|node| node.name == label)
            .ok_or_else(|| fail("no such child".to_owned()))?;
        let mesh_index = u32::try_from(node.info.mesh_index)
            .map_err(|_| fail("the node binds no mesh".to_owned()))?;
        let mesh = meshes
            .get(mesh_index)
            .ok_or_else(|| fail(format!("mesh {mesh_index} is absent")))?;
        if mesh.mesh.polygons.len() != quads {
            return Err(fail(format!(
                "{} polygons, expected {quads}",
                mesh.mesh.polygons.len()
            )));
        }
        let mut out = Vec::new();
        for (index, polygon) in mesh.mesh.polygons.iter().enumerate() {
            if polygon.corners.len() != 4 {
                return Err(fail(format!(
                    "polygon {index} has {} corners, expected 4",
                    polygon.corners.len()
                )));
            }
            let mut corners = [[0.0; 3]; 4];
            let mut uvs = [[0.0; 2]; 4];
            for (slot, corner) in polygon.corners.iter().enumerate() {
                corners[slot] = *mesh
                    .mesh
                    .positions
                    .get(corner.position as usize)
                    .ok_or_else(|| {
                        fail(format!("polygon {index} corner {slot} is off the mesh"))
                    })?;
                uvs[slot] = corner
                    .uv
                    .ok_or_else(|| fail(format!("polygon {index} corner {slot} has no uv")))?;
            }
            let texture = materials
                .material(polygon.material)
                .and_then(|material| materials.texture_of(material))
                .map(|texture| texture.name.clone())
                .ok_or_else(|| fail(format!("polygon {index} material names no texture")))?;
            out.push(BurstQuad {
                corners,
                uvs,
                texture,
            });
        }
        Ok(BurstNodeArt { quads: out })
    };
    let flash = child_art(FLASH_NODE, 1)?;
    let spin = child_art(SPIN_NODE, 3)?;

    let archive_path =
        texture_archive_of(&found, &lower).ok_or_else(|| MuzzleBurstArtError::Texture {
            name: "*_muzzle*.tif".to_owned(),
            reason: format!("{world_group} has no texture archive"),
        })?;
    let (archive_label, archive_bytes) = host_file(install_root, &found, &archive_path)?;
    let mut budget = cs_formats::AllocationBudget::with_defaults("muzzle-burst-art");
    let package = read_zbd_textures(&archive_label, &archive_bytes, &mut budget)
        .map_err(|e| read_error(&archive_label, e))?;
    let mut names: Vec<String> = Vec::new();
    for quad in flash.quads.iter().chain(&spin.quads) {
        if !names.contains(&quad.texture) {
            names.push(quad.texture.clone());
        }
    }
    let mut textures = Vec::new();
    for name in names {
        // The archive stores the name without its extension: the playtest's
        // declared reading (first `.`, lower case) joins the two spellings.
        let projected = crate::playtest_textures::NAME_READING.project(&name);
        let stored =
            package
                .named(&projected)
                .next()
                .ok_or_else(|| MuzzleBurstArtError::Texture {
                    name: name.clone(),
                    reason: format!("{projected} is not stored in {archive_label}"),
                })?;
        let image = stored
            .decode(&mut budget)
            .map_err(|error| MuzzleBurstArtError::Texture {
                name: name.clone(),
                reason: error.to_string(),
            })?;
        textures.push(BurstTexture { name, image });
    }

    Ok(MuzzleBurstArt {
        weapon: weapon.to_owned(),
        burst,
        world_group: world_group.to_owned(),
        texture_archive: archive_label,
        flash,
        spin,
        spin_rolls_deg,
        textures,
    })
}
