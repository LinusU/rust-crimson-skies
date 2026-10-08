//! Acceptance stage F47-D: the retail scrapbook captures
//! (`specs/F47-scrapbook-records-mementos-and-mission-replay.md`, section
//! `### F47-D`), evidence contract `docs/contracts/CLI-EVIDENCE.md`.
//!
//! The stage declares two capabilities, and this module is where the `gpu`
//! half is exercised: every page
//! [`cs_content::scrapbook::DiscoveredScrapbook`] discovers gets the original
//! picture its own `ImageName` resolves to, drawn on the real adapter through
//! [`cs_app::ui::front_end::capture_artwork`] and read back as a PNG under
//! `private/evidence/F47-D/` (or `$CS_EVIDENCE_DIR` when the acceptance run
//! supplies one). The frames are written by this engine's renderer over the
//! decoded original pixels; no original executable runs, and nothing here
//! claims what the original *presents*.
//!
//! The test is `#[ignore]`d: CI has no GPU adapter and no original data. Run
//! it with `--include-ignored` and `CS_GAME_DIR` set — the implementer and the
//! reviewer both do.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_app::ui::front_end::{FrontEndScreens, capture_artwork};
use cs_content::scrapbook::DiscoveredScrapbook;

/// Where captures are written: the evidence directory when the acceptance run
/// supplies one, otherwise this checkout's own private directory (ignored by
/// Git, like every other artifact of this task).
fn capture_dir() -> PathBuf {
    let dir = match std::env::var_os("CS_EVIDENCE_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() {
                dir
            } else {
                workspace_root().join(dir)
            }
        }
        None => workspace_root().join("private/evidence/F47-D-captures"),
    };
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    dir
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn retail_game_dir() -> PathBuf {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// SHA-256 of bytes as the workspace spells it, for a capture digest check.
fn sha256_hex(bytes: &[u8]) -> String {
    cs_assets::install::sha256(bytes).to_hex()
}

/// Every discovered page draws the original picture its own `ImageName`
/// resolves to: one measured frame per page, on the real adapter, each read
/// back from disk and paired with the pixels it was drawn from. A page whose
/// items name no picture the installation holds is a failure by name — the
/// owner's installation has none.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU adapter; run with --include-ignored"]
fn accept_f47_d_retail_gpu_every_discovered_page_draws_a_measured_frame() {
    let game_dir = retail_game_dir();
    let discovered = DiscoveredScrapbook::discover(&game_dir)
        .expect("the installation's scrapbook table reads end to end");
    let screens =
        FrontEndScreens::open(&game_dir).expect("the original artwork container reads end to end");
    let inventory = screens
        .inventory()
        .expect("the image inventory is complete");
    let by_name: BTreeMap<&str, &cs_app::ui::front_end::OriginalImage> = inventory
        .images
        .iter()
        .map(|image| (image.name.as_str(), image))
        .collect();

    let dir = capture_dir();
    let mut drawn = 0usize;
    let mut refused_flat = Vec::new();
    let mut without_picture = Vec::new();
    // Two captures may only agree when the pictures themselves agree: a frame
    // that repeats over different pixels means the capture ignored its
    // artwork, and a picture that repeats over different frames means the
    // capture is unstable.
    let mut frame_by_art: BTreeMap<String, String> = BTreeMap::new();
    let mut art_by_frame: BTreeMap<String, String> = BTreeMap::new();

    for page in &discovered.pages {
        let Some(item) = page.items.iter().find(|item| item.capture.is_some()) else {
            without_picture.push(page.page);
            continue;
        };
        let spelling = item
            .capture
            .as_deref()
            .expect("the item's capture is present");
        let image = by_name
            .get(spelling)
            .unwrap_or_else(|| panic!("page {}: the inventory holds no {spelling}", page.page));
        let art = screens.artwork(image).unwrap_or_else(|error| {
            panic!(
                "page {}: {spelling} could not be decoded: {error}",
                page.page
            )
        });
        assert_eq!(
            art.extent(),
            (image.width, image.height),
            "{spelling}'s pixels match its measured extent"
        );

        let stem = Path::new(spelling)
            .file_stem()
            .expect("a member has a file stem")
            .to_string_lossy()
            .into_owned();
        let png = dir.join(format!("f47-d-page-{:03}-{stem}.png", page.page));
        let _ = std::fs::remove_file(&png);
        if art.is_uniform() {
            let error = capture_artwork(spelling, &art, &[], &png)
                .expect_err("a flat original draws nothing and must be refused");
            assert!(
                matches!(
                    error,
                    cs_app::ui::front_end::ScreenCaptureError::UniformFrame { .. }
                ),
                "{spelling}: {error}"
            );
            assert!(!png.exists(), "a refused capture leaves no file");
            refused_flat.push(spelling.to_owned());
            continue;
        }
        let capture = capture_artwork(spelling, &art, &[], &png).unwrap_or_else(|error| {
            panic!(
                "page {}: the original picture {spelling} could not be captured: {error}",
                page.page
            )
        });
        assert!(
            capture.drew_screen(),
            "page {}: {spelling} drew a frame of {} distinct luminance levels",
            page.page,
            capture.distinct_luminance
        );
        assert!(png.is_file(), "{spelling} wrote {}", png.display());
        let bytes = std::fs::read(&png).expect("the capture reads back");
        assert_eq!(capture.png_sha256.to_hex(), sha256_hex(&bytes));

        let frame = capture.png_sha256.to_hex();
        let art_digest = sha256_hex(art.rgba());
        if let Some(previous) = frame_by_art.get(&art_digest) {
            assert_eq!(
                *previous, frame,
                "{spelling} is the same picture as an earlier capture but drew another frame"
            );
        } else {
            frame_by_art.insert(art_digest.clone(), frame.clone());
        }
        if let Some(previous) = art_by_frame.get(&frame) {
            assert_eq!(
                *previous, art_digest,
                "{spelling} drew a frame another picture already drew: the capture ignored its \
                 pixels"
            );
        } else {
            art_by_frame.insert(frame, art_digest);
        }
        drawn += 1;
    }

    assert!(
        without_picture.is_empty(),
        "every page of the owner's installation names at least one picture the container holds; \
         these name none: {without_picture:?}"
    );
    assert_eq!(
        drawn + refused_flat.len(),
        discovered.page_count(),
        "every page was either drawn or refused by name"
    );
    assert!(
        drawn > 0,
        "at least one page draws a real frame; refused flat {refused_flat:?}"
    );
    assert_eq!(
        art_by_frame.len(),
        frame_by_art.len(),
        "frames and pictures are in one-to-one correspondence: {} distinct frames over {} \
         distinct pictures from {drawn} captures",
        frame_by_art.len(),
        art_by_frame.len()
    );
    eprintln!(
        "{} scrapbook pages captured from {} discovered pages, {} refused flat",
        drawn,
        discovered.page_count(),
        refused_flat.len()
    );
}
