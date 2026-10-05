//! The original-media audit of the sound containers (F41-D).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-D`. Finding: `docs/findings/2026-10-05-f41-d-media-audit.md`.
//!
//! [`audit_container`] reads one sound container through the production ZBD
//! producer ([`cs_assets::zbd::ZbdContainer`]) and gives **every declared
//! member** a [`MemberRow`]: its own index position, its declared tag, channel
//! count, rate and block geometry, its F14-D.7 derived [`ContentId`] and its
//! readiness. A member this workspace cannot decode keeps its row and carries the
//! refusal's own code ([`MemberReadiness::Refused`]); nothing is dropped, and
//! [`ContainerAudit::reconciles`] states the count against the archive's own
//! index.
//!
//! A decoded member is not a played member. [`PlayableSample`] records that a
//! member decodes under its own declared format into non-silent samples, which is
//! as far as an audit without an output device can go; hearing it is the owner's
//! `human_review`.

use std::collections::BTreeMap;

use cs_assets::vfs::ContentSession;
use cs_assets::zbd::{SoundReadiness, ZbdContainer};
use cs_content::catalog::baseline::install_file_key;
use cs_formats::ParseContext;
use cs_formats::zbd::{
    PcmLayout, SampleFormat, SampleLayout, ZbdRole, decode_sound_sample, role_for_path,
};
use cs_types::asset_id::AssetKey;
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::InstallFileRecord;

/// How many distinct playable samples are kept per (channels, rate, tag) shape.
pub const SAMPLES_PER_SHAPE: usize = 2;

/// The (tag, channels, rate) a member declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Shape {
    /// `wFormatTag`.
    pub tag: u16,
    /// `nChannels`.
    pub channels: u16,
    /// `nSamplesPerSec`.
    pub rate_hz: u32,
}

/// What the audit established about one member's samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberReadiness {
    /// The member decodes under its own declared format.
    Decoded {
        /// Sample values the decode produced, over all channels.
        samples: u64,
    },
    /// The member does not decode; `code` is the refusal's own stable code.
    Refused {
        /// The refusal code (a header, format or payload refusal).
        code: String,
    },
}

/// One declared member of a sound container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberRow {
    /// Position in the container's declared index.
    pub index: usize,
    /// The declared name, lossily rendered.
    pub name: String,
    /// The F14-D.7 `ContentId` (container plus declared name), or why the name
    /// has none.
    pub id: Result<ContentId, String>,
    /// The declared `wFormatTag`, when the header reads.
    pub tag: Option<u16>,
    /// The declared channel count, when the header reads.
    pub channels: Option<u16>,
    /// The declared rate, when the header reads.
    pub rate_hz: Option<u32>,
    /// `nBlockAlign`, when the header reads.
    pub block_align: Option<u16>,
    /// `wSamplesPerBlock` of a block-coded member whose plan was built.
    pub samples_per_block: Option<u16>,
    /// Whether the member decodes, or the refusal that says why not.
    pub readiness: MemberReadiness,
}

impl MemberRow {
    /// The shape this member declares, when its header reads.
    pub fn shape(&self) -> Option<Shape> {
        Some(Shape {
            tag: self.tag?,
            channels: self.channels?,
            rate_hz: self.rate_hz?,
        })
    }
}

/// A member that decodes into audible (non-silent) samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayableSample {
    /// Position in the container's declared index.
    pub index: usize,
    /// The declared name.
    pub name: String,
    /// The shape it represents.
    pub shape: Shape,
    /// Samples decoded, over all channels.
    pub samples: u64,
    /// Largest excursion from silence.
    pub peak: u32,
}

/// One container, audited.
#[derive(Clone, Debug)]
pub struct ContainerAudit {
    /// The installation-relative spelling the container was opened by.
    pub container: String,
    /// Members the container's own index declares.
    pub declared: usize,
    /// One row per declared member that has bytes.
    pub rows: Vec<MemberRow>,
    /// Declared members that failed their bounds check, by the refusal's code.
    /// They have no bytes and so no row; they are counted, not dropped.
    pub unreadable_extent: BTreeMap<&'static str, usize>,
    /// Up to [`SAMPLES_PER_SHAPE`] distinct playable members per shape.
    pub playable: BTreeMap<Shape, Vec<PlayableSample>>,
}

impl ContainerAudit {
    /// Members that decode.
    pub fn decoded(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row.readiness, MemberReadiness::Decoded { .. }))
            .count()
    }

    /// Refused members by refusal code.
    pub fn refusals(&self) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        for row in &self.rows {
            if let MemberReadiness::Refused { code } = &row.readiness {
                *counts.entry(code.as_str()).or_default() += 1;
            }
        }
        counts
    }

    /// Members per shape, over the rows whose header reads.
    pub fn shapes(&self) -> BTreeMap<Shape, usize> {
        let mut counts = BTreeMap::new();
        for shape in self.rows.iter().filter_map(MemberRow::shape) {
            *counts.entry(shape).or_default() += 1;
        }
        counts
    }

    /// Whether decoded + refused + unreadable-extent members account for every
    /// member the archive's own index declares.
    pub fn reconciles(&self) -> bool {
        let unreadable: usize = self.unreadable_extent.values().sum();
        let refused: usize = self.refusals().values().sum();
        self.decoded() + refused + unreadable == self.declared
            && self.rows.len() + unreadable == self.declared
    }
}

/// Why a container could not be audited at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerRefusal {
    /// The container.
    pub container: String,
    /// The refusal's stable code.
    pub code: &'static str,
    /// The refusal's explanation.
    pub reason: String,
}

/// The installation-relative spellings of every sound container the
/// installation holds, by the sound family's own role rule, in canonical order.
pub fn sound_container_spellings(files: &[InstallFileRecord]) -> Vec<String> {
    let mut found: Vec<String> = files
        .iter()
        .filter(|record| {
            matches!(
                role_for_path(&record.relative_spelling),
                ZbdRole::Observed {
                    family: cs_formats::zbd::ZbdFamily::Sound,
                    ..
                }
            )
        })
        .map(|record| record.relative_spelling.as_str().to_owned())
        .collect();
    found.sort();
    found
}

/// Audits the sound container at `spelling`.
///
/// # Errors
///
/// [`ContainerRefusal`] when the container cannot be opened, indexed or read as
/// a sound archive. A member that fails is its own row, never an error.
pub fn audit_container(
    session: &ContentSession,
    spelling: &str,
) -> Result<ContainerAudit, ContainerRefusal> {
    let refuse = |code: &'static str, reason: String| ContainerRefusal {
        container: spelling.to_owned(),
        code,
        reason,
    };
    let key = AssetKey::from_spelling("install", spelling, "default")
        .map_err(|error| refuse("asset_key", error.to_string()))?;
    let container = ZbdContainer::open(session, &key)
        .map_err(|error| refuse(error.code(), error.to_string()))?;
    let mut parse = ParseContext::with_defaults(container.label());
    let index = container
        .index(&mut parse)
        .map_err(|error| refuse(error.code(), error.to_string()))?;
    let table = index.member_table();
    let assets = container
        .sound_assets(&mut parse, &index, &table)
        .map_err(|error| refuse(error.code(), error.to_string()))?;

    let listing = assets.listing();
    let mut audit = ContainerAudit {
        container: spelling.to_owned(),
        declared: listing.len(),
        rows: Vec::with_capacity(listing.len()),
        unreadable_extent: BTreeMap::new(),
        playable: BTreeMap::new(),
    };
    let mut kept_digests: BTreeMap<Shape, Vec<String>> = BTreeMap::new();

    for declared in listing.rows() {
        let Some(asset) = assets.entry(declared.index()) else {
            let code = declared
                .error()
                .map_or("member_extent", |error| error.code());
            *audit.unreadable_extent.entry(code).or_default() += 1;
            continue;
        };
        let name = String::from_utf8_lossy(asset.name()).into_owned();
        let id = ContentId::from_source(
            ContentKind::Sound,
            &install_file_key(&format!("{spelling}/{name}")),
        )
        .map_err(|error| error.to_string());
        let header = asset.wave().ok();
        let plan = SampleFormat::from_member(asset.content()).ok();
        let samples_per_block = plan.as_ref().and_then(|plan| match plan.layout() {
            SampleLayout::Adpcm(layout) => Some(layout.declared_samples_per_block()),
            SampleLayout::Pcm(_) => None,
        });
        let readiness = match asset.readiness() {
            SoundReadiness::Decoded { .. } => {
                let plan = plan.as_ref().expect("a decoded member has a plan");
                let mut member_parse = ParseContext::with_defaults(container.label());
                match decode_sound_sample(&mut member_parse, asset.content(), plan) {
                    Ok(decoded) => {
                        // Unsigned 8-bit PCM keeps its silence at 128, so its
                        // excursion is measured from there, not from zero.
                        let centre = match plan.layout() {
                            SampleLayout::Pcm(PcmLayout::Unsigned8) => 128,
                            _ => 0,
                        };
                        let peak = decoded
                            .samples()
                            .iter()
                            .map(|value| (value - centre).unsigned_abs())
                            .max()
                            .unwrap_or(0);
                        let shape = Shape {
                            tag: header.map_or(0, |header| header.format_tag()),
                            channels: plan.channels(),
                            rate_hz: plan.rate_hz(),
                        };
                        let digest = cs_assets::install::sha256(asset.content()).to_hex();
                        let seen = kept_digests.entry(shape).or_default();
                        let kept = audit.playable.entry(shape).or_default();
                        if peak > 0 && kept.len() < SAMPLES_PER_SHAPE && !seen.contains(&digest) {
                            seen.push(digest);
                            kept.push(PlayableSample {
                                index: asset.index(),
                                name: name.clone(),
                                shape,
                                samples: decoded.sample_count(),
                                peak,
                            });
                        }
                        MemberReadiness::Decoded {
                            samples: decoded.sample_count(),
                        }
                    }
                    Err(error) => MemberReadiness::Refused {
                        code: error.code().to_owned(),
                    },
                }
            }
            SoundReadiness::UnsupportedFormat { tag, .. } => MemberReadiness::Refused {
                code: format!("unsupported_format_{tag:#06x}"),
            },
            SoundReadiness::UnreadableHeader { reason } => MemberReadiness::Refused {
                code: format!("unreadable_header: {reason}"),
            },
            SoundReadiness::Undecodable { code } => MemberReadiness::Refused {
                code: (*code).to_owned(),
            },
        };
        audit.rows.push(MemberRow {
            index: asset.index(),
            name,
            id,
            tag: header.map(|header| header.format_tag()),
            channels: header.map(|header| header.channels()),
            rate_hz: header.map(|header| header.rate_hz()),
            block_align: header.map(|header| header.block_align()),
            samples_per_block,
            readiness,
        });
    }
    Ok(audit)
}
