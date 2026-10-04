//! A mission's retail `weather.zrd`, read and bound (task #636,
//! `M01-LC-WEATHER-BIND`).
//!
//! The member lives in each mission's `zrdr.zbd` reader archive. This module
//! walks that archive through the production container readers, decodes the
//! member with the `.zrd` grammar reader, parses it with
//! [`cs_content::weather::WeatherDocument`] and binds it with
//! [`cs_content::weather::bind_weather`]. What the binding leaves unknown is
//! listed in [`RetailWeather::binding`]'s `unbound` field; nothing is filled
//! in from a default. See `docs/findings/2026-10-05-m01-lc-weather-bind.md`.

use std::fmt;

use cs_assets::install::{self, Discovery};
use cs_content::stunts::{ZrdDecodeError, decode_zrd};
use cs_content::weather::{WeatherBinding, WeatherDocument, WeatherError, bind_weather};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_sim::time::TickRate;
use cs_types::asset_id::SourceSpan;

use super::session::{EnvironmentSession, RunSeeds};

/// The member name, matched case-insensitively.
pub const WEATHER_MEMBER: &str = "weather.zrd";

/// The reader archive's file name inside a mission directory.
const READER_ARCHIVE: &str = "zrdr.zbd";

/// Why a mission's weather could not be read or bound.
#[derive(Debug)]
pub enum RetailWeatherError {
    /// The reader archive is not under the installation.
    MissingReader {
        /// The mission key asked for.
        mission_key: String,
    },
    /// The reader archive could not be read.
    Io(String),
    /// A container reader refused the archive.
    Container {
        /// The stable refusal code.
        code: &'static str,
    },
    /// The archive has no `weather.zrd` member.
    MissingMember {
        /// The mission key asked for.
        mission_key: String,
    },
    /// The member's source span is not recordable.
    Span(String),
    /// The member did not decode as `.zrd`.
    Decode(ZrdDecodeError),
    /// The decoded tree is not a weather document, or could not be bound.
    Weather(WeatherError),
}

impl fmt::Display for RetailWeatherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingReader { mission_key } => {
                write!(f, "no {READER_ARCHIVE} under {mission_key}")
            }
            Self::Io(text) => write!(f, "cannot read the reader archive: {text}"),
            Self::Container { code } => write!(f, "the reader archive was refused: {code}"),
            Self::MissingMember { mission_key } => {
                write!(f, "{mission_key} has no {WEATHER_MEMBER}")
            }
            Self::Span(text) => write!(f, "the member's source span is not recordable: {text}"),
            Self::Decode(error) => write!(f, "weather member does not decode: {}", error.code()),
            Self::Weather(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RetailWeatherError {}

/// One mission's decoded and bound weather.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailWeather {
    /// The mission's logical key (`zbd/c1c/m01`).
    pub mission_key: String,
    /// The member's byte span inside the reader archive.
    pub span: SourceSpan,
    /// The member's size in bytes.
    pub member_bytes: usize,
    /// The typed document, in the file's own units.
    pub document: WeatherDocument,
    /// The environment and the list of fields left unbound.
    pub binding: WeatherBinding,
}

impl RetailWeather {
    /// Starts an [`EnvironmentSession`] on the bound environment.
    ///
    /// # Errors
    ///
    /// [`cs_sim::visibility::TimelineError`] from the session's clock.
    pub fn session(
        &self,
        rate: TickRate,
        seeds: RunSeeds,
    ) -> Result<EnvironmentSession, cs_sim::visibility::TimelineError> {
        EnvironmentSession::new(self.binding.definition.clone(), rate, seeds)
    }
}

/// Reads and binds the `weather.zrd` of one mission.
///
/// `found` is a discovery pass over the installation (the expensive half,
/// shared by a caller that reads several missions); `mission_key` is the
/// logical key of the mission directory, e.g. `zbd/c1c/m01`.
///
/// # Errors
///
/// A [`RetailWeatherError`] naming the step that failed.
pub fn read_mission_weather(
    found: &Discovery,
    mission_key: &str,
) -> Result<RetailWeather, RetailWeatherError> {
    let reader_key = format!("{mission_key}/{READER_ARCHIVE}");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == reader_key)
        .ok_or_else(|| RetailWeatherError::MissingReader {
            mission_key: mission_key.to_owned(),
        })?;
    let bytes = std::fs::read(
        found
            .manifest
            .host_root
            .join(record.relative_spelling.as_str()),
    )
    .map_err(|error| RetailWeatherError::Io(error.to_string()))?;
    let container = |code: &'static str| RetailWeatherError::Container { code };
    let mut context = ParseContext::with_defaults(&reader_key);
    let decision = dispatch(ZbdProbe::new(
        &reader_key,
        &record.relative_spelling,
        &bytes,
    ))
    .map_err(|error| container(error.code()))?;
    let index = read_version_one_index(&mut context, decision, &bytes)
        .map_err(|error| container(error.code()))?;
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data())
        .map_err(|error| container(error.code()))?;
    let entry = archive
        .entries()
        .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(WEATHER_MEMBER))
        .ok_or_else(|| RetailWeatherError::MissingMember {
            mission_key: mission_key.to_owned(),
        })?;
    let tree = decode_zrd(entry.content()).map_err(RetailWeatherError::Decode)?;
    let document = WeatherDocument::from_zrd(&tree).map_err(RetailWeatherError::Weather)?;
    let located = entry.span();
    let span = SourceSpan::new(
        install::fingerprint(&found.manifest),
        record.relative_spelling.as_str(),
        Some(WEATHER_MEMBER),
        located.offset,
        located.length,
        Some(install::sha256(entry.content())),
    )
    .map_err(|error| RetailWeatherError::Span(error.to_string()))?;
    let binding =
        bind_weather(mission_key, &document, &span).map_err(RetailWeatherError::Weather)?;
    Ok(RetailWeather {
        mission_key: mission_key.to_owned(),
        span,
        member_bytes: entry.content().len(),
        document,
        binding,
    })
}
