//! `MetaOpenFile`'s request-only folding, as a GOS chain rule (task #714).
//!
//! #693 settled the original's name-matching rule from addresses in
//! `roffile.dll`
//! (`docs/findings/2026-10-06-t693-metaopenfile-name-matching.md`,
//! section B): for a container the request is **upper-cased and compared
//! byte for byte against the stored name, which is never folded**
//! (`upper(request) == stored`). Neither #686 rule implemented that shape
//! literally — `AsciiInsensitive` folds both sides, `ExactSpelling` folds
//! neither — and on this installation (every container name ASCII-uppercase,
//! measured in `accept_t693_…`) the difference is invisible, a fidelity gap
//! for synthetic and future content rather than a retail behaviour bug.
//! Nothing here may change what the retail tests answer.
//!
//! What these tests pin, all through production code
//! (`SessionBuilder::mount_gos_chain`, `Vfs::resolve`, `GosChain::read`):
//!
//! * [`GosNameMatch::RequestUppercase`] implements the original's shape,
//!   and its answers **differ from both existing rules** on a
//!   mixed-case container key: the stored spelling is never folded, so
//!   neither its exact spelling nor any case-folded spelling of it is found,
//!   while `AsciiInsensitive` finds it and `ExactSpelling` finds only the
//!   byte-equal request;
//! * the two consequences the finding derives follow from the
//!   implementation: a stored ASCII-uppercase name answers under every
//!   casing of the request (and under both separator spellings — the
//!   request's `\` or `/` is part of the request, never of the stored
//!   name), and a stored name holding a lowercase ASCII
//!   letter can never be answered by any request;
//! * the rule is selectable per chain like the existing two, a chain still
//!   refuses a second, different rule, and `GosChain::name_match()` and the
//!   resolve trace report it (and a non-GOS lookup reports none).
//!
//! The synthetic tests author ROF containers under the system temporary
//! directory (`common::TempTree`) and mount them through the production
//! chain with the registry-key-absent case (no patch container), so the
//! fixture is `crimson.rof` plus the loose `GOSDATA` tree. No test here
//! needs `$CS_GAME_DIR`; the retail tests that must stay green unchanged are
//! `accept_t693_…` and `accept_f04_d_gos_registration_order_…`.

mod common;

use common::TempTree;
use cs_assets::install::sha256;
use cs_assets::vfs::{
    AttemptOutcome, ContentSession, ExePathOrigin, GosChain, GosError, GosInstall, GosNameMatch,
    LOOSE_MOUNT_ID, LookupOrder, MAIN_MOUNT_ID, ResolveError, SessionBuilder, gos_key,
};
use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_DIRECTORY, RECORD_BYTES};
use cs_types::asset_id::{AssetKey, ResolveContext};

// ------------------------------------------------------------ fixtures ---

/// One authored file entry of a directory block: its name and the bytes the
/// container stores for it.
struct Entry<'a> {
    name: &'a str,
    stored: Vec<u8>,
}

impl<'a> Entry<'a> {
    fn plain(name: &'a str, bytes: &[u8]) -> Self {
        Self {
            name,
            stored: bytes.to_vec(),
        }
    }
}

fn names(names: &[&str]) -> Vec<u8> {
    let mut table = Vec::new();
    for name in names {
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table
}

/// One directory block: header, 24-byte records (`start`, `raw_length` =
/// decoded count, `raw_length_on_disk` = stored count, `flags`,
/// `name_length`, `id`), name table.
fn block(records: &[[u32; 6]], names: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for record in records {
        for word in record {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes.extend_from_slice(names);
    bytes
}

/// A two-level container, `<first>/<second>/<entries>`: the retail shape,
/// whose members read `ASSETS/GRAPHICS/<entry>`.
fn nested_container(first: &str, second: &str, entries: &[Entry<'_>]) -> Vec<u8> {
    let first_names = names(&[second]);
    let first_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + first_names.len();
    let entry_names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
    let second_names = names(&entry_names);
    let second_len = DIRECTORY_HEADER_BYTES + entries.len() * RECORD_BYTES + second_names.len();

    let root_names = names(&[first]);
    let root_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + root_names.len();
    let root = block(
        &[[
            // The first block begins right after the root block, so the root
            // record's `start` is the root block's own length.
            root_len as u32,
            0,
            0,
            FLAG_DIRECTORY,
            first.len() as u32 + 1,
            1,
        ]],
        &root_names,
    );
    let first_block = block(
        &[[
            // The second block begins right after this one.
            (root_len + first_len) as u32,
            0,
            0,
            FLAG_DIRECTORY,
            second.len() as u32 + 1,
            2,
        ]],
        &first_names,
    );
    let mut cursor = (root_len + first_len + second_len) as u32;
    let mut records = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let stored = entry.stored.len() as u32;
        records.push([
            cursor,
            stored,
            stored,
            0,
            entry.name.len() as u32 + 1,
            10 + index as u32,
        ]);
        cursor += stored;
    }
    let mut bytes = root;
    bytes.extend_from_slice(&first_block);
    bytes.extend_from_slice(&block(&records, &second_names));
    for entry in entries {
        bytes.extend_from_slice(&entry.stored);
    }
    assert_eq!(bytes.len(), cursor as usize, "every payload placed once");
    bytes
}

/// One mounted chain over a synthetic installation, plus the tree its files
/// live in (kept so the chain's sources stay readable).
struct Chain {
    session: ContentSession,
    chain: GosChain,
    _tree: TempTree,
}

/// Mounts `main` as `GOSDATA/Assets/crimson.rof` and `loose` (spellings
/// relative to `GOSDATA`) as the loose tree, in the registry-key-absent case
/// — so the chain is `crimson.rof` then the loose directory, with no patch —
/// under the `matching` rule.
fn mount(main: &[u8], loose: &[(&str, &[u8])], matching: GosNameMatch) -> Chain {
    let tree = TempTree::new("t714-request-fold");
    tree.write("GOSDATA/Assets/crimson.rof", main);
    for (spelling, bytes) in loose {
        tree.write(&format!("GOSDATA/{spelling}"), bytes);
    }
    let install =
        GosInstall::new(tree.root(), ExePathOrigin::RegistryKeyAbsent).with_name_match(matching);
    let context = ResolveContext::new(sha256(tree.root().as_os_str().as_encoded_bytes()));
    let mut builder = SessionBuilder::new(context);
    let chain = builder
        .mount_gos_chain(&install)
        .expect("the synthetic chain mounts");
    Chain {
        session: builder.open(),
        chain,
        _tree: tree,
    }
}

fn key(spelling: &str) -> AssetKey {
    gos_key(spelling).expect("the fixture key spelling is valid")
}

// ----------------------------------------------------------- synthetic ---

/// The shape, on a **mixed-case container key**: the stored spelling
/// `ASSETS/GRAPHICS/BigDog.TGA` is never folded by the request-only rule,
/// so `RequestUppercase` answers none of the three spellings of the request
/// that the two existing rules answer between them — `AsciiInsensitive`
/// finds the member under a folded request, `ExactSpelling` finds it only
/// under the byte-equal one. One key, three rules, three different answers.
#[test]
fn accept_f04_d_gos_request_fold_rule_mixed_case_key_differs_from_both_existing_rules() {
    let main = nested_container(
        "ASSETS",
        "GRAPHICS",
        &[Entry::plain("BigDog.TGA", b"mixed case bytes")],
    );

    // The documented default folds both sides: the mixed-case stored name is
    // found under a fully folded request, and the answer keeps the stored
    // spelling.
    let folding = mount(&main, &[], GosNameMatch::AsciiInsensitive);
    let served = folding
        .session
        .resolve(&key("assets/graphics/bigdog.tga"))
        .expect("the folding rule finds the mixed-case member");
    assert_eq!(served.resolved().mount.as_str(), MAIN_MOUNT_ID);
    assert_eq!(
        served.resolved().span.member_key(),
        Some("ASSETS/GRAPHICS/BigDog.TGA"),
        "the stored spelling is kept"
    );

    // The exact rule folds neither side: only the byte-equal request is
    // answered.
    let exact = mount(&main, &[], GosNameMatch::ExactSpelling);
    assert!(
        exact
            .session
            .resolve(&key("ASSETS/GRAPHICS/BigDog.TGA"))
            .is_ok(),
        "the exact rule answers the byte-equal spelling"
    );
    assert!(
        matches!(
            exact.session.resolve(&key("ASSETS/GRAPHICS/BIGDOG.TGA")),
            Err(ResolveError::NotFound { .. })
        ),
        "the exact rule misses a differently-cased request"
    );

    // The request-only rule folds the request and leaves the stored spelling
    // alone — so on a mixed-case stored name **no** request can be found:
    // folding the request only ever produces uppercase, and uppercasing the
    // exact spelling leaves it mixed. That is the original's shape, not a
    // defect: #693 section B derives the same unreachable-spelling
    // consequence for any stored name holding a lowercase letter.
    let request_fold = mount(&main, &[], GosNameMatch::RequestUppercase);
    for spelling in [
        "ASSETS/GRAPHICS/BigDog.TGA",
        "ASSETS/GRAPHICS/BIGDOG.TGA",
        "assets/graphics/bigdog.tga",
        "ASSETS\\GRAPHICS\\BIGDOG.TGA",
        "assets\\graphics\\bigdog.tga",
    ] {
        assert!(
            matches!(
                request_fold.session.resolve(&key(spelling)),
                Err(ResolveError::NotFound { .. })
            ),
            "the request-only rule leaves a mixed-case stored spelling unreachable ({spelling})"
        );
    }
}

/// The other side of the same shape, on retail-shaped content: a stored
/// ASCII-uppercase name answers under **every** casing of the request, from
/// the container — the loose tree's lowercase spelling of the same image is
/// unreachable under this rule, so the container is the only answer. The
/// request's separator spelling is part of the request, not the stored
/// name, so `\` and `/` spellings of one path answer alike — the original
/// splits a request on `\` (`0x1001b238`), while this VFS's flattened
/// member spellings are `/`-joined, so both separator spellings are the
/// same canonical request here. This is the measured #693 premise
/// (`accept_t693_…`) expressed by the rule itself.
#[test]
fn accept_f04_d_gos_request_fold_rule_uppercase_stored_name_answers_every_casing() {
    let main = nested_container(
        "ASSETS",
        "GRAPHICS",
        &[Entry::plain("ARIAL8.TGA", b"container arial8")],
    );
    let chain = mount(
        &main,
        &[("Assets/GRAPHICS/arial8.tga", b"loose arial8")],
        GosNameMatch::RequestUppercase,
    );
    assert_eq!(chain.chain.name_match(), GosNameMatch::RequestUppercase);

    for spelling in [
        "assets/graphics/arial8.tga",
        "ASSETS/GRAPHICS/ARIAL8.TGA",
        "assets\\graphics\\arial8.tga",
        "ASSETS\\GRAPHICS\\ARIAL8.TGA",
    ] {
        let asset = chain
            .session
            .resolve(&key(spelling))
            .expect("the uppercase stored name answers every casing");
        assert_eq!(asset.resolved().mount.as_str(), MAIN_MOUNT_ID);
        assert_eq!(
            asset.resolved().span.member_key(),
            Some("ASSETS/GRAPHICS/ARIAL8.TGA")
        );
        assert_eq!(
            asset.resolved().trace.name_match,
            Some(GosNameMatch::RequestUppercase),
            "the trace reports the rule that decided"
        );
        let bytes = chain
            .chain
            .read(&chain.session, &asset)
            .expect("the served member reads");
        assert_eq!(bytes, b"container arial8");
    }

    // A request the rule misses reports the rule too: the loose spelling
    // holds a lowercase letter, so no request can upper-case into it.
    let missing = chain.session.resolve(&key("assets/graphics/other.tga"));
    let Err(ResolveError::NotFound { trace, .. }) = missing else {
        panic!("an unknown name is not found under any rule");
    };
    assert_eq!(
        trace.name_match,
        Some(GosNameMatch::RequestUppercase),
        "a failure trace reports the rule as well"
    );
    assert!(
        trace
            .attempts
            .iter()
            .all(|attempt| attempt.outcome == AttemptOutcome::Miss),
        "every source misses under the request-only rule"
    );
    assert_eq!(
        trace.attempts[0].mount.as_str(),
        MAIN_MOUNT_ID,
        "the container is consulted first, as registered"
    );
    assert_eq!(trace.attempts[1].mount.as_str(), LOOSE_MOUNT_ID);
}

/// The consequence the finding derives, measured through the implementation:
/// a stored name holding a lowercase ASCII letter is unreachable under the
/// request-only rule — while the default rule answers the very same
/// installation the same way retail is answered (see `accept_t693_…`).
#[test]
fn accept_f04_d_gos_request_fold_rule_lowercase_stored_name_is_never_answered() {
    let main = nested_container(
        "ASSETS",
        "SCRIPTS",
        &[Entry::plain("MainOnly.script", b"lowercase stored")],
    );
    let request_fold = mount(&main, &[], GosNameMatch::RequestUppercase);
    for spelling in [
        "ASSETS/SCRIPTS/MAINONLY.SCRIPT",
        "ASSETS/SCRIPTS/MainOnly.script",
        "assets/scripts/mainonly.script",
    ] {
        assert!(
            matches!(
                request_fold.session.resolve(&key(spelling)),
                Err(ResolveError::NotFound { .. })
            ),
            "no request upper-cases into a lowercase stored spelling ({spelling})"
        );
    }

    // The default rule finds the very same member, so the fixture really
    // holds it and only the rule differs.
    let folding = mount(&main, &[], GosNameMatch::AsciiInsensitive);
    assert_eq!(
        folding
            .session
            .resolve(&key("assets/SCRIPTS/mainonly.script"))
            .expect("the default rule finds the lowercase member")
            .resolved()
            .span
            .member_key(),
        Some("ASSETS/SCRIPTS/MainOnly.script")
    );
}

/// The rule is a per-chain input like the existing two: a chain states it,
/// `GosChain::name_match()` reports it with its stable label, stating the
/// same rule twice is accepted, and a second, **different** rule for the one
/// key space is refused rather than mixed.
#[test]
fn accept_f04_d_gos_request_fold_rule_is_selectable_and_a_second_different_rule_is_refused() {
    let main = nested_container(
        "ASSETS",
        "GRAPHICS",
        &[Entry::plain("ARIAL8.TGA", b"container arial8")],
    );
    let chain = mount(&main, &[], GosNameMatch::RequestUppercase);
    assert_eq!(chain.chain.name_match(), GosNameMatch::RequestUppercase);
    assert_eq!(chain.chain.name_match().label(), "request_uppercase");
    assert!(
        chain.chain.name_match().folds(),
        "the rule folds the request's case"
    );

    let tree = TempTree::new("t714-conflict");
    tree.write("GOSDATA/Assets/crimson.rof", &main);
    let install = || GosInstall::new(tree.root(), ExePathOrigin::RegistryKeyAbsent);
    let context = ResolveContext::new(sha256(tree.root().as_os_str().as_encoded_bytes()));
    let mut builder = SessionBuilder::new(context);
    // The rule is stated per chain, and stating the same one repeatedly is
    // accepted — only a *different* second rule is a conflict.
    builder
        .set_gos_name_match(GosNameMatch::RequestUppercase)
        .expect("the rule can be stated before any chain mounts");
    builder
        .set_gos_name_match(GosNameMatch::RequestUppercase)
        .expect("stating the same rule twice is accepted");
    builder
        .mount_gos_chain(&install().with_name_match(GosNameMatch::RequestUppercase))
        .expect("the chain mounts under the stated rule");
    let refused =
        builder.mount_gos_chain(&install().with_name_match(GosNameMatch::AsciiInsensitive));
    let message = refused
        .as_ref()
        .err()
        .unwrap_or_else(|| panic!("a second, different rule is refused"))
        .to_string();
    assert!(
        message.contains("request_uppercase") && message.contains("ascii_insensitive"),
        "the refusal names both rules: {message}"
    );
    assert!(
        matches!(refused, Err(GosError::Session(_))),
        "the refusal comes from the session's one-rule-per-key-space contract"
    );
}

/// The rule is reported on the resolve trace for a GOS key — and only
/// there: a key of another namespace is answered by the VFS's fixed ASCII
/// fold and reports no chain-stated rule, so one key space's rule can never
/// be read into another's answer.
#[test]
fn accept_f04_d_gos_request_fold_rule_trace_reports_it_only_for_gos_keys() {
    let main = nested_container(
        "ASSETS",
        "GRAPHICS",
        &[Entry::plain("ARIAL8.TGA", b"container arial8")],
    );
    let chain = mount(&main, &[], GosNameMatch::RequestUppercase);

    let gos = chain
        .session
        .resolve(&key("ASSETS/GRAPHICS/ARIAL8.TGA"))
        .expect("the GOS lookup resolves");
    assert_eq!(
        gos.resolved().trace.order.order,
        LookupOrder::GosRegistration
    );
    assert_eq!(
        gos.resolved().trace.name_match,
        Some(GosNameMatch::RequestUppercase)
    );

    // An `install` key over the same session: no mount of that namespace is
    // registered, so it is not found — and its trace carries no GOS rule.
    let other = cs_types::asset_id::AssetKey::from_spelling(
        "install",
        "ASSETS/GRAPHICS/ARIAL8.TGA",
        "default",
    )
    .expect("a valid key");
    let Err(ResolveError::NotFound { trace, .. }) = chain.session.resolve(&other) else {
        panic!("no install-namespace mount is registered here");
    };
    assert_eq!(trace.name_match, None, "a non-GOS lookup reports no rule");
    assert_eq!(trace.order.order, LookupOrder::Precedence);
}
