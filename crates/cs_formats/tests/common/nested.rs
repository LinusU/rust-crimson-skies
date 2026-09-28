//! Deterministic nested range/string corpus for the F03-D acceptance tests.
//!
//! Newly authored synthetic bytes only (`specs/F03-bounded-binary-parsing-
//! primitives.md`): nothing here is derived from original game data and
//! nothing reads `CS_GAME_DIR`.
//!
//! The grammar is one deliberately small structure that exercises exactly the
//! operations stage F03-D fuzzes — nested ranges ([`Reader::sub_reader`]),
//! bounded strings ([`Reader::read_str`] and [`Reader::read_bounded_cstr`]),
//! checked table extents ([`AllocationBudget::reserve`]) and the recursion
//! limit ([`RecursionBudget::enter`]):
//!
//! ```text
//! node   := kind:u8, payload
//! kind 0 := leaf  : len:u16, utf8[len]      (exactly len bytes follow)
//! kind 1 := cstr  : max:u8, field[max] with one 0x00 inside
//! kind 2 := range : len:u16, child nodes filling exactly len bytes
//! kind 3 := table : count:u32, elem:u8, payload[count * elem]
//! ```
//!
//! Only the low two bits of `kind` select the variant, so a raw random byte
//! reaches every one of them; the emitter always writes `0..=3`.
//!
//! Two properties make the corpus useful as *evidence* rather than as noise:
//!
//! * every node consumes exactly the bytes it declares, and the driver keeps
//!   reading nodes until the range it was handed is empty — so a successful
//!   decode accounts for every input byte, and truncating a valid buffer can
//!   only fail, never succeed "past the cut";
//! * every case is derived from a fixed seed (SplitMix64) or from an explicit
//!   byte table, so a failure names one reproducible input.

use std::cell::Cell;

use cs_formats::{
    AllocationBudget, ParseContext, ParseError, ParseErrorKind, Reader, RecursionBudget,
};

/// Provenance label every F03-D case reports its failures against.
pub const CORPUS_CONTAINER: &str = "synthetic/f03_d_nested.bin";

/// Entry point name stamped onto every error as it propagates.
pub const ENTRYPOINT: &str = "record";

/// Recorded allocation budget for one corpus case, in bytes.
///
/// A *designed* test budget (EvidenceClass `Designed`), not an original game
/// value: it is small enough that a hostile `u32` count is refused before it
/// can become an allocation, and large enough for every emitted table.
/// Recorded in `docs/findings/2026-09-28-f03-d-*`.
pub const RECORDED_ALLOCATION_LIMIT: u64 = 8 * 1024;

/// Recorded recursion limit for one corpus case, in nested nodes.
///
/// Also designed, never observed: the emitter never nests deeper than this,
/// and the crafted deep-chain vector deliberately goes past it.
pub const RECORDED_MAX_DEPTH: u32 = 8;

/// Random seeds in the fuzz corpus (`0..FUZZ_RANDOM_SEEDS`).
pub const FUZZ_RANDOM_SEEDS: u64 = 256;

/// Largest raw random input, in bytes.
pub const FUZZ_MAX_INPUT: usize = 96;

/// Seeds in the emitted (valid) corpus used by the truncation sweep.
pub const EMIT_SEEDS: u64 = 32;

/// Node budget for one emitted case: bounds the truncation sweep's cost.
pub const EMIT_NODES: usize = 12;

/// Soft byte budget for the children of one emitted range.
pub const EMIT_MAX_BYTES: usize = 64;

pub const KIND_LEAF: u8 = 0;
pub const KIND_CSTR: u8 = 1;
pub const KIND_RANGE: u8 = 2;
pub const KIND_TABLE: u8 = 3;

/// SplitMix64: one fixed stream per seed, so every case is reproducible from
/// the seed printed next to a failing assertion.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n`; `n` must be nonzero.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "modulus must be nonzero");
        self.next_u64() % n
    }
}

/// What one successful decode accounted for.
#[derive(Debug, Default)]
pub struct Stats<'bytes> {
    /// Nodes decoded.
    pub nodes: usize,
    /// Top-level nodes of the buffer, as counted by [`decode_all`]. `absorb`
    /// leaves it alone: it describes the buffer, not a subtree.
    pub roots: usize,
    /// Nodes per variant, indexed by `kind & 0b11`.
    pub kinds: [usize; 4],
    /// Deepest recursion level entered (1 is the outermost node).
    pub deepest: u32,
    /// Bounded fields returned as slices of the input (never copies).
    pub slices: Vec<&'bytes [u8]>,
}

impl<'bytes> Stats<'bytes> {
    /// Folds a child node's statistics into its parent's.
    pub fn absorb(&mut self, other: Stats<'bytes>) {
        self.nodes += other.nodes;
        for (mine, theirs) in self.kinds.iter_mut().zip(other.kinds) {
            *mine += theirs;
        }
        self.deepest = self.deepest.max(other.deepest);
        self.slices.extend(other.slices);
    }
}

/// Decodes one node and everything nested inside it.
///
/// This is production code under test: it only calls `cs_formats` reader,
/// budget and recursion operations, and it never indexes bytes itself.
/// Removing a bound check in `Reader`, a charge in `AllocationBudget` or the
/// ceiling in `RecursionBudget` changes what comes back here, which is what
/// the `accept_f03_d_*` tests assert on.
pub fn decode_node<'bytes>(
    reader: &mut Reader<'bytes>,
    allocation: &mut AllocationBudget,
    recursion: &RecursionBudget,
    peak_depth: &Cell<u32>,
) -> Result<Stats<'bytes>, ParseError> {
    let guard = recursion.enter("node", reader.position())?;
    let level = guard.level();
    peak_depth.set(peak_depth.get().max(level));

    let mut stats = Stats {
        nodes: 1,
        roots: 0,
        kinds: [0; 4],
        deepest: level,
        slices: Vec::new(),
    };
    let variant = (reader.read_u8("node.kind")? & 0b11) as usize;
    stats.kinds[variant] = 1;

    match variant {
        0 => {
            let len = usize::from(reader.read_u16("node.leaf.len")?);
            let text = reader.read_str("node.leaf.text", len)?;
            stats.slices.push(text.as_bytes());
        }
        1 => {
            let max = usize::from(reader.read_u8("node.cstr.max")?);
            let text = reader.read_bounded_cstr("node.cstr.field", max)?;
            stats.slices.push(text.as_bytes());
        }
        2 => {
            let len = usize::from(reader.read_u16("node.range.len")?);
            let mut range = reader.sub_reader("node.range", len)?;
            // Every child consumes at least its kind byte, so a hostile
            // `len` still terminates: either the range ends or a read fails.
            while !range.is_empty() {
                let child = decode_node(&mut range, allocation, recursion, peak_depth)?;
                stats.absorb(child);
            }
        }
        _ => {
            let count = u64::from(reader.read_u32("node.table.count")?);
            let elem = u64::from(reader.read_u8("node.table.elem_size")?);
            let anchor = reader.position();
            // Reserve before touching the payload: a hostile count is refused
            // by the budget without ever allocating the bytes it asks for.
            let bytes = allocation.reserve("node.table.payload", anchor, count, elem)?;
            let payload = reader.read_bytes("node.table.payload", bytes)?;
            stats.slices.push(payload);
        }
    }
    Ok(stats)
}

/// One attempt of the whole buffer through the production entrypoint.
///
/// The first node is required (an empty input is an `UnexpectedEof`, not a
/// silent success), then nodes are read until the buffer is exhausted.
pub fn decode_all<'bytes>(
    context: &mut ParseContext,
    bytes: &'bytes [u8],
    peak_depth: &Cell<u32>,
) -> Result<Stats<'bytes>, ParseError> {
    context.parse(ENTRYPOINT, bytes, |reader, allocation, recursion| {
        let mut stats = decode_node(reader, allocation, recursion, peak_depth)?;
        let mut roots = 1;
        while !reader.is_empty() {
            let more = decode_node(reader, allocation, recursion, peak_depth)?;
            stats.absorb(more);
            roots += 1;
        }
        // A successful decode accounts for every input byte: the loop only
        // ends when nothing is left, so no trailing byte was skipped as
        // padding (non-negotiable #4).
        assert!(
            reader.is_empty(),
            "the decode loop must consume the whole buffer"
        );
        stats.roots = roots;
        Ok(stats)
    })
}

/// A context carrying the recorded F03-D resource limits.
pub fn recorded_context() -> ParseContext {
    ParseContext::new(
        CORPUS_CONTAINER,
        RECORDED_ALLOCATION_LIMIT,
        RECORDED_MAX_DEPTH,
    )
}

/// Builds valid buffers: one root node whose extent is exactly the returned
/// buffer, so truncating it anywhere must fail.
pub fn emit_case(seed: u64) -> Vec<u8> {
    let mut emitter = Emitter::new(seed);
    emitter.root()
}

/// A raw random input for `seed` (`0..FUZZ_RANDOM_SEEDS`, length decided by
/// the same stream, `0..=FUZZ_MAX_INPUT`).
pub fn random_case(seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let len = rng.below(FUZZ_MAX_INPUT as u64 + 1) as usize;
    (0..len).map(|_| rng.next_u64() as u8).collect()
}

/// Deterministic emitter of valid nested structures.
struct Emitter {
    rng: Rng,
    /// Bytes already booked against the recorded allocation budget.
    reserved: u64,
    /// Nodes emitted so far (hard budget).
    nodes: usize,
    /// Bytes emitted so far (soft budget for range children).
    bytes: usize,
}

impl Emitter {
    fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            reserved: 0,
            nodes: 0,
            bytes: 0,
        }
    }

    fn root(&mut self) -> Vec<u8> {
        let node = self.node(1);
        debug_assert!(!node.is_empty());
        node
    }

    fn node(&mut self, level: u32) -> Vec<u8> {
        self.nodes += 1;
        // At the recorded recursion ceiling only leaf variants are emitted,
        // so a valid buffer always parses within the recorded limits.
        let leaf_only = level >= RECORDED_MAX_DEPTH;
        let pick = self.rng.below(if leaf_only { 2 } else { 4 }) as u8;
        let out = match pick {
            KIND_LEAF => self.leaf(),
            KIND_CSTR => self.cstr(),
            KIND_RANGE => self.range(level),
            _ => match self.table() {
                Some(bytes) => bytes,
                // Booking it would exceed the recorded budget: emit a leaf
                // instead so the buffer still parses successfully.
                None => self.leaf(),
            },
        };
        debug_assert!(!leaf_only || out[0] != KIND_RANGE);
        self.bytes += out.len();
        out
    }

    fn leaf(&mut self) -> Vec<u8> {
        let len = self.rng.below(17) as usize;
        let mut out = vec![KIND_LEAF];
        out.extend_from_slice(&(len as u16).to_le_bytes());
        for _ in 0..len {
            out.push(b'a' + self.rng.below(26) as u8);
        }
        out
    }

    fn cstr(&mut self) -> Vec<u8> {
        let max = 1 + self.rng.below(16) as usize;
        let text_len = self.rng.below(max as u64) as usize;
        let mut out = vec![KIND_CSTR, max as u8];
        for _ in 0..text_len {
            out.push(b'A' + self.rng.below(26) as u8);
        }
        // The terminator and any padding: exactly `max` field bytes follow.
        out.resize(2 + max, 0);
        out
    }

    fn range(&mut self, level: u32) -> Vec<u8> {
        let wanted = self.rng.below(4);
        let mut payload = Vec::new();
        let mut made = 0;
        while made < wanted && self.nodes < EMIT_NODES && self.bytes < EMIT_MAX_BYTES {
            let child = self.node(level + 1);
            payload.extend(child);
            made += 1;
        }
        let mut out = vec![KIND_RANGE];
        out.extend_from_slice(
            &u16::try_from(payload.len())
                .expect("emitted range fits in u16")
                .to_le_bytes(),
        );
        out.extend(payload);
        out
    }

    fn table(&mut self) -> Option<Vec<u8>> {
        let count = 1 + self.rng.below(8);
        let elem = 1 + self.rng.below(16);
        let bytes = count * elem;
        if self.reserved + bytes > RECORDED_ALLOCATION_LIMIT {
            return None;
        }
        self.reserved += bytes;
        let mut out = vec![KIND_TABLE];
        out.extend_from_slice(&(count as u32).to_le_bytes());
        out.push(elem as u8);
        for _ in 0..bytes {
            out.push(self.rng.next_u64() as u8);
        }
        Some(out)
    }
}

/// A hand-built input plus what the decode of it must observe.
pub struct CraftedCase {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    /// `None` when the case must parse successfully.
    pub expect: Option<ParseErrorKind>,
    /// Allocation ledger left behind by the attempt: a refusal must book
    /// nothing, and a failed attempt must roll back what it had reserved.
    pub expect_used: u64,
}

/// A range node wrapping `inner` in one nesting level.
fn range_wrap(inner: &[u8]) -> Vec<u8> {
    let mut out = vec![KIND_RANGE];
    out.extend_from_slice(
        &u16::try_from(inner.len())
            .expect("crafted chain fits in u16")
            .to_le_bytes(),
    );
    out.extend_from_slice(inner);
    out
}

/// A chain of `wraps` ranges around one empty leaf: `wraps + 1` levels deep.
fn deep_range_chain(wraps: usize) -> Vec<u8> {
    let mut node = vec![KIND_LEAF, 0, 0];
    for _ in 0..wraps {
        node = range_wrap(&node);
    }
    node
}

/// One table node reserving `bytes` payload bytes (`count * elem = bytes`).
fn table_of(bytes: usize) -> Vec<u8> {
    let mut out = vec![KIND_TABLE];
    out.extend_from_slice(&(bytes as u32).to_le_bytes());
    out.push(1);
    out.extend(std::iter::repeat_n(0u8, bytes));
    out
}

/// One table node reserving 64 bytes: `4 * 16`.
fn small_table() -> Vec<u8> {
    let mut out = vec![KIND_TABLE, 4, 0, 0, 0, 16];
    out.extend(std::iter::repeat_n(0u8, 64));
    out
}

/// The hostile inputs every F03-D test runs, each with the refusal the
/// recorded limits say it must produce.
pub fn crafted_cases() -> Vec<CraftedCase> {
    vec![
        CraftedCase {
            name: "empty input is refused, never a silent success",
            bytes: Vec::new(),
            expect: Some(ParseErrorKind::UnexpectedEof),
            expect_used: 0,
        },
        CraftedCase {
            name: "a leaf of four invalid UTF-8 bytes",
            bytes: vec![KIND_LEAF, 4, 0, 0xFF, 0xFF, 0xFF, 0xFF],
            expect: Some(ParseErrorKind::InvalidEncoding),
            expect_used: 0,
        },
        CraftedCase {
            name: "a bounded cstr with no terminator inside its bound",
            bytes: vec![KIND_CSTR, 8, b'h', b'e', b'l', b'l', b'o', b'!', b'?', b'~'],
            expect: Some(ParseErrorKind::MissingTerminator),
            expect_used: 0,
        },
        CraftedCase {
            name: "u32::MAX table refused by the allocation budget, unallocated",
            bytes: vec![KIND_TABLE, 0xFF, 0xFF, 0xFF, 0xFF, 16],
            expect: Some(ParseErrorKind::AllocationBudgetExceeded),
            expect_used: 0,
        },
        CraftedCase {
            name: "range nesting 12 levels deep against a ceiling of 8",
            bytes: deep_range_chain(11),
            expect: Some(ParseErrorKind::RecursionDepthExceeded),
            expect_used: 0,
        },
        CraftedCase {
            name: "table reserves 64 bytes, then its sibling is truncated",
            bytes: {
                let mut inner = small_table();
                inner.extend_from_slice(&[KIND_LEAF, 10, 0, b'x', b'y']);
                range_wrap(&inner)
            },
            expect: Some(ParseErrorKind::UnexpectedEof),
            expect_used: 0,
        },
        CraftedCase {
            name: "a successful table keeps its 64 charged bytes",
            bytes: small_table(),
            expect: None,
            expect_used: 64,
        },
        CraftedCase {
            name: "a table reserving exactly the recorded limit is accepted",
            bytes: table_of(RECORDED_ALLOCATION_LIMIT as usize),
            expect: None,
            expect_used: RECORDED_ALLOCATION_LIMIT,
        },
        CraftedCase {
            name: "one byte past the recorded limit is refused unallocated",
            bytes: table_of(RECORDED_ALLOCATION_LIMIT as usize + 1),
            expect: Some(ParseErrorKind::AllocationBudgetExceeded),
            expect_used: 0,
        },
        CraftedCase {
            name: "nesting exactly at the recorded ceiling still parses",
            bytes: deep_range_chain(7),
            expect: None,
            expect_used: 0,
        },
    ]
}

/// Asserts every bounded field handed back really points into `input`.
///
/// A slice into a copy (or into some other buffer) would fail the range
/// check, and one that claims a different region would fail the byte-for-byte
/// comparison.
pub fn assert_slices_within(name: &str, input: &[u8], slices: &[&[u8]]) {
    let base = input.as_ptr() as usize;
    let end = base.saturating_add(input.len());
    for (index, slice) in slices.iter().enumerate() {
        let start = slice.as_ptr() as usize;
        let finish = start.saturating_add(slice.len());
        assert!(
            start >= base && finish <= end,
            "{name}: slice {index} ({:#x}..{:#x}) escapes the input {:#x}..{:#x}",
            start,
            finish,
            base,
            end,
        );
        let offset = start - base;
        assert_eq!(
            *slice,
            &input[offset..offset + slice.len()],
            "{name}: slice {index} is not the input bytes it claims to be",
        );
    }
}
