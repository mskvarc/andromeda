//! Persistence layer for Entity Maps (clauses 5.2.39 and 5.5.14).

use crate::store_fault::{StoreFault, persistence_failure};
use andromeda_entity_map::error::EntityMapError;

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
/// Classify an adapter failure at the port boundary: backpressure keeps the
/// port's retryable category and every fault becomes its persistence failure.
#[track_caller]
fn persistence_error(error: impl StoreFault) -> EntityMapError {
    EntityMapError::from(persistence_failure(&error))
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
