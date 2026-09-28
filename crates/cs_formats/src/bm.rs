//! BM multilayer livery images: observed layout, planes kept separate.
//!
//! Spec `specs/F09-bm-multilayer-liveries-and-paint-composition.md`
//! describes the observed BM subset ([S09], [S10],
//! `docs/research/FORMAT-NOTES.md`, "BM observed subset"):
//!
//! * a 4-byte header: little-endian `u16` **height**, then `u16` **width**
//!   (height first — the easiest field order to get wrong);
//! * with `N = width * height`, five planes back to back: RGB base (`3N`),
//!   mask 1 (`N`), mask 2 (`N`), mask 3 (`N`) and an RGBA overlay (`4N`);
//! * so the covered length is exactly `4 + 10N` bytes.
//!
//! This stage (**F09-A**) reads that layout and nothing more:
//!
//! * every read runs through [`ParseContext`], so a truncated header or
//!   plane is a [`ParseError`] naming the short plane (`bm.overlay`) and its
//!   absolute offset, never a panic;
//! * the planes stay separate borrowed slices in stored order
//!   ([`BmFile::stored_plane`]); the overlay is *not* called specular: the
//!   extractor's file name for it proves nothing about its physical meaning
//!   (spec F09 deliverable paragraph);
//! * bytes after the covered length are not an error and not silently
//!   dropped: they are kept as a [`BmUnsupportedTail`] variant diagnostic
//!   (non-negotiable #1), so a later stage can see the file is outside the
//!   observed subset;
//! * the canonical orientation is fixed — texel `(x, y)` is column `x` from
//!   the left and row `y` from the top, as in F08's `DecodedImage`. The
//!   stored row order is [`BM_STORED_ROW_ORDER`] (bottom-up, see there), and
//!   [`BmFile::sample`] is the single place that turns it into the canonical
//!   one, so a missing or doubled flip is a wrong texel at a known
//!   coordinate.
//!
//! Composition (mask-weighted colors, overlay alpha) is F09-B; paint
//! catalogs and material variants are F09-C/F09-D. No color-space or alpha
//! interpretation happens here: values are returned exactly as stored.
//!
//! Every fixture exercised below is newly authored synthetic bytes; nothing
//! here is derived from original game data.
//!
//! ```
//! use cs_formats::{read_bm, BmPlane, ParseContext};
//!
//! // A 1-wide, 2-high image: header is height, then width.
//! let mut bytes = Vec::new();
//! bytes.extend_from_slice(&2u16.to_le_bytes()); // height
//! bytes.extend_from_slice(&1u16.to_le_bytes()); // width
//! bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6]); // base, stored bottom row first
//! bytes.extend_from_slice(&[10, 11]); // mask 1
//! bytes.extend_from_slice(&[20, 21]); // mask 2
//! bytes.extend_from_slice(&[30, 31]); // mask 3
//! bytes.extend_from_slice(&[40, 41, 42, 43, 50, 51, 52, 53]); // overlay
//!
//! let mut context = ParseContext::with_defaults("synthetic/bm_doc.bm");
//! let file = read_bm(&mut context, &bytes).expect("the image is valid");
//! assert_eq!((file.width(), file.height()), (1, 2));
//! // The top canonical row is the last stored row.
//! assert_eq!(file.base(0, 0), Some([4, 5, 6]));
//! assert_eq!(file.sample(BmPlane::Mask3, 0, 1), Some([30].as_slice()));
//! assert!(file.tail().is_none());
//! ```
//!
//! [S09]: https://github.com/rozab/crimsonskies2blend/blob/main/extract_bm.py
//! [S10]: https://github.com/rozab/crimsonskies2blend/blob/main/set_paintjob.py

use std::fmt;

use crate::error::ParseError;
use crate::io::ParseContext;
use crate::texture::RowOrder;

/// Error scope stamped onto failures raised inside [`read_bm`].
pub const BM_ENTRYPOINT: &str = "bm";

/// Bytes in the header: `u16` height, `u16` width.
pub const BM_HEADER_BYTES: usize = 4;

/// Stored bytes per pixel over all five planes: 3 base + 3 masks + 4
/// overlay.
pub const BM_BYTES_PER_PIXEL: usize = 10;

/// Row order of every stored plane.
///
/// Claim class *observed tool*: the pinned extractor [S09] builds each plane
/// with the first stored row as the image's top row and then flips it
/// top-to-bottom before saving, i.e. it treats the first stored row as the
/// **bottom** row. Not yet matched against the original renderer (F09-D);
/// see `docs/findings/2026-09-28-f09-a-bm-layout-and-rectangular-fixture.md`.
///
/// [S09]: https://github.com/rozab/crimsonskies2blend/blob/main/extract_bm.py
pub const BM_STORED_ROW_ORDER: RowOrder = RowOrder::BottomUp;

/// One of the five stored planes, in stored order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BmPlane {
    /// RGB base color, 3 bytes per pixel.
    Base,
    /// First 8-bit paint mask.
    Mask1,
    /// Second 8-bit paint mask.
    Mask2,
    /// Third 8-bit paint mask.
    Mask3,
    /// Final RGBA layer, 4 bytes per pixel. The extractor names it
    /// "specular"; that name is not evidence of physically based specular
    /// semantics, so it is called an overlay here.
    Overlay,
}

impl BmPlane {
    /// All planes in stored order.
    pub const ALL: [Self; 5] = [
        Self::Base,
        Self::Mask1,
        Self::Mask2,
        Self::Mask3,
        Self::Overlay,
    ];

    /// Bytes per pixel in this plane.
    pub const fn channels(self) -> usize {
        match self {
            Self::Base => 3,
            Self::Mask1 | Self::Mask2 | Self::Mask3 => 1,
            Self::Overlay => 4,
        }
    }

    /// Stable lowercase name, also the logical field of read errors
    /// (`bm.<name>`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Mask1 => "mask1",
            Self::Mask2 => "mask2",
            Self::Mask3 => "mask3",
            Self::Overlay => "overlay",
        }
    }

    const fn position(self) -> usize {
        match self {
            Self::Base => 0,
            Self::Mask1 => 1,
            Self::Mask2 => 2,
            Self::Mask3 => 3,
            Self::Overlay => 4,
        }
    }
}

/// The two little-endian `u16` header fields, in on-disk order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmRawHeader {
    /// First field: rows.
    pub height: u16,
    /// Second field: columns.
    pub width: u16,
}

/// Bytes after the covered `4 + 10N` length: outside the observed subset,
/// kept verbatim as a variant diagnostic instead of being dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmUnsupportedTail<'a> {
    /// Absolute offset of the first uncovered byte (the covered length).
    pub offset: u64,
    /// The uncovered bytes, borrowed.
    pub bytes: &'a [u8],
}

/// A BM image in the observed subset, planes borrowed in stored order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmFile<'a> {
    header: BmRawHeader,
    planes: [&'a [u8]; 5],
    offsets: [u64; 5],
    tail: Option<BmUnsupportedTail<'a>>,
}

impl<'a> BmFile<'a> {
    /// The header as stored.
    pub fn header(&self) -> BmRawHeader {
        self.header
    }

    /// Columns.
    pub fn width(&self) -> u32 {
        u32::from(self.header.width)
    }

    /// Rows.
    pub fn height(&self) -> u32 {
        u32::from(self.header.height)
    }

    /// Bytes the observed subset accounts for: `4 + 10 * width * height`.
    pub fn covered_len(&self) -> u64 {
        BM_HEADER_BYTES as u64 + BM_BYTES_PER_PIXEL as u64 * self.pixel_count()
    }

    /// The bytes of `plane` exactly as stored (stored row order, see
    /// [`BM_STORED_ROW_ORDER`]).
    pub fn stored_plane(&self, plane: BmPlane) -> &'a [u8] {
        self.planes[plane.position()]
    }

    /// Absolute offset of `plane`'s first stored byte.
    pub fn plane_offset(&self, plane: BmPlane) -> u64 {
        self.offsets[plane.position()]
    }

    /// Uncovered trailing bytes, if any. `None` means the input was exactly
    /// the covered length.
    pub fn tail(&self) -> Option<BmUnsupportedTail<'a>> {
        self.tail
    }

    /// The channels of `plane` at canonical texel `(x, y)` — `x` from the
    /// left, `y` from the top — or `None` outside the image.
    ///
    /// This is the only place the stored row order is applied.
    pub fn sample(&self, plane: BmPlane, x: u32, y: u32) -> Option<&'a [u8]> {
        if x >= self.width() || y >= self.height() {
            return None;
        }
        let stored_row = match BM_STORED_ROW_ORDER {
            RowOrder::TopDown => y,
            RowOrder::BottomUp => self.height() - 1 - y,
        };
        // Both factors are below 2^16, so the index fits in usize.
        let pixel = stored_row as usize * self.width() as usize + x as usize;
        let channels = plane.channels();
        self.stored_plane(plane)
            .get(pixel * channels..(pixel + 1) * channels)
    }

    /// Base RGB at canonical texel `(x, y)`.
    pub fn base(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        self.sample(BmPlane::Base, x, y).map(|c| [c[0], c[1], c[2]])
    }

    /// Mask value of `plane` at canonical texel `(x, y)`; `None` outside the
    /// image or when `plane` is not one of the three masks.
    pub fn mask(&self, plane: BmPlane, x: u32, y: u32) -> Option<u8> {
        match plane {
            BmPlane::Mask1 | BmPlane::Mask2 | BmPlane::Mask3 => {
                self.sample(plane, x, y).map(|c| c[0])
            }
            BmPlane::Base | BmPlane::Overlay => None,
        }
    }

    /// Overlay RGBA at canonical texel `(x, y)`, alpha as stored.
    pub fn overlay(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        self.sample(BmPlane::Overlay, x, y)
            .map(|c| [c[0], c[1], c[2], c[3]])
    }

    fn pixel_count(&self) -> u64 {
        u64::from(self.header.width) * u64::from(self.header.height)
    }
}

/// Why bytes are not a BM image in the observed subset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BmError {
    /// A structural failure from the checked reader, scoped as
    /// `bm.<field>`: a short header or plane is
    /// [`crate::ParseErrorKind::UnexpectedEof`].
    Parse(ParseError),
    /// Width or height is zero. The observed subset has no empty image, so
    /// such a header is reported as unsupported rather than accepted as a
    /// 4-byte image.
    EmptyImage {
        /// Container label the bytes came from.
        container: String,
        /// Stored height.
        height: u16,
        /// Stored width.
        width: u16,
    },
}

impl BmError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(error) => error.kind.as_str(),
            Self::EmptyImage { .. } => "empty_image",
        }
    }

    /// The container label the bytes came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::EmptyImage { container, .. } => container,
        }
    }
}

impl From<ParseError> for BmError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for BmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::EmptyImage {
                container,
                height,
                width,
            } => write!(
                f,
                "{container}: header declares a {width}x{height} image; an empty BM is \
                 outside the observed subset"
            ),
        }
    }
}

impl std::error::Error for BmError {}

/// Reads a BM image in the observed subset from `bytes`.
///
/// The planes are borrowed, not copied, so nothing is charged to the
/// context's allocation budget. Bytes beyond the covered length are kept as
/// [`BmFile::tail`].
pub fn read_bm<'bytes>(
    context: &mut ParseContext,
    bytes: &'bytes [u8],
) -> Result<BmFile<'bytes>, BmError> {
    context.parse(BM_ENTRYPOINT, bytes, |reader, _allocation, _recursion| {
        let height = reader.read_u16("header.height")?;
        let width = reader.read_u16("header.width")?;
        let header = BmRawHeader { height, width };
        if height == 0 || width == 0 {
            return Ok(Err(BmError::EmptyImage {
                container: reader.container().to_owned(),
                height,
                width,
            }));
        }

        let pixels = u64::from(width) * u64::from(height);
        let mut planes: [&[u8]; 5] = [&[]; 5];
        let mut offsets = [0u64; 5];
        for plane in BmPlane::ALL {
            let len = reader.checked_byte_len(plane.as_str(), pixels, plane.channels() as u64)?;
            offsets[plane.position()] = reader.position();
            planes[plane.position()] = reader.read_bytes(plane.as_str(), len)?;
        }

        let tail = if reader.is_empty() {
            None
        } else {
            let offset = reader.position();
            let rest = reader.remaining();
            Some(BmUnsupportedTail {
                offset,
                bytes: reader.read_bytes("tail", rest)?,
            })
        };

        Ok(Ok(BmFile {
            header,
            planes,
            offsets,
            tail,
        }))
    })?
}
