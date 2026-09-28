//! Acceptance stage F06-B: the reader and sound container subset with bounds
//! (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
//! section `### F06-B`, AC02 plus the bounds and inventory behaviour the stage
//! title names).
//!
//! Every byte here is authored in this file: newly authored synthetic content,
//! no original game data, no `CS_GAME_DIR` access.
//!
//! The fixtures are two containers and two member indexes. The containers are
//! opaque to this stage on purpose: no reader or sound header layout is
//! documented, so the tests supply the member index the family's own reader
//! would declare (`MemberTable`) and assert what the production code does with
//! it — the family gate, the bounds, the retained bytes and spans, the consumed
//! and uncovered ranges, and the strict status. Nothing here duplicates the
//! bounds logic: the assertions read the production listing.

use cs_formats::zbd::{
    CONTAINER_ENTRYPOINT, ContainerError, ContainerStatus, DispatchBasis, FamilyOrigin,
    HeaderStatus, INTERP_SIGNATURE, INTERP_VERSION, MEMBER_ROW_BYTES, MemberError, MemberExtent,
    MemberRow, MemberStatus, MemberTable, ReaderError, SOURCE_SPAN_BYTES, SoundError,
    UnsupportedRecord, ZbdDispatch, ZbdDispatchError, ZbdFamily, ZbdProbe, ZbdReaderId, dispatch,
    read_reader_archive, read_sound_archive,
};
use cs_formats::{ParseContext, ParseErrorKind};
use cs_types::evidence::{ClaimStatus, SourceSpan};
use cs_types::install::RelativePath;

/// Provenance label carried by every result and error these tests assert on.
const CONTAINER: &str = "synthetic/f06_b_container.zbd";

/// A container body long enough for several extents to sit inside it, with
/// recognizable content at each offset.
fn body() -> Vec<u8> {
    let mut bytes = Vec::new();
    for chunk in [
        &b"CHAPTER-ONE-TEXT"[..],
        &b"chapter two text"[..],
        &b"third chunk"[..],
    ] {
        bytes.extend_from_slice(chunk);
    }
    bytes
}

/// Offset of the first chunk inside [`body`].
const FIRST: u64 = 0;
/// Offset of the second chunk inside [`body`].
const SECOND: u64 = 16;
/// Offset of the third chunk inside [`body`].
const THIRD: u64 = 32;

/// The documented INTERP header (signature, version 7, script count) — the 12
/// bytes `docs/research/FORMAT-NOTES.md` ("INTERP observed subset") records.
fn interp_header() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&INTERP_SIGNATURE.to_le_bytes());
    bytes.extend_from_slice(&INTERP_VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
}

/// A fixture installation path, validated by the production type.
fn path(spelling: &str) -> RelativePath {
    RelativePath::new(spelling).expect("fixture spellings are valid relative paths")
}

/// Dispatches `header` at `path` with the fixture container label.
fn dispatch_at<'probe>(
    path: &'probe RelativePath,
    header: &'probe [u8],
) -> Result<ZbdDispatch<'probe>, ZbdDispatchError> {
    dispatch(ZbdProbe::new(CONTAINER, path, header))
}

/// A reader-family container: the observed world-group role name with bytes
/// that match no documented signature, so dispatch routes it on its role alone.
fn reader_family_dispatch<'probe>(
    path: &'probe RelativePath,
    header: &'probe [u8],
) -> ZbdDispatch<'probe> {
    dispatch_at(path, header).expect("an observed role name dispatches to the reader family")
}

/// Three members covering the whole body, as the family's index declares them.
fn reader_members() -> [MemberExtent<'static>; 3] {
    [
        MemberExtent::new(b"01_intro", Some(1), span(FIRST, 16)),
        MemberExtent::new(b"02_history", Some(2), span(SECOND, 16)),
        MemberExtent::new(b"03_appendix", Some(3), span(THIRD, 11)),
    ]
}

fn span(offset: u64, length: u64) -> SourceSpan {
    SourceSpan { offset, length }
}

/// The caller-named sound table, the only route to the sound family while no
/// dispatch key names it (F06-A's recorded unknown; task #340).
fn named_sound_table<'a>(members: &'a [MemberExtent<'a>]) -> MemberTable<'a> {
    MemberTable::named(CONTAINER, ZbdFamily::Sound, members)
        .expect("the sound family owns no observed role rule, so no dispatch names it")
}

/// Reads the reader archive `table` declares inside `bytes`, asserting that the
/// table really wraps the `members` the fixture built it from.
fn read<'a>(
    context: &mut ParseContext,
    members: &[MemberExtent<'a>],
    bytes: &'a [u8],
    table: &'a MemberTable<'a>,
) -> Result<cs_formats::zbd::ReaderArchive<'a>, ReaderError> {
    assert_eq!(
        table.members(),
        members,
        "the table wraps the fixture's members"
    );
    read_reader_archive(context, table, bytes)
}

// --- AC02: a valid header with incompatible family data fails ---------------

#[test]
fn accept_f06_b_valid_interp_header_is_not_reader_family_data() {
    // The documented INTERP header at its observed role: dispatch validates the
    // bytes and routes them to the interp reader (F06-A AC01).
    let interp_path = path("zbd/interp.zbd");
    let header = interp_header();
    let dispatched =
        dispatch_at(&interp_path, &header).expect("the documented INTERP header dispatches");
    assert_eq!(dispatched.family(), ZbdFamily::Interp);
    assert_eq!(dispatched.reader(), ZbdReaderId::Interp);
    assert!(matches!(
        dispatched.header_status(),
        HeaderStatus::Validated {
            signature: INTERP_SIGNATURE,
            version: INTERP_VERSION,
        }
    ));

    // The stage's minimum scenario: that valid header is **not** reader-family
    // data. The reader reader must refuse it — no interp bytes, no fallback,
    // and above all no quiet success (spec F06 AC02).
    let members = reader_members();
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = interp_header();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read(&mut context, &members, &bytes, &table)
        .expect_err("interp bytes must not be read as a reader archive");

    assert_eq!(error.code(), "family_mismatch");
    assert_eq!(error.container(), CONTAINER);
    let ReaderError::Family(mismatch) = &error else {
        panic!("expected a family mismatch, got {error:?}")
    };
    assert_eq!(mismatch.expected(), ZbdFamily::Reader);
    assert_eq!(mismatch.expected_reader(), ZbdReaderId::Reader);
    assert_eq!(mismatch.actual(), ZbdFamily::Interp);
    // The failure carries the header status that *did* validate, so a
    // diagnostic can show "the header was fine, the family was not".
    assert!(matches!(
        mismatch.header_status(),
        HeaderStatus::Validated { .. }
    ));
    assert_eq!(
        mismatch.origin(),
        FamilyOrigin::Dispatched {
            basis: DispatchBasis::HeaderAndRole
        }
    );

    // The message names both families and both reader slots, so nothing has to
    // be guessed from the error type alone.
    let message = error.to_string();
    assert!(message.contains("interp"), "message: {message}");
    assert!(message.contains("reader"), "message: {message}");
    assert!(message.contains(CONTAINER), "message: {message}");
}

#[test]
fn accept_f06_b_a_dispatch_routed_family_cannot_be_named_by_the_caller() {
    // AC02 has a second half: refusing another family's container is only
    // explicit if it is also the *only* way to hand a reader foreign bytes.
    // Naming the reader family by hand would skip the two keys, and the reader
    // would then read those bytes with nothing able to notice they are not
    // reader data — a silent fallback wearing a provenance label instead of a
    // refusal. So `MemberTable::named` refuses every family an observed role
    // already names.
    let members: [MemberExtent<'static>; 0] = [];
    let error = MemberTable::named(CONTAINER, ZbdFamily::Reader, &members)
        .expect_err("`zrdr.zbd` already names the reader family, so only a dispatch may say it");
    assert_eq!(error.code(), "family_routable_by_dispatch");
    assert_eq!(error.family(), ZbdFamily::Reader);
    assert_eq!(error.container(), CONTAINER);
    assert!(
        !error.source().is_empty(),
        "the refusal cites the inventory row it came from"
    );
    let message = error.to_string();
    assert!(message.contains("reader"), "message: {message}");
    assert!(message.contains(CONTAINER), "message: {message}");

    // The refusal tracks the inventory rather than a hard-coded list: a family
    // can be named exactly while no observed role rule names it. Today that is
    // the sound family alone (F06-A's recorded unknown, task #340).
    let nameable: Vec<ZbdFamily> = ZbdFamily::ALL
        .iter()
        .copied()
        .filter(|family| MemberTable::named(CONTAINER, *family, &members).is_ok())
        .collect();
    assert_eq!(nameable, vec![ZbdFamily::Sound]);

    // The valid INTERP container of the scenario above cannot be laundered
    // into the reader reader either: its bytes reach a reader only through the
    // dispatch, and the dispatch says `interp`, which the reader reader refuses.
    let interp_path = path("zbd/interp.zbd");
    let interp_bytes = interp_header();
    let dispatched =
        dispatch_at(&interp_path, &interp_bytes).expect("the documented INTERP header dispatches");
    assert_eq!(dispatched.family(), ZbdFamily::Interp);
    MemberTable::named(CONTAINER, dispatched.family(), &members)
        .expect_err("`interp.zbd` already names the interp family, so it cannot be asserted");
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let mut context = ParseContext::with_defaults(CONTAINER);
    let error = read(&mut context, &members, &interp_bytes, &table)
        .expect_err("and the reader reader still refuses the dispatched interp container");
    assert_eq!(error.code(), "family_mismatch");
}

#[test]
fn accept_f06_b_each_reader_refuses_the_other_family_in_both_directions() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let reader_header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let reader_dispatched = reader_family_dispatch(&zrdr_path, &reader_header);
    assert_eq!(reader_dispatched.family(), ZbdFamily::Reader);

    let members = reader_members();
    let reader_table = MemberTable::from_dispatch(&reader_dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);

    // A reader-family container is read by the reader reader…
    let archive = read(&mut context, &members, &bytes, &reader_table)
        .expect("a reader-family container is read by the reader reader");
    assert_eq!(archive.len(), 3);

    // …and refused by the sound reader: one archive is never read by the other
    // family's parser (spec F06 AC02).
    let error = read_sound_archive(&mut context, &reader_table, &bytes)
        .expect_err("reader-family bytes are not sound-family bytes");
    assert_eq!(error.code(), "family_mismatch");
    let SoundError::Family(mismatch) = &error else {
        panic!("expected a family mismatch, got {error:?}")
    };
    assert_eq!(mismatch.expected(), ZbdFamily::Sound);
    assert_eq!(mismatch.actual(), ZbdFamily::Reader);
    assert_eq!(mismatch.expected_reader(), ZbdReaderId::Sound);
}

// --- Non-negotiable #2/#3: entries retain content, ids, names, spans --------

#[test]
fn accept_f06_b_reader_entries_retain_bytes_names_ids_and_spans() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    let members = reader_members();
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read(&mut context, &members, &bytes, &table).expect("the container is in bounds");

    assert_eq!(archive.len(), 3);
    assert!(!archive.is_empty());
    assert_eq!(archive.family(), ZbdFamily::Reader);
    assert_eq!(archive.container(), CONTAINER);

    // Every entry keeps its declared identity, its source span and its exact
    // bytes — the reader hands out a borrow of the container, never a guess.
    let first = archive.entry(0).expect("the first member is readable");
    assert_eq!(first.index(), 0);
    assert_eq!(first.name(), b"01_intro");
    assert_eq!(first.id(), Some(1));
    assert_eq!(first.span(), span(FIRST, 16));
    assert_eq!(
        first.content(),
        &bytes[FIRST as usize..(FIRST + 16) as usize]
    );
    assert_eq!(first.content(), b"CHAPTER-ONE-TEXT");

    let second = archive.entry(1).expect("the second member is readable");
    assert_eq!(second.id(), Some(2));
    assert_eq!(second.content(), b"chapter two text");
    let third = archive.entry(2).expect("the third member is readable");
    assert_eq!(third.id(), Some(3));
    assert_eq!(third.content(), b"third chunk");

    // The entries iterate in declared order.
    let ids: Vec<Option<u32>> = archive.entries().map(|entry| entry.id()).collect();
    assert_eq!(ids, vec![Some(1), Some(2), Some(3)]);

    // The container's whole body is accounted for.
    assert_eq!(archive.consumed_ranges(), &[span(0, 43)]);
    assert!(archive.uncovered_ranges().is_empty());
    assert_eq!(archive.status(), ContainerStatus::Clean);
}

#[test]
fn accept_f06_b_duplicate_member_names_and_ids_are_preserved() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    // Two members with the same name **and** the same id, at different spans:
    // spec F06 non-negotiable #3 requires both to survive as distinct rows.
    let members = [
        MemberExtent::new(b"chapter", Some(7), span(FIRST, 16)),
        MemberExtent::new(b"chapter", Some(7), span(SECOND, 16)),
    ];
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read(&mut context, &members, &bytes, &table).expect("both members are in bounds");

    assert_eq!(archive.len(), 2, "duplicates are rows, not one merged row");
    let first = archive.entry(0).expect("row 0 is readable");
    let second = archive.entry(1).expect("row 1 is readable");
    assert_eq!(first.name(), second.name());
    assert_eq!(first.id(), second.id());
    assert_ne!(
        first.span(),
        second.span(),
        "two rows with the same identity stay distinct by their spans"
    );
    assert_eq!(first.content(), b"CHAPTER-ONE-TEXT");
    assert_eq!(second.content(), b"chapter two text");
}

#[test]
fn accept_f06_b_sound_entries_retain_spans_and_unknown_descriptor_fields() {
    // Sound has no observed archive name, so its table is the documented
    // caller-named route (F06-A recorded that unknown; task #340 owns it).
    let members = [
        MemberExtent::new(b"gun_loop", Some(11), span(FIRST, 16)),
        MemberExtent::new(b"engine_loop", Some(12), span(SECOND, 16)),
    ];
    let table = named_sound_table(&members);
    assert_eq!(table.family(), ZbdFamily::Sound);
    assert_eq!(table.origin(), FamilyOrigin::NamedByCaller);
    assert!(matches!(
        table.header_status(),
        HeaderStatus::Unvalidated { .. }
    ));

    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read_sound_archive(&mut context, &table, &bytes)
        .expect("the caller-named sound container is in bounds");

    assert_eq!(archive.family(), ZbdFamily::Sound);
    assert_eq!(archive.len(), 2);

    // Spec F06 non-negotiable #2: sound entries retain their source span and
    // their bytes…
    let first = archive.entry(0).expect("the first member is readable");
    assert_eq!(first.span(), span(FIRST, 16));
    assert_eq!(first.content(), b"CHAPTER-ONE-TEXT");
    assert_eq!(first.id(), Some(11));
    assert_eq!(first.name(), b"gun_loop");

    // …while every declared descriptor field stays unknown instead of being
    // invented. This stage documents no sound header, so a rate, a channel
    // count or a loop point would be a fabricated game value (spec F06 research
    // boundary; AGENTS.md "unknown means unknown").
    let descriptor = first.descriptor();
    let expected_reason = cs_formats::zbd::family_record(ZbdFamily::Sound)
        .header_rule()
        .undocumented_reason()
        .expect("the sound family's header layout is recorded as undocumented");
    for field in [
        descriptor.format().reason(),
        descriptor.channels().reason(),
        descriptor.rate_hz().reason(),
        descriptor.loop_points().reason(),
    ] {
        assert_eq!(field, Some(expected_reason));
    }
    assert!(!descriptor.format().is_known());
    assert!(!descriptor.channels().is_known());
    assert!(!descriptor.rate_hz().is_known());
    assert!(!descriptor.loop_points().is_known());
    assert!(descriptor.format().known().is_none());
    assert!(descriptor.rate_hz().known().is_none());
}

// --- Non-negotiable #4: an invalid member fails, its siblings do not --------

#[test]
fn accept_f06_b_a_corrupt_member_fails_its_content_but_not_its_siblings() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    let body_len = body().len() as u64;
    let members = [
        // Good: the first chunk.
        MemberExtent::new(b"good", Some(1), span(FIRST, 16)),
        // Past the end: declared length runs off the container.
        MemberExtent::new(b"past_the_end", Some(2), span(body_len - 4, 16)),
        // Overflowing: offset + length cannot even be represented.
        MemberExtent::new(b"overflow", Some(3), span(u64::MAX - 2, 8)),
        // Good again: the last chunk, ending exactly at the container end.
        MemberExtent::new(b"tail", Some(4), span(THIRD, 11)),
    ];
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read(&mut context, &members, &bytes, &table)
        .expect("a corrupt member fails its own content, not the whole listing");

    // Both valid siblings are still readable, with their own bytes.
    assert_eq!(
        archive.entry(0).expect("good").content(),
        b"CHAPTER-ONE-TEXT"
    );
    assert_eq!(archive.entry(3).expect("tail").content(), b"third chunk");

    // The corrupt ones yield nothing at all — never a truncated read.
    assert!(
        archive.entry(1).is_none(),
        "an out-of-bounds member has no bytes"
    );
    assert!(
        archive.entry(2).is_none(),
        "an overflowing member has no bytes"
    );

    // Each failure is named, with its own code and offset.
    let rows = archive.listing().rows();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].status(), MemberStatus::Readable);
    assert_eq!(rows[3].status(), MemberStatus::Readable);

    match rows[1].error() {
        Some(MemberError::OutOfBounds {
            index,
            offset,
            length,
            container_len,
            end,
        }) => {
            assert_eq!(index, 1);
            assert_eq!(offset, body_len - 4);
            assert_eq!(length, 16);
            assert_eq!(container_len, body_len);
            assert_eq!(end, body_len + 12);
        }
        other => panic!("expected `member_out_of_bounds` on row 1, got {other:?}"),
    }
    assert_eq!(
        rows[1].error().map(|error| error.code()),
        Some("member_out_of_bounds")
    );
    assert_eq!(rows[1].name(), b"past_the_end");
    assert_eq!(rows[1].span(), span(body_len - 4, 16));

    match rows[2].error() {
        Some(MemberError::ExtentOverflow {
            index,
            offset,
            length,
        }) => {
            assert_eq!(index, 2);
            assert_eq!(offset, u64::MAX - 2);
            assert_eq!(length, 8);
        }
        other => panic!("expected `extent_overflow` on row 2, got {other:?}"),
    }
    assert_eq!(
        rows[2].error().map(|error| error.code()),
        Some("extent_overflow")
    );

    // The strict status is nonzero: a listing with failures never reports clean.
    assert_eq!(archive.failures(), 2);
    assert_eq!(archive.status(), ContainerStatus::Failed { failures: 2 });
    assert!(!archive.status().is_clean());
    assert_eq!(archive.status().failures(), 2);
    assert_eq!(archive.status().label(), "failed");

    // Only the two good members were consumed.
    assert_eq!(
        archive.consumed_ranges(),
        &[span(FIRST, 16), span(THIRD, 11)]
    );

    // A member that failed its bounds check is a *failure*, not an unsupported
    // record: it has no content to interpret, and listing it as one would imply
    // a reader had already read it.
    let unsupported = archive.unsupported_records();
    assert_eq!(
        unsupported
            .iter()
            .map(|record| record.index())
            .collect::<Vec<_>>(),
        vec![0, 3],
        "only the members whose bytes were handed out are unsupported records"
    );
}

#[test]
fn accept_f06_b_gaps_and_overlaps_are_reported_in_ranges() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    // `b` and `c` overlap (16..24 is claimed twice); the stretch 8..12 and the
    // tail 40..43 are claimed by nobody. Both extents stay inside the 43-byte
    // body, so this is a structural question, not a bounds failure.
    let members = [
        MemberExtent::new(b"a", Some(1), span(FIRST, 8)),
        MemberExtent::new(b"b", Some(2), span(12, 12)),
        MemberExtent::new(b"c", Some(3), span(16, 24)),
    ];
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive =
        read(&mut context, &members, &bytes, &table).expect("all three extents are in bounds");

    // Overlapping extents merge into as few consumed ranges as cover them; a
    // gap between members keeps them apart.
    assert_eq!(archive.consumed_ranges(), &[span(0, 8), span(12, 28)]);
    // Every stretch no member claimed is reported, not hidden.
    assert_eq!(archive.uncovered_ranges(), &[span(8, 4), span(40, 3)]);
    // Overlap and gaps are not corruption: every member is in bounds.
    assert_eq!(archive.status(), ContainerStatus::Clean);
    assert_eq!(archive.failures(), 0);
    // Each member still hands out its own bytes, even the aliased one.
    assert_eq!(archive.entry(0).expect("a").content(), b"CHAPTER-");
    assert_eq!(archive.entry(2).expect("c").content(), &bytes[16..40]);
}

#[test]
fn accept_f06_b_an_empty_index_lists_nothing_and_claims_nothing() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    let members: [MemberExtent<'static>; 0] = [];
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read(&mut context, &members, &bytes, &table).expect("an empty index is valid");

    assert!(archive.is_empty());
    assert_eq!(archive.len(), 0);
    assert!(archive.consumed_ranges().is_empty());
    // Nothing was claimed, so the whole body is uncovered.
    assert_eq!(archive.uncovered_ranges(), &[span(0, 43)]);
    assert_eq!(archive.status(), ContainerStatus::Clean);
    assert!(archive.entry(0).is_none());
}

// --- Unsupported records are reported, not dropped --------------------------

#[test]
fn accept_f06_b_unsupported_records_are_listed_with_their_spans() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    let members = reader_members();
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read(&mut context, &members, &bytes, &table).expect("the container is in bounds");

    // No reader encoding layout is documented, so **every** entry this stage
    // hands out is also an unsupported record: the IDENTITY-CONTENT contract
    // forbids a collection from excluding entries, and spec F06 #3 forbids
    // calling a structural pass an interpretation.
    let unsupported: Vec<UnsupportedRecord<'_>> = archive.unsupported_records();
    assert_eq!(unsupported.len(), 3);
    for (index, record) in unsupported.iter().enumerate() {
        assert_eq!(record.index(), index);
        assert_eq!(
            record.span(),
            archive.entry(index).expect("readable").span()
        );
        assert!(
            !record.reason().is_empty(),
            "an unsupported record cites why it is unsupported"
        );
    }
    assert_eq!(unsupported[0].name(), b"01_intro");
    assert_eq!(unsupported[0].id(), Some(1));

    // The sound reader reports the same shape over its own entries.
    let sound_members = [MemberExtent::new(b"gun_loop", Some(11), span(FIRST, 16))];
    let sound_table = named_sound_table(&sound_members);
    let sound = read_sound_archive(&mut context, &sound_table, &bytes)
        .expect("the sound table is in bounds");
    let sound_unsupported = sound.unsupported_records();
    assert_eq!(sound_unsupported.len(), 1);
    assert_eq!(sound_unsupported[0].name(), b"gun_loop");
    assert!(!sound_unsupported[0].reason().is_empty());
}

// --- The bounded parse: budgets, scope, retries -----------------------------

#[test]
fn accept_f06_b_the_listing_is_bounded_by_the_parse_allocation_budget() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    let members = reader_members();
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();

    // A parse with no allocation headroom cannot hold the row table, and says
    // so instead of allocating anyway (spec F03 non-negotiable #2).
    let mut starved = ParseContext::new(CONTAINER, 0, 32);
    let error = read(&mut starved, &members, &bytes, &table)
        .expect_err("a member table must not be allocated without budget");
    match &error {
        ReaderError::Container(ContainerError::Parse(parse)) => {
            assert_eq!(parse.kind, ParseErrorKind::AllocationBudgetExceeded);
            assert_eq!(parse.container, CONTAINER);
            assert_eq!(
                parse.field,
                format!("{CONTAINER_ENTRYPOINT}.members"),
                "the failure is scoped by the container entrypoint"
            );
        }
        other => panic!("expected a budget refusal, got {other:?}"),
    }

    // The refused attempt left the ledger exactly as it found it: nothing was
    // booked, no nesting stayed behind and the allowance was not widened, so a
    // retry of the same bytes is decided only by the budget it is given
    // (spec F03-C teardown/retry).
    assert_eq!(starved.allocation().used(), 0, "a refusal books nothing");
    assert_eq!(starved.allocation().limit(), 0, "a refusal never widens");
    assert_eq!(starved.recursion().depth(), 0, "an attempt leaves no depth");

    // The same bytes read on a context that has the budget — the same table,
    // the same container, the same production code.
    let mut healthy = ParseContext::with_defaults(CONTAINER);
    let archive = read(&mut healthy, &members, &bytes, &table).expect("the retry succeeds");
    assert_eq!(archive.len(), 3);
}

#[test]
fn accept_f06_b_the_listing_charge_is_exact_and_never_widens_the_ledger() {
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);

    let members = reader_members();
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();

    // The charge describes the memory the listing really holds, so it is
    // checked against the types rather than taken on trust.
    assert_eq!(MEMBER_ROW_BYTES, size_of::<MemberRow<'static>>() as u64);
    assert_eq!(SOURCE_SPAN_BYTES, size_of::<SourceSpan>() as u64);
    let charge = 3 * MEMBER_ROW_BYTES + 3 * SOURCE_SPAN_BYTES;

    // One byte short of the charge is refused, and the refusal books nothing.
    let mut tight = ParseContext::new(CONTAINER, charge - 1, 32);
    let error = read(&mut tight, &members, &bytes, &table)
        .expect_err("one byte short of the table's own size is refused");
    assert_eq!(error.code(), "parse");
    assert_eq!(
        tight.allocation().used(),
        0,
        "a refused listing books nothing"
    );
    assert_eq!(
        tight.allocation().limit(),
        charge - 1,
        "a refusal never widens the budget"
    );

    // The exact charge fits, and the ledger then reports exactly it.
    let mut fitting = ParseContext::new(CONTAINER, charge, 32);
    let archive = read(&mut fitting, &members, &bytes, &table).expect("the exact charge fits");
    assert_eq!(archive.len(), 3);
    assert_eq!(fitting.allocation().used(), charge);
    assert_eq!(fitting.allocation().remaining(), 0);

    // A second listing of the same container on the same parse cannot spend
    // what is left, and does not hand back what the first listing took: the
    // allowance a parse was given is the allowance it keeps.
    let error = read(&mut fitting, &members, &bytes, &table)
        .expect_err("a parse's ledger accumulates, so a second listing does not fit");
    assert_eq!(error.code(), "parse");
    assert_eq!(
        fitting.allocation().used(),
        charge,
        "the refused second listing books nothing"
    );
    assert_eq!(fitting.recursion().depth(), 0, "an attempt leaves no depth");
}

// --- Evidence honesty --------------------------------------------------------

#[test]
fn accept_f06_b_nothing_this_stage_produces_claims_documented_bytes() {
    // The reader family is name-inferred (F06-A: `zrdr.zbd` -> reader), and no
    // reader or sound layout is documented. Every claim these readers make must
    // therefore carry `unknown` / `inferred` evidence, never `documented` or
    // `verified_original` — a test suite pass is not original verification.
    let zrdr_path = path("zbd/c1/zrdr.zbd");
    let header = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let dispatched = reader_family_dispatch(&zrdr_path, &header);
    assert!(matches!(
        dispatched.header_status(),
        HeaderStatus::Unvalidated { .. }
    ));

    let members = reader_members();
    let table = MemberTable::from_dispatch(&dispatched, &members);
    let bytes = body();
    let mut context = ParseContext::with_defaults(CONTAINER);
    let archive = read(&mut context, &members, &bytes, &table).expect("the container is in bounds");

    for entry in archive.entries() {
        assert_eq!(
            entry.encoding().evidence(),
            ClaimStatus::Unknown,
            "no reader encoding layout is documented, so no entry may claim otherwise"
        );
        assert!(!entry.encoding().reason().is_empty());
    }
    for record in archive.unsupported_records() {
        assert!(!record.reason().is_empty());
    }

    // The spans handed out are the contract's own span type, so a member keeps
    // a place in the container a catalog or an evidence record can cite.
    let span: SourceSpan = archive.entry(0).expect("readable").span();
    assert_eq!(span.offset, FIRST);
    assert_eq!(span.length, 16);
}
