//! Persistence layer for Entity Maps (clauses 5.2.39 and 5.5.14).

use andromeda_entity_map::error::EntityMapError;
use andromeda_postgresql_pool::is_pool_saturated;
use std::{any::Any, fmt::Display};

/// The statements over a map's `entity_map_entries` and `entity_map_links`.
pub mod entries;
/// The statements over the `entity_maps` control record.
pub mod lifecycle;
/// Row-to-domain conversions for Entity Map records.
pub mod mapper;
/// The Entity Map repository struct.
pub mod repo;
pub mod store;

/// Erase an adapter failure into the Entity Map policy's persistence category.
///
/// A `sqlx` pool-acquire timeout is backpressure, not a fault, so it becomes
/// [`EntityMapError::StoreSaturated`] (a 503); every other failure stays a
/// persistence fault. The concrete [`sqlx::Error`] is recovered by downcast
/// because callers hand it in already erased behind `impl Display`.
fn persistence_error(error: impl Display + Any) -> EntityMapError {
    match (&error as &dyn Any).downcast_ref::<sqlx::Error>() {
        Some(sqlx_error) if is_pool_saturated(sqlx_error) => EntityMapError::StoreSaturated,
        Some(_) | None => EntityMapError::Persistence { detail: error.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map::persistence_error;
    use andromeda_entity_map::error::EntityMapError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(persistence_error(sqlx::Error::PoolTimedOut), EntityMapError::StoreSaturated));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), EntityMapError::Persistence { .. }));
    }
}
