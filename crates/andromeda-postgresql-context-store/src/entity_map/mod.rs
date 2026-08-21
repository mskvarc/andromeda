//! Persistence layer for Entity Maps (clauses 5.2.39 and 5.5.14).

use andromeda_entity_map::error::EntityMapError;
use std::fmt::Display;

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
fn persistence_error(error: impl Display) -> EntityMapError {
    EntityMapError::Persistence { detail: error.to_string() }
}
