//! The `accept_f62_a_*` suite: binds the declared corpus contract to
//! production `cs_formats` code and runs the truncation oracle.
//!
//! The oracle itself lives in [`CorpusFixture::requires_refusal`] and in
//! [`cs_xtask::corpus`]'s manifest; these tests only resolve both against
//! real bytes and real parsers.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use cs_xtask::corpus::{
    self, CorpusClass, CorpusSource, ExpectedOutcome, PrivateAvailability, TruncationOracle,
};

use crate::bindings::{Binding, ProbeOutcome, registry};
use crate::spans::CorpusFixture;

fn binding_for(container: &str) -> &'static Binding {
    registry()
        .iter()
        .find(|binding| binding.container == container)
        .unwrap_or_else(|| panic!("no corpus binding for container {container}"))
}

/// Runs a probe with panic isolation: a panic is a failure, and the cut
/// that produced it is named.
fn probe(binding: &Binding, fixture: &CorpusFixture, cut: &[u8]) -> ProbeOutcome {
    match catch_unwind(AssertUnwindSafe(|| (binding.probe)(fixture, cut))) {
        Ok(outcome) => outcome,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".to_owned());
            panic!(
                "{}: parser panicked on a {}-byte input: {message}",
                binding.container,
                cut.len(),
            );
        }
    }
}

/// The manifest is internally consistent and every builder entry resolves
/// to a binding.
#[test]
fn accept_f62_a_manifest_is_internally_consistent() {
    let errors = corpus::verify_manifest();
    assert!(errors.is_empty(), "manifest violations: {errors:?}");

    for entry in corpus::entries() {
        if let CorpusSource::Builder { builder } = entry.source {
            binding_for(builder);
        }
    }
}

/// Every declared container has exactly one binding and no binding is
/// orphaned.
#[test]
fn accept_f62_a_registry_binds_every_declared_container() {
    let bindings = registry();
    for spec in corpus::containers() {
        let matches = bindings
            .iter()
            .filter(|binding| binding.container == spec.id)
            .count();
        assert_eq!(
            matches, 1,
            "container {} needs exactly one binding",
            spec.id
        );
    }
    for binding in bindings {
        assert!(
            corpus::find_container(binding.container).is_some(),
            "binding {} names no declared container",
            binding.container,
        );
    }
}

/// Every synthetic fixture resolves each boundary kind its container
/// declares, and the full buffer satisfies the entry's expected outcome.
#[test]
fn accept_f62_a_fixtures_resolve_declared_boundaries() {
    for spec in corpus::containers() {
        let binding = binding_for(spec.id);
        let fixture = (binding.fixture)();
        for kind in spec.boundaries {
            assert!(
                fixture.resolved_kinds().contains(kind),
                "{}: no span resolves declared boundary {kind}",
                spec.id,
            );
        }
        match probe(binding, &fixture, &fixture.bytes) {
            ProbeOutcome::Accepted(detail) => detail,
            other => panic!(
                "{}: the authored fixture must parse, got {other:?}",
                spec.id,
            ),
        };
    }
}

/// AC01: every cut inside a required span — every container header and
/// variable-length table boundary the contract knows — produces a bounded
/// diagnostic; the never-silent rule.
#[test]
fn accept_f62_a_truncation_refuses_every_required_cut() {
    let mut required_total = 0usize;
    let mut probed_total = 0usize;
    for spec in corpus::containers() {
        let binding = binding_for(spec.id);
        let fixture = (binding.fixture)();
        let mut required = 0usize;
        for cut in 0..fixture.bytes.len() {
            probed_total += 1;
            let outcome = probe(binding, &fixture, &fixture.bytes[..cut]);
            if !fixture.requires_refusal(cut) {
                continue; // slack and element boundaries: bounded-any
            }
            required += 1;
            match spec.truncation {
                TruncationOracle::PrefixRefusal => match outcome {
                    ProbeOutcome::Refused(code) => {
                        assert!(
                            !code.is_empty(),
                            "{}: cut {cut} refused without a code",
                            spec.id,
                        );
                    }
                    other => panic!(
                        "{}: cut {cut} damages a required span but the entrypoint \
                         produced {other:?}",
                        spec.id,
                    ),
                },
                TruncationOracle::ExtentStatus { .. } => match outcome {
                    ProbeOutcome::Refused(_) | ProbeOutcome::Damaged => {}
                    ProbeOutcome::Accepted(detail) => panic!(
                        "{}: cut {cut} drops member bytes the listing reports as \
                         intact ({detail})",
                        spec.id,
                    ),
                },
                TruncationOracle::NotApplicable { rationale } => {
                    panic!(
                        "{}: NotApplicable container has a required cut ({rationale})",
                        spec.id
                    );
                }
            }
        }
        if !matches!(spec.truncation, TruncationOracle::NotApplicable { .. }) {
            assert!(
                required > 0,
                "{}: the fixture declares no required cut at all",
                spec.id,
            );
        }
        required_total += required;
        println!(
            "accept_f62_a: {}: {} required cuts, {} bytes",
            spec.id,
            required,
            fixture.bytes.len(),
        );
    }
    assert!(
        required_total >= 1_000,
        "the truncation corpus is too thin: {required_total} required cuts",
    );
    println!("accept_f62_a: {required_total} required cuts over {probed_total} probes");
}

/// Cuts that damage nothing required still demand a bounded outcome; the
/// `probe` helper turns any panic into a failure.
#[test]
fn accept_f62_a_unrequired_cuts_stay_bounded() {
    for spec in corpus::containers() {
        let binding = binding_for(spec.id);
        let fixture = (binding.fixture)();
        for cut in 0..fixture.bytes.len() {
            if fixture.requires_refusal(cut) {
                continue;
            }
            let _ = probe(binding, &fixture, &fixture.bytes[..cut]);
        }
    }
}

/// The committed synthetic fixtures parse (or refuse) exactly as the
/// manifest's `expected` says, through the container's own binding.
#[test]
fn accept_f62_a_committed_fixtures_match_expected_outcomes() {
    let root = workspace_root();
    for entry in corpus::entries() {
        let CorpusSource::CommittedFixture { path } = entry.source else {
            continue;
        };
        let bytes =
            std::fs::read(root.join(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        let binding = binding_for(entry.container);
        let context_fixture = (binding.fixture)();
        match (entry.expected, probe(binding, &context_fixture, &bytes)) {
            (ExpectedOutcome::Accept, ProbeOutcome::Accepted(_))
            | (ExpectedOutcome::Refuse, ProbeOutcome::Refused(_)) => {}
            (expected, outcome) => {
                panic!("{path}: expected {expected:?}, the container probe produced {outcome:?}",)
            }
        }
    }
}

/// The separation audit on the real tracked tree: no private bytes under a
/// private-only prefix, every committed fixture tracked, the private suite
/// reported unavailable rather than silently passed.
#[test]
fn accept_f62_a_audit_reports_real_counts() {
    let report = corpus::audit(&workspace_root(), None).expect("the audit runs");
    assert!(
        report.tracked_private_paths.is_empty(),
        "private bytes tracked in git: {:?}",
        report.tracked_private_paths,
    );
    assert!(
        report.missing_committed_fixtures.is_empty(),
        "committed fixtures not tracked: {:?}",
        report.missing_committed_fixtures,
    );
    assert!(
        report.manifest_errors.is_empty(),
        "manifest violations: {:?}",
        report.manifest_errors,
    );
    assert_eq!(
        report.private_availability,
        PrivateAvailability::Unavailable,
        "no --private-root means the private suite did not run",
    );

    let counts = corpus::manifest_counts();
    assert_eq!(counts.containers, corpus::containers().len());
    assert_eq!(
        counts.synthetic + counts.private + counts.regression,
        corpus::entries().len(),
        "every entry is counted exactly once",
    );
    assert!(
        counts.private > 0,
        "the private selectors must exist so the suite can never silently pass as complete",
    );
}

/// The corpus fingerprint function is FIPS 180-4 SHA-256 — private audit
/// reports are only meaningful if the digest is real.
#[test]
fn accept_f62_a_sha256_fingerprints_match_published_vectors() {
    assert_eq!(
        corpus::sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
    assert_eq!(
        corpus::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
    assert_eq!(
        corpus::sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );
}

/// The manifest's JSON form round-trips the real declaration — the
/// machine-readable contract later stages consume.
#[test]
fn accept_f62_a_manifest_json_reports_real_counts() {
    let json = corpus::manifest_json();
    assert!(json.contains("\"id\": \"rof.directory\""));
    assert!(json.contains("\"kind\": \"extent_status\""));
    assert!(json.contains("\"kind\": \"private_install\""));
    let counts = corpus::manifest_counts();
    assert!(json.contains(&format!("\"containers\": {}", counts.containers)));
    assert!(json.contains(&format!("\"synthetic\": {}", counts.synthetic)));
}

/// The private corpus never produces entries from repo bytes: no declared
/// entry may carry a private source without the `Private` class, and no
/// `Synthetic`/`Regression` entry may point at a private source.
#[test]
fn accept_f62_a_class_and_source_never_cross() {
    for entry in corpus::entries() {
        let private_source = matches!(
            entry.source,
            CorpusSource::PrivateInstall { .. } | CorpusSource::PrivateSeed { .. }
        );
        match entry.class {
            CorpusClass::Private => assert!(private_source, "{}", entry.id),
            CorpusClass::Synthetic | CorpusClass::Regression => {
                assert!(!private_source, "{}", entry.id)
            }
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits at crates/cs_formats")
        .to_path_buf()
}
