//! `accept_f12_f_*`: the absolute-offset window of
//! [`cs_formats::Reader`] (stage F12-F) and the PE resource reader's use of it.
//!
//! A window is the random-access counterpart of `sub_reader`: it opens the
//! range an *absolute* offset names, bounds-checks it against the range the
//! reader was given, rebases it so `position()` is absolute and hands out
//! slices of the original input. These tests pin the four properties the rest
//! of the crate now relies on:
//!
//! * the window reports absolute positions and hands out the input's own
//!   bytes, at every nesting depth;
//! * a window that runs past the range is refused with *exactly* the
//!   [`ParseError`] a `skip` of the same read at the same offset gives —
//!   same container, same absolute offset, same kind, same
//!   expected/observed conditions — and is never clamped to the end of the
//!   input;
//! * every truncation boundary of the F03-D corpus is still a refusal when it
//!   is reached through a window, with the allocation ledger and the recursion
//!   depth left exactly as they were;
//! * the PE resource reader, the first random-access caller, reaches its bytes
//!   through those windows: a truncated image is refused at the absolute
//!   offset of the field that is missing, not somewhere else and not by
//!   quietly reading less.
//!
//! The corpus is the shared synthetic one from F03-D
//! (`tests/common/nested.rs`): newly authored bytes, nothing derived from the
//! original installation and no `CS_GAME_DIR` access.

mod common;

use common::nested::{EMIT_SEEDS, emit_case};
use cs_formats::{ParseContext, ParseError, ParseErrorKind, PeError, Reader, read_pe_layout};

/// The provenance label these cases report their refusals against.
const CONTAINER: &str = "synthetic/f12_f_window.bin";

/// Entrypoint name the budget-ledger checks run through.
const ENTRYPOINT: &str = "window";

/// A small container with a word at every offset, so a window can be asked for
/// any range of it and the bytes it hands out are predictable.
///
/// `byte(i)` is `(i as u8).wrapping_mul(7).wrapping_add(11)`, so two different
/// offsets never read alike by accident.
fn container(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| (index as u8).wrapping_mul(7).wrapping_add(11))
        .collect()
}

fn byte(index: usize) -> u8 {
    (index as u8).wrapping_mul(7).wrapping_add(11)
}

// ------------------------------------------------- absolute positions, borrow

/// A window is positioned at the absolute offset it was opened for, keeps that
/// position when it is nested, and hands out the input's own bytes rather than
/// a copy: the pointer of every borrowed range is the input's pointer plus the
/// offset asked for.
#[test]
fn accept_f12_f_window_reports_absolute_positions_and_borrows_the_input() -> Result<(), ParseError>
{
    let bytes = container(64);
    let image = Reader::new(CONTAINER, &bytes);
    assert_eq!(
        image.range_end(),
        64,
        "a whole-container reader's range end"
    );

    // A window in the middle of the container: absolute position, container
    // provenance, and exactly the range it was asked for.
    let mut middle = image
        .window(16, 8, "block.payload")
        .expect("inside the container");
    assert_eq!(middle.position(), 16);
    assert_eq!(middle.container(), CONTAINER);
    assert_eq!(middle.remaining(), 8);
    assert_eq!(middle.range_end(), 24);
    assert_eq!(middle.read_bytes("block.payload", 8)?, &bytes[16..24]);

    // Every word it decodes is the same word the same bytes spell.
    let mut words = image
        .window(4, 8, "block.words")
        .expect("inside the container");
    assert_eq!(words.position(), 4);
    let first = words.read_u32("block.words.first")?;
    let second = words.read_u16("block.words.second")?;
    assert_eq!(
        first,
        u32::from_le_bytes([byte(4), byte(5), byte(6), byte(7)])
    );
    assert_eq!(second, u16::from_le_bytes([byte(8), byte(9)]));
    // A read inside a window is anchored at the absolute offset, not at the
    // window's own start: the window is 4..12, a word and a short have been
    // read, and the next four bytes would start at 10.
    let error = words
        .read_u32("block.words.third")
        .expect_err("the window ends at 12");
    assert_eq!(error.offset, 10, "a failure inside a window is absolute");
    assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);

    // `window_bytes` hands out the same range, borrowed from the input: the
    // pointer proves nothing was copied.
    let borrowed = image.window_bytes(32, 6, "block.raw")?;
    assert_eq!(borrowed, &bytes[32..38]);
    assert_eq!(
        borrowed.as_ptr() as usize - bytes.as_ptr() as usize,
        32,
        "the bytes are the input's, at the offset asked for"
    );

    // A window of a window is still absolute: nesting does not rebase the
    // positions back to the inner range.
    let outer = image.window(16, 32, "outer").expect("inside the container");
    let mut inner = outer
        .window(24, 4, "inner")
        .expect("inside the outer range");
    assert_eq!(inner.position(), 24);
    assert_eq!(inner.range_end(), 28);
    assert_eq!(inner.read_bytes("inner", 4)?, &bytes[24..28]);

    // An empty window at the very end of the container is a window: nothing to
    // read, and nothing refused.
    let tail = image.window(64, 0, "tail")?;
    assert_eq!(tail.position(), 64);
    assert_eq!(tail.range_end(), 64);
    assert!(tail.is_empty());

    Ok(())
}

// --------------------------------------------------------- refused, not clamped

/// A window past the end of the range is refused with the same error a `skip`
/// of the same read at the same offset gives, and never clamped: a shorter
/// window than was asked for is not an option, because a parser that received
/// one would read a truncated record as if it were whole.
#[test]
fn accept_f12_f_window_past_the_container_is_refused_exactly_as_a_skip() -> Result<(), ParseError> {
    let bytes = container(24);
    let image = Reader::new(CONTAINER, &bytes);

    // Every window that runs past the end, one byte over through most of the
    // container again. (`offset + len` that overflows is a different refusal
    // and has its own test: `skip` takes a `usize` length, so the two APIs
    // cannot even express the same hostile request.)
    for (offset, len) in [(24u64, 1u64), (23, 2), (20, 8), (12, 64), (0, 25)] {
        // What a `skip` of the same read at the same offset reports, computed
        // with the cursor API this window is the random-access twin of.
        let mut sequential = Reader::new(CONTAINER, &bytes);
        sequential.skip("table.block", offset as usize)?;
        let expected = sequential
            .skip("table.block", len as usize)
            .expect_err("a skip past the end is refused");

        for error in [
            image
                .window(offset, len, "table.block")
                .expect_err("refused"),
            image
                .window_bytes(offset, len, "table.block")
                .expect_err("refused"),
        ] {
            assert_eq!(error, expected, "window and skip must refuse alike");
            assert_eq!(error.container, CONTAINER);
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(error.offset, offset, "the refusal names the window start");
            assert_eq!(error.expected, format!("{len} bytes available"));
            assert_eq!(
                error.observed,
                format!("{} bytes available", 24u64.saturating_sub(offset)),
                "the refusal counts the bytes the container really has"
            );
        }
    }

    // A window of a window cannot reach outside the range the inner reader
    // holds either: not past its end, and not back before its start.
    let inner = image.window(8, 8, "outer").expect("inside the container");
    let past = inner
        .window(12, 8, "past")
        .expect_err("past the inner range");
    assert_eq!(past.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(past.offset, 12);
    assert_eq!(past.observed, "4 bytes available");

    let before = inner
        .window(0, 4, "before")
        .expect_err("before the inner range");
    assert_eq!(before.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(before.offset, 0, "the refusal names the window start");
    assert_eq!(
        before.observed, "0 bytes available",
        "the inner range holds nothing at all before offset 8"
    );

    // A window that starts inside the inner range and runs past its end is
    // refused as the truncation it is, with the two bytes that do exist counted
    // rather than the four that were asked for.
    let straddling = inner
        .window(14, 4, "straddling")
        .expect_err("past the inner range");
    assert_eq!(straddling.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(straddling.offset, 14);
    assert_eq!(straddling.expected, "4 bytes available");
    assert_eq!(straddling.observed, "2 bytes available");

    Ok(())
}

/// Overflowing `offset + len` is a `LengthOverflow` and never a slice, a wrap
/// or a panic — for both window forms and at the extremes a hostile table can
/// name.
#[test]
fn accept_f12_f_window_overflow_is_refused_without_touching_the_bytes() -> Result<(), ParseError> {
    let bytes = container(8);
    let image = Reader::new(CONTAINER, &bytes);

    for (offset, len) in [
        (u64::MAX, 1u64),
        (1, u64::MAX),
        (u64::MAX, u64::MAX),
        (u64::MAX - 1, 4),
    ] {
        for error in [
            image
                .window(offset, len, "hostile.extent")
                .expect_err("refused"),
            image
                .window_bytes(offset, len, "hostile.extent")
                .expect_err("refused"),
        ] {
            assert_eq!(error.kind, ParseErrorKind::LengthOverflow, "{error}");
            assert_eq!(error.offset, offset);
            assert_eq!(error.container, CONTAINER);
            assert_eq!(error.expected, "offset + length to fit in u64");
        }
    }

    // The whole container is still windowable afterwards: none of the hostile
    // extents moved anything.
    assert_eq!(
        image.window(0, 8, "whole")?.read_bytes("whole", 8)?,
        &bytes[..]
    );
    Ok(())
}

// ------------------------------------------------------ the F03-D corpus sweep

/// The truncation corpus of F03-D, driven through the window API: for every
/// emitted buffer and every byte boundary, a window of the whole prefix is
/// accepted, one byte more is refused with `UnexpectedEof` naming the bytes
/// that are really there, and both leave the parse's ledgers untouched.
///
/// A window that clamped to the end of the input would accept the over-long
/// request, and this is what notices.
#[test]
fn accept_f12_f_window_refuses_every_truncated_prefix_of_the_f03_corpus() {
    let mut boundaries = 0usize;

    for seed in 0..EMIT_SEEDS {
        let name = format!("emitted seed {seed}");
        let bytes = emit_case(seed);
        assert!(!bytes.is_empty(), "{name}: the emitter produced nothing");

        for cut in 0..=bytes.len() {
            let prefix = &bytes[..cut];
            let cut_len = cut as u64;
            let mut context = ParseContext::with_defaults(CONTAINER);

            // The prefix itself: any range inside it opens, including the
            // empty prefix and the whole prefix.
            let accepted = context.parse(ENTRYPOINT, prefix, |reader, allocation, recursion| {
                for offset in 0..=cut_len {
                    let length = cut_len - offset;
                    let window = reader
                        .window(offset, length, "corpus.node")
                        .expect("a range inside the prefix opens");
                    assert_eq!(window.position(), offset);
                    assert_eq!(window.range_end(), cut_len);
                    assert_eq!(
                        window.remaining(),
                        length as usize,
                        "{name}: cut at {cut}: a window is exactly what it asked for"
                    );
                    let borrowed = reader
                        .window_bytes(offset, length, "corpus.node")
                        .expect("a range inside the prefix opens");
                    assert_eq!(borrowed, &prefix[offset as usize..]);
                }
                assert_eq!(allocation.used(), 0, "{name}: a window books nothing");
                assert_eq!(recursion.depth(), 0, "{name}");
                Ok(())
            });
            if let Err(error) = accepted {
                panic!("{name}: cut at {cut}: the prefix must be windowable: {error}");
            }

            // One byte past the prefix: refused, by both window forms, with
            // the bytes the prefix really has.
            let mut context = ParseContext::with_defaults(CONTAINER);
            let refused = context.parse(ENTRYPOINT, prefix, |reader, _allocation, recursion| {
                let error = reader
                    .window(0, cut_len + 1, "corpus.node")
                    .expect_err("one byte past the cut is refused");
                assert_eq!(error.kind, ParseErrorKind::UnexpectedEof, "{name}");
                assert_eq!(error.container, CONTAINER);
                assert_eq!(error.offset, 0);
                assert_eq!(error.expected, format!("{} bytes available", cut_len + 1));
                assert_eq!(error.observed, format!("{cut_len} bytes available"));
                assert_eq!(
                    reader
                        .window_bytes(0, cut_len + 1, "corpus.node")
                        .expect_err("refused"),
                    error,
                    "{name}: both window forms refuse alike"
                );
                assert_eq!(recursion.depth(), 0, "{name}");
                Err(error)
            });
            match refused {
                Err(error) => assert_eq!(error.field, format!("{ENTRYPOINT}.corpus.node")),
                Ok(()) => panic!("{name}: cut at {cut}: a one-byte-longer window was accepted"),
            }
            assert_eq!(
                context.allocation().used(),
                0,
                "{name}: cut at {cut}: a refused window books nothing"
            );
            assert_eq!(
                context.recursion().depth(),
                0,
                "{name}: cut at {cut}: the depth is released"
            );

            boundaries += 1;
        }
    }

    assert!(
        boundaries >= 1_000,
        "the recorded truncation corpus shrank to {boundaries} boundaries"
    );
    println!(
        "f12-f window sweep: {EMIT_SEEDS} emitted buffers, {boundaries} prefixes \
         windowed and one byte beyond each refused"
    );
}

// ------------------------------------------------- the ported PE reader

/// The PE resource reader reaches every byte through the shared windows, so a
/// truncated image is refused by them: the refusal names the absolute offset of
/// the field that is not there, counts the bytes the image really has, and the
/// layout of an intact header still reports the image's own length.
#[test]
fn accept_f12_f_pe_resource_reads_go_through_the_shared_window() {
    // Not an image at all: the DOS signature is present but `e_lfanew` is not,
    // and the refusal is the window's, at `e_lfanew`'s own offset.
    let mut context = ParseContext::with_defaults("fixture.f12_f.stub");
    let error = read_pe_layout(&mut context, &DOS_MAGIC).expect_err("a stub is not an image");
    match error {
        PeError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof, "{error}");
            assert_eq!(error.offset, 0x3c, "e_lfanew's absolute offset");
            assert_eq!(error.expected, "4 bytes available");
            assert_eq!(error.observed, "0 bytes available");
            assert_eq!(error.field, "pe.resources.pe.dos.e_lfanew");
        }
        other => panic!("{other}"),
    }
    assert_eq!(context.allocation().used(), 0, "a refusal books nothing");

    // A DOS header whose `e_lfanew` names a signature past the end of the
    // image: the refusal follows the offset the bytes give, not the reader's
    // cursor.
    let mut stub = vec![0u8; 0x40];
    stub[0..2].copy_from_slice(&DOS_MAGIC);
    stub[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    let mut context = ParseContext::with_defaults("fixture.f12_f.e_lfanew");
    let error = read_pe_layout(&mut context, &stub).expect_err("no NT signature in reach");
    match error {
        PeError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof, "{error}");
            assert_eq!(error.offset, 0x80, "where e_lfanew pointed");
            assert_eq!(error.expected, "4 bytes available");
            assert_eq!(error.observed, "0 bytes available");
            assert_eq!(error.field, "pe.resources.pe.signature");
        }
        other => panic!("{other}"),
    }

    // A byte-for-byte extension of the same stub: the signature is now really
    // there, and the refusal moves on to the COFF header behind it — at that
    // field's own absolute offset, still through the same window. The reader
    // does not clamp, shorten or invent the range.
    let mut longer = vec![0u8; 0x84];
    longer[0..2].copy_from_slice(&DOS_MAGIC);
    longer[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    longer[0x80..0x84].copy_from_slice(b"PE\0\0");
    let mut context = ParseContext::with_defaults("fixture.f12_f.signature");
    let error = read_pe_layout(&mut context, &longer).expect_err("no COFF header in reach");
    match error {
        PeError::Parse(error) => {
            assert_eq!(error.kind, ParseErrorKind::UnexpectedEof, "{error}");
            assert_eq!(error.offset, 0x86, "the COFF section count word");
            assert_eq!(error.expected, "2 bytes available");
            assert_eq!(error.observed, "0 bytes available");
            assert_eq!(error.field, "pe.resources.pe.coff.number_of_sections");
        }
        other => panic!("{other}"),
    }
    assert_eq!(context.allocation().used(), 0, "a refusal books nothing");
}

/// `IMAGE_DOS_SIGNATURE`, spelled out so this test needs no PE fixture.
const DOS_MAGIC: [u8; 2] = *b"MZ";
