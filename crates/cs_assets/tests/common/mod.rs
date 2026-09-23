//! Shared fixture helpers for the F02-B acceptance tests.
//!
//! Every tree built here is newly authored fixture data written under the
//! system temporary directory: it proves nothing about retail
//! installations, it never touches `$CS_GAME_DIR`, and it is removed again
//! when the test finishes (including on panic).
#![allow(dead_code)] // each test binary compiles this module and uses a subset

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Serial counter so parallel test binaries cannot collide on one name.
static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

/// A unique, disposable fixture directory.
pub struct TempTree {
    root: PathBuf,
}

impl TempTree {
    /// Creates a fresh empty tree with a collision-free name.
    pub fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let serial = NEXT_TREE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "cs-f02-b-{label}-{}-{nanos}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    /// The tree's host root, ready to be handed to discovery.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes `bytes` at `spelling` (a `/`-separated relative spelling),
    /// creating parent directories as needed.
    pub fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        let parent = path.parent().expect("fixture spellings have a parent");
        fs::create_dir_all(parent).expect("fixture directories are created");
        fs::write(&path, bytes).expect("fixture bytes are written");
    }

    /// Reads back one fixture file's bytes.
    pub fn read(&self, spelling: &str) -> Vec<u8> {
        fs::read(self.root.join(spelling)).expect("fixture bytes are readable")
    }

    /// Flips `xor_mask` into the byte at `index` while keeping the file's
    /// length: the one-byte edit of spec F02 AC02.
    pub fn edit_byte(&self, spelling: &str, index: usize, xor_mask: u8) {
        let mut bytes = self.read(spelling);
        assert!(index < bytes.len(), "the edited byte exists");
        bytes[index] ^= xor_mask;
        fs::write(self.root.join(spelling), bytes).expect("the edited bytes are written back");
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
