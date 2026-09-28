//! The `texture-audit` command (F08-D): every ZBD texture of an
//! installation decoded by the production path and compared, texel by texel,
//! with a pinned reference extraction; optionally a private contact sheet.
//!
//! ```text
//! cs-inspect texture-audit [--cs-path <dir>] --reference <dir>
//!                          [--sheet-dir <dir>] [--out <file>]
//! ```
//!
//! The installation is discovered (F02-B) and mounted into one
//! [`ContentSession`] with the designed layout. Every inventoried `.zbd` file
//! is offered to the F08-C [`TextureCatalog`] by its `install:` key; the
//! F06-A dispatch decides which ones are texture packages (`wrong_family`
//! means "not a texture archive" and the file is skipped, any other failure
//! is a failed archive row). Every entry is reached with
//! [`TextureCatalog::resolve_entry`] and handed to
//! [`TextureCatalog::prepare_upload`], so the values compared are exactly
//! what the renderer adapter would receive.
//!
//! # The reference
//!
//! `--reference <dir>` holds, for each texture archive spelled `P` in the
//! installation, the file `<dir>/P.zip` written by the pinned reference
//! extractor (`unzbd cs textures`, mech3ax v0.6.0, commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb`; `docs/research/SOURCES.md`
//! S02): one PNG per texture in table order, then `manifest.json`. The PNGs
//! are read here with a bounded ZIP and PNG reader (stored or deflated
//! members, CRC-checked; 8-bit RGB or RGBA, not interlaced). How the files
//! were produced is recorded in the F08-D findings.
//!
//! # What is compared (spec F08 non-negotiable #5)
//!
//! Independently for every texture: the name at the same table position
//! (the reference renames repeated names `name-N`), the dimensions, whether
//! the reference carries an alpha channel, the color of every texel, the
//! alpha of every texel and the orientation. The reference expands each
//! RGB565 word to 8-bit channels; that expansion is a presentation choice,
//! so a texel matches when the reference's channels truncate back to the
//! word's 5/6/5 fields, and separately every word must expand to one
//! reference color across the whole corpus (an inconsistent expansion is an
//! unexplained difference). Which expansion the reference uses is reported
//! (`lerp`, `replicate`), not adopted. A texel that differs is reported with
//! its coordinates and, when the whole image matches a mirrored reference,
//! the mirroring that would explain it — as an unexplained difference.
//!
//! The only explained difference is `alpha_source_unknown`: the descriptor
//! leaves coverage unknown (palette textures with the "simple alpha" flag)
//! and the reference presents the texture opaque. Everything else is
//! unexplained and makes the command exit 3.
//!
//! # Contact sheet
//!
//! With `--sheet-dir` each texture archive gets one uncompressed 32-bit TGA
//! (top-left origin) with every texture at its own size on shelves, plus the
//! cell of each texture in the report. The sheet is a review rendering: 565
//! words are expanded by bit replication and coverage comes from the alpha
//! plane or the stored-value key; neither is a runtime decision. It shows
//! original content, so it belongs in a private directory only.
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` every texture
//! matched or differs only in an explained way; `3` any archive, texture or
//! reference failed or any unexplained difference remains; `2` invalid input
//! or an output inside the installation; `4` no installation selected; `1` a
//! runtime failure. The report is written on exit 3 too.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install;
use cs_assets::vfs::{ContentSession, SessionBuilder, SessionError, SourceError};
use cs_content::textures::{TextureCatalog, TextureUpload};
use cs_formats::texture::{AlphaSource, DecodedFormat};
use cs_types::asset_id::{AssetKey, ResolveContext};

/// Version of the JSON report layout.
pub const TEXTURE_AUDIT_REPORT_VERSION: &str = "cs-inspect-texture-audit/1";

/// Largest reference file read (a ZIP of one archive's PNGs).
pub const MAX_REFERENCE_BYTES: u64 = 512 << 20;

/// Width of a contact sheet unless one texture is wider.
pub const SHEET_WIDTH: u32 = 1024;

/// Gap between contact-sheet cells, in texels.
pub const SHEET_GUTTER: u32 = 2;

/// Parsed `texture-audit` arguments.
#[derive(Debug, Default)]
struct AuditArgs {
    cs_path: Option<PathBuf>,
    reference: Option<PathBuf>,
    sheet_dir: Option<PathBuf>,
    out: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<AuditArgs, String> {
    let mut parsed = AuditArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let slot = match arg.as_str() {
            "--cs-path" => &mut parsed.cs_path,
            "--reference" => &mut parsed.reference,
            "--sheet-dir" => &mut parsed.sheet_dir,
            "--out" => &mut parsed.out,
            other => {
                return Err(format!(
                    "cs-inspect texture-audit: unsupported argument {other:?}; expected \
                     --cs-path <dir>, --reference <dir>, --sheet-dir <dir>, --out <file>"
                ));
            }
        };
        let Some(value) = cursor.next() else {
            return Err(format!("cs-inspect texture-audit: {arg} needs a value"));
        };
        *slot = Some(PathBuf::from(value));
    }
    Ok(parsed)
}

/// One difference between a decoded texture and its reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    /// Stable code, e.g. `color`, `dimensions`, `reference_missing`.
    pub code: &'static str,
    /// Whether a recorded finding explains it.
    pub explained: bool,
    /// Human detail: counts, first coordinates, both values.
    pub detail: String,
}

impl Difference {
    fn unexplained(code: &'static str, detail: String) -> Self {
        Self {
            code,
            explained: false,
            detail,
        }
    }
}

/// How a decoded texture relates to its reference image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Every texel matches in place.
    AsStored,
    /// Every texel matches the reference flipped top to bottom.
    VerticalFlip,
    /// Every texel matches the reference flipped left to right.
    HorizontalFlip,
    /// Every texel matches the reference turned half a circle.
    Rotated180,
    /// No arrangement matches, or the comparison did not get that far.
    Unmatched,
}

impl Orientation {
    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::AsStored => "as_stored",
            Self::VerticalFlip => "vertical_flip",
            Self::HorizontalFlip => "horizontal_flip",
            Self::Rotated180 => "rotated_180",
            Self::Unmatched => "unmatched",
        }
    }
}

/// The comparison of one texture.
#[derive(Clone, Debug)]
pub struct TextureComparison {
    /// Position in the archive's table.
    pub entry_index: usize,
    /// The stored name.
    pub name: String,
    /// The reference's name at the same position, without `.png`.
    pub reference_name: Option<String>,
    /// Decoded width and height.
    pub extent: Option<(u32, u32)>,
    /// `rgb565` or `indexed565`.
    pub stored: &'static str,
    /// The descriptor's coverage source.
    pub alpha: String,
    /// The reference's width and height.
    pub reference_extent: Option<(u32, u32)>,
    /// The reference's channel count (3 or 4).
    pub reference_channels: Option<u8>,
    /// How the texels relate.
    pub orientation: Orientation,
    /// Every difference, explained or not.
    pub differences: Vec<Difference>,
    /// The texture's cell on the contact sheet: x, y, width, height.
    pub sheet_cell: Option<(u32, u32, u32, u32)>,
}

impl TextureComparison {
    /// `match`, `explained` or `unexplained`.
    pub fn verdict(&self) -> &'static str {
        if self.differences.is_empty() {
            "match"
        } else if self
            .differences
            .iter()
            .all(|difference| difference.explained)
        {
            "explained"
        } else {
            "unexplained"
        }
    }
}

/// One file of the reference or of the contact sheet, by digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDigest {
    /// Where it is on the host.
    pub path: PathBuf,
    /// Its SHA-256.
    pub sha256: String,
}

/// The audit of one texture archive.
#[derive(Clone, Debug)]
pub struct ArchiveAudit {
    /// The installation-relative spelling.
    pub spelling: String,
    /// SHA-256 of the archive as inventoried.
    pub sha256: String,
    /// The reference file read for it.
    pub reference: Option<FileDigest>,
    /// Why the archive or its reference could not be compared at all.
    pub failure: Option<Difference>,
    /// One row per texture, in table order.
    pub textures: Vec<TextureComparison>,
    /// Reference images past the end of the table.
    pub extra_reference_entries: Vec<String>,
    /// The contact sheet written for it.
    pub sheet: Option<FileDigest>,
}

impl ArchiveAudit {
    /// Unexplained differences of the archive and its textures.
    pub fn unexplained(&self) -> usize {
        usize::from(self.failure.is_some())
            + usize::from(!self.extra_reference_entries.is_empty())
            + self
                .textures
                .iter()
                .filter(|texture| texture.verdict() == "unexplained")
                .count()
    }
}

/// Which reference expansion of a 565 channel was observed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExpansionTally {
    /// Distinct 565 words seen.
    pub words: usize,
    /// Words whose reference color equals `round(c * 255 / max)` per channel.
    pub lerp: usize,
    /// Words whose reference color equals the bit-replicated channels.
    pub replicate: usize,
    /// Words the reference expanded to more than one color.
    pub conflicts: Vec<String>,
}

/// Everything one audit found.
#[derive(Clone, Debug, Default)]
pub struct TextureAudit {
    /// One row per texture archive, in spelling order.
    pub archives: Vec<ArchiveAudit>,
    /// The reference's 565 expansion across the whole corpus.
    pub expansion: ExpansionTally,
}

impl TextureAudit {
    /// Every texture row.
    pub fn textures(&self) -> impl Iterator<Item = &TextureComparison> {
        self.archives.iter().flat_map(|archive| &archive.textures)
    }

    /// Unexplained differences, archive failures and expansion conflicts.
    pub fn unexplained(&self) -> usize {
        self.archives
            .iter()
            .map(ArchiveAudit::unexplained)
            .sum::<usize>()
            + self.expansion.conflicts.len()
    }

    /// Whether the audit passes.
    pub fn passes(&self) -> bool {
        self.unexplained() == 0
    }
}

/// Everything one `texture-audit` run produced.
#[derive(Debug)]
pub struct TextureAuditRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The audit, when it ran.
    pub audit: Option<TextureAudit>,
    /// The JSON report, when the audit ran.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
}

impl TextureAuditRun {
    fn failed(exit_code: u8, message: String) -> Self {
        Self {
            exit_code,
            audit: None,
            report: None,
            out: None,
            diagnostics: vec![message],
        }
    }
}

/// Runs the `texture-audit` command and returns its exit code.
pub fn texture_audit_command(args: &[String]) -> ExitCode {
    let run = texture_audit_command_result(args, std::env::var_os("CS_GAME_DIR"));
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!(
                "cs-inspect: wrote texture audit report to {}",
                path.display()
            );
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// The body of [`texture_audit_command`], with the environment's
/// installation passed in so tests can drive it.
pub fn texture_audit_command_result(
    args: &[String],
    env_cs_path: Option<OsString>,
) -> TextureAuditRun {
    let parsed = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return TextureAuditRun::failed(2, message),
    };
    let Some(reference) = parsed.reference.clone() else {
        return TextureAuditRun::failed(
            2,
            "cs-inspect texture-audit: --reference <dir> is required".to_owned(),
        );
    };
    let cs_path = parsed.cs_path.clone().or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return TextureAuditRun::failed(
            4,
            "no installation selected: pass --cs-path <dir> or set CS_GAME_DIR".to_owned(),
        );
    };
    for written in [&parsed.out, &parsed.sheet_dir].into_iter().flatten() {
        if inside(written, &cs_path) {
            return TextureAuditRun::failed(
                2,
                format!(
                    "{} lies inside the installation; cs-inspect never writes there",
                    written.display()
                ),
            );
        }
    }
    if !reference.is_dir() {
        return TextureAuditRun::failed(
            2,
            format!("reference directory {} does not exist", reference.display()),
        );
    }
    if let Some(dir) = &parsed.sheet_dir
        && let Err(error) = fs::create_dir_all(dir)
    {
        return TextureAuditRun::failed(1, format!("cannot create {}: {error}", dir.display()));
    }

    let found = match install::discover(&cs_path) {
        Ok(found) => found,
        Err(error) => return TextureAuditRun::failed(1, error.to_string()),
    };
    let context = ResolveContext::new(install::fingerprint(&found.manifest));
    let mut builder = SessionBuilder::new(context);
    if let Err(error) = builder.mount_installation(&cs_path, &found.diagnosis) {
        let code = match &error {
            SessionError::Source {
                error: SourceError::Member { .. } | SourceError::NonUtf8Name { .. },
                ..
            } => 3,
            _ => 1,
        };
        return TextureAuditRun::failed(code, error.to_string());
    }
    let session = builder.open();

    let mut archives: Vec<(String, String, AssetKey)> = Vec::new();
    let mut diagnostics = Vec::new();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        if !record.relative_spelling.logical_key().ends_with(".zbd") {
            continue;
        }
        match AssetKey::from_spelling("install", spelling, "default") {
            Ok(key) => archives.push((spelling.to_owned(), record.sha256.to_hex(), key)),
            Err(error) => diagnostics.push(format!("{spelling}: not an asset key: {error}")),
        }
    }
    archives.sort_by(|a, b| a.0.cmp(&b.0));

    let mut audit = TextureAudit::default();
    let mut colors: BTreeMap<u16, [u8; 3]> = BTreeMap::new();
    let mut runtime_error = None;
    for (spelling, sha256, key) in &archives {
        match audit_archive(
            &session,
            spelling,
            sha256,
            key,
            &reference,
            parsed.sheet_dir.as_deref(),
            &mut colors,
            &mut audit.expansion.conflicts,
        ) {
            Ok(Some(row)) => audit.archives.push(row),
            Ok(None) => {}
            Err(error) => {
                runtime_error = Some(format!("{spelling}: {error}"));
                break;
            }
        }
    }
    let _teardown = session.close();
    if let Some(message) = runtime_error {
        return TextureAuditRun::failed(1, message);
    }
    tally_expansion(&colors, &mut audit.expansion);

    for archive in &audit.archives {
        diagnostics.extend(archive_diagnostics(archive));
    }
    for conflict in &audit.expansion.conflicts {
        diagnostics.push(format!("reference expansion is inconsistent: {conflict}"));
    }
    let passes = audit.passes() && diagnostics.is_empty();
    let report = texture_audit_report_json(&found, &audit, &reference, passes);
    let mut exit_code = if passes { 0 } else { 3 };
    let out = match &parsed.out {
        Some(out) => match write_atomic(out, report.as_bytes()) {
            Ok(()) => Some(out.clone()),
            Err(error) => {
                diagnostics.push(format!("cannot write report to {}: {error}", out.display()));
                exit_code = 1;
                None
            }
        },
        None => None,
    };
    TextureAuditRun {
        exit_code,
        audit: Some(audit),
        report: Some(report),
        out,
        diagnostics,
    }
}

/// Audits one `.zbd` file; `Ok(None)` when the dispatch routes it to
/// another family. `Err` only for a failure to write the contact sheet.
#[allow(clippy::too_many_arguments)]
fn audit_archive(
    session: &ContentSession,
    spelling: &str,
    sha256: &str,
    key: &AssetKey,
    reference_dir: &Path,
    sheet_dir: Option<&Path>,
    colors: &mut BTreeMap<u16, [u8; 3]>,
    conflicts: &mut Vec<String>,
) -> io::Result<Option<ArchiveAudit>> {
    let catalog = TextureCatalog::open(session, std::slice::from_ref(key));
    let mut row = ArchiveAudit {
        spelling: spelling.to_owned(),
        sha256: sha256.to_owned(),
        reference: None,
        failure: None,
        textures: Vec::new(),
        extra_reference_entries: Vec::new(),
        sheet: None,
    };
    if let Some((_, error)) = catalog.failures().next() {
        if error.code() == "wrong_family" {
            return Ok(None);
        }
        row.failure = Some(Difference::unexplained(error.code(), error.to_string()));
        return Ok(Some(row));
    }
    let ids: Vec<(usize, String)> = catalog
        .archives()
        .flat_map(|archive| archive.ids())
        .map(|id| (id.entry_index, id.name.clone()))
        .collect();

    let reference_path = reference_dir.join(format!("{spelling}.zip"));
    let reference = match read_reference(&reference_path) {
        Ok(reference) => reference,
        Err(error) => {
            row.failure = Some(Difference::unexplained(
                "reference_unreadable",
                format!("{}: {error}", reference_path.display()),
            ));
            return Ok(Some(row));
        }
    };
    row.reference = Some(FileDigest {
        path: reference_path,
        sha256: reference.sha256.clone(),
    });

    let mut sheet = sheet_dir.map(|_| SheetBuilder::default());
    for (entry_index, name) in &ids {
        let upload = catalog
            .resolve_entry(session, key, *entry_index)
            .map_err(|error| error.to_string())
            .and_then(|resolved| {
                catalog
                    .prepare_upload(session, &resolved)
                    .map_err(|error| error.to_string())
            });
        let duplicate = ids.iter().filter(|(_, other)| other == name).count() > 1;
        let mut comparison = compare_texture(
            *entry_index,
            name,
            duplicate,
            upload.as_ref().ok(),
            reference.images.get(*entry_index),
            colors,
            conflicts,
        );
        if let Err(error) = &upload {
            comparison
                .differences
                .push(Difference::unexplained("decode_failed", error.clone()));
        }
        if let (Some(sheet), Ok(upload)) = (sheet.as_mut(), &upload) {
            comparison.sheet_cell = Some(sheet.place(upload));
        }
        row.textures.push(comparison);
    }
    row.extra_reference_entries = reference
        .images
        .iter()
        .skip(ids.len())
        .map(|image| image.name.clone())
        .collect();

    if let (Some(sheet), Some(dir)) = (sheet, sheet_dir) {
        let path = dir.join(format!("{}.tga", spelling.replace('/', "_")));
        let bytes = sheet.into_tga();
        write_atomic(&path, &bytes)?;
        row.sheet = Some(FileDigest {
            path,
            sha256: install::sha256(&bytes).to_hex(),
        });
    }
    Ok(Some(row))
}

/// The 565 word of every texel of an upload, top row first.
fn upload_words(upload: &TextureUpload) -> Option<Vec<u16>> {
    (upload.format() == DecodedFormat::Rgb565).then(|| {
        upload
            .rows()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect()
    })
}

/// The coverage the reference must show for each texel, or `None` when the
/// descriptor establishes no coverage.
fn expected_alpha(upload: &TextureUpload, words: &[u16]) -> Option<Vec<u8>> {
    match upload.image().alpha_source() {
        AlphaSource::Plane => upload.alpha_plane().map(<[u8]>::to_vec),
        AlphaSource::StoredValueKey { value } => Some(
            words
                .iter()
                .map(|word| if *word == value { 0 } else { 255 })
                .collect(),
        ),
        _ => None,
    }
}

fn alpha_code(source: AlphaSource) -> String {
    match source {
        AlphaSource::Opaque => "opaque".to_owned(),
        AlphaSource::Channel => "channel".to_owned(),
        AlphaSource::Plane => "plane".to_owned(),
        AlphaSource::StoredValueKey { value } => format!("stored_value_key_{value:04x}"),
        AlphaSource::PaletteKey { index } => format!("palette_key_{index}"),
        AlphaSource::Unknown => "unknown".to_owned(),
    }
}

/// The 5/6/5 fields the reference's 8-bit channels truncate to.
fn truncated(rgb: [u8; 3]) -> u16 {
    (u16::from(rgb[0] >> 3) << 11) | (u16::from(rgb[1] >> 2) << 5) | u16::from(rgb[2] >> 3)
}

/// Compares one decoded texture with the reference image at its position.
fn compare_texture(
    entry_index: usize,
    name: &str,
    duplicate: bool,
    upload: Option<&TextureUpload>,
    reference: Option<&ReferenceImage>,
    colors: &mut BTreeMap<u16, [u8; 3]>,
    conflicts: &mut Vec<String>,
) -> TextureComparison {
    let mut row = TextureComparison {
        entry_index,
        name: name.to_owned(),
        reference_name: reference.map(|image| image.name.clone()),
        extent: upload.map(|upload| (upload.extent().width, upload.extent().height)),
        stored: if upload.and_then(TextureUpload::indices).is_some() {
            "indexed565"
        } else {
            "rgb565"
        },
        alpha: upload.map_or_else(
            || "unknown".to_owned(),
            |upload| alpha_code(upload.image().alpha_source()),
        ),
        reference_extent: reference.map(|image| (image.width, image.height)),
        reference_channels: reference.map(|image| image.channels),
        orientation: Orientation::Unmatched,
        differences: Vec::new(),
        sheet_cell: None,
    };
    let Some(reference) = reference else {
        row.differences.push(Difference::unexplained(
            "reference_missing",
            format!("the reference has no image at position {entry_index}"),
        ));
        return row;
    };
    let renamed = duplicate
        && reference
            .name
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|suffix| suffix.parse::<u32>().is_ok());
    if reference.name != name && !renamed {
        row.differences.push(Difference::unexplained(
            "name",
            format!("stored {name:?}, reference {:?}", reference.name),
        ));
    }
    let Some(upload) = upload else {
        return row;
    };
    let (width, height) = (upload.extent().width, upload.extent().height);
    if (width, height) != (reference.width, reference.height) {
        row.differences.push(Difference::unexplained(
            "dimensions",
            format!(
                "decoded {width}x{height}, reference {}x{}",
                reference.width, reference.height
            ),
        ));
        return row;
    }
    let Some(words) = upload_words(upload) else {
        row.differences.push(Difference::unexplained(
            "format",
            format!("decoded format {:?} is not 565", upload.format()),
        ));
        return row;
    };
    let alpha = expected_alpha(upload, &words);
    match (&alpha, reference.channels) {
        (Some(_), 4) | (None, 3) => {}
        (None, 4) => row.differences.push(Difference::unexplained(
            "alpha_channel",
            format!(
                "the descriptor says {} but the reference carries coverage",
                row.alpha
            ),
        )),
        (Some(_), _) => row.differences.push(Difference::unexplained(
            "alpha_channel",
            format!(
                "the descriptor says {} but the reference carries no coverage",
                row.alpha
            ),
        )),
        (None, _) => {}
    }
    if upload.image().alpha_source() == AlphaSource::Unknown {
        row.differences.push(Difference {
            code: "alpha_source_unknown",
            explained: true,
            detail: "the descriptor leaves coverage unknown (palette texture with the simple \
                     alpha flag); the reference presents it opaque"
                .to_owned(),
        });
    }

    let texel = |x: u32, y: u32| -> ([u8; 3], u8) {
        let at = (y as usize * width as usize + x as usize) * usize::from(reference.channels);
        let pixel = &reference.pixels[at..at + usize::from(reference.channels)];
        let coverage = if reference.channels == 4 {
            pixel[3]
        } else {
            255
        };
        ([pixel[0], pixel[1], pixel[2]], coverage)
    };
    let arrangement = |orientation: Orientation, x: u32, y: u32| match orientation {
        Orientation::VerticalFlip => (x, height - 1 - y),
        Orientation::HorizontalFlip => (width - 1 - x, y),
        Orientation::Rotated180 => (width - 1 - x, height - 1 - y),
        Orientation::AsStored | Orientation::Unmatched => (x, y),
    };
    let color_mismatches = |orientation: Orientation| {
        let mut count = 0usize;
        let mut first = None;
        for y in 0..height {
            for x in 0..width {
                let (rx, ry) = arrangement(orientation, x, y);
                let word = words[(y * width + x) as usize];
                let (rgb, _) = texel(rx, ry);
                if truncated(rgb) != word {
                    count += 1;
                    first.get_or_insert((x, y, word, rgb));
                }
            }
        }
        (count, first)
    };

    let (mismatches, first) = color_mismatches(Orientation::AsStored);
    if mismatches == 0 {
        row.orientation = Orientation::AsStored;
    } else {
        row.orientation = [
            Orientation::VerticalFlip,
            Orientation::HorizontalFlip,
            Orientation::Rotated180,
        ]
        .into_iter()
        .find(|orientation| color_mismatches(*orientation).0 == 0)
        .unwrap_or(Orientation::Unmatched);
        let (x, y, word, rgb) = first.expect("a mismatch was counted");
        row.differences.push(Difference::unexplained(
            "color",
            format!(
                "{mismatches} of {} texels differ; first at x={x} y={y}: stored {word:#06x}, \
                 reference rgb({}, {}, {}); the reference matches when arranged as {}",
                words.len(),
                rgb[0],
                rgb[1],
                rgb[2],
                row.orientation.code()
            ),
        ));
    }
    if row.orientation == Orientation::AsStored {
        for y in 0..height {
            for x in 0..width {
                let word = words[(y * width + x) as usize];
                let (rgb, _) = texel(x, y);
                let seen = colors.entry(word).or_insert(rgb);
                if *seen != rgb {
                    conflicts.push(format!(
                        "{word:#06x} is rgb({}, {}, {}) in one texture and rgb({}, {}, {}) in \
                         {name}#{entry_index} at x={x} y={y}",
                        seen[0], seen[1], seen[2], rgb[0], rgb[1], rgb[2]
                    ));
                    *seen = rgb;
                }
            }
        }
    }
    if let Some(alpha) = alpha
        && reference.channels == 4
    {
        let mut count = 0usize;
        let mut first = None;
        for y in 0..height {
            for x in 0..width {
                let expected = alpha[(y * width + x) as usize];
                let (_, coverage) = texel(x, y);
                if coverage != expected {
                    count += 1;
                    first.get_or_insert((x, y, expected, coverage));
                }
            }
        }
        if let Some((x, y, expected, coverage)) = first {
            row.differences.push(Difference::unexplained(
                "alpha",
                format!(
                    "{count} of {} texels differ; first at x={x} y={y}: decoded {expected}, \
                     reference {coverage}",
                    alpha.len()
                ),
            ));
        }
    }
    row
}

/// Counts which expansion the reference used for every distinct word.
fn tally_expansion(colors: &BTreeMap<u16, [u8; 3]>, tally: &mut ExpansionTally) {
    let lerp = |value: u16, max: u16| {
        ((u32::from(value) * 255 + u32::from(max) / 2) / u32::from(max)) as u8
    };
    let replicate5 = |value: u16| ((value << 3) | (value >> 2)) as u8;
    let replicate6 = |value: u16| ((value << 2) | (value >> 4)) as u8;
    tally.words = colors.len();
    for (word, rgb) in colors {
        let (r, g, b) = (word >> 11, (word >> 5) & 0x3F, word & 0x1F);
        if *rgb == [lerp(r, 31), lerp(g, 63), lerp(b, 31)] {
            tally.lerp += 1;
        }
        if *rgb == [replicate5(r), replicate6(g), replicate5(b)] {
            tally.replicate += 1;
        }
    }
}

fn archive_diagnostics(archive: &ArchiveAudit) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(failure) = &archive.failure {
        lines.push(format!(
            "{}: failed ({}): {}",
            archive.spelling, failure.code, failure.detail
        ));
    }
    if !archive.extra_reference_entries.is_empty() {
        lines.push(format!(
            "{}: the reference holds {} images past the table: {:?}",
            archive.spelling,
            archive.extra_reference_entries.len(),
            archive.extra_reference_entries
        ));
    }
    for texture in &archive.textures {
        for difference in texture.differences.iter().filter(|d| !d.explained) {
            lines.push(format!(
                "{}#{}:{}: {} differs: {}",
                archive.spelling,
                texture.entry_index,
                texture.name,
                difference.code,
                difference.detail
            ));
        }
    }
    lines
}

// --- report ------------------------------------------------------------------

/// Renders the audit report.
pub fn texture_audit_report_json(
    found: &install::Discovery,
    audit: &TextureAudit,
    reference: &Path,
    passes: bool,
) -> String {
    let mut verdicts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut orientations: BTreeMap<&str, usize> = BTreeMap::new();
    let mut explained: BTreeMap<&str, usize> = BTreeMap::new();
    for texture in audit.textures() {
        *verdicts.entry(texture.verdict()).or_default() += 1;
        *orientations.entry(texture.orientation.code()).or_default() += 1;
        for difference in texture.differences.iter().filter(|d| d.explained) {
            *explained.entry(difference.code).or_default() += 1;
        }
    }
    let counts = |map: &BTreeMap<&str, usize>| {
        map.iter()
            .map(|(key, count)| format!("{}: {count}", jstr(key)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(
        out,
        "  \"report_version\": {},",
        jstr(TEXTURE_AUDIT_REPORT_VERSION)
    );
    let _ = writeln!(
        out,
        "  \"install_sha256\": \"{}\",",
        install::fingerprint(&found.manifest).to_hex()
    );
    let _ = writeln!(
        out,
        "  \"content_sha256\": \"{}\",",
        install::content_fingerprint(&found.manifest).to_hex()
    );
    let _ = writeln!(
        out,
        "  \"reference\": {{\"dir\": {}, \"extractor\": \"unzbd cs textures (mech3ax v0.6.0, \
         d3521a9721be731d365504568ddcd78e3f9846bb)\"}},",
        jstr(&reference.display().to_string())
    );
    let _ = writeln!(out, "  \"consumer\": \"TextureCatalog::prepare_upload\",");
    let _ = writeln!(out, "  \"passes\": {passes},");
    let _ = writeln!(
        out,
        "  \"totals\": {{\"archives\": {}, \"textures\": {}, \"unexplained\": {}, \"verdicts\": \
         {{{}}}, \"orientations\": {{{}}}, \"explained\": {{{}}}}},",
        audit.archives.len(),
        audit.textures().count(),
        audit.unexplained(),
        counts(&verdicts),
        counts(&orientations),
        counts(&explained)
    );
    let _ = writeln!(
        out,
        "  \"reference_expansion\": {{\"words\": {}, \"lerp\": {}, \"replicate\": {}, \
         \"conflicts\": [{}]}},",
        audit.expansion.words,
        audit.expansion.lerp,
        audit.expansion.replicate,
        audit
            .expansion
            .conflicts
            .iter()
            .map(|conflict| jstr(conflict))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(out, "  \"archives\": [");
    for (position, archive) in audit.archives.iter().enumerate() {
        let digest = |file: &Option<FileDigest>| {
            file.as_ref().map_or_else(
                || "null".to_owned(),
                |file| {
                    format!(
                        "{{\"path\": {}, \"sha256\": \"{}\"}}",
                        jstr(&file.path.display().to_string()),
                        file.sha256
                    )
                },
            )
        };
        let _ = writeln!(out, "    {{");
        let _ = writeln!(out, "      \"spelling\": {},", jstr(&archive.spelling));
        let _ = writeln!(out, "      \"sha256\": \"{}\",", archive.sha256);
        let _ = writeln!(out, "      \"reference\": {},", digest(&archive.reference));
        let _ = writeln!(out, "      \"sheet\": {},", digest(&archive.sheet));
        let _ = writeln!(
            out,
            "      \"failure\": {},",
            archive
                .failure
                .as_ref()
                .map_or_else(|| "null".to_owned(), difference_json)
        );
        let _ = writeln!(
            out,
            "      \"extra_reference_entries\": [{}],",
            archive
                .extra_reference_entries
                .iter()
                .map(|name| jstr(name))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(out, "      \"textures\": [");
        for (index, texture) in archive.textures.iter().enumerate() {
            let separator = if index + 1 == archive.textures.len() {
                ""
            } else {
                ","
            };
            let _ = writeln!(out, "        {}{separator}", texture_json(texture));
        }
        let _ = writeln!(out, "      ]");
        let separator = if position + 1 == audit.archives.len() {
            ""
        } else {
            ","
        };
        let _ = writeln!(out, "    }}{separator}");
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
    out
}

fn pair_json(pair: Option<(u32, u32)>) -> String {
    pair.map_or_else(|| "null".to_owned(), |(a, b)| format!("[{a}, {b}]"))
}

fn difference_json(difference: &Difference) -> String {
    format!(
        "{{\"code\": {}, \"explained\": {}, \"detail\": {}}}",
        jstr(difference.code),
        difference.explained,
        jstr(&difference.detail)
    )
}

fn texture_json(texture: &TextureComparison) -> String {
    format!(
        "{{\"entry\": {}, \"name\": {}, \"reference_name\": {}, \"extent\": {}, \"stored\": \
         \"{}\", \"alpha\": {}, \"reference_extent\": {}, \"reference_channels\": {}, \
         \"orientation\": \"{}\", \"verdict\": \"{}\", \"sheet_cell\": {}, \"differences\": \
         [{}]}}",
        texture.entry_index,
        jstr(&texture.name),
        texture
            .reference_name
            .as_deref()
            .map_or_else(|| "null".to_owned(), jstr),
        pair_json(texture.extent),
        texture.stored,
        jstr(&texture.alpha),
        pair_json(texture.reference_extent),
        texture
            .reference_channels
            .map_or_else(|| "null".to_owned(), |channels| channels.to_string()),
        texture.orientation.code(),
        texture.verdict(),
        texture.sheet_cell.map_or_else(
            || "null".to_owned(),
            |(x, y, w, h)| format!("[{x}, {y}, {w}, {h}]")
        ),
        texture
            .differences
            .iter()
            .map(difference_json)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A JSON string literal.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Whether `path` would land inside `root` once both are resolved as far as
/// they exist.
fn inside(path: &Path, root: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut existing = absolute.as_path();
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(canonical) = fs::canonicalize(existing) {
            let mut full = canonical;
            full.extend(rest.iter().rev());
            return full.starts_with(&root);
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return false,
        }
    }
}

/// Writes `bytes` to `out` atomically via a sibling temporary file.
fn write_atomic(out: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temp_name = out.as_os_str().to_owned();
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp_name);
    let result = fs::write(&temp, bytes).and_then(|()| fs::rename(&temp, out));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

// --- contact sheet -----------------------------------------------------------

/// One placed texture: its origin, its size and its review-rendered RGBA
/// texels.
struct SheetCell {
    origin: (u32, u32),
    size: (u32, u32),
    texels: Vec<[u8; 4]>,
}

/// Shelf layout of one archive's textures, kept as the uploads' review
/// rendering until the sheet is written.
#[derive(Default)]
struct SheetBuilder {
    cells: Vec<SheetCell>,
    cursor: (u32, u32),
    shelf_height: u32,
    width: u32,
}

impl SheetBuilder {
    /// Places one texture and returns its cell.
    fn place(&mut self, upload: &TextureUpload) -> (u32, u32, u32, u32) {
        let (width, height) = (upload.extent().width, upload.extent().height);
        let limit = SHEET_WIDTH.max(width);
        if self.cursor.0 > 0 && self.cursor.0 + width > limit {
            self.cursor = (0, self.cursor.1 + self.shelf_height + SHEET_GUTTER);
            self.shelf_height = 0;
        }
        let origin = self.cursor;
        self.cursor.0 += width + SHEET_GUTTER;
        self.shelf_height = self.shelf_height.max(height);
        self.width = self.width.max(origin.0 + width);
        let words = upload_words(upload).unwrap_or_default();
        let alpha = expected_alpha(upload, &words);
        let texels = words
            .iter()
            .enumerate()
            .map(|(at, word)| {
                let (r, g, b) = (word >> 11, (word >> 5) & 0x3F, word & 0x1F);
                [
                    ((r << 3) | (r >> 2)) as u8,
                    ((g << 2) | (g >> 4)) as u8,
                    ((b << 3) | (b >> 2)) as u8,
                    alpha.as_ref().map_or(255, |plane| plane[at]),
                ]
            })
            .collect();
        self.cells.push(SheetCell {
            origin,
            size: (width, height),
            texels,
        });
        (origin.0, origin.1, width, height)
    }

    /// An uncompressed 32-bit TGA, top-left origin, 8 alpha bits.
    fn into_tga(self) -> Vec<u8> {
        let width = self.width.max(1);
        let height = (self.cursor.1 + self.shelf_height).max(1);
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        for SheetCell {
            origin: (x0, y0),
            size: (w, h),
            texels,
        } in &self.cells
        {
            for y in 0..*h {
                for x in 0..*w {
                    let [r, g, b, a] = texels[(y * w + x) as usize];
                    let at = (((y0 + y) * width + x0 + x) * 4) as usize;
                    pixels[at..at + 4].copy_from_slice(&[b, g, r, a]);
                }
            }
        }
        let mut out = Vec::with_capacity(18 + pixels.len());
        out.extend_from_slice(&[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&(width as u16).to_le_bytes());
        out.extend_from_slice(&(height as u16).to_le_bytes());
        out.extend_from_slice(&[32, 0x28]);
        out.extend_from_slice(&pixels);
        out
    }
}

// --- reference reader --------------------------------------------------------

/// One reference image: the PNG's name without `.png`, its size and its
/// 8-bit channels, top row first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceImage {
    /// The member name without `.png`.
    pub name: String,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// 3 (RGB) or 4 (RGBA).
    pub channels: u8,
    /// Row-major channels.
    pub pixels: Vec<u8>,
}

/// One reference file: its images in member order and its digest.
#[derive(Clone, Debug)]
pub struct Reference {
    /// The PNG members, in the order the ZIP stores them.
    pub images: Vec<ReferenceImage>,
    /// SHA-256 of the file.
    pub sha256: String,
}

/// Reads one reference ZIP.
///
/// # Errors
///
/// A readable message when the file is missing, too large, not a ZIP this
/// reader supports, fails its CRC, or holds a PNG it cannot read.
pub fn read_reference(path: &Path) -> Result<Reference, String> {
    let length = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if length > MAX_REFERENCE_BYTES {
        return Err(format!(
            "{length} bytes exceeds the {MAX_REFERENCE_BYTES}-byte limit"
        ));
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut images = Vec::new();
    for (name, data) in zip_members(&bytes)? {
        if let Some(stem) = name.strip_suffix(".png") {
            let (width, height, channels, pixels) =
                read_png(&data).map_err(|error| format!("{name}: {error}"))?;
            images.push(ReferenceImage {
                name: stem.to_owned(),
                width,
                height,
                channels,
                pixels,
            });
        } else if name != "manifest.json" {
            return Err(format!("unexpected member {name:?}"));
        }
    }
    Ok(Reference {
        images,
        sha256: install::sha256(&bytes).to_hex(),
    })
}

fn le16(bytes: &[u8], at: usize) -> Result<usize, String> {
    bytes
        .get(at..at + 2)
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
        .ok_or_else(|| format!("truncated at {at}"))
}

fn le32(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("truncated at {at}"))
}

fn be32(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("truncated at {at}"))
}

/// CRC-32 (ISO-HDLC), as ZIP stores it.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Every member of a ZIP file in central-directory order, stored or
/// deflated, each checked against its CRC.
fn zip_members(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    const EOCD: u32 = 0x0605_4b50;
    const CENTRAL: u32 = 0x0201_4b50;
    const LOCAL: u32 = 0x0403_4b50;
    let floor = bytes.len().saturating_sub(22 + 0xFFFF);
    let eocd = (floor..=bytes.len().saturating_sub(22))
        .rev()
        .find(|at| le32(bytes, *at) == Ok(EOCD))
        .ok_or("no end-of-central-directory record")?;
    let count = le16(bytes, eocd + 10)?;
    let mut at = le32(bytes, eocd + 16)? as usize;
    let mut members = Vec::with_capacity(count.min(1 << 16));
    for _ in 0..count {
        if le32(bytes, at)? != CENTRAL {
            return Err(format!("no central directory entry at {at}"));
        }
        let method = le16(bytes, at + 10)?;
        let crc = le32(bytes, at + 16)?;
        let packed = le32(bytes, at + 20)? as usize;
        let size = le32(bytes, at + 24)? as usize;
        let name_len = le16(bytes, at + 28)?;
        let skip = le16(bytes, at + 30)? + le16(bytes, at + 32)?;
        let local = le32(bytes, at + 42)? as usize;
        let name = bytes
            .get(at + 46..at + 46 + name_len)
            .ok_or("truncated name")?;
        let name = String::from_utf8(name.to_vec()).map_err(|_| "member name is not UTF-8")?;
        at += 46 + name_len + skip;
        if le32(bytes, local)? != LOCAL {
            return Err(format!("{name}: no local header at {local}"));
        }
        let start = local + 30 + le16(bytes, local + 26)? + le16(bytes, local + 28)?;
        let raw = bytes
            .get(start..start + packed)
            .ok_or_else(|| format!("{name}: data runs past the file"))?;
        let data = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, size)
                .map_err(|error| format!("{name}: deflate: {error}"))?,
            other => return Err(format!("{name}: compression method {other} unsupported")),
        };
        if data.len() != size {
            return Err(format!("{name}: {} bytes, declared {size}", data.len()));
        }
        if crc32(&data) != crc {
            return Err(format!("{name}: CRC mismatch"));
        }
        members.push((name, data));
    }
    Ok(members)
}

/// Reads an 8-bit, non-interlaced RGB or RGBA PNG: width, height, channels
/// and the unfiltered pixels.
fn read_png(bytes: &[u8]) -> Result<(u32, u32, u8, Vec<u8>), String> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.get(..8) != Some(&SIGNATURE[..]) {
        return Err("not a PNG".to_owned());
    }
    let mut at = 8;
    let mut header = None;
    let mut compressed = Vec::new();
    loop {
        let length = be32(bytes, at)? as usize;
        let kind = bytes.get(at + 4..at + 8).ok_or("truncated chunk")?;
        let data = bytes
            .get(at + 8..at + 8 + length)
            .ok_or("chunk runs past the file")?;
        match kind {
            b"IHDR" => {
                if length != 13 {
                    return Err("IHDR is not 13 bytes".to_owned());
                }
                header = Some((be32(data, 0)?, be32(data, 4)?, data[8], data[9], data[12]));
            }
            b"IDAT" => compressed.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + length;
    }
    let (width, height, depth, color, interlace) = header.ok_or("no IHDR")?;
    let channels: u8 = match (depth, color, interlace) {
        (8, 2, 0) => 3,
        (8, 6, 0) => 4,
        _ => {
            return Err(format!(
                "depth {depth}, color type {color}, interlace {interlace} unsupported"
            ));
        }
    };
    if width == 0 || height == 0 || width > 1 << 14 || height > 1 << 14 {
        return Err(format!("{width}x{height} is outside the supported size"));
    }
    let stride = width as usize * usize::from(channels);
    let expected = height as usize * (stride + 1);
    let filtered = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&compressed, expected)
        .map_err(|error| format!("zlib: {error}"))?;
    if filtered.len() != expected {
        return Err(format!(
            "{} image bytes, expected {expected}",
            filtered.len()
        ));
    }
    let bpp = usize::from(channels);
    let mut pixels = vec![0u8; height as usize * stride];
    for y in 0..height as usize {
        let filter = filtered[y * (stride + 1)];
        let line = &filtered[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, rest) = pixels.split_at_mut(y * stride);
        let previous = done
            .get(done.len().saturating_sub(stride)..)
            .filter(|_| y > 0);
        let current = &mut rest[..stride];
        for i in 0..stride {
            let left = if i >= bpp { current[i - bpp] } else { 0 };
            let up = previous.map_or(0, |row| row[i]);
            let up_left = match previous {
                Some(row) if i >= bpp => row[i - bpp],
                _ => 0,
            };
            let predicted = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => paeth(left, up, up_left),
                other => return Err(format!("row {y}: filter type {other}")),
            };
            current[i] = line[i].wrapping_add(predicted);
        }
    }
    Ok((width, height, channels, pixels))
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let estimate = i16::from(left) + i16::from(up) - i16::from(up_left);
    let distance_left = (estimate - i16::from(left)).abs();
    let distance_up = (estimate - i16::from(up)).abs();
    let distance_up_left = (estimate - i16::from(up_left)).abs();
    if distance_left <= distance_up && distance_left <= distance_up_left {
        left
    } else if distance_up <= distance_up_left {
        up
    } else {
        up_left
    }
}

#[cfg(test)]
mod tests {
    //! Acceptance stage F08-D (`specs/F08-texture-archives-and-conventional-image-decoding.md`,
    //! section `### F08-D`, AC04): private decodes compared against the
    //! pinned reference with zero unexplained pixel differences.
    //!
    //! The synthetic installations, reference PNGs and ZIPs are newly authored
    //! bytes under the system temporary directory, removed on drop; the
    //! reference colors are computed here from the authored 565 words with
    //! the rounding expansion, independently of the code under test. The
    //! retail test reads `$CS_GAME_DIR` (never writes it) and the private
    //! reference extraction, and fails loudly without either.
    //! `evidence_report_f08_d_writes_the_acceptance_report` is the evidence
    //! harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test.

    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const FLAG_BPP2: u32 = 1;
    const FLAG_HAS_ALPHA: u32 = 2;
    const FLAG_NO_ALPHA: u32 = 4;
    const FLAG_FULL_ALPHA: u32 = 8;
    const OPAQUE: u32 = FLAG_BPP2 | FLAG_NO_ALPHA;
    const SIMPLE: u32 = FLAG_BPP2 | FLAG_HAS_ALPHA;
    const FULL: u32 = FLAG_BPP2 | FLAG_HAS_ALPHA | FLAG_FULL_ALPHA;

    // 565 words whose two bytes differ, so a byte swap is visible.
    const RED: u16 = 0xF800;
    const GREEN: u16 = 0x07E0;
    const BLUE: u16 = 0x001F;
    const YELLOW: u16 = 0xFFE0;
    const CYAN: u16 = 0x07FF;
    const MAGENTA: u16 = 0xF81F;
    const GREY: u16 = 0x8410;

    /// One authored texture: its table entry, its stored level and the image
    /// the reference extractor would show for it.
    #[derive(Clone)]
    struct Tex {
        name: &'static str,
        flags: u32,
        width: u16,
        height: u16,
        words: Vec<u16>,
        indices: Vec<u8>,
        alpha: Vec<u8>,
        palette: Vec<u16>,
    }

    impl Tex {
        fn direct(name: &'static str, flags: u32, width: u16, height: u16, words: &[u16]) -> Self {
            Self {
                name,
                flags,
                width,
                height,
                words: words.to_vec(),
                indices: Vec::new(),
                alpha: Vec::new(),
                palette: Vec::new(),
            }
        }

        fn body(&self) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&self.flags.to_le_bytes());
            out.extend_from_slice(&self.width.to_le_bytes());
            out.extend_from_slice(&self.height.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(self.palette.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            for word in &self.words {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out.extend_from_slice(&self.indices);
            out.extend_from_slice(&self.alpha);
            for word in &self.palette {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out
        }

        /// The 565 word each texel shows, top row first.
        fn shown(&self) -> Vec<u16> {
            if self.palette.is_empty() {
                self.words.clone()
            } else {
                self.indices
                    .iter()
                    .map(|index| self.palette[usize::from(*index)])
                    .collect()
            }
        }

        /// The reference image: RGB, or RGBA when the texture has a plane or
        /// a direct-color simple alpha (transparent where the word is 0).
        fn reference(&self, name: &str) -> RefImage {
            let shown = self.shown();
            let coverage: Option<Vec<u8>> = if self.flags & FLAG_FULL_ALPHA != 0 {
                Some(self.alpha.clone())
            } else if self.flags & FLAG_HAS_ALPHA != 0 && self.palette.is_empty() {
                Some(
                    shown
                        .iter()
                        .map(|w| if *w == 0 { 0 } else { 255 })
                        .collect(),
                )
            } else {
                None
            };
            let channels = if coverage.is_some() { 4 } else { 3 };
            let mut pixels = Vec::new();
            for (at, word) in shown.iter().enumerate() {
                pixels.extend_from_slice(&lerp(*word));
                if let Some(coverage) = &coverage {
                    pixels.push(coverage[at]);
                }
            }
            RefImage {
                name: name.to_owned(),
                width: u32::from(self.width),
                height: u32::from(self.height),
                channels,
                pixels,
            }
        }
    }

    /// The expansion the pinned reference uses: `round(c * 255 / max)`.
    fn lerp(word: u16) -> [u8; 3] {
        let channel = |value: u16, max: u32| ((u32::from(value) * 255 * 2 + max) / (2 * max)) as u8;
        [
            channel(word >> 11, 31),
            channel((word >> 5) & 0x3F, 63),
            channel(word & 0x1F, 31),
        ]
    }

    fn package(textures: &[Tex]) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [0u32, 1, 0, textures.len() as u32, 0, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        let mut offset = 24 + textures.len() * 40;
        let bodies: Vec<Vec<u8>> = textures.iter().map(Tex::body).collect();
        for (texture, body) in textures.iter().zip(&bodies) {
            let mut name = [0u8; 32];
            name[..texture.name.len()].copy_from_slice(texture.name.as_bytes());
            out.extend_from_slice(&name);
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            out.extend_from_slice(&(-1i32).to_le_bytes());
            offset += body.len();
        }
        for body in bodies {
            out.extend_from_slice(&body);
        }
        out
    }

    /// A reference image as the test authors it.
    #[derive(Clone)]
    struct RefImage {
        name: String,
        width: u32,
        height: u32,
        channels: u8,
        pixels: Vec<u8>,
    }

    impl RefImage {
        fn at(&mut self, x: u32, y: u32) -> &mut [u8] {
            let channels = usize::from(self.channels);
            let at = (y * self.width + x) as usize * channels;
            &mut self.pixels[at..at + channels]
        }

        fn flipped_vertically(&self) -> Self {
            let stride = self.width as usize * usize::from(self.channels);
            let pixels = self
                .pixels
                .chunks(stride)
                .rev()
                .flatten()
                .copied()
                .collect();
            Self {
                pixels,
                ..self.clone()
            }
        }

        fn flipped_horizontally(&self) -> Self {
            let channels = usize::from(self.channels);
            let stride = self.width as usize * channels;
            let pixels = self
                .pixels
                .chunks(stride)
                .flat_map(|row| row.chunks(channels).rev().flatten().copied())
                .collect();
            Self {
                pixels,
                ..self.clone()
            }
        }

        fn transposed(&self) -> Self {
            let channels = usize::from(self.channels);
            let mut pixels = Vec::new();
            for x in 0..self.width as usize {
                for y in 0..self.height as usize {
                    let at = (y * self.width as usize + x) * channels;
                    pixels.extend_from_slice(&self.pixels[at..at + channels]);
                }
            }
            Self {
                width: self.height,
                height: self.width,
                pixels,
                ..self.clone()
            }
        }
    }

    fn test_paeth(left: u8, up: u8, up_left: u8) -> u8 {
        let p = i32::from(left) + i32::from(up) - i32::from(up_left);
        let (pa, pb, pc) = (
            (p - i32::from(left)).abs(),
            (p - i32::from(up)).abs(),
            (p - i32::from(up_left)).abs(),
        );
        if pa <= pb && pa <= pc {
            left
        } else if pb <= pc {
            up
        } else {
            up_left
        }
    }

    fn adler32(bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for byte in bytes {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        (b << 16) | a
    }

    /// A PNG whose rows use filter type `y % 5`, so every filter the reader
    /// undoes appears; the zlib stream is made of stored blocks.
    fn png(image: &RefImage) -> Vec<u8> {
        let bpp = usize::from(image.channels);
        let stride = image.width as usize * bpp;
        let mut filtered = Vec::new();
        for y in 0..image.height as usize {
            let filter = (y % 5) as u8;
            filtered.push(filter);
            let row = &image.pixels[y * stride..(y + 1) * stride];
            let previous = (y > 0).then(|| &image.pixels[(y - 1) * stride..y * stride]);
            for i in 0..stride {
                let left = if i >= bpp { row[i - bpp] } else { 0 };
                let up = previous.map_or(0, |p| p[i]);
                let up_left = previous.filter(|_| i >= bpp).map_or(0, |p| p[i - bpp]);
                let predicted = match filter {
                    0 => 0,
                    1 => left,
                    2 => up,
                    3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                    _ => test_paeth(left, up, up_left),
                };
                filtered.push(row[i].wrapping_sub(predicted));
            }
        }
        let mut zlib = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = filtered.chunks(65_535).collect();
        for (index, block) in blocks.iter().enumerate() {
            zlib.push(u8::from(index + 1 == blocks.len()));
            let length = block.len() as u16;
            zlib.extend_from_slice(&length.to_le_bytes());
            zlib.extend_from_slice(&(!length).to_le_bytes());
            zlib.extend_from_slice(block);
        }
        zlib.extend_from_slice(&adler32(&filtered).to_be_bytes());

        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut chunk = |kind: &[u8], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            out.extend_from_slice(&[0, 0, 0, 0]);
        };
        let mut header = Vec::new();
        header.extend_from_slice(&image.width.to_be_bytes());
        header.extend_from_slice(&image.height.to_be_bytes());
        header.extend_from_slice(&[8, if image.channels == 4 { 6 } else { 2 }, 0, 0, 0]);
        chunk(b"IHDR", &header);
        // Two IDAT chunks, so the reader must join them.
        let half = zlib.len() / 2;
        chunk(b"IDAT", &zlib[..half]);
        chunk(b"IDAT", &zlib[half..]);
        chunk(b"IEND", &[]);
        out
    }

    /// A ZIP of stored members with correct CRCs, then `manifest.json`.
    fn zip(images: &[RefImage]) -> Vec<u8> {
        let mut members: Vec<(String, Vec<u8>)> = images
            .iter()
            .map(|image| (format!("{}.png", image.name), png(image)))
            .collect();
        members.push(("manifest.json".to_owned(), b"{}".to_vec()));
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in &members {
            let local = out.len() as u32;
            let crc = crc32(data);
            let fixed = |sig: u32, out: &mut Vec<u8>, central_entry: bool| {
                out.extend_from_slice(&sig.to_le_bytes());
                if central_entry {
                    out.extend_from_slice(&20u16.to_le_bytes());
                }
                out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
                out.extend_from_slice(&crc.to_le_bytes());
                out.extend_from_slice(&(data.len() as u32).to_le_bytes());
                out.extend_from_slice(&(data.len() as u32).to_le_bytes());
                out.extend_from_slice(&(name.len() as u16).to_le_bytes());
                out.extend_from_slice(&0u16.to_le_bytes());
            };
            fixed(0x0403_4b50, &mut out, false);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            fixed(0x0201_4b50, &mut central, true);
            // Comment length, disk, internal and external attributes.
            central.extend_from_slice(&[0; 10]);
            central.extend_from_slice(&local.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable installation, reference directory and output directory.
    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f08-d-{}-{}",
                std::process::id(),
                NEXT_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("install")).expect("fixture root");
            fs::create_dir_all(root.join("reference")).expect("reference root");
            Self { root }
        }

        fn install(&self) -> PathBuf {
            self.root.join("install")
        }

        fn reference(&self) -> PathBuf {
            self.root.join("reference")
        }

        fn write(&self, path: &Path, bytes: &[u8]) {
            fs::create_dir_all(path.parent().expect("a parent")).expect("dirs");
            fs::write(path, bytes).expect("written");
        }

        /// Writes the archive into the installation and its reference.
        fn archive(&self, spelling: &str, textures: &[Tex], reference: &[RefImage]) {
            self.write(&self.install().join(spelling), &package(textures));
            self.write(
                &self.reference().join(format!("{spelling}.zip")),
                &zip(reference),
            );
        }

        fn run(&self, extra: &[&str]) -> TextureAuditRun {
            let mut args = vec![
                "--cs-path".to_owned(),
                self.install().display().to_string(),
                "--reference".to_owned(),
                self.reference().display().to_string(),
            ];
            args.extend(extra.iter().map(|arg| (*arg).to_owned()));
            texture_audit_command_result(&args, None)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// One texture of every stored variant, plus a name stored twice.
    fn world_one() -> Vec<Tex> {
        vec![
            // AC01: 3x2 asymmetric, every texel different.
            Tex::direct(
                "sky",
                OPAQUE,
                3,
                2,
                &[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA],
            ),
            // Alpha edge: 0 and 255 at the corners of a non-square plane.
            Tex {
                alpha: vec![0, 128, 1, 254, 77, 255],
                ..Tex::direct(
                    "smoke",
                    FULL,
                    2,
                    3,
                    &[BLUE, RED, GREY, 0x0001, YELLOW, GREEN],
                )
            },
            // Direct-color simple alpha: the black texel is the key.
            Tex::direct("fence", SIMPLE, 2, 2, &[0x0000, GREY, 0x0020, 0x0000]),
            Tex {
                indices: vec![2, 0, 1, 1, 0, 2],
                palette: vec![CYAN, MAGENTA, GREY],
                ..Tex::direct("sign", OPAQUE, 3, 2, &[])
            },
            Tex {
                indices: vec![1, 0, 0],
                alpha: vec![255, 0, 9],
                palette: vec![RED, BLUE],
                ..Tex::direct("flag", FULL, 1, 3, &[])
            },
            // Palette simple alpha: coverage unknown, the reference is opaque.
            Tex {
                indices: vec![0, 1],
                palette: vec![0x0000, YELLOW],
                ..Tex::direct("rope", SIMPLE, 2, 1, &[])
            },
            Tex::direct("twin", OPAQUE, 1, 1, &[RED]),
            Tex::direct("twin", OPAQUE, 1, 1, &[BLUE]),
        ]
    }

    fn references(textures: &[Tex]) -> Vec<RefImage> {
        let mut seen: Vec<&str> = Vec::new();
        textures
            .iter()
            .map(|texture| {
                let repeats = seen.iter().filter(|name| **name == texture.name).count();
                seen.push(texture.name);
                let name = if repeats == 0 {
                    texture.name.to_owned()
                } else {
                    format!("{}-{repeats}", texture.name)
                };
                texture.reference(&name)
            })
            .collect()
    }

    fn texture<'a>(audit: &'a TextureAudit, archive: &str, entry: usize) -> &'a TextureComparison {
        &audit
            .archives
            .iter()
            .find(|row| row.spelling == archive)
            .unwrap_or_else(|| panic!("{archive} is audited"))
            .textures[entry]
    }

    fn codes(texture: &TextureComparison) -> Vec<&'static str> {
        texture.differences.iter().map(|d| d.code).collect()
    }

    const C1: &str = "ZBD/c1/texture.zbd";
    const C2: &str = "ZBD/c2/rtexture2.zbd";

    /// A two-world tree whose reference matches exactly.
    fn matching_tree() -> Tree {
        let tree = Tree::new();
        let one = world_one();
        tree.archive(C1, &one, &references(&one));
        let two = vec![Tex::direct(
            "sky",
            OPAQUE,
            3,
            2,
            &[MAGENTA, CYAN, YELLOW, BLUE, GREEN, RED],
        )];
        tree.archive(C2, &two, &references(&two));
        tree
    }

    /// AC04: every texture of every archive is compared through the upload
    /// boundary and matches; the one explained difference is listed; the
    /// contact sheet holds each texture at its cell.
    #[test]
    fn accept_f08_d_matching_reference_passes_with_every_texel_compared() {
        let tree = matching_tree();
        let sheets = tree.root.join("sheets");
        let out = tree.root.join("audit.json");
        let run = tree.run(&[
            "--sheet-dir",
            sheets.to_str().expect("UTF-8"),
            "--out",
            out.to_str().expect("UTF-8"),
        ]);
        assert_eq!(run.exit_code, 0, "{:?}", run.diagnostics);
        assert!(run.diagnostics.is_empty(), "{:?}", run.diagnostics);
        let audit = run.audit.expect("the audit ran");
        assert_eq!(audit.archives.len(), 2);
        assert_eq!(audit.textures().count(), 9);
        assert_eq!(audit.unexplained(), 0);
        for row in audit.textures() {
            assert_eq!(row.orientation, Orientation::AsStored, "{}", row.name);
        }
        let rope = texture(&audit, C1, 5);
        assert_eq!(rope.verdict(), "explained");
        assert_eq!(codes(rope), vec!["alpha_source_unknown"]);
        assert_eq!(rope.alpha, "unknown");
        assert_eq!(texture(&audit, C1, 2).alpha, "stored_value_key_0000");
        assert_eq!(texture(&audit, C1, 1).alpha, "plane");
        assert_eq!(texture(&audit, C1, 3).stored, "indexed565");
        assert_eq!(
            texture(&audit, C1, 7).reference_name.as_deref(),
            Some("twin-1")
        );
        assert_eq!(
            audit
                .textures()
                .filter(|row| row.verdict() == "match")
                .count(),
            8
        );
        // Every distinct shown word was expanded by rounding, never
        // inconsistently.
        assert!(audit.expansion.conflicts.is_empty());
        assert_eq!(audit.expansion.words, audit.expansion.lerp);
        assert!(audit.expansion.words >= 10);

        let report = fs::read_to_string(&out).expect("report written");
        assert_eq!(run.report.as_deref(), Some(report.as_str()));
        assert!(report.contains("\"passes\": true"));
        assert!(report.contains("\"unexplained\": 0"));
        assert!(report.contains("\"explained\": {\"alpha_source_unknown\": 1}"));

        // The contact sheet: the smoke texture's bottom-right texel is GREEN
        // (bit-replicated) with coverage 255, its top-left BLUE with 0.
        let archive = &audit
            .archives
            .iter()
            .find(|a| a.spelling == C1)
            .expect("c1");
        let sheet = archive.sheet.as_ref().expect("a sheet was written");
        let bytes = fs::read(&sheet.path).expect("sheet readable");
        assert_eq!(super::install::sha256(&bytes).to_hex(), sheet.sha256);
        assert_eq!(&bytes[..3], &[0, 0, 2], "uncompressed true-color TGA");
        assert_eq!(bytes[16], 32);
        assert_eq!(bytes[17], 0x28, "top-left origin, 8 alpha bits");
        let width = u32::from(u16::from_le_bytes([bytes[12], bytes[13]]));
        let (x0, y0, w, h) = texture(&audit, C1, 1).sheet_cell.expect("a cell");
        assert_eq!((w, h), (2, 3));
        let pixel = |x: u32, y: u32| {
            let at = 18 + ((y * width + x) * 4) as usize;
            [bytes[at + 2], bytes[at + 1], bytes[at], bytes[at + 3]]
        };
        assert_eq!(pixel(x0, y0), [0, 0, 255, 0]);
        assert_eq!(pixel(x0 + 1, y0 + 2), [0, 255, 0, 255]);
        let (fx, fy, _, _) = texture(&audit, C1, 2).sheet_cell.expect("a cell");
        assert_eq!(pixel(fx, fy)[3], 0, "the stored-value key is transparent");
        assert_eq!(pixel(fx + 1, fy)[3], 255);
    }

    /// Non-negotiable #5: a mirrored reference is caught and named, a
    /// transposed one differs in its dimensions — each an unexplained
    /// difference with a nonzero status.
    #[test]
    fn accept_f08_d_flipped_or_transposed_reference_is_an_unexplained_difference() {
        let one = world_one();
        for (change, code, orientation) in [
            ("vertical", "color", Orientation::VerticalFlip),
            ("horizontal", "color", Orientation::HorizontalFlip),
            ("transposed", "dimensions", Orientation::Unmatched),
        ] {
            let tree = Tree::new();
            let mut reference = references(&one);
            reference[0] = match change {
                "vertical" => reference[0].flipped_vertically(),
                "horizontal" => reference[0].flipped_horizontally(),
                _ => reference[0].transposed(),
            };
            tree.archive(C1, &one, &reference);
            let run = tree.run(&[]);
            assert_eq!(run.exit_code, 3, "{change}");
            let audit = run.audit.expect("the audit ran");
            let sky = texture(&audit, C1, 0);
            assert_eq!(sky.verdict(), "unexplained", "{change}");
            assert_eq!(codes(sky), vec![code], "{change}");
            assert_eq!(sky.orientation, orientation, "{change}");
            // The siblings are still compared and still match.
            assert_eq!(texture(&audit, C1, 3).verdict(), "match");
            assert_eq!(audit.unexplained(), 1, "{change}");
            assert!(
                run.diagnostics
                    .iter()
                    .any(|line| line.contains("texture.zbd#0:sky") && line.contains(code)),
                "{change}: {:?}",
                run.diagnostics
            );
        }
        // The vertical case names the first differing texel.
        let tree = Tree::new();
        let mut reference = references(&one);
        reference[0] = reference[0].flipped_vertically();
        tree.archive(C1, &one, &reference);
        let audit = tree.run(&[]).audit.expect("ran");
        let detail = &texture(&audit, C1, 0).differences[0].detail;
        assert!(
            detail.starts_with("6 of 6 texels differ; first at x=0 y=0: stored 0xf800"),
            "{detail}"
        );
    }

    /// AC02 alpha edge at the reference: one coverage value off by one, a
    /// plane the reference drops and a coverage the descriptor does not
    /// have are unexplained; so is a color whose 565 fields differ.
    #[test]
    fn accept_f08_d_alpha_edge_and_channel_mismatches_fail() {
        let one = world_one();
        type Change = Box<dyn Fn(&mut Vec<RefImage>)>;
        let cases: Vec<(&str, usize, Change, &str)> = vec![
            (
                "alpha edge",
                1,
                Box::new(|r: &mut Vec<RefImage>| r[1].at(1, 2)[3] = 254),
                "alpha",
            ),
            (
                "plane dropped",
                4,
                Box::new(|r: &mut Vec<RefImage>| {
                    let flag = one_tex(4);
                    let mut opaque = flag.clone();
                    opaque.flags = OPAQUE;
                    opaque.alpha.clear();
                    r[4] = opaque.reference("flag");
                }),
                "alpha_channel",
            ),
            (
                "coverage invented",
                0,
                Box::new(|r: &mut Vec<RefImage>| {
                    let mut sky = one_tex(0);
                    sky.flags = FULL;
                    sky.alpha = vec![255; 6];
                    r[0] = sky.reference("sky");
                }),
                "alpha_channel",
            ),
            (
                "one field",
                3,
                Box::new(|r: &mut Vec<RefImage>| r[3].at(2, 1)[1] ^= 0x04),
                "color",
            ),
        ];
        for (case, entry, change, code) in cases {
            let tree = Tree::new();
            let mut reference = references(&one);
            change(&mut reference);
            tree.archive(C1, &one, &reference);
            let run = tree.run(&[]);
            assert_eq!(run.exit_code, 3, "{case}");
            let audit = run.audit.expect("ran");
            assert_eq!(codes(texture(&audit, C1, entry)), vec![code], "{case}");
            assert_eq!(audit.unexplained(), 1, "{case}");
        }
    }

    fn one_tex(entry: usize) -> Tex {
        world_one()[entry].clone()
    }

    /// Every image position is accounted for: a missing, an extra or a
    /// renamed reference image, a missing or corrupt reference file and a
    /// word the reference expands two ways all fail beside the rest.
    #[test]
    fn accept_f08_d_missing_extra_renamed_or_inconsistent_references_fail() {
        let one = world_one();

        let tree = Tree::new();
        let mut reference = references(&one);
        reference.pop();
        tree.archive(C1, &one, &reference);
        let audit = tree.run(&[]).audit.expect("ran");
        assert_eq!(codes(texture(&audit, C1, 7)), vec!["reference_missing"]);
        assert_eq!(audit.unexplained(), 1);

        let tree = Tree::new();
        let mut reference = references(&one);
        reference.push(one[0].reference("stray"));
        tree.archive(C1, &one, &reference);
        let run = tree.run(&[]);
        assert_eq!(run.exit_code, 3);
        let audit = run.audit.expect("ran");
        assert_eq!(audit.archives[0].extra_reference_entries, vec!["stray"]);
        assert_eq!(audit.unexplained(), 1);

        let tree = Tree::new();
        let mut reference = references(&one);
        reference[6].name = "twin-1".to_owned();
        reference[3].name = "sign-1".to_owned();
        tree.archive(C1, &one, &reference);
        let audit = tree.run(&[]).audit.expect("ran");
        // `twin` is stored twice, so a renamed first entry is accepted as the
        // reference's convention; `sign` is stored once, so it is not.
        assert_eq!(texture(&audit, C1, 6).verdict(), "match");
        assert_eq!(codes(texture(&audit, C1, 3)), vec!["name"]);

        // No reference file for the second archive; a corrupt one for the
        // first. Both are failed rows; the audit still covers both.
        let tree = matching_tree();
        fs::remove_file(tree.reference().join(format!("{C2}.zip"))).expect("removed");
        let path = tree.reference().join(format!("{C1}.zip"));
        let mut bytes = fs::read(&path).expect("zip");
        let damaged = bytes.len() / 3;
        bytes[damaged] ^= 0xFF;
        fs::write(&path, bytes).expect("damaged");
        let run = tree.run(&[]);
        assert_eq!(run.exit_code, 3);
        let audit = run.audit.expect("ran");
        assert_eq!(audit.archives.len(), 2);
        for row in &audit.archives {
            let failure = row.failure.as_ref().expect("a failed row");
            assert_eq!(failure.code, "reference_unreadable", "{}", row.spelling);
        }
        assert_eq!(audit.unexplained(), 2);

        // The same word expanded two ways: both truncate to the word, so
        // each texel matches, but the reference is not one expansion.
        let tree = Tree::new();
        let pair = vec![
            Tex::direct("a", OPAQUE, 1, 1, &[GREY]),
            Tex::direct("b", OPAQUE, 1, 1, &[GREY]),
        ];
        let mut reference = references(&pair);
        reference[1].at(0, 0)[0] ^= 0x01;
        tree.archive(C1, &pair, &reference);
        let run = tree.run(&[]);
        assert_eq!(run.exit_code, 3);
        let audit = run.audit.expect("ran");
        assert!(audit.textures().all(|row| row.verdict() == "match"));
        assert_eq!(audit.expansion.conflicts.len(), 1);
        assert!(audit.expansion.conflicts[0].contains("0x8410"));
    }

    /// The reference reader: CRC-32 check value, every PNG filter, and
    /// refusal of what it does not support.
    #[test]
    fn accept_f08_d_reference_reader_undoes_every_filter_and_refuses_the_rest() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let tree = Tree::new();
        let mut image = RefImage {
            name: "ramp".to_owned(),
            width: 7,
            height: 11,
            channels: 4,
            pixels: Vec::new(),
        };
        for at in 0..7 * 11 * 4u32 {
            image.pixels.push((at.wrapping_mul(97) ^ (at >> 3)) as u8);
        }
        let path = tree.root.join("ramp.zip");
        fs::write(&path, zip(std::slice::from_ref(&image))).expect("written");
        let read = read_reference(&path).expect("readable");
        assert_eq!(read.images.len(), 1);
        assert_eq!(read.images[0].name, "ramp");
        assert_eq!((read.images[0].width, read.images[0].height), (7, 11));
        assert_eq!(read.images[0].pixels, image.pixels);

        let mut palette_png = png(&image);
        palette_png[8 + 8 + 9] = 3; // IHDR color type: indexed
        assert!(read_png(&palette_png).unwrap_err().contains("color type 3"));
        assert!(read_png(b"GIF89a").is_err());
        let mut bytes = zip(std::slice::from_ref(&image));
        // One byte of the stored PNG (after the 30-byte local header and
        // the 8-byte name) no longer matches the CRC.
        bytes[30 + 8 + 20] ^= 1;
        fs::write(&path, &bytes).expect("written");
        assert!(read_reference(&path).unwrap_err().contains("CRC mismatch"));
        assert!(read_reference(&tree.root.join("absent.zip")).is_err());
    }

    #[test]
    fn accept_f08_d_cli_refuses_bad_input_and_a_missing_installation() {
        let tree = matching_tree();
        let install = tree.install().display().to_string();
        let reference = tree.reference().display().to_string();
        let run = |args: &[&str], env: Option<&str>| {
            let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
            texture_audit_command_result(&args, env.map(OsString::from))
        };
        assert_eq!(run(&["--reference", &reference], None).exit_code, 4);
        assert_eq!(run(&["--reference", &reference], Some("")).exit_code, 4);
        assert_eq!(
            run(&["--reference", &reference], Some(&install)).exit_code,
            0
        );
        assert_eq!(run(&["--cs-path", &install], None).exit_code, 2);
        assert_eq!(run(&["--bogus"], None).exit_code, 2);
        assert_eq!(run(&["--reference"], None).exit_code, 2);
        let missing = tree.root.join("nowhere").display().to_string();
        assert_eq!(
            run(&["--cs-path", &install, "--reference", &missing], None).exit_code,
            2
        );
        let inside_out = tree.install().join("audit.json").display().to_string();
        let inside_sheets = tree.install().join("sheets").display().to_string();
        for flag in [["--out", &inside_out], ["--sheet-dir", &inside_sheets]] {
            let result = run(
                &[
                    "--cs-path",
                    &install,
                    "--reference",
                    &reference,
                    flag[0],
                    flag[1],
                ],
                None,
            );
            assert_eq!(result.exit_code, 2, "{flag:?}");
        }
        assert!(!tree.install().join("audit.json").exists());
        assert!(!tree.install().join("sheets").exists());
    }

    // --- retail ---------------------------------------------------------------

    fn game_dir() -> PathBuf {
        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this test needs the original installation");
        let dir = PathBuf::from(dir);
        assert!(
            dir.is_dir(),
            "CS_GAME_DIR is not a directory: {}",
            dir.display()
        );
        dir
    }

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The private reference extraction: `CS_F08_D_REFERENCE`, or
    /// `private/f08d-reference` of this checkout. How to produce it is in
    /// `docs/findings/2026-09-28-f08-d-texture-decode-audit.md`.
    fn reference_dir() -> PathBuf {
        let dir = std::env::var_os("CS_F08_D_REFERENCE").map_or_else(
            || workspace_root().join("private/f08d-reference"),
            PathBuf::from,
        );
        assert!(
            dir.join("ZBD/rimage.zbd.zip").is_file(),
            "the pinned reference extraction is missing at {}: run `unzbd cs textures` \
             (mech3ax v0.6.0) over every texture archive as the F08-D findings describe",
            dir.display()
        );
        dir
    }

    /// The texture count in a package's header, read directly (word 3).
    fn header_count(path: &Path) -> usize {
        let bytes = fs::read(path).expect("archive readable");
        u32::from_le_bytes(bytes[12..16].try_into().expect("four bytes")) as usize
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f08_d_retail_every_texture_matches_the_pinned_reference() {
        let root = game_dir();
        let reference = reference_dir();
        let args: Vec<String> = [
            "--cs-path",
            root.to_str().expect("UTF-8"),
            "--reference",
            reference.to_str().expect("UTF-8"),
        ]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
        let run = texture_audit_command_result(&args, None);
        assert!(run.diagnostics.is_empty(), "{:#?}", run.diagnostics);
        assert_eq!(run.exit_code, 0);
        let audit = run.audit.expect("the audit ran");

        // The F08-B census: 49 archives, 37,004 textures.
        assert_eq!(audit.archives.len(), 49);
        assert_eq!(audit.textures().count(), 37_004);
        assert_eq!(audit.unexplained(), 0);
        // Independently of the reader: each archive row has as many textures
        // as its header declares, and every one of them has a reference image.
        for archive in &audit.archives {
            assert_eq!(
                archive.textures.len(),
                header_count(&root.join(&archive.spelling)),
                "{}",
                archive.spelling
            );
            assert!(archive.failure.is_none() && archive.extra_reference_entries.is_empty());
            assert!(archive.reference.is_some());
        }
        for row in audit.textures() {
            assert_eq!(row.orientation, Orientation::AsStored, "{}", row.name);
            assert_eq!(row.extent, row.reference_extent, "{}", row.name);
            // The only explained difference is the unknown coverage source,
            // and exactly the textures whose coverage is unknown have it.
            let explained = row.verdict() == "explained";
            assert_eq!(explained, row.alpha == "unknown", "{}", row.name);
            if explained {
                assert_eq!(codes(row), vec!["alpha_source_unknown"]);
            }
        }
        assert_eq!(
            audit
                .textures()
                .filter(|row| row.alpha == "unknown")
                .count(),
            22
        );
        assert!(audit.expansion.conflicts.is_empty());
        assert_eq!(audit.expansion.words, audit.expansion.lerp);
        eprintln!(
            "F08-D retail: {} archives, {} textures, 0 unexplained; {} distinct 565 words, \
             {} expanded by rounding, {} also by bit replication",
            audit.archives.len(),
            audit.textures().count(),
            audit.expansion.words,
            audit.expansion.lerp,
            audit.expansion.replicate
        );
    }

    // --- evidence harness -----------------------------------------------------

    fn env_var(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!("{name} is not set: run the sequence in the evidence harness doc comment")
        })
    }

    fn command_output(program: &str, args: &[&str]) -> String {
        let output = Command::new(program)
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("{program} runs: {error}"));
        assert!(output.status.success(), "{program} {args:?} failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn locked_version(package: &str) -> String {
        let lock = fs::read_to_string(workspace_root().join("Cargo.lock")).expect("Cargo.lock");
        let mut wanted = false;
        for line in lock.lines().map(str::trim) {
            if line == "[[package]]" {
                wanted = false;
            } else if let Some(name) = line.strip_prefix("name = \"") {
                wanted = name.trim_end_matches('"') == package;
            } else if let Some(version) = line.strip_prefix("version = \"")
                && wanted
            {
                return version.trim_end_matches('"').to_owned();
            }
        }
        panic!("package {package:?} is not in Cargo.lock");
    }

    fn short_name(name: &str) -> &str {
        name.rsplit("::").next().unwrap_or(name)
    }

    /// Libtest totals plus `(test, "pass" | "fail")` for tests starting with
    /// `prefix`, from a recorded `cargo test` output.
    fn parse_suite(log: &str, prefix: &str) -> ([u64; 3], Vec<(String, &'static str)>) {
        let mut totals = [0u64; 3];
        let mut results: Vec<(String, &'static str)> = Vec::new();
        let mut pending: VecDeque<String> = VecDeque::new();
        let record = |results: &mut Vec<(String, &'static str)>, name: String, status| {
            if !results.iter().any(|(seen, _)| *seen == name) {
                results.push((name, status));
            }
        };
        for line in log.lines() {
            let trimmed = line.trim_start();
            if let Some(summary) = trimmed.strip_prefix("test result:") {
                for segment in summary.split(';') {
                    let words: Vec<&str> = segment.split_whitespace().collect();
                    if let Some(pair) = words.windows(2).find(|p| p[0].parse::<u64>().is_ok()) {
                        let count: u64 = pair[0].parse().expect("checked");
                        match pair[1] {
                            "passed" => totals[0] += count,
                            "failed" => totals[1] += count,
                            "ignored" => totals[2] += count,
                            _ => {}
                        }
                    }
                }
                continue;
            }
            if !pending.is_empty() && (trimmed == "ok" || trimmed == "FAILED") {
                let name = pending.pop_front().expect("pending");
                record(
                    &mut results,
                    name,
                    if trimmed == "ok" { "pass" } else { "fail" },
                );
                continue;
            }
            let mut cursor = trimmed;
            while let Some(position) = cursor.find("test ") {
                let after = &cursor[position + 5..];
                let Some(separator) = after.find(" ... ") else {
                    break;
                };
                let name = after[..separator].to_owned();
                let tail = &after[separator + 5..];
                cursor = tail;
                if !short_name(&name).starts_with(prefix) {
                    continue;
                }
                match tail.split_whitespace().next() {
                    Some("ok") => record(&mut results, name, "pass"),
                    Some("FAILED") => record(&mut results, name, "fail"),
                    _ => pending.push_back(name),
                }
            }
        }
        (totals, results)
    }

    fn iso_utc_now() -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64;
        // Howard Hinnant's `civil_from_days`.
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rest / 3_600,
            (rest % 3_600) / 60,
            rest % 60
        )
    }

    /// Evidence-report harness for F08-D (`docs/contracts/CLI-EVIDENCE.md`,
    /// schema `schemas/evidence.schema.json`). Not an acceptance test: it
    /// fails loudly when its inputs are missing. From the workspace root,
    /// after producing the reference extraction as the F08-D findings
    /// describe:
    ///
    /// 1. ```sh
    ///    mkdir -p private/evidence/F08-D
    ///    cargo test --workspace --locked -- accept_f08_d_ --include-ignored \
    ///      2>&1 | tee private/evidence/F08-D/cargo-test.log
    ///    ```
    ///    (record the exit status of `cargo test`, e.g. `${pipestatus[1]}` in zsh.)
    /// 2. ```sh
    ///    CS_EVIDENCE_DIR=private/evidence/F08-D \
    ///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
    ///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f08_d_ --include-ignored" \
    ///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
    ///    CS_F08_D_UNZBD=private/src/mech3ax/target/release/unzbd \
    ///      cargo test --locked -p cs_inspect --lib -- evidence_report_f08_d --ignored
    ///    ```
    ///    This runs the production `texture-audit` command over `$CS_GAME_DIR`
    ///    against the reference, keeps its report as the artifact
    ///    `texture-audit.json` and writes the contact sheets to `sheets/`
    ///    (their digests are in the report).
    /// 3. ```sh
    ///    python3 tools/validate_evidence.py private/evidence/F08-D/acceptance.json \
    ///      --artifact-root private/evidence/F08-D --require-pass
    ///    ```
    /// 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/F08-D.json`.
    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_F08_D_UNZBD, CS_GAME_DIR"]
    fn evidence_report_f08_d_writes_the_acceptance_report() {
        let absolute = |described: PathBuf| {
            if described.is_absolute() {
                described
            } else {
                workspace_root().join(described)
            }
        };
        let evidence_dir = absolute(PathBuf::from(env_var("CS_EVIDENCE_DIR")));
        let candidate_tree = env_var("CS_CANDIDATE_TREE");
        let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert!(
            !argv.is_empty(),
            "CS_EVIDENCE_ARGV must hold the acceptance command"
        );
        let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
        let unzbd = absolute(PathBuf::from(env_var("CS_F08_D_UNZBD")));
        let unzbd_sha256 =
            super::install::sha256(&fs::read(&unzbd).expect("unzbd is readable")).to_hex();
        let root = game_dir();
        let reference = reference_dir();
        assert_eq!(
            candidate_tree,
            command_output("git", &["rev-parse", "HEAD^{tree}"]),
            "CS_CANDIDATE_TREE must be the tree of the tested commit"
        );

        let log_path = evidence_dir.join("cargo-test.log");
        let log = fs::read_to_string(&log_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", log_path.display()));
        let ([passed, failed, ignored], results) = parse_suite(&log, "accept_f08_d_");
        assert!(
            passed > 0 && !results.is_empty(),
            "no accept_f08_d_ tests in the log"
        );
        let retail = "accept_f08_d_retail_every_texture_matches_the_pinned_reference";
        let status = results
            .iter()
            .find(|(name, _)| short_name(name) == retail)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| panic!("{retail} did not run: use --include-ignored"));
        assert_eq!(status, "pass", "{retail} must pass");

        // The production command over the installation, kept as the artifact.
        let audit_path = evidence_dir.join("texture-audit.json");
        let sheets = evidence_dir.join("sheets");
        let args: Vec<String> = [
            "--cs-path",
            root.to_str().expect("UTF-8"),
            "--reference",
            reference.to_str().expect("UTF-8"),
            "--sheet-dir",
            sheets.to_str().expect("UTF-8"),
            "--out",
            audit_path.to_str().expect("UTF-8"),
        ]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
        let audited = texture_audit_command_result(&args, None);
        assert_eq!(audited.out.as_deref(), Some(audit_path.as_path()));
        let audit = audited.audit.expect("the audit ran");
        let found = super::install::discover(&root).expect("discovery");
        let install_sha256 = super::install::fingerprint(&found.manifest).to_hex();
        let content_sha256 = super::install::content_fingerprint(&found.manifest).to_hex();

        let artifact = |path: &Path, kind: &str| {
            let bytes = fs::read(path).expect("artifact is readable");
            format!(
                "{{\"path\": {}, \"sha256\": \"{}\", \"kind\": \"{kind}\"}}",
                jstr(&path.file_name().expect("name").to_string_lossy()),
                super::install::sha256(&bytes).to_hex()
            )
        };
        let explained = audit
            .textures()
            .filter(|row| row.verdict() == "explained")
            .count();
        let method = format!(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR, the \
             production `cs-inspect texture-audit` run (texture-audit.json) that decodes every \
             texture of every ZBD texture archive through TextureCatalog::prepare_upload and \
             compares dimensions, coverage, orientation and every texel with the pinned \
             reference extraction (unzbd cs textures, mech3ax v0.6.0 commit \
             d3521a9721be731d365504568ddcd78e3f9846bb, binary sha256 {unzbd_sha256}; each \
             reference ZIP's sha256 is in the audit report), rustc and Cargo.lock; validated \
             with tools/validate_evidence.py --require-pass. {} archives, {} textures, {} \
             unexplained differences (exit {}); {explained} textures differ only in the \
             explained way (coverage source unknown, reference opaque); the reference expands \
             all {} distinct 565 words by rounding. The contact sheets (sheets/, digests in the \
             report) are private review renderings and were not reviewed by a human; the \
             reference is a tool, not the original renderer, so no claim beyond implemented is \
             made. Unknowns are recorded in \
             docs/findings/2026-09-28-f08-d-texture-decode-audit.md",
            audit.archives.len(),
            audit.textures().count(),
            audit.unexplained(),
            audited.exit_code,
            audit.expansion.words,
        );
        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F08-D\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"engine\": {{\"rust\": {}, \"bevy\": {}, \"avian\": {}}},\n\
             \x20\"created_at\": {},\n\
             \x20\"command\": {{\"argv\": [{}], \"cwd\": {}, \"exit_code\": {exit_code}}},\n\
             \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
             \x20\"seed\": 0,\n\
             \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
             \x20\"overrides\": [],\n\
             \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
             \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {passed}, \"failed\": {failed}, \"ignored\": {ignored}}},\n\
             \x20\"assertions\": [{}],\n\
             \x20\"artifacts\": [{}, {}],\n\
             \x20\"unknowns\": [],\n\
             \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
             \x20\"claim\": \"implemented\"\n\
             }}\n",
            jstr(&candidate_tree),
            jstr(&command_output("rustc", &["--version"])),
            jstr(&locked_version("bevy")),
            jstr(&locked_version("avian3d")),
            jstr(&iso_utc_now()),
            argv.iter()
                .map(|arg| jstr(arg))
                .collect::<Vec<_>>()
                .join(", "),
            jstr(&command_output("git", &["rev-parse", "--show-toplevel"])),
            jstr(&install_sha256),
            jstr(&content_sha256),
            passed + failed + ignored,
            passed + failed,
            results
                .iter()
                .map(|(name, status)| format!(
                    "{{\"id\": {}, \"status\": \"{status}\", \"evidence\": [\"cargo-test.log\"]}}",
                    jstr(short_name(name))
                ))
                .collect::<Vec<_>>()
                .join(", "),
            artifact(&log_path, "log"),
            artifact(&audit_path, "json"),
            jstr(
                "claude-1 (implementing agent; the reviewer regenerates it on the reviewed and rebased commit)"
            ),
            jstr(&method),
        );
        let out = evidence_dir.join("acceptance.json");
        fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
        assert!(
            failed == 0 && exit_code == 0 && audited.exit_code == 0,
            "the acceptance run or the audit failed (exit {exit_code}, {failed} failed, audit \
             exit {}): the report was written honestly and must not validate",
            audited.exit_code
        );
        println!("wrote {}", out.display());
    }
}
