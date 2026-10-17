//! One integration-test binary per crate: the layout convention and its guard
//! (task #1542).
//!
//! Every `crates/<crate>/tests/*.rs` file is normally its own Cargo test
//! binary, and each of them links the engine. On the owner's laptop macOS
//! XProtect scans every freshly linked executable on its first launch (~50-70 s
//! measured 2026-10-17), so `cargo test --workspace` takes hours although the
//! tests themselves run in seconds; CI has hit runner-disk and linker failures
//! from the same ~380 engine-linked binaries (`docs/findings/`, task #696).
//!
//! The fix is one integration-test binary per crate: the crate sets
//! `autotests = false` and declares exactly one `[[test]]` target, usually
//! `name = "<crate>_tests"` / `path = "tests/main.rs"`, and every former
//! `tests/<file>.rs` becomes a module of that one binary. Test prefixes keep
//! working because libtest matches filters by substring: `accept_f01_a_`
//! still selects `accept_f01_a_claim_records::accept_f01_a_...`.
//!
//! What can go wrong is silent: with `autotests = false` cargo discovers no
//! test file any more, so a `.rs` file under `tests/` that no `mod`
//! declaration reaches is never compiled and never run, and nothing says so.
//! [`check_workspace`] is the guard for exactly that: for every member that
//! sets `autotests = false` it resolves the `[[test]]` target, walks the
//! module tree from that file with rustc's own file rules (`mod name;` ->
//! `name.rs` or `name/mod.rs`, `#[path = "…"]` honoured), and reports every
//! `.rs` file under the crate's `tests/` directory the walk did not reach —
//! naming the stray file and the `main.rs` that should declare it.
//!
//! The guard is enforced by the ordinary `cargo test --workspace` run through
//! `accept_test_bin_guard_` in `tools/cs_xtask/tests/`, which runs it against
//! this workspace and against synthetic fixtures; `cs_xtask test-layout` is
//! the same check on the command line. Nothing here guesses: a crate that has
//! not migrated (no `autotests = false`) is left alone, and a member listed
//! through a glob is reported as unchecked rather than silently passed.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::bootstrap::{self, BootstrapError};
use crate::transient;

/// One workspace member that has switched to the single-binary layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigratedCrate {
    /// Workspace-relative member directory, e.g. `crates/cs_types`.
    pub member: String,
    /// Workspace-relative `[[test]]` source the modules hang off, e.g.
    /// `crates/cs_types/tests/main.rs`.
    pub main: String,
    /// Workspace-relative `.rs` files reached from `main` through `mod`
    /// declarations, `main` included. These are the files the one test binary
    /// really compiles.
    pub reachable: Vec<String>,
}

/// What one member's test layout looks like.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberLayout {
    /// Workspace-relative member directory, e.g. `crates/cs_types`.
    pub member: String,
    /// The member's layout, when it has migrated to one test binary.
    pub migrated: Option<MigratedCrate>,
    /// Everything wrong with this member's layout; empty is a pass.
    pub findings: Vec<LayoutFinding>,
}

/// One way a migrated crate's layout is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutFinding {
    /// The member sets `autotests = false` but does not declare exactly one
    /// `[[test]]` target, so there is no single binary for the convention to
    /// hang off. Only reported when test sources would silently stop running
    /// (or when more than one target makes the "one binary" claim false).
    TestTargetCount {
        /// Workspace-relative member directory.
        member: String,
        /// How many `[[test]]` targets the manifest declares.
        targets: usize,
        /// How many `.rs` files the member's `tests/` directory holds.
        sources: usize,
    },
    /// The single `[[test]]` target names a source file that is not there.
    MissingTestTarget {
        /// Workspace-relative member directory.
        member: String,
        /// Workspace-relative path the manifest declared.
        main: String,
    },
    /// A test source under the member's `tests/` directory is not reached from
    /// the single test binary, so cargo compiles and runs no test in it.
    StrayTestFile {
        /// Workspace-relative member directory.
        member: String,
        /// Workspace-relative path of the unreached file.
        file: String,
        /// Workspace-relative `[[test]]` source it should be reached from.
        main: String,
    },
}

impl fmt::Display for LayoutFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TestTargetCount {
                member,
                targets,
                sources,
            } => write!(
                f,
                "{member} sets `autotests = false` (one test binary per crate, task #1542) \
                 and its `tests/` holds {sources} `.rs` file(s), but its manifest declares \
                 {targets} `[[test]]` target(s); the convention is exactly one, e.g. \
                 `[[test]] name = \"<crate>_tests\" path = \"tests/main.rs\"`, and every \
                 test file becomes a `mod` of that one binary"
            ),
            Self::MissingTestTarget { member, main } => write!(
                f,
                "{member} declares a `[[test]]` target whose source {main} is not there, so \
                 the crate's single test binary cannot be built or checked"
            ),
            Self::StrayTestFile { member, file, main } => write!(
                f,
                "{file} is not reached from {main} by any `mod` declaration, and {member} sets \
                 `autotests = false`, so cargo compiles and runs no test in it; add it to \
                 {main} (a `tests/<name>/main.rs` target becomes `tests/<name>/mod.rs` under \
                 `mod <name>;`) or delete it"
            ),
        }
    }
}

/// The whole workspace's layout, and everything wrong with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutReport {
    /// How many `[workspace] members` entries were checked.
    pub members: usize,
    /// The members that have migrated to one test binary, in member order.
    pub migrated: Vec<MigratedCrate>,
    /// Members that are not a literal path (a glob such as `crates/*`) and
    /// therefore could not be checked. Reported, never silently passed.
    pub unchecked: Vec<String>,
    /// Every finding, in member order.
    pub findings: Vec<LayoutFinding>,
}

impl LayoutReport {
    /// Whether every checked member's layout is sound. An unchecked member
    /// (a glob) is not a finding here; it is reported through
    /// [`Self::unchecked`] and named in the command's own output, because the
    /// guard cannot know what the glob expands to.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// Number of test binaries the migrated crates ask cargo to link: one per
    /// migrated member.
    pub fn binaries(&self) -> usize {
        self.migrated.len()
    }

    /// One line per finding, for a caller that prints or asserts on them.
    pub fn lines(&self) -> Vec<String> {
        self.findings.iter().map(LayoutFinding::to_string).collect()
    }
}

/// Why the workspace's test layout cannot be read at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// A manifest or a `tests` directory could not be read.
    Io {
        /// What could not be read.
        path: String,
    },
    /// The workspace manifest has no usable `[workspace] members` list.
    Members(BootstrapError),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path } => write!(f, "cannot read {path}"),
            Self::Members(error) => write!(f, "workspace members check failed: {error}"),
        }
    }
}

impl std::error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Members(error) => Some(error),
            Self::Io { .. } => None,
        }
    }
}

/// Whether a manifest turns component auto-discovery off.
///
/// `autotests = false` is the one key that makes the single-`[[test]]`-target
/// convention necessary: with it, cargo links nothing under `tests/` unless a
/// `[[test]]` table asks for it, which is what makes an unreached file silent
/// rather than a second binary.
pub fn autotests_disabled(manifest: &str) -> bool {
    manifest
        .lines()
        .map(strip_comment)
        .map(str::trim)
        .any(|line| {
            line.strip_prefix("autotests")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .is_some_and(|value| value.trim() == "false")
        })
}

/// The `[[test]]` source paths a manifest declares, in declaration order.
///
/// A table without `path` resolves the way cargo does: `tests/<name>.rs`. A
/// table with neither key contributes nothing, and a missing file is caught by
/// [`check_member`] rather than guessed at here.
pub fn declared_test_paths(manifest: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut name: Option<String> = None;
    let mut path: Option<String> = None;
    let mut inside = false;

    let flush = |name: &mut Option<String>, path: &mut Option<String>, found: &mut Vec<String>| {
        if let Some(path) = path.take() {
            found.push(path);
        } else if let Some(name) = name.take() {
            found.push(format!("tests/{name}.rs"));
        }
        *name = None;
        *path = None;
    };

    for raw in manifest.lines() {
        let line = strip_comment(raw).trim();
        if line == "[[test]]" {
            if inside {
                flush(&mut name, &mut path, &mut found);
            }
            inside = true;
            continue;
        }
        if line.starts_with('[') {
            if inside {
                flush(&mut name, &mut path, &mut found);
            }
            inside = false;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => name = Some(unquote(value.trim()).to_string()),
            "path" => path = Some(unquote(value.trim()).to_string()),
            _ => {}
        }
    }
    if inside {
        flush(&mut name, &mut path, &mut found);
    }
    found
}

/// Drops a trailing TOML comment from a line.
fn strip_comment(line: &str) -> &str {
    line.split_once('#').map_or(line, |(head, _)| head)
}

/// Strips one layer of TOML string quoting.
fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim()
}

/// Every `.rs` file reachable from `main` through `mod` declarations, `main`
/// included, sorted.
///
/// The resolution rules are rustc's own: `mod name;` in a file looks for
/// `name.rs` beside it, then `name/mod.rs` below it, `#[path = "…"]`
/// redirects the lookup, and a file module declared inside an inline
/// `mod outer { … }` block resolves one directory deeper. A module file's own
/// declarations are followed relative to that file, and the walk is
/// cycle-safe. A module whose file is missing is dropped: rustc rejects the
/// crate anyway, so there is nothing here for the layout guard to report.
pub fn reachable_test_sources(main: &Path) -> Vec<PathBuf> {
    let root = normalize(main);
    let mut queue = VecDeque::from([main.to_path_buf()]);
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    while let Some(file) = queue.pop_front() {
        let key = normalize(&file);
        if seen.contains(&key) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let is_root = key == root;
        seen.insert(key);
        for module in declared_modules(&source, &file, is_root) {
            queue.push_back(module);
        }
    }
    seen.into_iter().collect()
}

/// The module files a Rust source declares with `mod`, resolved the way rustc
/// resolves them for the source file `file`.
///
/// `file` is the file `source` was read from and `crate_root` says whether it
/// is the target's root module, only so that the nested-module rule can be
/// applied: rustc treats a root module (the file the target names) and any
/// `mod.rs` as a *mod-rs* file, whose inline `mod outer { … }` blocks resolve
/// their own file modules one directory below the file, and every other file
/// as a *non-mod-rs* file, whose inline blocks resolve below `<dir>/<file
/// stem>`. Both are modelled here because a file module inside an inline block
/// would otherwise resolve to a path rustc never looks at, and the guard would
/// call a reachable file stray.
///
/// Only file modules count: an inline `mod name { … }` contributes no file of
/// its own. Comments and string literals are lexed away first, so a `mod`
/// mentioned in prose or inside a test fixture string is never mistaken for a
/// declaration.
pub fn declared_modules(source: &str, file: &Path, crate_root: bool) -> Vec<PathBuf> {
    let base = file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mod_rs = crate_root || file.file_name().is_some_and(|name| name == "mod.rs");
    let plain_scope = if mod_rs {
        base.clone()
    } else {
        stem_directory(&base, file)
    };

    // (brace depth of an inline module's body, the directory its nested file
    // modules resolve against)
    let mut inline: Vec<(usize, PathBuf)> = Vec::new();
    let mut scope = plain_scope.clone();
    let mut depth = 0usize;

    let tokens = lex(source);
    let mut found = Vec::new();
    let mut pending_path: Option<String> = None;
    let mut index = 0;

    while index < tokens.len() {
        match &tokens[index] {
            Token::Punct('#') => match read_attribute(&tokens, index) {
                Some((Attribute::Path(path), next)) => {
                    pending_path = Some(path);
                    index = next;
                }
                Some((Attribute::Other, next)) => index = next,
                None => index += 1,
            },
            Token::Punct('{') => {
                depth += 1;
                index += 1;
            }
            Token::Punct('}') => {
                depth = depth.saturating_sub(1);
                while inline.last().is_some_and(|(body, _)| *body > depth) {
                    inline.pop();
                }
                scope = inline
                    .last()
                    .map_or_else(|| plain_scope.clone(), |(_, dir)| dir.clone());
                index += 1;
            }
            Token::Ident(word) if word == "mod" => {
                let Some(Token::Ident(name)) = tokens.get(index + 1) else {
                    pending_path = None;
                    index += 1;
                    continue;
                };
                match tokens.get(index + 2) {
                    Some(Token::Punct(';')) => {
                        // rustc resolves `#[path]` on a file module against
                        // the directory of the file that declares it, and only
                        // inside an inline block against that block's directory.
                        found.push(match pending_path.take() {
                            Some(relative) if inline.is_empty() => base.join(relative),
                            Some(relative) => scope.join(relative),
                            None => resolve_module(&scope, name),
                        });
                        index += 3;
                    }
                    Some(Token::Punct('{')) => {
                        // An inline module: its own file modules hang one
                        // directory deeper.
                        pending_path = None;
                        depth += 1;
                        scope = scope.join(name);
                        inline.push((depth, scope.clone()));
                        index += 3;
                    }
                    _ => {
                        pending_path = None;
                        index += 2;
                    }
                }
            }
            _ => {
                pending_path = None;
                index += 1;
            }
        }
    }
    found
}

/// The directory a non-mod-rs file's inline modules resolve below:
/// `<dir>/<file stem>`.
fn stem_directory(dir: &Path, file: &Path) -> PathBuf {
    file.file_stem()
        .map_or_else(|| dir.to_path_buf(), |stem| dir.join(stem))
}

/// Where rustc looks for `mod name;` declared in a file inside `dir`.
fn resolve_module(dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(format!("{name}.rs"));
    if file.is_file() {
        return file;
    }
    let nested = dir.join(name).join("mod.rs");
    if nested.is_file() {
        return nested;
    }
    file
}

/// One token of the declaration lexer.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    /// An identifier such as `mod` or a module name.
    Ident(String),
    /// A string literal's contents, without the quotes.
    Str(String),
    /// Any other single character (`;`, `{`, `#`, `[`, `=`).
    Punct(char),
}

/// Lexes `source` into the tokens the declaration scan needs.
///
/// Comments (line and nested block) and the contents of string and character
/// literals are dropped, so only real syntax is seen; `path` attributes keep
/// their string value because [`Token::Str`] carries it. A `'` that opens a
/// lifetime (as in `&'a str`) is left for the identifier lexer, so a lifetime
/// cannot swallow the code after it.
fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let mut depth = 1usize;
                index += 2;
                while index < bytes.len() && depth > 0 {
                    if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                        depth += 1;
                        index += 2;
                    } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                        depth -= 1;
                        index += 2;
                    } else {
                        index += 1;
                    }
                }
            }
            b'"' => {
                let (value, next) = read_string_literal(source, index);
                tokens.push(Token::Str(value));
                index = next;
            }
            b'\'' => {
                index = skip_char_or_lifetime(bytes, index);
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                tokens.push(Token::Ident(source[start..index].to_string()));
            }
            byte if byte.is_ascii_digit() => {
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric()
                        || bytes[index] == b'_'
                        || bytes[index] == b'.')
                {
                    index += 1;
                }
            }
            byte if byte.is_ascii_whitespace() => {
                index += 1;
            }
            byte => {
                tokens.push(Token::Punct(byte as char));
                index += 1;
            }
        }
    }
    tokens
}

/// Reads the string literal starting at `start` (`source[start] == '"'`),
/// returning its contents and the index after the closing quote.
fn read_string_literal(source: &str, start: usize) -> (String, usize) {
    let bytes = source.as_bytes();
    let mut value = String::new();
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                if let Some(escaped) = source.get(index + 1..index + 2) {
                    value.push_str(escaped);
                }
                index += 2;
            }
            b'"' => {
                index += 1;
                break;
            }
            _ => {
                if let Some(character) = source.get(index..index + 1) {
                    value.push_str(character);
                }
                index += 1;
            }
        }
    }
    (value, index)
}

/// Steps past a character literal, or past just the quote of a lifetime.
fn skip_char_or_lifetime(bytes: &[u8], start: usize) -> usize {
    let end = match bytes.get(start + 1) {
        Some(b'\\') => {
            // `'\n'`, `'\''`: the escaped character is one code unit.
            let mut index = start + 3;
            while index < bytes.len() && bytes[index] != b'\'' {
                index += 1;
            }
            index + 1
        }
        Some(_) if bytes.get(start + 2) == Some(&b'\'') => start + 3,
        // `&'a str`, `'static`: a lifetime, not a literal. Skip the quote and
        // let the identifier lexer read the name.
        _ => start + 1,
    };
    end.min(bytes.len())
}

/// One attribute group, as far as the declaration scan needs it.
enum Attribute {
    /// A `#[path = "…"]` attribute, with the path it names.
    Path(String),
    /// Any other attribute.
    Other,
}

/// A `#[…]`/`#![…]` attribute at `index` (`tokens[index] == '#'`), with the
/// index just past its closing `]`, or `None` when the `#` opens no attribute.
fn read_attribute(tokens: &[Token], index: usize) -> Option<(Attribute, usize)> {
    let mut cursor = index + 1;
    if tokens.get(cursor) == Some(&Token::Punct('!')) {
        cursor += 1;
    }
    if tokens.get(cursor) != Some(&Token::Punct('[')) {
        return None;
    }
    let mut depth = 0usize;
    let mut end = None;
    let mut inner = Vec::new();
    for (offset, token) in tokens.iter().enumerate().skip(cursor) {
        match token {
            Token::Punct('[') => depth += 1,
            Token::Punct(']') => {
                depth -= 1;
                if depth == 0 {
                    end = Some(offset);
                    break;
                }
            }
            other => {
                if depth == 1 {
                    inner.push(other.clone());
                }
            }
        }
    }
    let end = end?;
    let attribute = match inner.as_slice() {
        [Token::Ident(key), Token::Punct('='), Token::Str(value)] if key == "path" => {
            Attribute::Path(value.clone())
        }
        _ => Attribute::Other,
    };
    Some((attribute, end + 1))
}

/// Every `.rs` file under `dir`, recursively, sorted.
///
/// Non-`.rs` files are not modules and are ignored; directories are followed
/// whether or not they are modules, so a former directory-style target
/// (`tests/campaign/main.rs`) is found and reported.
fn walk_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && path.extension() == Some("rs".as_ref()) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The canonical form of `path`, so two spellings of one file compare equal.
fn normalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `path` relative to `root`, with `/` separators, for reporting.
fn relative(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Checks one workspace member's test layout.
///
/// A member without `autotests = false` has not migrated and contributes no
/// [`MemberLayout::migrated`] and no findings: auto-discovered test files are
/// each their own binary, which is exactly what the convention replaces and
/// the guard must not mistake for a stray file.
pub fn check_member(workspace_root: &Path, member: &str) -> Result<MemberLayout, LayoutError> {
    let dir = workspace_root.join(member);
    let manifest_path = dir.join("Cargo.toml");
    let manifest = transient::read_to_string(&manifest_path, transient::PATIENT).map_err(|_| {
        LayoutError::Io {
            path: manifest_path.display().to_string(),
        }
    })?;

    let mut findings = Vec::new();
    if !autotests_disabled(&manifest) {
        return Ok(MemberLayout {
            member: member.to_string(),
            migrated: None,
            findings,
        });
    }

    let declared = declared_test_paths(&manifest);
    let tests_dir = dir.join("tests");
    let sources = if transient::is_dir(&tests_dir, transient::SCAN) {
        walk_rs_files(&tests_dir)
    } else {
        Vec::new()
    };

    // One target is the convention; no target silently drops every source
    // file, and more than one is not the one binary the convention promises.
    if declared.len() != 1 && (!sources.is_empty() || declared.len() > 1) {
        findings.push(LayoutFinding::TestTargetCount {
            member: member.to_string(),
            targets: declared.len(),
            sources: sources.len(),
        });
        return Ok(MemberLayout {
            member: member.to_string(),
            migrated: None,
            findings,
        });
    }
    let Some(main_rel) = declared.first() else {
        return Ok(MemberLayout {
            member: member.to_string(),
            migrated: None,
            findings,
        });
    };

    let main = dir.join(main_rel);
    let main_report = relative(workspace_root, &main);
    if !transient::is_file(&main, transient::SCAN) {
        findings.push(LayoutFinding::MissingTestTarget {
            member: member.to_string(),
            main: main_report,
        });
        return Ok(MemberLayout {
            member: member.to_string(),
            migrated: None,
            findings,
        });
    }

    let reachable = reachable_test_sources(&main);
    let reached: BTreeSet<PathBuf> = reachable.iter().map(|path| normalize(path)).collect();
    for file in &sources {
        if !reached.contains(&normalize(file)) {
            findings.push(LayoutFinding::StrayTestFile {
                member: member.to_string(),
                file: relative(workspace_root, file),
                main: main_report.clone(),
            });
        }
    }

    Ok(MemberLayout {
        member: member.to_string(),
        migrated: Some(MigratedCrate {
            member: member.to_string(),
            main: main_report,
            reachable: reachable
                .iter()
                .map(|path| relative(workspace_root, path))
                .collect(),
        }),
        findings,
    })
}

/// Checks every workspace member's test layout.
///
/// The members come from the workspace manifest's own `[workspace] members`
/// list ([`bootstrap::workspace_members`]). An entry that is not a literal
/// path (a glob) is reported through [`LayoutReport::unchecked`], because the
/// guard cannot know what it expands to.
pub fn check_workspace(workspace_root: &Path) -> Result<LayoutReport, LayoutError> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let manifest = transient::read_to_string(&manifest_path, transient::PATIENT).map_err(|_| {
        LayoutError::Io {
            path: manifest_path.display().to_string(),
        }
    })?;
    let members = bootstrap::workspace_members(&manifest).map_err(LayoutError::Members)?;

    let mut report = LayoutReport::default();
    for member in members {
        if member.contains('*') || member.contains('?') {
            report.unchecked.push(member);
            continue;
        }
        report.members += 1;
        let layout = check_member(workspace_root, &member)?;
        if let Some(migrated) = layout.migrated {
            report.migrated.push(migrated);
        }
        report.findings.extend(layout.findings);
    }
    Ok(report)
}
