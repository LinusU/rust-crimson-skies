//! Canonical content catalog and readiness accounting (F14-A).
//!
//! Spec F14, "Deliverable and interfaces": a `Catalog` holds stable-id
//! [`CatalogElement`] rows for every content collection and answers two
//! questions this stage owns. First, identity: an element is inserted once
//! by its [`ContentId`], a duplicate identity is refused rather than
//! silently merged (non-negotiable behavior 5), and the rows enumerate in
//! canonical id order, so the same content inserted in any order yields the
//! same list (AC02). Second, readiness over a **declared baseline**: the
//! launchable mission/scenario ids are declared explicitly, and the
//! [`Catalog::unsupported_count`] counts the declared rows whose element is
//! not ready. Because the denominator is the declared baseline and not the
//! set of rows that happen to be supported, an unsupported mission is
//! counted and prevents [`Catalog::is_fully_ready`] — it is never filtered
//! out to make the ratio look better (non-negotiable behavior 4, AC01).
//!
//! The catalog consumes the typed schema in `cs_types::content`; it does
//! not parse bytes. F14-B adds the two production paths over its rows:
//! [`normalize`] turns raw declared quantities into canonical, range-checked
//! values, and [`closure`] walks the transitive dependency graph from the
//! declared launchable roots, propagating an unsupported dependency into an
//! unavailable parent, reporting orphaned references and ownership cycles and
//! emitting a deterministic closure hash and JSON report. Those two paths
//! derive nothing from original game data; the catalog's own fixture rows
//! are authored content.
//!
//! [`baseline`] is the F14-D stage: it reads the original installation and
//! builds the complete private baseline inventory — every inventoried file,
//! every campaign mission program and one declared launchable row per
//! campaign mission — plus the reachable/unreachable coverage accounting, so
//! the denominator is fixed by the installation and never by a filtered list
//! of supported rows.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{CatalogElement, ContentId, ContentKind, ElementError};

pub mod baseline;
pub mod closure;
pub mod normalize;

/// The canonical content catalog: every row by identity plus the declared
/// launchable baseline readiness is measured against.
///
/// Rows live in a map keyed by [`ContentId`], so iteration is deterministic
/// (canonical id order) and duplicate identities are refused. The baseline
/// is a separate set of launchable mission/scenario ids: declaring a
/// baseline row does not require the row to be ready, which is exactly how
/// an unsupported mission stays counted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    elements: BTreeMap<ContentId, CatalogElement>,
    launchable: BTreeSet<ContentId>,
}

impl Catalog {
    /// An empty catalog with no declared baseline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one element, validating it and refusing a duplicate identity.
    ///
    /// # Errors
    ///
    /// [`CatalogError::Element`] when the element breaks its own rules, and
    /// [`CatalogError::DuplicateId`] when a row with the same identity is
    /// already present. A duplicate is never merged or overwritten: two
    /// contradictory rows must be resolved explicitly (non-negotiable
    /// behavior 5).
    pub fn insert(&mut self, element: CatalogElement) -> Result<(), CatalogError> {
        element.validate().map_err(CatalogError::Element)?;
        if self.elements.contains_key(&element.id) {
            return Err(CatalogError::DuplicateId { id: element.id });
        }
        self.elements.insert(element.id.clone(), element);
        Ok(())
    }

    /// The element with `id`, if it is present.
    pub fn get(&self, id: &ContentId) -> Option<&CatalogElement> {
        self.elements.get(id)
    }

    /// Every element, in canonical id order.
    pub fn elements(&self) -> impl Iterator<Item = &CatalogElement> {
        self.elements.values()
    }

    /// Every element id, in canonical order.
    pub fn sorted_ids(&self) -> Vec<&ContentId> {
        self.elements.keys().collect()
    }

    /// How many elements the catalog holds.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Whether the catalog holds no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Declares `id` a launchable mission/scenario: the readiness
    /// denominator.
    ///
    /// # Errors
    ///
    /// [`CatalogError::UnknownElement`] when no such row was inserted, and
    /// [`CatalogError::NotLaunchable`] when the row's kind is not a
    /// launchable mission or scenario.
    pub fn declare_launchable(&mut self, id: &ContentId) -> Result<(), CatalogError> {
        let Some(element) = self.elements.get(id) else {
            return Err(CatalogError::UnknownElement { id: id.clone() });
        };
        if !element.kind.is_launchable() {
            return Err(CatalogError::NotLaunchable {
                id: id.clone(),
                kind: element.kind,
            });
        }
        self.launchable.insert(id.clone());
        Ok(())
    }

    /// How many launchable missions/scenarios the baseline declares.
    pub fn launchable_count(&self) -> usize {
        self.launchable.len()
    }

    /// The declared launchable rows that are not ready, in canonical id
    /// order.
    ///
    /// A declared id always resolves to its inserted row, so every launchable
    /// row appears here or is ready — none is dropped.
    pub fn unsupported_launchables(&self) -> Vec<&CatalogElement> {
        self.launchable
            .iter()
            .filter_map(|id| self.elements.get(id))
            .filter(|element| !element.is_ready())
            .collect()
    }

    /// How many declared launchable missions/scenarios are not ready.
    ///
    /// Adding an unsupported mission to the baseline increases this count
    /// (AC01); it is never filtered out of the denominator.
    pub fn unsupported_count(&self) -> usize {
        self.unsupported_launchables().len()
    }

    /// Whether every declared launchable mission/scenario is ready.
    ///
    /// Readiness is over the declared baseline: an empty baseline is
    /// vacuously ready, and adding an unsupported mission makes this false.
    pub fn is_fully_ready(&self) -> bool {
        self.unsupported_count() == 0
    }

    /// The declared launchable rows whose origin is original installation
    /// data.
    ///
    /// Synthetic fixture rows are counted by
    /// [`Catalog::synthetic_launchable_count`] instead, so a synthetic
    /// launchable row can never be mistaken for a retail catalog entry
    /// (AC04).
    pub fn original_launchable_count(&self) -> usize {
        self.launchable
            .iter()
            .filter_map(|id| self.elements.get(id))
            .filter(|element| element.origin.is_original())
            .count()
    }

    /// The declared launchable rows that are not original installation data:
    /// synthetic fixtures and engine-authored designs.
    pub fn synthetic_launchable_count(&self) -> usize {
        self.launchable_count() - self.original_launchable_count()
    }

    /// Whether every declared launchable row could be used as a retail
    /// catalog entry: original origin and ready.
    ///
    /// This is deliberately stricter than [`Catalog::is_fully_ready`] — a
    /// catalog of ready synthetic rows is "fully ready" as a synthetic
    /// fixture but is `false` here, which is how AC04 stays honest.
    pub fn is_retail_ready(&self) -> bool {
        self.is_fully_ready() && self.synthetic_launchable_count() == 0
    }
}

/// Why a catalog operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// The element broke one of its per-record rules.
    Element(ElementError),
    /// A row with this identity is already in the catalog. The catalog
    /// refuses to choose between contradictory duplicates.
    DuplicateId {
        /// The repeated identity.
        id: ContentId,
    },
    /// A launchable declaration named an element that was never inserted.
    UnknownElement {
        /// The unknown identity.
        id: ContentId,
    },
    /// A launchable declaration named a non-launchable kind.
    NotLaunchable {
        /// The declared identity.
        id: ContentId,
        /// Its kind.
        kind: ContentKind,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Element(error) => write!(f, "{error}"),
            Self::DuplicateId { id } => {
                write!(f, "catalog already holds an element with id {id}")
            }
            Self::UnknownElement { id } => {
                write!(f, "no catalog element has id {id}")
            }
            Self::NotLaunchable { id, kind } => {
                write!(f, "{id} is a {kind}, which is not a launchable scenario")
            }
        }
    }
}

impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Element(error) => Some(error),
            Self::DuplicateId { .. } | Self::UnknownElement { .. } | Self::NotLaunchable { .. } => {
                None
            }
        }
    }
}
