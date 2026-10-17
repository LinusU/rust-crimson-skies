//! The front end's scrapbook inputs for a mounted original installation
//! (F47-C2).
//!
//! [`declared_inputs`] is the production join the F47-C1 screen was missing:
//! [`DiscoveredScrapbook::discover`] reads `ASSETS/SCRAPBOOK.CSV` out of the
//! installed archive, [`resource_names`] reads the same archive's resource
//! headers (`ASSETS/SCRIPTS/RESOURCE.H` and `ASSETS/SCRIPTS/RESRC1.H`), and
//! [`declare`] turns the discovered records
//! into the [`ScrapbookCatalog`](cs_content::scrapbook::ScrapbookCatalog) the
//! pages project — every refusal on the way is an error, never an empty
//! scrapbook.

use std::path::Path;

use cs_content::scrapbook::{
    DeclareError, DiscoveredScrapbook, ScrapbookSourceError, declare, resource_names,
};

use super::PageError;
use crate::ui::front_end::ScrapbookInputs;

/// Why the front end's scrapbook inputs could not be built.
#[derive(Debug)]
pub enum DeclaredScrapbookError {
    /// The original scrapbook table could not be discovered.
    Discover(ScrapbookSourceError),
    /// The resource-name join or the catalog build failed.
    Declare(DeclareError),
    /// The caller's page size was refused.
    Page(PageError),
}

impl std::fmt::Display for DeclaredScrapbookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discover(error) => write!(f, "the scrapbook cannot be discovered: {error}"),
            Self::Declare(error) => write!(f, "the scrapbook cannot be declared: {error}"),
            Self::Page(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DeclaredScrapbookError {}

/// Builds the [`ScrapbookInputs`] a [`FlowSetup`](crate::ui::front_end::FlowSetup)
/// carries for the original installation mounted at `install_root`: one
/// [`ScrapbookEntry`](cs_content::scrapbook::ScrapbookEntry) per record the
/// table declares, `page_size` entries a page.
///
/// A mount, member, header or build refusal is an error — the screen never
/// sees an empty catalog standing in for missing original data.
///
/// # Errors
///
/// [`DeclaredScrapbookError`].
pub fn declared_inputs(
    install_root: &Path,
    page_size: usize,
) -> Result<ScrapbookInputs, DeclaredScrapbookError> {
    let discovered =
        DiscoveredScrapbook::discover(install_root).map_err(DeclaredScrapbookError::Discover)?;
    let names = resource_names(install_root).map_err(DeclaredScrapbookError::Declare)?;
    let declared = declare(&discovered, &|name| names.get(name).copied())
        .map_err(DeclaredScrapbookError::Declare)?;
    ScrapbookInputs::try_new(declared.catalog, page_size).map_err(DeclaredScrapbookError::Page)
}
