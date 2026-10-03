//! Acceptance scenarios F59-B: deterministic input and state capture.
//! Task test prefix: `accept_f59_b_`.
//!
//! Spec: `specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
//! stage `### F59-B`. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! Every state hash, envelope comparison and compatibility verdict here comes
//! from the production capture path (`cs_app::capture`) driving the production
//! flight world: the same `PhysicsSession`, the same `FlightForcesPlugin`, the
//! same `spawn_flight_body` and the same `FlightModel` the game runs. Nothing
//! asserts anything about the original game: the airframe is the synthetic
//! fixture, carries `Origin::SyntheticFixture`, and every value is newly
//! authored project design.

use cs_app::capture::identity::{AIRFRAME_CONTENT_KEY, LoadedContent, airframe_content_digest};
use cs_app::capture::render::{render_for, settings_for};
use cs_app::capture::replay::{
    BuildContext, CaptureRunError, ReplaySubject, RunRequest, record, replay,
};
use cs_app::capture::state::{StateProbe, StateProbeError, StateReading};
use cs_app::physics::{FlightSpawnSpec, PhysicsSample};
use cs_assets::install::sha256;
use cs_content::replay::{
    BuildId, CompatibilityDifference, CompatibilityVerdict, CrossBuildPolicy, OverrideEntry,
    OverrideLog, RenderConfig, ReplayRecord, RunPurpose, TonemapKind, decode, encode,
};
use cs_sim::flight::{EngineState, HandlingProfile, synthetic_fixed_wing};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::input::{Action, AxisValue, CommandStream, FlightCommand, InputFrame};

/// Ticks each acceptance run flies. Short on purpose: the point is that the
/// production path is exercised, and a long run only makes CI slower.
const RUN_TICKS: u64 = 8;

/// A candidate tree hash. `BuildId` is a validated git object id, so a
/// fabricated one has to look like one; nothing here reads the real checkout.
fn tree() -> BuildId {
    BuildId::new("0f1e2d3c4b5a69788796a5b4c3d2e1f001234567")
        .expect("the fixture tree hash is 40 hex digits")
}

fn build() -> BuildContext {
    BuildContext::on_host(tree(), "rustc 1.98.0; bevy 0.19; avian3d 0.7")
}

fn subject() -> ReplaySubject {
    ReplaySubject::flight(subject_id(), synthetic_fixed_wing(), spawn())
}

fn subject_id() -> ContentId {
    ContentId::from_source(ContentKind::Mission, "synthetic.flight-replay")
        .expect("the fixture content id is valid")
}

/// The synthetic fixed wing trimmed at 120 m/s at 1000 m, throttle 0.7,
/// wings level — the same declared scenario F23-D's stability probe flies.
fn spawn() -> FlightSpawnSpec {
    FlightSpawnSpec {
        engine: EngineState::direct(0.6),
        command: cs_sim::flight::FlightInput {
            throttle: 0.7,
            ..cs_sim::flight::FlightInput::NEUTRAL
        },
        ..FlightSpawnSpec::level_at([0.0, 1000.0, 0.0], [0.0, 0.0, -120.0])
    }
}

/// A held axis stream: every tick from 1 through `RUN_TICKS` carries the same
/// pitch and throttle deflection.
fn held(pitch: i16, throttle: i16) -> CommandStream {
    let mut stream = CommandStream::new();
    for step in 1..=RUN_TICKS {
        let mut frame = InputFrame::new(Tick(step));
        frame.set_axis(
            AxisValue::from_quantized(FlightCommand::Pitch, pitch)
                .expect("pitch is a continuous axis"),
        );
        frame.set_axis(
            AxisValue::from_quantized(FlightCommand::Throttle, throttle)
                .expect("throttle is a continuous axis"),
        );
        stream
            .record_tick(frame)
            .expect("the fixture's ticks strictly increase");
    }
    stream
}

/// The fixture's input: a held half-pitch and the throttle axis that maps to
/// the spawn command's 0.7 (`(0.4 + 1) / 2 = 0.7`).
fn fixture_stream() -> CommandStream {
    held(16_384, 13_107)
}

fn request<'a>(
    subject: &'a ReplaySubject,
    build: &'a BuildContext,
    stream: &'a CommandStream,
) -> RunRequest<'a> {
    RunRequest {
        subject,
        build,
        stream,
        ticks: RUN_TICKS,
    }
}

fn recorded() -> ReplayRecord {
    let subject = subject();
    let build = build();
    let stream = fixture_stream();
    record(&request(&subject, &build, &stream)).expect("the fixture run records")
}

/// AC01's runtime half, and the minimum scenario's neighbour: the same input
/// stream, flown twice, in two independent worlds, promises the same state.
///
/// It fails if the recorder reused a world, if the state hash were derived from
/// the input frame (which would make this pass for the wrong reason — see
/// `accept_f59_b_the_state_hash_follows_the_world_not_the_input`), or if the
/// per-tick envelope were built from anything but the measurements.
#[test]
fn accept_f59_b_replaying_one_input_stream_twice_gives_identical_state_hashes() {
    let subject = subject();
    let build = build();
    let stream = fixture_stream();

    let first = record(&request(&subject, &build, &stream)).expect("the first run records");
    let second = record(&request(&subject, &build, &stream)).expect("the second run records");

    assert_eq!(first.promised.len(), RUN_TICKS as usize);
    assert_eq!(first.promised.entries(), second.promised.entries());
    assert_eq!(
        first.promised.chain_digest(first.initial_state.digest),
        second.promised.chain_digest(second.initial_state.digest),
        "two runs of one stream must chain to the same state"
    );
    assert_eq!(
        first.compatibility_signature(),
        second.compatibility_signature()
    );

    // AC01: replaying the record in a third, fresh world reproduces the promise
    // tick for tick, and the compatibility gate passes.
    let outcome = replay(&first, &subject, &build, CrossBuildPolicy::Reject)
        .expect("the recorded run replays");
    assert!(
        outcome.reproduces_promised_state(),
        "the replay diverged: {:?}",
        outcome.divergence()
    );
    assert_eq!(outcome.observed.entries(), first.promised.entries());
    assert_eq!(outcome.ticks, RUN_TICKS);
    assert!(
        outcome.verdict.certifies_determinism(),
        "a same-build replay of the same content must certify: {:?}",
        outcome.verdict
    );
}

/// **AC02, the stage's minimum scenario**: change a content asset and reject the
/// old replay's compatibility signature.
///
/// The change is made the way a content edit actually arrives — the subject
/// loads a different airframe tuning — and the refusal is the one production
/// code produces, so `Content` is named rather than a bare "it differs". The
/// complementary half matters too: with the original content the same replay is
/// accepted, so the refusal cannot be an artifact of the harness.
#[test]
fn accept_f59_b_a_changed_content_asset_rejects_the_old_replay() {
    let recorded = recorded();
    let build = build();
    let original = subject();

    let mut edited_tuning = synthetic_fixed_wing();
    edited_tuning.mass.mass_kg += 250.0;
    let edited = original.clone().with_tuning(edited_tuning.clone());

    // The edit really moved the content digest and only the content digest.
    assert_ne!(
        airframe_content_digest(&synthetic_fixed_wing()),
        airframe_content_digest(&edited_tuning)
    );
    assert_ne!(
        recorded.fingerprint.content,
        edited.loaded.digest(),
        "a loaded coefficient edit must move the record's content digest"
    );

    let outcome = replay(&recorded, &edited, &build, CrossBuildPolicy::Reject)
        .expect("the edited content still flies");
    let verdict = &outcome.verdict;
    assert!(
        !verdict.certifies_determinism(),
        "a replay recorded against different content must certify nothing: {verdict}"
    );
    assert!(
        matches!(verdict, CompatibilityVerdict::Rejected { .. }),
        "Reject must refuse rather than degrade: {verdict}"
    );
    assert!(
        verdict
            .differences()
            .contains(&CompatibilityDifference::Content),
        "the refusal must name the content as what moved: {verdict}"
    );

    // The unchanged content still accepts the very same record.
    let unchanged = replay(&recorded, &original, &build, CrossBuildPolicy::Reject)
        .expect("the unchanged content still flies");
    assert!(
        unchanged.verdict.certifies_determinism(),
        "the same record against its own content must be compatible: {:?}",
        unchanged.verdict
    );
}

/// The state hash is measured from the world, not derived from the input.
///
/// The two runs below are given the **same** input stream and differ only in
/// the content the world hosts. If the digest were taken from the input frame —
/// the shortcut this stage exists to refuse — the two envelopes would be
/// identical and the test would fail here.
#[test]
fn accept_f59_b_the_state_hash_follows_the_world_not_the_input() {
    let build = build();
    let stream = fixture_stream();
    let original = subject();

    let mut heavier = synthetic_fixed_wing();
    heavier.mass.mass_kg += 400.0;
    let edited = original.clone().with_tuning(heavier);

    let a = record(&request(&original, &build, &stream)).expect("the original run records");
    let b = record(&request(&edited, &build, &stream)).expect("the edited run records");

    assert_ne!(
        a.promised.entries(),
        b.promised.entries(),
        "identical input over different content must produce different measured state"
    );
    let divergence = a.verdict_against(&b.promised);
    assert!(
        !divergence.is_identical(),
        "the two worlds' envelopes must not compare identical"
    );
}

/// The recorded input is what drives the simulation: one quantization step of
/// pitch on one tick changes the state the run promises.
///
/// This is the sensitivity that makes AC01's comparison worth anything. A
/// recorder that ignored its stream — or that hashed the input instead of
/// measuring it — would produce two identical envelopes here.
#[test]
fn accept_f59_b_a_changed_input_sample_diverges_the_recorded_run() {
    let build = build();
    let subject = subject();
    let base = fixture_stream();

    let mut nudged = CommandStream::new();
    for frame in base.records() {
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
                AxisValue::from_quantized(axis.command(), quantized)
                    .expect("the axis is continuous"),
            );
        }
        nudged
            .record_tick(copy)
            .expect("the nudged ticks strictly increase");
    }

    let a = record(&request(&subject, &build, &base)).expect("the base run records");
    let b = record(&request(&subject, &build, &nudged)).expect("the nudged run records");

    assert_ne!(a.promised.entries(), b.promised.entries());
    let verdict = a.compatibility_with(&b, CrossBuildPolicy::Reject);
    assert!(!verdict.certifies_determinism());
    assert!(
        verdict
            .differences()
            .contains(&CompatibilityDifference::InputStream),
        "the refusal must name the input stream: {verdict}"
    );
}

/// A best-effort comparison names the difference and certifies nothing.
///
/// F59 non-negotiable 1 allows an explicitly best-effort path; what it must not
/// allow is that path quietly becoming a determinism claim. The verdict's own
/// `certifies_determinism` is the guard, and the comparison is still reported.
#[test]
fn accept_f59_b_best_effort_names_the_difference_and_certifies_nothing() {
    let recorded = recorded();
    let build = build();

    let mut edited_tuning = synthetic_fixed_wing();
    edited_tuning.mass.mass_kg += 250.0;
    let edited = subject().with_tuning(edited_tuning);

    let outcome = replay(&recorded, &edited, &build, CrossBuildPolicy::BestEffort)
        .expect("a best-effort comparison still flies the run");
    assert!(
        matches!(outcome.verdict, CompatibilityVerdict::BestEffort { .. }),
        "BestEffort must produce its own verdict: {:?}",
        outcome.verdict
    );
    assert!(
        !outcome.verdict.certifies_determinism(),
        "a best-effort verdict certifies no determinism"
    );
    assert!(
        outcome
            .verdict
            .differences()
            .contains(&CompatibilityDifference::Content)
    );
    // The state comparison is still reported, and it still moved.
    assert!(!outcome.reproduces_promised_state());
}

/// A rules change is reported as `Rules`, not as a content change.
///
/// The handling profile and the fixed rate are the evaluation context, not
/// coefficients. Folding them into the content digest would make a profile
/// switch indistinguishable from an edited airframe, which is exactly the
/// distinction AC02's refusal is worth having.
#[test]
fn accept_f59_b_a_changed_handling_profile_is_a_rules_difference() {
    let recorded = recorded();
    let build = build();

    let mut improved = synthetic_fixed_wing();
    improved.profile = HandlingProfile::Improved;
    let retuned = subject().with_tuning(improved.clone());

    assert_eq!(
        airframe_content_digest(&synthetic_fixed_wing()),
        airframe_content_digest(&improved),
        "a profile switch must not edit the coefficients"
    );
    let outcome = replay(&recorded, &retuned, &build, CrossBuildPolicy::Reject)
        .expect("the improved profile still flies");
    let differences = outcome.verdict.differences();
    assert!(
        differences.contains(&CompatibilityDifference::Rules),
        "expected a rules difference, got {differences:?}"
    );
    assert!(
        !differences.contains(&CompatibilityDifference::Content),
        "a profile switch must not report as a content change: {differences:?}"
    );
}

/// A capture/probe run records its purpose, and a production-profile write is
/// visible in the record instead of happening behind it.
///
/// F59 non-negotiable 4: no normal campaign profile writes during capture or
/// probe runs unless explicitly requested, and debug overrides invalidate
/// ordinary-play proof. The runtime path's half is that it *says* which it was.
#[test]
fn accept_f59_b_a_probe_run_records_its_purpose_and_overrides() {
    let build = build();
    let stream = fixture_stream();
    let overrides = OverrideLog::new(
        RunPurpose::Probe,
        vec![OverrideEntry {
            name: "assists".to_owned(),
            detail: "bank-level assist forced off".to_owned(),
        }],
        false,
    )
    .expect("the probe override log is valid");
    let subject = subject().with_overrides(overrides.clone());

    let recorded = record(&request(&subject, &build, &stream)).expect("the probe run records");
    assert_eq!(recorded.overrides, overrides);
    assert_eq!(recorded.overrides.purpose(), RunPurpose::Probe);
    assert!(recorded.overrides.invalidates_ordinary_play());
    assert!(!recorded.overrides.violates_profile_rule());

    let mut wrote = subject.clone();
    wrote.overrides = OverrideLog::new(RunPurpose::Capture, Vec::new(), true)
        .expect("the capture override log is valid");
    let wrote_record = record(&request(&wrote, &build, &stream)).expect("the capture run records");
    assert!(
        wrote_record.overrides.violates_profile_rule(),
        "a tooling run that wrote a production profile must be visible as such"
    );
}

/// A recorded edge this run cannot execute is refused, not dropped.
///
/// The fixed-wing capture path hosts no weapon, ordnance or menu system. Silently
/// dropping the press would produce a replay that promises the state of a run
/// whose press never happened.
#[test]
fn accept_f59_b_a_recorded_edge_this_run_cannot_execute_is_refused() {
    let build = build();
    let subject = subject();
    let mut stream = CommandStream::new();
    for step in 1..=RUN_TICKS {
        let mut frame = InputFrame::new(Tick(step));
        frame.set_axis(
            AxisValue::from_quantized(FlightCommand::Throttle, 13_107)
                .expect("throttle is a continuous axis"),
        );
        if step == 3 {
            frame.push_edge(Action::Flight(FlightCommand::FirePrimary));
        }
        stream
            .record_tick(frame)
            .expect("the fixture's ticks strictly increase");
    }

    let error = record(&request(&subject, &build, &stream))
        .expect_err("a fire edge has no consumer in this run");
    match error {
        CaptureRunError::UnconsumedAction { tick, action } => {
            assert_eq!(tick, Tick(3));
            assert_eq!(action, FlightCommand::FirePrimary.label());
        }
        other => panic!("expected an unconsumed-action refusal, got {other}"),
    }
}

/// A recorded frame outside the ticks the run drives is refused.
///
/// A frame past the last tick would never be applied. Dropping it would write a
/// record whose promised stream is longer than the run it came from.
#[test]
fn accept_f59_b_a_recorded_tick_outside_the_run_is_refused() {
    let build = build();
    let subject = subject();
    let mut stream = fixture_stream();
    let mut frame = InputFrame::new(Tick(RUN_TICKS + 4));
    frame.set_axis(
        AxisValue::from_quantized(FlightCommand::Pitch, 1000).expect("pitch is a continuous axis"),
    );
    stream
        .record_tick(frame)
        .expect("the frame's tick was not recorded yet");

    let error = record(&request(&subject, &build, &stream))
        .expect_err("a frame past the last tick cannot be flown");
    assert!(
        matches!(error, CaptureRunError::StreamTickOutsideRun { .. }),
        "expected a stream-range refusal, got {error}"
    );
}

/// A run that measured no tick promises nothing, and says so.
#[test]
fn accept_f59_b_a_run_with_no_ticks_is_refused() {
    let build = build();
    let subject = subject();
    let stream = CommandStream::new();
    let error = record(&RunRequest {
        subject: &subject,
        build: &build,
        stream: &stream,
        ticks: 0,
    })
    .expect_err("an empty run promises no state");
    assert!(
        matches!(error, CaptureRunError::NoTicksMeasured),
        "expected an empty-run refusal, got {error}"
    );
}

/// A state reading that is not a real state is refused rather than hashed.
///
/// `NaN` bits compare unequal to themselves and a digest over them is a number
/// that describes nothing, so the probe names the offending field instead.
#[test]
fn accept_f59_b_a_non_finite_state_is_refused_rather_than_hashed() {
    let pose = PhysicsSample {
        position_m: [0.0, f32::NAN, 0.0],
        linear_velocity_m_s: [0.0; 3],
        angular_velocity_rad_s: [0.0; 3],
    };
    let mut probe = StateProbe::new();
    probe
        .start(&StateReading::at_spawn(PhysicsSample {
            position_m: [0.0; 3],
            linear_velocity_m_s: [0.0; 3],
            angular_velocity_rad_s: [0.0; 3],
        }))
        .expect("a finite spawn state is measured");

    let error = probe
        .measure(&StateReading::after_tick(Tick(1), pose, finite_output()))
        .expect_err("a non-finite reading is refused");
    assert_eq!(
        error,
        StateProbeError::NonFinite {
            tick: Tick(1),
            field: "position_m",
        }
    );
    assert!(
        probe.envelope().is_empty(),
        "a refused reading must leave the envelope untouched"
    );
}

/// A tick measured before the run's start is refused: the envelope chains from
/// the initial state, so an envelope without one has nothing to hang from.
#[test]
fn accept_f59_b_a_tick_measured_before_the_run_started_is_refused() {
    let mut probe = StateProbe::new();
    let error = probe
        .measure(&StateReading::after_tick(
            Tick(1),
            rest_pose(),
            finite_output(),
        ))
        .expect_err("the run has not measured its initial state");
    assert_eq!(error, StateProbeError::NotStarted);
}

/// The record the runtime produces is a transportable document.
///
/// A state hash a replay can compare in memory but not on disk is half a
/// deliverable: this is the round trip through F59-A's own `encode`/`decode`, on
/// a record this path built.
#[test]
fn accept_f59_b_the_recorded_run_survives_the_document_form() {
    let recorded = recorded();
    let bytes = encode(&recorded).expect("the recorded run encodes");
    let decoded = decode(&bytes).expect("the recorded run decodes");
    assert_eq!(decoded, recorded);
    assert_eq!(
        decoded.compatibility_signature(),
        recorded.compatibility_signature()
    );
}

/// The render configuration the capture record pins and the settings the
/// renderer is given are one value, and a setting the record cannot pin is
/// refused.
///
/// The record stores thousandths and the renderer `f32`s; this is the boundary
/// that converts, and both directions have to come back to the same set or a
/// record's pinned settings would describe a frame rendered differently.
#[test]
fn accept_f59_b_the_render_lowering_round_trips_and_refuses_an_unpinned_setting() {
    let baseline = RenderConfig::comparison();
    let settings = settings_for(&baseline).expect("the baseline is in range");
    assert!(
        settings.is_fixed(),
        "F59's baseline must be F17's fixed set"
    );
    let raised = render_for(&settings, baseline.width, baseline.height)
        .expect("the fixed set lowers back into a record");
    assert_eq!(raised, baseline);

    // A filmic baseline is the other declared tone curve, and it round-trips too.
    let filmic_settings = settings_for(&RenderConfig {
        tonemap: TonemapKind::Filmic,
        ..baseline
    })
    .expect("a filmic capture is in range");
    let filmic_back = render_for(&filmic_settings, 800, 600).expect("the filmic set is in range");
    assert_eq!(
        filmic_back,
        RenderConfig {
            width: 800,
            height: 600,
            tonemap: TonemapKind::Filmic,
            ..baseline
        }
    );

    // An internal resolution override has no field in the pinned record, so it
    // is refused rather than dropped into a record that would claim a frame it
    // does not describe.
    let overridden = cs_app::render::capture::ComparisonSettings::with_render_resolution(Some(
        cs_app::render::profile::Resolution::new(1280, 720).expect("1280x720 is a real resolution"),
    ));
    let error = render_for(&overridden, baseline.width, baseline.height)
        .expect_err("a resolution override cannot be pinned");
    assert_eq!(
        error,
        cs_content::replay::CaptureError::RenderSettingUnpinned {
            field: "render_resolution",
        }
    );
}

/// The content digest of an inventoried installation is F02's own content
/// fingerprint.
///
/// The capture path does not define a second notion of "the content a run
/// loaded": for an installation it is the rows F02 already digests, so a retail
/// replay and an evidence report cannot disagree about it.
#[test]
fn accept_f59_b_installed_content_digests_exactly_as_f02_does() {
    let manifest = cs_types::install::InstallManifest::new(
        std::path::PathBuf::from("/private/crimson-skies"),
        vec![
            file_row("data/mesh.pl", b"first content bytes"),
            file_row("data/sound.rof", b"second content bytes"),
        ],
    )
    .expect("the fixture manifest is valid");

    let loaded = LoadedContent::from_manifest(&manifest);
    assert_eq!(
        loaded.digest(),
        cs_assets::install::content_fingerprint(&manifest),
        "the capture path must not define its own content digest"
    );

    // A one-byte content edit moves it, which is what makes AC02 fire for a
    // retail replay too.
    let edited = cs_types::install::InstallManifest::new(
        std::path::PathBuf::from("/private/crimson-skies"),
        vec![
            file_row("data/mesh.pl", b"first content byteS"),
            file_row("data/sound.rof", b"second content bytes"),
        ],
    )
    .expect("the edited fixture manifest is valid");
    assert_ne!(
        LoadedContent::from_manifest(&edited).digest(),
        loaded.digest()
    );

    // The host root is not part of the content digest, so the same bytes under
    // another spelling are the same content.
    let relocated = cs_types::install::InstallManifest::new(
        std::path::PathBuf::from("/Volumes/Other/crimson-skies"),
        vec![
            file_row("data/mesh.pl", b"first content bytes"),
            file_row("data/sound.rof", b"second content bytes"),
        ],
    )
    .expect("the relocated fixture manifest is valid");
    assert_eq!(
        LoadedContent::from_manifest(&relocated).digest(),
        loaded.digest()
    );

    // And the airframe row the flight path loads is a normal content row.
    assert_eq!(
        LoadedContent::of_record(AIRFRAME_CONTENT_KEY, sha256(b"coefficients")).digest(),
        LoadedContent::new()
            .with_row(AIRFRAME_CONTENT_KEY, sha256(b"coefficients"))
            .digest()
    );
}

/* ------------------------------------------------------------------ */
/* Fixtures                                                            */
/* ------------------------------------------------------------------ */

fn rest_pose() -> PhysicsSample {
    PhysicsSample {
        position_m: [0.0; 3],
        linear_velocity_m_s: [0.0; 3],
        angular_velocity_rad_s: [0.0; 3],
    }
}

/// A finite flight output, for the probe-only cases that never integrate.
fn finite_output() -> cs_sim::flight::FlightOutput {
    cs_sim::flight::FlightModel::new(synthetic_fixed_wing())
        .compute(
            &cs_sim::flight::FlightEnvironment::SEA_LEVEL,
            &cs_sim::flight::LoadoutMass::EMPTY,
            &cs_sim::flight::DamageState::PRISTINE,
            &cs_sim::flight::FlightState::at_rest(cs_types::space::Quaternion::IDENTITY),
            &cs_sim::flight::FlightInput::NEUTRAL,
            1.0 / 64.0,
        )
        .expect("the neutral state computes finite forces")
}

fn file_row(key: &str, bytes: &[u8]) -> cs_types::install::InstallFileRecord {
    cs_types::install::InstallFileRecord {
        relative_spelling: cs_types::install::RelativePath::new(key)
            .expect("the fixture logical path is valid"),
        size_bytes: bytes.len() as u64,
        sha256: sha256(bytes),
        family: None,
        role: cs_types::install::FileRole::Unknown,
        parse_state: cs_types::install::ParseState::Unparsed,
    }
}
