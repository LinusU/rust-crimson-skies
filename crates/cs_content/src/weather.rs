//! The mission `weather.zrd` document and its binding to an
//! [`EnvironmentDefinition`] (task #636, `M01-LC-WEATHER-BIND`).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md` (non-negotiable
//! behavior 1: unknown weather tuning remains unknown). Findings:
//! `docs/findings/2026-10-05-m01-lc-weather-bind.md`.
//!
//! # Two halves
//!
//! * [`WeatherDocument::from_zrd`] reads the decoded `.zrd` tree into typed
//!   records **strictly**: a key the grammar below does not name, a repeated
//!   key or a value of the wrong shape is refused with a [`WeatherError`] that
//!   names it. Numbers keep the encoding the file used ([`WeatherScalar`]),
//!   because the retail files mix integer and float spellings of the same
//!   field.
//! * [`bind_weather`] turns a document into an [`EnvironmentDefinition`] and
//!   an [`UnboundField`] list. A value is bound **known** only when the file
//!   itself decides it; everything whose unit, axis convention, color space or
//!   consumer is unmeasured stays an explicit unknown in the definition, with
//!   the raw record still in the document, and is *named* in the list. No
//!   constructor here converts a fog or clip range into a gameplay sight
//!   range.
//!
//! What is bound known: the precipitation kind (`TYPE`), and its absence as
//! clear air (inferred, tagged so). What is not claimed: any wind, sun, fog,
//! cloud or visibility number, because the original's world-unit scale and
//! its angle/axis conventions are unmeasured (see the F18 and F19 findings).

use std::collections::BTreeMap;
use std::fmt;

use cs_types::asset_id::SourceSpan;
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::environment::{
    EnvironmentDefinition, EnvironmentError, EnvironmentId, EnvironmentKeyError,
    EnvironmentProfile, EnvironmentState, EnvironmentTimeline, FogDefinition, GameplayVisibility,
    LightingDefinition, PrecipitationDefinition, PrecipitationKind, SkyArt, SkyOrientation,
    WindField,
};
use crate::stunts::{ZrdValue, zrd_flat_fields};

/// One number as the file spelled it.
///
/// The retail documents spell the same field both ways (`FOG_ALTITUDE`
/// `[4000 5000]` beside `[970.0f 1047.0f]`), so the spelling is kept rather
/// than normalized away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeatherScalar {
    /// A tag-1 `u32`.
    Int(u32),
    /// A tag-2 `f32`.
    Float(f32),
}

impl WeatherScalar {
    /// The value as `f64`. An integer is its **unsigned** value; see
    /// [`WeatherScalar::signed`] for angles.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Int(value) => f64::from(value),
            Self::Float(value) => f64::from(value),
        }
    }

    /// The value with an integer read as two's-complement `i32`.
    ///
    /// Observed: `SUNLIGHT_ORIENTATION [4294967271 4294967161 0.0f]` in the
    /// installation is `-25, -135` as `i32`; the float spelling of the same
    /// field in sibling zones is negative. That is an observation about
    /// spelling, not a claim about what the angles mean.
    #[must_use]
    #[allow(clippy::cast_possible_wrap)]
    pub fn signed(self) -> f64 {
        match self {
            Self::Int(value) => f64::from(value as i32),
            Self::Float(value) => f64::from(value),
        }
    }
}

/// Why a weather document was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WeatherError {
    /// The decoded tree has no record child.
    NoRecord,
    /// A key the grammar does not name.
    UnknownKey {
        /// Where it was found (`""` for the top level, else the parent key).
        scope: String,
        /// The key.
        key: String,
    },
    /// A key that appears twice in one scope.
    DuplicateKey {
        /// The scope.
        scope: String,
        /// The key.
        key: String,
    },
    /// A required key is absent.
    MissingKey {
        /// The scope.
        scope: String,
        /// The key.
        key: &'static str,
    },
    /// A value is not the shape the field has everywhere else.
    BadShape {
        /// The scope.
        scope: String,
        /// The key.
        key: String,
        /// What was expected.
        expected: &'static str,
    },
    /// A `TYPE` value that is neither `RAIN` nor `SNOW`.
    UnknownPrecipitationType {
        /// The spelling.
        text: String,
    },
    /// The mission key does not make a valid environment id.
    Id(EnvironmentKeyError),
    /// A claim id was malformed (an internal error).
    Claim(String),
    /// The assembled record was refused.
    Environment(EnvironmentError),
}

impl fmt::Display for WeatherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRecord => write!(f, "the weather member has no record node"),
            Self::UnknownKey { scope, key } => {
                write!(f, "unknown weather key {key:?} in scope {scope:?}")
            }
            Self::DuplicateKey { scope, key } => {
                write!(f, "weather key {key:?} repeats in scope {scope:?}")
            }
            Self::MissingKey { scope, key } => {
                write!(f, "weather key {key:?} is missing in scope {scope:?}")
            }
            Self::BadShape {
                scope,
                key,
                expected,
            } => write!(f, "weather key {key:?} in {scope:?} is not {expected}"),
            Self::UnknownPrecipitationType { text } => {
                write!(f, "unknown precipitation TYPE {text:?}")
            }
            Self::Id(error) => write!(f, "{error}"),
            Self::Claim(text) => write!(f, "malformed claim id: {text}"),
            Self::Environment(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for WeatherError {}

/// `VIEWING_RANGE` tier scales (`HIGH`, `SWHIGH`, `MED`, `SWMED`, `LOW`,
/// `SWLOW`): renderer detail multipliers, not gameplay values.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewingRangeTier {
    /// The tier name as spelled.
    pub name: String,
    /// `CLIP_SCALE`.
    pub clip_scale: WeatherScalar,
    /// `FOG_SCALE`.
    pub fog_scale: WeatherScalar,
}

/// The `WIND` record, in the file's own units and axes.
#[derive(Clone, Debug, PartialEq)]
pub struct WindRecord {
    /// `STATIC_VELOCITY`.
    pub static_velocity: [WeatherScalar; 3],
    /// `RANDOM_MAX_SPEED`.
    pub random_max_speed: WeatherScalar,
    /// `RANDOM_ACCEL`.
    pub random_accel: WeatherScalar,
    /// `RANDOM_ANG_VEL`.
    pub random_ang_vel: WeatherScalar,
}

/// The `CLOUD_COVER` record, in the file's own units.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudCoverRecord {
    /// `TOP`.
    pub top: WeatherScalar,
    /// `BOTTOM`.
    pub bottom: WeatherScalar,
    /// `THICKNESS`.
    pub thickness: WeatherScalar,
    /// `TOP_COLOR`, present in some documents.
    pub top_color: Option<[WeatherScalar; 3]>,
    /// `BOTTOM_COLOR`, present in some documents.
    pub bottom_color: Option<[WeatherScalar; 3]>,
}

/// One altitude zone (`ZONEn`, or its `SW_ZONEn` twin).
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneRecord {
    /// The key as spelled (`ZONE1`, `SW_ZONE2`, ...).
    pub name: String,
    /// `FOG_COLOR`.
    pub fog_color: [WeatherScalar; 3],
    /// `FOG_RANGES`.
    pub fog_ranges: [WeatherScalar; 2],
    /// `FOG_ALTITUDE`; absent in some zones (what absence means is
    /// unmeasured).
    pub fog_altitude: Option<[WeatherScalar; 2]>,
    /// `CLIP_RANGES`.
    pub clip_ranges: [WeatherScalar; 2],
    /// `SUNLIGHT_ACTIVE`.
    pub sunlight_active: WeatherScalar,
    /// `SUNLIGHT_ORIENTATION`.
    pub sunlight_orientation: [WeatherScalar; 3],
    /// `SUNLIGHT_DIFFUSE`.
    pub sunlight_diffuse: WeatherScalar,
    /// `SUNLIGHT_AMBIENT`.
    pub sunlight_ambient: WeatherScalar,
    /// `SUNLIGHT_COLOR_DIFFUSE`.
    pub sunlight_color_diffuse: [WeatherScalar; 3],
    /// `SUNLIGHT_COLOR_AMBIENT`.
    pub sunlight_color_ambient: [WeatherScalar; 3],
    /// `SUNLIGHT_STATIC`.
    pub sunlight_static: WeatherScalar,
    /// `SUNLIGHT_BICOLORED`.
    pub sunlight_bicolored: WeatherScalar,
}

/// What `TYPE` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecipitationType {
    /// `RAIN`.
    Rain,
    /// `SNOW`.
    Snow,
}

/// The top-level precipitation keys, present in 14 retail documents
/// (9 rain, 5 snow).
#[derive(Clone, Debug, PartialEq)]
pub struct PrecipitationRecord {
    /// `TYPE`.
    pub kind: PrecipitationType,
    /// `PARTICLES`; present in the 9 rain documents, absent in the 5 snow
    /// ones.
    pub particles: Option<WeatherScalar>,
    /// `COLOR`.
    pub color: [WeatherScalar; 3],
    /// `WIND_DIR`.
    pub wind_dir: WeatherScalar,
    /// `WIND_VEL`.
    pub wind_vel: WeatherScalar,
    /// `GRAVITY`.
    pub gravity: WeatherScalar,
    /// `ALPHA_GRADIENT`.
    pub alpha_gradient: [WeatherScalar; 2],
}

/// One decoded `weather.zrd`.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherDocument {
    /// `VIEWING_RANGE`, in file order.
    pub viewing_range: Vec<ViewingRangeTier>,
    /// `WIND`.
    pub wind: WindRecord,
    /// `CLOUD_COVER`.
    pub cloud_cover: CloudCoverRecord,
    /// Every `ZONEn` and `SW_ZONEn`, in file order.
    pub zones: Vec<ZoneRecord>,
    /// `SHADOW_ANGLES`.
    pub shadow_angles: [WeatherScalar; 3],
    /// The precipitation keys, when the document has them.
    pub precipitation: Option<PrecipitationRecord>,
}

type Fields<'a> = BTreeMap<&'a str, &'a ZrdValue>;

fn collect<'a>(scope: &str, node: &'a ZrdValue) -> Result<Fields<'a>, WeatherError> {
    let mut fields = Fields::new();
    for (key, value) in zrd_flat_fields(node) {
        if fields.insert(key, value).is_some() {
            return Err(WeatherError::DuplicateKey {
                scope: scope.to_owned(),
                key: key.to_owned(),
            });
        }
    }
    Ok(fields)
}

fn take<'a>(
    scope: &str,
    fields: &mut Fields<'a>,
    key: &'static str,
) -> Result<&'a ZrdValue, WeatherError> {
    fields.remove(key).ok_or_else(|| WeatherError::MissingKey {
        scope: scope.to_owned(),
        key,
    })
}

fn no_leftovers(scope: &str, fields: &Fields<'_>) -> Result<(), WeatherError> {
    match fields.keys().next() {
        Some(key) => Err(WeatherError::UnknownKey {
            scope: scope.to_owned(),
            key: (*key).to_owned(),
        }),
        None => Ok(()),
    }
}

fn shape(scope: &str, key: &str, expected: &'static str) -> WeatherError {
    WeatherError::BadShape {
        scope: scope.to_owned(),
        key: key.to_owned(),
        expected,
    }
}

fn scalar(value: &ZrdValue) -> Option<WeatherScalar> {
    match value {
        ZrdValue::Int(v) => Some(WeatherScalar::Int(*v)),
        ZrdValue::Float(v) => Some(WeatherScalar::Float(*v)),
        _ => None,
    }
}

fn bare(scope: &str, key: &str, value: &ZrdValue) -> Result<WeatherScalar, WeatherError> {
    scalar(value).ok_or_else(|| shape(scope, key, "a bare number"))
}

fn array<const N: usize>(
    scope: &str,
    key: &str,
    value: &ZrdValue,
) -> Result<[WeatherScalar; N], WeatherError> {
    let items = value
        .as_list()
        .filter(|items| items.len() == N)
        .ok_or_else(|| shape(scope, key, "a list of a fixed length"))?;
    let mut out = [WeatherScalar::Int(0); N];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = scalar(item).ok_or_else(|| shape(scope, key, "a list of numbers"))?;
    }
    Ok(out)
}

fn single(scope: &str, key: &str, value: &ZrdValue) -> Result<WeatherScalar, WeatherError> {
    Ok(array::<1>(scope, key, value)?[0])
}

fn parse_zone(name: &str, node: &ZrdValue) -> Result<ZoneRecord, WeatherError> {
    let mut f = collect(name, node)?;
    let zone = ZoneRecord {
        name: name.to_owned(),
        fog_color: array(name, "FOG_COLOR", take(name, &mut f, "FOG_COLOR")?)?,
        fog_ranges: array(name, "FOG_RANGES", take(name, &mut f, "FOG_RANGES")?)?,
        fog_altitude: f
            .remove("FOG_ALTITUDE")
            .map(|v| array(name, "FOG_ALTITUDE", v))
            .transpose()?,
        clip_ranges: array(name, "CLIP_RANGES", take(name, &mut f, "CLIP_RANGES")?)?,
        sunlight_active: single(
            name,
            "SUNLIGHT_ACTIVE",
            take(name, &mut f, "SUNLIGHT_ACTIVE")?,
        )?,
        sunlight_orientation: array(
            name,
            "SUNLIGHT_ORIENTATION",
            take(name, &mut f, "SUNLIGHT_ORIENTATION")?,
        )?,
        sunlight_diffuse: single(
            name,
            "SUNLIGHT_DIFFUSE",
            take(name, &mut f, "SUNLIGHT_DIFFUSE")?,
        )?,
        sunlight_ambient: single(
            name,
            "SUNLIGHT_AMBIENT",
            take(name, &mut f, "SUNLIGHT_AMBIENT")?,
        )?,
        sunlight_color_diffuse: array(
            name,
            "SUNLIGHT_COLOR_DIFFUSE",
            take(name, &mut f, "SUNLIGHT_COLOR_DIFFUSE")?,
        )?,
        sunlight_color_ambient: array(
            name,
            "SUNLIGHT_COLOR_AMBIENT",
            take(name, &mut f, "SUNLIGHT_COLOR_AMBIENT")?,
        )?,
        sunlight_static: single(
            name,
            "SUNLIGHT_STATIC",
            take(name, &mut f, "SUNLIGHT_STATIC")?,
        )?,
        sunlight_bicolored: single(
            name,
            "SUNLIGHT_BICOLORED",
            take(name, &mut f, "SUNLIGHT_BICOLORED")?,
        )?,
    };
    no_leftovers(name, &f)?;
    Ok(zone)
}

fn is_zone_key(key: &str) -> bool {
    let digits = key
        .strip_prefix("SW_ZONE")
        .or_else(|| key.strip_prefix("ZONE"));
    digits.is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

impl WeatherDocument {
    /// Reads a decoded `weather.zrd` tree.
    ///
    /// # Errors
    ///
    /// A [`WeatherError`] naming the first key or shape the grammar refuses.
    pub fn from_zrd(root: &ZrdValue) -> Result<Self, WeatherError> {
        let record = root
            .as_list()
            .and_then(|children| children.first())
            .ok_or(WeatherError::NoRecord)?;
        let mut top = collect("", record)?;

        let viewing = take("", &mut top, "VIEWING_RANGE")?;
        let mut viewing_range = Vec::new();
        for (name, tier) in zrd_flat_fields(viewing) {
            let mut f = collect(name, tier)?;
            let clip = single(name, "CLIP_SCALE", take(name, &mut f, "CLIP_SCALE")?)?;
            let fog = single(name, "FOG_SCALE", take(name, &mut f, "FOG_SCALE")?)?;
            no_leftovers(name, &f)?;
            viewing_range.push(ViewingRangeTier {
                name: name.to_owned(),
                clip_scale: clip,
                fog_scale: fog,
            });
        }

        let wind_node = take("", &mut top, "WIND")?;
        let mut f = collect("WIND", wind_node)?;
        let wind = WindRecord {
            static_velocity: array(
                "WIND",
                "STATIC_VELOCITY",
                take("WIND", &mut f, "STATIC_VELOCITY")?,
            )?,
            random_max_speed: bare(
                "WIND",
                "RANDOM_MAX_SPEED",
                take("WIND", &mut f, "RANDOM_MAX_SPEED")?,
            )?,
            random_accel: bare(
                "WIND",
                "RANDOM_ACCEL",
                take("WIND", &mut f, "RANDOM_ACCEL")?,
            )?,
            random_ang_vel: bare(
                "WIND",
                "RANDOM_ANG_VEL",
                take("WIND", &mut f, "RANDOM_ANG_VEL")?,
            )?,
        };
        no_leftovers("WIND", &f)?;

        let cloud_node = take("", &mut top, "CLOUD_COVER")?;
        let mut f = collect("CLOUD_COVER", cloud_node)?;
        let cloud_cover = CloudCoverRecord {
            top: bare("CLOUD_COVER", "TOP", take("CLOUD_COVER", &mut f, "TOP")?)?,
            bottom: bare(
                "CLOUD_COVER",
                "BOTTOM",
                take("CLOUD_COVER", &mut f, "BOTTOM")?,
            )?,
            thickness: bare(
                "CLOUD_COVER",
                "THICKNESS",
                take("CLOUD_COVER", &mut f, "THICKNESS")?,
            )?,
            top_color: f
                .remove("TOP_COLOR")
                .map(|v| array("CLOUD_COVER", "TOP_COLOR", v))
                .transpose()?,
            bottom_color: f
                .remove("BOTTOM_COLOR")
                .map(|v| array("CLOUD_COVER", "BOTTOM_COLOR", v))
                .transpose()?,
        };
        no_leftovers("CLOUD_COVER", &f)?;

        // Zones keep file order, so walk the flat fields rather than the map.
        let mut zones = Vec::new();
        for (key, node) in zrd_flat_fields(record) {
            if is_zone_key(key) {
                top.remove(key);
                zones.push(parse_zone(key, node)?);
            }
        }

        let shadow_angles = array("", "SHADOW_ANGLES", take("", &mut top, "SHADOW_ANGLES")?)?;

        let precipitation = match top.remove("TYPE") {
            None => None,
            Some(kind) => {
                let text = kind.as_text().ok_or_else(|| shape("", "TYPE", "text"))?;
                let kind = match text {
                    "RAIN" => PrecipitationType::Rain,
                    "SNOW" => PrecipitationType::Snow,
                    other => {
                        return Err(WeatherError::UnknownPrecipitationType {
                            text: other.to_owned(),
                        });
                    }
                };
                Some(PrecipitationRecord {
                    kind,
                    particles: top
                        .remove("PARTICLES")
                        .map(|v| bare("", "PARTICLES", v))
                        .transpose()?,
                    color: array("", "COLOR", take("", &mut top, "COLOR")?)?,
                    wind_dir: bare("", "WIND_DIR", take("", &mut top, "WIND_DIR")?)?,
                    wind_vel: bare("", "WIND_VEL", take("", &mut top, "WIND_VEL")?)?,
                    gravity: bare("", "GRAVITY", take("", &mut top, "GRAVITY")?)?,
                    alpha_gradient: array(
                        "",
                        "ALPHA_GRADIENT",
                        take("", &mut top, "ALPHA_GRADIENT")?,
                    )?,
                })
            }
        };
        no_leftovers("", &top)?;

        Ok(Self {
            viewing_range,
            wind,
            cloud_cover,
            zones,
            shadow_angles,
            precipitation,
        })
    }
}

/// One document field the binding does not turn into a known value, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnboundField {
    /// The key path in the document (`WIND.STATIC_VELOCITY`).
    pub key: &'static str,
    /// The environment-session consumer it would feed, or `none`.
    pub consumer: &'static str,
    /// What is unmeasured.
    pub reason: &'static str,
}

/// The result of binding one document.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherBinding {
    /// The environment the session starts from.
    pub definition: EnvironmentDefinition,
    /// Every present field that stays unbound, in a stable order.
    pub unbound: Vec<UnboundField>,
}

const UNIT_REASON: &str = "the original's world-unit scale and axis convention are unmeasured";

fn claim(id: &str) -> Result<ClaimId, WeatherError> {
    ClaimId::new(id).map_err(|error| WeatherError::Claim(error.to_string()))
}

fn unknown<T>(id: &str, reason: &str) -> Result<Resolved<T>, WeatherError> {
    Ok(Resolved::Unknown {
        claim_id: claim(id)?,
        reason: reason.to_owned(),
    })
}

/// Binds a document to an environment.
///
/// `mission_key` is the mission's logical key (`zbd/c1c/m01`); it names the
/// environment and the claims. `source` is the byte span of the
/// `weather.zrd` member inside its reader archive.
///
/// # Errors
///
/// [`WeatherError::Id`] when `mission_key` is not an environment key,
/// [`WeatherError::Environment`] when the assembled record is refused.
pub fn bind_weather(
    mission_key: &str,
    document: &WeatherDocument,
    source: &SourceSpan,
) -> Result<WeatherBinding, WeatherError> {
    let slug = mission_key.replace('/', ".");
    let id = EnvironmentId::new(&format!("{slug}.weather")).map_err(WeatherError::Id)?;
    let observed = |suffix: &str| -> Result<Provenance, WeatherError> {
        Provenance::new(
            claim(&format!("m01lc.weather.{slug}.{suffix}"))?,
            ClaimStatus::ObservedTool,
            Some(source.clone()),
        )
        .map_err(|error| WeatherError::Claim(error.to_string()))
    };
    let claim_for = |suffix: &str| format!("m01lc.weather.{slug}.{suffix}");

    let precipitation = match &document.precipitation {
        Some(record) => {
            let kind = match record.kind {
                PrecipitationType::Rain => PrecipitationKind::Rain,
                PrecipitationType::Snow => PrecipitationKind::Snow,
            };
            Resolved::Known(Known::new(kind, observed("precipitation")?))
        }
        // The document authors no precipitation record; the spelling of
        // "none" is absence, which is an inference and tagged as one.
        None => Resolved::Known(Known::new(
            PrecipitationKind::Clear,
            Provenance::new(
                claim(&claim_for("precipitation.absent"))?,
                ClaimStatus::Inferred,
                Some(source.clone()),
            )
            .map_err(|error| WeatherError::Claim(error.to_string()))?,
        )),
    };

    let sky = SkyArt::missing(
        &claim_for("sky-texture"),
        "weather.zrd names no sky texture; the sky art lives in another record",
    )
    .map_err(|error| WeatherError::Claim(error.to_string()))?;
    let lighting = LightingDefinition::try_new(
        unknown::<cs_types::space::UnitVec3>(
            &claim_for("sun-direction"),
            "SUNLIGHT_ORIENTATION is three angles whose order, unit and axes are unmeasured",
        )?,
        unknown(
            &claim_for("ambient"),
            "SUNLIGHT_AMBIENT is an intensity and SUNLIGHT_COLOR_AMBIENT a color of unmeasured scale and color space",
        )?,
    )
    .map_err(|error| WeatherError::Claim(error.to_string()))?;
    let fog = FogDefinition::unknown(
        &claim_for("fog"),
        "FOG_RANGES are near/far distances in unmeasured units and have no per-meter density; FOG_COLOR has an unmeasured scale",
    )
    .map_err(|error| WeatherError::Claim(error.to_string()))?;

    let state = EnvironmentState::new(
        unknown::<WindField>(&claim_for("wind"), UNIT_REASON)?,
        PrecipitationDefinition::new(precipitation),
        unknown::<GameplayVisibility>(
            &claim_for("gameplay-visibility"),
            "weather.zrd authors only renderer fog and clip ranges; the AI sight range is never derived from them",
        )?,
    );
    let orientation: Resolved<SkyOrientation> = unknown(
        &claim_for("sky-orientation"),
        "SHADOW_ANGLES and the zone sun angles do not state a horizon or a heading",
    )?;

    let definition = EnvironmentDefinition::try_new(
        id,
        Origin::Installation {
            source: source.clone(),
        },
        EnvironmentProfile::Retail,
        sky,
        orientation,
        lighting,
        fog,
        Vec::new(),
        state,
        EnvironmentTimeline::default(),
        observed("record")?,
    )
    .map_err(WeatherError::Environment)?;

    Ok(WeatherBinding {
        definition,
        unbound: unbound_fields(document),
    })
}

fn unbound_fields(document: &WeatherDocument) -> Vec<UnboundField> {
    let mut out = vec![
        field(
            "WIND.STATIC_VELOCITY",
            "EnvironmentSession::wind",
            UNIT_REASON,
        ),
        field(
            "WIND.RANDOM_MAX_SPEED",
            "none",
            "no random-wind model exists in the environment contract and the field's meaning is unmeasured",
        ),
        field(
            "WIND.RANDOM_ACCEL",
            "none",
            "no random-wind model exists in the environment contract and the field's meaning is unmeasured",
        ),
        field(
            "WIND.RANDOM_ANG_VEL",
            "none",
            "no random-wind model exists in the environment contract and the field's meaning is unmeasured",
        ),
        field(
            "CLOUD_COVER.TOP/BOTTOM/THICKNESS",
            "EnvironmentDefinition::cloud_layers",
            "altitudes are in unmeasured units and no coverage fraction is authored",
        ),
    ];
    if document.cloud_cover.top_color.is_some() || document.cloud_cover.bottom_color.is_some() {
        out.push(field(
            "CLOUD_COVER.TOP_COLOR/BOTTOM_COLOR",
            "none",
            "integer color of unmeasured scale and no cloud-color consumer",
        ));
    }
    out.extend([
        field(
            "ZONEn.FOG_COLOR/FOG_RANGES/FOG_ALTITUDE",
            "EnvironmentDefinition::fog",
            "near/far fog, altitude bands and color scale are unmeasured; the contract has one density",
        ),
        field(
            "ZONEn.CLIP_RANGES",
            "none",
            "renderer clip distance; never a gameplay visibility",
        ),
        field(
            "ZONEn.SUNLIGHT_ORIENTATION",
            "EnvironmentDefinition::sun_direction",
            "angle order, unit and axes are unmeasured",
        ),
        field(
            "ZONEn.SUNLIGHT_DIFFUSE/AMBIENT/COLOR_*",
            "EnvironmentDefinition::lighting",
            "intensity scale and color space are unmeasured",
        ),
        field(
            "ZONEn.SUNLIGHT_ACTIVE/STATIC/BICOLORED",
            "none",
            "flags whose behavior is unmeasured",
        ),
        field(
            "SW_ZONEn.*",
            "none",
            "the SW_ twin of each zone is unmeasured (a software-render variant is a guess, not a finding)",
        ),
        field(
            "VIEWING_RANGE.*",
            "none",
            "per-tier clip/fog multipliers; the option that selects a tier is unmeasured",
        ),
        field(
            "SHADOW_ANGLES",
            "EnvironmentDefinition::sky_orientation",
            "angle order, unit and axes are unmeasured",
        ),
    ]);
    if document.precipitation.is_some() {
        out.push(field(
            "PARTICLES/COLOR/WIND_DIR/WIND_VEL/GRAVITY/ALPHA_GRADIENT",
            "cosmetic weather effects",
            "the kind is bound; the particle tuning has unmeasured units and no bound consumer",
        ));
    }
    out
}

const fn field(key: &'static str, consumer: &'static str, reason: &'static str) -> UnboundField {
    UnboundField {
        key,
        consumer,
        reason,
    }
}
