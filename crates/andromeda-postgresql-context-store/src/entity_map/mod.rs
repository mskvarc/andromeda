//! Persistence layer for Entity Maps (clauses 5.2.39 and 5.5.14).

/// The statements over a map's `entity_map_entries` and `entity_map_links`.
pub mod entries;
/// The statements over the `entity_maps` control record.
pub mod lifecycle;
/// Row-to-domain conversions for Entity Map records.
pub mod mapper;
/// The Entity Map repository struct.
pub mod repo;
/// Translation between the operation that created a map and its stored code.
pub mod request_kind_code;
/// The columns recording the request that created a map.
pub mod request_record;
pub mod store;

#[cfg(test)]
mod tests {
    use crate::store_fault::persistence_error;
    use andromeda_entity_map::error::EntityMapError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(
            persistence_error::<EntityMapError>(sqlx::Error::PoolTimedOut),
            EntityMapError::StoreSaturated
        ));
        assert!(matches!(
            persistence_error::<EntityMapError>(sqlx::Error::RowNotFound),
            EntityMapError::Persistence { .. }
        ));
    }
}
