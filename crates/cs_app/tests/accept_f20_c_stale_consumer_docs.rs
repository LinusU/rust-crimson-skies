//! Acceptance scenario F20-C: the animation modules' doc paragraphs must
//! describe the mission-marker consumer task #507 actually added (task #670).
//!
//! Task test prefix: `accept_f20_c_`. This is prose plus a green build: a
//! rustdoc intra-doc-link check needs `cargo doc`, which no trybuild-free test
//! can invoke, so what can be pinned is pinned here — (1) the two paragraphs
//! name `crate::mission_markers` and the #507 finding and no longer claim the
//! consumer is absent or unstubbed (source-level, through `include_str!` of
//! the crate's own files, so reverting the prose fails these tests), (2) the
//! seam attribution is the recorded one (the 2026-10-02 finding recorded
//! `AnimationLog::drain`; the consumer binds the marker-specific
//! `AnimationLog::take_markers`), (3) the remaining "still not wired" claim
//! is the true one (the production mission host drives the path; the F39
//! declarations cannot be recovered, Rally #1219) and the stale #509 claim is
//! gone, and (4) the production items the paragraphs cross-link exist and are
//! the seam they say they are (compile-level: this test drives/references
//! them).

use cs_app::animation::AnimationLog;
use cs_app::mission_markers::{MissionMarkerBindings, MissionMarkerConsumer};
use cs_app::mission_session::{MissionHost, MissionHostRefusal};
use cs_app::objectives::LoweredObjectives;
use cs_types::net::SessionId;

/// The animation modules' docs, read from the crate's own sources.
const ANIMATION_MOD_RS: &str = include_str!("../src/animation/mod.rs");
const ANIMATION_BINDING_RS: &str = include_str!("../src/animation/binding.rs");

/// The two outside-the-animation-module paragraphs (task #1405).
const LIB_RS: &str = include_str!("../src/lib.rs");
const MISSION_MARKERS_RS: &str = include_str!("../src/mission_markers.rs");

/// The two files, as `(path, source)` for assertion messages.
const DOC_SOURCES: [(&str, &str); 2] = [
    ("crates/cs_app/src/animation/mod.rs", ANIMATION_MOD_RS),
    (
        "crates/cs_app/src/animation/binding.rs",
        ANIMATION_BINDING_RS,
    ),
];

#[test]
fn accept_f20_c_stale_consumer_docs_name_the_real_consumer() {
    for (path, source) in DOC_SOURCES {
        assert!(
            source.contains("crate::mission_markers"),
            "{path} must name `crate::mission_markers` as the mission-marker \
             consumer that drains the `AnimationLog`"
        );
        assert!(
            source.contains("2026-10-05-f20-c-mission-marker-consumer.md"),
            "{path} must cite the #507 finding that added the consumer"
        );
        assert!(
            !source.contains("mission-marker consumer is still absent"),
            "{path} must not claim the mission-marker consumer is still absent"
        );
        assert!(
            !source.contains("does **not** stub one"),
            "{path} must not claim the consumer is left unstubbed"
        );
    }
}

#[test]
fn accept_f20_c_stale_consumer_docs_name_the_recorded_seam() {
    for (path, source) in DOC_SOURCES {
        assert!(
            source.contains("AnimationLog::drain"),
            "{path} must name `AnimationLog::drain` as the seam the \
             2026-10-02 finding recorded"
        );
        assert!(
            source.contains("AnimationLog::take_markers"),
            "{path} must name `AnimationLog::take_markers` as the \
             marker-specific take the consumer binds"
        );
    }
}

#[test]
fn accept_f20_c_stale_consumer_docs_keep_the_unwired_claim_true() {
    for (path, source) in DOC_SOURCES {
        assert!(
            source.contains("crate::mission_session"),
            "{path} must name `crate::mission_session`, the production \
             mission host that now composes and drives the consumer"
        );
        assert!(
            source.contains("#1219"),
            "{path} must name #1219 as the remaining gap: the F39 objective \
             declarations cannot be recovered, so the host launches the \
             session from the empty `LoweredObjectives`"
        );
        assert!(
            !source.contains("#509"),
            "{path} must not cite #509 as an open gap: #509 was the \
             AnimationPlugin registration and is done, and the production \
             mission host drives the marker path since VS-M01-RT-MISSION-HOST"
        );
        assert!(
            !source.contains("no production mission host drives")
                && !source.contains("no production composition drives it"),
            "{path} must not claim no production composition drives the \
             path: `MissionHost::step` drives `step_mission_animations` on \
             every committed tick"
        );
    }
}

#[test]
fn accept_f20_c_stale_consumer_docs_cross_links_compile() {
    // The items the corrected paragraphs cross-link, exercised through
    // production code: `crate::mission_markers` takes the marker records out
    // of the log through `AnimationLog::take_markers`, exactly the seam the
    // paragraphs describe — an empty log hands over an empty batch, and the
    // consumer serves the generation it was built for.
    let session = SessionId::new(670).expect("a nonzero session generation");
    let consumer = MissionMarkerConsumer::new(session, MissionMarkerBindings::default());
    assert_eq!(consumer.served(), session);

    let mut log = AnimationLog::default();
    let taken: AnimationLog = log.take_markers();
    assert!(log.is_empty(), "the marker records were taken out");
    assert!(taken.is_empty(), "an empty log hands over an empty batch");

    // The production composition the paragraphs now point at: the mission
    // host and its declared-program refusal are nameable, the composed tick
    // is the public function they say it is, and the program the host
    // launches for original missions is the empty `LoweredObjectives`.
    let _host_is_public: Option<MissionHost> = None;
    let _refusal_is_public: Option<MissionHostRefusal> = None;
    let _empty_program: LoweredObjectives = cs_app::mission_session::no_declared_objectives();
    let _composed_tick = cs_app::mission_animations::step_mission_animations;
}

#[test]
fn accept_f20_c_stale_509_refs_in_lib_and_mission_markers_are_corrected() {
    for (path, source) in [
        ("crates/cs_app/src/lib.rs", LIB_RS),
        ("crates/cs_app/src/mission_markers.rs", MISSION_MARKERS_RS),
    ] {
        assert!(
            source.contains("MissionHost") && source.contains("step_mission_animations"),
            "{path} must name the mission host and the per-tick consumer step"
        );
        assert!(
            source.contains("#1219"),
            "{path} must name #1219 as the gap"
        );
        assert!(
            !source.contains("#509"),
            "{path} must not cite #509 as an open gap"
        );
        assert!(
            !source.contains("no production mission host drives")
                && !source.contains("no production composition drives it"),
            "{path} must not claim no production host drives the path"
        );
    }
}
