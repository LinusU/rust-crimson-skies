//! Reads and spawns that survive a file being absent for a moment
//! (task #610).
//!
//! The reported failure was `cargo test --workspace` dying with
//! `error: test failed, to rerun pass 'cs_xtask --lib'` and
//! `No such file or directory (os error 2)` while a second cargo run
//! overlapped it on the same `CARGO_TARGET_DIR`. A writer that replaces a
//! file — cargo removing a test binary or dep-info file and recreating it
//! at the end of its work, an editor or a `git` checkout writing a manifest
//! through a rename pair — leaves a window in which the path names nothing,
//! and a reader inside that gap gets `NotFound` for a file that exists a
//! moment later. The gates run exactly while such a writer works: they are
//! a `cargo test` away from the build that is rewriting the same directory.
//!
//! Retrying [`ErrorKind::NotFound`] — and only that kind — turns the gap
//! back into the read the file was going to answer. [`PATIENT`] waits out a
//! writer that removes a file and recreates it at the end of a compile or
//! link; [`SCAN`] covers a rename pair inside a listing where absence is a
//! normal answer, so a genuinely missing entry is still skipped within the
//! walk instead of costing the whole budget. Every other error kind is
//! returned on first sight: a permission problem is reported, never slept
//! through. A file that stays absent keeps its `NotFound` — the retry buys
//! the writer's window, it never invents the file.

use std::fs::{self, Metadata, ReadDir};
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

/// How many times an operation is retried after a `NotFound`, and how long
/// it waits between tries.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Attempts after the first one.
    pub attempts: u32,
    /// Wait between attempts.
    pub interval: Duration,
}

/// Policy for a file the caller requires to exist: long enough to span a
/// writer that removes the file and recreates it when its work finishes —
/// the remove-then-relink gap the reported flake came through — and bounded
/// so a file that is really gone is still reported as gone, only later.
pub const PATIENT: Policy = Policy {
    attempts: 60,
    interval: Duration::from_millis(50),
};

/// Policy for an entry inside a listing, where absence is a normal answer —
/// the ancestor walk for a workspace manifest, the profile scan of a target
/// directory. Long enough for a rename pair to complete, short enough that
/// an ordinary missing entry stays a cheap skip rather than a delay on
/// every directory walked.
pub const SCAN: Policy = Policy {
    attempts: 5,
    interval: Duration::from_millis(20),
};

/// `op` retried while it fails with [`ErrorKind::NotFound`], up to
/// `policy.attempts` extra tries `policy.interval` apart. An error of any
/// other kind — and the last `NotFound` once the attempts run out — is
/// returned untouched.
fn retry<T>(policy: Policy, mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut remaining = policy.attempts;
    loop {
        match op() {
            Err(error) if error.kind() == ErrorKind::NotFound && remaining > 0 => {
                remaining -= 1;
                thread::sleep(policy.interval);
            }
            result => return result,
        }
    }
}

/// [`fs::read_to_string`] that retries a transient `NotFound` under
/// `policy`: [`PATIENT`] for a file that must exist, [`SCAN`] for one a
/// walk may legitimately not find.
pub fn read_to_string(path: &Path, policy: Policy) -> io::Result<String> {
    retry(policy, || fs::read_to_string(path))
}

/// [`fs::read`] under `policy`, same contract as [`read_to_string`].
pub fn read(path: &Path, policy: Policy) -> io::Result<Vec<u8>> {
    retry(policy, || fs::read(path))
}

/// [`fs::read_dir`] under `policy`, same contract as [`read_to_string`].
pub fn read_dir(path: &Path, policy: Policy) -> io::Result<ReadDir> {
    retry(policy, || fs::read_dir(path))
}

/// [`fs::metadata`] under `policy`, same contract as [`read_to_string`].
pub fn metadata(path: &Path, policy: Policy) -> io::Result<Metadata> {
    retry(policy, || fs::metadata(path))
}

/// [`Path::is_file`] under `policy`: `false` only when the path is still
/// missing — or still not a file — once the retries run out. [`PATIENT`]
/// for a file the caller requires to exist, [`SCAN`] for a probe whose
/// expected answer may legitimately be "absent", so a listing of removed
/// paths does not pay the full relink window on every entry.
pub fn is_file(path: &Path, policy: Policy) -> bool {
    metadata(path, policy).is_ok_and(|meta| meta.is_file())
}

/// [`Path::is_dir`] under `policy`, same contract as [`is_file`].
pub fn is_dir(path: &Path, policy: Policy) -> bool {
    metadata(path, policy).is_ok_and(|meta| meta.is_dir())
}

/// [`Command::output`] that retries a transient `NotFound` under
/// [`PATIENT`]: the binary being exec'd can sit in the same target
/// directory a concurrent cargo is relinking, so a spawn inside the
/// remove-then-link window is tried again once the linker finishes instead
/// of reporting `os error 2` for a binary that exists a moment later.
pub fn command_output(command: &mut Command) -> io::Result<Output> {
    retry(PATIENT, || command.output())
}
