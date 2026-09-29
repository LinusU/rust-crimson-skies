//! The `<NAME>` placeholder pass of the keyed field list dialect (task #370,
//! keyed `F12-E`).
//!
//! Task #351 measured the placeholder rule from the retail members and
//! recorded that the reader does not yet implement it
//! (`docs/findings/2026-09-29-t351-keyed-list-reading-rules.md`, section
//! "Measured and recorded, but not implemented"). This module is that second
//! pass: the reader still yields one node per line and never expands
//! anything, and a caller that wants the names resolved runs
//! [`read_placeholders`] over the parsed list.
//!
//! # What is established from the retail data
//!
//! * A **definition** is an entry whose key is `V<digits>` or `G<digits>`
//!   and whose value has exactly two fields, `NAME,value`
//!   ([`definition_scope`]). `ASSETS/LAYOUT.CSV` has 186 of them: 157
//!   section-local `V` definitions and 29 global `G` ones in `[GLOBALVARS]`.
//! * A **reference** is a field that is exactly `<NAME>`
//!   ([`placeholder_name`]): all 1313 references of the member are whole
//!   fields, and no field mixes a placeholder with other text.
//! * A reference resolves against the definitions of its **own section**
//!   first (793 references) and against the global definitions otherwise
//!   (520). All 1313 resolve, none is unresolved, and no section defines a
//!   name the global table also defines.
//! * `ASSETS/SCRAPBOOK.CSV` has no definition and no reference at all.
//!
//! # What stays unknown and is not assumed
//!
//! * **Case.** Every observed name and reference is uppercase, so whether a
//!   placeholder name is compared with or without regard to ASCII case is
//!   unobserved. This pass compares the bytes exactly; a name spelled in
//!   another case is [`unresolved`](PlaceholderTable::unresolved) rather
//!   than folded.
//! * **Precedence.** No local definition shadows a global one in the
//!   surveyed member, so whether the original consults the local or the
//!   global table first is unobserved. This pass consults the local table
//!   first, which is the reading the task states; with no shadowing the
//!   choice cannot change a retail result.
//! * **The expansion itself.** Whether the original performs a textual
//!   substitution of the definition's value into the field, or something
//!   else (a typed assignment, a macro the interpreter evaluates), is not
//!   measured from the data. [`PlaceholderTable::resolved`] hands back the
//!   definition's value; a consumer that needs a number must still declare
//!   the width, signedness and range of the field it wants
//!   (`cs_content::config::TuningSchema`, spec F12 non-negotiable #2).
//! * **Quoting and padding interactions** with a placeholder, and whether a
//!   definition's value may itself carry a placeholder, were never observed.
//!   A quoted field is not read as a reference; a padded but unquoted field
//!   is, because #351's **R4** already drops the blank bytes around a field
//!   before a consumer compares it.
//!
//! The pass is bounded and budgeted like every other reader: the definition
//! and reference tables and the bytes they own are all booked against the
//! [`ParseContext`] allocation budget, and a refused pass charges nothing.
//! The raw line nodes are untouched, so [`KeyedList::reassemble`] still
//! returns the member byte for byte.

use std::mem::size_of;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext};
use crate::text::keyed_list::{Entry, Field, Fields, KeyedList};

/// Entrypoint label [`read_placeholders`] scopes its errors with.
pub const PLACEHOLDER_ENTRYPOINT: &str = "text.placeholders";

/// Which name table a definition belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceholderScope {
    /// A `V<digits>` definition, read in the section that spells it.
    Local,
    /// A `G<digits>` definition, read from any section. Every surveyed one
    /// sits in `[GLOBALVARS]`.
    Global,
}

impl PlaceholderScope {
    /// Stable, machine-matchable label.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Global => "global",
        }
    }
}

/// The scope of the variable-definition key `key`, or `None` when the key is
/// not one.
///
/// Established from `ASSETS/LAYOUT.CSV`: every one of its 186 definitions has
/// a key of the shape `V<digits>` or `G<digits>` and a value of exactly two
/// fields `NAME,value`. Whether the original tells a definition from an object
/// record by that key or by its first field not being a record letter is not
/// settled — on every observed line the two rules agree (recorded in
/// `docs/findings/2026-09-29-t351-keyed-list-reading-rules.md`, "The record
/// kinds").
pub fn definition_scope(key: &[u8]) -> Option<PlaceholderScope> {
    let (first, digits) = key.split_first()?;
    let scope = match first {
        b'V' => PlaceholderScope::Local,
        b'G' => PlaceholderScope::Global,
        _ => return None,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(scope)
}

/// The name between the angle brackets when `field` is exactly one `<NAME>`
/// reference, or `None`.
///
/// The field passed here is the form a consumer compares
/// ([`Field::value`], **R4**): blank bytes around the field already dropped.
/// A field that mixes a placeholder with other text
/// (`<A>-<B>`, `<A>.png`) is *not* a reference — no surveyed field has that
/// shape, so the pass keeps it whole rather than guessing how to substitute
/// into it.
pub fn placeholder_name(field: &[u8]) -> Option<&[u8]> {
    let name = field.strip_prefix(b"<")?.strip_suffix(b">")?;
    if name.is_empty() || name.contains(&b'<') || name.contains(&b'>') {
        return None;
    }
    Some(name)
}

/// One `V<digits>`/`G<digits>` definition: the name a `<NAME>` reference
/// resolves to and the value it stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceholderDefinition {
    /// The 1-based line of the defining entry.
    pub line: u64,
    /// The section header line index the definition follows, or `None`
    /// before the first header. Only a [`PlaceholderScope::Local`]
    /// definition uses it.
    pub section: Option<usize>,
    /// The definition key as written (`V1`, `G29`).
    pub key: Vec<u8>,
    /// Which table it belongs to.
    pub scope: PlaceholderScope,
    /// The first field: the name the reference spells.
    pub name: Vec<u8>,
    /// The second field: what the reference stands for.
    pub value: Vec<u8>,
}

/// One `<NAME>` reference field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceholderReference {
    /// The 1-based line of the entry that carries the reference.
    pub line: u64,
    /// The section header line index of that entry.
    pub section: Option<usize>,
    /// The entry key.
    pub key: Vec<u8>,
    /// Which field of the value, 0-based.
    pub field: usize,
    /// The name between the angle brackets.
    pub name: Vec<u8>,
    /// The index of the definition it resolved to, or `None` when no
    /// definition in scope carries that name exactly.
    pub definition: Option<usize>,
}

/// Where one reference resolved, and to what.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPlaceholder<'a> {
    /// The table the definition came from.
    pub scope: PlaceholderScope,
    /// The definition's value, blank bytes around it dropped.
    pub value: &'a [u8],
}

/// The scoped name table and every reference read through it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceholderTable {
    definitions: Vec<PlaceholderDefinition>,
    references: Vec<PlaceholderReference>,
}

impl PlaceholderTable {
    /// Every definition, in member order, local and global together.
    pub fn definitions(&self) -> &[PlaceholderDefinition] {
        &self.definitions
    }

    /// Every reference, in member order.
    pub fn references(&self) -> &[PlaceholderReference] {
        &self.references
    }

    /// The definition a reference resolved to, or `None` when it did not
    /// resolve.
    pub fn definition(&self, reference: &PlaceholderReference) -> Option<&PlaceholderDefinition> {
        self.definitions.get(reference.definition?)
    }

    /// Where a reference resolved and to what value, or `None` when no
    /// definition in scope carries its name.
    pub fn resolved(&self, reference: &PlaceholderReference) -> Option<ResolvedPlaceholder<'_>> {
        let definition = self.definition(reference)?;
        Some(ResolvedPlaceholder {
            scope: definition.scope,
            value: &definition.value,
        })
    }

    /// The references no definition in scope answered, in member order.
    ///
    /// An unresolved name is reported here, never replaced with a guess or
    /// dropped: it is one of the unknowns the caller's accounting has to see.
    pub fn unresolved(&self) -> impl Iterator<Item = &PlaceholderReference> {
        self.references
            .iter()
            .filter(|reference| reference.definition.is_none())
    }

    /// Counts of definitions, references and their resolutions.
    pub fn accounting(&self) -> PlaceholderAccounting {
        let mut accounting = PlaceholderAccounting {
            definitions: self.definitions.len(),
            references: self.references.len(),
            ..PlaceholderAccounting::default()
        };
        for definition in &self.definitions {
            match definition.scope {
                PlaceholderScope::Local => accounting.local_definitions += 1,
                PlaceholderScope::Global => accounting.global_definitions += 1,
            }
        }
        for reference in &self.references {
            match self
                .definition(reference)
                .map(|definition| definition.scope)
            {
                Some(PlaceholderScope::Local) => accounting.resolved_local += 1,
                Some(PlaceholderScope::Global) => accounting.resolved_global += 1,
                None => accounting.unresolved += 1,
            }
        }
        accounting
    }
}

/// What one placeholder pass read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaceholderAccounting {
    /// `V`/`G` definitions, both scopes.
    pub definitions: usize,
    /// Section-local (`V`) definitions.
    pub local_definitions: usize,
    /// Global (`G`) definitions.
    pub global_definitions: usize,
    /// `<NAME>` reference fields.
    pub references: usize,
    /// References a section-local definition answered.
    pub resolved_local: usize,
    /// References a global definition answered.
    pub resolved_global: usize,
    /// References no definition in scope answered.
    pub unresolved: usize,
}

/// Reads the `<NAME>` references of `list` and resolves them against the
/// scoped name table of its `V`/`G` definitions.
///
/// The raw nodes are not changed: this is a second pass that returns an owned
/// table beside the list. The only errors are the two tables and the bytes
/// they own not fitting this parse's allocation budget; a refused pass books
/// nothing.
pub fn read_placeholders(
    context: &mut ParseContext,
    list: &KeyedList<'_>,
) -> Result<PlaceholderTable, ParseError> {
    context.parse(
        PLACEHOLDER_ENTRYPOINT,
        list.bytes(),
        |_reader, allocation, _| scan_placeholders(allocation, list),
    )
}

/// The body of [`read_placeholders`] against an [`AllocationBudget`] the
/// caller already holds.
///
/// A consumer that runs under its own entrypoint (the `cs_content` document
/// parser, which reads a member inside one `parse` attempt) calls this
/// directly so the two do not parse the same bytes twice.
pub fn scan_placeholders(
    allocation: &mut AllocationBudget,
    list: &KeyedList<'_>,
) -> Result<PlaceholderTable, ParseError> {
    // Count first, so the two vectors are reserved before a single row is
    // pushed and a list with more references than the budget allows is
    // refused instead of growing past it.
    let (definition_count, reference_count) = count_placeholders(list);
    allocation.reserve(
        "placeholder_definitions",
        0,
        definition_count as u64,
        size_of::<PlaceholderDefinition>() as u64,
    )?;
    allocation.reserve(
        "placeholder_references",
        0,
        reference_count as u64,
        size_of::<PlaceholderReference>() as u64,
    )?;

    let mut definitions = Vec::with_capacity(definition_count);
    for (section, line, entry) in list.entries() {
        let Some(fields) = split_fields(entry) else {
            continue;
        };
        let Some(scope) = definition_scope(entry.key) else {
            continue;
        };
        if fields.len() != 2 {
            continue;
        }
        let name = fields[0].value();
        let value = fields[1].value();
        reserve_copy(allocation, entry.key, "placeholder_definition_key")?;
        reserve_copy(allocation, name, "placeholder_definition_name")?;
        reserve_copy(allocation, value, "placeholder_definition_value")?;
        definitions.push(PlaceholderDefinition {
            line: line.line.number,
            section,
            key: entry.key.to_vec(),
            scope,
            name: name.to_vec(),
            value: value.to_vec(),
        });
    }

    let mut references = Vec::with_capacity(reference_count);
    for (section, line, entry) in list.entries() {
        let Some(fields) = split_fields(entry) else {
            continue;
        };
        for (field, raw) in fields.iter().enumerate() {
            // A quoted field is not a reference: quoting a placeholder was
            // never observed, so the pass keeps the value whole.
            if raw.quoted {
                continue;
            }
            let Some(name) = placeholder_name(raw.value()) else {
                continue;
            };
            // The local table of the reference's own section first, then the
            // globals. Exact bytes, because the case fold of a placeholder
            // name is unobserved (module doc).
            let definition = definitions
                .iter()
                .position(|candidate| {
                    candidate.scope == PlaceholderScope::Local
                        && candidate.section == section
                        && candidate.name == name
                })
                .or_else(|| {
                    definitions.iter().position(|candidate| {
                        candidate.scope == PlaceholderScope::Global && candidate.name == name
                    })
                });
            reserve_copy(allocation, entry.key, "placeholder_reference_key")?;
            reserve_copy(allocation, name, "placeholder_reference_name")?;
            references.push(PlaceholderReference {
                line: line.line.number,
                section,
                key: entry.key.to_vec(),
                field,
                name: name.to_vec(),
                definition,
            });
        }
    }

    Ok(PlaceholderTable {
        definitions,
        references,
    })
}

/// The definition and reference counts of `list`, counted with the same
/// predicate the pass itself uses so the reservations are exact.
fn count_placeholders(list: &KeyedList<'_>) -> (usize, usize) {
    let mut definitions = 0usize;
    let mut references = 0usize;
    for (_, _, entry) in list.entries() {
        let Some(fields) = split_fields(entry) else {
            continue;
        };
        if definition_scope(entry.key).is_some() && fields.len() == 2 {
            definitions += 1;
        }
        references += fields
            .iter()
            .filter(|field| !field.quoted && placeholder_name(field.value()).is_some())
            .count();
    }
    (definitions, references)
}

/// The split fields of an entry's value, or `None` for a value the quoting
/// rules do not explain (which has no fields to read).
fn split_fields<'entry, 'bytes>(entry: &'entry Entry<'bytes>) -> Option<&'entry [Field<'bytes>]> {
    match &entry.fields {
        Fields::Split(fields) => Some(fields),
        Fields::Unsplit { .. } => None,
    }
}

/// Books `bytes` against `allocation` under `field`.
fn reserve_copy(
    allocation: &mut AllocationBudget,
    bytes: &[u8],
    field: &str,
) -> Result<(), ParseError> {
    allocation.reserve(field, 0, bytes.len() as u64, 1)?;
    Ok(())
}
