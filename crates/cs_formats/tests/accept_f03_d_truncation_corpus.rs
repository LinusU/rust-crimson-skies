//! Acceptance scenario F03-D: run the truncation corpus with recorded
//! resource limits.
//!
//! The corpus is the set of emitted valid buffers (`emit_case`), each a
//! *single* root node whose declared extent is exactly the buffer. Two facts
//! are asserted for every one of them:
//!
//! * the untruncated buffer parses through `ParseContext::parse` and hands
//!   back bounded fields that point into the input;
//! * truncating it at **every** byte boundary — including the empty prefix —
//!   is refused with `UnexpectedEof` at an absolute offset inside the prefix,
//!   never a panic and never a silent success "past the cut".
//!
//! Every case runs with the resource limits recorded in
//! `docs/findings/2026-09-28-f03-d-*`, and every refused attempt must leave
//! the allocation ledger and the recursion depth exactly as it found them.
//!
//! The buffer is synthetic: no `CS_GAME_DIR` read, nothing derived from the
//! original installation.

mod common;

use std::cell::Cell;

use common::nested::{
    CORPUS_CONTAINER, EMIT_SEEDS, RECORDED_ALLOCATION_LIMIT, assert_slices_within, decode_all,
    emit_case, recorded_context,
};
use cs_formats::ParseErrorKind;

#[test]
fn accept_f03_d_truncation_corpus_fails_at_every_boundary() {
    let mut boundaries = 0usize;
    let mut variants = [0usize; 4];

    for seed in 0..EMIT_SEEDS {
        let name = format!("emitted seed {seed}");
        let bytes = emit_case(seed);
        assert!(!bytes.is_empty(), "{name}: the emitter produced nothing");

        // The untruncated buffer: one root node covering every byte.
        let mut context = recorded_context();
        let stats = match decode_all(&mut context, &bytes, &Cell::new(0u32)) {
            Ok(stats) => stats,
            Err(error) => panic!("{name}: the emitted buffer must parse: {error}"),
        };
        assert_eq!(
            stats.roots, 1,
            "{name}: an emitted buffer is a single root node, not a sequence"
        );
        assert!(stats.nodes > 0, "{name}: decoded nothing");
        assert_slices_within(&name, &bytes, &stats.slices);
        assert!(
            context.allocation().used() <= RECORDED_ALLOCATION_LIMIT,
            "{name}: charged {} of {} bytes",
            context.allocation().used(),
            RECORDED_ALLOCATION_LIMIT,
        );
        assert_eq!(context.recursion().depth(), 0, "{name}");
        for (index, count) in stats.kinds.iter().enumerate() {
            variants[index] += count;
        }

        for cut in 0..bytes.len() {
            let prefix = &bytes[..cut];
            let mut context = recorded_context();
            let error = match decode_all(&mut context, prefix, &Cell::new(0u32)) {
                Ok(stats) => panic!(
                    "{name}: a {cut}-byte prefix of {} bytes was accepted \
                     ({} nodes, {} bounded fields)",
                    bytes.len(),
                    stats.nodes,
                    stats.slices.len(),
                ),
                Err(error) => error,
            };

            assert_eq!(
                error.kind,
                ParseErrorKind::UnexpectedEof,
                "{name}: cut at {cut}: {error}"
            );
            assert_eq!(
                error.container, CORPUS_CONTAINER,
                "{name}: cut at {cut}: {error}"
            );
            assert!(
                error.offset <= cut as u64,
                "{name}: cut at {cut}: offset {} leaves the prefix",
                error.offset,
            );

            // `observed` is `N bytes available`, and those bytes live inside
            // the prefix: N cannot reach past the cut from the failing read.
            let available = error
                .observed
                .strip_suffix(" bytes available")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| {
                    panic!(
                        "{name}: cut at {cut}: unexpected observed condition \
                         `{}`",
                        error.observed
                    )
                });
            assert!(
                available <= cut as u64 - error.offset,
                "{name}: cut at {cut}: {} bytes available past offset {} in a \
                 {cut}-byte prefix",
                available,
                error.offset,
            );

            assert_eq!(
                context.allocation().used(),
                0,
                "{name}: cut at {cut}: a failed attempt must book nothing"
            );
            assert_eq!(
                context.recursion().depth(),
                0,
                "{name}: cut at {cut}: every recursion guard was released"
            );
            boundaries += 1;
        }
    }

    for (variant, count) in variants.iter().enumerate() {
        assert!(
            *count > 0,
            "the emitted corpus never decoded node variant {variant}: {variants:?}"
        );
    }
    assert!(
        boundaries >= 1_000,
        "the recorded truncation corpus shrank to {boundaries} boundaries"
    );
    println!(
        "f03-d truncation: {EMIT_SEEDS} emitted buffers, {boundaries} truncated \
         boundaries refused with unexpected_eof, variants {variants:?}"
    );
}
