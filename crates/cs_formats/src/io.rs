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
