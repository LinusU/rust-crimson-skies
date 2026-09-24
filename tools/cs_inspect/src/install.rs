//! The synthetic installation-inventory fixture (F02-A) and the `inventory`
//! command's report wiring (F02-C).
//!
//! [`synthetic_install_fixture`] builds a small authored inventory through
//! the canonical [`InstallManifest`] constructor, so tests and the
//! `inventory` command have a real, validated input without touching the
//! owner's installation at `$CS_GAME_DIR`. Every fixture row — spellings,
//! sizes, digests, family labels — is newly authored fixture data, not
//! original content: the digests are not hashes of any original file, and
//! the fixture proves nothing about retail installations.
//!
//! [`inventory_command`] is the F02-C consumer of the F02-B production path:
//! it runs `cs_assets::install::discover` over the selected installation
//! (`--cs-path` wins over `CS_GAME_DIR`), renders the
//! [`inventory_report_json`] report — the full inventory, the discovery
//! diagnosis and the [`dependency_impact`] report — and writes it to `--out`
//! through an atomic rename (or to stdout). The dependency-impact report
//! lists every expected archive the observed layout depends on; an expected
//! archive that is missing stays a visible `available: false` row counted in
//! the unavailable total, never an omission (spec F02 AC03).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_assets::install::{self, Diagnosis, Discovery, DiscoveryError};
use cs_types::evidence::ContentHash;
use cs_types::install::{
    FileFamily, FileRole, InstallFileRecord, InstallManifest, ManifestError, ParseState,
    RelativePath,
};

/// Builds the minimal synthetic installation inventory for `host_root`.
///
/// The fixture deliberately covers every [`FileRole`] variant — including an
/// unclassified row with a failed parse that stays in the inventory — and a
/// mix of detected and undetected families, so consumers cannot pass by
/// omitting unknown or failed rows (IDENTITY-CONTENT: collections cannot
/// exclude failed entries).
///
/// The rows are identical for every root: calling this twice with
/// differently cased host roots yields manifests with different
/// [`InstallManifest::host_root`] values but one logical identity (F02
/// AC01).
pub fn synthetic_install_fixture(host_root: &Path) -> Result<InstallManifest, ManifestError> {
    let spelling =
        |text: &str| RelativePath::new(text).expect("synthetic fixture spelling is valid");
    let family = |label: &str| {
        Some(FileFamily::new(label).expect("synthetic fixture family label is valid"))
    };
    let files = vec![
        InstallFileRecord {
            relative_spelling: spelling("PLANES.ZBD"),
            size_bytes: 64,
            sha256: ContentHash::from_bytes([0x10; 32]),
            family: family("synthetic-zbd"),
            role: FileRole::Consumed,
            parse_state: ParseState::Parsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("Media/Tick.Wav"),
            size_bytes: 32,
            sha256: ContentHash::from_bytes([0x11; 32]),
            family: None,
            role: FileRole::OptionalMedia,
            parse_state: ParseState::Unparsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("System/Synthetic.Dll"),
            size_bytes: 16,
            sha256: ContentHash::from_bytes([0x12; 32]),
            family: None,
            role: FileRole::PlatformSupport,
            parse_state: ParseState::Unparsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("Unknown/Blob.Bin"),
            size_bytes: 8,
            sha256: ContentHash::from_bytes([0x13; 32]),
            family: None,
            role: FileRole::Unknown,
            parse_state: ParseState::Failed {
                diagnostic: "synthetic fixture row: no reader claims this file".to_owned(),
            },
        },
        InstallFileRecord {
            relative_spelling: spelling("Future/Asset.Dat"),
            size_bytes: 128,
            sha256: ContentHash::from_bytes([0x14; 32]),
            family: None,
            role: FileRole::NeededUnimplemented,
            parse_state: ParseState::Unparsed,
        },
        InstallFileRecord {
            relative_spelling: spelling("Stale/Leftover.Bak"),
            size_bytes: 4,
            sha256: ContentHash::from_bytes([0x15; 32]),
            family: None,
            role: FileRole::UnusedWithReason(
                "authored fixture leftover with no consumer".to_owned(),
            ),
            parse_state: ParseState::Unparsed,
        },
    ];
    InstallManifest::new(host_root.to_path_buf(), files)
}

// --- F02-C: inventory and dependency-impact reports ------------------------

/// Report schema label carried as `"report"` in the JSON output. The `/v1`
/// suffix versions the encoding: a future field change must bump it instead
/// of silently reinterpreting stored reports.
pub const INVENTORY_REPORT_VERSION: &str = "cs-inspect-inventory/v1";

/// The archives expected directly under the installation's `zbd` directory,
/// by logical file name: `planes.zbd` is named by spec F02 non-negotiable
/// behavior 2, and `interp.zbd` is the loading-script container observed in
/// the retail installation (the F07 interp family).
const EXPECTED_ZBD_ARCHIVES: [&str; 2] = ["interp.zbd", "planes.zbd"];

/// The archives every expected world group carries, by logical file name.
/// Observed in every world group of the retail installation (8/8: `c1`,
/// `c1b`, `c1c`, `c2`, `c2b`, `c3`, `c4`, `c5`); the `rtexture*.zbd` files
/// are deliberately absent from this list because their names vary between
/// groups.
const EXPECTED_GROUP_ARCHIVES: [&str; 4] = ["cam_anim.zbd", "gamez.zbd", "texture.zbd", "zrdr.zbd"];

/// The archives every mission directory carries, by logical file name.
/// Observed in every mission directory of the retail installation (53/53).
const EXPECTED_MISSION_ARCHIVES: [&str; 2] = ["mis_anim.zbd", "zrdr.zbd"];

/// Which expected slot one archive fills in the dependency-impact report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    /// An archive expected directly under `zbd` (`planes.zbd`, `interp.zbd`).
    Zbd,
    /// An archive every expected world group carries.
    Group,
    /// An archive every observed mission directory carries.
    Mission,
}

impl ArchiveKind {
    /// The report-vocabulary label of the kind.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Zbd => "zbd-archive",
            Self::Group => "group-archive",
            Self::Mission => "mission-archive",
        }
    }
}

/// One expected archive and its availability in the inventoried
/// installation.
///
/// An archive that is expected but absent stays a row with
/// `observed: None`: it is counted in [`DependencyImpact`]'s unavailable
/// total and its dependent scope lands in
/// [`DependencyImpact::impacted_dependents`] — a missing mission archive is
/// never omitted from the count (spec F02 AC03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedArchive {
    /// The case-folded logical key the archive is expected at
    /// (`zbd/c1/gamez.zbd`).
    pub logical_key: String,
    /// Which expected slot this archive fills.
    pub kind: ArchiveKind,
    /// The dependent scope: the logical key of the container whose content
    /// loses this archive when it is unavailable (`zbd`, `zbd/c1`,
    /// `zbd/c1/m02`).
    pub dependent: String,
    /// The preserved on-disk spelling when the archive was inventoried;
    /// `None` when it is missing.
    pub observed: Option<RelativePath>,
}

impl ExpectedArchive {
    /// Whether the archive was found in the inventory.
    pub fn available(&self) -> bool {
        self.observed.is_some()
    }
}

/// The dependency-impact report: every archive the observed installation
/// layout depends on, with its availability.
///
/// The expected set is derived from the spec's reference leads and the
/// observed layout conventions — it is never derived from the set of files
/// that happen to be present, so deleting an archive cannot shrink the
/// expected denominator.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DependencyImpact {
    /// Every expected archive, sorted by logical key.
    pub archives: Vec<ExpectedArchive>,
}

impl DependencyImpact {
    /// How many archives the expected set holds.
    pub fn expected_count(&self) -> usize {
        self.archives.len()
    }

    /// How many expected archives were inventoried.
    pub fn available_count(&self) -> usize {
        self.archives
            .iter()
            .filter(|archive| archive.available())
            .count()
    }

    /// How many expected archives are missing — counted, never omitted.
    pub fn unavailable_count(&self) -> usize {
        self.expected_count() - self.available_count()
    }

    /// The dependent scopes that lose at least one expected archive, sorted
    /// by logical key.
    pub fn impacted_dependents(&self) -> Vec<String> {
        self.archives
            .iter()
            .filter(|archive| !archive.available())
            .map(|archive| archive.dependent.clone())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect()
    }
}

/// Derives the dependency-impact report from a finished [`Discovery`]
/// (F02-C).
///
/// The expected set covers the `zbd`-level archives, every expected world
/// group — the observed groups plus the spec's
/// [`install::REFERENCE_WORLD_GROUP_LEADS`], so a group that is absent
/// entirely still reports its archives as unavailable — and every observed
/// mission directory under a world group. Availability is resolved
/// case-insensitively against the manifest while the preserved spelling
/// travels on the row.
pub fn dependency_impact(found: &Discovery) -> DependencyImpact {
    let diagnosis = &found.diagnosis;
    let present: BTreeMap<String, &RelativePath> = found
        .manifest
        .files
        .iter()
        .map(|row| (row.relative_spelling.logical_key(), &row.relative_spelling))
        .collect();

    // Expected groups: the observed world groups plus the reference leads.
    let mut groups: BTreeSet<String> = diagnosis
        .world_groups
        .iter()
        .filter_map(|group| group.logical_key().strip_prefix("zbd/").map(str::to_owned))
        .collect();
    groups.extend(
        install::REFERENCE_WORLD_GROUP_LEADS
            .iter()
            .map(|lead| (*lead).to_owned()),
    );

    // Mission directories: every observed directory that is exactly one
    // component below an expected world group (`zbd/<group>/<mission>`).
    // The set comes from the walk's real directory list, so a mission
    // directory carrying no regular file still expects its archives.
    let mut missions: Vec<String> = Vec::new();
    for directory in &diagnosis.directories {
        let key = directory.logical_key();
        let Some(rest) = key.strip_prefix("zbd/") else {
            continue;
        };
        let mut components = rest.split('/');
        let (Some(group), Some(mission), None) =
            (components.next(), components.next(), components.next())
        else {
            continue;
        };
        if groups.contains(group) {
            missions.push(format!("zbd/{group}/{mission}"));
        }
    }
    missions.sort();
    missions.dedup();

    let mut archives = Vec::new();
    let mut expect = |logical_key: String, kind: ArchiveKind, dependent: String| {
        archives.push(ExpectedArchive {
            observed: present
                .get(&logical_key)
                .map(|spelling| (*spelling).clone()),
            logical_key,
            kind,
            dependent,
        });
    };
    for name in EXPECTED_ZBD_ARCHIVES {
        expect(format!("zbd/{name}"), ArchiveKind::Zbd, "zbd".to_owned());
    }
    for group in &groups {
        for name in EXPECTED_GROUP_ARCHIVES {
            expect(
                format!("zbd/{group}/{name}"),
                ArchiveKind::Group,
                format!("zbd/{group}"),
            );
        }
    }
    for mission in &missions {
        for name in EXPECTED_MISSION_ARCHIVES {
            expect(
                format!("{mission}/{name}"),
                ArchiveKind::Mission,
                mission.clone(),
            );
        }
    }
    archives.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
    DependencyImpact { archives }
}

/// Why the `inventory` command failed.
#[derive(Debug)]
pub enum InventoryError {
    /// The command line was malformed: an unknown flag or a missing value.
    Usage(String),
    /// Neither `--cs-path` nor `CS_GAME_DIR` selected an installation: the
    /// `retail` capability is unavailable (CLI-EVIDENCE exit code 4).
    MissingInstallation,
    /// Production discovery refused the installation; the
    /// [`DiscoveryError`] names the host path it happened at.
    Discovery(DiscoveryError),
    /// The `--out` report could not be written or renamed into place.
    Output {
        /// The requested output path.
        path: PathBuf,
        /// Why the write or the rename failed.
        source: io::Error,
    },
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::MissingInstallation => write!(
                f,
                "no installation selected: pass --cs-path <dir> or set CS_GAME_DIR"
            ),
            Self::Discovery(error) => write!(f, "{error}"),
            Self::Output { path, source } => {
                write!(f, "cannot write report to {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for InventoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discovery(error) => Some(error),
            Self::Output { source, .. } => Some(source),
            Self::Usage(_) | Self::MissingInstallation => None,
        }
    }
}

/// Parsed `inventory` arguments.
struct InventoryArgs {
    /// The explicit `--cs-path`, which wins over `CS_GAME_DIR`.
    cs_path: Option<PathBuf>,
    /// The `--out` report path; `None` writes the report to stdout.
    out: Option<PathBuf>,
}

fn parse_inventory_args(args: &[String]) -> Result<InventoryArgs, InventoryError> {
    let mut parsed = InventoryArgs {
        cs_path: None,
        out: None,
    };
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        let flag = arg.as_str();
        if flag != "--cs-path" && flag != "--out" {
            return Err(InventoryError::Usage(format!(
                "cs-inspect inventory: unsupported argument {flag:?}; \
                 expected --cs-path <dir> and/or --out <file>"
            )));
        }
        let Some(value) = cursor.next() else {
            return Err(InventoryError::Usage(format!(
                "cs-inspect inventory: {flag} needs a value"
            )));
        };
        if flag == "--cs-path" {
            parsed.cs_path = Some(PathBuf::from(value));
        } else {
            parsed.out = Some(PathBuf::from(value));
        }
    }
    Ok(parsed)
}

/// Runs the `inventory` command: discovery in, the JSON report out (F02-C).
///
/// `--cs-path` wins over `CS_GAME_DIR` (spec F02 "Deliverable and
/// interfaces"). Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0`
/// when the report was produced, `2` for invalid input, `4` when no
/// installation is available and `1` for a discovery or output failure —
/// a failure is never returned as success. `--out` is written atomically
/// (a sibling temporary file renamed into place, removed on failure) and
/// its final path is reported on stderr; without `--out` the JSON report
/// goes to stdout.
pub fn inventory_command(args: &[String]) -> ExitCode {
    match inventory_command_result(args, std::env::var_os("CS_GAME_DIR")) {
        Ok(report_path) => {
            if let Some(path) = report_path {
                eprintln!("cs-inspect: wrote inventory report to {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err((code, error)) => {
            eprintln!("cs-inspect: {error}");
            code
        }
    }
}

/// The fallible body of [`inventory_command`], returning the `--out` path
/// that was written (`None` when the report went to stdout) or the exit
/// code and named error.
fn inventory_command_result(
    args: &[String],
    env_cs_path: Option<OsString>,
) -> Result<Option<PathBuf>, (ExitCode, InventoryError)> {
    let parsed = parse_inventory_args(args).map_err(|error| (ExitCode::from(2), error))?;
    let cs_path = parsed.cs_path.or_else(|| {
        env_cs_path
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let Some(cs_path) = cs_path else {
        return Err((ExitCode::from(4), InventoryError::MissingInstallation));
    };
    let found = install::discover(&cs_path).map_err(|error| (ExitCode::from(1), error.into()))?;
    let report = inventory_report_json(&found);
    match parsed.out {
        Some(out) => {
            write_report(&out, &report).map_err(|error| (ExitCode::from(1), error))?;
            Ok(Some(out))
        }
        None => {
            println!("{report}");
            Ok(None)
        }
    }
}

impl From<DiscoveryError> for InventoryError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

/// Writes `report` to `out` atomically: a sibling temporary file is written
/// first and renamed into place, so a half-written report can never be
/// mistaken for a finished one. The temporary file is removed again if the
/// write or the rename fails.
fn write_report(out: &Path, report: &str) -> Result<(), InventoryError> {
    let mut temp_name = out.as_os_str().to_owned();
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp_name);
    let write_result = fs::write(&temp, report).and_then(|()| fs::rename(&temp, out));
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(InventoryError::Output {
            path: out.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// Renders the full `inventory` report for one [`Discovery`] as JSON
/// (F02-C): fingerprints over the actual installation bytes, the discovery
/// diagnosis, the dependency-impact report and one row per inventoried
/// file — unknown and failed rows included (IDENTITY-CONTENT: collections
/// cannot exclude failed entries).
pub fn inventory_report_json(found: &Discovery) -> String {
    let diagnosis = &found.diagnosis;
    let impact = dependency_impact(found);

    let files: Vec<String> = found
        .manifest
        .files
        .iter()
        .map(|row| {
            format!(
                "{{\"spelling\": {}, \"logical_key\": {}, \"size_bytes\": {}, \"sha256\": {}, \
                 \"family\": {}, \"role\": {}, \"parse_state\": {}}}",
                jstr(row.relative_spelling.as_str()),
                jstr(&row.relative_spelling.logical_key()),
                row.size_bytes,
                jstr(&row.sha256.to_hex()),
                row.family
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |family| jstr(family.as_str())),
                role_json(&row.role),
                parse_state_json(&row.parse_state),
            )
        })
        .collect();

    let archives: Vec<String> = impact
        .archives
        .iter()
        .map(|archive| {
            format!(
                "{{\"expected\": {}, \"kind\": {:?}, \"dependent\": {}, \"available\": {}, \
                 \"observed\": {}}}",
                jstr(&archive.logical_key),
                archive.kind.label(),
                jstr(&archive.dependent),
                archive.available(),
                archive
                    .observed
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |spelling| jstr(spelling.as_str())),
            )
        })
        .collect();
    let impacted: Vec<String> = impact
        .impacted_dependents()
        .iter()
        .map(|dependent| jstr(dependent))
        .collect();

    format!(
        "{{\n\
         \x20\"report\": {},\n\
         \x20\"host_root\": {},\n\
         \x20\"fingerprints\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"counts\": {{\"files\": {}, \"directories\": {}, \"total_bytes\": {}, \
         \"skipped\": {}, \"cached_rows\": {}}},\n\
         \x20\"diagnosis\": {},\n\
         \x20\"dependency_impact\": {{\"summary\": {{\"expected\": {}, \"available\": {}, \
         \"unavailable\": {}, \"impacted_dependents\": {}}}, \"impacted_dependents\": [{}], \
         \"archives\": [{}]}},\n\
         \x20\"files\": [{}]\n\
         }}\n",
        jstr(INVENTORY_REPORT_VERSION),
        jstr(&found.manifest.host_root.to_string_lossy()),
        jstr(&install::fingerprint(&found.manifest).to_hex()),
        jstr(&install::content_fingerprint(&found.manifest).to_hex()),
        diagnosis.file_count,
        diagnosis.directory_count,
        diagnosis.total_bytes,
        diagnosis.skipped.len(),
        found.cached_rows,
        diagnosis_json(diagnosis),
        impact.expected_count(),
        impact.available_count(),
        impact.unavailable_count(),
        impacted.len(),
        impacted.join(", "),
        archives.join(", "),
        files.join(", "),
    )
}

/// The diagnosis section of the inventory report.
fn diagnosis_json(diagnosis: &Diagnosis) -> String {
    let groups: Vec<String> = diagnosis
        .world_groups
        .iter()
        .map(|group| jstr(group.as_str()))
        .collect();
    let absent: Vec<String> = diagnosis
        .absent_reference_groups
        .iter()
        .map(|lead| jstr(lead))
        .collect();
    let rofs: Vec<String> = diagnosis
        .rof_candidates
        .iter()
        .map(|candidate| jstr(candidate.as_str()))
        .collect();
    let skipped: Vec<String> = diagnosis
        .skipped
        .iter()
        .map(|entry| {
            format!(
                "{{\"path\": {}, \"reason\": {}}}",
                jstr(&entry.host_path.to_string_lossy()),
                jstr(entry.reason.label())
            )
        })
        .collect();
    format!(
        "{{\"zbd_dir\": {}, \"planes_zbd\": {}, \"world_groups\": [{}], \
         \"absent_reference_groups\": [{}], \"rof_candidates\": [{}], \"skipped\": [{}]}}",
        diagnosis
            .zbd_dir
            .as_ref()
            .map_or_else(|| "null".to_owned(), |zbd| jstr(zbd.as_str())),
        diagnosis
            .planes_zbd
            .as_ref()
            .map_or_else(|| "null".to_owned(), |planes| jstr(planes.as_str())),
        groups.join(", "),
        absent.join(", "),
        rofs.join(", "),
        skipped.join(", "),
    )
}

/// A [`FileRole`] as a JSON object: `{"kind": "..."}` plus `"reason"` on
/// the `unused` variant, whose reason is mandatory.
fn role_json(role: &FileRole) -> String {
    match role {
        FileRole::Consumed => "{\"kind\": \"consumed\"}".to_owned(),
        FileRole::NeededUnimplemented => "{\"kind\": \"needed-unimplemented\"}".to_owned(),
        FileRole::OptionalMedia => "{\"kind\": \"optional-media\"}".to_owned(),
        FileRole::UnusedWithReason(reason) => {
            format!("{{\"kind\": \"unused\", \"reason\": {}}}", jstr(reason))
        }
        FileRole::PlatformSupport => "{\"kind\": \"platform-support\"}".to_owned(),
        FileRole::Unknown => "{\"kind\": \"unknown\"}".to_owned(),
    }
}

/// A [`ParseState`] as a JSON object: `{"kind": "..."}` plus `"diagnostic"`
/// on the `failed` variant, whose diagnostic is mandatory.
fn parse_state_json(parse_state: &ParseState) -> String {
    match parse_state {
        ParseState::Unparsed => "{\"kind\": \"unparsed\"}".to_owned(),
        ParseState::Parsed => "{\"kind\": \"parsed\"}".to_owned(),
        ParseState::Failed { diagnostic } => {
            format!(
                "{{\"kind\": \"failed\", \"diagnostic\": {}}}",
                jstr(diagnostic)
            )
        }
    }
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
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
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
