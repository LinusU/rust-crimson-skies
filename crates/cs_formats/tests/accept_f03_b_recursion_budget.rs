//! Acceptance scenario F03-B: the recursion limit is an *independent* limit
//! (spec non-negotiable #2) that a real recursive descent parser can hold,
//! with the designed default as its ceiling and guards that release on both
//! the success and the error path (non-negotiable #3: cycles must terminate).
//!
//! Every case calls production code (`RecursionBudget::enter` and its
//! guard); removing the depth check makes the limit cases succeed where they
//! must fail, and removing the release in `Drop` makes the accounting cases
//! fail.

use cs_formats::{ParseError, ParseErrorKind, Reader, RecursionBudget};

/// Provenance label carried by every error these tests assert on.
const CONTAINER: &str = "synthetic/f03_b_nested.bin";

/// A recursive descent over a synthetic nested record: one byte per node,
/// holding the number of children that follow. This is the shape a directory
/// or scene-graph parser takes, and it is what holds the budget while it
/// recurses — the production usage pattern.
fn descend(reader: &mut Reader<'_>, budget: &RecursionBudget) -> Result<u64, ParseError> {
    let _guard = budget.enter("node", reader.position())?;
    let children = reader.read_u8("node.children")?;
    let mut nodes = 1u64;
    for _ in 0..children {
        nodes += descend(reader, budget)?;
    }
    Ok(nodes)
}

/// The designed default ceiling: a chain of 32 nodes parses, one node deeper
/// fails with a structured error naming the container, the offset of the
/// offending member and the level that was refused.
#[test]
fn accept_f03_b_default_depth_admits_32_levels_and_refuses_33() {
    let budget = RecursionBudget::with_defaults(CONTAINER);
    assert_eq!(budget.max_depth(), RecursionBudget::DEFAULT_MAX_DEPTH);

    // 31 children bytes followed by a leaf: exactly 32 nested levels.
    let ok_bytes: Vec<u8> = [vec![1u8; 31], vec![0u8]].concat();
    let mut reader = Reader::new(CONTAINER, &ok_bytes);
    let nodes = descend(&mut reader, &budget).expect("32 nested levels fit the default limit");
    assert_eq!(
        nodes, 32,
        "every node of the chain was visited exactly once"
    );
    assert_eq!(budget.depth(), 0, "every guard was released on the way out");

    // One node deeper: level 33 is refused at the position of the entry that
    // would have exceeded the budget.
    let deep_bytes: Vec<u8> = [vec![1u8; 33], vec![0u8]].concat();
    let mut reader = Reader::new(CONTAINER, &deep_bytes);
    let err =
        descend(&mut reader, &budget).expect_err("33 nested levels exceed the default limit of 32");
    assert_eq!(err.kind, ParseErrorKind::RecursionDepthExceeded);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.offset, 32, "the error anchors on the refused member");
    assert_eq!(err.field, "node");
    assert_eq!(err.expected, "at most 32 nested levels");
    assert_eq!(err.observed, "level 33 requested");
    assert_eq!(
        budget.depth(),
        0,
        "guards release on the error path too, so depth cannot leak"
    );
}

/// The exact ceiling is enforced level by level: the level at `max_depth` is
/// accepted, the next is refused, and dropping a guard makes room again.
#[test]
fn accept_f03_b_depth_ceiling_is_exact_and_released() {
    let budget = RecursionBudget::new(CONTAINER, 4);

    let first = budget
        .enter("node", 0)
        .expect("level 1 is inside the limit");
    assert_eq!(first.level(), 1);
    let second = budget
        .enter("node", 1)
        .expect("level 2 is inside the limit");
    assert_eq!(second.level(), 2);
    assert_eq!(budget.depth(), 2);

    drop(second);
    assert_eq!(
        budget.depth(),
        1,
        "dropping a guard releases exactly the level it entered"
    );

    // Siblings after a nested entry: the released level is reusable.
    let sibling = budget
        .enter("node.sibling", 2)
        .expect("a released level can be re-entered");
    assert_eq!(sibling.level(), 2);

    let third = budget
        .enter("node", 3)
        .expect("level 3 is inside the limit");
    let fourth = budget
        .enter("node", 4)
        .expect("level 4 reaches the ceiling");
    assert_eq!(fourth.level(), 4);
    assert_eq!(budget.depth(), 4);

    let err = budget
        .enter("node", 5)
        .expect_err("level 5 must be refused at a ceiling of 4");
    assert_eq!(err.kind, ParseErrorKind::RecursionDepthExceeded);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.offset, 5);
    assert_eq!(err.field, "node");
    assert_eq!(err.expected, "at most 4 nested levels");
    assert_eq!(err.observed, "level 5 requested");
    assert_eq!(
        budget.depth(),
        4,
        "a refused entry must not consume a level"
    );

    drop(third);
    let retry = budget
        .enter("node", 6)
        .expect("after a release the ceiling is reachable again");
    assert_eq!(retry.level(), 4);
    assert_eq!(budget.depth(), 4);

    drop((first, sibling, fourth, retry));
    assert_eq!(budget.depth(), 0, "the budget unwinds to zero");
}

/// The low end of the configuration surface: a zero ceiling refuses the very
/// first entry, so a parse that must not nest at all cannot.
#[test]
fn accept_f03_b_zero_depth_refuses_the_first_entry() {
    let budget = RecursionBudget::new(CONTAINER, 0);

    let err = budget
        .enter("node", 7)
        .expect_err("a zero ceiling refuses level 1");
    assert_eq!(err.kind, ParseErrorKind::RecursionDepthExceeded);
    assert_eq!(err.container, CONTAINER);
    assert_eq!(err.offset, 7);
    assert_eq!(err.field, "node");
    assert_eq!(err.expected, "at most 0 nested levels");
    assert_eq!(err.observed, "level 1 requested");
    assert_eq!(budget.depth(), 0, "the refusal consumes no level");
    assert_eq!(budget.max_depth(), 0);
}
