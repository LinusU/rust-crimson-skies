//! Acceptance scenarios F48-A: profile population separation and slot paths.
//! Task test prefix: `accept_f48_a_`.

use std::path::Path;

use cs_app::profile::{ProfileDirError, SessionOrigin, slot_dir};
use cs_types::profile::{ProfileId, ProfileKind};

#[test]
fn accept_f48_a_automation_never_gets_the_production_tree() {
    let id = ProfileId::new(3).expect("id");
    let base = Path::new("/synthetic/base");
    assert_eq!(
        slot_dir(base, SessionOrigin::Automated, ProfileKind::Production, id),
        Err(ProfileDirError::AutomatedProductionAccess)
    );
    let mut seen = Vec::new();
    for kind in ProfileKind::ALL {
        if let Ok(dir) = slot_dir(base, SessionOrigin::Automated, kind, id) {
            assert!(dir.starts_with(base.join(kind.label())));
            seen.push(dir);
        }
    }
    assert_eq!(seen.len(), 3);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 3, "kinds use distinct subtrees");
    let live = slot_dir(
        base,
        SessionOrigin::Interactive,
        ProfileKind::Production,
        id,
    )
    .expect("interactive production");
    assert_eq!(live, base.join("production").join("profile-3"));
}
