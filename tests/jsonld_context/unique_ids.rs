//! Identifiers no other test run ever uses.

use uuid::Uuid;

/// `prefix` followed by a fresh random UUID, so parallel tests and repeated
/// runs never address the same resource.
pub fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}
