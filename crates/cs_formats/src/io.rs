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
//! 4. Length arithmetic is checked before any slice is taken, so overflowing
//!    `count * element_size` or `offset + length` fails without allocating.
//! 5. [`AllocationBudget`] and [`RecursionBudget`] are separate limits (spec
//!    non-negotiable #2: independent limits for recursion and allocations).
//!    Both are counters plus checked arithmetic: refusing a hostile count or
//!    an over-deep nest never allocates, and neither budget can be silently
//!    widened — the defaults are designed safety budgets and every other
//!    value is passed in explicitly by the parse that tested it.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::error::ParseError;

/// A checked reader over an immutable byte slice.
///
/// The reader never owns or allocates buffer memory: slices are handed out
/// borrow-checked from the input, so a hostile length cannot cause an
/// allocation here.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    container: String,
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
            container: container.into(),
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
            container: self.container.clone(),
            bytes,
            base,
            pos: 0,
        })
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
                self.container.clone(),
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
            ParseError::missing_terminator(self.container.clone(), start, field, max_len as u64)
        })?;
        let text = &bytes[..end];
        std::str::from_utf8(text).map_err(|e| {
            ParseError::invalid_encoding(
                self.container.clone(),
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
                    self.container.clone(),
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
                self.container.clone(),
                self.position(),
                field,
                "offset + length to fit in u64".to_owned(),
                format!("offset {offset} plus length {len}"),
            )
        })
    }

    fn take(&mut self, field: &str, len: usize) -> Result<&'a [u8], ParseError> {
        let available = self.remaining();
        if available < len {
            return Err(ParseError::unexpected_eof(
                self.container.clone(),
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
