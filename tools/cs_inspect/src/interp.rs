//! The `interp` command (F07-B): decode and validate one INTERP
//! loading-script container, reported with its lossless tokens and the
//! findings the decoder retained.
//!
//! ```text
//! cs-inspect interp --file <path> [--out <file>] [--raw]
//! ```
//!
//! The file is read as-is and handed to `cs_formats::decode_interp`, the
//! validating decoder of `specs/F07-interp-loading-script-container.md`
//! (`### F07-B`). The JSON report names the container, its header, and for
//! every script its origin (index position, index-entry offset, script
//! offset, extent and terminator), its lines and each line's arguments as
//! `(offset, bytes)` pairs — the arguments stay bytes, because no encoding is
//! established for names or arguments, so a name or argument is rendered as
//! its length plus a lossless hex form rather than as guessed text.
//!
//! `--raw` adds the untouched F07-A records (`read_interp`: the name field
//! with its padding, the `argument_count` word and the argument bytes
//! verbatim) next to the decoded tokens, which is what a researcher needs to
//! see both views of the same line at once.
//!
//! This command is the F07-B *consumer*: it makes the decoder reachable
//! outside the parser crate and shows what a decoded container looks like.
//! It classifies nothing. Which tokens are loading commands, what they load
//! and how they are executed are F07-C (loading plan) and F07-D (retail
//! audit); nothing here resolves a world, executes a script or reports a
//! command as loaded.
//!
//! Exit codes follow `docs/contracts/CLI-EVIDENCE.md`: `0` the container
//! decoded and reported; `2` invalid input (no `--file`, an unreadable path,
//! a path that is not a file); `3` the bytes are not a valid INTERP
//! container, or they decoded with findings, which is a reported anomaly
//! rather than a clean pass; `1` a runtime failure. A decode failure is never
//! reported as success.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cs_formats::{DecodedInterp, InterpError, ParseContext, decode_interp, read_interp};

/// The report format version.
pub const INTERP_REPORT_VERSION: &str = "cs-inspect-interp/1";

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE).
const EXIT_INVALID_INPUT: u8 = 2;

/// Exit code for a failed validation or a reported anomaly.
const EXIT_FAILED_VALIDATION: u8 = 3;

/// Exit code for a runtime failure.
const EXIT_RUNTIME: u8 = 1;

/// Why the `interp` command could not produce a report.
#[derive(Debug)]
pub enum InterpCommandError {
    /// The command line was malformed.
    Usage(String),
    /// The selected path could not be read.
    Read {
        /// The requested path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The selected path is not a regular file.
    NotAFile(PathBuf),
    /// The report could not be written.
    Output {
        /// The requested path.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The bytes are not a valid INTERP container, or are not the version
    /// this stage documents.
    Container(InterpError),
}

impl fmt::Display for InterpCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::Read { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::NotAFile(path) => {
                write!(f, "{} is not a regular file", path.display())
            }
            Self::Output { path, source } => {
                write!(f, "cannot write report to {}: {source}", path.display())
            }
            Self::Container(error) => {
                // The code travels with the message so a caller can match the
                // diagnostic on the failure class rather than on wording.
                write!(f, "interp[{}]: {error}", error.code())
            }
        }
    }
}

impl std::error::Error for InterpCommandError {}

/// Parsed `interp` arguments.
#[derive(Debug, Default)]
struct InterpArgs {
    file: Option<PathBuf>,
    out: Option<PathBuf>,
    raw: bool,
}

fn parse_interp_args(args: &[String]) -> Result<InterpArgs, InterpCommandError> {
    let mut parsed = InterpArgs::default();
    let mut cursor = args.iter();
    while let Some(arg) = cursor.next() {
        match arg.as_str() {
            "--raw" => parsed.raw = true,
            "--file" | "--out" => {
                let Some(value) = cursor.next() else {
                    return Err(InterpCommandError::Usage(format!(
                        "cs-inspect interp: {arg} needs a value"
                    )));
                };
                if arg == "--file" {
                    parsed.file = Some(PathBuf::from(value));
                } else {
                    parsed.out = Some(PathBuf::from(value));
                }
            }
            other => {
                return Err(InterpCommandError::Usage(format!(
                    "cs-inspect interp: unsupported argument {other:?}; expected --file, \
                     --out or --raw"
                )));
            }
        }
    }
    Ok(parsed)
}

/// Runs the `interp` command and returns its exit code.
pub fn interp_command(args: &[String]) -> ExitCode {
    let run = interp_command_result(args);
    for line in &run.diagnostics {
        eprintln!("cs-inspect: {line}");
    }
    match (&run.report, &run.out) {
        (Some(_), Some(path)) => {
            eprintln!("cs-inspect: wrote interp report to {}", path.display());
        }
        (Some(report), None) => print!("{report}"),
        (None, _) => {}
    }
    ExitCode::from(run.exit_code)
}

/// Everything one `interp` run produced.
#[derive(Debug)]
pub struct InterpRun {
    /// The CLI-EVIDENCE exit code.
    pub exit_code: u8,
    /// The JSON report, when the container decoded.
    pub report: Option<String>,
    /// The `--out` path the report was written to, if any.
    pub out: Option<PathBuf>,
    /// Human diagnostics for stderr.
    pub diagnostics: Vec<String>,
}

impl InterpRun {
    fn failed(exit_code: u8, error: &InterpCommandError) -> Self {
        Self {
            exit_code,
            report: None,
            out: None,
            diagnostics: vec![error.to_string()],
        }
    }
}

/// The body of [`interp_command`].
///
/// The container label is the file name, so a diagnostic names the file the
/// bytes came from without leaking a path from the original installation into
/// the report.
pub fn interp_command_result(args: &[String]) -> InterpRun {
    let parsed = match parse_interp_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return InterpRun::failed(EXIT_INVALID_INPUT, &error),
    };
    let Some(path) = parsed.file else {
        return InterpRun::failed(
            EXIT_INVALID_INPUT,
            &InterpCommandError::Usage(
                "cs-inspect interp: --file <path> is required; it must be an INTERP \
                 container, for example one exported with `cs-inspect resolve \
                 --export-dir`"
                    .to_owned(),
            ),
        );
    };
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) => {
            return InterpRun::failed(
                EXIT_INVALID_INPUT,
                &InterpCommandError::Read {
                    path: path.clone(),
                    source,
                },
            );
        }
    };
    if !metadata.is_file() {
        return InterpRun::failed(
            EXIT_INVALID_INPUT,
            &InterpCommandError::NotAFile(path.clone()),
        );
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(source) => {
            return InterpRun::failed(
                EXIT_RUNTIME,
                &InterpCommandError::Read {
                    path: path.clone(),
                    source,
                },
            );
        }
    };
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let mut context = ParseContext::with_defaults(label.clone());
    let decoded = match decode_interp(&mut context, &bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            return InterpRun::failed(
                EXIT_FAILED_VALIDATION,
                &InterpCommandError::Container(error),
            );
        }
    };
    // `--raw` re-reads the same bytes through the F07-A reader. It is a
    // second view of one container, not a second source of truth: if the two
    // views ever disagree the report says so instead of preferring one.
    let raw = if parsed.raw {
        match read_interp(&mut ParseContext::with_defaults(label.clone()), &bytes) {
            Ok(raw) => Some(raw),
            Err(error) => {
                return InterpRun::failed(EXIT_RUNTIME, &InterpCommandError::Container(error));
            }
        }
    } else {
        None
    };

    let report = interp_report_json(&label, &decoded, raw.as_ref());
    // Findings are reported, not fatal on their own: a container that decodes
    // but leaves unclaimed bytes is an anomaly the owner has to see, and the
    // exit code says so instead of reporting a clean pass.
    let exit_code = if decoded.findings().is_empty() {
        0
    } else {
        EXIT_FAILED_VALIDATION
    };
    let mut diagnostics: Vec<String> = decoded
        .findings()
        .iter()
        .map(|finding| format!("{label}: {finding}"))
        .collect();
    let out = match &parsed.out {
        Some(out) => {
            if let Err(source) = write_report(out, &report) {
                let error = InterpCommandError::Output {
                    path: out.clone(),
                    source,
                };
                diagnostics.push(error.to_string());
                return InterpRun {
                    exit_code: EXIT_RUNTIME,
                    report: None,
                    out: None,
                    diagnostics,
                };
            }
            Some(out.clone())
        }
        None => None,
    };
    InterpRun {
        exit_code,
        report: Some(report),
        out,
        diagnostics,
    }
}

/// Writes `report` to `out` atomically via a sibling temporary file.
fn write_report(out: &Path, report: &str) -> io::Result<()> {
    let mut temp_name = out.as_os_str().to_owned();
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp_name);
    let result = fs::write(&temp, report).and_then(|()| fs::rename(&temp, out));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Renders the decode report.
///
/// Every byte string is a JSON object with its length and its lowercase hex,
/// never a decoded string: no encoding is established for names or arguments
/// (`specs/F07-*.md` non-negotiable #1), so a report that printed them as text
/// would be asserting an encoding this stage has not measured.
pub fn interp_report_json(
    label: &str,
    decoded: &DecodedInterp<'_>,
    raw: Option<&cs_formats::InterpFile<'_>>,
) -> String {
    let scripts: Vec<String> = decoded
        .scripts()
        .iter()
        .map(|script| {
            let lines: Vec<String> = script
                .lines()
                .iter()
                .enumerate()
                .map(|(line, decoded_line)| {
                    let tokens: Vec<String> = decoded_line
                        .tokens()
                        .iter()
                        .map(|token| {
                            format!(
                                "{{\"offset\": {}, \"bytes\": {}}}",
                                token.offset(),
                                jbytes(token.bytes())
                            )
                        })
                        .collect();
                    let mut row = format!(
                        "{{\"position\": {}, \"offset\": {}, \"data_offset\": {}, \"size\": {}, \
                         \"argument_count\": {}, \"token_count\": {}, \"tokens\": [{}]",
                        line,
                        decoded_line.offset(),
                        decoded_line.data_offset(),
                        decoded_line.size(),
                        decoded_line.argument_count(),
                        decoded_line.len(),
                        tokens.join(", ")
                    );
                    if let Some(head) = decoded_line.head() {
                        row.push_str(&format!(", \"head_offset\": {}", head.offset()));
                    }
                    row.push('}');
                    if raw.is_some() {
                        row.push_str(&format!(
                            ", \"raw\": {{\"offset\": {}, \"size\": {}, \"argument_count\": {}, \
                             \"data\": {}}}",
                            decoded_line.raw().offset,
                            decoded_line.raw().size,
                            decoded_line.raw().argument_count,
                            jbytes(decoded_line.raw().data),
                        ));
                    }
                    row.push('}');
                    row
                })
                .collect();
            format!(
                "{{\"index\": {}, \"entry_offset\": {}, \"script_offset\": {}, \"limit\": {}, \
                 \"terminator_offset\": {}, \"end\": {}, \"line_count\": {}, \"name\": {}, \
                 \"name_field\": {}, \"timestamp_metadata_only\": {}, \"lines\": [{}]}}",
                script.entry().index,
                script.entry().entry_offset,
                script.entry().script_offset,
                script.limit(),
                script.terminator_offset(),
                script.end(),
                script.len(),
                jbytes(script.name()),
                jbytes(script.entry().name_field),
                script.entry().raw_timestamp,
                lines.join(", "),
            )
        })
        .collect();
    let findings: Vec<String> = decoded.findings().iter().map(finding_json).collect();
    format!(
        "{{\n  \"report\": {},\n  \"container\": {},\n  \"container_bytes\": {},\n  \
         \"arguments_encoding\": \"unknown: bytes are reported as length and hex\",\n  \
         \"header\": {{\"signature\": {}, \"version\": {}, \"script_count\": {}}},\n  \
         \"index_end\": {},\n  \"script_count\": {},\n  \"findings\": [{}],\n  \"scripts\": [{}]\n}}\n",
        jstr(INTERP_REPORT_VERSION),
        jstr(label),
        decoded.container_len(),
        decoded.header().signature,
        decoded.header().version,
        decoded.header().script_count,
        decoded.index_end(),
        decoded.scripts().len(),
        findings.join(", "),
        scripts.join(",\n    "),
    )
}

/// One finding as a JSON object.
fn finding_json(finding: &cs_formats::InterpFinding) -> String {
    match finding {
        cs_formats::InterpFinding::Unclaimed { offset, length } => format!(
            "{{\"code\": \"unclaimed\", \"offset\": {}, \"length\": {}}}",
            offset, length
        ),
        cs_formats::InterpFinding::SharedScriptOffset { offset, entries } => {
            let entries: Vec<String> = entries.iter().map(usize::to_string).collect();
            format!(
                "{{\"code\": \"shared_script_offset\", \"offset\": {}, \"entries\": [{}]}}",
                offset,
                entries.join(", ")
            )
        }
        cs_formats::InterpFinding::UnterminatedName { index, offset } => format!(
            "{{\"code\": \"unterminated_name\", \"index\": {}, \"offset\": {}}}",
            index, offset
        ),
        cs_formats::InterpFinding::NamePadding {
            index,
            offset,
            length,
        } => format!(
            "{{\"code\": \"name_padding\", \"index\": {}, \"offset\": {}, \"length\": {}}}",
            index, offset, length
        ),
    }
}

/// A byte string as a JSON object: its length and its lowercase hex.
///
/// Deliberately not a text literal. Names and arguments have no established
/// encoding, so printing them as `str` would claim one; hex plus length is
/// lossless, comparable and says nothing the file does not say.
fn jbytes(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{{\"length\": {}, \"hex\": {}}}", bytes.len(), jstr(&hex))
}

/// A JSON string literal: quoted and escaped.
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

#[cfg(test)]
mod tests {
    //! F07-B acceptance tests for the `interp` command. Every container here
    //! is newly authored synthetic bytes written below the system temporary
    //! directory; no original game data, no `CS_GAME_DIR` access.

    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f07-b-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("temp dir is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(spelling);
            fs::write(&path, bytes).expect("bytes are written");
            path
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// Header, index and one script with one two-argument line.
    fn container(name: &[u8], data: &[u8], count: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [0x0897_1119u32, 7, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut field = [0u8; cs_formats::NAME_FIELD_BYTES];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&7u32.to_le_bytes()); // timestamp
        bytes.extend_from_slice(&140u32.to_le_bytes()); // script offset
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    /// The command wires the decoder end to end: a valid container decodes to
    /// exit 0 and a report whose script, extent, terminator and tokens match
    /// the bytes the test wrote.
    #[test]
    fn accept_f07_b_cli_decodes_a_container_and_reports_its_tokens() {
        let tree = Temp::new("valid");
        let path = tree.write("probe.interp", &container(b"probe", b"LOAD\0world\0", 2));
        let path = path.to_str().expect("temp paths are UTF-8");

        let run = interp_command_result(&args(&["--file", path]));
        assert_eq!(run.exit_code, 0, "diagnostics: {:?}", run.diagnostics);
        assert!(run.diagnostics.is_empty());
        let report = run.report.expect("the container decoded");

        // The header and the container fingerprint.
        assert!(report.contains("\"report\": \"cs-inspect-interp/1\""));
        assert!(report.contains("\"container_bytes\": 163"));
        assert!(report.contains("\"script_count\": 1"));
        assert!(report.contains("\"index_end\": 140"));
        // The script's origin: index position, entry offset, script offset and
        // the extent the decoder validated it against.
        assert!(report.contains("\"index\": 0"));
        assert!(report.contains("\"entry_offset\": 12"));
        assert!(report.contains("\"script_offset\": 140"));
        assert!(report.contains("\"limit\": 163"));
        assert!(report.contains("\"terminator_offset\": 159"));
        assert!(report.contains("\"end\": 163"));
        // The name as bytes, never as guessed text.
        assert!(report.contains("\"length\": 5, \"hex\": \"70726f6265\""));
        // The two arguments with their absolute offsets, and the head offset.
        assert!(
            report.contains("\"offset\": 148, \"bytes\": {\"length\": 4, \"hex\": \"4c4f4144\"}")
        );
        assert!(
            report.contains("\"offset\": 153, \"bytes\": {\"length\": 5, \"hex\": \"776f726c64\"}")
        );
        assert!(report.contains("\"head_offset\": 148"));
        assert!(report.contains("\"token_count\": 2"));
        // A clean container claims every byte.
        assert!(report.contains("\"findings\": []"));

        // `--raw` adds the F07-A view of the same line next to the tokens.
        let run = interp_command_result(&args(&["--file", path, "--raw"]));
        let report = run.report.expect("the container decoded");
        assert_eq!(run.exit_code, 0);
        assert!(report.contains("\"raw\": {\"offset\": 140, \"size\": 11, \"argument_count\": 2"));
    }

    /// A container whose argument count disagrees with the delimiters in its
    /// data is refused with the offset, and never reported as a clean pass.
    #[test]
    fn accept_f07_b_cli_refuses_a_container_that_fails_validation() {
        let tree = Temp::new("invalid");
        // Two delimiters, one declared argument.
        let path = tree.write("bad.interp", &container(b"bad", b"a\0b\0", 1));
        let path = path.to_str().expect("temp paths are UTF-8");

        let run = interp_command_result(&args(&["--file", path]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        assert!(run.report.is_none(), "a refused container reports nothing");
        let diagnostic = run.diagnostics.join("\n");
        assert!(diagnostic.contains("argument_count"), "{diagnostic}");
        assert!(diagnostic.contains("declares 1 arguments"), "{diagnostic}");
        assert!(diagnostic.contains("2 0x00 delimiters"), "{diagnostic}");

        // A missing --file and an unreadable path are invalid input, not a
        // runtime failure and not success.
        let run = interp_command_result(&args(&[]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.diagnostics[0].contains("--file"));

        let run = interp_command_result(&args(&["--file", "/nonexistent/nope.interp"]));
        assert_eq!(run.exit_code, EXIT_INVALID_INPUT);
        assert!(run.diagnostics[0].contains("cannot read"));

        // A container that is not INTERP at all is unsupported content.
        let path = tree.write("other.bin", b"not an interp container at all");
        let run = interp_command_result(&args(&["--file", path.to_str().expect("utf-8")]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        assert!(
            run.diagnostics[0].contains("signature"),
            "{:?}",
            run.diagnostics
        );
    }

    /// A container that decodes but leaves unclaimed bytes reports the regions
    /// and exits non-zero, so an anomaly is never read as a clean pass.
    #[test]
    fn accept_f07_b_cli_reports_unclaimed_regions_and_exits_nonzero() {
        let tree = Temp::new("gaps");
        let mut bytes = container(b"gapped", b"a\0", 1);
        let script_end = bytes.len() as u64;
        // Twelve bytes of tail after the script, which claims none of them.
        bytes.extend_from_slice(&[0xAB; 12]);
        let path = tree.write("gapped.interp", &bytes);
        let out = tree.0.join("report.json");

        let run = interp_command_result(&args(&[
            "--file",
            path.to_str().expect("utf-8"),
            "--out",
            out.to_str().expect("utf-8"),
        ]));
        assert_eq!(run.exit_code, EXIT_FAILED_VALIDATION);
        assert_eq!(run.out, Some(out.clone()));
        let report = fs::read_to_string(&out).expect("the report was written");
        assert!(
            report.contains(&format!(
                "\"code\": \"unclaimed\", \"offset\": {script_end}, \"length\": 12"
            )),
            "{report}"
        );
        // The anomaly is on stderr as well, for a human reading the run.
        assert!(
            run.diagnostics[0].contains("unclaimed"),
            "{:?}",
            run.diagnostics
        );
    }
}
