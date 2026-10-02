//! T-442 acceptance: `cs_content`'s declared damage node key is the one
//! shared `cs_types::content::DamageNodeKey`, not a declared-schema look-alike.
//!
//! Task test prefix: `accept_t442_`.
//!
//! The discriminators name the shared `cs_types` type in a function signature,
//! so a reverted declared key fails to compile. One test also reads the key
//! back out of the production `declared_synthetic_airframe_damage` fixture so
//! the check is on declared content, not only on free functions.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_content::damage::{DamageNodeKey, DamageNodeKeyError, declared_synthetic_airframe_damage};
use cs_types::content::{DamageNodeKey as SharedNodeKey, DamageNodeKeyError as SharedNodeKeyError};

fn as_shared_key(key: DamageNodeKey) -> SharedNodeKey {
    key
}

fn as_shared_key_error(error: DamageNodeKeyError) -> SharedNodeKeyError {
    error
}

/// The declared key and its error are the shared `cs_types::content` types.
#[test]
fn accept_t442_declared_damage_key_is_the_shared_key() {
    let key = DamageNodeKey::new("hull").expect("the fixture key is valid");
    assert_eq!(
        as_shared_key(key),
        SharedNodeKey::new("hull").expect("the fixture key is valid"),
        "cs_content's declared node key is cs_types::content::DamageNodeKey"
    );

    let error = DamageNodeKey::new("").expect_err("an empty key is refused");
    let shared = as_shared_key_error(error);
    assert_eq!(
        shared,
        SharedNodeKeyError::Empty,
        "The declared key error is the shared cs_types::content error"
    );
}

/// The production declared fixture hands out the shared key type.
#[test]
fn accept_t442_declared_fixture_nodes_use_the_shared_key() {
    let declared = declared_synthetic_airframe_damage();
    let node = declared
        .nodes()
        .first()
        .expect("the declared synthetic airframe has nodes");
    let shared: SharedNodeKey = as_shared_key(node.key.clone());
    assert_eq!(
        shared.as_str(),
        node.key.as_str(),
        "the declared fixture's node key is the shared key unchanged"
    );
}
