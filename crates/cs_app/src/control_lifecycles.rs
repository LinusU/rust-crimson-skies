//! The production reader for a control record's block lifecycles: the
//! per-block `BEGIN_DORMANT` and `TICK_DEPENDS_ON_OBJ` spellings, read into
//! the [`BlockLifecycleTable`] the mission host's fact fold answers through
//! (VS-M01-RT-HOST-BLOCK-LIFECYCLES, Rally #1282).
//!
//! Until this stage, **no production code anywhere turned a record's
//! `BEGIN_DORMANT` spellings into [`BlockLifecycleTable::declare`] calls**:
//! the mission host folded an empty table — its documented fail-closed read,
//! where every `Condition::ObjectiveAwake` answers `false` and no numbered
//! block can ever complete — under
//! [`MissionHostRefusal::BlockLifecycles`](crate::mission_session::MissionHostRefusal::BlockLifecycles)
//! (`crates/cs_app/src/mission_session/host.rs`). This module is that reader.
//!
//! # What is read, and from where it is measured
//!
//! [`read_block_lifecycles`] walks the decoded control document with the
//! **same asymmetric directive grammar** the record's parse and the control
//! lowering use (`cs_app::control_lowering`): a site is a key plus the list
//! beside it, a bare key is the measured no-argument spelling, and the
//! numbered blocks are the document's flat `OBJECTIVE<N>` fields in record
//! order — the zero-based position in that order is both the key
//! [`cs_script::ir::Condition::ObjectiveAwake`] reads and the index
//! [`BlockLifecycleTable::declare`] takes, so the two resolve in one key
//! space (`cs_script::conditions::lower_block_condition`).
//!
//! Two keys carry a block's starting lifecycle, both measured
//! (`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-
//! semantics.md`, and the parse sites in
//! `docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`):
//!
//! * **`BEGIN_DORMANT`** (`[float]` — presence is the whole lifecycle
//!   contribution): the key's presence sets the record's fields to
//!   **dormant** (`+0xc = 0`, `+0x5c8 = 0`, parse `0x467e09`), and child0 is
//!   stored as `+0x5d0`, the mission-clock second the block wakes itself at.
//!   A negative child0 (M01's `-1` sentinel over 1014 of the installation's
//!   1118 sites) disables the timed wake permanently; it stays spelled in
//!   the declaration as `Some(-1.0)` rather than being folded into `None`,
//!   because [`BlockLifecycleTable`] reads the record's own value.
//!   Measured: every site the installation spells carries child0 (1014
//!   sentinel plus 104 positive arguments), so a `BEGIN_DORMANT` site this
//!   build cannot read as exactly one finite number is **refused by block
//!   and key**, never treated as "dormant, never waking" (AGENTS.md rule 4).
//! * **`TICK_DEPENDS_ON_OBJ`** (`[int]`, parse `0x4679e1`): `+0x10 = child0 − 1`
//!   — the spelled integer is the dependency's one-based block number and
//!   the stored value is its zero-based record index. A spelled `0` lands on
//!   the record's `-1` "no dependency" sentinel and contributes no gate, the
//!   same as not spelling the key (the exact rule
//!   `cs_script::conditions::lower_block_condition` applies).
//!
//! The parse stores both fields **unconditionally**, so a second spelling of
//! one key in the same block overwrites the first; this reader mirrors that
//! measured behaviour (last site wins) rather than refusing a shape the
//! original accepts.
//!
//! A block **without** `BEGIN_DORMANT` is left awake by the record's own
//! defaults (`+0xc = 1`, `+0x5c8 = 1`), so it is declared
//! [`LifecycleDecl::awake`].
//!
//! # Fail-closed edges
//!
//! Every spelling outside the measured shapes above is refused **by block
//! and key** with the site's own detail — a bare `BEGIN_DORMANT`, a
//! `not_a_list` scalar beside either key, an argument list of the wrong
//! arity (the parse writes up to four `BEGIN_DORMANT` children, but children
//! 1–3 drive the nap and force-done timers whose measured writer is not
//! lowered yet — see [`BlockLifecycleTable`]'s own doc), a non-number
//! argument, a non-finite wake time, an int beyond the checked 32-bit value,
//! and a block value that is not a list. A repeated block key is walked
//! positionally — two block fields are two blocks — exactly as the control
//! lowering walks them, and the block's record number is its label, never
//! the lifecycle index (which is the record position, the key space
//! `Condition::ObjectiveAwake` reads).
//! A read with any refusal is **not complete**: [`BlockLifecycleRead::table`]
//! still carries what was declared (so a test can inspect the partial
//! accounting), but the mission host's stage only carries a **complete**
//! read to the seed and keeps the empty fail-closed table — plus the named
//! [`MissionHostRefusal::BlockLifecycles`](crate::mission_session::MissionHostRefusal::BlockLifecycles)
//! quoting these refusals — when the read is incomplete. Nothing half-read
//! ever answers a condition.
//!
//! [`read_mission_block_lifecycles`] is the installation-level entry: it
//! reads the mission's control member through
//! [`crate::mission_control::read_control_member`] — the production
//! control-member rule, the same member the census measures — and hands its
//! document to [`read_block_lifecycles`].
//!
//! No original executable ran; `retail` is read access to the owner's
//! installation. Nothing here is `verified_original`.

use std::fmt;
use std::path::Path;

use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};

/// The measured lifecycle key whose presence starts a block dormant.
const BEGIN_DORMANT: &str = "BEGIN_DORMANT";

/// The measured key carrying a block's tick-dependency target.
const TICK_DEPENDS_ON_OBJ: &str = "TICK_DEPENDS_ON_OBJ";

/// The label a refusal carries for a failure of the whole block rather than
/// of one directive site — a block value that is not a list, a key that is
/// not text, a duplicate `OBJECTIVE<N>` field.
const BLOCK_LEVEL: &str = "<block>";

/// Why one block's lifecycle spelling could not be read.
///
/// Every refusal names the block's key (`OBJECTIVE<N>`), the directive key
/// and the site's own detail, so a run always says exactly which spelling it
/// did not read (AGENTS.md rule 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleReadRefusal {
    /// The block's record key, e.g. `OBJECTIVE7`.
    block: String,
    /// The refusing directive key, or [`BLOCK_LEVEL`] for a block-level
    /// failure.
    key: String,
    /// Why the spelling is outside the measured shapes.
    detail: String,
}

impl fmt::Display for LifecycleReadRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` `{}`: {}", self.block, self.key, self.detail)
    }
}

/// What one lifecycle read produced: the table that was declared, and every
/// spelling the reader refused.
///
/// A complete read ([`Self::is_complete`]) is the record's own lifecycle,
/// ready for [`BlockLifecycleTable`] to fold; an incomplete one carries the
/// partial accounting for inspection only — the mission host's stage keeps
/// the empty fail-closed table and names the refusals instead
/// (`crates/cs_app/src/mission_session/host.rs`).
#[derive(Clone, Debug)]
pub struct BlockLifecycleRead {
    table: BlockLifecycleTable,
    refusals: Vec<LifecycleReadRefusal>,
}

impl BlockLifecycleRead {
    /// The declared table. On an incomplete read this is the partial
    /// accounting, never something a host may fold.
    #[must_use]
    pub const fn table(&self) -> &BlockLifecycleTable {
        &self.table
    }

    /// Every refusing block/key, in record order.
    #[must_use]
    pub fn refusals(&self) -> &[LifecycleReadRefusal] {
        &self.refusals
    }

    /// Whether every numbered block's lifecycle spelling was read.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.refusals.is_empty()
    }

    /// The declared table, for a caller that has already checked
    /// [`Self::is_complete`].
    #[must_use]
    pub fn into_table(self) -> BlockLifecycleTable {
        self.table
    }

    /// The refusals as one detail string — what the mission host's
    /// [`MissionHostRefusal::BlockLifecycles`](crate::mission_session::MissionHostRefusal::BlockLifecycles)
    /// quotes verbatim for an incomplete read.
    #[must_use]
    pub fn refusal_detail(&self) -> String {
        let joined: Vec<String> = self.refusals.iter().map(ToString::to_string).collect();
        format!(
            "the record's block lifecycles could not be read completely ({}); the table stays \
             empty and every Condition::ObjectiveAwake answers false",
            joined.join("; ")
        )
    }
}

/// Reads every numbered block's starting lifecycle out of a decoded control
/// record document, with the record's own asymmetric directive grammar.
///
/// The document is the control member the census measures — the same one
/// [`crate::mission_control::read_control_member`] returns — and the walk is
/// the one [`crate::control_lowering::lower_control_record`] performs, so a
/// block's declared lifecycle and its lowered condition are read from the
/// same sites in the same order.
///
/// Never fails and never guesses: a spelling outside the measured shapes is
/// a [`LifecycleReadRefusal`] of the read, and the caller decides whether the
/// partial table may be used (only a complete one may).
#[must_use]
pub fn read_block_lifecycles(document: &ZrdValue) -> BlockLifecycleRead {
    let mut table = BlockLifecycleTable::new();
    let mut refusals: Vec<LifecycleReadRefusal> = Vec::new();
    // One lifecycle slot per block, in record order — the same running index
    // the control lowering assigns, which is the key
    // `Condition::ObjectiveAwake` reads and `TICK_DEPENDS_ON_OBJ`'s
    // `child0 - 1` names.
    let mut block_index: u32 = 0;
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        if objective_block_number(key).is_none() {
            continue;
        }
        let index = block_index;
        block_index += 1;
        let Some(children) = value.as_list() else {
            refusals.push(LifecycleReadRefusal {
                block: key.to_owned(),
                key: BLOCK_LEVEL.to_owned(),
                detail: "the block's value is not a list, so its directive sites cannot be \
                         walked and its lifecycle is unreadable"
                    .to_owned(),
            });
            continue;
        };
        // The measured lifecycle spellings: presence of `BEGIN_DORMANT` is
        // the dormant contribution, its child0 the timed self-wake, and
        // `TICK_DEPENDS_ON_OBJ`'s `child0 - 1` the dependency. The parse
        // stores both unconditionally, so a second site of one key
        // overwrites the first — last site wins, exactly as at parse.
        let mut dormant: Option<f64> = None;
        let mut dependency: Option<u32> = None;
        let mut index_in_block = 0usize;
        while index_in_block < children.len() {
            let Some(name) = children[index_in_block].as_text() else {
                refusals.push(LifecycleReadRefusal {
                    block: key.to_owned(),
                    key: BLOCK_LEVEL.to_owned(),
                    detail: format!(
                        "site {index_in_block} is not text, so the block's directive list cannot \
                         be walked past it and its lifecycle is only partially readable"
                    ),
                });
                break;
            };
            let site = |detail: String| LifecycleReadRefusal {
                block: key.to_owned(),
                key: name.to_owned(),
                detail,
            };
            match children.get(index_in_block + 1) {
                Some(next) if next.as_list().is_some() => {
                    let items = next.as_list().unwrap_or_default();
                    match name {
                        BEGIN_DORMANT => match begin_dormant_wake(items) {
                            Ok(wake_at) => dormant = Some(wake_at),
                            Err(detail) => refusals.push(site(detail)),
                        },
                        TICK_DEPENDS_ON_OBJ => match tick_dependency(items) {
                            Ok(spelled) => dependency = spelled,
                            Err(detail) => refusals.push(site(detail)),
                        },
                        _ => {}
                    }
                    index_in_block += 2;
                }
                None | Some(ZrdValue::Text(_)) => {
                    // The measured bare spelling: a text follower is the next
                    // directive's key, and no follower ends the block. Neither
                    // lifecycle key has a measured bare site — presence alone
                    // cannot spell a wake time or a target — so it is refused
                    // rather than read as one.
                    if name == BEGIN_DORMANT || name == TICK_DEPENDS_ON_OBJ {
                        refusals.push(site(
                            "the measured sites of this key always spell an argument list beside \
                             the key, and this one is bare"
                                .to_owned(),
                        ));
                    }
                    index_in_block += 1;
                }
                Some(_) => {
                    // A scalar sits beside the key (`not_a_list`). It is not
                    // an argument list, so no lifecycle value can be read
                    // from it — refused by name, never coerced.
                    if name == BEGIN_DORMANT || name == TICK_DEPENDS_ON_OBJ {
                        refusals.push(site(
                            "the measured shape of this key is one argument list and this site \
                             spells a scalar beside its key (`not_a_list`)"
                                .to_owned(),
                        ));
                    }
                    index_in_block += 2;
                }
            }
        }
        let decl = match dormant {
            Some(wake_at) => LifecycleDecl::dormant(Some(wake_at)),
            None => LifecycleDecl::awake(),
        };
        let decl = match dependency {
            Some(target) => decl.depends_on(target),
            None => decl,
        };
        if let Err(error) = table.declare(index, decl) {
            refusals.push(LifecycleReadRefusal {
                block: key.to_owned(),
                key: BLOCK_LEVEL.to_owned(),
                detail: error.to_string(),
            });
        }
    }
    BlockLifecycleRead { table, refusals }
}

/// Reads one mission's block lifecycles from the installation: the mission's
/// control member through the production control-member rule, then
/// [`read_block_lifecycles`] over its decoded document.
///
/// `mission` is the mission scope spelling the census rows are keyed by
/// (`zbd/<group>/<mission>`), e.g. `plan.mission_dir`.
///
/// # Errors
///
/// A detail naming the absence when no control member could be read for the
/// mission at all — never an empty table presented as the record's
/// lifecycles.
pub fn read_mission_block_lifecycles(
    install_root: &Path,
    mission: &str,
) -> Result<BlockLifecycleRead, String> {
    let Some((document, _row)) = crate::mission_control::read_control_member(install_root, mission)
    else {
        return Err(format!(
            "no control member could be read for `{mission}` through the production \
             control-member rule, so the record's block lifecycles are unreadable"
        ));
    };
    Ok(read_block_lifecycles(&document))
}

/// The `BEGIN_DORMANT` wake time one measured site spells: exactly one
/// finite number, kept as the record wrote it (the `-1` sentinel stays
/// `Some(-1.0)` — [`BlockLifecycleTable`] reads the record's own value).
fn begin_dormant_wake(items: &[ZrdValue]) -> Result<f64, String> {
    let [only] = items else {
        return Err(format!(
            "the measured sites spell exactly one argument (child0, the mission-clock wake \
             second) and this site spells {}; children 1-3 write the nap and force-done timers \
             whose measured writer is not lowered yet, so the extra children are refused rather \
             than dropped",
            items.len()
        ));
    };
    match only {
        ZrdValue::Float(value) => {
            let wake_at = f64::from(*value);
            if wake_at.is_finite() {
                Ok(wake_at)
            } else {
                Err(format!("the wake time {wake_at} is not finite"))
            }
        }
        ZrdValue::Int(value) => Ok(f64::from(*value)),
        other => Err(format!(
            "the measured shape is one finite number and this site spells {}",
            value_label(other)
        )),
    }
}

/// The `TICK_DEPENDS_ON_OBJ` dependency one measured site spells: the
/// parse's own rule, `child0 - 1`, where a spelled `0` (or anything whose
/// decrement lands negative) is the record's "no dependency" sentinel.
fn tick_dependency(items: &[ZrdValue]) -> Result<Option<u32>, String> {
    let [only] = items else {
        return Err(format!(
            "the measured sites spell exactly one argument (the dependency's one-based block \
             number) and this site spells {}",
            items.len()
        ));
    };
    let ZrdValue::Int(target) = only else {
        return Err(format!(
            "the measured shape is one int and this site spells {}",
            value_label(only)
        ));
    };
    // The same checked 32-bit bound the control lowering carries each int
    // through: an int beyond it is not a value this build represents.
    let checked = i32::try_from(*target)
        .map_err(|_| format!("int {target} does not fit the checked 32-bit value"))?;
    let spelled = i64::from(checked) - 1;
    Ok((spelled >= 0).then_some(spelled as u32))
}

/// One refused argument node's kind, for a refusal's detail.
fn value_label(value: &ZrdValue) -> &'static str {
    match value {
        ZrdValue::Int(_) => "an int",
        ZrdValue::Float(_) => "a float",
        ZrdValue::Text(_) => "text",
        ZrdValue::List(_) => "a nested list",
    }
}
