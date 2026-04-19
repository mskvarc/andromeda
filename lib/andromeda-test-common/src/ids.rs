//! Identifier allocation for scoped tests.
//!
//! Each [`crate::scope::TestScope`] owns an `id_prefix` — every entity,
//! subscription, CSR, snapshot, or entity-map identifier used inside the
//! scope is derived from that prefix so scoped cleanup can wipe only the
//! rows that belong to the scope.

use uuid::Uuid;

/// Build a globally-unique test-id prefix for a suite.
///
/// The returned prefix is safe to use as an `id LIKE $1 || '%'` cleanup
/// filter: every id minted from the same scope shares the prefix and no
/// other scope will collide with it.
pub fn prefix(suite: &str) -> String {
    let suffix = Uuid::new_v4().simple();
    format!("urn:ngsi-ld:test:{suite}:{suffix}")
}

/// Compose a scoped id by appending `local` to `prefix` with a `:` separator.
///
/// `local` is typically an entity-type label (e.g. `"Building"`, `"Vehicle"`)
/// followed by a per-id discriminator if the test needs multiple instances
/// of the same type.
pub fn compose(prefix: &str, local: &str) -> String {
    format!("{prefix}:{local}")
}

/// Append a random v4 UUID to `base` to produce a single unique id.
///
/// Mirrors the legacy `unique_id` helper. Prefer [`compose`] with a
/// scoped prefix whenever the caller already has a [`crate::scope::TestScope`].
pub fn unique(base: &str) -> String {
    format!("{base}:{}", Uuid::new_v4())
}
