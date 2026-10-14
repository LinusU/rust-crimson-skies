//! Acceptance scenario F20-C: the animation modules' doc paragraphs must
//! describe the mission-marker consumer task #507 actually added (task #670).
//!
//! Task test prefix: `accept_f20_c_`. This is prose plus a green build: a
//! rustdoc intra-doc-link check needs `cargo doc`, which no trybuild-free test
//! can invoke, so what can be pinned is pinned here — (1) the two paragraphs
//! name `crate::mission_markers` and the #507 finding and no longer claim the
//! consumer is absent or unstubbed (source-level, through `include_str!` of
//! the crate's own files, so reverting the prose fails these tests), and (2)
//! the production items the paragraphs cross-link exist and are the seam they
//! say they are (compile-level: this test drives them).

use cs_app::animation::AnimationLog;
use cs_app::mission_markers::{MissionMarkerBindings, MissionMarkerConsumer};
use cs_types::net::SessionId;

/// The animation modules' docs, read from the crate's own sources.
const ANIMATION_MOD_RS: &str = include_str!("../src/animation/mod.rs");
const ANIMATION_BINDING_RS: &str = include_str!("../src/animation/binding.rs");

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
fn accept_f20_c_stale_consumer_docs_keep_the_unwired_claim_true() {
    for (path, source) in DOC_SOURCES {
        assert!(
            source.contains("#509"),
            "{path} must name #509 as the remaining gap: `ObjectiveSession` \
             is host-owned and no production composition drives it"
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
}
