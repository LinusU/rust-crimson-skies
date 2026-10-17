//! Acceptance stage F47-C2: the front end's declared scrapbook inputs.
//!
//! [`cs_app::ui::scrapbook::declared_inputs`] is the production join the
//! F47-C1 screen was missing: the installed original table discovered, its
//! resource names read out of the installation's own headers, and the
//! records declared into the `ScrapbookCatalog` the pages project. A missing
//! installation is an error, never an empty catalog.

use std::path::Path;

use cs_app::ui::front_end::ScrapbookInputs;
use cs_app::ui::scrapbook::{DeclaredScrapbookError, declared_inputs};

/// A path that is not an installation refuses rather than hand the screen an
/// empty scrapbook.
#[test]
fn accept_f47_e_a_missing_installation_is_an_error_not_an_empty_catalog() {
    let missing = std::env::temp_dir().join(format!("cs-f47-e-no-install-{}", std::process::id()));
    let error = declared_inputs(&missing, 12).unwrap_err();
    assert!(
        matches!(error, DeclaredScrapbookError::Discover(_)),
        "the failure is the discovery's own, named: {error}"
    );
}

/// A zero page size is refused at the inputs boundary, after the catalog
/// builds — the same rule `ScrapbookUi::new` applies.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f47_e_retail_inputs_feed_the_paged_screen() {
    let game_dir =
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the owner's installation");

    let error = declared_inputs(Path::new(&game_dir), 0).unwrap_err();
    assert!(
        matches!(error, DeclaredScrapbookError::Page(_)),
        "a zero page size is the page bound's own refusal: {error}"
    );

    let inputs = declared_inputs(Path::new(&game_dir), 18)
        .expect("the original installation declares its scrapbook");
    let ScrapbookInputs { catalog, page_size } = &inputs;
    assert_eq!(*page_size, 18);
    assert_eq!(
        catalog.entries().count(),
        461,
        "the screen's catalog is the whole original table"
    );
    assert!(
        catalog.unlocked(&|_| true).is_empty(),
        "unknown unlock rules never unlock, so nothing is invented as earned"
    );
    assert_eq!(
        catalog.visible(&|_| true).count(),
        461,
        "every entry shows locked — the book has data, nothing is earned"
    );
}
