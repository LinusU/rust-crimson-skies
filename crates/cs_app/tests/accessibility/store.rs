use std::fs;
use std::path::PathBuf;

use cs_app::accessibility::store::{LoadError, StartupOrigin, load, save_atomic, startup};
use cs_content::settings::{ColourFilter, Settings};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f52-a-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("dir");
        Self(root)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tuned() -> Settings {
    let mut s = Settings::designed();
    s.presentation.colour_filter = ColourFilter::Deuteranopia;
    s.presentation.ui_scale_percent = 200;
    s
}

#[test]
fn accept_f52_a_a_saved_file_loads_back_and_startup_uses_it() {
    let dir = Dir::new("roundtrip");
    let path = dir.0.join("settings.txt");
    save_atomic(&path, &tuned()).expect("save");
    assert_eq!(load(&path).expect("load"), tuned());
    let start = startup(&path, false);
    assert!(matches!(start.origin, StartupOrigin::Loaded));
    assert_eq!(start.settings, tuned());
    assert!(!dir.0.join("settings.txt.tmp").exists());
}

#[test]
fn accept_f52_a_a_failed_save_leaves_the_previous_file_whole() {
    let dir = Dir::new("atomic");
    let path = dir.0.join("settings.txt");
    save_atomic(&path, &tuned()).expect("save");
    let before = fs::read(&path).expect("read");

    // The temporary name is taken by a directory, so the write cannot start.
    fs::create_dir(dir.0.join("settings.txt.tmp")).expect("block");
    let mut next = tuned();
    next.presentation.subtitles = true;
    assert!(save_atomic(&path, &next).is_err());
    assert_eq!(fs::read(&path).expect("read"), before);

    // Settings that fail validation are never written.
    fs::remove_dir(dir.0.join("settings.txt.tmp")).expect("unblock");
    let mut bad = tuned();
    bad.presentation.ui_scale_percent = 5;
    assert!(save_atomic(&path, &bad).is_err());
    assert_eq!(fs::read(&path).expect("read"), before);
}

#[test]
fn accept_f52_a_safe_defaults_ignore_the_file_and_an_unusable_file_recovers() {
    let dir = Dir::new("safe");
    let path = dir.0.join("settings.txt");

    let none = startup(&path, false);
    assert!(matches!(none.origin, StartupOrigin::NoFile));
    assert_eq!(none.settings, Settings::designed());

    // An unusable display configuration, as a saved file would carry it.
    let text = tuned().to_text().replace("width=1280", "width=0");
    fs::write(&path, &text).expect("write");
    let recovered = startup(&path, false);
    assert!(matches!(
        recovered.origin,
        StartupOrigin::Recovered(LoadError::Invalid(_))
    ));
    assert_eq!(recovered.settings, Settings::designed());

    // The flag skips even a valid file, and neither path rewrites it.
    save_atomic(&path, &tuned()).expect("save");
    let safe = startup(&path, true);
    assert!(matches!(safe.origin, StartupOrigin::SafeFlag));
    assert_eq!(safe.settings, Settings::designed());
    assert_eq!(load(&path).expect("kept"), tuned());
}
