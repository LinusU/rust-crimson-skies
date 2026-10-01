//! Acceptance scenarios F59-A: replay, capture and evidence schema.
//! Task test prefix: `accept_f59_a_`.
//!
//! Spec: `specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
//! stage `### F59-A`. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//! Every value is newly authored synthetic data; nothing here is derived from
//! the original game, and no assertion certifies original behavior.

use cs_assets::install::sha256;
use cs_content::replay::{
    ArtifactDescription, ArtifactMedia, ArtifactOrigin, AuthoredChoice, AuthoredChoices,
    BuildFingerprint, BuildId, CandidateBuild, CapabilityClass, CaptureDifference, CaptureError,
    Certification, ChoiceSlot, CompatibilityVerdict, CrossBuildPolicy, DeclaredCapabilities,
    DecodeError, DivergenceReason, EnvelopeError, EvidenceArtifact, EvidenceBundle,
    EvidenceRefusal, InitialState, InputStreamDigest, MAX_MSAA_SAMPLES, MAX_REPLAY_LINES,
    MAX_STREAM_RECORDS, OverrideEntry, OverrideLog, PcmRole, PlatformTag, RenderConfig,
    ReplayError, ReplayField, ReplayRecord, ReplaySeeds, ReplayVersion, RunPurpose, SeedStream,
    StaleReason, StateEnvelope, TestCounts, TonemapKind, decode, decode_capture, encode,
    encode_capture, synthetic_capture_record, synthetic_evidence_bundle, synthetic_replay_record,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{
    ClaimId, ClaimStatus, ContentHash, Fingerprint, FingerprintKind, ObservationMethod,
};
use cs_types::input::{Action, AxisValue, CommandStream, FlightCommand, InputFrame, UiAction};
use cs_types::random::SYNTHETIC_BODY_DOMAIN;
use cs_types::space::{Radians, WorldPosition};

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id")
}

fn fixture() -> ReplayRecord {
    synthetic_replay_record()
}

/// A copy of `record` with a different content digest, as a changed content
/// asset would produce.
fn with_content(record: &ReplayRecord, content: ContentHash) -> ReplayRecord {
    let mut changed = record.clone();
    changed.fingerprint.content = content;
    changed
}

/// A copy of `record` whose third recorded tick carries one quantization step
/// more pitch, as a slightly different input would produce.
fn with_pitch_nudged(record: &ReplayRecord) -> ReplayRecord {
    let mut changed = record.clone();
    let mut stream = CommandStream::new();
    for frame in changed.stream.records() {
        let mut copy = InputFrame::new(frame.frame_tick());
        for edge in frame.edges() {
            copy.push_edge(*edge);
        }
        for axis in frame.axes() {
            let quantized = if axis.command() == FlightCommand::Pitch {
                axis.quantized() + 1
            } else {
                axis.quantized()
            };
            copy.set_axis(
                AxisValue::from_quantized(axis.command(), quantized).expect("continuous axis"),
            );
        }
        stream.record_tick(copy).expect("forward ticks");
    }
    changed.stream = stream;
    changed
}

/// An envelope equal to `record`'s promised one except that the state hash at
/// `tick` is different.
fn with_state_hash_changed(record: &ReplayRecord, tick: Tick) -> StateEnvelope {
    let mut envelope = StateEnvelope::new();
    for entry in record.promised.entries() {
        let state = if entry.tick == tick {
            sha256(format!("changed at {}", entry.tick.0).as_bytes())
        } else {
            entry.state
        };
        envelope.push(entry.tick, state).expect("forward ticks");
    }
    envelope
}

fn candidate_of(record: &ReplayRecord) -> CandidateBuild {
    CandidateBuild {
        tree: record.fingerprint.tree.clone(),
        content_sha256: record.fingerprint.content,
    }
}

/* ------------------------------------------------------------------ */
/* AC01: replay an input stream twice and compare the promised hashes   */
/* ------------------------------------------------------------------ */

/// **AC01.** The same input stream, replayed twice through the production
/// document form, produces the same compatibility signature and the same
/// promised envelope, and each replay is compared against the promised hashes
/// with no divergence.
#[test]
fn accept_f59_a_replaying_one_input_stream_twice_gives_identical_promises() {
    // Two independent constructions of the same recording agree on the
    // promised identity without being the same object.
    let first_run = fixture();
    let second_run = fixture();
    assert_eq!(
        first_run.compatibility_signature(),
        second_run.compatibility_signature(),
        "two independent constructions of the same replay must agree on the promised identity"
    );

    // Replaying twice: the record goes out to its document form and comes back
    // each time, and the stream it yields is fed back in.
    let first_replay = replay_through_the_document(&first_run);
    let second_replay = replay_through_the_document(&second_run);
    assert_eq!(
        first_replay.stream, second_replay.stream,
        "the recorded stream survives both replays unchanged"
    );

    for (replay, label) in [(&first_replay, "first"), (&second_replay, "second")] {
        let verdict = replay.verdict_against(&replay_stream(replay, replay));
        assert!(
            verdict.is_identical(),
            "{label}: promised hashes must match the replayed stream, got {:?}",
            verdict.divergence
        );
        assert_eq!(verdict.compared_ticks, replay.promised.len(), "{label}");
    }

    let first = first_replay.verdict_against(&replay_stream(&first_replay, &first_replay));
    let second = second_replay.verdict_against(&replay_stream(&second_replay, &second_replay));
    assert_eq!(first, second, "the two replays must compare the same");
    assert_eq!(
        first_replay
            .promised
            .chain_digest(first_replay.initial_state.digest),
        second_replay
            .promised
            .chain_digest(second_replay.initial_state.digest),
        "the chained envelope digest must agree across replays"
    );
}

/// A replay of one record: it goes through the production document form and
/// comes back, so the stream fed in is the one the file carried.
fn replay_through_the_document(record: &ReplayRecord) -> ReplayRecord {
    decode(&encode(record).expect("encode")).expect("decode")
}

/// Replays `observed`'s recorded stream and returns the state hashes such a
/// run produces.
///
/// `reference` is the recording the run is being compared against: a tick's
/// hash is the reference's promised hash exactly when the frame fed in is the
/// frame the reference recorded, and a different hash otherwise. That is the
/// minimum F59-B's runtime has to do — hash the state a frame produced — and
/// it is keyed on the frame so a test can change the stream and watch the
/// comparison notice.
fn replay_stream(observed: &ReplayRecord, reference: &ReplayRecord) -> StateEnvelope {
    let mut replayed = StateEnvelope::new();
    for frame in observed.stream.records() {
        let tick = frame.frame_tick();
        let promised = reference.promised.entry(tick).expect("promised tick");
        let recorded = reference.stream.record(tick).expect("recorded tick");
        let state = if frame == recorded {
            promised.state
        } else {
            sha256(format!("diverged at {}", tick.0).as_bytes())
        };
        replayed.push(tick, state).expect("recorded ticks increase");
    }
    replayed
}

/// **AC01, failure case.** Change one quantization step of one recorded axis
/// sample and the promised state hashes no longer hold: the divergence is
/// reported at that tick, not somewhere later and not nowhere.
#[test]
fn accept_f59_a_a_changed_input_sample_breaks_the_promised_state_hashes() {
    let record = fixture();
    let altered = with_pitch_nudged(&record);
    assert_ne!(
        input_digest(&record),
        input_digest(&altered),
        "one quantization step must change the recorded stream digest"
    );

    let verdict = record.verdict_against(&replay_stream(&altered, &record));
    assert!(
        !verdict.is_identical(),
        "an altered input stream must not satisfy the promised hashes"
    );
    let divergence = verdict.divergence.expect("a divergence");
    assert_eq!(divergence.tick, Some(Tick(0)), "the first altered tick");
    assert!(
        matches!(divergence.reason, DivergenceReason::State { .. }),
        "the difference is a state hash, not a tick or a length: {divergence}"
    );
    // The verdict is not silently a pass: the comparison stopped at the first
    // difference and reported it.
    assert_eq!(verdict.compared_ticks, 0);
}

/// The compatibility signature is over the stream, so an altered stream is a
/// different run even when the promised envelope is left alone.
#[test]
fn accept_f59_a_an_altered_input_stream_is_a_different_run() {
    let record = fixture();
    let altered = with_pitch_nudged(&record);
    assert_ne!(
        record.compatibility_signature(),
        altered.compatibility_signature()
    );
    let verdict = record.compatibility_with(&altered, CrossBuildPolicy::Reject);
    assert!(!verdict.certifies_determinism());
    assert_eq!(verdict.differences().len(), 1, "{verdict}");
    assert_eq!(verdict.differences()[0].label(), "input_stream");
}

/// A promised envelope missing its last hash is refused, and a comparison
/// against a shorter observed run reports the length rather than passing.
#[test]
fn accept_f59_a_a_truncated_envelope_is_a_divergence_not_a_match() {
    let record = fixture();
    let last = record.promised.last_tick().expect("the fixture has hashes");
    let mut truncated = StateEnvelope::new();
    for entry in record
        .promised
        .entries()
        .iter()
        .filter(|entry| entry.tick != last)
    {
        truncated
            .push(entry.tick, entry.state)
            .expect("forward ticks");
    }
    let verdict = record.verdict_against(&truncated);
    assert!(!verdict.is_identical());
    assert_eq!(
        verdict.divergence.map(|divergence| divergence.reason),
        Some(DivergenceReason::Length {
            expected: record.promised.len(),
            observed: truncated.len(),
        })
    );
}

/// A promised envelope whose ticks do not increase is refused at the point it
/// is built, not accepted and compared.
#[test]
fn accept_f59_a_an_envelope_refuses_a_repeated_or_backwards_tick() {
    let mut envelope = StateEnvelope::new();
    envelope.push(Tick(5), sha256(b"a")).expect("first");
    assert_eq!(
        envelope.push(Tick(5), sha256(b"b")),
        Err(EnvelopeError::NonIncreasingTick {
            last: Tick(5),
            received: Tick(5)
        })
    );
    assert_eq!(
        envelope.push(Tick(4), sha256(b"c")),
        Err(EnvelopeError::NonIncreasingTick {
            last: Tick(5),
            received: Tick(4)
        })
    );
    // The refused entries changed nothing.
    assert_eq!(envelope.len(), 1);
}

/// A promised state hash that changed at one tick is reported at that tick:
/// the promised envelope itself is not silently accepted when a run produced
/// something else.
#[test]
fn accept_f59_a_a_changed_state_hash_is_reported_at_its_own_tick() {
    let record = fixture();
    let middle = record.promised.entries()[1].tick;
    let observed = with_state_hash_changed(&record, middle);
    let verdict = record.verdict_against(&observed);
    assert!(!verdict.is_identical());
    assert_eq!(
        verdict.first_divergent_tick(),
        Some(middle),
        "the divergence is the tick whose hash moved"
    );
    assert_eq!(
        verdict.compared_ticks, 1,
        "the identical prefix is one tick"
    );
    let divergence = verdict.divergence.expect("a divergence");
    assert_eq!(divergence.tick, Some(middle));
    assert!(matches!(divergence.reason, DivergenceReason::State { .. }));
    assert!(
        divergence
            .to_string()
            .contains(&format!("tick {}", middle.0))
    );
}

/// A promised hash from outside the run's declared tick range is refused by
/// the record, and an observation from outside it is refused by the comparison
/// rather than compared.
#[test]
fn accept_f59_a_state_hashes_outside_the_run_range_are_refused() {
    let mut record = fixture();
    let mut envelope = StateEnvelope::new();
    envelope.push(Tick(0), sha256(b"in")).expect("in range");
    envelope
        .push(Tick(9_999), sha256(b"out"))
        .expect("increasing");
    record.promised = envelope;
    assert!(
        matches!(
            record.validate(),
            Err(ReplayError::OutsideTickRange {
                what: "a promised state hash",
                ..
            })
        ),
        "a hash outside the run's range must be refused"
    );

    let mut outside = StateEnvelope::new();
    outside
        .push(Tick(9_999), sha256(b"out"))
        .expect("increasing");
    let verdict = record.verdict_against(&outside);
    assert_eq!(
        verdict.divergence.map(|divergence| divergence.reason),
        Some(DivergenceReason::OutsideRange {
            tick: Tick(9_999),
            first: record.first_tick,
            last: record.last_tick,
        }),
        "an observation from another run is refused, not compared"
    );
}

/* ------------------------------------------------------------------ */
/* AC02: a changed content asset rejects the old replay                 */
/* ------------------------------------------------------------------ */

/// **AC02.** Change a content asset and the old replay is rejected: the
/// compatibility signature moves, the refusal names the content field, and the
/// same-build policy certifies nothing.
#[test]
fn accept_f59_a_a_changed_content_asset_rejects_the_old_replay() {
    let record = fixture();
    let changed = with_content(&record, sha256(b"cs.f59.changed.content"));
    assert_ne!(record.fingerprint.content, changed.fingerprint.content);
    assert_ne!(
        record.compatibility_signature(),
        changed.compatibility_signature(),
        "changed content must move the compatibility signature"
    );

    let verdict = record.compatibility_with(&changed, CrossBuildPolicy::Reject);
    assert!(
        !verdict.certifies_determinism(),
        "a content change must never certify determinism: {verdict}"
    );
    match &verdict {
        CompatibilityVerdict::Rejected { differences, .. } => {
            assert_eq!(differences.len(), 1, "{verdict}");
            assert_eq!(
                differences[0].label(),
                "content",
                "the refusal names the content"
            );
        }
        other => panic!("a content change under the same-build policy is a rejection: {other}"),
    }
    // A second, independent change of a *different* field is refused for that
    // other field, so the content refusal is not a generic "something differs".
    let mut rules_changed = record.clone();
    rules_changed.fingerprint.rules = sha256(b"cs.f59.changed.rules");
    let rules_verdict = record.compatibility_with(&rules_changed, CrossBuildPolicy::Reject);
    assert_eq!(rules_verdict.differences()[0].label(), "rules");
}

/// A best-effort cross-build comparison is an explicit, recorded state: it
/// names the differences and certifies nothing.
#[test]
fn accept_f59_a_best_effort_comparison_names_differences_and_certifies_nothing() {
    let record = fixture();
    let mut other = with_content(&record, sha256(b"cs.f59.other.content"));
    other.tick_rate = 60;
    other.fingerprint.platform = PlatformTag::new("linux", "x86_64").expect("valid tag");

    let verdict = record.compatibility_with(&other, CrossBuildPolicy::BestEffort);
    assert!(
        !verdict.certifies_determinism(),
        "a best-effort verdict must never certify determinism"
    );
    let labels: Vec<&str> = verdict
        .differences()
        .iter()
        .map(|difference| difference.label())
        .collect();
    assert_eq!(labels, ["content", "platform", "tick_rate"], "{verdict}");
    assert!(verdict.to_string().contains("best-effort"), "{verdict}");
    assert!(
        verdict.to_string().contains("no determinism certified"),
        "the verdict must say what it does not prove: {verdict}"
    );

    // The same-build policy refuses the same pair outright.
    let strict = record.compatibility_with(&other, CrossBuildPolicy::Reject);
    assert!(!strict.certifies_determinism());
    assert_eq!(strict.differences().len(), 3);
}

/// An identical record is compatible, and the verdict is the one state in which
/// determinism is certified.
#[test]
fn accept_f59_a_an_unchanged_replay_is_compatible() {
    let record = fixture();
    let verdict = record.compatibility_with(&fixture(), CrossBuildPolicy::Reject);
    assert!(verdict.certifies_determinism(), "{verdict}");
    assert!(verdict.differences().is_empty());
    assert_eq!(
        record.compatibility_signature(),
        match verdict {
            CompatibilityVerdict::Compatible { signature } => signature,
            other => panic!("unchanged record: {other}"),
        }
    );
}

/// Moves one identity field of a record, so the test can ask what the refusal
/// names when it moved.
type IdentityField = Box<dyn Fn(&mut ReplayRecord)>;

/// Each field that decides a run's identity is load-bearing: moving any one of
/// them is a refusal, and naming the right one.
#[test]
fn accept_f59_a_every_identity_field_is_load_bearing() {
    let record = fixture();
    let other_tree = BuildId::new("2222222222222222222222222222222222222222").expect("valid");
    let other_os = PlatformTag::new("linux", "x86_64").expect("valid");

    let other_subject = ContentId::from_source(ContentKind::Mission, "synthetic.other")
        .expect("a valid content id");

    let cases: Vec<(&str, IdentityField)> = vec![
        (
            "schema",
            Box::new(|r: &mut ReplayRecord| r.schema = ReplayVersion { major: 1, minor: 4 }),
        ),
        (
            "subject",
            Box::new(move |r: &mut ReplayRecord| r.subject = other_subject.clone()),
        ),
        (
            "engine",
            Box::new(|r: &mut ReplayRecord| r.fingerprint.engine = sha256(b"engine2")),
        ),
        (
            "rules",
            Box::new(|r: &mut ReplayRecord| r.fingerprint.rules = sha256(b"rules2")),
        ),
        (
            "tree",
            Box::new(move |r: &mut ReplayRecord| r.fingerprint.tree = other_tree.clone()),
        ),
        (
            "toolchain",
            Box::new(|r: &mut ReplayRecord| r.fingerprint.toolchain = "other".to_owned()),
        ),
        (
            "platform",
            Box::new(move |r: &mut ReplayRecord| r.fingerprint.platform = other_os.clone()),
        ),
        (
            "tick_rate",
            Box::new(|r: &mut ReplayRecord| r.tick_rate = 30),
        ),
        (
            "tick_range",
            Box::new(|r: &mut ReplayRecord| r.last_tick = Tick(99)),
        ),
        (
            "initial_state",
            Box::new(|r: &mut ReplayRecord| r.initial_state.digest = sha256(b"initial2")),
        ),
        (
            "initial_state",
            Box::new(|r: &mut ReplayRecord| {
                r.initial_state.label = "synthetic.replay@tick7".to_owned()
            }),
        ),
        (
            "overrides",
            Box::new(|r: &mut ReplayRecord| {
                r.overrides = OverrideLog::new(RunPurpose::Capture, Vec::new(), false)
                    .expect("a valid override log")
            }),
        ),
        (
            "overrides",
            Box::new(|r: &mut ReplayRecord| {
                r.overrides = OverrideLog::new(
                    RunPurpose::OrdinaryPlay,
                    vec![OverrideEntry {
                        name: "debug_overrides".to_owned(),
                        detail: "godmode=1".to_owned(),
                    }],
                    false,
                )
                .expect("a valid override log")
            }),
        ),
        (
            "overrides",
            Box::new(|r: &mut ReplayRecord| {
                r.overrides =
                    OverrideLog::new(RunPurpose::OrdinaryPlay, Vec::new(), true).expect("valid")
            }),
        ),
        ("seeds", Box::new(|r: &mut ReplayRecord| r.seeds = seeds(7))),
        (
            "input_stream",
            Box::new(|r: &mut ReplayRecord| r.stream = CommandStream::new()),
        ),
        (
            "choices",
            Box::new(|r: &mut ReplayRecord| r.choices = AuthoredChoices::none()),
        ),
        (
            "promised_envelope",
            Box::new(|r: &mut ReplayRecord| r.promised = StateEnvelope::new()),
        ),
    ];

    for (label, mutate) in cases {
        let mut changed = record.clone();
        mutate(&mut changed);
        // Every field the signature covers is compared field-wise, so a field
        // that moves the signature always names itself in the refusal. A field
        // in the signature and missing from `differences_from` would silently
        // certify determinism here, which is the shortcut this guards.
        assert_ne!(
            record.compatibility_signature(),
            changed.compatibility_signature(),
            "{label}: the mutation must move the signature at all"
        );
        let verdict = record.compatibility_with(&changed, CrossBuildPolicy::Reject);
        assert!(!verdict.certifies_determinism(), "{label}: {verdict}");
        assert_eq!(
            verdict.differences()[0].label(),
            label,
            "{label}: the refusal names the field that moved"
        );
    }
}

/// The two functions that decide whether two runs are the same run stay in
/// step: no field moves the compatibility signature without also being named by
/// `differences_from`, and no field is named by `differences_from` that does
/// not move it.
///
/// This is the invariant the case list above checks one field at a time, stated
/// as a property: it is the difference between "the signature moved" and "the
/// comparison refused", and AC02 needs the second one.
#[test]
fn accept_f59_a_every_signature_field_is_compared_field_wise() {
    let record = fixture();
    let mutations: Vec<(&str, IdentityField)> = vec![
        (
            "schema",
            Box::new(|r: &mut ReplayRecord| r.schema = ReplayVersion { major: 1, minor: 4 }),
        ),
        (
            "subject",
            Box::new(|r: &mut ReplayRecord| {
                r.subject = ContentId::from_source(ContentKind::Mission, "synthetic.other")
                    .expect("a valid content id")
            }),
        ),
        (
            "initial_state",
            Box::new(|r: &mut ReplayRecord| r.initial_state.label = "other".to_owned()),
        ),
        (
            "overrides",
            Box::new(|r: &mut ReplayRecord| {
                r.overrides = OverrideLog::new(RunPurpose::Probe, Vec::new(), true).expect("valid")
            }),
        ),
        (
            "content",
            Box::new(|r: &mut ReplayRecord| r.fingerprint.content = sha256(b"other")),
        ),
    ];

    for (label, mutate) in mutations {
        let mut changed = record.clone();
        mutate(&mut changed);
        let verdict = record.compatibility_with(&changed, CrossBuildPolicy::Reject);
        assert!(
            !verdict.certifies_determinism(),
            "{label}: a moved signature must not certify determinism"
        );
        assert!(
            verdict.differences().iter().any(|d| d.label() == label),
            "{label}: differences_from must name it: {:?}",
            verdict.differences()
        );
    }

    // And the converse direction: the two records themselves agree, so nothing
    // is named and determinism is certified.
    let identical = fixture();
    let verdict = record.compatibility_with(&identical, CrossBuildPolicy::Reject);
    assert!(verdict.certifies_determinism(), "{verdict}");
    assert!(verdict.differences().is_empty());
}

fn seeds(root: u64) -> ReplaySeeds {
    ReplaySeeds::derive(root, &[(SYNTHETIC_BODY_DOMAIN, "synthetic_body")])
        .expect("valid seed list")
}

/* ------------------------------------------------------------------ */
/* Seeds                                                               */
/* ------------------------------------------------------------------ */

/// The same root seed produces the same streams; a different root moves every
/// one of them, and a duplicate label is refused.
#[test]
fn accept_f59_a_seeds_are_domain_separated_and_derived_from_the_root() {
    let a = seeds(1);
    let b = seeds(1);
    let c = seeds(2);
    assert_eq!(a, b, "the same root derives the same streams");
    assert_ne!(a, c, "a different root is a different run");
    assert_eq!(a.digest(), b.digest());
    assert_ne!(a.digest(), c.digest());
    assert!(a.stream("synthetic_body").is_some());
    assert!(a.stream("absent").is_none());

    // Two consumers with different domains get different streams from one root:
    // adding a consumer never moves another's values.
    let one = ReplaySeeds::derive(5, &[(1, "first")]).expect("valid");
    let two = ReplaySeeds::derive(5, &[(1, "first"), (2, "second")]).expect("valid");
    assert_eq!(
        one.stream("first"),
        two.stream("first"),
        "a second consumer must not shift the first one's stream"
    );
    assert_ne!(one.stream("first"), two.stream("second"));

    let duplicate = ReplaySeeds::new(
        1,
        vec![
            SeedStream {
                label: "same".to_owned(),
                seed: 1,
            },
            SeedStream {
                label: "same".to_owned(),
                seed: 2,
            },
        ],
    );
    assert!(matches!(
        duplicate,
        Err(ReplayError::Duplicate {
            what: "seed stream",
            ..
        })
    ));
}

/* ------------------------------------------------------------------ */
/* The document form                                                   */
/* ------------------------------------------------------------------ */

/// A record survives a full encode/decode round trip, including its input
/// stream, its promised hashes, its seeds and its pinned choices.
#[test]
fn accept_f59_a_a_replay_document_round_trips_exactly() {
    let record = fixture();
    let bytes = encode(&record).expect("encode");
    let decoded = decode(&bytes).expect("decode");
    assert_eq!(decoded, record);
    assert_eq!(decoded.stream, record.stream, "the stream survives");
    assert_eq!(decoded.promised, record.promised, "the hashes survive");
    assert_eq!(decoded.seeds, record.seeds, "the seeds survive");
    assert_eq!(
        decoded.compatibility_signature(),
        record.compatibility_signature(),
        "the signature survives a round trip"
    );
    // Re-encoding the decoded record reproduces the same bytes.
    assert_eq!(encode(&decoded).expect("re-encode"), bytes);
}

/// A record's UI action and flight edge both survive the round trip and come
/// back as the same kind of action, not as the other kind.
#[test]
fn accept_f59_a_a_recorded_action_survives_the_document_form() {
    let record = fixture();
    let decoded = decode(&encode(&record).expect("encode")).expect("decode");
    let edges: Vec<Action> = decoded.stream.edges();
    assert!(
        edges.contains(&Action::Flight(FlightCommand::FirePrimary)),
        "the flight edge survives: {edges:?}"
    );
    assert!(
        edges.contains(&Action::Ui(UiAction::Pause)),
        "the ui edge survives as a ui action: {edges:?}"
    );
    // The fixture drives a continuous axis; the quantized sample is exact.
    let axes: Vec<i16> = record
        .stream
        .records()
        .iter()
        .flat_map(|frame| frame.axes())
        .map(|axis| axis.quantized())
        .collect();
    assert!(!axes.is_empty(), "the fixture drives an axis");
    let decoded_axes: Vec<i16> = decoded
        .stream
        .records()
        .iter()
        .flat_map(|frame| frame.axes())
        .map(|axis| axis.quantized())
        .collect();
    assert_eq!(axes, decoded_axes, "quantized samples are exact");
}

/// The seal is checked: a flipped byte in the body is refused, and a truncated
/// or unsealed document is refused.
#[test]
fn accept_f59_a_a_tampered_or_unsealed_replay_document_is_refused() {
    let record = fixture();
    let text = String::from_utf8(encode(&record).expect("encode")).expect("utf8");

    // Flip one digit of the recorded content hash, keeping the checksum line.
    let tampered = text.replace(
        &format!("content={}", record.fingerprint.content),
        &format!("content={}", sha256(b"other")),
    );
    assert_ne!(tampered, text, "the tamper changed the body");
    assert_eq!(
        decode(tampered.as_bytes()),
        Err(DecodeError::ChecksumMismatch),
        "a changed body must not pass the seal"
    );

    assert_eq!(
        decode(text.trim_end().as_bytes()),
        Err(DecodeError::MissingChecksum),
        "an unsealed document is refused"
    );
    assert_eq!(
        decode(b"CSREPLAY 1.0\n"),
        Err(DecodeError::MissingChecksum),
        "a header with no seal is refused"
    );
    assert_eq!(
        decode(b"not a header\nchecksum=0000\n"),
        Err(DecodeError::BadHeader),
        "a foreign header is refused"
    );
}

/// A newer minor of the same major is readable and its unknown lines survive;
/// a different major is refused and its bytes are not reinterpreted.
#[test]
fn accept_f59_a_a_newer_minor_is_readable_and_a_newer_major_is_refused() {
    let record = fixture();
    // The header is inside the sealed region, so a document with a different
    // version is written by resealing the same body.
    let body = sealed_body_of(&encode(&record).expect("encode"));

    let minor = reseal(&format!("CSREPLAY 1.7\n{body}"));
    let decoded = decode(&minor).expect("a newer minor is readable");
    assert_eq!(decoded.schema, ReplayVersion { major: 1, minor: 7 });
    assert_eq!(
        decoded.stream, record.stream,
        "a newer minor's body is read, not discarded"
    );

    let major = reseal(&format!("CSREPLAY 2.0\n{body}"));
    assert_eq!(
        decode(&major),
        Err(DecodeError::UnsupportedMajor { major: 2 }),
        "a different major is refused, not partially read"
    );
}

/// An unknown line of a readable major is preserved verbatim and re-emitted,
/// so a newer build's document survives an older build's round trip.
#[test]
fn accept_f59_a_an_unknown_field_is_preserved_and_re_emitted() {
    let mut record = fixture();
    record.extra.push(ReplayField {
        key: "future.field".to_owned(),
        value: "some-newer-value".to_owned(),
    });
    let bytes = encode(&record).expect("encode");
    let decoded = decode(&bytes).expect("decode");
    assert_eq!(decoded.extra, record.extra, "the unknown line survives");
    assert_eq!(
        encode(&decoded).expect("re-encode"),
        bytes,
        "and it is re-emitted unchanged"
    );
    // An unknown field describes a document, not a different run, so the
    // compatibility signature is unmoved.
    let mut without = fixture();
    without.extra.clear();
    assert_eq!(
        decoded.compatibility_signature(),
        without.compatibility_signature(),
        "an unknown field must not change what the run is"
    );
}

/// A pinned choice's provenance **locates** its evidence, and the document form
/// carries that location: a choice that names where it was observed and the same
/// choice with no span are different records, and a round trip does not quietly
/// turn one into the other.
#[test]
fn accept_f59_a_a_choice_provenance_survives_the_document_form() {
    let record = fixture();
    let span = source_span("crimson.dat", Some("missions/m01.dat"), 4096, 512);
    let mut with_span = record.clone();
    with_span.choices = AuthoredChoices::new(vec![AuthoredChoice {
        slot: ChoiceSlot::Mission,
        value: "synthetic.replay".to_owned(),
        provenance: Provenance::new(
            claim("f59.a.observed.choice"),
            ClaimStatus::ObservedTool,
            Some(span),
        )
        .expect("an observed_tool provenance may carry a span"),
    }])
    .expect("valid choices");

    let decoded = decode(&encode(&with_span).expect("encode")).expect("decode");
    assert_eq!(
        decoded.choices.choices()[0].provenance,
        with_span.choices.choices()[0].provenance,
        "the source span must survive the document form verbatim"
    );
    assert_eq!(decoded, with_span, "the whole record round trips");

    // The span is part of the choice's identity, so dropping it moves the
    // signature and is a named difference — a record that laundered an observed
    // value into an unlocated one would otherwise compare equal. The
    // comparison holds the class fixed, so it is the *span* and nothing else
    // that makes the two records differ.
    let mut unlocated = record.clone();
    unlocated.choices = AuthoredChoices::new(vec![AuthoredChoice {
        slot: ChoiceSlot::Mission,
        value: "synthetic.replay".to_owned(),
        provenance: Provenance::new(
            claim("f59.a.observed.choice"),
            ClaimStatus::ObservedTool,
            None,
        )
        .expect("an observed_tool provenance may carry no span"),
    }])
    .expect("valid choices");
    assert_eq!(
        unlocated.choices.choices()[0].provenance.class,
        with_span.choices.choices()[0].provenance.class,
        "the class is held fixed, so only the span differs"
    );
    assert_eq!(
        unlocated.choices.choices()[0].value,
        with_span.choices.choices()[0].value,
        "the value is held fixed too"
    );
    assert_ne!(
        unlocated.compatibility_signature(),
        with_span.compatibility_signature(),
        "a provenance that locates its evidence differs from one that does not"
    );
    let verdict = with_span.compatibility_with(&unlocated, CrossBuildPolicy::Reject);
    assert!(!verdict.certifies_determinism(), "{verdict}");
    assert_eq!(verdict.differences()[0].label(), "choices");

    // A different *location* is also a different choice, not a relabelling of
    // the same evidence.
    let mut elsewhere = with_span.clone();
    elsewhere.choices = AuthoredChoices::new(vec![AuthoredChoice {
        slot: ChoiceSlot::Mission,
        value: "synthetic.replay".to_owned(),
        provenance: Provenance::new(
            claim("f59.a.observed.choice"),
            ClaimStatus::ObservedTool,
            Some(source_span(
                "crimson.dat",
                Some("missions/m02.dat"),
                4096,
                512,
            )),
        )
        .expect("a valid provenance"),
    }])
    .expect("valid choices");
    assert_ne!(
        elsewhere.compatibility_signature(),
        with_span.compatibility_signature(),
        "a span naming a different member is a different choice"
    );

    let without = record.clone();
    assert_ne!(
        with_span.compatibility_signature(),
        without.compatibility_signature()
    );
    let verdict = with_span.compatibility_with(&without, CrossBuildPolicy::Reject);
    assert!(!verdict.certifies_determinism(), "{verdict}");
    assert_eq!(verdict.differences()[0].label(), "choices");

    // A verified_original choice with no span is refused by F01's own rule, and
    // the document form cannot be used to smuggle one past it.
    let text = String::from_utf8(encode(&record).expect("encode")).expect("utf8");
    let stripped = text.replace(
        &format!(
            "choice.mission={}|{}|-|synthetic.replay",
            ClaimStatus::Designed.label(),
            claim("f59.a.synthetic-fixture")
        ),
        &format!(
            "choice.mission={}|{}|-|synthetic.replay",
            ClaimStatus::VerifiedOriginal.label(),
            claim("f59.a.synthetic-fixture")
        ),
    );
    assert_ne!(stripped, text, "the body was edited");
    let forged = sealed_body_of_forged(&stripped);
    assert!(
        matches!(
            decode(&forged),
            Err(DecodeError::Malformed {
                reason: "the choice's provenance is not a valid provenance",
                ..
            })
        ),
        "a verified_original choice with no source span is refused"
    );
}

/// A source span whose keys contain a span delimiter cannot be written and read
/// back unambiguously, so it is refused rather than encoded lossily.
#[test]
fn accept_f59_a_an_ambiguous_source_span_is_refused() {
    let record = fixture();
    for container in ["crimson.dat:extra", "crimson[dat", "crimson+dat"] {
        let mut changed = record.clone();
        let result = AuthoredChoices::new(vec![AuthoredChoice {
            slot: ChoiceSlot::Mission,
            value: "synthetic.replay".to_owned(),
            provenance: Provenance::new(
                claim("f59.a.ambiguous"),
                ClaimStatus::ObservedTool,
                Some(source_span(container, None, 0, 1)),
            )
            .expect("a valid provenance"),
        }]);
        assert!(
            matches!(result, Err(ReplayError::UnpreservableField { .. })),
            "{container} must be refused: {result:?}"
        );
        changed.choices = AuthoredChoices::none();
    }
}

fn source_span(
    container: &str,
    member: Option<&str>,
    offset: u64,
    length: u64,
) -> cs_types::asset_id::SourceSpan {
    cs_types::asset_id::SourceSpan::new(
        sha256(b"cs.f59.test.installation"),
        container,
        member,
        offset,
        length,
        member.map(|_| sha256(b"cs.f59.test.member")),
    )
    .expect("a valid source span")
}

/// Re-seals an edited body whose seal no longer matches, so the decoder reaches
/// the fields rather than refusing the checksum.
fn sealed_body_of_forged(text: &str) -> Vec<u8> {
    let checksum_start = text.rfind("checksum=").expect("a seal");
    let head = &text[..checksum_start];
    format!("{head}checksum={}\n", sha256(head.as_bytes()).to_hex()).into_bytes()
}

/// A record whose encoded form would be larger than the decoder's own byte
/// bound is refused at `encode` rather than written as a file this build cannot
/// read back.
#[test]
fn accept_f59_a_a_record_too_large_to_encode_is_refused_rather_than_written() {
    let mut record = fixture();
    let mut stream = CommandStream::new();
    let mut promised = StateEnvelope::new();
    for tick in 0..MAX_STREAM_RECORDS as u64 {
        let mut frame = InputFrame::new(Tick(tick));
        frame.set_axis(
            AxisValue::from_quantized(FlightCommand::Pitch, (tick % 1000) as i16)
                .expect("pitch is a continuous axis"),
        );
        promised
            .push(Tick(tick), sha256(format!("state {tick}").as_bytes()))
            .expect("increasing ticks");
        stream.record_tick(frame).expect("increasing ticks");
    }
    record.stream = stream;
    record.promised = promised;
    record.first_tick = Tick(0);
    record.last_tick = Tick(MAX_STREAM_RECORDS as u64 - 1);
    // At the declared entry bound the record itself is valid...
    record
        .validate()
        .expect("a record at the entry bounds is valid");
    // ...but it does not fit the declared document bound, and writing it would
    // produce a file `decode` refuses with `TooLarge`.
    assert!(
        matches!(encode(&record), Err(ReplayError::DocumentTooLarge { .. })),
        "encode must not write a document its own decoder refuses"
    );

    // The line bound is checked the way the decoder counts it — over the body,
    // with the header and the seal line excluded — so a document of many short
    // preserved lines is measured honestly rather than slipping past on a
    // two-line discrepancy.
    let many_lines = fixture();
    let document = encode(&many_lines).expect("a small record encodes");
    let body_lines = String::from_utf8(document.clone())
        .expect("utf8")
        .lines()
        .count()
        - 2;
    assert!(body_lines > 0 && body_lines < MAX_REPLAY_LINES);
    let decoded = decode(&document).expect("decode");
    assert_eq!(decoded, many_lines, "and it round trips");
}

/// A preserved unknown line is re-emitted as one `key=value` line, so a value
/// carrying a newline would otherwise become a line the decoder reads as a
/// *different*, interpreted field — an injection of a promised state hash, an
/// input record or a second subject through a field whose whole purpose is to
/// be unknown. Both a newline and a reserved key are refused.
#[test]
fn accept_f59_a_a_preserved_field_cannot_inject_an_interpreted_line() {
    let mut record = fixture();
    record.promised = StateEnvelope::new();
    record.stream = CommandStream::new();

    // A newline in the value: the injected line is a promised state hash at a
    // tick the record promises nothing for.
    let mut injecting = record.clone();
    injecting.extra.push(ReplayField {
        key: "future".to_owned(),
        value: format!("harmless\nenvelope.0={}", sha256(b"injected")),
    });
    assert!(
        matches!(
            injecting.validate(),
            Err(ReplayError::UnpreservableField { .. })
        ),
        "a preserved value with a newline is refused: {:?}",
        injecting.validate()
    );

    // A key the decoder already interprets would collide with it.
    for key in [
        "subject",
        "content",
        "input.0",
        "envelope.3",
        "choice.mission",
    ] {
        let mut colliding = record.clone();
        colliding.extra.push(ReplayField {
            key: key.to_owned(),
            value: "whatever".to_owned(),
        });
        assert!(
            matches!(
                colliding.validate(),
                Err(ReplayError::UnpreservableField { .. })
            ),
            "{key} must not be preservable"
        );
    }

    // A genuinely unknown key with a single-line value still round trips, so
    // the refusal is about injection and not about preservation.
    let mut preserving = record;
    preserving.extra.push(ReplayField {
        key: "future.field".to_owned(),
        value: "some-newer-value".to_owned(),
    });
    let decoded = decode(&encode(&preserving).expect("encode")).expect("decode");
    assert_eq!(decoded, preserving);
    assert!(decoded.promised.is_empty(), "nothing was injected");
}

/// A document that does not state its run purpose states nothing about it, so
/// the decoder refuses it rather than reading a capture or probe run as a
/// player's ordinary session — the one conclusion non-negotiable 4 has to be
/// able to refuse.
#[test]
fn accept_f59_a_a_document_that_omits_its_purpose_is_refused() {
    let record = fixture();
    for line in ["purpose=ordinary_play\n", "profile_write=0\n"] {
        let text = String::from_utf8(encode(&record).expect("encode")).expect("utf8");
        let stripped = text.replace(line, "");
        assert!(!stripped.contains(line), "the line was removed");
        let resealed = reseal_after(&stripped);
        assert!(
            matches!(
                decode(&resealed),
                Err(DecodeError::MissingField("purpose" | "profile_write"))
            ),
            "a document without `{line}` must be refused, not defaulted"
        );
    }
}

/// Re-seals a document body after a line has been removed, so the decoder
/// reaches the field rules rather than refusing the checksum.
fn reseal_after(text: &str) -> Vec<u8> {
    let checksum_start = text.rfind("checksum=").expect("a seal");
    let head = &text[..checksum_start];
    format!("{head}checksum={}\n", sha256(head.as_bytes()).to_hex()).into_bytes()
}

/// A repeated single-valued field is refused rather than silently taking the
/// last value.
#[test]
fn accept_f59_a_a_repeated_field_is_refused() {
    let record = fixture();
    let body = sealed_body_of(&encode(&record).expect("encode"));
    let doubled = reseal(&format!(
        "CSREPLAY 1.0\n{body}subject=mission/other\n{body}"
    ));
    assert_eq!(
        decode(&doubled),
        Err(DecodeError::RepeatedField("subject")),
        "a repeated field must be refused, not resolved by order"
    );
}

/// A record that breaks a rule is refused by `validate` and never encoded, so
/// a document on disk is always a record that means what it says.
#[test]
fn accept_f59_a_an_invalid_record_is_never_encoded() {
    let mut zero_rate = fixture();
    zero_rate.tick_rate = 0;
    assert_eq!(zero_rate.validate(), Err(ReplayError::ZeroTickRate));
    assert!(
        encode(&zero_rate).is_err(),
        "an invalid record is not encoded"
    );

    let mut inverted = fixture();
    inverted.first_tick = Tick(100);
    inverted.last_tick = Tick(1);
    assert!(matches!(
        inverted.validate(),
        Err(ReplayError::InvertedTickRange { .. })
    ));

    let mut foreign_schema = fixture();
    foreign_schema.schema = ReplayVersion { major: 2, minor: 0 };
    assert_eq!(
        foreign_schema.validate(),
        Err(ReplayError::UnreadableSchema { major: 2 })
    );

    let mut blank_toolchain = fixture();
    blank_toolchain.fingerprint.toolchain = "   ".to_owned();
    assert!(matches!(
        blank_toolchain.validate(),
        Err(ReplayError::Blank { field: "toolchain" })
    ));
}

/// Recorded input ticks may be sparse but must stay inside the run's range and
/// strictly increase.
#[test]
fn accept_f59_a_recorded_input_ticks_stay_inside_the_run_range() {
    let mut outside = fixture();
    outside.last_tick = Tick(1);
    assert!(
        matches!(
            outside.validate(),
            Err(ReplayError::OutsideTickRange {
                what: "an input record",
                ..
            })
        ),
        "a recorded tick past the run's end is refused"
    );

    // Sparse ticks are legal: a run that recorded nothing for ten ticks simply
    // has no records there, and a replay reproduces the pause.
    let record = fixture();
    assert!(
        (record.stream.records().len() as u64) < tick_range_len(&record),
        "the fixture's ticks are sparse"
    );
    assert!(record.validate().is_ok(), "sparseness is legal");
}

/// The production digest of a record's recorded stream.
fn input_digest(record: &ReplayRecord) -> InputStreamDigest {
    InputStreamDigest::of(&record.stream)
}

/// How many ticks the run's declared range spans.
fn tick_range_len(record: &ReplayRecord) -> u64 {
    record.last_tick.0 - record.first_tick.0 + 1
}

/// The production digest of a recorded stream is order-sensitive and
/// change-sensitive: two streams with the same commands on different ticks are
/// different recordings.
#[test]
fn accept_f59_a_the_input_stream_digest_is_tick_and_value_sensitive() {
    let record = fixture();
    let digest = InputStreamDigest::of(&record.stream);

    let mut moved = record.stream.clone();
    let first = moved.records()[0].clone();
    moved = CommandStream::new();
    moved.record_tick(first).expect("one tick");
    assert_ne!(
        InputStreamDigest::of(&moved),
        digest,
        "the same commands at a different tick are a different recording"
    );

    let altered = with_pitch_nudged(&record);
    assert_ne!(
        InputStreamDigest::of(&altered.stream),
        digest,
        "one quantization step changes the digest"
    );
}

/* ------------------------------------------------------------------ */
/* AC03: capture at a fixed tick from a fixed camera on two runs        */
/* ------------------------------------------------------------------ */

/// **AC03.** Two runs of the same replay captured at the same tick from the
/// same camera under the same settings produce the same capture digest, and a
/// difference in any pinned field is named rather than averaged over.
#[test]
fn accept_f59_a_two_runs_at_a_fixed_tick_and_camera_compare_equal() {
    let first = synthetic_capture_record();
    let second = synthetic_capture_record();
    assert_eq!(
        first.digest(),
        second.digest(),
        "the same replay, tick, camera and settings must capture identically"
    );
    assert!(first.same_capture(&second));
    assert!(first.differences_from(&second).is_empty());

    // Moving the tick, the camera, one render setting, the build or the
    // produced bytes is each its own reported difference.
    let mut moved_tick = second.clone();
    moved_tick.capture_tick = Tick(first.capture_tick.0 + 1);
    assert_eq!(
        first.differences_from(&moved_tick)[0],
        CaptureDifference::Tick {
            recorded: first.capture_tick,
            candidate: moved_tick.capture_tick,
        }
    );

    let mut moved_camera = second.clone();
    moved_camera.camera.eye = WorldPosition::try_new([9.0, 2.0, 3.0]).expect("finite");
    assert!(
        moved_camera
            .differences_from(&first)
            .contains(&CaptureDifference::Camera)
    );

    let mut shaded = second.clone();
    shaded.render.shadows = true;
    assert!(
        shaded
            .differences_from(&first)
            .contains(&CaptureDifference::Render)
    );
    assert!(
        !shaded.render.is_comparison_baseline(),
        "enabling shadows leaves the comparison baseline"
    );

    let mut other_build = second.clone();
    other_build.build = BuildId::new("3333333333333333333333333333333333333333").expect("valid");
    assert!(matches!(
        first.differences_from(&other_build)[0],
        CaptureDifference::Build { .. }
    ));

    let mut other_bytes = second.clone();
    other_bytes.artifact = sha256(b"cs.f59.different.bytes");
    assert!(matches!(
        first.differences_from(&other_bytes)[0],
        CaptureDifference::Bytes { .. }
    ));
}

/// A capture from another build, or of another replay, is refused by name
/// rather than accepted into a report.
#[test]
fn accept_f59_a_a_capture_from_another_build_or_replay_is_refused() {
    let record = fixture();
    let capture = synthetic_capture_record();
    assert!(
        capture.validate_against(&record).is_ok(),
        "the fixture capture belongs to the fixture replay"
    );

    let mut other_build = capture.clone();
    other_build.build = BuildId::new("4444444444444444444444444444444444444444").expect("valid");
    assert!(matches!(
        other_build.validate_against(&record),
        Err(CaptureError::BuildMismatch { .. })
    ));

    let mut other_replay = capture.clone();
    other_replay.replay_signature = sha256(b"cs.f59.other.replay");
    assert!(matches!(
        other_replay.validate_against(&record),
        Err(CaptureError::ReplayMismatch { .. })
    ));
}

/// A degenerate camera or an out-of-range render setting is refused, and a
/// non-finite pose component never becomes a valid record.
#[test]
fn accept_f59_a_a_degenerate_camera_or_render_setting_is_refused() {
    let mut fov = synthetic_capture_record();
    fov.camera.vertical_fov = Radians(0.0);
    assert!(matches!(
        fov.validate(),
        Err(CaptureError::FieldOfView { .. })
    ));
    fov.camera.vertical_fov = Radians(std::f64::consts::PI);
    assert!(matches!(
        fov.validate(),
        Err(CaptureError::FieldOfView { .. })
    ));
    fov.camera.vertical_fov = Radians(f64::NAN);
    assert!(matches!(
        fov.validate(),
        Err(CaptureError::NonFinite {
            field: "vertical_fov"
        })
    ));

    let mut zero_width = synthetic_capture_record();
    zero_width.render.width = 0;
    assert!(matches!(
        zero_width.validate(),
        Err(CaptureError::RenderDimension { field: "width", .. })
    ));

    let mut heavy = synthetic_capture_record();
    heavy.render.msaa_samples = 0;
    assert!(matches!(
        heavy.validate(),
        Err(CaptureError::RenderRange {
            field: "msaa_samples",
            ..
        })
    ));
    heavy.render.msaa_samples = MAX_MSAA_SAMPLES + 1;
    assert!(heavy.validate().is_err());
}

/// A capture record survives its document form, and a PCM capture must state
/// its role rather than leaving the capability claim to be inferred.
#[test]
fn accept_f59_a_a_capture_document_round_trips_and_states_its_pcm_role() {
    let record = synthetic_capture_record();
    let bytes = encode_capture(&record).expect("encode");
    let decoded = decode_capture(&bytes).expect("decode");
    assert_eq!(decoded.digest(), record.digest());
    assert_eq!(decoded.render, record.render);
    assert_eq!(decoded.camera.mode, record.camera.mode);
    assert_eq!(decoded.camera.eye, record.camera.eye);
    assert_eq!(
        decoded.camera.vertical_fov.0.to_bits(),
        record.camera.vertical_fov.0.to_bits(),
        "the field of view survives bit-exactly"
    );

    let mut audible = record.clone();
    audible.media = ArtifactMedia::PcmCapture(PcmRole::AudiblePlayback);
    let text = String::from_utf8(encode_capture(&audible).expect("encode")).expect("utf8");
    assert!(text.contains("pcm.role=audible_playback"), "{text}");
    let decoded = decode_capture(text.as_bytes()).expect("decode");
    assert_eq!(
        decoded.media,
        ArtifactMedia::PcmCapture(PcmRole::AudiblePlayback),
        "the pcm role survives"
    );

    // The same document without the role line is refused.
    let stripped = text.replace("pcm.role=audible_playback\n", "");
    assert!(
        matches!(
            decode_capture(stripped.as_bytes()),
            Err(DecodeError::ChecksumMismatch)
        ),
        "a stripped body fails the seal before it can be under-specified"
    );
}

/// A capture document is bounded and sealed like a replay document.
#[test]
fn accept_f59_a_a_capture_document_is_bounded_and_sealed() {
    let record = synthetic_capture_record();
    let text = String::from_utf8(encode_capture(&record).expect("encode")).expect("utf8");
    assert!(text.starts_with("CSCAPTURE 1.0\n"), "{text}");
    assert!(
        decode_capture(text.replace("bytes=1024", "bytes=9999").as_bytes())
            .is_err_and(|error| matches!(error, DecodeError::ChecksumMismatch))
    );
    assert!(matches!(
        decode_capture(text.replace("CSCAPTURE 1.0", "CSCAPTURE 3.0").as_bytes()),
        Err(DecodeError::UnsupportedMajor { major: 3 })
    ));
    assert!(matches!(
        decode_capture(
            text.replace("camera.mode=external", "camera.mode=telescope")
                .as_bytes()
        ),
        Err(DecodeError::ChecksumMismatch)
    ));
}

/* ------------------------------------------------------------------ */
/* AC04: a headless-only machine cannot report audio or visual proof    */
/* ------------------------------------------------------------------ */

/// **AC04.** On a machine that declares only `synthetic`, an evidence bundle
/// whose artifacts are a screenshot and an audible PCM capture is *blocked*,
/// never certified, and the missing capabilities are named.
#[test]
fn accept_f59_a_a_headless_machine_cannot_certify_visual_or_audible_evidence() {
    let headless = DeclaredCapabilities::headless_synthetic();
    assert!(headless.is_headless());
    assert!(!headless.contains(CapabilityClass::Gpu));
    assert!(!headless.contains(CapabilityClass::Audio));
    assert!(!headless.contains(CapabilityClass::HumanReview));

    let bundle = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![
            artifact(
                "claim.visual",
                "screenshot.png",
                ArtifactMedia::Screenshot,
                sha256(b"png"),
            ),
            artifact(
                "claim.audible",
                "clip.wav",
                ArtifactMedia::PcmCapture(PcmRole::AudiblePlayback),
                sha256(b"wav"),
            ),
        ],
    );
    let report = bundle.certify(&candidate_of(&fixture()));
    assert!(
        !report.is_checked(),
        "a headless machine must not certify visual or audible evidence"
    );
    assert_eq!(report.certification(), Certification::Blocked { gaps: 3 });
    let gaps: Vec<(&str, CapabilityClass)> = report
        .blocked
        .iter()
        .map(|gap| (gap.claim.as_str(), gap.required))
        .collect();
    assert_eq!(
        gaps,
        [
            ("claim.visual", CapabilityClass::Gpu),
            ("claim.audible", CapabilityClass::Audio),
            ("claim.audible", CapabilityClass::HumanReview),
        ],
        "each missing capability is named: {gaps:?}"
    );
    assert!(report.certified.is_empty(), "nothing is certified");

    // A decoded PCM capture needs no capability: it is a decode artifact and
    // not audible review, and saying so is what keeps it from being claimed as
    // audible evidence. The same machine certifies it.
    let decode_only = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![artifact(
            "claim.decode",
            "decoded.wav",
            ArtifactMedia::PcmCapture(PcmRole::DecodeOnly),
            sha256(b"decoded"),
        )],
    );
    let decode_report = decode_only.certify(&candidate_of(&fixture()));
    assert!(
        decode_report.blocked.is_empty(),
        "a decode-only pcm capture needs no device: {:?}",
        decode_report.diagnostic_lines()
    );
    assert!(
        decode_report.is_checked(),
        "{:?}",
        decode_report.diagnostic_lines()
    );
    let limitations = decode_only.artifacts[0].limitations();
    assert!(
        limitations
            .iter()
            .any(|limitation| limitation.contains("decode artifact")),
        "the limitation travels with the artifact: {limitations:?}"
    );

    // A headless trace is likewise not blocked: simulation is not a
    // capability-gated activity.
    let trace_only = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![artifact(
            "claim.trace",
            "trace.jsonl",
            ArtifactMedia::Trace,
            sha256(b"trace"),
        )],
    );
    let trace_report = trace_only.certify(&candidate_of(&fixture()));
    assert!(trace_report.blocked.is_empty());
    assert!(trace_report.is_checked());
}

/// The capability table is a table: each media declares what it needs, and a
/// machine declaring the capability is no longer blocked for it.
#[test]
fn accept_f59_a_the_capability_table_is_explicit_per_media() {
    assert_eq!(ArtifactMedia::Trace.required_capabilities(), &[]);
    assert_eq!(ArtifactMedia::Report.required_capabilities(), &[]);
    assert_eq!(
        ArtifactMedia::Screenshot.required_capabilities(),
        &[CapabilityClass::Gpu]
    );
    assert_eq!(
        ArtifactMedia::PcmCapture(PcmRole::DecodeOnly).required_capabilities(),
        &[]
    );
    assert_eq!(
        ArtifactMedia::PcmCapture(PcmRole::AudiblePlayback).required_capabilities(),
        &[CapabilityClass::Audio, CapabilityClass::HumanReview]
    );

    // Adding the capability removes the block, so the refusal really is about
    // the capability and not about the artifact.
    let with_gpu = bundle_of(
        DeclaredCapabilities::of([CapabilityClass::Synthetic, CapabilityClass::Gpu]),
        vec![artifact(
            "claim.visual",
            "screenshot.png",
            ArtifactMedia::Screenshot,
            sha256(b"png"),
        )],
    );
    let report = with_gpu.certify(&candidate_of(&fixture()));
    assert!(
        report.blocked.is_empty(),
        "with a gpu declared, a screenshot is not blocked: {:?}",
        report.diagnostic_lines()
    );
    assert_eq!(
        report.certification(),
        Certification::Checked {
            claims: vec![claim("claim.visual")]
        }
    );

    // Audible playback still needs a human reviewer, so an `audio` declaration
    // alone is not enough on an agent's machine.
    let with_audio = bundle_of(
        DeclaredCapabilities::of([CapabilityClass::Synthetic, CapabilityClass::Audio]),
        vec![artifact(
            "claim.audible",
            "clip.wav",
            ArtifactMedia::PcmCapture(PcmRole::AudiblePlayback),
            sha256(b"wav"),
        )],
    );
    let report = with_audio.certify(&candidate_of(&fixture()));
    assert_eq!(
        report.blocked,
        vec![cs_content::replay::CapabilityGap {
            claim: claim("claim.audible"),
            required: CapabilityClass::HumanReview,
        }],
        "an audio device is not a human reviewer: {:?}",
        report.diagnostic_lines()
    );
}

/// A bundle carrying exactly the given artifacts on a machine with exactly the
/// given capabilities, fresh for the fixture build, passing, and an ordinary
/// play run. Every capability test starts from this so its conclusion is about
/// the one thing it changes.
fn bundle_of(
    capabilities: DeclaredCapabilities,
    artifacts: Vec<EvidenceArtifact>,
) -> EvidenceBundle {
    EvidenceBundle {
        artifacts,
        capabilities,
        ..synthetic_evidence_bundle()
    }
}

/// The declared-capability list is parsed from the contract's spelling, and an
/// unknown or blank entry is an error rather than a quiet omission.
#[test]
fn accept_f59_a_capability_lists_parse_and_refuse_unknown_names() {
    let parsed = DeclaredCapabilities::parse("retail, gpu ,audio").expect("valid list");
    assert!(parsed.contains(CapabilityClass::Retail));
    assert!(parsed.contains(CapabilityClass::Gpu));
    assert!(parsed.contains(CapabilityClass::Audio));
    assert!(!parsed.is_headless());
    assert_eq!(parsed.label(), "retail,gpu,audio");

    assert_eq!(
        DeclaredCapabilities::parse("   ").expect("empty list"),
        DeclaredCapabilities::none(),
        "an unset variable is an empty declaration, not an error"
    );
    assert!(
        DeclaredCapabilities::parse("retail,,gpu").is_err(),
        "blank element"
    );
    assert!(
        DeclaredCapabilities::parse("retail,teleport").is_err(),
        "an unknown capability must not be silently dropped"
    );

    // A human capability is one an agent never has, and the label round-trips.
    for class in CapabilityClass::ALL {
        assert_eq!(CapabilityClass::from_label(class.label()), Some(*class));
    }
    assert_eq!(
        DeclaredCapabilities::parse("human_review").expect("valid"),
        DeclaredCapabilities::of([CapabilityClass::HumanReview])
    );
}

/* ------------------------------------------------------------------ */
/* Non-negotiable 2: runtime-produced bytes only                        */
/* ------------------------------------------------------------------ */

/// An artifact whose bytes were authored cannot back a claim, however it is
/// labelled or fingerprinted: the origin decides the observation method, and
/// the F01 check refuses it.
#[test]
fn accept_f59_a_an_authored_artifact_never_backs_an_original_claim() {
    let install = Fingerprint {
        kind: FingerprintKind::Installation,
        sha256: sha256(b"cs.f59.original.installation"),
    };
    let authored = EvidenceArtifact::new(ArtifactDescription {
        claim: &claim("f59.a.drawn-image"),
        task: "F59-A",
        path: "screenshot.png",
        sha256: sha256(b"drawn-by-hand"),
        media: ArtifactMedia::Screenshot,
        origin: ArtifactOrigin::Authored,
        fingerprint: install,
        locator: "mission/screenshot.png",
    })
    .expect("a private-relative path is valid");

    let record = authored.evidence_record("cs", "0.1.0");
    assert!(
        !record.verifies_original(),
        "a hand-drawn image attached to a fingerprinted installation must not verify"
    );
    assert_eq!(
        record.method,
        ObservationMethod::Authored,
        "the origin decides the method"
    );
    assert!(
        record
            .limitations
            .iter()
            .any(|limitation| limitation.contains("authored")),
        "the limitation travels with the record: {:?}",
        record.limitations
    );

    // The same artifact marked runtime-produced does verify, so the check is
    // about the origin and not about the path or the digest.
    let mut runtime = authored.clone();
    runtime.origin = ArtifactOrigin::Runtime;
    assert!(
        runtime.evidence_record("cs", "0.1.0").verifies_original(),
        "runtime-produced bytes from a fingerprinted installation do verify"
    );
}

/// A bundle carrying an authored artifact is refused, and the claim it was
/// attached to is not certified — while a runtime artifact on the same machine
/// is.
#[test]
fn accept_f59_a_a_bundle_with_an_authored_artifact_refuses_that_claim() {
    let trace = artifact(
        "claim.trace",
        "trace.jsonl",
        ArtifactMedia::Trace,
        sha256(b"trace"),
    );
    let mut drawn = artifact(
        "claim.drawn",
        "screenshot.png",
        ArtifactMedia::Screenshot,
        sha256(b"drawn"),
    );
    drawn.origin = ArtifactOrigin::Authored;
    // A gpu is declared so the *only* problem left is the authored origin.
    let bundle = bundle_of(
        DeclaredCapabilities::of([CapabilityClass::Synthetic, CapabilityClass::Gpu]),
        vec![trace, drawn],
    );
    let report = bundle.certify(&candidate_of(&fixture()));
    assert!(!report.is_checked());
    assert!(
        report.blocked.is_empty(),
        "the capability is declared, so the refusal is about the origin: {:?}",
        report.diagnostic_lines()
    );
    assert_eq!(
        report.certification(),
        Certification::Refused { reasons: 1 }
    );
    assert!(
        report
            .refused
            .iter()
            .any(|refusal| matches!(refusal, EvidenceRefusal::AuthoredArtifact { .. })),
        "an authored artifact is a refusal: {:?}",
        report.diagnostic_lines()
    );
    // The runtime trace on the same machine and run is still certified; the
    // authored screenshot is not, and the two claims are kept apart.
    assert!(
        report.certified.contains(&claim("claim.trace")),
        "{:?}",
        report.certified
    );
    assert!(!report.certified.contains(&claim("claim.drawn")));

    // Marking it runtime removes the refusal, so the refusal was about the
    // origin and about nothing else.
    let mut relabelled = bundle.clone();
    for artifact in &mut relabelled.artifacts {
        artifact.origin = ArtifactOrigin::Runtime;
    }
    let report = relabelled.certify(&candidate_of(&fixture()));
    assert!(report.is_checked(), "{:?}", report.diagnostic_lines());
    assert_eq!(
        report.certified,
        vec![claim("claim.drawn"), claim("claim.trace")]
    );
}

fn artifact(
    claim_id: &str,
    path: &str,
    media: ArtifactMedia,
    digest: ContentHash,
) -> EvidenceArtifact {
    runtime_artifact(claim(claim_id), path, media, digest)
}

fn runtime_artifact(
    claim: ClaimId,
    path: &str,
    media: ArtifactMedia,
    digest: ContentHash,
) -> EvidenceArtifact {
    EvidenceArtifact::new(ArtifactDescription {
        claim: &claim,
        task: "F59-A",
        path,
        sha256: digest,
        media,
        origin: ArtifactOrigin::Runtime,
        fingerprint: Fingerprint {
            kind: FingerprintKind::Content,
            sha256: sha256(b"cs.f59.test.content"),
        },
        locator: "synthetic.artifact",
    })
    .expect("a valid artifact link")
}

/// An artifact path that is absolute or escapes the private directory is
/// refused, so a report cannot carry original data into the repository by
/// naming it.
#[test]
fn accept_f59_a_an_artifact_path_must_stay_inside_the_private_directory() {
    let fingerprint = Fingerprint {
        kind: FingerprintKind::Content,
        sha256: sha256(b"content"),
    };
    for path in [
        "/home/owner/game/mission.dat",
        "C:\\games\\crimson.dat",
        "../outside/trace.jsonl",
        "nested/../../escape.jsonl",
    ] {
        let result = EvidenceArtifact::new(ArtifactDescription {
            claim: &claim("f59.a.path"),
            task: "F59-A",
            path,
            sha256: sha256(b"bytes"),
            media: ArtifactMedia::Trace,
            origin: ArtifactOrigin::Runtime,
            fingerprint,
            locator: "synthetic.trace",
        });
        assert!(
            matches!(
                result,
                Err(ReplayError::Syntax {
                    field: "artifact path",
                    ..
                })
            ),
            "{path} must be refused"
        );
    }
    // A private-relative path is accepted.
    assert!(
        EvidenceArtifact::new(ArtifactDescription {
            claim: &claim("f59.a.path"),
            task: "F59-A",
            path: "artifacts/trace.jsonl",
            sha256: sha256(b"bytes"),
            media: ArtifactMedia::Trace,
            origin: ArtifactOrigin::Runtime,
            fingerprint,
            locator: "synthetic.trace",
        })
        .is_ok()
    );
}

/* ------------------------------------------------------------------ */
/* Non-negotiable 4: overrides invalidate ordinary play                 */
/* ------------------------------------------------------------------ */

/// A capture or probe run is not ordinary play, and a named debug override
/// removes the ordinary-play claim even from an ordinary-looking run.
#[test]
fn accept_f59_a_debug_overrides_invalidate_ordinary_play_proof() {
    let ordinary = OverrideLog::ordinary_play();
    assert!(!ordinary.invalidates_ordinary_play());
    assert!(!ordinary.violates_profile_rule());

    let capture = OverrideLog::new(RunPurpose::Capture, Vec::new(), false).expect("valid");
    assert!(
        capture.invalidates_ordinary_play(),
        "a capture run is not ordinary play"
    );
    assert!(
        !capture.violates_profile_rule(),
        "a capture run that wrote no production profile is fine"
    );

    let with_override = OverrideLog::new(
        RunPurpose::OrdinaryPlay,
        vec![OverrideEntry {
            name: "debug_overrides".to_owned(),
            detail: "invulnerable=1".to_owned(),
        }],
        false,
    )
    .expect("valid");
    assert!(
        with_override.invalidates_ordinary_play(),
        "a debug override removes the ordinary-play claim"
    );

    // A capture run that wrote a production profile without asking breaks the
    // profile rule, whatever else it did.
    let writing = OverrideLog::new(RunPurpose::Capture, Vec::new(), true).expect("valid");
    assert!(writing.violates_profile_rule());
    assert!(
        writing.production_profile_write(),
        "the write is recorded either way"
    );
}

/// A bundle whose run broke the profile rule is refused, and one that merely
/// had a debug override is not ordinary play — neither is a pass.
#[test]
fn accept_f59_a_a_capture_run_that_wrote_a_production_profile_is_refused() {
    let mut bundle = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![artifact(
            "claim.trace",
            "trace.jsonl",
            ArtifactMedia::Trace,
            sha256(b"trace"),
        )],
    );
    bundle.overrides = OverrideLog::new(RunPurpose::Capture, Vec::new(), true).expect("valid");
    let report = bundle.certify(&candidate_of(&fixture()));
    assert!(!report.is_checked());
    assert!(
        report
            .refused
            .iter()
            .any(|refusal| matches!(refusal, EvidenceRefusal::ProfileWriteDuringCapture { .. })),
        "{:?}",
        report.diagnostic_lines()
    );
    assert_eq!(
        report.certification(),
        Certification::Refused { reasons: 1 }
    );

    // The same run that did not write a production profile is not refused for
    // that, so the refusal is about the write.
    let mut quiet = bundle.clone();
    quiet.overrides = OverrideLog::new(RunPurpose::Capture, Vec::new(), false).expect("valid");
    let report = quiet.certify(&candidate_of(&fixture()));
    assert!(report.refused.is_empty(), "{:?}", report.diagnostic_lines());
    assert!(
        !report.ordinary_play_refused.is_empty(),
        "a capture run is not ordinary play: {:?}",
        report.diagnostic_lines()
    );
    assert_eq!(report.certification(), Certification::NotOrdinaryPlay);

    // An ordinary play run with a named debug override is likewise not
    // ordinary play, but is not a profile-rule violation.
    let mut overridden = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![artifact(
            "claim.trace",
            "trace.jsonl",
            ArtifactMedia::Trace,
            sha256(b"trace"),
        )],
    );
    overridden.overrides = OverrideLog::new(
        RunPurpose::OrdinaryPlay,
        vec![OverrideEntry {
            name: "debug_overrides".to_owned(),
            detail: "godmode=1".to_owned(),
        }],
        false,
    )
    .expect("valid");
    let report = overridden.certify(&candidate_of(&fixture()));
    assert!(
        report.refused.is_empty(),
        "an override is not a profile write"
    );
    assert_eq!(report.certification(), Certification::NotOrdinaryPlay);
    assert!(
        report.ordinary_play_refused[0]
            .to_string()
            .contains("debug_overrides"),
        "the override is named: {:?}",
        report.diagnostic_lines()
    );
}

/// A duplicate override name is refused, and the log is canonical.
#[test]
fn accept_f59_a_duplicate_override_names_are_refused() {
    let result = OverrideLog::new(
        RunPurpose::Capture,
        vec![
            OverrideEntry {
                name: "same".to_owned(),
                detail: "a".to_owned(),
            },
            OverrideEntry {
                name: "same".to_owned(),
                detail: "b".to_owned(),
            },
        ],
        false,
    );
    assert!(matches!(
        result,
        Err(ReplayError::Duplicate {
            what: "override",
            ..
        })
    ));
}

/* ------------------------------------------------------------------ */
/* Non-negotiable 5: a stale report cannot certify a new build          */
/* ------------------------------------------------------------------ */

/// A report made on a different tree or against different content is stale and
/// certifies nothing, and only removing both staleness reasons lets it
/// certify.
#[test]
fn accept_f59_a_a_stale_report_cannot_certify_a_new_build() {
    let record = fixture();
    let clean = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![artifact(
            "claim.trace",
            "trace.jsonl",
            ArtifactMedia::Trace,
            sha256(b"trace"),
        )],
    );

    // Fresh and passing: checked.
    let fresh = clean.certify(&candidate_of(&record));
    assert!(fresh.is_checked(), "{:?}", fresh.diagnostic_lines());

    // A different tree is stale.
    let other_tree = clean.certify(&CandidateBuild {
        tree: BuildId::new("5555555555555555555555555555555555555555").expect("valid"),
        content_sha256: clean.content_sha256,
    });
    assert!(!other_tree.is_checked());
    assert_eq!(
        other_tree.certification(),
        Certification::Stale { reasons: 1 }
    );
    assert!(matches!(
        other_tree.stale[0],
        StaleReason::TreeChanged { .. }
    ));

    // Different content is stale too, and the two reasons add up.
    let other_content = clean.certify(&CandidateBuild {
        tree: clean.tree.clone(),
        content_sha256: sha256(b"cs.f59.newer.content"),
    });
    assert_eq!(other_content.stale.len(), 1);
    assert!(matches!(
        other_content.stale[0],
        StaleReason::ContentChanged { .. }
    ));

    // A run that declares retail access but names no installation digest
    // cannot be checked against an installation at all.
    let mut retail = clean.clone();
    retail.capabilities =
        DeclaredCapabilities::of([CapabilityClass::Synthetic, CapabilityClass::Retail]);
    retail.install_sha256 = None;
    let report = retail.certify(&candidate_of(&record));
    assert!(!report.is_checked());
    assert!(
        report
            .stale
            .iter()
            .any(|reason| matches!(reason, StaleReason::RetailWithoutInstallationHash)),
        "{:?}",
        report.diagnostic_lines()
    );
    retail.install_sha256 = Some(sha256(b"cs.f59.installation"));
    assert!(
        retail
            .certify(&candidate_of(&record))
            .stale
            .iter()
            .all(|reason| !matches!(reason, StaleReason::RetailWithoutInstallationHash)),
        "naming the installation digest removes that reason"
    );
}

/// A run that did not pass is reported as not passing, before anything else is
/// examined, and a run with no tests at all is not a pass either.
#[test]
fn accept_f59_a_a_run_that_did_not_pass_is_never_a_pass() {
    let mut bundle = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![artifact(
            "claim.trace",
            "trace.jsonl",
            ArtifactMedia::Trace,
            sha256(b"trace"),
        )],
    );
    bundle.counts = TestCounts {
        passed: 4,
        failed: 1,
        assertions: 9,
    };
    let report = bundle.certify(&candidate_of(&fixture()));
    assert!(!report.is_checked());
    assert_eq!(
        report.certification(),
        Certification::NotPassed {
            passed: 4,
            failed: 1
        }
    );

    bundle.counts = TestCounts {
        passed: 0,
        failed: 0,
        assertions: 0,
    };
    let report = bundle.certify(&candidate_of(&fixture()));
    assert!(
        !report.is_checked(),
        "a run that executed nothing is not a pass"
    );
    assert_eq!(
        report.certification(),
        Certification::NotPassed {
            passed: 0,
            failed: 0
        }
    );
}

/// A report that does not carry the contract's record minimum certifies
/// nothing: without a task id, a tool or a test command a reader cannot tell
/// what was run, so the report is refused rather than checked.
#[test]
fn accept_f59_a_a_report_without_the_record_minimum_is_refused() {
    let base = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![artifact(
            "claim.trace",
            "trace.jsonl",
            ArtifactMedia::Trace,
            sha256(b"trace"),
        )],
    );
    assert!(
        base.certify(&candidate_of(&fixture())).is_checked(),
        "the complete bundle is checked"
    );

    for (field, blank) in [
        ("test command", BundleField::TestCommand),
        ("tool", BundleField::Tool),
        ("tool version", BundleField::ToolVersion),
    ] {
        let mut incomplete = base.clone();
        blank.clear(&mut incomplete);
        assert!(
            incomplete.validate().is_err(),
            "{field}: the bundle's own validate refuses it"
        );
        let report = incomplete.certify(&candidate_of(&fixture()));
        assert!(
            !report.is_checked(),
            "{field}: a report with no {field} is not a pass: {:?}",
            report.diagnostic_lines()
        );
        assert!(
            report
                .refused
                .iter()
                .any(|refusal| matches!(refusal, EvidenceRefusal::IncompleteRecord { .. })),
            "{field}: the refusal names the missing field: {:?}",
            report.diagnostic_lines()
        );
        assert_eq!(
            report.certification(),
            Certification::Refused { reasons: 1 },
            "{field}: one problem, one reason"
        );
    }

    // An oversized task id is refused on the same grounds, and an unresolved
    // issue is not: the contract requires unresolved issues to survive, so
    // naming one may never remove a claim.
    let mut padded = base.clone();
    padded.task = "F".repeat(200);
    assert!(!padded.certify(&candidate_of(&fixture())).is_checked());
    let mut unresolved = base;
    unresolved
        .unresolved
        .push("a product limitation is still open".to_owned());
    let report = unresolved.certify(&candidate_of(&fixture()));
    assert!(
        report.is_checked(),
        "an unresolved issue is reported, not refused: {:?}",
        report.diagnostic_lines()
    );
}

/// Which of a bundle's record-minimum fields a test blanks.
enum BundleField {
    TestCommand,
    Tool,
    ToolVersion,
}

impl BundleField {
    fn clear(self, bundle: &mut EvidenceBundle) {
        match self {
            Self::TestCommand => bundle.test_command = "   ".to_owned(),
            Self::Tool => bundle.tool = String::new(),
            Self::ToolVersion => bundle.tool_version = String::new(),
        }
    }
}

/// The report names the claims it certifies, and nothing is certified when
/// there is no artifact to certify.
#[test]
fn accept_f59_a_a_bundle_certifies_exactly_the_claims_it_can() {
    let bundle = bundle_of(
        DeclaredCapabilities::headless_synthetic(),
        vec![
            artifact(
                "claim.one",
                "one.jsonl",
                ArtifactMedia::Trace,
                sha256(b"one"),
            ),
            artifact(
                "claim.two",
                "two.jsonl",
                ArtifactMedia::Report,
                sha256(b"two"),
            ),
        ],
    );
    let report = bundle.certify(&candidate_of(&fixture()));
    assert_eq!(
        report.certified,
        vec![claim("claim.one"), claim("claim.two")]
    );
    assert!(report.is_checked());
    let lines = report.diagnostic_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("certified: claim claim.one"))
    );
    assert!(lines[0].contains("certification: checked"), "{lines:?}");

    // An artifact from another task is refused, so a bundle cannot borrow
    // another task's evidence.
    let mut foreign = bundle.clone();
    foreign.artifacts.push(
        EvidenceArtifact::new(ArtifactDescription {
            claim: &claim("claim.foreign"),
            task: "F59-B",
            path: "three.jsonl",
            sha256: sha256(b"three"),
            media: ArtifactMedia::Trace,
            origin: ArtifactOrigin::Runtime,
            fingerprint: Fingerprint {
                kind: FingerprintKind::Content,
                sha256: sha256(b"content"),
            },
            locator: "synthetic.trace",
        })
        .expect("valid"),
    );
    let report = foreign.certify(&candidate_of(&fixture()));
    assert!(!report.is_checked());
    assert!(
        report
            .refused
            .iter()
            .any(|refusal| matches!(refusal, EvidenceRefusal::ForeignTask { .. })),
        "{:?}",
        report.diagnostic_lines()
    );
    assert!(!report.certified.contains(&claim("claim.foreign")));

    // Nothing to certify is its own state, not a silent pass.
    let mut empty = bundle.clone();
    empty.artifacts.clear();
    let report = empty.certify(&candidate_of(&fixture()));
    assert!(!report.is_checked());
    assert_eq!(report.certification(), Certification::NoClaims);
}

/* ------------------------------------------------------------------ */
/* Authored choices, provenance and the record's own bounds             */
/* ------------------------------------------------------------------ */

/// A pinned authored choice needs a slot, a value and a provenance, and a
/// repeated slot is refused — a replay may not silently assume a difficulty.
#[test]
fn accept_f59_a_authored_choices_are_pinned_with_provenance() {
    let provenance = Provenance::designed(claim("f59.a.designed.choice"));
    let choices = AuthoredChoices::new(vec![
        AuthoredChoice {
            slot: ChoiceSlot::Difficulty,
            value: "ace".to_owned(),
            provenance: provenance.clone(),
        },
        AuthoredChoice {
            slot: ChoiceSlot::Airframe,
            value: "synthetic.fighter".to_owned(),
            provenance,
        },
    ])
    .expect("valid choices");
    // Canonical order, not insertion order.
    assert_eq!(choices.choices()[0].slot, ChoiceSlot::Airframe);
    assert_eq!(choices.get(ChoiceSlot::Difficulty), Some("ace"));
    assert!(choices.get(ChoiceSlot::Loadout).is_none());
    assert!(!choices.is_empty());
    assert_eq!(choices.len(), 2);

    let duplicate = AuthoredChoices::new(vec![
        AuthoredChoice {
            slot: ChoiceSlot::Assists,
            value: "a".to_owned(),
            provenance: Provenance::designed(claim("f59.a.d")),
        },
        AuthoredChoice {
            slot: ChoiceSlot::Assists,
            value: "b".to_owned(),
            provenance: Provenance::designed(claim("f59.a.d")),
        },
    ]);
    assert!(matches!(
        duplicate,
        Err(ReplayError::Duplicate {
            what: "authored choice",
            ..
        })
    ));

    let blank = AuthoredChoices::new(vec![AuthoredChoice {
        slot: ChoiceSlot::Mission,
        value: "  ".to_owned(),
        provenance: Provenance::designed(claim("f59.a.d")),
    }]);
    assert!(matches!(
        blank,
        Err(ReplayError::Blank {
            field: "choice value"
        })
    ));
}

/// The same value with a different provenance class is a different choice, so a
/// replay cannot launder an inferred value into a designed one.
#[test]
fn accept_f59_a_choice_provenance_is_part_of_the_choices_identity() {
    let designed = AuthoredChoices::new(vec![AuthoredChoice {
        slot: ChoiceSlot::Difficulty,
        value: "ace".to_owned(),
        provenance: Provenance::designed(claim("f59.a.p")),
    }])
    .expect("valid");
    let inferred = AuthoredChoices::new(vec![AuthoredChoice {
        slot: ChoiceSlot::Difficulty,
        value: "ace".to_owned(),
        provenance: Provenance {
            claim_id: claim("f59.a.p"),
            class: ClaimStatus::Inferred,
            source: None,
        },
    }])
    .expect("valid");
    assert_ne!(
        designed.canonical(),
        inferred.canonical(),
        "the provenance class is part of the canonical choice text"
    );
    assert_eq!(
        designed.get(ChoiceSlot::Difficulty),
        inferred.get(ChoiceSlot::Difficulty),
        "the value text is the same; the provenance class is what differs"
    );
    assert!(designed.canonical().contains(ClaimStatus::Designed.label()));
    assert!(inferred.canonical().contains(ClaimStatus::Inferred.label()));
    // ... and the two are not equal records, so a replay built from one is not
    // the same run as the other.
    assert_ne!(designed, inferred);
}

/// Platform tags and build ids are validated, so a comparison of two tags is
/// about real values.
#[test]
fn accept_f59_a_platform_tags_and_build_ids_are_validated() {
    let tag = PlatformTag::new("macos", "aarch64").expect("valid");
    assert_eq!(tag.os(), "macos");
    assert_eq!(tag.arch(), "aarch64");
    assert_eq!(tag.to_string(), "macos-aarch64");
    assert!(tag.same_as(&PlatformTag::new("macos", "aarch64").expect("valid")));
    assert!(!tag.same_as(&PlatformTag::new("macos", "x86_64").expect("valid")));
    assert!(!tag.same_as(&PlatformTag::new("linux", "aarch64").expect("valid")));
    assert!(matches!(
        PlatformTag::new("", "aarch64"),
        Err(ReplayError::Blank {
            field: "platform os"
        })
    ));
    assert!(matches!(
        PlatformTag::new("mac os", "aarch64"),
        Err(ReplayError::Syntax { .. })
    ));

    assert!(BuildId::new("abc").is_err(), "a short id is refused");
    assert!(BuildId::new(&"a".repeat(40)).is_ok());
    assert!(BuildId::new(&"a".repeat(64)).is_ok());
    assert!(
        BuildId::new(&"a".repeat(41)).is_err(),
        "a 41-digit id is refused"
    );
    assert!(
        BuildId::new(&"A".repeat(40)).is_err(),
        "uppercase is refused so two spellings cannot compare unequal"
    );
    assert!(BuildId::new(&"g".repeat(40)).is_err(), "non-hex is refused");
}

/// An initial state is a label and a digest; a blank label is refused, and the
/// digest is load-bearing for the run's identity.
#[test]
fn accept_f59_a_an_initial_state_needs_a_label_and_a_digest() {
    assert!(InitialState::new("synthetic.box@tick0", sha256(b"state")).is_ok());
    assert!(matches!(
        InitialState::new("   ", sha256(b"state")),
        Err(ReplayError::Blank {
            field: "initial state label"
        })
    ));
    let record = fixture();
    let mut changed = record.clone();
    changed.initial_state =
        InitialState::new("synthetic.box@tick1", sha256(b"other")).expect("valid label");
    assert_ne!(
        record.compatibility_signature(),
        changed.compatibility_signature(),
        "a different initial state is a different run"
    );
}

/// A build fingerprint carries three separate digests, so a report can say
/// which of engine, content and rules moved.
#[test]
fn accept_f59_a_the_build_fingerprint_separates_engine_content_and_rules() {
    let record = fixture();
    let fingerprint: &BuildFingerprint = &record.fingerprint;
    assert_ne!(fingerprint.engine, fingerprint.content);
    assert_ne!(fingerprint.content, fingerprint.rules);
    assert_ne!(fingerprint.engine, fingerprint.rules);
    assert!(fingerprint.validate().is_ok());

    for (label, mutate) in [("engine", 0usize), ("content", 1), ("rules", 2)] {
        let mut changed = record.clone();
        let digest = sha256(label.as_bytes());
        match mutate {
            0 => changed.fingerprint.engine = digest,
            1 => changed.fingerprint.content = digest,
            _ => changed.fingerprint.rules = digest,
        }
        let verdict = record.compatibility_with(&changed, CrossBuildPolicy::Reject);
        assert_eq!(verdict.differences()[0].label(), label);
    }
}

/// The record is the sheet's deliverable line, field by field: an empty one
/// built field by field is refused rather than accepted as a run.
#[test]
fn accept_f59_a_a_record_carries_every_field_the_sheet_names() {
    let record = fixture();
    record.validate().expect("the fixture is a valid record");
    // engine/content/rules fingerprint
    assert!(record.fingerprint.validate().is_ok());
    // initial state
    assert!(!record.initial_state.label.is_empty());
    assert_ne!(
        record.initial_state.digest,
        ContentHash::from_bytes([0; 32])
    );
    // input stream
    assert!(!record.stream.is_empty());
    // seeds
    assert!(record.seeds.stream("synthetic_body").is_some());
    // tick rate and range
    assert!(record.tick_rate > 0);
    assert!(record.last_tick >= record.first_tick);
    // authored choices
    assert!(!record.choices.is_empty());
    // the promised state hashes
    assert!(!record.promised.is_empty());
    // the subject is a typed catalog id
    assert_eq!(record.subject.kind(), ContentKind::Mission);
    assert!(ContentId::parse(record.subject.as_str()).is_ok());
}

/// A run purpose round-trips through the document form, and an unknown purpose
/// is refused.
#[test]
fn accept_f59_a_run_purposes_round_trip_and_are_validated() {
    for purpose in RunPurpose::ALL {
        assert_eq!(RunPurpose::from_label(purpose.label()), Some(*purpose));
        let mut record = fixture();
        record.overrides = OverrideLog::new(*purpose, Vec::new(), false).expect("valid");
        let decoded = decode(&encode(&record).expect("encode")).expect("decode");
        assert_eq!(decoded.overrides.purpose(), *purpose);
        assert!(
            decoded.overrides.invalidates_ordinary_play() || *purpose == RunPurpose::OrdinaryPlay
        );
    }

    let text = String::from_utf8(encode(&fixture()).expect("encode")).expect("utf8");
    let forged = text.replace("purpose=ordinary_play", "purpose=testing");
    assert!(
        matches!(
            decode(forged.as_bytes()),
            Err(DecodeError::ChecksumMismatch)
        ),
        "a forged body fails the seal before its purpose is interpreted"
    );
}

/// A record's own subject cannot be forged into a different mission without
/// failing the seal, and a document whose subject is not a content id is
/// refused.
#[test]
fn accept_f59_a_the_recorded_subject_is_a_typed_content_id() {
    let record = fixture();
    let text = String::from_utf8(encode(&record).expect("encode")).expect("utf8");
    let forged = text.replace(record.subject.as_str(), "mission/synthetic.other");
    assert!(matches!(
        decode(forged.as_bytes()),
        Err(DecodeError::ChecksumMismatch)
    ));

    // A body naming a namespace that does not exist is refused when the seal
    // is correct, which is what a hand-edited document would look like.
    let body = sealed_body_of(&encode(&record).expect("encode"));
    let edited = body.replace(
        &format!("subject={}\n", record.subject),
        "subject=teapot/synthetic.replay\n",
    );
    let sealed = reseal(&format!("CSREPLAY 1.0\n{edited}"));
    assert!(matches!(
        decode(&sealed),
        Err(DecodeError::Malformed {
            reason: "invalid content id",
            ..
        })
    ));
}

/// Re-seals an edited body with a correct checksum, so a test can examine what
/// the decoder does with well-formed but wrong content.
fn reseal(body: &str) -> Vec<u8> {
    format!("{body}checksum={}\n", sha256(body.as_bytes()).to_hex()).into_bytes()
}

/// The body of an encoded document: the covered lines between the header and
/// the trailing `checksum=` line, header excluded and the last line still
/// newline-terminated.
fn sealed_body_of(bytes: &[u8]) -> String {
    let text = std::str::from_utf8(bytes).expect("utf8");
    let body_start = text.find('\n').expect("a header line") + 1;
    let checksum_start = text.rfind('\n').expect("a body line") + 1;
    text[body_start..checksum_start].to_owned()
}

/// The tone-curve vocabulary matches the renderer's, so the lowering at F59-B's
/// boundary is a label mapping and not a translation.
#[test]
fn accept_f59_a_the_tonemap_vocabulary_round_trips() {
    for kind in TonemapKind::ALL {
        assert_eq!(TonemapKind::from_label(kind.label()), Some(*kind));
    }
    assert_eq!(TonemapKind::from_label("curves"), None);
    let record = synthetic_capture_record();
    let mut filmic = record.clone();
    filmic.render.tonemap = TonemapKind::Filmic;
    let decoded = decode_capture(&encode_capture(&filmic).expect("encode")).expect("decode");
    assert_eq!(decoded.render.tonemap, TonemapKind::Filmic);
    assert!(!filmic.render.is_comparison_baseline());
}

/// The comparison baseline is what it says it is: no exposure change, no tone
/// curve, one sample per pixel, no shadows.
#[test]
fn accept_f59_a_the_render_comparison_baseline_is_pinned() {
    let baseline = RenderConfig::comparison();
    assert!(baseline.is_comparison_baseline());
    assert_eq!(baseline.tonemap, TonemapKind::None);
    assert_eq!(baseline.msaa_samples, 1);
    assert!(!baseline.shadows);
    assert!(baseline.validate().is_ok());

    for mutate in [
        (|config: &mut RenderConfig| config.exposure_milli = 1_001) as fn(&mut RenderConfig),
        |config: &mut RenderConfig| config.gamma_milli = 1_800,
        |config: &mut RenderConfig| config.tonemap = TonemapKind::Filmic,
        |config: &mut RenderConfig| config.msaa_samples = 4,
        |config: &mut RenderConfig| config.shadows = true,
    ] {
        let mut changed = baseline;
        mutate(&mut changed);
        assert!(
            !changed.is_comparison_baseline(),
            "a changed setting leaves the baseline: {changed:?}"
        );
    }
    // The framebuffer size is not part of the baseline, so a different size is
    // still a baseline render at another resolution.
    let mut wider = baseline;
    wider.width = 1_280;
    assert!(wider.is_comparison_baseline());
    assert!(wider.validate().is_ok());
}

/// Nothing in this module can award `verified_original` on its own: the
/// certification a bundle reaches is at most `checked`, and the ledger is what
/// decides originality.
#[test]
fn accept_f59_a_no_certification_here_ever_claims_verified_original() {
    let report = Certification::Checked {
        claims: vec![claim("f59.a.checked")],
    };
    assert!(
        report.to_string().contains("not verified_original"),
        "even a checked certification says what it is not: {report}"
    );
    for certification in [
        Certification::NotPassed {
            passed: 1,
            failed: 0,
        },
        Certification::Stale { reasons: 1 },
        Certification::Blocked { gaps: 1 },
        Certification::Refused { reasons: 1 },
        Certification::NotOrdinaryPlay,
        Certification::NoClaims,
    ] {
        assert!(
            !certification.to_string().contains("verified_original"),
            "{certification} must not mention originality"
        );
    }
}
