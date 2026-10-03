//! What one run actually loaded and ran on: the three digests a replay
//! record's [`BuildFingerprint`] is built from.
//!
//! Spec: `specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
//! stage `### F59-B`. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! F59-A defined [`BuildFingerprint`] with three *separate* digests — engine,
//! content and rules — precisely so a refusal can say which one moved. That is
//! only useful if each is measured from something the run really had, so this
//! module is where they are computed:
//!
//! * [`LoadedContent`] is the content the run loaded, as the `(logical key,
//!   content digest)` rows a digest is computed over.
//!   [`LoadedContent::from_manifest`] takes exactly the rows F02's
//!   `cs_assets::install::content_fingerprint` digests, so
//!   [`digest`](LoadedContent::digest) is byte-for-byte that function's answer
//!   for an inventoried installation — one content digest, two producers, no
//!   second definition and no separate domain prefix. For a run that loads
//!   declared records rather than files ([`airframe_content_digest`]), the
//!   row's digest is a field-by-field digest of the record it loaded.
//! * [`rules_digest`] covers what changes how the *same* content is evaluated:
//!   the flight law, the handling profile and the fixed rate. Keeping it out of
//!   the content digest is what makes "a rules table changed" reportable as
//!   [`CompatibilityDifference::Rules`] rather than as a content change.
//! * [`engine_digest`] names the engine build and the state format version the
//!   digests use. Source changes move [`BuildId`] (the candidate tree), not this.
//!
//! **No constant stands in for a digest.** A content digest here is the digest
//! of the rows a run loaded: change one loaded value and the digest moves,
//! which is exactly what AC02 refuses on. `BuildId` and the toolchain string
//! are supplied by the caller because they name the checkout the binary was
//! built from (`CS_CANDIDATE_TREE`, the toolchain); the command that reads them
//! is F59-C's wiring.

use std::collections::BTreeMap;

use cs_assets::install::Sha256;
use cs_content::replay::{BuildFingerprint, BuildId, PlatformTag, ReplayError};
use cs_sim::flight::AirframeTuning;
use cs_types::evidence::ContentHash;
use cs_types::install::InstallManifest;

/// The logical content key of the airframe tuning one flight run flies.
///
/// A key, not a path: for a retail run the same tuning arrives as an inventoried
/// catalog file, and for a run that flies declared records this key names the
/// record. What the digest covers is the tuning's values either way.
pub const AIRFRAME_CONTENT_KEY: &str = "flight/airframe-tuning";

/// The version of the canonical per-tick state reading this module digests.
///
/// It is part of [`engine_digest`] on purpose: a build that measures state in a
/// different shape cannot compare its envelopes with one that does not, and the
/// version is what makes that visible instead of silent.
pub const STATE_FORMAT_VERSION: &str = "cs.f59.state-reading.v1";

const AIRFRAME_DOMAIN: &[u8] = b"cs.f59.content.airframe.v1";
const RULES_DOMAIN: &[u8] = b"cs.f59.rules.flight.v1";
const ENGINE_DOMAIN: &[u8] = b"cs.f59.engine.identity.v1";

/// The canonical content one run loaded, as the rows its digest is computed
/// over.
///
/// A row is a stable logical key and the SHA-256 of the bytes behind it. The
/// set's [`digest`](Self::digest) hashes the row digests in logical-key order —
/// the same construction F02's `content_fingerprint` uses for an installation —
/// so a run that loaded files and a run that loaded declared records are
/// digested by one rule, and neither needs a second notion of "the content".
///
/// Two properties follow from that construction and are relied on rather than
/// papered over: a byte change in any loaded row moves the digest (which is
/// what makes AC02's refusal fire), and renaming a row's key without changing
/// its bytes does **not**. F02 makes the same trade for the installation
/// content hash, and it is stated there too: the digest describes bytes, and the
/// *identity* of a run lives in the replay record's subject and choices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadedContent {
    rows: BTreeMap<String, ContentHash>,
}

impl LoadedContent {
    /// No content at all.
    ///
    /// Its digest is the SHA-256 of an empty row list, not of a placeholder: a
    /// run that loaded nothing is described by exactly that, and it is visibly
    /// different from any run that loaded anything.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The content of an inventoried installation.
    ///
    /// Every inventoried row is taken, including unknown, unparsed and failed
    /// rows: the run loaded the installation, so the digest has to describe all
    /// of it rather than the part a loader understood.
    #[must_use]
    pub fn from_manifest(manifest: &InstallManifest) -> Self {
        let mut content = Self::new();
        for file in &manifest.files {
            content.insert(&file.relative_spelling.logical_key(), file.sha256);
        }
        content
    }

    /// The content of one declared record, keyed by `key`.
    #[must_use]
    pub fn of_record(key: &str, record_digest: ContentHash) -> Self {
        let mut content = Self::new();
        content.insert(key, record_digest);
        content
    }

    /// Records one loaded row, replacing an earlier row with the same key.
    pub fn insert(&mut self, key: &str, digest: ContentHash) {
        self.rows.insert(key.to_owned(), digest);
    }

    /// Records one loaded row and returns the set, for building one expression.
    #[must_use]
    pub fn with_row(mut self, key: &str, digest: ContentHash) -> Self {
        self.insert(key, digest);
        self
    }

    /// The digest recorded for one logical key.
    #[must_use]
    pub fn row(&self, key: &str) -> Option<ContentHash> {
        self.rows.get(key).copied()
    }

    /// How many rows the run loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the run loaded nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The loaded keys, in the order [`digest`](Self::digest) hashes them.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.rows.keys().map(String::as_str)
    }

    /// The run's content digest: SHA-256 over the row digests in logical-key
    /// order.
    ///
    /// For a [`from_manifest`](Self::from_manifest) set this is exactly
    /// `cs_assets::install::content_fingerprint(manifest)`.
    ///
    /// There is deliberately **no domain-separation prefix** in front of the row
    /// digests, and that is the point: this digest's definition *is* F02's
    /// `content_sha256`, so the value in a replay record, the value in an
    /// evidence artifact and the value `cs_assets` computes are one number.
    /// Prefixing a copy would create a second definition that agrees with the
    /// first nowhere, which is the drift this module exists to prevent. The
    /// record's compatibility signature is domain-separated separately in
    /// `cs_content::replay`, so nothing reads this digest without that
    /// separation.
    #[must_use]
    pub fn digest(&self) -> ContentHash {
        let mut hasher = Sha256::new();
        for digest in self.rows.values() {
            hasher.update(digest.as_bytes());
        }
        hasher.finalize()
    }
}

/// Digests the airframe coefficients a flight run flew, field by field.
///
/// Every numeric coefficient goes in as its IEEE-754 bit pattern and the
/// provenance of the record as its stable label, each length-prefixed, so no two
/// tunings can reach the same digest by moving a decimal point. The result is
/// the row digest for [`AIRFRAME_CONTENT_KEY`].
///
/// What is in here is **content**: the mass, inertia, engine curve, boost,
/// drag, lift, stall, attitude response, assist gains and wing area the law
/// consumes, plus the `origin` that says where those numbers came from. One of
/// them changing is a content-asset edit, and it moves the content digest even
/// when the record's own shape is untouched.
///
/// What is deliberately *not* here is [`rules_digest`]'s half: which law runs,
/// under which handling profile, at which rate, and whether assists may
/// contribute at all are the evaluation context rather than coefficients, and
/// folding them in here would make a profile switch report as a content change.
#[must_use]
pub fn airframe_content_digest(tuning: &AirframeTuning) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(AIRFRAME_DOMAIN);
    put_label(&mut hasher, tuning.origin.label());
    put_f64(&mut hasher, tuning.mass.mass_kg);
    for value in tuning.mass.inertia_kg_m2 {
        put_f64(&mut hasher, value);
    }
    put_f64(&mut hasher, tuning.engine.idle_thrust_n);
    put_f64(&mut hasher, tuning.engine.max_thrust_n);
    put_f64(&mut hasher, tuning.engine.throttle_response_per_s);
    put_f64(&mut hasher, tuning.boost.thrust_n);
    put_f64(&mut hasher, tuning.boost.consumption_per_s);
    put_f64(&mut hasher, tuning.drag.zero_lift_coefficient);
    put_f64(&mut hasher, tuning.drag.induced_coefficient);
    put_f64(&mut hasher, tuning.lift.lift_at_zero_alpha);
    put_f64(&mut hasher, tuning.lift.lift_slope_per_rad);
    put_f64(&mut hasher, tuning.lift.max_lift_coefficient);
    put_f64(&mut hasher, tuning.stall.stall_angle_rad);
    put_f64(&mut hasher, tuning.stall.stall_width_rad);
    put_f64(&mut hasher, tuning.stall.residual_fraction);
    put_f64(&mut hasher, tuning.angular.rate_gain_per_s);
    put_f64(&mut hasher, tuning.angular.rate_damping_per_s);
    for value in tuning.angular.max_rate_radps {
        put_f64(&mut hasher, value);
    }
    for value in tuning.angular.max_torque_nm {
        put_f64(&mut hasher, value);
    }
    put_f64(&mut hasher, tuning.angular.control_airspeed_full_mps);
    put_f64(&mut hasher, tuning.assists.bank_level_gain_nm_per_rad);
    put_f64(&mut hasher, tuning.assists.bank_level_max_torque_nm);
    put_f64(&mut hasher, tuning.reference_area_m2);
    hasher.finalize()
}

/// Digests the declared rules one run flew under: the law, the handling profile,
/// the assist policy and the fixed rate.
///
/// This is deliberately **not** the tuning's coefficients (those are
/// [`airframe_content_digest`]) and not the candidate tree. It is the
/// evaluation context: change the law, switch the profile from fidelity to
/// improved, let assists contribute, or run the same content at 120 Hz instead
/// of 60, and the state a replay promised moves for a reason a reader can name
/// — and the refusal says `Rules`, not `Content`.
#[must_use]
pub fn rules_digest(tuning: &AirframeTuning, fixed_hz: u32) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(RULES_DOMAIN);
    put_label(&mut hasher, tuning.model_kind.label());
    put_label(&mut hasher, tuning.profile.label());
    put_bool(&mut hasher, tuning.assists.enabled);
    put_u64(&mut hasher, u64::from(fixed_hz));
    hasher.finalize()
}

/// Digests the engine build that produced a state stream.
///
/// It names the crate version, the toolchain the caller recorded and the state
/// format version, so a rebuild on another toolchain or a changed reading shape
/// is a different engine rather than the same one with different content. It
/// does **not** cover the sources: that is [`BuildId`], the candidate tree.
#[must_use]
pub fn engine_digest(toolchain: &str) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(ENGINE_DOMAIN);
    put_label(&mut hasher, env!("CARGO_PKG_VERSION"));
    put_text(&mut hasher, toolchain);
    put_label(&mut hasher, STATE_FORMAT_VERSION);
    hasher.finalize()
}

/// The platform tag of the build that is running the capture.
///
/// Taken from the compiled-in target, not from an environment variable, so it
/// describes the binary rather than a claim about the host.
#[must_use]
pub fn host_platform() -> PlatformTag {
    PlatformTag::new(std::env::consts::OS, std::env::consts::ARCH)
        .expect("the compiled-in target os and arch are valid platform label characters")
}

/// Everything a run's [`BuildFingerprint`] is built from, each part measured
/// from something the run really had.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunIdentity {
    /// The candidate tree the binary was built from.
    pub tree: BuildId,
    /// Digest of the engine build and the state format version.
    pub engine: ContentHash,
    /// Digest of the content the run loaded.
    pub content: ContentHash,
    /// Digest of the declared rules the run flew under.
    pub rules: ContentHash,
    /// The engine and toolchain version string.
    pub toolchain: String,
    /// The platform the build ran on.
    pub platform: PlatformTag,
}

impl RunIdentity {
    /// Builds the identity of a run over `loaded` content, the rules declared by
    /// `tuning` at `fixed_hz`, and the build coordinates the caller recorded.
    ///
    /// # Errors
    ///
    /// [`ReplayError`] from [`BuildFingerprint::validate`] when `toolchain` is
    /// blank or over-long.
    pub fn new(
        loaded: &LoadedContent,
        tuning: &AirframeTuning,
        fixed_hz: u32,
        tree: BuildId,
        toolchain: &str,
        platform: PlatformTag,
    ) -> Result<Self, ReplayError> {
        let identity = Self {
            engine: engine_digest(toolchain),
            content: loaded.digest(),
            rules: rules_digest(tuning, fixed_hz),
            tree,
            toolchain: toolchain.to_owned(),
            platform,
        };
        identity.fingerprint().validate()?;
        Ok(identity)
    }

    /// The record fingerprint this identity is.
    #[must_use]
    pub fn fingerprint(&self) -> BuildFingerprint {
        BuildFingerprint {
            tree: self.tree.clone(),
            engine: self.engine,
            content: self.content,
            rules: self.rules,
            toolchain: self.toolchain.clone(),
            platform: self.platform.clone(),
        }
    }
}

fn put_text(hasher: &mut Sha256, text: &str) {
    put_bytes(hasher, text.as_bytes());
}

fn put_label(hasher: &mut Sha256, label: &str) {
    put_bytes(hasher, label.as_bytes());
}

fn put_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn put_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(&value.to_be_bytes());
}

fn put_bool(hasher: &mut Sha256, value: bool) {
    hasher.update(&[u8::from(value)]);
}

/// Writes an `f64` as its IEEE-754 bit pattern.
///
/// A tuning value of `0.1` and one of `0.10000000000000001` are the same
/// double, and writing the decimal spelling instead would make two runs of the
/// same tuning disagree over a rounding of the text.
fn put_f64(hasher: &mut Sha256, value: f64) {
    hasher.update(&value.to_bits().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_sim::flight::{HandlingProfile, synthetic_fixed_wing};

    #[test]
    fn an_edited_content_row_moves_the_content_digest() {
        let loaded = LoadedContent::of_record(
            AIRFRAME_CONTENT_KEY,
            airframe_content_digest(&synthetic_fixed_wing()),
        );
        let before = loaded.digest();
        let edited = LoadedContent::of_record(
            AIRFRAME_CONTENT_KEY,
            airframe_content_digest(&synthetic_fixed_wing()),
        );
        assert_eq!(before, edited.digest());
        let mut moved = LoadedContent::of_record(
            AIRFRAME_CONTENT_KEY,
            airframe_content_digest(&synthetic_fixed_wing()),
        );
        moved.insert(AIRFRAME_CONTENT_KEY, cs_assets::install::sha256(b"edited"));
        assert_ne!(before, moved.digest());
    }

    #[test]
    fn the_handling_profile_is_rules_not_content() {
        let mut improved = synthetic_fixed_wing();
        improved.profile = HandlingProfile::Improved;
        assert_eq!(
            airframe_content_digest(&synthetic_fixed_wing()),
            airframe_content_digest(&improved),
            "the handling profile is the evaluation context, not a coefficient"
        );
        assert_ne!(
            rules_digest(&synthetic_fixed_wing(), 120),
            rules_digest(&improved, 120)
        );
        assert_ne!(
            rules_digest(&synthetic_fixed_wing(), 120),
            rules_digest(&synthetic_fixed_wing(), 60)
        );
    }

    #[test]
    fn an_edited_coefficient_is_content_not_rules() {
        let mut heavier = synthetic_fixed_wing();
        heavier.mass.mass_kg += 1.0;
        assert_ne!(
            airframe_content_digest(&synthetic_fixed_wing()),
            airframe_content_digest(&heavier)
        );
        assert_eq!(
            rules_digest(&synthetic_fixed_wing(), 120),
            rules_digest(&heavier, 120),
            "a coefficient edit must not also report as a rules change"
        );
    }
}
