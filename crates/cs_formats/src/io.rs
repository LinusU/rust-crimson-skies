//! Bounds-checked little-endian byte reader.
//!
//! Design rules from `specs/F03-bounded-binary-parsing-primitives.md`:
//!
//! 1. Every read is byte-wise with explicit little-endian decoding: no
//!    `unsafe`, no `transmute`, so alignment and host endianness never matter.
//! 2. Every read returns [`ParseError`] instead of panicking when the
//!    remaining bytes are insufficient (acceptance test AC01).
//! 3. Sub-readers keep the container provenance and rebase their offsets, so
//!    a failure inside a nested range still reports its absolute offset.
//!    [`Reader::window`] does the same for a range an absolute offset names,
//!    which is what a table-driven parser needs when a container points into
//!    its own middle: random access uses these primitives instead of a second
//!    private implementation of them.
//! 4. Length arithmetic is checked before any slice is taken, so overflowing
//!    `count * element_size` or `offset + length` fails without allocating.
//! 5. [`AllocationBudget`] and [`RecursionBudget`] are separate limits (spec
//!    non-negotiable #2: independent limits for recursion and allocations).
//!    Both are counters plus checked arithmetic: refusing a hostile count or
//!    an over-deep nest never allocates, and neither budget can be silently
//!    widened — the defaults are designed safety budgets and every other
//!    value is passed in explicitly by the parse that tested it.
//! 6. [`ParseContext::parse`] is the entrypoint every parser runs through: it
//!    hands out one reader and both budgets under a single provenance label,
//!    rolls an attempt's charges back when it fails (so the attempt can be
//!    retried) and stamps the entrypoint's name onto the error as it
//!    propagates out ([`ParseError::in_scope`], stage F03-C). The reader it
//!    hands out borrows the attempt's own bytes, so an attempt returns
//!    bounded fields as slices instead of being forced to copy them.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::error::ParseError;

/// A checked reader over an immutable byte slice.
///
/// The reader never owns or allocates buffer memory: slices are handed out
/// borrow-checked from the input, so a hostile length cannot cause an
/// allocation here. The provenance label is shared ([`Arc`]) rather than
/// copied, so opening a [`Self::window`] inside a container costs no
/// allocation either — a parser that opens one window per field is not
/// quietly allocating per field.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    container: Arc<str>,
    bytes: &'a [u8],
    /// Absolute offset of `bytes[0]` inside `container`.
    base: u64,
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader over `bytes`, whose provenance is the archive/container named
    /// `container` (a display label, never a path that gets joined).
    pub fn new(container: impl Into<String>, bytes: &'a [u8]) -> Self {
        Self {
            container: Arc::from(container.into()),
            bytes,
            base: 0,
            pos: 0,
        }
    }

    /// The archive/container these bytes came from.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Absolute offset of the next read inside the container.
    pub fn position(&self) -> u64 {
        self.base + self.pos as u64
    }

    /// Absolute offset one past the last byte of the current range.
    ///
    /// This is the outer bound every range of this reader is checked against:
    /// a window may not reach past it, and a caller that needs the whole
    /// container's length (for a diagnostic, or to bound a loop over it) has
    /// it here without keeping the input slice a second time.
    pub fn range_end(&self) -> u64 {
        // A range never starts past the container and never claims more bytes
        // than the container holds, so this cannot overflow in practice;
        // saturating keeps a hostile caller from wrapping it either.
        self.base.saturating_add(self.bytes.len() as u64)
    }

    /// Bytes still available in the current range.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Whether the current range has no bytes left.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Advances past `len` bytes without looking at them.
    ///
    /// Fails with [`crate::ParseErrorKind::UnexpectedEof`] when fewer than
    /// `len` bytes remain; `pos + len` cannot overflow because the check runs
    /// against `remaining()` first.
    pub fn skip(&mut self, field: &str, len: usize) -> Result<(), ParseError> {
        self.take(field, len).map(|_| ())
    }

    /// Borrows exactly `len` bytes from the current position.
    ///
    /// The slice points into the original input: no copy and no allocation,
    /// whatever `len` is.
    pub fn read_bytes(&mut self, field: &str, len: usize) -> Result<&'a [u8], ParseError> {
        self.take(field, len)
    }

    /// A nested range over the next `len` bytes.
    ///
    /// The sub-reader inherits the container and reports absolute offsets, so
    /// errors inside a member range stay locatable in the outer archive.
    pub fn sub_reader(&mut self, field: &str, len: usize) -> Result<Reader<'a>, ParseError> {
        let base = self.position();
        let bytes = self.take(field, len)?;
        Ok(Reader {
            container: Arc::clone(&self.container),
            bytes,
            base,
            pos: 0,
        })
    }

    /// A reader over the `len` bytes at the absolute offset `offset` inside
    /// the container.
    ///
    /// [`Self::sub_reader`] opens the range *at the cursor*; this opens the
    /// range an absolute offset names, which is what a table-driven parser
    /// needs when the bytes it wants sit in the middle of the container and
    /// the table that says so was read earlier (a PE resource directory, a
    /// ROF member, a level inside a texture package). The window borrows
    /// `offset..offset + len` of the input — no copy, no allocation, whatever
    /// `len` is — and is read with exactly the same accessors as any other
    /// range, so there is one implementation of the bounded reads rather than
    /// one per parser that needs random access.
    ///
    /// The window's [`Self::position`] is the absolute `offset`, and a window
    /// of a window stays absolute, so a failure inside a nested range is
    /// reported where the bytes are, not where the range began.
    ///
    /// # Refusals
    ///
    /// The window must lie inside the range this reader was handed: a range is
    /// the bound the F03 rules give it, and a window may not reach outside
    /// it. A window that runs past [`Self::range_end`] is refused with
    /// [`crate::ParseErrorKind::UnexpectedEof`] at the window's own absolute
    /// `offset`, expecting `len` bytes and reporting how many of them the
    /// range holds — the same numbers [`Self::skip`] reports for the same read
    /// at the same offset, so a caller that refuses a window and a caller
    /// that refuses a skip describe one condition identically. Nothing is
    /// clamped to the end of the input and no short window is ever handed
    /// back: a hostile offset costs the caller the error and no bytes. An
    /// `offset + len` that overflows is a
    /// [`crate::ParseErrorKind::LengthOverflow`], checked before anything is
    /// sliced.
    ///
    /// ```
    /// use cs_formats::{ParseError, Reader};
    ///
    /// let bytes = [10u8, 20, 30, 40];
    /// let mut second = Reader::new("record", &bytes).window(1, 2, "block")?;
    /// assert_eq!(second.position(), 1, "a window reports absolute offsets");
    /// assert_eq!(second.read_u8("block.value")?, 20);
    ///
    /// // The same refusal a `skip` past the end of the container gives.
    /// let refused = Reader::new("record", &bytes).window(3, 2, "block");
    /// assert_eq!(refused.unwrap_err().offset, 3);
    /// # Ok::<(), ParseError>(())
    /// ```
    pub fn window(&self, offset: u64, len: u64, field: &str) -> Result<Reader<'a>, ParseError> {
        let range = self.window_range(offset, len, field)?;
        Ok(Self {
            container: Arc::clone(&self.container),
            bytes: &self.bytes[range],
            base: offset,
            pos: 0,
        })
    }

    /// The `len` bytes at the absolute offset `offset`, as a borrow of the
    /// input.
    ///
    /// This is [`Self::window`]'s bounds check handed out as the range itself,
    /// for the caller that wants the bytes of a whole record (a table, a
    /// member payload) rather than sequential fields through a reader. The
    /// slice points into the original input, so nothing is copied and nothing
    /// is allocated on the success path.
    ///
    /// ```
    /// use cs_formats::{ParseError, Reader};
    ///
    /// let bytes = [10u8, 20, 30, 40];
    /// let reader = Reader::new("record", &bytes);
    /// assert_eq!(reader.window_bytes(2, 2, "block")?, &bytes[2..4]);
    /// assert!(reader.window_bytes(3, 2, "block").is_err());
    /// # Ok::<(), ParseError>(())
    /// ```
    pub fn window_bytes(&self, offset: u64, len: u64, field: &str) -> Result<&'a [u8], ParseError> {
        let range = self.window_range(offset, len, field)?;
        Ok(&self.bytes[range])
    }

    /// Reads one little-endian `u8`.
    pub fn read_u8(&mut self, field: &str) -> Result<u8, ParseError> {
        Ok(self.read_le::<1>(field)?[0])
    }

    /// Reads one little-endian `u16`.
    pub fn read_u16(&mut self, field: &str) -> Result<u16, ParseError> {
        Ok(u16::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian `u32`.
    pub fn read_u32(&mut self, field: &str) -> Result<u32, ParseError> {
        Ok(u32::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian `u64`.
    pub fn read_u64(&mut self, field: &str) -> Result<u64, ParseError> {
        Ok(u64::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian `i8`.
    pub fn read_i8(&mut self, field: &str) -> Result<i8, ParseError> {
        Ok(i8::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian `i16`.
    pub fn read_i16(&mut self, field: &str) -> Result<i16, ParseError> {
        Ok(i16::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian `i32`.
    pub fn read_i32(&mut self, field: &str) -> Result<i32, ParseError> {
        Ok(i32::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian `i64`.
    pub fn read_i64(&mut self, field: &str) -> Result<i64, ParseError> {
        Ok(i64::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian IEEE-754 `f32`.
    ///
    /// Any bit pattern is a valid `f32`, so this cannot fail on content;
    /// finiteness is validated where the value is consumed.
    pub fn read_f32(&mut self, field: &str) -> Result<f32, ParseError> {
        Ok(f32::from_le_bytes(self.read_le(field)?))
    }

    /// Reads one little-endian IEEE-754 `f64`.
    ///
    /// See [`Self::read_f32`] for the finiteness note.
    pub fn read_f64(&mut self, field: &str) -> Result<f64, ParseError> {
        Ok(f64::from_le_bytes(self.read_le(field)?))
    }

    /// Reads exactly `len` bytes and decodes them as UTF-8.
    ///
    /// `len` is the bound: the read never consumes more, whatever the content.
    pub fn read_str(&mut self, field: &str, len: usize) -> Result<&'a str, ParseError> {
        let start = self.position();
        let bytes = self.take(field, len)?;
        std::str::from_utf8(bytes).map_err(|e| {
            ParseError::invalid_encoding(
                self.container.to_string(),
                start + e.valid_up_to() as u64,
                field,
                e.valid_up_to(),
            )
        })
    }

    /// Reads a string field of exactly `max_len` bytes whose first `0x00` byte
    /// terminates the text.
    ///
    /// The whole bounded field must be present (so truncation is detected, not
    /// silently accepted as a shorter string), a terminator must occur inside
    /// it, and the prefix must be valid UTF-8.
    pub fn read_bounded_cstr(
        &mut self,
        field: &str,
        max_len: usize,
    ) -> Result<&'a str, ParseError> {
        let start = self.position();
        let bytes = self.take(field, max_len)?;
        let end = bytes.iter().position(|&b| b == 0).ok_or_else(|| {
            ParseError::missing_terminator(self.container.to_string(), start, field, max_len as u64)
        })?;
        let text = &bytes[..end];
        std::str::from_utf8(text).map_err(|e| {
            ParseError::invalid_encoding(
                self.container.to_string(),
                start + e.valid_up_to() as u64,
                field,
                e.valid_up_to(),
            )
        })
    }

    /// Checks `count * element_size` without allocating anything.
    ///
    /// Returns the byte length so a caller can bound a later read; overflow of
    /// the product (or of `usize`) is a [`crate::ParseErrorKind::
    /// LengthOverflow`] at the current position.
    pub fn checked_byte_len(
        &self,
        field: &str,
        count: u64,
        element_size: u64,
    ) -> Result<usize, ParseError> {
        count
            .checked_mul(element_size)
            .and_then(|total| usize::try_from(total).ok())
            .ok_or_else(|| {
                ParseError::length_overflow(
                    self.container.to_string(),
                    self.position(),
                    field,
                    "count * element_size to fit in usize".to_owned(),
                    format!("count {count} times element_size {element_size}"),
                )
            })
    }

    /// Checks `offset + length` for a range described by an absolute offset
    /// and a length, without touching the bytes.
    ///
    /// Returns the exclusive end offset. Overflow is a
    /// [`crate::ParseErrorKind::LengthOverflow`].
    pub fn checked_extent(&self, field: &str, offset: u64, len: u64) -> Result<u64, ParseError> {
        offset.checked_add(len).ok_or_else(|| {
            ParseError::length_overflow(
                self.container.to_string(),
                self.position(),
                field,
                "offset + length to fit in u64".to_owned(),
                format!("offset {offset} plus length {len}"),
            )
        })
    }

    /// The half-open range of `bytes` the window `[offset, offset + len)` is,
    /// with both ends proven to be inside this reader's range.
    ///
    /// This is the one bounds check behind [`Self::window`] and
    /// [`Self::window_bytes`], and it is deliberately the same check
    /// [`Self::take`] performs, at the same anchor: overflow of
    /// `offset + len` is a [`crate::ParseErrorKind::LengthOverflow`] before
    /// anything is sliced, and a range this reader does not hold is a
    /// [`crate::ParseErrorKind::UnexpectedEof`] reporting how many of the
    /// requested bytes the range actually offers.
    fn window_range(&self, offset: u64, len: u64, field: &str) -> Result<Range<usize>, ParseError> {
        let end = offset.checked_add(len).ok_or_else(|| {
            ParseError::length_overflow(
                self.container.to_string(),
                offset,
                field,
                "offset + length to fit in u64".to_owned(),
                format!("offset {offset} plus length {len}"),
            )
        })?;
        let range_end = self.range_end();
        if end > range_end {
            // The window is refused where it was asked for. How many of the
            // requested bytes this reader can offer is counted inside its own
            // range: everything left from `offset` when the window starts
            // inside it (which is what a `skip` at `offset` reports), and only
            // the part that overlaps when a window asks for bytes before the
            // range it was given.
            let available = end.min(range_end).saturating_sub(offset.max(self.base));
            return Err(ParseError::unexpected_eof(
                self.container.to_string(),
                offset,
                field,
                len,
                available,
            ));
        }
        // `end` is inside `bytes`, so both bounds are valid `usize` indices:
        // the conversions below cannot truncate and the slice cannot panic.
        let end = usize::try_from(end).expect("a checked window end fits in usize");
        let start = end - usize::try_from(len).expect("a checked window length fits in usize");
        Ok(start..end)
    }

    fn take(&mut self, field: &str, len: usize) -> Result<&'a [u8], ParseError> {
        let available = self.remaining();
        if available < len {
            return Err(ParseError::unexpected_eof(
                self.container.to_string(),
                self.position(),
                field,
                len as u64,
                available as u64,
            ));
        }
        let start = self.pos;
        self.pos += len;
        let bytes = self.bytes;
        Ok(&bytes[start..self.pos])
    }

    fn read_le<const N: usize>(&mut self, field: &str) -> Result<[u8; N], ParseError> {
        let chunk = self.take(field, N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(chunk);
        Ok(out)
    }
}

/// An independent allocation budget for one parse.
///
/// `specs/F03-bounded-binary-parsing-primitives.md` non-negotiable #2 demands
/// *independent* limits per safety dimension; this one bounds the bytes a
/// parse may hand out for decoded records (index tables, texture pixels, mesh
/// buffers). Recursion has its own budget ([`RecursionBudget`]), so exhausting
/// one never silently relaxes the other.
///
/// [`Self::reserve`] and [`Self::reserve_extent`] are arithmetic over two
/// counters: `count * element_size` and `offset + length` are computed with
/// checked math and compared against what is left of the budget. No buffer is
/// created on either path, so a `u32::MAX` count read from hostile bytes costs
/// nothing but the [`ParseError`] it returns. Charging happens only on
/// success: a refused request leaves the budget exactly as it was.
///
/// A budget belongs to one parse: it is constructed at zero use, and a clone
/// is an independent ledger that carries the same `limit` and the same `used`
/// bytes — never a shared counter and never a wider allowance — so no code
/// path can quietly enlarge what one parse may allocate.
///
/// `used` has exactly three writers: [`Self::reserve`], [`Self::reserve_extent`]
/// and the rollback [`ParseContext::parse`] performs when an attempt fails
/// (the attempt's buffers are released as it returns, so its charges must not
/// be booked against a retry). All three can only move `used` towards, and
/// never past, `limit`.
#[derive(Clone, Debug)]
pub struct AllocationBudget {
    /// Provenance label carried by this budget's errors.
    container: String,
    /// Designed ceiling on bytes reserved through this budget.
    limit: u64,
    /// Bytes reserved so far; never exceeds `limit`.
    used: u64,
}

impl AllocationBudget {
    /// The designed default budget: 64 MiB per parse.
    ///
    /// Large enough for the biggest decoded buffers the engine is expected to
    /// need (a 4096² RGBA8 texture is 64 MiB), small enough that a hostile
    /// count is refused long before it becomes a real allocation. Like every
    /// default here it is a *designed* budget, not a measured original value.
    /// Any other limit must be passed explicitly to [`Self::new`] by code
    /// that tests it.
    pub const DEFAULT_LIMIT: u64 = 64 * 1024 * 1024;

    /// A budget of `limit` bytes whose errors name `container`.
    ///
    /// `limit` is the tested configuration surface: `0` refuses everything,
    /// an exactly fitting request is accepted, one byte more is refused.
    pub fn new(container: impl Into<String>, limit: u64) -> Self {
        Self {
            container: container.into(),
            limit,
            used: 0,
        }
    }

    /// A budget at [`Self::DEFAULT_LIMIT`].
    pub fn with_defaults(container: impl Into<String>) -> Self {
        Self::new(container, Self::DEFAULT_LIMIT)
    }

    /// The ceiling this budget enforces.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Bytes reserved so far.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Bytes still reservable.
    pub fn remaining(&self) -> u64 {
        self.limit - self.used
    }

    /// Checks and books `count * element_size` bytes against the budget.
    ///
    /// Returns the byte length so the caller can bound a later read. Overflow
    /// of the product (or of `usize`) is a
    /// [`crate::ParseErrorKind::LengthOverflow`]; a product beyond what is
    /// left of the budget is a [`crate::ParseErrorKind::
    /// AllocationBudgetExceeded`]. `offset` is the anchor reported in the
    /// error (the caller's current reader position, or the start of the range
    /// the count describes).
    ///
    /// Nothing is allocated and nothing is charged unless the call succeeds.
    pub fn reserve(
        &mut self,
        field: &str,
        offset: u64,
        count: u64,
        element_size: u64,
    ) -> Result<usize, ParseError> {
        let bytes = count
            .checked_mul(element_size)
            .and_then(|total| usize::try_from(total).ok())
            .ok_or_else(|| {
                ParseError::length_overflow(
                    self.container.clone(),
                    offset,
                    field,
                    "count * element_size to fit in usize".to_owned(),
                    format!("count {count} times element_size {element_size}"),
                )
            })?;
        self.charge(field, offset, bytes as u64)?;
        Ok(bytes)
    }

    /// Checks and books the bytes of an absolute range, without touching it.
    ///
    /// Returns the exclusive end offset (`offset + len`), so a caller that
    /// allocates a buffer for a member range gets both checks in one call:
    /// overflow of the extent is a [`crate::ParseErrorKind::LengthOverflow`],
    /// an extent beyond the remaining budget is a
    /// [`crate::ParseErrorKind::AllocationBudgetExceeded`].
    pub fn reserve_extent(
        &mut self,
        field: &str,
        offset: u64,
        len: u64,
    ) -> Result<u64, ParseError> {
        let end = offset.checked_add(len).ok_or_else(|| {
            ParseError::length_overflow(
                self.container.clone(),
                offset,
                field,
                "offset + length to fit in u64".to_owned(),
                format!("offset {offset} plus length {len}"),
            )
        })?;
        self.charge(field, offset, len)?;
        Ok(end)
    }

    fn charge(&mut self, field: &str, offset: u64, bytes: u64) -> Result<(), ParseError> {
        let available = self.remaining();
        if bytes > available {
            return Err(ParseError::allocation_budget_exceeded(
                self.container.clone(),
                offset,
                field,
                available,
                bytes,
                self.limit,
            ));
        }
        self.used += bytes;
        Ok(())
    }

    /// Restores the ledger to a use mark taken earlier from [`Self::used`].
    ///
    /// The only caller is [`ParseContext::parse`]: when an attempt fails, the
    /// buffers it reserved are locals of that attempt and are released as it
    /// returns, so the bytes it charged must not stay booked against a later,
    /// honest attempt (F03-C's teardown/retry). The mark always comes from
    /// this same budget's earlier state, and the update only ever moves the
    /// ledger *backwards*, so `used <= limit` still holds and no allowance is
    /// widened — a refused reservation is still never charged, and a
    /// successful attempt is still never rolled back.
    fn rollback_to(&mut self, mark: u64) {
        if mark < self.used {
            self.used = mark;
        }
    }
}

/// An independent recursion limit for one parse.
///
/// Deeply nested directories, scene graphs or archives must not walk the
/// stack until it overflows, and a cyclic member list must terminate
/// (non-negotiable #2's recursion limit and non-negotiable #3's directory
/// cycles). [`Self::enter`] costs one checked increment and returns a
/// [`RecursionGuard`] that decrements the depth when it drops — on the
/// success path and on the `?` error path alike.
///
/// [`Self::enter`] borrows only `&self`, so a recursive parser can hold a
/// guard and still hand the same budget to its nested call:
///
/// ```
/// use cs_formats::{ParseError, RecursionBudget, Reader};
///
/// fn depth_of(reader: &mut Reader<'_>, budget: &RecursionBudget) -> Result<u32, ParseError> {
///     let _guard = budget.enter("node", reader.position())?;
///     let children = reader.read_u8("node.children")?;
///     let mut total = 1;
///     for _ in 0..children {
///         total += depth_of(reader, budget)?;
///     }
///     Ok(total)
/// }
/// ```
#[derive(Debug)]
pub struct RecursionBudget {
    /// Provenance label carried by this budget's errors.
    container: String,
    /// Designed ceiling on simultaneous nesting levels.
    max_depth: u32,
    /// Levels currently entered. Atomic so a guard can release the level it
    /// took without needing `&mut` on the budget a recursive caller holds.
    depth: AtomicU32,
}

impl RecursionBudget {
    /// The designed default limit: 32 nested levels.
    ///
    /// Deep enough for the deepest legitimate nesting the engine's content
    /// model is expected to need, shallow enough that a hostile or cyclic
    /// structure fails fast instead of overflowing the stack. This is a
    /// *designed* budget (EvidenceClass `Designed`): no original game value is
    /// observed or implied. Any other limit must be passed explicitly to
    /// [`Self::new`] by code that tests it.
    pub const DEFAULT_MAX_DEPTH: u32 = 32;

    /// A budget of `max_depth` levels whose errors name `container`.
    ///
    /// `max_depth` is the tested configuration surface: `0` refuses the first
    /// `enter`, level `max_depth` is accepted and level `max_depth + 1` is
    /// refused.
    pub fn new(container: impl Into<String>, max_depth: u32) -> Self {
        Self {
            container: container.into(),
            max_depth,
            depth: AtomicU32::new(0),
        }
    }

    /// A budget at [`Self::DEFAULT_MAX_DEPTH`].
    pub fn with_defaults(container: impl Into<String>) -> Self {
        Self::new(container, Self::DEFAULT_MAX_DEPTH)
    }

    /// The nesting ceiling this budget enforces.
    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }

    /// Levels currently entered.
    pub fn depth(&self) -> u32 {
        self.depth.load(Ordering::Relaxed)
    }

    /// Enters one nesting level, or fails when the ceiling is reached.
    ///
    /// `offset` is the anchor reported in the error: the position of the
    /// member being descended into. The returned guard releases the level when
    /// it drops, so depth cannot leak across sibling entries or error paths.
    pub fn enter(&self, field: &str, offset: u64) -> Result<RecursionGuard<'_>, ParseError> {
        match self
            .depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
                if depth < self.max_depth {
                    Some(depth + 1)
                } else {
                    None
                }
            }) {
            Ok(previous) => Ok(RecursionGuard {
                budget: self,
                level: previous + 1,
            }),
            Err(previous) => Err(ParseError::recursion_depth_exceeded(
                self.container.clone(),
                offset,
                field,
                self.max_depth,
                u64::from(previous) + 1,
            )),
        }
    }
}

/// The level taken by [`RecursionBudget::enter`], released on drop.
///
/// Holding the guard is what keeps the depth accounting correct: an early
/// `return`, a `?` or a panic all release the level exactly once.
#[derive(Debug)]
pub struct RecursionGuard<'a> {
    budget: &'a RecursionBudget,
    level: u32,
}

impl RecursionGuard<'_> {
    /// The 1-based nesting level this guard entered (1 is the outermost).
    pub fn level(&self) -> u32 {
        self.level
    }
}

impl Drop for RecursionGuard<'_> {
    fn drop(&mut self) {
        self.budget.depth.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The shared context of one parse: provenance plus both independent budgets.
///
/// A parse has exactly one container label, so the reader and the budgets an
/// attempt uses all report failures against the same archive name instead of
/// three strings a caller had to keep in step. [`Self::new`] takes both limits
/// explicitly (the tested configuration surface); [`Self::with_defaults`] uses
/// the designed [`AllocationBudget::DEFAULT_LIMIT`] and
/// [`RecursionBudget::DEFAULT_MAX_DEPTH`].
///
/// [`Self::parse`] is the entrypoint through which a parser runs:
///
/// * the reader, the allocation budget and the recursion budget are handed to
///   one attempt as three *separate* references, so holding a
///   [`RecursionGuard`] never blocks a reservation and vice versa — the
///   nesting that non-negotiable #2 and #3 require of a real parser;
/// * the reader borrows the attempt's own bytes, so an attempt can return a
///   bounded field as a slice of the input instead of a heap copy of it;
/// * a failed attempt rolls its allocation charges back to what the ledger
///   held before it and leaves the recursion depth at zero (its guards drop
///   with it), so the same context can retry the bytes honestly;
/// * an error crossing the entrypoint is scoped with its name
///   ([`ParseError::in_scope`]), keeping container, absolute offset and the
///   expected/observed conditions intact.
///
/// This is engineered plumbing (EvidenceClass `Designed`): no original file
/// layout, limit or game rule is implied by it.
///
/// ```
/// use cs_formats::{ParseContext, ParseError};
///
/// fn header_magic(context: &mut ParseContext, bytes: &[u8]) -> Result<u32, ParseError> {
///     context.parse("record", bytes, |reader, _allocation, _recursion| {
///         reader.read_u32("header.magic")
///     })
/// }
/// ```
#[derive(Debug)]
pub struct ParseContext {
    /// Provenance label shared by this parse's reader and budgets.
    container: String,
    allocation: AllocationBudget,
    recursion: RecursionBudget,
}

impl ParseContext {
    /// A context for one container with explicit limits: `allocation_limit`
    /// bytes (see [`AllocationBudget::new`]) and `max_depth` nested levels
    /// (see [`RecursionBudget::new`]). Both are passed by the parse that
    /// tested them; neither has a silent fallback.
    pub fn new(container: impl Into<String>, allocation_limit: u64, max_depth: u32) -> Self {
        let container = container.into();
        Self {
            allocation: AllocationBudget::new(container.clone(), allocation_limit),
            recursion: RecursionBudget::new(container.clone(), max_depth),
            container,
        }
    }

    /// A context at the two designed defaults.
    pub fn with_defaults(container: impl Into<String>) -> Self {
        let container = container.into();
        Self {
            allocation: AllocationBudget::with_defaults(container.clone()),
            recursion: RecursionBudget::with_defaults(container.clone()),
            container,
        }
    }

    /// The archive/container every error of this parse reports.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// This parse's allocation ledger, for inspection
    /// ([`AllocationBudget::limit`], [`AllocationBudget::used`],
    /// [`AllocationBudget::remaining`]).
    pub fn allocation(&self) -> &AllocationBudget {
        &self.allocation
    }

    /// This parse's recursion ledger, for inspection
    /// ([`RecursionBudget::max_depth`], [`RecursionBudget::depth`]).
    pub fn recursion(&self) -> &RecursionBudget {
        &self.recursion
    }

    /// Runs one attempt of the parser entrypoint named `entrypoint` over
    /// `bytes`.
    ///
    /// The closure is the parser: it gets a reader over `bytes` carrying this
    /// context's container label, this parse's allocation budget and this
    /// parse's recursion budget as three independent references, so it can
    /// hold a [`RecursionGuard`] across a [`AllocationBudget::reserve`]
    /// without the borrow checker forcing one limit to be dropped for the
    /// other. Nothing is charged for entering: only the reservations the
    /// attempt actually makes are booked.
    ///
    /// When the attempt returns `Err`, the context is left ready for a retry:
    ///
    /// * the attempt's guards and buffers are gone with it (Rust drops the
    ///   closure's locals when it returns), so the recursion depth is whatever
    ///   it was before the attempt;
    /// * its reservation charges are rolled back to the ledger mark taken on
    ///   entry, so a failed attempt cannot drain the budget of a later,
    ///   honest one — the rollback assumes the attempt kept nothing it
    ///   allocated, which is what its locals being dropped gives it;
    /// * the error is scoped with `entrypoint` and then propagated unchanged
    ///   otherwise: same container, same absolute offset, same kind and
    ///   conditions.
    ///
    /// The reader is tied to the lifetime of `bytes`, so an attempt can hand
    /// borrowed input straight back out — [`Reader::read_bytes`],
    /// [`Reader::read_str`] and [`Reader::sub_reader`] return slices into the
    /// bytes, not copies of them. A parser therefore never has to copy a
    /// bounded field onto the heap just to return it; the buffers it does
    /// allocate are the ones it reserves against the budget.
    ///
    /// A successful attempt keeps its charges: the ledger of one parse only
    /// accumulates, and a refused reservation was never charged in the first
    /// place.
    pub fn parse<'bytes, T>(
        &mut self,
        entrypoint: &str,
        bytes: &'bytes [u8],
        f: impl FnOnce(
            &mut Reader<'bytes>,
            &mut AllocationBudget,
            &RecursionBudget,
        ) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let mark = self.allocation.used();
        let mut reader = Reader::new(self.container.clone(), bytes);
        match f(&mut reader, &mut self.allocation, &self.recursion) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.allocation.rollback_to(mark);
                Err(error.in_scope(entrypoint))
            }
        }
    }
}
