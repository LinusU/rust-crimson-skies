//! The span map one synthetic fixture carries: which byte ranges a cut
//! damages, and what damaging them must do.
//!
//! The manifest (`cs_xtask::corpus`) declares the *boundary kinds* a
//! container has; a fixture resolves them to real offsets so the oracle
//! can map "cut at byte N" to "this span was damaged" without the test
//! re-deriving the format's layout.

use std::ops::Range;

use cs_xtask::corpus::BoundaryKind;

/// What truncating into a span must make the parser do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Damage {
    /// The span is load-bearing: a prefix that does not contain the whole
    /// span — any cut below `end` — must be refused (or, for `ExtentStatus`
    /// containers, reported as lost content).
    Refuse,
    /// A fixed-width element (opcode word, PCM frame): only a cut strictly
    /// inside must be refused; a cut on the element boundary is a shorter
    /// valid input.
    Interior,
    /// The entrypoint may ignore these bytes entirely (a tolerated tail):
    /// a cut here demands only a bounded outcome, accept or refuse.
    Bounded,
}

/// One resolved region of a fixture.
#[derive(Clone, Debug)]
pub struct Span {
    /// Which manifest boundary kind this span resolves.
    pub kind: BoundaryKind,
    /// What a cut that damages it must do.
    pub damage: Damage,
    /// Byte range of the span, `[start, end)`.
    pub range: Range<usize>,
}

/// The authored bytes of one corpus entry plus its boundary resolution.
pub struct CorpusFixture {
    /// The authored bytes — always synthetic, always complete (the full
    /// buffer must satisfy the entry's `expected` outcome).
    pub bytes: Vec<u8>,
    /// The span map, in offset order.
    pub spans: Vec<Span>,
}

impl CorpusFixture {
    /// Assembles a fixture and checks the span map is inside the bytes.
    pub fn new(bytes: Vec<u8>, spans: &[Span]) -> Self {
        for span in spans {
            assert!(
                span.range.end <= bytes.len(),
                "span {:?} runs past the {} bytes",
                span.range,
                bytes.len(),
            );
            assert!(
                span.range.start < span.range.end,
                "empty span {:?}",
                span.range
            );
        }
        Self {
            bytes,
            spans: spans.to_vec(),
        }
    }

    /// Whether a truncation to `cut` bytes must be refused (or reported,
    /// for `ExtentStatus` containers): any `Refuse` span damaged — cut
    /// below its end — or a `Frame` element cut strictly inside.
    pub fn requires_refusal(&self, cut: usize) -> bool {
        self.spans.iter().any(|span| match span.damage {
            Damage::Refuse => cut < span.range.end,
            Damage::Interior => span.range.start < cut && cut < span.range.end,
            Damage::Bounded => false,
        })
    }

    /// Every declared boundary kind the fixture resolves.
    pub fn resolved_kinds(&self) -> Vec<BoundaryKind> {
        let mut kinds: Vec<BoundaryKind> = self.spans.iter().map(|span| span.kind).collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }
}

/// A `Refuse`-damage span.
pub fn hard(kind: BoundaryKind, range: Range<usize>) -> Span {
    Span {
        kind,
        damage: Damage::Refuse,
        range,
    }
}

/// A `Frame` span (interior-only refusal).
pub fn frame(range: Range<usize>) -> Span {
    Span {
        kind: BoundaryKind::Frame,
        damage: Damage::Interior,
        range,
    }
}

/// A `Slack` span (bounded outcome only).
pub fn slack(range: Range<usize>) -> Span {
    Span {
        kind: BoundaryKind::Slack,
        damage: Damage::Bounded,
        range,
    }
}
