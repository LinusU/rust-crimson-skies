//! Safe mod-root mounts and payload resolution (F53-B).
//!
//! Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
//! stage `### F53-B`; shared contract `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! A mod ships files under a host directory of its own. This module is the
//! **root join** F53-A deliberately left open: F53-A validated every
//! source spelling as a [`RelativePath`] with no root attached, and the join
//! of that spelling to a mod root happens here, against a root that is walked
//! once, indexed read-only, and never escaped.
//!
//! # What a [`ModRoot`] guarantees
//!
//! * The root is mounted with [`crate::vfs::source::mount_directory`], so
//!   the whole tree is indexed in sorted order while every file is hashed,
//!   symbolic links are never followed (they are reported in
//!   [`ModRoot::rejected`]), a host name that is not UTF-8 or that is not a
//!   regular file fails the mount by name, and two names that fold onto one
//!   logical key are refused instead of flattened. Nothing here writes, and
//!   no file handle outlives the call that opened it.
//! * The mount is [`PrecedenceClass::Mod`] and carries its [`ModId`] in its
//!   [`MountScope`], so it can only ever serve a
//!   [`ResolveContext`](cs_types::asset_id::ResolveContext) that has opted
//!   into exactly that mod (spec F04 non-negotiable behavior 2).
//! * [`ModRoot::resolve`] re-validates the spelling it is asked for before
//!   it looks anything up. A `..`, an absolute spelling, a drive prefix, a
//!   `.`/empty component or a NUL byte is refused with the spelling that was
//!   found — even though a manifest constructor already refused it, because
//!   this is the function that would otherwise *join* it to a root.
//! * A spelling that is safe but that the mod does not ship is
//!   [`ModMountError::SourceNotMounted`], never a guess: a declared source
//!   that a refused symlink used to satisfy is reported as missing rather
//!   than resolved through the link.
//! * [`ModRoot::read`] re-checks, at read time, that nothing under the root
//!   became a symbolic link and that the bytes still hash to what the mount
//!   indexed, so a file swapped after mounting is refused instead of
//!   returning the new bytes.
//!
//! # Designed, not original
//!
//! Whether the original game could mount mods at all, and under which
//! directory layout, is unmeasured (F53 "Research boundary"; F53-D). The
//! namespace, the mount ids and the refusal vocabulary here are newly
//! authored project design carrying designed provenance; no original file,
//! byte or behavior is reproduced.

use std::fmt;
use std::path::Path;

use cs_types::asset_id::{
    AssetKey, AssetVariant, LabelError, ModId, MountId, MountNamespace, PrecedenceClass,
};
use cs_types::evidence::ContentHash;
use cs_types::install::{RelativePath, RelativePathError};

use crate::vfs::mount::{MemberRecord, Mount, MountBuilder};
use crate::vfs::resolve::read_whole_member;
use crate::vfs::source::{
    MountedDirectory, ReadError, RejectedEntry, SourceError, mount_directory,
};

/// The mount namespace every mod root is mounted in.
///
/// A mod's payload is looked up in its own namespace, so a mod payload can
/// never be answered by an installation mount or the other way round: the
/// two key spaces are separate by construction.
pub const MOD_NAMESPACE: &str = "mod";

/// Why a mod root could not be mounted, or a declared source could not be
/// found in one.
#[derive(Debug)]
pub enum ModMountError {
    /// The root is missing, unreadable, not a directory, or the walk
    /// refused something it found below it.
    Root {
        /// The mod whose root failed.
        mod_id: ModId,
        /// Why the walk failed.
        source: SourceError,
    },
    /// The mod id cannot be used as a mount id, so the mount could not be
    /// named. (Both labels are validated by the same rule, so this is the
    /// defensive path rather than an expected one.)
    MountId {
        /// The mod whose id was refused.
        mod_id: ModId,
        /// Which label rule refused it.
        reason: LabelError,
    },
    /// The declared source is not a safe relative spelling: it tried to
    /// leave the root, name a root, or carry a NUL byte (F53 AC02's path
    /// half, re-checked at the join).
    UnsafeSource {
        /// The mod the spelling was asked of.
        mod_id: ModId,
        /// The spelling exactly as it was given.
        spelling: String,
        /// Which rule refused it.
        reason: RelativePathError,
    },
    /// The spelling is safe but this mod does not ship such a file: it was
    /// never indexed under the root (a symbolic link is not a member, so a
    /// declared source that only a link would satisfy lands here too).
    SourceNotMounted {
        /// The mod that does not hold it.
        mod_id: ModId,
        /// The spelling that was asked for.
        spelling: String,
    },
}

impl fmt::Display for ModMountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root { mod_id, source } => {
                write!(f, "cannot mount the root of mod {mod_id}: {source}")
            }
            Self::MountId { mod_id, reason } => {
                write!(f, "mod id {mod_id} cannot name a mount: {reason}")
            }
            Self::UnsafeSource {
                mod_id,
                spelling,
                reason,
            } => write!(
                f,
                "mod {mod_id} declares the unsafe source {spelling:?}: {reason}"
            ),
            Self::SourceNotMounted { mod_id, spelling } => {
                write!(f, "mod {mod_id} does not ship {spelling:?} below its root")
            }
        }
    }
}

impl std::error::Error for ModMountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Root { source, .. } => Some(source),
            Self::MountId { reason, .. } => Some(reason),
            Self::UnsafeSource { reason, .. } => Some(reason),
            Self::SourceNotMounted { .. } => None,
        }
    }
}

/// One mod's root directory, walked and indexed once.
///
/// The mount it holds is immutable: the members, their sizes and their
/// digests were fixed when the root was walked, and a read refuses bytes
/// that no longer match them.
#[derive(Debug)]
pub struct ModRoot {
    mod_id: ModId,
    namespace: MountNamespace,
    mounted: MountedDirectory,
}

impl ModRoot {
    /// Walks `root` and mounts it as this mod's payload mount.
    ///
    /// The mount id is the mod id (both labels are the same validated
    /// `[a-z0-9._-]` vocabulary) and the container label — the provenance
    /// every [`SourceSpan`](cs_types::asset_id::SourceSpan) of this mount
    /// records — is the mod id as well; the host path itself never enters a
    /// span.
    ///
    /// The walk is read-only and deterministic, never follows a symbolic
    /// link, and fails the whole mount on anything it cannot index rather
    /// than dropping it silently.
    ///
    /// # Errors
    ///
    /// [`ModMountError::Root`] when the root cannot be walked, and
    /// [`ModMountError::MountId`] when the mod id cannot name a mount.
    pub fn mount(mod_id: ModId, root: &Path) -> Result<Self, ModMountError> {
        let mount_id = MountId::new(mod_id.as_str()).map_err(|reason| ModMountError::MountId {
            mod_id: mod_id.clone(),
            reason,
        })?;
        let namespace = MountNamespace::new(MOD_NAMESPACE)
            .expect("the mod namespace constant is a valid label");
        let builder = MountBuilder::new(
            mount_id,
            namespace.clone(),
            PrecedenceClass::Mod,
            mod_id.as_str(),
        )
        .with_mod(mod_id.clone());
        let mounted = mount_directory(builder, root).map_err(|source| ModMountError::Root {
            mod_id: mod_id.clone(),
            source,
        })?;
        Ok(Self {
            mod_id,
            namespace,
            mounted,
        })
    }

    /// The mod this root belongs to.
    pub fn mod_id(&self) -> &ModId {
        &self.mod_id
    }

    /// The payload mount, ready to be handed to a content session.
    ///
    /// It is [`PrecedenceClass::Mod`] and scoped to [`Self::mod_id`], so a
    /// context that has not opted into the mod skips it
    /// (`SkipReason::ModNotOptedIn`).
    pub fn mount_record(&self) -> &Mount {
        &self.mounted.mount
    }

    /// The entries below the root that were observed and deliberately not
    /// mounted — symbolic links and non-regular files — in walk order.
    ///
    /// Refusal is visible, never silent: a declared source that only one of
    /// these would have satisfied is reported by [`Self::resolve`] as not
    /// shipped.
    pub fn rejected(&self) -> &[RejectedEntry] {
        &self.mounted.rejected
    }

    /// Finds the member a declared source names.
    ///
    /// The spelling is validated **before** it is matched, so a malicious
    /// relative path is refused by the same production call that would
    /// otherwise join it to the root (F53 AC02). The match itself is the
    /// VFS's logical one: separators and case fold the way every other key
    /// in this workspace folds, so a manifest written with `\` separators
    /// finds the file a Unix walk spelled with `/`.
    ///
    /// # Errors
    ///
    /// [`ModMountError::UnsafeSource`] when the spelling can escape or name
    /// a root; [`ModMountError::SourceNotMounted`] when it is safe but this
    /// mod does not ship it.
    pub fn resolve(&self, spelling: &str) -> Result<&MemberRecord, ModMountError> {
        let path = RelativePath::new(spelling).map_err(|reason| ModMountError::UnsafeSource {
            mod_id: self.mod_id.clone(),
            spelling: spelling.to_owned(),
            reason,
        })?;
        let key = AssetKey::new(self.namespace.clone(), path, AssetVariant::default());
        self.mounted
            .mount
            .member(&key)
            .ok_or_else(|| ModMountError::SourceNotMounted {
                mod_id: self.mod_id.clone(),
                spelling: spelling.to_owned(),
            })
    }

    /// Reads one member's bytes, whole.
    ///
    /// The length is checked against what the mount indexed, every
    /// component of the host path is re-checked not to be a symbolic link
    /// and the bytes are re-hashed against the digest recorded at mount
    /// time, so a file swapped after mounting is refused — as
    /// [`ReadError::DigestMismatch`] — rather than read. The re-hash is
    /// the whole-member read every other digest-checked read in this
    /// crate goes through ([`crate::vfs::resolve::read_whole_member`]),
    /// so a payload the compatibility signature attests to is the payload
    /// a reader actually gets.
    ///
    /// # Errors
    ///
    /// [`ReadError`] when the member cannot be read coherently.
    pub fn read(&self, member: &MemberRecord) -> Result<Vec<u8>, ReadError> {
        read_whole_member(&self.mounted.mount, member)
    }

    /// The digest the mount recorded for `member`, if it has one.
    ///
    /// A directory mount hashes every file it indexes, so this is the
    /// SHA-256 of the member's bytes as they were when the root was walked.
    #[must_use]
    pub fn digest(member: &MemberRecord) -> Option<ContentHash> {
        member.sha256()
    }
}

#[cfg(test)]
mod tests {
    //! F53-B unit tests for the root join: the malicious-spelling refusal
    //! and the symlink refusal. Every tree here is newly authored synthetic
    //! bytes below the system temporary directory; no original game data and
    //! no `CS_GAME_DIR` access.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f53-b-mod-root-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("temp dir is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs");
            fs::write(path, bytes).expect("bytes are written");
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mod_id(label: &str) -> ModId {
        ModId::new(label).expect("the fixture mod id is valid")
    }

    /// A mod root with one shipped file in it.
    fn rooted(label: &str) -> Temp {
        let temp = Temp::new(label);
        temp.write("art/panel.png", b"synthetic panel bytes");
        temp
    }

    /// **F53 AC02, path half.** A malicious relative spelling is refused by
    /// the production call that resolves a declared source — the call that
    /// owns the root join — and it is refused *before* any lookup, with the
    /// spelling and the rule that caught it. A resolution that merely
    /// failed to find such a spelling would let `..` walk out of the root
    /// the moment a caller joined it itself.
    #[test]
    fn accept_f53_b_a_malicious_relative_path_is_refused_by_the_root_join() {
        let temp = rooted("unsafe");
        let root = ModRoot::mount(mod_id("synthetic.repaint"), temp.path())
            .expect("the fixture root mounts");

        for spelling in [
            "../outside.png",
            "art/../../outside.png",
            "art/..\\..\\outside.png",
            "/etc/passwd",
            "\\\\server\\share\\payload.png",
            "C:\\windows\\system32\\payload.png",
            "art/./panel.png",
            "",
            "art/pan\u{0}el.png",
        ] {
            let error = root
                .resolve(spelling)
                .expect_err("a malicious spelling must be refused");
            assert!(
                matches!(error, ModMountError::UnsafeSource { .. }),
                "{spelling:?} must be refused as unsafe, got: {error}"
            );
            assert!(
                error.to_string().contains(&format!("{spelling:?}")),
                "the refusal names the spelling it found: {error}"
            );
        }

        // The safe spelling the mod actually ships still resolves, and it
        // resolves to the indexed member with its recorded digest.
        let member = root.resolve("art/panel.png").expect("the file ships");
        assert_eq!(member.size_bytes(), b"synthetic panel bytes".len() as u64);
        assert_eq!(
            ModRoot::digest(member),
            Some(cs_assets_digest(b"synthetic panel bytes")),
            "the digest is the SHA-256 of the bytes the walk hashed"
        );
        let bytes = root.read(member).expect("the member reads");
        assert_eq!(bytes, b"synthetic panel bytes");
    }

    /// A symbolic link below the root is never mounted, and a declared
    /// source that only the link would satisfy is reported as *not shipped*
    /// rather than followed out of the root.
    #[cfg(unix)]
    #[test]
    fn accept_f53_b_a_symbolic_link_is_never_mounted_or_followed() {
        let outside = Temp::new("outside");
        outside.write("secret.png", b"outside the mod root");

        let temp = rooted("symlink");
        std::os::unix::fs::symlink(
            outside.path().join("secret.png"),
            temp.0.join("art/evil.png"),
        )
        .expect("the link is created");

        let root = ModRoot::mount(mod_id("synthetic.repaint"), temp.path())
            .expect("the fixture root mounts");
        assert_eq!(
            root.rejected().len(),
            1,
            "the link is refused, and the refusal is visible"
        );
        let error = root
            .resolve("art/evil.png")
            .expect_err("a link is not a shipped member");
        assert!(
            matches!(error, ModMountError::SourceNotMounted { .. }),
            "the link is reported as not shipped, not followed: {error}"
        );

        // The link target is never readable through the mount, even if a
        // caller somehow held a member-shaped request for it.
        let inside = root.resolve("art/panel.png").expect("the real file ships");
        assert_eq!(inside.size_bytes(), b"synthetic panel bytes".len() as u64);
    }

    /// A file rewritten after the root was walked is refused: the read
    /// re-hashes against the digest the walk recorded, so a **same-length**
    /// swap — which passes every length, link and inode check — never
    /// reaches the caller, never reaches the bytes a program gate would
    /// validate, and never disagrees with the digest the compatibility
    /// signature covers.
    #[test]
    fn accept_f53_b_a_payload_swapped_after_the_walk_is_refused() {
        let temp = rooted("swap");
        let root = ModRoot::mount(mod_id("synthetic.repaint"), temp.path())
            .expect("the fixture root mounts");
        let member = root
            .resolve("art/panel.png")
            .expect("the shipped file resolves");
        assert_eq!(
            root.read(member).expect("the original bytes read"),
            b"synthetic panel bytes"
        );

        // Same length, one byte different: the length check, the symlink
        // check and the inode check all pass it, so only the re-hash can
        // tell it from what the mount indexed.
        assert_eq!(
            b"synthetic panel bytes".len(),
            b"synthetic panel byteS".len(),
            "the swap must hold the length the mount recorded"
        );
        fs::write(temp.path().join("art/panel.png"), b"synthetic panel byteS")
            .expect("the payload is rewritten in place");

        let error = root
            .read(member)
            .expect_err("a swapped payload is refused, not returned");
        assert!(
            matches!(error, ReadError::DigestMismatch { .. }),
            "the refusal names the digest that moved: {error}"
        );
    }

    /// The mount a [`ModRoot`] hands out is mod-precedence, mod-scoped and
    /// opt-in: a context that has not selected this mod skips it, and a
    /// context that has selects it above every shared source.
    #[test]
    fn accept_f53_b_the_payload_mount_is_mod_precedence_and_mod_scoped() {
        let temp = rooted("scope");
        let root = ModRoot::mount(mod_id("synthetic.repaint"), temp.path())
            .expect("the fixture root mounts");
        let mount = root.mount_record();

        assert_eq!(mount.precedence(), PrecedenceClass::Mod);
        assert_eq!(
            mount.scope().mod_id.as_ref(),
            Some(&mod_id("synthetic.repaint"))
        );
        assert_eq!(
            mount.namespace(),
            &MountNamespace::new(MOD_NAMESPACE).expect("valid")
        );
        assert!(!mount.is_retail(), "a mod mount is never original data");

        let opted_out = cs_types::asset_id::ResolveContext::new(ContentHash::from_bytes([9u8; 32]));
        assert!(
            !mount.scope().matches(&opted_out),
            "no mod selected: the payload mount is skipped"
        );
        assert_eq!(
            mount.scope().admit(&opted_out),
            Err(crate::vfs::SkipReason::ModNotOptedIn)
        );
        let opted_in = opted_out.with_mods(
            cs_types::asset_id::ModStack::new(vec![mod_id("synthetic.repaint")]).expect("one mod"),
        );
        assert!(
            mount.scope().matches(&opted_in),
            "the context that opted in is admitted"
        );
    }

    /// The digest helper this module's tests compare against is the
    /// production SHA-256, not a copy of it.
    fn cs_assets_digest(bytes: &[u8]) -> ContentHash {
        let mut hasher = crate::install::Sha256::new();
        hasher.update(bytes);
        hasher.finalize()
    }
}
